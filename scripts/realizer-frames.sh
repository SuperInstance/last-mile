#!/usr/bin/env bash
# The frames: the game and the wording model on one GPU.
#
# The ProArt's RTX 4050 renders the game (Bevy, Windows) and -- when the
# wording model is a local one -- also writes the lines (Ollama, in WSL,
# which the Windows game reaches at its own localhost). Two rounds of the
# same shape measure what the second job costs the first:
#
#   without  the default configuration: the wording is the Thor's LFM2, and
#            the 4050 renders alone;
#   with     evidence/agents-utter-proart.toml: the wording is gemma2:2b on
#            the same card that is rendering.
#
# The round is the brief's: --agents stub --talk on, the same seed, the same
# number of ticks. The host times its own frames and says so every 600
# frames and once more when the window closes; the logs land in
# evidence/frames/, one per run.
#
# usage: bash scripts/realizer-frames.sh
set -uo pipefail
cd "$(dirname "$0")/.."

# The reports go here.
WORK="${WORK:-evidence/frames}"
# The round: the brief's.
TICKS="${TICKS:-3000}"
SEED="${SEED:-7}"
# The configuration with the wording on this machine.
CONFIG="${CONFIG:-evidence/agents-utter-proart.toml}"

mkdir -p "$WORK"

# One run of the game, and the log it leaves.
#
# The window opens on the Windows desktop; the run ends when the round is
# played out and the host says how the frames went.
run() {
  local name="$1"
  shift
  echo "==> $name"
  local started ended
  started="$(date +%s%N)"
  bash scripts/win-build.sh run -p lm-host --release -- \
    --agents stub --talk on --ticks "$TICKS" --seed "$SEED" "$@" \
    >"$WORK/$name.log" 2>&1
  ended="$(date +%s%N)"
  printf '    the run took %d s; the log is %s\n' \
    "$(( (ended - started) / 1000000000 ))" "$WORK/$name.log"
  grep -E "the frames (so far|in all)" "$WORK/$name.log" || true
}

# The run without the local wording model: the Thor writes the lines, the
# 4050 renders alone.
run "frames-without"

# The run with it: gemma2:2b writes the lines on the card that renders.
run "frames-with-gemma2" --agents-config "$CONFIG"

echo
echo "==> the table"
python3 - "$WORK" <<'PY'
import os, re, sys

work = sys.argv[1]
line = re.compile(r"the frames (so far|in all): (\d+) drawn, p50 ([\d.]+) ms, "
                  r"p95 ([\d.]+) ms, max ([\d.]+) ms")

for name in ("frames-without", "frames-with-gemma2"):
    path = os.path.join(work, name + ".log")
    if not os.path.exists(path):
        print(f"- {name}: no log")
        continue
    with open(path, errors="replace") as handle:
        rows = [m.group(1, 2, 3, 4, 5) for m in line.finditer(handle.read())]
    if not rows:
        print(f"- {name}: the host reported no frame times")
        continue
    print(f"- {name}:")
    for when, drawn, p50, p95, mx in rows:
        print(f"    {when:>6}: {drawn:>6} drawn, p50 {p50} ms, p95 {p95} ms, max {mx} ms")
PY
