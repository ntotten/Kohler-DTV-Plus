//! The one place water is authorised, and the one place faults are ruled on.

use crate::event::{Effect, FaultScope, LatchReason, SafetyEvent};
use crate::grant::{Denial, OpenGrant, StartAuthorization, ValidatedStart};
use kdtv_telemetry::Monotonic;
use kdtv_units::{BootId, LinkKind, SessionDuration, SlotSet, ZoneId};
use smallvec::{SmallVec, smallvec};

/// The resolved bounds this kernel enforces.
///
/// Passed in rather than read from configuration, so a safety decision cannot
/// depend on how a file was parsed. The service resolves these once at startup,
/// taking the tighter of the compiled-in constant and anything configured.
#[derive(Clone, Debug)]
pub struct Bounds {
    /// The longest a session may run. Already clamped to the hard limit.
    pub session_cap: SessionDuration,
    /// Which outlet slots each zone actually has. A slot outside this set cannot
    /// be opened.
    pub configured_outlets: [(ZoneId, SlotSet); 2],
}

impl Bounds {
    fn outlets_for(&self, zone: ZoneId) -> SlotSet {
        self.configured_outlets
            .iter()
            .find(|(z, _)| *z == zone)
            .map_or(SlotSet::EMPTY, |(_, s)| *s)
    }
}

/// What the kernel believes about one link.
#[derive(Clone, PartialEq, Debug)]
pub enum LinkState {
    /// Not yet confirmed off. Cannot start.
    Cold,
    /// Confirmed off and healthy.
    Ready,
    /// Water is on.
    Running,
    /// Unavailable until acknowledged.
    Latched {
        reason: LatchReason,
        acknowledged: bool,
    },
}

/// Rules on every safety question in the service.
///
/// Sans-IO: it returns [`Effect`]s and never performs them, which is what lets
/// the whole escalation matrix run at its real constants in microseconds.
#[derive(Debug)]
pub struct SafetyKernel {
    boot: BootId,
    bounds: Bounds,
    links: Vec<(LinkKind, LinkState)>,
}

impl SafetyKernel {
    #[must_use]
    pub fn new(boot: BootId, bounds: Bounds) -> Self {
        // Every link starts cold. Boot state is OFF, always, with no prior state
        // restored — a restart cannot resume a session because there is nothing
        // persisted for it to resume from.
        let links = LinkKind::ALL
            .iter()
            .map(|k| (*k, LinkState::Cold))
            .collect();
        Self {
            boot,
            bounds,
            links,
        }
    }

    #[must_use]
    pub fn state(&self, link: LinkKind) -> &LinkState {
        self.links
            .iter()
            .find(|(k, _)| *k == link)
            .map_or(&LinkState::Cold, |(_, s)| s)
    }

    fn set(&mut self, link: LinkKind, state: LinkState) {
        if let Some(entry) = self.links.iter_mut().find(|(k, _)| *k == link) {
            entry.1 = state;
        }
    }

    /// A link has completed discovery and confirmed itself off.
    pub fn mark_ready(&mut self, link: LinkKind) {
        if !matches!(self.state(link), LinkState::Latched { .. }) {
            self.set(link, LinkState::Ready);
        }
    }

    /// **The only way to obtain an [`OpenGrant`].**
    ///
    /// Everything that must be true before water moves is checked here, in one
    /// function a reviewer can read end to end:
    ///
    /// - the authorisation was minted in *this* service boot, so a restart
    ///   invalidates it and a start cannot be replayed;
    /// - the authorisation names the same command as the request, so one cannot
    ///   be used to authorise another;
    /// - the authorisation names the same command as the request, so one cannot
    ///   be used to authorise another;
    /// - the zone is not latched;
    /// - the zone is ready — not cold, so the boot sequence has confirmed it off;
    /// - at least one outlet was asked for, and every one is configured.
    ///
    /// A refusal returns a [`Denial`] and changes nothing. No frame is queued and
    /// no state moves, which is the distinction the design draws between invalid
    /// input and invalid wire data.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "taking the authorisation by value is the security property, not an \
                  oversight: it is spent by this call and cannot be presented twice. \
                  Clippy's suggestion to make it Copy is exactly what must not happen."
    )]
    pub fn authorize_open(
        &mut self,
        req: &ValidatedStart,
        auth: StartAuthorization,
    ) -> Result<OpenGrant, Denial> {
        if auth.boot() != self.boot {
            return Err(Denial::StaleSession {
                token_boot: auth.boot(),
                current_boot: self.boot,
            });
        }
        // The authorisation names the command it was issued for. If the request
        // carries a different one, something has paired an authorisation with a
        // request it was not minted for, and no grant comes out of that.
        if auth.command() != req.command {
            return Err(Denial::MismatchedCommand {
                authorised: auth.command(),
                requested: req.command,
            });
        }
        let link = LinkKind::Zone(req.zone);
        match self.state(link) {
            LinkState::Latched { reason, .. } => {
                return Err(Denial::ZoneLatched {
                    zone: req.zone,
                    reason: reason.clone(),
                });
            }
            LinkState::Cold => {
                return Err(Denial::NotReady {
                    zone: req.zone,
                    phase: "cold",
                });
            }
            LinkState::Running => {
                return Err(Denial::NotReady {
                    zone: req.zone,
                    phase: "running",
                });
            }
            LinkState::Ready => {}
        }
        if req.outlets.is_empty() {
            return Err(Denial::NoOutlets);
        }
        let configured = self.bounds.outlets_for(req.zone);
        if let Some(bad) = req.outlets.difference(configured).iter().next() {
            return Err(Denial::UnconfiguredOutlet(bad));
        }
        self.set(link, LinkState::Running);
        Ok(OpenGrant::new(req.zone, req.command, self.boot))
    }

    /// The session cap this kernel enforces.
    #[must_use]
    pub fn session_cap(&self) -> SessionDuration {
        self.bounds.session_cap
    }

    /// Water stopped for an ordinary reason and the zone is ready again.
    pub fn mark_stopped(&mut self, zone: ZoneId) {
        let link = LinkKind::Zone(zone);
        if matches!(self.state(link), LinkState::Running) {
            self.set(link, LinkState::Ready);
        }
    }

    /// **The only path to an all-off.**
    ///
    /// One function, one test surface. The scope comes from the event itself, so
    /// a new fault variant cannot slip through with an unconsidered blast radius.
    pub fn on_event(&mut self, e: &SafetyEvent, at: Monotonic) -> SmallVec<[Effect; 4]> {
        let mut out: SmallVec<[Effect; 4]> = smallvec![];
        // None only for a routine session expiry, which returns before using it.
        let reason = e.latch_reason().unwrap_or(LatchReason::ServiceFailure);

        match e.scope() {
            FaultScope::Zone(zone) => {
                out.push(Effect::AllOff(zone));
                if e.is_routine() {
                    // A session reaching its limit stops water and returns the
                    // zone to ready. It is not a fault and does not latch.
                    self.mark_stopped(zone);
                    return out;
                }
                let link = LinkKind::Zone(zone);
                out.push(Effect::ClosePort(link));
                out.push(Effect::Latch {
                    link,
                    reason: reason.clone(),
                });
                self.set(
                    link,
                    LinkState::Latched {
                        reason: reason.clone(),
                        acknowledged: false,
                    },
                );
                if matches!(reason, LatchReason::Welded) {
                    out.push(Effect::OperatorMessage {
                        link,
                        text: "WELDED: the mixing valve is mechanically stuck and no controller \
                               can close it. Remove valve power and close the hot and cold \
                               service shutoffs.",
                    });
                }
                if matches!(e, SafetyEvent::TemperatureDivergence { .. }) {
                    out.push(Effect::RecordFinding(
                        crate::event::FindingClass::TemperatureDivergence,
                    ));
                }
            }
            FaultScope::Link(LinkKind::Steam) => {
                // Degraded but alive: transmission still works, so the stop goes
                // out and is acknowledged before the link is given up. A lost
                // port cannot do that, and latches directly.
                if matches!(e, SafetyEvent::SteamLinkDegraded { .. }) {
                    out.push(Effect::SteamStopThenLatch);
                } else {
                    out.push(Effect::ClosePort(LinkKind::Steam));
                }
                out.push(Effect::Latch {
                    link: LinkKind::Steam,
                    reason: reason.clone(),
                });
                self.set(
                    LinkKind::Steam,
                    LinkState::Latched {
                        reason: reason.clone(),
                        acknowledged: false,
                    },
                );
            }
            FaultScope::Link(link) => {
                if let Some(zone) = link.zone() {
                    out.push(Effect::AllOff(zone));
                }
                out.push(Effect::ClosePort(link));
                out.push(Effect::Latch {
                    link,
                    reason: reason.clone(),
                });
                self.set(
                    link,
                    LinkState::Latched {
                        reason: reason.clone(),
                        acknowledged: false,
                    },
                );
            }
            FaultScope::Shared => {
                // The service itself is compromised: everything goes down. Water
                // first, on every zone, then the links.
                for zone in ZoneId::ALL {
                    out.push(Effect::AllOff(zone));
                }
                for link in LinkKind::ALL {
                    out.push(Effect::Latch {
                        link,
                        reason: reason.clone(),
                    });
                    self.set(
                        link,
                        LinkState::Latched {
                            reason: reason.clone(),
                            acknowledged: false,
                        },
                    );
                }
            }
        }
        let _ = at;
        out
    }

    /// Acknowledge a latched link. Recovery is never automatic.
    pub fn acknowledge(&mut self, link: LinkKind) -> Result<(), Denial> {
        match self.state(link).clone() {
            LinkState::Latched { reason, .. } => {
                self.set(
                    link,
                    LinkState::Latched {
                        reason,
                        acknowledged: true,
                    },
                );
                Ok(())
            }
            _ => Err(Denial::NotReady {
                zone: link.zone().unwrap_or(ZoneId::Zone1),
                phase: "not latched",
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_units::{CommandId, Cx2, Slot, ValveSetpoint};
    use std::time::Duration;

    fn slots(ns: &[u8]) -> SlotSet {
        ns.iter().filter_map(|n| Slot::new(*n).ok()).collect()
    }

    fn kernel() -> SafetyKernel {
        SafetyKernel::new(
            BootId(1),
            Bounds {
                session_cap: SessionDuration::clamped(Duration::from_secs(1200)),
                configured_outlets: [
                    (ZoneId::Zone1, slots(&[1, 2, 5])),
                    (ZoneId::Zone2, slots(&[1, 2, 3])),
                ],
            },
        )
    }

    fn start(zone: ZoneId, outlets: &[u8]) -> ValidatedStart {
        ValidatedStart {
            zone,
            outlets: slots(outlets),
            temperature: ValveSetpoint::try_new(Cx2::from_raw(76)).unwrap(),
            duration: SessionDuration::clamped(Duration::from_secs(300)),
            command: CommandId(1),
        }
    }

    fn at(s: u64) -> Monotonic {
        Monotonic::from_nanos(s * 1_000_000_000)
    }

    #[test]
    fn a_cold_zone_cannot_start_because_nothing_has_confirmed_it_off() {
        let mut k = kernel();
        let d = k
            .authorize_open(
                &start(ZoneId::Zone1, &[1]),
                StartAuthorization::issue(BootId(1), CommandId(1)),
            )
            .expect_err("cold must refuse");
        assert!(matches!(d, Denial::NotReady { phase: "cold", .. }), "{d:?}");
    }

    #[test]
    fn a_ready_zone_starts_and_the_grant_names_it() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Zone(ZoneId::Zone1));
        let mut req = start(ZoneId::Zone1, &[1, 2]);
        req.command = CommandId(4);
        let g = k
            .authorize_open(&req, StartAuthorization::issue(BootId(1), CommandId(4)))
            .expect("ready must authorise");
        assert_eq!(g.zone(), ZoneId::Zone1);
        assert_eq!(g.command(), CommandId(4));
    }

    #[test]
    fn an_authorisation_cannot_authorise_a_different_command() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Zone(ZoneId::Zone1));
        let mut req = start(ZoneId::Zone1, &[1]);
        req.command = CommandId(9);
        let d = k
            .authorize_open(&req, StartAuthorization::issue(BootId(1), CommandId(4)))
            .expect_err("an authorisation for command 4 must not authorise command 9");
        assert!(matches!(d, Denial::MismatchedCommand { .. }), "{d:?}");
    }

    #[test]
    fn an_authorisation_from_a_previous_boot_is_refused() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Zone(ZoneId::Zone1));
        let d = k
            .authorize_open(
                &start(ZoneId::Zone1, &[1]),
                StartAuthorization::issue(BootId(0), CommandId(1)),
            )
            .expect_err("a stale boot id must refuse");
        assert!(matches!(d, Denial::StaleSession { .. }), "{d:?}");
    }

    #[test]
    fn an_unconfigured_outlet_is_refused_and_names_itself() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Zone(ZoneId::Zone1));
        let d = k
            .authorize_open(
                &start(ZoneId::Zone1, &[1, 4]),
                StartAuthorization::issue(BootId(1), CommandId(1)),
            )
            .expect_err("slot 4 is not configured on zone 1");
        assert!(
            matches!(d, Denial::UnconfiguredOutlet(s) if s.get() == 4),
            "{d:?}"
        );
    }

    #[test]
    fn a_start_with_no_outlets_is_refused() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Zone(ZoneId::Zone1));
        assert!(matches!(
            k.authorize_open(
                &start(ZoneId::Zone1, &[]),
                StartAuthorization::issue(BootId(1), CommandId(1))
            ),
            Err(Denial::NoOutlets)
        ));
    }

    #[test]
    fn a_zone_fault_takes_that_zone_down_and_leaves_the_other_alone() {
        let mut k = kernel();
        for l in LinkKind::ALL {
            k.mark_ready(l);
        }
        let effects = k.on_event(
            &SafetyEvent::ChecksumFailedOnWrite {
                zone: ZoneId::Zone1,
            },
            at(1),
        );
        assert!(effects.contains(&Effect::AllOff(ZoneId::Zone1)));
        assert!(effects.contains(&Effect::ClosePort(LinkKind::Zone(ZoneId::Zone1))));
        assert!(
            !effects.contains(&Effect::AllOff(ZoneId::Zone2)),
            "the other zone is untouched"
        );
        assert!(matches!(
            k.state(LinkKind::Zone(ZoneId::Zone2)),
            LinkState::Ready
        ));
        assert!(matches!(k.state(LinkKind::Steam), LinkState::Ready));
    }

    #[test]
    fn a_shared_fault_takes_everything_down() {
        let mut k = kernel();
        for l in LinkKind::ALL {
            k.mark_ready(l);
        }
        let effects = k.on_event(&SafetyEvent::WatchdogMissed, at(1));
        for zone in ZoneId::ALL {
            assert!(
                effects.contains(&Effect::AllOff(zone)),
                "zone {zone} must be stopped"
            );
        }
        for l in LinkKind::ALL {
            assert!(
                matches!(k.state(l), LinkState::Latched { .. }),
                "{l} must latch"
            );
        }
    }

    #[test]
    fn a_degraded_steam_link_is_told_to_stop_before_it_is_given_up() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Steam);
        let effects = k.on_event(
            &SafetyEvent::SteamLinkDegraded {
                why: crate::DegradeReason::Nak,
            },
            at(1),
        );
        assert!(
            effects.contains(&Effect::SteamStopThenLatch),
            "a link that can still transmit must be told to stop first: {effects:?}"
        );
        assert!(!effects.iter().any(|e| matches!(e, Effect::ClosePort(_))));
    }

    #[test]
    fn a_lost_steam_port_cannot_be_told_anything_and_latches_directly() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Steam);
        let effects = k.on_event(
            &SafetyEvent::PortLost {
                link: LinkKind::Steam,
            },
            at(1),
        );
        assert!(effects.contains(&Effect::ClosePort(LinkKind::Steam)));
        assert!(!effects.contains(&Effect::SteamStopThenLatch));
    }

    #[test]
    fn a_welded_valve_produces_the_message_that_says_no_controller_can_help() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Zone(ZoneId::Zone1));
        let effects = k.on_event(
            &SafetyEvent::ValveFault {
                zone: ZoneId::Zone1,
                raw_code: 35,
                unrecoverable: true,
            },
            at(1),
        );
        let msg = effects.iter().find_map(|e| match e {
            Effect::OperatorMessage { text, .. } => Some(*text),
            _ => None,
        });
        let msg = msg.expect("a welded valve must produce an operator message");
        assert!(msg.contains("service shutoffs"), "{msg}");
    }

    #[test]
    fn a_session_expiry_stops_water_without_latching() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Zone(ZoneId::Zone1));
        k.authorize_open(
            &start(ZoneId::Zone1, &[1]),
            StartAuthorization::issue(BootId(1), CommandId(1)),
        )
        .unwrap();
        let effects = k.on_event(
            &SafetyEvent::SessionExpired {
                zone: ZoneId::Zone1,
            },
            at(1200),
        );
        assert_eq!(effects.as_slice(), &[Effect::AllOff(ZoneId::Zone1)]);
        assert!(
            matches!(k.state(LinkKind::Zone(ZoneId::Zone1)), LinkState::Ready),
            "reaching the limit is not a fault"
        );
    }

    #[test]
    fn a_latched_zone_refuses_to_start_until_acknowledged_and_reopened() {
        let mut k = kernel();
        k.mark_ready(LinkKind::Zone(ZoneId::Zone1));
        k.on_event(
            &SafetyEvent::PortLost {
                link: LinkKind::Zone(ZoneId::Zone1),
            },
            at(1),
        );

        let mut req = start(ZoneId::Zone1, &[1]);
        req.command = CommandId(2);
        let d = k
            .authorize_open(&req, StartAuthorization::issue(BootId(1), CommandId(2)))
            .expect_err("a latched zone must refuse");
        assert!(matches!(d, Denial::ZoneLatched { .. }), "{d:?}");

        // Acknowledging alone does not make it startable: the port is closed and
        // the service must bring the link back through discovery.
        k.acknowledge(LinkKind::Zone(ZoneId::Zone1)).unwrap();
        req.command = CommandId(3);
        assert!(
            k.authorize_open(&req, StartAuthorization::issue(BootId(1), CommandId(3)))
                .is_err(),
            "an acknowledged latch is still a latch until the link is reopened"
        );
    }

    #[test]
    fn every_event_produces_an_all_off_for_the_zone_it_affects() {
        // The property that matters more than any individual case: no fault
        // scoped to a zone can be handled without stopping that zone's water.
        for zone in ZoneId::ALL {
            for e in [
                SafetyEvent::ChecksumFailedOnWrite { zone },
                SafetyEvent::MalformedResponse {
                    zone,
                    detail: "x".into(),
                },
                SafetyEvent::OutOfRangeValue { zone, field: "t" },
                SafetyEvent::SafetyResponseMissed {
                    zone,
                    op: "AllOff".into(),
                },
                SafetyEvent::RtdFaultRegister { zone, bits: 1 },
                SafetyEvent::RtdStarved {
                    zone,
                    since: Duration::from_secs(6),
                },
                SafetyEvent::ValveFault {
                    zone,
                    raw_code: 7,
                    unrecoverable: false,
                },
            ] {
                let mut k = kernel();
                k.mark_ready(LinkKind::Zone(zone));
                let effects = k.on_event(&e, at(1));
                assert!(
                    effects.contains(&Effect::AllOff(zone)),
                    "{e:?} did not stop water on {zone}"
                );
            }
        }
    }
}
