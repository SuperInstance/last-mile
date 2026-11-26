# The batched System One server, on the Thor

**What this is.** A System One server for the d1-2B, on port 8104, that
answers the same requests as the llama.cpp fork on 8094 with the same answers,
and reads the questions of a pack at the same time instead of one after
another.

**The short version.** The brief asked for the pack to be scored in one
forward pass, with the questions sharing the state prefix. The d1-2B cannot
do that, and the reason is in the model: it is LFM3-D1, its attention is
bidirectional and it has no KV cache at all, so there is no prefix to share
and the short convolutions mix the sequences of a batch. One forward pass per
question is the floor, and the fork's server already reaches it.

What is left is to stop the questions of a pack from waiting for each other.
They are independent, so they do not have to. This server keeps a pool of
contexts, each with its own CUDA stream, and hands the questions to them. The
answers do not move: over 506 real packs, every one of the 1190 answers came
back **bit for bit identical** to the fork's — measured twice, once by the
run that built this and once by the handover run that checked it.

What the overlap buys in wall clock depends on the GPU: on this box, which is
95-98% busy serving a live game, the full pack is a wash at the median and
the single-question move pack is worse; the win is structural — six times the
model-time overlap — and pays when the GPU has room. Section 4 says what that
would let last-mile undo (D70's split, D74's 4000 ms).

**The code.** `tools/sysone-batched/` in this repository: the server, the
build, the service, the probes that prove the two claims above, and the
scripts that compare and measure.

---

## 1. The model, and why a pack is not one forward pass

### The options already are one forward pass

The "systemone" chat template ends with `Reply with the option code only.`,
so the model answers a question with a *label*, not with the option's text.
The fork reads the logits of those labels out of the last position of the
prompt and turns them into the option's probability; the labels of one option
are the spellings of it (`A` and ` A`), and the option's score is the largest
logit of its group (`server_decision_context::d1_labels`).

**Every option in last-mile's packs is a single token.** The keys are `A`…
`J` for a choice, `yes`/`no` for a noul, and the digits `0`…`n` for a score
(the score question's criteria are read as `{{ o.key }} {{ o.description }}`
and its key is the level's index). There are no multi-token options in the
real packs, so the shared-prefix machinery the brief describes for multi-token
options is not needed for this model — the simplest batching the brief asks
for is already what the fork does.

### The questions cannot share a prefix

The state is the same for every question of a pack and the template puts it
first, so the questions do share a token prefix — 86 tokens of a 200-token
prompt with a short state, and more with a long one. It cannot be used:

| what | where | what it says |
|---|---|---|
| the model has no KV cache | `src/llama-model.cpp`, `llama_model::create_memory` | *"the non-causal LFM2 and LFM3 decision graphs read the whole prompt in one batch, nothing is kept"* — for `LLM_ARCH_LFM3` with `hparams.causal_attn` false there is no memory at all, and `llama_get_memory()` on such a context returns `nullptr` |
| the attention is bidirectional | the model's own metadata, `lfm3.attention.causal = false` | every token of the prompt sees every other token, so the state cannot be evaluated without the question that follows it |
| the short convolutions mix the sequences | `src/models/lfm-base.cpp`, `llm_build_lfm_base::build_lfm_shortconv` | the conv runs over the *flat* token array of the batch, so two sequences in one batch convolve into each other at the boundary |

The fork says the same thing in `tools/server/server-decision.cpp`: *"lfm3 d1
has no memory, its prompt is read as one flat ubatch and the short convs
would mix the sequences"*, and it sets `task.decision.no_batching = true` for
this model.

### Measured, not just read

`tools/sysone-batched/probe-batching.cpp` reads the eleven questions of a
real pack three ways and compares the answers with one question per forward
pass:

| way | worst logit difference | questions that differ |
|---|---|---|
| A: one question per forward pass | — | — |
| B: one batch, the sequences of different lengths | 0.186495 | 11 / 11 |
| C: one batch, the sequences padded to the longest | 1.470747 | 11 / 11 |

B is the conv mixing the sequences; C is the padding, which the bidirectional
attention lets the real tokens see. Neither is a rounding difference.

### The other stacks

The brief asks what happens if the simple batching fails. On this box:

* **llama.cpp's own batched decoding** — the route above: the prefix copy
  (`llama_memory_seq_cp`) has no cache to copy into, and the multi-sequence
  batch mixes the sequences through the short conv.
* **A TensorRT build of the same weights** — there is no TensorRT runtime
  and no Model Optimizer on this box (`~/ml/.venv` has torch 2.11,
  transformers 5.20 and onnxruntime 1.31, and no `tensorrt` and no
  `nvidia-modelopt`; there is no `.engine` or `.plan` anywhere under
  `~/models`). Converting LFM3-D1 would also be a research project of its
  own, and it would not dodge the constraint: a batched prefill of a
  bidirectional architecture with a flat short convolution mixes the
  sequences in the engine too, unless the engine gives each sequence its own
  convolution state.
* **The Python stacks in `~/ml`** — no `llama-cpp-python`, and a PyTorch
  port of this architecture would be slower than the CUDA build that is
  already serving it.

So the llama.cpp build that is already on the Thor is the right stack, and
what it gives up is not the number of forward passes but the *ordering* of
them.

---

## 2. What the server does instead

The questions of a pack are independent of each other, so they do not have to
wait for each other.

The fork's server keeps eight slots in **one** context, and one context means
one CUDA stream: the questions of a pack are read one after another on the
GPU. This server keeps a pool of **contexts**, each with its own CUDA stream,
and hands the questions of a pack — and the packs of several clients — to
them.

The answers do not change. Each question is still evaluated on its own, in
its own forward pass, with its own prompt and its own positions — the way the
fork evaluates it. `tools/sysone-batched/probe-concurrency.cpp` checks it:
the same pack on 1, 2, 4, 8 and 11 contexts gives a worst logit difference of
**0.00000000**.

The rendering and the formatting are the fork's own
(`server_decision_context`), so the prompts, the tokenization, the option
labels, the temperatures and the answer shape are the bytes the fork uses.
The binary links against the build that is already on the box,
`~/llama.cpp-upstream/build-up`; nothing was cloned or built from GitHub.

### What the overlap looks like

From the server's own log, which logs the model time and the wall clock of
every pack. Over the live-lane window — the first 78 ten-question packs the
service answered — the questions took **695 s of model time** summed and the
packs took **113 s of wall clock**: the questions overlapped by about
**6.2x**, against 10x if the GPU were free. The pack-by-pack traffic of the
correctness compares reads the same (6.0x over their 68 ten-question packs).
The bursts read lower, and the reason is in the log with them: the bench of
section 3 fires four and eight full packs at once, the burst's packs queue
behind each other for the pool, and each pack's wall clock carries the wait —
1.9x over the bench's 195. What the ratio measures is how much of a pack's
wall clock is its own questions; the lane and the compares are the shape the
agents actually send, and there it is six. On this box the GPU is not free,
so the overlap mostly re-orders the GPU's work instead of shortening it; that
is why the wall-clock tables above move so little while the model-time
overlap is real.

---

## 3. The numbers

### The answers: 506 real packs, 1190 questions

The packs are the ones the harness asks in the live lane — the real gate
texts, the real menu, the real facts text of the game — captured from two
live-lane runs (one against each server; the two runs play the game
differently, because a faster answer lands on a different tick, so the two
runs between them cover 506 distinct packs).

Every pack was asked of both servers and the answers compared — twice: once
by the run that built this server, once by the handover run that checked it
under the systemd unit. Both runs gave the same table:

| | |
|---|---|
| packs | 506 |
| questions compared | 1190 |
| **the same choice** | **1190 / 1190 (100.000 %)** |
| different or missing | 0 |
| **the largest probability difference** | **0.000000000** |
| the whole response, byte for byte | identical on every response checked raw (the first 25 packs of the handover run); the 1190 are compared field for field |

The gate the brief sets is 99 % agreement and 1e-4 of probability. The
answer is bit-for-bit the fork's, which is what a drop-in replacement has to
be: the agents' thresholds and the escalation rule are calibrated on the
fork's numbers, and a server that moved the probabilities by a rounding
error would move the decisions.

### The latency

The two servers were asked the same packs, in turn, round by round, so they
see the same load of the box (15 rounds per point), and the load average and
the GPU use were sampled while each point ran, because the box is shared and
the load on it moves — during tonight's run another seat took the load from
13 to 98, and the points taken under that swing are marked below, not hidden.

**Tonight's run, with the load recorded** (`bench.py` samples `/proc/loadavg`
and `nvidia-smi` once a second while it measures):

The move pack — one question, the one the agent asks on every think:

| in flight | :8094 p50 / p95 (ms) | :8104 p50 / p95 (ms) | p50 / p95 | req/s 8094 / 8104 | load | GPU |
|---|---|---|---|---|---|---|
| 1 | 258.5 / 661.3 | 301.5 / 327.8 | 0.86x / 2.02x | 2.97 / 3.28 | 12.8-15.3 | 95-98% |
| 4 | 368.0 / 33094.6 | 826.8 / 1482.8 | 0.45x / 22.3x | 1.09 / 4.86 | 15.3-97.8 | 87-98% |
| 8 | 489.2 / 863.2 | 784.7 / 1130.4 | 0.62x / 0.76x | 9.16 / 7.94 | 50.8-71.3 | 81-98% |

The full pack — ten questions, the talk pack:

| in flight | :8094 p50 / p95 (ms) | :8104 p50 / p95 (ms) | p50 / p95 | req/s 8094 / 8104 | load | GPU |
|---|---|---|---|---|---|---|
| 1 | 754.2 / 996.6 | 719.1 / 947.0 | 1.05x / 1.05x | 1.27 / 1.30 | 38.7-50.8 | 90-98% |
| 4 | 2122.7 / 3018.4 | 2316.3 / 4820.5 | 0.92x / 0.63x | 0.78 / 1.18 | 16.4-36.2 | 44-98% |
| 8 | 2799.7 / 4746.7 | 3101.9 / 5230.3 | 0.90x / 0.91x | 1.67 / 1.59 | 18.9-25.0 | 5-98% |

Two points in tonight's run are load-polluted and should not be read: the
move pack's 4-in-flight row, where the fork's p95 of 33 s is the other seat's
load spike (the load column says so: 15 to 98 during the point), and the full
pack's 4-in-flight row, where the fork's higher requests/s sits next to worse
percentiles because the load was falling across the point and the fork is
asked first in every round. The percentiles are the safer comparison; the
load column is what tells the two apart.

**The earlier run** (the same method, minutes after the live-lane runs; the
load was not sampled then, and its requests/s were computed with one wall
clock shared by both servers, so its "identical throughput" was true by
construction — the wall clocks are separate in tonight's numbers):

| | :8094 p50 / p95 (ms) | :8104 p50 / p95 (ms) | p50 / p95 |
|---|---|---|---|
| move pack, 1 in flight | 192.3 / 201.5 | 190.8 / 201.1 | 1.01x / 1.00x |
| move pack, 4 in flight | 439.8 / 813.7 | 796.7 / 1002.1 | 0.55x / 0.81x |
| move pack, 8 in flight | 836.9 / 1588.3 | 1156.4 / 1608.9 | 0.72x / 0.99x |
| full pack, 1 in flight | 1412.0 / 1461.1 | 1255.9 / 1482.3 | 1.12x / 0.99x |
| full pack, 4 in flight | 4447.2 / 11098.4 | 4730.2 / 8011.7 | 0.94x / 1.39x |
| full pack, 8 in flight | 9653.0 / 35794.2 | 10235.3 / 17145.2 | 0.94x / 2.09x |

**What is stable across both runs, and what is not.** Stable: the full pack
at the median is a wash (0.90x-1.12x), with the batched server ahead when
only one request is in flight (1.05x, 1.12x) — the ten questions overlap, and
the pack's round trip is the slowest question plus the GPU's contention; the
move pack, one question with nothing to overlap, is *worse* at four and eight
in flight (0.45x-0.72x), because splitting the GPU over eight streams makes
every request in flight wait for the whole batch — that is the convoy, and it
is the honest cost of the design. Not stable: the p95 of the full pack. The
earlier run gave the batched server 1.39x and 2.09x at four and eight in
flight; tonight's run, under a swinging load, gave 0.63x and 0.91x. On a box
whose GPU is at 95-98% for the live game, the p95 is a measurement of the
load as much as of the server, and neither run's p95 should be quoted without
the other.

The requests/s of tonight's run tell the same story as the percentiles: the
full pack is a wash (1.27/1.30 at one in flight, 1.67/1.59 at eight), and the
fork is ahead on the move pack at eight in flight (9.16 against 7.94), where
the convoy costs the batched server its throughput. **The GPU is the limit on
this box, not the server**: the d1-2B on :8094 serves a live game while these
numbers are taken, and two servers that both keep the GPU busy deliver about
the same packs per second, whatever they do with the ordering.

**The rule this gives:** the batched server is the right one for a pack with
many questions on a GPU that has room, and it is the wrong one for a pack
with one question on a GPU that is already full. The fix for that is not a
better server but a d1-2B on a GPU of its own, or the move pack on the fork's
server and the talk pack on this one.

### The live lane

Three rounds, 3000 ticks each, seeds 1-3, the same harness, run back to back
on this box, against the same :8104 with eight workers:

| | :8094 (the fork) | :8104 (the batched one) | D74, on a quiet box |
|---|---|---|---|
| the move pack, p50 / p95 | 390 ms / 1585 ms | **374 ms** / 2183 ms | 164 ms / 262 ms |
| the talk pack, p50 / p95 | **2081 ms** / **3658 ms** | 2606 ms / 3887 ms | 770 ms / 1586 ms |
| decisions over the three rounds | 1325 | 1402 | 2240 |
| lines over the three rounds | 33 | 15 | 163 |
| the packs asked (sent / answered) | 420 / 410 | 380 / 371 | — |

Two things to read here.

**The D74 column is not comparable.** It was measured on 2026-10-29, on a
box that was not serving a live game. The box today serves one, and the GPU
is the limit for both servers, so both are slower than D74 by a factor that
has nothing to do with the servers. The numbers that *are* comparable are
the two right-hand columns, taken on the same box, minutes apart.

**The two runs are not the same game.** A faster answer lands on a different
tick, so the game plays out differently and the decision and line counts are
not a measurement of anything. The round trip per pack is the comparable
part. (The packs for the correctness gate came from the :8094 lane above and
an earlier :8104 lane with four workers — 351 and 296 packs captured, 506
distinct between them.)

And there the two servers are a wash: the move pack's median is 374 ms
against 390 ms, the talk pack's is 2606 ms against 2081 ms. The bench above
says why the talk pack can go either way — the batched server reads the ten
questions of a pack together, but in a live round the talk pack has to share
the pool with the move packs, and the GPU is full either way.

**The worker count is a real knob, and this is where it shows.** The live
lane above was run with eight workers, the fork's number of slots. The same
lane with four workers gave a move pack median of 433 ms — worse than the
fork's 390 — because with eight agents asking at once and only four
contexts, the requests queue where the fork's eight slots do not. With eight
workers the move pack is level with the fork.

---

## 4. What to take from this

* **The answers are safe.** The batched server is a drop-in: the same
  request, the same answer, the same bytes. Whatever is done with it, the
  agents' decisions do not move.
* **The pack cannot be one forward pass.** Not on this model. The options
  are already one forward pass; the questions cannot share a prefix, because
  the model has no cache and its attention is bidirectional. The probes in
  `tools/sysone-batched/` are the proof, and they are the answer to "say
  which options are single- and which are multi-token": all of them are
  single, and the shared-prefix work is not needed.
* **The server is worth it when the GPU has room.** On a box that is not
  already serving a live game, the ten questions of a pack are read about
  six times in parallel, and the pack's round trip is the slowest question
  instead of the sum of them.
* **The GPU is the limit on this box today.** Both servers deliver about the
  same packs per second of the same load (tonight: 1.27/1.30 req/s on the
  full pack at one in flight, 1.67/1.59 at eight). The next gain is not a
  faster server: it is giving the decision model a GPU of its own, or
  splitting the load between the move pack and the talk pack.

### What the speedup would let us undo

Two decisions in the table exist only because the fork reads a pack one
question after another:

* **D70 split the pack.** The full 11-question pack costs 473 ms per thought
  on the fork (about 11 ms per option, one after another) against the move
  question's 82 ms, so the move pack is asked on every think and the ten talk
  gates only when something changes. The price of the split is the same
  table's to read: between talk packs, most thinks run with no talk
  information at all, on stale social state.
* **D74 raised the request timeout to 4000 ms** (and the think budgets to
  105/105 ticks) to fit the talk pack plus its second opinion under the
  fork's latency.

What the batched server changes is the arithmetic of the talk pack. Its ten
questions are one forward pass each, read together — the server's own log
shows them overlapping 6.2x in model time over the live-lane window (the
first 78 ten-question packs the service answered: 695 s of model time summed,
113 s of wall). When the GPU has room, the pack's round
trip tends to the slowest question rather than the sum of ten: a talk-pack
question is the same one forward pass over about the same prompt length as
the move question, so on the quiet box where D74 measured the move pack at
p50 164 ms, a read-together talk pack lands near that — against the 770 ms
D74 measured for the fork reading it in order. That would undo both:

* **D70's split could come back.** One request with all eleven questions on
  every think, at about the cost of today's move pack, and the "when to ask
  the talk pack" harness parameter goes away with it — the agent would never
  think on stale social state again.
* **D74's 4000 ms timeout could come back down** toward the move pack's
  envelope: roughly a second for the pack plus its second opinion, instead of
  the 4 s that the sequential talk pack under load needs.

**What tonight's box says, honestly: nothing to undo yet.** The GPU is at
95-98% for the live game, and a full GPU turns the pool of streams into a
queue — at the median the full pack is a wash and the move pack is worse,
because eight streams make every request in flight wait for the whole batch.
The undo above is earned when the decision model has GPU headroom: a GPU of
its own, or the live game on another box. What can be done today is the
cheaper half of it — the move pack on the fork's :8094, where one question
and one stream are the right shape, and the talk pack on :8104, where the
ten questions are read together.

---

## 5. How to reproduce

```bash
# build and install
bash tools/sysone-batched/install.sh

# the service
install -Dm755 tools/sysone-batched/lm-sysone-batched.service \
    ~/.config/systemd/user/lm-sysone-batched.service
install -Dm755 tools/sysone-batched/lm-sysone-batched.sh \
    ~/lm-services/sysone-batched/lm-sysone-batched.sh
systemctl --user daemon-reload
systemctl --user enable --now lm-sysone-batched.service

# the live lane against the batched server, with the packs captured
LM_THOR_PORT=8104 LM_CAPTURE_PACKS=/tmp/packs.jsonl \
  cargo test --release -p lm-agents --test talk_on_thor -- --ignored --nocapture

# the answers of two servers on the same packs
python3 tools/sysone-batched/compare.py 8094 8104 /tmp/packs.jsonl 500

# the latency of two servers, at 1, 4 and 8 requests in flight; the load
# average and the GPU use are sampled while it runs and printed with every
# point, because the box is shared and the numbers move with its load
python3 tools/sysone-batched/bench.py 8094 8104 /tmp/packs.jsonl 15
```

The live lane takes `LM_THOR_PORT` (the port of the d1-2B to measure) and
`LM_SEEDS` (the seeds to play; the default is the three of D74).

The server's knobs: `--workers` is the pool of contexts, and so the number
of CUDA streams and the number of questions read at the same time. It is the
knob that matters: eight matches the fork's eight slots, and fewer workers
make the queue longer while more workers make the convoy wider.
