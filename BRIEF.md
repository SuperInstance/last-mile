# BRIEF: fix the two known issues in docs/ACCEPTANCE.md (the host, between rounds and the V key)

You wrote docs/ACCEPTANCE.md and found these. Fix them now, with evidence.

1. **`V` has two jobs in `--view both`.** It is push-to-talk (audio_plugin.rs PUSH_TO_TALK) and also the 2D<->3D flip (main.rs enforce_view). Move the view flip to a key that nothing else in the host uses (grep every KeyCode in crates/lm-host to prove it's free), and keep `V` as push-to-talk only. Fix the PUSH_TO_TALK comment. Add a test that fails if two bindings in the host share a key, so this can't come back. The test can be a table of the bindings, or a scan if the bindings are already in one place.
2. **The "flow is running" window never shows, and the flow is ~15x slower in the host than headless.** It takes ~30 s at --learn-ticks 300 in the host vs ~2 s for `lm learn --ticks 300`. Find out why with measurements, not guesses: is the flow thread starved by the render loop, polled once per frame, run on Bevy's compute pool, built in debug, or something else? Fix it so the time is close to headless, and so the waiting window is visible from the moment the round ends until the flow answers (screenshot it).
3. **Closing the window mid-flow** leaves the ledger with a seal and a verdict but no flow record. Make that consistent: either the flow finishes (or is cancelled cleanly) before exit, or the ledger records the abort. Pick the smaller correct fix and say why.
4. Update docs/ACCEPTANCE.md: remove the fixed issues, change the key in §5, and re-shoot docs/notes/accept-both.png and the between-screen shot if they changed.

Build and verify the Windows exe the way you did for the guide (scripts/win-build.sh; this worktree gets its own mirror). `bash scripts/check.sh` must print '==> all checks passed'. Don't edit docs/design/decisions.md. Do not commit. Answer in English, with the measured numbers.
