//! The independent temperature sampler: one task per zone.
//!
//! The channel's only output is a sample. There is no path from here to an
//! outlet: `kdtv-hal` has no function turning a reading into an authorisation
//! and `kdtv-safety` has none either, so the strongest thing a probe can do is
//! contribute to an all-off. `LOG-10`.
//!
//! # Why it is a task rather than a call in the loop
//!
//! An SPI transfer to a `MAX31865` is a syscall against a driver, and a
//! transfer that hangs must not hold up an all-off on the valve bus. Sampling in
//! its own task means a stalled amplifier shows up as **starvation**, which
//! [`kdtv_safety::RtdWatch::check_starvation`] turns into a safety event on the
//! supervisor's own tick, rather than as a wedged control loop.
//!
//! # The period
//!
//! [`kdtv_units::RTD_STARVATION`] is 5 s: a channel silent for longer than that
//! is a fault. Sampling at a fifth of it gives four consecutive misses before
//! starvation is declared, and puts four samples inside the 2 s dwell that the
//! corrected over-temperature trip needs. Nothing configures it, because a
//! period that could be lengthened past the starvation window is a period
//! someone can set to "never notice".

use std::sync::Arc;
use std::time::Duration;

use kdtv_hal::{Clock, RtdChannel, RtdError, RtdSample};
use kdtv_units::ZoneId;
use tokio::sync::{mpsc, watch};

/// How often each channel is read.
pub(crate) const SAMPLE_PERIOD: Duration = Duration::from_millis(1_000);

/// A reading, or why there was not one.
#[derive(Debug)]
pub(crate) struct Sampled {
    pub(crate) zone: ZoneId,
    pub(crate) result: Result<RtdSample, RtdError>,
}

/// Start a sampler for one zone.
///
/// The task ends when `stop` goes true or the supervisor drops its receiver.
pub(crate) fn spawn(
    mut channel: Box<dyn RtdChannel>,
    clock: Arc<dyn Clock>,
    out: mpsc::Sender<Sampled>,
    mut stop: watch::Receiver<bool>,
) {
    let zone = channel.zone();
    tokio::spawn(async move {
        loop {
            if *stop.borrow_and_update() {
                return;
            }
            let result = channel.sample().await;
            if out.send(Sampled { zone, result }).await.is_err() {
                return;
            }
            let next = clock
                .monotonic()
                .checked_add(SAMPLE_PERIOD)
                .unwrap_or_else(|| clock.monotonic());
            tokio::select! {
                () = clock.sleep_until(next) => {}
                _ = stop.changed() => return,
            }
        }
    });
}

/// The sampling period leaves room for the dwells that depend on it.
///
/// Not a runtime check — a compile-time-shaped assertion in a test, so that
/// moving either constant without the other fails the build rather than
/// silently making starvation undetectable.
#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_units::{CORRECTED_TRIP_DWELL, RTD_STARVATION};

    #[test]
    fn the_period_gives_several_misses_before_starvation_and_several_samples_in_a_dwell() {
        assert!(
            SAMPLE_PERIOD.saturating_mul(4) <= RTD_STARVATION,
            "a single slow sample must not read as starvation"
        );
        assert!(
            SAMPLE_PERIOD < CORRECTED_TRIP_DWELL,
            "the 2 s corrected trip needs more than one sample to decide on"
        );
    }
}
