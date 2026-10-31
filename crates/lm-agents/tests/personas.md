# The personas: six patches on the answer, and what they are worth

A persona is a **System Two patch** (ROADMAP §2.4). It is not an agent, not a
role, and not a new question: it biases the answers that came back, before
the choice is made. It never adds a question to the pack (D70).

This is the write-up of the measurement in
`crates/lm-agents/tests/personas_on_thor.rs`: the default squad against a
squad that thinks with a personality, on the real Thor.

The measurement has been run in full **twice**, and the two runs do not
agree on the numbers: **a round is not reproducible by its seed**, because
the round is coupled to the wall clock and the model's refusals are not.
What *does* reproduce across the two runs is the patch itself and the
personas' relative effect on the game, and that is what is taken from
here.

## What a persona is

A persona is a file of data, `assets/harness/personas/<id>.toml`, and a
harness may use any persona with any agent. The file carries:

- **`gate_bias`** — the patch itself: per gate, per option, a number in
  **tenths of a nat** added to the option's **log-odds** before the choice.
  A bias of 10 multiplies the option's odds by e (2.718); a bias of 30
  multiplies them by twenty.
- **`marks`** — how the persona talks ("halting", "breathy", "urgent").
  They go into the prompt the wording model is given.
- **`voice` and `style`** — the speaker and the emotion style, which must
  be ones the harness's `voice.toml` carries. A persona that names a voice
  the harness does not have is a refusal with a name (D26).
- **`barks`** — six short lines. They are seeded into the agent's
  phrasebook, so they are the words the agent says when the phrasebook
  words the line.

### The names a persona uses

The options are named the way the **gate** names them, not the way the
answer keys them:

| gate | kind | the names a persona uses |
|---|---|---|
| `move` | choice over the menu | `beacon`, `hold`, `follow`, `retreat`, and `beacon2` for one beacon alone |
| `intention` | score | `idle`, `ordinary`, `urgent` |
| `intent` | choice | `order`, `ack`, `warn`, `query`, `report` |
| `gesture` | choice | `command`, `question`, `statement`, `callout` |
| `attention` | choice | `squad`, `all`, `agent0` … `agent7` |
| `*_present` | noul | `yes`: the belief that there is something to say |

### The arithmetic

The bias is added to the log-odds in **fixed point** (Q16) and the answer is
turned back into a distribution by a softmax over the biased log-odds. The
arithmetic is integer-only, so the same answer gives the same choice on
every machine (D69) — the last bit of a logarithm is a `libm`'s business,
and a `libm` is not a thing this repo may depend on.

The patch is applied to the answer **before** the gates are read: the move's
lane values, the intent's choice, the urgency's score and the presence
gates' belief all come from the patched distribution. A gate the persona
does not name is left exactly as it was.

### Where it is logged

The patch is **outside the sim**: it is applied before the choice, so it is
not a decision and it does not enter the sim's state. It is kept as
telemetry — `Agents::persona_log()` and `Agents::persona_skipped()` — for
the same reason the model's latencies are kept: a reader cannot tell a mind
that thinks with a personality from one that does not by looking at the
round.

It is **not** in the decision's provenance, because `Provenance` travels in
the `lm-proto` message and a new variant is a protocol change (a version
bump and a handshake change). That is a decision for the director, not for
this patch; see "Proposed decisions" below.

## The commands

```bash
# the personas load, the patch moves the answer it names, the golden holds
cargo test -p lm-agents --lib persona

# the measurement: the real Thor, the shipped harness, six personas,
# three seeds, 3000 ticks, one round in flight at a time
cargo test --release -p lm-agents --test personas_on_thor -- --ignored --nocapture

# the voice on its own: the same plan, worded with and without the marks
cargo test --release -p lm-agents --test personas_on_thor \
  the_personas_voice_on_thor -- --ignored --nocapture

# a shorter run, while looking at it
PERSONAS_TICKS=600 PERSONAS_SEEDS=1 PERSONAS_LIST=anxious-grunt \
  cargo test --release -p lm-agents --test personas_on_thor -- --ignored --nocapture

# the same seed twice, to see whether a round is reproducible at all
PERSONAS_TICKS=3000 PERSONAS_SEEDS=1 PERSONAS_LIST=quiet-sharpshooter \
  cargo test --release -p lm-agents --test personas_on_thor -- --ignored --nocapture
```

The last one is the experiment behind "A round is not reproducible by its
seed" below: run it twice and compare the two `baseline 1:` lines.

## What is measured, and how

Every round is played the way the game plays it: the host loop, the agents,
the least-use-first queue, the sequencer, the recorder. The only thing that
is not the shipped code is the model behind the tier — the Thor's own
**d1-2B**, on the LAN — and the persona on one of the squads.

The round's own two squads are the A/B: **squad 0 thinks with the shipped
doctrine and squad 1 thinks with the persona**, on the same map, on the
same seed, at the same time. The baseline is the same round with no
persona at all, so the yardstick is known.

| what | how |
|---|---|
| the score | the two squads' scores at the end of the round, and the margin |
| contact | the ticks on which some agent had an enemy in sight, and the tags the ruleset made, by squad |
| the lines | how many were spoken, by squad, from the sim's own talk records |
| the distinctness of the lines | the **words** of the lines, per squad, and how many of the persona's lines the default squad also said |
| what the lines were for | the intent and the gesture mix of each squad's lines |
| refusals | every refusal of the round, by kind |
| the patch | how many biases the persona applied, and how many it could not |

One round is in flight at a time — the rounds are played one after another,
never two at once — so the Thor is asked for one round's worth of traffic
and no more. Within a round the agents ask on **two lanes** (move and
talk), one worker each, so at most **two requests are in flight** at any
moment, which is the cap the brief sets.

### The box it was measured on

This measurement has been run **twice in full**, on the same Thor, with
the same harness, the same six personas and the same three seeds.

| run | load during the run | 21 rounds took |
|---|---|---|
| the first | 19 → a peak of 93 (another seat's debug-build round sweep on the same fourteen cores; the run's own report printed `19.24 20.76 28.55`) | 35 min |
| the second (the numbers below) | 8 → a peak of about 100, in bursts, from the other seats' own sweeps on the same fourteen cores; the run's own report printed `74.33 36.18 37.32` at the end | 34 min |

**The two runs do not agree**, and that is the most important thing this
write-up has to say. It is not a matter of the numbers moving a little:
the second run's *baseline* — two squads with no persona at all — has a
completely different shape from the first run's baseline. See "A round is
not reproducible by its seed" below, where the three-run evidence is.

The load is reported because the rounds are coupled to the wall clock: a
round is a real-time simulation, and an answer that does not land before
its deadline is refused in favour of the reflex (D26). How many asks land
in time is a property of the machine, not of the seed.

## The measurement

21 rounds: 3 baseline (no persona) and 3 for each of the six personas, three
seeds each, 3000 ticks a round. The numbers below are the **second** run.

### The baseline: two squads with no persona

| seed | ticks | scores (squad 0 / squad 1) | margin | tags | sight ticks | lines | distinct lines |
|---|---|---|---|---|---|---|---|
| 1 | 3000 | 580 / 1747 | 1167 | 2 / 4 | 2850 / 2108 | 0 / 0 | 0 |
| 2 | 2944 | 1646 / 1800 | 154 | 0 / 0 | 2047 / 2462 | 2 / 0 | 2 |
| 3 | 2833 | 1724 / 1800 | 76 | 0 / 0 | 1545 / 1607 | 4 / 0 | 2 |

Squad 1 took **3 of the 3** baseline rounds. The first run's baseline had
squad 1 take 2 of 3, with 306 agent-ticks of sight in the one round where
the squads met at all and 0 in the other two; this run's baseline has
**thousands** of sight ticks in every round. The shipped doctrine is not a
frozen game — how much contact the squads get is decided run to run.

### The personas against the default squad

Three rounds each, 3000 ticks each, the persona on squad 1.

| persona | won | drew | lost | score (persona / default) | tags (persona / default) | sight ticks (persona / default) | lines (persona / default) | line overlap | biases applied | refusals |
|---|---|---|---|---|---|---|---|---|---|---|
| anxious-grunt | 2 | 0 | 1 | 4169 / 3351 | 0 / 0 | 0 / 0 | 0 / 12 | 0.00 | 4697 | 162 |
| chatterbox | 2 | 0 | 1 | 4166 / 3439 | 0 / 0 | 3397 / 3794 | 1 / 8 | 0.00 | 3394 | 136 |
| literal-minded-rule-follower | 0 | 0 | 3 | 2871 / 5402 | 1 / 2 | 687 / 1110 | 0 / 2 | 0.00 | 3153 | 122 |
| middle-manager-leader | 3 | 0 | 0 | 5400 / 4353 | 2 / 1 | 4549 / 3383 | 0 / 4 | 0.00 | 2312 | 154 |
| overconfident-sprinter | 2 | 0 | 1 | 4576 / 4671 | 1 / 1 | 2986 / 3515 | 0 / 0 | 0.00 | 2135 | 121 |
| quiet-sharpshooter | 1 | 0 | 2 | 3759 / 5311 | 0 / 1 | 3509 / 4624 | 0 / 1 | 0.00 | 1844 | 111 |

For comparison, the first run's table, which is kept here because it is the
run that did **not** reproduce:

| persona | won | drew | lost | score (persona / default) | sight ticks (persona / default) | biases applied | refusals |
|---|---|---|---|---|---|---|---|
| anxious-grunt | 3 | 0 | 0 | 4544 / 2223 | 0 / 0 | 5152 | 186 |
| chatterbox | 3 | 0 | 0 | 5175 / 2617 | 1031 / 670 | 5690 | 192 |
| literal-minded-rule-follower | 2 | 0 | 1 | 4419 / 3212 | 934 / 2634 | 5239 | 198 |
| middle-manager-leader | 3 | 0 | 0 | 4925 / 2596 | 2464 / 2332 | 3377 | 108 |
| overconfident-sprinter | 3 | 0 | 0 | 5368 / 2500 | 1967 / 1541 | 2143 | 113 |
| quiet-sharpshooter | 3 | 0 | 0 | 4457 / 2455 | 6463 / 7427 | 1348 | 93 |

The two tables agree on the patch and on the personas' relative effect on
contact, and disagree on the rest. The persona squad won **10 of 18**
rounds in the second run against **17 of 18** in the first; the quiet
sharpshooter went from 3 wins and the most contact in the library to 1 win
and the second most. The column that holds across both runs without
exception is the one the patch controls: **biases applied**, with **none
skipped**, in every one of the 36 persona rounds.

### What the numbers say

**The patch lands.** Every persona applied 1348 to 5690 biases over three
rounds in the first run and 1844 to 4697 in the second, and **skipped none**
in either: every gate and every option the six personas name exists in the
shipped harness. The patch is not a no-op that logs a lot.

**What reproduces, and what does not.** Over the two runs:

| | reproduces? |
|---|---|
| the patch applies, and applies every bias it names | **yes** — 36 rounds, 0 skipped |
| the patch adds no refusals | **yes** |
| the wording is unchanged by the marks | **yes** — 6 of 6, three times |
| no persona collapses its squad | **yes** |
| the personas' *relative* effect on contact | **yes** — the same ordering |
| the absolute contact | no — the baseline's own moves by a factor of eight |
| the win rate | no — 17 of 18, then 10 of 18 |
| the baseline itself | no — its squad 1 won 2 of 3, then 3 of 3 |

**A round is not reproducible by its seed.** This is the finding that
changes how every other number here has to be read, so it is stated with
its evidence. The same seed (1), the same persona (quiet-sharpshooter),
the same harness and the same model were played three times at full
length. Both squads' numbers are given, and **squad 1 is the persona's
squad** (in the baseline, squad 1 is the same squad with no persona on
it, so it is the control):

| run | baseline: squad 1's score | baseline sight (s0 / s1) | baseline decisions | persona squad's score | margin | persona sight | persona decisions | persona biases |
|---|---|---|---|---|---|---|---|---|
| A | 1747 — **won** by 1167 | 2850 / 2108 | 146 | 1215 | 585 **lost** | 419 | 155 | 461 |
| B | 619 — **lost** by 1096 | 1780 / 2441 | 206 | 1712 | 1307 **won** | 2577 | 192 | 761 |
| C | 1800 — **won** by 306 | 1999 / 1389 | 227 | 1800 | 579 **won** | 1458 | 254 | 927 |

Runs B and C are the same seed minutes apart on the same box. The
baseline's squad 1 **won, lost, won**; the persona squad **lost, won,
won**; the persona squad's contact was 419, 2577 and 1458 sight ticks —
a factor of six between two runs of the same seed.

The cause is not the model and not the seed:

- The **decision model is deterministic**. The same pack posted five
  times to the Thor's d1-2B returned byte-identical answers every time
  (same choice, same probabilities to the last digit). Nothing is sampled.
- The **sim is deterministic**. `lm-core` is fixed-point and the golden
  is pinned; there is no clock and no unordered map in it.
- The **round is not**. The host loop runs in real time, the agents ask
  on a wall-clock deadline, and an answer that does not land in time is
  refused in favour of the reflex (`mind.rs`: "No model lands in time: the
  reflex plays the moment"). So the number of decisions an agent gets in
  3000 ticks is a property of how fast the model answered that minute:
  146 decisions in the baseline of run A, 227 in run C, from the same seed.

**The win rate is a hint, not a result — and it is weaker than that.** The
persona squad won 17 of 18 rounds in the first run and 10 of 18 in the
second, and the baseline's own squad 1 — a squad with no persona on it —
won 2 of 3 in the first run and 3 of 3 in the second. A measure that
cannot tell a persona squad from a no-persona squad on the same seed is
not a measure of personas. **The win rate in this table should not be used
to choose a cast.**

**The contact is not attributable to the persona in absolute terms.** The
first run made the personas look like they were what made the game play:
the baseline had 306 agent-ticks of sight in the one round where the
squads met at all, and four personas put thousands on the board. The
second run's baseline had thousands of sight ticks in **every** round,
with no persona anywhere, and the persona squads' contact per round (0 to
2714) straddles the baseline's own (1607 to 2462). The earlier claim —
that the personas make the game play — **did not reproduce**, and it is
withdrawn. What does reproduce is their relative effect, in the table
below.

**One thing about contact does reproduce — the personas' *relative* effect
on it.** Some rounds have **no contact at all** — not for the persona
squad, but for *both* squads, so the round freezes. How often that
happened, in each run:

| arm | run 1 | run 2 |
|---|---|---|
| the baseline (no persona) | 2 of 3 | 0 of 3 |
| anxious-grunt | **3 of 3** | **3 of 3** |
| literal-minded-rule-follower | 2 of 3 | 2 of 3 |
| chatterbox | 1 of 3 | 1 of 3 |
| overconfident-sprinter | 1 of 3 | 0 of 3 |
| middle-manager-leader | 0 of 3 | 0 of 3 |
| quiet-sharpshooter | 0 of 3 | 0 of 3 |

The **ordering** is the same in both runs — the anxious grunt freezes the
most rounds, the literal rule-follower next, the chatterbox one, and the
middle-manager leader and the quiet sharpshooter never. The absolute
numbers are not the same: the baseline itself froze two rounds of three in
the first run and none in the second, and the personas' contact totals
moved by a factor of two or three between runs. What reproduces is the
**shape**: the anxious grunt and the literal rule-follower take the game
out of contact, and the middle-manager leader and the quiet sharpshooter
keep it playing.

The anxious grunt's mechanism is in its file: `beacon` −10, `retreat` +18,
`follow` +10. The beacon is the point the two squads meet at, so a squad
that will not take it leaves both squads alone. The literal-minded
rule-follower is the same story, weaker (`hold` +12, `retreat` +10), and it
is the one persona that lost every round of the second run.

| persona | contact, run 1 (persona / default) | contact, run 2 (persona / default) |
|---|---|---|
| middle-manager-leader | 2464 (2332) | 4549 (3383) |
| quiet-sharpshooter | 6463 (7427) | 3509 (4624) |
| chatterbox | 1031 (670) | 3397 (3794) |
| overconfident-sprinter | 1967 (1541) | 2986 (3515) |
| literal-minded-rule-follower | 934 (2634) | 687 (1110) |
| anxious-grunt | 0 (0) | 0 (0) |

**The refusals do not move.** The personas add none: 186 to 198 refusals
over three rounds against the baseline's 192 in the first run, and 111 to
162 against the baseline's 118 in the second. The patch does not make the
model answer worse. This holds in both runs.

**No persona collapsed its squad.** The persona squads ended on 3 to 7
distinct cells in both runs, not on one, and no persona drove its squad's
tags to zero and stayed there. The anxious grunt is the exception that
is about contact rather than movement: its squad moved (6 distinct cells)
and simply never met anyone.

### Do the personas sound different?

**Not with the shipped wording model.** The game's talk is too sparse to
answer the question in play: a hundred seconds of play is 0 to 12 lines per
squad, and the persona squads said **0 or 1** lines in the second run and
**0 to 4** in the first. The line overlap of 0.00 is therefore not evidence
of a distinct voice — there was almost nothing to compare. (The baseline
arm says the same thing: squad 0 spoke 6 lines and squad 1 spoke 0 across
three rounds of the same game with **no persona anywhere**, so "the persona
squad is the quiet one" is a fact about the squad, not about the persona.)

So the wording was measured where it is deterministic: the same plan,
worded by the same model (the Thor's LFM2-1.2B on :8092), with and without
the persona's marks.

| voice | the line it wrote for the same plan |
|---|---|
| the shipped voice | `move to beacon 2 now` |
| anxious grunt (`halting`, `breathy`, `urgent`) | `move to beacon 2 now` |
| chatterbox (`chatty`, `eager`, `quick`) | `move to beacon 2 now` |
| literal-minded rule-follower (`exact`, `literal`, `even`) | `move to beacon 2 now` |
| middle-manager leader (`clipped`, `procedural`, `even`) | `move to beacon 2 now` |
| overconfident sprinter (`brisk`, `cocky`, `loud`) | `move to beacon 2 now` |
| quiet sharpshooter (`terse`, `level`, `unhurried`) | `move to beacon 2 now` |

**Six of six personas worded the line exactly like the shipped voice.** The
marks are in the prompt and the model reads them; a 1.2B model does not act
on them. The persona's voice today is its **barks** and nothing else: they
are the phrasebook's templates, and the phrasebook speaks when the model
misses its deadline.

This was measured **three times** — in both full A/B runs, and once more on
its own — and the answer was the same line, for all six personas, every
time. It is one of the few measurements in this write-up that reproduces.

## The recommendation

### The starting cast: three personas

**The quiet sharpshooter**, **the overconfident sprinter** and the
**middle-manager leader**.

**This is a recommendation about the library, not a measurement of which
persona wins.** The A/B cannot rank them: it cannot separate a persona
squad from a no-persona squad on the same seed. What the cast is chosen
on is the personas' **designed behaviour** — the bias each file names —
and the one thing that reproduced across both runs: which personas leave
the game playing and which take it out of contact.

- **the anchor** (quiet-sharpshooter): holds the ground it is given
  (`hold` +1.2 nats), and it says nothing (0 lines in three rounds in both
  runs). It **never** froze a round in either run, and it had the most
  contact in the first (6463 against the default squad's 7427) and the
  second most in the second (3509 against 4624).
- **the runner** (overconfident-sprinter): takes the beacon (`beacon` +2.2
  nats, `retreat` −2.2). It froze one round of three in the first run and
  none in the second, and it saw more than the squad it played in the
  first (1967 against 1541) and slightly less in the second (2986 against
  3515).
- **the organiser** (middle-manager-leader): orders (`order` +2.0 nats,
  `command` +1.8) and addresses its own squad (`squad` +1.4, `all` −0.6).
  It **never** froze a round in either run, it had more contact than the
  squad it played in **both** runs (2464 against 2332, then 4549 against
  3383), and it is the only persona that won every round in both runs
  (3 of 3 and 3 of 3) — which is a hint, not a result, because the
  baseline's own squad 1 won 3 of 3 in one run and 2 of 3 in the other.

### Out of the first cast

- **the chatterbox** — the one thing it is for does not work. The presence
  gate already says "yes", so a +2.4-nat tilt on "yes" cannot make a mind
  talk: its squad spoke **1** line in three rounds in the second run against
  the default squad's **8** (and 4 against 16 in the first). It does make
  contact (3397 against 3794 in the second run, 1031 against 670 in the
  first), so it is worth keeping in the library — but not as the talker, and
  not in the first cast.
- **the anxious grunt** — it took the round out of the game. Its retreat
  (+1.8), follow (+1.0) and beacon (−1.0) biases cost it the engagement:
  it froze **every** round it played, in **both** runs, where the baseline
  froze two of three in one run and none in the other. It won some rounds
  by sitting on a beacon nobody contested, which is the frozen game with a
  name on it.
- **the literal-minded rule-follower** — the same, and quieter: it froze
  two of its three rounds in **both** runs, saw less contact than the
  squad it played in both runs (934 against 2634, then 687 against 1110),
  and it is the only persona to lose every round of the second run. A
  persona that removes play is a persona that makes the game worse.

### What has to change before the voice matters

1. **The wording model, or the wording path.** The marks do nothing with
   a 1.2B model. Either the persona's voice is carried by the **barks**
   alone — the phrasebook's templates, which do speak — or the marks go to
   a model that acts on them. The barks are the only mechanism that works
   today, and the persona files should be written as if the barks were the
   voice.
2. **The presence gate cannot make a mind talk.** It is a probability that
   is already near one. A persona that wants to talk more needs to change
   the **airtime budget** or the **presence floor**, not the probability.
   That is a change to the talk rules, not to the persona patch, and it is
   not made here.
3. **The intent and the gesture are not visible either.** Every line in
   the run was `Report` / `Callout`, on both sides, whatever the intent
   bias said. The model's answers carry one intent; a persona's bias on
   `intent` is applied to the probabilities and then overwritten by the
   model's habit. The gates that *do* show are `move` and `intention`.

## What this does not say

- **A round is not reproducible by its seed, so the numbers measured in
  play here are weaker than they look.** The same seed played three times
  gave three different baselines and three different persona outcomes (see
  "A round is not reproducible by its seed"). The model is deterministic
  and the sim is deterministic; the coupling between them is the wall
  clock. The win rate, the contact totals and the score in this write-up
  are **descriptions of trajectories**, not effects. What survives is the
  patch's own numbers and the *relative* ordering of the personas, and
  those are what the recommendation leans on.
- **The win rate is not a result.** It is reported because the brief asks
  for it. It went 17 of 18 in the first run and 10 of 18 in the second,
  and the baseline's own no-persona squad 1 went 2 of 3 and 3 of 3. The
  recommendation leans on the personas' designed behaviour and on the
  contact signal that survived both runs, not on the win rate.
- **The lines are not a measurement of the voice.** 0 to 12 lines a squad a
  round, and the persona squads said 0 or 1 of them. The overlap of 0.00
  is a small number's luck, not a measurement. The one thing that *is* a
  measurement is the wording probe above, and it says the marks do not
  reach the model's voice.
- **The personality is only half the mechanism.** The patch moves the
  answers; the *plan* the gates then produce is still the model's habit
  (one intent, one gesture). A persona changes the game through its move
  and its urgency, not through what it says.
- **The refusals are the same on both sides, but they are not small**: 16
  malformed answers and 43 to 55 late escalations a round on the first run
  (load 93) and 36 to 66 a round on the second (load up to about 100). That
  is the game's own health, not the personas' — the personas add none — but
  it is also the **mechanism** of the non-reproducibility above: a late
  answer is refused and the reflex plays the moment instead, so the load of
  the box writes itself into the trajectory.
- **Nothing here was measured on the game.** The scores, the contact and
  the lines come from the headless host loop, not from the windowed game:
  the personas are wired into the agents layer, and the host's rendering of
  a persona's speaker and style is not done yet.

## Proposed decisions

1. **The persona id in the round's log.** The patch is recorded as
   telemetry on the agents, so a reader of a *round's* log cannot see which
   persona a mind was thinking with. Putting it in `Decision::provenance`
   would say it, but `Provenance` travels in the `lm-proto` message and a
   new variant is a protocol change: a version bump, a handshake change,
   and every peer that decodes a decision. The alternative — the persona
   id in the **round header** (which is hashed and pinned, and where the
   harness commit already lives) — says the same thing at the cost of one
   persona per round rather than one per agent. **Proposed, not done.**
2. **The persona's voice: the barks or the marks.** The measurement says
   the marks do nothing with the shipped wording model and the barks are
   the only path that works. Either the persona files are written around
   the barks, or the marks are kept for a model that acts on them and are
   documented as inert today.
3. **The bias unit.** The bias is in tenths of a nat, chosen here so that
   a file's numbers are readable and a bias of 30 is a strong but not
   absolute tilt. It is not in the ROADMAP draft, which says only "added to
   option log-odds". If the director prefers whole nats, the personas
   need their numbers divided by ten.
4. **The persona as a harness or as a library.** The personas live in
   `assets/harness/personas/`, outside every harness, and are chosen per
   agent by `agents.toml` (`[personas]`). That keeps them out of the
   harness commit, which means a round's log does not pin them. The
   alternative — a persona inside the harness — would pin it and would
   make one persona per harness rather than per agent.
5. **How a persona is measured at all.** The finding above is not about
   the personas; it is that the round cannot be replayed, so nothing
   measured in play can be attributed to a patch. Two ways out, both
   bigger than this brief:
   - **Decouple the sim from the wall clock.** The host loop advances the
     sim on a wall clock and the agents answer on a deadline; if the
     decision arrived a tick later the round would still be the same
     round. A sim that steps on its answers rather than on the clock
     would make a seed mean what a seed is supposed to mean.
   - **Measure the patch without the game.** The patch's own effect is
     visible before the choice: the answer that came back, and the answer
     the patch turned it into. A test that asks the same question with
     and without the bias, and checks the choice moved, measures the
     persona directly and is reproducible. That is the measurement this
     patch can support today, and it is the one the golden's discipline
     wants. **Proposed, not done.**
