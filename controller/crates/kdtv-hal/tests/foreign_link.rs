//! `Link` must be implementable outside `kdtv-hal`.
//!
//! This file is a separate crate, which is the whole point: it can reach only
//! the public API, so it fails to compile under exactly the condition being
//! guarded against. A copy of it inside `src/` would compile against
//! `pub(crate)` items and prove nothing.
//!
//! [`Link`](kdtv_hal::Link)'s own documentation says it is object-safe "so that
//! the same engine drives a real converter, a pseudo-terminal and the
//! emulator's pipe", and `Backend::Loopback` is documented as having no
//! implementation in that crate. Neither was true: `LinkDescriptor`'s only
//! constructor was crate-private and `Link::descriptor` returns one, so the
//! trait could not be implemented anywhere else. `kdtv-service` had to route its
//! tests around a `Link` it could not fake.

// An integration test is its own crate, so `lib.rs`'s `cfg_attr(test, ...)`
// header does not reach it and the workspace lints apply in full. Same allow as
// `kdtv-config/tests/deploy_files.rs`, for the same reason.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use kdtv_hal::{
    Backend, BoxedFuture, EmulatedBackend, LineSettings, Link, LinkDescriptor, LinkIoError,
};
use kdtv_proto::FixtureSet;
use kdtv_proto::gate::TransmitAuthority;
use kdtv_units::{LinkKind, ZoneId};
use std::path::PathBuf;

const Z1: LinkKind = LinkKind::Zone(ZoneId::Zone1);

#[derive(Debug)]
struct ForeignLink(LinkDescriptor);

impl Link for ForeignLink {
    fn write_all<'a>(&'a mut self, _buf: &'a [u8]) -> BoxedFuture<'a, Result<(), LinkIoError>> {
        Box::pin(async { Ok(()) })
    }
    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxedFuture<'a, Result<usize, LinkIoError>> {
        Box::pin(async move {
            match buf.first_mut() {
                Some(b) => {
                    *b = 0xAA;
                    Ok(1)
                }
                None => Ok(0),
            }
        })
    }
    fn descriptor(&self) -> &LinkDescriptor {
        &self.0
    }
    fn close(self: Box<Self>) -> BoxedFuture<'static, Result<(), LinkIoError>> {
        Box::pin(async { Ok(()) })
    }
}

fn emulator_only() -> TransmitAuthority {
    TransmitAuthority::emulator_only(FixtureSet::embedded())
}

#[test]
fn a_link_can_be_implemented_outside_kdtv_hal() {
    let d = LinkDescriptor::emulated(
        Z1,
        EmulatedBackend::Pty,
        PathBuf::from("/dev/pts/7"),
        "pts/7".to_owned(),
        &emulator_only(),
    )
    .expect("a pseudo-terminal is not a real bus");
    let link: Box<dyn Link> = Box::new(ForeignLink(d));
    assert_eq!(link.descriptor().backend(), Backend::Pty);
    assert_eq!(link.descriptor().line(), LineSettings::for_link(Z1));
}

#[test]
fn an_emulated_descriptor_records_the_authority_it_was_opened_under() {
    let d = LinkDescriptor::emulated(
        LinkKind::Steam,
        EmulatedBackend::Loopback,
        PathBuf::from("loopback"),
        "loopback".to_owned(),
        &emulator_only(),
    )
    .expect("a loopback is not a real bus");
    assert_eq!(d.backend(), Backend::Loopback);
    // The question a support transcript asks first, answerable from the
    // descriptor rather than from a configuration file that may since have
    // changed.
    assert_eq!(d.authority().scope(), "emulator-only");
    assert!(!d.backend().is_real_bus());
}

/// There is no `EmulatedBackend::Serial`, so this constructor cannot describe a
/// real bus whatever authority it is handed. That is the denial — by absence of
/// a variant, not by a runtime check that could be got round.
#[test]
fn the_emulated_constructor_has_no_route_to_a_real_bus() {
    for b in [EmulatedBackend::Pty, EmulatedBackend::Loopback] {
        assert!(
            !b.backend().is_real_bus(),
            "{b:?} must not be a gated backend"
        );
    }
}
