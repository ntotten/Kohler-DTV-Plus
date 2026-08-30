//! The two sans-IO state machines: one per valve zone, one for steam.
//!
//! This crate decides *what to put on a bus and when*. It does not put anything
//! on one: it performs no I/O, owns no port, spawns no task and reads no clock.
//! An event and a [`kdtv_telemetry::Monotonic`] reading go in, and a [`Step`]
//! comes out saying what to transmit, what the safety kernel wants done, and
//! when to come back.
//!
//! # Why the clock is a parameter
//!
//! `clippy.toml` denies `Instant::now` and `SystemTime::now` in this workspace,
//! and this crate is the reason the denial is worth having. Every constant these
//! machines run at is a real one — a 525 ms Saturn tick, a 320 ms response
//! deadline, a 500 ms outlet stagger, a 1200 s session limit, a 2100 ms
//! transaction ceiling — and a test drives twenty-two simulated minutes of them
//! in microseconds. A state machine that read a clock would have to be tested by
//! waiting.
//!
//! # What is here
//!
//! | Module | Contents |
//! | --- | --- |
//! | [`zone`] | The valve-bus machine: boot sequence, discovery, sessions, purge, the fault matrix |
//! | [`steam`] | The DTV+ link machine: enrolment, setpoint and duration, degraded versus lost |
//! | [`budget`] | The explicit bound on one bus transaction |
//! | [`note`] | Log lines, minus the wall clock this crate cannot read |
//!
//! # Three properties worth reading the code for
//!
//! 1. **There is no edge from cold to running.** [`zone::ZonePhase::Running`] is
//!    reachable only from `ReadyOff`, and only by an event carrying an
//!    [`kdtv_safety::OpenGrant`]. Nothing about a session is persisted, so a
//!    restart or a watchdog reset starts the boot sequence over.
//!
//! 2. **Escalation is not decided here.** Every wire condition these machines
//!    observe — a fault byte, a checksum failure on a write, a malformed
//!    response, an out-of-range value, a missed safety response — becomes a
//!    [`kdtv_safety::SafetyEvent`] handed to the kernel, and the kernel's
//!    [`kdtv_safety::Effect`]s come back out in the step unaltered. The engine
//!    moves itself to match them; it does not choose them.
//!
//! 3. **A fault takes one link down.** The kernel names every zone a shared
//!    fault reaches, and each machine applies only the effects naming its own
//!    link. That scoping is asserted in every single-zone test in
//!    `crate::tests`, because it is the property most likely to break quietly.
//!
//! # No session can be extended
//!
//! There is no `RefreshTimer` event, no refresh operation, and no timer path
//! that emits a write. `kdtv-safety`'s `SessionDeadline` has no `extend` and no
//! setter, and this crate never asks for one. Whether the Prompt 3's own
//! 1800-second timer is refreshed by ordinary polling is unresolved — capture
//! question 5 — so it is not relied on in either direction. `SESS-02` /
//! `SESS-03`.

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

pub mod budget;
pub mod note;
pub mod steam;
pub mod zone;

#[cfg(test)]
mod tests;

pub use budget::RetryBudget;
pub use note::Note;
pub use steam::{
    SteamCache, SteamCommand, SteamEvent, SteamMachine, SteamParams, SteamPhase, SteamPhaseKind,
    SteamRefusal, SteamSettings, SteamStep,
};
pub use zone::{
    BaselineDrift, Health, OperatorCommand, Purge, Refusal, StaggerPlan, StartRequest, Step,
    ZoneCache, ZoneEvent, ZoneMachine, ZonePhase, ZonePhaseKind, ZoneSettings,
};
