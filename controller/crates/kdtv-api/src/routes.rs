//! The surface, and the only place it is spelled.
//!
//! # The table is the router
//!
//! `table` is the single list of operations, and [`router`] is a fold over it.
//! A route that is not in the table therefore cannot exist, which is a stronger
//! statement than a test comparing two lists that could both be edited. The test
//! that matters, in the crate's test module, compares the table against
//! `DESIGN.md` § Software design verbatim.
//!
//! Twelve operations come from the design: eight valve (`API-01`) and four
//! steam (`API-02`). The thirteenth entry is the event stream, which `SVC-05`
//! requires — "a local authenticated API **and a read-only event stream**" —
//! and which `get_cached_state` is not. It is `GET`, it carries no operation and
//! it cannot move water. It is called out here because it is the one route not
//! on either design list, and an unexplained extra route is exactly what the
//! surface test exists to catch.
//!
//! # `API-01` is met in seven of its eight operations
//!
//! `set_outlets(zone, outlet_set)` is exposed, validates its body, and then
//! refuses with `409` every time — see the `set_outlets` handler. The route
//! exists so the refusal is visible rather than reading as a deployment fault,
//! but the operation is a name and not a capability, and `API-01` is
//! `hard = true`. Recording it here, and in the test that asserts it
//! (`req_design_api_01`), is what stops the register reading
//! "covered" on the strength of eight matching strings.
//!
//! **This is also a behaviour change from the DTV+ this replacement stands in
//! for**, and `requirements.toml` and `DESIGN.md` do not record it.
//! They should.
//!
//! # What is deliberately absent
//!
//! - **`acknowledge`.** `kdtv_service::ServiceHandle::acknowledge` exists and
//!   clears a latched link, and there is no route to it, because the design's
//!   list of public operations has none. A latched link is a physical fault —
//!   a welded valve, a bus with two valves on it, a zone that would not confirm
//!   itself off — and acknowledging one over HTTP from a phone is not a
//!   reviewed operation. Recorded, not forgotten.
//! - **Anything raw.** There is no route that takes bytes, an address, a
//!   register or a frame. This crate cannot name those types.
//! - **A bitmap.** `outlet_set` is a list of configuration slot numbers
//!   (`API-04`), 1..=6, validated by `kdtv_units::Slot`. A single integer is a
//!   type error, not an alternative encoding.

use std::convert::Infallible;

use axum::extract::{Extension, Json, Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse as _, Response};
use axum::routing::{MethodRouter, get, post};
use axum::{Router, middleware};
use futures_util::StreamExt as _;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tower_http::limit::RequestBodyLimitLayer;

use kdtv_config::Bounds;
use kdtv_service::ServiceHandle;
use kdtv_service::surface::{OperatorCommand, SteamCommand, ValidatedStart};
use kdtv_units::{
    CommandId, Fx2, SessionDuration, Slot, SlotSet, SteamMinutes, SteamSetpoint, ValveSetpoint,
    ZoneId,
};

use crate::auth::{Caller, FreshCaller, authenticate, require_fresh_session};
use crate::control::Control;
use crate::error::ApiError;
use crate::serve::ApiState;

/// The largest request body this API will read.
///
/// Every body on the surface is a handful of numbers. A local API that parses
/// JSON should not accept an unbounded one.
pub const BODY_LIMIT: usize = 8 * 1024;

/// Which requirement puts an operation on the surface.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Requirement {
    /// `API-01` — the eight constrained valve operations.
    Api01,
    /// `API-02` — the four steam operations.
    Api02,
    /// `SVC-05` — the read-only event stream.
    Svc05,
}

impl Requirement {
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Api01 => "API-01",
            Self::Api02 => "API-02",
            Self::Svc05 => "SVC-05",
        }
    }
}

/// One route, and the design operation it implements.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Op {
    pub method: &'static str,
    pub path: &'static str,
    /// The operation exactly as `DESIGN.md` spells it.
    pub operation: &'static str,
    pub requirement: Requirement,
    /// True when this operation can open an outlet, raise a setpoint or extend
    /// a running session.
    ///
    /// These are the routes that need a session that was already live
    /// (`BOOT-07`). Everything that only reduces water — `pause`, `stop`,
    /// `stop_all`, `steam_stop` — is deliberately false: an expired session
    /// must never stand between an operator and turning the water off.
    pub opens_water: bool,
}

impl Op {
    const fn read(
        method: &'static str,
        path: &'static str,
        operation: &'static str,
        requirement: Requirement,
    ) -> Self {
        Self {
            method,
            path,
            operation,
            requirement,
            opens_water: false,
        }
    }

    const fn water(method: &'static str, path: &'static str, operation: &'static str) -> Self {
        Self {
            method,
            path,
            operation,
            requirement: Requirement::Api01,
            opens_water: true,
        }
    }

    const fn steam_water(
        method: &'static str,
        path: &'static str,
        operation: &'static str,
    ) -> Self {
        Self {
            method,
            path,
            operation,
            requirement: Requirement::Api02,
            opens_water: true,
        }
    }
}

/// The surface, as data. Every route in the router comes from here.
#[must_use]
pub fn surface() -> Vec<Op> {
    // Built through the real route list so the two cannot drift: the
    // `MethodRouter`s are constructed and dropped. `ServiceHandle` is named only
    // to pin the type parameter; nothing is called on it.
    table::<ServiceHandle>()
        .into_iter()
        .map(|(op, _)| op)
        .collect()
}

/// The route table. `router` is a fold over this and nothing else.
fn table<C: Control>() -> Vec<(Op, MethodRouter<ApiState<C>>)> {
    vec![
        // ---- API-01, the eight constrained valve operations ----
        (
            Op::water(
                "POST",
                "/v1/zones/{zone}/start",
                "start(zone, outlet_set, temperature_f, duration_seconds)",
            ),
            post(start::<C>),
        ),
        (
            Op::water(
                "POST",
                "/v1/zones/{zone}/temperature",
                "set_temperature(zone, temperature_f)",
            ),
            post(set_temperature::<C>),
        ),
        (
            Op::water(
                "POST",
                "/v1/zones/{zone}/outlets",
                "set_outlets(zone, outlet_set)",
            ),
            post(set_outlets::<C>),
        ),
        (
            Op::read(
                "POST",
                "/v1/zones/{zone}/pause",
                "pause(zone)",
                Requirement::Api01,
            ),
            post(pause::<C>),
        ),
        (
            Op::water("POST", "/v1/zones/{zone}/resume", "resume(zone)"),
            post(resume::<C>),
        ),
        (
            Op::read(
                "POST",
                "/v1/zones/{zone}/stop",
                "stop(zone)",
                Requirement::Api01,
            ),
            post(stop::<C>),
        ),
        (
            Op::read("POST", "/v1/stop-all", "stop_all()", Requirement::Api01),
            post(stop_all::<C>),
        ),
        (
            Op::read("GET", "/v1/state", "get_cached_state()", Requirement::Api01),
            get(cached_state::<C>),
        ),
        // ---- API-02, steam on the same pattern ----
        (
            Op::steam_water(
                "POST",
                "/v1/steam/start",
                "steam_start(temperature_f, duration_minutes)",
            ),
            post(steam_start::<C>),
        ),
        (
            Op::steam_water(
                "POST",
                "/v1/steam/temperature",
                "steam_set_temperature(temperature_f)",
            ),
            post(steam_set_temperature::<C>),
        ),
        (
            Op::steam_water("POST", "/v1/steam/duration", "steam_set_duration(minutes)"),
            post(steam_set_duration::<C>),
        ),
        (
            Op::read("POST", "/v1/steam/stop", "steam_stop()", Requirement::Api02),
            post(steam_stop::<C>),
        ),
        // ---- SVC-05, the read-only event stream ----
        (
            Op::read(
                "GET",
                "/v1/events",
                "the read-only event stream",
                Requirement::Svc05,
            ),
            get(events::<C>),
        ),
    ]
}

/// Build the API.
///
/// Layer order, outermost first: the body limit, then authentication, then —
/// on water-opening routes only — the live-session requirement. Authentication
/// wraps the fallback too, so an unauthenticated request to a path that does
/// not exist is answered `401` and learns nothing about the surface.
///
/// # There is no concurrency limit, rate limit or request timeout, deliberately
///
/// `tower` is already a dependency and neither `ConcurrencyLimitLayer` nor a
/// `TimeoutLayer` is applied, because both would land on `/v1/events`: a
/// timeout ends the `SVC-05` stream `SVC-05` exists to keep open, and a
/// concurrency limit is permanently consumed by every client holding one. What
/// bounds a client's cost here instead is that every step of an authenticated
/// request is cheap — a fixed-length constant-time compare, one hash lookup
/// (`auth::Sessions`), one atomic load and one serialisation — and that the
/// bind is loopback, so the set of clients is the set of processes on this Pi.
/// If that ever stops being enough, the answer is a limiter that exempts the
/// event stream, not a blanket one.
///
/// # `HEAD` is declined rather than inherited
///
/// ~~A `GET` route was registered with `axum::routing::get` and left there.~~
/// Superseded: axum dispatches `HEAD` to a `GET` handler when no `HEAD` is
/// registered, so `HEAD /v1/state` and `HEAD /v1/events` were served routes
/// that appeared nowhere in [`surface`] — and `HEAD /v1/events` ran the event
/// handler and took a subscription. Neither could move water, but "there is no
/// route outside `surface`, because the router is generated from it" is the
/// whole mechanism, and the generation step was adding a method. The `GET`
/// routes now register `HEAD` explicitly, to a refusal.
pub fn router<C: Control>(state: ApiState<C>) -> Router {
    let authenticator = state.authenticator();
    let mut app = Router::new();
    for (op, method_router) in table::<C>() {
        let method_router = if op.method == "GET" {
            method_router.head(method_not_on_the_surface)
        } else {
            method_router
        };
        let method_router = if op.opens_water {
            method_router.layer(middleware::from_fn(require_fresh_session))
        } else {
            method_router
        };
        app = app.route(op.path, method_router);
    }
    app.fallback(no_such_operation)
        .layer(middleware::from_fn_with_state(authenticator, authenticate))
        .layer(RequestBodyLimitLayer::new(BODY_LIMIT))
        .with_state(state)
}

async fn no_such_operation() -> ApiError {
    ApiError::NoSuchOperation
}

/// A method the table did not declare on a path it did.
///
/// The same answer axum gives for every other undeclared method, `Allow`
/// header included; it is written out only because `HEAD` is the one method
/// axum would otherwise answer from the `GET` handler.
async fn method_not_on_the_surface() -> Response {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [(axum::http::header::ALLOW, "GET")],
    )
        .into_response()
}

// ------------------------------------------------------------------ bodies

/// `deny_unknown_fields` throughout: a client sending a field this API does not
/// have is a client that believes it is setting something, and answering `400`
/// is better than silently ignoring it.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct StartBody {
    /// Configuration slot numbers, 1..=6 for zone 1 and 1..=3 for zone 2.
    /// `API-04`. Not a bitmap.
    outlet_set: Vec<u8>,
    temperature_f: f32,
    duration_seconds: u64,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct TemperatureBody {
    temperature_f: f32,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct OutletsBody {
    outlet_set: Vec<u8>,
}

/// The steam setpoint is an **integer** number of degrees Fahrenheit.
///
/// The generator moves in 1 °F steps (`HARDWARE.md` § 12) and `Fx2` cannot
/// represent anything else that the encoder will emit. Typing the field as an
/// integer refuses a half degree at the parser rather than rounding it
/// somewhere later — denial by absence rather than by check.
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct SteamStartBody {
    temperature_f: i32,
    duration_minutes: u8,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct SteamTemperatureBody {
    temperature_f: i32,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct SteamDurationBody {
    minutes: u8,
}

/// What an accepted command answers with.
///
/// The command id is the handle every log line about this command repeats
/// (`LOG-01`), so a client can correlate what it asked for with what the
/// service did.
#[derive(Serialize, Debug)]
struct Accepted {
    command: u64,
}

fn accepted(command: CommandId) -> Response {
    (StatusCode::ACCEPTED, Json(Accepted { command: command.0 })).into_response()
}

// ------------------------------------------------------------------ checks

/// Configuration slot numbers to a slot set. `API-04`.
///
/// Whether a slot is *configured on this valve* is the kernel's question and is
/// not second-guessed here; this rejects only what is not a slot at all.
fn slots(requested: &[u8]) -> Result<SlotSet, ApiError> {
    if requested.is_empty() {
        return Err(ApiError::rejected(
            "outlet set",
            "no outlets were requested",
        ));
    }
    let mut set = SlotSet::EMPTY;
    for n in requested {
        let slot = Slot::new(*n).map_err(|e| ApiError::rejected("outlet slot", e))?;
        set = set.insert(slot);
    }
    Ok(set)
}

/// A Fahrenheit request to a valve setpoint.
///
/// Two clamps in order, and both reject rather than saturate: the compiled one
/// in `kdtv_units::ValveSetpoint::from_fahrenheit`, then any narrowing the
/// configuration applied, which ends at `ValveSetpoint::try_new` again. A
/// caller asking for 120 °F is told no rather than quietly given 108.5 °F.
///
/// **Both ceilings are checked in Fahrenheit, against the request itself.**
/// `Cx2` resolves 0.5 °C — about 0.9 °F — so `from_fahrenheit` returns the
/// representable step *at or below* what was asked for. ~~The configured
/// ceiling was applied to that rounded value.~~ Superseded: a request in the
/// 0.9 °F band above a narrowed ceiling rounded down onto it and compared
/// equal, so it was accepted at the ceiling — the silent acceptance the
/// paragraph above says this function exists to refuse. The delivered water was
/// never hotter than configured, so this was never a scald path; what it cost
/// was an installation that narrowed `setpoint_ceiling` precisely to make the
/// API refuse hot requests, and got a `202` and a command id instead.
///
/// The floor needs no equivalent check: rounding down carries a request just
/// above a narrowed floor *below* it, so [`Bounds::valve_setpoint`] refuses it
/// on the `Cx2` comparison already.
fn setpoint(bounds: Bounds, requested: f32) -> Result<ValveSetpoint, ApiError> {
    let compiled = ValveSetpoint::from_fahrenheit(requested)
        .map_err(|e| ApiError::rejected("valve setpoint clamp", e))?;
    let ceiling = bounds.setpoint_ceiling();
    if requested > ceiling.fahrenheit() {
        return Err(ApiError::rejected(
            "configured setpoint bound",
            format!(
                "{requested:.1} °F is above the {:.1} °F ceiling configured for this system",
                ceiling.fahrenheit()
            ),
        ));
    }
    bounds
        .valve_setpoint(compiled.wire())
        .map_err(|e| ApiError::rejected("configured setpoint bound", e))
}

/// A whole number of degrees Fahrenheit to a steam setpoint.
///
/// `Fx2` is Fahrenheit times two, so this is a doubling and a range check. It
/// is **not** `Cx2::to_fx2`, which `clippy.toml` forbids outside the steam
/// encoder and which converts a valve temperature, not an operator's request.
fn steam_setpoint(bounds: Bounds, requested: i32) -> Result<SteamSetpoint, ApiError> {
    let raw = requested
        .checked_mul(2)
        .and_then(|doubled| u8::try_from(doubled).ok())
        .ok_or_else(|| {
            ApiError::rejected(
                "steam setpoint clamp",
                format!("{requested} °F is not a steam setpoint"),
            )
        })?;
    bounds
        .steam_setpoint(Fx2::from_raw(raw))
        .map_err(|e| ApiError::rejected("steam setpoint clamp", e))
}

/// A requested steam session length.
///
/// The compiled 1..=20 first, then the configured cap. ~~The configured cap was
/// not consulted at all.~~ Superseded: `bounds.steam_max_minutes` is parsed,
/// refused when it would widen, and narrowed by `kdtv-config`, and had no
/// caller anywhere — so an installation that set `steam_max_minutes = 5`
/// because the generator sits in a small enclosure got 20-minute sessions and
/// no clamp record, while the field looked enforced because the parser refuses
/// `30`. Its three siblings here — [`setpoint`], [`steam_setpoint`],
/// [`session`] — all consult [`Bounds`], and this one now does too.
fn steam_minutes(bounds: Bounds, minutes: u8) -> Result<SteamMinutes, ApiError> {
    let compiled = SteamMinutes::try_new(minutes)
        .map_err(|e| ApiError::rejected("steam session length", e))?;
    let cap = bounds.steam_max_minutes();
    if compiled.wire() > cap.wire() {
        return Err(ApiError::rejected(
            "steam session length",
            format!(
                "{minutes} minutes exceeds the {} minute maximum for this system",
                cap.wire()
            ),
        ));
    }
    Ok(compiled)
}

/// A requested session length.
///
/// Refused, not clamped, when it exceeds the configured maximum — the API
/// rejects out-of-envelope input and the encoder saturates as defence in depth,
/// which is the division of labour `ValveSetpoint` already draws.
fn session(bounds: Bounds, seconds: u64) -> Result<SessionDuration, ApiError> {
    let requested = std::time::Duration::from_secs(seconds);
    let cap = bounds.max_session();
    if requested > cap.get() {
        return Err(ApiError::rejected(
            "session length",
            format!(
                "{seconds} s exceeds the {} s maximum for this system",
                cap.get().as_secs()
            ),
        ));
    }
    SessionDuration::try_new(requested).map_err(|e| ApiError::rejected("session length", e))
}

// ---------------------------------------------------------------- handlers

/// `start(zone, outlet_set, temperature_f, duration_seconds)`. `API-01`.
///
/// The only handler that can open water, and the only one that takes a
/// [`FreshCaller`]. The authorisation is minted from the caller, carries this
/// boot's id and this command's id, and is moved into the call that spends it.
async fn start<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<FreshCaller>,
    Path(zone): Path<ZoneId>,
    Json(body): Json<StartBody>,
) -> Result<Response, ApiError> {
    let outlets = slots(&body.outlet_set)?;
    let temperature = setpoint(state.bounds(), body.temperature_f)?;
    let duration = session(state.bounds(), body.duration_seconds)?;
    let command = state.next_command()?;

    let request = ValidatedStart {
        zone,
        outlets,
        temperature,
        duration,
        command,
    };
    let authorization = caller.authorize(state.control().boot(), command);

    tracing::info!(
        %zone,
        command = command.0,
        session = caller.caller().session().0,
        outlets = ?outlets,
        temperature_f = temperature.fahrenheit(),
        duration_s = duration.get().as_secs(),
        "start requested"
    );
    let id = state
        .control()
        .start(request, authorization, caller.caller().source())
        .await?;
    Ok(accepted(id))
}

/// `set_temperature(zone, temperature_f)`. `API-01`.
async fn set_temperature<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<FreshCaller>,
    Path(zone): Path<ZoneId>,
    Json(body): Json<TemperatureBody>,
) -> Result<Response, ApiError> {
    let temp = setpoint(state.bounds(), body.temperature_f)?;
    let command = state.next_command()?;
    let id = state
        .control()
        .zone(
            zone,
            OperatorCommand::SetTemperature { temp, command },
            caller.caller().source(),
        )
        .await?;
    Ok(accepted(id))
}

/// `set_outlets(zone, outlet_set)`. `API-01`, and the one operation on the
/// design's list that the layers below cannot perform.
///
/// **This never transmits and never mints an authorisation.** Changing which
/// outlets are open *is* opening water, so it needs a grant, and the safety
/// kernel refuses to mint one for a zone that is already running —
/// `kdtv_engine::OperatorCommand` has no `SetOutlets` variant to spell it with.
/// The operation is denied by absence, one layer down.
///
/// The route exists rather than being omitted so the refusal is visible: a
/// client gets `409` and a reason, not a `404` that reads as a deployment
/// problem. The request is still validated first, so a malformed one is
/// answered `400` on its own terms.
///
/// This is a genuine disagreement between `API-01` and the engine's design, and
/// it is resolved in favour of the engine. Reopening it means deciding whether
/// a running zone may take an outlet change at all, which is a safety question
/// and a capture question, not an API one.
async fn set_outlets<C: Control>(
    State(_state): State<ApiState<C>>,
    Extension(_caller): Extension<FreshCaller>,
    Path(zone): Path<ZoneId>,
    Json(body): Json<OutletsBody>,
) -> Result<Response, ApiError> {
    let _validated = slots(&body.outlet_set)?;
    Err(ApiError::Refused(format!(
        "{zone}: changing which outlets are open opens water, so it needs a fresh \
         authorisation, and the safety kernel does not mint one for a running zone. \
         Stop the zone and start it again with the outlets you want."
    )))
}

/// `pause(zone)`. `API-01`. Reduces water, so no live session is required.
async fn pause<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<Caller>,
    Path(zone): Path<ZoneId>,
) -> Result<Response, ApiError> {
    let command = state.next_command()?;
    let id = state
        .control()
        .zone(zone, OperatorCommand::Pause { command }, caller.source())
        .await?;
    Ok(accepted(id))
}

/// `resume(zone)`. `API-01`. Opens water again, so it needs a live session.
async fn resume<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<FreshCaller>,
    Path(zone): Path<ZoneId>,
) -> Result<Response, ApiError> {
    let command = state.next_command()?;
    let id = state
        .control()
        .zone(
            zone,
            OperatorCommand::Resume { command },
            caller.caller().source(),
        )
        .await?;
    Ok(accepted(id))
}

/// `stop(zone)`. `API-01`.
async fn stop<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<Caller>,
    Path(zone): Path<ZoneId>,
) -> Result<Response, ApiError> {
    let command = state.next_command()?;
    let id = state
        .control()
        .zone(zone, OperatorCommand::Stop { command }, caller.source())
        .await?;
    Ok(accepted(id))
}

/// `stop_all()`. `API-01`, and `API-03`: one operator action, one command id,
/// three links — both valve zones and steam.
async fn stop_all<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<Caller>,
) -> Result<Response, ApiError> {
    let command = state.next_command()?;
    let id = state.control().stop_all(command, caller.source()).await?;
    Ok(accepted(id))
}

/// `get_cached_state()`. `API-01`, `API-06`.
///
/// **One atomic load out of the state cache.** No channel, no lock, no path to
/// a link, and therefore no way for a client to cause a bus transaction however
/// hard it calls this. That is the requirement, and it is what keeps the
/// replacement from regrowing the polling behaviour that hung the original
/// controller.
///
/// ~~The snapshot is cloned out of the `Arc` before serialising, so `serde`'s
/// `rc` feature is not needed workspace-wide for one field.~~ Superseded:
/// serialising through the reference needs neither, and the clone was a deep
/// copy of every zone and the steam status on **every status read**, on the
/// same thread as the control loop. `API-06` is written about bus traffic, but
/// what `I1` actually records is a wedged control loop, and a status read that
/// is free of frames and expensive in scheduler time is the same failure one
/// layer up.
async fn cached_state<C: Control>(State(state): State<ApiState<C>>) -> Response {
    let snapshot = state.control().snapshot();
    match serde_json::to_vec(&*snapshot) {
        Ok(body) => (
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            body,
        )
            .into_response(),
        // Unreachable: `SystemSnapshot` is plain data with no map keys that are
        // not strings. Written as a refusal rather than an `expect`, because
        // the alternative to a refusal on a read path is a panic in the
        // service that is holding the water off.
        Err(e) => ApiError::Unavailable(format!("the state cache could not be rendered: {e}"))
            .into_response(),
    }
}

/// `steam_start(temperature_f, duration_minutes)`. `API-02`.
///
/// The duration is never zero: `SteamMinutes` cannot represent it, because the
/// reverse-engineered notes describe `steamTimerSetTime = 0` as disabling the
/// generator's automatic shutoff — tier `[C]`, from third-party material, with
/// no capture taken.
///
/// **Whether that timer is the backstop that survives this service dying is an
/// open question, not a finding.** Kohler documents a generator auto-shutoff
/// after 20 minutes `[K]`; upstream describes the
/// `steamOnTicker`/`steamTimerSetTime` pair `[C]`; whether they are the same
/// timer is `STEAM-ADAPTER.md`'s contested item and its open question 2, marked
/// `[?]`. `[I]` Refusing zero is the conservative reading of an unresolved
/// question — it costs nothing if the two timers are independent, and it is the
/// only thing standing between a crashed master and a running boiler if they
/// are not.
async fn steam_start<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<FreshCaller>,
    Json(body): Json<SteamStartBody>,
) -> Result<Response, ApiError> {
    let temp = steam_setpoint(state.bounds(), body.temperature_f)?;
    let minutes = steam_minutes(state.bounds(), body.duration_minutes)?;
    let command = state.next_command()?;
    tracing::info!(
        command = command.0,
        session = caller.caller().session().0,
        temperature_f = temp.fahrenheit(),
        minutes = minutes.wire(),
        "steam start requested"
    );
    let id = state
        .control()
        .steam(
            SteamCommand::Start {
                temp,
                minutes,
                command,
            },
            caller.caller().source(),
        )
        .await?;
    Ok(accepted(id))
}

/// `steam_set_temperature(temperature_f)`. `API-02`.
async fn steam_set_temperature<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<FreshCaller>,
    Json(body): Json<SteamTemperatureBody>,
) -> Result<Response, ApiError> {
    let temp = steam_setpoint(state.bounds(), body.temperature_f)?;
    let command = state.next_command()?;
    let id = state
        .control()
        .steam(
            SteamCommand::SetTemperature { temp, command },
            caller.caller().source(),
        )
        .await?;
    Ok(accepted(id))
}

/// `steam_set_duration(minutes)`. `API-02`.
///
/// Extending a running steam session is water still moving, so this needs a
/// live session like the other two.
async fn steam_set_duration<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<FreshCaller>,
    Json(body): Json<SteamDurationBody>,
) -> Result<Response, ApiError> {
    let minutes = steam_minutes(state.bounds(), body.minutes)?;
    let command = state.next_command()?;
    let id = state
        .control()
        .steam(
            SteamCommand::SetDuration { minutes, command },
            caller.caller().source(),
        )
        .await?;
    Ok(accepted(id))
}

/// `steam_stop()`. `API-02`. Never gated on a live session.
async fn steam_stop<C: Control>(
    State(state): State<ApiState<C>>,
    Extension(caller): Extension<Caller>,
) -> Result<Response, ApiError> {
    let command = state.next_command()?;
    let id = state
        .control()
        .steam(SteamCommand::Stop { command }, caller.source())
        .await?;
    Ok(accepted(id))
}

/// The read-only event stream. `SVC-05`.
///
/// A `broadcast::Receiver` has no method that sends anything, so the stream is
/// read-only by construction rather than by check. A subscriber that falls
/// behind is told how many events it missed and never blocks the control loop —
/// a status client stalling the control loop is the failure this project
/// already lived through from the other side.
async fn events<C: Control>(
    State(state): State<ApiState<C>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(state.control().subscribe()).map(|next| {
        let event = match next {
            Ok(service_event) => Event::default()
                .json_data(&service_event)
                .unwrap_or_else(|_| Event::default().event("unserialisable").data("")),
            Err(BroadcastStreamRecvError::Lagged(missed)) => {
                Event::default().event("lagged").data(missed.to_string())
            }
        };
        Ok(event)
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}
