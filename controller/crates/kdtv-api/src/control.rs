//! What the API needs from the service, named as a trait.
//!
//! [`kdtv_service::ServiceHandle`] is the production implementation and the only
//! one that ships. The trait exists for one reason: `ServiceHandle::new` is
//! crate-private, so a handler test that wanted a real handle would have to
//! stand up a supervisor, three link pumps and two RTD samplers to assert that a
//! request without a token reaches no handler. A narrow trait makes those tests
//! possible without giving anything up — the trait names exactly the five things
//! the router calls and nothing else, so it cannot become a second, wider way
//! into the service.
//!
//! Every method is on the `ServiceHandle` already. Nothing is added here.

use std::fmt;
use std::future::Future;
use std::sync::Arc;

use kdtv_service::surface::{OperatorCommand, StartAuthorization, SteamCommand, ValidatedStart};
use kdtv_service::{CommandError, ServiceEvent, ServiceHandle, SystemSnapshot};
use kdtv_telemetry::RequestSource;
use kdtv_units::{BootId, CommandId, ZoneId};
use tokio::sync::broadcast;

/// The service, as the router sees it.
///
/// `Clone + Send + Sync + 'static` because it is axum state: every request
/// handler gets its own clone. [`ServiceHandle`] is cheap to clone and every
/// clone talks to the same supervisor.
pub trait Control: Clone + Send + Sync + 'static {
    /// This service boot's id, for [`crate::auth::FreshCaller::authorize`].
    fn boot(&self) -> BootId;

    /// The cached state. **No bus transaction, no channel, no lock.** `API-06`.
    fn snapshot(&self) -> Arc<SystemSnapshot>;

    /// The read-only event stream. A [`broadcast::Receiver`] has no method that
    /// sends anything, so it is read-only by construction. `SVC-05`.
    fn subscribe(&self) -> broadcast::Receiver<ServiceEvent>;

    /// Open water on a zone. The authorisation is taken by value and spent.
    fn start(
        &self,
        request: ValidatedStart,
        authorization: StartAuthorization,
        source: RequestSource,
    ) -> impl Future<Output = Result<CommandId, CommandError>> + Send;

    /// Change, pause, resume or stop a running zone.
    fn zone(
        &self,
        zone: ZoneId,
        command: OperatorCommand,
        source: RequestSource,
    ) -> impl Future<Output = Result<CommandId, CommandError>> + Send;

    /// Command the steam link.
    fn steam(
        &self,
        command: SteamCommand,
        source: RequestSource,
    ) -> impl Future<Output = Result<CommandId, CommandError>> + Send;

    /// Stop both zones and steam. `API-03`.
    fn stop_all(
        &self,
        command: CommandId,
        source: RequestSource,
    ) -> impl Future<Output = Result<CommandId, CommandError>> + Send;
}

impl Control for ServiceHandle {
    fn boot(&self) -> BootId {
        Self::boot(self)
    }

    fn snapshot(&self) -> Arc<SystemSnapshot> {
        Self::snapshot(self)
    }

    fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        Self::subscribe(self)
    }

    fn start(
        &self,
        request: ValidatedStart,
        authorization: StartAuthorization,
        source: RequestSource,
    ) -> impl Future<Output = Result<CommandId, CommandError>> + Send {
        Self::start(self, request, authorization, source)
    }

    fn zone(
        &self,
        zone: ZoneId,
        command: OperatorCommand,
        source: RequestSource,
    ) -> impl Future<Output = Result<CommandId, CommandError>> + Send {
        Self::zone(self, zone, command, source)
    }

    fn steam(
        &self,
        command: SteamCommand,
        source: RequestSource,
    ) -> impl Future<Output = Result<CommandId, CommandError>> + Send {
        Self::steam(self, command, source)
    }

    fn stop_all(
        &self,
        command: CommandId,
        source: RequestSource,
    ) -> impl Future<Output = Result<CommandId, CommandError>> + Send {
        Self::stop_all(self, command, source)
    }
}

/// Where a command id comes from.
///
/// Command ids are minted by `kdtv_hal::IdStore`, which persists and `fsync`s
/// the counter **before** issuing a value, so a crash can only skip ids forward
/// and never reuse one. This crate cannot reach that store — it does not depend
/// on `kdtv-hal` — and must not mint ids of its own, because an id reused across
/// a restart makes a frame log unreadable exactly when it is needed.
///
/// So the daemon passes one in. The trait is the whole of the coupling.
pub trait CommandIds: Send + Sync + fmt::Debug {
    /// The next command id, durable before it is returned.
    fn next(&self) -> Result<CommandId, IdUnavailable>;
}

/// The command id counter could not issue.
///
/// Carried as a rendered string rather than as `kdtv_hal::IdError`, because this
/// crate does not depend on `kdtv-hal` and should not gain the dependency to
/// name one error.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error("the command id counter is unavailable: {0}")]
pub struct IdUnavailable(pub String);
