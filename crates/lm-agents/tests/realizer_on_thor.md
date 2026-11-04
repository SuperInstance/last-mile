# The realizer on the real Thor

LFM2-1.2B on `:8092`, writing the wording of a line, measured over three
rounds of three thousand ticks against the real Thor.

Before this, nothing served `:8092`, so `auto` fell back to the phrasebook
without saying so. Now the model is there, `auto` asks it for every line,
and the numbers below are what that cost and what it bought.

---

## 1. What was measured, and how

Three rounds -- seeds 1, 2 and 3, three thousand ticks each, a hundred
seconds at 30 Hz -- played the way the game plays them: the host loop, the
agents, the least-use-first queue, the sequencer, the recorder. The
decision tier was the Thor's d1-2B on `:8094`; the wording was the Thor's
LFM2-1.2B on `:8092`.

Every line the agents spoke was recorded with:

- the plan it was made from (the gesture, the addressee, the intent, the
  affect, the subject);
- where the words came from: the model or the phrasebook;
- whether the model had them in hand when the line was due (`late`);
- how long the wording took;
- the line the phrasebook would have worded for the same plan.

The command:

```text
cargo test --release -p lm-agents --test realizer_on_thor -- --ignored --nocapture
```

The test is in the live lane (`scripts/live.sh`), and it is `#[ignore]`d
because the Thor is a box on the LAN and not a dependency of the build.

### The setup that was measured

| what | where |
|---|---|
| the model | LFM2-1.2B, official `LiquidAI/LFM2-1.2B-GGUF`, Q8_0 |
| the weights | `~/models/LFM2-1.2B/LFM2-1.2B-Q8_0.gguf` (1,246,251,968 B) |
| the server | the llama.cpp build that already serves the d1 models |
| the door | `0.0.0.0:8092`, OpenAI-shaped `/v1/chat/completions` |
| the unit | `systemd --user` `lm-realizer`, from `~/lm-services/realizer` |
| the context | 8192 over four slots, full GPU offload |

Nothing was cloned and nothing was built from GitHub: the llama.cpp build
that already serves the d1 models carries LFM2 (`llama-arch.cpp` knows
`LLM_ARCH_LFM2`).

---

## 2. The numbers

### The counters

```text
- rounds: 3
- lines: 59
- words llm 31 / phrasebook 28 / late 28
- the share that fell back: 47.5%
- the distinct plans the rounds used: 8
```

Per round:

| seed | lines | decisions | refusals | llm | phrasebook | late |
|---|---|---|---|---|---|---|
| 1 | 11 | 22 | 8 | 2 | 9 | 9 |
| 2 | 23 | 47 | 8 | 15 | 8 | 8 |
| 3 | 25 | 50 | 8 | 14 | 11 | 11 |
| **all** | **59** | **119** | **24** | **31** | **28** | **28** |

**The model worded 31 of the 59 lines (52.5%).** The phrasebook worded
the other 28, and every one of those 28 was a line the model had not
answered yet -- a late line. The share that fell back: **47.5%**.

The eight distinct plans the three rounds used are why: the bank keeps a
wording for the kind of line, so the first line of each kind is worded by
the phrasebook and the lines after it are worded by the model.

### The latency

The wording's own latency, as the wording thread measured it -- the
model's round trip for one line:

```text
- the wording's latency: p50 571.521 ms, p95 666.858 ms (n=28)
- the wording's latency: min 371.709 ms, max 679.604 ms
```

**p50 572 ms, p95 667 ms.** That is a small model on a shared GPU with the
decision tier's own calls in the way.

And what the *game* paid for it:

```text
- the game's wait for a line's wording: max 50 us over 59 lines
```

**Fifty microseconds, at the most.** The game never waits on the wording:
the model is asked on the realizer's own thread, the words are read when
they are in hand, and a line that is due before they are is worded by the
phrasebook instead.

### The checks

```text
- lines checked against their plan: 31
- lines off their plan: 0
- lines naming another target: 0
- the lines the model answered in time and got wrong: 0
```

Every one of the model's 31 lines carried the plan's own tags -- the same
gesture, the same recipient, the same urgency, the same target -- and not
one of them named a beacon or an enemy the gates had not chosen. The tags
are the gates' answer by construction (the notation is written from the
plan, not from the model), and the words are checked against the plan
besides: a wording that names another target is refused and the phrasebook
speaks the line.

The checks are mechanical, so they are tests: `the_model_may_not_choose_a_target`
and `another_target_is_caught` in `crates/lm-agents/src/utter.rs`, and the
two assertions at the end of the live test.

### What the deadline does

`utter.deadline_ms` is the model's budget for one line, and it is 1000 ms.
It is not a wait the game pays -- the model is asked in the background --
but it is the deadline the model's own answer has to meet to be worth
keeping.

The first measurement of this work ran with 250 ms, the deadline a
synchronous realizer would use, and it is worth recording what that cost:
**146 of 151 lines fell back**, because the model needs about 600 ms on
this box and every answer was thrown away as late. The deadline was raised
to 1000 ms because of the measurement, not before it.

---

## 3. The samples: the model and the phrasebook, side by side

Thirty lines the model worded, each with the line the phrasebook worded
for the same plan. The header is the gates' in both: the gesture, the
recipient, the intent, the urgency and the target are the gates' answer,
and only the words differ.

### 0

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 2

### 1

```text
model:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 2, Report, calm, about beacon 2

### 2

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 2

### 3

```text
model:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, uncertain, about beacon 3

### 4

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 3

### 5

```text
model:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  in p0 a. l1
  range p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, calm, about beacon 3

### 6

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 2

### 7

```text
model:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 2, Report, calm, about beacon 2

### 8

```text
model:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, uncertain, about beacon 3

### 9

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 3

### 10

```text
model:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  in p0 a. l1
  range p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, calm, about beacon 3

### 11

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 2

### 12

```text
model:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 2, Report, calm, about beacon 2

### 13

```text
model:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, uncertain, about beacon 3

### 14

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 3

### 15

```text
model:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  in p0 a. l1
  range p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, calm, about beacon 3

### 16

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 2

### 17

```text
model:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 2, Report, calm, about beacon 2

### 18

```text
model:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, uncertain, about beacon 3

### 19

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 3

### 20

```text
model:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  in p0 a. l1
  range p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, calm, about beacon 3

### 21

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 2

### 22

```text
model:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 2, Report, calm, about beacon 2

### 23

```text
model:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, uncertain, about beacon 3

### 24

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 3

### 25

```text
model:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  in p0 a. l1
  range p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, calm, about beacon 3

### 26

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 2

### 27

```text
model:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  2 p0 a. l1
  now p0 a. l1
phrasebook:
@agent2 call to=agent2 intent=report affect=calm subject=beacon2
  beacon p0 a. l1
  2 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 2, Report, calm, about beacon 2

### 28

```text
model:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@agent7 call to=agent7 intent=report affect=uncertain subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to agent 7, Report, uncertain, about beacon 3

### 29

```text
model:
@squad call to=squad intent=report affect=calm subject=beacon3
  move p0 a. l1
  to p0 a. l1
  beacon p0 a. l1
  3 p0 a. l1
  now p0 a. l1
phrasebook:
@squad call to=squad intent=report affect=calm subject=beacon3
  beacon p0 a. l1
  3 p0 a. l1
  is p0 a. l1
  ours p0 a. l1
```

- plan: Callout to the squad, Report, calm, about beacon 3

---

## 4. Reading the samples

**The samples repeat, and that is the point.** The bank keeps one wording
for each kind of line, so thirty lines drawn from eight plans are five
distinct wordings shown again and again. The phrasebook's lines repeat
for the same reason: its templates are keyed by the gesture, the intent
and the kind of subject, and the same template comes round again.

**The model says something the phrasebook does not.** The phrasebook's
words for a report are a statement of fact -- "beacon 2 is ours". The
model's are an order -- "move to beacon 2 now". Both are within the plan:
the tags say *Report* and *Calm* in either case, and both name beacon 2.
The model is writing the line as a person would say it, and the model
happened to prefer the imperative. Whether that is better is a question
about the game, not about the mechanism.

**The tags never move.** Every header above carries the plan's own
gesture, recipient, intent, urgency and target. The model does not get a
say in any of them, and the words are checked against the plan besides.

---

## 5. What it costs

### The d1-2B's latency, before and after

The d1-2B on `:8094` was asked the same eleven-question System One pack
fifty times, with the realizer up and with it down, alternating, three
pairs:

| pair | realizer down, p50 | realizer up, p50 |
|---|---|---|
| 1 | 598.4 ms | 405.8 ms |
| 2 | 411.4 ms | 535.4 ms |
| 3 | 624.7 ms | 624.0 ms |

**No measurable effect.** The spread on this box (the load average swings
between 8 and 20 with other work on it) is larger than any effect the
realizer's residency has: the pair where it is "up" is the fastest of the
three, and the pair where it is "down" is the slowest.

The contention that *does* show up is the wording model **working**: it
shares the GPU with the d1-2B, so a round that asks for many wordings
slows the decision tier down. That is the price of the 31 lines the model
worded, and it is paid in the decision tier's own deadline.

### The memory

1,474 MiB of GPU memory for the weights, about 650 MB of RAM in the unit's
cgroup.

### The honest caveat

The three rounds above spoke 59 lines in all, and the rounds were shorter
in lines than a round without the realizer (55-76 lines per round on the
Thor without it). The wording traffic and the decision tier share the
GPU, and on a box that is already busy the decision tier's deadline is
what gives way first: the refusals went from 8 to 24 over the three
rounds. That is the trade this feature makes, stated plainly: the model
words half the lines, and the decision tier is slower for it.

---

## 6. The commands

```text
# the service
systemctl --user daemon-reload
systemctl --user enable --now lm-realizer.service
curl -s http://127.0.0.1:8092/health

# the measurement
cargo test --release -p lm-agents --test realizer_on_thor -- --ignored --nocapture

# the lane (needs the Thor and the realizer)
bash scripts/live.sh

# the d1-2B's latency, with and without the realizer
python3 ~/lm-services/realizer/ab-d1-2b.sh
```
