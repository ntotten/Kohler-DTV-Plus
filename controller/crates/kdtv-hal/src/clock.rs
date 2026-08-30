//! Time, and the one place in the daemon's graph that reads it.
//!
//! `clippy.toml` forbids `std::time::Instant::now` and `std::time::SystemTime::now`
//! workspace-wide, so that every state machine takes time as a parameter and
//! stays deterministic. [`LinuxClock`] is what that ban points at: the trait is
//! the parameter, this is the implementation, and it is the only thing here that
//! looks at a clock.
//!
//! # The wall clock never travels alone
//!
//! The Pi 4 has no RTC. Before the first NTP sync its wall clock is whatever the
//! last shutdown left behind, or 1970. A frame log stamped in that window cannot
//! be correlated against anything outside the machine, and the failure is silent
//! — the stamps look like times.
//!
//! [`kdtv_telemetry::Stamp`] already builds that in: it has no constructor that
//! yields a wall time without an [`NtpSync`]. This module keeps the same shape at
//! the source. [`Clock::wall`] returns a [`WallClock`], which has no accessor for
//! the timestamp on its own — [`WallClock::into_parts`] hands over both or
//! neither.
//!
//! # Monotonic readings come from tokio
//!
//! [`LinuxClock`] reads `tokio::time::Instant`, not `std::time::Instant`. Two
//! reasons, and the first is the one that matters: under `tokio::time::pause()`
//! the whole clock becomes deterministic, so a ring-2 test can run a 20-minute
//! session in milliseconds against the real supervisor. The second is that it
//! keeps this crate honest about the `clippy.toml` ban rather than suppressing
//! it.
//!
//! Both readings are relative to an origin taken when the clock was built, so
//! [`Monotonic`] here is "nanoseconds since this service started", which is what
//! the log wants anyway.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kdtv_telemetry::{Monotonic, NtpSync, Stamp};

use crate::link::BoxedFuture;

/// Where `systemd-timesyncd` records that it has synchronised.
const TIMESYNC_DIR: &str = "/run/systemd/timesync";
/// The file it touches on first sync.
const SYNCHRONIZED: &str = "synchronized";

/// A wall-clock reading and the state that says whether it can be believed.
///
/// There is deliberately no accessor that returns the timestamp alone.
/// [`WallClock::into_parts`] gives both; [`WallClock::stamp`] gives the shape
/// everything downstream actually takes.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct WallClock {
    at: jiff::Timestamp,
    ntp: NtpSync,
}

impl WallClock {
    #[must_use]
    pub const fn new(at: jiff::Timestamp, ntp: NtpSync) -> Self {
        Self { at, ntp }
    }

    /// Both halves, or neither.
    #[must_use]
    pub const fn into_parts(self) -> (jiff::Timestamp, NtpSync) {
        (self.at, self.ntp)
    }

    /// The sync state on its own. Harmless: knowing the clock is untrustworthy
    /// without knowing what it says cannot mislead anyone.
    #[must_use]
    pub const fn ntp(self) -> NtpSync {
        self.ntp
    }

    /// Pairs this reading with a monotonic one, which is the only form the log
    /// records.
    #[must_use]
    pub fn stamp(self, monotonic: Monotonic) -> Stamp {
        Stamp::new(monotonic, self.at.as_second(), self.ntp)
    }
}

impl fmt::Display for WallClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({:?})", self.at, self.ntp)
    }
}

/// Whether the system clock has synchronised since boot.
///
/// Injectable so the three states can be tested without a running
/// `systemd-timesyncd`.
pub trait NtpProbe: Send + Sync + fmt::Debug {
    fn sync_state(&self) -> NtpSync;
}

/// Reads `systemd-timesyncd`'s marker.
///
/// | Observation | Result |
/// | --- | --- |
/// | `synchronized` exists | [`NtpSync::Synchronised`] |
/// | the directory exists, the file does not | [`NtpSync::Unsynchronised`] |
/// | the directory does not exist | [`NtpSync::Unknown`] |
///
/// The third row is not the same as the second: no `timesyncd` at all might mean
/// `chrony` is keeping the clock perfectly, or that nothing is. Recording it
/// distinctly is what stops a boot log claiming an unsynchronised clock it never
/// checked. `NtpSync::Unknown` is treated as untrustworthy everywhere it
/// matters, so the distinction costs nothing but says more.
#[derive(Clone, Debug)]
pub struct TimesyncdProbe {
    dir: PathBuf,
}

impl Default for TimesyncdProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl TimesyncdProbe {
    #[must_use]
    pub fn new() -> Self {
        Self {
            dir: PathBuf::from(TIMESYNC_DIR),
        }
    }

    /// The same probe against a different directory, for tests.
    #[must_use]
    pub fn rooted(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }
}

impl NtpProbe for TimesyncdProbe {
    fn sync_state(&self) -> NtpSync {
        if self.dir.join(SYNCHRONIZED).exists() {
            NtpSync::Synchronised
        } else if Path::new(&self.dir).is_dir() {
            NtpSync::Unsynchronised
        } else {
            NtpSync::Unknown
        }
    }
}

/// Monotonic time, wall time and sleeping.
pub trait Clock: Send + Sync + fmt::Debug {
    /// Nanoseconds since this clock's origin. Every interval in the system —
    /// session limits, response deadlines, RTD dwells — is computed from this,
    /// never from the wall clock, so an NTP step cannot shorten a session.
    fn monotonic(&self) -> Monotonic;

    /// The wall clock, inseparable from its sync state.
    fn wall(&self) -> WallClock;

    /// The pair, in the form the log records. This is what almost every caller
    /// wants.
    fn stamp(&self) -> Stamp {
        self.wall().stamp(self.monotonic())
    }

    /// Sleeps until a monotonic deadline. A deadline already past returns
    /// immediately.
    fn sleep_until(&self, deadline: Monotonic) -> BoxedFuture<'static, ()>;
}

/// The Linux clock.
#[derive(Clone, Debug)]
pub struct LinuxClock {
    origin: tokio::time::Instant,
    ntp: Arc<dyn NtpProbe>,
}

impl LinuxClock {
    /// Takes the origin now. Build one per service boot, at the top of `main`.
    #[must_use]
    pub fn new(ntp: Arc<dyn NtpProbe>) -> Self {
        Self {
            origin: tokio::time::Instant::now(),
            ntp,
        }
    }

    /// A clock reading `systemd-timesyncd`.
    #[must_use]
    pub fn systemd() -> Self {
        Self::new(Arc::new(TimesyncdProbe::new()))
    }

    fn deadline(&self, at: Monotonic) -> tokio::time::Instant {
        self.origin
            .checked_add(std::time::Duration::from_nanos(at.as_nanos()))
            .unwrap_or_else(tokio::time::Instant::now)
    }
}

impl Clock for LinuxClock {
    fn monotonic(&self) -> Monotonic {
        let elapsed = tokio::time::Instant::now().duration_since(self.origin);
        Monotonic::from_nanos(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the ban exists so state machines take time as a parameter; this is \
                  the implementation the Clock trait's parameter resolves to, and the \
                  only wall-clock read in the daemon's dependency graph"
    )]
    fn wall(&self) -> WallClock {
        let ntp = self.ntp.sync_state();
        match jiff::Timestamp::try_from(std::time::SystemTime::now()) {
            Ok(at) => WallClock::new(at, ntp),
            // A system clock outside jiff's range is not merely unsynchronised;
            // nothing is known about it. Recording Unknown rather than the
            // probe's answer keeps a nonsense stamp from being labelled
            // trustworthy.
            Err(_) => WallClock::new(jiff::Timestamp::UNIX_EPOCH, NtpSync::Unknown),
        }
    }

    fn sleep_until(&self, deadline: Monotonic) -> BoxedFuture<'static, ()> {
        let at = self.deadline(deadline);
        Box::pin(async move { tokio::time::sleep_until(at).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[derive(Debug)]
    struct Fixed(NtpSync);
    impl NtpProbe for Fixed {
        fn sync_state(&self) -> NtpSync {
            self.0
        }
    }

    #[tokio::test(start_paused = true)]
    async fn monotonic_time_advances_with_sleeping_and_starts_at_zero() {
        let clock = LinuxClock::new(Arc::new(Fixed(NtpSync::Synchronised)));
        assert_eq!(clock.monotonic().as_nanos(), 0);
        let deadline = Monotonic::from_nanos(525_000_000);
        clock.sleep_until(deadline).await;
        assert!(clock.monotonic().as_nanos() >= 525_000_000);
    }

    #[tokio::test(start_paused = true)]
    async fn a_deadline_already_past_returns_immediately() {
        let clock = LinuxClock::new(Arc::new(Fixed(NtpSync::Unknown)));
        tokio::time::advance(Duration::from_secs(10)).await;
        clock.sleep_until(Monotonic::from_nanos(1)).await;
        assert!(clock.monotonic().as_nanos() >= 10_000_000_000);
    }

    #[tokio::test(start_paused = true)]
    async fn the_sync_state_reaches_the_stamp() {
        for state in [
            NtpSync::Synchronised,
            NtpSync::Unsynchronised,
            NtpSync::Unknown,
        ] {
            let clock = LinuxClock::new(Arc::new(Fixed(state)));
            assert_eq!(clock.wall().ntp(), state);
            assert_eq!(clock.stamp().ntp, state);
            assert_eq!(clock.wall().into_parts().1, state);
        }
    }

    /// An unsynchronised boot must be visible in the log, not inferred from a
    /// suspicious-looking date.
    #[tokio::test(start_paused = true)]
    async fn an_unsynchronised_stamp_says_so_when_it_is_serialised() {
        let clock = LinuxClock::new(Arc::new(Fixed(NtpSync::Unsynchronised)));
        let json = serde_json::to_string(&clock.stamp()).unwrap();
        assert!(json.contains("unsynchronised"), "{json}");
    }

    #[test]
    fn the_timesyncd_probe_distinguishes_absent_from_unsynchronised() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("no-timesyncd-here");
        assert_eq!(
            TimesyncdProbe::rooted(&missing).sync_state(),
            NtpSync::Unknown
        );

        let present = dir.path().join("timesync");
        std::fs::create_dir_all(&present).unwrap();
        assert_eq!(
            TimesyncdProbe::rooted(&present).sync_state(),
            NtpSync::Unsynchronised
        );

        std::fs::write(present.join(SYNCHRONIZED), b"").unwrap();
        assert_eq!(
            TimesyncdProbe::rooted(&present).sync_state(),
            NtpSync::Synchronised
        );
        assert!(!NtpSync::Unknown.trustworthy());
    }

    #[test]
    fn a_wall_reading_hands_over_both_halves_or_neither() {
        let w = WallClock::new(jiff::Timestamp::UNIX_EPOCH, NtpSync::Unsynchronised);
        let (at, ntp) = w.into_parts();
        assert_eq!(at.as_second(), 0);
        assert_eq!(ntp, NtpSync::Unsynchronised);
        let s = w.stamp(Monotonic::from_nanos(7));
        assert_eq!(s.wall_unix_s, 0);
        assert_eq!(s.monotonic_ns.as_nanos(), 7);
        assert_eq!(s.ntp, NtpSync::Unsynchronised);
    }
}
