# The Thor's service index (2026-10-29)

The Thor's own agent publishes what it serves at `http://thor.local:8100/index.json` (format `otherhere.thor-index.v1`). `thor.local` does not resolve from WSL, so use `http://192.168.0.78:8100/index.json`. A snapshot is in [thor-index-2026-10-29.json](thor-index-2026-10-29.json); fetch the live one before relying on a port.

## Services

| Port | Service | Call | Its own note |
|---|---|---|---|
| 8094 | d1-2B | `POST /v1/systemone` | "the reflex tier: text decisions, 20-105 ms". Our default fast tier (D62) |
| 8093 | d1-3B | `POST /v1/systemone` | "slower than d1-2B and no better here". This matches decision-daw x3/x4 |
| 8095 | d1-omni-600M | `POST /v1/systemone` | "triage gate only: fastest, weak on photographs" |
| 8096 | clef-flash | `POST /v1/systemone` | "the accuracy champion: 100% on text packs, too slow for a per-tick reflex" |
| 8097 | decision tier, OpenAI-shaped | `POST /v1/chat/completions {model, messages}` | the four decision models behind one chat API |
| 8098 | capture service | `GET /places/registry.json` | every asset the Thor can make, content-addressed (byte count + sha256) |
| 8099 | geometry service | `POST /geometry` (image in) | MoGe ViT-L: a photograph -> depth + intrinsics, 72 ms warm |
| 8001 | coder | `POST /v1/chat/completions` | Qwen3-Coder-30B-A3B, 82 tok/s (the pi seat) |
| 8077 | jev-rack | `POST /v1/systemone` | perception, not decisions: I-JEPA, V-JEPA 2 |

Assets served now: lincoln-street-relief, sitka-dock-boat, sitka-oconnell-bridge, smoke-dock, st-michaels-spar3d (cards + GLB models). Tools: `depth-place` (one photo -> a relief mesh, 3.3 s), `finish-asset` (any mesh -> under 192,000 B), `publish-model`, `asset-capture`.

## What this means for last-mile

1. **Our wording realizer points at nothing.** `UterConfig` defaults to `utter.llm_port = 8092` and the model `LFM2-1.2B`, and the index has neither. Agent lines therefore always fall back to the phrasebook. Either point the realizer at a served chat endpoint or leave it on the phrasebook.
2. **clef-flash is a LAN candidate for the slow tier.** It's 100% on text packs but too slow per tick, which is exactly what the "deep" tier wants. It's free and local, where today the deep tier is the cloud Jev. Measure it with the live lane and the decision-daw harness before switching.
3. **The capture service serves real place assets** (GLB, content-addressed and hash-checked), the same shape as lm-archive's asset bank and D46's content addressing. That's the path for photographed places to enter the 3D view after M1.
4. The index confirms the ports in `AgentsConfig`'s defaults (8093-8096).
