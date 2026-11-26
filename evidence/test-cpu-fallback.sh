#!/usr/bin/env bash
# Does the CPU fallback work? The unit prefers the GPU, but the shared GPU
# is not always big enough, and the fallback has to be a real answer and
# not a promise. So: run the model on the CPU on a scratch port, ask it
# the same question the door asks, and time it.
set -uo pipefail
ROOT=/home/caseydigennaro/lm-wt/ears
BIN=/home/caseydigennaro/llama.cpp-upstream/build-up/bin/llama-server
M=/home/caseydigennaro/models/LFM3-Audio-2B
PORT=8104

"$BIN" -m "$M/LFM3-Audio-2B-Q8_0.gguf" --mmproj "$M/mmproj-LFM3-Audio-2B-Q8_0.gguf" \
  --host 127.0.0.1 --port $PORT -ngl 0 --threads 8 --ctx-size 8192 -ub 2048 \
  --alias LFM3-Audio-2B --no-webui > /tmp/lfm3-cpu.log 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null' EXIT

for i in $(seq 1 180); do
  curl -fsS --noproxy '*' --max-time 2 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1 && break
  sleep 1
done
if ! curl -fsS --noproxy '*' --max-time 2 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
  echo "the CPU model never came up"
  tail -5 /tmp/lfm3-cpu.log
  exit 1
fi
echo "the CPU model is up after ${i}s"

for n in 1 2 3; do
  started=$(date +%s%N)
  out=$(curl -sS --noproxy '*' --max-time 300 -X POST "http://127.0.0.1:$PORT/v1/audio/transcriptions" \
    -F file=@"$ROOT/evidence/wavs/00_Command-Order_mia.wav" -F language=en)
  ended=$(date +%s%N)
  echo "run $n: $(( (ended - started) / 1000000 )) ms -> $out"
done
