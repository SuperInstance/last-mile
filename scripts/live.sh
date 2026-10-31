#!/usr/bin/env bash
# The live lane: every test that needs the real Thor, in one run.
#
# Why this script exists: every test that talks to the Thor is #[ignore]d,
# because the Thor is a box on the LAN and not a dependency of the build.
# That is how a live regression shipped unnoticed — the Thor rejected every
# System One request (HTTP 400), the agents made 0 decisions, and no check
# noticed, because no check ever asked the Thor.
#
# This is the check that asks. It is not part of scripts/check.sh: the Thor
# has to be up, and a missing Thor is not a failure of the code.
#
#   1. the preflight: the Thor's health, on the ports in AgentsConfig's
#      defaults. If the Thor is unreachable, it says SKIP and exits 0.
#   2. every ignored test that needs the Thor, run --release --ignored
#      --nocapture.
#   3. the live facts: the Thor's answers become decisions in the game, and
#      the soak prints its table.
#
# Run it on the LAN, with the Thor's models up:
#
#   bash scripts/live.sh
#
# The tests it runs, and the port each one needs:
#
#   lm-agents      live_thor                                 d1-3b
#   lm-host-model  live_round_on_thor                        d1-3b
#   lm-host-model  the_doctrine_pack_is_confident_enough...  d1-3b
#   lm-host-model  talk_soak                                 d1-2b
#   lm-learn       the_vibe_gate_table_on_the_thor           d1-2b
#   lm-voice       live_skald_speaks                         the voice daemon
#   lm-voice       live_skald_round_trip_is_intelligible     the voice daemon (+ the ears)
#   lm-host-model  the_ears_hear_a_line...                   the ears (+ the voice daemon)
#   lm-agents      realizer_on_thor                          d1-2b, realizer
#
# The Riva tests (lm-voice's live_riva_tts and lm-host-model's
# live_round_with_riva) are not in this lane: they need a Riva service, not
# the Thor.
#
# The voice daemon and the ears run on the Thor as well, so they are
# probed and addressed the same way as the models: the Thor's base from
# the agents' config, the daemon's port from the voice map, and the
# ears' address from the live test that talks to them. Nothing in this
# lane assumes "localhost" -- that is only right on the daemon's own box,
# and the lane is often run from another one (the ProArt).
#
# The ears are the Thor's speech-to-text. They are not a decision model, so
# the agents' config does not name them: the live test says where they are,
# and the lane reads it from there.
set -uo pipefail
cd "$(dirname "$0")/.."

# The Thor's address and ports come from the config the agents use, so the
# lane follows the config and the config is the one thing that says where the
# Thor is.
CONFIG="crates/lm-agents/src/config.rs"

# The live test that talks to the ears: it says where they are.
EARS_TEST="crates/lm-host-model/tests/ears.rs"

# The voice map: the config that says where the voice daemon is.
VOICE_MAP="assets/harness/default/voice.toml"

# How long to wait for the Thor to answer a health call, in seconds.
PROBE_TIMEOUT="${LM_PROBE_TIMEOUT:-3}"

# The Thor's base URL, as AgentsConfig's default has it.
thor_base() {
  sed -n 's/.*thor_base: "\([^"]*\)".*/\1/p' "$CONFIG" | head -1
}

# The Thor's base URL, or a loud failure: a config that cannot be read is
# not a Thor that is switched off.
need_base() {
  local base
  base="$(thor_base)"
  if [ -z "$base" ]; then
    printf 'FAIL: %s has no thor_base to talk to\n' "$CONFIG"
    exit 1
  fi
  printf '%s' "$base"
}

# The Thor's model ports, as "name port" lines, in the config's order.
thor_ports() {
  awk '/ports: BTreeMap::from\(\[/ {p=1} p {print} p && /\]\)/ {p=0}' "$CONFIG" |
    grep -oE '\("[^"]+"\.to_owned\(\), *[0-9]+\)' |
    sed -E 's/\("([^"]+)"\.to_owned\(\), *([0-9]+)\)/\1 \2/'
}

# The port of one model, by its name in the config.
port_of() {
  thor_ports | awk -v name="$1" '$1 == name {print $2; exit}'
}

# The URL of the ears, as the live test has it.
ears_url() {
  sed -n 's/^const EARS: &str = "\([^"]*\)".*/\1/p' "$EARS_TEST" | head -1
}

# The port the voice daemon answers on, as the voice map says it.
#
# The map is the config: it says which port the daemon is on, and the
# lane reads it from there instead of repeating the number.
skald_port() {
  sed -n '/^\[skald\]/,/^\[/ s/^port = \([0-9]*\).*/\1/p' "$VOICE_MAP" | head -1
}

# Where the voice daemon is.
#
# The daemon runs on the Thor, and the lane can run from another box
# (the ProArt), so its address is read from the config the rest of the
# lane reads: the Thor's base and the voice map's port. Nothing here
# assumes "localhost", which only ever works on the daemon's own box.
skald_url() {
  printf '%s:%s' "$(thor_base)" "$(skald_port)"
}

# The port of one model, or a loud failure: a test that needs a port the
# config does not name cannot be run.
need_port() {
  local port
  port="$(port_of "$1")"
  if [ -z "$port" ]; then
    printf 'FAIL: the config has no port for the model %s\n' "$1"
    exit 1
  fi
  printf '%s' "$port"
}

# Is the Thor's port serving? The Thor's server answers /health once its
# model is loaded. Some builds do not have that path, so the model list is
# tried next, and the last resort is any HTTP answer at all.
#
# The proxy is ignored: the Thor is on the LAN, and a proxy in the
# environment would only break the route to it.
port_is_up() {
  local url
  url="$(thor_base):${1}"
  curl -fsS --noproxy '*' --connect-timeout "$PROBE_TIMEOUT" --max-time "$PROBE_TIMEOUT" \
    "$url/health" >/dev/null 2>&1 && return 0
  curl -fsS --noproxy '*' --connect-timeout "$PROBE_TIMEOUT" --max-time "$PROBE_TIMEOUT" \
    "$url/v1/models" >/dev/null 2>&1 && return 0
  curl -sS --noproxy '*' --connect-timeout "$PROBE_TIMEOUT" --max-time "$PROBE_TIMEOUT" \
    -o /dev/null "$url/" >/dev/null 2>&1 && return 0
  return 1
}

# The port the realizer serves on, as AgentsConfig's default has it.
#
# The realizer is not in the `ports` map: it is not a decision model, it is
# the model that writes the wording of a line, and it has its own port.
llm_port() {
  sed -n 's/.*llm_port: \([0-9][0-9]*\).*/\1/p' "$CONFIG" | head -1
}

# Is a URL serving? The same probe, for a service that is named by its own
# URL rather than by a port of the Thor's base.
#
# The paths are the ones the services answer: the ears' door answers
# /health, the voice daemon answers /v1/health (its / is slow — it takes
# seconds — so it is the last resort and not the first try).
url_is_up() {
  local url="$1"
  for path in /health /v1/health /v1/voices; do
    curl -fsS --noproxy '*' --connect-timeout "$PROBE_TIMEOUT" --max-time "$PROBE_TIMEOUT" \
      "$url$path" >/dev/null 2>&1 && return 0
  done
  curl -sS --noproxy '*' --connect-timeout "$PROBE_TIMEOUT" --max-time "$PROBE_TIMEOUT" \
    -o /dev/null "$url/" >/dev/null 2>&1 && return 0
  return 1
}

# Is the service serving? A port is on the Thor's base; a URL is its own.
service_is_up() {
  case "$1" in
    http://* | https://*) url_is_up "$1" ;;
    *) port_is_up "$1" ;;
  esac
}

# Where the service is, in a phrase.
where_of() {
  case "$1" in
    http://* | https://*) printf '%s' "$1" ;;
    *) printf 'the Thor on :%s' "$1" ;;
  esac
}

# The logs of the runs, in a scratch directory of their own: the repo is
# never written to.
WORK="$(mktemp -d "${TMPDIR:-/tmp}/lm-live.XXXXXX")"
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

passed=0
failed=0
skipped=0

# run_live <label> <port> <check> <cargo args...>
#
# Runs one live test and checks the live fact it is here to prove. The check
# is a function that reads the log the test wrote.
run_live() {
  local label="$1" port="$2" check="$3"
  shift 3
  local log="$WORK/$(printf '%s' "$label" | tr -c 'A-Za-z0-9' '_').log"

  printf '==> %s (%s)\n' "$label" "$(where_of "$port")"

  if ! service_is_up "$port"; then
    printf "SKIP: %s -- %s is not serving\n" "$label" "$(where_of "$port")"
    skipped=$((skipped + 1))
    return 0
  fi

  if ! cargo test --release "$@" -- --ignored --nocapture 2>&1 | tee "$log"; then
    printf 'FAIL: %s -- the test failed\n' "$label"
    failed=$((failed + 1))
    return 1
  fi

  # The test has to have run: a filter that matches nothing is a lane that
  # proves nothing.
  if ! grep -qE 'test result: ok\. 1 passed' "$log"; then
    printf 'FAIL: %s -- the test did not run\n' "$label"
    failed=$((failed + 1))
    return 1
  fi

  if ! "$check" "$log"; then
    printf 'FAIL: %s -- the live fact was not proved\n' "$label"
    failed=$((failed + 1))
    return 1
  fi

  printf 'PASS: %s\n' "$label"
  passed=$((passed + 1))
  return 0
}

# The fact of a test that proves its own: it ran, and it passed.
check_ran() {
  grep -qE 'test result: ok\. 1 passed' "$1"
}

# The live fact: the Thor's answers became decisions in the game. The test
# asserts it too; this reads the number back out of the log, so the lane
# still fails if that assertion is ever weakened.
check_thor_decisions() {
  local decisions
  decisions="$(grep -oE 'AgentStats \{.*decisions: [0-9]+' "$1" |
    tail -1 | sed -E 's/.*decisions: ([0-9]+).*/\1/')"
  if [ -z "$decisions" ] || [ "$decisions" -eq 0 ]; then
    printf 'the Thor made no decisions\n'
    return 1
  fi
  printf 'the Thor made %s decisions\n' "$decisions"
  return 0
}

# The live fact: the soak printed its table — one CSV line per run, three
# rounds on the stub and three on the Thor.
check_soak_table() {
  local stub thor
  stub="$(grep -cE '^stub,[0-9]+,' "$1")"
  thor="$(grep -cE '^thor-d1-2b,[0-9]+,' "$1")"
  if [ "$stub" -ne 3 ] || [ "$thor" -ne 3 ]; then
    printf 'the table is short: %s stub line(s), %s Thor line(s)\n' "$stub" "$thor"
    return 1
  fi
  printf 'the table has %s stub line(s) and %s Thor line(s)\n' "$stub" "$thor"
  return 0
}

# The live fact: the vibe gate's measurement ran to its summary line. The
# test asserts the accuracy floors; this reads the summary back out of the
# log, so the lane still fails if that assertion is ever weakened.
check_vibe_gate() {
  local measured
  measured="$(grep -oE 'the vibe gate measured: .*' "$1" | tail -1)"
  if [ -z "$measured" ]; then
    printf 'the measurement never reached its summary\n'
    return 1
  fi
  printf '%s\n' "$measured"
  return 0
}

# The live fact: the ears heard the line, and the line reached the score.
# The test asserts both; this reads the marker back out of the log, so the
# lane still fails if that assertion is ever weakened.
check_ears() {
  if ! grep -q 'the ears heard the line, and the line is in the log' "$1"; then
    printf 'the ears did not hear the line, or the line did not reach the log\n'
    return 1
  fi
  printf 'the ears heard the line and the line is in the log\n'
  return 0
}

# ------------------------------------------------------------------ the run

PORT_D1_3B="$(need_port d1-3b)" || exit 1
PORT_D1_2B="$(need_port d1-2b)" || exit 1
PORT_REALIZER="$(llm_port)"
if [ -z "$PORT_REALIZER" ]; then
  printf 'FAIL: the config has no llm_port for the realizer\n'
  exit 1
fi
THOR_BASE="$(need_base)" || exit 1

echo "==> the preflight: the Thor's health"
up=""
while read -r name port; do
  [ -n "$port" ] || continue
  if port_is_up "$port"; then
    printf 'up: %s on :%s\n' "$name" "$port"
    up="$up $port"
  else
    printf 'down: %s on :%s\n' "$name" "$port"
  fi
done <<EOF
$(thor_ports)
EOF

if [ -z "$up" ]; then
  # A missing Thor is not a failure of the code.
  printf 'SKIP: Thor unreachable (%s is not serving)\n' "$THOR_BASE"
  exit 0
fi

run_live "lm-agents live_thor" "$PORT_D1_3B" check_ran \
  -p lm-agents --lib live_thor

run_live "lm-host-model live_round_on_thor" "$PORT_D1_3B" check_thor_decisions \
  -p lm-host-model --test agents live_round_on_thor

run_live "lm-host-model the_doctrine_pack_is_confident_enough_to_drive_play" \
  "$PORT_D1_3B" check_ran \
  -p lm-host-model --test gates_tuning the_doctrine_pack_is_confident_enough_to_drive_play

run_live "lm-host-model talk_soak" "$PORT_D1_2B" check_soak_table \
  -p lm-host-model --test talk_soak talk_soak

# The vibe gate lane: Bench's two closed questions when a kid speaks,
# measured over the labelled phrasing fixture (docs/design/vibe-gate.md
# holds the table this prints and the command that reproduces it). The
# lane's floors live in the test: a pack that stops hearing three
# quarters of the labelled phrasings, or that matches six of the ten
# no-match asks to changes that do not exist, fails it.
run_live "lm-learn the_vibe_gate_table_on_the_thor" "$PORT_D1_2B" check_vibe_gate \
  -p lm-learn --test vibe_gate the_vibe_gate_table_on_the_thor

# The skald lane: the voice daemon, which runs on the Thor itself, not
# the decision models. The daemon's health path is /v1/health, which
# url_is_up does not try, so the probe is any HTTP answer at all — the
# daemon's / answers 200.
#
# The address is read from the config (the Thor's base and the voice
# map's port), so the lane works from another box: the daemon's
# "localhost" is only right on the daemon's own box. The same address
# goes to the tests through the environment, so the test and the probe
# are talking to the same daemon.
SKALD="$(skald_url)"
if [ -z "$SKALD" ]; then
  printf 'FAIL: the voice map %s does not say which port the daemon is on\n' "$VOICE_MAP"
  failed=$((failed + 1))
elif ! url_is_up "$SKALD"; then
  printf 'SKIP: the skald lane -- the voice daemon %s is not serving\n' "$SKALD"
  skipped=$((skipped + 1))
else
  # The base, without the port: the port is the voice map's, and the
  # tests take it from there.
  export LM_SKALD_BASE="$(thor_base)"
  run_live "lm-voice live_skald_speaks" "$SKALD" check_ran \
    -p lm-voice --lib live_skald_speaks

  # The round trip: the rendered line is heard back by an ear. The gate
  # asks the door on the Thor first (a real speech-to-text model), so
  # the whisper venv is only a fallback and not a requirement.
  run_live "lm-voice live_skald_round_trip_is_intelligible" "$SKALD" check_ran \
    -p lm-voice --lib live_skald_round_trip_is_intelligible
fi

# The ears: the Thor's speech-to-text. The line is rendered by the
# Thor's voice and heard by the Thor's ears, and the line reaches the
# score. The ears' address comes from the live test and the voice's
# address comes from the config, and both go to the test through the
# environment, so the probe and the test talk to the same services.
EARS="$(ears_url)"
VOICE="$(skald_url)"
if [ -z "$EARS" ] || [ -z "$VOICE" ]; then
  printf 'FAIL: %s does not say where the ears are, and the voice map does not say where the voice is\n' "$EARS_TEST"
  failed=$((failed + 1))
elif ! url_is_up "$VOICE"; then
  printf 'SKIP: the ears lane -- the voice %s is not serving\n' "$VOICE"
  skipped=$((skipped + 1))
elif ! url_is_up "$EARS"; then
  printf 'SKIP: the ears lane -- the ears %s are not serving\n' "$EARS"
  skipped=$((skipped + 1))
else
  export LM_EARS_BASE="$EARS"
  export LM_SKALD_BASE="$(thor_base)"
  run_live "lm-host-model the_ears_hear_a_line" "$EARS" check_ears \
    -p lm-host-model --test ears the_ears_hear_a_line_and_the_line_reaches_the_score
fi

# The realizer: the model that writes the wording of a line. It needs the
# decision tier (d1-2b) and its own door, so both are checked.
if port_is_up "$PORT_REALIZER"; then
  run_live "lm-agents realizer_on_thor" "$PORT_D1_2B" check_ran \
    -p lm-agents --test realizer_on_thor realizer_on_thor
else
  printf 'SKIP: lm-agents realizer_on_thor -- the realizer is not serving on :%s\n' \
    "$PORT_REALIZER"
  skipped=$((skipped + 1))
fi

if [ "$failed" -gt 0 ]; then
  printf '\n==> live checks FAILED: %s of %s test(s) failed\n' \
    "$failed" "$((passed + failed))"
  exit 1
fi

if [ "$skipped" -gt 0 ]; then
  # The Thor is up but a model of it is not: the lane could not prove the
  # facts it exists to prove, so it does not call itself passed.
  printf '\n==> live checks INCOMPLETE: %s test(s) skipped, the Thor was not serving every port\n' \
    "$skipped"
  exit 1
fi

printf '\n==> live checks passed\n'
