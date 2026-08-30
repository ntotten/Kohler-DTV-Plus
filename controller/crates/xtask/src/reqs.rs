//! Requirement coverage, read from `requirements.toml`.
//!
//! Tests carry their requirement by naming convention, so coverage is a text
//! search rather than a registry someone has to maintain twice:
//!
//! ```text
//! #[test]
//! fn req_controller_design_safe_05_missing_response_latches_zone() { ... }
//! ```
//!
//! The name is `req_` plus the requirement's [`Requirement::slug`], optionally
//! followed by `_` and a description. The description is the point: a suite of
//! 670 tests named `req_controller_design_safe_05` and nothing else is a suite
//! nobody can read, and the sentence is what tells a reviewer whether the test
//! actually proves the requirement it claims. [`covered_by`] is therefore a
//! prefix match anchored on that underscore, and [`slug_ambiguities`] asserts
//! the register never makes such a prefix mean two things.
//!
//! A test that proves more than one requirement names them all, separated by
//! `_req_` — see [`claimed_slugs`]. Several tests proving one requirement need
//! nothing special: they share the prefix and differ after it.
//!
//! The register is keyed on `(document, id)` because the ids collide badly
//! across source documents — 88 of 348 distinct ids are reused, and `SAFE-01`
//! through `SAFE-04` appear five times each with unrelated meanings. A coverage
//! report keyed on id alone would credit a requirement with a test written for a
//! different rule, which is worse than reporting no coverage at all.

use crate::{heading, report, workspace_root};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Deserialize, Debug)]
struct Register {
    requirement: Vec<Requirement>,
}

#[derive(Deserialize, Debug)]
struct Requirement {
    id: String,
    document: String,
    statement: String,
    source: String,
    verification: String,
    hard: bool,
    #[serde(default)]
    disputed: Option<String>,
}

impl Requirement {
    /// Can a test in this repository prove it, or does it need hardware?
    fn software_verifiable(&self) -> bool {
        let v = self.verification.to_ascii_lowercase();
        !(v.contains("not-testable-in-software")
            || v.contains("manual commissioning")
            || v.contains("commissioning inspection")
            || v.contains("physical"))
    }

    /// The slug a test name would carry: document stem plus id, lowercased.
    fn slug(&self) -> String {
        let stem = Path::new(&self.document).file_stem().map_or_else(
            || self.document.clone(),
            |s| s.to_string_lossy().into_owned(),
        );
        format!("{}_{}", sanitise(&stem), sanitise(&self.id))
    }
}

/// The slugs one test name claims: the first, and any introduced by `_req_`.
///
/// Requirements and tests are not in bijection, and the naming convention has to
/// survive both directions. Many tests to one requirement already works — they
/// simply share a prefix and differ in their descriptions. One test to many
/// requirements is this: `fn req_<slugA>_req_<slugB>_<description>`.
///
/// The alternative was to split such a test in two. That is not a rename, it
/// changes what each test asserts, and this repository has already paid for it
/// once — see [`declaration`] for the test that was split to work around a
/// different limitation of this scan.
fn claimed_slugs(name: &str) -> impl Iterator<Item = &str> {
    std::iter::once(name).chain(name.match_indices("_req_").map(|(i, _)| &name[i + 5..]))
}

/// Does any test name carry this slug?
///
/// `req_<slug>` and `req_<slug>_<description>` both count; `req_<slug>xyz` does
/// not. Anchoring on the underscore is what keeps the match honest — a bare
/// `starts_with` would let a test written for `OUT-5` satisfy `OUT-50` too.
/// [`slug_ambiguities`] rules out the remaining case, where one register slug is
/// a legitimate prefix of another.
///
/// This was an equality test until 2026-08-30, which no test name could satisfy
/// while also carrying a description, so the convention was unadoptable as
/// written: the module doc above and the `req_{slug}...` in the failure message
/// both promised a suffix the matcher rejected.
fn covered_by(names: &[String], slug: &str) -> bool {
    names.iter().flat_map(|n| claimed_slugs(n)).any(|claim| {
        claim
            .strip_prefix(slug)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('_'))
    })
}

/// Register slugs where one is a prefix of another at an underscore boundary.
///
/// [`covered_by`] would credit both from a single test name, which is the same
/// class of error as keying the register on id alone — a requirement marked
/// proven by a test written for a different rule. There are none today; this
/// exists so that adding `OUT-5-B` beside `OUT-5` breaks the build instead of
/// quietly inflating the coverage number.
fn slug_ambiguities(register: &[Requirement]) -> Vec<String> {
    let slugs: BTreeSet<String> = register.iter().map(Requirement::slug).collect();
    let mut out = Vec::new();
    for a in &slugs {
        for b in &slugs {
            if b.strip_prefix(a.as_str())
                .is_some_and(|r| r.starts_with('_'))
            {
                out.push(format!(
                    "slug `{a}` is a prefix of `{b}`: one test name would cover both"
                ));
            }
        }
    }
    out
}

fn sanitise(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn run(strict: bool, checklist: Option<&Path>) -> Result<()> {
    let root = workspace_root()?;
    let path = root.join("requirements.toml");
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let register: Register = toml::from_str(&text).context("parsing requirements.toml")?;

    // The key is (document, id). A collision here is a bug in the register.
    let mut by_key: BTreeMap<(&str, &str), &Requirement> = BTreeMap::new();
    let mut collisions = Vec::new();
    for r in &register.requirement {
        if by_key.insert((&r.document, &r.id), r).is_some() {
            collisions.push(format!("{} in {} appears twice", r.id, r.document));
        }
    }

    let sources = collect_test_names(&root);

    heading("register");
    println!("  {} requirements", register.requirement.len());
    println!(
        "  {} hard",
        register.requirement.iter().filter(|r| r.hard).count()
    );
    println!(
        "  {} disputed — a resolution the source documents do not have",
        register
            .requirement
            .iter()
            .filter(|r| r.disputed.is_some())
            .count()
    );

    // How bad would keying on id alone have been? Worth printing, because the
    // answer is the reason this tool keys on the pair.
    let mut ids: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &register.requirement {
        *ids.entry(r.id.as_str()).or_default() += 1;
    }
    let reused = ids.values().filter(|c| **c > 1).count();
    println!(
        "  {} of {} distinct ids are used by more than one document",
        reused,
        ids.len()
    );

    heading("coverage");
    let software: Vec<&Requirement> = register
        .requirement
        .iter()
        .filter(|r| r.software_verifiable())
        .collect();
    let covered: Vec<&Requirement> = software
        .iter()
        .copied()
        .filter(|r| covered_by(&sources, &r.slug()))
        .collect();
    println!(
        "  {}/{} software-verifiable requirements have a test naming them",
        covered.len(),
        software.len()
    );
    if covered.is_empty() && !software.is_empty() {
        // Without this line the run ends "requirement coverage: ok", which reads
        // as "the requirements are covered" rather than "nothing was checked".
        // Reaching zero again would mean the convention had been abandoned or
        // the scan had stopped seeing it, and either way the non-strict run must
        // not read as a pass.
        println!(
            "  NOTE: no test in the workspace uses the `req_*` naming convention,\n\
             \x20       so nothing below is a statement about requirement coverage.\n\
             \x20       `cargo xtask reqs --strict` is the check that would fail."
        );
    }

    let manual: Vec<&Requirement> = register
        .requirement
        .iter()
        .filter(|r| !r.software_verifiable())
        .collect();
    println!(
        "  {} require hardware — the commissioning checklist",
        manual.len()
    );

    if let Some(dest) = checklist {
        let path = if dest.is_absolute() {
            dest.to_path_buf()
        } else {
            root.join(dest)
        };
        write_checklist(&path, &manual)?;
        println!("  wrote {}", path.display());
    }

    let mut failures = collisions;
    failures.extend(slug_ambiguities(&register.requirement));
    if strict {
        for r in software
            .iter()
            .filter(|r| r.hard && !covered_by(&sources, &r.slug()))
        {
            failures.push(format!(
                "{} ({}) has no test named `req_{}...`: {}",
                r.id,
                r.document,
                r.slug(),
                r.statement.chars().take(90).collect::<String>()
            ));
        }
    }
    report(&failures, "requirement coverage")
}

/// Every `req_*` function name in every Rust source file under the workspace.
///
/// Unreadable files and directories are skipped rather than reported: a missing
/// test surfaces as a missing requirement, which is the finding that matters.
fn collect_test_names(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let mut stack = vec![root.join("crates")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let Ok(text) = std::fs::read_to_string(&p) else {
                    continue;
                };
                for line in text.lines() {
                    let t = declaration(line.trim_start());
                    if let Some(rest) = t.strip_prefix("fn req_") {
                        let name: String = rest
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                            .collect();
                        names.push(name);
                    }
                }
            }
        }
    }
    names
}

/// Strip the qualifiers that may precede `fn` in a test declaration.
///
/// `async` is the one that mattered. Much of the API and service suite is
/// `#[tokio::test]`, and while this scan matched only a bare `fn`, an
/// `async fn req_...` was invisible: five tests in `kdtv-api` carried a slug and
/// were still reported uncovered. It also cost the suite a real test —
/// `req_controller_design_api_01` was split off from
/// `seven_of_the_eight_api_01_operations_are_performed` for no reason but to put
/// the slug on something this function could see. A traceability tool that makes
/// the tests worse is not earning its place.
fn declaration(mut t: &str) -> &str {
    loop {
        let stripped = ["pub(crate) ", "pub ", "async ", "unsafe ", "const "]
            .iter()
            .find_map(|q| t.strip_prefix(q));
        match stripped {
            Some(rest) => t = rest.trim_start(),
            None => return t,
        }
    }
}

/// Write the commissioning checklist: every requirement that no test in this
/// repository can prove.
///
/// This is the document Phase 3 and Phase 6 need anyway. Generating it from the
/// register rather than writing it by hand means the two cannot drift, and it
/// makes the absence of a test *visible* rather than silent — a requirement that
/// needs a person with a thermometer is accounted for, not forgotten.
fn write_checklist(path: &Path, manual: &[&Requirement]) -> Result<()> {
    use std::fmt::Write as _;

    let mut by_doc: BTreeMap<String, Vec<&Requirement>> = BTreeMap::new();
    for r in manual {
        by_doc
            .entry(primary_document(&r.document))
            .or_default()
            .push(r);
    }

    let mut out = String::new();
    out.push_str("# Commissioning checklist\n\n");
    out.push_str("Generated from `controller/requirements.toml` by\n");
    out.push_str("`cargo xtask reqs --checklist commissioning/CHECKLIST.md`.\n");
    out.push_str("Do not edit by hand — edit the register and regenerate.\n\n");
    out.push_str("Every requirement below is one **no test in this repository can prove**.\n");
    out.push_str("They need hardware, a person, or both. Listing them here is what keeps\n");
    out.push_str("their absence from the test suite accounted for rather than silent.\n\n");
    out.push_str("Each line is a checkbox because that is how it gets used: printed, walked\n");
    out.push_str("through, and signed. **Record the measured value** wherever a threshold is\n");
    out.push_str("named — a tick against \"stops within the threshold\" is worth much less than\n");
    out.push_str("the number that was actually observed.\n\n");
    let _ = writeln!(out, "**{} items.**\n", manual.len());

    for (doc, items) in &by_doc {
        let _ = writeln!(out, "## {doc}\n");
        for r in items {
            let _ = writeln!(out, "- [ ] **{}** — {}", r.id, r.statement);
            let _ = writeln!(out, "  - _Verify:_ {}", r.verification);
            let _ = writeln!(out, "  - _Source:_ {}", r.source);
            if let Some(d) = &r.disputed {
                let _ = writeln!(out, "  - **Disputed:** {d}");
            }
        }
        out.push('\n');
    }

    // The loop separates groups with a blank line, which leaves one at the end
    // of the file. `oxfmt` formats Markdown, including this file, and strips it.
    // Without this trim the two tools disagree by exactly one line forever:
    // `cargo xtask reqs --checklist` then `npm run format` is a diff, and a
    // freshly generated checklist fails `npm run format:check`. Generating a
    // file the formatter would rewrite means it can never be both current and
    // formatted, so the generator emits the formatted form.
    while out.ends_with("\n\n") {
        out.pop();
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    std::fs::write(path, out).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// The first document path a source cites.
///
/// Several register entries cite more than one document in a single string, and
/// using the whole string as a grouping heading produces headings nobody can
/// read. The first path is the one the requirement is really from.
fn primary_document(document: &str) -> String {
    document
        .split([';', ','])
        .next()
        .unwrap_or(document)
        .split_whitespace()
        .find(|w| w.contains(".md"))
        .unwrap_or(document)
        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/' && c != '.' && c != '-')
        .to_owned()
}
