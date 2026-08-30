//! The replacement master daemon.
//!
//! # Two modes
//!
//! `--check-only` validates and exits. It is the mode that matters first:
//! `scripts/deploy.sh` runs it **on the Pi** against the staged configuration
//! before it replaces the live binary. A configuration that is wrong on the
//! target is a service that refuses to start, and finding that out while the
//! old binary is still in place is the difference between a failed deployment
//! and no shower.
//!
//! **It checks everything the run path checks before it opens anything**, and
//! that is the property, not a list of conveniences. ~~The credential was
//! checked only on the run path.~~ Superseded: `--check-only` reported "all
//! checks passed" for a credential that `ApiToken::load` refuses as too short —
//! so the deployment installed, the unit failed on every start,
//! `StartLimitBurst=5` gave up, and the previous working binary was already
//! gone. That is the exact outcome the mode exists to prevent.
//!
//! Without it, the same checks run and then the links are opened, the service
//! is started and the API is served until a signal arrives.
//!
//! # The order the run path does things in, and why
//!
//! 1. **Validate.** Exactly what `--check-only` does: the file, the hardware it
//!    names, the credential, and whether it may transmit. Nothing is opened
//!    until all four pass.
//! 2. **Identity and platform.** The durable id counter, the clock and the
//!    watchdog.
//! 3. **Open and assemble.** [`kdtv_service::Service::start`] opens each link
//!    through the transmit gate's second boundary and hands back a handle, a
//!    supervisor and a shutdown trigger.
//! 4. **Signals, before anything is served.** `SIGTERM` and `SIGINT` are wired
//!    to the trigger so neither can end the process without stopping water
//!    first.
//! 5. **Bind the API, then run the loop.** Binding first means a busy port
//!    fails *before* the service tells `systemd` it is ready — a `Type=notify`
//!    unit that reports ready and then cannot answer is worse than one that
//!    never reports at all. `READY=1` is sent by the supervisor's first pass
//!    (`kdtv_service` `announce_start`), by which time the socket is listening.
//! 6. **Exit on the outcome.** [`exit_code_for`] maps what the service managed
//!    to do about the water.
//!
//! # The runtime is `current_thread`
//!
//! `kdtv-service`'s author recommends it and this daemon takes the advice.
//! Nothing in the control loop is parallelisable: three links at a 525 ms and a
//! 500 ms cadence, one transaction each, is a few dozen microseconds of
//! decoding per second, and the whole of the state it touches is owned by one
//! supervisor precisely so that "which thread is the safety kernel on" is not a
//! question anyone has to answer. A loopback API with a handful of clients does
//! not change that arithmetic. The single thread also fits the unit's
//! `MemoryMax=256M` and `TasksMax=64` with room to spare.
//!
//! One cost is accepted and worth naming: `kdtv_hal::FileIdStore` `fsync`s a
//! block of command ids every [`kdtv_hal::RESERVATION_BLOCK`] commands, and on
//! a `current_thread` runtime that flush happens on the same thread as the
//! control loop. On an SD card that is single-digit milliseconds, against a
//! 525 ms tick and a 30 s `WatchdogSec`. If it ever is not, the fix is to move
//! the id store behind `spawn_blocking`, not to add worker threads.

// Tests legitimately panic on a broken invariant.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

use anyhow::{Context, Result};
use clap::Parser;
use kdtv_api::{Api, ApiState, ApiToken, CommandIds};
use kdtv_hal::PortBinding;
use kdtv_proto::gate::TransmitAuthority;
use kdtv_service::ShutdownOutcome;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

/// Exit codes, so a deployment script can tell the cases apart.
mod exit {
    /// Everything validated.
    pub(crate) const OK: u8 = 0;
    /// The configuration is wrong. Nothing was opened.
    pub(crate) const CONFIG: u8 = 2;
    /// The configuration is valid but the hardware it names is not there,
    /// or two links resolve to one device.
    pub(crate) const HARDWARE: u8 = 3;
    /// The configuration asks to transmit on a real bus and the evidence does
    /// not support it. Distinct from CONFIG because it is the one refusal that
    /// is about the state of the investigation rather than about the file.
    pub(crate) const GATE: u8 = 4;
    /// The service stopped without confirming that every link is off.
    ///
    /// **The worst outcome this system has**, and it gets its own code rather
    /// than sharing one, because a deployment script or a monitor must be able
    /// to tell "a valve did not answer a stop" from "the file is wrong". The
    /// ports were closed regardless, so what stops the water from here is the
    /// valve's own communication-loss shutdown.
    ///
    /// It is non-zero deliberately. Reporting a clean exit for an unconfirmed
    /// off would make `systemctl status` green for the one condition that needs
    /// a person: the design's emergency procedure is to remove valve power and
    /// close the hot and cold service shutoffs. The unit is
    /// `Restart=on-failure` with `StartLimitBurst=5` in 60 s, so a daemon that
    /// exits this way every time is restarted five times — each restart
    /// re-entering the OFF boot sequence, which is the only thing a restart can
    /// do — and is then left in a failed state where it is visible. That is the
    /// correct end state. A service that cannot confirm a valve closed is not
    /// one to keep restarting quietly.
    pub(crate) const UNCONFIRMED_OFF: u8 = 5;
    /// The service could not be brought up for a reason that is neither the
    /// file, nor the hardware it names, nor the transmit gate: the state
    /// directory, the credential, or the API socket.
    pub(crate) const RUNTIME: u8 = 6;
}

#[derive(Parser, Debug)]
#[command(
    name = "kdtvd",
    about = "Replacement master for the Kohler DTV+",
    long_about = None
)]
struct Cli {
    /// The configuration file.
    #[arg(long, value_name = "PATH", default_value = "/etc/kdtvd/kdtvd.toml")]
    config: PathBuf,

    /// Where the durable id counter lives.
    ///
    /// `systemd` sets `STATE_DIRECTORY` from the unit's `StateDirectory=kdtvd`,
    /// so this needs no value in production. It holds a counter and nothing
    /// else: no water state is persisted, which is what makes a restart — for
    /// any reason, a watchdog reset included — unable to resume a session.
    ///
    /// A `StateDirectory=` with several paths would arrive here colon-separated
    /// and would not be a directory. The unit names one.
    #[arg(
        long,
        value_name = "PATH",
        env = "STATE_DIRECTORY",
        default_value = "/var/lib/kdtvd"
    )]
    state_dir: PathBuf,

    /// Validate and exit without opening a link or transmitting anything.
    ///
    /// This is what a deployment runs on the target before it installs.
    #[arg(long)]
    check_only: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli.check_only {
        return report(check_only(&cli.config).map(|()| exit::OK));
    }
    report(run(&cli))
}

/// Print a failure and turn it into an exit code.
///
/// Everything a caller needs to tell the cases apart is in the code; the text
/// is for a person reading `systemctl status`. A failure that carries no
/// [`CheckFailure`] is a configuration failure by default, which is the
/// conservative reading: nothing was opened.
fn report(outcome: Result<u8>) -> ExitCode {
    match outcome {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("kdtvd: {e:#}");
            tracing::error!(error = %format!("{e:#}"), "kdtvd is exiting");
            ExitCode::from(
                e.downcast_ref::<CheckFailure>()
                    .map_or(exit::CONFIG, CheckFailure::code),
            )
        }
    }
}

/// What the service managed to do about the water, as an exit code.
const fn exit_code_for(outcome: &ShutdownOutcome) -> u8 {
    match outcome {
        ShutdownOutcome::ConfirmedOff => exit::OK,
        ShutdownOutcome::UnconfirmedOff { .. } => exit::UNCONFIRMED_OFF,
    }
}

/// The run path.
///
/// Builds the runtime — see the module docs for why it is `current_thread` —
/// and blocks on the whole of the service's life.
fn run(cli: &Cli) -> Result<u8> {
    init_logging();
    let runtime = build_runtime().map_err(|source| {
        anyhow::Error::new(CheckFailure::Runtime(format!(
            "the tokio runtime could not be built: {source}"
        )))
    })?;
    runtime.block_on(serve(cli))
}

/// The single-threaded runtime the whole service runs on.
fn build_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_name("kdtvd")
        .build()
}

/// Open the links, start the service, serve the API, and stop the water.
async fn serve(cli: &Cli) -> Result<u8> {
    use kdtv_hal::{FileIdStore, LinuxLinkFactory, RealSysfs, SystemdWatchdog, Watchdog as _};
    use kdtv_service::{Deps, Service, Started};

    // 1. Everything --check-only does. Nothing is opened until all four pass.
    let checked = validate(&cli.config, Reporting::Log)?;

    // 2. Identity and platform. The id counter is opened before any port,
    //    because every authorisation to open water is bound to a boot id and a
    //    counter that cannot be made durable is one a restart could reissue.
    let ids = Arc::new(DurableIds(FileIdStore::open(&cli.state_dir).map_err(
        |source| {
            CheckFailure::Runtime(format!(
                "the id counter in {} could not be opened: {source}",
                cli.state_dir.display()
            ))
        },
    )?));
    let watchdog = Arc::new(SystemdWatchdog::from_environment());
    if watchdog.interval().is_none() {
        // The supervisor records this too. Saying it here as well is what a
        // person running the binary by hand sees.
        tracing::warn!(
            "no systemd watchdog is configured for this process; a wedged control \
             loop will not restart the service"
        );
    }
    let deps = Deps {
        clock: Arc::clone(&checked.clock),
        watchdog,
    };

    // 3. Open and assemble.
    let mut factory = LinuxLinkFactory::new(RealSysfs::new());
    let Started {
        handle,
        supervisor,
        shutdown,
    } = Service::start(
        &checked.config,
        &checked.authority,
        &checked.bindings,
        &mut factory,
        &ids.0,
        deps,
    )
    .await
    .map_err(|e| start_failure(&e))?;

    // 4. Signals and the API socket.
    //
    //    **The trigger is held.** `install_signal_handlers` gets a clone and the
    //    original stays in this scope until the loop has finished: a review of
    //    `kdtv-service` found that dropping every trigger is indistinguishable
    //    from asking for a shutdown, and while the supervisor now says which of
    //    the two happened, the daemon must still not do it by accident. The
    //    explicit `drop` at the end of this function is what makes the lifetime
    //    visible rather than incidental.
    //
    //    **Every failure path from here ends OFF.** The links are open by now,
    //    so a credential that will not read or a socket already in use does not
    //    return straight out of this function — it asks the loop to stop, runs
    //    it, and returns only once the drain has finished. Dropping the
    //    supervisor instead would close the ports without ever commanding a
    //    stop, and "no water was on yet" is a fact about today's boot sequence
    //    rather than a property of this code.
    let (signals, api) = match bring_up(&checked, handle, &ids, &shutdown).await {
        Ok(pair) => pair,
        Err(why) => {
            shutdown.trigger("the service could not finish starting");
            let outcome = supervisor.run().await;
            drop(shutdown);
            return Err(why.context(format!(
                "the service stopped before it served anything ({})",
                match outcome {
                    ShutdownOutcome::ConfirmedOff => "every link confirmed off".to_owned(),
                    ShutdownOutcome::UnconfirmedOff { links } =>
                        format!("these links did NOT confirm off: {links:?}"),
                }
            )));
        }
    };
    let bound = api.local_addr().ok();
    tracing::info!(bind = ?bound, "the local API is listening");

    // 5. Serve, and run the loop.
    let (stop_api, api_stopped) = tokio::sync::oneshot::channel::<()>();
    let api_task = tokio::spawn(api.serve(async move {
        // A dropped sender ends the server too, so a panic in the loop cannot
        // leave the socket answering.
        let _ = api_stopped.await;
    }));

    // The supervisor sends `READY=1` on its first pass, by which point the
    // socket above is already listening.
    let outcome = supervisor.run().await;

    // 6. Stop answering, then report.
    //
    //    `Api::serve` bounds its own wait on open connections, so this cannot
    //    hang on a held-open event stream — which it did, until the whole
    //    exit-code contract below was being replaced by `SIGKILL` at
    //    `TimeoutStopSec=20s`. See `kdtv_api::SHUTDOWN_GRACE`.
    let _ = stop_api.send(());
    match api_task.await {
        Ok(Ok(())) => {}
        // Previously discarded, so a socket that failed left the journal saying
        // only that the command channel had closed.
        Ok(Err(source)) => {
            tracing::error!(error = %source, "the API server stopped with an error");
        }
        Err(joined) => tracing::warn!(error = %joined, "the API task did not end cleanly"),
    }
    signals.abort();

    match &outcome {
        ShutdownOutcome::ConfirmedOff => tracing::info!("every link confirmed off"),
        ShutdownOutcome::UnconfirmedOff { links } => tracing::error!(
            links = ?links,
            "the service could not confirm these links off; the ports were closed, so the \
             valve's own communication-loss shutdown is what stops the water. Remove valve \
             power and close the hot and cold service shutoffs if water is still moving."
        ),
    }

    // Held from the moment it existed until now, which is the point.
    drop(shutdown);
    Ok(exit_code_for(&outcome))
}

/// Wire up the signal handlers and the API socket.
///
/// Split out so that everything between "the links are open" and "the loop is
/// running" has one error path, which the caller answers by draining rather
/// than by returning.
async fn bring_up(
    checked: &Checked,
    handle: kdtv_service::ServiceHandle,
    ids: &Arc<DurableIds>,
    shutdown: &kdtv_service::ShutdownTrigger,
) -> Result<(tokio::task::JoinHandle<()>, Api)> {
    let signals = kdtv_service::install_signal_handlers(shutdown.clone()).map_err(|source| {
        CheckFailure::Runtime(format!("cannot install signal handlers: {source}"))
    })?;
    let token = ApiToken::load(checked.config.api().token_file())
        .await
        .map_err(|source| CheckFailure::Runtime(source.to_string()))?;
    let state = ApiState::new(
        handle,
        Arc::<DurableIds>::clone(ids),
        checked.config.bounds(),
        token,
        checked.config.api().session_ttl(),
    );
    let api = Api::bind(checked.config.api(), state)
        .await
        .map_err(|source| CheckFailure::Runtime(source.to_string()))?;
    Ok((signals, api))
}

/// The API's command ids, from the durable counter.
///
/// `kdtv-api` cannot reach `kdtv_hal::IdStore` — it does not depend on
/// `kdtv-hal`, and should not, to name one counter — so the daemon bridges the
/// two. Command ids are minted where they are already made durable rather than
/// counted in the API: the store persists and `fsync`s **before** it issues, so
/// a crash can only skip ids forward, never reuse one, and a reused id makes a
/// frame log unreadable exactly when it is needed.
#[derive(Debug)]
struct DurableIds(kdtv_hal::FileIdStore);

impl CommandIds for DurableIds {
    fn next(&self) -> Result<kdtv_units::CommandId, kdtv_api::IdUnavailable> {
        use kdtv_hal::IdStore as _;
        self.0
            .next_command()
            .map_err(|e| kdtv_api::IdUnavailable(e.to_string()))
    }
}

/// Map a start failure onto the code that says what to fix.
///
/// The transmit gate is the one refusal that is about the state of the
/// investigation rather than about a fault, and it keeps its own code so a
/// closed gate — today's expected outcome, with every fixture tier `[C]` —
/// never reads as broken hardware.
fn start_failure(e: &kdtv_service::StartError) -> anyhow::Error {
    use kdtv_service::StartError as E;
    match e {
        E::Open(open) if open.is_gate() => CheckFailure::Gate(e.to_string()).into(),
        E::Open(_) | E::Unbound(_) => CheckFailure::Hardware(e.to_string()).into(),
        E::Ids(_) => CheckFailure::Runtime(e.to_string()).into(),
    }
}

/// Structured logs to stderr, which `systemd` routes to the journal.
///
/// `RUST_LOG` selects the level, `info` by default. JSON so a frame record and
/// a command record come out as fields rather than as prose to grep.
///
/// **`logging.directory` and `logging.max_total_mb` are not honoured yet.** The
/// configuration carries them, the unit sets `LogsDirectory=kdtvd`, and nothing
/// here writes a file: the journal's own retention is what bounds the log
/// today. Recorded rather than silently dropped.
fn init_logging() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .finish();
    // A second initialisation is not a fault worth exiting for; it means
    // something already installed a subscriber, which in practice is a test.
    let _ = tracing::subscriber::set_global_default(subscriber);
}

/// A check that failed, carrying which kind it was.
#[derive(Debug, thiserror::Error)]
enum CheckFailure {
    #[error("configuration: {0}")]
    Config(String),
    #[error("hardware: {0}")]
    Hardware(String),
    #[error("transmit gate: {0}")]
    Gate(String),
    /// Neither the file, nor the hardware it names, nor the gate: the state
    /// directory, the credential, or the API socket.
    #[error("runtime: {0}")]
    Runtime(String),
}

impl CheckFailure {
    const fn code(&self) -> u8 {
        match self {
            Self::Config(_) => exit::CONFIG,
            Self::Hardware(_) => exit::HARDWARE,
            Self::Gate(_) => exit::GATE,
            Self::Runtime(_) => exit::RUNTIME,
        }
    }
}

/// Validate everything that can be validated without opening a link.
///
/// The order is deliberate: the file first, then the hardware it names, then
/// whether it may transmit. Each answers a different question, and a caller
/// reading the output top to bottom sees them in the order they would have to be
/// fixed.
fn check_only(path: &std::path::Path) -> Result<()> {
    println!("kdtvd --check-only");
    validate(path, Reporting::Print)?;
    println!("  all checks passed");
    Ok(())
}

/// Where the validation results go.
///
/// `--check-only` is read by a person and by `scripts/deploy.sh` on the target,
/// so it prints to stdout in the order the problems would have to be fixed. The
/// run path has a logging subscriber installed by then, and the same facts
/// belong in the journal with the rest of the boot record.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Reporting {
    Print,
    Log,
}

impl Reporting {
    fn say(self, line: &str) {
        match self {
            Self::Print => println!("  {line}"),
            Self::Log => tracing::info!("{line}"),
        }
    }
}

/// What validation produced, and what opening a link needs.
struct Checked {
    config: kdtv_config::ValidatedConfig,
    bindings: Vec<PortBinding>,
    authority: TransmitAuthority,
    /// Built during validation and carried forward, so the run path uses the
    /// clock the checks used. Two clocks would mean two monotonic origins, and
    /// a sample stamped by one compared against a deadline from the other.
    clock: Arc<dyn kdtv_hal::Clock>,
}

/// Validate everything that can be validated without opening a link.
///
/// Shared by both modes, so `--check-only` and the run path cannot answer
/// differently: what a deployment validated on the target is what the service
/// then starts against.
fn validate(path: &std::path::Path, how: Reporting) -> Result<Checked> {
    use kdtv_hal::{RealSysfs, bindings_of, resolve_distinct};

    let clock: Arc<dyn kdtv_hal::Clock> = Arc::new(kdtv_hal::LinuxClock::systemd());

    how.say(&format!("config: {}", path.display()));

    // 1. The file. Parses, and every rule in it holds.
    let fs = kdtv_config::fs::RealFs;
    let config = kdtv_config::ValidatedConfig::load(path, &fs)
        .map_err(|e| classify(&e))
        .context("the configuration did not validate")?;
    how.say(&format!("profile: {:?}", config.profile()));
    how.say("ok  configuration validates");

    // 2. The hardware it names. Present, and distinct — two by-id names can
    //    symlink one device, which is the confusion the by-id rule exists to
    //    prevent, so the comparison is on the resolved path.
    let bindings = match resolve_distinct(&bindings_of(&config), &RealSysfs::new()) {
        Ok(ports) => {
            for p in &ports {
                how.say(&format!(
                    "ok  {} -> {}",
                    p.link(),
                    p.port().device().display()
                ));
            }
            ports
        }
        Err(e) => {
            return Err(CheckFailure::Hardware(e.to_string()))
                .context("the configured links did not resolve");
        }
    };

    let authority = check_beyond_the_devices(&config, how)?;

    Ok(Checked {
        config,
        bindings,
        authority,
        clock,
    })
}

/// Steps 3 and 4: the credential, and the transmit gate.
///
/// Split from [`validate`] so a test can drive them against the shipped file
/// with the devices faked — on a machine with no converters that is the only
/// way to assert that the pre-flight refuses what the run path refuses.
///
/// The order is the order the problems would have to be fixed.
fn check_beyond_the_devices(
    config: &kdtv_config::ValidatedConfig,
    how: Reporting,
) -> Result<TransmitAuthority> {
    // 3. The credential. Read and dropped: nothing here retains it, and
    //    `ApiToken` zeroizes when it goes. This is the one startup failure a
    //    deployment cannot recover from remotely — by the time `bring_up`
    //    refuses a token that is too short, the old binary is gone.
    let token_file = config.api().token_file();
    check_credential_permissions(token_file)?;
    drop(
        ApiToken::load_blocking(token_file)
            .map_err(|source| CheckFailure::Runtime(source.to_string()))
            .context("the API credential is not usable")?,
    );
    how.say(&format!("ok  credential {}", token_file.display()));

    // 4. Whether it may transmit on a real bus. This is the one refusal that is
    //    about the state of the investigation rather than about the file: no
    //    frame in this workspace has been verified against the hardware, so
    //    until Phase 1 capture promotes the fixtures the gate stays shut.
    let fixtures = kdtv_proto::fixtures::FixtureSet::embedded();
    let authority = match TransmitAuthority::resolve(&gate_request(config.gate()), fixtures) {
        Ok(auth) => {
            if auth.permits_real_bus() {
                how.say("ok  transmit gate: real bus attested");
            } else {
                how.say("ok  transmit gate: emulator only — no real port will be opened");
            }
            auth
        }
        Err(e) => {
            return Err(CheckFailure::Gate(e.to_string())).context(
                "the configuration claims a real-bus attestation the fixtures do not support",
            );
        }
    };

    Ok(authority)
}

/// Refuse a credential any account but its owner can read.
///
/// `kdtv-config` refuses a **world**-readable token file and stops there, so
/// mode 0640 and 0660 both pass it. The unit runs with
/// `SupplementaryGroups=dialout spi i2c` and the default interactive user on a
/// Raspberry Pi image is in `dialout`, so "group" is not a small set — and
/// `OPS-04` is explicit that anything which can reach this API can run the
/// shower. `scripts/deploy.sh` already tells the operator to install it mode
/// 0400 owner root; this is what makes that true rather than advisory.
///
/// **The durable fix belongs in `kdtv_config::ApiConfig::build`**, which is the
/// only automated check on the credential's permissions and is where a second
/// caller would look for it. This is the composition root asserting it in the
/// meantime, and it is a second check on the same property rather than a
/// replacement for the first — the same argument [`kdtv_api::Api::bind`] makes
/// for re-checking the loopback bind.
///
/// One consequence worth naming: owner-only means the owner has to be an
/// account that can read it. `deploy/kdtvd.service` uses
/// `LoadCredential=api-token:/etc/kdtvd/api-token`, which systemd reads as root
/// — but `scripts/deploy.sh` runs this check as `User=kdtvd` and already
/// requires the source to be readable by that account, which its own
/// "mode 0400, owner root" message does not describe. `kdtvd:kdtvd 0400` is the
/// install that satisfies both; the message and the check in that script
/// disagree with each other, independently of this.
fn check_credential_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = std::fs::metadata(path)
        .map_err(|source| {
            CheckFailure::Runtime(format!(
                "cannot read the API credential at {}: {source}",
                path.display()
            ))
        })?
        .permissions()
        .mode()
        & 0o777;
    if mode & 0o077 != 0 {
        return Err(CheckFailure::Runtime(format!(
            "the API credential at {} is mode {mode:04o}; it must be readable by its owner \
             alone (0400 or 0600), because anything that can read it can run the shower",
            path.display()
        ))
        .into());
    }
    Ok(())
}

/// Which kind of failure a configuration error is.
///
/// `kdtv-config` validates the file *and* resolves the devices it names, because
/// it has to: two `by-id` names can symlink one `ttyUSB`, and comparing the
/// configured strings would miss that. So a missing converter arrives here as a
/// configuration error, and it is not one — the file is right and the hardware
/// is absent, which needs a different fix and gets a different exit code.
///
/// Two variants are about hardware. Everything else is about the file.
fn classify(e: &kdtv_config::ConfigError) -> CheckFailure {
    use kdtv_config::ConfigError as E;
    match e {
        E::PortAbsent { .. } | E::DuplicatePort { .. } => CheckFailure::Hardware(e.to_string()),
        _ => CheckFailure::Config(e.to_string()),
    }
}

/// Translate the configuration's gate section into the codec's request.
///
/// The two crates each define their own type on purpose: `kdtv-config` must not
/// depend on `kdtv-proto` to describe a file, and `kdtv-proto` must not depend
/// on `kdtv-config` to check evidence. Joining them is the composition root's
/// job, and this is the composition root.
///
/// **One field does not survive the crossing cleanly.** `kdtv-proto` wants the
/// attestation date as its own field, so a stale attestation is visible without
/// reading prose. The configuration schema keeps it inside the free-text note —
/// the shipped example writes "A+ = converter TA, measured 2026-XX-XX" — so the
/// whole string is carried as the note and the date field says where to look.
/// Worth tightening in the schema before Phase 1, when the first real
/// attestation is written; recorded here rather than silently dropped.
fn gate_request(cfg: &kdtv_config::TransmitGateConfig) -> kdtv_proto::gate::TransmitGateConfig {
    use kdtv_proto::gate::{PolarityAttestation, PolarityNote, RequestedScope};

    kdtv_proto::gate::TransmitGateConfig {
        scope: if cfg.scope().is_real_bus() {
            RequestedScope::RealBusAttested
        } else {
            RequestedScope::EmulatorOnly
        },
        capture_ref: cfg.capture_ref().map(ToOwned::to_owned),
        polarity: PolarityAttestation {
            notes: cfg
                .attested_links()
                .filter_map(|link| {
                    cfg.polarity(link).map(|note| PolarityNote {
                        link,
                        note: note.to_owned(),
                        attested_on: "see note".to_owned(),
                    })
                })
                .collect(),
        },
        expected_fixtures_sha256: cfg.fixtures_sha256().map(ToOwned::to_owned),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exit_codes_are_distinct() {
        // A deployment script branches on these, so two meaning the same thing
        // would make a gate refusal look like a typo in the file.
        let codes = [
            exit::OK,
            exit::CONFIG,
            exit::HARDWARE,
            exit::GATE,
            exit::UNCONFIRMED_OFF,
            exit::RUNTIME,
        ];
        let distinct: std::collections::BTreeSet<u8> = codes.iter().copied().collect();
        assert_eq!(distinct.len(), codes.len());
    }

    #[test]
    fn each_failure_kind_reports_its_own_code() {
        assert_eq!(CheckFailure::Config(String::new()).code(), exit::CONFIG);
        assert_eq!(CheckFailure::Hardware(String::new()).code(), exit::HARDWARE);
        assert_eq!(CheckFailure::Gate(String::new()).code(), exit::GATE);
        assert_eq!(CheckFailure::Runtime(String::new()).code(), exit::RUNTIME);
    }

    /// A stop that was never confirmed is not a clean exit.
    ///
    /// The alternative — exiting zero — would report the worst outcome this
    /// system has as success, and `systemctl status` would be green for the one
    /// condition whose remedy is a person removing valve power.
    #[test]
    fn an_unconfirmed_off_exits_non_zero_with_its_own_code() {
        assert_eq!(exit_code_for(&ShutdownOutcome::ConfirmedOff), exit::OK);
        let unconfirmed = ShutdownOutcome::UnconfirmedOff {
            links: vec![kdtv_units::LinkKind::Zone(kdtv_units::ZoneId::Zone1)],
        };
        assert_eq!(exit_code_for(&unconfirmed), exit::UNCONFIRMED_OFF);
        assert_ne!(exit_code_for(&unconfirmed), exit::OK);
        // And it is not confusable with a bad configuration file, which is the
        // distinction a deployment script needs.
        assert_ne!(exit_code_for(&unconfirmed), exit::CONFIG);
    }

    /// The runtime decision, asserted rather than described.
    #[test]
    fn the_runtime_is_single_threaded() {
        let runtime = build_runtime().expect("a runtime");
        let workers = runtime.block_on(async { tokio::runtime::Handle::current().metrics() });
        assert_eq!(
            workers.num_workers(),
            1,
            "nothing in the control loop is parallelisable; see the module docs"
        );
    }

    /// A closed transmit gate is the committed position of this project, not a
    /// fault, and it must not exit as broken hardware.
    #[test]
    fn a_closed_gate_and_a_fault_exit_differently() {
        use kdtv_hal::OpenError;
        use kdtv_service::StartError;
        use kdtv_units::{LinkKind, ZoneId};

        let gated = StartError::Open(OpenError::TransmitGateClosed {
            link: LinkKind::Zone(ZoneId::Zone1),
            scope: "emulator-only",
        });
        let code = |e: &StartError| {
            start_failure(e)
                .downcast_ref::<CheckFailure>()
                .map(CheckFailure::code)
        };
        assert_eq!(code(&gated), Some(exit::GATE));
        assert_eq!(
            code(&StartError::Unbound(LinkKind::Steam)),
            Some(exit::HARDWARE)
        );
    }

    /// The command ids the API mints come from the durable counter, and a crash
    /// can only skip them forward.
    #[test]
    fn the_command_id_bridge_issues_from_the_durable_counter() {
        use kdtv_api::CommandIds as _;
        let dir = tempfile::tempdir().expect("a temporary state directory");
        let ids = DurableIds(kdtv_hal::FileIdStore::open(dir.path()).expect("a counter"));
        let first = ids.next().expect("an id");
        let second = ids.next().expect("another id");
        assert!(second.0 > first.0, "{first:?} then {second:?}");

        // A restart re-opens the same directory and never reissues.
        let reopened = DurableIds(kdtv_hal::FileIdStore::open(dir.path()).expect("a counter"));
        let after = reopened.next().expect("an id");
        assert!(after.0 > second.0, "{second:?} then {after:?}");
    }

    /// The daemon mints no authorisation to open water.
    ///
    /// `kdtv-api` counts the calls to `StartAuthorization::issue` in its own six
    /// source files and asserts there is one. That is the whole of the argument
    /// that an open valve is reachable only through an authenticated request —
    /// and it cannot see this crate, which links `kdtv-service` directly and can
    /// therefore write
    /// `kdtv_service::surface::StartAuthorization::issue(handle.boot(), id)` and
    /// hand it to `ServiceHandle::start` with no token, no session and no
    /// `Caller` anywhere in the path. A local console command, a scheduled
    /// preheat or a recovery routine after an `UnconfirmedOff` restart is
    /// exactly the shape that would do it.
    ///
    /// **A workspace-wide count belongs in `cargo xtask`**, beside
    /// `audit-graph`, with an allowlist of the one production site and the test
    /// fixtures. It does not exist. This closes the one other crate that can
    /// reach the mint today; it does not close the class.
    ///
    /// The needle is assembled at runtime so this file does not match itself.
    #[test]
    fn the_daemon_never_mints_an_authorisation_to_open_water() {
        let needle = concat!("StartAuthorization", "::issue(");
        let sites: Vec<&str> = include_str!("main.rs")
            .lines()
            .map(str::trim)
            .filter(|line| line.contains(needle) && !line.starts_with("///"))
            .collect();
        assert!(
            sites.is_empty(),
            "the daemon mints an authorisation to open water: {sites:#?}"
        );
    }

    /// A credential of its own, since the pre-flight now reads one.
    fn credential(dir: &std::path::Path, bytes: &[u8], mode: u32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let path = dir.join("api-token");
        std::fs::write(&path, bytes).expect("the credential is written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("the credential's mode is set");
        path
    }

    /// A credential that will not load fails the pre-flight, not the first
    /// start after a deployment.
    #[test]
    fn the_pre_flight_refuses_a_credential_that_is_too_short_to_be_one() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let token = credential(dir.path(), b"hunter2\n", 0o400);
        let Some(config) = production_config_with(&token) else {
            return;
        };
        let err = check_beyond_the_devices(&config, Reporting::Log)
            .expect_err("seven bytes is not a credential for something that runs a shower");
        assert_eq!(
            err.downcast_ref::<CheckFailure>().map(CheckFailure::code),
            Some(exit::RUNTIME),
            "{err:#}"
        );
        // And the refusal does not quote it.
        assert!(!format!("{err:#}").contains("hunter2"), "{err:#}");
    }

    /// A credential the group can read fails the pre-flight.
    ///
    /// `kdtv-config` refuses only a world-readable one, so 0640 passes it. The
    /// unit's `SupplementaryGroups=dialout spi i2c` and a stock Pi image's login
    /// user in `dialout` make that a real set of accounts, every one of which
    /// could then reach 127.0.0.1:8443 and start the shower.
    #[test]
    fn the_pre_flight_refuses_a_credential_the_group_can_read() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for mode in [0o640, 0o660, 0o604] {
            let token = credential(dir.path(), b"0123456789abcdef0123456789abcdef", mode);
            let Some(config) = production_config_with(&token) else {
                return;
            };
            let err = check_beyond_the_devices(&config, Reporting::Log)
                .expect_err("a credential the group can read must be refused");
            assert_eq!(
                err.downcast_ref::<CheckFailure>().map(CheckFailure::code),
                Some(exit::RUNTIME),
                "mode {mode:04o}: {err:#}"
            );
        }
    }

    /// The path the shipped file names, which `systemd` supplies as a
    /// credential and which does not exist on a development machine.
    const SHIPPED_TOKEN_FILE: &str = "/run/credentials/kdtvd.service/api-token";

    /// The same, with the credential redirected to a file a test can create.
    ///
    /// `kdtv-config` checks the token file's mode through the injected
    /// filesystem, which is faked here; the pre-flight's own permission check
    /// reads the real one, which is why the path has to be real for anything
    /// past step 2.
    fn production_config_with(token: &std::path::Path) -> Option<kdtv_config::ValidatedConfig> {
        use kdtv_config::{FsEntry, MapFs, ValidatedConfig};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)?
            .join("deploy/kdtvd.toml");
        let token = token.to_str()?;
        let text = std::fs::read_to_string(&path)
            .ok()?
            .replace(SHIPPED_TOKEN_FILE, token);
        let fs = MapFs::new()
            .with(
                "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if00-port0",
                FsEntry::link("/dev/ttyUSB0"),
            )
            .with(
                "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if01-port0",
                FsEntry::link("/dev/ttyUSB1"),
            )
            .with(
                "/dev/serial/by-id/usb-Waveshare_USB_TO_RS485-if02-port0",
                FsEntry::link("/dev/ttyUSB2"),
            )
            .with(token, FsEntry::own(token).with_mode(0o400));
        ValidatedConfig::from_str_with(&text, &path, &fs).ok()
    }

    #[test]
    fn a_missing_configuration_file_is_a_configuration_failure() {
        let e = check_only(std::path::Path::new("/nonexistent/kdtvd.toml"))
            .expect_err("a missing file must fail");
        assert_eq!(
            e.downcast_ref::<CheckFailure>().map(CheckFailure::code),
            Some(exit::CONFIG)
        );
    }

    #[test]
    fn the_committed_production_example_fails_on_hardware_not_on_syntax() {
        // The example names USB converters this machine does not have. It must
        // get past parsing and validation and fail at resolution — which is the
        // evidence that the file itself is right and only the hardware is
        // absent. If this ever reports a configuration failure, the shipped
        // example has drifted from the parser.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("workspace root")
            .join("deploy/kdtvd.toml");
        if !path.exists() {
            return;
        }
        match check_only(&path) {
            Ok(()) => { /* a machine that really has the converters */ }
            Err(e) => {
                let code = e.downcast_ref::<CheckFailure>().map(CheckFailure::code);
                assert_ne!(
                    code,
                    Some(exit::CONFIG),
                    "the shipped example must not fail validation: {e:#}"
                );
            }
        }
    }
}
