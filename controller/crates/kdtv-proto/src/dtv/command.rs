//! The DTV+ opcode table, the allowlist, and the denied set.
//!
//! Tier `[C]`, from `research/xagon0/docs/protocols/dtv-plus-protocol.md`
//! § Command Set, narrowed by `docs/replacement-controller/STEAM-ADAPTER.md`
//! § 3.
//!
//! Nothing in this module can transmit. It exists so a capture can be read and
//! so a decoder can name what it rejected. The set of frames this system is able
//! to *emit* is [`crate::dtv::SteamOp`], which resolves to five opcodes.

use crate::dtv::frame::DtvDirection;

/// Every documented DTV+ opcode, as named constants.
///
/// Unlike Saturn's control bytes there is no read/write bit pattern here at all
/// — the opcodes are a flat enumeration and must be spelled out. Note that
/// `0x30`, `0x31` and `0x40` collide numerically with the device IDs for UI v1,
/// UI v2 and the amplifier; a decoder must never infer a field's meaning from
/// its value.
pub mod opcode {
    // -- Network management.
    /// `0x03`. Master to broadcast. **Denied**: resets every device address on
    /// the bus.
    pub const NETWORK_RESET: u8 = 0x03;
    /// `0x05`. Master to broadcast. Invites unaddressed devices to respond.
    pub const DEV_ADDRESS_OPP: u8 = 0x05;
    /// `0x06`. Device to master, payload = the device ID. The master never
    /// sends this one.
    pub const DEV_REQUEST_ADDR: u8 = 0x06;
    /// `0x07`. Master to the still-unaddressed device, payload = the assigned
    /// address.
    pub const DEV_ASSIGN_ADDR: u8 = 0x07;

    // -- Configuration.
    /// `0x13`. **Denied**: sets an external authentication key.
    pub const SET_EXT_KEY: u8 = 0x13;
    /// `0x18`. **Denied**: changes the UART baud rate, which would strand the
    /// link at a rate this master does not open.
    pub const CHANGE_BAUD: u8 = 0x18;
    /// `0x40`. **Denied**: identify (flash an LED). Numerically equal to the
    /// amplifier's device ID.
    pub const ID_DEV: u8 = 0x40;

    // -- Data transfer. All denied.
    /// `0x20`. **Denied**.
    pub const SET_FILE_TRANSFER: u8 = 0x20;
    /// `0x21`. **Denied**.
    pub const FLUSH_MD5: u8 = 0x21;
    /// `0x22`. **Denied**.
    pub const FILE_COMPLETE: u8 = 0x22;
    /// `0x70`. **Denied**.
    pub const WRITE_DATA: u8 = 0x70;
    /// `0x71`. **Denied**.
    pub const READ_DATA: u8 = 0x71;
    /// `0x72`. **Denied**. Device to master.
    pub const GET_DATA_BUFFER: u8 = 0x72;
    /// `0x74`. **Denied**. Carries the 2000 ms extended timeout, which is why
    /// it is denied twice over: the opcode is absent and no timing path exists
    /// for it.
    pub const WRITE_LARGE_DATA: u8 = 0x74;

    // -- Status and control.
    /// `0x30`. Master to device. Request current status. Numerically equal to
    /// the UI v1 device ID.
    pub const GET_DEV_STATUS: u8 = 0x30;
    /// `0x31`. Device to master. Numerically equal to the UI v2 device ID.
    pub const STATUS_UPDATE: u8 = 0x31;
    /// `0x32`. Either direction. **Denied**.
    pub const SETTINGS_UPDATE: u8 = 0x32;
    /// `0x33`. Master to device. **Denied**: reads an arbitrary parameter.
    pub const GET_DEV_PARAM: u8 = 0x33;
    /// `0x34`. Master to device. Writes the steam parameter block.
    pub const SET_DEV_PARAM: u8 = 0x34;
    /// `0x35`. Device to master. Positive acknowledgement.
    pub const DEV_ACK: u8 = 0x35;
    /// `0x36`. Device to master, payload = an error byte.
    pub const DEV_NAK: u8 = 0x36;
    /// `0x37`. Device to master. Error report with a code.
    pub const ERROR: u8 = 0x37;
    /// `0x38`. Master to device. **Denied**.
    pub const GET_FIRMWARE_VERSION: u8 = 0x38;
    /// `0x39`. Master to device. **Denied**: puts the generator into firmware
    /// update mode.
    pub const FIRMWARE_UPDATE: u8 = 0x39;
    /// `0x3A`. Master to device. Clears stored fault flags.
    pub const CLEAR_FAULT_FLAGS: u8 = 0x3A;

    // -- System. All denied.
    /// `0x80`. **Denied**: reboots the device.
    pub const REBOOT: u8 = 0x80;
    /// `0x90`. **Denied**.
    pub const GET_TRACK_STR: u8 = 0x90;
    /// `0x91`. **Denied**.
    pub const GET_DIR_ENTRIES: u8 = 0x91;
    /// `0xA1`. **Denied**: activates the bootloader.
    pub const ACTIVATE_BOOT: u8 = 0xA1;
}

/// The opcodes this master is permitted to *recognise* — the ones it sends plus
/// the ones the steam device may answer with. `STEAM-06`.
///
/// Being on this list is not permission to transmit. Four of the seven
/// (`0x31`, `0x35`, `0x36`, `0x37`) are device-to-master only and have no
/// encoder variant at all; see [`direction_of`].
#[must_use]
pub const fn allowlisted_opcodes() -> &'static [u8] {
    &[
        opcode::GET_DEV_STATUS,
        opcode::STATUS_UPDATE,
        opcode::SET_DEV_PARAM,
        opcode::DEV_ACK,
        opcode::DEV_NAK,
        opcode::ERROR,
        opcode::CLEAR_FAULT_FLAGS,
    ]
}

/// The three discovery opcodes, permitted only in [`LinkPhase::Discovery`] with
/// water off. `STEAM-04` / `STEAM-17`.
///
/// [`LinkPhase::Discovery`]: crate::saturn::LinkPhase::Discovery
#[must_use]
pub const fn discovery_opcodes() -> &'static [u8] {
    &[
        opcode::DEV_ADDRESS_OPP,
        opcode::DEV_REQUEST_ADDR,
        opcode::DEV_ASSIGN_ADDR,
    ]
}

/// **Opcodes this system will never emit**, asserted by a scan over every frame
/// the encoder can produce.
///
/// Each is denied by the absence of a [`crate::dtv::SteamOp`] variant; this list
/// is the test's expectation, not the enforcement.
///
/// The thirteen enumerated in `CORRECTIONS.md` item 8 are marked ★. The rest
/// come from `STEAM-ADAPTER.md` § 3's stronger rule — "everything not in the
/// allowlist" — and are included because a superset makes the scan strictly
/// harder to pass, never easier.
///
/// | Byte | Command | Why denied |
/// | --- | --- | --- |
/// | `0x03` ★ | `NETWORK_RESET` | wipes every address on the bus |
/// | `0x13` ★ | `SET_EXT_KEY` | writes an authentication key |
/// | `0x18` ★ | `CHANGE_BAUD` | strands the link at a rate this master does not open |
/// | `0x20` ★ | `SET_FILE_TRANSFER` | file transfer into an appliance that heats a room |
/// | `0x21` ★ | `FLUSH_MD5` | same session |
/// | `0x22` ★ | `FILE_COMPLETE` | same session |
/// | `0x32` | `SETTINGS_UPDATE` | undocumented payload, either direction |
/// | `0x33` | `GET_DEV_PARAM` | arbitrary parameter read, no documented index space |
/// | `0x38` | `GET_FIRMWARE_VERSION` | not needed; not in the captured frame set |
/// | `0x39` ★ | `FIRMWARE_UPDATE` | leaves the generator unable to run |
/// | `0x40` | `ID_DEV` | not needed; not in the captured frame set |
/// | `0x70` ★ | `WRITE_DATA` | arbitrary write |
/// | `0x71` ★ | `READ_DATA` | arbitrary read |
/// | `0x72` | `GET_DATA_BUFFER` | device-to-master half of the same pair |
/// | `0x74` ★ | `WRITE_LARGE_DATA` | arbitrary write, 2000 ms timeout |
/// | `0x80` ★ | `REBOOT` | reboots the generator mid-session |
/// | `0x90` | `GET_TRACK_STR` | not needed; not in the captured frame set |
/// | `0x91` ★ | `GET_DIR_ENTRIES` | filesystem enumeration |
/// | `0xA1` ★ | `ACTIVATE_BOOT` | leaves the generator in its bootloader |
///
/// **Power clean is not on this list, and could not be.** `0xCC` is not an
/// opcode — it is a value of the operation-state byte inside the payload of
/// `SET_DEV_PARAM`, which *is* allowlisted. Omitting a command variant does not
/// deny it. That denial lives in [`SteamOpState`], which has no `PowerClean`
/// variant. `CORRECTIONS.md` item 1.
///
/// [`SteamOpState`]: crate::dtv::SteamOpState
#[must_use]
pub const fn denied_opcodes() -> &'static [u8] {
    &[
        opcode::NETWORK_RESET,
        opcode::SET_EXT_KEY,
        opcode::CHANGE_BAUD,
        opcode::SET_FILE_TRANSFER,
        opcode::FLUSH_MD5,
        opcode::FILE_COMPLETE,
        opcode::SETTINGS_UPDATE,
        opcode::GET_DEV_PARAM,
        opcode::GET_FIRMWARE_VERSION,
        opcode::FIRMWARE_UPDATE,
        opcode::ID_DEV,
        opcode::WRITE_DATA,
        opcode::READ_DATA,
        opcode::GET_DATA_BUFFER,
        opcode::WRITE_LARGE_DATA,
        opcode::REBOOT,
        opcode::GET_TRACK_STR,
        opcode::GET_DIR_ENTRIES,
        opcode::ACTIVATE_BOOT,
    ]
}

/// Which side of the bus an opcode can come from. `ADDR-06`.
///
/// This is the dispatch rule during discovery, where `DEST` and `SRC` are both
/// `0x00` in both directions and address-based routing is impossible: `0x06` can
/// only arrive from a device, `0x05` and `0x07` can only be sent by the master.
///
/// [`DtvDirection::Indeterminate`] for `SETTINGS_UPDATE`, which the source marks
/// "either", and for every opcode no source lists.
#[must_use]
pub const fn direction_of(cmd: u8) -> DtvDirection {
    match cmd {
        opcode::NETWORK_RESET
        | opcode::DEV_ADDRESS_OPP
        | opcode::DEV_ASSIGN_ADDR
        | opcode::SET_EXT_KEY
        | opcode::CHANGE_BAUD
        | opcode::SET_FILE_TRANSFER
        | opcode::FLUSH_MD5
        | opcode::FILE_COMPLETE
        | opcode::GET_DEV_STATUS
        | opcode::GET_DEV_PARAM
        | opcode::SET_DEV_PARAM
        | opcode::GET_FIRMWARE_VERSION
        | opcode::FIRMWARE_UPDATE
        | opcode::CLEAR_FAULT_FLAGS
        | opcode::ID_DEV
        | opcode::WRITE_DATA
        | opcode::READ_DATA
        | opcode::WRITE_LARGE_DATA
        | opcode::REBOOT
        | opcode::GET_TRACK_STR
        | opcode::GET_DIR_ENTRIES
        | opcode::ACTIVATE_BOOT => DtvDirection::MasterToDevice,
        opcode::DEV_REQUEST_ADDR
        | opcode::STATUS_UPDATE
        | opcode::DEV_ACK
        | opcode::DEV_NAK
        | opcode::ERROR
        | opcode::GET_DATA_BUFFER => DtvDirection::DeviceToMaster,
        // SETTINGS_UPDATE 0x32 is documented as "either", and everything else is
        // undocumented. Neither is a direction.
        _ => DtvDirection::Indeterminate,
    }
}

/// The documented name of an opcode, or `None` for a byte no source lists.
///
/// `None` rather than a plausible default — an unknown opcode in a capture must
/// read as unknown.
#[must_use]
pub const fn name_of(cmd: u8) -> Option<&'static str> {
    Some(match cmd {
        opcode::NETWORK_RESET => "NETWORK_RESET",
        opcode::DEV_ADDRESS_OPP => "DEV_ADDRESS_OPP",
        opcode::DEV_REQUEST_ADDR => "DEV_REQUEST_ADDR",
        opcode::DEV_ASSIGN_ADDR => "DEV_ASSIGN_ADDR",
        opcode::SET_EXT_KEY => "SET_EXT_KEY",
        opcode::CHANGE_BAUD => "CHANGE_BAUD",
        opcode::SET_FILE_TRANSFER => "SET_FILE_TRANSFER",
        opcode::FLUSH_MD5 => "FLUSH_MD5",
        opcode::FILE_COMPLETE => "FILE_COMPLETE",
        opcode::GET_DEV_STATUS => "GET_DEV_STATUS",
        opcode::STATUS_UPDATE => "STATUS_UPDATE",
        opcode::SETTINGS_UPDATE => "SETTINGS_UPDATE",
        opcode::GET_DEV_PARAM => "GET_DEV_PARAM",
        opcode::SET_DEV_PARAM => "SET_DEV_PARAM",
        opcode::DEV_ACK => "DEV_ACK",
        opcode::DEV_NAK => "DEV_NAK",
        opcode::ERROR => "ERROR",
        opcode::GET_FIRMWARE_VERSION => "GET_FIRMWARE_VERSION",
        opcode::FIRMWARE_UPDATE => "FIRMWARE_UPDATE",
        opcode::CLEAR_FAULT_FLAGS => "CLEAR_FAULT_FLAGS",
        opcode::ID_DEV => "ID_DEV",
        opcode::WRITE_DATA => "WRITE_DATA",
        opcode::READ_DATA => "READ_DATA",
        opcode::GET_DATA_BUFFER => "GET_DATA_BUFFER",
        opcode::WRITE_LARGE_DATA => "WRITE_LARGE_DATA",
        opcode::REBOOT => "REBOOT",
        opcode::GET_TRACK_STR => "GET_TRACK_STR",
        opcode::GET_DIR_ENTRIES => "GET_DIR_ENTRIES",
        opcode::ACTIVATE_BOOT => "ACTIVATE_BOOT",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CORRECTIONS.md` item 8, spelled out literally so the enumeration is a
    /// diff rather than a claim.
    #[test]
    fn corrections_item_8_opcodes_are_all_denied() {
        let item_8: [u8; 13] = [
            0x03, // NETWORK_RESET
            0x13, // SET_EXT_KEY
            0x18, // CHANGE_BAUD
            0x20, // SET_FILE_TRANSFER
            0x21, // FLUSH_MD5
            0x22, // FILE_COMPLETE
            0x39, // FIRMWARE_UPDATE
            0x70, // WRITE_DATA
            0x71, // READ_DATA
            0x74, // WRITE_LARGE_DATA
            0x80, // REBOOT
            0x91, // GET_DIR_ENTRIES
            0xA1, // ACTIVATE_BOOT
        ];
        for b in item_8 {
            assert!(
                denied_opcodes().contains(&b),
                "0x{b:02X} is missing from denied_opcodes()"
            );
            assert!(name_of(b).is_some(), "0x{b:02X} has no documented name");
        }
        // The list here is the stronger STEAM-ADAPTER 3 rule: every opcode not
        // in the allowlist.
        assert_eq!(denied_opcodes().len(), 19);
    }

    /// The allowlist and the denied set do not intersect, and neither does the
    /// discovery set.
    #[test]
    fn the_three_opcode_sets_are_disjoint() {
        for a in allowlisted_opcodes() {
            assert!(!denied_opcodes().contains(a), "0x{a:02X}");
            assert!(!discovery_opcodes().contains(a), "0x{a:02X}");
        }
        for d in discovery_opcodes() {
            assert!(!denied_opcodes().contains(d), "0x{d:02X}");
        }
        assert_eq!(allowlisted_opcodes().len(), 7);
        assert_eq!(discovery_opcodes().len(), 3);
    }

    /// Every documented opcode is in exactly one of the three sets. A new
    /// opcode added to `opcode` without a decision breaks this.
    #[test]
    fn every_documented_opcode_is_classified() {
        let mut documented = 0;
        for b in 0u8..=255 {
            let Some(_) = name_of(b) else { continue };
            documented += 1;
            let n = usize::from(allowlisted_opcodes().contains(&b))
                + usize::from(discovery_opcodes().contains(&b))
                + usize::from(denied_opcodes().contains(&b));
            assert_eq!(n, 1, "0x{b:02X} is in {n} sets, not 1");
        }
        assert_eq!(documented, 7 + 3 + 19);
    }

    /// `ADDR-06`. The discovery dispatch rule, as values.
    #[test]
    fn discovery_opcodes_carry_their_own_direction() {
        assert_eq!(
            direction_of(opcode::DEV_ADDRESS_OPP),
            DtvDirection::MasterToDevice
        );
        assert_eq!(
            direction_of(opcode::DEV_ASSIGN_ADDR),
            DtvDirection::MasterToDevice
        );
        assert_eq!(
            direction_of(opcode::DEV_REQUEST_ADDR),
            DtvDirection::DeviceToMaster
        );
        // Which is what makes the DEST=0x00 SRC=0x00 handshake routable at all:
        // both frames carry identical addresses in both directions.
        assert_ne!(
            direction_of(opcode::DEV_REQUEST_ADDR),
            direction_of(opcode::DEV_ASSIGN_ADDR)
        );
    }

    #[test]
    fn the_status_reply_candidates_are_all_device_to_master() {
        for b in [
            opcode::STATUS_UPDATE,
            opcode::DEV_ACK,
            opcode::DEV_NAK,
            opcode::ERROR,
        ] {
            assert_eq!(direction_of(b), DtvDirection::DeviceToMaster, "0x{b:02X}");
        }
        // GET_DEV_STATUS is the exception: the sources disagree about whether
        // the reply echoes 0x30, and the master certainly sends 0x30, so it is
        // master-to-device here. STEAM-04 handles the reply ambiguity in the
        // status decoder rather than in this table.
        assert_eq!(
            direction_of(opcode::GET_DEV_STATUS),
            DtvDirection::MasterToDevice
        );
    }

    #[test]
    fn settings_update_is_the_one_documented_either_direction_opcode() {
        assert_eq!(
            direction_of(opcode::SETTINGS_UPDATE),
            DtvDirection::Indeterminate
        );
        // And an undocumented byte is indeterminate too, not defaulted.
        assert_eq!(direction_of(0x99), DtvDirection::Indeterminate);
        assert_eq!(name_of(0x99), None);
    }
}
