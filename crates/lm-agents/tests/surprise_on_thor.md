# The surprise signal on the real Thor

The between-round learning flow proposes changes and tests them under a
sealed prediction, but nothing told it **where** the agents struggle. The
surprise signal is the cheap answer: per situation bucket — a coarse key of
the state an ask was made in — how often the minds were unsure (D72),
refused (D26) or escalated (D71). A situation the models keep failing in is
a situation a gate, a fact or a lexicon entry is missing for (D47: the
trainer's output is questions).

This is the measurement of that signal on the Thor's own **d1-2B**, three
seeds × 3000 ticks, played the way the game plays it (the host loop, the
agents, the least-use-first queue, the sequencer), with the shipped floors:
the d1-2B at 0.30, the escalation at 0.5 to clef-flash at 0.05. The table
is the round's own ([`lm_agents::surprise`]), not a watcher's: the minds
count every answer that comes home, under the bucket of the situation it
was asked in.

## The command

```text
cargo test --release -p lm-agents --test surprise_on_thor -- --ignored --nocapture
```

The Thor's base URL is the config's (`http://192.168.0.78`);
`SURPRISE_THOR=http://localhost` runs it on the Thor itself. The
configuration is `shipped` by default and `split` with
`SURPRISE_BACKEND=split`; `SURPRISE_TICKS` and `SURPRISE_SEEDS` narrow a
smoke run.

## The two configurations

The shipped rounds rarely fight: the squads hold their own beacons, the
enemies never meet, and a surprise table of such a round can only ever say
"everywhere, a bit". So the round is played two ways (the pattern
`urgency_on_thor` set, on the branch that measured it):

| configuration | what plays | what it is for |
|---|---|---|
| **shipped** | the Thor stack answers both packs | the game as it ships |
| **split** | the stub answers the move pack, the Thor stack the talk pack | the fights: the stub's play makes the squads meet, so there are hot situations to be surprised about |

Both configurations register **every** backend the shipped policy names
(the `lm voice` pattern), with the Thor stack serving the `deep` tier that
names `jev`. The first run of this measurement did not: it registered
`thor-d1-2b` only, and the table's top bucket filled with 48 refusals of
`no backend named jev` — every quiet-moment ask the `deep` tier made, refused
by the wiring. The signal caught a missing backend before it said anything
about the model, which is the point of it; the fix was to wire the tier, not
to discount the bucket.

## The key

Five fields, each from facts the agents already get, at most
4 × 4 × 2 × 3 × 3 = 288 buckets:

| field | states | why it is there |
|---|---|---|
| which beacon | off / own / enemy / unheld | the beacon the agent stands on is the game's objective made local; the holder is said, not the index, so the buckets do not split with the world's shape |
| enemy | none / known / near / in sight | the doctrine's own ladder — the model's questions change at each rung |
| stunned | yes / no | a stunned agent's best move is not its calm one |
| score margin | behind / level / ahead | banded at 180 points (six seconds of one held beacon, a tenth of the win score); the exact number is a fact, the band is the situation |
| time | early / mid / late | thirds of the round's hard time limit — the same play means different things in each |

The score is `(unsure gates + refused asks + escalated gates) / asks`
x1000, and a bucket under 5 asks is counted but not ranked, so one bad ask
in a rare bucket cannot sit on top of the table.

## Shipped: the numbers

Three rounds, 3000 ticks each, seeds 1-3:

| seed | lines | decisions | thinks | answered | escalations |
|---|---|---|---|---|---|
| 1 | 15 | 438 | 79 | 75 | 16 |
| 2 | 17 | 387 | 84 | 78 | 20 |
| 3 | 21 | 557 | 175 | 167 | 26 |

320 asks came home in 2 buckets — every ask of the three rounds was made
early in the round, on a level score, with no fresh enemy in sight:

| bucket | asks | unsure (worst) | refused (worst) | escalated (worst) | events/ask x1000 |
|---|---|---|---|---|---|
| no fresh enemy, not stunned, off the beacons | 257 | 16 (attention 16) | 26 (timeout 26) | 27 (attention 27) | 268 |
| no fresh enemy, not stunned, on your beacon | 63 | 2 (subject 2) | 1 (timeout 1) | 35 (attention 20) | **603** |

The top two, as the screen shows them:

- `early round, level on score, no fresh enemy, not stunned, on your beacon: 35 gates escalated over 63 asks (attention: 20)`
- `early round, level on score, no fresh enemy, not stunned, off the beacons: 27 gates escalated over 257 asks (attention: 27)`

The refusals by name: 27 timeouts, and 18 answers just under the 0.5
escalation threshold (0.37-0.49) — the escalations these became are the
table's biggest column.

## Split: the numbers

The stub answers the move pack, so the squads meet and fight; the Thor's
d1-2B answers every talk pack:

| seed | lines | decisions | thinks | answered | escalations |
|---|---|---|---|---|---|
| 1 | 38 | 8 027 | 2 814 | 2 807 | 34 |
| 2 | 17 | 7 664 | 2 858 | 2 852 | 27 |
| 3 | 39 | 8 039 | 2 019 | 2 011 | 33 |

7 670 asks came home in 19 buckets, 15 of them over the ranking floor (the
move packs are the stub's, always confident, so the struggle the table sees
is the talk packs' own):

| bucket | asks | unsure | refused | escalated | events/ask x1000 |
|---|---|---|---|---|---|
| behind, no fresh enemy, off the beacons | 1 341 | 0 | 0 | 5 | 3 |
| level, no fresh enemy, off the beacons | 4 220 | 5 (attention) | 3 (timeout) | 71 (attention 70) | 18 |
| ahead, no fresh enemy, off the beacons | 377 | 0 | 0 | 0 | 0 |
| behind, a fresh enemy known, off the beacons | 46 | 0 | 0 | 0 | 0 |
| level, a fresh enemy known, off the beacons | 807 | 0 | 0 | 4 | 4 |
| level, an enemy in sight, not stunned, off the beacons | 518 | 2 (gesture) | 0 | 6 | 15 |
| level, an enemy in sight, stunned, off the beacons | 47 | 0 | 0 | 0 | 0 |
| ahead, an enemy in sight, stunned, off the beacons | 2 | 2 | 0 | 3 | (2 500: under the floor, not ranked) |
| ahead, no fresh enemy, on your beacon | 14 | 0 | 0 | 3 (subject) | **214** |
| level, an enemy in sight, not stunned, on your beacon | 67 | 0 | 0 | 1 (gesture) | 15 |
| level, an enemy in sight, stunned, on your beacon | 20 | 0 | 0 | 1 (attention) | 50 |
| level, an enemy in sight, stunned, on an enemy beacon | 49 | 0 | 0 | 0 | 0 |
| level, an enemy in sight, not stunned, on an unheld beacon | 78 | 0 | 0 | 0 | 0 |
| level, an enemy in sight, stunned, on an unheld beacon | 60 | 0 | 0 | 0 | 0 |

(The `early round` prefix is every bucket's: 3000 of the round's 9000-tick
time limit is the first third. `on score` clauses dropped for width.)

## What the numbers say

1. **The signal separates the situations the round actually visits, and the
   shipped round visits almost none.** Two buckets over 320 asks, shipped:
   no enemy ever came into sight, no squad ever left the ±180-point band,
   and no round left its first third. The split round's 19 buckets are what
   the table looks like when the game is played — and the shipped game's
   own table is the plainest statement of the "the shipped rounds rarely
   fight" problem a reviewer could be handed.

2. **The model is surest exactly where the game is hottest.** In the split
   round's fight buckets — an enemy in sight, stunned, on an enemy's
   beacon — the d1-2B was unsure or escalated *nothing* over 49 asks. The
   struggle stays in the quiet buckets, and it is the ten-option talk gates
   that carry it: `attention` (70 escalations in the biggest bucket) and
   `subject`. That agrees with D74's finding (the two ten-option talk gates
   carry the escalation), and it says the missing question is not "what do
   I do in a fight" but "who is there to attend to when nobody is" —
   exactly the kind of hole a gate or a fact has to fill (D47).

3. **The one ranked hot-and-quiet bucket names a real asymmetry.** `ahead
   on score, no fresh enemy, on your beacon: 3 of 14 asks escalated
   (subject)` is the split round's top situation (214 per mille over the
   18 of the biggest bucket): standing on a held beacon while ahead is when
   the model least knows who a line is for. The `subject` gate (ten
   options over the roster) has no fact to read when the squad is alone
   and winning — a lexicon or fact candidate, pointed at by a count.

4. **The floor earns its keep.** One bucket (`ahead, an enemy in sight,
   stunned, off the beacons`) scored 2 500 per mille on 2 asks — two unsure
   gates and three escalations over two asks, half of them the same ask.
   It is kept in the table and kept off the top five; without the floor it
   would have been the round's headline.

5. **The refusals the table sees are timeouts and near-threshold
   answers.** Shipped: 27 timeouts of 320 asks (the talk pack against the
   4000 ms deadline on a busy box) and 18 answers between the 0.30 floor
   and the 0.5 escalation threshold. The queue's own refusals (budget
   spent, expired in queue) never reach the table by design: an ask is
   counted when its answer comes home, so the table says what the *model*
   struggled with, not what the plumbing dropped. The one wiring fault the
   first run hit — 48 `no backend named jev` refusals — was counted under
   the bucket it happened in, which is how it was found.

## What this buys the reviewer

The between-round screen's top five is now a reading list with a reason
attached. On this Thor, on the shipped game, it would read: *the attention
gate, when the squad is alone on a beacon it holds* — one line, from
counts the round already had, no model asked.
