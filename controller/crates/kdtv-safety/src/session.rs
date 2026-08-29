//! The session limit. Monotonic, and with no way to extend it.

use kdtv_telemetry::Monotonic;
use kdtv_units::SessionDuration;
use std::time::Duration;

/// When a running session must stop.
///
/// Built from a monotonic reading, so a wall-clock step — the first NTP sync
/// after a boot with no RTC, say — cannot lengthen or shorten a session.
///
/// There is no `extend`, no `refresh`, no `set_deadline` and no `+=`. The design
/// requires that no keepalive may extend a session automatically, and the way to
/// guarantee that is for the operation not to exist. A longer session is a new
/// session, deliberately started.
///
/// # The valve's own timer
///
/// The Prompt 3 has a 1800-second runtime timer of its own, and this limit sits
/// below it. Whether that timer is an *independent* backstop is unresolved: one
/// source says its counter resets on any valid received command, under which
/// ordinary 525 ms polling would refresh it continuously and no such backstop
/// exists. This service therefore treats its own limit as the only one it can
/// rely on. Capture question 5.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct SessionDeadline {
    started: Monotonic,
    expires: Monotonic,
}

impl SessionDeadline {
    /// 20 minutes. Restated from `kdtv_units` so the number is visible where the
    /// deadline is built.
    pub const HARD_LIMIT: Duration = SessionDuration::HARD_LIMIT;

    /// Start a session.
    ///
    /// The effective length is the shortest of: what was requested, the caller's
    /// configured cap, and the hard limit. A cap can only shorten.
    #[must_use]
    pub fn start(now: Monotonic, requested: SessionDuration, cap: SessionDuration) -> Self {
        let d = requested.get().min(cap.get()).min(Self::HARD_LIMIT);
        let expires = now.checked_add(d).unwrap_or(now);
        Self {
            started: now,
            expires,
        }
    }

    #[must_use]
    pub fn expired(&self, now: Monotonic) -> bool {
        now >= self.expires
    }

    #[must_use]
    pub fn remaining(&self, now: Monotonic) -> Duration {
        self.expires.since(now)
    }

    #[must_use]
    pub fn elapsed(&self, now: Monotonic) -> Duration {
        now.since(self.started)
    }

    #[must_use]
    pub const fn expires_at(&self) -> Monotonic {
        self.expires
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64) -> Monotonic {
        Monotonic::from_nanos(secs.saturating_mul(1_000_000_000))
    }

    fn dur(secs: u64) -> SessionDuration {
        SessionDuration::clamped(Duration::from_secs(secs))
    }

    #[test]
    fn a_session_expires_at_its_deadline_and_not_before() {
        let d = SessionDeadline::start(at(0), dur(300), dur(1200));
        assert!(!d.expired(at(299)));
        assert!(d.expired(at(300)));
        assert_eq!(d.remaining(at(120)), Duration::from_secs(180));
    }

    #[test]
    fn nothing_can_ask_for_longer_than_the_hard_limit() {
        // Both the request and the cap try to exceed it; SessionDuration has
        // already clamped them, and the deadline clamps again.
        let d = SessionDeadline::start(at(0), dur(86_400), dur(86_400));
        assert!(d.expired(at(1200)));
        assert_eq!(d.remaining(at(0)), SessionDeadline::HARD_LIMIT);
    }

    #[test]
    fn a_configured_cap_can_only_shorten() {
        let d = SessionDeadline::start(at(0), dur(1200), dur(600));
        assert!(d.expired(at(600)));
    }

    #[test]
    fn the_deadline_ignores_the_wall_clock_entirely() {
        // There is no wall clock in this type. The test is the signature: the
        // only time input is Monotonic, so an NTP step cannot reach it.
        let d = SessionDeadline::start(at(100), dur(300), dur(1200));
        assert_eq!(d.expires_at(), at(400));
        assert_eq!(d.elapsed(at(250)), Duration::from_secs(150));
    }

    #[test]
    fn remaining_saturates_rather_than_underflowing_past_the_deadline() {
        let d = SessionDeadline::start(at(0), dur(60), dur(1200));
        assert_eq!(d.remaining(at(120)), Duration::ZERO);
    }
}
