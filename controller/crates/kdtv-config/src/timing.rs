//! Timing, and the wall between the two classes of duration.
//!
//! The end-to-end suite has to run a twenty-minute session limit in a few
//! seconds. It must never run a 525 ms bus tick in five. Those two facts pull in
//! opposite directions, and the usual resolution — a global time scale with a
//! comment saying which constants not to apply it to — puts the whole safety
//! argument on that comment.
//!
//! So the classes are types.
//!
//! | Class | Type | Scalable |
//! | --- | --- | --- |
//! | Session | [`SessionSpan`] | yes, by [`SessionScale`] |
//! | Wire | [`kdtv_proto::saturn::Timings`], [`kdtv_proto::dtv::DtvTimings`] | no |
//!
//! [`SessionScale::apply`] accepts a [`SessionSpan`] and nothing else, and a
//! `SessionSpan` cannot be built from an arbitrary [`Duration`] — its
//! constructors enumerate the session-class durations this service has:
//! the session limit. A protocol
//! deadline has no route into one, so scaling a 525 ms tick is not a mistake
//! that can be made; it is a program that does not compile.
//!
//! Under [`Profile::Production`] the scale is forced to
//! [`SessionScale::UNSCALED`] and a `[bench]` table is refused outright.
//!
//! [`Profile::Production`]: crate::Profile

use crate::error::ConfigError;
use crate::profile::Profile;
use kdtv_proto::dtv::DtvTimings;
use kdtv_proto::saturn::Timings;
use kdtv_units::SessionDuration;
use std::time::Duration;

/// A session-class duration: one that exists because a person is standing in a
/// shower, not because a device is waiting for a byte.
///
/// The constructors are the complete list. There is deliberately no
/// `From<Duration>`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct SessionSpan(Duration);

impl SessionSpan {
    /// The session limit.
    #[must_use]
    pub const fn of_session(d: SessionDuration) -> Self {
        Self(d.get())
    }

    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

/// The factor applied to session-class durations, and to nothing else.
///
/// Restricted to `0 < scale <= 1.0`. A scale above 1.0 would lengthen a session
/// limit or a trip dwell, which is widening a safety bound by another name; the
/// type refuses it, so the narrowing rule in [`crate::bounds`] holds through
/// scaling as well.
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub struct SessionScale(f64);

impl SessionScale {
    /// 1.0 — the only scale a production configuration can have.
    pub const UNSCALED: Self = Self(1.0);

    /// Rejects a scale outside `0 < scale <= 1.0`, and anything not finite.
    pub fn try_new(value: f64) -> Result<Self, ConfigError> {
        if !value.is_finite() || value <= 0.0 || value > 1.0 {
            return Err(ConfigError::SessionScaleOutOfRange { value });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }

    #[must_use]
    pub fn is_unscaled(self) -> bool {
        // Exact comparison against the one value construction can reach for an
        // unscaled configuration. `try_new` never produces a value that is
        // near-but-not 1.0 by accident: the number came from the file.
        self.0 >= 1.0
    }

    /// **The only scaling function in this crate.**
    ///
    /// Takes a [`SessionSpan`]. There is no overload, no generic and no
    /// `Duration` version.
    #[must_use]
    pub fn apply(self, span: SessionSpan) -> SessionSpan {
        SessionSpan(span.0.mul_f64(self.0))
    }
}

impl Default for SessionScale {
    fn default() -> Self {
        Self::UNSCALED
    }
}

/// Link timing plus the session scale.
///
/// The two wire timing sets are [`kdtv_proto`]'s own, carried by value and
/// unmodified. Both plumb their contradictions — 400 ms against 320 ms, 150 ms
/// against 500 ms, three retries against five — and this crate resolves none of
/// them. `CORRECTIONS.md` item 5.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct TimingConfig {
    saturn: Timings,
    dtv: DtvTimings,
    scale: SessionScale,
}

impl TimingConfig {
    /// Builds the set, forcing [`SessionScale::UNSCALED`] under production.
    pub fn new(
        profile: Profile,
        saturn: Timings,
        dtv: DtvTimings,
        scale: SessionScale,
    ) -> Result<Self, ConfigError> {
        if profile.is_production() && !scale.is_unscaled() {
            return Err(ConfigError::BenchTableUnderProduction);
        }
        Ok(Self {
            saturn,
            dtv,
            scale: if profile.is_production() {
                SessionScale::UNSCALED
            } else {
                scale
            },
        })
    }

    /// The Saturn wire timings. Never scaled, in any profile.
    #[must_use]
    pub const fn saturn(&self) -> Timings {
        self.saturn
    }

    /// The DTV+ wire timings. Never scaled, in any profile.
    #[must_use]
    pub const fn dtv(&self) -> DtvTimings {
        self.dtv
    }

    #[must_use]
    pub const fn session_scale(&self) -> SessionScale {
        self.scale
    }

    /// A session-class duration with the scale applied.
    #[must_use]
    pub fn scaled(&self, span: SessionSpan) -> SessionSpan {
        self.scale.apply(span)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scale_above_one_is_refused() {
        for v in [1.000_001_f64, 2.0, 1e9, f64::INFINITY] {
            assert!(matches!(
                SessionScale::try_new(v),
                Err(ConfigError::SessionScaleOutOfRange { .. })
            ));
        }
        for v in [0.0, -1.0, f64::NAN, f64::NEG_INFINITY] {
            assert!(matches!(
                SessionScale::try_new(v),
                Err(ConfigError::SessionScaleOutOfRange { .. })
            ));
        }
        assert!(SessionScale::try_new(1.0).is_ok());
        assert!(SessionScale::try_new(0.01).is_ok());
    }

    #[test]
    fn production_refuses_a_scale_and_bench_keeps_it() {
        let scaled = SessionScale::try_new(0.01).unwrap();
        assert!(matches!(
            TimingConfig::new(
                Profile::Production,
                Timings::DOCUMENTED,
                DtvTimings::DOCUMENTED,
                scaled
            ),
            Err(ConfigError::BenchTableUnderProduction)
        ));

        let prod = TimingConfig::new(
            Profile::Production,
            Timings::DOCUMENTED,
            DtvTimings::DOCUMENTED,
            SessionScale::UNSCALED,
        )
        .unwrap();
        assert!(prod.session_scale().is_unscaled());

        let bench = TimingConfig::new(
            Profile::Bench,
            Timings::DOCUMENTED,
            DtvTimings::DOCUMENTED,
            scaled,
        )
        .unwrap();
        assert!(!bench.session_scale().is_unscaled());
    }

    /// The structural claim, stated numerically as well: a bench scale of 0.01
    /// moves every session-class duration and leaves every wire deadline where
    /// it was.
    #[test]
    fn scaling_moves_session_durations_and_no_wire_deadline() {
        let scale = SessionScale::try_new(0.01).unwrap();
        let t = TimingConfig::new(
            Profile::Bench,
            Timings::DOCUMENTED,
            DtvTimings::DOCUMENTED,
            scale,
        )
        .unwrap();

        // Session class: scaled.
        let limit = SessionSpan::of_session(SessionDuration::clamped(Duration::from_secs(1200)));
        assert_eq!(t.scaled(limit).get(), Duration::from_secs(12));

        // Wire class: untouched. Every deadline the two protocols define.
        let s = t.saturn();
        assert_eq!(s.tick, Duration::from_millis(525));
        assert_eq!(s.response, Duration::from_millis(320));
        assert_eq!(s.response_candidate_long, Duration::from_millis(400));
        assert_eq!(s.response_candidate_short, Duration::from_millis(320));
        assert_eq!(s.address_enquiry_timeout, Duration::from_millis(400));
        assert_eq!(s.address_enquiry_rate, Duration::from_millis(2000));
        assert_eq!(s.address_clear_delay, Duration::from_millis(2000));
        assert_eq!(s.stagger, Duration::from_millis(500));
        assert_eq!(s.transaction_budget, Timings::DOCUMENTED.transaction_budget);
        assert_eq!(s.retries, 3);
        assert_eq!(s.address_retries, 3);
        assert_eq!(s, Timings::DOCUMENTED);

        let d = t.dtv();
        assert_eq!(d.tick, Duration::from_millis(500));
        assert_eq!(d.tick_candidate_fast, Duration::from_millis(150));
        assert_eq!(d.reply, Duration::from_millis(300));
        assert_eq!(d.address_enquiry_timeout, Duration::from_millis(400));
        assert_eq!(d, DtvTimings::DOCUMENTED);
    }

    #[test]
    fn scaling_only_shortens() {
        for raw in [1.0_f64, 0.5, 0.25, 0.01, 1e-6] {
            let scale = SessionScale::try_new(raw).unwrap();
            let span = SessionSpan::of_session(SessionDuration::clamped(Duration::from_secs(1200)));
            assert!(scale.apply(span) <= span);
        }
    }
}
