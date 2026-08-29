//! The transmit gate: without a [`TransmitAuthority`] there is no encoder, and
//! without [`TransmitScope::RealBusAttested`] there is no serial port.
//!
//! # What this exists to stop
//!
//! **No frame in this workspace has been verified against this installation's
//! hardware.** Every opcode, payload layout and checksum in [`crate::saturn`]
//! and [`crate::dtv`] is tier `[C]` — third-party reverse engineering, read out
//! of documents that contradict each other in at least eight places. A tier
//! `[C]` frame put on a real RS-485 bus at 9600 baud is an experiment on a
//! valve that moves water at up to 45 °C, and it is an experiment nobody has
//! reviewed.
//!
//! So the daemon must be structurally unable to transmit on a real port, while
//! the emulator path — loopback, PTY, the whole test suite — stays fully
//! usable. That is exactly the split between the two [`TransmitScope`]
//! variants.
//!
//! # The two boundaries
//!
//! The gate is enforced twice, because one check is one mistake away from
//! being deleted:
//!
//! 1. **This crate.** [`crate::saturn::Encoder::new`] and
//!    [`crate::dtv::SteamEncoder::new`] take a `&TransmitAuthority`. No
//!    authority means no encoder, and no encoder means no
//!    [`SaturnFrame`](crate::saturn::SaturnFrame) or
//!    [`DtvFrame`](crate::dtv::DtvFrame) — the only two types the link layer
//!    accepts. Nothing to transmit is a stronger property than nothing
//!    transmitted.
//! 2. **`kdtv-hal`, and that is another agent's work.** `LinkFactory::open`
//!    must refuse a real serial backend unless
//!    [`TransmitAuthority::permits_real_bus_on`] returns true for the link
//!    being opened, and must permit PTY and loopback backends under either
//!    scope. This crate cannot enforce that — it does not and must not depend
//!    on `kdtv-hal` — so what that check needs is exposed here:
//!    [`TransmitAuthority::scope`], [`TransmitAuthority::permits_real_bus`],
//!    [`TransmitAuthority::permits_real_bus_on`],
//!    [`TransmitAuthority::capture_ref`] and
//!    [`TransmitAuthority::fixtures_sha256`] for the boot log.
//!
//! With today's evidence base the daemon builds, boots, runs the whole
//! emulator suite, and cannot open `/dev/ttyUSB0`.
//!
//! # Why the fixture hash is part of the authority
//!
//! [`TransmitScope::RealBusAttested`] requires every allowlisted operation to
//! resolve to a fixture whose [`Provenance`] is
//! [`Captured`](Provenance::Captured) — tier `[A]`, measured on this hardware.
//! That alone would let someone open the gate by editing a JSON file. So
//! configuration must also pin the fixture set's SHA-256, and
//! [`TransmitAuthority::resolve`] refuses if the pin does not match what is
//! compiled in. Promoting a fixture from `[C]` to `[A]` therefore changes the
//! hash, which changes configuration, which is a reviewable, dated commit
//! rather than a flag flip.
//!
//! # `OPEN-01`, polarity
//!
//! Which conductor is A+ is unresolved. The converter's `TA`/`TB` silkscreen is
//! the converter vendor's convention, not Kohler's, and the two Saturn buses
//! were wired by whoever installed them. It is settled per link by measurement
//! at commissioning, and [`PolarityAttestation`] is where that measurement is
//! recorded. An unattested link cannot be driven.

use std::collections::BTreeSet;
use std::fmt;

use kdtv_units::LinkKind;
use serde::{Deserialize, Serialize};

use crate::fixtures::{FixtureId, FixtureSet, Provenance, required_transmit_ids};

/// What this build is allowed to put bytes on.
///
/// Denial is by absence of a variant here too: there is no `RealBusUnattested`,
/// so "transmit on a real port without evidence" is not a state this type can
/// hold.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TransmitScope {
    /// Loopback and PTY links only.
    ///
    /// The encoders still produce frames, so the entire emulator path works
    /// with today's tier `[C]` fixtures. This is the only scope reachable from
    /// the committed evidence base.
    EmulatorOnly,

    /// Real serial ports permitted.
    ///
    /// Requires that every operation in both allowlists resolve to a fixture
    /// with [`Provenance::Captured`] — that is, that Phase 1 capture has closed
    /// — that configuration pins the fixture-set hash, and that every link's
    /// bus polarity has been attested.
    RealBusAttested {
        /// The capture campaign this attestation rests on: a path under
        /// `research/diagnostics/` with the date in the filename, per
        /// `AGENT.md` § Conventions.
        capture_ref: String,
        /// Per-link measured polarity. `OPEN-01`.
        polarity: PolarityAttestation,
    },
}

impl fmt::Display for TransmitScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmulatorOnly => f.write_str("emulator-only"),
            Self::RealBusAttested { capture_ref, .. } => {
                write!(f, "real-bus-attested against {capture_ref}")
            }
        }
    }
}

/// The measured polarity of one link's RS-485 pair.
///
/// `ARCHITECTURE.md` § 3.1 keys this on `ZoneId`. It is keyed on [`LinkKind`]
/// here because the DTV+ steam link is the same physical layer with the same
/// unresolved polarity, and a map over zones alone would leave that link
/// attested by nobody. `[I]` — that the steam link's polarity is equally
/// unresolved is inference from it being RS-485 on the same converter family,
/// not a statement from a source.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolarityNote {
    /// The link this measurement was taken on.
    pub link: LinkKind,
    /// What was measured, in the commissioning engineer's words — which
    /// conductor carries A+, at which terminal, verified how.
    pub note: String,
    /// The date the measurement was taken, so a stale attestation is visible.
    /// Free text; this crate does not parse dates.
    pub attested_on: String,
}

impl PolarityNote {
    /// True when the note carries an actual measurement rather than a
    /// placeholder. An empty note is not an attestation.
    #[must_use]
    pub fn is_attested(&self) -> bool {
        !self.note.trim().is_empty() && !self.attested_on.trim().is_empty()
    }
}

/// Every link's measured polarity. `OPEN-01`.
///
/// A list rather than a map because [`LinkKind::Zone`] is a newtype variant and
/// so has no string form to be a JSON or TOML key.
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolarityAttestation {
    /// One note per link. Order is not significant; duplicates are not
    /// rejected, and the first note for a link wins.
    #[serde(default)]
    pub notes: Vec<PolarityNote>,
}

impl PolarityAttestation {
    /// The note for one link, if it carries a measurement.
    #[must_use]
    pub fn note(&self, link: LinkKind) -> Option<&PolarityNote> {
        self.notes
            .iter()
            .find(|n| n.link == link && n.is_attested())
    }

    /// True when this link has been measured.
    #[must_use]
    pub fn attests(&self, link: LinkKind) -> bool {
        self.note(link).is_some()
    }

    /// The links that have not been measured, in [`LinkKind::ALL`] order.
    #[must_use]
    pub fn unattested(&self) -> Vec<LinkKind> {
        LinkKind::ALL
            .into_iter()
            .filter(|l| !self.attests(*l))
            .collect()
    }
}

/// The scope configuration asks for, before any evidence is consulted.
///
/// Separate from [`TransmitScope`] on purpose: a config file can *ask* for a
/// real bus, and asking is not the same as being granted one.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestedScope {
    /// The default, and the only scope today's evidence base can grant.
    #[default]
    EmulatorOnly,
    /// Checked against the fixture set by [`TransmitAuthority::resolve`].
    RealBusAttested,
}

/// The transmit-gate section of the daemon's configuration.
///
/// Deserializable, because it comes from a file. [`TransmitAuthority`] is not,
/// because it must only ever come from [`TransmitAuthority::resolve`].
#[derive(Clone, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransmitGateConfig {
    /// What is being asked for.
    #[serde(default)]
    pub scope: RequestedScope,

    /// The capture campaign backing a [`RequestedScope::RealBusAttested`]
    /// request. Required for that scope, ignored for
    /// [`RequestedScope::EmulatorOnly`].
    #[serde(default)]
    pub capture_ref: Option<String>,

    /// Per-link measured polarity. `OPEN-01`.
    #[serde(default)]
    pub polarity: PolarityAttestation,

    /// The SHA-256 of the fixture set this configuration was reviewed against,
    /// lowercase hex.
    ///
    /// Optional under [`RequestedScope::EmulatorOnly`], where a developer
    /// editing fixtures should not have to re-pin a hash to run the emulator.
    /// **Required** under [`RequestedScope::RealBusAttested`]: without the pin,
    /// promoting a fixture to tier `[A]` would open the gate with no second
    /// signature.
    #[serde(default)]
    pub expected_fixtures_sha256: Option<String>,
}

impl TransmitGateConfig {
    /// The configuration the repository ships with, and the one CI asserts
    /// against: emulator only, nothing pinned, nothing attested.
    #[must_use]
    pub fn emulator_only() -> Self {
        Self::default()
    }
}

/// Proof that this build is allowed to encode, and possibly to transmit.
///
/// Private fields, no public constructor, no `Deserialize`. The only routes to
/// a value are [`TransmitAuthority::resolve`], which consults the evidence, and
/// [`TransmitAuthority::emulator_only`], which structurally cannot produce
/// [`TransmitScope::RealBusAttested`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TransmitAuthority {
    scope: TransmitScope,
    fixtures_sha256: [u8; 32],
}

impl TransmitAuthority {
    /// Grants exactly the emulator scope, whatever the fixture set says.
    ///
    /// This is the constructor the emulator, the unit tests and the property
    /// tests use. It takes the fixture set only to record which set was in
    /// force, and it has no parameter that could ask for more.
    #[must_use]
    pub fn emulator_only(fixtures: &FixtureSet) -> Self {
        Self {
            scope: TransmitScope::EmulatorOnly,
            fixtures_sha256: fixtures.sha256(),
        }
    }

    /// Resolves configuration against the compiled-in evidence.
    ///
    /// [`RequestedScope::EmulatorOnly`] is granted without consulting fixture
    /// provenance at all — an emulator does not need attested frames, and
    /// requiring them would make the test suite unrunnable.
    ///
    /// [`RequestedScope::RealBusAttested`] is granted only when all four of
    /// these hold, and the error names whichever failed:
    ///
    /// 1. configuration pins a fixture-set hash and it matches;
    /// 2. configuration names the capture campaign;
    /// 3. every operation in [`crate::saturn::SaturnOp::ALL`] and
    ///    [`crate::dtv::SteamOp::ALL`] resolves to a fixture that exists and is
    ///    [`Provenance::Captured`];
    /// 4. every link in [`LinkKind::ALL`] has an attested bus polarity.
    ///
    /// Today condition 3 fails: every fixture is tier `[C]`.
    pub fn resolve(cfg: &TransmitGateConfig, fixtures: &FixtureSet) -> Result<Self, GateError> {
        let found = fixtures.sha256();

        // The pin is checked in both scopes when it is present, so a stale pin
        // is reported the first time the daemon boots after a fixture edit and
        // not months later at commissioning.
        if let Some(expected_hex) = cfg.expected_fixtures_sha256.as_deref() {
            let expected = parse_sha256_hex(expected_hex)?;
            if expected != found {
                return Err(GateError::FixtureSetHashMismatch {
                    expected: hex::encode(expected),
                    found: hex::encode(found),
                });
            }
        }

        let scope = match cfg.scope {
            RequestedScope::EmulatorOnly => TransmitScope::EmulatorOnly,
            RequestedScope::RealBusAttested => {
                if cfg.expected_fixtures_sha256.is_none() {
                    return Err(GateError::FixtureSetHashUnpinned {
                        found: hex::encode(found),
                    });
                }

                let capture_ref = cfg
                    .capture_ref
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or(GateError::MissingCaptureRef)?
                    .to_owned();

                let (documented, missing) = unattested_fixtures(fixtures);
                if !documented.is_empty() || !missing.is_empty() {
                    return Err(GateError::FixturesNotCaptured {
                        documented,
                        missing,
                    });
                }

                let links = cfg.polarity.unattested();
                if !links.is_empty() {
                    return Err(GateError::BusPolarityUnattested { links });
                }

                TransmitScope::RealBusAttested {
                    capture_ref,
                    polarity: cfg.polarity.clone(),
                }
            }
        };

        Ok(Self {
            scope,
            fixtures_sha256: found,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> &TransmitScope {
        &self.scope
    }

    /// The fixture set this authority was resolved against. `kdtv-hal` and the
    /// boot log record it so a support transcript says which evidence base was
    /// in force.
    #[must_use]
    pub const fn fixtures_sha256(&self) -> [u8; 32] {
        self.fixtures_sha256
    }

    #[must_use]
    pub fn fixtures_sha256_hex(&self) -> String {
        hex::encode(self.fixtures_sha256)
    }

    /// True when a real serial port may be opened at all.
    ///
    /// The second gate boundary, in `kdtv-hal`, is the caller of this. Under
    /// [`TransmitScope::EmulatorOnly`] it is false and a `Backend::Serial` open
    /// must fail; PTY and loopback backends open under either scope.
    #[must_use]
    pub const fn permits_real_bus(&self) -> bool {
        matches!(self.scope, TransmitScope::RealBusAttested { .. })
    }

    /// True when a real serial port may be opened **for this link**.
    ///
    /// Polarity is measured per link, so authority is granted per link. A
    /// second zone commissioned later does not ride in on the first zone's
    /// attestation.
    #[must_use]
    pub fn permits_real_bus_on(&self, link: LinkKind) -> bool {
        match &self.scope {
            TransmitScope::EmulatorOnly => false,
            TransmitScope::RealBusAttested { polarity, .. } => polarity.attests(link),
        }
    }

    /// The capture campaign backing this authority, or `None` under
    /// [`TransmitScope::EmulatorOnly`].
    #[must_use]
    pub fn capture_ref(&self) -> Option<&str> {
        match &self.scope {
            TransmitScope::EmulatorOnly => None,
            TransmitScope::RealBusAttested { capture_ref, .. } => Some(capture_ref.as_str()),
        }
    }

    /// The per-link polarity measurements, or `None` under
    /// [`TransmitScope::EmulatorOnly`].
    #[must_use]
    pub fn polarity(&self) -> Option<&PolarityAttestation> {
        match &self.scope {
            TransmitScope::EmulatorOnly => None,
            TransmitScope::RealBusAttested { polarity, .. } => Some(polarity),
        }
    }
}

/// Every allowlisted operation whose fixture is not tier `[A]`, split by why.
///
/// Returned as two lists because the two states need different work: a
/// `documented` fixture needs a capture to promote it, a `missing` one needs a
/// fixture written first.
fn unattested_fixtures(fixtures: &FixtureSet) -> (Vec<FixtureId>, Vec<FixtureId>) {
    let mut documented = BTreeSet::new();
    let mut missing = BTreeSet::new();

    for id in required_transmit_ids() {
        match fixtures.get(id) {
            None => {
                missing.insert(id.to_owned());
            }
            Some(f) => {
                if !matches!(f.provenance(), Provenance::Captured { .. }) {
                    documented.insert(f.id().clone());
                }
            }
        }
    }

    let missing = missing
        .into_iter()
        .filter_map(|s| FixtureId::new(s).ok())
        .collect();
    (documented.into_iter().collect(), missing)
}

fn parse_sha256_hex(s: &str) -> Result<[u8; 32], GateError> {
    let bytes = hex::decode(s.trim()).map_err(|_| GateError::MalformedExpectedHash {
        value: s.to_owned(),
    })?;
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| GateError::MalformedExpectedHash {
        value: s.to_owned(),
    })
}

/// Why the gate stayed shut.
///
/// Every variant names what would have to change, because the answer to "the
/// daemon will not talk to my valve" must be a fact and not a shrug.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum GateError {
    /// Configuration claims attestation but the evidence is still tier `[C]`.
    /// **This is today's state**, and the test suite pins it.
    #[error(
        "real-bus transmit refused: {} allowlisted operation(s) still rest on tier [C] \
         documentation ({}) and {} have no fixture at all ({}); \
         capture them (Phase 1) and promote them before opening the gate",
        documented.len(),
        join_ids(documented),
        missing.len(),
        join_ids(missing),
    )]
    FixturesNotCaptured {
        /// Fixtures that exist but are [`Provenance::Documented`].
        documented: Vec<FixtureId>,
        /// Operations with no fixture in the set at all.
        missing: Vec<FixtureId>,
    },

    /// The compiled-in fixture set is not the one configuration was reviewed
    /// against. Either the fixtures changed or the pin is stale; both need a
    /// human.
    #[error("fixture set hash mismatch: configuration pins {expected}, this build embeds {found}")]
    FixtureSetHashMismatch {
        /// The hash configuration pinned, lowercase hex.
        expected: String,
        /// The hash of the compiled-in set, lowercase hex.
        found: String,
    },

    /// A real bus was requested without pinning a fixture-set hash, which would
    /// make promoting a fixture a one-line unreviewed change.
    #[error(
        "real-bus transmit refused: configuration does not pin a fixture-set hash; \
         set expected_fixtures_sha256 = \"{found}\" in the same reviewed commit that \
         promotes the fixtures"
    )]
    FixtureSetHashUnpinned {
        /// The hash of the compiled-in set, offered so the pin can be written.
        found: String,
    },

    /// `expected_fixtures_sha256` is not 32 bytes of hex.
    #[error("expected_fixtures_sha256 is not 64 hex characters: {value:?}")]
    MalformedExpectedHash {
        /// The value as configured.
        value: String,
    },

    /// A real bus was requested without naming the capture campaign it rests
    /// on.
    #[error(
        "real-bus transmit refused: configuration names no capture_ref; \
         attestation must point at the capture under research/diagnostics/ that backs it"
    )]
    MissingCaptureRef,

    /// `OPEN-01`: a link's RS-485 polarity has not been measured.
    #[error(
        "real-bus transmit refused: bus polarity unattested on {}; \
         which conductor is A+ is unresolved and is settled per link by measurement \
         at commissioning",
        join_links(links)
    )]
    BusPolarityUnattested {
        /// The links with no usable [`PolarityNote`].
        links: Vec<LinkKind>,
    },
}

fn join_ids(ids: &[FixtureId]) -> String {
    if ids.is_empty() {
        return "none".to_owned();
    }
    ids.iter()
        .map(FixtureId::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

fn join_links(links: &[LinkKind]) -> String {
    if links.is_empty() {
        return "none".to_owned();
    }
    links
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{Fixture, FixtureDirection};
    use kdtv_units::ZoneId;

    fn attest_all() -> PolarityAttestation {
        PolarityAttestation {
            notes: LinkKind::ALL
                .into_iter()
                .map(|link| PolarityNote {
                    link,
                    note: "synthetic: A+ on the terminal marked TA".to_owned(),
                    attested_on: "2026-01-01".to_owned(),
                })
                .collect(),
        }
    }

    /// A fixture set in which every allowlisted operation is tier `[A]`.
    /// Synthetic — it exists so the *granting* half of the gate is tested too,
    /// and its bytes are deliberately not real frames.
    fn all_captured() -> FixtureSet {
        let fixtures = required_transmit_ids()
            .map(|id| {
                Fixture::synthetic_captured(
                    FixtureId::new(id).unwrap(),
                    LinkKind::Zone(ZoneId::Zone1),
                    FixtureDirection::ToDevice,
                    vec![0xAA, 0x55],
                )
            })
            .collect();
        FixtureSet::from_fixtures(fixtures).unwrap()
    }

    fn attested_cfg(expected: &FixtureSet) -> TransmitGateConfig {
        TransmitGateConfig {
            scope: RequestedScope::RealBusAttested,
            capture_ref: Some("research/diagnostics/2026-09-01-phase1-saturn.bin".to_owned()),
            polarity: attest_all(),
            expected_fixtures_sha256: Some(expected.sha256_hex()),
        }
    }

    // ---- The committed state ----------------------------------------------

    /// **The test CI runs forever.** With the fixture set this repository
    /// actually ships, a request for a real bus is refused and the error names
    /// the fixtures standing in the way.
    ///
    /// If this test ever fails, either Phase 1 capture has landed and the
    /// fixtures were promoted deliberately — in which case this test is
    /// rewritten in the same reviewed commit — or the gate has been weakened by
    /// accident.
    #[test]
    fn the_gate_is_closed_in_the_committed_state() {
        let fx = FixtureSet::embedded();
        let cfg = attested_cfg(fx);

        let err = TransmitAuthority::resolve(&cfg, fx).unwrap_err();
        let GateError::FixturesNotCaptured {
            documented,
            missing,
        } = err
        else {
            panic!("expected FixturesNotCaptured, got {err:?}");
        };

        // Every required operation is accounted for, and every one of them is
        // still tier [C]. Nothing is missing: the fixture set covers the whole
        // allowlist already, so the only thing between here and a real bus is
        // evidence.
        assert_eq!(missing, Vec::<FixtureId>::new());
        assert_eq!(documented.len(), required_transmit_ids().count());
        for id in required_transmit_ids() {
            assert!(
                documented.iter().any(|d| d.as_str() == id),
                "{id} is not named in the refusal"
            );
        }
    }

    /// The refusal is legible: it names the operations, not just a count.
    #[test]
    fn the_refusal_names_the_fixtures_in_the_way() {
        let fx = FixtureSet::embedded();
        let err = TransmitAuthority::resolve(&attested_cfg(fx), fx).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("saturn.set_outlets"), "{text}");
        assert!(text.contains("steam.start"), "{text}");
        assert!(text.contains("tier [C]"), "{text}");
    }

    /// Nothing about today's evidence base stops the emulator.
    #[test]
    fn emulator_only_resolves_against_the_committed_fixtures() {
        let fx = FixtureSet::embedded();
        let auth = TransmitAuthority::resolve(&TransmitGateConfig::emulator_only(), fx).unwrap();
        assert_eq!(auth.scope(), &TransmitScope::EmulatorOnly);
        assert!(!auth.permits_real_bus());
        for link in LinkKind::ALL {
            assert!(!auth.permits_real_bus_on(link));
        }
        assert_eq!(auth.fixtures_sha256(), fx.sha256());
        assert_eq!(auth.capture_ref(), None);
        assert_eq!(auth.polarity(), None);
    }

    /// The emulator scope does not consult fixture provenance, so an empty
    /// fixture set still runs the emulator. Only the real bus needs evidence.
    #[test]
    fn emulator_only_does_not_require_any_fixture() {
        let empty = FixtureSet::from_fixtures(Vec::new()).unwrap();
        let auth =
            TransmitAuthority::resolve(&TransmitGateConfig::emulator_only(), &empty).unwrap();
        assert!(!auth.permits_real_bus());
    }

    // ---- The granting half -------------------------------------------------

    /// With a synthetic all-`[A]` set the gate opens, which is what makes the
    /// closed case above evidence about the fixtures rather than evidence that
    /// the code always refuses.
    #[test]
    fn a_fully_captured_fixture_set_resolves_to_a_real_bus() {
        let fx = all_captured();
        let auth = TransmitAuthority::resolve(&attested_cfg(&fx), &fx).unwrap();
        assert!(auth.permits_real_bus());
        for link in LinkKind::ALL {
            assert!(auth.permits_real_bus_on(link), "{link}");
        }
        assert_eq!(
            auth.capture_ref(),
            Some("research/diagnostics/2026-09-01-phase1-saturn.bin")
        );
        assert_eq!(auth.fixtures_sha256_hex(), fx.sha256_hex());
    }

    /// One operation left at tier `[C]` closes the gate for the whole system.
    /// There is no per-operation attestation: a valve bus is either understood
    /// or it is not.
    #[test]
    fn one_documented_fixture_closes_the_gate() {
        let mut fixtures: Vec<Fixture> = all_captured().iter().cloned().collect();
        let victim = fixtures
            .iter_mut()
            .find(|f| f.id().as_str() == "saturn.set_temperature")
            .unwrap();
        victim.set_provenance_for_test(Provenance::Documented {
            source: "research/xagon0/docs/protocols/saturn-protocol.md".to_owned(),
            section: "§ 6".to_owned(),
        });
        let fx = FixtureSet::from_fixtures(fixtures).unwrap();

        let err = TransmitAuthority::resolve(&attested_cfg(&fx), &fx).unwrap_err();
        assert!(matches!(
            err,
            GateError::FixturesNotCaptured { ref documented, ref missing }
                if documented.len() == 1 && missing.is_empty()
        ));
        assert!(err.to_string().contains("saturn.set_temperature"));
    }

    /// Deleting a fixture is not a way to satisfy "every fixture is captured".
    #[test]
    fn a_missing_fixture_closes_the_gate_too() {
        let fixtures: Vec<Fixture> = all_captured()
            .iter()
            .filter(|f| f.id().as_str() != "steam.stop")
            .cloned()
            .collect();
        let fx = FixtureSet::from_fixtures(fixtures).unwrap();

        let err = TransmitAuthority::resolve(&attested_cfg(&fx), &fx).unwrap_err();
        let GateError::FixturesNotCaptured { missing, .. } = &err else {
            panic!("expected FixturesNotCaptured, got {err:?}");
        };
        assert_eq!(missing.len(), 1);
        assert_eq!(missing.first().map(FixtureId::as_str), Some("steam.stop"));
    }

    // ---- The hash pin ------------------------------------------------------

    /// Promoting a fixture changes the set hash, so the pin in configuration
    /// stops matching and the daemon refuses to boot until a human re-pins it.
    /// That is the whole point of the pin.
    #[test]
    fn promoting_a_fixture_invalidates_a_stale_pin() {
        let before = FixtureSet::embedded();
        let after = all_captured();
        assert_ne!(before.sha256(), after.sha256());

        let stale = TransmitGateConfig {
            expected_fixtures_sha256: Some(before.sha256_hex()),
            ..attested_cfg(&after)
        };
        let err = TransmitAuthority::resolve(&stale, &after).unwrap_err();
        assert!(matches!(err, GateError::FixtureSetHashMismatch { .. }));
    }

    /// The pin is checked under the emulator scope too, when it is set — so a
    /// stale pin surfaces on a developer's machine and not at commissioning.
    #[test]
    fn a_stale_pin_is_reported_under_the_emulator_scope_as_well() {
        let fx = FixtureSet::embedded();
        let cfg = TransmitGateConfig {
            expected_fixtures_sha256: Some(hex::encode([0u8; 32])),
            ..TransmitGateConfig::emulator_only()
        };
        assert!(matches!(
            TransmitAuthority::resolve(&cfg, fx),
            Err(GateError::FixtureSetHashMismatch { .. })
        ));
    }

    /// An unpinned real-bus request is refused even when every fixture is
    /// captured, and the refusal hands over the hash to pin.
    #[test]
    fn a_real_bus_request_without_a_pin_is_refused() {
        let fx = all_captured();
        let cfg = TransmitGateConfig {
            expected_fixtures_sha256: None,
            ..attested_cfg(&fx)
        };
        let err = TransmitAuthority::resolve(&cfg, &fx).unwrap_err();
        assert_eq!(
            err,
            GateError::FixtureSetHashUnpinned {
                found: fx.sha256_hex()
            }
        );
        assert!(err.to_string().contains(&fx.sha256_hex()));
    }

    #[test]
    fn a_malformed_pin_is_refused_rather_than_ignored() {
        let fx = FixtureSet::embedded();
        for bad in ["", "zz", "abcd"] {
            let cfg = TransmitGateConfig {
                expected_fixtures_sha256: Some(bad.to_owned()),
                ..TransmitGateConfig::emulator_only()
            };
            assert!(
                matches!(
                    TransmitAuthority::resolve(&cfg, fx),
                    Err(GateError::MalformedExpectedHash { .. })
                ),
                "{bad:?} was accepted"
            );
        }
    }

    // ---- Capture reference and polarity ------------------------------------

    #[test]
    fn a_real_bus_request_without_a_capture_reference_is_refused() {
        let fx = all_captured();
        for capture_ref in [None, Some(String::new()), Some("   ".to_owned())] {
            let cfg = TransmitGateConfig {
                capture_ref,
                ..attested_cfg(&fx)
            };
            assert_eq!(
                TransmitAuthority::resolve(&cfg, &fx),
                Err(GateError::MissingCaptureRef)
            );
        }
    }

    /// `OPEN-01`. Evidence about frames says nothing about which wire is A+.
    #[test]
    fn captured_frames_do_not_attest_bus_polarity() {
        let fx = all_captured();
        let cfg = TransmitGateConfig {
            polarity: PolarityAttestation::default(),
            ..attested_cfg(&fx)
        };
        let err = TransmitAuthority::resolve(&cfg, &fx).unwrap_err();
        assert_eq!(
            err,
            GateError::BusPolarityUnattested {
                links: LinkKind::ALL.to_vec()
            }
        );
        assert!(err.to_string().contains("zone1"));
        assert!(err.to_string().contains("steam"));
    }

    /// A placeholder is not a measurement.
    #[test]
    fn an_empty_polarity_note_does_not_attest() {
        let note = PolarityNote {
            link: LinkKind::Steam,
            note: "  ".to_owned(),
            attested_on: "2026-01-01".to_owned(),
        };
        assert!(!note.is_attested());
        let undated = PolarityNote {
            note: "A+ on TA".to_owned(),
            attested_on: String::new(),
            ..note.clone()
        };
        assert!(!undated.is_attested());
        let good = PolarityNote {
            attested_on: "2026-01-01".to_owned(),
            ..undated
        };
        assert!(good.is_attested());
    }

    /// Commissioning one zone does not authorise the other. Polarity is
    /// measured per link, so it is granted per link.
    #[test]
    fn one_zone_does_not_ride_in_on_another_zones_attestation() {
        let fx = all_captured();
        let mut polarity = attest_all();
        polarity.notes.retain(|n| n.link != LinkKind::Steam);
        let cfg = TransmitGateConfig {
            polarity,
            ..attested_cfg(&fx)
        };
        // Resolution refuses outright while any link is unattested...
        assert!(matches!(
            TransmitAuthority::resolve(&cfg, &fx),
            Err(GateError::BusPolarityUnattested { .. })
        ));

        // ...and even given an authority, the per-link check is what kdtv-hal
        // must consult, not the coarse one.
        let auth = TransmitAuthority::resolve(&attested_cfg(&fx), &fx).unwrap();
        let TransmitScope::RealBusAttested { polarity, .. } = auth.scope() else {
            panic!("expected a real-bus scope");
        };
        assert_eq!(polarity.unattested(), Vec::new());
    }

    // ---- The type itself ---------------------------------------------------

    /// [`TransmitAuthority::emulator_only`] has no parameter that could ask for
    /// more, so the convenience constructor the whole test suite uses cannot
    /// become a back door.
    #[test]
    fn the_convenience_constructor_cannot_grant_a_real_bus() {
        let auth = TransmitAuthority::emulator_only(FixtureSet::embedded());
        assert!(!auth.permits_real_bus());
        assert_eq!(auth.scope(), &TransmitScope::EmulatorOnly);
    }

    #[test]
    fn the_scope_prints_its_evidence() {
        assert_eq!(TransmitScope::EmulatorOnly.to_string(), "emulator-only");
        let scope = TransmitScope::RealBusAttested {
            capture_ref: "capture.bin".to_owned(),
            polarity: attest_all(),
        };
        assert_eq!(scope.to_string(), "real-bus-attested against capture.bin");
    }

    #[test]
    fn the_shipped_configuration_is_emulator_only() {
        let cfg = TransmitGateConfig::emulator_only();
        assert_eq!(cfg.scope, RequestedScope::EmulatorOnly);
        assert_eq!(cfg.capture_ref, None);
        assert_eq!(cfg.expected_fixtures_sha256, None);
        assert_eq!(cfg.polarity, PolarityAttestation::default());
        assert_eq!(RequestedScope::default(), RequestedScope::EmulatorOnly);
    }

    /// Configuration round-trips through JSON, because it comes from a file.
    /// [`TransmitAuthority`] deliberately does not: there is no `Deserialize`
    /// for it, so no file can mint one.
    #[test]
    fn the_configuration_round_trips_but_the_authority_has_no_deserialize() {
        let fx = all_captured();
        let cfg = attested_cfg(&fx);
        let text = serde_json::to_string(&cfg).unwrap();
        let back: TransmitGateConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(back, cfg);

        // An unknown key is a typo in a safety-relevant file, not a default.
        let bad = r#"{"scope":"emulator_only","scoope":true}"#;
        assert!(serde_json::from_str::<TransmitGateConfig>(bad).is_err());

        // And the scope spelling is the one the config file uses.
        assert_eq!(
            serde_json::to_string(&RequestedScope::RealBusAttested).unwrap(),
            "\"real_bus_attested\""
        );
    }
}
