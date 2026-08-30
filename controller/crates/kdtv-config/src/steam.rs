//! The DTV+ link to the K-1737-K1 steam adapter.
//!
//! The generator behind the adapter is a self-contained appliance installed by a
//! professional and is out of scope: this service sends the adapter setpoints,
//! the same relationship it has with the valves.
//!
//! No DTV+ bus has ever been captured in this project, so both timing figures
//! here are unresolved between sources and both are carried. `CORRECTIONS.md`
//! item 5.

use crate::error::ConfigError;
use crate::port::PortPath;
use kdtv_proto::dtv::DtvTimings;
use kdtv_units::LinkKind;
use std::time::Duration;

/// The steam link, when it is enabled.
///
/// A disabled steam block produces `None` from
/// [`crate::ValidatedConfig::steam`], so downstream code has no steam
/// configuration to reach for rather than a struct with a `false` in it.
#[derive(Clone, PartialEq, Debug)]
pub struct SteamConfig {
    port: PortPath,
    timings: DtvTimings,
}

impl SteamConfig {
    pub(crate) fn build(
        port: PortPath,
        tick_ms: Option<u64>,
        retries: Option<u8>,
    ) -> Result<Self, ConfigError> {
        let mut timings = DtvTimings::DOCUMENTED;
        if let Some(ms) = tick_ms {
            // The floor is 150 ms, the fastest figure any source states.
            // Polling a fragile controller faster than it can answer is what
            // caused INVESTIGATIONS.md I1.
            timings = timings
                .with_tick(Duration::from_millis(ms))
                .map_err(|source| ConfigError::SteamTiming {
                    field: "tick_ms",
                    source,
                })?;
        }
        if let Some(n) = retries {
            timings = timings
                .with_retries(n)
                .map_err(|source| ConfigError::SteamTiming {
                    field: "retries",
                    source,
                })?;
        }
        Ok(Self { port, timings })
    }

    #[must_use]
    pub const fn port(&self) -> &PortPath {
        &self.port
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        LinkKind::Steam
    }

    /// The DTV+ wire timings for this link. Wire class: never scaled.
    #[must_use]
    pub const fn timings(&self) -> DtvTimings {
        self.timings
    }

    pub(crate) fn with_port(&self, port: PortPath) -> Self {
        Self {
            port,
            ..self.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::Profile;
    use kdtv_proto::dtv::TimingError;

    fn port() -> PortPath {
        PortPath::parse(
            "steam.port",
            "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if02-port0",
            Profile::Production,
        )
        .unwrap()
    }

    #[test]
    fn the_contract_values_build() {
        let s = SteamConfig::build(port(), Some(150), Some(4)).unwrap();
        assert_eq!(s.timings().tick, Duration::from_millis(150));
        assert_eq!(s.timings().retries, 4);
        assert_eq!(s.link(), LinkKind::Steam);
        // Both readings stay reachable: this crate resolves neither.
        assert_eq!(s.timings().tick_candidate_slow, Duration::from_millis(500));
        assert_eq!(s.timings().retries_alternate, 5);
    }

    #[test]
    fn omitting_the_timings_leaves_the_documented_position() {
        let s = SteamConfig::build(port(), None, None).unwrap();
        assert_eq!(s.timings(), DtvTimings::DOCUMENTED);
        assert_eq!(s.timings().tick, Duration::from_millis(500));
    }

    #[test]
    fn a_tick_below_the_floor_is_refused() {
        let err = SteamConfig::build(port(), Some(20), None).unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::SteamTiming {
                    field: "tick_ms",
                    source: TimingError::TickBelowFloor { .. }
                }
            ),
            "{err}"
        );
        assert!(err.to_string().contains("steam.tick_ms"), "{err}");
    }

    #[test]
    fn more_retries_than_any_source_states_is_refused() {
        let err = SteamConfig::build(port(), None, Some(9)).unwrap_err();
        assert!(
            matches!(
                err,
                ConfigError::SteamTiming {
                    field: "retries",
                    source: TimingError::RetriesAboveMaximum { .. }
                }
            ),
            "{err}"
        );
        // Both documented readings are accepted; neither is declared correct.
        assert!(SteamConfig::build(port(), None, Some(4)).is_ok());
        assert!(SteamConfig::build(port(), None, Some(5)).is_ok());
    }
}
