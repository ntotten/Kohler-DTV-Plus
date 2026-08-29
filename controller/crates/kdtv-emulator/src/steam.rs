//! An emulated K-1737-K1 steam adapter on the DTV+ link.
//!
//! # Evidence
//!
//! **Thinner than anything else in this workspace.** No DTV+ bus has ever been
//! captured in this project — the reference system reports
//! `steam_installed = false` and `steam_con_string = "not_seen"` — so every
//! frame this model produces is tier `[C]` from
//! `research/xagon0/docs/protocols/dtv-plus-protocol.md` and
//! `research/xagon0/docs/devices/steam-generator.md`, and the two disagree in
//! two places that change what appears on the wire. Both readings are plumbed
//! and neither is chosen here.
//!
//! | Contradiction | Where | Positions |
//! | --- | --- | --- |
//! | Does `SET_DEV_PARAM` get an acknowledgement? | [`WriteAck`] | `dtv-plus-protocol.md` § Set Device Parameter shows `DEV_ACK` or `DEV_NAK`; `steam-generator.md` § `SET_DEV_PARAM` says "No explicit response is expected" |
//! | Which opcode carries a status reply | [`SteamAdapterModel::with_status_carrier`] | `0x30` echoed, `0x31` `STATUS_UPDATE`, or `0x35` `DEV_ACK`. `kdtv-proto` accepts all three and records which arrived |
//!
//! # Device ID and device address are different namespaces
//!
//! `0x05` is the steam generator's **device ID**, carried in the payload of
//! `DEV_REQUEST_ADDR` while the device is still unaddressed. The address the
//! master assigns is a separate byte in `0x03..=0x07`. This model keeps them in
//! separate fields with no conversion, the same way `kdtv-proto` keeps them in
//! separate types. `CORRECTIONS.md` item 2.
//!
//! # Power clean is observable and not commandable
//!
//! `0xCC` in the operation-state byte starts a 45-minute unattended cycle. The
//! encoder cannot produce it — `SteamOpState` has two variants — so the only
//! way to reach it here is [`raw_dtv`], which is exactly the situation being
//! modelled: a cycle *someone else* started, which the service must observe,
//! report, and not interrupt. `STEAM-12`.

use crate::wire::DeviceModel;
use kdtv_proto::dtv::{
    DecodedDtv, DevAddr, DeviceId, DtvRxBuffer, EOF, MASTER, RX_CAPACITY, SOF, STATUS_PAYLOAD_LEN,
    StatusCarrier, SteamErrorFlags, SteamStateByte, SteamStatus, checksum, decode, is_reserved,
    opcode,
};
use kdtv_units::Fx2;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// Whether a parameter write is acknowledged.
///
/// **Unresolved, and this enum has no `Default` on purpose.** The two sources
/// describe different behaviour for the same opcode, and the difference is not
/// cosmetic: under [`WriteAck::Silent`] a master that waits for an
/// acknowledgement times out on every write it makes.
///
/// The same switch covers `CLEAR_FAULT_FLAGS`, whose reply no source describes
/// at all. That is inference `[I]` — the two are grouped because both are
/// master-to-device writes, not because any document links them.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum WriteAck {
    /// `dtv-plus-protocol.md` § Set Device Parameter: `DEV_ACK` on success,
    /// `DEV_NAK` with an error byte on rejection.
    DevAck,
    /// `steam-generator.md` § `SET_DEV_PARAM`: "No explicit response is
    /// expected. Poll with `GET_DEV_STATUS` to confirm the change took effect."
    Silent,
}

/// The error byte a `DEV_NAK` carries.
///
/// `[I]`. No source enumerates the DTV+ NAK error space, so only the presence
/// of a byte is modelled; the value is arbitrary and nothing asserts on it.
pub const NAK_ERROR_BYTE: u8 = 0x01;

/// An emulated steam adapter.
///
/// Boot state is unaddressed and `STEAM_OFF` with no session running. Nothing
/// persists across a construction: a generator that resumed heating after a
/// restart would defeat the whole fail-off design.
#[derive(Debug)]
pub struct SteamAdapterModel {
    /// The device **type** byte this adapter puts in `DEV_REQUEST_ADDR`.
    /// Never a destination. `CORRECTIONS.md` item 2.
    device_id: DeviceId,
    /// The bus address the master assigned, or `None` before discovery.
    address: Option<DevAddr>,

    actual: Fx2,
    desired: Fx2,
    state: SteamStateByte,
    errors: SteamErrorFlags,

    status_carrier: StatusCarrier,
    write_ack: WriteAck,
    nak_error: u8,

    /// Simulated time the session timer runs out. `None` when nothing is
    /// running.
    session_end: Option<Duration>,
    /// The duration most recently written, carried so a status poll can report
    /// a timer that has not started.
    session_len: Duration,
    /// Time per one raw `Fx2` step of movement towards the setpoint. `None`
    /// holds the actual temperature still, which is the default: no source
    /// describes the generator's heating rate, and inventing one would put a
    /// number in the transcript that means nothing.
    ramp: Option<Duration>,
    ramped_at: Duration,

    rx: DtvRxBuffer,
    forced: VecDeque<Vec<u8>>,
    next_nak: Option<u8>,
    next_error: Option<u8>,
}

impl SteamAdapterModel {
    /// A steam adapter with device ID `0x05`, unaddressed and off.
    ///
    /// `write_ack` has no default: see [`WriteAck`].
    #[must_use]
    pub fn new(write_ack: WriteAck) -> Self {
        Self {
            device_id: DeviceId::STEAM_GENERATOR,
            address: None,
            // Fx2 140 is 70 F: a cold generator at room temperature. Below the
            // 180-250 setpoint clamp, and deliberately not equal to any
            // setpoint, so a status frame that echoes the wrong field shows up.
            actual: Fx2::from_raw(140),
            desired: kdtv_units::SteamSetpoint::FACTORY_DEFAULT,
            state: SteamStateByte::Off,
            errors: SteamErrorFlags::empty(),
            status_carrier: StatusCarrier::DevAck,
            write_ack,
            nak_error: NAK_ERROR_BYTE,
            session_end: None,
            session_len: Duration::ZERO,
            ramp: None,
            ramped_at: Duration::ZERO,
            rx: DtvRxBuffer::new(),
            forced: VecDeque::new(),
            next_nak: None,
            next_error: None,
        }
    }

    /// Which opcode carries the status payload. `STEAM-04`: three candidates,
    /// all accepted by the decoder, and which one a real adapter uses is the
    /// open question.
    #[must_use]
    pub fn with_status_carrier(mut self, carrier: StatusCarrier) -> Self {
        self.status_carrier = carrier;
        self
    }

    /// Report a device ID other than the steam generator's, for the case where
    /// something unexpected answers an address opportunity.
    #[must_use]
    pub fn with_device_id(mut self, id: DeviceId) -> Self {
        self.device_id = id;
        self
    }

    /// Start already addressed, skipping discovery.
    #[must_use]
    pub fn preaddressed(mut self, addr: DevAddr) -> Self {
        self.address = Some(addr);
        self
    }

    /// Move the actual temperature one raw `Fx2` step towards the setpoint
    /// every `per_step` of simulated time while the generator is producing.
    ///
    /// `[I]`. No source gives a heating rate; this exists so a test can watch
    /// the actual and desired fields diverge, not because the number means
    /// anything.
    #[must_use]
    pub fn with_ramp(mut self, per_step: Option<Duration>) -> Self {
        self.ramp = per_step;
        self
    }

    // -- Observers -----------------------------------------------------------

    /// The address discovery assigned, if any.
    #[must_use]
    pub const fn address(&self) -> Option<DevAddr> {
        self.address
    }

    /// The device ID this adapter reports. Not an address.
    #[must_use]
    pub const fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// The operation state, including states this master cannot command.
    #[must_use]
    pub const fn state(&self) -> SteamStateByte {
        self.state
    }

    /// True while the generator is making heat, power clean included.
    #[must_use]
    pub const fn is_producing(&self) -> bool {
        self.state.is_producing()
    }

    #[must_use]
    pub const fn desired(&self) -> Fx2 {
        self.desired
    }

    #[must_use]
    pub const fn actual(&self) -> Fx2 {
        self.actual
    }

    #[must_use]
    pub const fn errors(&self) -> SteamErrorFlags {
        self.errors
    }

    /// How much of the session timer is left at `at`.
    #[must_use]
    pub fn remaining(&self, at: Duration) -> Duration {
        self.session_end
            .map_or(Duration::ZERO, |end| end.saturating_sub(at))
    }

    // -- Injection -----------------------------------------------------------

    /// Set the error bitmask a status frame reports.
    ///
    /// Undocumented bits are kept: `STEAM-14` requires any nonzero byte to read
    /// as a fault, including one carrying only bits no source names. Whether a
    /// real generator turns itself off on an overtemperature is unknown `[?]`,
    /// so this model does not — the service's own response is the thing under
    /// test.
    pub fn inject_errors(&mut self, errors: SteamErrorFlags) {
        self.errors = errors;
    }

    /// Answer the next request with `DEV_NAK` carrying this byte.
    ///
    /// `STEAM-09`: a NAK is a rejected command, not a transient failure.
    pub fn nak_next(&mut self, error: u8) {
        self.next_nak = Some(error);
    }

    /// Answer the next request with `ERROR` (`0x37`) carrying this code.
    pub fn error_next(&mut self, code: u8) {
        self.next_error = Some(code);
    }

    /// The actual temperature a status frame reports, independent of the
    /// setpoint.
    pub fn set_actual(&mut self, t: Fx2) {
        self.actual = t;
    }

    /// Reply with these exact bytes the next time this adapter would answer.
    ///
    /// Unframed, unstuffed, unchecked. See [`raw_dtv_unstuffed`].
    pub fn force_next_reply(&mut self, bytes: Vec<u8>) {
        self.forced.push_back(bytes);
    }

    // -- Internals -----------------------------------------------------------

    fn advance(&mut self, at: Duration) {
        if self.session_end.is_some_and(|end| at >= end) {
            self.session_end = None;
            // The generator's own automatic shutoff. `STEAM-18`: this is the
            // only backstop that survives the service dying, which is why
            // `SteamMinutes` has no zero — `steamTimerSetTime = 0` disables it.
            self.state = SteamStateByte::Off;
        }

        let Some(per_step) = self.ramp else {
            self.ramped_at = at;
            return;
        };
        if per_step.is_zero() {
            return;
        }
        let elapsed = at.saturating_sub(self.ramped_at);
        let steps = u8::try_from(elapsed.as_nanos() / per_step.as_nanos()).unwrap_or(u8::MAX);
        if steps == 0 {
            return;
        }
        self.ramped_at = at;
        let target = if self.state.is_producing() {
            self.desired.raw()
        } else {
            self.actual.raw()
        };
        let now = self.actual.raw();
        self.actual = Fx2::from_raw(if now < target {
            now.saturating_add(steps).min(target)
        } else {
            now.saturating_sub(steps).max(target)
        });
    }

    /// The six-byte status payload. `STEAM-02` / `STEAM-03`.
    fn status(&self, at: Duration) -> [u8; STATUS_PAYLOAD_LEN] {
        let left = self.remaining(at).as_secs();
        let minutes = u8::try_from(left / 60).unwrap_or(u8::MAX);
        let seconds = u8::try_from(left % 60).unwrap_or(0);
        SteamStatus {
            actual: self.actual,
            desired: self.desired,
            state: self.state,
            timer_minutes: minutes,
            timer_seconds: seconds,
            errors: self.errors,
        }
        .payload()
    }

    /// A reply from this adapter to the master.
    ///
    /// `DEST` is the master and `SRC` is the address the master assigned. An
    /// unaddressed adapter answers from `0x00`, which is the same byte as
    /// "master" — the reason discovery routes on opcode and never on address.
    /// `ADDR-06`.
    fn reply(&self, cmd: u8, payload: &[u8]) -> Vec<u8> {
        let src = self.address.map_or(MASTER, DevAddr::get);
        raw_dtv(MASTER, src, cmd, payload)
    }

    fn drain(&mut self, at: Duration) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for _ in 0..RX_CAPACITY {
            match decode(&mut self.rx) {
                Ok(None) => break,
                Ok(Some(frame)) => {
                    if let Some(reply) = self.on_frame(&frame, at) {
                        out.push(reply);
                    }
                }
                // Line noise, a truncated frame or a bad checksum. The decoder
                // has already resynchronised on the next `SOF`.
                Err(_) => {}
            }
        }
        out
    }

    fn on_frame(&mut self, f: &DecodedDtv, at: Duration) -> Option<Vec<u8>> {
        let reply = self.dispatch(f, at);
        match self.forced.pop_front() {
            Some(bytes) => Some(bytes),
            None => reply,
        }
    }

    fn dispatch(&mut self, f: &DecodedDtv, at: Duration) -> Option<Vec<u8>> {
        // Discovery first, because it is routed on opcode. Both frames carry
        // `DEST` and `SRC` of `0x00`, so an address check here would reject the
        // handshake that assigns the address.
        match f.cmd {
            opcode::DEV_ADDRESS_OPP if self.address.is_none() => {
                // The device ID goes in the payload and nowhere else.
                return Some(self.reply(opcode::DEV_REQUEST_ADDR, &[self.device_id.get()]));
            }
            opcode::DEV_ADDRESS_OPP => return None,
            opcode::DEV_ASSIGN_ADDR => {
                if let [addr, ..] = f.payload.as_slice() {
                    self.address = DevAddr::new(*addr).ok();
                }
                // The documented handshake ends here: step 3 has no reply.
                return None;
            }
            _ => {}
        }

        // Everything else is addressed, and an adapter that has not been given
        // an address answers nothing.
        let addr = self.address?;
        if f.dest != addr.get() {
            return None;
        }

        if let Some(code) = self.next_error.take() {
            return Some(self.reply(opcode::ERROR, &[code]));
        }
        if let Some(err) = self.next_nak.take() {
            return Some(self.reply(opcode::DEV_NAK, &[err]));
        }

        match f.cmd {
            opcode::GET_DEV_STATUS => {
                let payload = self.status(at);
                Some(self.reply(self.status_carrier.opcode(), &payload))
            }
            opcode::SET_DEV_PARAM => self.on_set_param(f, at),
            opcode::CLEAR_FAULT_FLAGS => {
                self.errors = SteamErrorFlags::empty();
                self.ack()
            }
            // Every denied opcode lands here. A denied opcode reaching the
            // adapter is a bug in the daemon, and a NAK puts it on the
            // transcript instead of leaving silence to be read as a timeout.
            _ => Some(self.reply(opcode::DEV_NAK, &[self.nak_error])),
        }
    }

    /// The three-field steam block: desired temperature, operation state, timer
    /// minutes, written atomically. `STEAM-06`.
    fn on_set_param(&mut self, f: &DecodedDtv, at: Duration) -> Option<Vec<u8>> {
        let [temp, state, minutes] = f.payload.as_slice() else {
            // A payload of any other length is refused rather than read
            // positionally: the field widths are inference and a five-byte read
            // of a six-byte layout slides every field.
            return Some(self.reply(opcode::DEV_NAK, &[self.nak_error]));
        };

        self.desired = Fx2::from_raw(*temp);
        // Decoded, not commanded. `SteamStateByte` carries `PowerClean` because
        // observing a cycle this master did not start is exactly what
        // `STEAM-12` requires; `SteamOpState`, which is what the encoder
        // accepts, has no such variant and cannot produce this byte.
        self.state = SteamStateByte::decode(*state);
        self.session_len = Duration::from_secs(u64::from(*minutes).saturating_mul(60));

        self.session_end = if self.state.is_producing() && !self.session_len.is_zero() {
            Some(at.saturating_add(self.session_len))
        } else {
            None
        };
        self.ramped_at = at;
        self.ack()
    }

    fn ack(&self) -> Option<Vec<u8>> {
        match self.write_ack {
            WriteAck::DevAck => Some(self.reply(opcode::DEV_ACK, &[])),
            WriteAck::Silent => None,
        }
    }
}

impl DeviceModel for SteamAdapterModel {
    fn on_bytes(&mut self, bytes: &[u8], at: Duration) -> Vec<Vec<u8>> {
        self.advance(at);
        self.rx.extend(bytes);
        self.drain(at)
    }

    fn tick(&mut self, at: Duration) -> Vec<Vec<u8>> {
        self.advance(at);
        // `STATUS_UPDATE` (`0x31`) is documented as "device reports its current
        // status", which could be unprompted. Nothing says when, so this model
        // never volunteers one. `[?]`
        Vec::new()
    }
}

/// A shared handle to an adapter behind a [`Wire`](crate::wire::Wire).
///
/// Same reason as [`ValveHandle`](crate::valve::ValveHandle): the transcript is
/// the oracle, but faults have to be injected part-way through a run.
#[derive(Clone, Debug)]
pub struct SteamHandle(Arc<Mutex<SteamAdapterModel>>);

impl SteamHandle {
    #[must_use]
    pub fn new(model: SteamAdapterModel) -> Self {
        Self(Arc::new(Mutex::new(model)))
    }

    /// Read or change the model.
    pub fn with<R>(&self, f: impl FnOnce(&mut SteamAdapterModel) -> R) -> R {
        f(&mut self.guard())
    }

    /// A poisoned lock still yields the model: a panic in one test's device
    /// must not turn every later assertion into a second panic that hides it.
    fn guard(&self) -> MutexGuard<'_, SteamAdapterModel> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl DeviceModel for SteamHandle {
    fn on_bytes(&mut self, bytes: &[u8], at: Duration) -> Vec<Vec<u8>> {
        self.guard().on_bytes(bytes, at)
    }

    fn tick(&mut self, at: Duration) -> Vec<Vec<u8>> {
        self.guard().tick(at)
    }
}

/// Build a DTV+ frame from raw parts: header, checksum, byte stuffing,
/// delimiters, in that order.
///
/// **Unreachable from the daemon.** `SteamEncoder` produces eight operations
/// resolving to five opcodes and speaks only master to device; this builds any
/// opcode, in either direction, with any payload — including the `0xCC`
/// operation state the encoder is built to make unspellable.
///
/// Stuffing is the last step, so the checksum never covers an escape byte.
/// `FRAME-09`.
#[must_use]
pub fn raw_dtv(dest: u8, src: u8, cmd: u8, payload: &[u8]) -> Vec<u8> {
    let chk = checksum(dest, src, cmd, payload);
    let mut logical = Vec::with_capacity(payload.len() + 4);
    logical.push(dest);
    logical.push(src);
    logical.push(cmd);
    logical.extend_from_slice(payload);
    logical.push(chk);
    stuff(&logical)
}

/// The same frame with a deliberately wrong checksum.
#[must_use]
pub fn raw_dtv_bad_checksum(dest: u8, src: u8, cmd: u8, payload: &[u8]) -> Vec<u8> {
    let chk = checksum(dest, src, cmd, payload).wrapping_add(1);
    let mut logical = Vec::with_capacity(payload.len() + 4);
    logical.push(dest);
    logical.push(src);
    logical.push(cmd);
    logical.extend_from_slice(payload);
    logical.push(chk);
    stuff(&logical)
}

/// A frame with the delimiters in place and **no byte stuffing at all**.
///
/// A reserved byte in the payload then terminates or resynchronises the frame
/// early, which is the failure a decoder that skips unstuffing produces. No
/// correct encoder can emit this.
#[must_use]
pub fn raw_dtv_unstuffed(dest: u8, src: u8, cmd: u8, payload: &[u8]) -> Vec<u8> {
    let chk = checksum(dest, src, cmd, payload);
    let mut v = Vec::with_capacity(payload.len() + 6);
    v.push(SOF);
    v.push(dest);
    v.push(src);
    v.push(cmd);
    v.extend_from_slice(payload);
    v.push(chk);
    v.push(EOF);
    v
}

fn stuff(logical: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(logical.len() * 2 + 2);
    v.push(SOF);
    for b in logical {
        if is_reserved(*b) {
            v.push(kdtv_proto::dtv::ESC);
        }
        v.push(*b);
    }
    v.push(EOF);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::{Direction as Dir, Entry};
    use crate::wire::{Wire, WireFault};
    use kdtv_proto::dtv::{DiscoveryStep, SteamEncoder, SteamOp, SteamOpState, decode_frame};
    use kdtv_proto::fixtures::FixtureSet;
    use kdtv_proto::gate::TransmitAuthority;
    use kdtv_proto::saturn::{DiscoveryToken, LinkPhase};
    use kdtv_units::{LinkKind, SteamMinutes, SteamSetpoint};
    use std::fs::{File, OpenOptions};
    use std::io::Write;

    fn encoder() -> SteamEncoder {
        SteamEncoder::new(&TransmitAuthority::emulator_only(FixtureSet::embedded()))
    }

    fn setpoint(f: u8) -> SteamSetpoint {
        SteamSetpoint::try_new(Fx2::from_raw(f)).unwrap()
    }

    struct Link {
        wire: Wire,
        steam: SteamHandle,
        daemon: File,
        now: Duration,
    }

    impl Link {
        fn new(model: SteamAdapterModel) -> Self {
            let steam = SteamHandle::new(model);
            let wire = Wire::new(Box::new(steam.clone())).expect("open a wire");
            let daemon = OpenOptions::new()
                .read(true)
                .write(true)
                .open(wire.follower_path())
                .expect("open the follower");
            Self {
                wire,
                steam,
                daemon,
                now: Duration::ZERO,
            }
        }

        fn run(&mut self, span: Duration) {
            let until = self.now.saturating_add(span);
            while self.now < until {
                self.now = self.now.saturating_add(Duration::from_millis(1));
                self.wire.pump(self.now).expect("pump");
            }
        }

        fn exchange(&mut self, bytes: &[u8]) {
            self.daemon.write_all(bytes).expect("daemon transmits");
            self.daemon.flush().ok();
            self.run(Duration::from_millis(60));
        }

        fn replies(&self) -> Vec<String> {
            self.wire
                .transcript()
                .entries()
                .iter()
                .filter(|e| e.direction == Dir::DeviceToDaemon)
                .map(Entry::hex)
                .collect()
        }

        fn reply_bytes(&self) -> Vec<Vec<u8>> {
            self.wire
                .transcript()
                .entries()
                .iter()
                .filter(|e| e.direction == Dir::DeviceToDaemon)
                .map(|e| e.bytes.clone())
                .collect()
        }

        fn last_reply(&self) -> String {
            self.replies().last().cloned().unwrap_or_default()
        }
    }

    fn encode(enc: &SteamEncoder, op: &SteamOp, phase: LinkPhase) -> Vec<u8> {
        enc.encode(DevAddr::REFERENCE, op, phase, None)
            .unwrap()
            .bytes()
            .to_vec()
    }

    /// Walk the documented three-step handshake and leave the adapter at
    /// `0x03`.
    fn discover(link: &mut Link, enc: &SteamEncoder) {
        let token = DiscoveryToken::mint(LinkKind::Steam, LinkPhase::Discovery).unwrap();
        for step in [
            DiscoveryStep::AddressOpportunity,
            DiscoveryStep::AssignAddress(DevAddr::REFERENCE),
        ] {
            let f = enc
                .encode(
                    DevAddr::REFERENCE,
                    &SteamOp::Discovery(step),
                    LinkPhase::Discovery,
                    Some(&token),
                )
                .unwrap();
            link.exchange(f.bytes());
        }
    }

    // -- Discovery -----------------------------------------------------------

    /// From cold: the adapter answers the broadcast with its device ID, takes
    /// the address it is assigned, and says nothing to the assignment itself.
    #[test]
    fn discovery_from_cold_assigns_an_address() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        assert_eq!(link.steam.with(|s| s.address()), None, "boots unaddressed");

        discover(&mut link, &enc);

        let replies = link.replies();
        assert_eq!(replies.len(), 1, "only step 2 comes back: {replies:?}");
        // dtv-plus-protocol.md Example 3 step 2, byte for byte.
        assert_eq!(replies[0], "88 00 00 06 05 F5 55");
        assert_eq!(link.steam.with(|s| s.address()), Some(DevAddr::REFERENCE));

        // The device ID stayed in the payload. It is not, and never becomes,
        // the destination. CORRECTIONS.md item 2.
        let decoded = decode_frame(&link.reply_bytes()[0]).unwrap();
        assert_eq!(
            decoded.requested_device_id(),
            Some(DeviceId::STEAM_GENERATOR)
        );
        assert_eq!(decoded.dest, MASTER);
    }

    #[test]
    fn an_addressed_adapter_ignores_a_later_address_opportunity() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        let before = link.replies().len();

        let token = DiscoveryToken::mint(LinkKind::Steam, LinkPhase::Discovery).unwrap();
        let f = enc
            .encode(
                DevAddr::REFERENCE,
                &SteamOp::Discovery(DiscoveryStep::AddressOpportunity),
                LinkPhase::Discovery,
                Some(&token),
            )
            .unwrap();
        link.exchange(f.bytes());
        assert_eq!(link.replies().len(), before);
    }

    #[test]
    fn an_unaddressed_adapter_answers_nothing_else() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));
        assert!(link.replies().is_empty());
    }

    // -- Status --------------------------------------------------------------

    #[test]
    fn a_status_poll_carries_both_temperatures_and_the_error_byte() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);

        link.steam
            .with(|s| s.inject_errors(SteamErrorFlags::OVERTEMPERATURE));
        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));

        let raw = link.reply_bytes().last().cloned().unwrap();
        let (carrier, status) = decode_frame(&raw)
            .unwrap()
            .steam_status(DevAddr::REFERENCE)
            .unwrap();
        assert_eq!(carrier, StatusCarrier::DevAck);
        assert_eq!(status.actual, Fx2::from_raw(140));
        assert_eq!(status.desired, SteamSetpoint::FACTORY_DEFAULT);
        assert_eq!(status.state, SteamStateByte::Off);
        assert_eq!(status.errors, SteamErrorFlags::OVERTEMPERATURE);
        assert!(status.errors.requires_immediate_off());
    }

    /// All three candidate carriers decode. Which one a real adapter uses is
    /// packet-capture work, not a decision this crate makes. `STEAM-04`.
    #[test]
    fn every_candidate_status_carrier_decodes() {
        let enc = encoder();
        for carrier in StatusCarrier::ALL {
            let mut link =
                Link::new(SteamAdapterModel::new(WriteAck::DevAck).with_status_carrier(carrier));
            discover(&mut link, &enc);
            link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));
            let raw = link.reply_bytes().last().cloned().unwrap();
            let (got, _) = decode_frame(&raw)
                .unwrap()
                .steam_status(DevAddr::REFERENCE)
                .unwrap();
            assert_eq!(got, carrier);
        }
    }

    /// An error byte carrying only undocumented bits is still a fault.
    /// `STEAM-14`.
    #[test]
    fn undocumented_error_bits_survive_the_round_trip() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        link.steam
            .with(|s| s.inject_errors(SteamErrorFlags::from_bits_retain(0x81)));
        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));

        let raw = link.reply_bytes().last().cloned().unwrap();
        let (_, status) = decode_frame(&raw)
            .unwrap()
            .steam_status(DevAddr::REFERENCE)
            .unwrap();
        assert_eq!(status.errors.bits(), 0x81);
        assert!(status.errors.is_fault());
        assert_eq!(status.errors.reserved_bits_set(), 0x81);
    }

    // -- Writes and the acknowledgement contradiction -------------------------

    #[test]
    fn a_start_sets_the_setpoint_the_state_and_the_session_timer() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);

        link.exchange(&encode(
            &enc,
            &SteamOp::Start {
                temp: setpoint(240),
                minutes: SteamMinutes::try_new(5).unwrap(),
            },
            LinkPhase::ReadyOff,
        ));
        // 88 00 03 35 C8 55 — a bare DEV_ACK from 0x03 to the master.
        assert_eq!(link.last_reply(), "88 00 03 35 C8 55");
        assert_eq!(link.steam.with(|s| s.desired()), Fx2::from_raw(240));
        assert_eq!(link.steam.with(|s| s.state()), SteamStateByte::On);
        assert!(link.steam.with(|s| s.is_producing()));

        // Five minutes, counting from the moment the write was decoded rather
        // than from the moment the assertion runs, so the bound is a window.
        let now = link.now;
        let left = link.steam.with(|s| s.remaining(now));
        assert!(
            left <= Duration::from_secs(300) && left > Duration::from_secs(299),
            "{left:?}"
        );
    }

    /// The contradiction, exercised both ways. Under `Silent` a master waiting
    /// for an acknowledgement gets nothing at all.
    #[test]
    fn set_dev_param_acknowledgement_is_configurable_and_neither_is_chosen() {
        let enc = encoder();
        let op = SteamOp::Start {
            temp: setpoint(220),
            minutes: SteamMinutes::try_new(3).unwrap(),
        };

        let mut acked = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        discover(&mut acked, &enc);
        let before = acked.replies().len();
        acked.exchange(&encode(&enc, &op, LinkPhase::ReadyOff));
        assert_eq!(acked.replies().len(), before + 1, "dtv-plus-protocol.md");

        let mut silent = Link::new(SteamAdapterModel::new(WriteAck::Silent));
        discover(&mut silent, &enc);
        let before = silent.replies().len();
        silent.exchange(&encode(&enc, &op, LinkPhase::ReadyOff));
        assert_eq!(silent.replies().len(), before, "steam-generator.md");
        // The write still took effect, which is why the source says to confirm
        // it with a status poll rather than with an acknowledgement.
        assert_eq!(silent.steam.with(|s| s.state()), SteamStateByte::On);
        silent.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::Running));
        assert_eq!(silent.replies().len(), before + 1);
    }

    #[test]
    fn a_stop_clears_the_state_and_the_session_timer() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        link.exchange(&encode(
            &enc,
            &SteamOp::Start {
                temp: setpoint(240),
                minutes: SteamMinutes::try_new(5).unwrap(),
            },
            LinkPhase::ReadyOff,
        ));
        link.exchange(&encode(
            &enc,
            &SteamOp::Stop {
                temp: setpoint(240),
                minutes: SteamMinutes::try_new(5).unwrap(),
            },
            LinkPhase::Running,
        ));
        assert_eq!(link.steam.with(|s| s.state()), SteamStateByte::Off);
        assert_eq!(
            link.steam.with(|s| s.remaining(Duration::ZERO)),
            Duration::ZERO
        );
    }

    /// The generator's own automatic shutoff — the only backstop that survives
    /// the service dying, which is why `SteamMinutes` has no zero.
    #[test]
    fn the_session_timer_turns_the_generator_off_by_itself() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        link.exchange(&encode(
            &enc,
            &SteamOp::Start {
                temp: setpoint(240),
                minutes: SteamMinutes::try_new(1).unwrap(),
            },
            LinkPhase::ReadyOff,
        ));
        assert!(link.steam.with(|s| s.is_producing()));

        link.run(Duration::from_secs(61));
        assert_eq!(link.steam.with(|s| s.state()), SteamStateByte::Off);

        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::Running));
        let raw = link.reply_bytes().last().cloned().unwrap();
        let (_, status) = decode_frame(&raw)
            .unwrap()
            .steam_status(DevAddr::REFERENCE)
            .unwrap();
        assert_eq!(status.state, SteamStateByte::Off);
        assert_eq!((status.timer_minutes, status.timer_seconds), (0, 0));
    }

    #[test]
    fn a_setpoint_change_carries_the_state_forward() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        link.exchange(&encode(
            &enc,
            &SteamOp::SetTemperature {
                temp: setpoint(200),
                minutes: SteamMinutes::try_new(10).unwrap(),
                state: SteamOpState::On,
            },
            LinkPhase::Running,
        ));
        assert_eq!(link.steam.with(|s| s.desired()), Fx2::from_raw(200));
        assert_eq!(link.steam.with(|s| s.state()), SteamStateByte::On);
    }

    #[test]
    fn clearing_fault_flags_empties_the_error_byte() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        link.steam
            .with(|s| s.inject_errors(SteamErrorFlags::THERMISTOR));
        link.exchange(&encode(&enc, &SteamOp::ClearFaults, LinkPhase::ReadyOff));
        assert_eq!(link.steam.with(|s| s.errors()), SteamErrorFlags::empty());
    }

    // -- Refusals ------------------------------------------------------------

    #[test]
    fn a_nak_comes_back_with_its_error_byte() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        link.steam.with(|s| s.nak_next(0x2A));
        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));

        let raw = link.reply_bytes().last().cloned().unwrap();
        let decoded = decode_frame(&raw).unwrap();
        assert_eq!(decoded.cmd, opcode::DEV_NAK);
        assert_eq!(decoded.nak_error_byte(), Some(0x2A));
        // And it is not a status frame, so nothing reads it as one.
        assert!(decoded.steam_status(DevAddr::REFERENCE).is_err());
    }

    #[test]
    fn an_error_report_comes_back_with_its_code() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        link.steam.with(|s| s.error_next(0x11));
        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));
        let decoded = decode_frame(&link.reply_bytes().last().cloned().unwrap()).unwrap();
        assert_eq!(decoded.cmd, opcode::ERROR);
        assert_eq!(decoded.payload.as_slice(), &[0x11]);
    }

    #[test]
    fn a_denied_opcode_is_nakked_rather_than_obeyed() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        // 0x80 REBOOT. `SteamOp` has no variant for it, so this frame can only
        // exist here. CORRECTIONS.md item 8.
        link.exchange(&raw_dtv(
            DevAddr::REFERENCE.get(),
            MASTER,
            opcode::REBOOT,
            &[],
        ));
        let decoded = decode_frame(&link.reply_bytes().last().cloned().unwrap()).unwrap();
        assert_eq!(decoded.cmd, opcode::DEV_NAK);
    }

    // -- Power clean ---------------------------------------------------------

    /// `STEAM-12`. The encoder cannot spell `0xCC`, so the only way in is a raw
    /// frame — which is the real situation: a cycle someone else started, that
    /// this service must see and not interrupt.
    #[test]
    fn a_power_clean_started_elsewhere_is_observable_but_not_commandable() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);

        // No `SteamOpState` variant produces 0xCC. CORRECTIONS.md item 1.
        for state in SteamOpState::ALL {
            assert_ne!(state.wire(), SteamOpState::POWER_CLEAN_BYTE);
        }
        link.exchange(&raw_dtv(
            DevAddr::REFERENCE.get(),
            MASTER,
            opcode::SET_DEV_PARAM,
            &[220, SteamOpState::POWER_CLEAN_BYTE, 10],
        ));
        assert_eq!(link.steam.with(|s| s.state()), SteamStateByte::PowerClean);

        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::Running));
        let raw = link.reply_bytes().last().cloned().unwrap();
        let (_, status) = decode_frame(&raw)
            .unwrap()
            .steam_status(DevAddr::REFERENCE)
            .unwrap();
        assert_eq!(status.state, SteamStateByte::PowerClean);
        assert_eq!(
            status.ui_status(),
            kdtv_proto::dtv::SteamUiStatus::PowerCleanActive
        );
    }

    // -- Framing -------------------------------------------------------------

    #[test]
    fn raw_dtv_reproduces_the_documented_frames() {
        // Example 3 step 1: the address opportunity broadcast.
        assert_eq!(
            raw_dtv(0xFF, 0x00, opcode::DEV_ADDRESS_OPP, &[]),
            vec![0x88, 0xFF, 0x00, 0x05, 0xFC, 0x55]
        );
        // Example 3 step 3: assign 0x03.
        assert_eq!(
            raw_dtv(0x00, 0x00, opcode::DEV_ASSIGN_ADDR, &[0x03]),
            vec![0x88, 0x00, 0x00, 0x07, 0x03, 0xF6, 0x55]
        );
        // Example 2, after the document corrects its own checksum from 0x92.
        // The payload byte 0x55 is stuffed; the checksum 0x73 is not.
        assert_eq!(
            raw_dtv(0x03, 0x00, opcode::SET_DEV_PARAM, &[0x01, 0x55]),
            vec![0x88, 0x03, 0x00, 0x34, 0x01, 0xAA, 0x55, 0x73, 0x55]
        );
        // Without stuffing the same frame ends early at the payload's 0x55.
        assert_eq!(
            raw_dtv_unstuffed(0x03, 0x00, opcode::SET_DEV_PARAM, &[0x01, 0x55]),
            vec![0x88, 0x03, 0x00, 0x34, 0x01, 0x55, 0x73, 0x55]
        );
    }

    #[test]
    fn a_corrupt_request_gets_no_answer_and_the_next_one_still_does() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        let before = link.replies().len();

        link.exchange(&raw_dtv_bad_checksum(
            DevAddr::REFERENCE.get(),
            MASTER,
            opcode::GET_DEV_STATUS,
            &[],
        ));
        assert_eq!(link.replies().len(), before);

        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));
        assert_eq!(link.replies().len(), before + 1);
    }

    /// The `SOF` resynchronisation survives echo bleed, even though the
    /// selected converters present none. `CORRECTIONS.md` item 3.
    #[test]
    fn echo_and_leading_garbage_do_not_stop_the_adapter_answering() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);

        let mut noisy = vec![0x00, 0xFF, 0x12];
        noisy.extend_from_slice(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));
        link.wire.inject(WireFault::Echo);
        link.exchange(&noisy);

        assert!(
            link.reply_bytes()
                .iter()
                .filter_map(|b| decode_frame(b).ok())
                .any(|d| d.steam_status(DevAddr::REFERENCE).is_ok()),
            "{:?}",
            link.replies()
        );
    }

    #[test]
    fn a_forced_reply_displaces_a_well_formed_one() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        link.steam
            .with(|s| s.force_next_reply(vec![0x88, 0x00, 0x03, 0x31, 0x55]));
        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));
        assert_eq!(link.last_reply(), "88 00 03 31 55");
    }

    // -- Ramp ----------------------------------------------------------------

    #[test]
    fn the_actual_temperature_moves_towards_the_setpoint_when_a_ramp_is_set() {
        let mut link = Link::new(
            SteamAdapterModel::new(WriteAck::DevAck).with_ramp(Some(Duration::from_secs(1))),
        );
        let enc = encoder();
        discover(&mut link, &enc);
        link.exchange(&encode(
            &enc,
            &SteamOp::Start {
                temp: setpoint(200),
                minutes: SteamMinutes::try_new(10).unwrap(),
            },
            LinkPhase::ReadyOff,
        ));
        let before = link.steam.with(|s| s.actual());
        link.run(Duration::from_secs(20));
        let after = link.steam.with(|s| s.actual());
        assert!(after.raw() > before.raw(), "{before:?} -> {after:?}");
        assert!(after.raw() <= 200);
    }

    #[test]
    fn with_no_ramp_the_actual_temperature_is_whatever_it_was_set_to() {
        let mut link = Link::new(SteamAdapterModel::new(WriteAck::DevAck));
        let enc = encoder();
        discover(&mut link, &enc);
        link.steam.with(|s| s.set_actual(Fx2::from_raw(190)));
        link.exchange(&encode(
            &enc,
            &SteamOp::Start {
                temp: setpoint(240),
                minutes: SteamMinutes::try_new(10).unwrap(),
            },
            LinkPhase::ReadyOff,
        ));
        link.run(Duration::from_secs(60));
        assert_eq!(link.steam.with(|s| s.actual()), Fx2::from_raw(190));
    }

    #[test]
    fn a_device_id_override_reaches_the_wire_without_becoming_an_address() {
        let mut link = Link::new(
            SteamAdapterModel::new(WriteAck::DevAck).with_device_id(DeviceId::RAIN_PANEL),
        );
        let enc = encoder();
        discover(&mut link, &enc);
        let decoded = decode_frame(&link.reply_bytes()[0]).unwrap();
        assert_eq!(decoded.requested_device_id(), Some(DeviceId::RAIN_PANEL));
        // Its own address is the one the master assigned, not the ID it sent.
        assert_eq!(link.steam.with(|s| s.address()), Some(DevAddr::REFERENCE));
        assert_eq!(link.steam.with(|s| s.device_id()), DeviceId::RAIN_PANEL);
    }

    #[test]
    fn a_preaddressed_adapter_answers_a_poll_immediately() {
        let mut link =
            Link::new(SteamAdapterModel::new(WriteAck::DevAck).preaddressed(DevAddr::REFERENCE));
        let enc = encoder();
        link.exchange(&encode(&enc, &SteamOp::ReadStatus, LinkPhase::ReadyOff));
        assert_eq!(link.replies().len(), 1);
    }
}
