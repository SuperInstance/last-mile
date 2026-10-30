# VOICE.md — the D67 sidecar, measured

The agent voices on this Thor, pre-rendered through skald, never blocking
the game. Every number below was measured on this box (aarch64, the Thor
itself) on **2026-10-29**, and each section says how. Nothing here is a
promise about another install; the probes that produced each fact are
named so they can be re-run.

## What the service accepts (probed, not assumed)

`POST http://localhost:8000/v1/audio/speech` with `{"input", "voice"}`
answers a WAV, 22 050 Hz mono 16-bit (`curl`, five calls; the WAV header
read back by `lm-voice`'s own parser). The voice surface, probed call by
call:

| ask | answer |
|---|---|
| `voice: "mia"` (alias) | 200, WAV |
| `voice: "Mia.Calm"` (the short form) | **HTTP 400** |
| `voice: "Magpie-Multilingual.EN-US.Mia.Calm"` | 200, WAV |
| `voice: "Magpie-Multilingual.EN-US.Jason.Angry"` | 200, WAV |
| `voice: "Magpie-Multilingual.EN-US.Jason.Sad"` | **HTTP 400** |

The facts that follow:

- **The emotion style is the suffix of the voice id**, not a parameter of
  its own. There is no rate, pitch or emotion field on the daemon (its
  own `app.py`: "Speed is a client-side concern (playbackRate)"). The
  score's prosody therefore reaches skald as the **choice of voice id**:
  `canonical_voice_id(speaker, style)` folds the two into the one string.
- **The style palette is per speaker.** `/v1/voices` lists 399 voices,
  14 speakers; Mia and Aria carry all six styles (Default, Neutral, Calm,
  Angry, Happy, Sad), Jason carries five — no Sad. The shipped
  `voice.toml` sticks to styles every shipped speaker has
  (`uncertain → Calm`, not `Sad`); this was found the honest way: the
  first `lm voice` run failed 2 of 6 renders with HTTP 400, and the probe
  above is why.
- **The engine is not byte-deterministic.** The same ask, five times,
  answered 43 052 / 45 100 / 47 148 / 43 052 / 49 196 bytes. The bank
  therefore also *stabilises* playback: one clip, filed once, is the clip
  every later round hears.

## Where the ears are (the ASR path)

skald's rule — audio "works" only when a recognizer transcribes it —
needed an ASR, and finding one was part of the work:

- the skald daemon (`:8000`) proxies TTS only; its `/v1/audio/transcriptions` is **404**;
- Riva's own HTTP surface (`:50000`) *has* `/v1/audio/transcriptions`
  (Whisper-shaped multipart), but **this deployment carries TTS models
  only** — it answered `{"detail":"Model not found for language …"}` for
  every spelling tried (`en`, `en-US`, `en_US`, `English`, `en-GB`);
  `~/src/skald-riva/model-repo-official/` holds `magpie_tts*` and
  `tts_preprocessor` and nothing else;
- the ears that hear are **faster-whisper** in the venv skald's own smoke
  test uses — `/tmp/whisperenv` (`tiny.en`, CPU, int8), exactly
  `daemon/server/test_smoke.py`'s recipe.

A hand probe of the full round trip: *"Hold the north beacon. Contact
east, fall back to the rally point."* came back as *"Hope the North
Beacon, contact East, fall back to the rally point."* — homophone-level
divergence, the class skald's gate calls a pass. The gate
(`lm_voice::asr`) asks the **door on the Thor** first (`:8102`, the
LFM3-Audio-2B speech-to-text of `docs/research/ears/EARS.md` — a real
ASR model, and the same door the game asks when the human speaks), then
Riva (the day its deployment carries ASR models, it is the better ear),
and falls through to the venv; when none of them answers, `lm voice`
prints `intelligibility: NOT VERIFIED — no ASR on this box answered` and
the reasons, and claims nothing. That branch was exercised by pointing
the map at a dead port and a missing venv.

## Render latency (the number that shapes everything)

Single-line `curl` against the daemon, "hold the north beacon":

| regime | latency |
|---|---|
| Thor idle | 0.69 / 0.69 / 0.71 / 0.78 / 0.83 s |
| Thor busy (another seat's `rustc`, GPU 68 %, `skald-riva` at 40 % CPU) | 2.8 / 3.1 / 3.1 s |

The brief's own figure — about 2.3 s for a 2.7 s line while busy — sits
inside the measured band. **No line is ever rendered where it is spoken**:
at 30 Hz, even the idle case is ~20 ticks, and the busy case is a
round's-worth of thinking. Hence the bank and the prefetch queue, and the
deadline of 8 000 ms in `voice.toml` (the busiest observed render was
5.7 s; 0 failures at 8 s across every run after the style map was fixed).

## `lm voice` — the report

`cargo run --release -p lm-tools -- voice --seed 1 --bank <dir>` plays a
seeded round (3 000 ticks) with the **stub minds of the shipped harness**
(the same gates, policy and phrasebook the game ships; only the model
behind them is a stub), collects every utterance with its arrival tick
(when the host could first prefetch) and its applied tick (when the ear
is due to hear it), maps each through `voice.toml`, and pre-renders
through the real queue and bank.

Measured runs (seeds 1–3; the stub round is played live, threads and all,
so the unique-line count of a seed varies a little run to run — 2 to 6 —
while the utterance count was 480 every time):

| | quiet Thor | busy Thor |
|---|---|---|
| utterances / unique lines | 480 / 6 | 480 / 6 |
| render pass | 6 rendered, 0 failed | 6 rendered, 0 failed |
| render latency p50 / p95 | 642 / 1 315 ms | 2 484–3 589 / 5 207–5 663 ms |
| real-time factor | **0.67** | 2.57–3.14 |
| warm pass (second run) | **480/480 hits, 100 %** | 480/480, 100 % |
| late lines, cold bank | 71 of 480 (15 %) | 216–287 of 480 (45–60 %) |
| late lines, warm bank | **0 of 480** | **0 of 480** |

How each column of that table is produced:

- **unique lines** — distinct (text, voice, style, engine) asks of the
  round; six for seed 1, because barks repeat ("still here", "with you").
- **cache hit rate on a second run** — the same round's every utterance
  looked up again after the render pass: 100 % every run. The bank's
  content-addressed key (FNV-1a 64 over the four fields, `bank.rs`) is
  what makes the second run free.
- **render latency p50/p95** — nearest-rank percentiles of the queue's
  own per-render measurements of this run.
- **real-time factor** — the wall time of rendering over the wall time of
  the audio it produced. Below 1 on a quiet Thor; above it on a busy one
  — which is exactly why the bank, not the renderer, is what keeps pace.
- **late lines** — the prefetch schedule replayed against the round's
  real clock (30 Hz, 33 ms a tick): one worker, each line submitted at
  its arrival tick, each unique line rendered once at its measured
  latency, repeats served by the first render; a line whose clip is not
  ready by its applied tick counts as late. The cold-bank lates are the
  round's first seconds — the six renders start when the first lines
  arrive — and every one of them plays as the tone and the round goes on
  (`PrefetchQueue::speak`, the fallback contract; the miss is counted in
  `QueueStats::fallback_tones`). A warm bank is 0 late, always.

### Intelligibility (the gate, `lm voice --sample 8`)

whisper-tiny.en (the venv above) on a spread sample of the rendered
lines, word accuracy by word-level edit distance:

- typical line: `"still here"` → `"Still here."`, `"with you"` →
  `"with you."` — **1000 ‰** (punctuation is not a word);
- worst observed: `"still here"` → `"Still."` — **500 ‰**, one dropped
  word on a two-word bark, the tiny.en-level drift skald's gate calls a
  pass;
- babble, for contrast, measures ≤ 333 ‰ (`asr.rs`'s unit tests hold the
  cases), and the empty transcript is 0.

The shipped floor is **500 ‰ per line** — it separates the two classes
measured on this box — and the mean is reported beside it (916–1000 ‰
across runs). One earlier run, before the per-speaker style palette was
found, failed 2 of 6 renders on `Jason.Sad` and the report said so by
name; the gate only judged the lines that were actually in the bank.

## What exists in code

- `lm_voice::skald` — [`VoiceClient`](`crate::skald::VoiceClient`): one
  line, one HTTP call, one WAV, with a per-call deadline (ureq's
  per-request `timeout_global`; a held answer yields `VoiceError::Timeout`
  — proven against a mock that never answers).
- `lm_voice::bank` — the content-addressed [`VoiceBank`](`crate::bank::VoiceBank`):
  `<root>/<xy>/<key>.wav` + `<key>.json` (duration, voice, latency,
  engine), written by rename so a lookup never sees a half entry; a key
  whose two files are not both there is a miss.
- `lm_voice::prefetch` — the one-worker [`PrefetchQueue`](`crate::prefetch::PrefetchQueue`):
  `submit` at the decision's arrival, `lookup` → hit / pending / miss
  (never a wait; the pending state is held by a gate the test can close),
  and `speak` — the fallback contract: a line not ready when due plays as
  the existing tone and the miss is counted.
- `lm_voice::voicemap` — `assets/harness/default/voice.toml`: the speaker
  per agent, the style per tone (affect) with an optional per-intent
  override, the daemon and its engine id, the gate's ears. Every entry a
  parameter with a default (D19); the engine id in the bank key means a
  swapped engine can never be served as the old one's clips.
- `lm voice` (lm-tools) — the report above.

Tests: the bank, the client and the queue are proven against a loopback
mock daemon (`mockhttp.rs`) — `cargo test` touches no network. The live
facts live behind `#[ignore]` in skald's lane (`scripts/live.sh`):
`live_skald_speaks` (one render through the daemon) and
`live_skald_round_trip_is_intelligible` (render + hear back, ≥ 500 ‰).

## What is deliberately not here yet

Wiring into the host (`lm-host`'s audio plugin and main) belongs to
another seat and comes later; nothing in the sim or the host's hot path
was touched, and the round's hash does not cover any of this — the audio
is for the human, and the sim never sees it. The roster's own `voice`
field still says `default`; when the host wires up, `voice.toml` is the
map it reads.

The bank grows without bound (one entry per unique ask, ~90 KB a clip);
a cap or an eviction is a harness decision nobody has asked for yet, and
the numbers above say a round's worth of unique lines is a handful.
