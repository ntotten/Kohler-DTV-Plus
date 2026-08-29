//! Serial port paths, and the shapes a port path is allowed to have.
//!
//! `SER-01` / `USB-02`: a zone is bound to a converter by a stable name, never
//! by `/dev/ttyUSB*` enumeration order. The failure that rule exists to prevent
//! is silent — after a reboot the kernel enumerates the two converters the other
//! way round, zone 1 becomes zone 2, and the service opens the wrong valve while
//! reporting success.
//!
//! Denial here is by absence of a variant: [`PortPath`] has no case that can
//! hold `/dev/ttyUSB0`, so a bound port carrying an enumeration-order name is
//! not a value this crate can produce.

use crate::error::ConfigError;
use crate::profile::Profile;
use std::fmt;
use std::path::{Path, PathBuf};

/// Stable-name prefix: the udev `by-id` tree.
pub const BY_ID: &str = "/dev/serial/by-id/";
/// Stable-name prefix: the udev `by-path` tree.
pub const BY_PATH: &str = "/dev/serial/by-path/";
/// Pseudo-terminal prefix. Bench profile only.
pub const PTY: &str = "/dev/pts/";
/// The prefix that is refused outright.
pub const UNSTABLE: &str = "/dev/tty";

/// The literal the emulated rig ships in place of a pseudo-terminal path.
///
/// A PTY path does not exist until the rig creates the pair, so
/// `deploy/kdtvd.emulated.toml` cannot name one. It names this instead, and the
/// rig substitutes real paths with [`crate::ValidatedConfig::bind_ptys`] before
/// anything is opened.
pub const PTY_PLACEHOLDER: &str = "/dev/pts/PLACEHOLDER";

/// A validated serial port path.
///
/// There is no `DevTty` variant. There is also no `From<String>` and no
/// `Deserialize`: [`PortPath::parse`] is the only way in, and it takes the
/// profile, so a pseudo-terminal cannot be bound under
/// [`Profile::Production`].
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum PortPath {
    /// A `/dev/serial/by-id/…` name — the converter's own identity.
    ById(PathBuf),
    /// A `/dev/serial/by-path/…` name — the physical USB port the converter is
    /// plugged into. Preferred over `by-id` when a converter ships a blank or
    /// duplicated USB serial number, which `HARDWARE-SPEC.md` § 5 records as an
    /// expected failure mode.
    ByPath(PathBuf),
    /// A pseudo-terminal. Bench profile only.
    Pty(PathBuf),
    /// The rig's placeholder, awaiting substitution. Bench profile only.
    ///
    /// It resolves to no device, so it never participates in the
    /// distinct-device check and it cannot be opened. The link factory has
    /// nothing to open until the rig has replaced it.
    PtyPlaceholder,
}

impl PortPath {
    /// The only constructor.
    ///
    /// `field` is the dotted key this path came from, so the refusal names the
    /// line in the file rather than the value alone.
    pub fn parse(field: &str, raw: &str, profile: Profile) -> Result<Self, ConfigError> {
        if raw == PTY_PLACEHOLDER {
            return Self::pty_guard(field, Self::PtyPlaceholder, raw, profile);
        }
        if let Some(rest) = raw.strip_prefix(BY_ID) {
            return Self::named(field, raw, rest).map(Self::ById);
        }
        if let Some(rest) = raw.strip_prefix(BY_PATH) {
            return Self::named(field, raw, rest).map(Self::ByPath);
        }
        if let Some(rest) = raw.strip_prefix(PTY) {
            let path = Self::named(field, raw, rest)?;
            return Self::pty_guard(field, Self::Pty(path), raw, profile);
        }
        if raw.starts_with(UNSTABLE) {
            return Err(ConfigError::UnstablePortPath {
                field: field.to_owned(),
                path: raw.to_owned(),
            });
        }
        Err(ConfigError::UnknownPortScheme {
            field: field.to_owned(),
            path: raw.to_owned(),
        })
    }

    fn named(field: &str, raw: &str, rest: &str) -> Result<PathBuf, ConfigError> {
        if rest.is_empty() || rest.contains('/') {
            return Err(ConfigError::UnknownPortScheme {
                field: field.to_owned(),
                path: raw.to_owned(),
            });
        }
        Ok(PathBuf::from(raw))
    }

    fn pty_guard(
        field: &str,
        candidate: Self,
        raw: &str,
        profile: Profile,
    ) -> Result<Self, ConfigError> {
        match profile {
            Profile::Bench => Ok(candidate),
            Profile::Production => Err(ConfigError::PtyUnderProduction {
                field: field.to_owned(),
                path: raw.to_owned(),
            }),
        }
    }

    /// The path as configured, or `None` for the placeholder — which names no
    /// path at all.
    #[must_use]
    pub fn as_path(&self) -> Option<&Path> {
        match self {
            Self::ById(p) | Self::ByPath(p) | Self::Pty(p) => Some(p),
            Self::PtyPlaceholder => None,
        }
    }

    /// True for a pseudo-terminal, placeholder or not.
    #[must_use]
    pub const fn is_pty(&self) -> bool {
        matches!(self, Self::Pty(_) | Self::PtyPlaceholder)
    }

    /// True when the rig still has to substitute a real path.
    #[must_use]
    pub const fn is_placeholder(&self) -> bool {
        matches!(self, Self::PtyPlaceholder)
    }
}

impl fmt::Display for PortPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.as_path() {
            Some(p) => write!(f, "{}", p.display()),
            None => f.write_str(PTY_PLACEHOLDER),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_names_parse_under_both_profiles() {
        for profile in [Profile::Production, Profile::Bench] {
            let by_id = PortPath::parse(
                "zones.zone1.port",
                "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if00-port0",
                profile,
            )
            .unwrap();
            assert!(matches!(by_id, PortPath::ById(_)));
            assert!(!by_id.is_pty());

            let by_path = PortPath::parse(
                "zones.zone2.port",
                "/dev/serial/by-path/pci-0000-usb-0",
                profile,
            )
            .unwrap();
            assert!(matches!(by_path, PortPath::ByPath(_)));
        }
    }

    /// The refusal the whole module exists for.
    #[test]
    fn an_enumeration_order_name_is_refused_in_every_profile() {
        for profile in [Profile::Production, Profile::Bench] {
            for raw in ["/dev/ttyUSB0", "/dev/ttyAMA0", "/dev/tty", "/dev/ttyS3"] {
                let err = PortPath::parse("zones.zone1.port", raw, profile).unwrap_err();
                assert!(
                    matches!(err, ConfigError::UnstablePortPath { .. }),
                    "{raw} under {profile:?} gave {err:?}"
                );
                // The message names the field and the offending path.
                let text = err.to_string();
                assert!(text.contains("zones.zone1.port"), "{text}");
                assert!(text.contains(raw), "{text}");
            }
        }
    }

    #[test]
    fn a_pty_is_bench_only() {
        assert!(matches!(
            PortPath::parse("zones.zone1.port", "/dev/pts/7", Profile::Bench),
            Ok(PortPath::Pty(_))
        ));
        assert!(matches!(
            PortPath::parse("zones.zone1.port", PTY_PLACEHOLDER, Profile::Bench),
            Ok(PortPath::PtyPlaceholder)
        ));
        for raw in ["/dev/pts/7", PTY_PLACEHOLDER] {
            assert!(matches!(
                PortPath::parse("zones.zone1.port", raw, Profile::Production),
                Err(ConfigError::PtyUnderProduction { .. })
            ));
        }
    }

    #[test]
    fn anything_else_is_refused_by_scheme() {
        for raw in [
            "",
            "ttyUSB0",
            "/dev/serial/by-id/",
            "/dev/serial/by-id/a/b",
            "/home/user/port",
            "COM3",
        ] {
            assert!(
                matches!(
                    PortPath::parse("zones.zone1.port", raw, Profile::Bench),
                    Err(ConfigError::UnknownPortScheme { .. })
                ),
                "{raw} was accepted"
            );
        }
    }

    #[test]
    fn the_placeholder_names_no_path() {
        let p = PortPath::parse("zones.zone1.port", PTY_PLACEHOLDER, Profile::Bench).unwrap();
        assert!(p.is_placeholder());
        assert!(p.is_pty());
        assert_eq!(p.as_path(), None);
        assert_eq!(p.to_string(), PTY_PLACEHOLDER);
    }
}
