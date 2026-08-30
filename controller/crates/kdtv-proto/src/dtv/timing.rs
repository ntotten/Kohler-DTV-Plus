//! DTV+ link timing.
//!
//! Every figure here is tier `[C]` and none has been measured on this
//! installation. Where the sources disagree, **both values are plumbed and
//! neither is declared correct** — picking one would be exactly the unmarked
//! inference `AGENT.md` rule 4 forbids. `CORRECTIONS.md` item 5.
//!
//! # The tick is unresolved, and the default is the slow one
//!
//! `steam-generator.md` § Device Configuration and `timing-constants.md` give
//! `STEAM_TICK_TIME` = **150 ms**. `dtv-plus-protocol.md` § Timing Parameters
//! gives a generic Port Tick of **500 ms** as the main polling interval per
//! port. `STEAM-ADAPTER.md` § 10.5 records the disagreement and does not settle
//! it.
//!
//! At 150 ms a steam port carries 3.3× the transactions of a valve port at
//! 525 ms. This system's one confirmed failure was caused by polling a
//! controller faster than it could take — `INVESTIGATIONS.md` I1, closed
//! 2026-08-29 — so the default here is the **slower** figure, 500 ms, with 150 ms
//! as a compile-time floor rather than a target. Both candidates are carried and
//! the effective rate is meant to be logged. Resolving it is a measurement of the
//! wall interface's own link, not a decision.
//!
//! # There is no echo timeout in this struct
//!
//! `dtv-plus-protocol.md` § Timing Parameters lists `RS485 Echo Timeout` =
//! 150 ms and its port state machine has a `WAIT_ECHO` state.
//! `HARDWARE-SPEC.md` § 6 already marks the DTV+ echo timeout **n/a**. The stock
//! master waits for its own transmission to return on the half-duplex bus; the
//! converters chosen for this build (Waveshare `USB TO RS485/422`, SKU 23949)
//! have automatic direction control and present **no local echo at all**, so
//! there is no signal to wait for. No echo constant, no `WAIT_ECHO` state, and
//! the decoder does not require one. Direction is inferred from the opcode.
//! `CORRECTIONS.md` item 3, `STEAM-07`, `PROTO-04` / `PROTO-06`.
//!
//! **150 ms appears in this module anyway**, as [`DtvTimings::TICK_FAST`]. That
//! is a numeric coincidence: `STEAM_TICK_TIME` and `RS485_ECHO_TIMEOUT` are two
//! different constants that happen to share a value, and only the first is
//! plumbed here. The Saturn module can assert "no field holds an echo figure"
//! because its echo constants are 20, 70 and 150 ms and none of its own figures
//! collide; this one cannot, so it asserts on names instead.
//!
//! That non-applicability is a property of the selected part. **If the converter
//! is ever substituted for one with local echo, the 150 ms echo figure comes back
//! into force and the platform decision in `HARDWARE-SPEC.md` § 2 must be
//! revisited.**

use core::time::Duration;

/// The complete timing set for the DTV+ link.
///
/// Configuration, not constants — the whole point is that a Phase 5 capture can
/// change these without touching the codec. [`DtvTimings::DOCUMENTED`] is the
/// starting position.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct DtvTimings {
    /// The polling interval this link actually runs at.
    ///
    /// **Unresolved `[?]`.** One of the two candidates below; defaults to the
    /// slower. Never below [`DtvTimings::TICK_FLOOR`].
    pub tick: Duration,
    /// Candidate A: `STEAM_TICK_TIME`, 150 ms.
    pub tick_candidate_fast: Duration,
    /// Candidate B: the generic per-port tick, 500 ms.
    pub tick_candidate_slow: Duration,

    /// `Device Reply Timeout`. How long to wait for an answer to a command.
    pub reply: Duration,
    /// `Address Enquiry Timeout`. How long to wait for a device to answer
    /// `DEV_ADDRESS_OPP`. Distinct from [`DtvTimings::reply`] in every source.
    pub address_enquiry_timeout: Duration,

    /// Retries for one command before the fault latches.
    ///
    /// **Unresolved `[?]`.** `steam-generator.md` § Recovery Logic says the
    /// controller "retries the failed command up to 4 times", after which the
    /// error is permanent; `dtv-plus-protocol.md` § Retry Logic and
    /// `timing-constants.md`'s `DEVICE_MAX_RETRIES` both say 5. It is an
    /// off-by-one that changes exactly when a fault latches. Defaults to **4**,
    /// the lower bound, which latches sooner — the safer direction for a device
    /// that heats a room. `CORRECTIONS.md` item 5, `STEAM-ADAPTER.md` § 10.7.
    pub retries: u8,
    /// The alternative reading of [`DtvTimings::retries`], carried so a
    /// configuration can select it without a code change.
    pub retries_alternate: u8,

    /// `TX Attempts (max)`. Cumulative transmit failures before the port itself
    /// is declared faulted. The hard-loss trigger in `STEAM-ADAPTER.md`'s
    /// two-tier link-loss rule.
    pub max_tx_failures: u16,

    /// The ceiling on one transaction: request, reply wait, and every retry.
    ///
    /// Four retries at a 300 ms reply deadline is 1.5 s against a 500 ms
    /// cadence, so the retry train can outrun the tick here exactly as it can on
    /// the Saturn bus. Overrun is a **link fault**, logged, rather than
    /// something that silently starves the scheduler. `CORRECTIONS.md` item 6.
    pub transaction_budget: Duration,
}

impl DtvTimings {
    /// `STEAM_TICK_TIME`. Candidate A, and the compile-time floor.
    pub const TICK_FAST: Duration = Duration::from_millis(150);
    /// The generic per-port tick. Candidate B, and the default.
    pub const TICK_SLOW: Duration = Duration::from_millis(500);
    /// No configuration may poll faster than this. Same number as
    /// [`DtvTimings::TICK_FAST`], because the faster candidate is the floor: a
    /// rate below the fastest figure any source states would be inventing one.
    pub const TICK_FLOOR: Duration = Self::TICK_FAST;
    /// `Device Reply Timeout`.
    pub const REPLY: Duration = Duration::from_millis(300);
    /// `Address Enquiry Timeout`.
    pub const ADDRESS_ENQUIRY_TIMEOUT: Duration = Duration::from_millis(400);
    /// `steam-generator.md` § Recovery Logic.
    pub const RETRIES_RECOVERY: u8 = 4;
    /// `dtv-plus-protocol.md` § Retry Logic, and `DEVICE_MAX_RETRIES`.
    pub const RETRIES_PROTOCOL: u8 = 5;
    /// **No configuration may exceed this.** The larger of the two candidate
    /// readings; anything above it is a number no source states.
    pub const RETRIES_MAX: u8 = Self::RETRIES_PROTOCOL;
    /// `TX Attempts (max)`.
    pub const MAX_TX_FAILURES: u16 = 250;

    /// The documented starting position. Every value tier `[C]`.
    pub const DOCUMENTED: Self = Self {
        tick: Self::TICK_SLOW,
        tick_candidate_fast: Self::TICK_FAST,
        tick_candidate_slow: Self::TICK_SLOW,
        reply: Self::REPLY,
        address_enquiry_timeout: Self::ADDRESS_ENQUIRY_TIMEOUT,
        retries: Self::RETRIES_RECOVERY,
        retries_alternate: Self::RETRIES_PROTOCOL,
        max_tx_failures: Self::MAX_TX_FAILURES,
        transaction_budget: Self::TICK_SLOW,
    };

    /// Selects the tick, refusing anything below [`DtvTimings::TICK_FLOOR`].
    ///
    /// The floor is the point: a configuration mistake that polls a fragile
    /// controller flat out is what caused `INVESTIGATIONS.md` I1.
    pub fn with_tick(self, tick: Duration) -> Result<Self, TimingError> {
        if tick < Self::TICK_FLOOR {
            return Err(TimingError::TickBelowFloor {
                requested_ms: millis(tick),
                floor_ms: millis(Self::TICK_FLOOR),
            });
        }
        Ok(Self { tick, ..self })
    }

    /// Selects the retry count, refusing anything above
    /// [`DtvTimings::RETRIES_MAX`].
    pub fn with_retries(self, retries: u8) -> Result<Self, TimingError> {
        if retries > Self::RETRIES_MAX {
            return Err(TimingError::RetriesAboveMaximum {
                requested: retries,
                max: Self::RETRIES_MAX,
            });
        }
        Ok(Self { retries, ..self })
    }

    /// The worst-case wall time of a full retry train: the first attempt plus
    /// [`DtvTimings::retries`] more, each waiting a full reply deadline.
    #[must_use]
    pub fn worst_case_transaction(&self) -> Duration {
        self.reply
            .saturating_mul(u32::from(self.retries).saturating_add(1))
    }

    /// True when the retry train cannot finish inside its budget.
    ///
    /// Not an error on its own — it is the condition the scheduler must handle
    /// by deferring remaining attempts to the next tick and logging the overrun,
    /// rather than blocking. `CORRECTIONS.md` item 6.
    #[must_use]
    pub fn retry_train_overruns_budget(&self) -> bool {
        self.worst_case_transaction() > self.transaction_budget
    }

    /// Line time for a frame at 9600 baud 8N1: ten bits per byte.
    #[must_use]
    pub fn line_time(frame_len: usize) -> Duration {
        let bits = u64::try_from(frame_len)
            .unwrap_or(u64::MAX)
            .saturating_mul(10);
        Duration::from_nanos(bits.saturating_mul(1_000_000_000) / 9600)
    }
}

impl Default for DtvTimings {
    fn default() -> Self {
        Self::DOCUMENTED
    }
}

fn millis(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum TimingError {
    #[error("a {requested_ms} ms tick is below the {floor_ms} ms floor")]
    TickBelowFloor { requested_ms: u64, floor_ms: u64 },
    #[error("{requested} retries exceeds the maximum of {max}")]
    RetriesAboveMaximum { requested: u8, max: u8 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_values_are_the_documented_values() {
        let t = DtvTimings::DOCUMENTED;
        assert_eq!(t.reply, Duration::from_millis(300));
        assert_eq!(t.address_enquiry_timeout, Duration::from_millis(400));
        assert_eq!(t.max_tx_failures, 250);
        assert_eq!(DtvTimings::default(), t);
    }

    /// `CORRECTIONS.md` item 5. Both tick readings are present, the configured
    /// value is one of exactly the two, and the default is the slower one.
    #[test]
    fn both_tick_candidates_are_plumbed_and_the_default_is_the_slow_one() {
        let t = DtvTimings::DOCUMENTED;
        assert_eq!(t.tick_candidate_fast, Duration::from_millis(150));
        assert_eq!(t.tick_candidate_slow, Duration::from_millis(500));
        assert!(
            t.tick == t.tick_candidate_fast || t.tick == t.tick_candidate_slow,
            "the configured tick must be one of the two candidates"
        );
        assert_eq!(t.tick, t.tick_candidate_slow);
        // The Saturn tick is a different bus and must not be folded in.
        assert_ne!(t.tick, Duration::from_millis(525));
    }

    /// The floor exists because I1 was caused by polling faster than the
    /// controller could take.
    #[test]
    fn the_tick_has_a_floor_and_selecting_the_fast_candidate_is_allowed() {
        let t = DtvTimings::DOCUMENTED;
        let fast = t.with_tick(DtvTimings::TICK_FAST).unwrap();
        assert_eq!(fast.tick, Duration::from_millis(150));
        assert_eq!(
            t.with_tick(Duration::from_millis(149)).unwrap_err(),
            TimingError::TickBelowFloor {
                requested_ms: 149,
                floor_ms: 150
            }
        );
        assert!(t.with_tick(Duration::from_millis(1000)).is_ok());
    }

    /// `CORRECTIONS.md` item 5 / `STEAM-ADAPTER.md` § 10.7. Both retry readings,
    /// defaulting to 4, and 5 is a hard ceiling.
    #[test]
    fn both_retry_counts_are_plumbed_and_five_is_the_ceiling() {
        let t = DtvTimings::DOCUMENTED;
        assert_eq!(t.retries, 4);
        assert_eq!(t.retries_alternate, 5);
        assert_eq!(DtvTimings::RETRIES_RECOVERY, 4);
        assert_eq!(DtvTimings::RETRIES_PROTOCOL, 5);
        // Selecting the other reading is a configuration change, not a code one.
        assert_eq!(t.with_retries(t.retries_alternate).unwrap().retries, 5);
        // And nothing may go above it.
        assert_eq!(
            t.with_retries(6).unwrap_err(),
            TimingError::RetriesAboveMaximum {
                requested: 6,
                max: 5
            }
        );
        assert!(t.with_retries(0).is_ok());
    }

    /// `CORRECTIONS.md` item 6. The arithmetic that motivates the budget.
    #[test]
    fn the_retry_train_can_outrun_the_tick_and_that_is_detected() {
        let d = DtvTimings::DOCUMENTED;
        // Four retries plus the first attempt, at 300 ms each.
        assert_eq!(d.worst_case_transaction(), Duration::from_millis(1500));
        assert!(d.retry_train_overruns_budget());

        // On the fast tick it is far worse: 1.5 s against 150 ms.
        let fast = d.with_tick(DtvTimings::TICK_FAST).unwrap();
        let fast = DtvTimings {
            transaction_budget: fast.tick,
            ..fast
        };
        assert!(fast.retry_train_overruns_budget());

        // A single attempt inside the tick does not overrun.
        let one = d.with_retries(0).unwrap();
        assert_eq!(one.worst_case_transaction(), Duration::from_millis(300));
        assert!(!one.retry_train_overruns_budget());
    }

    /// `CORRECTIONS.md` item 3. No echo field, no echo state, no echo method.
    ///
    /// Checked on **names**, not on values: 150 ms is plumbed here as
    /// `STEAM_TICK_TIME`, which happens to equal `RS485_ECHO_TIMEOUT`. A
    /// value-wise assertion would be a false alarm, in the same way a substring
    /// search for "20ms" matches "320ms".
    #[test]
    fn there_is_no_echo_timeout() {
        let rendered = format!("{:?}", DtvTimings::DOCUMENTED);
        assert!(
            !rendered.to_ascii_lowercase().contains("echo"),
            "an echo timeout has appeared in DtvTimings: {rendered}"
        );
        // The one 150 ms figure present is the tick candidate, and it is named
        // as such.
        assert!(rendered.contains("tick_candidate_fast"));
        assert_eq!(DtvTimings::TICK_FAST, Duration::from_millis(150));
    }

    #[test]
    fn line_time_is_computed_not_quoted() {
        // The longest steam frame the encoder builds is nine bytes: 9.375 ms.
        let nine = DtvTimings::line_time(9);
        assert!(nine >= Duration::from_micros(9_300) && nine <= Duration::from_micros(9_400));
        // Even the 42-byte wire maximum is well inside the reply deadline.
        assert!(DtvTimings::line_time(crate::dtv::MAX_FRAME) < DtvTimings::REPLY);
    }
}
