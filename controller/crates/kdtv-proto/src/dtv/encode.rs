//! The steam allowlist encoder.
//!
//! [`SteamOp`] is the complete set of frames this system can put on the DTV+
//! link. Adding an operation is a visible diff in one enum and breaks
//! [`SteamOp::ALL`]'s enumeration test.
//!
//! # What is denied, and how
//!
//! Two different mechanisms, because the DTV+ link has two different hazards.
//!
//! **Denied opcodes** are denied by the absence of a [`SteamOp`] variant. There
//! is no `Reboot`, no `FirmwareUpdate`, no `ActivateBoot`, so no program in this
//! workspace can spell one. The list is
//! [`denied_opcodes`](crate::dtv::denied_opcodes) and
//! [`req_dtv_plus_protocol_cmd_03_req_steam_adapter_cmd_02_no_denied_opcode_is_reachable`] scans every frame this encoder can produce
//! to prove none of them appears in the `CMD` field.
//!
//! **Power clean is not an opcode and is not denied that way.** `0xCC` is a
//! value of the operation-state byte inside the payload of `SET_DEV_PARAM`
//! (`0x34`), which is allowlisted. Omitting a command variant does nothing to
//! it. The denial is [`SteamOpState`], which has `Off` and `On` and no third
//! variant, and it is the only type this encoder accepts in that position.
//! [`req_hardware_steam_14_no_encoded_frame_carries_power_clean_in_the_state_byte`] scans the byte.
//! `CORRECTIONS.md` item 1, `STEAM-11`, `STEAM-14`(design).
//!
//! # Evidence
//!
//! Every payload layout below is tier `[C]` and unverified on this hardware. The
//! `SET_DEV_PARAM` shape is worse than that and is marked `[?]` at
//! [`ParamCodec`]: two sources describe incompatible payloads for the same
//! opcode and no capture of this bus exists.
//!
//! [`req_dtv_plus_protocol_cmd_03_req_steam_adapter_cmd_02_no_denied_opcode_is_reachable`]: self#tests
//! [`req_hardware_steam_14_no_encoded_frame_carries_power_clean_in_the_state_byte`]: self#tests

use crate::dtv::addr::{BROADCAST, DevAddr, MASTER, UNASSIGNED};
use crate::dtv::command::opcode;
use crate::dtv::frame::{EOF, MAX_FRAME, MAX_PAYLOAD, SOF, checksum, escape_into};
use crate::dtv::steam::{ParamCodec, SET_PARAM_PAYLOAD_LEN, SET_PARAM_STATE_OFFSET, SteamOpState};
use crate::gate::TransmitAuthority;
use crate::saturn::{DiscoveryToken, LinkPhase};
use core::fmt;
use kdtv_units::{LinkKind, SteamMinutes, SteamSetpoint};

/// **The allowlist.** Every frame this system can put on the DTV+ link.
///
/// Five opcodes come out of it: `0x05`, `0x07`, `0x30`, `0x34` and `0x3A`.
///
/// # Every parameter write is atomic
///
/// The three-field `SET_DEV_PARAM` payload writes setpoint, state and duration
/// together, so **a caller changing one must supply the other two**. That is why
/// [`SteamOp::SetTemperature`] carries a duration and a state, and why
/// [`SteamOp::SetDuration`] carries a setpoint: a partially-populated frame
/// would tell the generator to run at whatever the missing fields decoded to.
/// [`SteamOp::SetTemperature`] and [`SteamOp::SetDuration`] with equal fields
/// encode to identical bytes; they are separate variants because the log and the
/// confirming read (`STEAM-08`) need to know which field the operator changed.
///
/// # Operator consent is not represented here
///
/// `ARCHITECTURE.md` § 3.4 puts an `&OpenGrant` on the valve encode path and the
/// steam path has the same question — a steam session heats a room with someone
/// in it. `OpenGrant` lives in `kdtv-safety`, which depends on `kdtv-proto`, so
/// taking one here would be a dependency cycle. The consent gate sits one layer
/// up, at the `kdtv-engine` boundary that mints the grant, exactly as it does
/// for [`SaturnOp::SetOutlets`](crate::saturn::SaturnOp::SetOutlets). This crate
/// records the constraint rather than enforcing it. `AGENT.md` hard rule 2.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SteamOp {
    /// Start a session. `0x34` with operation state `On`.
    Start {
        temp: SteamSetpoint,
        minutes: SteamMinutes,
    },

    /// Stop. `0x34` with operation state `Off`.
    ///
    /// The one write that is never gated on anything: stopping is allowed in
    /// every phase, including a latched fault, and it is what a degraded link
    /// sends on every retry so the last frame the generator can have received is
    /// a stop. `STEAM-18`.
    Stop {
        temp: SteamSetpoint,
        minutes: SteamMinutes,
    },

    /// Change the setpoint, carrying the current state and duration forward.
    SetTemperature {
        temp: SteamSetpoint,
        minutes: SteamMinutes,
        state: SteamOpState,
    },

    /// Change the duration, carrying the current setpoint and state forward.
    ///
    /// [`SteamMinutes`] is 1..=20 with no zero: `steamTimerSetTime = 0` disables
    /// the generator's automatic shutoff, which is the only backstop that
    /// survives this service dying. `STEAM-18`(timer) / `STEAM-19`.
    SetDuration {
        temp: SteamSetpoint,
        minutes: SteamMinutes,
        state: SteamOpState,
    },

    /// Poll status. `0x30`, no payload. `STEAM-01`.
    ReadStatus,

    /// Clear the generator's stored fault flags. `0x3A`, no payload.
    ClearFaults,

    /// Address discovery. Requires a [`DiscoveryToken`]. `STEAM-04`.
    Discovery(DiscoveryStep),
}

/// The two discovery frames a **master** sends.
///
/// `DEV_REQUEST_ADDR` (`0x06`) is missing on purpose: it travels device to
/// master and carries the device ID, so a master that could build one could put
/// a device ID on the wire in a frame the master has no business sending. It is
/// decoded — see
/// [`DecodedDtv::requested_device_id`](crate::dtv::DecodedDtv::requested_device_id)
/// — and never encoded. Denial by absence of a variant, from the direction table
/// in [`crate::dtv::command::direction_of`].
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum DiscoveryStep {
    /// `0x05` `DEV_ADDRESS_OPP`, broadcast to `0xFF`, no payload. Invites
    /// unaddressed devices to answer. `ADDR-04`: this is the only frame that may
    /// carry the broadcast destination.
    AddressOpportunity,
    /// `0x07` `DEV_ASSIGN_ADDR`, to the still-unaddressed device at `0x00`,
    /// payload = the address being assigned.
    ///
    /// `DEST` is `0x00` because the device has no address yet — the same byte
    /// that means "master" in the other direction, which is why discovery routes
    /// on opcode. `ADDR-06`.
    AssignAddress(DevAddr),
}

impl SteamOp {
    /// Which kind this is, without its payload.
    #[must_use]
    pub const fn kind(&self) -> SteamOpKind {
        match self {
            Self::Start { .. } => SteamOpKind::Start,
            Self::Stop { .. } => SteamOpKind::Stop,
            Self::SetTemperature { .. } => SteamOpKind::SetTemperature,
            Self::SetDuration { .. } => SteamOpKind::SetDuration,
            Self::ReadStatus => SteamOpKind::ReadStatus,
            Self::ClearFaults => SteamOpKind::ClearFaults,
            Self::Discovery(DiscoveryStep::AddressOpportunity) => SteamOpKind::AddressOpportunity,
            Self::Discovery(DiscoveryStep::AssignAddress(_)) => SteamOpKind::AssignAddress,
        }
    }

    /// The operation state this operation writes, or `None` when it writes no
    /// state byte at all.
    ///
    /// **Never `0xCC`**: the return type has no variant for it.
    #[must_use]
    pub const fn state(&self) -> Option<SteamOpState> {
        match self {
            Self::Start { .. } => Some(SteamOpState::On),
            Self::Stop { .. } => Some(SteamOpState::Off),
            Self::SetTemperature { state, .. } | Self::SetDuration { state, .. } => Some(*state),
            Self::ReadStatus | Self::ClearFaults | Self::Discovery(_) => None,
        }
    }

    /// The complete allowlist, as kinds. Compared against a literal list in
    /// [`the_allowlist_is_exactly_these_eight_operations`].
    ///
    /// [`the_allowlist_is_exactly_these_eight_operations`]: self#tests
    pub const ALL: &'static [SteamOpKind] = SteamOpKind::ALL;
}

/// A [`SteamOp`] without its payload — the thing tables key on.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SteamOpKind {
    Start,
    Stop,
    SetTemperature,
    SetDuration,
    ReadStatus,
    ClearFaults,
    AddressOpportunity,
    AssignAddress,
}

impl SteamOpKind {
    pub const ALL: &'static [Self] = &[
        Self::Start,
        Self::Stop,
        Self::SetTemperature,
        Self::SetDuration,
        Self::ReadStatus,
        Self::ClearFaults,
        Self::AddressOpportunity,
        Self::AssignAddress,
    ];

    /// The `CMD` byte. An explicit table; there is no read/write bit pattern in
    /// this protocol to derive one from.
    #[must_use]
    pub const fn opcode(self) -> u8 {
        match self {
            Self::Start | Self::Stop | Self::SetTemperature | Self::SetDuration => {
                opcode::SET_DEV_PARAM
            }
            Self::ReadStatus => opcode::GET_DEV_STATUS,
            Self::ClearFaults => opcode::CLEAR_FAULT_FLAGS,
            Self::AddressOpportunity => opcode::DEV_ADDRESS_OPP,
            Self::AssignAddress => opcode::DEV_ASSIGN_ADDR,
        }
    }

    /// True for the two operations that require a [`DiscoveryToken`].
    #[must_use]
    pub const fn is_discovery(self) -> bool {
        matches!(self, Self::AddressOpportunity | Self::AssignAddress)
    }

    /// True for the operations that write the parameter block.
    #[must_use]
    pub const fn is_write(self) -> bool {
        matches!(
            self,
            Self::Start | Self::Stop | Self::SetTemperature | Self::SetDuration
        )
    }

    /// True for the one operation that can make the generator produce steam.
    /// [`SteamOpKind::Stop`] is not one of them.
    #[must_use]
    pub const fn can_start_steam(self) -> bool {
        matches!(self, Self::Start)
    }
}

/// A frame the encoder produced, and the only thing the DTV+ link is allowed to
/// transmit.
///
/// Private fields, no public constructor, no `Deserialize`, no `From<Vec<u8>>`,
/// no `From<DecodedDtv>`. Every value in existence came out of
/// [`SteamEncoder::encode`], so "what can this system transmit" is answered by
/// reading [`SteamOp`] and nothing else. `RAW-01`.
#[derive(Clone, PartialEq, Eq)]
pub struct DtvFrame {
    bytes: heapless::Vec<u8, MAX_FRAME>,
    op: SteamOpKind,
    dest: u8,
}

impl DtvFrame {
    /// The on-wire bytes, delimiters and byte stuffing included.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    #[must_use]
    pub const fn op(&self) -> SteamOpKind {
        self.op
    }

    /// The `DEST` field: an assigned address, [`BROADCAST`] for the address
    /// opportunity, or [`UNASSIGNED`] for the address assignment.
    #[must_use]
    pub const fn dest(&self) -> u8 {
        self.dest
    }

    /// The `CMD` field.
    #[must_use]
    pub const fn cmd(&self) -> u8 {
        self.op.opcode()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

impl fmt::Debug for DtvFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DtvFrame({:?} -> 0x{:02X}, [", self.op, self.dest)?;
        for (i, b) in self.bytes.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{b:02X}")?;
        }
        f.write_str("])")
    }
}

/// Why an encode was refused.
///
/// None of these is a denied *operation* — a denied operation has no variant to
/// name and cannot reach this function at all. These are the guards on the
/// operations that are allowed.
#[derive(Copy, Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum DtvEncodeDenied {
    /// Discovery outside [`LinkPhase::Discovery`], or without a token.
    /// `STEAM-17`.
    #[error("{op:?} requires a DiscoveryToken and LinkPhase::Discovery; the link is in {phase:?}")]
    DiscoveryOutsideDiscoveryPhase { op: SteamOpKind, phase: LinkPhase },

    /// A token minted for another link. Three links run side by side and a token
    /// is not transferable between them — a Saturn discovery token does not
    /// authorise a DTV+ address frame.
    #[error("DiscoveryToken is for {token}, this encoder drives {encoder}")]
    TokenForWrongLink { token: LinkKind, encoder: LinkKind },

    /// A parameter write in a phase that does not accept one.
    #[error("{op:?} is not permitted in {phase:?}")]
    WriteOutsideOperationalPhase { op: SteamOpKind, phase: LinkPhase },

    /// Clearing a latched fault mid-session would hide it. `STEAM-15` /
    /// `STEAM-16` require explicit operator action, which lives in the engine.
    #[error("{op:?} is only permitted in ReadyOff or Faulted, not {phase:?}")]
    FaultClearOutsideIdle { op: SteamOpKind, phase: LinkPhase },

    /// The assembled frame does not fit. Unreachable for the current allowlist —
    /// the longest payload is three bytes — and kept because the next operation
    /// added might not be. `FRAME-06`.
    #[error("{op:?} produced a frame above the {max}-byte wire maximum")]
    FrameLength { op: SteamOpKind, max: usize },
}

/// Builds DTV+ frames for the steam link.
///
/// The source address is always [`MASTER`] — this master has one identity and,
/// unlike the Saturn buses, the sources do not disagree about it. `ADDR-01`.
/// No `Default`, deliberately. A derived `Default` would be a public
/// constructor that bypasses the transmit gate, and the gate's whole claim is
/// that there is no way to obtain an encoder without an authority.
#[derive(Clone, Debug)]
pub struct SteamEncoder {
    auth: TransmitAuthority,
    codec: ParamCodec,
}

impl SteamEncoder {
    /// There is exactly one DTV+ link in this build, so the link identity is
    /// fixed rather than configured. A [`DiscoveryToken`] minted for a Saturn
    /// zone is refused by [`DtvEncodeDenied::TokenForWrongLink`].
    pub const LINK: LinkKind = LinkKind::Steam;

    /// **The first of the transmit gate's two boundaries**, exactly as on the
    /// valve side — see [`crate::saturn::Encoder::new`] and [`crate::gate`].
    #[must_use]
    pub fn new(auth: &TransmitAuthority) -> Self {
        Self::with_codec(auth, ParamCodec::SteamBlock)
    }

    /// Selects the `SET_DEV_PARAM` payload shape. `STEAM-07`.
    #[must_use]
    pub fn with_codec(auth: &TransmitAuthority, codec: ParamCodec) -> Self {
        Self {
            auth: auth.clone(),
            codec,
        }
    }

    /// The authority this encoder was built under. `kdtv-hal` reads
    /// [`permits_real_bus_on`](TransmitAuthority::permits_real_bus_on) from it
    /// before opening a serial backend for the steam link.
    #[must_use]
    pub const fn authority(&self) -> &TransmitAuthority {
        &self.auth
    }

    #[must_use]
    pub const fn codec(&self) -> ParamCodec {
        self.codec
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        Self::LINK
    }

    /// Encodes one allowlisted operation.
    ///
    /// `dest` is the address discovery assigned to the device. **It is never
    /// derived from the device ID**: there is no conversion from
    /// [`DeviceId`](crate::dtv::DeviceId) to [`DevAddr`], so
    /// `88 05 00 30 CB 55` — the source's own example, which puts the steam
    /// device ID in the `DEST` field — is reachable only if discovery actually
    /// assigned `0x05`. `CORRECTIONS.md` item 2.
    ///
    /// The two discovery operations ignore `dest`: the address opportunity is a
    /// broadcast and the assignment goes to `0x00`, because the device being
    /// addressed has no address yet.
    pub fn encode(
        &self,
        dest: DevAddr,
        op: &SteamOp,
        phase: LinkPhase,
        disco: Option<&DiscoveryToken>,
    ) -> Result<DtvFrame, DtvEncodeDenied> {
        let kind = op.kind();

        if kind.is_discovery() {
            let Some(token) = disco else {
                return Err(DtvEncodeDenied::DiscoveryOutsideDiscoveryPhase { op: kind, phase });
            };
            if token.link() != Self::LINK {
                return Err(DtvEncodeDenied::TokenForWrongLink {
                    token: token.link(),
                    encoder: Self::LINK,
                });
            }
            // Belt and braces: a token can only be minted in Discovery, and the
            // phase is checked again here so a token held across a transition is
            // still refused.
            if phase != LinkPhase::Discovery {
                return Err(DtvEncodeDenied::DiscoveryOutsideDiscoveryPhase { op: kind, phase });
            }
        }

        // Writes are refused before the link is addressed and during discovery,
        // when the generator must be off. `Stop` is exempt: stopping is allowed
        // in every phase, always.
        if kind.is_write()
            && kind != SteamOpKind::Stop
            && matches!(phase, LinkPhase::Booting | LinkPhase::Discovery)
        {
            return Err(DtvEncodeDenied::WriteOutsideOperationalPhase { op: kind, phase });
        }

        // Clearing fault flags mid-session would erase the evidence a latched
        // fault rests on. STEAM-15 / STEAM-16 want an explicit operator action,
        // and the engine is where that consent lives; the encoder narrows the
        // window to the two phases where nothing is running.
        if kind == SteamOpKind::ClearFaults
            && !matches!(phase, LinkPhase::ReadyOff | LinkPhase::Faulted)
        {
            return Err(DtvEncodeDenied::FaultClearOutsideIdle { op: kind, phase });
        }

        let (frame_dest, payload) = self.payload(dest, op);
        Self::assemble(frame_dest, kind, payload.as_slice())
    }

    /// The destination and payload for an operation.
    ///
    /// Infallible: every input is already a validated type. [`SteamSetpoint`] is
    /// clamped to 90–125 °F in whole degrees and [`SteamMinutes`] to 1–20, so
    /// there is no range check to fail here — the clamp is in the type, one
    /// layer up, which is where `STEAM-09` and `STEAM-19` want it.
    fn payload(&self, dest: DevAddr, op: &SteamOp) -> (u8, heapless::Vec<u8, MAX_PAYLOAD>) {
        let mut payload: heapless::Vec<u8, MAX_PAYLOAD> = heapless::Vec::new();
        let frame_dest = match op {
            SteamOp::Start { temp, minutes }
            | SteamOp::Stop { temp, minutes }
            | SteamOp::SetTemperature { temp, minutes, .. }
            | SteamOp::SetDuration { temp, minutes, .. } => {
                // The state comes from `SteamOp::state`, whose type has two
                // variants. This is the only place an operation-state byte is
                // written, and 0xCC has no way in. CORRECTIONS.md item 1.
                let state = op.state().unwrap_or(SteamOpState::Off);
                match self.codec {
                    ParamCodec::SteamBlock => {
                        // [desired temp Fx2, operation state, timer minutes].
                        // `[?]` — see `ParamCodec`.
                        let _ = payload.push(temp.wire().raw());
                        let _ = payload.push(state.wire());
                        let _ = payload.push(minutes.wire());
                    }
                }
                dest.get()
            }
            SteamOp::ReadStatus | SteamOp::ClearFaults => dest.get(),
            SteamOp::Discovery(DiscoveryStep::AddressOpportunity) => BROADCAST,
            SteamOp::Discovery(DiscoveryStep::AssignAddress(addr)) => {
                let _ = payload.push(addr.get());
                UNASSIGNED
            }
        };
        (frame_dest, payload)
    }

    /// Header, checksum over the logical bytes, byte stuffing, delimiters — in
    /// that order. `FRAME-09`: stuffing is the last step, so the checksum never
    /// covers an escape byte.
    fn assemble(dest: u8, kind: SteamOpKind, payload: &[u8]) -> Result<DtvFrame, DtvEncodeDenied> {
        let cmd = kind.opcode();
        let chk = checksum(dest, MASTER, cmd, payload);

        let too_long = || DtvEncodeDenied::FrameLength {
            op: kind,
            max: MAX_FRAME,
        };

        // Header, payload, checksum — each byte-stuffed as it goes in, which is
        // the last step before the wire. FRAME-09.
        let mut bytes: heapless::Vec<u8, MAX_FRAME> = heapless::Vec::new();
        bytes.push(SOF).map_err(|_| too_long())?;
        escape_into(&[dest, MASTER, cmd], &mut bytes).map_err(|_| too_long())?;
        escape_into(payload, &mut bytes).map_err(|_| too_long())?;
        escape_into(&[chk], &mut bytes).map_err(|_| too_long())?;
        bytes.push(EOF).map_err(|_| too_long())?;

        Ok(DtvFrame {
            bytes,
            op: kind,
            dest,
        })
    }
}

/// The payload offset of the operation-state byte in a frame this encoder
/// produced, or `None` for an operation that writes no state.
///
/// Exists so the scan tests, and the emulator's trace assertions, name the same
/// offset the encoder wrote. `CORRECTIONS.md` item 1.
#[must_use]
pub const fn state_byte_offset(kind: SteamOpKind) -> Option<usize> {
    if kind.is_write() {
        Some(SET_PARAM_STATE_OFFSET)
    } else {
        None
    }
}

const _: () = assert!(SET_PARAM_STATE_OFFSET < SET_PARAM_PAYLOAD_LEN);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dtv::command::{allowlisted_opcodes, denied_opcodes, discovery_opcodes};
    use crate::dtv::decode::{DtvRxBuffer, decode, decode_frame};
    use crate::dtv::frame::ESC;
    use crate::dtv::steam::SteamStateByte;
    use crate::fixtures::FixtureSet;
    use kdtv_units::Fx2;

    /// Every encoder in these tests is built under the emulator scope, which is
    /// the only scope today's tier [C] fixtures can grant. `crate::gate`.
    fn auth() -> TransmitAuthority {
        TransmitAuthority::emulator_only(FixtureSet::embedded())
    }

    fn token() -> DiscoveryToken {
        DiscoveryToken::mint(LinkKind::Steam, LinkPhase::Discovery).unwrap()
    }

    fn sp(f: u8) -> SteamSetpoint {
        SteamSetpoint::try_new(Fx2::from_raw(f)).unwrap()
    }

    fn mins(m: u8) -> SteamMinutes {
        SteamMinutes::try_new(m).unwrap()
    }

    /// The default: 110 °F for 10 minutes, the commissioning pair from
    /// `PH5-03`.
    fn default_pair() -> (SteamSetpoint, SteamMinutes) {
        (
            SteamSetpoint::try_new(SteamSetpoint::FACTORY_DEFAULT).unwrap(),
            SteamMinutes::default(),
        )
    }

    /// Every legal setpoint: 90–125 °F in whole degrees is `Fx2` 180..=250 even.
    fn every_setpoint() -> impl Iterator<Item = SteamSetpoint> {
        (180u8..=250)
            .step_by(2)
            .map(|f| SteamSetpoint::try_new(Fx2::from_raw(f)).unwrap())
    }

    /// Every operation the encoder can be asked for, with the given fields.
    fn every_op(temp: SteamSetpoint, minutes: SteamMinutes) -> Vec<SteamOp> {
        let mut ops = vec![
            SteamOp::Start { temp, minutes },
            SteamOp::Stop { temp, minutes },
            SteamOp::ReadStatus,
            SteamOp::ClearFaults,
            SteamOp::Discovery(DiscoveryStep::AddressOpportunity),
        ];
        for state in SteamOpState::ALL {
            ops.push(SteamOp::SetTemperature {
                temp,
                minutes,
                state,
            });
            ops.push(SteamOp::SetDuration {
                temp,
                minutes,
                state,
            });
        }
        for a in DevAddr::ALL {
            ops.push(SteamOp::Discovery(DiscoveryStep::AssignAddress(a)));
        }
        ops
    }

    // ---- Golden frames -----------------------------------------------------

    /// `STEAM-01`. `88 03 00 30 CD 55` at the address the reference topology
    /// assigns.
    #[test]
    fn the_status_request_is_the_documented_frame() {
        let e = SteamEncoder::new(&auth());
        let f = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::ReadStatus,
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        assert_eq!(f.bytes(), &[0x88, 0x03, 0x00, 0x30, 0xCD, 0x55]);
        assert_eq!(f.cmd(), opcode::GET_DEV_STATUS);
        assert_eq!(f.dest(), 0x03);
    }

    /// `STEAM-06`. `88 03 00 34 DC FF 0A E4 55` — 110 °F, on, ten minutes.
    #[test]
    fn the_start_frame_is_the_derived_fixture() {
        let e = SteamEncoder::new(&auth());
        let (temp, minutes) = default_pair();
        let f = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::Start { temp, minutes },
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        assert_eq!(
            f.bytes(),
            &[0x88, 0x03, 0x00, 0x34, 0xDC, 0xFF, 0x0A, 0xE4, 0x55]
        );

        // And the stop, same setpoint and duration: only the state byte moves.
        let s = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::Stop { temp, minutes },
                LinkPhase::Running,
                None,
            )
            .unwrap();
        assert_eq!(
            s.bytes(),
            &[0x88, 0x03, 0x00, 0x34, 0xDC, 0x00, 0x0A, 0xE3, 0x55]
        );
    }

    #[test]
    fn the_clear_fault_frame_is_the_derived_fixture() {
        let e = SteamEncoder::new(&auth());
        let f = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::ClearFaults,
                LinkPhase::Faulted,
                None,
            )
            .unwrap();
        assert_eq!(f.bytes(), &[0x88, 0x03, 0x00, 0x3A, 0xC3, 0x55]);
    }

    /// `STEAM-04`. The two discovery frames a master sends, byte for byte.
    #[test]
    fn the_master_side_discovery_frames_are_the_documented_ones() {
        let e = SteamEncoder::new(&auth());
        let t = token();
        let opp = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::Discovery(DiscoveryStep::AddressOpportunity),
                LinkPhase::Discovery,
                Some(&t),
            )
            .unwrap();
        assert_eq!(opp.bytes(), &[0x88, 0xFF, 0x00, 0x05, 0xFC, 0x55]);
        assert_eq!(opp.dest(), BROADCAST);

        let assign = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::Discovery(DiscoveryStep::AssignAddress(DevAddr::REFERENCE)),
                LinkPhase::Discovery,
                Some(&t),
            )
            .unwrap();
        assert_eq!(assign.bytes(), &[0x88, 0x00, 0x00, 0x07, 0x03, 0xF6, 0x55]);
        assert_eq!(assign.dest(), UNASSIGNED);
    }

    /// `DEV_REQUEST_ADDR` has no encoder variant, so the master cannot send the
    /// device's half of the handshake. The frame is still decodable.
    #[test]
    fn the_device_side_discovery_frame_is_decodable_and_unencodable() {
        let f = decode_frame(&[0x88, 0x00, 0x00, 0x06, 0x05, 0xF5, 0x55]).unwrap();
        assert_eq!(f.cmd, opcode::DEV_REQUEST_ADDR);
        // And no operation in the allowlist resolves to that opcode.
        for k in SteamOp::ALL {
            assert_ne!(k.opcode(), opcode::DEV_REQUEST_ADDR, "{k:?}");
        }
    }

    // ---- The allowlist -----------------------------------------------------

    #[test]
    fn the_allowlist_is_exactly_these_eight_operations() {
        let expected = [
            SteamOpKind::Start,
            SteamOpKind::Stop,
            SteamOpKind::SetTemperature,
            SteamOpKind::SetDuration,
            SteamOpKind::ReadStatus,
            SteamOpKind::ClearFaults,
            SteamOpKind::AddressOpportunity,
            SteamOpKind::AssignAddress,
        ];
        assert_eq!(SteamOp::ALL, &expected[..]);
        assert_eq!(SteamOp::ALL.len(), 8);

        // Four writes, two reads/clears, two discovery frames.
        assert_eq!(SteamOp::ALL.iter().filter(|k| k.is_write()).count(), 4);
        assert_eq!(SteamOp::ALL.iter().filter(|k| k.is_discovery()).count(), 2);
        // Exactly one operation can start steam, and it is not Stop.
        assert_eq!(
            SteamOp::ALL.iter().filter(|k| k.can_start_steam()).count(),
            1
        );
        assert!(!SteamOpKind::Stop.can_start_steam());
    }

    /// The five opcodes the allowlist resolves to, and their disjointness from
    /// the denied set.
    #[test]
    fn req_dtv_plus_protocol_cmd_07_req_steam_adapter_cmd_01_the_reachable_opcodes_are_a_known_five()
     {
        let mut reachable: Vec<u8> = SteamOp::ALL.iter().map(|k| k.opcode()).collect();
        reachable.sort_unstable();
        reachable.dedup();
        assert_eq!(reachable, vec![0x05, 0x07, 0x30, 0x34, 0x3A]);
        for d in denied_opcodes() {
            assert!(!reachable.contains(d), "0x{d:02X} is reachable");
        }
        // Every reachable opcode is either allowlisted or a discovery opcode.
        for r in &reachable {
            assert!(
                allowlisted_opcodes().contains(r) || discovery_opcodes().contains(r),
                "0x{r:02X}"
            );
        }
        // The device-to-master opcodes are recognised and never emitted.
        for b in [0x31u8, 0x35, 0x36, 0x37, 0x06] {
            assert!(!reachable.contains(&b), "0x{b:02X} is reachable");
        }
    }

    /// **The denied-opcode scan.**
    ///
    /// Builds every frame the encoder can emit — every operation, every legal
    /// setpoint, every legal duration, every destination address, every phase —
    /// and asserts the `CMD` field is never one of the denied opcodes.
    ///
    /// The scan is over the **`CMD` field**, not over all of a frame's bytes,
    /// for the same reason the Saturn scan is: a checksum or a payload byte can
    /// legitimately equal a denied opcode. `Fx2` 0x03 through 0xA1 are all
    /// inside the encoding range, and a checksum can be any byte at all, so a
    /// whole-frame byte scan would fail on a *correct* frame and would have to
    /// be weakened until it proved nothing. `CMD` is the field that selects the
    /// operation.
    #[test]
    fn req_dtv_plus_protocol_cmd_03_req_steam_adapter_cmd_02_no_denied_opcode_is_reachable() {
        let e = SteamEncoder::new(&auth());
        let t = token();
        let denied = denied_opcodes();
        let mut frames = 0u32;
        for temp in every_setpoint() {
            for m in [1u8, 5, 10, 19, 20] {
                for op in every_op(temp, mins(m)) {
                    for phase in [
                        LinkPhase::Booting,
                        LinkPhase::Discovery,
                        LinkPhase::ReadyOff,
                        LinkPhase::Running,
                        LinkPhase::Paused,
                        LinkPhase::Faulted,
                    ] {
                        for dest in DevAddr::ALL {
                            let Ok(f) = e.encode(dest, &op, phase, Some(&t)) else {
                                continue;
                            };
                            frames += 1;
                            let d = decode_frame(f.bytes()).expect("encoder emits valid frames");
                            assert_eq!(d.cmd, f.cmd());
                            assert!(
                                !denied.contains(&d.cmd),
                                "denied opcode 0x{:02X} emitted for {op:?}",
                                d.cmd
                            );
                        }
                    }
                }
            }
        }
        // A scan that encoded nothing would pass vacuously.
        assert!(frames > 10_000, "the scan only built {frames} frames");
    }

    /// **The `0xCC` scan.** `CORRECTIONS.md` item 1 / `STEAM-11`.
    ///
    /// Power clean is a payload value of an allowlisted command, so omitting a
    /// command variant does not deny it. What denies it is [`SteamOpState`]
    /// having no third variant. This walks every frame the encoder can produce,
    /// decodes it back, and asserts the operation-state byte is `0x00` or `0xFF`
    /// and never `0xCC`.
    #[test]
    fn req_hardware_steam_14_no_encoded_frame_carries_power_clean_in_the_state_byte() {
        let e = SteamEncoder::new(&auth());
        let t = token();
        let mut writes = 0u32;
        for temp in every_setpoint() {
            for m in 1u8..=20 {
                for op in every_op(temp, mins(m)) {
                    for phase in [LinkPhase::ReadyOff, LinkPhase::Running, LinkPhase::Paused] {
                        for dest in DevAddr::ALL {
                            let Ok(f) = e.encode(dest, &op, phase, Some(&t)) else {
                                continue;
                            };
                            let d = decode_frame(f.bytes()).expect("encoder emits valid frames");
                            let Some(offset) = state_byte_offset(f.op()) else {
                                // Not a parameter write: it has no state byte at
                                // all, which is its own form of the denial.
                                assert_ne!(d.cmd, opcode::SET_DEV_PARAM);
                                continue;
                            };
                            writes += 1;
                            assert_eq!(d.cmd, opcode::SET_DEV_PARAM);
                            assert_eq!(d.payload.len(), SET_PARAM_PAYLOAD_LEN);
                            let state = *d.payload.get(offset).expect("three-byte payload");
                            assert_ne!(
                                state,
                                SteamOpState::POWER_CLEAN_BYTE,
                                "0xCC reached the state byte for {op:?}"
                            );
                            assert!(
                                state == 0x00 || state == 0xFF,
                                "state byte 0x{state:02X} for {op:?}"
                            );
                            assert!(matches!(
                                SteamStateByte::decode(state),
                                SteamStateByte::Off | SteamStateByte::On
                            ));
                        }
                    }
                }
            }
        }
        assert!(writes > 10_000, "the scan only built {writes} writes");
    }

    /// Why the `0xCC` scan is field-level and not a whole-frame byte scan: the
    /// **checksum** can be `0xCC` on a perfectly correct frame.
    ///
    /// `0x04 + 0x00 + 0x30 = 0x34`, whose two's complement is `0xCC`. A
    /// whole-frame scan would reject this frame, which is the documented status
    /// request to a device at address `0x04`.
    #[test]
    fn zero_x_cc_can_be_a_checksum_and_that_is_why_the_scan_is_field_level() {
        let e = SteamEncoder::new(&auth());
        let f = e
            .encode(
                DevAddr::new(0x04).unwrap(),
                &SteamOp::ReadStatus,
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        assert_eq!(f.bytes(), &[0x88, 0x04, 0x00, 0x30, 0xCC, 0x55]);
        assert!(f.bytes().contains(&SteamOpState::POWER_CLEAN_BYTE));
        // It is a checksum, not a state byte — and this operation has no state
        // byte at all.
        assert_eq!(state_byte_offset(f.op()), None);

        // A parameter write can do it too: 0x05 + 0x34 + 0xFA + 0x00 + 0x01
        // sums to 0x134, and the two's complement of 0x34 is 0xCC.
        let g = e
            .encode(
                DevAddr::new(0x05).unwrap(),
                &SteamOp::Stop {
                    temp: sp(250),
                    minutes: mins(1),
                },
                LinkPhase::Running,
                None,
            )
            .unwrap();
        assert_eq!(
            g.bytes(),
            &[0x88, 0x05, 0x00, 0x34, 0xFA, 0x00, 0x01, 0xCC, 0x55]
        );
        let d = decode_frame(g.bytes()).unwrap();
        assert_eq!(
            *d.payload.get(SET_PARAM_STATE_OFFSET).unwrap(),
            SteamOpState::Off.wire()
        );
    }

    // ---- Round trips -------------------------------------------------------

    /// Everything the encoder emits, the decoder parses back to the same
    /// fields — through both entry points.
    #[test]
    fn every_encoded_frame_decodes_back_to_its_own_fields() {
        let e = SteamEncoder::new(&auth());
        let t = token();
        let (temp, minutes) = default_pair();
        for op in every_op(temp, minutes) {
            let phase = if op.kind().is_discovery() {
                LinkPhase::Discovery
            } else if op.kind() == SteamOpKind::ClearFaults {
                LinkPhase::Faulted
            } else {
                LinkPhase::Running
            };
            let f = e
                .encode(DevAddr::REFERENCE, &op, phase, Some(&t))
                .unwrap_or_else(|e| panic!("{op:?} refused: {e}"));

            let d = decode_frame(f.bytes()).unwrap();
            assert_eq!(d.dest, f.dest());
            assert_eq!(d.src, MASTER);
            assert_eq!(d.cmd, f.cmd());

            let mut rx = DtvRxBuffer::new();
            rx.extend(f.bytes());
            let streamed = decode(&mut rx).unwrap().unwrap();
            assert_eq!(streamed, d);
            assert!(rx.is_empty());
        }
    }

    /// **No frame this encoder can produce needs byte stuffing at all**, and
    /// that is a finding, not a convenience.
    ///
    /// It was not the expected result. The first version of this test asserted
    /// that some frame in the domain would carry an escaped checksum, exercising
    /// the stuffing path in ordinary traffic. It does not, and the arithmetic
    /// says it never can:
    ///
    /// - The three header bytes are safe by construction. `DEST` is `0x03..=0x07`
    ///   or `0xFF` or `0x00`, `SRC` is `0x00`, and the five reachable opcodes are
    ///   `0x05`, `0x07`, `0x30`, `0x34`, `0x3A`. None is reserved.
    /// - The payload is safe by the clamps. A setpoint is `Fx2` 180..=250 and
    ///   `0x55`, `0x88` and `0xAA` are 85, 136 and 170 — all below the floor. The
    ///   state byte is `0x00` or `0xFF`, and a duration is 1..=20.
    /// - The checksum cannot land on one either. For a `SET_DEV_PARAM` the
    ///   covered sum reaches only `0x00..=0x49` and `0xEB..=0xFF`, so the
    ///   checksum reaches only `0x00`, `0xB7..=0xFF` and `0x01..=0x15`. The three
    ///   sums that would produce a reserved checksum — `0xAB`, `0x78`, `0x56` —
    ///   all sit in the gap. `GET_DEV_STATUS`, `CLEAR_FAULT_FLAGS` and both
    ///   discovery frames have even narrower ranges.
    ///
    /// The consequence is what matters: **the byte-stuffing path is exercised
    /// only on receive**, and only by frames another device sent. It cannot be
    /// covered incidentally by transmit traffic, so it is covered deliberately by
    /// the decoder's own vectors and by the round-trip property test. It also
    /// means an escape byte appearing in a TX trace is itself evidence that
    /// something built a frame this encoder did not.
    #[test]
    fn no_frame_this_encoder_produces_needs_byte_stuffing() {
        // The payload domain, byte by byte.
        for f in (180u8..=250).step_by(2) {
            assert!(!crate::dtv::frame::is_reserved(f), "Fx2 {f}");
        }
        for b in [SteamOpState::Off.wire(), SteamOpState::On.wire()] {
            assert!(!crate::dtv::frame::is_reserved(b));
        }
        for m in 1u8..=20 {
            assert!(!crate::dtv::frame::is_reserved(m));
        }
        for k in SteamOp::ALL {
            assert!(!crate::dtv::frame::is_reserved(k.opcode()), "{k:?}");
        }

        let e = SteamEncoder::new(&auth());
        let t = token();
        let mut frames = 0u32;
        for temp in every_setpoint() {
            for m in 1u8..=20 {
                for op in every_op(temp, mins(m)) {
                    for dest in DevAddr::ALL {
                        let Ok(f) = e.encode(dest, &op, LinkPhase::Running, Some(&t)) else {
                            continue;
                        };
                        frames += 1;
                        let d = decode_frame(f.bytes()).unwrap();
                        // SOF + DEST + SRC + CMD + payload + CHECKSUM + EOF,
                        // with nothing stuffed.
                        let unstuffed = 2 + 3 + d.payload.len() + 1;
                        assert_eq!(f.len(), unstuffed, "{op:?} was stuffed");
                        let chk = checksum(d.dest, d.src, d.cmd, d.payload.as_slice());
                        assert!(
                            !crate::dtv::frame::is_reserved(chk),
                            "{op:?} chk 0x{chk:02X}"
                        );
                        for b in f.bytes() {
                            assert_ne!(*b, ESC, "an escape byte reached the wire for {op:?}");
                        }
                    }
                }
            }
        }
        assert!(frames > 10_000, "the scan only built {frames} frames");
    }

    // ---- The discovery token ----------------------------------------------

    /// `STEAM-17`. Address management outside discovery has no spelling: there
    /// is no token to pass, and passing none is refused.
    #[test]
    fn discovery_requires_a_token_and_the_discovery_phase() {
        let e = SteamEncoder::new(&auth());
        for phase in [
            LinkPhase::Booting,
            LinkPhase::ReadyOff,
            LinkPhase::Running,
            LinkPhase::Paused,
            LinkPhase::Faulted,
        ] {
            assert!(DiscoveryToken::mint(LinkKind::Steam, phase).is_none());
            for op in [
                SteamOp::Discovery(DiscoveryStep::AddressOpportunity),
                SteamOp::Discovery(DiscoveryStep::AssignAddress(DevAddr::REFERENCE)),
            ] {
                let err = e.encode(DevAddr::REFERENCE, &op, phase, None).unwrap_err();
                assert!(matches!(
                    err,
                    DtvEncodeDenied::DiscoveryOutsideDiscoveryPhase { .. }
                ));
            }
        }

        // A token held across a phase transition is still refused.
        let t = token();
        let err = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::Discovery(DiscoveryStep::AddressOpportunity),
                LinkPhase::Running,
                Some(&t),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            DtvEncodeDenied::DiscoveryOutsideDiscoveryPhase { .. }
        ));
    }

    /// Three links, three tokens, no crossing. A Saturn zone's discovery token
    /// does not authorise a DTV+ address frame.
    #[test]
    fn a_saturn_token_does_not_authorise_a_dtv_discovery_frame() {
        use kdtv_units::ZoneId;
        let e = SteamEncoder::new(&auth());
        for zone in [ZoneId::Zone1, ZoneId::Zone2] {
            let wrong = DiscoveryToken::mint(LinkKind::Zone(zone), LinkPhase::Discovery).unwrap();
            let err = e
                .encode(
                    DevAddr::REFERENCE,
                    &SteamOp::Discovery(DiscoveryStep::AddressOpportunity),
                    LinkPhase::Discovery,
                    Some(&wrong),
                )
                .unwrap_err();
            assert_eq!(
                err,
                DtvEncodeDenied::TokenForWrongLink {
                    token: LinkKind::Zone(zone),
                    encoder: LinkKind::Steam,
                }
            );
        }
    }

    // ---- Phase gating ------------------------------------------------------

    /// Stopping is allowed in every phase. Starting is not.
    #[test]
    fn stop_is_never_gated_and_start_is() {
        let e = SteamEncoder::new(&auth());
        let (temp, minutes) = default_pair();
        for phase in [
            LinkPhase::Booting,
            LinkPhase::Discovery,
            LinkPhase::ReadyOff,
            LinkPhase::Running,
            LinkPhase::Paused,
            LinkPhase::Faulted,
        ] {
            assert!(
                e.encode(
                    DevAddr::REFERENCE,
                    &SteamOp::Stop { temp, minutes },
                    phase,
                    None
                )
                .is_ok(),
                "stop refused in {phase:?}"
            );
            let start = e.encode(
                DevAddr::REFERENCE,
                &SteamOp::Start { temp, minutes },
                phase,
                None,
            );
            match phase {
                LinkPhase::Booting | LinkPhase::Discovery => {
                    assert!(matches!(
                        start.unwrap_err(),
                        DtvEncodeDenied::WriteOutsideOperationalPhase { .. }
                    ));
                }
                _ => assert!(start.is_ok(), "start refused in {phase:?}"),
            }
        }
    }

    /// Reads are free in every phase; clearing faults is not.
    #[test]
    fn a_status_read_is_free_and_a_fault_clear_is_narrowed() {
        let e = SteamEncoder::new(&auth());
        for phase in [
            LinkPhase::Booting,
            LinkPhase::Discovery,
            LinkPhase::ReadyOff,
            LinkPhase::Running,
            LinkPhase::Paused,
            LinkPhase::Faulted,
        ] {
            assert!(
                e.encode(DevAddr::REFERENCE, &SteamOp::ReadStatus, phase, None)
                    .is_ok(),
                "read refused in {phase:?}"
            );
            let clear = e.encode(DevAddr::REFERENCE, &SteamOp::ClearFaults, phase, None);
            match phase {
                LinkPhase::ReadyOff | LinkPhase::Faulted => assert!(clear.is_ok()),
                _ => assert!(matches!(
                    clear.unwrap_err(),
                    DtvEncodeDenied::FaultClearOutsideIdle { .. }
                )),
            }
        }
    }

    // ---- Clamps and types --------------------------------------------------

    /// `STEAM-09` / `STEAM-18`(timer) / `STEAM-19`. The clamps are in the types,
    /// so an out-of-range value never reaches the encoder.
    #[test]
    fn out_of_range_values_cannot_be_built_let_alone_encoded() {
        // Below 90 F, above 125 F, and on a half-degree step.
        assert!(SteamSetpoint::try_new(Fx2::from_raw(178)).is_err());
        assert!(SteamSetpoint::try_new(Fx2::from_raw(252)).is_err());
        assert!(SteamSetpoint::try_new(Fx2::from_raw(181)).is_err());
        assert!(SteamSetpoint::try_new(Fx2::from_raw(180)).is_ok());
        assert!(SteamSetpoint::try_new(Fx2::from_raw(250)).is_ok());

        // Zero minutes disables the generator's automatic shutoff, which is the
        // only backstop that survives this service dying. There is no way to
        // spell it.
        assert!(SteamMinutes::try_new(0).is_err());
        assert!(SteamMinutes::try_new(21).is_err());
        assert!(SteamMinutes::try_new(1).is_ok());
        assert!(SteamMinutes::try_new(20).is_ok());

        // And nothing the encoder emits carries a zero duration.
        let e = SteamEncoder::new(&auth());
        for temp in every_setpoint() {
            for m in 1u8..=20 {
                for op in [
                    SteamOp::Start {
                        temp,
                        minutes: mins(m),
                    },
                    SteamOp::Stop {
                        temp,
                        minutes: mins(m),
                    },
                ] {
                    let f = e
                        .encode(DevAddr::REFERENCE, &op, LinkPhase::Running, None)
                        .unwrap();
                    let d = decode_frame(f.bytes()).unwrap();
                    assert_ne!(*d.payload.get(2).unwrap(), 0);
                    assert!((1..=20).contains(d.payload.get(2).unwrap()));
                    assert!((180..=250).contains(d.payload.first().unwrap()));
                }
            }
        }
    }

    /// `ADDR-04`. The broadcast destination appears on exactly one operation.
    #[test]
    fn only_the_address_opportunity_broadcasts() {
        let e = SteamEncoder::new(&auth());
        let t = token();
        let (temp, minutes) = default_pair();
        for op in every_op(temp, minutes) {
            for phase in [
                LinkPhase::Discovery,
                LinkPhase::ReadyOff,
                LinkPhase::Running,
                LinkPhase::Faulted,
            ] {
                let Ok(f) = e.encode(DevAddr::REFERENCE, &op, phase, Some(&t)) else {
                    continue;
                };
                if f.dest() == BROADCAST {
                    assert_eq!(f.op(), SteamOpKind::AddressOpportunity);
                }
            }
        }
    }

    /// `CORRECTIONS.md` item 2. The destination comes from the discovery table,
    /// never from the device ID.
    #[test]
    fn req_steam_adapter_addr_05_the_destination_is_an_assigned_address_not_a_device_id() {
        let e = SteamEncoder::new(&auth());
        for a in DevAddr::ALL {
            let f = e
                .encode(a, &SteamOp::ReadStatus, LinkPhase::ReadyOff, None)
                .unwrap();
            assert_eq!(f.dest(), a.get());
        }
        // The source's own conflated example, `88 05 00 30 CB 55`, is reachable
        // only by way of an assigned address of 0x05 — never from
        // DeviceId::STEAM_GENERATOR, which has no path into a DEST field. The
        // reference topology enrols one device and expects 0x03. ADDR-07.
        let f = e
            .encode(
                DevAddr::new(0x05).unwrap(),
                &SteamOp::ReadStatus,
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        assert_eq!(f.bytes(), &[0x88, 0x05, 0x00, 0x30, 0xCB, 0x55]);
        assert_ne!(f.dest(), DevAddr::REFERENCE.get());
    }

    /// `STEAM-07`. Changing one field writes all three, so the two "set"
    /// operations are byte-identical given identical fields — the difference is
    /// intent, recorded in [`SteamOpKind`], not on the wire.
    #[test]
    fn a_parameter_write_is_atomic_in_all_three_fields() {
        let e = SteamEncoder::new(&auth());
        let (temp, minutes) = default_pair();
        let a = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::SetTemperature {
                    temp,
                    minutes,
                    state: SteamOpState::On,
                },
                LinkPhase::Running,
                None,
            )
            .unwrap();
        let b = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::SetDuration {
                    temp,
                    minutes,
                    state: SteamOpState::On,
                },
                LinkPhase::Running,
                None,
            )
            .unwrap();
        assert_eq!(a.bytes(), b.bytes());
        assert_ne!(a.op(), b.op());
        // And it is the same three-field frame Start produces.
        let start = e
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::Start { temp, minutes },
                LinkPhase::Running,
                None,
            )
            .unwrap();
        assert_eq!(start.bytes(), a.bytes());
        assert_eq!(decode_frame(a.bytes()).unwrap().payload.len(), 3);
    }

    #[test]
    fn the_state_byte_offset_names_what_the_encoder_wrote() {
        assert_eq!(state_byte_offset(SteamOpKind::Start), Some(1));
        assert_eq!(state_byte_offset(SteamOpKind::Stop), Some(1));
        assert_eq!(state_byte_offset(SteamOpKind::SetTemperature), Some(1));
        assert_eq!(state_byte_offset(SteamOpKind::SetDuration), Some(1));
        assert_eq!(state_byte_offset(SteamOpKind::ReadStatus), None);
        assert_eq!(state_byte_offset(SteamOpKind::ClearFaults), None);
        assert_eq!(state_byte_offset(SteamOpKind::AddressOpportunity), None);
        assert_eq!(state_byte_offset(SteamOpKind::AssignAddress), None);
    }

    #[test]
    fn the_codec_selection_is_carried_on_the_encoder() {
        let e = SteamEncoder::with_codec(&auth(), ParamCodec::SteamBlock);
        assert_eq!(e.codec(), ParamCodec::SteamBlock);
        assert_eq!(SteamEncoder::new(&auth()).codec(), ParamCodec::default());
        assert_eq!(e.link(), LinkKind::Steam);
        // No `Default` impl: a derived one would be a constructor that skips
        // the transmit gate. This is the emulator scope and it stays that way.
        assert!(!e.authority().permits_real_bus());
    }
}
