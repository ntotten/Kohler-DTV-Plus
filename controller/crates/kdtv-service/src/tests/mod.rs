//! The properties this crate exists to hold, driven against the real
//! supervisor.
//!
//! Every test runs on the committed configuration — `deploy/kdtvd.toml`, through
//! the real loader — at the real constants, with `tokio::time::pause()` so a
//! retry train and a shutdown grace cost microseconds. Nothing here reimplements
//! the control loop: the supervisor under test is the one the daemon runs, and
//! the only things replaced are the four platform traits and the byte pipe.
//!
//! What a green run proves, and does not: the valve on the far end is built from
//! the same tier `[C]` documents the encoder is. Agreement between them is
//! internal consistency with the specification, not evidence that the
//! specification matches a Kohler valve.

pub(crate) mod fakes;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kdtv_config::{FsEntry, MapFs, ValidatedConfig};
use kdtv_engine::{OperatorCommand, ZonePhaseKind};
use kdtv_hal::LinkIoError;
use kdtv_proto::fixtures::FixtureSet;
use kdtv_proto::gate::TransmitAuthority;
use kdtv_proto::saturn::{SaturnOpKind, ValveAddr};
use kdtv_safety::{StartAuthorization, ValidatedStart};
use kdtv_telemetry::RequestSource;
use kdtv_units::{
    BootId, CommandId, Cx2, LinkKind, SessionDuration, Slot, SlotSet, ValveSetpoint, ZoneId,
};

use crate::cache::SystemSnapshot;
use crate::command::{CommandError, ServiceHandle};
use crate::port::Pipe;
use crate::service::{Deps, ShutdownTrigger, assemble};
use crate::supervisor::{SHUTDOWN_GRACE, ShutdownOutcome};
use fakes::{FakeClock, FakeIds, FakePipe, FakeWatchdog, PipeScript, PipeWatch, Valve};

const PRODUCTION_TOML: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/kdtvd.toml");
const ZONE1_PORT: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if00-port0";
const ZONE2_PORT: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if01-port0";
const STEAM_PORT: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if02-port0";
const TOKEN: &str = "/run/credentials/kdtvd.service/api-token";

/// Firmware type `0x06`: the six-port valve zone 1 is configured as.
const DTV_6_PORT: u8 = 0x06;
/// Firmware type `0x1E`: the Prompt 3 zone 2 is configured as.
const PROMPT_3_PORT: u8 = 0x1E;

/// Long enough for discovery to probe five addresses at a 320 ms response
/// deadline, walk the eight identity reads and confirm the all-off.
const BOOT_TIME: Duration = Duration::from_millis(3_000);

/// The reference installation's committed configuration, through the real
/// loader. Using the shipped file rather than a fixture means a change to the
/// contract this crate cannot drive is a failing test here too.
fn config() -> ValidatedConfig {
    let fs = MapFs::new()
        .with(ZONE1_PORT, FsEntry::link("/dev/ttyUSB0"))
        .with(ZONE2_PORT, FsEntry::link("/dev/ttyUSB1"))
        .with(STEAM_PORT, FsEntry::link("/dev/ttyUSB2"))
        .with(TOKEN, FsEntry::own(TOKEN).with_mode(0o400));
    let text = std::fs::read_to_string(PRODUCTION_TOML).expect("deploy/kdtvd.toml is missing");
    ValidatedConfig::from_str_with(&text, Path::new(PRODUCTION_TOML), &fs)
        .expect("the committed contract must load")
}

fn slots(numbers: &[u8]) -> SlotSet {
    numbers.iter().filter_map(|n| Slot::new(*n).ok()).collect()
}

fn operator() -> RequestSource {
    RequestSource::Cli {
        peer: "operator".into(),
    }
}

/// One running service, with both valve buses under the test's control.
struct Harness {
    handle: ServiceHandle,
    shutdown: ShutdownTrigger,
    join: tokio::task::JoinHandle<ShutdownOutcome>,
    zone1: PipeScript,
    zone2: PipeScript,
    watch1: PipeWatch,
    watch2: PipeWatch,
    watchdog: Arc<FakeWatchdog>,
}

impl Harness {
    /// Start the service with a healthy valve on each bus.
    fn start() -> Self {
        Self::build()
    }

    fn build() -> Self {
        let config = config();
        let clock = FakeClock::new();
        let watchdog = FakeWatchdog::new(Some(Duration::from_secs(10)));

        let master = config.zone(ZoneId::Zone1).master();
        let address = ValveAddr::new(0x03).expect("0x03 is in range");
        let (zone1, watch1) = PipeScript::new();
        let zone1 = zone1.with_valve(Valve::new(master, address, DTV_6_PORT));
        let (zone2, watch2) = PipeScript::new();
        let zone2 = zone2.with_valve(Valve::new(
            config.zone(ZoneId::Zone2).master(),
            address,
            PROMPT_3_PORT,
        ));

        let pipes: Vec<(LinkKind, Box<dyn Pipe>)> = vec![
            (
                LinkKind::Zone(ZoneId::Zone1),
                Box::new(FakePipe::new(LinkKind::Zone(ZoneId::Zone1), zone1.clone())),
            ),
            (
                LinkKind::Zone(ZoneId::Zone2),
                Box::new(FakePipe::new(LinkKind::Zone(ZoneId::Zone2), zone2.clone())),
            ),
        ];

        let as_clock: Arc<dyn kdtv_hal::Clock> = clock;
        let as_watchdog: Arc<dyn kdtv_hal::Watchdog> = watchdog.clone();

        let started = assemble(
            &config,
            &TransmitAuthority::emulator_only(FixtureSet::embedded()),
            pipes,
            &FakeIds::new(7),
            Deps {
                clock: Arc::clone(&as_clock),
                watchdog: as_watchdog,
            },
            // No link factory ran, so there is no descriptor to record.
            Vec::new(),
        )
        .expect("the committed configuration must assemble");

        Self {
            handle: started.handle,
            shutdown: started.shutdown,
            join: tokio::spawn(started.supervisor.run()),
            zone1,
            zone2,
            watch1,
            watch2,
            watchdog,
        }
    }

    /// Let simulated time pass and the loop run.
    async fn settle(&self, span: Duration) {
        tokio::time::sleep(span).await;
    }

    /// Run the boot sequence to `ReadyOff` on both buses.
    async fn boot(&self) {
        self.settle(BOOT_TIME).await;
        assert_eq!(
            self.phase(ZoneId::Zone1),
            ZonePhaseKind::ReadyOff,
            "zone 1 did not reach ready-off: {:?}",
            self.snapshot()
        );
        assert_eq!(self.phase(ZoneId::Zone2), ZonePhaseKind::ReadyOff);
    }

    fn snapshot(&self) -> Arc<SystemSnapshot> {
        self.handle.snapshot()
    }

    fn phase(&self, zone: ZoneId) -> ZonePhaseKind {
        self.snapshot()
            .zone(zone)
            .map_or(ZonePhaseKind::Cold, |z| z.valve.phase)
    }

    /// Open water on zone 1, slot 1, at the reference setpoint.
    async fn start_zone1(&self, command: CommandId) -> Result<CommandId, CommandError> {
        let request = ValidatedStart {
            zone: ZoneId::Zone1,
            outlets: slots(&[1]),
            temperature: ValveSetpoint::try_new(Cx2::from_raw(76))
                .expect("38 C is inside the clamp"),
            duration: SessionDuration::clamped(Duration::from_secs(300)),
            command,
        };
        self.handle
            .start(
                request,
                StartAuthorization::issue(BootId(7), command),
                operator(),
            )
            .await
    }

    async fn finish(self) -> ShutdownOutcome {
        self.shutdown.trigger("test");
        self.join.await.expect("the supervisor must not panic")
    }
}

/// An all-off and an outlet-open share control byte `0x87`; the bitmap is what
/// tells them apart, and it is `DATA[0]`.
///
/// The frame is `SYNC1 SYNC2 ADDR CONTROL DATA_LEN DATA... CHECKSUM`, so the
/// bitmap is byte five. Written out here rather than assumed: a test that keyed
/// on the control byte alone would have called every all-off an outlet open.
fn is_outlet_write(frame: &[u8]) -> bool {
    frame.get(3) == Some(&SaturnOpKind::AllOff.control_byte())
}

/// A `0x87` frame that opens water.
fn opens_water(frame: &[u8]) -> bool {
    is_outlet_write(frame) && frame.get(5).is_some_and(|bitmap| *bitmap != 0x00)
}

/// A `0x87` frame with an empty bitmap: an all-off.
fn is_all_off(frame: &[u8]) -> bool {
    is_outlet_write(frame) && frame.get(5) == Some(&0x00)
}

// ------------------------------------------------------------------ boot

#[tokio::test(start_paused = true)]
async fn req_design_boot_06_the_boot_sequence_probes_every_address_and_confirms_off_before_ready() {
    let harness = Harness::start();
    harness.boot().await;

    let controls = harness.watch1.controls();
    // Five probes: the valve at 0x03 answers, four addresses are silent, and
    // the scan runs to the end of the range anyway — stopping early is what
    // would hide a second valve.
    let probe = SaturnOpKind::ReadFirmwareType.control_byte();
    let probes = controls
        .iter()
        .fold(0_usize, |count, byte| count + usize::from(*byte == probe));
    assert!(
        probes >= 5,
        "expected five probes, saw {probes}: {controls:02X?}"
    );
    // The boot sequence ends with an all-off whose acknowledgement is required.
    let frames = harness.watch1.frames();
    assert!(
        frames.iter().any(|f| is_all_off(f)),
        "boot must confirm the valve off: {controls:02X?}"
    );
    // Nothing that opens water was ever encodable during boot.
    assert!(
        !frames.iter().any(|f| opens_water(f)),
        "boot must not open an outlet: {frames:02X?}"
    );
    assert!(
        harness.watchdog.is_ready(),
        "the watchdog must be told ready"
    );

    let outcome = harness.finish().await;
    assert!(outcome.is_confirmed(), "{outcome:?}");
}

// ------------------------------------------------------------------ BUS-01

#[tokio::test(start_paused = true)]
async fn only_one_transaction_is_in_flight_on_a_link_at_a_time() {
    let harness = Harness::start();
    harness.boot().await;
    // Discovery deliberately probes four silent addresses, each of which is an
    // unanswered frame; the window that matters is the one where the valve
    // answers everything.
    harness.zone1.clear_outstanding();
    harness.zone2.clear_outstanding();
    // A command while a poll is outstanding is the case that would race: the
    // machine abandons the poll rather than putting a second frame on the bus.
    let _ = harness
        .handle
        .zone(
            ZoneId::Zone1,
            OperatorCommand::Pause {
                command: CommandId(50),
            },
            operator(),
        )
        .await;
    harness.settle(Duration::from_secs(5)).await;

    assert_eq!(
        harness.watch1.max_outstanding(),
        1,
        "two frames were on the bus with no answer between them"
    );
    assert_eq!(harness.watch2.max_outstanding(), 1);
    harness.finish().await;
}

#[tokio::test(start_paused = true)]
async fn steady_polling_runs_at_the_tick_and_not_at_the_housekeeping_interval() {
    let harness = Harness::start();
    harness.boot().await;

    let before = harness.watch1.frame_count();
    let ticks = 10_u32;
    let tick = kdtv_proto::saturn::Timings::DOCUMENTED.tick;
    harness.settle(tick * ticks).await;
    let sent = harness.watch1.frame_count() - before;

    // One frame per tick, give or take where the window falls. If the loop
    // leaked a tick into its 500 ms housekeeping pass this would be about
    // twenty-one.
    assert!(
        (9..=11).contains(&sent),
        "expected about {ticks} frames in {ticks} ticks, sent {sent}"
    );
    harness.finish().await;
}

// ------------------------------------------------------------------ SVC-02

#[tokio::test(start_paused = true)]
async fn a_silent_valve_gets_the_retries_the_budget_allows_and_no_more() {
    let harness = Harness::start();
    harness.boot().await;

    let before = harness.watch1.frame_count();
    harness.zone1.adjust(|valve| valve.silent = true);
    // Four attempts at a 525 ms tick is 2100 ms; ten seconds is far past the
    // point where a fifth would have gone out.
    harness.settle(Duration::from_secs(10)).await;
    let sent = harness.watch1.frame_count() - before;

    // Three retries plus the first send is four, then the all-off the
    // escalation demands. Nothing after that: the port is closed and the zone
    // is latched.
    assert_eq!(
        sent,
        5,
        "expected four attempts and one all-off, sent {sent}: {:02X?}",
        harness.watch1.controls()
    );
    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::Unavailable);
    assert!(harness.watch1.is_closed(), "the port must be closed");

    let outcome = harness.finish().await;
    // A latched zone was never confirmed off, and shutdown says so rather than
    // claiming a clean stop.
    assert_eq!(
        outcome,
        ShutdownOutcome::UnconfirmedOff {
            links: vec![LinkKind::Zone(ZoneId::Zone1)]
        }
    );
}

// ------------------------------------------------------------------ SVC-04

#[tokio::test(start_paused = true)]
async fn a_kernel_effect_reaches_the_wire_as_the_frame_it_names() {
    let harness = Harness::start();
    harness.boot().await;

    let before = harness.watch1.frame_count();
    // The next fault-flag read comes back set. The kernel's answer to a valve
    // fault is AllOff, ClosePort, Latch — in that order.
    harness.zone1.adjust(|valve| valve.faults = 0x0004);
    harness.settle(Duration::from_secs(3)).await;

    let recent: Vec<Vec<u8>> = harness.watch1.frames().into_iter().skip(before).collect();
    assert!(
        recent.iter().any(|f| is_all_off(f)),
        "the AllOff effect must reach the wire: {recent:02X?}"
    );
    assert!(
        harness.watch1.is_closed(),
        "ClosePort must follow the transmission, not replace it"
    );
    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::Unavailable);
    harness.finish().await;
}

// ------------------------------------------------------------------ SVC-01

#[tokio::test(start_paused = true)]
async fn a_zone_one_fault_leaves_zone_twos_machine_and_traffic_untouched() {
    let harness = Harness::start();
    harness.boot().await;

    let zone2_before = harness.watch2.frame_count();
    harness.zone1.adjust(|valve| valve.silent = true);
    harness.settle(Duration::from_secs(6)).await;

    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::Unavailable);
    assert_eq!(
        harness.phase(ZoneId::Zone2),
        ZonePhaseKind::ReadyOff,
        "zone 2 must be untouched"
    );
    assert!(
        !harness.watch2.is_closed(),
        "zone 2's port must stay open when zone 1 faults"
    );
    let snapshot = harness.snapshot();
    let zone2 = snapshot.zone(ZoneId::Zone2).expect("zone 2 is configured");
    assert!(
        !zone2.kernel.is_latched(),
        "the kernel must not latch zone 2: {:?}",
        zone2.kernel
    );
    assert!(
        harness.watch2.frame_count() > zone2_before + 5,
        "zone 2 must still be polling"
    );
    harness.finish().await;
}

#[tokio::test(start_paused = true)]
async fn a_lost_port_on_one_bus_latches_only_that_zone() {
    let harness = Harness::start();
    harness.boot().await;

    let before = harness.watch1.frame_count();
    harness
        .zone1
        .push_read_error(LinkIoError::eof(LinkKind::Zone(ZoneId::Zone1)));
    harness.settle(Duration::from_secs(2)).await;

    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::Unavailable);
    assert_eq!(harness.phase(ZoneId::Zone2), ZonePhaseKind::ReadyOff);

    // The escalation for a lost port is all-off, close, latch — in that order,
    // and the all-off is queued after the failure report that caused it. RS-485
    // is two pairs: a receive failure does not prove the transmit direction has
    // gone, so the stop is attempted rather than discarded. Asserting the phase
    // alone let a version of this pass with nothing on the wire at all.
    let recent: Vec<Vec<u8>> = harness.watch1.frames().into_iter().skip(before).collect();
    assert!(
        recent.iter().any(|f| is_all_off(f)),
        "a lost port must still be told to close its outlets: {recent:02X?}"
    );
    assert!(harness.watch1.is_closed(), "and then the port closes");
    harness.finish().await;
}

// ------------------------------------------------------------------ SAFE-05

// ------------------------------------------------------------------ API-06

#[tokio::test(start_paused = true)]
async fn a_status_read_storm_leaves_the_transmitted_frame_count_unchanged() {
    let harness = Harness::start();
    harness.boot().await;

    let frames_before = harness.watch1.frame_count() + harness.watch2.frame_count();
    let reported_before = harness.snapshot().frames_tx();

    // No await inside the loop, so no simulated time passes and the only thing
    // that could add a frame is the read itself.
    for _ in 0..100_000 {
        let snapshot = harness.handle.snapshot();
        assert_eq!(snapshot.frames_tx(), reported_before);
    }

    let frames_after = harness.watch1.frame_count() + harness.watch2.frame_count();
    assert_eq!(
        frames_before, frames_after,
        "a hundred thousand status reads changed the wire traffic"
    );
    harness.finish().await;
}

// ------------------------------------------------------------------ starts

#[tokio::test(start_paused = true)]
async fn a_start_opens_water_and_a_stop_closes_it() {
    let harness = Harness::start();
    harness.boot().await;

    let accepted = harness
        .start_zone1(CommandId(11))
        .await
        .expect("a healthy ready-off zone must accept a start");
    assert_eq!(accepted, CommandId(11));

    // The setpoint is written first and the first outlet opens one stagger
    // interval later. `[I]` — that this leaves the mixing valve at temperature
    // before flow is the intent of the ordering, not something observed: the
    // frame order is tier `[C]` and what the valve does with a setpoint write
    // in the 500 ms before an outlet opens has never been measured here.
    harness.settle(Duration::from_millis(1_500)).await;
    let frames = harness.watch1.frames();
    let setpoint = frames
        .iter()
        .position(|f| f.get(3) == Some(&SaturnOpKind::SetTemperature.control_byte()));
    let opened = frames.iter().position(|f| opens_water(f));
    assert!(setpoint.is_some(), "a start must write the setpoint");
    assert!(opened.is_some(), "a start must open the outlet");
    assert!(
        setpoint < opened,
        "the setpoint must precede the outlet: {frames:02X?}"
    );
    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::Running);

    harness
        .handle
        .zone(
            ZoneId::Zone1,
            OperatorCommand::Stop {
                command: CommandId(12),
            },
            operator(),
        )
        .await
        .expect("a running zone must accept a stop");
    harness.settle(Duration::from_secs(2)).await;
    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::ReadyOff);
    assert!(harness.snapshot().all_off());
    harness.finish().await;
}

#[tokio::test(start_paused = true)]
async fn a_start_on_a_zone_that_has_not_confirmed_itself_off_is_refused_and_transmits_nothing() {
    let harness = Harness::start();
    // No boot: both zones are still cold.
    harness.settle(Duration::from_millis(10)).await;
    let before = harness.watch1.frame_count();

    let error = harness
        .start_zone1(CommandId(21))
        .await
        .expect_err("a cold zone must refuse");
    assert!(matches!(error, CommandError::Denied(_)), "{error:?}");
    assert_eq!(
        harness.watch1.frame_count(),
        before,
        "a refusal transmits nothing"
    );
    harness.finish().await;
}

#[tokio::test(start_paused = true)]
async fn an_authorisation_from_another_boot_cannot_open_water() {
    let harness = Harness::start();
    harness.boot().await;

    let request = ValidatedStart {
        zone: ZoneId::Zone1,
        outlets: slots(&[1]),
        temperature: ValveSetpoint::try_new(Cx2::from_raw(76)).expect("inside the clamp"),
        duration: SessionDuration::clamped(Duration::from_secs(300)),
        command: CommandId(31),
    };
    let error = harness
        .handle
        .start(
            request,
            // The service booted as 7; this token says 6.
            StartAuthorization::issue(BootId(6), CommandId(31)),
            operator(),
        )
        .await
        .expect_err("a stale boot id must refuse");
    assert!(matches!(error, CommandError::Denied(_)), "{error:?}");
    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::ReadyOff);
    harness.finish().await;
}

#[tokio::test(start_paused = true)]
async fn a_command_storm_cannot_set_the_rate_on_the_bus() {
    let harness = Harness::start();
    harness.boot().await;
    harness
        .start_zone1(CommandId(600))
        .await
        .expect("a start must be accepted");
    harness.settle(Duration::from_millis(1_500)).await;
    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::Running);
    let before = harness.watch1.frame_count();

    // Nothing between them lets simulated time pass, so every one of these
    // arrives inside the response deadline of the one before. Each is a command
    // the machine accepts, and the machine abandons whatever transaction was
    // outstanding and sends in its place — so without a floor, the caller's rate
    // is the bus's rate.
    let mut accepted = 0_usize;
    let mut paced = 0_usize;
    for n in 0..64_u64 {
        match harness
            .handle
            .zone(
                ZoneId::Zone1,
                OperatorCommand::SetTemperature {
                    temp: ValveSetpoint::try_new(Cx2::from_raw(76)).expect("inside the clamp"),
                    command: CommandId(601 + n),
                },
                operator(),
            )
            .await
        {
            Ok(_) => accepted += 1,
            Err(CommandError::TooSoon { .. }) => paced += 1,
            Err(other) => panic!("unexpected refusal {other:?}"),
        }
    }
    assert!(paced > 0, "nothing paced the commands; {accepted} accepted");
    let sent = harness.watch1.frame_count() - before;
    assert!(
        sent < 8,
        "a command storm put {sent} frames on a bus specified for about two a second"
    );
    harness.finish().await;
}

#[tokio::test(start_paused = true)]
async fn a_stop_is_never_paced_out_of_the_way() {
    let harness = Harness::start();
    harness.boot().await;
    harness
        .start_zone1(CommandId(81))
        .await
        .expect("a start must be accepted");
    harness.settle(Duration::from_millis(1_500)).await;
    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::Running);

    // Immediately after the start's own frames, with no time passing: the floor
    // that refuses a second setpoint must not refuse the stop.
    harness
        .handle
        .zone(
            ZoneId::Zone1,
            OperatorCommand::Stop {
                command: CommandId(82),
            },
            operator(),
        )
        .await
        .expect("closing a valve is never made to wait");
    harness.settle(Duration::from_secs(2)).await;
    assert!(harness.snapshot().all_off());
    harness.finish().await;
}

#[tokio::test(start_paused = true)]
async fn stop_all_reports_ok_only_because_a_link_actually_took_it() {
    let harness = Harness::start();
    harness.boot().await;

    // Zone 1's valve goes quiet, so zone 1 spends its retry budget and latches:
    // its port is closed and its machine refuses a stop, because there is
    // nothing left to send one on. Zone 2 is healthy and takes it.
    harness.zone1.adjust(|valve| valve.silent = true);
    harness.settle(Duration::from_secs(6)).await;
    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::Unavailable);
    assert_eq!(harness.phase(ZoneId::Zone2), ZonePhaseKind::ReadyOff);

    let before = harness.watch2.frame_count();
    harness
        .handle
        .stop_all(CommandId(91), operator())
        .await
        .expect("one link refusing is not the shower failing to stop");
    harness.settle(Duration::from_millis(600)).await;

    // The `Ok` is only correct because something was sent. Under the old
    // unconditional `Ok`, "commanded off" and "nothing left the service" were
    // the same answer.
    let recent: Vec<Vec<u8>> = harness.watch2.frames().into_iter().skip(before).collect();
    assert!(
        recent.iter().any(|f| is_all_off(f)),
        "the stop must reach the link that accepted it: {recent:02X?}"
    );
    harness.finish().await;
}

// ------------------------------------------------------------------ shutdown

#[tokio::test(start_paused = true)]
async fn shutdown_closes_the_outlets_before_it_exits() {
    let harness = Harness::start();
    harness.boot().await;
    harness
        .start_zone1(CommandId(41))
        .await
        .expect("a start must be accepted");
    harness.settle(Duration::from_millis(1_500)).await;
    assert_eq!(harness.phase(ZoneId::Zone1), ZonePhaseKind::Running);

    let watch = harness.watch1.clone();
    let outcome = harness.finish().await;

    assert_eq!(outcome, ShutdownOutcome::ConfirmedOff, "{outcome:?}");
    let frames = watch.frames();
    let last_open = frames.iter().rposition(|f| opens_water(f));
    let last_off = frames.iter().rposition(|f| is_all_off(f));
    assert!(last_open.is_some(), "the test must have opened an outlet");
    assert!(
        last_off > last_open,
        "the last thing on the wire must be an all-off: {frames:02X?}"
    );
    assert!(watch.is_closed(), "the port must be closed on the way out");
}

#[tokio::test(start_paused = true)]
async fn a_valve_that_never_confirms_is_reported_rather_than_called_a_clean_stop() {
    let harness = Harness::start();
    harness.boot().await;
    harness
        .start_zone1(CommandId(51))
        .await
        .expect("a start must be accepted");
    harness.settle(Duration::from_millis(1_500)).await;

    // Both valves go quiet, so neither the stop nor its retries are ever
    // acknowledged. Zone 1 is running, so it also has water to lose.
    harness.zone1.adjust(|valve| valve.silent = true);
    harness.zone2.adjust(|valve| valve.silent = true);
    let watch = harness.watch1.clone();
    let outcome = harness.finish().await;

    match outcome {
        ShutdownOutcome::UnconfirmedOff { links } => {
            assert!(
                links.contains(&LinkKind::Zone(ZoneId::Zone1)),
                "zone 1 must be named: {links:?}"
            );
        }
        ShutdownOutcome::ConfirmedOff => panic!("a silent valve must not read as confirmed off"),
    }
    // The stop still went out, repeatedly, and the port still closed.
    assert!(watch.frames().iter().any(|f| is_all_off(f)));
    assert!(watch.is_closed());
}

#[tokio::test(start_paused = true)]
async fn a_stop_during_the_boot_sequence_is_unconfirmed_and_says_which_kind_of_unconfirmed() {
    let harness = Harness::start();
    // Silent valves, and a shutdown while discovery is still probing. Nothing
    // has acknowledged an all-off, so this service has no confirmation the
    // valve is closed — a watchdog reset mid-session looks exactly like this,
    // and reporting it as a clean stop would be a claim about a valve nobody
    // has spoken to.
    harness.zone1.adjust(|valve| valve.silent = true);
    harness.zone2.adjust(|valve| valve.silent = true);
    harness.settle(Duration::from_millis(10)).await;
    assert_ne!(harness.phase(ZoneId::Zone1), ZonePhaseKind::ReadyOff);

    let mut events = harness.handle.subscribe();
    let outcome = harness.finish().await;
    assert!(!outcome.is_confirmed(), "{outcome:?}");

    // ...and the message distinguishes it from a valve that went quiet with
    // water on, which is the case the same words used to cover.
    let mut said_why = false;
    loop {
        match events.try_recv() {
            Ok(event) => {
                let json = serde_json::to_string(&event).expect("every event must serialise");
                said_why |= json.contains("had not finished the boot sequence");
            }
            // A lag is a gap in the stream, not the end of it.
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => {}
            Err(_) => break,
        }
    }
    assert!(
        said_why,
        "an unconfirmed stop during boot must say that is what it was"
    );
}

#[tokio::test(start_paused = true)]
async fn a_command_arriving_during_shutdown_is_answered_rather_than_dropped() {
    let harness = Harness::start();
    harness.boot().await;

    // Silent valves keep the drain open long enough for a command to arrive
    // while the confirmations are outstanding.
    harness.zone1.adjust(|valve| valve.silent = true);
    harness.zone2.adjust(|valve| valve.silent = true);
    harness.shutdown.trigger("test");
    harness.settle(Duration::from_millis(50)).await;

    let error = harness
        .handle
        .zone(
            ZoneId::Zone1,
            OperatorCommand::Pause {
                command: CommandId(61),
            },
            operator(),
        )
        .await
        .expect_err("a shutting-down service accepts nothing new");
    assert_eq!(error, CommandError::ShuttingDown);

    let outcome = harness.join.await.expect("the supervisor must not panic");
    assert!(!outcome.is_confirmed(), "{outcome:?}");
}

#[tokio::test(start_paused = true)]
async fn the_shutdown_grace_is_bounded() {
    // Not a behaviour test — a statement about the number, so a change to it is
    // deliberate. It has to exceed a full Saturn transaction budget and stay
    // well inside what systemd waits for on a stop.
    let budget = kdtv_engine::RetryBudget::from_saturn(&kdtv_proto::saturn::Timings::DOCUMENTED);
    assert!(SHUTDOWN_GRACE > budget.ceiling);
    assert!(SHUTDOWN_GRACE < Duration::from_secs(30));
}

// ------------------------------------------------------------------ watchdog

#[tokio::test(start_paused = true)]
async fn the_watchdog_is_petted_by_the_loop_that_services_the_links() {
    let harness = Harness::start();
    harness.boot().await;
    let pets = harness.watchdog.pets();
    assert!(pets > 0, "the loop must pet the watchdog");

    // Half of a ten-second interval is five seconds, so twelve seconds is two
    // more pets and not two hundred.
    harness.settle(Duration::from_secs(12)).await;
    let later = harness.watchdog.pets();
    assert!(
        (pets + 1..=pets + 4).contains(&later),
        "expected a pet every five seconds, went from {pets} to {later}"
    );
    harness.finish().await;
}

// ------------------------------------------------------------------ logging

#[tokio::test(start_paused = true)]
async fn req_design_log_05_req_design_log_02_the_event_stream_carries_the_frames_and_the_stamps_the_log_requires()
 {
    let harness = Harness::start();
    let mut events = harness.handle.subscribe();
    harness.boot().await;

    let mut saw_frame = false;
    let mut saw_state = false;
    let mut saw_serial_opened = false;
    loop {
        let event = match events.try_recv() {
            Ok(event) => event,
            // A boot produces far more than the channel holds, so a subscriber
            // that reads at the end has lagged. Skipping the gap rather than
            // stopping at it is what makes the assertions below mean anything:
            // a `break` here would have passed on an empty stream.
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        };
        let json = serde_json::to_string(&event).expect("every event must serialise");
        if json.contains("\"event\":\"frame\"") {
            saw_frame = true;
            // LOG-05 wants both timestamps; LOG-02 wants the sync state with
            // the wall one, and the type will not let them be separated.
            assert!(json.contains("monotonic_ns"), "{json}");
            assert!(json.contains("wall_unix_s"), "{json}");
            assert!(json.contains("\"ntp\""), "{json}");
        }
        saw_serial_opened |= json.contains("serial_opened");
        saw_state |= json.starts_with("{\"state\":");
        // LOG-09: nothing credential-shaped ever reaches the stream.
        for word in ["token", "secret", "password", "credential"] {
            assert!(!json.contains(word), "{word} appeared in {json}");
        }
    }
    // `LOG-09` over a snapshot that actually has something in it. The check in
    // `crate::cache` serialises an empty one, which can only ever exercise the
    // envelope; this one carries both zones, their engine caches and their
    // kernel labels. Steam is `enabled = false` in the reference
    // configuration, so `SteamStatus` is not covered here either.
    let populated = harness.snapshot();
    let json = serde_json::to_string(&*populated).expect("a snapshot must serialise");
    assert!(json.contains("\"zone\":\"zone1\""), "{json}");
    for word in ["token", "secret", "password", "credential", "pairing"] {
        assert!(!json.contains(word), "{word} appears in {json}");
    }

    assert!(saw_frame, "raw frame bytes must be recorded");
    assert!(saw_state, "the state stream must publish snapshots");
    // Nothing was opened through a link factory in this harness, so there is no
    // descriptor to record and no such event.
    assert!(!saw_serial_opened);
    harness.finish().await;
}
