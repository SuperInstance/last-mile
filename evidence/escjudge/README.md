# evidence/escjudge: does escalation earn its keep (D75's open question)

The pre-registered measurement that settles D75's open question: whether
the move gate's second opinion (clef-flash, asked when the d1-2B answers
below 0.50) agrees with a neutral judge more often than the d1-2B's own
answer, and what turning it off does to the game. The registration is
`crates/lm-agents/tests/talk_on_thor.md`, "Does escalation earn its keep";
the collector is `crates/lm-agents/tests/escalation_keep.rs`.

## The files

- `situations.jsonl` — one line per **distinct** escalated move situation
  (facts + move question) from real 3000-tick rounds on the Thor's d1-2B
  (seeds 1-40, 2026-10-30, load 13-131 on 14 cores while the rounds ran).
  Each line: the seed and tick it landed on, the d1-2B's confidence, the
  in-round escalation record (outcome, both parties' choices, latency),
  and the three offline asks — the d1-2B itself, clef-flash, and the
  d1-3B judge — each asked the same single question alone, with choice,
  confidence and round trip.
- `collect_rounds.jsonl` — one line per collection round (tags, sight
  ticks, scores, decisions, refusals, escalations, load).
- `arms_on.jsonl` / `arms_off.jsonl` — one line per seed per arm
  (`escalate_gates = ["move"]` vs `[]`), 16 seeds x 3000 ticks, paired
  per seed.
- `analysis.py` — the analysis of all of the above: the 2x2 and the exact
  two-sided McNemar test (integer arithmetic), D78's exact one-sided sign
  test over the arms' per-seed deltas, the latency distributions, the
  refusal kinds. `python3 analysis.py` prints the whole report, and
  `analysis-output.txt` is its final run over the files above.

## The verdicts

- The McNemar test on 282 judged situations: b = 27 (the d1-2B wrong, the
  escalated answer right), c = 35 (the reverse), exact two-sided
  p = 0.3741 — the escalated answer does **not** agree with the judge
  significantly more often than the d1-2B's own answer; the point
  estimate favours the d1-2B. The registered rule says turn the
  escalation off.
- The arms and the sign tests: see `analysis.py`'s output and
  `talk_on_thor.md`'s "The measurement" section.
