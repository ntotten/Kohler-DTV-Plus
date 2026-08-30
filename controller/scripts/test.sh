#!/usr/bin/env bash
#
# Everything that must pass before a commit. No hardware, no water.
#
#   ./scripts/test.sh          format, lint, unit and integration tests
#   ./scripts/test.sh --quick  tests only, skipping format and lint
#
# This is the same set the CI workflows run, in the same order, so a green run
# here means a green run there. Note *workflows*, plural: .github/workflows has
# controller.yml for the Rust checks and format.yml for oxfmt, which formats the
# Markdown, TOML, YAML, JSON and HTML in this repository. Leaving oxfmt out meant
# a Cargo.toml this gate called clean failed on the remote — cargo fmt does not
# format manifests and nothing local did either.
#
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
cd "$CONTROLLER_DIR"

QUICK=0
[ "${1:-}" = "--quick" ] && QUICK=1

if [ "$QUICK" -eq 0 ]; then
  say "cargo fmt --check"
  cargo fmt --all --check

  say "cargo clippy -D warnings"
  cargo clippy --workspace --all-targets --all-features -- -D warnings
fi

say "cargo test"
cargo test --workspace --all-features

say "documentation builds without broken links"
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --document-private-items

say "the transmit gate is closed in the committed tree"
# Phase 1 capture has not run. A fixture at tier [A] would let the gate open, so
# its appearance must break the build and be argued for in a pull request rather
# than merged quietly.
#
# This parses the fixtures. The first version of this check grepped for the TOML
# spelling `provenance = "captured"` against files that are JSON, so it matched
# nothing and reported success no matter what the fixtures said. It sat here
# green and useless until the same mistake was caught in the CI workflow.
cargo xtask gate-closed
cargo test --package kdtv-proto gate

say "the dependency graph still holds the three structural guarantees"
cargo xtask audit-graph

say "every requirement in the register is still accounted for"
cargo xtask reqs

say "oxfmt: Markdown, TOML, YAML, JSON, HTML"
# The root workspace, not this one: oxfmt runs over the whole repository. Skipped
# with a warning rather than a failure when its dependencies are not installed,
# because a Rust contributor who has never run `npm install` should still get the
# rest of this gate.
if [ -x "$CONTROLLER_DIR/../node_modules/.bin/oxfmt" ]; then
  (cd "$CONTROLLER_DIR/.." && npm run --silent format:check)
else
  warn "oxfmt is not installed; run 'npm install' at the repository root."
  warn "the format.yml CI job will still check it."
fi

say "all checks passed"
