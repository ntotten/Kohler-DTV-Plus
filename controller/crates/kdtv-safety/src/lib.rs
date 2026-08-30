//! Where water is authorised, and where it is stopped.
//!
//! This is the second thing a reviewer should read, after `kdtv-units`. It has no
//! I/O, no clock and no configuration: time arrives as a parameter and the
//! resolved bounds arrive when the kernel is built, so a safety decision cannot
//! quietly depend on how a file was parsed or on when a test happened to run.
//!
//! Four properties are structural rather than checked, because a check can be
//! forgotten and a type cannot:
//!
//! 1. **One way to open water.** [`OpenGrant`] has no public constructor.
//!    [`SafetyKernel::authorize_open`] is its only source, so "what can turn
//!    water on" is answered by reading one function.
//!
//! 2. **Escalation scope is decided at compile time.** [`SafetyEvent::scope`] is
//!    an exhaustive match, so a new fault variant does not compile until someone
//!    has decided whether it takes down one zone or everything.
//!
//! 3. **Stopping cannot be done by halves.** [`ZoneAuthority`] owns its link by
//!    value and [`ZoneAuthority::latch`] consumes it into a variant with nowhere
//!    to put it, so the port closes on drop. "All-off sent but port left open" is
//!    not a state this type can be in.
//!
//! 4. **A session cannot be extended.** [`SessionDeadline`] exposes only whether
//!    it has expired and how long remains. There is no `extend`, no `refresh` and
//!    no setter, on the type or anywhere else.
//!
//! # What this crate does not do
//!
//! It does not close anything. It returns [`Effect`]s describing what must
//! happen, and the service performs them. That is what lets the whole escalation
//! matrix be tested at its real constants in microseconds, with no hardware and
//! no waiting.
//!
//! It is also not the physical backstop. The valve's own communication-loss
//! shutdown is, and that is measured at commissioning rather than assumed here —
//! no number this crate produces is evidence about a valve.

// Tests legitimately panic on a broken invariant; the production lints stay on
// for library code.
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

pub mod authority;
pub mod event;
pub mod grant;
pub mod kernel;
pub mod session;

pub use authority::{IsLatched, ZoneAuthority};
pub use event::{DegradeReason, Effect, FaultScope, FindingClass, LatchReason, SafetyEvent};
pub use grant::{Denial, OpenGrant, OperatorAck, StartAuthorization, ValidatedStart};
pub use kernel::{Bounds, LinkState, SafetyKernel};
pub use session::SessionDeadline;
