//! The wire between the daemon and an emulated device.
//!
//! A pseudo-terminal moves bytes instantly and perfectly. A 9600 baud
//! half-duplex RS-485 bus does neither, and the deadlines this service is built
//! around — 400 ms for a response, 320 ms for a complete message, a 525 ms tick
//! — only mean something against a transport that takes a realistic time to
//! carry a frame. So the wire sits on the leader side and models the three
//! things the kernel will not: **byte time**, **faults**, and **a record of what
//! actually crossed**.
//!
//! It is deliberately protocol-agnostic. It moves bytes and knows how long bytes
//! take; what those bytes mean is the device model's business. That keeps the
//! fault injection honest — a truncation here cannot accidentally produce a
//! well-formed frame because the wire has no idea what well-formed means.
//!
//! Time is a parameter, never a reading. Every entry point takes the current
//! offset from the start of the run, so a test can drive a hundred simulated
//! seconds in a microsecond and get exactly the same answer each time.

use crate::pty::PtyPair;
use crate::transcript::{Direction, Transcript};
use std::collections::VecDeque;
use std::io;
use std::time::Duration;

/// Bits on the wire per byte at 8N1: one start, eight data, one stop.
const BITS_PER_BYTE: u64 = 10;
/// The only baud rate this system uses, on all three links.
pub const BAUD: u64 = 9600;

/// How long `len` bytes occupy the wire at 9600 8N1.
///
/// One byte is ~1.042 ms, so a 20-byte Saturn frame — the maximum — takes about
/// 20.8 ms. That is the number the 320 ms message timeout has to be read
/// against.
#[must_use]
pub fn line_time(len: usize) -> Duration {
    let bits = u64::try_from(len)
        .unwrap_or(u64::MAX)
        .saturating_mul(BITS_PER_BYTE);
    Duration::from_nanos(bits.saturating_mul(1_000_000_000) / BAUD)
}

/// Anything that answers bytes with bytes.
///
/// Raw bytes rather than decoded frames, so the wire stays protocol-agnostic and
/// a model is free to reply with something malformed.
pub trait DeviceModel: Send {
    /// Bytes have arrived from the daemon. Return whatever should go back.
    fn on_bytes(&mut self, bytes: &[u8], at: Duration) -> Vec<Vec<u8>>;

    /// Time has passed with nothing arriving. Return anything the device emits
    /// unprompted — most devices return nothing.
    fn tick(&mut self, _at: Duration) -> Vec<Vec<u8>> {
        Vec::new()
    }
}

/// A fault to inject into the link.
///
/// These are the conditions the offline test phase is required to cover:
/// malformed lengths, checksum faults, delay, duplicates, partial frames,
/// missing responses and link loss.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WireFault {
    /// Corrupt one bit of the next device reply. Produces a checksum failure
    /// without the wire needing to know what a checksum is.
    BitFlip { byte: usize, bit: u8 },
    /// Cut the next device reply short. A partial frame, which must not be
    /// mistaken for a complete one.
    Truncate { after: usize },
    /// Send the next device reply twice.
    Duplicate,
    /// Hold the next device reply back by this long, on top of its line time.
    /// Set it past the response timeout to model a missed response.
    Delay(Duration),
    /// Swallow the next device reply entirely.
    Drop,
    /// Append junk to the next device reply, so its declared length and its
    /// actual length disagree.
    Append(Vec<u8>),
    /// Echo the daemon's own transmission back at it.
    ///
    /// A converter with automatic direction control does not do this, which is
    /// why the codec has no echo timeout. The fault exists to prove the decoder
    /// resynchronises anyway if a converter ever does.
    Echo,
    /// Stop carrying anything in either direction from now on. The link is up
    /// but dead — distinct from a hangup, which the daemon sees as an error.
    GoSilent,
}

/// One link: a pty, a device behind it, a fault queue and a transcript.
pub struct Wire {
    pty: PtyPair,
    device: Box<dyn DeviceModel>,
    faults: VecDeque<WireFault>,
    pending: VecDeque<(Duration, Vec<u8>)>,
    transcript: Transcript,
    silent: bool,
    rx: Vec<u8>,
    write_deadline: Duration,
}

impl std::fmt::Debug for Wire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wire")
            .field("follower", &self.pty.follower_path())
            .field("queued_faults", &self.faults.len())
            .field("pending_replies", &self.pending.len())
            .field("silent", &self.silent)
            .finish_non_exhaustive()
    }
}

impl Wire {
    pub fn new(device: Box<dyn DeviceModel>) -> io::Result<Self> {
        Ok(Self {
            pty: PtyPair::open()?,
            device,
            faults: VecDeque::new(),
            pending: VecDeque::new(),
            transcript: Transcript::new(),
            silent: false,
            rx: Vec::new(),
            write_deadline: PtyPair::WRITE_DEADLINE,
        })
    }

    /// How long a device reply may wait for room in the pty's output buffer
    /// before the link is declared broken.
    ///
    /// [`PtyPair::WRITE_DEADLINE`] is the only value the rig uses. The knob
    /// exists so the bound itself is testable in milliseconds rather than in
    /// seconds — an unbounded write here hangs the whole harness, so the test
    /// that proves it is bounded is worth having cheap.
    #[must_use]
    pub const fn with_write_deadline(mut self, d: Duration) -> Self {
        self.write_deadline = d;
        self
    }

    /// The device path to hand the daemon.
    #[must_use]
    pub fn follower_path(&self) -> &std::path::Path {
        self.pty.follower_path()
    }

    #[must_use]
    pub fn transcript(&self) -> &Transcript {
        &self.transcript
    }

    /// Queue a fault. Faults apply to device replies in the order queued.
    pub fn inject(&mut self, f: WireFault) {
        if f == WireFault::GoSilent {
            self.silent = true;
            return;
        }
        self.faults.push_back(f);
    }

    /// Drop the link entirely.
    ///
    /// The daemon's next read fails rather than returning nothing, which is what
    /// a USB converter being unplugged looks like. Consumes the wire, because
    /// there is no way back from it — matching the real thing.
    #[must_use]
    pub fn hangup(self) -> Transcript {
        let Self {
            pty, transcript, ..
        } = self;
        drop(pty.hangup());
        transcript
    }

    /// Move the link forward to `now`.
    ///
    /// Reads whatever the daemon has transmitted, gives it to the device, and
    /// delivers any replies whose scheduled time has arrived. Call it in a loop;
    /// it never blocks.
    pub fn pump(&mut self, now: Duration) -> io::Result<()> {
        let mut buf = [0u8; 512];
        let n = self.pty.read_available(&mut buf)?;
        if n > 0 {
            let got = buf.get(..n).unwrap_or(&[]);
            self.transcript.record(now, Direction::DaemonToDevice, got);
            self.rx.extend_from_slice(got);

            if self.take_fault(|f| matches!(f, WireFault::Echo)).is_some() {
                // Straight back out, before the device sees it.
                self.schedule(now, got.to_vec());
            }

            if !self.silent {
                let inbound = std::mem::take(&mut self.rx);
                for reply in self.device.on_bytes(&inbound, now) {
                    self.enqueue_reply(now, reply);
                }
            }
        } else if !self.silent {
            for reply in self.device.tick(now) {
                self.enqueue_reply(now, reply);
            }
        }

        self.deliver_due(now)
    }

    /// Apply the next queued fault to a reply and schedule what survives.
    fn enqueue_reply(&mut self, now: Duration, mut reply: Vec<u8>) {
        let mut extra = Duration::ZERO;
        if let Some(f) = self.faults.pop_front() {
            match f {
                WireFault::Drop => return,
                WireFault::Delay(d) => extra = d,
                WireFault::Truncate { after } => reply.truncate(after),
                WireFault::Append(mut junk) => reply.append(&mut junk),
                WireFault::BitFlip { byte, bit } => {
                    if let Some(b) = reply.get_mut(byte) {
                        *b ^= 1u8.checked_shl(u32::from(bit % 8)).unwrap_or(1);
                    }
                }
                WireFault::Duplicate => {
                    let copy = reply.clone();
                    self.schedule(now, copy);
                }
                WireFault::Echo | WireFault::GoSilent => {}
            }
        }
        self.schedule(now + extra, reply);
    }

    /// Schedule a reply, arriving after the time its bytes take on the wire.
    fn schedule(&mut self, from: Duration, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        let at = from + line_time(bytes.len());
        self.pending.push_back((at, bytes));
    }

    fn take_fault(&mut self, pred: impl Fn(&WireFault) -> bool) -> Option<WireFault> {
        let i = self.faults.iter().position(&pred)?;
        self.faults.remove(i)
    }

    fn deliver_due(&mut self, now: Duration) -> io::Result<()> {
        while let Some((at, _)) = self.pending.front() {
            if *at > now {
                break;
            }
            let Some((at, bytes)) = self.pending.pop_front() else {
                break;
            };
            if self.silent {
                continue;
            }
            self.pty.write_all_before(&bytes, self.write_deadline)?;
            self.transcript
                .record(at, Direction::DeviceToDaemon, &bytes);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::{Read, Write};

    /// Answers every inbound burst with a fixed six-byte frame.
    struct Parrot(Vec<u8>);
    impl DeviceModel for Parrot {
        fn on_bytes(&mut self, _bytes: &[u8], _at: Duration) -> Vec<Vec<u8>> {
            vec![self.0.clone()]
        }
    }

    fn wire() -> (Wire, std::fs::File) {
        let w = Wire::new(Box::new(Parrot(vec![0xAA, 0x55, 0x00, 0x01, 0x00, 0xFF])))
            .expect("open a wire");
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(w.follower_path())
            .expect("open the follower");
        (w, f)
    }

    /// Drive the wire until `until`, in steps, as the rig does.
    fn run(w: &mut Wire, until: Duration) {
        let mut t = Duration::ZERO;
        while t <= until {
            w.pump(t).expect("pump");
            t += Duration::from_millis(1);
        }
    }

    #[test]
    fn line_time_matches_the_bus() {
        // 20 bytes at 9600 8N1 is ~20.8 ms — the number the 320 ms message
        // timeout has to be read against.
        let t = line_time(20);
        assert!(
            t >= Duration::from_micros(20_800) && t <= Duration::from_micros(20_900),
            "{t:?}"
        );
        assert_eq!(line_time(0), Duration::ZERO);
    }

    #[test]
    fn a_reply_does_not_arrive_before_its_bytes_could_have() {
        let (mut w, mut f) = wire();
        f.write_all(b"\xAA\x55\x03\x02\x00\xFB")
            .expect("daemon transmits");
        f.flush().ok();
        // Six bytes take ~6.25 ms. At 3 ms nothing can have arrived yet.
        run(&mut w, Duration::from_millis(3));
        assert_eq!(
            w.transcript()
                .entries()
                .iter()
                .filter(|e| e.direction == Direction::DeviceToDaemon)
                .count(),
            0,
            "a reply arrived faster than 9600 baud allows"
        );
        run(&mut w, Duration::from_millis(20));
        let mut got = [0u8; 6];
        f.read_exact(&mut got).expect("the reply arrives");
        assert_eq!(got, [0xAA, 0x55, 0x00, 0x01, 0x00, 0xFF]);
    }

    #[test]
    fn drop_swallows_the_reply_and_the_daemon_sees_a_missed_response() {
        let (mut w, mut f) = wire();
        w.inject(WireFault::Drop);
        f.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        f.flush().ok();
        run(&mut w, Duration::from_millis(50));
        assert!(
            w.transcript()
                .entries()
                .iter()
                .all(|e| e.direction == Direction::DaemonToDevice)
        );
    }

    #[test]
    fn bit_flip_corrupts_exactly_one_bit() {
        let (mut w, mut f) = wire();
        w.inject(WireFault::BitFlip { byte: 5, bit: 0 });
        f.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        f.flush().ok();
        run(&mut w, Duration::from_millis(30));
        let mut got = [0u8; 6];
        f.read_exact(&mut got)
            .expect("a corrupted reply still arrives");
        assert_eq!(got[5], 0xFF ^ 0x01, "the checksum byte is wrong by one bit");
        assert_eq!(
            &got[..5],
            &[0xAA, 0x55, 0x00, 0x01, 0x00],
            "nothing else changed"
        );
    }

    #[test]
    fn truncate_delivers_a_partial_frame() {
        let (mut w, mut f) = wire();
        w.inject(WireFault::Truncate { after: 3 });
        f.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        f.flush().ok();
        run(&mut w, Duration::from_millis(30));
        let mut got = [0u8; 3];
        f.read_exact(&mut got).expect("the partial frame arrives");
        assert_eq!(got, [0xAA, 0x55, 0x00]);
    }

    #[test]
    fn delay_pushes_the_reply_past_a_deadline() {
        let (mut w, mut f) = wire();
        w.inject(WireFault::Delay(Duration::from_millis(500)));
        f.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        f.flush().ok();
        // Past the 400 ms response timeout, nothing has arrived.
        run(&mut w, Duration::from_millis(400));
        assert_eq!(
            w.transcript()
                .entries()
                .iter()
                .filter(|e| e.direction == Direction::DeviceToDaemon)
                .count(),
            0
        );
        run(&mut w, Duration::from_millis(520));
        assert_eq!(
            w.transcript()
                .entries()
                .iter()
                .filter(|e| e.direction == Direction::DeviceToDaemon)
                .count(),
            1
        );
    }

    #[test]
    fn duplicate_sends_the_reply_twice() {
        let (mut w, mut f) = wire();
        w.inject(WireFault::Duplicate);
        f.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        f.flush().ok();
        run(&mut w, Duration::from_millis(40));
        let mut got = [0u8; 12];
        f.read_exact(&mut got).expect("both copies arrive");
        assert_eq!(&got[..6], &got[6..]);
    }

    #[test]
    fn echo_returns_the_daemons_own_bytes_and_the_device_still_answers() {
        let (mut w, mut f) = wire();
        w.inject(WireFault::Echo);
        f.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        f.flush().ok();
        run(&mut w, Duration::from_millis(40));
        let mut got = [0u8; 12];
        f.read_exact(&mut got).expect("echo then reply");
        assert_eq!(
            &got[..6],
            b"\xAA\x55\x03\x02\x00\xFB",
            "the echo comes first"
        );
        assert_eq!(&got[6..], &[0xAA, 0x55, 0x00, 0x01, 0x00, 0xFF]);
    }

    #[test]
    fn going_silent_stops_the_link_without_hanging_it_up() {
        let (mut w, mut f) = wire();
        w.inject(WireFault::GoSilent);
        f.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        f.flush().ok();
        run(&mut w, Duration::from_millis(60));
        assert!(
            w.transcript()
                .entries()
                .iter()
                .all(|e| e.direction == Direction::DaemonToDevice),
            "a silent link carries nothing back"
        );
        // The daemon's transmission was still observed, which is what
        // distinguishes this from a hangup.
        assert!(!w.transcript().is_silent());
    }

    #[test]
    fn the_transcript_records_both_directions_with_their_times() {
        let (mut w, mut f) = wire();
        f.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        f.flush().ok();
        run(&mut w, Duration::from_millis(30));
        let e = w.transcript().entries();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].direction, Direction::DaemonToDevice);
        assert_eq!(e[1].direction, Direction::DeviceToDaemon);
        assert!(e[1].at > e[0].at, "the reply is stamped after the request");
    }
}
