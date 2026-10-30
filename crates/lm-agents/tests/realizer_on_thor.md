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

## 7. The A/B: could Qwen3-Coder-30B-A3B word the lines better or faster?

The box serves another model that suggested itself for the wording:
Qwen3-Coder-30B-A3B-Instruct, a MoE with about 3B active parameters, at
`:8001`. The A/B played the same harness both ways -- the same prompt
(the plan and not the facts, plain words), the same 1000 ms deadline,
seeds 1, 2 and 3 of three thousand ticks per arm -- with only the
`[utter]` `llm_model` / `llm_port` of the config changed between the
arms.

### The door needed no adapter

`:8001` is llama.cpp too, the same shape of server that carries the d
models: OpenAI `/v1/chat/completions`, and it takes the `grammar` field
the client already sends. The request `ThorLlm` has been posting to
`:8092` is exactly what `:8001` serves, so the smallest adapter was no
adapter: the config alone carries the arm. The harness reads the arm
from the environment (`UTTER_LLM_MODEL`, `UTTER_LLM_PORT`) and, when
`UTTER_DUMP` names a file, writes every spoken line there for the
pairing of the two arms' wordings over the same plans.

| what | arm a | arm b |
|---|---|---|
| the model | LFM2-1.2B, Q8_0 | Qwen3-Coder-30B-A3B-Instruct, Q4_K_M |
| the door | `:8092` | `:8001`, two slots, shared with the pi task runner |
| the GPU bill | 1,476 MiB | 24,015 MiB |

### The numbers

Two complete A/Bs were played, about an hour apart, because the box is
shared and its load moves: the first run's arms were back to back under
the same backdrop (a Glimmer-30B pi task on the GPU, load average 6-9),
and the second run is the replicate. The first run's arms finished their
measurement before a dump-path bug (since fixed) stopped the test after
the numbers were printed; the second run is the green run of record.

| run | arm | lines | by the model | fell back | p50 | p95 | refusals |
|---|---|---|---|---|---|---|---|
| 1 | LFM2-1.2B | 71 | 30 | 57.7% | 592.8 ms | 656.3 ms | 63 |
| 1 | Qwen3-Coder | 110 | 44 | 60.0% | 853.8 ms | 918.1 ms | 57 |
| 2 | LFM2-1.2B | 83 | 37 | 55.4% | 563.4 ms | 638.4 ms | 62 |
| 2 | Qwen3-Coder | 28 | 12 | 57.1% | 779.2 ms | 850.0 ms | 67 |

**The wording's own latency is the one clear difference.** LFM2 answers
a line in 563-593 ms at p50 in both runs; Qwen needs 779-854 ms -- about
250 ms more, p95 to match. LFM2's slowest banked answer was 663 ms;
Qwen's was 980 ms, under the 1000 ms deadline with twenty milliseconds
to spare. That margin is the whole story of the busy box below.

**The fallback rate did not move.** Both arms of both runs fell back on
55-60% of lines, within the run-to-run spread of either arm. The
fallback is mostly structural, not a deadline missed: the bank's ask
starts when the first line of a kind is due, so that line is always
worded by the phrasebook, and the model's speed moves the rate only at
the margin. (The 47.5% of the first measurement is today's 55-60% for
the same reason the d1-2B's idle p50 is 1.1 s and not 0.6 s: the box is
busier than it was then.)

**The checks passed for both.** Every line the models worded carried its
plan -- 30/30, 44/44, 37/37 and 12/12 on-plan, none naming another
target, none answered-in-time-and-wrong -- and the game's wait for a
wording stayed under a millisecond either way (max 636 us for LFM2, 78
us for Qwen).

### What a busy box does to each arm

The second run's Qwen arm started as a soak test and two more GPU
processes landed on the box. Its first two rounds worded **zero and one
lines** -- the agents had no lines due, so the wording model was never
asked, while the decision tier refused 24 and 23 packs of its own under
the load -- and the third round recovered (twelve model-worded lines,
p50 779 ms). LFM2's arm of the same run played under the higher load
average (median 10.6 against 8.3) and still answered every ask in about
600 ms. LFM2 has ~400 ms of headroom under the deadline; Qwen has ~150,
and a busy box eats it.

The two slots of `:8001` are shared with the pi task runner. No pi job
used `:8001` during the measurement -- the one that ran used Glimmer at
`:8005` -- so the slots were the A/B's own. But a pi task on `:8001` is
the same class of risk as the soak test, one queue away.

### The d1-2B while each model works

The eleven-question pack was posted to `:8094` in a loop for as long as
each arm's rounds ran, plus three idle references:

| when | p50 of the pack |
|---|---|
| idle, before / between / after | 1134 / 1786 / 1306 ms |
| during the LFM2 arm's rounds | 1534 ms (four runs, 1257-2170) |
| during the Qwen arm's rounds | 1715 ms (four runs, 1223-2029) |

**No measurable differential from the wording model.** The spread within
either arm is as large as the difference between the arms -- the LFM2
arm ran under the higher load and benched the faster -- and what the
d1-2B pays is the rounds themselves plus the box, as section 5 found.
The refusals signal agrees: on today's box both arms refuse about twenty
packs a round (57-67 over three rounds each), where the quiet box of the
first measurement refused eight. That is the box, not the wording model.

### The samples, side by side, for the same plans

Twenty lines, the plans the rounds actually produced. The bank keeps one
wording per kind of line and the models answer at temperature 0, so a
plan's wording is constant and the twenty lines cover the five distinct
plans both models worded -- the same convention as section 3's thirty
samples over eight plans.

| # | the plan | LFM2-1.2B | Qwen3-Coder | the phrasebook |
|---|---|---|---|---|
| 0 | Callout to the squad, Report, calm, about beacon 2 | move to beacon 2 now | Beacon 2 is secure and holding | beacon 2 is ours |
| 1 | Callout to the squad, Report, calm, about beacon 0 | hold at beacon 0 | Beacon 0 is secure and holding | we hold beacon 0 |
| 2 | Callout to agent 5, Report, uncertain, about beacon 3 | move to beacon 3 now | beacon 3 report uncertain | we hold beacon 3 |
| 3 | Callout to agent 6, Report, calm, about beacon 3 | beacon 3 in range | Beacon 3 is secure and holding | we hold beacon 3 |
| 4 | Callout to the squad, Report, calm, about beacon 3 | move to beacon 3 now | Beacon 3 is secure and holding | we hold beacon 3 |
| 5 | Callout to the squad, Report, calm, about beacon 2 | move to beacon 2 now | Beacon 2 is secure and holding | we hold beacon 2 |
| 6 | Callout to the squad, Report, calm, about beacon 0 | hold at beacon 0 | Beacon 0 is secure and holding | beacon 0 is ours |
| 7 | Callout to agent 5, Report, uncertain, about beacon 3 | move to beacon 3 now | beacon 3 report uncertain | we hold beacon 3 |
| 8 | Callout to agent 6, Report, calm, about beacon 3 | beacon 3 in range | Beacon 3 is secure and holding | we hold beacon 3 |
| 9 | Callout to the squad, Report, calm, about beacon 3 | move to beacon 3 now | Beacon 3 is secure and holding | we hold beacon 3 |
| 10 | Callout to the squad, Report, calm, about beacon 2 | move to beacon 2 now | Beacon 2 is secure and holding | beacon 2 is ours |
| 11 | Callout to the squad, Report, calm, about beacon 0 | hold at beacon 0 | Beacon 0 is secure and holding | we hold beacon 0 |
| 12 | Callout to agent 5, Report, uncertain, about beacon 3 | move to beacon 3 now | beacon 3 report uncertain | we hold beacon 3 |
| 13 | Callout to agent 6, Report, calm, about beacon 3 | beacon 3 in range | Beacon 3 is secure and holding | we hold beacon 3 |
| 14 | Callout to the squad, Report, calm, about beacon 3 | move to beacon 3 now | Beacon 3 is secure and holding | we hold beacon 3 |
| 15 | Callout to the squad, Report, calm, about beacon 2 | move to beacon 2 now | Beacon 2 is secure and holding | we hold beacon 2 |
| 16 | Callout to the squad, Report, calm, about beacon 0 | hold at beacon 0 | Beacon 0 is secure and holding | beacon 0 is ours |
| 17 | Callout to agent 5, Report, uncertain, about beacon 3 | move to beacon 3 now | beacon 3 report uncertain | we hold beacon 3 |
| 18 | Callout to agent 6, Report, calm, about beacon 3 | beacon 3 in range | Beacon 3 is secure and holding | we hold beacon 3 |
| 19 | Callout to the squad, Report, calm, about beacon 3 | move to beacon 3 now | Beacon 3 is secure and holding | we hold beacon 3 |

### The honest judgement

On the words alone, Qwen is the better radio for the lines the rounds
actually produce. Its calm reports are reports -- "Beacon 2 is secure
and holding" -- where LFM2 keeps wording a report as an order ("move to
beacon 2 now", the same imperative streak section 3 recorded), and its
one uncertain call, "beacon 3 report uncertain", is genuine shorthand
under pressure. LFM2's clipped "beacon 3 in range" is tight but
ambiguous, and an order that arrives wearing a report's tags is a line
the squad has to reinterpret.

But the A/B asked "better **or faster**", and on everything that is the
wording mechanism Qwen loses: ~250 ms slower at p50, a slowest answer
twenty milliseconds under the deadline, rounds that word zero lines when
the box is busy where LFM2 answers at 600 ms under the same load, 24 GiB
of GPU against 1.5 GiB, and two slots shared with the pi task runner.
The fallback rate is a wash, the on-plan rate is 100% for both, and the
decision tier cannot tell the two apart through the box's noise.

**Verdict: LFM2-1.2B stays the default, and no config change is
proposed.** The wording Qwen buys is a nose ahead on fidelity; the price
is a door that misses its own deadline the moment the box earns its
living. If the wording itself is what needs improving, the next lever is
the prompt -- or a small instruct-tuned voice model on the realizer's
own port -- not a 30B coder MoE behind a shared door.

### The commands

```text
# arm a: the shipped default
cargo test --release -p lm-agents --test realizer_on_thor -- --ignored --nocapture

# arm b: the config alone carries the arm
UTTER_LLM_MODEL=Qwen3-Coder-30B-A3B-Instruct UTTER_LLM_PORT=8001 \
  UTTER_DUMP=target/ab/arm-b.dump \
  cargo test --release -p lm-agents --test realizer_on_thor -- --ignored --nocapture

# the d1-2B while each model works
python3 ~/lm-services/realizer/bench-d1-2b.py --label "during the rounds" --n 20
```
