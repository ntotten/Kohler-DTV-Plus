//! The clamps. A setpoint outside them cannot be constructed.

use crate::temp::{Cx2, Fx2};
use serde::Serialize;

/// Which end of a clamp a request hit.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Bound {
    Floor,
    Ceiling,
    Step,
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum ClampError {
    #[error("{requested} is below the floor of {floor}")]
    BelowFloor { requested: i32, floor: i32 },
    #[error("{requested} is above the ceiling of {ceiling}")]
    AboveCeiling { requested: i32, ceiling: i32 },
    #[error("{requested} is not on the {step} step")]
    NotOnStep { requested: i32, step: &'static str },
}

/// What a clamp did, in the form the log requires.
///
/// Returned *with* the clamped value rather than alongside it, so a saturated
/// setpoint cannot be obtained without the record that explains it.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[non_exhaustive]
pub struct ClampRecord {
    pub requested: i32,
    pub emitted: i32,
    pub bound: Option<Bound>,
    pub reason: &'static str,
}

impl ClampRecord {
    #[must_use]
    pub const fn untouched(v: i32) -> Self {
        Self {
            requested: v,
            emitted: v,
            bound: None,
            reason: "within bounds",
        }
    }

    #[must_use]
    pub const fn was_clamped(&self) -> bool {
        self.bound.is_some()
    }
}

/// A valve temperature that has passed the clamp.
///
/// The field is private and there are exactly two ways in, so no other code can
/// produce one. Every encoder that writes a temperature takes this type, not a
/// bare [`Cx2`].
///
/// Bounds, from `CONTROLLER-DESIGN.md` § Safety boundary rule 6:
///
/// | Bound | Value | Source |
/// | --- | --- | --- |
/// | Ceiling | `Cx2 = 85` — 42.5 °C / 108.5 °F | the 109 °F user-facing limit, rounded down to the 0.5 °C step below it |
/// | Floor | `Cx2 = 60` — 30 °C / 86 °F | `MIN_SYS_VALVE_TEMP`; below it the valve returns *parameter out of range* |
///
/// The valve's own hardware ceiling is [`Cx2::MAX_WATER_TEMP`] — 49 °C — and is
/// never treated as a comfort limit. Water above 43 °C scalds.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize)]
#[serde(transparent)]
pub struct ValveSetpoint(Cx2);

impl ValveSetpoint {
    pub const FLOOR: Cx2 = Cx2::from_raw(60);
    pub const CEILING: Cx2 = Cx2::from_raw(85);

    /// The API path. A request outside the clamp is **rejected to the caller**
    /// and no valve state changes — it is not silently pulled to the nearest
    /// bound, because a caller asking for 120 °F should be told no rather than
    /// quietly given 108.5 °F.
    pub fn try_new(c: Cx2) -> Result<Self, ClampError> {
        if c < Self::FLOOR {
            return Err(ClampError::BelowFloor {
                requested: i32::from(c.raw()),
                floor: i32::from(Self::FLOOR.raw()),
            });
        }
        if c > Self::CEILING {
            return Err(ClampError::AboveCeiling {
                requested: i32::from(c.raw()),
                ceiling: i32::from(Self::CEILING.raw()),
            });
        }
        Ok(Self(c))
    }

    /// The encoder path — defence in depth, below the API's own rejection.
    ///
    /// Saturates rather than failing, because by the time a value reaches the
    /// encoder the alternative to a clamped setpoint is no setpoint at all. The
    /// [`ClampRecord`] it returns is what makes the clamp visible in the log.
    #[must_use]
    pub fn clamped(c: Cx2) -> (Self, ClampRecord) {
        let requested = i32::from(c.raw());
        if c < Self::FLOOR {
            let emitted = Self::FLOOR;
            return (
                Self(emitted),
                ClampRecord {
                    requested,
                    emitted: i32::from(emitted.raw()),
                    bound: Some(Bound::Floor),
                    reason: "below MIN_SYS_VALVE_TEMP",
                },
            );
        }
        if c > Self::CEILING {
            let emitted = Self::CEILING;
            return (
                Self(emitted),
                ClampRecord {
                    requested,
                    emitted: i32::from(emitted.raw()),
                    bound: Some(Bound::Ceiling),
                    reason: "above the configured ceiling",
                },
            );
        }
        (Self(c), ClampRecord::untouched(requested))
    }

    /// The value to put on the wire.
    #[must_use]
    pub const fn wire(self) -> Cx2 {
        self.0
    }

    #[must_use]
    pub fn celsius(self) -> f32 {
        self.0.celsius()
    }

    #[must_use]
    pub fn fahrenheit(self) -> f32 {
        self.0.fahrenheit()
    }
}

/// A steam temperature that has passed the clamp.
///
/// Bounds from `HARDWARE-SPEC.md` § 12: 90 °F to 125 °F in 1 °F steps, factory
/// default 110 °F. These are the generator's own documented envelope `[K]`. The
/// installer settings field `steam_max_temp` carries no `min`/`max` in the
/// shipped web interface, so it is treated as configuration and can only narrow
/// this range, never widen it.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize)]
#[serde(transparent)]
pub struct SteamSetpoint(Fx2);

impl SteamSetpoint {
    /// 90 °F.
    pub const FLOOR: Fx2 = Fx2::from_raw(180);
    /// 125 °F.
    pub const CEILING: Fx2 = Fx2::from_raw(250);
    /// 110 °F.
    pub const FACTORY_DEFAULT: Fx2 = Fx2::from_raw(220);

    pub fn try_new(f: Fx2) -> Result<Self, ClampError> {
        if !f.is_whole_degree() {
            return Err(ClampError::NotOnStep {
                requested: i32::from(f.raw()),
                step: "1 °F",
            });
        }
        if f < Self::FLOOR {
            return Err(ClampError::BelowFloor {
                requested: i32::from(f.raw()),
                floor: i32::from(Self::FLOOR.raw()),
            });
        }
        if f > Self::CEILING {
            return Err(ClampError::AboveCeiling {
                requested: i32::from(f.raw()),
                ceiling: i32::from(Self::CEILING.raw()),
            });
        }
        Ok(Self(f))
    }

    #[must_use]
    pub fn clamped(f: Fx2) -> (Self, ClampRecord) {
        let requested = i32::from(f.raw());
        // Round a half-degree down before the range check, so the two rules
        // compose in one order only.
        let stepped = Fx2::from_raw(f.raw() & !1);
        let (emitted, bound, reason) = if stepped < Self::FLOOR {
            (
                Self::FLOOR,
                Some(Bound::Floor),
                "below the generator's minimum operating setpoint",
            )
        } else if stepped > Self::CEILING {
            (
                Self::CEILING,
                Some(Bound::Ceiling),
                "above the generator's maximum setpoint",
            )
        } else if stepped == f {
            (f, None, "within bounds")
        } else {
            (
                stepped,
                Some(Bound::Step),
                "rounded down to a whole degree Fahrenheit",
            )
        };
        (
            Self(emitted),
            ClampRecord {
                requested,
                emitted: i32::from(emitted.raw()),
                bound,
                reason,
            },
        )
    }

    #[must_use]
    pub const fn wire(self) -> Fx2 {
        self.0
    }

    #[must_use]
    pub fn fahrenheit(self) -> f32 {
        self.0.fahrenheit()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valve_bounds_are_the_documented_numbers() {
        assert!((ValveSetpoint::FLOOR.celsius() - 30.0).abs() < f32::EPSILON);
        assert!((ValveSetpoint::CEILING.celsius() - 42.5).abs() < f32::EPSILON);
        assert!((ValveSetpoint::CEILING.fahrenheit() - 108.5).abs() < 0.01);
    }

    #[test]
    fn the_ceiling_sits_below_the_scald_threshold_plus_nothing() {
        // 42.5 C is below 43 C, which is where water begins to scald. The valve's
        // own ceiling of 49 C is far above it and is never used as a limit.
        assert!(ValveSetpoint::CEILING.celsius() < crate::independent::SCALD_C);
        assert!(Cx2::MAX_WATER_TEMP.celsius() > crate::independent::SCALD_C);
    }

    #[test]
    fn api_path_rejects_rather_than_clamping() {
        assert!(matches!(
            ValveSetpoint::try_new(Cx2::from_raw(98)),
            Err(ClampError::AboveCeiling { .. })
        ));
        assert!(matches!(
            ValveSetpoint::try_new(Cx2::from_raw(40)),
            Err(ClampError::BelowFloor { .. })
        ));
    }

    #[test]
    fn encoder_path_saturates_and_records_why() {
        let (v, rec) = ValveSetpoint::clamped(Cx2::from_raw(200));
        assert_eq!(v.wire(), ValveSetpoint::CEILING);
        assert_eq!(rec.bound, Some(Bound::Ceiling));
        assert!(rec.was_clamped());
        assert_eq!(rec.requested, 200);
        assert_eq!(rec.emitted, 85);
    }

    #[test]
    fn steam_bounds_are_the_documented_numbers() {
        assert!((SteamSetpoint::FLOOR.fahrenheit() - 90.0).abs() < f32::EPSILON);
        assert!((SteamSetpoint::CEILING.fahrenheit() - 125.0).abs() < f32::EPSILON);
        assert!((SteamSetpoint::FACTORY_DEFAULT.fahrenheit() - 110.0).abs() < f32::EPSILON);
    }

    #[test]
    fn steam_rejects_half_degrees_on_the_api_path() {
        assert!(matches!(
            SteamSetpoint::try_new(Fx2::from_raw(221)),
            Err(ClampError::NotOnStep { .. })
        ));
        let (v, rec) = SteamSetpoint::clamped(Fx2::from_raw(221));
        assert_eq!(v.wire(), Fx2::from_raw(220));
        assert_eq!(rec.bound, Some(Bound::Step));
    }

    proptest::proptest! {
        /// The property the encoder relies on: whatever comes in, what goes out
        /// is inside the clamp. No input reaches the wire outside 60..=85.
        #[test]
        fn no_input_escapes_the_valve_clamp(raw in 0u8..=255) {
            let (v, _) = ValveSetpoint::clamped(Cx2::from_raw(raw));
            proptest::prop_assert!(v.wire() >= ValveSetpoint::FLOOR);
            proptest::prop_assert!(v.wire() <= ValveSetpoint::CEILING);
        }

        #[test]
        fn no_input_escapes_the_steam_clamp(raw in 0u8..=255) {
            let (v, _) = SteamSetpoint::clamped(Fx2::from_raw(raw));
            proptest::prop_assert!(v.wire() >= SteamSetpoint::FLOOR);
            proptest::prop_assert!(v.wire() <= SteamSetpoint::CEILING);
            proptest::prop_assert!(v.wire().is_whole_degree());
        }

        /// Accepting on the API path implies the encoder would not move it.
        #[test]
        fn the_two_paths_agree(raw in 0u8..=255) {
            let c = Cx2::from_raw(raw);
            if let Ok(api) = ValveSetpoint::try_new(c) {
                let (enc, rec) = ValveSetpoint::clamped(c);
                proptest::prop_assert_eq!(api, enc);
                proptest::prop_assert!(!rec.was_clamped());
            }
        }
    }
}
