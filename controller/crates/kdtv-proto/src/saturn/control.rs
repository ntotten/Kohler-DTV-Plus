//! The control-byte table, direction-aware, and the firmware type IDs.
//!
//! Tier `[C]`, from `research/xagon0/docs/protocols/saturn-protocol.md`
//! § Control Bytes and § Valve Firmware Types.
//!
//! Nothing in this module can transmit. It exists so a capture can be read and
//! so a decoder can name what it rejected. The set of frames this system is
//! able to *emit* is [`crate::saturn::SaturnOp`], which is a much shorter list.

use core::fmt;

/// Every documented control byte, as named constants.
///
/// **Write opcodes are not derived from read opcodes.** The `read | 0x80`
/// pattern holds for `0x01`/`0x81`, `0x02`/`0x82`, `0x07`/`0x87`,
/// `0x0B`/`0x8B`, `0x0C`/`0x8C` and `0x15`/`0x95`, and then breaks three times:
/// calibration is `0x10` read but `0xC0` write, generic outlet is `0x16` read
/// but `0xA1` write, and extended status is `0x40` read but `0xA4` write. A
/// computed mapping would send a factory-calibration write where a read was
/// meant. `CMD-05`.
pub mod opcode {
    // -- Address management. One control byte, three subcommands. `CMD-01`.
    /// `0x3A`. The subcommand is `DATA[0]`.
    pub const ADDRESS_MANAGEMENT: u8 = 0x3A;
    /// `DATA[0]` = `0x01`: discover unaddressed valves. Master to broadcast.
    pub const SUB_ENQUIRY: u8 = 0x01;
    /// `DATA[0]` = `0x02`: allocate an address. Carries the new address.
    pub const SUB_ALLOCATE: u8 = 0x02;
    /// `DATA[0]` = `0x03`: clear all assigned addresses. Master to broadcast.
    pub const SUB_CLEAR: u8 = 0x03;

    // -- Read commands, master to device. `CMD-02`.
    pub const READ_FIRMWARE_VERSION: u8 = 0x01;
    pub const READ_FIRMWARE_TYPE: u8 = 0x02;
    pub const READ_OUTLET_STATES: u8 = 0x07;
    pub const READ_TEMPERATURE: u8 = 0x0B;
    pub const READ_FLOW_RATE: u8 = 0x0C;
    pub const READ_FAULT_FLAGS: u8 = 0x0F;
    pub const READ_CALIBRATION: u8 = 0x10;
    pub const READ_SERIAL_NUMBER: u8 = 0x11;
    pub const READ_CONFIGURATION: u8 = 0x15;
    pub const READ_GENERIC_OUTLET: u8 = 0x16;
    pub const READ_EXTENDED_STATUS: u8 = 0x40;
    pub const READ_DIAGNOSTICS: u8 = 0x54;

    // -- Write commands, master to device. `CMD-03`.
    pub const WRITE_FIRMWARE_VERSION: u8 = 0x81;
    pub const WRITE_FIRMWARE_TYPE: u8 = 0x82;
    pub const WRITE_OUTLET_STATES: u8 = 0x87;
    pub const WRITE_TARGET_TEMPERATURE: u8 = 0x8B;
    pub const WRITE_TARGET_FLOW_RATE: u8 = 0x8C;
    pub const WRITE_CONFIGURATION: u8 = 0x95;
    pub const WRITE_PAUSE_STATE: u8 = 0x99;
    pub const WRITE_GENERIC_OUTLET: u8 = 0xA1;
    pub const WRITE_EXTENDED_CONTROL: u8 = 0xA4;
    pub const WRITE_CALIBRATION: u8 = 0xC0;

    // -- System commands, master to device. `CMD-04`.
    pub const FACTORY_RESET: u8 = 0xF4;
    pub const ENTER_BOOTLOADER: u8 = 0xF6;
    pub const CALIBRATE_FLOW_SENSOR: u8 = 0xF7;

    // -- Response indicators, device to master.
    /// `0x01`. Ambiguous by design: device to master this is an ACK, master to
    /// device it is read-firmware-version. Resolved by direction, never by a
    /// flat map. `CMD-06`.
    pub const RESPONSE_ACK: u8 = 0x01;
    /// `0x80`. Carries a one-byte error code in `DATA`. `CMD-08`.
    pub const RESPONSE_ERROR: u8 = 0x80;
    /// `0xFF`. Command rejected, no data. `CMD-08`.
    ///
    /// Numerically equal to the `Prompt 3 Flow Control` firmware type ID and
    /// deliberately not the same type — see [`super::FirmwareTypeId`].
    /// `CMD-09`.
    pub const RESPONSE_NAK: u8 = 0xFF;
}

/// Control bytes this system will never emit, asserted by a scan over every
/// frame the encoder can produce.
///
/// Each is denied by the absence of a [`crate::saturn::SaturnOp`] variant; this
/// list is the test's expectation, not the enforcement.
///
/// | Byte | Operation | Why denied |
/// | --- | --- | --- |
/// | `0x81` | write firmware version | factory operation, permanent |
/// | `0x82` | write firmware type | factory operation, permanent |
/// | `0x8C` | write target flow rate | outside the operations this master needs; not in the allowlist |
/// | `0x95` | write configuration | EEPROM write, permanent |
/// | `0xA1` | write generic outlet control | a second, unverified path to opening water |
/// | `0xA4` | write extended control | undocumented payload |
/// | `0xC0` | write calibration | EEPROM write; the Phase 0 baseline is the only rollback |
/// | `0xF4` | factory reset | destroys the valve's stored configuration |
/// | `0xF6` | enter bootloader | leaves the valve unable to run |
/// | `0xF7` | calibrate flow sensor | drives the valve through its range unattended |
///
/// The corresponding **reads** — calibration `0x10` and configuration `0x15` —
/// are allowed, and are required: `PH0-01` and the manual rollback procedure
/// both need each valve's calibration code read back and diffed against the
/// Phase 0 baseline, and once the K-99695 is powered down this service is the
/// only thing that can do it. `CORRECTIONS.md` item 7.
#[must_use]
pub const fn denied_control_bytes() -> &'static [u8] {
    &[
        opcode::WRITE_FIRMWARE_VERSION,
        opcode::WRITE_FIRMWARE_TYPE,
        opcode::WRITE_TARGET_FLOW_RATE,
        opcode::WRITE_CONFIGURATION,
        opcode::WRITE_GENERIC_OUTLET,
        opcode::WRITE_EXTENDED_CONTROL,
        opcode::WRITE_CALIBRATION,
        opcode::FACTORY_RESET,
        opcode::ENTER_BOOTLOADER,
        opcode::CALIBRATE_FLOW_SENSOR,
    ]
}

/// A control byte as it appeared on the wire.
///
/// A raw byte with no interpretation attached, because interpretation needs the
/// direction. Distinct from [`FirmwareTypeId`] so `0xFF` NAK and `0xFF` Prompt 3
/// Flow Control cannot be confused. `CMD-09`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ControlByte(pub u8);

impl ControlByte {
    /// What this byte means travelling master to valve.
    #[must_use]
    pub const fn as_command(self) -> MasterControl {
        MasterControl::of(self.0)
    }

    /// What this byte means travelling valve to master.
    #[must_use]
    pub const fn as_response(self) -> ValveControl {
        ValveControl::of(self.0)
    }
}

impl fmt::Debug for ControlByte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ControlByte(0x{:02X})", self.0)
    }
}

impl fmt::Display for ControlByte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02X}", self.0)
    }
}

/// The master-to-valve reading of a control byte.
///
/// Decode-only. Several variants name operations this system will never emit;
/// they are here so a capture of the *stock* controller can be read, and there
/// is no function anywhere that turns one back into a frame.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum MasterControl {
    AddressManagement,
    ReadFirmwareVersion,
    ReadFirmwareType,
    ReadOutletStates,
    ReadTemperature,
    ReadFlowRate,
    ReadFaultFlags,
    ReadCalibration,
    ReadSerialNumber,
    ReadConfiguration,
    ReadGenericOutlet,
    ReadExtendedStatus,
    ReadDiagnostics,
    WriteFirmwareVersion,
    WriteFirmwareType,
    WriteOutletStates,
    WriteTargetTemperature,
    WriteTargetFlowRate,
    WriteConfiguration,
    WritePauseState,
    WriteGenericOutlet,
    WriteExtendedControl,
    WriteCalibration,
    FactoryReset,
    EnterBootloader,
    CalibrateFlowSensor,
    /// Not in any documented table. Never mapped to a default.
    Undocumented(u8),
}

impl MasterControl {
    #[must_use]
    pub const fn of(byte: u8) -> Self {
        match byte {
            opcode::ADDRESS_MANAGEMENT => Self::AddressManagement,
            opcode::READ_FIRMWARE_VERSION => Self::ReadFirmwareVersion,
            opcode::READ_FIRMWARE_TYPE => Self::ReadFirmwareType,
            opcode::READ_OUTLET_STATES => Self::ReadOutletStates,
            opcode::READ_TEMPERATURE => Self::ReadTemperature,
            opcode::READ_FLOW_RATE => Self::ReadFlowRate,
            opcode::READ_FAULT_FLAGS => Self::ReadFaultFlags,
            opcode::READ_CALIBRATION => Self::ReadCalibration,
            opcode::READ_SERIAL_NUMBER => Self::ReadSerialNumber,
            opcode::READ_CONFIGURATION => Self::ReadConfiguration,
            opcode::READ_GENERIC_OUTLET => Self::ReadGenericOutlet,
            opcode::READ_EXTENDED_STATUS => Self::ReadExtendedStatus,
            opcode::READ_DIAGNOSTICS => Self::ReadDiagnostics,
            opcode::WRITE_FIRMWARE_VERSION => Self::WriteFirmwareVersion,
            opcode::WRITE_FIRMWARE_TYPE => Self::WriteFirmwareType,
            opcode::WRITE_OUTLET_STATES => Self::WriteOutletStates,
            opcode::WRITE_TARGET_TEMPERATURE => Self::WriteTargetTemperature,
            opcode::WRITE_TARGET_FLOW_RATE => Self::WriteTargetFlowRate,
            opcode::WRITE_CONFIGURATION => Self::WriteConfiguration,
            opcode::WRITE_PAUSE_STATE => Self::WritePauseState,
            opcode::WRITE_GENERIC_OUTLET => Self::WriteGenericOutlet,
            opcode::WRITE_EXTENDED_CONTROL => Self::WriteExtendedControl,
            opcode::WRITE_CALIBRATION => Self::WriteCalibration,
            opcode::FACTORY_RESET => Self::FactoryReset,
            opcode::ENTER_BOOTLOADER => Self::EnterBootloader,
            opcode::CALIBRATE_FLOW_SENSOR => Self::CalibrateFlowSensor,
            other => Self::Undocumented(other),
        }
    }
}

/// The valve-to-master reading of a control byte.
///
/// Read commands echo the request's control byte in the reply, so most of this
/// space is [`ValveControl::Echo`] and only `0x80` and `0xFF` are special.
/// `CMD-07` / `CMD-08`.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ValveControl {
    /// `0x80`, one-byte error code in `DATA`.
    Error,
    /// `0xFF`, no data.
    Nak,
    /// Any other byte, read as the echo of the request's control byte. `0x01`
    /// lands here and is simultaneously the documented bare ACK — the two are
    /// indistinguishable in this field, which is why a response is correlated
    /// with its request by the strictly serialised one-in-flight rule rather
    /// than by the frame. `TIME-06`.
    Echo(u8),
}

impl ValveControl {
    #[must_use]
    pub const fn of(byte: u8) -> Self {
        match byte {
            opcode::RESPONSE_ERROR => Self::Error,
            opcode::RESPONSE_NAK => Self::Nak,
            other => Self::Echo(other),
        }
    }
}

/// The firmware type ID a valve reports, as a raw byte.
///
/// A separate type from [`ControlByte`] on purpose: `0xFF` is `Prompt 3 Flow
/// Control` here and NAK there, and the two must not share an enum. `CMD-09`.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FirmwareTypeId(pub u8);

impl FirmwareTypeId {
    #[must_use]
    pub const fn classify(self) -> FirmwareType {
        FirmwareType::of(self.0)
    }
}

impl fmt::Debug for FirmwareTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FirmwareTypeId(0x{:02X})", self.0)
    }
}

/// The five documented valve firmware types. `RESP-03`.
///
/// An unrecognised byte becomes [`FirmwareType::Unknown`] carrying the byte —
/// never a default. Guessing a valve family picks the wrong outlet bitmap, and
/// on this hardware that opens a different outlet than the operator asked for.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum FirmwareType {
    /// `0x00` — no valve, or an uninitialised one.
    Null,
    /// `0x06` — DTV 6-Port, six outlets.
    Dtv6Port,
    /// `0x17` — Prompt 2-Port, two outlets.
    Prompt2Port,
    /// `0x1E` — Prompt 3-Port, three outlets.
    Prompt3Port,
    /// `0xFF` — Prompt 3 with flow rate control, three outlets.
    Prompt3FlowControl,
    /// Anything else.
    Unknown(u8),
}

impl FirmwareType {
    #[must_use]
    pub const fn of(byte: u8) -> Self {
        match byte {
            0x00 => Self::Null,
            0x06 => Self::Dtv6Port,
            0x17 => Self::Prompt2Port,
            0x1E => Self::Prompt3Port,
            0xFF => Self::Prompt3FlowControl,
            other => Self::Unknown(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CMD-05`. The three places the `read | 0x80` shortcut sends a
    /// destructive write where a read was meant.
    #[test]
    fn write_opcodes_are_not_derived_from_read_opcodes() {
        // Where the pattern happens to hold.
        for (read, write) in [
            (
                opcode::READ_FIRMWARE_VERSION,
                opcode::WRITE_FIRMWARE_VERSION,
            ),
            (opcode::READ_FIRMWARE_TYPE, opcode::WRITE_FIRMWARE_TYPE),
            (opcode::READ_OUTLET_STATES, opcode::WRITE_OUTLET_STATES),
            (opcode::READ_TEMPERATURE, opcode::WRITE_TARGET_TEMPERATURE),
            (opcode::READ_FLOW_RATE, opcode::WRITE_TARGET_FLOW_RATE),
            (opcode::READ_CONFIGURATION, opcode::WRITE_CONFIGURATION),
        ] {
            assert_eq!(read | 0x80, write);
        }

        // Where it breaks. These are the ones that matter.
        assert_ne!(opcode::READ_CALIBRATION | 0x80, opcode::WRITE_CALIBRATION);
        assert_eq!(opcode::READ_CALIBRATION | 0x80, 0x90);
        assert_ne!(
            opcode::READ_GENERIC_OUTLET | 0x80,
            opcode::WRITE_GENERIC_OUTLET
        );
        assert_ne!(
            opcode::READ_EXTENDED_STATUS | 0x80,
            opcode::WRITE_EXTENDED_CONTROL
        );
        // 0x40 | 0x80 is 0xC0, which is the *calibration write*. Deriving the
        // extended-control opcode would burn a valve's EEPROM.
        assert_eq!(
            opcode::READ_EXTENDED_STATUS | 0x80,
            opcode::WRITE_CALIBRATION
        );
    }

    #[test]
    fn control_byte_is_direction_aware() {
        // CMD-06: 0x01 is two different things depending on which way it went.
        assert_eq!(
            ControlByte(0x01).as_command(),
            MasterControl::ReadFirmwareVersion
        );
        assert_eq!(ControlByte(0x01).as_response(), ValveControl::Echo(0x01));
        assert_eq!(ControlByte(0x80).as_response(), ValveControl::Error);
        assert_eq!(ControlByte(0xFF).as_response(), ValveControl::Nak);
        // 0xFF master-to-valve is in no table.
        assert_eq!(
            ControlByte(0xFF).as_command(),
            MasterControl::Undocumented(0xFF)
        );
    }

    /// Exhaustive: every byte that is not in a documented table must classify
    /// as `Undocumented`, and must carry the byte so a capture keeps it.
    #[test]
    fn master_control_maps_all_256_bytes_without_a_default() {
        let documented: &[u8] = &[
            0x3A, 0x01, 0x02, 0x07, 0x0B, 0x0C, 0x0F, 0x10, 0x11, 0x15, 0x16, 0x40, 0x54, 0x81,
            0x82, 0x87, 0x8B, 0x8C, 0x95, 0x99, 0xA1, 0xA4, 0xC0, 0xF4, 0xF6, 0xF7,
        ];
        let mut undocumented = 0u32;
        for b in 0u8..=255 {
            match MasterControl::of(b) {
                MasterControl::Undocumented(carried) => {
                    assert_eq!(carried, b);
                    assert!(!documented.contains(&b), "0x{b:02X} is documented");
                    undocumented += 1;
                }
                _ => assert!(documented.contains(&b), "0x{b:02X} is not documented"),
            }
        }
        assert_eq!(undocumented, 256 - 26);
    }

    /// `RESP-03`. 251 of the 256 bytes are unknown, and none of them silently
    /// becomes a valve family.
    #[test]
    fn firmware_type_maps_all_256_bytes_without_a_default() {
        let mut unknown = 0u32;
        for b in 0u8..=255 {
            match FirmwareTypeId(b).classify() {
                FirmwareType::Unknown(carried) => {
                    assert_eq!(carried, b);
                    unknown += 1;
                }
                FirmwareType::Null => assert_eq!(b, 0x00),
                FirmwareType::Dtv6Port => assert_eq!(b, 0x06),
                FirmwareType::Prompt2Port => assert_eq!(b, 0x17),
                FirmwareType::Prompt3Port => assert_eq!(b, 0x1E),
                FirmwareType::Prompt3FlowControl => assert_eq!(b, 0xFF),
            }
        }
        assert_eq!(unknown, 251);
    }

    /// `CMD-09`. `0xFF` in the control field and `0xFF` in the firmware-type
    /// field mean unrelated things, and no code path can carry one as the other.
    #[test]
    fn nak_and_flow_control_firmware_type_share_a_byte_but_not_a_type() {
        assert_eq!(opcode::RESPONSE_NAK, 0xFF);
        assert_eq!(ControlByte(0xFF).as_response(), ValveControl::Nak);
        assert_eq!(
            FirmwareTypeId(0xFF).classify(),
            FirmwareType::Prompt3FlowControl
        );
    }

    #[test]
    fn denied_list_is_the_documented_ten_and_holds_no_read() {
        let denied = denied_control_bytes();
        assert_eq!(denied.len(), 10);
        for b in denied {
            assert!(
                !matches!(
                    MasterControl::of(*b),
                    MasterControl::ReadCalibration | MasterControl::ReadConfiguration
                ),
                "0x{b:02X} is a read and must stay allowed"
            );
        }
        // CORRECTIONS.md item 7: the reads must not be on the list.
        assert!(!denied.contains(&opcode::READ_CALIBRATION));
        assert!(!denied.contains(&opcode::READ_CONFIGURATION));
        // The writes must be.
        assert!(denied.contains(&opcode::WRITE_CALIBRATION));
        assert!(denied.contains(&opcode::WRITE_CONFIGURATION));
    }
}
