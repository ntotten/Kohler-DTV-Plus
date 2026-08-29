//! The allowlist encoder.
//!
//! [`SaturnOp`] is the complete set of frames this system can put on a valve
//! bus. Adding an operation is a visible diff in one enum and breaks
//! [`SaturnOp::ALL`]'s enumeration test.
//!
//! # What is denied, and how
//!
//! Denial is **the absence of a variant**, not a runtime check. There is no
//! `WriteCalibration`, no `FactoryReset`, no `EnterBootloader`, so no program in
//! this workspace can spell one — a reviewer confirms the denial by reading this
//! file rather than by auditing every call site, and no configuration, feature
//! flag or test harness can re-enable it.
//!
//! The full denied list is [`denied_control_bytes`](crate::saturn::denied_control_bytes),
//! and [`no_denied_control_byte_is_reachable`] scans every frame this encoder
//! can produce to prove none of them appears.
//!
//! The corresponding **reads** — calibration `0x10` and configuration `0x15` —
//! are on the allowlist and must stay there. `PH0-01` and the manual rollback
//! procedure both require reading each valve's calibration code back and diffing
//! it against the Phase 0 baseline, and after the K-99695 is powered down this
//! service is the only thing that can. `CORRECTIONS.md` item 7.
//!
//! # Evidence
//!
//! Every payload layout below is tier `[C]` and unverified on this hardware.
//! Three are worse than that and are marked `[?]` at their definitions: the
//! `0x8B` temperature payload length, the `0x99` pause state byte, and the whole
//! discovery sequence, whose printed frames are schematic rather than literal.
//!
//! [`no_denied_control_byte_is_reachable`]: self#tests

use crate::saturn::control::opcode;
use crate::saturn::frame::{
    BROADCAST, FRAME_OVERHEAD, MAX_DATA, MAX_DATA_LEN, MAX_FRAME, MasterAddr, SYNC1, SYNC2,
    ValveAddr, checksum,
};
use crate::saturn::outlets::{OutletBitmap, OutletError, OutletTable, PrimaryFlags, ValveType};
use core::fmt;
use core::marker::PhantomData;
use kdtv_units::{LinkKind, Slot, SlotSet, ValveSetpoint};

/// Where a link is in its lifecycle. Gates which operations may be encoded.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum LinkPhase {
    /// The port is open but nothing has been established. Reads only.
    Booting,
    /// Address discovery, with water off. The only phase in which address
    /// management can be encoded.
    Discovery,
    /// Addressed, idle, no outlet open.
    ReadyOff,
    /// At least one outlet open.
    Running,
    /// Outputs held.
    Paused,
    /// Latched. All-off and reads only.
    Faulted,
}

/// Authority to encode an address-management frame.
///
/// `!Clone` and `!Send`: it is not derived `Clone`, and the `PhantomData`
/// removes the auto traits, so a token cannot be stashed on another thread and
/// used after discovery ends. The only way to obtain one is [`DiscoveryToken::mint`],
/// which requires [`LinkPhase::Discovery`]. **Address clear outside discovery is
/// therefore unspellable**, not merely refused. `DENY-09`.
///
/// The engine mints one on entry to `DISCOVERY` with water off and drops it on
/// entry to `READY_OFF`.
pub struct DiscoveryToken {
    link: LinkKind,
    /// Removes `Send` and `Sync`. The `!Clone` half is the absent derive.
    _not_shared: PhantomData<*const ()>,
}

impl DiscoveryToken {
    /// The only constructor. Returns `None` in every phase but
    /// [`LinkPhase::Discovery`].
    #[must_use]
    pub fn mint(link: LinkKind, phase: LinkPhase) -> Option<Self> {
        matches!(phase, LinkPhase::Discovery).then_some(Self {
            link,
            _not_shared: PhantomData,
        })
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        self.link
    }
}

impl fmt::Debug for DiscoveryToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DiscoveryToken({})", self.link)
    }
}

/// **The allowlist.** Every frame this system can put on a valve bus.
///
/// Not a `(control, payload)` pair — an unknown opcode is an unwritable program
/// rather than a runtime rejection. `DENY-07`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SaturnOp {
    /// Close every outlet. `0x87` with an empty bitmap and no flags.
    ///
    /// The one write that is never gated on anything: turning water off is
    /// always allowed, in every phase, including a latched fault.
    AllOff,

    /// Open the named configuration slots. `0x87`.
    ///
    /// Slots are translated to the wire bitmap through the valve's
    /// [`OutletTable`], which is the only thing that knows this valve family's
    /// numbering. An unconfigured slot is refused, not skipped. `CLAMP-05`.
    ///
    /// **Operator consent is not represented here.** `ARCHITECTURE.md` § 3.3
    /// puts an `&OpenGrant` argument on this encode path, but `OpenGrant` is
    /// defined in `kdtv-safety`, which depends on `kdtv-proto` — taking it here
    /// would be a dependency cycle. The consent gate therefore sits one layer
    /// up, at the `kdtv-engine` boundary that mints the grant, and this crate
    /// records the constraint rather than enforcing it. `AGENT.md` hard rule 2.
    SetOutlets {
        slots: SlotSet,
        flags: PrimaryFlags,
    },

    /// Write the target temperature. `0x8B`.
    ///
    /// Takes [`ValveSetpoint`], not a bare `Cx2`: the clamp is in the type, so a
    /// setpoint outside 30.0–42.5 °C cannot be constructed, let alone encoded.
    /// An `Fx2` steam value does not compile here at all.
    SetTemperature(ValveSetpoint),

    /// Hold the outputs in their current state. `0x99`.
    Pause,
    /// Release a pause. `0x99`.
    Resume,

    ReadFirmwareVersion,
    ReadFirmwareType,
    ReadOutlets,
    ReadTemperature,
    ReadFlow,
    ReadFaults,
    /// `0x10`. Read-only; the `0xC0` write is denied. `CORRECTIONS.md` item 7.
    ReadCalibration,
    /// `0x15`. Read-only; the `0x95` write is denied. `CORRECTIONS.md` item 7.
    ReadConfiguration,
    ReadSerialNumber,
    ReadGenericOutlets,
    ReadExtendedStatus,
    ReadDiagnostics,

    /// Broadcast enquiry for unaddressed valves. Discovery only.
    AddressEnquiry,
    /// Allocate an address. Broadcast, because the target has none yet.
    /// Discovery only. `DISC-09`.
    AddressAllocate(ValveAddr),
    /// Clear every assigned address. Broadcast. Discovery only.
    AddressClear,
}

impl SaturnOp {
    /// Which kind this is, without its payload.
    #[must_use]
    pub const fn kind(&self) -> SaturnOpKind {
        match self {
            Self::AllOff => SaturnOpKind::AllOff,
            Self::SetOutlets { .. } => SaturnOpKind::SetOutlets,
            Self::SetTemperature(_) => SaturnOpKind::SetTemperature,
            Self::Pause => SaturnOpKind::Pause,
            Self::Resume => SaturnOpKind::Resume,
            Self::ReadFirmwareVersion => SaturnOpKind::ReadFirmwareVersion,
            Self::ReadFirmwareType => SaturnOpKind::ReadFirmwareType,
            Self::ReadOutlets => SaturnOpKind::ReadOutlets,
            Self::ReadTemperature => SaturnOpKind::ReadTemperature,
            Self::ReadFlow => SaturnOpKind::ReadFlow,
            Self::ReadFaults => SaturnOpKind::ReadFaults,
            Self::ReadCalibration => SaturnOpKind::ReadCalibration,
            Self::ReadConfiguration => SaturnOpKind::ReadConfiguration,
            Self::ReadSerialNumber => SaturnOpKind::ReadSerialNumber,
            Self::ReadGenericOutlets => SaturnOpKind::ReadGenericOutlets,
            Self::ReadExtendedStatus => SaturnOpKind::ReadExtendedStatus,
            Self::ReadDiagnostics => SaturnOpKind::ReadDiagnostics,
            Self::AddressEnquiry => SaturnOpKind::AddressEnquiry,
            Self::AddressAllocate(_) => SaturnOpKind::AddressAllocate,
            Self::AddressClear => SaturnOpKind::AddressClear,
        }
    }

    /// The complete allowlist, as kinds. Compared against a literal list in
    /// [`the_allowlist_is_exactly_these_twenty_operations`].
    ///
    /// [`the_allowlist_is_exactly_these_twenty_operations`]: self#tests
    pub const ALL: &'static [SaturnOpKind] = SaturnOpKind::ALL;
}

/// A [`SaturnOp`] without its payload — the thing tables key on.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SaturnOpKind {
    AllOff,
    SetOutlets,
    SetTemperature,
    Pause,
    Resume,
    ReadFirmwareVersion,
    ReadFirmwareType,
    ReadOutlets,
    ReadTemperature,
    ReadFlow,
    ReadFaults,
    ReadCalibration,
    ReadConfiguration,
    ReadSerialNumber,
    ReadGenericOutlets,
    ReadExtendedStatus,
    ReadDiagnostics,
    AddressEnquiry,
    AddressAllocate,
    AddressClear,
}

impl SaturnOpKind {
    pub const ALL: &'static [Self] = &[
        Self::AllOff,
        Self::SetOutlets,
        Self::SetTemperature,
        Self::Pause,
        Self::Resume,
        Self::ReadFirmwareVersion,
        Self::ReadFirmwareType,
        Self::ReadOutlets,
        Self::ReadTemperature,
        Self::ReadFlow,
        Self::ReadFaults,
        Self::ReadCalibration,
        Self::ReadConfiguration,
        Self::ReadSerialNumber,
        Self::ReadGenericOutlets,
        Self::ReadExtendedStatus,
        Self::ReadDiagnostics,
        Self::AddressEnquiry,
        Self::AddressAllocate,
        Self::AddressClear,
    ];

    /// **The explicit opcode table.** Not `read | 0x80`, which breaks for
    /// `0x10`/`0xC0`, `0x16`/`0xA1` and `0x40`/`0xA4` — and where it breaks, a
    /// derived write is a factory-calibration write. `CMD-05`.
    #[must_use]
    pub const fn control_byte(self) -> u8 {
        match self {
            Self::AllOff | Self::SetOutlets => opcode::WRITE_OUTLET_STATES,
            Self::SetTemperature => opcode::WRITE_TARGET_TEMPERATURE,
            Self::Pause | Self::Resume => opcode::WRITE_PAUSE_STATE,
            Self::ReadFirmwareVersion => opcode::READ_FIRMWARE_VERSION,
            Self::ReadFirmwareType => opcode::READ_FIRMWARE_TYPE,
            Self::ReadOutlets => opcode::READ_OUTLET_STATES,
            Self::ReadTemperature => opcode::READ_TEMPERATURE,
            Self::ReadFlow => opcode::READ_FLOW_RATE,
            Self::ReadFaults => opcode::READ_FAULT_FLAGS,
            Self::ReadCalibration => opcode::READ_CALIBRATION,
            Self::ReadConfiguration => opcode::READ_CONFIGURATION,
            Self::ReadSerialNumber => opcode::READ_SERIAL_NUMBER,
            Self::ReadGenericOutlets => opcode::READ_GENERIC_OUTLET,
            Self::ReadExtendedStatus => opcode::READ_EXTENDED_STATUS,
            Self::ReadDiagnostics => opcode::READ_DIAGNOSTICS,
            Self::AddressEnquiry | Self::AddressAllocate | Self::AddressClear => {
                opcode::ADDRESS_MANAGEMENT
            }
        }
    }

    /// True for the three operations that require a [`DiscoveryToken`].
    #[must_use]
    pub const fn is_address_management(self) -> bool {
        matches!(
            self,
            Self::AddressEnquiry | Self::AddressAllocate | Self::AddressClear
        )
    }

    /// True for the operations that can open water. `AllOff` is not one of
    /// them.
    #[must_use]
    pub const fn can_open_water(self) -> bool {
        matches!(self, Self::SetOutlets)
    }

    /// True when the operation changes valve state at all.
    #[must_use]
    pub const fn is_write(self) -> bool {
        matches!(
            self,
            Self::AllOff | Self::SetOutlets | Self::SetTemperature | Self::Pause | Self::Resume
        )
    }
}

/// The total on-wire length of the response this operation expects. `RESP-01`.
///
/// `None` for [`SaturnOpKind::AddressClear`], which is a broadcast with no
/// documented reply — the response-length table has no entry for `0x3A`/`0x03`.
/// Returning `None` rather than a plausible number keeps the decoder from
/// enforcing a length nobody wrote down.
#[must_use]
pub const fn expected_response_len(op: SaturnOpKind) -> Option<u8> {
    Some(match op {
        // Generic write ACK: control echoed, no data.
        SaturnOpKind::AllOff
        | SaturnOpKind::SetOutlets
        | SaturnOpKind::SetTemperature
        | SaturnOpKind::Pause
        | SaturnOpKind::Resume
        | SaturnOpKind::AddressAllocate => 6,
        SaturnOpKind::ReadFirmwareVersion => 9,
        SaturnOpKind::ReadFirmwareType => 7,
        SaturnOpKind::ReadOutlets
        | SaturnOpKind::ReadTemperature
        | SaturnOpKind::ReadFlow
        | SaturnOpKind::ReadFaults => 8,
        SaturnOpKind::ReadCalibration | SaturnOpKind::ReadExtendedStatus => 14,
        SaturnOpKind::ReadConfiguration | SaturnOpKind::ReadSerialNumber => 12,
        SaturnOpKind::ReadGenericOutlets => 17,
        SaturnOpKind::ReadDiagnostics => 10,
        SaturnOpKind::AddressEnquiry => 11,
        SaturnOpKind::AddressClear => return None,
    })
}

/// A frame the encoder produced, and the only thing a link is allowed to
/// transmit.
///
/// Private field, no public constructor, no `Deserialize`, no `From<Vec<u8>>`,
/// no `From<DecodedFrame>`. Every value in existence came out of
/// [`Encoder::encode`], so "what can this system transmit" is answered by
/// reading [`SaturnOp`] and nothing else. `RAW-01`.
#[derive(Clone, PartialEq, Eq)]
pub struct SaturnFrame {
    bytes: heapless::Vec<u8, MAX_FRAME>,
    op: SaturnOpKind,
    dest: u8,
}

impl SaturnFrame {
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }

    #[must_use]
    pub const fn op(&self) -> SaturnOpKind {
        self.op
    }

    /// The `ADDRESS` field: a valve address, or [`BROADCAST`] for the two
    /// broadcast operations and for address allocation.
    #[must_use]
    pub const fn dest(&self) -> u8 {
        self.dest
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The control byte, at offset 3. Used by the denied-opcode scan.
    #[must_use]
    pub fn control_byte(&self) -> u8 {
        self.op.control_byte()
    }
}

impl fmt::Debug for SaturnFrame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SaturnFrame({:?} -> 0x{:02X}, [", self.op, self.dest)?;
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
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum EncodeDenied {
    /// Address management outside [`LinkPhase::Discovery`], or without a token.
    /// `DENY-09`.
    #[error("{op:?} requires a DiscoveryToken and LinkPhase::Discovery; the link is in {phase:?}")]
    AddressOpOutsideDiscovery { op: SaturnOpKind, phase: LinkPhase },

    /// A token minted for another link. Two Saturn buses run side by side and a
    /// token is not transferable between them. `PROTO-09`.
    #[error("DiscoveryToken is for {token}, this encoder drives {encoder}")]
    TokenForWrongLink { token: LinkKind, encoder: LinkKind },

    /// A write in a phase that does not accept writes.
    #[error("{op:?} is not permitted in {phase:?}")]
    WriteOutsideOperationalPhase { op: SaturnOpKind, phase: LinkPhase },

    /// A slot this valve has not configured. `CLAMP-05` — refused, never
    /// silently dropped from the set.
    #[error("configuration slot {0} is not configured on this valve")]
    UnconfiguredOutlet(Slot),

    /// The outlet table rejected the lookup for a reason
    /// [`OutletTable::new`] should already have caught. Unreachable for a table
    /// built through that constructor, and carried rather than swallowed so a
    /// mapping bug shows up as a named refusal instead of a wrong outlet.
    #[error("outlet table: {0}")]
    OutletTable(#[from] OutletError),

    /// A bitmap built for a different valve family. The tag on
    /// [`OutletBitmap`] exists for this check.
    #[error("bitmap is for a {bitmap} valve, this encoder drives a {encoder}")]
    ValveTypeMismatch {
        bitmap: ValveType,
        encoder: ValveType,
    },

    /// A flags byte with an undefined bit set. `FLAG-01`.
    #[error("primary flags 0x{0:02X} set a bit no source defines")]
    UndefinedFlagBits(u8),

    /// The assembled payload does not fit a legal frame. Unreachable for the
    /// current allowlist — every payload is at most two bytes — and kept because
    /// the next operation added might not be. `DENY-07` / `PHY-02`.
    #[error("{op:?} produced a {len}-byte payload, above the maximum of {max}")]
    PayloadLength {
        op: SaturnOpKind,
        len: usize,
        max: u8,
    },
}

/// Builds Saturn frames for one link.
///
/// Holds the link's master identity (unresolved between `0x00` and `0x10`; see
/// [`MasterAddr`]) and its outlet table, which carries the valve family. There
/// is no path from an [`Encoder`] to a frame that is not a [`SaturnOp`].
#[derive(Clone, Debug)]
pub struct Encoder {
    link: LinkKind,
    master: MasterAddr,
    outlets: OutletTable,
}

impl Encoder {
    #[must_use]
    pub const fn new(link: LinkKind, master: MasterAddr, outlets: OutletTable) -> Self {
        Self {
            link,
            master,
            outlets,
        }
    }

    #[must_use]
    pub const fn link(&self) -> LinkKind {
        self.link
    }

    /// The master identity this link speaks as. Configuration, because the
    /// sources contradict each other — see [`MasterAddr`].
    #[must_use]
    pub const fn master(&self) -> MasterAddr {
        self.master
    }

    #[must_use]
    pub const fn outlets(&self) -> &OutletTable {
        &self.outlets
    }

    #[must_use]
    pub const fn valve(&self) -> ValveType {
        self.outlets.valve()
    }

    /// Encodes one allowlisted operation.
    ///
    /// `dst` is the destination for every addressed operation. The three address
    /// operations ignore it and go to [`BROADCAST`]: enquiry and clear are
    /// broadcasts by definition, and allocation is too, because the valve being
    /// addressed has no address yet. `DISC-09`.
    pub fn encode(
        &self,
        target: ValveAddr,
        op: &SaturnOp,
        phase: LinkPhase,
        disco: Option<&DiscoveryToken>,
    ) -> Result<SaturnFrame, EncodeDenied> {
        let kind = op.kind();

        if kind.is_address_management() {
            let Some(token) = disco else {
                return Err(EncodeDenied::AddressOpOutsideDiscovery { op: kind, phase });
            };
            if token.link() != self.link {
                return Err(EncodeDenied::TokenForWrongLink {
                    token: token.link(),
                    encoder: self.link,
                });
            }
            // Belt and braces: a token can only be minted in Discovery, and the
            // phase is checked again here so a token held across a transition
            // is still refused.
            if phase != LinkPhase::Discovery {
                return Err(EncodeDenied::AddressOpOutsideDiscovery { op: kind, phase });
            }
        }

        // Writes that change flow are refused before the link is addressed and
        // during discovery, when water must be off. `AllOff` is exempt: closing
        // a valve is allowed in every phase, always.
        if kind.is_write()
            && kind != SaturnOpKind::AllOff
            && matches!(phase, LinkPhase::Booting | LinkPhase::Discovery)
        {
            return Err(EncodeDenied::WriteOutsideOperationalPhase { op: kind, phase });
        }

        let (dest, data) = self.payload(target, op)?;
        Self::assemble(dest, kind, &data)
    }

    /// The destination and `DATA` field for an operation.
    fn payload(
        &self,
        target: ValveAddr,
        op: &SaturnOp,
    ) -> Result<(u8, heapless::Vec<u8, MAX_DATA>), EncodeDenied> {
        let mut data: heapless::Vec<u8, MAX_DATA> = heapless::Vec::new();
        let dest = match op {
            SaturnOp::AllOff => {
                // 0x87 with an empty bitmap and no flags. OUT-03: DATA_LEN 2,
                // [bitmap, flags].
                push(&mut data, op.kind(), 0x00)?;
                push(&mut data, op.kind(), PrimaryFlags::CAPTURED.bits())?;
                target.get()
            }
            SaturnOp::SetOutlets { slots, flags } => {
                if flags.bits() & PrimaryFlags::UNDEFINED_BITS != 0 {
                    return Err(EncodeDenied::UndefinedFlagBits(flags.bits()));
                }
                let bitmap: OutletBitmap = self.outlets.bitmap(*slots).map_err(|e| match e {
                    // The common case, and the one a caller can act on: the
                    // operator asked for a slot this valve does not have.
                    OutletError::UnconfiguredSlot(s) => EncodeDenied::UnconfiguredOutlet(s),
                    // Everything else is caught when the table is built, so
                    // reaching here means the table and the encoder disagree.
                    other => EncodeDenied::OutletTable(other),
                })?;
                if bitmap.valve() != self.valve() {
                    return Err(EncodeDenied::ValveTypeMismatch {
                        bitmap: bitmap.valve(),
                        encoder: self.valve(),
                    });
                }
                push(&mut data, op.kind(), bitmap.bits())?;
                push(&mut data, op.kind(), flags.bits())?;
                target.get()
            }
            SaturnOp::SetTemperature(sp) => {
                // `[?]` No source states the payload length of 0x8B. Cx2 is one
                // byte and covers the whole 0–127.5 °C range, so one byte is
                // encoded here. The two-byte alternative is live until the
                // Phase 1 capture settles it; see the module docs.
                push(&mut data, op.kind(), sp.wire().raw())?;
                target.get()
            }
            SaturnOp::Pause | SaturnOp::Resume => {
                // `[?]` No source gives the 0x99 payload. The valve state bit
                // for Pause is 0x02 (`FLAG-03`), so that is what is written,
                // and 0x00 releases it. Packet-capture question, not a fact.
                let state = if matches!(op, SaturnOp::Pause) {
                    0x02
                } else {
                    0x00
                };
                push(&mut data, op.kind(), state)?;
                target.get()
            }
            SaturnOp::AddressEnquiry => {
                push(&mut data, op.kind(), opcode::SUB_ENQUIRY)?;
                BROADCAST
            }
            SaturnOp::AddressAllocate(addr) => {
                // DATA_LEN 2, [subcommand, new address]. The source prints
                // DATA_LEN 1 with two data bytes following, which is internally
                // inconsistent: under its own `total = 6 + DATA_LEN` rule the
                // frame would be seven bytes and the address would be lost.
                // Encoded from the field definitions, not from the diagram.
                push(&mut data, op.kind(), opcode::SUB_ALLOCATE)?;
                push(&mut data, op.kind(), addr.get())?;
                BROADCAST
            }
            SaturnOp::AddressClear => {
                push(&mut data, op.kind(), opcode::SUB_CLEAR)?;
                BROADCAST
            }
            SaturnOp::ReadFirmwareVersion
            | SaturnOp::ReadFirmwareType
            | SaturnOp::ReadOutlets
            | SaturnOp::ReadTemperature
            | SaturnOp::ReadFlow
            | SaturnOp::ReadFaults
            | SaturnOp::ReadCalibration
            | SaturnOp::ReadConfiguration
            | SaturnOp::ReadSerialNumber
            | SaturnOp::ReadGenericOutlets
            | SaturnOp::ReadExtendedStatus
            | SaturnOp::ReadDiagnostics => target.get(),
        };
        Ok((dest, data))
    }

    fn assemble(dest: u8, kind: SaturnOpKind, data: &[u8]) -> Result<SaturnFrame, EncodeDenied> {
        let Ok(data_len) = u8::try_from(data.len()) else {
            return Err(EncodeDenied::PayloadLength {
                op: kind,
                len: data.len(),
                max: MAX_DATA_LEN,
            });
        };
        if data_len > MAX_DATA_LEN {
            return Err(EncodeDenied::PayloadLength {
                op: kind,
                len: data.len(),
                max: MAX_DATA_LEN,
            });
        }
        let control = kind.control_byte();
        let mut bytes: heapless::Vec<u8, MAX_FRAME> = heapless::Vec::new();
        let too_long = || EncodeDenied::PayloadLength {
            op: kind,
            len: data.len(),
            max: MAX_DATA_LEN,
        };
        bytes
            .extend_from_slice(&[SYNC1, SYNC2, dest, control, data_len])
            .map_err(|_| too_long())?;
        bytes.extend_from_slice(data).map_err(|_| too_long())?;
        bytes
            .push(checksum(dest, control, data_len, data))
            .map_err(|_| too_long())?;
        debug_assert_eq!(bytes.len(), FRAME_OVERHEAD + data.len());
        Ok(SaturnFrame {
            bytes,
            op: kind,
            dest,
        })
    }
}

fn push(
    data: &mut heapless::Vec<u8, MAX_DATA>,
    op: SaturnOpKind,
    b: u8,
) -> Result<(), EncodeDenied> {
    data.push(b).map_err(|_| EncodeDenied::PayloadLength {
        op,
        len: usize::from(MAX_DATA_LEN) + 1,
        max: MAX_DATA_LEN,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::saturn::control::denied_control_bytes;
    use crate::saturn::outlets::OutletMapping;
    use kdtv_units::{Cx2, ZoneId};

    fn slot(n: u8) -> Slot {
        Slot::new(n).unwrap()
    }

    fn addr(v: u8) -> ValveAddr {
        ValveAddr::new(v).unwrap()
    }

    /// Zone 1: the six-port valve, five configured outlets, identity mapping.
    fn zone1() -> Encoder {
        let table = OutletTable::new(
            ValveType::Dtv6Port,
            (1u8..=5).map(|n| OutletMapping {
                slot: slot(n),
                status_index: n,
                wire_outlet: n - 1,
            }),
        )
        .unwrap();
        Encoder::new(LinkKind::Zone(ZoneId::Zone1), MasterAddr::Dtv, table)
    }

    /// Zone 2: the three-port Prompt valve. Its first outlet is mask `0x04`.
    fn zone2() -> Encoder {
        let table = OutletTable::new(
            ValveType::Prompt3Port,
            (1u8..=3).map(|n| OutletMapping {
                slot: slot(n),
                status_index: n,
                wire_outlet: n,
            }),
        )
        .unwrap();
        Encoder::new(LinkKind::Zone(ZoneId::Zone2), MasterAddr::Dtv, table)
    }

    fn token(link: LinkKind) -> DiscoveryToken {
        DiscoveryToken::mint(link, LinkPhase::Discovery).unwrap()
    }

    fn set(ns: &[u8]) -> SlotSet {
        ns.iter().map(|n| slot(*n)).collect()
    }

    // ---- Golden frames -----------------------------------------------------

    /// `saturn-protocol.md` Example 2, the only captured outlet-open frame.
    /// Turning on Prompt 3 outlet 1, mask `0x04`, with `DATA[1]` = `0x00`.
    /// `OUT-03` / `OUT-04`.
    #[test]
    fn golden_write_outlet_states_prompt3_outlet_1() {
        let f = zone2()
            .encode(
                addr(0x03),
                &SaturnOp::SetOutlets {
                    slots: set(&[1]),
                    flags: PrimaryFlags::CAPTURED,
                },
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        assert_eq!(f.bytes(), &[0xAA, 0x55, 0x03, 0x87, 0x02, 0x04, 0x00, 0x70]);
    }

    /// `saturn-protocol.md` Example 1, with its own correction applied.
    /// `CHK-04`: the stale `0xA1` must never appear.
    #[test]
    fn golden_read_firmware_type() {
        let f = zone1()
            .encode(
                addr(0x03),
                &SaturnOp::ReadFirmwareType,
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        assert_eq!(f.bytes(), &[0xAA, 0x55, 0x03, 0x02, 0x00, 0xFB]);
        assert_ne!(f.bytes().last(), Some(&0xA1));
    }

    /// `saturn-protocol.md` Example 3, corrected. `CHK-04`: not `0xAE`.
    #[test]
    fn golden_address_clear_broadcast() {
        let e = zone1();
        let t = token(e.link());
        let f = e
            .encode(
                addr(0x03),
                &SaturnOp::AddressClear,
                LinkPhase::Discovery,
                Some(&t),
            )
            .unwrap();
        assert_eq!(f.bytes(), &[0xAA, 0x55, 0x0F, 0x3A, 0x01, 0x03, 0xB3]);
        assert_ne!(f.bytes().last(), Some(&0xAE));
        assert_eq!(f.dest(), BROADCAST);
    }

    #[test]
    fn golden_reads_from_valve_0x03() {
        let e = zone1();
        for (op, expected) in [
            (
                SaturnOp::ReadFirmwareVersion,
                &[0xAA, 0x55, 0x03, 0x01, 0x00, 0xFC][..],
            ),
            (
                SaturnOp::ReadTemperature,
                &[0xAA, 0x55, 0x03, 0x0B, 0x00, 0xF2][..],
            ),
            (
                SaturnOp::ReadFaults,
                &[0xAA, 0x55, 0x03, 0x0F, 0x00, 0xEE][..],
            ),
        ] {
            let f = e
                .encode(addr(0x03), &op, LinkPhase::ReadyOff, None)
                .unwrap();
            assert_eq!(f.bytes(), expected, "{op:?}");
        }
    }

    /// **The discovery frames are encoded, not quoted.**
    ///
    /// `saturn-protocol.md`'s four-step sequence diagram prints `DATA_LEN`
    /// values that disagree with the data bytes it shows on the same line: the
    /// enquiry reply is `len=5` with three bytes named, and the allocation is
    /// `len=1` with two bytes following. The `..` placeholders in those lines
    /// mean the diagram is schematic. These frames are therefore built from the
    /// field definitions and their checksums computed, and they are
    /// **unresolved pending Phase 1 capture question 2** — not golden fixtures.
    #[test]
    fn discovery_frames_are_derived_from_field_definitions_and_remain_unresolved() {
        let e = zone1();
        let t = token(e.link());

        let enquiry = e
            .encode(
                addr(0x03),
                &SaturnOp::AddressEnquiry,
                LinkPhase::Discovery,
                Some(&t),
            )
            .unwrap();
        assert_eq!(enquiry.bytes(), &[0xAA, 0x55, 0x0F, 0x3A, 0x01, 0x01, 0xB5]);

        let allocate = e
            .encode(
                addr(0x03),
                &SaturnOp::AddressAllocate(addr(0x03)),
                LinkPhase::Discovery,
                Some(&t),
            )
            .unwrap();
        // DATA_LEN 0x02, not the diagram's 0x01: with DATA_LEN 1 the frame is
        // seven bytes and carries the subcommand but not the address, which is
        // untenable. The alternate seven-byte reading has checksum 0xB4 and is
        // recorded as the fallback to try at commissioning if allocation is
        // refused.
        assert_eq!(
            allocate.bytes(),
            &[0xAA, 0x55, 0x0F, 0x3A, 0x02, 0x02, 0x03, 0xB0]
        );
        assert_eq!(allocate.bytes().get(4), Some(&0x02));
        assert_eq!(checksum(0x0F, 0x3A, 0x01, &[0x02]), 0xB4);

        // Every discovery frame is addressed to the broadcast address. DISC-09.
        for f in [&enquiry, &allocate] {
            assert_eq!(f.dest(), BROADCAST);
        }
    }

    /// `FRAME-02` / `CHK-01`. Every frame the encoder emits is self-consistent:
    /// the length formula holds and the covered bytes sum to zero.
    #[test]
    fn every_encoded_frame_is_well_formed() {
        for (e, valve_slots) in [(zone1(), &[1u8, 2, 3, 4, 5][..]), (zone2(), &[1, 2, 3][..])] {
            let t = token(e.link());
            for op in every_op(valve_slots) {
                for phase in [
                    LinkPhase::Booting,
                    LinkPhase::Discovery,
                    LinkPhase::ReadyOff,
                    LinkPhase::Running,
                    LinkPhase::Paused,
                    LinkPhase::Faulted,
                ] {
                    let Ok(f) = e.encode(addr(0x03), &op, phase, Some(&t)) else {
                        continue;
                    };
                    let b = f.bytes();
                    assert_eq!(b.first(), Some(&SYNC1));
                    assert_eq!(b.get(1), Some(&SYNC2));
                    let data_len = usize::from(*b.get(4).unwrap());
                    assert_eq!(b.len(), FRAME_OVERHEAD + data_len);
                    assert!(b.len() <= MAX_FRAME);
                    assert!(data_len <= usize::from(MAX_DATA_LEN));
                    let sum = b
                        .iter()
                        .skip(2)
                        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
                    assert_eq!(sum, 0, "checksum does not close for {op:?}: {f:?}");
                }
            }
        }
    }

    /// Every operation, with a representative payload for each.
    fn every_op(slots: &[u8]) -> Vec<SaturnOp> {
        let mut ops = vec![
            SaturnOp::AllOff,
            SaturnOp::Pause,
            SaturnOp::Resume,
            SaturnOp::ReadFirmwareVersion,
            SaturnOp::ReadFirmwareType,
            SaturnOp::ReadOutlets,
            SaturnOp::ReadTemperature,
            SaturnOp::ReadFlow,
            SaturnOp::ReadFaults,
            SaturnOp::ReadCalibration,
            SaturnOp::ReadConfiguration,
            SaturnOp::ReadSerialNumber,
            SaturnOp::ReadGenericOutlets,
            SaturnOp::ReadExtendedStatus,
            SaturnOp::ReadDiagnostics,
            SaturnOp::AddressEnquiry,
            SaturnOp::AddressClear,
        ];
        for a in ValveAddr::ALL {
            ops.push(SaturnOp::AddressAllocate(a));
        }
        // Every setpoint the clamp admits.
        for raw in 0u8..=255 {
            if let Ok(sp) = ValveSetpoint::try_new(Cx2::from_raw(raw)) {
                ops.push(SaturnOp::SetTemperature(sp));
            }
        }
        // Every subset of the configured slots, with every legal flag byte.
        let n = slots.len();
        for mask in 0u32..(1u32 << n) {
            let chosen: SlotSet = slots
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, s)| slot(*s))
                .collect();
            for fb in 0u8..=255 {
                if let Some(flags) = PrimaryFlags::from_bits(fb) {
                    ops.push(SaturnOp::SetOutlets {
                        slots: chosen,
                        flags,
                    });
                }
            }
        }
        ops
    }

    // ---- The allowlist -----------------------------------------------------

    /// The enumeration test. Adding an operation must change this literal list.
    #[test]
    fn the_allowlist_is_exactly_these_twenty_operations() {
        let expected = [
            SaturnOpKind::AllOff,
            SaturnOpKind::SetOutlets,
            SaturnOpKind::SetTemperature,
            SaturnOpKind::Pause,
            SaturnOpKind::Resume,
            SaturnOpKind::ReadFirmwareVersion,
            SaturnOpKind::ReadFirmwareType,
            SaturnOpKind::ReadOutlets,
            SaturnOpKind::ReadTemperature,
            SaturnOpKind::ReadFlow,
            SaturnOpKind::ReadFaults,
            SaturnOpKind::ReadCalibration,
            SaturnOpKind::ReadConfiguration,
            SaturnOpKind::ReadSerialNumber,
            SaturnOpKind::ReadGenericOutlets,
            SaturnOpKind::ReadExtendedStatus,
            SaturnOpKind::ReadDiagnostics,
            SaturnOpKind::AddressEnquiry,
            SaturnOpKind::AddressAllocate,
            SaturnOpKind::AddressClear,
        ];
        assert_eq!(SaturnOp::ALL, &expected[..]);
        assert_eq!(SaturnOp::ALL.len(), 20);

        // Five writes, twelve reads, three address operations.
        assert_eq!(SaturnOp::ALL.iter().filter(|k| k.is_write()).count(), 5);
        assert_eq!(
            SaturnOp::ALL
                .iter()
                .filter(|k| k.is_address_management())
                .count(),
            3
        );
        assert_eq!(
            SaturnOp::ALL
                .iter()
                .filter(|k| !k.is_write() && !k.is_address_management())
                .count(),
            12
        );
        // Exactly one operation can open water, and it is not AllOff.
        assert_eq!(
            SaturnOp::ALL.iter().filter(|k| k.can_open_water()).count(),
            1
        );
        assert!(!SaturnOpKind::AllOff.can_open_water());
    }

    /// **The denied-opcode scan.**
    ///
    /// Builds every frame the encoder can emit — both valve families, every
    /// operation, every legal setpoint, every subset of configured slots, every
    /// legal flag byte, every valve address — and asserts the control byte is
    /// never one of the ten denied opcodes.
    ///
    /// The scan is over the **control-byte field**, not over all of a frame's
    /// bytes. That is deliberate and worth stating: a checksum or an outlet
    /// bitmap can legitimately equal a denied opcode. Prompt 3 outlets
    /// 1, 3, 4, 5 and 6 OR together to `0xF4`, which is also the factory-reset
    /// opcode; a whole-frame byte scan would fail on a correct frame and would
    /// have to be weakened until it proved nothing. The control byte is the
    /// field that selects the operation, and it is the field that matters.
    #[test]
    fn no_denied_control_byte_is_reachable() {
        let denied = denied_control_bytes();
        let mut frames = 0u32;
        for (e, valve_slots) in [(zone1(), &[1u8, 2, 3, 4, 5][..]), (zone2(), &[1, 2, 3][..])] {
            let t = token(e.link());
            for op in every_op(valve_slots) {
                for phase in [
                    LinkPhase::Discovery,
                    LinkPhase::ReadyOff,
                    LinkPhase::Running,
                    LinkPhase::Paused,
                    LinkPhase::Faulted,
                ] {
                    for a in ValveAddr::ALL {
                        let Ok(f) = e.encode(a, &op, phase, Some(&t)) else {
                            continue;
                        };
                        frames += 1;
                        let control = *f.bytes().get(3).expect("frames have a control byte");
                        assert_eq!(control, f.control_byte());
                        assert!(
                            !denied.contains(&control),
                            "denied control byte 0x{control:02X} emitted for {op:?}"
                        );
                    }
                }
            }
        }
        // A scan that encoded nothing would pass vacuously.
        assert!(frames > 10_000, "the scan only built {frames} frames");
    }

    /// The complement: the control bytes the allowlist *can* emit, as a set,
    /// disjoint from the denied set.
    #[test]
    fn the_reachable_control_bytes_are_a_known_five() {
        let mut reachable: Vec<u8> = SaturnOp::ALL.iter().map(|k| k.control_byte()).collect();
        reachable.sort_unstable();
        reachable.dedup();
        assert_eq!(
            reachable,
            vec![
                0x01, 0x02, 0x07, 0x0B, 0x0C, 0x0F, 0x10, 0x11, 0x15, 0x16, 0x3A, 0x40, 0x54, 0x87,
                0x8B, 0x99
            ]
        );
        for d in denied_control_bytes() {
            assert!(!reachable.contains(d), "0x{d:02X} is reachable");
        }
        // The named writes, spelled out.
        for byte in [0xC0u8, 0x95, 0x81, 0x82, 0xA4, 0xA1, 0xF4, 0xF6, 0xF7, 0x8C] {
            assert!(!reachable.contains(&byte), "0x{byte:02X} is reachable");
        }
    }

    // ---- The discovery token ----------------------------------------------

    /// `DENY-09`. Address clear outside discovery has no spelling: there is no
    /// token to pass, and passing none is refused.
    #[test]
    fn address_management_requires_a_token_and_the_discovery_phase() {
        let e = zone1();
        for phase in [
            LinkPhase::Booting,
            LinkPhase::ReadyOff,
            LinkPhase::Running,
            LinkPhase::Paused,
            LinkPhase::Faulted,
        ] {
            // No token can be minted outside Discovery at all.
            assert!(DiscoveryToken::mint(e.link(), phase).is_none());
            // And without one, every address operation is refused.
            for op in [
                SaturnOp::AddressClear,
                SaturnOp::AddressEnquiry,
                SaturnOp::AddressAllocate(addr(0x03)),
            ] {
                let err = e.encode(addr(0x03), &op, phase, None).unwrap_err();
                assert!(matches!(
                    err,
                    EncodeDenied::AddressOpOutsideDiscovery { .. }
                ));
            }
        }

        // A token held across a phase transition is still refused.
        let t = token(e.link());
        let err = e
            .encode(
                addr(0x03),
                &SaturnOp::AddressClear,
                LinkPhase::Running,
                Some(&t),
            )
            .unwrap_err();
        assert!(matches!(
            err,
            EncodeDenied::AddressOpOutsideDiscovery { .. }
        ));
    }

    /// `PROTO-09`. Two buses, two tokens, no crossing.
    #[test]
    fn a_token_is_not_transferable_between_links() {
        let e = zone1();
        let other = token(LinkKind::Zone(ZoneId::Zone2));
        let err = e
            .encode(
                addr(0x03),
                &SaturnOp::AddressClear,
                LinkPhase::Discovery,
                Some(&other),
            )
            .unwrap_err();
        assert_eq!(
            err,
            EncodeDenied::TokenForWrongLink {
                token: LinkKind::Zone(ZoneId::Zone2),
                encoder: LinkKind::Zone(ZoneId::Zone1),
            }
        );
    }

    // ---- Phase gating ------------------------------------------------------

    #[test]
    fn all_off_is_permitted_in_every_phase_and_needs_nothing() {
        let e = zone1();
        for phase in [
            LinkPhase::Booting,
            LinkPhase::Discovery,
            LinkPhase::ReadyOff,
            LinkPhase::Running,
            LinkPhase::Paused,
            LinkPhase::Faulted,
        ] {
            let f = e
                .encode(addr(0x03), &SaturnOp::AllOff, phase, None)
                .unwrap();
            // DATA_LEN 2, empty bitmap, no flags.
            assert_eq!(f.bytes().get(4..7), Some(&[0x02, 0x00, 0x00][..]));
        }
    }

    #[test]
    fn flow_changing_writes_are_refused_before_the_link_is_addressed() {
        let e = zone1();
        for phase in [LinkPhase::Booting, LinkPhase::Discovery] {
            for op in [
                SaturnOp::SetOutlets {
                    slots: set(&[1]),
                    flags: PrimaryFlags::CAPTURED,
                },
                SaturnOp::SetTemperature(ValveSetpoint::try_new(Cx2::from_raw(76)).unwrap()),
                SaturnOp::Pause,
                SaturnOp::Resume,
            ] {
                let err = e.encode(addr(0x03), &op, phase, None).unwrap_err();
                assert!(matches!(
                    err,
                    EncodeDenied::WriteOutsideOperationalPhase { .. }
                ));
            }
        }
    }

    #[test]
    fn reads_are_permitted_in_every_phase() {
        let e = zone1();
        for phase in [
            LinkPhase::Booting,
            LinkPhase::Discovery,
            LinkPhase::ReadyOff,
            LinkPhase::Running,
            LinkPhase::Paused,
            LinkPhase::Faulted,
        ] {
            for op in [
                SaturnOp::ReadFaults,
                SaturnOp::ReadCalibration,
                SaturnOp::ReadConfiguration,
                SaturnOp::ReadTemperature,
            ] {
                assert!(e.encode(addr(0x03), &op, phase, None).is_ok());
            }
        }
    }

    // ---- Payload rules -----------------------------------------------------

    /// `CLAMP-05`. An unconfigured slot is refused by name, not dropped.
    #[test]
    fn an_unconfigured_slot_is_refused() {
        let e = zone1();
        let err = e
            .encode(
                addr(0x03),
                &SaturnOp::SetOutlets {
                    slots: set(&[6]),
                    flags: PrimaryFlags::CAPTURED,
                },
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap_err();
        assert_eq!(err, EncodeDenied::UnconfiguredOutlet(slot(6)));

        // And a partly valid set is refused whole rather than half started.
        let err = e
            .encode(
                addr(0x03),
                &SaturnOp::SetOutlets {
                    slots: set(&[1, 6]),
                    flags: PrimaryFlags::CAPTURED,
                },
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap_err();
        assert_eq!(err, EncodeDenied::UnconfiguredOutlet(slot(6)));
    }

    /// The two families select different bitmaps for the same slot number.
    #[test]
    fn the_same_slot_reaches_a_different_wire_bit_on_each_valve() {
        let one = zone1()
            .encode(
                addr(0x03),
                &SaturnOp::SetOutlets {
                    slots: set(&[1]),
                    flags: PrimaryFlags::CAPTURED,
                },
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        let two = zone2()
            .encode(
                addr(0x03),
                &SaturnOp::SetOutlets {
                    slots: set(&[1]),
                    flags: PrimaryFlags::CAPTURED,
                },
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap();
        assert_eq!(one.bytes().get(5), Some(&0x01));
        assert_eq!(two.bytes().get(5), Some(&0x04));
    }

    /// `TEMP-02` / `TEMP-03`. The clamp is in the type, so the encoder cannot
    /// emit a setpoint outside it — and the emitted byte is the clamped Cx2.
    #[test]
    fn only_clamped_setpoints_can_be_encoded() {
        let e = zone1();
        let mut emitted = Vec::new();
        for raw in 0u8..=255 {
            let Ok(sp) = ValveSetpoint::try_new(Cx2::from_raw(raw)) else {
                continue;
            };
            let f = e
                .encode(
                    addr(0x03),
                    &SaturnOp::SetTemperature(sp),
                    LinkPhase::Running,
                    None,
                )
                .unwrap();
            let byte = *f.bytes().get(5).unwrap();
            assert_eq!(byte, raw);
            emitted.push(byte);
        }
        // 60..=85 inclusive: the floor is MIN_SYS_VALVE_TEMP and the ceiling is
        // the 109 F user-facing limit rounded down to the 0.5 C step below it.
        assert_eq!(emitted.first(), Some(&60));
        assert_eq!(emitted.last(), Some(&85));
        assert_eq!(emitted.len(), 26);
        // 42.5 C, not the valve's own 49 C hardware ceiling.
        assert!(!emitted.contains(&98));
        assert!(!emitted.contains(&90));
    }

    /// `FLAG-01`. An undefined flag bit cannot be constructed, and if one is
    /// forced in it is refused rather than written.
    #[test]
    fn undefined_flag_bits_never_reach_the_wire() {
        let e = zone1();
        // from_bits refuses them outright.
        assert!(PrimaryFlags::from_bits(0x08).is_none());
        // from_bits_retain is the only way to build one, and the encoder stops
        // it.
        let bad = PrimaryFlags::from_bits_retain(0x08);
        let err = e
            .encode(
                addr(0x03),
                &SaturnOp::SetOutlets {
                    slots: set(&[1]),
                    flags: bad,
                },
                LinkPhase::ReadyOff,
                None,
            )
            .unwrap_err();
        assert_eq!(err, EncodeDenied::UndefinedFlagBits(0x08));
    }

    // ---- Response lengths --------------------------------------------------

    /// `RESP-01`. Every entry satisfies `total = 6 + DATA_LEN` and fits the
    /// 20-byte maximum.
    #[test]
    fn expected_response_lengths_match_the_documented_table() {
        let table = [
            (SaturnOpKind::AddressEnquiry, Some(11u8)),
            (SaturnOpKind::AddressAllocate, Some(6)),
            (SaturnOpKind::AddressClear, None),
            (SaturnOpKind::ReadFirmwareVersion, Some(9)),
            (SaturnOpKind::ReadFirmwareType, Some(7)),
            (SaturnOpKind::ReadOutlets, Some(8)),
            (SaturnOpKind::ReadTemperature, Some(8)),
            (SaturnOpKind::ReadFlow, Some(8)),
            (SaturnOpKind::ReadFaults, Some(8)),
            (SaturnOpKind::ReadCalibration, Some(14)),
            (SaturnOpKind::ReadSerialNumber, Some(12)),
            (SaturnOpKind::ReadConfiguration, Some(12)),
            (SaturnOpKind::ReadGenericOutlets, Some(17)),
            (SaturnOpKind::ReadExtendedStatus, Some(14)),
            (SaturnOpKind::ReadDiagnostics, Some(10)),
            (SaturnOpKind::AllOff, Some(6)),
            (SaturnOpKind::SetOutlets, Some(6)),
            (SaturnOpKind::SetTemperature, Some(6)),
            (SaturnOpKind::Pause, Some(6)),
            (SaturnOpKind::Resume, Some(6)),
        ];
        assert_eq!(table.len(), SaturnOp::ALL.len());
        for (op, expected) in table {
            assert_eq!(expected_response_len(op), expected, "{op:?}");
            if let Some(total) = expected {
                assert!(total >= 6, "{op:?} is shorter than the frame overhead");
                assert!(usize::from(total) <= MAX_FRAME, "{op:?} exceeds 20 bytes");
                let data_len = total - 6;
                assert!(data_len <= MAX_DATA_LEN, "{op:?} needs DATA_LEN {data_len}");
            }
        }
    }

    /// The broadcast clear has no documented reply, and the codec says so
    /// rather than inventing a length.
    #[test]
    fn address_clear_expects_no_response() {
        assert_eq!(expected_response_len(SaturnOpKind::AddressClear), None);
    }

    /// The frame reports the same control byte as its opcode table, so the
    /// denied scan and the decoder cannot drift apart.
    #[test]
    fn the_control_byte_in_the_frame_is_the_control_byte_in_the_table() {
        let e = zone1();
        let t = token(e.link());
        for op in every_op(&[1, 2, 3, 4, 5]) {
            for phase in [LinkPhase::Discovery, LinkPhase::Running] {
                if let Ok(f) = e.encode(addr(0x03), &op, phase, Some(&t)) {
                    assert_eq!(f.bytes().get(3), Some(&op.kind().control_byte()));
                    assert_eq!(f.op(), op.kind());
                }
            }
        }
    }
}
