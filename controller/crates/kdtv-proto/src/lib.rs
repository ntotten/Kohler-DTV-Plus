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
//! # Modules
//!
//! | Module | Contents |
//! | --- | --- |
//! | [`saturn`] | The Saturn valve protocol: framing, checksum, decoder, allowlist encoder, outlet tables, fault tables, timings |
//! | [`dtv`] | The DTV+ protocol and the steam device profile: byte-stuffed framing, addressing, decoder, allowlist encoder, status and error decoding, timings |
//! | [`fixtures`] | Golden frames with evidence tiers |
//! | [`gate`] | The transmit gate: no authority, no real bus |
//!
//! # Why one crate
//!
//! The two codecs share nothing but a physical layer, and merging them is
//! deliberate: `TEST-15` wants a *cross-codec* test — a `Cx2` value must be
//! rejected by the steam encoder and an `Fx2` by the valve encoder — and that
//! test belongs where both encoders are visible. It lives in `dtv`'s module
//! tests.

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

pub use saturn::{Direction, LinkPhase};
