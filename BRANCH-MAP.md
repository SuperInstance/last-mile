# BRANCH-MAP: every `origin/*` branch, and what to do with it

Written 2026-11-24 by the consolidation lead, on branch `consolidate`
(started at `86a4bd9`, PR #3's head).

`consolidate` already contains `m1-integration`, `escoff`, `escjudge-land` and
PR #3 (`fix/win-build-mirror-pruning`), so those need no work.

## Method

For every `origin/*` branch that is not already contained in `consolidate`:

1. `git log consolidate..origin/<b>` — the commits the branch adds.
2. `git diff --stat consolidate...origin/<b>` — the surface it touches.
3. **Patch identity against the trunk's ancestry** (the decisive test):
   `git show <c> | git patch-id --stable` for every commit in
   `git rev-list consolidate`, compared with the patch-id of every commit the
   branch adds. If the branch's commit has the same patch-id as a commit in the
   trunk's history, **that work already landed** — the branch is a re-push of
   it, not new work. This is what caught `conform2`, `paintpres2`, `vibe2`,
   `esc-tune-2`, `timing`, `decomp2`, `docsync` and `kidwords2`: each was
   pushed again after the trunk had taken the same change under a different
   hash.
4. **Content, not hashes.** Where a branch's commits are *not* in the trunk's
   ancestry, the blob at the branch tip is compared with the blob at the same
   path in `consolidate` and in every other `origin/*` tip. If the branch
   carries lines the trunk does not, the branch is new work.
5. For the 23 `snapshot/last-mile-build-*` branches, each snapshot tree is
   compared with every commit in the repository's history to find its
   baseline; the residual is the work-in-progress that the Windows mirror held
   uncommitted. Every residual blob is then searched across every `origin/*`
   tip and every historical commit to see whether the content exists anywhere
   else.

Verdicts:

- **MERGE** — the branch carries content that exists nowhere else; merge it
  with `git merge --no-ff`.
- **SUPERSEDED-BY `<b>`** — the branch's content is carried, in the same or a
  later form, by `<b>`; merge `<b>` instead.
- **SKIP (reason)** — the content is already in `consolidate`, or the branch
  is seat scratch, or merging it would destroy trunk work.

## The headline: most of the 40 tips are re-pushes

Twelve branches carry work the trunk already has. Their commit subjects are
identical to trunk commits; only the hash and the base differ:

| Branch | Its commit | The trunk's copy |
|---|---|---|
| `conform` | `c8c72a1` "crates/lm-vox, the Vox Score core" | `917ce71` |
| `conform2` | `2c55e84` "conform a rendered line to Vox Score marks (E10), on the smoothing pass" | `0539cdc` |
| `decomp2` | `a7cc64d` "decompose vs escalate, measured" | `f68c7eb` |
| `docsync` | `1130a83` "docs: bring the overview docs up to date" | `50ee07b` |
| `esc-tune`, `esc-tune-2` | `b048771`, `32b76a5` "escalate only the move gate by default (D75, D76)" | `b3652e8` |
| `kidwords2` | `4662f06`, `a6acde3` "the kid-words pass" | `a4ad8d4`, `d9baec9` |
| `painteff`, `paintpres2` | `8ab6cb8`, `41aac3f`/`875fada` "paint slows and pushes back" / "is there enough paint?" | `0317180`, `3b77ca6` |
| `res-eden` | `1522127` "EdenLive measured" | `c9b605b` |
| `res-lucineer` | `2aa3339` "Magnus's ecosystem" | `52c2284` |
| `rl-spike-2` | `c6d3174` "the RL spike, measured" | `91987b7` |
| `shared-target` | `c682f90` "shared target and ghost attractor" | `b4efff4` |
| `timing` | `3073060` "where a decision's time goes (N1) … (N3)" | `04e911d` |
| `vibe2` | `64a859f`, `e4d4fa6` "the first Vibe Dictionary slice" / "an adopted Vibe change reaches the next round" | `a9f5b53`, `6a6e903` |
| `w2-paint` | `4a8353c` "paint as hashed world state, protocol v4 (M2a)" | `458fecf` |

`phase1-core` is the same story in a different shape: its three commits append
the **D61, D62 and D63 rows** to `docs/design/decisions.md`, and all three
rows are byte-identical to the rows already in the trunk's decision log.

## Cluster 1 — docs and research

| Branch | Adds | Verdict | Why |
|---|---|---|---|
| `main` | `docs/research/thor-transfer/*` (8 files) + one README row | **MERGE** | `main` is the PR base. Its tree is byte-identical to `docs/thor-transfer-phase6`; the only change it brings that the trunk does not have is the Thor-lane research and the README pointer to it. Merging it also makes `main`'s tip an ancestor of `consolidate`, which is what keeps the final PR from conflicting on `README.md`. |
| `docs/thor-transfer-phase6` | the same 9 paths | SUPERSEDED-BY `main` | `git diff origin/main origin/docs/thor-transfer-phase6` is empty: identical trees. |
| `docs/thor-transfer` | the same files, earlier wording | SUPERSEDED-BY `main` | The first cut. `main` adds 31 lines on top of it (the briefing pointed at the code that exists). |
| `si-survey` | `docs/research/superinstance-survey.md` (1 new file) | **MERGE** | The survey exists nowhere else. |

**The README conflict, and how it resolves.** `main`'s README is the
pre-M1 "design phase" README: merging it wholesale would wipe the trunk's
134-line README (status, phases, build instructions, the crates table). The
merge conflict on `README.md` is resolved by **keeping the trunk's README and
adding `main`'s one meaningful line** — the `docs/research/thor-transfer/` row
in the "Repository map" table. Nothing else on `main` differs from the merge
base `28da80f3`, so that is the whole merge.

## Cluster 2 — agents: the fair hot zone

| Branch | Adds | Verdict | Why |
|---|---|---|---|
| `hotzone2-r` | a fair hot zone with a second objective: the `shared-fair` and `shared-fair-walk` harness variants, `crates/lm-agents/tests/hotzone_fair_judged.rs`, `crates/lm-agents/src/shared.rs` (+567 lines), `evidence/hotzone-fair/lm-hotzone-fair.json` | **MERGE** | The hot zone that was judged but *not* adopted as the doctrine (D78): it qualifies on contact, so it ships as a harness variant, not as the default. 18 paths, none of them in the trunk. |
| `hotzone2` | the same hot zone, first cut | SUPERSEDED-BY `hotzone2-r` | 13 of 18 touched paths are byte-identical between the two; `hotzone2-r` (17:04) is the later commit (16:47) and differs in `facts.rs`, `shared.rs`, `engagement.md`, `shared_variants.rs`, `decisiveness.rs`. |

## Cluster 3 — agents: the question cache

| Branch | Adds | Verdict | Why |
|---|---|---|---|
| `qcache2-r` | question identity, the question ledger and the answer cache (off by default), plus the fix that gives the cache and the decompose literals each other's new fields | **MERGE** | 23 paths, none of them in the trunk: `crates/lm-agents/src/{cache,question,ledger}.rs` are new modules. |
| `qcache2` | the same cache without the fix | SUPERSEDED-BY `qcache2-r` | `qcache2-r` is the same work plus commit `5aa26e5`; it touches one path more and 14 files differ (the fix). |

## Cluster 4 — agents: the utter pass and the realizer A/B

| Branch | Adds | Verdict | Why |
|---|---|---|---|
| `local-words` | the utter pass and the realizer A/B harness (`crates/lm-agents/src/utter.rs` +664 lines, `crates/lm-agents/src/config.rs`, `crates/lm-agents/tests/realizer_on_thor.{md,rs}`, `evidence/realizer-ab/*`, `scripts/realizer-*.sh`) | **MERGE** | 19 paths, none of them in the trunk. |

## Cluster 5 — host: the look and feel

| Branch | Adds | Verdict | Why |
|---|---|---|---|
| `look2` | the look-and-feel pass: `crates/lm-render/src/look.rs` (761 lines), `crates/lm-render/src/street.rs`, `crates/lm-render/src/mesh.rs` (+250), `crates/lm-host/src/view3d/{agents,effects,mod,warmup}.rs`, `crates/lm-host/src/{map_plugin,look_effects}.rs`, `docs/design/look-and-feel.md`, the before/after screenshots | **MERGE** | 36 paths, none of them in the trunk. This is the only branch that carries the sunlit-diorama look. |
| `look` | the look-and-feel pass, first cut | SUPERSEDED-BY `look2` | `look2` is `look` plus commit `f9abe3c`; it changes 19 of the 36 paths (the 3D view, the mesh builder, the screenshots) and adds `street.rs`. |

## Cluster 6 — the 23 `snapshot/last-mile-build-*` emergency snapshots

Every snapshot is a single commit whose parent is `608dd1b0`, captured
verbatim from the Windows build mirror `C:\Users\casey\src\last-mile-build-<name>`
on 2026-11-04, before the mirrors were purged. **A snapshot tree is an old
checkout of the repo plus whatever work-in-progress that mirror held
uncommitted** — which is why each one diverges from the trunk in 47–465 files.
Comparing each snapshot tree against every commit in the repository's history
finds its baseline; the residual is the uncommitted work.

**Every one of the 23 is SKIP.** Across all 23 (and `rescue/unmerged-work`),
the only content that exists nowhere else in the repository is:

- **13 seat-scratch `BRIEF.md` / `BRIEF-MERGE.md` files** — task briefs for
  the seats that ran in the mirrors, not source.
- **two `crates/lm-host/src/main.rs` blobs** (`8340a39d9d` in
  `snapshot/last-mile-build`, `d2a7d0737c` in
  `snapshot/last-mile-build-scoreview`). Both are *older* than the trunk's
  `main.rs`: they carry the between-round screen and the score view, but the
  trunk's copy has every functional line of both, plus D70's
  `Box<dyn SysOne + Send + Sync>` fix and the `Result`-returning `backends()`.
  Merging them would be a regression, not a recovery.

Everything else in every snapshot resolves to the trunk, to another branch tip,
or to the trunk's ancestry.

| Snapshot | Closest baseline | Unique | Carried by | Verdict |
|---|---|---|---|---|
| `last-mile-build` | `6e24f9ec` | `main.rs` (older) + brief | consolidate | SKIP |
| `last-mile-build-accept` | `a42e176e` | brief | `origin/accept2` | SKIP |
| `last-mile-build-accept2` | `e0e09228` | brief | `origin/accept2` (466/467 identical) | SKIP |
| `last-mile-build-apply` | `62623f05` | — | `origin/play` (98% identical) | SKIP |
| `last-mile-build-casey` | `fc88ed99` | brief | consolidate's ancestry | SKIP |
| `last-mile-build-esc3` | `f929d6c7` | brief | `origin/conform2`, `origin/esc-tune-2`, `origin/hotzone2` | SKIP |
| `last-mile-build-escrb` | `32b76a58` | brief | `origin/esc-tune-2` (466/467 identical) | SKIP |
| `last-mile-build-fixref` | `8e7d6264` | — | `origin/res-eden` (483/484 identical) | SKIP |
| `last-mile-build-hearvoices` | `e81e0a5c` | — | `origin/local-words` (438/443 identical) | SKIP |
| `last-mile-build-hostfix` | `a42e176e` | brief | `origin/esc-tune`, `origin/accept2` | SKIP |
| `last-mile-build-localwords` | `02338e47` | brief | `origin/local-words` (457/459 identical) | SKIP |
| `last-mile-build-look` | `77b3d965` | brief | `origin/look` (604/605 identical) | SKIP |
| `last-mile-build-look2` | `f9abe3cc` | — | `origin/look2` (604/605 identical) | SKIP |
| `last-mile-build-merge` | `608dd1b0` | — | its own parent (nothing captured) | SKIP |
| `last-mile-build-msurprise` | `1dfdef88` | — | `1dfdef88`, in consolidate's ancestry | SKIP |
| `last-mile-build-p4t9` | `d538535f` | — | `d538535f`, in consolidate's ancestry | SKIP |
| `last-mile-build-p5t7host` | `63e8c6ff` | `BRIEF-MERGE.md` | `origin/esc-tune`, `origin/accept2` | SKIP |
| `last-mile-build-p6t6` | `d1785bc9` | — | `d1785bc9`, in consolidate's ancestry | SKIP |
| `last-mile-build-p6t78` | `0bae7ff3` | — | `origin/conform` (93% identical) | SKIP |
| `last-mile-build-play` | `09bcdb03` | — | `origin/play` (560/560 identical) | SKIP |
| `last-mile-build-realears` | `ddea09c7` | — | `ddea09c7`, in consolidate's ancestry | SKIP |
| `last-mile-build-scoreview` | `75296db6` | `main.rs` (older) + brief | consolidate | SKIP |
| `last-mile-build-timingmerge` | `3392d7fa` | brief | `origin/conform2`, `origin/play` | SKIP |

### `rescue/unmerged-work`

`origin/rescue/unmerged-work` (single commit `b7e77fc`, parent `608dd1b0`,
message "rescue: work that existed in no commit") is **SKIP**: it would
destroy trunk work. Its *tree* is an old checkout — its
`crates/lm-host-model/src/lib.rs` has no `score`, `talk`, `voices`, `view` or
`wav` modules and its `crates/lm-host/Cargo.toml` has no `lm-render` or
`lm-voice` dependency, all of which its own parent `608dd1b0` has. Its actual
payload is the `look2` work (12 touched files byte-identical to `look2`) and
the `local-words` work (17 byte-identical to `local-words`), both merged from
their own branches.

## Branches already contained in `consolidate`

`accept2`, `auto-cam`, `escoff`, `fix/win-build-mirror-pruning`,
`m1-integration`, `play` — `git rev-list consolidate..origin/<b>` is empty
for each.

## Merge order

Newest-dependency-first, so that the newest branch wins a conflict:

1. `main` (Thor research docs + the README row)
2. `si-survey` (the survey doc)
3. `hotzone2-r` (the fair hot zone)
4. `qcache2-r` (the question cache)
5. `local-words` (the utter pass and the realizer A/B)
6. `look2` (the look and feel)

Four conflicts are expected, all in one file each, and each has one correct
resolution:

| Branch | File | Resolution |
|---|---|---|
| `main` | `README.md` | keep the trunk's README; add `main`'s one `docs/research/thor-transfer/` row to the "Repository map" table |
| `qcache2-r` | `crates/lm-tools/src/main.rs` | keep the trunk's CLI; add `qcache2-r`'s `lm qcache` subcommand and its module wiring |
| `local-words` | `crates/lm-agents/tests/realizer_on_thor.{md,rs}` | keep both sides: the trunk's later sections and `local-words`' realizer A/B sections |
| `look2` | `crates/lm-host/src/map_plugin.rs` | keep the trunk's paint/agent drawing and `look2`'s sunlit-diorama look |

`bash scripts/check.sh` runs after each cluster and must print
`==> all checks passed` before the next cluster starts.

## What is deliberately not merged

- Nothing is pushed to `main`, `m1-integration` or any existing branch.
- No branch is deleted (that happens after Casey's review).
- `docs/design/decisions.md` is only appended to, and only if a merge forces a
  genuinely new conflict-resolution decision.
