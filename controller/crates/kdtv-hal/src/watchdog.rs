//! The systemd watchdog: ready, and pet.
//!
//! `deploy/kdtvd.service` is `Type=notify` with `WatchdogSec=10s` and
//! `Restart=always`. Two things follow, and both are properties of the service
//! file rather than of this module:
//!
//! - **The service must say it is ready**, once, after configuration has
//!   validated and every link has been bound. Saying it earlier would report a
//!   service that cannot drive anything as healthy.
//! - **A missed pet restarts the daemon.** That is safe here only because boot
//!   state is OFF and no water state is persisted: a restart cannot resume a
//!   session, so the worst a watchdog reset does is stop the shower.
//!
//! [`Watchdog::interval`] is what systemd set, not what to pet at.
//! [`Watchdog::pet_interval`] halves it, which is the margin systemd's own
//! documentation asks for.
//!
//! When the daemon is run outside systemd there is no notify socket and no
//! watchdog. [`SystemdWatchdog`] then reports [`Watchdog::interval`] as `None`
//! and its calls are no-ops — the supervisor is expected to notice a `None`
//! interval and log it, because a production start with no watchdog is a
//! deployment mistake worth seeing.

use std::fmt;
use std::time::Duration;

/// Liveness reporting.
pub trait Watchdog: Send + Sync + fmt::Debug {
    /// Reports the service ready. Called once, after every link is bound.
    fn notify_ready(&self);

    /// Reports the service still alive.
    fn pet(&self);

    /// The interval systemd will restart the service after, or `None` when no
    /// watchdog is configured for this process.
    fn interval(&self) -> Option<Duration>;

    /// How often to call [`Watchdog::pet`]: half the interval, so one missed
    /// tick does not restart the daemon.
    fn pet_interval(&self) -> Option<Duration> {
        self.interval().map(|d| d / 2)
    }
}

/// The real one.
///
/// Both `notify_ready` and `pet` ignore errors from the notify socket, and that
/// is deliberate: the failure mode of a socket write that did not land is
/// systemd restarting the service, which is the safe direction. Turning it into
/// an error the caller must handle would put a failure path in the middle of the
/// tick loop that ends somewhere other than OFF.
#[derive(Clone, Debug)]
pub struct SystemdWatchdog {
    interval: Option<Duration>,
}

impl Default for SystemdWatchdog {
    fn default() -> Self {
        Self::from_environment()
    }
}

impl SystemdWatchdog {
    /// Reads `WATCHDOG_USEC` and `WATCHDOG_PID` from the environment, as
    /// systemd sets them.
    #[must_use]
    pub fn from_environment() -> Self {
        Self {
            interval: sd_notify::watchdog_enabled(),
        }
    }

    /// A watchdog with a stated interval, for tests and for the bench rig.
    #[must_use]
    pub const fn with_interval(interval: Option<Duration>) -> Self {
        Self { interval }
    }
}

impl Watchdog for SystemdWatchdog {
    fn notify_ready(&self) {
        // `sd_notify::notify` is a no-op when NOTIFY_SOCKET is unset, so this is
        // silent outside systemd rather than an error.
        let _ = sd_notify::notify(&[sd_notify::NotifyState::Ready]);
    }

    fn pet(&self) {
        let _ = sd_notify::notify(&[sd_notify::NotifyState::Watchdog]);
    }

    fn interval(&self) -> Option<Duration> {
        self.interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pet_interval_is_half_the_watchdog_interval() {
        let wd = SystemdWatchdog::with_interval(Some(Duration::from_secs(10)));
        assert_eq!(wd.interval(), Some(Duration::from_secs(10)));
        assert_eq!(wd.pet_interval(), Some(Duration::from_secs(5)));
    }

    /// `deploy/kdtvd.service` sets `WatchdogSec=10s`. If that moves, this is the
    /// number the tick loop is budgeted against.
    #[test]
    fn the_deployed_interval_leaves_room_for_a_missed_tick() {
        let wd = SystemdWatchdog::with_interval(Some(Duration::from_secs(10)));
        let pet = wd.pet_interval().unwrap();
        // A Saturn tick is 525 ms. Nine ticks fit inside the petting interval,
        // so a single slow transaction cannot restart the daemon.
        assert!(pet >= kdtv_proto::saturn::Timings::DOCUMENTED.tick * 9);
    }

    #[test]
    fn no_watchdog_is_a_state_the_caller_can_see() {
        let wd = SystemdWatchdog::with_interval(None);
        assert_eq!(wd.interval(), None);
        assert_eq!(wd.pet_interval(), None);
        // Calls outside systemd are silent, not fatal.
        wd.notify_ready();
        wd.pet();
    }
}
