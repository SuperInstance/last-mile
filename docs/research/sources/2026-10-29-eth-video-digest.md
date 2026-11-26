# Digest — "AI Just Made a Discovery That Changes EVERYTHING" (Dr. Know-it-all, 26:48)

Marks: **[V]** = said in the video; **[X]** = inferred/corrected by me from the primary source the video cites (not spoken).

## 1. What discovery / paper is discussed

Two threads, announced the same day, **26 Oct 2026**.

- **CS:** the **Exponential Time Hypothesis (ETH) is false** — 3-SAT, and the harder counting problem #3-SAT, get a *deterministic* subexponential-time algorithm. The video quotes OpenAI's headline "The Exponential Time Hypothesis is false" **[V]**; the post is *"Working with academic communities…"*; Anthropic's paper, credited to "Claude (Anthropic)", is ***A 2^o(n) Time Algorithm for 3-SAT*** (Lean 4 formalization in `anthropics/formal-math`) **[X]**.
- **Physics (OpenAI only):** a **rigorous existence proof** of interacting **4D quantum field theories** with gauge fields and fermions, including **QCD** and a **unified completion of the Standard Model**, built on **Balaban's approach** in constructive QFT; **pure Yang–Mills existence** is a *subproblem* of the Clay Millennium Prize problem **[V]**.

Names: ETH's proposers **Russell Impagliazzo, Ramamohan Paturi, Francis Zane** (1999/2001) **[V: "Impalazano Ptorii and Francis Zayn"]**; review group **Impagliazzo, Paturi (UC San Diego), Ryan Williams (MIT)** **[V: "Russell and Pagnyazo, Rabamoen Patoui, and Ryan Williams"]**; **David Gross** (Nobel 2004) leads the physics shepherds, with Douglas, Jaffe, Klebanov, Nekrasov **[X, unsaid]**. "Lean" = the **Lean 4 theorem prover** with Mathlib **[V]**.

Dates **[V]**: ETH discoveries **16 Oct 2026** (both labs); OpenAI's ETH Lean formalization Oct 17; Yang–Mills proof Sept 9, QCD proof Sept 18, Yang–Mills formalization Oct 16. The **8 Sept 2026 Navier–Stokes** credit fight is cited as why OpenAI now defers to outside scholars.

## 2. Core idea, plainly

ETH said no algorithm solves 3-SAT faster than exponentially in the variable count. Two labs' AI systems claim a *deterministic* algorithm beating every fixed exponential — counting *all* satisfying assignments, strictly harder than deciding satisfiability. Then ETH is false, and hundreds of papers using it for conditional hardness lose their foundation. It does **not** settle P vs NP. OpenAI's models also produced existence proofs for QCD, a Standard Model completion and Yang–Mills, checked in Lean. The speaker frames this as AI moving from known-answer puzzles to unknown mathematics, and now into mathematical physics — a possible new golden age.

## 3. The math / method, precisely

**Computed:** the **exact count of satisfying assignments** of a Boolean formula in conjunctive normal form (k-CNF: ≤ k literals per clause, no repeated clause; n variables, m clauses). SAT is decided by "count > 0" **[V]**.

**Representation:** canonically encoded binary formula, **L = encoding's bit length** **[V]**; Anthropic's Lean statement formalizes it on Mathlib's `Turing.FinTM2`, a *deterministic* stack machine **[X]**.

**Bounds**
- OpenAI as spoken: ≈ **2^(O(n log log n / (log n)²))** — subexponential, still vastly superpolynomial **[V]**. Released forms **[X]**: time (n+m)^(O_k(1))·2^(n/(log n)^(1/2+o(1))); Lean scope statement (L+1)^(O_k(1))·2^(O_k(n(log log n)²/√(log n))).
- Anthropic **[X]**: #3-SAT in **2^(O(n log log n / √log n))**; Lean `MainTheorem`: one machine, constant C, 2^(C·min(km/√(log m), k²n log log n/√(log n))) steps.
- The video's "(L+1)^(polynomial) × subexponential" bound **[V]** matches OpenAI's repo scope doc, not Anthropic's headline bound — likely an attribution swap **[inferred]**.
- ETH's formal form **[X]**: s₃ > 0 — some δ>0 with no deterministic 3-SAT algorithm in O(2^(δn)); "ETH false" ⇔ s₃ = 0.
- Consequences claimed: ETH false, hence **#ETH** and **SETH** (SETH ⇒ ETH); conditional hardness results in graph theory, computational biology, databases and geometry need re-auditing **[V for ETH/SETH; rest X]**.
- **Physics claim [V]:** *existence only* — the existence half of the Millennium problem; the **mass gap is not solved**, no new particles or forces claimed; gluons supply ~96% of nuclear mass, quarks ~4%.

## 4. Why "changes everything" — and caveats

**Why:** a 25-year-old assumption overturned the same day by two labs; AI moving from contests → frontier math → mathematical physics; parallel AI agents plus fast formal verification, with humans on interpretation and experiment — "millions of artificial minds" added to science, likened to 1905–1928 (Einstein, de Broglie, Heisenberg, Schrödinger, Dirac), which later yielded cryptography, radio, semiconductors, lasers, GPS. Anthropic: **14M output tokens, no human input** **[V]**.

**Caveats:** ETH false ≠ P = NP (P vs NP is open; P = NP would be catastrophic). Subexponential ≠ polynomial — too expensive for realistic sizes, "no immediate technology follows". Physics results are existence proofs, not full Millennium solutions (mass gap outstanding). Everything "remains a claim awaiting independent scrutiny"; Lean checks the formal statement, not whether it matches intent. OpenAI claims neither credit nor the $1M prizes.

## 5. Concrete technical terms

**Complexity:** Exponential Time Hypothesis (ETH — not Ethereum); Strong ETH (SETH); P vs NP (P = polynomial time; NP = *nondeterministic* polynomial time — the video says "non-polynomial"); NP-complete; SAT; 3-SAT; k-SAT; #3-SAT/#k-SAT; deterministic algorithm; exponential vs subexponential (2^o(n)) vs polynomial time; conditional hardness; ETH/SETH/#ETH; s₃; sparse parity checks; polynomial interpolation; tensor networks (**[X, unsaid]**).
**Verification:** Lean 4; Mathlib; Turing machine (FinTM2); natural-language proof.
**Physics:** QCD; pure Yang–Mills theory; Quantum Yang–Mills Theory (Clay Millennium Prize, $1M); mass gap; Standard Model; grand unified theory; gauge fields; fermions; Balaban's constructive QFT; asymptotic freedom; confinement; color charge (RGB); gluons; quarks; strong force; 4D spacetime.
**Models/orgs:** OpenAI internal model (video: "ChatGPT"); Anthropic internal Claude research model; Impagliazzo & Paturi (UC San Diego); Ryan Williams (MIT); David Gross; Clay Mathematics Institute; Navier–Stokes.
**Also invoked:** number theory; public-key cryptography; electromagnetism; radio; quantum mechanics; semiconductors; lasers; relativity; GPS. **No datasets named.**
