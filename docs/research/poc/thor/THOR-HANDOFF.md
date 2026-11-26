# Decision models on the Thor — handoff for the ProArt agent

**Written 2026-10-28. Everything below was measured on the Thor, not quoted from a datasheet.**

---

## 0. The short version

The Thor is a **Jetson AGX Thor** on the LAN at **`thor.local`** (192.168.0.78). It runs four
decision models plus a perception rack, and **all of them speak one API**: TypeSafe's
**System One** protocol, `POST /v1/systemone`. The same protocol the cloud Jev provider uses.

You almost certainly do **not** need to download anything — just point your client at the
endpoints in §2. Fetch instructions (§4) are only if you want a copy on the ProArt.

---

## 1. The API

```
POST {base}/v1/systemone
Content-Type: application/json

{
  "model": "<model id>",
  "state": "any string or JSON describing the situation",
  "questions": {
    "<question-id>": {
      "type": "noul",                                   // probability of "yes", 0..1
      "instructions": "...",
      "criteria": { "true": "...", "false": "..." }     // optional
    },
    "<question-id>": {
      "type": "choice",                                 // picks one key of criteria
      "instructions": "...",
      "criteria": { "<key>": "description of when this applies", ... }
    },
    "<question-id>": {
      "type": "score",                                  // probability-weighted level
      "instructions": "...",
      "criteria": ["level 0", "level 1", "level 2"]     // ordered list
    }
  }
}
```

Response:

```json
{
  "model": "/home/caseydigennaro/models/d1-omni-600M/d1-omni-600M-Q8_0.gguf",
  "answers": {
    "urgent":   { "type": "noul",   "noul": 0.9848 },
    "responder":{ "type": "choice", "choice": "nearby", "probabilities": {...}, "confidence": 0.93 },
    "severity": { "type": "score",  "score": 1.19, "legend": {"0":"none","1":"minor",...},
                  "probabilities": {...}, "confidence": 0.10 }
  },
  "usage": { "input_tokens": 245, "output_tokens": 0 }
}
```

**`output_tokens` is always 0.** These models do not generate text. They score options. That
is the whole point: one forward pass, typed answers, no parsing, no prompt-injection surface
from the model's own output.

Images (on the models that take them): send `"images": ["data:image/png;base64,..."]` — a
**plain data-URL string list**, up to 8. (The *cloud* Jev API instead takes an `assets` map
plus a `TypeSafe-Danger-Zone-Images: true` header — different shape, same idea. If you are
writing a client that must talk to both, that is the one difference to abstract.)

---

## 2. Endpoints — all live, all LAN-open

| URL | Model | Use it for | Measured (Thor, GPU on) |
| --- | --- | --- | --- |
| `http://thor.local:8095` | **d1-omni-600M** | **the reflex tier** — text, fast, free | 9.7 ms (1 q) · **15.4 ms (3 q)** · 33.9 ms (3 q, long state) |
| `http://thor.local:8094` | d1-2B | mid tier, vision + video | 20.3 ms (1 q) · 60.3 ms (3 q) · 104.6 ms (3 q, long) |
| `http://thor.local:8093` | d1-3B | the vision tier | 27.3 ms (1 q) · 90.1 ms (3 q) · 103.6 ms (3 q, long) |
| `http://thor.local:8096` | clef-flash | strongest answers | 157.1 ms (1 q) · 284.4 ms (3 q) · 386.6 ms (3 q, long) |
| `http://thor.local:8077` | jev-rack | **perception**, not decisions — see §3.5 | I-JEPA 162.5 ms · judge 23.1 ms · V-JEPA2 7.8 s |
| `http://thor.local:8188` | ComfyUI | image generation | — |
| `http://thor.local:8200` | foundry portal | procedural asset foundry | — |

Latency is the median of 15 calls after 3 warmups, HTTP baseline 0.6–0.8 ms, all four models
measured on the same llama.cpp build and the same flags, one at a time. Cloud Jev answers the
same three-question pack in **~334 ms**; cloud clef-flash does it in **38.8 ms median**.

**These endpoints have no authentication.** Anyone on the LAN can spend the Thor's GPU. That is
Casey's deliberate choice. If you need to be polite, keep your call rate low; if you need
auth, say so and a `--api-key` gets added.

**Name resolution:** `thor.local` now resolves to 192.168.0.78. Two fixes were needed and are
in place: the Thor was advertising `thor` on its Docker bridge (so `thor.local` pointed at
172.17.0.1), and it was publishing `proart.local` for itself. **Consequence for you:
`proart.local` no longer resolves on the LAN** — the Thor was the only thing publishing that
name, so the record was pointing every lookup at the wrong machine. The only other host
advertising itself on this segment is **192.168.0.50, as `linux.local`** — if that is your
ProArt, rename its avahi hostname to `proart`, or add `192.168.0.50 proart.local` to your
`/etc/hosts`. Either way, **use the IP if in doubt**: 192.168.0.78 is the Thor.

---

## 3. A brief on each

### 3.1 d1-omni-600M — `thor.local:8095` ← **the one you want by default**

Liquid AI's smallest decision model. **587M parameters total**: a 381M LFM2.5 trunk, a 94M
SigLIP2 vision encoder, a 112M FastConformer audio encoder.

- **Why:** it is 20× faster than the cloud tier and free. A three-question pack answers in
  15.4 ms, so a whole decision pack fits inside a game tick.
- **Strengths:** fastest by a wide margin; takes text, images and audio; strong enough that
  the answers are usable, not just fast.
- **Weaknesses:** the smallest model, so it is the first to look unsure. Its `score` answers
  can come back nearly flat (0.28 / 0.28 / 0.39 across three levels), which is honest but
  means you need a confidence threshold.
- **Scaling:** the first question pays the state prefill (~14 ms); each extra question adds
  ~10 ms. Three questions ≈ 15 ms, eight ≈ 83 ms. **Pack size is a budget.**
- **Licence:** LFM Open License (LFM1.0). Read it if the work is commercial.

### 3.2 d1-2B — `thor.local:8094`

LFM3-VL-2B backbone. Text + images + video.

- **Why:** the middle option — 2B params, faster than d1-3B on short packs (60 ms vs 90 ms
  for three questions), and the only d1 that takes **video**.
- **Weakness:** the **LFM3 architecture needs a recent llama.cpp**. The Thor's older
  0.7.0-dev build rejects it with `unknown model architecture: 'lfm3'`; upstream added it in
  llama.cpp #31252. If you fetch it yourself, use upstream `master` (0.8.0-dev or later).
- **Licence:** LFM Open License (LFM1.0).

### 3.3 d1-3B — `thor.local:8093`

LFM2.5-VL-3B backbone. The flagship d1 — what Liquid publishes a 16 ms AGX Thor figure for.

- **Why:** bigger and better-calibrated than the 600M; the right choice when a decision needs
  to be right more than it needs to be fast.
- **Weakness:** it is **not** the fastest despite being newer. On a three-question pack it
  costs 90 ms against the 600M's 15 ms, because the bottleneck is backbone prefill, which
  scales with parameter count. On the Thor the smaller model wins by 6×.
- **Vision:** 320 px image ≈ 204 ms; 512 px ≈ 317 ms; 2,050 px ≈ 2.8 s. **Fit images down.**
- **Licence:** LFM Open License (LFM1.0).

### 3.4 clef-flash — `thor.local:8096`

Cloudflare's 9B decision model, from `Qwen3.5-9B` plus a small **joint schema head**
(243 MB) that routes evidence to each question and scores all options of all questions
jointly.

- **Why:** the most *confident* answers of any model here — asked to classify a contact sheet
  of 3D renders it said `render` at 0.98 confidence and `is_grid` at 0.94, correctly.
- **The catch, and it is a big one:** locally it is **157 ms for a single question** — 4×
  *slower than the cloud clef-flash* it would replace. Cloudflare runs it on H200/B200 with
  SGLang; a 9B backbone on a Thor's consumer GPU does not compete. "Flash" here means 9B, and
  9B is not flash on this hardware.
- **Use it for:** offline labelling, evals, checking the fast tier against a stronger model.
  **Not** for a real-time loop.
- **The joint head is not a local win.** It amortises the *backbone*, but the joint scoring
  itself costs ~65–90 ms per extra question. Its real advantage shows up on datacenter
  hardware, where the backbone is cheap and the head dominates.
- **Gotcha:** it needs `-ub 4096` (`--ubatch-size`). Its joint head processes the whole prompt
  in one physical batch, so with the default 512 a six-question pack fails with HTTP 500
  *"input (683 tokens) is too large to process"*.
- **Licence:** Apache-2.0 (both the Cloudflare repo and the GGUF conversion).

### 3.5 jev-rack — `thor.local:8077` (perception, not decisions)

A FastAPI service on the Thor serving three different models behind a **card registry**.
Read `GET /cards` — each card says what it predicts, what it is *not* for, and its measured
cost.

| card | endpoint | what | mean latency |
| --- | --- | --- | --- |
| `ijepa-vith14` | `POST /embed/image` (multipart) | 1280-dim image representation, I-JEPA | 162.5 ms over 6,276 calls |
| `vjepa2-vitl-fpc64-256` | `POST /embed/video` (multipart) | 1024-dim video-clip representation | 7,813 ms over 279 calls |
| `jev-semantic / intuition-student-v1` | `POST /judge` (JSON) | ternary neg/zero/pos judgment on short text | 23.1 ms over 4,355 calls |

**These emit representations, not semantics.** I-JEPA cannot label, caption or detect — it
gives you a vector for similarity, clustering, retrieval and change detection. Use it as the
*feedback* half of a loop, feeding a decision model, not as a decision model itself.

**Licence — read this one:** the JEPA weights are Meta's **CC-BY-NC-4.0**, i.e. **non-commercial**.
If the ProArt work is commercial, I-JEPA and V-JEPA2 are the wrong weights.

### 3.6 The one fact to design around

**The models do not agree with each other.** The same question — *"Does this require
immediate rescue?"* over a slipping hiker — came back:

| model | answer |
| --- | --- |
| d1-omni-600M | **0.985** |
| d1-2B | 0.393 |
| d1-3B | 0.183 |
| clef-flash | 0.129 |

Four models, four calibration regimes, one question. Two of them think this is an emergency
and two do not. **Do not average them and do not treat a probability as a truth.** Decide which
one is your policy, set a threshold on *its* confidence, and treat the others as a spread you
are choosing between. There is no accuracy comparison yet against cloud Jev on our question
packs — everything measured so far is latency, shape and cost.

---

## 4. Getting the models yourself

Only if you want a local copy on the ProArt. **They need a recent llama.cpp** (0.8.0-dev or
later) — the `d1` family needs "decision model" support at all, and `d1-2B` needs LFM3.

### 4.1 The weights

| model | repo | file | bytes | sha256 |
| --- | --- | --- | --- | --- |
| d1-omni-600M | `LiquidAI/d1-omni-600M-GGUF` | `d1-omni-600M-Q8_0.gguf` | 407,207,584 | `cd94463f4cac9c700ec6750df8df005bb6846de410e159d1f5e985e1171693d3` |
| | | `mmproj-d1-omni-600M-Q8_0.gguf` | 262,791,232 | `df887978550cdbb45f0f57d7b436a0f844689f9f561395a144ced500606c3a89` |
| d1-2B | `LiquidAI/d1-2B-GGUF` | `d1-2B-Q8_0.gguf` | 1,846,240,576 | `073313d92b53b0ac8193db6e906cb3c78a98183d244b15b7b8f5426115c6738c` |
| | | `mmproj-d1-2B-Q8_0.gguf` | 203,031,584 | `62cc6d74fd1e5b1d6b48518566bf4c0cfd2468079a0c096ea323b74eed01e0d1` |
| d1-3B | `LiquidAI/d1-3B-GGUF` | `d1-3B-Q8_0.gguf` | 2,874,781,280 | `2f0942d5a5f64cf69c3356d2be439b71644b1f0eb3a580912a5a0179eeaba77d` |
| | | `mmproj-d1-3B-Q8_0.gguf` | 583,109,728 | `2505920ce464b7c54acd8b7b74a931c0c671f06d0c8833760b2fbceeb253e92a` |
| clef-flash | `ggml-org/Clef-Flash-GGUF` | `Clef-Flash-Q4_K_M.gguf` | 6,732,781,824 | `eef2968c238a80d49d340430f22410ea3db9934ad1c6fdb233a503cbf9a9b54e` |
| | | `mmproj-Clef-Flash-BF16.gguf` | 921,704,928 | `477598b2425d3aea55fefb895c414e04bda1ee2ec55878d78e850cb8b420d8d6` |

All eight checksums were verified against the Hugging Face metadata on the Thor. The
`mmproj` file is the **multimodal projector** — it carries the vision (and, for omni, audio)
encoder. Without it the model is text-only.

Other quantisations exist on the same repos (`BF16`, `F16`, `Q4_K_M`, `Q5_K_M`); `Q8_0` is
the quality/size point the Thor uses. `d1-omni-600M` has **no** `Q4_K_M`.

### 4.2 Fetch

```bash
mkdir -p ~/models/d1-omni-600M && cd ~/models/d1-omni-600M
for f in d1-omni-600M-Q8_0.gguf mmproj-d1-omni-600M-Q8_0.gguf; do
  curl -L --fail -o "$f" \
    "https://huggingface.co/LiquidAI/d1-omni-600M-GGUF/resolve/main/$f"
done
sha256sum -c <<'EOF'
cd94463f4cac9c700ec6750df8df005bb6846de410e159d1f5e985e1171693d3  d1-omni-600M-Q8_0.gguf
df887978550cdbb45f0f57d7b436a0f844689f9f561395a144ced500606c3a89  mmproj-d1-omni-600M-Q8_0.gguf
EOF
```

### 4.3 Serve

```bash
llama-server \
  -m ~/models/d1-omni-600M/d1-omni-600M-Q8_0.gguf \
  --mmproj ~/models/d1-omni-600M/mmproj-d1-omni-600M-Q8_0.gguf \
  --host 127.0.0.1 --port 8095 \
  -ngl 99 --flash-attn on --ctx-size 8192 -ub 4096
```

`-ub 4096` matters for clef (see §3.4). `--flash-attn on` is wanted on modern GPUs. The
server prints the decision type it detected — `lfm2-d1-omni`, `lfm3-d1`, `lfm2-d1`, `clef` —
which is the fastest way to confirm the right weights loaded.

### 4.4 Where the Thor keeps them

```
~/models/d1-omni-600M/     639 MB   d1-omni-600M-Q8_0.gguf + mmproj-d1-omni-600M-Q8_0.gguf
~/models/d1-2B/           2.0 GB    d1-2B-Q8_0.gguf + mmproj-d1-2B-Q8_0.gguf
~/models/d1-3B/           3.3 GB    d1-3B-Q8_0.gguf + mmproj-d1-3B-Q8_0.gguf
~/models/clef-flash/      7.1 GB    Clef-Flash-Q4_K_M.gguf + mmproj-Clef-Flash-BF16.gguf
~/llama.cpp-upstream/build-up/bin/llama-server    0.8.0-dev, CUDA sm_110
~/llama.cpp/build/bin/llama-server                0.7.0-dev (older; llama-coder30.service depends on it)
~/d1-lab/                                     bench scripts, this file, tier.sh
~/d1-lab/tier.sh status | start <name> | stop <name>
```

The Thor's systemd units are `d1-flash.service`, `d1-2b.service`, `d1-3b.service`,
`clef-flash.service` (user units). **Load one at a time** — three loading simultaneously took
4 minutes each and thrashed the GPU; alone it is 10–20 seconds.

---

## 5. Gotchas, each one measured

1. **Images are priced in pixels.** 320 px ≈ 95 ms on the 600M; 2,050 px ≈ 2.8 s on d1-3B.
   Downscale before sending. The cloud Jev limit (sides up to 1,568 px) is far too slow locally.
2. **`score` answers come with a `legend`** — a map of level index → label. Keep it; without
   it a score of `1.19` is unreadable. The `Answer` schema in Otherhere was dropping it until
   2026-10-28.
3. **`choice` and `score` answers may arrive with no `confidence`.** Derive it from the
   probability distribution (largest normalised share) or the parse fails.
4. **d1 is one forward pass *per question*.** A pack of N questions costs N passes. Only
   clef's joint head is one pass for the whole pack — and on the Thor that does not make it
   faster overall.
5. **The score scale is a probability-weighted level index**, 0-based, matching the criteria
   list order.
6. **No auth, CORS open to every origin.** A browser on the LAN can POST from any page it has
   open. Both are reversible with `--api-key`.
7. **The models disagree** (§3.6). Pick a policy, threshold its confidence, do not average.

---

## 6. A reference client

`Otherhere` (https://github.com/SuperInstance/Otherhere) has a drop-in provider and a Ladder
rung for this — PR **#104**, branch `feat/local-decision-tiers`, open with CI green:

- `packages/models/src/providers/local.ts` — `LocalDecisionProvider`
- `packages/models/src/ladder.ts` — `localRung`, on the reserved `distilled` rung
- `docs/adr/0019-local-decision-tiers.md` — the tier architecture
- `docs/notes/local-decision-models.md` — every measurement, with the honest negatives

```ts
import { LocalDecisionProvider, localRung, Ladder } from '@otherhere/models';

const local = new LocalDecisionProvider({ baseUrl: 'http://thor.local:8095', model: 'd1-omni-600M' });
const ladder = new Ladder([localRung(local), cloudRung], { log: (d) => void logs.push(d) });
// The local rung is asked first and is free; the cloud rung is only reached
// when the local model's confidence is below the threshold.
```

The same shape works in Python — it is just `POST /v1/systemone` with a JSON body.

Casey Digennaro <casey.digennaro@gmail.com>
10:07 PM (0 minutes ago)
to me

https://cdml-lab.github.io/LLMto3D/
https://huggingface.co/blog/daya-shankar/open-source-llm
https://github.com/StarKnightt/night-street
https://github.com/threedle/ll3m