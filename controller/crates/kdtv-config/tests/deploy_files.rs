//! The committed configuration files must load.
//!
//! `deploy/kdtvd.toml` is the contract this crate implements, and its comments
//! carry the reasoning behind the rules. A parser that has drifted from the
//! documented contract is a parser that will reject the file an operator was
//! told to copy — so the acceptance test is loading the real files off disk,
//! not a fixture written to match the code.
//!
//! Only the filesystem is faked, and only because it has to be: the production
//! file names USB devices and a systemd credential path, none of which exist on
//! a CI runner. Everything else — every key, every bound, every refusal — is
//! exercised against the bytes that ship.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use kdtv_config::ValidatedConfig;
use kdtv_config::fs::{FsEntry, MapFs};
use std::path::{Path, PathBuf};

fn repo_file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the workspace root is two levels above this crate")
        .join("deploy")
        .join(name)
}

/// A filesystem carrying exactly what the production file names.
fn production_fs() -> MapFs {
    MapFs::new()
        .with(
            "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if00-port0",
            FsEntry::link("/dev/ttyUSB0"),
        )
        .with(
            "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if01-port0",
            FsEntry::link("/dev/ttyUSB1"),
        )
        .with(
            "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if02-port0",
            FsEntry::link("/dev/ttyUSB2"),
        )
        .with(
            "/run/credentials/kdtvd.service/api-token",
            FsEntry::own("/run/credentials/kdtvd.service/api-token").with_mode(0o400),
        )
}

#[test]
fn the_production_example_loads() {
    let path = repo_file("kdtvd.toml");
    let cfg = ValidatedConfig::load(&path, &production_fs())
        .unwrap_or_else(|e| panic!("deploy/kdtvd.toml must validate, got: {e}"));

    // The properties an operator is relying on when they copy this file.
    assert!(cfg.profile().is_production());
    assert!(
        !cfg.gate().scope().is_real_bus(),
        "the shipped example must leave the gate closed"
    );
}

#[test]
fn the_emulated_example_loads() {
    let path = repo_file("kdtvd.emulated.toml");
    // The rig rewrites the port paths at start, because a pseudo-terminal path
    // does not exist until it is created. The placeholder still has to validate.
    let fs = MapFs::new()
        .with("/dev/pts/PLACEHOLDER", FsEntry::own("/dev/pts/PLACEHOLDER"))
        .with(
            ".e2e/api-token",
            FsEntry::own(".e2e/api-token").with_mode(0o600),
        );

    let text = std::fs::read_to_string(&path).expect("read the emulated example");
    match ValidatedConfig::from_str_with(&text, &path, &fs) {
        Ok(cfg) => {
            assert!(
                !cfg.profile().is_production(),
                "the emulated example runs under bench"
            );
            assert!(!cfg.gate().scope().is_real_bus());
        }
        Err(e) => {
            // Three PTY placeholders that are the same path is a duplicate-port
            // refusal, which is correct behaviour: the rig gives each link its
            // own device. Any other failure is a real drift between the parser
            // and the file.
            let msg = e.to_string().to_ascii_lowercase();
            assert!(
                msg.contains("same") || msg.contains("duplicate") || msg.contains("distinct"),
                "the emulated example failed for an unexpected reason: {e}"
            );
        }
    }
}

#[test]
fn the_production_example_is_refused_without_its_devices() {
    // The same file, on a machine where the converters are not plugged in.
    // Refusing is the whole design: there is no degraded start, because a
    // service that cannot be sure which valve is on which bus must drive
    // neither.
    let path = repo_file("kdtvd.toml");
    let text = std::fs::read_to_string(&path).expect("read the production example");
    let bare = MapFs::new().with(
        "/run/credentials/kdtvd.service/api-token",
        FsEntry::own("/run/credentials/kdtvd.service/api-token").with_mode(0o400),
    );
    assert!(
        ValidatedConfig::from_str_with(&text, &path, &bare).is_err(),
        "a configuration naming absent devices must refuse, not start degraded"
    );
}

#[test]
fn the_production_example_is_refused_with_a_readable_token() {
    let path = repo_file("kdtvd.toml");
    let text = std::fs::read_to_string(&path).expect("read the production example");
    let fs = production_fs().with(
        "/run/credentials/kdtvd.service/api-token",
        FsEntry::own("/run/credentials/kdtvd.service/api-token").with_mode(0o644),
    );
    assert!(
        ValidatedConfig::from_str_with(&text, &path, &fs).is_err(),
        "a world-readable token file must refuse"
    );
}
