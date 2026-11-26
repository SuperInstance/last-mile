# PLATO's School of Music project (1970–1994): last-mile's ancestor

*Source: [Wikipedia, PLATO (computer system)](https://en.wikipedia.org/wiki/PLATO_(computer_system)#University_of_Illinois_School_of_Music_PLATO_Project_(Technology_and_Research-based_Chronology)), which Casey sent on 2026-10-28. Details beyond the article (G. Gooch's "PLATO Music Systems", ERIC ED161421, 1978) are worth reading before Phase 4.*

## What they built

| PLATO | Facts from the article | last-mile equivalent |
|---|---|---|
| **OPAL** (Octave-Pitch-Accent-Length) | A PLATO music language with a compiler, two music text editors, a filing system for music binaries, real-time players and score printing | **The MIDI principle (D8):** a compact symbolic score, compiled once, rendered at the edge |
| **Gooch Synthetic Woodwind** (c. 1972) | A four-voice square-wave synth peripheral on the PLATO IV terminal | **The renderer is an instrument** (D8): the terminal renders, and the mainframe only sends the score |
| **Gooch Cybernetic Synthesizer** (PLATO V) | Sixteen voices, individually programmable or combined | A **better renderer, same score**: Casey's subwoofer analogy, done in 1980 |
| **Random-access audio device** | 17 minutes of pre-recorded audio, **any of 4,096 clips within 0.4 s** | **The bark cache (D25):** short lines pre-rendered and addressed by key |
| **Votrax voice synth** | TUTOR gained `say` and `saylang` instructions | Utterances as first-class score events; TTS at the edge (phase 4) |
| **Pitch recognition** (Peters, 1969, PLATO III) | Filters for true pitch, plus a counter for tone duration, judging trumpet runs, trills and slurs | The **utterance score's lanes** (D12): pitch and duration, measured from the human's voice (M2 prosody capture) |
| **PLATO IV plasma display** | 512×512, 1260 baud, about 180 characters/s | Tiny streams over thin pipes are a solved problem; bandwidth was never the constraint |

## What the research found (and why it matters to us)

- **Immediate feedback** was the consistent win: drills, automatically graded performances, ear-training with instant judgement. That's last-mile's between-round loop (D32) and the "permanent test".
- **Practice volume ≠ learning:** computer users "completed significantly more practice exercises… in significantly less time", with **no significant difference** in post-tests (Sanders 1978; Watanabe 1981). This is why D32 measures adoption on held-out seeds with sealed predictions, not on engagement.
- **Sound quality limited the early work** (random-access audio dropouts, cassette access times). Renderer fidelity mattered for *humans* even when the score was perfect, which is why last-mile keeps human-facing audio quality a phase-4 concern while agents read the score.

## The idea to carry forward: an OPAL-style notation for utterance scores

D12 says an utterance is a score: words timed against pitch, energy, rate and emphasis lanes, plus intent and affect tags. Agents and models see an "annotated text" projection of it. **OPAL shows the shape that projection should take:** a compact, line-oriented notation that a person, an LLM and a decision model can all read and write, and that **compiles deterministically** into score lanes. A sketch for phase 4 (not final):

```
@squad say intent=order affect=urgent to=red
  hold      p+2 a! l2
  the       p0  a.  l1
  north     p+3 a!! l3
  beacon    p+1 a.  l2   ~rest1
```

Each word gets `p` (pitch step from the speaker's baseline), `a` (accent: `.` none, `!` stressed, `!!` emphatic), `l` (length in beats) and an optional `~rest`. `lm-core` compiles this into the utterance lanes (fixed point, deterministic). TTS renders it at the edge, and the hearing physics degrade it for listeners. **Add to the phase 4 plan:** an `opal`-style utterance notation, a parser plus a compiler into lanes, and round-trip tests.
