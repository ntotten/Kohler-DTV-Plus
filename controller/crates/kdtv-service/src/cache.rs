//! The state an external reader gets, and the swap that hands it over.
//!
//! # `API-06` is the reason this module exists
//!
//! "Homebridge and Worker status reads use the service cache; external callers
//! cannot trigger an extra valve transaction." That is not a performance
//! preference. A month of investigation went into a phantom valve fault that
//! turned out to be this project's own client polling the K-99695 until its web
//! server hung (`INVESTIGATIONS.md` I1). The replacement must not be able to
//! regrow that behaviour, and the way it cannot is that a read has nowhere to go
//! but memory.
//!
//! [`StateCache`] is an [`arc_swap::ArcSwap`]. A reader takes an
//! [`std::sync::Arc`] out of it and is done: no channel, no lock the supervisor
//! could be holding, no path to a link. A hundred thousand reads a second change
//! the transmitted frame count by zero, which is what
//! `a_status_read_storm_transmits_nothing` asserts.
//!
//! # What is in a snapshot
//!
//! The engine's own caches ([`ZoneCache`], [`SteamCache`]) verbatim, plus the
//! one thing the engine cannot know: what the safety kernel believes about each
//! link.

use std::sync::Arc;

use arc_swap::ArcSwap;
use kdtv_engine::{SteamCache, ZoneCache};
use kdtv_safety::{LatchReason, LinkState};
use kdtv_telemetry::Stamp;
use kdtv_units::{BootId, PiBootId, ZoneId};
use serde::Serialize;

/// What the safety kernel believes about one link, in a form that serialises.
///
/// [`LinkState`] is not `Serialize` and should not become so to satisfy one
/// caller; this is the projection, and it carries the latch reason because "why"
/// is the only useful part of "unavailable".
#[derive(Clone, PartialEq, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum LinkStateLabel {
    Cold,
    Ready,
    Running,
    Latched {
        reason: LatchReason,
        acknowledged: bool,
    },
}

impl LinkStateLabel {
    pub(crate) fn of(state: &LinkState) -> Self {
        match state {
            LinkState::Cold => Self::Cold,
            LinkState::Ready => Self::Ready,
            LinkState::Running => Self::Running,
            LinkState::Latched {
                reason,
                acknowledged,
            } => Self::Latched {
                reason: reason.clone(),
                acknowledged: *acknowledged,
            },
        }
    }

    /// True while the link is unavailable, whether or not anyone has
    /// acknowledged it. Acknowledging is not recovering.
    #[must_use]
    pub const fn is_latched(&self) -> bool {
        matches!(self, Self::Latched { .. })
    }
}

/// One zone, as an external reader sees it.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct ZoneStatus {
    pub zone: ZoneId,
    /// What the safety kernel believes. Distinct from `valve.phase`, which is
    /// what the zone machine believes; they agree except in the instant between
    /// an escalation and the machine being stepped to match it.
    pub kernel: LinkStateLabel,
    /// The engine's own cache, unaltered.
    pub valve: ZoneCache,
    /// Frames that reached the wire on this link since boot.
    pub frames_tx: u64,
    /// Frames decoded off this link since boot, decode failures included.
    pub frames_rx: u64,
}

/// The steam link, as an external reader sees it.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct SteamStatus {
    pub kernel: LinkStateLabel,
    pub adapter: SteamCache,
    pub frames_tx: u64,
    pub frames_rx: u64,
}

/// Everything the service knows, at one moment, without touching a bus.
#[derive(Clone, PartialEq, Debug, Serialize)]
pub struct SystemSnapshot {
    /// The Linux kernel's boot id, so a reader can tell a service restart from
    /// a reboot.
    pub pi_boot: PiBootId,
    pub service_boot: BootId,
    pub zones: Vec<ZoneStatus>,
    /// `None` when `steam.enabled = false`. A link that is not driven is not
    /// opened and has nothing to report.
    pub steam: Option<SteamStatus>,
    /// True once a stop has been commanded on every link and the service is
    /// waiting for the confirmation. Commands are refused from here on.
    pub shutting_down: bool,
    pub as_of: Stamp,
}

impl SystemSnapshot {
    /// The zone's entry, if this snapshot has one.
    #[must_use]
    pub fn zone(&self, id: ZoneId) -> Option<&ZoneStatus> {
        self.zones.iter().find(|z| z.zone == id)
    }

    /// Total frames transmitted across every link since boot.
    ///
    /// The number `API-06` is asserted against: a read storm must leave it
    /// exactly where it was.
    #[must_use]
    pub fn frames_tx(&self) -> u64 {
        let zones: u64 = self.zones.iter().map(|z| z.frames_tx).sum();
        zones.saturating_add(self.steam.as_ref().map_or(0, |s| s.frames_tx))
    }

    /// True when no link has an outlet commanded open and no water may still be
    /// moving.
    #[must_use]
    pub fn all_off(&self) -> bool {
        self.zones.iter().all(|z| !z.valve.water_moving)
            && self.steam.as_ref().is_none_or(|s| !s.adapter.steaming)
    }

    pub(crate) fn empty(pi_boot: PiBootId, service_boot: BootId, at: Stamp) -> Self {
        Self {
            pi_boot,
            service_boot,
            zones: Vec::new(),
            steam: None,
            shutting_down: false,
            as_of: at,
        }
    }
}

/// The published snapshot, swapped whole.
///
/// A reader never sees a half-updated system: the supervisor builds the next
/// snapshot and swaps the pointer, so a status read is one atomic load and a
/// clone of an [`std::sync::Arc`].
#[derive(Debug)]
pub struct StateCache {
    current: ArcSwap<SystemSnapshot>,
}

impl StateCache {
    pub(crate) fn new(initial: SystemSnapshot) -> Self {
        Self {
            current: ArcSwap::from_pointee(initial),
        }
    }

    /// The current snapshot. **Touches no channel and no link.** `API-06`.
    #[must_use]
    pub fn load(&self) -> Arc<SystemSnapshot> {
        self.current.load_full()
    }

    pub(crate) fn store(&self, next: SystemSnapshot) {
        self.current.store(Arc::new(next));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_telemetry::{Monotonic, NtpSync};

    fn stamp() -> Stamp {
        Stamp::new(
            Monotonic::from_nanos(1),
            1_756_500_000,
            NtpSync::Synchronised,
        )
    }

    fn snapshot() -> SystemSnapshot {
        SystemSnapshot::empty(PiBootId("boot-uuid".into()), BootId(1), stamp())
    }

    #[test]
    fn a_read_returns_the_last_published_snapshot_and_nothing_else() {
        let cache = StateCache::new(snapshot());
        let first = cache.load();
        assert_eq!(first.service_boot, BootId(1));

        let mut next = snapshot();
        next.shutting_down = true;
        cache.store(next);

        // The Arc handed out earlier is unchanged: a reader holding a snapshot
        // holds that snapshot, not a window onto live state.
        assert!(!first.shutting_down);
        assert!(cache.load().shutting_down);
    }

    #[test]
    fn a_latched_label_carries_the_reason_and_survives_serialisation() {
        let label = LinkStateLabel::of(&LinkState::Latched {
            reason: LatchReason::PortLost,
            acknowledged: false,
        });
        assert!(label.is_latched());
        let json = serde_json::to_string(&label).unwrap();
        assert!(json.contains("port_lost"), "{json}");
        assert!(!LinkStateLabel::of(&LinkState::Ready).is_latched());
    }

    #[test]
    fn an_empty_snapshot_reports_nothing_transmitted_and_everything_off() {
        let s = snapshot();
        assert_eq!(s.frames_tx(), 0);
        assert!(s.all_off());
        assert!(s.zone(ZoneId::Zone1).is_none());
    }

    /// `LOG-09`, over the snapshot envelope only.
    ///
    /// [`ZoneCache`] and [`SteamCache`] have no public constructor, so nothing
    /// here can build a populated snapshot; what this covers is the four fields
    /// of [`SystemSnapshot`] itself. The same assertion over a real snapshot —
    /// both zones, their engine caches and their kernel labels — is in
    /// `crate::tests`, driven off the running supervisor, and
    /// that is the one that would catch a credential-shaped field added to
    /// [`ZoneStatus`].
    #[test]
    fn a_serialised_snapshot_envelope_carries_no_credential_shaped_field() {
        let json = serde_json::to_string(&snapshot()).unwrap();
        for word in ["token", "secret", "password", "credential", "pairing"] {
            assert!(!json.contains(word), "{word} appears in {json}");
        }
    }
}
