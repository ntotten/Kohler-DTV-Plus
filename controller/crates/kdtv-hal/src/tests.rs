//! Crate-level assertions: the shapes this crate promises, and the ones it
//! promises not to have.

use crate::{
    Backend, Clock, IdStore, LineSettings, Link, LinkFactory, NO_GPIO_OUTPUT, NtpProbe, SysfsView,
    Watchdog,
};
use kdtv_units::LinkKind;

/// `GPIO-03`. The ban is restated workspace-wide by `cargo xtask audit-graph`;
/// this is the crate-local half, so a dependency added here fails a test in the
/// same crate rather than only in the audit job.
#[test]
fn req_hardware_spec_gpio_03_no_gpio_crate_is_a_dependency() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let deps = manifest
        .split("# There is deliberately no")
        .next()
        .expect("manifest has a dependency section");
    for banned in ["rppal", "gpiod", "linux-embedded-hal", "sysfs_gpio"] {
        assert!(
            !deps.contains(banned),
            "{banned} is a dependency of kdtv-hal; {NO_GPIO_OUTPUT}"
        );
    }
}

/// The traits the supervisor holds as `dyn`. If one stops being object-safe the
/// composition root cannot be written, and the failure would otherwise show up
/// three crates away. Naming the `dyn` type is the whole assertion.
#[test]
fn the_boundary_traits_are_object_safe() {
    let link: Option<&dyn Link> = None;
    let factory: Option<&dyn LinkFactory> = None;
    let clock: Option<&dyn Clock> = None;
    let watchdog: Option<&dyn Watchdog> = None;
    let ids: Option<&dyn IdStore> = None;
    let sysfs: Option<&dyn SysfsView> = None;
    let ntp: Option<&dyn NtpProbe> = None;
    assert!(
        link.is_none()
            && factory.is_none()
            && clock.is_none()
            && watchdog.is_none()
            && ids.is_none()
            && sysfs.is_none()
            && ntp.is_none()
    );
}

/// Every link this system has is covered by line settings and, where it is a
/// real bus, by the gate.
#[test]
fn every_link_kind_has_line_settings_and_only_serial_is_gated() {
    for link in LinkKind::ALL {
        let settings = LineSettings::for_link(link);
        assert_eq!(settings.baud, 9600, "{link}");
        assert_eq!((settings.data_bits, settings.stop_bits), (8, 1), "{link}");
    }
    assert!(Backend::Serial.is_real_bus());
    assert!(!Backend::Pty.is_real_bus());
    assert!(!Backend::Loopback.is_real_bus());
}

/// The committed configuration, the committed fixture tree, and the gate, end
/// to end: `deploy/kdtvd.toml` binds to real device nodes, and every one of them
/// is then refused at open.
///
/// This is the claim the README makes, tested rather than asserted: the daemon
/// gets all the way to the port and cannot go through it.
#[tokio::test]
async fn the_deployed_configuration_binds_and_is_then_refused_at_the_gate() {
    use kdtv_config::{FsEntry, MapFs, ValidatedConfig};
    use kdtv_proto::{FixtureSet, TransmitAuthority};
    use std::path::Path;

    const TOKEN: &str = "/run/credentials/kdtvd.service/api-token";
    let toml = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/kdtvd.toml");

    // The reference installation as udev presents it, which is the same machine
    // `fixtures/sysfs/reference` describes.
    let config_fs = MapFs::new()
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
        .with(TOKEN, FsEntry::own(TOKEN).with_mode(0o400));

    let cfg = ValidatedConfig::load(&toml, &config_fs).unwrap();
    let bindings = crate::bindings_of(&cfg);
    // Steam is disabled in the committed file, so it is not bound. A link that
    // is not driven is not opened.
    assert_eq!(bindings.len(), 2, "{bindings:?}");

    let sysfs = crate::DirSysfs::fixture("reference");
    let bound = crate::resolve_distinct(&bindings, &sysfs).unwrap();
    assert_eq!(bound.len(), 2);
    assert!(bound.iter().all(|b| b.backend() == Backend::Serial));

    let auth = TransmitAuthority::emulator_only(FixtureSet::embedded());
    let mut factory = crate::LinuxLinkFactory::new(sysfs);
    for binding in &bound {
        let err = factory.open(binding, &auth).await.unwrap_err();
        assert!(err.is_gate(), "{} opened: {err:?}", binding.link());
    }
}
