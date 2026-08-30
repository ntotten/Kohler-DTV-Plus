//! Addressing: two byte-wide namespaces that must never be joined.
//!
//! Tier `[C]`, from `research/xagon0/docs/protocols/dtv-plus-protocol.md`
//! § Addressing and § Device IDs.
//!
//! # The mistake this module exists to make unspellable
//!
//! `0x05` is the steam generator's **device ID**. It is carried in the payload
//! of `DEV_REQUEST_ADDR` (`0x06`) while the device is still unaddressed. It is
//! **not** a bus address, and the master never puts it in a `DEST` field.
//!
//! The source conflates the two in two separate places. Its Example 1 addresses
//! "Get Steam Generator Status" to `DEST = 0x05`, labelled "steam", and
//! `steam-generator.md` § Communication writes "Device `0x05`" in both the
//! request and the response layouts. Its own discovery Example 3 is internally
//! consistent and contradicts both: device ID `0x05` in `DEV_REQUEST_ADDR`,
//! address `0x03` in `DEV_ASSIGN_ADDR`.
//!
//! `STEAM-ADAPTER.md` § 10.3 predicts this exact mistake, so [`DeviceId`] and
//! [`DevAddr`] are distinct types with **no conversion in either direction** —
//! no `From`, no `TryFrom`, no shared trait. `CORRECTIONS.md` item 2,
//! `ADDR-05`.
//!
//! The two namespaces genuinely overlap, which is why the split cannot be
//! replaced by a range check: device IDs `0x03` (rain panel), `0x05` (steam),
//! `0x06` (SMM) and `0x07` (amplifier-alt) all sit inside the assignable address
//! range, and device IDs `0x30`, `0x31` and `0x40` collide numerically with the
//! opcodes `GET_DEV_STATUS`, `STATUS_UPDATE` and `ID_DEV`. **Never infer a
//! field's meaning from its value.**

use core::fmt;

/// The master controller's own address, and simultaneously the address every
/// device ships with. `ADDR-01`.
///
/// One byte, two meanings, which is why discovery routes on opcode rather than
/// on address. `ADDR-06`.
pub const MASTER: u8 = 0x00;

/// The unassigned-device address. The same byte as [`MASTER`], deliberately
/// named twice so a call site says which meaning it intends.
pub const UNASSIGNED: u8 = 0x00;

/// All devices on the port. Legal only as a `DEST`, and only for
/// `DEV_ADDRESS_OPP`. `ADDR-04`.
pub const BROADCAST: u8 = 0xFF;

/// Lowest assignable device address. `ADDR-02`.
pub const DEV_ADDR_MIN: u8 = 0x03;
/// Highest assignable device address. `ADDR-02`.
pub const DEV_ADDR_MAX: u8 = 0x07;

/// A bus address the master has **assigned**, restricted to `0x03..=0x07`.
///
/// Not constructible from a [`DeviceId`]. The value in one of these came from a
/// `DEV_ASSIGN_ADDR` the master itself sent, and nowhere else.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DevAddr(u8);

impl DevAddr {
    /// Every address the master may allocate, in scan order.
    pub const ALL: [Self; 5] = [Self(0x03), Self(0x04), Self(0x05), Self(0x06), Self(0x07)];

    /// The address the reference topology expects: one adapter alone on its own
    /// converter takes the first slot. Any other assigned address is a topology
    /// change and requires re-discovery rather than adaptation. `ADDR-07`.
    pub const REFERENCE: Self = Self(0x03);

    /// Rejects `0x00` (master and unassigned), the reserved ranges `0x01..=0x02`
    /// and `0x08..=0xFE`, and the broadcast address. `ADDR-03`.
    pub fn new(v: u8) -> Result<Self, DtvAddrError> {
        if !(DEV_ADDR_MIN..=DEV_ADDR_MAX).contains(&v) {
            return Err(DtvAddrError::OutOfRange { requested: v });
        }
        Ok(Self(v))
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl fmt::Debug for DevAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DevAddr(0x{:02X})", self.0)
    }
}

impl fmt::Display for DevAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02X}", self.0)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum DtvAddrError {
    #[error("device address 0x{requested:02X} is outside the assignable range 0x03..=0x07")]
    OutOfRange { requested: u8 },
}

/// A device **type** identifier, carried in the payload of `DEV_REQUEST_ADDR`.
///
/// Any byte is a legal device ID — the namespace is not range-checked, and
/// checking one against the address ranges is exactly the error this type
/// exists to prevent. Unknown IDs are carried verbatim rather than rejected,
/// because a capture of the factory bus is expected to contain devices this
/// project has never seen.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(u8);

impl DeviceId {
    /// `0x00` — the DTV+ central controller.
    pub const CONTROLLER: Self = Self(0x00);
    /// `0x03` — rain showerhead module.
    pub const RAIN_PANEL: Self = Self(0x03);
    /// `0x05` — the steam generator. **The one this project talks to**, and the
    /// byte `STEAM-ADAPTER.md` § 10.3 warns is misused as an address.
    pub const STEAM_GENERATOR: Self = Self(0x05);
    /// `0x06` — "Steam Management Module". `[?]` No Kohler product by that name
    /// appears in any document found.
    pub const STEAM_MANAGEMENT_MODULE: Self = Self(0x06);
    /// `0x07` — amplifier, alternate ID.
    pub const AMPLIFIER_ALT: Self = Self(0x07);
    /// `0x08` — chromatherapy light bridge.
    pub const LIGHT_BRIDGE: Self = Self(0x08);
    /// `0x09` — factory test equipment.
    pub const TEST_FIXTURE: Self = Self(0x09);
    /// `0x30` — first-generation touchscreen. Numerically `GET_DEV_STATUS`.
    pub const UI_V1: Self = Self(0x30);
    /// `0x31` — second-generation touchscreen. Numerically `STATUS_UPDATE`.
    pub const UI_V2: Self = Self(0x31);
    /// `0x40` — audio amplifier. Numerically `ID_DEV`.
    pub const AMPLIFIER: Self = Self(0x40);
    /// `0xF0` — a device sitting in its bootloader.
    pub const BOOTLOADER: Self = Self(0xF0);

    /// Every documented identifier, for decoding a capture.
    pub const DOCUMENTED: [Self; 11] = [
        Self::CONTROLLER,
        Self::RAIN_PANEL,
        Self::STEAM_GENERATOR,
        Self::STEAM_MANAGEMENT_MODULE,
        Self::AMPLIFIER_ALT,
        Self::LIGHT_BRIDGE,
        Self::TEST_FIXTURE,
        Self::UI_V1,
        Self::UI_V2,
        Self::AMPLIFIER,
        Self::BOOTLOADER,
    ];

    #[must_use]
    pub const fn new(v: u8) -> Self {
        Self(v)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// The documented name, or `None` for an identifier no source lists.
    ///
    /// `None` rather than a default, so an unknown device in a capture reads as
    /// unknown instead of as something plausible. `RESP-03`'s rule, applied
    /// here.
    #[must_use]
    pub const fn name(self) -> Option<&'static str> {
        Some(match self.0 {
            0x00 => "controller",
            0x03 => "rain panel",
            0x05 => "steam generator",
            0x06 => "steam management module",
            0x07 => "amplifier (alt)",
            0x08 => "light bridge",
            0x09 => "test fixture",
            0x30 => "UI v1",
            0x31 => "UI v2",
            0x40 => "amplifier",
            0xF0 => "bootloader",
            _ => return None,
        })
    }
}

impl fmt::Debug for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(n) => write!(f, "DeviceId(0x{:02X} {n})", self.0),
            None => write!(f, "DeviceId(0x{:02X})", self.0),
        }
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:02X}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_addr_accepts_only_the_assignable_range() {
        for v in 0u8..=255 {
            assert_eq!(
                DevAddr::new(v).is_ok(),
                (0x03..=0x07).contains(&v),
                "byte 0x{v:02X}"
            );
        }
        assert!(DevAddr::new(MASTER).is_err());
        assert!(DevAddr::new(UNASSIGNED).is_err());
        assert!(DevAddr::new(BROADCAST).is_err());
        // ADDR-03: the reserved ranges, named.
        for v in [0x01u8, 0x02, 0x08, 0x7F, 0xFE] {
            assert!(DevAddr::new(v).is_err(), "0x{v:02X}");
        }
        assert_eq!(DevAddr::ALL.len(), 5);
        assert_eq!(DevAddr::REFERENCE.get(), 0x03);
    }

    /// `CORRECTIONS.md` item 2 / `ADDR-05`. The two namespaces share a byte
    /// width and nothing else.
    ///
    /// The absence of a conversion cannot be asserted from inside the crate —
    /// there is nothing to call. What this test pins is the shape that makes the
    /// absence meaningful: the steam device ID is `0x05`, the address the
    /// reference topology assigns is `0x03`, and the two are different types
    /// holding different numbers.
    #[test]
    fn req_steam_adapter_addr_05_the_steam_device_id_is_not_the_steam_bus_address() {
        assert_eq!(DeviceId::STEAM_GENERATOR.get(), 0x05);
        assert_eq!(DevAddr::REFERENCE.get(), 0x03);
        assert_ne!(DeviceId::STEAM_GENERATOR.get(), DevAddr::REFERENCE.get());

        // 0x05 *is* a legal assigned address — five devices can share a port —
        // so the denial cannot be a range check. `88 05 00 30 CB 55` is
        // reachable only if discovery assigned 0x05 to something, never from
        // the device ID. The reference topology enrols one device and expects
        // 0x03; anything else is a topology change. ADDR-07.
        assert!(DevAddr::new(DeviceId::STEAM_GENERATOR.get()).is_ok());
    }

    #[test]
    fn documented_device_ids_are_named_and_the_rest_are_not() {
        for id in DeviceId::DOCUMENTED {
            assert!(id.name().is_some(), "{id:?}");
        }
        let documented: Vec<u8> = DeviceId::DOCUMENTED.iter().map(|d| d.get()).collect();
        for v in 0u8..=255 {
            assert_eq!(
                DeviceId::new(v).name().is_some(),
                documented.contains(&v),
                "0x{v:02X}"
            );
        }
    }

    /// The overlaps that make a value-based inference wrong, stated as values.
    #[test]
    fn the_two_namespaces_overlap_on_purpose() {
        // Device IDs inside the assignable address range.
        for id in [
            DeviceId::RAIN_PANEL,
            DeviceId::STEAM_GENERATOR,
            DeviceId::STEAM_MANAGEMENT_MODULE,
            DeviceId::AMPLIFIER_ALT,
        ] {
            assert!((DEV_ADDR_MIN..=DEV_ADDR_MAX).contains(&id.get()), "{id:?}");
        }
        // Device IDs equal to opcodes.
        assert_eq!(
            DeviceId::UI_V1.get(),
            super::super::command::opcode::GET_DEV_STATUS
        );
        assert_eq!(
            DeviceId::UI_V2.get(),
            super::super::command::opcode::STATUS_UPDATE
        );
        assert_eq!(
            DeviceId::AMPLIFIER.get(),
            super::super::command::opcode::ID_DEV
        );
    }

    #[test]
    fn the_reserved_bytes_are_the_documented_ones() {
        assert_eq!(MASTER, 0x00);
        assert_eq!(UNASSIGNED, MASTER);
        assert_eq!(BROADCAST, 0xFF);
        assert_eq!((DEV_ADDR_MIN, DEV_ADDR_MAX), (0x03, 0x07));
    }
}
