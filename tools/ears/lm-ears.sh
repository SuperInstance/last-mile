#!/usr/bin/env bash
# lm-ears: start the ears.
#
# Two processes, one unit:
#
#   1. llama-server with LFM3-Audio-2B on 127.0.0.1:8103 -- the model;
#   2. the proxy (tools/ears/ears.py) on 0.0.0.0:8102 -- the door last-mile
#      knocks on, which shapes the answer.
#
# The model server is bound to localhost on purpose: :8102 is the door,
# and :8103 is not for the network.
#
# The GPU on this box is shared, and a full offload can fail to fit when
# the other tenants are busy. So the model is started on the GPU first and,
# if it cannot be loaded there, on the CPU instead. The second attempt is
# slower but it is still an answer, and "the box was busy" is not a
# reason for the ears to be deaf.
set -uo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

BIN="${EARS_BIN:-/home/caseydigennaro/llama.cpp-upstream/build-up/bin/llama-server}"
MODEL="${EARS_MODEL_DIR:-/home/caseydigennaro/models/LFM3-Audio-2B}/LFM3-Audio-2B-Q8_0.gguf"
MMPROJ="${EARS_MODEL_DIR:-/home/caseydigennaro/models/LFM3-Audio-2B}/mmproj-LFM3-Audio-2B-Q8_0.gguf"
PY="${EARS_PY:-/home/caseydigennaro/ml/.venv/bin/python}"
UPSTREAM_PORT="${EARS_UPSTREAM_PORT:-8103}"
DOOR_PORT="${EARS_DOOR_PORT:-8102}"
START_GRACE="${EARS_START_GRACE:-240}"

LLAMA_PID=""
cleanup() {
  if [ -n "$LLAMA_PID" ] && kill -0 "$LLAMA_PID" 2>/dev/null; then
    kill "$LLAMA_PID" 2>/dev/null
    wait "$LLAMA_PID" 2>/dev/null
  fi
}
trap cleanup EXIT INT TERM

up() {
  curl -fsS --noproxy '*' --max-time 2 "http://127.0.0.1:${UPSTREAM_PORT}/health" >/dev/null 2>&1
}

# start_model <label> <extra args...>
#
# Starts llama-server and waits for it to answer. Returns 0 if it came
# up, 1 if it died or never answered.
start_model() {
  local label="$1"; shift
  printf 'lm-ears: starting the model (%s)\n' "$label"
  "$BIN" -m "$MODEL" --mmproj "$MMPROJ" \
    --host 127.0.0.1 --port "$UPSTREAM_PORT" \
    --alias LFM3-Audio-2B --no-webui "$@" \
    > "$ROOT/evidence/logs/lm-ears-llama.log" 2>&1 &
  LLAMA_PID=$!
  local waited=0
  while [ "$waited" -lt "$START_GRACE" ]; do
    if ! kill -0 "$LLAMA_PID" 2>/dev/null; then
      printf 'lm-ears: the model server died on the %s attempt\n' "$label"
      return 1
    fi
    if up; then
      printf 'lm-ears: the model is up on 127.0.0.1:%s (%s)\n' "$UPSTREAM_PORT" "$label"
      return 0
    fi
    sleep 2
    waited=$((waited + 2))
  done
  printf 'lm-ears: the model server did not answer within %ss (%s)\n' "$START_GRACE" "$label"
  kill "$LLAMA_PID" 2>/dev/null
  wait "$LLAMA_PID" 2>/dev/null
  LLAMA_PID=""
  return 1
}

mkdir -p "$ROOT/evidence/logs"

# 1. the GPU, then the CPU.
start_model "gpu" -ngl 99 --flash-attn on --ctx-size 8192 -ub 2048 ||
  start_model "cpu" -ngl 0 --threads 8 --ctx-size 8192 -ub 2048 ||
  { printf 'lm-ears: the model could not be started at all\n'; exit 1; }

# 2. the door.
printf 'lm-ears: opening the door on 0.0.0.0:%s\n' "$DOOR_PORT"
export EARS_UPSTREAM="http://127.0.0.1:${UPSTREAM_PORT}"
export EARS_MODEL="LFM3-Audio-2B"
exec "$PY" -m uvicorn ears:app \
  --app-dir "$HERE" \
  --host 0.0.0.0 --port "$DOOR_PORT" \
  --log-level info
