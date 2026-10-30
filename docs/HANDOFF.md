# last-mile handoff (2026-10-30, early morning)

For whoever directs the team next, whether Claude in a fresh session or a person. It covers the state of the work, the machines, the running jobs, the decisions waiting on Casey, the plan, and how to operate. Read it top to bottom once; after that, §6 is the daily loop.

> **Update 2026-10-30 ~05:00:** see docs/MORNING-2026-10-30.md for the overnight state. zcode is out until 2026-10-31 14:38 and mcode hit its plan limit; the unfinished jobs and their worktrees are listed there. New lessons: never run two merges at once (they share `lm-wt/merge`); after a Thor reboot, re-mount the cold shelf with sudo (`sudo mount -o uid,gid /dev/sda1 "/media/caseydigennaro/TOSHIBA EXT"`), restart lm-realizer and lm-sysone-batched, recreate tmux, and push Thor branches through the ProArt (its keyring locks).

## 1. Where the project is

**The trunk is `m1-integration`** (origin, tip ~`0a0994d` plus later merges). `main` has only the PR #1 docs. **Milestone 1 is functionally complete; Casey's hands-on acceptance has not happened yet.**

The game today: two squads of four AI agents fight over three beacons in a generated city. Every agent's System One runs on the Thor (d1-2B). Agents talk over a squad radio: lines are worded by LFM2-1.2B and spoken in skald/Riva voices, falling back to tones when a voice isn't ready. The human commands by push-to-talk: `V` → LFM3-Audio on the Thor → an order on the score. Between rounds, the learning flow proposes a change, seals a prediction, tests it on held-out seeds, and the human adopts or rejects it. Replays are bit-exact (D69).

**How to play it:** `docs/ACCEPTANCE.md`. The fresh build is at `C:\Users\casey\src\last-mile-build-play`:
- `.\target\release\lm-host.exe --agents thor --talk on`
- keys: `V` talk, `1-8` possess, `0`/`Esc` release, `F` follow, `H` hold, `E` roll/score, `B` 2D/3D.

**The direction (Casey, 2026-10-29/30):** the innovation is agentic play and building the game from inside it. Next is **the sandbox**: city tactics as paintball teams in Otherhere (downtown Sitka), which kids change by talking to Bench, the AI co-developer, through a closed "Vibe Dictionary" of typed changes (data, never generated code). It must be fun to watch from the first round. The design is `docs/design/sandbox-on-otherhere.md`, published as an artifact ("Sitka Paint Sandbox"). The theory underneath is `docs/design/questions-as-experience.md`: an agent's experience is its question library; decomposition is a budget dial; the cloud writes questions and the local models answer them.

## 2. Decisions

- **Recorded:** D1-D76 in `docs/design/decisions.md`. The latest: D72 the confidence rule is per gate; D73 voices pre-rendered through skald; D74 D70/D71 as measured; D75/D76 come in with esc-tune (escalate only the move gate; d1-3B floor 0.15) and are **not yet merged**, see §4.
- **Approved by Casey, being applied** (job `apply`, queued on the ProArt), to record as **D77/D78** once merged:
  - **D77:** the default doctrine becomes `advance` (squads meet; evidence: `crates/lm-agents/tests/engagement.md`).
  - **D78:** the learning flow judges on **16 seeds** with "ceil(3N/4) qualified AND exact sign test p < 0.05", at 1500 ticks (evidence: `crates/lm-learn/SEEDFARM.md`: 4 seeds wrongly adopted 4 of 30).
- **Waiting on Casey:**
  1. AGPL: may last-mile become AGPL to reuse Digital Extinction code? Default and recommendation: no; study and re-implement only.
  2. MIG maintenance window on the Thor: plan and risks in `docs/research/fleet/16-mig-on-thor.md` (a tech preview; it breaks Docker/Riva without a workaround). Recommendation: an experiment on a quiet day.
  3. Name Magnus as co-designer and first playtester (sandbox doc, open question 1). Ask Casey before involving him.
  4. The project's own license (our crates set none; `docs/research/licences/rust-deps.md`).
  5. The cold-move / model cleanup is done; the remaining proposals are in `docs/research/fleet/12-thor-models.md`.

## 3. The machines

- **ProArt** (Windows + WSL, 15 GB for WSL, RTX 4050 6 GB): the only machine that builds and runs `lm-host` (Windows-only Bevy), via `scripts/win-build.sh` (one mirror per worktree under `C:\Users\casey\src\last-mile-build-<dir>`). Memory is tight: run at most **2 seats here**. It ran out tonight with 5 seats + Windows builds + Ollama. Ollama (`gemma2:2b`, `llama3.2:1b`, `qwen3:0.6b`, `qwen2.5-coder:7b`) is available for a local pi provider `proart`.
- **Thor** (`ssh thor-lan`, 192.168.0.78, aarch64, 122 GiB unified, JetPack 7.2.1): **last-mile is now its only user**; Casey's other agents signed off. Services (`~/lm-services/last-mile.sh status|start|stop`; the index at :8100 shows the truth):
  - d1-2B :8094 (System One); clef-flash :8096 (second opinion); d1-3B :8093 (judge);
  - LFM2-1.2B :8092 (realizer, unit `lm-realizer`, disabled at boot);
  - LFM3-Audio :8102/:8103 (ears, `lm-ears`); skald :8000 + Riva container (voices);
  - batched System One :8104 (`lm-sysone-batched`);
  - Qwen3-Coder-30B :8001 (`llama-coder30`, pi worker); Muse Glimmer 30B :8005 (`lm-glimmer`, pi worker);
  - the soak timer (`lm-soak.timer`, every 3 h, `~/lm-soak/latest.md`).

  **Every service change is logged with its undo in `~/last-mile-ops.md`.** Stopped and disabled tonight (not uninstalled): OpenClaw (removed, archive `~/openclaw-archive-2026-10-29.tar.gz`), ComfyUI, the Foundry, image portals, jev-rack, the fleet controller/agent, decision-openai, capture, geometry, d1-flash.
- **Drives:** the NVMe (1 TB, ~370 GB free) is warm; the external USB HDD (2 TB, "TOSHIBA EXT", `cold-shelf/` mirrors original paths, links left behind) is cold.
- **Orin** (7 GB): the archive/verify role in the design; unused tonight.

## 4. Work in flight (at handoff time)

**Merged tonight** (selection): urgency fix, ears/push-to-talk, in-game voices, realizer, score view, D70/D71, refusals N2/N4, surprise signal, batched server, engagement study, seed farm, soak, Vox Score core + E4b/E6, model inventory, licences, nugget reviews, sandbox design, questions-as-experience, model fit.

**Pushed branches not yet merged** (merge each with `tools/ops/merge-branch.sh <branch>`; it runs check.sh **and** the Windows build):
- `vox-smooth`: E2 under target, 288 B/s. Clean; merge.
- `res-mig`: MIG research doc. Merge.
- `timing`: N1/N3. **Conflicts with the trunk**; job `timingmerge` resolves it. Its N3 `EscalationLate` must reuse N2's reserved `escalation_late`.
- `esc-tune` / `esc-tune-2`: D75/D76. `esc-tune-2` is rebased but conflicts in `talk_on_thor.rs`; job `esc3` resolves it. Then record D75/D76.
- `docs/thor-transfer`: main's PR #1 docs; fold into the trunk once.

**Seat jobs.** The briefs are in each worktree's `BRIEF.md`: ProArt `/home/proart/lm-wt/<name>`, Thor `~/lm-wt/<name>`.
- ProArt: `apply` (D77/D78), `esc3`, `timingmerge`, `accept2` (refresh ACCEPTANCE.md + README), `sisurvey` (SuperInstance survey; no sub-agents), `autocam` (auto-director camera), `localwords` (ProArt-GPU wording rung).
- Thor:
  - `decomp`: **decompose vs. escalate** on the attention gate, plus the Hodge curl test;
  - `resdec`: Casey's 48 "decompose" repos + the rea-style question ledger;
  - `reslucineer`: Magnus's ecosystem (craftmind, jepa-perception-lab, cuda-self-evolve/modify, the-seed, self-evolve-ai; Orin → Thor portability);
  - `res-prior`: sandbox prior art and kids' privacy;
  - `sharedtarget`: the engagement lever in facts.rs;
  - `resde`: Digital Extinction study; no AGPL code;
  - `rlspike`: ES-trained policy + coaching on the seed farm;
  - `res-eden`: EdenLive as the restyle layer;
  - `conform`: Vox E10 conformer.

**Free GPU batch jobs on the Thor** (no tokens): the E6 LFM3 agreement sweep (`~/lm-batch/e6`, tmux window `e6-lfm3`); the real-pack corpus for offline analysis (`~/lm-batch/packs/`, seeds 4-60, window `packs`).

## 5. The plan (priority order)

1. **Land the in-flight merges** (§4), record D75-D78, rebuild the play build, and refresh ACCEPTANCE.md (job `accept2`).
2. **Casey's hands-on acceptance of M1** with the refreshed guide. Fix what he finds.
3. **Questions-as-experience experiments:** decompose vs. escalate (`decomp`) → if B wins, apply the pattern to every hard gate; anytime packs; per-question model routing; the question ledger as an evidence DAG (rea's shape). Use the pack corpus for offline Hodge analysis: gradient means keep the answer, curl means decompose, harmonic means refuse.
4. **M2: the sandbox's smallest slice** (`sandbox-on-otherhere.md` §8): one Otherhere place (Lincoln Street), paint as hashed sim state, ten Vibe Dictionary blocks, Bench on the radio (radio framing makes latency diegetic, after Magnus's TALKSITKA), one rival move from Bolt, thumbs up/down, "put it back". Fold in: `advance` + the shared-target fact (squads fight), the auto-director camera (watchability), personalities as System Two patches, research findings (prior art, Digital Extinction, Magnus's craftmind-herding and fishing), and Vox Score marks for Bench and Bolt's delivery.
5. **Platform:** MIG experiment (risky; see §2); the batched server becomes the default when the decision model has headroom; RL as a complement if `rlspike` shows coaching works.

## 6. How to operate (the daily loop)

- **Seats.** zcode (GLM-5.3 main; GLM-5.3-Flash via `--model account:zai-individual-coding-plan/GLM-5.3-Flash`) and mcode (MiniMax; **its Token Plan quota ran out tonight**, error 2056, so retry tomorrow). Seats never commit; the director reviews and merges. The pi task runner is free: local Qwen3-Coder (fast, needs exact test cases) and Glimmer (slow, right first time; good as the reviewer); see `docs/research/fleet/15-model-fit.md`.
- **Scheduler.** `tools/ops/sched.sh` with `$LM_OPS/queue.txt` (lines: `thor|proart <worktree> main|flash`) and `running.txt`. Caps: main ≤4, flash ≤3, ProArt ≤2, launches ≥2 min apart; rate-limited jobs re-queue after 10 min. It exits on each clean finish so the director reviews. `tools/ops/relaunch-z.sh <machine> <name> [flash]` appends a CONTINUE note and launches zcode.
- **Merging.**
  - `tools/ops/merge-branch.sh <branch>` for pushed branches;
  - `tools/ops/gate.sh <worktree> <branch> m1-integration <msgfile>` to commit a seat's work and merge;
  - both require check.sh **and the Windows `lm-host` build**. That gate caught two real breaks tonight.
  - Merge one at a time (one fixed `merge` worktree).
- **Hard-won lessons:**
  - **Rate limits:** zcode 429/1302 above ~8 concurrent seats, so stagger launches. **Memory:** the ProArt at 5 seats ran out, and Claude Code reaps background shells under pressure.
  - Never `pgrep -f`/`pkill -f` a pattern that appears in your own command line (it self-matches).
  - Write commit messages and briefs to a file and `scp` them; inline quoting over ssh breaks.
  - Never force-push; push a new branch name instead.
  - Briefs must say "decision backends are `Box<dyn SysOne + Send + Sync>`" (D70).
  - Measure latency only on a quiet Thor; tonight's numbers under load were 10x worse.
  - Seat output is *claims*: verify the key number yourself (pi's first passes, a seat's stale tables).
- **Memory index:** `~/.claude/projects/-home-proart-projects-last-mile/memory/MEMORY.md` (Thor access, models, seats, Magnus, distribute-work).
