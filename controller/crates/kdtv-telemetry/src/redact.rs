//! A wrapper that cannot be printed.
//!
//! `DESIGN.md`: "No credential, access token, or pairing data belongs
//! in these logs." A rule like that survives exactly as long as the person who
//! remembers it. Wrapping the value makes it structural: `Redacted<T>` prints
//! `[redacted]` in `Debug` and serialises as `"[redacted]"`, so neither a
//! structured log line nor a `dbg!` can leak one.

use serde::{Serialize, Serializer};
use std::fmt;

/// A value that must never reach a log.
///
/// The inner value is reachable only through [`Redacted::expose`], which is
/// deliberately verbose at the call site, and through the constant-time
/// comparison in [`Redacted::ct_eq_bytes`].
#[derive(Clone)]
pub struct Redacted<T>(T);

impl<T> Redacted<T> {
    pub const fn new(v: T) -> Self {
        Self(v)
    }

    /// Read the secret. Named so that a review notices it.
    pub const fn expose(&self) -> &T {
        &self.0
    }

    pub fn into_inner(self) -> T {
        self.0
    }
}

impl Redacted<String> {
    /// Compare against a candidate without leaking timing.
    ///
    /// Length is compared first and does leak, which is unavoidable and
    /// acceptable: the token's length is not the secret.
    #[must_use]
    pub fn ct_eq_bytes(&self, candidate: &[u8]) -> bool {
        let expected = self.0.as_bytes();
        if expected.len() != candidate.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (a, b) in expected.iter().zip(candidate.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T> Serialize for Redacted<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("[redacted]")
    }
}

/// Deserialises into a `Redacted<T>` so a config file can carry a token without
/// the token becoming printable on the way through.
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for Redacted<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        T::deserialize(d).map(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_and_display_and_serialize_all_hide_the_value() {
        let secret = Redacted::new("hunter2-the-real-token".to_owned());
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert_eq!(format!("{secret}"), "[redacted]");
        let json = serde_json::to_string(&secret).unwrap();
        assert_eq!(json, "\"[redacted]\"");
        assert!(!json.contains("hunter2"));
    }

    #[test]
    fn nesting_in_a_struct_still_hides_it() {
        #[derive(Debug, serde::Serialize)]
        struct Cfg {
            name: String,
            token: Redacted<String>,
        }
        let c = Cfg {
            name: "local".into(),
            token: Redacted::new("s3cret".into()),
        };
        assert!(!format!("{c:?}").contains("s3cret"));
        assert!(!serde_json::to_string(&c).unwrap().contains("s3cret"));
    }

    #[test]
    fn comparison_matches_only_the_exact_bytes() {
        let t = Redacted::new("abcdef".to_owned());
        assert!(t.ct_eq_bytes(b"abcdef"));
        assert!(!t.ct_eq_bytes(b"abcdeg"));
        assert!(!t.ct_eq_bytes(b"abcde"));
        assert!(!t.ct_eq_bytes(b""));
    }

    #[test]
    fn the_value_is_still_reachable_when_it_has_to_be() {
        let t = Redacted::new(41_u32 + 1);
        assert_eq!(*t.expose(), 42);
    }
}
