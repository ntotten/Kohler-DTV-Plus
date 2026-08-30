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
# The temporary tree gets EVERY tracked `.rs` file at its index state, not just
# the staged ones. rustfmt follows `mod` declarations: handed a `lib.rs`, it
# reads the submodules too and fails outright if one is missing from the tree
# around it. The first version of this script materialised the staged files one
# at a time and checked each as it landed, so a `lib.rs` staged alongside two new
# submodules was checked before either existed and the unresolvable `mod` was
# reported as "unformatted". Populating the whole tree first removes both the
# ordering dependency and the unstaged-sibling case.
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

# Every tracked .rs file at its index state — the staged ones because they are
# what is being checked, the rest because a staged file may `mod` into them.
git ls-files -z -- '*.rs' | xargs -0 --no-run-if-empty \
  git checkout-index --prefix="$TMP/" --

fail=0
while IFS= read -r f; do
  [ -n "$f" ] || continue
  out=$(rustfmt --edition 2024 --check "$TMP/$f" 2>&1) && continue
  # rustfmt exits non-zero both for "this differs from what I would emit" and
  # for "I could not read this at all". They need different answers, so say
  # which one happened rather than blaming formatting for a parse error.
  if grep -q '^Error' <<< "$out"; then
    warn "rustfmt could not process staged content: $f"
    sed 's/^/       /' <<< "$out" >&2
  else
    warn "staged content is unformatted: $f"
  fi
  fail=1
done <<< "$STAGED"

if [ "$fail" -ne 0 ]; then
  die "fix the file(s) above, re-stage, and try again"
fi
say "staged Rust content is formatted ($(wc -l <<< "$STAGED") files)"
