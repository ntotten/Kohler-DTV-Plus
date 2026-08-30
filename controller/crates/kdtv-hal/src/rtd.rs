//! The independent temperature channels, and the negative space around them.
//!
//! # The sensor has no authority to open anything
//!
//! [`RtdSample`] is a reading and a fault register. There is no function in this
//! crate, and none anywhere in the workspace, that turns one into an
//! authorisation: no `authorize_from_sensor`, no `From<RtdSample> for OpenGrant`.
//! The RTD path's only output is a safety event. That is what makes the
//! independent channel independent — it can contribute to an all-off and it can
//! do nothing else.
//!
//! # Why the raw reading travels with the corrected one
//!
//! A surface clamp is not an immersion measurement: it reads pipe wall, lags by
//! seconds, and reads low. The correction is characterised at commissioning
//! against an immersion probe, and every threshold is evaluated on the corrected
//! value — but the raw reading is logged beside it, and
//! [`kdtv_units::RAW_TRIP_C`] is an absolute backstop under the corrected trip,
//! for the case where the curve is wrong or absent.
//!
//! So [`RtdSample`] carries [`RawC`]. Correction happens above this crate,
//! against the [`OffsetCurve`](kdtv_units::OffsetCurve) the configuration
//! carries; a HAL that corrected on the way out would leave the raw value
//! nowhere to be logged from.
//!
//! # The fault register is carried, not interpreted
//!
//! An open probe reads *low*, not high, so a broken sensor looks like cold water
//! and no threshold fires. The fault register is the only thing that says so,
//! and it is a required field of every sample rather than an optional extra.
//!
//! # Chip select is a constant
//!
//! [`CS_FOR_ZONE`] maps zone to chip select in code. Configuration may restate
//! it — `deploy/kdtvd.toml` does, in `sensors.zoneN.chip_select` — and
//! [`ChipSelect::check`] refuses a start where the two disagree. A swapped pair
//! of chip selects reads zone 2's pipe and calls it zone 1's, which the
//! divergence check would not catch, because both channels would be reading real
//! temperatures.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kdtv_telemetry::Monotonic;
use kdtv_units::{RawC, ZoneId};

use crate::clock::Clock;
use crate::link::BoxedFuture;

/// A SPI chip select, as the kernel names it.
///
/// Denial by absence: there is no variant for a GPIO pin used as an output, and
/// no way to spell one. See [`NO_GPIO_OUTPUT`].
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum ChipSelect {
    Spi0Ce0,
    Spi0Ce1,
    Spi1Ce0,
    Spi1Ce1,
}

impl ChipSelect {
    /// The kernel's name, e.g. `spi0.0`. This is the string configuration uses.
    #[must_use]
    pub const fn kernel_name(self) -> &'static str {
        match self {
            Self::Spi0Ce0 => "spi0.0",
            Self::Spi0Ce1 => "spi0.1",
            Self::Spi1Ce0 => "spi1.0",
            Self::Spi1Ce1 => "spi1.1",
        }
    }

    /// Refuses a configured chip select that disagrees with [`CS_FOR_ZONE`].
    ///
    /// Configuration may restate the mapping. It may not change it.
    pub fn check(zone: ZoneId, configured: &str) -> Result<Self, RtdError> {
        let expected = chip_select_for(zone);
        if configured.trim() == expected.kernel_name() {
            Ok(expected)
        } else {
            Err(RtdError::ChipSelectMismatch {
                zone,
                expected: expected.kernel_name(),
                configured: configured.trim().to_owned(),
            })
        }
    }
}

impl fmt::Display for ChipSelect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kernel_name())
    }
}

/// Which amplifier answers for which zone. A checked constant, not
/// configuration.
pub const CS_FOR_ZONE: [(ZoneId, ChipSelect); 2] = [
    (ZoneId::Zone1, ChipSelect::Spi0Ce0),
    (ZoneId::Zone2, ChipSelect::Spi0Ce1),
];

/// The chip selects held for a third and fourth channel, should a later zone or
/// a second instrumented outlet be added. Disjoint from [`CS_FOR_ZONE`] by
/// construction, and asserted so.
pub const EXPANSION_CS: [ChipSelect; 2] = [ChipSelect::Spi1Ce0, ChipSelect::Spi1Ce1];

/// The mapping, as an exhaustive match. Adding a zone stops this compiling until
/// its chip select is decided.
#[must_use]
pub const fn chip_select_for(zone: ZoneId) -> ChipSelect {
    match zone {
        ZoneId::Zone1 => ChipSelect::Spi0Ce0,
        ZoneId::Zone2 => ChipSelect::Spi0Ce1,
    }
}

/// **There is deliberately no GPIO output trait in this crate, and no GPIO
/// dependency in its manifest.**
///
/// Nothing in this design drives a relay, a contactor, or anything in a mains
/// path. The service's authority stops at three serial links and two read-only
/// SPI amplifiers, and the way that is enforced is that the capability does not
/// exist: no `GpioOut`, no `rppal`, no `gpiod`, no `linux-embedded-hal/gpio`,
/// with the ban restated workspace-wide by `cargo xtask audit-graph` so it
/// cannot arrive through another crate either.
///
/// The value is a string so the reason has somewhere to live in the built
/// documentation. `tests::no_gpio_crate_is_a_dependency` reads the manifest back.
pub const NO_GPIO_OUTPUT: &str = "this service drives no relay, contactor or mains path; there is no GPIO output trait \
     and no GPIO crate in the daemon's dependency graph";

/// The MAX31865's fault status register.
///
/// Bit meanings are from the MAX31865 datasheet's fault-status table. **No RTD
/// channel has been read on this installation**, so nothing here has been seen
/// on the real amplifier; the decode is carried so that a fault is visible the
/// first time one occurs rather than discovered afterwards.
///
/// The raw byte is kept. Naming a bit does not consume it, and an unrecognised
/// bit pattern stays visible in the log rather than being reduced to the bits
/// this table happens to know.
#[derive(Copy, Clone, PartialEq, Eq, Default, Hash)]
pub struct FaultRegister(u8);

impl FaultRegister {
    pub const RTD_HIGH_THRESHOLD: u8 = 0x80;
    pub const RTD_LOW_THRESHOLD: u8 = 0x40;
    pub const REFIN_HIGH: u8 = 0x20;
    pub const REFIN_LOW_FORCE_OPEN: u8 = 0x10;
    pub const RTDIN_LOW_FORCE_OPEN: u8 = 0x08;
    pub const UNDER_OVER_VOLTAGE: u8 = 0x04;

    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// True when no fault bit is set.
    ///
    /// Note what this is **not**: it is not a statement that the reading is
    /// good. A probe that has come off the pipe reads a real temperature — the
    /// air's — with a clear fault register. Nothing here authorises anything.
    #[must_use]
    pub const fn is_clear(self) -> bool {
        self.0 == 0
    }

    /// The bits that indicate a broken or disconnected probe rather than an
    /// out-of-range temperature.
    ///
    /// `[I]` — grouping these four as "wiring" rather than "measurement" is
    /// inference from the datasheet's descriptions, not from a fault seen here.
    #[must_use]
    pub const fn indicates_wiring_fault(self) -> bool {
        self.0
            & (Self::REFIN_HIGH
                | Self::REFIN_LOW_FORCE_OPEN
                | Self::RTDIN_LOW_FORCE_OPEN
                | Self::UNDER_OVER_VOLTAGE)
            != 0
    }

    /// Every named bit that is set, for the log.
    #[must_use]
    pub fn named(self) -> Vec<&'static str> {
        [
            (Self::RTD_HIGH_THRESHOLD, "rtd_high_threshold"),
            (Self::RTD_LOW_THRESHOLD, "rtd_low_threshold"),
            (Self::REFIN_HIGH, "refin_high"),
            (Self::REFIN_LOW_FORCE_OPEN, "refin_low_force_open"),
            (Self::RTDIN_LOW_FORCE_OPEN, "rtdin_low_force_open"),
            (Self::UNDER_OVER_VOLTAGE, "under_over_voltage"),
        ]
        .into_iter()
        .filter(|(mask, _)| self.0 & mask != 0)
        .map(|(_, name)| name)
        .collect()
    }
}

impl fmt::Debug for FaultRegister {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FaultRegister(0x{:02X}", self.0)?;
        let named = self.named();
        if !named.is_empty() {
            write!(f, " {}", named.join("|"))?;
        }
        f.write_str(")")
    }
}

impl fmt::Display for FaultRegister {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_clear() {
            f.write_str("clear")
        } else {
            write!(f, "0x{:02X} {}", self.0, self.named().join("|"))
        }
    }
}

/// One reading from one channel.
///
/// The raw temperature and the fault register are both required fields. A
/// sample that could omit the fault register is a sample that can hide an open
/// probe, which reads low and therefore looks safe.
#[derive(Copy, Clone, PartialEq, Debug)]
pub struct RtdSample {
    pub zone: ZoneId,
    /// The uncorrected pipe-wall reading. Correction happens above this crate,
    /// against the commissioned curve; both values are logged.
    pub raw: RawC,
    pub fault: FaultRegister,
    pub at: Monotonic,
}

/// One independent temperature channel.
///
/// Its only output is a sample. There is no method here that opens, closes,
/// authorises or commands anything.
pub trait RtdChannel: Send + fmt::Debug {
    fn zone(&self) -> ZoneId;

    /// The chip select this channel answers on. Fixed by [`CS_FOR_ZONE`].
    fn chip_select(&self) -> ChipSelect {
        chip_select_for(self.zone())
    }

    fn sample(&mut self) -> BoxedFuture<'_, Result<RtdSample, RtdError>>;
}

#[derive(Debug, thiserror::Error)]
pub enum RtdError {
    /// Configuration named a chip select the constant does not agree with.
    #[error(
        "sensors.{zone}.chip_select is {configured}, but {zone}'s amplifier is on \
         {expected}; a swapped pair reads the other zone's pipe and reports it as this one's"
    )]
    ChipSelectMismatch {
        zone: ZoneId,
        expected: &'static str,
        configured: String,
    },

    /// The SPI transfer failed.
    #[error("{zone}: SPI transfer on {chip_select} failed")]
    Transfer {
        zone: ZoneId,
        chip_select: ChipSelect,
        #[source]
        source: std::io::Error,
    },

    /// No sample arrived within the sampler's budget.
    ///
    /// Starvation is a safety event in its own right
    /// ([`kdtv_units::RTD_STARVATION`]): a channel that has stopped answering
    /// is not a channel that is reading a safe temperature.
    #[error("{zone}: no sample within the sampling budget")]
    Starved { zone: ZoneId },
}

impl RtdError {
    #[must_use]
    pub const fn zone(&self) -> ZoneId {
        match self {
            Self::ChipSelectMismatch { zone, .. }
            | Self::Transfer { zone, .. }
            | Self::Starved { zone } => *zone,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock whose only job is to stamp a sample.
    #[derive(Debug)]
    struct FixedClock;
    impl Clock for FixedClock {
        fn monotonic(&self) -> Monotonic {
            Monotonic::from_nanos(1_000_000_000)
        }
        fn wall(&self) -> crate::clock::WallClock {
            crate::clock::WallClock::new(
                jiff::Timestamp::UNIX_EPOCH,
                kdtv_telemetry::NtpSync::Unsynchronised,
            )
        }
        fn sleep_until(&self, _deadline: Monotonic) -> BoxedFuture<'static, ()> {
            Box::pin(async {})
        }
    }

    fn probe(dir: &Path, contents: &str) -> FileRtdChannel {
        let ch = FileRtdChannel::new(ZoneId::Zone1, dir, Arc::new(FixedClock));
        std::fs::write(ch.path(), contents).expect("write the probe file");
        ch
    }

    fn read(ch: &mut FileRtdChannel) -> Result<RtdSample, RtdError> {
        futures_lite_block_on(ch.sample())
    }

    /// The channel's future does no I/O awaiting, so a minimal executor is
    /// enough and the tests need no runtime.
    fn futures_lite_block_on<T>(fut: BoxedFuture<'_, T>) -> T {
        use std::task::{Context, Poll, Wake, Waker};
        struct Noop;
        impl Wake for Noop {
            fn wake(self: Arc<Self>) {}
        }
        let waker = Waker::from(Arc::new(Noop));
        let mut cx = Context::from_waker(&waker);
        let mut fut = fut;
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => std::hint::spin_loop(),
            }
        }
    }

    #[test]
    fn a_bench_probe_reads_the_number_something_else_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let mut ch = probe(dir.path(), "38.5\n");
        let s = read(&mut ch).expect("a written file reads");
        assert!((s.raw.0 - 38.5).abs() < f32::EPSILON, "{:?}", s.raw);
        assert!(s.fault.is_clear());
        assert_eq!(s.zone, ZoneId::Zone1);
    }

    #[test]
    fn a_fault_register_can_be_written_beside_the_reading() {
        let dir = tempfile::tempdir().unwrap();
        // An open probe reads low with a fault bit set — the case the module
        // docs say no threshold would catch on the temperature alone.
        let mut ch = probe(dir.path(), "12.0 0x04\n");
        let s = read(&mut ch).expect("a written file reads");
        assert!(!s.fault.is_clear());
        assert!(s.fault.indicates_wiring_fault());
    }

    /// An absent or unreadable file is a failed transfer, not a missing sample.
    ///
    /// The distinction is five seconds: a `Transfer` error escalates on the next
    /// pass, and a silently absent sample would look like starvation and take
    /// `RTD_STARVATION` to be noticed.
    #[test]
    fn a_missing_or_malformed_probe_file_is_a_failed_transfer() {
        let dir = tempfile::tempdir().unwrap();
        let mut absent = FileRtdChannel::new(ZoneId::Zone1, dir.path(), Arc::new(FixedClock));
        assert!(matches!(
            read(&mut absent),
            Err(RtdError::Transfer {
                zone: ZoneId::Zone1,
                ..
            })
        ));

        for bad in ["", "warm", "NaN", "38.5 0xZZ"] {
            let mut ch = probe(dir.path(), bad);
            assert!(
                matches!(read(&mut ch), Err(RtdError::Transfer { .. })),
                "{bad:?} must not parse"
            );
        }
    }

    /// The file is named for the zone, and two zones never share one.
    #[test]
    fn each_zone_reads_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let one = FileRtdChannel::path_for(dir.path(), ZoneId::Zone1);
        let two = FileRtdChannel::path_for(dir.path(), ZoneId::Zone2);
        assert_ne!(one, two);
    }

    /// A bench channel still reports the chip select its zone is wired to, so a
    /// log line from an emulated run reads the same as one from hardware.
    #[test]
    fn a_bench_probe_still_answers_on_its_zones_chip_select() {
        let dir = tempfile::tempdir().unwrap();
        let ch = FileRtdChannel::new(ZoneId::Zone2, dir.path(), Arc::new(FixedClock));
        assert_eq!(ch.chip_select(), chip_select_for(ZoneId::Zone2));
    }

    #[test]
    fn the_mapping_covers_every_zone_exactly_once_with_distinct_chip_selects() {
        assert_eq!(CS_FOR_ZONE.len(), ZoneId::ALL.len());
        for zone in ZoneId::ALL {
            let entries: Vec<_> = CS_FOR_ZONE.iter().filter(|(z, _)| *z == zone).collect();
            assert_eq!(entries.len(), 1, "{zone} appears {} times", entries.len());
            assert_eq!(entries[0].1, chip_select_for(zone));
        }
        assert_ne!(CS_FOR_ZONE[0].1, CS_FOR_ZONE[1].1);
    }

    #[test]
    fn the_expansion_selects_do_not_overlap_the_zones() {
        for cs in EXPANSION_CS {
            assert!(
                !CS_FOR_ZONE.iter().any(|(_, c)| *c == cs),
                "{cs} is already a zone's"
            );
        }
        assert_ne!(EXPANSION_CS[0], EXPANSION_CS[1]);
    }

    /// The names in `deploy/kdtvd.toml` are `spi0.0` and `spi0.1`. If they ever
    /// disagree with the constant, the daemon refuses to start.
    #[test]
    fn configuration_may_restate_the_mapping_but_not_change_it() {
        assert_eq!(
            ChipSelect::check(ZoneId::Zone1, "spi0.0").unwrap(),
            ChipSelect::Spi0Ce0
        );
        assert_eq!(
            ChipSelect::check(ZoneId::Zone2, " spi0.1 ").unwrap(),
            ChipSelect::Spi0Ce1
        );
        // The swap, which is the mistake worth catching.
        let err = ChipSelect::check(ZoneId::Zone1, "spi0.1").unwrap_err();
        assert!(matches!(err, RtdError::ChipSelectMismatch { .. }));
        assert_eq!(err.zone(), ZoneId::Zone1);
        assert!(err.to_string().contains("swapped"), "{err}");
        assert!(ChipSelect::check(ZoneId::Zone2, "spi1.0").is_err());
    }

    #[test]
    fn the_fault_register_keeps_its_raw_byte_and_names_what_it_knows() {
        let clear = FaultRegister::from_bits(0);
        assert!(clear.is_clear());
        assert!(clear.named().is_empty());
        assert_eq!(clear.to_string(), "clear");

        // An open FORCE- line, which is what a probe that has fallen off looks
        // like electrically.
        let open = FaultRegister::from_bits(FaultRegister::RTDIN_LOW_FORCE_OPEN);
        assert!(!open.is_clear());
        assert!(open.indicates_wiring_fault());
        assert_eq!(open.named(), vec!["rtdin_low_force_open"]);

        // Bits are independent and arrive together.
        let both = FaultRegister::from_bits(
            FaultRegister::RTD_HIGH_THRESHOLD | FaultRegister::UNDER_OVER_VOLTAGE,
        );
        assert_eq!(both.named().len(), 2);
        assert!(both.indicates_wiring_fault());

        // An unrecognised bit is not lost.
        let odd = FaultRegister::from_bits(0x02);
        assert_eq!(odd.bits(), 0x02);
        assert!(!odd.is_clear());
        assert!(odd.named().is_empty());
        assert!(format!("{odd:?}").contains("0x02"));
    }

    /// A threshold bit is not a wiring fault: it says the temperature is outside
    /// the configured window, which is a different event.
    #[test]
    fn a_threshold_bit_alone_is_not_a_wiring_fault() {
        assert!(
            !FaultRegister::from_bits(FaultRegister::RTD_HIGH_THRESHOLD).indicates_wiring_fault()
        );
        assert!(
            !FaultRegister::from_bits(FaultRegister::RTD_LOW_THRESHOLD).indicates_wiring_fault()
        );
    }

    #[test]
    fn a_sample_cannot_be_built_without_its_fault_register() {
        // Not an assertion about behaviour — an assertion about the shape. If
        // `fault` ever becomes optional, this stops compiling.
        let s = RtdSample {
            zone: ZoneId::Zone1,
            raw: RawC(38.5),
            fault: FaultRegister::from_bits(0),
            at: Monotonic::from_nanos(1),
        };
        assert_eq!(s.raw.0, 38.5);
        assert!(s.fault.is_clear());
    }
}

/// An independent temperature channel that reads a file.
///
/// # Why this exists
///
/// No `RtdChannel` for the MAX31865 has been written: an SPI transfer needs
/// either `unsafe`, which this workspace denies, or a driver crate that is
/// deliberately absent from this one's graph. So on a bench there is nothing to
/// read — and `kdtv-service` refuses to start a zone with no channel, because
/// the interlock covers the instrumented outlet and nothing else covers it.
///
/// A stub returning a plausible number would satisfy that check while removing
/// the only measurement in the system that does not come from the valve, which
/// is the entire point of the channel. This reads a real number that something
/// else wrote, so the emulated daemon exercises the same escalation paths as
/// hardware would: a value past the corrected trip, a value past the raw
/// backstop, a gap past the starvation window, a fault register with bits in it.
///
/// # It cannot reach a real bathroom
///
/// The only route to one is
/// [`ValidatedConfig::bench_probe_dir`](kdtv_config::ValidatedConfig::bench_probe_dir),
/// which reads a key in the `[bench]` table, and a `[bench]` table under
/// `profile = "production"` is refused outright by `kdtv-config`. There is no
/// other key for it. Denial by absence, as with the port placeholders.
///
/// # The file format
///
/// One line: a Celsius reading, optionally followed by whitespace and a
/// hexadecimal fault register.
///
/// ```text
/// 38.5
/// 38.5 0x04
/// ```
///
/// An unreadable or unparseable file is [`RtdError::Transfer`] — the same error
/// a failed SPI transfer produces, because to everything above this it is the
/// same condition: the channel did not answer. It is deliberately *not*
/// silently treated as a missing sample, which would look like starvation and
/// would take five seconds to escalate instead of one.
#[derive(Debug)]
pub struct FileRtdChannel {
    zone: ZoneId,
    path: PathBuf,
    clock: Arc<dyn Clock>,
}

impl FileRtdChannel {
    /// The file this channel reads for `zone`, under `dir`.
    ///
    /// Named for the zone rather than the chip select, because a bench has no
    /// SPI bus and naming it after one would invite the mistake the chip-select
    /// check exists to catch.
    #[must_use]
    pub fn path_for(dir: &Path, zone: ZoneId) -> PathBuf {
        dir.join(format!("{zone}.degc"))
    }

    #[must_use]
    pub fn new(zone: ZoneId, dir: &Path, clock: Arc<dyn Clock>) -> Self {
        Self {
            zone,
            path: Self::path_for(dir, zone),
            clock,
        }
    }

    /// The file this channel reads. The harness writes it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn parse(text: &str) -> Result<(RawC, FaultRegister), std::io::Error> {
        let mut parts = text.split_whitespace();
        let celsius: f32 = parts
            .next()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "empty"))?
            .parse()
            .map_err(|e| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("not a Celsius reading: {e}"),
                )
            })?;
        if !celsius.is_finite() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "not a temperature",
            ));
        }
        let fault = match parts.next() {
            None => FaultRegister::default(),
            Some(bits) => {
                let hex = bits.strip_prefix("0x").unwrap_or(bits);
                FaultRegister::from_bits(u8::from_str_radix(hex, 16).map_err(|e| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("not a fault register: {e}"),
                    )
                })?)
            }
        };
        Ok((RawC(celsius), fault))
    }
}

impl RtdChannel for FileRtdChannel {
    fn zone(&self) -> ZoneId {
        self.zone
    }

    fn sample(&mut self) -> BoxedFuture<'_, Result<RtdSample, RtdError>> {
        Box::pin(async move {
            let text =
                std::fs::read_to_string(&self.path).map_err(|source| RtdError::Transfer {
                    zone: self.zone,
                    chip_select: self.chip_select(),
                    source,
                })?;
            let (raw, fault) = Self::parse(&text).map_err(|source| RtdError::Transfer {
                zone: self.zone,
                chip_select: self.chip_select(),
                source,
            })?;
            Ok(RtdSample {
                zone: self.zone,
                raw,
                fault,
                at: self.clock.monotonic(),
            })
        })
    }
}
