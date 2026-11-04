# BRIEF: finish an in-progress rebase (your 3D Tasks 7-8 onto the current trunk)

This worktree is MID-REBASE. Your commit (3D upscale, view modes, fog, render fixes) is being replayed onto m1-integration, which meanwhile gained:
- phase 4 Task 9 (talk): speech ticks in map_plugin/map.rs, a talk HUD row, Speaking spans in the roll, audio_plugin's playback gate, `--talk off|stub|riva`;
- phase 5's between-round screen (between_plugin, wired in the 2D arm of main.rs; options --reviewer/--harness-root/--runs/--learn-ticks/--ticks);
- lm-learn/lm-voice dependencies.
Conflicts: Cargo.lock, crates/lm-host-model/Cargo.toml, crates/lm-host-model/src/map.rs, crates/lm-host/src/main.rs.

Do:
1. Resolve each file keeping BOTH sides fully: your view modes/fog/3D wiring AND the talk + between-screen wiring. MapModel must keep the trunk's speech ticks and your shared fog. In main.rs, the between plugin and talk must work in the 2D view. Since `both` now has egui, wire the between screen in `both` too if it's straightforward; otherwise leave it in 2D and say so.
2. Cargo.lock: `git checkout --ours Cargo.lock` (during a rebase "ours" is the trunk), then `cargo metadata --format-version 1 >/dev/null`.
3. `git add` the resolved files, then `GIT_EDITOR=true git rebase --continue`. These git commands are ALLOWED for this task; no other git writes.
4. `bash scripts/check.sh` must print '==> all checks passed', and `bash scripts/win-build.sh build -p lm-host --release` must finish. Re-take docs/notes/host-3d-seed1.png and host-3d-both.png and confirm by sky fraction (as before) that they still render the world. If they changed, leave the new PNGs uncommitted in the tree.
Answer in English. Print what conflicted, how you resolved it, and the results.
