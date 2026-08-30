//! Session durations. Values only — the deadlines built from them live in
//! `kdtv-safety`, which owns the monotonic clock.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long a water session may run.
///
/// The hard limit is 20 minutes (`DESIGN.md` § Safety boundary rule
/// 7). It sits below the Prompt 3 valve's own 1800-second stop.
///
/// **No keepalive may extend a session automatically.** There is no `extend`,
/// no `refresh` and no setter on this type or on the deadline built from it.
///
/// A note on the valve's own timer, because the sources disagree and it matters:
/// `DESIGN.md` rule 7 says never sending the refresh "leaves the
/// valve's timer as an independent hardware backstop", while
/// `research/xagon0/docs/protocols/saturn-protocol.md` § Prompt 3 Timeout says
/// the counter "resets on any valid received command" — under which ordinary
/// 525 ms polling would refresh it continuously and no such backstop would
/// exist. `[?]` Unresolved; it is packet-capture question 5. This service's own
/// 20-minute limit is therefore treated as the only limit it can rely on, and
/// the valve timer is not counted as a second one until a capture settles it.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionDuration(Duration);

impl SessionDuration {
    /// 20 minutes.
    pub const HARD_LIMIT: Duration = Duration::from_secs(1200);

    /// Saturates at the hard limit. A caller asking for an hour gets 20 minutes;
    /// there is no configuration that widens this.
    #[must_use]
    pub fn clamped(requested: Duration) -> Self {
        Self(if requested > Self::HARD_LIMIT {
            Self::HARD_LIMIT
        } else {
            requested
        })
    }

    pub fn try_new(requested: Duration) -> Result<Self, SessionError> {
        if requested.is_zero() {
            return Err(SessionError::Zero);
        }
        if requested > Self::HARD_LIMIT {
            return Err(SessionError::TooLong {
                requested_s: requested.as_secs(),
                limit_s: Self::HARD_LIMIT.as_secs(),
            });
        }
        Ok(Self(requested))
    }

    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum SessionError {
    #[error("a session of zero length is not a session")]
    Zero,
    #[error("{requested_s} s exceeds the {limit_s} s hard limit")]
    TooLong { requested_s: u64, limit_s: u64 },
}

/// A steam session length, in whole minutes.
///
/// 1 to 20 minutes, default 10 (`HARDWARE.md` § 12, `[K][B]`).
///
/// **Zero is not representable.** `steamTimerSetTime = 0` disables the
/// generator's automatic shutoff and leaves it in manual control
/// (`research/xagon0/docs/devices/steam-generator.md` § Timer System). Since the
/// generator's own auto-shutoff is the only backstop that survives this service
/// dying, sending zero would remove the one protection the hard-link-loss case
/// depends on.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SteamMinutes(u8);

impl SteamMinutes {
    pub const MIN: u8 = 1;
    pub const MAX: u8 = 20;
    pub const DEFAULT: Self = Self(10);

    pub fn try_new(m: u8) -> Result<Self, SteamMinutesError> {
        if m < Self::MIN {
            return Err(SteamMinutesError::WouldDisableAutoShutoff { requested: m });
        }
        if m > Self::MAX {
            return Err(SteamMinutesError::TooLong {
                requested: m,
                limit: Self::MAX,
            });
        }
        Ok(Self(m))
    }

    #[must_use]
    pub fn clamped(m: u8) -> Self {
        Self(m.clamp(Self::MIN, Self::MAX))
    }

    /// The byte to put on the wire. Never zero.
    #[must_use]
    pub const fn wire(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn as_duration(self) -> Duration {
        Duration::from_secs(u64::from(self.0) * 60)
    }
}

impl Default for SteamMinutes {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum SteamMinutesError {
    #[error("{requested} minutes would disable the generator's automatic shutoff")]
    WouldDisableAutoShutoff { requested: u8 },
    #[error("{requested} minutes exceeds the {limit} minute maximum")]
    TooLong { requested: u8, limit: u8 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_design_sess_01_the_hard_limit_is_twenty_minutes() {
        assert_eq!(SessionDuration::HARD_LIMIT.as_secs(), 1200);
    }

    #[test]
    fn req_valve_control_timer_05_req_design_sess_03_the_hard_limit_sits_below_the_prompt_three_stop()
     {
        // The valve's own stop is 1800 s. Ours must be strictly below it so the
        // service stops first even if the valve's timer never becomes a backstop.
        assert!(SessionDuration::HARD_LIMIT.as_secs() < 1800);
    }

    #[test]
    fn nothing_can_ask_for_longer() {
        let v = SessionDuration::clamped(Duration::from_secs(86_400));
        assert_eq!(v.get(), SessionDuration::HARD_LIMIT);
        assert!(matches!(
            SessionDuration::try_new(Duration::from_secs(1201)),
            Err(SessionError::TooLong { .. })
        ));
    }

    #[test]
    fn req_steam_generator_steam_18_steam_zero_is_unrepresentable_because_it_disables_the_shutoff()
    {
        assert!(matches!(
            SteamMinutes::try_new(0),
            Err(SteamMinutesError::WouldDisableAutoShutoff { .. })
        ));
        assert_eq!(SteamMinutes::clamped(0).wire(), 1);
        // The property that matters on the wire.
        for m in 0..=255u8 {
            assert_ne!(SteamMinutes::clamped(m).wire(), 0);
        }
    }

    #[test]
    fn req_hardware_steam_11_steam_default_is_ten_minutes() {
        assert_eq!(SteamMinutes::default().wire(), 10);
        assert_eq!(
            SteamMinutes::DEFAULT.as_duration(),
            Duration::from_secs(600)
        );
    }
}
