//! The transcript: every byte that crossed a link, and the assertions that read it.
//!
//! **The oracle is the wire, not the state.** Every end-to-end assertion runs
//! against what the daemon actually transmitted, never against what the daemon
//! reports about itself. A service that believes it is off while putting an open
//! frame on the bus passes a state assertion and fails this one, and that is the
//! failure worth catching.
//!
//! Entries carry a monotonic offset from the start of the run. Wall-clock time is
//! deliberately absent: it is not needed to decide any of these questions, and
//! including it would make the transcript's answers depend on NTP.

use std::fmt::Write as _;
use std::time::Duration;

/// Which way a byte went, recorded by whichever side moved it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Direction {
    /// Emitted by the daemon under test.
    DaemonToDevice,
    /// Emitted by the emulated device.
    DeviceToDaemon,
}

/// One frame's worth of bytes, as they crossed the wire.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    pub at: Duration,
    pub direction: Direction,
    pub bytes: Vec<u8>,
}

impl Entry {
    #[must_use]
    pub fn hex(&self) -> String {
        let mut s = String::with_capacity(self.bytes.len() * 3);
        for (i, b) in self.bytes.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            let _ = write!(s, "{b:02X}");
        }
        s
    }
}

/// What crossed one link during a run.
#[derive(Clone, Default, Debug)]
pub struct Transcript {
    entries: Vec<Entry>,
}

/// Why an assertion about the transcript failed, with enough of the transcript
/// attached to diagnose it without re-running.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error("{what}\n{context}")]
pub struct TranscriptViolation {
    pub what: String,
    pub context: String,
}

impl Transcript {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, at: Duration, direction: Direction, bytes: &[u8]) {
        self.entries.push(Entry {
            at,
            direction,
            bytes: bytes.to_vec(),
        });
    }

    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Everything the daemon transmitted, in order.
    pub fn transmitted(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|e| e.direction == Direction::DaemonToDevice)
    }

    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.transmitted().next().is_none()
    }

    /// Total bytes the daemon put on the wire.
    ///
    /// The cheapest possible assertion, and a surprisingly strong one: a
    /// rejected request must transmit zero, and an external status read must not
    /// change this number at all.
    #[must_use]
    pub fn transmitted_bytes(&self) -> usize {
        self.transmitted().map(|e| e.bytes.len()).sum()
    }

    /// Render the whole transcript, for a failure message.
    #[must_use]
    pub fn render(&self) -> String {
        let mut s = String::new();
        for e in &self.entries {
            let arrow = match e.direction {
                Direction::DaemonToDevice => "-->",
                Direction::DeviceToDaemon => "<--",
            };
            let _ = writeln!(s, "  {:>9.3}s {arrow} {}", e.at.as_secs_f64(), e.hex());
        }
        if s.is_empty() {
            s.push_str("  (nothing crossed this link)\n");
        }
        s
    }

    fn violation(&self, what: impl Into<String>) -> TranscriptViolation {
        TranscriptViolation {
            what: what.into(),
            context: self.render(),
        }
    }

    /// Nothing was transmitted at all.
    ///
    /// The assertion behind "a rejected request changes no valve state": the
    /// service must refuse it before any frame is queued, not send something and
    /// then correct it.
    pub fn assert_silent(&self) -> Result<(), TranscriptViolation> {
        match self.transmitted().next() {
            None => Ok(()),
            Some(e) => Err(self.violation(format!(
                "expected no transmission, but the daemon sent {} at {:.3}s",
                e.hex(),
                e.at.as_secs_f64()
            ))),
        }
    }

    /// Every frame the daemon transmitted satisfies `is_allowed`.
    ///
    /// The caller supplies the predicate so this module stays free of protocol
    /// knowledge; the suite passes one that decodes the frame and requires it to
    /// match an allowlisted operation.
    pub fn assert_all_transmitted(
        &self,
        what: &str,
        is_allowed: impl Fn(&[u8]) -> bool,
    ) -> Result<(), TranscriptViolation> {
        for e in self.transmitted() {
            if !is_allowed(&e.bytes) {
                return Err(self.violation(format!(
                    "{what}: frame {} at {:.3}s is not permitted",
                    e.hex(),
                    e.at.as_secs_f64()
                )));
            }
        }
        Ok(())
    }

    /// The last frame the daemon transmitted matches `is_expected`.
    ///
    /// Used for "the run ends with an all-off": a service that stops water and
    /// then keeps chattering has not ended in the state the assertion claims.
    pub fn assert_last_transmitted(
        &self,
        what: &str,
        is_expected: impl Fn(&[u8]) -> bool,
    ) -> Result<(), TranscriptViolation> {
        match self.transmitted().last() {
            Some(e) if is_expected(&e.bytes) => Ok(()),
            Some(e) => Err(self.violation(format!(
                "{what}: the last frame was {} at {:.3}s",
                e.hex(),
                e.at.as_secs_f64()
            ))),
            None => Err(self.violation(format!("{what}: the daemon transmitted nothing"))),
        }
    }

    /// No transmitted frame matches `is_forbidden`.
    ///
    /// The shape behind "no start frame after a restart" and "no timer refresh
    /// was ever sent".
    pub fn assert_never_transmitted(
        &self,
        what: &str,
        is_forbidden: impl Fn(&[u8]) -> bool,
    ) -> Result<(), TranscriptViolation> {
        for e in self.transmitted() {
            if is_forbidden(&e.bytes) {
                return Err(self.violation(format!(
                    "{what}: found {} at {:.3}s",
                    e.hex(),
                    e.at.as_secs_f64()
                )));
            }
        }
        Ok(())
    }

    /// How long after `from` the first frame matching `is_stop` was transmitted.
    ///
    /// This measures the service's own reaction, and nothing more. It is **not**
    /// a fail-off latency: that is measured at the outlet, against flow, on real
    /// hardware, during commissioning. Naming it `since` rather than `latency`
    /// is deliberate.
    #[must_use]
    pub fn first_stop_since(
        &self,
        from: Duration,
        is_stop: impl Fn(&[u8]) -> bool,
    ) -> Option<Duration> {
        self.transmitted()
            .find(|e| e.at >= from && is_stop(&e.bytes))
            .map(|e| e.at.saturating_sub(from))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> Transcript {
        let mut t = Transcript::new();
        t.record(
            Duration::from_millis(0),
            Direction::DaemonToDevice,
            &[0xAA, 0x55, 0x03, 0x0F],
        );
        t.record(
            Duration::from_millis(12),
            Direction::DeviceToDaemon,
            &[0xAA, 0x55, 0x00, 0x0F],
        );
        t.record(
            Duration::from_millis(525),
            Direction::DaemonToDevice,
            &[0xAA, 0x55, 0x03, 0x87],
        );
        t
    }

    #[test]
    fn only_daemon_frames_count_as_transmitted() {
        assert_eq!(t().transmitted().count(), 2);
        assert_eq!(t().transmitted_bytes(), 8);
        assert!(!t().is_silent());
    }

    #[test]
    fn a_silent_transcript_is_silent_even_with_device_chatter() {
        let mut s = Transcript::new();
        s.record(Duration::ZERO, Direction::DeviceToDaemon, &[0xFF]);
        assert!(
            s.is_silent(),
            "a device talking to itself is not a transmission"
        );
        assert!(s.assert_silent().is_ok());
    }

    #[test]
    fn assert_silent_names_the_offending_frame() {
        let err = t().assert_silent().expect_err("should not be silent");
        assert!(err.what.contains("AA 55 03 0F"), "{}", err.what);
        assert!(
            err.context.contains("-->"),
            "the failure carries the transcript"
        );
    }

    #[test]
    fn never_transmitted_finds_a_forbidden_frame_anywhere_in_the_run() {
        let err = t()
            .assert_never_transmitted("no outlet write", |b| b.get(3) == Some(&0x87))
            .expect_err("0x87 was transmitted");
        assert!(err.what.contains("0.525"), "{}", err.what);
    }

    #[test]
    fn last_transmitted_ignores_device_replies_after_it() {
        let mut s = t();
        s.record(
            Duration::from_millis(600),
            Direction::DeviceToDaemon,
            &[0x00],
        );
        assert!(
            s.assert_last_transmitted("ends with 0x87", |b| b.get(3) == Some(&0x87))
                .is_ok()
        );
    }

    #[test]
    fn all_transmitted_rejects_the_first_frame_that_fails() {
        let err = t()
            .assert_all_transmitted("only discovery", |b| b.get(3) == Some(&0x0F))
            .expect_err("0x87 is not discovery");
        assert!(err.what.contains("AA 55 03 87"), "{}", err.what);
    }

    #[test]
    fn first_stop_since_measures_from_the_given_moment_only() {
        let s = t();
        let d = s.first_stop_since(Duration::from_millis(100), |b| b.get(3) == Some(&0x87));
        assert_eq!(d, Some(Duration::from_millis(425)));
        // A stop that happened before the moment asked about does not count.
        assert_eq!(
            s.first_stop_since(Duration::from_millis(600), |_| true),
            None
        );
    }

    #[test]
    fn an_empty_transcript_renders_something_a_human_can_read() {
        assert!(Transcript::new().render().contains("nothing crossed"));
    }
}
