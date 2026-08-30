//! Saturn and DTV+ wire codecs, the command allowlist, golden fixtures and the
//! transmit gate.
//!
//! # Evidence tier
//!
//! **No frame in this crate has been verified against this installation's
//! hardware.** Every constant, opcode, payload layout, bitmask and timing figure
//! below is tier `[C]` — third-party reverse engineering vendored from
//! `research/xagon0/`, plus the local `docs/devices/valve-control.md` and
//! `docs/replacement-controller/STEAM-ADAPTER.md`, which disagree with it in
//! several places. Nothing here is `[A]`.
//!
//! The DTV+ side is thinner still: **no DTV+ bus has ever been captured in this
//! project.** The reference system reports `steam_installed = false`, so the
//! steam codec has never seen a real device at all. The Phase 1 and Phase 5
//! captures exist to promote these to measured facts, and until they do the
//! transmit gate (`gate`) is what keeps the encoders off a real bus.
//!
//! # How denial works here
//!
//! Operations this system must never perform are denied by the **absence of a
//! variant**, not by a runtime check. There is no `SaturnOp::WriteCalibration`
//! and no `SteamOp::Reboot`, so no program in this workspace can spell one; a
//! reviewer confirms the denial by reading one enum rather than by auditing
//! every call site. The scan tests in `saturn::encode` and `dtv::encode` assert
//! the property mechanically.
//!
//! **The absent variant is not always a command.** The steam generator's
//! 45-minute power-clean cycle is started by a *payload value* — `0xCC` in the
//! operation-state byte of `SET_DEV_PARAM`, which is an allowlisted command — so
//! omitting a command variant does nothing to it. The denial there is
//! [`dtv::SteamOpState`], which has `Off` and `On` and no third variant, and a
//! byte-level scan proves `0xCC` never reaches that position.
//! `CORRECTIONS.md` item 1.
//!
//! Frames are the other half of the same idea. [`saturn::SaturnFrame`] and
//! [`dtv::DtvFrame`] have private fields, no public constructor, no
//! `Deserialize` and no `From<Vec<u8>>`: every value that exists came out of an
//! encoder. The decoders produce [`saturn::DecodedFrame`] and
//! [`dtv::DecodedDtv`], separate permissive types that are deliberately not
//! convertible into transmittable ones.
//!
//! # The allowlist principle
//!
//! The two `enum`s [`saturn::SaturnOp`] and [`dtv::SteamOp`] are, together, the
//! complete answer to "what can this system put on a bus" — twenty valve
//! operations and eight steam operations, and nothing else. An unknown opcode is
//! not a runtime rejection; it is a program that does not compile.
//!
//! Three consequences follow, and all three are what make the allowlist worth
//! having over a validation function:
//!
//! - **Adding an operation is a visible diff in one enum**, and it breaks the
//!   enumeration test, the fixture-coverage test and the scan tests at once.
//! - **A denied operation has no fixture and needs none**, because there is
//!   nothing to record. Conversely every allowlisted operation *must* have a
//!   fixture: [`fixtures::required_transmit_ids`] is derived from the two
//!   allowlists, so a new operation with no recorded frame fails the build's
//!   own evidence check.
//! - **The reads stay in.** Calibration `0x10` and configuration `0x15` are
//!   allowlisted reads whose corresponding writes, `0xC0` and `0x95`, are not.
//!   The Phase 0 rollback procedure needs to read each valve's calibration code
//!   back, and after the K-99695 is powered down this service is the only thing
//!   that can. `CORRECTIONS.md` item 7.
//!
//! # The transmit gate, at two boundaries
//!
//! Because nothing here is `[A]`, the daemon must be unable to drive a real bus
//! while the emulator path stays fully usable. [`gate::TransmitAuthority`] is
//! how, and it is checked twice:
//!
//! 1. **Here.** [`saturn::Encoder::new`] and [`dtv::SteamEncoder::new`] require
//!    a `&TransmitAuthority`. No authority means no encoder, and no encoder
//!    means no [`saturn::SaturnFrame`] and no [`dtv::DtvFrame`] — the only two
//!    types a link will transmit. *Nothing to send* is a stronger property than
//!    *nothing sent*.
//! 2. **In `kdtv-hal`**, which is a different crate and another agent's work:
//!    opening a real serial backend must consult
//!    [`gate::TransmitAuthority::permits_real_bus_on`] for the link being
//!    opened, and PTY and loopback backends must open under either scope. This
//!    crate cannot enforce that boundary — it must not depend on `kdtv-hal` —
//!    so it exposes what the check needs and says so here.
//!
//! [`gate::TransmitScope::RealBusAttested`] requires every allowlisted
//! operation to resolve to a [`fixtures::Provenance::Captured`] fixture, a
//! pinned fixture-set hash, and a measured bus polarity per link. Today the
//! first of those fails on all twenty-eight operations, and
//! `the_gate_is_closed_in_the_committed_state` is the test that says so on
//! every CI run.
//!
//! # Modules
//!
//! | Module | Contents |
//! | --- | --- |
//! | [`saturn`] | The Saturn valve protocol: framing, checksum, decoder, allowlist encoder, outlet tables, fault tables, timings |
//! | [`dtv`] | The DTV+ protocol and the steam device profile: byte-stuffed framing, addressing, decoder, allowlist encoder, status and error decoding, timings |
//! | [`fixtures`] | Golden frames with evidence tiers, and the set hash |
//! | [`gate`] | The transmit gate: no authority, no encoder; no attestation, no real bus |
//!
//! # Why one crate
//!
//! The two codecs share nothing but a physical layer, and merging them is
//! deliberate: `TEST-15` wants a *cross-codec* test — a `Cx2` value must be
//! rejected by the steam encoder and an `Fx2` by the valve encoder — and that
//! test belongs where both encoders are visible. It lives in `dtv`'s module
//! tests.
//!
//! # The public surface
//!
//! Each protocol's names stay behind its own module, because `Direction` and
//! `checksum` mean different things on the two links and flattening them would
//! be the units hazard all over again. Only the names that are genuinely
//! protocol-independent are re-exported at the crate root: the transmit gate,
//! the fixture types, and [`LinkPhase`], which is shared by both encoders.

// Tests legitimately panic on a broken invariant; the production lints stay on
// for library code, where a panic is a fault, not a diagnosis.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp
    )
)]

pub mod dtv;
pub mod fixtures;
pub mod gate;
pub mod saturn;

pub use fixtures::{
    EvidenceTier, Fixture, FixtureDirection, FixtureError, FixtureId, FixtureSet, Provenance,
    required_transmit_ids, saturn_op_fixture_id, steam_op_fixture_id,
};
pub use gate::{
    GateError, PolarityAttestation, PolarityNote, RequestedScope, TransmitAuthority,
    TransmitGateConfig, TransmitScope,
};
pub use saturn::{Direction, LinkPhase};

#[cfg(test)]
mod tests {
    use super::*;

    /// The crate root exports the protocol-independent half and nothing that
    /// would let a caller confuse the two links. `saturn::checksum` and
    /// `dtv::checksum` are different functions over different fields, and
    /// neither is re-exported here.
    #[test]
    fn the_crate_root_exports_the_gate_and_the_evidence_and_not_the_codecs() {
        let fx: &FixtureSet = FixtureSet::embedded();
        let auth = TransmitAuthority::emulator_only(fx);
        assert_eq!(auth.scope(), &TransmitScope::EmulatorOnly);
        assert_eq!(EvidenceTier::C.to_string(), "[C]");
        assert_eq!(FixtureDirection::ToDevice.to_string(), "to-device");
        assert_eq!(required_transmit_ids().count(), 28);
        assert_eq!(
            saturn_op_fixture_id(saturn::SaturnOpKind::AllOff),
            "saturn.all_off"
        );
        assert_eq!(steam_op_fixture_id(dtv::SteamOpKind::Stop), "steam.stop");

        // LinkPhase is shared by both encoders, so it is a root name.
        assert_eq!(LinkPhase::ReadyOff, saturn::LinkPhase::ReadyOff);
        // Direction is not: it is the Saturn one, re-exported under its own
        // name, and `dtv::DtvDirection` deliberately keeps a different one.
        assert_eq!(Direction::MasterToValve, saturn::Direction::MasterToValve);
    }

    /// **`RAW-01`, at the crate boundary.** The only public routes to a
    /// transmittable frame are the two encoders, and both require a
    /// [`TransmitAuthority`]. This test is the compiled statement of that: it
    /// builds one of each frame the only way there is, and the two decoded
    /// types beside them, which have no route back.
    #[test]
    fn req_controller_design_raw_01_the_only_route_to_a_frame_is_an_encoder_holding_an_authority() {
        use kdtv_units::{LinkKind, Slot, ZoneId};

        let auth = TransmitAuthority::emulator_only(FixtureSet::embedded());

        let table = saturn::OutletTable::new(
            saturn::ValveType::Prompt3Port,
            (1u8..=3).map(|n| saturn::OutletMapping {
                slot: Slot::new(n).unwrap(),
                status_index: n,
                wire_outlet: n,
            }),
        )
        .unwrap();
        let valve = saturn::Encoder::new(
            &auth,
            LinkKind::Zone(ZoneId::Zone2),
            saturn::MasterAddr::Dtv,
            table,
        );
        let frame = valve
            .encode(
                saturn::ValveAddr::new(0x03).unwrap(),
                &saturn::SaturnOp::AllOff,
                LinkPhase::ReadyOff,
                None,
                // AllOff closes outlets. Only opening them needs an authority.
                None,
            )
            .unwrap();
        assert_eq!(frame.op(), saturn::SaturnOpKind::AllOff);

        let steam = dtv::SteamEncoder::new(&auth);
        let dtv_frame = steam
            .encode(
                dtv::DevAddr::REFERENCE,
                &dtv::SteamOp::ReadStatus,
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        assert_eq!(dtv_frame.op(), dtv::SteamOpKind::ReadStatus);

        // Both encoders carry the authority they were built under, which is
        // what `kdtv-hal` reads at the second boundary.
        assert!(!valve.authority().permits_real_bus());
        assert!(!steam.authority().permits_real_bus());
        assert!(!valve.authority().permits_real_bus_on(valve.link()));

        // A decoded frame is a different type with no conversion back. Its
        // bytes are readable; that is all.
        let mut rx = saturn::RxBuffer::new();
        rx.extend(frame.bytes());
        let decoded = saturn::decode(
            &mut rx,
            &saturn::Expectation::capture(saturn::MasterAddr::Dtv),
        )
        .unwrap()
        .unwrap();
        assert_eq!(decoded.control.0, saturn::opcode::WRITE_OUTLET_STATES);
    }

    /// The evidence base and the allowlists agree, checked from outside the
    /// modules that define either. Twenty valve operations, eight steam
    /// operations, twenty-eight fixtures required, and today every one of them
    /// tier `[C]`.
    #[test]
    fn the_committed_evidence_base_is_entirely_tier_c() {
        let fx = FixtureSet::embedded();
        assert_eq!(saturn::SaturnOp::ALL.len(), 20);
        assert_eq!(dtv::SteamOp::ALL.len(), 8);
        assert_eq!(required_transmit_ids().count(), 28);
        assert_eq!(fx.documented_only().len(), fx.len());
        assert!(fx.captured_only().is_empty());
        for id in required_transmit_ids() {
            let f: &Fixture = fx.get(id).unwrap();
            assert_eq!(f.tier(), EvidenceTier::C, "{id}");
            assert!(matches!(f.provenance(), Provenance::Documented { .. }));
        }
    }

    /// The shipped configuration resolves, and resolves to the emulator. A
    /// daemon built from this repository as committed boots and cannot
    /// transmit.
    #[test]
    fn the_shipped_configuration_resolves_to_the_emulator() {
        let fx = FixtureSet::embedded();
        let auth = TransmitAuthority::resolve(&TransmitGateConfig::emulator_only(), fx).unwrap();
        assert!(!auth.permits_real_bus());

        let asking = TransmitGateConfig {
            scope: RequestedScope::RealBusAttested,
            capture_ref: Some("nothing yet".to_owned()),
            polarity: PolarityAttestation {
                notes: vec![PolarityNote {
                    link: kdtv_units::LinkKind::Steam,
                    note: "unmeasured".to_owned(),
                    attested_on: "never".to_owned(),
                }],
            },
            expected_fixtures_sha256: Some(fx.sha256_hex()),
        };
        assert!(matches!(
            TransmitAuthority::resolve(&asking, fx),
            Err(GateError::FixturesNotCaptured { .. })
        ));
    }
}
