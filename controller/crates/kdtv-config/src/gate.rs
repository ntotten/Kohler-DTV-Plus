//! The transmit gate's **configuration**, and only its configuration.
//!
//! Two crates share this subject and the split is deliberate:
//!
//! - This module parses and validates the *shape* of the operator's claim: the
//!   scope, the capture it rests on, the fixture-set hash it was taken at, and
//!   an A/B polarity attestation for every configured link.
//! - `kdtv_proto`'s gate decides whether that claim is *true* — whether the
//!   fixtures named by the hash are actually tier `[A]`, and therefore whether a
//!   `TransmitAuthority` exists at all.
//!
//! A configuration that says `scope = "real-bus-attested"` is a request, not a
//! grant. Nothing in this crate mints an authority, and nothing here can open a
//! port.
//!
//! Polarity is in the configuration rather than in code because it is a
//! measurement of the physical installation and there is no way to derive it. A
//! reversed A/B pair is silent — the link simply never answers — and on a bus
//! that drives water, "it never answered" is not a diagnosis anyone should have
//! to reach twice.

use crate::error::ConfigError;
use kdtv_units::{LinkKind, ZoneId};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;

/// How far the operator claims the evidence reaches.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum GateScope {
    /// Emulated and pseudo-terminal links only. The committed position, and the
    /// only one every fixture in this repository supports today.
    EmulatorOnly,
    /// A real bus, on the strength of a named capture and a fixture set
    /// promoted to tier `[A]`.
    RealBusAttested,
}

impl GateScope {
    #[must_use]
    pub const fn is_real_bus(self) -> bool {
        matches!(self, Self::RealBusAttested)
    }
}

impl fmt::Display for GateScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmulatorOnly => "emulator-only",
            Self::RealBusAttested => "real-bus-attested",
        })
    }
}

/// The validated gate configuration.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TransmitGateConfig {
    scope: GateScope,
    capture_ref: Option<String>,
    fixtures_sha256: Option<String>,
    polarity: BTreeMap<LinkKind, String>,
}

impl TransmitGateConfig {
    /// The committed position: emulator only, no attestation.
    #[must_use]
    pub fn emulator_only() -> Self {
        Self {
            scope: GateScope::EmulatorOnly,
            capture_ref: None,
            fixtures_sha256: None,
            polarity: BTreeMap::new(),
        }
    }

    /// Validates the shape of the claim against the links this configuration
    /// actually has.
    ///
    /// `links` is every link the service will drive — both zones, and the steam
    /// link when steam is enabled.
    pub(crate) fn validate(
        scope: GateScope,
        capture_ref: Option<String>,
        fixtures_sha256: Option<String>,
        polarity: BTreeMap<LinkKind, String>,
        links: &[LinkKind],
    ) -> Result<Self, ConfigError> {
        match scope {
            GateScope::EmulatorOnly => {
                // Half-filled attestation under a closed gate reads, at a
                // glance, as a gate that is open. Refuse rather than ignore.
                if capture_ref.is_some() {
                    return Err(ConfigError::GateAttestationUnused {
                        field: "capture_ref",
                    });
                }
                if fixtures_sha256.is_some() {
                    return Err(ConfigError::GateAttestationUnused {
                        field: "fixtures_sha256",
                    });
                }
                if !polarity.is_empty() {
                    return Err(ConfigError::GateAttestationUnused { field: "polarity" });
                }
            }
            GateScope::RealBusAttested => {
                let capture =
                    capture_ref
                        .as_deref()
                        .ok_or(ConfigError::GateAttestationMissing {
                            field: "capture_ref",
                        })?;
                if capture.trim().is_empty() {
                    return Err(ConfigError::GateCaptureRefEmpty);
                }
                let hash =
                    fixtures_sha256
                        .as_deref()
                        .ok_or(ConfigError::GateAttestationMissing {
                            field: "fixtures_sha256",
                        })?;
                if !is_sha256(hash) {
                    return Err(ConfigError::GateFixtureHash {
                        value: hash.to_owned(),
                    });
                }
                if polarity.is_empty() {
                    return Err(ConfigError::GateAttestationMissing { field: "polarity" });
                }
                for link in links {
                    let text =
                        polarity
                            .get(link)
                            .ok_or_else(|| ConfigError::GatePolarityMissing {
                                link: link.to_string(),
                            })?;
                    if text.trim().is_empty() {
                        return Err(ConfigError::GatePolarityEmpty {
                            link: link.to_string(),
                        });
                    }
                }
                for link in polarity.keys() {
                    if !links.contains(link) {
                        return Err(ConfigError::GatePolarityUnknownLink {
                            link: link.to_string(),
                        });
                    }
                }
            }
        }
        Ok(Self {
            scope,
            capture_ref,
            fixtures_sha256,
            polarity,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> GateScope {
        self.scope
    }

    /// The capture the claim rests on, as a repository-relative path. Present
    /// only under [`GateScope::RealBusAttested`].
    #[must_use]
    pub fn capture_ref(&self) -> Option<&str> {
        self.capture_ref.as_deref()
    }

    /// The fixture-set hash the claim was made at.
    ///
    /// It changes whenever a fixture is promoted, which is what makes opening
    /// the gate a dated, reviewable act rather than a flag someone flips.
    /// Whether the fixtures behind it are genuinely tier `[A]` is
    /// `kdtv_proto`'s question, not this crate's.
    #[must_use]
    pub fn fixtures_sha256(&self) -> Option<&str> {
        self.fixtures_sha256.as_deref()
    }

    /// The measured A/B polarity note for one link.
    #[must_use]
    pub fn polarity(&self, link: LinkKind) -> Option<&str> {
        self.polarity.get(&link).map(String::as_str)
    }

    /// Every attested link, ascending.
    pub fn attested_links(&self) -> impl Iterator<Item = LinkKind> + '_ {
        self.polarity.keys().copied()
    }
}

/// The polarity table's keys, as the file spells them.
pub(crate) fn polarity_link(name: &str) -> Option<LinkKind> {
    match name {
        "zone1" => Some(LinkKind::Zone(ZoneId::Zone1)),
        "zone2" => Some(LinkKind::Zone(ZoneId::Zone2)),
        "steam" => Some(LinkKind::Steam),
        _ => None,
    }
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn zones() -> Vec<LinkKind> {
        vec![LinkKind::Zone(ZoneId::Zone1), LinkKind::Zone(ZoneId::Zone2)]
    }

    fn attested() -> BTreeMap<LinkKind, String> {
        zones()
            .into_iter()
            .map(|l| (l, "A+ = converter TA, measured 2026-01-01".to_owned()))
            .collect()
    }

    #[test]
    fn the_committed_position_validates() {
        let g = TransmitGateConfig::validate(
            GateScope::EmulatorOnly,
            None,
            None,
            BTreeMap::new(),
            &zones(),
        )
        .unwrap();
        assert_eq!(g.scope(), GateScope::EmulatorOnly);
        assert!(!g.scope().is_real_bus());
        assert_eq!(g.capture_ref(), None);
        assert_eq!(g.fixtures_sha256(), None);
        assert_eq!(g.attested_links().count(), 0);
        assert_eq!(g, TransmitGateConfig::emulator_only());
    }

    #[test]
    fn attestation_under_a_closed_gate_is_refused() {
        for (capture, hash, polarity, field) in [
            (
                Some("research/diagnostics/x.bin".to_owned()),
                None,
                BTreeMap::new(),
                "capture_ref",
            ),
            (
                None,
                Some(HASH.to_owned()),
                BTreeMap::new(),
                "fixtures_sha256",
            ),
            (None, None, attested(), "polarity"),
        ] {
            let err = TransmitGateConfig::validate(
                GateScope::EmulatorOnly,
                capture,
                hash,
                polarity,
                &zones(),
            )
            .unwrap_err();
            assert!(err.to_string().contains(field), "{err}");
        }
    }

    #[test]
    fn a_real_bus_claim_needs_all_of_it() {
        let full = TransmitGateConfig::validate(
            GateScope::RealBusAttested,
            Some("research/diagnostics/2026-01-01-saturn-zone1.bin".to_owned()),
            Some(HASH.to_owned()),
            attested(),
            &zones(),
        )
        .unwrap();
        assert!(full.scope().is_real_bus());
        assert_eq!(full.fixtures_sha256(), Some(HASH));
        assert!(
            full.polarity(LinkKind::Zone(ZoneId::Zone1))
                .is_some_and(|s| s.contains("TA"))
        );

        // Each piece removed in turn.
        assert!(matches!(
            TransmitGateConfig::validate(
                GateScope::RealBusAttested,
                None,
                Some(HASH.to_owned()),
                attested(),
                &zones()
            ),
            Err(ConfigError::GateAttestationMissing {
                field: "capture_ref"
            })
        ));
        assert!(matches!(
            TransmitGateConfig::validate(
                GateScope::RealBusAttested,
                Some("x".to_owned()),
                None,
                attested(),
                &zones()
            ),
            Err(ConfigError::GateAttestationMissing {
                field: "fixtures_sha256"
            })
        ));
        assert!(matches!(
            TransmitGateConfig::validate(
                GateScope::RealBusAttested,
                Some("x".to_owned()),
                Some(HASH.to_owned()),
                BTreeMap::new(),
                &zones()
            ),
            Err(ConfigError::GateAttestationMissing { field: "polarity" })
        ));
    }

    #[test]
    fn every_configured_link_needs_its_polarity() {
        let mut partial = attested();
        partial.remove(&LinkKind::Zone(ZoneId::Zone2));
        let err = TransmitGateConfig::validate(
            GateScope::RealBusAttested,
            Some("x".to_owned()),
            Some(HASH.to_owned()),
            partial,
            &zones(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("zone2"), "{err}");

        // Steam attested but not configured: the claim describes a link this
        // service does not have.
        let mut extra = attested();
        extra.insert(LinkKind::Steam, "A+ = adapter pin 2".to_owned());
        let err = TransmitGateConfig::validate(
            GateScope::RealBusAttested,
            Some("x".to_owned()),
            Some(HASH.to_owned()),
            extra,
            &zones(),
        )
        .unwrap_err();
        assert!(matches!(err, ConfigError::GatePolarityUnknownLink { .. }));

        // Present but empty.
        let mut blank = attested();
        blank.insert(LinkKind::Zone(ZoneId::Zone1), "   ".to_owned());
        assert!(matches!(
            TransmitGateConfig::validate(
                GateScope::RealBusAttested,
                Some("x".to_owned()),
                Some(HASH.to_owned()),
                blank,
                &zones()
            ),
            Err(ConfigError::GatePolarityEmpty { .. })
        ));
    }

    #[test]
    fn the_fixture_hash_must_look_like_one() {
        for bad in [
            "",
            "not-a-hash",
            "0123456789ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcde",
        ] {
            assert!(
                matches!(
                    TransmitGateConfig::validate(
                        GateScope::RealBusAttested,
                        Some("x".to_owned()),
                        Some(bad.to_owned()),
                        attested(),
                        &zones()
                    ),
                    Err(ConfigError::GateFixtureHash { .. })
                ),
                "{bad} was accepted"
            );
        }
        assert!(is_sha256(HASH));
    }

    #[test]
    fn the_polarity_keys_are_the_three_links() {
        assert_eq!(polarity_link("zone1"), Some(LinkKind::Zone(ZoneId::Zone1)));
        assert_eq!(polarity_link("zone2"), Some(LinkKind::Zone(ZoneId::Zone2)));
        assert_eq!(polarity_link("steam"), Some(LinkKind::Steam));
        assert_eq!(polarity_link("zone3"), None);
    }

    #[test]
    fn the_two_scopes_spell_as_the_contract_does() {
        #[derive(Deserialize, Debug)]
        struct W {
            scope: GateScope,
        }
        let a: W = toml::from_str(r#"scope = "emulator-only""#).unwrap();
        assert_eq!(a.scope, GateScope::EmulatorOnly);
        let b: W = toml::from_str(r#"scope = "real-bus-attested""#).unwrap();
        assert_eq!(b.scope, GateScope::RealBusAttested);
        assert_eq!(b.scope.to_string(), "real-bus-attested");
        assert!(toml::from_str::<W>(r#"scope = "open""#).is_err());
    }
}
