//! Deterministic stand-ins for the four things this crate takes from the
//! platform, plus a valve that answers on a pipe.
//!
//! **`kdtv-emulator` is deliberately not a dependency**, in dev-dependencies or
//! anywhere else: `cargo xtask audit-graph` asserts the shipped daemon cannot
//! reach the only crate that can build arbitrary or malformed frames, and a
//! dev-dependency here would be a path to it. So the fakes live here, and the
//! end-to-end suite against the real device models is a separate crate.
//!
//! The valve below is not a device model. It answers each read with a payload of
//! the length `kdtv-proto`'s own response-length table demands, and acknowledges
//! each write by echoing its control byte. That is enough to drive the boot
//! sequence and a session, and it is explicitly **not evidence about a Kohler
//! valve** — everything about this protocol in this workspace is tier `[C]`.
//!
//! # Why the valve lives inside the pipe
//!
//! A half-duplex master-slave bus answers a request and is otherwise silent, so
//! the reply is queued by the write that asked for it. Nothing races the control
//! loop and no test needs a responder task; a silent valve simply queues
//! nothing, which is exactly what a response timeout is.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;

use kdtv_hal::{BoxedFuture, Clock, IdError, IdStore, LinkIoError, WallClock, Watchdog};
use kdtv_proto::saturn::{MasterAddr, SYNC1, SYNC2, ValveAddr, checksum, opcode};
use kdtv_telemetry::{Monotonic, NtpSync};
use kdtv_units::{BootId, CommandId, LinkKind, PiBootId};

use crate::port::Pipe;

// ---------------------------------------------------------------- clock

/// A clock over tokio's paused time.
///
/// Monotonic readings come from `tokio::time::Instant`, so under
/// `#[tokio::test(start_paused = true)]` a twenty-minute session costs
/// microseconds. The wall clock is derived from the same reading rather than
/// from `SystemTime`, which keeps the whole test deterministic and keeps this
/// file clear of the call the workspace denies.
#[derive(Debug)]
pub(crate) struct FakeClock {
    origin: tokio::time::Instant,
    wall_base: i64,
    ntp: NtpSync,
}

impl FakeClock {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            origin: tokio::time::Instant::now(),
            wall_base: 1_756_500_000,
            ntp: NtpSync::Synchronised,
        })
    }
}

impl Clock for FakeClock {
    fn monotonic(&self) -> Monotonic {
        let elapsed = tokio::time::Instant::now().duration_since(self.origin);
        Monotonic::from_nanos(u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX))
    }

    fn wall(&self) -> WallClock {
        let seconds = i64::try_from(self.monotonic().as_nanos() / 1_000_000_000).unwrap_or(0);
        let at = jiff::Timestamp::from_second(self.wall_base.saturating_add(seconds))
            .unwrap_or(jiff::Timestamp::UNIX_EPOCH);
        WallClock::new(at, self.ntp)
    }

    fn sleep_until(&self, deadline: Monotonic) -> BoxedFuture<'static, ()> {
        let at = self
            .origin
            .checked_add(Duration::from_nanos(deadline.as_nanos()))
            .unwrap_or_else(tokio::time::Instant::now);
        Box::pin(async move { tokio::time::sleep_until(at).await })
    }
}

// ---------------------------------------------------------------- watchdog

/// Counts pets, so a test can assert the loop is the thing doing the petting.
#[derive(Debug)]
pub(crate) struct FakeWatchdog {
    pets: Mutex<u64>,
    ready: Mutex<bool>,
    interval: Option<Duration>,
}

impl FakeWatchdog {
    pub(crate) fn new(interval: Option<Duration>) -> Arc<Self> {
        Arc::new(Self {
            pets: Mutex::new(0),
            ready: Mutex::new(false),
            interval,
        })
    }

    pub(crate) fn pets(&self) -> u64 {
        *self.pets.lock().unwrap()
    }

    pub(crate) fn is_ready(&self) -> bool {
        *self.ready.lock().unwrap()
    }
}

impl Watchdog for FakeWatchdog {
    fn notify_ready(&self) {
        *self.ready.lock().unwrap() = true;
    }

    fn pet(&self) {
        *self.pets.lock().unwrap() += 1;
    }

    fn interval(&self) -> Option<Duration> {
        self.interval
    }
}

// ---------------------------------------------------------------- ids

/// In-memory counters. The real one `fsync`s; nothing here needs to.
#[derive(Debug)]
pub(crate) struct FakeIds {
    boot: u64,
    next: Mutex<u64>,
}

impl FakeIds {
    pub(crate) const fn new(boot: u64) -> Self {
        Self {
            boot,
            next: Mutex::new(1),
        }
    }
}

impl IdStore for FakeIds {
    fn begin_boot(&self) -> Result<BootId, IdError> {
        Ok(BootId(self.boot))
    }

    fn next_command(&self) -> Result<CommandId, IdError> {
        let mut next = self.next.lock().unwrap();
        let id = *next;
        *next += 1;
        Ok(CommandId(id))
    }

    fn pi_boot_id(&self) -> Result<PiBootId, IdError> {
        Ok(PiBootId("test-pi-boot".into()))
    }
}

// ---------------------------------------------------------------- valve

/// A valve that answers on a bus, well enough to walk the boot sequence.
#[derive(Clone, Debug)]
pub(crate) struct Valve {
    pub(crate) master: MasterAddr,
    pub(crate) address: ValveAddr,
    pub(crate) firmware: u8,
    /// When set, this valve answers nothing — which is what a response timeout
    /// is, and what four of the five discovery probes must see anyway.
    pub(crate) silent: bool,
    /// The fault bitmap the `0x0F` read returns.
    pub(crate) faults: u16,
    /// The temperature the valve reports, as `Cx2`.
    pub(crate) temperature: u8,
}

impl Valve {
    pub(crate) const fn new(master: MasterAddr, address: ValveAddr, firmware: u8) -> Self {
        Self {
            master,
            address,
            firmware,
            silent: false,
            faults: 0,
            temperature: 76,
        }
    }

    /// The reply to one transmitted frame, or `None` for a silent valve or an
    /// address this valve does not answer to.
    fn answer(&self, transmitted: &[u8]) -> Option<Vec<u8>> {
        if self.silent {
            return None;
        }
        let destination = *transmitted.get(2)?;
        let control = *transmitted.get(3)?;
        if destination != self.address.get() {
            return None;
        }
        let data = self.payload(control);
        let address = self.master.byte();
        let length = u8::try_from(data.len()).unwrap_or(0);
        let mut out = vec![SYNC1, SYNC2, address, control, length];
        out.extend_from_slice(&data);
        out.push(checksum(address, control, length, &data));
        Some(out)
    }

    fn payload(&self, control: u8) -> Vec<u8> {
        match control {
            opcode::READ_FIRMWARE_TYPE => vec![self.firmware],
            opcode::READ_FIRMWARE_VERSION => vec![0x00, 0x0C, 0x00],
            opcode::READ_SERIAL_NUMBER => vec![1, 2, 3, 4, 5, 6],
            opcode::READ_CALIBRATION => vec![173, 0, 0, 0, 0, 0, 0, 0],
            opcode::READ_CONFIGURATION => vec![0, 0, 0, 0, 0, 0],
            opcode::READ_TEMPERATURE => vec![self.temperature, 0],
            opcode::READ_FAULT_FLAGS => self.faults.to_be_bytes().to_vec(),
            opcode::READ_OUTLET_STATES => vec![0x00, 0x00],
            // Every write is acknowledged by echoing its control byte with no
            // payload, which is what the decoder's response-length table
            // expects for one.
            _ => Vec::new(),
        }
    }
}

// ---------------------------------------------------------------- pipe

/// What a [`FakePipe`] should do next when read.
#[derive(Debug)]
enum Reply {
    Bytes(Vec<u8>),
    Error(LinkIoError),
}

#[derive(Debug, Default)]
struct ScriptState {
    replies: VecDeque<Reply>,
    written: Vec<u8>,
    frames: Vec<Vec<u8>>,
    valve: Option<Valve>,
    closed: bool,
    /// Set to fail the next write, so a transmit error can be exercised.
    write_error: Option<LinkIoError>,
    /// Frames written with no reply yet read. On a bus whose valve answers,
    /// this must never exceed one — that is `BUS-01` measured at the wire
    /// rather than asserted about the engine.
    outstanding: usize,
    max_outstanding: usize,
}

/// The writable side of a fake pipe: what it will answer, and what it saw.
#[derive(Clone, Debug)]
pub(crate) struct PipeScript {
    inner: Arc<Mutex<ScriptState>>,
    arrived: Arc<Notify>,
}

/// The read-only side of a [`PipeScript`].
#[derive(Clone, Debug)]
pub(crate) struct PipeWatch {
    inner: Arc<Mutex<ScriptState>>,
}

impl PipeWatch {
    pub(crate) fn written(&self) -> Vec<u8> {
        self.inner.lock().unwrap().written.clone()
    }

    /// Every frame handed to the pipe, in order.
    pub(crate) fn frames(&self) -> Vec<Vec<u8>> {
        self.inner.lock().unwrap().frames.clone()
    }

    /// How many frames have been written. The number `API-06` is asserted
    /// against.
    pub(crate) fn frame_count(&self) -> usize {
        self.inner.lock().unwrap().frames.len()
    }

    /// Every control byte written, in order.
    pub(crate) fn controls(&self) -> Vec<u8> {
        self.frames()
            .iter()
            .filter_map(|frame| frame.get(3).copied())
            .collect()
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.inner.lock().unwrap().closed
    }

    /// The most frames this pipe ever held unanswered at once.
    pub(crate) fn max_outstanding(&self) -> usize {
        self.inner.lock().unwrap().max_outstanding
    }
}

impl PipeScript {
    pub(crate) fn new() -> (Self, PipeWatch) {
        let inner = Arc::new(Mutex::new(ScriptState::default()));
        (
            Self {
                inner: Arc::clone(&inner),
                arrived: Arc::new(Notify::new()),
            },
            PipeWatch { inner },
        )
    }

    /// Put a valve on the far end. Every write is answered by it.
    pub(crate) fn with_valve(self, valve: Valve) -> Self {
        self.inner.lock().unwrap().valve = Some(valve);
        self
    }

    /// Forget the outstanding-frame high-water mark, so a window can be
    /// measured after the boot sequence's silent probes.
    pub(crate) fn clear_outstanding(&self) {
        let mut state = self.inner.lock().unwrap();
        state.outstanding = 0;
        state.max_outstanding = 0;
    }

    /// Change the valve mid-test: go silent, report a fault, run hot.
    pub(crate) fn adjust(&self, change: impl FnOnce(&mut Valve)) {
        if let Some(valve) = self.inner.lock().unwrap().valve.as_mut() {
            change(valve);
        }
    }

    pub(crate) fn push_read(&self, bytes: Vec<u8>) {
        self.inner
            .lock()
            .unwrap()
            .replies
            .push_back(Reply::Bytes(bytes));
        self.arrived.notify_one();
    }

    pub(crate) fn push_read_error(&self, error: LinkIoError) {
        self.inner
            .lock()
            .unwrap()
            .replies
            .push_back(Reply::Error(error));
        self.arrived.notify_one();
    }
}

/// A byte pipe with whatever the test put behind it.
///
/// A read with nothing queued never resolves, which is what a silent bus is.
#[derive(Debug)]
pub(crate) struct FakePipe {
    link: LinkKind,
    script: PipeScript,
}

impl FakePipe {
    pub(crate) const fn new(link: LinkKind, script: PipeScript) -> Self {
        Self { link, script }
    }
}

impl Pipe for FakePipe {
    fn write_all<'a>(&'a mut self, buf: &'a [u8]) -> BoxedFuture<'a, Result<(), LinkIoError>> {
        Box::pin(async move {
            let mut state = self.script.inner.lock().unwrap();
            if let Some(error) = state.write_error.take() {
                return Err(error);
            }
            state.written.extend_from_slice(buf);
            state.frames.push(buf.to_vec());
            state.outstanding += 1;
            state.max_outstanding = state.max_outstanding.max(state.outstanding);
            if let Some(reply) = state.valve.as_ref().and_then(|v| v.answer(buf)) {
                state.replies.push_back(Reply::Bytes(reply));
                drop(state);
                self.script.arrived.notify_one();
            }
            Ok(())
        })
    }

    fn read<'a>(&'a mut self, buf: &'a mut [u8]) -> BoxedFuture<'a, Result<usize, LinkIoError>> {
        Box::pin(async move {
            loop {
                // Registered before the queue is checked, so a reply pushed
                // between the two is not a lost wake-up.
                let arrived = self.script.arrived.notified();
                tokio::pin!(arrived);
                arrived.as_mut().enable();

                let next = {
                    let mut state = self.script.inner.lock().unwrap();
                    let next = state.replies.pop_front();
                    if matches!(next, Some(Reply::Bytes(_))) {
                        state.outstanding = state.outstanding.saturating_sub(1);
                    }
                    next
                };
                match next {
                    Some(Reply::Bytes(bytes)) => {
                        let n = bytes.len().min(buf.len());
                        if let (Some(target), Some(source)) = (buf.get_mut(..n), bytes.get(..n)) {
                            target.copy_from_slice(source);
                        }
                        return Ok(n);
                    }
                    Some(Reply::Error(error)) => return Err(error),
                    // A silent bus: park rather than poll, so tokio's paused
                    // clock can auto-advance straight to the next real deadline.
                    None => arrived.await,
                }
            }
        })
    }

    fn close(self: Box<Self>) -> BoxedFuture<'static, Result<(), LinkIoError>> {
        let script = self.script.clone();
        let link = self.link;
        Box::pin(async move {
            script.inner.lock().unwrap().closed = true;
            tracing::debug!(link = %link, "fake pipe closed");
            Ok(())
        })
    }
}
