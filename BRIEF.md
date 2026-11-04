# BRIEF: a good look and feel (lm-host)

You are working in a git worktree of last-mile, a deterministic Rust/Bevy squad game: two small paintball squads in a city, agents that think through local decision models, kids as the audience. Read AGENTS.md, docs/ARCHITECTURE.md, docs/CONCEPTS.md, docs/design/sandbox-on-otherhere.md (§0 and the paint tier) and docs/research/digital-extinction.md (the watchability ideas only) first.

Casey asked for "a good look and feel". Today the host (crates/lm-host: map_plugin, view3d/, hud_plugin, score_plugin, between_plugin, roll_plugin) works but has no art direction. Give it one, and make it feel good to watch and to play.

## Direction
- **The feel we took is a toy diorama: Lemmings meets paintball.** Small, chunky, readable figures; a bright, friendly city; paint as the loud colour on a calm world. Readable at a glance by a ten-year-old: who is on which team, who is tagged, where the paint is, who is talking.
- **Everything original.** No assets from anywhere; shapes, colours and type from code. No imitation of any existing game's art.
- **Team colours** are the sandbox's paint palette: ochre, teal, violet, saffron (two teams use two of them; make the pairing a parameter).
- **Watchability** (re-implement the ideas, don't copy code): a splat at the moment of a tag, stun pips over a tagged agent, a drawn path for the agent you follow or possess, clear silhouettes against the ground, a slightly tilted camera for the 3D view.
- **Motion with restraint:** short, eased transitions (100-250 ms); nothing bobs forever; respect that the sim is deterministic: the look reads sim state and never writes it.

## Do
1. **Style tokens first.** Write `assets/look/look.toml` (palette: ground, buildings, roads, the four paints, UI surface/text/accent; light and shadow; outline width; type sizes) and load it in the host, so every choice is a tunable parameter with a default. Write `docs/design/look-and-feel.md`: the direction above in your own words, the tokens, and a before/after for each screen.
2. **Before screenshots.** Build and run the current host (`bash scripts/win-build.sh build -p lm-host --release`, then run the exe as `scripts/live.sh` or the README describes) and capture each view: 2D map, 3D view (B flips), HUD, score view (E), the between-round screen. Save to `docs/design/look/before-*.png`.
3. **Pass over each screen**, smallest change for the biggest readability gain, in this order: the 3D view (ground, buildings, agents' silhouettes and team colour, camera tilt and light), the 2D map, the HUD (one clear type scale, team colours, who is talking), the score and between-round screens. Then the tag moment (splat + stun pips) and the followed agent's path.
4. **After screenshots** of the same views to `docs/design/look/after-*.png`, and a short list of what changed per screen.

## Coordination
- Another job is adding paint *effects* (slow/bounce) and a first drawing of the paint layer in map_plugin. Do not draw the paint layer yourself; do define the paint colours in look.toml so that drawing can use them, and say so in your summary.
- Do not touch lm-core, the sim, the rules, or anything hashed: `lm golden` must stay OK 3/3 with no re-pin.

## Rules
- `bash scripts/check.sh` must pass and the Windows lm-host build must succeed.
- Only modify files inside this worktree. Do not commit. Do not edit docs/design/decisions.md (propose decisions in your summary). No sub-agents.
- Keep frame time sane on the ProArt: report fps before and after in the 3D view.

## End with
A short summary: what changed per screen, the tokens file, before/after screenshot paths, fps before/after, check.sh and Windows build results, proposed decisions, anything left open.

## Addendum: CONTINUE (2026-10-30 14:15)
Your previous run was cut off by a machine restart. Anything you had done is still in this worktree, uncommitted. Do not start over and do not revert anything: run `git status` and `git diff --stat`, read your own changes, and carry on from there. Keep builds light: the ProArt is shared, so run one build at a time. No sub-agents. Do not open any editor or IDE.

## Addendum: CONTINUE (2026-10-30 15:30)
Your previous run stopped on a server error (500). Your work is still here, uncommitted (look.toml, look.rs, look_effects.rs, street.rs, and changes across lm-host and lm-render). Do not start over and do not revert: run `git status` and `git diff --stat`, read your changes, finish what is missing (after screenshots, the write-up, fps before/after), make check.sh pass and the Windows build succeed. lm-render's hash.rs is changed: the sim's golden (`lm golden`) must stay OK 3/3; if a render hash is pinned somewhere, re-pin it only with the written reason. One build at a time. No sub-agents. Do not open any editor or IDE.
