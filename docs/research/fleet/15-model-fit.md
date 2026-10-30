# Model fit: what each model has proven good (and bad) at, in last-mile

*A living record. Every line is something we measured or saw happen, with where. Published claims go elsewhere (12-thor-models.md). Update a line when a new measurement changes it. Last update: 2026-10-30.*

## Runtime models (in the game loop, on the Thor)

| Model | Port | Proven good at | Proven bad at / limits | Evidence |
|---|---|---|---|---|
| **d1-2B** (Liquid, LFM Open, $10M cap) | :8094 | System One gates. Move pack p50/p95 164/262 ms; about 11 ms per option, sequential. Confidence floor 0.30 from its own calibration | The intention gate answers "calm" almost always, even in fights (being investigated). The ten-option attention gate is unsure by nature (0.29-0.64) | D62, D70, D72, D74; talk_on_thor.md |
| **d1-3B** (Liquid) | :8093 | A neutral **judge** between two other models' answers (esctune). Floor 0.15, calibrated | 90 ms for a 3-question pack, slower than d1-2B and no better at gates. Near-flat on the intention score | D76; talk_on_thor.md §3 |
| **clef-flash** (Apache-2.0) | :8096 | A second opinion on the **move** gate (D71's 0.98 vs 0.96) | About 570 ms idle to 986 ms busy per single question here, not the 25-39 ms vendor figure. On ten-option talk gates it changed answers mostly for the worse (judge: 35 d1-2B vs 12 clef) | D74, D75 |
| **LFM2-1.2B** (Liquid) | :8092 | Wording a line in **plain words** from the **plan**. 31/31 lines on plan; p50/p95 571/667 ms | Can't write the game's notation (0/14 compilable). Given the facts, it names the wrong beacon (7/10), so give it the plan only. 47.5% of lines miss a 1 s deadline | realizer_on_thor.md |
| **LFM3-Audio-2B** (Liquid) | :8102 | Speech-to-text: 90% word accuracy on skald voices; push-to-talk key-up → line applied in 179 ms p50. Hears order vs question from the sound (49/58) | No word timestamps. Its affect/urgency answers are a prior, not a reading (52/58 called urgent). Reads the word, not the delivery | EARS.md; ears/measurement.md |
| **Riva Magpie 357M** via skald (NVIDIA; production needs NVAIE) | :8000 | Agent voices, pre-rendered into the bank: 642 ms p50 quiet; a warm bank is never late | Its emotion styles change the **words** ("hold" → "Old!"). It is not byte-deterministic. Only six discrete styles; no SSML and no numeric prosody. About 4x slower under load | VOICE.md; E4-E5.md |
| **whisper tiny.en** (faster-whisper venv) | — | A cheap intelligibility check in the voice tests | Drops a word of two-word barks | VOICE.md |

## Build and research seats (agents that write the code)

| Seat (model) | Proven good at | Watch for |
|---|---|---|
| **mcode** (MiniMax) | Long measured investigations that end in honest negatives (talkthor, esctune, E4/E4b, the ears, the nuggets review); strict prior-art surveys; root-causing ("the 50% was a constant answer") | Network timeouts mid-run: resume with a CONTINUE note. It sometimes edits outside its brief, so review decisions.md diffs |
| **zcode** (GLM) | Implementation across crates; Windows host work with screenshots; precise UI fixes (keys.rs, the waiting card); careful protocol code (lm-vox) | Provider rate limits when many runs go at once: restart after a pause. It took a D-number once (D73) |
| **pi + Qwen3-Coder-30B** (local, :8001, no API cost) | Mechanical jobs with a pass/fail check, especially when given exact test cases (the license classifier passed 9/9 on its second pass) | Its first pass is often wrong in a way that looks right (it misparsed "/" and parentheses). Never use it for docs or readings: it invented facts in a README. Always verify its result independently |
| **Muse Glimmer 30B** (Meta, Apache-2.0, local :8005, dense, ~4 tok/s on the busy Thor) | **Correct on the first pass** where Qwen3-Coder needed two: the license classifier brief, unprompted handling of "/", parentheses and WITH, and it excluded our own workspace crates (3 REVIEW, exactly the real ones). Use it when a plausible-but-wrong answer costs more than the wait, or as the **reviewer** for Qwen3-Coder's drafts (different families make different mistakes) | About 3x slower than Qwen3-Coder (4 vs 14 tok/s, same box and load); it reasons before even one-line answers. Not tuned for the Thor |

## Rules of thumb so far
- **Local pi pair (Casey's suggestion, now measured):** Qwen3-Coder for fast mechanical jobs with exact checks; Glimmer for jobs that must be right first time, or to review Qwen's output.
- **Give small models the plan, not the facts,** and ask in the vocabulary of the signal they have (E4b: the question's criteria must use delivery words).
- **Measure every vendor latency on our box before planning against it.** Every figure checked so far was 3-25x optimistic.
- **A second model's opinion is worth it only where its accuracy was measured** (the move gate), not everywhere it's unsure.
