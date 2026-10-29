#!/usr/bin/env bash
# The final check: everything the brief asked for, asked and checked.
set -uo pipefail
ROOT=/home/caseydigennaro/lm-wt/ears
cd "$ROOT"

echo "=== 1. the unit ==="
systemctl --user is-enabled lm-ears.service 2>&1
systemctl --user is-active lm-ears.service 2>&1
systemctl --user status lm-ears.service --no-pager 2>&1 | grep -E "Main PID|Active:|Memory:" | head -3

echo
echo "=== 2. the door on :8102 ==="
curl -sS --noproxy '*' --max-time 5 http://127.0.0.1:8102/health; echo
curl -sS --noproxy '*' --max-time 10 http://127.0.0.1:8102/v1/models | head -c 300; echo

echo
echo "=== 3. a transcription through the door ==="
curl -sS --noproxy '*' --max-time 60 -X POST http://127.0.0.1:8102/v1/audio/transcriptions \
  -F file=@evidence/wavs/00_Command-Order_mia.wav -F language=en | python3 -m json.tool | head -20

echo
echo "=== 4. the other services are untouched ==="
for p in 8000 8093 8094 8095 8096 8097 8098 8099 8100; do
  code=$(curl -sS --noproxy '*' --max-time 4 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$p/" 2>/dev/null || echo "down")
  printf '  :%s -> %s\n' "$p" "$code"
done

echo
echo "=== 5. the evidence ==="
ls -la evidence/*.md evidence/*.csv evidence/*.json 2>/dev/null | awk '{print $5, $9}'
echo "  wavs: $(ls evidence/wavs/*.wav 2>/dev/null | wc -l) clips, intent: $(ls evidence/intent/*.wav 2>/dev/null | wc -l) clips"
echo "  ops log: $(wc -c < /home/caseydigennaro/last-mile-ops.md) bytes"

echo
echo "=== 6. the doc ==="
ls -la crates/lm-voice-ears/EARS.md
echo
echo "=== 7. git (nothing committed) ==="
git status --short | head -20
