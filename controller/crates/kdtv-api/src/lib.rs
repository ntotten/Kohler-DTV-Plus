//! The local authenticated API and the read-only event stream. `SVC-05`.
//!
//! # The surface is closed
//!
//! `DESIGN.md` § Software design lists twelve public operations —
//! eight for the valves (`API-01`) and four for steam (`API-02`) — and the
//! router is built from a table that names exactly those, plus the event stream
//! `SVC-05` requires. There is no route outside [`routes::surface`], because the
//! router is *generated* from it rather than checked against it, and
//! the crate's own test module asserts the table against the design list
//! verbatim.
//!
//! A route that is not in the design is not a feature. It is an unreviewed way
//! to move water.
//!
//! # A status read never reaches a bus
//!
//! `API-06`. `GET /v1/state` calls [`kdtv_service::ServiceHandle::snapshot`],
//! which is one atomic load out of an `arc_swap::ArcSwap`: no channel, no lock
//! and no path to a link. Hammering it changes the transmitted frame count by
//! zero. This is the requirement that stops the replacement regrowing the
//! polling behaviour that hung the original controller (`INVESTIGATIONS.md`
//! I1), and it is the reason the cache exists at all.
//!
//! # What can open water, and what it takes to get there
//!
//! [`kdtv_service::surface::StartAuthorization`] is the workspace's only token
//! that leads to an open valve, and this crate is the only place that mints one.
//! The path to `StartAuthorization::issue` is a chain of types, each of which
//! can only be produced by the step before it:
//!
//! ```text
//!   request
//!     └─ authenticate      constant-time token comparison   → Caller
//!         └─ fresh session  established, not expired        → FreshCaller
//!             └─ FreshCaller::authorize                     → StartAuthorization
//!                 └─ ServiceHandle::start                   (moved, spent once)
//! ```
//!
//! [`auth::Caller`] and [`auth::FreshCaller`] have private fields and no public
//! constructor, so a handler cannot fabricate either. There is exactly one call
//! to `StartAuthorization::issue` in this crate and a test counts it.
//!
//! The authorisation carries the service boot id, so a restart invalidates every
//! outstanding one. That is the whole of "a restart cannot replay a start".
//!
//! # Nothing here speaks the wire
//!
//! This crate declares no dependency on `kdtv-proto`, `kdtv-safety` or
//! `kdtv-engine`. `cargo xtask audit-graph` asserts the first of those, and the
//! consequence is that no handler can name a Saturn or DTV+ frame, let alone
//! build one. `outlet_set` crosses this boundary as configuration slot numbers
//! (`API-04`); the translation to a wire bitmap happens two crates below and is
//! not reimplemented here.
//!
//! # Evidence
//!
//! Everything about the Saturn and DTV+ wire protocols in this workspace is
//! tier `[C]` — reverse-engineered from third-party material, with no capture
//! taken against this hardware. Nothing in this crate has been exercised against
//! a valve.

// Tests legitimately panic on a broken invariant; the production lints stay on
// for library code, where a panic is a fault, not a diagnosis.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp
    )
)]

pub mod auth;
pub mod control;
pub mod error;
pub mod routes;
pub mod serve;

#[cfg(test)]
mod tests;

pub use auth::{
    ApiToken, Authenticator, Caller, FreshCaller, Freshness, MAX_SESSIONS, MIN_TOKEN_BYTES,
    SESSION_HEADER, Sessions, TokenError,
};
pub use control::{CommandIds, Control, IdUnavailable};
pub use error::ApiError;
pub use routes::{BODY_LIMIT, Op, Requirement, router, surface};
pub use serve::{Api, ApiState, BindError, SHUTDOWN_GRACE};
