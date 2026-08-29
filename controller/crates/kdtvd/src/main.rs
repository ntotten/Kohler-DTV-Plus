//! The replacement master daemon.
//!
//! # Status
//!
//! `--check-only` is implemented, and it is the mode that matters first:
//! `scripts/deploy.sh` runs it **on the Pi** against the staged configuration
//! before it replaces the live binary. A configuration that is wrong on the
//! target is a service that refuses to start, and finding that out while the
//! old binary is still in place is the difference between a failed deployment
//! and no shower.
//!
//! The run path lands with the service crate. Nothing here opens a link or
//! transmits a byte.

// Tests legitimately panic on a broken invariant.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

/// Exit codes, so a deployment script can tell the cases apart.
mod exit {
    /// Everything validated.
    pub(crate) const OK: u8 = 0;
    /// The configuration is wrong. Nothing was opened.
    pub(crate) const CONFIG: u8 = 2;
    /// The configuration is valid but the hardware it names is not there,
    /// or two links resolve to one device.
    pub(crate) const HARDWARE: u8 = 3;
    /// The configuration asks to transmit on a real bus and the evidence does
    /// not support it. Distinct from CONFIG because it is the one refusal that
    /// is about the state of the investigation rather than about the file.
    pub(crate) const GATE: u8 = 4;
}

#[derive(Parser, Debug)]
#[command(
    name = "kdtvd",
    about = "Replacement master for the Kohler DTV+",
    long_about = None
)]
struct Cli {
    /// The configuration file.
    #[arg(long, value_name = "PATH", default_value = "/etc/kdtvd/kdtvd.toml")]
    config: PathBuf,

    /// Validate and exit without opening a link or transmitting anything.
    ///
    /// This is what a deployment runs on the target before it installs.
    #[arg(long)]
    check_only: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if !cli.check_only {
        eprintln!(
            "kdtvd: the run path is not implemented yet; only --check-only is available.\n\
             Nothing was opened and nothing was transmitted."
        );
        return ExitCode::from(exit::CONFIG);
    }
    match check_only(&cli.config) {
        Ok(()) => ExitCode::from(exit::OK),
        Err(e) => {
            eprintln!("kdtvd: {e:#}");
            ExitCode::from(
                e.downcast_ref::<CheckFailure>()
                    .map_or(exit::CONFIG, CheckFailure::code),
            )
        }
    }
}

/// A check that failed, carrying which kind it was.
#[derive(Debug, thiserror::Error)]
enum CheckFailure {
    #[error("configuration: {0}")]
    Config(String),
    #[error("hardware: {0}")]
    Hardware(String),
    #[error("transmit gate: {0}")]
    Gate(String),
}

impl CheckFailure {
    const fn code(&self) -> u8 {
        match self {
            Self::Config(_) => exit::CONFIG,
            Self::Hardware(_) => exit::HARDWARE,
            Self::Gate(_) => exit::GATE,
        }
    }
}

/// Validate everything that can be validated without opening a link.
///
/// The order is deliberate: the file first, then the hardware it names, then
/// whether it may transmit. Each answers a different question, and a caller
/// reading the output top to bottom sees them in the order they would have to be
/// fixed.
fn check_only(path: &std::path::Path) -> Result<()> {
    use kdtv_hal::{RealSysfs, bindings_of, resolve_distinct};

    println!("kdtvd --check-only");
    println!("  config: {}", path.display());

    // 1. The file. Parses, and every rule in it holds.
    let fs = kdtv_config::fs::RealFs;
    let cfg = kdtv_config::ValidatedConfig::load(path, &fs)
        .map_err(|e| classify(&e))
        .context("the configuration did not validate")?;
    println!("  profile: {:?}", cfg.profile());
    println!("  ok  configuration validates");

    // 2. The hardware it names. Present, and distinct — two by-id names can
    //    symlink one device, which is the confusion the by-id rule exists to
    //    prevent, so the comparison is on the resolved path.
    let bindings = bindings_of(&cfg);
    match resolve_distinct(&bindings, &RealSysfs::new()) {
        Ok(ports) => {
            for p in &ports {
                println!("  ok  {} -> {}", p.link(), p.port().device().display());
            }
        }
        Err(e) => {
            return Err(CheckFailure::Hardware(e.to_string()))
                .context("the configured links did not resolve");
        }
    }

    // 3. Whether it may transmit on a real bus. This is the one refusal that is
    //    about the state of the investigation rather than about the file: no
    //    frame in this workspace has been verified against the hardware, so
    //    until Phase 1 capture promotes the fixtures the gate stays shut.
    let fixtures = kdtv_proto::fixtures::FixtureSet::embedded();
    match kdtv_proto::gate::TransmitAuthority::resolve(&gate_request(cfg.gate()), fixtures) {
        Ok(auth) => {
            if auth.permits_real_bus() {
                println!("  ok  transmit gate: real bus attested");
            } else {
                println!("  ok  transmit gate: emulator only — no real port will be opened");
            }
        }
        Err(e) => {
            return Err(CheckFailure::Gate(e.to_string())).context(
                "the configuration claims a real-bus attestation the fixtures do not support",
            );
        }
    }

    println!("  all checks passed");
    Ok(())
}

/// Which kind of failure a configuration error is.
///
/// `kdtv-config` validates the file *and* resolves the devices it names, because
/// it has to: two `by-id` names can symlink one `ttyUSB`, and comparing the
/// configured strings would miss that. So a missing converter arrives here as a
/// configuration error, and it is not one — the file is right and the hardware
/// is absent, which needs a different fix and gets a different exit code.
///
/// Two variants are about hardware. Everything else is about the file.
fn classify(e: &kdtv_config::ConfigError) -> CheckFailure {
    use kdtv_config::ConfigError as E;
    match e {
        E::PortAbsent { .. } | E::DuplicatePort { .. } => CheckFailure::Hardware(e.to_string()),
        _ => CheckFailure::Config(e.to_string()),
    }
}

/// Translate the configuration's gate section into the codec's request.
///
/// The two crates each define their own type on purpose: `kdtv-config` must not
/// depend on `kdtv-proto` to describe a file, and `kdtv-proto` must not depend
/// on `kdtv-config` to check evidence. Joining them is the composition root's
/// job, and this is the composition root.
///
/// **One field does not survive the crossing cleanly.** `kdtv-proto` wants the
/// attestation date as its own field, so a stale attestation is visible without
/// reading prose. The configuration schema keeps it inside the free-text note —
/// the shipped example writes "A+ = converter TA, measured 2026-XX-XX" — so the
/// whole string is carried as the note and the date field says where to look.
/// Worth tightening in the schema before Phase 1, when the first real
/// attestation is written; recorded here rather than silently dropped.
fn gate_request(cfg: &kdtv_config::TransmitGateConfig) -> kdtv_proto::gate::TransmitGateConfig {
    use kdtv_proto::gate::{PolarityAttestation, PolarityNote, RequestedScope};

    kdtv_proto::gate::TransmitGateConfig {
        scope: if cfg.scope().is_real_bus() {
            RequestedScope::RealBusAttested
        } else {
            RequestedScope::EmulatorOnly
        },
        capture_ref: cfg.capture_ref().map(ToOwned::to_owned),
        polarity: PolarityAttestation {
            notes: cfg
                .attested_links()
                .filter_map(|link| {
                    cfg.polarity(link).map(|note| PolarityNote {
                        link,
                        note: note.to_owned(),
                        attested_on: "see note".to_owned(),
                    })
                })
                .collect(),
        },
        expected_fixtures_sha256: cfg.fixtures_sha256().map(ToOwned::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exit_codes_are_distinct() {
        // A deployment script branches on these, so two meaning the same thing
        // would make a gate refusal look like a typo in the file.
        let codes = [exit::OK, exit::CONFIG, exit::HARDWARE, exit::GATE];
        let distinct: std::collections::BTreeSet<u8> = codes.iter().copied().collect();
        assert_eq!(distinct.len(), codes.len());
    }

    #[test]
    fn each_failure_kind_reports_its_own_code() {
        assert_eq!(CheckFailure::Config(String::new()).code(), exit::CONFIG);
        assert_eq!(CheckFailure::Hardware(String::new()).code(), exit::HARDWARE);
        assert_eq!(CheckFailure::Gate(String::new()).code(), exit::GATE);
    }

    #[test]
    fn a_missing_configuration_file_is_a_configuration_failure() {
        let e = check_only(std::path::Path::new("/nonexistent/kdtvd.toml"))
            .expect_err("a missing file must fail");
        assert_eq!(
            e.downcast_ref::<CheckFailure>().map(CheckFailure::code),
            Some(exit::CONFIG)
        );
    }

    #[test]
    fn the_committed_production_example_fails_on_hardware_not_on_syntax() {
        // The example names USB converters this machine does not have. It must
        // get past parsing and validation and fail at resolution — which is the
        // evidence that the file itself is right and only the hardware is
        // absent. If this ever reports a configuration failure, the shipped
        // example has drifted from the parser.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root")
            .join("deploy/kdtvd.toml");
        if !path.exists() {
            return;
        }
        match check_only(&path) {
            Ok(()) => { /* a machine that really has the converters */ }
            Err(e) => {
                let code = e.downcast_ref::<CheckFailure>().map(CheckFailure::code);
                assert_ne!(
                    code,
                    Some(exit::CONFIG),
                    "the shipped example must not fail validation: {e:#}"
                );
            }
        }
    }
}
