//! Whole-file tests: the two committed contracts, and one test per refusal.
//!
//! `deploy/kdtvd.toml` and `deploy/kdtvd.emulated.toml` are loaded from disk
//! rather than copied here. That is the point — a change to either file that
//! this crate cannot parse is a failing test, not a service that will not start
//! on the target.
//!
//! The mutation tests take the committed production file and change exactly one
//! thing, so each refusal is demonstrated against the real contract rather than
//! against a minimal fixture that might not resemble it.

use super::*;
use crate::fs::FsEntry;
use kdtv_proto::saturn::{MasterAddr, ValveType};
use kdtv_units::{Cx2, Fx2, SessionDuration, Slot, SteamMinutes, SteamSetpoint, ValveSetpoint};

const PRODUCTION_TOML: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/kdtvd.toml");
const EMULATED_TOML: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../deploy/kdtvd.emulated.toml"
);

const ZONE1_PORT: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if00-port0";
const ZONE2_PORT: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if01-port0";
const STEAM_PORT: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if02-port0";
const TOKEN: &str = "/run/credentials/kdtvd.service/api-token";

/// The reference installation, as udev would present it: three converter
/// interfaces symlinked to three separate `ttyUSB` nodes, and a systemd
/// credential readable only by the service.
fn installed() -> MapFs {
    MapFs::new()
        .with(ZONE1_PORT, FsEntry::link("/dev/ttyUSB0"))
        .with(ZONE2_PORT, FsEntry::link("/dev/ttyUSB1"))
        .with(STEAM_PORT, FsEntry::link("/dev/ttyUSB2"))
        .with(TOKEN, FsEntry::own(TOKEN).with_mode(0o400))
}

/// The bench rig's filesystem: no real converters, and a token file inside the
/// rig's own working tree.
fn bench_fs() -> MapFs {
    MapFs::new().with(
        ".e2e/api-token",
        FsEntry::own(".e2e/api-token").with_mode(0o600),
    )
}

fn production_text() -> String {
    std::fs::read_to_string(PRODUCTION_TOML).expect("deploy/kdtvd.toml is missing")
}

fn load_production() -> Result<ValidatedConfig, ConfigError> {
    ValidatedConfig::from_str_with(&production_text(), Path::new(PRODUCTION_TOML), &installed())
}

/// Loads the committed production file with one substring replaced.
fn mutated(from: &str, to: &str) -> Result<ValidatedConfig, ConfigError> {
    let text = production_text();
    assert!(
        text.contains(from),
        "the contract no longer contains {from:?}"
    );
    let text = text.replacen(from, to, 1);
    ValidatedConfig::from_str_with(&text, Path::new(PRODUCTION_TOML), &installed())
}

// ---------------------------------------------------------------- acceptance

/// `deploy/kdtvd.toml`, from disk, through the real loader.
#[test]
fn the_committed_production_contract_loads() {
    let c = ValidatedConfig::load(Path::new(PRODUCTION_TOML), &installed()).unwrap();

    assert_eq!(c.profile(), Profile::Production);
    assert_eq!(c.gate().scope(), GateScope::EmulatorOnly);
    assert_eq!(c.gate().capture_ref(), None);
    assert_eq!(c.gate().fixtures_sha256(), None);

    let z1 = c.zone(ZoneId::Zone1);
    assert_eq!(z1.expected_valve(), ValveType::Dtv6Port);
    assert_eq!(z1.master(), MasterAddr::Dtv);
    assert_eq!(z1.port().to_string(), ZONE1_PORT);
    assert_eq!(z1.configured_slots().len(), 5);
    assert_eq!(z1.instrumented_slot().get(), 1);
    assert_eq!(z1.label(Slot::new(5).unwrap()), Some("bath filler"));
    // Slot 1 on a DTV 6-Port is wire outlet 0, mask 0x01.
    let bits = z1
        .outlets()
        .bitmap([Slot::new(1).unwrap()].into_iter().collect())
        .unwrap();
    assert_eq!(bits.bits(), 0x01);

    let z2 = c.zone(ZoneId::Zone2);
    assert_eq!(z2.expected_valve(), ValveType::Prompt3Port);
    assert_eq!(z2.configured_slots().len(), 3);
    // The same slot number on the other family is wire outlet 1, mask 0x04.
    let bits = z2
        .outlets()
        .bitmap([Slot::new(1).unwrap()].into_iter().collect())
        .unwrap();
    assert_eq!(bits.bits(), 0x04);

    // steam.enabled = false, so there is no steam configuration to reach for.
    assert!(c.steam().is_none());
    assert_eq!(c.links(), vec![z1.link(), z2.link()]);

    assert_eq!(c.sensor(ZoneId::Zone1).chip_select(), "spi0.0");
    assert!(!c.sensor(ZoneId::Zone1).is_characterised());
    assert_eq!(c.sensor(ZoneId::Zone2).chip_select(), "spi0.1");

    assert_eq!(c.api().bind().port(), 8443);
    assert!(c.api().bind().ip().is_loopback());
    assert_eq!(c.api().session_ttl(), Duration::from_secs(900));
    assert_eq!(c.logging().directory(), Path::new("/var/log/kdtvd"));
    assert!(c.logging().frames());
    assert_eq!(c.logging().max_total_mb(), 512);

    // No [bounds] table: the compiled-in bounds stand unchanged.
    assert_eq!(c.bounds(), Bounds::COMPILED);
    assert!(c.timing().session_scale().is_unscaled());
    assert_eq!(c.timing().saturn(), Timings::DOCUMENTED);
}

/// `deploy/kdtvd.emulated.toml`, from disk, through the real loader.
#[test]
fn the_committed_bench_contract_loads() {
    let c = ValidatedConfig::load(Path::new(EMULATED_TOML), &bench_fs()).unwrap();

    assert_eq!(c.profile(), Profile::Bench);
    assert_eq!(c.gate().scope(), GateScope::EmulatorOnly);
    for z in c.zones() {
        assert!(z.port().is_placeholder(), "{}", z.port());
    }
    let steam = c.steam().expect("the emulated rig enables steam");
    assert!(steam.port().is_placeholder());
    assert_eq!(steam.timings().tick, Duration::from_millis(150));
    assert_eq!(steam.timings().retries, 4);
    assert_eq!(c.links().len(), 3);
    assert_eq!(c.api().token_file(), Path::new(".e2e/api-token"));
    assert_eq!(c.logging().directory(), Path::new(".e2e/logs"));

    // Three placeholders are not three links on one port. They are three links
    // the rig has yet to bind.
    assert_eq!(c.bounds(), Bounds::COMPILED);
}

/// The rig's substitution step, and the check that survives it.
#[test]
fn the_rig_binds_real_pseudo_terminals_over_the_placeholders() {
    let c = ValidatedConfig::load(Path::new(EMULATED_TOML), &bench_fs()).unwrap();
    let fs = bench_fs()
        .with_device("/dev/pts/3")
        .with_device("/dev/pts/4")
        .with_device("/dev/pts/5");

    let mut ptys = BTreeMap::new();
    ptys.insert(LinkKind::Zone(ZoneId::Zone1), PathBuf::from("/dev/pts/3"));
    ptys.insert(LinkKind::Zone(ZoneId::Zone2), PathBuf::from("/dev/pts/4"));
    ptys.insert(LinkKind::Steam, PathBuf::from("/dev/pts/5"));

    let bound = c.bind_ptys(&ptys, &fs).unwrap();
    assert_eq!(bound.zone(ZoneId::Zone1).port().to_string(), "/dev/pts/3");
    assert_eq!(bound.zone(ZoneId::Zone2).port().to_string(), "/dev/pts/4");
    assert_eq!(bound.steam().unwrap().port().to_string(), "/dev/pts/5");
    assert!(!bound.zone(ZoneId::Zone1).port().is_placeholder());

    // Two links, one terminal: refused after substitution exactly as before it.
    let mut collided = ptys.clone();
    collided.insert(LinkKind::Zone(ZoneId::Zone2), PathBuf::from("/dev/pts/3"));
    assert!(matches!(
        c.bind_ptys(&collided, &fs),
        Err(ConfigError::DuplicatePort { .. })
    ));

    // A link left unbound has nothing to open.
    let mut short = ptys.clone();
    short.remove(&LinkKind::Steam);
    assert!(matches!(
        c.bind_ptys(&short, &fs),
        Err(ConfigError::PlaceholderUnbound { .. })
    ));

    // And a production configuration has no placeholder to substitute.
    let prod = load_production().unwrap();
    assert!(matches!(
        prod.bind_ptys(&ptys, &installed()),
        Err(ConfigError::NotAPlaceholder { .. })
    ));
}

// ------------------------------------------------------------------ refusals

/// The refusal `SER-01` / `USB-02` exists for.
#[test]
fn a_dev_tty_port_is_refused() {
    let err = mutated(ZONE1_PORT, "/dev/ttyUSB0").unwrap_err();
    let text = err.to_string();
    assert!(
        matches!(err, ConfigError::UnstablePortPath { .. }),
        "{text}"
    );
    assert!(text.contains("zones.zone1.port"), "{text}");
    assert!(text.contains("/dev/ttyUSB0"), "{text}");
    assert!(text.contains("zone 1 silently"), "{text}");
}

#[test]
fn two_zones_naming_one_port_are_refused() {
    let err = mutated(ZONE2_PORT, ZONE1_PORT).unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, ConfigError::DuplicatePort { .. }), "{text}");
    assert!(text.contains("zones.zone1.port"), "{text}");
    assert!(text.contains("zones.zone2.port"), "{text}");
}

/// The harder half of the same rule: two different `by-id` names that are both
/// symlinks to one `ttyUSB`. Comparing the configured strings would accept it.
#[test]
fn two_zones_resolving_through_symlinks_to_one_device_are_refused() {
    let fs = MapFs::new()
        .with(ZONE1_PORT, FsEntry::link("/dev/ttyUSB0"))
        .with(ZONE2_PORT, FsEntry::link("/dev/ttyUSB0"))
        .with(STEAM_PORT, FsEntry::link("/dev/ttyUSB2"))
        .with(TOKEN, FsEntry::own(TOKEN).with_mode(0o400));
    let err = ValidatedConfig::from_str_with(&production_text(), Path::new(PRODUCTION_TOML), &fs)
        .unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, ConfigError::DuplicatePort { .. }), "{text}");
    assert!(text.contains("/dev/ttyUSB0"), "{text}");
}

#[test]
fn a_port_that_is_not_present_is_refused_under_production() {
    let fs = installed().without(ZONE2_PORT);
    let err = ValidatedConfig::from_str_with(&production_text(), Path::new(PRODUCTION_TOML), &fs)
        .unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, ConfigError::PortAbsent { .. }), "{text}");
    assert!(text.contains("zones.zone2.port"), "{text}");
}

#[test]
fn a_duplicate_slot_in_an_outlet_table_is_refused() {
    let err = mutated(
        "{ slot = 2, status_index = 2, wire_outlet = 1, name = \"handshower\" }",
        "{ slot = 1, status_index = 2, wire_outlet = 1, name = \"handshower\" }",
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("zones.zone1.outlets"), "{text}");
    assert!(text.contains("appears twice"), "{text}");
}

#[test]
fn a_duplicate_status_index_in_an_outlet_table_is_refused() {
    let err = mutated(
        "{ slot = 2, status_index = 2, wire_outlet = 1, name = \"handshower\" }",
        "{ slot = 2, status_index = 1, wire_outlet = 1, name = \"handshower\" }",
    )
    .unwrap_err();
    assert!(err.to_string().contains("status index 1"), "{err}");
}

/// A DTV 6-Port numbers its outlets 0..5; there is no wire outlet 6.
#[test]
fn a_wire_outlet_the_family_does_not_have_is_refused() {
    let err = mutated(
        "{ slot = 5, status_index = 5, wire_outlet = 4, name = \"bath filler\" }",
        "{ slot = 5, status_index = 5, wire_outlet = 6, name = \"bath filler\" }",
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("zones.zone1.outlets"), "{text}");
    assert!(text.contains("DTV 6-Port"), "{text}");
}

/// And the other family, whose numbering starts at 1: there is no wire outlet 0
/// on a Prompt 3, and the reference zone 2 table is the one that proves the two
/// numbering spaces are not interchangeable.
#[test]
fn a_prompt_valve_has_no_wire_outlet_zero() {
    let err = mutated(
        "{ slot = 1, status_index = 1, wire_outlet = 1, name = \"overhead\" }",
        "{ slot = 1, status_index = 1, wire_outlet = 0, name = \"overhead\" }",
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("zones.zone2.outlets"), "{text}");
    assert!(text.contains("Prompt 3-Port"), "{text}");
}

#[test]
fn an_instrumented_slot_that_is_not_configured_is_refused() {
    // Zone 2 has slots 1..=3.
    let err = mutated(
        "instrumented_slot = 1\n\n# ------",
        "instrumented_slot = 6\n\n# ------",
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("instrumented_slot = 6"), "{text}");
    assert!(text.contains("configured: 1, 2, 3"), "{text}");
}

#[test]
fn a_pty_under_production_is_refused() {
    let err = mutated(ZONE1_PORT, "/dev/pts/4").unwrap_err();
    let text = err.to_string();
    assert!(
        matches!(err, ConfigError::PtyUnderProduction { .. }),
        "{text}"
    );
    assert!(text.contains("zones.zone1.port"), "{text}");
}

#[test]
fn time_scaling_under_production_is_refused() {
    let err = mutated(
        "profile = \"production\"",
        "profile = \"production\"\n[bench]\nsession_scale = 0.01",
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::BenchTableUnderProduction),
        "{err}"
    );
    assert!(err.to_string().contains("bench"), "{err}");

    // Even a scale of 1.0: the table itself is the bench facility.
    let err = mutated(
        "profile = \"production\"",
        "profile = \"production\"\n[bench]\nsession_scale = 1.0",
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::BenchTableUnderProduction),
        "{err}"
    );
}

#[test]
fn steam_enabled_without_a_port_is_refused() {
    let text = production_text()
        .replacen("enabled = false", "enabled = true", 1)
        .replacen(&format!("port = \"{STEAM_PORT}\"\n"), "", 1);
    let err = ValidatedConfig::from_str_with(&text, Path::new(PRODUCTION_TOML), &installed())
        .unwrap_err();
    assert!(matches!(err, ConfigError::SteamEnabledWithoutPort), "{err}");
}

#[test]
fn a_missing_token_file_is_refused() {
    let err = ValidatedConfig::from_str_with(
        &production_text(),
        Path::new(PRODUCTION_TOML),
        &installed().without(TOKEN),
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(
        matches!(err, ConfigError::TokenFileMissing { .. }),
        "{text}"
    );
    assert!(text.contains(TOKEN), "{text}");
}

#[test]
fn a_world_readable_token_file_is_refused() {
    let fs = installed().with(TOKEN, FsEntry::own(TOKEN).with_mode(0o644));
    let err = ValidatedConfig::from_str_with(&production_text(), Path::new(PRODUCTION_TOML), &fs)
        .unwrap_err();
    let text = err.to_string();
    assert!(
        matches!(err, ConfigError::TokenFileWorldReadable { .. }),
        "{text}"
    );
    assert!(text.contains("0644"), "{text}");
}

// -------------------------------------------------------------- transmit gate

#[test]
fn attestation_under_an_emulator_only_scope_is_refused() {
    let err = mutated(
        "scope = \"emulator-only\"",
        "scope = \"emulator-only\"\ncapture_ref = \"research/diagnostics/x.bin\"",
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("capture_ref"), "{text}");
    assert!(text.contains("emulator-only"), "{text}");
}

#[test]
fn a_real_bus_scope_needs_a_capture_a_hash_and_a_polarity_note_per_link() {
    let hash = "a".repeat(64);

    // Scope alone: refused, naming the first thing missing.
    let err = mutated("scope = \"emulator-only\"", "scope = \"real-bus-attested\"").unwrap_err();
    assert!(
        matches!(
            err,
            ConfigError::GateAttestationMissing {
                field: "capture_ref"
            }
        ),
        "{err}"
    );

    // Everything but the polarity of zone 2.
    let err = mutated(
        "scope = \"emulator-only\"",
        &format!(
            "scope = \"real-bus-attested\"\n\
             capture_ref = \"research/diagnostics/2026-09-01-saturn-zone1.bin\"\n\
             fixtures_sha256 = \"{hash}\"\n\
             [transmit_gate.polarity]\n\
             zone1 = \"A+ = converter TA, measured 2026-09-01\""
        ),
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("zone2"), "{text}");
    assert!(text.contains("reversed"), "{text}");

    // The complete claim. It validates as a *shape*; it is not an authority.
    let c = mutated(
        "scope = \"emulator-only\"",
        &format!(
            "scope = \"real-bus-attested\"\n\
             capture_ref = \"research/diagnostics/2026-09-01-saturn-zone1.bin\"\n\
             fixtures_sha256 = \"{hash}\"\n\
             [transmit_gate.polarity]\n\
             zone1 = \"A+ = converter TA, measured 2026-09-01\"\n\
             zone2 = \"A+ = converter TA, measured 2026-09-01\""
        ),
    )
    .unwrap();
    assert_eq!(c.gate().scope(), GateScope::RealBusAttested);
    assert_eq!(c.gate().fixtures_sha256(), Some(hash.as_str()));
    assert_eq!(c.gate().attested_links().count(), 2);
}

#[test]
fn a_polarity_note_for_a_link_this_configuration_does_not_have_is_refused() {
    let err = mutated(
        "scope = \"emulator-only\"",
        "scope = \"emulator-only\"\n[transmit_gate.polarity]\nzone9 = \"whatever\"",
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::GatePolarityUnknownLink { .. }),
        "{err}"
    );
}

// ------------------------------------------------------------------- bounds

#[test]
fn a_bounds_table_may_narrow() {
    let c = mutated(
        "profile = \"production\"",
        "profile = \"production\"\n\
         [bounds]\n\
         setpoint_ceiling_c = 40.0\n\
         max_session_s = 300\n\
         steam_ceiling_f = 115.0\n\
         steam_max_minutes = 12",
    )
    .unwrap();
    let b = c.bounds();
    assert_eq!(b.setpoint_ceiling(), Cx2::from_raw(80));
    assert_eq!(b.max_session().get(), Duration::from_secs(300));
    assert_eq!(b.steam_ceiling(), Fx2::from_raw(230));
    assert_eq!(b.steam_max_minutes().wire(), 12);
    // And the compiled bounds are still the outer edge.
    assert!(b.setpoint_ceiling() <= ValveSetpoint::CEILING);
    assert!(b.max_session().get() <= SessionDuration::HARD_LIMIT);
    assert!(b.steam_ceiling() <= SteamSetpoint::CEILING);
    assert!(b.steam_max_minutes().wire() <= SteamMinutes::MAX);
}

#[test]
fn a_bounds_table_may_not_widen() {
    for (line, field) in [
        ("setpoint_ceiling_c = 45.0", "bounds.setpoint_ceiling_c"),
        ("max_session_s = 3600", "bounds.max_session_s"),
        // 126 °F, not 130: above the 125 °F ceiling but still representable in
        // Fx2, so this exercises the narrowing rule rather than the encoding
        // range check that sits in front of it.
        ("steam_ceiling_f = 126.0", "bounds.steam_ceiling_f"),
        ("steam_max_minutes = 30", "bounds.steam_max_minutes"),
        ("setpoint_floor_c = 25.0", "bounds.setpoint_floor_c"),
        ("steam_floor_f = 80.0", "bounds.steam_floor_f"),
    ] {
        let err = mutated(
            "profile = \"production\"",
            &format!("profile = \"production\"\n[bounds]\n{line}"),
        )
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains(field), "{text}");
        assert!(text.contains("narrow"), "{text}");
    }
}

#[test]
fn a_temperature_off_the_encoding_step_is_refused() {
    let err = mutated(
        "profile = \"production\"",
        "profile = \"production\"\n[bounds]\nsetpoint_ceiling_c = 40.3",
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(text.contains("bounds.setpoint_ceiling_c"), "{text}");
    assert!(text.contains("0.5 °C"), "{text}");

    // The steam envelope moves in whole degrees Fahrenheit.
    let err = mutated(
        "profile = \"production\"",
        "profile = \"production\"\n[bounds]\nsteam_ceiling_f = 114.5",
    )
    .unwrap_err();
    assert!(err.to_string().contains("1 °F"), "{err}");
}

// -------------------------------------------------------------- misc parsing

/// A misspelled key is a refusal, not a setting quietly ignored. `frames = ture`
/// reading as "frame logging is off" is exactly the failure that leaves you
/// without evidence after the event you needed it for.
#[test]
fn an_unknown_key_is_refused() {
    let err = mutated("frames = true", "framez = true").unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, ConfigError::Syntax { .. }), "{text}");
    assert!(text.contains("framez"), "{text}");
}

#[test]
fn a_missing_file_names_the_path() {
    let err =
        ValidatedConfig::load(Path::new("/nonexistent/kdtvd.toml"), &installed()).unwrap_err();
    assert!(matches!(err, ConfigError::Read { .. }), "{err}");
    assert!(err.to_string().contains("/nonexistent/kdtvd.toml"), "{err}");
}

#[test]
fn a_master_address_that_is_neither_identity_is_refused() {
    let err = mutated("master_address = 0x00", "master_address = 0x03").unwrap_err();
    let text = err.to_string();
    assert!(matches!(err, ConfigError::MasterAddress { .. }), "{text}");
    assert!(text.contains("0x03"), "{text}");
    // The other documented identity is accepted, because the sources disagree
    // and this crate resolves nothing.
    assert!(mutated("master_address = 0x00", "master_address = 0x10").is_ok());
}

#[test]
fn two_sensors_on_one_chip_select_are_refused() {
    let err = mutated("chip_select = \"spi0.1\"", "chip_select = \"spi0.0\"").unwrap_err();
    assert!(
        matches!(err, ConfigError::DuplicateChipSelect { .. }),
        "{err}"
    );
}

#[test]
fn a_commissioned_correction_curve_loads() {
    let c = mutated(
        "chip_select = \"spi0.0\"",
        "chip_select = \"spi0.0\"\n\
         correction = [\n\
         { surface_c = 33.0, immersion_c = 35.0 },\n\
         { surface_c = 42.0, immersion_c = 45.0 },\n\
         ]",
    )
    .unwrap();
    assert!(c.sensor(ZoneId::Zone1).is_characterised());
    assert!(!c.sensor(ZoneId::Zone2).is_characterised());
}

/// The bench profile's whole reason for existing, end to end: session-class
/// durations shrink, wire-class deadlines do not.
#[test]
fn a_bench_scale_shortens_sessions_and_leaves_the_bus_alone() {
    let text = std::fs::read_to_string(EMULATED_TOML).unwrap().replacen(
        "profile = \"bench\"",
        "profile = \"bench\"\n[bench]\nsession_scale = 0.01",
        1,
    );
    let c = ValidatedConfig::from_str_with(&text, Path::new(EMULATED_TOML), &bench_fs()).unwrap();

    assert_eq!(c.scaled_max_session(), Duration::from_secs(12));
    let d = c.scaled_dwells();
    assert_eq!(d.corrected_trip, Duration::from_millis(20));
    assert_eq!(d.divergence, Duration::from_millis(100));
    assert_eq!(d.rtd_starvation, Duration::from_millis(50));

    // Wire class, untouched.
    assert_eq!(c.timing().saturn(), Timings::DOCUMENTED);
    assert_eq!(c.timing().saturn().tick, Duration::from_millis(525));
    assert_eq!(c.timing().dtv().tick, Duration::from_millis(150));
    assert_eq!(c.timing().dtv().reply, Duration::from_millis(300));

    // And unscaled, the session limit is the compiled one.
    let plain = ValidatedConfig::load(Path::new(EMULATED_TOML), &bench_fs()).unwrap();
    assert_eq!(plain.scaled_max_session(), SessionDuration::HARD_LIMIT);
}

#[test]
fn a_scale_above_one_is_refused_even_on_the_bench() {
    let text = std::fs::read_to_string(EMULATED_TOML).unwrap().replacen(
        "profile = \"bench\"",
        "profile = \"bench\"\n[bench]\nsession_scale = 2.0",
        1,
    );
    let err =
        ValidatedConfig::from_str_with(&text, Path::new(EMULATED_TOML), &bench_fs()).unwrap_err();
    assert!(
        matches!(err, ConfigError::SessionScaleOutOfRange { .. }),
        "{err}"
    );
    assert!(err.to_string().contains("narrow"), "{err}");
}
