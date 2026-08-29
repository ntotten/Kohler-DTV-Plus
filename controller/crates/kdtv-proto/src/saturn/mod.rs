//! The Saturn valve protocol.
//!
//! Saturn is the protocol the DTV+ controller speaks to its mixing valves, on
//! two dedicated RS-485 ports at 9600 8N1. It predates the DTV+ protocol, and
//! the two share nothing but a physical layer.
//!
//! # Evidence
//!
//! **Nothing here has been verified against this installation.** Every frame,
//! opcode, payload layout, bitmap and timing figure is tier `[C]` — third-party
//! reverse engineering from `research/xagon0/docs/protocols/saturn-protocol.md`
//! plus the local `docs/devices/valve-control.md`. The two disagree on the
//! master address, the error-code table, the calibration opcode, the payload
//! for turning a valve on, and what refreshes the Prompt 3 runtime timer.
//!
//! Where they disagree, this crate carries both readings and decides nothing.
//! The contradictions and where each is handled:
//!
//! | Contradiction | Handled by |
//! | --- | --- |
//! | Master address `0x00` or `0x10` for a Prompt 3 | [`MasterAddr`] — per-link configuration, default `0x00` |
//! | Error-code tables invert 0 and 1 | [`RawErrorByte`] and [`ErrorTable`] — the byte survives, meaning needs a named table |
//! | Response timeout 400 ms or 320 ms | [`Timings`] — both candidates plumbed, neither declared correct |
//! | Retries 3 or 5 | [`Timings`] — both plumbed, default 3 |
//! | Calibration at `0x10`/`0xC0` or `0x15`/`0x95` | [`opcode`] follows the document with byte-level captures; both writes are denied either way |
//! | `ON` flag required in a `0x87` write or not | [`PrimaryFlags::CAPTURED`] — the captured value, `0x00` |
//! | Allocation `DATA_LEN` 1 or 2 | encoded as 2 from the field definitions; the 7-byte alternative is recorded |
//! | Endianness of every 2-byte field | [`TwoByteField`] — both readings carried |
//!
//! # Layout
//!
//! | Module | Contents |
//! | --- | --- |
//! | [`frame`] | Sync, addressing, the checksum, direction inference |
//! | [`control`] | The control-byte table, direction-aware, and firmware type IDs |
//! | [`outlets`] | The three outlet numbering spaces and the one table that bridges them |
//! | [`faults`] | Error bytes, the two incompatible tables, fault bitmaps |
//! | [`timing`] | Link timing. No echo timeout — see the module docs for why |
//! | [`mod@decode`] | The permissive decoder |
//! | [`encode`] | The allowlist encoder, [`SaturnOp`], and [`DiscoveryToken`] |
//!
//! # The wire format
//!
//! ```text
//! +-------+-------+---------+---------+----------+-------------+----------+
//! | SYNC1 | SYNC2 | ADDRESS | CONTROL | DATA_LEN | DATA (0-14) | CHECKSUM |
//! | 0xAA  | 0x55  | 1 byte  | 1 byte  | 1 byte   | DATA_LEN    | 1 byte   |
//! +-------+-------+---------+---------+----------+-------------+----------+
//! ```
//!
//! Total length is `6 + DATA_LEN`, capped at 20 bytes. The checksum is the two's
//! complement of `ADDRESS + CONTROL + DATA_LEN + DATA`, **excluding the sync
//! bytes** — see [`checksum`].
//!
//! `ADDRESS` is the **destination**, not the sender, so a valve's reply carries
//! the master's address. There is no sender field at all, which is why the
//! one-request-in-flight rule is load-bearing rather than a nicety: it is the
//! only thing that correlates a response with its request.

pub mod control;
pub mod decode;
pub mod encode;
pub mod faults;
pub mod frame;
pub mod outlets;
pub mod timing;

pub use control::{
    ControlByte, FirmwareType, FirmwareTypeId, MasterControl, ValveControl, denied_control_bytes,
    opcode,
};
pub use decode::{
    DecodeError, DecodedFrame, Expectation, MAX_DECODED_DATA, RX_CAPACITY, RxBuffer, decode,
};
pub use encode::{
    DiscoveryToken, EncodeDenied, Encoder, LinkPhase, SaturnFrame, SaturnOp, SaturnOpKind,
    expected_response_len,
};
pub use faults::{
    ErrorTable, FaultBitmap, FaultDisposition, LatchReason, RawErrorByte, ReservedBlock,
    SaturnProtocolCode, TwoByteField, ValveControlCode, disposition,
};
pub use frame::{
    AddrError, BROADCAST, Direction, FRAME_OVERHEAD, HEADER_LEN, MAX_DATA_LEN, MAX_FRAME,
    MasterAddr, SYNC, SYNC1, SYNC2, VALVE_ADDR_MAX, VALVE_ADDR_MIN, ValveAddr, checksum,
    checksum_valid,
};
pub use outlets::{
    OutletBitmap, OutletError, OutletMapping, OutletTable, PrimaryFlags, ValveStateBits, ValveType,
};
pub use timing::{BAUD, DATA_BITS, STOP_BITS, Timings};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::FixtureSet;
    use crate::gate::TransmitAuthority;
    use kdtv_units::{Cx2, LinkKind, Slot, SlotSet, ValveSetpoint, ZoneId};

    fn zone2_encoder() -> Encoder {
        let table = OutletTable::new(
            ValveType::Prompt3Port,
            (1u8..=3).map(|n| OutletMapping {
                slot: Slot::new(n).unwrap(),
                status_index: n,
                wire_outlet: n,
            }),
        )
        .unwrap();
        Encoder::new(
            &TransmitAuthority::emulator_only(FixtureSet::embedded()),
            LinkKind::Zone(ZoneId::Zone2),
            MasterAddr::Dtv,
            table,
        )
    }

    /// The round trip that proves the two halves agree: everything the encoder
    /// emits, the decoder parses back to the same fields.
    #[test]
    fn every_encoded_frame_decodes_back_to_its_own_fields() {
        let e = zone2_encoder();
        let t = DiscoveryToken::mint(e.link(), LinkPhase::Discovery).unwrap();
        let slots: SlotSet = [1u8, 3].iter().filter_map(|n| Slot::new(*n).ok()).collect();
        let ops = [
            SaturnOp::AllOff,
            SaturnOp::SetOutlets {
                slots,
                flags: PrimaryFlags::CAPTURED,
            },
            SaturnOp::SetTemperature(ValveSetpoint::try_new(Cx2::from_raw(76)).unwrap()),
            SaturnOp::Pause,
            SaturnOp::Resume,
            SaturnOp::ReadFaults,
            SaturnOp::AddressEnquiry,
            SaturnOp::AddressAllocate(ValveAddr::new(0x05).unwrap()),
            SaturnOp::AddressClear,
        ];
        for op in ops {
            let phase = if op.kind().is_address_management() {
                LinkPhase::Discovery
            } else {
                LinkPhase::Running
            };
            let f = e
                .encode(ValveAddr::new(0x03).unwrap(), &op, phase, Some(&t))
                .unwrap();

            let mut rx = RxBuffer::new();
            rx.extend(f.bytes());
            // Capture mode: these are master-to-valve frames, so they are not
            // addressed to the master and strict mode would reject them. That
            // is the correct behaviour on a live link and the wrong behaviour
            // for reading a capture, which is why there are two modes.
            let d = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
                .unwrap()
                .unwrap();
            assert_eq!(d.address, f.dest());
            assert_eq!(d.control, ControlByte(op.kind().control_byte()));
            assert_eq!(d.wire_len(), f.len());
            assert_eq!(d.inferred_direction, Direction::MasterToValve);
            assert!(rx.is_empty());
        }
    }

    /// A frame the encoder produced and a frame the decoder produced are
    /// different types, and there is no function turning the second into the
    /// first. `RAW-01`. Asserted here by construction: the only public route to
    /// a [`SaturnFrame`] is [`Encoder::encode`], whose input is [`SaturnOp`].
    #[test]
    fn a_decoded_frame_cannot_become_a_transmittable_one() {
        let mut rx = RxBuffer::new();
        rx.extend(&[0xAA, 0x55, 0x00, 0x02, 0x01, 0x1E, 0xDF]);
        let d = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        // The decoded frame exposes its bytes, and that is all it exposes.
        assert_eq!(d.data.as_slice(), &[0x1E]);
        assert_eq!(
            d.control.as_response(),
            ValveControl::Echo(opcode::READ_FIRMWARE_TYPE)
        );
        // FirmwareTypeId, not ControlByte: two fields, two types. CMD-09.
        assert_eq!(FirmwareTypeId(0x1E).classify(), FirmwareType::Prompt3Port);
    }

    /// The three numbering spaces, the two error tables and the two master
    /// identities are all reachable from the crate root under their documented
    /// names, so a reviewer can find them without reading the module tree.
    #[test]
    fn the_public_surface_names_the_contradictions() {
        assert_eq!(MasterAddr::ALL.len(), 2);
        assert_eq!(ValveType::ALL.len(), 4);
        assert_eq!(denied_control_bytes().len(), 10);
        assert_eq!(SaturnOp::ALL.len(), 20);
        assert_eq!(Timings::DOCUMENTED.tick.as_millis(), 525);
        assert_eq!(checksum(0x00, 0x02, 0x01, &[0x1E]), 0xDF);
        assert!(checksum_valid(0x03, 0x87, 0x02, &[0x04, 0x00], 0x70));
        assert_eq!(MAX_FRAME, 20);
        assert_eq!(usize::from(MAX_DATA_LEN), MAX_FRAME - FRAME_OVERHEAD);
        assert_eq!(HEADER_LEN, 5);
        assert_eq!(SYNC, [SYNC1, SYNC2]);
        assert_eq!(BROADCAST, 0x0F);
        assert_eq!((VALVE_ADDR_MIN, VALVE_ADDR_MAX), (0x03, 0x07));
        assert_eq!(
            disposition(ErrorTable::ValveControl, RawErrorByte(0))
                .raw()
                .0,
            0
        );
    }
}
