//! The valve-zone state machine: one per Saturn bus.
//!
//! Sans-IO. It performs no transmission, owns no port and reads no clock: an
//! event and a [`Monotonic`] go in, a [`Step`] comes out, and the service does
//! whatever the step says. That is what lets the entire fault matrix run at its
//! real constants — a 525 ms tick, a 320 ms response deadline, a 1200 s session
//! — in microseconds, with no hardware and no waiting.
//!
//! # The path to water
//!
//! ```text
//! Cold -> Discovery -> Identify -> ConfirmOff -> ReadyOff -> Running
//! ```
//!
//! **There is no edge from `Cold` to `Running`.** [`ZonePhase::Running`] is
//! reachable only from [`ZonePhase::ReadyOff`], and only by an event carrying an
//! [`OpenGrant`], which only `kdtv-safety`'s kernel can mint. Nothing about a
//! session is persisted, so a restart or a watchdog reset re-enters `Cold` and
//! walks the whole sequence again. `SAFE-04`, `BOOT-01`.
//!
//! # The boot sequence
//!
//! `BOOT-01`..`BOOT-08`, in order:
//!
//! 1. The port opens with no start transmitted — this machine starts in `Cold`
//!    and its first transmission is a read.
//! 2. Discovery **probes** addresses `0x03..=0x07` and refuses to proceed if
//!    more than one valve answers. `CORRECTIONS.md` item 10.
//! 3. Identity, firmware, serial, calibration, configuration, outlets,
//!    temperature and faults are read. Calibration and configuration are read
//!    because `PH0-01`'s rollback baseline needs them and, once the K-99695 is
//!    powered down, this service is the only thing that can read them.
//!    `CORRECTIONS.md` item 7.
//! 4. All-off is sent and its acknowledgement is **required**. A valve that does
//!    not acknowledge leaves the zone unavailable with an operator message
//!    saying to remove valve power. `BOOT-05` / `BOOT-08`.
//! 5. Only then `ReadyOff`.
//!
//! ## Discovery is read-only
//!
//! The three address-management operations are on the allowlist and remain
//! encodable in [`LinkPhase::Discovery`], but **this machine never emits one**.
//! Probing with a read establishes which addresses are occupied without
//! rewriting what is installed, and `PH0-01` needs the installed addressing
//! preserved so the rollback baseline still describes the system. A bus with two
//! valves is refused rather than re-addressed.
//!
//! # The session limit
//!
//! [`SessionDeadline`] from `kdtv-safety`, which has no `extend`, no `refresh`
//! and no setter. **No path in this file emits a timer-refresh operation**, and
//! the twenty-two-minute test in `crate::tests` asserts it over simulated time.
//! Whether the Prompt 3's own 1800-second timer is refreshed by ordinary polling
//! is unresolved — capture question 5 — so it is not counted as a backstop in
//! either direction. `SESS-01`..`SESS-04`.
//!
//! # Purge
//!
//! Investigation I4 is open: whether this system performs an automatic purge
//! after a stop is not known. [`ZonePhase::Purging`] and
//! [`ZoneCache::water_moving`] exist so that "confirmed off" can mean flow has
//! stopped rather than the valve has been commanded, if it turns out purge is
//! enabled. It is configuration, and it is off by default. `PURGE-01`.
//!
//! # Deviations from `ARCHITECTURE.md` § 8, and why
//!
//! - [`ZoneMachine::step`] takes `&mut SafetyKernel`. The kernel holds all three
//!   links' state because a shared fault escalates across them, so it cannot be
//!   owned per machine. Passing it in is also what lets a test assert the
//!   **other** zone was untouched.
//! - The [`DiscoveryToken`] is minted at encode time rather than held across the
//!   phase. `DiscoveryToken::mint` returns `None` outside
//!   [`LinkPhase::Discovery`], so minting on demand has exactly the property
//!   holding one has, without making the machine `!Send` for a second reason.
//!   (It is `!Send` anyway while it holds an [`OpenGrant`]; that is deliberate
//!   in `kdtv-safety`, and the service must place a zone task accordingly.)
//! - `Step` carries [`Note`]s rather than `LogEvent`s. See [`crate::note`].
//! - `OperatorCommand` has no `SetOutlets`. Changing which outlets are open is
//!   opening water, so it needs a grant, and `SafetyKernel::authorize_open`
//!   refuses a zone that is already running. Changing outlets therefore means
//!   stopping and starting again — a consequence of the kernel, recorded here
//!   rather than worked around.

use crate::budget::RetryBudget;
use crate::note::Note;
use kdtv_config::ZoneConfig;
use kdtv_proto::saturn::{
    ControlByte, DecodeError, DecodedFrame, DiscoveryToken, EncodeDenied, Encoder, ErrorTable,
    Expectation, FirmwareTypeId, LinkPhase, MasterAddr, OutletTable, PrimaryFlags, RawErrorByte,
    SaturnFrame, SaturnOp, SaturnOpKind, Timings, ValveAddr, ValveControl, ValveType, opcode,
};
use kdtv_safety::{Effect, LatchReason, OpenGrant, SafetyEvent, SafetyKernel, SessionDeadline};
use kdtv_telemetry::{Monotonic, PlatformEvent, StopReason};
use kdtv_units::{CommandId, Cx2, LinkKind, SessionDuration, Slot, SlotSet, ValveSetpoint, ZoneId};
use serde::Serialize;
use smallvec::SmallVec;
use std::time::Duration;

/// Where a zone is. The phase names are the design's, and the payloads are
/// monotonic readings because nothing here may consult a wall clock.
///
/// Not `Eq`: `kdtv_safety::LatchReason` carries a divergence in degrees, and a
/// float has no total equality. Comparing two latch reasons for exact equality
/// is not something this crate should be doing anyway.
#[derive(Clone, PartialEq, Debug)]
pub enum ZonePhase {
    /// The entry state on every boot, restart and watchdog reset. Nothing has
    /// been established and nothing has been transmitted.
    Cold,
    /// Probing `0x03..=0x07`. The only phase in which address operations could
    /// be encoded at all — this machine still does not emit one.
    Discovery,
    /// Reading identity, firmware, calibration, outlets, temperature and faults.
    Identify,
    /// All-off has been sent and its acknowledgement is outstanding. Entered at
    /// boot and after every stop.
    ConfirmOff,
    /// Confirmed off. The only phase a start is accepted from.
    ReadyOff,
    /// Water is on.
    Running {
        since: Monotonic,
        command: CommandId,
    },
    /// Water is on but held. The session deadline keeps running: pausing is not
    /// a way to have a longer session.
    Paused {
        since: Monotonic,
        command: CommandId,
    },
    /// The valve reports off and water may still be moving. `PURGE-01`.
    Purging { since: Monotonic, until: Monotonic },
    /// Unavailable until a person acknowledges it. Recovery is never automatic.
    Unavailable {
        reason: LatchReason,
        acknowledged: bool,
    },
}

impl ZonePhase {
    /// The phase without its payload, for the cache and the API.
    #[must_use]
    pub const fn kind(&self) -> ZonePhaseKind {
        match self {
            Self::Cold => ZonePhaseKind::Cold,
            Self::Discovery => ZonePhaseKind::Discovery,
            Self::Identify => ZonePhaseKind::Identify,
            Self::ConfirmOff => ZonePhaseKind::ConfirmOff,
            Self::ReadyOff => ZonePhaseKind::ReadyOff,
            Self::Running { .. } => ZonePhaseKind::Running,
            Self::Paused { .. } => ZonePhaseKind::Paused,
            Self::Purging { .. } => ZonePhaseKind::Purging,
            Self::Unavailable { .. } => ZonePhaseKind::Unavailable,
        }
    }

    /// The encoder phase this maps to.
    ///
    /// `Identify` and `ConfirmOff` map to [`LinkPhase::Booting`], which permits
    /// reads and — by the encoder's own exemption — all-off, and nothing else.
    /// A start frame is unencodable in either.
    #[must_use]
    pub const fn link_phase(&self) -> LinkPhase {
        match self {
            Self::Cold | Self::Identify | Self::ConfirmOff => LinkPhase::Booting,
            Self::Discovery => LinkPhase::Discovery,
            // Purging: the valve has been commanded off and only reads are due.
            Self::ReadyOff | Self::Purging { .. } => LinkPhase::ReadyOff,
            Self::Running { .. } => LinkPhase::Running,
            Self::Paused { .. } => LinkPhase::Paused,
            Self::Unavailable { .. } => LinkPhase::Faulted,
        }
    }
}

/// [`ZonePhase`] without its payloads.
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZonePhaseKind {
    #[default]
    Cold,
    Discovery,
    Identify,
    ConfirmOff,
    ReadyOff,
    Running,
    Paused,
    Purging,
    Unavailable,
}

impl std::fmt::Display for ZonePhaseKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cold => "cold",
            Self::Discovery => "discovery",
            Self::Identify => "identify",
            Self::ConfirmOff => "confirm-off",
            Self::ReadyOff => "ready-off",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Purging => "purging",
            Self::Unavailable => "unavailable",
        })
    }
}

/// What the valve's fault read established.
///
/// There is no way to reach [`Health::Clear`] from an error **byte**: the two
/// documented tables invert 0 and 1, so `FaultDisposition` has no `Healthy`
/// variant and `CORRECTIONS.md` item 4 forbids inventing one. Health becomes
/// clear only when a `0x0F` fault-flag read returns zero in **both** byte
/// orders, which is a positive statement rather than the absence of a negative
/// one. A start requires it.
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "health")]
pub enum Health {
    /// Nothing has been read, or what was read establishes nothing.
    #[default]
    Unknown,
    /// A fault-flag read came back clear in both byte orders.
    Clear,
    /// A fault-flag read came back set, in at least one byte order.
    Faulted { bits: u16 },
}

/// Whether this zone performs an automatic purge after a stop.
///
/// Investigation I4 is **open**. Neither position is asserted here: the default
/// is disabled, and enabling it changes what "confirmed off" means without
/// changing what is transmitted.
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug)]
pub enum Purge {
    #[default]
    Disabled,
    Enabled {
        duration: Duration,
    },
}

/// Everything about a zone that is not the wire codec.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ZoneSettings {
    pub timings: Timings,
    /// Which of the two incompatible error tables to read an error byte under.
    /// Naming one is mandatory; which is right for this hardware is unresolved.
    pub error_table: ErrorTable,
    pub session_cap: SessionDuration,
    pub purge: Purge,
    pub retry: RetryBudget,
    /// The `DATA[1]` flags byte of a `0x87` write. `FLAG-01` is unresolved: the
    /// only captured turn-on frame carries `0x00`.
    pub flags: PrimaryFlags,
    /// The Phase 0 calibration read-back, if one was recorded. A difference is
    /// reported in [`ZoneCache::baseline_drift`]. `PH0-01`.
    pub calibration_baseline: Option<Vec<u8>>,
    /// The Phase 0 configuration read-back, if one was recorded. `PH0-01`.
    pub configuration_baseline: Option<Vec<u8>>,
}

impl Default for ZoneSettings {
    fn default() -> Self {
        let timings = Timings::DOCUMENTED;
        Self {
            timings,
            error_table: ErrorTable::ValveControl,
            session_cap: SessionDuration::clamped(SessionDuration::HARD_LIMIT),
            purge: Purge::Disabled,
            retry: RetryBudget::from_saturn(&timings),
            flags: PrimaryFlags::CAPTURED,
            calibration_baseline: None,
            configuration_baseline: None,
        }
    }
}

/// A value that differs from the Phase 0 baseline. `PH0-01`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct BaselineDrift {
    pub field: &'static str,
    pub baseline: String,
    pub observed: String,
}

/// What the service reports without touching the bus.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct ZoneCache {
    pub zone: ZoneId,
    pub phase: ZonePhaseKind,
    /// The valve's own thermistor reading. Not evidence on its own — the
    /// independent probe is the other half, and it belongs to the service.
    pub valve_reported_c: Option<f32>,
    /// Which configured slots the valve last reported open.
    pub outlets: SlotSet,
    pub faults: Option<u16>,
    pub health: Health,
    /// Whether an outlet is commanded open.
    pub valve_on: bool,
    /// Whether water may still be moving. Reported **separately** from
    /// `valve_on`, so that a purge — if I4 turns out to be enabled — is visible
    /// as flow that outlives the command. `PURGE-01`.
    pub water_moving: bool,
    pub session_remaining_s: Option<u64>,
    /// Sends on the operation currently in flight.
    pub attempts: u32,
    pub firmware_type: Option<u8>,
    pub firmware_version: Option<String>,
    pub serial_number: Option<String>,
    pub calibration: Option<String>,
    pub configuration: Option<String>,
    /// Where the valve no longer matches the Phase 0 baseline.
    ///
    /// Carried in the cache rather than logged as an event: `kdtv-telemetry`'s
    /// `LogEvent` has no finding variant, and mapping a baseline difference onto
    /// a safety or platform line would mislabel it. The service logs it.
    pub baseline_drift: Vec<BaselineDrift>,
    pub as_of: Option<Monotonic>,
}

impl ZoneCache {
    fn new(zone: ZoneId) -> Self {
        Self {
            zone,
            phase: ZonePhaseKind::Cold,
            valve_reported_c: None,
            outlets: SlotSet::EMPTY,
            faults: None,
            health: Health::Unknown,
            valve_on: false,
            water_moving: false,
            session_remaining_s: None,
            attempts: 0,
            firmware_type: None,
            firmware_version: None,
            serial_number: None,
            calibration: None,
            configuration: None,
            baseline_drift: Vec::new(),
            as_of: None,
        }
    }
}

/// A start, already clamped and slot-checked by the layers above.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StartRequest {
    pub outlets: SlotSet,
    pub temperature: ValveSetpoint,
    pub duration: SessionDuration,
    pub command: CommandId,
}

/// What an operator can ask of a running zone.
///
/// No `SetOutlets`: changing which outlets are open opens water, so it needs a
/// grant, and the kernel refuses to mint one for a zone that is already running.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum OperatorCommand {
    SetTemperature {
        temp: ValveSetpoint,
        command: CommandId,
    },
    Pause {
        command: CommandId,
    },
    Resume {
        command: CommandId,
    },
    Stop {
        command: CommandId,
    },
}

impl OperatorCommand {
    const fn id(&self) -> CommandId {
        match self {
            Self::SetTemperature { command, .. }
            | Self::Pause { command }
            | Self::Resume { command }
            | Self::Stop { command } => *command,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Self::SetTemperature { .. } => "SetTemperature",
            Self::Pause { .. } => "Pause",
            Self::Resume { .. } => "Resume",
            Self::Stop { .. } => "Stop",
        }
    }
}

/// Why the engine refused a command.
///
/// A refusal transmits nothing and changes no valve state. Distinct from
/// `kdtv-safety`'s [`kdtv_safety::Denial`] — which answers "may this zone open"
/// — because these answer "does this machine agree", and the two are checked at
/// different layers on purpose. A grant obtained from the kernel is still
/// refused here if this machine has not itself confirmed the valve off.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum Refusal {
    #[error("the grant authorises {authorised}, this machine drives {zone}")]
    GrantForAnotherZone { authorised: ZoneId, zone: ZoneId },
    #[error("a start is accepted only from ready-off; this zone is {phase}")]
    NotReadyOff { phase: ZonePhaseKind },
    #[error("the valve's health is not established; a start needs a clear fault read")]
    HealthNotEstablished,
    #[error("no outlets were requested")]
    NoOutlets,
    #[error("outlet slot {0} is not on this valve's outlet table")]
    UnconfiguredOutlet(Slot),
    #[error("{command} is not accepted while the zone is {phase}")]
    WrongPhase {
        command: &'static str,
        phase: ZonePhaseKind,
    },
}

/// Everything the engine wants done as a result of one event.
///
/// # The contract
///
/// The service transmits [`Step::tx`] **first**, then performs [`Step::effects`]
/// in order. That ordering is what makes an all-off reach the valve before its
/// port is closed on the way into a latch.
#[derive(Clone, Debug)]
pub struct Step {
    /// The one operation to transmit, if any. One in flight at a time, always:
    /// the Saturn frame has no sender field, so a serialised bus is the only
    /// thing that correlates a response with its request.
    pub tx: Option<SaturnOp>,
    /// The address `tx` is for.
    pub target: Option<ValveAddr>,
    /// The encoder phase `tx` must be encoded under.
    pub phase: LinkPhase,
    /// What the safety kernel returned, unaltered.
    pub effects: SmallVec<[Effect; 4]>,
    /// When the service should call [`ZoneMachine::step`] again.
    pub deadline: Option<Monotonic>,
    pub notes: SmallVec<[Note; 2]>,
    /// Set when a command was refused. Nothing was transmitted.
    pub refused: Option<Refusal>,
}

impl Step {
    fn new(phase: LinkPhase) -> Self {
        Self {
            tx: None,
            target: None,
            phase,
            effects: SmallVec::new(),
            deadline: None,
            notes: SmallVec::new(),
            refused: None,
        }
    }
}

/// The staggered outlet plan for a multi-outlet start.
///
/// Solenoids are energised one at a time, 500 ms apart, rather than together:
/// the sources attribute relay damage to the inrush. Each step carries the
/// **cumulative** set, because a `0x87` write states the whole bitmap and a
/// per-outlet write would close the ones already open. `OUT-05`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StaggerPlan {
    steps: SmallVec<[(Monotonic, SlotSet); 6]>,
    next: usize,
}

impl StaggerPlan {
    fn build(start: Monotonic, spacing: Duration, slots: SlotSet) -> Self {
        let mut steps: SmallVec<[(Monotonic, SlotSet); 6]> = SmallVec::new();
        let mut cumulative = SlotSet::EMPTY;
        for (i, slot) in slots.iter().enumerate() {
            cumulative = cumulative.insert(slot);
            // Step 0 falls one spacing after the command: the setpoint write
            // goes first and the mixing valve gets the same interval to reach
            // it before anything opens.
            let due = spacing
                .checked_mul(u32::try_from(i).unwrap_or(u32::MAX).saturating_add(1))
                .and_then(|d| start.checked_add(d))
                .unwrap_or(start);
            steps.push((due, cumulative));
        }
        Self { steps, next: 0 }
    }

    /// Every step, as `(when, cumulative slots)`.
    #[must_use]
    pub fn steps(&self) -> &[(Monotonic, SlotSet)] {
        &self.steps
    }

    fn due(&self, now: Monotonic) -> Option<SlotSet> {
        self.steps
            .get(self.next)
            .filter(|(at, _)| now >= *at)
            .map(|(_, s)| *s)
    }

    fn next_due(&self) -> Option<Monotonic> {
        self.steps.get(self.next).map(|(at, _)| *at)
    }

    fn advance(&mut self) {
        self.next = self.next.saturating_add(1);
    }

    fn finished(&self) -> bool {
        self.next >= self.steps.len()
    }
}

/// What an in-flight transaction is for, and therefore what its answer means.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Job {
    /// A discovery probe. A timeout means the address is empty, not that the
    /// link is faulty.
    Probe(ValveAddr),
    Identify(usize),
    /// The all-off whose acknowledgement `BOOT-05` requires.
    ConfirmOff,
    Poll,
    SetTemperature,
    Open(SlotSet),
    Pause,
    Resume,
}

#[derive(Clone, Debug)]
struct InFlight {
    op: SaturnOp,
    job: Job,
    target: ValveAddr,
    started_at: Monotonic,
    sent_at: Monotonic,
    attempts: u8,
    awaiting_retry: bool,
}

/// One valve bus, as a state machine.
#[derive(Debug)]
pub struct ZoneMachine {
    zone: ZoneId,
    link: LinkKind,
    master: MasterAddr,
    expected_valve: ValveType,
    outlets: OutletTable,
    settings: ZoneSettings,

    phase: ZonePhase,
    address: Option<ValveAddr>,
    scan_next: u8,
    found: SmallVec<[ValveAddr; 2]>,
    identify_step: usize,
    poll_step: usize,

    inflight: Option<InFlight>,
    /// Responses to transactions this machine gave up on, which may still be in
    /// the pipe. Counted rather than ignored blindly, so a genuinely duplicated
    /// or unsolicited frame is still a fault.
    stale: u8,
    stagger: Option<StaggerPlan>,
    session: Option<SessionDeadline>,
    grant: Option<OpenGrant>,
    command: Option<CommandId>,
    /// Set while an all-off is outstanding after a stop, so the acknowledgement
    /// knows what to record and where to go next.
    stopping: Option<StopReason>,
    next_tick: Option<Monotonic>,
    cache: ZoneCache,
}

/// The identity reads the boot sequence performs, in order.
///
/// Firmware type is first: everything after it is read through the outlet table,
/// and the table is only correct for the family the valve actually is.
/// Calibration and configuration are here for `PH0-01`'s rollback baseline;
/// their **writes** are denied by having no variant. `CORRECTIONS.md` item 7.
const IDENTIFY: [SaturnOpKind; 8] = [
    SaturnOpKind::ReadFirmwareType,
    SaturnOpKind::ReadFirmwareVersion,
    SaturnOpKind::ReadSerialNumber,
    SaturnOpKind::ReadCalibration,
    SaturnOpKind::ReadConfiguration,
    SaturnOpKind::ReadOutlets,
    SaturnOpKind::ReadTemperature,
    SaturnOpKind::ReadFaults,
];

/// The polling rotation, one operation per tick.
const POLL: [SaturnOpKind; 3] = [
    SaturnOpKind::ReadTemperature,
    SaturnOpKind::ReadFaults,
    SaturnOpKind::ReadOutlets,
];

/// What an operator must do when a valve will not confirm itself off. `BOOT-08`.
const CANNOT_CONFIRM_OFF: &str = "This zone could not be confirmed off. Remove valve power and \
                                  close the hot and cold service shutoffs before using the \
                                  shower.";

/// What an operator must do when a bus answers with more than one valve.
const TOO_MANY_VALVES: &str = "More than one valve answered on this bus. This system is wired one \
                               valve per bus; check the cabling before running anything.";

/// What an operator must do when nothing answers at all.
const NO_VALVE: &str = "No valve answered on this bus. Check the converter, the cabling and valve \
                        power.";

impl ZoneMachine {
    /// Always starts [`ZonePhase::Cold`]. There is no constructor that does not.
    #[must_use]
    pub fn new(cfg: &ZoneConfig, settings: ZoneSettings) -> Self {
        Self {
            zone: cfg.id(),
            link: cfg.link(),
            master: cfg.master(),
            expected_valve: cfg.expected_valve(),
            outlets: cfg.outlets().clone(),
            settings,
            phase: ZonePhase::Cold,
            address: None,
            scan_next: kdtv_proto::saturn::VALVE_ADDR_MIN,
            found: SmallVec::new(),
            identify_step: 0,
            poll_step: 0,
            inflight: None,
            stale: 0,
            stagger: None,
            session: None,
            grant: None,
            command: None,
            stopping: None,
            next_tick: None,
            cache: ZoneCache::new(cfg.id()),
        }
    }

    #[must_use]
    pub const fn zone(&self) -> ZoneId {
        self.zone
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        self.link
    }

    #[must_use]
    pub const fn phase(&self) -> &ZonePhase {
        &self.phase
    }

    #[must_use]
    pub const fn cached(&self) -> &ZoneCache {
        &self.cache
    }

    /// The valve address discovery settled on, once it has.
    #[must_use]
    pub const fn address(&self) -> Option<ValveAddr> {
        self.address
    }

    /// True when the configured retry train cannot finish inside one tick, so
    /// attempts are spread across ticks. Reported rather than hidden.
    #[must_use]
    pub fn retry_train_is_deferred(&self) -> bool {
        self.settings.timings.retry_train_overruns_budget()
    }

    /// What the decoder should expect next, or `None` with nothing in flight.
    #[must_use]
    pub fn expectation(&self) -> Option<Expectation> {
        self.inflight
            .as_ref()
            .map(|f| Expectation::response_to(self.master, f.op.kind()))
    }

    /// Encode a step's transmission, supplying the phase, the discovery token
    /// and the open authority from this machine's own state.
    ///
    /// Returns `None` when the step has nothing to transmit. The
    /// [`DiscoveryToken`] is minted here rather than held: `mint` returns `None`
    /// outside [`LinkPhase::Discovery`], so an address operation is unspellable
    /// in any other phase either way.
    ///
    /// The [`OpenGrant`] is passed as the encoder's open authority, and it is
    /// the only thing in the workspace that implements one. A machine with no
    /// grant cannot produce a `SetOutlets` frame, whatever its phase says.
    pub fn encode(&self, enc: &Encoder, step: &Step) -> Option<Result<SaturnFrame, EncodeDenied>> {
        let op = step.tx.as_ref()?;
        let target = step.target.or(self.address).unwrap_or(ValveAddr::ALL[0]);
        let token = DiscoveryToken::mint(self.link, step.phase);
        Some(
            enc.encode(
                target,
                op,
                step.phase,
                token.as_ref(),
                self.grant
                    .as_ref()
                    .map(|g| -> &dyn kdtv_units::OpenAuthority { g }),
            ),
        )
    }

    /// Advance the machine.
    ///
    /// `now` is a monotonic reading supplied by the caller. This crate reads no
    /// clock, which is what makes a hundred simulated seconds cost microseconds.
    pub fn step(&mut self, ev: ZoneEvent, now: Monotonic, kernel: &mut SafetyKernel) -> Step {
        let mut out = Step::new(self.phase.link_phase());
        match ev {
            ZoneEvent::Tick => self.on_tick(now, kernel, &mut out),
            ZoneEvent::Response(frame) => self.on_response(&frame, now, kernel, &mut out),
            ZoneEvent::DecodeFailed(why) => self.on_decode_failed(why, now, kernel, &mut out),
            ZoneEvent::ResponseTimeout => self.on_timeout(now, kernel, &mut out),
            ZoneEvent::Start(req, grant) => self.on_start(&req, grant, now, &mut out),
            ZoneEvent::Command(cmd) => self.on_command(&cmd, now, &mut out),
            ZoneEvent::Safety(event) => self.escalate(&event, now, kernel, &mut out),
            ZoneEvent::PortClosed => {
                let lost = SafetyEvent::PortLost { link: self.link };
                self.escalate(&lost, now, kernel, &mut out);
            }
            ZoneEvent::Acknowledged => self.on_acknowledged(),
        }
        out.phase = self.phase.link_phase();
        out.deadline = self.next_deadline(now);
        self.refresh_cache(now);
        out
    }

    // ---- ticks ----------------------------------------------------------

    fn on_tick(&mut self, now: Monotonic, kernel: &mut SafetyKernel, out: &mut Step) {
        self.next_tick = now.checked_add(self.settings.timings.tick);

        if matches!(self.phase, ZonePhase::Unavailable { .. }) {
            return;
        }

        // A session reaching its own limit is checked before anything is
        // transmitted, so an expiry is never one tick late behind a poll.
        if let Some(session) = self.session
            && session.expired(now)
        {
            let expired = SafetyEvent::SessionExpired { zone: self.zone };
            self.escalate(&expired, now, kernel, out);
            return;
        }

        if let Some(until) = self.purge_until()
            && now >= until
        {
            self.finish_purge(kernel);
        }

        // A retry is issued on a tick and never sooner, so a train that cannot
        // finish inside one tick is deferred rather than starving the cadence.
        if let Some(f) = self.inflight.as_mut()
            && f.awaiting_retry
        {
            f.awaiting_retry = false;
            f.attempts = f.attempts.saturating_add(1);
            f.sent_at = now;
            out.tx = Some(f.op.clone());
            out.target = Some(f.target);
            return;
        }
        if self.inflight.is_some() {
            return;
        }

        match self.phase {
            ZonePhase::Cold => self.begin_discovery(now, kernel, out),
            ZonePhase::Discovery => self.next_probe(now, kernel, out),
            ZonePhase::Identify => self.next_identify(now, out),
            ZonePhase::ReadyOff | ZonePhase::Purging { .. } | ZonePhase::Paused { .. } => {
                self.send_poll(now, out);
            }
            ZonePhase::Running { .. } => self.run_tick(now, out),
            // ConfirmOff has an all-off outstanding by construction; if it does
            // not, the transaction was consumed by an escalation and a fresh one
            // is sent.
            ZonePhase::ConfirmOff => self.send_all_off(now, out),
            ZonePhase::Unavailable { .. } => {}
        }
    }

    fn run_tick(&mut self, now: Monotonic, out: &mut Step) {
        if let Some(plan) = self.stagger.as_ref()
            && let Some(slots) = plan.due(now)
        {
            let op = SaturnOp::SetOutlets {
                slots,
                flags: self.settings.flags,
            };
            self.send(op, Job::Open(slots), now, out);
            return;
        }
        self.send_poll(now, out);
    }

    fn send_poll(&mut self, now: Monotonic, out: &mut Step) {
        let kind = POLL.get(self.poll_step % POLL.len()).copied();
        self.poll_step = self.poll_step.wrapping_add(1);
        let op = match kind {
            Some(SaturnOpKind::ReadFaults) => SaturnOp::ReadFaults,
            Some(SaturnOpKind::ReadOutlets) => SaturnOp::ReadOutlets,
            _ => SaturnOp::ReadTemperature,
        };
        self.send(op, Job::Poll, now, out);
    }

    // ---- discovery ------------------------------------------------------

    fn begin_discovery(&mut self, now: Monotonic, kernel: &mut SafetyKernel, out: &mut Step) {
        self.phase = ZonePhase::Discovery;
        self.scan_next = kdtv_proto::saturn::VALVE_ADDR_MIN;
        self.found.clear();
        out.notes.push(Note::Platform {
            what: PlatformEvent::ServiceStarted,
            detail: format!("{}: probing 0x03..=0x07 for exactly one valve", self.link),
        });
        self.next_probe(now, kernel, out);
    }

    fn next_probe(&mut self, now: Monotonic, kernel: &mut SafetyKernel, out: &mut Step) {
        while self.scan_next <= kdtv_proto::saturn::VALVE_ADDR_MAX {
            let addr = ValveAddr::new(self.scan_next);
            self.scan_next = self.scan_next.saturating_add(1);
            if let Ok(addr) = addr {
                self.send(SaturnOp::ReadFirmwareType, Job::Probe(addr), now, out);
                return;
            }
        }
        self.finish_discovery(now, kernel, out);
    }

    fn finish_discovery(&mut self, now: Monotonic, kernel: &mut SafetyKernel, out: &mut Step) {
        match self.found.len() {
            0 => {
                out.effects.push(Effect::OperatorMessage {
                    link: self.link,
                    text: NO_VALVE,
                });
                let missed = SafetyEvent::SafetyResponseMissed {
                    zone: self.zone,
                    op: "Discovery".to_owned(),
                };
                self.escalate(&missed, now, kernel, out);
            }
            1 => {
                self.address = self.found.first().copied();
                self.phase = ZonePhase::Identify;
                self.identify_step = 0;
                self.next_identify(now, out);
            }
            _ => {
                // A silent "use the first one" would hide a mis-cabled bus.
                // CORRECTIONS.md item 10.
                out.effects.push(Effect::OperatorMessage {
                    link: self.link,
                    text: TOO_MANY_VALVES,
                });
                let conflict = SafetyEvent::OutOfRangeValue {
                    zone: self.zone,
                    // `kdtv-safety`'s vocabulary has no address-conflict variant.
                    // The number of valves answering a single-valve bus is a
                    // value read off the wire that is outside what the
                    // configuration permits, so it is reported as one rather
                    // than as a malformed frame — nothing here was malformed.
                    field: "discovered_valve_count",
                };
                self.escalate(&conflict, now, kernel, out);
            }
        }
    }

    fn record_probe_answer(
        &mut self,
        addr: ValveAddr,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut Step,
    ) {
        if !self.found.contains(&addr) {
            self.found.push(addr);
        }
        // The scan runs to the end of the range even after a valve answers:
        // stopping early is exactly what would hide the second one.
        self.next_probe(now, kernel, out);
    }

    // ---- identify and confirm-off ---------------------------------------

    fn next_identify(&mut self, now: Monotonic, out: &mut Step) {
        let Some(kind) = IDENTIFY.get(self.identify_step).copied() else {
            self.phase = ZonePhase::ConfirmOff;
            self.stopping = None;
            self.send_all_off(now, out);
            return;
        };
        let step = self.identify_step;
        let op = match kind {
            SaturnOpKind::ReadFirmwareVersion => SaturnOp::ReadFirmwareVersion,
            SaturnOpKind::ReadSerialNumber => SaturnOp::ReadSerialNumber,
            SaturnOpKind::ReadCalibration => SaturnOp::ReadCalibration,
            SaturnOpKind::ReadConfiguration => SaturnOp::ReadConfiguration,
            SaturnOpKind::ReadOutlets => SaturnOp::ReadOutlets,
            SaturnOpKind::ReadTemperature => SaturnOp::ReadTemperature,
            SaturnOpKind::ReadFaults => SaturnOp::ReadFaults,
            _ => SaturnOp::ReadFirmwareType,
        };
        self.send(op, Job::Identify(step), now, out);
    }

    fn send_all_off(&mut self, now: Monotonic, out: &mut Step) {
        self.send(SaturnOp::AllOff, Job::ConfirmOff, now, out);
    }

    // ---- transmission bookkeeping ---------------------------------------

    fn send(&mut self, op: SaturnOp, job: Job, now: Monotonic, out: &mut Step) {
        let target = match job {
            Job::Probe(addr) => addr,
            _ => self.address.unwrap_or(ValveAddr::ALL[0]),
        };
        self.inflight = Some(InFlight {
            op: op.clone(),
            job,
            target,
            started_at: now,
            sent_at: now,
            attempts: 1,
            awaiting_retry: false,
        });
        out.tx = Some(op);
        out.target = Some(target);
    }

    /// Give up on the outstanding transaction, remembering that one late
    /// response may still arrive for it.
    fn abandon(&mut self) {
        if self.inflight.take().is_some() {
            self.stale = self.stale.saturating_add(1);
        }
    }

    // ---- responses ------------------------------------------------------

    fn on_response(
        &mut self,
        frame: &DecodedFrame,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut Step,
    ) {
        let Some(f) = self.inflight.clone() else {
            if self.stale > 0 {
                self.stale -= 1;
                return;
            }
            // Nothing was outstanding and nothing was abandoned. A duplicate, or
            // a frame from a device that was not asked. Either way the bus is not
            // behaving as a strictly serialised one, and nothing correlates this
            // frame with a request.
            let bad = SafetyEvent::MalformedResponse {
                zone: self.zone,
                detail: format!(
                    "unsolicited or duplicate response, control 0x{:02X}",
                    frame.control.0
                ),
            };
            self.escalate(&bad, now, kernel, out);
            return;
        };

        // A response that arrives after its own deadline is not an answer to
        // anything: the retry train may already have re-sent the request, so the
        // frame cannot be correlated with either send.
        if let Some(deadline) = f.sent_at.checked_add(self.settings.timings.response)
            && now > deadline
        {
            self.inflight = None;
            let late = SafetyEvent::MalformedResponse {
                zone: self.zone,
                detail: format!(
                    "response {} ms late for {:?}",
                    now.since(deadline).as_millis(),
                    f.op.kind()
                ),
            };
            self.escalate(&late, now, kernel, out);
            return;
        }

        self.inflight = None;
        out.notes.push(Note::Transaction {
            link: self.link,
            op: op_name(f.op.kind()),
            latency: now.since(f.sent_at),
            attempts: u32::from(f.attempts),
        });

        match frame.response() {
            ValveControl::Error => self.on_error_response(frame, f.job, now, kernel, out),
            ValveControl::Nak => {
                let nak = SafetyEvent::SafetyResponseMissed {
                    zone: self.zone,
                    op: format!("{:?} (NAK)", f.op.kind()),
                };
                self.escalate(&nak, now, kernel, out);
            }
            ValveControl::Echo(b) if b == f.op.kind().control_byte() => {
                self.on_echo(frame, f.job, now, kernel, out);
            }
            ValveControl::Echo(b) => {
                let bad = SafetyEvent::MalformedResponse {
                    zone: self.zone,
                    detail: format!(
                        "control 0x{b:02X} is not the echo of 0x{:02X}",
                        f.op.kind().control_byte()
                    ),
                };
                self.escalate(&bad, now, kernel, out);
            }
        }
    }

    fn on_error_response(
        &mut self,
        frame: &DecodedFrame,
        job: Job,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut Step,
    ) {
        // During a probe an error response still means something answered, so
        // the address is occupied — which is what the conflict check counts.
        if let Job::Probe(addr) = job {
            self.record_probe_answer(addr, now, kernel, out);
            return;
        }
        let raw = frame.error_byte().unwrap_or(RawErrorByte(0xFF));
        let disposition = raw.disposition(self.settings.error_table);
        // An error byte establishes nothing about health under either table.
        self.cache.health = Health::Unknown;
        let fault = SafetyEvent::ValveFault {
            zone: self.zone,
            raw_code: raw.0,
            unrecoverable: !disposition.is_retryable(),
        };
        self.escalate(&fault, now, kernel, out);
    }

    fn on_echo(
        &mut self,
        frame: &DecodedFrame,
        job: Job,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut Step,
    ) {
        match job {
            Job::Probe(addr) => self.record_probe_answer(addr, now, kernel, out),
            Job::Identify(step) => {
                if let Some(event) = self.absorb(frame, step) {
                    self.escalate(&event, now, kernel, out);
                    return;
                }
                self.identify_step = step.saturating_add(1);
                self.next_identify(now, out);
            }
            Job::ConfirmOff => self.on_off_confirmed(now, kernel, out),
            Job::Poll => {
                if let Some(event) = self.absorb_poll(frame) {
                    self.escalate(&event, now, kernel, out);
                }
            }
            Job::SetTemperature => {}
            Job::Open(slots) => {
                self.cache.outlets = slots;
                if let Some(plan) = self.stagger.as_mut() {
                    plan.advance();
                    if plan.finished() {
                        self.stagger = None;
                    }
                }
            }
            Job::Pause => {
                if let ZonePhase::Running { command, .. } = self.phase {
                    self.phase = ZonePhase::Paused {
                        since: now,
                        command,
                    };
                }
            }
            Job::Resume => {
                if let ZonePhase::Paused { command, .. } = self.phase {
                    self.phase = ZonePhase::Running {
                        since: now,
                        command,
                    };
                }
            }
        }
    }

    fn on_off_confirmed(&mut self, now: Monotonic, kernel: &mut SafetyKernel, out: &mut Step) {
        self.cache.outlets = SlotSet::EMPTY;
        if let Some(stop) = self.stopping.take() {
            let ran = self.session.map_or(Duration::ZERO, |s| s.elapsed(now));
            if let Some(command) = self.command.take() {
                out.notes.push(Note::Session {
                    link: self.link,
                    command,
                    duration: ran,
                    stop,
                });
            }
            self.session = None;
            self.grant = None;
            kernel.mark_stopped(self.zone);
        }
        match self.settings.purge {
            Purge::Enabled { duration } => {
                // The valve says off. Whether water has stopped is a different
                // question, and until the purge window closes the answer is no.
                let until = now.checked_add(duration).unwrap_or(now);
                self.phase = ZonePhase::Purging { since: now, until };
            }
            Purge::Disabled => {
                self.phase = ZonePhase::ReadyOff;
                kernel.mark_ready(self.link);
            }
        }
    }

    fn finish_purge(&mut self, kernel: &mut SafetyKernel) {
        self.phase = ZonePhase::ReadyOff;
        kernel.mark_ready(self.link);
    }

    // ---- reading payloads -----------------------------------------------

    /// Absorb an identity response. Returns the safety event it proves, if any.
    fn absorb(&mut self, frame: &DecodedFrame, step: usize) -> Option<SafetyEvent> {
        let kind = IDENTIFY.get(step).copied()?;
        let data = frame.data.as_slice();
        match kind {
            SaturnOpKind::ReadFirmwareType => {
                let byte = *data.first()?;
                self.cache.firmware_type = Some(byte);
                let ft = FirmwareTypeId(byte).classify();
                if ValveType::from_firmware(ft) == Some(self.expected_valve) {
                    None
                } else {
                    // Guessing a valve family picks the wrong outlet bitmap, and
                    // on this hardware that opens a different outlet than the
                    // operator asked for.
                    Some(SafetyEvent::OutOfRangeValue {
                        zone: self.zone,
                        field: "firmware_type",
                    })
                }
            }
            SaturnOpKind::ReadFirmwareVersion => {
                self.cache.firmware_version = Some(hex(data));
                None
            }
            SaturnOpKind::ReadSerialNumber => {
                self.cache.serial_number = Some(hex(data));
                None
            }
            SaturnOpKind::ReadCalibration => {
                self.cache.calibration = Some(hex(data));
                let baseline = self.settings.calibration_baseline.clone();
                self.check_baseline("calibration", data, baseline);
                None
            }
            SaturnOpKind::ReadConfiguration => {
                self.cache.configuration = Some(hex(data));
                let baseline = self.settings.configuration_baseline.clone();
                self.check_baseline("configuration", data, baseline);
                None
            }
            _ => self.absorb_poll(frame),
        }
    }

    /// Absorb an outlet, temperature or fault response.
    fn absorb_poll(&mut self, frame: &DecodedFrame) -> Option<SafetyEvent> {
        match frame.control {
            ControlByte(opcode::READ_OUTLET_STATES) => self.absorb_outlets(frame),
            ControlByte(opcode::READ_TEMPERATURE) => self.absorb_temperature(frame),
            ControlByte(opcode::READ_FAULT_FLAGS) => self.absorb_faults(frame),
            _ => None,
        }
    }

    fn absorb_outlets(&mut self, frame: &DecodedFrame) -> Option<SafetyEvent> {
        // `[?]` Which of the two payload bytes carries the bitmap, and in which
        // order, is not stated by any source. The first is read; a bit the table
        // does not know is refused rather than ignored.
        let bits = *frame.data.as_slice().first()?;
        let (slots, extra) = self.outlets.slots_from_bits(bits);
        if extra == 0 {
            self.cache.outlets = slots;
            None
        } else {
            Some(SafetyEvent::OutOfRangeValue {
                zone: self.zone,
                field: "outlet_bitmap",
            })
        }
    }

    fn absorb_temperature(&mut self, frame: &DecodedFrame) -> Option<SafetyEvent> {
        let raw = *frame.data.as_slice().first()?;
        if raw > Cx2::MAX_WATER_TEMP.raw() {
            self.cache.valve_reported_c = None;
            return Some(SafetyEvent::OutOfRangeValue {
                zone: self.zone,
                field: "valve_reported_temperature",
            });
        }
        self.cache.valve_reported_c = Some(Cx2::from_raw(raw).celsius());
        None
    }

    fn absorb_faults(&mut self, frame: &DecodedFrame) -> Option<SafetyEvent> {
        let (be, le) = frame.fault_bitmaps()?;
        // Both byte orders, because the endianness is unresolved and either
        // reading being non-zero is a fault. `ERR-07` / `RESP-05`.
        if be.is_clear() && le.is_clear() {
            self.cache.faults = Some(0);
            self.cache.health = Health::Clear;
            return None;
        }
        let bits = if be.is_clear() { le.0 } else { be.0 };
        self.cache.faults = Some(bits);
        self.cache.health = Health::Faulted { bits };
        Some(SafetyEvent::ValveFault {
            zone: self.zone,
            // The safety vocabulary carries one byte and the bitmap is two. The
            // low byte travels as the code; the full sixteen bits are in the
            // cache, so a later capture can retro-classify what this build
            // could not.
            raw_code: u8::try_from(bits & 0xFF).unwrap_or(0xFF),
            unrecoverable: false,
        })
    }

    fn check_baseline(&mut self, field: &'static str, observed: &[u8], baseline: Option<Vec<u8>>) {
        let Some(baseline) = baseline else { return };
        if baseline.as_slice() != observed {
            self.cache.baseline_drift.push(BaselineDrift {
                field,
                baseline: hex(&baseline),
                observed: hex(observed),
            });
        }
    }

    // ---- failures -------------------------------------------------------

    fn on_timeout(&mut self, now: Monotonic, kernel: &mut SafetyKernel, out: &mut Step) {
        let Some(f) = self.inflight.clone() else {
            return;
        };
        // A silent address is the expected answer at four of the five addresses,
        // so a probe timeout advances the scan rather than declaring a fault.
        if let Job::Probe(_) = f.job {
            self.inflight = None;
            self.next_probe(now, kernel, out);
            return;
        }

        let op = f.op.kind();
        if now.since(f.started_at) >= self.settings.retry.ceiling {
            self.inflight = None;
            out.notes.push(Note::Platform {
                what: PlatformEvent::SerialError,
                detail: format!(
                    "{}: {op:?} overran the {} ms transaction budget after {} attempts",
                    self.link,
                    self.settings.retry.ceiling.as_millis(),
                    f.attempts
                ),
            });
            self.give_up(
                f.job,
                format!("{op:?} (transaction budget)"),
                now,
                kernel,
                out,
            );
            return;
        }
        if f.attempts >= self.settings.retry.attempts {
            self.inflight = None;
            self.give_up(f.job, format!("{op:?}"), now, kernel, out);
            return;
        }
        if let Some(inflight) = self.inflight.as_mut() {
            inflight.awaiting_retry = true;
        }
    }

    fn give_up(
        &mut self,
        job: Job,
        op: String,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut Step,
    ) {
        if matches!(job, Job::ConfirmOff) {
            // BOOT-08: a valve that will not confirm itself off is beyond
            // anything this controller can do about it.
            out.effects.push(Effect::OperatorMessage {
                link: self.link,
                text: CANNOT_CONFIRM_OFF,
            });
        }
        let missed = SafetyEvent::SafetyResponseMissed {
            zone: self.zone,
            op,
        };
        self.escalate(&missed, now, kernel, out);
    }

    fn on_decode_failed(
        &mut self,
        why: DecodeError,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut Step,
    ) {
        let outstanding = self.inflight.take();
        // A probe that answers with a broken frame still means something is at
        // that address. Counting it as an answer is the fail-safe direction: the
        // refusal is what a two-valve bus must produce.
        if let Some(f) = outstanding.as_ref()
            && let Job::Probe(addr) = f.job
        {
            out.notes.push(Note::Platform {
                what: PlatformEvent::SerialError,
                detail: format!(
                    "{}: the probe of 0x{:02X} answered malformed: {why}",
                    self.link,
                    addr.get()
                ),
            });
            self.record_probe_answer(addr, now, kernel, out);
            return;
        }

        let was_write = outstanding.as_ref().is_some_and(|f| f.op.kind().is_write());
        let event = if matches!(why, DecodeError::BadChecksum { .. }) && was_write {
            // A write whose acknowledgement did not checksum is its own event:
            // this service does not know whether the valve acted on it.
            SafetyEvent::ChecksumFailedOnWrite { zone: self.zone }
        } else {
            SafetyEvent::MalformedResponse {
                zone: self.zone,
                detail: why.to_string(),
            }
        };
        self.escalate(&event, now, kernel, out);
    }

    // ---- commands -------------------------------------------------------

    // The grant is taken by value because it is spent by this call: taking it by
    // reference would let one authorisation open water twice. It is stored, so
    // clippy has nothing to complain about here — but the reason it is stored is
    // that property, not convenience.
    fn on_start(&mut self, req: &StartRequest, grant: OpenGrant, now: Monotonic, out: &mut Step) {
        if let Some(refusal) = self.refuse_start(req, &grant) {
            out.notes.push(Note::Rejected {
                command: req.command,
                reason: refusal.to_string(),
                check: "zone machine start",
            });
            out.refused = Some(refusal);
            return;
        }
        self.grant = Some(grant);
        self.command = Some(req.command);
        self.session = Some(SessionDeadline::start(
            now,
            req.duration,
            self.settings.session_cap,
        ));
        self.phase = ZonePhase::Running {
            since: now,
            command: req.command,
        };
        self.stagger = Some(StaggerPlan::build(
            now,
            self.settings.timings.stagger,
            req.outlets,
        ));
        out.notes.push(Note::Accepted {
            command: req.command,
            requested: format!(
                "start {} outlets {:?} at {:.1} C for {} s",
                self.zone,
                req.outlets,
                req.temperature.celsius(),
                req.duration.get().as_secs()
            ),
        });
        // The setpoint is written first and the first outlet opens one stagger
        // interval later, so the mixing valve is at temperature before flow.
        self.send(
            SaturnOp::SetTemperature(req.temperature),
            Job::SetTemperature,
            now,
            out,
        );
    }

    fn refuse_start(&self, req: &StartRequest, grant: &OpenGrant) -> Option<Refusal> {
        if grant.zone() != self.zone {
            return Some(Refusal::GrantForAnotherZone {
                authorised: grant.zone(),
                zone: self.zone,
            });
        }
        if !matches!(self.phase, ZonePhase::ReadyOff) {
            return Some(Refusal::NotReadyOff {
                phase: self.phase.kind(),
            });
        }
        if self.cache.health != Health::Clear {
            return Some(Refusal::HealthNotEstablished);
        }
        if req.outlets.is_empty() {
            return Some(Refusal::NoOutlets);
        }
        let configured = self.outlets.configured_slots();
        if let Some(bad) = req.outlets.difference(configured).iter().next() {
            return Some(Refusal::UnconfiguredOutlet(bad));
        }
        None
    }

    fn on_command(&mut self, cmd: &OperatorCommand, now: Monotonic, out: &mut Step) {
        if let Some(refusal) = self.refuse_command(cmd) {
            out.notes.push(Note::Rejected {
                command: cmd.id(),
                reason: refusal.to_string(),
                check: "zone machine command",
            });
            out.refused = Some(refusal);
            return;
        }

        // One in flight at a time: an operator command replaces whatever poll
        // was outstanding rather than racing it, and the abandoned poll's
        // response is expected and ignored once.
        self.abandon();
        match cmd {
            OperatorCommand::Stop { .. } => {
                self.stagger = None;
                self.stopping = Some(StopReason::Commanded);
                self.phase = ZonePhase::ConfirmOff;
                self.send_all_off(now, out);
            }
            OperatorCommand::Pause { .. } => self.send(SaturnOp::Pause, Job::Pause, now, out),
            OperatorCommand::Resume { .. } => self.send(SaturnOp::Resume, Job::Resume, now, out),
            OperatorCommand::SetTemperature { temp, .. } => {
                self.send(
                    SaturnOp::SetTemperature(*temp),
                    Job::SetTemperature,
                    now,
                    out,
                );
            }
        }
    }

    fn refuse_command(&self, cmd: &OperatorCommand) -> Option<Refusal> {
        let running = matches!(self.phase, ZonePhase::Running { .. });
        let paused = matches!(self.phase, ZonePhase::Paused { .. });
        let latched = matches!(self.phase, ZonePhase::Unavailable { .. });
        let ok = match cmd {
            // Stopping is accepted in every phase but a latched one, where the
            // port has already been closed and there is nothing to send it on.
            // Closing a valve is otherwise always allowed.
            OperatorCommand::Stop { .. } => !latched,
            OperatorCommand::Pause { .. } => running,
            OperatorCommand::Resume { .. } => paused,
            OperatorCommand::SetTemperature { .. } => running || paused,
        };
        (!ok).then(|| Refusal::WrongPhase {
            command: cmd.name(),
            phase: self.phase.kind(),
        })
    }

    fn on_acknowledged(&mut self) {
        if let ZonePhase::Unavailable { reason, .. } = &self.phase {
            self.phase = ZonePhase::Unavailable {
                reason: reason.clone(),
                acknowledged: true,
            };
        }
    }

    // ---- escalation -----------------------------------------------------

    /// Hand one observation to the kernel and apply what comes back.
    ///
    /// **No escalation decision is made here.** The scope, the latch reason and
    /// the effects are the kernel's, matched exhaustively there; this function
    /// moves the machine to match them and passes them out unaltered.
    fn escalate(
        &mut self,
        event: &SafetyEvent,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut Step,
    ) {
        let effects = kernel.on_event(event, now);
        out.notes.push(Note::Safety {
            link: self.link,
            trigger: format!("{event:?}"),
            effects: effects.iter().map(|e| format!("{e:?}")).collect(),
        });
        let stop = if event.is_routine() {
            StopReason::SessionLimit
        } else {
            StopReason::Safety {
                event: format!("{event:?}"),
            }
        };
        self.apply(&effects, &stop, now, out);
        out.effects.extend(effects);
    }

    fn apply(&mut self, effects: &[Effect], stop: &StopReason, now: Monotonic, out: &mut Step) {
        for effect in effects {
            match effect {
                // The kernel names every zone a shared fault takes down; this
                // machine applies only the one it drives. That scoping is what
                // keeps one bad cable out of the other bathroom.
                Effect::AllOff(zone) if *zone == self.zone => {
                    self.abandon();
                    self.stagger = None;
                    self.stopping = Some(stop.clone());
                    self.phase = ZonePhase::ConfirmOff;
                    self.send_all_off(now, out);
                }
                Effect::Latch { link, reason } if *link == self.link => {
                    self.latch(reason.clone(), now, out);
                }
                _ => {}
            }
        }
    }

    fn latch(&mut self, reason: LatchReason, now: Monotonic, out: &mut Step) {
        if let (Some(command), Some(session)) = (self.command.take(), self.session) {
            out.notes.push(Note::Session {
                link: self.link,
                command,
                duration: session.elapsed(now),
                stop: StopReason::Safety {
                    event: format!("{reason:?}"),
                },
            });
        }
        self.inflight = None;
        self.stale = 0;
        self.stagger = None;
        self.session = None;
        self.grant = None;
        self.stopping = None;
        // Nothing is commanded open any more. What the valve is physically doing
        // is a question for the independent probe, not for this cache.
        self.cache.outlets = SlotSet::EMPTY;
        self.phase = ZonePhase::Unavailable {
            reason,
            acknowledged: false,
        };
    }

    // ---- housekeeping ---------------------------------------------------

    const fn purge_until(&self) -> Option<Monotonic> {
        match self.phase {
            ZonePhase::Purging { until, .. } => Some(until),
            _ => None,
        }
    }

    fn next_deadline(&self, now: Monotonic) -> Option<Monotonic> {
        if matches!(self.phase, ZonePhase::Unavailable { .. }) {
            return None;
        }
        let mut best = self.next_tick.unwrap_or(now);
        let mut consider = |t: Monotonic| {
            if t < best {
                best = t;
            }
        };
        if let Some(f) = self.inflight.as_ref()
            && !f.awaiting_retry
            && let Some(t) = f.sent_at.checked_add(self.settings.timings.response)
        {
            consider(t);
        }
        if let Some(t) = self.stagger.as_ref().and_then(StaggerPlan::next_due) {
            consider(t);
        }
        if let Some(s) = self.session.as_ref() {
            consider(s.expires_at());
        }
        if let Some(t) = self.purge_until() {
            consider(t);
        }
        Some(best)
    }

    fn refresh_cache(&mut self, now: Monotonic) {
        self.cache.phase = self.phase.kind();
        self.cache.valve_on = !self.cache.outlets.is_empty();
        self.cache.water_moving =
            self.cache.valve_on || matches!(self.phase, ZonePhase::Purging { .. });
        self.cache.session_remaining_s = self.session.map(|s| s.remaining(now).as_secs());
        self.cache.attempts = self.inflight.as_ref().map_or(0, |f| u32::from(f.attempts));
        self.cache.as_of = Some(now);
    }
}

/// What can happen to a zone.
///
/// **There is no `RefreshTimer`.** No variant of this enum, and no path in this
/// file, emits an operation whose purpose is to extend a session. `SESS-02`.
#[derive(Debug)]
pub enum ZoneEvent {
    /// The poll cadence, or a deadline the last [`Step`] asked for.
    Tick,
    /// A frame that decoded.
    Response(DecodedFrame),
    /// A frame that did not.
    DecodeFailed(DecodeError),
    /// The outstanding request's response deadline passed.
    ResponseTimeout,
    /// **The only way into [`ZonePhase::Running`].** Carries the grant the
    /// safety kernel minted, which is the workspace's only permission to open
    /// water.
    Start(StartRequest, OpenGrant),
    Command(OperatorCommand),
    /// Something the service observed — an RTD alarm, a lost port, a watchdog
    /// miss — that the kernel must rule on.
    Safety(SafetyEvent),
    /// The port went away.
    PortClosed,
    /// An operator acknowledged the latch. This does **not** reopen anything:
    /// the port closed when the zone latched, and coming back means going
    /// through discovery again.
    Acknowledged,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        let _ = write!(s, "{b:02X}");
    }
    s
}

const fn op_name(kind: SaturnOpKind) -> &'static str {
    match kind {
        SaturnOpKind::AllOff => "AllOff",
        SaturnOpKind::SetOutlets => "SetOutlets",
        SaturnOpKind::SetTemperature => "SetTemperature",
        SaturnOpKind::Pause => "Pause",
        SaturnOpKind::Resume => "Resume",
        SaturnOpKind::ReadFirmwareVersion => "ReadFirmwareVersion",
        SaturnOpKind::ReadFirmwareType => "ReadFirmwareType",
        SaturnOpKind::ReadOutlets => "ReadOutlets",
        SaturnOpKind::ReadTemperature => "ReadTemperature",
        SaturnOpKind::ReadFlow => "ReadFlow",
        SaturnOpKind::ReadFaults => "ReadFaults",
        SaturnOpKind::ReadCalibration => "ReadCalibration",
        SaturnOpKind::ReadConfiguration => "ReadConfiguration",
        SaturnOpKind::ReadSerialNumber => "ReadSerialNumber",
        SaturnOpKind::ReadGenericOutlets => "ReadGenericOutlets",
        SaturnOpKind::ReadExtendedStatus => "ReadExtendedStatus",
        SaturnOpKind::ReadDiagnostics => "ReadDiagnostics",
        SaturnOpKind::AddressEnquiry => "AddressEnquiry",
        SaturnOpKind::AddressAllocate => "AddressAllocate",
        SaturnOpKind::AddressClear => "AddressClear",
    }
}
