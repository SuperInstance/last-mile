# The agents' voices in the host (D73 wired in)

*2026-10-29/30. The brief: the host plays the agents' lines in skald
voices from the bank, and never waits. `--talk on` is the switch;
`docs/notes/voices-in-host.png` is the HUD.*

The host used to play every agent line as a tone — the stub's ramp,
whatever the line was. Now a round with `--talk on` attaches a `Voices`
handle to the host loop (`lm-host-model::voices`): the map, the bank and
the prefetch queue of D73. A mind's line is **submitted the moment it
arrives** at the host (between the inbox and the sequencer — long before
its airtime), the worker renders it into the bank on its own thread, and
when the line is **due in the ear** the host looks it up: a hit plays the
WAV through Bevy's audio, a pending or missed line plays the tone, and
the miss is counted. Nothing in the sim changed — the audio is
presentation only, and the golden stayed OK 3/3 (`scripts/check.sh`:
all checks passed).

The bank is a per-user cache (`%LOCALAPPDATA%\last-mile\voice-bank` on
Windows; `~/.cache/...` elsewhere) with a size cap — `voice.toml`'s
`[bank]` section says both, `--voice-bank`/`--voice-bank-cap-mb` override
it, and eviction removes whole entries oldest-first. A background
**pre-warm** plays the round's seed once with the stub minds of the
shipped harness — the honest enumerator of the phrasebook, the same one
`lm voice` reports from — and asks the bank for every line it speaks
before the round's own lines arrive. The daemon's address is the
machine's own word: the map names it as the Thor spells it
(`localhost`), and `LM_SKALD_BASE`/`LM_SKALD_PORT` say where it really
is — on the ProArt, `http://192.168.0.78:8000` over Wi-Fi.

## The HUD

The talk row carries the voice's own numbers beside the talk line —
`voice hit H / tone T / pending P` — and the squad-coloured legend of
who speaks in what voice, read from `voice.toml`: `s0 mia+jason` in
squad 0's colour, `s1 aria+jason` in squad 1's
(`docs/notes/voices-in-host.png`, a warm-bank stub round at tick 1372).

## Proving playback without ears

Nobody in the harness can hear the ProArt's speakers, so `--voice-log
FILE` writes one line a clip as the player is handed it — the tick, the
speaker, the origin (hit / pending-tone / miss-tone), the voice and
style, **the sample count and rate**, the duration, the render's own
latency, the bank key and the text. The exit line sums it:

```text
lm-host: voice hit 345 tone 11 pending 11 — rendered 6 failed 0 — gate played 356 cut 0
```

A hit's samples are the daemon's WAV (13 312–43 008 samples at
22 050 Hz in the cold run below); a tone's are the stub's ramp (9 600 at
16 000 Hz). The sample counts are the proof the mixed output reached the
player.

## Measured on the ProArt, over Wi-Fi (2026-10-29, evening)

Seed 1, the shipped harness, `--talk on --agents stub --ticks 3000`,
`LM_SKALD_BASE=http://192.168.0.78`. The pre-warm asked for **6 unique
lines** (of the round's 467 utterances); 356 lines reached the
spectator's ear (squad 0's radio).

| round | hit | tone | pending | rendered | failed |
|---|---|---|---|---|---|
| cold bank (first ever) | 345 | 11 | 11 | 6 | 0 |
| warm bank (second run) | **356** | **0** | **0** | 0 | 0 |

- **Cold:** the round's first ~30 s are the renders' flight time — the
  11 pending-tones land on ticks 4–909 (the first hit is tick 604, about
  20 s in, once the first WAV landed): **96.9 % hits on a bank that
  started empty**. The pre-warm is why the cold round is this good: the
  six asks were submitted before the round's own lines arrived.
- **Warm:** **100 % hits, zero renders** — the bank is the whole warm
  path, exactly as `lm voice` measured on the Thor (480/480).

**Render latency, ProArt over Wi-Fi vs the Thor-local numbers in
VOICE.md** (the sidecar's own measurement, per unique line):

| | Thor local, idle (VOICE.md) | ProArt over Wi-Fi (this run) |
|---|---|---|
| p50 (nearest-rank, n=6) | 642 ms | 5 093 ms |
| range | 0.69–0.83 s | 4.1–7.3 s |

The Thor was not idle this evening (its `/v1/voices` alone answered in
4.7 s during the probes), so this column is Wi-Fi **and** a busy Thor —
VOICE.md's busy-Thor column was 2.5–5.7 s, and the wire adds the rest.
Every render still cleared the 8 000 ms deadline of `voice.toml`, and
the round never waited for any of them: the six renders happened on the
worker's thread while the round played 345 of its 356 audible lines as
hits.

**`--agents thor`:** the wiring works the same; the model behind the
minds is the thin part. One 1 800-tick round spoke two audible lines
("we hold beacon 2", `jason`/`Neutral`), both played as pending-tones
while their renders were in flight; three renders in that window failed
against the busy daemon (the 8 s deadline held the line, the tone
played, the round went on — a probe minutes later rendered the same
line in 4.9 s). Two further thor rounds spoke nothing audible at all,
which is the already-known phase-4 tuning gap ("agents barely speak on
the real model"), not a voice-path issue: the stub rounds' 467 lines
through the same path show the path itself.

## Where it lives

- `crates/lm-host-model/src/voices.rs` — the `Voices` handle (map, bank,
  queue, the hit/tone/pending counters), `speech_of` (the utterance →
  skald ask mapping), the per-user bank root, the pre-warm thread and
  the squad-voice legend.
- `crates/lm-host-model/src/host_loop.rs` — the plan prefetches (at the
  decision's arrival) and the airtime speaks (hit or tone, never a
  wait).
- `crates/lm-host/src/main.rs` — `--talk on`, `--voice-bank`,
  `--voice-bank-cap-mb`, `--voice-log`, the env overrides, the pre-warm
  launch; `audio_plugin.rs` — the played-clip log; `hud_plugin.rs` —
  the voice row; `sim_plugin.rs` — the exit summary line.
- `crates/lm-voice/src/bank.rs` — the cap (whole-entry eviction,
  oldest-first, enforced on store and on open).
- `crates/lm-host-model/tests/voices.rs` — the proofs, against the
  loopback mock daemon: the map, the hit, the pending tone, the miss,
  the host loop end to end, the pre-warm, the bank's place.

Determinism: the voice path is presentation only. The prefetch sits
between the arrival and the sequencer, the lookup between the tick and
the player, and neither touches a decision, a tick or a hash. The
golden stayed OK 3/3 and `scripts/check.sh` printed
`==> all checks passed`.
