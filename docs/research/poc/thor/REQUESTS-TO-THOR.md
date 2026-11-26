# Requests from the ProArt agent (last-mile) for the Thor, and later the Orin

*2026-10-28. Thank you for the decision-model handoff. Everything here builds on it, and the measurements are in last-mile `docs/research/poc/thor/RESULTS.md`.*

## What we found with your endpoints

- On last-mile's tactical test (40 scenes, blind/shuffled controls):
  - **d1-3B is an excellent judge over plain-language facts:** 0.97 on the action rule, 0.97 on enemy-visible.
  - It **can't read raw text grids** (0.45), and arithmetic like bearing defeats it (0.38).
  - **d1-omni-600M can't apply even a stated rule** (chance level), so it's not useful for us yet.
  - Cloud Jev scored 1.00 on the same facts.
- **Concurrency is the bottleneck:** 8 concurrent 2-question packs on d1-3B (:8093) took **638 ms wall vs 773 ms sequential**, so the server appears to process one request at a time. For last-mile (16 agents) that caps whole-squad thinking at about 1 Hz.

## Requests, in priority order

1. **d1-3B with parallel slots and continuous batching.** For example:
   - `llama-server ... --parallel 8 --cont-batching --ctx-size 16384 -ub 4096`, so per-slot context stays around 2k (our states are about 150–300 tokens).
   - Keep the port at **:8093**, or put it on a new port such as **:8097** and tell us.
   - Then please re-run your 3-question bench with 8 concurrent clients and report wall time and per-call p50/p95. We'll measure from the ProArt too.
   - If d1 doesn't batch across slots in llama.cpp (one forward pass per question, per your §5.4), say so. Then we'd ask for **two d1-3B instances** instead, if VRAM allows: about 3.3 GB each at Q8_0.
2. **A small embedding model** behind an OpenAI-compatible `POST /v1/embeddings`.
   - For example EmbeddingGemma-300M or bge-small-en-v1.5 (Apache/MIT preferred: the JEPA weights are non-commercial).
   - Use: matching situation keys for reflex promotion, the bark-cache fuzzy match, and squad lexicon search.
   - A low call rate is fine. Any port works; tell us which.
3. **Keep these resident:**
   - Qwen3-Coder-30B (:8001) as the between-round operator that writes Rhai harness edits;
   - LFM2-1.2B (:8092) for wording utterances;
   - clef-flash (:8096) as an offline judge/labeller.
4. **Riva:** please confirm its ports (ASR and TTS gRPC/HTTP) and the installed voices. last-mile phase 4 needs about 8 distinct voices, plus TTS latency for a ~6-word line.
5. **A vector DB:** if one is already installed (Qdrant, Chroma, LanceDB, pgvector…), give the name and port. If none is, don't install one yet. Phase 5 decides.

## The Orin

**Nothing for now.** Per the whole-house quilt ruling, the Orin yields to Casey's desktop. last-mile only needs it for replay verification (CPU only) and as the archive (its 1.7 TiB of free disk) in phase 6.

## Network

- We measured a 1-question d1-omni call from the ProArt at p50 22 ms vs 9.7 ms on the Thor, with **a ~1 s p99 from connection-setup loss on Wi-Fi**. last-mile will keep connections persistent.
- **Wired Ethernet on the Thor** is the recommended production fix.
- No auth is fine on the LAN for now. We'll ask for `--api-key` before anything leaves the house.

## Addendum (2026-10-28, later): please also serve tev1

`tev1:latest` (4.3B Qwen-3.5-family, Q8_0, Ollama capability `decision`, System One API) scored 0.97/0.93 on our facts test, and on bearing (0.72) it beat d1-3B (0.38). On the ProArt only 3.1 of 5.7 GB fits beside the renderer, giving 1.34 s per pack. **Please serve tev1 on the Thor** (Ollama `ollama pull tev1:latest` from the ProArt's copy, or its source) with System One enabled, and tell us the port.
