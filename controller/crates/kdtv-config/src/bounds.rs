//! Safety bounds, and the rule that configuration may only narrow them.
//!
//! The setpoint ceiling, the session limit and the steam envelope are compiled
//! in — [`kdtv_units::ValveSetpoint`], [`kdtv_units::SessionDuration`],
//! [`kdtv_units::SteamSetpoint`] and [`kdtv_units::SteamMinutes`] own the
//! numbers. A `[bounds]` table may tighten any of them for a particular
//! installation; nothing it can contain loosens one.
//!
//! That is enforced twice, deliberately:
//!
//! 1. [`ValidatedConfig::load`] refuses a configured bound that would widen,
//!    naming the field. An operator who writes `max_session_s = 3600` is told
//!    no rather than silently given 1200 seconds.
//! 2. [`Bounds::resolve`] takes the tighter of the compiled constant and the
//!    request regardless, so the widening is not representable even if a future
//!    edit drops the first check. [`no_request_can_widen_a_bound`] is the
//!    property test over that function, and it is this crate's most important
//!    test.
//!
//! [`ValidatedConfig::load`]: crate::ValidatedConfig::load
//! [`no_request_can_widen_a_bound`]: self#tests

use crate::error::ConfigError;
use kdtv_units::{
    ClampError, Cx2, Fx2, SessionDuration, SteamMinutes, SteamSetpoint, ValveSetpoint,
};
use std::time::Duration;

/// A narrowing request, already converted to wire encodings.
///
/// Separate from [`Bounds`] so that [`Bounds::resolve`] is a total function of
/// plain values: the property test can hand it arbitrary bytes without going
/// through TOML.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct BoundsRequest {
    pub setpoint_ceiling: Option<Cx2>,
    pub setpoint_floor: Option<Cx2>,
    pub max_session: Option<Duration>,
    pub steam_ceiling: Option<Fx2>,
    pub steam_floor: Option<Fx2>,
    pub steam_max_minutes: Option<u8>,
}

/// The bounds this service will actually enforce.
///
/// Every field is at least as tight as its compiled-in counterpart.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Bounds {
    setpoint_ceiling: Cx2,
    setpoint_floor: Cx2,
    max_session: Duration,
    steam_ceiling: Fx2,
    steam_floor: Fx2,
    steam_max_minutes: u8,
}

impl Bounds {
    /// The compiled-in bounds, before any configuration.
    pub const COMPILED: Self = Self {
        setpoint_ceiling: ValveSetpoint::CEILING,
        setpoint_floor: ValveSetpoint::FLOOR,
        max_session: SessionDuration::HARD_LIMIT,
        steam_ceiling: SteamSetpoint::CEILING,
        steam_floor: SteamSetpoint::FLOOR,
        steam_max_minutes: SteamMinutes::MAX,
    };

    /// Takes the tighter of the compiled constant and the request, field by
    /// field. Total, and monotone in the narrowing direction.
    #[must_use]
    pub fn resolve(request: &BoundsRequest) -> Self {
        let c = Self::COMPILED;
        Self {
            // A ceiling narrows downward.
            setpoint_ceiling: min_cx2(c.setpoint_ceiling, request.setpoint_ceiling),
            // A floor narrows upward.
            setpoint_floor: max_cx2(c.setpoint_floor, request.setpoint_floor),
            max_session: match request.max_session {
                Some(d) if d < c.max_session => d,
                _ => c.max_session,
            },
            steam_ceiling: min_fx2(c.steam_ceiling, request.steam_ceiling),
            steam_floor: max_fx2(c.steam_floor, request.steam_floor),
            steam_max_minutes: match request.steam_max_minutes {
                Some(m) if m < c.steam_max_minutes => m,
                _ => c.steam_max_minutes,
            },
        }
    }

    /// Refuses a request that would widen any bound, naming the field.
    ///
    /// Runs before [`Bounds::resolve`], not instead of it.
    pub fn check_narrowing(request: &BoundsRequest) -> Result<(), ConfigError> {
        let c = Self::COMPILED;
        if let Some(v) = request.setpoint_ceiling
            && v > c.setpoint_ceiling
        {
            return Err(widen(
                "bounds.setpoint_ceiling_c",
                v.celsius(),
                c.setpoint_ceiling.celsius(),
            ));
        }
        if let Some(v) = request.setpoint_floor
            && v < c.setpoint_floor
        {
            return Err(widen(
                "bounds.setpoint_floor_c",
                v.celsius(),
                c.setpoint_floor.celsius(),
            ));
        }
        if let Some(v) = request.max_session
            && v > c.max_session
        {
            return Err(ConfigError::BoundWiden {
                field: "bounds.max_session_s",
                requested: v.as_secs().to_string(),
                compiled: c.max_session.as_secs().to_string(),
            });
        }
        if let Some(v) = request.steam_ceiling
            && v > c.steam_ceiling
        {
            return Err(widen(
                "bounds.steam_ceiling_f",
                v.fahrenheit(),
                c.steam_ceiling.fahrenheit(),
            ));
        }
        if let Some(v) = request.steam_floor
            && v < c.steam_floor
        {
            return Err(widen(
                "bounds.steam_floor_f",
                v.fahrenheit(),
                c.steam_floor.fahrenheit(),
            ));
        }
        if let Some(v) = request.steam_max_minutes
            && v > c.steam_max_minutes
        {
            return Err(ConfigError::BoundWiden {
                field: "bounds.steam_max_minutes",
                requested: v.to_string(),
                compiled: c.steam_max_minutes.to_string(),
            });
        }
        Ok(())
    }

    /// Refuses a resolved range whose floor sits above its ceiling.
    pub fn check_ordering(self) -> Result<(), ConfigError> {
        if self.setpoint_floor > self.setpoint_ceiling {
            return Err(ConfigError::SetpointRangeInverted {
                floor: f64::from(self.setpoint_floor.celsius()),
                ceiling: f64::from(self.setpoint_ceiling.celsius()),
            });
        }
        if self.steam_floor > self.steam_ceiling {
            return Err(ConfigError::SteamRangeInverted {
                floor: f64::from(self.steam_floor.fahrenheit()),
                ceiling: f64::from(self.steam_ceiling.fahrenheit()),
            });
        }
        Ok(())
    }

    #[must_use]
    pub const fn setpoint_ceiling(self) -> Cx2 {
        self.setpoint_ceiling
    }

    #[must_use]
    pub const fn setpoint_floor(self) -> Cx2 {
        self.setpoint_floor
    }

    /// The session limit, as a [`SessionDuration`] — which saturates at the
    /// compiled 20 minutes on the way through, so this cannot exceed it however
    /// the field was reached.
    #[must_use]
    pub fn max_session(self) -> SessionDuration {
        SessionDuration::clamped(self.max_session)
    }

    #[must_use]
    pub const fn steam_ceiling(self) -> Fx2 {
        self.steam_ceiling
    }

    #[must_use]
    pub const fn steam_floor(self) -> Fx2 {
        self.steam_floor
    }

    /// The steam session cap, as [`SteamMinutes`] — which clamps into 1..=20,
    /// so zero is not reachable and neither is anything above the generator's
    /// documented maximum.
    #[must_use]
    pub fn steam_max_minutes(self) -> SteamMinutes {
        SteamMinutes::clamped(self.steam_max_minutes)
    }

    /// A valve setpoint checked against the narrowed range **and** the compiled
    /// clamp.
    ///
    /// The compiled clamp is applied last, by
    /// [`ValveSetpoint::try_new`], so no narrowing arithmetic here can produce a
    /// value outside it.
    pub fn valve_setpoint(self, c: Cx2) -> Result<ValveSetpoint, ClampError> {
        if c > self.setpoint_ceiling {
            return Err(ClampError::AboveCeiling {
                requested: i32::from(c.raw()),
                ceiling: i32::from(self.setpoint_ceiling.raw()),
            });
        }
        if c < self.setpoint_floor {
            return Err(ClampError::BelowFloor {
                requested: i32::from(c.raw()),
                floor: i32::from(self.setpoint_floor.raw()),
            });
        }
        ValveSetpoint::try_new(c)
    }

    /// The same for the steam envelope, ending at
    /// [`SteamSetpoint::try_new`].
    pub fn steam_setpoint(self, f: Fx2) -> Result<SteamSetpoint, ClampError> {
        if f > self.steam_ceiling {
            return Err(ClampError::AboveCeiling {
                requested: i32::from(f.raw()),
                ceiling: i32::from(self.steam_ceiling.raw()),
            });
        }
        if f < self.steam_floor {
            return Err(ClampError::BelowFloor {
                requested: i32::from(f.raw()),
                floor: i32::from(self.steam_floor.raw()),
            });
        }
        SteamSetpoint::try_new(f)
    }
}

impl Default for Bounds {
    fn default() -> Self {
        Self::COMPILED
    }
}

/// Degrees Celsius from the file to the valve encoding.
///
/// Refuses anything that is not exactly on the half-degree step `Cx2` can
/// represent. Rounding an operator's `40.3` to `40.5` would be inventing a
/// setpoint they did not write.
pub(crate) fn cx2_from_celsius(field: &'static str, c: f64) -> Result<Cx2, ConfigError> {
    Ok(Cx2::from_raw(exact_byte(field, c * 2.0, c, "0.5 °C")?))
}

/// Degrees Fahrenheit from the file to the steam encoding.
///
/// `Fx2` can hold a half degree; the generator's documented setpoint moves in
/// whole degrees, so an odd raw value is refused rather than emitted.
pub(crate) fn fx2_from_fahrenheit(field: &'static str, f: f64) -> Result<Fx2, ConfigError> {
    let raw = exact_byte(field, f * 2.0, f, "1 °F")?;
    if !raw.is_multiple_of(2) {
        return Err(ConfigError::TemperatureStep {
            field,
            value: f,
            step: "1 °F",
        });
    }
    Ok(Fx2::from_raw(raw))
}

/// `doubled` must be a whole number inside `0..=255`. `original` is what the
/// file said, for the message.
fn exact_byte(
    field: &'static str,
    doubled: f64,
    original: f64,
    step: &'static str,
) -> Result<u8, ConfigError> {
    if !doubled.is_finite() || !(0.0..=255.0).contains(&doubled) {
        return Err(ConfigError::TemperatureRange {
            field,
            value: original,
        });
    }
    let rounded = doubled.round();
    if (doubled - rounded).abs() > 1e-9 {
        return Err(ConfigError::TemperatureStep {
            field,
            value: original,
            step,
        });
    }
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "f64 has no checked conversion to u8; `rounded` is finite, \
                  integral and inside 0..=255 by the two guards above"
    )]
    Ok(rounded as u8)
}

fn widen(field: &'static str, requested: f32, compiled: f32) -> ConfigError {
    ConfigError::BoundWiden {
        field,
        requested: format!("{requested}"),
        compiled: format!("{compiled}"),
    }
}

fn min_cx2(compiled: Cx2, requested: Option<Cx2>) -> Cx2 {
    match requested {
        Some(v) if v < compiled => v,
        _ => compiled,
    }
}

fn max_cx2(compiled: Cx2, requested: Option<Cx2>) -> Cx2 {
    match requested {
        Some(v) if v > compiled => v,
        _ => compiled,
    }
}

fn min_fx2(compiled: Fx2, requested: Option<Fx2>) -> Fx2 {
    match requested {
        Some(v) if v < compiled => v,
        _ => compiled,
    }
}

fn max_fx2(compiled: Fx2, requested: Option<Fx2>) -> Fx2 {
    match requested {
        Some(v) if v > compiled => v,
        _ => compiled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn the_compiled_bounds_are_the_units_crate_bounds() {
        let b = Bounds::COMPILED;
        assert_eq!(b.setpoint_ceiling(), ValveSetpoint::CEILING);
        assert_eq!(b.setpoint_floor(), ValveSetpoint::FLOOR);
        assert_eq!(b.max_session().get(), SessionDuration::HARD_LIMIT);
        assert_eq!(b.steam_ceiling(), SteamSetpoint::CEILING);
        assert_eq!(b.steam_floor(), SteamSetpoint::FLOOR);
        assert_eq!(b.steam_max_minutes().wire(), SteamMinutes::MAX);
    }

    #[test]
    fn a_narrower_request_is_taken() {
        let r = BoundsRequest {
            setpoint_ceiling: Some(Cx2::from_raw(80)),
            setpoint_floor: Some(Cx2::from_raw(64)),
            max_session: Some(Duration::from_secs(120)),
            steam_ceiling: Some(Fx2::from_raw(230)),
            steam_floor: Some(Fx2::from_raw(190)),
            steam_max_minutes: Some(15),
        };
        assert!(Bounds::check_narrowing(&r).is_ok());
        let b = Bounds::resolve(&r);
        assert_eq!(b.setpoint_ceiling(), Cx2::from_raw(80));
        assert_eq!(b.setpoint_floor(), Cx2::from_raw(64));
        assert_eq!(b.max_session().get(), Duration::from_secs(120));
        assert_eq!(b.steam_ceiling(), Fx2::from_raw(230));
        assert_eq!(b.steam_floor(), Fx2::from_raw(190));
        assert_eq!(b.steam_max_minutes().wire(), 15);
    }

    #[test]
    fn a_widening_request_is_refused_by_name() {
        let cases: [(BoundsRequest, &str); 6] = [
            (
                BoundsRequest {
                    setpoint_ceiling: Some(Cx2::from_raw(98)),
                    ..BoundsRequest::default()
                },
                "bounds.setpoint_ceiling_c",
            ),
            (
                BoundsRequest {
                    setpoint_floor: Some(Cx2::from_raw(40)),
                    ..BoundsRequest::default()
                },
                "bounds.setpoint_floor_c",
            ),
            (
                BoundsRequest {
                    max_session: Some(Duration::from_secs(3600)),
                    ..BoundsRequest::default()
                },
                "bounds.max_session_s",
            ),
            (
                BoundsRequest {
                    steam_ceiling: Some(Fx2::from_raw(255)),
                    ..BoundsRequest::default()
                },
                "bounds.steam_ceiling_f",
            ),
            (
                BoundsRequest {
                    steam_floor: Some(Fx2::from_raw(100)),
                    ..BoundsRequest::default()
                },
                "bounds.steam_floor_f",
            ),
            (
                BoundsRequest {
                    steam_max_minutes: Some(45),
                    ..BoundsRequest::default()
                },
                "bounds.steam_max_minutes",
            ),
        ];
        for (request, field) in cases {
            let err = Bounds::check_narrowing(&request).unwrap_err();
            let text = err.to_string();
            assert!(text.contains(field), "{text}");
            assert!(text.contains("narrow"), "{text}");
        }
    }

    #[test]
    fn an_inverted_range_is_refused() {
        let b = Bounds::resolve(&BoundsRequest {
            setpoint_ceiling: Some(Cx2::from_raw(62)),
            setpoint_floor: Some(Cx2::from_raw(84)),
            ..BoundsRequest::default()
        });
        assert!(matches!(
            b.check_ordering(),
            Err(ConfigError::SetpointRangeInverted { .. })
        ));

        let s = Bounds::resolve(&BoundsRequest {
            steam_ceiling: Some(Fx2::from_raw(190)),
            steam_floor: Some(Fx2::from_raw(240)),
            ..BoundsRequest::default()
        });
        assert!(matches!(
            s.check_ordering(),
            Err(ConfigError::SteamRangeInverted { .. })
        ));
    }

    fn any_request() -> impl Strategy<Value = BoundsRequest> {
        (
            proptest::option::of(0u8..=255),
            proptest::option::of(0u8..=255),
            proptest::option::of(0u64..=1_000_000),
            proptest::option::of(0u8..=255),
            proptest::option::of(0u8..=255),
            proptest::option::of(0u8..=255),
        )
            .prop_map(|(sc, sf, ms, tc, tf, mm)| BoundsRequest {
                setpoint_ceiling: sc.map(Cx2::from_raw),
                setpoint_floor: sf.map(Cx2::from_raw),
                max_session: ms.map(Duration::from_secs),
                steam_ceiling: tc.map(Fx2::from_raw),
                steam_floor: tf.map(Fx2::from_raw),
                steam_max_minutes: mm,
            })
    }

    proptest! {
        /// **The crate's most important test.**
        ///
        /// No configuration input, valid or not, produces a bound looser than
        /// the compiled-in one. The request here bypasses TOML parsing and
        /// [`Bounds::check_narrowing`] entirely, so the property rests on
        /// [`Bounds::resolve`] alone.
        #[test]
        fn no_request_can_widen_a_bound(request in any_request()) {
            let b = Bounds::resolve(&request);
            let c = Bounds::COMPILED;

            prop_assert!(b.setpoint_ceiling() <= c.setpoint_ceiling());
            prop_assert!(b.setpoint_floor() >= c.setpoint_floor());
            prop_assert!(b.max_session().get() <= c.max_session().get());
            prop_assert!(b.steam_ceiling() <= c.steam_ceiling());
            prop_assert!(b.steam_floor() >= c.steam_floor());
            prop_assert!(b.steam_max_minutes() <= c.steam_max_minutes());

            // And the same stated against the units crate directly, so the
            // property does not depend on COMPILED being right.
            prop_assert!(b.setpoint_ceiling() <= ValveSetpoint::CEILING);
            prop_assert!(b.setpoint_floor() >= ValveSetpoint::FLOOR);
            prop_assert!(b.max_session().get() <= SessionDuration::HARD_LIMIT);
            prop_assert!(b.steam_ceiling() <= SteamSetpoint::CEILING);
            prop_assert!(b.steam_floor() >= SteamSetpoint::FLOOR);
            prop_assert!(b.steam_max_minutes().wire() <= SteamMinutes::MAX);
            prop_assert!(b.steam_max_minutes().wire() >= SteamMinutes::MIN);
        }

        /// The accessors that hand a setpoint to the rest of the service cannot
        /// return one outside the compiled clamp either, for any request and
        /// any asked-for value.
        #[test]
        fn no_accepted_setpoint_escapes_the_compiled_clamp(
            request in any_request(),
            raw in 0u8..=255,
        ) {
            let b = Bounds::resolve(&request);
            if let Ok(v) = b.valve_setpoint(Cx2::from_raw(raw)) {
                prop_assert!(v.wire() >= ValveSetpoint::FLOOR);
                prop_assert!(v.wire() <= ValveSetpoint::CEILING);
                prop_assert!(v.wire() <= b.setpoint_ceiling());
                prop_assert!(v.wire() >= b.setpoint_floor());
            }
            if let Ok(v) = b.steam_setpoint(Fx2::from_raw(raw)) {
                prop_assert!(v.wire() >= SteamSetpoint::FLOOR);
                prop_assert!(v.wire() <= SteamSetpoint::CEILING);
                prop_assert!(v.wire().is_whole_degree());
            }
        }
    }
}
