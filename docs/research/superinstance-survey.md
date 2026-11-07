# Survey: what Casey's own repos (SuperInstance) give the sandbox

*2026-10-29. A read-only pass, through the GitHub API only, over the SuperInstance repos that
precede last-mile: the erised family, the MUD line, the holodeck/plato line, and the tools
around them. The question is not "what gives last-mile" — the fleet record already answered that
(`docs/research/fleet/00-overview.md`, D23–D34) — but "what gives **the sandbox**"
(`docs/design/sandbox-on-otherhere.md`): a paintball match kids change by talking to an AI
co-developer. The sandbox is a different consumer than M1 was, so some repos the fleet record
dropped get re-judged here.*

## Method

- The 28 repos named in the brief: the erised family found by
  `gh api "search/repositories?q=erised+user:SuperInstance"` (which returned exactly the nine
  named repos), plus the 19 others listed. Everything read through `gh api` — repo metadata,
  READMEs, contents listings, key source files, commit lists. Nothing cloned, nothing run.
- The sandbox doc read first; every relevance judgement below is mapped onto its sections:
  **the starting match** (§0), **the Vibe Dictionary** (§3), **Bench and Bolt** (§4), **the
  Glitch Meter** (§5), **Otherhere as stage** (§6), **kids' safety** (§7), **the smallest
  slice** (§8).
- Conflicts are judged against last-mile's hard rules: determinism and bit-exact replay
  (D69), "only decisions travel", never wait (D11), data not code (Otherhere invariant 3),
  no floats in the core, per-agent git harnesses pinned per round (D21/D27/D65).
- Prior coverage: `docs/research/fleet/00-overview.md` and `02-lessons.md` already scouted
  `erised-sequencer`, `erised-cli`, `holodeck-core` and `lau-a2ui` — for M1's sim, not for the
  sandbox. Where this survey re-judges one, it says so and why.

## 1. The table

*(all 28 repos; "size" is the GitHub repo size, C = commit count; last commit is the last push
to the default branch on 2026-10-29.)*

<!-- TABLE_PLACEHOLDER -->

## 2. The high and medium repos, in detail

<!-- DETAIL_PLACEHOLDER -->

## 3. Cross-cutting patterns

<!-- PATTERNS_PLACEHOLDER -->

## 4. The top 5 things to bring into the sandbox

<!-- TOP5_PLACEHOLDER -->

## 5. Surprising, and stale

<!-- SURPRISE_PLACEHOLDER -->
