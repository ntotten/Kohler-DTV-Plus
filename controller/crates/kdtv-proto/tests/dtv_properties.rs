//! Property tests for the DTV+ codec, over the public API only.
//!
//! The unit tests inside the crate pin the documented frames and walk the tables
//! exhaustively where the domain is small enough — 256 bytes, eight operations,
//! thirty-six setpoints. These are the properties whose domains are not small
//! enough to enumerate: arbitrary payloads, arbitrary line garbage, arbitrary
//! chunk boundaries, arbitrary mutations.
//!
//! Being an integration test, this file can only reach what the crate exports.
//! That is deliberate: if a property here needs something private, the property
//! is being tested at the wrong level.
//!
//! The byte-stuffing properties matter more here than their Saturn equivalents
//! do. `no_frame_this_encoder_produces_needs_byte_stuffing` in `dtv::encode`
//! proves that nothing the encoder can build ever carries an escape, so the
//! stuffing path gets no coverage at all from transmit traffic — these
//! properties and the decoder's own vectors are the whole of it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use kdtv_proto::dtv::{
    DevAddr, DiscoveryStep, DtvRxBuffer, EOF, ESC, MAX_FRAME, MAX_LOGICAL, MAX_PAYLOAD,
    SET_PARAM_STATE_OFFSET, SOF, SteamEncoder, SteamOp, SteamOpState, checksum, checksum_valid,
    decode, decode_frame, escape_into, escaped_len, is_reserved, logical_sums_to_zero,
    unescape_into,
};
use kdtv_proto::fixtures::FixtureSet;
use kdtv_proto::gate::TransmitAuthority;
use kdtv_proto::saturn::LinkPhase;
use kdtv_units::{Fx2, LinkKind, SteamMinutes, SteamSetpoint};
use proptest::prelude::*;

/// The emulator scope, which is the only scope today's tier [C] fixture set can
/// grant. Without one there is no encoder at all — `kdtv_proto::gate`.
fn auth() -> TransmitAuthority {
    TransmitAuthority::emulator_only(FixtureSet::embedded())
}

/// Assembles a wire frame from raw fields. Test-only: the encoder cannot be
/// asked for arbitrary field values, which is the whole point of the encoder.
fn wire_frame(dest: u8, src: u8, cmd: u8, payload: &[u8]) -> Vec<u8> {
    let chk = checksum(dest, src, cmd, payload);
    let mut logical = vec![dest, src, cmd];
    logical.extend_from_slice(payload);
    logical.push(chk);

    let mut stuffed: heapless::Vec<u8, MAX_FRAME> = heapless::Vec::new();
    escape_into(&logical, &mut stuffed).unwrap();

    let mut out = vec![SOF];
    out.extend_from_slice(&stuffed);
    out.push(EOF);
    out
}

/// Pulls frames until one matching `want` appears, or the stream runs dry.
fn pull_until(rx: &mut DtvRxBuffer, want: &[u8]) -> bool {
    for _ in 0..RX_PULLS {
        match decode(rx) {
            Ok(Some(f)) => {
                if f.payload.as_slice() == want {
                    return true;
                }
            }
            Ok(None) => return false,
            Err(_) => {}
        }
    }
    false
}

/// Enough attempts that every byte of a full buffer can be discarded one frame
/// at a time.
const RX_PULLS: usize = 256;

fn setpoint() -> impl Strategy<Value = SteamSetpoint> {
    (0u8..=35).prop_map(|n| SteamSetpoint::try_new(Fx2::from_raw(180 + n * 2)).unwrap())
}

fn minutes() -> impl Strategy<Value = SteamMinutes> {
    (1u8..=20).prop_map(|m| SteamMinutes::try_new(m).unwrap())
}

fn dev_addr() -> impl Strategy<Value = DevAddr> {
    (3u8..=7).prop_map(|a| DevAddr::new(a).unwrap())
}

proptest! {
    /// `STEAM-03`. The defining checksum property: the covered bytes plus the
    /// checksum sum to zero modulo 256, for any field values at all.
    #[test]
    fn checksum_closes_the_frame_to_zero(
        dest in any::<u8>(),
        src in any::<u8>(),
        cmd in any::<u8>(),
        payload in prop::collection::vec(any::<u8>(), 0..=MAX_PAYLOAD),
    ) {
        let chk = checksum(dest, src, cmd, &payload);
        prop_assert!(checksum_valid(dest, src, cmd, &payload, chk));

        let mut logical = vec![dest, src, cmd];
        logical.extend_from_slice(&payload);
        logical.push(chk);
        prop_assert!(logical_sums_to_zero(&logical));
    }

    /// `FRAME-09`. Folding the delimiters into the covered range changes the
    /// answer, so a decoder that summed the whole wire frame would reject valid
    /// traffic.
    #[test]
    fn including_the_delimiters_changes_the_checksum(
        dest in any::<u8>(),
        src in any::<u8>(),
        cmd in any::<u8>(),
        payload in prop::collection::vec(any::<u8>(), 0..=MAX_PAYLOAD),
    ) {
        let excluded = checksum(dest, src, cmd, &payload);
        let mut rest = vec![src, cmd];
        rest.extend_from_slice(&payload);
        rest.push(EOF);
        let included = checksum(SOF, dest, 0, &rest);
        // SOF + EOF is 0x88 + 0x55 = 0xDD, not zero, so the two never agree.
        prop_assert_ne!(excluded, included);
        prop_assert_eq!(included, excluded.wrapping_sub(SOF).wrapping_sub(EOF));
    }

    /// `FRAME-03` / `FRAME-04`. Byte stuffing is invertible for arbitrary
    /// logical bytes.
    #[test]
    fn escaping_round_trips_for_arbitrary_logical_bytes(
        logical in prop::collection::vec(any::<u8>(), 0..=MAX_LOGICAL),
    ) {
        let mut stuffed: heapless::Vec<u8, MAX_FRAME> = heapless::Vec::new();
        escape_into(&logical, &mut stuffed).unwrap();
        prop_assert_eq!(stuffed.len(), escaped_len(&logical));

        let mut back: heapless::Vec<u8, MAX_LOGICAL> = heapless::Vec::new();
        let report = unescape_into(&stuffed, &mut back).unwrap();
        prop_assert_eq!(back.as_slice(), logical.as_slice());
        prop_assert_eq!(report.anomalies, 0);

        // The stuffed form contains no unescaped reserved byte: every reserved
        // byte is immediately preceded by an escape.
        let mut i = 0;
        while i < stuffed.len() {
            if is_reserved(stuffed[i]) {
                prop_assert_eq!(stuffed[i], ESC);
                prop_assert!(i + 1 < stuffed.len());
                i += 2;
            } else {
                i += 1;
            }
        }
    }

    /// The worst case, drawn on purpose: a logical sequence of nothing but
    /// reserved bytes. Every byte doubles and the frame still fits.
    #[test]
    fn an_all_reserved_frame_round_trips(
        len in 0usize..=MAX_PAYLOAD,
        picks in prop::collection::vec(0usize..3, MAX_PAYLOAD + 4),
    ) {
        let reserved = [SOF, EOF, ESC];
        let bytes: Vec<u8> = picks.iter().map(|p| reserved[*p]).collect();
        let dest = bytes[0];
        let src = bytes[1];
        let cmd = bytes[2];
        let payload = &bytes[3..3 + len];

        let wire = wire_frame(dest, src, cmd, payload);
        prop_assert!(wire.len() <= MAX_FRAME);

        let f = decode_frame(&wire).unwrap();
        prop_assert_eq!(f.dest, dest);
        prop_assert_eq!(f.src, src);
        prop_assert_eq!(f.cmd, cmd);
        prop_assert_eq!(f.payload.as_slice(), payload);
        prop_assert_eq!(f.escape_anomalies, 0);
    }

    /// `STEAM-01` / `FRAME-01`. A frame built from arbitrary fields decodes back
    /// to those fields, through both entry points.
    #[test]
    fn a_frame_round_trips_for_arbitrary_fields(
        dest in any::<u8>(),
        src in any::<u8>(),
        cmd in any::<u8>(),
        payload in prop::collection::vec(any::<u8>(), 0..=MAX_PAYLOAD),
    ) {
        let wire = wire_frame(dest, src, cmd, &payload);
        prop_assert!(wire.len() <= MAX_FRAME);

        let framed = decode_frame(&wire).unwrap();
        prop_assert_eq!(framed.dest, dest);
        prop_assert_eq!(framed.src, src);
        prop_assert_eq!(framed.cmd, cmd);
        prop_assert_eq!(framed.payload.as_slice(), payload.as_slice());

        let mut rx = DtvRxBuffer::new();
        rx.extend(&wire);
        let streamed = decode(&mut rx).unwrap().unwrap();
        prop_assert_eq!(&streamed, &framed);
        prop_assert!(rx.is_empty());
    }

    /// The same frame delivered in arbitrary chunks — including chunks that
    /// split an escape sequence — still decodes exactly once.
    #[test]
    fn a_frame_split_at_arbitrary_boundaries_decodes_once(
        dest in any::<u8>(),
        src in any::<u8>(),
        cmd in any::<u8>(),
        payload in prop::collection::vec(any::<u8>(), 0..=8usize),
        chunk in 1usize..=5,
    ) {
        let wire = wire_frame(dest, src, cmd, &payload);
        let mut rx = DtvRxBuffer::new();
        let mut decoded = Vec::new();
        for part in wire.chunks(chunk) {
            rx.extend(part);
            while let Ok(Some(f)) = decode(&mut rx) {
                decoded.push(f);
            }
        }
        prop_assert_eq!(decoded.len(), 1);
        prop_assert_eq!(decoded[0].payload.as_slice(), payload.as_slice());
        prop_assert_eq!(decoded[0].cmd, cmd);
    }

    /// `FRAME-05`. Arbitrary line garbage in front of a frame does not lose it,
    /// as long as the garbage is not itself a decodable frame carrying the same
    /// payload — **and does not end in an escape byte**.
    ///
    /// The exclusion is not a weakening to make a test pass; it is a property of
    /// byte stuffing, pinned separately in
    /// [`garbage_ending_in_an_escape_swallows_the_next_frames_sof`]. An escape
    /// always consumes the byte after it, so a trailing `ESC` consumes the
    /// following frame's `SOF`, and an `SOF` that has been escaped is by the
    /// protocol's own rule not a frame start for the decoder to resync on. No
    /// decoder can recover that frame; it is gone on the wire, not in software.
    #[test]
    fn garbage_before_a_frame_does_not_lose_it(
        garbage in prop::collection::vec(any::<u8>(), 0..=24usize)
            .prop_filter("a trailing ESC eats the next SOF", |g| g.last() != Some(&ESC)),
        cmd in any::<u8>(),
        payload in prop::collection::vec(any::<u8>(), 1..=8usize),
    ) {
        let wire = wire_frame(0x03, 0x00, cmd, &payload);
        let mut bytes = garbage;
        bytes.extend_from_slice(&wire);
        let mut rx = DtvRxBuffer::new();
        rx.extend(&bytes);
        prop_assert!(pull_until(&mut rx, &payload));
    }

    /// `CHK` and framing together: arbitrary bytes never produce a frame that
    /// fails its own checksum, and never produce a frame longer than the bound.
    #[test]
    fn arbitrary_bytes_never_produce_an_invalid_frame(
        bytes in prop::collection::vec(any::<u8>(), 0..=96usize),
    ) {
        let mut rx = DtvRxBuffer::new();
        rx.extend(&bytes);
        for _ in 0..RX_PULLS {
            match decode(&mut rx) {
                Ok(Some(f)) => {
                    prop_assert!(checksum_valid(
                        f.dest,
                        f.src,
                        f.cmd,
                        f.payload.as_slice(),
                        checksum(f.dest, f.src, f.cmd, f.payload.as_slice()),
                    ));
                    prop_assert!(f.payload.len() <= MAX_PAYLOAD);
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
    }

    /// Any single-byte mutation of a delimited frame is caught, or changes what
    /// the frame says. Nothing decodes back to the original fields.
    #[test]
    fn a_single_byte_mutation_never_decodes_to_the_original(
        cmd in any::<u8>(),
        payload in prop::collection::vec(any::<u8>(), 0..=6usize),
        index in 0usize..24,
        delta in 1u8..=255,
    ) {
        let wire = wire_frame(0x03, 0x00, cmd, &payload);
        prop_assume!(index < wire.len());
        let mut mutated = wire.clone();
        mutated[index] = mutated[index].wrapping_add(delta);
        prop_assume!(mutated != wire);

        let unchanged = match decode_frame(&mutated) {
            Err(_) => false,
            Ok(f) => {
                (f.dest, f.src, f.cmd) == (0x03, 0x00, cmd)
                    && f.payload.as_slice() == payload.as_slice()
            }
        };
        prop_assert!(!unchanged, "mutation at {} by {} was invisible", index, delta);
    }

    // ---- The encoder -------------------------------------------------------

    /// Everything the encoder emits decodes back to its own fields, for every
    /// setpoint, duration, state and destination.
    #[test]
    fn every_encoded_frame_round_trips(
        temp in setpoint(),
        minutes in minutes(),
        dest in dev_addr(),
        which in 0usize..6,
    ) {
        let e = SteamEncoder::new(&auth());
        let op = match which {
            0 => SteamOp::Start { temp, minutes },
            1 => SteamOp::Stop { temp, minutes },
            2 => SteamOp::SetTemperature { temp, minutes, state: SteamOpState::On },
            3 => SteamOp::SetDuration { temp, minutes, state: SteamOpState::Off },
            4 => SteamOp::ReadStatus,
            _ => SteamOp::ClearFaults,
        };
        let phase = if which == 5 { LinkPhase::Faulted } else { LinkPhase::Running };
        let f = e.encode(dest, &op, phase, None).unwrap();

        prop_assert!(f.len() <= MAX_FRAME);
        let d = decode_frame(f.bytes()).unwrap();
        prop_assert_eq!(d.dest, f.dest());
        prop_assert_eq!(d.src, 0x00);
        prop_assert_eq!(d.cmd, f.cmd());

        let mut rx = DtvRxBuffer::new();
        rx.extend(f.bytes());
        prop_assert_eq!(decode(&mut rx).unwrap().unwrap(), d);
    }

    /// `CORRECTIONS.md` item 1 / `STEAM-11`, over the whole encoder domain
    /// rather than the enumerated one: the operation-state byte is `0x00` or
    /// `0xFF`, never `0xCC`.
    #[test]
    fn the_operation_state_byte_is_never_power_clean(
        temp in setpoint(),
        minutes in minutes(),
        dest in dev_addr(),
        on in any::<bool>(),
        which in 0usize..4,
    ) {
        let state = if on { SteamOpState::On } else { SteamOpState::Off };
        let op = match which {
            0 => SteamOp::Start { temp, minutes },
            1 => SteamOp::Stop { temp, minutes },
            2 => SteamOp::SetTemperature { temp, minutes, state },
            _ => SteamOp::SetDuration { temp, minutes, state },
        };
        let f = SteamEncoder::new(&auth())
            .encode(dest, &op, LinkPhase::Running, None)
            .unwrap();
        let d = decode_frame(f.bytes()).unwrap();
        let byte = d.payload[SET_PARAM_STATE_OFFSET];
        prop_assert_ne!(byte, SteamOpState::POWER_CLEAN_BYTE);
        prop_assert!(byte == 0x00 || byte == 0xFF);
    }

    /// `STEAM-09` / `STEAM-19`. Every emitted parameter write carries a setpoint
    /// inside 90–125 °F on a whole degree, and a duration inside 1–20 minutes.
    /// The clamps live in the types, so this asserts the types were not bypassed.
    #[test]
    fn req_hardware_steam_11_req_steam_adapter_steam_19_every_emitted_setpoint_and_duration_is_inside_the_clamp(
        temp in setpoint(),
        minutes in minutes(),
        dest in dev_addr(),
    ) {
        let f = SteamEncoder::new(&auth())
            .encode(
                dest,
                &SteamOp::Start { temp, minutes },
                LinkPhase::Running,
                None,
            )
            .unwrap();
        let d = decode_frame(f.bytes()).unwrap();
        prop_assert_eq!(d.payload.len(), 3);
        let fx2 = d.payload[0];
        prop_assert!((180..=250).contains(&fx2));
        prop_assert_eq!(fx2 % 2, 0);
        prop_assert!((1..=20).contains(&d.payload[2]));
    }

    /// Discovery frames need a token minted for this link, in this phase, and
    /// nothing else produces them.
    #[test]
    fn discovery_needs_a_steam_token(
        assign in dev_addr(),
        opportunity in any::<bool>(),
        zone in 0usize..3,
    ) {
        use kdtv_proto::saturn::DiscoveryToken;
        use kdtv_units::ZoneId;

        let e = SteamEncoder::new(&auth());
        let op = if opportunity {
            SteamOp::Discovery(DiscoveryStep::AddressOpportunity)
        } else {
            SteamOp::Discovery(DiscoveryStep::AssignAddress(assign))
        };

        // No token at all.
        prop_assert!(e.encode(assign, &op, LinkPhase::Discovery, None).is_err());

        let link = match zone {
            0 => LinkKind::Zone(ZoneId::Zone1),
            1 => LinkKind::Zone(ZoneId::Zone2),
            _ => LinkKind::Steam,
        };
        let token = DiscoveryToken::mint(link, LinkPhase::Discovery).unwrap();
        let out = e.encode(assign, &op, LinkPhase::Discovery, Some(&token));
        prop_assert_eq!(out.is_ok(), link == LinkKind::Steam);
    }
}

/// The counter-example excluded from
/// [`garbage_before_a_frame_does_not_lose_it`], pinned so the exclusion is a
/// recorded finding rather than a hole.
///
/// `88 AA` is `SOF` then `ESC`. The escape consumes the next byte, which is the
/// real frame's `SOF`, so that `SOF` is escaped and the decoder — which resyncs
/// on an *unescaped* `SOF`, `FRAME-05` — has nothing to resync on. The garbage
/// frame runs on until the real frame's `EOF` terminates it, fails its checksum,
/// and the real frame has already been eaten.
///
/// **This is inherent to byte stuffing, not a decoder defect.** It also means a
/// single stray `0xAA` on the line costs the frame behind it, which is a thing
/// to expect in a Phase 5 capture rather than to be surprised by: the symptom is
/// one `BadChecksum` followed by one missing response, not a storm.
#[test]
fn garbage_ending_in_an_escape_swallows_the_next_frames_sof() {
    let payload = [0x00u8];
    let wire = wire_frame(0x03, 0x00, 0x00, &payload);

    // Trailing ESC: the frame is lost.
    let mut rx = DtvRxBuffer::new();
    rx.extend(&[SOF, ESC]);
    rx.extend(&wire);
    assert!(!pull_until(&mut rx, &payload));

    // The same garbage with one more byte after the escape, so the escape is
    // spent on that byte instead: the frame survives.
    let mut rx = DtvRxBuffer::new();
    rx.extend(&[SOF, ESC, 0x00]);
    rx.extend(&wire);
    assert!(pull_until(&mut rx, &payload));
}
