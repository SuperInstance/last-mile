#!/usr/bin/env bash
# The realizer A/B: the same harness, four arms, one after another.
#
# The arms are the same measurement with a different door in front of the
# wording: the Thor's LFM2-1.2B on :8092, and three models on the ProArt's
# Ollama on :11434. The prompt, the round, the deadline and the checks are
# the harness's own -- `crates/lm-agents/tests/realizer_on_thor.rs` -- and
# the environment says which arm plays:
#
#   UTTER_LLM_MODEL  the model's name, as the server knows it
#   UTTER_LLM_PORT   the port it serves on
#   UTTER_LLM_BASE   the base URL of the server
#   UTTER_DUMP       where the report is written, as JSON
#
# Each arm is three rounds of three thousand ticks -- five minutes -- so the
# whole sweep is twenty minutes.
#
#   bash scripts/realizer-ab.sh [arm ...]
#
# With no arm given, every arm plays. The reports land in a scratch
# directory (WORK, or a fresh one), and the table is printed at the end.
set -uo pipefail
cd "$(dirname "$0")/.."

# The arms: name, base, port, model -- and, for a ladder arm, the
# fallback's base, port and model after them.
#
# The Thor's arm is the one the game plays tonight: LFM2-1.2B on :8092, with
# the decision tier (the d1-2B on :8094) sharing the GPU with it. The
# ProArt's arms are the small models Ollama already has. The ladder arm is
# the A/B's answer to the busy night: the Thor first, the ProArt second,
# each but the last for its share of the deadline.
ARMS=(
  "thor-lfm2|http://192.168.0.78|8092|LFM2-1.2B"
  "proart-gemma2|http://127.0.0.1|11434|gemma2:2b"
  "proart-llama32|http://127.0.0.1|11434|llama3.2:1b"
  "proart-qwen3|http://127.0.0.1|11434|qwen3:0.6b"
  "ladder-thor-gemma2|http://192.168.0.78|8092|LFM2-1.2B|http://127.0.0.1|11434|gemma2:2b"
)

# The arms asked for, by name. With no name, every arm plays.
wanted=("$@")
keep_arm() {
  [ "${#wanted[@]}" -eq 0 ] && return 0
  local name="$1" w
  for w in "${wanted[@]}"; do
    [ "$w" = "$name" ] && return 0
  done
  return 1
}

WORK="${WORK:-$(mktemp -d "${TMPDIR:-/tmp}/lm-realizer-ab.XXXXXX")}"
echo "==> the reports land in $WORK"

# The models Ollama has, by name -- the arms' local models, and any other
# the server holds. The card has 6 GB, and a model left resident by the
# arm before is memory the arm's own model spills against.
OLLAMA_MODELS=(gemma2:2b llama3.2:1b qwen3:0.6b qwen2.5:0.5b qwen2.5-coder:7b)

# Take a model out of memory. An empty generate with a keep_alive of zero
# is Ollama's own unload; a model that is not loaded unloads in a
# millisecond, and one that is not there at all answers an error this
# ignores.
unload() {
  curl -fsS --noproxy '*' -m 60 -o /dev/null \
    -X POST http://127.0.0.1:11434/api/generate \
    -d "{\"model\":\"$1\",\"keep_alive\":0}" ||
    true
}

# Unload every local model, so an arm is measured with its own model and
# nothing else on the card.
unload_all() {
  local m
  for m in "${OLLAMA_MODELS[@]}"; do
    unload "$m"
  done
}

# Load the model before the round plays, and say how long that took.
#
# A server that loads the weights on the first request would spend the
# first line's deadline on the loading, and the arm would measure the
# loading instead of the wording. The Thor's model is always resident;
# this makes the local arms the same.
warm_start() {
  local base="$1" port="$2" model="$3"
  local started ended
  started="$(date +%s%N)"
  if curl -fsS --noproxy '*' -m 300 -o /dev/null \
      -X POST "$base:$port/v1/chat/completions" \
      -H 'Content-Type: application/json' \
      -d "{\"model\":\"$model\",\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}],\"max_tokens\":1,\"keep_alive\":\"30m\"}"; then
    ended="$(date +%s%N)"
    printf '    the model is loaded (%s ms)\n' "$(( (ended - started) / 1000000 ))"
  else
    printf '    FAIL: the model did not answer the warm-up\n'
  fi
}

for arm in "${ARMS[@]}"; do
  IFS='|' read -r name base port model fbase fport fmodel <<<"$arm"
  keep_arm "$name" || continue

  # An arm that already reported is not played again: the sweep can be
  # resumed after it is interrupted.
  if [ -s "$WORK/$name.json" ]; then
    echo "SKIP: $name -- the report is already there"
    continue
  fi

  echo "==> $name: $model on $base:$port"

  # The local models go out of memory first: an arm is measured with its
  # own model on the card and not with the last arm's still resident.
  if [ "$base" = "http://127.0.0.1" ] || [ -n "$fmodel" ]; then
    unload_all
  fi

  # A ladder arm warms its second stage as well: the first wording that
  # falls through must not be the one that pays for the loading.
  if [ -n "$fmodel" ]; then
    warm_start "$fbase" "$fport" "$fmodel"
  fi

  # The model is loaded before the round plays. A server that loads the
  # weights on the first request would spend the first line's deadline on
  # the loading, and the arm would measure the loading instead of the
  # wording. The Thor's model is always resident; this makes the local
  # arms the same.
  warm_start "$base" "$port" "$model"

  UTTER_LLM_MODEL="$model" \
  UTTER_LLM_PORT="$port" \
  UTTER_LLM_BASE="$base" \
  UTTER_FALLBACK_MODEL="${fmodel:-}" \
  UTTER_FALLBACK_PORT="${fport:-}" \
  UTTER_FALLBACK_BASE="${fbase:-}" \
  UTTER_DUMP="$WORK/$name.json" \
    cargo test --release -p lm-agents --test realizer_on_thor -- \
      --ignored --nocapture >"$WORK/$name.log" 2>&1
  code=$?
  if [ ! -s "$WORK/$name.json" ]; then
    echo "FAIL: $name -- the arm did not report (exit $code); the log is $WORK/$name.log"
    tail -20 "$WORK/$name.log"
    continue
  fi
  echo "PASS: $name -- the report is $WORK/$name.json"
done

echo
echo "==> the table"
python3 - "$WORK" <<'PY'
import json, os, sys

work = sys.argv[1]
rows = []
for name in sorted(os.listdir(work)):
    if not name.endswith(".json"):
        continue
    with open(os.path.join(work, name)) as handle:
        d = json.load(handle)
    arm = d["arm"]
    lat = d["latency"]
    plan = d["on_plan"]
    rows.append((name[:-5], arm.get("ladder") or arm["model"], d["lines"],
                 d["llm"], d["phrasebook"], d["fallback_pct"], d["misses"],
                 lat["p50_ms"], lat["p95_ms"], lat["n"],
                 plan["checked"], plan["off_plan"], plan["wrong_target"],
                 plan["rate_pct"]))

head = ("arm", "the wording", "lines", "llm", "fallback%", "p50 ms", "p95 ms",
        "n", "misses", "on-plan%", "off-plan", "wrong target")
print("| " + " | ".join(head) + " |")
print("|" + "|".join("---" for _ in head) + "|")
for r in rows:
    print(f"| {r[0]} | {r[1]} | {r[2]} | {r[3]} | {r[5]:.1f} | {r[7]:.0f} | "
          f"{r[8]:.0f} | {r[9]} | {r[6]} | {r[13]:.1f} | {r[11]} | {r[12]} |")
print()
for r in rows:
    print(f"- {r[0]}: {r[1]}")
PY

# The sweep is over: the local models go out of memory, so the card is
# left as the sweep found it and the game (or the next measurement) has
# all of it.
unload_all
echo "==> the local models are unloaded"
