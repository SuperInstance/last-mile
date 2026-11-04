# BRIEF: phase 5 Task 7 (host half): the between-round screen

Read docs/superpowers/plans/2026-10-28-m1-phase5-learning.md (row 7 of the task table and the "done when" line). The headless core is merged: lm_learn::flow (run_learning_round, Reviewer, FlowRecord, override_verdict) and `lm learn`. Read crates/lm-learn/src/flow.rs first. This task adds the screen a person uses between rounds.

Build:
1. lm-host-model (pure logic, tested in scripts/check.sh): `between::BetweenModel` built from a FlowRecord. It exposes what the screen shows:
   - the metrics with deltas vs the previous round;
   - the proposal (a summary plus the diff);
   - the sealed prediction (metric, direction, min delta, held-out seeds) and the seal id;
   - the verdict as measured;
   - the three actions Adopt / Reject / Override.
   An action yields the call into lm_learn (override_verdict when the human goes against the evidence). Also a human request box: its text becomes a deposit (lm_learn::deposits) and can be turned into the reviewer's next input.
   Tests:
   - `screen_shows_the_sealed_prediction_before_the_verdict` (the order of the fields the model exposes);
   - `override_goes_through_the_ledger`;
   - `request_box_writes_a_deposit`;
   - `reject_leaves_the_header_pin_unchanged`.
2. lm-host: when a round ends, run the flow (StubReviewer by default; RouterReviewer when `--reviewer router` is passed) on a background thread, never on the frame, and show an egui window with the BetweenModel. Then "Next round" starts round N+1 with the header the flow wrote. Keep the lm-host part thin.
lm-host is Windows-only and does NOT build here: compile it with `bash scripts/win-build.sh build -p lm-host --release` (read the script header). Take a screenshot of the between-round screen with the host's --screenshot flag if one exists for that state; if not, say so.
Run `bash scripts/check.sh` until it prints '==> all checks passed'. Do not commit. Do not add rows to docs/design/decisions.md (put any decision you make in your final summary instead). Print a summary, judgement calls, and anything unverified.
