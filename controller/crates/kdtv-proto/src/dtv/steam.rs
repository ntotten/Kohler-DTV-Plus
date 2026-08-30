//! The steam device profile: operation state, status payload, error bitmask.
//!
//! Tier `[C]`, from `research/xagon0/docs/devices/steam-generator.md`, narrowed
//! by `controller/docs/STEAM-ADAPTER.md` § 7 and § 10 and
//! `HARDWARE.md` § 12.
//!
//! # Where the hazard actually is
//!
//! Not in the opcode set. `SET_DEV_PARAM` (`0x34`) is allowlisted, and the
//! 45-minute unattended power-clean cycle is started by writing `0xCC` into its
//! **operation-state byte**. Omitting a command variant does not deny a payload
//! value. `CORRECTIONS.md` item 1.
//!
//! So the denial is one enum: [`SteamOpState`] has `Off` and `On` and nothing
//! else, and it is the only type the encoder accepts in that position. `0xCC` is
//! unconstructible there — not refused at runtime, unspellable.
//!
//! Decoding is a separate type. [`SteamStateByte`] *does* carry `PowerClean`,
//! because observing a cycle we did not start is exactly what `STEAM-12`
//! requires: lock out every steam command, report `PC_ACTIVE`, and do nothing
//! that could interrupt it — an interrupted power clean must be restarted from
//! the beginning.
//!
//! # Three vocabularies for one device
//!
//! The sources describe the generator's state three incompatible ways and this
//! module keeps them apart:
//!
//! | Vocabulary | Type | Values |
//! | --- | --- | --- |
//! | The wire operation-state byte | [`SteamStateByte`] | `0x00`, `0xFF`, `0xCC`, invalid |
//! | The device's internal state machine | *not modelled* | INIT, OFF, ON, PREHEAT, STEAMING, COOLDOWN |
//! | The UI status code | [`SteamUiStatus`] | 0–8 |
//!
//! The middle one has six states and three wire encodings, so it cannot be
//! decoded from a single byte and is not guessed at here. A byte outside
//! `{0x00, 0xFF, 0xCC}` decodes to [`SteamStateByte::Invalid`] and surfaces UI
//! status 8, rather than being read as PREHEAT or COOLDOWN.

use crate::dtv::frame::MAX_PAYLOAD;
use core::fmt;
use kdtv_units::Fx2;

/// The operation state the **encoder** may write. `CORRECTIONS.md` item 1.
///
/// Two variants, and there is no third. `STEAM_POWER_CLEAN` (`0xCC`) has no
/// variant here, so no program in this workspace can put it in the
/// operation-state position of a `SET_DEV_PARAM` payload. `STEAM-11`.
///
/// A byte-level scan over every frame the encoder can produce asserts the
/// property mechanically — see `no_encoded_frame_carries_power_clean` in
/// [`crate::dtv::encode`].
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum SteamOpState {
    /// `0x00` — `STEAM_OFF`. The default, because a value that was never set
    /// must not run a heater.
    #[default]
    Off,
    /// `0xFF` — `STEAM_ON`.
    On,
}

impl SteamOpState {
    /// Both reachable states.
    pub const ALL: [Self; 2] = [Self::Off, Self::On];

    /// `STEAM_POWER_CLEAN`, for the decoder and for the scan tests to compare
    /// against. **This constant is a value to recognise and refuse, never one
    /// to write** — nothing in [`SteamOpState`] produces it.
    pub const POWER_CLEAN_BYTE: u8 = 0xCC;

    /// An explicit `match`, not a `#[repr(u8)]` discriminant cast — `as`
    /// conversions are denied workspace-wide.
    #[must_use]
    pub const fn wire(self) -> u8 {
        match self {
            Self::Off => 0x00,
            Self::On => 0xFF,
        }
    }
}

impl fmt::Display for SteamOpState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Off => "off",
            Self::On => "on",
        })
    }
}

/// The operation state as **decoded** from a status frame. `STEAM-10`.
///
/// A superset of [`SteamOpState`]: the generator can be in a state this master
/// would never command, and refusing to represent that would blind the observer
/// that `STEAM-12` depends on.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum SteamStateByte {
    /// `0x00`.
    Off,
    /// `0xFF`.
    On,
    /// `0xCC`. A cycle this master did not and cannot start. Lock out every
    /// steam command and do not interrupt it. `STEAM-12`.
    PowerClean,
    /// Anything else, carried verbatim. The device-side machine has six states
    /// and three encodings, so an unrecognised byte is not evidence of PREHEAT
    /// or COOLDOWN — it is evidence of something unmodelled.
    Invalid(u8),
}

impl SteamStateByte {
    #[must_use]
    pub const fn decode(b: u8) -> Self {
        match b {
            0x00 => Self::Off,
            0xFF => Self::On,
            SteamOpState::POWER_CLEAN_BYTE => Self::PowerClean,
            other => Self::Invalid(other),
        }
    }

    /// The byte this state came from.
    #[must_use]
    pub const fn raw(self) -> u8 {
        match self {
            Self::Off => 0x00,
            Self::On => 0xFF,
            Self::PowerClean => SteamOpState::POWER_CLEAN_BYTE,
            Self::Invalid(b) => b,
        }
    }

    /// True while the generator is doing something that makes heat.
    #[must_use]
    pub const fn is_producing(self) -> bool {
        matches!(self, Self::On | Self::PowerClean)
    }
}

bitflags::bitflags! {
    /// The error-flags byte from a status response. `STEAM-13`.
    ///
    /// Four bits are documented and four are not. Unknown bits are **retained**,
    /// not masked off: `STEAM-14` requires them preserved, reported verbatim,
    /// and treated as a fault, so an error byte of `0x80` produces a fault
    /// rather than a clean status. That is why decoding uses
    /// `from_bits_retain`.
    ///
    /// Bits are independent and can be set together. `CORRECTIONS.md` item 9.
    #[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
    pub struct SteamErrorFlags: u8 {
        /// Bit 2. Temperature sensor failure — the actual-temperature field
        /// stops being trustworthy. `STEAM-16`.
        const THERMISTOR = 0x04;
        /// Bit 3. The generator's own serial link is lost. Distinct from *our*
        /// link to the adapter being lost.
        const COMMUNICATION = 0x08;
        /// Bit 5. Steam temperature past the safety limit. `STEAM-15`.
        const OVERTEMPERATURE = 0x20;
        /// Bit 6. A hardware safety interlock tripped. `STEAM-15`.
        const SAFETY_CIRCUIT = 0x40;
    }
}

impl SteamErrorFlags {
    /// Bits 0, 1, 4 and 7. No source documents them. Preserved and reported,
    /// never interpreted. `STEAM-14`.
    pub const RESERVED_BITS: u8 = 0x01 | 0x02 | 0x10 | 0x80;

    /// The bits that require `STEAM_OFF` immediately and a latched fault:
    /// overtemperature, safety circuit, and thermistor. `STEAM-15` /
    /// `STEAM-16`.
    pub const IMMEDIATE_OFF: Self = Self::OVERTEMPERATURE
        .union(Self::SAFETY_CIRCUIT)
        .union(Self::THERMISTOR);

    /// Decodes the byte, keeping undocumented bits.
    #[must_use]
    pub const fn decode(b: u8) -> Self {
        Self::from_bits_retain(b)
    }

    /// **Any** non-zero error byte is a fault, including one carrying only
    /// undocumented bits. `STEAM-14`.
    #[must_use]
    pub const fn is_fault(self) -> bool {
        self.bits() != 0
    }

    /// The undocumented bits that are set, if any.
    #[must_use]
    pub const fn reserved_bits_set(self) -> u8 {
        self.bits() & Self::RESERVED_BITS
    }

    /// True when the fault requires commanding `STEAM_OFF` on this poll and
    /// latching. `STEAM-15` / `STEAM-16`.
    #[must_use]
    pub const fn requires_immediate_off(self) -> bool {
        self.bits() & Self::IMMEDIATE_OFF.bits() != 0
    }
}

/// The status codes the user interface speaks. `STEAM-17`.
///
/// Nine values and no tenth: the enum is the enforcement for "never invent a
/// code outside 0..8".
///
/// Four of them cannot be derived from one status frame and are set by the
/// engine from state a frame does not carry — [`SteamUiStatus::NotInstalled`]
/// from discovery, [`SteamUiStatus::PowerCleanWarning`] and
/// [`SteamUiStatus::PowerCleanRequired`] from the 600-minute cumulative-runtime
/// accumulator (`STEAM-22`), and [`SteamUiStatus::PurgeActive`] from something
/// with no wire encoding at all. That last one is the likely home of
/// `INVESTIGATIONS.md` I4: `PURGE_ACTIVE` has a status code and no operation
/// state to produce it.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SteamUiStatus {
    /// 0. No generator detected.
    NotInstalled,
    /// 1. Present and idle.
    Off,
    /// 2. Producing steam.
    On,
    /// 3. Power clean running.
    PowerCleanActive,
    /// 4. Power clean recommended soon.
    PowerCleanWarning,
    /// 5. Power clean overdue.
    PowerCleanRequired,
    /// 6. A fault; read the error bits.
    Error,
    /// 7. Post-session purge. `[?]` No wire encoding produces this.
    PurgeActive,
    /// 8. Unknown or corrupt.
    Invalid,
}

impl SteamUiStatus {
    /// All nine, in code order.
    pub const ALL: [Self; 9] = [
        Self::NotInstalled,
        Self::Off,
        Self::On,
        Self::PowerCleanActive,
        Self::PowerCleanWarning,
        Self::PowerCleanRequired,
        Self::Error,
        Self::PurgeActive,
        Self::Invalid,
    ];

    /// The numeric code, 0..=8.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::NotInstalled => 0,
            Self::Off => 1,
            Self::On => 2,
            Self::PowerCleanActive => 3,
            Self::PowerCleanWarning => 4,
            Self::PowerCleanRequired => 5,
            Self::Error => 6,
            Self::PurgeActive => 7,
            Self::Invalid => 8,
        }
    }
}

/// Wire layout of the status payload. `STEAM-02` / `STEAM-03`.
///
/// | Offset | Field |
/// | --- | --- |
/// | 0 | actual temperature, `Fx2` |
/// | 1 | desired temperature, `Fx2` |
/// | 2 | operation state |
/// | 3 | timer minutes |
/// | 4 | timer seconds |
/// | 5 | error flags |
pub const STATUS_PAYLOAD_LEN: usize = 6;

/// Offset of the operation-state byte inside a status payload.
pub const STATUS_STATE_OFFSET: usize = 2;

/// Wire layout of the `SET_DEV_PARAM` payload for steam. `STEAM-06`.
///
/// | Offset | Field |
/// | --- | --- |
/// | 0 | desired temperature, `Fx2` |
/// | 1 | operation state |
/// | 2 | timer duration, minutes |
///
/// **Unresolved `[?]`.** `dtv-plus-protocol.md` § Set Device Parameter shows a
/// generic `[param_id][value]` pair instead, and its Example 2 uses
/// `PARAM_ID = 0x01`. These cannot both be the on-wire shape. The device-
/// specific source wins here because it matches the `DT_W_Steam*` variable set,
/// and the choice is a single named constant so a capture can move it. See
/// [`ParamCodec`].
pub const SET_PARAM_PAYLOAD_LEN: usize = 3;

/// Offset of the operation-state byte inside a `SET_DEV_PARAM` payload.
///
/// **This is the byte `0xCC` must never occupy.** `CORRECTIONS.md` item 1.
pub const SET_PARAM_STATE_OFFSET: usize = 1;

/// Which `SET_DEV_PARAM` payload shape this link speaks. `STEAM-07`.
///
/// The two sources describe incompatible shapes and neither has been seen on
/// this installation's wire:
///
/// - `steam-generator.md` § `SET_DEV_PARAM` — a fixed three-field block with no
///   parameter id.
/// - `dtv-plus-protocol.md` § Set Device Parameter — a generic
///   `[param_id][value]` pair, with `PARAM_ID = 0x01` in its Example 2.
///
/// `STEAM-07` asks for both behind a selector. **Only one variant exists here,
/// and that is a deliberate stop.** The generic shape needs a parameter-id space
/// for the steam device — an id for the setpoint, one for the operation state,
/// one for the duration — and no source states a single one of them. Adding a
/// `Generic` variant would mean inventing three numbers and shipping them as if
/// they were documented, which is exactly the unmarked inference `AGENT.md`
/// rule 4 forbids. The selector exists so the second variant is a one-line
/// addition the day a capture supplies those ids; the encoder already routes
/// through it.
///
/// The generic shape also cannot express what the three-field form does: that
/// form writes setpoint, state and duration **atomically**, so a caller changing
/// only the state must supply the current setpoint and duration too. A
/// parameter-at-a-time codec would put a partially-updated generator on the bus
/// between writes.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[non_exhaustive]
pub enum ParamCodec {
    /// The three-field steam block: desired temperature, operation state, timer
    /// minutes. The default, because it is the device-specific source and it
    /// matches `DT_W_SteamDesiredTemperature` / `DT_W_SteamOperationState` /
    /// `DT_W_SteamDuration`.
    #[default]
    SteamBlock,
}

/// A decoded steam status response.
///
/// Every field is what the wire said. Nothing here is interpreted beyond naming
/// the bits, and no field authorises anything on its own.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct SteamStatus {
    /// Actual temperature. Not to be trusted when
    /// [`SteamErrorFlags::THERMISTOR`] is set. `STEAM-16`.
    pub actual: Fx2,
    /// The setpoint the generator believes it has.
    pub desired: Fx2,
    /// The operation state, including states this master cannot command.
    pub state: SteamStateByte,
    /// Timer minutes remaining.
    pub timer_minutes: u8,
    /// Timer seconds remaining.
    ///
    /// **Not monotonic.** The device-side ticker persists across pause and
    /// resume, so a later reading can be larger than an earlier one. `STEAM-21`.
    pub timer_seconds: u8,
    /// The error bitmask, undocumented bits retained.
    pub errors: SteamErrorFlags,
}

/// Why a status payload could not be decoded.
#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum SteamStatusError {
    /// `STEAM-03`. Field widths are inference `[I]` — no source states them —
    /// so a payload of a different length is rejected rather than read
    /// positionally. Reading five bytes as six would slide the error flags into
    /// the seconds field.
    #[error("steam status payload is {found} bytes, expected {expected}")]
    WrongLength { found: usize, expected: usize },
}

impl SteamStatus {
    /// Decodes the six-byte payload. `STEAM-02`.
    pub fn decode(payload: &[u8]) -> Result<Self, SteamStatusError> {
        let [actual, desired, state, minutes, seconds, errors] = payload else {
            return Err(SteamStatusError::WrongLength {
                found: payload.len(),
                expected: STATUS_PAYLOAD_LEN,
            });
        };
        Ok(Self {
            actual: Fx2::from_raw(*actual),
            desired: Fx2::from_raw(*desired),
            state: SteamStateByte::decode(*state),
            timer_minutes: *minutes,
            timer_seconds: *seconds,
            errors: SteamErrorFlags::decode(*errors),
        })
    }

    /// The six payload bytes, in wire order.
    ///
    /// For fixtures and for the emulator's device model. This is a status
    /// payload, which only a device sends; it is not a route to a transmittable
    /// frame, and [`crate::dtv::SteamEncoder`] has no operation that carries it.
    #[must_use]
    pub const fn payload(&self) -> [u8; STATUS_PAYLOAD_LEN] {
        [
            self.actual.raw(),
            self.desired.raw(),
            self.state.raw(),
            self.timer_minutes,
            self.timer_seconds,
            self.errors.bits(),
        ]
    }

    /// The UI status code this frame implies on its own. `STEAM-17`.
    ///
    /// Only ever `OFF`, `ON`, `PC_ACTIVE`, `ERROR` or `INVALID` — the other four
    /// codes need state a single frame does not carry, and are the engine's to
    /// set. A power clean outranks an error flag because `STEAM-12` says do
    /// nothing that could interrupt the cycle, and an unknown state outranks
    /// both because it is not evidence of anything.
    #[must_use]
    pub const fn ui_status(&self) -> SteamUiStatus {
        match self.state {
            SteamStateByte::PowerClean => SteamUiStatus::PowerCleanActive,
            SteamStateByte::Invalid(_) => SteamUiStatus::Invalid,
            SteamStateByte::On => {
                if self.errors.is_fault() {
                    SteamUiStatus::Error
                } else {
                    SteamUiStatus::On
                }
            }
            SteamStateByte::Off => {
                if self.errors.is_fault() {
                    SteamUiStatus::Error
                } else {
                    SteamUiStatus::Off
                }
            }
        }
    }

    /// True when the reported setpoint is inside the operational range this
    /// master enforces, 90–125 °F.
    ///
    /// A desired temperature between `Fx2` 150 (75 °F, the firmware floor) and
    /// 179 is legal to decode and must raise an "out-of-policy setpoint
    /// observed" event, because it means something other than this master wrote
    /// it. `STEAM-ADAPTER.md` § 10.8.
    #[must_use]
    pub fn setpoint_is_in_policy(&self) -> bool {
        kdtv_units::SteamSetpoint::try_new(self.desired).is_ok()
    }
}

/// `MIN_STEAM_SETPOINT` as the firmware documents it: `Cx2` 48, which is 24 °C
/// and converts to `Fx2` 150 — 75.0 °F, not the 75.2 °F the true arithmetic
/// gives, because the device's conversion truncates.
///
/// **Decoder tolerance only.** The operational floor is
/// [`kdtv_units::SteamSetpoint::FLOOR`] at `Fx2` 180 and this value is never
/// reachable as a setpoint. `STEAM-ADAPTER.md` § 10.8.
pub const FIRMWARE_MIN_SETPOINT_FX2: u8 = 150;

const _: () = assert!(STATUS_PAYLOAD_LEN <= MAX_PAYLOAD);
const _: () = assert!(SET_PARAM_PAYLOAD_LEN <= MAX_PAYLOAD);

#[cfg(test)]
mod tests {
    use super::*;
    use kdtv_units::SteamSetpoint;

    /// `CORRECTIONS.md` item 1, at the type level. Two variants, two bytes,
    /// and `0xCC` is not among them.
    #[test]
    fn req_controller_design_deny_08_the_encodable_operation_states_are_off_and_on_only() {
        assert_eq!(SteamOpState::ALL.len(), 2);
        assert_eq!(SteamOpState::Off.wire(), 0x00);
        assert_eq!(SteamOpState::On.wire(), 0xFF);
        for s in SteamOpState::ALL {
            assert_ne!(s.wire(), SteamOpState::POWER_CLEAN_BYTE);
        }
        // The default is off. A state that was never set must not run a heater.
        assert_eq!(SteamOpState::default(), SteamOpState::Off);
    }

    /// `STEAM-10`, exhaustively over all 256 bytes.
    #[test]
    fn every_operation_state_byte_decodes_to_exactly_one_state() {
        for b in 0u8..=255 {
            let s = SteamStateByte::decode(b);
            assert_eq!(s.raw(), b, "0x{b:02X} did not round-trip");
            match b {
                0x00 => assert_eq!(s, SteamStateByte::Off),
                0xFF => assert_eq!(s, SteamStateByte::On),
                0xCC => assert_eq!(s, SteamStateByte::PowerClean),
                other => assert_eq!(s, SteamStateByte::Invalid(other)),
            }
        }
        assert!(SteamStateByte::On.is_producing());
        assert!(SteamStateByte::PowerClean.is_producing());
        assert!(!SteamStateByte::Off.is_producing());
        assert!(!SteamStateByte::Invalid(0x42).is_producing());
    }

    /// `CORRECTIONS.md` item 9 / `STEAM-13`. The four documented bits, and
    /// combinations.
    #[test]
    fn the_error_bitmask_decodes_bit_by_bit() {
        assert_eq!(SteamErrorFlags::THERMISTOR.bits(), 0x04);
        assert_eq!(SteamErrorFlags::COMMUNICATION.bits(), 0x08);
        assert_eq!(SteamErrorFlags::OVERTEMPERATURE.bits(), 0x20);
        assert_eq!(SteamErrorFlags::SAFETY_CIRCUIT.bits(), 0x40);

        let both = SteamErrorFlags::decode(0x24);
        assert!(both.contains(SteamErrorFlags::THERMISTOR));
        assert!(both.contains(SteamErrorFlags::OVERTEMPERATURE));
        assert!(!both.contains(SteamErrorFlags::COMMUNICATION));
        assert!(!both.contains(SteamErrorFlags::SAFETY_CIRCUIT));

        // All four at once.
        let all = SteamErrorFlags::decode(0x6C);
        for bit in [
            SteamErrorFlags::THERMISTOR,
            SteamErrorFlags::COMMUNICATION,
            SteamErrorFlags::OVERTEMPERATURE,
            SteamErrorFlags::SAFETY_CIRCUIT,
        ] {
            assert!(all.contains(bit), "{bit:?}");
        }
        assert_eq!(all.reserved_bits_set(), 0);

        // Clean.
        assert!(!SteamErrorFlags::decode(0x00).is_fault());
        assert_eq!(SteamErrorFlags::default().bits(), 0);
    }

    /// `STEAM-14`. Undocumented bits survive decoding and still mean fault.
    #[test]
    fn req_steam_generator_steam_14_undocumented_error_bits_are_preserved_and_are_faults() {
        for b in [0x01u8, 0x02, 0x10, 0x80] {
            let f = SteamErrorFlags::decode(b);
            assert_eq!(f.bits(), b, "0x{b:02X} lost its bit");
            assert!(f.is_fault(), "0x{b:02X} decoded as clean");
            assert_eq!(f.reserved_bits_set(), b);
            // Undocumented is not the same as immediately dangerous; the fault
            // still latches, but the immediate-off rule names three bits.
            assert!(!f.requires_immediate_off(), "0x{b:02X}");
        }
        assert_eq!(SteamErrorFlags::RESERVED_BITS, 0x93);
        // Exhaustively: every non-zero byte is a fault.
        for b in 0u8..=255 {
            assert_eq!(SteamErrorFlags::decode(b).is_fault(), b != 0);
            assert_eq!(SteamErrorFlags::decode(b).bits(), b);
        }
    }

    /// `STEAM-15` / `STEAM-16`. The three bits that demand `STEAM_OFF` now.
    #[test]
    fn overtemperature_safety_and_thermistor_demand_an_immediate_off() {
        for b in [0x04u8, 0x20, 0x40] {
            assert!(
                SteamErrorFlags::decode(b).requires_immediate_off(),
                "{b:02X}"
            );
        }
        // A generator-side comms error is a fault but not one that changes what
        // this master does on this poll.
        assert!(SteamErrorFlags::decode(0x08).is_fault());
        assert!(!SteamErrorFlags::decode(0x08).requires_immediate_off());
    }

    /// The derived status fixture from `ARITHMETIC-NOTES.md`: device reporting
    /// 110 °F actual and desired, off, no timer, no errors.
    #[test]
    fn the_derived_status_payload_decodes_field_by_field() {
        let s = SteamStatus::decode(&[0xDC, 0xDC, 0x00, 0x00, 0x00, 0x00]).unwrap();
        assert_eq!(s.actual.raw(), 220);
        assert_eq!(s.desired.raw(), 220);
        assert_eq!(s.desired, SteamSetpoint::FACTORY_DEFAULT);
        assert_eq!(s.state, SteamStateByte::Off);
        assert_eq!((s.timer_minutes, s.timer_seconds), (0, 0));
        assert!(!s.errors.is_fault());
        assert_eq!(s.ui_status(), SteamUiStatus::Off);
        assert!(s.setpoint_is_in_policy());
        assert_eq!(s.payload(), [0xDC, 0xDC, 0x00, 0x00, 0x00, 0x00]);

        // Fx2 220 is 110.0 F. Read as Cx2 the same byte is 110 C — the hazard
        // HARDWARE.md section 12 exists for.
        assert!((s.desired.fahrenheit() - 110.0).abs() < f32::EPSILON);
    }

    /// A mid-session frame: 108 °F rising to 110 °F, on, nine minutes and
    /// thirty seconds left, overtemperature set.
    #[test]
    fn a_faulted_running_status_decodes_and_reports_error() {
        let s = SteamStatus::decode(&[0xD8, 0xDC, 0xFF, 0x09, 0x1E, 0x20]).unwrap();
        assert_eq!(s.state, SteamStateByte::On);
        assert_eq!((s.timer_minutes, s.timer_seconds), (9, 30));
        assert!(s.errors.contains(SteamErrorFlags::OVERTEMPERATURE));
        assert!(s.errors.requires_immediate_off());
        assert_eq!(s.ui_status(), SteamUiStatus::Error);
    }

    /// `STEAM-03`. Field widths are inference, so the length guard is the only
    /// thing standing between a short payload and a misread error byte.
    #[test]
    fn a_status_payload_of_the_wrong_length_is_rejected_not_read_positionally() {
        for len in 0usize..=8 {
            let payload = vec![0x00u8; len];
            let out = SteamStatus::decode(&payload);
            if len == STATUS_PAYLOAD_LEN {
                assert!(out.is_ok(), "{len} bytes");
            } else {
                assert_eq!(
                    out,
                    Err(SteamStatusError::WrongLength {
                        found: len,
                        expected: STATUS_PAYLOAD_LEN
                    })
                );
            }
        }
        // The concrete misread the guard prevents: five bytes read as six would
        // put the error flags where the seconds are.
        let five = [0xDC, 0xDC, 0xFF, 0x09, 0x20];
        assert!(SteamStatus::decode(&five).is_err());
    }

    /// `STEAM-12`. A power clean we did not start is observable, and it
    /// outranks an error flag in the UI mapping.
    #[test]
    fn an_observed_power_clean_reports_pc_active() {
        let s = SteamStatus::decode(&[0xE6, 0xE6, 0xCC, 0x2D, 0x00, 0x00]).unwrap();
        assert_eq!(s.state, SteamStateByte::PowerClean);
        assert_eq!(s.ui_status(), SteamUiStatus::PowerCleanActive);
        // Even with a fault set, do nothing that could interrupt the cycle.
        let faulted = SteamStatus::decode(&[0xE6, 0xE6, 0xCC, 0x2D, 0x00, 0x20]).unwrap();
        assert_eq!(faulted.ui_status(), SteamUiStatus::PowerCleanActive);
    }

    #[test]
    fn an_unmodelled_state_byte_reports_invalid_rather_than_a_guess() {
        // PREHEAT and COOLDOWN exist in the device's own state machine and have
        // no wire encoding. Whatever byte they might use, the answer is 8.
        for b in [0x01u8, 0x02, 0x42, 0xAA, 0xFE] {
            let s = SteamStatus::decode(&[0xDC, 0xDC, b, 0x00, 0x00, 0x00]).unwrap();
            assert_eq!(s.ui_status(), SteamUiStatus::Invalid, "0x{b:02X}");
        }
    }

    /// `STEAM-17`. Nine codes, 0..=8, and the enum is what makes a tenth
    /// unspellable.
    #[test]
    fn ui_status_codes_are_exactly_zero_through_eight() {
        let codes: Vec<u8> = SteamUiStatus::ALL.iter().map(|s| s.code()).collect();
        assert_eq!(codes, (0u8..=8).collect::<Vec<_>>());
    }

    /// `STEAM-ADAPTER.md` § 10.8. The firmware floor is decodable and is not a
    /// reachable setpoint.
    #[test]
    fn req_steam_adapter_limit_04_an_out_of_policy_setpoint_decodes_and_is_flagged() {
        let low = SteamStatus::decode(&[0x96, FIRMWARE_MIN_SETPOINT_FX2, 0x00, 0, 0, 0]).unwrap();
        assert_eq!(low.desired.raw(), 150);
        assert!(!low.setpoint_is_in_policy());
        // And the operational floor is where policy starts.
        let floor = SteamStatus::decode(&[0xB4, 0xB4, 0x00, 0, 0, 0]).unwrap();
        assert_eq!(floor.desired, SteamSetpoint::FLOOR);
        assert!(floor.setpoint_is_in_policy());
        // A half-degree setpoint is out of policy too: the documented UI cannot
        // produce one, so seeing one means something else wrote it.
        let odd = SteamStatus::decode(&[0xDD, 0xDD, 0x00, 0, 0, 0]).unwrap();
        assert!(!odd.setpoint_is_in_policy());
    }

    /// `STEAM-21`. The seconds field is not monotonic and nothing here assumes
    /// it is: two frames, the second reporting more time left than the first,
    /// both decode.
    #[test]
    fn the_timer_fields_are_carried_not_interpreted() {
        let a = SteamStatus::decode(&[0xDC, 0xDC, 0xFF, 0x05, 0x0A, 0x00]).unwrap();
        let b = SteamStatus::decode(&[0xDC, 0xDC, 0xFF, 0x05, 0x2C, 0x00]).unwrap();
        assert!(b.timer_seconds > a.timer_seconds);
        assert_eq!(a.timer_minutes, b.timer_minutes);
    }

    /// `STEAM-07`. The payload shape is a named selection, not a decision.
    #[test]
    fn the_param_codec_is_configuration_with_a_documented_default() {
        assert_eq!(ParamCodec::default(), ParamCodec::SteamBlock);
        assert_eq!(SET_PARAM_PAYLOAD_LEN, 3);
        assert_eq!(SET_PARAM_STATE_OFFSET, 1);
        assert_eq!(STATUS_PAYLOAD_LEN, 6);
        assert_eq!(STATUS_STATE_OFFSET, 2);
    }
}
