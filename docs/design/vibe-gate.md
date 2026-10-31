# The vibe gate: a kid's words become a proposal

**Status:** built and measured (Vibe Dictionary part 3, M2b). Part 1 is the
dictionary (`assets/harness/default/vibe.toml`), part 2 the landing
(`lm vibe propose/state/undo`, protocol v5).
**Date:** 2026-10-30.

## What this is

Today a Vibe proposal is typed at the CLI (`lm vibe propose unit.vision 32`).
In the sandbox a kid *says* what they want — "let them see farther" — and
**Bench**, the AI co-developer, turns it into that typed proposal. The vibe
gate is how: **two closed questions the decision server answers**, in the
same shape every gate in this game is asked (D37, D70). The decision models
answer closed questions; so does this one. The model's output is an index
into the Vibe Dictionary — never free text, never a program (sandbox §3).

```text
the kid speaks
  -> question 1  vibe:        which change? one option per dictionary entry,
                              plus none — the options are built from the
                              entries' own `says` vocabulary and `explain`
  -> question 2  vibe_value:  how much? the chosen entry's own bounded steps
                              (a little / a lot / back to normal), keyed by
                              the step's value
  -> a typed proposal: entry + value, each answer with its confidence
  -> --propose hands it to `lm vibe propose`'s pipeline, unchanged:
     bounds check, prediction sealed BEFORE the test, the held-out sweep,
     the verdict (D78), the landing
```

## Where each piece lives

| Piece | Where |
|---|---|
| The two questions and the policy (model, confidence floor, deadline) | `assets/harness/default/vibe-gate.toml` — the gate's own small policy pack, data in the harness |
| The entries the first question asks over, and their bounded steps | `assets/harness/default/vibe.toml` (`[[block]]` … `steps`) |
| The gate itself: question building, answering, refusals | `crates/lm-learn/src/hear.rs` (`lm_learn::hear`) |
| The command | `lm vibe hear "<words>" [--propose]` (`crates/lm-tools/src/vibe.rs`) |
| The measurement fixture and the live table | `crates/lm-learn/tests/vibe-gate-phrasings.toml`, `crates/lm-learn/tests/vibe_gate.rs` |

The gate is asked **only when a kid speaks**, so it is deliberately not part
of `gates.toml` or the agents' move/talk packs (D70): those are in-round
questions with the round's clock; this is the sandbox's interpret step, with
its own budget (the design's ≤ 120 ms against the radio's 1–3 s of cover,
sandbox §2). The pack names the model (`d1-2b`); the agents' config — the
one place that says where the Thor is — supplies the base and the port
(:8094), so no address is duplicated here.

### The steps are data

Each dictionary entry now carries its own bounded `steps`, and load refuses
a dictionary whose steps fall outside the bounds or that never carry the
default as a step (so "back to normal" is always sayable). The shipped
steps follow one rule, stated in the file: **a little** = 1.5× the shipped
default in the direction the entry's `says` names (half the stun, for the
one entry that says "shorter"), **a lot** = the bound that direction points
at, **back to normal** = the default — the same value `lm vibe undo`
restores.

### The refusals are named (sandbox §3, D72, D26)

| Refusal | What the kid hears |
|---|---|
| `EntryUnsure` — the first answer is under the gate's floor (0.30, the D72 mechanism, per question) | "ask the kid to say it another way" — never a guess |
| `ValueUnsure` — the amount is under the floor, the change named | the same ask, with the entry it heard |
| `NoMatch` — the model picked `none` | "Bench has no word for that yet; the ask goes to the request pile" |
| `OffMenu` — the answer named an option the question never offered | a refusal, never a neighbouring option (D26) |
| `PutItBack` — the words say "put it back", or the model picked the "back to normal" step | that is the undo, not a proposal (`lm vibe undo`) |
| `TooLong` / `SaidNothing` | "say it shorter" / nothing to hear |

"Back to normal" as a *step* is refused rather than proposed because the
pipeline measures every change against the shipped default: a proposal of
the default is a test of nothing.

### Privacy (sandbox §7)

The words travel to the decision server and stop there. A hearing writes no
file at all; a `--propose` record carries the words only as the bounded
48-character label the ledger already carries — never a transcript, never
audio. A hearing over 160 characters is refused ("say it shorter") rather
than silently cut.

## An example run

On the Thor, 2026-10-30 (the box was loaded; see the latency note below):

```console
$ lm vibe hear "let them see farther"
heard:     "let them see farther"
  entry:    unit.vision (confidence 0.99, 273 ms)
  value:    30  "a little farther" (confidence 0.43, 299 ms)
  would propose: unit.vision 20 -> 30  "let them see farther"
  (a hearing writes nothing; say --propose to seal and test it)

$ lm vibe hear "let them see WAY farther, like the whole map" --propose --seeds 2 --ticks 400
heard:     "let them see WAY farther, like the whole map"
  entry:    unit.vision (confidence 0.99, 248 ms)
  value:    64  "a lot farther" (confidence 0.96, 290 ms)
  would propose: unit.vision 20 -> 64  "let them see WAY farther, like the whole map"
unit.vision 20 -> 64  "let them see WAY farther, like the whole map"
  sealed:   seal 0 in …/vibe.jsonl (before the test)
  predict:  sight_ticks up by at least 1
  test:     2 seeds x 400 ticks, 20 vs 64
  sight_ticks  up   0/2 seeds qualified … — NOT MET
  verdict:  NOT ADOPTED (0 of 1 predictions held)

$ lm vibe hear "make the paint bouncy"
lm: refused: the words match no change in the dictionary — Bench has no word
   for that yet; the ask goes to the request pile
```

The two questions read the *amount* out of the words: "let them see
farther" hears `30` (a little), "WAY farther, like the whole map" hears
`64` (a lot).

## The measurement

55 kid-style phrasings — 10 per dictionary entry, 10 that match nothing the
dictionary holds, and 5 honestly ambiguous ones — in
`crates/lm-learn/tests/vibe-gate-phrasings.toml`, run through the whole gate
(both questions, the real floor, the real deadline) against the Thor's
d1-2B. Reproduce with:

```console
cargo test --release -p lm-learn --test vibe_gate \
  the_vibe_gate_table_on_the_thor -- --ignored --nocapture
```

The same test is a lane in `bash scripts/live.sh` (d1-2b), with floors: a
pack that hears fewer than three quarters of the labelled phrasings, or
that matches six of the ten no-match asks to changes that do not exist,
fails the lane.

**Run 1** — load average 24 when it ran (the box was busy all evening;
these latencies are load-inflated, not the quiet-Thor numbers):

| set | right | of | unsure | wrong (heard instead) |
|---|---|---|---|---|
| unit.vision | 9 | 10 | 0 | `their eyes should be better` -> the "back to normal" step (the amount, not the entry, was misheard) |
| unit.speed | 10 | 10 | 0 | — |
| unit.tag_range | 9 | 10 | 0 | one deadline refusal (loaded box) |
| unit.stun_time | 9 | 10 | 0 | one deadline refusal (loaded box) |
| none | 10 | 10 | 0 | — |
| ambiguous | 3 | 5 | 0 | `make it easier to catch people`, `they should be better at tagging` -> none |

**37/40 labelled right (93%), 10/10 none right, 3/5 ambiguous accepted, 3
refusals of 55; mean latency 412 ms (entry) / 434 ms (value)** over the
answered questions. The confidence of a right entry answer ran 0.51–1.00;
the amount answers ran lower (0.40–0.78), all above the 0.30 floor — the
floor refused nothing in either run; every refusal was a deadline or the
"back to normal" step.

**Run 2**, minutes later at load 36–69, shows what a busy box does — and
that the deadline rule behaves as designed (the hearing refuses, the round
would go on): 34/40 labelled right, 4 deadline refusals, and the one
**genuine confusion** both runs agree on the shape of:

- **unit.stun_time ↔ unit.speed** on "they should get up faster": the word
  "faster" pulls the speed entry (heard at 0.68 confidence, under-load);
  the entry's own `says` ("make the stun shorter") does not contain
  "faster". The phrasing sits in the fixture because kids say it.
- "their eyes should be better" hears the entry right and the amount wrong
  ("back to normal"): an *amount* confusion, refused as `PutItBack`.
- The two ambiguous "catching/tagging" phrasings hear `none` rather than
  guess — the honest edge, not an error, but it means a kid asking
  obliquely gets the request-pile answer. If that shows up in play, the
  fix is a `says` phrase on the entries ("better at catching people"), not
  a softer floor.

No confusion ever crossed into `unit.vision` ↔ `unit.tag_range` ("see from
farther away" was heard as vision, which the fixture accepts as right), and
none of the ten no-match asks was ever matched to a change that does not
exist.

## What is deliberately not here

- **No transcript, anywhere.** The words go to the decision server and into
  the 48-char label; the measurement fixture is the one place phrasings are
  written down, and they are the bench's, not a kid's.
- **No escalation** (D71) on the vibe gate: an unsure answer asks the kid
  to say it another way — decompose before escalate, with the kid as the
  second model. Whether a clef-flash second opinion beats that is Q4's
  experiment, not this slice's guess.
- **No ears.** This slice hears typed words; the voice path (D67, the
  Thor's ears at :8102) feeds the same `hear` later.
- **No step accuracy grade.** The amount a phrasing means is mushy ("see
  farther" is honestly either step); the table records what was picked per
  case instead of inventing a right answer for it.

## What is next

1. **Wire `hear` into the between-round path** (Wave 3, Bench on the
   radio): the ears' transcript -> `hear` -> the proposal screen, inside
   the 1–3 s radio budget; a hearing that misses its deadline is cover
   static, not a wait.
2. **The request pile**: `NoMatch` asks should land in `Deposit.questions`
   (`crates/lm-learn/src/deposits.rs`), which exists for exactly this.
3. **More entries**, more `says`: the dictionary grows toward the sandbox's
   ten (paint blocks especially), and each entry arrives with its 10
   phrasings so the table grows with the vocabulary — the gate's accuracy
   is a property of the *pair*, and the pair is measured, not assumed
   (sandbox §3: any block the model cannot reach reliably stays a button).
4. **Value-floor calibration**: the amount answers run 0.40–0.78 on the
   d1-2B; if that tightens the gate too much once kids play, the floor for
   the value question alone is the knob (it is per question in the pack).
