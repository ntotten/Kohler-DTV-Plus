//! The byte pipe, and the two faults it can present.
//!
//! # Why close consumes the link
//!
//! "Close the port" appears in the fail-off path: loss of a valid response
//! attempts all-off on the affected zone, closes that port and latches the zone
//! unavailable. If close took `&mut self`, the caller would still hold a value
//! that looks like a working link, and a later write would either silently do
//! nothing or reopen the question of whether the fd is really gone.
//!
//! [`Link::close`] therefore takes `self: Box<Self>`. After it returns, there is
//! no link value left to write to, and the descriptor that named the device is
//! gone with it. A partial escalation — port closed, zone still driveable — is
//! not representable.
//!
//! # Why disconnection is not a read error
//!
//! A USB converter that has fallen off the bus and a converter that returned
//! `EINTR` need different escalations. The first is terminal for that link: the
//! fd will not start working again, retrying is a waste of the tick, and the
//! zone must be latched. The second is one attempt inside a retry budget.
//!
//! [`LinkIoError`] has exactly those two shapes, and
//! [`LinkIoError::classify`] is the single place the kernel's errno becomes one
//! of them.

use std::fmt;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use kdtv_units::LinkKind;

use crate::latency::Hardened;

/// A boxed future, because [`Link`] and [`LinkFactory`](crate::LinkFactory) are
/// used as `dyn` and `async fn` in trait is not `dyn`-compatible.
pub type BoxedFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What kind of thing is behind a link.
///
/// The transmit gate keys on this and nothing else: [`Backend::Serial`] needs a
/// real-bus attestation, the other two do not. It is decided from the
/// **canonical** device path, not the configured one, so a `/dev/pts/7` symlink
/// pointing at `/dev/ttyUSB0` is a serial backend.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Backend {
    /// A real USB-serial converter on a real RS-485 bus. Gated.
    Serial,
    /// A pseudo-terminal, the other end of which is an emulated device.
    Pty,
    /// An in-process pipe. No implementation of it lives in this crate — see
    /// [`OpenError::BackendUnavailable`](crate::OpenError::BackendUnavailable) —
    /// but the gate's decision for it is stated here so both this crate's
    /// factory and the emulator's call the same [`permit_open`](crate::permit_open).
    Loopback,
}

impl Backend {
    /// True when opening this backend requires a real-bus attestation.
    #[must_use]
    pub const fn is_real_bus(self) -> bool {
        matches!(self, Self::Serial)
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Serial => "serial",
            Self::Pty => "pty",
            Self::Loopback => "loopback",
        })
    }
}

/// The line settings for one link.
///
/// Both protocols are 9600 8N1 and the constants come from `kdtv-proto`, not
/// from here: [`kdtv_proto::saturn::BAUD`] and [`kdtv_proto::dtv::BAUD`]. They
/// are read per link rather than shared so that if one ever moves, the other
/// does not follow by accident.
///
/// There is no parity field. Neither protocol uses parity, and a value that
/// could be set to `Even` is a value someone can set to `Even`.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct LineSettings {
    pub baud: u32,
    pub data_bits: u8,
    pub stop_bits: u8,
}

impl LineSettings {
    /// The settings for one link, from the protocol crate's constants.
    #[must_use]
    pub const fn for_link(link: LinkKind) -> Self {
        match link {
            LinkKind::Zone(_) => Self {
                baud: kdtv_proto::saturn::BAUD,
                data_bits: kdtv_proto::saturn::DATA_BITS,
                stop_bits: kdtv_proto::saturn::STOP_BITS,
            },
            LinkKind::Steam => Self {
                baud: kdtv_proto::dtv::BAUD,
                data_bits: kdtv_proto::dtv::DATA_BITS,
                stop_bits: kdtv_proto::dtv::STOP_BITS,
            },
        }
    }
}

impl fmt::Display for LineSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}N{}", self.baud, self.data_bits, self.stop_bits)
    }
}

/// Everything a log line about an open port needs, fixed at open time.
///
/// It carries the transmit authority's fixture-set hash because "which evidence
/// base was in force when this port was opened" is the first question a support
/// transcript has to answer, and the answer must not be re-derived later from a
/// config file that may since have changed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkDescriptor {
    link: LinkKind,
    backend: Backend,
    device: PathBuf,
    line: LineSettings,
    hardened: Hardened,
    authority: crate::factory::AuthorityRecord,
}

impl LinkDescriptor {
    pub(crate) fn new(
        link: LinkKind,
        backend: Backend,
        device: PathBuf,
        settings: LineSettings,
        hardened: Hardened,
        authority: crate::factory::AuthorityRecord,
    ) -> Self {
        Self {
            link,
            backend,
            device,
            line: settings,
            hardened,
            authority,
        }
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        self.link
    }

    #[must_use]
    pub const fn backend(&self) -> Backend {
        self.backend
    }

    /// The canonical device path that was opened, never the configured name.
    #[must_use]
    pub fn device(&self) -> &Path {
        &self.device
    }

    #[must_use]
    pub const fn line(&self) -> LineSettings {
        self.line
    }

    #[must_use]
    pub const fn hardened(&self) -> &Hardened {
        &self.hardened
    }

    #[must_use]
    pub const fn authority(&self) -> &crate::factory::AuthorityRecord {
        &self.authority
    }
}

impl fmt::Display for LinkDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} on {} ({}, {}, {})",
            self.link,
            self.device.display(),
            self.backend,
            self.line,
            self.hardened
        )
    }
}

/// An asynchronous byte pipe to one device.
///
/// Object-safe on purpose: the supervisor holds `Box<dyn Link>` so that the same
/// engine drives a real converter, a pseudo-terminal and the emulator's pipe
/// without three copies of the loop.
pub trait Link: Send + fmt::Debug + 'static {
    /// Writes the whole buffer, or fails.
    ///
    /// A short write is not reported: either every byte reached the driver or an
    /// error did. A half-written Saturn frame on a 9600-baud bus is a frame the
    /// valve will resynchronise out of, and pretending otherwise would put the
    /// retry decision in the wrong place.
    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> BoxedFuture<'a, Result<(), LinkIoError>>;

    /// Reads whatever is available, returning the byte count.
    ///
    /// Never returns `Ok(0)`: end-of-file on a USB serial node means the
    /// converter has gone, which is [`LinkIoError::Disconnected`].
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxedFuture<'a, Result<usize, LinkIoError>>;

    fn descriptor(&self) -> &LinkDescriptor;

    /// Closes the port, consuming the link.
    ///
    /// The fd is released before this returns. There is no way to hold a closed
    /// link, and therefore no way to write to one.
    fn close(self: Box<Self>) -> BoxedFuture<'static, Result<(), LinkIoError>>;
}

/// The two faults a link can present, which take different escalations.
#[derive(Debug, thiserror::Error)]
pub enum LinkIoError {
    /// The device is gone: USB enumeration loss, or the node was unlinked.
    ///
    /// Terminal for this link. The caller must not retry — it must attempt
    /// all-off on the affected zone, close the port and latch the zone. Retrying
    /// here spends the tick on a device that is not coming back.
    #[error("{link}: serial device disconnected ({detail})")]
    Disconnected { link: LinkKind, detail: String },

    /// A transient I/O error. One attempt inside the caller's retry budget.
    ///
    /// The budget is the caller's, not this crate's: `CORRECTIONS.md` item 6
    /// requires an explicit per-transaction budget, because three retries at a
    /// 400 ms response timeout is 1.2 s against a 525 ms cadence.
    #[error("{link}: serial I/O error")]
    Retryable {
        link: LinkKind,
        #[source]
        source: io::Error,
    },
}

impl LinkIoError {
    /// The one place an errno becomes a fault class.
    ///
    /// Disconnection is recognised by errno rather than by `ErrorKind`, because
    /// `ENODEV` and `ENXIO` both arrive as [`io::ErrorKind::Uncategorized`],
    /// which is unstable and not matchable.
    ///
    /// - `ENODEV` — the driver unbound; the converter was unplugged.
    /// - `ENXIO` — no such device or address.
    /// - `EIO` — the USB transfer failed; on `ftdi_sio` this is what a yanked
    ///   cable produces mid-read. Treated as terminal: a bus that returns `EIO`
    ///   is not a bus this service should keep driving water on. `[I]` — that
    ///   `EIO` here always means enumeration loss is inference, not something
    ///   measured on this installation.
    /// - `EBADF`, `EPIPE` — the fd is already gone.
    #[must_use]
    pub fn classify(link: LinkKind, source: io::Error) -> Self {
        let terminal = matches!(
            source.raw_os_error(),
            Some(libc::ENODEV | libc::ENXIO | libc::EIO | libc::EBADF | libc::EPIPE)
        ) || matches!(
            source.kind(),
            io::ErrorKind::BrokenPipe | io::ErrorKind::NotFound | io::ErrorKind::NotConnected
        );
        if terminal {
            Self::Disconnected {
                link,
                detail: source.to_string(),
            }
        } else {
            Self::Retryable { link, source }
        }
    }

    /// End of file on a character device that should never end.
    #[must_use]
    pub fn eof(link: LinkKind) -> Self {
        Self::Disconnected {
            link,
            detail: "read returned 0 bytes; the device node has gone".to_owned(),
        }
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        match self {
            Self::Disconnected { link, .. } | Self::Retryable { link, .. } => *link,
        }
    }

    /// True when the caller must escalate rather than retry.
    #[must_use]
    pub const fn is_disconnected(&self) -> bool {
        matches!(self, Self::Disconnected { .. })
    }

    /// True when one more attempt inside the caller's budget is legitimate.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(self, Self::Retryable { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_units::ZoneId;

    const Z1: LinkKind = LinkKind::Zone(ZoneId::Zone1);

    #[test]
    fn both_links_are_9600_8n1_from_the_protocol_constants() {
        let saturn = LineSettings::for_link(Z1);
        let steam = LineSettings::for_link(LinkKind::Steam);
        assert_eq!(saturn.baud, kdtv_proto::saturn::BAUD);
        assert_eq!(steam.baud, kdtv_proto::dtv::BAUD);
        assert_eq!((saturn.data_bits, saturn.stop_bits), (8, 1));
        assert_eq!((steam.data_bits, steam.stop_bits), (8, 1));
        assert_eq!(saturn.to_string(), "9600 8N1");
    }

    /// The distinction the whole error type exists for.
    #[test]
    fn enumeration_loss_is_terminal_and_eintr_is_not() {
        for errno in [
            libc::ENODEV,
            libc::ENXIO,
            libc::EIO,
            libc::EBADF,
            libc::EPIPE,
        ] {
            let e = LinkIoError::classify(Z1, io::Error::from_raw_os_error(errno));
            assert!(
                e.is_disconnected(),
                "errno {errno} was treated as retryable"
            );
            assert!(!e.is_retryable());
            assert_eq!(e.link(), Z1);
        }
        for errno in [libc::EINTR, libc::EAGAIN, libc::ETIMEDOUT] {
            let e = LinkIoError::classify(Z1, io::Error::from_raw_os_error(errno));
            assert!(e.is_retryable(), "errno {errno} was treated as terminal");
            assert!(!e.is_disconnected());
        }
    }

    #[test]
    fn a_zero_length_read_is_disconnection_not_an_empty_frame() {
        let e = LinkIoError::eof(LinkKind::Steam);
        assert!(e.is_disconnected());
        assert!(e.to_string().contains("steam"));
    }

    #[test]
    fn only_the_serial_backend_is_gated() {
        assert!(Backend::Serial.is_real_bus());
        assert!(!Backend::Pty.is_real_bus());
        assert!(!Backend::Loopback.is_real_bus());
        assert_eq!(Backend::Pty.to_string(), "pty");
    }
}
