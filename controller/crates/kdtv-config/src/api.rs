//! The local API's binding and its credential.
//!
//! The controller has no authentication of its own. Anything that can reach this
//! API can run the shower, so the bind address is checked to be loopback here
//! rather than left to the operator, and the token is read from a file the
//! service manager supplies rather than from the configuration — a configuration
//! file gets copied, pasted, and pasted into an issue.
//!
//! This module validates that the credential file exists and is not
//! world-readable. It never reads the token. `kdtv-api` does that, once.

use crate::error::ConfigError;
use crate::fs::FsView;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The validated API configuration.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApiConfig {
    bind: SocketAddr,
    token_file: PathBuf,
    session_ttl: Duration,
}

impl ApiConfig {
    /// The longest session time-to-live this service accepts.
    ///
    /// `BOOT-07` requires a *fresh* authenticated session before a start is
    /// accepted. An hour is the outer edge of fresh; beyond it the word stops
    /// meaning anything.
    pub const TTL_CEILING: Duration = Duration::from_secs(3600);

    pub(crate) fn build(
        bind: &str,
        token_file: &str,
        session_ttl_s: u64,
        fs: &dyn FsView,
    ) -> Result<Self, ConfigError> {
        let addr: SocketAddr = bind.parse().map_err(|e: std::net::AddrParseError| {
            ConfigError::ApiBindUnparseable {
                bind: bind.to_owned(),
                reason: e.to_string(),
            }
        })?;
        if !addr.ip().is_loopback() {
            return Err(ConfigError::ApiBindNotLoopback {
                bind: bind.to_owned(),
            });
        }

        if session_ttl_s == 0 || session_ttl_s > Self::TTL_CEILING.as_secs() {
            return Err(ConfigError::ApiSessionTtl {
                value: session_ttl_s,
                max: Self::TTL_CEILING.as_secs(),
            });
        }

        let path = PathBuf::from(token_file);
        let Some(mode) = fs.mode(&path) else {
            return Err(ConfigError::TokenFileMissing {
                path: token_file.to_owned(),
            });
        };
        if mode & 0o004 != 0 {
            return Err(ConfigError::TokenFileWorldReadable {
                path: token_file.to_owned(),
                mode,
            });
        }

        Ok(Self {
            bind: addr,
            token_file: path,
            session_ttl: Duration::from_secs(session_ttl_s),
        })
    }

    #[must_use]
    pub const fn bind(&self) -> SocketAddr {
        self.bind
    }

    /// The path the token is read from. The token itself never enters this
    /// crate.
    #[must_use]
    pub fn token_file(&self) -> &Path {
        &self.token_file
    }

    #[must_use]
    pub const fn session_ttl(&self) -> Duration {
        self.session_ttl
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::{FsEntry, MapFs};

    fn fs_with_mode(mode: u32) -> MapFs {
        MapFs::new().with(
            "/run/credentials/kdtvd.service/api-token",
            FsEntry::own("/run/credentials/kdtvd.service/api-token").with_mode(mode),
        )
    }

    const TOKEN: &str = "/run/credentials/kdtvd.service/api-token";

    #[test]
    fn the_contract_values_build() {
        let a = ApiConfig::build("127.0.0.1:8443", TOKEN, 900, &fs_with_mode(0o400)).unwrap();
        assert_eq!(a.bind().port(), 8443);
        assert!(a.bind().ip().is_loopback());
        assert_eq!(a.session_ttl(), Duration::from_secs(900));
        assert_eq!(a.token_file(), Path::new(TOKEN));
    }

    #[test]
    fn a_non_loopback_bind_is_refused() {
        for bind in ["0.0.0.0:8443", "192.168.1.10:8443", "[::]:8443"] {
            let err = ApiConfig::build(bind, TOKEN, 900, &fs_with_mode(0o400)).unwrap_err();
            assert!(
                matches!(err, ConfigError::ApiBindNotLoopback { .. }),
                "{bind} accepted"
            );
            assert!(err.to_string().contains("loopback"));
        }
        // IPv6 loopback is still loopback.
        assert!(ApiConfig::build("[::1]:8443", TOKEN, 900, &fs_with_mode(0o400)).is_ok());
    }

    #[test]
    fn an_unparseable_bind_is_refused() {
        let err = ApiConfig::build("localhost:8443", TOKEN, 900, &fs_with_mode(0o400)).unwrap_err();
        assert!(
            matches!(err, ConfigError::ApiBindUnparseable { .. }),
            "{err}"
        );
    }

    #[test]
    fn a_missing_token_file_is_refused() {
        let err = ApiConfig::build("127.0.0.1:8443", TOKEN, 900, &MapFs::new()).unwrap_err();
        assert!(matches!(err, ConfigError::TokenFileMissing { .. }), "{err}");
        assert!(err.to_string().contains(TOKEN));
    }

    #[test]
    fn a_world_readable_token_file_is_refused() {
        for mode in [0o444, 0o644, 0o604, 0o777, 0o004] {
            let err =
                ApiConfig::build("127.0.0.1:8443", TOKEN, 900, &fs_with_mode(mode)).unwrap_err();
            assert!(
                matches!(err, ConfigError::TokenFileWorldReadable { .. }),
                "mode {mode:o} accepted"
            );
            let text = err.to_string();
            assert!(text.contains("world-readable"), "{text}");
        }
        for mode in [0o400, 0o600, 0o640, 0o660] {
            assert!(
                ApiConfig::build("127.0.0.1:8443", TOKEN, 900, &fs_with_mode(mode)).is_ok(),
                "mode {mode:o} refused"
            );
        }
    }

    #[test]
    fn a_session_ttl_outside_the_range_is_refused() {
        for ttl in [0u64, 3601, 86_400] {
            assert!(matches!(
                ApiConfig::build("127.0.0.1:8443", TOKEN, ttl, &fs_with_mode(0o400)),
                Err(ConfigError::ApiSessionTtl { .. })
            ));
        }
        assert!(ApiConfig::build("127.0.0.1:8443", TOKEN, 3600, &fs_with_mode(0o400)).is_ok());
    }
}
