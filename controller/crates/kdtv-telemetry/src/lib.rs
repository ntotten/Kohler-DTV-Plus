//! The log schema, and the guarantees the schema itself carries.
//!
//! `DESIGN.md` § Software design lists what must be recorded. Several
//! of those requirements are easy to satisfy on the day and easy to lose six
//! months later, so they are built into the types rather than left to the call
//! sites:
//!
//! - **A wall-clock stamp never travels without its NTP sync state.** The Pi 4
//!   has no RTC of its own; stamps before the first sync are wrong, which makes
//!   an unsynced boot's frame log useless for correlating a fault. There is no
//!   API here that yields a bare wall time. See [`Stamp`].
//! - **Every command record carries all five identifying fields.** They are
//!   non-optional struct fields, so a record cannot be built without them.
//! - **Credentials cannot be logged.** [`Redacted`] prints `[redacted]` in both
//!   `Debug` and `Serialize`, so neither a structured line nor a debug dump can
//!   leak one.
//!
//! Nothing in this crate reads a clock. Stamps are constructed by the caller
//! from the `Clock` trait in `kdtv-hal`, which is the only thing that does.

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

pub mod capture;
pub mod redact;
pub mod stamp;

pub use capture::{Direction, FrameRecord, SessionRecord, StopReason};
pub use redact::Redacted;
pub use stamp::{Monotonic, NtpSync, Stamp};

use kdtv_units::{BootId, CommandId, LinkKind, PiBootId};
use serde::Serialize;

/// Where a command came from. Recorded on every command, because "who asked for
/// this" is the first question after an unexpected event.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RequestSource {
    /// The local authenticated API. The session id identifies the caller's
    /// session, not the caller.
    LocalApi { session: u64, peer: String },
    /// The operator, at the command-line client.
    Cli { peer: String },
    /// The service itself — a session expiry, a safety escalation, the boot
    /// sequence. Never a user.
    Service { reason: &'static str },
}

/// The identifying fields every command carries into the log.
///
/// All five are required. `DESIGN.md` asks for "Pi boot ID, service
/// boot ID, command ID, request source, and requested state"; making them
/// non-`Option` fields is the whole mechanism — there is no builder to forget
/// one and no default to fill one in.
#[derive(Clone, Debug, Serialize)]
pub struct CommandRecord {
    pub pi_boot: PiBootId,
    pub service_boot: BootId,
    pub command: CommandId,
    pub source: RequestSource,
    /// The state that was asked for, rendered. Not the state that resulted.
    pub requested: String,
    pub at: Stamp,
}

/// Why a request was refused, in the form the log wants.
///
/// A rejection and a fault are different things and must not be conflated:
/// a request that fails validation is refused to the caller and **no valve state
/// changes**, whereas bad data on the wire escalates to all-off and a latch.
/// `DESIGN.md` § Safety boundary rule 9 draws that line; this type
/// is only ever the first of the two.
#[derive(Clone, Debug, Serialize)]
pub struct RejectionRecord {
    pub command: CommandId,
    pub reason: String,
    /// The local clamp or check that refused it, named.
    pub check: &'static str,
    pub at: Stamp,
}

/// What the engine wants written, without knowing how logging is configured.
///
/// The engine is sans-IO: it returns these rather than calling `tracing` itself,
/// so a state machine test can assert on what would have been logged.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum LogEvent {
    Command(Box<CommandRecord>),
    Rejected(Box<RejectionRecord>),
    Frame(Box<FrameRecord>),
    Session(Box<SessionRecord>),
    /// A setpoint the encoder had to pull to a bound, with the record that
    /// explains it.
    Clamped {
        command: CommandId,
        record: kdtv_units::ClampRecord,
        at: Stamp,
    },
    /// Acknowledgement latency, retry count and the values read back, per link.
    Transaction {
        link: LinkKind,
        op: String,
        latency_ms: u64,
        retries: u32,
        at: Stamp,
    },
    /// A serial, watchdog, USB, service-restart, controller-power or
    /// valve-power-loss event.
    Platform {
        what: PlatformEvent,
        detail: String,
        at: Stamp,
    },
    /// A safety escalation: what tripped, what it did, and to which links.
    Safety {
        link: LinkKind,
        trigger: String,
        effects: Vec<String>,
        at: Stamp,
    },
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformEvent {
    SerialOpened,
    SerialClosed,
    SerialError,
    UsbEnumerationLost,
    WatchdogPetted,
    WatchdogMissed,
    ServiceStarted,
    ServiceStopping,
    ValvePowerLossSuspected,
    NtpSyncAcquired,
    NtpSyncLost,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp() -> Stamp {
        Stamp::new(
            Monotonic::from_nanos(1_000),
            1_756_500_000,
            NtpSync::Synchronised,
        )
    }

    #[test]
    fn a_command_record_cannot_be_built_without_its_identity() {
        // This test is a statement about the type, not about runtime behaviour:
        // every field below is required, so omitting one does not compile.
        let r = CommandRecord {
            pi_boot: PiBootId("boot-uuid".into()),
            service_boot: BootId(7),
            command: CommandId(42),
            source: RequestSource::Cli {
                peer: "operator".into(),
            },
            requested: "start zone1 outlets [1] at 100.0F for 300s".into(),
            at: stamp(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"command\":42"));
        assert!(json.contains("boot-uuid"));
    }

    #[test]
    fn a_log_event_serialises_with_its_discriminant() {
        let e = LogEvent::Platform {
            what: PlatformEvent::UsbEnumerationLost,
            detail: "zone1 converter vanished".into(),
            at: stamp(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"event\":\"platform\""));
        assert!(json.contains("usb_enumeration_lost"));
    }
}
