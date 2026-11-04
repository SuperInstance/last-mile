#!/usr/bin/env bash
# Run the LFM3 per-family arm, riding out the model server's restarts.
#
# The upstream llama-server on :8103 is restarted underneath this work
# often enough that a plain run comes back with nothing but "connection
# refused" rows. This waits for the service to be healthy, runs the arm,
# and runs it again until every answer is in.
set -u
cd /home/caseydigennaro/lm-wt/marks
OUT=evidence/vox-e6/lfm3_families.csv
LOG=evidence/vox-e6/lfm3_families.log
WANT=1700

good() {
  python3 - "$OUT" <<'PY'
import csv, sys
try:
    rows = list(csv.DictReader(open(sys.argv[1], encoding="utf-8")))
except Exception:
    rows = []
print(sum(1 for r in rows if r.get("lfm3")))
PY
}

for attempt in $(seq 1 40); do
  n=$(good)
  if [ "$n" -ge "$WANT" ]; then
    echo "done: $n answers"
    break
  fi
  echo "--- attempt $attempt: $n good answers, waiting for the model server ---"
  for wait in $(seq 1 120); do
    code=$(curl -s -m 5 -o /dev/null -w "%{http_code}" http://localhost:8102/health || true)
    if [ "$code" = "200" ]; then
      echo "healthy after ${wait} checks"
      break
    fi
    sleep 10
  done
  echo "--- running the arm ---"
  python3 -u tools/vox-e4/e6_lfm3_families.py \
      --marks evidence/vox-e6/marks.csv \
      --clip-root ~/lm-evidence/vox-e4 \
      --out "$OUT" --workers 3 --resume >> "$LOG" 2>&1
  sleep 5
done
echo "final: $(good) good answers"
