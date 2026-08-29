//! Time, always in pairs.

use serde::Serialize;
use std::fmt;
use std::time::Duration;

/// Whether the wall clock beside a stamp can be believed.
///
/// The Pi 4 has no RTC. A DS3231 module narrows the pre-sync window; it does not
/// remove the requirement to record which state a stamp was taken in.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NtpSync {
    /// The system clock has synchronised since boot.
    Synchronised,
    /// It has not. Wall-clock stamps from this period are approximate at best,
    /// and a frame log taken now cannot be correlated against anything external.
    Unsynchronised,
    /// The sync state could not be determined. Treated as unsynchronised
    /// everywhere it matters; recorded distinctly so the difference is visible.
    Unknown,
}

impl NtpSync {
    #[must_use]
    pub const fn trustworthy(self) -> bool {
        matches!(self, Self::Synchronised)
    }
}

/// A monotonic instant, as nanoseconds since an arbitrary origin.
///
/// Deliberately not `std::time::Instant`: this crate must be serialisable and
/// must be constructible in a test at an exact value. All interval arithmetic —
/// session limits, response deadlines, dwell times — uses this, never the wall
/// clock, so an NTP step cannot shorten or lengthen a session.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize)]
#[serde(transparent)]
pub struct Monotonic(u64);

impl Monotonic {
    #[must_use]
    pub const fn from_nanos(n: u64) -> Self {
        Self(n)
    }

    #[must_use]
    pub const fn as_nanos(self) -> u64 {
        self.0
    }

    /// Saturating, so a clock that appears to go backwards yields zero rather
    /// than an enormous interval.
    #[must_use]
    pub fn since(self, earlier: Self) -> Duration {
        Duration::from_nanos(self.0.saturating_sub(earlier.0))
    }

    #[must_use]
    pub fn checked_add(self, d: Duration) -> Option<Self> {
        u64::try_from(d.as_nanos())
            .ok()
            .and_then(|n| self.0.checked_add(n))
            .map(Self)
    }
}

/// A moment, recorded the only way this system records one.
///
/// The monotonic reading is what intervals are computed from. The wall clock is
/// for correlating against the outside world, and it is inseparable from the
/// sync state that says whether it can be believed.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize)]
pub struct Stamp {
    pub monotonic_ns: Monotonic,
    /// Unix seconds. Meaningless unless `ntp` is [`NtpSync::Synchronised`].
    pub wall_unix_s: i64,
    pub ntp: NtpSync,
}

impl Stamp {
    #[must_use]
    pub const fn new(monotonic: Monotonic, wall_unix_s: i64, ntp: NtpSync) -> Self {
        Self {
            monotonic_ns: monotonic,
            wall_unix_s,
            ntp,
        }
    }

    /// The interval between two stamps, from the monotonic reading only.
    #[must_use]
    pub fn since(self, earlier: Self) -> Duration {
        self.monotonic_ns.since(earlier.monotonic_ns)
    }
}

impl fmt::Display for Stamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mark = match self.ntp {
            NtpSync::Synchronised => "",
            NtpSync::Unsynchronised => " (unsynced)",
            NtpSync::Unknown => " (sync unknown)",
        };
        write!(
            f,
            "{}{mark} +{}ns",
            self.wall_unix_s,
            self.monotonic_ns.as_nanos()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intervals_come_from_the_monotonic_reading_not_the_wall_clock() {
        let a = Stamp::new(
            Monotonic::from_nanos(1_000_000_000),
            1_000,
            NtpSync::Unsynchronised,
        );
        // The wall clock steps backwards by an hour, as it does on first sync.
        let b = Stamp::new(
            Monotonic::from_nanos(3_000_000_000),
            -2_600,
            NtpSync::Synchronised,
        );
        assert_eq!(b.since(a), Duration::from_secs(2));
    }

    #[test]
    fn a_backwards_monotonic_reading_saturates_rather_than_wrapping() {
        let a = Monotonic::from_nanos(5_000);
        let b = Monotonic::from_nanos(1_000);
        assert_eq!(b.since(a), Duration::ZERO);
    }

    #[test]
    fn the_sync_state_travels_with_the_stamp_in_the_serialised_form() {
        let s = Stamp::new(
            Monotonic::from_nanos(1),
            1_756_500_000,
            NtpSync::Unsynchronised,
        );
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("unsynchronised"), "{json}");
        assert!(json.contains("wall_unix_s"));
        assert!(!NtpSync::Unsynchronised.trustworthy());
        assert!(!NtpSync::Unknown.trustworthy());
    }

    #[test]
    fn display_marks_an_untrustworthy_wall_clock() {
        let s = Stamp::new(Monotonic::from_nanos(1), 5, NtpSync::Unsynchronised);
        assert!(format!("{s}").contains("unsynced"));
    }
}
