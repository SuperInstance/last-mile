# E7: the score view — the piano roll grown into a conductor's score

*2026-10-29. The brief: one system of staves per agent on one time axis, built
from data the round already has, working live and in replay (docs/design/
vox-score.md §11–§13, experiment E7).*

The bottom panel has two modes now. `E` walks between them; `--bottom score`
opens on the score; the panel takes 40% of the window's height while the score
is drawing (the roll keeps its quarter), and the map above shrinks by the same
amount. The replay scrubber stays in both modes — the score is the same round
on the same clock, drawn differently.

| shot | what it is |
|---|---|
| `score-live-thor.png` | live, `--agents thor`, seed 1, frame 4000 (the round ended on tick 2012 and the app reopened its own log), score mode |
| `score-replay.png` | the replay of the log that same run recorded (`runs\1-1793317088.lml`), scrubbed to tick 620 |
| `score-calm-vs-urgent.png` | two moments of one real round (seed 7, `runs\7-1793315901.lml`): a calm stretch (ticks 21..620) over the tag fight (ticks 1261..1860, first tag at 1817) |
| `score-moment-calm.png`, `score-moment-urgent.png` | the two moments on their own |

Commands (Windows mirror, `scripts/win-build.sh build -p lm-host --release`):

```text
lm-host.exe --agents thor --bottom score --screenshot shot.png --frames 4000
lm-host.exe --replay runs\1-1793317088.lml --bottom score --scrub 620 --screenshot shot.png --frames 90
```

## What each staff shows, and where its data comes from

One **system** per agent, stacked in agent-id order, the squad's colour down
the left edge and the agent's name beside it (`a0 L`, `a1 m`), like the
instrument names of an orchestral score. Top to bottom within a system:

**Voice** — each utterance decision as a **note bar**: the line's airtime
(`tick .. tick + duration_ticks`), drawn in the dimmed colour of its channel.
Above the bar, the line's **words at the ticks the score timed them to**
(`Word.t_on/t_off`), each lifted or lowered by its pitch lane (`pitch_steps`,
±24 semitones into the band's half-height), stressed words in amber
(`emphasis_q16`), abbreviated to three letters, then one, when the word's
ticks are narrower than the word. Below the bar, the line's **marks**: the
utterance plan's own fields — the tone (`affect`), the intention (`intent`),
the address (`addressee`) and the subject (`subject`), as short words:
`urgent order to-squad b2`. The bar's top edge carries a hair of the tone's
colour, so urgency is a colour before it is a word. *Data: the round log
(`Kind::Utterance`), exactly what `crates/lm-agents/src/utter.rs` chose and
what the log replays.*

**Intent** — the mind's hand on the body. Every applied decision is a
**gesture**: move decisions (`Kind::Lane`, `Roll`, `Intent`) as narrow bars,
actions and releases (`ActionOn`/`ActionOff`) as wider ones, coloured by
whose hand made it (the roll's own colours: green human, blue System One).
**Hollow marks** — stroke-only rectangles, amber when the reason was
confidence — are the refusals and the unsure gates (D72's `Answers::unsure`,
which reaches the mind as a per-gate `LowConfidence` refusal): a tick where a
mind asked and did not answer. *Data: the log for the gestures; the live
minds' own refusal records for the hollows — see "what's missing".*

**Gaze** — the agent's attention target over time, as a lane of colours and
ids: the move its lanes hold (`lane_cmd`: `b2`, `hold`, `follow`, `retreat`,
each in its own colour, labelled when the run is wide enough), sampled once
per tick from the agent's own state — live as the round plays, and by the
scrubber's opening replay for a round opened from a file, so both modes draw
the same lane. At the tick each line begins, a bright tick and a hair along
the band top name **who the line was for** (the squad's colour for the squad,
white for all, the spoken-to agent's squad colour for one agent): the gaze of
the voice, at the moment it spoke. *Data: the agent's own state, sampled by
replay or play; addressees from the log.*

**Tier** — §13's engine and gamepad, made visible: where the lanes held no
move the **engine** (the reflex, tier 0) played the body — dark; where the
lanes held a move, the **gamepad** (System One, tier 1) — teal. A decision
that landed late draws an amber tick here. *Data: the same attention samples
as the gaze staff, read as tiers.*

The current-tick cursor runs vertically through every system (at the right
edge live, wherever the scrubber is in replay), the second marks are faint
verticals through the whole track, and the score zooms to the last N seconds
(`--score-seconds`, 20 by default). In replay the scrubber pans the window:
both moments of the side-by-side were taken by scrubbing the same log.

## The E7 pass question: can a reader tell the urgent moment from the score alone?

**Honest answer: not from these rounds — and the reason is data, not drawing.**

I played four Thor rounds (seeds 1, 5, 7, 11, `--agents thor`, d1-2B on
:8094) and scanned every log I could find on the machine (83 logs across the
build mirrors). **Not one line was delivered urgent**: d1-2B answered the
intention gate calm 54/uncertain 1 in the seed-1 round, calm in all the
others (the stub rounds, of course, answer the middle of every scale — 26
logs of "uncertain" roll-calls). The ears work measured the same prior
independently (EARS.md: 52/58 clips called urgent by the audio probe, and
d1's own answers flat). So the calm-vs-urgent pair above is a calm stretch
against the round's *fight* — the first tag of the round at tick 1817 — which
is the round's urgent moment in the game's own terms.

What the two moments actually show, read from the score:

- **The calm stretch talks.** One note per agent (the round's roll-call,
  `we hold beacon 2` / `beacon 2 is ours`), all marked `calm order to-squad
  b2`; the gaze lane is settled (squad 0 magenta on b2, squad 1 green on b3),
  the tiers go teal within the first seconds as the lanes choose, and the
  only gestures are the agents' first moves.
- **The fight is silent.** Through ticks 1261..1860 the score shows no
  gestures, no notes, no gaze change, no tier change: the minds had stopped
  answering (the HUD of that round's live run read `agents think 23 refused
  14` — 14 of 23 asks refused), the last line had been said at tick 844, and
  the gamepad kept the move it held while the world tagged two agents down.

So a reader *can* tell the two moments apart — but by the presence of
performance versus the absence of it, not by reading urgency off a delivery.
The score truthfully shows that the mind went quiet through the urgent
moment; it cannot show an urgent delivery, because in every real round
available there was none. Judged against E7's pass as written ("a reviewer
can tell from the score alone which of two **deliveries** was the urgent
one"): **not passed on real data; the notation, the marks and the staves are
in place, and the missing thing is a round in which the intention gate ever
answers urgent.** The model-level guarantee is tested without a round
(`a_calm_line_and_an_urgent_line_read_apart`): given an urgent delivery, the
marks read it apart.

## What's missing (the honest list)

- **No prosody.** The voice staff's marks come from the plan, not from
  measured audio; `pitch`/`emphasis` ride the words but nothing was heard.
  Vox Score's L0/L2 fills that later (§4, §11): the marks will be quantized
  from a measured delivery, not asserted by a gate.
- **No urgent deliveries exist in any round I could produce.** d1-2B's
  intention gate answered calm throughout; the stub answers the middle of the
  scale. E7's pass needs either a round where squads fight while the gate is
  being asked (the Thor serves ~9 asks/s for the whole match, D68, so the
  asks are sparse), or a model that scores urgency less flatly.
- **The hollow marks are live-only.** A refusal is an outcome the *mind*
  records (D26); the round log carries no refusals, so a replay opened from a
  file shows none (a round the app played and reopened keeps the ones it
  collected). Making them replayable is a logging change — a refusal event in
  the log — not a view change.
- **The fight itself is on the roll, not the score.** Tags and stuns are
  world events; the roll draws their spans, the score (by design, per the
  brief's four staves) does not. Through the seed-7 fight the score shows the
  mind's silence, not the world's violence.
- **Smallness at 720p.** With the default eight agents the words sit at
  ~7px and the marks at ~6.5px — legible at 1280x720 on the panel, at the
  edge of it in a screenshot. The pitch lane compresses to a couple of pixels
  per agent at eight systems; it opens up when there are fewer.

## Where it lives

- `crates/lm-host-model/src/score.rs` — the model: staves, notes, gestures,
  hollow marks, the attention trace and its sampler (`note_attention`), and
  `ScoreModel::from_log`.
- `crates/lm-host-model/tests/score.rs` — the properties, including that a
  replayed round scores exactly like a played one
  (`a_replayed_round_scores_like_a_played_one`).
- `crates/lm-host/src/score_plugin.rs` — the drawing; `roll_plugin.rs` keeps
  the panel and flips modes; `keys.rs` holds `E`; `map_plugin.rs` sizes the
  panel; `main.rs` reads `--bottom`, `--score-seconds` and `--scrub`.
- The scrubber samples attention during its opening replay
  (`crates/lm-host-model/src/scrub.rs`), which replays the log once anyway
  for the events — the score costs that pass a `Vec` push per tick.

Determinism: the view reads only. The golden stayed OK 3/3 and
`scripts/check.sh` printed `==> all checks passed`.
