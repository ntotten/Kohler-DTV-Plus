//! The independent temperature channels: one PT1000 on a MAX31865 per zone.
//!
//! **The sensor has no authority to open an outlet.** Its only output is a
//! safety event. Nothing in this module, and nothing that reads it, maps a
//! reading to an authorisation.
//!
//! A surface clamp is not an immersion measurement: it reads pipe wall, lags by
//! seconds, and reads low. The optional correction is characterised at
//! commissioning against an immersion probe, and every threshold is evaluated on
//! the corrected value. With no `correction` the channel is uncorrected — which
//! invents no offset, and is why [`kdtv_units::RAW_TRIP_C`] exists.

use crate::error::ConfigError;
use kdtv_units::{OffsetCurve, RawC, ZoneId};

/// One RTD channel.
#[derive(Clone, PartialEq, Debug)]
pub struct SensorConfig {
    zone: ZoneId,
    chip_select: String,
    correction: OffsetCurve,
    corrected: bool,
}

impl SensorConfig {
    pub(crate) fn build(
        zone: ZoneId,
        chip_select: String,
        points: Option<Vec<(f64, f64)>>,
    ) -> Result<Self, ConfigError> {
        if chip_select.trim().is_empty() {
            return Err(ConfigError::ChipSelectEmpty { zone });
        }
        let (correction, corrected) = match points {
            None => (OffsetCurve::uncorrected(), false),
            Some(points) => {
                let measured: Vec<(RawC, RawC)> = points
                    .into_iter()
                    .map(|(surface, immersion)| (raw_c(surface), raw_c(immersion)))
                    .collect();
                let curve = OffsetCurve::from_commissioning(&measured)
                    .map_err(|source| ConfigError::Curve { zone, source })?;
                (curve, true)
            }
        };
        Ok(Self {
            zone,
            chip_select,
            correction,
            corrected,
        })
    }

    #[must_use]
    pub const fn zone(&self) -> ZoneId {
        self.zone
    }

    /// The SPI chip-select this channel's amplifier answers on, as the kernel
    /// names it — `spi0.0`, `spi0.1`.
    #[must_use]
    pub fn chip_select(&self) -> &str {
        &self.chip_select
    }

    /// The commissioned surface-to-immersion correction, applied before any
    /// threshold is evaluated.
    #[must_use]
    pub const fn correction(&self) -> &OffsetCurve {
        &self.correction
    }

    /// False when the file gave no correction. The channel still produces a
    /// [`kdtv_units::CorrectedC`], because the thresholds are typed on it —
    /// but the curve is the identity, so the reading is the pipe wall's and
    /// the absolute raw trip is the only backstop under it.
    #[must_use]
    pub const fn is_characterised(&self) -> bool {
        self.corrected
    }
}

#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "f64 has no checked narrowing to f32. TOML carries floats as f64 and \
              the amplifier's own reading is f32; a commissioning temperature in \
              degrees Celsius has no f64 precision to lose at f32"
)]
fn raw_c(v: f64) -> RawC {
    RawC(v as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_without_a_curve_is_uncorrected() {
        let s = SensorConfig::build(ZoneId::Zone2, "spi0.1".to_owned(), None).unwrap();
        assert_eq!(s.zone(), ZoneId::Zone2);
        assert_eq!(s.chip_select(), "spi0.1");
        assert!(!s.is_characterised());
        // The identity: 40 C in, 40 C out. No offset is invented.
        assert!((s.correction().correct(RawC(40.0)).celsius() - 40.0).abs() < 0.001);
    }

    #[test]
    fn a_commissioned_curve_corrects_upward() {
        let s = SensorConfig::build(
            ZoneId::Zone1,
            "spi0.0".to_owned(),
            Some(vec![(33.0, 35.0), (42.0, 45.0)]),
        )
        .unwrap();
        assert!(s.is_characterised());
        assert!(s.correction().correct(RawC(33.0)).celsius() > 34.9);
        assert!(s.correction().correct(RawC(42.0)).celsius() > 44.9);
    }

    /// A curve that subtracts is a miswired probe or a swapped column, and it
    /// would make a hot outlet look cooler — the one direction of error this
    /// chain exists to catch. `kdtv_units` refuses it; this test proves the
    /// refusal reaches the operator with the zone attached.
    #[test]
    fn a_downward_curve_is_refused_with_the_zone_named() {
        let err = SensorConfig::build(
            ZoneId::Zone1,
            "spi0.0".to_owned(),
            Some(vec![(35.0, 33.0), (45.0, 42.0)]),
        )
        .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("sensors.zone1.correction"), "{text}");
        assert!(text.contains("downward"), "{text}");
    }

    #[test]
    fn a_one_point_curve_is_refused() {
        let err = SensorConfig::build(ZoneId::Zone2, "spi0.1".to_owned(), Some(vec![(33.0, 35.0)]))
            .unwrap_err();
        assert!(err.to_string().contains("at least two"), "{err}");
    }

    #[test]
    fn an_empty_chip_select_is_refused() {
        assert!(matches!(
            SensorConfig::build(ZoneId::Zone1, "  ".to_owned(), None),
            Err(ConfigError::ChipSelectEmpty { .. })
        ));
    }
}
