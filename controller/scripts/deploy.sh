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
#
#      The production configuration reads its API token from
#      /run/credentials/kdtvd.service/api-token, which systemd populates from
#      LoadCredential= and which exists only while the unit is running. A
#      --check-only run outside the unit cannot see it, so the check would have
#      failed on every correct configuration. The staged check configuration
#      therefore points token_file at the credential SOURCE
#      (/etc/kdtvd/api-token), which is the file that actually has to be there
#      and readable. The token itself is never copied anywhere.
#   4. Keeps the binary that is already installed, swaps in the new one, and
#      starts the service again.
#   5. If the new one does not come up, puts the kept binary back and starts it.
#
# Step 3 is the point of the whole script. A configuration that is wrong on the
# Pi is a service that refuses to start, and finding that out before the old
# binary is replaced is the difference between a failed deploy and no shower.
#
# Step 5 covers what step 3 cannot: a binary that validates the configuration and
# then fails for its own reasons. Without it a bad deploy leaves the Pi with no
# working daemon and the only way back is a laptop and a cross-compiler. The kept
# copy is the previously installed binary, whatever it was — not a rebuild of
# what this checkout thinks was there.
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
# The binary that was installed before this run, kept for step 5.
ROLLBACK_BIN="/usr/local/bin/kdtvd.previous"
INSTALL_UNIT="/etc/systemd/system/kdtvd.service"
INSTALL_CONFIG="/etc/kdtvd/kdtvd.toml"
# What LoadCredential= in deploy/kdtvd.service reads the API token from. The
# running service sees it under /run/credentials; this is where it lives on disk.
CREDENTIAL_SRC="/etc/kdtvd/api-token"

need ssh
need rsync

[ -f "$CONFIG_PATH" ] || die "no configuration at $CONFIG_PATH"
[ -f deploy/kdtvd.service ] || die "no systemd unit at deploy/kdtvd.service"

say "building for the Pi"
"$CONTROLLER_DIR/scripts/build.sh" --pi
BIN="target/$PI_TARGET/release/kdtvd"

say "staging on $TARGET_HOST at $STAGING"
# Readable by the service account, which is what runs the staged check below.
# Nothing secret is staged: the configuration names the token's path, never the
# token.
ssh "$TARGET_HOST" "mkdir -p '$STAGING' && chmod 0755 '$STAGING'"
rsync -q "$BIN" "$TARGET_HOST:$STAGING/kdtvd"
rsync -q deploy/kdtvd.service "$TARGET_HOST:$STAGING/kdtvd.service"
rsync -q "$CONFIG_PATH" "$TARGET_HOST:$STAGING/kdtvd.toml"
ssh "$TARGET_HOST" "chmod +x '$STAGING/kdtvd'"

# The configuration that gets checked, which differs from the one that gets
# installed in exactly one line — see step 3 in the header.
CHECK_CONFIG="$(mktemp)"
trap 'rm -f "$CHECK_CONFIG"' EXIT
sed 's|^token_file = "/run/credentials/[^"]*"|token_file = "'"$CREDENTIAL_SRC"'"|' \
  "$CONFIG_PATH" > "$CHECK_CONFIG"
if ! grep -q "^token_file = " "$CHECK_CONFIG"; then
  die "$CONFIG_PATH has no [api] token_file line; the staged check would not \
have covered the credential. Refusing rather than deploying an unchecked token \
path."
fi
rsync -q "$CHECK_CONFIG" "$TARGET_HOST:$STAGING/kdtvd.check.toml"

# Both checks below run as the account the unit runs as, where it exists. A
# token root can read and `kdtvd` cannot is a service that starts and refuses
# every request, and checking as root would report that as fine.
SERVICE_USER="kdtvd"
if ssh "$TARGET_HOST" "id -u '$SERVICE_USER'" >/dev/null 2>&1; then
  RUN_AS="sudo -u $SERVICE_USER"
else
  warn "no '$SERVICE_USER' account on $TARGET_HOST yet; checking as root."
  warn "the unit runs as User=$SERVICE_USER, so create it before the first start."
  RUN_AS="sudo"
fi

say "checking the API credential is installed on the target"
if ! ssh "$TARGET_HOST" "$RUN_AS test -r '$CREDENTIAL_SRC'"; then
  ssh "$TARGET_HOST" "rm -rf '$STAGING'" || true
  die "$CREDENTIAL_SRC is missing or unreadable on $TARGET_HOST. systemd loads \
the API token from there; without it the service starts and refuses every \
request. Install it (mode 0400, owner root) and deploy again. Nothing was \
changed."
fi

say "validating the staged configuration on the target"
if ! ssh "$TARGET_HOST" "$RUN_AS '$STAGING/kdtvd' --check-only --config '$STAGING/kdtvd.check.toml'"; then
  ssh "$TARGET_HOST" "rm -rf '$STAGING'" || true
  die "the staged configuration did not validate on the target. Nothing was changed."
fi

if [ "$DRY_RUN" != "0" ]; then
  say "DRY_RUN set: staged and validated, installing nothing"
  say "staging left at $TARGET_HOST:$STAGING"
  exit 0
fi

say "installing"
# Keep whatever is installed before it is overwritten. `cp -a` preserves the
# mode; the `|| true` covers the first deploy, where there is nothing to keep.
ssh "$TARGET_HOST" "[ -f '$INSTALL_BIN' ] && sudo cp -a '$INSTALL_BIN' '$ROLLBACK_BIN' || true"
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
  exit 0
fi

warn "kdtvd did not come up. The log from the failed start:"
ssh "$TARGET_HOST" "journalctl -u kdtvd --no-pager --lines=50" || true

if ! ssh "$TARGET_HOST" "[ -f '$ROLLBACK_BIN' ]"; then
  die "there is no previously installed binary to fall back to (first deploy). \
Read the log above, and see the manual rollback procedure in \
docs/replacement-controller/CONTROLLER-DESIGN.md."
fi

say "putting the previously installed binary back"
ssh "$TARGET_HOST" "sudo systemctl stop kdtvd || true"
ssh "$TARGET_HOST" "sudo install -m 0755 '$ROLLBACK_BIN' '$INSTALL_BIN'"
ssh "$TARGET_HOST" "sudo systemctl start kdtvd || true"

# One attempt, not a loop. If the binary that was working before this deploy is
# not working now, the difference is not the binary and retrying will not find it.
if ssh "$TARGET_HOST" "systemctl is-active --quiet kdtvd"; then
  die "the new binary failed to start and the previous one has been restored. \
The service is running on the previously installed binary; nothing is left \
half-deployed. Read the failed start's log above."
fi

ssh "$TARGET_HOST" "journalctl -u kdtvd --no-pager --lines=50" || true
die "the new binary failed to start, and so did the previous one. The service is \
DOWN. Both logs are above — the second is from the binary that was running \
before this deploy, so the cause is unlikely to be the binary. See the manual \
rollback procedure in docs/replacement-controller/CONTROLLER-DESIGN.md."
