# CONSOLIDATE-REPORT: every lane folded into one trunk

Branch `consolidate`, started at `86a4bd9` (PR #3's head). Written
2026-11-24 by the consolidation lead.

> Completed 2026-11-25 by the director after the consolidation seat's run was
> cut off by its time limit (after the six merges, before the gate).

## What this was

`SuperInstance/last-mile` had ~40 unmerged lane branches and 23 emergency
snapshots, developed in parallel by many agent "lanes" and never consolidated.
`main` was stale (Oct 29), the documented trunk was `m1-integration`, and PR #3
was open and mergeable.

This branch maps every `origin/*` branch, merges the six that carry work the
trunk does not already have, and opens one pull request.

## The map: `BRANCH-MAP.md`

The map is committed alongside this report. The short version:

| Verdict | Count | Branches |
|---|---|---|
| Already contained in `consolidate` | 6 | `accept2`, `auto-cam`, `escoff`, `fix/win-build-mirror-pruning`, `m1-integration`, `play` |
| Already landed under a different hash (same patch-id, same subject) | 19 | `conform`, `conform2`, `decomp2`, `docsync`, `esc-tune`, `esc-tune-2`, `kidwords2`, `painteff`, `paintpres2`, `phase1-core`, `res-eden`, `res-lucineer`, `rl-spike-2`, `shared-target`, `timing`, `vibe2`, `w2-paint` |
| Superseded by another tip | 3 | `hotzone2` → `hotzone2-r`, `qcache2` → `qcache2-r`, `look` → `look2` (and `docs/thor-transfer`, `docs/thor-transfer-phase6` → `main`) |
| Merged | 6 | `main`, `si-survey`, `hotzone2-r`, `qcache2-r`, `local-words`, `look2` |
| Skipped | 24 | the 23 `snapshot/last-mile-build-*` branches and `rescue/unmerged-work` |

### The decisive test

`git show <c> | git patch-id --stable` over every commit in the trunk's
ancestry, compared with the patch-id of every commit a branch adds. A branch
whose commit has the same patch-id as a trunk commit is a **re-push of work the
trunk already took** — a different hash and a different base, the same change.
That is how fourteen branches turned out to be already-landed work rather than
new work:

| Branch | Its commit | The trunk's copy |
|---|---|---|
| `conform` | `c8c72a1` "crates/lm-vox, the Vox Score core" | `917ce71` |
| `conform2` | `2c55e84` "conform a rendered line to Vox Score marks (E10), on the smoothing pass" | `0539cdc` |
| `decomp2` | `a7cc64d` "decompose vs escalate, measured" | `f68c7eb` |
| `docsync` | `1130a83` "docs: bring the overview docs up to date" | `50ee07b` |
| `esc-tune`, `esc-tune-2` | `b048771`, `32b76a5` "escalate only the move gate by default (D75, D76)" | `b3652e8` |
| `kidwords2` | `4662f06`, `a6acde3` "the kid-words pass" | `a4ad8d4`, `d9baec9` |
| `painteff`, `paintpres2` | `8ab6cb8`, `41aac3f`, `875fada` | `0317180`, `3b77ca6` |
| `res-eden` | `1522127` "EdenLive measured" | `c9b605b` |
| `res-lucineer` | `2aa3339` "Magnus's ecosystem" | `52c2284` |
| `rl-spike-2` | `c6d3174` "the RL spike, measured" | `91987b7` |
| `shared-target` | `c682f90` "shared target and ghost attractor" | `b4efff4` |
| `timing` | `3073060` "where a decision's time goes (N1) … (N3)" | `04e911d` |
| `vibe2` | `64a859f`, `e4d4fa6` | `a9f5b53`, `6a6e903` |
| `w2-paint` | `4a8353c` "paint as hashed world state, protocol v4 (M2a)" | `458fecf` |

`phase1-core` is the same story in a different shape: its three commits append
the **D61, D62 and D63 rows** to `docs/design/decisions.md`, and all three
rows are byte-identical to the rows already in the trunk's decision log.

### The snapshots

The 23 `snapshot/last-mile-build-*` branches are emergency captures of the
Windows build mirrors (`C:\Users\casey\src\last-mile-build-<name>`) taken
2026-11-04. **Each snapshot tree is an old checkout of the repo plus whatever
work-in-progress that mirror held uncommitted** — which is why each one
diverges from the trunk in 47–465 files. Comparing each snapshot tree against
every commit in the repository's history finds its baseline; the residual is
the uncommitted work.

Across all 23 (and `rescue/unmerged-work`), the only content that exists
nowhere else in the repository is:

- **13 seat-scratch `BRIEF.md` / `BRIEF-MERGE.md` files** — task briefs for
  the seats that ran in the mirrors, not source.
- **two `crates/lm-host/src/main.rs` blobs** (`8340a39d9d` in
  `snapshot/last-mile-build`, `d2a7d0737c` in
  `snapshot/last-mile-build-scoreview`). Both are *older* than the trunk's
  `main.rs`: they carry the between-round screen and the score view, but the
  trunk's copy has every functional line of both, plus D70's
  `Box<dyn SysOne + Send + Sync>` fix and the `Result`-returning `backends()`.
  Merging them would be a regression, not a recovery.

Every other byte in every snapshot resolves to the trunk, to another branch
tip, or to the trunk's ancestry.

### `rescue/unmerged-work`

`origin/rescue/unmerged-work` (`b7e77fc`, message "rescue: work that existed
in no commit") is **skipped**: merging it would destroy trunk work. Its *tree*
is an old checkout — its `crates/lm-host-model/src/lib.rs` has no `score`,
`talk`, `voices`, `view` or `wav` modules and its `crates/lm-host/Cargo.toml`
has no `lm-render` or `lm-voice` dependency, all of which its own parent
`608dd1b0` has. Its actual payload is the `look2` work (12 touched files
byte-identical to `look2`) and the `local-words` work (17 byte-identical to
`local-words`), both merged from their own branches.

## The `lm` subcommands builder-ide's rewind wants

`crates/lm-tools/src/main.rs` defines the `lm` CLI. The full command tree on
`consolidate`:

| `lm` subcommand | Present? | Notes |
|---|---|---|
| `lm run` | **YES** | `--seed <u64>`, `--ticks <u64>`, `--header <path>`, `--inject <path>`, `--out <path>` |
| `lm replay` | **YES** | `lm replay <file>` — replays a log and checks every hash it recorded |
| `lm diff` | **YES** | `lm diff <a.lml> <b.lml>` — the first tick that moves |
| `lm branch` | **YES** | `lm branch <parent> --at <tick> --reflex NAME=FILE --param FIELD=VALUE --out <path>` |
| `lm branch --drop` | **NO** | `lm branch` has no `--drop` flag. The *library* has `lm_tools::branch::Change::DropDecisions { source }` (`crates/lm-tools/src/branch.rs:42`), but the CLI does not expose it. The nearest CLI thing is `lm vibe undo <record>`, which reverts an adopted Vibe record — a different thing entirely. |
| `lm dump` | **NO** | there is no `dump` subcommand and no `Dump` variant anywhere in `crates/lm-tools/`. |

The rest of the tree, for completeness: `lm golden` (`--write`, `--file`),
`lm learn` (`--seed`, `--harness`, `--reviewer stub\|router`, `--runs`,
`--ticks`, `--eval-seeds`), `lm gen-trig`, `lm voice` (`--seed`, `--ticks`,
`--bank`, `--voice-toml`, `--sample`), `lm vox analyze|render|conform`,
`lm vibe propose|ledger|state|undo|hear`.

**What that means for builder-ide's rewind** (`core/rewind/bevy.py`): three of
its five calls (`lm run`, `lm replay`, `lm diff`) exist and work; two do not
(`lm dump`, and `lm branch --drop`). Per the brief, nothing was implemented to
close that gap — this is a report of what is there.

## The merges

Six `--no-ff` merges, oldest first:

- `18e3136` merge(main): the Thor-lane research (thor-transfer) and its README pointer
- `d182bc2` merge(si-survey): the SuperInstance survey
- `5d8c558` merge(hotzone2-r): a fair hot zone with a second objective, as a harness variant (still not the doctrine)
- `1102bcb` merge(qcache2-r): question identity, the question ledger and the answer cache (off by default)
- `e467e80` merge(local-words): the utter pass, the realizer A/B harness, and the evidence they produced
- `32bf51d` merge(look2): the look and feel pass - a sunlit toy diorama, readable teams, the tag moment

## The gate

*(filled in below)*

## lm-host, unverified

`lm-host` is a Windows-only Bevy app and cannot build in WSL: `scripts/check.sh`
excludes it, so any `crates/lm-host/` or `crates/lm-host-model/` change is
merged but **not** compile-verified here. The Windows gate
(`bash scripts/win-build.sh build -p lm-host --release`) was **not** run.
The `crates/lm-host/src/keys.rs` test — "one key, one job" — also only runs in
the Windows build and was checked by eye only.

The lm-host / lm-host-model changes merged unverified (from `look2` and `local-words`):

```
crates/lm-host-model/src/map.rs       |   8 +
 crates/lm-host-model/tests/between.rs |   1 +
 crates/lm-host/src/between_plugin.rs  |  45 ++--
 crates/lm-host/src/hud_plugin.rs      | 125 +++++++--
 crates/lm-host/src/look.rs            | 107 ++++++++
 crates/lm-host/src/look_effects.rs    | 462 ++++++++++++++++++++++++++++++++++
 crates/lm-host/src/main.rs            | 310 +++++++++++++++++++++--
 crates/lm-host/src/map_plugin.rs      | 382 ++++++++++++++++++++++------
 crates/lm-host/src/roll_plugin.rs     |  17 +-
 crates/lm-host/src/score_plugin.rs    |  27 +-
 crates/lm-host/src/view3d/agents.rs   | 168 ++++++++++---
 crates/lm-host/src/view3d/effects.rs  | 297 ++++++++++++++++++++++
 crates/lm-host/src/view3d/mod.rs      | 167 +++++++++---
 crates/lm-host/src/view3d/warmup.rs   | 186 +++++++++-----
 14 files changed, 2036 insertions(+), 266 deletions(-)
```

## Hard limits honoured

- **No force-push, no history rewrite.** Every merge is a `git merge --no-ff`
  commit with two parents.
- **Nothing pushed to `main`, `m1-integration` or any existing branch.** One
  new branch, `consolidate`, was pushed.
- **No branch deleted.**
- **`docs/design/decisions.md` was not modified.** No conflict in this
  consolidation forced a genuinely new cross-cutting decision, so no D-row was
  appended.
- **`tests/golden.toml` was not re-pinned.** The world did not move.

STATUS: DONE
