# 08: MIDI Maze (1987) lessons, checked against the source

Sources read:
- `sarnau/AtariST-MIDIMaze-Source`: the original game, reverse-compiled to C. Files read: `maingame.c`, `midicomm.c`, `README/MIDICommunication.md`, `README/MIDImazeEngineOverview.md`.
- `diegoparrilla/midi-maze-js`: a browser port that is wire-compatible with real STs. Read: `README.md`, `docs/epics/DECISIONS.md`.
- `forclaude3.md` was used only as a pointer. **Several of its claims don't match the code** (see the end of this file).

## What MIDI Maze actually did

**In gameplay, only one byte per player per tick goes over the wire: the joystick bitmask** (left/right/up/down/fire). From `maingame.c`:
```c
/* forward all joystick/key presses to all other machines in the MIDI ring */
do {
    Bconout(MIDI, player_joy_table[i]);
    if(--i < 0) i = machines_online-1;
    player_joy_table[i] = get_midi(30);
} while(i != own_number);
```
Everything else (positions, shots, hits, kills, respawns) is **recomputed identically on every machine** from three things:
1. the shared maze (sent once at setup),
2. a shared random seed (two bytes, sent once at setup),
3. the shared stream of joystick bytes.

The midi-maze-js port states it plainly (decision D-02): *"Only the 1-byte joystick state per player per tick travels the wire."*

This is the MIDI principle in its purest form. The stream carries only **decisions**, and every machine is the "instrument" that renders the full world from them.

## Ten lessons, mapped to last-mile

| # | MIDI Maze fact | Lesson for us |
|---|---|---|
| 1 | Only inputs travel; state is recomputed everywhere | **Our round stream should carry decisions, not state.** Human inputs, Thor System-1 decisions and cloud System-2 decisions are all "joystick bytes" into a deterministic sim. The log *is* the inputs, and replay is exact. |
| 2 | **Drones cost zero bandwidth.** `drone_action()` runs on every machine, driven by the shared RNG and joystick table | Our **reflex layer** should be deterministic sim code that runs identically on every node. Only non-deterministic decisions (model outputs) need to cross the wire. |
| 3 | The shared RNG is used for gameplay. MIDIcam uses a **separate, non-shared** RNG (`XBIOS Random()`) for cosmetic choices | Confirms the split in 07: **authoritative sim** (shared seed) vs **cosmetic rendering** (local randomness allowed). |
| 4 | **MIDIcam**: a passive node in the ring that echoes bytes, renders everything locally, and auto-follows players | Our spectator and drone-cam mode, 37 years early. A spectator is just another renderer consuming the decision stream. |
| 5 | Each tick, a **different player is processed first** in `move_player()` so low player numbers don't always win ties | Fairness rule for our sim: rotate squad and agent processing order per tick. |
| 6 | **No floating point.** Positions in 1/256-cell units, angles as 0–255, sin/cos from a 65-entry table | **Critical for us.** The ProArt is x86 while the Thor and Orin are ARM. Float results can differ across them (FMA, libm), which would desync replays and headless re-sims. The authoritative sim core uses **fixed-point integer math** (or a strictly deterministic float subset). Rendering can use whatever it likes. |
| 7 | Lockstep's flaw: every tick **blocks** on the neighbor's byte. One slow or dropped node freezes or kills the game (`get_midi` timeout, midi-maze-js constraint C-01) | **Never put model inference in the lockstep path.** AI decisions are **commands scheduled for a future tick** (the RTS trick from Age of Empires: commands execute 2–3 turns ahead). If the Thor misses the deadline, the reflex default executes. The sim never waits. |
| 8 | midi-maze-js adds **golden-master tests** (bit-exact traces from the original C) and a **per-tick checksum** to detect desync | Adopt both: a periodic state hash in the log and on every node, plus golden-replay tests in CI (`replay(log) == recorded hashes`). |
| 9 | Setup sends the maze, settings and seed **once**. Membership is **frozen per game** and changes only between games (constraint C-04) | Matches our round structure. Round-start packet = `{seed, world params, ruleset/script hashes, roster}`. Mid-round joins happen only as spectators. |
| 10 | Humans and drones share one input table (`player_joy_table[]`) | **Humans and agents share one decision interface.** The human can *possess* or *release* any squad member mid-round, and the stream doesn't change shape. This fits "human doesn't have to be the leader". |

## Where it doesn't fit (bones)
- **The ring topology.** A token ring was forced by MIDI's in/out ports. Our LAN is a star with the ProArt as host, which is simpler and has no "one loose cable kills all" failure.
- **Pure lockstep for everything.** It works for 1-byte joysticks at 30-ish Hz on identical machines. Our decision sources have wildly different latencies (reflex in µs, Thor in 100s of ms, cloud in seconds), so we use **lockstep for the sim plus scheduled future commands for AI** (lesson 7).
- **Uncompressed setup** (4096 maze bytes). Irrelevant at our bandwidth, but the principle stands: our world is a seed, never a mesh.

## forclaude3.md corrections
- *"Fixed 4-byte packets: command, player ID, X/Y coordinates."* **Not what the code does.** Gameplay traffic is 1 joystick byte per player. Coordinates never travel.
- *"The master injects a constant stream of ticks."* No. Lockstep emerges from every node blocking until it has received everyone's byte.
- *"If the network slowed down, the game slowed smoothly."* Partly. It stalls, and past the timeout it **fails**.
- Faceball 2000 link details, the PC game-port MIDI hack, "aftertouch for positional audio", and "ghost bots on channels 2–4" are **unverified**. Treat them as folklore. (The ghost-bot idea is still a good one; see lesson 10.)
- **PLATO:** shared "common blocks" (TUTOR) and **redefinable terminal character sets** are real. The character-set trick is the MIDI principle again: define the glyph once, then send 1-byte codes. That's exactly our asset pipeline: the Thor generates an asset once, it gets a content-addressed ID, and from then on only the ID travels (a "program change").
- The CUDA/PTX/RDMA section is out of scope (see 04).

## The common thread across three eras
| Era | Define once | Then send tiny codes |
|---|---|---|
| PLATO (1970s) | Character-set glyphs | 1-byte characters |
| MIDI and MIDI Maze (1980s) | Instrument patch, maze plus seed | Notes, joystick bytes |
| last-mile | Assets, voices, agent roles, world seed, General Action Set | Decisions, actions, utterance scores |
