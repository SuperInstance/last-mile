# 02: Wire protocol: should we speak MIDI 2.0?

## What the source proposed
Use MIDI 2.0 Universal MIDI Packets (UMP) as *the* universal format for network, memory, GPU and AI tensors:
- 16 groups × 16 channels = 256 lanes, with Group = entity, Channel = sub-system, Index = axis
- 32-bit values, normalized to 0–1
- MIDI-CI Property Exchange for self-describing endpoints
- delta-threshold broadcasting
- UDP transport

## Verdict: borrow the ideas, not the format

> Read [07-the-midi-principle](07-the-midi-principle.md) first. The protocol's *semantic model* follows MIDI closely: channels per agent, note on/off actions, controllers, clock and transport, meta text. This section is only about the byte encoding and transport.

**Why not UMP as our core protocol:**
- Our payloads are *semantic*: an utterance with text, intent, urgency and addressee; a squad intent; a perception summary; a job request with a deadline. Packing these into 32-bit scalars on (group, channel, index) lanes is a lossy, awkward encoding.
- Normalizing world coordinates to 0–1 requires a fixed world bound. Our world is procedural and unbounded (chunk coordinates plus a local offset).
- 256 lanes is an arbitrary ceiling. The source "solves" this with NVSwitch.
- Being a standard buys nothing here: no third-party MIDI device needs to understand agent intent.

**What to borrow:**
| UMP idea | Our version |
|---|---|
| Fixed-size, typed, little-endian packets | Compact binary messages (e.g. `postcard`/`bitcode` serde), with a message-type byte up front |
| Group/channel addressing | `squad_id` / `agent_id` / `topic` routing. Squads are the natural batching unit (4 small messages per update, not 16) |
| MIDI-CI Property Exchange (discovery) | A **schema/version handshake** on connect: each node announces protocol version, capabilities (models available, budgets) and the topics it publishes |
| Send-on-change threshold | Agents' kinematic state is sent only when dead-reckoning error would exceed a threshold, plus a slow heartbeat |
| Timestamps on every packet | Every message carries the **sim tick** it refers to, plus a send time for latency tracking |

## Dead reckoning and reconciliation (the source's best networking content)
The source's predict / correct / smooth loop is right and is old (SIMNET/DIS from the 1980s, then Quake). For us:
- The Thor sends each agent's *intent and state* (position, velocity, current action) at around 10 Hz.
- The ProArt extrapolates each tick, and when a correction arrives it **blends** toward it over ~100–200 ms instead of snapping.
- The player's own avatar is predicted locally (client-side prediction), so input never waits on the network.

## Transport
- **Real-time state and perception:** UDP-based. The likely choice is QUIC datagrams plus reliable streams in one connection, or plain UDP with a small reliability layer.
- **Jobs and results (System-1 replies, TTS audio, assets):** reliable streams with **deadlines**. The ProArt submits `{job_id, kind, deadline_tick, priority}` and drops results that arrive late.
- **Bandwidth reality:** 16 agents × 10 Hz × ~64 B ≈ 10 KB/s. Even 16 simultaneous Opus voice streams are about 50 KB/s. Bandwidth isn't the constraint; latency jitter is, so use **wired Ethernet**.

## Where real MIDI *does* belong
- **Input edge:** MIDI controllers (faders, knobs, pads) as a physical control surface for the spectator or studio director, through the `midir` crate. Cheap and fun (see 06).
- **Output edge:** optional sonification, exporting a round's events as a `.mid` file so a match "plays" as music. A toy, but nearly free once the event log exists.

## Explicitly not doing
DPDK kernel bypass, GPUDirect RDMA, eBPF/XDP filtering, lock-free parsing with SIMD intrinsics, PTP clocks, Kubernetes. A `crossbeam` channel between the network task and the ECS is enough at our message rates. If profiling ever says otherwise, we revisit.
