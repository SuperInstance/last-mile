#!/usr/bin/env bash
# A read-only look at what is running.
set -uo pipefail
echo "=== unit ==="
systemctl --user status lm-ears.service --no-pager 2>&1 | head -12
echo
echo "=== the processes of the unit ==="
systemctl --user show lm-ears.service -p MainPID -p ExecMainPID 2>/dev/null
pgrep -af "llama-server.*8103" | head -3
pgrep -af "uvicorn ears:app" | head -3
echo
echo "=== ports ==="
ss -tlnp 2>/dev/null | grep -E ':(8102|8103)' 
echo
echo "=== the ears log (last 15) ==="
tail -15 /home/caseydigennaro/lm-wt/ears/evidence/logs/lm-ears-llama.log 2>/dev/null
