#!/usr/bin/env python3
"""report — one soak run, written down: history.csv, latest.md, status.json.

Why this exists. The soak's runner (scripts/soak.sh) knows whether a step
passed; it does not know what the numbers mean. The numbers the live lane
already prints — talk_soak's CSV rows, the round's AgentStats, the
realizer's word counts, the ears' word accuracy — are the reason the soak
runs at all: a lane that "passes" while the agents stopped talking is not
a green lane. So the raw logs are parsed here, one row is appended to
history.csv, and a short human summary (latest.md) is written that names
what moved.

What a regression is, here: a step that went from pass to fail, or a
number that moved more than LM_SOAK_PCT percent since the previous run
(the threshold is a parameter; the default is 25). The 7-run median sits
beside every number, because on a shared box a single run jumps and the
median is what "normal" means.

Stdlib only, like the Thor index that reads the status this writes: a
reporter that cannot run because a wheel is missing is not a reporter.

Usage (from scripts/soak.sh):

    python3 tools/soak/report.py --home ~/lm-soak --logs <run-logs-dir> \
        --stamp <time> --tip <sha> --pct 25 --started <epoch> \
        --fetch ok --build ok --golden ok --check ok --live ok
"""

import argparse
import csv
import json
import os
import re
import statistics
import sys
import time

# The columns of history.csv, in order. The steps, then the numbers the
# live lane prints. A number no test printed stays empty — an honest dash
# beats an invented zero (the ears' word accuracy is printed only when a
# test fails, and the voice hit/late pair is printed by no live test yet).
COLUMNS = [
    "stamp",
    "tip",
    "fetch",
    "build",
    "golden",
    "check",
    "live",
    "talk_stub_utts",
    "talk_stub_decisions",
    "talk_thor_utts",
    "talk_thor_decisions",
    "talk_thor_refusals",
    "round_decisions",
    "round_escalations",
    "words_llm",
    "words_phrasebook",
    "words_late",
    "voice_hit",
    "voice_late",
    "ears_permille",
    "duration_s",
]

# The human name of every tracked number, for latest.md. The key is the
# history column; the value is what a person calls it.
NUMBERS = {
    "talk_stub_utts": "talk soak: utterances (stub, 3 rounds — deterministic)",
    "talk_stub_decisions": "talk soak: decisions (stub, 3 rounds)",
    "talk_thor_utts": "talk soak: utterances (thor-d1-2b, 3 rounds)",
    "talk_thor_decisions": "talk soak: decisions (thor-d1-2b, 3 rounds)",
    "talk_thor_refusals": "talk soak: refusals (thor-d1-2b, 3 rounds)",
    "round_decisions": "live round: the Thor's decisions became",
    "round_escalations": "live round: escalations (D71)",
    "words_llm": "realizer: words by the model",
    "words_phrasebook": "realizer: words by the phrasebook",
    "words_late": "realizer: words late",
    "voice_hit": "voice: bark-cache hits",
    "voice_late": "voice: late",
    "ears_permille": "ears: word accuracy (per-mille)",
}

# The steps of the lane, in order, as history.csv records them.
STEPS = ["fetch", "build", "golden", "check", "live"]

# talk_soak's CSV rows: backend,seed,utterances,airtime,per-1k,decisions,
# refusals,scores — the scores column may contain commas, so the line is
# matched (not split) and only the first seven fields are taken.
TALK_ROW = re.compile(
    r"^(stub|thor-d1-2b),([0-9]+),([0-9]+),([0-9]+),([0-9]+),([0-9]+),([0-9]+),",
    re.MULTILINE,
)

# The round's own report, printed by live_round_on_thor: one AgentStats
# debug line with the decisions and the escalations in it.
ROUND_STATS = re.compile(r"^the round: AgentStats \{.*$", re.MULTILINE)

# The realizer's summary line: words llm N / phrasebook N / late N.
WORDS = re.compile(r"- words llm ([0-9]+) / phrasebook ([0-9]+) / late ([0-9]+)")

# The voice pair, recorded when a live test prints it (none does yet; the
# parsers are here so the day one does, the history starts itself).
VOICE_HIT = re.compile(r"bark_hits: ([0-9]+)")
VOICE_LATE = re.compile(r"TalkCounters \{[^}]*late: ([0-9]+)")

# The ears' word accuracy, printed by the round-trip test when it fails
# (and by whatever test prints it the day one does).
EARS = re.compile(r"([0-9]+) per-mille")

# The three endings of scripts/live.sh (and the fourth: a Thor that is
# down is not a failure of the code, so SKIP is an outcome, not an error).
LIVE_ENDINGS = [
    (re.compile(r"==> live checks passed"), "pass"),
    (re.compile(r"==> live checks FAILED"), "fail"),
    (re.compile(r"==> live checks INCOMPLETE"), "incomplete"),
    (re.compile(r"SKIP: Thor unreachable"), "skip"),
]


def read(path):
    """A log file as text, or '' when the step never wrote one."""
    try:
        with open(path, encoding="utf-8", errors="replace") as fh:
            return fh.read()
    except OSError:
        return ""


def parse_live(log):
    """The live lane's outcome and numbers, out of the log it printed."""
    numbers = {}

    # The outcome: the last ending the log carries decides (a lane that
    # skips one test prints INCOMPLETE after the passes that came before).
    outcome = "skip"  # a log with no ending at all proved nothing
    for pattern, word in LIVE_ENDINGS:
        if pattern.search(log):
            outcome = word
    numbers["live"] = outcome

    # talk_soak's rows, summed per backend: three rounds each.
    sums = {}
    for backend, _seed, utts, _air, _per1k, decisions, refusals in TALK_ROW.findall(log):
        key = "stub" if backend == "stub" else "thor"
        u, d, r = sums.get(key, (0, 0, 0))
        sums[key] = (u + int(utts), d + int(decisions), r + int(refusals))
    if "stub" in sums:
        numbers["talk_stub_utts"], numbers["talk_stub_decisions"], _ = sums["stub"]
    if "thor" in sums:
        (
            numbers["talk_thor_utts"],
            numbers["talk_thor_decisions"],
            numbers["talk_thor_refusals"],
        ) = sums["thor"]

    # The round's AgentStats: decisions and escalations.
    line = ROUND_STATS.search(log)
    if line:
        stats = line.group(0)
        for field, column in (("decisions:", "round_decisions"), ("escalations:", "round_escalations")):
            found = re.search(field + r" ([0-9]+)", stats)
            if found:
                numbers[column] = int(found.group(1))

    # The realizer's word counts.
    words = WORDS.search(log)
    if words:
        numbers["words_llm"] = int(words.group(1))
        numbers["words_phrasebook"] = int(words.group(2))
        numbers["words_late"] = int(words.group(3))

    # The voice pair and the ears' accuracy: only when a test printed them.
    hit = VOICE_HIT.search(log)
    if hit:
        numbers["voice_hit"] = int(hit.group(1))
    late = VOICE_LATE.search(log)
    if late:
        numbers["voice_late"] = int(late.group(1))
    ears = EARS.search(log)
    if ears:
        numbers["ears_permille"] = int(ears.group(1))

    return numbers


def load_history(path):
    """The history rows, oldest first; the header is written when missing."""
    if not os.path.exists(path):
        with open(path, "w", encoding="utf-8", newline="") as fh:
            csv.DictWriter(fh, COLUMNS).writeheader()
    with open(path, encoding="utf-8", newline="") as fh:
        return [row for row in csv.DictReader(fh) if row.get("stamp")]


def write_atomic(path, text):
    """A file that appears whole or not at all: the index reads it live."""
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as fh:
        fh.write(text)
    os.replace(tmp, path)


def verdict(row):
    """The run's one word: fail beats partial beats pass."""
    steps = {step: row.get(step, "") for step in STEPS}
    if "fail" in steps.values():
        return "fail"
    if steps.get("live") in ("pass",) and all(
        steps.get(step) == "ok" for step in ("fetch", "build", "golden", "check")
    ):
        return "pass"
    return "partial"


def fmt(value):
    """A number for the table: whole, or a dash when nothing printed it."""
    return str(value) if value not in (None, "") else "—"


def moved(now, prev, pct):
    """Did a number move more than the threshold since the previous run?

    A percent against zero is undefined, so a column that was empty or zero
    and now is not counts as moved — the threshold is there to be tuned,
    not to hide a 0-to-N step change behind arithmetic.
    """
    if now in (None, "") or prev in (None, ""):
        return None
    now, prev = int(now), int(prev)
    if now == prev:
        return None
    if prev == 0:
        return f"0 → {now}"
    change = abs(now - prev) * 100.0 / prev
    if change > pct:
        return f"{now - prev:+d} ({change:.0f}%)"
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--home", required=True, help="the soak clone (~/lm-soak)")
    ap.add_argument("--logs", required=True, help="this run's log directory")
    ap.add_argument("--stamp", required=True, help="the run's timestamp")
    ap.add_argument("--tip", required=True, help="the trunk tip (short sha)")
    ap.add_argument("--pct", type=float, default=25.0, help="the regression threshold, in %")
    ap.add_argument("--started", type=int, default=0, help="the run's start, epoch seconds")
    ap.add_argument("--fetch", default="ok")
    ap.add_argument("--build", default="ok")
    ap.add_argument("--golden", default="ok")
    ap.add_argument("--check", default="ok")
    ap.add_argument("--live", default="ok", help="live.sh's exit-code verdict")
    args = ap.parse_args()

    # The numbers, out of the lane's own logs. The live log's own ending is
    # the truth of the live column when it printed one; the exit code that
    # soak.sh saw is the fallback.
    numbers = parse_live(read(os.path.join(args.logs, "live.log")))
    if numbers["live"] in ("pass", "skip", "incomplete", "fail"):
        live = numbers["live"]
    else:
        live = "fail" if args.live != "ok" else "incomplete"

    row = {column: "" for column in COLUMNS}
    row.update(
        stamp=args.stamp,
        tip=args.tip,
        fetch=args.fetch,
        build=args.build,
        golden=args.golden,
        check=args.check,
        live=live,
        duration_s=int(time.time()) - args.started if args.started else "",
    )
    for column in NUMBERS:
        if column in numbers:
            row[column] = numbers[column]

    history_path = os.path.join(args.home, "history.csv")
    history = load_history(history_path)
    with open(history_path, "a", encoding="utf-8", newline="") as fh:
        csv.DictWriter(fh, COLUMNS).writerow(row)

    prev = history[-1] if history else None
    # The 7-run median: the seven runs before this one, however many there
    # are. A shared box jumps; the median is what normal looks like.
    window = history[-7:]

    overall = verdict(row)
    regressions = []

    # A step that went from pass to fail.
    if prev:
        for step in STEPS:
            was, now = prev.get(step, ""), row[step]
            if was in ("ok", "pass") and now == "fail":
                regressions.append(f"{step} went from pass to fail")

    # A number that moved more than the threshold.
    number_rows = []
    for column, label in NUMBERS.items():
        now_value = row.get(column, "")
        prev_value = prev.get(column, "") if prev else ""
        values = [int(r[column]) for r in window if r.get(column, "") not in ("", None)]
        med = statistics.median(values) if values else None
        change = moved(now_value, prev_value, args.pct) if prev else None
        if change:
            regressions.append(f"{label} moved {change}")
        number_rows.append(
            (label, fmt(now_value), fmt(prev_value), "—" if med is None else f"{med:g}", change or "")
        )

    # --- latest.md: the human summary --------------------------------------
    duration = row["duration_s"]
    mins = f"{int(duration) // 60} min" if duration != "" else "—"
    lines = []
    lines.append(f"# the soak — {args.stamp}")
    lines.append("")
    lines.append(f"- tip: `{args.tip}` on origin/m1-integration")
    lines.append(
        f"- verdict: **{overall}** — fetch {row['fetch']}, build {row['build']}, "
        f"golden {row['golden']}, check {row['check']}, live {row['live']} "
        f"(the whole run {mins})"
    )
    if prev:
        lines.append(
            f"- previous run: {prev['stamp']} on `{prev['tip']}` "
            f"(verdict {verdict(prev)})"
        )
    else:
        lines.append("- previous run: none — this is the first row in history.csv")
    lines.append("")
    lines.append("## the live numbers")
    lines.append("")
    lines.append("| number | this run | previous | 7-run median | moved |")
    lines.append("|---|---|---|---|---|")
    for label, now_value, prev_value, med, change in number_rows:
        mark = f"⚠ {change}" if change else ""
        lines.append(f"| {label} | {now_value} | {prev_value} | {med} | {mark} |")
    lines.append("")
    lines.append("## regressions")
    lines.append("")
    if regressions:
        for note in regressions:
            lines.append(f"- ⚠ {note}")
    else:
        lines.append("- none")
    lines.append("")
    lines.append(
        f"(the raw logs of this run: {os.path.relpath(args.logs, args.home)}; "
        f"a number is — when no live test printed it)"
    )
    lines.append("")
    write_atomic(os.path.join(args.home, "latest.md"), "\n".join(lines))

    # --- status.json: the Thor index's window on the soak ------------------
    status = {
        "time": args.stamp,
        "tip": args.tip,
        "status": overall,
        "live": live,
        "steps": {step: row[step] for step in STEPS},
        "regressions": regressions,
        "duration_s": row["duration_s"],
        "runs": len(history) + 1,
    }
    write_atomic(os.path.join(args.home, "status.json"), json.dumps(status, indent=2) + "\n")

    print("\n".join(lines))
    return 0


if __name__ == "__main__":
    sys.exit(main())
