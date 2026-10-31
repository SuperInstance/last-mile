# Question identity, the answer cache, and the question ledger

*Note, 2026-10-30. A seat's work on wave 2 of the questions-as-experience
track. What was built, what it costs, and what it would take to decide it.*

This note is the companion to
[docs/design/questions-as-experience.md](../design/questions-as-experience.md)
§3 and §5.4: the design note says that an agent's experience is its
question library, and that the library has to be readable, versioned and
measured. This is the machinery for that: **every gate now has an id and a
version**, the answers carry them, **a cache can serve a repeated
question**, and **a ledger records what each question cost**.

Nothing here changes what the game does. The cache is **off by default**,
the ledger is a file beside the round's other files and is **never
hashed**, and the golden is unchanged (OK 3/3): only decisions travel.

---

## 1. The question's identity

A gate's wording is behaviour, so the identity of a question has to say
which wording it is. Three names, from `crates/lm-agents/src/question.rs`:

| name | what it is | derived from |
|---|---|---|
| **id** `q_<16 hex>` | *which* question this is | the gate's **slot** and its **kind** |
| **version** `v_<16 hex>` | *which revision* of that question | the digest of the **normalized form** |
| **normalized form** | the canonical text: `kind \| instructions \| key=text;…` | the gate's own wording, normalized |

- The **id survives a rewording**: the `move` gate is the `move` gate
  however it is worded, so the ledger holds one row per question and the
  version says which revision was measured.
- The **version does not**: a reworded gate is a new version, which is
  exactly what the brief asks for, and it is what keeps an old gate's
  cached answer from being served to a new one.
- The **normalized form** collapses whitespace and case, so "Hold
  position" and "hold   position" are one question.

Everything is derived from **harness bytes** (the gate's own definition)
or from the deterministic sim. No clock, no randomness, no map order: two
machines that play the same round compute the same ids (D69).

The digest is worked out **once**, when the gate set is read: the pack is
built on every think, and a digest per gate per think is work the round
does not need to do again. (It was measured: digesting per ask made the
debug test suite nearly twice as slow, 5.9 s to 11 s for the escalation
fence.)

### The pack format

Every question in a System One pack now names itself:

```json
"move": {
  "type": "choice",
  "instructions": "Your squad's doctrine, in order: …",
  "criteria": {"opt0": "go to beacon 0 (…)", …},
  "id": "q_9f2c…",
  "version": "v_41ab…",
  "normalized": "choice | your squad's doctrine, in order: … | beacon=go to beacon {beacon} (…); …"
}
```

The model server was measured with the new fields and **answers the same
pack the same way** (§4): the three System One endpoints on the Thor
(d1-2B :8094, d1-3B :8093, clef-flash :8096) and the batched one (:8104)
all answer a named pack with the **same choice and the same
probabilities** as the unnamed pack, to the last digit. The answer carries
the names on (`Answers.questions`), so **every logged answer says which
question it answers** and which revision of it.

---

## 2. The context digest and the answer cache

The cache is the doctrine scrap-spark already runs in production
(docs/research/lucineer-ecosystem.md §13): *model call heavy at first,
then more and more canned responses*. The key is the same shape:

```
SHA-256(question id | version | context digest)
```

The **context digest** is the facts the gate saw, normalized, together with
the options it was really asked about. The same question over a different
situation is a different key, so it is asked again (D26: no answer is
ever invented).

A hit is served only when the **backend** and its **confidence floor**
match: an answer one model gave under one floor is not an answer another
model would give, and a stricter floor would have refused it. The cache
is in memory, shared by the round (an answer one agent paid for is an
answer every agent may have), and **off by default** — the
`answer_cache` lever in `policy.toml`.

The cache works **per question**, not per pack: a pack whose questions are
half held is asked about the half it does not hold, and the two answers
are merged. The count of hits, misses and the age of every answer (in
asks) is kept, and the age is counted in asks rather than in milliseconds
so that nothing in the cache reads a clock.

---

## 3. The question ledger

One file per round, written beside the round's other records and **never
hashed**: it describes the questions, not the world.

```toml
[round]
questions = 11
asks = 302
cache_hits = 0

[[question]]
id = "q_9f2c…"
version = "v_41ab…"
slot = "move"
kind = "choice"
normalized = "choice | your squad's doctrine, in order: … | …"
asks = 276
accepted = 271
unsure = 5
refused = 0
escalated = 3
cache_hits = 0
ms_cost = 44160

[[question.confidence]]
model = "thor-d1-2b"
mean_permille = 620
n = 271
```

Per question: **asks**, **unsure**, **refused**, **escalated**, the **mean
confidence per model** (in permille, so the file is the same on every
machine), the **ms cost** (the model's own time on the packs that carried
the question) and the **cache hits**. This is the seed of ROADMAP-AHEAD
§2.5.

The ledger is written by `Agents::write_ledger`, and the learning flow
writes it with the round's other files (`runs/questions/<round>.toml`).

---

## 4. The measurement: the cache's ceiling

**How it was measured.** Offline, on the pack corpus
(`~/lm-batch/packs`): 147 seed files, 32 167 real packs, 72 442 question
asks, captured from real rounds (seeds 1–150, three missing). The tool is
`lm qcache --packs ~/lm-batch/packs`; it computes the **same key the live
cache uses**, so the hit rate it reports is the rate the cache would get.

The corpus carries no harness, so the version term is the question's own
content digest rather than the harness's. The hit/miss **partition is
identical either way**, because the context digest already covers the
question as asked.

### The result

```
the pack corpus
  files:          147
  packs:          32167
  questions:      72442
  distinct pairs: 48991
  recurring asks: 23451 (32.4%)
  packs all hit:  4056 (12.6%)

per gate: the cache's ceiling for each question
  gate                             asks   distinct     hits     rate
  attention                        4475       2320     2155    48.2%
  attention_present                4475       2320     2155    48.2%
  gesture                          4475       2320     2155    48.2%
  gesture_present                  4475       2320     2155    48.2%
  intent                           4475       2320     2155    48.2%
  intent_present                   4475       2320     2155    48.2%
  intention                        4475       2320     2155    48.2%
  intention_present                4475       2320     2155    48.2%
  move                            27692      25791     1901     6.9%
  subject                          4475       2320     2155    48.2%
  subject_present                  4475       2320     2155    48.2%
```

**A third of all the questions the game asks are repeats.** The talk pack
recurs **48.2%** of the time; the move gate, whose context is the whole
world state, recurs **6.9%** of the time.

The gates of a pack recur **together**: the ten talk gates all saw the
same facts, so their hit rate is the same to the digit, and a talk pack is
fully held 48.2% of the time. The per-question cache therefore buys
nothing over a per-pack cache *in this corpus* — the design is kept
because the corpus is one harness with one shape, and a pack whose gates
disagree (one gate's facts moved, another's did not) is exactly where a
per-question cache still helps.

**What a hit is worth.** The talk pack costs 473–1600 ms on the Thor
(D70), and the cache's hit rate for it is 48.2%: a quarter of the game's
thinking would be free. The move pack is one question and 162 ms, and its
6.9% is worth about 11 ms a round.

**What a hit costs.** A hit is an answer the model did not give this time.
The move gate's answer moves the game, so a stale answer there would be
the reflex's job anyway (D26 refuses, the reflex plays). The talk gates
choose a line, and a repeated situation is a repeated situation — but the
*same question over the same facts* can still deserve a different answer
if the model would have given one. That is the experiment this
instrumentation makes possible, and it is not run yet.

### The pack format's cost on the Thor

Measured on the Thor (d1-2B, :8094) with a real ten-question talk pack,
15 interleaved calls per variant, so the load drift hits all three alike:

| variant | body | median | vs. base |
|---|---|---|---|
| base (today's pack) | 5 489 B | 876 ms | — |
| + id + version | 6 099 B | 880 ms | +0.4% |
| + id + version + normalized | 10 114 B | 880 ms | +0.5% |

**The load was 101 (14 cores) during the measurement** — other seats were
running the Thor — so the absolute latencies are meaningless and only the
*relative* comparison is quoted. The relative cost is inside the noise: the
named questions cost nothing measurable, even though the normalized form
nearly doubles the body.

---

## 5. The live lane

The pack format changed, so the lane that plays against the real models was
run (`bash scripts/live.sh`, the Thor at 192.168.0.78, this box):

| lane | result |
|---|---|
| `lm-agents live_thor` (d1-3B) | **pass** |
| `lm-host-model live_round_on_thor` (d1-3B) | **pass** — a whole round with live minds, the Thor's decisions and all |
| `the_doctrine_pack_is_confident_enough_to_drive_play` (d1-3B) | **pass** — the pack the model reads is the named one, and the answer still drives play |
| `lm-host-model talk_soak` (d1-2B) | **pass** — three 3000-tick rounds against the Thor: 125, 120 and 170 decisions |
| `lm-agents realizer_on_thor` (d1-2B) | **pass** (it failed on the first run, under a load of 101: every line fell back to the phrasebook because the wording model was too slow) |
| `lm-voice live_skald_speaks` | **fail** — the voice lane |
| `lm-voice live_skald_round_trip_is_intelligible` | **fail** — the voice lane |
| the ears lane | **skipped** |

**The two failures are the voice lanes, and they are not this change.** The
`lm-ears` service on this box is `inactive` (`systemctl is-active
lm-ears`), so the skald has nothing to hear with, and both skald tests
time out. Nothing in `lm-voice` was touched.

**The refusal counts are unchanged.** A live round's stats were compared
with the base code (the work stashed, the same test re-run): both rounds
report the same **16 malformed** refusals — the malformed count is the
model's own, and it is the same with named packs as without.

---

## 6. Where the ledger is written, and the one wire left

The ledger is written per round by `Agents::write_ledger(path)`, and the
learning flow writes it with the round's other files
(`runs/questions/<round>.toml`) when it is handed one
(`FlowConfig::with_question_ledger`). The seam is the same one the
surprise table uses: the host that played the round with live minds hands
the ledger to the flow.

**That hand-over is the one wire left**, and it lives in `lm-host`, which
is Windows-only and cannot be built here (`scripts/win-build.sh` needs
the ProArt's WSL). It is one line in
`crates/lm-host/src/between_plugin.rs`, where `run_the_flow` builds the
`FlowConfig`:

```rust
let cfg = FlowConfig::new(seed, settings.harness_root.clone())
    .with_runs_root(settings.runs_root.clone())
    .with_ticks(settings.ticks)
    .with_round_id(format!("r{seed}-{}", settings.stamp))
    .with_question_ledger(ledger);   // <- the round's ledger, from the agents
```

Until that is wired, the ledger is written by whoever plays the round —
the test does it, and the mechanism is proven end to end.

---

## 7. What this proposes, and what it does not decide

**Proposed decisions** (for the director; `docs/design/decisions.md` is
not touched):

1. **A question's identity is its slot and its kind; its version is the
   digest of its normalized wording.** A rewording is a new version and
   not a new question.
2. **The answer cache is off by default** and its key is
   `SHA-256(id | version | context digest)`, with a hit served only to the
   same backend under the same floor. Its ceiling is **48.2% on the talk
   pack and 6.9% on the move gate**, measured on 32 167 real packs.
3. **The question ledger is written per round, outside the hashed log**,
   with the confidence in permille.

**Not decided, and not done:**

- The cache is **not** enabled in the shipped harness, and no round has
  been played with it on.
- The **experiment** the cache exists for — does a cached talk answer
  change what the agents say? — is not run. The instrumentation for it is.
- The ledger is a **row per question**, not the full §2.5 record: it has
  no provenance, no authority, no evidence DAG and no adopt/retire dates.
  It is the seed of that record, not the record.
- The corpus was captured **before** the pack carried names, so the
  measurement's version term is the question's content digest. The hit
  partition is the same, but a fresh capture would remove the caveat.

---

## 8. The code

| where | what |
|---|---|
| `crates/lm-agents/src/question.rs` | the id, the version, the normalized form, the context digest, the cache key |
| `crates/lm-agents/src/gates.rs` | the gate's identity; the pack names every question |
| `crates/lm-agents/src/sysone.rs` | `Answers.questions`: every logged answer names its question; `SysOne::confidence_floor` |
| `crates/lm-agents/src/cache.rs` | the answer cache and the backend that fronts it |
| `crates/lm-agents/src/ledger.rs` | the question ledger and its file |
| `crates/lm-agents/src/agents.rs` | the round's cache and ledger |
| `crates/lm-learn/src/flow.rs` | the ledger is written with the round's files |
| `crates/lm-tools/src/qcache.rs` | `lm qcache`: the measurement |
