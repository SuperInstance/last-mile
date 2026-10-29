# 04: Hardware reality check

The source drifted to datacenter hardware (H100, Mellanox, NVLink) and then came back to Thor and the ProArt. Here is what holds for *our* machines.

## Jetson AGX Thor
| Source claim | Assessment |
|---|---|
| CPU and GPU share LPDDR5X memory coherently, so no `cudaMemcpy` is needed | **True and useful.** Perception buffers, embedding batches and model I/O can live in shared memory. Design Thor services to keep data resident instead of shuttling it between processes. |
| Hand-written NEON/SVE2 intrinsics for packet parsing | Correct that hand assembly is pointless (compilers emit `UBFX` etc.), but **intrinsics are also pointless here.** Parsing ~1,000 messages/s is nothing. |
| A persistent CUDA kernel polling network memory | **No.** That design is for fixed-function, line-rate pipelines. Our GPU work is model inference, which TensorRT/TensorRT-LLM/vLLM/NIM schedule far better than a hand-rolled loop. |
| A Triton custom backend taking raw pointers | **No.** Use the NIM/Triton APIs as intended. If two Thor services must share tensors, Triton's CUDA shared-memory feature already exists. |

**Where Thor's leverage actually is:**
1. **Keep models resident.** Load the small LLM, perception model, Riva, embeddings and vector DB once. 128 GB of unified memory lets them all stay loaded at the same time.
2. **Batch per squad tick.** One System-1 call per squad per decision tick, with all four agents' context in it, beats 16 separate calls. Continuous batching in vLLM or TensorRT-LLM does the rest.
3. **A budget scheduler** in front of the models: a token/compute budget per tick, priority by urgency, with deadline-missed requests falling back to reflex. *This* is the real-time engineering that matters.
4. **Nsight Systems** on the Thor to see GPU timelines across the LLM, Riva and perception, which shows whether they're fighting each other.

## ProArt (RTX 4050 6 GB, Ryzen AI 9 HX)
| Topic | Note |
|---|---|
| "144 Hz+ uninhibited display loop" | Fine as a target, but on a laptop a frame cap plus pacing beats uncapped rendering (heat, then throttling). |
| Dead-reckoning projection | Correct role for the ProArt (see 02). |
| GPU selection | Force wgpu to `HighPerformance` so Bevy doesn't land on the Radeon iGPU. |
| Mixed Zen 5 / Zen 5c cores | Pin sim and render threads to the full Zen 5 cores. |
| NPU | Small side jobs later (push-to-talk VAD, maybe local barks or classification). Not on the critical path. |
| WSL2 | Run the game natively on Windows. WSL is for tools. Use mirrored networking if WSL services must reach the Thor. |

## Orin Nano Super (8 GB, own NVMe)
Not mentioned in the source. Proposed role, consistent with earlier discussion: **asset librarian and background worker**. It would host the content-addressed asset cache, bake and decimate generated assets, archive round logs, and run the **headless fast-forward sim** for replays and A/B (see 01, 05). It keeps the Thor free for real-time inference.

## Network
- The Thor's high-speed QSFP port is real, but the ProArt can't use it. Wired 1–5 GbE is far more than our traffic (about 60 KB/s) needs.
- **What matters is jitter.** Use wired, not Wi-Fi, and keep the Thor and ProArt on the same switch.

## Datacenter items: explicitly out of scope
H100s, NVLink/NVSwitch/NVSHMEM, GPUDirect RDMA, DPDK, BlueField DPUs, AES-GCM line-rate encryption, PTP grandmasters, Kubernetes, Prometheus/Grafana stacks. If the project ever outgrows three boxes, revisit then.
