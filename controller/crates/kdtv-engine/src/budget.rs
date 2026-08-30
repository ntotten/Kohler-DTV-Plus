//! The explicit bound on one bus transaction.
//!
//! Both links have the same problem and it is worth stating once. On the Saturn
//! bus, three retries at a 400 ms response deadline is 1.2 s against a 525 ms
//! cadence; on the DTV+ link, four retries at a 300 ms reply deadline is 1.5 s
//! against a 500 ms one. In both cases the retry train can outrun the tick.
//!
//! `kdtv-proto` reports the condition — `Timings::retry_train_overruns_budget`
//! and its DTV+ twin — and says what to do about it: defer the remaining
//! attempts to the next tick and log the overrun rather than blocking. This type
//! is the other half, the bound that says when deferring has gone on long
//! enough. `CORRECTIONS.md` item 6.
//!
//! Two numbers, because there are two failure modes:
//!
//! - `attempts` caps how many times one operation is sent.
//! - `ceiling` caps how long the whole train may occupy in wall time, which is
//!   what a link that answers slowly but never usefully would otherwise consume.
//!
//! Exceeding either is a **link fault**, logged as one, not a silently starved
//! scheduler.

use kdtv_proto::dtv::DtvTimings;
use kdtv_proto::saturn::Timings;
use std::time::Duration;

/// How much one transaction may cost.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct RetryBudget {
    /// Total sends, including the first.
    pub attempts: u8,
    /// The longest one transaction may take, first send to last timeout.
    pub ceiling: Duration,
}

impl RetryBudget {
    /// One attempt per tick, for as many ticks as there are attempts.
    ///
    /// The derivation is deliberate rather than a round number: retries are
    /// issued on a tick and never sooner, so a train of `attempts` sends
    /// occupies `attempts` ticks and no more. A link that needs longer than that
    /// is not slow, it is broken.
    #[must_use]
    pub fn from_saturn(t: &Timings) -> Self {
        let attempts = t.retries.saturating_add(1);
        Self {
            attempts,
            ceiling: t.tick.saturating_mul(u32::from(attempts)),
        }
    }

    /// The same derivation for the DTV+ link.
    #[must_use]
    pub fn from_dtv(t: &DtvTimings) -> Self {
        let attempts = t.retries.saturating_add(1);
        Self {
            attempts,
            ceiling: t.tick.saturating_mul(u32::from(attempts)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_documented_saturn_train_does_not_fit_one_tick_and_the_budget_says_so() {
        let t = Timings::DOCUMENTED;
        // kdtv-proto already reports the overrun; this crate's job is to bound
        // it rather than to argue with it.
        assert!(t.retry_train_overruns_budget());
        let b = RetryBudget::from_saturn(&t);
        assert_eq!(b.attempts, 4);
        assert_eq!(b.ceiling, Duration::from_millis(2100));
        // Four attempts at the 320 ms response deadline is 1280 ms, which fits
        // inside the ceiling — the ceiling is what a *slower* failure hits.
        assert!(t.worst_case_transaction() < b.ceiling);
    }

    #[test]
    fn the_documented_dtv_train_gets_the_same_treatment() {
        let t = DtvTimings::DOCUMENTED;
        let b = RetryBudget::from_dtv(&t);
        assert_eq!(b.attempts, 5);
        assert_eq!(b.ceiling, Duration::from_millis(2500));
    }
}
