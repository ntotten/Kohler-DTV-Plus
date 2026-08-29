//! Emulated valves, steam adapter and wire.
//!
//! This crate is `publish = false` and is **never a dependency of anything that
//! ships**. That is asserted by a dependency-graph check in CI rather than left
//! to convention, because this is the only crate allowed to build arbitrary or
//! malformed frames. Keeping it out of the daemon's graph is what stops that
//! capability existing in production at all.
//!
//! What a green run of this emulator proves, and does not: the device models
//! here are built from the same tier `[C]` reverse-engineering documents the
//! encoder is. Agreement between them is internal consistency with the
//! specification, not evidence that the specification matches the valve. Phase 1
//! capture is what closes that.

// Tests legitimately panic on a broken invariant; the production lints stay on
// for library code.
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

pub mod pty;
