//! What can go wrong, how far it escalates, and what must be done about it.

use kdtv_units::{CorrectedC, LinkKind, RawC, ZoneId};
use serde::Serialize;
use std::time::Duration;

/// How far a fault escalates.
///
/// The design is explicit that escalation is scoped: a link fault takes one zone
/// down, and only a shared fault — the service process, the watchdog, the USB
/// controller, or a failed configuration check — takes both. Getting this wrong
/// in the widening direction means one bad cable stops someone's shower in the
/// other room; getting it wrong in the narrowing direction means a dead watchdog
/// leaves water running.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize)]
pub enum FaultScope {
    /// One zone, and only that zone.
    Zone(ZoneId),
    /// One link, which may be the steam link and so not a zone at all.
    Link(LinkKind),
    /// Everything. The service itself is compromised.
    Shared,
}

/// Why a link was latched unavailable.
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "reason")]
pub enum LatchReason {
    ValveFault {
        raw_code: u8,
    },
    /// A stuck mixing valve. No controller can turn this off — the only remedy
    /// is removing valve power and closing the service shutoffs.
    Welded,
    ChecksumFailedOnWrite,
    MalformedResponse {
        detail: String,
    },
    OutOfRangeValue {
        field: &'static str,
    },
    SafetyResponseMissed {
        op: String,
    },
    PortLost,
    SteamLinkDegraded {
        why: DegradeReason,
    },
    IndependentOverTemperature {
        corrected_c: f32,
    },
    IndependentRawOverTemperature {
        raw_c: f32,
    },
    RtdFault {
        bits: u8,
    },
    RtdStarved {
        since_s: u64,
    },
    /// The independent probe and the valve's own thermistor disagree. Recorded
    /// as a finding as well as latching, because it means one of them is lying
    /// and this project does not yet know which.
    TemperatureDivergence {
        delta_c: f32,
    },
    ConfigCheckFailed {
        detail: String,
    },
    WatchdogMissed,
    UsbControllerLost,
    ServiceFailure,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DegradeReason {
    Timeouts,
    Nak,
    ChecksumFailures,
    GeneratorFault,
}

/// A finding worth recording beyond the immediate escalation.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingClass {
    /// The two temperature sources disagree. Relevant to the open question about
    /// what the valve's reported temperature actually means.
    TemperatureDivergence,
    /// A response decoded but carried a value no documented table explains.
    UndocumentedWireValue,
}

/// Something that happened which the safety kernel must rule on.
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum SafetyEvent {
    // ---- wire conditions -------------------------------------------------
    /// The valve reported a fault. The raw byte travels with it, because the two
    /// documented error tables disagree about what codes mean and this crate
    /// does not get to pick one.
    ValveFault {
        zone: ZoneId,
        raw_code: u8,
        unrecoverable: bool,
    },
    ChecksumFailedOnWrite {
        zone: ZoneId,
    },
    MalformedResponse {
        zone: ZoneId,
        detail: String,
    },
    OutOfRangeValue {
        zone: ZoneId,
        field: &'static str,
    },
    /// A response this service needs in order to believe the valve is off did
    /// not arrive within its deadline and retry budget.
    SafetyResponseMissed {
        zone: ZoneId,
        op: String,
    },

    // ---- link and platform -----------------------------------------------
    /// The port went away — USB enumeration loss, or the cable.
    PortLost {
        link: LinkKind,
    },
    /// The steam link is degraded but still able to transmit, so a stop can and
    /// must be sent before latching.
    SteamLinkDegraded {
        why: DegradeReason,
    },

    // ---- the independent temperature chain -------------------------------
    /// Corrected outlet temperature above the trip, for its dwell, with the
    /// instrumented outlet on.
    IndependentOverTemperature {
        zone: ZoneId,
        corrected: CorrectedC,
        dwell: Duration,
    },
    /// Raw reading above the absolute backstop, regardless of any correction.
    /// This one has no dwell: it is the guard against a correction curve that is
    /// itself wrong.
    IndependentRawOverTemperature {
        zone: ZoneId,
        raw: RawC,
    },
    RtdFaultRegister {
        zone: ZoneId,
        bits: u8,
    },
    RtdStarved {
        zone: ZoneId,
        since: Duration,
    },
    /// Independent and valve-reported temperatures differ beyond the limit, for
    /// the dwell.
    TemperatureDivergence {
        zone: ZoneId,
        delta_c: f32,
        dwell: Duration,
    },

    // ---- the service itself ----------------------------------------------
    SessionExpired {
        zone: ZoneId,
    },
    ConfigCheckFailed {
        detail: String,
    },
    WatchdogMissed,
    UsbControllerLost,
    ServiceFailure,
}

impl SafetyEvent {
    /// How far this event escalates.
    ///
    /// An exhaustive match with no wildcard arm, deliberately: adding a variant
    /// fails to compile until its scope has been decided. A `_ => Zone(..)`
    /// here would make the next fault silently local, and a `_ => Shared` would
    /// make it silently take the house down.
    ///
    /// Exactly four events are [`FaultScope::Shared`], and they are the four
    /// things that mean the service itself can no longer be trusted to stop
    /// anything: a failed configuration check, a missed watchdog, a lost USB
    /// controller, and an internal failure.
    #[must_use]
    pub fn scope(&self) -> FaultScope {
        match self {
            Self::ValveFault { zone, .. }
            | Self::ChecksumFailedOnWrite { zone }
            | Self::MalformedResponse { zone, .. }
            | Self::OutOfRangeValue { zone, .. }
            | Self::SafetyResponseMissed { zone, .. }
            | Self::IndependentOverTemperature { zone, .. }
            | Self::IndependentRawOverTemperature { zone, .. }
            | Self::RtdFaultRegister { zone, .. }
            | Self::RtdStarved { zone, .. }
            | Self::TemperatureDivergence { zone, .. }
            | Self::SessionExpired { zone } => FaultScope::Zone(*zone),

            // A lost port may be the steam link, which is not a zone.
            Self::PortLost { link } => FaultScope::Link(*link),
            Self::SteamLinkDegraded { .. } => FaultScope::Link(LinkKind::Steam),

            Self::ConfigCheckFailed { .. }
            | Self::WatchdogMissed
            | Self::UsbControllerLost
            | Self::ServiceFailure => FaultScope::Shared,
        }
    }

    /// The latch reason this event produces, when it latches at all.
    ///
    /// `None` for a session reaching its own limit, which stops water and
    /// returns the zone to ready. An earlier version mapped that case onto
    /// `ServiceFailure` to keep the signature total; that was dishonest — a
    /// session ending on time is not a service failure, and a log would have
    /// said it was.
    #[must_use]
    pub fn latch_reason(&self) -> Option<LatchReason> {
        Some(match self {
            Self::ValveFault {
                raw_code,
                unrecoverable,
                ..
            } => {
                // 35 is WELDED under the valve-control table: a mechanically
                // stuck valve that no controller can close. It gets its own
                // reason so the operator message can say so.
                if *unrecoverable && *raw_code == 35 {
                    LatchReason::Welded
                } else {
                    LatchReason::ValveFault {
                        raw_code: *raw_code,
                    }
                }
            }
            Self::ChecksumFailedOnWrite { .. } => LatchReason::ChecksumFailedOnWrite,
            Self::MalformedResponse { detail, .. } => LatchReason::MalformedResponse {
                detail: detail.clone(),
            },
            Self::OutOfRangeValue { field, .. } => LatchReason::OutOfRangeValue { field },
            Self::SafetyResponseMissed { op, .. } => {
                LatchReason::SafetyResponseMissed { op: op.clone() }
            }
            Self::PortLost { .. } => LatchReason::PortLost,
            Self::SteamLinkDegraded { why } => LatchReason::SteamLinkDegraded { why: *why },
            Self::IndependentOverTemperature { corrected, .. } => {
                LatchReason::IndependentOverTemperature {
                    corrected_c: corrected.celsius(),
                }
            }
            Self::IndependentRawOverTemperature { raw, .. } => {
                LatchReason::IndependentRawOverTemperature { raw_c: raw.0 }
            }
            Self::RtdFaultRegister { bits, .. } => LatchReason::RtdFault { bits: *bits },
            Self::RtdStarved { since, .. } => LatchReason::RtdStarved {
                since_s: since.as_secs(),
            },
            Self::TemperatureDivergence { delta_c, .. } => {
                LatchReason::TemperatureDivergence { delta_c: *delta_c }
            }
            // A session reaching its limit is not a fault: it stops water and
            // returns the zone to ready, and never latches.
            Self::SessionExpired { .. } => return None,
            Self::ConfigCheckFailed { detail } => LatchReason::ConfigCheckFailed {
                detail: detail.clone(),
            },
            Self::WatchdogMissed => LatchReason::WatchdogMissed,
            Self::UsbControllerLost => LatchReason::UsbControllerLost,
            Self::ServiceFailure => LatchReason::ServiceFailure,
        })
    }

    /// True when the event stops water without marking the link unusable.
    ///
    /// Only a session reaching its own limit. Everything else here means
    /// something is wrong, and a zone that has been latched requires a
    /// deliberate acknowledgement before it can run again.
    #[must_use]
    pub const fn is_routine(&self) -> bool {
        matches!(self, Self::SessionExpired { .. })
    }
}

/// Something the service must do. This crate decides; the service performs.
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "effect")]
pub enum Effect {
    /// Command every outlet closed on this zone.
    AllOff(ZoneId),
    /// Command steam off, require the acknowledgement, and only then latch.
    ///
    /// Ordering matters and is why this is one effect rather than two: on a
    /// degraded-but-alive link, transmitting still works, so the stop must go
    /// out before the link is given up.
    SteamStopThenLatch,
    /// Close the port. The link is owned by value, so this is not advisory.
    ClosePort(LinkKind),
    /// Mark the link unavailable until an operator acknowledges it.
    Latch { link: LinkKind, reason: LatchReason },
    /// Tell the operator something they must act on physically.
    OperatorMessage { link: LinkKind, text: &'static str },
    /// Record a finding for the investigation log.
    RecordFinding(FindingClass),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone_events() -> Vec<SafetyEvent> {
        vec![
            SafetyEvent::ValveFault {
                zone: ZoneId::Zone1,
                raw_code: 7,
                unrecoverable: false,
            },
            SafetyEvent::ChecksumFailedOnWrite {
                zone: ZoneId::Zone1,
            },
            SafetyEvent::MalformedResponse {
                zone: ZoneId::Zone1,
                detail: "bad len".into(),
            },
            SafetyEvent::OutOfRangeValue {
                zone: ZoneId::Zone1,
                field: "temperature",
            },
            SafetyEvent::SafetyResponseMissed {
                zone: ZoneId::Zone1,
                op: "AllOff".into(),
            },
            SafetyEvent::IndependentOverTemperature {
                zone: ZoneId::Zone1,
                corrected: kdtv_units::OffsetCurve::uncorrected().correct(RawC(46.0)),
                dwell: Duration::from_secs(3),
            },
            SafetyEvent::IndependentRawOverTemperature {
                zone: ZoneId::Zone1,
                raw: RawC(51.0),
            },
            SafetyEvent::RtdFaultRegister {
                zone: ZoneId::Zone1,
                bits: 0x04,
            },
            SafetyEvent::RtdStarved {
                zone: ZoneId::Zone1,
                since: Duration::from_secs(6),
            },
            SafetyEvent::TemperatureDivergence {
                zone: ZoneId::Zone1,
                delta_c: 6.0,
                dwell: Duration::from_secs(11),
            },
            SafetyEvent::SessionExpired {
                zone: ZoneId::Zone1,
            },
        ]
    }

    fn shared_events() -> Vec<SafetyEvent> {
        vec![
            SafetyEvent::ConfigCheckFailed {
                detail: "two zones on one port".into(),
            },
            SafetyEvent::WatchdogMissed,
            SafetyEvent::UsbControllerLost,
            SafetyEvent::ServiceFailure,
        ]
    }

    #[test]
    fn every_zone_event_scopes_to_its_own_zone_and_no_further() {
        for e in zone_events() {
            assert_eq!(e.scope(), FaultScope::Zone(ZoneId::Zone1), "{e:?}");
        }
    }

    #[test]
    fn exactly_four_events_take_everything_down() {
        let shared = shared_events();
        assert_eq!(shared.len(), 4);
        for e in shared {
            assert_eq!(e.scope(), FaultScope::Shared, "{e:?}");
        }
    }

    #[test]
    fn a_lost_port_scopes_to_the_link_because_it_may_be_steam() {
        assert_eq!(
            SafetyEvent::PortLost {
                link: LinkKind::Steam
            }
            .scope(),
            FaultScope::Link(LinkKind::Steam)
        );
        assert_eq!(
            SafetyEvent::PortLost {
                link: LinkKind::Zone(ZoneId::Zone2)
            }
            .scope(),
            FaultScope::Link(LinkKind::Zone(ZoneId::Zone2))
        );
        // A degraded steam link is a steam link by construction, not by argument.
        assert_eq!(
            SafetyEvent::SteamLinkDegraded {
                why: DegradeReason::Nak
            }
            .scope(),
            FaultScope::Link(LinkKind::Steam)
        );
    }

    #[test]
    fn a_welded_valve_gets_its_own_latch_reason() {
        let e = SafetyEvent::ValveFault {
            zone: ZoneId::Zone1,
            raw_code: 35,
            unrecoverable: true,
        };
        assert_eq!(e.latch_reason(), Some(LatchReason::Welded));
        // The same code that is not flagged unrecoverable is not assumed welded:
        // the two error tables disagree about what 35 means, so the decision is
        // made where the table is known, not here.
        let ambiguous = SafetyEvent::ValveFault {
            zone: ZoneId::Zone1,
            raw_code: 35,
            unrecoverable: false,
        };
        assert_eq!(
            ambiguous.latch_reason(),
            Some(LatchReason::ValveFault { raw_code: 35 })
        );
    }

    #[test]
    fn a_session_expiry_has_no_latch_reason_because_it_does_not_latch() {
        assert_eq!(
            SafetyEvent::SessionExpired {
                zone: ZoneId::Zone1
            }
            .latch_reason(),
            None
        );
        for e in zone_events().into_iter().chain(shared_events()) {
            if matches!(e, SafetyEvent::SessionExpired { .. }) {
                continue;
            }
            assert!(e.latch_reason().is_some(), "{e:?} must name why it latched");
        }
    }

    #[test]
    fn only_a_session_expiry_is_routine() {
        assert!(
            SafetyEvent::SessionExpired {
                zone: ZoneId::Zone1
            }
            .is_routine()
        );
        for e in zone_events().into_iter().chain(shared_events()) {
            if matches!(e, SafetyEvent::SessionExpired { .. }) {
                continue;
            }
            assert!(!e.is_routine(), "{e:?} must not be routine");
        }
    }
}
