//! Ownership of a link, and the one-way trip into latched.

use crate::event::LatchReason;
use crate::grant::OperatorAck;
use crate::session::SessionDeadline;
use kdtv_telemetry::Monotonic;

/// The phases a link passes through, holding its port by value.
///
/// The design requires that losing a valid response makes the service attempt
/// all-off on the affected zone, close that zone's port, **and** latch the zone
/// unavailable. Those three are one action, and this type is how they stay one:
/// [`ZoneAuthority::latch`] takes `self` by value and returns a variant with
/// nowhere to put the link, so the port is dropped — and therefore closed — by
/// the language rather than by remembering to.
///
/// "All-off was sent but the port is still open" is not a state this type can
/// represent, which is a stronger guarantee than any test.
///
/// The path into `Running` goes only through `ReadyOff`, and only carrying a
/// grant. There is no edge from the cold start to running, so a restart cannot
/// resume a session.
#[derive(Debug)]
pub enum ZoneAuthority<L> {
    /// Address management is legal here and nowhere else.
    Discovery { link: L },
    /// Identity read, faults read, and the all-off confirmed.
    ReadyOff { link: L },
    /// Water is on, with a deadline that cannot be extended.
    Running { link: L, session: SessionDeadline },
    /// Water is on but held. The session deadline keeps running: pausing is not
    /// a way to have a longer session.
    Paused { link: L, session: SessionDeadline },
    /// Unavailable until a person acknowledges it. There is no link here — that
    /// is the point.
    Latched {
        reason: LatchReason,
        since: Monotonic,
        acknowledged: bool,
    },
}

/// Returned when a caller reaches for a link that is no longer there.
#[derive(Clone, PartialEq, Debug, thiserror::Error)]
#[error("the link is latched: {reason:?}")]
pub struct IsLatched {
    pub reason: LatchReason,
}

impl<L> ZoneAuthority<L> {
    /// Every link starts in discovery, holding its port.
    pub const fn new(link: L) -> Self {
        Self::Discovery { link }
    }

    #[must_use]
    pub const fn phase(&self) -> &'static str {
        match self {
            Self::Discovery { .. } => "discovery",
            Self::ReadyOff { .. } => "ready-off",
            Self::Running { .. } => "running",
            Self::Paused { .. } => "paused",
            Self::Latched { .. } => "latched",
        }
    }

    #[must_use]
    pub const fn is_latched(&self) -> bool {
        matches!(self, Self::Latched { .. })
    }

    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. } | Self::Paused { .. })
    }

    /// The link, if there still is one.
    pub fn link_mut(&mut self) -> Result<&mut L, IsLatched> {
        match self {
            Self::Discovery { link }
            | Self::ReadyOff { link }
            | Self::Running { link, .. }
            | Self::Paused { link, .. } => Ok(link),
            Self::Latched { reason, .. } => Err(IsLatched {
                reason: reason.clone(),
            }),
        }
    }

    /// The session deadline, while one is running.
    #[must_use]
    pub const fn session(&self) -> Option<&SessionDeadline> {
        match self {
            Self::Running { session, .. } | Self::Paused { session, .. } => Some(session),
            _ => None,
        }
    }

    /// Take the link down. Consumes `self`; the link has nowhere to go and is
    /// dropped, which closes it.
    #[must_use]
    pub fn latch(self, reason: LatchReason, at: Monotonic) -> Self {
        Self::Latched {
            reason,
            since: at,
            acknowledged: false,
        }
    }

    /// Discovery is finished and the valve is confirmed off.
    ///
    /// This is also where the discovery capability ends: the caller drops its
    /// discovery token on this transition, so address management stops being
    /// encodable.
    #[must_use]
    pub fn ready(self) -> Self {
        match self {
            Self::Discovery { link }
            | Self::ReadyOff { link }
            | Self::Running { link, .. }
            | Self::Paused { link, .. } => Self::ReadyOff { link },
            latched @ Self::Latched { .. } => latched,
        }
    }

    /// Start a session. Only from `ReadyOff`, and the caller must already hold a
    /// grant — this method does not mint one and cannot.
    pub fn start(self, session: SessionDeadline) -> Result<Self, Self> {
        match self {
            Self::ReadyOff { link } => Ok(Self::Running { link, session }),
            other => Err(other),
        }
    }

    #[must_use]
    pub fn pause(self) -> Self {
        match self {
            Self::Running { link, session } => Self::Paused { link, session },
            other => other,
        }
    }

    #[must_use]
    pub fn resume(self) -> Self {
        match self {
            Self::Paused { link, session } => Self::Running { link, session },
            other => other,
        }
    }

    /// Acknowledge a latched link.
    ///
    /// Marks it acknowledged; it does **not** hand back a link, because the port
    /// was closed when it latched. Reopening is the service's job, and it goes
    /// through discovery again — which is the only honest way back, since
    /// nothing here knows whether the fault is gone.
    pub fn acknowledge(self, _ack: &OperatorAck) -> Result<Self, Self> {
        match self {
            Self::Latched { reason, since, .. } => Ok(Self::Latched {
                reason,
                since,
                acknowledged: true,
            }),
            other => Err(other),
        }
    }

    /// True once a person has acknowledged the latch and the service may attempt
    /// to bring the link back.
    #[must_use]
    pub const fn may_reopen(&self) -> bool {
        matches!(
            self,
            Self::Latched {
                acknowledged: true,
                ..
            }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_units::SessionDuration;
    use std::time::Duration;

    /// Stands in for a serial port, and records whether it was dropped.
    #[derive(Debug)]
    struct FakeLink(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Drop for FakeLink {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn at(s: u64) -> Monotonic {
        Monotonic::from_nanos(s * 1_000_000_000)
    }

    fn session() -> SessionDeadline {
        SessionDeadline::start(
            at(0),
            SessionDuration::clamped(Duration::from_secs(300)),
            SessionDuration::clamped(Duration::from_secs(1200)),
        )
    }

    #[test]
    fn latching_drops_the_link_which_is_what_closes_the_port() {
        let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let a = ZoneAuthority::new(FakeLink(dropped.clone()));
        assert!(!dropped.load(std::sync::atomic::Ordering::SeqCst));

        let a = a.latch(LatchReason::PortLost, at(5));
        assert!(
            dropped.load(std::sync::atomic::Ordering::SeqCst),
            "the link must be dropped by latching, not merely forgotten"
        );
        assert!(a.is_latched());
    }

    #[test]
    fn a_latched_authority_hands_out_no_link() {
        let mut a: ZoneAuthority<FakeLink> = ZoneAuthority::Latched {
            reason: LatchReason::WatchdogMissed,
            since: at(1),
            acknowledged: false,
        };
        assert!(a.link_mut().is_err());
    }

    #[test]
    fn there_is_no_edge_from_discovery_to_running() {
        let a = ZoneAuthority::new(());
        // start() only accepts ReadyOff; from Discovery it refuses and hands the
        // authority back unchanged.
        let back = a
            .start(session())
            .expect_err("discovery cannot start water");
        assert_eq!(back.phase(), "discovery");
    }

    #[test]
    fn the_route_to_running_is_discovery_then_ready_then_start() {
        let a = ZoneAuthority::new(()).ready();
        assert_eq!(a.phase(), "ready-off");
        let a = a.start(session()).expect("ready-off may start");
        assert_eq!(a.phase(), "running");
        assert!(a.is_running());
    }

    #[test]
    fn pausing_does_not_extend_the_session() {
        let a = ZoneAuthority::new(()).ready().start(session()).unwrap();
        let expires = a.session().unwrap().expires_at();
        let a = a.pause();
        assert_eq!(
            a.session().unwrap().expires_at(),
            expires,
            "the deadline is untouched"
        );
        let a = a.resume();
        assert_eq!(a.session().unwrap().expires_at(), expires);
    }

    #[test]
    fn acknowledging_does_not_hand_back_a_link() {
        let a: ZoneAuthority<()> = ZoneAuthority::Latched {
            reason: LatchReason::PortLost,
            since: at(1),
            acknowledged: false,
        };
        assert!(!a.may_reopen());
        let a = a
            .acknowledge(&OperatorAck::issue(kdtv_units::CommandId(9)))
            .unwrap();
        assert!(a.may_reopen(), "the service may now try to reopen");
        assert!(
            a.is_latched(),
            "but it is still latched until it actually does"
        );
    }

    #[test]
    fn acknowledging_something_that_is_not_latched_is_refused() {
        let a = ZoneAuthority::new(()).ready();
        assert!(
            a.acknowledge(&OperatorAck::issue(kdtv_units::CommandId(1)))
                .is_err()
        );
    }

    #[test]
    fn latching_an_already_latched_link_keeps_it_latched() {
        let a: ZoneAuthority<()> = ZoneAuthority::Latched {
            reason: LatchReason::PortLost,
            since: at(1),
            acknowledged: true,
        };
        let a = a.ready();
        assert!(a.is_latched(), "ready() must not resurrect a latched link");
    }
}
