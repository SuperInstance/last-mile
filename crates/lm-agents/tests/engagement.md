# The engagement: why the shipped doctrine holds, and what could hold instead

The shipped doctrine ends every round in a draw. On the real Thor, over three
seeds and 3000 ticks a round, the two squads never saw each other and never
touched: **0 tags, 0 enemy-in-sight ticks**, and the three rounds ended
[1690, 1690], [1702, 1702] and [1721, 1721]. The brief asks why — which gate
answers, which facts and which branch, and whether the cause is the model's
choice, the criteria's wording, or a rule. Then it asks for doctrine
variants, measured, and a recommendation.

This is the measurement of all of it, and the write-up of the numbers.

## The commands

```text
# the real Thor: the shipped harness and every variant, three seeds, 3000 ticks
cargo test --release -p lm-agents --test engagement_on_thor -- --ignored --nocapture
ENGAGEMENT_HARNESS=assets/harness/variants/advance \
  cargo test --release -p lm-agents --test engagement_on_thor -- --ignored --nocapture

# the headless sweep: the same question over 100 seeds, without the model
DECISIVENESS_CELL=1800x9000 DECISIVENESS_HARNESS=assets/harness/variants/scout \
  cargo test --release -p lm-core --test decisiveness -- --ignored --nocapture
```

| variable | meaning |
|---|---|
| `ENGAGEMENT_HARNESS` | the harness directory (relative to the repository root): `assets/harness/default` or `assets/harness/variants/<name>` |
| `ENGAGEMENT_BACKEND` | `thor` (the default), `stub` (the control: a model that answers at once), `split` (the stub moves, the Thor speaks) |
| `ENGAGEMENT_SEEDS` | the seeds, comma separated (the default is 1,2,3) |
| `ENGAGEMENT_TICKS` | the ticks a round plays (the default is 3000) |
| `ENGAGEMENT_DUMP` | a path for the raw record: one JSON line per ask |
| `DECISIVENESS_HARNESS` | the harness the headless sweep plays (the default is the shipped reflex) |
| `DECISIVENESS_CELL` | the cells the headless sweep plays, `win_score x time_limit_ticks` (the default is all eight) |

## What is measured, and how

### The Thor measurement (`crates/lm-agents/tests/engagement_on_thor.rs`)

The round is the shipped one, played the way the game plays it: the host
loop, the agents, the least-use-first queue, the sequencer, the recorder, at
the round's own rate (30 Hz). The only thing that is not the shipped code is
the model behind the tier — the Thor's own **d1-2B** on the LAN — and the
harness, which the caller picks.

Two requests are in flight at a time (one worker thread per pack, D70), so
the measurement stays inside the brief's four.

Every ask is recorded with the facts it was asked over and the answer it came
back with. Every tick is recorded with what the agents could see. Per round
the measurement reports: the **tags**, the **enemy-sight ticks** (ticks on
which some agent had a fresh enemy within reach and a clear line of sight),
the **scores** and their **margin**, the **decisions** the answers became,
the **asks**, and the lines that were delivered **urgent**.

The facts parser that reads the branch off the facts text is covered by
`the_facts_parser_reads_the_builders_words`, on real facts texts from real
rounds, so the branch table below is not a reading of the text but a
derivation of it.

### The headless sweep (`crates/lm-core/tests/decisiveness.rs`)

The sweep plays 100 seeds headless, for the full `time_limit_ticks`, with the
harness's own **reflex** — the script that plays the moment the model does not
answer (D26). It is now harness-aware: `DECISIVENESS_HARNESS` names the
harness, and the sweep plays that harness's `reflexes/default.rhai`. The
sweep also counts what the rounds did: the tags, the sight ticks, the score
margin, and how many rounds had a squad held off the board entirely.

The sweep is the fast half of the measurement and the only one with 100
seeds; it is also a different half of the game, because the reflex plays it
and not the model. Both halves are reported, and the difference between them
is part of the finding.

## The answer: the gate, the facts, the branch

### The gate

The **`move` gate** of the default harness
(`assets/harness/default/gates.toml`). It is the only gate whose answer is
the agent's move (`options_from = "menu"`, so the options are the moves the
simulation can carry out), and D70 put it in the **move pack**, which is asked
on every think. The ten talk gates are asked separately, when the agent has
something to say.

The move gate's question is the doctrine, written as a rule over the facts:

> (1) If an enemy can see you within 6 cells and you have fewer allies within
> 4 cells than there are enemies within 6 cells, retreat. (2) If you are
> standing on a beacon your squad holds and no enemy is within 6 cells, hold.
> (3) Otherwise go to the nearest beacon your squad does not hold, if there is
> one; if there is none, hold.

That is the doctrine this document measured: the one the default harness
shipped when the measurement was made. The default harness now states the
`advance` doctrine — the second branch holds the beacon only while it is
contested — which is what this measurement recommended, and the doctrine as
it was here is kept, comparable, as the `hold` variant
(`assets/harness/variants/hold`).

### The facts

`crates/lm-agents/src/facts.rs` writes them in plain language. Two of them
are the whole story, and they are quoted here from a real round:

> Beacon 2 at column 32, row 63 is unheld; **it is the nearest beacon your
> squad does not hold**; a walking path from you is 61 steps long to the
> south. […] **You are standing on beacon 3, which your squad holds.** […]
> **No enemy is within 6 cells of you.**

### The branch

The **second** branch: *if you are standing on a beacon your squad holds and
no enemy is within 6 cells, hold.*

### It is a rule, not the model's choice and not the criteria's wording

The measurement derives the branch the doctrine selects from the facts text
(the same way the model reads it) and compares it with the option the model
chose, ask by ask. Over the three shipped rounds:

| branch | asks | the model took that branch's option |
|---|---|---|
| go to the nearest unheld beacon | 827 | 827 |
| hold (on the beacon) | 36 | 36 |
| **total** | **863** | **863 (100.0%)** |

The model agreed with the doctrine **on every single move ask**. The
criteria's wording is not what makes the agents hold: the wording only says
*which option* a branch means ("hold position (the doctrine's second branch:
you are standing on a beacon your squad holds and no enemy is within 6 cells,
or there is no beacon your squad does not hold)"), and the model picks it
because the facts say the branch applies. Remove the branch and the model
takes the other option — measured, below.

## Why the whole game freezes

Two facts do it, and they compound.

**The target is mirrored.** The map is point-symmetric (D61), and the
doctrine's target is "the nearest beacon your squad does not hold". From the
spawns, squad 0's nearest is **beacon 2** (61 walking steps) and squad 1's is
**beacon 3** (61 walking steps) — the mirror of each other, 63 cells apart.
Each squad walks to its own corner.

**The hold keeps it there.** When an agent arrives, its squad now holds that
beacon, so the facts say "You are standing on beacon 2, which your squad
holds" and "No enemy is within 6 cells of you" — and branch 2 fires and holds
it there for the rest of the round.

The measured end state of the three rounds is exactly that, and it is the
same in every seed: all four agents of squad 0 stacked on (32, 63) — beacon
2 — and all four agents of squad 1 stacked on (95, 64) — beacon 3.

| seed | ticks | end tick | tags | sight ticks | scores | margin | decisions | asks | lines | urgent |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 3000 | – | 0 | 0 | [1690, 1690] | 0 | 765 | 368 | 57 | 0 |
| 2 | 3000 | – | 0 | 0 | [1702, 1702] | 0 | 749 | 368 | 56 | 0 |
| 3 | 3000 | – | 0 | 0 | [1721, 1721] | 0 | 751 | 351 | 43 | 0 |

No round was decided inside the window, and none was going to be: each squad
holds its one beacon to the end, the scores stay level, and the round runs
into its draw.

## The urgency gate is downstream of the engagement

The urgency gate is the `intention` score gate, and the fix for it (branch
`origin/urgent`, cherry-picked into this worktree) makes it a rule over the
facts the builder states: an enemy that can see you, a stun, or a stunned
ally is urgent.

It works. It has nothing to be urgent about:

| | shipped harness |
|---|---|
| talk asks answered | 160 |
| the gate said urgent | 0 |
| lines delivered | 156 |
| lines delivered urgent | 0 |

With 0 enemy-in-sight ticks in the round, "an enemy can see you" is never
true. **The urgency gate's delivery rate is a function of the engagement**:
a doctrine that never produces contact can never produce an urgent line, and
the 160 talk asks were all answered on the quiet. The one round in the whole
measurement that kept an enemy in sight for a stretch — `raid`'s seed 3,
below — is also the only one whose gate answered urgent (4 asks) and the
only one with urgent lines delivered (3 of them).

## What a harness change can and cannot change

The measurements below are not just about the doctrine: they are about what
the d1-2B will and will not apply. Three findings, each read off the full
rounds of the variants below, over the same move question.

**A changed hold condition moves the model.** The `advance` variant inverts
one clause of the doctrine's second branch ("…and an enemy is within 6 cells
of you"). Over its three rounds, on the **68 move asks whose facts selected
the shipped doctrine's hold branch**, the model held **0 times** — it took a
beacon every time, with 0.5–0.94 on the beacon it took. The model applies
the branch faithfully; it is the branch that decides.

**A changed target the facts do not state does not move the model.** The
`contest` variant's target is "the beacon with the lowest number that no
squad holds" — a fact the builder never states (it names *the nearest beacon
your squad does not hold*, one beacon, by number). Over `contest`'s three
rounds the model took the branch the facts name on **267 of 267** move asks:
the doctrine's own target was never once chosen. The one target a harness
*can* pull the model with, some of the time, is one the facts state per
beacon — the holder: `raid`'s doctrine sends the squad to the beacon the
enemy holds, and on the 52 camp-facts where the shipped doctrine would hold,
the model moved every time, 18 of those moves to a beacon the enemy held
(the rest went back toward its own squad's beacons). The pull is real but a
minority one; the named target still wins most asks.

**A qualifier the facts do not state in one sentence is dropped.** The
`scout` variant's doctrine adds "and you are the leader of your squad" to
the hold branch. The model applied the branch without it: over the three
`scout` rounds it followed the shipped doctrine's branch on **373 of 373**
asks, and the members held on the squad's beacon 21 times — the leader-only
hold is wording the model cannot check, so it holds the way the shipped
doctrine does.

So the model applies the part of a branch condition that the facts state as a
single sentence — "you are standing on a beacon your squad holds", "no enemy
is within 6 cells of you", "the nearest beacon your squad does not hold" —
and drops what has to be combined with them. The **target** is therefore set
by the facts builder, not by the harness: a harness can change *when* an
agent takes the target (and, weakly, pull it to a target the facts state per
beacon), not *which* beacon the named target is. And on a point-symmetric
map the one target the facts name is mirrored, so the two squads are sent to
opposite beacons. The variants below are therefore built out of the
*conditions* (who holds, who moves, who retreats), because those are the
reliable levers a harness has.

## The variants

Four variants, each a harness directory under `assets/harness/variants/`:

| variant | design intent (one line) |
|---|---|
| `advance` | **Never camp**: a squad holds a beacon only while it is contested, and otherwise keeps advancing to the next beacon. |
| `scout` | **The leader holds, the members fight**: the squad keeps its beacon and its points while its members stay in the game. |
| `contest` | **Everyone fights for the one free beacon**: the lowest-numbered beacon no squad holds, so both squads go to the same place. |
| `raid` | **Take the enemy's ground**: the squad holds its own beacon only while the enemy is near it, and otherwise goes for the beacon the enemy squad holds. |

Each variant is a full harness: `gates.toml` (the doctrine the **model**
plays), `policy.toml`, `phrasebook.toml` and `reflexes/default.rhai` (the
same doctrine in the form the headless sweep can play). The talk gates,
including the urgency gate, are the shipped ones in every variant, so the
urgency measurement is comparable across them.

## The measurements

### The headless sweep (100 seeds, win_score 1800, time_limit 9000)

| harness | draws | rounds with a fight | tags/round | sight ticks/round | score margin/round | mean scores | collapsed rounds |
|---|---|---|---|---|---|---|---|
| shipped | **63.0%** | 35% | 0.62 | 413.5 | 353.6 | 1627 / 1589 | 2 |
| `advance` | **63.0%** | 35% | 0.62 | 413.5 | 353.6 | 1627 / 1589 | 2 |
| `scout` | **0.0%** | 99% | 2.93 | 1232.7 | 971.9 | 1326 / 1272 | 9 |
| `contest` | **0.0%** | 98% | 4.21 | 2029.0 | 1516.9 | 911 / 932 | 70 |
| `raid` | **89.0%** | 10% | 0.39 | 240.0 | 99.8 | 1750 / 1719 | 3 |

(A "collapsed round" is a round in which a squad scored under 200 points:
held off the board entirely. The shipped harness has two of them, and its
lowest squad score over the 100 seeds is 29.)

Four things to read here.

**`advance` is the shipped reflex.** Its headless reflex cannot say "hold
while the beacon is contested" — the reflex's contact branch already has the
moment when an enemy is near — so the variant's reflex is the shipped one and
its headless numbers are the shipped numbers. That is not a null result: it
says the variant's change is in the **doctrine**, and the doctrine is what
the model plays. The shipped harness has two different behaviours, and the
Thor plays the doctrine.

**The sweep plays the reflex, not the doctrine.** The shipped harness's
reflex already advances (it has no hold rule at all), which is why the
headless sweep sees 413 sight ticks a round while the Thor sees zero: the
reflex keeps the agents moving, and the doctrine stops them.

**The sweep also shows what a shared target is worth.** `contest` and `scout`
are the two variants whose reflex sends both squads to the same beacon, and
they are the two that turn almost every round into a fight (98% and 99%).
`contest` buys the most contact (4.21 tags a round) and pays for it: 70 of
its 100 rounds leave a squad on the board. `scout` keeps a reserve at home
and pays 9.

**`raid`'s reflex is a mirror swap.** Two squads that each take the beacon
the other holds trade the corners for ever, and a perfect mirror ends with
both of them reaching 1800 on the same tick: 89 of `raid`'s 100 rounds are
*early* draws, and only 10 have a tag in them at all. The reflex cannot
break the symmetry it is built on; on the Thor (below) the model's loose
application of the same doctrine does break it — which is why `raid`'s Thor
row, not its sweep row, is the one to read.

### The real Thor (3 seeds, 3000 ticks)

| harness | seed | ticks | end tick | tags | sight ticks | scores | margin | decisions | asks | lines | urgent |
|---|---|---|---|---|---|---|---|---|---|---|---|
| shipped | 1 | 3000 | – | 0 | 0 | [1690, 1690] | 0 | 765 | 368 | 57 | 0 |
| shipped | 2 | 3000 | – | 0 | 0 | [1702, 1702] | 0 | 749 | 368 | 56 | 0 |
| shipped | 3 | 3000 | – | 0 | 0 | [1721, 1721] | 0 | 751 | 351 | 43 | 0 |
| `advance` | 1 | 3000 | – | **3** | 802 | [1085, 1727] | 642 | 260 | 299 | 2 | 0 |
| `advance` | 2 | 3000 | – | 0 | 490 | [817, 1494] | 677 | 310 | 331 | 19 | 0 |
| `advance` | 3 | 2629 | 2628 | 0 | 444 | [1355, 1800] | 445 | 295 | 307 | 7 | 0 |
| `scout` | 1 | 3000 | – | 0 | 0 | [1671, 1690] | 19 | 470 | 162 | 6 | 0 |
| `scout` | 2 | 3000 | – | 0 | 0 | [1702, 1691] | 11 | 489 | 169 | 14 | 0 |
| `scout` | 3 | 3000 | – | 0 | 0 | [1721, 1721] | 0 | 536 | 171 | 15 | 0 |
| `contest` | 1 | 3000 | – | 0 | 0 | [1690, 1690] | 0 | 529 | 161 | 26 | 0 |
| `contest` | 2 | 2266 | 2265 | 0 | 0 | [913, 1802] | 889 | 220 | 119 | 7 | 0 |
| `contest` | 3 | 3000 | – | 0 | 0 | [1721, 1721] | 0 | 320 | 106 | 7 | 0 |
| `raid` | 1 | 3000 | – | 0 | 0 | [1690, 1690] | 0 | 148 | 165 | 5 | 0 |
| `raid` | 2 | 2649 | 2648 | 0 | 428 | [1800, 1351] | 449 | 206 | 229 | 1 | 0 |
| `raid` | 3 | 2810 | 2809 | 0 | 607 | [1531, 1800] | 269 | 240 | 261 | 12 | **3** |

(The round's own time limit is 9000 ticks, so a 3000-tick window can end
before the round is decided. "end tick" is the tick a squad reached 1800
points; a dash means neither squad had reached it when the window closed, and
a level score means the round was still undecided.)

Reading the Thor table against the shipped rows:

- **`advance` and `raid` are the only variants that change the game.** Every
  `advance` round had an enemy in sight (444–802 sight ticks against 0 in
  every shipped round), `advance` seed 1 is the only round in the measurement
  with tags (3), and three of the six `advance`/`raid` rounds were decided
  inside the window — against none of the three shipped rounds, whose scores
  never left level.
- **`advance`'s model behaviour is unambiguous.** It followed the shipped
  doctrine's branch on 703 of 785 move asks (89.6%), and the disagreements
  are the variant working: on the 68 asks whose facts selected the shipped
  hold branch it held **0** times, it retreated on all 27 outnumbered asks,
  and the remaining 14 are contact moments (9 retreats and 5 holds under
  fire, at 0.27–0.68 confidence). It never camped once.
- **`raid` never camps.** It followed the shipped branch on 460 of 538
  asks (85.5%), and the disagreements are the hold breaking: on all **52**
  camp-facts — standing on its squad's beacon, no enemy near — it moved
  instead of holding (18 of those moves to enemy-held ground, the rest back
  toward its own squad's beacons), and on 26 take-the-beacon asks it chose a
  beacon its own squad already held over the named one. A squad that never
  sits is a squad that keeps crossing the beacon field, and that is what
  produced the contact: two of its three rounds were decided ([1800, 1351]
  at 2648, [1531, 1800] at 2809) with 428 and 607 sight ticks, while seed 1
  played the shipped mirror camp after all — the model's application is
  loose enough to break the mirror most of the time, not always.
- **`scout` and `contest` play like the shipped doctrine on the Thor**, and
  the reason is measured, not guessed: their doctrines change the *target*
  (`contest`) or add a qualifier the facts do not state in one sentence
  (`scout`), and the d1-2B applies neither — 373 of 373 and 267 of 267 move
  asks took the branch the shipped doctrine selects. `contest`'s seed 2 is
  the shipped game's own rare symmetry-break ([913, 1802]), not the variant's
  doing.
- **Contact is still not tags.** `advance` seed 1's 3 tags are the only ones
  in fifteen rounds. The squads meet (802 and 607 sight ticks) and mostly do
  not touch: they are sent to mirrored beacons, and the
  map's centre pair — beacons 0 at (64,63) and 1 at (63,64) — is a diagonal
  step apart, 2.83 m, while a tag needs 2 m.
- **The urgency gate fires when there is something to be urgent about.** The
  shipped harness's 160 talk asks produced no urgent answer at all; `raid`
  seed 3 — the round with the most sustained contact — answered urgent on 4
  of its 26 talk asks and delivered 3 urgent lines. The gate (fixed on
  `origin/urgent`) works; the shipped doctrine never gives it a moment.

## The same measurement after the change (2026-10-30)

The default harness now states the `advance` doctrine (Casey's decision, and
the recommendation above). The old doctrine is kept, comparable, as the `hold`
variant (`assets/harness/variants/hold`). Both were measured again with the
same command, the same three seeds and 3000 ticks a round:

| harness | seed | ticks | end tick | tags | sight ticks | scores | margin | decisions | asks | lines | urgent |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `hold` (the old default) | 1 | 3000 | – | 0 | 0 | [1690, 1690] | 0 | 728 | 339 | 60 | 0 |
| `hold` (the old default) | 2 | 3000 | – | 0 | 0 | [1702, 1702] | 0 | 740 | 355 | 56 | 0 |
| `hold` (the old default) | 3 | 3000 | – | 0 | 0 | [1721, 1721] | 0 | 751 | 353 | 53 | 0 |
| `advance` (the new default), run 1 | 1 | 3000 | – | 0 | 0 | [896, 903] | 7 | 587 | 589 | 37 | 0 |
| `advance` (the new default), run 1 | 2 | 3000 | – | 0 | 0 | [854, 830] | 24 | 601 | 607 | 34 | 0 |
| `advance` (the new default), run 1 | 3 | 3000 | – | 0 | 0 | [921, 867] | 54 | 617 | 615 | 38 | 0 |
| `advance` (the new default), run 2 | 1 | 2984 | 2983 | 0 | 0 | [1006, 1800] | 794 | 446 | 445 | 33 | 0 |
| `advance` (the new default), run 2 | 2 | 3000 | – | 0 | 0 | [844, 845] | 1 | 592 | 594 | 35 | 0 |
| `advance` (the new default), run 2 | 3 | 3000 | – | 0 | 0 | [969, 818] | 151 | 612 | 615 | 38 | 0 |

Two things to read here, and one that does not reproduce.

**The `hold` variant is the old doctrine, exactly.** Its three rounds are the
shipped rows above, score for score ([1690, 1690], [1702, 1702], [1721,
1721]) and 0 tags and 0 sight ticks each. The doctrine pins the play — every
agent camps on its beacon — so the round is a function of its seed, and the
variant measures what the default harness measured. That is what makes it
comparable.

**The new doctrine changes the game, and the change is real.** The
symmetric stalemate is gone: the rounds now end unevenly ([896, 903], [854,
830], [921, 867], [844, 845], [969, 818]), and run 2's seed 1 ended at tick
2983 with a squad on 1800 — the win score, and a decided round, where every
shipped round sat level at 0. The scores are lower because a squad that keeps
advancing holds its beacon for less of the round, and they differ from round
to round because the model, not the rule, now decides where a squad goes.

**The contact did not reproduce.** The `advance` rows in the measurement
above carry 444–802 sight ticks a round; these six rounds carry zero. The
gates are the same ones (the new default's move gate is the `advance`
variant's, byte for byte) and the reflex is the same, so the difference is
the play: under the old doctrine the branch pins every agent's behaviour and
the round is a function of its seed, while under the new doctrine the model's
sampling decides where the squads go, and in these six rounds they never met.
A single three-seed pass is not a verdict on the doctrine, but it is not
evidence for it either, and it is recorded here rather than left out.

## The recommendation

**Ship `advance` now, with `raid` as the variant to try next. Do not ship
`scout` or `contest` as they stand.**

`advance` is the one variant whose change the d1-2B applies cleanly, and it
is strictly better than the shipped doctrine on the measures: the camp
breaks (0 holds on 68 camp-facts), every round has an enemy in sight (444–802
sight ticks against 0), it owns the measurement's only tags (3, in seed 1),
and its rounds carry real margins (642, 677, 445) with one decided inside
the window where the shipped rounds sit level at 0. Its doctrine is also the
smallest edit that could work — the shipped doctrine's second branch becomes
"…and an enemy is within 6 cells of you", which is one clause, and it is a
rule over a sentence the facts already state. It is the brief's own "switch
to attack when holding is safe", as one clause of one branch.

`raid` decides two of three rounds, keeps the longest contact (607 sight
ticks in seed 3) and produced the measurement's only urgent lines — the
urgency fix paying off the moment a doctrine makes a fight possible. Its
doctrine is the brief's own lure: never sit on what you hold, and take the
enemy's ground. It is
second because its application is looser (85.5% against `advance`'s 89.6%,
one round of three fell back into the mirror camp) and its headless reflex
is a mirror swap (89% early draws in the sweep): the variant works through
the model's loose application of it, which is a thinner thing to rely on
than `advance`'s clean rule.

`scout` and `contest` should not ship as they stand: on the Thor they are
the shipped doctrine with extra words. Their headless numbers are real but
they are the **reflex's** numbers, and the reflex is not what the model
plays.

**But the brief's target is not met by any harness-only variant**, and the
measurement says exactly why and where the next lever is. The target is
"fights in most rounds"; the best a harness change reached is contact in
every round (`advance`) and tags in one round of three. The blocker is the
**target**, and the target is not the harness's to set:

1. The facts builder names one target per agent — "the nearest beacon your
   squad does not hold" — and the model takes that one on 827 of 827 asks.
2. On a point-symmetric map that target is mirrored: squad 0's is beacon 2
   (61 steps), squad 1's is beacon 3 (61 steps), and after they take them
   squad 0's is beacon 1 (31 steps) and squad 1's is beacon 0 (31 steps).
   The two are always on the mirrored pair.
3. The mirrored pair in the centre is a diagonal step apart (2.83 m) and the
   tag range is 2 m, so contact is not a fight.

The headless sweep says what a **shared** target is worth, because a reflex
can name one: `contest` puts both squads on beacon 0 and 98% of its rounds
have a fight (4.21 tags a round), and `scout` does the same with a reserve
at home — 99% of rounds with a fight, and a squad left on the board in 9
rounds instead of 70.

So the next lever is `crates/lm-agents/src/facts.rs`: it should state a
second, **shared** fact — the beacon no squad holds, and the lowest-numbered
one — the way it already states the doctrine's own target in words. With that
fact in place the `contest` and `scout` doctrines become expressible to the
d1-2B, and the headless sweep's numbers become the Thor's. The harness
change to make first is the one measured here: `advance`.

## What this does not say

- **The sample is small on the Thor.** Three seeds and a 3000-tick window is
  three rounds per harness against the sweep's 100 seeds and 9000 ticks. The
  Thor numbers say what happened in three rounds; the sweep says what happens
  in a hundred.
- **The Thor's answer rate is a condition of the run.** An earlier pass of
  these suites on the same day answered far fewer asks per round (its
  shipped rounds recorded tens of answered asks where these record hundreds),
  and its per-round tick, decision and line columns differ from these tables
  for that reason. Every structural finding was the same in
  both passes: the shipped doctrine's 100% branch agreement, the stacked
  corner camps, the zero contact of the shipped rounds, `scout`'s and
  `contest`'s shipped-branch play, and `raid`'s cross-map raids. The tables
  here are the re-measured ones, and the `raid` row of the sweep is the
  variant's final reflex (the reflex was revised once during the work; the
  row was measured again after the revision).
- **The sweep and the Thor are not the same game.** The sweep plays the
  reflex and the Thor plays the doctrine, and the reflex's "nearest beacon"
  is a Chebyshev distance while the facts' is a walking path. That is why
  `advance`'s sweep numbers are the shipped ones while its Thor numbers are
  not, and it is the reason the sweep cannot stand in for the Thor when the
  target matters.
- **The two suites disagree about `scout` and `contest`.** The sweep says
  99% and 98% of rounds have a fight; the Thor says zero contact in six
  rounds. Both are right: the first is the reflex's play, the second is the
  model's. A variant is only as good as the part of it the model actually
  applies.
- **The d1-2B is one model.** The pattern — the model applies the branch
  whose condition the facts state as a single sentence — is consistent
  across the shipped doctrine (863 of 863 asks), `scout` (373 of 373),
  `contest` (267 of 267) and `advance`'s inversion (0 holds on 68
  camp-facts), but it is a pattern read off a 2B model on one map.
- **Nothing here changes the scoring rules.** `assets/rules/beacons.rhai` is
  the oracle and is untouched, and no variant changes the win score, the
  time limit, the tag range or the beacon radius.
