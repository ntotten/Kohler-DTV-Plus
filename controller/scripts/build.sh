#!/usr/bin/env bash
#
# Build the controller.
#
#   ./scripts/build.sh            debug build for this machine
#   ./scripts/build.sh --release  release build for this machine
#   ./scripts/build.sh --pi       release build for the Raspberry Pi (aarch64)
#
# The Pi build cross-compiles. It needs the target and a linker:
#
#   rustup target add aarch64-unknown-linux-gnu
#   apt-get install gcc-aarch64-linux-gnu      # Debian/Ubuntu
#
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
cd "$CONTROLLER_DIR"

PROFILE=dev
TARGET_ARGS=()
OUT_DIR=target/debug

for arg in "$@"; do
  case "$arg" in
    --release) PROFILE=release; OUT_DIR=target/release ;;
    --pi)
      PROFILE=release
      TARGET_ARGS=(--target "$PI_TARGET")
      OUT_DIR="target/$PI_TARGET/release"
      ;;
    -h|--help) sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown argument: $arg" ;;
  esac
done

if [ "${#TARGET_ARGS[@]}" -gt 0 ]; then
  need "$PI_LINKER" "Install gcc-aarch64-linux-gnu, or set PI_LINKER."
  rustup target list --installed | grep -qx "$PI_TARGET" \
    || die "the $PI_TARGET target is not installed. Run: rustup target add $PI_TARGET"
  say "cross-compiling for $PI_TARGET"
fi

RELEASE_FLAG=()
[ "$PROFILE" = release ] && RELEASE_FLAG=(--release)

say "building kdtvd"
cargo build "${RELEASE_FLAG[@]}" "${TARGET_ARGS[@]}" --package kdtvd

BIN="$OUT_DIR/kdtvd"
[ -f "$BIN" ] || die "expected a binary at $BIN"
say "built $BIN"
file "$BIN" 2>/dev/null || true
