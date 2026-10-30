# RL-SPIKE: can reinforcement learning train a squad policy, and can a directive coach it?

*A measurement, not a change. Nothing in the game's defaults moves, the golden does not
move, and nothing is written into a flow record or a ledger. The bench is
`crates/lm-learn/examples/rlspike.rs`.*

## The short answer

**Yes to the first question. The second one is "not in this game — and the bench can say
exactly why, and exactly what it would take".**

* **RL trains a squad policy that beats the shipped reflex** on held-out seeds, by the
  seed farm's own rule: **29 of 32 seeds up, sign test p = 1.278e-6, adopted** on both
  `hold_ticks_squad0` and `margin_squad0`. The policy holds 186 beacon-ticks where the
  reflex holds 132. And it finds the tactic on its own — hold the objective you have
  instead of leaving it — which is the tactic D60 predicted from the outside. The same
  bench reaches the verdict through `lm-learn`'s own evaluator, and the rounds it plays
  are hash-for-hash the rounds `lm-learn`'s evaluator plays.
* **A directive changes the reward. In the shipped game it does not change the
  behaviour**, and the reason is measured rather than guessed: the three metrics the
  flow measures are *affinely dependent* here. `tags_for_squad0` is identically zero
  because the squads never meet, and the other squad's beacon-ticks do not move when we
  move, so `margin_squad0` is `hold_ticks_squad0` minus a constant. Every directive
  therefore induces the *same ordering* over the policies the search can reach, so
  every directive lands on the same policy: four directives, four rewards spanning
  three orders of magnitude (205 / 58 / 6 005 / 263), one policy, one behaviour — from
  a cold start and from a warm start alike. A directive is a lever with nothing to pull
  on.
* **The lever bites as soon as the game gives the metrics something to disagree
  about.** In a small world where the squads actually meet, they contest the same
  beacon — and then the opponent's beacon-ticks *do* depend on ours, the tag metric
  comes alive, and the directive moves the behaviour. The bench measures the threshold
  (a tag has to be worth roughly 150–300 beacon-ticks) and then shows the shift: the
  "reward contact" directive trains a policy that lands 7 tags where the objective-
  shaped directives land 3, at the cost of the objectives. The trainer was never the
  problem; the reward had nothing to discriminate.
* **A learned policy does not break the mirror.** D63 says decisiveness has to come
  from breaking it. Measured: policy against policy is 30 level out of 32, because the
  policy is the same table on both sides and the world is point-symmetric. Learning
  does not create an asymmetry — a policy that knows *which squad it is* does.

Two more things the bench turned up that the flow needs to know:

* **`tags_for` is inert in this game.** The squads never meet, so the tag metric cannot
  move, and a claim sealed on it cannot be evidence. That is a property of the shipped
  game, not of the trainer, and it is measured, not assumed.
* **The hand-written rule beats the learned one.** `hold` — a rule a person wrote by
  watching a round — is adopted on 32 of 32 seeds and holds 197 beacon-ticks; the
  learned policy is adopted on 29 of 32 and holds 186. The search *rediscovers* the
  tactic; it does not improve on it, and trained on a handful of seeds it overfits
  them.


## The setup

### The policy is a table

An observation is four buckets:

| bucket | values | what it sees |
|---|---|---|
| `t` | 0, 1, 2 | the fresh threats near me: none, one, two or more (a fresh sighting is an enemy seen within the last 6 ticks and within 6 cells) |
| `a` | 0, 1, 2 | the squadmates near me: none, one, two or more (not stunned, within 4 cells) |
| `h` | 0, 1 | whether my squad holds a beacon |
| `e` | 0, 1 | whether my squad remembers an enemy at all, fresh or stale |

That is 36 observations. The policy is a table with one action per observation, and there
are seven actions, each of them an answer the reflex contract already allows — a cell to
walk to, or a hold:

| action | the answer |
|---|---|
| `hold` | `#{ hold: true }` |
| `charge` | walk to the nearest fresh enemy |
| `flee` | walk four cells away from the nearest fresh enemy |
| `take` | walk to the free beacon nearest to me |
| `defend` | walk to the beacon of my squad that a fresh enemy is nearest to (the one nearest to me breaks the tie) |
| `hunt` | walk to the nearest enemy my squad remembers, fresh or stale |
| `spread` | walk to the free beacon *second* nearest to me, to spread the squad over two objectives |

The buckets are the reflex's own rule, quantised: the shipped reflex counts allies
exactly, and the table buckets them at "two or more". That is a real loss of
information — in a game with more than three squadmates it would change the rule — but
in this game it changes nothing, and the bench measures that: the `reflex-like` table
below reproduces the shipped reflex *exactly*, on every held-out seed.

### The policy runs inside the sim, not beside it

The trained table is compiled into a **Rhai reflex** and played by the same sim, in the
same sandbox, that plays the shipped reflex. `lm-core` calls the reflex with a fresh
scope, so the table has to live *inside* the function — a top-level `let`, and even a
top-level `const`, are invisible to it. That is a detail worth writing down, because the
first version of this bench produced a policy that answered nothing and scored a silent
zero.

Because the policy is a pure integer function of the score view, a round played with it
replays bit for bit: the determinism D69 proves across architectures holds for a learned
policy too, because there is nothing in it that is not an integer. The `verify` command
checks it, and it checks the other half too — that the round the flow's own evaluator
plays is *this* round, hash chain for hash chain.

### The reward is the round's own metrics

A directive is a set of weights over the metrics the flow already measures:

```text
reward = hold_weight   · hold_ticks_squad0
       + tags_weight   · tags_for_squad0
       + margin_weight · margin_squad0
```

Changing the weights between rounds *is* the coaching. Nothing else moves: the same
search, the same training seeds, the same horizon, the same generator seed.

### The search is a (1+λ) evolution strategy over the table

The parameters are the table's entries. Every generation proposes one offspring per
(observation, action) pair — the parent with that one entry changed — 216 of them, and
they are all judged in parallel across the cores, the way the seed farm does it. The
changes that each improved the parent on their own are then applied **one at a time**,
each kept only if it is still an improvement where the last one landed, because two
changes that each help can hurt together.

There is no PyTorch, no autodiff and no gradient: a round is a deterministic function of
(seed, policy), the reward is an integer, and the table is small enough to search
directly.

The parallelism is **flat**: the offspring are split between eight workers, and each
worker plays its own offspring's seeds one after another. The parallelism never nests,
so the spike holds eight worker threads and no more — the box is shared with other jobs,
and the brief caps this one at eight cores.

## How to run it

The bench is one example in `lm-learn`, and it writes nothing but a scratch policy under
`runs/`, which is already gitignored.

```text
cargo run --release -p lm-learn --example rlspike -- probe [seeds] [ticks] [world]
    the shipped reflex against itself: what the game is worth on a block of seeds

cargo run --release -p lm-learn --example rlspike -- candidates [seeds] [ticks]
    the hand-written tables, and the trained policy, against the shipped reflex

cargo run --release -p lm-learn --example rlspike -- train [seeds] [generations] [directive] [ticks]
    train a policy and judge it on the held-out seeds; writes runs/rlspike/policy.rhai

cargo run --release -p lm-learn --example rlspike -- judge [seeds]
    judge the trained policy with lm-learn's own evaluator -- the call the flow makes

cargo run --release -p lm-learn --example rlspike -- verify
    the trained policy's rounds are bit-exact, and are the flow's rounds

cargo run --release -p lm-learn --example rlspike -- coach [seeds] [generations] [cold|warm]
    the same search under four directives, and what each policy does

cargo run --release -p lm-learn --example rlspike -- lever [seeds] [world]
    how much a tag has to be worth before a directive moves the choice

cargo run --release -p lm-learn --example rlspike -- scrimmage [seeds] [generations]
    the same experiment in the scrimmage world, where the squads meet

cargo run --release -p lm-learn --example rlspike -- check [seeds]
    the hand-written tactics, judged against the shipped reflex by the flow's own rule

cargo run --release -p lm-learn --example rlspike -- mirror [seeds]
    both squads playing the trained policy

cargo run --release -p lm-learn --example rlspike -- trace [seed] [ticks]
    where the agents of a round actually go
```

The directives are `hold`, `margin`, `deny`, `contact` and `base`. The trainer's
generator is seeded, so a training run is repeatable: the same seed gives the same
search and the same policy.

## What the game actually allows

Three measurements shaped the design, and all three were surprises.

**The squads never meet.** In the shipped world — eight chunks a side, 256 m square —
the nearest beacon to each spawn is its *own* outer beacon and the contested middle is
further away, so each squad walks to its own side of the map and stays there. The tag
metric is zero for every policy and every horizon tried — 1 500, 3 000 and 6 000 ticks —
and for the shipped reflex and for every learned policy in this bench. Contact only
happens when *both* squads leave their own objective at the same time.

**The reflex does not stay on the beacon it has.** `free_beacon` looks for a beacon my
squad does *not* hold, so the moment the squad takes one, its agents set off for the
next one. That is D60's finding, and the trace shows it: on seed 900 000 the reflex is
still walking at tick 1 250 with a score of zero, it reaches the radius of its
objective at about tick 1 350, and by tick 1 499 it is at (34, 64) — two cells from the
beacon at (32, 63), which is exactly the beacon radius (4 m at 2 m a cell) — with 145
beacon-ticks. It does not stop there: by tick 1 750 it has walked off to (29, 62) and
picked up another beacon on the way.

**The learned policy stops.** The same trace, with the trained policy on squad 0 and the
same shipped reflex on squad 1, is on the objective at tick 1 500 with 191 beacon-ticks
and stays there: (29, 62) and (30, 63) at ticks 1 500, 1 750, 2 000 and 2 999, with the
score rising one point a tick the whole way. The reflex's own rule sends it to the
*next* beacon; the learned table sends it to the one its squad has.

The world's paths are long — the agents walk for more than four fifths of the round
before they arrive — so this one difference is worth 65 beacon-ticks on the held-out
seeds: 197 for the hand-written `hold` rule and 186 for the learned policy, against
132 for the reflex.

## The numbers

All of it on this box: 14 cores, the spike using 8 of them, the box shared and loaded
the whole time (a soak and another job's test suite ran alongside it). These are wall
times on a busy machine, so they are upper bounds.

### The baseline, the shipped reflex against itself

32 seeds at 1 500 ticks — the training seeds, which is where `probe` plays:

| | squad 0 | squad 1 |
|---|---:|---:|
| beacon-ticks | 129 | 129 |
| tags | 0 | 0 |
| margin | 0 | |

31 of the 32 rounds were level, one was won by squad 0 and none was lost. The mirror is
exact at this horizon, which is D63's finding in its purest form. (On the *held-out*
seeds the same baseline is 132 beacon-ticks; the seeds differ, the mirror does not.)

### The training

The directive "lock down the objectives" — the reward is `hold_ticks_squad0` and nothing
else — over 8 training seeds at 1 500 ticks, 8 generations:

| generation | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| reward | 160 | 206 | 206 | 206 | 206 | 206 | 206 | 206 |

The random start was worth 0. The search finds the tactic in two generations and does
not move after that. The wall time was **1 181 s** — 19.7 minutes of 8 cores, 216
offspring per generation judged on 8 seeds.

What it found, in the six observations this game can actually reach (no enemy is ever
seen, so `t` is always 0 and `e` is always 0):

| observation | action | what it means |
|---|---|---|
| no ally near, not holding | `hold` | stand still |
| one ally near, not holding | `take` | walk to the nearest free beacon |
| two allies near, not holding | `take` | walk to the nearest free beacon |
| no ally near, holding | `hold` | stand still |
| one ally near, holding | `defend` | stay on the beacon my squad holds |
| two allies near, holding | `hunt` | hold — there is no remembered enemy to hunt |

That is D60's tactic, written out: *take the nearest objective, and when you hold one,
keep it*. The search was never told any of it.

### The trained policy on the held-out seeds

32 held-out seeds (the block the flow itself judges, `eval_seeds(&[], 32, 1)`), squad 0
playing the trained policy and squad 1 playing the shipped reflex:

| | the trained policy | the shipped reflex | delta |
|---|---:|---:|---:|
| beacon-ticks | 186 | 132 | +54 |
| tags | 0 | 0 | 0 |
| margin | +54 | — | — |

The policy's reflex was clean: 0 errors, 0 refusals, 0 runs out of fuel, over all 32
seeds. Every one of those rounds is a round the same sim, in the same sandbox, played
with the same rules as any other.

### The judgement, by the seed farm's rule

The bench's own judge and **`lm-learn`'s own evaluator** — the call the flow makes — give
the same verdict, seed for seed:

| metric | seeds up | sign test p | the seed farm's rule |
|---|---:|---:|---|
| `hold_ticks_squad0` | 29 of 32 | 1.278e-6 | **adopted** |
| `tags_for_squad0` | 0 of 32 | 1.000e0 | rejected |
| `margin_squad0` | 29 of 32 | 1.278e-6 | **adopted** |

29 of 32 is above the three-in-four the rule asks for, and the sign test on the
direction is p = 1.278e-6, far under 0.05. The per-seed deltas are

```text
[71, 45, 50, 110, 81, 45, 81, 81, -34, 81, 75, 95, 60, 63, 64,
 60, 45, 65, 62, 75, -49, 41, 45, 45, 81, 54, 81, 45, -99, 81, 20, 110]
```

Twenty-nine up, three down, and the three down are the ones to look at: the policy is
not a reflex, it is a policy, and on three seeds out of thirty-two the reflex's mirror
happens to suit the map better. That is what the held-out seeds are for.

A note on the rule: SEEDFARM.md *proposes* the count **and** the sign test, and the bench
applies both. The flow's own `verdict_of` applies the count alone today — the sign test
is the half of the rule that is not shipped yet. On this result the two agree, because
29 of 32 clears the count by a wide margin and the sign test is decisive.

### The rounds are the flow's rounds

`verify` checks two things on four held-out seeds, and both hold:

```text
seed 1001000: the same round twice: identical
seed 1001000: the flow's round is this bench's round: identical
```

The first is the replay: the same seed plays the same round twice, hash chain for hash
chain. The policy is a pure integer function of the score view — there is nothing in
it that is not an integer — so the determinism D69 proves across architectures holds
for a learned policy too.

The second is the same check taken through the flow's own evaluator: the round
`lm-learn`'s `judge_seeds` plays is hash-identical to the round this bench plays, so
the two paths are the same game and not two measurements that happen to agree.

And the numbers agree on their own terms. The bench counts the rounds twice — once
with its own counter, which reads the sim's events, and once with `lm-learn`'s
(`RoundMetrics::from_log`, which re-runs the round from its own log) — and the two
produce the *same per-seed deltas* and the same verdict on all 32 held-out seeds. The
bench's numbers are the flow's numbers, not a parallel measurement of the same thing.

### The learned policy against hand-written ones

The honest comparison, all of them on the same 32 held-out seeds at 1 500 ticks against
the shipped reflex:

| policy | beacon-ticks 0 | beacon-ticks 1 | tags 0 | margin |
|---|---:|---:|---:|---:|
| `reflex-like` — the reflex's own rule in the table's buckets | 132 | 132 | 0 | 0 |
| `defend` | 132 | 132 | 0 | 0 |
| `charge` | 132 | 132 | 0 | 0 |
| `hold` — stand on the beacon you have | 197 | 132 | 0 | 65 |
| `hold-and-hunt` | 197 | 132 | 0 | 65 |
| `hunt` | 0 | 132 | 0 | −132 |
| `spread` | 0 | 132 | 0 | −132 |
| `spread-and-hold` | 0 | 132 | 0 | −132 |
| **the trained policy** | **186** | **132** | **0** | **54** |

Three things to read off that table.

**`reflex-like` reproduces the shipped reflex exactly.** The bench checks this seed by
seed, with the flow's own rule, and the deltas are *all zero*:

| tactic | hold: seeds up | sign p | verdict |
|---|---:|---:|---|
| `reflex-like` | 0 of 32 (every delta is 0) | 1.000e0 | rejected — it is the reflex |
| `defend` | 0 of 32 (every delta is 0) | 1.000e0 | rejected — it is the reflex |
| `charge` | 0 of 32 (every delta is 0) | 1.000e0 | rejected — it is the reflex |
| `hold` | **32 of 32** | 2.328e-10 | **adopted** |
| `hold-and-hunt` | **32 of 32** | 2.328e-10 | **adopted** |
| `hunt` | 0 of 32 (all 32 deltas down) | 1.000e0 | rejected |
| `spread` | 0 of 32 (all 32 deltas down) | 1.000e0 | rejected |
| `spread-and-hold` | 0 of 32 (all 32 deltas down) | 1.000e0 | rejected |

`reflex-like` is the shipped reflex's own rule written in the table's buckets, and it
plays the *same game*: not a similar one, the same one, tick for tick on all 32 seeds.
That is the bench's own check that the policy compiler and the observation are right.
(`defend` and `charge` are the reflex too, in this game: a fresh enemy never appears,
so those two rules never fire differently.)

**RL finds the tactic D60 predicted, and does not beat a rule a human writes in five
minutes.** The hand-written `hold` holds 197 beacon-ticks and is adopted on **32 of 32**
seeds (p = 2.3e-10); the trained policy holds 186 and is adopted on **29 of 32**
(p = 1.3e-6). Both are far above the reflex's 132, and the trained one is not the
better of the two. That is the honest headline: the search *rediscovers* "hold what you
have" without being told, and it is the rediscovery, not the discovery, that is worth
having — because the flow can seal a claim on a policy and can seal one on a hand-written
reflex too, but only the policy generalises to states nobody thought to write a rule
for. On this game, though, the hand-written rule is also the better policy, and the
flow should not be told otherwise.

**The search is not the interesting part.** Two generations on eight seeds finds the
tactic a human found by watching a round (D60). What the search buys is the *ability to
choose* between tactics, which is what the coaching experiment is about — and, as the
next section shows, that ability needs something to choose between.

### The mirror: both sides on the learned policy

D63 says decisiveness has to come from breaking the mirror, because the reflex match is
exactly equal. A learned policy is deployed per agent, so it *could* break it. Measured,
on the same 32 held-out seeds:

| match | beacon-ticks 0 | beacon-ticks 1 | margin | ahead | level | behind |
|---|---:|---:|---:|---:|---:|---:|
| reflex vs reflex | 132 | 132 | 0 | 0 | 31 | 1 |
| policy vs reflex | 186 | 132 | +54 | 29 | 0 | 3 |
| reflex vs policy | 132 | 185 | −53 | 3 | 0 | 29 |
| policy vs policy | 186 | 185 | +1 | 1 | 30 | 1 |

**The learned policy does not break the mirror either.** Policy against policy is 30
level and one ahead, a margin of 1 beacon-tick on 1 500. The reason is not subtle: the
policy is the *same* table on both sides and the world is point-symmetric, so the two
squads still mirror each other. Learning does not create an asymmetry — a squad-aware
policy does. That is the open question, and the answer is not "a policy", it is "a
policy that knows which squad it is".

## The coaching experiment

The search, the training seeds, the horizon and the generator seed are the same. The
only thing that changes between the runs is the weights of the reward, which is what a
directive between rounds would change.

### First, how much a tag has to be worth

The reward is a linear form in the round's metrics, so the bench measures the metrics
once and reads the reward off for every weight. That gives the **threshold**: the
weight at which the directive stops choosing the same tactic.

In the **shipped world**, 8 training seeds at 1 500 ticks:

| policy | hold 0 | tags 0 | margin |
|---|---:|---:|---:|
| `reflex-like` | 147 | 0 | 0 |
| `defend` | 147 | 0 | 0 |
| `charge` | 147 | 0 | 0 |
| `hold` | 206 | 0 | 59 |
| `hold-and-hunt` | 206 | 0 | 59 |
| `hunt` | 0 | 0 | −147 |
| `spread` | 0 | 0 | −147 |
| `spread-and-hold` | 0 | 0 | −147 |

The tag column is zero for every tactic, so **no tag weight changes anything**: at
every weight from 0 to 4 800 the winner is `hold`, and the runner-up is `hold-and-hunt`
with a gap of zero. There is no threshold to clear, because the metric the directive
rewards cannot move.

In the **scrimmage world** — one chunk a side, two beacons, the same code — the squads
meet and the tag column comes alive:

| policy | hold 0 | tags 0 | margin |
|---|---:|---:|---:|
| `reflex-like` | 782 | 9 | 344 |
| `defend` | 637 | 9 | 221 |
| `hold` | 2 045 | 4 | 1 867 |
| `hold-and-hunt` | 1 076 | 6 | 779 |
| `spread` | 1 117 | 7 | 419 |
| `spread-and-hold` | 1 900 | 5 | 1 860 |
| `charge` | 732 | 10 | 445 |
| `hunt` | 328 | 5 | −838 |

and the winner moves:

| tag weight | the best tactic | its reward | the runner-up | the gap |
|---:|---|---:|---|---:|
| 0 | `hold` | 3 912 | `spread-and-hold` | 152 |
| 60 | `hold` | 4 152 | `spread-and-hold` | 92 |
| 90 | `hold` | 4 272 | `spread-and-hold` | 62 |
| 150 | `hold` | 4 512 | `spread-and-hold` | 2 |
| 300 | `spread-and-hold` | 5 260 | `hold` | 148 |
| 600 | `charge` | 7 177 | `spread-and-hold` | 417 |
| 1 200 | `charge` | 13 177 | `reflex-like` | 1 251 |
| 2 400 | `charge` | 25 177 | `reflex-like` | 2 451 |

The threshold is at a tag weight of roughly 150–300: below it the directive cannot
afford to give up an objective for a tag, above it the contact-seeking tactic wins.
The obvious calibration — "a tag stuns for 90 ticks, so a tag is worth 90 beacon-ticks"
— sits *below* the threshold, which is why the bench's CONTACT directive weights a tag
at 600: contact worth 40% of an objective held end to end.

### Then: the same search, four directives, in the shipped world

Four directives, 4 training seeds, 3 generations, 1 500 ticks. Two starts are measured,
because a directive can only move the behaviour when there is a choice to move towards:

* a **cold** start begins from a random table, so the search has to find the tactic as
  well as choose it;
* a **warm** start begins from the best of the hand-written tactics, as the directive
  sees them, so the directive has something to choose between.

**Cold start:**

| directive | weights (hold/tags/margin) | the reward it found | hold 0 | hold 1 | tags 0 | margin |
|---|---|---|---:|---:|---:|---:|
| lock down the objectives | 1 / 0 / 0 | 0 → 205 | 186 | 132 | 0 | 54 |
| win the margin | 0 / 0 / 1 | −147 → 58 | 186 | 132 | 0 | 54 |
| hold ours and take theirs | 1 / 0 / 100 | −14 750 → 6 005 | 186 | 132 | 0 | 54 |
| reward contact and tags more | 1 / 600 / 1 | −147 → 263 | 186 | 132 | 0 | 54 |

(the behaviour columns are the 32 held-out seeds at 1 500 ticks, against the shipped
reflex, in the shipped world)

**Warm start:**

| directive | the reward it found | hold 0 | hold 1 | tags 0 | margin |
|---|---|---:|---:|---:|---:|
| lock down the objectives | 205 → 205 | 197 | 132 | 0 | 65 |
| win the margin | 58 → 58 | 197 | 132 | 0 | 65 |
| hold ours and take theirs | 6 005 → 6 005 | 197 | 132 | 0 | 65 |
| reward contact and tags more | 263 → 263 | 197 | 132 | 0 | 65 |

**The reward responds to the directive and the behaviour does not.** Every directive
sends the search somewhere different — the rewards differ by three orders of magnitude
— and every directive lands on the *same policy*, the same behaviour, the same numbers.
The warm start does not change it either: with the tactics in front of it, the
directive still has nothing to choose between.

That is not a bug in the bench, and it is the most useful thing this spike found. The
three metrics are **affinely dependent** in this game, and the dependence is exact:

* `tags_for_squad0` is identically zero, so its weight is worth nothing. The directive
  that "rewards contact and tags more" is the directive that does not reward contact
  at all.
* The other squad plays the *shipped reflex*, a fixed opponent, so its beacon-ticks
  are a constant of the seed, not a function of what we do. `margin_squad0` is
  `hold_ticks_squad0` minus a constant. Any two metrics that are affinely related
  induce the *same ordering* over the policies, whatever their weights — so every
  directive here has the same argmax, and the search finds the same policy every time.

The consequence is sharp: **reweighting the reward cannot move the behaviour until the
game gives the metrics something to disagree about.** A directive is a lever with
nothing to pull on. The threshold table above says exactly what "something to disagree
about" is worth: a metric that is not an affine function of the one the policy is
already maximising.

### And: the cold search overfits its own seeds

The cold start and the warm start reach the *same reward on the training seeds* — 205
for "lock down the objectives" — but they do not reach the same policy. On the 32
held-out seeds the cold search holds 186 beacon-ticks and the warm start holds 197.
The search found a policy that is as good on the four seeds it was given and worse on
the seeds it was not; the hand-written tactic generalises better. That is a second
reason to prefer the human rule here, and a third thing the flow should know: a policy
learned from a handful of seeds needs more seeds than a policy written by a person who
watched a round.

### And: the same experiment, in a world where the squads meet

The scrimmage world is one chunk a side — 32 m square, two beacons — and it is a round
the bench *builds*, not a change to the game: the shipped defaults are untouched. In
it the squads walk onto the same beacons, and two things happen that do not happen in
the shipped world.

First, **the squads contest.** A beacon two squads stand in is held by nobody, so the
opponent's beacon-ticks stop being a constant of the seed and become a *function of
what we do*. That is what breaks the affine dependence: `margin_squad0` is no longer
`hold_ticks_squad0` minus a constant, because holding the middle is what takes the
middle away from them. Second, **the tag metric comes alive**: the reflex against
itself scores 12 tags for squad 0 and 6 for squad 1 over the four training seeds,
against 0 and 0 in the shipped world.

The same search, the same 4 training seeds, the same 3 generations, the same horizon —
only the reward's weights differ. **Cold start:**

| directive | the reward it found | hold 0 | hold 1 | tags 0 | tags 1 | margin |
|---|---|---:|---:|---:|---:|---:|
| lock down the objectives | 1 → 1 727 | 738 | 1 841 | 2 | 2 | −1 103 |
| win the margin | −1 784 → 2 156 | 1 513 | 299 | 4 | 5 | 1 214 |
| hold ours and take theirs | −178 448 → 218 006 | 1 513 | 299 | 4 | 5 | 1 214 |
| reward contact and tags more | −1 183 → 10 522 | 949 | 303 | 5 | 5 | 646 |

(the behaviour columns are the 32 held-out seeds at 1 500 ticks, against the shipped
reflex, in the scrimmage world)

**The lever bites.** Read the three behaviours against each other:

* **"lock down the objectives"** hoards one objective and *loses* the round's margin:
  738 beacon-ticks against the reflex's 1 841, a margin of −1 103. The directive said
  "hold", the policy holds, and holding costs it the contested middle.
* **"win the margin"** and **"hold ours and take theirs"** take the middle instead:
  1 513 beacon-ticks against the reflex's 299, a margin of +1 214. Same game, same
  search, same seeds — a different reward, a different policy, the opposite result.
* **"reward contact and tags more"** lands the most tags (5) and still wins the margin
  (+646): it trades holding for contact, because the directive told it to.

So the answer to the brief's question is measured, not argued: **a directive moves the
trained behaviour exactly when the game gives the metrics something to disagree about.**
In the shipped world they have nothing to disagree about and the directive is inert; in
the scrimmage world the contested middle and the tag count give it something to pull
on and the behaviour follows the directive.

**Warm start**, the same experiment again, starting from the best of the hand-written
tactics as each directive sees them:

| directive | the reward it found | hold 0 | hold 1 | tags 0 | tags 1 | margin |
|---|---|---:|---:|---:|---:|---:|
| lock down the objectives | 2 076 → 2 479 | 2 184 | 71 | 3 | 2 | 2 113 |
| win the margin | 1 973 → 2 470 | 2 184 | 71 | 3 | 2 | 2 113 |
| hold ours and take theirs | 199 401 → 249 479 | 2 184 | 71 | 3 | 2 | 2 113 |
| reward contact and tags more | 9 795 → 13 503 | 655 | 319 | **7** | **8** | 336 |

The CONTACT directive's learned table is visibly a different animal — it *charges* in
almost every observation, where the other three *take* and then *defend*:

```text
                     not holding          holding
other three:         take                 defend
contact:             charge               charge
```

**655 beacon-ticks but 7 tags, against 2 184 beacon-ticks and 3 tags.** That is the
directive doing exactly what it says: it paid for contact in objectives and got the
contact.

There is one thing the warm start does that the cold start does not, and it is worth
naming: **the warm start collapses the three objective-shaped directives into one
policy.** HOLD, MARGIN and DENY all start from the same hand-written tactic (`hold`),
the search is a local hill climb, and it cannot climb out of that basin — so all three
end at the same policy, 2 184 beacon-ticks and a margin of 2 113. From a cold start the
same three directives land on *different* policies, because the directive decides which
basin the search finds at all. The lever is strongest from a cold start; a warm start
decides the basin before the directive gets a vote.

## The wall time

The spike uses 8 of this box's 14 cores, and the box was shared and loaded the whole
time — a soak and other jobs ran alongside it — so these are upper bounds on an idle
machine. The parallelism is flat: eight worker threads, no nesting.

| what | wall time |
|---|---:|
| one 1 500-tick round, played and counted | 0.55 s of CPU |
| the baseline: the reflex against itself, 32 seeds | 3.4 s |
| the lever: 8 tactics × 8 seeds, and every tag weight | 5.3 s |
| the mirror: 4 matches × 32 held-out seeds | 15.1 s |
| `verify`: 4 seeds, replayed and compared hash chain by hash chain | 22.8 s |
| the judgement through `lm-learn`'s own evaluator, 32 seeds × 3 metrics | 25.9 s |
| the catalogue self-check: 8 tactics × 32 held-out seeds | 42.7 s |
| the hand-written sweep: 9 policies × 32 seeds, each also against itself | 46.7 s |
| one coaching run: 4 directives × 3 generations × 4 seeds | 784 s cold, 828 s warm |
| one scrimmage run: the same, in the scrimmage world | 1 085 s cold, 1 607 s warm |
| the training run: 8 generations × 8 seeds, 1 500 ticks | **1 181 s** (19.7 min) |

A training run is twenty minutes of eight cores. That is the number that matters for
the flow: it is two orders of magnitude more than a round, so RL cannot sit in the loop
of every round — it belongs to a periodic retrain, and the gates that are cheap stay in
the round.

## How it relates to the language-model gates in the flow

### What the flow does today

A round is a chain of files (D64): play, measure, show a reviewer, **seal the prediction
before the test** (D32), evaluate on held-out seeds, record the verdict, and only then
apply the change, commit it and tag the round. The System-2 model proposes one of four
things: a **new reflex** (`EditReflex`), a **new gate** (`AddGate`), a **policy tweak**
(`PolicyTweak` — one key and one integer) or a **lexicon entry** (`LexiconEntry`). The
claim it seals is about a metric the flow measures — `hold_ticks_squad0`,
`tags_for_squad0`, `margin_squad0`.

### Where RL fits, and what it does not replace

A reflex edit is a blob of text, and the text the model writes is usually worse than the
shipped reflex — the reflex plays 63% of the time because the model's answers are late
(D68). But a blob of text also cannot express a *function of the observation*: "hold the
objective you have, unless the middle is contested, in which case spread" is not a
paragraph, it is a table. A `PolicyTweak` cannot either: it is one integer. That is the
gap RL fills, and it is a gap the flow already has a shape for — `PolicyTweak` is
already one of the proposal kinds, and what is behind it today is a scalar.

The division of labour that falls out of this bench is:

> **The model proposes the tactics. The trainer chooses between them and writes the
> policy.**

The model's lever is not the reflex text — a blob of text cannot be a function of the
observation, and it is usually worse than the shipped reflex. The model's lever is the
*menu*: it writes the tactics it thinks are good, and the trainer picks the one the
objective prefers and refines it. That is what the bench measures, and it is the shape
the flow already has a place for: the gates D47 lists are already a menu.

The bench also says what the objective can and cannot do. A directive's weights only
choose between tactics if the metrics disagree; in the shipped game they do not, so a
directive on its own changes the reward without changing the play. That is a property
of the game, and it is the thing to fix before the lever is worth pulling — and the
scrimmage world is what the fix looks like: the same code, a smaller map, and the
directive moves the behaviour.

The evidence discipline does not change. The trained policy is a Rhai reflex, which is
exactly the harness's reflex asset, so it lands as an ordinary change: seal the
prediction, evaluate on held-out seeds, record the verdict, then commit and tag the
round (D65). The bench's `judge` command calls `lm-learn`'s own evaluator — the same
call the flow makes — and the two agree seed for seed, so the numbers that come out of
the bench are the flow's numbers.

### The one thing that gets harder

A gate today seals a claim of the form "this reflex raises `hold_ticks_squad0` by at least
one on the held-out seeds". A directive cannot be sealed that way, because the effect of a
reward is only visible *after* the trainer has run. The claim for a directive has to be
two-stage: "training under this directive yields a policy whose held-out metric is at
least X". That is a design question for the flow, not a detail, and it is the first thing
to decide if RL is to go near the chain.

### The cost changes the cadence, not the chain

A training run is twenty minutes of eight cores; a flow round is seconds. RL therefore
cannot sit in the loop of every round. It belongs to a **periodic retrain** — a sweep of
directives run between rounds or nightly — while the per-round flow keeps carrying the
gates that are cheap. The flow's chain stays exactly as it is; only the *proposal* gets
more expensive to make.

### The finding the flow needs: `tags_for` is inert

`tags_for_squad0` is identically zero in this game: the squads never come within tag
range, at any horizon tried, for the shipped reflex or for any policy this bench
trained. Two things follow.

1. **No gate should seal a claim on `tags_for_squad0`.** The metric cannot move, so the
   claim cannot be evidence — and a claim that cannot be evidence is worse than no
   claim.
2. **A directive that rewards contact has nothing to reward.** The bench's CONTACT
   directive weights a tag at 600, and the tag term in its reward contributes exactly
   nothing in the shipped world, because there are no tags to earn. That is a property
   of the game, not of the trainer, and it is worth fixing in the game before it is
   worth coaching.

## The growth path

**Stage 0 — this bench.** The trainer, the policy compiler, the held-out judge, the
lever measurement and the coaching experiment, in one example, with the game's defaults
untouched.

**Stage 1 — the policy lands as an ordinary change.** The trained table compiles to a
Rhai reflex, so it drops into the existing harness format and the existing chain: a
sealed prediction, the flow's own held-out evaluation, a verdict, a commit, a tag. The
first sealed prediction is the one D60 asks for — "hold what you have" — and this bench
says a *learned* policy beats the reflex on 29 of 32 held-out seeds, so the claim is
strong enough to seal. It also says the hand-written reflex for the same claim is
*better* (32 of 32, and 11 more beacon-ticks), so the first round to seal should be the
human rule — and the learned policy should earn its place on the states the human rule
does not cover.

**Stage 2 — the model proposes the tactic menu.** A new proposal kind in the reviewer:
the tactics to choose between, not the reflex text. The bench measures what the lever
is worth before the flow has to guess — and it measures, first, that the lever does
nothing until the game gives the metrics something to disagree about, and second, what
the threshold is once it does.

**Stage 3 — the directive is the thing the model writes.** The model is good at the
*objective* and bad at the *move*: its own answers are too slow to play (D68). So let
it write the weights — "hold the objectives", "reward contact", "take theirs off them"
— and let the trainer turn those weights into a policy. The bench's `lever` command is
the honest first half of this: it says how much a tag has to be worth before the choice
moves, which is the number a directive needs to be worth anything.

**Stage 4 — a policy that listens.** The score view already carries what the squad knows
*and what it has heard*. A policy that reads the heard lines could answer the model's
talk directly — which is where the language model's contribution actually lives, since
the model's own move decisions are too slow to use. Gates speak; the policy listens.

**Stage 5 — breaking the mirror, for real.** D63 says decisiveness has to come from
breaking the mirror, because the reflex match is exactly equal. This bench measured the
obvious candidate and it does not work: the learned policy against itself is 30 level
out of 32, because the policy is the same table on both sides and the world is
point-symmetric. Learning does not create an asymmetry. What would is a policy that
knows which squad it is — D63's option (b), seeded per-squad variation — and that is a
small change to the observation, not a large one to the trainer.

**Stage 6 — the game, not the policy.** The tag mechanic never fires because the squads
never meet. If contact is wanted — and a tag that stuns for 90 ticks is the game's main
offensive tool — the fix is in the map or the spawns, not in the policy. A better game
makes a better policy possible; it is worth checking that before optimising harder. The
scrimmage world in this bench is what that looks like: the same code, a smaller world,
and the tag metric comes alive.

## What this does not do

* It does not change the game, the flow, the golden or any default. Nothing in the game
  was touched; the whole bench is one example and one markdown file.
* It does not write a ledger line, a deposit or a flow record. It is a bench, not a round,
  and it claims nothing in the chain.
* It does not use the language model. No gate was asked anything here. The point of the
  coach experiment is that the *objective* is the part a language model is good at, and
  the policy is the part it is not.
* It does not show a learned policy beating the reflex when **both** sides use it. The
  mirror table above says the opposite, and says why.
* The reward is a hand-written linear combination of three metrics. Nothing here says
  that is the right objective — only that changing it moves the behaviour *when the
  metrics disagree*, which is what makes it worth optimising.
* The trainer is a hill climb with one observation and one action changed at a time. It
  finds the tactic in two generations here, but it is a hill climb: a landscape with
  ridges would need a real ES, and a bigger table would need a better search.
* The observation is four buckets. It is enough to express "hold what you have" and not
  enough to express "spread unless the middle is contested" — the tables the bench
  learned are mostly unreachable in the shipped game, because the game never puts an
  agent in them.
