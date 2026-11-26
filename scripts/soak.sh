#!/usr/bin/env bash
# The standing soak on the Thor: fetch the trunk tip, build it in release,
# and run the whole lane (golden, check, live) every few hours, so a
# regression in last-mile's live behaviour is caught the same day it lands
# and not the next time a person runs the checks by hand.
#
# Why a soak and not another test: the live regressions of this week (score
# criteria as a map that broke every Thor request; a docs-only folder that
# broke the workspace) passed the offline gate, because the gate never asks
# the Thor. The only check that asks is scripts/live.sh, and nobody was
# asking. This script is the someone: it asks on a timer, from a clone of
# its own, and writes down what the Thor said.
#
# It runs from the dedicated clone at ~/lm-soak (never from a seat's
# worktree): the timer resets that clone to origin/m1-integration before
# each run, so the soak measures the trunk and only the trunk, and a seat's
# half-finished branch can never colour it.
#
#   bash scripts/soak.sh          # one run, by hand
#   systemctl --user list-timers lm-soak.timer   # the standing schedule
#
# What it writes, all inside the clone (kept out of git by .git/info/exclude):
#
#   history.csv   one row per run: the tip, pass/fail per step, the numbers
#   latest.md     the human summary: this run vs the previous and the median
#   status.json   the last run for the Thor index (:8100)
#   logs/         one directory per run, the raw output of every step
#   skipped.log   the runs that did not happen, and why
#
# The parameters (env-overridable):
#
#   LM_SOAK_JOBS  how many cores cargo and the tests may use (default: half
#                 the box — the seats share it, and the d1 servers share
#                 its GPU)
#   LM_SOAK_PCT   the regression threshold: a live number that moved more
#                 than this many percent since the previous run is marked
#                 (default 25)
#   LM_SOAK_BRANCH  the trunk to follow (default m1-integration)
#   LM_SOAK_HOME    the only directory the script will run in (default
#                   ~/lm-soak — override to try the lane in a scratch clone)
set -uo pipefail
cd "$(dirname "$0")/.."

SOAK_HOME="$(pwd)"
BRANCH="${LM_SOAK_BRANCH:-m1-integration}"

# The one place this may run: the dedicated clone. The reset below throws
# away whatever the clone has checked out, and a seat's worktree is a
# seat's own state — a lane tried from one would reset the seat's branch
# to the trunk tip and call it testing. So the script refuses; this
# actually happened here (a seat's branch moved three commits under it),
# which is why the check is the script's and not the operator's memory.
ALLOWED_HOME="${LM_SOAK_HOME:-$HOME/lm-soak}"
if [ "$SOAK_HOME" != "$ALLOWED_HOME" ]; then
  printf 'soak: refusing to run from %s\n' "$SOAK_HOME"
  printf '  the soak resets its clone hard to origin/%s, and only the\n' "$BRANCH"
  printf '  dedicated clone (%s) may be reset. To try the lane in a\n' "$ALLOWED_HOME"
  printf '  scratch clone of its own, point LM_SOAK_HOME at that clone.\n'
  exit 2
fi
JOBS="${LM_SOAK_JOBS:-$(( $(nproc 2>/dev/null || echo 2) / 2 ))}"
PCT="${LM_SOAK_PCT:-25}"
ORIGIN="${LM_SOAK_ORIGIN:-origin}"

# --- one soak at a time -------------------------------------------------------
# The lane takes half an hour; the timer fires every three. A run that
# overlaps its predecessor is skipped, not queued: two soaks would double
# the load on the box and race for the d1 servers the seats are using.
exec 9>"$SOAK_HOME/soak.lock"
if ! flock -n 9; then
  printf '%s skipped: another soak is still running\n' "$(date +%Y-%m-%dT%H:%M:%S%z)" \
    >>"$SOAK_HOME/skipped.log"
  printf 'SKIP: another soak is still running (see skipped.log)\n'
  exit 0
fi

STAMP="$(date +%Y-%m-%dT%H:%M:%S%z)"
LOGS="$SOAK_HOME/logs/$STAMP"
mkdir -p "$LOGS"
STARTED="$(date +%s)"

# The concurrency cap: cargo builds at most JOBS crates at once and the
# test harness runs at most JOBS tests at once. The live tests pace
# themselves at the round's own rate (30 Hz sleeps), so the cap costs them
# nothing; what it protects is the seats building beside us.
export CARGO_BUILD_JOBS="$JOBS"
export RUST_TEST_THREADS="$JOBS"

# run <name> <log> <command...>
#
# Runs one step of the lane with its output teed to the run's log. The
# soak never stops early: a failing step is a finding to record, and the
# steps after it still say something (a build that fails fails the golden
# and the checks too, but the live lane can still run and often names the
# culprit).
run() {
  local name="$1" log="$2"
  shift 2
  printf '==> soak step: %s\n' "$name"
  if "$@" >"$log" 2>&1; then
    printf '%s\n' "$name ok"
    return 0
  fi
  printf '%s FAILED (the tail of %s):\n' "$name" "$log"
  tail -n 20 "$log"
  return 1
}

# --- the lane -----------------------------------------------------------------

# 1. the tip: fetch the trunk and reset the clone to it. This clone has no
#    other job, so a hard reset is safe; the state files below are
#    untracked and survive it (they are in .git/info/exclude, and reset
#    never touches untracked files).
git fetch "$ORIGIN" "$BRANCH" >"$LOGS/fetch.log" 2>&1 &&
  git reset --hard "origin/$BRANCH" >>"$LOGS/fetch.log" 2>&1
FETCH=$?
TIP="$(git rev-parse --short "origin/$BRANCH" 2>/dev/null || echo '?')"
if [ "$FETCH" -ne 0 ]; then
  # Without the tip there is nothing to soak: this run is over, but it is
  # still a row in the history (a trunk nobody can reach is a finding).
  python3 tools/soak/report.py \
    --home "$SOAK_HOME" --logs "$LOGS" --stamp "$STAMP" \
    --tip "$TIP" --pct "$PCT" --started "$STARTED" \
    --fetch fail --build skip --golden skip --check skip --live skip
  cat "$SOAK_HOME/latest.md"
  exit 1
fi

# 2. the build: release, as the lane's own tests assume.
run build "$LOGS/build.log" cargo build --release
BUILD=$?

# 3. the golden: the pinned hashes of the headless rounds.
if [ "$BUILD" -eq 0 ]; then
  run golden "$LOGS/golden.log" ./target/release/lm golden
  GOLDEN=$?
else
  GOLDEN=99
fi

# 4. the gate: everything that must pass before a merge.
run check "$LOGS/check.log" bash scripts/check.sh
CHECK=$?

# 5. the live lane: the tests that ask the Thor. It prints SKIP and exits
#    0 when the Thor is not up; the report reads which of the three
#    endings the log carries (passed / INCOMPLETE / FAILED / SKIP).
run live "$LOGS/live.log" bash scripts/live.sh
LIVE=$?

# --- the record ---------------------------------------------------------------

python3 tools/soak/report.py \
  --home "$SOAK_HOME" --logs "$LOGS" --stamp "$STAMP" \
  --tip "$TIP" --pct "$PCT" --started "$STARTED" \
  --fetch ok \
  --build "$([ "$BUILD" -eq 0 ] && echo ok || echo fail)" \
  --golden "$([ "$GOLDEN" -eq 0 ] && echo ok || { [ "$GOLDEN" -eq 99 ] && echo skip || echo fail; })" \
  --check "$([ "$CHECK" -eq 0 ] && echo ok || echo fail)" \
  --live "$([ "$LIVE" -eq 0 ] && echo ok || echo fail)"

cat "$SOAK_HOME/latest.md"

# The logs of the last ten runs are kept; older ones go. The history and
# the summaries are the record — the logs are the evidence for the latest
# disagreement only.
ls -1 "$SOAK_HOME/logs" | sort | head -n -10 | while read -r old; do
  rm -rf "$SOAK_HOME/logs/$old"
done

# The exit code is the run's verdict: a fail or a skipped live lane is not
# a green soak, but a SKIP (Thor down) is not a red one either.
if [ "$BUILD" -ne 0 ] || [ "$GOLDEN" -ne 0 ] || [ "$CHECK" -ne 0 ] || [ "$LIVE" -ne 0 ]; then
  exit 1
fi
exit 0
