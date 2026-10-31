#!/usr/bin/env bash
# The outside-in acceptance session: drive the real `lm` binary the way a
# person would, and say PASS or FAIL for every step.
#
# This is the check that the tool is usable from the outside: the right
# commands, the right output, the right exit codes. It builds the release
# binary and runs it; nothing here reaches inside the crates. A step that
# does not do what it says makes the whole script fail.
set -uo pipefail
cd "$(dirname "$0")/.."

LM="target/release/lm"

# A scratch directory of its own, so the session never writes in the repo.
WORK="$(mktemp -d "${TMPDIR:-/tmp}/lm-acceptance.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

fails=0

# expect <name> <exit code> <pattern the output must match> -- <command...>
#
# The command's stdout and stderr are read together, the exit code is the
# one the tool returned, and both have to be what the tool promised.
expect() {
  local name="$1" want_code="$2" want_text="$3"
  shift 3
  [ "${1:-}" = "--" ] && shift

  local out code
  out="$("$@" 2>&1)"
  code=$?

  if [ "$code" != "$want_code" ]; then
    printf 'FAIL: %s -- exit %s, wanted %s\n' "$name" "$code" "$want_code"
    printf '%s\n' "$out" | sed 's/^/      /'
    fails=$((fails + 1))
    return 1
  fi
  if ! printf '%s' "$out" | grep -qE -- "$want_text"; then
    printf 'FAIL: %s -- the output did not match: %s\n' "$name" "$want_text"
    printf '%s\n' "$out" | sed 's/^/      /'
    fails=$((fails + 1))
    return 1
  fi
  printf 'PASS: %s\n' "$name"
}

echo "==> building the release binary"
if cargo build --release -p lm-tools >/dev/null; then
  printf 'PASS: build the release binary\n'
else
  printf 'FAIL: build the release binary\n'
  exit 1
fi

echo "==> the acceptance session"

expect "run plays a round and writes its log" \
  0 "seed 1: 900 ticks, [0-9]+ hashes" \
  -- "$LM" run --seed 1 --ticks 900 --out "$WORK/a.lml"

if [ -s "$WORK/a.lml" ]; then
  printf 'PASS: the round log is on disk\n'
else
  printf 'FAIL: the round log is on disk\n'
  fails=$((fails + 1))
fi

expect "replay checks every hash the round recorded" \
  0 "^OK [0-9]+ hashes$" \
  -- "$LM" replay "$WORK/a.lml"

expect "run plays a second round, from another seed" \
  0 "seed 2: 900 ticks, [0-9]+ hashes" \
  -- "$LM" run --seed 2 --ticks 900 --out "$WORK/b.lml"

expect "diff of two rounds names the first tick that moves" \
  1 "DIFFER at tick [0-9]+" \
  -- "$LM" diff "$WORK/a.lml" "$WORK/b.lml"

expect "diff of a round with itself finds nothing" \
  0 "^IDENTICAL$" \
  -- "$LM" diff "$WORK/a.lml" "$WORK/a.lml"

expect "golden matches the pinned hashes" \
  0 "^OK 3/3$" \
  -- "$LM" golden

# A harness for the learning round: one agent's harness, with the shipped
# reflex in it.
mkdir -p "$WORK/harness/a0/reflexes"
cp assets/reflexes/default.rhai "$WORK/harness/a0/reflexes/default.rhai"

expect "learn runs a learning round and says what it decided" \
  0 "decision:       reject" \
  -- "$LM" learn --seed 1 --harness "$WORK/harness" --runs "$WORK/runs" --ticks 900

if [ -s "$WORK/runs/flow/r1.json" ]; then
  printf 'PASS: the flow record is on disk\n'
else
  printf 'FAIL: the flow record is on disk\n'
  fails=$((fails + 1))
fi

if [ -s "$WORK/runs/ledger.jsonl" ]; then
  printf 'PASS: the prediction ledger is on disk\n'
else
  printf 'FAIL: the prediction ledger is on disk\n'
  fails=$((fails + 1))
fi

# The Vibe Dictionary slice: a typed change, a prediction sealed before
# the test, a held-out sweep, a verdict. Two seeds can never clear D78's
# sign test (2/2 is p = 0.25), so the small verdict below is NOT ADOPTED
# by arithmetic, not by luck.
expect "a value outside the dictionary's bounds is refused, never clamped" \
  1 "refused: 99 is outside .* bounds 4..=64" \
  -- "$LM" vibe propose unit.vision 99 --predict "sight_ticks:up" --runs "$WORK/vibe-runs"

expect "vibe proposes, seals before the test, and says the verdict" \
  0 "verdict: +NOT ADOPTED" \
  -- "$LM" vibe propose unit.vision 32 --predict "sight_ticks:up" --say "let them see farther" \
     --seeds 2 --ticks 400 --runs "$WORK/vibe-runs"

if [ -s "$WORK/vibe-runs/vibe.jsonl" ]; then
  printf 'PASS: the vibe ledger is on disk\n'
else
  printf 'FAIL: the vibe ledger is on disk\n'
  fails=$((fails + 1))
fi

expect "the vibe ledger lists its records and the prediction-trust tally" \
  0 "prediction trust: 0/1 measures held" \
  -- "$LM" vibe ledger --runs "$WORK/vibe-runs"

if [ "$fails" -gt 0 ]; then
  printf '\n==> %d step(s) FAILED\n' "$fails"
  exit 1
fi

printf '\n==> every step passed\n'
