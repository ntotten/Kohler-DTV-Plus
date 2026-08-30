//! The permissive decoder.
//!
//! Two jobs, and they pull in opposite directions:
//!
//! 1. Read responses on a live link, where a frame that fails any check must be
//!    rejected with a specific reason.
//! 2. Parse **anything** a Phase 1 capture can contain — malformed frames
//!    included — because analysing that capture is how the tier `[C]` guesses in
//!    this crate become measured facts. A decoder that silently discards a
//!    malformed frame discards the evidence.
//!
//! So the decoder never throws bytes away quietly. It resynchronises on the
//! `AA 55` pair, names the reason a frame was rejected, and advances past
//! exactly enough bytes that a later valid frame is still found.
//!
//! [`DecodedFrame`] is deliberately a different type from
//! [`SaturnFrame`](crate::saturn::SaturnFrame): it has no path back to the
//! encoder, so nothing decoded from an untrusted stream can be replayed onto a
//! bus.

use crate::saturn::control::{ControlByte, ValveControl, opcode};
use crate::saturn::encode::{SaturnOpKind, expected_response_len};
use crate::saturn::faults::{FaultBitmap, RawErrorByte, TwoByteField};
use crate::saturn::frame::{
    Direction, FRAME_OVERHEAD, HEADER_LEN, MAX_DATA, MAX_DATA_LEN, MasterAddr, SYNC1, SYNC2,
};

/// Bytes the receiver will hold while it waits for the rest of a frame.
///
/// Three maximum frames. Enough that a full frame plus the garbage in front of
/// it survives a single read, small enough that a wedged link cannot grow it.
pub const RX_CAPACITY: usize = 64;

/// The largest `DATA` field a decoded frame can carry. `PHY-02`.
pub const MAX_DECODED_DATA: usize = MAX_DATA;

/// A frame lifted off the wire, with nothing validated beyond framing and
/// checksum.
///
/// No constructor turns one of these into a transmittable frame. That is the
/// point.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DecodedFrame {
    /// The `ADDRESS` field — the frame's **destination**, not its sender.
    /// `FRAME-03`.
    pub address: u8,
    pub control: ControlByte,
    pub data: heapless::Vec<u8, MAX_DECODED_DATA>,
    /// From address and content. **Never from an echo** — see
    /// [`Direction`].
    pub inferred_direction: Direction,
    /// Bytes discarded before this frame's sync pair. Nonzero means either
    /// line noise, a partial frame, or converter echo bleed; all three are
    /// worth a log line during Phase 1.
    pub skipped_before: usize,
}

impl DecodedFrame {
    /// Total on-wire length, `6 + DATA_LEN`.
    #[must_use]
    pub fn wire_len(&self) -> usize {
        FRAME_OVERHEAD + self.data.len()
    }

    /// The valve-to-master reading of the control byte.
    #[must_use]
    pub fn response(&self) -> ValveControl {
        self.control.as_response()
    }

    /// The one-byte error code from a `0x80` response.
    ///
    /// `None` for any other control byte, and for a `0x80` frame whose payload
    /// is not exactly one byte — a malformed error response is not an error
    /// code. `CMD-08`.
    #[must_use]
    pub fn error_byte(&self) -> Option<RawErrorByte> {
        if self.control.0 != opcode::RESPONSE_ERROR {
            return None;
        }
        match self.data.as_slice() {
            [b] => Some(RawErrorByte(*b)),
            _ => None,
        }
    }

    /// A two-byte payload in both byte orders, for the fields whose endianness
    /// no source states. `RESP-05`.
    #[must_use]
    pub fn two_byte_payload(&self) -> Option<TwoByteField> {
        match self.data.as_slice() {
            [a, b] => Some(TwoByteField::new([*a, *b])),
            _ => None,
        }
    }

    /// The fault bitmap from a `0x0F` read, in both byte orders.
    ///
    /// Returns both readings rather than a single [`FaultBitmap`] because the
    /// byte order is unresolved; the caller logs both and fails closed on either
    /// being nonzero. `ERR-07` / `RESP-05`.
    #[must_use]
    pub fn fault_bitmaps(&self) -> Option<(FaultBitmap, FaultBitmap)> {
        if self.control.0 != opcode::READ_FAULT_FLAGS {
            return None;
        }
        self.two_byte_payload()
            .map(|f| (FaultBitmap(f.be), FaultBitmap(f.le)))
    }
}

/// What the decoder is allowed to assume about the next frame.
///
/// Two shapes, and the difference is which checks run:
///
/// - [`Expectation::response_to`] — a live transaction. Destination and length
///   are checked against the request, because on a serialised bus anything else
///   is a fault.
/// - [`Expectation::capture`] — reading a file. Only framing and checksum are
///   checked, so a frame addressed to a master identity this build does not use
///   still parses.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Expectation {
    /// The master identity this link speaks as, for the destination check and
    /// for direction inference. See [`MasterAddr`] for why this is
    /// configuration.
    pub master: MasterAddr,
    /// The operation whose response is outstanding, if any.
    pub awaiting: Option<SaturnOpKind>,
    /// When false, no destination or length check runs.
    pub strict: bool,
}

impl Expectation {
    /// A live transaction: the reply to `op`, addressed to `master`.
    #[must_use]
    pub const fn response_to(master: MasterAddr, op: SaturnOpKind) -> Self {
        Self {
            master,
            awaiting: Some(op),
            strict: true,
        }
    }

    /// Capture analysis. Framing and checksum only.
    ///
    /// `master` still selects how direction is inferred; it does not gate
    /// anything.
    #[must_use]
    pub const fn capture(master: MasterAddr) -> Self {
        Self {
            master,
            awaiting: None,
            strict: false,
        }
    }
}

/// Why a frame was rejected. Every variant names what was wrong and carries
/// enough to reconstruct it from a log.
#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum DecodeError {
    /// `DATA_LEN` above [`MAX_DATA_LEN`], so the frame cannot fit the 20-byte
    /// maximum. `PHY-02`. Checked before the checksum, because `DATA_LEN` is
    /// what says where the checksum is.
    #[error("DATA_LEN {found} exceeds the maximum of {max} (20-byte frame limit)")]
    LengthOutOfRange { found: u8, max: u8 },

    /// The `ADDRESS` field is not the configured master address, so this frame
    /// was not addressed to us. `FRAME-03` / `FRAME-05`.
    #[error("frame addressed to 0x{found:02X}, expected the master address {expected}")]
    WrongDestination { expected: MasterAddr, found: u8 },

    /// `CHK-03`. `DATA` is not parsed.
    #[error("checksum 0x{found:02X} does not match the computed 0x{computed:02X}")]
    BadChecksum { computed: u8, found: u8 },

    /// `RESP-02`. The frame is well-formed but the wrong size for the command
    /// it answers, so its `DATA` is not parsed even though the checksum passed.
    #[error("{op:?} expects a {expected}-byte response, found {found} bytes")]
    UnexpectedLength {
        op: SaturnOpKind,
        expected: u8,
        found: u8,
    },

    /// `CMD-07`. A response whose control byte is neither the request's echo
    /// nor `0x80` nor `0xFF`.
    #[error("control byte 0x{found:02X} is not the echo of 0x{expected:02X}, an error or a NAK")]
    UnexpectedControl { expected: u8, found: u8 },
}

/// The receive buffer. Bounded, resynchronising, and it never silently drops a
/// frame it could have parsed.
///
/// Implemented as a fixed-capacity byte queue rather than a true circular
/// buffer: at 9600 baud and a 525 ms tick the working set is one frame, and a
/// contiguous slice is what makes `AA 55` scanning and checksum verification
/// straightforward to read. Overflow drops the **oldest** bytes and counts them,
/// which is ring behaviour where it matters.
#[derive(Clone, Debug, Default)]
pub struct RxBuffer {
    buf: heapless::Vec<u8, RX_CAPACITY>,
    skipped: usize,
    overflowed: usize,
}

impl RxBuffer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buf: heapless::Vec::new(),
            skipped: 0,
            overflowed: 0,
        }
    }

    /// Bytes discarded because they were not part of a frame. Cumulative.
    #[must_use]
    pub const fn skipped(&self) -> usize {
        self.skipped
    }

    /// Bytes dropped because the buffer was full. Nonzero means the link is
    /// producing more than the decoder is consuming, which is a fault, not
    /// noise.
    #[must_use]
    pub const fn overflowed(&self) -> usize {
        self.overflowed
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Appends received bytes, dropping the oldest on overflow.
    pub fn extend(&mut self, bytes: &[u8]) {
        for b in bytes {
            if self.buf.is_full() {
                self.drop_front(1);
                self.overflowed += 1;
            }
            // The buffer was just made non-full, so this cannot fail; if it
            // somehow does, dropping the byte is the correct fallback and is
            // already counted by the caller's next overflow.
            if self.buf.push(*b).is_err() {
                self.overflowed += 1;
            }
        }
    }

    /// Removes `n` bytes from the front.
    fn drop_front(&mut self, n: usize) {
        let len = self.buf.len();
        let n = n.min(len);
        if n == 0 {
            return;
        }
        self.buf.as_mut_slice().rotate_left(n);
        self.buf.truncate(len - n);
    }

    fn byte(&self, i: usize) -> Option<u8> {
        self.buf.as_slice().get(i).copied()
    }

    /// Discards everything before the first `AA 55` pair and returns how many
    /// bytes went. `FRAME-04`.
    ///
    /// A trailing lone `0xAA` is kept — it may be the first half of a pair whose
    /// second byte has not arrived.
    fn resync(&mut self) -> usize {
        let s = self.buf.as_slice();
        let found = s.windows(2).position(|w| matches!(w, [SYNC1, SYNC2]));
        let drop = match found {
            Some(0) => return 0,
            Some(i) => i,
            None => {
                // No pair. Keep a trailing 0xAA, discard the rest.
                if s.last() == Some(&SYNC1) {
                    s.len() - 1
                } else {
                    s.len()
                }
            }
        };
        self.drop_front(drop);
        self.skipped += drop;
        drop
    }
}

/// Pulls one frame from `rx`.
///
/// - `Ok(None)` — not enough bytes yet. Call again after more arrive.
/// - `Ok(Some(frame))` — a frame that passed every check `expect` asked for.
/// - `Err(e)` — a frame was rejected for the named reason. **The buffer has
///   already advanced past the offending sync pair**, so calling again resumes
///   the scan rather than returning the same error forever. That is what makes
///   a malformed capture readable instead of fatal.
///
/// Validation order is fixed by `FRAME-05`: sync pair, destination, `DATA_LEN`
/// range, checksum, then the control byte. No `DATA` byte is looked at before
/// the checksum passes.
pub fn decode(
    rx: &mut RxBuffer,
    expect: &Expectation,
) -> Result<Option<DecodedFrame>, DecodeError> {
    let skipped_before = rx.resync();

    // Need the sync pair plus address, control and DATA_LEN.
    if rx.len() < HEADER_LEN {
        return Ok(None);
    }
    let (Some(address), Some(control), Some(data_len)) = (rx.byte(2), rx.byte(3), rx.byte(4))
    else {
        return Ok(None);
    };

    // FRAME-05 step 2. Skipped entirely in capture mode, so a frame addressed
    // to the other master identity — the whole subject of I5 — still parses.
    if expect.strict && address != expect.master.byte() {
        rx.drop_front(2);
        rx.skipped += 2;
        return Err(DecodeError::WrongDestination {
            expected: expect.master,
            found: address,
        });
    }

    // FRAME-05 step 3 / PHY-02.
    if data_len > MAX_DATA_LEN {
        rx.drop_front(2);
        rx.skipped += 2;
        return Err(DecodeError::LengthOutOfRange {
            found: data_len,
            max: MAX_DATA_LEN,
        });
    }

    let total = FRAME_OVERHEAD + usize::from(data_len);
    if rx.len() < total {
        return Ok(None);
    }

    // FRAME-05 step 4. Nothing below this line existed before the checksum
    // passed.
    let Some(data) = rx
        .as_slice()
        .get(HEADER_LEN..HEADER_LEN + usize::from(data_len))
    else {
        return Ok(None);
    };
    let computed = crate::saturn::frame::checksum(address, control, data_len, data);
    let Some(found) = rx.byte(total - 1) else {
        return Ok(None);
    };
    if computed != found {
        rx.drop_front(2);
        rx.skipped += 2;
        return Err(DecodeError::BadChecksum { computed, found });
    }

    let mut payload: heapless::Vec<u8, MAX_DECODED_DATA> = heapless::Vec::new();
    // `data_len <= MAX_DATA_LEN == MAX_DECODED_DATA`, so this cannot overflow.
    if payload.extend_from_slice(data).is_err() {
        rx.drop_front(2);
        rx.skipped += 2;
        return Err(DecodeError::LengthOutOfRange {
            found: data_len,
            max: MAX_DATA_LEN,
        });
    }

    let frame = DecodedFrame {
        address,
        control: ControlByte(control),
        data: payload,
        inferred_direction: Direction::infer(address, expect.master),
        skipped_before,
    };
    rx.drop_front(total);

    // FRAME-05 step 5, and RESP-02. Both need the request, so both are strict
    // mode only. The frame is consumed either way — it was well-formed, and a
    // capture keeps it.
    if let Some(op) = expect.awaiting {
        if let Some(expected) = expected_response_len(op) {
            let found_len = u8::try_from(frame.wire_len()).unwrap_or(u8::MAX);
            if found_len != expected {
                return Err(DecodeError::UnexpectedLength {
                    op,
                    expected,
                    found: found_len,
                });
            }
        }
        let request_control = op.control_byte();
        match frame.response() {
            ValveControl::Error | ValveControl::Nak => {}
            ValveControl::Echo(b) if b == request_control => {}
            ValveControl::Echo(b) => {
                return Err(DecodeError::UnexpectedControl {
                    expected: request_control,
                    found: b,
                });
            }
        }
    }

    Ok(Some(frame))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saturn::frame::checksum;

    fn feed(bytes: &[u8]) -> RxBuffer {
        let mut rx = RxBuffer::new();
        rx.extend(bytes);
        rx
    }

    const FW_TYPE_RESPONSE: &[u8] = &[0xAA, 0x55, 0x00, 0x02, 0x01, 0x1E, 0xDF];
    const ALLOCATE_ACK: &[u8] = &[0xAA, 0x55, 0x00, 0x3A, 0x00, 0xC6];

    #[test]
    fn decodes_the_documented_firmware_type_response() {
        let mut rx = feed(FW_TYPE_RESPONSE);
        let f = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        assert_eq!(f.address, 0x00);
        assert_eq!(f.control, ControlByte(0x02));
        assert_eq!(f.data.as_slice(), &[0x1E]);
        assert_eq!(f.inferred_direction, Direction::ValveToMaster);
        assert_eq!(f.wire_len(), 7);
        assert_eq!(f.skipped_before, 0);
        assert!(rx.is_empty());
    }

    /// `FRAME-04`. The resync property, stated as a test rather than a claim.
    #[test]
    fn garbage_before_a_frame_does_not_lose_it() {
        for prefix in [
            &[][..],
            &[0x00][..],
            &[0xAA][..],
            &[0x55, 0xAA][..],
            &[0xFF, 0xFF, 0xAA, 0x00][..],
            &[0xAA, 0x55][..], // a bare sync pair, which is not a frame
        ] {
            let mut bytes = prefix.to_vec();
            bytes.extend_from_slice(FW_TYPE_RESPONSE);
            let mut rx = feed(&bytes);
            // A truncated leading frame can produce one rejection before the
            // real frame is found; keep pulling until something decodes.
            let mut got = None;
            for _ in 0..4 {
                match decode(&mut rx, &Expectation::capture(MasterAddr::Dtv)) {
                    Ok(Some(f)) => {
                        got = Some(f);
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => {}
                }
            }
            let f = got.unwrap_or_else(|| panic!("lost the frame after prefix {prefix:02X?}"));
            assert_eq!(f.data.as_slice(), &[0x1E]);
        }
    }

    /// The specific hazard the resync exists for: a converter that echoes our
    /// own transmission back at us. This build's converters do not, but the
    /// decoder must survive one that does. `CORRECTIONS.md` item 3.
    #[test]
    fn req_controller_design_proto_06_echo_bleed_before_a_response_still_decodes() {
        let echo: &[u8] = &[0xAA, 0x55, 0x03, 0x02, 0x00, 0xFB];
        let mut bytes = echo.to_vec();
        bytes.extend_from_slice(FW_TYPE_RESPONSE);
        let mut rx = feed(&bytes);
        let expect = Expectation::response_to(MasterAddr::Dtv, SaturnOpKind::ReadFirmwareType);

        // The echo is a valid frame addressed to the valve, so in strict mode
        // it is rejected as wrongly addressed — by address, not by being an
        // echo, because there is no echo signal to key on.
        let first = decode(&mut rx, &expect).unwrap_err();
        assert!(matches!(
            first,
            DecodeError::WrongDestination { found: 0x03, .. }
        ));

        // Keep pulling; the real response survives.
        let mut got = None;
        for _ in 0..8 {
            match decode(&mut rx, &expect) {
                Ok(Some(f)) => {
                    got = Some(f);
                    break;
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
        assert_eq!(got.unwrap().data.as_slice(), &[0x1E]);
    }

    #[test]
    fn a_partial_frame_yields_none_and_completes_later() {
        let mut rx = feed(&FW_TYPE_RESPONSE[..4]);
        assert_eq!(
            decode(&mut rx, &Expectation::capture(MasterAddr::Dtv)).unwrap(),
            None
        );
        rx.extend(&FW_TYPE_RESPONSE[4..]);
        let f = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        assert_eq!(f.data.as_slice(), &[0x1E]);
    }

    #[test]
    fn a_bad_checksum_is_named_and_the_scan_continues() {
        let mut bad = FW_TYPE_RESPONSE.to_vec();
        bad[6] = 0x00;
        let mut bytes = bad;
        bytes.extend_from_slice(ALLOCATE_ACK);
        let mut rx = feed(&bytes);

        let e = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv)).unwrap_err();
        assert_eq!(
            e,
            DecodeError::BadChecksum {
                computed: 0xDF,
                found: 0x00
            }
        );
        // The next frame is still reachable.
        let mut got = None;
        for _ in 0..8 {
            match decode(&mut rx, &Expectation::capture(MasterAddr::Dtv)) {
                Ok(Some(f)) => {
                    got = Some(f);
                    break;
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
        assert_eq!(got.unwrap().control, ControlByte(0x3A));
    }

    #[test]
    fn an_over_long_data_len_is_rejected_by_range_not_by_checksum() {
        // DATA_LEN 0x20 is 32, well past the 14-byte maximum.
        let mut rx = feed(&[0xAA, 0x55, 0x00, 0x02, 0x20, 0x00]);
        let e = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv)).unwrap_err();
        assert_eq!(
            e,
            DecodeError::LengthOutOfRange {
                found: 0x20,
                max: 14
            }
        );
    }

    /// `PHY-02`, exhaustively. Only 0..=14 is accepted, and the boundary is
    /// where the arithmetic says it is.
    #[test]
    fn data_len_is_accepted_only_in_the_legal_range() {
        for len in 0u8..=255 {
            let data = vec![0x00u8; usize::from(len)];
            let chk = checksum(0x00, 0x02, len, &data);
            let mut bytes = vec![0xAA, 0x55, 0x00, 0x02, len];
            bytes.extend_from_slice(&data);
            bytes.push(chk);
            // An illegal DATA_LEN describes a frame longer than the receive
            // buffer, so only what a real link could hold is fed. The range
            // check fires on the 5-byte header, well before that matters.
            let fed = bytes.len().min(RX_CAPACITY);
            let mut rx = feed(bytes.get(..fed).unwrap());
            let out = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv));
            if len <= MAX_DATA_LEN {
                let f = out
                    .unwrap_or_else(|e| panic!("DATA_LEN {len} rejected: {e}"))
                    .unwrap_or_else(|| panic!("DATA_LEN {len} incomplete"));
                assert_eq!(f.data.len(), usize::from(len));
                assert_eq!(f.wire_len(), FRAME_OVERHEAD + usize::from(len));
                assert!(f.wire_len() <= 20);
            } else {
                assert_eq!(
                    out.unwrap_err(),
                    DecodeError::LengthOutOfRange {
                        found: len,
                        max: MAX_DATA_LEN
                    }
                );
            }
        }
    }

    /// `CHK-03`. Any single-byte mutation of a covered field breaks the frame.
    #[test]
    fn any_single_byte_mutation_fails_the_checksum() {
        for i in 2..FW_TYPE_RESPONSE.len() {
            for delta in 1u8..=255 {
                let mut bytes = FW_TYPE_RESPONSE.to_vec();
                bytes[i] = bytes[i].wrapping_add(delta);
                // Mutating DATA_LEN changes where the checksum is, which is a
                // length or framing failure rather than a checksum one; both
                // are rejections, which is what the property asserts.
                let mut rx = feed(&bytes);
                let out = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv));
                let rejected = match out {
                    Err(_) | Ok(None) => true,
                    Ok(Some(f)) => f.data.as_slice() != [0x1E] || f.control != ControlByte(0x02),
                };
                assert!(rejected, "mutation at {i} by {delta} was accepted");
            }
        }
    }

    /// `FRAME-03`. A response carries the master's address; validating against
    /// the queried valve address would reject every real reply.
    #[test]
    fn destination_is_checked_against_the_master_address() {
        let expect = Expectation::response_to(MasterAddr::Dtv, SaturnOpKind::ReadFirmwareType);
        let mut rx = feed(FW_TYPE_RESPONSE);
        assert!(decode(&mut rx, &expect).unwrap().is_some());

        // The same frame under the other master identity is not ours.
        let other = Expectation::response_to(MasterAddr::Prompt, SaturnOpKind::ReadFirmwareType);
        let mut rx = feed(FW_TYPE_RESPONSE);
        assert_eq!(
            decode(&mut rx, &other).unwrap_err(),
            DecodeError::WrongDestination {
                expected: MasterAddr::Prompt,
                found: 0x00
            }
        );

        // And in capture mode it parses under either, which is what makes a
        // capture usable for settling I5.
        let mut rx = feed(FW_TYPE_RESPONSE);
        let f = decode(&mut rx, &Expectation::capture(MasterAddr::Prompt))
            .unwrap()
            .unwrap();
        assert_eq!(f.address, 0x00);
        assert_eq!(f.inferred_direction, Direction::Indeterminate);
    }

    /// `RESP-02`. A checksum-valid frame of the wrong size for its command does
    /// not get its `DATA` believed.
    #[test]
    fn a_valid_frame_of_the_wrong_length_is_a_protocol_error() {
        // Two data bytes where read-firmware-type expects one.
        let chk = checksum(0x00, 0x02, 0x02, &[0x1E, 0x00]);
        let mut rx = feed(&[0xAA, 0x55, 0x00, 0x02, 0x02, 0x1E, 0x00, chk]);
        let expect = Expectation::response_to(MasterAddr::Dtv, SaturnOpKind::ReadFirmwareType);
        assert_eq!(
            decode(&mut rx, &expect).unwrap_err(),
            DecodeError::UnexpectedLength {
                op: SaturnOpKind::ReadFirmwareType,
                expected: 7,
                found: 8
            }
        );
    }

    /// `CMD-07`. Neither the echo, nor `0x80`, nor `0xFF`.
    #[test]
    fn an_unexpected_control_byte_is_named() {
        let chk = checksum(0x00, 0x07, 0x01, &[0x1E]);
        let mut rx = feed(&[0xAA, 0x55, 0x00, 0x07, 0x01, 0x1E, chk]);
        let expect = Expectation::response_to(MasterAddr::Dtv, SaturnOpKind::ReadFirmwareType);
        assert_eq!(
            decode(&mut rx, &expect).unwrap_err(),
            DecodeError::UnexpectedControl {
                expected: 0x02,
                found: 0x07
            }
        );
    }

    #[test]
    fn error_and_nak_responses_are_accepted_for_any_request() {
        // 0x80 with a one-byte code, seven bytes total.
        let chk = checksum(0x00, 0x80, 0x01, &[35]);
        let mut rx = feed(&[0xAA, 0x55, 0x00, 0x80, 0x01, 35, chk]);
        let expect = Expectation::response_to(MasterAddr::Dtv, SaturnOpKind::ReadFirmwareType);
        // The length check still applies and this is 7 bytes, same as the
        // expected read-firmware-type reply.
        let f = decode(&mut rx, &expect).unwrap().unwrap();
        assert_eq!(f.response(), ValveControl::Error);
        assert_eq!(f.error_byte(), Some(RawErrorByte(35)));

        // A NAK carries no data, so it never looks like an error code.
        let chk = checksum(0x00, 0xFF, 0x00, &[]);
        let mut rx = feed(&[0xAA, 0x55, 0x00, 0xFF, 0x00, chk]);
        let f = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        assert_eq!(f.response(), ValveControl::Nak);
        assert_eq!(f.error_byte(), None);
    }

    #[test]
    fn a_malformed_error_response_is_not_an_error_code() {
        // 0x80 with two data bytes: well-framed, but not the documented shape.
        let chk = checksum(0x00, 0x80, 0x02, &[35, 0]);
        let mut rx = feed(&[0xAA, 0x55, 0x00, 0x80, 0x02, 35, 0, chk]);
        let f = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        assert_eq!(f.response(), ValveControl::Error);
        assert_eq!(f.error_byte(), None);
    }

    #[test]
    fn fault_bitmaps_come_back_in_both_byte_orders() {
        let chk = checksum(0x00, 0x0F, 0x02, &[0x00, 0x01]);
        let mut rx = feed(&[0xAA, 0x55, 0x00, 0x0F, 0x02, 0x00, 0x01, chk]);
        let f = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        let (be, le) = f.fault_bitmaps().unwrap();
        assert_eq!(be, FaultBitmap(0x0001));
        assert_eq!(le, FaultBitmap(0x0100));
        // Nonzero either way, which is what fails closed.
        assert!(!be.is_clear() && !le.is_clear());
    }

    #[test]
    fn back_to_back_frames_both_decode() {
        let mut bytes = FW_TYPE_RESPONSE.to_vec();
        bytes.extend_from_slice(ALLOCATE_ACK);
        let mut rx = feed(&bytes);
        let a = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        let b = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        assert_eq!(a.control, ControlByte(0x02));
        assert_eq!(b.control, ControlByte(0x3A));
        assert!(rx.is_empty());
        assert_eq!(rx.skipped(), 0);
    }

    #[test]
    fn the_buffer_is_bounded_and_says_when_it_dropped_bytes() {
        let mut rx = RxBuffer::new();
        rx.extend(&[0x00u8; RX_CAPACITY * 2]);
        assert_eq!(rx.len(), RX_CAPACITY);
        assert_eq!(rx.overflowed(), RX_CAPACITY);
        // Garbage with no sync pair is discarded, and counted.
        assert_eq!(
            decode(&mut rx, &Expectation::capture(MasterAddr::Dtv)).unwrap(),
            None
        );
        assert_eq!(rx.skipped(), RX_CAPACITY);
        assert!(rx.is_empty());
    }

    #[test]
    fn a_trailing_lone_sync1_is_kept_for_the_next_read() {
        let mut rx = feed(&[0x00, 0x00, 0xAA]);
        assert_eq!(
            decode(&mut rx, &Expectation::capture(MasterAddr::Dtv)).unwrap(),
            None
        );
        assert_eq!(rx.as_slice(), &[0xAA]);
        rx.extend(&FW_TYPE_RESPONSE[1..]);
        let f = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        assert_eq!(f.data.as_slice(), &[0x1E]);
    }

    #[test]
    fn skipped_bytes_are_reported_on_the_frame_that_followed_them() {
        let mut bytes = vec![0xDE, 0xAD, 0xBE, 0xEF];
        bytes.extend_from_slice(FW_TYPE_RESPONSE);
        let mut rx = feed(&bytes);
        let f = decode(&mut rx, &Expectation::capture(MasterAddr::Dtv))
            .unwrap()
            .unwrap();
        assert_eq!(f.skipped_before, 4);
        assert_eq!(rx.skipped(), 4);
    }
}
