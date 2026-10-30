# The engagement: why the shipped doctrine holds, and what could hold instead

The shipped doctrine ends every round in a draw. On the real Thor, over three
seeds and 3000 ticks a round, the two squads never saw each other and never
touched: **0 tags, 0 enemy-in-sight ticks**, and the three rounds ended
[1690, 1690], [1702, 1702] and [1721, 1721]. The brief asks why — which gate
answers, which facts and which branch, and whether the cause is the model's
choice, the criteria's wording, or a rule. Then it asks for doctrine
variants, measured, and a recommendation.

This is the measurement of all of it, and the write-up of the numbers.

The shared target — the deeper lever this file names, built and measured —
is in **The shared target**, below.

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

That lever has since been built and measured: **The shared target**,
below.

## The shared target: one objective both squads are sent to

The recommendation above ends by naming a lever it does not pull. The
doctrine's **target** is set by the facts builder
(`crates/lm-agents/src/facts.rs`), not by the harness, so no harness change
can send both squads to the same beacon: the builder names one beacon per
agent — "the nearest beacon your squad does not hold" — and the model takes
that one on 827 of 827 asks. The lever it names is a **shared** fact: one
target that both squads read off the same field, so both walk to the same
beacon and meet there.

This section is that lever, built, measured, and written up.

### What was built

One harness parameter: `facts.toml`, in the harness directory beside
`gates.toml` and `policy.toml`. It is **off by default** — the shipped
harness has no facts file, a harness without one states no shared target,
and a blank one leaves the harness's identity alone (`content_id` takes the
facts in only when they are not empty). Nothing about the shipped game
changes until a harness asks for it.

The parameter is read once, when the mind is built
(`crates/lm-agents/src/mind.rs`), and it is **parsed, not defaulted**: a
facts file that does not load — or that asks for something the facts cannot
state — is a refusal the round can see (D26), not a silent agent that
states the wrong target. It is then handed to the facts builder, which is
the only place the change is.

Three rules, each a function of what every agent can see — the beacons, who
holds them, and where they are:

| rule | the shared target | how the facts state it |
|---|---|---|
| `off` | none: the shipped facts, unchanged | — |
| `free` | the lowest-numbered beacon **no squad holds** | a named target |
| `centre` | the **most central** beacon — the one nearest the middle of the others | a named target |
| `ghost` | the **hot beacon**: the one the attractor sits on now | a **field**: an attractor with a strength, a radius and a falloff |

All of them are deterministic, integer-only and fixed-point: the same
situation always yields the same target and the same facts, so the
situation key still works (D49). The rule takes the doctrine's own target's
place — the facts state one target, not two — and the shared target is
marked per beacon, so the model reads it the way it reads every other
per-beacon fact.

### The ghost field

`ghost` follows the ghost-tiles model (SuperInstance/ghost-tiles-1):
invisible tiles that influence the visible state. The tile has a position, a
kind (an attractor pulls, a repulsor pushes), a phase, a strength, a radius
and a falloff, and the phase says how the strength changes over the round —
**permanent** (constant), **temporal** (fades over `max_age_ticks`),
**recurring** (pulses between a half and the whole over `cycle_ticks`).

The **field model** knows both kinds; the **rule** states an attractor. A
repulsor is a place the squads are pushed *away* from, so it names no
target to go to, and a harness that asks for one is refused rather than
handed a target the facts would contradict (D26).

What is new here is the **hot zone**: the attractor sits on one beacon at a
time and it **walks** — it moves to the next free beacon every
`rotation_ticks` ticks. The hot beacon is a function of the tick and of the
beacon list, and both are the same for both squads, so both are pulled
toward the same place at the same time. The facts report it as facts:

> The shared target is beacon 0: the beacon you are pulled toward. Both
> squads are going for it. You are pulled toward beacon 0, and the pull is
> weak where you stand. The pull reaches 96 cells, and beacon 0 is 61 cells
> from you. Beacon 0 at column 64, row 63 is unheld; it is the shared
> target: the beacon you are pulled toward; a walking path from you is 75
> steps long to the west. Beacon 1 at column 63, row 64 is unheld; it is
> not the shared target; […]

The pull is in the facts' **fields** as well as their text (`shared.pull`,
`shared.band`, `shared.radius`), so it is in the situation key and the
field moves with the agent as it walks.

### The commands

```text
# the headless sweep, one per harness
DECISIVENESS_CELL=1800x9000 DECISIVENESS_HARNESS=assets/harness/variants/shared-free \
  cargo test --release -p lm-core --test decisiveness -- --ignored --nocapture

# the Thor
ENGAGEMENT_HARNESS=assets/harness/variants/shared-free \
  cargo test --release -p lm-agents --test engagement_on_thor -- --ignored --nocapture
```

### The headless sweep (100 seeds, win_score 1800, time_limit 9000)

| harness | draws | rounds with a fight | tags/round | sight ticks/round | score margin/round | mean scores | collapsed rounds | median tick |
|---|---|---|---|---|---|---|---|---|
| shipped | **63.0%** | 35% | 0.62 | 413.5 | 353.6 | 1627 / 1589 | 2 | 4784 |
| `advance` | **63.0%** | 35% | 0.62 | 413.5 | 353.6 | 1627 / 1589 | 2 | 4784 |
| `shared-free` | **0.0%** | 98% | 4.19 | 2007.3 | 1557.5 | 912 / 919 | 75 | 3414 |
| `shared-centre` | **0.0%** | 98% | 4.68 | 1984.0 | 1571.1 | 1034 / 807 | 76 | 3222 |
| `shared-ghost` | **0.0%** | 97% | **5.45** | 1900.2 | 1336.3 | 1482 / 756 | **45** | 4578 |
| `shared-ghost-fixed` | **0.0%** | 98% | 4.19 | 2007.3 | 1557.5 | 912 / 919 | 75 | 3414 |

(The shipped and `advance` rows are the same row because the sweep plays
the **reflex**, and the two reflexes are the same rule under two names:
the shipped one calls it `free_beacon`, the variant's `nearest_ours`, and
both skip the beacons the agent's own squad holds. What `advance` changes
is the **doctrine** — the shipped one camps on a held beacon when no enemy
is near and `advance` does not — and the sweep never sees the gates, only
the reflex. It is the Thor, which plays the model, that tells the two
apart.
`shared-ghost-fixed` is the same row as `shared-free` for a different
reason: the reflex plays the target's *rule*, and the two harnesses name
the same target. The difference between them is how the target is
**stated**, and only the model sees that.)

Three things to read here.

**The shared target ends the draws.** Every shared rule takes the sweep
from 63% draws to 0%, and from 35% of rounds with a fight to 97–98%. The
brief's target — "fights in most rounds" — is met by every one of them, and
by none of the harness-only variants above.

**It is worth more than the whole previous measurement.** The best a
harness change reached was `advance`'s contact in every round and tags in
one round of three. A shared target gives 4.19–5.45 tags a round over a
hundred rounds — seven to nine times `advance`'s 0.62 — and the hot zone
gives the most of any harness ever measured on this sweep.

**The cost is a squad held off the board.** 45–76 of the 100 rounds leave a
squad under 200 points. The hot zone pays the least of the three (45), and
the two fixed targets pay the most (75 and 76). A squad that walks to the
same beacon as its enemy and loses it scores nothing, and the hot zone
moves on before that can settle.

### The real Thor (3 seeds, 3000 ticks)

| harness | seed | ticks | end tick | tags | sight ticks | scores | margin | draw | decisions | asks | lines | urgent | refusals |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `advance` | 1 | 2410 | 2409 | 3 | 174 | [1156, 1802] | 646 | false | 191 | 208 | 15 | 0 | 26 |
| `advance` | 2 | 2559 | 2558 | 1 | 156 | [676, 1800] | 1124 | false | 278 | 284 | 13 | 0 | 24 |
| `advance` | 3 | 3000 | – | 0 | 712 | [1004, 1376] | 372 | false | 322 | 341 | 3 | 0 | 24 |
| `shared-free` | 1 | 3000 | – | **6** | 184 | [25, 1453] | 1428 | false | 284 | 314 | 4 | 0 | 31 |
| `shared-free` | 2 | 2736 | 2735 | **6** | 224 | [1800, 7] | 1793 | false | 279 | 313 | 16 | **1** | 29 |
| `shared-free` | 3 | 3000 | – | 4 | 199 | [1244, 5] | 1239 | false | 297 | 330 | 6 | 0 | 31 |
| `shared-centre` | 1 | 2736 | 2735 | **6** | 184 | [25, 1801] | 1776 | false | 390 | 385 | 10 | 0 | 25 |
| `shared-centre` | 2 | 2736 | 2735 | **6** | 224 | [1800, 7] | 1793 | false | 314 | 303 | 8 | 0 | 24 |
| `shared-centre` | 3 | 2702 | 2701 | 4 | 199 | [1801, 5] | 1796 | false | 280 | 279 | 6 | 0 | 30 |
| `shared-ghost` | 1 | 3000 | – | **6** | **478** | [268, 1071] | 803 | false | 198 | 231 | 9 | 0 | 29 |
| `shared-ghost` | 2 | 2640 | 2639 | **6** | 342 | [1800, 362] | 1438 | false | 133 | 164 | 2 | 0 | 31 |
| `shared-ghost` | 3 | 2682 | 2681 | 1 | **465** | [1801, 31] | 1770 | false | 157 | 189 | 4 | 0 | 28 |
| `shared-ghost-fixed` | 1 | 3000 | – | 6 | 184 | [25, 1463] | 1438 | false | 279 | 306 | 2 | 0 | 26 |
| `shared-ghost-fixed` | 2 | 3000 | – | 6 | 224 | [1251, 7] | 1244 | false | 427 | 460 | 31 | **2** | 17 |
| `shared-ghost-fixed` | 3 | 3000 | – | 4 | 199 | [1588, 5] | 1583 | false | 433 | 467 | 26 | **3** | 18 |

(The `advance` row here is a **fresh measurement** of the same arm, taken
with the refusal counters this section added, and it does not reproduce the
`advance` rows of the table above: seed 1 ends at 2409 with 174 sight ticks
here and ran the whole window with 802 there, and seed 3 is decided there
(at 2628, [1355, 1800]) and still level here at [1004, 1376]. The Thor is a
live model on a shared box and a round is not a replay — it samples, it
waits behind other tenants, and a decision that lands one tick later can
move the round. So the two tables are two samples of the same arm, not a
contradiction, and every comparison inside this section is between arms
measured **in the same run**, where the difference is the harness and not
the day.)

Over the three rounds:

| harness | tags | sight ticks | rounds decided | draws | urgent lines | refusals | the model followed the branch |
|---|---|---|---|---|---|---|---|
| `advance` | 4 | 1042 | 2 of 3 | 0 | 0 | 74 | 609 of 702 (86.8%) |
| `shared-free` | **16** | 607 | 1 of 3 | 0 | **1** | 91 | 781 of 821 (95.1%) |
| `shared-centre` | **16** | 607 | **3 of 3** | 0 | 0 | 79 | 817 of 850 (**96.1%**) |
| `shared-ghost` | 13 | **1285** | 2 of 3 | 0 | 0 | 88 | 469 of 473 (**99.2%**) |
| `shared-ghost-fixed` | **16** | 607 | 0 of 3 | 0 | **5** | 61 | 992 of 1080 (91.9%) |

And what the model chose, over the same move asks:

| harness | the options it chose |
|---|---|
| `advance` | beacon 0 ×117, beacon 1 ×115, beacon 2 ×232, beacon 3 ×202, hold ×3, retreat ×31, follow ×2 |
| `shared-free` | beacon 0 ×680, beacon 2 ×122, hold ×13, retreat ×6 |
| `shared-centre` | beacon 0 ×828, hold ×16, retreat ×6 |
| `shared-ghost` | beacon 0 ×113, beacon 1 ×122, beacon 2 ×119, beacon 3 ×101, hold ×2, retreat ×16 |
| `shared-ghost-fixed` | beacon 0 ×820, beacon 1 ×1, beacon 2 ×228, hold ×20, retreat ×11 |

### What the model did with it

**The model goes where the facts say to go, whatever the facts name.** Over
the asks where the facts named a target, the model chose that target on
**96–98%** of them — and that is true of the shipped target as well as of
every shared one:

| harness | asks where the facts named a target | the model chose it | of those, asks where the target was **not** beacon 0 | chose it |
|---|---|---|---|---|
| `advance` | 681 | 666 (97.8%) | 563 | 549 (97.5%) |
| `shared-free` | 821 | 802 (97.7%) | 136 | 122 (89.7%) |
| `shared-centre` | 850 | 828 (97.4%) | **0** | — |
| `shared-ghost` | 473 | 455 (96.2%) | 360 | 342 (95.0%) |
| `shared-ghost-fixed` | 1080 | 1049 (97.1%) | 256 | 229 (89.5%) |

This is the lever the research predicted would work, and it works: the
shared target is a sentence the facts state per beacon, and the model reads
a sentence. The shared target does not make the model *more* obedient — the
shipped target already had 97.8% — it makes the target the **same beacon
for both squads**.

**And it goes where the target is, and only there.** `advance`'s model
spread itself over the four beacons (117, 115, 232, 202); every shared
variant's model chose **only** the shared target — `shared-free` chose
beacon 0 on 680 asks and beacon 2 on 122 and nothing else; `shared-centre`
chose beacon 0 on 828 asks and nothing else. The hot zone's model followed
the zone as it walked, splitting its 455 target asks evenly over the four
beacons (113, 122, 119, 101) — which is what the zone does, and what the
model does with it.

**The contact is real.** 16 tags in `shared-free`'s and `shared-centre`'s
three rounds against `advance`'s 4, and the hot zone keeps both squads in
sight for 1285 ticks against `advance`'s 1042 and the fixed targets' 607.
The urgency gate — which had nothing to be urgent about under the shipped
doctrine — **fired**: `shared-free` seed 2 is the round with the most
sustained contact in this measurement, and it is the round that answered
urgent on a talk ask and delivered an urgent line.

### The finding: a fixed shared target is too much

The draws are gone and the contact is there, but the rounds are now
**routs**. `shared-free` and `shared-centre` end with one squad on 5, 7 and
25 points — one squad is held off the board in six rounds out of six — and
the sweep says the same thing at scale (75 and 76 collapsed rounds of 100).
The mechanism is visible in the end positions: under `shared-centre` seed 3,
all four of squad 0's agents are stacked on beacon 0 and squad 1's are in
the far corner on 5 points.

That is what "too shared" means. A target both squads walk to is a target
one of them takes, and the other is left with nothing to do and nowhere to
score. The hot zone is the same idea with the one change that matters: it
**moves**. The place worth fighting over walks from beacon to beacon every
600 ticks, so the squad that is losing is never pinned to a ground it has
already lost — the hot zone's collapse count is 45 rounds against 75 and
76, its seed 1 ends [268, 1071] where the fixed targets end [25, 1453] and
[25, 1801], and it holds the most contact of the lot.

The hot zone is not the only thing that keeps a squad in the game: the
pinned field (`shared-ghost-fixed`) is the same game as the pinned name
(`shared-free`) and the same rout. What the hot zone buys is that the
**rout stops being the whole story** — 45 collapsed rounds of 100 is still
a lot, but it is a third fewer than the fixed targets', and its rounds
have margins (803, 1438, 1770) where the fixed targets' are all near the
same 1800.

### The representation: a plain target against a field

The addendum asks for the shared target tried as a **ghost field**, and for
the field compared with the plain shared-target fact on the same metrics.
That is what `shared-ghost-fixed` is for: the same target as `shared-free`
— the lowest-numbered beacon no squad holds, moving to the next one when
it is taken — stated as a field instead of as a name. `shared-ghost` is the
same field with the hot zone walking.

The headless sweep cannot tell them apart, and that is the point: the
reflex plays the target's **rule**, and the two rules are the same, so the
two rows are identical to the digit (4.19 tags, 2007.3 sight ticks, 912 /
919 mean scores, 75 collapsed rounds). A sweep can measure a rule; it
cannot measure a **representation**, because the representation is what the
model reads and the reflex never sees.

The Thor is where the two are separable, and the answer is that the
representation does not make the game. The same target, named plainly or
stated as a field, gave **the same game**: 16 tags and 607 sight ticks in
both arms, seed for seed, and the same rout at the end of it ([25, 1463]
and [1251, 7] against [25, 1453] and [1800, 7]). The field's extra content
— the pull's strength, its radius, its falloff, the pulse — is content the
model does not need in order to take the target, and it takes the target
just as faithfully as it takes a plain named one (1049 of 1080 asks against
802 of 821).

That equality is exact wherever the model does not sample: the **stub**
control below plays the same two arms and reproduces the same tags, the same
sight ticks and the same scores, seed for seed. On the live Thor it is the
*shape* that holds rather than the counts — a re-run of the two arms gave
14 tags and 955 sight ticks under the field against 16 and 607 under the
plain name, with the losing squad on 25, 7 and 5 points in both. The Thor
samples, and a longer fight is a different sample of the same fight.

What the field *does* change is how much the agents think and say. The
field arm asked 1233 times against the plain arm's 957, delivered 59 lines
against 26, and delivered **5 urgent lines against 1** — the most urgent
lines of any arm measured, on the arm with the same game underneath. The
lines and the urgent lines are the part of that which repeats (a re-run
gave 58 lines against 25 and 5 urgent lines against 3); the *ask* counts are
not (the same re-run gave the field arm 983 asks against the plain arm's
1107), so the ask gap is measured on the stub, not on the Thor.

The **stub control** says why, and it is the model's load that is not the
reason. The stub (`ENGAGEMENT_BACKEND=stub`) is the same round with the
model replaced by one that answers at once and always takes the first
option of the menu, so the arms' ask counts are a property of the facts
and the policy alone:

| arm (stub) | asks a round | decisions | lines | refusals |
|---|---|---|---|---|
| `shared-free` | 2194 / 2664 / 2551 | 7625 / 7586 / 7393 | 337 / 337 / 193 | 16 / 55 / 16 |
| `shared-ghost-fixed` | **3200 / 3200 / 3200** | 3186 / 3192 / 3188 | 27 / 42 / 25 | 4856 / 5052 / 5169 |

The field arm asked more with **no model on the Thor at all**. And 3200 is
exactly the round's whole model-call budget — `budget_calls_per_round` is
400 per agent and there are eight agents — so the field arm spent the
budget and every later request was refused by it, which is where its
thousands of refusals come from. A re-run of the stub reproduces that
exactly (3200 / 3200 / 3200 asks and 4854 / 5052 / 5169 refusals for the
field arm, 2136 / 2682 / 2543 asks for the plain one): the budget runs out
the same way every time, and the plain arm stays well inside it.

The likely mechanism is the field's own doing, and it is the kind the
architecture makes easy: the pull is a function of the tick as well as of
the distance, it is in the facts' fields, and the facts' fields are what
the situation key is hashed over (D25). A field that **breathes** makes the
situation key move even when the world does not, and the verdict cache
that keys on it hits less often — so the same moment is asked about again.
A plain target's key moves only when the agent's own situation moves.

So the answer to "is the field worth it?" is: **not for the game.** What
makes the game is the **movement**, which the field can carry and a plain
name cannot. What the field costs is a hotter key, and that cost is paid
in model calls.

### Which representation reads better

For a **model**, the plain target. "The shared target is beacon 0: the
beacon no squad holds" is one clause about one fact, checkable against one
sentence the facts builder already writes. The field's version asks the
model to hold a target *and* a pull in mind, and the measurement says it
does not need the second: on the same target, the model takes the field's
target at the same rate it takes the plain one, it makes the same moves,
and the game plays out the same. The extra sentences are read and then not
used.

For a **viewer**, the field is the better representation, and for a
reason the plain target cannot have. A plain target is a point: there is
nothing to draw, so a viewer sees a marker on a beacon and learns only
that both squads are going there. A field is a *shape* over the map — an
attractor with a radius and a falloff — and if it were drawn faintly, the
viewer would see a soft well centred on the hot beacon, its depth reading
as the pull's strength at that cell. The pull band the facts already state
("strong where you stand") is the legend for that well: it is why *this*
agent is leaning toward *that* part of the field. And a hot zone that
walks would read as a well that moves, which says something a marker never
can — that the objective itself is moving, and that the next fight is
wherever the light is next.

So: the field is the better thing to *draw* and the worse thing to *ask*.
The recommendation follows from that.

### The recommendation

**Ship the shared target as a harness parameter. Do not turn it on by
default, and do not ship it on the plain `free` or `centre` rules.**

The lever works, and it is the lever the research named: the facts builder
now states a second, shared fact, the model goes where it says on 96–98% of
the asks where it names a target, the draws go from 63% to 0%, and the tags
go from 0.62 a round to 4.19–5.45. That is the brief's target — fights in
most rounds — reached for the first time in this programme.

But the honest reading of the Thor is that **a fixed shared target trades a
draw for a rout**, and a rout is not a better game: six rounds out of six
end with one squad on under 30 points. Of the three rules, only the
**hot zone** (`ghost`, with the zone walking) keeps the contact *and* keeps
both squads in the game — 45 collapsed rounds of 100 against 75 and 76, and
the most contact of any harness measured. So the rule to pursue is the
walking one, and the rule to avoid is the fixed one, whatever it is named.

And the field is worth shipping as a **representation** for the same reason
it is worth drawing: it costs the model nothing that shows in the game —
the same target, the same contact, the same rout — and it buys the viewer a
thing a marker cannot show. It is not worth shipping as the thing that makes
the game — the movement is.

Two things are worth saying about what is *not* in this recommendation.

**The default stays off.** `assets/harness/default` has no `facts.toml`,
the parameter is off, and nothing about the shipped game changes. The
measurement says the shared target should be the doctrine's target, but the
measurement is three rounds on one map against a 2B model, and the sweep
plays a reflex. That is enough to justify the parameter and to justify
measuring the hot zone harder — not enough to move the default on the
strength of it.

**The hot zone needs one more measurement before it is a doctrine.** It is
the best arm on every aggregate in the sweep and the most faithful on the
Thor, and it is also the arm the sweep and the Thor agree about least: the
sweep gives it the most tags (5.45) and the Thor gives it fewer than the
fixed targets (13 against 16), because the sweep plays the reflex's walk
and the Thor plays the model's. Before the hot zone is the shipped
doctrine it wants its own hundred-seed sweep at the round's own time limit,
and a Thor pass with more seeds than three.

### What this does not say

- **The sample is small on the Thor.** Three seeds and a 3000-tick window is
  three rounds per harness against the sweep's 100 seeds and 9000 ticks. The
  Thor numbers say what happened in three rounds; the sweep says what happens
  in a hundred.
- **The Thor does not replay.** A re-run of the two arms that carry the
  representation comparison (`shared-free` and `shared-ghost-fixed`, seeds 1-3,
  the same day) reproduced `shared-free` exactly — 6, 6 and 4 tags, 184, 224 and
  199 sight ticks, 16 tags and 607 sight ticks over the three rounds — and
  reproduced the rout in every round of both arms (the losing squad on 25, 7
  and 5 points). What it did not reproduce is the per-seed counts of the field
  arm (14 tags and 955 sight ticks against the 16 and 607 of the run above),
  the ask counts, or the urgent lines of the plain arm (3 against 1). The
  sweep and the stub control are the reproducible measurements; the Thor's
  counts are one sample of a live model, and only the invariants — the
  target, the contact, the rout — travel from one run to the next.
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
- **The Thor was shared while these rounds were played.** Another job was
  using the Thor's ports at the same time, which shows up in the round
  trip: a mean of 415 ms over `advance`'s 702 move asks, against the
  20–105 ms the Thor's own index advertises. The refusals are counted and
  reported for exactly this reason, and the breakdown says what they were:
  they are the round's own housekeeping — a request that expired while it
  was queued, one the mind replaced with a newer one — and not the model
  failing to answer. The **stub** control (the same round with the model
  replaced by one that answers at once) refuses as many, and more, with no
  load on the Thor at all.
- **`shared-centre` cannot show that the model applied the shared target.**
  On the round's symmetric map the most central beacon is beacon 0, and
  the decision menu's **first** option is always beacon 0. So this arm's
  model chose the shared target on 97.4% of its asks *and* took the first
  option of the menu on the same asks — the two readings are the same
  reading here, and this measurement cannot tell them apart. The arms
  whose target moves (`shared-free`, `shared-ghost`) can, and they do:
  when the target was not beacon 0 the model still went to it on 89.7% and
  95.0% of those asks.
- **The sweep's shared rows are the reflex's, not the facts'.** The sweep
  plays a Rhai reflex; it cannot read a facts text, so it cannot tell a
  plain target from a field, and it cannot tell a fixed target from a
  walking one except through the rule the reflex encodes. Every claim about
  the *representation* in this section is a Thor claim, and every claim
  about the *rule* is a sweep claim. Where the two disagree, both are
  reported.

