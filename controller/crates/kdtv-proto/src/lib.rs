//! Saturn and DTV+ wire codecs, the command allowlist, golden fixtures and the
//! transmit gate.
//!
//! # Evidence tier
//!
//! **No frame in this crate has been verified against this installation's
//! hardware.** Every Saturn constant, opcode, payload layout and timing figure
//! below is tier `[C]` — third-party reverse engineering vendored from
//! `research/xagon0/`, plus the local `docs/devices/valve-control.md`, which
//! disagrees with it in several places. Nothing here is `[A]`. The Phase 1
//! packet capture exists to promote these to measured facts, and until it does
//! the transmit gate (`gate`) is what keeps the encoder off a real bus.
//!
//! # How denial works here
//!
//! Operations this system must never perform are denied by the **absence of a
//! variant**, not by a runtime check. There is no `SaturnOp::WriteCalibration`,
//! so no program in this workspace can spell one; a reviewer confirms the denial
//! by reading one enum rather than by auditing every call site. The scan tests
//! in `saturn::encode` assert the property mechanically.
//!
//! Frames are the other half of the same idea. [`saturn::SaturnFrame`] has
//! private fields, no public constructor, no `Deserialize` and no
//! `From<Vec<u8>>`: every value that exists came out of the encoder. The decoder
//! produces [`saturn::DecodedFrame`], a separate permissive type that is
//! deliberately not convertible into a transmittable one.
//!
//! # Modules
//!
//! | Module | Contents |
//! | --- | --- |
//! | [`saturn`] | The Saturn valve protocol: framing, checksum, decoder, allowlist encoder, outlet tables, fault tables, timings |
//! | [`dtv`] | The DTV+ steam link |
//! | [`fixtures`] | Golden frames with evidence tiers |
//! | [`gate`] | The transmit gate: no authority, no real bus |

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
