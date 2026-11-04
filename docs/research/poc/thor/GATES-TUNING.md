# GATES-TUNING — making the default gates confident enough to drive play (2026-10-29)

The finding this answers (phase 3 Task 7): against the real Thor d1-3B
(`http://192.168.0.78:8093`, System One, no auth) the default question pack
(`assets/harness/default/gates.toml`: a "what now?" choice over the bare menu)
scored **0.20–0.47** confidence, below
`AgentsConfig::min_confidence["thor-d1-3b"] = 0.55`, so nearly every thought was
refused and the reflex played every moment.

The evidence for the fix is E4c ([RESULTS.md](RESULTS.md)): d1-3B scored **0.97**
when the question **stated a rule** over code-computed plain-language facts, and
0.45 on the same facts with an open question. "Which action, given this rule?"
works; "what would you do?" does not.

The numbers below are this document's own measurement, over its own sample of
situations, so they are not the same sample as the finding they answer: the old
pack is asked the same questions again, here, over the situations the test
builds.

## What changed

| file | change |
|---|---|
| `assets/harness/default/gates.toml` | the move gate states the squad's doctrine as an explicit rule over the facts, and every menu option has a criteria description in that rule's wording |
| `crates/lm-agents/src/gates.rs` | a menu gate's criteria come from the file, written **by option kind** (`beacon`, `hold`, `follow`, `retreat`, with `{beacon}` filled in), so the file does not depend on how many beacons the round has. The option keys stay `opt0..optN` |
| `crates/lm-agents/src/facts.rs` | the facts state every term the rule refers to: which beacon is the nearest one your squad does not hold, which beacon you are standing on and whose it is, how many enemies are near and how many of them see you, how many allies are near |
| `crates/lm-host-model/tests/gates_tuning.rs` | **new**: the measurement (live, `#[ignore]d`) |

The doctrine, as the gate now states it:

> Your squad's doctrine, in order: (1) If an enemy can see you within 6 cells and
> you have fewer allies within 4 cells than there are enemies within 6 cells,
> retreat. (2) If you are standing on a beacon your squad holds and no enemy is
> within 6 cells, hold. (3) Otherwise go to the nearest beacon your squad does
> not hold, if there is one; if there is none, hold.

and the criteria, one per menu option, each naming the branch it answers:

```
opt0  go to beacon 0 (the doctrine's third branch: go to the nearest beacon your squad does not hold)
opt1  go to beacon 1 (the doctrine's third branch: ...)
opt2  go to beacon 2 (the doctrine's third branch: ...)
opt3  hold position (the doctrine's second branch: you are standing on a beacon your squad holds and no enemy is within 6 cells, ...)
opt4  follow the squad leader (no branch of the doctrine says to follow the leader)
opt5  retreat to the squad's spawn (the doctrine's first branch: an enemy can see you within 6 cells and you have fewer allies within 4 cells than there are enemies within 6 cells)
```

## The measurement

`crates/lm-host-model/tests/gates_tuning.rs` builds the situations from **real
rounds**: seeds 1..30 are played with the shipped reflex, and one agent's view
is asked about. Two samples are taken from each round, because the doctrine has
three branches and a round has a rhythm:

* **the march** (tick 600) — both squads are still walking to the beacons and
  have never seen each other. Every situation here is the doctrine's *third*
  branch.
* **the fight** (tick 1800) — the squads have met at the beacons, so enemies
  are in sight and the *first* branch (retreat) fires.

Each situation is asked with the pack the harness shipped before the tuning and
with the pack it ships now, over the same facts, and the answer is compared with
a **code oracle** that applies the same rule to the same computed facts.

```
cargo test --release -p lm-host-model --test gates_tuning -- --ignored --nocapture
```

## Before and after

**The march** (30 situations, seeds 1..30, tick 600):

| | before: "What should you do now?" | after: the doctrine, as a rule |
|---|---|---|
| answered | 30 of 30 | 30 of 30 |
| confidence p10 / p50 / p90 | 0.201 / 0.571 / 0.803 | **0.399 / 0.865 / 0.913** |
| above the 0.55 floor | 16 of 30 (53%) | **21 of 30 (70%)** |
| agreement with the oracle | 17 of 30 (57%) | **30 of 30 (100%)** |
| latency p50 / p90 | 255 ms / 375 ms | 355 ms / 372 ms |

**The fight** (60 situations, seeds 1..30, tick 1800, both squads):

| | before: "What should you do now?" | after: the doctrine, as a rule |
|---|---|---|
| answered | 60 of 60 | see the note below |
| confidence p10 / p50 / p90 | 0.193 / 0.316 / 0.454 | — |
| above the 0.55 floor | 4 of 60 (7%) | — |
| agreement with the oracle | 25 of 60 (42%) | — |
| of which the oracle said "retreat" | 0 of 13 right | 4 of 4 right (partial) |
| latency p50 / p90 | 317 ms / 331 ms | 399 ms / 422 ms (partial) |

> **Note on the runs.** The figures come from two runs of the same test over the
> same deterministic situations (the same seeds, the same ticks, the same
> facts): the march row's "before" figures were captured in the first run and
> its "after" figures in the second, so the two packs are compared over
> identical situations but not in a single pass. The only numbers that move
> between runs are the latencies (network jitter); the confidences and the
> agreements are the same.
>
> **Note on the "after" column of the fight sample.** The Thor's model servers
> (`d1-3b.service` and its siblings) stopped listening part way through that
> run: the host still answers a ping, but no port accepts a connection, and the
> remaining calls came back as refusals. The figures above for the new pack are
> therefore only the **first 25 of the 60** situations — a partial sample that
> stops when the endpoint goes away, not a measurement of the pack. The
> "before" column of the fight sample was measured in full, before the endpoint
> went, so it stands.
>
> The re-run is the one open item. The test is `#[ignore]d`, it prints the
> refusal reasons, and it refuses to report a median over a sample that did
> not answer.

## What the numbers say

1. **The rule is what makes the difference.** The old pack asked an open
   question over a long list of facts; the model had to invent a policy and
   then defend it, and its own confidence in that invention was 0.20–0.57. The
   new pack asks a question with one right answer, and the model's confidence
   in it is 0.87 in the median. That is E4c's lesson applied to the shipped
   harness: *which action, given this rule?*
2. **Confidence and agreement move together.** The old pack was not merely
   unsure, it was wrong: 57% agreement on the march, 42% in the fight, and it
   never once picked "retreat" (0 of 13) when the oracle did. A rule the model
   can apply is worth more than a question it can only guess at.
3. **The pack costs more.** The doctrine is a longer prompt: 355 ms against
   255 ms at the median, and the fight sample reached 399 ms. The shipped
   `AgentsConfig::request_timeout_ms` is **400 ms**, so the new pack lands
   inside the deadline most of the time but not always. Two ways out, both
   Casey's to take: raise the deadline (the mind's D56 levers already budget
   for a slow tier), or trim the wording.
4. **The floor is now reachable.** With the shipped floor of 0.55 and the
   shipped 400 ms deadline, 21 of the 30 march thoughts become decisions,
   against 16 of 30 before — and, unlike before, they are the *right*
   decisions.

## Open

* The fight sample needs one more run against a live Thor; the endpoint went
  away during the first attempt (see the note above).
* The doctrine's second branch — "hold" — is rare in a real round: an agent has
  to be standing exactly on a beacon its squad holds, and the reflex is usually
  walking or charging. Over seeds 1..30 at ticks 600, 1200, 1800, 2400 and
  3000 the branch never fired once. The branch is correct but nearly dead
  code in practice; if the game wants a real "hold", the doctrine needs a
  different second branch, not a better wording.
