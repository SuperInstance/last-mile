#!/usr/bin/env bash
# Install and start the lm-ears unit.
#
# Kept as a script because the install is three commands that have to
# happen in order, and because the ops log append has to happen too.
#
# What it does:
#   1. stops the scratch model server on :8103, if this repo started one
#      (the unit starts its own; two on one port is a mess);
#   2. installs tools/ears/lm-ears.service into the systemd user unit dir;
#   3. reloads systemd, enables and starts the unit;
#   4. waits for the door on :8102 to answer.
#
# Undo: systemctl --user disable --now lm-ears;
#        rm ~/.config/systemd/user/lm-ears.service;
#        systemctl --user daemon-reload
set -uo pipefail

ROOT=/home/caseydigennaro/lm-wt/ears
UNIT_DIR="$HOME/.config/systemd/user"
UNIT="$UNIT_DIR/lm-ears.service"

# 1. the scratch server this repo may have left on :8103
for pid in $(pgrep -f "llama-server.*--port 8103" 2>/dev/null); do
  echo "stopping the scratch model server on :8103 (pid $pid)"
  kill "$pid" 2>/dev/null
done
sleep 2

# 2. the unit
install -m 644 "$ROOT/tools/ears/lm-ears.service" "$UNIT"
echo "installed $UNIT"

# 3. systemd
systemctl --user daemon-reload
systemctl --user enable lm-ears.service
systemctl --user restart lm-ears.service
echo "started lm-ears.service"

# 4. the door
for i in $(seq 1 90); do
  if curl -fsS --noproxy '*' --max-time 3 http://127.0.0.1:8102/health >/dev/null 2>&1; then
    echo "the door is open after ${i}s"
    curl -sS --noproxy '*' --max-time 5 http://127.0.0.1:8102/health
    echo
    exit 0
  fi
  sleep 2
done
echo "the door did not open in 180s"
systemctl --user status lm-ears.service --no-pager | tail -20
exit 1
