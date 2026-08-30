//! Framing, addressing and the checksum.
//!
//! All tier `[C]`, from `research/xagon0/docs/protocols/saturn-protocol.md`
//! § Frame Format, § Addressing and § Checksum Calculation.

use core::fmt;

/// First synchronisation byte. `FRAME-01`.
pub const SYNC1: u8 = 0xAA;
/// Second synchronisation byte. `FRAME-01`.
pub const SYNC2: u8 = 0x55;
/// The pair the receiver resynchronises on. `FRAME-04`.
pub const SYNC: [u8; 2] = [SYNC1, SYNC2];

/// Bytes before the `DATA` field: two sync, address, control, `DATA_LEN`.
pub const HEADER_LEN: usize = 5;
/// Non-`DATA` bytes in a frame: [`HEADER_LEN`] plus the checksum. Total frame
/// length is `FRAME_OVERHEAD + DATA_LEN`.
pub const FRAME_OVERHEAD: usize = 6;
/// Maximum on-wire frame length. `PROTO-03` / `PHY-02`.
pub const MAX_FRAME: usize = 20;
/// Largest legal `DATA_LEN`, derived from [`MAX_FRAME`] and
/// [`FRAME_OVERHEAD`]. `PHY-02`.
pub const MAX_DATA_LEN: u8 = 14;
/// The same bound as a `usize`, for buffer sizes and const generics.
///
/// Computed from the frame arithmetic rather than cast from [`MAX_DATA_LEN`]:
/// `as` conversions are denied workspace-wide, and
/// `max_data_len_agrees_in_both_widths` pins the two together.
pub const MAX_DATA: usize = MAX_FRAME - FRAME_OVERHEAD;

/// All valves on the bus. `ADDR-01`.
pub const BROADCAST: u8 = 0x0F;
/// Lowest assignable valve address. `ADDR-02`.
pub const VALVE_ADDR_MIN: u8 = 0x03;
/// Highest assignable valve address. `ADDR-02`.
pub const VALVE_ADDR_MAX: u8 = 0x07;

/// The checksum over `ADDRESS + CONTROL + DATA_LEN + DATA`, as the 8-bit two's
/// complement of their sum. `CHK-01`.
///
/// **`SYNC1` and `SYNC2` are excluded.** This is not a stylistic choice: the
/// documented response `AA 55 00 02 01 1E DF` carries checksum `0xDF`, which is
/// only correct with the sync bytes omitted. Including them yields `0xE0`.
/// `CHK-02`, and `checksum_excludes_sync_bytes` proves it.
///
/// Wrapping arithmetic throughout — the sum is defined modulo 256, and
/// `wrapping_neg` is exactly `(!total + 1) & 0xFF`.
#[must_use]
pub const fn checksum(address: u8, control: u8, data_len: u8, data: &[u8]) -> u8 {
    let mut total = address.wrapping_add(control).wrapping_add(data_len);
    let mut rest = data;
    // A `const fn` cannot use an iterator; `split_first` walks the slice without
    // indexing it.
    while let Some((byte, tail)) = rest.split_first() {
        total = total.wrapping_add(*byte);
        rest = tail;
    }
    total.wrapping_neg()
}

/// True when the covered bytes and the checksum sum to zero modulo 256.
/// `CHK-03`.
#[must_use]
pub const fn checksum_valid(
    address: u8,
    control: u8,
    data_len: u8,
    data: &[u8],
    found: u8,
) -> bool {
    checksum(address, control, data_len, data) == found
}

/// A dynamically assigned valve address, restricted to `0x03..=0x07`.
/// `ADDR-02`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValveAddr(u8);

impl ValveAddr {
    /// Every address the master may allocate, in scan order. `DISC-07`.
    pub const ALL: [Self; 5] = [Self(0x03), Self(0x04), Self(0x05), Self(0x06), Self(0x07)];

    /// Rejects the broadcast address, either master address, and everything
    /// else outside `0x03..=0x07`.
    pub fn new(v: u8) -> Result<Self, AddrError> {
        if !(VALVE_ADDR_MIN..=VALVE_ADDR_MAX).contains(&v) {
            return Err(AddrError::OutOfRange { requested: v });
        }
        Ok(Self(v))
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for ValveAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ValveAddr(0x{:02X})", self.0)
    }
}

impl fmt::Display for ValveAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02X}", self.0)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum AddrError {
    #[error("valve address 0x{requested:02X} is outside the assignable range 0x03..=0x07")]
    OutOfRange { requested: u8 },
}

/// Which master identity this link speaks as.
///
/// **The sources contradict each other and this crate does not decide.**
///
/// - `research/xagon0/docs/protocols/saturn-protocol.md` § Addressing makes the
///   master address a property of the *controller*: DTV controllers use `0x00`,
///   Prompt controllers `0x10`, and "when integrating with DTV+ hardware,
///   always use `0x00`".
/// - `docs/devices/valve-control.md` § Master Address Selection makes it a
///   property of the *valve*: a Prompt 3-Port "always" answers `0x10`, and
///   sending `0x00` to one gets "no response".
///
/// This installation is exactly the irreconcilable case — a DTV+ master driving
/// a Prompt 3 valve on zone 2. Evidence exists on both sides from inside the
/// same corpus: `saturn-protocol.md`'s own Example 1 shows a valve reporting
/// firmware type `0x1E` (Prompt 3-Port) answering with `ADDRESS` `0x00`. That is
/// one third-party capture from unknown hardware, so it is inference `[I]`, not
/// a decision.
///
/// Tracked as `INVESTIGATIONS.md` I5 and packet-capture question 1. The value is
/// therefore per-zone configuration with a [`MasterAddr::Dtv`] default, and
/// discovery is expected to retry the whole enquiry sequence at
/// [`MasterAddr::Prompt`] before declaring a bus empty. `ADDR-03` / `ADDR-04`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum MasterAddr {
    /// `0x00` — the DTV controller identity.
    #[default]
    Dtv,
    /// `0x10` — the Prompt controller identity.
    Prompt,
}

impl MasterAddr {
    /// Both identities, in the order discovery should try them.
    pub const ALL: [Self; 2] = [Self::Dtv, Self::Prompt];

    /// An explicit `match`, not a `#[repr(u8)]` cast — `as` conversions are
    /// denied workspace-wide and a discriminant cast is exactly the kind of
    /// silent widening the ban exists for.
    #[must_use]
    pub const fn byte(self) -> u8 {
        match self {
            Self::Dtv => 0x00,
            Self::Prompt => 0x10,
        }
    }

    /// The other identity, for the discovery retry described on this type.
    #[must_use]
    pub const fn alternate(self) -> Self {
        match self {
            Self::Dtv => Self::Prompt,
            Self::Prompt => Self::Dtv,
        }
    }
}

impl fmt::Display for MasterAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02X}", self.byte())
    }
}

/// Which way a frame travelled.
///
/// Inferred from the `ADDRESS` field and the frame's content — **never from an
/// echo.** The converters selected for this build have automatic direction
/// control and present no local echo at all, so an echo-derived direction would
/// be inferring from a signal that does not exist. See `CORRECTIONS.md` item 3
/// and `PROTO-04` / `PROTO-06`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum Direction {
    /// `ADDRESS` is a valve address or the broadcast address.
    MasterToValve,
    /// `ADDRESS` is the configured master address. `FRAME-03`: the field is the
    /// *destination*, so a valve's reply carries the master's address, not its
    /// own. `saturn-protocol.md` § Response Validation step 2 says to check it
    /// "matches expected sender", which would reject every real response — both
    /// captured replies in that same document carry `ADDRESS` `0x00`.
    ValveToMaster,
    /// Neither. A capture can contain these; a live link should not.
    Indeterminate,
}

impl Direction {
    /// The only direction rule in the crate. `master` is the configured
    /// identity for this link, because `0x00` and `0x10` are both plausible.
    #[must_use]
    pub fn infer(address: u8, master: MasterAddr) -> Self {
        if address == master.byte() {
            Self::ValveToMaster
        } else if address == BROADCAST || (VALVE_ADDR_MIN..=VALVE_ADDR_MAX).contains(&address) {
            Self::MasterToValve
        } else {
            Self::Indeterminate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CHK-02`. The single arithmetic fact that pins the algorithm down.
    #[test]
    fn checksum_excludes_sync_bytes() {
        // AA 55 00 02 01 1E DF — the documented read-firmware-type response.
        let with_sync_excluded = checksum(0x00, 0x02, 0x01, &[0x1E]);
        assert_eq!(with_sync_excluded, 0xDF);

        // Folding the sync bytes into the covered range gives 0xE0, which is
        // not what the wire carries. This is the whole proof.
        let with_sync_included = checksum(SYNC1, SYNC2, 0x00, &[0x02, 0x01, 0x1E]);
        assert_eq!(with_sync_included, 0xE0);
        assert_ne!(with_sync_included, 0xDF);

        assert!(checksum_valid(0x00, 0x02, 0x01, &[0x1E], 0xDF));
        assert!(!checksum_valid(0x00, 0x02, 0x01, &[0x1E], 0xE0));
    }

    #[test]
    fn checksum_worked_examples_from_the_documents() {
        // Read firmware type, valve 0x03. saturn-protocol.md Example 1 prints
        // 0xA1 on its first TX line and then corrects itself to 0xFB. CHK-04:
        // the stale value must never appear.
        assert_eq!(checksum(0x03, 0x02, 0x00, &[]), 0xFB);
        assert_ne!(checksum(0x03, 0x02, 0x00, &[]), 0xA1);

        // Write outlet states, Prompt 3 outlet 1. Example 2, correct as printed.
        assert_eq!(checksum(0x03, 0x87, 0x02, &[0x04, 0x00]), 0x70);

        // Address clear broadcast. Example 3 prints 0xAE, then corrects to 0xB3.
        assert_eq!(checksum(0x0F, 0x3A, 0x01, &[0x03]), 0xB3);
        assert_ne!(checksum(0x0F, 0x3A, 0x01, &[0x03]), 0xAE);

        // Derived frames, checksums recomputed in ARITHMETIC-NOTES.md.
        assert_eq!(checksum(0x0F, 0x3A, 0x01, &[0x01]), 0xB5);
        assert_eq!(checksum(0x0F, 0x3A, 0x02, &[0x02, 0x03]), 0xB0);
        assert_eq!(checksum(0x03, 0x01, 0x00, &[]), 0xFC);
        assert_eq!(checksum(0x03, 0x0B, 0x00, &[]), 0xF2);
        assert_eq!(checksum(0x03, 0x0F, 0x00, &[]), 0xEE);
        assert_eq!(checksum(0x00, 0x3A, 0x00, &[]), 0xC6);
    }

    #[test]
    fn frame_length_formula_matches_the_constants() {
        assert_eq!(FRAME_OVERHEAD, HEADER_LEN + 1);
        assert_eq!(usize::from(MAX_DATA_LEN) + FRAME_OVERHEAD, MAX_FRAME);
    }

    #[test]
    fn max_data_len_agrees_in_both_widths() {
        assert_eq!(usize::from(MAX_DATA_LEN), MAX_DATA);
        assert_eq!(MAX_DATA, 14);
    }

    #[test]
    fn valve_addr_accepts_only_the_assignable_range() {
        for v in 0u8..=255 {
            let ok = ValveAddr::new(v).is_ok();
            assert_eq!(ok, (0x03..=0x07).contains(&v), "byte 0x{v:02X}");
        }
        assert!(ValveAddr::new(BROADCAST).is_err());
        assert!(ValveAddr::new(MasterAddr::Dtv.byte()).is_err());
        assert!(ValveAddr::new(MasterAddr::Prompt.byte()).is_err());
        assert_eq!(ValveAddr::ALL.len(), 5);
    }

    #[test]
    fn master_addresses_are_the_two_documented_bytes() {
        assert_eq!(MasterAddr::Dtv.byte(), 0x00);
        assert_eq!(MasterAddr::Prompt.byte(), 0x10);
        assert_eq!(MasterAddr::default(), MasterAddr::Dtv);
        assert_eq!(MasterAddr::Dtv.alternate(), MasterAddr::Prompt);
        assert_eq!(MasterAddr::Prompt.alternate(), MasterAddr::Dtv);
    }

    /// The whole point of `MasterAddr` being configuration: `0x10` is a valve
    /// reply under one identity and nothing at all under the other.
    #[test]
    fn direction_depends_on_the_configured_master_identity() {
        assert_eq!(
            Direction::infer(0x00, MasterAddr::Dtv),
            Direction::ValveToMaster
        );
        assert_eq!(
            Direction::infer(0x00, MasterAddr::Prompt),
            Direction::Indeterminate
        );
        assert_eq!(
            Direction::infer(0x10, MasterAddr::Prompt),
            Direction::ValveToMaster
        );
        assert_eq!(
            Direction::infer(0x10, MasterAddr::Dtv),
            Direction::Indeterminate
        );
        for v in VALVE_ADDR_MIN..=VALVE_ADDR_MAX {
            assert_eq!(
                Direction::infer(v, MasterAddr::Dtv),
                Direction::MasterToValve
            );
        }
        assert_eq!(
            Direction::infer(BROADCAST, MasterAddr::Dtv),
            Direction::MasterToValve
        );
        assert_eq!(
            Direction::infer(0x42, MasterAddr::Dtv),
            Direction::Indeterminate
        );
    }
}
