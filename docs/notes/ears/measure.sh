#!/usr/bin/env bash
# The measurement: twenty turns, from the key up to the line applied.
#
# The host plays each wave of the corpus as a push-to-talk turn: the
# wave is the capture, the ears are asked what was said, and the line is
# scored when the words come back. The host prints the cost of every
# turn and the p50 and the p95 at the end.
#
# The run is on the Windows side, because lm-host is Windows-only (the
# WSL toolchain has none of Bevy's Linux system libraries). The waves
# are in this folder and the mirror carries them.
#
# usage: bash docs/notes/ears/measure.sh
set -euo pipefail

cd "$(dirname "$0")/../../.."

CORPUS="docs/notes/ears"
WAVS=()
for wave in "$CORPUS"/line*.wav; do
  WAVS+=(--mic-wav "$wave")
done
if [ "${#WAVS[@]}" -eq 0 ]; then
  echo "the corpus is empty: $CORPUS/line*.wav" >&2
  exit 1
fi

printf 'the corpus: %d waves\n' "$((${#WAVS[@]} / 2))"

bash scripts/win-build.sh run -p lm-host --release -- \
  "${WAVS[@]}" \
  --possess 0 \
  --agents stub \
  --screenshot "C:/Users/casey/src/ears-measure.png" \
  --frames 60
