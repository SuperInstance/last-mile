# The look and feel: a toy diorama

**Status:** design document. Ships with the look file that describes it.
**Date:** 2026-10-30
**Author:** for Casey

## What this document is

The host had no art direction. It worked: the 2D map, the 3D view, the
HUD, the score and the between-round screen all drew, and all of them
agreed about nothing except which agent was which. This document is the
direction that came out of the work — a toy diorama of a bright, friendly
city, with paint as the only loud colour on it — and the list of what
changed on each screen.

The direction is **data**. Every colour, size and shadow a person can see
is a token in [`assets/look/look.toml`](../../assets/look/look.toml), and
the host reads it once at start-up. The 2D map, the 3D view and the panels
around them all take their colours from it, so the world looks the same in
every view — and a different look is a different file, not a different
build.

Nothing here was taken from anywhere. The shapes are boxes and cylinders
built in code, the colours are spelled in code, and the type is the
system's own.

---

## The direction

**A toy diorama.** The city is a small model on a table, lit from over
one shoulder. The figures on it are game pieces: chunky, a little
oversized for their world, and separated from the board by a dark edge
so they read as *pieces* and not as people.

**Lemmings meets paintball.** The world is calm, friendly and warm: pale
sand for the ground, terracotta for the buildings, sage for the hedges.
Nothing in the world shouts. The only loud things on screen are the paint
— the figures wear it, a splat is made of it, and the score line of a
squad is written in it. That contrast is the whole art direction: *a calm
world with loud paint on it*.

**Readable by a ten-year-old, at a glance.** Five questions, and each one
has a shape, not a caption:

| Question | The answer |
| --- | --- |
| Who is on which team? | The colour of the piece: ochre against teal. |
| Who is tagged? | The pips ringing the head of the piece that is out. |
| Where is the paint? | The splat on the ground, in the paint of the squad that threw it. |
| Who is talking? | The speakers named in the talk bar, each in their squad's colour. |
| Who leads? | The hat: a brim on the head. A *silhouette*, so it survives a shadow or a two-pixel figure. |

**Motion with restraint.** Transitions are short and eased (100–250 ms);
nothing bobs forever; the splat dries in half a second and the board is
clean again. The look **reads** the sim and never writes it: the trail is
the host's own bookkeeping, frame by frame, and the round is never asked
to remember anything.

**The pairing is a parameter.** The sandbox's palette is four paints —
ochre, teal, violet, saffron. A round has two teams and the teams take
two of them, so the pairing is an array in the look file, not a constant
in the code. The default pairs the warm paint against the cool one.

---

## The tokens

[`assets/look/look.toml`](../../assets/look/look.toml) is the whole
vocabulary. It may name any subset of the knobs; whatever it leaves out
keeps its default, the way `render.toml` reads. A file that exists but
cannot be read is an error at start-up, not a silent fallback — a
renderer running on defaults nobody asked for is a lie about what is on
screen.

| Section | What it says |
| --- | --- |
| `[world]` | ground, road, building, cover, sky, void, beacons, fog |
| `[paints]` | the four paints: ochre, teal, violet, saffron |
| `[teams]` | which paint each squad wears, in squad order |
| `[ui]` | the panels' surface, text, accent, danger, leader, stunned |
| `[light]` | ambient, the sun's colour and strength and direction, the side shade, the shadow's strength |
| `[figure]` | how tall a piece stands, and the leader's hat |
| `[outline]` | the dark edge: its width in the 2D map and in the 3D view, and its colour |
| `[type_scale]` | five sizes, and no sixth |
| `[camera]` | how far the 3D camera tilts above the ground |

Colours are sRGB hex — the space a colour picker, a screenshot and a
colour-blindness test all speak. The 3D view consumes linear light, so the
conversion happens once, at the boundary.

Two notes for the next person:

* **The type scale has five sizes and no sixth.** A panel that needs a
  sixth size is a panel that has stopped saying what it means. The names
  are what the size is *for* (`huge`, `big`, `body`, `small`, `tiny`), not
  how big it is.
* **The paints belong to the other job too.** The paint *layer* — the
  persistent marks on the ground — is a separate piece of work. The four
  paint colours are defined in `[paints]` above precisely so that job can
  read them rather than choose its own.

---

## Before and after, screen by screen

The shots are in [`docs/design/look/`](look/): `before-*.png` is the
host as it was, `after-*.png` is the host as it is now. Both sets were
taken with the **same commands** (see *How the shots were taken*), on the
same machine, from the same seed — the one exception is `after-splat.png`,
the tag moment, which needed a seed of its own and a trigger (see below).

### The 3D view — `before-3d.png` → `after-3d.png`

**Before:** a navy floor under a violet sky. Grey boxes for buildings,
dark green boxes for cover, orange figures with no edge against the dark
ground. The world's edge and the sky were two greys of the same value, so
the board did not read as a board.

**After:** warm sand for the ground, terracotta for the buildings, sage
for the hedges, a pale grey table for the void the board sits on, and a
blue-grey sky. The figures carry a **dark outline shell** and a **contact
shadow**, so they read as pieces on a board. The leader wears a **white
brim**. The camera **tilts further over the board** (0.45 rad, the look's
`[camera] pitch_rad`, up from the 0.35 the host used to hard-code), so a
diorama is looked at from above rather than from the street — and the
city still recedes into the fog at the top of the frame, so it reads as a
city and not as a beach.

The light is now the look's: a warm sun over the left shoulder (so the
shadow falls to the right and the player can tell which way the light is),
a weak ambient fill, and a flat shade on the sides of every box — the
shadow of a box in a world with no other shadow.

### The 2D map — `before-2d.png` → `after-2d.png`

**Before:** a navy map with grey blocks and green speckle. The agents
were 7-pixel dots with a leader's ring; the ground, the cover and the
buildings were all cool greys and greens, so nothing on the map said
"paintball".

**After:** the same city in the 3D view's colours, and the **streets**:
a floor cell that touches a building is drawn in the road's colour, so the
city reads as blocks with roads between them — the same rule in both
views, from one function (`lm_render::street::is_street`). The agents are
**12-pixel** discs with a **dark edge** and — for the leader — a **halo**,
so a piece is findable at a glance. The score line of each squad is in
that squad's paint.

### The HUD — visible at the top of every shot

**Before:** everything was the same size and the same weight, so the score
of the round looked like the scheduler's counters.

**After:** one type scale, and the scale does the work. The **score of
each squad** is the biggest thing in the bar, in the squad's own paint;
the **clock and the counters** are small and dim; "watching" is quiet.
The talk bar now names **who is talking** — each speaker with the channel
they are on ("speaking", "whispering", "on the radio"), in their squad's
colour, so a line that is heard is a line whose squad is known.

### The score view — `before-score.png` → `after-score.png`

**Before:** a dark table with white labels.

**After:** the same table in the look's surface and text colours, with
each system's label bar in its squad's paint and the "now" line in the
look's accent. The score view is the same picture as the round, not a
second picture that happens to be next to it.

### The between-round screen — `before-between.png` → `after-between.png`

**Before:** a grey window floating over the map.

**After:** the window sits on the look's surface, its sections are
**headings** (one step above the body, so the screen has a shape), and
the notes and the refusals use the look's accent and danger colours
instead of ad-hoc RGB.

### The tag moment — `before-tag.png` → `after-tag.png`

This is the shot the direction is for. The possessed agent is followed;
behind it is **the path it has walked**, in its own squad's paint, drawn
by the host as it watched.

* **The splat.** A tag throws five dabs of paint on the ground where the
  target stood, in the paint of the squad that hit it. The paint spreads
  and fades over half a second and the board is clean again. It is drawn
  in **both** views — the 2D map and the 3D diorama — from one resource,
  so a hit looks like the same hit in both.
* **The stun pips.** A ring of pips over the head of an agent that is out
  of the round, one for each tick of the stun that is left, in the paint
  that put them there. In the 3D view the ring is in the horizontal plane,
  so it reads as a row of pips from the side and as a circle from above —
  a mark that says "this one is out" from every angle the camera can be
  at.
* **The path.** The places the watched agent has been, frame by frame.
  The host's own bookkeeping: the round is never asked to remember.

**What the stills show.** `after-tag.png` catches the **path** — the
paint of the trail behind the followed piece, and the leader's white
brim on its head. `after-splat.png` catches the **hit itself**: the
teal ring of stun pips over the head of the piece that was hit, the
teal paint at its feet, and the path it walked to get there.

**How the tag shot was taken.** The paint lives half a second and a
still is one frame, so a blind sweep is a lottery: the squads only
engage when they see each other within six cells, which for seed 1 is
tick 1845 — a minute into the round. So the tick was found first (the
headless round, played at 30 ticks a second, says which tick the first
hit lands on), and the shot was taken with a **temporary capture
trigger**: a patched copy of the host that takes the picture when the
round's events carry a `Tagged`, instead of at a frame count. The patch
was reverted afterwards and the host rebuilt; the pixels in the picture
are the ones the shipped host draws. The trigger was needed because
Bevy writes a 1×1 PNG whenever the window's swapchain is momentarily
1×1, and on this machine that happens whenever the window is
minimised — so the trigger takes a burst of frames across the life of
the paint and the shooter keeps the ones that are real.

**An honest note about the paint.** The dabs land where the target
stood, which means that from the 3/4 camera of the diorama the piece
that was hit stands on top of them and hides most of the paint. What
the still shows is its edge, at the feet. From the 2D map the same
paint is fully visible. Whether the paint of a hit should be thrown
under the piece that was hit, or at the piece that threw it, or over
both, is a question for the director — it is listed below.

The splat and the pips are also covered by the unit tests in
`crates/lm-host/src/look_effects.rs` —
`a_tag_is_seen_even_when_it_is_not_the_first_event_of_its_tick`,
`a_tag_is_not_seen_twice`, `two_tags_on_one_tick_are_both_seen`,
`the_paint_of_a_hit_is_the_paint_of_the_squad_that_threw_it` and
`a_splat_spreads_and_goes`.

Those tests are also where the one real bug in this work was found: the
effects remembered the **tick** of the last event they had seen, so a
tag that was not the first event of its tick was dropped — and a tag is
often the second thing that happens on a tick. The effects now remember
**how many events** they have read, which is stable because the list
only grows.

---

## How the shots were taken

Both sets come from the same commands, on the same machine, from seed 1,
at the default window size (1280×720 logical, captured at the display's
2.25 scale, so 2880×1620):

```
lm-host --view 2d    --seed 1 --frames 420          → after-2d.png
lm-host --view 3d    --seed 1 --frames 420          → after-3d.png
lm-host --view both  --seed 1 --frames 420          → after-both.png
lm-host --view 2d --bottom score --seed 1 --frames 420
                                                 → after-score.png
lm-host --view 3d --possess 0 --seed 1 --frames 900 → after-tag.png
lm-host --view 2d --ticks 120 --seed 1 --frames 700 → after-between.png
```

and one more, the tag moment, taken with the trigger described above and
from seed 7 — the first hit of seed 7 lands on the piece the camera
follows, and the one of seed 1 lands on another piece a street away:

```
lm-host --view 3d --possess 1 --seed 7  → after-splat.png
```

The `before` shots were taken the same way with a binary built from the
commit this work started on, `HEAD` (`bfdb2a2`).

Two honest notes on the method:

* **The round is live, so the tick drifts.** The sim advances on a fixed
  timestep fed by the clock, so two runs of the same command are a few
  ticks apart and the agents are not in quite the same places. The shots
  compare *looks*, and the look is what was measured.
* **The capture is flaky.** Bevy writes a 1×1 PNG when the window's
  swapchain is momentarily 1×1 — which happens while the between-round
  flow's console subprocess runs. Every shot was checked and re-taken
  until it was a real picture. This is pre-existing: the old binary does
  it too.

---

## The frame rate

The look adds work to every figure: an outline shell, a base plate, a
hat for the leader, and the pools of effects. So the 3D view was measured
before and after, with the **same instrument on both sides** — the "before"
binary is `HEAD` plus only the fps hook, so the two numbers mean the same
thing.

**How.** `lm-host --view 3d --seed 1 --fps --screenshot … --frames 600`.
The `--fps` flag turns the display's waiting off (`PresentMode::Immediate`)
and times the last 120 frames after a 60-frame warm-up, so the number is
the scene's own rate and not the screen's refresh rate. The runs were
**interleaved** (before, after, before, after …) because the ProArt is
shared and another seat's test suite was running much of the time.

**What came out.** The samples are **bimodal**: some runs present at
exactly **60 fps** — the display's refresh — and some run ahead, at
**180–390 fps** depending on how much of the box was free. Both builds
show both modes; the mode is the present path's, not the scene's. So the
**median** of a handful of samples is a noisy statistic, and the honest
summary is the **ceiling** — the best run each side managed.

Two sessions, eighteen interleaved rounds in all:

| | session 1 — 10 rounds | session 2 — 8 rounds |
| --- | --- | --- |
| the load during the runs | 5 – 12 | 0.6 – 5.2 |
| before: ceiling | 295 fps | 388 fps |
| after: ceiling | 282 fps | 380 fps |
| before: median | 271 fps | 366 fps |
| after: median | 259 fps | 294 fps |

So: **in normal use the 3D view is limited by the screen, not by the
scene** — it holds the display's 60 Hz either way. When the present path
lets it run ahead, the look costs **2–4%** of the ceiling (295 → 282 fps,
then 388 → 380 fps): one extra mesh per figure, and the pools of
effects. The medians say something worse than that (−4% and −19%), but
the samples behind them are spread from 60 to 388 fps and there are only
seven of them; the medians are measuring the machine, not the look.

**The load.** Both sessions were taken on the shared ProArt: the first
while another seat's test suite was running, the second on a quieter box.
Several runs in both were discarded because Bevy captured a 1×1 PNG (the
window's swapchain was momentarily 1×1). **These are not numbers to quote
as the game's frame rate**; they are the numbers that say the look does
not cost the frame.

---

## What changed in the code

| Where | What |
| --- | --- |
| `assets/look/look.toml` | the tokens — the whole vocabulary |
| `crates/lm-render/src/look.rs` | the tokens as types, and the sRGB → linear conversion |
| `crates/lm-render/src/street.rs` | which cells are road: a floor cell that touches a building |
| `crates/lm-render/src/palette.rs`, `greedy.rs`, `mesh.rs` | the road material class, the side shade from the look, the palette from the look |
| `crates/lm-render/src/scene.rs` | the stun ticks on a figure, and where an agent stands |
| `crates/lm-render/src/hash.rs` | the chunk hash covers the ring of cells around it and the palette's own colours |
| `crates/lm-host/src/look.rs` | the host's half: the file, the theme, the colours as Bevy and egui want them |
| `crates/lm-host/src/look_effects.rs` | the splat, the pips and the trail — one resource, both views |
| `crates/lm-host/src/view3d/effects.rs` | the same effects as pieces of the diorama |
| `crates/lm-host/src/view3d/agents.rs` | the outline shell, the contact shadow, the leader's brim |
| `crates/lm-host/src/map_plugin.rs`, `hud_plugin.rs`, `score_plugin.rs`, `between_plugin.rs`, `roll_plugin.rs` | the panels and the map, in the look's colours and type |

**Nothing in the sim changed.** `lm golden` is still OK 3/3, with no
re-pin: the look touches the renderer's cache key and the host's drawing,
never the world the rules play on.

---

## What is left open

* **The camera's zoom is still the viewer's job.** The tilt is now the
  look's, but how far back the camera sits when it follows a possessed
  agent is still the old behaviour, and it is very close. A "diorama
  distance" token would make the followed view as tunable as everything
  else.
* **The fog is a colour, not a weather.** The fog over the unexplored map
  is a single flat wash. It reads, but a city that fades into the distance
  would read better.
* **The paint layer is the other job's.** The splat here is the *hit* —
  half a second of paint in the air. The marks that stay on the ground
  are a separate piece of work, and the four paint colours are waiting in
  `[paints]` for it.
* **Where the paint of a hit lands is undecided.** It lands where the
  target stood, so the piece that was hit hides it from the 3/4 camera
  (see `after-splat.png`). Three answers: keep it under the target, throw
  it at the piece that threw it, or spread it over both. The recipe — five
  dabs, a fixed spread — is a constant in the code today; if the answer
  changes, the recipe belongs in the token file next to the rest of the
  vocabulary.
