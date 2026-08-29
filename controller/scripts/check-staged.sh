#!/usr/bin/env bash
#
# Verify what is STAGED, not what is in the working tree.
#
# Run this between `git add` and `git commit`. The distinction matters whenever
# anything else is writing to the tree — another editor, a background job, a
# subagent — because `cargo fmt --check` looks at the working tree, and a file
# edited between the check and the `git add` is committed unverified. That has
# put an unformatted file on this branch twice.
#
# It extracts the staged content to a temporary tree and checks that, so the
# answer is about the commit that is actually about to be made.
#
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
cd "$CONTROLLER_DIR/.."

STAGED=$(git diff --cached --name-only --diff-filter=ACM -- '*.rs')
if [ -z "$STAGED" ]; then
  say "no staged Rust files"
  exit 0
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

fail=0
while IFS= read -r f; do
  [ -n "$f" ] || continue
  mkdir -p "$TMP/$(dirname "$f")"
  git show ":$f" > "$TMP/$f"
  if ! rustfmt --edition 2024 --check "$TMP/$f" >/dev/null 2>&1; then
    warn "staged content is unformatted: $f"
    fail=1
  fi
done <<< "$STAGED"

if [ "$fail" -ne 0 ]; then
  die "run 'cargo fmt --all', re-stage, and try again"
fi
say "staged Rust content is formatted ($(wc -l <<< "$STAGED") files)"
