//! Binding links to devices: present, distinct and unambiguous, or refuse.
//!
//! # The failure this exists to prevent
//!
//! It is silent. After a reboot the kernel enumerates the two converters the
//! other way round, zone 1 becomes zone 2, and the service opens the wrong valve
//! while reporting success. Nobody sees a fault; somebody sees the wrong outlet
//! run at the wrong temperature.
//!
//! `kdtv-config` closes half of it: [`PortPath`] has no variant that can hold
//! `/dev/ttyUSB0`, so an enumeration-order name never reaches this crate. This
//! module closes the other half, which needs the machine:
//!
//! 1. **Present.** Every configured path resolves to a device node, and that
//!    node is one the kernel describes. There is no degraded start — a service
//!    that cannot be sure which physical valve is on which bus must not drive
//!    either of them.
//! 2. **Distinct.** No two links resolve to the same node. Two `by-id` names can
//!    both point at `ttyUSB0`; comparing the configured strings would accept it.
//! 3. **Unambiguous.** A `by-id` name is built by udev from the bridge's vendor,
//!    product and serial. When two USB devices report the same triple their
//!    `by-id` names collide, and which converter ends up wearing the name is
//!    enumeration order again, one level up. Refused.
//!
//! Rule 3 does **not** fire on the reference configuration. Its three ports are
//! three interfaces of one physical converter — one `usb_device`, so one
//! identity, so no collision — and its serial is blank, which udev handles by
//! falling back to the model name plus the interface number. That is why the
//! `by-id` names in `deploy/kdtvd.toml` carry no serial. It is stable for one
//! converter of that model and stops being stable the moment a second is
//! plugged in, which is exactly when rule 3 fires.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use kdtv_config::PortPath;
use kdtv_units::LinkKind;

use crate::link::{Backend, LineSettings};
use crate::sysfs::{SysfsView, TtyCandidate};

/// Where a pseudo-terminal lives. A canonical path under here is a PTY,
/// whatever the configured name was.
const PTY_DIR: &str = "/dev/pts";

/// The USB identity behind a device node.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UsbIdentity {
    pub vendor: String,
    pub product: String,
    /// `None` when the bridge's EEPROM carries no serial number.
    pub serial: Option<String>,
    /// The physical USB device, e.g. `1-1.4`.
    pub usb_device: String,
    /// The USB interface, e.g. `1-1.4:1.0`.
    pub devpath: String,
}

impl fmt::Display for UsbIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{} at {}", self.vendor, self.product, self.devpath)?;
        match &self.serial {
            Some(s) => write!(f, " serial {s}"),
            None => f.write_str(" (no serial)"),
        }
    }
}

/// What sits between the service and the bus.
///
/// There is no `Unknown` variant. A device the kernel does not describe is not
/// resolved at all — see [`ResolveError::NotEnumerated`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BridgeKind {
    /// An FTDI bridge. `latency_timer` is FTDI's, and this is the only family
    /// whose low-latency setting this service knows how to establish.
    Ftdi { part: String },
    /// Some other USB bridge, named by its driver. Refused at open until its
    /// own low-latency equivalent has been established — see [`crate::latency`].
    Other { driver: String },
    /// A pseudo-terminal. No USB bridge, no latency timer, and never gated.
    Pty,
}

impl BridgeKind {
    /// The backend the transmit gate keys on.
    #[must_use]
    pub const fn backend(&self) -> Backend {
        match self {
            Self::Ftdi { .. } | Self::Other { .. } => Backend::Serial,
            Self::Pty => Backend::Pty,
        }
    }

    /// FTDI part names by USB product id.
    ///
    /// Tier `[C]`: read off FTDI's published product-id list, not measured here.
    /// The name is for the boot log; nothing branches on it.
    #[must_use]
    pub fn ftdi_part(vendor: &str, product: &str) -> String {
        match (vendor, product) {
            ("0403", "6001") => "FT232R".to_owned(),
            ("0403", "6010") => "FT2232H".to_owned(),
            ("0403", "6011") => "FT4232H".to_owned(),
            ("0403", "6014") => "FT232H".to_owned(),
            ("0403", "6015") => "FT-X".to_owned(),
            _ => format!("ftdi {vendor}:{product}"),
        }
    }
}

impl fmt::Display for BridgeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ftdi { part } => write!(f, "FTDI {part}"),
            Self::Other { driver } => write!(f, "{driver}"),
            Self::Pty => f.write_str("pty"),
        }
    }
}

/// One link, bound to one device that has been shown to be present, distinct
/// and unambiguous.
///
/// [`resolve_distinct`] is the only constructor, and it either produces one of
/// these for **every** link it was given or produces none at all.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ResolvedPort {
    link: LinkKind,
    configured: PortPath,
    device: PathBuf,
    node: String,
    bridge: BridgeKind,
    usb: Option<UsbIdentity>,
}

impl ResolvedPort {
    #[must_use]
    pub const fn link(&self) -> LinkKind {
        self.link
    }

    /// The name the configuration used.
    #[must_use]
    pub const fn configured(&self) -> &PortPath {
        &self.configured
    }

    /// The device node the name resolved to. This is what gets opened.
    #[must_use]
    pub fn device(&self) -> &Path {
        &self.device
    }

    /// The kernel node name, e.g. `ttyUSB0`, which is how sysfs attributes are
    /// addressed.
    #[must_use]
    pub fn node(&self) -> &str {
        &self.node
    }

    #[must_use]
    pub const fn bridge(&self) -> &BridgeKind {
        &self.bridge
    }

    /// `None` for a pseudo-terminal.
    #[must_use]
    pub const fn usb(&self) -> Option<&UsbIdentity> {
        self.usb.as_ref()
    }

    #[must_use]
    pub const fn backend(&self) -> Backend {
        self.bridge.backend()
    }
}

impl fmt::Display for ResolvedPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} -> {} via {}",
            self.link,
            self.device.display(),
            self.bridge
        )
    }
}

/// A resolved port plus the line settings for its protocol: everything
/// [`LinkFactory::open`](crate::LinkFactory::open) needs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PortBinding {
    port: ResolvedPort,
    line: LineSettings,
}

impl PortBinding {
    #[must_use]
    fn new(port: ResolvedPort) -> Self {
        let line = LineSettings::for_link(port.link);
        Self { port, line }
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        self.port.link
    }

    #[must_use]
    pub const fn port(&self) -> &ResolvedPort {
        &self.port
    }

    #[must_use]
    pub const fn line(&self) -> LineSettings {
        self.line
    }

    #[must_use]
    pub const fn backend(&self) -> Backend {
        self.port.backend()
    }
}

/// Why the service will not start.
///
/// Every variant names the link, because the operator's next action is physical
/// and needs to know which cable to look at.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// The configured path resolves to nothing.
    #[error("{link}: {path} is not present ({source})")]
    NotPresent {
        link: LinkKind,
        path: String,
        #[source]
        source: std::io::Error,
    },

    /// The path resolved, but the kernel describes no such serial device — so
    /// its bridge family is unknown and it cannot be hardened.
    #[error("{link}: {} is not a USB serial device this kernel describes", device.display())]
    NotEnumerated { link: LinkKind, device: PathBuf },

    /// Two links, one device.
    #[error("{a} and {b} both resolve to {}", device.display())]
    Collision {
        a: LinkKind,
        b: LinkKind,
        device: PathBuf,
    },

    /// Two USB devices share the identity a `by-id` name is built from.
    #[error(
        "{link}: {path} is not a stable name — {count} USB devices report {identity}, \
         so which one wears this name is enumeration order. Bind by /dev/serial/by-path \
         instead, or program distinct serial numbers."
    )]
    AmbiguousById {
        link: LinkKind,
        path: String,
        identity: String,
        count: usize,
    },

    /// The configuration still carries the rig's pseudo-terminal placeholder.
    #[error(
        "{link}: the pty placeholder has not been substituted; call ValidatedConfig::bind_ptys first"
    )]
    UnboundPlaceholder { link: LinkKind },

    /// The enumeration itself failed.
    #[error("cannot enumerate USB serial devices")]
    Enumeration {
        #[source]
        source: std::io::Error,
    },
}

/// The links a validated configuration asks for, paired with their configured
/// paths, in the order [`kdtv_config::ValidatedConfig::links`] gives.
///
/// Steam appears only when it is enabled. A disabled steam block still has its
/// port checked for shape and collision by `kdtv-config`, but there is nothing
/// here to bind: a link that is not driven is not opened.
#[must_use]
pub fn bindings_of(cfg: &kdtv_config::ValidatedConfig) -> Vec<(LinkKind, PortPath)> {
    let mut out: Vec<(LinkKind, PortPath)> = cfg
        .zones()
        .into_iter()
        .map(|z| (z.link(), z.port().clone()))
        .collect();
    if let Some(steam) = cfg.steam() {
        out.push((steam.link(), steam.port().clone()));
    }
    out
}

/// Binds every link to a device, or refuses.
///
/// All-or-nothing: on any failure no [`PortBinding`] is returned for any link,
/// so there is no partially-bound state for a caller to proceed from.
pub fn resolve_distinct(
    bindings: &[(LinkKind, PortPath)],
    sysfs: &dyn SysfsView,
) -> Result<Vec<PortBinding>, ResolveError> {
    // Placeholders first, before anything touches the USB tree. An unbound
    // placeholder is a caller error that no amount of enumeration can fix, and
    // the machine that most often has one — a developer box running the
    // emulated profile — is also the machine least likely to have a
    // /dev/serial/by-id at all. Enumerating first meant `kdtvd --check-only
    // --config deploy/kdtvd.emulated.toml` reported "cannot enumerate USB
    // serial devices" when the real answer was "call bind_ptys first".
    //
    // This also gives an unbound placeholder precedence over a collision or an
    // absent device on another link, wherever it sits in the list. That is the
    // right order: until the placeholders are substituted the binding list is
    // not the one the service will run with, so anything else it says about
    // those links is provisional.
    for (link, configured) in bindings {
        if configured.as_path().is_none() {
            return Err(ResolveError::UnboundPlaceholder { link: *link });
        }
    }

    // Canonicalise before enumerating, because whether the USB tree is needed at
    // all is a property of what the paths resolve *to*.
    let mut canonical: Vec<(LinkKind, &PortPath, PathBuf)> = Vec::with_capacity(bindings.len());
    for (link, configured) in bindings {
        let link = *link;
        let Some(path) = configured.as_path() else {
            // Ruled out above; the pattern stays so the binding is irrefutable.
            return Err(ResolveError::UnboundPlaceholder { link });
        };
        let device = sysfs
            .canonicalize(path)
            .map_err(|source| ResolveError::NotPresent {
                link,
                path: path.display().to_string(),
                source,
            })?;
        canonical.push((link, configured, device));
    }

    // Enumerate only if some link is not a pseudo-terminal.
    //
    // A PTY has no USB bridge, no `by-id` name and no latency timer, so nothing
    // below reads the enumeration for one. Enumerating unconditionally made an
    // all-pseudo-terminal bench configuration refuse to start on any machine
    // with no usbserial driver loaded — which is every CI runner and most
    // developer boxes — and the failure it reported, "cannot enumerate USB
    // serial devices", named a bus the configuration does not mention. The
    // end-to-end suite was carrying a mount-namespace shim to work around it.
    let needs_usb = canonical.iter().any(|(_, _, d)| !d.starts_with(PTY_DIR));
    let enumerated = if needs_usb {
        sysfs
            .enumerate()
            .map_err(|source| ResolveError::Enumeration { source })?
    } else {
        Vec::new()
    };
    let by_node: BTreeMap<&str, &TtyCandidate> =
        enumerated.iter().map(|c| (c.node.as_str(), c)).collect();

    let mut resolved: Vec<ResolvedPort> = Vec::with_capacity(bindings.len());
    let mut seen: BTreeMap<PathBuf, LinkKind> = BTreeMap::new();

    for (link, configured, device) in canonical {
        // Distinctness is decided on the canonical node, never on the
        // configured name.
        if let Some(other) = seen.insert(device.clone(), link) {
            return Err(ResolveError::Collision {
                a: other,
                b: link,
                device,
            });
        }

        // The backend follows the canonical path, so a /dev/pts name symlinked
        // to a real converter is a serial backend and stays gated.
        if device.starts_with(PTY_DIR) {
            resolved.push(ResolvedPort {
                link,
                configured: configured.clone(),
                node: node_name(&device),
                device,
                bridge: BridgeKind::Pty,
                usb: None,
            });
            continue;
        }

        let node = node_name(&device);
        let candidate = by_node
            .get(node.as_str())
            .ok_or_else(|| ResolveError::NotEnumerated {
                link,
                device: device.clone(),
            })?;

        if matches!(configured, PortPath::ById(_)) {
            let siblings = devices_sharing_identity(&enumerated, candidate);
            if siblings > 1 {
                return Err(ResolveError::AmbiguousById {
                    link,
                    path: configured
                        .as_path()
                        .map_or_else(String::new, |p| p.display().to_string()),
                    identity: identity_of(candidate).to_string(),
                    count: siblings,
                });
            }
        }

        let bridge = if candidate.driver == "ftdi_sio" {
            BridgeKind::Ftdi {
                part: BridgeKind::ftdi_part(&candidate.vendor, &candidate.product),
            }
        } else {
            BridgeKind::Other {
                driver: candidate.driver.clone(),
            }
        };

        resolved.push(ResolvedPort {
            link,
            configured: configured.clone(),
            device,
            node,
            bridge,
            usb: Some(identity_of(candidate)),
        });
    }

    Ok(resolved.into_iter().map(PortBinding::new).collect())
}

fn identity_of(c: &TtyCandidate) -> UsbIdentity {
    UsbIdentity {
        vendor: c.vendor.clone(),
        product: c.product.clone(),
        serial: c.serial.clone(),
        usb_device: c.usb_device.clone(),
        devpath: c.devpath.clone(),
    }
}

/// How many distinct physical USB devices report the identity udev builds a
/// `by-id` name from. More than one means the name is ambiguous.
fn devices_sharing_identity(all: &[TtyCandidate], target: &TtyCandidate) -> usize {
    all.iter()
        .filter(|c| c.identity() == target.identity())
        .map(|c| c.usb_device.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len()
}

fn node_name(device: &Path) -> String {
    device
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sysfs::DirSysfs;
    use kdtv_config::Profile;
    use kdtv_units::ZoneId;

    const Z1: LinkKind = LinkKind::Zone(ZoneId::Zone1);
    const Z2: LinkKind = LinkKind::Zone(ZoneId::Zone2);
    const W: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485";

    fn by_id(name: &str) -> PortPath {
        PortPath::parse("zones.zone1.port", name, Profile::Production).unwrap()
    }

    fn by_path(name: &str) -> PortPath {
        PortPath::parse("zones.zone1.port", name, Profile::Production).unwrap()
    }

    fn reference_bindings() -> Vec<(LinkKind, PortPath)> {
        vec![
            (Z1, by_id(&format!("{W}-if00-port0"))),
            (Z2, by_id(&format!("{W}-if01-port0"))),
            (LinkKind::Steam, by_id(&format!("{W}-if02-port0"))),
        ]
    }

    #[test]
    fn the_reference_configuration_binds_all_three_links() {
        let fs = DirSysfs::fixture("reference");
        let bound = resolve_distinct(&reference_bindings(), &fs).unwrap();
        assert_eq!(bound.len(), 3);
        assert_eq!(bound[0].port().device(), Path::new("/dev/ttyUSB0"));
        assert_eq!(bound[2].link(), LinkKind::Steam);
        assert!(matches!(bound[0].port().bridge(), BridgeKind::Ftdi { .. }));
        assert_eq!(bound[0].backend(), Backend::Serial);
        // The FT4232H is what a three-interface Waveshare board presents.
        assert_eq!(
            bound[0].port().bridge().to_string(),
            "FTDI FT4232H".to_owned()
        );
        // Same physical converter behind all three.
        let usb = bound[1].port().usb().unwrap();
        assert_eq!(usb.usb_device, "1-1.4");
        assert!(usb.serial.is_none());
    }

    /// Present or refuse. There is no degraded branch that starts the two zones
    /// and leaves steam out.
    #[test]
    fn req_design_ser_02_req_hardware_usb_03_a_missing_interface_refuses_the_whole_start() {
        let fs = DirSysfs::fixture("missing-interface");
        let err = resolve_distinct(&reference_bindings(), &fs).unwrap_err();
        match err {
            ResolveError::NotPresent { link, ref path, .. } => {
                assert_eq!(link, LinkKind::Steam);
                assert!(path.contains("if02"), "{path}");
            }
            other => panic!("{other:?}"),
        }
        assert!(err.to_string().contains("steam"));
    }

    #[test]
    fn two_converters_sharing_a_serial_number_make_a_by_id_name_meaningless() {
        let fs = DirSysfs::fixture("shared-serial");
        let bindings = vec![
            (
                Z1,
                by_id("/dev/serial/by-id/usb-Waveshare_USB_TO_RS485_FT8N4KZL-if00-port0"),
            ),
            (
                Z2,
                by_id("/dev/serial/by-id/usb-Waveshare_USB_TO_RS485_FT8N4KZL-if01-port0"),
            ),
        ];
        let err = resolve_distinct(&bindings, &fs).unwrap_err();
        match err {
            ResolveError::AmbiguousById {
                link,
                count,
                ref identity,
                ..
            } => {
                assert_eq!(link, Z1);
                assert_eq!(count, 2);
                assert!(identity.contains("FT8N4KZL"), "{identity}");
            }
            other => panic!("{other:?}"),
        }
        // The message tells the operator what to do instead.
        assert!(err.to_string().contains("by-path"), "{err}");
    }

    /// The same machine, bound by physical port instead. `by-path` names the
    /// socket, which two converters cannot share.
    #[test]
    fn by_path_binds_the_same_ambiguous_machine() {
        let fs = DirSysfs::fixture("shared-serial");
        let bindings = vec![
            (
                Z1,
                by_path("/dev/serial/by-path/pci-0000:01:00.0-usb-0:1.4:1.0-port0"),
            ),
            (
                Z2,
                by_path("/dev/serial/by-path/pci-0000:01:00.0-usb-0:1.5:1.0-port0"),
            ),
        ];
        let bound = resolve_distinct(&bindings, &fs).unwrap();
        assert_eq!(bound.len(), 2);
        assert_ne!(bound[0].port().device(), bound[1].port().device());
    }

    #[test]
    fn req_hardware_usb_03_two_names_for_one_device_are_refused_by_the_canonical_path() {
        let fs = DirSysfs::fixture("aliased");
        let bindings = vec![
            (Z1, by_id(&format!("{W}-if00-port0"))),
            (Z2, by_id(&format!("{W}-if01-port0"))),
        ];
        let err = resolve_distinct(&bindings, &fs).unwrap_err();
        match err {
            ResolveError::Collision { a, b, ref device } => {
                assert_eq!((a, b), (Z1, Z2));
                assert_eq!(device, Path::new("/dev/ttyUSB0"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_non_ftdi_bridge_resolves_and_is_named_by_its_driver() {
        let fs = DirSysfs::fixture("non-ftdi");
        let bindings = vec![(
            Z1,
            by_id("/dev/serial/by-id/usb-Silicon_Labs_CP2102_0001-if00-port0"),
        )];
        let bound = resolve_distinct(&bindings, &fs).unwrap();
        assert_eq!(
            bound[0].port().bridge(),
            &BridgeKind::Other {
                driver: "cp210x".to_owned()
            }
        );
        // Resolution is not permission: hardening refuses it. See crate::latency.
        assert_eq!(bound[0].backend(), Backend::Serial);
    }

    #[test]
    fn a_pty_is_a_pty_backend_and_carries_no_usb_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("dev/pts")).unwrap();
        std::fs::create_dir_all(root.join("devices")).unwrap();
        std::fs::write(root.join("dev/pts/7"), "/dev/pts/7\n").unwrap();
        let fs = DirSysfs::new(root);
        let bindings = vec![(
            Z1,
            PortPath::parse("zones.zone1.port", "/dev/pts/7", Profile::Bench).unwrap(),
        )];
        let bound = resolve_distinct(&bindings, &fs).unwrap();
        assert_eq!(bound[0].backend(), Backend::Pty);
        assert_eq!(bound[0].port().bridge(), &BridgeKind::Pty);
        assert!(bound[0].port().usb().is_none());
    }

    /// A `/dev/pts` name pointing at a real converter is a serial backend, so
    /// the gate still sees it. Deciding the backend from the configured string
    /// would have let a symlink walk round the gate.
    #[test]
    fn the_backend_follows_the_canonical_path_not_the_configured_name() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("dev/pts")).unwrap();
        std::fs::write(root.join("dev/pts/7"), "/dev/ttyUSB0\n").unwrap();
        let devices = root.join("devices/ttyUSB0");
        std::fs::create_dir_all(&devices).unwrap();
        for (k, v) in [
            ("driver", "ftdi_sio"),
            ("idVendor", "0403"),
            ("idProduct", "6011"),
            ("usb_device", "1-1.4"),
            ("devpath", "1-1.4:1.0"),
            ("latency_timer", "16"),
        ] {
            std::fs::write(devices.join(k), v).unwrap();
        }
        let fs = DirSysfs::new(root);
        let bindings = vec![(
            Z1,
            PortPath::parse("zones.zone1.port", "/dev/pts/7", Profile::Bench).unwrap(),
        )];
        let bound = resolve_distinct(&bindings, &fs).unwrap();
        assert_eq!(bound[0].backend(), Backend::Serial);
    }

    #[test]
    fn the_rig_placeholder_must_be_substituted_first() {
        let fs = DirSysfs::fixture("reference");
        let bindings = vec![(
            Z1,
            PortPath::parse("zones.zone1.port", "/dev/pts/PLACEHOLDER", Profile::Bench).unwrap(),
        )];
        assert!(matches!(
            resolve_distinct(&bindings, &fs),
            Err(ResolveError::UnboundPlaceholder { link: Z1 })
        ));
    }

    /// The placeholder answer must not depend on the USB tree existing.
    ///
    /// The test above uses a fixture where enumeration succeeds, so it passed
    /// while the placeholder check sat *after* the enumeration and could not be
    /// reached on a machine with no `/dev/serial/by-id`. That is the machine
    /// running the emulated profile.
    #[test]
    fn a_placeholder_is_named_even_where_there_is_no_usb_tree_to_enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let fs = DirSysfs::new(dir.path());
        // Nothing under the root at all: no dev/, no devices/.
        assert!(
            fs.enumerate().is_err(),
            "this test is only meaningful where enumeration fails"
        );
        let bindings = vec![(
            Z1,
            PortPath::parse("zones.zone1.port", "/dev/pts/PLACEHOLDER", Profile::Bench).unwrap(),
        )];
        assert!(matches!(
            resolve_distinct(&bindings, &fs),
            Err(ResolveError::UnboundPlaceholder { link: Z1 })
        ));
    }

    /// An all-pseudo-terminal configuration resolves where there is no USB tree
    /// to enumerate.
    ///
    /// This is the bench profile on any machine with no usbserial driver loaded
    /// — every CI runner, and most developer boxes. Enumerating unconditionally
    /// made it refuse to start, reporting a bus its configuration never
    /// mentions, and the end-to-end suite carried a mount-namespace shim to
    /// work around it.
    #[test]
    fn a_configuration_of_only_pseudo_terminals_needs_no_usb_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("dev/pts")).unwrap();
        std::fs::write(root.join("dev/pts/3"), "/dev/pts/3\n").unwrap();
        std::fs::write(root.join("dev/pts/4"), "/dev/pts/4\n").unwrap();
        let fs = DirSysfs::new(root);
        // No `devices` directory under the root, so enumeration fails.
        assert!(
            fs.enumerate().is_err(),
            "this test is only meaningful where enumeration fails"
        );

        let bindings = vec![
            (
                Z1,
                PortPath::parse("zones.zone1.port", "/dev/pts/3", Profile::Bench).unwrap(),
            ),
            (
                LinkKind::Zone(ZoneId::Zone2),
                PortPath::parse("zones.zone2.port", "/dev/pts/4", Profile::Bench).unwrap(),
            ),
        ];
        let bound = resolve_distinct(&bindings, &fs).expect("pseudo-terminals need no USB tree");
        assert_eq!(bound.len(), 2);
        for b in &bound {
            assert_eq!(b.backend(), Backend::Pty);
        }
    }

    /// One real port among the pseudo-terminals still needs the tree, so the
    /// laziness cannot be used to skip the checks a converter requires.
    #[test]
    fn one_real_port_among_them_still_requires_the_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("dev/pts")).unwrap();
        std::fs::write(root.join("dev/pts/3"), "/dev/pts/3\n").unwrap();
        std::fs::create_dir_all(root.join("dev/serial/by-id")).unwrap();
        std::fs::write(
            root.join("dev/serial/by-id/usb-thing-if00-port0"),
            "/dev/ttyUSB0\n",
        )
        .unwrap();
        // Again no `devices` directory: both paths canonicalise, and the USB
        // one makes the enumeration necessary.
        let fs = DirSysfs::new(root);

        let bindings = vec![
            (
                Z1,
                PortPath::parse("zones.zone1.port", "/dev/pts/3", Profile::Bench).unwrap(),
            ),
            (
                LinkKind::Zone(ZoneId::Zone2),
                by_id("/dev/serial/by-id/usb-thing-if00-port0"),
            ),
        ];
        // Both canonicalise, so the failure has to come from the enumeration
        // this configuration still requires — proving the laziness cannot be
        // used to skip the checks a converter needs.
        assert!(matches!(
            resolve_distinct(&bindings, &fs),
            Err(ResolveError::Enumeration { .. })
        ));
    }

    /// ...and a configuration with no placeholder is still refused, so hoisting
    /// the placeholder check did not swallow the failure.
    ///
    /// It is now [`ResolveError::NotPresent`] rather than
    /// [`ResolveError::Enumeration`]: canonicalisation runs first, so a `by-id`
    /// name that is not there is reported as the missing name rather than as a
    /// bus that could not be read. That is the more specific of the two, and it
    /// names the thing the operator can fix.
    #[test]
    fn a_real_binding_that_is_not_there_names_the_path_not_the_bus() {
        let dir = tempfile::tempdir().unwrap();
        let fs = DirSysfs::new(dir.path());
        let bindings = vec![(Z1, by_id("/dev/serial/by-id/usb-anything-if00-port0"))];
        assert!(matches!(
            resolve_distinct(&bindings, &fs),
            Err(ResolveError::NotPresent { link: Z1, .. })
        ));
    }

    #[test]
    fn a_node_the_kernel_does_not_describe_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("dev/serial/by-id")).unwrap();
        std::fs::create_dir_all(root.join("devices")).unwrap();
        std::fs::write(
            root.join("dev/serial/by-id/usb-ghost-if00-port0"),
            "/dev/ttyUSB9\n",
        )
        .unwrap();
        let fs = DirSysfs::new(root);
        let bindings = vec![(Z1, by_id("/dev/serial/by-id/usb-ghost-if00-port0"))];
        assert!(matches!(
            resolve_distinct(&bindings, &fs),
            Err(ResolveError::NotEnumerated { .. })
        ));
    }

    #[test]
    fn ftdi_part_names_fall_back_to_the_ids() {
        assert_eq!(BridgeKind::ftdi_part("0403", "6011"), "FT4232H");
        assert_eq!(BridgeKind::ftdi_part("0403", "6001"), "FT232R");
        assert_eq!(BridgeKind::ftdi_part("0403", "9999"), "ftdi 0403:9999");
    }
}
