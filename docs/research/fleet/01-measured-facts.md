# Measured facts about our machines

*Source: [whole-house-quilt `docs/stage-0/`](https://github.com/SuperInstance/whole-house-quilt/tree/main/docs/stage-0), measured on the metal 2026-10-28 between 01:58 and 02:15 UTC. Nothing was installed or reconfigured. These are a snapshot. The 24 h samplers write `~/whq-stage0/weather-24h.csv` on each box, and later estimates should cite those files. Casey's Thor spec sheet supersedes the service list when it arrives.*

## LAN weather (ICMP, ms)

| Path | p50 | p95 | p99 | max |
|---|---|---|---|---|
| ProArt → Thor | 5.4 | 42.9 | 84.2 | 101 |
| ProArt → Orin | 5.0 | 6.7 | 18.3 | 93 |
| Thor → Orin | 4.5 | 41.1 | 80.0 | 85 |
| Orin → Thor | 4.3 | 51.8 | 92.0 | 106 |

- Loss was 0% on every path.
- **Any path that touches the Thor has an 80–112 ms tail at p99.** The Thor's Wi-Fi radio (−62 dBm, the weakest in the house) is the cause.
- Throughput: ProArt → Orin ~192 Mb/s, ProArt → Thor ~92 Mb/s, Thor → Orin 61–94 Mb/s.

**What this means for last-mile:**
- The decision stream is ~60 KB/s (research/04), so bandwidth isn't a concern. The **tail** is what matters.
- An agent decision travels one way, Thor → ProArt. At p99 that's about 42 ms, half the round trip. The default 3-tick lead at 30 Hz (100 ms) covers p99, but not the worst spikes. **The lead becomes adaptive (D34):** lead = measured p99 one-way + margin, clamped to a parameter range.
- The stale window (15 ticks = 500 ms) is far outside every measured spike, which is good.
- **Hardware recommendation:** put the Thor on wired Ethernet. It's the one change that removes the storm source. Until then, budgets on Thor paths must come from tails, never medians.
- **NAT:** the ProArt's WSL2 sits behind NAT, and nothing on the LAN can open a connection *into* WSL. The native Windows `lm-host` can accept inbound connections only if the Windows firewall allows it. **Default: the ProArt dials out** (QUIC client) to the Thor and the Orin. Connection direction is independent of authority (D34).

## The Thor, live (resident services, all HTTP 200)

| Port | Service | last-mile use |
|---|---|---|
| 8001 | llama-server: **Qwen3-Coder-30B-A3B-Instruct** (~82 tok/s) | **Between rounds:** applying harness edits and writing Rhai (operator tier, D25/D27). Not in the round's reaction path |
| 8092 | llama-server: **LFM2-1.2B** (liquid) | **System-1 candidate (fast)** for members' think requests |
| 11434 | Ollama: `nemotron-3.5-lightning`, `qwen3.5:9b` | **System-1 candidates (heavier):** leader planning, squad summaries |
| 8077 | JEV rack: I-JEPA, V-JEPA 2, a judge | Later: a judge for "did the order work" labels, and perception spikes |
| 8188 / 8190 / 8200 | ComfyUI / image portal / **Foundry** (parametric 3D asset factory: seed + params *are* the asset) | Prerender-ahead (M2): textures, vistas, archetype props |
| 18789 | NemoClaw gateway (Docker) | Not used by last-mile |
| 2206 / 8788 | Hub forwards: ProArt SSH / "otherhere" | **Port 8788 is taken.** last-mile picks its own ports (`net.*` parameters) |

- Not in that probe list: **Riva** and a vector DB. Casey reports Riva is ready on the Thor, but Stage 0 didn't probe for it. Confirm the Riva port and the vector DB choice from the spec sheet.
- **The GPU is multi-tenant even when "idle":** about 54.5 GB is held by the 30B, ComfyUI, the JEV rack and the 1.2B. RAM: 122 GiB total, about 42 GiB available during the audit.
  - Consequence: last-mile's System-1 runs **beside** other tenants, so the least-use-first queue must expect variable latency. This is one more reason for "never wait".
  - During a round, last-mile think requests belong in the quilt's *reaction/conversation* class, which preempts *production* work like renders and Foundry runs. See [03](03-whole-house-quilt-fit.md).
- **Disk:** the NVMe has 184 GiB free (80% used). There's also a Toshiba USB shelf with 1.6 TiB free. Agent harness repos and vector DB namespaces are small; generated assets go to the Orin.
- Seats on the Thor: ZCode, MiniMax Code and **Kimi Code** (the only Kimi seat).

## The Orin

- 6 cores, **7 GiB RAM** (about 3 GiB free), **NVMe 1.9 TiB with 1.7 TiB free**, the emptiest disk in the fleet.
- Its standing tenant is Casey's desktop and Earth-survey work. The quilt's ruling is that the **Orin yields when Casey sits down** and holds nothing canonical. That fits last-mile's plan for it: the archive copy, replay verification and the asset bank are all **replicas**, and the ProArt keeps the authoritative log (D18).
- Verification replays should run at low priority (`nice`) so they never contend with the desktop.

## The ProArt

- 24 threads, **15 GiB RAM visible to WSL**, 871 GiB free disk, RTX 4050 6 GiB.
- In WSL, `nvidia-smi` is at `/usr/lib/wsl/lib/nvidia-smi`.
- **A local tier already exists:** a 0.6B model at ~230 tok/s, measured by the quilt adjudication. It's a candidate for **ProArt-local System-1**. When the Thor is unreachable, agents could keep a thin fast-thinking tier instead of dropping straight to reflexes. Make it a parameter: `agent.s1_fallback = reflex | proart_local`.
- Casey has a standing stop order on `proart-gpu-resident`, `browser-capture` and `proart-portal` (inactive since 2026-10-25). last-mile doesn't start them. Any resident GPU model on the ProArt is opt-in.

## Subscriptions

| Pool | Access point |
|---|---|
| Z.AI (GLM) | ZCode seats (all boxes) |
| MiniMax | MiniMax Code seats (all boxes) |
| Kimi | Kimi Code on the Thor |
| Claude | Claude Code on the ProArt |

- **No CLI reports remaining quota.** The Claude seat has hit its session limit in practice, so pool exhaustion is a real, observed event.
- `lm-router` has to meter by observation: count tokens and requests itself, and treat a 429 or "limit" error as a cold pool. It can't rely on querying a balance.
