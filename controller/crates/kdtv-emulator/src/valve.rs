//! An emulated Saturn mixing valve.
//!
//! Two are installed: a DTV 6-Port on zone 1 and a Prompt 3-Port on zone 2. One
//! model covers both, because the difference that matters is the wire bitmap —
//! mask `0x04` is outlet 2 on the DTV and outlet 1 on the Prompt — and that
//! difference already lives in [`ValveType`], which this model uses rather than
//! reimplements.
//!
//! # Evidence
//!
//! **Nothing here has been verified against a real valve.** Every frame this
//! model produces is tier `[C]`: assembled from
//! `research/xagon0/docs/protocols/saturn-protocol.md` and the local
//! `docs/devices/valve-control.md`, which disagree in several places. A green
//! test run proves this model and `kdtv-proto` agree with each other. That is
//! internal consistency with the specification, not evidence about the valve.
//!
//! # Why this model builds frames from raw bytes
//!
//! `kdtv-proto`'s encoder is an allowlist: by design it cannot produce a
//! malformed frame, a denied opcode, or a response at all — it only speaks
//! master to valve. A device model has to do all three, because what the daemon
//! does when a valve answers with rubbish is exactly what needs testing. So this
//! module assembles replies with [`raw_saturn`] and its variants and uses
//! `kdtv-proto` only to *decode* what the daemon sent. That asymmetry is why
//! this crate is kept out of the daemon's dependency graph.
//!
//! # What is deliberately configurable
//!
//! Three behaviours are contested in the sources or open in the investigation
//! log, so this model plumbs both readings and picks neither:
//!
//! | Behaviour | Where | Why |
//! | --- | --- | --- |
//! | Communication-loss shutdown | [`SaturnValveModel::with_comms_loss_shutdown`] | `None` models a valve that does **not** close, which is what proves the service's own fail-off path independently of the valve's. `CORRECTIONS.md` item 11 |
//! | What refreshes the Prompt 3 runtime timer | [`TimerRefresh`] | "any valid command" against "a deliberate refresh only" — packet-capture question 5, `CORRECTIONS.md` item 5 |
//! | Purge after an off acknowledgement | [`SaturnValveModel::with_purge`] | `INVESTIGATIONS.md` I4 is open. Off by default |

use crate::wire::DeviceModel;
use kdtv_proto::saturn::{
    BROADCAST, DecodedFrame, Direction, Expectation, FRAME_OVERHEAD, MasterAddr, RX_CAPACITY,
    RawErrorByte, RxBuffer, SYNC, ValveAddr, ValveStateBits, ValveType, checksum, decode, opcode,
};
use kdtv_units::Cx2;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The Prompt 3 maximum runtime, `PROMPT3_TIMEOUT_MAX`. Tier `[C]`, and every
/// source that mentions it agrees on the number.
pub const PROMPT3_TIMEOUT_MAX: Duration = Duration::from_secs(1800);

/// The window in which the stock controller is documented to accept a timer
/// refresh: only once at least this much of the runtime has elapsed.
///
/// This master never sends a refresh — there is no `SaturnOp` for one, and
/// `CORRECTIONS.md` item 5 forbids counting the valve timer as a backstop — so
/// the constant is here to be read, not used.
pub const PROMPT3_REFRESH_FLOOR: Duration = Duration::from_secs(900);

/// The error byte `docs/devices/valve-control.md` calls `WELDED`.
///
/// **Only under that table.** `saturn-protocol.md` puts 35 in a reserved
/// calibration block instead, which is why [`SaturnValveModel::weld`] names the
/// table it applies rather than deriving mechanics from the byte.
/// `CORRECTIONS.md` item 4.
pub const WELDED_CODE: u8 = 35;

/// Which byte order this valve reports two-byte fields in.
///
/// **No source states the endianness of any multi-byte Saturn field.**
/// `RESP-05`. `kdtv-proto` carries both readings out of a decoded frame, so a
/// model that only ever emitted one would leave half the decoder untested. The
/// default is arbitrary and is not a claim.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Default)]
pub enum TwoByteOrder {
    /// High byte first.
    #[default]
    BigEndian,
    /// Low byte first.
    LittleEndian,
}

impl TwoByteOrder {
    fn bytes(self, v: u16) -> [u8; 2] {
        match self {
            Self::BigEndian => v.to_be_bytes(),
            Self::LittleEndian => v.to_le_bytes(),
        }
    }
}

/// What refreshes the Prompt 3 runtime timer.
///
/// **Unresolved, and this enum has no `Default` on purpose.** Packet-capture
/// question 5. `saturn-protocol.md` § Prompt 3 Timeout describes the countdown
/// restarting on communication; `docs/devices/valve-control.md` describes an
/// explicit `send_timer_reset()` accepted only below
/// [`PROMPT3_REFRESH_FLOOR`]. Building a Prompt 3 model therefore requires
/// naming which behaviour is being modelled, and the two produce opposite
/// outcomes for the same polling — see the tests.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum TimerRefresh {
    /// Every frame the valve accepts restarts the countdown, so ordinary
    /// polling holds the valve open indefinitely.
    AnyValidCommand,
    /// Only a deliberate refresh restarts it. This master sends none, so under
    /// this reading the valve closes 1800 s after the outlets opened however
    /// busy the link is.
    DeliberateRefreshOnly,
}

/// The valve's own 1800 s stop.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
struct RuntimeTimer {
    limit: Duration,
    refresh: TimerRefresh,
    /// Simulated time at which the outlets close. `None` while nothing is open.
    deadline: Option<Duration>,
}

/// Why the outlets last went to zero.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum ShutdownCause {
    /// The master wrote an empty bitmap.
    Commanded,
    /// The valve's own runtime timer expired.
    RuntimeTimer,
    /// The valve saw no traffic for its communication-loss window.
    CommunicationLoss,
}

/// An emulated Saturn valve: a DTV 6-Port or a Prompt 3-Port.
///
/// Boot state is unaddressed, every outlet closed, no fault. Nothing here
/// persists across a construction, which is the rule the service follows too:
/// no water state survives a restart.
#[derive(Debug)]
pub struct SaturnValveModel {
    kind: ValveType,
    /// The `ADDRESS` field this valve puts in its replies.
    ///
    /// Configuration, because the master identity is unresolved between `0x00`
    /// and `0x10` — investigation I5. Worth stating what this model does
    /// **not** do: `docs/devices/valve-control.md` claims a Prompt 3 gives "no
    /// response" to a `0x00` master, and that cannot be reproduced from the
    /// documented frame layout, because a request carries only its destination
    /// and no field naming the sender. A valve has nothing to test against.
    /// `[?]`
    answers_as: MasterAddr,
    address: Option<ValveAddr>,
    firmware_type: u8,
    firmware_version: [u8; 3],
    serial: [u8; 6],
    calibration: [u8; 8],
    configuration: [u8; 6],
    two_byte_order: TwoByteOrder,

    outlets: u8,
    setpoint: Cx2,
    reported: Cx2,
    paused: bool,
    flow_rate: u16,

    /// When set, every addressed command answers `0x80` carrying this byte
    /// instead of its normal reply.
    ///
    /// `[?]` Whether a faulted valve still answers reads normally is not
    /// documented anywhere; this model takes the simple reading so a fault is
    /// unambiguous on the transcript.
    error_response: Option<RawErrorByte>,
    /// The two-byte register a `0x0F` read reports. Independent of
    /// [`SaturnValveModel::error_response`]: no source maps one to the other.
    /// `ERR-07`.
    fault_bitmap: u16,
    /// Mechanically stuck: no command moves the outlets. Set by
    /// [`SaturnValveModel::weld`], and never unset, because nothing a
    /// controller can send clears it.
    stuck: bool,

    comms_loss_shutdown: Option<Duration>,
    runtime_timer: Option<RuntimeTimer>,
    purge: Option<Duration>,

    rx: RxBuffer,
    last_rx: Option<Duration>,
    flow_until: Option<Duration>,
    closed_by: Option<ShutdownCause>,
    forced: VecDeque<Vec<u8>>,
}

impl SaturnValveModel {
    /// A DTV 6-Port. Firmware type `0x06`, outlets numbered 0..5 in bits 0..5.
    ///
    /// No runtime timer: the 1800 s stop is documented for Prompt 3 valves
    /// only, and inventing one here would put a backstop under zone 1 that no
    /// source says exists.
    #[must_use]
    pub fn dtv_6_port() -> Self {
        Self::new(ValveType::Dtv6Port, None)
    }

    /// A Prompt 3-Port. Firmware type `0x1E`, outlets numbered 1..6 in bits
    /// 2..7, plus its own [`PROMPT3_TIMEOUT_MAX`] runtime stop.
    ///
    /// `refresh` has no default: see [`TimerRefresh`].
    #[must_use]
    pub fn prompt_3_port(refresh: TimerRefresh) -> Self {
        Self::new(
            ValveType::Prompt3Port,
            Some(RuntimeTimer {
                limit: PROMPT3_TIMEOUT_MAX,
                refresh,
                deadline: None,
            }),
        )
    }

    fn new(kind: ValveType, runtime_timer: Option<RuntimeTimer>) -> Self {
        Self {
            kind,
            answers_as: MasterAddr::Dtv,
            address: None,
            firmware_type: firmware_type_of(kind),
            // Arbitrary: no source publishes a firmware version for either
            // valve. `[I]`, and nothing asserts on the value.
            firmware_version: [1, 2, 3],
            serial: [0x53, 0x4E, 0x00, 0x00, 0x00, 0x01],
            calibration: [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17],
            configuration: [0x15, 0x00, 0x00, 0x00, 0x00, 0x00],
            two_byte_order: TwoByteOrder::BigEndian,
            outlets: 0,
            // Cx2 76 is 38.0 C: a plausible resting setpoint inside the
            // 30.0-42.5 clamp, not a documented power-on default. `[I]`
            setpoint: Cx2::from_raw(76),
            reported: Cx2::from_raw(76),
            paused: false,
            flow_rate: 0,
            error_response: None,
            fault_bitmap: 0,
            stuck: false,
            comms_loss_shutdown: None,
            runtime_timer,
            purge: None,
            rx: RxBuffer::new(),
            last_rx: None,
            flow_until: None,
            closed_by: None,
            forced: VecDeque::new(),
        }
    }

    /// Which `ADDRESS` this valve puts in its replies. Investigation I5.
    #[must_use]
    pub fn answering_as(mut self, master: MasterAddr) -> Self {
        self.answers_as = master;
        self
    }

    /// The valve's own fail-closed behaviour on a quiet bus.
    ///
    /// **`None` is the case that matters.** It models a valve that does not
    /// close when traffic stops, which is the only way to prove the service's
    /// own fail-off path rather than the valve's. `CORRECTIONS.md` item 11
    /// forbids asserting any latency figure in software either way; this switch
    /// exists so the software path is exercised without one.
    #[must_use]
    pub fn with_comms_loss_shutdown(mut self, after: Option<Duration>) -> Self {
        self.comms_loss_shutdown = after;
        self
    }

    /// Flow continuing after the valve has reported its outlets closed.
    ///
    /// `INVESTIGATIONS.md` I4 is open — two documents in this repository state
    /// opposite values for automatic purge — so this is `None` by default and a
    /// test that wants the behaviour asks for it. While a purge runs,
    /// [`SaturnValveModel::open_outlets`] reads zero and
    /// [`SaturnValveModel::is_flowing`] reads true. That gap is the whole
    /// point: "acknowledged off" and "no longer moving water" are different
    /// facts, and only the first has a wire encoding.
    #[must_use]
    pub fn with_purge(mut self, purge: Option<Duration>) -> Self {
        self.purge = purge;
        self
    }

    /// Which byte order two-byte reads come back in. `RESP-05`.
    #[must_use]
    pub fn with_two_byte_order(mut self, order: TwoByteOrder) -> Self {
        self.two_byte_order = order;
        self
    }

    /// Start already addressed, skipping discovery.
    #[must_use]
    pub fn preaddressed(mut self, addr: ValveAddr) -> Self {
        self.address = Some(addr);
        self
    }

    /// Report a firmware type byte other than this family's.
    ///
    /// For the case where a valve answers something no table covers, which
    /// `ValveType::from_firmware` has to refuse rather than default.
    #[must_use]
    pub fn with_firmware_type(mut self, id: u8) -> Self {
        self.firmware_type = id;
        self
    }

    // -- Observers -----------------------------------------------------------

    #[must_use]
    pub const fn valve_type(&self) -> ValveType {
        self.kind
    }

    /// The address discovery allocated, if any.
    #[must_use]
    pub const fn address(&self) -> Option<ValveAddr> {
        self.address
    }

    /// The wire bitmap of outlets the valve reports open.
    #[must_use]
    pub const fn open_outlets(&self) -> u8 {
        self.outlets
    }

    /// Whether water is actually moving.
    ///
    /// Not the same question as [`SaturnValveModel::open_outlets`] whenever a
    /// purge is configured. I4.
    #[must_use]
    pub fn is_flowing(&self, at: Duration) -> bool {
        self.outlets != 0 || self.flow_until.is_some_and(|until| at < until)
    }

    #[must_use]
    pub const fn setpoint(&self) -> Cx2 {
        self.setpoint
    }

    /// What a `0x0B` [`opcode::READ_TEMPERATURE`] read answers with.
    ///
    /// **Not the setpoint**, though the two start equal and a commanded
    /// setpoint write syncs them. This is the number the daemon compares
    /// against the independent probe, so it is the one an assertion about
    /// divergence has to read — see
    /// [`Self::set_reported_temperature`].
    #[must_use]
    pub const fn reported_temperature(&self) -> Cx2 {
        self.reported
    }

    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Why the outlets last closed, or `None` if they have not closed since
    /// they were last opened.
    #[must_use]
    pub const fn shutdown_cause(&self) -> Option<ShutdownCause> {
        self.closed_by
    }

    /// The simulated time the runtime timer will close the outlets at.
    #[must_use]
    pub fn runtime_deadline(&self) -> Option<Duration> {
        self.runtime_timer.and_then(|t| t.deadline)
    }

    // -- Injection -----------------------------------------------------------

    /// Answer every addressed command with `0x80` carrying this byte.
    ///
    /// The byte is carried verbatim and this model attaches no meaning to it:
    /// meaning requires naming one of the two incompatible tables, and that is
    /// the caller's decision. `CORRECTIONS.md` item 4.
    pub fn report_error(&mut self, code: RawErrorByte) {
        self.error_response = Some(code);
    }

    /// Stop answering with an error.
    ///
    /// This does not un-weld a welded valve. [`SaturnValveModel::weld`] also
    /// sets a mechanical flag, and there is no method that clears that.
    pub fn clear_error(&mut self) {
        self.error_response = None;
    }

    /// Report `WELDED` and stop responding to outlet commands.
    ///
    /// Code 35 under `ErrorTable::ValveControl`. The table is named because
    /// under `ErrorTable::SaturnProtocol` the same byte is a reserved
    /// calibration code carrying no such meaning. `ERR-06`, `CORRECTIONS.md`
    /// item 4.
    ///
    /// **There is no way back.** No controller command clears a welded valve,
    /// so no method here does either: the outlets keep whatever state they were
    /// in, and every command answers `0x80 23`.
    pub fn weld(&mut self) {
        self.stuck = true;
        self.error_response = Some(RawErrorByte(WELDED_CODE));
    }

    /// Set the two-byte register a `0x0F` read reports. Any nonzero value is a
    /// fault the service must fail closed on, whatever the bits turn out to
    /// mean.
    pub fn set_fault_bitmap(&mut self, bits: u16) {
        self.fault_bitmap = bits;
    }

    /// The temperature a `0x0B` read reports, independent of the setpoint.
    pub fn set_reported_temperature(&mut self, t: Cx2) {
        self.reported = t;
    }

    /// Reply with these exact bytes the next time this valve would answer.
    ///
    /// The reason this crate lives outside the daemon's dependency graph: the
    /// bytes are not framed, checksummed, bounded or checked, so a test can
    /// answer a well-formed request with anything at all.
    pub fn force_next_reply(&mut self, bytes: Vec<u8>) {
        self.forced.push_back(bytes);
    }

    // -- Internals -----------------------------------------------------------

    /// Advance the valve's own timers to `at`.
    fn advance(&mut self, at: Duration) {
        if self
            .runtime_timer
            .and_then(|t| t.deadline)
            .is_some_and(|dl| at >= dl)
        {
            self.close_outlets(at, ShutdownCause::RuntimeTimer);
        }

        let quiet = match (self.comms_loss_shutdown, self.last_rx) {
            (Some(window), Some(last)) => at.saturating_sub(last) >= window,
            // No configured shutdown is the valve that never closes on its own.
            _ => false,
        };
        if quiet && self.outlets != 0 {
            self.close_outlets(at, ShutdownCause::CommunicationLoss);
        }
    }

    fn close_outlets(&mut self, at: Duration, cause: ShutdownCause) {
        if let Some(t) = self.runtime_timer.as_mut() {
            t.deadline = None;
        }
        if self.stuck || self.outlets == 0 {
            return;
        }
        self.outlets = 0;
        self.flow_rate = 0;
        self.closed_by = Some(cause);
        // I4: the valve reports off here. Whether water has stopped is a
        // separate question, and with a purge configured the answer is no.
        if let Some(purge) = self.purge {
            self.flow_until = Some(at.saturating_add(purge));
        }
    }

    fn set_outlets(&mut self, bits: u8, at: Duration) {
        if self.stuck {
            return;
        }
        if bits == 0 {
            self.close_outlets(at, ShutdownCause::Commanded);
            return;
        }
        if self.outlets == 0
            && let Some(t) = self.runtime_timer.as_mut()
        {
            t.deadline = Some(at.saturating_add(t.limit));
        }
        self.outlets = bits;
        self.flow_rate = 100;
        self.flow_until = None;
        self.closed_by = None;
    }

    /// A reply frame addressed to the master identity this valve answers as.
    fn reply(&self, control: u8, data: &[u8]) -> Vec<u8> {
        raw_saturn(self.answers_as.byte(), control, data)
    }

    /// The state byte a valve reports alongside its outlet bitmap. `FLAG-03`.
    fn state_bits(&self) -> u8 {
        let mut s = ValveStateBits::empty();
        if self.paused {
            s |= ValveStateBits::PAUSE;
        }
        if self.error_response.is_some() || self.fault_bitmap != 0 {
            s |= ValveStateBits::ERROR;
        }
        s.bits()
    }

    /// Decode everything buffered and answer it.
    fn drain(&mut self, at: Duration) -> Vec<Vec<u8>> {
        let expect = Expectation::capture(self.answers_as);
        let mut out = Vec::new();
        // Bounded rather than `loop`. Every error path in the decoder advances
        // the buffer, so this terminates anyway — but a device model must not
        // be the thing that spins if that ever stops being true.
        for _ in 0..RX_CAPACITY {
            match decode(&mut self.rx, &expect) {
                Ok(None) => break,
                Ok(Some(frame)) => {
                    if let Some(reply) = self.on_frame(&frame, at) {
                        out.push(reply);
                    }
                }
                // Line noise, a truncated frame or a bad checksum. A real valve
                // says nothing; the decoder has already resynchronised.
                Err(_) => {}
            }
        }
        out
    }

    fn on_frame(&mut self, f: &DecodedFrame, at: Duration) -> Option<Vec<u8>> {
        if f.inferred_direction != Direction::MasterToValve {
            return None;
        }
        // `[?]` Modelled as any master-to-valve frame on the bus, not only
        // frames addressed to this valve. No source says which the firmware
        // counts, and with one valve per bus here the two coincide.
        self.last_rx = Some(at);

        let control = f.control.0;
        let addressed_here = self.address.is_some_and(|a| a.get() == f.address);

        let reply = if control == opcode::ADDRESS_MANAGEMENT && f.address == BROADCAST {
            self.on_address_management(f)
        } else if addressed_here {
            Some(self.on_command(f, control, at))
        } else {
            return None;
        };

        // The refresh reading under which ordinary polling holds the valve
        // open. Applied after the command so an all-off does not arm a timer.
        if self.outlets != 0
            && let Some(t) = self.runtime_timer.as_mut()
            && t.refresh == TimerRefresh::AnyValidCommand
        {
            t.deadline = Some(at.saturating_add(t.limit));
        }

        // A forced reply displaces whatever this valve would have said, and
        // still displaces it when the valve would have said nothing.
        match self.forced.pop_front() {
            Some(bytes) => Some(bytes),
            None => reply,
        }
    }

    /// `0x3A` to the broadcast address: one control byte, three subcommands.
    /// `CMD-01`.
    fn on_address_management(&mut self, f: &DecodedFrame) -> Option<Vec<u8>> {
        match f.data.as_slice() {
            // Discovery finds valves that have no address yet, so an addressed
            // valve stays quiet rather than colliding with the one being found.
            [opcode::SUB_ENQUIRY, ..] if self.address.is_none() => {
                // Five data bytes, for the documented 11-byte response. The
                // source prints `01 [type] [ver]` and declares DATA_LEN 5, so
                // the version occupies three bytes; the split below is
                // inference `[I]`.
                let d = [
                    opcode::SUB_ENQUIRY,
                    self.firmware_type,
                    self.firmware_version[0],
                    self.firmware_version[1],
                    self.firmware_version[2],
                ];
                Some(self.reply(opcode::ADDRESS_MANAGEMENT, &d))
            }
            [opcode::SUB_ALLOCATE, addr, ..] if self.address.is_none() => {
                self.address = ValveAddr::new(*addr).ok();
                // A six-byte ACK: control echoed, no data.
                Some(self.reply(opcode::ADDRESS_MANAGEMENT, &[]))
            }
            [opcode::SUB_CLEAR, ..] => {
                self.address = None;
                // No documented reply to a clear broadcast, which is why
                // `expected_response_len` returns `None` for it.
                None
            }
            _ => None,
        }
    }

    /// The reply to one addressed command. Always a reply: a valve that has
    /// been given an address answers everything, a denied opcode included.
    fn on_command(&mut self, f: &DecodedFrame, control: u8, at: Duration) -> Vec<u8> {
        // A fault displaces every answer. Worth recording what that does to the
        // daemon: an error response is seven bytes whatever was asked, so
        // against a request whose documented reply is eight, the decoder's
        // length check fires before its control-byte check and the frame is
        // reported as the wrong length rather than as an error. That is a
        // property of the documented tables, not of this model.
        if let Some(code) = self.error_response {
            return self.reply(opcode::RESPONSE_ERROR, &[code.0]);
        }

        let order = self.two_byte_order;
        match control {
            opcode::READ_FIRMWARE_VERSION => self.reply(control, &self.firmware_version),
            opcode::READ_FIRMWARE_TYPE => self.reply(control, &[self.firmware_type]),
            // `[I]` The source says "2 bytes outlet bitmap" and nothing about
            // the second byte; the documented valve state byte is what
            // plausibly rides there.
            opcode::READ_OUTLET_STATES => self.reply(control, &[self.outlets, self.state_bits()]),
            // `[?]` Cx2 covers 0-127.5 C in one byte, so the second byte's role
            // is unknown. Zero here, ordered by `RESP-05`.
            opcode::READ_TEMPERATURE => {
                let d = order.bytes(u16::from(self.reported.raw()));
                self.reply(control, &d)
            }
            opcode::READ_FLOW_RATE => {
                let d = order.bytes(self.flow_rate);
                self.reply(control, &d)
            }
            opcode::READ_FAULT_FLAGS => {
                let d = order.bytes(self.fault_bitmap);
                self.reply(control, &d)
            }
            // Read-only. The `0xC0` calibration write is denied by the absence
            // of a `SaturnOp` variant; the read is required, because the
            // Phase 0 calibration baseline is the only rollback there is.
            // `CORRECTIONS.md` item 7.
            opcode::READ_CALIBRATION => self.reply(control, &self.calibration),
            opcode::READ_SERIAL_NUMBER => self.reply(control, &self.serial),
            opcode::READ_CONFIGURATION => self.reply(control, &self.configuration),
            opcode::READ_GENERIC_OUTLET => {
                let mut d = [0u8; 11];
                if let Some(first) = d.first_mut() {
                    *first = self.outlets;
                }
                self.reply(control, &d)
            }
            opcode::READ_EXTENDED_STATUS => {
                let mut d = [0u8; 8];
                if let Some(first) = d.first_mut() {
                    *first = self.state_bits();
                }
                self.reply(control, &d)
            }
            opcode::READ_DIAGNOSTICS => self.reply(control, &[0u8; 4]),

            opcode::WRITE_OUTLET_STATES => {
                if let [bits, _flags, ..] = f.data.as_slice() {
                    self.set_outlets(*bits, at);
                    self.reply(control, &[])
                } else {
                    self.reply(opcode::RESPONSE_NAK, &[])
                }
            }
            opcode::WRITE_TARGET_TEMPERATURE => {
                if let [t, ..] = f.data.as_slice() {
                    self.setpoint = Cx2::from_raw(*t);
                    // No thermal model: the reported temperature follows the
                    // setpoint immediately. `[I]`, and the reason
                    // `set_reported_temperature` exists.
                    self.reported = self.setpoint;
                    self.reply(control, &[])
                } else {
                    self.reply(opcode::RESPONSE_NAK, &[])
                }
            }
            opcode::WRITE_PAUSE_STATE => {
                if let [state, ..] = f.data.as_slice() {
                    self.paused = state & ValveStateBits::PAUSE.bits() != 0;
                    self.reply(control, &[])
                } else {
                    self.reply(opcode::RESPONSE_NAK, &[])
                }
            }

            // Everything else, every denied opcode included. A denied control
            // byte reaching a valve is a bug in the daemon, and a NAK is what
            // puts it on the transcript instead of silence.
            _ => self.reply(opcode::RESPONSE_NAK, &[]),
        }
    }
}

impl DeviceModel for SaturnValveModel {
    fn on_bytes(&mut self, bytes: &[u8], at: Duration) -> Vec<Vec<u8>> {
        self.advance(at);
        self.rx.extend(bytes);
        self.drain(at)
    }

    fn tick(&mut self, at: Duration) -> Vec<Vec<u8>> {
        self.advance(at);
        // A valve never speaks unprompted.
        Vec::new()
    }
}

/// A shared handle to a valve behind a [`Wire`](crate::wire::Wire).
///
/// `Wire` takes its device as a `Box<dyn DeviceModel>` and never hands it back,
/// which is right: the transcript is the oracle and a test that reads device
/// state to decide whether it passed is asserting on the wrong thing. Two
/// things still need the model itself — injecting a fault part-way through a
/// run, and reading [`SaturnValveModel::is_flowing`], which has no wire
/// encoding at all — so this is the one supported route in.
#[derive(Clone, Debug)]
pub struct ValveHandle(Arc<Mutex<SaturnValveModel>>);

impl ValveHandle {
    #[must_use]
    pub fn new(model: SaturnValveModel) -> Self {
        Self(Arc::new(Mutex::new(model)))
    }

    /// Read or change the model.
    pub fn with<R>(&self, f: impl FnOnce(&mut SaturnValveModel) -> R) -> R {
        f(&mut self.guard())
    }

    /// A poisoned lock still yields the model. A panic in one test's device
    /// must not turn every later assertion into a second panic that hides it.
    fn guard(&self) -> MutexGuard<'_, SaturnValveModel> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl DeviceModel for ValveHandle {
    fn on_bytes(&mut self, bytes: &[u8], at: Duration) -> Vec<Vec<u8>> {
        self.guard().on_bytes(bytes, at)
    }

    fn tick(&mut self, at: Duration) -> Vec<Vec<u8>> {
        self.guard().tick(at)
    }
}

/// The firmware type byte a family reports. `RESP-03`.
const fn firmware_type_of(kind: ValveType) -> u8 {
    match kind {
        ValveType::Dtv6Port => 0x06,
        ValveType::Prompt2Port => 0x17,
        ValveType::Prompt3Port => 0x1E,
        ValveType::Prompt3FlowControl => 0xFF,
    }
}

/// Build a Saturn frame from raw parts.
///
/// **Unreachable from the daemon.** `kdtv-proto`'s encoder is an allowlist of
/// twenty operations and speaks only master to valve; this builds any control
/// byte, in either direction, with any payload. That capability is why this
/// crate is excluded from the daemon's dependency graph rather than hidden
/// behind a feature flag.
///
/// `DATA_LEN` and the checksum are computed to match `data`. For a frame where
/// they do not, use [`raw_saturn_malformed`].
#[must_use]
pub fn raw_saturn(address: u8, control: u8, data: &[u8]) -> Vec<u8> {
    let len = u8::try_from(data.len()).unwrap_or(u8::MAX);
    let mut v = Vec::with_capacity(data.len() + FRAME_OVERHEAD);
    v.extend_from_slice(&SYNC);
    v.push(address);
    v.push(control);
    v.push(len);
    v.extend_from_slice(data);
    v.push(checksum(address, control, len, data));
    v
}

/// The same frame with a deliberately wrong checksum.
#[must_use]
pub fn raw_saturn_bad_checksum(address: u8, control: u8, data: &[u8]) -> Vec<u8> {
    let mut v = raw_saturn(address, control, data);
    if let Some(last) = v.last_mut() {
        *last = last.wrapping_add(1);
    }
    v
}

/// A frame whose declared `DATA_LEN` and checksum are whatever the caller says.
///
/// For the cases a correct encoder cannot produce: a `DATA_LEN` above the
/// 14-byte maximum, a length that disagrees with the payload, or a checksum
/// over the wrong bytes.
#[must_use]
pub fn raw_saturn_malformed(
    address: u8,
    control: u8,
    declared_len: u8,
    data: &[u8],
    checksum_byte: u8,
) -> Vec<u8> {
    let mut v = Vec::with_capacity(data.len() + FRAME_OVERHEAD);
    v.extend_from_slice(&SYNC);
    v.push(address);
    v.push(control);
    v.push(declared_len);
    v.extend_from_slice(data);
    v.push(checksum_byte);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Direction as Dir, Entry};
    use crate::wire::{Wire, WireFault};
    use kdtv_proto::fixtures::FixtureSet;
    use kdtv_proto::gate::TransmitAuthority;
    use kdtv_proto::saturn::{
        DiscoveryToken, Encoder, LinkPhase, OutletMapping, OutletTable, PrimaryFlags, SaturnOp,
    };
    use kdtv_units::{LinkKind, OpenAuthority, Slot, SlotSet, ValveSetpoint, ZoneId};
    use std::fs::{File, OpenOptions};
    use std::io::Write;

    /// Stands in for `kdtv-safety`'s grant. The encoder will not build an
    /// outlet-open frame without one.
    #[derive(Debug)]
    struct Grant(ZoneId);
    impl OpenAuthority for Grant {
        fn authorised_zone(&self) -> ZoneId {
            self.0
        }
    }

    fn zone1_encoder() -> Encoder {
        let table = OutletTable::new(
            ValveType::Dtv6Port,
            (1u8..=5).map(|n| OutletMapping {
                slot: Slot::new(n).unwrap(),
                status_index: n,
                wire_outlet: n - 1,
            }),
        )
        .unwrap();
        Encoder::new(
            &TransmitAuthority::emulator_only(FixtureSet::embedded()),
            LinkKind::Zone(ZoneId::Zone1),
            MasterAddr::Dtv,
            table,
        )
    }

    fn zone2_encoder() -> Encoder {
        let table = OutletTable::new(
            ValveType::Prompt3Port,
            (1u8..=3).map(|n| OutletMapping {
                slot: Slot::new(n).unwrap(),
                status_index: n,
                wire_outlet: n,
            }),
        )
        .unwrap();
        Encoder::new(
            &TransmitAuthority::emulator_only(FixtureSet::embedded()),
            LinkKind::Zone(ZoneId::Zone2),
            MasterAddr::Dtv,
            table,
        )
    }

    /// One link driven by hand: the daemon end is a plain file on the pty
    /// follower, and time is a parameter.
    struct Link {
        wire: Wire,
        valve: ValveHandle,
        daemon: File,
        now: Duration,
    }

    impl Link {
        fn new(model: SaturnValveModel) -> Self {
            let valve = ValveHandle::new(model);
            let wire = Wire::new(Box::new(valve.clone())).expect("open a wire");
            let daemon = OpenOptions::new()
                .read(true)
                .write(true)
                .open(wire.follower_path())
                .expect("open the follower");
            Self {
                wire,
                valve,
                daemon,
                now: Duration::ZERO,
            }
        }

        fn send(&mut self, bytes: &[u8]) {
            self.daemon.write_all(bytes).expect("daemon transmits");
            self.daemon.flush().ok();
        }

        /// Advance the wire by `span`, one simulated millisecond at a time.
        fn run(&mut self, span: Duration) {
            let until = self.now.saturating_add(span);
            while self.now < until {
                self.now = self.now.saturating_add(Duration::from_millis(1));
                self.wire.pump(self.now).expect("pump");
            }
        }

        /// Transmit, then give the device long enough to answer.
        fn exchange(&mut self, bytes: &[u8]) {
            self.send(bytes);
            self.run(Duration::from_millis(60));
        }

        /// Every frame the device put on the wire, as hex.
        fn replies(&self) -> Vec<String> {
            self.wire
                .transcript()
                .entries()
                .iter()
                .filter(|e| e.direction == Dir::DeviceToDaemon)
                .map(Entry::hex)
                .collect()
        }

        fn last_reply(&self) -> String {
            self.replies().last().cloned().unwrap_or_default()
        }

        fn flowing(&self) -> bool {
            let now = self.now;
            self.valve.with(|v| v.is_flowing(now))
        }
    }

    /// Drive a model through the documented discovery sequence and leave it
    /// addressed at `0x03`.
    fn discover(link: &mut Link, enc: &Encoder) {
        let token = DiscoveryToken::mint(enc.link(), LinkPhase::Discovery).unwrap();
        let target = ValveAddr::new(0x03).unwrap();
        for op in [
            SaturnOp::AddressClear,
            SaturnOp::AddressEnquiry,
            SaturnOp::AddressAllocate(target),
        ] {
            let f = enc
                .encode(target, &op, LinkPhase::Discovery, Some(&token), None)
                .unwrap();
            link.exchange(f.bytes());
        }
    }

    fn encode(enc: &Encoder, op: &SaturnOp, phase: LinkPhase) -> Vec<u8> {
        let zone = enc.link().zone().map(Grant);
        enc.encode(
            ValveAddr::new(0x03).unwrap(),
            op,
            phase,
            None,
            zone.as_ref().map(|g| -> &dyn OpenAuthority { g }),
        )
        .unwrap()
        .bytes()
        .to_vec()
    }

    fn open_slot_one(link: &mut Link, enc: &Encoder) {
        let slots: SlotSet = [Slot::new(1).unwrap()].into_iter().collect();
        link.exchange(&encode(
            enc,
            &SaturnOp::SetOutlets {
                slots,
                flags: PrimaryFlags::CAPTURED,
            },
            LinkPhase::ReadyOff,
        ));
    }

    // -- Discovery -----------------------------------------------------------

    /// From cold: unaddressed, answers the enquiry with its firmware type, and
    /// takes the address it is allocated.
    #[test]
    fn discovery_from_cold_addresses_the_valve() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        assert_eq!(link.valve.with(|v| v.address()), None, "boots unaddressed");

        discover(&mut link, &enc);

        // The clear broadcast has no documented reply, so exactly two frames
        // came back: the enquiry response and the allocation ACK.
        let replies = link.replies();
        assert_eq!(replies.len(), 2, "{replies:?}");
        // 11 bytes: AA 55 00 3A 05 01 [type] [ver x3] CHK.
        assert_eq!(replies[0].len(), 32, "{}", replies[0]);
        assert!(
            replies[0].starts_with("AA 55 00 3A 05 01 06"),
            "{}",
            replies[0]
        );
        // Six bytes: control echoed, no data.
        assert_eq!(replies[1], "AA 55 00 3A 00 C6");

        assert_eq!(
            link.valve.with(|v| v.address()),
            Some(ValveAddr::new(0x03).unwrap())
        );
    }

    #[test]
    fn an_addressed_valve_stays_quiet_during_a_later_enquiry() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        let before = link.replies().len();

        let token = DiscoveryToken::mint(enc.link(), LinkPhase::Discovery).unwrap();
        let f = enc
            .encode(
                ValveAddr::new(0x03).unwrap(),
                &SaturnOp::AddressEnquiry,
                LinkPhase::Discovery,
                Some(&token),
                None,
            )
            .unwrap();
        link.exchange(f.bytes());
        assert_eq!(
            link.replies().len(),
            before,
            "an addressed valve answered an enquiry meant for unaddressed ones"
        );
    }

    /// A clear broadcast takes the address away again, and the valve is
    /// discoverable from cold a second time.
    #[test]
    fn an_address_clear_returns_the_valve_to_unaddressed() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        assert!(link.valve.with(|v| v.address().is_some()));

        let token = DiscoveryToken::mint(enc.link(), LinkPhase::Discovery).unwrap();
        let f = enc
            .encode(
                ValveAddr::new(0x03).unwrap(),
                &SaturnOp::AddressClear,
                LinkPhase::Discovery,
                Some(&token),
                None,
            )
            .unwrap();
        link.exchange(f.bytes());
        assert_eq!(link.valve.with(|v| v.address()), None);
    }

    #[test]
    fn a_prompt_3_reports_its_own_firmware_type() {
        let mut link = Link::new(SaturnValveModel::prompt_3_port(
            TimerRefresh::DeliberateRefreshOnly,
        ));
        let enc = zone2_encoder();
        discover(&mut link, &enc);
        assert!(
            link.replies()[0].starts_with("AA 55 00 3A 05 01 1E"),
            "{}",
            link.replies()[0]
        );
    }

    // -- Reads ---------------------------------------------------------------

    #[test]
    fn a_read_answers_with_the_documented_length() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);

        for (op, expected_len) in [
            (SaturnOp::ReadFirmwareVersion, 9usize),
            (SaturnOp::ReadFirmwareType, 7),
            (SaturnOp::ReadOutlets, 8),
            (SaturnOp::ReadTemperature, 8),
            (SaturnOp::ReadFlow, 8),
            (SaturnOp::ReadFaults, 8),
            (SaturnOp::ReadCalibration, 14),
            (SaturnOp::ReadConfiguration, 12),
            (SaturnOp::ReadSerialNumber, 12),
            (SaturnOp::ReadGenericOutlets, 17),
            (SaturnOp::ReadExtendedStatus, 14),
            (SaturnOp::ReadDiagnostics, 10),
        ] {
            link.exchange(&encode(&enc, &op, LinkPhase::ReadyOff));
            let reply = link.last_reply();
            let got = reply.split_whitespace().count();
            assert_eq!(
                got, expected_len,
                "{op:?} answered {got} bytes, expected {expected_len}: {reply}"
            );
        }
    }

    /// The reads `CORRECTIONS.md` item 7 requires. The Phase 0 calibration
    /// baseline is the only rollback there is, and once the K-99695 is powered
    /// down this service is the only thing that can read it back.
    #[test]
    fn calibration_and_configuration_are_readable() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);

        link.exchange(&encode(
            &enc,
            &SaturnOp::ReadCalibration,
            LinkPhase::ReadyOff,
        ));
        assert!(
            link.last_reply()
                .starts_with("AA 55 00 10 08 10 11 12 13 14 15 16 17"),
            "{}",
            link.last_reply()
        );
        link.exchange(&encode(
            &enc,
            &SaturnOp::ReadConfiguration,
            LinkPhase::ReadyOff,
        ));
        assert!(
            link.last_reply().starts_with("AA 55 00 15 06 15"),
            "{}",
            link.last_reply()
        );
    }

    #[test]
    fn a_temperature_read_carries_the_configured_byte_order() {
        let enc = zone1_encoder();
        for (order, expect_data) in [
            (TwoByteOrder::BigEndian, "00 4C"),
            (TwoByteOrder::LittleEndian, "4C 00"),
        ] {
            let mut link = Link::new(SaturnValveModel::dtv_6_port().with_two_byte_order(order));
            discover(&mut link, &enc);
            link.exchange(&encode(
                &enc,
                &SaturnOp::ReadTemperature,
                LinkPhase::ReadyOff,
            ));
            assert!(
                link.last_reply().contains(expect_data),
                "{order:?}: {}",
                link.last_reply()
            );
        }
    }

    // -- Writes --------------------------------------------------------------

    /// The same slot reaches a different physical outlet on each family. The
    /// model does not know which — it applies the byte it was sent — and that
    /// is the point: the mapping is the encoder's job and this asserts it
    /// arrived intact. `OUT-01`.
    #[test]
    fn an_outlet_write_opens_the_bitmap_the_encoder_sent() {
        let mut dtv = Link::new(SaturnValveModel::dtv_6_port());
        let e1 = zone1_encoder();
        discover(&mut dtv, &e1);
        open_slot_one(&mut dtv, &e1);
        assert_eq!(dtv.last_reply(), "AA 55 00 87 00 79", "write ACK");
        // Slot 1 on the DTV is wire outlet 0, mask 0x01.
        assert_eq!(dtv.valve.with(|v| v.open_outlets()), 0x01);

        let mut prompt = Link::new(SaturnValveModel::prompt_3_port(
            TimerRefresh::DeliberateRefreshOnly,
        ));
        let e2 = zone2_encoder();
        discover(&mut prompt, &e2);
        open_slot_one(&mut prompt, &e2);
        // Slot 1 on the Prompt 3 is wire outlet 1, mask 0x04 — the same slot,
        // a different byte.
        assert_eq!(prompt.valve.with(|v| v.open_outlets()), 0x04);

        // And the valve reports it back over the wire, not just in its state.
        prompt.exchange(&encode(&e2, &SaturnOp::ReadOutlets, LinkPhase::Running));
        assert_eq!(prompt.last_reply(), "AA 55 00 07 02 04 00 F3");
    }

    #[test]
    fn an_all_off_closes_every_outlet() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        let slots: SlotSet = [Slot::new(1).unwrap(), Slot::new(2).unwrap()]
            .into_iter()
            .collect();
        link.exchange(&encode(
            &enc,
            &SaturnOp::SetOutlets {
                slots,
                flags: PrimaryFlags::CAPTURED,
            },
            LinkPhase::ReadyOff,
        ));
        assert_eq!(link.valve.with(|v| v.open_outlets()), 0x03);

        link.exchange(&encode(&enc, &SaturnOp::AllOff, LinkPhase::Running));
        assert_eq!(link.valve.with(|v| v.open_outlets()), 0x00);
        assert_eq!(
            link.valve.with(|v| v.shutdown_cause()),
            Some(ShutdownCause::Commanded)
        );
    }

    #[test]
    fn a_temperature_write_is_acknowledged_and_read_back() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);

        // Cx2 80 is 40.0 C, inside the 30.0-42.5 clamp the type enforces.
        let sp = ValveSetpoint::try_new(Cx2::from_raw(80)).unwrap();
        link.exchange(&encode(
            &enc,
            &SaturnOp::SetTemperature(sp),
            LinkPhase::ReadyOff,
        ));
        assert_eq!(link.last_reply(), "AA 55 00 8B 00 75", "write ACK");
        assert_eq!(link.valve.with(|v| v.setpoint().raw()), 80);

        link.exchange(&encode(
            &enc,
            &SaturnOp::ReadTemperature,
            LinkPhase::ReadyOff,
        ));
        assert_eq!(
            link.last_reply(),
            "AA 55 00 0B 02 00 50 A3",
            "0x50 is Cx2 80, which is 40.0 C"
        );
    }

    #[test]
    fn pause_and_resume_move_the_state_bit() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);

        link.exchange(&encode(&enc, &SaturnOp::Pause, LinkPhase::Running));
        assert!(link.valve.with(|v| v.is_paused()));
        link.exchange(&encode(&enc, &SaturnOp::ReadOutlets, LinkPhase::Paused));
        assert!(
            link.last_reply().ends_with("02 00 02 F5"),
            "PAUSE set: {}",
            link.last_reply()
        );

        link.exchange(&encode(&enc, &SaturnOp::Resume, LinkPhase::Paused));
        assert!(!link.valve.with(|v| v.is_paused()));
    }

    // -- Faults --------------------------------------------------------------

    #[test]
    fn a_reported_fault_answers_every_command_with_an_error_frame() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);

        link.exchange(&encode(
            &enc,
            &SaturnOp::ReadTemperature,
            LinkPhase::ReadyOff,
        ));
        let healthy = link.last_reply();

        // Under `ErrorTable::SaturnProtocol` code 14 is over-temperature; under
        // `ErrorTable::ValveControl` it is outside the eight-entry table
        // altogether. The model carries the byte and names neither.
        link.valve.with(|v| v.report_error(RawErrorByte(14)));
        link.exchange(&encode(
            &enc,
            &SaturnOp::ReadTemperature,
            LinkPhase::ReadyOff,
        ));
        assert_eq!(link.last_reply(), "AA 55 00 80 01 0E 71");
        assert_ne!(link.last_reply(), healthy);
    }

    /// `WELDED` (35): the outlets do not move, and the fault does not clear.
    #[test]
    fn a_welded_valve_does_not_close_and_cannot_be_cleared() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        open_slot_one(&mut link, &enc);

        link.valve.with(SaturnValveModel::weld);
        link.exchange(&encode(&enc, &SaturnOp::AllOff, LinkPhase::Faulted));
        // 0x80 carrying 35 = 0x23.
        assert_eq!(link.last_reply(), "AA 55 00 80 01 23 5C");
        assert_eq!(link.valve.with(|v| v.open_outlets()), 0x01, "still open");

        // Clearing the reported byte does not un-weld the mechanism.
        link.valve.with(SaturnValveModel::clear_error);
        link.exchange(&encode(&enc, &SaturnOp::AllOff, LinkPhase::Faulted));
        assert_eq!(
            link.valve.with(|v| v.open_outlets()),
            0x01,
            "no controller command closes a welded valve"
        );
    }

    #[test]
    fn a_nonzero_fault_register_comes_back_from_a_fault_read() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        link.valve.with(|v| v.set_fault_bitmap(0x0140));
        link.exchange(&encode(&enc, &SaturnOp::ReadFaults, LinkPhase::ReadyOff));
        assert_eq!(link.last_reply(), "AA 55 00 0F 02 01 40 AE");
    }

    // -- Communication loss ---------------------------------------------------

    /// The valve that behaves as the sources describe: traffic stops, the valve
    /// closes itself.
    #[test]
    fn traffic_stopping_closes_a_valve_with_a_comms_loss_shutdown() {
        let mut link = Link::new(
            SaturnValveModel::dtv_6_port().with_comms_loss_shutdown(Some(Duration::from_secs(5))),
        );
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        open_slot_one(&mut link, &enc);

        // Still inside the window.
        link.run(Duration::from_secs(4));
        assert_eq!(link.valve.with(|v| v.open_outlets()), 0x01);

        // Past it, with nothing on the wire.
        link.run(Duration::from_secs(10));
        assert_eq!(link.valve.with(|v| v.open_outlets()), 0x00);
        assert_eq!(
            link.valve.with(|v| v.shutdown_cause()),
            Some(ShutdownCause::CommunicationLoss)
        );

        // And it says so when asked, so the daemon can see it too.
        link.exchange(&encode(&enc, &SaturnOp::ReadOutlets, LinkPhase::Running));
        assert_eq!(link.last_reply(), "AA 55 00 07 02 00 00 F7");
    }

    /// **The case that matters.** A valve with no communication-loss shutdown
    /// keeps water running indefinitely, so anything that stops it came from
    /// the service. `CORRECTIONS.md` item 11: no latency figure is asserted
    /// here or anywhere else in software.
    #[test]
    fn traffic_stopping_leaves_a_valve_with_no_shutdown_wide_open() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port().with_comms_loss_shutdown(None));
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        open_slot_one(&mut link, &enc);

        link.run(Duration::from_secs(120));
        assert_eq!(
            link.valve.with(|v| v.open_outlets()),
            0x01,
            "this valve must NOT close on its own"
        );
        assert_eq!(link.valve.with(|v| v.shutdown_cause()), None);
        assert!(link.flowing());

        // And it says so on the wire, which is where the assertion has to
        // land: a service that believed the valve had closed itself would be
        // reading its own hope rather than the bus.
        link.exchange(&encode(&enc, &SaturnOp::ReadOutlets, LinkPhase::Running));
        assert_eq!(link.last_reply(), "AA 55 00 07 02 01 00 F6");
    }

    // -- The Prompt 3 runtime timer -------------------------------------------

    /// Under "a deliberate refresh only", polling does not help: the valve
    /// stops 1800 s after the outlets opened. This master sends no refresh, so
    /// this is the reading in which the timer is a hazard rather than a
    /// backstop.
    #[test]
    fn the_prompt_3_timer_expires_under_deliberate_refresh_only() {
        let mut link = Link::new(
            SaturnValveModel::prompt_3_port(TimerRefresh::DeliberateRefreshOnly)
                .with_comms_loss_shutdown(None),
        );
        let enc = zone2_encoder();
        discover(&mut link, &enc);
        open_slot_one(&mut link, &enc);

        // Poll all the way through, as the service would.
        for _ in 0..40 {
            link.exchange(&encode(&enc, &SaturnOp::ReadOutlets, LinkPhase::Running));
            link.run(Duration::from_secs(45));
        }
        assert_eq!(
            link.valve.with(|v| v.open_outlets()),
            0x00,
            "the runtime timer should have closed it"
        );
        assert_eq!(
            link.valve.with(|v| v.shutdown_cause()),
            Some(ShutdownCause::RuntimeTimer)
        );
    }

    /// Under "any valid command", the same polling holds it open — which is why
    /// the two readings cannot be collapsed into one.
    #[test]
    fn the_prompt_3_timer_is_held_off_under_any_valid_command() {
        let mut link = Link::new(
            SaturnValveModel::prompt_3_port(TimerRefresh::AnyValidCommand)
                .with_comms_loss_shutdown(None),
        );
        let enc = zone2_encoder();
        discover(&mut link, &enc);
        open_slot_one(&mut link, &enc);

        for _ in 0..40 {
            link.exchange(&encode(&enc, &SaturnOp::ReadOutlets, LinkPhase::Running));
            link.run(Duration::from_secs(45));
        }
        assert_eq!(
            link.valve.with(|v| v.open_outlets()),
            0x04,
            "polling should have refreshed the timer"
        );
    }

    #[test]
    fn a_dtv_valve_has_no_runtime_timer_at_all() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port().with_comms_loss_shutdown(None));
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        open_slot_one(&mut link, &enc);
        assert_eq!(link.valve.with(|v| v.runtime_deadline()), None);
        link.run(PROMPT3_TIMEOUT_MAX + Duration::from_secs(60));
        assert_eq!(link.valve.with(|v| v.open_outlets()), 0x01);
    }

    // -- Purge ---------------------------------------------------------------

    /// I4. With a purge configured the valve reports its outlets closed while
    /// water is still moving, so "acknowledged off" is not "stopped".
    #[test]
    fn a_purge_keeps_water_moving_after_the_off_acknowledgement() {
        let purge = Duration::from_secs(8);
        let mut link = Link::new(
            SaturnValveModel::dtv_6_port()
                .with_purge(Some(purge))
                .with_comms_loss_shutdown(None),
        );
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        open_slot_one(&mut link, &enc);

        link.exchange(&encode(&enc, &SaturnOp::AllOff, LinkPhase::Running));
        assert_eq!(link.valve.with(|v| v.open_outlets()), 0x00, "reported off");
        assert!(
            link.flowing(),
            "but still flowing — that gap is what I4 is about"
        );

        link.run(purge + Duration::from_secs(1));
        assert!(!link.flowing(), "the purge should have finished");
    }

    #[test]
    fn with_no_purge_the_acknowledgement_and_the_flow_agree() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port().with_comms_loss_shutdown(None));
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        open_slot_one(&mut link, &enc);
        assert!(link.flowing());
        link.exchange(&encode(&enc, &SaturnOp::AllOff, LinkPhase::Running));
        assert!(!link.flowing());
    }

    // -- Malformity ----------------------------------------------------------

    #[test]
    fn a_forced_reply_displaces_a_well_formed_one() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        // A DATA_LEN of 200, which no legal frame can carry: `PHY-02` caps it
        // at 14. Only this crate can build it.
        let junk = raw_saturn_malformed(0x00, 0x0B, 200, &[0xDE, 0xAD], 0x00);
        link.valve.with(|v| v.force_next_reply(junk));
        link.exchange(&encode(
            &enc,
            &SaturnOp::ReadTemperature,
            LinkPhase::ReadyOff,
        ));
        assert_eq!(link.last_reply(), "AA 55 00 0B C8 DE AD 00");
    }

    #[test]
    fn a_corrupt_request_gets_no_answer_and_the_next_one_still_does() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        let before = link.replies().len();

        link.exchange(&raw_saturn_bad_checksum(
            0x03,
            opcode::READ_TEMPERATURE,
            &[],
        ));
        assert_eq!(
            link.replies().len(),
            before,
            "a bad checksum must not be answered"
        );

        link.exchange(&encode(
            &enc,
            &SaturnOp::ReadTemperature,
            LinkPhase::ReadyOff,
        ));
        assert_eq!(link.replies().len(), before + 1, "and the link recovers");
    }

    /// `FRAME-04`: garbage in front of a frame is skipped, not fatal. Echo
    /// bleed is the realistic source, which is why the wire can inject it even
    /// though the selected converters present none. `CORRECTIONS.md` item 3.
    #[test]
    fn leading_garbage_and_echo_do_not_stop_the_valve_answering() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);

        let req = encode(&enc, &SaturnOp::ReadFirmwareType, LinkPhase::ReadyOff);
        let mut noisy = vec![0xFF, 0x00, 0xAA, 0x13];
        noisy.extend_from_slice(&req);
        link.wire.inject(WireFault::Echo);
        link.exchange(&noisy);

        assert!(
            link.replies().iter().any(|r| r == "AA 55 00 02 01 06 F7"),
            "{:?}",
            link.replies()
        );
    }

    #[test]
    fn a_denied_control_byte_is_nakked_rather_than_obeyed() {
        let mut link = Link::new(SaturnValveModel::dtv_6_port());
        let enc = zone1_encoder();
        discover(&mut link, &enc);
        // 0xF4 FACTORY_RESET. `SaturnOp` has no variant for it, so this frame
        // can only exist here.
        link.exchange(&raw_saturn(0x03, opcode::FACTORY_RESET, &[]));
        assert_eq!(link.last_reply(), "AA 55 00 FF 00 01");
    }

    // -- Raw builders --------------------------------------------------------

    #[test]
    fn raw_saturn_reproduces_the_documented_frames() {
        // saturn-protocol.md Example 2, correct as printed.
        assert_eq!(
            raw_saturn(0x03, 0x87, &[0x04, 0x00]),
            vec![0xAA, 0x55, 0x03, 0x87, 0x02, 0x04, 0x00, 0x70]
        );
        // Example 3, after the document corrects its own checksum from 0xAE.
        assert_eq!(
            raw_saturn(0x0F, 0x3A, &[0x03]),
            vec![0xAA, 0x55, 0x0F, 0x3A, 0x01, 0x03, 0xB3]
        );
        // The malformed builder is free of all of it.
        assert_eq!(
            raw_saturn_malformed(0xEE, 0xF4, 0xFF, &[], 0x00),
            vec![0xAA, 0x55, 0xEE, 0xF4, 0xFF, 0x00]
        );
        assert_ne!(
            raw_saturn_bad_checksum(0x03, 0x87, &[0x04, 0x00]).last(),
            Some(&0x70)
        );
    }

    #[test]
    fn every_family_reports_the_firmware_byte_its_table_gives() {
        assert_eq!(firmware_type_of(ValveType::Dtv6Port), 0x06);
        assert_eq!(firmware_type_of(ValveType::Prompt2Port), 0x17);
        assert_eq!(firmware_type_of(ValveType::Prompt3Port), 0x1E);
        assert_eq!(firmware_type_of(ValveType::Prompt3FlowControl), 0xFF);
        assert_eq!(
            SaturnValveModel::dtv_6_port().valve_type(),
            ValveType::Dtv6Port
        );
    }

    /// A valve that starts addressed skips discovery, and a firmware type
    /// override reaches the wire — the case where `ValveType::from_firmware`
    /// must refuse rather than default.
    #[test]
    fn a_preaddressed_valve_answers_immediately_with_its_override() {
        let mut link = Link::new(
            SaturnValveModel::dtv_6_port()
                .preaddressed(ValveAddr::new(0x03).unwrap())
                .with_firmware_type(0x42),
        );
        let enc = zone1_encoder();
        link.exchange(&encode(
            &enc,
            &SaturnOp::ReadFirmwareType,
            LinkPhase::ReadyOff,
        ));
        assert_eq!(link.last_reply(), "AA 55 00 02 01 42 BB");
    }

    /// I5. The reply address is configuration, because the sources disagree
    /// about which identity a Prompt 3 answers.
    #[test]
    fn the_reply_address_follows_the_configured_master_identity() {
        let mut link = Link::new(
            SaturnValveModel::prompt_3_port(TimerRefresh::DeliberateRefreshOnly)
                .preaddressed(ValveAddr::new(0x03).unwrap())
                .answering_as(MasterAddr::Prompt),
        );
        let enc = zone2_encoder();
        link.exchange(&encode(
            &enc,
            &SaturnOp::ReadFirmwareType,
            LinkPhase::ReadyOff,
        ));
        assert!(
            link.last_reply().starts_with("AA 55 10 02"),
            "{}",
            link.last_reply()
        );
    }
}
