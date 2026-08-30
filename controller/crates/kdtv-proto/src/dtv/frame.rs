//! Framing, byte stuffing and the checksum.
//!
//! All tier `[C]`, from `research/xagon0/docs/protocols/dtv-plus-protocol.md`
//! § Frame Format, § Special Characters and Byte Stuffing and § Checksum
//! Calculation, cross-checked against `HARDWARE.md` § 12.
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
//! **No length field is transmitted.** Frame extent comes from the delimiters
//! alone, which is why every buffer here is bounded and a frame that grows past
//! [`MAX_FRAME`] is a fault rather than an allocation. `FRAME-06`.
//!
//! # Two byte domains, and the order they compose in
//!
//! There are two views of every frame and confusing them is the single easiest
//! mistake in this file:
//!
//! - **Logical bytes** — `DEST, SRC, CMD, PAYLOAD…, CHECKSUM`. This is what the
//!   checksum covers and what a decoder hands upward.
//! - **Wire bytes** — `SOF`, then the byte-stuffed logical bytes, then `EOF`.
//!
//! The checksum is computed over the **unescaped** logical bytes and is then
//! itself subject to stuffing, so escaping is strictly the last step before
//! transmission. `FRAME-09`. Reversing the two would produce a checksum over
//! `AA 55` rather than over `55`, and the document's own Example 2 is the case
//! that catches it: `88 03 00 34 01 AA 55 73 55` carries checksum `0x73`,
//! computed over the logical `03 00 34 01 55`. Summing the stuffed bytes gives
//! `0xC9` instead.
//!
//! `SOF` and `EOF` are never escaped **in their framing role** — as data bytes
//! they are. `FRAME-02` / `FRAME-03`.

use core::fmt;

/// Start of frame. Never escaped in its framing role. `FRAME-02`.
pub const SOF: u8 = 0x88;
/// End of frame. Never escaped in its framing role. `FRAME-02`.
pub const EOF: u8 = 0x55;
/// The escape prefix. `FRAME-03`.
pub const ESC: u8 = 0xAA;

/// The three bytes that must be stuffed wherever they appear in `DEST`, `SRC`,
/// `CMD`, `PAYLOAD` or `CHECKSUM`.
pub const RESERVED: [u8; 3] = [SOF, EOF, ESC];

/// True for the three bytes byte stuffing applies to.
#[must_use]
pub const fn is_reserved(b: u8) -> bool {
    b == SOF || b == EOF || b == ESC
}

/// The largest payload this codec will build or accept.
///
/// The steam profile needs six bytes (a status response) and three (a parameter
/// write); this is generous headroom, not a documented protocol limit — the
/// protocol states none. It exists so `FRAME-06`'s "abort on overrun rather than
/// growing without limit" has a number.
pub const MAX_PAYLOAD: usize = 16;

/// Logical bytes that are not payload: `DEST`, `SRC`, `CMD`, `CHECKSUM`.
pub const LOGICAL_OVERHEAD: usize = 4;

/// The longest logical byte sequence a frame can carry.
pub const MAX_LOGICAL: usize = LOGICAL_OVERHEAD + MAX_PAYLOAD;

/// The longest on-wire frame, with every logical byte stuffed.
///
/// Two delimiters plus a worst case of two wire bytes per logical byte. A frame
/// of all-reserved bytes really does double, which is why the bound is not
/// `2 + MAX_LOGICAL`; the round-trip property test feeds exactly that case.
pub const MAX_FRAME: usize = 2 + 2 * MAX_LOGICAL;

/// The number of wire bytes `logical` stuffs into, excluding the delimiters.
#[must_use]
pub fn escaped_len(logical: &[u8]) -> usize {
    logical.len() + logical.iter().filter(|b| is_reserved(**b)).count()
}

/// The checksum over `DEST + SRC + CMD + PAYLOAD`, as the 8-bit two's
/// complement of their sum. `STEAM-03`.
///
/// Computed over **unescaped** bytes. `SOF` and `EOF` are excluded — they are
/// delimiters, not content.
///
/// The steam device page calls this field a "CRC". There is no CRC anywhere in
/// this protocol; that is loose wording for this one-byte additive checksum, and
/// no CRC-8 fallback exists here.
///
/// Wrapping arithmetic throughout — the sum is defined modulo 256, and
/// `wrapping_neg` is exactly `(!total + 1) & 0xFF`.
#[must_use]
pub const fn checksum(dest: u8, src: u8, cmd: u8, payload: &[u8]) -> u8 {
    let mut total = dest.wrapping_add(src).wrapping_add(cmd);
    let mut rest = payload;
    // A `const fn` cannot use an iterator; `split_first` walks the slice without
    // indexing it.
    while let Some((byte, tail)) = rest.split_first() {
        total = total.wrapping_add(*byte);
        rest = tail;
    }
    total.wrapping_neg()
}

/// True when the covered bytes and the checksum sum to zero modulo 256.
/// `STEAM-03` § Verification.
#[must_use]
pub const fn checksum_valid(dest: u8, src: u8, cmd: u8, payload: &[u8], found: u8) -> bool {
    checksum(dest, src, cmd, payload) == found
}

/// The same rule applied to an already-assembled logical sequence: every byte
/// between `SOF` and `EOF`, checksum included, sums to zero.
#[must_use]
pub fn logical_sums_to_zero(logical: &[u8]) -> bool {
    logical.iter().fold(0u8, |a, b| a.wrapping_add(*b)) == 0
}

/// A wire buffer that could not hold the frame it was asked to hold.
#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
#[error("frame exceeds the {MAX_FRAME}-byte wire maximum")]
pub struct FrameTooLong;

/// Byte-stuffs `logical` and appends the result to `out`.
///
/// The delimiters are the caller's job: this function writes only the stuffed
/// middle. `FRAME-03`.
pub fn escape_into(
    logical: &[u8],
    out: &mut heapless::Vec<u8, MAX_FRAME>,
) -> Result<(), FrameTooLong> {
    for b in logical {
        if is_reserved(*b) {
            out.push(ESC).map_err(|_| FrameTooLong)?;
        }
        out.push(*b).map_err(|_| FrameTooLong)?;
    }
    Ok(())
}

/// What [`unescape_into`] saw on the way through.
#[derive(Copy, Clone, PartialEq, Eq, Debug, Default)]
pub struct UnescapeReport {
    /// Escape sequences whose escaped byte was not one of [`RESERVED`].
    ///
    /// **No source specifies this case.** `FRAME-07`: the byte is taken
    /// literally and the occurrence is counted, because a decoder that guessed
    /// would silently reshape a capture. A nonzero count is worth a log line
    /// during Phase 1 and means either line corruption or an encoder that does
    /// not follow the documented rule.
    pub anomalies: usize,
}

/// Why an escaped byte sequence could not be turned back into logical bytes.
#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum UnescapeError {
    /// The sequence ends with an escape that has nothing to escape.
    /// `FRAME-08`.
    #[error("the frame ends with a truncated escape sequence")]
    TruncatedEscape,
    /// More logical bytes than [`MAX_LOGICAL`].
    #[error("the frame unescapes to more than {max} logical bytes")]
    TooLong { max: usize },
}

/// Removes byte stuffing from the bytes between `SOF` and `EOF`.
///
/// `body` must be exactly the wire bytes between the two delimiters. Every
/// `0xAA` consumes the byte after it and that byte is taken literally, so an
/// escaped `0x55` never terminates a frame and an escaped `0x88` never
/// resynchronises one. `FRAME-04`.
pub fn unescape_into(
    body: &[u8],
    out: &mut heapless::Vec<u8, MAX_LOGICAL>,
) -> Result<UnescapeReport, UnescapeError> {
    let mut report = UnescapeReport::default();
    let mut escaped = false;
    for b in body {
        if escaped {
            if !is_reserved(*b) {
                report.anomalies += 1;
            }
            out.push(*b)
                .map_err(|_| UnescapeError::TooLong { max: MAX_LOGICAL })?;
            escaped = false;
        } else if *b == ESC {
            escaped = true;
        } else {
            out.push(*b)
                .map_err(|_| UnescapeError::TooLong { max: MAX_LOGICAL })?;
        }
    }
    if escaped {
        return Err(UnescapeError::TruncatedEscape);
    }
    Ok(report)
}

/// Line settings for the DTV+ bus. Identical to Saturn's, on a different bus.
/// `PHY-01`.
pub const BAUD: u32 = 9600;
/// Data bits.
pub const DATA_BITS: u8 = 8;
/// Stop bits.
pub const STOP_BITS: u8 = 1;

/// Which way a DTV+ frame travelled.
///
/// **Inferred from the opcode, never from the address and never from an echo.**
/// Both discovery frames in the documented handshake carry `DEST` `0x00` and
/// `SRC` `0x00`, because `0x00` means "master" *and* "unassigned device" at the
/// same time, so address alone cannot identify a sender. `ADDR-06`.
///
/// The echo half of that rule is `CORRECTIONS.md` item 3: the converters chosen
/// for this build have automatic direction control and present no local echo, so
/// there is no echo signal to key on. `PROTO-04` / `PROTO-06`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum DtvDirection {
    /// Only a master sends this opcode.
    MasterToDevice,
    /// Only a device sends this opcode.
    DeviceToMaster,
    /// Either side, or an opcode no source assigns a direction to.
    Indeterminate,
}

impl fmt::Display for DtvDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::MasterToDevice => "master -> device",
            Self::DeviceToMaster => "device -> master",
            Self::Indeterminate => "indeterminate",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The six worked DTV+ examples, recomputed. `ARITHMETIC-NOTES.md` checked
    /// all six by hand and every one holds as published — unlike the Saturn
    /// document, which needed two of its printed checksums corrected.
    #[test]
    fn the_documented_checksums_all_verify() {
        // Example 1, "Get Steam Generator Status" addressed to DEST 0x05.
        // Arithmetically correct, and semantically the documented-wrong frame:
        // 0x05 is the steam *device ID*, not an address. See `addr`.
        assert_eq!(checksum(0x05, 0x00, 0x30, &[]), 0xCB);

        // Example 2, the escaped payload. 0x73, not the 0x92 the document
        // prints before self-correcting.
        assert_eq!(checksum(0x03, 0x00, 0x34, &[0x01, 0x55]), 0x73);
        assert_ne!(checksum(0x03, 0x00, 0x34, &[0x01, 0x55]), 0x92);

        // Example 3, the three-step discovery handshake.
        assert_eq!(checksum(0xFF, 0x00, 0x05, &[]), 0xFC);
        assert_eq!(checksum(0x00, 0x00, 0x06, &[0x05]), 0xF5);
        assert_eq!(checksum(0x00, 0x00, 0x07, &[0x03]), 0xF6);

        // The steam device page's status request, at the address the reference
        // topology actually assigns.
        assert_eq!(checksum(0x03, 0x00, 0x30, &[]), 0xCD);

        for (d, s, c, p, chk) in [
            (0x05u8, 0x00u8, 0x30u8, &[][..], 0xCBu8),
            (0x03, 0x00, 0x34, &[0x01, 0x55][..], 0x73),
            (0xFF, 0x00, 0x05, &[][..], 0xFC),
            (0x00, 0x00, 0x06, &[0x05][..], 0xF5),
            (0x00, 0x00, 0x07, &[0x03][..], 0xF6),
            (0x03, 0x00, 0x30, &[][..], 0xCD),
        ] {
            assert!(checksum_valid(d, s, c, p, chk));
            let mut logical = vec![d, s, c];
            logical.extend_from_slice(p);
            logical.push(chk);
            assert!(logical_sums_to_zero(&logical));
        }
    }

    /// `FRAME-09`. The checksum is computed over logical bytes; stuffing is
    /// applied afterwards. Doing it the other way round gives a different
    /// answer, and this is the frame that shows it.
    #[test]
    fn the_checksum_covers_unescaped_bytes_only() {
        // The correct value, over the logical payload [0x01, 0x55].
        assert_eq!(checksum(0x03, 0x00, 0x34, &[0x01, 0x55]), 0x73);
        // Summing the stuffed payload [0x01, 0xAA, 0x55] instead.
        let over_stuffed = checksum(0x03, 0x00, 0x34, &[0x01, ESC, 0x55]);
        assert_eq!(over_stuffed, 0xC9);
        assert_ne!(over_stuffed, 0x73);
        // And folding the delimiters in as well.
        let with_delimiters = checksum(SOF, 0x03, 0x00, &[0x34, 0x01, 0x55, EOF]);
        assert_ne!(with_delimiters, 0x73);
    }

    #[test]
    fn escaping_covers_exactly_the_three_reserved_bytes() {
        for b in 0u8..=255 {
            let mut out: heapless::Vec<u8, MAX_FRAME> = heapless::Vec::new();
            escape_into(&[b], &mut out).unwrap();
            if is_reserved(b) {
                assert_eq!(out.as_slice(), &[ESC, b], "0x{b:02X}");
            } else {
                assert_eq!(out.as_slice(), &[b], "0x{b:02X}");
            }
            assert_eq!(out.len(), escaped_len(&[b]));
        }
        assert_eq!(RESERVED, [0x88, 0x55, 0xAA]);
    }

    #[test]
    fn escape_and_unescape_are_inverse_for_every_byte_pair() {
        for a in 0u8..=255 {
            for b in 0u8..=255 {
                let logical = [a, b];
                let mut wire: heapless::Vec<u8, MAX_FRAME> = heapless::Vec::new();
                escape_into(&logical, &mut wire).unwrap();
                let mut back: heapless::Vec<u8, MAX_LOGICAL> = heapless::Vec::new();
                let report = unescape_into(&wire, &mut back).unwrap();
                assert_eq!(back.as_slice(), &logical, "0x{a:02X} 0x{b:02X}");
                assert_eq!(report.anomalies, 0);
            }
        }
    }

    /// The worst case the [`MAX_FRAME`] bound exists for: every logical byte
    /// reserved, so every one doubles.
    #[test]
    fn an_all_reserved_payload_doubles_and_still_fits() {
        let logical = [ESC; MAX_LOGICAL];
        let mut wire: heapless::Vec<u8, MAX_FRAME> = heapless::Vec::new();
        escape_into(&logical, &mut wire).unwrap();
        assert_eq!(wire.len(), 2 * MAX_LOGICAL);
        assert_eq!(wire.len() + 2, MAX_FRAME);
        let mut back: heapless::Vec<u8, MAX_LOGICAL> = heapless::Vec::new();
        unescape_into(&wire, &mut back).unwrap();
        assert_eq!(back.as_slice(), &logical);
    }

    /// `FRAME-08`. A body ending in a lone escape has nothing to escape.
    #[test]
    fn a_truncated_escape_is_rejected() {
        let mut back: heapless::Vec<u8, MAX_LOGICAL> = heapless::Vec::new();
        assert_eq!(
            unescape_into(&[0x03, 0x00, 0x34, 0x74, ESC], &mut back),
            Err(UnescapeError::TruncatedEscape)
        );
        // The odd/even rule: an even run of escapes is data, an odd run is a
        // truncation.
        for n in 1usize..=6 {
            let body = vec![ESC; n];
            let mut back: heapless::Vec<u8, MAX_LOGICAL> = heapless::Vec::new();
            let out = unescape_into(&body, &mut back);
            if n % 2 == 0 {
                assert_eq!(back.len(), n / 2);
                assert!(out.is_ok());
            } else {
                assert_eq!(out, Err(UnescapeError::TruncatedEscape));
            }
        }
    }

    /// `FRAME-07`. `0xAA` followed by a byte no source covers: take it
    /// literally, count it, do not guess.
    #[test]
    fn an_escape_of_a_non_reserved_byte_is_literal_and_counted() {
        let mut back: heapless::Vec<u8, MAX_LOGICAL> = heapless::Vec::new();
        let report = unescape_into(&[0x03, ESC, 0x42, 0x00], &mut back).unwrap();
        assert_eq!(back.as_slice(), &[0x03, 0x42, 0x00]);
        assert_eq!(report.anomalies, 1);
    }

    #[test]
    fn unescaping_is_bounded() {
        let body = vec![0x00u8; MAX_LOGICAL + 1];
        let mut back: heapless::Vec<u8, MAX_LOGICAL> = heapless::Vec::new();
        assert_eq!(
            unescape_into(&body, &mut back),
            Err(UnescapeError::TooLong { max: MAX_LOGICAL })
        );
    }

    #[test]
    fn the_size_constants_agree_with_the_arithmetic() {
        assert_eq!(LOGICAL_OVERHEAD, 4);
        assert_eq!(MAX_LOGICAL, MAX_PAYLOAD + LOGICAL_OVERHEAD);
        assert_eq!(MAX_FRAME, 2 + 2 * MAX_LOGICAL);
        assert_eq!(BAUD, 9600);
        assert_eq!((DATA_BITS, STOP_BITS), (8, 1));
    }
}
