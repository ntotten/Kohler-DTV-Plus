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
//! The daemon-spawning half of the end-to-end rig lands with the daemon binary.
//! This part is what it will drive.

use crate::transcript::Transcript;
use crate::wire::{DeviceModel, Wire, WireFault};
use kdtv_units::{LinkKind, ZoneId};
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

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
