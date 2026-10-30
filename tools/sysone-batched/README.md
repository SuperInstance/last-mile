# sysone-batched

A System One server for the d1-2B decision model, on port 8104.

It takes the same request as the llama.cpp fork behind `:8094`
(`POST /v1/systemone` with `{"model", "state", "questions"}`), it answers with
the same shape, and it gives **the same answers**: the responses are
byte-for-byte identical, down to the last bit of every probability.

What it does differently is *when* the questions of a pack are evaluated. The
fork's server reads the questions of a pack one after another; this server
reads them at the same time, each on its own CUDA stream.

---

## The brief asked for one forward pass per pack. The model will not do it.

The brief's plan was: share the state prefix across the questions of a pack,
score the pack in one forward pass, and read every option's probability out of
the next-token distribution. That plan is right about the *options* and wrong
about the *questions*, and the reason is in the model.

### The options are already one forward pass

Every option of a question is already scored from a single forward pass. The
"systemone" chat template ends with `Reply with the option code only.`, so
the model answers with a label (`A`, `B`, …; `yes`/`no`; `0`, `1`, …) and the
fork reads the logits of those labels out of the last position of the prompt.
The fork calls this the *label groups*: the labels of one option are the
spellings of it (`A` and ` A`), and the option's score is the largest logit of
its group.

**So every option of every question in last-mile's packs is a single token.**
There are no multi-token options to worry about: the option keys are the
single tokens `A`…`J` for a choice, `yes`/`no` for a noul, and the digits
`0`…`n` for a score. The shared-prefix machinery the brief describes for
multi-token options is not needed for this model.

### The questions cannot share a prefix

The state is the same for every question of a pack, and the template puts it
first, so the questions do share a token prefix (86 tokens of a 200-token
prompt with a short state, more with a long one). It cannot be used, because:

1. **The model has no KV cache.** `lfm3.decision.type` is `lfm3-d1`, the
   architecture is `lfm3`, and the model's own metadata says
   `lfm3.attention.causal = false`. A non-causal LFM3 has no memory at all:
   `llama_model::create_memory` in `src/llama-model.cpp` returns nothing for
   `LLM_ARCH_LFM3` when `hparams.causal_attn` is false, with the comment
   *"the non-causal LFM2 and LFM3 decision graphs read the whole prompt in one
   batch, nothing is kept"*. There is no cache to copy a prefix into and
   nothing to continue from. `llama_get_memory()` on such a context returns
   `nullptr`.

2. **The attention is bidirectional.** `causal_attn = 0`, so every token of
   the prompt sees every other token. The state cannot be evaluated without
   the question: the state's own representation depends on the question that
   follows it. A cached prefix would be the wrong prefix.

3. **The short convolutions mix the sequences of a batch.** The LFM3 short
   convolutions are applied to the *flat* token array of the batch
   (`ggml_ssm_conv` over the whole ubatch in
   `llm_build_lfm_base::build_lfm_shortconv`), so two sequences in one batch
   convolve into each other at the boundary. The fork says the same thing in
   `tools/server/server-decision.cpp`: *"the short convs of lfm3 d1 have no
   memory to separate the sequences of a batch"*, and it sets
   `task.decision.no_batching = true` for this model.

### Measured, not just read

`probe-batching.cpp` puts the questions of a pack into one batch, three ways,
and compares the answers with one question per forward pass:

| way | worst logit difference | questions that differ |
|---|---|---|
| A: one question per forward pass | — | — |
| B: one batch, the sequences of different lengths | 0.186495 | 11 / 11 |
| C: one batch, the sequences padded to the longest | 1.470747 | 11 / 11 |

B is the conv mixing the sequences; C is the padding, which the bidirectional
attention lets the real tokens see. Both are wrong, and neither is a
rounding difference.

So **one forward pass per question is the floor for this model**, and the
fork's server already reaches it.

---

## What this server does instead

The questions of a pack are independent of each other. They do not have to
wait for each other.

The fork's server keeps eight slots in **one** context, and one context means
one CUDA stream: the questions of a pack are evaluated one after another on
the GPU, and the GPU idles between the small forward passes.

This server keeps a pool of **contexts**, each with its own CUDA stream, and
hands the questions of a pack (and the packs of several clients) to them. The
GPU stays busy, and the pack's round trip is the slowest question rather than
the sum of them.

The answers do not change. A question is still evaluated on its own, in its
own forward pass, with its own prompt and its own positions — the fork
evaluates it exactly the same way. `probe-concurrency.cpp` checks this: the
same pack on 1, 2, 4, 8 and 11 contexts gives a worst logit difference of
**0.00000000**.

## What is reused, and what is written here

The rendering and the formatting are the fork's own, so the prompts, the
tokenization, the option labels and the answer shape are the bytes the fork
would use:

* the prompt is rendered by the fork's `server_decision_context::render`,
  through the model's own `systemone` chat template;
* the option labels are the fork's `server_decision_context::d1_labels`;
* the answer is shaped by the fork's `server_decision_context::format_answer`,
  with the model's own temperatures (`choice` 0.8, `score` 0.75, `noul` 0.7).

The binary links against the build that is already on the box,
`~/llama.cpp-upstream/build-up`. Nothing is cloned from GitHub, nothing is
built from GitHub, and no other llama.cpp tree is needed.

What is written here is the part the fork does not have: the pool of
contexts, the queue of questions, and the HTTP server around them.

## Files

| file | what it is |
|---|---|
| `sysone-batched.cpp` | the server |
| `build.sh` | builds the server and the two probes against `~/llama.cpp-upstream/build-up` |
| `install.sh` | builds it and installs the binary, the launcher and the unit |
| `lm-sysone-batched.sh` | the launcher the unit runs |
| `lm-sysone-batched.service` | the systemd user unit |
| `compare.py` | asks two servers the same packs and compares the answers |
| `bench.py` | the latency of two servers, at 1, 4 and 8 requests in flight |
| `probe-batching.cpp` | the measurement that shows a pack cannot be one forward pass |
| `probe-concurrency.cpp` | the measurement that shows the contexts agree bit for bit |

## Running it

```bash
# build and install into ~/lm-services/sysone-batched/, and put the unit in
# ~/.config/systemd/user/
bash tools/sysone-batched/install.sh

# run it in the foreground
tools/sysone-batched/sysone-batched --port 8104 --workers 8

# as a service
systemctl --user daemon-reload
systemctl --user enable --now lm-sysone-batched.service
```

The options: `--model`, `--host`, `--port`, `--workers` (the contexts, and so
the CUDA streams), `--ctx`, `--ubatch`, `--http` (the server's own thread
pool), `--alias`.

### The knobs that matter

* **`--workers`**: how many contexts, and so how many questions are read at
  the same time. Each context holds about 1.1 GiB of GPU compute buffer at
  `--ubatch 2048`.
* **`--ubatch`**: the model reads the whole prompt in one ubatch, so this has
  to be at least as long as the longest prompt of a pack (about 630 tokens
  with a long state). 2048 is comfortable.

## Measuring it

```bash
# the answers of two servers on the same packs
python3 tools/sysone-batched/compare.py 8094 8104 packs.jsonl 500

# the latency of two servers, at 1, 4 and 8 requests in flight
python3 tools/sysone-batched/bench.py 8094 8104 packs.jsonl
```

The packs are the real ones the harness asks: the live lane writes them, one
per line, when `LM_CAPTURE_PACKS` is set (see
`crates/lm-agents/tests/batched_sysone.md`).

**The box is shared.** The d1-2B on `:8094` serves a live game from the
ProArt, and the GPU is at 98% while it does. The two servers have to be
measured against each other, in turn, on the same packs, or the numbers are
noise.
