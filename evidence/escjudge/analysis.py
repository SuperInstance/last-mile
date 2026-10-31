#!/usr/bin/env python3
"""The analysis of the escalation measurement (D75's open question).

Reads what `crates/lm-agents/tests/escalation_keep.rs` wrote:

  * `situations.jsonl`  — one line per distinct escalated move situation,
    with the d1-2B's own answer (in-round and offline), clef-flash's
    answer, the d1-3B judge's answer, and each call's round trip;
  * `arms_on.jsonl` / `arms_off.jsonl` — one line per seed per arm.

and prints the pre-registered verdicts (`tests/talk_on_thor.md`, "Does
escalation earn its keep"): the 2x2 table and the exact two-sided McNemar
test over its paired discordants, and the D78 exact one-sided sign test
over the arms' per-seed deltas. Every p-value is integer arithmetic — the
spirit of D69: no float near a verdict.
"""

from __future__ import annotations

import json
import math
import sys
from pathlib import Path

# clef-flash's own floor: a second opinion below it is no opinion (D71).
CLEF_FLOOR = 0.05


def binom(n: int, k: int) -> int:
    return math.comb(n, k)


def mcnemar_two_sided_exact(b: int, c: int) -> tuple[int, int, int]:
    """The exact two-sided McNemar p on the paired discordants.

    p = 2 * P(X <= min(b, c)) for X ~ Binomial(b + c, 1/2), as an exact
    fraction: (numerator, denominator), both integers. The capped decimal
    rendering is the caller's business.
    """
    n = b + c
    if n == 0:
        return (1, 1)
    k = min(b, c)
    tail = sum(binom(n, j) for j in range(0, k + 1))
    num = 2 * tail
    den = 2**n
    if num >= den:
        return (1, 1)
    return (num, den)


def fraction_text(num: int, den: int) -> str:
    """The fraction as decimal, truncated at 4 places, capped at 1."""
    if num >= den:
        return "1.0000"
    scaled = (num * 10_000) // den
    whole = scaled // 10_000
    frac = scaled % 10_000
    return f"{whole}.{frac:04d}"


def sign_test_one_sided(deltas: list[int]) -> tuple[int, int, int, int]:
    """D78's exact one-sided sign test, run on the positive side.

    Ties (delta 0) are excluded. Returns (moved, up, numerator,
    denominator) where the p-value is P(X >= up) for X ~
    Binomial(moved, 1/2) — the probability that at least this many of the
    seeds that moved moved up, under the null that a seed is as likely to
    move either way. The caller runs it on both signs for the two-sided
    reading.
    """
    moved = sum(1 for d in deltas if d != 0)
    up = sum(1 for d in deltas if d > 0)
    if moved == 0:
        return (0, 0, 1, 1)
    tail = sum(binom(moved, j) for j in range(up, moved + 1))
    return (moved, up, tail, 2**moved)


def percentile(values: list[float], q: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    rank = round((len(ordered) - 1) * q)
    return ordered[rank]


def load(path: Path) -> list[dict]:
    if not path.exists():
        return []
    out = []
    for line in path.read_text().splitlines():
        line = line.strip()
        if line:
            out.append(json.loads(line))
    return out


def situations_analysis(directory: Path) -> None:
    situations = load(directory / "situations.jsonl")
    print(f"# the situations: {len(situations)} distinct, from real rounds on the Thor")
    if not situations:
        return

    seeds = {s.get("seed") for s in situations}
    ticks = [s.get("tick", 0) for s in situations]
    print(f"- seeds {min(seeds)}-{max(seeds)} ({len(seeds)} of them), ticks {min(ticks)}-{max(ticks)}")

    # The determinism cross-check: the d1 servers are deterministic, so the
    # d1-2B's offline answer is the round's own answer wherever the round's
    # record carries one. The registered rule: the round's own choice where
    # the record has one, the offline answer where it does not (the
    # sub-floor situations, whose answer the shipped floor withholds).
    comparable = 0
    mismatched = 0
    for s in situations:
        in_round = s.get("in_round", {}).get("primary_choice")
        offline = s.get("d1_2b", {}).get("choice")
        if in_round is not None and offline is not None:
            comparable += 1
            if in_round != offline:
                mismatched += 1
    print(f"- the determinism cross-check: {comparable - mismatched} of {comparable} comparable agree"
          f" ({mismatched} mismatched)")

    def d12b_choice(s: dict) -> str | None:
        own = s.get("in_round", {}).get("primary_choice")
        if own is not None:
            return own
        return s.get("d1_2b", {}).get("choice")

    def escalated_choice(s: dict) -> str | None:
        """clef-flash's answer as the escalation takes it: its own top
        option when its confidence clears its floor, no answer below it."""
        clef = s.get("clef_flash", {})
        if clef.get("refusal") is not None:
            return None
        if clef.get("choice") is None:
            return None
        if (clef.get("confidence") or 0.0) < CLEF_FLOOR:
            return None
        return clef.get("choice")

    # The judge's refusals are excluded and counted, never dropped quietly.
    judged = []
    excluded = {"judge refused": 0, "judge answered nothing": 0,
                "d1-2B has no answer": 0}
    for s in situations:
        judge = s.get("judge", {})
        if judge.get("refusal") is not None:
            excluded["judge refused"] += 1
            continue
        if judge.get("choice") is None:
            excluded["judge answered nothing"] += 1
            continue
        if d12b_choice(s) is None and escalated_choice(s) is None:
            excluded["d1-2B has no answer"] += 1
            continue
        judged.append((s, judge["choice"]))
    print(f"- judged: {len(judged)}; excluded and counted: {excluded}")

    # The 2x2: is each party's answer the judge's own top option?
    both = both_wrong = 0
    b = c = 0  # b: the d1-2B wrong, the escalated answer right; c: the reverse
    for s, truth in judged:
        primary = d12b_choice(s)
        escalated = escalated_choice(s)
        p_right = primary == truth
        e_right = escalated == truth
        if p_right and e_right:
            both += 1
        elif not p_right and not e_right:
            both_wrong += 1
        elif e_right:
            b += 1
        else:
            c += 1
    n = len(judged)
    p_right_count = both + c
    e_right_count = both + b
    print()
    print("## the 2x2 (agreement with the judge)")
    print()
    print("| | the escalated answer right | the escalated answer wrong | |")
    print("|---|---|---|---|")
    print(f"| the d1-2B's own answer right | {both} | {c} | {p_right_count} |")
    print(f"| the d1-2B's own answer wrong | {b} | {both_wrong} | {n - p_right_count} |")
    print(f"| | {e_right_count} | {n - e_right_count} | {n} |")
    print()
    print(f"- the d1-2B's own answer agrees with the judge on {p_right_count} of {n}"
          f" ({100.0 * p_right_count / n:.1f}%)")
    print(f"- the escalated answer agrees on {e_right_count} of {n}"
          f" ({100.0 * e_right_count / n:.1f}%)")
    num, den = mcnemar_two_sided_exact(b, c)
    p_text = fraction_text(num, den)
    print(f"- the exact two-sided McNemar on the paired discordants "
          f"(b = {b}, c = {c}, n = {b + c}): p = {num}/{den} = {p_text}")
    keep = b > c and num < den and (num * 20 < den)
    print(f"- the registered rule: {'KEEP' if keep else 'TURN OFF'} "
          f"(keep needs b > c and p < 0.05)")

    # Secondary: the situations where the two models' own answers differ —
    # the only ones where the escalation changes anything.
    differ = [(s, t) for s, t in judged if d12b_choice(s) != escalated_choice(s)]
    same_answer = n - len(differ)
    d_b = sum(1 for s, t in differ if escalated_choice(s) == t and d12b_choice(s) != t)
    d_c = sum(1 for s, t in differ if d12b_choice(s) == t and escalated_choice(s) != t)
    d_neither = len(differ) - d_b - d_c
    print()
    print("## secondary: where the two models differ")
    print(f"- the two parties answer differently on {len(differ)} of {n} "
          f"({same_answer} identical answers)")
    if differ:
        print(f"- of those: the escalated answer right {d_b}, the d1-2B right {d_c}, neither {d_neither}")
        num, den = mcnemar_two_sided_exact(d_b, d_c)
        print(f"- McNemar on the differing situations: p = {fraction_text(num, den)}"
              f" ({num}/{den})")

    # Secondary: the split at the d1-2B's own floor (0.30).
    above = [(s, t) for s, t in judged if (s.get("primary_confidence") or 0.0) >= 0.30]
    below = [(s, t) for s, t in judged if (s.get("primary_confidence") or 0.0) < 0.30]
    print()
    print("## secondary: the split at the d1-2B's floor (0.30)")
    for name, group in [("at or above the floor (acted on alone when the escalation is off)", above),
                        ("below the floor (refused when the escalation is off)", below)]:
        if not group:
            print(f"- {name}: 0 situations")
            continue
        p_right = sum(1 for s, t in group if d12b_choice(s) == t)
        e_right = sum(1 for s, t in group if escalated_choice(s) == t)
        print(f"- {name}: {len(group)} situations; the d1-2B right {p_right}, "
              f"the escalated answer right {e_right}")

    # The latencies: each model's own round trip on the same single
    # question, offline, back to back.
    print()
    print("## the latencies (ms, one move question, offline)")
    for label, key in [("d1-2B", "d1_2b"), ("clef-flash", "clef_flash"), ("d1-3B (the judge)", "judge")]:
        values = [s[key]["latency_ms"] for s in situations
                  if s.get(key, {}).get("latency_ms") is not None]
        if not values:
            continue
        print(f"- {label}: min {min(values):.0f}, p50 {percentile(values, 0.5):.0f}, "
              f"p95 {percentile(values, 0.95):.0f}, max {max(values):.0f} over {len(values)} asks")

    # The in-round outcomes: what the fence did with the escalations.
    outcomes: dict[str, int] = {}
    for s in situations:
        outcome = s.get("in_round", {}).get("outcome", "?")
        outcomes[outcome] = outcomes.get(outcome, 0) + 1
    print()
    print(f"## the in-round outcomes of these escalations: {outcomes}")

    # The confidences the situations came in with.
    confs = [s.get("primary_confidence") or 0.0 for s in situations]
    print(f"- the d1-2B's confidence on the escalated gate: min {min(confs):.3f}, "
          f"p50 {percentile(confs, 0.5):.3f}, max {max(confs):.3f}")


def arms_analysis(directory: Path) -> None:
    on = {r["seed"]: r for r in load(directory / "arms_on.jsonl")}
    off = {r["seed"]: r for r in load(directory / "arms_off.jsonl")}
    shared = sorted(set(on) & set(off))
    print()
    print(f"# the arms: {len(shared)} seeds played both "
          f"({len(on)} on, {len(off)} off)")
    if not shared:
        return

    metrics = [
        ("tags", lambda r: r["tags"]),
        ("sight ticks", lambda r: r["sight_ticks"]),
        ("draws (margin 0)", lambda r: 1 if r["margin"] == 0 else 0),
        ("refusals", lambda r: r["refusals"]),
        ("decisions", lambda r: r["decisions"]),
        ("margin", lambda r: r["margin"]),
        ("lines", lambda r: r["utterances"]),
        ("escalations", lambda r: r["escalations"]),
        ("escalation cost ms", lambda r: r["escalation_cost_ms"]),
    ]

    header = "| seed | on: tags | sight | margin | refusals | decisions | esc | esc ms |" \
             " | off: tags | sight | margin | refusals | decisions |"
    print(header)
    print("|" + "---|" * 14)
    for seed in shared:
        r_on, r_off = on[seed], off[seed]
        print(f"| {seed} | {r_on['tags']} | {r_on['sight_ticks']} | {r_on['margin']} "
              f"| {r_on['refusals']} | {r_on['decisions']} | {r_on['escalations']} "
              f"| {r_on['escalation_cost_ms']} "
              f"| {r_off['tags']} | {r_off['sight_ticks']} | {r_off['margin']} "
              f"| {r_off['refusals']} | {r_off['decisions']} |")

    print()
    print("## the totals")
    print()
    print("| metric | on | off | delta (off - on) |")
    print("|---|---|---|---|")
    for name, get in metrics:
        total_on = sum(get(on[s]) for s in shared)
        total_off = sum(get(off[s]) for s in shared)
        print(f"| {name} | {total_on} | {total_off} | {total_off - total_on:+d} |")

    print()
    print("## the D78 sign test on the per-seed deltas (off - on), ties excluded")
    print()
    for name, get in metrics:
        deltas = [get(off[s]) - get(on[s]) for s in shared]
        moved_up, up, num_up, den_up = sign_test_one_sided(deltas)
        moved_down, down, num_down, den_down = sign_test_one_sided([-d for d in deltas])
        two_num = 2 * min(num_up, num_down)
        two_den = max(den_up, den_down)
        # The two-sided reading: twice the smaller one-sided p, capped at 1.
        # (The two one-sided tails are over the same moved count, so the
        # denominators agree; cap on the doubled numerator.)
        two_text = fraction_text(two_num, two_den)
        print(f"- {name}: moved {moved_up} of {len(shared)} "
              f"(up {up}, down {moved_up - up}); "
              f"p(off > on) = {fraction_text(num_up, den_up)}, "
              f"p(off < on) = {fraction_text(num_down, den_down)}, "
              f"two-sided {two_text}"
              + (" (below 0.05)" if two_num * 20 < two_den else ""))

    # The escalation's own cost in the on arm.
    esc = sum(on[s]["escalations"] for s in shared)
    cost = sum(on[s]["escalation_cost_ms"] for s in shared)
    latencies = [ms for s in shared for ms in on[s]["escalation_latencies_ms"]]
    outcomes: dict[str, int] = {}
    for s in shared:
        for entry in on[s].get("escalation_outcomes", []):
            outcomes[entry["outcome"]] = outcomes.get(entry["outcome"], 0) + entry["count"]
    print()
    print(f"## the escalation in the on arm: {esc} escalations over {len(shared)} rounds "
          f"({esc / len(shared):.1f} a round), {cost} ms of second opinions in all "
          f"({cost / max(1, len(shared)):.0f} ms a round)")
    if latencies:
        print(f"- the second opinion's round trip: p50 {percentile(latencies, 0.5):.0f} ms, "
              f"p95 {percentile(latencies, 0.95):.0f} ms, max {max(latencies)} ms")
    print(f"- the outcomes: {outcomes}")

    # The refusals by kind, both arms: the fence's own refusal
    # (`escalation_late`) is the on arm's signature, and everything else
    # should sit at the same level in both arms.
    by_kind: dict[str, dict[str, int]] = {"on": {}, "off": {}}
    for arm, table in [("on", on), ("off", off)]:
        for s in shared:
            for entry in table[s].get("refusal_kinds", []):
                by_kind[arm][entry["kind"]] = (by_kind[arm].get(entry["kind"], 0)
                                               + entry["count"])
    print()
    print("## the refusals by kind")
    print()
    kinds = sorted(set(by_kind["on"]) | set(by_kind["off"]))
    print("| kind | on | off |")
    print("|---|---|---|")
    for kind in kinds:
        print(f"| {kind} | {by_kind['on'].get(kind, 0)} | {by_kind['off'].get(kind, 0)} |")

    loads = [on[s].get("load", "") for s in shared if on[s].get("load")]
    if loads:
        print(f"- the load while the on arm ran, first to last: {loads[0]!r} ... {loads[-1]!r}")


def main() -> int:
    directory = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent
    situations_analysis(directory)
    arms_analysis(directory)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
