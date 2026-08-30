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

/// Why a Fahrenheit request could not become a valve setpoint.
///
/// Separate from [`ClampError`], which counts in `Cx2` raw units. A Fahrenheit
/// request lands between representable points, so reporting it as an integer
/// raw value would round the number in the message and could produce
/// "85 is above the ceiling of 85" for a request of 108.6 °F.
///
/// ~~The requested value was formatted `{:.1}`.~~ Superseded: that reproduced
/// the same defect one decimal place further down. A request of 108.51 °F is
/// refused — correctly, the ceiling is checked before rounding — and rendered
/// "108.5 °F is above the 108.5 °F ceiling", which reads to an operator as a
/// system that cannot compare two numbers. The requested value is now printed
/// to two places and the bound to one, because they are different kinds of
/// number: one is what someone asked for and the other is a property of the
/// `Cx2` encoding, which has nothing below a half degree Celsius.
///
/// Not `Eq`: the requested value is a float.
#[derive(Copy, Clone, PartialEq, Debug, thiserror::Error)]
pub enum FahrenheitError {
    #[error("{requested} is not a temperature")]
    NotATemperature { requested: f32 },
    #[error("{requested:.2} °F is below the {floor:.1} °F floor")]
    BelowFloor { requested: f32, floor: f32 },
    #[error("{requested:.2} °F is above the {ceiling:.1} °F ceiling")]
    AboveCeiling { requested: f32, ceiling: f32 },
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

    /// The API path from Fahrenheit. `API-01`.
    ///
    /// The public API speaks Fahrenheit; the valve speaks [`Cx2`]. This is the
    /// only route between them, and it ends at [`ValveSetpoint::try_new`] like
    /// every other accepted setpoint.
    ///
    /// # Rounding is down, toward cooler
    ///
    /// `Cx2` resolves 0.5 °C — about 0.9 °F — so a request in Fahrenheit
    /// generally falls between two representable points. This returns the
    /// **largest representable setpoint at or below the request**. Erring cool
    /// costs comfort; erring warm costs skin, and water above 43 °C scalds
    /// ([`crate::independent::SCALD_C`]). [`SteamSetpoint::clamped`] rounds a
    /// half degree down for the same reason.
    ///
    /// # A request above the ceiling is refused, not rounded into it
    ///
    /// The bounds are checked **in Fahrenheit, against the request itself**,
    /// before anything is rounded. Rounding first would turn 108.6 °F into
    /// `Cx2` 85 and accept it as though the operator had asked for 108.5 —
    /// which is exactly the silent acceptance [`ValveSetpoint::try_new`] exists
    /// to refuse. So anything above [`ValveSetpoint::CEILING`]'s 108.5 °F or
    /// below [`ValveSetpoint::FLOOR`]'s 86.0 °F is rejected to the caller and
    /// no valve state changes.
    ///
    /// # The candidate is chosen by search, not by arithmetic
    ///
    /// The 26 setpoints in `FLOOR..=CEILING` are scanned and compared against
    /// [`Cx2::fahrenheit`] — the same function that defines what a `Cx2` *is*
    /// in Fahrenheit. That makes the round trip exact by construction rather
    /// than by an epsilon: `from_fahrenheit(c.fahrenheit())` is `c` for every
    /// `c` in range, which a divide-and-floor implementation does not give,
    /// because `fahrenheit()` can land a few parts in `10^7` below the exact
    /// value and drop a whole step.
    ///
    /// This is the compiled clamp only. A configuration may narrow it further,
    /// and `kdtv_config::Bounds::valve_setpoint` is where that is applied.
    pub fn from_fahrenheit(requested: f32) -> Result<Self, FahrenheitError> {
        let floor = Self::FLOOR.fahrenheit();
        let ceiling = Self::CEILING.fahrenheit();
        if !requested.is_finite() {
            return Err(FahrenheitError::NotATemperature { requested });
        }
        if requested < floor {
            return Err(FahrenheitError::BelowFloor { requested, floor });
        }
        if requested > ceiling {
            return Err(FahrenheitError::AboveCeiling { requested, ceiling });
        }

        let mut chosen = Self::FLOOR;
        for raw in Self::FLOOR.raw()..=Self::CEILING.raw() {
            let candidate = Cx2::from_raw(raw);
            if candidate.fahrenheit() > requested {
                break;
            }
            chosen = candidate;
        }

        match Self::try_new(chosen) {
            Ok(v) => Ok(v),
            // Unreachable: `chosen` is drawn from `FLOOR..=CEILING`. Written as
            // a refusal rather than an `expect`, because the alternative to a
            // refusal on this path is a panic in the water path.
            Err(ClampError::BelowFloor { .. }) => {
                Err(FahrenheitError::BelowFloor { requested, floor })
            }
            Err(_) => Err(FahrenheitError::AboveCeiling { requested, ceiling }),
        }
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
    /// A refusal must not read as "108.5 is above 108.5".
    ///
    /// The requested value and the bound are rendered at different precisions
    /// on purpose: a request just over the ceiling is refused before rounding,
    /// so the two are genuinely different numbers and a message that showed
    /// them as equal would look like a broken comparison rather than a refused
    /// request.
    #[test]
    fn a_refusal_names_a_number_different_from_the_bound_it_refuses() {
        let err = ValveSetpoint::from_fahrenheit(108.51).expect_err("above the ceiling");
        let text = err.to_string();
        assert!(text.contains("108.51"), "{text}");
        assert!(
            !text.contains("108.50 °F is above the 108.5"),
            "the requested value must not render as the bound: {text}"
        );

        let low = ValveSetpoint::from_fahrenheit(85.99).expect_err("below the floor");
        let text = low.to_string();
        assert!(text.contains("85.99"), "{text}");
    }

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
    fn fahrenheit_lands_on_the_representable_point_at_or_below_the_request() {
        // 100.0 F is 37.78 C. The step below it is 37.5 C — Cx2 75, 99.5 F.
        let v = ValveSetpoint::from_fahrenheit(100.0).unwrap();
        assert_eq!(v.wire(), Cx2::from_raw(75));
        // 100.4 F is 38.0 C exactly, and is representable.
        assert_eq!(
            ValveSetpoint::from_fahrenheit(100.4).unwrap().wire(),
            Cx2::from_raw(76)
        );
        // One notch under it stays on the cooler step.
        assert_eq!(
            ValveSetpoint::from_fahrenheit(100.39).unwrap().wire(),
            Cx2::from_raw(75)
        );
    }

    #[test]
    fn fahrenheit_boundaries_are_the_clamp_boundaries() {
        // The floor, exactly.
        assert_eq!(
            ValveSetpoint::from_fahrenheit(86.0).unwrap().wire(),
            ValveSetpoint::FLOOR
        );
        // The ceiling, exactly.
        assert_eq!(
            ValveSetpoint::from_fahrenheit(108.5).unwrap().wire(),
            ValveSetpoint::CEILING
        );
        // Just under the floor.
        assert!(matches!(
            ValveSetpoint::from_fahrenheit(85.9),
            Err(FahrenheitError::BelowFloor { .. })
        ));
    }

    /// The case the rounding rule exists to get right.
    ///
    /// 108.6 °F rounds *down* onto the ceiling. Accepting it would hand the
    /// caller 108.5 °F while they asked for more than the clamp allows, which
    /// is the silent acceptance `try_new` refuses on the `Cx2` path. It must
    /// be refused here too.
    #[test]
    fn a_request_above_the_ceiling_is_refused_rather_than_rounded_into_it() {
        for f in [108.51_f32, 108.6, 109.0, 109.4, 113.0, 120.0] {
            let err = ValveSetpoint::from_fahrenheit(f)
                .expect_err(&format!("{f} F is above the ceiling"));
            assert!(
                matches!(err, FahrenheitError::AboveCeiling { .. }),
                "{f} F gave {err:?}"
            );
            // And the message says so in Fahrenheit, not in raw Cx2 units.
            assert!(err.to_string().contains("108.5 °F"), "{err}");
        }
    }

    #[test]
    fn a_request_that_is_not_a_temperature_is_refused() {
        for f in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(matches!(
                ValveSetpoint::from_fahrenheit(f),
                Err(FahrenheitError::NotATemperature { .. })
            ));
        }
    }

    /// The round trip is exact for every setpoint in range, which is the
    /// property that makes a client able to read a temperature back and send it
    /// again without drifting a step cooler each time.
    #[test]
    fn every_setpoint_survives_a_round_trip_through_fahrenheit() {
        for raw in ValveSetpoint::FLOOR.raw()..=ValveSetpoint::CEILING.raw() {
            let c = Cx2::from_raw(raw);
            let back = ValveSetpoint::from_fahrenheit(c.fahrenheit())
                .unwrap_or_else(|e| panic!("Cx2 {raw} came back as {e}"));
            assert_eq!(back.wire(), c, "Cx2 {raw} did not round-trip");
        }
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

        /// Nothing a caller can ask for in Fahrenheit escapes the clamp, and
        /// what comes back is never warmer than what was asked for.
        #[test]
        fn no_fahrenheit_request_escapes_the_valve_clamp(f in -1000.0f32..1000.0) {
            if let Ok(v) = ValveSetpoint::from_fahrenheit(f) {
                proptest::prop_assert!(v.wire() >= ValveSetpoint::FLOOR);
                proptest::prop_assert!(v.wire() <= ValveSetpoint::CEILING);
                proptest::prop_assert!(v.fahrenheit() <= f);
            }
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
