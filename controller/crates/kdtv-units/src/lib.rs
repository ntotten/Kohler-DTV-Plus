//! Temperature encodings, safety bounds and shared identifiers.
//!
//! This crate has no dependencies beyond `serde` and `thiserror`, and no I/O. It
//! exists so that two things live in exactly one place each:
//!
//! 1. **The encoding split.** Valves speak `Cx2` — Celsius times two. The steam
//!    generator speaks `Fx2` — Fahrenheit times two. They are unrelated types
//!    with no `From`, no `Deref`, no shared trait and no arithmetic between them,
//!    because the failure they prevent is not caught by range checking: Fx2 220
//!    is 110 °F, and the same byte read as Cx2 asks a valve for 110 °C.
//!    See `docs/replacement-controller/HARDWARE-SPEC.md` § 12.
//!
//! 2. **The numeric safety bounds.** [`ValveSetpoint`] and [`SteamSetpoint`] have
//!    private fields, so a value outside the clamp cannot be constructed, let
//!    alone transmitted.
//!
//! Nothing here reads a clock. Durations are values; the deadlines built from
//! them live in `kdtv-safety`.

// Tests legitimately panic on a broken invariant; the production lints below stay
// on for library code, where a panic is a fault, not a diagnosis.
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

pub mod ids;
pub mod independent;
pub mod session;
pub mod setpoint;
pub mod temp;

pub use ids::{
    BootId, CommandId, LinkKind, OpenAuthority, PiBootId, SessionId, Slot, SlotSet, ZoneId,
};
pub use independent::{
    CORRECTED_TRIP_C, CORRECTED_TRIP_DWELL, CorrectedC, CurveError, DIVERGENCE_DWELL,
    DIVERGENCE_LIMIT_C, OffsetCurve, RAW_TRIP_C, RTD_STARVATION, RawC, SCALD_C,
};
pub use session::{SessionDuration, SteamMinutes};
pub use setpoint::{Bound, ClampError, ClampRecord, SteamSetpoint, ValveSetpoint};
pub use temp::{Cx2, Fx2, LossyCx2};
