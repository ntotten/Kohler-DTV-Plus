//! Which world this configuration describes.

use serde::Deserialize;
use std::fmt;

/// Production or bench, and nothing in between.
///
/// The distinction is not cosmetic: it decides whether a pseudo-terminal may be
/// bound to a zone and whether session-class durations may be scaled. Both are
/// indispensable for the end-to-end suite and both are indefensible on a machine
/// plumbed to a shower, so the profile gates them at parse time rather than at
/// use.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub enum Profile {
    /// Real valves on real buses. Refuses pseudo-terminals and any time scaling.
    Production,
    /// Emulator and bench. Permits pseudo-terminals and scaled session
    /// durations. Wire-class deadlines are not scalable in either profile — see
    /// [`crate::timing`].
    Bench,
}

impl Profile {
    #[must_use]
    pub const fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Production => "production",
            Self::Bench => "bench",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Debug)]
    struct Wrapper {
        profile: Profile,
    }

    #[test]
    fn the_two_spellings_the_contract_uses() {
        let p: Wrapper = toml::from_str(r#"profile = "production""#).unwrap();
        assert_eq!(p.profile, Profile::Production);
        let b: Wrapper = toml::from_str(r#"profile = "bench""#).unwrap();
        assert_eq!(b.profile, Profile::Bench);
        assert!(p.profile.is_production());
        assert!(!b.profile.is_production());
        assert_eq!(p.profile.to_string(), "production");
    }

    #[test]
    fn a_third_profile_does_not_exist() {
        let err = toml::from_str::<Wrapper>(r#"profile = "staging""#).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("production"), "{text}");
        assert!(text.contains("bench"), "{text}");
    }
}
