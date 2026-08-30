//! `cargo xtask emulate` — the whole system, with no hardware.
//!
//! The interactive half of ring 3. `crates/kdtv-emulator/tests/e2e.rs` is the
//! automated one, and both build their rig with [`kdtv_emulator::e2e`] rather
//! than each assembling a daemon environment of its own: the credential, the
//! probe files, the rendered configuration and the pseudo-terminal paths are
//! all things `deploy/kdtvd.emulated.toml` has an opinion about, and two copies
//! of that opinion would drift.
//!
//! What this adds over the suite is the part a person wants: it prints where
//! everything is, then streams the transcript — the actual bytes on each of the
//! three links — until `Ctrl-C`.
//!
//! # Ctrl-C
//!
//! `SIGINT` from the terminal reaches every process in the foreground group,
//! which is this one *and* the daemon. The daemon's own handler turns it into a
//! stop on every link and an orderly exit, which is exactly the path worth
//! watching; this process must therefore not die first, or the wire would stop
//! being pumped half way through that shutdown and the stop would never be
//! answered. So the `tokio` runtime here exists for one reason:
//! `tokio::signal::ctrl_c` handles `SIGINT` instead of letting it terminate,
//! and the loop below then waits for the daemon to finish stopping the water.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use kdtv_emulator::e2e::{
    Daemon, DaemonCommand, Rig, RigOptions, SysfsShim, USB_SERIAL_DEVICES, controller_dir,
};
use kdtv_emulator::rig::all_links;
use kdtv_emulator::transcript::Direction;
use kdtv_units::LinkKind;

/// Which binary to run.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    /// This machine's own binary.
    Native,
    /// The ARM64 binary that gets deployed, under `qemu-aarch64-static`. It
    /// catches what a native run cannot — pointer width, alignment, and
    /// anything architecture-dependent in a dependency — and it emulates none
    /// of the Pi's peripherals, which is why it still needs this rig.
    PiSim,
}

impl Mode {
    pub(crate) fn parse(s: &str) -> Result<Self> {
        match s {
            "native" => Ok(Self::Native),
            "pi-sim" => Ok(Self::PiSim),
            other => bail!("unknown mode {other:?}; expected `native` or `pi-sim`"),
        }
    }
}

/// The Raspberry Pi 4 target, matching `scripts/common.sh`.
const PI_TARGET: &str = "aarch64-unknown-linux-gnu";

/// How often the transcript is drained onto the terminal.
const REFRESH: Duration = Duration::from_millis(200);

pub(crate) fn run(mode: Mode) -> Result<()> {
    let command = build(mode)?;

    let rig = Rig::start("emulate", &RigOptions::default())
        .context("assembling the emulated rig")
        .map_err(annotate)?;
    announce(&rig, &command, mode);

    let mut daemon = Daemon::start(&rig, &command).context("starting the daemon")?;
    println!("  daemon pid   {}", daemon.pid());
    println!("  daemon log   {}", daemon.log_path().display());
    println!("\nCtrl-C stops both. The daemon stops water first.\n");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building the runtime that handles Ctrl-C")?;

    let mut tail = Tail::default();
    let interrupted = runtime.block_on(async {
        loop {
            let stop = tokio::select! {
                signalled = tokio::signal::ctrl_c() => {
                    if let Err(e) = signalled {
                        eprintln!("xtask: cannot handle Ctrl-C ({e}); stopping anyway");
                    }
                    true
                }
                () = tokio::time::sleep(REFRESH) => false,
            };
            tail.drain(&rig);
            if stop {
                return true;
            }
            match daemon.exited() {
                Ok(Some(status)) => {
                    println!("\nthe daemon exited on its own: {status}");
                    return false;
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("xtask: cannot poll the daemon ({e})");
                    return false;
                }
            }
        }
    });

    if interrupted {
        // The terminal already delivered SIGINT to the daemon, which is the
        // path worth exercising. SIGTERM is the fallback for the case where it
        // did not — a daemon started outside this process group, or one already
        // wedged — and is harmless when the orderly stop is already under way.
        println!("\nstopping: waiting for the daemon to command every link off");
        if daemon.exited()?.is_none() {
            daemon.terminate().context("asking the daemon to stop")?;
        }
        match daemon.wait_for_exit(&rig, Duration::from_secs(20)) {
            Ok(status) => {
                tail.drain(&rig);
                report_exit(status);
            }
            Err(e) => eprintln!("xtask: {e}"),
        }
    }

    summarise(&rig);
    Ok(())
}

/// Make the one refusal a reader will actually hit say what to do about it.
fn annotate(e: anyhow::Error) -> anyhow::Error {
    e.context(format!(
        "if this is about {USB_SERIAL_DEVICES}, the daemon's port resolver enumerates it \
         before it looks at what any link is bound to; see kdtv_emulator::e2e::SysfsShim"
    ))
}

/// Build the daemon this mode runs, and say how to invoke it.
fn build(mode: Mode) -> Result<DaemonCommand> {
    let root = controller_dir();
    let mut cargo = Command::new(env!("CARGO"));
    cargo
        .args(["build", "--package", "kdtvd"])
        .current_dir(&root);
    if mode == Mode::PiSim {
        cargo.args(["--release", "--target", PI_TARGET]);
    }
    let binary = daemon_path(mode);

    println!("building the daemon ({mode:?})");
    let status = cargo.status().context("running cargo build")?;
    if !status.success() {
        match mode {
            Mode::PiSim => bail!(
                "the ARM64 build failed. It needs the target and a linker:\n  \
                 rustup target add {PI_TARGET}\n  apt-get install gcc-aarch64-linux-gnu"
            ),
            Mode::Native => bail!("the daemon did not build"),
        }
    }
    if !binary.is_file() {
        bail!("no binary at {}", binary.display());
    }

    let runner = match mode {
        Mode::Native => Vec::new(),
        Mode::PiSim => {
            let sysroot = std::env::var("QEMU_SYSROOT")
                .unwrap_or_else(|_| "/usr/aarch64-linux-gnu".to_owned());
            let qemu = std::env::var("QEMU").unwrap_or_else(|_| "qemu-aarch64-static".to_owned());
            vec![qemu, "-L".to_owned(), sysroot]
        }
    };
    Ok(DaemonCommand::new(binary, runner))
}

/// Everything a person needs in order to talk to what is now running.
fn announce(rig: &Rig, command: &DaemonCommand, mode: Mode) {
    println!("\nthe emulated rig ({mode:?})\n");
    for (link, path) in rig.port_paths() {
        println!("  {:<12} {}", device_label(link), path.display());
    }
    println!();
    println!("  api          http://{}", rig.api_addr());
    println!("  token        {}", rig.token_path().display());
    println!("  config       {}", rig.config_path().display());
    println!("  probes       {}", rig.probe_dir().display());
    println!("  binary       {command}");
    if *rig.sysfs_shim() != SysfsShim::NotNeeded {
        println!(
            "  sysfs        synthesised: {USB_SERIAL_DEVICES} is absent and the daemon's \
             port resolver enumerates it unconditionally"
        );
    }
    println!(
        "\n  the transmit gate is closed: every fixture is tier [C], so only these \
         pseudo-terminals can be opened."
    );
    println!(
        "  write a Celsius reading into <probes>/zone1.degc to drive the independent \
         temperature interlock.\n"
    );
}

fn device_label(link: LinkKind) -> String {
    match link {
        LinkKind::Zone(z) => format!("{z} valve"),
        LinkKind::Steam => "steam".to_owned(),
    }
}

fn report_exit(status: std::process::ExitStatus) {
    match status.code() {
        Some(0) => println!("the daemon exited 0: every link confirmed off"),
        Some(5) => println!(
            "the daemon exited 5: it could NOT confirm every link off. On real hardware \
             that is the outcome whose remedy is a person removing valve power."
        ),
        Some(other) => println!("the daemon exited {other}"),
        None => println!("the daemon was killed by a signal: {status}"),
    }
}

/// What crossed each link, printed as it happens.
#[derive(Default, Debug)]
struct Tail {
    seen: std::collections::BTreeMap<LinkKind, usize>,
}

impl Tail {
    /// Print everything that has crossed any link since the last call, in the
    /// order it crossed.
    ///
    /// Merged and sorted rather than printed link by link: the three links run
    /// concurrently, and a listing that walked them in turn would put a zone-2
    /// frame after a steam frame that happened later, which is exactly the
    /// question a person reads this output to answer.
    fn drain(&mut self, rig: &Rig) {
        let mut batch: Vec<(Duration, LinkKind, &'static str, String)> = Vec::new();
        for link in all_links() {
            let t = rig.transcript(link);
            let from = self.seen.get(&link).copied().unwrap_or(0);
            let entries = t.entries();
            for entry in entries.iter().skip(from) {
                let arrow = match entry.direction {
                    Direction::DaemonToDevice => "-->",
                    Direction::DeviceToDaemon => "<--",
                };
                batch.push((entry.at, link, arrow, entry.hex()));
            }
            self.seen.insert(link, entries.len());
        }
        batch.sort_by_key(|(at, link, _, _)| (*at, *link));
        for (at, link, arrow, hex) in batch {
            println!("{:>9.3}s {link:<6} {arrow} {hex}", at.as_secs_f64());
        }
    }
}

/// One line per link at the end, so a run that scrolled past is still readable.
fn summarise(rig: &Rig) {
    println!();
    for link in all_links() {
        let t = rig.transcript(link);
        let sent = t.transmitted().count();
        let total = t.entries().len();
        println!(
            "  {link:<6} {sent} frames transmitted, {} received, {} bytes out",
            total.saturating_sub(sent),
            t.transmitted_bytes()
        );
    }
    println!("  work        {}", rig.root().display());
}

/// The binary each mode runs. Named in one place, because `build` has to put
/// `cargo` and this in step and the pi-sim path is easy to get subtly wrong.
#[must_use]
fn daemon_path(mode: Mode) -> PathBuf {
    match mode {
        Mode::Native => controller_dir().join("target/debug/kdtvd"),
        Mode::PiSim => controller_dir().join(format!("target/{PI_TARGET}/release/kdtvd")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_modes_are_the_two_scripts_emulate_sh_offers() {
        assert!(matches!(Mode::parse("native"), Ok(Mode::Native)));
        assert!(matches!(Mode::parse("pi-sim"), Ok(Mode::PiSim)));
        // `--docker` is scripts/e2e.sh's, and it re-enters that script inside
        // the harness container rather than being a mode of this command.
        assert!(Mode::parse("docker").is_err());
    }

    #[test]
    fn each_mode_names_its_own_binary() {
        assert_ne!(daemon_path(Mode::Native), daemon_path(Mode::PiSim));
        assert!(
            daemon_path(Mode::PiSim)
                .to_string_lossy()
                .contains(PI_TARGET)
        );
    }
}
