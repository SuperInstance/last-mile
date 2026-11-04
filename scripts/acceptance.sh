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

# The landing: an adopted value joins the harness the next round is played
# with, and the undo puts it back. The adopted proposal below is the full
# D78 count (16 seeds x 1500 ticks); it adopts by arithmetic — every seed's
# hold ticks rise when the Dabs are faster — so the verdict is ADOPTED
# every time it runs, by determinism, not by luck.
expect "a value outside a new entry's bounds is refused too" \
  1 "refused: 2049 is outside .* bounds 0..=2048" \
  -- "$LM" vibe propose unit.speed 2049 --predict "tags:up" --runs "$WORK/vibe-runs"

expect "the vibe state says nothing is in force yet" \
  0 "nothing in force" \
  -- "$LM" vibe state --runs "$WORK/vibe-runs"

expect "an adopted change lands: the state file, the commit-shaped header, the verdict" \
  0 "verdict: +ADOPTED" \
  -- "$LM" vibe propose unit.speed 272 --predict "hold_ticks:up" --say "make them faster" \
     --runs "$WORK/vibe-runs" --harness "$WORK/harness" --next-seed 41

if [ -s "$WORK/harness/vibe-state.toml" ]; then
  printf 'PASS: the harness carries the Vibe state\n'
else
  printf 'FAIL: the harness carries the Vibe state\n'
  fails=$((fails + 1))
fi

if grep -q '"speed_upt": 272' "$WORK/vibe-runs/vibe/next-header.json" 2>/dev/null \
  && grep -q '"block": "unit.speed"' "$WORK/vibe-runs/vibe/next-header.json"; then
  printf 'PASS: the next round%s header carries the value and names the record\n' "'"
else
  printf 'FAIL: the next round%s header carries the value and names the record\n' "'"
  fails=$((fails + 1))
fi

expect "the vibe state names the record in force" \
  0 "unit.speed *= *272 .*ledger line" \
  -- "$LM" vibe state --runs "$WORK/vibe-runs"

expect "the next round plays from the landed header" \
  0 "seed 41: 900 ticks" \
  -- "$LM" run --header "$WORK/vibe-runs/vibe/next-header.json" --ticks 900 --out "$WORK/vibe-next.lml"

expect "the landed round replays" \
  0 "^OK [0-9]+ hashes$" \
  -- "$LM" replay "$WORK/vibe-next.lml"

expect "the undo of a record that was never adopted is a named refusal" \
  1 "seal 0 was not adopted" \
  -- "$LM" vibe undo 0 --runs "$WORK/vibe-runs"

# The record in force, read from the state rather than assumed: its seal
# is the ledger line the state names.
ADOPTED_SEAL="$("$LM" vibe state --runs "$WORK/vibe-runs" \
  | sed -n 's/.*ledger line \([0-9]*\)).*/\1/p' | head -n 1)"

expect "the undo puts the adopted record back" \
  0 "back to 136" \
  -- "$LM" vibe undo "$ADOPTED_SEAL" --runs "$WORK/vibe-runs" --harness "$WORK/harness" --next-seed 41

expect "the state is back to the shipped defaults" \
  0 "nothing in force" \
  -- "$LM" vibe state --runs "$WORK/vibe-runs"

if grep -q '"speed_upt": 136' "$WORK/vibe-runs/vibe/next-header.json" 2>/dev/null \
  && grep -q '"vibe": \[\]' "$WORK/vibe-runs/vibe/next-header.json"; then
  printf 'PASS: the header after the undo is the shipped round again\n'
else
  printf 'FAIL: the header after the undo is the shipped round again\n'
  fails=$((fails + 1))
fi

expect "the vibe ledger names the undo" \
  0 "undone \(back to 136" \
  -- "$LM" vibe ledger --runs "$WORK/vibe-runs"

if [ "$fails" -gt 0 ]; then
  printf '\n==> %d step(s) FAILED\n' "$fails"
  exit 1
fi

printf '\n==> every step passed\n'
