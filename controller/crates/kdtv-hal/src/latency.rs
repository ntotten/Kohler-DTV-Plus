//! The USB latency timer, written and **read back**.
//!
//! # Why it matters here
//!
//! The FTDI default is 16 ms. The bridge holds received bytes in its buffer for
//! up to that long before it sends a USB packet up, so every transaction picks
//! up as much as 16 ms of delay that has nothing to do with the bus, and every
//! arrival time is quantised to a 16 ms grid.
//!
//! The Saturn response deadline this service works against is 320 ms message /
//! 400 ms response inside a 525 ms tick (`CORRECTIONS.md` item 5 keeps both
//! plumbed and declares neither correct). A 16 ms quantisation cannot resolve
//! jitter against a deadline of that size: a response that arrives at 310 ms and
//! one that arrives at 322 ms can land in the same bucket, so the measurement
//! that would say whether the deadline is right is not available. Setting the
//! timer to 1 ms is what makes the timing evidence worth recording at all.
//!
//! # Why it is read back
//!
//! A write to a sysfs attribute reports whether the *write* was accepted, not
//! whether the value took. A bridge whose EEPROM ignores it accepts the write
//! and stays at 16, and nothing in the boot log would say so. So the value is
//! read back and a mismatch **refuses to start**: this service's timing claims
//! are only worth making on a port whose latency is known, and the alternative
//! is a commissioning report whose numbers quietly include a 16 ms bridge delay.
//!
//! # `latency_timer` is FTDI's
//!
//! It is an `ftdi_sio` driver attribute, not a USB or a tty one. A CP2102, a
//! CH340 or a PL2303 has no such file, and each has its own arrangement — or
//! none — for shortening the same buffering. This module therefore refuses a
//! non-FTDI bridge outright rather than opening it unhardened: the equivalent
//! has to be **established** for that bridge family first, on the bench, and
//! written down, before a port of that family drives water.
//!
//! # `ASYNC_LOW_LATENCY`
//!
//! The kernel's other route to the same setting is the `ASYNC_LOW_LATENCY` flag
//! in `struct serial_struct`, set with the `TIOCSSERIAL` ioctl on an open tty.
//! **It is not set here, and the reason is recorded rather than hidden:**
//! `TIOCSSERIAL` needs a raw `ioctl`, `unsafe_code = "deny"` applies to this
//! whole workspace, and `nix` 0.31 exposes no safe wrapper for it. Writing
//! `latency_timer` is the route that is reachable without `unsafe`.
//!
//! On `ftdi_sio` the two are the same setting: the driver's `ASYNC_LOW_LATENCY`
//! handling sets the latency timer to 1 ms. `[I]` — that is inference from the
//! driver's documented behaviour, not something measured on this installation,
//! and it is the reason the read-back is the check that actually runs. If
//! commissioning finds a bridge where the two differ, this is the note to come
//! back to.

use std::fmt;

use kdtv_units::LinkKind;

use crate::factory::OpenError;
use crate::resolve::{BridgeKind, PortBinding};
use crate::sysfs::SysfsView;

/// What the latency timer is set to. One millisecond is the minimum the driver
/// accepts.
pub const REQUIRED_LATENCY_MS: u8 = 1;

/// What FTDI ships. Recorded so a boot log says what was changed, not just what
/// it ended at.
pub const FTDI_DEFAULT_LATENCY_MS: u8 = 16;

/// The hardening state of one open port.
///
/// There is deliberately no variant meaning "a real bridge, hardening skipped".
/// A serial port either carries the latency value that was read back off it, or
/// it was never opened.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Hardened {
    /// An FTDI bridge whose `latency_timer` was written and read back.
    FtdiLatencyTimer {
        node: String,
        /// What the attribute read back as, not what was written to it.
        latency_ms: u8,
        /// What it held before. `None` when it could not be read first.
        was_ms: Option<u8>,
    },
    /// A pseudo-terminal. No USB bridge, no buffering timer, nothing to set.
    Pty { node: String },
}

impl Hardened {
    /// The measured latency, or `None` for a pseudo-terminal.
    #[must_use]
    pub const fn latency_ms(&self) -> Option<u8> {
        match self {
            Self::FtdiLatencyTimer { latency_ms, .. } => Some(*latency_ms),
            Self::Pty { .. } => None,
        }
    }
}

impl fmt::Display for Hardened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FtdiLatencyTimer {
                latency_ms,
                was_ms: Some(was),
                ..
            } => write!(f, "latency_timer {was} -> {latency_ms} ms"),
            Self::FtdiLatencyTimer { latency_ms, .. } => write!(f, "latency_timer {latency_ms} ms"),
            Self::Pty { .. } => f.write_str("pty, no latency timer"),
        }
    }
}

/// Writes the latency timer, reads it back, and refuses if it did not stick.
///
/// Called **before** the port is opened. A bridge that cannot be hardened is
/// one this service never held an fd to.
pub fn harden(binding: &PortBinding, sysfs: &dyn SysfsView) -> Result<Hardened, OpenError> {
    let link = binding.link();
    let port = binding.port();
    let node = port.node().to_owned();

    match port.bridge() {
        BridgeKind::Pty => Ok(Hardened::Pty { node }),

        BridgeKind::Other { driver } => Err(OpenError::NonFtdiBridge {
            link,
            part: driver.clone(),
        }),

        BridgeKind::Ftdi { .. } => {
            let was_ms = sysfs.read_latency_timer(&node).ok();

            sysfs
                .write_latency_timer(&node, REQUIRED_LATENCY_MS)
                .map_err(|source| OpenError::LatencyWrite {
                    link,
                    node: node.clone(),
                    source,
                })?;

            let read_back =
                sysfs
                    .read_latency_timer(&node)
                    .map_err(|source| OpenError::LatencyWrite {
                        link,
                        node: node.clone(),
                        source,
                    })?;

            if read_back != REQUIRED_LATENCY_MS {
                return Err(OpenError::NotLowLatency {
                    link,
                    node,
                    wrote: REQUIRED_LATENCY_MS,
                    read_back,
                });
            }

            Ok(Hardened::FtdiLatencyTimer {
                node,
                latency_ms: read_back,
                was_ms,
            })
        }
    }
}

/// Hardens every binding, or refuses.
///
/// All-or-nothing for the same reason resolution is: a run with one bridge at
/// 1 ms and another at 16 ms produces timing evidence that reads as one system
/// and is not.
pub fn harden_all(
    bindings: &[PortBinding],
    sysfs: &dyn SysfsView,
) -> Result<Vec<(LinkKind, Hardened)>, OpenError> {
    bindings
        .iter()
        .map(|b| harden(b, sysfs).map(|h| (b.link(), h)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::resolve_distinct;
    use crate::sysfs::DirSysfs;
    use kdtv_config::{PortPath, Profile};
    use kdtv_units::ZoneId;

    const Z1: LinkKind = LinkKind::Zone(ZoneId::Zone1);
    const Z2: LinkKind = LinkKind::Zone(ZoneId::Zone2);

    fn by_id(name: &str) -> PortPath {
        PortPath::parse("zones.zone1.port", name, Profile::Production).unwrap()
    }

    fn bind(case: &str, names: &[(LinkKind, &str)]) -> (DirSysfs, Vec<PortBinding>) {
        let fs = DirSysfs::fixture(case);
        let bindings: Vec<_> = names.iter().map(|(l, n)| (*l, by_id(n))).collect();
        let bound = resolve_distinct(&bindings, &fs).unwrap();
        (fs, bound)
    }

    const W: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485";

    #[test]
    fn the_default_is_moved_from_sixteen_to_one_and_read_back() {
        let (fs, bound) = bind(
            "reference",
            &[
                (Z1, &format!("{W}-if00-port0")),
                (Z2, &format!("{W}-if01-port0")),
            ],
        );
        let hardened = harden_all(&bound, &fs).unwrap();
        assert_eq!(hardened.len(), 2);
        for (link, h) in &hardened {
            assert_eq!(h.latency_ms(), Some(REQUIRED_LATENCY_MS), "{link}");
            assert!(matches!(
                h,
                Hardened::FtdiLatencyTimer {
                    was_ms: Some(FTDI_DEFAULT_LATENCY_MS),
                    ..
                }
            ));
        }
        assert_eq!(hardened[0].1.to_string(), "latency_timer 16 -> 1 ms");
    }

    /// The refusal the read-back exists for.
    #[test]
    fn a_latency_timer_that_does_not_stick_refuses_the_start() {
        let (fs, bound) = bind("latency-stuck", &[(Z1, &format!("{W}-if00-port0"))]);
        let err = harden(&bound[0], &fs).unwrap_err();
        match err {
            OpenError::NotLowLatency {
                link,
                wrote,
                read_back,
                ..
            } => {
                assert_eq!(link, Z1);
                assert_eq!((wrote, read_back), (1, 16));
            }
            other => panic!("{other:?}"),
        }
        // And it refuses the whole set, not just that link.
        let (fs, bound) = bind(
            "latency-stuck",
            &[
                (Z1, &format!("{W}-if00-port0")),
                (Z2, &format!("{W}-if01-port0")),
            ],
        );
        assert!(harden_all(&bound, &fs).is_err());
    }

    /// `latency_timer` is FTDI's. A different bridge needs its own equivalent
    /// established first — it is not opened unhardened in the meantime.
    #[test]
    fn a_non_ftdi_bridge_is_refused_rather_than_opened_unhardened() {
        let (fs, bound) = bind(
            "non-ftdi",
            &[(
                Z1,
                "/dev/serial/by-id/usb-Silicon_Labs_CP2102_0001-if00-port0",
            )],
        );
        let err = harden(&bound[0], &fs).unwrap_err();
        match err {
            OpenError::NonFtdiBridge { link, ref part } => {
                assert_eq!(link, Z1);
                assert_eq!(part, "cp210x");
            }
            other => panic!("{other:?}"),
        }
        assert!(err.to_string().contains("latency"), "{err}");
    }

    #[test]
    fn a_pty_has_no_latency_timer_and_that_is_a_state_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("dev/pts")).unwrap();
        std::fs::create_dir_all(root.join("devices")).unwrap();
        std::fs::write(root.join("dev/pts/3"), "/dev/pts/3\n").unwrap();
        let fs = DirSysfs::new(root);
        let bindings = vec![(
            Z1,
            PortPath::parse("zones.zone1.port", "/dev/pts/3", Profile::Bench).unwrap(),
        )];
        let bound = resolve_distinct(&bindings, &fs).unwrap();
        let h = harden(&bound[0], &fs).unwrap();
        assert_eq!(
            h,
            Hardened::Pty {
                node: "3".to_owned()
            }
        );
        assert_eq!(h.latency_ms(), None);
    }
}
