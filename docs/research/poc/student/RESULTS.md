The brief is carried out. `sim.py` is one self-contained 493-line stdlib file that runs in ~16s, deterministically (two runs byte-identical).

**What the experiment says**

The teacher is the exact oracle (HIDE if any enemy has Bresenham LoS within 6; else ADVANCE if the BFS path to the beacon is ≤10; else SCOUT) with 5% of its training labels flipped. Held-out accuracy against the *noiseless* truth, on 10,000 fresh eval worlds from a seed never trained on:

| student | n=50 | n=200 | n=1000 | n=5000 | cost/decision |
|---|---|---|---|---|---|
| S-raw (2884 pixels) | 31.8 | 41.5 | 45.4 | 46.3 | 2318 mul-adds |
| S-gates (5 authored gates) | 91.3 | 99.2 | 100.0 | 100.0 | 15 mul-adds |
| S-gates-minus (no LOS gate) | 84.0 | 96.5 | 96.7 | 96.7 | 14 mul-adds |
| CODE (hand rule) | 100.0 | 100.0 | 100.0 | 100.0 | 1135 ops |
| BASE (always SCOUT) | 45.0 | 45.0 | 45.0 | 45.0 | — |

The **dial** reads 100.00% agreement between S-gates@5000 and CODE, so the script prints **"model can be replaced by code here"** — and the student is 77× cheaper per call than the rule it learned.

**The outside-space boundary** is where it gets sharp. Changing the truth to also HIDE when an enemy sits exactly 2 cells away diagonally flips only 38/10,000 eval cases (0.4%), so S-gates' headline accuracy barely moves (100.0 → 99.6) — yet it scores **0.0%** on exactly those changed cases. Because the natural distribution is too rare to learn the clause, S-raw also scores near zero there (21.1%), so the slice alone can't tell the two apart. I added a matched-pair capability probe (200 pairs differing *only* in the enemy's position, both hidden behind walls) to separate the causes: the gate vectors are provably identical across each pair while the raw vectors differ, which pins S-gates at a hard 50% ceiling no matter how much data it gets, while S-raw reaches 88.2% when handed data of that kind. The gates' blindness is structural; plain code's is not.

Two things worth flagging about how I set it up, since both were judgment calls:

- **S-raw got its fair shot.** It first landed at chance level (30–46%), which looked like a bug. It wasn't: with ~766 inputs active per case, the step size that's safe for the 8-feature gates overshoots badly, and across seeds it collapsed to 30.1/38.7/45.8. Retuning to `lr=0.03` made it stable and monotone. It still ends at 46.3% — barely above the 45.0% majority-class baseline — which is the honest result for a linear model over one-hot pixels.
- **Cost proxy semantics.** The 2318 figure is genuinely the whole board read agent-relative (off-board padding is marked as wall, which blocks movement and sight-lines exactly as walls do). The brief permits a 9×9 crop, which would be far cheaper but would also hide the beacon in many cases; I chose full coverage to keep the comparison about representation rather than field of view, and noted the trade-off in the reading.

Deliverables: [sim.py](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/student/sim.py) and the captured run output in [out.md](/tmp/claude-1000/-home-proart-projects-last-mile/e49c5640-effc-40cf-8174-a667000d464b/scratchpad/poc/student/out.md). Nothing outside this directory was touched, and `BRIEF.md` is unmodified.
84 raw pixels) climbs 31.8 -> 46.3% and stops, barely above the 45.0% majority-class baseline: a linear model over pixels cannot represent 'LoS within 6 through walls' nor an exact BFS path length.
  3. S-gates-minus (the LOS gate forgotten) loses 3.3 pts at n=5000 and plateaus at 96.7%: the coarse 'nearest enemy <=7' bucket cannot certify 'visible within 6' - the sharp question is what carries the signal.
  4. Small data favours the authored questions: at n=50 S-gates 91.3% vs S-raw 31.8% (59.5 pts), for 15 mul-adds instead of 2318 - the raw student reads the whole board, so it is even dearer than the 1135-op rule it replaces.
  5. The dial: S-gates agrees with CODE on 100.00% of decisions at n=5000 -> "model can be replaced by code here"; the student is 77x cheaper per call than the rule it learned.
  6. Outside the declared space (diagonal-2 HIDE) S-gates drops to 99.6% overall and 0.0% on the 38 changed cases (S-raw 21.1% - the clause is too rare to learn), but the probe separates causes: S-gates is hard-capped at 50% while S-raw reaches 88.2% given such data - the gates' blindness is structural, code's is not.
EXIT 0
