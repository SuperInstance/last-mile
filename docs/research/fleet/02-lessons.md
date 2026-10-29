# Lessons from the fleet, as changes to last-mile

Each lesson names its source repo, the change, and its decision number. Every value is a parameter (D19).

## D23: Fast models roll dice instead of writing decisions

*Sources: whole-house-quilt §22 ("decision models are the dice"), erised-sequencer (dice derived from the ledger).*

- **Today:** System-1 emits a structured decision.
- **Change:** in addition, the sim offers each agent a **menu** of legal decisions from its `ScoreView`, such as "go to beacon B", "follow the leader" or "say *contact east*". System-1 returns **a probability for each option**, quantized to u16.
- The **roll happens inside `lm-core` at `apply_tick`**, using the round's seeded RNG stream for that agent.

**Why:**
- The roll is part of the deterministic sim, so every mirror agrees without trusting the Thor.
- The logged `p[]` makes a biased or overconfident model visible from its own history.
- Temperature becomes an honest game knob.
- Replays never need the model.

The free-form path stays for utterance text, which can't be a menu. So does a direct `argmax` mode.

⚙ `agent.s1_output = distribution | argmax | direct` (default `distribution` for actions, `direct` for utterance text). ⚙ `agent.menu_max` = 8.

## D24: What happens when a budget runs out is a parameter

*Source: COLOR SQUAD, where a contender over budget is frozen cold.*

COLOR SQUAD freezes the squad. last-mile's default is softer: reflexes keep playing. Both are legitimate game designs.

⚙ `budget.on_exhaust = reflex | freeze` (default `reflex`).

## D25: The descent: repeated wins become reflexes, and barks are cached

*Sources: voice-games ("3 clean runs → local reflex"), voice-reflex-gate (compile a reflex after 3 similar answers; reflexes fade, are superseded or evicted), pincher, and the quilt's §19 ("a job is finished being designed when what remains of it is watching").*

- Every System-1 decision is logged with a **situation key**: a deterministic hash of the summarized `ScoreView` features that the policy used.
- When the same key leads to the same decision class and a good outcome *N* times (⚙ `reflex.promote_after = 3`), that pair becomes a **reflex candidate**.
- Between rounds, candidates are written into `reflexes/*.rhai`:
  - by template, or
  - by the Thor's resident 30B coder.
- Promoted reflexes carry a confidence that decays when they aren't used, and they're **evicted** when contradicted (⚙ `reflex.decay`, ⚙ `reflex.evict_on_contradiction`).
- **Casey's rubric made measurable:** the **reflex share** is the fraction of an agent's decisions served by reflexes. It's tracked per round and should rise while win rate holds. "Better excavators that smaller agents can operate."
- TTS barks use the same mechanism. The bark cache is keyed by normalized text, voice and affect bucket.
- **M1 scope:** log situation keys and outcomes, and show reflex share. Automatic promotion is M2.

## D26: A refusal is an outcome, and intents expire

*Source: thor-firstline, which once flew the boat to (0,0,0) because a misheard number was replaced with a default.*

- A model output that fails the schema is **not a decision**. It's logged as a refusal, and the reflex layer keeps playing. **Nothing ever falls back to a guessed default value.**
- `Intent` decisions (a plan or an order, as opposed to a single action) carry a TTL (⚙ `intent.default_ttl_ticks` = 300, or 10 s). When an intent expires, the agent's reflex defaults take over, the equivalent of HOLD.
- **Parity vectors:** thor-firstline checks its Lua and Python control laws against 43 shared vectors in CI. Our equivalent is golden vectors for fixed-point math and Rhai reflexes, run on x86 (ProArt) and ARM (Thor/Orin). This is already in spec §9 and is now explicitly a vector suite, not just replays.

## D27: Harness commits are frozen states, and round headers pin them

*Sources: saddle (frozen alignment states), color-squad (strategies versioned in git).*

- At round start, the **round header records each agent's harness commit hash**.
- After the round, the harness is tagged `round-<id>`. That tag is the frozen state to roll back to.
- Replaying a round means checking out those hashes, so learned behavior reproduces exactly along with the sim.

## D28: Agent voices are half-duplex around the human

*Source: thor-firstline `voice/gate.py`.*

- On the ProArt's speakers, agent TTS **yields to the human's push-to-talk**.
- Audio frames held back by the gate are **dropped, not queued**, so an agent never talks over the moment the human needs quiet.
- There's a hang window so a pause in the middle of a sentence isn't taken as a cue, and a max-talk limit after which the agent must yield.
- This affects playback only: the score still records every utterance.

⚙ `audio.duplex = half | full`, ⚙ `audio.hang_ms` = 400, ⚙ `audio.max_talk_ms` = 6000.

## D29: A tutor ladder for the human in a support role

*Source: voice-games COLOR SQUAD (verbs unlock as the player uses earlier ones; negation like "don't stop" is refused).*

- When the human possesses a squad member, command verbs can unlock in a ladder. The human learns the game in a support role (D5).
- **Negations are refused** at the parser level, not "interpreted".

⚙ `human.tutor = off | ladder` (default `ladder` for new players, `off` in tournaments). Built in M1 phase 4, as an option.

## D30: Downmixing: old history is summarized, but notable events never are

*Source: chrono-tensor §4.*

This applies to agent memory (the context fed to models) and to archive retention.

- A segment older than its full-fidelity window becomes a **stem**: counts, ranges and means.
- **Notable events are kept verbatim:** flagged, referenced by a later decision's provenance, budget-breaching, or errors.
- Each stem records its source tick range, event count and summarizer version.
- Losing detail is a recorded act, never a side effect of a full buffer.

⚙ `memory.full_window_ticks`, ⚙ `archive.downmix_after_rounds`.

## D31: Squad talk has timing: pre-drafting, airtime and silence

*Source: quilt-conversation (T-minus pre-drafting, per-agent cadence, energy budget, silence as signal).*

- **Pre-drafting:** when a teammate starts an utterance addressed to an agent, that agent may enqueue its reply's think request right away. The reply's `apply_tick` is set after the utterance's last word, so answers arrive "already on the runway" instead of after a full think latency.
- **Airtime is a budget**, not just a metric: ⚙ `budget.airtime_s` per agent per round.
- **Silence is a valid decision** on the menu (D23).
- **Cadence** (talkativeness and response latency) is a per-agent personality parameter in `policy.toml`.

## D32: Learning is measured honestly

*Sources: dungeon-ml-zoo (sealed pre-registered predictions; training seeds and eval seeds never mixed), color-squad (round-robin matrix, decision graphs), erised (who-cites-whom).*

- A between-round lexicon or harness change is judged on **held-out evaluation seeds** that played no part in producing the lesson. ⚙ `learning.eval_seeds`.
- Before an evaluation runs, the System-2 reviewer writes down what it expects the change to do: a sealed prediction, stored in the round record.
- Squad metrics gain **uptake by speaker**: whose words were followed by actions. This is the erised "citations" graph for squad talk.
- In M2, COLOR SQUAD's **season with innovation breaks** (breed, fleet-authored counter, hand-authored) becomes the template for between-round re-harness.

## D33: Spectator change requests become deposits

*Source: moldmaker (voice → translation file with what was said vs what was inferred, an agent-to-agent twin, and open questions).*

Between rounds, each human change request is recorded as a markdown deposit containing:

- the human's words;
- what the agent inferred;
- **the machine twin**: the exact Rhai/parameter diff;
- any open questions.

It's committed with the round. This keeps "why did the rules change" traceable, and it's the seed of Studio mode's A/B history.

## D34: Networking: adaptive lead, the ProArt dials out, wired Thor recommended

*Source: [01-measured-facts](01-measured-facts.md).*

- ⚙ `sched.lead_mode = fixed | adaptive` (default `adaptive`). The Thor measures one-way p99 to the sequencer from QUIC RTT samples and requests `apply_tick ≥ now + clamp(p99_one_way + margin, min_lead, max_lead)`. ⚙ `sched.max_lead_ticks` = 9.
- The **ProArt dials out** to the Thor and the Orin (QUIC client). Authority and connection direction are independent. ⚙ `net.dial = proart_dials | listen`. ⚙ `net.*_port`: avoid 8788 and the Thor's resident ports.
- **Sequencer placement is a parameter** (⚙ `net.sequencer_node`, default `proart`) because lockstep allows any node to sequence. See [03](03-whole-house-quilt-fit.md) for why the ProArt stays the default.
- Recommend wired Ethernet for the Thor.

## D35: Default model assignments on the Thor (provisional until the spec sheet)

*Source: [01-measured-facts](01-measured-facts.md).*

| Role | Default | Why |
|---|---|---|
| Member System-1 (4 Hz think) | LFM2-1.2B (:8092) | Already resident, fast, small |
| Leader System-1 (1 Hz) and squad summaries | `qwen3.5:9b` via Ollama (or `nemotron-3.5-lightning`) | More reasoning, still local |
| Between-round operator (Rhai edits, reflex promotion, harness diffs) | Qwen3-Coder-30B-A3B (:8001) | "Generators up, instances down": cloud System-2 writes the lesson, the local coder applies it |
| ProArt fallback System-1 | the 0.6B local tier (opt-in) | ⚙ `agent.s1_fallback = reflex | proart_local` |

All of these are parameters in `lm-agents` config, and Casey's spec sheet may replace any of them. Riva and the vector DB stay open until the spec sheet arrives.
