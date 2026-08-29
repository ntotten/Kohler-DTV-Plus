//! Error codes, fault bitmaps, and the reason there is no `Healthy`.
//!
//! Two source tables assign incompatible meanings to codes 0, 1, 3, 7, 35, 36,
//! 60 and 71, and the senses of 0 and 1 are exactly inverted:
//! `saturn-protocol.md` says 0 is *no error*, `docs/devices/valve-control.md`
//! says 0 is `UNCONFIGURED` and 1 is `ERROR_OK`. A single flat enum would pick
//! one silently, and the one it picked would decide whether a zero byte
//! authorises opening water.
//!
//! So: the raw byte is carried through decoding, meaning requires naming a
//! table, and **no `Healthy` variant is reachable from the code byte alone.**
//! `ERR-01` / `ERR-02`, `CORRECTIONS.md` item 4.
//!
//! One consequence worth stating: the design register's claim that "below the
//! floor the valve returns error 3, parameter out of range" is table-dependent.
//! Under the other table 3 is `OVERTEMP_CONTROL`. Neither is asserted here.

use core::fmt;

/// The error code byte from a `0x80` response, undecoded.
///
/// One byte, from a seven-byte error response. Deliberately not the same type
/// as [`FaultBitmap`], which is two bytes from a `0x0F` read; no source gives a
/// mapping between them. `ERR-07`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RawErrorByte(pub u8);

impl RawErrorByte {
    /// What to do about it, under a named table. There is no argument-free
    /// version of this call.
    #[must_use]
    pub const fn disposition(self, table: ErrorTable) -> FaultDisposition {
        disposition(table, self)
    }
}

impl fmt::Debug for RawErrorByte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RawErrorByte({})", self.0)
    }
}

impl fmt::Display for RawErrorByte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Which of the two incompatible tables a caller is applying.
///
/// Naming one is mandatory. Which is correct for this hardware is unresolved
/// `[?]` and is what the Phase 1 capture exists to settle.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ErrorTable {
    /// `research/xagon0/docs/protocols/saturn-protocol.md` § Error Codes — a
    /// 115-entry protocol-and-fault table.
    SaturnProtocol,
    /// `docs/devices/valve-control.md` § Error Codes — an eight-entry
    /// device-fault table.
    ValveControl,
}

/// What the controller does about an error byte.
///
/// **There is no `Healthy` variant.** Codes 0 and 1 are the two the tables
/// disagree about most, and both land in
/// [`FaultDisposition::UnknownHealth`] — which never authorises opening a valve.
/// Every other byte fails closed.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum FaultDisposition {
    /// The byte does not establish health. Under one table this value is
    /// healthy and under the other it is not, so nothing is concluded.
    UnknownHealth { raw: RawErrorByte },
    /// Command every outlet off and latch until an operator clears it.
    AllOffLatch {
        raw: RawErrorByte,
        reason: LatchReason,
    },
    /// A fault no controller command can clear.
    Unrecoverable {
        raw: RawErrorByte,
        operator_message: &'static str,
    },
}

impl FaultDisposition {
    /// True when this disposition must close the valve. Both latching and
    /// unrecoverable dispositions do; unknown health does not command anything
    /// on its own, because it is the absence of information rather than a
    /// fault report.
    #[must_use]
    pub const fn requires_all_off(self) -> bool {
        matches!(self, Self::AllOffLatch { .. } | Self::Unrecoverable { .. })
    }

    /// The retry engine matches on this: an unrecoverable fault is never
    /// retried, because retrying a welded valve just fills the log. `ERR-06`.
    #[must_use]
    pub const fn is_retryable(self) -> bool {
        !matches!(self, Self::Unrecoverable { .. })
    }

    /// The byte, always. Every fault record logs it verbatim so a later capture
    /// can retro-classify what this build could not.
    #[must_use]
    pub const fn raw(self) -> RawErrorByte {
        match self {
            Self::UnknownHealth { raw }
            | Self::AllOffLatch { raw, .. }
            | Self::Unrecoverable { raw, .. } => raw,
        }
    }
}

/// Why a fault latched, coarse enough to be true under either table.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum LatchReason {
    OverTemperature,
    UnderTemperature,
    TemperatureSensor,
    FlowSensor,
    Motor,
    Relay,
    Eeprom,
    Protocol,
    Communication,
    Calibration,
    Firmware,
    Internal,
    Application,
    /// The table has no entry for this byte. Failing closed is the only safe
    /// reading of a fault code nobody documented.
    Unclassified,
}

/// The disposition of an error byte under a named table.
///
/// `ERR-02`: codes 0 and 1 never produce anything but
/// [`FaultDisposition::UnknownHealth`], under either table, and an
/// undocumented byte fails closed rather than defaulting to anything.
#[must_use]
pub const fn disposition(table: ErrorTable, raw: RawErrorByte) -> FaultDisposition {
    // The two bytes the tables invert. Neither can be read as healthy, so
    // neither is read at all.
    if raw.0 == 0 || raw.0 == 1 {
        return FaultDisposition::UnknownHealth { raw };
    }
    let reason = match table {
        ErrorTable::SaturnProtocol => saturn_protocol_reason(raw.0),
        ErrorTable::ValveControl => {
            // WELDED. The only code either table marks as beyond any command.
            if raw.0 == 35 {
                return FaultDisposition::Unrecoverable {
                    raw,
                    operator_message: "WELDED (35): the mixing valve is mechanically stuck and no \
                                       controller command can close it. Remove valve power and \
                                       close the hot and cold service shutoffs.",
                };
            }
            valve_control_reason(raw.0)
        }
    };
    FaultDisposition::AllOffLatch { raw, reason }
}

const fn saturn_protocol_reason(code: u8) -> LatchReason {
    match code {
        2..=9 => LatchReason::Protocol,
        10 | 11 => LatchReason::Eeprom,
        12 | 13 => LatchReason::TemperatureSensor,
        // 14 over-temperature, 72 inlet water too hot, 73 mixed water
        // over-temperature safety. Three separate table entries, one response.
        14 | 72 | 73 => LatchReason::OverTemperature,
        // 15 under-temperature, 71 inlet water too cold.
        15 | 71 => LatchReason::UnderTemperature,
        16..=18 => LatchReason::FlowSensor,
        19..=30 => LatchReason::Motor,
        31..=50 => LatchReason::Calibration,
        51..=60 => LatchReason::Communication,
        61..=70 => LatchReason::Firmware,
        74..=100 => LatchReason::Application,
        101..=113 => LatchReason::Internal,
        _ => LatchReason::Unclassified,
    }
}

const fn valve_control_reason(code: u8) -> LatchReason {
    match code {
        3 | 7 => LatchReason::OverTemperature,
        36 => LatchReason::Relay,
        60 | 71 => LatchReason::Motor,
        _ => LatchReason::Unclassified,
    }
}

/// The `saturn-protocol.md` reading of an error byte, for logs and capture
/// analysis. Never used to decide whether water may flow — that is
/// [`disposition`].
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum SaturnProtocolCode {
    NoError,
    UnknownCommand,
    InvalidParameter,
    ParameterOutOfRange,
    ChecksumError,
    PacketTooShort,
    PacketTooLong,
    InvalidAddress,
    DeviceBusy,
    NotSupportedInState,
    EepromWriteFailure,
    EepromReadFailure,
    TempSensorOpenCircuit,
    TempSensorShortCircuit,
    OverTemperature,
    UnderTemperature,
    FlowSensorFault,
    NoFlowDetected,
    OverFlowFault,
    MotorStall,
    MotorOvercurrent,
    CalibrationRequired,
    CalibrationInProgress,
    CalibrationFailed,
    CommunicationTimeout,
    BusContention,
    AddressConflict,
    FirmwareUpdateInProgress,
    FirmwareUpdateFailed,
    FirmwareCrcMismatch,
    InletWaterTooCold,
    InletWaterTooHot,
    MixedWaterOverTemperatureSafety,
    OutletBlocked,
    InternalWatchdogReset,
    StackOverflow,
    HeapAllocationFailure,
    AssertionFailure,
    Unclassified,
    /// A range the table marks reserved, carrying the byte and the block it
    /// falls in.
    Reserved(ReservedBlock, u8),
    /// Outside the table entirely — 115..=255. Never mapped to a default.
    Unknown(u8),
}

/// The reserved blocks `saturn-protocol.md` names but does not enumerate.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ReservedBlock {
    MotorActuator,
    Calibration,
    Communication,
    Firmware,
    Application,
    Internal,
}

impl SaturnProtocolCode {
    #[must_use]
    pub const fn of(code: u8) -> Self {
        match code {
            0 => Self::NoError,
            1 => Self::UnknownCommand,
            2 => Self::InvalidParameter,
            3 => Self::ParameterOutOfRange,
            4 => Self::ChecksumError,
            5 => Self::PacketTooShort,
            6 => Self::PacketTooLong,
            7 => Self::InvalidAddress,
            8 => Self::DeviceBusy,
            9 => Self::NotSupportedInState,
            10 => Self::EepromWriteFailure,
            11 => Self::EepromReadFailure,
            12 => Self::TempSensorOpenCircuit,
            13 => Self::TempSensorShortCircuit,
            14 => Self::OverTemperature,
            15 => Self::UnderTemperature,
            16 => Self::FlowSensorFault,
            17 => Self::NoFlowDetected,
            18 => Self::OverFlowFault,
            19 => Self::MotorStall,
            20 => Self::MotorOvercurrent,
            21..=30 => Self::Reserved(ReservedBlock::MotorActuator, code),
            31 => Self::CalibrationRequired,
            32 => Self::CalibrationInProgress,
            33 => Self::CalibrationFailed,
            34..=50 => Self::Reserved(ReservedBlock::Calibration, code),
            51 => Self::CommunicationTimeout,
            52 => Self::BusContention,
            53 => Self::AddressConflict,
            54..=60 => Self::Reserved(ReservedBlock::Communication, code),
            61 => Self::FirmwareUpdateInProgress,
            62 => Self::FirmwareUpdateFailed,
            63 => Self::FirmwareCrcMismatch,
            64..=70 => Self::Reserved(ReservedBlock::Firmware, code),
            71 => Self::InletWaterTooCold,
            72 => Self::InletWaterTooHot,
            73 => Self::MixedWaterOverTemperatureSafety,
            74 => Self::OutletBlocked,
            75..=100 => Self::Reserved(ReservedBlock::Application, code),
            101 => Self::InternalWatchdogReset,
            102 => Self::StackOverflow,
            103 => Self::HeapAllocationFailure,
            104 => Self::AssertionFailure,
            105..=113 => Self::Reserved(ReservedBlock::Internal, code),
            114 => Self::Unclassified,
            other => Self::Unknown(other),
        }
    }
}

/// The `valve-control.md` reading of the same byte. `ERR-04`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ValveControlCode {
    Unconfigured,
    ErrorOk,
    OvertempControl,
    OvertempOutlet,
    /// 35. A mechanically stuck mixing valve. `ERR-06`.
    Welded,
    RelayFault,
    MotorStuck,
    MotorHoming,
    /// Outside the eight-entry table. Never mapped to a default.
    Unknown(u8),
}

impl ValveControlCode {
    #[must_use]
    pub const fn of(code: u8) -> Self {
        match code {
            0 => Self::Unconfigured,
            1 => Self::ErrorOk,
            3 => Self::OvertempControl,
            7 => Self::OvertempOutlet,
            35 => Self::Welded,
            36 => Self::RelayFault,
            60 => Self::MotorStuck,
            71 => Self::MotorHoming,
            other => Self::Unknown(other),
        }
    }
}

/// The two-byte fault bitmap from a `0x0F` read.
///
/// No source gives its bit assignments, so nothing is decoded from it. It is
/// logged verbatim on every read so the bits can be inferred from captures
/// later, and **any nonzero value fails closed** rather than waiting for a
/// meaning. `ERR-07`.
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct FaultBitmap(pub u16);

impl FaultBitmap {
    #[must_use]
    pub const fn is_clear(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Debug for FaultBitmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FaultBitmap(0x{:04X})", self.0)
    }
}

/// A two-byte response field, carried in both byte orders.
///
/// **No source states the endianness of any multi-byte Saturn field** —
/// temperature `0x0B`, flow `0x0C`, fault flags `0x0F` and outlet states `0x07`
/// are all specified as "2 bytes" and nothing more. `RESP-05`. Rather than pick
/// one and log a plausible-looking wrong number, both readings are carried until
/// a real capture settles it.
///
/// Cx2 temperature needs only one byte for the whole 0–127.5 °C range, so the
/// second temperature byte's role is itself unknown `[?]`: high byte, sign,
/// status, or a second sensor.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct TwoByteField {
    pub raw: [u8; 2],
    pub be: u16,
    pub le: u16,
}

impl TwoByteField {
    #[must_use]
    pub const fn new(raw: [u8; 2]) -> Self {
        Self {
            raw,
            be: u16::from_be_bytes(raw),
            le: u16::from_le_bytes(raw),
        }
    }

    /// True when the two readings agree, which they do only for a symmetric
    /// pair. Worth logging: it means this sample cannot settle the question.
    #[must_use]
    pub const fn is_ambiguous(self) -> bool {
        self.be == self.le
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ERR-02` / `CORRECTIONS.md` item 4. The load-bearing test: no byte, under
    /// either table, produces a disposition that says the valve is well.
    #[test]
    fn no_error_byte_reports_health_under_either_table() {
        for table in [ErrorTable::SaturnProtocol, ErrorTable::ValveControl] {
            for b in 0u8..=255 {
                let d = RawErrorByte(b).disposition(table);
                assert_eq!(d.raw(), RawErrorByte(b), "byte {b} lost its raw value");
                match d {
                    FaultDisposition::UnknownHealth { .. } => {
                        assert!(b <= 1, "byte {b} must not read as unknown health");
                        assert!(!d.requires_all_off());
                    }
                    FaultDisposition::AllOffLatch { .. }
                    | FaultDisposition::Unrecoverable { .. } => {
                        assert!(b > 1);
                        assert!(d.requires_all_off());
                    }
                }
            }
        }
    }

    /// The exact inversion `CORRECTIONS.md` names.
    #[test]
    fn codes_zero_and_one_are_inverted_between_the_tables_so_neither_is_trusted() {
        assert_eq!(SaturnProtocolCode::of(0), SaturnProtocolCode::NoError);
        assert_eq!(ValveControlCode::of(0), ValveControlCode::Unconfigured);
        assert_eq!(
            SaturnProtocolCode::of(1),
            SaturnProtocolCode::UnknownCommand
        );
        assert_eq!(ValveControlCode::of(1), ValveControlCode::ErrorOk);

        for table in [ErrorTable::SaturnProtocol, ErrorTable::ValveControl] {
            for b in [0u8, 1] {
                assert_eq!(
                    RawErrorByte(b).disposition(table),
                    FaultDisposition::UnknownHealth {
                        raw: RawErrorByte(b)
                    }
                );
            }
        }
    }

    /// `ERR-06`. WELDED is unrecoverable under the table that names it, and the
    /// retry engine can see that without a string comparison.
    #[test]
    fn welded_is_unrecoverable_and_never_retried() {
        let d = RawErrorByte(35).disposition(ErrorTable::ValveControl);
        assert!(matches!(d, FaultDisposition::Unrecoverable { .. }));
        assert!(!d.is_retryable());
        assert!(d.requires_all_off());
        assert_eq!(ValveControlCode::of(35), ValveControlCode::Welded);

        // Under the other table 35 is inside a reserved block, and the crate
        // does not pretend otherwise.
        assert_eq!(
            SaturnProtocolCode::of(35),
            SaturnProtocolCode::Reserved(ReservedBlock::Calibration, 35)
        );
        let other = RawErrorByte(35).disposition(ErrorTable::SaturnProtocol);
        assert!(matches!(other, FaultDisposition::AllOffLatch { .. }));
        assert!(other.is_retryable());
    }

    /// The code the register attributes to a below-floor setpoint. Both
    /// readings are recorded; neither is stated as fact.
    #[test]
    fn code_three_means_two_different_things() {
        assert_eq!(
            SaturnProtocolCode::of(3),
            SaturnProtocolCode::ParameterOutOfRange
        );
        assert_eq!(ValveControlCode::of(3), ValveControlCode::OvertempControl);
        assert!(matches!(
            RawErrorByte(3).disposition(ErrorTable::SaturnProtocol),
            FaultDisposition::AllOffLatch {
                reason: LatchReason::Protocol,
                ..
            }
        ));
        assert!(matches!(
            RawErrorByte(3).disposition(ErrorTable::ValveControl),
            FaultDisposition::AllOffLatch {
                reason: LatchReason::OverTemperature,
                ..
            }
        ));
    }

    /// `ERR-03`. Exhaustive: 115..=255 is outside the table and stays outside.
    #[test]
    fn saturn_protocol_table_maps_all_256_bytes_without_a_default() {
        let mut unknown = 0u32;
        for b in 0u8..=255 {
            match SaturnProtocolCode::of(b) {
                SaturnProtocolCode::Unknown(carried) => {
                    assert_eq!(carried, b);
                    assert!(b >= 115, "byte {b} is inside the documented table");
                    unknown += 1;
                }
                SaturnProtocolCode::Reserved(_, carried) => {
                    assert_eq!(carried, b);
                    assert!(b < 115);
                }
                _ => assert!(b < 115, "byte {b} is outside the documented table"),
            }
        }
        assert_eq!(unknown, 256 - 115);
    }

    /// `ERR-04`. Exhaustive: 248 of 256 bytes are unknown to this table.
    #[test]
    fn valve_control_table_maps_all_256_bytes_without_a_default() {
        let documented: &[u8] = &[0, 1, 3, 7, 35, 36, 60, 71];
        let mut unknown = 0u32;
        for b in 0u8..=255 {
            match ValveControlCode::of(b) {
                ValveControlCode::Unknown(carried) => {
                    assert_eq!(carried, b);
                    assert!(!documented.contains(&b));
                    unknown += 1;
                }
                _ => assert!(documented.contains(&b), "byte {b} is not in the table"),
            }
        }
        assert_eq!(unknown, 256 - 8);
    }

    /// An undocumented byte must fail closed, not fall through to something
    /// benign.
    #[test]
    fn undocumented_bytes_fail_closed_under_both_tables() {
        for b in [200u8, 250, 255] {
            for table in [ErrorTable::SaturnProtocol, ErrorTable::ValveControl] {
                let d = RawErrorByte(b).disposition(table);
                assert!(matches!(
                    d,
                    FaultDisposition::AllOffLatch {
                        reason: LatchReason::Unclassified,
                        ..
                    }
                ));
                assert!(d.requires_all_off());
            }
        }
    }

    /// `ERR-07`. Two sizes, two types, no conversion, and a nonzero bitmap is a
    /// fault regardless of which bits are set.
    #[test]
    fn fault_bitmap_is_not_an_error_byte() {
        assert!(FaultBitmap(0).is_clear());
        assert!(!FaultBitmap(0x0001).is_clear());
        assert!(!FaultBitmap(0x8000).is_clear());
        // The 1-byte code 35 and the 2-byte bitmap 35 are unrelated values.
        assert_ne!(
            core::mem::size_of::<FaultBitmap>(),
            core::mem::size_of::<RawErrorByte>()
        );
    }

    /// `RESP-05`. Both readings survive, and the codec says when a sample
    /// cannot distinguish them.
    #[test]
    fn two_byte_fields_carry_both_endiannesses() {
        let f = TwoByteField::new([0x01, 0x02]);
        assert_eq!(f.raw, [0x01, 0x02]);
        assert_eq!(f.be, 0x0102);
        assert_eq!(f.le, 0x0201);
        assert!(!f.is_ambiguous());
        assert!(TwoByteField::new([0x4C, 0x4C]).is_ambiguous());
        // The Cx2 hazard in miniature: 0x00 0x4C is 76 one way and 19456 the
        // other. Only one of those is 38.0 C.
        let t = TwoByteField::new([0x00, 0x4C]);
        assert_eq!(t.be, 76);
        assert_eq!(t.le, 0x4C00);
    }
}
