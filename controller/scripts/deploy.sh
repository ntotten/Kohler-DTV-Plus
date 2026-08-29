#!/usr/bin/env bash
#
# Cross-build and install the daemon on the Raspberry Pi.
#
#   ./scripts/deploy.sh pi@shower-controller.local
#   DRY_RUN=1 ./scripts/deploy.sh pi@...     stage everything, change nothing
#
# What it does, in order:
#
#   1. Cross-builds a release binary for aarch64.
#   2. Copies the binary, the systemd unit and the configuration to a staging
#      directory on the target. Nothing is live yet.
#   3. Runs `kdtvd --check-only` against the staged configuration ON THE TARGET.
#      The daemon validates configuration, resolves and distinguishes the serial
#      ports, and checks the transmit gate — without opening a link or
#      transmitting a byte. A failure here stops the deployment.
#   4. Stops the service, swaps the binary, and starts it again.
#
# Step 3 is the point of the whole script. A configuration that is wrong on the
# Pi is a service that refuses to start, and finding that out before the old
# binary is replaced is the difference between a failed deploy and no shower.
#
# The service always starts into its OFF boot sequence. Restarting it never
# restores a previous water state, so a deployment cannot turn water on.
#
source "$(dirname "${BASH_SOURCE[0]}")/common.sh"
cd "$CONTROLLER_DIR"

TARGET_HOST="${1:-}"
[ -n "$TARGET_HOST" ] || die "usage: $0 <user@host> [--config <path>]"
shift || true

CONFIG_PATH="${CONFIG_PATH:-deploy/kdtvd.toml}"
while [ $# -gt 0 ]; do
  case "$1" in
    --config) CONFIG_PATH="$2"; shift 2 ;;
    *) die "unknown argument: $1" ;;
  esac
done

DRY_RUN="${DRY_RUN:-0}"
STAGING="/tmp/kdtvd-staging-$$"
INSTALL_BIN="/usr/local/bin/kdtvd"
INSTALL_UNIT="/etc/systemd/system/kdtvd.service"
INSTALL_CONFIG="/etc/kdtvd/kdtvd.toml"

need ssh
need rsync

[ -f "$CONFIG_PATH" ] || die "no configuration at $CONFIG_PATH"
[ -f deploy/kdtvd.service ] || die "no systemd unit at deploy/kdtvd.service"

say "building for the Pi"
"$CONTROLLER_DIR/scripts/build.sh" --pi
BIN="target/$PI_TARGET/release/kdtvd"

say "staging on $TARGET_HOST at $STAGING"
ssh "$TARGET_HOST" "mkdir -p '$STAGING'"
rsync -q "$BIN" "$TARGET_HOST:$STAGING/kdtvd"
rsync -q deploy/kdtvd.service "$TARGET_HOST:$STAGING/kdtvd.service"
rsync -q "$CONFIG_PATH" "$TARGET_HOST:$STAGING/kdtvd.toml"
ssh "$TARGET_HOST" "chmod +x '$STAGING/kdtvd'"

say "validating the staged configuration on the target"
if ! ssh "$TARGET_HOST" "'$STAGING/kdtvd' --check-only --config '$STAGING/kdtvd.toml'"; then
  ssh "$TARGET_HOST" "rm -rf '$STAGING'" || true
  die "the staged configuration did not validate on the target. Nothing was changed."
fi

if [ "$DRY_RUN" != "0" ]; then
  say "DRY_RUN set: staged and validated, installing nothing"
  say "staging left at $TARGET_HOST:$STAGING"
  exit 0
fi

say "installing"
ssh "$TARGET_HOST" "sudo systemctl stop kdtvd || true"
ssh "$TARGET_HOST" "sudo install -m 0755 '$STAGING/kdtvd' '$INSTALL_BIN'"
ssh "$TARGET_HOST" "sudo install -m 0644 '$STAGING/kdtvd.service' '$INSTALL_UNIT'"
ssh "$TARGET_HOST" "sudo mkdir -p '$(dirname "$INSTALL_CONFIG")' && sudo install -m 0640 '$STAGING/kdtvd.toml' '$INSTALL_CONFIG'"
ssh "$TARGET_HOST" "sudo systemctl daemon-reload && sudo systemctl enable --now kdtvd"
ssh "$TARGET_HOST" "rm -rf '$STAGING'"

say "waiting for the service to reach its OFF ready state"
if ssh "$TARGET_HOST" "systemctl is-active --quiet kdtvd"; then
  say "kdtvd is running"
  ssh "$TARGET_HOST" "systemctl --no-pager --lines=20 status kdtvd" || true
else
  ssh "$TARGET_HOST" "journalctl -u kdtvd --no-pager --lines=50" || true
  die "kdtvd did not come up. The previous binary has already been replaced; \
read the log above, and see the manual rollback procedure in \
docs/replacement-controller/CONTROLLER-DESIGN.md."
fi
