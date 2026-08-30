//! Every way a configuration is refused.
//!
//! This enum is the crate's output as much as [`crate::ValidatedConfig`] is.
//! Each variant names the dotted key it came from and says what is wrong with
//! it, because the reader is a person on a Raspberry Pi with a shower that will
//! not start and no debugger.
//!
//! There is no `Warning` and no `Defaulted`. `BOOT-02` / `USB-03`: a
//! configuration that cannot be trusted is a refusal to start, not a degraded
//! start.

use kdtv_proto::dtv::TimingError;
use kdtv_proto::saturn::OutletError;
use kdtv_units::{CurveError, Slot, ZoneId};
use std::path::PathBuf;

/// Why the configuration was refused.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read the configuration file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("{path} is not valid TOML for this service: {source}")]
    Syntax {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },

    #[error(
        "{field} = \"{path}\" binds a zone by kernel enumeration order. Use a \
         /dev/serial/by-id or /dev/serial/by-path name: after a reboot the \
         converters can enumerate the other way round and zone 1 silently \
         becomes zone 2"
    )]
    UnstablePortPath { field: String, path: String },

    #[error(
        "{field} = \"{path}\" is not a port path this service accepts. Expected \
         /dev/serial/by-id/<name>, /dev/serial/by-path/<name>, or /dev/pts/<n> \
         under the bench profile"
    )]
    UnknownPortScheme { field: String, path: String },

    #[error("{field} = \"{path}\" is a pseudo-terminal, which the production profile refuses")]
    PtyUnderProduction { field: String, path: String },

    #[error("{field} = \"{path}\" does not resolve to a device")]
    PortAbsent { field: String, path: String },

    #[error(
        "{field} and {other} both resolve to {resolved}. Two links cannot share \
         one converter, and a service that cannot tell which physical valve is \
         on which bus must not drive either of them"
    )]
    DuplicatePort {
        field: String,
        other: String,
        resolved: String,
    },

    #[error(
        "zones.{zone}.master_address = 0x{value:02X} is not a master identity. \
         Use 0x00 (DTV) or 0x10 (Prompt); the two sources disagree over which a \
         Prompt 3 answers, which is why this is configuration and not a constant"
    )]
    MasterAddress { zone: ZoneId, value: u8 },

    #[error("zones.{zone}.outlets is empty; a zone with no outlet cannot be commanded")]
    NoOutlets { zone: ZoneId },

    #[error("zones.{zone}.outlets has slot = {value}, which is outside 1..=6")]
    SlotOutOfRange { zone: ZoneId, value: u8 },

    #[error("zones.{zone}.outlets: {source}")]
    Outlets {
        zone: ZoneId,
        source: Box<OutletError>,
    },

    #[error(
        "zones.{zone}.instrumented_slot = {slot} names a slot that is not in \
         zones.{zone}.outlets (configured: {configured}). The instrumented \
         outlet is the one with independent temperature coverage; naming an \
         absent slot would leave the zone with none"
    )]
    InstrumentedSlotNotConfigured {
        zone: ZoneId,
        slot: u8,
        configured: String,
    },

    #[error("steam.enabled = true but steam.port is not set")]
    SteamEnabledWithoutPort,

    #[error("steam.{field}: {source}")]
    SteamTiming {
        field: &'static str,
        source: TimingError,
    },

    #[error("api.token_file = \"{path}\" does not exist")]
    TokenFileMissing { path: String },

    #[error(
        "api.token_file = \"{path}\" is mode {mode:04o} and therefore \
         world-readable. Anything that can read the token can run the shower"
    )]
    TokenFileWorldReadable { path: String, mode: u32 },

    #[error("api.bind = \"{bind}\" is not a socket address: {reason}")]
    ApiBindUnparseable { bind: String, reason: String },

    #[error(
        "api.bind = \"{bind}\" is not a loopback address. The controller has no \
         authentication of its own and this API can run the shower; it binds to \
         loopback only"
    )]
    ApiBindNotLoopback { bind: String },

    #[error("api.session_ttl_s = {value} is outside 1..={max}")]
    ApiSessionTtl { value: u64, max: u64 },

    #[error("logging.directory is empty")]
    LoggingDirectoryEmpty,

    #[error("logging.max_total_mb = 0 leaves no room for the log the service is required to write")]
    LoggingBudgetZero,

    #[error("sensors.{zone}.chip_select is empty")]
    ChipSelectEmpty { zone: ZoneId },

    #[error("sensors.{zone1}.chip_select and sensors.{zone2}.chip_select are both \"{value}\"")]
    DuplicateChipSelect {
        zone1: ZoneId,
        zone2: ZoneId,
        value: String,
    },

    #[error("sensors.{zone}.correction: {source}")]
    Curve { zone: ZoneId, source: CurveError },

    #[error(
        "transmit_gate.{field} is required when transmit_gate.scope = \
         \"real-bus-attested\". Opening the gate is a dated, reviewable act and \
         the evidence is part of it"
    )]
    GateAttestationMissing { field: &'static str },

    #[error(
        "transmit_gate.{field} is set while transmit_gate.scope = \
         \"emulator-only\". Attestation without a scope that uses it reads as a \
         gate that is open when it is not"
    )]
    GateAttestationUnused { field: &'static str },

    #[error(
        "transmit_gate.fixtures_sha256 = \"{value}\" is not 64 lowercase hex \
         characters"
    )]
    GateFixtureHash { value: String },

    #[error("transmit_gate.capture_ref is empty")]
    GateCaptureRefEmpty,

    #[error(
        "transmit_gate.polarity.{link} is missing. Every configured link needs \
         its A/B polarity recorded before this service may drive it; a reversed \
         pair is silent"
    )]
    GatePolarityMissing { link: String },

    #[error("transmit_gate.polarity.{link} is empty")]
    GatePolarityEmpty { link: String },

    #[error(
        "transmit_gate.polarity.{link} attests a link this configuration does \
         not have"
    )]
    GatePolarityUnknownLink { link: String },

    #[error(
        "the [bench] table is present under profile = \"production\". Scaled \
         session timers are a bench facility; production forces a scale of 1.0"
    )]
    BenchTableUnderProduction,

    #[error(
        "bench.session_scale = {value} is outside 0 < scale <= 1.0. A scale \
         above 1.0 would lengthen a session, and configuration may only narrow"
    )]
    SessionScaleOutOfRange { value: f64 },

    #[error(
        "{field} = {requested} would widen the compiled-in bound of {compiled}. \
         Configuration may only narrow a safety bound"
    )]
    BoundWiden {
        field: &'static str,
        requested: String,
        compiled: String,
    },

    #[error("{field} = {value} is not on the {step} step this encoding uses")]
    TemperatureStep {
        field: &'static str,
        value: f64,
        step: &'static str,
    },

    #[error("{field} = {value} is outside the range this encoding can represent")]
    TemperatureRange { field: &'static str, value: f64 },

    #[error(
        "bounds.setpoint_floor_c resolves above bounds.setpoint_ceiling_c ({floor} > {ceiling})"
    )]
    SetpointRangeInverted { floor: f64, ceiling: f64 },

    #[error("bounds.steam_floor_f resolves above bounds.steam_ceiling_f ({floor} > {ceiling})")]
    SteamRangeInverted { floor: f64, ceiling: f64 },

    #[error(
        "a pseudo-terminal was supplied for {link}, which this configuration \
         does not bind to a placeholder"
    )]
    NotAPlaceholder { link: String },

    #[error("no pseudo-terminal was supplied for {link}, which is configured as a placeholder")]
    PlaceholderUnbound { link: String },
}

impl ConfigError {
    /// The slot list an [`ConfigError::InstrumentedSlotNotConfigured`] prints.
    pub(crate) fn slot_list(slots: impl IntoIterator<Item = Slot>) -> String {
        let mut out = String::new();
        for s in slots {
            if !out.is_empty() {
                out.push_str(", ");
            }
            out.push_str(&s.get().to_string());
        }
        if out.is_empty() {
            out.push_str("none");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_the_field_and_the_reason() {
        let e = ConfigError::UnstablePortPath {
            field: "zones.zone2.port".to_owned(),
            path: "/dev/ttyUSB1".to_owned(),
        };
        let text = e.to_string();
        assert!(text.contains("zones.zone2.port"));
        assert!(text.contains("/dev/ttyUSB1"));
        assert!(text.contains("by-id"));
    }

    #[test]
    fn slot_lists_read_as_lists() {
        let slots = [1u8, 3, 5].map(|n| Slot::new(n).unwrap());
        assert_eq!(ConfigError::slot_list(slots), "1, 3, 5");
        assert_eq!(ConfigError::slot_list([]), "none");
    }
}
