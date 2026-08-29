#!/usr/bin/env bash
#
# Run the whole system with no hardware.
#
#   ./scripts/emulate.sh              native binary against emulated devices
#   ./scripts/emulate.sh --pi-sim     the ARM64 binary, under qemu
#
# Starts the emulator, which creates three PTY pairs — one per link — and models
# a DTV 6-port valve, a Prompt 3-port valve and a K-1737-K1 steam adapter behind
# them. Then starts the daemon pointed at the PTY follower devices, exactly as it
# would be pointed at /dev/serial/by-id/... on the Pi.
#
# The daemon's transmit gate stays closed against real serial ports throughout:
# every frame in this repository is tier [C], derived from third-party reverse
# engineering and unverified against the hardware. Only the emulator backends
# open. That is not a mode of this script; it is a property of the build.
#
# Ctrl-C stops both.
#
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
cd "$CONTROLLER_DIR"

MODE=native
[ "${1:-}" = "--pi-sim" ] && MODE=pi-sim

say "starting the emulated rig (mode: $MODE)"
exec cargo run --package xtask -- emulate --mode "$MODE" "${@:2}"
