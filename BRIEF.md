# BRIEF: Casey's hands-on acceptance guide for Milestone 1 (docs/ACCEPTANCE.md)

Casey will sit at the ProArt and accept M1 by playing it. Write docs/ACCEPTANCE.md: a short, numbered checklist Casey can follow in about 20 minutes, one section per phase, each step being:
- the exact command (Windows PowerShell, from C:\Users\casey\src\last-mile-build-accept after `bash scripts/win-build.sh build -p lm-host --release`; give the exe path);
- what to do (keys, clicks);
- what Casey should see (pass) and what would be a fail.
Cover at least: the 2D map + piano roll + HUD (phase 2); `--agents stub` and `--agents thor` (phase 3: agents think, HUD counters move); `--talk stub` (phase 4: speech ticks, the talk HUD row, push-to-talk on the key the code uses, the half-duplex cut); a round ending into the between-round screen (phase 5: prediction before verdict, Adopt/Reject/Override, next round); `--view 3d`, `--view both`, and `V` to switch (phase 6); replay of a saved round (`--replay`); and `lm golden` / `lm learn` from the CLI.
Get every flag and key from the code (crates/lm-host/src/main.rs parse_args, input_plugin.rs, audio_plugin.rs, between_plugin.rs, view3d), not from memory. Verify each step yourself on Windows via `bash scripts/win-build.sh run -p lm-host --release -- <args> --screenshot <path> --frames N`, and put one screenshot per section in docs/notes/accept-*.png, linked from the guide. If a step doesn't work as the code claims, don't paper over it: list it under "Known issues" at the top with what you saw.
Don't change product code. Run `bash scripts/check.sh` (it must print '==> all checks passed'). Do not commit. Answer in English.
