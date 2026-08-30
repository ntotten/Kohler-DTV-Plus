//! The command surface the API layer calls. `SVC-05`.
//!
//! **This crate builds the surface, not the HTTP.** `kdtv-api` is a separate
//! crate; it holds a [`ServiceHandle`], calls typed methods on it, and
//! subscribes to a stream. It never sees a frame, a link or the kernel — a
//! dependency-graph check in `cargo xtask audit-graph` denies it even the
//! ability to name a wire type.
//!
//! # How an authorisation crosses
//!
//! [`StartAuthorization`] is minted by the API layer from an authenticated
//! request and consumed by [`kdtv_safety::SafetyKernel::authorize_open`], which
//! is the only thing in the workspace that mints an
//! [`OpenGrant`](kdtv_safety::OpenGrant). It is `!Clone` and taken by value, so
//! it is spent exactly once — and the channel that carries it has to move it
//! exactly once too. [`Command::Start`] owns it by value; a bounded
//! [`tokio::sync::mpsc`] moves it into the supervisor, which moves it into the
//! kernel. There is no point at which two copies exist, and no `Clone` to make
//! one.
//!
//! It also carries the boot id it was minted under, so a service restart
//! invalidates every outstanding token. That is what stops a restart replaying a
//! start.
//!
//! # A status read is not a command
//!
//! [`ServiceHandle::snapshot`] does not go through this channel. It reads the
//! [`crate::StateCache`] directly, so an external client hammering status
//! changes the transmitted frame count by zero. `API-06`.

use std::sync::Arc;

use kdtv_engine::{OperatorCommand, Refusal, SteamCommand, SteamRefusal};
use kdtv_safety::{Denial, OperatorAck, StartAuthorization, ValidatedStart};
use kdtv_telemetry::RequestSource;
use kdtv_units::{BootId, CommandId, LinkKind, ZoneId};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::cache::{StateCache, SystemSnapshot};
use crate::event::ServiceEvent;

/// How many commands may be queued for the supervisor.
///
/// Small on purpose. A caller that has filled this has already sent more
/// commands than a shower can act on, and back-pressure at the API is a better
/// answer than a queue of stale intentions reaching a valve.
pub const COMMAND_CAPACITY: usize = 16;

/// Why a command did not take effect.
///
/// A refusal transmits nothing and changes no valve state, which is the
/// distinction the design draws between invalid input and invalid wire data:
/// bad input is rejected to the caller, bad wire data escalates to all-off.
///
/// Not `Eq`: a [`Denial`] can carry a temperature divergence in degrees, and a
/// float has no total equality. Comparing two denials for exact equality is not
/// something a caller should be doing anyway.
#[derive(Clone, PartialEq, Debug, thiserror::Error)]
pub enum CommandError {
    /// The safety kernel refused to authorise water.
    #[error("safety: {0}")]
    Denied(#[from] Denial),
    /// The zone machine refused. Distinct from a denial: the kernel answers
    /// "may this zone open", the machine answers "does this machine agree".
    #[error("zone: {0}")]
    ZoneRefused(#[from] Refusal),
    #[error("steam: {0}")]
    SteamRefused(#[from] SteamRefusal),
    /// Steam is not configured on this system, so there is nothing to command.
    #[error("{0} is not configured on this service")]
    NoSuchLink(LinkKind),
    /// A command arrived before the previous commanded transaction on that link
    /// could finish. **Nothing was transmitted**; the caller may retry.
    ///
    /// `kdtv_engine::ZoneMachine` abandons an outstanding transaction when a
    /// command arrives and sends the command in its place, which is right for
    /// one command and wrong for a hundred a second: the bus correlates a
    /// response with its request by there being one. `SVC-02` fixes this
    /// service's own cadence, and `API-06` keeps status reads off the wire, but
    /// nothing paced commands — which is `INVESTIGATIONS.md` I1 reached through
    /// a different door. A stop is never refused this way while there is
    /// anything to stop.
    #[error("{link}: the previous command on this bus has not finished")]
    TooSoon { link: LinkKind },
    /// The zone's independent temperature channel has never produced a sample,
    /// so the interlock that would catch a wrong valve thermistor is not there
    /// to catch it. `SAFE-05`.
    #[error("{0}: the independent temperature channel has never produced a sample")]
    NoIndependentReading(ZoneId),
    /// A stop has already been commanded on every link and the service is
    /// waiting for the confirmations. Nothing new is accepted from here.
    #[error("the service is shutting down")]
    ShuttingDown,
    /// The supervisor has gone. Nothing was transmitted.
    #[error("the service is not running")]
    NotRunning,
}

/// One instruction for the supervisor.
///
/// Every variant carries the [`RequestSource`] that asked, because "who asked
/// for this" is the first question after an unexpected event and `LOG-01`
/// requires it on every command record.
#[derive(Debug)]
pub enum Command {
    /// Open water on a zone. **The only variant that can.**
    ///
    /// The authorisation is owned, not borrowed: it is spent by the call it
    /// authorises, and a reference would let one authorisation open water twice.
    Start {
        request: ValidatedStart,
        authorization: StartAuthorization,
        source: RequestSource,
        reply: oneshot::Sender<Result<CommandId, CommandError>>,
    },
    /// Change, pause, resume or stop a running zone. There is no `SetOutlets`:
    /// changing which outlets are open is opening water, so it needs a grant,
    /// and the kernel refuses to mint one for a zone that is already running.
    Zone {
        zone: ZoneId,
        command: OperatorCommand,
        source: RequestSource,
        reply: oneshot::Sender<Result<CommandId, CommandError>>,
    },
    Steam {
        command: SteamCommand,
        source: RequestSource,
        reply: oneshot::Sender<Result<CommandId, CommandError>>,
    },
    /// Stop both zones and steam. One operator action, one command id, three
    /// links.
    StopAll {
        command: CommandId,
        source: RequestSource,
        reply: oneshot::Sender<Result<CommandId, CommandError>>,
    },
    /// Acknowledge a latched link. **This does not reopen anything**: the port
    /// closed when the link latched, and coming back means going through
    /// discovery again. Recovery is never automatic.
    Acknowledge {
        link: LinkKind,
        ack: OperatorAck,
        source: RequestSource,
        reply: oneshot::Sender<Result<CommandId, CommandError>>,
    },
}

/// The API layer's end of the service.
///
/// Cheap to clone; every clone talks to the same supervisor and reads the same
/// cache.
#[derive(Clone, Debug)]
pub struct ServiceHandle {
    commands: mpsc::Sender<Command>,
    cache: Arc<StateCache>,
    events: broadcast::Sender<ServiceEvent>,
    boot: BootId,
}

impl ServiceHandle {
    pub(crate) const fn new(
        commands: mpsc::Sender<Command>,
        cache: Arc<StateCache>,
        events: broadcast::Sender<ServiceEvent>,
        boot: BootId,
    ) -> Self {
        Self {
            commands,
            cache,
            events,
            boot,
        }
    }

    /// This service boot's id.
    ///
    /// The API layer needs it to mint a [`StartAuthorization`], and the kernel
    /// refuses one from any other boot. That is the whole of "a restart cannot
    /// replay a start".
    #[must_use]
    pub const fn boot(&self) -> BootId {
        self.boot
    }

    /// The current state. **No bus transaction, no channel, no lock.**
    ///
    /// `API-06`. This is one atomic load and an [`Arc`] clone; calling it a
    /// million times leaves the wire traffic exactly as it was.
    #[must_use]
    pub fn snapshot(&self) -> Arc<SystemSnapshot> {
        self.cache.load()
    }

    /// Subscribe to the read-only event stream. `SVC-05`.
    ///
    /// A [`broadcast::Receiver`] has no method that sends anything, so the
    /// stream is read-only by construction rather than by check. A subscriber
    /// that falls behind is told how far, and never blocks the control loop.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.events.subscribe()
    }

    /// Open water on a zone.
    pub async fn start(
        &self,
        request: ValidatedStart,
        authorization: StartAuthorization,
        source: RequestSource,
    ) -> Result<CommandId, CommandError> {
        let (reply, answer) = oneshot::channel();
        self.dispatch(
            Command::Start {
                request,
                authorization,
                source,
                reply,
            },
            answer,
        )
        .await
    }

    /// Change, pause, resume or stop a zone.
    pub async fn zone(
        &self,
        zone: ZoneId,
        command: OperatorCommand,
        source: RequestSource,
    ) -> Result<CommandId, CommandError> {
        let (reply, answer) = oneshot::channel();
        self.dispatch(
            Command::Zone {
                zone,
                command,
                source,
                reply,
            },
            answer,
        )
        .await
    }

    /// Command the steam link.
    pub async fn steam(
        &self,
        command: SteamCommand,
        source: RequestSource,
    ) -> Result<CommandId, CommandError> {
        let (reply, answer) = oneshot::channel();
        self.dispatch(
            Command::Steam {
                command,
                source,
                reply,
            },
            answer,
        )
        .await
    }

    /// Stop both zones and steam. `API-03`.
    pub async fn stop_all(
        &self,
        command: CommandId,
        source: RequestSource,
    ) -> Result<CommandId, CommandError> {
        let (reply, answer) = oneshot::channel();
        self.dispatch(
            Command::StopAll {
                command,
                source,
                reply,
            },
            answer,
        )
        .await
    }

    /// Acknowledge a latched link.
    pub async fn acknowledge(
        &self,
        link: LinkKind,
        ack: OperatorAck,
        source: RequestSource,
    ) -> Result<CommandId, CommandError> {
        let (reply, answer) = oneshot::channel();
        self.dispatch(
            Command::Acknowledge {
                link,
                ack,
                source,
                reply,
            },
            answer,
        )
        .await
    }

    async fn dispatch(
        &self,
        command: Command,
        answer: oneshot::Receiver<Result<CommandId, CommandError>>,
    ) -> Result<CommandId, CommandError> {
        self.commands
            .send(command)
            .await
            .map_err(|_| CommandError::NotRunning)?;
        answer.await.map_err(|_| CommandError::NotRunning)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::SystemSnapshot;
    use kdtv_telemetry::{Monotonic, NtpSync, Stamp};
    use kdtv_units::PiBootId;

    fn handle() -> (ServiceHandle, mpsc::Receiver<Command>) {
        let (tx, rx) = mpsc::channel(COMMAND_CAPACITY);
        let (events, _) = broadcast::channel(8);
        let snapshot = SystemSnapshot::empty(
            PiBootId("boot-uuid".into()),
            BootId(2),
            Stamp::new(Monotonic::from_nanos(0), 0, NtpSync::Unknown),
        );
        let cache = Arc::new(StateCache::new(snapshot));
        (ServiceHandle::new(tx, cache, events, BootId(2)), rx)
    }

    /// `API-06`, at the handle. A read does not touch the command channel, so
    /// it cannot become a bus transaction however hard it is called.
    #[tokio::test]
    async fn a_status_read_never_reaches_the_command_channel() {
        let (h, mut commands) = handle();
        for _ in 0..10_000 {
            let snapshot = h.snapshot();
            assert_eq!(snapshot.frames_tx(), 0);
        }
        assert!(
            commands.try_recv().is_err(),
            "a status read must send nothing"
        );
    }

    #[tokio::test]
    async fn a_command_sent_to_a_supervisor_that_has_gone_reports_it_rather_than_hanging() {
        let (h, commands) = handle();
        drop(commands);
        let err = h
            .zone(
                ZoneId::Zone1,
                OperatorCommand::Stop {
                    command: CommandId(1),
                },
                RequestSource::Cli {
                    peer: "operator".into(),
                },
            )
            .await
            .expect_err("no supervisor");
        assert_eq!(err, CommandError::NotRunning);
    }

    #[tokio::test]
    async fn a_supervisor_that_drops_the_reply_channel_is_not_running_either() {
        let (h, mut commands) = handle();
        let task = tokio::spawn(async move {
            h.zone(
                ZoneId::Zone1,
                OperatorCommand::Pause {
                    command: CommandId(1),
                },
                RequestSource::Cli {
                    peer: "operator".into(),
                },
            )
            .await
        });
        let command = commands.recv().await.expect("a command");
        drop(command);
        assert_eq!(task.await.unwrap(), Err(CommandError::NotRunning));
    }

    /// The authorisation is moved, never copied. This is the compiled statement
    /// of that: after the command is built the local is gone, and there is no
    /// `Clone` that could have kept it.
    #[test]
    fn a_start_authorisation_is_moved_into_the_command_exactly_once() {
        let (reply, _answer) = oneshot::channel();
        let authorization = StartAuthorization::issue(BootId(2), CommandId(5));
        let command = Command::Start {
            request: ValidatedStart {
                zone: ZoneId::Zone1,
                outlets: kdtv_units::SlotSet::EMPTY,
                temperature: kdtv_units::ValveSetpoint::try_new(kdtv_units::Cx2::from_raw(76))
                    .unwrap(),
                duration: kdtv_units::SessionDuration::clamped(std::time::Duration::from_secs(300)),
                command: CommandId(5),
            },
            authorization,
            source: RequestSource::Cli {
                peer: "operator".into(),
            },
            reply,
        };
        let Command::Start { authorization, .. } = command else {
            panic!("expected a start");
        };
        assert_eq!(authorization.command(), CommandId(5));
        assert_eq!(authorization.boot(), BootId(2));
    }
}
