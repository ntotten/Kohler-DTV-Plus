//! Ring 3: the real `kdtvd` binary, as a separate process, driving emulated
//! valves and an emulated steam adapter over pseudo-terminals.
//!
//! **Every assertion here runs against the transcript — the bytes the daemon
//! actually put on the wire — and never against what the daemon says about
//! itself.** That is the whole point of the ring: a service that believes it is
//! off while transmitting an open frame passes a state assertion and fails
//! these. The daemon's own log is read in exactly one place, and only to put
//! its complaint into a failure message.
//!
//! Run it with `./scripts/e2e.sh`, which builds the daemon and exports
//! [`kdtv_emulator::e2e::DAEMON_ENV`]. Without that variable every test here
//! **skips**, loudly: `./scripts/test.sh` runs `cargo test --workspace` on
//! machines that have never built `kdtvd`, and a suite that failed there would
//! be deleted rather than fixed.
//!
//! # Each test owns everything it touches
//!
//! `scripts/e2e.sh` passes `--test-threads=1`, and nothing here depends on it.
//! Every test allocates its own pseudo-terminals (one set per [`Rig`]), its own
//! `.e2e/<name>` directory, and its own loopback port, so two running at once
//! do not collide.
//!
//! # Opening water here is correct
//!
//! The devices are models in this process. The rule that matters is that no
//! *real* bus can be opened, which the transmit gate enforces structurally and
//! [`the_transmit_gate_stayed_closed_for_the_whole_run`] asserts from both
//! ends.

// An integration test is its own crate, so `lib.rs`'s `cfg_attr(test, ...)`
// header does not reach it and the workspace lints apply in full. Same allow as
// `kdtv-hal/tests/foreign_link.rs`, for the same reason.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Write as _;
use std::time::Duration;

use kdtv_config::GateScope;
use kdtv_emulator::e2e::{
    Cadence, DAEMON_ENV, Daemon, DaemonCommand, Rig, RigOptions, TxFrame, transactions,
    transmitted_at, transmitted_dtv, transmitted_saturn,
};
use kdtv_emulator::rig::all_links;
use kdtv_emulator::transcript::Direction;
use kdtv_emulator::wire::WireFault;
use kdtv_proto::dtv::opcode as dtv;
use kdtv_proto::saturn::{VALVE_ADDR_MAX, VALVE_ADDR_MIN, opcode};
use kdtv_units::{LinkKind, ZoneId};

/// How long the boot sequence is given.
///
/// Discovery probes five addresses and four of them time out at the 320 ms
/// response deadline, so about 1.4 s is the floor. Thirty seconds is slack for
/// a loaded CI runner and for the emulated-Pi run, where every instruction is
/// interpreted.
const BOOT_BUDGET: Duration = Duration::from_secs(30);

/// The exit code `kdtvd` documents for "every link confirmed off".
const EXIT_CONFIRMED_OFF: i32 = 0;

/// How much shorter than the true interval a **measured** one may read.
///
/// This is observation error and nothing else. A transcript timestamp is the
/// moment the wire simulator observed the bytes, and it observes on
/// `RunningHarness::PUMP_INTERVAL` — one millisecond. Two adjacent samples can
/// each be late by a different amount, so an interval measured from them can
/// understate the true one by that difference. Five pump intervals is that,
/// with room for a pump thread scheduled late on a loaded runner or under
/// `qemu-aarch64`, where the emulated-Pi run competes with the emulator for the
/// same CPU.
///
/// It is 1.6 % of the 320 ms response deadline it is subtracted from below, and
/// a second frame sent inside the previous transaction's window shows
/// single-digit milliseconds against that deadline — so it cannot hide the
/// fault it is next to. Found by `./scripts/e2e.sh --pi-sim`, which measured a
/// 320 ms probe timeout as 319 ms.
const OBSERVATION_SLACK: Duration = Duration::from_millis(5);

/// The shortest gap between two consecutive frames on a link that is still
/// consistent with its documented tick.
///
/// **Not the same quantity as [`OBSERVATION_SLACK`], which is why it is not the
/// same constant.** The dominant term here is not pump jitter: it is the
/// supervisor's own refusal to catch up a late deadline. A pass that ran late
/// is followed by one at the next *absolute* deadline, so the gap after a late
/// pass is shorter than the tick by exactly the previous pass's lateness — and
/// that lateness is scheduling delay, which is the same number of milliseconds
/// on a 150 ms steam tick as on a 525 ms zone tick. A single constant therefore
/// spends 3.3× more of the steam link's budget than of a zone's, and the steam
/// link is where a loaded runner runs out of it first: measured under 48 busy
/// loops on 4 cores, post-boot steam gaps reached 145 ms against a 145 ms
/// floor.
///
/// A tenth of the tick, floored at the observation error, treats the two links
/// alike. It stays far too tight to hide the fault: a poller at twice the
/// cadence shows 262 ms against a 472 ms floor, and a burst inside one pump
/// read now shows **zero** — see `transactions`' documentation, and the count
/// ceiling below, which is what catches a sustained over-rate that each
/// individual gap survives.
fn cadence_floor(tick: Duration) -> Duration {
    tick.saturating_sub(std::cmp::max(OBSERVATION_SLACK, tick / 10))
}

/// The reads the boot sequence performs, in the order `kdtv_engine`'s
/// `IDENTIFY` lists them. Firmware type is first because everything after it is
/// read through the outlet table, and the table is only right for the family
/// the valve actually is.
const IDENTIFY: [u8; 8] = [
    opcode::READ_FIRMWARE_TYPE,
    opcode::READ_FIRMWARE_VERSION,
    opcode::READ_SERIAL_NUMBER,
    opcode::READ_CALIBRATION,
    opcode::READ_CONFIGURATION,
    opcode::READ_OUTLET_STATES,
    opcode::READ_TEMPERATURE,
    opcode::READ_FAULT_FLAGS,
];

/// The daemon to drive, or `None` with the reason printed.
///
/// The reason goes to the process's real stderr rather than through `println!`,
/// so it is visible under a bare `cargo test --workspace` as well as under
/// `scripts/e2e.sh`'s `--nocapture`. A silent skip is how a suite stops running
/// without anyone noticing.
fn daemon_or_skip(test: &str) -> Option<DaemonCommand> {
    match DaemonCommand::from_env() {
        Ok(Some(c)) => Some(c),
        Ok(None) => {
            let mut err = std::io::stderr();
            let _ = writeln!(
                err,
                "SKIP {test}: {DAEMON_ENV} is not set, so there is no daemon to drive. \
                 Run ./scripts/e2e.sh, which builds one."
            );
            None
        }
        Err(why) => panic!("{why}"),
    }
}

/// A booted rig and the daemon driving it.
///
/// The daemon is returned by value so its guard's `Drop` runs at the end of the
/// test — including when an assertion panics, which is the case that matters:
/// a leaked `kdtvd` holds three pseudo-terminals and wedges the next run.
fn boot(name: &str, command: &DaemonCommand) -> (Rig, Daemon) {
    let rig = match Rig::start(name, &RigOptions::default()) {
        Ok(rig) => rig,
        Err(e) => panic!("{name}: the rig would not start: {e}"),
    };
    let mut daemon = match Daemon::start(&rig, command) {
        Ok(d) => d,
        Err(e) => panic!("{name}: the daemon would not start: {e}"),
    };
    if let Err(e) = rig.wait_for_boot(&mut daemon, BOOT_BUDGET) {
        panic!("{name}: {e}");
    }
    (rig, daemon)
}

/// Every Saturn frame the daemon transmitted on a link.
fn saturn(rig: &Rig, link: LinkKind) -> Vec<TxFrame> {
    transmitted_saturn(&rig.transcript(link))
}

/// Every Saturn frame transmitted on a link at or after `mark`.
fn saturn_since(rig: &Rig, link: LinkKind, mark: Duration) -> Vec<TxFrame> {
    saturn(rig, link)
        .into_iter()
        .filter(|f| f.at >= mark)
        .collect()
}

fn render(frames: &[TxFrame]) -> String {
    frames
        .iter()
        .map(|f| format!("  {:>9.3}s {}", f.at.as_secs_f64(), f.hex()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The same frames with consecutive repeats of one `(address, control)`
/// collapsed to the first of them.
///
/// A retry is byte-for-byte the frame it retries, so this is "the sequence of
/// distinct steps the daemon took". **Only the discovery and identification
/// assertions read it**, because only they are positional: a reply that never
/// arrives makes `kdtv_engine`'s zone machine re-send the read on the next
/// tick, and one retry renumbers every index after it — so `frames[i]` reports
/// a lost reply as a wrong boot sequence, which is a different finding
/// entirely. Verified by dropping zone 1's answer to `READ_SERIAL_NUMBER`:
/// indexing the raw frames failed with "the identity reads are not the
/// documented sequence", and indexing the steps passed.
///
/// **The all-off is checked on the raw frames instead**, and deliberately.
/// "The all-off was retried rather than acknowledged" is an assertion *about* a
/// repeat, so collapsing repeats first makes it unable to fail — found by
/// dropping the valve's answer to the stop and watching it stay green through
/// this function. The cadence and one-transaction tests measure every frame
/// too, for the same class of reason.
///
/// A late-but-arriving reply is not this case: `kdtv_engine` treats it as
/// `MalformedResponse` and latches the zone, so the run fails at
/// `wait_for_boot` with the daemon's own "response N ms late" rather than
/// anywhere near here.
fn steps(frames: &[TxFrame]) -> Vec<TxFrame> {
    let mut out: Vec<TxFrame> = Vec::with_capacity(frames.len());
    for f in frames {
        if out
            .last()
            .is_some_and(|p| p.address == f.address && p.control == f.control)
        {
            continue;
        }
        out.push(f.clone());
    }
    out
}

/// The per-zone half of the boot assertion, extracted so the test reads as the
/// four steps it checks.
fn one_zone_booted_in_the_documented_order(rig: &Rig, zone: ZoneId, probes: usize, address: u8) {
    let link = LinkKind::Zone(zone);
    let all = saturn(rig, link);
    // Steps 1 and 2 are positional, so they run over the distinct steps
    // rather than over every frame: one retried read would otherwise renumber
    // everything after it. Step 3 uses `all`. See `steps`.
    let frames = steps(&all);

    assert!(
        frames.len() > probes + IDENTIFY.len() + 1,
        "{zone}: the boot sequence did not finish\n{}",
        render(&frames)
    );

    // 1. Discovery: one read per address, in order, and no address
    //    management — probing with a read establishes which addresses are
    //    occupied without rewriting what is installed. `PH0-01`.
    for (i, expected) in (VALVE_ADDR_MIN..=VALVE_ADDR_MAX).enumerate() {
        let f = &frames[i];
        assert!(
            f.is_probe(),
            "{zone}: frame {i} is control 0x{:02X}, not a firmware-type read\n{}",
            f.control,
            render(&frames)
        );
        assert_eq!(
            f.address,
            expected,
            "{zone}: probe {i} went to 0x{:02X}, not 0x{expected:02X}\n{}",
            f.address,
            render(&frames)
        );
    }
    assert!(
        frames
            .iter()
            .all(|f| f.control != opcode::ADDRESS_MANAGEMENT),
        "{zone}: discovery must be read-only\n{}",
        render(&frames)
    );

    // 2. Identification, in the order the engine lists it.
    let identify: Vec<u8> = frames
        .iter()
        .skip(probes)
        .take(IDENTIFY.len())
        .map(|f| f.control)
        .collect();
    assert_eq!(
        identify,
        IDENTIFY.to_vec(),
        "{zone}: the identity reads are not the documented sequence\n{}",
        render(&frames)
    );

    // 3. An all-off, before anything else, and something after it — which
    //    is the acknowledgement, since nothing leaves ConfirmOff without
    //    one and a retry would be another all-off. `BOOT-05`.
    //
    //    Over the raw frames, not the steps: collapsing repeats is exactly
    //    what would make "retried rather than acknowledged" unable to fail,
    //    since a retry *is* the repeat. Found by dropping one reply and
    //    watching this assertion stay green. So the all-off is located
    //    rather than indexed, and what follows it is read unfiltered.
    //
    //    Addressed to the valve the rig enrolled: a `0x87 00` sent
    //    somewhere else is bytes on the wire that close nothing.
    let stop_at = all
        .iter()
        .position(|f| f.is_all_off_to(address))
        .unwrap_or_else(|| {
            panic!(
                "{zone}: no all-off addressed to 0x{address:02X} in the whole run; the \
                 boot sequence must reach a confirmed all-off before it polls\n{}",
                render(&all)
            )
        });
    assert!(
        all.iter().take(stop_at).all(|f| !f.is_outlet_stop()),
        "{zone}: an outlet write went out before the boot all-off\n{}",
        render(&all)
    );
    assert_eq!(
        steps(all.get(..stop_at).unwrap_or_default()).len(),
        probes + IDENTIFY.len(),
        "{zone}: the all-off is not the frame that follows identification\n{}",
        render(&all)
    );
    let after_all_off = all.get(stop_at + 1).unwrap_or_else(|| {
        panic!(
            "{zone}: nothing followed the all-off, so nothing acknowledged it\n{}",
            render(&all)
        )
    });
    assert!(
        !after_all_off.is_outlet_stop(),
        "{zone}: the all-off was retried rather than acknowledged\n{}",
        render(&all)
    );

    // 4. And nothing, anywhere in the run — every frame, not just the
    //    distinct steps — opened an outlet.
    assert!(
        all.iter().all(|f| !f.opens_water()),
        "{zone}: a frame opened water in a run that never commanded a start\n{}",
        render(&all)
    );
}

// ------------------------------------------------------------------ 1. boot

/// **It boots**, and the wire shows the documented sequence in order.
///
/// `BOOT-01`..`BOOT-08`. What this measures, frame by frame: the first thing
/// transmitted on a valve bus is a read, not a start; discovery probes every
/// address in `0x03..=0x07` and does **not** stop at the one that answers,
/// because stopping early is what would hide a second valve; the eight identity
/// reads follow in their documented order; and an all-off is transmitted and
/// answered before anything else is. Then, across the whole run, no frame that
/// could open an outlet was transmitted at all.
#[test]
fn the_daemon_boots_through_discovery_identification_and_a_confirmed_all_off() {
    let Some(command) = daemon_or_skip("boot") else {
        return;
    };
    let (rig, mut daemon) = boot("boot", &command);
    // A little ordinary running, so "nothing opened water" covers the polling
    // phase and not only the boot.
    rig.observe(&mut daemon, Duration::from_secs(2))
        .expect("the daemon stays up");

    let probes = usize::from(VALVE_ADDR_MAX - VALVE_ADDR_MIN + 1);
    let address = rig.valve_address();
    for zone in ZoneId::ALL {
        one_zone_booted_in_the_documented_order(&rig, zone, probes, address);
    }

    // The steam link's own three-step handshake, then status polling: the
    // adapter is enrolled and the first thing asked of it is a read.
    let steam = transmitted_dtv(&rig.transcript(LinkKind::Steam));
    // The same de-duplication, for the same reason: a retried DEV_ASSIGN_ADDR
    // is not a different handshake.
    let mut distinct: Vec<u8> = Vec::new();
    for f in &steam {
        if distinct.last() != Some(&f.cmd) {
            distinct.push(f.cmd);
        }
    }
    let cmds: Vec<u8> = distinct.iter().copied().take(3).collect();
    assert_eq!(
        cmds,
        vec![
            dtv::DEV_ADDRESS_OPP,
            dtv::DEV_ASSIGN_ADDR,
            dtv::GET_DEV_STATUS
        ],
        "steam: the transmitted opcodes were {:02X?}",
        steam.iter().map(|f| f.cmd).collect::<Vec<_>>()
    );
    assert_eq!(
        steam.first().map(|f| f.dest),
        Some(kdtv_proto::dtv::BROADCAST),
        "the address opportunity is a broadcast"
    );
    assert!(
        steam.iter().all(|f| f.cmd != dtv::SET_DEV_PARAM),
        "steam: a parameter write was transmitted in a run that commanded nothing"
    );
}

// --------------------------------------------------------------- 2. cadence

/// **It polls at the documented cadence and no faster.**
///
/// `FIELD-NOTES` § 1 and `INVESTIGATIONS.md` I1 are why this matters: this
/// project's own client hung the original controller by exceeding its
/// documented limits, and a replacement that did the same would be the same bug
/// with a different name.
///
/// What it measures: the gap between consecutive **frames** on one link, over a
/// real interval of ordinary running, against the tick in
/// `deploy/kdtvd.emulated.toml` — read from the file the daemon is running,
/// never restated here. The window starts after boot, because discovery and
/// identification are deliberately faster than the tick: each of those frames
/// goes out only once the previous one has been answered or has timed out,
/// which is the one-transaction rule below rather than the cadence.
///
/// # Frames, not transcript entries
///
/// Both measurements below read `transmitted_at`, which decodes. The first
/// version of this test read `Transcript::transmitted().map(|e| e.at)`, and
/// that is the one shape in which this assertion cannot see the fault it exists
/// for. `Wire::pump` reads each link once per millisecond and records
/// everything it got as **one** entry, so a supervisor that flushed a queue —
/// a poll, a retry and a status read written back to back without waiting for
/// a reply, which is the I1 shape and is instantaneous on a pseudo-terminal —
/// puts three frames into one entry with one timestamp. Measured between
/// entries: one transmission per tick, a 525 ms gap, nine in five seconds
/// against a ceiling of twelve, green. Measured between frames: a 0 ms gap,
/// and both checks below fail loudly.
///
/// The conflation also bites the other way. When a write *is* split across two
/// pump reads, two entries land about a millisecond apart and the gap check
/// fails on a daemon that did nothing wrong. Decoding is what makes an entry
/// boundary stop mattering in either direction.
#[test]
fn each_link_polls_at_its_documented_cadence_and_no_faster() {
    let Some(command) = daemon_or_skip("cadence") else {
        return;
    };
    let (rig, mut daemon) = boot("cadence", &command);

    let mark = rig.elapsed();
    let window = Duration::from_secs(5);
    rig.observe(&mut daemon, window)
        .expect("the daemon stays up");

    for link in all_links() {
        let Cadence { tick, .. } = rig.cadence(link);
        let t = rig.transcript(link);
        let ats: Vec<Duration> = transmitted_at(&t, link)
            .into_iter()
            .filter(|at| *at >= mark)
            .collect();
        assert!(
            ats.len() >= 3,
            "{link}: only {} frames in {window:?}; there is nothing to measure",
            ats.len()
        );

        // A gap may read short of the tick by the daemon's own scheduling
        // lateness plus the harness's observation error; see `cadence_floor`.
        // More than that is the service running faster than the bus is
        // documented to accept.
        let floor = cadence_floor(tick);
        for pair in ats.windows(2) {
            let [a, b] = pair else { continue };
            let gap = b.saturating_sub(*a);
            assert!(
                gap >= floor,
                "{link}: {} ms between frames at {:.3}s and {:.3}s, and the documented \
                 tick is {} ms, so the floor is {} ms. This is what hung the K-99695.\n{}",
                gap.as_millis(),
                a.as_secs_f64(),
                b.as_secs_f64(),
                tick.as_millis(),
                floor.as_millis(),
                t.render()
            );
        }

        // And the count agrees with the cadence, which catches a sustained
        // over-rate that no individual gap happened to expose.
        let expected = window.as_secs_f64() / tick.as_secs_f64();
        let ceiling = expected.mul_add(0.05, expected) + 2.0;
        let seen = u32::try_from(ats.len()).unwrap_or(u32::MAX);
        assert!(
            f64::from(seen) <= ceiling,
            "{link}: {seen} frames in {window:?}, and a {} ms tick allows about \
             {expected:.0}\n{}",
            tick.as_millis(),
            t.render()
        );
    }
}

// ------------------------------------------------------- 3. one transaction

/// **One transaction in flight per link**, measured at the wire.
///
/// Saturn carries no sender field, so nothing in a reply correlates it with a
/// request except its being the only one outstanding. The rule is therefore
/// load-bearing rather than a nicety, and the wire is where it has to be
/// checked: between any two consecutive frames on a link there is either the
/// device's answer to the first, or a gap at least as long as that link's
/// response deadline — which is the difference between a serialised bus and a
/// timeout.
///
/// `transactions` pairs **decoded frames** with the replies that answered them,
/// which is the whole of this assertion's power: two frames written inside one
/// pump interval are one transcript entry, and pairing entries would report the
/// pipelining defect this test is named for as a single answered transaction.
#[test]
fn req_dtv_plus_protocol_time_03_req_design_bus_01_only_one_transaction_is_in_flight_per_link() {
    let Some(command) = daemon_or_skip("in-flight") else {
        return;
    };
    let (rig, mut daemon) = boot("in-flight", &command);
    rig.observe(&mut daemon, Duration::from_secs(3))
        .expect("the daemon stays up");

    for link in all_links() {
        let Cadence { response, .. } = rig.cadence(link);
        let t = rig.transcript(link);
        let tx = transactions(&t, link);
        assert!(
            tx.len() > 3,
            "{link}: {} transactions is too little traffic to judge",
            tx.len()
        );

        // Observation error only. The deadline itself is not negotiable — a
        // 320 ms message timeout is a property of 9600 8N1 — so this subtracts
        // what two pump samples can mis-measure and nothing else.
        let floor = response.saturating_sub(OBSERVATION_SLACK);
        for pair in tx.windows(2) {
            let [first, second] = pair else { continue };
            let gap = second.sent_at.saturating_sub(first.sent_at);
            let answered = first.answered_at.is_some_and(|at| at <= second.sent_at);
            assert!(
                answered || gap >= floor,
                "{link}: a second frame went out {} ms after the first, which was still \
                 unanswered. The response deadline is {} ms, so nothing correlates either \
                 frame with the reply that follows.\n{}",
                gap.as_millis(),
                response.as_millis(),
                t.render()
            );
        }
    }
}

// -------------------------------------------------------------- 4. scoping

/// **A fault on zone 1 leaves zone 2 alone.**
///
/// Getting the scope wrong in the widening direction stops someone's shower in
/// the other room; getting it wrong in the narrowing direction leaves water
/// running. The fault injected is a link that stops carrying: the valve is
/// still there, the port is still open, and nothing comes back.
///
/// What it measures. On zone 1: the retry train runs out, an all-off is
/// transmitted, and the link then goes quiet — which is the port closing behind
/// the latch rather than a service that keeps talking to a dead bus. On zone 2
/// and steam: traffic continues, and **no outlet write of any kind** appears on
/// zone 2, because a stop that reached the other zone would be the widening
/// failure and it would be a `0x87`.
///
/// # A note on `links_disturbed_since`
///
/// In a lockstep test the other links are idle and that call is expected to
/// return nothing. Here they are polling at their own cadence throughout, so
/// its meaning is inverted: an **empty** result would mean zone 2 and steam had
/// gone silent along with zone 1, which is the widening fault. So it is asserted
/// non-empty, and the frame-level check below is what says nothing was
/// *commanded* on them.
#[test]
fn req_design_svc_04_a_wire_fault_on_zone_1_stops_zone_1_and_leaves_zone_2_and_steam_alone() {
    let Some(command) = daemon_or_skip("scoping") else {
        return;
    };
    let (rig, mut daemon) = boot("scoping", &command);
    let zone1 = LinkKind::Zone(ZoneId::Zone1);
    let zone2 = LinkKind::Zone(ZoneId::Zone2);

    let address = rig.valve_address();
    let mark = rig.elapsed();
    rig.with(|h| h.inject(zone1, WireFault::GoSilent));

    // The retry train has to run out first: three attempts at a 320 ms
    // deadline, issued one per 525 ms tick.
    rig.wait_for(
        &mut daemon,
        "zone 1 to give up and command itself off",
        Duration::from_secs(20),
        |rig| {
            saturn_since(rig, zone1, mark)
                .iter()
                .any(|f| f.is_all_off_to(address))
        },
    )
    .unwrap_or_else(|e| panic!("a silent zone-1 bus did not produce an all-off: {e}"));

    let quiet_since = rig.elapsed();
    rig.observe(&mut daemon, Duration::from_secs(3))
        .expect("the daemon stays up");

    let after = saturn_since(&rig, zone1, mark);
    let last = after.last().expect("zone 1 transmitted after the fault");
    assert!(
        last.is_all_off_to(address),
        "zone 1: the last frame was {}, not an all-off addressed to 0x{address:02X}\n{}",
        last.hex(),
        render(&after)
    );
    assert!(
        last.at <= quiet_since,
        "zone 1: it was still transmitting {} after the latch\n{}",
        last.hex(),
        render(&after)
    );

    // Zone 2 and steam carried on. An empty list here would mean they had gone
    // down with zone 1 — see the note in the doc comment.
    let still_running = rig.with(|h| h.links_disturbed_since(zone1, quiet_since));
    assert!(
        still_running.contains(&zone2) && still_running.contains(&LinkKind::Steam),
        "a zone-1 fault took another link down with it; still carrying traffic after \
         {quiet_since:?}: {still_running:?}"
    );

    // And nothing was commanded on zone 2.
    let zone2_after = saturn_since(&rig, zone2, mark);
    assert!(
        zone2_after
            .iter()
            .all(|f| f.control != opcode::WRITE_OUTLET_STATES),
        "zone 2 was commanded by zone 1's fault\n{}",
        render(&zone2_after)
    );
    assert!(
        zone2_after.len() >= 3,
        "zone 2 stopped polling after zone 1 faulted\n{}",
        render(&zone2_after)
    );
}

// --------------------------------------------------------------- 5. SIGTERM

/// **`SIGTERM` stops water before the process exits.**
///
/// An abrupt exit that leaves a valve open is the worst outcome this system
/// has, so the signal is not allowed to end the process directly: it asks the
/// control loop to command every link off, and the loop exits when it has
/// confirmed that.
///
/// What it measures: an all-off went out on each valve bus after the signal,
/// **the valve answered it**, and that answer was on the wire before the
/// process was observed gone — all three readings from the harness clock, which
/// is the clock the transcript is stamped with. Then the exit code, which is
/// the one `kdtvd` documents for a confirmed off.
///
/// The acknowledgement is what carries the claim. Comparing the *stop* against
/// the moment the process was gone cannot fail in the direction the name
/// promises — a dead process cannot transmit, so no frame can be stamped later
/// — and it would hold equally for a daemon that wrote a stop on its way out
/// from an atexit path, having already released the links. The reply is
/// different: the emulated valve schedules it a line-time after the stop, so a
/// daemon that exits without waiting for it is gone *before* it is on the
/// wire, and this fails. "The loop confirmed every link off, then exited" and
/// "the process wrote a stop as it died" are distinguishable here and nowhere
/// else in the test.
///
/// # Why the signal waits for a quiet bus
///
/// Not to make the test easier to pass. `kdtvd` exits **5**, not 0, when
/// `SIGTERM` arrives while a poll is still unanswered: the stop abandons that
/// transaction and goes out in its place, the abandoned poll's eight-byte reply
/// then arrives inside the stop's response window, and the engine decodes it
/// against the stop's own six-byte expectation and latches the zone with
/// `MalformedResponse`. The all-off still reaches the wire, so the first
/// assertion below holds either way — but the run is reported as an unconfirmed
/// off, which is the code reserved for the condition whose remedy is a person
/// removing valve power. That is a defect in the shutdown path rather than in
/// this rig, and this test signals on a quiet bus so that it asserts the
/// documented outcome instead of pinning the wrong one.
#[test]
fn sigterm_puts_an_all_off_on_the_wire_before_the_process_is_gone() {
    let Some(command) = daemon_or_skip("sigterm") else {
        return;
    };
    let (rig, mut daemon) = boot("sigterm", &command);

    // Both valve buses idle: the last thing on each wire is the valve's answer,
    // and it has been there long enough that no reply can still be in flight.
    rig.wait_for(
        &mut daemon,
        "both valve buses to go quiet between polls",
        Duration::from_secs(10),
        |rig| {
            ZoneId::ALL.iter().all(|z| {
                let t = rig.transcript(LinkKind::Zone(*z));
                t.entries().last().is_some_and(|e| {
                    e.direction == Direction::DeviceToDaemon
                        && rig.elapsed().saturating_sub(e.at) >= Duration::from_millis(50)
                })
            })
        },
    )
    .expect("an idle moment between polls");

    let address = rig.valve_address();
    let mark = rig.elapsed();
    daemon.terminate().expect("SIGTERM reaches the daemon");
    let status = daemon
        .wait_for_exit(&rig, Duration::from_secs(20))
        .expect("the daemon exits");
    let gone = rig.elapsed();
    // Let anything the valves had already scheduled arrive, so a reply that
    // came too late is *in* the transcript with a stamp after `gone` rather
    // than merely absent from it. Without this the ordering assertion below
    // could only fire by winning a race with the pump.
    rig.settle(Duration::from_millis(300));

    for zone in ZoneId::ALL {
        let link = LinkKind::Zone(zone);
        let t = rig.transcript(link);
        let after = saturn_since(&rig, link, mark);
        let stop = after
            .iter()
            .find(|f| f.is_all_off_to(address))
            .unwrap_or_else(|| {
                panic!(
                    "{zone}: no all-off addressed to 0x{address:02X} was transmitted between \
                     the signal and the exit\n{}",
                    render(&after)
                )
            });

        // Whether the signal actually landed on the quiet bus the wait above
        // established. Quietness is observed and the signal is sent in two
        // statements with a scheduler between them, so a preemption longer
        // than the ~467 ms that remained of the tick moves the signal to an
        // arbitrary phase — and roughly 8 ms in every 525 ms lands inside a
        // response window, where `kdtvd` exits 5 by a defect in the shutdown
        // path this test is deliberately not pinning. Checked retrospectively,
        // off the transcript, so it is not itself a race: the question is
        // whether the frame before the stop had been answered when the stop
        // went out, and a run where it had not cannot judge the exit code. It
        // is reported as the rig losing a race, because that is what it is.
        let all = saturn(&rig, link);
        let quiet = all
            .iter()
            .position(|f| f.at >= stop.at && f.is_all_off_to(address))
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| all.get(i))
            .is_none_or(|prev| {
                t.entries().iter().any(|e| {
                    e.direction == Direction::DeviceToDaemon && e.at >= prev.at && e.at <= stop.at
                })
            });
        assert!(
            quiet,
            "{zone}: the frame before the stop had not been answered when the stop went \
             out, so SIGTERM did not arrive on a quiet bus and this run cannot judge the \
             exit code — the abandoned poll's reply lands inside the stop's window and \
             latches the zone. That is the rig losing a race with the poll cadence, not a \
             finding about the daemon.\n{}",
            t.render()
        );

        // The valve answered the stop, and the answer was on the wire before
        // the process was observed gone. This is the ordering the name
        // promises, and the only reading in the transcript that can fail in
        // that direction.
        let ack = t
            .entries()
            .iter()
            .find(|e| e.direction == Direction::DeviceToDaemon && e.at >= stop.at)
            .unwrap_or_else(|| {
                panic!(
                    "{zone}: the stop at {:.3}s was never answered, so nothing confirmed \
                     the valve off\n{}",
                    stop.at.as_secs_f64(),
                    t.render()
                )
            });
        assert!(
            ack.at <= gone,
            "{zone}: the process was gone at {:.3}s and the valve's answer to the stop \
             reached the wire at {:.3}s. The control loop is meant to exit once every link \
             has confirmed itself off; this exited first and the stop went out on the way \
             down.\n{}",
            gone.as_secs_f64(),
            ack.at.as_secs_f64(),
            t.render()
        );
    }

    assert_eq!(
        status.code(),
        Some(EXIT_CONFIRMED_OFF),
        "kdtvd exited {status}, and it documents {EXIT_CONFIRMED_OFF} for a confirmed off. \
         Anything else means at least one link never answered its stop.\n{}",
        daemon.log()
    );
}

// -------------------------------------------------------------- 6. the gate

/// **The gate is closed**: the daemon opened pseudo-terminals and no real
/// serial port.
///
/// Asserted from three ends, because each alone is weak.
///
/// 1. **The evidence.** `TransmitAuthority::resolve` is the call that consults
///    fixture provenance, and it is the one the daemon makes. Asked for the
///    committed scope it yields an authority that cannot open a real bus;
///    asked — with a pinned hash, a capture reference and a polarity note for
///    every link, which is everything an operator could supply — for the other
///    one, it **refuses, and refuses on provenance**. That is the assertion
///    that fails the day a fixture is promoted to tier `[A]`, which is what
///    this test's failure message has always claimed to be about.
/// 2. **The declaration.** The gate scope in the configuration the rig
///    rendered and the daemon was started with, parsed rather than searched
///    for: `deploy/kdtvd.emulated.toml` says the words "emulator-only" in a
///    comment as well as in the assignment.
/// 3. **The process.** Every device node under `/dev` the daemon holds, read
///    out of `/proc/<pid>/fd` — as an allowlist, and read twice.
///
/// The first assertion needs no daemon and runs even on a skip, because it is
/// about the build rather than about a process.
/// The evidence half, extracted so the test that uses it stays readable.
///
/// Returns the authority the committed configuration resolves to, which is the
/// one `kdtv_hal::permit_open` is then asked about.
fn the_evidence_still_refuses_a_real_bus() -> kdtv_proto::gate::TransmitAuthority {
    use kdtv_proto::FixtureSet;
    use kdtv_proto::gate::{
        GateError, PolarityAttestation, PolarityNote, RequestedScope, TransmitAuthority,
        TransmitGateConfig,
    };

    let fixtures = FixtureSet::embedded();

    // 1a. The scope the daemon is asked for, through the call that weighs the
    //     evidence rather than through the constructor that cannot ask for
    //     anything else.
    let authority = TransmitAuthority::resolve(&TransmitGateConfig::emulator_only(), fixtures)
        .expect("the committed scope resolves against the embedded fixtures");
    assert!(
        !authority.permits_real_bus(),
        "resolving the committed configuration produced an authority that permits a real \
         bus"
    );

    // 1b. And the other scope is refused *because the fixtures are tier [C]* —
    //     not because a field was left blank. Everything an operator could fill
    //     in is filled in, so provenance is the only thing left to refuse on.
    let asking = TransmitGateConfig {
        scope: RequestedScope::RealBusAttested,
        capture_ref: Some("research/diagnostics/there-is-no-such-capture.bin".to_owned()),
        polarity: PolarityAttestation {
            notes: all_links()
                .into_iter()
                .map(|link| PolarityNote {
                    link,
                    note: "A+ = converter TA".to_owned(),
                    attested_on: "2026-01-01".to_owned(),
                })
                .collect(),
        },
        expected_fixtures_sha256: Some(authority.fixtures_sha256_hex()),
    };
    match TransmitAuthority::resolve(&asking, fixtures) {
        Err(GateError::FixturesNotCaptured { documented, .. }) => assert!(
            !documented.is_empty(),
            "the gate refused a real bus, but not because any fixture is still tier [C]"
        ),
        Err(other) => panic!(
            "the gate refused a real bus for the wrong reason: {other}. Phase 1 capture has \
             not run, so the refusal has to be about fixture provenance — a refusal on a \
             missing field would still be there after every fixture was promoted."
        ),
        Ok(granted) => panic!(
            "the gate GRANTED a real bus on fixture set {}. Every fixture in this \
             repository is tier [C]; one has been promoted, and opening the gate is meant \
             to be a dated, reviewable act rather than something a test discovers.",
            granted.fixtures_sha256_hex()
        ),
    }

    authority
}

#[test]
fn the_transmit_gate_stayed_closed_for_the_whole_run() {
    use kdtv_hal::{Backend, permit_open};

    let authority = the_evidence_still_refuses_a_real_bus();

    // 1c. And with that authority, `kdtv-hal` refuses a real backend per link.
    for link in all_links() {
        assert!(
            permit_open(Backend::Serial, link, &authority).is_err(),
            "{link}: the gate would open a real serial port"
        );
        assert!(
            permit_open(Backend::Pty, link, &authority).is_ok(),
            "{link}: the gate refuses the pseudo-terminal the emulated rig needs"
        );
    }

    let Some(command) = daemon_or_skip("gate") else {
        return;
    };
    let (rig, mut daemon) = boot("gate", &command);

    // 2. The declaration the daemon was actually started with, as `kdtv-config`
    //    parsed it — not as a substring of the file. `deploy/kdtvd.emulated.toml`
    //    line 27 is a comment reading `scope` must be "emulator-only", so a
    //    `contains("emulator-only")` check stays green with the assignment on
    //    line 34 changed to `real-bus-attested`.
    assert_eq!(
        rig.gate().scope(),
        GateScope::EmulatorOnly,
        "the rendered configuration at {} declares transmit_gate.scope = {}",
        rig.config_path().display(),
        rig.gate().scope()
    );

    // 3. What the process holds. An **allowlist**: a denylist has to anticipate
    //    every spelling of a real device, and /dev/spidev0.0 is not spelled
    //    /dev/tty, nor is /dev/rfcomm0 or /dev/bus/usb/001/002.
    //
    //    A run on this machine holds exactly `/dev/null` (stdin, which
    //    `Daemon::start` supplies) and the three pseudo-terminals. The two
    //    random devices are here because a libc can back `getrandom` with one,
    //    notably under `qemu-aarch64`; nothing else is, and anything else
    //    appearing is a finding rather than a reason to widen this.
    let allowed = |device: &std::path::Path| {
        let name = device.to_string_lossy().into_owned();
        name.starts_with("/dev/pts/")
            || matches!(name.as_str(), "/dev/null" | "/dev/random" | "/dev/urandom")
    };
    let check = |open: &[std::path::PathBuf], when: &str| {
        let ptys = open.iter().filter(|p| p.starts_with("/dev/pts/")).count();
        assert!(
            ptys >= all_links().len(),
            "{when}: the daemon holds {ptys} pseudo-terminals and this rig gave it {}: \
             {open:?}",
            all_links().len()
        );
        for device in open {
            assert!(
                allowed(device),
                "{when}: the daemon has {} open. The transmit gate is closed, so the only \
                 devices it may hold are the three pseudo-terminals this rig created: \
                 {open:?}",
                device.display()
            );
        }
    };

    let at_boot = daemon
        .open_devices()
        .expect("the daemon's open descriptors");
    check(&at_boot, "at boot");

    // The configuration it was given names nothing else, which is the other way
    // this could have gone wrong.
    let config = std::fs::read_to_string(rig.config_path()).expect("the rendered configuration");
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("port = ") {
            assert!(
                trimmed.contains("\"/dev/pts/"),
                "the rig configured a port that is not a pseudo-terminal: {trimmed}"
            );
        }
    }

    // Read again after some running. The first reading is taken moments after
    // boot, when the daemon has done nothing but open its links — and "for the
    // whole run" is not something one sample establishes.
    rig.observe(&mut daemon, Duration::from_secs(2))
        .expect("the daemon stays up");
    let after_running = daemon
        .open_devices()
        .expect("the daemon's open descriptors");
    check(&after_running, "after two seconds of polling");
}
