# The talk gates on the real Thor

The agents decide fine on the Thor and say almost nothing: 57-85 decisions per
3000-tick round, 0-1 lines. The stub says 480 lines in the same round. This is
the measurement that says why, and it is the measurement the fix is read
against.

## The command

```text
cargo test --release -p lm-agents --test talk_on_thor -- --ignored --nocapture
```

## What is measured

The round is the shipped one, played the way the game plays it: the host
loop, the agents, the least-use-first queue, the sequencer, the recorder. The
only thing that is not the shipped code is the model behind the tier: the
Thor's own **d1-2B**, on the LAN. Three seeds, 3000 ticks each, at the round's
own rate (30 Hz), so an answer that took four slots to be served lands four
slots late.

The watcher behind the tier records, for **every ask that comes back**:

- the five presence nouls (`gesture_present`, `attention_present`,
  `intention_present`, `intent_present`, `subject_present`);
- the confidence of every gate that has one (the move gate, the five talk
  gates and the intention score);
- the lowest of those confidences — the number the old pack-level rule folded
  the whole pack into;
- the round trip the model reported.

The watcher's floor is zero, so every gate's own number is seen; the rule that
would have been applied to them is derived from them and reported below.

The situations are many and the answers are not: the Thor answers the same
pack over the same facts the same way, so the distribution is over the
*distinct situations*, and every distinct situation counts once.

## Before the fix

Three rounds, 3000 ticks each, seeds 1-3:

| seed | lines | decisions | refusals | asks that came back |
|---|---|---|---|---|
| 1 | 0 | 43 | 15 | 43 |
| 2 | 0 | 0 | 16 | 0 |
| 3 | 0 | 0 | 16 | 0 |

The brief's own baseline is 57-85 decisions and 0-1 lines per round. The
numbers here are lower in rounds 2 and 3 because the Thor was slower on the
day (see the first point below: the pack did not fit the deadline, and a box
that is slower still fails every ask). The shape is the same either way: the
round ends early, and no line is ever spoken.

Rounds 2 and 3 made no decision at all, and an instrumented re-run showed why:
of 49 asks, **24 came back as "the model did not answer before the deadline"**
and only 1 came back at all (825 ms).

### The presence nouls (43 situations)

| gate | min | p10 | p50 | p90 | max | below the 0.05 floor |
|---|---|---|---|---|---|---|
| gesture_present | 0.2413 | 0.2506 | 0.2975 | 0.3462 | 0.3822 | 0/43 |
| attention_present | 0.1859 | 0.2110 | 0.7023 | 0.8355 | 0.8610 | 0/43 |
| **intention_present** | **0.0157** | **0.0175** | **0.0199** | **0.0396** | **0.0476** | **43/43** |
| intent_present | 0.2718 | 0.2896 | 0.3391 | 0.3771 | 0.3978 | 0/43 |
| subject_present | 0.0700 | 0.0765 | 0.2502 | 0.3173 | 0.3714 | 0/43 |

**What blocked the line: `intention_present`, 43 of 43 situations.**

### The confidences (43 situations)

| gate | min | p10 | p50 | p90 | max |
|---|---|---|---|---|---|
| move | 0.7956 | 0.8293 | 0.8805 | 0.9264 | 0.9415 |
| gesture | 0.7283 | 0.7945 | 0.8611 | 0.8953 | 0.9019 |
| **attention** | **0.2912** | **0.3133** | **0.4264** | **0.5625** | **0.6435** |
| intention | 0.5200 | 0.5815 | 0.6974 | 0.8039 | 0.8175 |
| intent | 0.6170 | 0.6404 | 0.7322 | 0.7771 | 0.7921 |
| subject | 0.6266 | 0.6507 | 0.7079 | 0.8372 | 0.8650 |

The pack-level fold — the lowest confidence in the pack — is the attention
gate's, every time: ten options, so unsure by nature.

| the old pack-level rule at a floor of | packs it would refuse |
|---|---|
| 0.15 (what the soak pins) | 0/43 (0%) |
| 0.55 | 35/43 (81%) |
| 0.60 (what the game shipped) | 39/43 (91%) |

### What the numbers say

1. **The pack did not fit the deadline, so the round had nothing to speak
   with.** The eleven-question pack costs the d1-2B 600-1400 ms. The shipped
   deadline was 1200 ms and the shipped think budget was 30 ticks (1 s). A
   request that times out teaches the tier the cost of the timeout (1200 ms →
   36 ticks), the tier then no longer fits the 30-tick budget, `pick_tier`
   returns nothing, and the minds stop asking: 16 asks in the whole round and
   then silence for the other ninety seconds. That is why rounds 2 and 3 made
   no decision at all.
2. **The intention presence gate read as "no".** It asked "Is this moment
   worth an urgent tone?" — a question about *urgency*, read by the gate as
   "is this even here?". The model answered it truthfully: 0.020 in the
   median, and 0.62 in a fight. A truthful "no" silenced every line in every
   quiet moment, and the other four presence gates (0.19-0.86) never blocked
   anything: the agents' only voice was one gate, asking the wrong question.
3. **The pack-level confidence rule vetoed everything.** The fold is the
   weakest gate's confidence, so at the shipped floor of 0.60 the rule
   refused 91% of the packs — the game, as opposed to the soak, which pins
   0.15, had no decisions at all. One unsure gate took every other gate's
   decision with it.

## The fix

| what | where | why |
|---|---|---|
| The presence gate of the intention gate asks for the moment's news, not for its urgency | `assets/harness/default/gates.toml` | The question the model read as "no" was a question about urgency |
| The confidence rule is per gate | `crates/lm-agents/src/sysone.rs`, `crates/lm-agents/src/mind.rs` | An unsure talk gate silences the line and leaves the move alone; an unsure move gate refuses the move and leaves the line alone |
| The floor the d1-2B is held to: 0.30 | `crates/lm-agents/src/config.rs` | The model's own calibration: the move gate answers 0.80-0.98, the attention gate 0.27-0.66 |
| The deadline: 2400 ms | `crates/lm-agents/src/config.rs` | The pack costs 600-1400 ms; the deadline has to clear the slow case |
| The think budgets: 45 and 60 ticks | `assets/harness/default/policy.toml` | A thought is 20-42 ticks; at 15 and 30 no tier fitted and no thought was ever made |

No question was added: the pack is still the eleven questions it was, and the
load on the Thor did not grow either — a mind holds one thought at a time, so
a round asks for what the box can answer (83 asks in a round of a hundred
seconds) and the cost of the round is the cost of those answers.

## After the fix

Three rounds, 3000 ticks each, seeds 1-3:

| seed | lines | decisions | refusals | asks sent | asks that came back |
|---|---|---|---|---|---|
| 1 | 74 | 150 | 8 | 83 | 83 |
| 2 | 55 | 118 | 8 | 71 | 71 |
| 3 | 62 | 124 | 8 | 70 | 70 |

The soak itself — the shipped test, the stub half and the Thor half — says the
same thing. Its CSV line is `backend, seed, utterances, airtime_ticks,
utterances_per_1k_ticks, decisions, refusals, scores`:

```text
stub,1,480,4800,160,8483,0,[1800, 19]
stub,2,480,4800,160,8021,467,[1800, 7]
stub,3,480,4800,160,8485,0,[1801, 5]
thor-d1-2b,1,64,1280,21,128,8,[1690, 1690]
thor-d1-2b,2,56,1120,18,118,8,[1702, 1702]
thor-d1-2b,3,76,1520,25,154,8,[1721, 1721]
```

The Thor's half of the table is the acceptance: **64, 56 and 76 lines per
round** (seeds 1, 2 and 3) against 0-1 before, and **128, 118 and 154
decisions** against 57-85 before. The stub's half is unchanged, to the line.

The live lane (`scripts/live.sh`) plays the same soak a second time, and says
the same thing:

```text
thor-d1-2b,1,77,1540,25,154,8,[1690, 1690]
thor-d1-2b,2,69,1380,23,144,8,[1702, 1702]
thor-d1-2b,3,73,1460,24,150,8,[1721, 1721]
```

### The presence nouls (192 situations)

| gate | min | p10 | p50 | p90 | max | below the 0.05 floor |
|---|---|---|---|---|---|---|
| gesture_present | 0.2373 | 0.2625 | 0.3151 | 0.3745 | 0.4115 | 0/192 |
| attention_present | 0.1887 | 0.5799 | 0.7324 | 0.8277 | 0.8620 | 0/192 |
| intention_present | 0.0451 | 0.0622 | 0.1260 | 0.2476 | 0.5079 | 8/192 |
| intent_present | 0.2526 | 0.2882 | 0.3427 | 0.4168 | 0.4929 | 0/192 |
| subject_present | 0.0704 | 0.1780 | 0.2597 | 0.3363 | 0.4555 | 0/192 |

**What blocked the line: `intention_present` in 8 of 192 situations, nothing in
184.** The gate now discriminates the moment (0.51 in a fight, 0.13 in the
median) instead of vetoing every line.

### The confidences (192 situations)

| gate | min | p10 | p50 | p90 | max |
|---|---|---|---|---|---|
| move | 0.5816 | 0.8283 | 0.8859 | 0.9413 | 0.9790 |
| gesture | 0.6456 | 0.7445 | 0.8257 | 0.8836 | 0.9040 |
| attention | 0.2725 | 0.3418 | 0.4246 | 0.5605 | 0.6623 |
| intention | 0.4949 | 0.5394 | 0.7023 | 0.8883 | 0.9618 |
| intent | 0.5979 | 0.6527 | 0.7183 | 0.7601 | 0.7976 |
| subject | 0.3327 | 0.5830 | 0.7079 | 0.8512 | 0.9081 |

### The rule that is shipped now, per gate

| floor | gates dropped | lines silenced | moves refused |
|---|---|---|---|
| 0.15 (what the soak pins) | none | 0 | 0 |
| 0.30 (what the game ships) | `attention`, 6 of 192 | 6 | **0** |

That is the rule the brief asks for, in the numbers: the unsure gate is the
addressee, it takes the line with it and leaves the move alone.

### The round trip

600-1400 ms across the runs; 674 ms at the minimum, 950 ms at the median and
1082 ms at the maximum over the 200 answers of this run. The pack fits the
2400 ms deadline with room to spare.

## What was not changed

- **No question was added.** The pack is still eleven: the move gate, the
  five talk gates and their five presence gates.
- **The stub half of the soak is unchanged**: 480 lines, 4800 ticks of
  airtime and 8032-8486 decisions per round, before and after.
- **The presence floor is unchanged** (0.05, "not here at all"). The gates
  that were mis-worded were re-worded, not re-weighted.

## What this measurement does not say

- **It is not a score.** The Thor's model answers the same pack the same way
  every time, so the three rounds are three samples of the same behaviour,
  not three games. The score column of the soak is the game's, not this
  measurement's.
- **It is one box's speed.** The pack costs 600 ms on an idle Thor and
  1400 ms on a busy one. The deadline and the think budgets are set from the
  slow case; a faster box answers more, and the line rate follows the answer
  rate.
- **The gate wordings are this model's.** "Is there something for your squad
  to hear about this moment?" reads 0.15 in a quiet moment, 0.89 in a fight
  and 0.14 when the agent is alone *on d1-2B*. Another model reads it
  differently, and the presence gates are the harness's to re-word when it
  does.

---

# The split pack and the second opinion (D70, D71)

The pack is split and the unsure gates get a second opinion. This is the
measurement of both, on the real Thor, with the same harness and the same
three seeds as the section above.

## What changed

| what | where | why |
|---|---|---|
| The pack is split: the **move pack** (the move gate) and the **talk pack** (the five talk gates and their five presence gates) | `crates/lm-agents/src/gates.rs` | The move question alone costs 82 ms on the d1-2B and the ten talk gates cost the other 80% of a thought (D70) |
| The move pack is asked on **every think**; the talk pack only when there is something to say | `crates/lm-agents/src/mind.rs` | The decision that moves the game is cheap now; the line is not |
| The four talk triggers and the timer period are harness parameters | `assets/harness/default/policy.toml` | A harness decides when its agents speak: a change in sight, an order heard, a threat, a slow timer |
| Two requests, two worker threads | `crates/lm-agents/src/mind.rs` | The talk pack never waits behind the move pack, and the move pack never waits behind the talk pack |
| The tier's latency is measured **per pack** | `crates/lm-agents/src/mind.rs` | One question and ten questions are not the same call; one estimator over both would price the talk pack as ten times the move pack |
| An unsure gate is asked of clef-flash **alone** | `crates/lm-agents/src/sysone.rs` | D71: one question in one call, so the second model's batch always fits |
| The escalation is logged (gate, both confidences, latency) and counted next to `refused` | `crates/lm-agents/src/mind.rs`, `crates/lm-host/src/hud_plugin.rs` | The second opinion is a cost, and a cost that is not measured is not managed |
| The deadline: 4000 ms; the think budgets: 105 and 105 ticks | `crates/lm-agents/src/config.rs`, `assets/harness/default/policy.toml` | The talk pack plus its second opinion costs 2-3 s; at 2400 ms and 60 ticks half of the escalations died on the way and the talk pack never fitted a tier |

## Before and after, on the Thor

Three rounds, 3000 ticks each, seeds 1-3, the same harness:

| seed | lines before | lines after | decisions before | decisions after | escalations |
|---|---|---|---|---|---|
| 1 | 71 | 55 | 142 | 741 | 52 |
| 2 | 61 | 50 | 130 | 733 | 50 |
| 3 | 68 | 58 | 142 | 766 | 57 |
| **total** | **200** | **163** | **414** | **2240** | **159** |

**The move decisions per round went from 71 to 692** (the decisions less the
lines), which is 9.7 times the 118-154 the brief measured before, against a
target of twice. The lines are 50-58 per round, inside the 15-120 the brief
asks for, against 61-68 before.

| metric | before | after |
|---|---|---|
| decisions per round | 130-142 | **733-766** |
| move decisions per round (the lines less) | 71-76 | **692-708** |
| utterances per round | 61-71 | 50-58 |
| questions asked per round | 77-79 | 346-366 |
| thoughts per second per agent | 0.10 | 0.44 |
| decisions per second per agent | 0.17 | 0.93 |
| move pack round trip, p50 / p95 | 473-1400 ms (the whole pack) | **164 ms / 262 ms** |
| talk pack round trip, p50 / p95 | — | 770 ms / 1586 ms |

The move pack's round trip is the number that matters: 164 ms at the median
and 262 ms at p95, against the 473-1400 ms the one big pack cost. The
decision that moves the game is now nearly ten times faster than the line
that goes with it.

## The live lane says the same thing

`scripts/live.sh` plays the shipped soak against the same Thor. Its CSV line
is `backend, seed, utterances, airtime_ticks, utterances_per_1k_ticks,
decisions, refusals, scores`:

```text
stub,1,467,4670,155,7831,641,[1800, 19]
stub,2,480,4800,160,6266,2302,[86, 1801]
stub,3,480,4800,160,8480,0,[1801, 5]
thor-d1-2b,1,76,1520,25,803,16,[1690, 1690]
thor-d1-2b,2,77,1540,25,782,16,[1702, 1702]
thor-d1-2b,3,59,1180,19,741,18,[1721, 1721]
```

The Thor's half: **76, 77 and 59 lines** per round and **803, 782 and 741
decisions**, against 64, 56, 76 lines and 128, 118, 154 decisions before.

**The stub's half moved too, and it should have:** the move pack is asked on
every think, so the stub's minds decide every second as well, and the stub's
rounds now end in different places (86-1801 instead of 5-19 on the score
column). The stub is the model that answers at once, so it is the one that
shows what the split does to the game itself.

## The escalation (D71)

| | |
|---|---|
| escalations | 159 |
| taken | 158 |
| the second model did not answer in time | 1 |
| the gate: `attention` / `subject` / `intention` / `move` | 103 / 51 / 3 / 2 |
| the added latency | 593 ms at the median, 1300 ms at p95 |
| the first model's confidence on the escalated gate | 0.31-0.50, 0.39 at the median |
| the second model's confidence | 0.11-0.69, 0.40 at the median |

**The rate: 9.6% of the gates answered, 16.1% of the asks, 7.1% of the
decisions.** The brief's target was "near 4%", and that is where this misses.
D71's 4% was measured on the **action** gate in the decision-daw scenes, and
on the Thor in the game the move gate is escalated 2 times in 816 (0.25%):
the d1-2B is almost always sure where the agent walks. The rate in this
measurement is carried by the two ten-option talk gates — `attention`
(thirteen ways to address somebody) and `subject` (which beacon the line is
about) — and the d1-2B is genuinely unsure about those: the attention gate
answers 0.40 at the median and the subject gate 0.75. Escalating them is
what the rule says to do ("a single question below 0.5"), and it is what
makes the lines worth saying, but it costs 593 ms a time and it is not the
4% D71 measured. A harness that wants the action's rate and nothing else
has the threshold to change it: at 0.55 the attention gate stops being
escalated.

**The added latency is 593 ms, not the +25 ms D71 measured.** D71's number
came from the decision-daw setup, where the second opinion was a warm call
on a small prompt. On the Thor, clef-flash answers one question in about
570 ms of its own, and that is what the escalation adds to a pack that was
already 180 ms.

## The 0.5 and the 0.30 together

D72's floor (0.30) and D71's threshold (0.50) are two different questions,
and they answer two different ones:

- **0.30 is the model's own admission bar.** The d1-2B's answer below 0.30
  makes no decision: the gate is unsure, and an unsure gate takes the line
  with it (a talk gate) or leaves the move alone (a move gate).
- **0.50 is when the cheap model stops being trusted alone.** A gate the
  d1-2B answers under 0.50 is not acted on without a second opinion.

So a gate in [0.30, 0.50) is heard but not acted on: the second model's
answer is taken, and if the second opinion fails the gate stays unsure
(the first model's 0.4 is not enough on its own). A gate under 0.30 was
going to be dropped anyway, and the second opinion is its rescue: clef-flash
answered it 0.40 at the median, so it decides.

**The second model has its own floor.** clef-flash's confidence scale runs
about half of the d1-2B's on the same gates (the attention gate reads 0.13
against the d1-2B's 0.33), so the d1-2B's 0.30 has no counterpart on it:
`thor-clef-flash`'s floor is 0.05, and a second opinion that answered is
taken. The gate is unsure only when the second model is down, slow or
shrugs — one such case in 159.

## What this measurement does not say

- **It is not a score.** The Thor's model answers the same pack the same way
  every time, so the three rounds are three samples of the same behaviour,
  not three games.
- **The move decisions are counted with the lines.** The "move decisions" row
  is the decisions less the lines, so it is the lanes the agents' answers
  became.
- **The escalation rate is the game's, not D71's.** D71's 4% is the action
  gate's rate in the decision-daw scenes; see above for the two numbers on
  the Thor.

---

# Tuning the second opinion (D71) and the d1-3B's floor

The escalation above ran at 9.6% of the gates — not the 4% D71 measured —
and it was carried by the two ten-option talk gates. This is the
measurement that tunes it, the same harness and the same three seeds
(1-3, 3000 ticks each, the d1-2B behind the tier), in three arms:

- **all gates**: every gate below `escalate_below` (0.50) is asked of
  clef-flash, the escalation as D71 shipped it;
- **move only**: the move gate alone;
- **off**: no gate.

Every gate the "all gates" arm escalated was then judged offline, and the
d1-3B was put behind the tier once with its floor at zero, so its own
confidence floor could be read the way the d1-2B's 0.30 was.

## The command

```text
cargo test --release -p lm-agents --test talk_on_thor -- --ignored --nocapture
```

## The arms

Three rounds per arm, seeds 1-3:

| arm | lines per seed | total lines | total decisions | refusals | asks answered | escalations |
|---|---|---|---|---|---|---|
| all gates | 41 / 29 / 28 | **98** | 1808 | 53 | 681 | **100** |
| move only | 40 / 51 / 51 | 142 | 1940 | 56 | 732 | 3 |
| off | 49 / 60 / 61 | **170** | **1992** | 50 | 760 | 0 |

| metric | all gates | move only | off |
|---|---|---|---|
| the escalation rate, per gates answered | 9.2% | 0.2% | — |
| the escalation rate, per decision | 5.5% | 0.2% | — |
| escalations by gate | attention 73, subject 24, intention 2, move 1 | move 3 | — |
| escalations taken / timed out | 94 / 6 | 3 / 0 | — |
| move pack round trip, p50 / p95 / max | 240 / 409 / 4086 ms | 243 / 436 / 750 ms | 253 / 445 / 502 ms |
| talk pack round trip, p50 / p95 / max | 1119 / 2307 / 3703 ms | 1606 / 2348 / 3710 ms | 1164 / 2419 / 3503 ms |
| the added latency, p50 / p95 / max | 986 / 1587 / 1724 ms | 1383-1423 ms (3 of them) | — |
| asks the Thor timed out | 5 | 8 | 2 |
| the talk pack's answers | 104 | 144 | 171 |

**Escalating the talk gates does not pay for itself: it costs the round
lines and decisions.** The "all gates" arm spoke 98 lines against the
"off" arm's 170 and made 1808 decisions against 1992. The mechanism is the
talk lane: a second opinion adds 986 ms at the median to a talk pack that
already costs 1.1 s, the lane answers only 104 asks against the "off"
arm's 171, and 6 of the 100 second opinions died on the deadline and took
their lines with them. The "move only" arm is within noise of "off"
(142 against 170 lines, 1940 against 1992 decisions) at three second
opinions in three rounds.

No arm spoke an Order-intent line in any round (the orders column is 0/0
everywhere), so uptake could not tell the arms apart; the line and decision
counts are the round-level evidence, and they need no judge.

## The second opinion, answer by answer

For every gate the "all gates" arm escalated, the escalation record now
carries both models' own choices (the option each distribution favoured),
and every distinct escalated question — 64 of the 100 — was asked of a
third model, the Thor's **d1-3B**, alone, over the same facts, after the
rounds were over.

Why the d1-3B as the judge: it is neither party in the dispute, it is
resident on the same Thor, it is deterministic, and asking it the same
single question over the same facts is exactly the call clef-flash was
asked, so the only difference between the two parties is the model. The
caveat is D62's: the d1-3B measured the weaker decider on the *move* gate
(0.84 against the d1-2B's 0.96), so its agreement is evidence, not ground
truth — and the round-level totals above, which need no judge, point the
same way.

| gate | situations judged | the two models differ | the d1-2B agrees with the judge | clef-flash agrees | neither agrees |
|---|---|---|---|---|---|
| attention | 54 | 33 | **35** | 12 | 17 |
| subject | 7 | 7 | 0 | **6** | 1 |
| intention | 2 | 2 | 0 | 0 | 2 |
| move | 1 | 0 | 0 | 0 | 1 |

- **The second opinion is not a rubber stamp: the two models differ on 40
  of the 64 distinct situations**, and where they differ the line differs —
  the attention gate is the line's addressee and the subject gate is what
  the line is about.
- **On attention — 73% of the escalations — the second opinion replaced
  the better answer.** The judge agreed with the d1-2B's own choice 35 of
  54 times and with clef-flash's 12 of 54. The d1-2B is unsure on the
  ten-option addressee gate (0.40 at the median), but its unsure answer is
  still the better one; the second second of latency buys a worse addressee.
- **On subject the picture reverses** — clef-flash 6 of 7 against the
  d1-2B's 0 of 7 — but it is 7 situations carrying 24 escalations, and the
  latency it costs lands on the same talk lane that lost 72 lines in the
  "all gates" arm. A harness that wants it can name it; that is what the
  parameter is for.

## What was changed

| what | where | why |
|---|---|---|
| `escalate_gates` in `policy.toml`: a list of gate ids; absent is every gate, `[]` is off | `assets/harness/default/policy.toml`, `crates/lm-agents/src/mind.rs`, `crates/lm-agents/src/sysone.rs`, `crates/lm-host/src/main.rs` | Which gates are worth a second opinion is a harness question (the gates are the harness's); the threshold stays with the model's calibration (`AgentsConfig::escalate_below`) |
| The default: `escalate_gates = ["move"]` | `assets/harness/default/policy.toml` | The talk gates' second opinion did not pay (98 against 170 lines, 1808 against 1992 decisions); the move gate is where D71's accuracy was measured (0.98 against the d1-2B's own 0.96) and it costs 3 second opinions in 3 rounds |
| The escalation record carries both models' own choices | `crates/lm-agents/src/sysone.rs` (`Escalation::primary_choice`/`second_choice`) | An escalation that never changes the answer buys latency and nothing else; the two choices are the difference |
| The deadline stays 4000 ms | `crates/lm-agents/src/config.rs` | Without the talk-gate escalation the deadline clears the talk pack's *own* slow case: p95 2.3-2.4 s and a 3.5-3.7 s tail on a busy box; the move lane's worst case (pack plus its rare escalation) is 445 + 1423 = 2.2 s |
| The think budgets stay 105 / 105 ticks | `assets/harness/default/policy.toml` | The talk pack is the slow case (p95 ≈ 72 ticks, max ≈ 112); 105 clears the p95 with room, and the rare 3.7 s tail misses it and plays the reflex (D26) — cheaper than a budget that never lets the talk pack think |
| The d1-3B's floor: 0.55 → 0.15 | `crates/lm-agents/src/config.rs` | Its own calibration, below |

The escalation rate the harness now ships is **0.2% of the gates answered,
about one second opinion a round** — D71's 4% was the *action* gate's rate
in the decision-daw scenes, and the Thor's move gate (0.25% of its answers)
is in that family; the 9.6% was the talk gates', and they do not want it.

## The d1-3B's floor

The d1-3B played the same three rounds behind the tier with its floor at
zero (two runs, 393 and 415 distinct situations; the shapes agree). With
the floor at zero it spoke 50-75 lines and made 535-581 decisions a round —
the model works behind the tier; it was the floor that killed it.

| gate | min | p10 | p50 | p90 | max |
|---|---|---|---|---|---|
| move | 0.2815 | 0.8623 | 0.8874 | 0.9135 | 0.9609 |
| gesture | 0.2165 | 0.2692 | 0.3099 | 0.3657 | 0.4150 |
| attention | 0.2123 | 0.3044 | 0.4681 | 0.5701 | 0.6055 |
| **intention** | **0.0000** | **0.0000** | **0.2000** | **0.2953** | **0.3674** |
| intent | 0.1491 | 0.2099 | 0.2536 | 0.3278 | 0.3814 |
| subject | 0.2291 | 0.4124 | 0.5285 | 0.6234 | 0.6743 |

The presence nouls are all above the 0.05 floor (intention_present reads
0.11-0.24, the lowest), so the wordings hold on this model; it is the
confidences that run lower than the d1-2B's.

The per-gate rule over the 82 talk situations of the second run:

| floor | gates dropped | situations silent | without the move |
|---|---|---|---|
| 0.05 | intention 14 | 14 | 0 |
| 0.10 | intention 27 | 27 | 0 |
| **0.15** | **intention 34, intent 1** | **35** | **0** |
| 0.20 | intention 42, intent 5 | 45 | 0 |
| 0.30 | every talk gate (intention 74, intent 68, gesture 35…) | 82 | 1 |
| 0.55 (what shipped) | every talk gate | 82 | 0 |

**The floor is 0.15, read the way the d1-2B's 0.30 was**: the d1-2B's floor
sits at the bottom of its weakest gate's range (the attention gate's
0.29-0.64), and the d1-3B's gates bottom out at 0.15-0.23 (the intent
gate's minimum is 0.149). The one gate that answers below the floor is the
intention score, which the d1-3B answers near-flat — 0.00-0.37 over three
levels, barely over the 0.33 of no opinion at all — so those 34 of 82
situations are honestly unsure and the line is silent. No move is ever
refused (the move gate answers 0.28-0.96). The 0.55 this replaces was the
old failure in miniature: it silenced every line of every talk round, 82 of
82, exactly what the 0.60 pack-level fold did to the d1-2B.

## What this measurement does not say

- **It is not a score.** The Thor's models answer the same pack the same
  way, so the three seeds are samples of the same behaviour, not three
  games.
- **It is one busy day's speed.** The escalation cost 986 ms at the median
  against the 570-593 ms of the idle days the previous section and D71
  measured; the arms are comparable with each other (same day), and the
  move-only default leaves the deadline and the budgets clear of the
  busy-day tail either way.
- **The judge is a third model, not ground truth.** D62 measured the d1-3B
  the weaker decider on the move gate; its agreement on the talk gates is
  evidence — and the round-level totals, which need no judge, agree with
  it.
- **The subject-gate result is 7 situations deep.** clef-flash won it 6-0,
  but 7 distinct situations do not carry a default; they carry a comment,
  and the parameter is there the day a harness wants it.
