#!/usr/bin/env bash
#
# Run the real ARM64 daemon binary on this machine, under user-mode emulation.
#
#   ./scripts/pi-sim.sh --check-only --config deploy/kdtvd.toml
#   ./scripts/pi-sim.sh -- --check-only --config deploy/kdtvd.toml
#
# This is the closest thing to the Pi that does not involve a Pi: the binary is
# the same aarch64 ELF that gets deployed, executed by qemu-aarch64. It catches
# what a native x86_64 test cannot — pointer width, alignment, and any
# architecture-dependent behaviour in a dependency.
#
# It does not emulate the Pi's peripherals. There is no SPI, no I2C and no USB
# serial here; the daemon must be pointed at PTY links, which is what
# scripts/emulate.sh sets up.
#
#   apt-get install qemu-user-static      # Debian/Ubuntu
#
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
cd "$CONTROLLER_DIR"

QEMU="${QEMU:-qemu-aarch64-static}"
SYSROOT="${QEMU_SYSROOT:-/usr/aarch64-linux-gnu}"

need "$QEMU" "Install qemu-user-static."
[ -d "$SYSROOT" ] || die "no aarch64 sysroot at $SYSROOT. Install gcc-aarch64-linux-gnu, or set QEMU_SYSROOT."

BIN="target/$PI_TARGET/release/kdtvd"
if [ ! -f "$BIN" ]; then
  say "no ARM64 binary yet; building one"
  "$CONTROLLER_DIR/scripts/build.sh" --pi
fi

# A leading `--` is what a reader naturally types to separate this script's
# arguments from the daemon's, and it was in this script's own usage line. It
# has to be dropped rather than forwarded: clap reads `--` as end-of-options and
# then treats every following flag as a positional argument, so `kdtvd` — which
# takes none — rejects the first one. Found by running it.
if [ "${1:-}" = "--" ]; then
  shift
fi

say "running $BIN under $QEMU"
exec "$QEMU" -L "$SYSROOT" "$BIN" "$@"
