//! The composition root: everything that has to be true before the loop runs.
//!
//! Ports are opened here, and only here. The order is deliberate and each step
//! refuses rather than degrading:
//!
//! 1. **Identity.** The service boot id is minted and made durable before
//!    anything else, because every authorisation to open water is bound to it
//!    and a restart must invalidate the outstanding ones.
//! 2. **Ports.** Each configured link is opened through the
//!    [`LinkFactory`], which runs the transmit gate's second boundary: a real
//!    serial backend is refused unless that link's fixtures are captured and its
//!    bus polarity attested. With today's evidence base — every fixture tier
//!    `[C]` — that refusal is the expected outcome and not a fault.
//! 3. **Bounds.** The kernel is built from the resolved configuration bounds —
//!    the tighter of the compiled-in constant and anything configured. Nothing
//!    in a file can widen one.
//!
//! Nothing is opened until the identity is durable, and if any link fails to
//! open, the ones already opened are closed on the way out. There is no
//! partially-bound start.
//!
//! # Boot state is off
//!
//! No water state is persisted and none is restored. Every machine begins
//! `Cold` and walks discovery, identity and a **confirmed** all-off before it
//! will accept a start. That is what makes a watchdog reset safe: the worst it
//! can do is stop the shower.

use std::sync::Arc;

use kdtv_config::ValidatedConfig;
use kdtv_engine::{RetryBudget, SteamMachine, SteamSettings, ZoneMachine, ZoneSettings};
use kdtv_hal::{Clock, IdError, IdStore, Link, LinkFactory, OpenError, PortBinding, Watchdog};
use kdtv_proto::TransmitAuthority;
use kdtv_proto::dtv::SteamEncoder;
use kdtv_proto::saturn::Encoder;
use kdtv_safety::{Bounds, SafetyKernel};
use kdtv_telemetry::Stamp;
use kdtv_units::{LinkKind, SessionDuration, ZoneId};
use tokio::sync::{broadcast, mpsc, watch};

use crate::cache::{StateCache, SystemSnapshot};
use crate::command::{COMMAND_CAPACITY, ServiceHandle};
use crate::event::EVENT_CAPACITY;
use crate::port::{self, LinkReport, Pipe};
use crate::record::Recorder;
use crate::supervisor::{SteamRuntime, Supervisor, SupervisorChannels, ZoneRuntime};

/// How many link reports may queue before a pump waits.
///
/// Generous relative to the traffic — three links at their tick produce single
/// digits per second — so a pump never stalls on a supervisor that is busy for
/// one pass.
const REPORT_CAPACITY: usize = 256;

/// Why the service did not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    /// The boot or command counter could not be made durable. Starting without
    /// one would mean a restart could replay a start.
    #[error("identity: {0}")]
    Ids(#[from] IdError),
    /// A port did not open. [`OpenError::is_gate`] distinguishes the transmit
    /// gate — the committed position of this project — from a fault.
    #[error("link: {0}")]
    Open(#[from] OpenError),
    /// The configuration names a link the resolver produced no binding for.
    /// There is no degraded start: a service that cannot be sure which physical
    /// valve is on which bus must not drive either of them.
    #[error("{0} has no resolved port binding")]
    Unbound(LinkKind),
}

/// The platform pieces the service needs and does not build.
///
/// Every one is a trait object so a test can supply a deterministic stand-in
/// without a serial port, an SPI bus or a systemd socket.
#[derive(Debug)]
pub struct Deps {
    pub clock: Arc<dyn Clock>,
    pub watchdog: Arc<dyn Watchdog>,
}

/// What a successful start hands back.
#[derive(Debug)]
pub struct Started {
    /// For the API layer.
    pub handle: ServiceHandle,
    /// The control loop, not yet running. The caller decides whether to
    /// `await` it or spawn it, which is what lets a test drive it on a
    /// `current_thread` runtime with time paused.
    pub supervisor: Supervisor,
    /// What asks the loop to stop water and exit.
    pub shutdown: ShutdownTrigger,
}

/// Asks the service to stop.
///
/// Cloneable so a signal handler, a test and the daemon can all hold one; the
/// first trigger wins and later ones are ignored by the loop.
#[derive(Clone, Debug)]
pub struct ShutdownTrigger {
    sender: Arc<watch::Sender<Option<&'static str>>>,
}

impl ShutdownTrigger {
    /// Ask the loop to command every link off and exit. `reason` reaches the
    /// log and the event stream.
    pub fn trigger(&self, reason: &'static str) {
        let _ = self.sender.send(Some(reason));
    }
}

/// Wire `SIGTERM` and `SIGINT` to the shutdown trigger.
///
/// An abrupt exit that leaves a valve open is the worst outcome this system
/// has, so neither signal is allowed to end the process directly: both ask the
/// loop to stop water, and the loop exits when it has confirmed it or when the
/// grace period says it never will.
///
/// Returns the spawned task's handle so a caller that wants to can abort it.
pub fn install_signal_handlers(
    trigger: ShutdownTrigger,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    Ok(tokio::spawn(async move {
        tokio::select! {
            _ = term.recv() => trigger.trigger("SIGTERM"),
            _ = interrupt.recv() => trigger.trigger("SIGINT"),
        }
    }))
}

/// Opens everything and assembles the loop.
#[derive(Debug)]
pub struct Service;

impl Service {
    /// Start the service against real links.
    ///
    /// `bindings` come from [`kdtv_hal::resolve_distinct`], which has already
    /// refused a configuration whose ports are absent, colliding or ambiguous.
    pub async fn start(
        config: &ValidatedConfig,
        authority: &TransmitAuthority,
        bindings: &[PortBinding],
        factory: &mut dyn LinkFactory,
        ids: &dyn IdStore,
        deps: Deps,
    ) -> Result<Started, StartError> {
        let mut opened: Vec<(LinkKind, Box<dyn Link>)> = Vec::new();
        for link in config.links() {
            let Some(binding) = bindings.iter().find(|b| b.link() == link) else {
                close_all(opened).await;
                return Err(StartError::Unbound(link));
            };
            match factory.open(binding, authority).await {
                Ok(port) => {
                    // The structured record is written by the supervisor, which
                    // is the first thing here that has a boot id and a clock.
                    tracing::info!(descriptor = %port.descriptor(), "opened");
                    opened.push((link, port));
                }
                Err(why) => {
                    close_all(opened).await;
                    return Err(StartError::Open(why));
                }
            }
        }

        // Kept so the boot record can name what was bound to what. `LOG-07`
        // wants serial events in the log and the close of a port already
        // produces one; the open produced only a `tracing` line with no boot ids
        // and no NTP-paired stamp, which on a Pi with no RTC is the one record
        // that could not be trusted or correlated.
        let mut descriptors: Vec<(LinkKind, String)> = Vec::with_capacity(opened.len());
        let pipes = opened
            .into_iter()
            .map(|(link, port)| {
                descriptors.push((link, port.descriptor().to_string()));
                let pipe: Box<dyn Pipe> = Box::new(port);
                (link, pipe)
            })
            .collect();
        assemble(config, authority, pipes, ids, deps, descriptors)
    }
}

async fn close_all(opened: Vec<(LinkKind, Box<dyn Link>)>) {
    for (link, port) in opened {
        if let Err(why) = port.close().await {
            tracing::warn!(link = %link, error = %why, "closing a port after a failed start");
        }
    }
}

/// The half of the start that has no I/O in it, so a test can reach it with
/// pipes it built itself.
pub(crate) fn assemble(
    config: &ValidatedConfig,
    authority: &TransmitAuthority,
    pipes: Vec<(LinkKind, Box<dyn Pipe>)>,
    ids: &dyn IdStore,
    deps: Deps,
    opened: Vec<(LinkKind, String)>,
) -> Result<Started, StartError> {
    // Identity first, and durable before anything is opened: every
    // authorisation to open water is bound to this boot id, so a restart has to
    // invalidate the outstanding ones before a port exists to act on one.
    let boot = ids.begin_boot()?;
    let pi_boot = ids.pi_boot_id()?;
    let shutdown_command = ids.next_command()?;

    let (events, _) = broadcast::channel(EVENT_CAPACITY);
    let recorder = Recorder::new(
        pi_boot.clone(),
        boot,
        events.clone(),
        config.logging().frames(),
    );

    let session_cap = SessionDuration::clamped(config.scaled_max_session());
    let kernel = SafetyKernel::new(boot, bounds(config, session_cap));

    let (reports_tx, reports) = mpsc::channel::<LinkReport>(REPORT_CAPACITY);
    let mut pipes = pipes;
    let mut zones = Vec::with_capacity(ZoneId::ALL.len());
    for id in ZoneId::ALL {
        zones.push(zone_runtime(
            config,
            authority,
            id,
            session_cap,
            take_pipe(&mut pipes, LinkKind::Zone(id))?,
            &reports_tx,
        ));
    }
    let steam = match config.steam() {
        None => None,
        Some(steam_config) => Some(SteamRuntime::new(
            SteamMachine::new(SteamSettings::from_config(steam_config, session_cap)),
            SteamEncoder::new(authority),
            port::spawn(
                LinkKind::Steam,
                take_pipe(&mut pipes, LinkKind::Steam)?,
                reports_tx.clone(),
            ),
            steam_config.timings(),
        )),
    };
    drop(reports_tx);

    let (commands_tx, commands) = mpsc::channel(COMMAND_CAPACITY);
    let (shutdown_tx, shutdown_rx) = watch::channel(None);

    let now = deps.clock.monotonic();
    let at: Stamp = deps.clock.wall().stamp(now);
    let cache = Arc::new(StateCache::new(SystemSnapshot::empty(
        pi_boot.clone(),
        boot,
        at,
    )));

    let handle = ServiceHandle::new(commands_tx, Arc::clone(&cache), events, boot);
    let supervisor = Supervisor::assemble(
        kernel,
        zones,
        steam,
        recorder,
        deps.clock,
        deps.watchdog,
        cache,
        SupervisorChannels {
            commands,
            reports,
            shutdown: shutdown_rx,
        },
        pi_boot,
        shutdown_command,
        opened,
    );

    Ok(Started {
        handle,
        supervisor,
        shutdown: ShutdownTrigger {
            sender: Arc::new(shutdown_tx),
        },
    })
}

/// The resolved bounds the kernel enforces: the tighter of each compiled-in
/// constant and anything configured. Nothing in a file can widen one.
fn bounds(config: &ValidatedConfig, session_cap: SessionDuration) -> Bounds {
    Bounds {
        session_cap,
        configured_outlets: [
            (ZoneId::Zone1, config.zone(ZoneId::Zone1).configured_slots()),
            (ZoneId::Zone2, config.zone(ZoneId::Zone2).configured_slots()),
        ],
    }
}

/// Take the pipe belonging to one link, or refuse. There is no degraded start.
fn take_pipe(
    pipes: &mut Vec<(LinkKind, Box<dyn Pipe>)>,
    link: LinkKind,
) -> Result<Box<dyn Pipe>, StartError> {
    let index = pipes
        .iter()
        .position(|(kind, _)| *kind == link)
        .ok_or(StartError::Unbound(link))?;
    Ok(pipes.remove(index).1)
}

fn zone_runtime(
    config: &ValidatedConfig,
    authority: &TransmitAuthority,
    id: ZoneId,
    session_cap: SessionDuration,
    pipe: Box<dyn Pipe>,
    reports: &mpsc::Sender<LinkReport>,
) -> ZoneRuntime {
    let link = LinkKind::Zone(id);
    let zone_config = config.zone(id);
    let timings = config.timing().saturn();
    let settings = ZoneSettings {
        timings,
        retry: RetryBudget::from_saturn(&timings),
        session_cap,
        ..ZoneSettings::default()
    };
    ZoneRuntime::new(
        id,
        ZoneMachine::new(zone_config, settings),
        Encoder::new(
            authority,
            link,
            zone_config.master(),
            zone_config.outlets().clone(),
        ),
        port::spawn(link, pipe, reports.clone()),
        zone_config.master(),
        timings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shutdown_trigger_can_be_pulled_from_more_than_one_place() {
        let (sender, mut receiver) = watch::channel(None);
        let trigger = ShutdownTrigger {
            sender: Arc::new(sender),
        };
        let clone = trigger.clone();
        clone.trigger("SIGTERM");
        assert!(receiver.has_changed().unwrap_or(false));
        assert_eq!(*receiver.borrow_and_update(), Some("SIGTERM"));
        // A second trigger is harmless; the loop ignores it once draining.
        trigger.trigger("SIGINT");
        assert_eq!(*receiver.borrow_and_update(), Some("SIGINT"));
    }
}
