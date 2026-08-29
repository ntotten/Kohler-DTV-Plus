//! Crate-level assertions: the shapes this crate promises, and the ones it
//! promises not to have.

use crate::{
    Backend, ChipSelect, Clock, IdStore, LineSettings, Link, LinkFactory, NO_GPIO_OUTPUT, NtpProbe,
    RtdChannel, SysfsView, Watchdog,
};
use kdtv_units::{LinkKind, ZoneId};

/// `GPIO-03`. The ban is restated workspace-wide by `cargo xtask audit-graph`;
/// this is the crate-local half, so a dependency added here fails a test in the
/// same crate rather than only in the audit job.
#[test]
fn no_gpio_crate_is_a_dependency() {
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
    let rtd: Option<&dyn RtdChannel> = None;
    let clock: Option<&dyn Clock> = None;
    let watchdog: Option<&dyn Watchdog> = None;
    let ids: Option<&dyn IdStore> = None;
    let sysfs: Option<&dyn SysfsView> = None;
    let ntp: Option<&dyn NtpProbe> = None;
    assert!(
        link.is_none()
            && factory.is_none()
            && rtd.is_none()
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

/// The chip-select mapping restated once more from outside the module, because
/// it is the thing configuration is checked against.
#[test]
fn the_zone_chip_selects_are_the_ones_the_deployed_config_names() {
    assert_eq!(
        ChipSelect::check(ZoneId::Zone1, "spi0.0")
            .unwrap()
            .to_string(),
        "spi0.0"
    );
    assert_eq!(
        ChipSelect::check(ZoneId::Zone2, "spi0.1")
            .unwrap()
            .to_string(),
        "spi0.1"
    );
}
