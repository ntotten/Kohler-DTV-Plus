//! The credential, the session, and the two types that stand between a request
//! and an open valve.
//!
//! # The credential
//!
//! The token is read once, from [`kdtv_config::ApiConfig::token_file`]. In
//! production that path is `/run/credentials/kdtvd.service/api-token`, which
//! systemd populates from the unit's `LoadCredential=` and which exists only
//! while the unit runs — so the secret is not in the configuration file, not in
//! the repository, and not on disk when the service is stopped. `kdtv-config`
//! has already refused to start if the file is missing or world-readable; it
//! never reads the contents, and this is the one place that does.
//!
//! Three things are structural rather than remembered:
//!
//! - **It is compared in constant time.** `subtle::ConstantTimeEq`, not `==`.
//!   An `==` on a secret is a timing oracle: it returns on the first differing
//!   byte, so an attacker on loopback can recover the token one byte at a time.
//!   Lengths are compared first and that comparison does leak the length, which
//!   is unavoidable and acceptable — the token's length is not the secret.
//! - **It is zeroized.** The bytes live in a `zeroize::Zeroizing<Vec<u8>>`, so
//!   both the file buffer and the trimmed token are wiped when dropped rather
//!   than left in the allocator for whatever reads that page next.
//! - **It cannot be printed.** [`kdtv_telemetry::Redacted`] renders
//!   `[redacted]` in `Debug` and in `Serialize`, so neither a structured log
//!   line nor a `dbg!` nor an error body can carry it. `LOG-09`.
//! - **It does not travel past [`authenticate`].** The `Authorization` header
//!   is removed from the request once it has verified. `hyper`'s `HeaderValue`
//!   is not zeroized on drop, so the three properties above would otherwise be
//!   true only of this module's copies and false of the authoritative one, and
//!   anything layered inside the authentication — a request-tracing layer, a
//!   panic catcher, a handler that echoes headers — would see the token.
//!
//! # `session_ttl`, and what it does and does not buy
//!
//! `api.session_ttl_s` bounds the life of an API **session**. A session is
//! established by the first authenticated request that does not present a live
//! one, is identified by an opaque [`kdtv_units::SessionId`], and expires
//! `session_ttl` after it was established — an absolute cap, not a sliding idle
//! timeout, so an active client's session expires too.
//!
//! It does three concrete things:
//!
//! 1. It is what appears in `RequestSource::LocalApi { session, peer }`, so
//!    every command in the log belongs to one authenticated span (`LOG-01`).
//! 2. It bounds the session table: expired entries are dropped on the next
//!    request rather than accumulating for the life of the process.
//! 3. It gates the operations that **open or increase water**. `BOOT-07` says a
//!    start is accepted "only after a fresh authenticated session and explicit
//!    user command", and that is two steps: a request that establishes a
//!    session, then a command on it. A request that establishes a session and
//!    opens water in the same breath is refused.
//!
//! **A stop is never gated on it.** `pause`, `stop`, `stop_all` and
//! `steam_stop` need the token like everything else, but not a live session. An
//! expired session must never stand between an operator and turning the water
//! off.
//!
//! **What it is not:** with a static bearer token, expiry costs a live client
//! one extra presentation of the same credential, so this is not an
//! authentication-strength boundary and is not described as one. What actually
//! stops a start being replayed across a restart is the boot id inside
//! [`kdtv_service::surface::StartAuthorization`], which no restart survives;
//! what keeps the credential reachable at all is the loopback bind `OPS-04`
//! requires, because the controller has no authentication of its own and
//! anything that can reach this API can run the shower.
//!
//! # The chain to an authorisation
//!
//! [`Caller`] is produced only by [`authenticate`], after the token has
//! verified. [`FreshCaller`] is produced only by [`require_fresh_session`],
//! from a [`Caller`] whose session is live. [`FreshCaller::authorize`] is the
//! only call to `StartAuthorization::issue` in this crate. Both types have
//! private fields and no public constructor, so there is no path from an
//! unauthenticated request to an authorisation that does not go through both.

use std::collections::{HashMap, VecDeque};
use std::hash::{BuildHasher as _, Hasher as _, RandomState};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::{IntoResponse as _, Response};
use kdtv_service::surface::StartAuthorization;
use kdtv_telemetry::{Redacted, RequestSource};
use kdtv_units::{BootId, CommandId, SessionId};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

use crate::error::ApiError;

/// The header a client presents a session id in, and the one every response
/// carries it back in.
pub const SESSION_HEADER: &str = "x-kdtv-session";

/// The most sessions this boot will hold at once.
///
/// A session is established by any authenticated request that does not present
/// a live one, so a client polling status without keeping its session id makes
/// a new entry every time. Expiry alone bounds that by
/// `session_ttl` × request rate, which on a local API is small but is not a
/// number this service chooses. This is. When the table is full the oldest
/// entry is dropped, which costs whoever held it one refused water-opening
/// request and a new session — never a refused stop.
pub const MAX_SESSIONS: usize = 1024;

/// The shortest token this service will serve.
///
/// The API can run a shower. A credential that a person typed is not one, and
/// refusing it at startup is better than refusing it in an incident report.
pub const MIN_TOKEN_BYTES: usize = 16;

/// Why the credential could not be loaded.
///
/// No variant carries the token or any part of it. `LOG-09`.
#[derive(Debug, thiserror::Error)]
pub enum TokenError {
    #[error("cannot read the API token at {}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the API token at {} is empty", path.display())]
    Empty { path: PathBuf },
    #[error(
        "the API token at {} is {found} bytes; at least {least} are required, because \
         anything that can reach this API can run the shower",
        path.display()
    )]
    TooShort {
        path: PathBuf,
        found: usize,
        least: usize,
    },
}

/// The API credential.
///
/// `Debug` prints `[redacted]`, the bytes are zeroized on drop, and the only
/// operation is a constant-time comparison. There is no accessor that returns
/// the token.
#[derive(Debug)]
pub struct ApiToken {
    token: Redacted<Zeroizing<Vec<u8>>>,
}

impl ApiToken {
    /// Read the token from the file the service manager supplied.
    ///
    /// Leading and trailing ASCII whitespace is trimmed, because a credential
    /// file written by `systemd-creds` or by an editor ends in a newline and an
    /// operator should not have to know that. The untrimmed buffer is zeroized
    /// when it drops.
    pub async fn load(path: &Path) -> Result<Self, TokenError> {
        let raw =
            Zeroizing::new(
                tokio::fs::read(path)
                    .await
                    .map_err(|source| TokenError::Read {
                        path: path.to_path_buf(),
                        source,
                    })?,
            );
        Self::from_bytes(path, &raw)
    }

    /// The same, from a blocking read.
    ///
    /// `kdtvd --check-only` validates before a runtime exists, and the
    /// credential is the one startup failure a deployment cannot recover from
    /// remotely: by the time `bring_up` refuses a token that is too short, the
    /// old binary is already gone. So the pre-flight reads it too, and this is
    /// what it reads it with. The buffer is zeroized the same way.
    pub fn load_blocking(path: &Path) -> Result<Self, TokenError> {
        let raw = Zeroizing::new(std::fs::read(path).map_err(|source| TokenError::Read {
            path: path.to_path_buf(),
            source,
        })?);
        Self::from_bytes(path, &raw)
    }

    /// The same, from bytes already in hand. The caller owns wiping them.
    pub fn from_bytes(path: &Path, raw: &[u8]) -> Result<Self, TokenError> {
        let trimmed: &[u8] =
            raw.iter()
                .position(|b| !b.is_ascii_whitespace())
                .map_or(&[], |first| {
                    let last = raw
                        .iter()
                        .rposition(|b| !b.is_ascii_whitespace())
                        .unwrap_or(first);
                    raw.get(first..=last).unwrap_or(&[])
                });
        if trimmed.is_empty() {
            return Err(TokenError::Empty {
                path: path.to_path_buf(),
            });
        }
        if trimmed.len() < MIN_TOKEN_BYTES {
            return Err(TokenError::TooShort {
                path: path.to_path_buf(),
                found: trimmed.len(),
                least: MIN_TOKEN_BYTES,
            });
        }
        Ok(Self {
            token: Redacted::new(Zeroizing::new(trimmed.to_vec())),
        })
    }

    /// Constant-time comparison against what a client presented.
    ///
    /// The length check short-circuits and leaks the token's length. That is
    /// the standard trade and it is stated rather than hidden: the length is
    /// not the secret, and hashing both sides to hide it would add a dependency
    /// to conceal something already visible in the credential file's size.
    #[must_use]
    pub fn verify(&self, presented: &[u8]) -> bool {
        let expected: &[u8] = self.token.expose();
        if expected.len() != presented.len() {
            return false;
        }
        expected.ct_eq(presented).into()
    }
}

/// Whether the request arrived on a session that already existed.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Freshness {
    /// The request presented a session id that exists and has not expired.
    Live,
    /// No live session was presented, so one was established for this request.
    /// Read operations proceed; anything that opens water does not.
    Established,
}

/// The sessions this boot has issued.
///
/// # An id is not a credential, and is not guessable either
///
/// The bearer token is what authenticates; a session id proves nothing on its
/// own and is primarily a correlation handle, which is what
/// `RequestSource::LocalApi` wants it for. But it is also load-bearing: a
/// request that presents a **live** id is one step from an open valve, because
/// that is the second of `BOOT-07`'s two steps.
///
/// ~~Ids were a counter starting at 1 on every boot.~~ Superseded. A counter
/// makes the second step guessable and, worse, makes it *replayable*: a stored
/// request carrying `x-kdtv-session: 1` is accepted by a daemon that has just
/// restarted and served one other request, which is exactly the one-step start
/// the two-step rule exists to refuse. So an id is now drawn from a keyed
/// pseudorandom function over a private counter, with the key taken from the
/// operating system when the table is built:
///
/// - **Unguessable.** The counter is never exposed; what a client sees is
///   `PRF(key, n)` over 64 bits. Trying ids until one is live is not a
///   strategy.
/// - **Boot-unique.** A new process draws a new key, so no id issued before a
///   restart is valid after one — the same property the boot id inside
///   [`StartAuthorization`] gives a start, applied to the session gate that
///   precedes it.
///
/// `std::hash::RandomState` is the key source rather than a new dependency: it
/// is seeded from the operating system's randomness, and `[I]` its keyed
/// `SipHash-1-3` is not invertible from a handful of outputs, which is what
/// unpredictability here needs. A dedicated CSPRNG would be the better answer
/// and needs a crate this workspace does not have — recorded rather than
/// silently traded away. Lookup is a hash-table probe and so is not constant
/// time; `[I]` a timing side channel on a 64-bit random id, behind a token that
/// has already verified, is not the exposure this is protecting against.
///
/// # The table costs the control loop a constant per request
///
/// The daemon runs this API on the same `current_thread` runtime as the control
/// loop, so work done here is work the 525 ms link tick does not get.
/// ~~Expiry swept the whole table and eviction scanned it for the oldest
/// entry, on every authenticated request.~~ Superseded: `order` holds ids in
/// establishment order, which is expiry order, so both are a `pop_front` and
/// `admit` is O(1) amortised. `API-06` is written about bus traffic, but the
/// failure `I1` records is a wedged control loop, and a status read must be
/// cheap in scheduler time as well as in frames.
#[derive(Debug)]
pub struct Sessions {
    ttl: Duration,
    table: Mutex<Table>,
}

#[derive(Debug)]
struct Table {
    /// What the id is derived from. Never leaves this struct.
    counter: u64,
    /// The process-private key. Two boots derive different ids from the same
    /// counter, which is what makes a captured request unreplayable.
    keys: RandomState,
    live: HashMap<u64, tokio::time::Instant>,
    /// Ids in the order they were established, which — the time-to-live being
    /// an absolute cap rather than a sliding one — is the order they expire in.
    /// Keeps expiry and eviction O(1) instead of a scan of the whole table.
    order: VecDeque<u64>,
}

impl Table {
    /// Drop everything past its time-to-live. O(1) per entry dropped, and
    /// nothing at all when the oldest is still live.
    fn expire(&mut self, now: tokio::time::Instant, ttl: Duration) {
        while let Some(&oldest) = self.order.front() {
            let gone = self
                .live
                .get(&oldest)
                .is_none_or(|established| now.duration_since(*established) >= ttl);
            if !gone {
                return;
            }
            self.order.pop_front();
            self.live.remove(&oldest);
        }
    }

    /// An id a client cannot predict and a restart cannot reissue.
    fn mint(&mut self) -> u64 {
        loop {
            self.counter = self.counter.wrapping_add(1);
            let mut hasher = self.keys.build_hasher();
            hasher.write_u64(self.counter);
            let id = hasher.finish();
            // Zero is excluded so that "no session" and "session 0" cannot be
            // confused in a log line; a collision is answered by drawing again
            // rather than by handing two clients one session.
            if id != 0 && !self.live.contains_key(&id) {
                return id;
            }
        }
    }
}

impl Sessions {
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            table: Mutex::new(Table {
                counter: 0,
                keys: RandomState::new(),
                live: HashMap::new(),
                order: VecDeque::new(),
            }),
        }
    }

    #[must_use]
    pub const fn ttl(&self) -> Duration {
        self.ttl
    }

    /// How many sessions are currently held. Expired ones are dropped by
    /// [`Sessions::admit`], so this is the size of the table, not of history.
    #[must_use]
    pub fn live(&self) -> usize {
        self.lock().live.len()
    }

    /// Resolve the session a request arrives on.
    ///
    /// A presented id that exists and is inside the time-to-live is kept, with
    /// its original establishment time — the cap is absolute, so an active
    /// client's session still expires. Anything else establishes a new one.
    pub fn admit(&self, presented: Option<u64>) -> (SessionId, Freshness) {
        // `tokio::time::Instant`, not `kdtv_hal::Clock`. This crate does not
        // depend on `kdtv-hal` and must not — the same absence that keeps a
        // command id minted where it is made durable. The tokio clock is the
        // injectable one here: `#[tokio::test(start_paused = true)]` drives the
        // whole time-to-live in microseconds, which is what `clippy.toml`'s ban
        // on `std::time::Instant::now` is asking for. And the dependency runs
        // the other way — `kdtv-service` does not know this crate exists — so a
        // `Clock` fake in its harness could not reach this table however it was
        // built. Recorded here so the choice reads as one.
        let now = tokio::time::Instant::now();
        let ttl = self.ttl;
        let mut table = self.lock();
        table.expire(now, ttl);

        if let Some(id) = presented
            && table.live.contains_key(&id)
        {
            return (SessionId(id), Freshness::Live);
        }

        // Bounded. The oldest goes, because it is the one closest to expiring
        // anyway, and `order` already holds them in that order.
        while table.live.len() >= MAX_SESSIONS {
            let Some(oldest) = table.order.pop_front() else {
                break;
            };
            table.live.remove(&oldest);
        }

        let id = table.mint();
        table.live.insert(id, now);
        table.order.push_back(id);
        (SessionId(id), Freshness::Established)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The credential and the session table, as one piece of middleware state.
#[derive(Debug)]
pub struct Authenticator {
    token: ApiToken,
    sessions: Sessions,
}

impl Authenticator {
    #[must_use]
    pub const fn new(token: ApiToken, sessions: Sessions) -> Self {
        Self { token, sessions }
    }

    #[must_use]
    pub const fn sessions(&self) -> &Sessions {
        &self.sessions
    }
}

/// A request that has presented the API credential.
///
/// Private fields, no public constructor: [`authenticate`] is the only thing
/// that can produce one, and it only does so after [`ApiToken::verify`] has
/// returned true.
#[derive(Clone, Debug)]
pub struct Caller {
    session: SessionId,
    peer: String,
    freshness: Freshness,
}

impl Caller {
    #[must_use]
    pub const fn session(&self) -> SessionId {
        self.session
    }

    #[must_use]
    pub fn peer(&self) -> &str {
        &self.peer
    }

    #[must_use]
    pub const fn freshness(&self) -> Freshness {
        self.freshness
    }

    /// How this caller is recorded on every command. `LOG-01`.
    #[must_use]
    pub fn source(&self) -> RequestSource {
        RequestSource::LocalApi {
            session: self.session.0,
            peer: self.peer.clone(),
        }
    }
}

/// A caller whose session was already live when the request arrived.
///
/// The only type in this crate that can mint an authorisation to open water.
/// Produced only by [`require_fresh_session`], which is layered onto exactly the
/// routes marked [`crate::Op::opens_water`].
#[derive(Clone, Debug)]
pub struct FreshCaller(Caller);

impl FreshCaller {
    #[must_use]
    pub const fn caller(&self) -> &Caller {
        &self.0
    }

    /// Mint the workspace's only authorisation to open water.
    ///
    /// **The single call to `StartAuthorization::issue` in this crate.** It is
    /// reachable only from a `FreshCaller`, which is reachable only from a
    /// `Caller`, which is reachable only from a verified token — so there is no
    /// path here from an unauthenticated request.
    ///
    /// The authorisation carries `boot`, and the kernel refuses one minted
    /// under any other boot id. It is `!Clone` and is moved into the command
    /// that spends it.
    #[must_use]
    pub fn authorize(&self, boot: BootId, command: CommandId) -> StartAuthorization {
        StartAuthorization::issue(boot, command)
    }
}

/// Verify the credential, resolve the session, and refuse everything else.
///
/// Layered onto the whole router, so it runs before any handler and before the
/// not-found fallback: an unauthenticated request to a path that does not exist
/// is answered `401`, not `404`, and learns nothing about the surface.
pub async fn authenticate(
    State(auth): State<Arc<Authenticator>>,
    mut request: Request,
    next: Next,
) -> Response {
    let presented = bearer(&request);
    let Some(presented) = presented else {
        // The message names no header. Every refusal is logged (`LOG-04`), and
        // a line carrying the word `Authorization` next to the word `Bearer` is
        // indistinguishable, to whatever greps the journal for a leaked
        // credential, from the leak itself.
        return refuse(ApiError::Unauthenticated("no API credential was presented"));
    };
    if !auth.token.verify(presented.expose()) {
        // Nothing about what was presented reaches the log or the body.
        tracing::warn!(
            peer = %peer_of(&request),
            "an API request presented a credential that did not verify"
        );
        return refuse(ApiError::Unauthenticated("the credential did not verify"));
    }

    // **The credential goes no further than this line.** `presented` is a
    // zeroized copy; the header itself is a `hyper::HeaderValue`, which is not
    // wiped when it drops, so leaving it attached would put the token in front
    // of every handler and every layer inside this one — a `TraceLayer` with
    // `include_headers(true)` is a one-argument change away, and would print it
    // on every request. Removing it makes `LOG-09` a property of the request
    // rather than of which middleware happens to be installed today.
    request
        .headers_mut()
        .remove(axum::http::header::AUTHORIZATION);

    let offered = request
        .headers()
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let (session, freshness) = auth.sessions.admit(offered);

    let caller = Caller {
        session,
        peer: peer_of(&request),
        freshness,
    };
    request.extensions_mut().insert(caller);

    let mut response = next.run(request).await;
    stamp_session(&mut response, session);
    response
}

/// Require a session that was already live. Layered onto the water-opening
/// routes only.
pub async fn require_fresh_session(request: Request, next: Next) -> Response {
    let caller = request.extensions().get::<Caller>().cloned();
    let Some(caller) = caller else {
        // Unreachable while `authenticate` wraps the whole router. Written as a
        // refusal rather than an `expect`, because the alternative to a refusal
        // on this path is a panic between a request and a valve.
        return refuse(ApiError::Unauthenticated(
            "this request did not pass authentication",
        ));
    };
    if caller.freshness != Freshness::Live {
        let session = caller.session;
        let mut response = refuse(ApiError::NoLiveSession);
        stamp_session(&mut response, session);
        return response;
    }
    let mut request = request;
    request.extensions_mut().insert(FreshCaller(caller));
    next.run(request).await
}

/// The bearer token a request presented, wrapped so it cannot be printed and is
/// wiped when the request is done with.
fn bearer(request: &Request) -> Option<Redacted<Zeroizing<Vec<u8>>>> {
    let value = request.headers().get(axum::http::header::AUTHORIZATION)?;
    let text = value.to_str().ok()?;
    let token = text.strip_prefix("Bearer ").or_else(|| {
        // Case-insensitive scheme, as RFC 7235 requires.
        let (scheme, rest) = text.split_once(' ')?;
        scheme.eq_ignore_ascii_case("bearer").then_some(rest)
    })?;
    let token = token.trim();
    (!token.is_empty()).then(|| Redacted::new(Zeroizing::new(token.as_bytes().to_vec())))
}

fn peer_of(request: &Request) -> String {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map_or_else(|| "loopback".to_owned(), |ConnectInfo(a)| a.to_string())
}

fn stamp_session(response: &mut Response, session: SessionId) {
    if let Ok(value) = HeaderValue::from_str(&session.0.to_string()) {
        response.headers_mut().insert(SESSION_HEADER, value);
    }
}

fn refuse(error: ApiError) -> Response {
    error.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &[u8] = b"0123456789abcdef0123456789abcdef";

    fn token() -> ApiToken {
        ApiToken::from_bytes(Path::new("/test/token"), GOOD).expect("a 32 byte token")
    }

    #[test]
    fn a_token_verifies_only_against_its_exact_bytes() {
        let t = token();
        assert!(t.verify(GOOD));
        assert!(!t.verify(b"0123456789abcdef0123456789abcdeg"));
        assert!(!t.verify(b"0123456789abcdef0123456789abcde"));
        assert!(!t.verify(b""));
        assert!(!t.verify(b"0123456789abcdef0123456789abcdef "));
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_because_credential_files_end_in_a_newline() {
        let t = ApiToken::from_bytes(Path::new("/t"), b"\n  0123456789abcdef0123456789abcdef \n")
            .expect("a token with a newline");
        assert!(t.verify(GOOD));
    }

    #[test]
    fn an_empty_or_short_token_is_refused_at_load() {
        assert!(matches!(
            ApiToken::from_bytes(Path::new("/t"), b"   \n"),
            Err(TokenError::Empty { .. })
        ));
        let err = ApiToken::from_bytes(Path::new("/t"), b"hunter2").expect_err("too short");
        assert!(matches!(err, TokenError::TooShort { .. }));
        // And the refusal does not quote the credential.
        assert!(!err.to_string().contains("hunter2"), "{err}");
    }

    /// `LOG-09` at the type. Neither rendering of the token shows it.
    #[test]
    fn a_token_cannot_be_printed() {
        let t = token();
        let rendered = format!("{t:?}");
        assert!(rendered.contains("[redacted]"), "{rendered}");
        assert!(!rendered.contains("0123456789"), "{rendered}");
    }

    #[tokio::test(start_paused = true)]
    async fn a_session_stays_live_until_the_ttl_and_then_is_replaced() {
        let s = Sessions::new(Duration::from_secs(900));
        let (first, freshness) = s.admit(None);
        assert_eq!(freshness, Freshness::Established);

        tokio::time::advance(Duration::from_secs(899)).await;
        let (again, freshness) = s.admit(Some(first.0));
        assert_eq!(again, first);
        assert_eq!(freshness, Freshness::Live);

        // The cap is absolute, not sliding: using it did not extend it.
        tokio::time::advance(Duration::from_secs(2)).await;
        let (renewed, freshness) = s.admit(Some(first.0));
        assert_ne!(renewed, first);
        assert_eq!(freshness, Freshness::Established);
    }

    #[tokio::test(start_paused = true)]
    async fn expired_sessions_are_dropped_rather_than_accumulating() {
        let s = Sessions::new(Duration::from_secs(60));
        for _ in 0..50 {
            let _ = s.admit(None);
        }
        assert_eq!(s.live(), 50);
        tokio::time::advance(Duration::from_secs(61)).await;
        let _ = s.admit(None);
        assert_eq!(s.live(), 1, "the expired 50 must be gone");
    }

    #[tokio::test(start_paused = true)]
    async fn the_session_table_is_bounded_however_hard_it_is_hammered() {
        let s = Sessions::new(Duration::from_secs(3_600));
        for _ in 0..(MAX_SESSIONS + 500) {
            let _ = s.admit(None);
        }
        assert_eq!(s.live(), MAX_SESSIONS);
    }

    /// `BOOT-07`'s second step must not be a small integer.
    ///
    /// A counter starting at 1 on every boot made the "already live session"
    /// gate satisfiable by guessing, and satisfiable by a *stored* request
    /// carrying `x-kdtv-session: 1` after a restart. Both are asserted against
    /// here: ids are not sequential, are not small, and a second table — which
    /// is what a restart produces — reissues none of the first one's.
    #[tokio::test(start_paused = true)]
    async fn a_session_id_is_not_a_number_a_stored_request_can_carry() {
        let s = Sessions::new(Duration::from_secs(900));
        let ids: Vec<u64> = (0..16).map(|_| s.admit(None).0.0).collect();

        for pair in ids.windows(2) {
            let (previous, next) = (pair[0], pair[1]);
            assert_ne!(next, previous.wrapping_add(1), "sequential ids: {ids:?}");
        }
        let guessable = u64::try_from(MAX_SESSIONS).expect("the cap fits in a u64");
        for id in &ids {
            assert!(*id > guessable, "an id inside guessing range: {ids:?}");
            assert_ne!(*id, 0, "zero is not an id: {ids:?}");
        }

        let after_restart = Sessions::new(Duration::from_secs(900));
        let fresh: Vec<u64> = (0..16).map(|_| after_restart.admit(None).0.0).collect();
        assert!(
            fresh.iter().all(|id| !ids.contains(id)),
            "a restart reissued an id: {ids:?} then {fresh:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn an_unknown_session_id_establishes_a_new_one_rather_than_being_honoured() {
        let s = Sessions::new(Duration::from_secs(60));
        let (id, freshness) = s.admit(Some(9_999_999));
        assert_ne!(id, SessionId(9_999_999));
        assert_eq!(freshness, Freshness::Established);
    }
}
