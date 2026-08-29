#!/usr/bin/env bash
#
# Everything that must pass before a commit. No hardware, no water.
#
#   ./scripts/test.sh          format, lint, unit and integration tests
#   ./scripts/test.sh --quick  tests only, skipping format and lint
#
# This is the same set the CI workflow runs, in the same order, so a green run
# here means a green run there.
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

say "all checks passed"
