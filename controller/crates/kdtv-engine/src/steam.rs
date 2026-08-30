//! The DTV+ steam link state machine.
//!
//! Sans-IO, on the same terms as [`crate::zone`]: an event and a [`Monotonic`]
//! go in, a [`SteamStep`] comes out, and the service transmits.
//!
//! # The path to steam
//!
//! ```text
//! Cold -> Discovery -> ReadyOff -> Running
//! ```
//!
//! Discovery broadcasts `DEV_ADDRESS_OPP`, reads the device ID out of the
//! device's `DEV_REQUEST_ADDR` answer, checks it is the steam generator, and
//! assigns an address from `0x03..=0x07`. **The device ID is not the address.**
//! `0x05` is the steam generator's device ID and also a perfectly valid assigned
//! address, which is exactly why `kdtv-proto` gives them two types with no
//! conversion between them and this file never builds one from the other.
//! `CORRECTIONS.md` item 2.
//!
//! A generator found already running at boot is commanded **off** before the
//! link is declared ready. Boot state is off, and no prior state is restored.
//!
//! # Degraded, versus lost
//!
//! The distinction is the whole reason this machine has a
//! [`SteamPhase::StoppingBeforeLatch`]:
//!
//! - **Degraded but alive.** Timeouts, a NAK, checksum failures, or the
//!   generator's own error flags. Transmission still works, so a stop is
//!   commanded and its acknowledgement is **required** before the link is given
//!   up. `kdtv-safety` returns [`Effect::SteamStopThenLatch`] for exactly this,
//!   and this machine drives that ordering rather than reimplementing the
//!   decision.
//! - **Lost port.** Nothing can be told anything, so the link latches directly.
//!
//! Every retry of the stop is a stop, so the last frame the generator can have
//! received on a dying link is one that turns it off.
//!
//! # The error bitmask
//!
//! Every `GET_DEV_STATUS` reply carries one, and the four documented bits are
//! independent and can be set together. Thermistor, generator-link
//! communication, overtemperature and safety-circuit all reach the kernel as
//! [`SafetyEvent::SteamLinkDegraded`] with [`DegradeReason::GeneratorFault`], so
//! all four stop the generator before latching. Undocumented bits are retained
//! by `kdtv-proto` and are a fault here too — an error byte nobody documented is
//! not evidence of health. `CORRECTIONS.md` item 9, `STEAM-14`.
//!
//! # The generator's own timer
//!
//! [`SteamMinutes`] is 1..=20 with no zero, because `steamTimerSetTime = 0`
//! disables the generator's automatic shutoff — the only backstop that survives
//! this service dying. Every write carries a non-zero duration, and there is no
//! path here that can write one that does not.
//!
//! # Power clean is denied in the payload, not the opcode
//!
//! `0xCC` is a value of the operation-state byte inside an allowlisted command,
//! so no missing opcode denies it. The denial is
//! [`kdtv_proto::dtv::SteamOpState`] having two variants, and this machine never
//! sources a state byte from anywhere else. `CORRECTIONS.md` item 1.

use crate::budget::RetryBudget;
use crate::note::Note;
use kdtv_config::SteamConfig;
use kdtv_proto::dtv::{
    DecodedDtv, DevAddr, DeviceId, DiscoveryStep, DtvDecodeError, DtvEncodeDenied, DtvFrame,
    DtvTimings, SteamEncoder, SteamErrorFlags, SteamOp, SteamOpKind, SteamOpState, SteamStateByte,
    SteamStatus, opcode,
};
use kdtv_proto::saturn::{DiscoveryToken, LinkPhase};
use kdtv_safety::{DegradeReason, Effect, LatchReason, SafetyEvent, SafetyKernel, SessionDeadline};
use kdtv_telemetry::{Monotonic, PlatformEvent, StopReason};
use kdtv_units::{CommandId, LinkKind, SessionDuration, SteamMinutes, SteamSetpoint};
use serde::Serialize;
use smallvec::SmallVec;

/// Where the steam link is.
///
/// Not `Eq`, for the same reason [`crate::zone::ZonePhase`] is not.
#[derive(Clone, PartialEq, Debug)]
pub enum SteamPhase {
    /// The entry state on every boot, restart and watchdog reset.
    Cold,
    /// Address opportunity broadcast, device ID checked, address assigned.
    Discovery,
    /// Enrolled and confirmed off.
    ReadyOff,
    /// The generator is producing steam.
    Running {
        since: Monotonic,
        command: CommandId,
    },
    /// A stop has been commanded and its acknowledgement is required **before**
    /// the link is latched. An explicit sub-state, so the ordering is asserted
    /// rather than incidental. `STEAM-18`.
    StoppingBeforeLatch {
        since: Monotonic,
        attempts: u8,
        reason: LatchReason,
    },
    /// Unavailable until a person acknowledges it.
    Unavailable {
        reason: LatchReason,
        acknowledged: bool,
    },
}

impl SteamPhase {
    #[must_use]
    pub const fn kind(&self) -> SteamPhaseKind {
        match self {
            Self::Cold => SteamPhaseKind::Cold,
            Self::Discovery => SteamPhaseKind::Discovery,
            Self::ReadyOff => SteamPhaseKind::ReadyOff,
            Self::Running { .. } => SteamPhaseKind::Running,
            Self::StoppingBeforeLatch { .. } => SteamPhaseKind::StoppingBeforeLatch,
            Self::Unavailable { .. } => SteamPhaseKind::Unavailable,
        }
    }

    /// The encoder phase this maps to.
    ///
    /// `StoppingBeforeLatch` maps to [`LinkPhase::Faulted`], where the encoder
    /// permits reads and — by its own exemption — a stop, and nothing else. A
    /// start is unencodable there.
    #[must_use]
    pub const fn link_phase(&self) -> LinkPhase {
        match self {
            Self::Cold => LinkPhase::Booting,
            Self::Discovery => LinkPhase::Discovery,
            Self::ReadyOff => LinkPhase::ReadyOff,
            Self::Running { .. } => LinkPhase::Running,
            Self::StoppingBeforeLatch { .. } | Self::Unavailable { .. } => LinkPhase::Faulted,
        }
    }
}

/// [`SteamPhase`] without its payloads.
#[derive(Copy, Clone, PartialEq, Eq, Default, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SteamPhaseKind {
    #[default]
    Cold,
    Discovery,
    ReadyOff,
    Running,
    StoppingBeforeLatch,
    Unavailable,
}

impl std::fmt::Display for SteamPhaseKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cold => "cold",
            Self::Discovery => "discovery",
            Self::ReadyOff => "ready-off",
            Self::Running => "running",
            Self::StoppingBeforeLatch => "stopping-before-latch",
            Self::Unavailable => "unavailable",
        })
    }
}

/// The three-field parameter block, carried forward between writes.
///
/// The `SET_DEV_PARAM` payload states setpoint, operation state and duration
/// **atomically**, so a caller changing one has to supply the other two. Keeping
/// them here is what stops a partially-updated generator from reaching the bus.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct SteamParams {
    pub temp: SteamSetpoint,
    pub minutes: SteamMinutes,
    pub state: SteamOpState,
}

/// Everything about the steam link that is not the wire codec.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SteamSettings {
    pub timings: DtvTimings,
    pub retry: RetryBudget,
    pub session_cap: SessionDuration,
    /// The address discovery will assign. Configuration, because the assignment
    /// is this master's to make — and emphatically not the device ID.
    pub assign: DevAddr,
    /// The setpoint and duration the link starts life believing in, before any
    /// operator command. Never transmitted on its own: a write happens only when
    /// something asks for one.
    pub initial: SteamParams,
    /// How many times a stop is sent before the link latches without its
    /// acknowledgement.
    pub stop_attempts: u8,
}

impl SteamSettings {
    /// Defaults from a validated configuration.
    #[must_use]
    pub fn from_config(cfg: &SteamConfig, session_cap: SessionDuration) -> Self {
        Self::from_timings(cfg.timings(), session_cap)
    }

    /// Defaults from a timing set directly, for tests and for a bench rig.
    #[must_use]
    pub fn from_timings(timings: DtvTimings, session_cap: SessionDuration) -> Self {
        Self {
            timings,
            retry: RetryBudget::from_dtv(&timings),
            session_cap,
            assign: DevAddr::REFERENCE,
            initial: SteamParams {
                temp: SteamSetpoint::clamped(SteamSetpoint::FACTORY_DEFAULT).0,
                minutes: SteamMinutes::DEFAULT,
                state: SteamOpState::Off,
            },
            stop_attempts: timings.retries,
        }
    }
}

/// What the service reports about steam without touching the bus.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct SteamCache {
    pub phase: SteamPhaseKind,
    pub assigned_address: Option<u8>,
    pub actual_f: Option<f32>,
    pub desired_f: Option<f32>,
    pub timer_minutes: Option<u8>,
    /// **Not monotonic.** The device-side ticker persists across pause and
    /// resume, so a later reading can be larger than an earlier one. `STEAM-21`.
    pub timer_seconds: Option<u8>,
    /// The error byte verbatim, undocumented bits included.
    pub error_bits: Option<u8>,
    /// The undocumented bits that were set. Reported, never interpreted.
    pub reserved_bits: Option<u8>,
    pub steaming: bool,
    pub session_remaining_s: Option<u64>,
    /// True when the generator reported a setpoint outside this master's
    /// 90–125 °F policy, which means something other than this master wrote it.
    pub out_of_policy_setpoint: bool,
    pub as_of: Option<Monotonic>,
}

impl SteamCache {
    const fn new() -> Self {
        Self {
            phase: SteamPhaseKind::Cold,
            assigned_address: None,
            actual_f: None,
            desired_f: None,
            timer_minutes: None,
            timer_seconds: None,
            error_bits: None,
            reserved_bits: None,
            steaming: false,
            session_remaining_s: None,
            out_of_policy_setpoint: false,
            as_of: None,
        }
    }
}

/// What an operator can ask of the steam link.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SteamCommand {
    Start {
        temp: SteamSetpoint,
        minutes: SteamMinutes,
        command: CommandId,
    },
    Stop {
        command: CommandId,
    },
    SetTemperature {
        temp: SteamSetpoint,
        command: CommandId,
    },
    SetDuration {
        minutes: SteamMinutes,
        command: CommandId,
    },
}

impl SteamCommand {
    const fn id(&self) -> CommandId {
        match self {
            Self::Start { command, .. }
            | Self::Stop { command }
            | Self::SetTemperature { command, .. }
            | Self::SetDuration { command, .. } => *command,
        }
    }

    const fn name(&self) -> &'static str {
        match self {
            Self::Start { .. } => "Start",
            Self::Stop { .. } => "Stop",
            Self::SetTemperature { .. } => "SetTemperature",
            Self::SetDuration { .. } => "SetDuration",
        }
    }
}

/// Why the steam machine refused a command.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum SteamRefusal {
    #[error("{command} is not accepted while the steam link is {phase}")]
    WrongPhase {
        command: &'static str,
        phase: SteamPhaseKind,
    },
    #[error("the steam link has no assigned address yet")]
    NotEnrolled,
}

/// Everything the engine wants done as a result of one steam event.
///
/// The service transmits [`SteamStep::tx`] first, then performs
/// [`SteamStep::effects`] in order — the same contract as the zone machine.
///
/// When an [`Effect::SteamStopThenLatch`] appears, the stop it names is already
/// on `tx` and the phase is already [`SteamPhase::StoppingBeforeLatch`]. The
/// [`Effect::Latch`] beside it is the kernel's own bookkeeping, returned
/// unaltered; the machine reaches [`SteamPhase::Unavailable`] when the stop is
/// acknowledged, or when it has been retried its budget and cannot be got
/// through. The [`Effect::ClosePort`] that finishes the sequence is emitted on
/// that later step, not this one.
#[derive(Clone, Debug)]
pub struct SteamStep {
    /// The one operation to transmit. There is no variant of [`SteamOp`] that
    /// can carry a Saturn frame or open a valve outlet.
    pub tx: Option<SteamOp>,
    pub dest: Option<DevAddr>,
    pub phase: LinkPhase,
    pub effects: SmallVec<[Effect; 4]>,
    pub deadline: Option<Monotonic>,
    pub notes: SmallVec<[Note; 2]>,
    pub refused: Option<SteamRefusal>,
}

impl SteamStep {
    fn new(phase: LinkPhase) -> Self {
        Self {
            tx: None,
            dest: None,
            phase,
            effects: SmallVec::new(),
            deadline: None,
            notes: SmallVec::new(),
            refused: None,
        }
    }
}

/// What can happen to the steam link.
#[derive(Clone, Debug)]
pub enum SteamEvent {
    Tick,
    Response(DecodedDtv),
    DecodeFailed(DtvDecodeError),
    ResponseTimeout,
    Command(SteamCommand),
    /// Something the service observed that the kernel must rule on.
    Safety(SafetyEvent),
    PortClosed,
    Acknowledged,
}

/// What an in-flight steam transaction is for.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum SteamJob {
    AddressOpportunity,
    AssignAddress,
    /// The first status read after enrolment, which is what establishes whether
    /// the generator is already running.
    FirstStatus,
    Poll,
    Write,
    /// The stop that must be acknowledged before the link is given up.
    StopBeforeLatch,
}

#[derive(Clone, Debug)]
struct SteamInFlight {
    op: SteamOp,
    job: SteamJob,
    dest: DevAddr,
    started_at: Monotonic,
    sent_at: Monotonic,
    attempts: u8,
    awaiting_retry: bool,
}

/// The DTV+ steam link, as a state machine.
#[derive(Debug)]
pub struct SteamMachine {
    settings: SteamSettings,
    phase: SteamPhase,
    addr: Option<DevAddr>,
    params: SteamParams,
    inflight: Option<SteamInFlight>,
    /// Responses to transactions this machine gave up on, which may still be in
    /// the pipe.
    stale: u8,
    session: Option<SessionDeadline>,
    command: Option<CommandId>,
    /// Set while a stop write is outstanding, so its acknowledgement knows what
    /// to record and where to go next.
    stopping: Option<StopReason>,
    next_tick: Option<Monotonic>,
    cache: SteamCache,
}

/// What an operator must do when nothing on this port is the steam adapter.
const NO_ADAPTER: &str = "No K-1737-K1 steam adapter answered on the DTV+ port. Steam is \
                          unavailable until the cabling and the adapter are checked.";

impl SteamMachine {
    /// Always starts [`SteamPhase::Cold`].
    #[must_use]
    pub fn new(settings: SteamSettings) -> Self {
        let params = settings.initial;
        Self {
            settings,
            phase: SteamPhase::Cold,
            addr: None,
            params,
            inflight: None,
            stale: 0,
            session: None,
            command: None,
            stopping: None,
            next_tick: None,
            cache: SteamCache::new(),
        }
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        LinkKind::Steam
    }

    #[must_use]
    pub const fn phase(&self) -> &SteamPhase {
        &self.phase
    }

    #[must_use]
    pub const fn cached(&self) -> &SteamCache {
        &self.cache
    }

    #[must_use]
    pub const fn address(&self) -> Option<DevAddr> {
        self.addr
    }

    /// The parameter block the next write will carry forward.
    #[must_use]
    pub const fn params(&self) -> SteamParams {
        self.params
    }

    /// Encode a step's transmission.
    ///
    /// The [`DiscoveryToken`] is minted here from the step's own phase, so an
    /// address operation outside [`LinkPhase::Discovery`] is unspellable.
    pub fn encode(
        &self,
        enc: &SteamEncoder,
        step: &SteamStep,
    ) -> Option<Result<DtvFrame, DtvEncodeDenied>> {
        let op = step.tx.as_ref()?;
        let dest = step.dest.or(self.addr).unwrap_or(self.settings.assign);
        let token = DiscoveryToken::mint(LinkKind::Steam, step.phase);
        Some(enc.encode(dest, op, step.phase, token.as_ref()))
    }

    /// Advance the machine.
    pub fn step(&mut self, ev: SteamEvent, now: Monotonic, kernel: &mut SafetyKernel) -> SteamStep {
        let mut out = SteamStep::new(self.phase.link_phase());
        match ev {
            SteamEvent::Tick => self.on_tick(now, kernel, &mut out),
            SteamEvent::Response(frame) => self.on_response(&frame, now, kernel, &mut out),
            SteamEvent::DecodeFailed(why) => self.on_decode_failed(&why, now, kernel, &mut out),
            SteamEvent::ResponseTimeout => self.on_timeout(now, kernel, &mut out),
            SteamEvent::Command(cmd) => self.on_command(&cmd, now, &mut out),
            SteamEvent::Safety(event) => self.escalate(&event, now, kernel, &mut out),
            SteamEvent::PortClosed => {
                let lost = SafetyEvent::PortLost {
                    link: LinkKind::Steam,
                };
                self.escalate(&lost, now, kernel, &mut out);
            }
            SteamEvent::Acknowledged => self.on_acknowledged(),
        }
        out.phase = self.phase.link_phase();
        out.deadline = self.next_deadline(now);
        self.refresh_cache(now);
        out
    }

    // ---- ticks ----------------------------------------------------------

    fn on_tick(&mut self, now: Monotonic, kernel: &mut SafetyKernel, out: &mut SteamStep) {
        self.next_tick = now.checked_add(self.settings.timings.tick);
        if matches!(self.phase, SteamPhase::Unavailable { .. }) {
            return;
        }

        if let Some(session) = self.session
            && session.expired(now)
        {
            self.stop_session(StopReason::SessionLimit, now, out);
            return;
        }

        if let Some(f) = self.inflight.as_mut()
            && f.awaiting_retry
        {
            f.awaiting_retry = false;
            f.attempts = f.attempts.saturating_add(1);
            f.sent_at = now;
            out.tx = Some(f.op.clone());
            out.dest = Some(f.dest);
            return;
        }
        if self.inflight.is_some() {
            return;
        }

        let _ = kernel;
        match self.phase {
            SteamPhase::Cold => {
                self.phase = SteamPhase::Discovery;
                out.notes.push(Note::Platform {
                    what: PlatformEvent::ServiceStarted,
                    detail: "steam: broadcasting the address opportunity".to_owned(),
                });
                self.send_address_opportunity(now, out);
            }
            // Discovery re-broadcasts on each tick until a device answers; the
            // retry budget is what bounds it.
            SteamPhase::Discovery => self.send_address_opportunity(now, out),
            SteamPhase::ReadyOff | SteamPhase::Running { .. } => {
                let dest = self.dest();
                self.send(SteamOp::ReadStatus, SteamJob::Poll, dest, now, out);
            }
            SteamPhase::StoppingBeforeLatch { .. } => self.send_stop_before_latch(now, out),
            SteamPhase::Unavailable { .. } => {}
        }
    }

    fn send_address_opportunity(&mut self, now: Monotonic, out: &mut SteamStep) {
        let assign = self.settings.assign;
        self.send(
            SteamOp::Discovery(DiscoveryStep::AddressOpportunity),
            SteamJob::AddressOpportunity,
            assign,
            now,
            out,
        );
    }

    fn dest(&self) -> DevAddr {
        self.addr.unwrap_or(self.settings.assign)
    }

    fn send(
        &mut self,
        op: SteamOp,
        job: SteamJob,
        dest: DevAddr,
        now: Monotonic,
        out: &mut SteamStep,
    ) {
        self.inflight = Some(SteamInFlight {
            op: op.clone(),
            job,
            dest,
            started_at: now,
            sent_at: now,
            attempts: 1,
            awaiting_retry: false,
        });
        out.tx = Some(op);
        out.dest = Some(dest);
    }

    fn abandon(&mut self) {
        if self.inflight.take().is_some() {
            self.stale = self.stale.saturating_add(1);
        }
    }

    // ---- responses ------------------------------------------------------

    fn on_response(
        &mut self,
        frame: &DecodedDtv,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut SteamStep,
    ) {
        let Some(f) = self.inflight.clone() else {
            if self.stale > 0 {
                self.stale -= 1;
                return;
            }
            out.notes.push(Note::Platform {
                what: PlatformEvent::SerialError,
                detail: format!(
                    "steam: unsolicited or duplicate frame, cmd 0x{:02X}",
                    frame.cmd
                ),
            });
            // A frame this master cannot account for is a framing-level problem
            // on a link that is otherwise alive, so it degrades rather than
            // latching outright. `DegradeReason` has no closer name for it.
            let degraded = SafetyEvent::SteamLinkDegraded {
                why: DegradeReason::ChecksumFailures,
            };
            self.escalate(&degraded, now, kernel, out);
            return;
        };

        // A NAK is a rejected command, not a transient failure: re-sending the
        // same values would just be rejected again. `STEAM-09`.
        if frame.cmd == opcode::DEV_NAK {
            self.inflight = None;
            out.notes.push(Note::Platform {
                what: PlatformEvent::SerialError,
                detail: format!(
                    "steam: NAK, error byte {:?}, for {:?}",
                    frame.nak_error_byte(),
                    f.op.kind()
                ),
            });
            let degraded = SafetyEvent::SteamLinkDegraded {
                why: DegradeReason::Nak,
            };
            self.escalate(&degraded, now, kernel, out);
            return;
        }

        self.inflight = None;
        out.notes.push(Note::Transaction {
            link: LinkKind::Steam,
            op: op_name(f.op.kind()),
            latency: now.since(f.sent_at),
            attempts: u32::from(f.attempts),
        });

        match f.job {
            SteamJob::AddressOpportunity => self.on_address_request(frame, now, kernel, out),
            SteamJob::AssignAddress => self.on_address_assigned(frame, now, kernel, out),
            SteamJob::FirstStatus => self.on_first_status(frame, now, kernel, out),
            SteamJob::Poll => {
                let _ = self.absorb_status(frame, now, kernel, out);
            }
            SteamJob::Write => self.on_write_acknowledged(frame, now, kernel, out),
            SteamJob::StopBeforeLatch => self.on_stop_acknowledged(now, out),
        }
    }

    fn on_address_request(
        &mut self,
        frame: &DecodedDtv,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut SteamStep,
    ) {
        let Some(id) = frame.requested_device_id() else {
            // Something answered the broadcast that is not asking for an
            // address. Discovery routes on opcode, never on address, so there is
            // nothing else this can be read as.
            self.refuse_adapter(
                &format!("cmd 0x{:02X} is not DEV_REQUEST_ADDR", frame.cmd),
                now,
                kernel,
                out,
            );
            return;
        };
        if id != DeviceId::STEAM_GENERATOR {
            self.refuse_adapter(
                &format!("device ID 0x{:02X} is not the steam generator", id.get()),
                now,
                kernel,
                out,
            );
            return;
        }
        // The address being assigned is configuration. It is emphatically not
        // derived from the device ID: there is no conversion between the two
        // types, so the source's own `88 05 00 30` example — which puts the
        // device ID in DEST — is unreachable unless discovery assigned 0x05.
        let assign = self.settings.assign;
        self.send(
            SteamOp::Discovery(DiscoveryStep::AssignAddress(assign)),
            SteamJob::AssignAddress,
            assign,
            now,
            out,
        );
    }

    fn on_address_assigned(
        &mut self,
        frame: &DecodedDtv,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut SteamStep,
    ) {
        if frame.cmd == opcode::DEV_ACK {
            self.addr = Some(self.settings.assign);
            let dest = self.dest();
            self.send(SteamOp::ReadStatus, SteamJob::FirstStatus, dest, now, out);
        } else {
            self.refuse_adapter(
                &format!("cmd 0x{:02X} did not acknowledge the assignment", frame.cmd),
                now,
                kernel,
                out,
            );
        }
    }

    /// The first status read after enrolment. A generator found already running
    /// is stopped before the link is called ready — boot state is off.
    fn on_first_status(
        &mut self,
        frame: &DecodedDtv,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut SteamStep,
    ) {
        let Some(status) = self.absorb_status(frame, now, kernel, out) else {
            return;
        };
        if status.state.is_producing() {
            out.notes.push(Note::Platform {
                what: PlatformEvent::ServiceStarted,
                detail: "steam: the generator was already producing at boot; commanding off"
                    .to_owned(),
            });
            self.params.state = SteamOpState::Off;
            self.stopping = Some(StopReason::ServiceStopping);
            let op = self.stop_op();
            let dest = self.dest();
            self.send(op, SteamJob::Write, dest, now, out);
            return;
        }
        self.phase = SteamPhase::ReadyOff;
        kernel.mark_ready(LinkKind::Steam);
    }

    fn on_write_acknowledged(
        &mut self,
        frame: &DecodedDtv,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut SteamStep,
    ) {
        // A write is acknowledged either bare or with a status payload; a status
        // is absorbed if one is present.
        if frame.steam_status(self.dest()).is_ok()
            && self.absorb_status(frame, now, kernel, out).is_none()
        {
            return;
        }
        if let Some(stop) = self.stopping.take() {
            if let (Some(command), Some(session)) = (self.command.take(), self.session) {
                out.notes.push(Note::Session {
                    link: LinkKind::Steam,
                    command,
                    duration: session.elapsed(now),
                    stop,
                });
            }
            self.session = None;
            self.phase = SteamPhase::ReadyOff;
            kernel.mark_ready(LinkKind::Steam);
        }
    }

    /// Decode a status payload, apply what it proves, and return it.
    ///
    /// Returns `None` when the frame did not carry a usable status, or when what
    /// it carried has already escalated.
    fn absorb_status(
        &mut self,
        frame: &DecodedDtv,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut SteamStep,
    ) -> Option<SteamStatus> {
        let (_carrier, status) = match frame.steam_status(self.dest()) {
            Ok(v) => v,
            Err(why) => {
                out.notes.push(Note::Platform {
                    what: PlatformEvent::SerialError,
                    detail: format!("steam: {why}"),
                });
                let degraded = SafetyEvent::SteamLinkDegraded {
                    why: DegradeReason::ChecksumFailures,
                };
                self.escalate(&degraded, now, kernel, out);
                return None;
            }
        };
        self.cache.actual_f = Some(status.actual.fahrenheit());
        self.cache.desired_f = Some(status.desired.fahrenheit());
        self.cache.timer_minutes = Some(status.timer_minutes);
        self.cache.timer_seconds = Some(status.timer_seconds);
        self.cache.error_bits = Some(status.errors.bits());
        self.cache.reserved_bits = Some(status.errors.reserved_bits_set());
        self.cache.steaming = status.state.is_producing();
        self.cache.out_of_policy_setpoint = !status.setpoint_is_in_policy();

        // A state byte this master cannot command — power clean, or a value no
        // source explains — is recorded and is not evidence of health.
        if matches!(status.state, SteamStateByte::Invalid(_)) {
            out.notes.push(Note::Platform {
                what: PlatformEvent::SerialError,
                detail: format!(
                    "steam: operation state 0x{:02X} is not a value this master can command",
                    status.state.raw()
                ),
            });
        }

        if status.errors.is_fault() {
            out.notes.push(Note::Platform {
                what: PlatformEvent::SerialError,
                detail: describe_errors(status.errors),
            });
            let degraded = SafetyEvent::SteamLinkDegraded {
                why: DegradeReason::GeneratorFault,
            };
            self.escalate(&degraded, now, kernel, out);
            return None;
        }
        Some(status)
    }

    // ---- failures -------------------------------------------------------

    fn on_timeout(&mut self, now: Monotonic, kernel: &mut SafetyKernel, out: &mut SteamStep) {
        let Some(f) = self.inflight.clone() else {
            return;
        };

        if matches!(f.job, SteamJob::StopBeforeLatch) {
            self.inflight = None;
            self.retry_stop_or_latch(now, out);
            return;
        }

        let over_budget = f.attempts >= self.settings.retry.attempts
            || now.since(f.started_at) >= self.settings.retry.ceiling;
        if !over_budget {
            if let Some(inflight) = self.inflight.as_mut() {
                inflight.awaiting_retry = true;
            }
            return;
        }

        self.inflight = None;
        if matches!(
            f.job,
            SteamJob::AddressOpportunity | SteamJob::AssignAddress
        ) {
            self.refuse_adapter("no device answered discovery", now, kernel, out);
            return;
        }
        out.notes.push(Note::Platform {
            what: PlatformEvent::SerialError,
            detail: format!(
                "steam: {:?} unanswered after {} attempts, {} ms",
                f.op.kind(),
                f.attempts,
                now.since(f.started_at).as_millis()
            ),
        });
        let degraded = SafetyEvent::SteamLinkDegraded {
            why: DegradeReason::Timeouts,
        };
        self.escalate(&degraded, now, kernel, out);
    }

    fn on_decode_failed(
        &mut self,
        why: &DtvDecodeError,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut SteamStep,
    ) {
        self.inflight = None;
        out.notes.push(Note::Platform {
            what: PlatformEvent::SerialError,
            detail: format!("steam: {why}"),
        });
        let degraded = SafetyEvent::SteamLinkDegraded {
            why: DegradeReason::ChecksumFailures,
        };
        self.escalate(&degraded, now, kernel, out);
    }

    /// Nothing on this port is the steam adapter.
    ///
    /// Reported as [`SafetyEvent::PortLost`] rather than as a degraded link:
    /// `kdtv-safety` has no "wrong device on this port" event, and a port with
    /// nothing addressable on it must **not** produce a stop-then-latch, because
    /// there is nothing to send a stop to. Adding a variant is that crate's
    /// decision, not this one's; the effect set here is the correct one either
    /// way.
    fn refuse_adapter(
        &mut self,
        detail: &str,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut SteamStep,
    ) {
        out.notes.push(Note::Platform {
            what: PlatformEvent::SerialError,
            detail: format!("steam discovery refused: {detail}"),
        });
        out.effects.push(Effect::OperatorMessage {
            link: LinkKind::Steam,
            text: NO_ADAPTER,
        });
        let lost = SafetyEvent::PortLost {
            link: LinkKind::Steam,
        };
        self.escalate(&lost, now, kernel, out);
    }

    // ---- commands -------------------------------------------------------

    fn on_command(&mut self, cmd: &SteamCommand, now: Monotonic, out: &mut SteamStep) {
        if let Some(refusal) = self.refuse_command(cmd) {
            out.notes.push(Note::Rejected {
                command: cmd.id(),
                reason: refusal.to_string(),
                check: "steam machine command",
            });
            out.refused = Some(refusal);
            return;
        }
        self.abandon();
        let dest = self.dest();
        match cmd {
            SteamCommand::Start {
                temp,
                minutes,
                command,
            } => {
                self.params = SteamParams {
                    temp: *temp,
                    minutes: *minutes,
                    state: SteamOpState::On,
                };
                self.command = Some(*command);
                // Two limits, and the shorter wins: this service's own session
                // deadline, and the generator's timer, which is the one that
                // survives this service dying.
                let requested = SessionDuration::clamped(minutes.as_duration());
                self.session = Some(SessionDeadline::start(
                    now,
                    requested,
                    self.settings.session_cap,
                ));
                self.phase = SteamPhase::Running {
                    since: now,
                    command: *command,
                };
                out.notes.push(Note::Accepted {
                    command: *command,
                    requested: format!(
                        "steam on at {:.1} F for {} minutes",
                        temp.fahrenheit(),
                        minutes.wire()
                    ),
                });
                let op = SteamOp::Start {
                    temp: *temp,
                    minutes: *minutes,
                };
                self.send(op, SteamJob::Write, dest, now, out);
            }
            SteamCommand::Stop { command } => {
                self.command = Some(*command);
                out.notes.push(Note::Accepted {
                    command: *command,
                    requested: "steam off".to_owned(),
                });
                self.stop_session(StopReason::Commanded, now, out);
            }
            SteamCommand::SetTemperature { temp, .. } => {
                self.params.temp = *temp;
                let op = SteamOp::SetTemperature {
                    temp: self.params.temp,
                    minutes: self.params.minutes,
                    state: self.params.state,
                };
                self.send(op, SteamJob::Write, dest, now, out);
            }
            SteamCommand::SetDuration { minutes, .. } => {
                self.params.minutes = *minutes;
                let op = SteamOp::SetDuration {
                    temp: self.params.temp,
                    minutes: self.params.minutes,
                    state: self.params.state,
                };
                self.send(op, SteamJob::Write, dest, now, out);
            }
        }
    }

    fn refuse_command(&self, cmd: &SteamCommand) -> Option<SteamRefusal> {
        if matches!(self.phase, SteamPhase::Unavailable { .. }) {
            return Some(SteamRefusal::WrongPhase {
                command: cmd.name(),
                phase: self.phase.kind(),
            });
        }
        if self.addr.is_none() {
            return Some(SteamRefusal::NotEnrolled);
        }
        let running = matches!(self.phase, SteamPhase::Running { .. });
        let ready = matches!(self.phase, SteamPhase::ReadyOff);
        let ok = match cmd {
            SteamCommand::Start { .. } => ready,
            // Stopping is accepted wherever there is still an address to send it
            // to.
            SteamCommand::Stop { .. } => true,
            SteamCommand::SetTemperature { .. } | SteamCommand::SetDuration { .. } => {
                running || ready
            }
        };
        (!ok).then(|| SteamRefusal::WrongPhase {
            command: cmd.name(),
            phase: self.phase.kind(),
        })
    }

    /// The stop frame, carrying the current setpoint and duration forward.
    ///
    /// Its operation state is [`SteamOpState::Off`] by construction: the type
    /// has two variants and this is the one that turns the generator off.
    fn stop_op(&self) -> SteamOp {
        SteamOp::Stop {
            temp: self.params.temp,
            minutes: self.params.minutes,
        }
    }

    /// Command the generator off and wait for the acknowledgement before calling
    /// the link ready again.
    fn stop_session(&mut self, stop: StopReason, now: Monotonic, out: &mut SteamStep) {
        self.abandon();
        self.params.state = SteamOpState::Off;
        self.stopping = Some(stop);
        let op = self.stop_op();
        let dest = self.dest();
        self.send(op, SteamJob::Write, dest, now, out);
    }

    fn on_acknowledged(&mut self) {
        if let SteamPhase::Unavailable { reason, .. } = &self.phase {
            self.phase = SteamPhase::Unavailable {
                reason: reason.clone(),
                acknowledged: true,
            };
        }
    }

    // ---- escalation -----------------------------------------------------

    /// Hand one observation to the kernel and apply what comes back.
    ///
    /// **No escalation decision is made here.** The scope, the latch reason and
    /// the effects are the kernel's; this function moves the machine to match
    /// them and passes them out unaltered.
    fn escalate(
        &mut self,
        event: &SafetyEvent,
        now: Monotonic,
        kernel: &mut SafetyKernel,
        out: &mut SteamStep,
    ) {
        let effects = kernel.on_event(event, now);
        out.notes.push(Note::Safety {
            link: LinkKind::Steam,
            trigger: format!("{event:?}"),
            effects: effects.iter().map(|e| format!("{e:?}")).collect(),
        });
        self.apply(&effects, now, out);
        out.effects.extend(effects);
    }

    fn apply(&mut self, effects: &[Effect], now: Monotonic, out: &mut SteamStep) {
        // The two steam effects are read together, because the ordering between
        // them is the property being enforced: a stop-then-latch must not be
        // collapsed into the latch that follows it in the same list.
        let stop_first = effects.contains(&Effect::SteamStopThenLatch);
        for effect in effects {
            if let Effect::Latch { link, reason } = effect
                && *link == LinkKind::Steam
            {
                if stop_first {
                    self.begin_stop_before_latch(reason.clone(), now, out);
                } else {
                    // A lost port cannot be told anything. Latch directly.
                    self.latch(reason.clone(), now, out);
                }
            }
        }
    }

    fn begin_stop_before_latch(
        &mut self,
        reason: LatchReason,
        now: Monotonic,
        out: &mut SteamStep,
    ) {
        if matches!(self.phase, SteamPhase::StoppingBeforeLatch { .. }) {
            return;
        }
        self.params.state = SteamOpState::Off;
        self.inflight = None;
        self.stale = 0;
        self.stopping = None;
        self.phase = SteamPhase::StoppingBeforeLatch {
            since: now,
            attempts: 0,
            reason,
        };
        // Transmission still works on a degraded link, so the stop goes out
        // before the link is given up. `STEAM-18`.
        self.send_stop_before_latch(now, out);
    }

    fn send_stop_before_latch(&mut self, now: Monotonic, out: &mut SteamStep) {
        let op = self.stop_op();
        let dest = self.dest();
        self.send(op, SteamJob::StopBeforeLatch, dest, now, out);
        if let SteamPhase::StoppingBeforeLatch { attempts, .. } = &mut self.phase {
            *attempts = attempts.saturating_add(1);
        }
    }

    fn retry_stop_or_latch(&mut self, now: Monotonic, out: &mut SteamStep) {
        let SteamPhase::StoppingBeforeLatch {
            attempts, reason, ..
        } = self.phase.clone()
        else {
            return;
        };
        if attempts >= self.settings.stop_attempts {
            // The stop could not be got through inside its budget. Latching
            // anyway is the only remaining move, and the log says the stop was
            // never acknowledged.
            out.notes.push(Note::Platform {
                what: PlatformEvent::SerialError,
                detail: format!("steam: the stop was unacknowledged after {attempts} attempts"),
            });
            out.effects.push(Effect::ClosePort(LinkKind::Steam));
            self.latch(reason, now, out);
        }
        // Otherwise the next tick sends it again, and every retry is a stop.
    }

    fn on_stop_acknowledged(&mut self, now: Monotonic, out: &mut SteamStep) {
        let SteamPhase::StoppingBeforeLatch { reason, .. } = self.phase.clone() else {
            return;
        };
        out.notes.push(Note::Platform {
            what: PlatformEvent::SerialError,
            detail: "steam: the stop was acknowledged; latching".to_owned(),
        });
        // The ordering `STEAM-18` requires is complete: the stop went out and
        // came back. Only now does the port close.
        out.effects.push(Effect::ClosePort(LinkKind::Steam));
        self.latch(reason, now, out);
    }

    fn latch(&mut self, reason: LatchReason, now: Monotonic, out: &mut SteamStep) {
        if let (Some(command), Some(session)) = (self.command.take(), self.session) {
            out.notes.push(Note::Session {
                link: LinkKind::Steam,
                command,
                duration: session.elapsed(now),
                stop: StopReason::Safety {
                    event: format!("{reason:?}"),
                },
            });
        }
        self.inflight = None;
        self.stale = 0;
        self.session = None;
        self.stopping = None;
        self.params.state = SteamOpState::Off;
        self.phase = SteamPhase::Unavailable {
            reason,
            acknowledged: false,
        };
    }

    // ---- housekeeping ---------------------------------------------------

    fn next_deadline(&self, now: Monotonic) -> Option<Monotonic> {
        if matches!(self.phase, SteamPhase::Unavailable { .. }) {
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
        {
            // The address enquiry has its own deadline in every source, and it
            // is not the device reply timeout.
            let wait = if matches!(f.job, SteamJob::AddressOpportunity) {
                self.settings.timings.address_enquiry_timeout
            } else {
                self.settings.timings.reply
            };
            if let Some(t) = f.sent_at.checked_add(wait) {
                consider(t);
            }
        }
        if let Some(s) = self.session.as_ref() {
            consider(s.expires_at());
        }
        Some(best)
    }

    fn refresh_cache(&mut self, now: Monotonic) {
        self.cache.phase = self.phase.kind();
        self.cache.assigned_address = self.addr.map(DevAddr::get);
        self.cache.session_remaining_s = self.session.map(|s| s.remaining(now).as_secs());
        self.cache.as_of = Some(now);
        if matches!(self.phase, SteamPhase::Unavailable { .. }) {
            self.cache.steaming = false;
        }
    }
}

/// Names every error bit that is set, documented and undocumented.
///
/// `CORRECTIONS.md` item 9. Bits are independent; a byte with three set gets
/// three names, not the first one.
fn describe_errors(flags: SteamErrorFlags) -> String {
    let mut names: Vec<&str> = Vec::new();
    if flags.contains(SteamErrorFlags::THERMISTOR) {
        names.push("thermistor");
    }
    if flags.contains(SteamErrorFlags::COMMUNICATION) {
        names.push("generator link lost");
    }
    if flags.contains(SteamErrorFlags::OVERTEMPERATURE) {
        names.push("overtemperature");
    }
    if flags.contains(SteamErrorFlags::SAFETY_CIRCUIT) {
        names.push("safety circuit tripped");
    }
    let reserved = flags.reserved_bits_set();
    if reserved == 0 {
        format!("steam error 0x{:02X}: {}", flags.bits(), names.join(", "))
    } else {
        names.push("undocumented bits");
        format!(
            "steam error 0x{:02X}: {} (0x{reserved:02X} is documented nowhere)",
            flags.bits(),
            names.join(", ")
        )
    }
}

const fn op_name(kind: SteamOpKind) -> &'static str {
    match kind {
        SteamOpKind::Start => "Start",
        SteamOpKind::Stop => "Stop",
        SteamOpKind::SetTemperature => "SetTemperature",
        SteamOpKind::SetDuration => "SetDuration",
        SteamOpKind::ReadStatus => "ReadStatus",
        SteamOpKind::ClearFaults => "ClearFaults",
        SteamOpKind::AddressOpportunity => "AddressOpportunity",
        SteamOpKind::AssignAddress => "AssignAddress",
    }
}
