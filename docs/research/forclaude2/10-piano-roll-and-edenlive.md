# 10: The piano roll is the game; rendering is the subwoofer

## The principle (your framing, 2026-10-28)
> MIDI holds everything the song needs in order to function. A better renderer renders the last mile better: a subwoofer makes the sound richer, but it doesn't change the rhythm. An AI can play a game from a live spreadsheet of variables. A DAW professional doesn't need to hear the sound to see the music in the piano roll. The AI doesn't need to see the rendering to see the gameplay, as long as it understands the game's physics.

**Consequences for the design:**
1. **The game *is* the score**: the deterministic sim state plus the event stream (07, 08). Everything that affects outcomes lives there.
2. **Agents perceive the piano roll, never pixels.** Each agent gets *its own view* of the score, filtered by the game's physics: what it can see (line of sight, range, a light/visibility value), what it can hear (utterance scores that reach it, 09), and its own body state. That filtered view is the agent's "screen".
3. **Speakers and screens are for humans only.** Sound, voices and visuals are last-mile renderings at whatever fidelity the device allows. A better GPU or a subwoofer improves the experience without changing the game.
4. **Physics must be in the score.** If something matters to play, such as camouflage, darkness or noise, it needs a *number* in the sim (a visibility value, a noise level), not just a look on screen.

**Retro precedent: Thief: The Dark Project (1998).** Its guards never "saw" pixels. They read a computed light value at the player's position (the light gem) and sound values propagated through a room graph. Stealth worked *because* perception was numbers in the sim. The renderer just showed the human what those numbers meant.

## EdenLive (VAST / Tripo AI, Oct 2026): the same split, from the rendering side

Primary source: `github.com/VAST-AI-Research/EdenLive` README (Apache-2.0 code; weights mostly Apache-2.0, some components under other licenses).

**What it is:** a real-time *generative renderer*. In their words: *"we decouple state evolution (maintained by coding agents or engines) and appearance (rendered by EdenLive)."*
- An upstream program (their demos use three.js greybox games) runs the rules and state and renders a crude **proxy video**.
- EdenLive (a video diffusion model built on Wan 2.1/2.2, in 5B and 14B versions) turns the proxy into detailed frames, guided by a **target first frame** plus a **static prompt plus timestamped dynamic prompts**.
- There's also EdenGolem, a skill that turns a text prompt into a coarse playable game, built using the Claude CLI.

**How it validates us:** this is exactly "a simpler underlying game, rendered better for humans than for the AI". Their timestamped `dynamicPrompts` are lyric events for *appearance*, the same shape as our utterance scores.

**The hardware reality (from their README):**
| Setup | Claim |
|---|---|
| Verified environment | Linux x86-64, **80 GB** NVIDIA GPUs, CUDA 13; default profile needs SM90 kernels (FlashAttention 4) |
| Consumer real-time | 5B model at about **24.7 fps, 1280×704 on an RTX 5090**, via LightX2V (quantization, CUDA graphs, etc.) |
| Weights | about 210 GB total for all variants |

- **RTX 4050 (6 GB):** not feasible.
- **Thor (128 GB unified memory, Blackwell, ARM):** the model fits in memory, but Thor has much less memory bandwidth and compute than a 5090, and the stack is only verified on x86 with SM90 kernels. **Real-time 24 fps on the Thor is unlikely.** Low-res, low-fps or offline rendering is *plausible but unverified*. It needs a spike.

## How EdenLive fits last-mile
| Use | Feasibility | When |
|---|---|---|
| **Design our renderer to emit a clean proxy.** Flat colors per material class, clear silhouettes, stable camera, optional depth and segmentation. Our retro-modern look already is a good proxy. | Free, and a design rule | M1 |
| **Neural "director's cut" replays.** After a round, the Thor (or a cloud GPU) re-renders the spectator replay's proxy into a stylized highlight reel, a few minutes per clip. Fits the between-round screen and Studio mode. | Plausible; needs the spike | M2–M3 |
| **Live neural rendering tier** on a future 5090-class GPU or cloud stream | Real per their claims, but not on our hardware today | Future "subwoofer" |

The score (sim plus events) stays the same across all three. Only the last mile changes.

## Open items
- Spike: EdenLive 5B few-step on the Thor (aarch64, Blackwell SM, LightX2V or plain PyTorch): does it install at all, and at what fps/resolution?
- The tech report (`assets.vast-eden.ai/assets/edenlive/EdenLive-tech-report.pdf`) hasn't been read yet. Read it before the spike.
