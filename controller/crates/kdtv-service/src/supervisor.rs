//! The control loop: one task, one kernel, three links.
//!
//! # What a pass of the loop does
//!
//! It waits on the earliest of four things — a byte report from a pump, a
//! command from the API, the shutdown signal, and the deadline the engine last
//! asked to be woken on — turns it into an
//! event for the machine it concerns, transmits whatever the resulting
//! [`Step`] says to transmit, performs the kernel's
//! [`Effect`]s in order, pets the watchdog and publishes the cache.
//!
//! A deadline that is already due is served **before** the wait rather than as
//! one arm of it, alternating with it. The select is `biased` and the timer is
//! its last arm, so an arm that is always ready — a link reporting failures
//! faster than they can be consumed — would mean the timer was never polled:
//! no tick, no response timeout, no session expiry, and the watchdog petted
//! throughout, because the loop really was completing
//! passes. Everything that keeps water bounded is behind that deadline, so
//! nothing else may be allowed to displace it indefinitely.
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
//! for, floored by [`HOUSEKEEPING`] so a latched service still publishes. A
//! deadline already past is not slept on
//! and not caught up: exactly one event is delivered, because two ticks in a
//! pass would put two frames on a bus that allows one. Lateness beyond a tick is
//! logged as a platform event rather than absorbed.
//!
//! # One transaction in flight, per link
//!
//! A step carries at most one `tx` and a machine emits none of its own while it
//! has one outstanding, so for everything the engine generates the property is
//! the engine's. What this module adds is the bookkeeping that tells a tick from
//! a response timeout: `awaiting_until` is set when a frame is queued and
//! cleared when the answer, the failure or the timeout is delivered.
//!
//! An **operator command** is the exception the engine makes deliberately:
//! `ZoneMachine::on_command` abandons the outstanding transaction and sends in
//! its place, so a command's frame can go out inside the previous frame's
//! response window. Two consequences are this module's to handle, and both are
//! below — the rate of commands would otherwise be the rate of frames
//! (`Supervisor::refuse_zone_command`), and the abandoned transaction's reply
//! must not be read as the replacement's (`apply_zone_step`).

use std::sync::Arc;
use std::time::Duration;

use kdtv_engine::{
    OperatorCommand, Refusal, StartRequest, SteamCommand, SteamEvent, SteamMachine, SteamPhaseKind,
    SteamRefusal, SteamStep, Step, ZoneEvent, ZoneMachine, ZonePhaseKind,
};
use kdtv_hal::{Clock, Watchdog};
use kdtv_proto::dtv::{DiscoveryStep, DtvRxBuffer, DtvTimings, SteamEncoder, SteamOp};
use kdtv_proto::saturn::{Encoder, Expectation, MasterAddr, RxBuffer, Timings};
use kdtv_safety::{Effect, SafetyEvent, SafetyKernel};
use kdtv_telemetry::{Direction, LogEvent, Monotonic, PlatformEvent, RequestSource, Stamp};
use kdtv_units::{CommandId, LinkKind, PiBootId, ZoneId};
use tokio::sync::{mpsc, watch};

use crate::cache::{LinkStateLabel, StateCache, SteamStatus, SystemSnapshot, ZoneStatus};
use crate::command::{Command, CommandError};
use crate::event::{Lifecycle, ServiceEvent};
use crate::port::{LinkReport, PortHandle, Transmitted};
use crate::record::Recorder;

/// The longest the loop will sleep with nothing else to do.
///
/// It bounds two things: how stale a published snapshot can be, and how long a
/// latched service can go without petting the watchdog. Half of the deployed
/// 10 s `WatchdogSec`.
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
    commands: bool,
}

impl Default for Live {
    fn default() -> Self {
        Self {
            reports: true,
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
    /// When the outstanding response is due, if one is.
    awaiting_until: Option<Monotonic>,
    /// When the machine asked to be stepped again.
    wake_at: Option<Monotonic>,
    /// When a frame this link transmitted last came from an operator command
    /// rather than from the machine's own cadence. What
    /// [`Supervisor::refuse_zone_command`] paces.
    last_command_tx: Option<Monotonic>,
    frames_tx: u64,
    frames_rx: u64,
    /// The highest reading seen since the last session record. `LOG-08`.
    max_valve_c: Option<f32>,
    last_refusal: Option<Refusal>,
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
            awaiting_until: None,
            wake_at: Some(Monotonic::from_nanos(0)),
            last_command_tx: None,
            frames_tx: 0,
            frames_rx: 0,
            max_valve_c: None,
            last_refusal: None,
            overflowed: 0,
        }
    }

    /// This service has **confirmation** the valve is closed.
    ///
    /// `ReadyOff` is the only phase that carries it, because it is reached only
    /// through an acknowledged all-off. Everything else is a belief about what
    /// this service did, not about what the valve did:
    ///
    /// - `Cold`, `Discovery` and `Identify` mean the boot sequence has not
    ///   reached its confirmed all-off yet. Nothing this *instance* opened, but
    ///   a previous instance or a watchdog reset can have left an outlet open,
    ///   and the safe boot sequence reaches `READY_OFF` "only after both zones
    ///   are confirmed off" for exactly that reason.
    /// - `Unavailable` sends an all-off and closes the port in the same step, so
    ///   the acknowledgement never arrives and nothing here knows what the valve
    ///   did with it.
    fn confirmed_off(&self) -> bool {
        matches!(self.machine.phase().kind(), ZonePhaseKind::ReadyOff)
            && !self.machine.cached().water_moving
    }

    /// The boot sequence has not finished on this link.
    ///
    /// Not confirmation of anything — it is what tells an unconfirmed shutdown
    /// during boot from one where a valve stopped answering with water on.
    fn still_booting(&self) -> bool {
        matches!(
            self.machine.phase().kind(),
            ZonePhaseKind::Cold | ZonePhaseKind::Discovery | ZonePhaseKind::Identify
        )
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
    /// As [`ZoneRuntime::last_command_tx`].
    last_command_tx: Option<Monotonic>,
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
            last_command_tx: None,
            frames_tx: 0,
            frames_rx: 0,
            last_refusal: None,
        }
    }

    /// On the same terms as [`ZoneRuntime::confirmed_off`].
    fn confirmed_off(&self) -> bool {
        matches!(self.machine.phase().kind(), SteamPhaseKind::ReadyOff)
            && !self.machine.cached().steaming
    }

    /// On the same terms as [`ZoneRuntime::still_booting`].
    fn still_booting(&self) -> bool {
        matches!(
            self.machine.phase().kind(),
            SteamPhaseKind::Cold | SteamPhaseKind::Discovery
        )
    }

    fn settled(&self) -> bool {
        self.confirmed_off() || !self.port.is_open()
    }
}

/// What woke the loop.
enum Woke {
    Report(Option<LinkReport>),
    Command(Option<Command>),
    /// Someone asked the service to stop. `false` means nobody did: the last
    /// [`crate::ShutdownTrigger`] was dropped, which resolves `changed()` with
    /// an error. Draining is still the only correct answer — nothing can ask
    /// again — but it is not a shutdown request and is not logged as one.
    Shutdown {
        requested: bool,
    },
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
    pub(crate) shutdown: watch::Receiver<Option<&'static str>>,
    pub(crate) pi_boot: PiBootId,
    pub(crate) shutdown_command: CommandId,
    pub(crate) grace: Duration,
    /// What was bound to each link, as the factory described it, for the one
    /// record that says so. Empty when the caller assembled the loop from pipes
    /// it built itself.
    opened: Vec<(LinkKind, String)>,
    mode: Mode,
    /// Which of the loop's channels are still live.
    live: Live,
    /// Which links had not finished the boot sequence when the drain began.
    /// Recorded there rather than read at the end, because the drain's own stop
    /// moves a booting machine into `ConfirmOff`.
    booting_at_drain: Vec<LinkKind>,
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
        opened: Vec<(LinkKind, String)>,
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
            shutdown: channels.shutdown,
            pi_boot,
            shutdown_command,
            grace: SHUTDOWN_GRACE,
            opened,
            mode: Mode::Serving,
            live: Live::default(),
            booting_at_drain: Vec::new(),
            last_pet: None,
            last_published: None,
            escalating: false,
        }
    }

    /// Run until the shutdown signal, then stop water and confirm it.
    pub async fn run(mut self) -> ShutdownOutcome {
        self.announce_start();
        // True for exactly one pass after a deadline was served ahead of the
        // select, so the two alternate. Without it a deadline that is
        // permanently in the past would starve the channels the same way the
        // channels used to starve the deadline.
        let mut deadline_served = false;
        loop {
            let now = self.clock.monotonic();
            if let Mode::Draining { until } = self.mode
                && (self.all_settled() || now >= until)
            {
                break;
            }

            // A deadline that is already due is served before anything is
            // waited on. The select below is `biased`, so an arm that is always
            // ready — a pump reporting failures faster than they can be
            // consumed, or a client keeping the command channel full — would
            // otherwise mean the timer arm is never polled and `on_deadline`
            // never runs: no tick, no response timeout and no session expiry,
            // with the watchdog petted throughout. Stepping the deadline first
            // bounds all of them by one pass.
            if !deadline_served && now >= self.next_wake(now) {
                deadline_served = true;
                self.on_deadline(now);
                self.pet(now);
                self.publish(now);
                continue;
            }
            deadline_served = false;

            // A closed channel resolves immediately and forever, so each arm is
            // disabled once it has ended. Without that the loop would spin at
            // the speed of the scheduler the moment the last pump exited, which
            // is precisely when it most needs to be servicing the drain.
            let Live { reports, commands } = self.live;
            let serving = matches!(self.mode, Mode::Serving);
            let nap = self.clock.sleep_until(self.next_wake(now));
            let woke = tokio::select! {
                biased;
                report = self.reports.recv(), if reports => Woke::Report(report),
                command = self.commands.recv(), if commands => Woke::Command(command),
                changed = self.shutdown.changed(), if serving => Woke::Shutdown {
                    requested: changed.is_ok(),
                },
                () = nap => Woke::Deadline,
            };

            let now = self.clock.monotonic();
            match woke {
                Woke::Report(Some(report)) => self.on_report(report, now),
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
                Woke::Shutdown { requested: true } => {
                    let reason = (*self.shutdown.borrow_and_update()).unwrap_or("shutdown");
                    self.begin_drain(reason, now);
                }
                // Nobody asked. Every trigger was dropped, so nobody ever can,
                // and a service that cannot be stopped deliberately stops now —
                // but the log says which of the two happened.
                Woke::Shutdown { requested: false } => {
                    self.begin_drain(
                        "every shutdown trigger was dropped without being pulled",
                        now,
                    );
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
        for (link, descriptor) in self.opened.clone() {
            // `LOG-07`. The bare `tracing` line the factory writes has no boot
            // ids and no NTP-paired stamp, and the close of the same port
            // produces both — so which device was bound to which link was the
            // one thing the durable log could not answer.
            self.recorder.platform(
                PlatformEvent::SerialOpened,
                format!("{link}: {descriptor}"),
                at,
            );
        }
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
                    // The escalation goes first. It is what produces the
                    // all-off, and `PortHandle::transmit` drops a frame on a
                    // port already marked closed — so closing here first was
                    // the one place in this crate that inverted the
                    // transmit-then-close order `apply_effect` exists to keep.
                    // The escalation's own `Effect::ClosePort` does the close in
                    // that order; the call below is the belt and braces for a
                    // machine that did not escalate, and is idempotent.
                    self.drive_port_closed(link, now);
                    self.close_port(link);
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
    /// whose effects name a link other than the one that raised them, and none
    /// of them can be observed by a machine, so the fan-out lives here. Each
    /// machine applies only what names its own link and writes its own log
    /// line, which is one line per link and exactly what the log wants.
    ///
    /// Only [`SafetyEvent::ServiceFailure`] is raised today. The other three are
    /// unclaimed, and it is worth being exact about why rather than leaving them
    /// looking accidental:
    ///
    /// - `ConfigCheckFailed` — configuration is validated in `kdtv-config` and
    ///   the composition root refuses to start on it, so no running loop can
    ///   observe one.
    /// - `WatchdogMissed` — by definition the process that missed a pet cannot
    ///   record it; `announce_start` logs the *absence* of a watchdog as a
    ///   platform event, which is a different thing.
    /// - `UsbControllerLost` — a terminal [`kdtv_hal::LinkIoError`] arrives per
    ///   link, and this service escalates it per link as
    ///   [`SafetyEvent::PortLost`]. In the reference configuration all three
    ///   links are interfaces of one USB device, so an enumeration loss *is*
    ///   shared — but that is a fact about one installation's wiring, not
    ///   something a report on one link proves, and `SVC-01` requires a zone 1
    ///   fault to leave zone 2 driving. The other links learn of it through
    ///   their own next transaction. `[I]` — that a terminal error on one
    ///   interface implies the others have gone is inference, and the service
    ///   does not act on it.
    ///
    /// This is the one path that can put a second frame on a bus inside the same
    /// loop pass as a poll: it is reached from
    /// [`Supervisor::on_encode_denied`], which can fire on a zone that has
    /// already transmitted in this pass, and the kernel's shared response
    /// carries no `ClosePort` to suppress it. Accepted deliberately — the second
    /// frame is the all-off, and it goes last, which is the ordering that gets
    /// water stopped. Everything else that could double up is ordered so that it
    /// cannot; see [`Supervisor::on_deadline`].
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
                let answer = self.stop_all(command, &source, now, true);
                let _ = reply.send(answer);
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

    /// Pace commanded transmissions on one valve bus.
    ///
    /// `BUS-01` allows one transaction in flight per link, and the engine keeps
    /// to it for everything it generates itself — but an operator command
    /// abandons whatever was outstanding and sends in its place, so the *rate*
    /// of commands is the rate of frames. Nothing else bounds it: `refuse_command`
    /// gates on phase, `authorize_open` on authority, and a `ServiceHandle`
    /// holder in a loop would put frames on a 9600-baud bus as fast as a tokio
    /// task can call. The floor is the link's own response deadline from
    /// `kdtv-proto`, never a number invented here: it is exactly how long the
    /// previous commanded transaction has to be answered in.
    ///
    /// A `Stop` on a zone that is not already stopping is exempt. Closing a
    /// valve is never made to wait, and a second stop while the first is
    /// confirming adds nothing but a frame.
    ///
    /// `None` means the command may proceed. `Some` refuses it, having recorded
    /// the reason (`LOG-04`); nothing was transmitted.
    fn refuse_zone_command(
        &self,
        index: usize,
        command: Option<&OperatorCommand>,
        id: CommandId,
        now: Monotonic,
    ) -> Option<CommandError> {
        let zone = self.zones.get(index)?;
        let stopping_now = matches!(command, Some(OperatorCommand::Stop { .. }))
            && !matches!(zone.machine.phase().kind(), ZonePhaseKind::ConfirmOff);
        if stopping_now {
            return None;
        }
        let last = zone.last_command_tx?;
        if now.since(last) >= zone.timings.response {
            return None;
        }
        let why = CommandError::TooSoon { link: zone.link };
        self.recorder
            .rejection(id, why.to_string(), "command pacing", self.stamp(now));
        Some(why)
    }

    /// [`Supervisor::refuse_zone_command`] for the steam link, on its own reply
    /// deadline. A stop is exempt while there is anything to stop.
    fn refuse_steam_command(
        &self,
        command: &SteamCommand,
        id: CommandId,
        now: Monotonic,
    ) -> Option<CommandError> {
        let steam = self.steam.as_ref()?;
        let stopping_now = matches!(command, SteamCommand::Stop { .. })
            && matches!(steam.machine.phase().kind(), SteamPhaseKind::Running);
        if stopping_now {
            return None;
        }
        let last = steam.last_command_tx?;
        if now.since(last) >= steam.timings.reply {
            return None;
        }
        let why = CommandError::TooSoon {
            link: LinkKind::Steam,
        };
        self.recorder
            .rejection(id, why.to_string(), "command pacing", self.stamp(now));
        Some(why)
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
        if let Some(why) = self.refuse_zone_command(index, None, id, now) {
            return Err(why);
        }
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
        if let Some(why) = self.refuse_zone_command(index, Some(&command), id, now) {
            return Err(why);
        }
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
        if let Some(why) = self.refuse_steam_command(&command, id, now) {
            return Err(why);
        }
        self.drive_steam(SteamEvent::Command(command), now, Some(source));
        match self.steam.as_ref().and_then(|s| s.last_refusal.clone()) {
            Some(refusal) => Err(CommandError::SteamRefused(refusal)),
            None => Ok(id),
        }
    }

    /// Stop every link, and say whether anything took it.
    ///
    /// The one operation an operator reaches for in an emergency used to answer
    /// `Ok` unconditionally, so "both links commanded off" and "nothing was sent
    /// on any link" were the same answer. A refusal on one link with another
    /// still accepting is not a failure of `stop_all` — the shower is stopping —
    /// so the error is reserved for the case where every link refused. Each
    /// individual refusal reaches the log as the machine's own `Rejected` note
    /// either way.
    ///
    /// `paced` applies [`Supervisor::refuse_zone_command`]. An operator's
    /// `stop_all` is paced like any other command; the shutdown drain is not,
    /// because its whole job is to command a stop on every link once.
    fn stop_all(
        &mut self,
        command: CommandId,
        source: &RequestSource,
        now: Monotonic,
        paced: bool,
    ) -> Result<CommandId, CommandError> {
        let stop = OperatorCommand::Stop { command };
        let mut accepted = 0_usize;
        let mut refused: Option<CommandError> = None;
        for index in 0..self.zones.len() {
            if paced && let Some(why) = self.refuse_zone_command(index, Some(&stop), command, now) {
                refused = refused.or(Some(why));
                continue;
            }
            if let Some(zone) = self.zones.get_mut(index) {
                zone.last_refusal = None;
            }
            self.drive_zone(index, ZoneEvent::Command(stop.clone()), now, Some(source));
            match self.zones.get(index).and_then(|z| z.last_refusal.clone()) {
                Some(refusal) => refused = refused.or(Some(CommandError::ZoneRefused(refusal))),
                None => accepted = accepted.saturating_add(1),
            }
        }
        if self.steam.is_some() {
            let stop = SteamCommand::Stop { command };
            if let Some(why) = paced
                .then(|| self.refuse_steam_command(&stop, command, now))
                .flatten()
            {
                refused = refused.or(Some(why));
            } else {
                if let Some(steam) = self.steam.as_mut() {
                    steam.last_refusal = None;
                }
                self.drive_steam(SteamEvent::Command(stop), now, Some(source));
                match self.steam.as_ref().and_then(|s| s.last_refusal.clone()) {
                    Some(refusal) => {
                        refused = refused.or(Some(CommandError::SteamRefused(refusal)));
                    }
                    None => accepted = accepted.saturating_add(1),
                }
            }
        }
        match refused {
            Some(why) if accepted == 0 => Err(why),
            _ => Ok(command),
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
        self.booting_at_drain = self.still_booting();
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
        // The answer is the outcome's business, not this function's: a link that
        // refuses the stop is a link that never confirms itself off, and
        // `unconfirmed` names it.
        let _ = self.stop_all(command, &source, now, false);
    }

    /// Every link has either confirmed itself off or lost the port it would
    /// have been told on. Waiting past that point buys nothing: a closed port
    /// cannot be commanded, and the outcome already names it as unconfirmed.
    fn all_settled(&self) -> bool {
        self.zones.iter().all(ZoneRuntime::settled)
            && self.steam.as_ref().is_none_or(SteamRuntime::settled)
    }

    /// The links whose boot sequence never finished. A subset of
    /// [`Supervisor::unconfirmed`], and the reason it says what it says.
    fn still_booting(&self) -> Vec<LinkKind> {
        let mut out: Vec<LinkKind> = self
            .zones
            .iter()
            .filter(|z| z.still_booting())
            .map(|z| z.link)
            .collect();
        if self.steam.as_ref().is_some_and(SteamRuntime::still_booting) {
            out.push(LinkKind::Steam);
        }
        out
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

        // Waited on the *pumps*, not on `is_open`: `PortHandle::close` clears
        // that flag synchronously, so a loop keyed on it exited before it began
        // and this wait did nothing. The orders queued by the last drain pass —
        // an all-off, or a retried stop — are written by the pump after the
        // close it is ordered behind, so returning here early meant the runtime
        // dropped the pump with that frame still in its queue.
        let deadline = now.checked_add(CLOSE_GRACE).unwrap_or(now);
        while self.any_pump_running() {
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
                // A stop during the boot sequence is unconfirmed for a
                // different reason from a valve that went quiet with water on,
                // and the two must not read the same. Both are genuinely
                // unconfirmed — the boot sequence exists because this service
                // does not know what a valve is doing until it has been told —
                // but only one of them means a session was interrupted.
                let booting: Vec<String> = self
                    .booting_at_drain
                    .iter()
                    .filter(|link| links.contains(link))
                    .map(ToString::to_string)
                    .collect();
                let note = if booting.is_empty() {
                    String::new()
                } else {
                    format!(
                        " {} had not finished the boot sequence when the stop was commanded, so \
                         this service had never established what that valve was doing; nothing it \
                         opened is running.",
                        booting.join(", ")
                    )
                };
                let detail = format!(
                    "these links never confirmed themselves off within {} s: {}. The ports are \
                     closed; the valve's own communication-loss shutdown is what stops the water \
                     now. Check the outlet before anyone uses the shower.{note}",
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

    fn any_pump_running(&self) -> bool {
        self.zones.iter().any(|z| z.port.is_running())
            || self.steam.as_ref().is_some_and(|s| s.port.is_running())
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
                // A transmission while one was already outstanding is the
                // engine abandoning that transaction and sending in its place —
                // the only way it emits two. Whatever is in the receive buffer
                // at this instant was therefore addressed to the frame that was
                // just given up on, and cannot be an answer to one not yet on
                // the wire. This service owns the buffer, so this is where it
                // goes: leaving it would let the abandoned reply be decoded
                // inside the replacement's response window and accepted as its
                // acknowledgement, which for two operations sharing a control
                // byte is indistinguishable. A reply already in flight is not
                // reachable from here and stays with the engine's own late- and
                // stale-response checks.
                if zone.awaiting_until.is_some() {
                    zone.rx.clear();
                }
                if zone.port.transmit(frame.bytes().to_vec()) == Transmitted::PumpGone {
                    let link = zone.link;
                    recorder.platform(
                        PlatformEvent::SerialError,
                        format!(
                            "{link}: a frame was not queued because the byte pump for this link \
                             has gone without closing the port"
                        ),
                        at,
                    );
                }
                zone.awaiting_until = now.checked_add(zone.timings.response);
                if source.is_some() {
                    zone.last_command_tx = Some(now);
                }
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
                // As in [`apply_zone_step`].
                if steam.awaiting_until.is_some() {
                    steam.rx.clear();
                }
                if steam.port.transmit(frame.bytes().to_vec()) == Transmitted::PumpGone {
                    recorder.platform(
                        PlatformEvent::SerialError,
                        format!(
                            "{}: a frame was not queued because the byte pump for this link has \
                             gone without closing the port",
                            LinkKind::Steam
                        ),
                        at,
                    );
                }
                steam.awaiting_until = now.checked_add(steam_wait(op, &steam.timings));
                if source.is_some() {
                    steam.last_command_tx = Some(now);
                }
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
