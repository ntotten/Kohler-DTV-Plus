//! The DTV+ protocol and the steam device profile.
//!
//! DTV+ is the protocol the K-99695 controller speaks to its peripheral
//! devices, on eight independent RS-485 ports at 9600 8N1. This build uses one
//! of them: the link to the K-1737-K1 steam adapter. It shares nothing with
//! Saturn but a physical layer — different delimiters, different framing,
//! different addressing, and a different temperature encoding.
//!
//! # Evidence
//!
//! **Nothing here has been verified against this installation.** No DTV+ bus has
//! ever been captured in this project: the reference system reports
//! `steam_con_string = "not_seen"` and `steam_installed = false`. Every frame,
//! opcode, payload layout, bitmask and timing figure below is tier `[C]` —
//! third-party reverse engineering from
//! `research/xagon0/docs/protocols/dtv-plus-protocol.md` and
//! `research/xagon0/docs/devices/steam-generator.md` — narrowed by
//! `docs/replacement-controller/STEAM-ADAPTER.md` and `HARDWARE-SPEC.md` § 12.
//! Nothing is `[A]`.
//!
//! Where the sources disagree, this module carries both readings and decides
//! nothing:
//!
//! | Contradiction | Handled by |
//! | --- | --- |
//! | Device ID `0x05` used as a bus address | [`DeviceId`] and [`DevAddr`] — two types, no conversion |
//! | Tick 150 ms or 500 ms | [`DtvTimings`] — both candidates plumbed, default 500 ms, floor 150 ms |
//! | Retries 4 or 5 | [`DtvTimings`] — both plumbed, default 4, hard ceiling 5 |
//! | Which opcode carries the status reply — `0x30`, `0x31` or `0x35` | [`StatusCarrier`] — all three accepted, which one arrived is recorded |
//! | `SET_DEV_PARAM` shape: three-field block or `[param_id][value]` | [`ParamCodec`] — a named selection with one implementable variant |
//! | Example 2's checksum `0x92` or `0x73` | `0x73`; `0x92` is a negative fixture the verifier rejects |
//! | Status field widths | `[I]` — one byte each, and a payload of any other length is refused rather than read positionally |
//! | Five device states, three wire encodings | [`SteamStateByte`] decodes three and `Invalid`; the internal machine is not modelled |
//! | Devices per port: 1, 2 or 5 | the allocator covers `0x03..=0x07`; enrolling more than one on this port is the engine's refusal |
//!
//! # Denial
//!
//! Two mechanisms, because there are two hazards.
//!
//! **Opcodes** are denied by the absence of a [`SteamOp`] variant — there is no
//! `Reboot`, no `FirmwareUpdate`, no `ActivateBoot`. [`denied_opcodes`] is the
//! list and the scan test in [`encode`] proves it.
//!
//! **Power clean is not an opcode.** `0xCC` is a value of the operation-state
//! byte inside the payload of `SET_DEV_PARAM` (`0x34`), which is allowlisted, so
//! omitting a command variant does nothing to it. The denial is
//! [`SteamOpState`], which has `Off` and `On` and no third variant, and a
//! byte-level scan over every frame the encoder can produce asserts it.
//! `CORRECTIONS.md` item 1.
//!
//! # Layout
//!
//! | Module | Contents |
//! | --- | --- |
//! | [`frame`] | Delimiters, byte stuffing, the checksum, direction |
//! | [`addr`] | The two byte-wide namespaces that must never be joined |
//! | [`command`] | The opcode table, the allowlist, the denied set |
//! | [`steam`] | Operation state, status payload, error bitmask, UI codes |
//! | [`mod@decode`] | The permissive decoder |
//! | [`encode`] | The allowlist encoder, [`SteamOp`], [`DtvFrame`] |
//! | [`timing`] | Link timing. No echo timeout — see the module docs for why |
//!
//! # The wire format
//!
//! ```text
//! +-------+------+------+------+------------+----------+-------+
//! |  SOF  | DEST | SRC  | CMD  |  PAYLOAD   | CHECKSUM |  EOF  |
//! | 0x88  |  1B  |  1B  |  1B  |   0-N B    |    1B    | 0x55  |
//! +-------+------+------+------+------------+----------+-------+
//! ```
//!
//! `0x88`, `0x55` and `0xAA` are byte-stuffed with a leading `0xAA` everywhere
//! except the two framing bytes. The checksum is the two's complement of
//! `DEST + SRC + CMD + PAYLOAD` over **unescaped** bytes, and is then itself
//! subject to stuffing. There is no length field: frame extent comes from the
//! delimiters, which is why every buffer here is bounded.
//!
//! `DEST` and `SRC` are both `0x00` in both directions of the discovery
//! handshake, because `0x00` means "master" and "unassigned device" at the same
//! time. Discovery therefore routes on opcode, never on address.

pub mod addr;
pub mod command;
pub mod decode;
pub mod encode;
pub mod frame;
pub mod steam;
pub mod timing;

pub use addr::{
    BROADCAST, DEV_ADDR_MAX, DEV_ADDR_MIN, DevAddr, DeviceId, DtvAddrError, MASTER, UNASSIGNED,
};
pub use command::{
    allowlisted_opcodes, denied_opcodes, direction_of, discovery_opcodes, name_of, opcode,
};
pub use decode::{
    DecodedDtv, DtvDecodeError, DtvRxBuffer, RX_CAPACITY, StatusCarrier, StatusDecodeError, decode,
    decode_frame,
};
pub use encode::{
    DiscoveryStep, DtvEncodeDenied, DtvFrame, SteamEncoder, SteamOp, SteamOpKind, state_byte_offset,
};
pub use frame::{
    BAUD, DATA_BITS, DtvDirection, EOF, ESC, FrameTooLong, LOGICAL_OVERHEAD, MAX_FRAME,
    MAX_LOGICAL, MAX_PAYLOAD, RESERVED, SOF, STOP_BITS, UnescapeError, UnescapeReport, checksum,
    checksum_valid, escape_into, escaped_len, is_reserved, logical_sums_to_zero, unescape_into,
};
pub use steam::{
    FIRMWARE_MIN_SETPOINT_FX2, ParamCodec, SET_PARAM_PAYLOAD_LEN, SET_PARAM_STATE_OFFSET,
    STATUS_PAYLOAD_LEN, STATUS_STATE_OFFSET, SteamErrorFlags, SteamOpState, SteamStateByte,
    SteamStatus, SteamStatusError, SteamUiStatus,
};
pub use timing::{DtvTimings, TimingError};

#[cfg(test)]
mod tests {

    /// Stands in for `kdtv-safety`'s grant, which is the only shipping
    /// implementation. `kdtv_units::OpenAuthority` is deliberately unsealed so
    /// that a test can supply one; `cargo xtask audit-graph` asserts no second
    /// implementation reaches the daemon.
    #[derive(Debug)]
    struct TestAuthority(ZoneId);
    impl kdtv_units::OpenAuthority for TestAuthority {
        fn authorised_zone(&self) -> ZoneId {
            self.0
        }
    }
    use super::*;
    use crate::saturn::{
        self, DiscoveryToken, LinkPhase, MasterAddr, OutletMapping, OutletTable, PrimaryFlags,
        SaturnOp, ValveAddr, ValveType,
    };
    use kdtv_units::{
        Cx2, Fx2, LinkKind, Slot, SlotSet, SteamMinutes, SteamSetpoint, ValveSetpoint, ZoneId,
    };

    fn auth() -> crate::gate::TransmitAuthority {
        crate::gate::TransmitAuthority::emulator_only(crate::fixtures::FixtureSet::embedded())
    }

    fn steam_encoder() -> SteamEncoder {
        SteamEncoder::new(&auth())
    }

    fn zone2_encoder() -> saturn::Encoder {
        let table = OutletTable::new(
            ValveType::Prompt3Port,
            (1u8..=3).map(|n| OutletMapping {
                slot: Slot::new(n).unwrap(),
                status_index: n,
                wire_outlet: n,
            }),
        )
        .unwrap();
        saturn::Encoder::new(
            &auth(),
            LinkKind::Zone(ZoneId::Zone2),
            MasterAddr::Dtv,
            table,
        )
    }

    /// The contradictions this module carries are all reachable from the module
    /// root under their documented names, so a reviewer can find them without
    /// reading the module tree.
    #[test]
    fn req_saturn_protocol_err_01_the_public_surface_names_the_contradictions() {
        assert_eq!(SteamOp::ALL.len(), 8);
        assert_eq!(allowlisted_opcodes().len(), 7);
        assert_eq!(discovery_opcodes().len(), 3);
        assert_eq!(denied_opcodes().len(), 19);
        assert_eq!(StatusCarrier::ALL.len(), 3);
        assert_eq!(SteamOpState::ALL.len(), 2);
        assert_eq!(SteamUiStatus::ALL.len(), 9);
        assert_eq!(DevAddr::ALL.len(), 5);
        assert_eq!(DeviceId::DOCUMENTED.len(), 11);
        assert_eq!(ParamCodec::default(), ParamCodec::SteamBlock);
        assert_eq!(DtvTimings::DOCUMENTED.retries, 4);
        assert_eq!(DtvTimings::DOCUMENTED.tick.as_millis(), 500);
        assert_eq!(DtvTimings::TICK_FLOOR.as_millis(), 150);
        assert_eq!((SOF, EOF, ESC), (0x88, 0x55, 0xAA));
        assert_eq!((MASTER, BROADCAST), (0x00, 0xFF));
        assert_eq!((DEV_ADDR_MIN, DEV_ADDR_MAX), (0x03, 0x07));
        assert_eq!(checksum(0x03, 0x00, 0x30, &[]), 0xCD);
        assert!(checksum_valid(0x03, 0x00, 0x34, &[0x01, 0x55], 0x73));
        assert_eq!(MAX_FRAME, 42);
        assert_eq!(BAUD, 9600);
        assert_eq!(SteamErrorFlags::decode(0x24).bits(), 0x24);
    }

    /// A frame the encoder produced and a frame the decoder produced are
    /// different types, and there is no function turning the second into the
    /// first. `RAW-01`. Asserted by construction: the only public route to a
    /// [`DtvFrame`] is [`SteamEncoder::encode`], whose input is [`SteamOp`].
    #[test]
    fn a_decoded_frame_cannot_become_a_transmittable_one() {
        let d = decode_frame(&[
            0x88, 0x00, 0x03, 0x31, 0xDC, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x14, 0x55,
        ])
        .unwrap();
        // The decoded frame exposes its fields, and that is all it exposes.
        assert_eq!(d.cmd, opcode::STATUS_UPDATE);
        assert_eq!(d.direction, DtvDirection::DeviceToMaster);
        let (carrier, status) = d.steam_status(DevAddr::REFERENCE).unwrap();
        assert_eq!(carrier, StatusCarrier::StatusUpdate);
        assert_eq!(status.state, SteamStateByte::Off);
    }

    // ---- The cross-codec test ---------------------------------------------

    /// `TEST-15` / `PH5-02` / `STEAM-08`(encoding). **The units hazard, from
    /// both directions.**
    ///
    /// This test lives here because it is the one place both encoders are
    /// visible, which is the stated reason the two codecs share a crate.
    ///
    /// The **primary** barrier is the type split and it is not testable at
    /// runtime, because the code that would fail does not compile:
    /// [`SaturnOp::SetTemperature`] takes a [`ValveSetpoint`] (backed by `Cx2`)
    /// and [`SteamOp::Start`] takes a [`SteamSetpoint`] (backed by `Fx2`), with
    /// no `From`, no `Deref` and no shared trait between `Cx2` and `Fx2`.
    ///
    /// What *is* testable is the second line of defence: the raw byte from each
    /// encoding, offered to the other side's constructor, is rejected before any
    /// frame exists. That matters because a byte read off a wire or out of a
    /// config file arrives as a number, and a number is what the type split
    /// cannot catch on its own.
    #[test]
    fn req_controller_design_ph5_02_req_hardware_spec_steam_08_a_valve_setpoint_and_a_steam_setpoint_reject_each_other_s_bytes()
     {
        // The concrete hazard from HARDWARE-SPEC.md section 12: 110 F is Fx2
        // 220. Read as Cx2 the same byte asks a valve for 110 C — 2.2x the
        // valve's own hardware ceiling of Cx2 98 (49 C).
        let steam_default = SteamSetpoint::try_new(SteamSetpoint::FACTORY_DEFAULT).unwrap();
        assert_eq!(steam_default.wire().raw(), 220);
        assert!((Cx2::from_raw(220).celsius() - 110.0).abs() < f32::EPSILON);
        assert!(Cx2::from_raw(220) > Cx2::MAX_WATER_TEMP);

        // The Saturn encoder cannot be handed that byte: ValveSetpoint refuses
        // it, so SaturnOp::SetTemperature cannot be built.
        assert!(ValveSetpoint::try_new(Cx2::from_raw(220)).is_err());
        // In fact every legal steam setpoint byte is refused by the valve clamp,
        // because the two ranges do not overlap at all.
        for f in (180u8..=250).step_by(2) {
            let sp = SteamSetpoint::try_new(Fx2::from_raw(f)).unwrap();
            assert!(
                ValveSetpoint::try_new(Cx2::from_raw(sp.wire().raw())).is_err(),
                "Fx2 {f} was accepted as a valve setpoint"
            );
        }

        // And the other direction. Cx2 for 43 C is 86; as Fx2 that is 43 F,
        // harmless but wrong, and the steam clamp refuses it.
        assert!(SteamSetpoint::try_new(Fx2::from_raw(86)).is_err());
        for c in 60u8..=85 {
            let sp = ValveSetpoint::try_new(Cx2::from_raw(c)).unwrap();
            assert!(
                SteamSetpoint::try_new(Fx2::from_raw(sp.wire().raw())).is_err(),
                "Cx2 {c} was accepted as a steam setpoint"
            );
        }
    }

    /// The same property one level up: the frames the two encoders emit carry
    /// bytes from disjoint ranges, so a temperature byte on the wrong bus is
    /// visible in a capture.
    #[test]
    fn the_two_encoders_emit_temperature_bytes_from_disjoint_ranges() {
        let steam = steam_encoder();
        let valve = zone2_encoder();

        let mut steam_bytes = Vec::new();
        for f in (180u8..=250).step_by(2) {
            let frame = steam
                .encode(
                    DevAddr::REFERENCE,
                    &SteamOp::Start {
                        temp: SteamSetpoint::try_new(Fx2::from_raw(f)).unwrap(),
                        minutes: SteamMinutes::default(),
                    },
                    LinkPhase::Running,
                    None,
                )
                .unwrap();
            let d = decode_frame(frame.bytes()).unwrap();
            steam_bytes.push(*d.payload.first().unwrap());
        }

        let mut valve_bytes = Vec::new();
        for c in 60u8..=85 {
            let frame = valve
                .encode(
                    ValveAddr::new(0x03).unwrap(),
                    &SaturnOp::SetTemperature(ValveSetpoint::try_new(Cx2::from_raw(c)).unwrap()),
                    LinkPhase::Running,
                    None,
                    None,
                )
                .unwrap();
            // Saturn: SYNC1 SYNC2 ADDR CTRL LEN DATA... CHK.
            valve_bytes.push(*frame.bytes().get(5).unwrap());
        }

        assert_eq!(steam_bytes.first(), Some(&180));
        assert_eq!(steam_bytes.last(), Some(&250));
        assert_eq!(valve_bytes.first(), Some(&60));
        assert_eq!(valve_bytes.last(), Some(&85));
        for s in &steam_bytes {
            assert!(!valve_bytes.contains(s), "byte {s} is legal on both buses");
        }
    }

    /// `STEAM-15`. No code path leads from the steam operation set to a Saturn
    /// outlet open — deluge is unconstructible, not refused.
    ///
    /// Three independent barriers, each asserted by what it takes to build the
    /// other side's frame: the two operation enums are different types, the two
    /// frame types are different types, and the discovery token that authorises
    /// address management on one link is refused on the other.
    #[test]
    fn req_hardware_spec_steam_15_nothing_on_the_steam_path_can_open_a_valve() {
        // SteamOp has no variant that resolves to a Saturn control byte, and no
        // Saturn opcode appears in the DTV+ reachable set.
        let steam_opcodes: Vec<u8> = SteamOp::ALL.iter().map(|k| k.opcode()).collect();
        assert!(!steam_opcodes.contains(&crate::saturn::opcode::WRITE_OUTLET_STATES));

        // And the token split: a steam discovery token does not authorise a
        // Saturn address frame either.
        let steam_token = DiscoveryToken::mint(LinkKind::Steam, LinkPhase::Discovery).unwrap();
        let valve = zone2_encoder();
        let err = valve
            .encode(
                ValveAddr::new(0x03).unwrap(),
                &SaturnOp::AddressClear,
                LinkPhase::Discovery,
                Some(&steam_token),
                None,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            crate::saturn::EncodeDenied::TokenForWrongLink { .. }
        ));

        // Opening water needs a SaturnOp::SetOutlets, which needs a SlotSet,
        // which nothing on the steam path holds.
        let slots: SlotSet = [1u8].iter().filter_map(|n| Slot::new(*n).ok()).collect();
        let open = valve
            .encode(
                ValveAddr::new(0x03).unwrap(),
                &SaturnOp::SetOutlets {
                    slots,
                    flags: PrimaryFlags::CAPTURED,
                },
                LinkPhase::Running,
                None,
                Some(&TestAuthority(ZoneId::Zone2)),
            )
            .unwrap();
        assert_eq!(open.op(), crate::saturn::SaturnOpKind::SetOutlets);
    }

    /// The two codecs share a physical layer and nothing else: the same byte
    /// sequence does not decode on both.
    #[test]
    fn a_saturn_frame_is_not_a_dtv_frame_and_the_reverse() {
        let steam = steam_encoder()
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::ReadStatus,
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        let valve = zone2_encoder()
            .encode(
                ValveAddr::new(0x03).unwrap(),
                &SaturnOp::ReadFaults,
                LinkPhase::Running,
                None,
                None,
            )
            .unwrap();

        // The Saturn frame starts AA 55, which the DTV+ decoder reads as an
        // escape followed by an EOF — no SOF, so nothing to decode.
        let mut rx = DtvRxBuffer::new();
        rx.extend(valve.bytes());
        assert_eq!(decode(&mut rx).unwrap(), None);
        assert_eq!(rx.skipped(), valve.len());

        // And the DTV+ frame has no AA 55 sync pair for the Saturn decoder.
        let mut srx = crate::saturn::RxBuffer::new();
        srx.extend(steam.bytes());
        assert_eq!(
            crate::saturn::decode(
                &mut srx,
                &crate::saturn::Expectation::capture(MasterAddr::Dtv)
            )
            .unwrap(),
            None
        );
    }

    /// `API-02` / `STEAM-13`. The public steam surface is start, set
    /// temperature, set duration, stop and read status — and this crate's
    /// contribution to it takes Fahrenheit, so no `Cx2 -> Fx2` conversion is
    /// needed anywhere on the steam path.
    ///
    /// `clippy.toml` permits `Cx2::to_fx2` to be called from `kdtv_proto::dtv`.
    /// It is not called: the operator sets steam in Fahrenheit, the value is
    /// clamped once into [`SteamSetpoint`], and it reaches the wire without ever
    /// having been a Celsius number. The conversion stays available for reading
    /// captures, and chaining it back is impossible by return type.
    #[test]
    fn the_steam_path_never_converts_a_celsius_value() {
        let e = steam_encoder();
        let f = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::Start {
                    temp: SteamSetpoint::try_new(Fx2::from_raw(220)).unwrap(),
                    minutes: SteamMinutes::try_new(10).unwrap(),
                },
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        assert_eq!(
            f.bytes(),
            &[0x88, 0x03, 0x00, 0x34, 0xDC, 0xFF, 0x0A, 0xE4, 0x55]
        );

        // The five named operations, all constructible from Fahrenheit alone.
        let temp = SteamSetpoint::try_new(Fx2::from_raw(220)).unwrap();
        let minutes = SteamMinutes::try_new(10).unwrap();
        let surface = [
            SteamOp::Start { temp, minutes },
            SteamOp::SetTemperature {
                temp,
                minutes,
                state: SteamOpState::On,
            },
            SteamOp::SetDuration {
                temp,
                minutes,
                state: SteamOpState::On,
            },
            SteamOp::Stop { temp, minutes },
            SteamOp::ReadStatus,
        ];
        for op in &surface {
            assert!(
                e.encode(DevAddr::REFERENCE, op, LinkPhase::Running, None)
                    .is_ok()
            );
        }
        assert_eq!(surface.len(), 5);
    }
}
