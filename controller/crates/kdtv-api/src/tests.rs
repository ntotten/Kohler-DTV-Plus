//! The properties this crate exists to hold, driven against the real router.
//!
//! Every test below runs the router that [`crate::router`] builds, through
//! `tower::ServiceExt::oneshot`, with the real middleware stack. What is
//! replaced is the service behind it: [`FakeControl`] records what the router
//! asked for instead of driving three serial links.
//!
//! **Why a fake and not a `ServiceHandle`.** `ServiceHandle::new` is
//! crate-private in `kdtv-service`, so obtaining a real one means starting a
//! supervisor and three link pumps. The assertions here are
//! about the boundary — what reaches a handler, what a handler sends on, and
//! what it never sends — and a fake is the only way to see "nothing reached the
//! service" at all. What the service does *with* a command is tested in
//! `kdtv-service` against the real supervisor.
//!
//! **What a green run does not prove.** Nothing here has been exercised against
//! a valve, and every wire frame in this workspace is tier `[C]`.
//!
//! # A limit worth naming
//!
//! `SystemSnapshot::zones` holds `kdtv_engine::ZoneCache` values and
//! `SystemSnapshot::steam` a `kdtv_engine::SteamCache`, and this crate cannot
//! name `kdtv_engine` — that absence is the point of the dependency audit. So
//! every snapshot the fake returns has `zones: Vec::new(), steam: None`, and
//! `SystemSnapshot::frames_tx` sums exactly those two: it is **structurally
//! zero here, whatever the router does**.
//!
//! ~~`API-06` was therefore asserted twice over: the frame count does not move,
//! and no command reached the service.~~ Superseded — the first of those was
//! `0 == 0` and held for any implementation, including one that transmitted on
//! every read. The frame-count form of `API-06` is asserted where a populated
//! snapshot exists, in `kdtv-service`'s
//! `a_status_read_never_reaches_the_command_channel`. What is asserted here is
//! the boundary: no command, no command id, no subscription, and exactly one
//! cache read per request.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use kdtv_config::{Bounds, BoundsRequest};
use kdtv_service::surface::{OperatorCommand, StartAuthorization, SteamCommand, ValidatedStart};
use kdtv_service::{CommandError, ServiceEvent, SystemSnapshot};
use kdtv_telemetry::{Monotonic, NtpSync, RequestSource, Stamp};
use kdtv_units::{BootId, CommandId, Cx2, PiBootId, ValveSetpoint, ZoneId};
use tokio::sync::broadcast;
use tower::ServiceExt as _;

use crate::auth::{ApiToken, Authenticator, SESSION_HEADER, Sessions, authenticate};
use crate::control::{CommandIds, Control, IdUnavailable};
use crate::routes::{Op, Requirement, router, surface};
use crate::serve::{Api, ApiState, SHUTDOWN_GRACE};

/// A 32-byte credential, the sort `systemd-creds` produces.
const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const BOOT: BootId = BootId(7);

// ------------------------------------------------------------------- fakes

/// What the router asked the service to do.
#[derive(Clone, PartialEq, Debug)]
enum Recorded {
    Start {
        request: ValidatedStart,
        authorized_boot: BootId,
        authorized_command: CommandId,
        source: RequestSource,
    },
    Zone {
        zone: ZoneId,
        command: OperatorCommand,
        source: RequestSource,
    },
    Steam {
        command: SteamCommand,
        source: RequestSource,
    },
    StopAll {
        command: CommandId,
        source: RequestSource,
    },
}

#[derive(Debug, Default)]
struct Ledger {
    calls: Mutex<Vec<Recorded>>,
    snapshot_reads: AtomicU64,
    subscriptions: AtomicU64,
    refuse: Mutex<Option<CommandError>>,
}

/// The service, replaced by a notebook.
#[derive(Clone, Debug)]
struct FakeControl {
    ledger: Arc<Ledger>,
    events: broadcast::Sender<ServiceEvent>,
}

impl FakeControl {
    fn new() -> Self {
        let (events, _) = broadcast::channel(8);
        Self {
            ledger: Arc::new(Ledger::default()),
            events,
        }
    }

    fn calls(&self) -> Vec<Recorded> {
        self.ledger
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(&self, what: Recorded) -> Result<CommandId, CommandError> {
        if let Some(refusal) = self
            .ledger
            .refuse
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return Err(refusal);
        }
        let id = match &what {
            Recorded::Start { request, .. } => request.command,
            Recorded::Zone { command, .. } => match command {
                OperatorCommand::SetTemperature { command, .. }
                | OperatorCommand::Pause { command }
                | OperatorCommand::Resume { command }
                | OperatorCommand::Stop { command } => *command,
            },
            Recorded::Steam { command, .. } => match command {
                SteamCommand::Start { command, .. }
                | SteamCommand::Stop { command }
                | SteamCommand::SetTemperature { command, .. }
                | SteamCommand::SetDuration { command, .. } => *command,
            },
            Recorded::StopAll { command, .. } => *command,
        };
        self.ledger
            .calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(what);
        Ok(id)
    }

    fn refuse_with(&self, error: CommandError) {
        *self
            .ledger
            .refuse
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(error);
    }
}

impl Control for FakeControl {
    fn boot(&self) -> BootId {
        BOOT
    }

    fn snapshot(&self) -> Arc<SystemSnapshot> {
        self.ledger.snapshot_reads.fetch_add(1, Ordering::Relaxed);
        Arc::new(SystemSnapshot {
            pi_boot: PiBootId("boot-uuid".into()),
            service_boot: BOOT,
            zones: Vec::new(),
            steam: None,
            shutting_down: false,
            as_of: Stamp::new(
                Monotonic::from_nanos(1),
                1_756_500_000,
                NtpSync::Synchronised,
            ),
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<ServiceEvent> {
        self.ledger.subscriptions.fetch_add(1, Ordering::Relaxed);
        self.events.subscribe()
    }

    async fn start(
        &self,
        request: ValidatedStart,
        authorization: StartAuthorization,
        source: RequestSource,
    ) -> Result<CommandId, CommandError> {
        self.record(Recorded::Start {
            request,
            authorized_boot: authorization.boot(),
            authorized_command: authorization.command(),
            source,
        })
    }

    async fn zone(
        &self,
        zone: ZoneId,
        command: OperatorCommand,
        source: RequestSource,
    ) -> Result<CommandId, CommandError> {
        self.record(Recorded::Zone {
            zone,
            command,
            source,
        })
    }

    async fn steam(
        &self,
        command: SteamCommand,
        source: RequestSource,
    ) -> Result<CommandId, CommandError> {
        self.record(Recorded::Steam { command, source })
    }

    async fn stop_all(
        &self,
        command: CommandId,
        source: RequestSource,
    ) -> Result<CommandId, CommandError> {
        self.record(Recorded::StopAll { command, source })
    }
}

/// A command id counter. The real one persists and `fsync`s before it issues;
/// this one only has to be monotonic.
#[derive(Debug, Default)]
struct FakeIds {
    next: AtomicU64,
    broken: AtomicU64,
}

impl CommandIds for FakeIds {
    fn next(&self) -> Result<CommandId, IdUnavailable> {
        if self.broken.load(Ordering::Relaxed) != 0 {
            return Err(IdUnavailable("the state directory is read-only".into()));
        }
        Ok(CommandId(self.next.fetch_add(1, Ordering::Relaxed) + 1))
    }
}

// --------------------------------------------------------------------- rig

struct Rig {
    router: Router,
    control: FakeControl,
    ids: Arc<FakeIds>,
}

/// One answered request.
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Answer {
    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap_or(serde_json::Value::Null)
    }

    fn session(&self) -> Option<String> {
        self.headers
            .get(SESSION_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned)
    }
}

fn test_token() -> ApiToken {
    ApiToken::from_bytes(std::path::Path::new("/test/token"), TOKEN.as_bytes())
        .expect("a valid test credential")
}

impl Rig {
    fn new() -> Self {
        Self::build(Duration::from_secs(900), Bounds::COMPILED)
    }

    fn with_ttl(ttl: Duration) -> Self {
        Self::build(ttl, Bounds::COMPILED)
    }

    /// A router whose configured bounds are narrower than the compiled ones,
    /// which is the only way to see the difference between the two clamps.
    fn with_bounds(bounds: Bounds) -> Self {
        Self::build(Duration::from_secs(900), bounds)
    }

    fn build(ttl: Duration, bounds: Bounds) -> Self {
        let control = FakeControl::new();
        let ids = Arc::new(FakeIds::default());
        let state = ApiState::new(
            control.clone(),
            Arc::<FakeIds>::clone(&ids),
            bounds,
            test_token(),
            ttl,
        );
        Self {
            router: router(state),
            control,
            ids,
        }
    }

    async fn send(&self, request: Request<Body>) -> Answer {
        let response = self
            .router
            .clone()
            .oneshot(request)
            .await
            .expect("the router is infallible");
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .expect("a bounded body");
        Answer {
            status,
            headers,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    /// An authenticated request with no session presented.
    async fn call(&self, method: &str, uri: &str, body: Option<&str>) -> Answer {
        self.call_as(method, uri, body, None, Some(TOKEN)).await
    }

    async fn call_as(
        &self,
        method: &str,
        uri: &str,
        body: Option<&str>,
        session: Option<&str>,
        token: Option<&str>,
    ) -> Answer {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            builder = builder.header("authorization", format!("Bearer {t}"));
        }
        if let Some(s) = session {
            builder = builder.header(SESSION_HEADER, s);
        }
        let request = match body {
            Some(json) => builder
                .header("content-type", "application/json")
                .body(Body::from(json.to_owned())),
            None => builder.body(Body::empty()),
        }
        .expect("a well-formed request");
        self.send(request).await
    }

    /// Establish a session the way a client does — any authenticated request —
    /// and hand back its id, so the next call can open water.
    async fn establish(&self) -> String {
        self.call("GET", "/v1/state", None)
            .await
            .session()
            .expect("every authenticated response carries a session id")
    }
}

fn ok_start_body() -> &'static str {
    r#"{"outlet_set":[1,2],"temperature_f":100.0,"duration_seconds":600}"#
}

/// A refusal that changed nothing, whichever of the two shapes it took.
///
/// A body that is not the right **shape** — a bitmap where a list belongs, a
/// half degree where the generator takes whole ones, a field this API does not
/// have — is refused by the deserialiser as `422`. A body that is the right
/// shape carrying a **value** this system will not accept is refused by a named
/// local check as `400`. Both mean the same thing to a caller: nothing was
/// transmitted and no valve state changed.
fn assert_refused(answer: &Answer, what: &str) {
    assert!(
        answer.status == StatusCode::BAD_REQUEST
            || answer.status == StatusCode::UNPROCESSABLE_ENTITY,
        "{what}: {} {}",
        answer.status,
        answer.body
    );
}

// ---------------------------------------------------------------- surface

/// The eight valve operations, exactly as `DESIGN.md` § Software
/// design → "Expose only constrained public operations" spells them.
const DESIGN_VALVE_OPERATIONS: [&str; 8] = [
    "start(zone, outlet_set, temperature_f, duration_seconds)",
    "set_temperature(zone, temperature_f)",
    "set_outlets(zone, outlet_set)",
    "pause(zone)",
    "resume(zone)",
    "stop(zone)",
    "stop_all()",
    "get_cached_state()",
];

/// The four steam operations, from the same section.
const DESIGN_STEAM_OPERATIONS: [&str; 4] = [
    "steam_start(temperature_f, duration_minutes)",
    "steam_set_temperature(temperature_f)",
    "steam_set_duration(minutes)",
    "steam_stop()",
];

/// Where each of the eight `API-01` operations is addressed, and with what.
///
/// Kept beside [`DESIGN_VALVE_OPERATIONS`] and asserted against it, so an
/// operation cannot be added to the surface without a request for it appearing
/// here.
const API_01_REQUESTS: [(&str, &str, &str, Option<&str>); 8] = [
    (
        "start(zone, outlet_set, temperature_f, duration_seconds)",
        "POST",
        "/v1/zones/zone1/start",
        Some(r#"{"outlet_set":[1],"temperature_f":100.0,"duration_seconds":600}"#),
    ),
    (
        "set_temperature(zone, temperature_f)",
        "POST",
        "/v1/zones/zone1/temperature",
        Some(r#"{"temperature_f":100.0}"#),
    ),
    (
        "set_outlets(zone, outlet_set)",
        "POST",
        "/v1/zones/zone1/outlets",
        Some(r#"{"outlet_set":[1]}"#),
    ),
    ("pause(zone)", "POST", "/v1/zones/zone1/pause", None),
    ("resume(zone)", "POST", "/v1/zones/zone1/resume", None),
    ("stop(zone)", "POST", "/v1/zones/zone1/stop", None),
    ("stop_all()", "POST", "/v1/stop-all", None),
    ("get_cached_state()", "GET", "/v1/state", None),
];

/// `API-01`: expose **only** the eight constrained valve operations.
///
/// The names are half of it; the other half is
/// [`seven_of_the_eight_api_01_operations_are_performed`], which is separate
/// only because `cargo xtask reqs` matches on `fn req_` and does not see an
/// `async fn`.
#[test]
fn req_controller_design_api_01() {
    let listed: Vec<&str> = surface()
        .iter()
        .filter(|op| op.requirement == Requirement::Api01)
        .map(|op| op.operation)
        .collect();
    assert_eq!(listed, DESIGN_VALVE_OPERATIONS);

    let addressed: Vec<&str> = API_01_REQUESTS.iter().map(|(name, ..)| *name).collect();
    assert_eq!(
        addressed, DESIGN_VALVE_OPERATIONS,
        "every API-01 operation needs a request here"
    );
}

/// Seven of `API-01`'s eight operations are performed. The eighth refuses.
///
/// A register entry that reads "covered" because eight strings matched says
/// nothing about whether any of them does anything, so each is asked for and
/// the answers are compared against the one exception.
///
/// `set_outlets(zone, outlet_set)` is that exception, and it is asserted as one
/// rather than left to prose. It refuses with `409` by construction — changing
/// which outlets are open opens water, the safety kernel does not mint a grant
/// for a running zone, and `kdtv_engine::OperatorCommand` has no variant to
/// spell it with. `API-01` is `hard = true`, so this is a hard requirement met
/// in seven of its eight parts, and a future change that either implements it
/// or breaks another operation the same way fails here.
#[tokio::test]
async fn seven_of_the_eight_api_01_operations_are_performed() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let mut refused = Vec::new();
    for (name, method, uri, body) in API_01_REQUESTS {
        let answer = rig
            .call_as(method, uri, body, Some(&session), Some(TOKEN))
            .await;
        match answer.status {
            StatusCode::ACCEPTED | StatusCode::OK => {}
            StatusCode::CONFLICT => refused.push(name),
            other => panic!("{name}: {other} {}", answer.body),
        }
    }
    assert_eq!(
        refused,
        vec!["set_outlets(zone, outlet_set)"],
        "exactly one API-01 operation is a name rather than a capability"
    );
}

/// `API-02`: expose steam on the same pattern, with only these four.
#[test]
fn req_controller_design_api_02() {
    let listed: Vec<&str> = surface()
        .iter()
        .filter(|op| op.requirement == Requirement::Api02)
        .map(|op| op.operation)
        .collect();
    assert_eq!(listed, DESIGN_STEAM_OPERATIONS);
}

/// **The surface test.** A route that is not on the two design lists fails
/// here.
///
/// The one permitted exception is the read-only event stream `SVC-05` requires,
/// and it is named explicitly rather than allowed by a rule: an entry that is
/// neither on a design list nor exactly this one is a route nobody reviewed.
#[test]
fn the_surface_is_exactly_the_design_lists_plus_the_svc_05_event_stream() {
    let listed = surface();
    let unaccounted: Vec<&Op> = listed
        .iter()
        .filter(|op| {
            !(DESIGN_VALVE_OPERATIONS.contains(&op.operation)
                || DESIGN_STEAM_OPERATIONS.contains(&op.operation))
        })
        .collect();
    assert_eq!(
        unaccounted.len(),
        1,
        "routes outside DESIGN.md: {unaccounted:#?}"
    );
    let stream = unaccounted[0];
    assert_eq!(stream.requirement, Requirement::Svc05);
    assert_eq!(stream.method, "GET");
    assert_eq!(stream.path, "/v1/events");
    assert!(!stream.opens_water);

    // And nothing is listed twice, by operation or by (method, path).
    let operations: std::collections::BTreeSet<&str> =
        listed.iter().map(|op| op.operation).collect();
    assert_eq!(operations.len(), listed.len());
    let addresses: std::collections::BTreeSet<(&str, &str)> =
        listed.iter().map(|op| (op.method, op.path)).collect();
    assert_eq!(addresses.len(), listed.len());
    assert_eq!(listed.len(), 13);
}

/// Only the operations that can open an outlet, raise a setpoint or extend a
/// running session are gated on a live session — and every stop is not.
#[test]
fn nothing_that_reduces_water_is_gated_on_a_session() {
    let gated: Vec<&str> = surface()
        .iter()
        .filter(|op| op.opens_water)
        .map(|op| op.operation)
        .collect();
    assert_eq!(
        gated,
        vec![
            "start(zone, outlet_set, temperature_f, duration_seconds)",
            "set_temperature(zone, temperature_f)",
            "set_outlets(zone, outlet_set)",
            "resume(zone)",
            "steam_start(temperature_f, duration_minutes)",
            "steam_set_temperature(temperature_f)",
            "steam_set_duration(minutes)",
        ]
    );
    for stop in ["stop(zone)", "stop_all()", "steam_stop()", "pause(zone)"] {
        let op = surface()
            .into_iter()
            .find(|op| op.operation == stop)
            .expect("the design lists it");
        assert!(!op.opens_water, "{stop} must never need a live session");
    }
}

#[tokio::test]
async fn a_path_that_is_not_on_the_surface_is_not_served() {
    let rig = Rig::new();
    for uri in [
        "/v1/zones/zone1/acknowledge",
        "/v1/zones/zone1/frame",
        "/v1/raw",
        "/v1/steam/powerclean",
        "/",
    ] {
        let answer = rig.call("POST", uri, Some("{}")).await;
        assert_eq!(answer.status, StatusCode::NOT_FOUND, "{uri}");
    }
    assert!(rig.control.calls().is_empty());
}

#[tokio::test]
async fn a_read_route_does_not_answer_a_write_and_a_write_route_does_not_answer_a_read() {
    let rig = Rig::new();
    let answer = rig.call("POST", "/v1/state", Some("{}")).await;
    assert_eq!(answer.status, StatusCode::METHOD_NOT_ALLOWED);
    let answer = rig.call("GET", "/v1/stop-all", None).await;
    assert_eq!(answer.status, StatusCode::METHOD_NOT_ALLOWED);
}

/// The router serves the methods the table declares, and no others.
///
/// The surface guarantee is that the router is *generated* from
/// [`surface`], so a route nobody reviewed cannot exist. That was not quite
/// true: axum dispatches `HEAD` to a `GET` handler unless `HEAD` is registered,
/// so `HEAD /v1/state` and `HEAD /v1/events` were served and appeared in no
/// list — and `HEAD /v1/events` ran the handler and took a `broadcast`
/// subscription. Neither could move water; both were routes the generation step
/// added after the table had been reviewed.
///
/// Every method is tried against every path, which is why `HEAD` is not
/// special-cased here either. The requests carry a live session, because
/// `require_fresh_session` wraps the whole method router on a water-opening
/// route and would otherwise answer `401` before the method was ever looked
/// at — which refuses the request, but not for the reason under test.
#[tokio::test]
async fn the_router_serves_no_method_the_table_did_not_declare() {
    let rig = Rig::new();
    let session = rig.establish().await;
    for op in surface() {
        let uri = op.path.replace("{zone}", "zone1");
        for method in ["HEAD", "GET", "POST", "PUT", "PATCH", "DELETE"] {
            if method == op.method {
                continue;
            }
            let answer = rig
                .call_as(method, &uri, None, Some(&session), Some(TOKEN))
                .await;
            assert_eq!(
                answer.status,
                StatusCode::METHOD_NOT_ALLOWED,
                "{method} {} is served and is on no list",
                op.path
            );
        }
    }
    assert!(rig.control.calls().is_empty());
    assert_eq!(
        rig.control.ledger.subscriptions.load(Ordering::Relaxed),
        0,
        "an undeclared method must not reach the event stream"
    );
}

// ------------------------------------------------------------ authentication

/// `SVC-05`: the API is authenticated, and the event stream accepts no writes.
///
/// The write half is structural — a subscriber holds a
/// `tokio::sync::broadcast::Receiver`, which has no method that sends — so what
/// is asserted here is the half that can be: the stream is `GET` only, and no
/// request without a credential reaches any handler.
#[tokio::test]
async fn req_controller_design_svc_05() {
    let rig = Rig::new();
    for op in surface() {
        let uri = op.path.replace("{zone}", "zone1");
        let answer = rig.call_as(op.method, &uri, Some("{}"), None, None).await;
        assert_eq!(
            answer.status,
            StatusCode::UNAUTHORIZED,
            "{} {} answered without a credential",
            op.method,
            op.path
        );
    }
    let stream = surface()
        .into_iter()
        .find(|op| op.requirement == Requirement::Svc05)
        .expect("the event stream is on the surface");
    let answer = rig.call("POST", stream.path, Some("{}")).await;
    assert_eq!(answer.status, StatusCode::METHOD_NOT_ALLOWED);

    // Nothing reached the service, and nothing subscribed.
    assert!(rig.control.calls().is_empty());
    assert_eq!(rig.control.ledger.snapshot_reads.load(Ordering::Relaxed), 0);
    assert_eq!(rig.control.ledger.subscriptions.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn an_unauthenticated_request_reaches_no_handler_even_on_a_path_that_exists() {
    let rig = Rig::new();
    let answer = rig
        .call_as(
            "POST",
            "/v1/zones/zone1/start",
            Some(ok_start_body()),
            None,
            None,
        )
        .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.json()["error"], "unauthenticated");
    assert!(
        rig.control.calls().is_empty(),
        "nothing may reach the service"
    );
    // Not even a command id was minted.
    assert_eq!(rig.ids.next.load(Ordering::Relaxed), 0);
}

/// An unauthenticated request to a path that does not exist is `401`, not
/// `404`: an unauthenticated caller learns nothing about the surface.
#[tokio::test]
async fn authentication_wraps_the_fallback_too() {
    let rig = Rig::new();
    let answer = rig
        .call_as("POST", "/v1/nope", Some("{}"), None, None)
        .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_wrong_credential_is_refused_however_it_is_wrong() {
    let rig = Rig::new();
    let wrong = [
        "0123456789abcdef0123456789abcdeg",  // last byte
        "1123456789abcdef0123456789abcdef",  // first byte
        "0123456789abcdef0123456789abcde",   // short
        "0123456789abcdef0123456789abcdefg", // long
        "",
    ];
    for candidate in wrong {
        let answer = rig
            .call_as("GET", "/v1/state", None, None, Some(candidate))
            .await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{candidate:?}");
    }
    assert_eq!(rig.control.ledger.snapshot_reads.load(Ordering::Relaxed), 0);
}

/// The credential is removed from the request once it has verified.
///
/// `hyper`'s `HeaderValue` is not zeroized when it drops, so a token left on the
/// request outlives every `Zeroizing` in this crate and is visible to anything
/// layered inside the authentication — which is what a `tower_http` tracing
/// layer with `include_headers(true)` prints, on every request. `LOG-09` holds
/// structurally once the header is gone, rather than because no header-logging
/// middleware happens to be installed today.
///
/// Driven through a router of its own, because no route on the real surface
/// reports its request headers — which is the point.
#[tokio::test]
async fn the_credential_does_not_travel_past_the_authentication() {
    async fn header_names(request: axum::extract::Request) -> String {
        request
            .headers()
            .keys()
            .map(|name| name.as_str().to_owned())
            .collect::<Vec<String>>()
            .join(",")
    }

    let authenticator = Arc::new(Authenticator::new(
        test_token(),
        Sessions::new(Duration::from_secs(900)),
    ));
    let app = Router::new()
        .route("/", axum::routing::get(header_names))
        .layer(axum::middleware::from_fn_with_state(
            authenticator,
            authenticate,
        ));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/")
                .header("authorization", format!("Bearer {TOKEN}"))
                .header(SESSION_HEADER, "1")
                .body(Body::empty())
                .expect("a well-formed request"),
        )
        .await
        .expect("the router is infallible");
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 16)
        .await
        .expect("a bounded body");
    let seen = String::from_utf8_lossy(&bytes).into_owned();
    assert!(
        seen.contains(SESSION_HEADER),
        "the handler should still see the rest of the request: {seen:?}"
    );
    assert!(
        !seen.to_ascii_lowercase().contains("authorization"),
        "the credential is still on the request when it reaches a handler: {seen:?}"
    );
}

/// `BOOT-07`'s live-session gate is not satisfied by guessing a small integer.
///
/// The gate is the second of the two steps a start needs. When session ids were
/// a counter that restarted at 1 on every boot, a stored request carrying
/// `x-kdtv-session: 1` satisfied it in one shot against a freshly restarted
/// daemon — no preceding interaction, water open at the requested setpoint.
#[tokio::test]
async fn a_guessed_session_id_does_not_open_water() {
    let rig = Rig::new();
    // Sessions exist: this is a running daemon, not an empty table.
    for _ in 0..8 {
        let _ = rig.establish().await;
    }
    for guess in ["1", "2", "3", "8", "9", "42", "1024"] {
        let answer = rig
            .call_as(
                "POST",
                "/v1/zones/zone1/start",
                Some(ok_start_body()),
                Some(guess),
                Some(TOKEN),
            )
            .await;
        assert_eq!(
            answer.status,
            StatusCode::UNAUTHORIZED,
            "session {guess} was honoured: {}",
            answer.body
        );
        assert_eq!(answer.json()["error"], "no_live_session", "session {guess}");
    }
    assert!(
        rig.control.calls().is_empty(),
        "a guessed session must reach nothing"
    );
}

/// The comparison is constant time, and this is the regression guard.
///
/// A timing assertion is not reliable in a test runner, so what is asserted is
/// the thing that can be: the one comparison site uses `subtle`'s
/// `ConstantTimeEq` and not `==`. An `==` on a secret returns at the first
/// differing byte and hands an attacker on loopback the token one byte at a
/// time.
#[test]
fn the_credential_comparison_is_constant_time() {
    let source = include_str!("auth.rs");
    assert!(
        source.contains("expected.ct_eq(presented).into()"),
        "the token comparison is no longer subtle::ConstantTimeEq"
    );
    assert!(
        !source.contains("expected == presented"),
        "the token is compared with =="
    );
}

/// There is no path from an unauthenticated request to an authorisation to open
/// water, and the way that is kept true is that there is exactly one place that
/// mints one.
///
/// **This counts this crate's sources only.** `StartAuthorization::issue` is
/// `pub` in `kdtv-safety` and re-exported through `kdtv_service::surface`, so
/// any crate that links `kdtv-service` can mint one — `kdtvd` does link it, and
/// carries its own test that it never does. A workspace-wide count belongs in
/// `cargo xtask`, beside `audit-graph`, with an allowlist of the one production
/// site and the test fixtures; it does not exist yet and this is not a
/// substitute for it.
#[test]
fn the_start_authorisation_is_minted_in_exactly_one_place() {
    let sources = [
        ("lib.rs", include_str!("lib.rs")),
        ("auth.rs", include_str!("auth.rs")),
        ("control.rs", include_str!("control.rs")),
        ("error.rs", include_str!("error.rs")),
        ("routes.rs", include_str!("routes.rs")),
        ("serve.rs", include_str!("serve.rs")),
    ];
    let mut sites = Vec::new();
    for (name, text) in sources {
        for line in text.lines() {
            if line.contains("StartAuthorization::issue(") && !line.trim_start().starts_with("///")
            {
                sites.push(format!("{name}: {}", line.trim()));
            }
        }
    }
    assert_eq!(sites.len(), 1, "{sites:#?}");
    assert!(sites[0].starts_with("auth.rs:"), "{sites:#?}");
}

// ------------------------------------------------------------------ sessions

/// `BOOT-07`: a start is accepted only after a fresh authenticated session and
/// an explicit user command — two steps, not one.
#[tokio::test(start_paused = true)]
async fn req_controller_design_boot_07() {
    let rig = Rig::with_ttl(Duration::from_secs(900));

    // Step one and two in the same request: refused.
    let answer = rig
        .call("POST", "/v1/zones/zone1/start", Some(ok_start_body()))
        .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.json()["error"], "no_live_session");
    assert!(rig.control.calls().is_empty());

    // A session, then the command.
    let session = answer.session().expect("a session was established");
    let answer = rig
        .call_as(
            "POST",
            "/v1/zones/zone1/start",
            Some(ok_start_body()),
            Some(&session),
            Some(TOKEN),
        )
        .await;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    assert_eq!(rig.control.calls().len(), 1);

    // Past the time-to-live the same session no longer opens water.
    tokio::time::advance(Duration::from_secs(901)).await;
    let answer = rig
        .call_as(
            "POST",
            "/v1/zones/zone1/start",
            Some(ok_start_body()),
            Some(&session),
            Some(TOKEN),
        )
        .await;
    assert_eq!(answer.status, StatusCode::UNAUTHORIZED);
    assert_eq!(answer.json()["error"], "no_live_session");
    assert_eq!(
        rig.control.calls().len(),
        1,
        "nothing more reached the service"
    );
}

/// An expired session never stands between an operator and a stop.
#[tokio::test(start_paused = true)]
async fn a_stop_is_never_refused_for_want_of_a_session() {
    let rig = Rig::with_ttl(Duration::from_secs(60));
    tokio::time::advance(Duration::from_secs(3_600)).await;
    for uri in [
        "/v1/zones/zone1/stop",
        "/v1/zones/zone2/stop",
        "/v1/zones/zone1/pause",
        "/v1/stop-all",
        "/v1/steam/stop",
    ] {
        let answer = rig.call("POST", uri, None).await;
        assert_eq!(
            answer.status,
            StatusCode::ACCEPTED,
            "{uri}: {}",
            answer.body
        );
    }
    assert_eq!(rig.control.calls().len(), 5);
}

#[tokio::test]
async fn the_session_a_command_was_issued_on_is_what_the_log_records() {
    let rig = Rig::new();
    let session = rig.establish().await;
    rig.call("POST", "/v1/zones/zone1/stop", None).await;
    let calls = rig.control.calls();
    let Some(Recorded::Zone { source, .. }) = calls.first() else {
        panic!("a zone command: {calls:#?}");
    };
    match source {
        RequestSource::LocalApi { session: id, peer } => {
            // The stop established its own session, so it is not the one above;
            // what matters is that a session id and a peer are both recorded.
            assert!(*id > 0);
            assert!(!peer.is_empty());
            assert_ne!(session, "0");
        }
        other => panic!("the API must record itself as the source: {other:?}"),
    }
}

// -------------------------------------------------------------- API-04, -06

/// `API-04`: `outlet_set` is configuration slot numbers, never a wire bitmap.
#[tokio::test]
async fn req_controller_design_api_04() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let post = async |body: &str| {
        rig.call_as(
            "POST",
            "/v1/zones/zone1/start",
            Some(body),
            Some(&session),
            Some(TOKEN),
        )
        .await
    };

    // A bitmap is a type error, not an alternative encoding: `outlet_set` is a
    // sequence of slot numbers and a single integer does not deserialise into
    // one.
    let answer = post(r#"{"outlet_set":3,"temperature_f":100.0,"duration_seconds":600}"#).await;
    assert_refused(&answer, "a bitmap");

    // Slot 0 and slot 7 are not slots on any valve here.
    for bad in ["[0]", "[7]", "[1,7]", "[]"] {
        let answer = post(&format!(
            r#"{{"outlet_set":{bad},"temperature_f":100.0,"duration_seconds":600}}"#
        ))
        .await;
        assert_refused(&answer, bad);
    }
    assert!(
        rig.control.calls().is_empty(),
        "nothing may have reached the service"
    );

    // Slot numbers arrive as slot numbers, deduplicated and order-independent.
    let answer =
        post(r#"{"outlet_set":[3,1,3],"temperature_f":100.0,"duration_seconds":600}"#).await;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    let calls = rig.control.calls();
    let Some(Recorded::Start { request, .. }) = calls.first() else {
        panic!("a start: {calls:#?}");
    };
    let slots: Vec<u8> = request.outlets.iter().map(kdtv_units::Slot::get).collect();
    assert_eq!(slots, vec![1, 3]);
}

/// `API-06`: hammering the status API reaches nothing that could put a frame on
/// a bus.
///
/// This is the requirement that stops the replacement regrowing the polling
/// behaviour that hung the original controller (`INVESTIGATIONS.md` I1).
///
/// ~~The reported frame count is asserted not to move.~~ Superseded: this
/// crate cannot populate a snapshot — see the module docs — so
/// `SystemSnapshot::frames_tx` is zero here by construction and `0 == 0` held
/// for any implementation, including one that transmitted on every read. The
/// frame count is asserted in `kdtv-service`, over a snapshot with links in it.
///
/// What is asserted here is the boundary, and each of these fails if
/// `cached_state` grows a path to the service: no command, no command id, no
/// subscription, and exactly one cache read per request — the last of which is
/// what catches a read that refreshes the cache by a round trip before serving
/// it.
#[tokio::test]
async fn req_controller_design_api_06() {
    const READS: u64 = 2_000;
    let rig = Rig::new();
    for _ in 0..READS {
        let answer = rig.call("GET", "/v1/state", None).await;
        assert_eq!(answer.status, StatusCode::OK);
    }
    assert!(
        rig.control.calls().is_empty(),
        "a status read must send no command"
    );
    assert_eq!(
        rig.ids.next.load(Ordering::Relaxed),
        0,
        "and mint no command id"
    );
    assert_eq!(
        rig.control.ledger.subscriptions.load(Ordering::Relaxed),
        0,
        "and subscribe to nothing"
    );
    assert_eq!(
        rig.control.ledger.snapshot_reads.load(Ordering::Relaxed),
        READS,
        "and be exactly one load out of the cache, per request, and nothing else"
    );
}

#[tokio::test]
async fn the_cached_state_is_what_a_reader_gets() {
    let rig = Rig::new();
    let answer = rig.call("GET", "/v1/state", None).await;
    assert_eq!(answer.status, StatusCode::OK);
    let body = answer.json();
    assert_eq!(body["service_boot"], 7);
    assert_eq!(body["pi_boot"], "boot-uuid");
    assert_eq!(body["shutting_down"], false);
}

// ------------------------------------------------------------------ commands

/// One command route, its body, and what the service must have been asked for.
type ZoneCase = (
    &'static str,
    Option<&'static str>,
    fn(&OperatorCommand) -> bool,
);
type SteamCase = (
    &'static str,
    Option<&'static str>,
    fn(&SteamCommand) -> bool,
);

/// `stop_all()` is one operator action and one command id, and the API sends it
/// as one command rather than fanning it out itself.
///
/// The fan-out is the supervisor's, and this asserts only that the API does not
/// do it: one `StopAll`, not two zone stops with steam left running.
///
/// ~~The fan-out to both zones and steam (`API-03`) is asserted against real
/// transmitted frames in `kdtv-service`'s
/// `stop_all_reports_ok_only_because_a_link_actually_took_it`.~~ Superseded —
/// that test silences zone 1, asserts zone 1 latches, and then asserts zone 2's
/// watcher saw an all-off. It never inspects the steam link, and no test in the
/// workspace asserts that `stop_all` reaches steam.
/// `kdtv_service::Supervisor::stop_all` does fan out to it; nothing holds that
/// there. **`API-03` is `hard = true` and its steam half is unguarded**, and
/// the test belongs in `kdtv-service`, where a steam link watcher exists.
#[tokio::test]
async fn stop_all_is_one_command_and_not_three_zone_stops() {
    let rig = Rig::new();
    let answer = rig.call("POST", "/v1/stop-all", None).await;
    assert_eq!(answer.status, StatusCode::ACCEPTED);
    let calls = rig.control.calls();
    assert_eq!(calls.len(), 1, "{calls:#?}");
    assert!(matches!(calls[0], Recorded::StopAll { .. }), "{calls:#?}");
    assert_eq!(answer.json()["command"], 1);
}

#[tokio::test]
async fn every_zone_command_arrives_as_the_operation_it_names() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let cases: [ZoneCase; 4] = [
        ("pause", None, |c| {
            matches!(c, OperatorCommand::Pause { .. })
        }),
        ("resume", None, |c| {
            matches!(c, OperatorCommand::Resume { .. })
        }),
        ("stop", None, |c| matches!(c, OperatorCommand::Stop { .. })),
        ("temperature", Some(r#"{"temperature_f":100.0}"#), |c| {
            matches!(c, OperatorCommand::SetTemperature { .. })
        }),
    ];
    for (leaf, body, is_expected) in cases {
        let answer = rig
            .call_as(
                "POST",
                &format!("/v1/zones/zone2/{leaf}"),
                body,
                Some(&session),
                Some(TOKEN),
            )
            .await;
        assert_eq!(
            answer.status,
            StatusCode::ACCEPTED,
            "{leaf}: {}",
            answer.body
        );
        let calls = rig.control.calls();
        let Some(Recorded::Zone { zone, command, .. }) = calls.last() else {
            panic!("{leaf}: {calls:#?}");
        };
        assert_eq!(*zone, ZoneId::Zone2, "{leaf}");
        assert!(is_expected(command), "{leaf}: {command:?}");
    }
}

#[tokio::test]
async fn every_steam_command_arrives_as_the_operation_it_names() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let cases: [SteamCase; 4] = [
        (
            "start",
            Some(r#"{"temperature_f":110,"duration_minutes":10}"#),
            |c| matches!(c, SteamCommand::Start { .. }),
        ),
        ("temperature", Some(r#"{"temperature_f":115}"#), |c| {
            matches!(c, SteamCommand::SetTemperature { .. })
        }),
        ("duration", Some(r#"{"minutes":5}"#), |c| {
            matches!(c, SteamCommand::SetDuration { .. })
        }),
        ("stop", None, |c| matches!(c, SteamCommand::Stop { .. })),
    ];
    for (leaf, body, is_expected) in cases {
        let answer = rig
            .call_as(
                "POST",
                &format!("/v1/steam/{leaf}"),
                body,
                Some(&session),
                Some(TOKEN),
            )
            .await;
        assert_eq!(
            answer.status,
            StatusCode::ACCEPTED,
            "{leaf}: {}",
            answer.body
        );
        let calls = rig.control.calls();
        let Some(Recorded::Steam { command, .. }) = calls.last() else {
            panic!("{leaf}: {calls:#?}");
        };
        assert!(is_expected(command), "{leaf}: {command:?}");
    }
}

/// The authorisation carries this boot and this command, which is what the
/// safety kernel checks before it mints a grant.
#[tokio::test]
async fn req_controller_design_id_01_a_start_authorisation_names_this_boot_and_this_command() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let answer = rig
        .call_as(
            "POST",
            "/v1/zones/zone1/start",
            Some(ok_start_body()),
            Some(&session),
            Some(TOKEN),
        )
        .await;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    let calls = rig.control.calls();
    let Some(Recorded::Start {
        request,
        authorized_boot,
        authorized_command,
        ..
    }) = calls.first()
    else {
        panic!("a start: {calls:#?}");
    };
    assert_eq!(*authorized_boot, BOOT);
    assert_eq!(*authorized_command, request.command);
}

/// `set_outlets` is on the design's list and the layers below have no way to
/// perform it. It refuses, transmits nothing, and mints no authorisation.
#[tokio::test]
async fn set_outlets_refuses_rather_than_reopening_water() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let answer = rig
        .call_as(
            "POST",
            "/v1/zones/zone1/outlets",
            Some(r#"{"outlet_set":[1,4]}"#),
            Some(&session),
            Some(TOKEN),
        )
        .await;
    assert_eq!(answer.status, StatusCode::CONFLICT, "{}", answer.body);
    assert!(
        rig.control.calls().is_empty(),
        "set_outlets must reach nothing"
    );
    assert_eq!(rig.ids.next.load(Ordering::Relaxed), 0);
    // A malformed one is still a malformed one.
    let answer = rig
        .call_as(
            "POST",
            "/v1/zones/zone1/outlets",
            Some(r#"{"outlet_set":[9]}"#),
            Some(&session),
            Some(TOKEN),
        )
        .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_service_refusal_becomes_a_status_a_client_can_act_on() {
    let rig = Rig::new();
    rig.control.refuse_with(CommandError::TooSoon {
        link: kdtv_units::LinkKind::Zone(ZoneId::Zone1),
    });
    let answer = rig.call("POST", "/v1/zones/zone1/stop", None).await;
    assert_eq!(answer.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(answer.json()["error"], "too_soon");

    rig.control
        .refuse_with(CommandError::NoSuchLink(kdtv_units::LinkKind::Steam));
    let answer = rig.call("POST", "/v1/steam/stop", None).await;
    assert_eq!(answer.status, StatusCode::NOT_FOUND);
}

/// A command id that could not be made durable stops the request before
/// anything is attempted: an id a restart could reissue makes a frame log
/// unreadable exactly when it is needed.
#[tokio::test]
async fn nothing_is_attempted_without_a_durable_command_id() {
    let rig = Rig::new();
    rig.ids.broken.store(1, Ordering::Relaxed);
    let answer = rig.call("POST", "/v1/stop-all", None).await;
    assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(answer.json()["error"], "no_command_id");
    assert!(rig.control.calls().is_empty());
}

// -------------------------------------------------------------- temperature

/// The Fahrenheit boundaries, through the whole stack rather than at the unit.
#[tokio::test]
async fn the_fahrenheit_boundaries_hold_through_the_api() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let post = async |f: &str| {
        rig.call_as(
            "POST",
            "/v1/zones/zone1/temperature",
            Some(&format!(r#"{{"temperature_f":{f}}}"#)),
            Some(&session),
            Some(TOKEN),
        )
        .await
    };

    // Accepted, and rounded down to the representable step at or below.
    for (asked, expect_raw) in [
        ("86.0", ValveSetpoint::FLOOR.raw()),
        ("100.0", 75),
        ("100.4", 76),
        ("108.5", ValveSetpoint::CEILING.raw()),
    ] {
        let answer = post(asked).await;
        assert_eq!(
            answer.status,
            StatusCode::ACCEPTED,
            "{asked}: {}",
            answer.body
        );
        let calls = rig.control.calls();
        let Some(Recorded::Zone {
            command: OperatorCommand::SetTemperature { temp, .. },
            ..
        }) = calls.last()
        else {
            panic!("{asked}: {calls:#?}");
        };
        assert_eq!(temp.wire(), Cx2::from_raw(expect_raw), "{asked}");
    }

    // Refused, and never quietly pulled to a bound.
    let before = rig.control.calls().len();
    for asked in ["85.9", "108.6", "109.0", "113.0", "120.0"] {
        let answer = post(asked).await;
        assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{asked}");
        assert_eq!(answer.json()["check"], "valve setpoint clamp", "{asked}");
    }
    assert_eq!(
        rig.control.calls().len(),
        before,
        "nothing may have reached the service"
    );
}

/// A request above a **configured** ceiling is refused, not rounded into it.
///
/// `Cx2` resolves 0.5 °C — about 0.9 °F — so `from_fahrenheit` returns the step
/// at or below the request. The configured ceiling used to be applied to that
/// rounded value, so every request in the 0.9 °F band above a narrowed ceiling
/// compared equal to it and was accepted: `202`, a command id, and a
/// temperature the caller did not ask for, with no clamp record and no
/// rejection naming the check. The delivered water was never hotter than
/// configured — the error is always toward cooler — but an installation that
/// narrowed `setpoint_ceiling` precisely so the API would refuse hot requests
/// did not get the refusal.
///
/// The compiled ceiling never had this: `from_fahrenheit` checks 108.5 °F
/// against the request itself, which
/// `a_request_above_the_ceiling_is_refused_rather_than_rounded_into_it`
/// covers in `kdtv-units`.
#[tokio::test]
async fn req_valve_control_temp_03_a_request_above_a_narrowed_ceiling_is_refused_rather_than_rounded_into_it()
 {
    // Cx2 80 is 40.0 °C, which is 104.0 °F exactly. The next step up, Cx2 81,
    // is 40.5 °C = 104.9 °F, so everything in (104.0, 104.9] rounds down onto
    // the ceiling.
    let bounds = Bounds::resolve(&BoundsRequest {
        setpoint_ceiling: Some(Cx2::from_raw(80)),
        ..BoundsRequest::default()
    });
    let rig = Rig::with_bounds(bounds);
    let session = rig.establish().await;
    let post = async |f: &str| {
        rig.call_as(
            "POST",
            "/v1/zones/zone1/temperature",
            Some(&format!(r#"{{"temperature_f":{f}}}"#)),
            Some(&session),
            Some(TOKEN),
        )
        .await
    };

    for asked in ["104.1", "104.4", "104.8", "104.9", "105.0", "108.5"] {
        let answer = post(asked).await;
        assert_eq!(
            answer.status,
            StatusCode::BAD_REQUEST,
            "{asked} °F was accepted above the configured ceiling: {}",
            answer.body
        );
        assert_eq!(
            answer.json()["check"],
            "configured setpoint bound",
            "{asked} °F"
        );
    }
    assert!(
        rig.control.calls().is_empty(),
        "nothing may have reached the service"
    );

    // The ceiling itself, and the step below it, are still accepted.
    for (asked, expect_raw) in [("104.0", 80), ("103.5", 79)] {
        let answer = post(asked).await;
        assert_eq!(
            answer.status,
            StatusCode::ACCEPTED,
            "{asked}: {}",
            answer.body
        );
        let calls = rig.control.calls();
        let Some(Recorded::Zone {
            command: OperatorCommand::SetTemperature { temp, .. },
            ..
        }) = calls.last()
        else {
            panic!("{asked}: {calls:#?}");
        };
        assert_eq!(temp.wire(), Cx2::from_raw(expect_raw), "{asked}");
    }
}

/// The configured steam session cap is enforced, not just parsed.
///
/// `bounds.steam_max_minutes` is validated by `kdtv-config`, refused when it
/// would widen, and narrowed into `Bounds` — and had no caller anywhere in the
/// workspace. An installation that set it to 5 because the generator sits in a
/// small enclosure got 20-minute sessions, and nothing logged a clamp because
/// nothing clamped.
#[tokio::test]
async fn a_steam_session_longer_than_the_configured_cap_is_refused() {
    let bounds = Bounds::resolve(&BoundsRequest {
        steam_max_minutes: Some(5),
        ..BoundsRequest::default()
    });
    let rig = Rig::with_bounds(bounds);
    let session = rig.establish().await;
    let post = async |uri: &str, body: &str| {
        rig.call_as("POST", uri, Some(body), Some(&session), Some(TOKEN))
            .await
    };

    for (uri, body) in [
        (
            "/v1/steam/start",
            r#"{"temperature_f":115,"duration_minutes":20}"#,
        ),
        (
            "/v1/steam/start",
            r#"{"temperature_f":115,"duration_minutes":6}"#,
        ),
        ("/v1/steam/duration", r#"{"minutes":6}"#),
        ("/v1/steam/duration", r#"{"minutes":20}"#),
    ] {
        let answer = post(uri, body).await;
        assert_eq!(
            answer.status,
            StatusCode::BAD_REQUEST,
            "{uri} {body}: {}",
            answer.body
        );
        assert_eq!(answer.json()["check"], "steam session length", "{body}");
    }
    assert!(
        rig.control.calls().is_empty(),
        "nothing may have reached the service"
    );

    // The cap itself is still a session.
    let answer = post("/v1/steam/duration", r#"{"minutes":5}"#).await;
    assert_eq!(answer.status, StatusCode::ACCEPTED, "{}", answer.body);
    assert_eq!(rig.control.calls().len(), 1);
}

/// The steam setpoint is a whole number of degrees Fahrenheit, and a half
/// degree is refused by the parser rather than rounded somewhere later.
#[tokio::test]
async fn the_steam_setpoint_is_whole_degrees_and_inside_the_generator_envelope() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let post = async |body: &str| {
        rig.call_as(
            "POST",
            "/v1/steam/temperature",
            Some(body),
            Some(&session),
            Some(TOKEN),
        )
        .await
    };
    assert_eq!(
        post(r#"{"temperature_f":110}"#).await.status,
        StatusCode::ACCEPTED
    );
    for bad in [
        // A half degree is not representable: `Fx2` steps in whole degrees and
        // the field is typed as an integer, so the parser refuses it.
        r#"{"temperature_f":110.5}"#,
        // Outside the generator's documented 90..=125 envelope.
        r#"{"temperature_f":89}"#,
        r#"{"temperature_f":126}"#,
        r#"{"temperature_f":100000}"#,
        r#"{"temperature_f":-5}"#,
    ] {
        assert_refused(&post(bad).await, bad);
    }
}

/// Zero minutes is unrepresentable.
///
/// The reverse-engineered notes describe `steamTimerSetTime = 0` as disabling
/// the generator's automatic shutoff — tier `[C]`, no capture taken — and
/// whether that timer is the same one as Kohler's documented 20-minute
/// auto-shutoff `[K]` is `STEAM-ADAPTER.md`'s open question 2, `[?]`. `[I]`
/// Refusing zero is the conservative reading of the unresolved question.
#[tokio::test]
async fn a_steam_session_of_zero_minutes_is_refused() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let answer = rig
        .call_as(
            "POST",
            "/v1/steam/duration",
            Some(r#"{"minutes":0}"#),
            Some(&session),
            Some(TOKEN),
        )
        .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert!(rig.control.calls().is_empty());
}

#[tokio::test]
async fn a_session_longer_than_the_limit_is_refused_rather_than_capped() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let answer = rig
        .call_as(
            "POST",
            "/v1/zones/zone1/start",
            Some(r#"{"outlet_set":[1],"temperature_f":100.0,"duration_seconds":1201}"#),
            Some(&session),
            Some(TOKEN),
        )
        .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST, "{}", answer.body);
    assert_eq!(answer.json()["check"], "session length");
    assert!(rig.control.calls().is_empty());
}

#[tokio::test]
async fn an_unknown_field_is_a_refusal_not_a_shrug() {
    let rig = Rig::new();
    let session = rig.establish().await;
    let answer = rig
        .call_as(
            "POST",
            "/v1/zones/zone1/start",
            Some(
                r#"{"outlet_set":[1],"temperature_f":100.0,"duration_seconds":600,"temperature_c":45}"#,
            ),
            Some(&session),
            Some(TOKEN),
        )
        .await;
    assert_eq!(
        answer.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        answer.body
    );
    assert!(rig.control.calls().is_empty());
}

// -------------------------------------------------------------------- LOG-09

/// `LOG-09`: no credential, access token or pairing data in the logs.
///
/// Asserted over what this crate actually emits during a scripted run: every
/// `tracing` line, every response body and every response header, across a good
/// credential, a wrong credential, a start, a refusal and a status read. The
/// token bytes appear in none of it.
#[tokio::test]
async fn req_controller_design_log_09() {
    let sink = LogSink::default();
    let collected = {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(sink.clone())
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let rig = Rig::new();
        let mut seen = String::new();
        let mut record = |answer: &Answer| {
            seen.push_str(&answer.body);
            for (name, value) in &answer.headers {
                seen.push_str(name.as_str());
                seen.push_str(&String::from_utf8_lossy(value.as_bytes()));
            }
        };

        record(&rig.call("GET", "/v1/state", None).await);
        let session = rig.establish().await;
        record(
            &rig.call_as(
                "POST",
                "/v1/zones/zone1/start",
                Some(ok_start_body()),
                Some(&session),
                Some(TOKEN),
            )
            .await,
        );
        record(
            &rig.call_as(
                "POST",
                "/v1/steam/start",
                Some(r#"{"temperature_f":110,"duration_minutes":10}"#),
                Some(&session),
                Some(TOKEN),
            )
            .await,
        );
        record(
            &rig.call_as(
                "GET",
                "/v1/state",
                None,
                None,
                Some("the-wrong-token-entirely"),
            )
            .await,
        );
        record(&rig.call("POST", "/v1/stop-all", None).await);
        seen
    };

    let logged = sink.contents();
    assert!(!logged.is_empty(), "the run must have logged something");
    for haystack in [&logged, &collected] {
        assert!(
            !haystack.contains(TOKEN),
            "the credential reached an output: {haystack}"
        );
        for word in ["Bearer ", "authorization", "the-wrong-token-entirely"] {
            assert!(
                !haystack
                    .to_ascii_lowercase()
                    .contains(&word.to_ascii_lowercase()),
                "{word:?} reached an output: {haystack}"
            );
        }
    }
    // And the start really was logged, so this is not passing on an empty run.
    assert!(logged.contains("start requested"), "{logged}");
}

// ------------------------------------------------------------------ shutdown

/// The shipped configuration, through the real loader, with the devices it
/// names faked and the API on an ephemeral port.
///
/// Read from the committed file rather than written here, so this cannot drift
/// from the schema the daemon validates. Returns `None` when the file is not
/// where this crate expects it, which is the same skip `kdtvd`'s tests take.
fn loopback_config() -> Option<kdtv_config::ValidatedConfig> {
    use kdtv_config::{FsEntry, MapFs, ValidatedConfig};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)?
        .join("deploy/kdtvd.toml");
    let text = std::fs::read_to_string(&path)
        .ok()?
        .replace("127.0.0.1:8443", "127.0.0.1:0");
    let token = "/run/credentials/kdtvd.service/api-token";
    let fs = MapFs::new()
        .with(
            "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if00-port0",
            FsEntry::link("/dev/ttyUSB0"),
        )
        .with(
            "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if01-port0",
            FsEntry::link("/dev/ttyUSB1"),
        )
        .with(
            "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if02-port0",
            FsEntry::link("/dev/ttyUSB2"),
        )
        .with(token, FsEntry::own(token).with_mode(0o400));
    ValidatedConfig::from_str_with(&text, &path, &fs).ok()
}

/// The API stops serving even while an `SVC-05` event stream is held open.
///
/// This is the daemon's only clean exit path, and it deadlocked. axum's
/// graceful shutdown waits for every open connection with no timeout; the event
/// stream's `BroadcastStream` ends only when the `broadcast::Sender` closes;
/// that sender lives in the `ServiceHandle` held by the router inside the task
/// being waited on. One Homebridge client subscribed to `/v1/events` and
/// `serve()` never returned — `systemd` then `SIGKILL`ed at
/// `TimeoutStopSec=20s`, so `exit 5` for an unconfirmed off, the one outcome
/// whose remedy is a person removing valve power, was reported as
/// `status=9/KILL` instead.
///
/// The water is already off by the time this matters. What this protects is the
/// exit code.
#[tokio::test]
async fn the_api_stops_serving_even_with_the_event_stream_held_open() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    let Some(config) = loopback_config() else {
        return;
    };
    let control = FakeControl::new();
    let state = ApiState::new(
        control.clone(),
        Arc::new(FakeIds::default()),
        Bounds::COMPILED,
        test_token(),
        Duration::from_secs(900),
    );
    let api = Api::bind(config.api(), state)
        .await
        .expect("an ephemeral loopback port");
    let address = api.local_addr().expect("the bound address");

    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let serving = tokio::spawn(api.serve(async move {
        let _ = stopped.await;
    }));

    // What `SVC-05` is for: a client that holds the stream open indefinitely.
    let mut client = tokio::net::TcpStream::connect(address)
        .await
        .expect("a client");
    client
        .write_all(
            format!(
                "GET /v1/events HTTP/1.1\r\nHost: {address}\r\nauthorization: Bearer {TOKEN}\r\n\r\n"
            )
            .as_bytes(),
        )
        .await
        .expect("the request was sent");
    let mut head = [0_u8; 15];
    tokio::time::timeout(Duration::from_secs(5), client.read_exact(&mut head))
        .await
        .expect("the event stream answered")
        .expect("a status line");
    assert_eq!(
        &head,
        b"HTTP/1.1 200 OK",
        "{:?}",
        String::from_utf8_lossy(&head)
    );
    assert_eq!(
        control.ledger.subscriptions.load(Ordering::Relaxed),
        1,
        "the stream must really be subscribed, or this proves nothing"
    );

    let _ = stop.send(());
    let ended = tokio::time::timeout(SHUTDOWN_GRACE + Duration::from_secs(10), serving)
        .await
        .expect("serve() did not return while an event stream was open");
    ended
        .expect("the API task panicked")
        .expect("the API server reported an error");

    // Held to here on purpose: dropping it earlier would end the connection and
    // this would pass without ever exercising the case.
    drop(client);
}

/// A `tracing` writer that keeps everything, so a test can read back exactly
/// what would have gone to the journal.
#[derive(Clone, Debug, Default)]
struct LogSink(Arc<Mutex<Vec<u8>>>);

impl LogSink {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

impl std::io::Write for LogSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogSink {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}
