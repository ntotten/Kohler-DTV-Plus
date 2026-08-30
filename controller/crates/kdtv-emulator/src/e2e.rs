//! Ring 3: the real `kdtvd` binary, in its own process, against the emulated
//! devices.
//!
//! Rings 1 and 2 run inside `cargo test` and can lockstep a simulated clock.
//! This one cannot: the thing under test is another process, on the real clock,
//! reached only through three pseudo-terminals and a signal. So this module
//! assembles everything that process refuses to start without, starts it, and
//! hands back the harness and a guard that kills and reaps it on every exit
//! path — a panicking assertion included.
//!
//! It is shared deliberately. `crates/kdtv-emulator/tests/e2e.rs` is the
//! automated caller and `cargo xtask emulate` is the interactive one; a second
//! copy of this setup would be a second thing to keep in step with
//! `deploy/kdtvd.emulated.toml`.
//!
//! # What the daemon refuses to start without
//!
//! `kdtvd`'s module docs give the order, and every one of these has to be true
//! before it opens anything:
//!
//! 1. **Three real ports.** The committed bench file names
//!    [`kdtv_config::port::PTY_PLACEHOLDER`] on all three links, because a
//!    pseudo-terminal path does not exist until something creates the pair.
//!    `ValidatedConfig::bind_ptys` does this substitution in process; a separate
//!    process needs a *file*, so [`Rig`] renders one.
//! 2. **A credential.** `.e2e/<name>/api-token`, at least
//!    `kdtv_api::MIN_TOKEN_BYTES` long, mode `0600` — `kdtvd`'s own
//!    `check_credential_permissions` refuses anything an account other than the
//!    owner can read.
//! 3. **A probe file per zone**, under `bench.probe_dir`, named by
//!    [`kdtv_hal::FileRtdChannel::path_for`]. Writing these is how a test drives
//!    the independent temperature interlock, which is the only safety input in
//!    this system that does not come from the valve.
//! 4. **A closed transmit gate**, which is a property of the build rather than
//!    of anything here: every fixture is tier `[C]`, so the daemon may open a
//!    pseudo-terminal and may not open a real serial port.
//!
//! ~~5. **An enumerable USB serial bus.**~~ Superseded. `resolve_distinct` used
//! to enumerate `/sys/bus/usb-serial/devices` before looking at what any link
//! was bound to, so an all-pseudo-terminal configuration refused to start on
//! every machine with no usbserial driver loaded. This rig carried a
//! mount-namespace shim to synthesise the directory, with a user-namespace
//! fallback for unprivileged runners. `kdtv-hal` now enumerates only when a
//! link canonicalises outside `/dev/pts`, so there is nothing left to work
//! around and the shim is gone.
//!
//! # Nothing in here reads a clock
//!
//! `Instant::now` is denied workspace-wide and this module needs no exception.
//! Every deadline is expressed against [`crate::rig::RunningHarness::elapsed`],
//! which is the same reading the wire is pumped with and the same origin the
//! transcript timestamps carry — so "the all-off was on the wire before the
//! process was gone" is a comparison between two numbers from one clock rather
//! than between two clocks.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io;
use std::io::Write as _;
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

use kdtv_config::port::PTY_PLACEHOLDER;
use kdtv_config::{RealFs, TimingConfig, ValidatedConfig};
use kdtv_hal::FileRtdChannel;
use kdtv_proto::dtv::{DtvRxBuffer, decode as decode_dtv};
use kdtv_proto::saturn::{
    Expectation, MasterAddr, RX_CAPACITY, RxBuffer, ValveAddr, ValveType, decode, opcode,
};
use kdtv_units::{LinkKind, ZoneId};

use crate::rig::{Harness, RunningHarness};
use crate::steam::{AssignAck, SteamAdapterModel, SteamHandle, WriteAck};
use crate::transcript::{Direction, Transcript};
use crate::valve::{SaturnValveModel, TimerRefresh, TwoByteOrder, ValveHandle};
use crate::wire::DeviceModel;

/// The environment variable naming the daemon binary the suite drives.
///
/// Unset means "no daemon was built", which is a skip and not a failure:
/// `scripts/test.sh` runs `cargo test --workspace` on machines that have never
/// built `kdtvd`.
pub const DAEMON_ENV: &str = "KDTV_E2E_DAEMON";

/// The environment variable naming a command prefix for the daemon.
///
/// `qemu-aarch64-static -L /usr/aarch64-linux-gnu` for the emulated-Pi run.
/// Split on whitespace.
pub const RUNNER_ENV: &str = "KDTV_E2E_RUNNER";

/// Where the rig puts everything it generates, relative to the controller
/// workspace root. Already in `.gitignore`.
pub const WORK_DIR: &str = ".e2e";

/// The credential the rig writes.
///
/// Fixed, long enough for `kdtv_api::MIN_TOKEN_BYTES`, and not a secret: it
/// authenticates a loopback socket on a bench rig whose valves are models in
/// this process. A generated one would only make the failure mode "the test
/// cannot log in" harder to read.
const TOKEN: &str = "e2e-token-0123456789abcdef";

/// The resting independent-probe reading the rig starts every zone at.
///
/// 38.0 °C: inside the setpoint clamp, well under the 45 °C corrected trip and
/// the 50 °C raw backstop, and not equal to either — so a test that means to
/// cross one has to say so.
///
/// **It also has to agree with what the valve reports**, to within
/// `kdtv_units::DIVERGENCE_LIMIT_C`. The independent probe and the valve's own
/// thermistor disagreeing by more than 5 °C for
/// `kdtv_units::DIVERGENCE_DWELL` latches the zone, and correctly so: one of
/// them is lying and this project does not know which. `SaturnValveModel`
/// reports `Cx2` 76, which is this number, so an idle rig sits at zero
/// divergence.
pub const RESTING_PROBE_C: &str = "38.0";

/// Why the rig could not be assembled or driven.
#[derive(Debug, thiserror::Error)]
pub enum RigError {
    #[error("{what}: {source}")]
    Io {
        what: String,
        #[source]
        source: io::Error,
    },

    /// The bench template did not contain a key the rig has to substitute.
    #[error(
        "deploy/kdtvd.emulated.toml has no `{key}` under `[{section}]`; the rig has to \
         substitute it before the daemon can start"
    )]
    TemplateKeyMissing {
        section: &'static str,
        key: &'static str,
    },

    /// The rig rendered a configuration the daemon's own parser refuses.
    ///
    /// Checked here rather than left to the daemon: `kdtvd` answers a bad file
    /// with exit code 2 and a line on stderr, and a rig that produced it should
    /// say what it produced rather than making a test read an exit code.
    #[error("the rendered configuration at {path} does not validate: {source}")]
    ConfigRejected {
        path: PathBuf,
        #[source]
        source: kdtv_config::ConfigError,
    },

    /// A placeholder survived the substitution.
    #[error("the rendered configuration still contains `{PTY_PLACEHOLDER}`")]
    PlaceholderSurvived,

    /// A wire condition did not arrive inside its budget.
    #[error("timed out after {waited:?} waiting for {what}\n{context}")]
    Timeout {
        what: String,
        waited: Duration,
        context: String,
    },

    /// The daemon exited while the rig was waiting for something.
    #[error("the daemon exited ({status}) while waiting for {what}\n{context}")]
    DaemonGone {
        what: String,
        status: String,
        context: String,
    },

    /// The harness pump thread stopped, so nothing can be observed any more.
    ///
    /// Distinct from a timeout on purpose. The pump ends on the first I/O
    /// error, on every link at once, and the harness clock keeps running — so
    /// without this the run reports "the daemon did not do X", which is the one
    /// conclusion that is not available.
    #[error(
        "the harness pump thread stopped ({why}) while waiting for {what}, so every link \
         went quiet and nothing further could be observed. This is a harness failure, not \
         a daemon one.\n{context}"
    )]
    PumpStopped {
        what: String,
        why: String,
        context: String,
    },
}

fn io_err(what: impl Into<String>) -> impl FnOnce(io::Error) -> RigError {
    let what = what.into();
    move |source| RigError::Io { what, source }
}

/// The controller workspace root, from this crate's manifest directory.
///
/// `crates/kdtv-emulator` -> `crates` -> the workspace. Resolved at compile
/// time, so it is the tree this binary was built from whatever the working
/// directory is when a test runs.
#[must_use]
pub fn controller_dir() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map_or_else(|| manifest.to_path_buf(), Path::to_path_buf)
}

/// The committed bench configuration the rig renders from.
#[must_use]
pub fn template_path() -> PathBuf {
    controller_dir().join("deploy/kdtvd.emulated.toml")
}

// ---------------------------------------------------------------- the daemon

/// How to invoke the daemon: the binary, and anything that has to run it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DaemonCommand {
    runner: Vec<String>,
    daemon: PathBuf,
}

impl DaemonCommand {
    /// Read [`DAEMON_ENV`] and [`RUNNER_ENV`].
    ///
    /// `Ok(None)` means [`DAEMON_ENV`] is unset — the skip case, and the reason
    /// this returns an option rather than failing. A binary that is named and
    /// then missing is a broken invocation and is reported as one.
    pub fn from_env() -> Result<Option<Self>, String> {
        let Ok(daemon) = std::env::var(DAEMON_ENV) else {
            return Ok(None);
        };
        if daemon.trim().is_empty() {
            return Ok(None);
        }
        let daemon = PathBuf::from(daemon);
        if !daemon.is_file() {
            return Err(format!(
                "{DAEMON_ENV} names {}, which is not a file. scripts/e2e.sh builds it \
                 with `cargo build --package kdtvd`.",
                daemon.display()
            ));
        }
        let runner = std::env::var(RUNNER_ENV)
            .unwrap_or_default()
            .split_whitespace()
            .map(ToOwned::to_owned)
            .collect();
        Ok(Some(Self { runner, daemon }))
    }

    /// Build one explicitly, for `xtask emulate`.
    #[must_use]
    pub fn new(daemon: PathBuf, runner: Vec<String>) -> Self {
        Self { runner, daemon }
    }

    #[must_use]
    pub fn daemon(&self) -> &Path {
        &self.daemon
    }

    #[must_use]
    pub fn runner(&self) -> &[String] {
        &self.runner
    }

    /// The whole argument vector, shim first, then runner, then the daemon.
    ///
    /// Returns the program and its arguments separately because that is what
    /// `Command` wants; an empty vector is impossible, since the daemon path is
    /// always in it.
    fn argv(&self) -> (PathBuf, Vec<String>) {
        let mut all: Vec<String> = Vec::new();
        all.extend(self.runner.iter().cloned());
        all.push(self.daemon.to_string_lossy().into_owned());
        match all.split_first() {
            None => (self.daemon.clone(), Vec::new()),
            Some((first, rest)) => (PathBuf::from(first), rest.to_vec()),
        }
    }
}

impl std::fmt::Display for DaemonCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for r in &self.runner {
            write!(f, "{r} ")?;
        }
        write!(f, "{}", self.daemon.display())
    }
}

/// A running `kdtvd`, killed and reaped when it goes out of scope.
///
/// **The `Drop` is the point.** A leaked daemon holds three pseudo-terminals and
/// a loopback port, and the next run — or the next test in the same run — finds
/// them taken. A panicking assertion unwinds through this, so the guard covers
/// the failure path as well as the success one.
#[derive(Debug)]
pub struct Daemon {
    child: Child,
    command: DaemonCommand,
    log: PathBuf,
    pid_file: PathBuf,
    /// Set once the process has been reaped, so `Drop` does not wait again on a
    /// pid that may since have been reused.
    reaped: Option<ExitStatus>,
}

/// Where [`Daemon::start`] records the pid it spawned, so a daemon orphaned by
/// a test binary that never unwound can be found by the next run.
pub const DAEMON_PID_FILE: &str = "daemon.pid";

/// Kill a `kdtvd` left behind by a previous run of this rig, if there is one.
///
/// Called before the rig's directory is cleared, which is where the record
/// lives. The pid is only signalled when `/proc/<pid>/cmdline` still names the
/// same binary, so a reused pid belonging to something else is left alone.
fn reap_orphaned_daemon(root: &Path) {
    use nix::sys::signal::{Signal, kill};
    use nix::unistd::Pid;

    let Ok(text) = std::fs::read_to_string(root.join(DAEMON_PID_FILE)) else {
        return;
    };
    let mut lines = text.lines();
    let Some(pid) = lines.next().and_then(|s| s.trim().parse::<i32>().ok()) else {
        return;
    };
    let Some(binary) = lines.next().map(str::trim) else {
        return;
    };
    if pid <= 1 || binary.is_empty() {
        return;
    }
    // `/proc/<pid>/cmdline` is NUL-separated. A match means this pid is still
    // the daemon that was recorded rather than a pid the kernel has reissued.
    let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return;
    };
    let named = cmdline
        .split(|b| *b == 0)
        .any(|arg| arg == binary.as_bytes());
    if !named {
        return;
    }
    let mut err = io::stderr();
    let _ = writeln!(
        err,
        "the previous run of {} left {binary} running as pid {pid}; killing it",
        root.display()
    );
    let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
}

impl Daemon {
    /// Start the daemon against a rig.
    ///
    /// stdout and stderr go to `<rig>/daemon.log`. The daemon logs JSON to
    /// stderr, so that file is the journal for this run and is quoted into
    /// every failure this module raises.
    pub fn start(rig: &Rig, command: &DaemonCommand) -> Result<Self, RigError> {
        let log_path = rig.root().join("daemon.log");
        let log = std::fs::File::create(&log_path)
            .map_err(io_err(format!("creating {}", log_path.display())))?;
        let errors = log
            .try_clone()
            .map_err(io_err("duplicating the daemon log handle"))?;

        let (program, leading) = command.argv();
        let child = Command::new(&program)
            .args(leading)
            .arg("--config")
            .arg(rig.config_path())
            .arg("--state-dir")
            .arg(rig.state_dir())
            .current_dir(controller_dir())
            .env("RUST_LOG", "info")
            // systemd's variables, if this happens to run under one. The
            // watchdog and the state directory are both taken from the
            // environment by the daemon, and a rig that inherited either would
            // be driving something other than what it configured.
            .env_remove("STATE_DIRECTORY")
            .env_remove("NOTIFY_SOCKET")
            .env_remove("WATCHDOG_USEC")
            .env_remove("WATCHDOG_PID")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(errors))
            .spawn()
            .map_err(io_err(format!("spawning {}", program.display())))?;

        // So a daemon that outlived its parent can be found and killed by the
        // next run of the same rig. `Drop` is what reaps one normally, and it
        // covers every path that unwinds — but a test binary killed with
        // SIGKILL (an OOM kill, a runner cancelling a step, a double panic
        // aborting the process) runs no destructor, and the orphan does not
        // exit on its own: the next `Rig::start` deletes its probe directory,
        // it latches on RTD starvation and then sits there.
        let pid_file = rig.root().join(DAEMON_PID_FILE);
        std::fs::write(
            &pid_file,
            format!("{}\n{}\n", child.id(), command.daemon().display()),
        )
        .map_err(io_err(format!("writing {}", pid_file.display())))?;

        Ok(Self {
            child,
            command: command.clone(),
            log: log_path,
            pid_file,
            reaped: None,
        })
    }

    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    #[must_use]
    pub fn command(&self) -> &DaemonCommand {
        &self.command
    }

    /// The file the daemon's stdout and stderr went to.
    #[must_use]
    pub fn log_path(&self) -> &Path {
        &self.log
    }

    /// Everything the daemon has written so far, for a failure message.
    #[must_use]
    pub fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_else(|e| format!("(no daemon log: {e})"))
    }

    /// The exit status if the process has already ended.
    pub fn exited(&mut self) -> Result<Option<ExitStatus>, RigError> {
        if let Some(status) = self.reaped {
            return Ok(Some(status));
        }
        match self
            .child
            .try_wait()
            .map_err(io_err("polling the daemon process"))?
        {
            None => Ok(None),
            Some(status) => {
                self.reaped = Some(status);
                Ok(Some(status))
            }
        }
    }

    /// The device nodes this process currently has open, from `/proc`.
    ///
    /// The runtime half of the transmit-gate assertion. The structural half is
    /// that `kdtv_hal::permit_open` refuses `Backend::Serial` under an
    /// emulator-only authority; this says what the process it produced actually
    /// holds.
    pub fn open_devices(&self) -> Result<Vec<PathBuf>, RigError> {
        let dir = PathBuf::from(format!("/proc/{}/fd", self.pid()));
        let entries =
            std::fs::read_dir(&dir).map_err(io_err(format!("reading {}", dir.display())))?;
        let mut out = Vec::new();
        for entry in entries.flatten() {
            // A descriptor can close between the listing and the readlink, so a
            // failure here is not the rig's problem.
            if let Ok(target) = std::fs::read_link(entry.path())
                && target.starts_with("/dev")
            {
                out.push(target);
            }
        }
        out.sort();
        Ok(out)
    }

    /// Send `SIGTERM`. The daemon's own handler turns it into a stop on every
    /// link, so this is the shutdown path under test rather than a kill.
    pub fn terminate(&self) -> Result<(), RigError> {
        use nix::sys::signal::{Signal, kill};
        use nix::unistd::Pid;
        let pid = i32::try_from(self.pid())
            .map_err(|_| io_err("the daemon pid does not fit an i32")(io::Error::other("pid")))?;
        kill(Pid::from_raw(pid), Signal::SIGTERM)
            .map_err(|e| io_err("sending SIGTERM to the daemon")(io::Error::from(e)))
    }

    /// Wait for the process to end, bounded by the harness clock.
    pub fn wait_for_exit(&mut self, rig: &Rig, within: Duration) -> Result<ExitStatus, RigError> {
        let deadline = rig.elapsed().saturating_add(within);
        loop {
            if let Some(status) = self.exited()? {
                return Ok(status);
            }
            if rig.elapsed() >= deadline {
                return Err(RigError::Timeout {
                    what: "the daemon to exit".to_owned(),
                    waited: within,
                    context: rig.render(),
                });
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if self.reaped.is_none() {
            // `kill` is SIGKILL and is correct here: by this point either the
            // test has already exercised the shutdown path or it has failed,
            // and a daemon left holding three pseudo-terminals wedges the next
            // run.
            drop(self.child.kill());
            drop(self.child.wait());
        }
        // Reaped, so the record of it is stale.
        drop(std::fs::remove_file(&self.pid_file));
    }
}

/// How often the rig polls a condition it is waiting on.
///
/// Matches `RunningHarness::PUMP_INTERVAL`: finer would spin against a pump
/// that cannot have advanced, coarser would quantise every measurement the
/// suite makes.
const POLL_INTERVAL: Duration = Duration::from_millis(1);

// ------------------------------------------------------------------- the rig

/// What the emulated devices should be, before the daemon meets them.
///
/// Every field here is a behaviour the sources disagree about, and none has a
/// default this crate is entitled to pick — so the rig picks one *for the
/// suite*, says so, and leaves the switch reachable.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RigOptions {
    /// The address each valve answers at. Discovery probes `0x03..=0x07` and
    /// refuses a bus where more than one answers, so the models are
    /// pre-addressed: an unaddressed valve answers no probe and the zone never
    /// boots.
    pub valve_address: u8,
    /// What refreshes the Prompt 3 runtime timer. Capture question 5.
    pub timer_refresh: TimerRefresh,
    /// Whether the steam adapter acknowledges a parameter write. The two
    /// sources disagree; under [`WriteAck::Silent`] a master that waits for one
    /// times out on every write.
    pub write_ack: WriteAck,
    /// Whether the steam adapter acknowledges the address assignment.
    ///
    /// **The rig picks [`AssignAck::DevAck`], and that is a choice worth
    /// stating rather than burying.** Every source draws the discovery
    /// handshake as three steps with no reply to the third, which is
    /// [`AssignAck::Silent`] and the model's default;
    /// `kdtv_engine::steam::SteamMachine::on_address_assigned` refuses anything
    /// that is not a `DEV_ACK` and gives up with "no device answered
    /// discovery". So the suite cannot exercise steam enrolment as documented —
    /// it exercises it under the only reading in which the daemon enrols at
    /// all, and the disagreement is `kdtv-engine`'s to resolve.
    pub assign_ack: AssignAck,
    /// Which byte order the valves report two-byte fields in. `RESP-05`.
    ///
    /// **The rig picks [`TwoByteOrder::LittleEndian`], and the model's default
    /// is the other one.** No source states the endianness of any multi-byte
    /// Saturn field, so `TwoByteOrder`'s own documentation says its default "is
    /// arbitrary and is not a claim" — but the two halves of this repository
    /// have quietly taken opposite readings, and against the real daemon that
    /// is not survivable:
    /// `kdtv_engine::zone::ZoneMachine::absorb_temperature` reads the **first**
    /// payload byte as the whole `Cx2` (the second byte's role is `[?]`), while
    /// the model under `TwoByteOrder::BigEndian` puts the value in the second
    /// and `0x00` in the first. The daemon therefore reads every valve as
    /// 0.0 °C, the independent probe reads 38 °C, and after the 10 s divergence
    /// dwell **both zones latch** — on every emulated run, about eleven seconds
    /// in, whatever else is being tested.
    ///
    /// So the rig selects the reading under which the two agree. Which one the
    /// valve actually uses is still open, and Phase 1 capture is what answers
    /// it.
    pub two_byte_order: TwoByteOrder,
    /// The valve's own communication-loss shutdown. `None` — a valve that does
    /// **not** close on a quiet bus — is the setting that makes the service's
    /// own fail-off path the thing being tested.
    pub comms_loss_shutdown: Option<Duration>,
}

impl Default for RigOptions {
    fn default() -> Self {
        Self {
            valve_address: 0x03,
            timer_refresh: TimerRefresh::AnyValidCommand,
            write_ack: WriteAck::DevAck,
            assign_ack: AssignAck::DevAck,
            two_byte_order: TwoByteOrder::LittleEndian,
            comms_loss_shutdown: None,
        }
    }
}

/// Everything the daemon needs, plus the wire it will speak on.
#[derive(Debug)]
pub struct Rig {
    root: PathBuf,
    config: PathBuf,
    token: PathBuf,
    probes: PathBuf,
    state: PathBuf,
    logs: PathBuf,
    api: SocketAddr,
    timing: TimingConfig,
    gate: kdtv_config::TransmitGateConfig,
    valve_address: u8,
    harness: RunningHarness,
    valves: BTreeMap<ZoneId, ValveHandle>,
    steam: SteamHandle,
}

impl Rig {
    /// Assemble a rig under `.e2e/<name>` and start pumping the wire.
    ///
    /// `name` scopes everything: the pseudo-terminals are per-rig by
    /// construction, and the directory, the credential, the probe files and the
    /// API port are per-rig by this. Two rigs can run at once, which is what
    /// makes the suite's correctness independent of `--test-threads=1`.
    pub fn start(name: &str, options: &RigOptions) -> Result<Self, RigError> {
        let root = controller_dir().join(WORK_DIR).join(name);
        // Before the record of it is deleted with everything else.
        reap_orphaned_daemon(&root);
        // A previous run's probe files would be read as this run's readings.
        if root.exists() {
            std::fs::remove_dir_all(&root)
                .map_err(io_err(format!("clearing {}", root.display())))?;
        }
        let probes = root.join("probes");
        let state = root.join("state");
        let logs = root.join("logs");
        for dir in [&root, &probes, &state, &logs] {
            std::fs::create_dir_all(dir).map_err(io_err(format!("creating {}", dir.display())))?;
        }

        let token = root.join("api-token");
        write_credential(&token)?;

        for zone in ZoneId::ALL {
            write_probe_file(&probes, zone, RESTING_PROBE_C)?;
        }

        let api = free_loopback_port()?;
        // Decided before anything is opened, so a machine that cannot run the
        // daemon at all says so before three pseudo-terminals exist.

        let address = ValveAddr::new(options.valve_address).unwrap_or(ValveAddr::ALL[0]);
        let zone1 = ValveHandle::new(
            SaturnValveModel::dtv_6_port()
                .preaddressed(address)
                .with_two_byte_order(options.two_byte_order)
                .with_comms_loss_shutdown(options.comms_loss_shutdown),
        );
        let zone2 = ValveHandle::new(
            SaturnValveModel::prompt_3_port(options.timer_refresh)
                .preaddressed(address)
                .with_two_byte_order(options.two_byte_order)
                .with_comms_loss_shutdown(options.comms_loss_shutdown),
        );
        let steam = SteamHandle::new(
            SteamAdapterModel::new(options.write_ack).with_assign_ack(options.assign_ack),
        );

        let devices: Vec<(LinkKind, Box<dyn DeviceModel>)> = vec![
            (LinkKind::Zone(ZoneId::Zone1), Box::new(zone1.clone())),
            (LinkKind::Zone(ZoneId::Zone2), Box::new(zone2.clone())),
            (LinkKind::Steam, Box::new(steam.clone())),
        ];
        let harness = Harness::new(devices).map_err(io_err("allocating the three wires"))?;
        let ports = harness.port_paths();

        let config = root.join("kdtvd.toml");
        let template = template_path();
        let text = std::fs::read_to_string(&template)
            .map_err(io_err(format!("reading {}", template.display())))?;
        let rendered = render_config(
            &text,
            &Substitutions {
                ports: &ports,
                probe_dir: &probes,
                token_file: &token,
                log_dir: &logs,
                api,
            },
        )?;
        std::fs::write(&config, rendered)
            .map_err(io_err(format!("writing {}", config.display())))?;

        // The daemon will do this again, and refuse to start if it fails. Doing
        // it here first means a rig that renders something wrong says what is
        // wrong with it, and hands the suite the tick and response deadlines it
        // has to assert against — read from the file the daemon will run, never
        // restated as a constant in a test.
        let validated =
            ValidatedConfig::load(&config, &RealFs).map_err(|source| RigError::ConfigRejected {
                path: config.clone(),
                source,
            })?;
        let timing = *validated.timing();
        let gate = validated.gate().clone();

        let valves = [(ZoneId::Zone1, zone1), (ZoneId::Zone2, zone2)]
            .into_iter()
            .collect();

        Ok(Self {
            root,
            config,
            token,
            probes,
            state,
            logs,
            api,
            timing,
            gate,
            valve_address: address.get(),
            harness: harness.start_real_time(),
            valves,
            steam,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn config_path(&self) -> &Path {
        &self.config
    }

    #[must_use]
    pub fn token_path(&self) -> &Path {
        &self.token
    }

    #[must_use]
    pub fn probe_dir(&self) -> &Path {
        &self.probes
    }

    #[must_use]
    pub fn state_dir(&self) -> &Path {
        &self.state
    }

    #[must_use]
    pub fn log_dir(&self) -> &Path {
        &self.logs
    }

    /// The loopback address the daemon's API will bind.
    #[must_use]
    pub const fn api_addr(&self) -> SocketAddr {
        self.api
    }

    /// The timings the daemon will run with, read from the rendered file.
    ///
    /// The cadence assertions measure against these rather than against a
    /// constant copied into a test: the tick is `deploy/kdtvd.emulated.toml`'s
    /// to state, and a suite carrying its own copy would keep passing after the
    /// file changed.
    #[must_use]
    pub const fn timing(&self) -> &TimingConfig {
        &self.timing
    }

    /// The polling cadence and response deadline for one link.
    #[must_use]
    pub fn cadence(&self, link: LinkKind) -> Cadence {
        match link {
            LinkKind::Zone(_) => Cadence {
                tick: self.timing.saturn().tick,
                response: self.timing.saturn().response,
            },
            LinkKind::Steam => Cadence {
                tick: self.timing.dtv().tick,
                response: self.timing.dtv().reply,
            },
        }
    }

    #[must_use]
    pub fn port_paths(&self) -> BTreeMap<LinkKind, PathBuf> {
        self.harness.port_paths()
    }

    /// The valve model behind one zone, for injecting a device-side condition
    /// that has no wire encoding.
    #[must_use]
    pub fn valve(&self, zone: ZoneId) -> Option<&ValveHandle> {
        self.valves.get(&zone)
    }

    #[must_use]
    pub const fn steam(&self) -> &SteamHandle {
        &self.steam
    }

    /// Real time since the pump started. The clock every transcript timestamp
    /// and every deadline in this module is measured on.
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.harness.elapsed()
    }

    /// Do something with the harness: inject a fault, read a transcript, hang a
    /// link up. Holds the pump's lock, so nothing in the closure may wait.
    pub fn with<R>(&self, f: impl FnOnce(&mut Harness) -> R) -> R {
        self.harness.with(f)
    }

    /// A copy of one link's transcript, taken under the pump's lock.
    #[must_use]
    pub fn transcript(&self, link: LinkKind) -> Transcript {
        self.with(|h| h.transcript(link).cloned().unwrap_or_default())
    }

    /// Every link's transcript, rendered — the context every failure carries.
    ///
    /// The header names the two things that make a failure diagnosable without
    /// opening the daemon log: the loopback port the rig allocated, because a
    /// lost race for it exits the daemon at boot and otherwise looks like any
    /// other boot failure, and whether the pump thread is still running,
    /// because a dead pump makes every wait time out on a condition that never
    /// had a chance to arrive.
    #[must_use]
    pub fn render(&self) -> String {
        let mut s = format!("api {} | config {}\n", self.api, self.config.display());
        if let Some(why) = self.harness.pump_failure() {
            let _ = writeln!(
                s,
                "THE HARNESS PUMP THREAD STOPPED ({why}). Every link went quiet with it, so \
                 nothing below after that moment is the daemon's doing."
            );
        }
        self.with(|h| {
            for (link, t) in h.transcripts() {
                let _ = writeln!(s, "{link}:");
                s.push_str(&t.render());
            }
        });
        s
    }

    /// The address the rig enrolled both valve models at.
    ///
    /// Every assertion that a stop reached a valve has to name it: a Saturn
    /// frame is addressed, and `0x87 00` sent to an address no valve answers at
    /// closes nothing.
    #[must_use]
    pub const fn valve_address(&self) -> u8 {
        self.valve_address
    }

    /// The transmit-gate section of the configuration the daemon was actually
    /// given, as `kdtv-config` parsed it.
    ///
    /// Read from the rendered file rather than from a constant: the rig renders
    /// `deploy/kdtvd.emulated.toml` line by line and hands the result to a real
    /// process, so this — not the template, and not anything a test states — is
    /// the gate declaration the run was made under.
    #[must_use]
    pub const fn gate(&self) -> &kdtv_config::TransmitGateConfig {
        &self.gate
    }

    /// Why the harness pump thread stopped, if it stopped on an error.
    #[must_use]
    pub fn pump_failure(&self) -> Option<String> {
        self.harness.pump_failure()
    }

    /// Write one zone's independent temperature.
    ///
    /// `<celsius>` or `<celsius> 0x<fault register>`, which is the whole of the
    /// format `kdtv_hal::FileRtdChannel` reads. This is the only safety input in
    /// the system that does not come from the valve, so it is the only one a
    /// test can drive without the valve's cooperation.
    pub fn write_probe(&self, zone: ZoneId, reading: &str) -> Result<(), RigError> {
        write_probe_file(&self.probes, zone, reading)
    }

    /// Stop supplying a zone's independent temperature at all.
    ///
    /// Not the same as writing a cold reading: the channel's `sample` reports a
    /// failed transfer, no sample is absorbed, and the supervisor's own
    /// starvation check is what has to notice.
    pub fn remove_probe(&self, zone: ZoneId) -> Result<(), RigError> {
        let path = FileRtdChannel::path_for(&self.probes, zone);
        std::fs::remove_file(&path).map_err(io_err(format!("removing {}", path.display())))
    }

    /// Stop the pump and join its thread. The transcripts stay readable.
    pub fn stop(&mut self) -> io::Result<()> {
        self.harness.stop()
    }

    /// Block until `ready` says the wire shows what was asked for.
    ///
    /// Bounded by the harness clock and by the daemon still being alive: a
    /// condition that will never arrive because the process died should fail as
    /// that, not as a timeout twenty seconds later.
    pub fn wait_for(
        &self,
        daemon: &mut Daemon,
        what: &str,
        within: Duration,
        mut ready: impl FnMut(&Rig) -> bool,
    ) -> Result<Duration, RigError> {
        let deadline = self.elapsed().saturating_add(within);
        loop {
            if ready(self) {
                return Ok(self.elapsed());
            }
            if let Some(status) = daemon.exited()? {
                return Err(RigError::DaemonGone {
                    what: what.to_owned(),
                    status: format!("{status}\n{}", daemon.log()),
                    context: self.render(),
                });
            }
            // A dead pump makes every link go quiet at once, and `elapsed`
            // keeps advancing off the `Instant` — so this loop still
            // terminates, on a timeout that blames the daemon for a condition
            // the harness stopped being able to observe. Ask before waiting
            // any longer.
            if let Some(why) = self.harness.pump_failure() {
                return Err(RigError::PumpStopped {
                    what: what.to_owned(),
                    why,
                    context: self.render(),
                });
            }
            if self.elapsed() >= deadline {
                return Err(RigError::Timeout {
                    what: what.to_owned(),
                    waited: within,
                    context: format!("{}\n{}", self.render(), daemon.log()),
                });
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Let the wire run on for `span` with no daemon to watch.
    ///
    /// [`Self::observe`] is the call for a live daemon; this is for the moments
    /// after one has exited, when the question is what still arrives. A reply
    /// the emulated device had already scheduled lands during this, so an
    /// assertion about ordering around the exit has something to read rather
    /// than racing the pump for it.
    pub fn settle(&self, span: Duration) {
        let until = self.elapsed().saturating_add(span);
        while self.elapsed() < until {
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// Let the run go on for `span`, failing early if the daemon exits.
    ///
    /// The cadence assertions need a real interval of ordinary running to
    /// measure across; this is that interval, with the one thing that would
    /// make the measurement meaningless — a dead daemon — turned into a
    /// failure rather than a short transcript.
    pub fn observe(&self, daemon: &mut Daemon, span: Duration) -> Result<(), RigError> {
        let until = self.elapsed().saturating_add(span);
        match self.wait_for(daemon, "the observation window to pass", span * 2, |rig| {
            rig.elapsed() >= until
        }) {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Block until both zones have finished the documented boot sequence.
    ///
    /// The condition is read off the wire and nothing else: an all-off was
    /// transmitted and something other than an all-off followed it. Nothing
    /// leaves `ConfirmOff` except an acknowledged all-off, and the only frame
    /// that can follow one while it is still outstanding is a retry of the same
    /// all-off — so a different frame after it means the valve confirmed itself
    /// off. `BOOT-05`.
    pub fn wait_for_boot(
        &self,
        daemon: &mut Daemon,
        within: Duration,
    ) -> Result<Duration, RigError> {
        let address = self.valve_address;
        self.wait_for(
            daemon,
            "both zones to confirm themselves off",
            within,
            move |rig| {
                ZoneId::ALL
                    .iter()
                    .all(|z| zone_is_ready(&rig.transcript(LinkKind::Zone(*z)), address))
            },
        )
    }
}

/// True once this zone's transcript shows an all-off, addressed to the valve,
/// that was acknowledged rather than retried.
#[must_use]
fn zone_is_ready(t: &Transcript, address: u8) -> bool {
    let frames = transmitted_saturn(t);
    let Some(i) = frames.iter().position(|f| f.is_all_off_to(address)) else {
        return false;
    };
    frames.iter().skip(i + 1).any(|f| !f.is_outlet_stop())
}

fn write_credential(path: &Path) -> Result<(), RigError> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::write(path, TOKEN).map_err(io_err(format!("writing {}", path.display())))?;
    // `kdtvd::check_credential_permissions` refuses anything with a group or
    // other bit set, and it is right to: the unit runs with
    // `SupplementaryGroups=dialout`, and anything that can read this can run
    // the shower.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(io_err(format!("setting mode 0600 on {}", path.display())))
}

/// Write one zone's probe reading, atomically.
///
/// `std::fs::write` truncates and then writes, and the reader is **another
/// process** polling on its own schedule — so a plain write leaves a window in
/// which the daemon samples an empty file or `55` where `55.0` was meant. Both
/// are absorbed today (one unparsable sample is far inside the 5 s starvation
/// window, one low sample far inside the 10 s divergence dwell), which is
/// precisely why it would not be found by the tests that exist: it stops being
/// benign the first time something asserts on a dwell shorter than the poll
/// interval. Writing a sibling and renaming makes every reading the daemon sees
/// a reading this rig wrote.
fn write_probe_file(dir: &Path, zone: ZoneId, reading: &str) -> Result<(), RigError> {
    let path = FileRtdChannel::path_for(dir, zone);
    let mut staging = path.clone();
    staging.as_mut_os_string().push(".staging");
    std::fs::write(&staging, format!("{reading}\n"))
        .map_err(io_err(format!("writing {}", staging.display())))?;
    // Same directory, so the rename is atomic rather than a copy.
    std::fs::rename(&staging, &path).map_err(io_err(format!(
        "renaming {} over {}",
        staging.display(),
        path.display()
    )))
}

/// A loopback port nothing is listening on.
///
/// Asking the kernel for one and letting it go is a race in principle. In
/// practice the alternative — a fixed port — is a *certain* collision between
/// two rigs, and this suite is required to run two at once.
///
/// The window between this returning and the daemon's own `bind` is
/// milliseconds wide, and losing it makes the daemon exit at boot. That is
/// diagnosable rather than mysterious only because the address is in
/// [`Rig::render`], which every failure carries: a bind collision and a genuine
/// boot failure otherwise look identical without opening the uploaded log.
fn free_loopback_port() -> Result<SocketAddr, RigError> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(io_err("asking the kernel for a free loopback port"))?;
    let addr = listener
        .local_addr()
        .map_err(io_err("reading the port the kernel gave"))?;
    drop(listener);
    Ok(addr)
}

// --------------------------------------------------------- the configuration

struct Substitutions<'a> {
    ports: &'a BTreeMap<LinkKind, PathBuf>,
    probe_dir: &'a Path,
    token_file: &'a Path,
    log_dir: &'a Path,
    api: SocketAddr,
}

/// Render `deploy/kdtvd.emulated.toml` with this rig's paths.
///
/// Line-oriented and section-aware rather than positional: the three port
/// placeholders are identical strings, so replacing the first, second and third
/// occurrence would bind zone 2's valve to the steam link the day someone
/// reorders the file. Every key the rig owns is rewritten under the section it
/// belongs to, and a missing one is an error rather than a silently unbound
/// link.
///
/// The comments survive, which matters: the rendered file is what a person
/// reads when the daemon refuses to start, and `deploy/kdtvd.emulated.toml`
/// carries the reasoning for every value in it.
fn render_config(template: &str, s: &Substitutions<'_>) -> Result<String, RigError> {
    let mut section = String::new();
    let mut out = String::with_capacity(template.len() + 256);
    let mut done: Vec<(&'static str, &'static str)> = Vec::new();

    for line in template.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            trimmed.trim_matches(['[', ']']).clone_into(&mut section);
        }
        let replacement = match (section.as_str(), key_of(trimmed)) {
            ("zones.zone1", Some("port")) => port_line(s, LinkKind::Zone(ZoneId::Zone1)),
            ("zones.zone2", Some("port")) => port_line(s, LinkKind::Zone(ZoneId::Zone2)),
            ("steam", Some("port")) => port_line(s, LinkKind::Steam),
            ("bench", Some("probe_dir")) => Some(quoted("probe_dir", s.probe_dir)),
            ("api", Some("token_file")) => Some(quoted("token_file", s.token_file)),
            ("api", Some("bind")) => Some(format!("bind = \"{}\"", s.api)),
            ("logging", Some("directory")) => Some(quoted("directory", s.log_dir)),
            _ => None,
        };
        match replacement {
            None => out.push_str(line),
            Some(text) => {
                if let Some(key) = key_of(trimmed) {
                    done.push((leak_section(&section), leak_key(key)));
                }
                out.push_str(&text);
            }
        }
        out.push('\n');
    }

    for (section, key) in REQUIRED_KEYS {
        if !done.iter().any(|(s, k)| s == section && k == key) {
            return Err(RigError::TemplateKeyMissing { section, key });
        }
    }
    if out.contains(PTY_PLACEHOLDER) {
        return Err(RigError::PlaceholderSurvived);
    }
    Ok(out)
}

/// Every key the rig has to substitute before the daemon will start.
const REQUIRED_KEYS: &[(&str, &str)] = &[
    ("zones.zone1", "port"),
    ("zones.zone2", "port"),
    ("steam", "port"),
    ("bench", "probe_dir"),
    ("api", "token_file"),
    ("api", "bind"),
    ("logging", "directory"),
];

/// The key on a `key = value` line, ignoring comments and blanks.
fn key_of(trimmed: &str) -> Option<&str> {
    if trimmed.starts_with('#') {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    (!key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).then_some(key)
}

/// The value assigned to `scope` under `[transmit_gate]`, as the rendered file
/// spells it.
///
/// A substring search over the whole file is not this: the template's own
/// commentary contains the string `"emulator-only"` in prose, so
/// `contains("emulator-only")` is satisfied by a file whose actual assignment
/// says `real-bus-attested`. Section-aware, comment-aware, and it returns what
/// the assignment says rather than whether some spelling of it appears.
#[must_use]
pub fn rendered_gate_scope(config: &str) -> Option<String> {
    let mut section = String::new();
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            trimmed.trim_matches(['[', ']']).clone_into(&mut section);
            continue;
        }
        if section != "transmit_gate" || key_of(trimmed) != Some("scope") {
            continue;
        }
        let (_, value) = trimmed.split_once('=')?;
        return Some(value.trim().trim_matches('"').to_owned());
    }
    None
}

fn port_line(s: &Substitutions<'_>, link: LinkKind) -> Option<String> {
    s.ports.get(&link).map(|p| quoted("port", p))
}

fn quoted(key: &str, path: &Path) -> String {
    format!("{key} = \"{}\"", path.display())
}

/// The section and key names are compared against `REQUIRED_KEYS`, which is
/// `&'static str`. Matching on the parsed strings and recording the static one
/// keeps the comparison exact rather than by content.
fn leak_section(section: &str) -> &'static str {
    REQUIRED_KEYS
        .iter()
        .find(|(s, _)| *s == section)
        .map_or("", |(s, _)| *s)
}

fn leak_key(key: &str) -> &'static str {
    REQUIRED_KEYS
        .iter()
        .find(|(_, k)| *k == key)
        .map_or("", |(_, k)| *k)
}

// ------------------------------------------------------------- reading a wire

/// One Saturn frame the daemon put on the wire, with the moment it crossed.
///
/// The oracle for every assertion in the suite. It is decoded from the
/// transcript rather than taken from the daemon, so a service that believes it
/// is off while transmitting an open frame fails here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TxFrame {
    /// Offset from the start of the run, on the harness clock.
    pub at: Duration,
    /// The frame's **destination** — Saturn carries no sender field.
    pub address: u8,
    pub control: u8,
    pub data: Vec<u8>,
}

impl TxFrame {
    /// `0x87` with an empty bitmap, addressed to `to`: **the all-off**.
    ///
    /// The bitmap is byte 0 and the flags byte follows it; only the bitmap
    /// decides whether water moves. The destination is checked because a
    /// Saturn frame is addressed and a stop sent to an address no valve
    /// answers at is bytes on the wire that close nothing — which is exactly
    /// what an assertion reading "water stopped" must not accept. The address
    /// to pass is [`Rig::valve_address`], the one the rig enrolled the models
    /// at.
    #[must_use]
    pub fn is_all_off_to(&self, to: u8) -> bool {
        self.address == to && self.is_outlet_stop()
    }

    /// `0x87` with an empty bitmap, wherever it was addressed.
    ///
    /// Only for the two questions where the address genuinely does not matter:
    /// "was a stop encoded at all", and — negated — "was this frame something
    /// other than a retry of the stop". Anything asserting that water actually
    /// stopped wants [`Self::is_all_off_to`].
    #[must_use]
    pub fn is_outlet_stop(&self) -> bool {
        self.control == opcode::WRITE_OUTLET_STATES && self.data.first() == Some(&0x00)
    }

    /// `0x87` with any bit set: a frame that opens water.
    #[must_use]
    pub fn opens_water(&self) -> bool {
        self.control == opcode::WRITE_OUTLET_STATES
            && self.data.first().is_some_and(|bits| *bits != 0x00)
    }

    /// A discovery probe: the firmware-type read the boot sequence walks
    /// `0x03..=0x07` with.
    #[must_use]
    pub fn is_probe(&self) -> bool {
        self.control == opcode::READ_FIRMWARE_TYPE
    }

    #[must_use]
    pub fn hex(&self) -> String {
        let mut s = format!("{:02X} {:02X}", self.address, self.control);
        for b in &self.data {
            let _ = write!(s, " {b:02X}");
        }
        s
    }
}

/// Split everything the daemon transmitted on a Saturn link into frames.
///
/// The buffer is carried across transcript entries: a pseudo-terminal read can
/// split a write, so an entry is not a frame. Undecodable bytes are skipped the
/// way the real decoder skips them — this is the same decoder, in capture mode,
/// which is what makes "the daemon transmitted an all-off" mean the same thing
/// here as on the link.
#[must_use]
pub fn transmitted_saturn(t: &Transcript) -> Vec<TxFrame> {
    let expect = Expectation::capture(MasterAddr::Dtv);
    let mut rx = RxBuffer::new();
    let mut out = Vec::new();
    for entry in t.transmitted() {
        rx.extend(&entry.bytes);
        for _ in 0..RX_CAPACITY {
            match decode(&mut rx, &expect) {
                Ok(None) => break,
                Ok(Some(f)) => out.push(TxFrame {
                    at: entry.at,
                    address: f.address,
                    control: f.control.0,
                    data: f.data.as_slice().to_vec(),
                }),
                Err(_) => {}
            }
        }
    }
    out
}

/// When each **frame** the daemon transmitted on a link crossed it, in order.
///
/// The link kind selects the codec. Use this, never
/// `Transcript::transmitted().map(|e| e.at)`, for anything that measures an
/// interval: an entry is one pump read, and one pump read coalesces every byte
/// the daemon wrote in the preceding millisecond. A burst of three frames is
/// one entry with one timestamp, which reads as a single well-spaced
/// transmission — and hiding a burst is the one thing a cadence assertion in
/// this project must not do.
#[must_use]
pub fn transmitted_at(t: &Transcript, link: LinkKind) -> Vec<Duration> {
    match link {
        LinkKind::Zone(_) => transmitted_saturn(t).iter().map(|f| f.at).collect(),
        LinkKind::Steam => transmitted_dtv(t).iter().map(|f| f.at).collect(),
    }
}

/// The gaps between consecutive transmitted frames on a link, in order.
#[must_use]
pub fn transmit_gaps(t: &Transcript, link: LinkKind) -> Vec<Duration> {
    transmitted_at(t, link)
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some(b.saturating_sub(*a)),
            _ => None,
        })
        .collect()
}

/// Every **frame** the daemon transmitted on a link, paired with the device
/// reply that answered it, if one did.
///
/// The shape the one-transaction-in-flight assertion needs: a second frame that
/// went out before the first was answered has `None` here **and** a gap shorter
/// than the response deadline, which is the difference between a serialised bus
/// and a timeout.
///
/// # Frames, not transcript entries
///
/// This decodes, and the link kind selects the codec, because an entry is not a
/// frame. [`crate::wire::Wire::pump`] does one `read(2)` per link per
/// millisecond and records everything it got as a single entry, so a daemon
/// that writes a poll and a second frame microseconds later — the I1 shape,
/// and the defect this assertion exists for — produces **one** entry with one
/// timestamp. Pairing entries would see one transaction, answered, and report
/// a serialised bus. Pairing decoded frames sees two, the second unanswered
/// and zero milliseconds after the first, and fails.
///
/// Both frames then carry the same `sent_at`, since a timestamp is when the
/// pump observed the bytes rather than when the daemon wrote them. That is the
/// right reading here: a zero-millisecond gap is the strongest possible
/// statement of "these went out together".
#[must_use]
pub fn transactions(t: &Transcript, link: LinkKind) -> Vec<Transaction> {
    let sent: Vec<Duration> = match link {
        LinkKind::Zone(_) => transmitted_saturn(t).iter().map(|f| f.at).collect(),
        LinkKind::Steam => transmitted_dtv(t).iter().map(|f| f.at).collect(),
    };
    let mut out: Vec<Transaction> = Vec::with_capacity(sent.len());
    let mut next = sent.iter().peekable();
    for e in t.entries() {
        match e.direction {
            Direction::DaemonToDevice => {
                // Every frame whose last byte was observed in this entry.
                while next.peek().is_some_and(|at| **at <= e.at) {
                    let Some(at) = next.next() else { break };
                    out.push(Transaction {
                        sent_at: *at,
                        answered_at: None,
                    });
                }
            }
            Direction::DeviceToDaemon => {
                if let Some(last) = out.last_mut()
                    && last.answered_at.is_none()
                {
                    last.answered_at = Some(e.at);
                }
            }
        }
    }
    // Anything still undecoded at the end of the transcript is a partial frame
    // and is not a transaction.
    out
}

/// One request on the wire, and when it was answered.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Transaction {
    pub sent_at: Duration,
    pub answered_at: Option<Duration>,
}

/// One link's documented timing, as the configuration states it.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub struct Cadence {
    /// One request per tick per port, no faster. `TIME-01`.
    pub tick: Duration,
    /// How long a request may go unanswered before it is a timeout.
    pub response: Duration,
}

/// One DTV+ frame the daemon put on the wire.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DtvTx {
    pub at: Duration,
    pub dest: u8,
    pub src: u8,
    pub cmd: u8,
    pub payload: Vec<u8>,
}

/// Split everything the daemon transmitted on the steam link into frames.
///
/// The same shape as [`transmitted_saturn`], against the other codec. DTV+ has
/// no length field — extent comes from the delimiters — so the buffer is
/// carried across entries for the same reason.
#[must_use]
pub fn transmitted_dtv(t: &Transcript) -> Vec<DtvTx> {
    let mut rx = DtvRxBuffer::new();
    let mut out = Vec::new();
    for entry in t.transmitted() {
        rx.extend(&entry.bytes);
        for _ in 0..kdtv_proto::dtv::RX_CAPACITY {
            match decode_dtv(&mut rx) {
                Ok(None) => break,
                Ok(Some(f)) => out.push(DtvTx {
                    at: entry.at,
                    dest: f.dest,
                    src: f.src,
                    cmd: f.cmd,
                    payload: f.payload.as_slice().to_vec(),
                }),
                Err(_) => {}
            }
        }
    }
    out
}

/// The valve family each zone is configured with, for a message that names it.
#[must_use]
pub const fn valve_type(zone: ZoneId) -> ValveType {
    match zone {
        ZoneId::Zone1 => ValveType::Dtv6Port,
        ZoneId::Zone2 => ValveType::Prompt3Port,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subs(dir: &Path) -> BTreeMap<LinkKind, PathBuf> {
        LinkKind::ALL
            .iter()
            .enumerate()
            .map(|(i, l)| (*l, dir.join(format!("pts{i}"))))
            .collect()
    }

    fn render(template: &str) -> Result<String, RigError> {
        let dir = Path::new("/tmp/rig");
        let ports = subs(dir);
        render_config(
            template,
            &Substitutions {
                ports: &ports,
                probe_dir: Path::new("/tmp/rig/probes"),
                token_file: Path::new("/tmp/rig/api-token"),
                log_dir: Path::new("/tmp/rig/logs"),
                api: "127.0.0.1:19999".parse().expect("a loopback address"),
            },
        )
    }

    /// The committed bench file is the one the rig renders, so a key that moves
    /// in it breaks here rather than in a daemon that will not start.
    #[test]
    fn the_committed_bench_file_renders() {
        let text = std::fs::read_to_string(template_path()).expect("the bench template");
        let out = render(&text).expect("it renders");
        assert!(!out.contains(PTY_PLACEHOLDER), "{out}");
        assert!(out.contains("port = \"/tmp/rig/pts0\""), "{out}");
        assert!(out.contains("port = \"/tmp/rig/pts1\""), "{out}");
        assert!(out.contains("port = \"/tmp/rig/pts2\""), "{out}");
        assert!(out.contains("probe_dir = \"/tmp/rig/probes\""), "{out}");
        assert!(out.contains("token_file = \"/tmp/rig/api-token\""), "{out}");
        assert!(out.contains("bind = \"127.0.0.1:19999\""), "{out}");
        assert!(out.contains("directory = \"/tmp/rig/logs\""), "{out}");
        // The reasoning in the comments is the reason this is a rewrite rather
        // than a fresh file.
        assert!(
            out.contains("# ---"),
            "the comments survive rendering:\n{out}"
        );
    }

    /// **The rendered file still declares a closed gate.**
    ///
    /// The rig hands this text to a real process, so it — not the template, and
    /// not anything a test states — is the gate declaration every end-to-end
    /// run and every `xtask emulate` is made under.
    ///
    /// The predecessor of this test asserted `out.contains("emulator-only")`,
    /// which the template satisfies from a **comment**: line 27 reads "`scope`
    /// must be \"emulator-only\" and ...". Changing the real assignment on line
    /// 34 to `real-bus-attested` left it green. So the value is parsed here,
    /// with the same parser the daemon uses.
    #[test]
    fn the_rendered_configuration_declares_an_emulator_only_gate() {
        let text = std::fs::read_to_string(template_path()).expect("the bench template");
        let scope = rendered_gate_scope(&render(&text).expect("it renders"));
        assert_eq!(
            scope.as_deref(),
            Some("emulator-only"),
            "deploy/kdtvd.emulated.toml declares scope = {scope:?} under [transmit_gate]. \
             Every fixture in this repository is tier [C]: the daemon this rig starts must \
             be asked for the emulator scope and nothing else."
        );
    }

    /// A `scope` under any other section is not the gate's.
    #[test]
    fn the_gate_scope_is_read_from_the_gate_section() {
        let text = "scope = \"real-bus-attested\"\n[transmit_gate]\nscope = \"emulator-only\"\n\
                    [other]\nscope = \"real-bus-attested\"\n";
        assert_eq!(rendered_gate_scope(text).as_deref(), Some("emulator-only"));
        // And a commented-out assignment is not an assignment.
        let commented =
            "[transmit_gate]\n# scope = \"emulator-only\"\nscope = \"real-bus-attested\"\n";
        assert_eq!(
            rendered_gate_scope(commented).as_deref(),
            Some("real-bus-attested")
        );
    }

    /// The three placeholders are the same string. Substituting by occurrence
    /// would bind the wrong link the day the file is reordered, so the check is
    /// that reordering changes nothing.
    #[test]
    fn the_links_are_bound_by_section_not_by_order() {
        let reordered = "\
profile = \"bench\"
[steam]
enabled = true
port = \"/dev/pts/PLACEHOLDER\"
[zones.zone2]
port = \"/dev/pts/PLACEHOLDER\"
[zones.zone1]
port = \"/dev/pts/PLACEHOLDER\"
[bench]
probe_dir = \".e2e/probes\"
[api]
bind = \"127.0.0.1:8443\"
token_file = \".e2e/api-token\"
[logging]
directory = \".e2e/logs\"
";
        let out = render(reordered).expect("it renders");
        let steam = out.find("[steam]").expect("the steam section");
        let zone2 = out.find("[zones.zone2]").expect("the zone2 section");
        let zone1 = out.find("[zones.zone1]").expect("the zone1 section");
        let pts = |name: &str| {
            out.find(&format!("port = \"/tmp/rig/{name}\""))
                .unwrap_or(0)
        };
        // pts0 is zone1, pts1 is zone2, pts2 is steam — wherever the sections
        // happen to sit.
        assert!(pts("pts2") > steam && pts("pts2") < zone2);
        assert!(pts("pts1") > zone2 && pts("pts1") < zone1);
        assert!(pts("pts0") > zone1);
    }

    #[test]
    fn a_template_missing_a_key_the_rig_owns_is_refused() {
        let err = render("profile = \"bench\"\n").expect_err("nothing to substitute");
        assert!(matches!(err, RigError::TemplateKeyMissing { .. }), "{err}");
    }

    #[test]
    fn a_commented_key_is_not_a_key() {
        assert_eq!(key_of("# port = \"x\""), None);
        assert_eq!(key_of("port = \"x\""), Some("port"));
        assert_eq!(key_of("[zones.zone1]"), None);
        assert_eq!(key_of(""), None);
        // A table entry inside an array is not a top-level key.
        assert_eq!(key_of("{ slot = 1, status_index = 1 }"), None);
    }

    #[test]
    fn frames_are_decoded_across_a_split_write() {
        let mut t = Transcript::new();
        let frame = crate::valve::raw_saturn(0x03, opcode::WRITE_OUTLET_STATES, &[0x00, 0x00]);
        let (head, tail) = frame.split_at(3);
        t.record(Duration::from_millis(1), Direction::DaemonToDevice, head);
        t.record(Duration::from_millis(2), Direction::DaemonToDevice, tail);
        let frames = transmitted_saturn(&t);
        assert_eq!(frames.len(), 1, "{frames:?}");
        assert!(frames[0].is_all_off_to(0x03));
        assert!(!frames[0].opens_water());
        assert_eq!(frames[0].at, Duration::from_millis(2));
    }

    #[test]
    fn an_open_frame_is_not_an_all_off() {
        let mut t = Transcript::new();
        t.record(
            Duration::ZERO,
            Direction::DaemonToDevice,
            &crate::valve::raw_saturn(0x03, opcode::WRITE_OUTLET_STATES, &[0x04, 0x00]),
        );
        let frames = transmitted_saturn(&t);
        assert!(frames[0].opens_water());
        assert!(!frames[0].is_all_off_to(0x03));
    }

    /// An all-off is addressed. A stop sent to an address no valve answers at
    /// is bytes on the wire that close nothing, and every "water stopped"
    /// assertion in the suite reads this predicate.
    #[test]
    fn an_all_off_to_the_wrong_address_is_not_an_all_off() {
        let mut t = Transcript::new();
        t.record(
            Duration::ZERO,
            Direction::DaemonToDevice,
            &crate::valve::raw_saturn(0x05, opcode::WRITE_OUTLET_STATES, &[0x00, 0x00]),
        );
        let frames = transmitted_saturn(&t);
        assert!(
            frames[0].is_outlet_stop(),
            "it is a stop, and it is on the wire"
        );
        assert!(
            !frames[0].is_all_off_to(0x03),
            "but the valve at 0x03 never saw it"
        );
        assert!(frames[0].is_all_off_to(0x05));
    }

    const ZONE1: LinkKind = LinkKind::Zone(ZoneId::Zone1);

    fn poll_frame() -> Vec<u8> {
        crate::valve::raw_saturn(0x03, opcode::READ_TEMPERATURE, &[])
    }

    #[test]
    fn a_transaction_pairs_a_request_with_the_reply_that_answered_it() {
        let mut t = Transcript::new();
        t.record(
            Duration::from_millis(0),
            Direction::DaemonToDevice,
            &poll_frame(),
        );
        t.record(Duration::from_millis(10), Direction::DeviceToDaemon, &[2]);
        t.record(
            Duration::from_millis(20),
            Direction::DaemonToDevice,
            &poll_frame(),
        );
        let tx = transactions(&t, ZONE1);
        assert_eq!(tx.len(), 2);
        assert_eq!(tx[0].answered_at, Some(Duration::from_millis(10)));
        assert_eq!(tx[1].answered_at, None);
        assert_eq!(
            transmit_gaps(&t, ZONE1),
            vec![Duration::from_millis(20)],
            "device replies are not transmissions"
        );
    }

    /// **The measurement that decides whether ring 3 can see an I1 regression
    /// at all.**
    ///
    /// `Wire::pump` reads once per link per millisecond and records everything
    /// it got as one entry, so a daemon that puts a second frame out
    /// microseconds after the first — without waiting for the answer, which is
    /// exactly what hung the K-99695 — writes two frames into one transcript
    /// entry. Measured between entries that is one well-spaced transmission
    /// and one answered transaction. Measured between decoded frames it is two
    /// transmissions zero milliseconds apart, the second unanswered.
    #[test]
    fn two_frames_in_one_pump_read_are_two_transactions_not_one() {
        let mut burst = poll_frame();
        burst.extend_from_slice(&poll_frame());
        let mut t = Transcript::new();
        t.record(
            Duration::from_millis(500),
            Direction::DaemonToDevice,
            &burst,
        );
        t.record(Duration::from_millis(510), Direction::DeviceToDaemon, &[2]);

        assert_eq!(
            t.entries().len(),
            2,
            "one pump read coalesced the burst into one entry"
        );
        let tx = transactions(&t, ZONE1);
        assert_eq!(tx.len(), 2, "and it carried two frames: {tx:?}");
        assert_eq!(tx[0].answered_at, None, "the first was never answered");
        assert_eq!(tx[0].sent_at, tx[1].sent_at, "they went out together");
        assert_eq!(
            transmit_gaps(&t, ZONE1),
            vec![Duration::ZERO],
            "a zero-millisecond gap is what a cadence assertion has to see"
        );
    }

    /// The same conflation on the steam link, which uses the other codec.
    #[test]
    fn the_steam_link_is_split_into_frames_too() {
        let one = crate::steam::raw_dtv(0x01, 0x00, kdtv_proto::dtv::opcode::GET_DEV_STATUS, &[]);
        let mut burst = one.clone();
        burst.extend_from_slice(&one);
        let mut t = Transcript::new();
        t.record(
            Duration::from_millis(150),
            Direction::DaemonToDevice,
            &burst,
        );
        assert_eq!(transactions(&t, LinkKind::Steam).len(), 2);
        assert_eq!(transmit_gaps(&t, LinkKind::Steam), vec![Duration::ZERO]);
    }

    /// The readiness condition is "an all-off, then something else", and it must
    /// not fire on an all-off that is still being retried.
    #[test]
    fn a_retried_all_off_is_not_a_confirmed_one() {
        let all_off = crate::valve::raw_saturn(0x03, opcode::WRITE_OUTLET_STATES, &[0x00, 0x00]);
        let poll = crate::valve::raw_saturn(0x03, opcode::READ_TEMPERATURE, &[]);
        let mut t = Transcript::new();
        t.record(
            Duration::from_millis(0),
            Direction::DaemonToDevice,
            &all_off,
        );
        assert!(!zone_is_ready(&t, 0x03));
        t.record(
            Duration::from_millis(525),
            Direction::DaemonToDevice,
            &all_off,
        );
        assert!(
            !zone_is_ready(&t, 0x03),
            "a retry is another all-off, not a confirmation"
        );
        t.record(
            Duration::from_millis(1050),
            Direction::DaemonToDevice,
            &poll,
        );
        assert!(zone_is_ready(&t, 0x03));
        assert!(
            !zone_is_ready(&t, 0x05),
            "no valve at 0x05 was ever commanded off"
        );
    }

    /// The three readings the rig picks, pinned with their reasons.
    ///
    /// Each is a place where two sources, or a source and this repository's own
    /// engine, disagree, and each was found by running the daemon rather than
    /// by reading. A default that drifts back would not fail loudly — it would
    /// make the whole suite fail somewhere unrelated — so the choices are
    /// asserted here where the reason is written down.
    #[test]
    fn the_rig_pins_the_readings_the_daemon_needs() {
        let o = RigOptions::default();

        // Under BigEndian the model puts Cx2 in the second payload byte and
        // `kdtv_engine::zone::ZoneMachine::absorb_temperature` reads the first,
        // so every valve reads 0.0 C against a 38 C probe: 38 C of divergence,
        // and both zones latch ten seconds into every run.
        assert_eq!(o.two_byte_order, TwoByteOrder::LittleEndian);

        // Under Silent — which is what every source draws — `kdtv-engine`
        // refuses the enrolment and the steam link never comes up.
        assert_eq!(o.assign_ack, AssignAck::DevAck);

        // An unaddressed valve answers no probe, so discovery finds nothing and
        // the zone latches before it can be tested. The address is inside
        // `0x03..=0x07`, which is the range discovery scans.
        assert!(ValveAddr::new(o.valve_address).is_ok());

        // And the resting probe agrees with what the valve reports, or the
        // divergence check latches the zone whatever else is under test.
        //
        // `reported_temperature`, not `setpoint`. A `0x0B` read answers the
        // `reported` field, which has its own setter documented as
        // "independent of the setpoint" and only follows the setpoint when one
        // is commanded. The two are equal today by coincidence of their
        // construction defaults, and asserting the setpoint would read a field
        // the daemon never sees — which is the exact drift this test exists to
        // catch, since the consequence is every ring-3 test latching on
        // `TemperatureDivergence` ten seconds in.
        let resting: f32 = RESTING_PROBE_C.parse().expect("a Celsius reading");
        for zone in ZoneId::ALL {
            let model = match zone {
                ZoneId::Zone1 => SaturnValveModel::dtv_6_port(),
                ZoneId::Zone2 => SaturnValveModel::prompt_3_port(TimerRefresh::AnyValidCommand),
            };
            let answers = model.reported_temperature().celsius();
            assert!(
                (resting - answers).abs() < kdtv_units::DIVERGENCE_LIMIT_C,
                "{zone}: the resting probe reads {resting} C and a 0x0B read answers \
                 {answers} C, which is {} C of divergence against a {} C limit",
                (resting - answers).abs(),
                kdtv_units::DIVERGENCE_LIMIT_C
            );
        }
    }

    /// A daemon orphaned by a test binary that never unwound is killed by the
    /// next run of the same rig.
    ///
    /// `Daemon::drop` covers every path that unwinds, and a panicking assertion
    /// is one — but a test process killed outright (an OOM kill, a runner
    /// cancelling a step, a double panic aborting) runs no destructor, and the
    /// orphan does not exit on its own: the next `Rig::start` deletes its probe
    /// directory, it latches on RTD starvation and then sits there polling.
    ///
    /// `PR_SET_PDEATHSIG` would be the direct fix and needs `pre_exec`, which
    /// is `unsafe`; this workspace forbids it. So the pid is recorded and
    /// checked instead.
    /// A `/bin/sleep` that is definitely running `/bin/sleep`.
    ///
    /// `Command::spawn` returns after the fork and before the exec, so for a
    /// short window `/proc/<pid>/cmdline` still holds *this* binary's command
    /// line. `reap_orphaned_daemon` compares against it and correctly declines
    /// to signal a pid it cannot identify — so a test that reaped immediately
    /// after spawning was racing the exec, and lost on a loaded machine. It then
    /// waited out the sleep, which is why this used to take five minutes to fail.
    ///
    /// The window is why the second half of that test could also pass for the
    /// wrong reason: an un-exec'd process does not match the decoy name either.
    fn spawn_sleeper() -> Child {
        // Seconds, not minutes. A regression that stops the reap working should
        // cost the suite this long, not `cargo test --workspace`'s patience.
        let mut child = Command::new("/bin/sleep")
            .arg("20")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("a process to orphan");
        let pid = child.id();
        for _ in 0..500 {
            let named = std::fs::read(format!("/proc/{pid}/cmdline"))
                .map(|c| c.split(|b| *b == 0).any(|a| a == b"/bin/sleep"))
                .unwrap_or(false);
            if named {
                return child;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Reap it before failing: a panicking helper that leaves a process
        // behind is the thing this whole test is about.
        drop(child.kill());
        drop(child.wait());
        panic!("/bin/sleep never appeared in /proc/{pid}/cmdline");
    }

    #[test]
    fn a_daemon_orphaned_by_a_dead_test_process_is_killed_by_the_next_run() {
        let dir = controller_dir()
            .join(WORK_DIR)
            .join("reap-orphaned-daemon-test");
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("the directory");

        let mut child = spawn_sleeper();
        let pid = child.id();
        std::fs::write(dir.join(DAEMON_PID_FILE), format!("{pid}\n/bin/sleep\n"))
            .expect("the record");

        reap_orphaned_daemon(&dir);
        // `wait` returns because the process is this one's child; the point is
        // that it ended, and it was not going to for another five minutes.
        let status = child.wait().expect("it was signalled");
        assert!(!status.success(), "{status}");

        // And a pid the kernel has since reissued to something else is left
        // alone: the recorded binary has to still be what is running.
        let mut other = spawn_sleeper();
        std::fs::write(
            dir.join(DAEMON_PID_FILE),
            format!("{}\n/usr/bin/kdtvd-that-is-not-this\n", other.id()),
        )
        .expect("the record");
        reap_orphaned_daemon(&dir);
        assert!(
            other.try_wait().expect("polling it").is_none(),
            "a process whose cmdline does not name the recorded daemon was killed"
        );
        drop(other.kill());
        drop(other.wait());
        drop(std::fs::remove_dir_all(&dir));
    }

    /// The daemon reads the probe file from another process on its own
    /// schedule, so a truncate-then-write leaves a window in which it samples
    /// an empty file or half a number. Writing a sibling and renaming closes
    /// it.
    #[test]
    fn a_probe_reading_is_written_atomically() {
        let dir = controller_dir().join(WORK_DIR).join("atomic-probe-test");
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("the directory");

        write_probe_file(&dir, ZoneId::Zone1, "38.0").expect("the first reading");
        write_probe_file(&dir, ZoneId::Zone1, "55.0").expect("the second");
        let path = FileRtdChannel::path_for(&dir, ZoneId::Zone1);
        assert_eq!(
            std::fs::read_to_string(&path).expect("the reading"),
            "55.0\n"
        );
        // Nothing left behind that a directory scan would find.
        let stray: Vec<_> = std::fs::read_dir(&dir)
            .expect("the directory")
            .flatten()
            .map(|e| e.file_name())
            .filter(|n| n != path.file_name().unwrap_or_default())
            .collect();
        assert!(stray.is_empty(), "{stray:?}");
        drop(std::fs::remove_dir_all(&dir));
    }

    #[test]
    fn the_rig_names_the_valve_each_zone_is_configured_with() {
        assert_eq!(valve_type(ZoneId::Zone1), ValveType::Dtv6Port);
        assert_eq!(valve_type(ZoneId::Zone2), ValveType::Prompt3Port);
    }
}
