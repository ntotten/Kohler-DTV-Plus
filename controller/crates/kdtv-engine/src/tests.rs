//! The fault matrix, the boot sequence, and the session limit.
//!
//! Every test here runs at the **real** constants — a 525 ms tick, a 320 ms
//! response deadline, a 500 ms outlet stagger, a 1200 s session limit — and the
//! whole file runs in microseconds, because the machines take time as a
//! parameter. The twenty-two-minute run is twenty-two simulated minutes.
//!
//! Two things every single-link failure test asserts, and they are the point:
//!
//! 1. the faulted zone ends with **nothing commanded open** and latched, and
//! 2. the **other** zone is untouched — same phase, same kernel state, and still
//!    polling.
//!
//! The second is the scoping check. A fault matrix that only looks at the zone
//! that broke passes just as happily when a bad cable in one bathroom stops the
//! shower in the other.
//!
//! The frames are built as bytes and put through the real decoders, not
//! constructed as decoded values. A test that hand-builds a `DecodedFrame`
//! proves the engine agrees with the test author; one that builds bytes proves
//! the engine agrees with `kdtv-proto`.

use super::*;
use crate::zone::{OperatorCommand, Purge, StartRequest, Step, ZoneEvent, ZoneMachine, ZonePhase};
use kdtv_config::{FsEntry, MapFs, ValidatedConfig, ZoneConfig};
use kdtv_proto::dtv::{
    self, DecodedDtv, DevAddr, DeviceId, DtvTimings, SteamErrorFlags, SteamOp, SteamOpState,
    SteamStatus,
};
use kdtv_proto::saturn::{
    self, DecodeError, DecodedFrame, Expectation, MasterAddr, RxBuffer, SYNC1, SYNC2, SaturnOp,
    SaturnOpKind, ValveAddr, opcode,
};
use kdtv_safety::{
    Bounds, DegradeReason, Effect, LinkState, OpenGrant, SafetyEvent, SafetyKernel,
    StartAuthorization, ValidatedStart,
};
use kdtv_telemetry::Monotonic;
use kdtv_units::{
    BootId, CommandId, Cx2, Fx2, LinkKind, SessionDuration, Slot, SlotSet, SteamMinutes,
    SteamSetpoint, ValveSetpoint, ZoneId,
};
use smallvec::SmallVec;
use std::path::Path;
use std::time::Duration;

const PRODUCTION_TOML: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/kdtvd.toml");
const ZONE1_PORT: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if00-port0";
const ZONE2_PORT: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if01-port0";
const STEAM_PORT: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if02-port0";
const TOKEN: &str = "/run/credentials/kdtvd.service/api-token";

/// The reference installation's committed configuration, through the real
/// loader. Using the shipped file rather than a fixture means a change to the
/// contract that this crate cannot drive is a failing test here too.
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

fn ms(n: u64) -> Monotonic {
    Monotonic::from_nanos(n.saturating_mul(1_000_000))
}

fn slots(ns: &[u8]) -> SlotSet {
    ns.iter().filter_map(|n| Slot::new(*n).ok()).collect()
}

/// A kernel whose configured outlets are **wider** than either valve's table:
/// slots 1..=6 on both zones. That is deliberate. It lets a request reach the
/// engine's own outlet check with a grant already in hand, which is the only way
/// to prove the engine refuses an unconfigured outlet rather than relying on the
/// kernel having done it.
fn kernel() -> SafetyKernel {
    SafetyKernel::new(
        BootId(1),
        Bounds {
            session_cap: SessionDuration::clamped(SessionDuration::HARD_LIMIT),
            configured_outlets: [
                (ZoneId::Zone1, slots(&[1, 2, 3, 4, 5, 6])),
                (ZoneId::Zone2, slots(&[1, 2, 3, 4, 5, 6])),
            ],
        },
    )
}

// ------------------------------------------------------------------ wire

/// A valve-to-master frame, as bytes on the wire.
fn wire(master: MasterAddr, control: u8, data: &[u8]) -> Vec<u8> {
    let addr = master.byte();
    let len = u8::try_from(data.len()).unwrap();
    let mut v = vec![SYNC1, SYNC2, addr, control, len];
    v.extend_from_slice(data);
    v.push(saturn::checksum(addr, control, len, data));
    v
}

/// The reply payload a healthy valve gives to each read, at the exact length the
/// decoder's response-length table demands.
fn payload(kind: SaturnOpKind, firmware: u8) -> Vec<u8> {
    match kind {
        SaturnOpKind::ReadFirmwareType => vec![firmware],
        SaturnOpKind::ReadFirmwareVersion => vec![0x00, 0x0C, 0x00],
        SaturnOpKind::ReadSerialNumber => vec![1, 2, 3, 4, 5, 6],
        SaturnOpKind::ReadCalibration => vec![173, 0, 0, 0, 0, 0, 0, 0],
        SaturnOpKind::ReadConfiguration => vec![0, 0, 0, 0, 0, 0],
        // Outlets clear, temperature 38.0 C, faults clear.
        SaturnOpKind::ReadOutlets | SaturnOpKind::ReadFaults => vec![0x00, 0x00],
        SaturnOpKind::ReadTemperature => vec![76, 0],
        _ => vec![],
    }
}

/// A healthy echo of `op`.
fn echo(master: MasterAddr, op: SaturnOpKind, firmware: u8) -> DecodedFrame {
    let bytes = wire(master, op.control_byte(), &payload(op, firmware));
    let mut rx = RxBuffer::new();
    rx.extend(&bytes);
    saturn::decode(&mut rx, &Expectation::response_to(master, op))
        .expect("a healthy reply must decode")
        .expect("a complete frame must decode")
}

// ------------------------------------------------------------------ rig

/// One zone, its kernel, and a simulated valve that answers at one address.
struct Rig {
    cfg: ValidatedConfig,
    kernel: SafetyKernel,
    zone: ZoneMachine,
    other: ZoneMachine,
    now: Monotonic,
    /// Where the valve answers. Every other address in `0x03..=0x07` is silent.
    valve_at: ValveAddr,
    /// A second valve on the same bus, for the address-conflict case.
    second: Option<ValveAddr>,
    firmware: u8,
    /// Every operation the zone machine asked to transmit, in order.
    sent: Vec<SaturnOp>,
    /// Every effect the zone machine returned, in order.
    effects: Vec<Effect>,
}

impl Rig {
    fn new() -> Self {
        Self::with(ZoneSettings::default())
    }

    fn with(settings: ZoneSettings) -> Self {
        let cfg = config();
        let zone = ZoneMachine::new(cfg.zone(ZoneId::Zone1), settings);
        let other = ZoneMachine::new(cfg.zone(ZoneId::Zone2), ZoneSettings::default());
        Self {
            cfg,
            kernel: kernel(),
            zone,
            other,
            now: ms(0),
            valve_at: ValveAddr::new(0x03).unwrap(),
            second: None,
            firmware: 0x06, // DTV 6-Port, which is what zone 1 is configured as.
            sent: Vec::new(),
            effects: Vec::new(),
        }
    }

    fn zone1(&self) -> &ZoneConfig {
        self.cfg.zone(ZoneId::Zone1)
    }

    fn master(&self) -> MasterAddr {
        self.zone1().master()
    }

    fn advance(&mut self, d: Duration) {
        self.now = self.now.checked_add(d).unwrap();
    }

    fn record(&mut self, step: &Step) {
        if let Some(op) = step.tx.clone() {
            self.sent.push(op);
        }
        self.effects.extend(step.effects.iter().cloned());
    }

    fn feed(&mut self, ev: ZoneEvent) -> Step {
        let step = self.zone.step(ev, self.now, &mut self.kernel);
        self.record(&step);
        step
    }

    fn tick(&mut self) -> Step {
        self.feed(ZoneEvent::Tick)
    }

    fn answers(&self, target: ValveAddr) -> bool {
        target == self.valve_at || Some(target) == self.second
    }

    /// Answer the step's transmission the way a healthy valve would, or let it
    /// time out if nothing is at that address.
    fn follow(&mut self, step: &Step) -> Step {
        self.advance(Duration::from_millis(20));
        let Some(op) = step.tx.clone() else {
            self.advance(Duration::from_millis(505));
            return self.tick();
        };
        let target = step.target.expect("a transmission names an address");
        if self.answers(target) {
            let frame = echo(self.master(), op.kind(), self.firmware);
            self.feed(ZoneEvent::Response(frame))
        } else {
            self.advance(Duration::from_millis(300));
            self.feed(ZoneEvent::ResponseTimeout)
        }
    }

    /// Run the boot sequence until the valve has confirmed off, or panic saying
    /// where it stopped.
    ///
    /// Both `ReadyOff` and `Purging` are finished boots. The boot all-off is a
    /// real all-off: a service that restarts mid-shower finds the valve running
    /// and stops it, so with purge enabled the window opens here exactly as it
    /// does after a session. Waiting for `ReadyOff` specifically would run the
    /// purge out before the caller ever saw it.
    fn boot(&mut self) {
        let mut step = self.tick();
        for _ in 0..80 {
            if matches!(
                self.zone.phase(),
                ZonePhase::ReadyOff | ZonePhase::Purging { .. }
            ) {
                return;
            }
            if matches!(self.zone.phase(), ZonePhase::Unavailable { .. }) {
                panic!("boot latched: {:?}", self.zone.phase());
            }
            step = self.follow(&step);
        }
        panic!("boot did not finish; stopped at {:?}", self.zone.phase());
    }

    /// Boot the second zone too, so the scoping assertions have something real
    /// to compare against.
    fn boot_other(&mut self) {
        let master = self.cfg.zone(ZoneId::Zone2).master();
        let mut step = self.other.step(ZoneEvent::Tick, self.now, &mut self.kernel);
        for _ in 0..80 {
            if matches!(self.other.phase(), ZonePhase::ReadyOff) {
                return;
            }
            let ev = match step.tx.clone() {
                None => ZoneEvent::Tick,
                Some(op) => {
                    let target = step.target.unwrap();
                    if target == ValveAddr::new(0x03).unwrap() {
                        // 0x1E is the Prompt 3-Port firmware byte, which is what
                        // zone 2 is configured as.
                        ZoneEvent::Response(echo(master, op.kind(), 0x1E))
                    } else {
                        ZoneEvent::ResponseTimeout
                    }
                }
            };
            step = self.other.step(ev, self.now, &mut self.kernel);
        }
        panic!(
            "zone 2 boot did not finish; stopped at {:?}",
            self.other.phase()
        );
    }

    /// A grant for this zone, minted the only way there is.
    fn grant(&mut self, outlets: SlotSet, command: CommandId) -> OpenGrant {
        let req = ValidatedStart {
            zone: ZoneId::Zone1,
            outlets,
            temperature: ValveSetpoint::try_new(Cx2::from_raw(76)).unwrap(),
            duration: SessionDuration::clamped(Duration::from_secs(600)),
            command,
        };
        self.kernel
            .authorize_open(&req, StartAuthorization::issue(BootId(1), command))
            .expect("the kernel must authorise a ready zone")
    }

    fn start(&mut self, outlets: SlotSet, duration: Duration) -> Step {
        let command = CommandId(7);
        let grant = self.grant(outlets, command);
        self.feed(ZoneEvent::Start(
            StartRequest {
                outlets,
                temperature: ValveSetpoint::try_new(Cx2::from_raw(76)).unwrap(),
                duration: SessionDuration::clamped(duration),
                command,
            },
            grant,
        ))
    }

    /// Withhold every response until the machine gives up.
    fn starve(&mut self) {
        for _ in 0..24 {
            if matches!(self.zone.phase(), ZonePhase::Unavailable { .. }) {
                return;
            }
            self.advance(Duration::from_millis(320));
            self.feed(ZoneEvent::ResponseTimeout);
            self.advance(Duration::from_millis(205));
            self.tick();
        }
        panic!(
            "the machine never gave up; it is at {:?}",
            self.zone.phase()
        );
    }

    // ---- assertions --------------------------------------------------

    fn assert_off_and_latched(&self) {
        assert!(
            matches!(self.zone.phase(), ZonePhase::Unavailable { .. }),
            "expected the zone latched, found {:?}",
            self.zone.phase()
        );
        assert!(
            !self.zone.cached().valve_on,
            "the zone must end with nothing commanded open"
        );
        assert!(!self.zone.cached().water_moving);
        assert!(matches!(
            self.kernel.state(LinkKind::Zone(ZoneId::Zone1)),
            LinkState::Latched { .. }
        ));
        // Best-effort all-off is attempted on the way down, every time.
        assert!(
            self.sent.contains(&SaturnOp::AllOff),
            "an all-off must have been attempted"
        );
        assert!(
            self.effects
                .iter()
                .any(|e| matches!(e, Effect::ClosePort(LinkKind::Zone(ZoneId::Zone1)))),
            "the port must be closed"
        );
    }

    /// **The scoping check.** Zone 2 saw nothing, is where it was, and still
    /// polls.
    fn assert_other_zone_untouched(&mut self) {
        assert!(
            matches!(self.other.phase(), ZonePhase::ReadyOff),
            "zone 2 must be untouched, found {:?}",
            self.other.phase()
        );
        assert_eq!(
            *self.kernel.state(LinkKind::Zone(ZoneId::Zone2)),
            LinkState::Ready,
            "zone 2's kernel state must be untouched"
        );
        let step = self.other.step(ZoneEvent::Tick, self.now, &mut self.kernel);
        assert!(step.tx.is_some(), "zone 2 must still be polling");
        assert!(step.effects.is_empty(), "zone 2 must have nothing to do");
    }
}

// ------------------------------------------------------------ boot sequence

#[test]
fn req_controller_design_boot_01_req_controller_design_boot_05_the_boot_sequence_reaches_ready_off_without_transmitting_a_start()
 {
    let mut r = Rig::new();
    r.boot();

    assert!(matches!(r.zone.phase(), ZonePhase::ReadyOff));
    assert_eq!(
        *r.kernel.state(LinkKind::Zone(ZoneId::Zone1)),
        LinkState::Ready
    );
    assert_eq!(r.zone.address(), Some(ValveAddr::new(0x03).unwrap()));

    // BOOT-01: nothing that opens water was transmitted.
    assert!(
        !r.sent
            .iter()
            .any(|op| op.kind().can_open_water() || op.kind() == SaturnOpKind::SetTemperature),
        "the boot sequence transmitted a write that moves water: {:?}",
        r.sent
    );
    // Discovery is read-only: the installed addressing is left as it is.
    assert!(
        !r.sent.iter().any(|op| op.kind().is_address_management()),
        "the boot sequence re-addressed the bus: {:?}",
        r.sent
    );
    // BOOT-05: the all-off went out and was acknowledged, exactly once.
    assert_eq!(
        r.sent.iter().filter(|op| **op == SaturnOp::AllOff).count(),
        1
    );
    // BOOT-04, plus the two PH0-01 reads.
    for kind in [
        SaturnOpKind::ReadFirmwareType,
        SaturnOpKind::ReadFirmwareVersion,
        SaturnOpKind::ReadSerialNumber,
        SaturnOpKind::ReadCalibration,
        SaturnOpKind::ReadConfiguration,
        SaturnOpKind::ReadOutlets,
        SaturnOpKind::ReadTemperature,
        SaturnOpKind::ReadFaults,
    ] {
        assert!(
            r.sent.iter().any(|op| op.kind() == kind),
            "the boot sequence never read {kind:?}"
        );
    }
    let cache = r.zone.cached();
    assert_eq!(cache.health, Health::Clear);
    assert_eq!(cache.firmware_type, Some(0x06));
    assert!(cache.calibration.is_some());
    assert!(!cache.valve_on);
}

#[test]
fn discovery_probes_every_address_in_the_range_even_after_one_answers() {
    let mut r = Rig::new();
    r.boot();
    // Five probes, one per address, and the valve at 0x03 did not stop the scan.
    let probes = r
        .sent
        .iter()
        .filter(|op| op.kind() == SaturnOpKind::ReadFirmwareType)
        .count();
    // Five probes plus the identity read that follows discovery.
    assert_eq!(probes, 6, "{:?}", r.sent);
}

#[test]
fn req_controller_design_boot_08_a_valve_that_never_acknowledges_all_off_leaves_the_zone_unavailable()
 {
    let mut r = Rig::new();
    r.boot_other();

    // Answer everything up to the all-off, then answer nothing.
    let mut step = r.tick();
    for _ in 0..40 {
        if step.tx.as_ref().is_some_and(|op| *op == SaturnOp::AllOff) {
            break;
        }
        step = r.follow(&step);
    }
    assert!(matches!(r.zone.phase(), ZonePhase::ConfirmOff));
    r.starve();

    r.assert_off_and_latched();
    // BOOT-08: the operator is told what to do, in words.
    let message = r
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::OperatorMessage { text, .. } => Some(*text),
            _ => None,
        })
        .expect("an unconfirmable valve must produce an operator message");
    assert!(
        message.contains("Remove valve power"),
        "the message must say what to do: {message}"
    );
    r.assert_other_zone_untouched();
}

#[test]
fn discovery_refuses_a_bus_where_two_valves_answer() {
    let mut r = Rig::new();
    r.boot_other();
    r.second = Some(ValveAddr::new(0x05).unwrap());

    let mut step = r.tick();
    for _ in 0..24 {
        if matches!(r.zone.phase(), ZonePhase::Unavailable { .. }) {
            break;
        }
        step = r.follow(&step);
    }

    assert!(
        matches!(r.zone.phase(), ZonePhase::Unavailable { .. }),
        "two valves on one bus must refuse, found {:?}",
        r.zone.phase()
    );
    // It never got as far as an identity read, let alone a write.
    assert!(
        !r.sent
            .iter()
            .any(|op| op.kind().is_write() && *op != SaturnOp::AllOff)
    );
    let message = r
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::OperatorMessage { text, .. } => Some(*text),
            _ => None,
        })
        .expect("a mis-cabled bus must produce an operator message");
    assert!(message.contains("More than one valve"), "{message}");
    r.assert_other_zone_untouched();
}

#[test]
fn a_bus_where_nothing_answers_refuses_rather_than_carrying_on() {
    let mut r = Rig::new();
    r.boot_other();
    // A silent bus: every probe times out, and the scan runs the whole range
    // before concluding anything.
    let mut step = r.tick();
    for _ in 0..24 {
        if matches!(r.zone.phase(), ZonePhase::Unavailable { .. }) {
            break;
        }
        step = if step.tx.is_some() {
            r.advance(Duration::from_millis(320));
            r.feed(ZoneEvent::ResponseTimeout)
        } else {
            r.advance(Duration::from_millis(205));
            r.tick()
        };
    }
    assert!(
        matches!(r.zone.phase(), ZonePhase::Unavailable { .. }),
        "a bus with no valve must refuse, found {:?}",
        r.zone.phase()
    );
    // Five probes: the scan visited every address in 0x03..=0x07 before
    // concluding the bus is empty.
    assert_eq!(
        r.sent
            .iter()
            .filter(|op| op.kind() == SaturnOpKind::ReadFirmwareType)
            .count(),
        5
    );
    let message = r
        .effects
        .iter()
        .find_map(|e| match e {
            Effect::OperatorMessage { text, .. } => Some(*text),
            _ => None,
        })
        .expect("a silent bus must produce an operator message");
    assert!(message.contains("No valve answered"), "{message}");
    r.assert_other_zone_untouched();
}

// --------------------------------------------------------------- starting

#[test]
fn a_start_is_refused_when_the_zone_is_cold() {
    let mut r = Rig::new();
    // The kernel is told the zone is ready — as the service would after a boot
    // — but this machine has confirmed nothing itself. Defence in depth: a
    // grant is not enough.
    r.kernel.mark_ready(LinkKind::Zone(ZoneId::Zone1));
    let step = r.start(slots(&[1]), Duration::from_secs(600));

    assert!(
        matches!(step.refused, Some(Refusal::NotReadyOff { .. })),
        "{:?}",
        step.refused
    );
    assert!(step.tx.is_none(), "a refusal transmits nothing");
    assert!(matches!(r.zone.phase(), ZonePhase::Cold));
}

#[test]
fn a_start_is_refused_when_the_zone_is_latched() {
    let mut r = Rig::new();
    r.boot();
    let command = CommandId(7);
    let grant = r.grant(slots(&[1]), command);

    // Latch the zone after the grant was minted, which is the race that matters:
    // the authorisation is in hand and the zone has gone.
    r.feed(ZoneEvent::PortClosed);
    assert!(matches!(r.zone.phase(), ZonePhase::Unavailable { .. }));

    let step = r.feed(ZoneEvent::Start(
        StartRequest {
            outlets: slots(&[1]),
            temperature: ValveSetpoint::try_new(Cx2::from_raw(76)).unwrap(),
            duration: SessionDuration::clamped(Duration::from_secs(600)),
            command,
        },
        grant,
    ));
    assert!(matches!(
        step.refused,
        Some(Refusal::NotReadyOff {
            phase: ZonePhaseKind::Unavailable
        })
    ));
    assert!(step.tx.is_none());
}

#[test]
fn a_start_naming_an_unconfigured_outlet_is_refused_by_the_engine_too() {
    let mut r = Rig::new();
    r.boot();
    // Slot 6 is inside the kernel's (deliberately wide) bounds and outside this
    // valve's outlet table. The grant is real; the engine still refuses.
    let step = r.start(slots(&[1, 6]), Duration::from_secs(600));
    assert!(
        matches!(step.refused, Some(Refusal::UnconfiguredOutlet(s)) if s.get() == 6),
        "{:?}",
        step.refused
    );
    assert!(step.tx.is_none());
    assert!(matches!(r.zone.phase(), ZonePhase::ReadyOff));
}

#[test]
fn a_start_is_refused_when_the_valve_health_was_never_established() {
    // A booted zone has read its fault flags clear, and starts.
    let mut booted = Rig::new();
    booted.boot();
    assert_eq!(booted.zone.cached().health, Health::Clear);
    assert!(
        booted
            .start(slots(&[1]), Duration::from_secs(600))
            .refused
            .is_none()
    );

    // A zone the kernel believes is ready but which has read nothing has no
    // positive statement of health to rest on. There is no route to
    // `Health::Clear` from an error byte, because the two error tables disagree
    // about what a zero means. CORRECTIONS.md item 4.
    let mut cold = Rig::new();
    cold.kernel.mark_ready(LinkKind::Zone(ZoneId::Zone1));
    let step = cold.start(slots(&[1]), Duration::from_secs(600));
    assert!(step.refused.is_some());
    assert_eq!(cold.zone.cached().health, Health::Unknown);
}

#[test]
fn a_grant_for_the_other_zone_does_not_open_this_one() {
    let mut r = Rig::new();
    r.boot();
    r.kernel.mark_ready(LinkKind::Zone(ZoneId::Zone2));
    let req = ValidatedStart {
        zone: ZoneId::Zone2,
        outlets: slots(&[1]),
        temperature: ValveSetpoint::try_new(Cx2::from_raw(76)).unwrap(),
        duration: SessionDuration::clamped(Duration::from_secs(600)),
        command: CommandId(9),
    };
    let grant = r
        .kernel
        .authorize_open(&req, StartAuthorization::issue(BootId(1), CommandId(9)))
        .unwrap();
    let step = r.feed(ZoneEvent::Start(
        StartRequest {
            outlets: slots(&[1]),
            temperature: ValveSetpoint::try_new(Cx2::from_raw(76)).unwrap(),
            duration: SessionDuration::clamped(Duration::from_secs(600)),
            command: CommandId(9),
        },
        grant,
    ));
    assert!(matches!(
        step.refused,
        Some(Refusal::GrantForAnotherZone { .. })
    ));
    assert!(step.tx.is_none());
}

#[test]
fn a_multi_outlet_start_staggers_the_solenoids_five_hundred_milliseconds_apart() {
    let mut r = Rig::new();
    r.boot();
    let began = r.now;
    let step = r.start(slots(&[1, 2, 3]), Duration::from_secs(600));

    // The setpoint goes first, before anything opens.
    assert_eq!(
        step.tx.as_ref().map(SaturnOp::kind),
        Some(SaturnOpKind::SetTemperature)
    );

    // Drive the plan and record when each outlet write went out.
    let mut opened: Vec<(u64, SlotSet)> = Vec::new();
    let mut step = step;
    for _ in 0..12 {
        step = r.follow(&step);
        if let Some(SaturnOp::SetOutlets { slots, .. }) = step.tx.as_ref() {
            opened.push((r.now.since(began).as_millis().try_into().unwrap(), *slots));
        }
    }
    assert_eq!(opened.len(), 3, "one write per outlet: {opened:?}");
    // Cumulative sets: a 0x87 write states the whole bitmap, so a per-outlet
    // write would close what is already open.
    assert_eq!(opened[0].1, slots(&[1]));
    assert_eq!(opened[1].1, slots(&[1, 2]));
    assert_eq!(opened[2].1, slots(&[1, 2, 3]));
    assert!(
        opened[0].0 >= 500,
        "the first outlet waits a stagger: {opened:?}"
    );
    assert!(opened[1].0 - opened[0].0 >= 500, "{opened:?}");
    assert!(opened[2].0 - opened[1].0 >= 500, "{opened:?}");
}

// ------------------------------------------------------------- the session

#[test]
fn req_controller_design_sess_01_a_session_expires_at_exactly_twelve_hundred_seconds() {
    let mut r = Rig::new();
    r.boot();
    let began = r.now;
    r.start(slots(&[1]), SessionDuration::HARD_LIMIT);

    // One tick short of the limit: still running.
    r.now = began.checked_add(Duration::from_millis(1_199_999)).unwrap();
    let step = r.tick();
    assert!(
        matches!(r.zone.phase(), ZonePhase::Running { .. }),
        "the session ended early"
    );
    assert!(step.effects.is_empty());

    // Exactly the limit: stopped.
    r.now = began.checked_add(Duration::from_secs(1200)).unwrap();
    let step = r.tick();
    assert_eq!(step.tx, Some(SaturnOp::AllOff));
    assert!(step.effects.contains(&Effect::AllOff(ZoneId::Zone1)));
    // A session reaching its own limit is not a fault: it must not latch.
    assert!(
        !step
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Latch { .. })),
        "a session limit must not latch: {:?}",
        step.effects
    );
    assert!(matches!(r.zone.phase(), ZonePhase::ConfirmOff));

    // The valve acknowledges the all-off and the zone is ready again.
    let frame = echo(r.master(), SaturnOpKind::AllOff, r.firmware);
    r.feed(ZoneEvent::Response(frame));
    assert!(matches!(r.zone.phase(), ZonePhase::ReadyOff));
    assert_eq!(
        *r.kernel.state(LinkKind::Zone(ZoneId::Zone1)),
        LinkState::Ready
    );
}

#[test]
fn req_valve_control_timer_02_req_controller_design_sess_03_no_timer_refresh_is_emitted_over_a_twenty_two_minute_run()
 {
    let mut r = Rig::new();
    r.boot();
    // How many operations boot sent, not how many of them were writes: this is
    // a position in `sent`, and skipping a count of writes instead left the
    // boot all-off in the list.
    let boot_ops = r.sent.len();
    let began = r.now;
    // Follow the step the start itself produced. Ticking instead would leave
    // the setpoint transaction unanswered for the whole run, and the session
    // would end on its deadline having never opened an outlet — which reads on
    // the assertion below exactly like a machine that emits no refresh.
    let mut step = r.start(slots(&[1, 2]), SessionDuration::HARD_LIMIT);

    // Twenty-two minutes at the real 525 ms cadence. Every transmission is
    // answered, so nothing is ever waiting on a retry.
    while r.now.since(began) < Duration::from_secs(22 * 60) {
        step = r.follow(&step);
    }

    let writes: Vec<SaturnOpKind> = r
        .sent
        .iter()
        .skip(boot_ops)
        .map(SaturnOp::kind)
        .filter(|k| k.is_write())
        .collect();

    // Exactly what a start and a stop are made of, and nothing else: one
    // setpoint, one outlet write per outlet, and the all-off at the limit.
    // A timer refresh would have to be one of the five write operations, and
    // there is no room for one here.
    assert_eq!(
        writes,
        vec![
            SaturnOpKind::SetTemperature,
            SaturnOpKind::SetOutlets,
            SaturnOpKind::SetOutlets,
            SaturnOpKind::AllOff,
        ],
        "an extra write appeared over 22 minutes: {writes:?}"
    );
    // And the session did end, so the run really did pass 1200 s.
    assert!(matches!(
        r.zone.phase(),
        ZonePhase::ReadyOff | ZonePhase::ConfirmOff
    ));
    assert!(r.now.since(began) >= Duration::from_secs(1320));
}

#[test]
fn req_controller_design_sess_02_pausing_does_not_buy_a_longer_session() {
    let mut r = Rig::new();
    r.boot();
    let began = r.now;
    r.start(slots(&[1]), SessionDuration::HARD_LIMIT);
    let step = r.feed(ZoneEvent::Command(OperatorCommand::Pause {
        command: CommandId(8),
    }));
    assert_eq!(step.tx, Some(SaturnOp::Pause));
    let frame = echo(r.master(), SaturnOpKind::Pause, r.firmware);
    r.feed(ZoneEvent::Response(frame));
    assert!(matches!(r.zone.phase(), ZonePhase::Paused { .. }));

    // The deadline kept running while paused.
    r.now = began.checked_add(Duration::from_secs(1200)).unwrap();
    let step = r.tick();
    assert_eq!(step.tx, Some(SaturnOp::AllOff));
}

#[test]
fn a_commanded_stop_requires_the_acknowledgement_before_the_zone_is_ready() {
    let mut r = Rig::new();
    r.boot();
    r.start(slots(&[1]), Duration::from_secs(600));
    let step = r.feed(ZoneEvent::Command(OperatorCommand::Stop {
        command: CommandId(11),
    }));
    assert_eq!(step.tx, Some(SaturnOp::AllOff));
    assert!(matches!(r.zone.phase(), ZonePhase::ConfirmOff));

    let frame = echo(r.master(), SaturnOpKind::AllOff, r.firmware);
    r.feed(ZoneEvent::Response(frame));
    assert!(matches!(r.zone.phase(), ZonePhase::ReadyOff));
    assert!(!r.zone.cached().valve_on);
}

#[test]
fn purge_reports_water_moving_after_the_valve_says_off() {
    let settings = ZoneSettings {
        purge: Purge::Enabled {
            duration: Duration::from_secs(4),
        },
        ..ZoneSettings::default()
    };
    let mut r = Rig::with(settings);
    r.boot();
    // The boot all-off is confirmed, so the zone is already purging: "confirmed
    // off" means the valve said off, not that flow has stopped. I4 is open, and
    // this is what carrying it costs.
    assert!(matches!(r.zone.phase(), ZonePhase::Purging { .. }));
    assert!(r.zone.cached().water_moving);
    assert!(!r.zone.cached().valve_on);

    r.advance(Duration::from_secs(5));
    r.tick();
    assert!(matches!(r.zone.phase(), ZonePhase::ReadyOff));
    assert!(!r.zone.cached().water_moving);
}

// ------------------------------------------------------------ fault matrix

/// Drives one wire fault against a running zone and asserts the two things every
/// row of the matrix must produce: this zone off and latched, and the other zone
/// untouched.
fn matrix_case(fault: impl FnOnce(&mut Rig)) -> Rig {
    let mut r = Rig::new();
    r.boot_other();
    r.boot();
    r.start(slots(&[1]), Duration::from_secs(600));
    fault(&mut r);
    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
    r
}

#[test]
fn req_valve_control_safe_01_req_controller_design_valid_02_a_malformed_length_takes_the_zone_off_and_latches_it()
 {
    let r = matrix_case(|r| {
        // DATA_LEN 0x20 cannot fit the 20-byte frame maximum.
        let mut rx = RxBuffer::new();
        rx.extend(&[
            SYNC1,
            SYNC2,
            r.master().byte(),
            opcode::READ_TEMPERATURE,
            0x20,
            0x00,
        ]);
        let why = saturn::decode(
            &mut rx,
            &Expectation::response_to(r.master(), SaturnOpKind::SetTemperature),
        )
        .expect_err("an over-long DATA_LEN must be rejected");
        assert!(matches!(why, DecodeError::LengthOutOfRange { .. }));
        r.feed(ZoneEvent::DecodeFailed(why));
    });
    assert!(
        r.effects.iter().any(|e| matches!(e, Effect::Latch { .. })),
        "a malformed frame must latch"
    );
}

#[test]
fn req_controller_design_valid_02_a_checksum_failure_on_a_write_is_its_own_event() {
    let mut r = Rig::new();
    r.boot_other();
    r.boot();
    let step = r.start(slots(&[1]), Duration::from_secs(600));
    assert_eq!(
        step.tx.as_ref().map(SaturnOp::kind),
        Some(SaturnOpKind::SetTemperature)
    );

    // A well-framed acknowledgement whose checksum is wrong: this service does
    // not know whether the valve acted on the write.
    let mut bytes = wire(r.master(), opcode::WRITE_TARGET_TEMPERATURE, &[]);
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    let mut rx = RxBuffer::new();
    rx.extend(&bytes);
    let why = saturn::decode(
        &mut rx,
        &Expectation::response_to(r.master(), SaturnOpKind::SetTemperature),
    )
    .expect_err("a bad checksum must be rejected");
    assert!(matches!(why, DecodeError::BadChecksum { .. }));

    let step = r.feed(ZoneEvent::DecodeFailed(why));
    let trigger = step
        .notes
        .iter()
        .find_map(|n| match n {
            Note::Safety { trigger, .. } => Some(trigger.clone()),
            _ => None,
        })
        .expect("the escalation must be logged");
    assert!(
        trigger.contains("ChecksumFailedOnWrite"),
        "a checksum failure on a write is its own event, not a malformed frame: {trigger}"
    );
    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
}

#[test]
fn a_checksum_failure_on_a_read_is_a_malformed_response() {
    let mut r = Rig::new();
    r.boot_other();
    r.boot();
    let step = r.tick();
    assert!(!step.tx.as_ref().unwrap().kind().is_write());

    let mut bytes = wire(r.master(), opcode::READ_TEMPERATURE, &[76, 0]);
    let last = bytes.len() - 1;
    bytes[last] ^= 0x01;
    let mut rx = RxBuffer::new();
    rx.extend(&bytes);
    let why = saturn::decode(
        &mut rx,
        &Expectation::response_to(r.master(), SaturnOpKind::ReadTemperature),
    )
    .expect_err("a bad checksum must be rejected");
    let step = r.feed(ZoneEvent::DecodeFailed(why));
    let trigger = step
        .notes
        .iter()
        .find_map(|n| match n {
            Note::Safety { trigger, .. } => Some(trigger.clone()),
            _ => None,
        })
        .unwrap();
    assert!(trigger.contains("MalformedResponse"), "{trigger}");
    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
}

#[test]
fn a_delayed_response_cannot_be_correlated_and_latches() {
    let r = matrix_case(|r| {
        // The setpoint write is outstanding. Answer it a full second late, past
        // its own 320 ms deadline: by then the retry train may have re-sent the
        // request, and nothing correlates this frame with either send.
        r.advance(Duration::from_millis(1000));
        let frame = echo(r.master(), SaturnOpKind::SetTemperature, r.firmware);
        r.feed(ZoneEvent::Response(frame));
    });
    assert!(r.sent.contains(&SaturnOp::AllOff));
}

#[test]
fn a_duplicate_response_latches() {
    let mut r = Rig::new();
    r.boot_other();
    r.boot();
    let step = r.start(slots(&[1]), Duration::from_secs(600));
    let kind = step.tx.as_ref().unwrap().kind();

    r.advance(Duration::from_millis(20));
    let frame = echo(r.master(), kind, r.firmware);
    r.feed(ZoneEvent::Response(frame.clone()));
    // The same frame again, with nothing outstanding and nothing abandoned.
    r.feed(ZoneEvent::Response(frame));

    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
}

#[test]
fn a_partial_frame_never_completes_and_the_zone_latches_on_the_missing_response() {
    let mut r = Rig::new();
    r.boot_other();
    r.boot();
    r.start(slots(&[1]), Duration::from_secs(600));

    // Half an acknowledgement. The decoder correctly returns "not yet", and
    // keeps returning it, so the transaction is decided by its deadline.
    let bytes = wire(r.master(), opcode::WRITE_TARGET_TEMPERATURE, &[]);
    let mut rx = RxBuffer::new();
    rx.extend(&bytes[..3]);
    assert!(
        saturn::decode(
            &mut rx,
            &Expectation::response_to(r.master(), SaturnOpKind::SetTemperature)
        )
        .unwrap()
        .is_none(),
        "a partial frame must not decode"
    );

    r.starve();
    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
}

#[test]
fn req_valve_control_safe_01_a_missing_response_exhausts_the_retry_budget_and_latches() {
    let mut r = Rig::new();
    r.boot_other();
    r.boot();
    r.start(slots(&[1]), Duration::from_secs(600));

    // Four sends — the first and three retries — before the zone gives up.
    let before = r.sent.len();
    r.starve();
    let sends = r.sent[before..]
        .iter()
        .filter(|op| op.kind() == SaturnOpKind::SetTemperature)
        .count();
    assert_eq!(sends, 3, "the retry budget is four sends in total: {sends}");

    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
}

#[test]
fn the_documented_retry_train_does_not_fit_one_tick_and_the_machine_says_so() {
    let r = Rig::new();
    // Four sends at the 320 ms response deadline is 1280 ms against a 525 ms
    // cadence. Retries are therefore issued one per tick rather than back to
    // back, so the train cannot starve the poll. Reported, not hidden.
    assert!(r.zone.retry_train_is_deferred());
}

#[test]
fn overrunning_the_transaction_budget_is_a_link_fault_and_is_logged_as_one() {
    // An attempts budget large enough never to fire, so the *ceiling* is what
    // ends the transaction. This is the bound that stops a link which answers
    // slowly but never usefully from occupying the tick indefinitely.
    let settings = ZoneSettings {
        retry: RetryBudget {
            attempts: 200,
            ceiling: Duration::from_millis(700),
        },
        ..ZoneSettings::default()
    };
    let mut r = Rig::with(settings);
    r.boot_other();
    r.boot();
    r.start(slots(&[1]), Duration::from_secs(600));

    let mut overrun = None;
    for _ in 0..12 {
        if matches!(r.zone.phase(), ZonePhase::Unavailable { .. }) {
            break;
        }
        r.advance(Duration::from_millis(320));
        let step = r.feed(ZoneEvent::ResponseTimeout);
        for note in &step.notes {
            if let Note::Platform { detail, .. } = note
                && detail.contains("transaction budget")
            {
                overrun = Some(detail.clone());
            }
        }
        r.advance(Duration::from_millis(205));
        r.tick();
    }

    let logged = overrun.expect("the overrun must be logged, not silently absorbed");
    assert!(logged.contains("700 ms"), "{logged}");
    // Two sends inside 700 ms, not the two hundred the attempts budget allowed.
    assert_eq!(
        r.sent
            .iter()
            .filter(|op| op.kind() == SaturnOpKind::SetTemperature)
            .count(),
        2
    );
    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
}

#[test]
fn req_valve_control_err_05_req_controller_design_valid_02_a_valve_fault_bitmap_takes_the_zone_off_and_latches_it()
 {
    let mut r = Rig::new();
    r.boot_other();
    r.boot();

    // Poll until the fault read is the outstanding operation, then answer it
    // with bits set.
    let mut step = r.tick();
    for _ in 0..8 {
        if step.tx.as_ref().map(SaturnOp::kind) == Some(SaturnOpKind::ReadFaults) {
            break;
        }
        step = r.follow(&step);
    }
    assert_eq!(
        step.tx.as_ref().map(SaturnOp::kind),
        Some(SaturnOpKind::ReadFaults)
    );
    let bytes = wire(r.master(), opcode::READ_FAULT_FLAGS, &[0x00, 0x0E]);
    let mut rx = RxBuffer::new();
    rx.extend(&bytes);
    let frame = saturn::decode(
        &mut rx,
        &Expectation::response_to(r.master(), SaturnOpKind::ReadFaults),
    )
    .unwrap()
    .unwrap();
    r.feed(ZoneEvent::Response(frame));

    assert!(matches!(r.zone.cached().health, Health::Faulted { .. }));
    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
}

#[test]
fn req_controller_design_valid_02_a_temperature_the_valve_could_not_be_delivering_is_out_of_range()
{
    let mut r = Rig::new();
    r.boot_other();
    r.boot();
    let mut step = r.tick();
    for _ in 0..8 {
        if step.tx.as_ref().map(SaturnOp::kind) == Some(SaturnOpKind::ReadTemperature) {
            break;
        }
        step = r.follow(&step);
    }
    // Cx2 200 is 100 C, which is not water this valve mixes.
    let bytes = wire(r.master(), opcode::READ_TEMPERATURE, &[200, 0]);
    let mut rx = RxBuffer::new();
    rx.extend(&bytes);
    let frame = saturn::decode(
        &mut rx,
        &Expectation::response_to(r.master(), SaturnOpKind::ReadTemperature),
    )
    .unwrap()
    .unwrap();
    let step = r.feed(ZoneEvent::Response(frame));
    let trigger = step
        .notes
        .iter()
        .find_map(|n| match n {
            Note::Safety { trigger, .. } => Some(trigger.clone()),
            _ => None,
        })
        .expect("an impossible temperature must escalate");
    assert!(trigger.contains("OutOfRangeValue"), "{trigger}");
    assert_eq!(r.zone.cached().valve_reported_c, None);
    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
}

#[test]
fn req_controller_design_safe_07_a_lost_port_takes_only_its_own_zone_down() {
    let mut r = Rig::new();
    r.boot_other();
    r.boot();
    r.feed(ZoneEvent::PortClosed);
    r.assert_off_and_latched();
    r.assert_other_zone_untouched();
}

#[test]
fn req_controller_design_safe_07_a_shared_fault_is_the_only_thing_that_reaches_both_zones() {
    let mut r = Rig::new();
    r.boot_other();
    r.boot();

    let effects = r
        .kernel
        .on_event(&SafetyEvent::WatchdogMissed, r.now)
        .to_vec();
    // The kernel names both zones and all three links. Each machine applies only
    // what names its own.
    assert!(effects.contains(&Effect::AllOff(ZoneId::Zone1)));
    assert!(effects.contains(&Effect::AllOff(ZoneId::Zone2)));

    let step = r.zone.step(
        ZoneEvent::Safety(SafetyEvent::WatchdogMissed),
        r.now,
        &mut r.kernel,
    );
    r.record(&step);
    let other = r.other.step(
        ZoneEvent::Safety(SafetyEvent::WatchdogMissed),
        r.now,
        &mut r.kernel,
    );
    assert!(matches!(r.zone.phase(), ZonePhase::Unavailable { .. }));
    assert!(matches!(r.other.phase(), ZonePhase::Unavailable { .. }));
    assert_eq!(other.tx, Some(SaturnOp::AllOff));
}

#[test]
fn req_controller_design_valid_03_recovery_from_a_latch_is_never_automatic() {
    let mut r = Rig::new();
    r.boot();
    r.feed(ZoneEvent::PortClosed);

    // Ticking forever changes nothing, and there is no deadline to come back on.
    for _ in 0..10 {
        r.advance(Duration::from_secs(60));
        let step = r.tick();
        assert!(step.tx.is_none(), "a latched zone must transmit nothing");
        assert!(step.deadline.is_none());
    }
    r.feed(ZoneEvent::Acknowledged);
    assert!(matches!(
        r.zone.phase(),
        ZonePhase::Unavailable {
            acknowledged: true,
            ..
        }
    ));
    // Acknowledging does not hand a link back: the port closed when the zone
    // latched, and coming back means going through discovery again.
    let step = r.tick();
    assert!(step.tx.is_none());
}

// -------------------------------------------------------------- encoding

#[test]
fn a_step_that_opens_water_cannot_be_encoded_without_the_grant() {
    use kdtv_proto::gate::TransmitAuthority;
    use kdtv_proto::{FixtureSet, saturn::Encoder};

    let mut r = Rig::new();
    r.boot();
    let auth = TransmitAuthority::emulator_only(FixtureSet::embedded());
    let enc = Encoder::new(
        &auth,
        LinkKind::Zone(ZoneId::Zone1),
        r.master(),
        r.zone1().outlets().clone(),
    );

    // Every boot frame encodes, because none of them opens water.
    for op in r.sent.clone() {
        let step = Step {
            tx: Some(op.clone()),
            target: r.zone.address(),
            phase: kdtv_proto::LinkPhase::ReadyOff,
            effects: SmallVec::default(),
            deadline: None,
            notes: SmallVec::default(),
            refused: None,
        };
        assert!(
            r.zone.encode(&enc, &step).unwrap().is_ok(),
            "{op:?} must encode"
        );
    }

    // A hand-built outlet write, on a machine holding no grant, does not.
    let step = Step {
        tx: Some(SaturnOp::SetOutlets {
            slots: slots(&[1]),
            flags: saturn::PrimaryFlags::CAPTURED,
        }),
        target: r.zone.address(),
        phase: kdtv_proto::LinkPhase::ReadyOff,
        effects: SmallVec::default(),
        deadline: None,
        notes: SmallVec::default(),
        refused: None,
    };
    assert!(r.zone.encode(&enc, &step).unwrap().is_err());

    // The same write, after a real start, encodes — because the machine now
    // holds the grant the kernel minted.
    let opened = r.start(slots(&[1]), Duration::from_secs(600));
    let _ = opened;
    assert!(r.zone.encode(&enc, &step).unwrap().is_ok());
}

// ----------------------------------------------------------------- steam

/// A device-to-master DTV+ frame, byte-stuffed by `kdtv-proto` itself.
fn dtv_wire(dest: u8, src: u8, cmd: u8, payload: &[u8]) -> Vec<u8> {
    let chk = dtv::checksum(dest, src, cmd, payload);
    let mut out: heapless::Vec<u8, { dtv::MAX_FRAME }> = heapless::Vec::new();
    out.push(dtv::SOF).unwrap();
    dtv::escape_into(&[dest, src, cmd], &mut out).unwrap();
    dtv::escape_into(payload, &mut out).unwrap();
    dtv::escape_into(&[chk], &mut out).unwrap();
    out.push(dtv::EOF).unwrap();
    out.to_vec()
}

fn dtv_frame(dest: u8, src: u8, cmd: u8, payload: &[u8]) -> DecodedDtv {
    dtv::decode_frame(&dtv_wire(dest, src, cmd, payload)).expect("the frame must decode")
}

fn steam_status(errors: u8, state: u8) -> SteamStatus {
    SteamStatus {
        actual: Fx2::from_raw(200),
        desired: Fx2::from_raw(220),
        state: dtv::SteamStateByte::decode(state),
        timer_minutes: 10,
        timer_seconds: 0,
        errors: SteamErrorFlags::decode(errors),
    }
}

struct SteamRig {
    kernel: SafetyKernel,
    machine: SteamMachine,
    now: Monotonic,
    sent: Vec<SteamOp>,
    effects: Vec<Effect>,
}

impl SteamRig {
    fn new() -> Self {
        let settings = SteamSettings::from_timings(
            DtvTimings::DOCUMENTED,
            SessionDuration::clamped(SessionDuration::HARD_LIMIT),
        );
        Self {
            kernel: kernel(),
            machine: SteamMachine::new(settings),
            now: ms(0),
            sent: Vec::new(),
            effects: Vec::new(),
        }
    }

    fn feed(&mut self, ev: SteamEvent) -> SteamStep {
        let step = self.machine.step(ev, self.now, &mut self.kernel);
        if let Some(op) = step.tx.clone() {
            self.sent.push(op);
        }
        self.effects.extend(step.effects.iter().cloned());
        step
    }

    fn advance(&mut self, d: Duration) {
        self.now = self.now.checked_add(d).unwrap();
    }

    /// Enrol the generator: opportunity, device ID, assignment, first status.
    fn enrol(&mut self) {
        let step = self.feed(SteamEvent::Tick);
        assert_eq!(
            step.tx,
            Some(SteamOp::Discovery(dtv::DiscoveryStep::AddressOpportunity))
        );
        self.advance(Duration::from_millis(20));

        // The device asks for an address, naming its device ID.
        let step = self.feed(SteamEvent::Response(dtv_frame(
            dtv::MASTER,
            dtv::UNASSIGNED,
            dtv::opcode::DEV_REQUEST_ADDR,
            &[DeviceId::STEAM_GENERATOR.get()],
        )));
        assert_eq!(
            step.tx,
            Some(SteamOp::Discovery(dtv::DiscoveryStep::AssignAddress(
                DevAddr::REFERENCE
            )))
        );
        self.advance(Duration::from_millis(20));

        let step = self.feed(SteamEvent::Response(dtv_frame(
            dtv::MASTER,
            DevAddr::REFERENCE.get(),
            dtv::opcode::DEV_ACK,
            &[],
        )));
        assert_eq!(step.tx, Some(SteamOp::ReadStatus));
        self.advance(Duration::from_millis(20));

        self.feed(SteamEvent::Response(SteamRig::status_frame(0x00, 0x00)));
        assert!(matches!(self.machine.phase(), SteamPhase::ReadyOff));
    }

    fn status_frame(errors: u8, state: u8) -> DecodedDtv {
        dtv_frame(
            dtv::MASTER,
            DevAddr::REFERENCE.get(),
            dtv::opcode::GET_DEV_STATUS,
            &steam_status(errors, state).payload(),
        )
    }
}

#[test]
fn steam_enrolment_assigns_an_address_that_is_not_the_device_id() {
    let mut r = SteamRig::new();
    r.enrol();
    assert_eq!(r.machine.address(), Some(DevAddr::REFERENCE));
    // The steam generator's device ID is 0x05; the assigned address is 0x03.
    // Two byte-wide namespaces, and this is the test that they did not merge.
    assert_ne!(
        u16::from(r.machine.address().unwrap().get()),
        u16::from(DeviceId::STEAM_GENERATOR.get())
    );
    assert_eq!(
        *r.kernel.state(LinkKind::Steam),
        kdtv_safety::LinkState::Ready
    );
}

#[test]
fn a_generator_already_running_at_boot_is_commanded_off_before_ready() {
    let mut r = SteamRig::new();
    let step = r.feed(SteamEvent::Tick);
    assert!(step.tx.is_some());
    r.advance(Duration::from_millis(20));
    r.feed(SteamEvent::Response(dtv_frame(
        dtv::MASTER,
        dtv::UNASSIGNED,
        dtv::opcode::DEV_REQUEST_ADDR,
        &[DeviceId::STEAM_GENERATOR.get()],
    )));
    r.advance(Duration::from_millis(20));
    r.feed(SteamEvent::Response(dtv_frame(
        dtv::MASTER,
        DevAddr::REFERENCE.get(),
        dtv::opcode::DEV_ACK,
        &[],
    )));
    r.advance(Duration::from_millis(20));

    // The generator says it is producing. Boot state is off, so it is stopped.
    let step = r.feed(SteamEvent::Response(SteamRig::status_frame(0x00, 0xFF)));
    assert!(matches!(step.tx, Some(SteamOp::Stop { .. })));
    assert!(!matches!(r.machine.phase(), SteamPhase::ReadyOff));

    r.advance(Duration::from_millis(20));
    r.feed(SteamEvent::Response(dtv_frame(
        dtv::MASTER,
        DevAddr::REFERENCE.get(),
        dtv::opcode::DEV_ACK,
        &[],
    )));
    assert!(matches!(r.machine.phase(), SteamPhase::ReadyOff));
    assert_eq!(r.machine.params().state, SteamOpState::Off);
}

#[test]
fn req_hardware_spec_steam_18_a_degraded_steam_link_stops_the_generator_before_it_latches() {
    let mut r = SteamRig::new();
    r.enrol();

    let step = r.feed(SteamEvent::Safety(SafetyEvent::SteamLinkDegraded {
        why: DegradeReason::Timeouts,
    }));

    // The kernel asked for the compound action, and the stop is already on the
    // wire. The link is NOT latched yet.
    assert!(step.effects.contains(&Effect::SteamStopThenLatch));
    assert!(matches!(step.tx, Some(SteamOp::Stop { .. })));
    assert!(
        matches!(r.machine.phase(), SteamPhase::StoppingBeforeLatch { .. }),
        "found {:?}",
        r.machine.phase()
    );
    assert!(
        !step.effects.contains(&Effect::ClosePort(LinkKind::Steam)),
        "the port must not close before the stop is acknowledged"
    );

    // The acknowledgement arrives, and only now does the link go.
    r.advance(Duration::from_millis(20));
    let step = r.feed(SteamEvent::Response(dtv_frame(
        dtv::MASTER,
        DevAddr::REFERENCE.get(),
        dtv::opcode::DEV_ACK,
        &[],
    )));
    assert!(step.effects.contains(&Effect::ClosePort(LinkKind::Steam)));
    assert!(matches!(r.machine.phase(), SteamPhase::Unavailable { .. }));
    assert!(!r.machine.cached().steaming);
}

#[test]
fn a_lost_steam_port_latches_directly_because_nothing_can_be_told_anything() {
    let mut r = SteamRig::new();
    r.enrol();

    let step = r.feed(SteamEvent::PortClosed);
    assert!(step.tx.is_none(), "a lost port cannot be sent a stop");
    assert!(!step.effects.contains(&Effect::SteamStopThenLatch));
    assert!(step.effects.contains(&Effect::ClosePort(LinkKind::Steam)));
    assert!(matches!(r.machine.phase(), SteamPhase::Unavailable { .. }));
}

#[test]
fn an_unacknowledged_stop_latches_anyway_and_every_retry_was_a_stop() {
    let mut r = SteamRig::new();
    r.enrol();
    r.feed(SteamEvent::Safety(SafetyEvent::SteamLinkDegraded {
        why: DegradeReason::Nak,
    }));
    let before = r.sent.len();

    for _ in 0..12 {
        if matches!(r.machine.phase(), SteamPhase::Unavailable { .. }) {
            break;
        }
        r.advance(Duration::from_millis(300));
        r.feed(SteamEvent::ResponseTimeout);
        r.advance(Duration::from_millis(200));
        r.feed(SteamEvent::Tick);
    }
    assert!(matches!(r.machine.phase(), SteamPhase::Unavailable { .. }));
    assert!(
        r.sent[before..]
            .iter()
            .all(|op| matches!(op, SteamOp::Stop { .. })),
        "every frame on a dying link must be a stop: {:?}",
        &r.sent[before..]
    );
    assert!(r.effects.contains(&Effect::ClosePort(LinkKind::Steam)));
}

#[test]
fn req_steam_generator_steam_15_every_documented_error_bit_stops_the_generator_and_every_combination_does_too()
 {
    // CORRECTIONS.md item 9: the bits are independent and can be set together.
    // An undocumented bit is a fault too — an error byte nobody wrote down is
    // not evidence of health.
    for bits in [0x04u8, 0x08, 0x20, 0x40, 0x24, 0x6C, 0x80] {
        let mut r = SteamRig::new();
        r.enrol();
        r.advance(Duration::from_millis(600));
        let step = r.feed(SteamEvent::Tick);
        assert_eq!(step.tx, Some(SteamOp::ReadStatus));
        r.advance(Duration::from_millis(20));
        let step = r.feed(SteamEvent::Response(SteamRig::status_frame(bits, 0xFF)));
        assert!(
            step.effects.contains(&Effect::SteamStopThenLatch),
            "error byte 0x{bits:02X} must stop the generator"
        );
        assert!(matches!(step.tx, Some(SteamOp::Stop { .. })));
        assert_eq!(r.machine.cached().error_bits, Some(bits));
    }
}

#[test]
fn req_steam_generator_steam_18_req_steam_adapter_steam_11_a_steam_start_carries_a_non_zero_duration_and_never_the_power_clean_byte()
 {
    use kdtv_proto::FixtureSet;
    use kdtv_proto::dtv::{SET_PARAM_STATE_OFFSET, SteamEncoder};
    use kdtv_proto::gate::TransmitAuthority;

    let mut r = SteamRig::new();
    r.enrol();
    let step = r.feed(SteamEvent::Command(SteamCommand::Start {
        temp: SteamSetpoint::clamped(Fx2::from_raw(220)).0,
        minutes: SteamMinutes::try_new(15).unwrap(),
        command: CommandId(3),
    }));
    let Some(SteamOp::Start { minutes, .. }) = step.tx.as_ref() else {
        panic!("a start must transmit a start: {:?}", step.tx);
    };
    // steamTimerSetTime = 0 disables the generator's own shutoff, which is the
    // only backstop that survives this service dying. SteamMinutes has no zero.
    assert!(minutes.wire() >= SteamMinutes::MIN);

    // And the operation-state byte is never 0xCC, in any frame this machine can
    // produce. Denial by absence of a variant of a payload field.
    let auth = TransmitAuthority::emulator_only(FixtureSet::embedded());
    let enc = SteamEncoder::new(&auth);
    let frame = r.machine.encode(&enc, &step).unwrap().unwrap();
    let logical: Vec<u8> = frame
        .bytes()
        .iter()
        .copied()
        .filter(|b| *b != dtv::ESC)
        .collect();
    assert!(
        !logical.contains(&SteamOpState::POWER_CLEAN_BYTE),
        "0xCC reached the wire: {:02X?}",
        frame.bytes()
    );
    // Named position, not just "somewhere in the frame": SOF, DEST, SRC, CMD,
    // then the payload.
    assert_eq!(logical[4 + SET_PARAM_STATE_OFFSET], SteamOpState::On.wire());
}

#[test]
fn req_steam_adapter_steam_20_a_steam_session_stops_at_its_own_limit() {
    let mut r = SteamRig::new();
    r.enrol();
    let began = r.now;
    let step = r.feed(SteamEvent::Command(SteamCommand::Start {
        temp: SteamSetpoint::clamped(Fx2::from_raw(220)).0,
        minutes: SteamMinutes::try_new(5).unwrap(),
        command: CommandId(3),
    }));
    assert!(matches!(step.tx, Some(SteamOp::Start { .. })));

    r.now = began.checked_add(Duration::from_secs(299)).unwrap();
    let step = r.feed(SteamEvent::Tick);
    assert!(!matches!(step.tx, Some(SteamOp::Stop { .. })));

    r.now = began.checked_add(Duration::from_secs(300)).unwrap();
    let step = r.feed(SteamEvent::Tick);
    assert!(
        matches!(step.tx, Some(SteamOp::Stop { .. })),
        "the five-minute request is the shorter of the two limits"
    );
}

#[test]
fn req_controller_design_boot_02_a_port_carrying_the_wrong_device_is_refused_rather_than_enrolled()
{
    let mut r = SteamRig::new();
    r.feed(SteamEvent::Tick);
    r.advance(Duration::from_millis(20));
    // The rain panel, not the steam generator.
    let step = r.feed(SteamEvent::Response(dtv_frame(
        dtv::MASTER,
        dtv::UNASSIGNED,
        dtv::opcode::DEV_REQUEST_ADDR,
        &[DeviceId::RAIN_PANEL.get()],
    )));
    assert!(matches!(r.machine.phase(), SteamPhase::Unavailable { .. }));
    assert!(r.machine.address().is_none());
    assert!(
        step.effects
            .iter()
            .any(|e| matches!(e, Effect::OperatorMessage { .. }))
    );
}
