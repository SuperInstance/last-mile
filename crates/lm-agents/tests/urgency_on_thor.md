# The urgency gate on the real Thor

The agents never speak urgently. The E7 score view found that no line was
ever delivered urgent — 83 logged rounds and four Thor rounds — and the
brief asks for the reason and the fix, with the distribution in the
comments.

This is the measurement of both, on the Thor's own **d1-2B**, on the
shipped harness, over the game's own facts.

## The command

```text
cargo test --release -p lm-agents --test urgency_on_thor -- --ignored --nocapture
```

## What is measured

Every ask that comes back from the Thor's **d1-2B** is recorded with:

- the `intention` gate's answer: the score (the level's index), the
  probability of every level, and the confidence;
- the **game state at the moment the question was asked**: the tick, the
  score margin, the time left, whether a tag fight is on, how many enemies
  the agent can see;
- the **facts text the question was asked over**, which is what the
  situation is read from: an enemy with a clear line of sight, a stun, an
  ally's stun, a beacon the enemy holds;
- the line the gate's answer became, and the affect it was delivered with.

The round is the shipped one, played the way the game plays it: the host
loop, the agents, the least-use-first queue, the sequencer, the recorder.
Three seeds, 3000 ticks each, at the round's own rate (30 Hz).

The watcher asks the Thor with the floor at zero, so every gate's own
number is seen; the rule that would have been applied to them is derived
from them and reported below.

### The three configurations

The measurement plays the round in three ways, because **the game does not
always have a fight** and a measurement of urgency needs one:

| configuration | what plays | what it is for |
|---|---|---|
| **shipped** | the d1-2B answers both packs | the game as it ships |
| **stub** | the stub answers both packs | the control: the same rounds with a model that answers at once |
| **split** | the stub answers the move pack, the d1-2B the talk pack | the fight, with the real model's urgency answers |

The configurations are chosen with `URGENCY_BACKEND=stub` and
`URGENCY_BACKEND=split`; the shipped one is the default.

## Before the fix

Three rounds, 3000 ticks each, seeds 1-3, the shipped configuration:

| seed | lines delivered | asks answered | ticks of game |
|---|---|---|---|
| 1 | 59 | 351 | 3000 |
| 2 | 59 | 352 | 3000 |
| 3 | 61 | 355 | 3000 |

**180 talk asks answered, 878 move asks answered.** The intention gate's
answers:

| level | count |
|---|---|
| 0 (idle) | 175 |
| 0.345, 0.368 (idle — below the ordinary line) | 2 |
| 1 (ordinary) | 3 |
| 2 (urgent) | **0** |

The probability of the urgent level: p50 **0.0016**, p90 0.0088, max
**0.0361**. **Never above 0.2 — never above 0.04.**

The lines that came out of it:

| affect | lines |
|---|---|
| Calm | 176 |
| Ordinary | 3 |
| **Urgent** | **0** |

**The symptom, exactly as the brief describes it: no line is ever
delivered urgent.**

### The confusion table

The situation the gate was asked over (read off the facts it was given),
against the urgency it answered with:

| the situation the facts state | asks | calm | ordinary | urgent |
|---|---|---|---|---|
| under fire | 0 | 0 | 0 | 0 |
| contest | 116 | 116 | 0 | 0 |
| quiet | 64 | 61 | 3 | 0 |

There is no fight row, because **the shipped rounds have no fight**: over
9000 ticks there were **0 tags and 0 ticks with an enemy in sight**. Every
one of the 180 asks was made over facts that said:

> You know of no fresh enemies. No enemy is within 6 cells of you. You are
> not stunned.

### The control: the same rounds, the stub's play

The same three rounds with the stub's play (the stub answers at once, so
the agents move every tick and both squads walk to the same beacon):

| | shipped (d1-2B) | stub |
|---|---|---|
| ticks with an enemy in sight | **0** | 199 |
| ticks with a tag in the last 90 | **0** | 150 |
| talk asks answered | 180 | 1954 |
| the levels the gate picked | idle 175, ordinary 3, urgent 0 | ordinary 1954 |

**The game does have fights — the shipped rounds simply do not have them.**
The two squads each walk to the beacon nearest their own spawn (squad 0 to
beacon 2 at column 32, row 63; squad 1 to beacon 3 at column 95, row 64),
they never cross, and the round is over before they meet. The cause is
the doctrine's second branch — "if you are standing on a beacon your squad
holds and no enemy is within 6 cells, hold" — which is not in the shipped
reflex: the reflex always walks to the nearest beacon its squad does not
hold, so a squad that has taken a beacon goes on to the next one, and the
two squads converge on the central pair (the reflex's seed 1: first sight
on tick 1785, first tag on tick 1846).

The branch quoted here is the one the default harness shipped when this was
measured. The default harness now holds the beacon only while it is
contested — the `advance` doctrine (2026-10-30) — and the branch as it was
is kept, comparable, as the `hold` variant
(assets/harness/variants/hold).

**That is a fact about the game's play, and it is not what this brief is
about** — the doctrine is the game's strategy and another seat's evidence
(E4c) is measured on it. What follows is about the *voice*: the mapping
from a moment to a tone.

### The fighting round, before the fix

The same measurement on a round that fights (the split configuration: the
stub's play, the d1-2B on the talk pack):

| the situation the facts state | asks | calm | ordinary | urgent |
|---|---|---|---|---|
| **under fire** | **23** | **1** | **8** | **14** |
| contest | 28 | 28 | 0 | 0 |
| quiet | 156 | 147 | 9 | 0 |

The probability of the urgent level in a fight: min **0.011**, p50 0.962,
max 0.975 — **14 of 23 above 0.2**.

**So even in a fight the gate was a coin flip: 8 of the 23 asks came back
ordinary and 1 came back calm.** The brief's question — "is urgent ever
above 0.2 when a fight is on?" — has the answer *sometimes, and sometimes
it is 1%*.

## The diagnosis

### The gate is not flat: asked about a fight, it says urgent

The d1-2B, asked the **shipped** intention gate over the game's own fight
facts — the facts builder's text, on the ticks where the squads are in
contact — answers **level 2, urgent**:

| the facts | the level it picked | p(urgent) | confidence |
|---|---|---|---|
| 4 enemies within 6 cells, all with a clear line of sight | **2 (urgent)** | 0.962 | 0.97 |
| 3 enemies within 6 cells, all with a clear line of sight | **2 (urgent)** | 0.959 | 0.97 |
| 1 enemy within 6 cells, with a clear line of sight | **2 (urgent)** | 0.882 | 0.88 |

And over the real quiet states of the shipped rounds it answers **calm**,
41 of 41. The separation is perfect *when the facts carry the threat*.

### The floor and the escalation hide nothing

| | the gate's confidence |
|---|---|
| the shipped rounds (every ask) | min 0.45, p50 0.94, max 0.96 |
| a fight state | 0.88-0.97 |

**No ask was refused by D72's floor (0.30)**: the lowest confidence in the
whole measurement was 0.45, and that was an *idle* answer. Only 2 of the
180 asks fell under D71's threshold (0.50) and were escalated to the second
model — both of them idle answers too, so the escalation took an idle
answer and left an idle answer. Neither can be hiding an urgent answer.

### The wording: the levels name facts the builder never states

This is where the defect is. The shipped levels were:

| level | the level said |
|---|---|
| idle (0) | "the moment is quiet: **no enemy is within 6 cells** and your squad is where it should be" |
| ordinary (1) | "the moment is ordinary: the usual play goes on and nothing has to be said now" |
| urgent (2) | "the moment is urgent: **an enemy can see you within 6 cells**, or **a beacon your squad holds is about to be lost**" |

Three things are wrong with that, and each of them was measured on the
d1-2B by asking the gate over the same facts with one situation changed:

1. **"an enemy can see you within 6 cells" contradicts the facts.** The
   builder's count line is "No enemy is within 6 cells of you" — the
   doctrine's own number, for the move gate — and it says that *while* the
   enemy lines say "Enemy 3, last seen 1 tick ago at column 64, row 58, is
   11 cells away and can see you (clear line of sight)". The model reads
   the count line against the level and answers **ordinary** for an enemy
   that is watching the agent from eleven cells away:

   | the facts | the level | p(urgent) |
   |---|---|---|
   | an enemy 11 cells away, with a clear line of sight | **ordinary** | 0.022 |
   | three enemies within 6 cells, all with a clear line of sight | urgent | 0.958 |

2. **"a beacon your squad holds is about to be lost" is a fact the builder
   does not state.** It says which squad holds a beacon — "held by your
   squad", "held by enemy squad 1" — and never that one is about to be
   lost. The model cannot match it to anything.

3. **The one fact that means "you are under fire right now" was in no
   level at all**: "You are stunned." — the agent that was just tagged.
   The gate reads a tagged agent as **calm**:

   | the facts | the level | p(urgent) |
   |---|---|---|
   | you are stunned, and no enemy is in sight | **calm** | 0.005 |
   | an ally 2 cells away is stunned | **calm** | 0.007 |

**So the tone of the round was decided by two conditions the model could
not check, and the condition it could check — an enemy with a clear line
of sight — was narrowed to "within 6 cells", which the facts' count line
denies.** The gate was not flat: it was answering the wrong question.

### The fact assembly is not the culprit

The threat fact expires: an enemy sighting is dropped once it is older
than `stale_enemy_ticks` (6 ticks — two tenths of a second), so a squad
whose line of sight breaks for a third of a second has "no fresh enemies"
in its facts. Measured over the shipped reflex's fight (seed 1, 19200
agent-ticks):

| | agent-ticks |
|---|---|
| with a fresh enemy next door | 305 |
| with only a remembered enemy next door | 462 |
| of those, the threat was less than a second old | 24 |
| the remembered threat's age | median 238 ticks (8 s), worst 468 ticks (15.6 s) |

**The expired ones are ghosts** — the enemy was seen eight seconds ago and
has moved on — so the builder is right to drop them, and the fact assembly
is not the defect. (The one thing the factory says that fights the
urgency rule is the count line, and that line belongs to the move gate's
doctrine; it stays.)

## The fix

| what | where | why |
|---|---|---|
| The urgency gate is stated as a rule over the facts the builder states: "an enemy can see you", "you are stunned", "an ally within 4 cells of you is stunned" | `assets/harness/default/gates.toml` | The levels are what the model reads; they named two conditions it was never given, and left out the two that mean "now" |
| The regression pin: the shipped gate's levels name the facts the builder states, and no level asks for one it does not | `crates/lm-agents/src/gates.rs` | The wording is doctrine and it drifts like any other |

**No question was added**: the pack is still the eleven questions it was
(the move gate, the five talk gates and their five presence gates), and
the load on the Thor did not grow.

### What the new wording does, measured

The same states, the same model, the old wording and the new one:

| the facts | old | new |
|---|---|---|
| quiet | calm, p(urgent) 0.003 | **calm**, p(urgent) 0.002 |
| an enemy 11 cells away, watching | ordinary, p 0.022 | **urgent**, p 0.976 |
| three enemies within 6 cells | urgent, p 0.958 | **urgent**, p 0.996 |
| you are stunned | **calm**, p 0.005 | **urgent**, p 0.985 |
| an ally 2 cells away is stunned | **calm**, p 0.007 | **urgent**, p 0.989 |
| the enemy holds a beacon | calm, p 0.010 | **calm**, p 0.002 |

The quiet moment stays calm, and the enemy holding a beacon stays calm —
a squad that has lost a beacon to the enemy is not a squad under fire.
Everything that means "now" moves to urgent.

## After the fix

### The shipped configuration

Three rounds, 3000 ticks each, seeds 1-3:

| seed | lines delivered | asks answered | ticks of game | scores |
|---|---|---|---|---|
| 1 | 60 | 349 | 3000 | [1689, 1689] |
| 2 | 43 | 291 | 3000 | [1701, 1701] |
| 3 | 44 | 286 | 3000 | [1720, 1720] |

(The scores are the game's own, and they are the same shape as before the
fix: the fix changes the tone of a line, not the move the agent makes.)

**149 talk asks answered, 777 move asks answered.** The intention gate
answered **level 0 (idle) 149 times out of 149**, and the lines:

| affect | lines |
|---|---|
| Calm | 147 |
| Ordinary | 0 |
| **Urgent** | 0 |

The confusion table:

| the situation the facts state | asks | calm | ordinary | urgent |
|---|---|---|---|---|
| under fire | 0 | 0 | 0 | 0 |
| contest | 90 | 90 | 0 | 0 |
| quiet | 59 | 59 | 0 | 0 |

**The quiet stretches stay calm, and they are now consistently calm**:
before, 3 of the 64 quiet moments came back ordinary; now none does. The
gate's confidence also rose — the minimum was 0.45 before (two of the
asks were escalated to the second model) and is **0.82** now.

**The shipped rounds still have no fight** — 0 tags, 0 ticks with an enemy
in sight — so there is still no urgent line to deliver. That is the game's
play, not the voice.

### The fighting round

The same measurement on a round that fights (the split configuration):

| the situation the facts state | asks | calm | ordinary | urgent |
|---|---|---|---|---|
| **under fire** | **19** | **0** | **0** | **19** |
| contest | 24 | 24 | 0 | 0 |
| quiet | 149 | 149 | 0 | 0 |

The probability of the urgent level in a fight: min **0.799**, p50 0.995,
max 0.999 — **19 of 19 above 0.2**.

**Before and after, on the same states:**

| the situation the facts state | before | after |
|---|---|---|
| under fire | 1 calm, 8 ordinary, **14 urgent** | **0 calm, 0 ordinary, 19 urgent** |
| p(urgent) in a fight | min 0.011, p50 0.962, max 0.975 | **min 0.799**, p50 0.995, max 0.999 |
| the asks above p(urgent) = 0.2 in a fight | 14 of 23 | **19 of 19** |
| contest | 28 calm | 24 calm |
| quiet | 147 calm, 9 ordinary | **149 calm, 0 ordinary** |

The lines that came out of it: 179 lines — 160 calm and **19 urgent**
(before: 165 calm, 17 ordinary, 14 urgent).

**The brief's target, in the numbers: a fight produces urgent lines (19
of 19 asks over a moment the agent was under fire, and 19 urgent lines
delivered), and the quiet stretches stay calm (149 of 149).**

### The delivered confusion table

The situation of the **round** at the tick a line was delivered, against
the urgency it was delivered with — the table the brief asks for:

| situation | before: lines | calm | ordinary | urgent | after: lines | calm | ordinary | urgent |
|---|---|---|---|---|---|---|---|---|
| fight | 11 | 6 | 5 | 0 | 10 | 8 | 0 | 2 |
| threat | 3 | 3 | 0 | 0 | 2 | 2 | 0 | 0 |
| quiet | 182 | 156 | 12 | 14 | 167 | 150 | 0 | 17 |

**This table is the round's situation, and it is one or two seconds
behind the gate.** A thought lands late (D55: thinking time is gameplay),
so a line asked before the contact is delivered inside the fight with the
tone of the moment before it, and a line asked in the fight is delivered
after it. The tables above it — the situation the **facts** state against
the urgency the gate answered — are the mapping itself, and they are the
ones the fix changes.

## What this measurement does not say

- **It is not a score.** The Thor's model answers the same pack the same
  way every time, so the three rounds are three samples of the same
  behaviour, not three games. The score column of the soak is the game's,
  not this measurement's.
- **The shipped rounds have no fight**, so the "fight" row of the
  delivered table has to come from a round that has one (the stub's play,
  with the d1-2B on the talk pack). That is a property of the game's play
  — the doctrine's hold branch — and not of the voice; it is reported, not
  changed here.
- **A line's urgency is the urgency of the moment it was asked over**, and
  the round's situation at the moment it was delivered is a second or two
  later. The two tables above differ for that reason and for no other.
- **The gate wordings are this model's.** Another model reads them
  differently, and the wording is the harness's to re-word when it does.
- **The scale is three levels.** The affect is bucketed by code from the
  weighted level: below 0.5 calm, up to 1.0 ordinary, above it urgent
  (`bucket_affect` in `crates/lm-agents/src/utter.rs`). A fight that the
  model reads as 0.8 is a calm line, and that is the middle of the scale
  doing its work.
