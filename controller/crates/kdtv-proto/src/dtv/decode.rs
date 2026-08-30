//! The permissive DTV+ decoder.
//!
//! Same two jobs as the Saturn decoder, pulling the same two ways: reject a bad
//! frame with a named reason on a live link, and parse **anything** a Phase 5
//! capture can contain, because that capture is how the tier `[C]` guesses in
//! this crate become measured facts.
//!
//! [`DecodedDtv`] is deliberately a different type from
//! [`DtvFrame`](crate::dtv::DtvFrame): it has no path back to the encoder, so
//! nothing decoded from an untrusted stream can be replayed onto a bus.
//!
//! # Two entry points, and why they disagree about one thing
//!
//! [`decode`] is the streaming form. It finds the terminating `EOF` by an
//! escape-aware scan, so `… AA 55` is an escaped `0x55` and not a frame
//! terminator; the frame simply continues.
//!
//! [`decode_frame`] is the single-frame form used for fixtures and for a
//! capture file that already carries frame boundaries. There the caller asserts
//! that the last byte *is* the terminating `EOF`, so a body ending in a lone
//! `0xAA` has nothing to escape and is rejected as
//! [`DtvDecodeError::TruncatedEscape`]. That is `FRAME-08` — "a trailing `0xAA`
//! immediately before the unescaped `0x55`" — and it is only expressible where
//! something other than the escape rule decides where the frame ends.
//!
//! The streaming decoder reaches the same truncated frame by a different route:
//! it keeps scanning, and either the next frame's unescaped `SOF` discards the
//! partial one ([`DtvDecodeError::IncompleteFrame`], `FRAME-05`) or the frame
//! grows past [`MAX_FRAME`] and is refused ([`DtvDecodeError::FrameOverrun`],
//! `FRAME-06`). Both are tested against the same bytes.
//!
//! # Escape state and frame boundaries
//!
//! Escape state lives inside one scan of one frame. Nothing carries an escape
//! across a frame boundary, and that is the point: a `0xAA` at the end of a
//! frame cannot swallow the next frame's `SOF`, because the frame's own `EOF`
//! ended the scan first. Where a frame is *incomplete*, the raw bytes stay in
//! the buffer and the scan restarts from the beginning on the next call, so
//! bytes arriving mid-escape are handled by re-scanning rather than by a
//! resumable state machine. A resumable one would have to be reset on every
//! resync, and forgetting that reset is how a decoder starts eating framing
//! bytes.

use crate::dtv::addr::{DevAddr, DeviceId, MASTER};
use crate::dtv::command::{direction_of, opcode};
use crate::dtv::frame::{
    DtvDirection, EOF, ESC, LOGICAL_OVERHEAD, MAX_FRAME, MAX_LOGICAL, MAX_PAYLOAD, SOF,
    UnescapeError, checksum, unescape_into,
};
use crate::dtv::steam::{SteamStatus, SteamStatusError};

/// Bytes the receiver will hold while it waits for the rest of a frame.
///
/// Three maximum frames. Enough that a full frame plus the garbage in front of
/// it survives a single read, small enough that a wedged link cannot grow it.
pub const RX_CAPACITY: usize = 128;

/// A frame lifted off the wire, with nothing validated beyond framing and
/// checksum.
///
/// No constructor turns one of these into a transmittable frame. That is the
/// point.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DecodedDtv {
    /// The `DEST` field. `0x00` means master **or** unassigned device.
    pub dest: u8,
    /// The `SRC` field. `0x00` means master **or** unassigned device.
    pub src: u8,
    /// The `CMD` field.
    pub cmd: u8,
    /// The unescaped payload.
    pub payload: heapless::Vec<u8, MAX_PAYLOAD>,
    /// From the opcode. **Never from the address** — both discovery frames
    /// carry `DEST` `0x00` and `SRC` `0x00`. `ADDR-06`.
    pub direction: DtvDirection,
    /// Bytes discarded before this frame's `SOF`. Nonzero means line noise, a
    /// partial frame, or converter echo bleed; all three are worth a log line
    /// during Phase 5.
    pub skipped_before: usize,
    /// Escape sequences whose escaped byte was not one of the three reserved
    /// bytes. `FRAME-07`: undocumented, taken literally, counted.
    pub escape_anomalies: usize,
}

impl DecodedDtv {
    /// The documented opcode name, or `None`.
    #[must_use]
    pub const fn command_name(&self) -> Option<&'static str> {
        crate::dtv::command::name_of(self.cmd)
    }

    /// The device ID from a `DEV_REQUEST_ADDR` (`0x06`).
    ///
    /// `None` for any other opcode and for a `0x06` frame whose payload is not
    /// exactly one byte — a malformed request is not a device ID. This is the
    /// **only** place a [`DeviceId`] enters the system, and it goes nowhere near
    /// a `DEST` field. `CORRECTIONS.md` item 2.
    #[must_use]
    pub fn requested_device_id(&self) -> Option<DeviceId> {
        if self.cmd != opcode::DEV_REQUEST_ADDR {
            return None;
        }
        match self.payload.as_slice() {
            [b] => Some(DeviceId::new(*b)),
            _ => None,
        }
    }

    /// The error byte from a `DEV_NAK` (`0x36`).
    ///
    /// `STEAM-09`: a NAK is a rejected command, not a transient failure. Do not
    /// retry with the same values; surface this byte to the operator.
    #[must_use]
    pub fn nak_error_byte(&self) -> Option<u8> {
        if self.cmd != opcode::DEV_NAK {
            return None;
        }
        match self.payload.as_slice() {
            [b] => Some(*b),
            _ => None,
        }
    }

    /// The six-field steam status, if this frame carries one from `expected`.
    ///
    /// Three checks, all from `STEAM-04` / `STEAM-05`:
    ///
    /// 1. `SRC` equals the device's assigned address, and `DEST` is the master.
    ///    A frame failing either is dropped rather than decoded.
    /// 2. The opcode is one of `0x30`, `0x31` or `0x35`. **The sources name
    ///    three different opcodes for one reply** — `steam-generator.md` shows
    ///    the response echoing `0x30`, `dtv-plus-protocol.md` § Get Device
    ///    Status shows `DEV_ACK` `0x35`, and the same page defines
    ///    `STATUS_UPDATE` `0x31` as "device reports its current status". All
    ///    three are accepted and [`StatusCarrier`] records which one arrived, so
    ///    the first real capture narrows this rather than a guess doing it now.
    /// 3. The payload is exactly six bytes. `STEAM-03`.
    pub fn steam_status(
        &self,
        expected: DevAddr,
    ) -> Result<(StatusCarrier, SteamStatus), StatusDecodeError> {
        if self.src != expected.get() {
            return Err(StatusDecodeError::WrongSource {
                expected: expected.get(),
                found: self.src,
            });
        }
        if self.dest != MASTER {
            return Err(StatusDecodeError::WrongDestination { found: self.dest });
        }
        let carrier = StatusCarrier::of(self.cmd)
            .ok_or(StatusDecodeError::NotAStatusCarrier { cmd: self.cmd })?;
        let status = SteamStatus::decode(self.payload.as_slice())?;
        Ok((carrier, status))
    }
}

/// Which opcode carried a steam status payload. `STEAM-04`.
///
/// Recorded rather than normalised away, because "which opcode is actually
/// observed" is the open question this type exists to answer.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum StatusCarrier {
    /// `0x30`, the request opcode echoed back. `steam-generator.md`.
    EchoedRequest,
    /// `0x31` `STATUS_UPDATE`. `dtv-plus-protocol.md` § Command Set.
    StatusUpdate,
    /// `0x35` `DEV_ACK` with a status payload. `dtv-plus-protocol.md` § Get
    /// Device Status.
    DevAck,
}

impl StatusCarrier {
    /// All three candidates.
    pub const ALL: [Self; 3] = [Self::EchoedRequest, Self::StatusUpdate, Self::DevAck];

    #[must_use]
    pub const fn of(cmd: u8) -> Option<Self> {
        Some(match cmd {
            opcode::GET_DEV_STATUS => Self::EchoedRequest,
            opcode::STATUS_UPDATE => Self::StatusUpdate,
            opcode::DEV_ACK => Self::DevAck,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn opcode(self) -> u8 {
        match self {
            Self::EchoedRequest => opcode::GET_DEV_STATUS,
            Self::StatusUpdate => opcode::STATUS_UPDATE,
            Self::DevAck => opcode::DEV_ACK,
        }
    }
}

/// Why a well-framed frame was not a steam status.
#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum StatusDecodeError {
    /// `STEAM-05`. Not from the enrolled device.
    #[error("status came from SRC 0x{found:02X}, expected the assigned address 0x{expected:02X}")]
    WrongSource { expected: u8, found: u8 },
    /// `STEAM-05`. Not addressed to the master.
    #[error("status addressed to 0x{found:02X}, expected the master 0x00")]
    WrongDestination { found: u8 },
    /// `STEAM-04`. Not one of the three candidate opcodes.
    #[error("opcode 0x{cmd:02X} does not carry a steam status payload")]
    NotAStatusCarrier { cmd: u8 },
    /// `STEAM-03`.
    #[error(transparent)]
    Payload(#[from] SteamStatusError),
}

/// Why a frame was rejected. Every variant names what was wrong and carries
/// enough to reconstruct it from a log.
#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum DtvDecodeError {
    /// `FRAME-05`. An unescaped `SOF` arrived while a frame was in progress, so
    /// the partial frame is discarded and the scan restarts at the new `SOF`.
    #[error("discarded {discarded} bytes of a partial frame at a resynchronisation point")]
    IncompleteFrame { discarded: usize },

    /// `FRAME-06`. No terminating `EOF` inside the frame bound. There is no
    /// length field on the wire, so this is the only thing that stops an
    /// unterminated frame from consuming the buffer.
    #[error("no EOF within the {max}-byte frame bound")]
    FrameOverrun { max: usize },

    /// `FRAME-08`. The frame ends with an escape that has nothing to escape.
    /// Reachable from [`decode_frame`], where the caller supplies the frame
    /// boundary.
    #[error("the frame ends with a truncated escape sequence")]
    TruncatedEscape,

    /// Fewer than four logical bytes, so there is no room for `DEST`, `SRC`,
    /// `CMD` and `CHECKSUM`.
    #[error("frame carries {found} logical bytes, fewer than the {min} a header needs")]
    TooShort { found: usize, min: usize },

    /// More payload than [`MAX_PAYLOAD`].
    #[error("frame payload exceeds the {max}-byte maximum")]
    PayloadTooLong { max: usize },

    /// `STEAM-03` § Verification.
    #[error("checksum 0x{found:02X} does not match the computed 0x{computed:02X}")]
    BadChecksum { computed: u8, found: u8 },

    /// [`decode_frame`] only: the slice does not begin with `SOF` and end with
    /// `EOF`.
    #[error("the slice is not a delimited frame")]
    NotDelimited,
}

/// The receive buffer. Bounded, resynchronising, and it never silently drops a
/// frame it could have parsed.
///
/// A fixed-capacity byte queue rather than a true circular buffer, for the same
/// reason as the Saturn one: the working set is a single frame and a contiguous
/// slice is what makes the escape-aware scan readable. Overflow drops the
/// **oldest** bytes and counts them.
#[derive(Clone, Debug, Default)]
pub struct DtvRxBuffer {
    buf: heapless::Vec<u8, RX_CAPACITY>,
    skipped: usize,
    overflowed: usize,
    anomalies: usize,
}

impl DtvRxBuffer {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buf: heapless::Vec::new(),
            skipped: 0,
            overflowed: 0,
            anomalies: 0,
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

    /// Cumulative `FRAME-07` anomalies across every frame decoded from this
    /// buffer.
    #[must_use]
    pub const fn escape_anomalies(&self) -> usize {
        self.anomalies
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
            // counted here.
            if self.buf.push(*b).is_err() {
                self.overflowed += 1;
            }
        }
    }

    fn drop_front(&mut self, n: usize) {
        let len = self.buf.len();
        let n = n.min(len);
        if n == 0 {
            return;
        }
        self.buf.as_mut_slice().rotate_left(n);
        self.buf.truncate(len - n);
    }

    /// Discards everything before the first `SOF` and returns how many bytes
    /// went.
    ///
    /// An unescaped `SOF` is the only resynchronisation point the protocol has —
    /// there is no sync pair here, and no length field. `FRAME-05`.
    fn resync(&mut self) -> usize {
        let drop = match self.buf.as_slice().iter().position(|b| *b == SOF) {
            Some(0) => return 0,
            Some(i) => i,
            None => self.buf.len(),
        };
        self.drop_front(drop);
        self.skipped += drop;
        drop
    }
}

/// Pulls one frame from `rx`.
///
/// - `Ok(None)` — not enough bytes yet. Call again after more arrive.
/// - `Ok(Some(frame))` — a frame whose checksum and framing are sound.
/// - `Err(e)` — a frame was rejected for the named reason. **The buffer has
///   already advanced**, so calling again resumes the scan rather than returning
///   the same error forever. That is what makes a malformed capture readable
///   instead of fatal.
///
/// Validation order: resynchronise to `SOF`, find the terminating unescaped
/// `EOF` within the frame bound, unescape, check the header length, check the
/// checksum. No payload byte is interpreted before the checksum passes.
pub fn decode(rx: &mut DtvRxBuffer) -> Result<Option<DecodedDtv>, DtvDecodeError> {
    let skipped_before = rx.resync();
    if rx.is_empty() {
        return Ok(None);
    }

    let bytes = rx.as_slice();
    let bound = bytes.len().min(MAX_FRAME);
    let mut end = None;
    let mut escaped = false;
    let mut i = 1;
    while i < bound {
        let Some(b) = bytes.get(i).copied() else {
            break;
        };
        if escaped {
            escaped = false;
        } else if b == ESC {
            escaped = true;
        } else if b == EOF {
            end = Some(i);
            break;
        } else if b == SOF {
            // FRAME-05: an unescaped SOF discards the partial frame in
            // progress. Everything before it goes; the scan restarts there.
            rx.drop_front(i);
            rx.skipped += i;
            return Err(DtvDecodeError::IncompleteFrame { discarded: i });
        }
        i += 1;
    }

    let Some(end) = end else {
        if bytes.len() >= MAX_FRAME {
            // FRAME-06. Drop this SOF so the scan can find the next one.
            rx.drop_front(1);
            rx.skipped += 1;
            return Err(DtvDecodeError::FrameOverrun { max: MAX_FRAME });
        }
        return Ok(None);
    };

    let Some(body) = bytes.get(1..end) else {
        return Ok(None);
    };
    let parsed = parse_body(body, skipped_before);
    rx.drop_front(end + 1);
    if let Ok(f) = &parsed {
        rx.anomalies += f.escape_anomalies;
    }
    parsed.map(Some)
}

/// Decodes one already-delimited frame.
///
/// `wire` must start with `SOF` and end with `EOF`; the caller is asserting
/// where the frame ends, which is what makes [`DtvDecodeError::TruncatedEscape`]
/// expressible. For fixtures, for a capture file that carries frame boundaries,
/// and for round-trip tests against the encoder.
pub fn decode_frame(wire: &[u8]) -> Result<DecodedDtv, DtvDecodeError> {
    let (Some(&SOF), Some(&EOF)) = (wire.first(), wire.last()) else {
        return Err(DtvDecodeError::NotDelimited);
    };
    if wire.len() < 2 {
        return Err(DtvDecodeError::NotDelimited);
    }
    let Some(body) = wire.get(1..wire.len() - 1) else {
        return Err(DtvDecodeError::NotDelimited);
    };
    parse_body(body, 0)
}

/// Unescapes and validates the bytes between the two delimiters.
fn parse_body(body: &[u8], skipped_before: usize) -> Result<DecodedDtv, DtvDecodeError> {
    let mut logical: heapless::Vec<u8, MAX_LOGICAL> = heapless::Vec::new();
    let report = unescape_into(body, &mut logical).map_err(|e| match e {
        UnescapeError::TruncatedEscape => DtvDecodeError::TruncatedEscape,
        UnescapeError::TooLong { .. } => DtvDecodeError::PayloadTooLong { max: MAX_PAYLOAD },
    })?;

    let too_short = DtvDecodeError::TooShort {
        found: logical.len(),
        min: LOGICAL_OVERHEAD,
    };
    let Some(([dest, src, cmd], rest)) = logical.as_slice().split_first_chunk::<3>() else {
        return Err(too_short);
    };
    let (dest, src, cmd) = (*dest, *src, *cmd);
    let Some((found_checksum, payload)) = rest.split_last() else {
        return Err(too_short);
    };

    let computed = checksum(dest, src, cmd, payload);
    if computed != *found_checksum {
        return Err(DtvDecodeError::BadChecksum {
            computed,
            found: *found_checksum,
        });
    }

    let mut out: heapless::Vec<u8, MAX_PAYLOAD> = heapless::Vec::new();
    if out.extend_from_slice(payload).is_err() {
        return Err(DtvDecodeError::PayloadTooLong { max: MAX_PAYLOAD });
    }

    Ok(DecodedDtv {
        dest,
        src,
        cmd,
        payload: out,
        direction: direction_of(cmd),
        skipped_before,
        escape_anomalies: report.anomalies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dtv::steam::{SteamStateByte, SteamUiStatus};

    fn feed(bytes: &[u8]) -> DtvRxBuffer {
        let mut rx = DtvRxBuffer::new();
        rx.extend(bytes);
        rx
    }

    /// `88 03 00 30 CD 55` — the steam status request at the address the
    /// reference topology assigns.
    const STATUS_REQUEST: &[u8] = &[0x88, 0x03, 0x00, 0x30, 0xCD, 0x55];
    /// The derived status response fixture from `ARITHMETIC-NOTES.md`.
    const STATUS_RESPONSE: &[u8] = &[
        0x88, 0x00, 0x03, 0x31, 0xDC, 0xDC, 0x00, 0x00, 0x00, 0x00, 0x14, 0x55,
    ];

    #[test]
    fn decodes_the_documented_status_request() {
        let mut rx = feed(STATUS_REQUEST);
        let f = decode(&mut rx).unwrap().unwrap();
        assert_eq!((f.dest, f.src, f.cmd), (0x03, 0x00, 0x30));
        assert!(f.payload.is_empty());
        assert_eq!(f.direction, DtvDirection::MasterToDevice);
        assert_eq!(f.command_name(), Some("GET_DEV_STATUS"));
        assert_eq!(f.skipped_before, 0);
        assert!(rx.is_empty());
    }

    #[test]
    fn decodes_the_derived_status_response_into_six_fields() {
        let mut rx = feed(STATUS_RESPONSE);
        let f = decode(&mut rx).unwrap().unwrap();
        assert_eq!((f.dest, f.src, f.cmd), (0x00, 0x03, 0x31));
        assert_eq!(f.direction, DtvDirection::DeviceToMaster);
        let (carrier, s) = f.steam_status(DevAddr::REFERENCE).unwrap();
        assert_eq!(carrier, StatusCarrier::StatusUpdate);
        assert_eq!(s.state, SteamStateByte::Off);
        assert_eq!(s.desired.raw(), 220);
        assert_eq!(s.ui_status(), SteamUiStatus::Off);
    }

    /// `STEAM-04`. Three opcodes, one payload, and the decoder says which one
    /// arrived instead of normalising the question away.
    #[test]
    fn all_three_candidate_status_opcodes_are_accepted_and_recorded() {
        for carrier in StatusCarrier::ALL {
            let cmd = carrier.opcode();
            let payload = [0xDC, 0xDC, 0x00, 0x00, 0x00, 0x00];
            let chk = checksum(0x00, 0x03, cmd, &payload);
            let mut wire = vec![0x88, 0x00, 0x03, cmd];
            wire.extend_from_slice(&payload);
            wire.extend_from_slice(&[chk, 0x55]);
            let f = decode_frame(&wire).unwrap();
            let (got, _) = f.steam_status(DevAddr::REFERENCE).unwrap();
            assert_eq!(got, carrier);
        }
        // ARITHMETIC-NOTES.md records the three checksums for the same payload
        // under the three opcodes.
        assert_eq!(checksum(0x00, 0x03, 0x31, &[0xDC, 0xDC, 0, 0, 0, 0]), 0x14);
        assert_eq!(checksum(0x00, 0x03, 0x30, &[0xDC, 0xDC, 0, 0, 0, 0]), 0x15);
        assert_eq!(checksum(0x00, 0x03, 0x35, &[0xDC, 0xDC, 0, 0, 0, 0]), 0x10);
    }

    /// `STEAM-05`. A status frame from the wrong source or to the wrong
    /// destination is dropped, not decoded.
    #[test]
    fn a_status_frame_must_come_from_the_assigned_address_to_the_master() {
        let f = decode_frame(STATUS_RESPONSE).unwrap();
        assert!(f.steam_status(DevAddr::REFERENCE).is_ok());
        assert_eq!(
            f.steam_status(DevAddr::new(0x04).unwrap()),
            Err(StatusDecodeError::WrongSource {
                expected: 0x04,
                found: 0x03
            })
        );

        // Addressed to a device rather than to the master.
        let payload = [0xDC, 0xDC, 0x00, 0x00, 0x00, 0x00];
        let chk = checksum(0x04, 0x03, 0x31, &payload);
        let mut wire = vec![0x88, 0x04, 0x03, 0x31];
        wire.extend_from_slice(&payload);
        wire.extend_from_slice(&[chk, 0x55]);
        let f = decode_frame(&wire).unwrap();
        assert_eq!(
            f.steam_status(DevAddr::REFERENCE),
            Err(StatusDecodeError::WrongDestination { found: 0x04 })
        );
    }

    /// The critical escape case: the checksum is `0x55` and is stuffed, and the
    /// byte after it is the real `EOF`. `FRAME-04`.
    #[test]
    fn an_escaped_checksum_does_not_terminate_the_frame() {
        // 03 + 00 + 34 + 74 = 0xAB; two's complement 0x55.
        assert_eq!(checksum(0x03, 0x00, 0x34, &[0x74]), 0x55);
        let wire = [0x88, 0x03, 0x00, 0x34, 0x74, 0xAA, 0x55, 0x55];
        let mut rx = feed(&wire);
        let f = decode(&mut rx).unwrap().unwrap();
        assert_eq!((f.dest, f.src, f.cmd), (0x03, 0x00, 0x34));
        assert_eq!(f.payload.as_slice(), &[0x74]);
        assert_eq!(f.escape_anomalies, 0);
        assert!(rx.is_empty());
        assert_eq!(decode_frame(&wire).unwrap(), f);
    }

    /// The document's own Example 2, with the corrected checksum. `FRAME-09`.
    #[test]
    fn the_escaped_payload_example_decodes_and_the_stale_checksum_does_not() {
        let good = [0x88, 0x03, 0x00, 0x34, 0x01, 0xAA, 0x55, 0x73, 0x55];
        let f = decode_frame(&good).unwrap();
        assert_eq!(f.payload.as_slice(), &[0x01, 0x55]);

        // The value the document prints before self-correcting.
        let stale = [0x88, 0x03, 0x00, 0x34, 0x01, 0xAA, 0x55, 0x92, 0x55];
        assert_eq!(
            decode_frame(&stale),
            Err(DtvDecodeError::BadChecksum {
                computed: 0x73,
                found: 0x92
            })
        );
    }

    /// The other two derived escaped-checksum vectors: `0xAA` and `0x88`.
    #[test]
    fn a_checksum_of_esc_or_sof_is_stuffed_too() {
        assert_eq!(checksum(0x03, 0x00, 0x34, &[0x1F]), 0xAA);
        let esc_chk = [0x88, 0x03, 0x00, 0x34, 0x1F, 0xAA, 0xAA, 0x55];
        assert_eq!(decode_frame(&esc_chk).unwrap().payload.as_slice(), &[0x1F]);

        assert_eq!(checksum(0x03, 0x00, 0x34, &[0x41]), 0x88);
        let sof_chk = [0x88, 0x03, 0x00, 0x34, 0x41, 0xAA, 0x88, 0x55];
        let f = decode_frame(&sof_chk).unwrap();
        assert_eq!(f.payload.as_slice(), &[0x41]);
        // And streaming: the escaped 0x88 must not resynchronise the decoder.
        let mut rx = feed(&sof_chk);
        assert_eq!(
            decode(&mut rx).unwrap().unwrap().payload.as_slice(),
            &[0x41]
        );
    }

    /// `FRAME-08`, and the one place the two entry points disagree.
    #[test]
    fn a_truncated_escape_is_rejected_by_the_frame_form() {
        // The escaped-checksum frame with its final EOF cut off. The caller has
        // said the last byte is the terminating EOF, so the 0xAA in front of it
        // has nothing to escape — FRAME-08 verbatim.
        let truncated = [0x88, 0x03, 0x00, 0x34, 0x74, 0xAA, 0x55];
        assert_eq!(
            decode_frame(&truncated),
            Err(DtvDecodeError::TruncatedEscape)
        );
        // A longer body ending the same way.
        let body_truncated = [0x88, 0x03, 0x00, 0x34, 0x74, 0xAA, 0x55, 0xAA, 0x55];
        assert_eq!(
            decode_frame(&body_truncated),
            Err(DtvDecodeError::TruncatedEscape)
        );
        // The well-formed frame the two are truncations of.
        assert!(decode_frame(&[0x88, 0x03, 0x00, 0x34, 0x74, 0xAA, 0x55, 0x55]).is_ok());

        // Streaming sees the same bytes as an unterminated frame: the escape
        // swallowed the EOF, so the scan keeps going and waits for more.
        let mut rx = feed(&truncated);
        assert_eq!(decode(&mut rx).unwrap(), None);
        // A following frame's SOF discards the partial one, FRAME-05, and the
        // good frame survives.
        rx.extend(STATUS_REQUEST);
        assert_eq!(
            decode(&mut rx).unwrap_err(),
            DtvDecodeError::IncompleteFrame { discarded: 7 }
        );
        let f = decode(&mut rx).unwrap().unwrap();
        assert_eq!(f.cmd, 0x30);
    }

    /// `FRAME-05`. A truncated frame followed by a valid one: the valid one is
    /// recovered.
    #[test]
    fn a_partial_frame_does_not_lose_the_frame_after_it() {
        for prefix in [
            &[][..],
            &[0x00][..],
            &[0xFF, 0xFF][..],
            &[0x88, 0x03, 0x00][..],
            &[0x88, 0x03, 0x00, 0x34, 0xDC][..],
            &[0x55, 0x55, 0x55][..],
        ] {
            let mut bytes = prefix.to_vec();
            bytes.extend_from_slice(STATUS_REQUEST);
            let mut rx = feed(&bytes);
            let mut got = None;
            for _ in 0..6 {
                match decode(&mut rx) {
                    Ok(Some(f)) => {
                        got = Some(f);
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => {}
                }
            }
            let f = got.unwrap_or_else(|| panic!("lost the frame after {prefix:02X?}"));
            assert_eq!(f.cmd, 0x30);
        }
    }

    /// The echo hazard. This build's converters present no local echo, but the
    /// decoder must survive one that does. `CORRECTIONS.md` item 3.
    #[test]
    fn req_design_proto_06_echo_bleed_before_a_response_still_decodes() {
        let mut bytes = STATUS_REQUEST.to_vec(); // our own transmission, echoed
        bytes.extend_from_slice(STATUS_RESPONSE);
        let mut rx = feed(&bytes);
        let echo = decode(&mut rx).unwrap().unwrap();
        assert_eq!(echo.cmd, 0x30);
        // The echo is a well-formed frame; it is told apart by direction and
        // address at the transaction layer, not by an echo signal here.
        assert_eq!(echo.direction, DtvDirection::MasterToDevice);
        let reply = decode(&mut rx).unwrap().unwrap();
        assert_eq!(reply.cmd, 0x31);
        assert_eq!(reply.direction, DtvDirection::DeviceToMaster);
        assert!(rx.is_empty());
    }

    #[test]
    fn a_partial_frame_yields_none_and_completes_later() {
        let mut rx = feed(&STATUS_REQUEST[..3]);
        assert_eq!(decode(&mut rx).unwrap(), None);
        rx.extend(&STATUS_REQUEST[3..]);
        assert_eq!(decode(&mut rx).unwrap().unwrap().cmd, 0x30);
    }

    /// Bytes arriving mid-escape: the buffer holds them and the scan restarts.
    #[test]
    fn a_frame_split_inside_an_escape_sequence_completes() {
        let wire = [0x88, 0x03, 0x00, 0x34, 0x74, 0xAA, 0x55, 0x55];
        for split in 1..wire.len() {
            let mut rx = feed(&wire[..split]);
            assert_eq!(decode(&mut rx).unwrap(), None, "split at {split}");
            rx.extend(&wire[split..]);
            let f = decode(&mut rx).unwrap().unwrap();
            assert_eq!(f.payload.as_slice(), &[0x74], "split at {split}");
        }
    }

    #[test]
    fn a_bad_checksum_is_named_and_the_scan_continues() {
        let mut bad = STATUS_REQUEST.to_vec();
        bad[4] = 0x00;
        let mut bytes = bad;
        bytes.extend_from_slice(STATUS_RESPONSE);
        let mut rx = feed(&bytes);
        assert_eq!(
            decode(&mut rx).unwrap_err(),
            DtvDecodeError::BadChecksum {
                computed: 0xCD,
                found: 0x00
            }
        );
        assert_eq!(decode(&mut rx).unwrap().unwrap().cmd, 0x31);
    }

    /// Every single-byte mutation of a delimited frame is rejected, or decodes
    /// to something that is not the original.
    #[test]
    fn any_single_byte_mutation_is_caught() {
        for i in 1..STATUS_RESPONSE.len() {
            for delta in 1u8..=255 {
                let mut bytes = STATUS_RESPONSE.to_vec();
                bytes[i] = bytes[i].wrapping_add(delta);
                let mut rx = feed(&bytes);
                let rejected = match decode(&mut rx) {
                    Err(_) | Ok(None) => true,
                    Ok(Some(f)) => {
                        (f.dest, f.src, f.cmd) != (0x00, 0x03, 0x31)
                            || f.payload.as_slice() != [0xDC, 0xDC, 0x00, 0x00, 0x00, 0x00]
                    }
                };
                assert!(rejected, "mutation at {i} by {delta} was accepted");
            }
        }
    }

    /// `FRAME-06`. There is no length field, so the bound is the only thing
    /// stopping an unterminated frame.
    #[test]
    fn an_unterminated_frame_overruns_and_the_scan_recovers() {
        let mut bytes = vec![0x88];
        bytes.extend(core::iter::repeat_n(0x00u8, MAX_FRAME));
        let mut rx = feed(&bytes);
        assert_eq!(
            decode(&mut rx).unwrap_err(),
            DtvDecodeError::FrameOverrun { max: MAX_FRAME }
        );
        // Exactly one byte went, so the next SOF — if there is one — is found.
        rx.clear();
        let mut bytes = vec![0x88];
        bytes.extend(core::iter::repeat_n(0x00u8, MAX_FRAME));
        bytes.extend_from_slice(STATUS_REQUEST);
        let mut rx = feed(&bytes);
        let mut got = None;
        for _ in 0..RX_CAPACITY {
            match decode(&mut rx) {
                Ok(Some(f)) => {
                    got = Some(f);
                    break;
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
        assert_eq!(got.map(|f| f.cmd), Some(0x30));
    }

    #[test]
    fn a_frame_with_no_room_for_a_header_is_too_short() {
        // SOF, three logical bytes, EOF: no checksum.
        let wire = [0x88, 0x03, 0x00, 0x30, 0x55];
        assert_eq!(
            decode_frame(&wire),
            Err(DtvDecodeError::TooShort { found: 3, min: 4 })
        );
        // SOF EOF: nothing at all.
        assert_eq!(
            decode_frame(&[0x88, 0x55]),
            Err(DtvDecodeError::TooShort { found: 0, min: 4 })
        );
    }

    #[test]
    fn decode_frame_requires_both_delimiters() {
        assert_eq!(decode_frame(&[]), Err(DtvDecodeError::NotDelimited));
        assert_eq!(decode_frame(&[0x88]), Err(DtvDecodeError::NotDelimited));
        assert_eq!(
            decode_frame(&[0x03, 0x00, 0x30, 0xCD, 0x55]),
            Err(DtvDecodeError::NotDelimited)
        );
        assert_eq!(
            decode_frame(&[0x88, 0x03, 0x00, 0x30, 0xCD]),
            Err(DtvDecodeError::NotDelimited)
        );
    }

    /// `FRAME-07`. An escape of a byte no source covers is taken literally and
    /// counted, on the frame and cumulatively on the buffer.
    #[test]
    fn an_escape_anomaly_is_counted_not_guessed_at() {
        // Payload [0x42] with a gratuitous escape in front of it.
        let chk = checksum(0x03, 0x00, 0x34, &[0x42]);
        let wire = [0x88, 0x03, 0x00, 0x34, 0xAA, 0x42, chk, 0x55];
        let mut rx = feed(&wire);
        let f = decode(&mut rx).unwrap().unwrap();
        assert_eq!(f.payload.as_slice(), &[0x42]);
        assert_eq!(f.escape_anomalies, 1);
        assert_eq!(rx.escape_anomalies(), 1);
    }

    /// `ADDR-06`. The discovery handshake, routed by opcode because both frames
    /// carry `DEST` `0x00` `SRC` `0x00`.
    #[test]
    fn the_discovery_handshake_decodes_and_routes_by_opcode() {
        let opp = decode_frame(&[0x88, 0xFF, 0x00, 0x05, 0xFC, 0x55]).unwrap();
        assert_eq!(opp.dest, 0xFF);
        assert_eq!(opp.direction, DtvDirection::MasterToDevice);

        let request = decode_frame(&[0x88, 0x00, 0x00, 0x06, 0x05, 0xF5, 0x55]).unwrap();
        assert_eq!((request.dest, request.src), (0x00, 0x00));
        assert_eq!(request.direction, DtvDirection::DeviceToMaster);
        assert_eq!(
            request.requested_device_id(),
            Some(DeviceId::STEAM_GENERATOR)
        );

        let assign = decode_frame(&[0x88, 0x00, 0x00, 0x07, 0x03, 0xF6, 0x55]).unwrap();
        assert_eq!((assign.dest, assign.src), (0x00, 0x00));
        assert_eq!(assign.direction, DtvDirection::MasterToDevice);
        // Same addresses, opposite directions — the whole point of ADDR-06.
        assert_eq!((request.dest, request.src), (assign.dest, assign.src));
        assert_ne!(request.direction, assign.direction);
        // And the device ID never becomes an address.
        assert_eq!(assign.requested_device_id(), None);
        assert_eq!(assign.payload.as_slice(), &[DevAddr::REFERENCE.get()]);
    }

    #[test]
    fn a_nak_surfaces_its_error_byte() {
        let chk = checksum(0x00, 0x03, 0x36, &[0x07]);
        let f = decode_frame(&[0x88, 0x00, 0x03, 0x36, 0x07, chk, 0x55]).unwrap();
        assert_eq!(f.nak_error_byte(), Some(0x07));
        assert_eq!(f.requested_device_id(), None);
        // A malformed NAK is not an error code.
        let chk = checksum(0x00, 0x03, 0x36, &[0x07, 0x08]);
        let f = decode_frame(&[0x88, 0x00, 0x03, 0x36, 0x07, 0x08, chk, 0x55]).unwrap();
        assert_eq!(f.nak_error_byte(), None);
    }

    #[test]
    fn back_to_back_frames_both_decode() {
        let mut bytes = STATUS_REQUEST.to_vec();
        bytes.extend_from_slice(STATUS_RESPONSE);
        let mut rx = feed(&bytes);
        assert_eq!(decode(&mut rx).unwrap().unwrap().cmd, 0x30);
        assert_eq!(decode(&mut rx).unwrap().unwrap().cmd, 0x31);
        assert!(rx.is_empty());
        assert_eq!(rx.skipped(), 0);
    }

    #[test]
    fn the_buffer_is_bounded_and_says_when_it_dropped_bytes() {
        let mut rx = DtvRxBuffer::new();
        rx.extend(&[0x00u8; RX_CAPACITY * 2]);
        assert_eq!(rx.len(), RX_CAPACITY);
        assert_eq!(rx.overflowed(), RX_CAPACITY);
        assert_eq!(decode(&mut rx).unwrap(), None);
        assert_eq!(rx.skipped(), RX_CAPACITY);
        assert!(rx.is_empty());
    }

    #[test]
    fn skipped_bytes_are_reported_on_the_frame_that_followed_them() {
        let mut bytes = vec![0xDE, 0xAD, 0xBE, 0xEF];
        bytes.extend_from_slice(STATUS_REQUEST);
        let mut rx = feed(&bytes);
        let f = decode(&mut rx).unwrap().unwrap();
        assert_eq!(f.skipped_before, 4);
        assert_eq!(rx.skipped(), 4);
    }

    /// A payload longer than the codec will hold is a named refusal, not an
    /// allocation. `FRAME-06`.
    #[test]
    fn an_over_long_payload_is_refused() {
        let payload = vec![0x00u8; MAX_PAYLOAD + 1];
        let chk = checksum(0x03, 0x00, 0x34, &payload);
        let mut wire = vec![0x88, 0x03, 0x00, 0x34];
        wire.extend_from_slice(&payload);
        wire.extend_from_slice(&[chk, 0x55]);
        assert_eq!(
            decode_frame(&wire),
            Err(DtvDecodeError::PayloadTooLong { max: MAX_PAYLOAD })
        );
    }
}
