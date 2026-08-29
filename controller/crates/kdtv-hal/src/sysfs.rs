//! The four filesystem questions port resolution asks, behind a trait.
//!
//! Every filesystem touch in the resolver goes through [`SysfsView`], for the
//! same reason time is a parameter everywhere else in this workspace: a resolver
//! that reads the real `/sys` can only be tested on a machine with the real
//! converters attached, and the refusals that matter are the ones nobody has the
//! hardware to reproduce on demand.
//!
//! There are exactly four questions:
//!
//! 1. what device node does this configured name point at,
//! 2. what USB serial devices exist and what are they,
//! 3. what is this bridge's latency timer,
//! 4. set it.
//!
//! # The fixture tree layout
//!
//! [`DirSysfs`] reads a directory tree rooted anywhere, so
//! `fixtures/sysfs/<case>/` under this crate stands in for a machine. The layout
//! is **flattened** relative to real sysfs:
//!
//! ```text
//! <root>/dev/serial/by-id/<name>          one line: the device node path
//! <root>/dev/serial/by-path/<name>        the same
//! <root>/devices/<node>/driver            "ftdi_sio"
//! <root>/devices/<node>/latency_timer     "16"
//! <root>/devices/<node>/idVendor          "0403"
//! <root>/devices/<node>/idProduct         "6011"
//! <root>/devices/<node>/serial            absent when the bridge reports none
//! <root>/devices/<node>/usb_device        "1-1.4"  — the physical USB device
//! <root>/devices/<node>/devpath           "1-1.4:1.0" — the interface
//! ```
//!
//! A `by-id` entry is a **file containing the target path**, not a symlink, so a
//! checkout does not depend on symlink support and a `git diff` shows what
//! changed. [`RealSysfs`] uses `std::fs::canonicalize` for the same question.
//!
//! **What these fixtures do not prove.** The flattened layout is not real
//! sysfs. [`RealSysfs`] walks `/sys/bus/usb-serial/devices/<node>` up two levels
//! to reach the USB device directory, and that walk is exercised only on the Pi.
//! What the fixtures prove is the resolver's decisions — present, distinct,
//! unambiguous, hardened — given an answer, not that the answer is read
//! correctly off a real kernel.
//!
//! A fixture may also carry `latency_timer.writes_stick` containing `false`.
//! Real sysfs has no such attribute; it is how the tree spells a bridge that
//! accepts the write and keeps its old value. That is the failure
//! [`crate::harden`] exists to catch, and it has to be reproducible without a
//! clone converter in hand.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// The kernel's name for the FTDI latency attribute. FTDI-specific.
const LATENCY_TIMER: &str = "latency_timer";
/// Fixture-only sibling of [`LATENCY_TIMER`]; see the module docs.
const WRITES_STICK: &str = "latency_timer.writes_stick";

/// One USB serial device, as the kernel describes it.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct TtyCandidate {
    /// The kernel node name, e.g. `ttyUSB0`.
    pub node: String,
    /// The device node, e.g. `/dev/ttyUSB0`.
    pub device: PathBuf,
    /// The bound driver, e.g. `ftdi_sio`. The latency timer is FTDI-specific,
    /// so this decides whether the port can be hardened at all.
    pub driver: String,
    /// USB vendor id, four lowercase hex digits.
    pub vendor: String,
    /// USB product id, four lowercase hex digits.
    pub product: String,
    /// The USB serial number. `None` when the bridge's EEPROM carries none,
    /// which is common on the Waveshare boards and is why the `by-id` names in
    /// `deploy/kdtvd.toml` have no serial in them.
    pub serial: Option<String>,
    /// The physical USB device this interface belongs to, e.g. `1-1.4`. Two
    /// interfaces of one converter share it; two converters do not.
    pub usb_device: String,
    /// The USB interface path, e.g. `1-1.4:1.0`.
    pub devpath: String,
}

impl TtyCandidate {
    /// The identity a `by-id` name is built from. Two devices with the same
    /// triple produce colliding `by-id` names.
    #[must_use]
    pub fn identity(&self) -> (&str, &str, Option<&str>) {
        (&self.vendor, &self.product, self.serial.as_deref())
    }
}

/// What the port resolver may ask of the filesystem. Four questions, no more.
pub trait SysfsView: Send + Sync + fmt::Debug {
    /// The device node a configured path names, with symlinks resolved.
    ///
    /// This is the identity two links are compared on. Comparing configured
    /// strings would accept two `by-id` names that both point at `ttyUSB0`,
    /// which is the exact confusion the `by-id` rule exists to prevent.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf>;

    /// Every USB serial device the kernel currently presents.
    fn enumerate(&self) -> io::Result<Vec<TtyCandidate>>;

    /// The bridge's latency timer, in milliseconds.
    fn read_latency_timer(&self, node: &str) -> io::Result<u8>;

    /// Writes the latency timer.
    ///
    /// Success here means the write was accepted, **not** that it took effect.
    /// [`crate::harden`] reads it back, because a bridge that silently keeps 16
    /// is the failure worth catching.
    fn write_latency_timer(&self, node: &str, ms: u8) -> io::Result<()>;
}

/// The kernel's own `/sys` and `/dev`.
///
/// Exercised on the Pi only; CI runs the fixture trees. The paths it reads are:
///
/// | Question | Path |
/// | --- | --- |
/// | node for a name | `canonicalize(<configured path>)` |
/// | enumeration | `/sys/bus/usb-serial/devices/` |
/// | driver | `readlink <node>/driver` |
/// | latency timer | `<node>/latency_timer` |
/// | vendor, product, serial | the USB device directory, two levels above `<node>` |
#[derive(Clone, Debug)]
pub struct RealSysfs {
    usb_serial_devices: PathBuf,
    dev: PathBuf,
}

impl Default for RealSysfs {
    fn default() -> Self {
        Self::new()
    }
}

impl RealSysfs {
    #[must_use]
    pub fn new() -> Self {
        Self {
            usb_serial_devices: PathBuf::from("/sys/bus/usb-serial/devices"),
            dev: PathBuf::from("/dev"),
        }
    }

    fn candidate(&self, node: &str) -> io::Result<TtyCandidate> {
        let entry = self.usb_serial_devices.join(node);
        // `<node>` is a symlink into /sys/devices/…/<usb device>/<interface>/<node>.
        let resolved = std::fs::canonicalize(&entry)?;
        let interface = parent_of(&resolved)?;
        let usb_device = parent_of(interface)?;

        let driver = std::fs::read_link(entry.join("driver"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "unknown".to_owned());

        Ok(TtyCandidate {
            node: node.to_owned(),
            device: self.dev.join(node),
            driver,
            vendor: attr(usb_device, "idVendor").unwrap_or_default(),
            product: attr(usb_device, "idProduct").unwrap_or_default(),
            serial: attr(usb_device, "serial").filter(|s| !s.is_empty()),
            usb_device: file_name(usb_device),
            devpath: file_name(interface),
        })
    }
}

fn parent_of(path: &Path) -> io::Result<&Path> {
    path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} has no parent directory", path.display()),
        )
    })
}

fn attr(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name))
        .ok()
        .map(|s| s.trim().to_owned())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl SysfsView for RealSysfs {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        std::fs::canonicalize(path)
    }

    fn enumerate(&self) -> io::Result<Vec<TtyCandidate>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&self.usb_serial_devices)? {
            let node = entry?.file_name().to_string_lossy().into_owned();
            // A device that vanishes between the listing and the read is not an
            // error for the enumeration; it is simply not present, and the
            // resolver's present-and-distinct check is what refuses.
            if let Ok(c) = self.candidate(&node) {
                out.push(c);
            }
        }
        out.sort();
        Ok(out)
    }

    fn read_latency_timer(&self, node: &str) -> io::Result<u8> {
        let text = std::fs::read_to_string(self.usb_serial_devices.join(node).join(LATENCY_TIMER))?;
        parse_latency(text.trim())
    }

    fn write_latency_timer(&self, node: &str, ms: u8) -> io::Result<()> {
        std::fs::write(
            self.usb_serial_devices.join(node).join(LATENCY_TIMER),
            ms.to_string(),
        )
    }
}

fn parse_latency(text: &str) -> io::Result<u8> {
    text.parse::<u8>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("latency_timer read back as {text:?}, which is not a millisecond count"),
        )
    })
}

/// A [`SysfsView`] over a directory tree, in the flattened layout the module
/// docs describe.
///
/// It answers questions. It holds no port, opens no device and transmits
/// nothing — pointing it at a crafted tree can make the resolver believe a
/// converter is present, and the open that follows then fails on the real
/// filesystem with the transmit gate still in front of it.
///
/// Writes land in an in-memory overlay, so a fixture tree committed to the
/// repository is never modified by a test run.
#[derive(Debug)]
pub struct DirSysfs {
    root: PathBuf,
    written: Mutex<BTreeMap<String, u8>>,
}

impl DirSysfs {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            written: Mutex::new(BTreeMap::new()),
        }
    }

    /// One of this crate's committed fixture trees, by directory name.
    #[must_use]
    pub fn fixture(case: &str) -> Self {
        Self::new(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures")
                .join("sysfs")
                .join(case),
        )
    }

    fn device_dir(&self, node: &str) -> PathBuf {
        self.root.join("devices").join(node)
    }

    fn attr(&self, node: &str, name: &str) -> Option<String> {
        std::fs::read_to_string(self.device_dir(node).join(name))
            .ok()
            .map(|s| s.trim().to_owned())
    }

    fn writes_stick(&self, node: &str) -> bool {
        self.attr(node, WRITES_STICK)
            .is_none_or(|v| !v.eq_ignore_ascii_case("false"))
    }

    fn candidate(&self, node: &str) -> io::Result<TtyCandidate> {
        let missing = |what: &str| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("fixture device {node} has no {what}"),
            )
        };
        Ok(TtyCandidate {
            node: node.to_owned(),
            device: self.root.join("dev").join(node),
            driver: self.attr(node, "driver").ok_or_else(|| missing("driver"))?,
            vendor: self
                .attr(node, "idVendor")
                .ok_or_else(|| missing("idVendor"))?,
            product: self
                .attr(node, "idProduct")
                .ok_or_else(|| missing("idProduct"))?,
            serial: self.attr(node, "serial").filter(|s| !s.is_empty()),
            usb_device: self
                .attr(node, "usb_device")
                .ok_or_else(|| missing("usb_device"))?,
            devpath: self.attr(node, "devpath").unwrap_or_default(),
        })
    }
}

impl SysfsView for DirSysfs {
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        // A configured path is absolute; strip the leading separator so it
        // reads relative to the fixture root.
        let rel = path.strip_prefix("/").unwrap_or(path);
        let target = std::fs::read_to_string(self.root.join(rel))?;
        let target = target.trim();
        if target.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} names no device node", path.display()),
            ));
        }
        Ok(PathBuf::from(target))
    }

    fn enumerate(&self) -> io::Result<Vec<TtyCandidate>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(self.root.join("devices"))? {
            let node = entry?.file_name().to_string_lossy().into_owned();
            out.push(self.candidate(&node)?);
        }
        out.sort();
        Ok(out)
    }

    fn read_latency_timer(&self, node: &str) -> io::Result<u8> {
        if let Some(v) = self
            .written
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(node)
        {
            return Ok(*v);
        }
        let text = self.attr(node, LATENCY_TIMER).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("fixture device {node} has no {LATENCY_TIMER}"),
            )
        })?;
        parse_latency(&text)
    }

    fn write_latency_timer(&self, node: &str, ms: u8) -> io::Result<()> {
        if self.attr(node, LATENCY_TIMER).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("fixture device {node} has no {LATENCY_TIMER}"),
            ));
        }
        if self.writes_stick(node) {
            self.written
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(node.to_owned(), ms);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_reference_tree_enumerates_three_interfaces_of_one_converter() {
        let fs = DirSysfs::fixture("reference");
        let found = fs.enumerate().unwrap();
        assert_eq!(found.len(), 3);
        assert!(found.iter().all(|c| c.driver == "ftdi_sio"));
        // One physical converter: three interfaces, one usb_device.
        assert_eq!(found[0].usb_device, found[2].usb_device);
        // The Waveshare boards this configuration names report no serial.
        assert!(found.iter().all(|c| c.serial.is_none()));
    }

    #[test]
    fn a_by_id_name_resolves_to_the_node_the_file_names() {
        let fs = DirSysfs::fixture("reference");
        let node = fs
            .canonicalize(Path::new(
                "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if00-port0",
            ))
            .unwrap();
        assert_eq!(node, PathBuf::from("/dev/ttyUSB0"));
        assert!(
            fs.canonicalize(Path::new("/dev/serial/by-id/nothing-here"))
                .is_err()
        );
    }

    #[test]
    fn a_write_lands_in_the_overlay_not_in_the_committed_tree() {
        let fs = DirSysfs::fixture("reference");
        let on_disk = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/sysfs/reference/devices/ttyUSB0/latency_timer"),
        )
        .unwrap();
        assert_eq!(on_disk.trim(), "16");
        assert_eq!(fs.read_latency_timer("ttyUSB0").unwrap(), 16);
        fs.write_latency_timer("ttyUSB0", 1).unwrap();
        assert_eq!(fs.read_latency_timer("ttyUSB0").unwrap(), 1);
        // The file itself is untouched.
        let after = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fixtures/sysfs/reference/devices/ttyUSB0/latency_timer"),
        )
        .unwrap();
        assert_eq!(after.trim(), "16");
    }

    #[test]
    fn a_tree_can_spell_a_bridge_whose_write_does_not_take() {
        let fs = DirSysfs::fixture("latency-stuck");
        fs.write_latency_timer("ttyUSB0", 1).unwrap();
        assert_eq!(fs.read_latency_timer("ttyUSB0").unwrap(), 16);
    }

    #[test]
    fn an_unparseable_latency_value_is_an_error_not_a_zero() {
        assert!(parse_latency("").is_err());
        assert!(parse_latency("fast").is_err());
        assert_eq!(parse_latency("1").unwrap(), 1);
    }

    #[test]
    fn identity_is_the_triple_a_by_id_name_is_built_from() {
        let fs = DirSysfs::fixture("shared-serial");
        let found = fs.enumerate().unwrap();
        assert_eq!(found.len(), 2);
        assert_ne!(found[0].usb_device, found[1].usb_device);
        assert_eq!(found[0].identity(), found[1].identity());
    }
}
