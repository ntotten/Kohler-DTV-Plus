#!/usr/bin/env bash
#
# The end-to-end suite: the real daemon binary, against emulated devices, over
# virtual serial ports.
#
#   ./scripts/e2e.sh                 on this machine
#   ./scripts/e2e.sh --docker        inside the harness container
#   ./scripts/e2e.sh --pi-sim        with the ARM64 binary under qemu
#
# Every assertion runs against the transcript — the bytes the daemon actually
# put on the wire — rather than against the daemon's own reported state. A
# service that believes it is off while transmitting an open frame passes a
# state assertion and fails this one.
#
# Retries are set to zero. A flaky end-to-end test here is a bug in the service
# or in the harness, not weather.
#
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
cd "$CONTROLLER_DIR"

if [ "${1:-}" = "--docker" ]; then
  shift
  need docker
  say "building the harness image"
  docker build -q -f docker/Dockerfile -t kdtv-harness "$CONTROLLER_DIR/.." >/dev/null
  say "running the end-to-end suite in the container"
  exec docker run --rm -t kdtv-harness ./controller/scripts/e2e.sh "$@"
fi

PI_SIM=0
[ "${1:-}" = "--pi-sim" ] && { PI_SIM=1; shift; }

if [ "$PI_SIM" -eq 1 ]; then
  need qemu-aarch64-static "Install qemu-user-static."
  say "building the ARM64 daemon for the emulated-Pi run"
  "$CONTROLLER_DIR/scripts/build.sh" --pi
  export KDTV_E2E_DAEMON="$CONTROLLER_DIR/target/$PI_TARGET/release/kdtvd"
  export KDTV_E2E_RUNNER="qemu-aarch64-static -L /usr/aarch64-linux-gnu"
else
  say "building the daemon the suite will drive"
  cargo build --package kdtvd
  export KDTV_E2E_DAEMON="$CONTROLLER_DIR/target/debug/kdtvd"
fi

say "running the end-to-end suite"
cargo test --package kdtv-emulator --test e2e -- --test-threads=1 --nocapture "$@"
