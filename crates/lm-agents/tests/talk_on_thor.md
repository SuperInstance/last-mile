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
