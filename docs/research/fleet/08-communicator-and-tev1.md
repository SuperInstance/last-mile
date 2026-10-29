# communicator (the utterance score, built) and tev1 (a local decision model)

*2026-10-28. Source: [SuperInstance/communicator](https://github.com/SuperInstance/communicator), sent by Casey, plus the E4 test run against `tev1:latest` on the ProArt's Ollama.*

## communicator: D12's utterance score, already built and measured

**What it is.** A human's voice rendered as a **nine-track MIDI score**:
- **five measured tracks:** pitch via YIN (0.03 Hz on synthetic tones), tempo (speech rate → BPM), velocity (RMS), inflection (contour direction), silence (hysteresis segmentation);
- **four judged tracks**, decided by a decision model in one System One call: timbre (Score warm↔cold → CC71), gesture (Choice: speech act → the CC 80–87 bank), intention (Score idle↔urgent → CC74), attention (Choice: addressee → CC91).

The law underneath: **"a calibrated probability IS a continuous controller value"**, and *confidence never drives a CC* (`policy.py` enforces this in code).

**What it measured that last-mile adopts:**

| Finding | Number | Adopt as |
|---|---|---|
| **Presence gates** fix ill-posed questions; re-wording made them *worse* | ATTENTION's garbage separation went from −8 to **+53** once a Noul "is this property even here?" was multiplied into each judged value | **D53:** every judged gate gets an optional presence gate in the same call, multiplied into the lane value (phase 3 gates, phase 4 utterances) |
| Jev isn't bit-deterministic | (`verdict_cache.py`) same utterance + questions + model → same CC | Already covered for replay (answers are logged decisions). Adopt a **live verdict cache keyed by situation key + gate set + model**, so identical situations don't jitter or spend budget (phase 3 Task 5) |
| Calibration by construction | GESTURE acc 1.000 / ECE 0.001; ATTENTION 0.917 / 0.097; INTENTION Spearman 0.978, with a +0.6-level mid-scale urgency bias | Calibrate every gate the same way before trusting it (phase 5 metrics) |
| **Casey's pauses** | median 0.72 s; **33.8% ≥ 1 s, 6.2% ≥ 2 s** | Phase 4 turn-taking: **never end the human's turn on ~1 s of silence**; use communicator's `turn.py` idea (turn end ≠ voice stop). Push-to-talk stays the default |
| Speech rate | ≈ 200–225 wpm | Utterance score tempo defaults (phase 4) |
| Deaf mic | every sample exactly 0 is a perfect detector | Phase 4 mic health check: `zero_fraction == 1.0` |
| Webcam mic | rolls off above ~6 kHz | Don't trust pitch or timbre measurements above that band |
| End to end | WAV + text → gated 9-track `.mid` in 553 ms, one model call | **Phase 4 human prosody capture reuses communicator** (port or service) instead of rebuilding it |

## tev1: a local decision model on the ProArt

`tev1:latest` (Ollama, Qwen-3.5-family, 4.3B, Q8_0, capability `decision`) answers **the same System One API** at `http://127.0.0.1:11434/v1/systemone`. last-mile's Phase 3 client works with it unchanged.

E4 (40 scenes; facts vs frame vs blind):

| model | cond | enemy_visible | action | bearing | reachable | p50 |
|---|---|---|---|---|---|---|
| **tev1 (ProArt)** | facts | **0.97** | **0.93** | **0.72** | 1.00 | 1,336 ms |
| tev1 | frame | 0.65 | 0.40 | 0.75 | 0.95 | 1,373 ms |
| tev1 | blind | 0.38 | 0.40 | 0.15 | 0.97 | 1,338 ms |
| d1-3B (Thor) | facts | 0.97 | 0.97 | 0.38 | 1.00 | 153 ms |
| Jev (cloud) | facts | 1.00 | 1.00 | 0.82 | 1.00 | 229 ms |

- tev1 judges facts as well as d1-3B and handles the bearing much better (0.72 vs 0.38).
- **But it's slow on the ProArt: only 3.1 of its 5.7 GB fit in the 4050's VRAM**, so the rest runs on the CPU (communicator measured 0.34 s per turn under better conditions).
- During play the 4050's VRAM belongs to the renderer (D14).

**D54:** tev1 is a strong **leader / second-opinion** judge.
- **Serve it on the Thor**, which has the VRAM. This is added to the Thor requests.
- On the ProArt, use it only offline or as the `s1_fallback` when the Thor is unreachable, and only if a smaller quant fits beside the renderer.
