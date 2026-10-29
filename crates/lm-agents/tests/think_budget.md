# The think budget, measured in our own code

decision-daw x5 measured the Thor: d1-2B serves about nine requests a second,
serialized (median 141 ms alone, about 755 ms with sixteen in flight). The
shipped defaults ask for far more — `think_hz_member = 4` and
`think_hz_leader = 1` — so eight agents ask for about thirty requests a
second. This is what our agents actually do against a server with that
capacity.

## The command

```text
cargo test --release -p lm-agents --test think_budget -- --ignored --nocapture
```

## The table

| think_hz_member | request_timeout_ms | asks/s | decisions/agent/s | on time | late | stale | timeouts | reflex share | mean score |
|---|---|---|---|---|---|---|---|---|---|
| 4 | 400 | 14.90 | 0.009 | 100.0% | 0.0% | 0.0% | 12439 | 62.7% | 1161.3 |
| 4 | 1200 | 8.92 | 1.692 | 96.1% | 0.3% | 3.6% | 0 | 0.8% | 909.0 |
| 2 | 400 | 14.02 | 0.009 | 100.0% | 0.0% | 0.0% | 11474 | 62.7% | 1161.3 |
| 2 | 1200 | 8.79 | 1.683 | 96.0% | 0.4% | 3.6% | 0 | 0.8% | 909.0 |
| 1 | 400 | 8.38 | 0.948 | 95.6% | 4.4% | 0.0% | 4009 | 38.7% | 1157.5 |
| 1 | 1200 | 7.59 | 1.570 | 96.0% | 2.1% | 1.9% | 0 | 0.8% | 909.0 |

## Reading

At the shipped 4 Hz the eight agents put 14.90 requests/s at a server that
answers 9/s, and the server's own queue wait (110 ms a request, so about
880 ms for a full squad) is longer than a 400 ms request timeout: 12,439 of
those asks time out, the model delivers 0.009 decisions per agent per
second, and the reflex drives 62.7% of the agent-ticks against 0.8% at
1200 ms.

The request timeout, not the think rate, is what decides it: at 1200 ms the
same 4 Hz and 2 Hz ask 8.92 and 8.79 requests/s and get 1.692 and 1.683
decisions per agent per second with no timeout at all, while 400 ms loses
12,439 and 11,474 asks at 4 and 2 Hz — and 4,009 even at 1 Hz, where only
0.948 decisions per agent per second survive — so the phase-3 400 ms
deadline sits below the queue wait of a saturated server and must not come
back.

With the timeout fixed at 1200 ms, 4 Hz, 2 Hz and 1 Hz land within 8% of
each other (1.692, 1.683 and 1.570 decisions per agent per second) because
the 9/s server, not the ask rate, is the binding constraint: 4 Hz and 2 Hz
run it at 8.92 and 8.79 requests/s (about 99% of capacity, with 3.6% of the
answers stale) while 1 Hz asks 7.59 requests/s and halves the stale share to
1.9% — ask at 1 Hz and the server keeps 16% of headroom for bursts, for 7%
fewer decisions.

## What was measured

- **The rounds are the host's own path**: the host loop, the agents, the
  least-use-first queue, the sequencer and the recorder
  (`lm_host_model::HostLoop` plus `lm_agents::Agents`). Nothing in the
  cognition loop is re-implemented.
- **The model behind the tiers is a `ThrottledStub`**: a server that serves
  one request at a time, 110 ms each, so it answers about nine requests a
  second. Every tier the policy names — the "fast" tier's `thor-d1-2b` and
  the "deep" tier's `jev` — is served by that one server, so the round's
  whole demand meets one capacity.
- **The clock is the sim's own**: a round of 3,000 ticks is 100 seconds of
  play at 30 Hz, and the model server's time is the sim's time, so an answer
  that waited four slots to be served lands four slots late.
- **The round**: 8 agents (2 squads × 4), the shipped harness, gates, policy
  and reflex, seeds 1..=10 per cell, 3,000 ticks each. The cells run in
  parallel; each round has its own server.
- **The columns**: `asks/s` is the demand the round put at the server;
  `decisions/agent/s` counts the agent decisions that were applied, over the
  agents and over the play; `on time`/`late`/`stale` are the sequencer's own
  counts; `timeouts` is the requests the server refused as too late;
  `reflex share` is the model-vs-code dial (D25/D40) from the learning
  flow's own metric, replayed over the round log; `mean score` is the mean
  final score over the rounds and the squads.

## What the score column does not say

The model here is the menu stub, and the menu stub always favours the first
option of the menu — so a model-driven round herds the squad onto one beacon
and scores lower (909.0) than a reflex-driven one (1161.3). Read the score
as "the reflex beats this stub", never as "the model plays badly": it is a
property of the stub's answer policy, not of the budget, and it must not be
used to choose a default. The budget is read from the timeliness, the
timeouts and the dial.

## What was not changed

No shipped default was touched: `think_hz_member`, `think_hz_leader` and
`request_timeout_ms` are the shipped values, and the sweep only reads them.
The recommendation that follows from this table is in the run's summary, not
in a default.
