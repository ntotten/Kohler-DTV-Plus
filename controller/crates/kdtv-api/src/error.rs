//! What a refused request looks like on the wire.
//!
//! A refusal here **transmits nothing and changes no valve state**. That is the
//! line `DESIGN.md` § Safety boundary rule 9 draws between invalid
//! input and invalid wire data: bad input is rejected to the caller, bad wire
//! data escalates to all-off. Every variant below is the first of the two.
//!
//! Every refusal is written to the journal as it is rendered, in one place —
//! [`IntoResponse for ApiError`](ApiError#impl-IntoResponse-for-ApiError).
//! `LOG-04` asks for the local safety clamps and the rejection reason, and the
//! API owns checks the layers below never see: `ValveSetpoint::from_fahrenheit`,
//! the configured setpoint and steam bounds, the session length, the slot
//! validation, the steam envelope, the missing live session, the unminted
//! command id. A refusal made here and not logged here is invisible, because
//! `kdtv-service` — which does implement `LOG-01` and `LOG-04` — is never
//! reached. `tests/rejections_are_logged.rs` reads the lines back.
//!
//! No variant carries a credential, a session token or any part of one.
//! `LOG-09`; the crate's test module asserts it over the rendered bodies.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use kdtv_service::CommandError;
use kdtv_units::LinkKind;
use serde::Serialize;

/// Why a request did not take effect.
#[derive(Clone, Debug, thiserror::Error)]
pub enum ApiError {
    /// No credential, or one that did not verify. `401`.
    #[error("unauthenticated: {0}")]
    Unauthenticated(&'static str),

    /// The credential verified but the request did not arrive on a session that
    /// was already live, and this operation opens water. `401`.
    #[error(
        "this operation needs a session that was already live: repeat the request with the \
         x-kdtv-session value from a previous authenticated response. A stop never needs one."
    )]
    NoLiveSession,

    /// The request was well-formed HTTP and not a valid command. `400`.
    ///
    /// `check` names the clamp or rule that refused it, which is what `LOG-04`
    /// asks for.
    #[error("{detail}")]
    Rejected { check: &'static str, detail: String },

    /// The service refused: the safety kernel, the zone machine or the steam
    /// machine said no. `409`.
    #[error("{0}")]
    Refused(String),

    /// The link is not configured on this service. `404`.
    #[error("{0} is not configured on this service")]
    NoSuchLink(LinkKind),

    /// A command arrived before the previous one on that bus finished. Nothing
    /// was transmitted and the caller may retry. `429`.
    #[error("{0}: the previous command on this bus has not finished")]
    TooSoon(LinkKind),

    /// The service is stopping water, or has stopped. `503`.
    #[error("{0}")]
    Unavailable(String),

    /// The command id counter could not issue, so nothing was attempted. `503`.
    #[error("{0}")]
    NoCommandId(String),

    /// The requested route is not part of the API surface. `404`.
    #[error("no such operation: the API exposes only the operations in DESIGN.md")]
    NoSuchOperation,
}

/// The body every refusal renders as.
#[derive(Serialize)]
struct Body<'a> {
    /// A stable machine-readable kind, so a client can branch without parsing
    /// prose.
    error: &'a str,
    /// The check that refused it, where one is named.
    #[serde(skip_serializing_if = "Option::is_none")]
    check: Option<&'a str>,
    detail: String,
}

impl ApiError {
    /// The stable kind string. Not the message.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Unauthenticated(_) => "unauthenticated",
            Self::NoLiveSession => "no_live_session",
            Self::Rejected { .. } => "rejected",
            Self::Refused(_) => "refused",
            Self::NoSuchLink(_) => "no_such_link",
            Self::TooSoon(_) => "too_soon",
            Self::Unavailable(_) => "unavailable",
            Self::NoCommandId(_) => "no_command_id",
            Self::NoSuchOperation => "no_such_operation",
        }
    }

    #[must_use]
    pub const fn status(&self) -> StatusCode {
        match self {
            Self::Unauthenticated(_) | Self::NoLiveSession => StatusCode::UNAUTHORIZED,
            Self::Rejected { .. } => StatusCode::BAD_REQUEST,
            Self::Refused(_) => StatusCode::CONFLICT,
            Self::NoSuchLink(_) | Self::NoSuchOperation => StatusCode::NOT_FOUND,
            Self::TooSoon(_) => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable(_) | Self::NoCommandId(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    /// A rejection by a named local check. `LOG-04`.
    #[must_use]
    pub fn rejected(check: &'static str, detail: impl std::fmt::Display) -> Self {
        Self::Rejected {
            check,
            detail: detail.to_string(),
        }
    }
}

impl From<CommandError> for ApiError {
    /// The service's refusals, mapped to status codes.
    ///
    /// `TooSoon` is `429` and not `409` because it is the one refusal a client
    /// should retry: nothing was transmitted, the bus is simply still holding
    /// the previous transaction. Everything the kernel or a machine refused is
    /// `409` — the request was understood and the system's state does not allow
    /// it, which is not something a retry fixes.
    fn from(e: CommandError) -> Self {
        match e {
            CommandError::NoSuchLink(link) => Self::NoSuchLink(link),
            CommandError::TooSoon { link } => Self::TooSoon(link),
            CommandError::ShuttingDown | CommandError::NotRunning => {
                Self::Unavailable(e.to_string())
            }
            CommandError::Denied(_)
            | CommandError::ZoneRefused(_)
            | CommandError::SteamRefused(_) => Self::Refused(e.to_string()),
        }
    }
}

impl IntoResponse for ApiError {
    /// **Every refusal is logged here.** `LOG-04`.
    ///
    /// This is the one funnel every rejection in the crate passes through — the
    /// handlers' `?`, the live-session gate, the not-found fallback and the
    /// service's own refusals — so a check cannot be added to this crate
    /// without the journal getting a line naming it. The alternative, a
    /// `tracing` call at each rejection site, is a list someone has to keep
    /// complete; this one cannot be incomplete.
    ///
    /// `detail` is the rendered error, which by construction carries no
    /// credential (`LOG-09`): no variant holds one, and the two
    /// [`ApiError::Unauthenticated`] messages are static strings chosen so that
    /// a journal grepped for a leaked token does not match on this line.
    fn into_response(self) -> Response {
        let check = match &self {
            Self::Rejected { check, .. } => Some(*check),
            _ => None,
        };
        let status = self.status();
        tracing::warn!(
            error = self.kind(),
            check = check.unwrap_or("-"),
            status = status.as_u16(),
            detail = %self,
            "an API request was refused"
        );
        let body = Body {
            error: self.kind(),
            check,
            detail: self.to_string(),
        };
        (status, Json(body)).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_units::ZoneId;

    #[test]
    fn the_service_refusals_map_to_distinct_and_meaningful_statuses() {
        let cases = [
            (
                CommandError::NoSuchLink(LinkKind::Steam),
                StatusCode::NOT_FOUND,
            ),
            (
                CommandError::TooSoon {
                    link: LinkKind::Zone(ZoneId::Zone1),
                },
                StatusCode::TOO_MANY_REQUESTS,
            ),
            (CommandError::ShuttingDown, StatusCode::SERVICE_UNAVAILABLE),
            (CommandError::NotRunning, StatusCode::SERVICE_UNAVAILABLE),
        ];
        for (from, want) in cases {
            let rendered = format!("{from}");
            let mapped = ApiError::from(from);
            assert_eq!(mapped.status(), want, "{rendered}");
        }
    }

    #[test]
    fn a_rejection_names_the_check_that_refused_it() {
        let e = ApiError::rejected(
            "valve setpoint clamp",
            "120.0 °F is above the 108.5 °F ceiling",
        );
        assert_eq!(e.status(), StatusCode::BAD_REQUEST);
        assert_eq!(e.kind(), "rejected");
        assert!(e.to_string().contains("108.5"));
    }

    #[test]
    fn every_kind_is_distinct_so_a_client_can_branch_on_it() {
        let all = [
            ApiError::Unauthenticated("x"),
            ApiError::NoLiveSession,
            ApiError::rejected("c", "d"),
            ApiError::Refused("r".into()),
            ApiError::NoSuchLink(LinkKind::Steam),
            ApiError::TooSoon(LinkKind::Steam),
            ApiError::Unavailable("u".into()),
            ApiError::NoCommandId("n".into()),
            ApiError::NoSuchOperation,
        ];
        let kinds: std::collections::BTreeSet<&str> = all.iter().map(ApiError::kind).collect();
        assert_eq!(kinds.len(), all.len());
    }
}
