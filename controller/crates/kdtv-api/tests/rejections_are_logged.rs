//! `LOG-04`: every refusal the API makes reaches the journal with its reason.
//!
//! This file is a separate crate for a reason that is about `tracing` rather
//! than about visibility. A test that reads back what was logged has to install
//! a subscriber, and `tracing::subscriber::set_default` installs one *for the
//! calling thread* while the callsite-interest cache it is filtered through is
//! **global**. In `kdtv-api`'s unit-test binary fifty other tests provoke
//! refusals in parallel with no subscriber on their threads, and the capture
//! loses an arbitrary prefix of its events — a flake in the one test whose job
//! is to read the journal. An integration test binary runs this alone, so the
//! callsite is first reached with the subscriber already installed and the
//! result is the same on every run.
//!
//! What is asserted is the funnel: `IntoResponse for ApiError` is the single
//! place every rejection in the crate is rendered — the handlers' `?`, the
//! live-session gate, the not-found fallback and the service's own refusals all
//! pass through it — so a check cannot be added to that crate without a line
//! about it appearing here. The router-driven half, that each of those refusals
//! really is an `ApiError`, is the crate's own test module.

// An integration test is its own crate, so `lib.rs`'s `cfg_attr(test, ...)`
// header does not reach it and the workspace lints apply in full. Same allow as
// `kdtv-hal/tests/foreign_link.rs`, for the same reason.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use axum::response::IntoResponse as _;
use kdtv_api::ApiError;
use kdtv_units::{LinkKind, ZoneId};
use std::sync::{Arc, Mutex, PoisonError};

/// A `tracing` writer that keeps everything, so this can read back exactly what
/// would have gone to the journal.
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

/// `LOG-04`: log local safety clamps and the rejection reason.
///
/// Every variant is rendered, so this fails if a new one is added and the
/// funnel is bypassed, and it fails if the funnel stops logging.
///
/// ~~`ApiError::rejected` names the check, which is what `LOG-04` asks for.~~
/// Superseded: naming it in the response body reaches the client and nothing
/// else. `kdtv-api` had exactly three `tracing` calls and none was on a
/// rejection path, so every check the API owns — the setpoint clamps, the
/// configured bounds, the session length, the slot validation, the steam
/// envelope, the missing live session, the unminted command id — refused
/// silently. `kdtv-service` implements `LOG-01` and `LOG-04` for what reaches
/// it, and none of those ever do.
#[test]
fn req_design_log_04() {
    let sink = LogSink::default();
    let every_refusal = [
        ApiError::Unauthenticated("no API credential was presented"),
        ApiError::NoLiveSession,
        ApiError::rejected(
            "valve setpoint clamp",
            "115.0 °F is above the 108.5 °F ceiling",
        ),
        ApiError::rejected("configured setpoint bound", "104.8 °F is above the ceiling"),
        ApiError::rejected("outlet slot", "9 is not a slot"),
        ApiError::rejected("session length", "9000 s exceeds the 1200 s maximum"),
        ApiError::rejected("steam setpoint clamp", "126 °F is outside the envelope"),
        ApiError::rejected("steam session length", "0 minutes disables the shutoff"),
        ApiError::Refused("the safety kernel would not mint a grant".to_owned()),
        ApiError::NoSuchLink(LinkKind::Steam),
        ApiError::TooSoon(LinkKind::Zone(ZoneId::Zone1)),
        ApiError::Unavailable("the service is stopping water".to_owned()),
        ApiError::NoCommandId("the state directory is read-only".to_owned()),
        ApiError::NoSuchOperation,
    ];

    {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(sink.clone())
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        for refusal in &every_refusal {
            let response = refusal.clone().into_response();
            assert!(
                response.status().is_client_error() || response.status().is_server_error(),
                "{refusal} rendered as {}",
                response.status()
            );
        }
    }

    let logged = sink.contents();
    let lines = logged.lines().count();
    assert_eq!(
        lines,
        every_refusal.len(),
        "one line per refusal, and no refusal without one: {logged}"
    );
    for refusal in &every_refusal {
        // The stable kind a client branches on, and the human reason.
        assert!(
            logged.contains(refusal.kind()),
            "{refusal} was refused without its kind reaching the journal: {logged}"
        );
        let detail = refusal.to_string();
        let head: String = detail.chars().take(24).collect();
        assert!(
            logged.contains(&head),
            "{refusal} was refused without its reason reaching the journal: {logged}"
        );
    }
    // And the check that refused it, where a local check did. `LOG-04` names
    // "local safety clamps" specifically.
    for check in [
        "valve setpoint clamp",
        "configured setpoint bound",
        "outlet slot",
        "session length",
        "steam setpoint clamp",
        "steam session length",
    ] {
        assert!(logged.contains(check), "{check} named no clamp: {logged}");
    }
}

/// `LOG-09` over the same lines: a refusal never carries a credential.
///
/// The two [`ApiError::Unauthenticated`] messages are static strings, and they
/// are chosen so that a journal grepped for a leaked token does not match on
/// them — the first of them used to name the `Authorization: Bearer` header,
/// which is indistinguishable, to that grep, from the leak itself.
#[test]
fn a_refusal_line_never_looks_like_a_leaked_credential() {
    let sink = LogSink::default();
    {
        let subscriber = tracing_subscriber::fmt()
            .with_writer(sink.clone())
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);
        let _ = ApiError::Unauthenticated("no API credential was presented").into_response();
        let _ = ApiError::Unauthenticated("the credential did not verify").into_response();
        let _ = ApiError::NoLiveSession.into_response();
    }
    let logged = sink.contents().to_ascii_lowercase();
    assert!(!logged.is_empty(), "the refusals must have been logged");
    for word in ["authorization", "bearer ", "token"] {
        assert!(
            !logged.contains(word),
            "{word:?} reached a refusal line: {logged}"
        );
    }
}
