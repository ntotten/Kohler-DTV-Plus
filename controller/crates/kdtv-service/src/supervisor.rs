//! The control loop: one task, one kernel, three links.
//!
//! # What a pass of the loop does
//!
//! It waits on the earliest of five things — a byte report from a pump, an
//! independent temperature sample, a command from the API, the shutdown signal,
//! and the deadline the engine last asked to be woken on — turns it into an
//! event for the machine it concerns, transmits whatever the resulting
//! [`Step`] says to transmit, performs the kernel's
//! [`Effect`]s in order, pets the watchdog and publishes the cache.
//!
//! # Answers to the five design questions
//!
//! **Runtime shape.** One supervisor task, plus a byte pump per link. The
//! kernel holds all three links' state because a shared fault crosses them, so
//! it is borrowed `&mut` by whichever machine is stepping; one owner means that
//! borrow is never contended and never half-applied. Everything is `Send`, so
//! either runtime flavour works, and the daemon should use `current_thread`
//! because none of this is parallelisable.
//!
//! **How a command reaches a link.** A bounded [`tokio::sync::mpsc`] carrying
//! [`Command`], with a `oneshot` for the answer. A
//! [`StartAuthorization`](kdtv_safety::StartAuthorization) is `!Clone` and moves
//! through it by value exactly once, into
//! [`SafetyKernel::authorize_open`](kdtv_safety::SafetyKernel::authorize_open),
//! which is the only thing that mints a grant.
//!
//! **Shutdown.** `SIGTERM` and `SIGINT` command a stop on every link and the
//! loop keeps running until every machine confirms itself off. If the
//! confirmation does not arrive inside [`SHUTDOWN_GRACE`] the ports are closed
//! anyway — the valve's own communication-loss shutdown is the backstop — and
//! [`ShutdownOutcome::UnconfirmedOff`] names the links, so the daemon exits
//! non-zero and the log says which valve was never heard from. It does not claim
//! a clean stop it did not get.
//!
//! **Watchdog.** Petted at the bottom of this loop and nowhere else. A separate
//! petting task would keep the daemon alive while the loop was wedged, which is
//! the one failure the watchdog exists to catch. The pet is proof the loop
//! completed a pass; it is deliberately *not* proof a link is healthy — a link
//! that has gone is latched, and a latched service must stay up, because
//! recovery from a latch is never automatic and a restart would walk the boot
//! sequence again.
//!
//! **What a tick is.** Whatever [`Step::deadline`](kdtv_engine::Step) asked
//! for, floored by [`HOUSEKEEPING`] so a latched service still publishes and
//! still checks for probe starvation. A deadline already past is not slept on
//! and not caught up: exactly one event is delivered, because two ticks in a
//! pass would put two frames on a bus that allows one. Lateness beyond a tick is
//! logged as a platform event rather than absorbed.
//!
//! # One transaction in flight, per link
//!
//! A step carries at most one `tx` and a machine emits none while it has one
//! outstanding, so the property is the engine's. What this module adds is the
//! bookkeeping that tells a tick from a response timeout: `awaiting_until` is
//! set when a frame is queued and cleared when the answer, the failure or the
//! timeout is delivered.

use std::sync::Arc;
use std::time::Duration;

use kdtv_engine::{
    OperatorCommand, Refusal, StartRequest, SteamCommand, SteamEvent, SteamMachine, SteamPhaseKind,
    SteamRefusal, SteamStep, Step, ZoneEvent, ZoneMachine, ZonePhaseKind,
};
use kdtv_hal::{Clock, Watchdog};
use kdtv_proto::dtv::{DiscoveryStep, DtvRxBuffer, DtvTimings, SteamEncoder, SteamOp};
use kdtv_proto::saturn::{Encoder, Expectation, MasterAddr, RxBuffer, Timings};
use kdtv_safety::{Effect, RtdWatch, SafetyEvent, SafetyKernel};
use kdtv_telemetry::{Direction, LogEvent, Monotonic, PlatformEvent, RequestSource, Stamp};
use kdtv_units::{CommandId, LinkKind, PiBootId, ZoneId};
use tokio::sync::{mpsc, watch};

use crate::cache::{
    IndependentReading, LinkStateLabel, StateCache, SteamStatus, SystemSnapshot, ZoneStatus,
};
use crate::command::{Command, CommandError};
use crate::event::{Lifecycle, ServiceEvent};
use crate::port::{LinkReport, PortHandle};
use crate::record::Recorder;
use crate::rtd::Sampled;

/// The longest the loop will sleep with nothing else to do.
///
/// It bounds three things: how stale a published snapshot can be, how late a
/// probe-starvation check can be, and how long a latched service can go without
/// petting the watchdog. Well inside the 5 s
/// [`kdtv_units::RTD_STARVATION`] window and half of the deployed 10 s
/// `WatchdogSec`.
pub const HOUSEKEEPING: Duration = Duration::from_millis(500);

/// How long shutdown waits for every link to confirm itself off.
///
/// Two full Saturn transaction budgets and change: a stop, its retries and its
/// acknowledgement fit inside it twice over. Longer would delay the exit past
/// what systemd will wait for on a restart; shorter would report an unconfirmed
/// stop that was merely slow.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// How long the loop waits for the pumps to report their ports closed.
const CLOSE_GRACE: Duration = Duration::from_millis(500);

/// A bound on how many frames one read chunk may yield, so a wedged decoder
/// cannot spin. The buffer shrinks on every path, so this is belt and braces.
const MAX_FRAMES_PER_CHUNK: usize = 16;

/// How the service stopped.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ShutdownOutcome {
    /// Every link commanded off and confirmed off before the process exited.
    ConfirmedOff,
    /// At least one link never confirmed. **The worst outcome this system
    /// has.** The ports were closed regardless, so the valve's own
    /// communication-loss shutdown is what stops the water; the links are named
    /// so the daemon can exit non-zero and the operator knows which one.
    UnconfirmedOff { links: Vec<LinkKind> },
}

impl ShutdownOutcome {
    #[must_use]
    pub const fn is_confirmed(&self) -> bool {
        matches!(self, Self::ConfirmedOff)
    }
}

/// Which of the loop's channels still have a sender.
///
/// A `recv` on a closed channel resolves immediately and forever, so an arm
/// that is not disabled becomes a spin at the speed of the scheduler — which is
/// precisely when the loop most needs to be servicing the drain.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct Live {
    reports: bool,
    samples: bool,
    commands: bool,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            reports: true,
            samples: true,
            commands: true,
        }
    }
}

/// Whether the loop is serving commands or stopping water.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Mode {
    Serving,
    Draining { until: Monotonic },
}

/// One valve bus, as the supervisor holds it.
#[derive(Debug)]
pub(crate) struct ZoneRuntime {
    pub(crate) zone: ZoneId,
    pub(crate) link: LinkKind,
    pub(crate) master: MasterAddr,
    pub(crate) machine: ZoneMachine,
    pub(crate) encoder: Encoder,
    pub(crate) rx: RxBuffer,
    pub(crate) port: PortHandle,
    pub(crate) timings: Timings,
    pub(crate) watch: RtdWatch,
    independent: Option<IndependentReading>,
    /// When the outstanding response is due, if one is.
    awaiting_until: Option<Monotonic>,
    /// When the machine asked to be stepped again.
    wake_at: Option<Monotonic>,
    frames_tx: u64,
    frames_rx: u64,
    /// The highest readings seen since the last session record. `LOG-08`.
    max_valve_c: Option<f32>,
    max_independent_c: Option<f32>,
    last_refusal: Option<Refusal>,
    /// Starvation is reported once per episode, not once per housekeeping pass.
    starvation_reported: bool,
    overflowed: usize,
}

impl ZoneRuntime {
    pub(crate) fn new(
        zone: ZoneId,
        machine: ZoneMachine,
        encoder: Encoder,
        port: PortHandle,
        master: MasterAddr,
        timings: Timings,
        watch: RtdWatch,
    ) -> Self {
        Self {
            zone,
            link: LinkKind::Zone(zone),
            master,
            machine,
            encoder,
            rx: RxBuffer::new(),
            port,
            timings,
            watch,
            independent: None,
            awaiting_until: None,
            wake_at: Some(Monotonic::from_nanos(0)),
            frames_tx: 0,
            frames_rx: 0,
            max_valve_c: None,
            max_independent_c: None,
            last_refusal: None,
            starvation_reported: false,
            overflowed: 0,
        }
    }

    /// This service has **confirmation** the valve is closed.
    ///
    /// `ReadyOff` is reached only through an acknowledged all-off, and `Cold`
    /// has never transmitted a write, so nothing this service did opened
    /// anything. `Unavailable` is deliberately **not** confirmed: a latch sends
    /// an all-off and closes the port in the same step, so the acknowledgement
    /// never arrives and nothing here knows what the valve did with it.
    fn confirmed_off(&self) -> bool {
        matches!(
            self.machine.phase().kind(),
            ZonePhaseKind::ReadyOff | ZonePhaseKind::Cold
        ) && !self.machine.cached().water_moving
    }

    /// Nothing more can be done for this link: it is confirmed off, or its port
    /// is gone and there is nothing left to command it on.
    fn settled(&self) -> bool {
        self.confirmed_off() || !self.port.is_open()
    }

    fn expectation(&self) -> Expectation {
        self.machine.expectation().unwrap_or(Expectation {
            master: self.master,
            awaiting: None,
            strict: true,
        })
    }
}

/// The DTV+ steam link, as the supervisor holds it.
#[derive(Debug)]
pub(crate) struct SteamRuntime {
    pub(crate) machine: SteamMachine,
    pub(crate) encoder: SteamEncoder,
    pub(crate) rx: DtvRxBuffer,
    pub(crate) port: PortHandle,
    pub(crate) timings: DtvTimings,
    awaiting_until: Option<Monotonic>,
    wake_at: Option<Monotonic>,
    frames_tx: u64,
    frames_rx: u64,
    last_refusal: Option<SteamRefusal>,
}

impl SteamRuntime {
    pub(crate) fn new(
        machine: SteamMachine,
        encoder: SteamEncoder,
        port: PortHandle,
        timings: DtvTimings,
    ) -> Self {
        Self {
            machine,
            encoder,
            rx: DtvRxBuffer::new(),
            port,
            timings,
            awaiting_until: None,
            wake_at: Some(Monotonic::from_nanos(0)),
            frames_tx: 0,
            frames_rx: 0,
            last_refusal: None,
        }
    }

    /// On the same terms as [`ZoneRuntime::confirmed_off`].
    fn confirmed_off(&self) -> bool {
        matches!(
            self.machine.phase().kind(),
            SteamPhaseKind::ReadyOff | SteamPhaseKind::Cold
        ) && !self.machine.cached().steaming
    }

    fn settled(&self) -> bool {
        self.confirmed_off() || !self.port.is_open()
    }
}

/// What woke the loop.
enum Woke {
    Report(Option<LinkReport>),
    Sample(Option<Sampled>),
    Command(Option<Command>),
    Shutdown,
    Deadline,
}

/// The control loop.
#[derive(Debug)]
pub struct Supervisor {
    pub(crate) kernel: SafetyKernel,
    pub(crate) zones: Vec<ZoneRuntime>,
    pub(crate) steam: Option<SteamRuntime>,
    pub(crate) recorder: Recorder,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) watchdog: Arc<dyn Watchdog>,
    pub(crate) cache: Arc<StateCache>,
    pub(crate) commands: mpsc::Receiver<Command>,
    pub(crate) reports: mpsc::Receiver<LinkReport>,
    pub(crate) samples: mpsc::Receiver<Sampled>,
    pub(crate) stop_samplers: watch::Sender<bool>,
    pub(crate) shutdown: watch::Receiver<Option<&'static str>>,
    pub(crate) pi_boot: PiBootId,
    pub(crate) shutdown_command: CommandId,
    pub(crate) grace: Duration,
    mode: Mode,
    /// Which of the loop's channels are still live.
    live: Live,
    last_pet: Option<Monotonic>,
    last_published: Option<SystemSnapshot>,
    /// Guards the one recursive escalation path: an encoder refusal escalates
    /// to a shared fault, and the escalation must not be able to escalate again.
    escalating: bool,
}

impl Supervisor {
    /// Assemble the loop. Everything is already open; nothing here can fail.
    #[expect(
        clippy::too_many_arguments,
        reason = "this is the composition root's argument list; grouping it into a \
                  struct would move the same fields one level down and hide which of \
                  them the loop actually owns"
    )]
    pub(crate) fn assemble(
        kernel: SafetyKernel,
        zones: Vec<ZoneRuntime>,
        steam: Option<SteamRuntime>,
        recorder: Recorder,
        clock: Arc<dyn Clock>,
        watchdog: Arc<dyn Watchdog>,
        cache: Arc<StateCache>,
        channels: SupervisorChannels,
        pi_boot: PiBootId,
        shutdown_command: CommandId,
    ) -> Self {
        Self {
            kernel,
            zones,
            steam,
            recorder,
            clock,
            watchdog,
            cache,
            commands: channels.commands,
            reports: channels.reports,
            samples: channels.samples,
            stop_samplers: channels.stop_samplers,
            shutdown: channels.shutdown,
            pi_boot,
            shutdown_command,
            grace: SHUTDOWN_GRACE,
            mode: Mode::Serving,
            live: Live::default(),
            last_pet: None,
            last_published: None,
            escalating: false,
        }
    }

    /// Run until the shutdown signal, then stop water and confirm it.
    pub async fn run(mut self) -> ShutdownOutcome {
        self.announce_start();
        loop {
            let now = self.clock.monotonic();
            if let Mode::Draining { until } = self.mode
                && (self.all_settled() || now >= until)
            {
                break;
            }

            // A closed channel resolves immediately and forever, so each arm is
            // disabled once it has ended. Without that the loop would spin at
            // the speed of the scheduler the moment the last pump exited, which
            // is precisely when it most needs to be servicing the drain.
            let Live {
                reports,
                samples,
                commands,
            } = self.live;
            let serving = matches!(self.mode, Mode::Serving);
            let nap = self.clock.sleep_until(self.next_wake(now));
            let woke = tokio::select! {
                biased;
                report = self.reports.recv(), if reports => Woke::Report(report),
                sample = self.samples.recv(), if samples => Woke::Sample(sample),
                command = self.commands.recv(), if commands => Woke::Command(command),
                changed = self.shutdown.changed(), if serving => {
                    let _ = changed;
                    Woke::Shutdown
                }
                () = nap => Woke::Deadline,
            };

            let now = self.clock.monotonic();
            match woke {
                Woke::Report(Some(report)) => self.on_report(report, now),
                Woke::Sample(Some(sample)) => self.on_sample(sample, now),
                Woke::Command(Some(command)) => self.on_command(command, now),
                // Every pump has gone, or the API has. Neither is survivable as
                // a serving state, and both end the only correct way.
                Woke::Report(None) => {
                    self.live.reports = false;
                    self.begin_drain("every link pump has exited", now);
                }
                Woke::Command(None) => {
                    self.live.commands = false;
                    self.begin_drain("the command channel closed", now);
                }
                Woke::Sample(None) => self.live.samples = false,
                Woke::Shutdown => {
                    let reason = (*self.shutdown.borrow_and_update()).unwrap_or("shutdown");
                    self.begin_drain(reason, now);
                }
                Woke::Deadline => self.on_deadline(now),
            }

            self.pet(now);
            self.publish(now);
        }
        self.finish().await
    }

    fn announce_start(&mut self) {
        let now = self.clock.monotonic();
        let at = self.stamp(now);
        if self.watchdog.interval().is_none() {
            // The hal reports no watchdog as `None` rather than pretending; a
            // production start without one is a deployment mistake, and this is
            // where it becomes visible.
            self.recorder.platform(
                PlatformEvent::WatchdogMissed,
                "no systemd watchdog is configured for this process; a missed control \
                 loop will not restart the service"
                    .to_owned(),
                at,
            );
        }
        self.watchdog.notify_ready();
        self.recorder.platform(
            PlatformEvent::ServiceStarted,
            format!(
                "boot {} on pi boot {}: {} valve links and {} steam link",
                self.recorder.boot().0,
                self.pi_boot.0,
                self.zones.len(),
                usize::from(self.steam.is_some()),
            ),
            at,
        );
        self.recorder.publish(ServiceEvent::Lifecycle {
            what: Lifecycle::Ready,
            detail: "every link is bound".to_owned(),
        });
        self.publish(now);
    }

    // ---- scheduling ------------------------------------------------------

    fn next_wake(&self, now: Monotonic) -> Monotonic {
        let mut best = now.checked_add(HOUSEKEEPING).unwrap_or(now);
        for zone in &self.zones {
            if let Some(at) = zone.wake_at
                && at < best
            {
                best = at;
            }
        }
        if let Some(steam) = self.steam.as_ref()
            && let Some(at) = steam.wake_at
            && at < best
        {
            best = at;
        }
        best
    }

    /// A deadline came due, or nothing happened for [`HOUSEKEEPING`].
    ///
    /// One event per link per pass. A deadline already in the past is stepped
    /// immediately and **not** caught up — two ticks in a pass would put two
    /// frames on a bus that correlates responses by there being one.
    fn on_deadline(&mut self, now: Monotonic) {
        for index in 0..self.zones.len() {
            let Some(zone) = self.zones.get(index) else {
                continue;
            };
            let Some(due) = zone.wake_at else { continue };
            if now < due {
                continue;
            }
            self.report_lateness(zone.link, due, zone.timings.tick, now);
            let timed_out = zone.awaiting_until.is_some_and(|at| now >= at);
            let event = if timed_out {
                ZoneEvent::ResponseTimeout
            } else {
                ZoneEvent::Tick
            };
            if timed_out && let Some(zone) = self.zones.get_mut(index) {
                zone.awaiting_until = None;
            }
            self.drive_zone(index, event, now, None);
        }

        if let Some(steam) = self.steam.as_ref()
            && let Some(due) = steam.wake_at
            && now >= due
        {
            self.report_lateness(LinkKind::Steam, due, steam.timings.tick, now);
            let timed_out = steam.awaiting_until.is_some_and(|at| now >= at);
            let event = if timed_out {
                SteamEvent::ResponseTimeout
            } else {
                SteamEvent::Tick
            };
            if timed_out && let Some(steam) = self.steam.as_mut() {
                steam.awaiting_until = None;
            }
            self.drive_steam(event, now, None);
        }

        self.check_starvation(now);
    }

    /// A loop that fell a whole tick behind is reported, not absorbed.
    fn report_lateness(&self, link: LinkKind, due: Monotonic, tick: Duration, now: Monotonic) {
        let late = now.since(due);
        if late > tick {
            self.recorder.platform(
                PlatformEvent::SerialError,
                format!(
                    "{link}: the control loop was {} ms late for a deadline; the tick is {} ms",
                    late.as_millis(),
                    tick.as_millis()
                ),
                self.stamp(now),
            );
        }
    }

    /// The absence of a sample cannot announce itself, so it is checked here.
    fn check_starvation(&mut self, now: Monotonic) {
        for index in 0..self.zones.len() {
            let Some(zone) = self.zones.get(index) else {
                continue;
            };
            if zone.starvation_reported
                || LinkStateLabel::of(self.kernel.state(zone.link)).is_latched()
            {
                continue;
            }
            let Some(event) = zone.watch.check_starvation(now) else {
                continue;
            };
            if let Some(zone) = self.zones.get_mut(index) {
                zone.starvation_reported = true;
            }
            self.drive_zone(index, ZoneEvent::Safety(event), now, None);
        }
    }

    // ---- link reports ----------------------------------------------------

    fn on_report(&mut self, report: LinkReport, now: Monotonic) {
        let link = report.link();
        let at = self.stamp(now);
        match report {
            LinkReport::Sent { bytes, .. } => {
                // Recorded when it reached the wire, not when it was queued.
                // `LOG-05`.
                self.recorder.frame(link, Direction::Tx, &bytes, at, None);
                self.count_tx(link);
            }
            LinkReport::Received { bytes, .. } => {
                self.recorder.frame(link, Direction::Rx, &bytes, at, None);
                self.on_bytes(link, &bytes, now);
            }
            LinkReport::Failed { error, .. } => {
                let terminal = error.is_disconnected();
                self.recorder.platform(
                    if terminal {
                        PlatformEvent::UsbEnumerationLost
                    } else {
                        PlatformEvent::SerialError
                    },
                    error.to_string(),
                    at,
                );
                if terminal {
                    self.close_port(link);
                    self.drive_port_closed(link, now);
                }
            }
            LinkReport::Closed { .. } => {
                self.mark_closed(link);
                self.recorder.platform(
                    PlatformEvent::SerialClosed,
                    format!("{link}: port closed"),
                    at,
                );
            }
        }
    }

    fn on_bytes(&mut self, link: LinkKind, bytes: &[u8], now: Monotonic) {
        match link {
            LinkKind::Zone(zone) => {
                let Some(index) = self.zone_index(zone) else {
                    return;
                };
                self.feed_zone_bytes(index, bytes, now);
            }
            LinkKind::Steam => self.feed_steam_bytes(bytes, now),
        }
    }

    fn feed_zone_bytes(&mut self, index: usize, bytes: &[u8], now: Monotonic) {
        {
            let Some(zone) = self.zones.get_mut(index) else {
                return;
            };
            zone.rx.extend(bytes);
        }
        self.report_overflow(index, now);

        for _ in 0..MAX_FRAMES_PER_CHUNK {
            let Some(zone) = self.zones.get_mut(index) else {
                return;
            };
            let expect = zone.expectation();
            let before = zone.rx.as_slice().to_vec();
            let outcome = kdtv_proto::saturn::decode(&mut zone.rx, &expect);
            match outcome {
                Ok(None) => return,
                Ok(Some(frame)) => {
                    zone.frames_rx = zone.frames_rx.saturating_add(1);
                    zone.awaiting_until = None;
                    self.drive_zone(index, ZoneEvent::Response(frame), now, None);
                }
                Err(why) => {
                    zone.frames_rx = zone.frames_rx.saturating_add(1);
                    zone.awaiting_until = None;
                    let link = zone.link;
                    let at = self.stamp(now);
                    self.recorder
                        .frame(link, Direction::Rx, &before, at, Some(why.to_string()));
                    self.drive_zone(index, ZoneEvent::DecodeFailed(why), now, None);
                }
            }
        }
    }

    fn feed_steam_bytes(&mut self, bytes: &[u8], now: Monotonic) {
        {
            let Some(steam) = self.steam.as_mut() else {
                return;
            };
            steam.rx.extend(bytes);
        }
        for _ in 0..MAX_FRAMES_PER_CHUNK {
            let Some(steam) = self.steam.as_mut() else {
                return;
            };
            let before = steam.rx.as_slice().to_vec();
            let outcome = kdtv_proto::dtv::decode(&mut steam.rx);
            match outcome {
                Ok(None) => return,
                Ok(Some(frame)) => {
                    steam.frames_rx = steam.frames_rx.saturating_add(1);
                    steam.awaiting_until = None;
                    self.drive_steam(SteamEvent::Response(frame), now, None);
                }
                Err(why) => {
                    steam.frames_rx = steam.frames_rx.saturating_add(1);
                    steam.awaiting_until = None;
                    let at = self.stamp(now);
                    self.recorder.frame(
                        LinkKind::Steam,
                        Direction::Rx,
                        &before,
                        at,
                        Some(why.to_string()),
                    );
                    self.drive_steam(SteamEvent::DecodeFailed(why), now, None);
                }
            }
        }
    }

    /// A receive buffer that overflowed is producing more than the decoder is
    /// consuming, which the decoder's own documentation calls a fault rather
    /// than noise.
    fn report_overflow(&mut self, index: usize, now: Monotonic) {
        let Some(zone) = self.zones.get_mut(index) else {
            return;
        };
        let overflowed = zone.rx.overflowed();
        if overflowed <= zone.overflowed {
            return;
        }
        let dropped = overflowed.saturating_sub(zone.overflowed);
        zone.overflowed = overflowed;
        let link = zone.link;
        self.recorder.platform(
            PlatformEvent::SerialError,
            format!("{link}: the receive buffer dropped {dropped} bytes it could not hold"),
            self.stamp(now),
        );
    }

    // ---- driving the machines -------------------------------------------

    fn drive_zone(
        &mut self,
        index: usize,
        event: ZoneEvent,
        now: Monotonic,
        source: Option<&RequestSource>,
    ) {
        let at = self.stamp(now);
        let denial = {
            let Self {
                zones,
                kernel,
                recorder,
                ..
            } = self;
            let Some(zone) = zones.get_mut(index) else {
                return;
            };
            let step = zone.machine.step(event, now, kernel);
            apply_zone_step(zone, &step, now, at, recorder, source)
        };
        if let Some(detail) = denial {
            self.on_encode_denied(&detail, now);
        }
    }

    fn drive_steam(&mut self, event: SteamEvent, now: Monotonic, source: Option<&RequestSource>) {
        let at = self.stamp(now);
        let denial = {
            let Self {
                steam,
                kernel,
                recorder,
                ..
            } = self;
            let Some(steam) = steam.as_mut() else {
                return;
            };
            let step = steam.machine.step(event, now, kernel);
            apply_steam_step(steam, &step, now, at, recorder, source)
        };
        if let Some(detail) = denial {
            self.on_encode_denied(&detail, now);
        }
    }

    /// The encoder refused something a machine asked for.
    ///
    /// That is not a wire condition; it is this service disagreeing with its own
    /// allowlist, which is [`SafetyEvent::ServiceFailure`] — one of the four
    /// events the kernel scopes to everything. Harsh, and correct: a controller
    /// that cannot build the frame it decided to send does not get to keep
    /// driving the other zone.
    fn on_encode_denied(&mut self, detail: &str, now: Monotonic) {
        if self.escalating {
            return;
        }
        self.escalating = true;
        self.recorder.platform(
            PlatformEvent::SerialError,
            format!("the encoder refused a frame this service decided to send: {detail}"),
            self.stamp(now),
        );
        self.escalate_shared(&SafetyEvent::ServiceFailure, now);
        self.escalating = false;
    }

    /// Hand a shared fault to every machine.
    ///
    /// The four [`kdtv_safety::FaultScope::Shared`] events are the only ones
    /// whose effects name a link other than the one that raised them, and all
    /// four are observed here rather than by a machine — a failed configuration
    /// check, a missed watchdog, a lost USB controller, an internal failure. So
    /// the fan-out is deliberate and lives here. Each machine applies only what
    /// names its own link and writes its own log line, which is one line per
    /// link and exactly what the log wants.
    fn escalate_shared(&mut self, event: &SafetyEvent, now: Monotonic) {
        for index in 0..self.zones.len() {
            self.drive_zone(index, ZoneEvent::Safety(event.clone()), now, None);
        }
        if self.steam.is_some() {
            self.drive_steam(SteamEvent::Safety(event.clone()), now, None);
        }
    }

    fn drive_port_closed(&mut self, link: LinkKind, now: Monotonic) {
        match link {
            LinkKind::Zone(zone) => {
                if let Some(index) = self.zone_index(zone) {
                    self.drive_zone(index, ZoneEvent::PortClosed, now, None);
                }
            }
            LinkKind::Steam => self.drive_steam(SteamEvent::PortClosed, now, None),
        }
    }

    // ---- the independent temperature chain ------------------------------

    fn on_sample(&mut self, sampled: Sampled, now: Monotonic) {
        let Some(index) = self.zone_index(sampled.zone) else {
            return;
        };
        let at = self.stamp(now);
        let events = match sampled.result {
            Ok(sample) => self.absorb_sample(index, &sample, at),
            Err(why) => {
                self.recorder
                    .platform(PlatformEvent::SerialError, why.to_string(), at);
                // A transfer that failed is not a reading. Starvation is what
                // reports a channel that has stopped answering, and it is
                // checked on the loop's own tick.
                return;
            }
        };
        for event in events {
            self.drive_zone(index, ZoneEvent::Safety(event), now, None);
        }
    }

    fn absorb_sample(
        &mut self,
        index: usize,
        sample: &kdtv_hal::RtdSample,
        at: Stamp,
    ) -> Vec<SafetyEvent> {
        let Some(zone) = self.zones.get_mut(index) else {
            return Vec::new();
        };
        zone.starvation_reported = false;
        let first = zone.independent.is_none();
        let cached = zone.machine.cached();
        let outlet_on = cached.valve_on;
        let valve_c = cached.valve_reported_c;
        let water_moving = cached.water_moving;
        let corrected = zone.watch.correct(sample.raw);
        let events = zone
            .watch
            .observe(
                kdtv_safety::RtdSample {
                    raw: sample.raw,
                    fault_register: sample.fault.bits(),
                    at: sample.at,
                },
                outlet_on,
                valve_c,
            )
            .to_vec();
        zone.independent = Some(IndependentReading {
            raw_c: sample.raw.0,
            corrected_c: corrected.celsius(),
            fault_bits: sample.fault.bits(),
            at: sample.at,
        });
        if water_moving {
            zone.max_independent_c = Some(
                zone.max_independent_c
                    .map_or(corrected.celsius(), |m| m.max(corrected.celsius())),
            );
        }
        let link = zone.link;
        // `LOG-03` / `LOG-10`: both numbers, together. Every sample while water
        // may be moving, because that is when it is evidence; otherwise once per
        // starvation window, so a live channel is still visible in the log
        // without filling it.
        if water_moving || first || !events.is_empty() {
            self.recorder.temperature(
                link,
                Some(sample.raw.0),
                Some(corrected.celsius()),
                valve_c,
                at,
            );
        } else {
            tracing::trace!(
                link = %link,
                raw_c = sample.raw.0,
                corrected_c = corrected.celsius(),
                "independent temperature"
            );
        }
        events
    }

    // ---- commands --------------------------------------------------------

    fn on_command(&mut self, command: Command, now: Monotonic) {
        // A stop has already been commanded on every link and the confirmations
        // are outstanding. Answering the caller is better than dropping the
        // reply channel and leaving them to infer it from a closed connection.
        if matches!(self.mode, Mode::Draining { .. }) {
            refuse(command, &CommandError::ShuttingDown);
            return;
        }
        match command {
            Command::Start {
                request,
                authorization,
                source,
                reply,
            } => {
                let answer = self.start(&request, authorization, &source, now);
                let _ = reply.send(answer);
            }
            Command::Zone {
                zone,
                command,
                source,
                reply,
            } => {
                let answer = self.zone_command(zone, command, &source, now);
                let _ = reply.send(answer);
            }
            Command::Steam {
                command,
                source,
                reply,
            } => {
                let answer = self.steam_command(command, &source, now);
                let _ = reply.send(answer);
            }
            Command::StopAll {
                command,
                source,
                reply,
            } => {
                self.stop_all(command, &source, now);
                let _ = reply.send(Ok(command));
            }
            Command::Acknowledge {
                link,
                ack,
                source,
                reply,
            } => {
                let answer = self.acknowledge(link, &ack, &source, now);
                let _ = reply.send(answer);
            }
        }
    }

    fn start(
        &mut self,
        request: &kdtv_safety::ValidatedStart,
        authorization: kdtv_safety::StartAuthorization,
        source: &RequestSource,
        now: Monotonic,
    ) -> Result<CommandId, CommandError> {
        let zone = request.zone;
        let id = request.command;
        let Some(index) = self.zone_index(zone) else {
            return Err(CommandError::NoSuchLink(LinkKind::Zone(zone)));
        };
        let grant = match self.kernel.authorize_open(request, authorization) {
            Ok(grant) => grant,
            Err(denial) => {
                self.recorder
                    .rejection(id, denial.to_string(), "safety kernel", self.stamp(now));
                return Err(CommandError::Denied(denial));
            }
        };

        if let Some(zone) = self.zones.get_mut(index) {
            zone.last_refusal = None;
        }
        let start = StartRequest {
            outlets: request.outlets,
            temperature: request.temperature,
            duration: request.duration,
            command: id,
        };
        self.drive_zone(index, ZoneEvent::Start(start, grant), now, Some(source));

        // The kernel commits `Running` before the machine has agreed, and the
        // machine has its own refusals — a valve whose health this service has
        // not itself established, an outlet the valve's table does not carry.
        // A refusal here means no frame went out and the kernel's belief is
        // wrong, so it is put back.
        match self.zones.get(index).and_then(|z| z.last_refusal.clone()) {
            Some(refusal) => {
                self.kernel.mark_stopped(zone);
                Err(CommandError::ZoneRefused(refusal))
            }
            None => Ok(id),
        }
    }

    fn zone_command(
        &mut self,
        zone: ZoneId,
        command: OperatorCommand,
        source: &RequestSource,
        now: Monotonic,
    ) -> Result<CommandId, CommandError> {
        let id = zone_command_id(&command);
        let Some(index) = self.zone_index(zone) else {
            return Err(CommandError::NoSuchLink(LinkKind::Zone(zone)));
        };
        if let Some(zone) = self.zones.get_mut(index) {
            zone.last_refusal = None;
        }
        self.drive_zone(index, ZoneEvent::Command(command), now, Some(source));
        match self.zones.get(index).and_then(|z| z.last_refusal.clone()) {
            Some(refusal) => Err(CommandError::ZoneRefused(refusal)),
            None => Ok(id),
        }
    }

    fn steam_command(
        &mut self,
        command: SteamCommand,
        source: &RequestSource,
        now: Monotonic,
    ) -> Result<CommandId, CommandError> {
        let id = steam_command_id(&command);
        if self.steam.is_none() {
            return Err(CommandError::NoSuchLink(LinkKind::Steam));
        }
        self.drive_steam(SteamEvent::Command(command), now, Some(source));
        match self.steam.as_ref().and_then(|s| s.last_refusal.clone()) {
            Some(refusal) => Err(CommandError::SteamRefused(refusal)),
            None => Ok(id),
        }
    }

    fn stop_all(&mut self, command: CommandId, source: &RequestSource, now: Monotonic) {
        for index in 0..self.zones.len() {
            self.drive_zone(
                index,
                ZoneEvent::Command(OperatorCommand::Stop { command }),
                now,
                Some(source),
            );
        }
        if self.steam.is_some() {
            self.drive_steam(
                SteamEvent::Command(SteamCommand::Stop { command }),
                now,
                Some(source),
            );
        }
    }

    /// Acknowledge a latched link.
    ///
    /// It marks the latch acknowledged and nothing else. The port closed when
    /// the link latched, and **this service does not reopen it**: coming back
    /// means walking discovery again, which needs the link factory and the
    /// transmit gate a second time. That is not implemented here, so today an
    /// acknowledgement is a record that a person saw the fault, and the way back
    /// to water is a service restart. `kdtv_safety::ZoneAuthority::may_reopen`
    /// is the flag a later reopen path would read.
    fn acknowledge(
        &mut self,
        link: LinkKind,
        ack: &kdtv_safety::OperatorAck,
        source: &RequestSource,
        now: Monotonic,
    ) -> Result<CommandId, CommandError> {
        let id = ack.command();
        if let Err(denial) = self.kernel.acknowledge(link) {
            self.recorder
                .rejection(id, denial.to_string(), "safety kernel", self.stamp(now));
            return Err(CommandError::Denied(denial));
        }
        match link {
            LinkKind::Zone(zone) => {
                let Some(index) = self.zone_index(zone) else {
                    return Err(CommandError::NoSuchLink(link));
                };
                self.drive_zone(index, ZoneEvent::Acknowledged, now, Some(source));
            }
            LinkKind::Steam => {
                if self.steam.is_none() {
                    return Err(CommandError::NoSuchLink(link));
                }
                self.drive_steam(SteamEvent::Acknowledged, now, Some(source));
            }
        }
        Ok(id)
    }

    // ---- shutdown --------------------------------------------------------

    fn begin_drain(&mut self, reason: &'static str, now: Monotonic) {
        if matches!(self.mode, Mode::Draining { .. }) {
            return;
        }
        let until = now.checked_add(self.grace).unwrap_or(now);
        self.mode = Mode::Draining { until };
        self.recorder.platform(
            PlatformEvent::ServiceStopping,
            format!("{reason}: commanding every link off"),
            self.stamp(now),
        );
        self.recorder.publish(ServiceEvent::Lifecycle {
            what: Lifecycle::ShuttingDown,
            detail: reason.to_owned(),
        });
        let command = self.shutdown_command;
        let source = RequestSource::Service {
            reason: "shutdown: stop water before exit",
        };
        self.stop_all(command, &source, now);
    }

    /// Every link has either confirmed itself off or lost the port it would
    /// have been told on. Waiting past that point buys nothing: a closed port
    /// cannot be commanded, and the outcome already names it as unconfirmed.
    fn all_settled(&self) -> bool {
        self.zones.iter().all(ZoneRuntime::settled)
            && self.steam.as_ref().is_none_or(SteamRuntime::settled)
    }

    fn unconfirmed(&self) -> Vec<LinkKind> {
        let mut out: Vec<LinkKind> = self
            .zones
            .iter()
            .filter(|z| !z.confirmed_off())
            .map(|z| z.link)
            .collect();
        if self.steam.as_ref().is_some_and(|s| !s.confirmed_off()) {
            out.push(LinkKind::Steam);
        }
        out
    }

    /// Close every port, wait briefly for the pumps to say so, and report what
    /// actually happened.
    async fn finish(mut self) -> ShutdownOutcome {
        let now = self.clock.monotonic();
        let links = self.unconfirmed();
        let outcome = if links.is_empty() {
            ShutdownOutcome::ConfirmedOff
        } else {
            ShutdownOutcome::UnconfirmedOff { links }
        };

        for zone in &mut self.zones {
            zone.port.close();
        }
        if let Some(steam) = self.steam.as_mut() {
            steam.port.close();
        }
        let _ = self.stop_samplers.send(true);

        let deadline = now.checked_add(CLOSE_GRACE).unwrap_or(now);
        while self.any_port_open() {
            let nap = self.clock.sleep_until(deadline);
            let report = tokio::select! {
                report = self.reports.recv() => report,
                () = nap => None,
            };
            match report {
                Some(report) => {
                    if matches!(report, LinkReport::Closed { .. }) {
                        self.mark_closed(report.link());
                    }
                }
                None => break,
            }
        }

        let at = self.stamp(self.clock.monotonic());
        match &outcome {
            ShutdownOutcome::ConfirmedOff => {
                self.recorder.platform(
                    PlatformEvent::ServiceStopping,
                    "every link confirmed off".to_owned(),
                    at,
                );
                self.recorder.publish(ServiceEvent::Lifecycle {
                    what: Lifecycle::StoppedConfirmed,
                    detail: "every link confirmed off".to_owned(),
                });
            }
            ShutdownOutcome::UnconfirmedOff { links } => {
                let detail = format!(
                    "these links never confirmed themselves off within {} s: {}. The ports are \
                     closed; the valve's own communication-loss shutdown is what stops the water \
                     now. Check the outlet before anyone uses the shower.",
                    self.grace.as_secs(),
                    links
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                tracing::error!(detail = %detail, "shutdown could not confirm off");
                self.recorder
                    .platform(PlatformEvent::ServiceStopping, detail.clone(), at);
                self.recorder.publish(ServiceEvent::Lifecycle {
                    what: Lifecycle::StoppedUnconfirmed,
                    detail,
                });
            }
        }
        self.publish(self.clock.monotonic());
        outcome
    }

    fn any_port_open(&self) -> bool {
        self.zones.iter().any(|z| z.port.is_open())
            || self.steam.as_ref().is_some_and(|s| s.port.is_open())
    }

    // ---- housekeeping ----------------------------------------------------

    fn pet(&mut self, now: Monotonic) {
        let Some(interval) = self.watchdog.pet_interval() else {
            return;
        };
        if self.last_pet.is_some_and(|last| now.since(last) < interval) {
            return;
        }
        self.watchdog.pet();
        self.last_pet = Some(now);
        // Deliberately `tracing` only. A record every five seconds forever is
        // noise; the events worth a log line are a missing watchdog at startup,
        // which `announce_start` writes, and a missed pet, which by definition
        // the process that missed it cannot record.
        tracing::trace!("watchdog petted");
    }

    fn publish(&mut self, now: Monotonic) {
        let at = self.stamp(now);
        let zones: Vec<ZoneStatus> = self
            .zones
            .iter()
            .map(|zone| ZoneStatus {
                zone: zone.zone,
                kernel: LinkStateLabel::of(self.kernel.state(zone.link)),
                valve: zone.machine.cached().clone(),
                independent: zone.independent,
                frames_tx: zone.frames_tx,
                frames_rx: zone.frames_rx,
            })
            .collect();
        let steam = self.steam.as_ref().map(|steam| SteamStatus {
            kernel: LinkStateLabel::of(self.kernel.state(LinkKind::Steam)),
            adapter: steam.machine.cached().clone(),
            frames_tx: steam.frames_tx,
            frames_rx: steam.frames_rx,
        });
        let snapshot = SystemSnapshot {
            pi_boot: self.pi_boot.clone(),
            service_boot: self.recorder.boot(),
            zones,
            steam,
            shutting_down: matches!(self.mode, Mode::Draining { .. }),
            as_of: at,
        };
        let changed = self
            .last_published
            .as_ref()
            .is_none_or(|previous| differs(previous, &snapshot));
        self.cache.store(snapshot.clone());
        if changed {
            self.recorder
                .publish(ServiceEvent::State(Arc::new(snapshot.clone())));
        }
        self.last_published = Some(snapshot);
    }

    fn stamp(&self, now: Monotonic) -> Stamp {
        self.clock.wall().stamp(now)
    }

    fn zone_index(&self, zone: ZoneId) -> Option<usize> {
        self.zones.iter().position(|z| z.zone == zone)
    }

    fn count_tx(&mut self, link: LinkKind) {
        match link {
            LinkKind::Zone(id) => {
                if let Some(zone) = self.zones.iter_mut().find(|z| z.zone == id) {
                    zone.frames_tx = zone.frames_tx.saturating_add(1);
                }
            }
            LinkKind::Steam => {
                if let Some(steam) = self.steam.as_mut() {
                    steam.frames_tx = steam.frames_tx.saturating_add(1);
                }
            }
        }
    }

    fn close_port(&mut self, link: LinkKind) {
        match link {
            LinkKind::Zone(id) => {
                if let Some(zone) = self.zones.iter_mut().find(|z| z.zone == id) {
                    zone.port.close();
                }
            }
            LinkKind::Steam => {
                if let Some(steam) = self.steam.as_mut() {
                    steam.port.close();
                }
            }
        }
    }

    fn mark_closed(&mut self, link: LinkKind) {
        match link {
            LinkKind::Zone(id) => {
                if let Some(zone) = self.zones.iter_mut().find(|z| z.zone == id) {
                    zone.port.mark_closed();
                }
            }
            LinkKind::Steam => {
                if let Some(steam) = self.steam.as_mut() {
                    steam.port.mark_closed();
                }
            }
        }
    }
}

/// Answer a command without acting on it.
fn refuse(command: Command, why: &CommandError) {
    let answer = Err(why.clone());
    match command {
        Command::Start { reply, .. }
        | Command::Zone { reply, .. }
        | Command::Steam { reply, .. }
        | Command::StopAll { reply, .. }
        | Command::Acknowledge { reply, .. } => {
            let _ = reply.send(answer);
        }
    }
}

/// The channels the loop owns, grouped so the constructor stays readable.
#[derive(Debug)]
pub(crate) struct SupervisorChannels {
    pub(crate) commands: mpsc::Receiver<Command>,
    pub(crate) reports: mpsc::Receiver<LinkReport>,
    pub(crate) samples: mpsc::Receiver<Sampled>,
    pub(crate) stop_samplers: watch::Sender<bool>,
    pub(crate) shutdown: watch::Receiver<Option<&'static str>>,
}

/// Perform one zone step: transmit, log, apply effects, schedule.
///
/// The order is the engine's contract — the transmission goes first, then the
/// effects in order, which is what makes an all-off reach the valve before its
/// port is closed on the way into a latch.
///
/// Returns the encoder's refusal, when there was one, for the caller to
/// escalate. Escalating here is not possible: it needs the kernel, and the
/// kernel is the borrow this function was split out of.
fn apply_zone_step(
    zone: &mut ZoneRuntime,
    step: &Step,
    now: Monotonic,
    at: Stamp,
    recorder: &Recorder,
    source: Option<&RequestSource>,
) -> Option<String> {
    let mut denial = None;
    if step.tx.is_some() {
        match zone.machine.encode(&zone.encoder, step) {
            Some(Ok(frame)) => {
                zone.port.transmit(frame.bytes().to_vec());
                zone.awaiting_until = now.checked_add(zone.timings.response);
            }
            Some(Err(refused)) => denial = Some(refused.to_string()),
            None => {}
        }
    }

    for note in step.notes.iter().cloned() {
        let mut event = recorder.finish(note, at);
        match &mut event {
            LogEvent::Session(record) => {
                record.max_valve_reported_c = zone.max_valve_c.take();
                record.max_independent_corrected_c = zone.max_independent_c.take();
            }
            LogEvent::Command(_) => {
                if let Some(source) = source {
                    Recorder::attribute(&mut event, source);
                }
            }
            LogEvent::Rejected(record) => recorder.publish(ServiceEvent::Refused {
                command: record.command,
                reason: record.reason.clone(),
            }),
            _ => {}
        }
        recorder.emit(event);
    }

    for effect in &step.effects {
        apply_effect(effect, zone.link, &mut zone.port, recorder);
    }

    zone.last_refusal.clone_from(&step.refused);
    zone.wake_at = step.deadline;
    if zone.machine.cached().water_moving
        && let Some(celsius) = zone.machine.cached().valve_reported_c
    {
        zone.max_valve_c = Some(zone.max_valve_c.map_or(celsius, |m| m.max(celsius)));
    }
    denial
}

/// The steam link's half of [`apply_zone_step`], on the same contract.
fn apply_steam_step(
    steam: &mut SteamRuntime,
    step: &SteamStep,
    now: Monotonic,
    at: Stamp,
    recorder: &Recorder,
    source: Option<&RequestSource>,
) -> Option<String> {
    let mut denial = None;
    if let Some(op) = step.tx.as_ref() {
        match steam.machine.encode(&steam.encoder, step) {
            Some(Ok(frame)) => {
                steam.port.transmit(frame.bytes().to_vec());
                steam.awaiting_until = now.checked_add(steam_wait(op, &steam.timings));
            }
            Some(Err(refused)) => denial = Some(refused.to_string()),
            None => {}
        }
    }

    for note in step.notes.iter().cloned() {
        let mut event = recorder.finish(note, at);
        match &mut event {
            LogEvent::Command(_) => {
                if let Some(source) = source {
                    Recorder::attribute(&mut event, source);
                }
            }
            LogEvent::Rejected(record) => recorder.publish(ServiceEvent::Refused {
                command: record.command,
                reason: record.reason.clone(),
            }),
            _ => {}
        }
        recorder.emit(event);
    }

    for effect in &step.effects {
        apply_effect(effect, LinkKind::Steam, &mut steam.port, recorder);
    }

    steam.last_refusal.clone_from(&step.refused);
    steam.wake_at = step.deadline;
    denial
}

/// Perform one kernel effect. **Apply, do not interpret.**
///
/// `AllOff`, `Latch` and `SteamStopThenLatch` are the machines' to drive and
/// have already been driven by the step that carried them; what is left for the
/// service is the port, the operator and the finding log.
fn apply_effect(effect: &Effect, link: LinkKind, port: &mut PortHandle, recorder: &Recorder) {
    match effect {
        // Not advisory. The pump holds the link by value, so this drops it.
        // Ordered behind the transmission above, which is why an all-off still
        // reaches the valve.
        Effect::ClosePort(named) if *named == link => port.close(),
        Effect::OperatorMessage { link: named, text } => {
            tracing::error!(link = %named, text = %text, "operator action required");
            recorder.publish(ServiceEvent::OperatorMessage {
                link: *named,
                text: (*text).to_owned(),
            });
        }
        Effect::RecordFinding(class) => recorder.publish(ServiceEvent::Finding {
            class: *class,
            detail: format!("{link}: {class:?}"),
        }),
        Effect::AllOff(_)
        | Effect::SteamStopThenLatch
        | Effect::Latch { .. }
        | Effect::ClosePort(_) => {}
    }
}

/// How long to wait for an answer on the steam link.
///
/// The address opportunity has its own deadline in every source and it is not
/// the device reply timeout, which is the same distinction the steam machine
/// makes when it schedules itself.
fn steam_wait(op: &SteamOp, timings: &DtvTimings) -> Duration {
    match op {
        SteamOp::Discovery(DiscoveryStep::AddressOpportunity) => timings.address_enquiry_timeout,
        _ => timings.reply,
    }
}

/// Two snapshots differ in something other than when they were taken.
fn differs(previous: &SystemSnapshot, next: &SystemSnapshot) -> bool {
    previous.shutting_down != next.shutting_down
        || previous.zones != next.zones
        || previous.steam != next.steam
}

fn zone_command_id(command: &OperatorCommand) -> CommandId {
    match command {
        OperatorCommand::SetTemperature { command, .. }
        | OperatorCommand::Pause { command }
        | OperatorCommand::Resume { command }
        | OperatorCommand::Stop { command } => *command,
    }
}

fn steam_command_id(command: &SteamCommand) -> CommandId {
    match command {
        SteamCommand::Start { command, .. }
        | SteamCommand::Stop { command }
        | SteamCommand::SetTemperature { command, .. }
        | SteamCommand::SetDuration { command, .. } => *command,
    }
}
