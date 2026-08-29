//! The I/O boundary: every trait the service needs, and the Linux
//! implementations behind them.
//!
//! # What is here, and what is deliberately not
//!
//! This crate is the only place in the daemon's dependency graph that opens a
//! file descriptor, reads a clock or writes a sysfs attribute. Everything above
//! it — the link state machines, the safety kernel, the API — is sans-IO and
//! takes time as a parameter.
//!
//! **The fakes are not here.** `SimLink`, `SimClock`, `SimRtd` and the rest live
//! in `kdtv-emulator`, which `cargo xtask audit-graph` already excludes from the
//! daemon's dependency graph. That placement is the mechanism: there is no
//! feature flag on this crate that could leak a fake into production, because
//! there is no fake in this crate to gate.
//!
//! What *is* here that is not a device is [`DirSysfs`] — a read-mostly view of a
//! directory tree that answers the four questions the port resolver asks. It
//! holds no port, opens no fd and transmits nothing; it lets the resolver's
//! refusals be tested on an `x86_64` runner with no hardware attached.
//!
//! # Safety posture
//!
//! **No frame in this workspace has been verified against the real hardware.**
//! Everything the encoders produce is tier `[C]`. This crate is the transmit
//! gate's *second* boundary: [`LinkFactory::open`] refuses a real serial backend
//! unless [`kdtv_proto::TransmitAuthority::permits_real_bus_on`] returns true for
//! that link.
//! Gating only the encoder would leave a real port open with a real
//! `SerialStream` behind it, relying on nothing ever writing bytes from another
//! source. See [`mod@factory`].
//!
//! Every failure path ends OFF. Nothing here persists a water state; the only
//! thing [`IdStore`] writes to disk is a counter, and [`Link::close`] consumes
//! the link so a port that has been closed is not a value anyone still holds.
//!
//! # Denial by absence
//!
//! - [`PortPath`](kdtv_config::PortPath) has no variant that can hold
//!   `/dev/ttyUSB0`, so an enumeration-order binding never reaches this crate.
//! - [`Hardened`] has no variant meaning "hardening was skipped on a real
//!   bridge": an FTDI port carries its read-back latency value, a pseudo-terminal
//!   says so, and there is no third case.
//! - There is no `GpioOut` trait, no GPIO dependency and no mains path. See
//!   [`NO_GPIO_OUTPUT`].

// Tests legitimately panic on a broken invariant; the production lints stay on
// for library code, where a panic is a fault, not a diagnosis.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp
    )
)]

pub mod clock;
pub mod factory;
pub mod ids;
pub mod latency;
pub mod link;
pub mod resolve;
pub mod rtd;
pub mod sysfs;
pub mod watchdog;

pub use clock::{Clock, LinuxClock, NtpProbe, TimesyncdProbe, WallClock};
pub use factory::{AuthorityRecord, LinkFactory, LinuxLinkFactory, OpenError, permit_open};
pub use ids::{FileIdStore, IdError, IdStore, RESERVATION_BLOCK};
pub use latency::{
    FTDI_DEFAULT_LATENCY_MS, Hardened, REQUIRED_LATENCY_MS, harden, harden_all,
};
pub use link::{Backend, BoxedFuture, LineSettings, Link, LinkDescriptor, LinkIoError};
pub use resolve::{
    BridgeKind, PortBinding, ResolveError, ResolvedPort, UsbIdentity, resolve_distinct,
};
pub use rtd::{
    CS_FOR_ZONE, ChipSelect, EXPANSION_CS, FaultRegister, NO_GPIO_OUTPUT, RtdChannel, RtdError,
    RtdSample, chip_select_for,
};
pub use sysfs::{DirSysfs, RealSysfs, SysfsView, TtyCandidate};
pub use watchdog::{SystemdWatchdog, Watchdog};

#[cfg(test)]
mod tests;
