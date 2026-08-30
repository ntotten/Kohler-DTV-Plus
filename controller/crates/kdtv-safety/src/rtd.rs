//! The independent temperature chain's alarm logic.
//!
//! The sensor has **no authority to open an outlet**. Its only output type is a
//! [`SafetyEvent`], and there is no function anywhere in this workspace that
//! turns a reading into an [`crate::OpenGrant`].
//!
//! Every threshold is evaluated on the **corrected** value, except the absolute
//! raw backstop — which exists precisely because a correction curve can itself
//! be wrong, and a surface clamp reads low, so an error in the curve hides heat
//! rather than inventing it.
//!
//! Three of the five conditions need a dwell: a single sample over a threshold
//! is as likely to be noise as heat. The dwell is tracked here rather than by
//! the caller so that "for more than 2 seconds" means the same thing everywhere.

use crate::event::SafetyEvent;
use kdtv_telemetry::Monotonic;
use kdtv_units::{
    CORRECTED_TRIP_C, CORRECTED_TRIP_DWELL, CorrectedC, DIVERGENCE_DWELL, DIVERGENCE_LIMIT_C,
    OffsetCurve, RAW_TRIP_C, RTD_STARVATION, RawC, ZoneId,
};
use smallvec::{SmallVec, smallvec};

/// One reading from the amplifier.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct RtdSample {
    pub raw: RawC,
    /// The MAX31865 fault register. Any non-zero bit is a fault: RTD open or
    /// short, or a supply out of range.
    pub fault_register: u8,
    pub at: Monotonic,
}

/// Tracks one zone's independent channel and decides when it has seen enough.
#[derive(Debug)]
pub struct RtdWatch {
    zone: ZoneId,
    curve: OffsetCurve,
    last_sample: Option<Monotonic>,
    over_corrected_since: Option<Monotonic>,
    diverged_since: Option<Monotonic>,
}

impl RtdWatch {
    #[must_use]
    pub fn new(zone: ZoneId, curve: OffsetCurve) -> Self {
        Self {
            zone,
            curve,
            last_sample: None,
            over_corrected_since: None,
            diverged_since: None,
        }
    }

    #[must_use]
    pub fn correct(&self, raw: RawC) -> CorrectedC {
        self.curve.correct(raw)
    }

    /// Feed a sample and get whatever it proves.
    ///
    /// `valve_reported_c` is the valve's own thermistor reading, when there is a
    /// current one. It is only used for the divergence check — never as a
    /// substitute for the independent reading, which is the entire point of
    /// having a second sensor.
    ///
    /// `outlet_on` gates the corrected over-temperature check: an instrumented
    /// outlet that is closed is measuring standing water, not delivered water.
    /// The raw backstop is **not** gated, because a pipe above 50 °C with the
    /// outlet closed is a finding whatever caused it.
    pub fn observe(
        &mut self,
        sample: RtdSample,
        outlet_on: bool,
        valve_reported_c: Option<f32>,
    ) -> SmallVec<[SafetyEvent; 2]> {
        let mut out: SmallVec<[SafetyEvent; 2]> = smallvec![];
        self.last_sample = Some(sample.at);

        if sample.fault_register != 0 {
            // A faulted amplifier's reading means nothing, so nothing else in
            // this sample is evaluated.
            self.over_corrected_since = None;
            self.diverged_since = None;
            out.push(SafetyEvent::RtdFaultRegister {
                zone: self.zone,
                bits: sample.fault_register,
            });
            return out;
        }

        if sample.raw.0 >= RAW_TRIP_C {
            out.push(SafetyEvent::IndependentRawOverTemperature {
                zone: self.zone,
                raw: sample.raw,
            });
        }

        let corrected = self.curve.correct(sample.raw);
        if outlet_on && corrected.celsius() >= CORRECTED_TRIP_C {
            let since = *self.over_corrected_since.get_or_insert(sample.at);
            let dwell = sample.at.since(since);
            if dwell >= CORRECTED_TRIP_DWELL {
                out.push(SafetyEvent::IndependentOverTemperature {
                    zone: self.zone,
                    corrected,
                    dwell,
                });
            }
        } else {
            self.over_corrected_since = None;
        }

        if let Some(reported) = valve_reported_c {
            let delta = (corrected.celsius() - reported).abs();
            if delta > DIVERGENCE_LIMIT_C {
                let since = *self.diverged_since.get_or_insert(sample.at);
                let dwell = sample.at.since(since);
                if dwell >= DIVERGENCE_DWELL {
                    out.push(SafetyEvent::TemperatureDivergence {
                        zone: self.zone,
                        delta_c: delta,
                        dwell,
                    });
                }
            } else {
                self.diverged_since = None;
            }
        } else {
            self.diverged_since = None;
        }

        out
    }

    /// Check for starvation. Called on the service's own tick, because the
    /// absence of a sample cannot announce itself.
    pub fn check_starvation(&self, now: Monotonic) -> Option<SafetyEvent> {
        let last = self.last_sample?;
        let gap = now.since(last);
        (gap > RTD_STARVATION).then_some(SafetyEvent::RtdStarved {
            zone: self.zone,
            since: gap,
        })
    }

    /// A channel that has never produced a sample.
    ///
    /// Distinct from starvation: never having spoken is a startup condition, and
    /// the boot sequence refuses to reach ready without one.
    #[must_use]
    pub const fn has_never_sampled(&self) -> bool {
        self.last_sample.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Monotonic {
        Monotonic::from_nanos(ms * 1_000_000)
    }

    fn watch() -> RtdWatch {
        RtdWatch::new(ZoneId::Zone1, OffsetCurve::uncorrected())
    }

    fn sample(raw: f32, at_ms: u64) -> RtdSample {
        RtdSample {
            raw: RawC(raw),
            fault_register: 0,
            at: at(at_ms),
        }
    }

    #[test]
    fn a_single_hot_sample_is_not_enough() {
        let mut w = watch();
        let out = w.observe(sample(46.0, 0), true, None);
        assert!(
            out.is_empty(),
            "one sample over the trip is noise until it persists"
        );
    }

    #[test]
    fn two_seconds_over_the_corrected_trip_stops_the_zone() {
        let mut w = watch();
        assert!(w.observe(sample(46.0, 0), true, None).is_empty());
        assert!(w.observe(sample(46.0, 1_900), true, None).is_empty());
        let out = w.observe(sample(46.0, 2_000), true, None);
        assert!(
            matches!(
                out.first(),
                Some(SafetyEvent::IndependentOverTemperature { .. })
            ),
            "{out:?}"
        );
    }

    #[test]
    fn dropping_below_the_trip_restarts_the_dwell() {
        let mut w = watch();
        w.observe(sample(46.0, 0), true, None);
        w.observe(sample(40.0, 1_000), true, None);
        // Back over, but the clock restarts: 1500 ms is not 2 s.
        assert!(w.observe(sample(46.0, 1_500), true, None).is_empty());
        assert!(w.observe(sample(46.0, 3_000), true, None).is_empty());
        let out = w.observe(sample(46.0, 3_600), true, None);
        assert!(!out.is_empty(), "the dwell completes 2 s after re-crossing");
    }

    #[test]
    fn the_corrected_check_is_gated_on_the_outlet_but_the_raw_backstop_is_not() {
        let mut w = watch();
        // Outlet closed: no corrected trip even after a long dwell.
        w.observe(sample(46.0, 0), false, None);
        assert!(w.observe(sample(46.0, 10_000), false, None).is_empty());

        // But a pipe above the raw backstop reports regardless, with no dwell.
        let out = w.observe(sample(51.0, 11_000), false, None);
        assert!(
            matches!(
                out.first(),
                Some(SafetyEvent::IndependentRawOverTemperature { .. })
            ),
            "{out:?}"
        );
    }

    #[test]
    fn a_faulted_amplifier_suppresses_every_other_judgement() {
        let mut w = watch();
        let out = w.observe(
            RtdSample {
                raw: RawC(60.0),
                fault_register: 0x04,
                at: at(0),
            },
            true,
            Some(20.0),
        );
        assert_eq!(
            out.len(),
            1,
            "a faulted reading proves only that it is faulted: {out:?}"
        );
        assert!(matches!(
            out.first(),
            Some(SafetyEvent::RtdFaultRegister { bits: 0x04, .. })
        ));
    }

    #[test]
    fn divergence_needs_ten_seconds_and_a_valve_reading_to_compare_against() {
        let mut w = watch();
        // No valve reading: nothing to diverge from.
        assert!(w.observe(sample(40.0, 0), true, None).is_empty());
        // 6 C apart, but not yet for long enough.
        assert!(w.observe(sample(40.0, 1_000), true, Some(34.0)).is_empty());
        assert!(w.observe(sample(40.0, 9_000), true, Some(34.0)).is_empty());
        let out = w.observe(sample(40.0, 11_100), true, Some(34.0));
        assert!(
            matches!(out.first(), Some(SafetyEvent::TemperatureDivergence { .. })),
            "{out:?}"
        );
    }

    #[test]
    fn agreeing_again_clears_the_divergence_dwell() {
        let mut w = watch();
        w.observe(sample(40.0, 0), true, Some(34.0));
        w.observe(sample(40.0, 5_000), true, Some(39.0));
        assert!(w.observe(sample(40.0, 12_000), true, Some(34.0)).is_empty());
    }

    #[test]
    fn starvation_is_measured_from_the_last_sample() {
        let mut w = watch();
        assert!(w.has_never_sampled());
        w.observe(sample(38.0, 0), true, None);
        assert!(!w.has_never_sampled());
        assert!(w.check_starvation(at(4_000)).is_none());
        assert!(matches!(
            w.check_starvation(at(6_000)),
            Some(SafetyEvent::RtdStarved { .. })
        ));
    }

    #[test]
    fn a_channel_that_never_spoke_does_not_report_starvation() {
        // It reports has_never_sampled instead, which the boot sequence acts on.
        assert!(watch().check_starvation(at(60_000)).is_none());
    }

    #[test]
    fn a_correcting_curve_moves_the_trip_where_it_belongs() {
        // A clamp reading 3 C low: 43 C at the pipe is 46 C in the water.
        let curve =
            OffsetCurve::from_commissioning(&[(RawC(30.0), RawC(33.0)), (RawC(43.0), RawC(46.0))])
                .unwrap();
        let mut w = RtdWatch::new(ZoneId::Zone1, curve);
        w.observe(sample(43.0, 0), true, None);
        let out = w.observe(sample(43.0, 2_100), true, None);
        assert!(
            !out.is_empty(),
            "a raw 43 C corrects to 46 C, which is over the trip — the uncorrected \
             value would have passed"
        );
    }
}
