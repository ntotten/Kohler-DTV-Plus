//! The tokio runtime and the composition root: link tasks, supervisor, state
//! cache, watchdog.
//!
//! Everything below this crate is sans-IO. `kdtv-engine` decides what to put on
//! a bus, `kdtv-safety` decides whether water may move, `kdtv-proto` turns an
//! operation into bytes and `kdtv-hal` moves bytes. None of them reads a clock,
//! owns a port or spawns a task. This crate does all four, and it is the only
//! one that does.
//!
//! # The shape
//!
//! ```text
//!            commands (mpsc)                  ┌──────────────┐
//!  API ────────────────────────────────────►  │              │
//!  API ◄── snapshot (arc-swap, no channel) ── │  Supervisor  │
//!  API ◄── events (broadcast)  ─────────────  │              │
//!                                             └──┬───┬───┬───┘
//!                       orders / reports (mpsc)  │   │   │
//!                                       ┌────────┘   │   └────────┐
//!                                  ┌────▼───┐   ┌────▼───┐   ┌────▼───┐
//!                                  │ zone 1 │   │ zone 2 │   │ steam  │
//!                                  │  pump  │   │  pump  │   │  pump  │
//!                                  └────────┘   └────────┘   └────────┘
//! ```
//!
//! One supervisor task owns every state machine, the one [`SafetyKernel`], the
//! receive buffers, the encoders and the cache. Each link gets a **byte pump**
//! task that owns its [`Link`] and does nothing else: it writes what it is told
//! to write and forwards what it reads. No protocol decision is made in a pump,
//! and no byte is moved in the supervisor.
//!
//! Why that split rather than one task per link with the kernel behind a mutex:
//! the kernel holds all three links' state because a shared fault crosses them,
//! so it is borrowed `&mut` by whichever machine is stepping. A mutex would make
//! "zone 1's fault reaches zone 2" a lock-ordering question and would let two
//! machines step while the kernel's cross-link state was half-updated. One owner
//! removes the question. The work is small enough that it costs nothing: three
//! links at a 525 ms and a 500 ms cadence, one transaction each, is a few dozen
//! microseconds of decoding per second.
//!
//! Everything here is `Send`, so the supervisor runs on either runtime flavour.
//! The daemon should use a `current_thread` runtime: nothing in the loop is
//! parallelisable and a single thread removes every remaining question about
//! which thread the kernel is on.
//!
//! # What is guaranteed here rather than checked
//!
//! - **One transaction in flight per link.** A [`Step`] carries at most one
//!   `tx`, and a machine emits none while it has one outstanding. The Saturn
//!   frame has no sender field, so a serialised bus is the only thing that
//!   correlates a response with its request.
//! - **A status read never causes a bus transaction.** [`ServiceHandle::snapshot`]
//!   reads an [`arc_swap::ArcSwap`]. It sends nothing, touches no channel and
//!   cannot reach a link. `API-06`; this is what keeps the replacement from
//!   regrowing the polling behaviour that hung the original controller
//!   (`INVESTIGATIONS.md` I1).
//! - **Fail-off.** Every exit from the loop closes outlets first. Shutdown
//!   commands a stop, waits for the confirmation, and reports
//!   [`ShutdownOutcome::UnconfirmedOff`] when it does not arrive rather than
//!   claiming a clean stop.
//! - **Effects are applied, not interpreted.** The kernel's
//!   [`Effect`](kdtv_safety::Effect)s come back through the step and are
//!   performed in order, after the transmission, so an all-off reaches the valve
//!   before its port is closed.
//!
//! # Requirements
//!
//! `SVC-01` two independent zone machines, no shared mutable state but the
//! kernel; `SVC-02` the deadlines and checksums of `kdtv-proto`, never numbers
//! invented here; `SVC-03` the authoritative desired and actual state;
//! `SVC-04` fault polling and all-off escalation; `SVC-05` a typed command
//! surface and a read-only event stream; `API-06` cached status reads;
//! `BUS-01` per-bus serialisation; `LOG-01`..`LOG-10` the log schema.
//!
//! # Evidence
//!
//! Everything about the Saturn and DTV+ wire protocols in this workspace is
//! tier `[C]` — reverse-engineered, with no capture taken. This crate transmits
//! what `kdtv-proto` encodes and believes what `kdtv-proto` decodes; it adds no
//! evidence of its own and none of its behaviour against a real valve has been
//! observed.
//!
//! [`Link`]: kdtv_hal::Link
//! [`SafetyKernel`]: kdtv_safety::SafetyKernel
//! [`Step`]: kdtv_engine::Step

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

pub mod cache;
pub mod command;
pub mod event;
pub mod record;
pub mod service;
pub mod supervisor;

mod port;
mod rtd;

#[cfg(test)]
mod tests;

pub use cache::{
    IndependentReading, LinkStateLabel, StateCache, SteamStatus, SystemSnapshot, ZoneStatus,
};
pub use command::{Command, CommandError, ServiceHandle};
pub use event::{Lifecycle, ServiceEvent};
pub use record::Recorder;
pub use service::{Deps, Service, ShutdownTrigger, StartError, Started, install_signal_handlers};
pub use supervisor::{ShutdownOutcome, Supervisor};

/// The names the API layer needs and cannot reach on its own.
///
/// `cargo xtask audit-graph` denies a direct edge from `kdtv-api` to
/// `kdtv-proto`, and `kdtv-api` declares no dependency on `kdtv-safety` or
/// `kdtv-engine` either. It still has to name the types the command surface
/// takes and returns — an authorisation it mints, a validated start it fills in,
/// the refusal it renders. A crate cannot name a type from a dependency it has
/// not declared, so those names are re-exported here.
///
/// Nothing that can build or accept a wire frame is re-exported. That is the
/// capability the graph check exists to deny, and forwarding it through this
/// module would hand it back.
pub mod surface {
    pub use kdtv_engine::{OperatorCommand, Refusal, StartRequest, SteamCommand, SteamRefusal};
    pub use kdtv_safety::{
        Denial, FindingClass, LatchReason, OperatorAck, StartAuthorization, ValidatedStart,
    };
}

pub use surface::{
    Denial, FindingClass, LatchReason, OperatorAck, OperatorCommand, Refusal, StartAuthorization,
    SteamCommand, SteamRefusal, ValidatedStart,
};
