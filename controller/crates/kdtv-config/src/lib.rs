//! Typed configuration: parse, validate, and refuse to start when wrong.
//!
//! The contract this crate implements is `controller/deploy/kdtvd.toml`, comments
//! included — the comments there carry the reasoning and this crate carries the
//! enforcement. `deploy/kdtvd.emulated.toml` is the same contract for the bench
//! rig. Both are loaded from disk by this crate's tests, so the committed files
//! and the parser cannot drift apart.
//!
//! # This crate's job is to refuse
//!
//! There is no degraded start. A service that cannot be sure which physical
//! valve is on which bus must not drive either of them, so every problem
//! produces a [`ConfigError`] naming the dotted key and saying what is wrong
//! with it. Nothing here defaults a value it could not parse, and nothing here
//! warns and carries on. `BOOT-02`, `SER-02`, `USB-03`.
//!
//! The refusals, and where each lives:
//!
//! | Refusal | Module |
//! | --- | --- |
//! | A `/dev/tty*` port path — enumeration order is how zone 1 becomes zone 2 after a reboot | [`port`] |
//! | Two links resolving to one device, through symlinks | this module |
//! | A duplicate slot, duplicate status index, or a wire outlet the valve family does not have | [`zone`] |
//! | `instrumented_slot` naming a slot that is not configured | [`zone`] |
//! | A pseudo-terminal or a time scale under `profile = "production"` | [`port`], [`timing`] |
//! | Steam enabled with no port | this module |
//! | A token file that is missing or world-readable | [`api`] |
//! | An attestation that does not match the transmit gate's scope | [`gate`] |
//! | A configured bound that would widen a compiled-in one | [`bounds`] |
//!
//! # What it does not do
//!
//! It does not decide whether the transmit gate may open. This crate validates
//! the *shape* of the operator's claim — scope, capture reference, fixture-set
//! hash, per-link polarity attestation. Whether the fixtures behind that hash
//! are genuinely tier `[A]` is `kdtv_proto`'s question, and no value this crate
//! produces is a `TransmitAuthority`.
//!
//! It also decides none of the protocol contradictions. The master address, the
//! Saturn retry count, the DTV+ tick and the DTV+ retry count are all
//! configuration precisely because the sources disagree; both readings stay
//! reachable and neither is declared correct. `CORRECTIONS.md` item 5.
//!
//! # Types, not conventions
//!
//! Everything that has a type in `kdtv_units` or `kdtv_proto` gets it here:
//! [`kdtv_units::Slot`], [`kdtv_proto::saturn::MasterAddr`],
//! [`kdtv_proto::saturn::OutletTable`], [`kdtv_proto::saturn::ValveType`]. A
//! configuration value that cannot be a valid protocol value therefore fails at
//! parse, in the crate whose tests already cover it, rather than at the encoder.
//!
//! Two walls are structural rather than checked:
//!
//! - **A port path cannot be an unstable name.** [`port::PortPath`] has no
//!   variant that holds one.
//! - **A wire deadline cannot be scaled.** [`timing::SessionScale::apply`] takes
//!   a [`timing::SessionSpan`], whose constructors enumerate the session-class
//!   durations this service has. There is no route from a 525 ms tick into one.

// Tests legitimately panic on a broken invariant; the production lints stay on
// for library code, where a panic is a fault, not a diagnosis.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp
    )
)]

pub mod api;
pub mod bounds;
pub mod error;
pub mod fs;
pub mod gate;
pub mod logging;
pub mod port;
pub mod profile;
mod raw;
pub mod sensors;
pub mod steam;
pub mod timing;
pub mod zone;

pub use api::ApiConfig;
pub use bounds::{Bounds, BoundsRequest};
pub use error::ConfigError;
pub use fs::{FsEntry, FsView, MapFs};
pub use gate::{GateScope, TransmitGateConfig};
pub use logging::LoggingConfig;
pub use port::PortPath;
pub use profile::Profile;
pub use sensors::SensorConfig;
pub use steam::SteamConfig;
pub use timing::{SessionScale, SessionSpan, TimingConfig};
pub use zone::{ConfiguredValve, OutletConfig, ZoneConfig};

#[cfg(unix)]
pub use fs::RealFs;

use kdtv_proto::saturn::Timings;
use kdtv_units::{LinkKind, ZoneId};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A configuration that has passed every check in this crate.
///
/// The fields are private and [`ValidatedConfig::load`] is the only
/// constructor, so downstream crates that accept this type are accepting
/// something validated — an unvalidated configuration is not a value that
/// exists.
#[derive(Clone, Debug)]
pub struct ValidatedConfig {
    profile: Profile,
    gate: TransmitGateConfig,
    zone1: ZoneConfig,
    zone2: ZoneConfig,
    steam: Option<SteamConfig>,
    sensor1: SensorConfig,
    sensor2: SensorConfig,
    api: ApiConfig,
    logging: LoggingConfig,
    bounds: Bounds,
    timing: TimingConfig,
}

impl ValidatedConfig {
    /// Reads and validates a configuration file.
    ///
    /// `fs` answers the three questions validation cannot answer from the file:
    /// whether a port resolves to a device, what two ports resolve *to*, and the
    /// permission bits on the token file. Pass [`RealFs`] on a real machine and
    /// [`MapFs`] in a test.
    pub fn load(path: &Path, fs: &dyn FsView) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        Self::from_str_with(&text, path, fs)
    }

    /// The same, from text already in hand. `path` is used only in messages.
    pub fn from_str_with(text: &str, path: &Path, fs: &dyn FsView) -> Result<Self, ConfigError> {
        let raw: raw::RawConfig = toml::from_str(text).map_err(|source| ConfigError::Syntax {
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
        Self::validate(raw, fs)
    }

    fn validate(raw: raw::RawConfig, fs: &dyn FsView) -> Result<Self, ConfigError> {
        let profile = raw.profile;

        // --- time scaling -------------------------------------------------
        let scale = match raw.bench {
            None => SessionScale::UNSCALED,
            Some(_) if profile.is_production() => {
                return Err(ConfigError::BenchTableUnderProduction);
            }
            Some(b) => SessionScale::try_new(b.session_scale)?,
        };

        // --- zones --------------------------------------------------------
        let zone1 = build_zone(ZoneId::Zone1, raw.zones.zone1, profile)?;
        let zone2 = build_zone(ZoneId::Zone2, raw.zones.zone2, profile)?;

        // --- steam --------------------------------------------------------
        let (steam, steam_port) = match raw.steam {
            None => (None, None),
            Some(s) => {
                let port = match s.port.as_deref() {
                    Some(p) => Some(PortPath::parse("steam.port", p, profile)?),
                    None => None,
                };
                match (s.enabled, port.clone()) {
                    (true, None) => return Err(ConfigError::SteamEnabledWithoutPort),
                    (true, Some(p)) => (Some(SteamConfig::build(p, s.tick_ms, s.retries)?), port),
                    // A disabled steam block still has its port checked for
                    // shape and for collision: a port that shadows a valve bus
                    // is a wiring mistake whether or not it is opened today.
                    (false, _) => (None, port),
                }
            }
        };

        // --- one device per link ------------------------------------------
        let mut ports: Vec<(&str, &PortPath)> = vec![
            ("zones.zone1.port", zone1.port()),
            ("zones.zone2.port", zone2.port()),
        ];
        if let Some(p) = steam_port.as_ref() {
            ports.push(("steam.port", p));
        }
        check_ports_distinct(&ports, profile, fs)?;

        // --- transmit gate ------------------------------------------------
        let mut links = vec![zone1.link(), zone2.link()];
        if steam.is_some() {
            links.push(LinkKind::Steam);
        }
        let mut polarity = BTreeMap::new();
        for (name, text) in raw.transmit_gate.polarity.unwrap_or_default() {
            let link = gate::polarity_link(&name)
                .ok_or(ConfigError::GatePolarityUnknownLink { link: name })?;
            polarity.insert(link, text);
        }
        let gate = TransmitGateConfig::validate(
            raw.transmit_gate.scope,
            raw.transmit_gate.capture_ref,
            raw.transmit_gate.fixtures_sha256,
            polarity,
            &links,
        )?;

        // --- sensors ------------------------------------------------------
        let sensor1 = build_sensor(ZoneId::Zone1, raw.sensors.zone1)?;
        let sensor2 = build_sensor(ZoneId::Zone2, raw.sensors.zone2)?;
        if sensor1.chip_select() == sensor2.chip_select() {
            return Err(ConfigError::DuplicateChipSelect {
                zone1: ZoneId::Zone1,
                zone2: ZoneId::Zone2,
                value: sensor1.chip_select().to_owned(),
            });
        }

        // --- api and logging ----------------------------------------------
        let api = ApiConfig::build(
            &raw.api.bind,
            &raw.api.token_file,
            raw.api.session_ttl_s,
            fs,
        )?;
        let logging = LoggingConfig::build(
            &raw.logging.directory,
            raw.logging.frames,
            raw.logging.max_total_mb,
        )?;

        // --- bounds -------------------------------------------------------
        let request = build_bounds_request(&raw.bounds.unwrap_or_default())?;
        Bounds::check_narrowing(&request)?;
        let bounds = Bounds::resolve(&request);
        bounds.check_ordering()?;

        let dtv = steam.as_ref().map_or(
            kdtv_proto::dtv::DtvTimings::DOCUMENTED,
            SteamConfig::timings,
        );
        let timing = TimingConfig::new(profile, Timings::DOCUMENTED, dtv, scale)?;

        Ok(Self {
            profile,
            gate,
            zone1,
            zone2,
            steam,
            sensor1,
            sensor2,
            api,
            logging,
            bounds,
            timing,
        })
    }

    #[must_use]
    pub const fn profile(&self) -> Profile {
        self.profile
    }

    #[must_use]
    pub const fn gate(&self) -> &TransmitGateConfig {
        &self.gate
    }

    #[must_use]
    pub const fn zone(&self, id: ZoneId) -> &ZoneConfig {
        match id {
            ZoneId::Zone1 => &self.zone1,
            ZoneId::Zone2 => &self.zone2,
        }
    }

    /// Both zones, in [`ZoneId`] order.
    #[must_use]
    pub const fn zones(&self) -> [&ZoneConfig; 2] {
        [&self.zone1, &self.zone2]
    }

    /// The steam link, or `None` when `steam.enabled = false`.
    #[must_use]
    pub const fn steam(&self) -> Option<&SteamConfig> {
        self.steam.as_ref()
    }

    #[must_use]
    pub const fn sensor(&self, id: ZoneId) -> &SensorConfig {
        match id {
            ZoneId::Zone1 => &self.sensor1,
            ZoneId::Zone2 => &self.sensor2,
        }
    }

    #[must_use]
    pub const fn api(&self) -> &ApiConfig {
        &self.api
    }

    #[must_use]
    pub const fn logging(&self) -> &LoggingConfig {
        &self.logging
    }

    /// The resolved safety bounds: the tighter of each compiled-in constant and
    /// any configured value. Nothing in a configuration file can widen one.
    #[must_use]
    pub const fn bounds(&self) -> Bounds {
        self.bounds
    }

    #[must_use]
    pub const fn timing(&self) -> &TimingConfig {
        &self.timing
    }

    /// The session limit, with the bench scale applied.
    ///
    /// The scale is at most 1.0 and the bound is at most
    /// [`kdtv_units::SessionDuration::HARD_LIMIT`], so this is never longer
    /// than 20 minutes whatever the file says.
    #[must_use]
    pub fn scaled_max_session(&self) -> Duration {
        self.timing
            .scaled(SessionSpan::of_session(self.bounds.max_session()))
            .get()
    }

    /// The independent-temperature dwells, with the bench scale applied. Their
    /// unscaled values are `kdtv_units` constants; nothing configures them.
    #[must_use]
    pub fn scaled_dwells(&self) -> Dwells {
        Dwells {
            corrected_trip: self
                .timing
                .scaled(SessionSpan::corrected_trip_dwell())
                .get(),
            divergence: self.timing.scaled(SessionSpan::divergence_dwell()).get(),
            rtd_starvation: self.timing.scaled(SessionSpan::rtd_starvation()).get(),
        }
    }

    /// Every link this service will drive: both zones, and steam when enabled.
    #[must_use]
    pub fn links(&self) -> Vec<LinkKind> {
        let mut out = vec![self.zone1.link(), self.zone2.link()];
        if self.steam.is_some() {
            out.push(LinkKind::Steam);
        }
        out
    }

    /// Substitutes real pseudo-terminal paths for the placeholders in
    /// `deploy/kdtvd.emulated.toml`.
    ///
    /// A PTY path does not exist until the rig creates the pair, so the
    /// committed bench file cannot name one; it names
    /// [`port::PTY_PLACEHOLDER`] and the rig calls this once the pairs are up.
    /// The distinct-device check runs again on the substituted paths, so the rig
    /// cannot hand two links one terminal.
    ///
    /// Bench only, and every placeholder must be supplied: a link left on a
    /// placeholder has nothing to open.
    pub fn bind_ptys(
        &self,
        ptys: &BTreeMap<LinkKind, PathBuf>,
        fs: &dyn FsView,
    ) -> Result<Self, ConfigError> {
        let mut next = self.clone();
        for link in self.links() {
            let configured = self.port_of(link);
            let supplied = ptys.get(&link);
            match (configured.is_placeholder(), supplied) {
                (true, None) => {
                    return Err(ConfigError::PlaceholderUnbound {
                        link: link.to_string(),
                    });
                }
                (false, Some(_)) => {
                    return Err(ConfigError::NotAPlaceholder {
                        link: link.to_string(),
                    });
                }
                (false, None) => {}
                (true, Some(path)) => {
                    let field = field_for(link);
                    let parsed = PortPath::parse(field, &path.to_string_lossy(), self.profile)?;
                    match link {
                        LinkKind::Zone(ZoneId::Zone1) => {
                            next.zone1 = next.zone1.with_port(parsed);
                        }
                        LinkKind::Zone(ZoneId::Zone2) => {
                            next.zone2 = next.zone2.with_port(parsed);
                        }
                        LinkKind::Steam => {
                            next.steam = next.steam.as_ref().map(|s| s.with_port(parsed));
                        }
                    }
                }
            }
        }

        let mut ports: Vec<(&str, &PortPath)> = vec![
            ("zones.zone1.port", next.zone1.port()),
            ("zones.zone2.port", next.zone2.port()),
        ];
        if let Some(s) = next.steam.as_ref() {
            ports.push(("steam.port", s.port()));
        }
        check_ports_distinct(&ports, next.profile, fs)?;
        Ok(next)
    }

    fn port_of(&self, link: LinkKind) -> &PortPath {
        match link {
            LinkKind::Zone(ZoneId::Zone1) => self.zone1.port(),
            LinkKind::Zone(ZoneId::Zone2) => self.zone2.port(),
            LinkKind::Steam => self
                .steam
                .as_ref()
                .map_or(&PLACEHOLDER_PORT, SteamConfig::port),
        }
    }
}

/// The dwells [`ValidatedConfig::scaled_dwells`] returns.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Dwells {
    /// Corrected reading above [`kdtv_units::CORRECTED_TRIP_C`] for this long
    /// stops the zone.
    pub corrected_trip: Duration,
    /// Corrected and reported temperatures apart by more than
    /// [`kdtv_units::DIVERGENCE_LIMIT_C`] for this long stops the zone.
    pub divergence: Duration,
    /// No RTD sample for this long stops the zone.
    pub rtd_starvation: Duration,
}

/// Only reachable when steam is absent, in which case
/// [`ValidatedConfig::links`] never names it.
const PLACEHOLDER_PORT: PortPath = PortPath::PtyPlaceholder;

const fn field_for(link: LinkKind) -> &'static str {
    match link {
        LinkKind::Zone(ZoneId::Zone1) => "zones.zone1.port",
        LinkKind::Zone(ZoneId::Zone2) => "zones.zone2.port",
        LinkKind::Steam => "steam.port",
    }
}

fn build_zone(id: ZoneId, raw: raw::RawZone, profile: Profile) -> Result<ZoneConfig, ConfigError> {
    let field = match id {
        ZoneId::Zone1 => "zones.zone1.port",
        ZoneId::Zone2 => "zones.zone2.port",
    };
    let port = PortPath::parse(field, &raw.port, profile)?;
    let mut outlets = Vec::with_capacity(raw.outlets.len());
    for o in raw.outlets {
        let slot = kdtv_units::Slot::new(o.slot).map_err(|_| ConfigError::SlotOutOfRange {
            zone: id,
            value: o.slot,
        })?;
        outlets.push(OutletConfig {
            slot,
            status_index: o.status_index,
            wire_outlet: o.wire_outlet,
            name: o.name,
        });
    }
    ZoneConfig::build(
        id,
        port,
        raw.valve,
        raw.master_address,
        outlets,
        raw.instrumented_slot,
    )
}

fn build_sensor(id: ZoneId, raw: raw::RawSensor) -> Result<SensorConfig, ConfigError> {
    let points = raw.correction.map(|ps| {
        ps.into_iter()
            .map(|p| (p.surface_c, p.immersion_c))
            .collect()
    });
    SensorConfig::build(id, raw.chip_select, points)
}

fn build_bounds_request(raw: &raw::RawBounds) -> Result<BoundsRequest, ConfigError> {
    Ok(BoundsRequest {
        setpoint_ceiling: raw
            .setpoint_ceiling_c
            .map(|v| bounds::cx2_from_celsius("bounds.setpoint_ceiling_c", v))
            .transpose()?,
        setpoint_floor: raw
            .setpoint_floor_c
            .map(|v| bounds::cx2_from_celsius("bounds.setpoint_floor_c", v))
            .transpose()?,
        max_session: raw.max_session_s.map(Duration::from_secs),
        steam_ceiling: raw
            .steam_ceiling_f
            .map(|v| bounds::fx2_from_fahrenheit("bounds.steam_ceiling_f", v))
            .transpose()?,
        steam_floor: raw
            .steam_floor_f
            .map(|v| bounds::fx2_from_fahrenheit("bounds.steam_floor_f", v))
            .transpose()?,
        steam_max_minutes: raw.steam_max_minutes,
    })
}

/// `SER-02` / `USB-03`. Every link must resolve to a present device, and to a
/// *different* one.
///
/// Two `by-id` names can be symlinks to one `ttyUSB`, which the configured
/// strings alone do not reveal — so the comparison is on the canonical target
/// as well as on the text.
fn check_ports_distinct(
    ports: &[(&str, &PortPath)],
    profile: Profile,
    fs: &dyn FsView,
) -> Result<(), ConfigError> {
    let mut resolved: Vec<(&str, &PortPath, Option<PathBuf>)> = Vec::with_capacity(ports.len());
    for (field, port) in ports {
        let target = port.as_path().and_then(|p| fs.canonicalize(p));
        if profile.is_production() && target.is_none() {
            return Err(ConfigError::PortAbsent {
                field: (*field).to_owned(),
                path: port.to_string(),
            });
        }
        resolved.push((field, port, target));
    }

    for (i, (field_a, port_a, target_a)) in resolved.iter().enumerate() {
        for (field_b, port_b, target_b) in resolved.iter().skip(i + 1) {
            // The placeholder names no device, so two placeholders are not two
            // links on one port — they are two links the rig has yet to bind.
            if port_a.is_placeholder() || port_b.is_placeholder() {
                continue;
            }
            if port_a == port_b {
                return Err(ConfigError::DuplicatePort {
                    field: (*field_a).to_owned(),
                    other: (*field_b).to_owned(),
                    resolved: port_a.to_string(),
                });
            }
            if let (Some(a), Some(b)) = (target_a, target_b)
                && a == b
            {
                return Err(ConfigError::DuplicatePort {
                    field: (*field_a).to_owned(),
                    other: (*field_b).to_owned(),
                    resolved: a.display().to_string(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
