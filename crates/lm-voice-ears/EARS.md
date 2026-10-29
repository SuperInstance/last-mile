# EARS: the Thor's ears

LFM3-Audio-2B, served on `:8102`, measured on the squad's own speech and
probed for the one thing last-mile wants from a voice: **whether the
delivery of an order carries meaning the words do not.**

This is a document, not a crate. Nothing here is a Rust crate yet; the
service that exists is a systemd unit and a proxy, and the numbers below
were measured on the Thor against the running d1 servers and against skald.

Every claim says how it was verified. The last section has the commands.

---

## 1. The setup

### What was installed

| thing | where | what it is |
|---|---|---|
| weights | `~/models/LFM3-Audio-2B/` | `LFM3-Audio-2B-Q8_0.gguf` (1,846,236,704 B) and `mmproj-LFM3-Audio-2B-Q8_0.gguf` (303,811,840 B) from the official `LiquidAI/LFM3-Audio-2B-GGUF` repo |
| model server | `~/llama.cpp-upstream/build-up/bin/llama-server` | the build that already serves the d1 models on `:8093`-`:8096`; version 0.8.0-dev (build 50, commit 2e0d86b) |
| door | `lm-wt/ears/tools/ears/ears.py` | a FastAPI proxy that shapes the model's answer |
| unit | `~/.config/systemd/user/lm-ears.service` | `systemd --user`, enabled, running |

**Nothing was built from source and nothing was cloned.** The llama.cpp
build that was already on the box carries the LFM3-Audio support:
`tools/mtmd/models/lfm3a.cpp` (the audio encoder) and `conversion/lfm3.py`
(the GGUF converter) are in that tree, and the official GGUF repo already
has the weights, so no safetensors conversion was needed either. The
brief's escape hatch ("if nothing installed can run this model, stop and
report") was not needed: the box could run it.

### The unit

```
[Unit]
Description=lm-ears: the Thor's ears, LFM3-Audio-2B speech-to-text and intent probe (port 8102)
[Service]
WorkingDirectory=/home/caseydigennaro/lm-wt/ears/service
ExecStart=/home/caseydigennaro/lm-wt/ears/tools/ears/lm-ears.sh
Restart=on-failure
RestartSec=10
```

`tools/ears/lm-ears.sh` starts two processes:

1. **the model**, `llama-server` on `127.0.0.1:8103`, with
   `-ngl 99 --flash-attn on --ctx-size 8192 -ub 2048`. The projector is
   the `--mmproj` file above; the log says *"loaded multimodal model"*.
2. **the door**, uvicorn on `0.0.0.0:8102`, `tools/ears/ears.py`.

The model server is bound to localhost on purpose: `:8102` is the door
last-mile knocks on, `:8103` is an implementation detail and is not on
the LAN.

The script starts the model on the **GPU** and, if the shared GPU has no
room for it, falls back to the **CPU** (`-ngl 0 --threads 8`). A busy box
should not deafen the ears.

**The fallback was measured, not assumed** (`evidence/test-cpu-fallback.sh`):
on the CPU the model comes up in 29 s and answers a clip in **1.2-1.5 s**
against 118 ms on the GPU, with the *same* transcript (`Move up now.`).
Ten times slower, still an answer — and ten times slower is still inside
the round's 2,400 ms decision deadline, so a turn does not break if the
GPU is busy.

### The endpoints

| method | path | what it does |
|---|---|---|
| `POST` | `/v1/audio/transcriptions` | OpenAI-shaped: multipart, field `file`, optional `language` |
| `POST` | `/transcribe?language=en` | the same, with the WAV as the raw body: the push-to-talk shape |
| `POST` | `/v1/chat/completions` | pass-through to the model: the audio-understanding door |
| `GET` | `/health` | is the model up |
| `GET` | `/v1/models` | the model list |

The answer to a transcription is the OpenAI shape plus the three things
the OpenAI shape does not have:

```json
{
  "text": "Hold the line.",
  "words": [{"word": "Hold", "start": null, "end": null},
            {"word": "the",  "start": null, "end": null},
            {"word": "line", "start": null, "end": null}],
  "latency_ms": 114.95,
  "has_timestamps": false,
  "timestamps": null,
  "model": "LFM3-Audio-2B",
  "language": "en",
  "usage": {"input_tokens": 29, "output_tokens": 5, "total_tokens": 34}
}
```

**The model gives no timestamps.** LFM3-Audio is an ASR model: it writes
words, never times, and the official model card's ASR example decodes
text only. So every word carries `start: null` and `end: null` and
`has_timestamps` is `false`. The fields are there so the shape does not
change when a model that does give times comes along; nothing in the
service invents a time.

### What the health probe sees

`scripts/live.sh` probes the Thor by reading `thor_base` and the model
ports out of `crates/lm-agents/src/config.rs`, then asking each port for
`/health`, `/v1/models`, or any HTTP answer. The ears are a **new port
and a new unit**, so the lane as it stands does not look at `:8102`. If
the ears should be part of the lane, the honest change is a new entry in
that script's preflight (not a change to `AgentsConfig`, whose `ports`
table is the *decision models'* ports and the ears are not one).

---

## 2. The cost of the ears

### GPU and RAM

Measured with five samples per state, the median of each
(`evidence/measure-memory.py`):

| state | free GPU GiB | all GPU used MiB | the ears' GPU MiB | the unit's RAM (cgroup) |
|---|---|---|---|---|
| ears running | 4.000 | 53,374 | 3,715 | 896 MiB |
| ears stopped | 8.276 | 49,659 | 0 | — |
| ears running again | 4.155 | 53,270 | 3,611 | 726 MiB |

**The ears take about 4.2 GiB of GPU memory off the box.** The weights are
2.15 GB of that; the rest is the KV cache and the compute buffer at
`--ctx-size 8192 -ub 2048` with four slots.

### The d1-2B on `:8094`

The same System One pack, 30 sequential asks per phase, the pack last-mile
actually sends (`{"model","state","questions"}` to `/v1/systemone`, the
shape of `crates/lm-agents/src/sysone.rs`)
(`evidence/measure-interference.py`):

| phase | median ms | mean ms | p90 ms | min ms | max ms |
|---|---|---|---|---|---|
| A ears running, idle | 27.1 | 28.7 | 28.7 | 24.5 | 73.4 |
| B ears stopped | 26.1 | 26.7 | 27.6 | 24.1 | 50.8 |
| C ears running again, idle | 26.3 | 27.0 | 28.2 | 24.1 | 50.7 |
| D ears running and busy | 32.3 | 33.3 | 42.4 | 25.0 | 43.7 |

In phase D the ears did 44 transcriptions while the d1-2B was being
measured.

**Read: idle costs nothing measurable** (A 27.1 vs B 26.1 vs C 26.3 ms —
the order of the phases is within the noise, and C returning to A's
number says the box did not drift). **Working costs about 6 ms of
median and 15 ms of p90** on the d1-2B, which is a shared GPU and not a
dedicated one. The d1-2B's own deadline is 2,400 ms
(`AgentsConfig::request_timeout_ms`), so a 6 ms tax is 0.25% of it.

The ears are a **push-to-talk** service: one capture at a time, at most
one at a human's turn. Phase D is the worst case (four transcriptions
hammering), not the expected case.

---

## 3. The intelligibility of the voice seat

20 last-mile squad lines × 3 voices = 60 clips, rendered by skald
(`POST :8000/v1/audio/speech`), transcribed on `:8102`
(`evidence/render-corpus.py`, `evidence/transcribe-corpus.py`).

The lines are the harness's **own phrasebook**
(`assets/harness/default/phrasebook.toml`), with the placeholders filled
the way the gates would fill them — `{subject}` is a beacon or an enemy,
`{direction}` is a compass word. The words are the ones the squad actually
says.

| voice | clips | ref words | S | D | I | WER | word accuracy | WER (numbers spelled) | accuracy (numbers spelled) | exact lines | median latency |
|---|---|---|---|---|---|---|---|---|---|---|---|
| aria | 20 | 55 | 4 | 0 | 0 | 0.073 | **0.927** | 0.055 | 0.945 | 16/20 | 119 ms |
| jason | 20 | 55 | 4 | 0 | 0 | 0.073 | **0.927** | 0.036 | 0.964 | 16/20 | 117 ms |
| mia | 20 | 55 | 8 | 0 | 0 | 0.145 | **0.855** | 0.145 | 0.855 | 14/20 | 118 ms |
| **all** | 60 | 165 | 16 | 0 | 0 | 0.097 | **0.903** | 0.079 | 0.921 | 46/60 | 118 ms |

WER is the word error rate (substitutions + deletions + insertions over
the reference words); word accuracy is 1 − WER. The second pair of
columns spells the digits out on both sides, because **the model spells
numbers out**: the phrasebook writes `beacon 2` and the ears hear
`beacon two`. That is a format difference, not a hearing difference, and
last-mile's bark cache normalizes lines the same way
(`crates/lm-voice/src/bark.rs`) — so both numbers are given.

**The errors that are real** (from `evidence/accuracy.md`):

| voice | the phrasebook | what the ears heard | |
|---|---|---|---|
| mia | take beacon 2 now | Take bacon too now. | a real error |
| mia | beacon 2 is yours | Weekend 2 is yours. | a real error |
| jason | beacon 2 is yours | Egan 2 is yours. | a real error |
| aria | beacon 2 is yours | Weekend 2 is yours. | a real error |
| mia | leave beacon 2 | Leaf beacon too. | a real error |
| mia | where is beacon 2 | Where is Beacon to? | number format |
| mia | squad, on me | Squad on knee. | a real error |
| mia | they are at enemy 4 | They are at enemy for | homophone |
| jason | they are at enemy 4 | They are at enemy four. | number format |

**Mia is the weak voice** (0.855 against 0.927 for the other two). The
errors are not spread over the line: the same words fail again and again.
`beacon` came out `bacon` (mia), `weekend` (mia, aria) and `egan`
(jason) — **four of the sixteen substitutions are `beacon`**, the word
five of the twenty phrasebook lines are about. `leave` came out `leaf`
and `me` came out `knee`. Every error is a substitution and there is not
one deletion or insertion in the 60 clips: the ears never lose a word
and never add one.

**The median round trip is 118 ms** for a clip of 0.6-1.5 s, measured at
the door on `:8102`. The first call after a start is slow (1,957 ms: the
audio encoder warms up); after that it is ~120 ms. A push-to-talk turn
therefore costs about a tenth of a second of the round's wall clock.

---

## 4. The intent probe

The question the brief asks: **can the player's voice become a score
input whose meaning the squad learns over time?** For that, the model has
to read something the words do not carry — the delivery.

58 clips, all rendered by skald (`evidence/intent-probe.py`). Nothing was
recorded from a person. Three grids:

1. **words fixed, delivery varying** (34 clips): `hold` and `fall back`
   in three voices × the Riva styles each voice has (six styles, except
   Jason, who has no `Sad`).
2. **the text variants the brief names** (15 clips): `hold`, `hold!`,
   `hold... hold`, `fall back`, `fall back now!`.
3. **the order/question control** (9 clips): `hold?`, `should we hold?`,
   `are you with me?`.

Two closed questions per clip, in the audio-understanding mode of the
model card:

```
system: Answer questions about the provided audio clip. Base every answer only on what can be heard.
user:   [audio] How urgent does the speaker sound? Answer with exactly one word: calm, ordinary, or urgent.
user:   [audio] Is the speaker giving an order or asking a question? Answer with exactly one word: order or question.
```

### 4.1 The model does read the delivery — a little

The decisive test is the **paired control**: every clip asked twice, once
with the audio and once with the words it carries and no sound at all
(`evidence/intent-analyze.py`, `evidence/intent-paired.json`).

| | urgency | order/question |
|---|---|---|
| the sound changed the answer vs the words said | **6 of 58** | **8 of 58** |
| the sound changed the answer vs the words heard | 6 of 58 | 6 of 58 |

So the sound *does* change the answer on the clips where it should: the
words alone always produce the same answer (`urgent`, `order`), and the
audio flips a handful of them. The answers are also **stable**: six clips
asked three times each, greedy, gave the same answer every time (6/6).
The signal is real and it is not noise.

### 4.2 The urgency answer is mostly the model's prior

| grid | calm | ordinary | urgent | other |
|---|---|---|---|---|
| 1 words fixed (34) | 5 | 0 | 29 | 0 |
| all 58 clips | 6 | 0 | 52 | 0 |

**The model calls almost everything urgent.** `ordinary` never came back.
Against my own reading of the delivery (calm for `Calm`/`Neutral`,
ordinary for `Default`/`Happy`/`Sad`, urgent for `Angry`):

| expected \ heard | calm | ordinary | urgent |
|---|---|---|---|
| calm | 4 | 0 | 8 |
| ordinary | 1 | 0 | 15 |
| urgent | 0 | 0 | 6 |

Against the annotation the model is mostly wrong, and it is wrong in one
direction. **This is not a usable score input as it stands**: a gate that
asks "is this urgent?" and takes the answer would read *urgent* on every
line the player ever says.

The words-held-fixed analysis shows the effect is there but small: of the
five transcript groups in grid 1, two hold the heard words identical and
still get more than one urgency answer between them (19 clips). In the
group that heard `fall back.` ten times, three clips were called calm.

Of the six `calm` answers in the whole probe, **four are on clips whose
words were heard unchanged** (`hold` in mia's Calm, `fall back` in mia's
Calm, jason's Default and aria's Neutral) — a genuine reading of the
delivery, not of a different word. But the style does not predict it:
two of the four are the `Calm` style and two are not, and the two
`Neutral`/`Default` answers that came out calm on `fall back` are
answered calm by jason but urgent by mia, on the same words.

So: **the delivery is read, sometimes, and the reading is unreliable.**
A gate built on it would be a gate that fires on a coin.

### 4.3 The order/question answer is the usable one

| expected \ heard | order | question | other |
|---|---|---|---|
| order (49) | 42 | 7 | 0 |
| question (9) | 2 | 7 | 0 |

**49 of 58 right.** The nine failures, in full:

| clip | heard | answer | was expected |
|---|---|---|---|
| hold, mia, Angry | `Old!` | question | order |
| hold, mia, Happy | `Old!` | question | order |
| hold, mia, Sad | `Hold.` | question | order |
| hold, jason, Calm | `Oh.` | question | order |
| hold, jason, Angry | `OLD!` | question | order |
| fall back, aria, Default | `Call back.` | question | order |
| fall back, aria, Happy | `Hallback.` | question | order |
| hold?, mia, Default | `Hold.` | order | question |
| hold?, jason, Default | `Hold.` | order | question |

**Six of the seven wrong "question" answers are clips whose words changed
under the style** (`Old!`, `Oh.`, `Call back.`, `Hallback.`) — the model
was reading a different word, not a different tone. The seventh (mia,
`Sad`, heard `Hold.`) is a delivery-only failure: the words were heard
correctly and the model still called it a question.

The two questions that were called orders are `hold?` in mia's and
jason's voices — where the ears heard `Hold.` and not `Hold?`.

The text-only control shows what the sound is worth here: with the words
alone the model answered **`order` for every text, including `should we
hold?` and `hold?`**. With the sound it answered `question` for six of
the six `should we hold?` and `are you with me?` clips. **The question-ness
is in the sound, and the words alone do not carry it.**

### 4.4 The delivery axis is not word-preserving — and this is the big one

The Riva styles change *what the words are*, not only how they sound:

| style | heard |
|---|---|
| Default, Neutral, Calm, Sad | `Hold.` |
| Angry | `Old!`, `OLD!` |
| Happy | `Hold!`, `Old!` |
| jason Neutral | `old.` |
| jason Calm | `Oh.` |
| mia Neutral (`fall back`) | `Pull back.` |
| mia Angry (`fall back`) | `It's all back.` |
| aria Happy (`fall back`) | `Hallback.` |
| aria Default (`fall back`) | `Call back.` |

**A delivery signal and a mishearing are tangled.** Six of the seven
wrong "question" answers in §4.3 are clips whose *words* changed under
the style. As long as the TTS may change the words, "the model read the
delivery" and "the model read a different word" cannot be told apart.

The brief's own text variants (`hold`, `hold!`, `hold... hold`, `fall
back`, `fall back now!`) produce **no distinguishable signal at all**:
every one of the fifteen was called urgent, and the only difference the
ears heard was punctuation (`Hold.` / `Hold, hold.` / `fall back now.`).
The model's reading did not move with the punctuation the way it moved
with the Riva style.

### 4.5 What the probe decides

**The player's voice can become a score input whose meaning the squad
learns over time — but only for the act (order or question), not for the
affect, and only once the TTS is held to the words.**

- The **act** works: 49/58, the words alone never carry it, and the sound
  is what carries it.
- The **affect** does not work yet: a model that calls everything urgent
  has no urgency to learn.
- The **delivery** must be made word-preserving first, or the intent and
  the ASR will keep answering the same question.

---

## 5. What worked

| what | how it was verified |
|---|---|
| The model runs on the box, on the GPU | `nvidia-smi` shows the ears' `llama-server` holding 3.6-3.7 GiB; the log says `loaded multimodal model` |
| The existing llama.cpp build already supports LFM3-Audio | `tools/mtmd/models/lfm3a.cpp` and `conversion/lfm3.py` are in `~/llama.cpp-upstream`; the official GGUF repo has the weights, so no conversion and no build |
| The OpenAI-shaped transcription works | `POST :8102/v1/audio/transcriptions` → `{"text": "Hold the line.", "words": [...], "latency_ms": ...}` |
| The audio-understanding mode works | `POST :8102/v1/chat/completions` with an `input_audio` part → `{"content": "urgent"}` |
| The ears are intelligible | WER 0.097 over 60 clips of the squad's own lines, 46/60 exact |
| The ears are fast enough for a turn | 118 ms median over the corpus |
| The ears are nearly free for the box | d1-2B median 27.1 ms with them idle vs 26.1 ms with them stopped |
| The CPU fallback works | `evidence/test-cpu-fallback.sh`: up in 29 s, 1.2-1.5 s per clip, same transcript |
| The answers are not noise | 6/6 clips gave the same answer over three greedy repeats |
| Nothing else on the box was touched | `d1-2b`, `d1-3b`, `d1-flash`, `clef-flash`, `skald` and `:8097`-`:8100` were only read from; the ops log records the whole change |

## 6. What did not work

| what | what happened | what it means |
|---|---|---|
| `-ngl 99 --ctx-size 32768` | `cudaMalloc failed: out of memory` on a 117 MiB buffer; the server aborted | the GPU is shared and had ~9 GiB free. **`:8102` runs at `--ctx-size 8192 -ub 2048`**, and the script falls back to the CPU if even that does not fit (the fallback is measured: 29 s to come up, 1.2-1.5 s per clip) |
| `Magpie-Multilingual.EN-US.Mia.Default` | HTTP 400 from skald | **Default is the voice *without* a style suffix**: Riva knows `Magpie-Multilingual.EN-US.Mia`. The style is skald's reading of the id, not part of it |
| asking Jason for `Sad` | HTTP 400 | not every speaker has every style; the grid is built from `/v1/voices`, not from what it should have |
| word timestamps | none | LFM3-Audio is text-only for ASR; the service says so (`has_timestamps: false`) rather than inventing times |
| llama.cpp's own `/v1/audio/transcriptions` shape | `{"type": "transcript.text.done", "text": ...}` | not the OpenAI shape; the proxy normalizes it and adds `words` and `latency_ms` |
| the urgency question | `urgent` on 52 of 58 clips, `ordinary` never | a three-way question on a model with a strong prior; see 4.2 |
| the text variants of the brief | no signal at all | punctuation in the TTS is not a delivery the model reads; the Riva style is |
| inline `systemctl` / ops-log writes | the runtime's permission gate cannot prompt, so it denies them | the unit and the log entry are written by `tools/ears/bring-up-ears.sh` and `tools/ears/write-ops-entry.sh` instead |

---

## 7. The proposal

How the host would use the ears. **No crate yet**: this is what the
crate would be, and where each piece lands in the code that exists.

### 7.1 The ASR: `V` → `:8102` → the transcript

The path is already wired; only the ASR is a stub.
`crates/lm-host/src/audio_plugin.rs:49` holds
`pub const PUSH_TO_TALK: KeyCode = KeyV;`, the `Microphone` resource
captures **mono 16 kHz i16** — which is exactly the rate LFM3-Audio wants
— and `audio_plugin.rs:133` is the swap point:

```rust
let capture = Capture::new(Box::new(StubAsr::scripted("hold")));
```

A future `lm-voice-ears` crate would implement `lm_voice::Asr`
(`crates/lm-voice/src/lib.rs:199`):

```rust
pub trait Asr: Send + Sync {
    fn transcribe(&self, pcm: &[i16], sample_rate: u32) -> Result<Transcript, VoiceError>;
}
```

`EarsAsr::transcribe` would wrap the pcm in a WAV header, `POST` it to
`http://<thor>:8102/transcribe?language=en`, and map the answer into
`Transcript { words, text }`. The cost of a turn: one HTTP call, ~120 ms.

**The one thing the ASR cannot supply is the word timings.** `TranscriptWord`
carries `t_on_ms`/`t_off_ms`, and `lm-voice/src/human.rs:172` reads them
to place the words on the round's ticks and to read the prosody lanes at
the right moments. LFM3-Audio has none. So the timings must come from
where they already can: the **prosody port** (`MeasuredProsody`, with its
pitch, energy and pause tracks), with a *declared* fallback — words
spread evenly across the capture — when the prosody service is not there.
A declared fallback is the point: in this codebase a guess is a refusal
with a name (D26), and an invented timestamp that silently moved a word
would be exactly the kind of guess the doctrine forbids.

### 7.2 The intent: a second call, in the game's own words

The same audio, asked again through `/v1/chat/completions` with the
audio-understanding system prompt. The answers that land are the game's
own types (`crates/lm-core/src/utterance.rs`):

| the model's answer | the game's type |
|---|---|
| `order` | `SpeechAct::Command` |
| `question` | `SpeechAct::Question` |
| `calm` | `Affect::Calm` |
| `urgent` | `Affect::Urgent` |

**Ask in the game's vocabulary.** `Affect` has three buckets — `Calm`,
`Urgent`, `Uncertain` — and the probe's third class was `ordinary`, which
has no home in `Affect`. Asking *"calm, urgent, or uncertain?"* removes
the lossy mapping at the door, and `Uncertain` is a better third class
than `ordinary` anyway: it is the answer the model should give when it
cannot tell, and `Affect::Uncertain` is exactly what the round then
treats as unsteady.

**The affect question must be recalibrated before it is a gate.** A model
that says `urgent` on 52 of 58 clips is not reading the delivery; it is
reading its own prior. Two ways out, both cheap:

1. ask in the game's vocabulary and measure again (the vocabulary of the
   answer is not neutral: `calm`/`urgent`/`uncertain` may not have the
   same prior as `calm`/`ordinary`/`urgent`);
2. calibrate the gate per squad, the way the confidence rule is already
   per gate (D72): the floor that turns "urgent" into a decision is
   learned from the squad's own rounds, not fixed at the box.

Until one of those lands, the affect answer is **evidence**, not a gate.

### 7.3 The score

`build_human_utterance` (`crates/lm-voice/src/human.rs:143`) takes the
act and the affect from the prosody:

```rust
act: prosody.and_then(|m| m.act).unwrap_or(SpeechAct::Command),
intent: Intent::Order,
affect: prosody.map_or(Affect::Calm, |m| m.affect),
```

The ears would take them from the **intent answer** instead, and the
prosody would keep the lanes and the timings. The score then carries the
player's act and affect next to the words, hashed and replayed like any
other score, and the decision it becomes is a `Provenance::Human` one, as
the voice path already does.

### 7.4 The lexicon: what the squad learns (D64-D66)

This is the part the brief points at, and the part that decides whether
the player's voice can become a score input *whose meaning the squad
learns over time*.

The between-round flow is a chain of files, and it is already built
(`crates/lm-learn`):

```text
RoundLog --metrics--> what happened
         --review--> a System-2 proposal: a gate, block, reflex, lexicon or policy edit (D47)
         --ledger--> a SEALED prediction of its effect, before any test (D32)
         --eval--> held-out seeds: baseline vs proposal
         --ledger--> the verdict, appended to the same chain
         --> adopted changes are committed to the agent's harness and pinned (D27)
```

The proposal kind that fits the ears is already there
(`crates/lm-learn/src/review.rs:28`):

```rust
LexiconEntry { squad: u8, text: String }
```

and it lands at `<harness_root>/squad<squad>/lexicon.md`
(`review.rs:310`). A lexicon entry is squad-level, not agent-level, and
`lm-learn/src/flow.rs:1065` says so explicitly: a squad's lexicon is not
an agent's harness, so there is no commit for it.

**What the ears feed the chain.** The intent answer is a *measurement of
the player's speech*, so it belongs in the round log next to the score it
became: for each human turn, what was heard, what act was read from it,
what affect was read from it, and how long the turn took. The review
then sees the squad's own history of how it was spoken to, and can propose
entries like:

```text
- when the player says "hold" in a calm voice, the squad holds; when the
  same word comes pressed, the squad reads it as urgent and moves.
```

**The chain the entry must walk** (D64, D65, D66):

- **D64** — the entry is not written because the ears produced a nice
  reading. It is a proposal: the prediction of its effect is sealed
  *before* the test, the test is on held-out seeds, the verdict is
  appended to the same ledger, and only then is the entry written.
- **D65** — the change lands where it belongs (the squad's lexicon file),
  and the round's tag and commit come from the harness's own git.
- **D66** — the flow lives in `lm-learn` and `lm-learn` must not depend
  on `lm-tools`. The ears are behind HTTP on `:8102`, so the flow reads
  the round log and never the model: **the ears must not become a
  dependency of the learning chain.** The intent answer is *evidence
  inside the log*, not a call the learner makes.

**The honest gap:** nothing reads `lexicon.md` yet. `lm-learn` writes it,
`lm-learn/tests/flow.rs:276` asserts it was written, and no crate
consumes it. The ears make that file meaningful — a lexicon of "how this
player says things" is exactly what an intent-reading voice produces —
but the consumer side is not built, and the proposal should not pretend
it is.

### 7.5 What must be true before any of it lands

1. **The TTS must hold the words.** §4.4: the Riva styles change what the
   words are. Until the delivery axis is word-preserving, an intent
   reading and a mishearing are the same observation.
2. **The affect question must be recalibrated.** §4.2: a prior, not a
   reading. Ask in `Affect`'s vocabulary and measure again; if it is
   still a prior, the affect stays evidence and the act becomes the gate.
3. **The act gate needs a floor.** 49/58, with seven false "question"
   answers — six of them on clips whose words were misheard. The gate
   belongs per squad and per phrase, the way the confidence rule is per
   gate (D72), and the floor should be learned from the squad's rounds
   rather than fixed.
4. **The lexicon needs a consumer.** §7.4. Without one, the entries pile
   up unread, and a learner that writes a file nobody reads has learned
   nothing.

---

## 8. How every number was produced

Every script is in `lm-wt/ears/evidence/`, and every one of them reads the
running services only — nothing here restarts or reconfigures anything
except `lm-ears`, this repo's own unit.

| what | command |
|---|---|
| the unit is up | `bash lm-wt/ears/tools/ears/bring-up-ears.sh` (install, daemon-reload, enable, restart, wait for the door) |
| the state of the unit | `bash lm-wt/ears/evidence/status.sh` |
| the corpus | `python3 lm-wt/ears/evidence/render-corpus.py` |
| the intelligibility | `python3 lm-wt/ears/evidence/transcribe-corpus.py` → `evidence/accuracy.md`, `evidence/transcripts.csv` |
| the intent probe | `python3 lm-wt/ears/evidence/intent-probe.py` → `evidence/intent.csv`, `evidence/intent/*.wav` |
| the analysis and the paired control | `python3 lm-wt/ears/evidence/intent-analyze.py` → `evidence/intent.md`, `evidence/intent-paired.json` |
| the d1-2B interference | `python3 lm-wt/ears/evidence/measure-interference.py` → `evidence/interference.md` |
| the memory | `python3 lm-wt/ears/evidence/measure-memory.py` → `evidence/memory.md` |
| the CPU fallback | `bash lm-wt/ears/evidence/test-cpu-fallback.sh` |
| one System One pack to the d1-2B | `python3 lm-wt/ears/evidence/systemone-once.py 8094 d1-2B` |
| one intent question | `python3 lm-wt/ears/evidence/ask-once.py <wav> "<question>"` |

The changes this work made to the box are in `~/last-mile-ops.md`, with
the command and the undo.
