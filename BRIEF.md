# BRIEF: E7, grow the piano roll into a conductor's score (one system of staves per agent)

Read docs/design/vox-score.md §11-§13 (the marks, the full score, the tiers) and the existing piano roll in crates/lm-host (and crates/lm-host-model if the roll's model lives there). Today the roll is one lane per agent. Turn it into a **score view**: per agent, a small system of staves on one shared time axis, built from data the round already has, so it works live and in replay.

1. **Staves per agent (from data that exists today; don't invent new sim data):**
   - **Voice:** each utterance as a note bar (start, airtime), its words above it (abbreviated when space is tight), and below it the line's tone/intention/urgency as marks. Use the utterance plan's fields (crates/lm-agents/src/utter.rs); render them as short mark words, e.g. [urgent, to-squad].
   - **Intent:** the System One decisions applied (move/attention/action) as gestures, plus refusals and unsure/escalated answers (D72's Answers::unsure) as hollow marks, so you can see where the mind hesitated.
   - **Gaze:** the agent's attention target over time, drawn as a lane of target ids or colours. This is the gaze staff of §12, taken from the agent's own state.
   - **Tier (§13):** where the reflex tier acted because System One was late or absent (the reflex/fallback counts the HUD already shows), so engine vs. gamepad is visible.
2. **Layout:** a "score" mode beside the current roll (a key from the new single binding table in crates/lm-host/src/keys.rs; its no_key_has_two_jobs test must still pass). The systems are stacked, with the agent name and squad colour at the left like an orchestral score; the time axis is shared with the replay scrubber; the current-tick cursor runs through all systems. Zoom to the last N seconds (a parameter). Keep it readable at 1280x720.
3. **The E7 pass test:** produce two screenshots (live with --agents thor or stub, and in replay) and one side-by-side of two moments: a calm stretch and an urgent one (find them in a real round). Write docs/notes/score-view.md: what each staff shows, where its data comes from, and whether a reader can tell the urgent moment from the score alone, with your honest judgement and what's missing (e.g. no prosody yet: the voice staff's marks come from the plan, not measured audio; Vox Score fills that later).
4. Determinism: the view reads only; no sim changes. The golden must stay OK 3/3.

Build and verify with scripts/win-build.sh (this worktree gets its own mirror). `bash scripts/check.sh` must print '==> all checks passed'. Don't edit docs/design/decisions.md. Do not commit. Answer in English.
