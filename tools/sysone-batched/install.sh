#!/usr/bin/env bash
# Build sysone-batched and install it where the service runs it from.
#
# The service does not run out of a worktree: the worktree is deleted after a
# merge, and a unit that points into one would stop serving with it. So the
# binary is built here and installed into ~/lm-services/sysone-batched/.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
DEST="${SYSONE_BATCHED_HOME:-$HOME/lm-services/sysone-batched}"
UNIT_DIR="${SYSONE_BATCHED_UNIT_DIR:-$HOME/.config/systemd/user}"

bash "$HERE/build.sh"

mkdir -p "$DEST"
install -m 0755 "$HERE/sysone-batched" "$DEST/sysone-batched"
install -m 0755 "$HERE/lm-sysone-batched.sh" "$DEST/lm-sysone-batched.sh"
echo "installed $DEST/sysone-batched"

# the unit, when it is asked for: the service does not run out of a worktree
if [ "${SYSONE_BATCHED_INSTALL_UNIT:-1}" = "1" ]; then
    mkdir -p "$UNIT_DIR"
    install -m 0644 "$HERE/lm-sysone-batched.service" "$UNIT_DIR/lm-sysone-batched.service"
    echo "installed $UNIT_DIR/lm-sysone-batched.service"
    echo "now: systemctl --user daemon-reload && systemctl --user enable --now lm-sysone-batched.service"
fi
