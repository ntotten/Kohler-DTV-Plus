//! Three links on one simulated clock.
//!
//! A single [`crate::wire::Wire`] models one bus. The system has three,
//! and the interesting failures are the ones that cross them: a fault on zone 1
//! must stop zone 1 and leave zone 2 and steam untouched, and a shared fault
//! must take all three down. That is the assertion most likely to catch a real
//! integration bug, and it needs all three links advancing together against one
//! clock.
//!
//! Time is a parameter here too. [`Harness::run_for`] advances every link in
//! lockstep at a fixed step, so a twenty-minute session limit is exercised at
//! its real constant in a few milliseconds of wall time, and the same run
//! produces the same transcript every time.
//!
//! # Two ways to drive it, and why both exist
//!
//! [`Harness::run_for`] advances a simulated clock. Nothing sleeps, the run is
//! reproducible, and a twenty-minute session limit is exercised at its real
//! constant in milliseconds of wall time. That works because the thing on the
//! other end of the pseudo-terminal is in this process and is stepped by the
//! same loop.
//!
//! The end-to-end suite drives the real `kdtvd` binary, which is a separate
//! process on the real clock. Nothing can lockstep it, so
//! [`Harness::start_real_time`] pumps every link against elapsed wall time from
//! a background thread instead. The trade is the one you would expect: the run
//! is no longer reproducible, and a deadline can only be exercised by waiting
//! for it. Session-length spans are compressed with `[bench] session_scale` in
//! the emulated configuration rather than by scaling the wire, which
//! `kdtv-config` deliberately makes impossible — a 320 ms message timeout is a
//! property of 9600 8N1 and stays at 320 ms.

use crate::transcript::Transcript;
use crate::wire::{DeviceModel, Wire, WireFault};
use kdtv_units::{LinkKind, ZoneId};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The three links, each with its own emulated device.
#[derive(Debug)]
pub struct Harness {
    wires: Vec<(LinkKind, Wire)>,
    now: Duration,
    step: Duration,
}

impl Harness {
    /// One step per millisecond of simulated time.
    ///
    /// Finer than any deadline in the system — the shortest is the 150 ms DTV+
    /// tick — and coarse enough that a twenty-minute session is 1.2 million
    /// steps, which runs in well under a second because nothing sleeps.
    pub const DEFAULT_STEP: Duration = Duration::from_millis(1);

    /// Build a harness from one device model per link.
    ///
    /// Every link gets a wire whether or not a device is attached, because "the
    /// port is open and the device says nothing" is a distinct condition from
    /// "there is no port", and both have to be reachable.
    pub fn new(devices: Vec<(LinkKind, Box<dyn DeviceModel>)>) -> io::Result<Self> {
        let mut wires = Vec::with_capacity(devices.len());
        for (link, device) in devices {
            wires.push((link, Wire::new(device)?));
        }
        Ok(Self {
            wires,
            now: Duration::ZERO,
            step: Self::DEFAULT_STEP,
        })
    }

    #[must_use]
    pub fn with_step(mut self, step: Duration) -> Self {
        self.step = step;
        self
    }

    /// The device path to hand the daemon for each link.
    #[must_use]
    pub fn port_paths(&self) -> BTreeMap<LinkKind, PathBuf> {
        self.wires
            .iter()
            .map(|(k, w)| (*k, w.follower_path().to_path_buf()))
            .collect()
    }

    /// Simulated time since the run started.
    #[must_use]
    pub const fn now(&self) -> Duration {
        self.now
    }

    /// Advance every link to the same instant, one step at a time.
    ///
    /// In lockstep rather than link by link: advancing one link to the end
    /// before starting the next would let a zone-1 fault be handled before
    /// zone 2 had reached the same moment, which is the ordering the
    /// cross-link assertions are about.
    pub fn run_for(&mut self, span: Duration) -> io::Result<()> {
        let until = self.now.saturating_add(span);
        while self.now < until {
            self.now = self.now.saturating_add(self.step);
            for (_, w) in &mut self.wires {
                w.pump(self.now)?;
            }
        }
        Ok(())
    }

    /// Inject a fault into one link.
    pub fn inject(&mut self, link: LinkKind, fault: WireFault) {
        if let Some((_, w)) = self.wires.iter_mut().find(|(k, _)| *k == link) {
            w.inject(fault);
        }
    }

    /// One link's transcript.
    #[must_use]
    pub fn transcript(&self, link: LinkKind) -> Option<&Transcript> {
        self.wires
            .iter()
            .find(|(k, _)| *k == link)
            .map(|(_, w)| w.transcript())
    }

    /// Every link's transcript, for an assertion that spans them.
    #[must_use]
    pub fn transcripts(&self) -> BTreeMap<LinkKind, &Transcript> {
        self.wires
            .iter()
            .map(|(k, w)| (*k, w.transcript()))
            .collect()
    }

    /// Drop one link entirely, as a USB converter being unplugged does.
    ///
    /// Returns the transcript up to that moment: the wire is consumed, because
    /// there is no way back from a hangup, and the record of what crossed it is
    /// the thing worth keeping.
    pub fn hangup(&mut self, link: LinkKind) -> Option<Transcript> {
        let i = self.wires.iter().position(|(k, _)| *k == link)?;
        let (_, wire) = self.wires.remove(i);
        Some(wire.hangup())
    }

    /// Assert that every link other than `affected` carried nothing from the
    /// daemon after `since`.
    ///
    /// The scoping check, as one call. A fault scoped to one zone must leave the
    /// others alone; getting that wrong in the widening direction stops
    /// someone's shower in the other room, and in the narrowing direction leaves
    /// water running.
    ///
    /// Returns the links that were disturbed, so a failure names them.
    #[must_use]
    pub fn links_disturbed_since(&self, affected: LinkKind, since: Duration) -> Vec<LinkKind> {
        self.wires
            .iter()
            .filter(|(k, _)| *k != affected)
            .filter(|(_, w)| w.transcript().transmitted().any(|e| e.at > since))
            .map(|(k, _)| *k)
            .collect()
    }
}

impl Harness {
    /// Hand the harness to a background thread that pumps it against the real
    /// clock, and get back a handle.
    ///
    /// For driving something this process cannot step: the real daemon binary.
    /// [`Harness::run_for`] is the right call for anything in-process.
    ///
    /// The thread pumps every link, sleeps [`RunningHarness::PUMP_INTERVAL`],
    /// and repeats. It stops on [`RunningHarness::stop`], or on the first I/O
    /// error, which is kept and returned from `stop` rather than swallowed or
    /// panicked on — a write to a pseudo-terminal whose follower the daemon has
    /// closed is the ordinary end of a run, and the test decides whether that
    /// was expected.
    // `Instant::now` is denied workspace-wide so that state machines take time
    // as a parameter and stay testable without waiting. This is the one place
    // that cannot: the thing being driven is another process on the real clock,
    // and there is no reading to be handed in. The denial's purpose is intact —
    // `Wire::pump` still takes the instant as an argument, and this is the only
    // caller that computes one from a clock.
    #[allow(
        clippy::disallowed_methods,
        reason = "an out-of-process daemon runs on the real clock; see above"
    )]
    #[must_use]
    pub fn start_real_time(self) -> RunningHarness {
        let inner = Arc::new(Mutex::new(self));
        let stop = Arc::new(AtomicBool::new(false));
        let started = Instant::now();

        let pump_inner = Arc::clone(&inner);
        let pump_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !pump_stop.load(Ordering::Relaxed) {
                // Elapsed real time is what `pump` wants, and the lock is held
                // only for the pump itself so a test can inject a fault or read
                // a transcript between iterations.
                let now = started.elapsed();
                {
                    let mut h = lock(&pump_inner);
                    for (_, w) in &mut h.wires {
                        w.pump(now)?;
                    }
                    h.now = now;
                }
                std::thread::sleep(RunningHarness::PUMP_INTERVAL);
            }
            Ok(())
        });

        RunningHarness {
            inner,
            stop,
            thread: Some(thread),
            started,
        }
    }
}

/// A [`Harness`] being pumped against the real clock by a background thread.
///
/// Dropping one stops the thread. It does not join it and it discards any I/O
/// error, so a test that cares about either should call [`Self::stop`].
#[derive(Debug)]
pub struct RunningHarness {
    inner: Arc<Mutex<Harness>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<io::Result<()>>>,
    started: Instant,
}

impl RunningHarness {
    /// How long the pump thread sleeps between passes.
    ///
    /// One millisecond, matching [`Harness::DEFAULT_STEP`], because that is
    /// about one byte at 9600 8N1 and finer than every deadline in the system.
    /// The pseudo-terminal reads never block, so a shorter interval would spin
    /// and a longer one would quantise arrival times the way the 16 ms FTDI
    /// latency timer does — which is the measurement error this project already
    /// refuses to start with.
    pub const PUMP_INTERVAL: Duration = Duration::from_millis(1);

    /// Real time since the pump started. The same reading the links are pumped
    /// with, so a transcript timestamp and this are the same clock.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Do something with the harness while it is running: inject a fault, read
    /// a transcript, hang a link up.
    ///
    /// Holds the lock for the duration of the closure, which blocks the pump —
    /// so do not sleep or wait in one.
    pub fn with<R>(&self, f: impl FnOnce(&mut Harness) -> R) -> R {
        f(&mut lock(&self.inner))
    }

    /// The device path for each link, for handing to the daemon.
    #[must_use]
    pub fn port_paths(&self) -> BTreeMap<LinkKind, PathBuf> {
        self.with(|h| h.port_paths())
    }

    /// Stop the pump and join its thread.
    ///
    /// Returns whatever ended the pump, if anything did — a write to a
    /// pseudo-terminal whose follower the daemon has closed is the ordinary end
    /// of a run. Idempotent: stopping twice is `Ok(())`.
    ///
    /// The harness stays reachable through [`Self::with`] afterwards, because
    /// the transcript of a run that ended badly is the thing worth reading. It
    /// is deliberately not handed back by value: doing that would mean
    /// unwrapping the `Arc` the pump thread shares, which has no answer that is
    /// not a panic.
    pub fn stop(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Relaxed);
        match self.thread.take() {
            None => Ok(()),
            Some(t) => t
                .join()
                .unwrap_or_else(|_| Err(io::Error::other("the harness pump thread panicked"))),
        }
    }
}

impl Drop for RunningHarness {
    fn drop(&mut self) {
        drop(self.stop());
    }
}

/// A poisoned harness lock is still a harness. The pump thread returns its error
/// rather than panicking, so poisoning means a test panicked while holding the
/// lock — and the test that is already failing should be the one that reports.
fn lock(m: &Mutex<Harness>) -> MutexGuard<'_, Harness> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The three links this installation has.
#[must_use]
pub fn all_links() -> [LinkKind; 3] {
    [
        LinkKind::Zone(ZoneId::Zone1),
        LinkKind::Zone(ZoneId::Zone2),
        LinkKind::Steam,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::Direction;
    use std::fs::OpenOptions;
    use std::io::Write;

    /// Answers any inbound burst with a fixed frame.
    struct Parrot(Vec<u8>);
    impl DeviceModel for Parrot {
        fn on_bytes(&mut self, _bytes: &[u8], _at: Duration) -> Vec<Vec<u8>> {
            vec![self.0.clone()]
        }
    }

    /// Says nothing, ever. The "port open, device silent" case.
    struct Mute;
    impl DeviceModel for Mute {
        fn on_bytes(&mut self, _bytes: &[u8], _at: Duration) -> Vec<Vec<u8>> {
            Vec::new()
        }
    }

    fn harness() -> Harness {
        Harness::new(vec![
            (
                LinkKind::Zone(ZoneId::Zone1),
                Box::new(Parrot(vec![0xAA, 0x55, 0x00, 0x01, 0x00, 0xFF])),
            ),
            (
                LinkKind::Zone(ZoneId::Zone2),
                Box::new(Parrot(vec![0xAA, 0x55, 0x00, 0x02, 0x00, 0xFE])),
            ),
            (LinkKind::Steam, Box::new(Mute)),
        ])
        .expect("three wires")
    }

    /// Read from `f` until `want` bytes have arrived or `deadline` passes.
    ///
    /// Bounded, so a pump that never delivers fails in a second rather than
    /// hanging the suite — the failure mode a real-time test has and a
    /// lockstep one does not.
    // Same exception as `start_real_time`: this waits on another thread's real
    // progress, so there is no reading to be handed in.
    #[allow(
        clippy::disallowed_methods,
        reason = "waiting on a real-clock pump thread"
    )]
    fn read_up_to(f: &mut std::fs::File, want: usize, deadline: Duration) -> Vec<u8> {
        use std::io::Read;
        let until = Instant::now() + deadline;
        let mut got = Vec::new();
        while got.len() < want && Instant::now() < until {
            let mut buf = [0u8; 64];
            // A non-blocking read on a pseudo-terminal with nothing waiting
            // returns EAGAIN, and one whose peer has closed returns EIO. Both
            // mean "nothing now"; only bytes are different.
            match f.read(&mut buf) {
                Ok(n) if n > 0 => got.extend_from_slice(buf.get(..n).unwrap_or(&[])),
                Ok(_) | Err(_) => std::thread::sleep(Duration::from_millis(1)),
            }
        }
        got
    }

    fn open_follower(path: &std::path::Path) -> std::fs::File {
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .expect("the follower opens");
        let fd = std::os::fd::AsRawFd::as_raw_fd(&f);
        // Non-blocking, so `read_up_to` polls rather than parking forever on a
        // pump that is not delivering.
        nix::fcntl::fcntl(&f, nix::fcntl::F_SETFL(nix::fcntl::OFlag::O_NONBLOCK))
            .unwrap_or_else(|e| panic!("O_NONBLOCK on fd {fd}: {e}"));
        f
    }

    #[test]
    fn a_real_time_pump_answers_without_being_stepped() {
        let h = harness();
        let paths = h.port_paths();
        let mut one = open_follower(&paths[&LinkKind::Zone(ZoneId::Zone1)]);
        let mut running = h.start_real_time();

        one.write_all(&[0xAA, 0x55, 0x03, 0x21, 0x00, 0xDC])
            .expect("write to the follower");
        let got = read_up_to(&mut one, 6, Duration::from_secs(2));

        running.stop().expect("the pump ran clean");
        assert_eq!(
            got,
            vec![0xAA, 0x55, 0x00, 0x01, 0x00, 0xFF],
            "nothing called run_for; the background pump is what delivered this"
        );
        assert!(
            running.with(|h| {
                h.transcript(LinkKind::Zone(ZoneId::Zone1))
                    .is_some_and(|t| t.transmitted().any(|e| e.bytes.starts_with(&[0xAA, 0x55])))
            }),
            "the transcript records what crossed the wire"
        );
    }

    #[test]
    fn the_pump_leaves_the_other_links_alone() {
        let h = harness();
        let paths = h.port_paths();
        let mut one = open_follower(&paths[&LinkKind::Zone(ZoneId::Zone1)]);
        let _two = open_follower(&paths[&LinkKind::Zone(ZoneId::Zone2)]);
        let mut running = h.start_real_time();

        one.write_all(&[0xAA, 0x55, 0x03, 0x21, 0x00, 0xDC])
            .expect("write to the follower");
        let answered = read_up_to(&mut one, 6, Duration::from_secs(2));

        running.stop().expect("the pump ran clean");
        // The scoping assertion below is vacuously true on a pump that never
        // ran, so establish that something crossed zone 1 first.
        assert_eq!(
            answered,
            vec![0xAA, 0x55, 0x00, 0x01, 0x00, 0xFF],
            "zone 1 must actually have carried traffic for the scoping check to mean anything"
        );
        assert_eq!(
            running
                .with(|h| h.links_disturbed_since(LinkKind::Zone(ZoneId::Zone1), Duration::ZERO)),
            Vec::<LinkKind>::new(),
            "traffic on zone 1 must not appear on zone 2 or steam"
        );
    }

    #[test]
    fn a_fault_can_be_injected_while_the_pump_is_running() {
        let h = harness();
        let paths = h.port_paths();
        let mut one = open_follower(&paths[&LinkKind::Zone(ZoneId::Zone1)]);
        let mut running = h.start_real_time();

        running.with(|h| h.inject(LinkKind::Zone(ZoneId::Zone1), WireFault::Drop));
        one.write_all(&[0xAA, 0x55, 0x03, 0x21, 0x00, 0xDC])
            .expect("write to the follower");
        let dropped = read_up_to(&mut one, 6, Duration::from_millis(400));

        // The fault queue is drained one reply at a time, so the next exchange
        // is unfaulted. Without this the test passes on a pump that never ran:
        // "nothing arrived" is what both a working Drop and a dead thread look
        // like.
        one.write_all(&[0xAA, 0x55, 0x03, 0x21, 0x00, 0xDC])
            .expect("write to the follower");
        let after = read_up_to(&mut one, 6, Duration::from_secs(2));

        running.stop().expect("the pump ran clean");
        assert!(
            dropped.is_empty(),
            "the dropped reply must not arrive, and got {dropped:02X?}"
        );
        assert_eq!(
            after,
            vec![0xAA, 0x55, 0x00, 0x01, 0x00, 0xFF],
            "the pump was alive; only the faulted reply was suppressed"
        );
    }

    #[test]
    fn the_pump_survives_the_daemon_closing_its_port() {
        let h = harness();
        let paths = h.port_paths();
        let mut one = open_follower(&paths[&LinkKind::Zone(ZoneId::Zone1)]);
        let mut running = h.start_real_time();

        one.write_all(&[0xAA, 0x55, 0x03, 0x21, 0x00, 0xDC])
            .expect("write to the follower");
        drop(one); // the daemon exiting
        std::thread::sleep(Duration::from_millis(50));

        // Whatever happened, it is reported rather than panicked on, and the
        // harness comes back so the transcript is still readable — including
        // the write that went out before the port closed, which is what makes
        // this more than "stop() returned".
        drop(running.stop());
        assert!(
            running.with(|h| {
                h.transcript(LinkKind::Zone(ZoneId::Zone1))
                    .is_some_and(|t| t.transmitted().any(|e| e.bytes.starts_with(&[0xAA, 0x55])))
            }),
            "the bytes sent before the close are still in the transcript"
        );
    }

    #[test]
    fn every_link_gets_its_own_device_path() {
        let h = harness();
        let paths = h.port_paths();
        assert_eq!(paths.len(), 3);
        let distinct: std::collections::BTreeSet<_> = paths.values().collect();
        assert_eq!(
            distinct.len(),
            3,
            "two links sharing a device is the bug this prevents"
        );
        for p in paths.values() {
            assert!(p.starts_with("/dev/pts/"), "{p:?}");
        }
    }

    #[test]
    fn links_advance_in_lockstep() {
        let mut h = harness();
        let paths = h.port_paths();
        let mut one = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths[&LinkKind::Zone(ZoneId::Zone1)])
            .unwrap();
        let mut two = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths[&LinkKind::Zone(ZoneId::Zone2)])
            .unwrap();

        one.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        two.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        one.flush().ok();
        two.flush().ok();
        h.run_for(Duration::from_millis(30)).unwrap();

        for z in [ZoneId::Zone1, ZoneId::Zone2] {
            let t = h
                .transcript(LinkKind::Zone(z))
                .expect("a transcript per link");
            assert!(
                t.entries()
                    .iter()
                    .any(|e| e.direction == Direction::DeviceToDaemon),
                "{z} did not reply"
            );
        }
        assert_eq!(h.now(), Duration::from_millis(30));
    }

    #[test]
    fn a_silent_device_leaves_an_empty_reply_side() {
        let mut h = harness();
        let paths = h.port_paths();
        let mut steam = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths[&LinkKind::Steam])
            .unwrap();
        steam.write_all(b"\x88\x05\x00\x30\xCB\x55").unwrap();
        steam.flush().ok();
        h.run_for(Duration::from_millis(50)).unwrap();

        let t = h.transcript(LinkKind::Steam).unwrap();
        assert!(
            t.entries()
                .iter()
                .all(|e| e.direction == Direction::DaemonToDevice),
            "a mute device carries nothing back"
        );
        // But the request was still observed, which is what distinguishes an
        // open port with a silent device from no port at all.
        assert!(!t.is_silent());
    }

    #[test]
    fn the_scoping_check_names_the_links_that_were_disturbed() {
        let mut h = harness();
        let paths = h.port_paths();
        let mut one = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths[&LinkKind::Zone(ZoneId::Zone1)])
            .unwrap();
        let mut two = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths[&LinkKind::Zone(ZoneId::Zone2)])
            .unwrap();

        h.run_for(Duration::from_millis(10)).unwrap();
        let mark = h.now();

        // Only zone 1 speaks after the mark.
        one.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        one.flush().ok();
        h.run_for(Duration::from_millis(30)).unwrap();
        assert!(
            h.links_disturbed_since(LinkKind::Zone(ZoneId::Zone1), mark)
                .is_empty(),
            "nothing but zone 1 should have carried traffic"
        );

        // Now zone 2 speaks too, and the check names it.
        two.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        two.flush().ok();
        h.run_for(Duration::from_millis(30)).unwrap();
        assert_eq!(
            h.links_disturbed_since(LinkKind::Zone(ZoneId::Zone1), mark),
            vec![LinkKind::Zone(ZoneId::Zone2)]
        );
    }

    #[test]
    fn a_hangup_removes_the_link_and_returns_what_crossed_it() {
        let mut h = harness();
        let paths = h.port_paths();
        let mut one = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&paths[&LinkKind::Zone(ZoneId::Zone1)])
            .unwrap();
        one.write_all(b"\xAA\x55\x03\x02\x00\xFB").unwrap();
        one.flush().ok();
        h.run_for(Duration::from_millis(20)).unwrap();

        let t = h
            .hangup(LinkKind::Zone(ZoneId::Zone1))
            .expect("the link was there");
        assert!(!t.is_silent(), "the record survives the hangup");
        assert!(
            h.transcript(LinkKind::Zone(ZoneId::Zone1)).is_none(),
            "the link is gone"
        );
        assert!(
            h.hangup(LinkKind::Zone(ZoneId::Zone1)).is_none(),
            "and stays gone"
        );
        // The other links are unaffected.
        assert!(h.transcript(LinkKind::Zone(ZoneId::Zone2)).is_some());
        assert!(h.transcript(LinkKind::Steam).is_some());
    }

    #[test]
    fn a_long_run_costs_almost_nothing_because_nothing_sleeps() {
        let mut h = harness();
        // The 20-minute session limit, at its real constant.
        h.run_for(Duration::from_secs(1200)).unwrap();
        assert_eq!(h.now(), Duration::from_secs(1200));
    }

    #[test]
    fn all_links_is_the_three_this_installation_has() {
        assert_eq!(all_links().len(), 3);
        assert!(all_links().contains(&LinkKind::Steam));
    }
}
