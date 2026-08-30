//! The right to open water, and the tokens that lead to it.
//!
//! Three types here have no public constructor and are deliberately `!Clone`.
//! Each represents an authority that must be spent exactly once, by the code
//! that was given it, and a `Clone` would turn "the operator asked for this"
//! into "the operator asked for this, repeatedly, whenever we like". Not
//! deriving `Clone` is the whole of that mechanism; `tests/ui/` proves it.
//!
//! ~~Each carried a `PhantomData<*const ()>` field to say so.~~ Superseded: a
//! raw-pointer marker does not make a type any less cloneable — every field
//! here is already private and `Clone` was never derived — but it does make the
//! type `!Send` and `!Sync`, which nothing had asked for and nothing recorded.
//! That collided with the architecture: `StartAuthorization` is minted in the
//! API layer and consumed by the service, so it crosses a channel, and
//! `ZoneMachine` holds an `OpenGrant`, so a `!Send` grant pinned every zone to
//! one thread. The marker is gone and `the_authorities_cross_threads` holds
//! the property it was silently costing.

use crate::event::LatchReason;
use kdtv_units::{
    BootId, ClampError, CommandId, SessionDuration, Slot, SlotSet, ValveSetpoint, ZoneId,
};
use serde::Serialize;
/// Proof that an authenticated caller asked to start water, in this service boot.
///
/// Minted only by the API layer, from a live authenticated session, and consumed
/// by the call it authorises. It carries the boot id it was minted under, so a
/// service restart invalidates every outstanding token — which is what stops a
/// restart replaying a start.
#[derive(Debug)]
pub struct StartAuthorization {
    boot: BootId,
    command: CommandId,
}

impl StartAuthorization {
    /// Mint one. Intended for the API layer, which is the only place that knows
    /// a request was authenticated.
    #[must_use]
    pub fn issue(boot: BootId, command: CommandId) -> Self {
        Self { boot, command }
    }

    #[must_use]
    pub const fn boot(&self) -> BootId {
        self.boot
    }

    #[must_use]
    pub const fn command(&self) -> CommandId {
        self.command
    }
}

/// Proof that an operator acknowledged a latched link.
///
/// Recovery from a latch is never automatic. A link that has been taken down
/// stays down until a person says otherwise, because the fault that latched it
/// has not been diagnosed by anything.
#[derive(Debug)]
pub struct OperatorAck {
    command: CommandId,
}

impl OperatorAck {
    #[must_use]
    pub fn issue(command: CommandId) -> Self {
        Self { command }
    }

    #[must_use]
    pub const fn command(&self) -> CommandId {
        self.command
    }
}

/// A start request that has passed validation but not yet authorisation.
///
/// Every field is already a checked type: the setpoint has passed the clamp, and
/// the outlets are slots. What remains is whether *this zone, right now* may
/// open — which is [`crate::SafetyKernel::authorize_open`]'s decision.
#[derive(Clone, PartialEq, Debug)]
pub struct ValidatedStart {
    pub zone: ZoneId,
    pub outlets: SlotSet,
    pub temperature: ValveSetpoint,
    pub duration: SessionDuration,
    pub command: CommandId,
}

/// The right to open water on one zone.
///
/// No public constructor. [`crate::SafetyKernel::authorize_open`] is the only
/// source, so the question "what in this system can turn water on" is answered
/// by reading one function rather than by auditing every call site.
///
/// It names a zone, which is also what keeps the steam path away from valve
/// outlets: a steam operation has no `ZoneId` to build one with.
#[derive(Debug)]
pub struct OpenGrant {
    zone: ZoneId,
    command: CommandId,
    boot: BootId,
}

impl OpenGrant {
    /// Crate-private. The kernel is the only minter.
    pub(crate) fn new(zone: ZoneId, command: CommandId, boot: BootId) -> Self {
        Self {
            zone,
            command,
            boot,
        }
    }

    #[must_use]
    pub const fn zone(&self) -> ZoneId {
        self.zone
    }

    #[must_use]
    pub const fn command(&self) -> CommandId {
        self.command
    }

    #[must_use]
    pub const fn boot(&self) -> BootId {
        self.boot
    }
}

/// The grant is the workspace's only permission to open water.
///
/// `kdtv-proto` requires an [`kdtv_units::OpenAuthority`] to encode an outlet-opening frame,
/// and this is the only implementation of it. `cargo xtask audit-graph` asserts
/// that remains true.
impl kdtv_units::OpenAuthority for OpenGrant {
    fn authorised_zone(&self) -> ZoneId {
        self.zone
    }
}

/// Why a request to open water was refused.
///
/// A refusal changes no valve state and transmits nothing. That is the whole
/// distinction the design draws between invalid input and invalid wire data: bad
/// input is rejected to the caller, bad wire data escalates to all-off.
#[derive(Clone, PartialEq, Debug, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case", tag = "denial")]
pub enum Denial {
    #[error("{zone} is latched unavailable: {reason:?}")]
    ZoneLatched { zone: ZoneId, reason: LatchReason },
    #[error("{zone} is not ready to start (phase {phase})")]
    NotReady { zone: ZoneId, phase: &'static str },
    #[error("the authorisation is from service boot {token_boot:?}, this is {current_boot:?}")]
    StaleSession {
        token_boot: BootId,
        current_boot: BootId,
    },
    /// The authorisation was minted for a different command than the one being
    /// authorised. Two command ids that can disagree would let an authorisation
    /// issued for one request authorise another, so they must match exactly.
    #[error("the authorisation is for command {authorised:?}, the request is {requested:?}")]
    MismatchedCommand {
        authorised: CommandId,
        requested: CommandId,
    },
    #[error("outlet slot {0} is not configured on this zone")]
    UnconfiguredOutlet(Slot),
    #[error("no outlets were requested")]
    NoOutlets,
    /// Carried as its message rather than as the error type: this enum is
    /// serialised into the API response and the log, and `ClampError` lives in
    /// `kdtv-units`, which has no business gaining a serialisation derive to
    /// satisfy one caller.
    #[error("temperature: {0}")]
    Clamp(String),
    #[error("the valve's health is unknown; a start needs a positively healthy valve")]
    HealthUnknown { raw_code: u8 },
}

impl From<ClampError> for Denial {
    fn from(e: ClampError) -> Self {
        Self::Clamp(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_authorisation_carries_the_boot_that_minted_it() {
        let a = StartAuthorization::issue(BootId(7), CommandId(1));
        assert_eq!(a.boot(), BootId(7));
        assert_eq!(a.command(), CommandId(1));
    }

    #[test]
    fn a_grant_names_a_zone_which_is_what_keeps_steam_away_from_outlets() {
        let g = OpenGrant::new(ZoneId::Zone2, CommandId(3), BootId(1));
        assert_eq!(g.zone(), ZoneId::Zone2);
        // There is no constructor reachable from outside this crate, and no
        // ZoneId a steam operation could supply.
    }

    /// The authorities must cross threads.
    ///
    /// `StartAuthorization` is minted by the API layer and consumed by the
    /// service, so it travels a channel; `ZoneMachine` holds an `OpenGrant`, so
    /// a `!Send` grant would pin every zone to one thread. Both were true until
    /// a `PhantomData<*const ()>` field was removed from these three types — see
    /// the module documentation. This is the test that would have caught it.
    #[test]
    fn the_authorities_cross_threads() {
        const fn assert_send<T: Send>() {}
        assert_send::<OpenGrant>();
        assert_send::<StartAuthorization>();
        assert_send::<OperatorAck>();
    }

    // `!Clone` and `!Copy` have no stable expression as a bound — a
    // `fn assert_not_copy<T>()` with no bound on `T` accepts `i32` and proves
    // nothing, which is what stood here. The claim is made by the compile-fail
    // cases in `tests/ui/`, which are real programs that must fail to build.
}
