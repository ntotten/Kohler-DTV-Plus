//! The transmit gate's second boundary, and the thing that opens ports.
//!
//! # Why the gate is here as well as in the encoder
//!
//! `kdtv-proto` already refuses to build a frame without a
//! [`TransmitAuthority`]. That is the stronger property — nothing to transmit
//! beats nothing transmitted — but it is not sufficient on its own. Gating only
//! the encoder leaves a **real port open with a real `SerialStream` behind it**,
//! and the safety claim then rests on nothing anywhere ever writing bytes to
//! that fd from another source: a debug helper, a future feature, a `dyn Write`
//! handed somewhere it should not have been. The claim would be about the whole
//! workspace's future rather than about one function.
//!
//! So [`LinkFactory::open`] refuses [`Backend::Serial`] unless
//! [`TransmitAuthority::permits_real_bus_on`] returns true **for that link**.
//! With today's evidence base — every fixture tier `[C]` — that is never true,
//! and the daemon boots, runs the whole emulated suite and cannot open
//! `/dev/ttyUSB0`.
//!
//! Authority is per link because polarity is measured per link (`OPEN-01`). A
//! second zone commissioned later does not ride in on the first zone's
//! attestation.
//!
//! # Order of operations
//!
//! [`permit_open`] runs **first**, before resolution results are used, before
//! the latency timer is touched and before any fd exists. A refused open leaves
//! nothing behind to close.

use std::fmt;
use std::path::PathBuf;

use kdtv_proto::TransmitAuthority;
use kdtv_units::LinkKind;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio_serial::{SerialPortBuilderExt as _, SerialStream};

use crate::latency::{Hardened, harden};
use crate::link::{Backend, BoxedFuture, Link, LinkDescriptor, LinkIoError};
use crate::resolve::PortBinding;
use crate::sysfs::SysfsView;

/// What the transmit authority said, at the moment a port was opened.
///
/// Copied into the [`LinkDescriptor`] rather than looked up later: "which
/// evidence base was in force when this port was opened" is the first question
/// a support transcript has to answer, and a configuration file may since have
/// changed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AuthorityRecord {
    scope: &'static str,
    capture_ref: Option<String>,
    fixtures_sha256: [u8; 32],
}

impl AuthorityRecord {
    #[must_use]
    pub fn of(auth: &TransmitAuthority) -> Self {
        Self {
            scope: scope_label(auth),
            capture_ref: auth.capture_ref().map(str::to_owned),
            fixtures_sha256: auth.fixtures_sha256(),
        }
    }

    #[must_use]
    pub const fn scope(&self) -> &'static str {
        self.scope
    }

    #[must_use]
    pub fn capture_ref(&self) -> Option<&str> {
        self.capture_ref.as_deref()
    }

    #[must_use]
    pub const fn fixtures_sha256(&self) -> [u8; 32] {
        self.fixtures_sha256
    }
}

impl fmt::Display for AuthorityRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.scope)?;
        if let Some(c) = &self.capture_ref {
            write!(f, " against {c}")?;
        }
        Ok(())
    }
}

fn scope_label(auth: &TransmitAuthority) -> &'static str {
    if auth.permits_real_bus() {
        "real-bus-attested"
    } else {
        "emulator-only"
    }
}

/// **The gate.** The one decision every factory in this workspace must route
/// through — this crate's and the emulator's alike.
///
/// It is a free function rather than a method so that a second implementation
/// of [`LinkFactory`] cannot quietly grow its own version of the check.
pub fn permit_open(
    backend: Backend,
    link: LinkKind,
    auth: &TransmitAuthority,
) -> Result<(), OpenError> {
    if backend.is_real_bus() && !auth.permits_real_bus_on(link) {
        return Err(OpenError::TransmitGateClosed {
            link,
            scope: scope_label(auth),
        });
    }
    Ok(())
}

/// Opens links. The only route from a [`PortBinding`] to a [`Link`].
pub trait LinkFactory: Send + fmt::Debug {
    /// Opens one link, or refuses.
    ///
    /// The link kind is not a separate parameter: it is
    /// [`PortBinding::link`], and passing it twice would let a caller ask for
    /// one link's authority while opening another's port.
    fn open<'a>(
        &'a mut self,
        binding: &'a PortBinding,
        auth: &'a TransmitAuthority,
    ) -> BoxedFuture<'a, Result<Box<dyn Link>, OpenError>>;
}

/// Why a port was not opened.
#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    /// A real serial backend was requested and the authority does not permit a
    /// real bus on this link.
    ///
    /// This is the expected state of the committed evidence base, not an
    /// unexpected failure. Every wire fixture is tier `[C]`.
    #[error(
        "{link}: the transmit gate is closed (scope {scope}); a real serial port \
         cannot be opened until this link's fixtures are captured and its bus \
         polarity attested"
    )]
    TransmitGateClosed { link: LinkKind, scope: &'static str },

    /// The latency timer was written and did not take. See [`crate::latency`].
    #[error(
        "{link}: {node} latency_timer wrote {wrote} ms and read back {read_back} ms; \
         timing measured through this bridge would be quantised to {read_back} ms"
    )]
    NotLowLatency {
        link: LinkKind,
        node: String,
        wrote: u8,
        read_back: u8,
    },

    /// The bridge is not FTDI, so `latency_timer` does not apply and no
    /// equivalent has been established for it.
    #[error(
        "{link}: {part} is not an FTDI bridge; latency_timer is FTDI-specific and \
         this family's low-latency equivalent has not been established"
    )]
    NonFtdiBridge { link: LinkKind, part: String },

    /// The latency attribute could not be written or read.
    #[error("{link}: cannot set {node} latency_timer")]
    LatencyWrite {
        link: LinkKind,
        node: String,
        #[source]
        source: std::io::Error,
    },

    /// A backend this factory has no implementation for.
    #[error("{link}: no {backend} backend here — {why}")]
    BackendUnavailable {
        link: LinkKind,
        backend: Backend,
        why: &'static str,
    },

    /// The open itself failed.
    #[error("{link}: cannot open {}", device.display())]
    Io {
        link: LinkKind,
        device: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl OpenError {
    #[must_use]
    pub const fn link(&self) -> LinkKind {
        match self {
            Self::TransmitGateClosed { link, .. }
            | Self::NotLowLatency { link, .. }
            | Self::NonFtdiBridge { link, .. }
            | Self::LatencyWrite { link, .. }
            | Self::BackendUnavailable { link, .. }
            | Self::Io { link, .. } => *link,
        }
    }

    /// True when the refusal is the transmit gate rather than a fault. The boot
    /// path reports the two differently: a closed gate is the committed
    /// position, a fault is something to fix.
    #[must_use]
    pub const fn is_gate(&self) -> bool {
        matches!(self, Self::TransmitGateClosed { .. })
    }
}

/// The Linux factory: `tokio-serial` over a real converter or a pseudo-terminal.
#[derive(Debug)]
pub struct LinuxLinkFactory<S: SysfsView> {
    sysfs: S,
}

impl<S: SysfsView> LinuxLinkFactory<S> {
    pub const fn new(sysfs: S) -> Self {
        Self { sysfs }
    }

    fn open_blocking(
        &self,
        binding: &PortBinding,
        auth: &TransmitAuthority,
    ) -> Result<SerialLink, OpenError> {
        let link = binding.link();

        // 1. The gate, before anything is resolved into an fd.
        permit_open(binding.backend(), link, auth)?;

        // Exhaustive rather than a catch-all. Today `BridgeKind` has no
        // loopback case, so a `PortBinding` cannot carry one and this arm
        // cannot fire — but if a route to one is ever added, this is where it
        // has to be decided rather than falling through into an open.
        match binding.backend() {
            Backend::Serial | Backend::Pty => {}
            Backend::Loopback => {
                return Err(OpenError::BackendUnavailable {
                    link,
                    backend: Backend::Loopback,
                    why: "the loopback backend lives in kdtv-emulator, which cargo xtask \
                          audit-graph keeps out of the daemon's dependency graph",
                });
            }
        }

        // 2. Harden the bridge. A port that cannot be hardened is one this
        //    service never held an fd to.
        let hardened: Hardened = harden(binding, &self.sysfs)?;

        // 3. Open.
        let device = binding.port().device().to_path_buf();
        let settings = binding.line();
        let stream = tokio_serial::new(device.to_string_lossy(), settings.baud)
            .data_bits(data_bits(settings.data_bits))
            .stop_bits(stop_bits(settings.stop_bits))
            // Neither protocol uses parity or flow control. RS-485 direction is
            // the converter's own automatic control; there is no RTS toggling
            // here and no local echo to wait for (`CORRECTIONS.md` item 3).
            .parity(tokio_serial::Parity::None)
            .flow_control(tokio_serial::FlowControl::None)
            .open_native_async()
            .map_err(|source| OpenError::Io {
                link,
                device: device.clone(),
                source: source.into(),
            })?;

        Ok(SerialLink {
            stream,
            descriptor: LinkDescriptor::new(
                link,
                binding.backend(),
                device,
                settings,
                hardened,
                AuthorityRecord::of(auth),
            ),
        })
    }
}

fn data_bits(n: u8) -> tokio_serial::DataBits {
    match n {
        5 => tokio_serial::DataBits::Five,
        6 => tokio_serial::DataBits::Six,
        7 => tokio_serial::DataBits::Seven,
        // Both protocols are eight data bits; anything else is a bug in the
        // constants, and eight is the value that matches them.
        _ => tokio_serial::DataBits::Eight,
    }
}

fn stop_bits(n: u8) -> tokio_serial::StopBits {
    match n {
        2 => tokio_serial::StopBits::Two,
        _ => tokio_serial::StopBits::One,
    }
}

impl<S: SysfsView> LinkFactory for LinuxLinkFactory<S> {
    fn open<'a>(
        &'a mut self,
        binding: &'a PortBinding,
        auth: &'a TransmitAuthority,
    ) -> BoxedFuture<'a, Result<Box<dyn Link>, OpenError>> {
        Box::pin(async move {
            let opened: Box<dyn Link> = Box::new(self.open_blocking(binding, auth)?);
            Ok(opened)
        })
    }
}

/// A link over a real serial port or a pseudo-terminal.
struct SerialLink {
    stream: SerialStream,
    descriptor: LinkDescriptor,
}

impl fmt::Debug for SerialLink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SerialLink")
            .field("descriptor", &self.descriptor)
            .finish_non_exhaustive()
    }
}

impl Link for SerialLink {
    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> BoxedFuture<'a, Result<(), LinkIoError>> {
        let link = self.descriptor.link();
        Box::pin(async move {
            self.stream
                .write_all(buf)
                .await
                .map_err(|e| LinkIoError::classify(link, e))?;
            // Flushed on every frame: a Saturn transaction's response deadline
            // starts at the last byte on the wire, and a frame sitting in a
            // buffer would be measured as bus latency it is not.
            self.stream
                .flush()
                .await
                .map_err(|e| LinkIoError::classify(link, e))
        })
    }

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxedFuture<'a, Result<usize, LinkIoError>> {
        let link = self.descriptor.link();
        Box::pin(async move {
            match self.stream.read(buf).await {
                Ok(0) => Err(LinkIoError::eof(link)),
                Ok(n) => Ok(n),
                Err(e) => Err(LinkIoError::classify(link, e)),
            }
        })
    }

    fn descriptor(&self) -> &LinkDescriptor {
        &self.descriptor
    }

    fn close(self: Box<Self>) -> BoxedFuture<'static, Result<(), LinkIoError>> {
        Box::pin(async move {
            // Destructured rather than dropped as a whole, so the fd's release
            // is the visible statement it needs to be. After this the
            // descriptor is gone too: there is no closed link left to hold.
            let Self { stream, .. } = *self;
            drop(stream);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::resolve_distinct;
    use crate::sysfs::DirSysfs;
    use kdtv_config::{PortPath, Profile};
    use kdtv_proto::FixtureSet;
    use kdtv_units::ZoneId;

    const Z1: LinkKind = LinkKind::Zone(ZoneId::Zone1);
    const W: &str = "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485";

    fn emulator_only() -> TransmitAuthority {
        TransmitAuthority::emulator_only(FixtureSet::embedded())
    }

    fn serial_binding() -> (DirSysfs, PortBinding) {
        let fs = DirSysfs::fixture("reference");
        let bindings = vec![(
            Z1,
            PortPath::parse(
                "zones.zone1.port",
                &format!("{W}-if00-port0"),
                Profile::Production,
            )
            .unwrap(),
        )];
        let mut bound = resolve_distinct(&bindings, &fs).unwrap();
        (fs, bound.remove(0))
    }

    /// The property the whole crate exists to hold.
    #[tokio::test]
    async fn a_real_serial_port_cannot_be_opened_under_the_committed_evidence_base() {
        let (fs, binding) = serial_binding();
        assert_eq!(binding.backend(), Backend::Serial);
        let auth = emulator_only();
        let mut factory = LinuxLinkFactory::new(fs);
        let err = factory.open(&binding, &auth).await.unwrap_err();
        assert!(err.is_gate(), "{err:?}");
        assert_eq!(err.link(), Z1);
        assert!(err.to_string().contains("emulator-only"), "{err}");
    }

    /// The gate runs before anything else. The reference fixture's `ttyUSB0`
    /// does not exist on a CI runner and its latency timer is at 16, so an
    /// implementation that opened first, or hardened first, would report an I/O
    /// or latency error instead of a closed gate.
    #[tokio::test]
    async fn the_gate_runs_before_the_bridge_is_touched_or_the_fd_exists() {
        let (fs, binding) = serial_binding();
        let auth = emulator_only();
        let mut factory = LinuxLinkFactory::new(fs);
        let err = factory.open(&binding, &auth).await.unwrap_err();
        assert!(
            matches!(err, OpenError::TransmitGateClosed { .. }),
            "{err:?}"
        );
        // Nothing was written to the bridge: it still reads the FTDI default.
        let fs = DirSysfs::fixture("reference");
        assert_eq!(fs.read_latency_timer("ttyUSB0").unwrap(), 16);
    }

    /// `permit_open` is the gate, and it is what every factory must call.
    #[test]
    fn only_the_serial_backend_needs_an_attestation() {
        let auth = emulator_only();
        assert!(permit_open(Backend::Pty, Z1, &auth).is_ok());
        assert!(permit_open(Backend::Loopback, Z1, &auth).is_ok());
        assert!(permit_open(Backend::Pty, LinkKind::Steam, &auth).is_ok());
        for link in LinkKind::ALL {
            let err = permit_open(Backend::Serial, link, &auth).unwrap_err();
            assert!(err.is_gate());
            assert_eq!(err.link(), link);
        }
    }

    /// Not a test of this crate's code so much as a statement of where the
    /// evidence stands: no test here can construct an authority that opens a
    /// serial port, because the committed fixtures are all tier `[C]`.
    #[test]
    fn the_committed_fixture_set_cannot_grant_a_real_bus() {
        let cfg = kdtv_proto::TransmitGateConfig {
            scope: kdtv_proto::RequestedScope::RealBusAttested,
            capture_ref: Some("research/diagnostics/does-not-exist.bin".to_owned()),
            polarity: kdtv_proto::PolarityAttestation::default(),
            expected_fixtures_sha256: Some(FixtureSet::embedded().sha256_hex()),
        };
        let err = TransmitAuthority::resolve(&cfg, FixtureSet::embedded()).unwrap_err();
        assert!(
            format!("{err}").contains("captur") || format!("{err}").contains("polarity"),
            "{err}"
        );
    }

    #[test]
    fn the_authority_record_carries_the_evidence_base_into_the_log() {
        let auth = emulator_only();
        let record = AuthorityRecord::of(&auth);
        assert_eq!(record.scope(), "emulator-only");
        assert_eq!(record.capture_ref(), None);
        assert_eq!(record.fixtures_sha256(), FixtureSet::embedded().sha256());
        assert_eq!(record.to_string(), "emulator-only");
    }

    /// A pseudo-terminal opens under either scope, which is what makes the
    /// whole emulated suite runnable with the gate closed.
    #[tokio::test]
    async fn a_pseudo_terminal_opens_with_the_gate_closed() {
        let Some((primary, secondary)) = pty_pair() else {
            // No /dev/ptmx in this sandbox; the gate assertions above are the
            // ones that matter and they do not need one.
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("dev/pts")).unwrap();
        std::fs::create_dir_all(root.join("devices")).unwrap();
        let rel = secondary.strip_prefix("/").unwrap();
        std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
        std::fs::write(root.join(rel), format!("{secondary}\n")).unwrap();

        let fs = DirSysfs::new(root);
        let bindings = vec![(
            Z1,
            PortPath::parse("zones.zone1.port", &secondary, Profile::Bench).unwrap(),
        )];
        let bound = resolve_distinct(&bindings, &fs).unwrap();
        assert_eq!(bound[0].backend(), Backend::Pty);

        let auth = emulator_only();
        let mut factory = LinuxLinkFactory::new(fs);
        let link = factory.open(&bound[0], &auth).await.unwrap();
        assert_eq!(link.descriptor().backend(), Backend::Pty);
        assert_eq!(link.descriptor().link(), Z1);
        assert_eq!(link.descriptor().hardened().latency_ms(), None);
        // The consuming close leaves nothing to write to.
        link.close().await.unwrap();
        drop(primary);
    }

    /// Bytes actually cross the boundary, and the read side reports a byte
    /// count rather than a frame — framing is `kdtv-proto`'s job, and a HAL that
    /// framed would have to decide where a frame ends before the decoder does.
    #[tokio::test]
    async fn bytes_go_out_and_come_back_over_a_real_file_descriptor() {
        let Some((mut primary, secondary)) = pty_pair() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("devices")).unwrap();
        let rel = secondary.strip_prefix("/").unwrap();
        std::fs::create_dir_all(root.join(rel).parent().unwrap()).unwrap();
        std::fs::write(root.join(rel), format!("{secondary}\n")).unwrap();

        let fs = DirSysfs::new(root);
        let bindings = vec![(
            Z1,
            PortPath::parse("zones.zone1.port", &secondary, Profile::Bench).unwrap(),
        )];
        let bound = resolve_distinct(&bindings, &fs).unwrap();
        let auth = emulator_only();
        let mut factory = LinuxLinkFactory::new(fs);
        let mut link = factory.open(&bound[0], &auth).await.unwrap();

        // The 0xAA 0x55 preamble the Saturn decoder resynchronises on. Its
        // meaning is `kdtv-proto`'s; here it is four bytes.
        link.write_all(&[0xAA, 0x55, 0x02, 0x10]).await.unwrap();
        let mut seen = [0_u8; 8];
        let n = primary.read(&mut seen).await.unwrap();
        assert_eq!(&seen[..n], &[0xAA, 0x55, 0x02, 0x10]);

        primary.write_all(&[0x06, 0x00]).await.unwrap();
        primary.flush().await.unwrap();
        let mut back = [0_u8; 8];
        let n = link.read(&mut back).await.unwrap();
        assert_eq!(&back[..n], &[0x06, 0x00]);

        link.close().await.unwrap();
        drop(primary);
    }

    /// A pty pair, or `None` where the sandbox has no `/dev/ptmx`.
    fn pty_pair() -> Option<(SerialStream, String)> {
        let (primary, secondary) = SerialStream::pair().ok()?;
        let name = tokio_serial::SerialPort::name(&secondary)?;
        drop(secondary);
        Some((primary, name))
    }
}
