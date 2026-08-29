//! Repository automation for the controller workspace.
//!
//! Run as `cargo xtask <command>` — the alias is in `.cargo/config.toml`.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

mod audit;
mod gate;
mod reqs;

#[derive(Parser, Debug)]
#[command(
    name = "xtask",
    about = "Automation for the Kohler DTV+ replacement controller"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Assert the dependency edges that must not exist.
    ///
    /// Some architectural rules cannot be expressed in Rust's type system but
    /// can be expressed in the dependency graph. This checks them, and CI runs
    /// it on every change.
    AuditGraph,
    /// Assert no fixture claims to have been captured from this hardware.
    ///
    /// The transmit gate cannot open while every fixture is tier `[C]`. This is
    /// the same claim checked against the committed data rather than the code.
    GateClosed,
    /// Report requirement coverage from requirements.toml.
    Reqs {
        /// Fail if a hard, software-verifiable requirement has no covering test.
        #[arg(long)]
        strict: bool,
        /// Also write the commissioning checklist to this path.
        #[arg(long, value_name = "PATH")]
        checklist: Option<std::path::PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::AuditGraph => audit::run(),
        Command::GateClosed => gate::run(),
        Command::Reqs { strict, checklist } => reqs::run(strict, checklist.as_deref()),
    }
}

/// The workspace root, from this crate's manifest directory.
pub(crate) fn workspace_root() -> Result<std::path::PathBuf> {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(std::path::Path::parent)
        .map(std::path::Path::to_path_buf)
        .context("locating the workspace root from the xtask manifest directory")
}

/// Print a heading, so a CI log reads as a sequence of checks.
pub(crate) fn heading(s: &str) {
    println!("\n== {s}");
}

pub(crate) fn report(failures: &[String], what: &str) -> Result<()> {
    if failures.is_empty() {
        println!("\n{what}: ok");
        return Ok(());
    }
    for f in failures {
        println!("  FAIL {f}");
    }
    bail!("{what}: {} problem(s)", failures.len())
}
