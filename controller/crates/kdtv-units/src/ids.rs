//! Identifiers shared across the workspace.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A valve zone. This installation has two, each on its own isolated bus.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ZoneId {
    /// The six-port valve, firmware `0.12`, five configured outlets.
    Zone1,
    /// The three-port Prompt valve, firmware `0.14`, three configured outlets.
    Zone2,
}

impl ZoneId {
    pub const ALL: [Self; 2] = [Self::Zone1, Self::Zone2];

    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Zone1 => 0,
            Self::Zone2 => 1,
        }
    }
}

impl fmt::Display for ZoneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zone1 => f.write_str("zone1"),
            Self::Zone2 => f.write_str("zone2"),
        }
    }
}

/// One of the three serial links. Each has its own converter, its own isolation
/// barrier, and its own state machine.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkKind {
    /// A Saturn valve bus.
    Zone(ZoneId),
    /// The DTV+ link to the K-1737-K1 steam adapter.
    Steam,
}

impl LinkKind {
    pub const ALL: [Self; 3] = [
        Self::Zone(ZoneId::Zone1),
        Self::Zone(ZoneId::Zone2),
        Self::Steam,
    ];

    #[must_use]
    pub const fn zone(self) -> Option<ZoneId> {
        match self {
            Self::Zone(z) => Some(z),
            Self::Steam => None,
        }
    }
}

impl fmt::Display for LinkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zone(z) => write!(f, "{z}"),
            Self::Steam => f.write_str("steam"),
        }
    }
}

/// A **configuration slot** number — the numbering the public API speaks.
///
/// One of three numbering schemes that do not agree, and the only one that
/// crosses this system's public boundary. See `CONTROLLER-DESIGN.md`
/// § Outlet index spaces:
///
/// | Space | Where it appears |
/// | --- | --- |
/// | Configuration slot | `one_type`..`six_type`, `valveN_outletM_func`, `quick_shower.cgi` digits |
/// | Status index | `system_info.cgi`'s `valveNoutletM` booleans |
/// | Saturn wire bitmap | the bytes sent to the valve; differs per valve type |
///
/// Slots are 1-based: 1..=6 for the six-port valve, 1..=3 for the Prompt 3.
/// Translation to a wire bitmap lives in `kdtv-proto`, against a table, with a
/// regression test that permutes a slot — because the slot-to-status mapping is
/// the identity on this system, and an identity-only test does not exercise the
/// code path that dereferenced a null in a shipped Hubitat driver
/// (`research/FIELD-NOTES.md` § 2).
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Slot(u8);

impl Slot {
    /// The largest slot any supported valve exposes.
    pub const MAX: u8 = 6;

    /// Rejects 0 and anything above six. Whether a slot is *configured* is a
    /// separate question, answered by the outlet table in `kdtv-config`.
    pub fn new(n: u8) -> Result<Self, SlotError> {
        if n == 0 || n > Self::MAX {
            return Err(SlotError::OutOfRange { requested: n });
        }
        Ok(Self(n))
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Zero-based index, for table lookups.
    #[must_use]
    #[expect(
        clippy::as_conversions,
        reason = "const fn cannot use From; `new` guarantees 1..=6 so this cannot underflow"
    )]
    pub const fn index(self) -> usize {
        // `new` guarantees 1..=6, so the subtraction cannot underflow and the
        // widening cannot truncate.
        (self.0 - 1) as usize
    }
}

impl fmt::Debug for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Slot({})", self.0)
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum SlotError {
    #[error("outlet slot {requested} is outside 1..=6")]
    OutOfRange { requested: u8 },
}

/// A set of configuration slots, as a small bitset over slots 1..=6.
///
/// Order-independent and duplicate-free by construction, which matters because
/// `outlet_set` reaches the API as a list.
#[derive(Copy, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SlotSet(u8);

impl SlotSet {
    pub const EMPTY: Self = Self(0);

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub fn insert(mut self, s: Slot) -> Self {
        self.0 |= 1 << s.index();
        self
    }

    #[must_use]
    pub fn contains(self, s: Slot) -> bool {
        self.0 & (1 << s.index()) != 0
    }

    #[must_use]
    pub fn len(self) -> u32 {
        self.0.count_ones()
    }

    /// Every slot in the set, ascending.
    pub fn iter(self) -> impl Iterator<Item = Slot> {
        (1..=Slot::MAX).filter_map(move |n| {
            let s = Slot::new(n).ok()?;
            self.contains(s).then_some(s)
        })
    }

    /// True when every slot in `self` is also in `allowed`.
    #[must_use]
    pub fn is_subset_of(self, allowed: Self) -> bool {
        self.0 & !allowed.0 == 0
    }

    /// The slots in `self` that are not in `allowed`.
    #[must_use]
    pub fn difference(self, allowed: Self) -> Self {
        Self(self.0 & !allowed.0)
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }
}

impl FromIterator<Slot> for SlotSet {
    fn from_iter<I: IntoIterator<Item = Slot>>(iter: I) -> Self {
        iter.into_iter().fold(Self::EMPTY, Self::insert)
    }
}

impl fmt::Debug for SlotSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter().map(Slot::get)).finish()
    }
}

/// The Linux kernel's boot id, so a log can tell a service restart from a reboot.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PiBootId(pub String);

/// Monotonically increasing across service starts, persisted and `fsync`ed
/// before it is issued, so a crash can only skip ids forward — never reuse one.
///
/// Every authorisation to open water is bound to the boot id that minted it, so
/// a restart invalidates outstanding tokens and a start cannot be replayed.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BootId(pub u64);

/// Monotonically increasing within a service boot. Every command carries one,
/// and every log line about that command repeats it.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CommandId(pub u64);

/// An authenticated API session. Opaque; never logged.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub u64);

/// Permission to open water on a zone.
///
/// This trait exists to solve a layering problem without giving up the property
/// it protects. The right to open water is minted by the safety kernel in
/// `kdtv-safety`, but the frame that opens an outlet is built in `kdtv-proto`,
/// and neither crate may depend on the other: a wire codec that depends on the
/// safety kernel has the layering upside down, and a safety kernel that depends
/// on a codec would let a change to a wire format reach the thing that decides
/// whether water may move.
///
/// So the capability is named here, in the crate they both already depend on.
/// `kdtv-proto` requires one to encode an outlet-opening frame; `kdtv-safety`
/// implements it for the grant its kernel mints.
///
/// # It is deliberately not sealed, and that is a real limitation
///
/// A sealed trait could only be implemented in this crate, which would defeat
/// the point — `kdtv-safety` has to implement it. So in principle another crate
/// could write its own implementation and forge authority.
///
/// What that buys is still worth having. Forgetting a check is invisible;
/// writing `impl OpenAuthority for MyThing` is a deliberate, greppable,
/// reviewable act. The audit in `cargo xtask audit-graph` asserts there is
/// exactly one implementation in the workspace and that it lives in
/// `kdtv-safety`, so a second one fails the build rather than passing review.
pub trait OpenAuthority {
    /// The zone this authority permits, and only this zone.
    fn authorised_zone(&self) -> ZoneId;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_rejects_zero_and_seven() {
        assert!(Slot::new(0).is_err());
        assert!(Slot::new(7).is_err());
        assert!(Slot::new(1).is_ok());
        assert!(Slot::new(6).is_ok());
    }

    #[test]
    fn slot_set_is_order_independent_and_deduplicating() {
        let a: SlotSet = [1, 3, 3]
            .iter()
            .filter_map(|n| Slot::new(*n).ok())
            .collect();
        let b: SlotSet = [3, 1].iter().filter_map(|n| Slot::new(*n).ok()).collect();
        assert_eq!(a, b);
        assert_eq!(a.len(), 2);
    }

    #[test]
    fn subset_check_names_the_offending_slots() {
        let configured: SlotSet = [1, 2, 5]
            .iter()
            .filter_map(|n| Slot::new(*n).ok())
            .collect();
        let asked: SlotSet = [1, 4].iter().filter_map(|n| Slot::new(*n).ok()).collect();
        assert!(!asked.is_subset_of(configured));
        let extra: Vec<u8> = asked.difference(configured).iter().map(Slot::get).collect();
        assert_eq!(extra, vec![4]);
    }

    #[test]
    fn link_kinds_cover_three_links() {
        assert_eq!(LinkKind::ALL.len(), 3);
        assert_eq!(LinkKind::Steam.zone(), None);
        assert_eq!(LinkKind::Zone(ZoneId::Zone2).zone(), Some(ZoneId::Zone2));
    }
}
