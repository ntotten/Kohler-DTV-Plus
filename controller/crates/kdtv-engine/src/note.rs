//! Log lines the engine wants written, minus the wall clock it cannot read.
//!
//! # Why this type exists rather than [`LogEvent`] directly
//!
//! `ARCHITECTURE.md` § 8 shows `Step.log` as a `SmallVec<[LogEvent; 2]>`. Every
//! [`LogEvent`] variant carries a [`Stamp`], and a [`Stamp`] is a monotonic
//! reading **and** a wall time **and** its NTP sync state — inseparably, which is
//! the guarantee `kdtv-telemetry` exists to make. This crate has no wall clock:
//! `Instant::now` and `SystemTime::now` are denied by `clippy.toml` and time
//! arrives as a [`Monotonic`] parameter, which is what lets the fault matrix run
//! at real constants in microseconds.
//!
//! So the engine emits a [`Note`], which is everything about the line except
//! when it happened, and the service — which owns the clock — turns it into a
//! [`LogEvent`] with [`Note::into_log_event`]. Nothing is lost and no wall time
//! is invented.

use kdtv_telemetry::{
    CommandRecord, LogEvent, Monotonic, PlatformEvent, RejectionRecord, RequestSource,
    SessionRecord, Stamp, StopReason,
};
use kdtv_units::{BootId, CommandId, LinkKind, PiBootId};
use std::time::Duration;

/// A log line, waiting for a stamp.
#[derive(Clone, PartialEq, Debug)]
pub enum Note {
    /// One bus transaction: how long the acknowledgement took and how many
    /// attempts it needed.
    Transaction {
        link: LinkKind,
        op: &'static str,
        latency: Duration,
        /// Total sends, including the first. `retries` in the log is one less.
        attempts: u32,
    },
    /// A serial, watchdog, USB or service-lifecycle event.
    Platform { what: PlatformEvent, detail: String },
    /// A safety escalation: what tripped and what the kernel returned.
    Safety {
        link: LinkKind,
        trigger: String,
        effects: Vec<String>,
    },
    /// A water session, start to stop.
    Session {
        link: LinkKind,
        command: CommandId,
        duration: Duration,
        stop: StopReason,
    },
    /// A command the engine refused. **No valve state changed and nothing was
    /// transmitted** — that is the distinction the design draws between invalid
    /// input and invalid wire data.
    Rejected {
        command: CommandId,
        reason: String,
        check: &'static str,
    },
    /// A command the engine accepted, with what was asked for.
    Accepted {
        command: CommandId,
        requested: String,
    },
}

impl Note {
    /// Attach the moment this happened.
    ///
    /// A session's start stamp is derived by subtracting its own duration from
    /// `at`, on both readings. That is arithmetic on a recorded interval, not an
    /// invented wall time: the monotonic half is exact, and the wall half is as
    /// good as the wall half of `at`, which carries its own sync state.
    #[must_use]
    pub fn into_log_event(self, at: Stamp, pi_boot: &PiBootId, service_boot: BootId) -> LogEvent {
        match self {
            Self::Transaction {
                link,
                op,
                latency,
                attempts,
            } => LogEvent::Transaction {
                link,
                op: op.to_owned(),
                latency_ms: u64::try_from(latency.as_millis()).unwrap_or(u64::MAX),
                retries: attempts.saturating_sub(1),
                at,
            },
            Self::Platform { what, detail } => LogEvent::Platform { what, detail, at },
            Self::Safety {
                link,
                trigger,
                effects,
            } => LogEvent::Safety {
                link,
                trigger,
                effects,
                at,
            },
            Self::Session {
                link,
                command,
                duration,
                stop,
            } => LogEvent::Session(Box::new(SessionRecord::new(
                link,
                command,
                rewind(at, duration),
                at,
                stop,
            ))),
            Self::Rejected {
                command,
                reason,
                check,
            } => LogEvent::Rejected(Box::new(RejectionRecord {
                command,
                reason,
                check,
                at,
            })),
            Self::Accepted { command, requested } => LogEvent::Command(Box::new(CommandRecord {
                pi_boot: pi_boot.clone(),
                service_boot,
                command,
                source: RequestSource::Service {
                    reason: "engine accepted an operator command",
                },
                requested,
                at,
            })),
        }
    }
}

/// `at` moved back by `d`, on both the monotonic and the wall reading.
fn rewind(at: Stamp, d: Duration) -> Stamp {
    let ns = u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
    let secs = i64::try_from(d.as_secs()).unwrap_or(i64::MAX);
    Stamp::new(
        Monotonic::from_nanos(at.monotonic_ns.as_nanos().saturating_sub(ns)),
        at.wall_unix_s.saturating_sub(secs),
        at.ntp,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_telemetry::NtpSync;
    use kdtv_units::ZoneId;

    fn stamp(secs: u64) -> Stamp {
        Stamp::new(
            Monotonic::from_nanos(secs * 1_000_000_000),
            1_756_500_000 + i64::try_from(secs).unwrap(),
            NtpSync::Synchronised,
        )
    }

    fn boot() -> (PiBootId, BootId) {
        (PiBootId("boot-uuid".into()), BootId(3))
    }

    #[test]
    fn a_transaction_note_reports_retries_as_one_fewer_than_attempts() {
        let (pi, svc) = boot();
        let e = Note::Transaction {
            link: LinkKind::Zone(ZoneId::Zone1),
            op: "AllOff",
            latency: Duration::from_millis(42),
            attempts: 3,
        }
        .into_log_event(stamp(10), &pi, svc);
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"retries\":2"), "{json}");
        assert!(json.contains("\"latency_ms\":42"), "{json}");
    }

    #[test]
    fn a_session_note_derives_its_start_from_the_duration_it_recorded() {
        let (pi, svc) = boot();
        let e = Note::Session {
            link: LinkKind::Zone(ZoneId::Zone2),
            command: CommandId(9),
            duration: Duration::from_secs(1200),
            stop: StopReason::SessionLimit,
        }
        .into_log_event(stamp(1500), &pi, svc);
        let LogEvent::Session(rec) = e else {
            panic!("expected a session record");
        };
        assert_eq!(rec.duration_s, 1200);
        assert_eq!(
            rec.started.monotonic_ns,
            Monotonic::from_nanos(300 * 1_000_000_000)
        );
        assert_eq!(rec.ended.wall_unix_s - rec.started.wall_unix_s, 1200);
        // The sync state travels with both stamps; a derived start cannot claim
        // to be better synchronised than the reading it came from.
        assert_eq!(rec.started.ntp, rec.ended.ntp);
    }

    #[test]
    fn a_rejection_names_the_check_that_refused_it() {
        let (pi, svc) = boot();
        let e = Note::Rejected {
            command: CommandId(1),
            reason: "the zone is cold".into(),
            check: "zone phase",
        }
        .into_log_event(stamp(1), &pi, svc);
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("zone phase"), "{json}");
        assert!(json.contains("\"event\":\"rejected\""), "{json}");
    }

    #[test]
    fn rewinding_further_than_the_stamp_saturates_rather_than_wrapping() {
        let s = rewind(stamp(5), Duration::from_secs(9_999));
        assert_eq!(s.monotonic_ns, Monotonic::from_nanos(0));
    }
}
