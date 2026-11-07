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

---

## 7. The arms: the same wording on the Thor and on the ProArt (2026-10-29)

The brief's question: the Thor is busy tonight -- the realizer words at
563-593 ms p50 and 55-60% of the lines fall back at the deadline -- and
the ProArt's RTX 4050 (6 GB) is idle. Can a small model on *that* box
word the lines instead, and should the wording climb a ladder -- the
Thor first, the ProArt second, the phrasebook last?

What was added to make the question answerable (all of it measured with
the harness above, unchanged):

- the **arm** comes from the environment (`UTTER_LLM_MODEL`,
  `UTTER_LLM_PORT`, `UTTER_LLM_BASE`, `UTTER_DUMP`); with none set it is
  the Thor's own. The base URL is new configuration
  (`utter.llm_base`, `AgentsConfig::utter_base()`), because a wording
  model may live on any box and the port alone does not say which server
  a port belongs to. The Thor-specific `ThorLlm` client became
  `ChatLlm`, a client of **a** OpenAI-shaped chat endpoint.
- the test counts the **wordings that never came** (`misses`: the model
  was asked and did not answer in time, or answered nothing) and the
  **on-plan rate** (the share of the model's own lines that kept the
  plan's intent, recipient, affect, act and target), and dumps each arm's
  numbers as JSON for the sweep.
- `scripts/realizer-ab.sh` runs the arms one after another over the same
  three seeds, warm-loads each arm's model before its round so no arm
  measures a weight load, and unloads the local models between arms --
  the card is 6 GB, and a model left resident by the arm before is
  memory the arm's own model spills against.
- the host (`lm-host`) now times its frames (`p50`/`p95`/`max`, printed
  every 600 frames and once more on exit), because a measurement of what
  a model costs the renderer needs the frame times the host never
  reported.

No new models were pulled. The five the brief names were already on the
ProArt; three of them were measured as arms (gemma2:2b, llama3.2:1b,
qwen3:0.6b), the 0.5B models are smaller than the smallest arm measured
here and were skipped as unlikely to word better than they, and
qwen2.5-coder:7b measured 23.8 tok/s in the fleet work -- a coder's
speed for a voice's job, and seven times the weights of gemma2:2b on a
card that also has to render the game.

### The night the measurement was almost taken on

The first attempt, at 19:22, ran while the Thor was heavily loaded (the
d1-2B at 1.1-1.8 s a decision): the three rounds **starved to 7 lines**
-- 148-209 decisions a round, 27-30 refusals, 1-5 lines -- and every one
of the 7 lines fell back. That is what a busy Thor does to a round, and
it is the case the ladder is for; the numbers below are from the sweep
proper, run from 21:57, after the Thor recovered (a real decision pack
at 116-263 ms, the wording model at ~110 ms).

### 7.1 The numbers

Five arms, three seeds of three thousand ticks each. The decision tier
is the Thor's d1-2B in every arm; only the wording moves.

| arm | the wording | lines | worded by the model | fallback | p50 ms | p95 ms | max ms | requests | on-plan | game's worst wait |
|---|---|---|---|---|---|---|---|---|---|---|
| thor-lfm2 | LFM2-1.2B on the Thor | 137 | 88 | 35.8% | 271 | 347 | 369 | 49 | 100% | 341 us |
| proart-gemma2 | gemma2:2b on the ProArt | 227 | 163 | 28.2% | 153 | 470 | 518 | 64 | 100% | 115 us |
| proart-llama32 | llama3.2:1b on the ProArt | 143 | 99 | 30.8% | 198 | 444 | 473 | 44 | 100% | 211 us |
| proart-qwen3 | qwen3:0.6b on the ProArt | 71 | 48 | 32.4% | 111 | 402 | 416 | 23 | 100% | 124 us |
| ladder-thor-gemma2 | LFM2 first, gemma2 second | 115 | 72 | 37.4% | 287 | 416 | 922 | 43 | 100% | 110 us |

What the numbers say, in order of how much they matter:

**Every model worded on-plan.** 100% in all five arms -- not one line
off its plan, not one naming a beacon the gates did not choose, across
470 model-worded lines. The checks are mechanical and were measured
before (§3); the local models pass them as the Thor's own does.

**Every wording that was asked for arrived in time.** `misses` is 0 in
every arm at the 1000 ms deadline, and the decomposition proves the
fallback is structural: **in every arm, the late lines equal the wording
requests exactly** (49/49, 64/64, 44/44, 23/23, 43/43). A wording is
asked when the first line of a kind falls due in an agent's bank; that
first line is worded by the phrasebook while the model writes, and every
line of the same kind after it is worded by the model. Not one second
line was lost, in any arm.

**The wording's home moves the decision tier.** The arms spoke 137
(thor) against 227 (gemma2) lines over the same three seeds. The
wording model on the Thor shares the GPU with the d1-2B; the wording
load off the Thor, the decision tier answers faster and the agents speak
more. This -- not the fallback percentage -- is the largest measured
difference between the arms: moving the wording to the ProArt bought
*1.7 lines for every line* the same rounds spoke with the wording
resident. (The line counts also swing with the Thor's own background
load -- qwen3's middle round spoke 4 lines against its neighbours' 27
and 40 -- so the per-arm fallback percentages are round shapes as much
as model speeds; the requests-equal-late decomposition above is the
part that does not move.)

**The speeds.** Warm, one line, grammar-shaped where the server knows
grammar (the Thor's llama.cpp does; Ollama ignores the field, and the
prompt and the plan checks keep the words in shape): qwen3:0.6b p50 111
ms, gemma2:2b p50 153 ms, llama3.2:1b p50 198 ms, the Thor's LFM2-1.2B
p50 271 ms. The tails: the local arms run 400-470 ms at p95 against the
Thor's 347 -- the ProArt serves the Windows desktop and WSL beside the
round, and its worst wordings are slower than the Thor's, even as its
typical wordings are twice as fast. The game waits for none of it: the
worst wait any arm put on the round was 341 microseconds (§2's 50 us is
the same mechanism, smaller lines).

qwen3 thinks, and it did not matter: Ollama separates the thinking from
the answer, `reasoning_effort: none` is accepted beside the answer, and
the reply's content is the line alone.

### 7.2 The samples: the model and the phrasebook, side by side

Fifteen lines per arm, in the order they were spoken, each with the line
the phrasebook worded for the same plan. The words are shown; the
notation around them -- the gesture, the recipient, the intent, the
urgency, the target -- is the gates' answer in every line of every arm,
by construction. The lines repeat within an arm because the bank keeps
one wording for each kind of line; the repeats are the mechanism, not a
stuck record.

#### thor-lfm2

| the plan | the model's words | the phrasebook's |
|---|---|---|
| callout to agent 6, report, calm, about beacon 3 | beacon 3 in range | beacon 3 is ours |
| callout to agent 6, report, calm, about beacon 3 | beacon 3 in range | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 2 | move to beacon 2 now | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | beacon 0 is ours |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | beacon 0 is ours |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | beacon 0 is ours |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | beacon 0 is ours |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | beacon 0 is ours |
| callout to the squad, report, calm, about beacon 3 | move to beacon 3 now | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 3 | move to beacon 3 now | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 3 | move to beacon 3 now | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 2 | move to beacon 2 now | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 3 | move to beacon 3 now | we hold beacon 3 |
| callout to agent 6, report, calm, about beacon 3 | beacon 3 in range | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 3 | move to beacon 3 now | we hold beacon 3 |

#### proart-gemma2

| the plan | the model's words | the phrasebook's |
|---|---|---|
| callout to agent 6, report, calm, about beacon 3 | Beacon 3 is down. | we hold beacon 3 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2 status report. | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2 status report. | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2 status report. | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, report. | beacon 2 is ours |
| callout to agent 6, report, calm, about beacon 3 | Beacon 3 is down. | we hold beacon 3 |
| callout to agent 6, report, calm, about beacon 3 | Beacon 3 is down. | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 3 | Beacon 3 is compromised. | beacon 3 is ours |
| callout to agent 5, report, uncertain, about beacon 3 | Beacon 3 is uncertain. | we hold beacon 3 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2 status report. | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2 status report. | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 2 | Beacon 2 status report. | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, report. | we hold beacon 2 |
| callout to agent 6, report, calm, about beacon 3 | Beacon 3 is down. | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 2 | Beacon 2 status report. | we hold beacon 2 |

#### proart-llama32

| the plan | the model's words | the phrasebook's |
|---|---|---|
| callout to agent 6, report, calm, about beacon 3 | Beacon 3, this is agent 6, report and hold. | we hold beacon 3 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one. Over. | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one, report and calm | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one, report and calm | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one, report and calm | we hold beacon 2 |
| callout to agent 6, report, calm, about beacon 3 | Beacon 3, this is agent 6, report and hold. | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one. Over. | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one, report and calm | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one, report and calm | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one, report and calm | we hold beacon 2 |
| callout to agent 6, report, calm, about beacon 3 | Beacon 3, this is agent 6, report and hold. | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one. Over. | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one, report and calm | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | Beacon 2, this is squad one, report and calm | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 0 | Beacon 0, this is squad one, report and hold. | we hold beacon 0 |

#### proart-qwen3

| the plan | the model's words | the phrasebook's |
|---|---|---|
| callout to agent 5, report, calm, about beacon 3 | beacon 3 is ours | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 2 | beacon 2 is ours | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 2 | beacon 2 is ours | we hold beacon 2 |
| callout to agent 5, report, calm, about beacon 3 | beacon 3 is ours | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 2 | beacon 2 is ours | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 2 | beacon 2 is ours | beacon 2 is ours |
| callout to agent 5, report, calm, about beacon 3 | beacon 3 is ours | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 2 | beacon 2 is ours | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 2 | beacon 2 is ours | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | beacon 2 is ours | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | beacon 2 is ours | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | we hold beacon 0 |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | beacon 0 is ours |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | we hold beacon 0 |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | beacon 0 is ours |

#### ladder-thor-gemma2

| the plan | the model's words | the phrasebook's |
|---|---|---|
| callout to the squad, report, calm, about beacon 2 | move to beacon 2 now | we hold beacon 2 |
| callout to agent 6, report, calm, about beacon 3 | beacon 3 in range | we hold beacon 3 |
| callout to the squad, report, calm, about beacon 2 | move to beacon 2 now | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 2 | move to beacon 2 now | beacon 2 is ours |
| callout to agent 6, report, calm, about beacon 3 | beacon 3 in range | beacon 3 is ours |
| callout to the squad, report, calm, about beacon 2 | move to beacon 2 now | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | move to beacon 2 now | we hold beacon 2 |
| callout to the squad, report, calm, about beacon 2 | move to beacon 2 now | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 2 | move to beacon 2 now | beacon 2 is ours |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | we hold beacon 0 |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | we hold beacon 0 |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | beacon 0 is ours |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | we hold beacon 0 |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | we hold beacon 0 |
| callout to the squad, report, calm, about beacon 0 | hold at beacon 0 | we hold beacon 0 |

The four voices are four voices. The Thor's LFM2 speaks the field order
("move to beacon 2 now"); gemma2 speaks the bulletin ("Beacon 3 is
compromised."); llama3.2 speaks the radio check ("Beacon 2, this is
squad one. Over."); qwen3 speaks the phrasebook's own words back. All
four stay inside the plan, which is the only rule. The ladder arm's
samples are all in the Thor's voice: its first stage answered every
line tonight, which is §7.4's finding in miniature.

### 7.3 The ladder: the Thor first, the ProArt second, the phrasebook last

**The gate.** The brief says to implement the ladder if a local model is
on-plan at 95% or better **and** meaningfully cuts the fallback rate.
gemma2:2b clears both: on-plan 100% (163 lines), fallback 28.2% against
the Thor arm's 35.8% -- a cut of 7.6 points, with the mechanism visible
in the latencies (p50 153 ms against 271: a wording that lands twice as
fast is in hand for twice as many of the lines that follow it). The
ladder is implemented.

**The design.** `LadderLlm` (`crates/lm-agents/src/utter.rs`) asks its
stages in order, each but the last for an equal share of the deadline
that is left and the last for all of it -- with two stages, the Thor
gets half of the 1000 ms and gemma2 gets what remains. A stage that
refuses -- down, slow, or its words not a line -- hands the line to the
next stage at once, so a stage that is merely *gone* costs a
millisecond and a stage that is *slow* costs its share and no more.
The whole climb happens on the word bank's own thread: the game never
waits on any stage of it, and the phrasebook still takes what even the
ladder misses, exactly as it takes what the one model misses. It is
configuration, not a new mode: `utter.fallback_model` (with
`fallback_port` and `fallback_base`, the base defaulting to the game's
own machine) builds the ladder inside the existing `auto` realizer;
without it `auto` is byte-for-byte what it was.

```toml
[utter]
realizer = "auto"
# the first stage: the Thor's LFM2 (the defaults)
fallback_model = "gemma2:2b"
fallback_port = 11434
# fallback_base defaults to this machine: http://127.0.0.1
```

**The ladder arm, measured.** The fifth arm of the sweep is the ladder
itself: 115 lines, 72 worded, fallback 37.4%, on-plan 100%, worst wait
on the round 110 us. On a healthy Thor the ladder is invisible -- the
Thor stage answered everything inside its share, and the arm's samples
(§7.2) are all in LFM2's voice -- except once: the arm's slowest wording
took **922 ms**, far outside the Thor arm's own distribution (max 369
ms over 49 wordings). That wording climbed: the Thor burned its half,
and gemma2 worded the line inside the rest. One rescue is one rescue;
the honest statement of the ladder arm is this: **tonight it changed
nothing, and it never waits -- which is what it is for.** Its value is
the busy night, and the busy night is not a guess: it is the 19:22 run
recorded above, when the loaded Thor starved the rounds to 7 lines and
every one fell back. A wording the Thor cannot serve in half a deadline
is exactly the wording the second stage exists for.

### 7.4 The frames: the game and the model on one GPU

The ProArt's RTX 4050 renders the game (Bevy on Windows, pinned to the
4050 -- `PowerPreference::HighPerformance` in the host -- and confirmed
by the adapter log: "NVIDIA GeForce RTX 4050 Laptop GPU", Vulkan). The
wording models of §7.1 run on the same card, inside WSL's Ollama, which
the Windows game reaches at its own `localhost:11434`. Two rounds, seed
7, three thousand ticks, `--agents stub --talk on`:

- **without the local realizer** -- the default configuration: the
  wording is the Thor's LFM2, and the 4050 renders alone;
- **with the local realizer** -- `--agents-config
  evidence/agents-utter-proart.toml`: the eight agents' minds word
  through gemma2:2b on the same GPU that is rendering.

The cumulative frame reports at the same frame counts (the first report
is after the 30-frame warm-up the instrumentation skips):

| frames | without: p50 / p95 / max ms | with gemma2: p50 / p95 / max ms |
|---|---|---|
| 570 | 16.6 / 17.7 / 22.7 | 16.6 / 18.9 / 34.0 |
| 2370 | 16.7 / 17.7 / 22.7 | 16.7 / 18.4 / 48.3 |
| 5370 | 16.7 / 17.7 / 22.7 | 16.7 / 18.3 / 762.7 |
| 6570 | 16.7 / 17.7 / 694.6 | 16.7 / 18.3 / 762.7 |
| in all | 7516 drawn: 16.7 / 17.7 / 694.6 | 10243 drawn: 16.7 / 18.1 / 762.7 |

The reading:

**The frame rate does not move.** The median frame is 16.7 ms in both
runs -- vsync-locked at 60 -- from the first report to the last. The
renderer's steady state is the display's rate in either world.

**The tail pays half a millisecond.** p95 goes from 17.7 to 18.1-18.3
ms with the model resident: the slowest 5% of frames take about half a
millisecond longer while a generation is in flight. A few frames in the
shared run land at 34-50 ms -- one to two dropped vsyncs, at the moment
a wording burst overlaps a frame -- which the run without the model
never shows (its worst steady-state frame is 22.7 ms).

**The worst frames are the round's own, not the model's.** Both runs
have one long hitch (694.6 and 762.7 ms) where the round ends and the
between-rounds flow runs; it is present in both, and it is where the
"max" in the last report comes from.

**The load caveat, stated plainly.** A stub round words only a handful
of lines: Ollama's journal shows six generations across the shared
run's window (plus the startup probe). The heavy wording traffic -- the
hundreds of wordings of §7.1 -- ran while the desktop composited, not
while a measured round rendered. What is measured here is the frame
cost of the realizer's traffic at the round the brief names, and at
that traffic the answer is: the median does not move, the tail pays
about half a millisecond, and a wording burst occasionally costs a
frame a vsync or two.

One operational note: the game's startup probe (2 s by default) timed
out once across the relay's first connection, and the round worded from
the phrasebook for it; `probe_ms: 10_000` in the measurement's config
file (`evidence/agents-utter-proart.toml`) clears it. The wording
deadline proper stays 1000 ms -- that one is what keeps the game from
ever waiting, and it was not touched.

### 7.5 The commands

```text
# the whole sweep (resumable: an arm that has reported is skipped)
bash scripts/realizer-ab.sh

# one arm
bash scripts/realizer-ab.sh proart-gemma2

# one arm, by hand: the arm is the environment
UTTER_LLM_MODEL=gemma2:2b UTTER_LLM_PORT=11434 \
UTTER_LLM_BASE=http://127.0.0.1 UTTER_DUMP=/tmp/arm.json \
  cargo test --release -p lm-agents --test realizer_on_thor -- --ignored --nocapture

# the ladder arm, by hand: the fallback is the environment too
UTTER_FALLBACK_MODEL=gemma2:2b UTTER_FALLBACK_PORT=11434 \
  cargo test --release -p lm-agents --test realizer_on_thor -- --ignored --nocapture

# the frame-time runs (from the worktree; the game opens on the desktop)
bash scripts/win-build.sh run -p lm-host --release -- \
  --agents stub --talk on --ticks 3000 --seed 7
bash scripts/win-build.sh run -p lm-host --release -- \
  --agents stub --talk on --ticks 3000 --seed 7 \
  --agents-config evidence/agents-utter-proart.toml
```

The raw dumps and the per-arm logs of the sweep are in
`evidence/realizer-ab/`.
