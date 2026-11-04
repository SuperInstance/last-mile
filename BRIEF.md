# BRIEF: look and feel, part 2 — closer, calmer beacons, a visible splat

You are working in a git worktree of last-mile on top of commit 77b3d96 (a look and feel: assets/look/look.toml, crates/lm-host/src/look.rs and look_effects.rs, crates/lm-render/src/look.rs and street.rs). Read that commit's message and docs/design/look-and-feel.md first, and look at the screenshots in docs/design/look/.

The director looked at the after shots. The direction works (warm diorama, readable teams). Three things hold it back; fix them, as tokens in look.toml, smallest change first:
1. **The camera is too far.** Agents are a few pixels tall and the board looks empty. Add the "diorama distance" token (follow distance and default zoom) so that at the default view an agent is clearly readable (aim: an agent at least ~40 px tall at 1080p in the 3D view) and two or three squad members fit in frame.
2. **The beacons dominate.** The yellow pillars are far taller than anything else. Make them read as objectives without towering: shorter, maybe a base/flag shape, still the most readable thing on the map, in a calmer tone that does not fight the team paints.
3. **The hit is hidden.** A hit's splat lands under the target, which hides it. Make the tag moment visible from the default camera: e.g. a bigger, brief burst around the target that settles into the ground splat beside/behind it, in the shooter's paint colour. Keep everything pooled (no spawns mid-round).

Then re-take the after screenshots (same seeds and commands as before; replace the after-*.png files; keep the before ones) and update look-and-feel.md's per-screen notes.

## Rules
Do not touch lm-core, the sim or anything hashed: `lm golden` OK 3/3, no re-pin. `bash scripts/check.sh` must pass and `bash scripts/win-build.sh build -p lm-host --release` must succeed. One build at a time; the ProArt is shared. Only modify files inside this worktree. Do not commit. Do not open any editor or IDE. No sub-agents.

## End with
A short summary: what changed for each of the three, the new after-screenshot paths, check.sh and Windows build results.


## Addendum: CONTINUE (2026-10-30 20:00)
Your previous run stopped when the provider's usage limit was reached. Your work is still here, uncommitted. Do not start over and do not revert: run `git status` and `git diff --stat`, read your changes, finish what is missing, and make check.sh pass. Do not commit. No sub-agents. End with a short summary of what changed, what passed, and anything left open.
