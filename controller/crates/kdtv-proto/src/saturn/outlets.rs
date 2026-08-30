//! The outlet mapping table — three numbering schemes that do not agree.
//!
//! | Space | Where it appears | Type here |
//! | --- | --- | --- |
//! | Configuration slot | `one_type`..`six_type`, `valveN_outletM_func`, `quick_shower.cgi` digits | [`kdtv_units::Slot`] |
//! | Status index | `system_info.cgi`'s `valveNoutletM` booleans, bridged by `valveN_outletM_func.id` | [`OutletMapping::status_index`] |
//! | Wire bitmap | the byte sent to the valve; **differs by valve family** | [`OutletBitmap`] |
//!
//! The wire bitmap is the trap. A DTV 6-Port numbers its outlets 0..5 in bits
//! 0..5, so its first outlet is mask `0x01`. A Prompt 3 numbers its outlets 1..6
//! in bits 2..7, so its first outlet is mask `0x04`. The two are shifted by two
//! bits *and* based differently, which means mask `0x04` is outlet 2 on one
//! valve and outlet 1 on the other. This installation has one of each.
//!
//! `research/FIELD-NOTES.md` § 2 records what happens when these spaces are
//! conflated: a shipped Hubitat driver dereferenced a null walking this
//! mapping, and a Home Assistant integration turned on outlet 6 when the user
//! asked for outlet 2. On this installation the slot-to-status mapping happens
//! to be the identity, so a test built from the reference configuration does not
//! execute the interesting path at all. [`permuted_slots_reach_the_right_outlet`]
//! permutes a synthetic configuration for exactly that reason.
//!
//! [`permuted_slots_reach_the_right_outlet`]: self#tests

use crate::saturn::control::FirmwareType;
use core::fmt;
use kdtv_units::{Slot, SlotSet};

/// The valve families this master speaks to, and the only thing that selects an
/// outlet bitmap.
///
/// Derived from the firmware type ID the valve reports at discovery
/// ([`ValveType::from_firmware`]), never from a configuration guess — a
/// mis-configured family opens the wrong outlet silently.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ValveType {
    /// Firmware type `0x06`. Outlets numbered 0..5, masks `0x01`..`0x20`.
    Dtv6Port,
    /// Firmware type `0x17`. Two outlets. No source gives its bitmap; it is
    /// treated as the Prompt generic layout truncated to two outlets, which is
    /// inference `[I]` and is not exercised by this installation.
    Prompt2Port,
    /// Firmware type `0x1E`. Outlets numbered 1..6, masks `0x04`..`0x80`.
    Prompt3Port,
    /// Firmware type `0xFF`. As [`ValveType::Prompt3Port`], plus flow control.
    Prompt3FlowControl,
}

impl ValveType {
    pub const ALL: [Self; 4] = [
        Self::Dtv6Port,
        Self::Prompt2Port,
        Self::Prompt3Port,
        Self::Prompt3FlowControl,
    ];

    /// The only supported route from a discovered valve to a bitmap layout.
    /// `Null` and `Unknown` produce `None`: a valve that did not say what it is
    /// does not get commanded.
    #[must_use]
    pub const fn from_firmware(ft: FirmwareType) -> Option<Self> {
        match ft {
            FirmwareType::Dtv6Port => Some(Self::Dtv6Port),
            FirmwareType::Prompt2Port => Some(Self::Prompt2Port),
            FirmwareType::Prompt3Port => Some(Self::Prompt3Port),
            FirmwareType::Prompt3FlowControl => Some(Self::Prompt3FlowControl),
            FirmwareType::Null | FirmwareType::Unknown(_) => None,
        }
    }

    /// The lowest outlet number in this family's own numbering: 0 for DTV,
    /// 1 for Prompt.
    #[must_use]
    pub const fn first_outlet(self) -> u8 {
        match self {
            Self::Dtv6Port => 0,
            Self::Prompt2Port | Self::Prompt3Port | Self::Prompt3FlowControl => 1,
        }
    }

    /// The mask of [`ValveType::first_outlet`]: `0x01` for DTV, `0x04` for
    /// Prompt.
    #[must_use]
    pub const fn first_mask(self) -> u8 {
        match self {
            Self::Dtv6Port => 0x01,
            Self::Prompt2Port | Self::Prompt3Port | Self::Prompt3FlowControl => 0x04,
        }
    }

    /// How many mask positions the family's bitmap defines.
    ///
    /// Six for both DTV 6-Port and the Prompt 3 generic layout — the Prompt 3
    /// table defines six mask slots even though the valve has three ports.
    /// Whether outlets 4..6 exist on a given valve is a question for the
    /// configuration, not for the bitmap.
    #[must_use]
    pub const fn mask_positions(self) -> u8 {
        match self {
            Self::Prompt2Port => 2,
            Self::Dtv6Port | Self::Prompt3Port | Self::Prompt3FlowControl => 6,
        }
    }

    /// The number of physical ports the valve is documented to have. Not the
    /// same as [`ValveType::mask_positions`].
    #[must_use]
    pub const fn ports(self) -> u8 {
        match self {
            Self::Dtv6Port => 6,
            Self::Prompt2Port => 2,
            Self::Prompt3Port | Self::Prompt3FlowControl => 3,
        }
    }

    /// **The one mapping function.** Wire outlet number to wire mask, in this
    /// family's own numbering.
    ///
    /// `OUT-01`. There is no second implementation of this, and no shared table
    /// between the families.
    pub fn mask_for_outlet(self, outlet: u8) -> Result<u8, OutletError> {
        let first = self.first_outlet();
        let last = first
            .checked_add(self.mask_positions())
            .and_then(|n| n.checked_sub(1))
            .ok_or(OutletError::OutletOutOfRange {
                valve: self,
                outlet,
            })?;
        if outlet < first || outlet > last {
            return Err(OutletError::OutletOutOfRange {
                valve: self,
                outlet,
            });
        }
        let bit = outlet - first + self.first_mask().trailing_zeros_u8();
        1u8.checked_shl(u32::from(bit))
            .ok_or(OutletError::OutletOutOfRange {
                valve: self,
                outlet,
            })
    }

    /// The inverse, for reading a capture back. `None` when the mask has no
    /// outlet in this family or names more than one.
    #[must_use]
    pub fn outlet_for_mask(self, mask: u8) -> Option<u8> {
        if mask.count_ones() != 1 {
            return None;
        }
        (self.first_outlet()..)
            .take(usize::from(self.mask_positions()))
            .find(|o| self.mask_for_outlet(*o) == Ok(mask))
    }
}

/// A tiny helper so [`ValveType::mask_for_outlet`] reads as arithmetic rather
/// than as a cast.
trait TrailingZerosU8 {
    fn trailing_zeros_u8(self) -> u8;
}

impl TrailingZerosU8 for u8 {
    fn trailing_zeros_u8(self) -> u8 {
        // `u8::trailing_zeros` returns 0..=8, which always fits.
        u8::try_from(self.trailing_zeros()).unwrap_or(8)
    }
}

impl fmt::Display for ValveType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Dtv6Port => "DTV 6-Port",
            Self::Prompt2Port => "Prompt 2-Port",
            Self::Prompt3Port => "Prompt 3-Port",
            Self::Prompt3FlowControl => "Prompt 3 Flow Control",
        })
    }
}

/// A wire outlet bitmap, tagged with the valve family it was built for.
///
/// The field is private and the only constructor is [`OutletTable::bitmap`], so
/// a bitmap cannot exist without a table having chosen the family. The encoder
/// re-checks the tag before it emits, which turns "these two bitmaps look alike"
/// from a silent wrong-outlet into a rejected encode.
///
/// The architecture proposed two newtypes with no `From` between them
/// (`DtvOutletMask`, `PromptOutletMask`). One tagged type was chosen instead
/// because the frame layout is identical and the tag survives into the encoder,
/// where the check actually has to happen; two newtypes would have needed a
/// generic encoder or a conversion, and the conversion is the hazard.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct OutletBitmap {
    valve: ValveType,
    bits: u8,
}

impl OutletBitmap {
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.bits
    }

    #[must_use]
    pub const fn valve(self) -> ValveType {
        self.valve
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.bits == 0
    }

    /// The all-off bitmap for a family. Turning water off is never gated, so
    /// this is the one bitmap that can be built without a slot set.
    #[must_use]
    pub const fn all_off(valve: ValveType) -> Self {
        Self { valve, bits: 0 }
    }
}

impl fmt::Debug for OutletBitmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OutletBitmap({}, 0x{:02X})", self.valve, self.bits)
    }
}

bitflags::bitflags! {
    /// The write-primary flag byte — `DATA[1]` of a `0x87` write.
    ///
    /// A distinct type from [`OutletBitmap`] because the two collide
    /// numerically: `FULL_COLD` is `0x04` and so is Prompt 3 outlet 1;
    /// `DISINFECT` is `0x40` and so is Prompt 3 outlet 5. `FLAG-02`.
    ///
    /// Bits `0x08`, `0x10` and `0x80` are undefined and `from_bits` rejects
    /// them, so an undefined bit cannot reach the wire. `FLAG-01`.
    ///
    /// **Unresolved `[?]`.** `OUT-04`: the only captured outlet-open frame,
    /// `saturn-protocol.md` Example 2, carries `DATA[1]` = `0x00` and is
    /// annotated "no flags" while claiming to turn outlet 1 on. If `ON` were
    /// required here that frame would not work. The two documents may be
    /// describing two different fields — `0x87` outlet states versus a separate
    /// primary-register write. The encoder emits what was captured;
    /// [`PrimaryFlags::CAPTURED`] is that value.
    #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
    pub struct PrimaryFlags: u8 {
        const ON = 0x01;
        const PAUSE = 0x02;
        const FULL_COLD = 0x04;
        const DUTY_FLUSH = 0x20;
        const DISINFECT = 0x40;
    }
}

impl PrimaryFlags {
    /// `0x00` — the value in the only frame with evidence behind it.
    pub const CAPTURED: Self = Self::empty();
    /// The bits no source defines. Never written.
    pub const UNDEFINED_BITS: u8 = 0x08 | 0x10 | 0x80;
}

bitflags::bitflags! {
    /// The valve state byte a valve reports. `FLAG-03`.
    ///
    /// Only two bits are documented. The rest are reserved or valve-family
    /// specific and are carried verbatim rather than interpreted.
    #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
    pub struct ValveStateBits: u8 {
        /// Bit 1. Outputs paused, holding current state.
        const PAUSE = 0x02;
        /// Bit 7. An active error condition — a trigger to read the fault
        /// register, not the fault itself. `FLAG-04`.
        const ERROR = 0x80;
    }
}

/// One configuration slot's place in the other two numbering spaces.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct OutletMapping {
    /// The configuration slot, 1..=6. The only number the public API speaks.
    pub slot: Slot,
    /// The index `system_info.cgi` reports this outlet's state under, from
    /// `valveN_outletM_func.id`. **Not** the slot number, even though it is on
    /// this installation.
    pub status_index: u8,
    /// The outlet number in the valve family's own numbering — 0-based on a
    /// DTV 6-Port, 1-based on a Prompt. Converted to a mask by
    /// [`ValveType::mask_for_outlet`].
    pub wire_outlet: u8,
}

/// The mapping table for one valve. One table, built once from configuration.
#[derive(Clone, Debug)]
pub struct OutletTable {
    valve: ValveType,
    /// Indexed by [`Slot::index`]. `None` means the slot is not configured, and
    /// an unconfigured slot is refused rather than defaulted — `CLAMP-05`.
    entries: [Option<OutletMapping>; 6],
}

impl OutletTable {
    /// Builds the table, rejecting duplicates and out-of-range wire outlets up
    /// front so the encoder never has to.
    pub fn new(
        valve: ValveType,
        mappings: impl IntoIterator<Item = OutletMapping>,
    ) -> Result<Self, OutletError> {
        let mut entries: [Option<OutletMapping>; 6] = [None; 6];
        for m in mappings {
            // Validates the wire outlet against the family's own numbering.
            valve.mask_for_outlet(m.wire_outlet)?;
            let cell = entries
                .get_mut(m.slot.index())
                .ok_or(OutletError::UnconfiguredSlot(m.slot))?;
            if cell.is_some() {
                return Err(OutletError::DuplicateSlot(m.slot));
            }
            *cell = Some(m);
        }
        // Two slots pointing at one physical outlet would make "outlet 3 is on"
        // unanswerable.
        for (i, a) in entries.iter().enumerate() {
            for b in entries.iter().skip(i + 1) {
                if let (Some(a), Some(b)) = (a, b) {
                    if a.wire_outlet == b.wire_outlet {
                        return Err(OutletError::DuplicateWireOutlet {
                            valve,
                            outlet: a.wire_outlet,
                        });
                    }
                    if a.status_index == b.status_index {
                        return Err(OutletError::DuplicateStatusIndex(a.status_index));
                    }
                }
            }
        }
        Ok(Self { valve, entries })
    }

    #[must_use]
    pub const fn valve(&self) -> ValveType {
        self.valve
    }

    /// Every configured slot, ascending.
    pub fn configured(&self) -> impl Iterator<Item = &OutletMapping> {
        self.entries.iter().filter_map(Option::as_ref)
    }

    /// The set of slots this valve has configured. Anything outside it is
    /// refused, not defaulted.
    #[must_use]
    pub fn configured_slots(&self) -> SlotSet {
        self.configured().map(|m| m.slot).collect()
    }

    pub fn mapping(&self, slot: Slot) -> Result<&OutletMapping, OutletError> {
        self.entries
            .get(slot.index())
            .and_then(Option::as_ref)
            .ok_or(OutletError::UnconfiguredSlot(slot))
    }

    /// **The one function that crosses from slots to the wire.** `OUT-01` /
    /// `OUT-02`: the masks of the selected slots, OR-ed together.
    ///
    /// Fails on any slot this valve has not configured rather than skipping it,
    /// so a request for an outlet that does not exist is an error the operator
    /// sees, not a shower that half starts.
    pub fn bitmap(&self, slots: SlotSet) -> Result<OutletBitmap, OutletError> {
        let mut bits = 0u8;
        for slot in slots.iter() {
            let m = self.mapping(slot)?;
            bits |= self.valve.mask_for_outlet(m.wire_outlet)?;
        }
        Ok(OutletBitmap {
            valve: self.valve,
            bits,
        })
    }

    /// The reverse, for decoding a reported outlet-state byte. Bits with no
    /// configured slot behind them are returned separately rather than dropped —
    /// a valve reporting an outlet this table does not know about is a finding,
    /// not noise.
    #[must_use]
    pub fn slots_from_bits(&self, bits: u8) -> (SlotSet, u8) {
        let mut set = SlotSet::EMPTY;
        let mut unaccounted = bits;
        for m in self.configured() {
            if let Ok(mask) = self.valve.mask_for_outlet(m.wire_outlet)
                && bits & mask != 0
            {
                set = set.insert(m.slot);
                unaccounted &= !mask;
            }
        }
        (set, unaccounted)
    }

    /// Slot to status index. `STATE-01`: always bridged, never assumed equal.
    pub fn status_index(&self, slot: Slot) -> Result<u8, OutletError> {
        Ok(self.mapping(slot)?.status_index)
    }

    /// Status index back to slot, for reading `system_info.cgi`-shaped data.
    #[must_use]
    pub fn slot_for_status_index(&self, index: u8) -> Option<Slot> {
        self.configured()
            .find(|m| m.status_index == index)
            .map(|m| m.slot)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum OutletError {
    #[error("outlet {outlet} is outside the {valve} bitmap")]
    OutletOutOfRange { valve: ValveType, outlet: u8 },
    #[error("configuration slot {0} is not configured on this valve")]
    UnconfiguredSlot(Slot),
    #[error("configuration slot {0} appears twice in the outlet table")]
    DuplicateSlot(Slot),
    #[error("two slots map to {valve} wire outlet {outlet}")]
    DuplicateWireOutlet { valve: ValveType, outlet: u8 },
    #[error("two slots map to status index {0}")]
    DuplicateStatusIndex(u8),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(n: u8) -> Slot {
        Slot::new(n).unwrap()
    }

    fn set(ns: &[u8]) -> SlotSet {
        ns.iter().map(|n| slot(*n)).collect()
    }

    /// The reference configuration for zone 1: a six-port valve with five
    /// configured outlets, slot number equal to status index equal to
    /// `wire_outlet + 1`. This is the identity case — see the permutation test
    /// below for why it is not enough on its own.
    fn zone1_table() -> OutletTable {
        OutletTable::new(
            ValveType::Dtv6Port,
            (1u8..=5).map(|n| OutletMapping {
                slot: slot(n),
                status_index: n,
                wire_outlet: n - 1,
            }),
        )
        .unwrap()
    }

    /// Zone 2: a three-port Prompt valve, three configured outlets, 1-based
    /// wire numbering.
    fn zone2_table() -> OutletTable {
        OutletTable::new(
            ValveType::Prompt3Port,
            (1u8..=3).map(|n| OutletMapping {
                slot: slot(n),
                status_index: n,
                wire_outlet: n,
            }),
        )
        .unwrap()
    }

    /// `OUT-01`. The single sentence that makes the two tables worth having.
    #[test]
    fn mask_0x04_is_a_different_outlet_on_each_valve() {
        assert_eq!(ValveType::Dtv6Port.outlet_for_mask(0x04), Some(2));
        assert_eq!(ValveType::Prompt3Port.outlet_for_mask(0x04), Some(1));
        assert_eq!(ValveType::Dtv6Port.mask_for_outlet(2), Ok(0x04));
        assert_eq!(ValveType::Prompt3Port.mask_for_outlet(1), Ok(0x04));

        // And the same again one bit up, so this is a shift and not a one-off.
        assert_eq!(ValveType::Dtv6Port.outlet_for_mask(0x20), Some(5));
        assert_eq!(ValveType::Prompt3Port.outlet_for_mask(0x20), Some(4));
    }

    #[test]
    fn documented_bitmaps_reproduce_exactly() {
        // docs/devices/valve-control.md — DTV 6-Port Outlets.
        for (outlet, mask) in [
            (0, 0x01),
            (1, 0x02),
            (2, 0x04),
            (3, 0x08),
            (4, 0x10),
            (5, 0x20),
        ] {
            assert_eq!(ValveType::Dtv6Port.mask_for_outlet(outlet), Ok(mask));
        }
        assert!(ValveType::Dtv6Port.mask_for_outlet(6).is_err());

        // saturn-protocol.md — Generic Outlet Bitmaps (Prompt 3).
        for (outlet, mask) in [
            (1, 0x04),
            (2, 0x08),
            (3, 0x10),
            (4, 0x20),
            (5, 0x40),
            (6, 0x80),
        ] {
            assert_eq!(ValveType::Prompt3Port.mask_for_outlet(outlet), Ok(mask));
        }
        assert!(ValveType::Prompt3Port.mask_for_outlet(0).is_err());
        assert!(ValveType::Prompt3Port.mask_for_outlet(7).is_err());

        assert_eq!(ValveType::Dtv6Port.first_mask(), 0x01);
        assert_eq!(ValveType::Prompt3Port.first_mask(), 0x04);
        assert_eq!(ValveType::Dtv6Port.first_outlet(), 0);
        assert_eq!(ValveType::Prompt3Port.first_outlet(), 1);
    }

    /// `OUT-02`. The document's own worked example.
    #[test]
    fn prompt3_outlets_1_and_3_or_to_0x14() {
        let t = zone2_table();
        assert_eq!(t.bitmap(set(&[1, 3])).unwrap().bits(), 0x14);
    }

    /// **The regression test the field notes demand.**
    ///
    /// On this installation slot, status index and `wire_outlet + 1` all
    /// coincide, so a test built from the reference configuration walks a path
    /// where every lookup could be replaced by the identity and still pass.
    /// This configuration permutes all three, so a table that quietly assumed
    /// any two were equal produces the wrong mask.
    #[test]
    fn permuted_slots_reach_the_right_outlet() {
        // Slot 1 -> DTV outlet 4 (mask 0x10), reported under status index 6.
        // Slot 2 -> DTV outlet 0 (mask 0x01), reported under status index 3.
        // Slot 5 -> DTV outlet 3 (mask 0x08), reported under status index 1.
        let t = OutletTable::new(
            ValveType::Dtv6Port,
            [
                OutletMapping {
                    slot: slot(1),
                    status_index: 6,
                    wire_outlet: 4,
                },
                OutletMapping {
                    slot: slot(2),
                    status_index: 3,
                    wire_outlet: 0,
                },
                OutletMapping {
                    slot: slot(5),
                    status_index: 1,
                    wire_outlet: 3,
                },
            ],
        )
        .unwrap();

        assert_eq!(t.bitmap(set(&[1])).unwrap().bits(), 0x10);
        assert_eq!(t.bitmap(set(&[2])).unwrap().bits(), 0x01);
        assert_eq!(t.bitmap(set(&[5])).unwrap().bits(), 0x08);
        assert_eq!(t.bitmap(set(&[1, 2, 5])).unwrap().bits(), 0x19);

        // The identity would have produced 0x01 for slot 1 and 0x02 for slot 2.
        assert_ne!(t.bitmap(set(&[1])).unwrap().bits(), 0x01);

        // Status indices bridge the other way, and are also not the identity.
        assert_eq!(t.status_index(slot(1)).unwrap(), 6);
        assert_eq!(t.slot_for_status_index(6), Some(slot(1)));
        assert_eq!(t.slot_for_status_index(2), None);

        // Slots 3, 4 and 6 are not configured. STATE-02: a reported port count
        // is not a configured count, and the absent entry is refused rather
        // than dereferenced.
        assert_eq!(
            t.bitmap(set(&[3])),
            Err(OutletError::UnconfiguredSlot(slot(3)))
        );
        assert_eq!(t.configured_slots(), set(&[1, 2, 5]));

        // Reading back: unknown bits are surfaced, not dropped.
        let (slots, unaccounted) = t.slots_from_bits(0x11 | 0x40);
        assert_eq!(slots, set(&[1, 2]));
        assert_eq!(unaccounted, 0x40);
    }

    /// The same permutation on the other family, so the bug cannot hide in the
    /// half of the table this installation exercises less.
    #[test]
    fn permuted_slots_on_a_prompt_valve_use_the_prompt_bitmap() {
        let t = OutletTable::new(
            ValveType::Prompt3Port,
            [
                OutletMapping {
                    slot: slot(1),
                    status_index: 2,
                    wire_outlet: 3,
                },
                OutletMapping {
                    slot: slot(3),
                    status_index: 1,
                    wire_outlet: 1,
                },
            ],
        )
        .unwrap();
        assert_eq!(t.bitmap(set(&[1])).unwrap().bits(), 0x10);
        assert_eq!(t.bitmap(set(&[3])).unwrap().bits(), 0x04);
        // Under the DTV bitmap the same wire outlets would be 0x08 and 0x02.
        assert_ne!(t.bitmap(set(&[1])).unwrap().bits(), 0x08);
        assert_ne!(t.bitmap(set(&[3])).unwrap().bits(), 0x02);
    }

    #[test]
    fn a_bitmap_carries_the_family_it_was_built_for() {
        assert_eq!(
            zone1_table().bitmap(set(&[1])).unwrap().valve(),
            ValveType::Dtv6Port
        );
        assert_eq!(
            zone2_table().bitmap(set(&[1])).unwrap().valve(),
            ValveType::Prompt3Port
        );
        assert!(OutletBitmap::all_off(ValveType::Dtv6Port).is_empty());
    }

    #[test]
    fn the_two_bitmaps_never_alias_across_the_shared_range() {
        // Every outlet number both families define maps to a different mask.
        for outlet in 1u8..=5 {
            let dtv = ValveType::Dtv6Port.mask_for_outlet(outlet).unwrap();
            let prompt = ValveType::Prompt3Port.mask_for_outlet(outlet).unwrap();
            assert_ne!(dtv, prompt, "outlet {outlet} aliases");
        }
    }

    #[test]
    fn empty_slot_set_is_the_all_off_bitmap() {
        assert_eq!(zone1_table().bitmap(SlotSet::EMPTY).unwrap().bits(), 0x00);
    }

    #[test]
    fn table_rejects_duplicate_slots_and_outlets() {
        let dup_slot = OutletTable::new(
            ValveType::Dtv6Port,
            [
                OutletMapping {
                    slot: slot(1),
                    status_index: 1,
                    wire_outlet: 0,
                },
                OutletMapping {
                    slot: slot(1),
                    status_index: 2,
                    wire_outlet: 1,
                },
            ],
        );
        assert_eq!(dup_slot.unwrap_err(), OutletError::DuplicateSlot(slot(1)));

        let dup_outlet = OutletTable::new(
            ValveType::Dtv6Port,
            [
                OutletMapping {
                    slot: slot(1),
                    status_index: 1,
                    wire_outlet: 0,
                },
                OutletMapping {
                    slot: slot(2),
                    status_index: 2,
                    wire_outlet: 0,
                },
            ],
        );
        assert_eq!(
            dup_outlet.unwrap_err(),
            OutletError::DuplicateWireOutlet {
                valve: ValveType::Dtv6Port,
                outlet: 0
            }
        );

        let dup_status = OutletTable::new(
            ValveType::Dtv6Port,
            [
                OutletMapping {
                    slot: slot(1),
                    status_index: 4,
                    wire_outlet: 0,
                },
                OutletMapping {
                    slot: slot(2),
                    status_index: 4,
                    wire_outlet: 1,
                },
            ],
        );
        assert_eq!(
            dup_status.unwrap_err(),
            OutletError::DuplicateStatusIndex(4)
        );

        // A wire outlet outside the family's bitmap is rejected at build time.
        let bad = OutletTable::new(
            ValveType::Prompt3Port,
            [OutletMapping {
                slot: slot(1),
                status_index: 1,
                wire_outlet: 0,
            }],
        );
        assert!(bad.is_err());
    }

    /// `FLAG-01` / `FLAG-02`. The undefined bits cannot be constructed, and the
    /// two byte-shaped types are not interchangeable even where they collide.
    #[test]
    fn primary_flags_reject_undefined_bits_and_do_not_alias_outlets() {
        assert_eq!(PrimaryFlags::from_bits(0x08), None);
        assert_eq!(PrimaryFlags::from_bits(0x10), None);
        assert_eq!(PrimaryFlags::from_bits(0x80), None);
        assert_eq!(PrimaryFlags::UNDEFINED_BITS, 0x98);
        assert_eq!(PrimaryFlags::CAPTURED.bits(), 0x00);

        // The numeric collisions FLAG-02 names.
        assert_eq!(PrimaryFlags::FULL_COLD.bits(), 0x04);
        assert_eq!(ValveType::Prompt3Port.mask_for_outlet(1), Ok(0x04));
        assert_eq!(PrimaryFlags::DISINFECT.bits(), 0x40);
        assert_eq!(ValveType::Prompt3Port.mask_for_outlet(5), Ok(0x40));

        for b in 0u8..=255 {
            let allowed = b & PrimaryFlags::UNDEFINED_BITS == 0;
            assert_eq!(PrimaryFlags::from_bits(b).is_some(), allowed, "0x{b:02X}");
        }
    }

    #[test]
    fn valve_state_bits_interpret_only_the_two_documented_flags() {
        // 0x81: the ERROR bit plus a reserved bit, which is truncated away.
        let s = ValveStateBits::from_bits_truncate(0x81);
        assert!(s.contains(ValveStateBits::ERROR));
        assert!(!s.contains(ValveStateBits::PAUSE));
        // Both documented bits together.
        let both = ValveStateBits::from_bits_truncate(0x82);
        assert!(both.contains(ValveStateBits::ERROR | ValveStateBits::PAUSE));
        assert!(ValveStateBits::from_bits_truncate(0x02).contains(ValveStateBits::PAUSE));
        // Reserved bits are truncated away rather than guessed at.
        assert_eq!(
            ValveStateBits::from_bits_truncate(0x7D),
            ValveStateBits::empty()
        );
    }

    #[test]
    fn valve_type_comes_only_from_a_reported_firmware_type() {
        assert_eq!(
            ValveType::from_firmware(FirmwareType::Dtv6Port),
            Some(ValveType::Dtv6Port)
        );
        assert_eq!(
            ValveType::from_firmware(FirmwareType::Prompt3FlowControl),
            Some(ValveType::Prompt3FlowControl)
        );
        assert_eq!(ValveType::from_firmware(FirmwareType::Null), None);
        assert_eq!(ValveType::from_firmware(FirmwareType::Unknown(0x42)), None);
    }

    #[test]
    fn mask_and_outlet_round_trip_for_every_family() {
        for v in ValveType::ALL {
            for outlet in 0u8..=8 {
                match v.mask_for_outlet(outlet) {
                    Ok(mask) => assert_eq!(v.outlet_for_mask(mask), Some(outlet)),
                    Err(_) => {
                        assert!(
                            outlet < v.first_outlet()
                                || outlet >= v.first_outlet() + v.mask_positions()
                        );
                    }
                }
            }
            // A multi-bit mask names no single outlet.
            assert_eq!(v.outlet_for_mask(0x0C), None);
            assert_eq!(v.outlet_for_mask(0x00), None);
        }
        // The Prompt 3 defines six mask slots for a three-port valve, and the
        // difference is deliberate.
        assert_eq!(ValveType::Prompt3Port.ports(), 3);
        assert_eq!(ValveType::Prompt3Port.mask_positions(), 6);
    }
}
