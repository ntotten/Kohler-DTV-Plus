//! The TOML file, exactly as written, before any of it means anything.
//!
//! One deserialisation step, one validation step. The types here mirror
//! `deploy/kdtvd.toml` key for key and carry no invariants at all — a `u8` here
//! is a number someone typed, not a slot.
//!
//! Every struct is `deny_unknown_fields`. A misspelled key is a refusal that
//! names the line, not a setting silently ignored: `frames = ture` reading as
//! "frame logging is off" is exactly the class of failure that leaves you with
//! no evidence after the event you needed it for.

use crate::gate::GateScope;
use crate::profile::Profile;
use crate::zone::ConfiguredValve;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    pub(crate) profile: Profile,
    #[serde(default)]
    pub(crate) bench: Option<RawBench>,
    #[serde(default)]
    pub(crate) bounds: Option<RawBounds>,
    pub(crate) transmit_gate: RawGate,
    pub(crate) zones: RawZones,
    #[serde(default)]
    pub(crate) steam: Option<RawSteam>,
    pub(crate) sensors: RawSensors,
    pub(crate) api: RawApi,
    pub(crate) logging: RawLogging,
}

/// Bench-only facilities. Refused outright under
/// [`Profile::Production`].
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawBench {
    /// Applied to session-class durations and to nothing else. See
    /// [`crate::timing`].
    pub(crate) session_scale: f64,
    /// Where the harness writes each zone's independent temperature, in place
    /// of a MAX31865 on the SPI bus. See
    /// [`ValidatedConfig::bench_probe_dir`](crate::ValidatedConfig::bench_probe_dir).
    pub(crate) probe_dir: Option<String>,
}

/// Optional narrowing of the compiled-in safety bounds. Every field may only
/// tighten; a widening value is refused by name.
#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawBounds {
    pub(crate) setpoint_ceiling_c: Option<f64>,
    pub(crate) setpoint_floor_c: Option<f64>,
    pub(crate) max_session_s: Option<u64>,
    pub(crate) steam_ceiling_f: Option<f64>,
    pub(crate) steam_floor_f: Option<f64>,
    pub(crate) steam_max_minutes: Option<u8>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawGate {
    pub(crate) scope: GateScope,
    #[serde(default)]
    pub(crate) capture_ref: Option<String>,
    #[serde(default)]
    pub(crate) fixtures_sha256: Option<String>,
    #[serde(default)]
    pub(crate) polarity: Option<BTreeMap<String, String>>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawZones {
    pub(crate) zone1: RawZone,
    pub(crate) zone2: RawZone,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawZone {
    pub(crate) port: String,
    pub(crate) valve: ConfiguredValve,
    pub(crate) master_address: u8,
    pub(crate) outlets: Vec<RawOutlet>,
    pub(crate) instrumented_slot: u8,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawOutlet {
    pub(crate) slot: u8,
    pub(crate) status_index: u8,
    pub(crate) wire_outlet: u8,
    pub(crate) name: String,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSteam {
    pub(crate) enabled: bool,
    #[serde(default)]
    pub(crate) port: Option<String>,
    #[serde(default)]
    pub(crate) tick_ms: Option<u64>,
    #[serde(default)]
    pub(crate) retries: Option<u8>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSensors {
    pub(crate) zone1: RawSensor,
    pub(crate) zone2: RawSensor,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawSensor {
    pub(crate) chip_select: String,
    #[serde(default)]
    pub(crate) correction: Option<Vec<RawCurvePoint>>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCurvePoint {
    pub(crate) surface_c: f64,
    pub(crate) immersion_c: f64,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawApi {
    pub(crate) bind: String,
    pub(crate) token_file: String,
    pub(crate) session_ttl_s: u64,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawLogging {
    pub(crate) directory: String,
    pub(crate) frames: bool,
    pub(crate) max_total_mb: u64,
}
