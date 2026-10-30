#!/usr/bin/env bash
# lm-sysone-batched: start the batched System One server.
#
# The same door as the d1-2B's, on port 8104: POST /v1/systemone with the
# same body and the same answer. It serves the same weights, out of the same
# llama.cpp build, and it gives the same answers -- the difference is that the
# questions of a pack are evaluated at the same time instead of one after
# another.
#
# The model is LFM3-D1: a non-causal architecture with short convolutions
# and no KV cache, so a pack cannot be read in one forward pass (the tools
# README has the proof). What it can do is run the pack's questions in
# parallel, each on its own CUDA stream. Eight workers is what the fork's
# eight slots do, so the two servers can be measured against each other: with
# four workers the live lane's move pack was worse than the fork's, with
# eight it is level with it.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"

BIN="${SYSONE_BATCHED_BIN:-$HERE/sysone-batched}"
MODEL="${SYSONE_BATCHED_MODEL:-/home/caseydigennaro/models/d1-2B/d1-2B-Q8_0.gguf}"
PORT="${SYSONE_BATCHED_PORT:-8104}"
WORKERS="${SYSONE_BATCHED_WORKERS:-8}"
CTX="${SYSONE_BATCHED_CTX:-8192}"
UBATCH="${SYSONE_BATCHED_UBATCH:-2048}"
LOG="${SYSONE_BATCHED_LOG:-$HOME/lm-services/evidence/logs/lm-sysone-batched.log}"

if [ ! -f "$MODEL" ]; then
  printf 'lm-sysone-batched: the weights are not there: %s\n' "$MODEL"
  exit 1
fi
if [ ! -x "$BIN" ]; then
  printf 'lm-sysone-batched: the binary is not there: %s\n' "$BIN"
  printf 'lm-sysone-batched: build it with tools/sysone-batched/install.sh\n'
  exit 1
fi

mkdir -p "$(dirname "$LOG")"

PID=""
cleanup() {
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    kill "$PID" 2>/dev/null
    wait "$PID" 2>/dev/null
  fi
  PID=""
}
trap cleanup EXIT
trap 'cleanup; exit 0' INT TERM

up() {
  curl -fsS --noproxy '*' --max-time 2 "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1
}

start() {
  printf 'lm-sysone-batched: starting (%s workers, port %s)\n' "$WORKERS" "$PORT"
  "$BIN" --model "$MODEL" --host 0.0.0.0 --port "$PORT" \
    --workers "$WORKERS" --ctx "$CTX" --ubatch "$UBATCH" \
    --alias d1-2B >> "$LOG" 2>&1 &
  PID=$!
}

start
while true; do
  wait "$PID" 2>/dev/null
  if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
    continue
  fi
  printf 'lm-sysone-batched: the server stopped; starting it again\n'
  start
done
