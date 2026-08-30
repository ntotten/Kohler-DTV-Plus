//! The read-only event stream. `SVC-05`.
//!
//! "The service exposes a local authenticated API **and a read-only event
//! stream**." Read-only is structural rather than enforced: a subscriber gets a
//! [`tokio::sync::broadcast::Receiver`], which has no method that sends
//! anything. There is no path from a subscriber back into the supervisor.
//!
//! # A slow subscriber cannot slow the bus
//!
//! The channel is bounded. A receiver that falls behind gets
//! [`tokio::sync::broadcast::error::RecvError::Lagged`] and is told how many
//! events it missed; the supervisor never blocks on it. That is the right
//! trade: a status client stalling the control loop is exactly the failure mode
//! this project already lived through once from the other side
//! (`INVESTIGATIONS.md` I1), and a dropped log line on a wedged subscriber is a
//! smaller loss than a late all-off.
//!
//! Nothing here is the durable log. `tracing` is, and [`crate::Recorder`] writes
//! to both. This stream is for a live client watching a shower run.

use std::sync::Arc;

use kdtv_safety::FindingClass;
use kdtv_telemetry::LogEvent;
use kdtv_units::{CommandId, LinkKind};
use serde::Serialize;

use crate::cache::SystemSnapshot;

/// How many events the stream holds for a subscriber that is behind.
///
/// Sized for the busiest second this service can have: three links at their
/// tick, a frame record each way, an RTD sample per zone and a session record
/// is well under a hundred. A subscriber that cannot keep up with that is not
/// going to be rescued by a deeper buffer.
pub const EVENT_CAPACITY: usize = 256;

/// A service lifecycle transition worth telling a client about.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    /// Every link is bound and the watchdog has been told the service is ready.
    Ready,
    /// A stop has been commanded on every link; the service is waiting for the
    /// confirmations.
    ShuttingDown,
    /// Every link confirmed itself off before the service exited.
    StoppedConfirmed,
    /// The grace period expired with at least one link unconfirmed. **The worst
    /// outcome this system has**, and it says so rather than reporting a clean
    /// stop.
    StoppedUnconfirmed,
}

/// One thing that happened, as a live subscriber sees it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceEvent {
    /// A line that also went to the durable log.
    Log(Box<LogEvent>),
    /// The whole system state, published after every pass of the control loop.
    ///
    /// Shared rather than cloned: a broadcast with several subscribers hands
    /// each of them the same snapshot. Serialised through the pointer, so the
    /// wire form is the snapshot itself.
    State(#[serde(serialize_with = "serialize_shared")] Arc<SystemSnapshot>),
    /// Something an operator must act on **physically** — a welded valve, a bus
    /// with two valves on it, a zone that will not confirm itself off. Carried
    /// separately from the log because it is the one class of message that is
    /// useless unless a person reads it.
    OperatorMessage {
        link: LinkKind,
        text: String,
    },
    /// A finding for the investigation log, from
    /// [`kdtv_safety::Effect::RecordFinding`].
    Finding {
        class: FindingClass,
        detail: String,
    },
    /// A command that was refused. Nothing was transmitted and no valve state
    /// changed — that is the distinction the design draws between invalid input
    /// and invalid wire data.
    Refused {
        command: CommandId,
        reason: String,
    },
    Lifecycle {
        what: Lifecycle,
        detail: String,
    },
}

/// Serialise what an [`Arc`] points at, rather than requiring `serde`'s `rc`
/// feature across the whole workspace for one field.
fn serialize_shared<S: serde::Serializer>(
    snapshot: &Arc<SystemSnapshot>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    SystemSnapshot::serialize(snapshot, serializer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_units::ZoneId;

    #[test]
    fn a_subscriber_that_falls_behind_is_told_so_rather_than_blocking_the_sender() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<ServiceEvent>(2);
        for n in 0..5u64 {
            // Every send succeeds. The supervisor is never blocked by a reader.
            tx.send(ServiceEvent::Refused {
                command: CommandId(n),
                reason: "test".into(),
            })
            .expect("a live receiver exists");
        }
        let err = rx.try_recv().expect_err("the receiver is behind");
        assert!(
            matches!(err, tokio::sync::broadcast::error::TryRecvError::Lagged(3)),
            "{err:?}"
        );
    }

    #[test]
    fn an_operator_message_serialises_with_the_link_it_names() {
        let e = ServiceEvent::OperatorMessage {
            link: LinkKind::Zone(ZoneId::Zone1),
            text: "remove valve power".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("operator_message"), "{json}");
        assert!(json.contains("zone1"), "{json}");
    }
}
