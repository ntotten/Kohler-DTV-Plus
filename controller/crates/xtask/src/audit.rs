//! Dependency edges that must not exist.
//!
//! Three of this design's guarantees are properties of the dependency graph
//! rather than of any one type, so they are asserted here and in CI:
//!
//! 1. **The shipped binary cannot reach the emulator.** `kdtv-emulator` is the
//!    only crate that can build arbitrary or malformed frames. Keeping it out of
//!    `kdtvd`'s graph is what stops that capability existing in production at
//!    all — a feature flag would not, because a feature can be turned on.
//!
//! 2. **The API cannot name a frame type.** No handler may construct or accept a
//!    wire frame, so `kdtv-api` declares no dependency on `kdtv-proto`. It
//!    reaches it transitively through `kdtv-service`, which is unavoidable and
//!    harmless: Rust will not let a crate name a type from a transitive
//!    dependency it has not declared. The **direct** edge is precisely the
//!    capability being denied, so that is what this checks.
//!
//! 3. **No HTTP client is reachable from the daemon.** Automated polling of the
//!    K-99695 is what hung the controller and cost a month of investigation
//!    (INVESTIGATIONS.md I1). After cutover the K-99695 is powered down. A
//!    daemon that cannot link an HTTP client cannot regrow a poller.
//!
//! `hyper` is deliberately not banned: the local API is an HTTP *server* and
//! needs it. The ban is on client crates, which is the capability that matters.

use crate::{heading, report, workspace_root};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    resolve: Resolve,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    dependencies: Vec<String>,
}

/// Crates that must never be reachable from the daemon.
const BANNED_FROM_DAEMON: &[(&str, &str)] = &[
    (
        "kdtv-emulator",
        "the only crate that can build arbitrary or malformed frames",
    ),
    (
        "xtask",
        "repository automation has no business in a shipped binary",
    ),
    (
        "reqwest",
        "an HTTP client in the daemon is how a poller against the K-99695 regrows",
    ),
    ("ureq", "same reason as reqwest"),
    ("curl", "same reason as reqwest"),
    (
        "rppal",
        "nothing in this design drives a GPIO, a relay or anything in a mains path",
    ),
];

/// Direct edges banned between specific pairs.
///
/// Direct, not transitive: a crate cannot `use` a type from a dependency it has
/// not declared, so declaring the dependency is the capability. Asserting
/// unreachability instead would fail on paths that have to exist — the API
/// reaches the codec through the service, because the service speaks the
/// protocol.
const BANNED_DIRECT_EDGES: &[(&str, &str, &str)] = &[(
    "kdtv-api",
    "kdtv-proto",
    "no API handler may name a wire frame type; external callers get constrained \
     operations, never raw bytes",
)];

pub(crate) fn run() -> Result<()> {
    let root = workspace_root()?;
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--all-features"])
        .current_dir(&root)
        .output()
        .context("running cargo metadata")?;
    anyhow::ensure!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: Metadata = serde_json::from_slice(&out.stdout).context("parsing cargo metadata")?;

    let name_of: BTreeMap<&str, &str> = meta
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p.name.as_str()))
        .collect();
    let deps_of: BTreeMap<&str, Vec<&str>> = meta
        .resolve
        .nodes
        .iter()
        .map(|n| {
            (
                n.id.as_str(),
                n.dependencies.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let id_of = |want: &str| -> Option<&str> {
        name_of.iter().find(|(_, n)| **n == want).map(|(id, _)| *id)
    };

    let mut failures = Vec::new();

    heading("crates reachable from the daemon");
    let Some(daemon) = id_of("kdtvd") else {
        anyhow::bail!("kdtvd is not in the workspace metadata");
    };
    let reachable = reachable_from(daemon, &deps_of);
    let reachable_names: BTreeSet<&str> = reachable
        .iter()
        .filter_map(|id| name_of.get(id).copied())
        .collect();
    println!("  kdtvd reaches {} crates", reachable_names.len());

    for (banned, why) in BANNED_FROM_DAEMON {
        if reachable_names.contains(banned) {
            failures.push(format!("kdtvd reaches `{banned}` — {why}"));
        } else {
            println!("  ok  kdtvd does not reach `{banned}`");
        }
    }

    heading("banned direct edges");
    for (from, to, why) in BANNED_DIRECT_EDGES {
        match id_of(from) {
            None => println!("  skip {from} is not in the workspace"),
            Some(from_id) => {
                let direct: BTreeSet<&str> = deps_of
                    .get(from_id)
                    .into_iter()
                    .flatten()
                    .filter_map(|id| name_of.get(id).copied())
                    .collect();
                if direct.contains(to) {
                    failures.push(format!("`{from}` depends directly on `{to}` — {why}"));
                } else {
                    println!("  ok  `{from}` does not depend directly on `{to}`");
                }
            }
        }
    }

    report(&failures, "dependency graph audit")
}

/// Every package reachable from `start`, excluding `start` itself.
fn reachable_from<'a>(start: &'a str, deps: &BTreeMap<&'a str, Vec<&'a str>>) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        for d in deps.get(id).into_iter().flatten() {
            if seen.insert(*d) {
                stack.push(d);
            }
        }
    }
    seen
}
