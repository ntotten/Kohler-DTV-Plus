//! Saturn link timing.
//!
//! Every figure here is tier `[C]` and none has been measured on this
//! installation. Where the sources disagree, **both values are plumbed and
//! neither is declared correct** — picking one would be exactly the unmarked
//! inference `AGENT.md` rule 4 forbids. `CORRECTIONS.md` item 5.
//!
//! # There is no echo timeout in this struct
//!
//! `HARDWARE.md` § 6 lists a 20 ms Saturn echo timeout, `timing-constants.md`
//! lists `VALVE_MESSAGE_ECHO_TIMEOUT` = 20 ms alongside a general 150 ms
//! `RS485_ECHO_TIMEOUT`, and the 525 ms cycle diagram allocates ~70 ms to "echo
//! clear". All three are moot here. The stock master waits for its own
//! transmission to return on the half-duplex bus; the converters chosen for this
//! build (Waveshare `USB TO RS485/422`, SKU 23949) have automatic direction
//! control and present **no local echo at all**, so there is no signal to wait
//! for. No echo constant, no echo-wait state, and the decoder does not require
//! one. `CORRECTIONS.md` item 3, `PROTO-04` / `PROTO-06`.
//!
//! That non-applicability is a property of the selected part. **If the converter
//! is ever substituted for one with local echo, all three figures come back into
//! force and the platform decision in `HARDWARE.md` § 2 must be
//! revisited.** The emulator can still inject echo so the `AA 55` resync can be
//! proven against echo bleed either way.

use core::time::Duration;

/// The complete timing set for one Saturn link.
///
/// Configuration, not constants — the whole point is that a Phase 1 capture can
/// change these without touching the codec. [`Timings::DOCUMENTED`] is the
/// starting position.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Timings {
    /// `VALVE_PORT_TICK_TIME`. One request per tick per port, no faster:
    /// faster yields no benefit and slower makes the valve assume loss of
    /// communication. `TIME-01`. All three sources agree on 525 ms.
    pub tick: Duration,

    /// The deadline the decoder actually enforces.
    ///
    /// **Unresolved.** `HARDWARE.md` § 6 lists "response timeout 400 ms"
    /// and "message timeout 320 ms" as two distinct parameters; § 15 open item 8
    /// states the open question as "Saturn response timeout: 320 ms or 400 ms",
    /// implying they may be one parameter reported twice. Defaults to the
    /// shorter of the two candidates so a decoder never waits past the tighter
    /// one. Both candidates are carried below.
    pub response: Duration,
    /// Candidate A for [`Timings::response`]: `saturn-protocol.md` § Timing
    /// Parameters, "Response Timeout".
    pub response_candidate_long: Duration,
    /// Candidate B for [`Timings::response`]: `VALVE_MESSAGE_TIMEOUT`, the
    /// maximum time for a complete valve message to arrive. `TIME-02`.
    pub response_candidate_short: Duration,

    /// `ADDRESS_ENQUIRY_TIMEOUT`. Distinct from [`Timings::response`] in every
    /// source, and not part of open item 8. `DISC-08`.
    pub address_enquiry_timeout: Duration,
    /// `ADDRESS_ENQUIRY_RATE_TIME`. No faster than this between enquiry
    /// broadcasts. `DISC-08`.
    pub address_enquiry_rate: Duration,
    /// `ADDRESS_CLEAR_DELAY_TIME`. Wait after the clear broadcast so it takes
    /// effect on every valve. `DISC-02`.
    ///
    /// Two of three references say 2000 ms; `timing-constants.md` also carries a
    /// per-device `DEVICE_ADDRESS_CLEAR_DELAY_TIME` of 1000 ms, which is
    /// labelled per-device and most likely belongs to the DTV+ bus. 2000 ms is
    /// used, and the extra second costs nothing because discovery runs only at
    /// startup.
    pub address_clear_delay: Duration,

    /// `STAGGER_TIME`. Solenoids are energised one at a time at this spacing
    /// rather than together, to avoid the inrush the sources attribute to relay
    /// damage. `OUT-05`.
    pub stagger: Duration,

    /// Retries for an ordinary read or write.
    ///
    /// **Unresolved.** `saturn-protocol.md` § Timing Parameters says 3 for read,
    /// write and address management, with 5 reserved for "critical" commands;
    /// `timing-constants.md` says `VALVE_MAX_RETRIES` = 5 for valve commands,
    /// with 3 applying specifically to address enquiries. Defaults to 3 — fewer
    /// frames on the wire and a faster fault declaration. Both are tested.
    pub retries: u8,
    /// The alternative reading of [`Timings::retries`], carried so a
    /// configuration can select it without a code change.
    pub retries_alternate: u8,
    /// Retries for an address enquiry. Both sources agree on 3. `DISC-07`.
    pub address_retries: u8,

    /// The ceiling on one transaction: request, response wait, and every retry.
    ///
    /// Three retries at a 400 ms response deadline is 1.2 s against a 525 ms
    /// cadence, so the retry train can outrun the tick. Overrun is a **link
    /// fault**, logged, rather than something that silently starves the
    /// scheduler. `CORRECTIONS.md` item 6.
    pub transaction_budget: Duration,
}

impl Timings {
    /// `VALVE_PORT_TICK_TIME`.
    pub const TICK: Duration = Duration::from_millis(525);
    /// Open item 8, candidate A.
    pub const RESPONSE_LONG: Duration = Duration::from_millis(400);
    /// Open item 8, candidate B. Also `VALVE_MESSAGE_TIMEOUT`.
    pub const RESPONSE_SHORT: Duration = Duration::from_millis(320);
    /// `ADDRESS_ENQUIRY_TIMEOUT`.
    pub const ADDRESS_ENQUIRY_TIMEOUT: Duration = Duration::from_millis(400);
    /// `ADDRESS_ENQUIRY_RATE_TIME`.
    pub const ADDRESS_ENQUIRY_RATE: Duration = Duration::from_millis(2000);
    /// `ADDRESS_CLEAR_DELAY_TIME`.
    pub const ADDRESS_CLEAR_DELAY: Duration = Duration::from_millis(2000);
    /// `STAGGER_TIME`.
    pub const STAGGER: Duration = Duration::from_millis(500);
    /// `saturn-protocol.md`'s read/write/address figure.
    pub const RETRIES_ROUTINE: u8 = 3;
    /// `timing-constants.md`'s `VALVE_MAX_RETRIES`, and `saturn-protocol.md`'s
    /// "critical" figure. The same number reached from two directions, which is
    /// why it is not obviously the wrong answer.
    pub const RETRIES_CRITICAL: u8 = 5;
    /// `MAX_ADDRESS_ENQUIRIES` / `MAX_MULTIDROP_ENQUIRIES`.
    pub const RETRIES_ADDRESS: u8 = 3;

    /// The documented starting position. Every value tier `[C]`.
    pub const DOCUMENTED: Self = Self {
        tick: Self::TICK,
        response: Self::RESPONSE_SHORT,
        response_candidate_long: Self::RESPONSE_LONG,
        response_candidate_short: Self::RESPONSE_SHORT,
        address_enquiry_timeout: Self::ADDRESS_ENQUIRY_TIMEOUT,
        address_enquiry_rate: Self::ADDRESS_ENQUIRY_RATE,
        address_clear_delay: Self::ADDRESS_CLEAR_DELAY,
        stagger: Self::STAGGER,
        retries: Self::RETRIES_ROUTINE,
        retries_alternate: Self::RETRIES_CRITICAL,
        address_retries: Self::RETRIES_ADDRESS,
        transaction_budget: Self::TICK,
    };

    /// The worst-case wall time of a full retry train: the first attempt plus
    /// [`Timings::retries`] more, each waiting a full response deadline.
    #[must_use]
    pub fn worst_case_transaction(&self) -> Duration {
        self.response
            .saturating_mul(u32::from(self.retries).saturating_add(1))
    }

    /// True when the retry train cannot finish inside its budget.
    ///
    /// Not an error on its own — it is the condition the scheduler must handle
    /// by deferring remaining attempts to the next tick and logging the
    /// overrun, rather than blocking. `CORRECTIONS.md` item 6.
    #[must_use]
    pub fn retry_train_overruns_budget(&self) -> bool {
        self.worst_case_transaction() > self.transaction_budget
    }

    /// Line time for a frame at 9600 baud 8N1: ten bits per byte.
    ///
    /// The 525 ms cycle diagram allocates "TX ~50 ms", which cannot be line time
    /// for any legal Saturn frame — the 20-byte maximum is 20.83 ms. That figure
    /// is a budget allocation inside the tick and is never used for a timeout.
    #[must_use]
    pub fn line_time(frame_len: usize) -> Duration {
        let bits = u64::try_from(frame_len)
            .unwrap_or(u64::MAX)
            .saturating_mul(10);
        Duration::from_nanos(bits.saturating_mul(1_000_000_000) / 9600)
    }
}

impl Default for Timings {
    fn default() -> Self {
        Self::DOCUMENTED
    }
}

/// Line settings for the Saturn bus. `PHY-01`.
pub const BAUD: u32 = 9600;
/// Data bits.
pub const DATA_BITS: u8 = 8;
/// Stop bits.
pub const STOP_BITS: u8 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_values_are_the_documented_values() {
        let t = Timings::DOCUMENTED;
        assert_eq!(t.tick, Duration::from_millis(525));
        assert_eq!(t.address_enquiry_timeout, Duration::from_millis(400));
        assert_eq!(t.address_enquiry_rate, Duration::from_millis(2000));
        assert_eq!(t.address_clear_delay, Duration::from_millis(2000));
        assert_eq!(t.stagger, Duration::from_millis(500));
        assert_eq!(t.address_retries, 3);
        assert_eq!(BAUD, 9600);
    }

    /// `CORRECTIONS.md` item 5. Both contradictory readings are present, and
    /// the configured value is one of exactly the two candidates.
    #[test]
    fn both_response_timeout_candidates_are_plumbed() {
        let t = Timings::DOCUMENTED;
        assert_eq!(t.response_candidate_long, Duration::from_millis(400));
        assert_eq!(t.response_candidate_short, Duration::from_millis(320));
        assert!(
            t.response == t.response_candidate_short || t.response == t.response_candidate_long,
            "the configured response deadline must be one of the two candidates"
        );
        // The shorter one, so no wait ever exceeds the tighter candidate.
        assert_eq!(t.response, t.response_candidate_short);
    }

    /// Both retry readings, defaulting to 3.
    #[test]
    fn both_retry_counts_are_plumbed_and_the_default_is_three() {
        let t = Timings::DOCUMENTED;
        assert_eq!(t.retries, 3);
        assert_eq!(t.retries_alternate, 5);
        assert_eq!(Timings::RETRIES_ROUTINE, 3);
        assert_eq!(Timings::RETRIES_CRITICAL, 5);
        // Selecting the other reading is a configuration change, not a code one.
        let five = Timings {
            retries: t.retries_alternate,
            ..t
        };
        assert_eq!(five.retries, 5);
    }

    /// `CORRECTIONS.md` item 6. The arithmetic that motivates the budget.
    #[test]
    fn the_retry_train_can_outrun_the_tick_and_that_is_detected() {
        // Three retries at the 400 ms candidate is 1.6 s of worst case against
        // a 525 ms tick.
        let long = Timings {
            response: Timings::RESPONSE_LONG,
            ..Timings::DOCUMENTED
        };
        assert_eq!(long.worst_case_transaction(), Duration::from_millis(1600));
        assert!(long.retry_train_overruns_budget());

        // The default is not immune either; the budget exists to make the
        // overrun a fault instead of a silent stall.
        let d = Timings::DOCUMENTED;
        assert_eq!(d.worst_case_transaction(), Duration::from_millis(1280));
        assert!(d.retry_train_overruns_budget());

        // A single attempt inside the tick does not overrun.
        let one = Timings {
            retries: 0,
            ..Timings::DOCUMENTED
        };
        assert_eq!(one.worst_case_transaction(), Duration::from_millis(320));
        assert!(!one.retry_train_overruns_budget());
    }

    /// No field, constant or method in this module mentions an echo.
    /// `CORRECTIONS.md` item 3.
    #[test]
    fn there_is_no_echo_timeout() {
        let rendered = format!("{:?}", Timings::DOCUMENTED);
        assert!(
            !rendered.to_ascii_lowercase().contains("echo"),
            "an echo timeout has appeared in Timings: {rendered}"
        );
        // And none of the three documented echo figures is present as a value.
        // Checked field-wise rather than by substring, because "320ms" contains
        // "20ms" and a substring test would have been a false alarm.
        let t = Timings::DOCUMENTED;
        for d in [
            t.tick,
            t.response,
            t.response_candidate_long,
            t.response_candidate_short,
            t.address_enquiry_timeout,
            t.address_enquiry_rate,
            t.address_clear_delay,
            t.stagger,
            t.transaction_budget,
        ] {
            for echo in [20u64, 70, 150] {
                assert_ne!(d, Duration::from_millis(echo), "an echo figure is plumbed");
            }
        }
    }

    /// The 50 ms transmit figure in the cycle diagram is not line time.
    #[test]
    fn line_time_is_computed_not_quoted() {
        // 20 bytes x 10 bits / 9600 = 20.83 ms.
        let max = Timings::line_time(20);
        assert!(max >= Duration::from_micros(20_800) && max <= Duration::from_micros(20_900));
        // A 6-byte frame is 6.25 ms, not 50.
        let short = Timings::line_time(6);
        assert!(short >= Duration::from_micros(6_200) && short <= Duration::from_micros(6_300));
        assert!(max < Duration::from_millis(50));
    }
}
