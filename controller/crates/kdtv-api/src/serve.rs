//! Binding the socket and serving the router.
//!
//! # Loopback, and why it is not a preference
//!
//! `OPS-04`: the controller has no authentication of its own, so anything that
//! can reach this API can run the shower. `kdtv_config::ApiConfig` has already
//! refused a non-loopback `api.bind`, and [`Api::bind`] checks it again before
//! it opens a socket — a second check on the one property whose failure exposes
//! a shower to a network costs nothing and does not depend on the first check
//! still being there in a year.
//!
//! # Shutdown
//!
//! [`Api::serve`] takes the future that ends it. The daemon passes one that
//! resolves when the service has stopped water, so the API stops answering at
//! the same time the supervisor stops accepting — a client cannot get a start
//! accepted into a service that is draining, and if one arrives it is refused
//! with `CommandError::ShuttingDown` rather than transmitted.
//!
//! **The wait for open connections is bounded.** ~~`serve` returned whatever
//! `axum::serve(..).with_graceful_shutdown(..)` returned.~~ Superseded: axum's
//! graceful shutdown waits for every open connection with no timeout, and the
//! `SVC-05` event stream is a connection that never ends — its
//! `BroadcastStream` finishes only when the `broadcast::Sender` closes, and
//! that sender lives inside the `ServiceHandle` held by the router inside the
//! task being waited on. One Homebridge client on `GET /v1/events` therefore
//! deadlocked the daemon's only clean exit path. The water was already off by
//! then, so nothing was left running; what was destroyed was the **exit code**,
//! because `TimeoutStopSec=20s` turned every shutdown into `SIGKILL` and
//! `exit 5` — the `UnconfirmedOff` code whose remedy is a person removing valve
//! power — became `status=9/KILL`, indistinguishable from a slow stop.
//!
//! So after shutdown is signalled this waits [`SHUTDOWN_GRACE`] for connections
//! to finish and then returns regardless. The bound is on the wait, not on the
//! event stream, because a never-ending response body is not the only shape of
//! this: a half-sent request or a connection the peer has stopped reading would
//! do the same.

use std::future::IntoFuture as _;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::FromRef;
use kdtv_config::{ApiConfig, Bounds};
use kdtv_units::CommandId;
use tokio::net::TcpListener;

use crate::auth::{ApiToken, Authenticator, Sessions};
use crate::control::{CommandIds, Control};
use crate::error::ApiError;
use crate::routes::router;

/// Everything a handler needs.
///
/// Cloned per request, so every field is cheap to clone.
#[derive(Debug)]
pub struct ApiState<C: Control> {
    control: C,
    ids: Arc<dyn CommandIds>,
    bounds: Bounds,
    authenticator: Arc<Authenticator>,
}

impl<C: Control> Clone for ApiState<C> {
    fn clone(&self) -> Self {
        Self {
            control: self.control.clone(),
            ids: Arc::clone(&self.ids),
            bounds: self.bounds,
            authenticator: Arc::clone(&self.authenticator),
        }
    }
}

impl<C: Control> ApiState<C> {
    /// Assemble the state.
    ///
    /// `bounds` come from the validated configuration: the tighter of each
    /// compiled-in constant and anything configured. Nothing in a file can
    /// widen one, and the API refuses against the resolved value.
    #[must_use]
    pub fn new(
        control: C,
        ids: Arc<dyn CommandIds>,
        bounds: Bounds,
        token: ApiToken,
        session_ttl: std::time::Duration,
    ) -> Self {
        Self {
            control,
            ids,
            bounds,
            authenticator: Arc::new(Authenticator::new(token, Sessions::new(session_ttl))),
        }
    }

    pub(crate) const fn control(&self) -> &C {
        &self.control
    }

    pub(crate) const fn bounds(&self) -> Bounds {
        self.bounds
    }

    pub(crate) fn authenticator(&self) -> Arc<Authenticator> {
        Arc::clone(&self.authenticator)
    }

    /// The next command id, or a refusal.
    ///
    /// A failure here means the counter could not be made durable, and a
    /// command id that is not durable is one a restart could reissue. Nothing is
    /// attempted without one.
    pub(crate) fn next_command(&self) -> Result<CommandId, ApiError> {
        self.ids
            .next()
            .map_err(|e| ApiError::NoCommandId(e.to_string()))
    }
}

impl<C: Control> FromRef<ApiState<C>> for Arc<Authenticator> {
    fn from_ref(state: &ApiState<C>) -> Self {
        Arc::clone(&state.authenticator)
    }
}

/// How long [`Api::serve`] waits for open connections once shutdown has been
/// signalled.
///
/// Well under `deploy/kdtvd.service`'s `TimeoutStopSec=20s`, so the daemon
/// exits on its own terms with its own exit code rather than being killed —
/// which is the whole point, since `exit 5` for an unconfirmed off is the one
/// outcome that needs a person.
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// Why the API could not bind.
#[derive(Debug, thiserror::Error)]
pub enum BindError {
    #[error(
        "api.bind = {bind} is not a loopback address; the controller has no authentication \
         of its own, so anything that can reach this API can run the shower"
    )]
    NotLoopback { bind: SocketAddr },
    #[error("cannot bind the API to {bind}")]
    Io {
        bind: SocketAddr,
        #[source]
        source: std::io::Error,
    },
}

/// A bound, not yet serving, API.
///
/// Split from [`Api::serve`] so the daemon can fail on a busy port **before**
/// it tells the service manager it is ready. A `Type=notify` unit that reports
/// ready and then cannot answer is worse than one that never reports at all.
#[derive(Debug)]
pub struct Api {
    listener: TcpListener,
    router: Router,
}

impl Api {
    /// Bind the configured address and build the router.
    pub async fn bind<C: Control>(
        config: &ApiConfig,
        state: ApiState<C>,
    ) -> Result<Self, BindError> {
        let bind = config.bind();
        if !bind.ip().is_loopback() {
            return Err(BindError::NotLoopback { bind });
        }
        let listener = TcpListener::bind(bind)
            .await
            .map_err(|source| BindError::Io { bind, source })?;
        Ok(Self {
            listener,
            router: router(state),
        })
    }

    /// The address actually bound. Port 0 in a test resolves here.
    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serve until `shutdown` resolves, and for at most [`SHUTDOWN_GRACE`]
    /// after that.
    ///
    /// `into_make_service_with_connect_info` is what gives every command record
    /// its `peer` (`LOG-01`): "who asked for this" is the first question after
    /// an unexpected event.
    ///
    /// The grace period is why this returns at all — see the module docs. A
    /// connection still open when it expires is dropped: by that point the
    /// supervisor has already commanded every link off, so there is nothing
    /// left for a client to be told, and the exit code the daemon owes
    /// `systemd` is worth more than a clean `FIN` on an event stream.
    pub async fn serve<F>(self, shutdown: F) -> std::io::Result<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let (announce, signalled) = tokio::sync::oneshot::channel();
        let served = axum::serve(
            self.listener,
            self.router
                .into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            shutdown.await;
            let _ = announce.send(());
        })
        .into_future();
        let grace = async move {
            if signalled.await.is_err() {
                // The sender went without sending, which cannot happen while
                // the shutdown future is alive. Wait forever rather than treat
                // it as a signal to stop serving.
                std::future::pending::<()>().await;
            }
            tokio::time::sleep(SHUTDOWN_GRACE).await;
        };

        tokio::pin!(served);
        tokio::select! {
            result = &mut served => result,
            () = grace => {
                tracing::warn!(
                    grace_s = SHUTDOWN_GRACE.as_secs(),
                    "a connection was still open when the API shutdown grace period expired; \
                     closing it. The event stream is the usual reason and the water is already \
                     off by here."
                );
                Ok(())
            }
        }
    }
}
