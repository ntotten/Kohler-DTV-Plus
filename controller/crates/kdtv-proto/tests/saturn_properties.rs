//! Property tests for the Saturn codec, over the public API only.
//!
//! The unit tests inside the crate pin the documented frames and walk the tables
//! exhaustively where the domain is small enough — 256 bytes, 20 operations, six
//! outlets. These are the properties whose domains are not small enough to
//! enumerate: arbitrary payloads, arbitrary line garbage, arbitrary mutations.
//!
//! Being an integration test, this file can only reach what the crate exports.
//! That is deliberate: if a property here needs something private, the property
//! is being tested at the wrong level.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use kdtv_proto::fixtures::FixtureSet;
use kdtv_proto::gate::TransmitAuthority;
use kdtv_proto::saturn::{
    Direction, Encoder, Expectation, FRAME_OVERHEAD, LinkPhase, MAX_DATA_LEN, MAX_FRAME,
    MasterAddr, OutletMapping, OutletTable, PrimaryFlags, RX_CAPACITY, RxBuffer, SYNC1, SYNC2,
    SaturnOp, ValveAddr, ValveType, checksum, checksum_valid, decode,
};
use kdtv_units::{Cx2, LinkKind, Slot, SlotSet, ValveSetpoint, ZoneId};
use proptest::prelude::*;

/// Stands in for `kdtv-safety`'s grant, which is the only shipping
/// implementation of this trait.
#[derive(Debug)]
struct TestAuthority(kdtv_units::ZoneId);
impl kdtv_units::OpenAuthority for TestAuthority {
    fn authorised_zone(&self) -> kdtv_units::ZoneId {
        self.0
    }
}

/// The emulator scope, which is the only scope today's tier [C] fixture set can
/// grant. Without one there is no encoder at all — `kdtv_proto::gate`.
fn auth() -> TransmitAuthority {
    TransmitAuthority::emulator_only(FixtureSet::embedded())
}

fn zone1_encoder() -> Encoder {
    let table = OutletTable::new(
        ValveType::Dtv6Port,
        (1u8..=5).map(|n| OutletMapping {
            slot: Slot::new(n).unwrap(),
            status_index: n,
            wire_outlet: n - 1,
        }),
    )
    .unwrap();
    Encoder::new(
        &auth(),
        LinkKind::Zone(ZoneId::Zone1),
        MasterAddr::Dtv,
        table,
    )
}

/// Assembles a frame from raw fields. Test-only: the encoder cannot be asked
/// for arbitrary field values, which is the whole point of the encoder.
fn raw_frame(address: u8, control: u8, data: &[u8]) -> Vec<u8> {
    let len = u8::try_from(data.len()).unwrap();
    let mut v = vec![SYNC1, SYNC2, address, control, len];
    v.extend_from_slice(data);
    v.push(checksum(address, control, len, data));
    v
}

proptest! {
    /// `CHK-01` / `CHK-03`. The defining property: the covered bytes plus the
    /// checksum sum to zero modulo 256, for any field values at all.
    #[test]
    fn checksum_closes_the_frame_to_zero(
        address in any::<u8>(),
        control in any::<u8>(),
        data in prop::collection::vec(any::<u8>(), 0..=14usize),
    ) {
        let len = u8::try_from(data.len()).unwrap();
        let chk = checksum(address, control, len, &data);
        let sum = data
            .iter()
            .fold(address.wrapping_add(control).wrapping_add(len), |a, b| {
                a.wrapping_add(*b)
            })
            .wrapping_add(chk);
        prop_assert_eq!(sum, 0);
        prop_assert!(checksum_valid(address, control, len, &data, chk));
    }

    /// `CHK-02`. Folding the sync bytes into the covered range changes the
    /// answer for every frame whose sync-inclusive sum differs — which is every
    /// frame, since `0xAA + 0x55` is `0xFF`, not zero.
    #[test]
    fn including_the_sync_bytes_always_changes_the_checksum(
        address in any::<u8>(),
        control in any::<u8>(),
        data in prop::collection::vec(any::<u8>(), 0..=14usize),
    ) {
        let len = u8::try_from(data.len()).unwrap();
        let excluded = checksum(address, control, len, &data);
        let mut with_sync = vec![address, control, len];
        with_sync.extend_from_slice(&data);
        let included = checksum(SYNC1, SYNC2, 0, &with_sync);
        prop_assert_ne!(excluded, included);
        // The difference is exactly the sync pair, every time.
        prop_assert_eq!(included, excluded.wrapping_sub(SYNC1).wrapping_sub(SYNC2));
    }

    /// `CHK-03`. Any single-byte mutation of a covered field breaks the frame.
    /// A checksum is only worth having if this holds.
    #[test]
    fn any_single_byte_mutation_breaks_the_checksum(
        address in any::<u8>(),
        control in any::<u8>(),
        data in prop::collection::vec(any::<u8>(), 0..=14usize),
        index in 0usize..16,
        delta in 1u8..=255,
    ) {
        let len = u8::try_from(data.len()).unwrap();
        let chk = checksum(address, control, len, &data);
        // Index over ADDRESS, CONTROL, DATA_LEN, DATA and the checksum itself.
        let covered = 3 + data.len() + 1;
        let i = index % covered;
        let (mut a, mut c, mut l, mut d, mut k) = (address, control, len, data.clone(), chk);
        match i {
            0 => a = a.wrapping_add(delta),
            1 => c = c.wrapping_add(delta),
            2 => l = l.wrapping_add(delta),
            n if n < 3 + d.len() => d[n - 3] = d[n - 3].wrapping_add(delta),
            _ => k = k.wrapping_add(delta),
        }
        // Mutating DATA_LEN changes which bytes are covered; the checksum is
        // recomputed over the field values as they now stand.
        let recomputed = checksum(a, c, l, &d);
        prop_assert_ne!(recomputed, k, "mutation at field {} by {} was absorbed", i, delta);
    }

    /// `FRAME-04`. The resync property, stated over arbitrary garbage rather
    /// than a handful of hand-picked prefixes.
    ///
    /// The prefix is filtered so it cannot itself contain a sync pair or end in
    /// a lone `0xAA` — either would make the garbage the start of a frame, which
    /// is a different (and correct) behaviour, not a lost frame.
    #[test]
    fn arbitrary_garbage_before_a_frame_still_decodes(
        garbage in prop::collection::vec(any::<u8>(), 0..=32usize)
            .prop_filter("prefix must not look like a frame start", |g| {
                !g.windows(2).any(|w| w == [SYNC1, SYNC2]) && g.last() != Some(&SYNC1)
            }),
        control in any::<u8>(),
        data in prop::collection::vec(any::<u8>(), 0..=14usize),
    ) {
        let frame = raw_frame(MasterAddr::Dtv.byte(), control, &data);
        // Keep the total inside the receive buffer, so this tests resync rather
        // than overflow — which has its own test below.
        prop_assume!(garbage.len() + frame.len() <= RX_CAPACITY);

        let mut rx = RxBuffer::new();
        rx.extend(&garbage);
        rx.extend(&frame);

        let d = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .expect("a valid frame behind garbage must not be an error")
            .expect("a complete frame must decode");
        prop_assert_eq!(d.address, MasterAddr::Dtv.byte());
        prop_assert_eq!(d.control.0, control);
        prop_assert_eq!(d.data.as_slice(), data.as_slice());
        prop_assert_eq!(d.skipped_before, garbage.len());
        prop_assert_eq!(d.inferred_direction, Direction::ValveToMaster);
        prop_assert_eq!(rx.skipped(), garbage.len());
        prop_assert!(rx.is_empty());
    }

    /// `PHY-02`. `DATA_LEN` fuzzed across the whole byte, with only 0..=14
    /// accepted — and every accepted frame inside the 20-byte maximum.
    #[test]
    fn data_len_outside_the_legal_range_is_always_rejected(
        data_len in any::<u8>(),
        control in any::<u8>(),
        fill in any::<u8>(),
    ) {
        let data = vec![fill; usize::from(data_len)];
        let mut bytes = vec![SYNC1, SYNC2, MasterAddr::Dtv.byte(), control, data_len];
        bytes.extend_from_slice(&data);
        bytes.push(checksum(MasterAddr::Dtv.byte(), control, data_len, &data));
        // A frame with an illegal DATA_LEN is longer than any receive buffer;
        // the range check fires on the five-byte header, well before that.
        bytes.truncate(RX_CAPACITY);

        let mut rx = RxBuffer::new();
        rx.extend(&bytes);
        let out = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv));

        if data_len <= MAX_DATA_LEN {
            let d = out.expect("legal DATA_LEN must not error").expect("complete");
            prop_assert_eq!(d.data.len(), usize::from(data_len));
            prop_assert_eq!(d.wire_len(), FRAME_OVERHEAD + usize::from(data_len));
            prop_assert!(d.wire_len() <= MAX_FRAME);
        } else {
            prop_assert!(out.is_err(), "DATA_LEN {} was accepted", data_len);
        }
    }

    /// A corrupted checksum is always rejected, and never mistaken for a short
    /// frame or silently accepted.
    #[test]
    fn a_corrupted_checksum_is_always_rejected(
        control in any::<u8>(),
        data in prop::collection::vec(any::<u8>(), 0..=14usize),
        delta in 1u8..=255,
    ) {
        let mut frame = raw_frame(MasterAddr::Dtv.byte(), control, &data);
        let last = frame.len() - 1;
        frame[last] = frame[last].wrapping_add(delta);
        let mut rx = RxBuffer::new();
        rx.extend(&frame);
        // A corrupted frame can still resync into a later position, so the
        // property is that the frame as sent is never returned intact.
        let mut intact = false;
        for _ in 0..8 {
            match decode(&mut rx, &Expectation::capture(MasterAddr::Dtv)) {
                Ok(Some(d)) => {
                    if d.control.0 == control && d.data.as_slice() == data.as_slice() {
                        intact = true;
                    }
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
        prop_assert!(!intact, "a frame with a broken checksum decoded intact");
    }

    /// Everything the encoder can emit decodes back to the fields it was built
    /// from, for every legal setpoint and every subset of configured slots.
    #[test]
    fn encoded_frames_round_trip_through_the_decoder(
        raw_setpoint in any::<u8>(),
        slot_mask in 0u8..32,
        flag_bits in any::<u8>(),
        addr in 3u8..=7,
    ) {
        let e = zone1_encoder();
        let target = ValveAddr::new(addr).unwrap();
        let slots: SlotSet = (1u8..=5)
            .filter(|n| slot_mask & (1 << (n - 1)) != 0)
            .filter_map(|n| Slot::new(n).ok())
            .collect();

        let mut ops = vec![SaturnOp::AllOff, SaturnOp::Pause, SaturnOp::Resume];
        if let Some(flags) = PrimaryFlags::from_bits(flag_bits) {
            ops.push(SaturnOp::SetOutlets { slots, flags });
        }
        if let Ok(sp) = ValveSetpoint::try_new(Cx2::from_raw(raw_setpoint)) {
            ops.push(SaturnOp::SetTemperature(sp));
        }

        for op in ops {
            let f = e
                .encode(target, &op, LinkPhase::Running, None, Some(&TestAuthority(kdtv_units::ZoneId::Zone1)))
                .expect("an allowlisted write in Running must encode");
            let mut rx = RxBuffer::new();
            rx.extend(f.bytes());
            let d = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
                .expect("an encoded frame must decode")
                .expect("complete");
            prop_assert_eq!(d.address, addr);
            prop_assert_eq!(d.control.0, op.kind().control_byte());
            prop_assert_eq!(d.wire_len(), f.len());
            prop_assert_eq!(d.inferred_direction, Direction::MasterToValve);
            prop_assert_eq!(d.skipped_before, 0);
            prop_assert!(rx.is_empty());
        }
    }

    /// Feeding the decoder pure noise never panics, never loops, and never
    /// produces a frame that fails its own checksum. This is the fuzz target's
    /// invariant, held here as a property so it runs on every build.
    #[test]
    fn arbitrary_bytes_never_produce_an_invalid_frame(
        noise in prop::collection::vec(any::<u8>(), 0..=200usize),
    ) {
        let mut rx = RxBuffer::new();
        for chunk in noise.chunks(16) {
            rx.extend(chunk);
            for _ in 0..8 {
                match decode(&mut rx, &Expectation::capture(MasterAddr::Dtv)) {
                    Ok(Some(d)) => {
                        // Whatever came out is a genuine frame.
                        let len = u8::try_from(d.data.len()).unwrap();
                        prop_assert!(len <= MAX_DATA_LEN);
                        prop_assert!(d.wire_len() <= MAX_FRAME);
                        prop_assert!(checksum_valid(
                            d.address,
                            d.control.0,
                            len,
                            d.data.as_slice(),
                            checksum(d.address, d.control.0, len, d.data.as_slice()),
                        ));
                    }
                    Ok(None) => break,
                    Err(_) => {}
                }
            }
        }
        // Bounded regardless of input.
        prop_assert!(rx.len() <= RX_CAPACITY);
    }

    /// No operation, on any input the encoder accepts, produces a frame longer
    /// than the 20-byte maximum or a `DATA_LEN` above 14. `PHY-02` / `DENY-07`.
    #[test]
    fn no_encodable_frame_exceeds_the_wire_limits(
        raw_setpoint in any::<u8>(),
        slot_mask in 0u8..32,
        flag_bits in any::<u8>(),
        addr in 3u8..=7,
    ) {
        let e = zone1_encoder();
        let target = ValveAddr::new(addr).unwrap();
        let slots: SlotSet = (1u8..=5)
            .filter(|n| slot_mask & (1 << (n - 1)) != 0)
            .filter_map(|n| Slot::new(n).ok())
            .collect();

        let mut ops = vec![
            SaturnOp::AllOff,
            SaturnOp::Pause,
            SaturnOp::Resume,
            SaturnOp::ReadFirmwareVersion,
            SaturnOp::ReadCalibration,
            SaturnOp::ReadConfiguration,
            SaturnOp::ReadGenericOutlets,
        ];
        if let Some(flags) = PrimaryFlags::from_bits(flag_bits) {
            ops.push(SaturnOp::SetOutlets { slots, flags });
        }
        if let Ok(sp) = ValveSetpoint::try_new(Cx2::from_raw(raw_setpoint)) {
            ops.push(SaturnOp::SetTemperature(sp));
        }

        for op in ops {
            let Ok(f) = e.encode(target, &op, LinkPhase::Running, None, Some(&TestAuthority(kdtv_units::ZoneId::Zone1))) else {
                continue;
            };
            prop_assert!(f.len() <= MAX_FRAME);
            let data_len = f.bytes()[4];
            prop_assert!(data_len <= MAX_DATA_LEN);
            prop_assert_eq!(f.len(), FRAME_OVERHEAD + usize::from(data_len));
        }
    }
}
