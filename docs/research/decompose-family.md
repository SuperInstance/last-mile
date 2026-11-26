# The decompose family: Casey's and his son's decomposition projects, mapped onto last-mile's questions-as-experience

*Research note, 2026-10-30. Measured, not guessed: every repository below was read through `gh api` only (metadata, `git/trees?recursive=1`, `contents/…` base64-decoded, `commits`, `check-runs`). Nothing was cloned and no code from those repositories was executed. The one thing I did run was last-mile's own Thor, to capture the `attention` gate's real answers for the Hodge computation in §5 — that is last-mile's shipped code being asked a question, and it is the only way to get the numbers the addendum asks for.*

Companion artifacts, all re-runnable, in [`decompose-family/`](decompose-family/): [`hodge.py`](decompose-family/hodge.py) (the decomposition and its 23 self-tests), [`capture_attention.py`](decompose-family/capture_attention.py) (the capture), [`attention_capture.json`](decompose-family/attention_capture.json) (60 real answers), [`analyse_attention.py`](decompose-family/analyse_attention.py) → [`results.txt`](decompose-family/results.txt), [`escalation_triggers.py`](decompose-family/escalation_triggers.py) → [`escalation_results.txt`](decompose-family/escalation_results.txt), [`curl_vs_escalation.py`](decompose-family/curl_vs_escalation.py) → [`curl_vs_escalation.txt`](decompose-family/curl_vs_escalation.txt).

---

## 0. What I inherited, and what I did

The handover said "the previous agent's uncommitted work is in this worktree". **It was not.** `git status` showed one untracked file — `BRIEF.md` itself — and `git diff` was empty. This worktree is at `0a0994d` (the questions-as-experience design note) with no work in progress; the sibling `decomp` worktree is equally clean. The previous agent appears to have stopped before writing anything down.

So this note is the job from scratch. What I did, in order:

1. Read the three design documents the brief names: `docs/design/questions-as-experience.md` in full, `docs/design/vox-score.md` §11–§15, and `docs/design/sandbox-on-otherhere.md` §3–§5 (the Vibe Dictionary blocks, the Bench, the archetypes).
2. Read `crates/lm-agents/tests/talk_on_thor.md` §3 (the escalation record) and the escalation code in `crates/lm-agents/src/sysone.rs`.
3. Listed the 48 repositories with `gh api "search/repositories?q=decompose+user:SuperInstance&per_page=100"` and read every one's README, file tree, license, metadata and last commit through `gh api`.
4. Read the fourteen priority repositories deeply (source files, test files, license text, CI status), and skimmed the other 34 with the same care but less depth.
5. Read the five repositories the addenda name (`morluto/rea`, `plato-tiling`, `cuda-ghost-tiles`, `plato-ghostable`, `plato-afterlife`) the same way.
6. **Captured 60 real answers to the shipped `attention` gate from the Thor** (d1-2B on :8094, clef-flash on :8096), because the addendum's "the attention gate's logged answers" do not exist in this repository — see §5.1, which is the one place where the brief and the repository disagree.
7. Computed the Hodge decomposition of those answers, and tested the addendum's hypothesis directly.

---

## 1. The short version

- **The family is a set of near-duplicate implementations, not a library.** Forty-eight repositories, of which roughly a third carry real substance; the rest are single-file crates of 83–765 lines, a mass "Add MIT license" commit sweep dated 2026-07-12 (26 of 34 in the skimmed set), or design documents with no code at all.
- **There is exactly one repository in the family that already has what questions-as-experience §5.4 asks for**: `quilt-runbook`'s append-only, hash-chained ledger with a fail-closed writer and 53 tests. It is the right shape for a question ledger, but it has no typed provenance and no authority/confidence vocabulary.
- **The Hodge split is real, well-implemented in one repo, and cannot answer the question it is being asked.** Applied to one question's answers, the curl is *identically zero* — measured at 3e-30 on real data, and proved in §5.2. The addendum's hypothesis ("high-curl questions are the ones where decomposition beats escalation") fails, and it fails for a structural reason, not a tuning reason.
- **The rule for "how far to decompose" exists and is arithmetic, not judgement**: `abstraction-planes` stops one level down when the marginal quality gain falls below 0.5 of 10. That is directly portable to questions-as-experience §2's granularity dial.
- **The ghost/decay/resurrect mechanism already exists, with numbers**: `plato-afterlife` gives initial weight 0.1, decay 10% per period, forget below 0.05, and a relevance boost that cannot fall below 0.06 (so a ghost can always resurrect). `plato-ghostable` gives the seven-state lifecycle. Together with `plato-tiling`'s tile shape they are most of §7's mechanism, already written.

---

## 2. All 48, one line each

Ordered as `gh api` returned them. License is the **LICENSE file's** text, not the API field (nine repositories disagree with their own manifest — see §9). "Tests" counts `#[test]` / `def test_` in source or in a test file, read via `gh api`; it is a count of tests written, not of tests run.

| # | repository | license | maturity | what it is | for last-mile |
|---|---|---|---|---|---|
| 1 | `abstraction-planes` | MIT | 20+ pytest tests, last 2026-08-21 | Decomposes an idea down a 6-level plane stack (intent → domain → IR → bytecode → native → metal) and stops at the best one; `src/abstraction_planes/__init__.py` `find_optimal_plane` | **Port the stopping rule.** §2's granularity dial |
| 2 | `ap-compiler` | MIT | **STUB**: 3 files (`CHARTER.md`, `DOCKSIDE-EXAM.md`, `LICENSE`), no code, no tests, last 2026-04-14 | The "Abstraction Planes Compiler Toolkit" is a charter and an exam prompt | **Nothing.** A promise, not a tool |
| 3 | `band-ensemble-rs` | MIT | 38 inline tests in `src/lib.rs`, last 2026-06-08 | 777-line autonomous musical band; Hodge-based tempo negotiation between five `InstrumentRole`s | Learn only: agents negotiating a shared tempo is the disagreement problem |
| 4 | `cot-quilt` | **none** (no LICENSE file) | 5 pytest tests (`tests/test_jev_doctrine.py`) + a committed real run under `runs/2026-10-01-run1/`, last 2026-10-01 | A large model decomposes a chain of thought into a cellular graph, **judges its own decomposition**, and the judged gaps become the next round's questions | **Learn the shape, copy the discipline.** §4.4 and §6 |
| 5 | `entropy-conservation-rs` | MIT | 13 inline tests in `src/hodge_decomposition.rs`, last 2026-07-12 | Hodge–Helmholtz decomposition of an entropy-change matrix with `is_conserved` / `conservation_violation` | Learn: the "did the split lose anything?" question, asked of an energy not of bytes |
| 6 | `Equipment-CellLogic-Distiller` | MIT (root `Cargo.toml` says MIT OR Apache-2.0; `package.json` says MIT) | 23 vitest blocks, last 2026-10-18 | `src/CellLogicDistiller.ts` distils an LLM prompt+response into named `LogicTile`s (origin, logic, transformation, confidence, interface) | Learn: a tile that carries its own confidence and provenance |
| 7 | `exocortex-ast-cpp` | MIT | 13 `test_*` functions in `test_main.cpp`, last 2026-07-12 | Header-only C++17 recursive-descent parser; `exocortex_ast.hpp` | Learn: brace-depth as a segmentation rule (a cheap boundary rule) |
| 8 | `forge-audio` | MIT (README and `Cargo.toml` say Apache-2.0) | 14 inline tests, last 2026-07-12 | `src/lib.rs` splits audio into fixed chunks tagged with RMS, peak, zero-crossing, spectral centroid | Learn: audio tiles; not a question tile |
| 9 | `forge-code` | MIT (manifest says Apache-2.0) | 15 inline tests, last 2026-07-12 | Regex-only source tiling across 6 languages; each tile has language, kind, name, line span | Learn: the tile schema (id / kind / meta) a question library would mirror |
| 10 | `forge-data` | MIT | 27 inline tests, last 2026-07-12 | `src/lib.rs` (708 lines) parses CSV/JSON/TSV into `DataTile`s and does **lossless** `reconstruct_csv` / `reconstruct_json` | Learn: the decompose → operate → reconstruct-losslessly contract |
| 11 | `forge-detect` | MIT (manifest says Apache-2.0) | 25 inline tests, last 2026-07-12 | Sniffs 23 input formats (20 reachable), returns a per-format confidence **lookup table**, and auto-decomposes per format | Learn: route the input to the right decomposer; the confidence table is the part not to copy |
| 12 | `forge-flux` | MIT | **50 tests** (49 inline + `tests/pipeline_tests.rs`), CI green at HEAD, last 2026-07-12 | Byte-payload tiling: one decomposer → N transforms → one assembler, 8 decomposers, 5 assemblers | Learn: the pipeline shape. **Do not copy the conservation ratio — it is a stub** (§9) |
| 13 | `forge-image` | MIT (manifest says Apache-2.0) | 14 inline tests, last 2026-07-12 | Grid / strip / column region tiling with brightness, variance, edge density | Learn: region metrics |
| 14 | `forge-meta` | MIT (manifest says Apache-2.0) | 12 inline tests, last 2026-07-12 | `ForgeRegistry` over a **hardcoded** 16-entry table; `suggest_pipeline` is a hardcoded name sequence | Learn: a catalogue schema. The registry's `test_count` for `forge-detect` is stale (claims 15, actual 25) |
| 15 | `forge-pipeline` | MIT | 18 inline tests, no `tests/`, CI green, last 2026-07-12 | A **linear** stage runner with a JSON side-channel config; `Branch` and `Merge` exist as enum variants and are **never constructed** | Learn the `FilterStage`/`SortStage` shapes; **the crate name on crates.io belongs to another project** |
| 16 | `forge-soniqo` | MIT | 11 inline tests, last 2026-07-12 | `AudioDecomposer` — a near-duplicate of `forge-audio` under a different name | Learn: that this family duplicates itself without noticing |
| 17 | `forge-subtitle` | MIT | 21 inline tests, last 2026-07-12 | SRT/VTT → `SubtitleTile`s, pure transforms (`shift_timing`, `filter_by_time`, `merge_tiles`, `split_long`), reserialise | Learn: transform verbs over tiles |
| 18 | `forge-text` | MIT | 19 inline tests, last 2026-07-12 | Text → `TextTile`s by paragraph / sentence / line / word, `TextKind`-tagged, with `reconstruct` | Learn: the canonical text-tile primitive |
| 19 | `hodge-belief` | MIT | 36 inline tests, last 2026-07-12 | `HodgeDecomposition::decompose` splits a belief vector into evidence / coherence / prior; `HodgeRealization` gives Green's function and spectral gap | **Reuse** (MIT): a three-way split with an interpretation attached — the shape §5 wants |
| 20 | `hodge-belief-c` | MIT | 29 tests in `tests/test_hodge_belief.c`, last 2026-07-12 | The same split again, in C11 (`include/hodge_belief.h`) | Learn: that the same idea was ported to C for no stated reason |
| 21 | `hodge-belief-rs` | MIT **or** Apache-2.0 (one file, two texts — GitHub says NOASSERTION) | 39 inline tests, last 2026-06-11 | Belief flow split into gradient / harmonic / curl; `harmonic_flow.rs` is an **echo-chamber detector** | Learn: "echo chamber" is the right name for a disconnected agent cluster |
| 22 | `hodge-consensus` | MIT **or** Apache-2.0 (one file, two texts — GitHub says NOASSERTION) | **45 inline tests**, `check`/`test` green, `clippy`/`fmt` **red**, last 2026-06-13 | The only real Hodge consensus implementation in the family: opinion graph → Laplacian → CG solve → cycle-space curl | **Reuse the idea, port the code** (§3.1) |
| 23 | `hodge-consensus-rs` | MIT | **0 tests, 0 code**: `src/lib.rs` is one comment line; the README documents an API that does not exist | A documentation-only shell | **Nothing.** Read its README for the *idea* (`D[i][j] = x_i − x_j`) and ignore the repo |
| 24 | `hodge-theory` | MIT (manifest says MIT OR Apache-2.0) | **2 inline tests**; `src/lib.rs` is 83 lines, last 2026-07-12 | `HodgeDiamond` only — the description's "harmonic forms, Hodge decomposition, Laplacian, spectral sequences" are **absent** | Nothing. An overclaimed description is a warning label |
| 25 | `lau-conservation-matrix` | MIT | 66 tests in `tests/integration.rs`, last 2026-07-12 | Doubly-stochastic validation, Sinkhorn balancing, Birkhoff permutation decomposition, Markov mixing time | Learn: Birkhoff — a flow decomposes into permutations |
| 26 | `lau-hodge-decomposition-agents` | MIT | **121 inline tests** across 8 modules, last 2026-07-12 | The most literal "split an agent signal" repo: exact = learned, coexact = told, harmonic = already known | Learn: those three names are better than gradient/curl/harmonic for an audience |
| 27 | `lau-hodge-theory` | MIT | 62 tests in `tests/hodge_tests.rs`, last 2026-07-12 | Proper exterior derivative d, Hodge star ⋆, codifferential δ = ±⋆d⋆, ω = dα + δβ + γ, Betti numbers, spectral sequence | Learn: the "knowledge into derivable / constrained / core-understanding" split |
| 28 | `lau-morse-theory` | MIT | 90 inline tests, last 2026-07-12 | Morse theory: critical points, handle decomposition of a state space, persistence pairs | Learn: critical points are where a decomposition should stop |
| 29 | `lau-numerical-agents` | MIT | 95 inline tests, last 2026-07-12 | LU/QR/Cholesky/SVD, ODE, PDE, quadrature, CFL stability | Nothing. "Decomposition" here is matrix decomposition |
| 30 | `lau-time-series` | MIT | 53 tests in `tests/integration.rs`, last 2026-07-12 | Additive and multiplicative trend / seasonal / residual decomposition | Learn the three-part vocabulary, which is the one most people already know |
| 31 | `logtensor` | MIT | 88 Python tests in 4 files, last 2026-08-09 | Tensor decompositions (SVD, CP, Tucker, QR, Schur, TT) plus `logtensor/utils/tile_library.py` and `tile_library_v2.py` | Learn: somebody in this family already wrote a "tile library" |
| 32 | `neural-plato` | MIT | 53 inline tests across 8 Rust files + 7 Python experiments, last 2026-07-12 | Fortran kernels (sparse memory, amnesia curve, Eisenstein lattice) behind Rust FFI | Learn: `fortran/negative_space.f90` — reconstructing what is inferable from what is absent |
| 33 | `openrooms` | MIT | **49 Rust + 80 Python** tests, CI matrix on 3 Python versions, last 2026-08-07 | Live agent rooms in Rust + a Python bridge + a Cloudflare Durable Object; `python_bridge` carries a `HodgeDecomposition` | Learn: rooms are the right container for a situation-keyed question library |
| 34 | `persona-engine` | MIT | 18 pytest tests in 3 files, last 2026-06-13 | Decompose a voice from audio into named axes, **dial** them, **compose** content through them; `persona_engine/schemas/persona.py` | **Port the decompose/dial/compose split** (§4.3) |
| 35 | `plato-forge-bridge` | MIT (manifest and README say Apache-2.0) | 20 tests in `src/tests.rs` — compiled into the release build, last 2026-07-12 | A knowledge tile `{domain, question, answer, confidence, tags, provenance}` → a `ForgeTick` → rooms, with a dead-letter buffer | **Reuse the tile shape** — it is already a question (§7.1) |
| 36 | `plato-tile-split` | MIT (holder reads "Cocapn") | 7 inline Rust tests; the Python half has **zero** tests; last 2026-05-08 | `TileSplitter.by_sentence / by_paragraph / by_word / by_markdown / by_size` → `SubTile{content, parent_id, index, confidence, tags, metadata}` | **Reuse the shape**: parent linkage + confidence + tags is the split-tree schema |
| 37 | `quilt-neudecide` | **none** (no LICENSE file) | **0 code**: a README and three `perspectives/*.md`; last 2026-10-09 | A pre-proof-of-concept: a 43 MB voice-to-tool model as five reactive cells, with a ledger | Learn: "the sheet is the program — every named cell becomes an MCP tool" |
| 38 | `quilt-runbook` | **none** (no LICENSE file) | **53 tests** in 5 `node:test` files, zero network, last 2026-10-02 | Append-only hash-chained runbooks; `§5a` WHY-records; `§5c` stablepoints; rewind; a miner that compiles recurring WHYs into cells; HMAC signed custody | **Reuse the ledger discipline** (§6). The closest thing in the family to a question ledger |
| 39 | `rusty-series` | MIT (manifest and README say MIT OR Apache-2.0) | 53 tests in `tests/integration.rs`, last 2026-07-12 | A near-verbatim clone of `lau-time-series`: the same 9 module names and the same API | Learn: that a repo can be a fork with no attribution |
| 40 | `self-improving-band` | MIT | **no `src/`, no `Cargo.toml`** — 6 design docs and 3 stdlib-only Python demos; last 2026-07-12 | A 7-layer architecture spec for a 5-agent musical ensemble, with conservation laws and Hodge tempo negotiation | Learn: the design vocabulary, which is good; there is no implementation to reuse |
| 41 | `si-hodge-consensus-demo` | MIT | 36 tests in 5 files, CI **red** (exit 101), last 2026-07-12 | A dense-matrix Hodge demo: Gauss-Seidel, a triangle/4-cycle heuristic for curl, and a strategy harness with hard-coded energy constants | Learn: **it contradicts `hodge-consensus` about curl** (§9) |
| 42 | `spectral-fingerprint` | MIT | 67 inline tests, last 2026-06-09 | An AST → symmetric adjacency matrix → top-64 eigenvalues → L2-normalised signature | Learn: a question library wants a fingerprint like this, not a UUID |
| 43 | `spectral-prosody` | MIT **or** Apache-2.0 (one file, two texts — GitHub says NOASSERTION) | **105 inline tests** across 12 modules, last 2026-06-13 | Graph Laplacians over metrical structure: Fiedler value as metrical coherence, rhyme schemes as spectral radii, isospectral meters | Learn: **the Fiedler value is a cheap coherence score for a graph of questions** |
| 44 | `superinstance-ternary-wavelet` | MIT | 14 inline tests, last 2026-07-12 | Ternary Haar wavelet decomposition with an exact `conservation_audit` and lossy `Fidelity{p,q}` control | Learn: decomposition with a stated fidelity budget — questions-as-experience §2's dial, made concrete |
| 45 | `terminal-hodge-harness` | MIT (README says MIT OR Apache-2.0) | 33 inline tests, last 2026-07-12 | A thin wrapper splitting an error into evidence / coherence / prior and naming the dominant part | Learn: "name the dominant part" is the right output verb |
| 46 | `ternary-form` | MIT | 20 inline tests in the single `src/lib.rs`, last 2026-07-12 | Musical forms as task templates: Binary A-B, Ternary A-B-A', Rondo A-B-A-C-A, Sonata | Learn: a decomposition *plan* is a named form, and A-B-A' (return transformed) is the right shape for a refinement loop |
| 47 | `ternary-mud` | MIT | 36 inline tests across 6 modules, last 2026-07-12 | Z₃ ternary algebra, dungeon genus, `solve_dirichlet`, `hodge_decompose` of traversal flow, a `LostnessReport` | Learn: "lostness" — a report object that says *where* the agent is stuck |
| 48 | `token-wavelet` | MIT | 23 inline tests, last 2026-07-12 | Haar/Daubechies-4 decomposition into trend / oscillation / noise, plus `ModelCommunity` and **structural-hole detection** | Learn: **a structural hole is a missing combination of questions** |

### The five addendum repositories (not in the `decompose` query)

| repository | license | maturity | what it is | for last-mile |
|---|---|---|---|---|
| `morluto/rea` | MIT | 409k stars, 6,820 files, active (last commit 2026-10-30) | Reverse-engineering agent; `src/domain/evidence.ts` defines content-addressed Evidence with typed confidence, authority, mandatory limitations and an evidence DAG; `src/domain/residualUnknown.ts` a revision-chained ledger of residual unknowns | **Port the shape** (§6). The model to copy |
| `plato-tiling` | MIT | 14 files, **no tests**, last 2026-05-08 | `src/plato_tiling/tiling.py`: adaptive chunking with semantic boundaries, `TileType` including `QUESTION`, per-tile confidence | **Reuse the tile shape**; the ghost/resurrect promise is *not* in this file |
| `cuda-ghost-tiles` | MIT | 386 files but **no test source** (only committed `target/` fingerprints), last 2026-04-18 | Learned sparse attention: Q/K scoring with load, experience, skill-gap and affinity terms; five routing strategies; `seed: 42` | Learn the scoring terms. The repo is 18 MB of build artefacts |
| `plato-ghostable` | MIT | 203 files, last 2026-10-18 (a non-`main` branch) | `src/lib.rs`: the seven-state `TileState` lifecycle and `ResurrectionCondition` | **Reuse the state machine** (§7.3) |
| `plato-afterlife` | MIT | 7 files, **no tests**, last 2026-05-08 | `src/lib.rs`: ghost tiles with weight 0.1, 10% decay per period, forget below 0.05, relevance boost ≥ 0.06 | **Reuse the numbers** (§7.3) |

---

## 3. The deep repositories

### 3.1 `hodge-consensus`, `hodge-consensus-rs`, `si-hodge-consensus-demo`

These three are usually read as one idea. They are three different things, and they **contradict each other about what curl means**.

**`hodge-consensus`** (Rust, MIT OR Apache-2.0, 45 inline tests in `src/lib.rs`, `check`/`test` green, `clippy`/`fmt` red, last commit 2026-06-13) is the only real implementation. The mechanism, in `src/graph.rs` and `src/decomposition.rs`:

- `OpinionGraph { agents: Vec<String>, edges: Vec<(String, String, f64)> }` — nodes are agents, edges are signed opinions.
- The Laplacian is `L = D_out − A` built from `w.abs()`, so the **sign of the opinion is stripped from the Laplacian** and survives only in the flow vector.
- The flow is `edges.iter().map(|(_,_,w)| *w)` — the edge weight *is* the flow. **There is no function anywhere in this repository that derives an edge weight from a vector of scores.** The caller supplies a number.
- The gradient comes from a conjugate-gradient solve of `L φ = Bᵀ f` (200 iterations, tol 1e-10), then `gradient_k = φ_i − φ_j` — **unweighted**, dropping the `√w` that `B` carries.
- The curl is the residual projected onto cycles found by a DFS with `max_depth = 8` (`find_simple_cycles`), using a set of **non-orthogonal** indicator vectors, not an orthogonal projection onto `im(B₂ᵀ)`.
- The harmonic part is whatever is left over (`src/decomposition.rs:88-92`).
- `predict_all` decides **per edge** by argmax of absolute magnitudes, with **no ratio cut-off anywhere**. Gradient-dominant ⇒ will resolve; curl-dominant ⇒ will resolve, at confidence `0.5 + 0.5·c_frac`; harmonic-dominant ⇒ will not resolve.

**`hodge-consensus-rs`** (MIT, 0 tests) is a shell. `src/lib.rs` is 34 bytes: `// Hodge consensus implementation`. Its README documents `disagreement_matrix`, `decompose_disagreement` and `consensus_vote` — **none of which exist in any `.rs` file**; a grep for them hits `README.md` and `INTEGRATION.md` only. Its CI is green because `cargo test` on an empty crate reports 0 tests and succeeds. Its `Cargo.toml` declares `name = "hodge-consensus"`, which would collide with the first repository's package name.

**`si-hodge-consensus-demo`** (MIT, 36 tests in 5 files, CI **red**, exit 101) is a separate, cruder implementation: dense `n×n` matrix, Gauss-Seidel instead of CG, and a curl/harmonic split by a per-edge **"does this edge sit in a triangle or 4-cycle?"** heuristic (`src/hodge.rs:126-137`) — a 5-cycle is invisible to it.

**The contradiction, which matters more than the code.** `hodge-consensus` says a curl-dominant dispute **will resolve** ("agents loop but converge"). The demo says a curl-dominant dispute **will not resolve** ("needs external intervention"). The rs README says "partially resolvable — break cycles first". Any note that treats the Hodge family as holding one position is wrong.

**Maturity, honestly.** All three have real tests that assert real invariants (Laplacian row sums zero, PSD, reconstruction within 1e-6), but **no test in any of the three pins a numeric expected decomposition**. The demo's README result table ("HodgeAware 83%") is a consequence of hard-coded constants in `src/experiment.rs:66-71` (`initial_energy − gradient_e − curl_e * 0.5`), not of an experiment.

**For last-mile: port the idea, not the code.** The algorithm is sound enough to reimplement in ~150 lines with the exact Hodge projection that §5.2 shows is needed. The shipped version cannot be run as-is on the attention gate: §5.5 shows the cycle enumeration alone would be 172,974 cycles against 90 edges.

### 3.2 `abstraction-planes` and `ap-compiler`

`abstraction-planes` (Python, MIT, 20+ pytest tests in `tests/test_plane_analyzer.py`, all mocked, last commit 2026-08-21) is the real one. Its idea — "**The Middle Way**: decompose ideas to their optimal abstraction plane, not all the way to bytecode" — is a six-level stack: intent → domain language → IR → bytecode → native → metal, with plane 4 (domain language) named the sweet spot.

The machinery that matters is `find_optimal_plane(intent, target)` in `src/abstraction_planes/__init__.py`:

- `target_floors = {"esp32": 0, "jetson": 1, "cloud": 3, "mud": 2, "agent": 3, "auto": 5}` — **the floor is set by the target**, not by the algorithm.
- It descends one plane at a time. At each plane it asks a large model to score the result on four criteria (correctness, compactness, executability, maintainability) and takes the mean.
- **The stopping rule**: if the score at the current plane is *worse* than the previous by more than 1.0, it stops (diminishing returns); if the improvement is under 0.5 *and* the plane is already at the floor, it stops there (marginal returns); otherwise it descends to the floor.

So the answer to "**how far** to decompose" is arithmetic: **descend while one more level buys more than half a point, and stop where it does not.** It is not a judgement call — though the *score* that feeds it is a model's judgement, and the whole thing needs a large model and the network.

`ap-compiler` is a **stub**: three files, no code, no tests, no README, last commit 2026-04-14 ("[fleet] Add CHARTER — fleet identity"). The "compiler toolkit" does not exist.

### 3.3 `cot-quilt`

JavaScript, **no LICENSE file at all**, 5 pytest tests, last commit 2026-10-01, and — unusually — a **committed real run**: `runs/2026-10-01-run1/` holds the seeds ledger, the judge verdict, the compilations and the coverage table.

The pipeline is six receipted phases: SEEDS → COT → SPLIT → WIRE → MERGE → JUDGE. The parts that map onto questions-as-experience:

- **A large model judges its own decomposition**: thoroughness 0–10 plus up to five gaps. Those gaps become the **next iteration's question cells** — the cloud writing questions, literally.
- **The ledger** (`runs/…/seeds/ledger.jsonl`): one line per call with `row`, `ts_utc`, `run`, `phase`, `service`, `model_requested`, `model_served`, `seed`, `temperature`, `purpose`, `status`, `latency_ms`, `usage`, `finish`, `cost_usd_declared`, `cost_basis`, `request_sha256`, `notes`. Two rows carry `status: FAIL`, and a `ledger-note` row says they were **kept verbatim, never deleted**. That is the rea ledger's spirit, already written.
- **`exports/coverage-table.md`** is the question library's table, with weights, stability classes (core / stable / volatile / critique), and honest verdicts (COVERED / PARTIAL / UNCOVERED / DOCTRINE) each pointing at the evidence.
- **`docs/JEV-DOCTRINE.md`** is written for exactly last-mile's stack ("jev" is a shipped backend name in `crates/lm-agents/src/config.rs`). Three of its rules transfer directly:
  - *"refusal is never faked and absence is never laundered into zero-confidence evidence"* — this is last-mile's D26 split and N2's named refusals, already written down as doctrine.
  - *budget allocated by **RANK**, never by absolute score* — judged scores cluster at the top of a rubric, so absolute scores are not comparable across questions. This is the rule for §5.3's per-question model routing.
  - *divergent routes kept as first-class ALTERNATE ROUTES rather than averaged away* — this is the one-lie-among-five discipline, and it is the correct answer to what to do with a disagreeing second model.

### 3.4 `quilt-runbook`

JavaScript, **no LICENSE file**, 53 tests in 5 `node:test` files, zero network, ~500 ms, last commit 2026-10-02. This is the most valuable repository in the family for last-mile, and it is not about Hodge at all.

Ten laws, each enforced in code and each with a test:

| law | |
|---|---|
| L1 | Append-only ledger; seq contiguous; history is a byte-prefix that only grows |
| L2 | Hash chain: `id = sha256(run_id\|seq\|prev\|body)`; tamper = a hard error naming the seq |
| L3 | Fail-closed: invalid payloads are recorded as `error` steps, never skipped |
| L4 | The `§5a`/`§5c` contract shapes are exact — field names are interop law |
| L5 | A stablepoint's `state_hash` is over the snapshot **bytes**; mismatch = hard error naming the seq |
| L6 | A stale handle refuses to append (`STALE_HANDLE`) — a second writer fails closed rather than duplicating a seq |
| L7 | Rewind never rewrites: a `resumed-from` marker appends; drift is preserved |
| L8 | Only generalising, recurring adjustments compile (≥ 2 per cluster) |
| L9 | Replay re-executes only pure, marked steps; everything else is honestly `skipped` |
| L10 | Custody gate: a sealed run proves custody **before** any resume materialises; resuming below the sealed boundary refuses (`REWIND_PAST_CUSTODY`) |

The `§5a` adjustment record is the shape a question's provenance should take:

```json
{ "kind":"adjustment", "run_id":"…", "at_seq":3, "ts_utc":"…",
  "target": { "cell_id":"ztable", "sheet":"catalog" },
  "before": { "tails":"two" }, "after": { "tails":"param" },
  "why": { "trigger":"judge flagged conflation",
           "hypothesis":"tails must be a parameter, not a value",
           "evidence":["run.jsonl seq 3"] },
  "generalizes": true, "compiled_cell": null }
```

And the miner (`src/mine.js`) clusters recurring `why` records by (target, hypothesis keywords) and emits `§5a`-shaped **compiled-cell proposals** into `compilations.jsonl`, idempotently. That is "a question that recurs gets compiled into a cell" — exactly what a question library wants, and the L8 rule ("only generalising, recurring adjustments compile; a one-off never does") is the rule that keeps the library honest.

It also has signed custody: an HMAC-SHA256 `quilt.organ.checkpoint` seal over the chain, and a "courtroom" (`verifyRunCustody`) that re-derives every anchored snapshot before anything resumes.

### 3.5 `persona-engine`

Python, MIT, 18 pytest tests in 3 files, last commit 2026-06-13. Three verbs, three modules: `decompose` (audio → persona), `dial` (adjust the character), `compose` (content → SSML → piper TTS).

The data structure is the interesting part (`persona_engine/schemas/persona.py`): a `Persona` is **five named sub-vectors** — `CadenceProfile`, `ProsodyEnvelope`, `LexicalFingerprint`, `GrooveParameters`, and a `persona_vector` embedding — plus `confidence`, `smp_seed` (deterministic identity) and `tags`.

The dials (`CharacterParameters`) are the knob set: `pitch_baseline`, `pitch_variability`, `speed`, `breathiness`, `pause_length`, `pause_variability`, `thought_length`, `interruption_tolerance`, `conversational_bpm`, `swing`, `anticipation`, `call_response`, and a "personality proxies" block: `formality`, `enthusiasm`, `humor_frequency`, `technical_density`, `verbosity`, `assertiveness`.

### 3.6 `spectral-prosody`

Rust, MIT OR Apache-2.0 (one file, two texts), **105 inline tests** across 12 modules, last commit 2026-06-13 — the best-tested repository in the family.

Graph Laplacians over metrical structure. `src/metrical_graph.rs` builds a weighted graph whose nodes are lines and whose edge weight is `0.6 × stress_similarity + 0.4 × syllable_distance`; `src/laplacian_scan.rs` computes the spectrum; the Fiedler value λ₂ is the **algebraic connectivity**, i.e. how coherent the structure is; `src/rhyme_adjacency.rs` encodes rhyme schemes as adjacency matrices with distinct spectral radii; `src/iso_breath.rs` tests whether breath-constrained meters are isospectral across languages; `src/linalg.rs` implements Gaussian elimination and the Jacobi eigenvalue method with no external math dependency.

### 3.7 `ternary-form`

Rust, MIT, 20 inline tests in a single 76-line-ish `src/lib.rs`, last commit 2026-07-12. Musical forms as task templates: `BinaryForm` (A-B), `TernaryForm` (A-B-A'), `RondoForm` (A-B-A-C-A-…), Sonata. The third section of a ternary form is labelled **A'** — a return that is *not* an identical copy, because the B section transformed it. That is the right vocabulary for a refinement loop over questions.

### 3.8 `quilt-neudecide`

A **pre-proof-of-concept with no code**: a README and three `perspectives/*.md` files, no LICENSE. The idea — a 43 MB voice-to-tool model decomposed into five reactive cells (`audio_input → audio_encoder → tool_encoder → decoder → tool_call_output`), where "every named cell becomes an MCP tool" — is a good vocabulary for the cell library. It is a document, not a tool.

### 3.9 `forge-flux`, `forge-pipeline`, `plato-forge-bridge`

Three islands with **no dependency between them** and three incompatible `Tile` types.

- **`forge-flux`** (50 tests, CI green) is a byte-payload pipeline: one decomposer → N transforms → one assembler, with eight decomposers (text by paragraph/sentence/line, CSV row, JSON member, SRT cue, code block, audio chunk, image region, sensor reading) and five assemblers.
- **`forge-pipeline`** (18 tests) is a **linear** stage list, not a graph: `StageType::Branch` and `StageType::Merge` are declared and never constructed, there is no topological sort, and the first `?` aborts the whole pipeline with no partial output and no dead-letter. Its `FilterStage` and `SortStage` are the shapes a budget-aware pack assembler wants.
- **`plato-forge-bridge`** (20 tests) has a `Tile` that is *already a question*: `{domain, question, answer, source, confidence, tags, provenance, metadata}`. It maps a tile to a `ForgeTick`, lets agents mutate it in flight while appending to `agent_path`, and routes the result to rooms by `domain`. **It has a dead-letter buffer** — the only failure-handling mechanism in the whole family that is not a bare abort. It does **not** depend on `forge-flux` and its `Tile` is unrelated to forge-flux's.

Three traps: `forge-pipeline`'s crates.io name belongs to a different project (`marcusdunn/openapi-forge`); `plato-forge-bridge`'s `mod tests;` is not `#[cfg(test)] mod tests`, so its test code ships in the release build; and **none of the three hashes anything** — tile identity is a random `Uuid::new_v4()` per run, so the same input cannot produce the same tile set.

---

## 4. The four questions

### 4.1 Can the Hodge split predict when escalation is worth it?

**No, and the reason is structural.** §5 has the numbers; the short version is that the curl is *identically zero* on a fixed option set, and in the one construction where it is not zero it is non-zero everywhere, so it selects nothing.

What *does* predict escalation is the confidence itself, and it does it better than any alternative I tested (§5.6). The shipped threshold (0.5) catches all 7 disagreements in the corpus at the cost of 6 pointless second calls; the best alternative statistic I could construct — the effective number of live options, `exp(entropy)` — catches all 7 at the cost of 5 pointless calls, i.e. **one call in fifteen saved**. That is not worth a new mechanism.

The honest recommendation is the one the repository has already found: `crates/lm-agents/tests/talk_on_thor.md` §3 says the attention gate is 103 of 159 escalations, and "at 0.55 the attention gate stops being escalated". The Hodge machinery buys nothing; the threshold knob buys the same thing for nothing.

**The experiment the brief asked for, restated so it can actually be run.** `talk_on_thor.md` §3 does **not** log both models' choices per record — it logs aggregate bands (first model's confidence 0.31–0.50 at 0.39 median; second model's 0.11–0.69 at 0.40 median; 159 escalations, 158 taken, 1 timed out). A per-record log does not exist. The proposal therefore has two parts:

1. **Log the pairs.** Extend the `Escalation` record in `crates/lm-agents/src/sysone.rs` to carry the *second* model's option vector alongside the first's, and write one line per escalation into the round log. The field is already there in spirit: `Escalation { gate, primary_confidence, second_confidence, latency_ms, outcome, refusal }`.
2. **Then the experiment.** For each logged pair, compute (a) the candidate-only curl — which will be 0, which is the point; (b) the bipartite curl; (c) the confidence; (d) whether the two models' choices differed. Fit the decision "escalate" against (c) and against (b) and compare. The prediction from §5.6 is that (c) wins and (b) does not.

### 4.2 Does `abstraction-planes` give a rule for how far to decompose?

**Yes, and it is the best answer in the family.** The rule, lifted out of its LLM-judgement wrapper:

> Descend one level at a time. If the level you just reached is **worse** than the previous one by more than 1.0, stop and go back. If it is **better by less than 0.5**, stop. Otherwise keep descending, down to the floor set by the target.

For questions-as-experience §2 this becomes: a question's decomposition has a **floor set by the decision** (the move gate's floor is the legal menu; the attention gate's floor is the roster) and a **marginal-gain stop**. The one thing to change: last-mile cannot afford a large model to score each level, so the quality score must come from the local model the question is aimed at — the d1-2B's own confidence on each candidate decomposition. That is cheap (11 ms per option, and the decomposition tree's leaves are the same options the question already had) and it is exactly the quantity §5 shows is informative.

**The honest limitation**: `abstraction-planes` compares levels using a *model's* score, so its stopping rule inherits the model's non-determinism and its cost. A local version would compare the d1-2B's calibrated confidence on the coarse question against the fine question's — which is measurable, and which §5.6 says is the one number that matters.

### 4.3 Does `persona-engine`'s decompose/dial/compose map onto the sandbox's personality patches and the Vox Score marks?

**Yes, and it maps better than the Vox Score note currently claims.** The sandbox design says an archetype is "a patch over three surfaces at once: the gate weights, the Vox Score marks, and the barks"; the Vox Score's L2.5 marks are "a small vocabulary of named marks, each a closed question about the sound".

`persona-engine` supplies the missing middle layer. A `Persona` is **five named sub-vectors** (cadence, prosody, lexical, groove, embedding) plus a confidence; a `CharacterParameters` is a **set of named dials over those vectors**. So:

| persona-engine | sandbox | Vox Score |
|---|---|---|
| `Persona` = decomposed named vectors | the patch's *shape* | the mark vocabulary |
| `CharacterParameters` = the dials | the patch's *weights* | L2 dials |
| `cmd_dial` (the vibe-coding CLI) | the sandbox's hot-reloaded persona layer | — |
| `compose` → SSML → piper | the bark / renderer | §14's conform-any-voice |
| `confidence` per persona | the archetype's confidence | marks' calibration |

The useful part is the **separation**. persona-engine never conflates "what this voice is" with "how this voice is rendered": the decomposition is a set of named axes, the dials are a named subset of them, and composing is a third, separate step. The sandbox's archetype currently touches gate weights, marks and barks in one patch — which is exactly the conflation persona-engine avoids. The **learn**: make the sandbox archetype a *layered* patch — an axis layer (marks), a weight layer (gate weights), a voice layer (barks) — so that a patch can touch one without touching the others.

The **port**: the `CharacterParameters` field list is a ready-made dial vocabulary — **18 numeric dials** (4 voice, 4 cadence, 4 groove, 6 "personality proxies"), each with a stated range, plus 4 art dials — and `smp_seed` gives a persona a deterministic identity, which the sandbox's hot-reload needs.

### 4.4 Does `cot-quilt`'s pattern fit "the cloud writes questions, the local model answers"?

**Yes, and it is the closest thing in the family to a working instance of the pattern.** The mapping:

| questions-as-experience §3-§4 | cot-quilt |
|---|---|
| The cloud compiles understanding into questions | SEEDS → COT → SPLIT: the large model decomposes, then the judge writes the gaps |
| The local models execute them | The cells: reactive units recomputed on change, not a monolith |
| §4.1 "find where the questions are missing" | The surprise signal's cousins: unsure / refused / escalated, per situation |
| §4.2 "propose a question" | `judge.gaps` → the next iteration's question cells |
| §4.3 "seal a prediction and test it" | The judge, on held-out cells, with a verdict |
| §4.4 "keep the question, with its record" | `seeds/ledger.jsonl` — one receipt per call, `model_requested` vs `model_served`, `request_sha256` |
| §5.4 the question ledger | `exports/coverage-table.md` — per cell: weight, stability class, verdict, evidence pointer |

Three things it has that last-mile does not, and should copy:

1. **The large model judges its own decomposition** and the judged gaps *are* the next questions. This is a closed loop the cloud can run between rounds without a human.
2. **A receipt per call**, including `model_requested` vs `model_served` (served-vs-requested drift is recorded, not assumed) and `request_sha256`.
3. **A coverage table with a verdict per cell** that is allowed to say UNCOVERED.

And one thing last-mile has that cot-quilt does not, which is why the pattern fits and is not merely analogous: **cot-quilt has no local model answering anything at scale.** Its cells are recomputed by a pipeline; the pattern in questions-as-experience is specifically that the *local* model answers a million times for almost nothing, and the cloud writes the question once. cot-quilt writes the *cells*, not the *questions that are asked*.

**The licensing problem is real**: cot-quilt has **no LICENSE file**. Nothing in it can be reused as code without Casey's explicit OK; the *pattern* can be learned, and the ledger shape re-implemented.

---

## 5. The Hodge detector, computed on real answers

### 5.1 Two corrections to the brief's premises

**The brief says `talk_on_thor.md` §3 logs both models' choices.** It does not. §3 logs aggregates: 159 escalations, 158 taken, 1 timed out; the gate breakdown (attention 103 / subject 51 / intention 3 / move 2); the first model's confidence on escalated gates 0.31–0.50 (0.39 median); the second model's 0.11–0.69 (0.40 median); the added latency 593 ms median, 1300 ms p95; and the rate (9.6% of gates answered, 16.1% of asks, 7.1% of decisions, against a 4% target). There is no per-record log with both models' option vectors anywhere in the repository.

**The brief says "the attention gate's logged answers".** No such log exists either. The only logged Thor gate answers in the repository are `docs/research/poc/thor/e4-d1-3B-log.json` (160 scenes) and its `jev` sibling, and they cover the E4 poc gates (`action`, `beacon_bearing`, `enemy_visible`, `beacon_reachable`), not `attention`.

So I captured the real thing. `capture_attention.py` posts the **shipped default-harness `attention` gate verbatim** (from `assets/harness/default/gates.toml`) to the Thor's System One endpoint, with states written in the wording `crates/lm-agents/src/facts.rs` produces. 3 rosters × 5 situations × 2 models × 2 repeats = **60 answers, all successful**, saved in [`attention_capture.json`](decompose-family/attention_capture.json).

One fact fell out of the capture and is worth recording on its own: **the Thor's answers are bit-identical on repeat.** All 30 repeat pairs matched exactly — same probabilities, same confidence. This confirms what `talk_on_thor.md` §3 already says ("the Thor's model answers the same pack the same way every time"), and it is the foundation of §6's and §7's bit-exact replay requirement: a question's answer is a pure function of (gate wording, facts, model), which is exactly what makes content-addressed provenance possible.

### 5.2 The construction, and why the candidate-only curl is always zero

A gate asks one question over N options; a model answers with a probability per option. The natural flow on the graph whose vertices are the options puts, for each observation *t* with option set *Sₜ* and probabilities *pₜ*:

```
w_ij  +=  pₜ(i) − pₜ(j)        for i, j ∈ Sₜ
```

and leaves pairs that never co-occur at zero. That flow is then split by the classical Hodge decomposition on the graph: **E = im(Bᵀ)** (gradient) and **C = ker(B)** (cycle space) are orthogonal in the standard inner product and span ℝ^E, so `ω_g = Bᵀ(BBᵀ)⁺Bω`, `ω_c = P_ker(B)ω`, `ω_h = ω − ω_g − ω_c` (exactly 0 on a graph).

Now the fact that decides the addendum's question:

> **A single probability vector *is* a potential.** `w_ij = p(i) − p(j)` is exactly the gradient of `p`, so its flow lies in `im(Bᵀ)` and its curl is zero — not approximately, **identically**. And a *sum* of gradients over one vertex set is still a gradient.

Therefore, for any set of answers that share an option set — which every gate with fixed options does, the attention gate included — the curl is exactly zero, no matter how the models disagree. `hodge.py` verifies this against synthetic cases with known answers (`3-cycle -> curl > 0` passes; `reversed orderings, SAME option set -> curl == 0` passes at 0.0e+00) and the measured data agrees to 33 decimal places.

**This is not an artefact of my metric.** It holds in any positive-definite metric, because a gradient is a gradient.

### 5.3 The numbers

From [`results.txt`](decompose-family/results.txt):

| corpus | n | curl energy | h0 | h1 | h2 | max triangle circulation |
|---|---|---|---|---|---|---|
| `attention`, roster r8 (10 options), both models | 20 | 3.44e-30 | 1.000000 | 0.000000 | 0.000000 | 0.000000 |
| `attention`, roster r8, d1-2B only | 10 | 5.28e-31 | 1.000000 | 0.000000 | 0.000000 | 0.000000 |
| `attention`, roster r8, one situation, both models | 4 | 5.49e-32 | 1.000000 | 0.000000 | 0.000000 | 0.000000 |
| **all 60 single answers, one at a time** | 60 | **8.20e-33** | — | — | — | 0.000000 |
| `attention`, **all three rosters mixed** | 60 | 1.11e+02 | 0.800831 | **0.199169** | 0.000000 | **9.307372** |
| `attention`, rosters r8+r4 | 40 | 3.25e+01 | 0.896950 | 0.103050 | 0.000000 | 4.461091 |
| repo's own log, gate `action` (E4 poc) | 160 | 3.46e-28 | 1.000000 | 0.000000 | 0.000000 | 0.000000 |
| repo's own log, gate `beacon_bearing` (E4 poc) | 160 | 3.11e-28 | 1.000000 | 0.000000 | 0.000000 | 0.000000 |

Read the third-from-last row carefully. **The curl only becomes non-zero when the option set varies** — when the same gate is asked under different rosters, so that some observations compare options the others never saw. Then the flows live on different subgraphs, the sum is no longer a gradient, and a real circulation appears. That is the one mechanism in this whole family that produces a non-trivial Hodge signal for a *question*.

The last two rows are the repository's own logged answers, and they behave the same way: h0 = 1.000000, h1 = 0.000000, and every triangle's circulation exactly 0.000000 across 160 scenes. This is not a property of my capture; it is a property of option probabilities.

**The harmonic part is 0 everywhere, and that is not a bug.** On a graph, `im(Bᵀ) ⊕ ker(B) = ℝ^E`, so the harmonic space is empty. HodgeRank's H2 exists only in its bipartite agent/candidate form. The operational form of "the evidence is disconnected" on a graph is the **number of connected components of the option graph** — which is 1 in every corpus above, because the attention gate's options always co-occur.

### 5.4 The construction that does work, and what it measures

Putting the **models** on one side and the **options** on the other — HodgeRank's heterogeneous form — the curl is non-zero, and it means something precise.

Build the bipartite agent/candidate graph; the edge (agent, candidate) carries the agent's score for that candidate, centred on the agent's mean, and the score is the **rank** of the candidate under that agent (best = high) rather than the raw probability. The cycle space then contains the 4-cycles `agent₁ – c₁ – agent₂ – c₂ – agent₁`, and the circulation around one is

```
(s₁(c₁) − s₁(c₂)) − (s₂(c₁) − s₂(c₂))
```

which is zero exactly when the two agents **order** c₁ and c₂ the same way.

Measured on the 15 situations of the corpus:

| roster | situation | h1 (curl) | h2 | same **choice**? | same **order**? |
|---|---|---|---|---|---|
| r8 | contested | 0.103030 | 0 | **NO** | NO |
| r8 | holding_beacon | 0.127273 | 0 | **NO** | NO |
| r8 | last_standing | 0.036364 | 0 | **NO** | NO |
| r8 | retreat_danger | 0.157576 | 0 | yes | NO |
| r8 | scout_alone | 0.236364 | 0 | **NO** | NO |
| r4 | contested | 0.085714 | 0 | **NO** | NO |
| r4 | holding_beacon | 0.257143 | 0 | yes | NO |
| r4 | last_standing | 0.314286 | 0 | yes | NO |
| r4 | retreat_danger | 0.400000 | 0 | yes | NO |
| r4 | scout_alone | 0.428571 | 0 | yes | NO |
| r2 | contested | 0.200000 | 0 | **NO** | NO |
| r2 | holding_beacon | 0.100000 | 0 | yes | NO |
| r2 | last_standing | 0.400000 | 0 | yes | NO |
| r2 | retreat_danger | 0.700000 | 0 | **NO** | NO |
| r2 | scout_alone | 0.300000 | 0 | yes | NO |

**The two models' full orderings differ in 15 of 15 situations.** They agree on the winner in 8 of 15 and differ in 7. So the curl is non-zero everywhere — it says "these two models are not on one scale", which is always true, and therefore selects nothing.

### 5.5 The addendum's test: does high curl pick out the questions where decomposition beats escalation?

**No.** From [`curl_vs_escalation.txt`](decompose-family/curl_vs_escalation.txt):

```
curl (h1)     AUC for 'the models differ' = 0.250   (0.5 = a coin, 1.0 = perfect)
confidence     AUC for 'the models differ' = 0.714   (oriented: low confidence predicts a difference)
```

The curl is **worse than a coin** at predicting that the two models will choose differently. The confidence is good at it. The overlap is total — the highest curl in the corpus (0.700) is a disagreement, but 0.400 and 0.428 are agreements, and the lowest (0.036) is also a disagreement.

Three separate reasons, each sufficient:

1. **On a fixed option set the curl is identically zero**, so there are no "high-curl questions" to select. The addendum's premise cannot arise on the attention gate as it is shipped.
2. **In the bipartite form the curl measures ordering agreement, not choice agreement.** The two models disagree about the order in 15 of 15 situations, so the curl is non-zero everywhere and separates nothing.
3. **What escalation actually needs to know** is "will the two models pick different options", and the curl cannot see it (AUC 0.250) while the confidence can (AUC 0.714).

There is also a fourth, practical reason: **the shipped `hodge-consensus` could not run on this gate anyway.** Its DFS enumerates simple cycles with `max_depth = 8`, and the attention gate's dense candidate graph is K₁₀, which has 556,014 simple cycles in total, 172,974 of them within the depth cap. Each is then projected against 90 edges — about 15.6 million inner operations and 172,974 cycle vectors, before the DFS's own path enumeration (Σ P(9,k) for k ≤ 8 ≈ 6.2 million paths per start node). `hodge.py` computes the counts; `hodge-consensus` would have to be rewritten with a sparse construction to be usable at all.

### 5.6 Is the confidence threshold the right escalation trigger?

I swept every one-call statistic that could be computed **before** spending the second call, against the label "the two models chose different options" (from [`escalation_results.txt`](decompose-family/escalation_results.txt)):

| statistic | AUC | calls needed to catch all 7 | wasted | missed |
|---|---|---|---|---|
| **confidence** (max share) | 0.714 | 13 of 15 | 6 | 0 |
| margin (top two gap) | 0.714 | 14 of 15 | 7 | 0 |
| normalised entropy | 0.589 | 13 of 15 | 6 | 0 |
| effective options (`exp(H)`) | 0.714 | 12 of 15 | 5 | 0 |
| top minus mean of the rest | 0.375 | 13 of 15 | 6 | 0 |

The shipped rule (`confidence < 0.5`) catches all 7 disagreements at the cost of 6 pointless second calls. The best alternative saves **one call in fifteen**. That is not worth a new mechanism, and it says something true about the situation: the escalation cost is not statistical, it is structural — the d1-2B is genuinely unsure on the attention gate (26 of 30 of its answers fall below 0.5), so the rule fires almost always.

**So the answer to the addendum's question is: the Hodge decomposition tells us nothing the escalation policy does not already tell us, and the escalation policy is already near the best a one-call trigger can be.** What would actually help is a different lever — a finer question, not a better classifier — which is exactly questions-as-experience §5.1's experiment B.

### 5.7 What the decomposition *is* good for

Three things, none of them the trichotomy:

1. **As a coherence score for the question library.** `spectral-prosody`'s Fiedler value is the algebraic connectivity of a graph; the same quantity over a graph whose vertices are questions and whose edges are co-occurrence would say whether the library is one coherent set or several disconnected ones. That is the library-level question, not the answer-level one.
2. **As the detector for "this gate mixes kinds of choice" — but only when the option set varies.** §5.3's mixed-roster row is the real signal: h1 = 0.199 with a maximum triangle circulation of 9.31 on the same gate asked under three rosters. A gate whose option set changes between situations is a gate that mixes kinds, and the decomposition finds it. The `move` gate, whose options are the legal menu at the moment of the question (`options_from = "menu"` in `assets/harness/default/gates.toml`), is exactly such a gate. **The attention gate is not, and the move gate is.**
3. **As the honest label for what a disagreement is.** The bipartite curl at least distinguishes "these two models disagree" from "these two models are on one scale" — a distinction the confidence threshold cannot make and the escalation policy does not need.

---

## 6. The question ledger, in rea's shape

**What rea actually does** (`src/domain/evidence.ts`, `src/domain/residualUnknown.ts`, read via `gh api`; MIT):

- **Evidence is content-addressed** — `ev_<sha256>` — and typed: a `subject` (the artifact it is about), a `provider` (id, name, version), an `analysis_profile`, a `predicate_type`, an `operation`, `parameters`, a `raw_result` and a `normalized_result`.
- **Confidence is a type, not a number**: `observed | derived | inferred`.
- **Authority is a type**: `shipped-artifact | controlled-replay | external-service | analyst-inference`.
- **Limitations are mandatory** — an Evidence record without limitations is invalid.
- **Evidence forms a DAG** through `evidence_links` — each record points at the records that contributed to it.
- **Residual unknowns are a separate ledger** with a stable identity (`unk_<digest>` over domain, question and scope), a **revision chain** (`revision`, `previous_revision_digest`, `revision_digest`), a status (`open | investigating | blocked | contradicted | resolved`), a severity, `supporting_evidence_ids` **and** `contradicting_evidence_ids`, the `required_authority` / `required_confidence` / `required_environment` needed to close it, `recommended_probes`, relationships to other unknowns, and a resolution with a rationale.

**The proposal**, in that shape, for questions-as-experience §5.4's question ledger:

```
Question {
  id              : q_<sha256(canonical(question_text, gate_kind, options))>   // content-addressed
  text            : the question as asked
  kind            : choice | noul | score
  options         : [{ key, criteria }]            // the menu or the fixed criteria
  authored_by     : { who, when }                  // a seat, a person, Bench, the judge
  origin          : { parent: q_<id> | null, kind: new | split | reword }
  situation_scope : [situation_key]                // the surprise signal's keys
  evidence        : [ev_<id>]                      // the DAG edges
  limitations     : [string]                       // MANDATORY, non-empty
  calibration     : { per model: { model: { n, mean_confidence, accuracy,
                                            brier, reliable_bins } } }
  cost            : { options, ms_estimate, tokens }
  lifecycle       : { created, adopted, retired, last_used }
  residual_unknowns : [unk_<id>]                   // what we do not know about this question
}
```

**How the four evidence-authority classes map onto last-mile's evidence:**

| rea authority | last-mile's equivalent | where it comes from |
|---|---|---|
| `shipped-artifact` | the gate as it runs in a round | the round log, the harness commit (D27) |
| `controlled-replay` | a **sealed prediction** replayed on held-out seeds with the sign test (16 seeds, D-pending) | the deterministic sim + the seed farm |
| `external-service` | a **cloud-inferred** question: the cloud director's or judge's reading of the situation | the cloud seat's receipt |
| `analyst-inference` | a human or seat's reading that nobody has tested | the analyst, and it is labelled as such |

`controlled-replay` is the load-bearing one and it is the one last-mile can do best, because the sim is deterministic: the same seed, the same question and the same model give the same answer — which §5.1 measured directly on the Thor.

**Where the decompose repos already have something like this:**

- **cot-quilt's judged decomposition** is a *table*, not a DAG: `exports/coverage-table.md` gives each cell a weight, a stability class (core / stable / volatile / critique) and a verdict (COVERED / PARTIAL / UNCOVERED / DOCTRINE) with an evidence pointer. That is the **index** a ledger needs, and cot-quilt's `seeds/ledger.jsonl` is the **rows**: one receipt per call with `model_requested`, `model_served`, `request_sha256`, `cost_usd_declared`, `cost_basis`. What cot-quilt does not have is typed confidence, typed authority, or mandatory limitations — and it cannot have them, because it has no LICENSE to copy from and no content-addressed identity (its ids are per-run).
- **quilt-runbook's WHY-records** are the other end: `§5a` adjustments carry `{trigger, hypothesis, evidence[], generalizes}` and the miner compiles recurring ones into cells. **This is the mechanism by which a question earns its place** — it is questions-as-experience §4.3's "seal a prediction and test it" with the test written down. The gap is that `quilt-runbook` has a hash chain but no *typing*: its steps are `attempt | observe | stablepoint | adjust | note | error | resumed-from`, and nothing in the record says whether a claim was observed, derived or inferred, or on whose authority.

**The synthesis worth making:** rea gives you the **types**, quilt-runbook gives you the **chain**. A question ledger needs both, and neither repo has both. Port rea's `Evidence` and `ResidualUnknown` shapes; port quilt-runbook's `id = sha256(run_id|seq|prev|body)` chain and its fail-closed writer; and drop in cot-quilt's coverage table as the ledger's index and quilt-runbook's `§5a` record as the ledger's "why did this question change" row.

**The residual-unknown ledger is not a nice-to-have here.** The honest entries for the attention gate are already known: *we do not know whether the second model's answer is better than the first's, because we have no neutral judge on this gate* (d1-3B is the proposed one, per questions-as-experience §5.1), and *we do not know whether the 10 options are the right granularity*, which is exactly §5.1's experiment B. Those are `unk_` records with `required_authority: controlled-replay`, not prose.

---

## 7. A situation-retrieved question library

### 7.1 The tile shape

`plato-forge-bridge`'s `Tile` is already a question, and `plato-tiling`'s `Tile` is already a tile. Take the bridge's shape and the tiling's metadata:

```
QuestionTile {
  id            : qt_<sha256(canonical(text, kind, options, situation_scope))>
  text          : the question, as the model will read it
  kind          : choice | noul | score
  options       : [{ key, criteria }]          // the fixed criteria, or a menu recipe
  situation_keys: [u64]                        // the surprise signal's keys (§7.2)
  confidence    : f32                          // the model's own confidence in the question
  granularity   : u8                           // depth in the decomposition tree
  parent_id     : qt_<id> | null               // if it came from splitting a coarser tile
  children      : [qt_<id>]                   // if it was split
  cost          : { options, ms_estimate }     // 11 ms per option (D70)
  state         : TileState                    // alive | fading | ghost | haunting | resurrected | afterlife | expired
  weight        : f32                          // the ghost weight (§7.3)
  evidence      : [ev_<id>]                    // the §6 ledger
  limitations   : [string]                     // mandatory
  last_used     : { round, tick, situation }
  created_by    : { who, when, from: cloud | judge | human | split }
}
```

`plato-tiling` contributes the classification that matters most: it has an explicit `TileType.QUESTION` and classifies by `text.stripped.endswith("?")` — a cheap, deterministic type test for a question tile, exactly the kind of thing the library needs to sort itself.

`plato-tile-split`'s `SubTile{content, parent_id, index, confidence, tags, metadata}` is the split-tree schema, and its `TileSplitter.by_sentence / by_paragraph / by_word / by_markdown / by_size` is the reusable splitting rule — with the caveat, from the skim, that its Python half has **zero tests**.

### 7.2 The deterministic retrieval rule

The requirement is that **the pack chosen is logged and replays bit-exact**. Two facts make this possible: the Thor's answers are a pure function of (gate wording, facts, model) — measured, §5.1 — and the sim is deterministic and fixed-point (D10).

The rule, in one line:

> **For a situation key K and a budget B, the pack P(K, B) is the B-cheapest set of tiles whose situation_scope contains K, ordered by (co-occurrence weight with K's situation, then by tile id), with ghost tiles excluded and resurrected tiles restored first.**

Concretely:

```
retrieve(K, B):
    candidates = tiles where K ∈ situation_scope
                and state ∈ {alive, resurrected}          # ghosts are out until they resurrect
    # order by how strongly the tile's situation matches K, then by id for a total order
    candidates.sort_by( |situation_keys ∩ window(K)| desc, cost asc, id asc )
    pack = []
    spent = 0
    for t in candidates:
        if spent + t.cost.options > B: continue          # skip, don't break: a smaller tile may still fit
        pack.append(t)
        spent += t.cost.options
    log(pack_ids, K, B, spent)                            # the pack is logged, always
    return pack
```

The ordering key is the **Fiedler-style co-occurrence weight** from `spectral-prosody`: build the graph whose vertices are tiles and whose edges are shared situation keys, weight them by co-occurrence, and use the neighbourhood of K as the relevance signal. This is `spectral-prosody`'s Laplacian machinery applied to the library rather than to poetry, and it is the one place in the family where a Hodge/Fiedler computation earns its keep (§5.7).

**Why it replays bit-exact.** The pack depends only on (the tile table, K, B) — all of which are logged — and never on the model's answers, on wall-clock time, or on iteration order of a hash map. The `id` is a content hash, so the sort has a total order. The only time-dependent field is `last_used`, and it is written *after* the pack is chosen, so it cannot affect the choice within a round.

### 7.3 The lifecycle, as one state machine

The numbers are not invented; they are `plato-afterlife`'s and `plato-ghostable`'s, and they are stated in the source.

```
                         ┌──────────────────────────────────────────────┐
                         │                  ALIVE                      │
                         │  weight = 1.0, state = alive                │
                         └───┬──────────────┬───────────────────┬──────┘
              used in a pack │              │ unused for       │ split
                             ▼              │ N periods        ▼
                       ┌──────────┐         │            ┌─────────────┐
                       │ FADING   │─────────┘            │  PARENT    │
                       │ weight>0.05,                 │  (coarse)   │
                       │ state=fading                └─────────────┘
                             │ decays 10% per period         │
                             │ (plato-afterlife: weight *= 1 - decay_rate)
                             ▼
                       ┌──────────┐   boost = max(relevance * 0.1, 0.06)
                       │  GHOST   │◄──────────────────────────────┐
                       │ weight<0.05                          │
                       │ state=ghost, forgotten=true           │
                       └────┬──────────────────────────────────┘
                            │ weight ≥ 0.05 and the situation recurs
                            ▼
                       ┌──────────────┐   decay again
                       │ RESURRECTED  │──────────────────────► GHOST
                       │ state=resurrected, resurrection_count += 1
                       └────┬─────────┘
                            │ the owning agent or round ends
                            ▼
                       ┌──────────────┐
                       │  AFTERLIFE   │  a retired squad's or a dead agent's tile;
                       │ state=afterlife │ kept as a ghost, decayed, adoptable
                       └────┬─────────┘
                            │ never reinforced again
                            ▼
                       ┌──────────┐
                       │ EXPIRED  │  weight < 0.05 after the final decay
                       └──────────┘

  also: HAUNTING — a ghost that is being read by a living agent right now
        (plato-ghostable's seventh state): a read marks it haunting, and a
        read that finds it still relevant promotes it to RESURRECTED.
```

**The decay and boost constants, from `plato-afterlife/src/lib.rs`:**

- new ghost weight **0.1**
- decay: `weight *= 1 − decay_rate`, `decay_rate` default **0.1** (10% per period), applied when the period advances past `last_access_period`
- forget: `weight < 0.05` ⇒ `forgotten = true`
- boost: `boost = max(relevance * 0.1, 0.06)` — the floor of 0.06 is deliberate, "enough to resurrect from 0.05"
- strong: `weight > 0.5 && !forgotten`
- `plato-ghostable`'s variant: `health` 1.0, `ghost_threshold` 0.1, `decay_rate` **0.01**, `haunt_boost`, `references`, `resurrection_count`, `original_confidence`, `importance`

**The two inheritance cases the addendum asks for**, in this state machine:

- **In-match**: a stunned agent's last observations become a **decaying belief ghost** that teammates read as facts. That is a tile whose `situation_scope` is the situation in which the agent was stunned, whose `state` is `alive` while the stun holds and `fading` after it, and whose `evidence` carries the authority `shipped-artifact` (the agent's own observation, in the round log) with a `limitations` entry naming the stun. The teammate's read marks the tile `haunting`; if the teammate acts on it and the outcome is good, the tile is promoted to `resurrected` — and if the outcome is bad, the tile's `evidence` gains a `contradicting` link and it becomes a **residual unknown** in the §6 ledger. Deterministic and hashed: the tile's id is a content hash of the observation, so two nodes that see the same stun compute the same tile id.
- **Across rounds and players**: a retired squad's questions and lexicon persist as ghosts with their own `decay_rate`, and a new squad inherits them by retrieval (§7.2) — a ghost that is retrieved and confirmed is `resurrected`; one that is retrieved and ignored decays further; one that is never retrieved at all eventually reaches `expired`. The lexicon inherits the same way: a retired squad's phrasebook entries are tiles with `kind: noul` or a mark, and the new squad's `use` count is what keeps them alive.

**The evidence hooks, at each transition.** Every transition above writes one row:

| transition | what is recorded | authority |
|---|---|---|
| create | the authoring seat, the parent tile, the source evidence | `analyst-inference` or `external-service` |
| retrieve | the pack id, the situation key, the budget | `shipped-artifact` |
| strengthen | the reading agent, the tick | `shipped-artifact` |
| decay | the period, the weight before and after | `shipped-artifact` |
| ghost | the period it crossed 0.05 | `shipped-artifact` |
| resurrect | the relevance that justified the boost | `shipped-artifact` |
| inherit | the inheriting squad, the new owner | `shipped-artifact` |

And the "why did this change" row is `quilt-runbook`'s `§5a` record verbatim in shape: `{trigger, hypothesis, evidence[], generalizes}` — with `generalizes` deciding whether the change compiles into a new tile or stays a one-off.

### 7.4 What the decomp experiment's results would change

The decomp experiment is questions-as-experience §5.1, arm B: a two-step decomposition of the attention gate on d1-2B alone. Its result lands in three places.

1. **The granularity dial's stop rule.** If arm B is more confident and as accurate, the library stops treating a 10-option gate as one tile and starts storing it as a **tree**: the coarse question (`squad` or `agent`?) as the parent and the fine question (which agent) as the child. The `granularity` field and the `parent_id`/`children` fields become load-bearing rather than decorative, and the split rule in `plato-tile-split` becomes the mechanism.
2. **The budget arithmetic.** At ~11 ms per option, a 10-option question costs ~110 ms and a two-step question costs ~66 ms. If the decomposition holds, the retrieval rule's budget B buys more coverage, and `cost.options` in the tile schema starts doing real work: the pack becomes "the most situations covered per millisecond" rather than "the most situations covered".
3. **What the Hodge signal would be used for.** If decomposition wins, then the interesting question stops being "which of these ten options" and becomes "**is this gate one question or several**" — which is exactly the §5.3 mixed-roster result (the `move` gate, whose option set is the legal menu, has real curl; the `attention` gate, whose option set is fixed, does not). The experiment would tell us whether to **split the attention gate into two gates** (an addressee-kind gate and an addressee-identity gate), which would make it a genuinely mixed-kind question and would give the Hodge decomposition something real to measure.

**And what would not change:** the confidence threshold's job. Even a perfectly decomposed gate still needs a rule for "is this answer sure enough to act on", and the confidence is still the best one-call predictor of model disagreement (AUC 0.714 against the curl's 0.250).

---

## 8. The top 5 ideas to bring in

Ranked by how much they change last-mile per unit of work, with the first concrete step for each.

### 1. The ledger, with rea's types inside quilt-runbook's chain

**What:** a question ledger where every question and every claim about it is content-addressed and typed — confidence as `observed | derived | inferred`, authority as `shipped-artifact | controlled-replay | external-service | analyst-inference`, mandatory limitations, an evidence DAG — and the ledger itself is an append-only hash chain with a fail-closed writer.

**Why first:** questions-as-experience §6 says it takes "a question ledger format in the harness" to make the note a decision. It is also the precondition for everything else in §7: decay, resurrection and inheritance are all bookkeeping over a ledger.

**First step:** port rea's `Evidence` and `ResidualUnknown` shapes into a Rust `lm-learn` type, and write one `Evidence` row per gate answer into the round log — `(gate, model, options, confidence, situation_key, request_hash)`. Do not build the chain yet; get the *types* into the log first, because the types are what the ledger's readers need and the chain is only the seal. License: rea is MIT (port the shape, not the code); quilt-runbook has **no LICENSE file** (learn the discipline, re-implement it).

### 2. The granularity dial's stop rule, from `abstraction-planes`

**What:** descend one decomposition level while the marginal quality gain is above 0.5 of 10, and stop when it drops below; never descend below the floor the target sets.

**Why second:** it is the only rule in the family that answers "how far", and it is five lines of arithmetic wrapped around a model score — which last-mile can compute locally at 11 ms per option instead of paying a cloud call for each level.

**First step:** implement the rule against the **d1-2B's own calibrated confidence** as the quality score, not a cloud model's judgement: for a candidate decomposition, ask the coarse question and the fine question on the Thor, compare the finer question's confidence against the coarser one's, and descend while the gain exceeds a threshold. Log both confidences so the threshold can be tuned on evidence. This is also the cleanest way to run §5.1's arm B.

### 3. The ghost lifecycle, with the numbers already written

**What:** `plato-afterlife`'s ghost weight 0.1, 10% decay per period, forget below 0.05, boost `max(relevance × 0.1, 0.06)`; `plato-ghostable`'s seven-state `TileState` and five `ResurrectionCondition`s; `plato-tiling`'s tile shape with `TileType.QUESTION`; `plato-tile-split`'s `SubTile` with parent linkage.

**Why third:** this is the mechanism questions-as-experience §4.4 ("keep the question, with its record") has been missing. It is also the answer to "what happens to a question that stops being asked" — which is the question a question library inevitably raises and which the current design does not address.

**First step:** add `state` and `weight` to the question tile, implement the decay function and the boost floor in `lm-learn`, and run it over the existing round logs to produce the first decay curve: how long does each shipped gate go unused before it crosses 0.05? The answer is a number the library needs and nobody has measured. License: all MIT; `plato-tiling` has no tests, so **port and test** rather than reuse directly.

### 4. The `§5a` WHY-record, so a question can earn its place

**What:** `quilt-runbook`'s adjustment record — `{target, before, after, why: {trigger, hypothesis, evidence[]}, generalizes}` — plus the miner's rule that only **generalising, recurring** adjustments compile into a new cell (≥ 2 per cluster), and the fail-closed writer that records an invalid payload as an `error` step rather than dropping it.

**Why fourth:** questions-as-experience §4.3 wants a question to "earn its place by moving outcomes", sealed against held-out seeds with the sign test. The `§5a` record is that seal, already shaped, and the L8 rule is the anti-clutter rule a library needs.

**First step:** when the sign test moves a question's adoption, write one `§5a` row — trigger ("the sign test on 16 seeds moved engagement 0/179 → 19/19"), hypothesis, the evidence links, and `generalizes`. Then, once there is a row set, run the miner and see which *recurring* adjustments actually recur: that list is the library's first curriculum.

### 5. Rank-based budgeting and alternate routes, from cot-quilt's JEV doctrine

**What:** two rules, both written down and both tested against last-mile's own stack: budget is allocated by **rank**, never by absolute score (because judged scores cluster at the top of a rubric and are not comparable across questions), and **divergent routes are kept as first-class alternate routes** rather than averaged away.

**Why fifth:** §5.3's experiment (per-question model routing) needs both. Rank-based budgeting is how you route *which model* gets which question when the scores are not comparable — which is exactly the situation with d1-2B (median confidence 0.39 on escalated gates) and clef-flash (0.40, on a different scale). Alternate routes are what you do with a disagreeing second model: do not average a one-lie-among-five into mush, and do not discard it.

**First step:** write the two rules into the harness's policy file as the first two entries of a `question_budget` block, and change the escalation path to record both models' option vectors as an **alternate route** on the answer rather than overwriting the primary — which is also the logging change §4.1's experiment needs. License: cot-quilt has **no LICENSE file**, so this is a learn-and-re-implement, and it needs Casey's OK before any code is copied.

---

## 9. Things that contradict our design

1. **The Hodge family does not agree with itself about curl.** `hodge-consensus` says a curl-dominant dispute **will resolve**; `si-hodge-consensus-demo` says it **will not** ("needs external intervention"); `hodge-consensus-rs`'s README says "partially resolvable". Any last-mile decision that cites "the Hodge consensus" as a position would be citing three positions.
2. **The trichotomy itself is the wrong shape for a single question.** A single probability vector is a potential; its flow is exactly a gradient; the curl is identically zero (proved in §5.2, measured at 1e-33). "Does this question mix kinds of choice?" cannot be answered by the curl of its own answers. It *can* be answered by the curl across answers with **different option sets** — which is the `move` gate's situation and not the `attention` gate's.
3. **The repository most likely to be reached for — `hodge-consensus-rs` — has no code.** Its README documents `decompose_disagreement` and `D[i][j] = x_i − x_j`; neither exists in any `.rs` file. Its CI is green because `cargo test` on an empty crate succeeds.
4. **Two of the three Hodge repos have CI red** (`clippy`/`fmt` on `hodge-consensus`; exit 101 on the demo), and **none of the three has a test that pins a numeric expected decomposition**. The demo's headline "83%" is a hard-coded constant.
5. **The `forge-flux` conservation ratio does not work.** `ForgePipeline::conservation_ratio()` returns `1.0` with the comment "placeholder; real calculation requires running the pipeline"; `Transform::conservation_ratio` is **never called**; the per-tile `cr` field is written by `SummarizeTransform` with a *tile-count* ratio and is compared against a *byte* threshold by `ConservationTransform`. And it is the wrong invariant anyway: a question library's budget is measured in tokens and coverage, not in bytes. The real entropy-based version lives in `forge-conservation`, which is outside the five.
6. **`forge-pipeline` is not a graph.** `Branch` and `Merge` are declared and never constructed; there is no topological sort; the first `?` aborts with no partial output and no dead-letter. If last-mile wants a question pipeline, this is a shape to copy *and* fix.
7. **Nothing in the family hashes anything.** Tile identity is `Uuid::new_v4()` per run across all five forge-family repos, and `HashMap` metadata means the serialized key order varies between processes. A question library needs a stable key for "unused → ghost → resurrect" to be trackable at all; and `forge-meta`'s `HashSet → Vec` collection makes even its own ecosystem report non-reproducible.
8. **The `attention` gate's option set is fixed, so its "thirteen ways to address somebody" (the phrase the escalation section of `talk_on_thor.md` uses; the shipped gate in fact has **ten** options) is one kind of choice wearing ten hats.** The gate's own instructions read: "Who the line is for, in order: (1) If the whole squad has to act on it, your squad. (2) If one agent in particular needs it, that agent by number. (3) Only if the whole field must hear it, everybody." — a three-way *kind* question with an eight-way *name* question inside branch 2. This is the strongest argument for the decomposition experiment, and it is an argument from the gate's own wording, not from Hodge.
9. **Nine repositories' manifests disagree with their own LICENSE files** (forge-audio, forge-code, forge-image, forge-meta, hodge-theory, rusty-series, terminal-hodge-harness, Equipment-CellLogic-Distiller, and the two hodge repos with NOASSERTION). In every case the **LICENSE file is the authority** and it is MIT (or MIT OR Apache-2.0 for the two dual ones). But `Cargo.toml`'s `license` field is what `cargo` reports and what a scanner reads, so `plato-forge-bridge` and `forge-meta` will be reported as Apache-2.0 by some tools and MIT by others.
10. **Two repositories have no LICENSE at all** — `cot-quilt` and `quilt-runbook` — and they are the two with the most interesting designs. Nothing in either can be reused as code without Casey's explicit OK. This is worth fixing in the source repos, because it is the two most likely to be borrowed from.
11. **`plato-tiling`'s description promises "create / search / ghost / resurrect"; the file does not deliver the last two.** `src/plato_tiling/tiling.py` is adaptive chunking with a `TileType` enum; the ghost and resurrect machinery is in `plato-afterlife` and `plato-ghostable` instead. Similarly `plato-tile-split`'s README is three lines and its Python half has no tests.
12. **`plato-ghostable`'s `pushed_at` (2026-10-18) is a different branch, not `main`.** `main` was last touched 2026-05-08. Do not read the October timestamp as recent work.

---

## 10. What is not settled

- **Whether decomposition beats escalation on the attention gate.** Nothing in this family answers it; only §5.1's arm B experiment can, and it has not been run. The Hodge analysis says the question is not a *detector* problem but a *design* problem: the gate is one kind of choice wearing ten hats.
- **Whether the escalation threshold should move.** The evidence here says the threshold's cost is structural (the d1-2B is unsure on this gate), and that the repo's own 0.55 suggestion is the cheapest lever. Whether to route the attention gate to a different model outright (§5.3) is untested.
- **The calibration numbers.** §6's `calibration` block wants per-model reliability bins. The corpus here has 30 answers per model with no ground truth, so nothing can be said about calibration yet; the neutral judge (d1-3B) that §5.1 proposes is the missing piece.
- **What the cloud's judge is for.** cot-quilt's large-model-judges-its-own-decomposition loop is the most promising mechanism found for §4's "find where the questions are missing", but it was run with a different toolchain and its verdicts are on a 0–10 rubric with the clustering problem the doctrine warns about.
- **The residual-unknown ledger's first entries.** They are obvious (no neutral judge on `attention`; the right granularity unknown) but they are not yet written, and §6 argues they should be `unk_` records with `required_authority: controlled-replay` rather than prose.
- **The exact question-tile identity function.** §7.1 proposes `sha256(canonical(text, kind, options, situation_scope))`. The canonical form has to be pinned down, because the whole ledger and the whole ghost lifecycle hang off it — and because `quilt-runbook`'s `canonicalJSON` is the obvious candidate, but it has no LICENSE.

---

*No other file in this repository was edited. Nothing was committed. The `decompose-family/` directory holds only new analysis artifacts — the capture, the decomposition tool, its self-tests and their outputs — so that every number above can be re-derived rather than taken on trust; delete it if it is not wanted.*
