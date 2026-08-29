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
# Belt and braces around the gate's own unit tests: no fixture in the repository
# may claim to be captured from this hardware until Phase 1 has actually run.
if grep -rql 'provenance *= *"captured"' fixtures/ 2>/dev/null; then
  die "a fixture claims tier [A] provenance. Phase 1 capture has not happened; \
see docs/replacement-controller/CONTROLLER-DESIGN.md."
fi

say "all checks passed"
