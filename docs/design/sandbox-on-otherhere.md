# The sandbox: a paintball match in Otherhere that becomes a game you build by talking to it

**Status:** design document. No code changes.
**Date:** 2026-10-29
**Author:** for Casey

## What this document is

A design for the sandbox: a world-building game where a player — aimed at curious middle
schoolers — **changes the game by speaking or typing**, an AI **co-developer** makes the change
happen and explains it, and the same AI sometimes **plays the rival faction** in a live verbal
negotiation. The stage is **Otherhere**, the real-place world, and the rules underneath are
last-mile's deterministic sim.

Everything here is original: the rules, the cast, the names, the look. We took a *feel* — small
comic units, playful non-lethal paint chaos, splats, plans going sideways funnily — and a set of
*design lessons* from a source conversation, a flight-sim fork and two decision logs. We took no
code, no assets, and no art direction from anywhere.

**The one correction we make to the source conversation.** It proposed that an LLM rewrite the
game's source files live while you watch. We do not do that, and we will not. Generated code
crashes, cannot be replayed, cannot be trusted around kids, and is forbidden outright by
Otherhere's invariant 3 (`Otherhere/docs/BRIEF.md:42`). Instead: **words → a typed change from a
closed vocabulary** (the Vibe Dictionary) → a parameter or rule change in the harness → validated,
hash-chained, replayable, undoable. The "Glitch Meter" survives, and it becomes *more* honest,
because a real measurement replaces a vibe.

### Sources, and how to read the citations

| Prefix | Repository |
| --- | --- |
| *(none)* | this repo, `/home/proart/lm-wt/gamedesign` — the last-mile sim, the harness, the decision log |
| `Otherhere/` | `/home/proart/projects/Otherhere` — the real-place world |
| `OpenSkyFlight/` | `github.com/Lucineer/OpenSkyFlight` — Magnus's Sitka Skies fork |
| `src-conversation` | `/home/proart/lm-inbox/fromgoogle2.md` — the design conversation this takes lessons from |

---

## 0. The starting game comes first

The sandbox does not start empty. It starts as a **match**, and the building grows out of the match.

### Ochre vs Teal

Two squads of **Dabs** — small, round, paint-carrying units — fight over four **amber points**
in one cell of a real town. The tone is silly. The tactics are not.

| Sim thing | What it becomes in the match | Where it already exists |
| --- | --- | --- |
| `beacons` (default 4, even, 180°-mirrored pairs) | The four amber points. Each sits on a building the world is still guessing about. | `crates/lm-core/src/params.rs:78-82`, D61 |
| `beacon_radius_units` (4096 = 4 m) | How close a Dab must be to hold a point. | `crates/lm-core/src/params.rs:159` |
| `beacon_hold_ticks` | Points per tick held. The score. | `crates/lm-learn/src/metrics.rs:44` |
| `win_score` (1800) / `time_limit_ticks` (9000 = 300 s) | One round is **exactly five minutes**. | `crates/lm-core/src/params.rs:150-151` |
| A tag (contact within `tag_range_units` = 2048 = 2 m) | A dab of paint. The tagged Dab is stunned for `stun_ticks` (90 = 3 s) and **leaves a splat on the ground**. | `crates/lm-core/src/params.rs:100-102,157-158`; `assets/rules/beacons.rhai:11-15` |
| `wall_density_pct` / `cover_density_pct` | Real cover, cut from the cell's buildings and streets. | `crates/lm-core/src/params.rs:129-133` |
| `squads` 2 × `squad_size` 4 | Eight Dabs. Agent `squad_size * s` leads squad `s`, so agents 0 and 4 are the squad leaders. | `crates/lm-core/src/params.rs:87-91,152-153` |
| The talk gates (`order`, `ack`, `warn`, `query`, `report`) | Radio traffic. The addressee is checked against the roster, so a line can never name a stranger. | `assets/harness/default/gates.toml:175-186,105-107` |
| The reflex (`assets/reflexes/default.rhai`) | The Dabs' instincts when nobody is thinking. Charges with numbers, flees without, holds what it has. | `assets/reflexes/default.rhai:113-137` |

Tactics the match keeps intact: **cover** (the worldgen cover blocks), **flanking** (the tag
requires line of sight — `assets/rules/beacons.rhai:12-14`), **holding a point** (the beacons),
**orders** (the `order` gate, spoken on the radio), and **roles** (the leader/member split that
`voice.toml` already assumes — `assets/harness/default/voice.toml:42-49`).

Tone that is dropped: the grimness. Nobody dies. A tagged Dab is stunned, painted, and back up in
three seconds with a quip.

### The paint is the scoreboard, and the mirror-breaker

Two things make the paint more than decoration.

1. **Paint is the readable consequence of a tag.** The sim's tag state today is a `stunned_until`
   tick on an agent (`crates/lm-core/src/agent.rs:76-77`). The sandbox adds the second half of
   that consequence: the tag also writes a **splat** — a per-cell paint value with an owner — into
   the world state. So the thing that just happened to you is now a thing you can walk around, hide behind, and argue about. The map tells you what the round was.

2. **Paint breaks the mirror.** This is the gameplay risk, not a footnote. D63 measured that
   reflex-vs-reflex on the point-symmetric default map draws **63% of the time** for every
   `win_score` in 900..1800 and time limit in 9000..12000, and every draw ends with both squads
   reaching `win_score` on the same tick (`docs/design/decisions.md:77`). D63's own conclusion is
   that "decisiveness has to come from breaking the mirror" and that thinking is what should win
   it. **In the sandbox, paint is the mirror-breaker.** Splats accumulate asymmetrically, so the
   second half of a round plays differently from the first, and the seeded default reflex — which
   is deliberately symmetric — stops being a fair predictor of what a painted map will do. That
   is a real mechanic with a real reason to exist, not a skin.

### Why it is fun with no building at all

Because the round is already a game: two squads, four points, five minutes, paint on the ground,
and a rival that talks. **The build screen is not the game's front door; the match is.** The kid
who never says a word gets a complete, silly, five-minute paintball match with a rival that
complains. Everything after that is optional and additive.

### The one thing we are proposing that Otherhere may disagree with

Otherhere already has **amber beacons**: "Amber beacons mark buildings whose roof, floors, or
wall colour are still imagined" (`Otherhere/README.md:32`). We want the match's points to be
*those* beacons — so that holding a point means the building you were standing on gets
**sharpened**, which is Otherhere's own word for the world getting better where people play
(`Otherhere/README.md:3`). A paintball match would then literally be a way to make the town more
real, and the scoring fiction would be "how much of this block got figured out today".

That is a strong idea and it is not free: it overloads one marker with two meanings, and it
proposes that a match mutates Facts. It is **Open Question 2** in §9, and until Casey rules on it
the slice in §8 uses a **separate point marker** so that no fact is ever mutated by a match.

---

## 1. The first five minutes for a kid

A storyboard in words. The kid is called **Riley**. Riley has an iPad and no patience for
tutorials. Times are seconds from the kid's first touch.

### 0:00–0:20 — A real place, not a menu

The app opens on **Downtown Sitka — Lincoln Street** (`Otherhere/apps/client/public/places/sitka.json`,
`title: "Downtown Sitka — Lincoln Street"`, `id: pc_ci9yjekkdg`). It is the baked front door of
the real client, so it opens with no build, no login and no network (`Otherhere/README.md:11,114`).
The world is a stylized sketch: real streets, real footprints, buildings soft where the data is
guessing and crisp where it is sure (`Otherhere/README.md:24`).

Four amber points are drawn on buildings. Eight round Dabs are on the street — four ochre, four
teal — and the HUD says, once, in one line:

> **Sitka. Two teams, four points, five minutes. Push 🎙 to change the rules.**

That is the entire tutorial. Riley can play right now.

### 0:20–1:05 — Riley plays, and finds the paint

Riley grabs an ochre Dab and runs at the nearest point. Teal is already there. A teal Dab gets
close enough, and there is a **splat**: a bright teal blob on the street, an ochre Dab frozen for
three seconds, and a line in the radio log in teal's voice: *"Pfft."*

The splat stays. Riley walks around it. Riley walks *through* it.

> **Riley:** why is my team green?

This is the hook. The answer is not a menu; it is that the teal team painted Riley's team. The
game has already taught its own subject — **what happened to you is painted on the ground** —
without a single word of instruction.

### 1:05–1:35 — The first word

Riley holds the big round **🎙** button (bottom of screen, thumb-sized — the shape borrowed from
`OpenSkyFlight/docs/radio-chatbot-DESIGN.md` §2.1) and says:

> **Riley:** make the paint bouncy

On release, the transcript shows `YOU: make the paint bouncy` immediately — the words are on
screen the instant Riley stops talking, because the text is not waiting on anything.

Then the radio opens: static, and **"Bench is thinking…"** for one to three seconds. The game
keeps running the whole time. Nothing is blocked. This is the radio framing from the flight-sim
fork: *"a few seconds later, a chatbot responds like air traffic control or a co-pilot. Like
being on the radio"* (`OpenSkyFlight/docs/radio-chatbot-DESIGN.md` §1). The latency becomes
characterisation instead of waiting.

### 1:35–2:05 — Bench answers in a named change, not in code

> **Bench:** Bouncy paint. When you dab someone, the paint bounces off them instead of
> stopping them. That means no stun on a painted cell. I'll set `paint.bounce` on, and drop
> `stun_ticks` on painted cells from 90 to 0. Want to see what that touches?

Riley taps the little **▸ 2 fields** link and gets the honest version — the two named fields, the
two files, the before and after. Not source code. **The explanation is the diff, and it is short
enough to read.**

This is "Under the Hood" from the source conversation, done safely: the source proposed that the
kid watch a JavaScript file change (`src-conversation:302`); here the kid watches *two numbers
change, with their names on them*. The fun is the same and nothing can crash.

### 2:05–2:20 — The prediction comes before the proof

The Glitch Meter needle swings to **PREDICTING** and Bench says the prediction out loud, before
anything has been tested:

> **Bench:** I'll guess that tags go up — because now a hit doesn't stop you, so you'll trade
> more of them. At least 2 more tags on 4 maps I haven't shown you. If I'm wrong, we don't keep it.

This is the sealed prediction, and it is the spine of the whole design. `Prediction` is a
first-class record with a metric, a direction, a minimum delta, the held-out seeds and an author
(`crates/lm-learn/src/ledger.rs:37-54`), and the rule is that the change is applied **after** the
evidence, never before (D64, `docs/design/decisions.md:78`).

### 2:20–2:50 — The test, and the needle

Four held-out maps run headless. The needle moves to **HELD**, and the numbers appear:

> **4 / 4 maps moved as promised. Tags +2.4 on average. Stability 0.94.**

Underneath, in the small type the between-round screen already knows how to draw: what was
measured, what was proposed, what was promised, and only then what the test measured
(`docs/ACCEPTANCE.md:117-126`).

### 2:50–3:10 — The thumbs

Two big buttons: **👍** and **👎**. Not "Accept / Decline" — **thumbs**. This is the kid-facing
front end of the learning flow, borrowed from the radio chatbot, where a thumbs-up on a
response is *the* learning signal (`OpenSkyFlight/docs/radio-chatbot-DESIGN.md` §2.1, §2.2). The
flow's own call is adopted or refused; the three-way Adopt / Reject / Override screen
(`docs/ACCEPTANCE.md:127-133`) is one tap deeper, for the curious.

Riley thumbs up. The change lands. **"Adopted. The next round has bouncy paint."**

### 3:10–4:05 — The first laugh

The next round starts. Riley dabs a teal Dab — and the paint **bounces off them**, and Riley's
own Dab bounces too, sliding off sideways. The teal team spends a full round trying to hold a
point while sliding off it. From the radio, deadpan:

> **Bolt (teal):** "This is a slippery street."

> **Bench:** "That was your idea. 👍 was on you."

That is the first laugh, and it is **caused**, not scripted: Riley's own change produced the
comedy, Bench attributed it, and the attribution is the joke.

### 4:05–4:40 — The rival asks for something

Teal's leader — **Bolt** — speaks. Bolt is Bench wearing a different personality patch
(§4), and Bolt negotiates:

> **Bolt:** "I'll leave the north point alone if you turn bouncing off north. Deal?"

A **DEAL** button and a **NO THANKS** button. Riley taps DEAL. The deal becomes a *scoped*
dictionary block — `paint.bounce`, scope `point:north` — which goes through **exactly the same
bounds, the same sealed prediction and the same Adopt/Reject** as Riley's own change. A deal is
not a shortcut. It expires at the end of the round, so Bolt has to ask again next round.

### 4:40–5:00 — The undo is already there

Under the radio log, one small line:

> ↩ **put it back**  (reverts `paint.bounce`)

Riley has never been told about it. It is just there, next to every change, and it works.

The round ends. Five minutes. The screen that comes up is the one last-mile already knows how to
draw: the metrics, the proposal, the prediction, the verdict, and the three buttons
(`docs/ACCEPTANCE.md:94-142`). Riley looks at it for about two seconds and presses **NEXT ROUND**.

**What the kid learned in five minutes:** they changed a rule by talking, they were told what
would happen before it happened, they watched it be checked, they were asked what they thought,
they made a deal with the enemy, and they can take it back. Nobody said the word "parameter."

---

## 2. The core loop, and what each step is allowed to cost

```
  say ──▶ interpret ──▶ explain ──▶ predict ──▶ test ──▶ adopt ──▶ see ──▶ undo
   │           │            │           │         │        │        │       │
  ears      closed       Bench      sealed    held-out  the      the    inverse
 179 ms    choice      (System    prediction   seeds   human   round  deposit
           gate         Two)      (before)              thumbs
```

### The latency budget, from our own measurements

| Step | Measured | Budget | Source |
| --- | --- | --- | --- |
| Key up → line applied | **179 ms p50, 198 ms p95** (network 6 ms p50, compute 172 ms p50) | ≤ 250 ms | `docs/notes/ears/measurement.md:30,35` |
| Interpret: one closed choice gate on d1-2B | 20.3 ms (1 question) / 60.3 ms (3 questions) | ≤ 120 ms | `Otherhere/docs/adr/0019-local-decision-tiers.md` (latency table) |
| Interpret: last-mile's move pack on d1-2B | 162 ms; the full 11-question pack 473–1400 ms | keep the vibe pack separate from the move pack | `assets/harness/default/policy.toml:45-46`; D70 |
| Radio cover while the flow runs | — | **1–3 s**, diegetic | `OpenSkyFlight/docs/radio-chatbot-DESIGN.md` §2.2 |
| Measure + seal + test on held-out seeds | ~1.5–2 s at `--learn-ticks 300` | ≤ 3 s | `docs/ACCEPTANCE.md:110-111` |
| Bench's spoken line | pre-rendered bank, p50 0.64 s idle / 2.5–3.6 s busy; **never rendered inline** | bank hit ≤ 1 s | D73, `docs/design/decisions.md:87`; `assets/harness/default/voice.toml:6-10` |
| Apply + next round | the flow writes the header, the next round pins it | ≤ 1 round (5 s) | `docs/ACCEPTANCE.md:136-139` |
| **Key press → visible effect** | | **≤ 5 s** | |

### The rules that make those numbers true

- **The frame never waits.** The flow "runs on its own thread and the frame never waits for it:
  the app polls for the answer and shows the screen when it arrives"
  (`crates/lm-host/src/between_plugin.rs:5-8`). Same for the voice: a line that is not ready when
  due plays the existing tone and the miss is counted (D73).
- **The deadline stands in, and the round goes on.** On a busy shared box the same ear call that
  took 179 ms took 27–36 s; "the game's deadline is 5 s, so the ears are late and the stub stands
  in — exactly as the design says, and the round goes on"
  (`docs/notes/ears/measurement.md:63-68`). The same rule covers a slow Thor, a missing
  `communicator`, or a dead microphone: the round falls back to `Prosody::Absent` and never waits
  (D67, `docs/design/decisions.md:81`).
- **Only decisions travel.** The sim is the source of truth and the browser is a renderer. This is
  not a wish: cross-architecture determinism is proven on hardware — the Thor rebuilds the pinned
  golden hashes and passes `lm golden` OK 3/3, identical to the ProArt (D69,
  `docs/design/decisions.md:83`). So the sim can run on one machine and the kid can play on
  another, and the bytes still agree.
- **The undo is a change, not a rewind.** The between-round screen's Adopt / Reject / Override
  only covers *the current proposal* (`docs/ACCEPTANCE.md:127-133`). Undoing a change that is
  already adopted has to be a *new* change — an inverse deposit, filed with the same rigour. That
  is why the "↩ put it back" button in §1 files an inverse proposal rather than reaching into
  history.

---

## 3. The Vibe Dictionary v0

**The rule:** the model's output is never a program. It is an **index into this table** — a finite
list of typed changes, each with bounds. A closed choice gate over the block ids (the `choice`
shape, 1–255 options, `Otherhere/docs/adr/0013-question-packs-speak-jev.md`) maps the kid's words
to one block. An index that is not in the table is a **refusal**, never a fallback and never a
free-form patch.

The table lives in the harness as data — `assets/harness/default/vibe.toml` — because the harness
is pinned into the round log by the harness commit (D27, `assets/harness/default/gates.toml:20-21`)
and it is a git repo per agent (D21/D65).

### Blocks

Bounds are the *hard* limits. A block outside its bounds is refused at parse time, before any
model is asked. Tier **A** blocks are locked: a thumbs-up can promote a learned block (Tier B)
into the dictionary, but it can never widen a bound or unlock a locked block. That is the
anti-drift guard from the radio chatbot's design, applied to the dictionary instead of the
responses — *"Thumbs-up rewards what sounds good, not what's radio-correct"*, mitigated by a
two-tier store where safety-relevant content is locked (`OpenSkyFlight/docs/radio-chatbot-DESIGN.md` §3.3).

**Paint (11)** — the first tier, because the starting game is a paintball match.

| Block | Say it like | Typed change | Bounds | Touches |
| --- | --- | --- | --- | --- |
| `paint.bounce` | "make the paint bouncy" | `PaintBounce` on/off, scoped global or per point | bool; scope ∈ {global, point:0..3} | `vibe.toml` → new `paint_bounce` rule; `stun_ticks` for painted cells |
| `paint.sticky` | "make the paint sticky" | `PaintSticky` on/off | bool | new paint rule file |
| `paint.slow` | "make the paint slow you down" | `paint_slow_q8` — how much a painted cell drags | 0..256 (Q8) | new `paint_slow_q8` in `Params` |
| `paint.dry_time` | "make the paint dry fast" | `paint_dry_ticks` | 10..600 ticks | new paint param |
| `paint.splat_size` | "bigger splats" | `splat_radius_units` | 512..4096 (units = 1/1024 m) | new paint param |
| `paint.tag_stun` | "don't get stunned, just get painted" | `stun_on_paint` on/off | bool | `assets/rules/beacons.rhai` (tagging phase) + native oracle |
| `paint.erase` | "let me wipe the paint off" | `paint_erase_ticks` — how fast a splat fades when a Dab stands on it | 0..300 | new paint param |
| `paint.chain` | "paint should bounce off walls" | `PaintChain` on/off | bool | new paint rule file |
| `paint.team_color` | "our paint should be yellow" | `team_paint` | {ochre, teal, violet, saffron} | `assets/harness/default/voice.toml`-adjacent palette; new team palette |
| `paint.show_on_map` | "I want to see the paint on the map" | `paint_render` on/off | bool | display layer only — **must not enter Bundle bytes** (Otherhere ADR-0016) |
| `paint.stain_persist` | "leave the paint there forever" | `paint_persist` on/off | bool | new paint param |

**Units (13)** — the Dabs themselves.

| Block | Say it like | Typed change | Bounds | Touches |
| --- | --- | --- | --- | --- |
| `unit.speed` | "make them faster" | `speed_upt` | 32..512 units/tick (default 136) | `Params::speed_upt` — **BRANCH_FIELD** (`crates/lm-core/src/params.rs:37-46`) |
| `unit.slow` | "make them slower" | `speed_upt` lower | 32..512 | same, one field |
| `unit.size` | "make them bigger" | `unit_radius_units` | 256..2048 | new param |
| `unit.tag_range` | "let them tag from farther away" | `tag_range_units` | 1024..8192 (default 2048 = 2 m) | `Params::tag_range_units` — **BRANCH_FIELD** |
| `unit.stun_time` | "make the stun shorter" | `stun_ticks` | 0..300 (default 90 = 3 s) | `Params::stun_ticks` — **BRANCH_FIELD** |
| `unit.respawn` | "put them back right away" | `respawn_ticks` | 0..300 | new param; the respawn phase of `assets/rules/beacons.rhai:10` |
| `unit.vision` | "let them see farther" | `view_range_cells` | 4..64 (default 20) | `Params::view_range_cells` — **BRANCH_FIELD** |
| `unit.explore` | "let them wander" | `explore_temp_q16` | 0..65536 (default 0 = argmax) | `Params::explore_temp_q16` |
| `unit.squad_count` | "give me another squad" | `squads` | 2..4 | `Params::squads` |
| `unit.squad_size` | "more of us" | `squad_size` | 2..8 (default 4) | `Params::squad_size` |
| `unit.no_warning` | "don't warn them" | `ask_on_threat` off | bool | `assets/harness/default/policy.toml:90-92` |
| `unit.role_leader` | "the leader should be braver" | archetype patch weight on the `move` gate's retreat option | weight 0..255 | `assets/harness/default/gates.toml:52-73` (personality patch) |
| `unit.repath` | "let them change their minds faster" | `repath_ticks` | 5..300 (default 15) | `Params::repath_ticks` |

**Physics (9)** — how the world behaves.

| Block | Say it like | Typed change | Bounds | Touches |
| --- | --- | --- | --- | --- |
| `physics.walls` | "more cover" | `wall_density_pct` | 0..40 | `Params::wall_density_pct` |
| `physics.cover` | "more crates" | `cover_density_pct` | 0..40 | `Params::cover_density_pct` |
| `physics.points` | "more points to hold" | `beacons` | 4..8, **even only** | `Params::beacons`; `Params::validate` refuses an odd count (D61) |
| `physics.point_radius` | "bigger points" | `beacon_radius_units` | 1024..16384 (default 4096 = 4 m) | `Params::beacon_radius_units` — **BRANCH_FIELD** |
| `physics.smooth_move` | "smoother movement" | `lane_alpha_shift` / `lane_deadband_q16` | 0..8 / 0..65535 | both **BRANCH_FIELDs** |
| `physics.remember` | "let them remember what they saw" | `stale_window_ticks` | 5..600 (default 15) | `Params::stale_window_ticks` |
| `physics.no_jitter` | "stop the wobble" | `axis_swap_ticks` | 0..30 | `Params::axis_swap_ticks` |
| `physics.order_memory` | "orders should last longer" | `intent_ttl_ticks` | 5..600 (default 300) | `Params::intent_ttl_ticks` |
| `physics.rule_fuel` | "let the rules think harder" | `script_fuel` | 1000..100000 (default 10000) | `Params::script_fuel` — **BRANCH_FIELD**; the fuel is what actually bounds the work (`crates/lm-core/src/script.rs:47-51`) |

**Personality (9)** — archetypes, gate weights, marks and barks.

| Block | Say it like | Typed change | Bounds | Touches |
| --- | --- | --- | --- | --- |
| `persona.archetype` | "make the team more nervous" | swap the squad's archetype patch | one of the 8 archetypes below | `gates.toml` weights + `voice.toml` marks + `phrasebook.toml` barks |
| `persona.talk_more` | "talk more" | `ask_on_timer` on / `timer_period_ticks` | 30..600 (default 300) | `assets/harness/default/policy.toml:95-98` |
| `persona.voice` | "give them a different voice" | speaker per agent | {mia, jason, aria} + canonical ids | `assets/harness/default/voice.toml:42-49` |
| `persona.style` | "sound angrier" | tone style | {Default, Neutral, Calm, Angry, Happy} — **the style is the suffix of the voice id**; the short form `Mia.Calm` is HTTP 400, and Jason carries no Sad | `assets/harness/default/voice.toml:12-18,56-60` |
| `persona.marks` | "say it clipped" | marks profile | one of the six Vox Score families | `docs/design/vox-score.md:115-123`; the profile in `voice.toml` |
| `persona.reporting` | "they should report more" | weight on the `intent` gate's `report` option | 0..255 | `assets/harness/default/gates.toml:102,175-186` |
| `persona.channels` | "shout instead of using the radio" | `channel` per phrase | {radio, shout} | `assets/harness/default/phrasebook.toml:38,72` |
| `persona.callsign` | "call us the Ochres" | squad callsign | free string ≤ 16 chars | new field in the harness |
| `persona.courage` | "make them brave" | weight on the `move` gate's charge branch | 0..255 | `assets/harness/default/gates.toml:52-73` |

**World and radio (6)** — the ones that open the world up; these come *after* the paint.

| Block | Say it like | Typed change | Bounds | Touches |
| --- | --- | --- | --- | --- |
| `world.place` | "let's play somewhere else" | which Otherhere cell / place | a cell id the kid may see | Otherhere: a place file (`Otherhere/apps/client/public/places/*.json`) |
| `world.round_length` | "longer game" / "shorter game" | `time_limit_ticks` | 3000..18000 (default 9000 = 5 min) | `Params::time_limit_ticks` |
| `world.win_score` | "make it easier to win" | `win_score` | 300..6000 (default 1800) | `Params::win_score` |
| `world.tick_rate` | "make everything faster" | `tick_hz` | 10..60 (default 30) | `Params::tick_hz` |
| `radio.quiet` | "quiet match" | all talk triggers off | bool | `assets/harness/default/policy.toml:82-98` |
| `radio.undo` | "put it back" | file an inverse deposit | one block at a time | `crates/lm-learn/src/deposits.rs:7-14` |

**The eight archetypes.** Each one is a patch over three surfaces at once — gate weights, Vox Score
marks, and barks — which is what "personalities as patches" has to mean if it is to be more than a
name. All eight are original.

| Archetype | What it is | Gate shift | Marks | Bark |
| --- | --- | --- | --- | --- |
| **Flinch** | the anxious one; freezes under contact, over-reports | `warn` and `retreat` up | rushed · shaking · rising | *"They saw me. They saw me."* |
| **Ledger** | the middle-manager warlord; never advances without a report | `hold` and `report` up | level · measured · falling | *"Status. Status. I want status."* |
| **Sprig** | fast, poor at holding, always chasing | charge up, `hold` down | brisk · clipped | *"Moving. Moving."* |
| **Anchor** | plants itself and never leaves | `hold` up hard | level · steady · even | *"Holding. Still here."* |
| **Echo** | repeats the squad's last line; great on the radio, useless alone | `ack` up, `command` down | even · flat · drawn | *"Copy. Copy."* |
| **Midas** | paints everything and ignores the objective | charge up, `hold` down | strong · pressed · shouted | *"Hit. Hit again."* |
| **Tideline** | wipes the splats it walks past | `report` up, `command` down | soft · even · measured | *"I'll sweep it."* |
| **Gull** | chases the nearest thing that moves and announces it | `warn` up hard | lifted · rising · questioning | *"Something moved. Over there."* |

### Two properties the table must have, and how we get them

**It must be small enough to be reliable.** The dictionary is the closed choice of a gate whose
options are the block ids. D62 measured d1-2B right on `action` **96% of the time over 200 fresh
paired scenes** at a 0.60 confidence gate (`docs/design/decisions.md:76`) — but that was over a
small option set with a rule stated over the facts. **A 48-option vocabulary is a different
measurement and we have not made it.** That is Open Question 6 and it has an exit: the gate is
scored per block on a labelled set of utterances, and any block the model cannot reach reliably is
*not* in the dictionary — it stays a button.

**It must be checkable without a model.** Every block's bounds are enforced before the model is
consulted, and the whole dictionary is data in a TOML file that a test can read. This is the same
move as the geometry DSL being JSON and interpreted by a trusted interpreter
(`Otherhere/docs/adr/0005-geometry-dsl-is-json.md`, `Otherhere/AGENTS.md:9`).

---

## 4. The co-developer and the rival

Two voices, one mind, and the division between them is the three-tier split last-mile already
uses — engine, gamepad, director (`docs/design/vox-score.md:171-179`).

### Bench — the co-developer

**Bench is System Two as director.** The tier table puts it plainly: tier 2 is "the leader, the
learning flow, a human", and what it controls is "**the patch and the intent**"
(`docs/design/vox-score.md:179`). Bench is the director of the game the kid is building.

What Bench says, always, in this order:

1. **The name of the change.** `paint.bounce`. Not "I have updated the parameter".
2. **What it will touch**, in plain words and in two numbers. "Two fields: `paint.bounce` on, and `stun_ticks` on painted cells from 90 to 0."
3. **The prediction**, before the test. Always.
4. **The verdict**, with the per-map deltas.
5. **The ask.** One thumb.

When Bench explains, unprompted: when the prediction **failed** (the most important time — the
kid has to learn that the game checked and disagreed); when the change is **bigger** than the last
one; or when the kid asks "why". Otherwise it stays quiet, because a co-developer who narrates
every step is a co-developer nobody listens to.

**Bench can never write code.** It can only choose a block from §3. When Riley asks Bench to do
something that is not in the dictionary, the honest answer is a *question*, not a workaround:

> **Bench:** "I don't have a word for that one yet. I'll write it down — it's in your request
> pile, and if it shows up enough I'll learn it."

That is not a failure mode, it is `Deposit.questions` — the field that already exists for exactly
this (`crates/lm-learn/src/deposits.rs:12`). The kid's idea is never lost; it is queued.

**Bench's voice** is one speaker in the harness, pre-rendered into the content-addressed bank and
never rendered inline (D73). Bench is the *director*, so it speaks in the plain register of
`docs/design/vox-score.md:131`: the between-round screen shows how the change was said, beside
what was said, and the kid can correct the reading.

### Bolt — the rival leader

**Bolt is Bench wearing a different personality patch.** Squad Teal's leader. Bolt is not a script
and not a chatbot with an intent table — Bolt is an agent with an archetype patch, speaking
through the same gates, the same phrasebook and the same voice bank. That means Bolt's lines are
already covered by the machinery: the `attention` gate checks the addressee against the roster
(`assets/harness/default/gates.toml:105-107`), the `intent` gate chooses order/ack/warn/query/
report, and the phrasebook holds the wording (`assets/harness/default/phrasebook.toml:37-109`).

The difference is *what Bolt wants*: not the points. Bolt wants the kid to change the rules in a
way that flatters Bolt's squad. A **Bolting offer** is a typed structure:

```
offer {
  give:  block_id,      // e.g. paint.bounce
  scope: point:1,       // where
  want:  block_id,      // e.g. unit.tag_range
  scope: global,
  expires_at: end_of_round
}
```

### How a negotiation works mechanically

1. **Bolt opens.** At a round boundary, Bolt's System Two produces at most **one** offer per
   round. One. It is spoken on the radio, in Bolt's voice, with the trade stated in one sentence.
2. **Riley answers** with two buttons: **DEAL** / **NO THANKS**. (Or by voice — the same closed
   vocabulary maps "deal" to the accept option.)
3. **A deal is a scoped block.** Accepting Bolt's offer writes a block with a *scope* — the block
   the kid gave, narrowed to the scope Bolt asked for. It is then put through **the whole rigour**:
   bounds check → sealed prediction → held-out test → Adopt / Reject. A deal is not a shortcut and
   it cannot install a block the kid could not have installed themselves.
4. **It expires.** At the end of the round the scoped block lapses. Bolt must re-ask next round.
   The map therefore changes hands of concessions, which is a nice rhythm and stops the game from
   becoming one long negotiation.
5. **Refusal is a line.** Riley can refuse, and Bolt says something about it. Bolt's refusals and
   acceptances are marked in the Vox Score vocabulary, so the radio log shows how the negotiation
   felt.

**What Bolt may never do:** offer a block that is Tier A locked, offer a scope that is not in the
bounds table, or offer anything that changes the ledger's rules. Bolt is a negotiation partner,
not an authority.

### The co-developer and the rival do not fight

One rule, and it is the reason this works: **Bench never lies to Riley about Bolt, and Bolt never
edits the code.** Bench will say "Bolt wants the paint off the north point because it's hurting
Bolt's route." It will not say "Bolt is good." The split is: Bench owns the *truth about the
game*, Bolt owns the *negotiation about the rules*, and Riley owns both.

---

## 5. The Glitch Meter

### What it measures

The Glitch Meter is a readout over the ledger, not a new subsystem. The sealed-prediction
machinery already produces everything a meter needs:

- `Prediction { round_id, proposal_hash, metric, direction, min_delta, eval_seeds, author, text }`
  — `crates/lm-learn/src/ledger.rs:37-54`
- `Verdict { seal, deltas: Vec<(u64, i64)>, adopted, override_reason }` —
  `crates/lm-learn/src/ledger.rs:58-68`
- `Entry::Override { seal, adopted, reason }` — a human deciding against the evidence, logged
  — `crates/lm-learn/src/ledger.rs:79-86`

So define, for the current proposal:

```
stability = ( number of held-out seeds whose delta moved in the predicted
              direction by at least min_delta ) / ( number of eval seeds )

glitch     = 1 - stability
```

and report the **spread** as well as the mean, because a change that moves 4 maps by +2, +2, +2
and +9 is not the same change as one that moves them by +3, +3, +3, +4, even though both score
1.00.

The meter is drawn in the place the between-round screen already exists
(`docs/ACCEPTANCE.md:117-126`), and mirrored as a small gauge in the play HUD during the round —
the "sync the physical and generative Glitch Meter UI" idea from the source conversation
(`src-conversation:497,622`), landed in a screen we already have.

### What a "safe glitch" is

The source conversation's Glitch Meter was a fear meter: chaotic code was *bad* and the player had
to keep it above 80% (`src-conversation:294-300`). We invert that, because our code cannot
produce chaos — but our *rules* can produce surprise.

**A safe glitch is a change that is stable but strange.** Formally, a change is a safe glitch when
all five of these hold:

1. `stability ≥ 0.75` across the held-out seeds.
2. **No seed regresses** the predicted metric.
3. The change is inside its dictionary bounds (checked at parse time, so this is free).
4. The round still produces a **decisive** outcome — not a draw. This is D63's real lesson: 63% of
   symmetric rounds draw, and a change that raises the draw rate is a bug, not a glitch
   (`docs/design/decisions.md:77`).
5. The change costs no more than the fuel budget: a rule that would exceed `script_fuel` is
   refused *before* it is applied. The fuel is what actually bounds the work, not the nesting
   depth (`crates/lm-core/src/script.rs:47-51`).

A **glitch** in the game's sense is then: *the game is allowed to render a consequence it chose.*
The paint bounces, and the game says so out loud and attributes it. The game does not invent
behaviour; it *shows* behaviour that is already bounded, measured and predicted.

### The budget on glitchiness

At most **two** safe-glitch behaviours may be active at once, and each is bounded by its own
dictionary block. This is a game-design constant, not a technical one — it is the same kind of
limit as "the 10 talk gates are ~80% of every thought's cost" (D70): a thing that is cheap
individually is expensive in aggregate, and the aggregate is what the round's clock notices.

### Why the thumbs-up is not allowed to break it

The thumbs-up is the kid-facing front end of the learning flow — but a popularity signal must
never be able to rewrite a bound, unlock a Tier A block, or promote a learned phrase into a
safety-critical one. That is the radio chatbot's anti-drift guard exactly
(`OpenSkyFlight/docs/radio-chatbot-DESIGN.md` §3.3), and it is also D59's "find hard, check
cheap": a decision may carry a short, code-checkable certificate, checked before apply; a failed
certificate is a refusal (`docs/design/decisions.md:71`).

So the rule is: **a thumbs-up can make a block *easier to reach*. It can never make a block
*stronger*.** Promotions go Tier B → Tier A, one way, and only after a person or the flow has
checked the evidence.

---

## 6. Otherhere as the stage

### The first place: Downtown Sitka — Lincoln Street

`Otherhere/apps/client/public/places/sitka.json` — `id: pc_ci9yjekkdg`, `kind: place`,
`scope: public`, `ownerIsKid: false`, `privacy: { blurred: true }`, 7 cells, 1649 features,
4727 facts. It is the client's baked front door
(`Otherhere/README.md:11,114`), so the sandbox opens with **no build and no network**.

The names in it are real, and they are the stage's texture: **Lincoln Street, Harbor Drive,
Katlian Street, Lake Street, Race Street**, and the buildings **Sitka Lutheran Church, Saint
Michael's Russian Orthodox Cathedral, Saint Gregory Nazianzen Catholic Church, Alaska Native
Brotherhood, Baranof Island Housing Authority, Municipal-State Office Building, Pacific High
School, Coldwater Restaurant Deck**.

Two reasons this place and not somewhere else:

- **It is already shipped.** A baked place opens from a URL (`Otherhere/README.md:63`). The
  sandbox's first round needs no ingest, no Overpass, no build.
- **It is the same Sitka as Sitka Skies.** Magnus's fork is home-based in Sitka, Alaska, taking
  off from Sitka Rocky Gutierrez Airport (`OpenSkyFlight/README.md`). A shared Sitka universe is
  explicit in the brief: the kid who has flown over Baranof Island in Sitka Skies can walk
  Lincoln Street in the sandbox and know where they are. That is worth a lot, and it costs nothing.

### The sim grid onto a real place

| | last-mile | Otherhere |
| --- | --- | --- |
| Grid | 8 × 8 chunks of 32 m = **256 m square**, nav cell 2 m (`crates/lm-core/src/params.rs:72-77,146-148`) | a **Cell** is an H3 res-9 cell, ~0.1 km², ~200 m edge (`Otherhere/docs/adr/0001-h3-resolution-9-for-cells.md`) |
| Size | 65 536 m² | ~100 000 m² |
| A walk | the 7 cells around you | centre + 1 ring = 7 cells, ~700 m across (ADR-0001) |

So **one round is a 256 m window onto one Cell** — about two thirds of the Cell's area — and the
Cell is the round's address. The window is anchored to the Cell centre and the overhang is
clipped at the boundary; the exact anchoring rule is Open Question 3.

**How the place becomes a map.** The Cell's facts give the buildings, the roads and the named
places (`Otherhere/packages/core/src/geo/h3.ts`, `Otherhere/packages/core/src/schema/cell.ts`). The
sim's nav grid is 2 m cells; the Cell's features are rasterised onto that grid — roads become
walls and lanes, buildings become cover blocks. `wall_density_pct` and `cover_density_pct` already
exist as worldgen parameters (`crates/lm-core/src/params.rs:129-133`), so cover is a first-class
thing the dictionary can turn up and down.

**Why this is honest.** A place is facts plus recipes (`Otherhere/README.md:21-26`); the map is
facts plus a rasteriser. The rasteriser is derived, so it is regenerable, and it never enters the
Bundle bytes if it is drawn as a display layer (Otherhere ADR-0016).

### The axial hex grid idea (nugget N4)

The nugget is the Eisenstein A₂ hexagonal lattice as an alternative substrate: a hex grid has no
diagonal, so distance is a clean norm and "flanking" has an honest geometry. The question is
whether to build a new A₂ lattice or use the one already in the data.

**Use the one already in the data — but say precisely what it is.** H3 is *not* an axial lattice:
it is a **hierarchical hex index** with a base-16 child/parent scheme, and Otherhere's own
vocabulary deliberately forbids calling a Cell a "hex" (`Otherhere/CONTEXT.md:11` — for Cell,
"_Avoid_: tile, chunk, hex"). What h3-js 4.5.0 *does* already expose is the part a hex sim
actually needs: a **local i/j coordinate frame** per cell (`cellToLocalIj` / `localIjToCell`),
`gridDistance` (hex distance) and `gridDisk` (the k-ring)
(`Otherhere/packages/core/package.json:15`; verified against the installed 4.5.0). That local frame
*is* axial-shaped — it is the classic i/j basis over one cell — and Otherhere already computes
geometry in a **local metric frame centred on the Cell**, in metres, with deterministic trig and
millimetre quantisation
(`Otherhere/docs/adr/0006-determinism-via-local-frames-and-quantization.md`) — which is exactly
the frame a local hex lattice would want.

So the recommendation is:

- **v0 keeps the square 8-connected grid.** The sim's motor, the collision audit (audit N6:
  "more than one cell per tick would let the motor step over walls",
  `crates/lm-core/src/params.rs:237-238`) and the reflex's Chebyshev distances
  (`assets/reflexes/default.rhai:31-33`) are all square-grid assumptions. A hex substrate is a
  substrate change to the motor, not to the data.
- **When the substrate is swapped, it swaps to H3's own local i/j frame**, not to a new A₂
  lattice. That gives hex distance for free, keeps the place lookup free, and keeps the
  quantisation discipline Otherhere already has. A new Eisenstein lattice would mean building a
  second index for a grid that already exists in the data.
- The nugget's real lesson survives the swap either way: **hex distance makes flanking honest**,
  because on a square grid "around" and "through" are the same eight moves, and the two are
  indistinguishable to a reflex.

### Legible render tiers: the 142 m lesson

This is the section that has to be right, because Otherhere already paid for it.

**What the ladder is.** `SPLAT_NEAR_M = 140` (`Otherhere/apps/client/src/assets.ts:78`), and
`chooseTier` returns the gaussian splat under 140 m and the mesh beyond it, with the impostor and
the photograph as the fallbacks
(`Otherhere/apps/client/src/assets.ts:145-156`). The four tiers are `splat`, `model`, `impostor`
and `card` (`Otherhere/AGENTS.md:21-27`); the real capture in the Sitka place file carries all
four with real byte counts — splat 193 512 B, model 169 132 B, impostor 70 214 B, card 26 360 B
(`Otherhere/apps/client/public/places/sitka.json`, the `asset.capture` fact for
`osm:w116029214`, Saint Michael's).

**What the lesson is.** Round 17 stood six poses at the cathedral and recorded which tier the
game fetched. The A/B is the first two rows: the same bearing, **4 m apart** — 138 m fetched the
splat, 142 m fetched the mesh, 6/6 runs clean
(`Otherhere/docs/playtests/round-17.md:18-31`). And then the finding:

> "The mesh is fetched, verified and drawn — and it is still not legible. At 142 m the cathedral's
> mesh is behind the town's hill from most bearings, or a few dozen pixels where it is not. […]
> This is the round's honest negative and it is not a small one: **the tier is correct, verifiable
> and invisible.**" — `Otherhere/docs/playtests/round-17.md:50-59`

**The design consequence, and it is a hard one:** *the asset tier boundary is not a legibility
boundary, and the sandbox must not use it as one.* A mesh can be fetched, hash-verified, correctly
transformed into its building's frame, drawn without error — and be invisible. So:

- **The gameplay legibility ladder is separate, and it is measured in what the player can act on.**
  Proposed three rungs: **Near (splat)** — inside the point's radius; **Mid (card)** — the point's
  silhouette, flat and readable; **Far (impostor)** — a mark. The rung boundaries are the *player's*
  distances (how far can I see a teammate? how far can I read a point?), not the client's fetch
  distances. This is not an invention: round 17 left the *same* question open for the asset ladder
  — "Either the ladder gets a third band (`model` near, `impostor` beyond), or the mesh is admitted
  into the near field and the splat kept for the first few metres" — and recorded it as a design
  decision, not a bug (`Otherhere/docs/playtests/round-17.md:118-123`). And it names the cause
  outright in the next item: "it has not been proved by a frame taken from in front of the
  building, because **the range the tier is drawn at does not put the building in front of the
  camera. The likely cause is the tier's range, not its placement**"
  (`Otherhere/docs/playtests/round-17.md:124-128`). The sandbox's ladder is the
  same idea applied to what the *player* must read, and it is deliberately a different axis.
- **A precedent for what a legibility ladder looks like.** The forest display layer already has
  one: "every tree within `FULL_M` (450 m), every 2nd to `MID_M` (950 m), every 4th beyond", with a
  hysteresis so chunk boundaries never flicker (`Otherhere/apps/client/src/forest.ts:60-92`). The
  bands are chosen for what you can *see*, not for what can be fetched. The sandbox's point rungs
  should be cut the same way.
- **The test is a screenshot, not a fetch.** A rung is "legible" when a reviewer can name the
  building from a one-frame capture at that distance — the same discipline the round-17 probe
  used when it found the mesh's absence. A tier that fetches successfully has proved nothing about
  being seen.
- **One source of truth.** Round 17 also found that the ladder sentence had been written wrongly
  in *four* places — ADR-0023, `AGENTS.md`, `Otherhere/docs/proart-handoff.md` and two doc comments — while
  the code said something different and defensible ("it outranks the impostor wherever it
  exists"); all four were corrected
  (`Otherhere/docs/playtests/round-17.md:60-69`). So the sandbox's own tier ladder must be
  **generated from `chooseTier`**, not written out by hand, or it will drift exactly the same way.
- **One consequence for the photo pipeline to know:** "a capture with a `model` tier never shows
  its photograph again" (`Otherhere/AGENTS.md:26-27`) — so once a kid photographs the cathedral, the
  photograph is never seen again at any distance. For a game where "look at this building I just
  photographed" is a *reward*, that is a design collision, and it is Open Question 4.

### Three.js client vs Bevy host

| | Three.js browser client | Bevy host |
| --- | --- | --- |
| Exists today? | **Yes** — Otherhere's client is Vite + Three.js, WebGPU with a WebGL2 fallback (`Otherhere/README.md:81`, `Otherhere/docs/BRIEF.md:55`) | **Yes** — last-mile's host is Bevy 0.20.1 + bevy_egui 0.43.1 (`crates/lm-host/Cargo.toml:18-23`), with a 3D view (`crates/lm-host/src/view3d/`) and a piano-roll HUD |
| Frame budget | measured on Apple Silicon: **83.8 fps avg / 47.8 p5 / 17.0 p1** on M4 Max Chrome; **37.7 / 32.3 / 30.3** on M4 Pro Safari (`OpenSkyFlight/BENCHMARKS.md`) — Safari's p1 is *higher* than Chrome's, i.e. steadier | native, comfortably 60 Hz |
| iPad / no install | **Yes** — this is the whole point (`OpenSkyFlight/README.md`: "no install, runs in your browser (iPad + desktop)") | **No** — it is a desktop binary (`docs/ACCEPTANCE.md:13`: `lm-host.exe`) |
| Can it be the source of truth? | No — but it does not need to be | Yes, and it should be |
| Latency to the sim | over the wire, so the wire protocol matters | local |

**Recommendation: Three.js in the browser, with the Bevy host kept as the authoring console.**

The argument is not "the browser is prettier". It is three constraints that the browser satisfies
and the host cannot:

1. **The kid's device.** Sitka Skies' requirement is browser + iPad, no install
   (`OpenSkyFlight/README.md`), and the brief names it explicitly. A Bevy host is a desktop
   binary; a kid on an iPad cannot be the one holding it.
2. **One world, two games.** The sandbox is Otherhere. Otherhere's client already renders Otherhere.
   A second renderer would mean a second world.
3. **Only decisions travel.** Determinism across architectures is proven on hardware (D69), so the
   sim can stay where it is — on the Thor, in a container, on the ProArt — and the browser is
   *a renderer*, exactly the role Otherhere's ADR-0021 already gives the ProArt: "the Thor hosts
   models and generates assets; the ProArt renders them" (`Otherhere/AGENTS.md:33-38`).

So the split is: **Bevy host = the bench** (the between-round screen, the piano roll, the Glitch
Meter, the Under-the-Hood diff panel — an authoring console for an adult or an advanced kid),
**Three.js client = the street** (the match, the radio widget, the paint — what a kid plays).
`crates/lm-render` is already Bevy-free (`crates/lm-render/src/lib.rs:1-2`), so the geometry is
available to both sides of that split.

The cost, stated plainly: the browser cannot be the source of truth for the round log, the ledger
or the state hashes. So the browser must be able to **render a replay**, and it can — `lm replay`
checks every hash the log recorded, and the piano-roll viewer already exists
(`docs/ACCEPTANCE.md:172-192`).

---

## 7. Safety and privacy for kids

### The voice never leaves the machine, and is never stored

The ears path is a **local sidecar on the LAN machine**, not a cloud service: `communicator` runs
on the ProArt as an HTTP sidecar, the client is `lm_voice::ProsodyService`, the round falls back
to `Prosody::Absent` when the service is absent, and "the mic audio never leaves the machine, and
a local sidecar needs no key and no LAN hop" (D67, `docs/design/decisions.md:81`). The
measurement confirms it: the network p50 is **6 ms** and the p95 is 11 ms
(`docs/notes/ears/measurement.md:36-37`) — that is a local hop, not a round trip to a datacentre.

**But recording is a capture, and Otherhere has a rule about captures.** "Private by default.
Photos are never stored; only Facts derived from them" (`Otherhere/AGENTS.md:54`,
`Otherhere/docs/BRIEF.md:44`). So:

- The **mic buffer is never written to disk** and never sent to a server.
- Only the **derived transcript** is kept, and only where the flow needs it: `Deposit.said` — "the
  human's words, verbatim" (`crates/lm-learn/src/deposits.rs:9`) — in a local file under
  `runs/deposits/` (`docs/ACCEPTANCE.md:134-135`). It is on the player's own machine.
- The transcript is **never** used as training data, and the closed dictionary means there is
  nothing else free-text to harvest.
- The kid's account is never `public` scope — Otherhere's rule for kid accounts is exactly this
  (`Otherhere/docs/BRIEF.md:44`).

**One honest caveat.** The design above is only as good as the deployment: if the ears are ever
replaced by a cloud recogniser, every one of these claims inverts. The rule therefore has to be
**enforced by the build, not by the doc**: the recognition service's address is a config value with
a default of `localhost`, and a non-local address is refused without an explicit parent opt-in.
The flight-sim fork made the same disclosure choice — "Chrome streams audio to cloud; Safari can
do on-device. Disclose in a footnote, not a popup" (`OpenSkyFlight/docs/radio-chatbot-DESIGN.md`
§2.3) — and we take the *disclosure* from it and strengthen the *default* from Otherhere.

### Prompt filtering happens through the closed vocabulary

There is no prompt filter in the classic sense, and that is the point. **The model is never asked
to do anything with the words.** It is asked a closed question: "which of these 48 blocks does
this match?" The words cannot become code because they are never given to anything that writes
code.

Four consequences, all of them load-bearing:

1. **Off-vocabulary is a refusal, not a fallback.** "I don't have a word for that one yet" and the
   request goes into the pile (`crates/lm-learn/src/deposits.rs:12`). No partial patch, no "close
   enough" block.
2. **Negation submits nothing.** The ears already refuse a dead microphone, a negation in the
   sentence and a locked verb (D67). So *"don't make the paint bouncy"* submits nothing — which is
   the correct semantics, and it is the existing behaviour rather than a new filter.
3. **Three independent walls, and the kid sits behind all three.**
   - last-mile's Rhai sandbox: no floats, a fuel budget, and a contract that answers only `()`,
     `#{ "goto": [x, y] }` or `#{ hold: true }` (`crates/lm-core/src/script.rs:8-17,190-208`).
   - Otherhere's invariant 3: "No executing model-written code. Generators and rulesets are data
     in constrained DSLs, run by our trusted interpreter. No `eval`, no dynamic imports of
     generated code." (`Otherhere/docs/BRIEF.md:42`, `Otherhere/AGENTS.md:9`)
   - **The Vibe Dictionary itself**: the model outputs an index into a finite table, and an index
     is not a program.
4. **A bound is checked before a model is consulted.** The kid cannot widen `stun_ticks` past 300
   by any route, because the parser refuses it before the gate is asked.

### What a parent or a teacher sees

A **parent card**, one page, refreshed weekly, on the parent's device:

- Blocks tried, adopted, refused — counts, not transcripts.
- The stability of each adopted change.
- How long the child played and what they unlocked.
- **Nothing the child said**, unless the parent turned on transcript retention — which is off by
  default and is itself a toggle with a plain-language label.

A **teacher mode**, which is the between-round screen with the story taken off: the metrics grid,
the proposal, the sealed prediction, the per-seed deltas, the verdict and the three buttons
(`docs/ACCEPTANCE.md:117-133`) — the same screen, unromanticised. A teacher can read a child's
entire learning history as a ledger, because the ledger is hash-linked and every link is a file
(D64, `docs/design/decisions.md:78`).

**A fail-state, defined.** If the dictionary's gate starts refusing most of what Riley says, the
kid's experience is a game that ignores them — which is worse than a crash. So the gate's refusal
rate is a monitored number: if the refusal rate over a session crosses a threshold, the game
**offers the buttons** — the block list, tap to apply — rather than pretending the voice is
working. The text fallback is not optional either: it is the same gate over typed input, and it
is always present (`OpenSkyFlight/docs/radio-chatbot-DESIGN.md` §2.3: "always offer text
fallback… Feature-detect; hide mic where unsupported, keep text input").

### Nothing about the child is a training target

The closed vocabulary is the privacy story. There is no free-text corpus to scrape, because the
only free text is the transcript, which we do not store by default, and which is never used as
training data in any case. A kid can say anything at all and the worst thing that can happen to
the world is a line in a local deposit file saying "I asked for something we don't have yet".

---

## 8. The smallest playable slice (M2 candidate)

**One place. Ten dictionary blocks. One rival move. The existing sim.**

### The ten blocks

`paint.bounce`, `paint.slow`, `paint.splat_size`, `paint.dry_time`, `paint.tag_stun`,
`unit.speed`, `unit.tag_range`, `unit.stun_time`, `physics.cover`, `persona.talk_more`.

These are the first ten because they are (a) all in the paintball match, (b) all measurable on
existing metrics (`tags_for`, `tags_against`, `beacon_hold_ticks`, `scores` —
`crates/lm-learn/src/metrics.rs:38-44`), and (c) the first five are exactly the paintball
vocabulary a kid would reach for.

### The one rival move

**Bolt offers one scoped block per round.** One offer, one scope, one expiry. Mechanically it is
a `vibe` proposal with `scope` set, which goes through the same bounds check, the same sealed
prediction and the same Adopt/Reject as the kid's own changes.

### Tasks, by crate and by Otherhere piece

**`lm-core` (Rust) — the paint becomes state.**
- Add paint to the world state: a per-cell `paint_q8` coverage and a `paint_owner`, alongside the
  nav grid. Keep it fixed-point — the crate denies float arithmetic
  (`crates/lm-proto/src/lib.rs:15-19` is the same discipline in the wire crate).
- Add the paint parameters (`paint_bounce`, `paint_slow_q8`, `paint_dry_ticks`,
  `splat_radius_units`, `stun_on_paint`, `paint_erase_ticks`) to `Params`, with bounds in
  `Params::validate` (`crates/lm-core/src/params.rs:183+`).
- Make the **tagging phase** write a splat as well as a stun: `assets/rules/beacons.rhai:11-15`
  and the native `rules::beacons` oracle — and the two must agree exactly, because "the native
  ruleset is the oracle: this script must produce exactly the same events, the same agent states
  and the same canonical hashes, tick for tick" (`assets/rules/beacons.rhai:3-6`). **Both sides, or
  it is a bug.**
- **Hash the paint into the state hash.** If paint is not in the hash, a replay with paint is not
  the same round, and `lm replay` will not catch it
  (`crates/lm-core/src/params.rs:338-352` is where the params are hashed; the world hash is
  adjacent).
- Extend the reflex's score view with the paint layer so a rule can read it, without widening the
  answer contract (`crates/lm-core/src/script.rs:16-17`).

**`lm-agents` — the closed gate over the dictionary.**
- A `vibe` gate: `type = "choice"`, `options_from = "fixed"`, one option per block id. It goes
  in `assets/harness/default/gates.toml` beside the existing gates.
- Per D70, the vibe pack is **not** on the move pack: it is asked only when the kid speaks
  (`assets/harness/default/policy.toml:72-98` is the template for a second pack). One question,
  d1-2B, ~20 ms (`Otherhere/docs/adr/0019-local-decision-tiers.md`).
- Keep the 0.60 action confidence gate and the per-gate confidence rule from D72: one unsure gate
  removes only that gate's decision, and an unsure vibe gate refuses the change without touching
  the move (`docs/design/decisions.md:86`).

**`lm-learn` — the proposal becomes a block.**
- A `vibe` proposal kind carrying `(block_id, scope, bounds_check)`. The sealed prediction uses
  the existing `Prediction` struct unchanged (`crates/lm-learn/src/ledger.rs:37-54`) — a block's
  predicted effect is a metric and a direction like any other.
- The between-round screen shows the **block name**, the **fields it touches**, the **bounds**,
  the prediction and the per-seed deltas. It already shows metrics, proposal, prediction and
  verdict (`docs/ACCEPTANCE.md:117-126`); the block name is the new row.
- An **inverse deposit** for "put it back", using the existing `Deposit` shape
  (`crates/lm-learn/src/deposits.rs:7-14`).

**The harness (data, not code) — `assets/harness/default/`.**
- `vibe.toml` — **new**. The dictionary: every block, its type, its bounds, the field it touches,
  its tier (A locked or B learnable), and the phrasebook keys it draws its bark from.
- `gates.toml` — add the `vibe` gate and the `vibe_present` presence gate (the pattern is
  established: "a noul of about zero means the line is not spoken at all — silence is an
  outcome" `assets/harness/default/gates.toml:77-80`).
- `policy.toml` — add the `[vibe]` pack: asked when the kid speaks, with its own budget.
- `voice.toml` — add **Bench** and **Bolt** as speakers, with their tone styles. Bench uses a
  distinct speaker so the kid can tell "the co-developer" from "the enemy" without being told;
  Bolt's is the archetype patch's voice. Remember: the style is the **suffix** of the voice id and
  the palette is **per speaker** — Jason carries no Sad (`assets/harness/default/voice.toml:12-18,56-60`).
- `phrasebook.toml` — add the `Bench.*` and `Bolt.*` families. The existing structure already has
  `Command.*` and `Question.*` families with a `channel` and `templates`
  (`assets/harness/default/phrasebook.toml:37-109`), so these are new families, not a new format.

**Otherhere — the place and the paint.**
- A **Ruleset** file. Otherhere's glossary already has the word: "Ruleset: the game being
  played. It defines objectives, simulation, and fog behavior"
  (`Otherhere/docs/BRIEF.md:31`), and the place Capsule schema **already has the slot**:
  `ruleset: Ruleset.optional()` and an `arena` field beside it
  (`Otherhere/packages/core/src/schema/capsule.ts:39-43`). The paintball match is a Ruleset; the
  walk is another. This is a fill-in, not an invention — which is the cheapest possible version
  of "the sandbox grows out of the game".
- The **paint display layer**: splats drawn over the nav grid. It must be a display layer and
  **must not change Bundle bytes** (Otherhere ADR-0016, `Otherhere/AGENTS.md:10-17`), or every
  bundle hash in the world breaks.
- The **point markers**: four amber marks. If Open Question 2 resolves in favour of reusing the
  quest beacons, this is free; otherwise it is a new marker that must look clearly different from
  Otherhere's quest amber.
- The **fog**: the match inherits Otherhere's own match rule — "enemies stay hidden unless a
  teammate is within 45 m (team fog)" (`Otherhere/README.md:67`) — which lands almost exactly on
  the sim's `view_range_cells` of 20 at 2 m per cell, i.e. 40 m. The dictionary's
  `unit.vision` block then moves a boundary that already has a number behind it.
- The **radio widget**: the push-to-talk button, the transcript, the 👍/👎, the collapsible badge
  (`OpenSkyFlight/docs/radio-chatbot-DESIGN.md` §2.1). A build of this shape, not a new idea.
- The **between-round panel**: the metrics, the proposal, the prediction, the verdict, the three
  buttons, and the Glitch Meter needle.

### What "done" means for the slice

Written the way `docs/ACCEPTANCE.md` writes its own criteria — measurable, and *fail* stated:

1. A kid can play a full five-minute match on the Sitka cell with **no build UI at all**.
2. Saying "make the paint bouncy" produces, in ≤ 5 s: a **named block**, a **sealed prediction**,
   a **held-out test on 4 seeds**, and an **Adopt/Reject** prompt.
3. The adopted change lands in the harness's git history with a commit id derived from the round
   id, so the same round always produces the same commit (D65,
   `docs/design/decisions.md:79`), and the next round pins that harness
   (`docs/ACCEPTANCE.md:136-139`).
4. `lm replay` on the next round's log reports **OK 100 hashes** — the paint is in the state hash
   and the round is exactly reproducible (`docs/ACCEPTANCE.md:191-192`).
5. "put it back" files an inverse deposit, and the round after that is back to the old behaviour.
6. Bolt makes **exactly one** offer per round, the offer expires at the round's end, and accepting
   it produces a scoped block that passed the same bounds check and the same sealed prediction.
7. **Fail:** a block outside its bounds is applied anyway. **Fail:** a replay of a painted round
   reports a hash mismatch. **Fail:** the refusal rate over a session is high enough that the kid
   stops talking — in which case the game offers the block buttons instead.

### What the slice explicitly does *not* do

No Bevy. No new renderer. No place ingest. No fact mutation (pending Open Question 2). No
capture tier. No face staff. No cloud fallback for the ears. No free-text anywhere that is not
the local deposit.

---

## 9. Risks and open questions

### Risks, with the ones we think about at night first

| # | Risk | Evidence | Mitigation |
| --- | --- | --- | --- |
| R1 | **The 142 m trap**: using the asset tier boundary as the gameplay boundary. A mesh can be fetched, verified, correctly placed and invisible. | `Otherhere/docs/playtests/round-17.md:50-59` | Separate ladders; a screenshot legibility test, not a fetch test (§6) |
| R2 | **Doc drift**: the same ladder sentence was wrong in four places at once while the code was right. | `Otherhere/docs/playtests/round-17.md:60-69` | Generate the tier docs from `chooseTier`; one source of truth |
| R3 | **Draws.** The starting game is symmetric and the reflex is symmetric, so 63% of rounds draw and the scoring knobs do not fix it. | D63, `docs/design/decisions.md:77` | Paint as the mirror-breaker (§0); decisive-outcome is a Glitch Meter criterion |
| R4 | **The gate is not reliable enough over 48 options.** D62's 96% is measured on a small option set, and "repeated calls to one model are deterministic, so self-consistency votes are useless" (D62) means we cannot vote our way out. | `docs/design/decisions.md:76` | Per-block accuracy on a labelled set; unreliable blocks stay buttons; a refusal is cheap and safe |
| R5 | **A busy box turns 179 ms into 27–36 s.** | `docs/notes/ears/measurement.md:63-68` | The deadline stands in; radio framing makes the delay diegetic; the round never waits |
| R6 | **Privacy inversion**: the moment the ears move to the cloud, every privacy claim in §7 inverts. | — | Enforce it in the build: a non-local recognition address is refused without parent opt-in |
| R7 | **Invariant 3 vs the source conversation's promise.** The kid was promised "watch the game rewrite itself". We are promising "watch two numbers change, with their names on them". | `src-conversation:286-300` vs `Otherhere/docs/BRIEF.md:42` | §1 beat 3 is designed to deliver the *same* delight: the diff is the show |
| R8 | **Cost.** Each proposal can cost a cloud call (`--reviewer router`, `docs/ACCEPTANCE.md:140-141`). A kid making 20 proposals in a session makes 20 reviewer calls. | `docs/ACCEPTANCE.md:140-141` | The stub reviewer is the default; the router is opt-in per session; the Ladder goes cheapest-first (`Otherhere/docs/BRIEF.md:69`) |
| R9 | **The round is 5 minutes and the flow takes 1.5–2 s**, so a kid's changes arrive a round late. That is right — but it must feel right, or the loop reads as "it ignored me". | `docs/ACCEPTANCE.md:110-111`; `crates/lm-core/src/params.rs:151` | The prediction is sealed *before* the test, so the kid watches the check happen live; the change is promised, not retrospective |
| R10 | **Paint is a new sim state in a deterministic fixed-point core.** Every hash, every golden, every replay depends on getting it right. | `crates/lm-core/src/params.rs:48-52` | Paint is in the hash from the first commit; the oracle/script agreement test (`assets/rules/beacons.rhai:3-6`) is the gate |
| R11 | **A capture's `model` tier means the photograph is never seen again** — which kills "look at what I just photographed" as a reward. | `Otherhere/AGENTS.md:26-27` | Open Question 4 |
| R12 | **There is no LOD for distant Cells.** Terrain and forest have distance bands; a Cell beyond the walker's ring simply is not loaded, so a sandbox that widens the world will find edges. | `Otherhere/docs/advice/2026-10-19-roadmap.md:47` ("Distant Cells could load a cheap footprint-only LOD … instead of nothing") | Keep the slice inside one Cell (which is what §8 does) and treat "open the world up" as a later dictionary block, not a slice feature |

### Open questions for Casey

1. **Magnus.** We want to name **Magnus as the intended co-designer and first playtester** of the
   sandbox — the way TALKSITKA was "design only, do not build until Magnus approves"
   (`OpenSkyFlight/docs/radio-chatbot-DESIGN.md`, header). His fork already solves the radio
   problem, the iPad problem and the thumbs-up problem, and the shared Sitka universe is explicit
   in the brief. **Please confirm before we put his name on it.**
2. **The beacons.** Should the match's four amber points *be* Otherhere's quest beacons — so that
   holding one sharpens the building and a match literally makes the town more real
   (`Otherhere/README.md:32`) — or should they be a separate marker and no Fact is ever mutated by
   a match? We have designed the slice to work either way, but the answer changes what the
   *score* means.
3. **The window.** A round is a 256 m square; a Cell is ~200 m edge and ~0.1 km². Should the round
   cover the whole Cell (which means changing the sim's world shape) or be a 256 m window onto it
   (which means some of the Cell is off the map)? We lean window.
4. **The photo reward.** If a capture has a `model` tier, the photograph is never shown again. Do
   we change the ladder for the sandbox (the photo always stays available as the `card`), or do we
   design the reward around the mesh instead?
5. **Who plays.** One human against the AI, or two humans plus an AI rival? The negotiation in §4
   is built for the AI rival; two humans would need the offer to be a network message, and the
   wire protocol exists (`crates/lm-proto/src/lib.rs`) but the transport is phase 3 (D34).
6. **Dictionary reachability.** We should measure the gate's per-block accuracy on a labelled set
   of kid utterances before we ship 48 blocks. How many blocks can we trust? (This is the honest
   version of R4, and it is a measurement, not an opinion.)
7. **Which blocks are Tier A locked?** We propose the *bounds*, the *world shape* and the
   *ledger* rules are locked, and the *feel* blocks are learnable. Casey's call.
8. **Do the kids' builds leave the device?** Otherhere's postcard is a shareable snapshot of the
   facts of the seven cells around you, rebuilt in the recipient's browser
   (`Otherhere/README.md:61`). A *ruleset* is also a capsule in Otherhere's glossary
   (`Otherhere/docs/BRIEF.md:32`). So a built game could travel as a capsule. Should it? What is
   the kid's scope — `private`, `friends`, `public`? Otherhere's rule is that kid accounts never
   get `public` (`Otherhere/docs/BRIEF.md:44`), and we would want the same here.
9. **The request pile.** When Riley asks for something with no word for it, the request sits in
   `Deposit.questions`. Who ever reads that pile, and does reading it become a way to grow the
   dictionary (a human adding Tier A blocks)?

---

## Appendix A — where each source idea landed

| Source idea | Where it is now | Why it changed |
| --- | --- | --- |
| **Vibe Alchemy** — drag blocks + a chaos prompt box, and the game rewrites itself (`src-conversation:286-293`) | §3, the Vibe Dictionary; §1 beat 2 | The chaos prompt box becomes a **closed choice**, not free text into a compiler. The block-drag UI becomes the **thumbs** and the **block list**. |
| **Glitch Meter** — a stability score with a safe zone and a glitch zone, and the mission constraint "keep it above 80%" (`src-conversation:294-301`) | §5 | It stops being a fear meter and becomes a **measurement of the sealed prediction**: stability = the fraction of held-out seeds that moved as promised. A "safe glitch" is a change that is stable but strange. |
| **Under the Hood** — the raw source flashes beside the game (`src-conversation:302-321`) | §1 beat 3; §4 | It becomes **the diff, with field names**, not a source file. Safer, shorter, and it is the same show. |
| **The Great Janitor Rebellion** — a typed command rewrites the AI's pathfinding and injects a mesh (`src-conversation:322-345`) | §4 (the rival), §3 (`paint.*`) | A janitor that vacuums paint and complains about it is a **personality archetype** and a **paint rule**. It becomes `Tideline` and `paint.erase` — both bounded, both in the dictionary. |
| **Reality Loom** — voice deforms the world mid-sentence (`src-conversation:384-390`) | §6 (the paint layer), §3 (`paint.*`) | Voice changes *rules*, not vertices. The deformation is a **dictated change** with a sealed prediction. |
| **Agent as Faction Leader** — the AI plays the rival and negotiates live (`src-conversation:391-404`) | §4 | The negotiation becomes a **typed offer** with a scope and an expiry, and it goes through the same bounds and prediction rigour. |
| **The "Vibe Dictionary" question** — the source asked who would design the list of approved words and blocks (`src-conversation:330`) | §3 | That is this document's §3, and it is 48 blocks with bounds. |
| **TALKSITKA** — push-to-talk, radio framing, two voices, thumbs up/down, text fallback, anti-drift tiers (`OpenSkyFlight/docs/radio-chatbot-DESIGN.md`) | §1, §2, §5, §7 | Taken wholesale: the radio makes latency diegetic, the thumbs are the learning signal, the two-tier store stops popularity from rewriting the rules, and the text fallback is never optional. |
| **Sitka Skies' simple mode** — "modes are a menu over the sim, not a rebuild" (`OpenSkyFlight/docs/super-suggestions/05-simple-mode.md`) | §8 | The dictionary is a menu over the sim. The mode ladder becomes: match → match + your blocks → world. |
| **The 142 m lesson** (`Otherhere/docs/playtests/round-17.md`) | §6 | The gameplay ladder and the asset ladder are separate, and the legibility test is a screenshot. |
| **D62, D64–D66, D70, D73** (`docs/design/decisions.md`) | §2, §3, §4, §5 | The measurement discipline: d1-2B for closed choices, the file chain for every change, the split pack, the pre-rendered voice bank. |
| **D63** — draws are mirror ties (`docs/design/decisions.md:77`) | §0, §5, R3 | Paint is the mirror-breaker, and "the round must still be decisive" is a Glitch Meter criterion. |

## Appendix B — the numbers this document leans on

| Number | What it is | Where |
| --- | --- | --- |
| 179 ms p50 / 198 ms p95 | key up → line applied | `docs/notes/ears/measurement.md:30,35` |
| 6 ms p50 / 11 ms p95 | the network part of that turn (a local hop) | `docs/notes/ears/measurement.md:36-37` |
| 27–36 s | the same turn on a busy box; the stub stands in | `docs/notes/ears/measurement.md:65-68` |
| 20.3 / 60.3 ms | d1-2B, one / three questions | `Otherhere/docs/adr/0019-local-decision-tiers.md` |
| 162 ms / 473–1400 ms | last-mile's move pack / full pack on the Thor | `assets/harness/default/policy.toml:45-46` |
| 0.64 s idle / 2.5–3.6 s busy | skald render; the bank is the point | D73 |
| ~1.5–2 s | the learning flow at `--learn-ticks 300` | `docs/ACCEPTANCE.md:110-111` |
| 96% / 0.60 | d1-2B's action accuracy and the confidence gate | D62 |
| 63% | draws on the symmetric map | D63 |
| 9000 ticks @ 30 Hz | one round = 300 s = **5 minutes** | `crates/lm-core/src/params.rs:151` |
| 2048 units | the tag range = 2 m | `crates/lm-core/src/params.rs:99,157` |
| 4096 units | the beacon radius = 4 m | `crates/lm-core/src/params.rs:159` |
| 10 000 | the default script fuel | `crates/lm-core/src/params.rs:162` |
| 140 m | `SPLAT_NEAR_M` | `Otherhere/apps/client/src/assets.ts:78` |
| 138 m / 142 m | the A/B: splat / mesh, same bearing, 4 m apart | `Otherhere/docs/playtests/round-17.md:20-21` |
| ~0.1 km², ~200 m edge | an Otherhere Cell (H3 res-9) | `Otherhere/docs/adr/0001-h3-resolution-9-for-cells.md` |
| 700 m across | a walk: centre + 1 ring = 7 cells | `Otherhere/docs/adr/0001-h3-resolution-9-for-cells.md` |
| 83.8 / 47.8 / 17.0 fps | M4 Max Chrome: avg / p5 / p1 | `OpenSkyFlight/BENCHMARKS.md` |
| 37.7 / 32.3 / 30.3 fps | M4 Pro Safari — steadier p1 than Chrome | `OpenSkyFlight/BENCHMARKS.md` |
