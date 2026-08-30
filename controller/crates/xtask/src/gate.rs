//! Assert the transmit gate is still closed in the committed tree.
//!
//! The daemon's own gate refuses to open a real serial port unless every
//! allowlisted operation resolves to a fixture measured on this hardware. This
//! check is the same claim made against the committed data, independently of the
//! code that reads it, so promoting a fixture has to be argued for in a pull
//! request rather than merged quietly.
//!
//! It parses. An earlier version of this check was a `grep` for
//! `provenance = "captured"` — TOML syntax, against fixture files that are
//! JSON — so it matched nothing and would have reported success no matter what
//! the fixtures said. A check that cannot fail is indistinguishable from a check
//! that passes, which is worse than having no check at all.

use crate::{heading, report, workspace_root};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct Fixture {
    id: String,
    provenance: Provenance,
}

/// Only which variant it is matters here; the payloads are the daemon's
/// business. `serde` still requires the block to be well formed, so a fixture
/// whose provenance is neither of these fails to parse rather than being
/// skipped — an unparsed fixture must never read as a passing check.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Provenance {
    /// Tier `[A]`: measured on this installation's hardware during a capture.
    Captured(IgnoredPayload),
    /// Tier `[C]`: third-party reverse engineering, unverified here.
    Documented(IgnoredPayload),
}

/// Accepts and discards whatever the provenance block carries.
#[derive(Deserialize)]
#[serde(transparent)]
struct IgnoredPayload(serde::de::IgnoredAny);

pub(crate) fn run() -> Result<()> {
    let root = workspace_root()?;
    let dir = root.join("fixtures");
    let mut failures = Vec::new();
    let mut counted = 0usize;
    let mut files = 0usize;

    heading("fixture provenance");
    let entries = std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;
    let mut paths: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();

    anyhow::ensure!(
        !paths.is_empty(),
        "no fixture files found in {}",
        dir.display()
    );

    for path in &paths {
        files += 1;
        let (documented, captured) = scan(path)?;
        counted += documented + captured.len();
        println!(
            "  {}: {} documented `[C]`, {} captured `[A]`",
            path.file_name().unwrap_or_default().to_string_lossy(),
            documented,
            captured.len()
        );
        for id in captured {
            failures.push(format!(
                "{id} claims tier `[A]` provenance. Phase 1 capture has not happened; \
                 see controller/docs/DESIGN.md"
            ));
        }
    }

    println!("  {counted} fixtures across {files} files");
    anyhow::ensure!(
        counted > 0,
        "the fixture files parsed but contained no fixtures"
    );
    report(&failures, "transmit gate closed")
}

/// Returns the documented count and the ids of any captured fixtures.
fn scan(path: &Path) -> Result<(usize, Vec<String>)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let fixtures: Vec<Fixture> = serde_json::from_str(&text).with_context(|| {
        format!(
            "parsing {} — a fixture file that does not parse is not a passing check",
            path.display()
        )
    })?;
    let mut documented = 0;
    let mut captured = Vec::new();
    for f in fixtures {
        match f.provenance {
            Provenance::Documented(_) => documented += 1,
            Provenance::Captured(_) => captured.push(f.id),
        }
    }
    Ok((documented, captured))
}
