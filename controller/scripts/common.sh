#!/usr/bin/env bash
# Shared settings for the controller scripts. Sourced, not executed.

set -euo pipefail

CONTROLLER_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CONTROLLER_DIR

# The Raspberry Pi 4 running 64-bit Raspberry Pi OS.
PI_TARGET="${PI_TARGET:-aarch64-unknown-linux-gnu}"
PI_LINKER="${PI_LINKER:-aarch64-linux-gnu-gcc}"
export PI_TARGET PI_LINKER

# Every shipping crate. The two that are not shipped — kdtv-emulator and xtask —
# are deliberately absent: nothing in this list may depend on either.
SHIPPING_CRATES=(
  kdtv-units
  kdtv-proto
  kdtv-config
  kdtv-telemetry
  kdtv-hal
  kdtv-safety
  kdtv-engine
  kdtv-service
  kdtv-api
  kdtvd
)
export SHIPPING_CRATES

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m warn\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31merror\033[0m %s\n' "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "$1 is required but not on PATH.${2:+ $2}"
}
