//! The two wire temperature encodings, and the one conversion between them.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Celsius times two — the encoding the Saturn valve protocol uses.
///
/// Half-degree Celsius resolution in one byte. `Cx2(70)` is 35.0 °C.
///
/// Deliberately *not* implemented, and asserted by compile-fail tests:
/// `From<Fx2>`, `Into<Fx2>`, `Deref`, `AsRef<Fx2>`, arithmetic with [`Fx2`], or
/// any trait both types share. The only route across is [`Cx2::to_fx2`].
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cx2(u8);

/// Fahrenheit times two — the encoding the DTV+ steam link uses.
///
/// `Fx2(220)` is 110.0 °F, the steam factory default.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Fx2(u8);

impl Cx2 {
    /// `MIN_SYS_VALVE_TEMP` — 30.0 °C. Below it the valve rejects the write.
    pub const MIN_SYS_VALVE_TEMP: Self = Self(60);
    /// `MAX_WATER_TEMP` — 49.0 °C. The valve's own hardware ceiling, recorded
    /// here for comparison only. It is **not** a comfort limit and is never used
    /// as one; see [`crate::ValveSetpoint::CEILING`].
    pub const MAX_WATER_TEMP: Self = Self(98);

    #[must_use]
    pub const fn from_raw(v: u8) -> Self {
        Self(v)
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn celsius(self) -> f32 {
        f32::from(self.0) / 2.0
    }

    #[must_use]
    pub fn fahrenheit(self) -> f32 {
        self.celsius().mul_add(9.0 / 5.0, 32.0)
    }

    /// The only `Cx2` to `Fx2` conversion in the workspace.
    ///
    /// `Fx2 = ((Cx2 * 9) / 5) + 64`, in **integer** arithmetic, exactly as the
    /// shipped firmware computes it. The truncation is deliberate and must be
    /// preserved: a floating-point implementation disagrees with the device by
    /// up to 0.5 °F. Cx2 76 is 38.0 °C — 100.4 °F — and encodes to Fx2 200,
    /// which decodes as 100.0 °F.
    ///
    /// Saturating at the top of the byte. The conversion is only reached for
    /// values inside a clamp, so saturation is unreachable in the control path;
    /// it exists so this function is total rather than panicking.
    ///
    /// `clippy.toml` forbids calling this outside the steam encoder.
    #[must_use]
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_truncation,
        reason = "const fn cannot use From/TryFrom; the widening to u16 and the \
                  guarded narrowing back are both checked here"
    )]
    pub const fn to_fx2(self) -> Fx2 {
        let scaled = ((self.0 as u16 * 9) / 5) + 64;
        if scaled > u8::MAX as u16 {
            Fx2(u8::MAX)
        } else {
            Fx2(scaled as u8)
        }
    }
}

impl Fx2 {
    #[must_use]
    pub const fn from_raw(v: u8) -> Self {
        Self(v)
    }

    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn fahrenheit(self) -> f32 {
        f32::from(self.0) / 2.0
    }

    /// True when this value is a whole degree Fahrenheit.
    ///
    /// The steam setpoint moves in 1 °F steps, so only even `Fx2` values are
    /// reachable through the documented interface. Whether the generator accepts
    /// a half-degree is unknown `[?]`; the encoder does not emit one.
    #[must_use]
    pub const fn is_whole_degree(self) -> bool {
        self.0.is_multiple_of(2)
    }
}

/// The result of converting `Fx2` back to Celsius — a dead end, on purpose.
///
/// The reverse conversion loses a step: Cx2 86 encodes to Fx2 218, which
/// converts back to Cx2 85. No constructor in this workspace accepts a
/// `LossyCx2`, so a value that has been round-tripped cannot re-enter the
/// control path. It exists for display and for capture analysis.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct LossyCx2(u8);

impl LossyCx2 {
    #[must_use]
    pub const fn raw(self) -> u8 {
        self.0
    }

    #[must_use]
    pub fn celsius(self) -> f32 {
        f32::from(self.0) / 2.0
    }
}

/// `Fx2` to Celsius-times-two, for reading captures and rendering status.
///
/// Never for building a command. The return type is the enforcement.
#[must_use]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "the subtraction is guarded and the quotient cannot exceed u8"
)]
pub const fn fx2_to_lossy_cx2(f: Fx2) -> LossyCx2 {
    if f.0 < 64 {
        return LossyCx2(0);
    }
    let scaled = ((f.0 as u16 - 64) * 5) / 9;
    if scaled > u8::MAX as u16 {
        LossyCx2(u8::MAX)
    } else {
        LossyCx2(scaled as u8)
    }
}

impl fmt::Debug for Cx2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cx2({} = {:.1}C)", self.0, self.celsius())
    }
}

impl fmt::Debug for Fx2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fx2({} = {:.1}F)", self.0, self.fahrenheit())
    }
}

impl fmt::Display for Cx2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1} °C", self.celsius())
    }
}

impl fmt::Display for Fx2 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1} °F", self.fahrenheit())
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "these tests exist to pin the conversion's behaviour, which is the one \
              legitimate reason to call it outside the steam encoder"
)]
mod tests {
    use super::*;

    /// The worked example in `research/xagon0/docs/devices/steam-generator.md`.
    #[test]
    fn documented_conversion_example_holds() {
        assert_eq!(Cx2::from_raw(70).to_fx2(), Fx2::from_raw(190));
    }

    /// The truncation is the point. These are the values a float implementation
    /// would get wrong, and the device would not.
    #[test]
    fn conversion_truncates_exactly_as_the_firmware_does() {
        // 38.0 C is 100.4 F, and encodes to 100.0 F.
        assert_eq!(Cx2::from_raw(76).to_fx2(), Fx2::from_raw(200));
        // 40.5 C is 104.9 F, and encodes to 104.5 F.
        assert_eq!(Cx2::from_raw(81).to_fx2(), Fx2::from_raw(209));
        // 42.5 C is 108.5 F exactly, and survives.
        assert_eq!(Cx2::from_raw(85).to_fx2(), Fx2::from_raw(217));
        // MIN_STEAM_SETPOINT: the document calls Cx2 48 "24 C / 75 F". 24 C is
        // 75.2 F; the truncation is why the document says 75.
        assert_eq!(Cx2::from_raw(48).to_fx2(), Fx2::from_raw(150));
    }

    #[test]
    fn the_units_hazard_is_representable_only_as_a_deliberate_act() {
        let steam_default = Fx2::from_raw(220);
        assert!((steam_default.fahrenheit() - 110.0).abs() < f32::EPSILON);
        // The same byte read as Cx2 would be 110 C. There is no code path that
        // does this; the assertion records why the types are separate.
        let misread = Cx2::from_raw(steam_default.raw());
        assert!(misread.celsius() > Cx2::MAX_WATER_TEMP.celsius());
    }

    #[test]
    fn round_trip_is_lossy_and_lands_in_a_dead_end_type() {
        let back = fx2_to_lossy_cx2(Cx2::from_raw(86).to_fx2());
        assert_eq!(
            back.raw(),
            85,
            "a step is lost, which is why the type differs"
        );
    }

    #[test]
    fn conversion_saturates_rather_than_wrapping() {
        assert_eq!(Cx2::from_raw(u8::MAX).to_fx2(), Fx2::from_raw(u8::MAX));
    }

    #[test]
    fn whole_degree_detection() {
        assert!(Fx2::from_raw(220).is_whole_degree());
        assert!(!Fx2::from_raw(221).is_whole_degree());
    }

    proptest::proptest! {
        /// Over the whole byte range the conversion never wraps and never panics.
        #[test]
        fn conversion_is_total_and_monotonic(a in 0u8..=254) {
            let lo = Cx2::from_raw(a).to_fx2();
            let hi = Cx2::from_raw(a + 1).to_fx2();
            proptest::prop_assert!(hi >= lo);
        }
    }
}
