//! Where an engine note becomes a log line, and where the wall clock is
//! attached.
//!
//! The engine has no wall clock — `Instant::now` and `SystemTime::now` are
//! denied workspace-wide, and every constant those machines run at is a real one
//! that a test drives in microseconds. So the engine emits a
//! [`Note`], which is everything about a line except when it
//! happened, and this is the crate that reads [`kdtv_hal::Clock`] and finishes
//! it.
//!
//! # The required logs
//!
//! `LOG-01` boot ids, command id, request source and requested state;
//! `LOG-02` every wall stamp paired with its NTP sync state;
//! `LOG-04` clamps and rejection reasons; `LOG-05` raw RX and TX bytes with both
//! timestamps; `LOG-06` acknowledgement latency, retries and fault flags;
//! `LOG-07` serial, watchdog, USB and lifecycle events; `LOG-08` session start,
//! stop reason, duration and maximum observed temperature.
//!
//! Two of those are guaranteed by types rather than by this module.
//! [`Stamp`] has no constructor yielding a wall time without an
//! [`NtpSync`](kdtv_telemetry::NtpSync), which is `LOG-02`; and
//! [`CommandRecord`](kdtv_telemetry::CommandRecord)'s five identifying fields are
//! non-optional, which is `LOG-01`.
//!
//! # `LOG-09`
//!
//! "No credential, access token, or pairing data belongs in these logs." This
//! crate holds none: no field of any type here is a credential, the command
//! channel carries none, and [`kdtv_telemetry::Redacted`] is what a value would
//! have to be wrapped in if one ever arrived. The negative is asserted where it
//! can be — over the serialised snapshot, in [`crate::cache`].

use kdtv_engine::Note;
use kdtv_telemetry::{
    Direction, FrameRecord, LogEvent, PlatformEvent, RejectionRecord, RequestSource, Stamp,
};
use kdtv_units::{BootId, CommandId, LinkKind, PiBootId};
use tokio::sync::broadcast;

use crate::event::ServiceEvent;

/// Turns what the engine wanted written into what is written.
#[derive(Debug)]
pub struct Recorder {
    pi_boot: PiBootId,
    boot: BootId,
    events: broadcast::Sender<ServiceEvent>,
    /// `logging.frames` from the configuration. Raw frame bytes are the whole
    /// evidence base for Phase 1, and they are also the bulkiest thing this
    /// service writes, so the switch is configuration rather than a constant.
    frames: bool,
}

impl Recorder {
    pub(crate) const fn new(
        pi_boot: PiBootId,
        boot: BootId,
        events: broadcast::Sender<ServiceEvent>,
        frames: bool,
    ) -> Self {
        Self {
            pi_boot,
            boot,
            events,
            frames,
        }
    }

    /// The service boot id every record repeats.
    #[must_use]
    pub const fn boot(&self) -> BootId {
        self.boot
    }

    /// Attach the moment to an engine note.
    ///
    /// Returned rather than emitted, because two of the variants need finishing
    /// that only the supervisor can do: a session record's maximum observed
    /// temperature (`LOG-08`) and a command record's real request source
    /// (`LOG-01`). The engine cannot know either.
    #[must_use]
    pub fn finish(&self, note: Note, at: Stamp) -> LogEvent {
        note.into_log_event(at, &self.pi_boot, self.boot)
    }

    /// Write a finished event to the durable log and to the live stream.
    pub fn emit(&self, event: LogEvent) {
        match &event {
            LogEvent::Safety {
                link,
                trigger,
                effects,
                ..
            } => tracing::warn!(link = %link, trigger = %trigger, ?effects, "safety escalation"),
            LogEvent::Rejected(r) => {
                tracing::info!(command = r.command.0, reason = %r.reason, check = r.check, "refused");
            }
            LogEvent::Platform { what, detail, .. } => {
                tracing::info!(?what, detail = %detail, "platform");
            }
            LogEvent::Session(s) => {
                tracing::info!(link = %s.link, duration_s = s.duration_s, stop = ?s.stop, "session");
            }
            _ => tracing::debug!(event = ?event, "log"),
        }
        self.publish(ServiceEvent::Log(Box::new(event)));
    }

    /// Both at once, for the notes that need no finishing.
    pub fn note(&self, note: Note, at: Stamp) {
        self.emit(self.finish(note, at));
    }

    /// One frame, exactly as it appeared on the wire. `LOG-05`.
    ///
    /// A frame that failed to decode is the one most worth having, so the
    /// decode error travels with the bytes rather than replacing them.
    pub fn frame(
        &self,
        link: LinkKind,
        direction: Direction,
        bytes: &[u8],
        at: Stamp,
        decode_error: Option<String>,
    ) {
        if !self.frames {
            return;
        }
        let mut record = FrameRecord::new(link, direction, bytes, at);
        if let Some(why) = decode_error {
            record = record.with_decode_error(why);
        }
        self.emit(LogEvent::Frame(Box::new(record)));
    }

    /// A request this service refused before anything was transmitted.
    /// `LOG-04`.
    ///
    /// `reason` is why, `check` is which gate said so. Both reach the durable
    /// record; the live stream carries the reason, because that is the field an
    /// operator is shown and "safety kernel" is the same answer for a latched
    /// zone, a stale boot token and an outlet the valve does not have. The
    /// engine's own refusals publish their reason into the same field
    /// (`crate::supervisor`), and one event type must not mean two things.
    pub fn rejection(&self, command: CommandId, reason: String, check: &'static str, at: Stamp) {
        self.emit(LogEvent::Rejected(Box::new(RejectionRecord {
            command,
            reason: reason.clone(),
            check,
            at,
        })));
        self.publish(ServiceEvent::Refused { command, reason });
    }

    /// A serial, watchdog, USB or lifecycle event. `LOG-07`.
    pub fn platform(&self, what: PlatformEvent, detail: String, at: Stamp) {
        self.emit(LogEvent::Platform { what, detail, at });
    }

    /// Put an event on the live stream only.
    ///
    /// A send with no subscribers is not an error: the stream is a convenience
    /// for a live client, and the durable log is `tracing`.
    pub fn publish(&self, event: ServiceEvent) {
        let _ = self.events.send(event);
    }

    /// Patch the request source into a command record the engine produced.
    ///
    /// `Note::Accepted` becomes a [`kdtv_telemetry::CommandRecord`] with
    /// [`RequestSource::Service`], because the engine has no idea who asked.
    /// `LOG-01` wants the real source, and this is the only place that knows it.
    pub(crate) fn attribute(event: &mut LogEvent, source: &RequestSource) {
        if let LogEvent::Command(record) = event {
            record.source = source.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_telemetry::{Monotonic, NtpSync, StopReason};
    use kdtv_units::ZoneId;
    use std::time::Duration;

    fn stamp(secs: u64) -> Stamp {
        Stamp::new(
            Monotonic::from_nanos(secs.saturating_mul(1_000_000_000)),
            1_756_500_000_i64.saturating_add(i64::try_from(secs).unwrap()),
            NtpSync::Unsynchronised,
        )
    }

    fn recorder(frames: bool) -> (Recorder, broadcast::Receiver<ServiceEvent>) {
        let (tx, rx) = broadcast::channel(16);
        (
            Recorder::new(PiBootId("boot-uuid".into()), BootId(3), tx, frames),
            rx,
        )
    }

    #[test]
    fn req_hardware_spec_time_01_a_transaction_note_reaches_the_stream_with_a_wall_stamp_and_its_sync_state()
     {
        let (rec, mut rx) = recorder(false);
        rec.note(
            Note::Transaction {
                link: LinkKind::Zone(ZoneId::Zone1),
                op: "ReadFaults",
                latency: Duration::from_millis(42),
                attempts: 2,
            },
            stamp(10),
        );
        let ServiceEvent::Log(event) = rx.try_recv().unwrap() else {
            panic!("expected a log event");
        };
        let json = serde_json::to_string(&event).unwrap();
        // LOG-06: latency and retries. LOG-02: the sync state, inseparably.
        assert!(json.contains("\"latency_ms\":42"), "{json}");
        assert!(json.contains("\"retries\":1"), "{json}");
        assert!(json.contains("unsynchronised"), "{json}");
    }

    #[test]
    fn frame_records_are_written_only_when_configuration_asks_for_them() {
        let (off, mut rx_off) = recorder(false);
        off.frame(
            LinkKind::Steam,
            Direction::Tx,
            &[0x88, 0x01],
            stamp(1),
            None,
        );
        assert!(rx_off.try_recv().is_err(), "frames are off");

        let (on, mut rx_on) = recorder(true);
        on.frame(
            LinkKind::Steam,
            Direction::Rx,
            &[0x88, 0x01],
            stamp(1),
            Some("truncated before EOF".into()),
        );
        let ServiceEvent::Log(event) = rx_on.try_recv().unwrap() else {
            panic!("expected a frame record");
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("88 01"), "{json}");
        assert!(json.contains("truncated"), "{json}");
    }

    #[test]
    fn a_session_record_can_be_finished_with_the_maxima_the_engine_cannot_know() {
        let (rec, _rx) = recorder(false);
        let mut event = rec.finish(
            Note::Session {
                link: LinkKind::Zone(ZoneId::Zone2),
                command: CommandId(9),
                duration: Duration::from_secs(300),
                stop: StopReason::Commanded,
            },
            stamp(1000),
        );
        let LogEvent::Session(record) = &mut event else {
            panic!("expected a session record");
        };
        assert_eq!(record.max_valve_reported_c, None);
        record.max_valve_reported_c = Some(40.5);
        rec.emit(event);
    }

    #[test]
    fn an_accepted_command_is_attributed_to_the_caller_not_to_the_service() {
        let (rec, _rx) = recorder(false);
        let mut event = rec.finish(
            Note::Accepted {
                command: CommandId(4),
                requested: "start zone1 outlets {1} at 38.0 C for 300 s".into(),
            },
            stamp(1),
        );
        let source = RequestSource::LocalApi {
            session: 7,
            peer: "127.0.0.1".into(),
        };
        Recorder::attribute(&mut event, &source);
        let LogEvent::Command(record) = &event else {
            panic!("expected a command record");
        };
        // LOG-01: all five fields, and the source is the caller's.
        assert_eq!(record.service_boot, BootId(3));
        assert_eq!(record.command, CommandId(4));
        assert_eq!(record.pi_boot, PiBootId("boot-uuid".into()));
        assert!(matches!(record.source, RequestSource::LocalApi { .. }));
        assert!(record.requested.contains("zone1"));
    }

    #[test]
    fn a_rejection_names_the_check_and_reaches_the_live_stream_as_a_refusal() {
        let (rec, mut rx) = recorder(false);
        rec.rejection(
            CommandId(2),
            "the zone is cold".into(),
            "safety kernel",
            stamp(1),
        );
        // The durable record keeps both: which gate refused, and why.
        let ServiceEvent::Log(event) = rx.try_recv().unwrap() else {
            panic!("expected a rejection record");
        };
        let LogEvent::Rejected(record) = *event else {
            panic!("expected a rejection record");
        };
        assert_eq!(record.check, "safety kernel");
        assert_eq!(record.reason, "the zone is cold");

        // The live stream carries the reason. An operator shown the name of the
        // check cannot tell a latched zone from a stale token. `LOG-04`.
        let ServiceEvent::Refused { command, reason } = rx.try_recv().unwrap() else {
            panic!("expected a refusal");
        };
        assert_eq!(command, CommandId(2));
        assert_eq!(reason, "the zone is cold");
    }
}
