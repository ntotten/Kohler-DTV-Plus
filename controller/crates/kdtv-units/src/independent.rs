//! The independent outlet temperature: raw reading, commissioned correction,
//! and the thresholds evaluated against it.
//!
//! Every other temperature in this system is the valve's own thermistor
//! self-report. Per `DISCLAIMER.md` that is not a measurement. This chain — a
//! PT1000 on a MAX31865, clamped to the outlet pipe — is the only independent
//! one, and it has **no authority to open an outlet**. Its only output is a
//! safety event.
//!
//! A surface clamp is not an immersion measurement: it reads pipe wall, lags by
//! seconds, and reads low. [`RawC`] and [`CorrectedC`] are separate types so
//! that "evaluate every threshold on the corrected value" is structural rather
//! than a convention someone can forget.

use serde::{Deserialize, Serialize};

/// A reading as the amplifier reported it, before the commissioned correction.
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RawC(pub f32);

/// A reading with the commissioned surface-clamp offset applied.
///
/// Private field: the only way to obtain one is [`OffsetCurve::correct`], so a
/// raw value cannot be compared against a corrected threshold by accident.
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug, Serialize)]
#[serde(transparent)]
pub struct CorrectedC(f32);

impl CorrectedC {
    #[must_use]
    pub const fn celsius(self) -> f32 {
        self.0
    }
}

/// Water above this scalds. Documentation only — it is never used as a bound,
/// because the controller's configured maximum sits below it by design.
pub const SCALD_C: f32 = 43.0;

/// Corrected outlet temperature above this, for [`CORRECTED_TRIP_DWELL`], with
/// the instrumented outlet on, stops that zone and latches it.
///
/// 45.0 °C sits above the 42.5 °C setpoint ceiling and above the 43 °C scald
/// threshold, with margin for sensor lag. It is a fault threshold, not a
/// comfort limit.
pub const CORRECTED_TRIP_C: f32 = 45.0;
pub const CORRECTED_TRIP_DWELL: std::time::Duration = std::time::Duration::from_secs(2);

/// Raw reading above this stops the zone regardless of any correction. The
/// backstop for a correction curve that is itself wrong.
pub const RAW_TRIP_C: f32 = 50.0;

/// Corrected reading versus the valve's own reported temperature differing by
/// more than this, for [`DIVERGENCE_DWELL`], stops the zone, latches it, and is
/// recorded as an I5-class finding.
pub const DIVERGENCE_LIMIT_C: f32 = 5.0;
pub const DIVERGENCE_DWELL: std::time::Duration = std::time::Duration::from_secs(10);

/// No RTD sample for longer than this stops the zone and latches it.
pub const RTD_STARVATION: std::time::Duration = std::time::Duration::from_secs(5);

// The ordering between these thresholds is part of the safety argument, so it is
// checked when the crate compiles rather than when a test runs. Editing one of
// the constants above out of order is a build failure, not a red test.
const _: () = assert!(
    CORRECTED_TRIP_C > SCALD_C,
    "the corrected trip must sit above the scald threshold, with margin for lag"
);
const _: () = assert!(
    CORRECTED_TRIP_C > 42.5,
    "the corrected trip must sit above the 42.5 C setpoint ceiling"
);
const _: () = assert!(
    RAW_TRIP_C > CORRECTED_TRIP_C,
    "the uncorrected backstop must sit above the corrected trip"
);

/// The commissioned correction from pipe-surface reading to delivered water
/// temperature, characterised against an immersion probe across the working
/// range and applied before any threshold is evaluated.
///
/// Piecewise linear between measured points, flat outside them. Extrapolating a
/// clamp offset past the range it was characterised over would be inventing
/// data; holding the end correction is the conservative choice and is recorded
/// as such in the commissioning report.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct OffsetCurve {
    /// `(surface reading, immersion reference)`, ascending by surface reading.
    points: Vec<(f32, f32)>,
}

#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum CurveError {
    #[error("a correction curve needs at least two characterised points, got {got}")]
    TooFewPoints { got: usize },
    #[error("characterised points must ascend by surface reading; {at} does not")]
    NotAscending { at: usize },
    #[error("a characterised point is not a finite number at index {at}")]
    NotFinite { at: usize },
    #[error(
        "the curve corrects downward at index {at}: a surface clamp reads low, so a \
         correction that subtracts is a miswired probe or a swapped column"
    )]
    CorrectsDownward { at: usize },
}

impl OffsetCurve {
    /// Build from commissioning measurements.
    ///
    /// Rejects a curve that corrects *downward*. A surface clamp reads low, so
    /// every characterised point must have the immersion reference at or above
    /// the surface reading. A curve that subtracts would make a hot outlet look
    /// cooler, which is the one direction of error this chain exists to catch.
    pub fn from_commissioning(points: &[(RawC, RawC)]) -> Result<Self, CurveError> {
        if points.len() < 2 {
            return Err(CurveError::TooFewPoints { got: points.len() });
        }
        let mut out: Vec<(f32, f32)> = Vec::with_capacity(points.len());
        for (i, (surface, reference)) in points.iter().enumerate() {
            if !surface.0.is_finite() || !reference.0.is_finite() {
                return Err(CurveError::NotFinite { at: i });
            }
            if reference.0 < surface.0 {
                return Err(CurveError::CorrectsDownward { at: i });
            }
            if let Some((prev, _)) = out.last()
                && *prev >= surface.0
            {
                return Err(CurveError::NotAscending { at: i });
            }
            out.push((surface.0, reference.0));
        }
        Ok(Self { points: out })
    }

    /// The identity curve, for a channel that has not been commissioned yet.
    ///
    /// Correct only in the sense that it does not invent a correction. A zone
    /// whose channel is uncommissioned still trips on [`RAW_TRIP_C`], which is
    /// why that threshold exists.
    #[must_use]
    pub fn uncorrected() -> Self {
        Self {
            points: vec![(0.0, 0.0), (100.0, 100.0)],
        }
    }

    /// Apply the correction. Flat outside the characterised range.
    #[must_use]
    pub fn correct(&self, raw: RawC) -> CorrectedC {
        let x = raw.0;
        if !x.is_finite() {
            // A non-finite reading is a sensor fault, handled by the fault
            // register path. Return something that trips rather than something
            // that looks safe.
            return CorrectedC(f32::INFINITY);
        }
        let Some(&(first_x, first_y)) = self.points.first() else {
            return CorrectedC(x);
        };
        if x <= first_x {
            return CorrectedC(x + (first_y - first_x));
        }
        let Some(&(last_x, last_y)) = self.points.last() else {
            return CorrectedC(x);
        };
        if x >= last_x {
            return CorrectedC(x + (last_y - last_x));
        }
        for pair in self.points.windows(2) {
            let [(x0, y0), (x1, y1)] = pair else { continue };
            if x >= *x0 && x <= *x1 {
                let span = x1 - x0;
                if span <= f32::EPSILON {
                    return CorrectedC(*y1);
                }
                let t = (x - x0) / span;
                return CorrectedC(t.mul_add(y1 - y0, *y0));
            }
        }
        CorrectedC(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> OffsetCurve {
        // A clamp reading 2 C low at 35 C and 3 C low at 45 C.
        OffsetCurve::from_commissioning(&[(RawC(33.0), RawC(35.0)), (RawC(42.0), RawC(45.0))])
            .expect("valid curve")
    }

    #[test]
    fn thresholds_are_the_documented_numbers() {
        assert!((CORRECTED_TRIP_C - 45.0).abs() < f32::EPSILON);
        assert!((RAW_TRIP_C - 50.0).abs() < f32::EPSILON);
        assert!((DIVERGENCE_LIMIT_C - 5.0).abs() < f32::EPSILON);
        assert_eq!(CORRECTED_TRIP_DWELL.as_secs(), 2);
        assert_eq!(DIVERGENCE_DWELL.as_secs(), 10);
        assert_eq!(RTD_STARVATION.as_secs(), 5);
    }

    #[test]
    fn correction_interpolates_between_characterised_points() {
        let c = curve();
        let mid = c.correct(RawC(37.5)).celsius();
        assert!(
            mid > 37.5,
            "a surface clamp reads low, so correction goes up"
        );
        assert!((mid - 40.0).abs() < 0.2, "got {mid}");
    }

    #[test]
    fn correction_is_flat_outside_the_characterised_range() {
        let c = curve();
        // Below: holds the +2.0 offset from the first point.
        assert!((c.correct(RawC(20.0)).celsius() - 22.0).abs() < 0.01);
        // Above: holds the +3.0 offset from the last point.
        assert!((c.correct(RawC(60.0)).celsius() - 63.0).abs() < 0.01);
    }

    #[test]
    fn a_downward_correction_is_rejected_as_a_miswired_probe() {
        let bad =
            OffsetCurve::from_commissioning(&[(RawC(40.0), RawC(35.0)), (RawC(50.0), RawC(45.0))]);
        assert!(matches!(bad, Err(CurveError::CorrectsDownward { .. })));
    }

    #[test]
    fn a_single_point_is_not_a_curve() {
        assert!(matches!(
            OffsetCurve::from_commissioning(&[(RawC(35.0), RawC(37.0))]),
            Err(CurveError::TooFewPoints { got: 1 })
        ));
    }

    #[test]
    fn a_non_finite_reading_corrects_to_something_that_trips() {
        assert!(curve().correct(RawC(f32::NAN)).celsius() > CORRECTED_TRIP_C);
    }

    #[test]
    fn an_uncommissioned_channel_invents_no_correction() {
        let c = OffsetCurve::uncorrected();
        assert!((c.correct(RawC(41.0)).celsius() - 41.0).abs() < 0.01);
    }
}
