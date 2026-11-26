# Digital Extinction: what a Bevy RTS teaches about looking and controlling like a game

**Status:** research. No code changes.
**Date:** 2026-10-30
**For:** Casey, and the sandbox slice in `docs/design/sandbox-on-otherhere.md`
**Subject:** [github.com/DigitalExtinction/Game](https://github.com/DigitalExtinction/Game) — an
original, open-source 3D RTS in Rust on Bevy, archived September 2024, **AGPL-3.0**.

## What this document is

last-mile's weak spot is watchability: a match reads as boxes on grey blocks
(`docs/notes/host-3d-both.png`, `docs/notes/host-3d-seed1.png`). Digital
Extinction is an AGPL-licensed Bevy RTS that shipped a camera, a selection
model, a minimap and combat feedback — the exact machinery that makes an RTS
*feel* like a game rather than a diagram. This document reads that machinery
and asks, for each piece, what last-mile could re-implement on its own
fixed-point sim and Bevy host.

**The licence posture, stated once and obeyed throughout.** Digital Extinction
is AGPL-3.0 (`gh api repos/DigitalExtinction/Game` reports `license.spdx_id:
"AGPL-3.0"`). Copying its code into last-mile would bind last-mile. So:

* everything here was read **only through the GitHub API** (`gh api
  repos/DigitalExtinction/Game/contents/<path>` and the commits endpoints);
  the repository was never cloned, never built and never run;
* **no code is reproduced.** Every mechanism below is described in our own
  words, with the file path of the file it was read from, so that the idea can
  be re-implemented from scratch against our own architecture. A file path is
  a citation, not a licence to copy;
* where Digital Extinction's approach conflicts with our own invariants, the
  conflict is named rather than smoothed over.

**Every claim carries a path.** A path like `crates/camera/src/camera.rs` is
the file in the Digital Extinction repo; a path like
`crates/lm-core/src/world/path.rs` is a file in *this* repo. Where a claim is
about our side, the path is ours; where it is about theirs, it is theirs.

## How to read the citations

| Prefix | Repository |
| --- | --- |
| *(none)* | this repo, last-mile — the sim, the host, the design docs |
| `crates/…` (no `lm-` prefix) | `github.com/DigitalExtinction/Game` — the subject of this document |

## The three invariants this document is written against

These are last-mile's, and they are the filter every idea below passes
through.

1. **Only decisions travel.** The sim is the source of truth; the host is a
   renderer; what crosses a machine boundary is a decision, not a state dump
   (`docs/design/decisions.md:16`, D10; proven on hardware at
   `docs/design/decisions.md:83`, D69).
2. **Fixed-point determinism.** No floats in the core; the same seed and the
   same decisions produce the same bytes on x86-64 and aarch64
   (`crates/lm-core/src/sim.rs:34-36`).
3. **Agents perceive the score, never pixels.** A mind sees a numeric view of
   the world, not a render (`docs/design/decisions.md:19`, D13).

An idea that needs the renderer to be authoritative, or that needs float time
on the wire, is not a candidate. It is a warning.

## The last-mile we are re-implementing for

Before the RTS's machinery, the surface it has to land on.

**The sim (`crates/lm-core`).** A `Sim` owns a generated world, a roster of
agents, per-squad knowledge, a ruleset and a reflex runtime; the host drives
it one tick at a time through `Sim::step` (`crates/lm-core/src/sim.rs:1-7`).
Each tick runs five fixed phases — decisions, knowledge, agents, rules, hash —
and that order *is* the replay contract (`crates/lm-core/src/sim.rs:9-27`).
Nothing in the sim reads the clock, the environment, or any random source not
derived from the round seed (`crates/lm-core/src/sim.rs:34-36`).

* **World.** A coarse navigation grid: 2 m cells by default
  (`crates/lm-core/src/params.rs:77`), held in a flat row-major array of three
  cell kinds — open floor, wall (blocks movement *and* sight), cover (blocks
  sight only) — so a world serialises and hashes canonically on every machine
  (`crates/lm-core/src/world/grid.rs:1-6`, `:11-41`).
* **Pathing.** Integer Jump Point Search, 8-connected, strict no corner
  cutting, octile costs, with a total `(f, h, y, x)` tie-break so the same
  map always yields the same path (`crates/lm-core/src/world/path.rs:1-28`).
  The motor caches the route on the agent and re-plans when the goal changes
  (`crates/lm-core/src/agent.rs:86-87`).
* **Knowledge.** `SquadKnowledge` is a per-squad bitset of explored cells
  plus a memory of where each enemy was last seen and what the squad was
  *told*; it only ever grows (`crates/lm-core/src/view.rs:54-62`, `:106-126`).
* **The view.** `ScoreView` is the world as one agent's squad knows it: the
  squad in id order, enemies as aged memories, beacons with their holder, the
  score, and the lines heard since the last view update
  (`crates/lm-core/src/view.rs:224-284`).
* **Rules and events.** The ruleset scores the tick and *reports its events*
  (`crates/lm-core/src/sim.rs:25-26`). The event vocabulary already contains
  the four facts a watchable game needs: `Tagged { by, target }`,
  `Respawned`, `BeaconHeld { beacon, squad }` and `BeaconLost`
  (`crates/lm-core/src/rules/mod.rs:62-88`). The tag itself is decided by the
  rules, not by the view: an agent tags the first unstunned enemy within
  `tag_range_units` that is in line of sight, and the target is stunned for
  `stun_ticks` (`assets/rules/beacons.rhai:77-122`).
* **State.** An agent is pure state: id, squad, role, fixed-point position,
  fixed-point facing, the tick its stun ends on, a held action, a committed
  intent, one lane conditioner per gate, the chosen menu option, the cached
  route, and its spent airtime (`crates/lm-core/src/agent.rs:67-96`).

**The host (`crates/lm-host`).** Bevy `=0.20.1` with `bevy_egui =0.43.1`,
pinned exactly (`crates/lm-host/Cargo.toml:18-23`). It draws the same round
twice: a 2D overhead map and a 3D view, and the 3D view is forbidden from
inventing a second fog (`crates/lm-render/src/fog.rs:1-14`).

* **Camera.** A single `CameraRig` — a focus point in world metres, a zoom in
  pixels per metre, and an optional agent to follow
  (`crates/lm-host/src/map_plugin.rs:248-257`). The camera is free only when
  nobody is being played or watched; otherwise it follows
  (`crates/lm-host/src/input_plugin.rs:229-230`).
* **Input.** A click on the map is a cell to walk to; a drag is the camera;
  the wheel is the zoom; `WASD` pans (`crates/lm-host/src/input_plugin.rs:16-20`,
  `:196-266`). There is **no selection, no box-select and no right-click
  order menu** today — the click is the order
  (`crates/lm-host/src/input_plugin.rs:213-225`).
* **Units.** One shared low-poly figure mesh and **one material per squad**,
  so the engine instances the whole roster on the GPU; what makes a figure its
  own is its `Transform`, set every frame from the lerped position and facing
  (`crates/lm-host/src/view3d/agents.rs:4-14`, `:200-217`).
* **Fog.** One rule, two views: a spectator sees the round as the authority
  does, a possessing player sees only what their squad's `ScoreView` vouches
  for (`crates/lm-render/src/fog.rs:25-44`).
* **Palette.** Four squad colours, five material classes, and a leader rim
  (`crates/lm-render/src/palette.rs:176-203`).
* **Audio.** The host's audio is **the human's voice and the agents' talk
  only** — microphone, ASR, prosody, and the agent clips. There is no
  gameplay sound effect at all (`crates/lm-host/src/audio_plugin.rs:1-26`).

**The gap this document is about.** last-mile already has a deterministic
sim, a fog rule, a palette and a top-down map. It has no camera beyond pan and
zoom, no selection model, no health or damage readout, no minimap, no
projectiles, no hit feedback, and no sound that describes the game. A tagged
Dab simply stops moving for three seconds
(`crates/lm-core/src/params.rs:102`; `assets/rules/beacons.rhai:117-118`) and
nothing on screen says why.

---

## 1. Architecture: what Digital Extinction is made of

### 1.1 The crate map

Digital Extinction is one binary (the `de` binary in the `de_game` package,
`Cargo.toml:1-17`) over a workspace of **32 crates**
(`members = ["crates/*"]`, `Cargo.toml:67`), each crate named after the concern
it owns (`Cargo.toml:82-111`).

| Group | Crates | What lives there |
| --- | --- | --- |
| Simulation core | `core`, `types`, `conf`, `uom`, `index` | the schedule order, the game state, shared enums, tunables, float units of measure, the spatial index |
| World | `terrain`, `map`, `pathing`, `movement`, `objects`, `construction`, `spawner`, `energy`, `behaviour` | the heightfield and its colliders, map loading, the pathfinder, the motor, footprints, building placement, spawning, power, behaviour |
| Presentation | `camera`, `controller`, `signs`, `gui`, `menu`, `audio` | the camera, input/selection/commands/HUD/minimap, the 3D world-space signs, the UI, the front end, sound |
| Network | `net`, `messages`, `multiplayer`, `connector`, `lobby`, `lobby_client`, `lobby_model` | the transport, the wire types, the client plugin, the game server, the HTTP lobby server, its client, its shared model |
| Support | `loader`, `log`, `tools`, `test_utils` | asset loading, tracing, CLI tools, test helpers |

The split that matters for us: **`core` owns no rendering at all**, and the
presentation crates read the world rather than write it.

### 1.2 Bevy 0.13 and a hand-extended schedule

* **Bevy 0.13**, pulled with `default-features = false` and an explicit
  feature list (`Cargo.toml:165-194`); audio is `bevy_kira_audio 0.19`
  (`Cargo.toml:122`). Our host is on Bevy `=0.20.1`
  (`crates/lm-host/Cargo.toml:22`), so DE's schedule idioms are old, but its
  *shape* is current.
* The schedule is not Bevy's stock one. `GameSchedulesPlugin` inserts four
  custom labels into the `MainScheduleOrder`:
  `First → InputSchedule → PreMovement → Movement → PostMovement`
  (`crates/core/src/schedule.rs:28-35`; the labels at `:37-59`). The contract
  is explicit: **all** game-active `Transform` changes happen in `Movement`
  and in no other stage (`crates/core/src/schedule.rs:47-55`), and
  `auto_insert_apply_deferred` is switched off so the scheduler cannot
  silently rewrite the ordering (`crates/core/src/schedule.rs:18-21`).
* Every crate is a Bevy `Plugin` or `PluginGroup` the binary assembles
  (`crates/camera/src/lib.rs:15-22`, `crates/combat/src/lib.rs:17-27`,
  `crates/terrain/src/lib.rs:16-23`, `crates/signs/src/lib.rs:22-31`,
  `crates/audio/src/lib.rs:9-16`). The combat group is ordered
  Laser → Attack → Trail → Health (`crates/combat/src/lib.rs:21-25`), and its
  systems are split into three sets — Attack, Charge, Fire
  (`crates/combat/src/lib.rs:29-34`) — so a shot resolves in a known order.

### 1.3 The determinism answer: it has none, and it does not need one

The brief asks whether DE keeps its sim deterministic. **It does not**, and the
reason is architectural rather than accidental:

1. **The simulation is float.** `glam 0.25` and `nalgebra 0.32`
   (`Cargo.toml:137-138`) carry positions, and `de_uom` types even the camera
   constants as floats (`Quantity::new_unchecked(2.0)`,
   `crates/camera/src/camera.rs:23`). `crates/combat/src/health.rs:50-54` panics
   if a health delta "is not finite" — an invariant only a float world has.
2. **There is no round seed and no replay.** Searching the ~120 source files
   I read for `deterministic | replay | seed | record` returns exactly one hit
   in gameplay code, and it is a tie-break *approximation* rather than a
   guarantee: *"Make it more deterministic, objects with smaller coordinates…"*
   (`crates/combat/src/attack.rs:227`). The only `Record` in the tree is a
   network delivery record (`crates/net/src/record.rs:6-11`), not a round log.
3. **The PRNG is deliberately per-client.** The multiplayer sync timer jitters
   by `fastrand::u64(0..250)` ms (`crates/movement/src/syncing.rs:49`; the
   constants at `:16-17`); `fastrand` is in the workspace at `Cargo.toml:130`.
4. **Multiplayer syncs *state*, not decisions.** Each client simulates its
   own entities and periodically broadcasts their `Transform`
   (`crates/movement/src/syncing.rs:88-104`); the receiver overwrites the local
   transform with the remote one (`crates/movement/src/syncing.rs:77-86`), and
   the message is marked `Reliability::Unreliable`
   (`crates/multiplayer/src/messages.rs:145`) because a stale position beats
   a late one.

So DE's model is **state synchronisation with periodic correction**, not
deterministic lockstep. Ours is the opposite: the same seed and the same
decisions produce the same bytes on x86-64 and aarch64
(`crates/lm-core/src/sim.rs:34-36`; D69 at `docs/design/decisions.md:83`),
and only decisions cross a machine boundary (D10,
`docs/design/decisions.md:16`).

**This is the single most important thing to know before reusing anything from
DE.** Every mechanism below has to be re-derived, not ported: a float
position that crosses the wire cannot be ours, and a renderer that is allowed
to invent state cannot be ours either.

### 1.4 What travels

The `ToPlayers` message set is the whole wire vocabulary for a game
(`crates/multiplayer/src/messages.rs:141-148`): `Chat`, `Spawn`, `Despawn`,
`SetPath`, `Transform`, `ChangeHealth`, `Projectile`. Each is tagged with a
reliability — chat unordered, spawn/despawn/path/health semi-ordered,
transform and projectile unreliable.

Entities are identified by a **(player, local index)** pair
(`crates/messages/src/players/entity.rs:8-31`): the owning player's own entity
numbering, which both sides must agree on. That is a state-ownership model —
the owner is the authority for its own objects.

Ours is different in kind: a decision is `(tick, source, kind, payload)` and
is applied by the sim that owns the round. There is no per-entity owner, and
no per-entity state on the wire at all.

---

## 2. The watchability machinery, area by area

Each area below says what DE does, what we already have, what we could
re-implement on our fixed-point sim and Bevy host, and where DE's approach
collides with determinism or with "only decisions travel".

### 2.1 The RTS camera

**What DE does.** The camera is an orbit rig with three degrees of freedom.

* `CameraFocus` is a **point in the world plus a distance**
  (`crates/camera/src/camera.rs:216-243`) — not a position and a look-at. The
  camera's position is derived from the focus, the distance and the tilt.
* **Pan** moves the focus along the ground at `CAMERA_HORIZONTAL_SPEED = 2.0`
  per second (`crates/camera/src/camera.rs:23`; handler at `:416-446`).
* **Zoom** moves the camera along its own forward axis at
  `CAMERA_VERTICAL_SPEED = 2.0` per second (`crates/camera/src/camera.rs:32-36`;
  handler at `:448-483`), clamped to the configured range by `zoom_clamped`
  (`crates/camera/src/camera.rs:275-282`).
* **Tilt** is clamped to `[0.001, 0.7·π/2]` radians
  (`crates/camera/src/camera.rs:38-40`, `:293-295`). The camera can never
  reach a true top-down: the game is always seen at an angle, and the angle
  has both a floor and a ceiling.
* **Azimuth** is free (`crates/camera/src/camera.rs:302-309`), so the player
  can spin the world around.
* Shadows are four cascades, re-tuned to the focus every frame
  (`crates/camera/src/camera.rs:43`, `:485-499`).
* `CameraDistance` is a per-object component holding that object's distance
  from the camera, updated every frame (`crates/camera/src/distance.rs:28-36`,
  `:54-69`). It is the budget hook: everything else that wants to know "is
  this close enough to draw" reads this one component.

**What we have.** A `CameraRig` with a focus point in world metres, a zoom in
pixels per metre, and an optional agent to follow
(`crates/lm-host/src/map_plugin.rs:246-258`); a drag pans, the wheel zooms,
`WASD` pans (`crates/lm-host/src/input_plugin.rs:229-266`); the camera is free
only when nobody is being played or watched
(`crates/lm-host/src/input_plugin.rs:229-230`).

**What to take.** Two things, and only two.

1. **Tilt.** Our 3D view is a fixed camera. An RTS camera that can lower toward
   the ground changes what a match *feels* like more than any amount of
   texture work: at a shallow angle the eight Dabs read as figures on a
   street; from directly above they read as dots. DE clamps tilt away from
   nadir (`crates/camera/src/camera.rs:38-40`), and so should we — the top-down
   case is already covered by the 2D map.
2. **One distance component, one LOD budget.** DE has exactly one place that
   knows how far a thing is from the camera
   (`crates/camera/src/distance.rs`), and the signs crate uses it to stop
   drawing at 140 m (`crates/signs/src/lib.rs:17-18`). We should have the
   same: a per-agent distance, and one rule "draw this only if it is closer
   than N".

**Conflict?** None. The camera is a host resource and never touches the sim;
our own view switch is built so that "a view that owned the round could reset
it; this one cannot" (`crates/lm-host-model/src/view.rs:5-11`).

### 2.2 Selection and box-select

**What DE does.** A selection model in two halves.

* The *area* half: `SelectInRectEvent` carries a screen rectangle, a
  `SelectionMode` and an optional object-type filter
  (`crates/controller/src/selection/area.rs:36-67`). `select_in_area` runs in
  `InputSchedule`, before the selection bookkeeping updates
  (`crates/controller/src/selection/area.rs:19-29`).
* The resolution is the good part. The screen rectangle is turned into a
  **world-space frustum** (`crates/controller/src/frustum.rs:28-63`: a
  perspective frustum with the camera's near and far planes and four
  half-spaces for the rectangle's edges), and a unit is selected if **any part
  of its bounding box** intersects that frustum
  (`crates/controller/src/selection/area.rs:85-92`). Not its centre point: its
  volume. A unit half-covered by the box is in.
* The *bookkeeping* half owns the truth: `Selected` is a marker component,
  `SelectedEvent` and `DeselectedEvent` are the deltas
  (`crates/controller/src/selection/bookkeeping.rs:66-73`), and the mode is
  `Replace`, `Add` or `AddToggle`
  (`crates/controller/src/selection/bookkeeping.rs:75-84`).
* The HUD draws it (`crates/controller/src/hud/selection.rs`), and the commands
  are a separate subsystem entirely (`crates/controller/src/commands/`), so
  "what is selected" and "what did you order" never get confused.

**What we have.** No selection at all. A click on the map is the order
(`crates/lm-host/src/input_plugin.rs:213-225`), and the input layer turns that
into `ActionOn(GoTo(cell))` for the one agent being possessed
(`crates/lm-host-model/src/input.rs:12-17`, `:62-64`).

**What to take.** The principle, not the frustum: **select on volume, not on
a point.** In our top-down map the equivalent is a circle around each agent's
radius, and the rule "if any part of the agent is inside the rubber band, the
agent is in" is the difference between a selection that feels like a selection
and one that feels like a hit test.

The second thing to take is the *separation*: DE's selection is a UI fact and
the commands that follow are separate. We already have the right half — the
decision log (`crates/lm-host-model/src/input.rs:12-17`); the missing half is
the UI fact.

**Conflict — and it matters.** A selection set is **host state, not sim
state**. It must never enter the round's hash or the decision log, and it must
never itself be a decision. DE can afford to keep it in the ECS because DE has
no hash; we have to keep ours out of `lm-core` entirely. The first step below
is therefore *host-model state*, not a `lm-core` field.

### 2.3 Unit readability: shape, colour, bars

**What DE does.** Four separate mechanisms, and the separation is the lesson.

* **Shape.** Every object has an *ichnography* — a convex-hull footprint plus
  a bounding sphere (`crates/objects/src/ichnography.rs:9-49`). The same hull
  drives collision and the selection test, so what you see is what you hit.
* **Colour.** The minimap carries a per-player colour table
  (`crates/controller/src/hud/minimap/fill.rs:24-29`).
* **Bars.** `crates/signs/src/bars.rs` floats a 1.5 × 0.3 m bar
  (`crates/signs/src/bars.rs:195`) 2.0 m above the object's bounding-box top
  (`crates/signs/src/bars.rs:26-28`, `:210`), drawn with a dedicated WGSL
  shader (`crates/signs/src/bars.rs:152-157`; `assets/shaders/bar.wgsl`), and
  **it only appears for three seconds after the value changes**
  (`UPDATE_VISIBILITY_DURATION`, `crates/signs/src/bars.rs:30-31`). A bar that
  is always on is a bar nobody reads.
* **Distance culling.** The whole signs group stops drawing past 140 m
  (`crates/signs/src/lib.rs:17-18`), and the bars carry their own visibility
  flag bits (`crates/signs/src/lib.rs:19-20`; `crates/signs/src/bars.rs:269-300`).

**What we have.** One shared low-poly figure mesh and **one material per
squad** (`crates/lm-host/src/view3d/agents.rs:4-14`), so the whole roster
instances on the GPU; what makes a figure its own is its `Transform`, set
every frame from the lerped position and facing
(`crates/lm-host/src/view3d/agents.rs:203-217`). We have the colours — four
squad colours, five material classes and a leader rim
(`crates/lm-render/src/palette.rs:176-203`). We have no bars and no shapes.

**What to take.**

1. **A mark that only shows when something changed.** Our sim already has a
   per-agent `stunned_until` (`crates/lm-core/src/agent.rs:76-77`) and already
   emits `Event::Tagged { by, target }` and `Event::Respawned`
   (`crates/lm-core/src/rules/mod.rs:63-71`). The host already receives them:
   `TickOut` carries `events: Vec<Event>`
   (`crates/lm-host-model/src/host_loop.rs:59-67`) and the host keeps them in
   `LiveEvents` (`crates/lm-host/src/sim_plugin.rs:184`, `:353`, `:369-370`).
   **The data is already at the host. Nothing is drawn.** A small "stunned"
   pip over a Dab for three seconds is the cheapest watchability win in this
   whole document.
2. **Shape as a second channel.** DE separates *what* a thing is from *whose*
   it is (`crates/objects/src/ichnography.rs:9-49`). We have only colour.
   One extra mesh — or one extra scale — for the squad leader, or a distinct
   pose for "stunned", costs nothing and reads instantly at any zoom.

**Conflict?** The mark must be derived from the sim's state, never from the
host's guess. `stunned_until` is hashed game state — the same pattern as the
airtime field, which the sim covers by its hash
(`crates/lm-core/src/agent.rs:90-95`) — so a mark driven by it is
deterministic by construction. The mark itself is a render artefact and never
enters the hash.

### 2.4 The minimap

**What DE does.** A **software rasteriser**, not a second 3D render
(`crates/controller/src/hud/minimap/`):

* `draw.rs` owns an RGBA buffer and implements `fill`, `line` and `rect`
  directly on it (`crates/controller/src/hud/minimap/draw.rs:24-96`), with the
  world→pixel mapping at `:145`.
* `fill.rs` decides what goes on it: a terrain colour
  (`crates/controller/src/hud/minimap/fill.rs:23`), per-player colours
  (`:24-29`), a **minimum entity size** of 0.02 so a unit is never
  sub-pixel (`:30`), and a **white camera rectangle** (`:31`, `:137-157`).
* `nodes.rs` places it in the UI; `interaction.rs` makes it clickable.
* The map is a *projection*: `flat_to_rel` maps world metres into a 0..1 UI
  space (`crates/controller/src/hud/minimap/fill.rs:85-94`).

**What we have.** A 2D overhead map that is already exactly this projection
(`crates/lm-host-model/src/map.rs:1-44`), and a 3D view of the same round
(`crates/lm-render/src/fog.rs:1-14`).

**What to take.** Two specific things, both cheap:

1. **The camera rectangle on the minimap.** A white box showing where the 3D
   camera is looking is the cheapest possible orientation cue in an RTS, and
   against a map model we already build it is a dozen lines.
2. **The minimum entity size.** On a 256 m world shrunk into a 120 px corner,
   an eight-agent match is eight sub-pixel specks. DE's rule — never draw an
   entity smaller than 2% of the minimap — is what keeps the minimap readable
   at the scale our rounds actually are.

**Conflict?** None, as long as the minimap is drawn from the same `MapModel`
and the same fog rule as the map, so the two cannot disagree. Our fog rule is
already one rule with two views (`crates/lm-render/src/fog.rs:1-14`); a third
view has to go through it.

### 2.5 Pathing and formation movement

**What DE does.** Two layers.

* **Global path**: Polyanya, a visibility-graph any-angle pathfinder over the
  triangulated heightfield (`crates/pathing/src/polyanya.rs:1-33`), after Cui,
  Harabor & Grastien (IJCAI 2017), with hard search limits
  (`MAX_SEARCH_STEPS = 10_000_000`, `MAX_OPEN_SET_SIZE = 1_000_000`;
  `crates/pathing/src/polyanya.rs:16-17`). Any-angle means units cut corners
  instead of walking a tile route, which is most of why they look like they
  are *deciding* rather than *following a grid*.
* **Local avoidance**: a repulsion field
  (`crates/movement/src/repulsion.rs:22-25`: `MAX_REPULSION_DISTANCE = 4.0` m,
  `REPULSION_FACTOR = 0.6`, minimum 1.0 m from static objects and 0.5 m from
  movers). The repulsion is clamped to a per-direction bound so it can never
  exceed the motor's maximum acceleration
  (`crates/movement/src/repulsion.rs:87-98`, `:119-139`). That clamp is the
  trick: units get out of each other's way without ever being *pushed*, so
  their paths stay their own.

**What we have.** Integer Jump Point Search, 8-connected, strict no corner
cutting, octile costs, with a total `(f, h, y, x)` tie-break so the same map
always yields the same path (`crates/lm-core/src/world/path.rs:1-28`). The
route is cached on the agent and re-planned when the goal changes
(`crates/lm-core/src/agent.rs:86-87`).

**What to take.** Not the algorithm — JPS is the right choice for us, and
any-angle Polyanya over a floating heightfield is not available on fixed point
without a lot of new arithmetic. Take the **look**:

1. **Draw the path.** We already store the route on the agent
   (`crates/lm-core/src/agent.rs:86-87`). Drawing it as a thin ribbon on the
   ground is the best answer to "why did that Dab go there?", and it costs one
   draw call and no new sim state.
2. **The repulsion clamp, later.** Our Dabs have no local avoidance at all.
   When they get one, the invariant to copy is DE's: **avoidance may change
   the velocity, never the intent.** The goal stays the goal.

**Conflict?** The route is sim state (`Agent::route` is part of the agent and
therefore of the hash), so drawing it raises no determinism question. But the
ribbon must be drawn from the *cached* route, not re-derived by the renderer,
or the renderer would be inventing state — the thing
`crates/lm-host/src/view3d/agents.rs:11-14` forbids for positions.

### 2.6 Combat feedback

**What DE does.** Four plugins in one group — Laser, Attack, Trail, Health
(`crates/combat/src/lib.rs:19-26`) — with the systems split into Attack,
Charge and Fire sets (`crates/combat/src/lib.rs:29-34`).

* **The shot.** `LaserFireEvent` carries the attacker, a `Ray`, a maximum
  distance and a damage value (`crates/combat/src/laser.rs:29-59`). The fire
  system resolves it against a line of sight
  (`crates/combat/src/laser.rs:78-81`) and applies health.
* **The beam.** `crates/combat/src/trail.rs` spawns a quad along the ray
  (`crates/combat/src/trail.rs:159-186`) with a material that carries only a
  `start_time` uniform and a WGSL shader that fades it
  (`crates/combat/src/trail.rs:91-124`; `assets/shaders/trail.wgsl`); it lives
  `TRAIL_LIFESPAN = 500 ms` and then despawns itself
  (`crates/combat/src/trail.rs:23`, `:200-207`). One mesh, one material, one
  uniform, one age-based fade — a reusable "fading mark" primitive.
* **The sound.** The same event also fires a positional sound at the shot's
  origin (`crates/combat/src/trail.rs:188-198`).
* **Health.** Health is a component with two update paths — locally simulated
  entities and remote ones (`crates/combat/src/health.rs:42-75`, `:77-105`) —
  because in DE each client owns some entities and receives others.

**What we have.** A tag, a stun, and a ruleset event
(`crates/lm-core/src/rules/mod.rs:63-69`). No projectile, no flash, no beam,
and nothing at all that says a tag happened.

**What to take.** The **fading-mark primitive** is the steal, not the laser.
DE's trail is one mesh, one material with a `start_time`, one shader, one
lifetime, one despawn. We need exactly this for the sandbox's **paint splat**,
which is the sandbox's own central idea
(`docs/design/sandbox-on-otherhere.md:72-75`): a splat is a mark with an owner
that stays on the ground.

**Conflict — and it is the sharp one.** DE's trail is a *pure render*
artefact: it is spawned from an event and never read back. Our splat is *sim
state*: the sandbox wants the paint to accumulate in the world so the map tells
you what the round was, and so paint can change the rules (`paint.slow`,
`paint.stain_persist` in `docs/design/sandbox-on-otherhere.md:324-331`). So
the splat must be a **fixed-point, hashed field in `lm-core`**, and the
*rendering* of it is the DE-style fading mark on top. DE's version would put
the splat in the renderer, where it would be invisible to the rules and lost on
replay. **This is the one place where DE's design and ours genuinely disagree,
and ours is right.**

### 2.7 Terrain rendering

**What DE does.** The terrain is a heightfield with a custom WGSL shader
(`crates/terrain/src/shader.rs`; `assets/shaders/terrain.wgsl`), its own
collider (`crates/terrain/src/collider.rs`), and a plugin group of
`TerrainPlugin` and `MarkerPlugin` with `MAX_ELEVATION = 1024.0`
(`crates/terrain/src/lib.rs:14-23`).

* **Ground marks.** `crates/terrain/src/marker.rs` is a *separate plugin*
  from the terrain itself (`crates/terrain/src/lib.rs:16-23`), and it holds
  the ground decals: `CircleMarker` is "a semi-transparent circle drawn on the
  terrain surface below the entity"
  (`crates/terrain/src/marker.rs:50-55`) and `RectangleMarker` is "a
  semi-transparent rectangle drawn on the terrain surface below the entity",
  sized from the entity's AABB plus a margin
  (`crates/terrain/src/marker.rs:77-83`, `:94-96`). Their shapes are blended
  *into the terrain material* rather than floated above it as separate quads
  (`crates/terrain/src/marker.rs:42-48`), and each marker carries its own
  visibility flags (`crates/terrain/src/marker.rs:38-40`).
* The repository ships four custom shaders: `terrain`, `bar`, `trail` and
  `rally_point` (`assets/shaders/`).

**What we have.** A chunked 3D world with greedy-meshed chunks
(`crates/lm-render/src/greedy.rs`) and a palette of five material classes
(`crates/lm-render/src/palette.rs:194-200`). Our grid knows three cell kinds
— floor, wall, cover (`crates/lm-core/src/world/grid.rs:11-41`).

**What to take.** The **marker layer, as a separate system with its own
visibility state**. For us that means paint splats, beacon-hold rings and
selection rings are one decal system with a visibility flag each, not three
special cases in the map renderer — and, like DE's, they are blended into the
surface they sit on rather than floated above it. DE's `RectangleMarker` is
sized from the entity's AABB plus a margin
(`crates/terrain/src/marker.rs:94-96`), which is the same rule a paint splat
wants: a mark the size of the thing that made it.

**Conflict?** Our cell kinds are coarse (2 m, `crates/lm-core/src/params.rs:77`)
and a splat wants to be a shape, not a cell tint. That is a resolution
question, not a determinism question — but the *owner* of a splat and its
*age* are sim state, and must be fixed-point and hashed.

### 2.8 Audio cues

**What DE does.** `crates/audio/src/lib.rs:9-16` is a group of `MusicPlugin`
and `SpatialSoundPlugin`. `SpatialSoundPlugin` takes a
`PlaySpatialAudioEvent` carrying a sound enum and a **world position**
(`crates/audio/src/spatial.rs:46-57`), and computes volume and pan from the
camera (`crates/audio/src/spatial.rs:94-131`; `update_spatial` at `:167`). The
sound vocabulary is five events (`crates/audio/src/spatial.rs:37-44`):
construct, manufacture, destroy-building, destroy-unit, laser-fire. The asset
folder matches exactly (`assets/audio/sounds/`: `construct.ogg`,
`destruction_building.ogg`, `destruction_unit.ogg`, `laser.ogg`,
`manufacture.ogg`) plus one music loop (`assets/audio/music/menu_loop.mp3`).

Two details worth stealing: the sound is **positioned, not a channel**
(`crates/audio/src/spatial.rs:13-14` computes the view angle from a point
behind the camera), and the sound fires from the *same event* that draws the
effect (`crates/combat/src/trail.rs:188-198`).

**What we have.** The human's voice and the agents' talk — microphone, ASR,
prosody, the agent clips — and **no gameplay sound effect at all**
(`crates/lm-host/src/audio_plugin.rs:1-26`). The host's audio is deliberately
for the human, and the sim never depends on it.

**What to take.** One positional cue, fired from the tag event: a short
"pfft" at the tagged Dab's position. DE proves the shape (an event with a
world position, volume and pan computed against the camera) and proves that
five sounds are enough for a whole game.

**Conflict?** Sound must never block or change the round. Our audio path
already has the right property — "with no microphone, no ASR and no prosody
service, the round plays exactly as it did before"
(`crates/lm-host/src/audio_plugin.rs:20-23`) — and a gameplay cue must keep
it: fire-and-forget, never awaited, never in the sim's dependency graph.

---

## 3. The top 8 ideas for the starting paintball match

Ranked by **watchability gained per unit of effort**, and aimed at the
sandbox's first match: two squads of four Dabs, four amber points, five
minutes, a tag that paints (`docs/design/sandbox-on-otherhere.md:44-56`).

| # | Idea | Watchability | Effort | Where it lands |
| --- | --- | --- | --- | --- |
| 1 | Paint the tag: a splat in the world, drawn on the ground | very high | low | `lm-core` state + `lm-render` decal |
| 2 | Say what happened: a stun pip over the tagged Dab | very high | very low | `lm-host` only |
| 3 | Draw the path: a ribbon on the ground for each Dab | high | low | `lm-host` only |
| 4 | Team colour *and* shape: colour plus silhouette | high | low | `lm-render` only |
| 5 | Tilt the camera: an RTS camera, not a diorama | high | medium | `lm-host` |
| 6 | Selection: click, rubber band, and a visible selection set | medium-high | medium | `lm-host-model` + `lm-host` |
| 7 | The minimap: a corner projection with a camera box | medium-high | medium | `lm-host` |
| 8 | Sound: one positional cue for the tag | medium | medium | `lm-host` |

### 1. Paint the tag — the splat is the scoreboard

**Why.** The sandbox's own thesis is that the paint is the readable
consequence of a tag, and that the map then tells you what the round was
(`docs/design/sandbox-on-otherhere.md:72-75`, `:135-141`). Right now a tag is
a stun and a line in a log; there is nothing on the ground.

**What DE teaches.** The *rendering* half of a ground mark is one mesh, one
material carrying a `start_time` uniform, one WGSL shader, one lifetime and
one despawn (`crates/combat/src/trail.rs:91-124`, `:159-207`). Its ground
marks are a separate plugin from the terrain, with their own visibility flags
(`crates/terrain/src/lib.rs:16-23`; `crates/terrain/src/marker.rs:38-48`).

**Where it lands.** The *state* half is ours and DE's cannot be copied: a
fixed-point paint field — cell, amount, owner — in `lm-core`, hashed like any
other world state (`crates/lm-core/src/world/grid.rs:1-6`). The *look* is the
DE-style decal.

**Smallest first step.** One cell of the world already exists, and one tag
event already exists. Add to the sim a per-cell `paint: (amount, owner)` on
the cell the tag happened on, emitted through the existing
`Event::Tagged { by, target }` (`crates/lm-core/src/rules/mod.rs:63-69`), and
draw it in the 2D map as a filled cell in the tagging squad's colour. No new
renderer, no new material, no new shader — a paintball match on a top-down map
with coloured cells is already the game.

### 2. Say what happened — the stun pip

**Why.** A tagged Dab simply stops moving for three seconds
(`crates/lm-core/src/params.rs:102`; `assets/rules/beacons.rhai:117-118`) and
nothing on screen says why. This is the single worst thing about watching the
current match.

**What DE teaches.** Marks that appear only for three seconds after the value
changes (`crates/signs/src/bars.rs:30-31`, `:238-253`), floated above the
object (`crates/signs/src/bars.rs:26-28`), and culled by distance
(`crates/signs/src/lib.rs:17-18`).

**Where it lands.** Purely host-side. The events are already at the host:
`TickOut.events` (`crates/lm-host-model/src/host_loop.rs:59-67`) and
`LiveEvents` (`crates/lm-host/src/sim_plugin.rs:184`, `:369-370`).

**Smallest first step.** In the 2D map, draw a filled disc over the tagged
agent for the length of the stun, in the tagging squad's colour. The host
already knows the agent's `stunned_until` and already receives the tag event;
this is a painter, not a feature.

### 3. Draw the path

**Why.** "Why did that Dab go there?" is the question a spectator asks every
ten seconds, and we already hold the answer — the route is cached on the
agent (`crates/lm-core/src/agent.rs:86-87`).

**What DE teaches.** Polyanya's any-angle routes look like decisions
(`crates/pathing/src/polyanya.rs:19-33`), and DE treats the path as a
first-class network message (`SetPath`,
`crates/multiplayer/src/messages.rs:144`) — worth showing, not just walking.

**Where it lands.** Host-side, from the agent's cached route. Not re-derived by
the renderer — that would be the renderer inventing state, which
`crates/lm-host/src/view3d/agents.rs:11-14` forbids.

**Smallest first step.** One polyline per possessed or selected agent, drawn
from `Agent::route`, in the squad colour at low alpha. The 2D map already
draws agents at interpolated positions
(`crates/lm-host-model/src/map.rs:36-44`), so the map model is the place.

### 4. Team colour *and* shape

**Why.** We have colour (`crates/lm-render/src/palette.rs:176-203`) and one
shared figure mesh (`crates/lm-host/src/view3d/agents.rs:4-14`). DE separates
what a thing *is* from whose it is (`crates/objects/src/ichnography.rs:9-49`).

**Smallest first step.** Give the leader a distinct silhouette — the palette
already carries a `leader_rim` colour
(`crates/lm-render/src/palette.rs:181-182`, `:201`) — and give a stunned Dab a
squashed or tilted pose. Two shapes, no new assets.

### 5. Tilt the camera

**Why.** Fixed-angle diorama versus RTS camera is the difference between "a
diagram of a match" and "a match". DE's camera can never reach nadir
(`crates/camera/src/camera.rs:38-40`), and our 2D map already covers the
top-down case, so the 3D view has nothing to lose.

**Smallest first step.** Add a pitch to `CameraRig`
(`crates/lm-host/src/map_plugin.rs:246-258`), bind it to two keys, and clamp it
to the same "never quite top-down" range DE uses.

### 6. Selection

**Why.** The sandbox's storyboard has Riley *grabbing a Dab*
(`docs/design/sandbox-on-otherhere.md:131-133`); our input can only order the
one agent being possessed (`crates/lm-host-model/src/input.rs:62-64`).

**What DE teaches.** Select on **volume**: a unit is in the box if any part of
its bounding box intersects the box's world frustum
(`crates/controller/src/selection/area.rs:85-92`;
`crates/controller/src/frustum.rs:28-63`), and the selection modes are
Replace / Add / AddToggle
(`crates/controller/src/selection/bookkeeping.rs:75-84`).

**Smallest first step.** Hold a modifier and drag a rectangle in the 2D map;
every agent whose **radius** overlaps the rectangle joins the selection; a
click with no drag clears it. Keep the selection in the **host model**, not in
`lm-core` — it is a UI fact, and the moment it enters the sim it starts
costing hash bytes and determinism for nothing.

### 7. The minimap

**Why.** A 256 m world seen through a 3D camera at a playable zoom is not the
whole match. DE's minimap is a software rasteriser over the same world
(`crates/controller/src/hud/minimap/draw.rs:24-96`) with a minimum entity size
(`crates/controller/src/hud/minimap/fill.rs:30`) and a camera rectangle
(`crates/controller/src/hud/minimap/fill.rs:31`, `:137-157`).

**Smallest first step.** A 120 × 120 corner panel that reuses the map model's
own drawing, with two additions: never draw an agent smaller than 2% of the
panel, and draw the 3D camera's view box on top. The fog rule is already one
rule with two views (`crates/lm-render/src/fog.rs:1-14`); the minimap must go
through it or it becomes a third, divergent view.

### 8. Sound: one positional cue for the tag

**Why.** The match is silent apart from voices. A tag should *sound* like a
dab of paint.

**What DE teaches.** One event with a world position, volume and pan computed
against the camera (`crates/audio/src/spatial.rs:46-57`, `:94-131`), and five
sounds for a whole game (`crates/audio/src/spatial.rs:37-44`;
`assets/audio/sounds/`).

**Smallest first step.** Fire a `PlaySpatialAudioEvent`-shaped event from the
same place that fires the stun pip, with the tag's position. Fire-and-forget:
never awaited, never in the sim's graph — our audio is already outside it
(`crates/lm-host/src/audio_plugin.rs:20-23`).

---

## 4. Multiplayer and lobby design

### 4.1 The topology: a relay, not an authority

DE's multiplayer has **a small authority plus per-game servers**, and the
authority is a *relay*, not a simulator.

* The **main server** (`crates/connector/src/server.rs:16-115`) does almost
  nothing: it answers pings, and on an `OpenGame` request it binds a new UDP
  socket and starts a **game server** on it (`:77-104`). It keeps a
  client→port table (`crates/connector/src/clients.rs`).
* The **game server** (`crates/connector/src/game/mod.rs:32-64`) spawns four
  tasks: `ereceiver` for events, `mreceiver` for messages to players, the
  `GameProcessor` in `greceiver`, and `preceiver` for player-to-player
  traffic (`crates/connector/src/game/mod.rs:46-63`).
* The **player-to-player path** is the key. `preceiver::run` takes a message
  from one player, wraps it with the sender's id, and **pushes it into every
  other player's buffer** (`crates/connector/src/game/preceiver.rs:57-62`).
  The buffers batch messages by reliability and flush them on a timer
  (`crates/connector/src/game/buffer.rs:55-81`).

**The server never simulates the game.** It tracks player ids, readiness and
who is connected (`crates/connector/src/game/state.rs:99-103`) and relays
bytes. The clients each run their own world and correct each other by
overwriting transforms (`crates/movement/src/syncing.rs:77-86`).

### 4.2 The lobby server

A separate HTTP service, `crates/lobby/`, with an OpenAPI specification
(`docs/src/multiplayer/openapi.yaml`; `docs/src/multiplayer/lobby.md`). It is
configured by environment variables only: a SQLite URL (`DE_DB_URL`), a
base64 JWT secret (`DE_JWT_SECRET`, 12–86 characters), an HTTP port
(`DE_HTTP_PORT`, default 8080) and `RUST_LOG`
(`docs/src/multiplayer/lobby.md:9-23`). Its game routes are five verbs
(`crates/lobby/src/games/endpoints.rs:9-18`): create, get, list, join, leave.

The interesting design decisions are in the *errors*, because the errors are
the lobby's rules written as refusals
(`crates/lobby/src/games/endpoints.rs:35-42`, `:96-105`): "Game name is
already taken", "User is already in different game", "Another player has
already joined the game under the given ordinal", "The given ordinal is larger
than maximum number of players". A lobby with **named** refusals is a lobby
whose rules you can read.

### 4.3 The readiness ladder

Every client walks a four-rung ladder — `NotReady → Ready → Initialized →
Prepared` (`crates/messages/src/game.rs:70-88`) — and the server advances the
game's readiness only when *all* players have reached a rung
(`crates/connector/src/game/state.rs:153-195`). The transitions refuse to skip
a rung (`:171-178`), refuse to go backwards (`:162-168`), and refuse to let a
client get ahead of the game (`:179-186`).

That is a **soft barrier**, not a lockstep: the server does not wait for
everyone to be on the same tick; it waits for everyone to be *ready*.

### 4.4 What is relevant to two players, two squads

For the sandbox's later two-player, two-squad shape, DE offers three things
worth taking and two to refuse.

**Take: the ordinal.** A player joining a game claims a slot number, and the
lobby refuses an ordinal that is taken or out of range
(`crates/lobby/src/games/endpoints.rs:96-105`). For us the ordinal is the
natural identity of "which squad is this human in" — and the refusal is the
right shape, because it is a *fact about the world* that the server states
rather than a guess.

**Take: the relay topology.** DE's main server binds a game server per game
(`crates/connector/src/server.rs:88-96`). That is the same shape our own
design already assumes: the ProArt is the sequencer and authority, and the
Thor and Orin run mirror sims from the decision stream (D18,
`docs/design/decisions.md:24`). DE reaches the shape — a small authority plus
per-game servers — but stops at relaying bytes.

**Take: named refusals.** Every failure in the lobby and in the connector has
a name (`crates/lobby/src/games/endpoints.rs:35-42`; the connector's
`JoinError` variants at `crates/connector/src/game/state.rs:253-261`).

**Refuse: state on the wire.** DE's `ToPlayers` set carries `Transform`,
`ChangeHealth` and `Projectile`
(`crates/multiplayer/src/messages.rs:141-148`). For us that is the line we
must not cross: a transform that travels is a position the receiver believes
without re-deriving it, and it is exactly what our fixed-point core exists to
prevent. We travel decisions (`docs/design/decisions.md:16`) and the sim
re-derives everything (`crates/lm-core/src/sim.rs:9-27`).

**Refuse: the readiness ladder as a substitute for determinism.** DE needs a
readiness ladder because its clients can disagree. Ours cannot disagree — the
hash *is* the agreement (`crates/lm-core/src/sim.rs:34-36`). Our equivalent
is the state hash on its interval (`crates/lm-core/src/sim.rs:26-27`), which
is a check, not a barrier.

### 4.5 What DE's lobby is *not*

DE's lobby is an account-and-chat service: a user, a game name, an ordinal
and a JWT (`crates/lobby/src/games/endpoints.rs`,
`crates/lobby/src/auth/`). There is no round, no ruleset, no header, no
replay and no spectator in it.

For a game whose unit of work is a **reproducible round** (D64's chain,
`docs/design/decisions.md:78`), that is a very different artefact. Our
"lobby" is a round header plus a decision stream, and our replay is the log.
The lesson to take from DE's lobby is not its data model but its *shape*: a
small HTTP service with named refusals, in front of a per-game server, in
front of a transport.

---

## What to take, what to leave

| Take | From | Because |
| --- | --- | --- |
| The camera as point + distance + tilt, with one distance LOD budget | `crates/camera/src/camera.rs`, `crates/camera/src/distance.rs` | changes the feel of a match more than any material does |
| Select on volume, not on a point | `crates/controller/src/selection/area.rs`, `crates/controller/src/frustum.rs` | the difference between a selection and a hit test |
| Marks that show only for three seconds after a change | `crates/signs/src/bars.rs` | a permanent bar is a bar nobody reads |
| The fading-mark primitive: mesh + `start_time` uniform + shader + lifetime | `crates/combat/src/trail.rs` | the paint splat needs exactly this |
| Ground marks as their own plugin, separate from the terrain | `crates/terrain/src/marker.rs` | decals are one system, not three special cases |
| A software-raster minimap with a minimum entity size and a camera box | `crates/controller/src/hud/minimap/` | orientation for a few dozen lines |
| Positional cues: one event, one position, volume and pan from the camera | `crates/audio/src/spatial.rs` | five sounds are enough for a whole game |
| Named refusals everywhere | `crates/lobby/src/games/endpoints.rs` | a lobby's rules should be readable |
| The relay topology: a small authority plus per-game servers | `crates/connector/src/server.rs` | the shape D18 already assumes |

| Leave | From | Because |
| --- | --- | --- |
| Floats in the simulation | `crates/camera/src/camera.rs:23` | ours is fixed-point on purpose (`crates/lm-core/src/sim.rs:34-36`) |
| State on the wire (`Transform`, `ChangeHealth`) | `crates/multiplayer/src/messages.rs:141-148` | only decisions travel (`docs/design/decisions.md:16`) |
| A server that relays bytes instead of sequencing decisions | `crates/connector/src/game/preceiver.rs:57-62` | our authority *is* the sequencer (D18) |
| The readiness ladder as an agreement mechanism | `crates/connector/src/game/state.rs:153-195` | our agreement is the hash |
| A float health model | `crates/combat/src/health.rs:77-105` | our tag is a stun, and a stun is a tick |
| Render-side ground state — DE's trail has no owner | `crates/combat/src/trail.rs` | our paint must be hashed world state |

---

## Method, and what was not read

* **Read only, over HTTP, one file at a time.** The repository was never
  cloned, never built and never run. The GitHub API's contents endpoint was
  used for the tree and the repository metadata; when the API's
  unauthenticated rate limit (60 requests per hour) was exhausted, the same
  individual files were read over `raw.githubusercontent.com`, which serves the
  same bytes from the same `main` ref. No repository was written to disk and
  no code from it appears in this document or anywhere in last-mile.
* **What was read.** The workspace manifest and the 32 crate manifests; the
  full file tree; and 116 source files across `core`, `camera`, `controller`,
  `combat`, `signs`, `terrain`, `movement`, `pathing`, `objects`, `audio`,
  `multiplayer`, `connector`, `lobby`, `lobby_model`, `messages` and `net`;
  plus `assets/shaders/`, `assets/audio/sounds/` and the multiplayer
  documentation.
* **What was not read.** The `gui`, `menu`, `construction`, `spawner`,
  `energy`, `behaviour`, `map`, `loader`, `log`, `index`, `tools` and
  `test_utils` crates in depth; the `.github` workflows; `docs/src/design/`
  and `docs/src/tutorial/`; and the two `.dem.tar` map assets. None of those
  bear on watchability, with one exception noted in §4.4.
* **One caveat.** The tree is a snapshot of `main` as of the archive
  (September 2024). The line numbers in the citations are from that
  snapshot and will drift if the archive is ever re-published.
