# The attention gate, asked four ways

The evidence behind the section *"Decompose before you escalate: the attention
gate, four ways"* in `crates/lm-agents/tests/talk_on_thor.md`.

`attention-sweep.jsonl` is one line per situation: 75 distinct facts texts
that the shipped round asked the attention gate in, each asked by all four
arms and by the judge. The arms are the shipped code under a different shape
of that one gate:

| key | the arm |
|---|---|
| `A today` | the ten-option question, escalated to clef-flash below 0.5. The key is the note's word for it ("arm A: today"); D75 has since narrowed the shipped escalation to the move gate alone, so the arm is the attention gate escalated, not the attention gate as the harness runs it |
| `B tree` | the same gate as a two-step tree on the d1-2B alone |
| `C tree+escalate` | the tree, with its last step escalated to clef-flash |
| `D ghost` | the ten-option question with its rarely-chosen options ghosted |

Each line carries, per arm: the option it chose, its confidence, how many
options the gate was asked between, the wall clock the decision took
(`wall_ms`), the model's own round trip (`model_ms`), the cheap model's
probabilities (`primary_probs`), the second opinion's (`second_probs`, empty
where none was bought), and the tree's steps (`steps`, empty where the gate
was not a tree). The line also carries `judge` (what the d1-3B chose) and the
Hodge's `gradient`, `curl` and `harmonic` for that question.

Regenerate it with:

```text
LM_DUMP=evidence/decompose/attention-sweep.jsonl \
  cargo test --release -p lm-agents --test decompose_on_thor -- --ignored --nocapture
```

It needs the Thor on this machine (d1-2B on :8094, clef-flash on :8096, the
d1-3B judge on :8093).

## What the numbers say

- The d1-2B's own flat answer agrees with the judge in **55 of 75**; after
  clef-flash has had its say, the escalated arm agrees in **29 of 75**. The
  escalation on this gate makes the answer worse, and costs 1394 ms a time.
- The tree agrees with the judge in **4 of 75**. It never says "squad": its
  first step picks the `squad_0` or `squad_1` partition in every one of the
  75 situations, and the judge says "squad" in 67 of them.
- The Hodge says gradient in 75 of 75 (mean curl 0.027), so this gate is not
  one to decompose.
