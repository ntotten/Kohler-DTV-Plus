//! Frame capture and session records.
//!
//! `DESIGN.md` requires "raw RX/TX frame bytes with monotonic and
//! wall-clock timestamps" among the required logs. The frame log is also the
//! oracle the end-to-end tests assert against: what the service *did* on the
//! wire, rather than what it believes about itself. A service that reports
//! itself off while transmitting an open frame passes a state assertion and
//! fails this one.

use crate::stamp::Stamp;
use kdtv_units::LinkKind;
use serde::Serialize;
use std::time::Duration;

/// Which way a frame went.
///
/// On a half-duplex bus with an automatic-direction converter there is no local
/// echo, so this is recorded by the code that transmitted or received it — never
/// inferred from the wire.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Tx,
    Rx,
}

/// One frame, exactly as it appeared on the wire.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct FrameRecord {
    pub link: LinkKind,
    pub direction: Direction,
    /// The raw bytes, hex-encoded. Not the decoded form — a frame that failed to
    /// decode is the one most worth having.
    pub bytes_hex: String,
    /// Why decoding failed, when it did. `None` means it decoded.
    pub decode_error: Option<String>,
    pub at: Stamp,
}

impl FrameRecord {
    #[must_use]
    pub fn new(link: LinkKind, direction: Direction, bytes: &[u8], at: Stamp) -> Self {
        Self {
            link,
            direction,
            bytes_hex: hex_encode(bytes),
            decode_error: None,
            at,
        }
    }

    #[must_use]
    pub fn with_decode_error(mut self, why: impl Into<String>) -> Self {
        self.decode_error = Some(why.into());
        self
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        // Writing to a String cannot fail; the result is discarded deliberately.
        let _ = write!(s, "{b:02X}");
    }
    s
}

/// Why a session ended. Recorded on every session, because "it stopped" is not
/// a diagnosis.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum StopReason {
    /// The operator asked.
    Commanded,
    /// The service's own 20-minute limit. Not a fault.
    SessionLimit,
    /// A safety escalation stopped it. Carries what tripped.
    Safety { event: String },
    /// The link went away and the service commanded all-off on the way down.
    LinkLost,
    /// The service is shutting down.
    ServiceStopping,
}

/// One water session, start to stop.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct SessionRecord {
    pub link: LinkKind,
    pub command: kdtv_units::CommandId,
    pub started: Stamp,
    pub ended: Stamp,
    pub duration_s: u64,
    pub stop: StopReason,
    /// The highest temperature the valve reported during the session.
    pub max_valve_reported_c: Option<f32>,
}

impl SessionRecord {
    #[must_use]
    pub fn new(
        link: LinkKind,
        command: kdtv_units::CommandId,
        started: Stamp,
        ended: Stamp,
        stop: StopReason,
    ) -> Self {
        Self {
            link,
            command,
            started,
            ended,
            duration_s: ended.since(started).as_secs(),
            stop,
            max_valve_reported_c: None,
        }
    }

    #[must_use]
    pub const fn duration(&self) -> Duration {
        Duration::from_secs(self.duration_s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stamp::{Monotonic, NtpSync};
    use kdtv_units::{CommandId, ZoneId};

    fn at(ns: u64) -> Stamp {
        Stamp::new(
            Monotonic::from_nanos(ns),
            1_756_500_000,
            NtpSync::Synchronised,
        )
    }

    #[test]
    fn a_frame_record_keeps_the_raw_bytes_readable() {
        let r = FrameRecord::new(
            LinkKind::Zone(ZoneId::Zone1),
            Direction::Tx,
            &[0xAA, 0x55, 0x03, 0x02, 0x00, 0xFB],
            at(1),
        );
        assert_eq!(r.bytes_hex, "AA 55 03 02 00 FB");
        assert!(r.decode_error.is_none());
    }

    #[test]
    fn a_frame_that_failed_to_decode_is_still_recorded_in_full() {
        let r = FrameRecord::new(LinkKind::Steam, Direction::Rx, &[0x88, 0x01], at(2))
            .with_decode_error("truncated before EOF");
        assert_eq!(r.bytes_hex, "88 01");
        assert_eq!(r.decode_error.as_deref(), Some("truncated before EOF"));
    }

    #[test]
    fn empty_frames_encode_to_an_empty_string_not_a_panic() {
        let r = FrameRecord::new(LinkKind::Steam, Direction::Rx, &[], at(3));
        assert_eq!(r.bytes_hex, "");
    }

    #[test]
    fn session_duration_comes_from_the_monotonic_stamps() {
        let s = SessionRecord::new(
            LinkKind::Zone(ZoneId::Zone2),
            CommandId(3),
            at(0),
            at(300_000_000_000),
            StopReason::SessionLimit,
        );
        assert_eq!(s.duration(), Duration::from_secs(300));
        assert!(serde_json::to_string(&s).unwrap().contains("session_limit"));
    }
}
