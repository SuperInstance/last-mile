# E6 — the marks layer (L2.5): words for nuance, and whether they agree

*2026-10-29. Companion to `E4-E5.md`. The design under test is
`docs/design/vox-score.md` §11 (the notation layer); the quantizer is
`tools/vox-ref/vox_ref/marks.py`; the harness is `tools/vox-e4/e6_*.py`. The
evidence is in `evidence/vox-e6/`.*

---

## 0. What was run, in one table

| step | what | result |
|---|---|---|
| **the quantizer** | L2 → marks, deterministic, thresholds in a config with defaults | 5 families, 21 marks; the analyzer reproduced E4b's L2 line on **340/340** clips, 67 ms per clip |
| **the vocabulary** | §11's draft against the fields that survived E4b | **steadiness is dropped** (vibrato and tremolo read zero on every clip); three more marks are dropped and one renamed |
| **E6(a)** | per family, the marks against LFM3-Audio's own answer about the same audio | **incomplete** — the arm is infrastructure-bound on this box and got 15 of 340 clips; no family can be placed above or below chance. What it did establish: an LFM3 answer only counts if the model was allowed to think first (§5) |
| **E6(b)** | per family, the marks against the clip's intended label | all five families **above chance** (κ +0.06 … +0.26); contour carries *intent*, the other four carry *delivery* |
| **E6(c)** | the marks alone through the d1-2B gate, against the 7-field L2 line | the marks **beat the line on order/question** (67.3% against 65.6%) and **lose on urgency** (80.0% against 90.6%); the marks line is **6 words against 24** |
| **System Two** | 20 same-word pairs as marks only, to a 30B reasoning model, no audio | **70.0%** against a 50% chance line; the 7-field line gets **90.0%** on the same pairs |

Everything below is reproducible from the repository with the scripts named in
§9. The evidence is in `evidence/vox-e6/`.

---

## 1. The quantizer

`tools/vox-ref/vox_ref/marks.py` turns a measured, profile-relative L2 patch
into a mark set:

```
vox: vel soft -4dB | bright low | tempo 4.8/s | dur 0.63s | contour fall -0.79 | end fall -1.16 | tone fall
marks: [soft, clipped, falling, regular, dark]
```

Four properties, in the order the brief asks for them:

1. **Deterministic.** No RNG, no threshold fitted to the answer, no
   dependency on the clip's position in the corpus. The same bytes give the
   same marks, always. `tools/vox-ref/tests/test_marks.py` checks this and
   the bands (9/9 pass).
2. **Profile-relative throughout.** Every mark says how *this* delivery
   compares with *this speaker's* usual delivery. That is the principle E4
   already forced on the L2 line, and it is what makes a mark mean anything
   for a new speaker.
3. **Thresholded, with the thresholds in a config.** `MarkConfig` holds every
   number; the defaults live in the dataclass and the shipped file is
   `tools/vox-ref/marks.config.json`. A config file only has to carry the
   keys it wants to change.
4. **One mark per family, in family order.** The output is a flat list, so a
   mark word identifies its family. That is why §2 renames one word.

```
python3 -m vox_ref marks clip.wav --profile profile.json
marks: [strong, pressed, falling, brisk, bright]
```

---

## 2. The vocabulary, adapted to what was measured

§11's table is a draft, and four of its six families carry marks that cannot
be measured from the fields that survived E4b. The corpus decides:

| §11's draft | what happened | why |
|---|---|---|
| **Steadiness** — steady · tight · shaking | **dropped** | `vib` and `trem` read **zero on all 340 clips**. Riva's synthesis has no measurable vibrato or tremolo at a 100 Hz frame rate, so none of the three marks could ever fire. A family whose words never appear teaches the reader a vocabulary they will never use. |
| **Articulation** — clipped · pressed · even · drawn · trailing | `trailing` **dropped** | `release_ms` is **zero on 327 of 340 clips**, and the soft-release class fires on 2 of them, so a trailing mark cannot be seen in practice. The other four marks survive: the decay ratio against the speaker's own runs from 0.33 to 8.33 with a median of 1.00, and it tracks the delivery. |
| **Pace** — rushed · brisk · even · measured · halting | `halting` **dropped**, `even` **renamed** to `regular` | there is **no pause in any clip of the corpus** (`pause_count` is 0 everywhere), so a halting mark could never fire. And §11 lists "even" in two families, which is ambiguous in a flat mark list; articulation keeps "even" and pace takes "regular". |
| **Colour** — dark · plain · bright · breathy · strained | `breathy` and `strained` **dropped** | they need a noise and tilt channel, and the L2 layer carries neither: the measured timbre is the spectral centroid. The L0 frame layer *does* carry `hnr_db` and `tilt_db_per_octave`, so this is a possible extension, not a closed door — but it is not measured here, so it is not in the vocabulary. |
| **Dynamics** — hushed · soft · level · strong · shouted | kept, all five fire | the level against the speaker's own peak runs from −8.8 dB to +7.8 dB, so all five bands are populated. |
| **Contour** — falling · flat · lifting · rise-fall · questioning | kept, all five fire | the pitch track carries all of them. |

What ships is therefore:

| family | marks | measured from |
|---|---|---|
| dynamics | hushed · soft · level · strong · shouted | the level against the profile |
| articulation | clipped · pressed · even · drawn | the loudness envelope against the profile |
| contour | falling · flat · lifting · rise-fall · questioning | the pitch track and the boundary tone |
| pace | rushed · brisk · regular · measured | the tempo against the profile |
| colour | dark · plain · bright | the spectral centroid against the profile |

21 marks over 5 families. The vocabulary is a **subset** of §11's, and every
omission is a measurement rather than an opinion.

---

## 3. The marks on the corpus

`tools/vox-e4/e6_marks.py` re-analyzes all 340 clips with the same speaker
profiles E4b used, and writes `evidence/vox-e6/marks.csv`. It also
**re-renders the v3 L2 line from the analyzer and checks it against E4b's**:
**340/340 identical**. The marks therefore sit on exactly the measurement the
L2 line was measured from, and the comparison in §6 is like for like.

How often each mark fires:

| family | distribution over the 340 clips |
|---|---|
| dynamics | level 157 · strong 79 · soft 60 · shouted 25 · hushed 19 |
| articulation | drawn 117 · even 96 · clipped 74 · pressed 53 |
| contour | falling 140 · rise-fall 80 · questioning 80 · flat 34 · lifting 6 |
| pace | measured 120 · regular 112 · brisk 68 · rushed 40 |
| colour | bright 155 · dark 99 · plain 86 |

Every mark fires at least six times. Two are thin — `lifting` (6) and
`hushed` (19) — and that is worth saying out loud rather than hiding: the
corpus has almost no deliveries that rise overall, and only a handful that
are very quiet.

---

## 4. E6(b) — the marks against the clip's intended label

The intended label is the Riva style that was rendered, read through an
explicit mapping from style to mark (`tools/vox-e4/e6_agreement.py`). The
mapping is a **stated expectation** — it comes from the plain meaning of the
style names, not from the measurements — so the marks are scored against
something they were not fitted to.

| family | n | agreement | Cohen's κ | chance |
|---|---|---|---|---|
| dynamics | 340 | 38.5% | **+0.159** | 20.0% |
| articulation | 340 | 38.8% | **+0.178** | 25.0% |
| contour | 340 | 24.7% | **+0.064** | 20.0% |
| pace | 340 | 35.9% | **+0.114** | 25.0% |
| colour | 340 | 50.3% | **+0.257** | 33.3% |

**No family is below chance**, so §11's rule — *families below chance are
reworked or dropped* — does not fire, and nothing is dropped on this
evidence. But the shape of the table matters more than the pass/fail, and it
splits the families into two groups.

### 4.1 Four families carry the delivery; contour carries the intent

The corpus has two labels: the **style** (the delivery that was asked for)
and the **intent** (order or question). The four delivery families agree
with the style; contour barely does (24.7%, κ +0.064) — and that is
correct, because the contour of a Riva render does not depend on the style,
it depends on whether the line was spoken as an order or a question.

Against the intent label, contour is the strongest family in the layer:

| contour mark | n | question-share |
|---|---|---|
| questioning | 80 | **75.0%** |
| rise-fall | 80 | 58.8% |
| lifting | 6 | 66.7% |
| flat | 34 | 52.9% |
| falling | 140 | **29.3%** |

Read as a speech act — *falling* is an order, *questioning* is a question —
the marks get **66.5%** of all 340 clips right. Restricted to the two
decisive marks (*falling*, *questioning*), they get **72.3%** of 220 clips.
That is the E4b contour result (AUC 0.769) surviving quantization into five
words.

### 4.2 Dynamics: strong on urgent, weak on ordinary — and the reason is Riva

Against the corpus's own delivery classes (calm / ordinary / urgent, the
E4 vocabulary):

| class | the mark agrees | reading |
|---|---|---|
| urgent (Angry) | 44/60 = **73.3%** | `strong` or `shouted` |
| calm (Calm) | 26/60 = 43.3% | `level` or `soft` |
| ordinary (Neutral) | 11/60 = **18.3%** | mostly `soft` or `hushed` |

The ordinary class fails, and the reason is in the measurement, not the
vocabulary. Riva's **Neutral** style is *quieter* than its **Calm** style
(median −4.64 dB against −2.63 dB against the speaker's own profile), and
E4 said so then: *"Calm is slow and long, not quiet."* The marks report the
level faithfully, so Neutral reads `soft` and Calm reads `level`. **The
style names and the measurements disagree, and the marks side with the
measurement.** That is the right side to be on — but it means a three-class
delivery gate cannot be read off loudness alone, which is exactly what E4
§3.1 found (the level, the timbre, the tempo *and* the duration together
reach 0.839 leave-one-speaker-out; the level alone reaches 0.700).

The same thing happens on the other side. Read as **urgent against not**,
the marks' dynamics is strong: 44 of 60 urgent clips are `strong` or
`shouted`, and only 2 of the 120 non-urgent clips are. Colour does it even
better — **54 of the 60 urgent clips are `bright`, against 15 of the 120
non-urgent ones.**

---

## 5. E6(a) — the marks against LFM3-Audio's own answer

### 5.1 The method, and why it is shaped the way it is

For every clip and every family, LFM3-Audio is given the **audio** and asked
the closed question that family answers in words — *"Rate the volume of this
voice delivery: hushed, soft, level, strong, or shouted? One word only."* —
and its answer is compared with the mark the quantizer produced for the same
clip. The two raters share no machinery: the marks are a deterministic
quantizer over measured signal features, and LFM3 is an audio model that has
never seen the analyzer. Where they agree, the word is probably describing
the delivery rather than an artifact of either one.

Two details of the method are worth stating, because the obvious
alternatives measure nothing.

**The answer is read with a JSON schema, not scraped.** LFM3 is a *reasoning*
model: it thinks before it answers, and on this corpus the thinking runs to
several hundred tokens. Asking for a JSON object whose `answer` field is an
enum of the family's words constrains the **answer** and leaves the
**reasoning free**, so the model still listens and thinks and still lands on
one of the closed words. The obvious alternative, a `grammar` over the whole
generation, constrains the reasoning too — the answer is then forced at the
*first* token, before the model has heard anything. Measured on twelve clips
of the first family, that degenerate variant answered "hushed" on eleven of
them and agreed with the marks on two: a constant answer wearing a
constrained costume.

**A truncated trace is not an answer.** The token budget is 1024 and a
response that hit the limit is recorded as *no answer*, not scraped. A
half-finished reasoning trace does not contain a conclusion; taking the last
option it happens to mention measures where the trace was cut.

### 5.2 The agreement, per family — **this arm did not finish**

**This is the one part of the brief that was not delivered at scale, and the
reason is the box rather than the method.** The arm needs 1700 LFM3
questions; on a contended machine at 20–60 s a question, with the `lm-ears`
proxy being restarted underneath it, the sweep is a day of wall-clock. It
was run for as long as the machine would give it and it got **15 clips of
the 340**.

What was measured, per family and per prompt style (the raw rows are in
`evidence/vox-e6/lfm3_families.csv`, the table is regenerated by
`tools/vox-e4/e6_agreement.py`):

| family | prompt style | n | agreement | LFM3's answers | the marks |
|---|---|---|---|---|---|
| dynamics | reasoned | 8 | 25.0% | soft 5 · level 2 · strong 1 | soft 3 · strong 3 · level 1 · hushed 1 |
| dynamics | direct | 14 | 28.6% | **soft 14** | soft 4 · strong 4 · level 3 · hushed 1 · shouted 2 |
| articulation | reasoned | 8 | 0.0% | **even 8** | clipped 3 · pressed 3 · drawn 2 |
| articulation | direct | 10 | 0.0% | **even 10** | clipped 2 · drawn 5 · pressed 3 |
| contour | reasoned | 6 | 50.0% | falling 3 · flat 3 | falling 6 |
| contour | direct | 5 | 0.0% | **flat 5** | falling 5 |
| pace | reasoned | 2 | 50.0% | regular 2 | regular 1 · measured 1 |
| colour | reasoned | 7 | 28.6% | dark 5 · plain 2 | dark 4 · plain 2 · bright 1 |
| colour | direct | 7 | 42.9% | **dark 7** | dark 3 · bright 2 · plain 2 |

**At these n, nothing can be concluded.** The confidence intervals run from
11.7% to 54.6% on the largest family and from 0.0% to 43.4% on the
smallest, and every one of them contains the chance line. §11's rule —
*families below chance are reworked or dropped* — **cannot be applied to
E6(a) on this evidence**, and nothing was reworked or dropped on the basis
of it. The families that §2 drops were dropped on the *measurement* (a mark
that cannot fire), not on this arm.

Two things did come out of the attempt, and both are worth more than the
numbers above.

**The cheap prompt is degenerate, and so is the grammar.** The `direct`
prompt — *"Answer directly with the word, without explaining."* — collapses
the reasoning from ~270 tokens to as few as 2, and the model then answers
with **one default word per question**: `soft` for all 14 dynamics clips,
`even` for all 10 articulation clips, `flat` for all 5 contour clips,
`dark` for all 7 colour clips. The agreement rate such an arm produces is
not inter-rater agreement at all: it is the share of clips whose mark
happens to equal LFM3's default word, dressed up as a rate. The same
degeneration appeared with the `grammar` variant (§5.1). **An LFM3 answer
on this corpus only counts if the model was allowed to think first**, and
any future arm should check the answer distribution before trusting a rate.

**With thinking allowed, the answers do vary.** On the reasoned prompt LFM3
gave `soft`/`level`/`strong` across the dynamics family, `falling`/`flat`
across the contour family and `dark`/`plain` across the colour family —
which is the behaviour an inter-rater test needs. It is not constant
everywhere: on articulation it answered `even` on all 8 clips, which may be
a real judgement (the marks put 96 of 340 clips in `even` and the envelope
is the hardest thing to hear) or a lazy default, and **with n=8 there is no
way to tell**. That question is exactly what the full arm would answer, and
it is the first thing to re-run.

#### What it takes to finish it

```bash
# on a box with the GPU to itself, or with the model served elsewhere
python3 tools/vox-e4/e6_lfm3_families.py --marks evidence/vox-e6/marks.csv \
    --clip-root ~/lm-evidence/vox-e4 --workers 4 --resume --url auto
```

The harness is resumable, it interleaves the five families clip by clip so a
partial run is still balanced, it tags every row with the prompt style, and
it refuses to scrape a truncated reasoning trace. On an idle box the
reasoned prompt runs at 2–8 s a question, which is about 90 minutes for the
whole sweep.

### 5.3 The caveat, and the throughput problem

The `lm-ears` proxy on `:8102` is restarted underneath this work often
enough that a plain run came back with nothing but "connection refused"
rows, and the model server behind it was reloading its weights more than
once. The arm therefore falls over between the proxy and the model server
behind it (`:8103`), which is the same model behind the same OpenAI-shaped
contract, and `tools/vox-e4/e6_lfm3_supervise.sh` rides out the restarts.

The machine was also running other models concurrently, which made the
per-request latency 20–60 s rather than the 2–8 s an idle box gives. That
is the binding constraint on this arm: at roughly 30 s a request, the full
1700-question sweep is a day of wall-clock on a contended box, and the arm
was run for as long as the box would give it.

**There is a cheaper variant, and it changes what is measured.** Adding
*"Answer directly with the word, without explaining."* to the prompt
collapsed the reasoning from ~270 tokens to as few as **2** on some clips —
the model then emits the bare word, and the answer is still one of the
closed options. It is a 3–10× saving on the arm's cost. It is **not** used
for the numbers below, because a two-token answer is a snap judgement and a
270-token answer is a considered one, and the point of the arm is to compare
the marks with what LFM3 *thinks* the delivery is. If the arm has to be
repeated at scale, that trade is worth making deliberately and saying so.

---

## 6. E6(c) — the marks as a gate input

The d1-2B gate (System One, `:8094`) answers the same two questions E4 and
E4b asked, from five different states over the same 340 clips
(`tools/vox-e4/e6_gate.py`):

| arm | what the gate sees | urgency (3-class) | urgent vs not | order/question | can it *read* the line? |
|---|---|---|---|---|---|
| `l2-v3` | the 7-field L2 line, E4/E4b's criteria | 45.0% | **90.6%** | 50.0% | — |
| `l2-v3-read` | the same line, asked to read the pitch | 45.0% | 90.6% | 50.0% | **88.2%** |
| `marks` | the marks, **the same criteria** | 45.0% | 80.0% | **54.7%** | — |
| `marks-vocab` | the marks, criteria in the marks' vocabulary | 45.0% | 80.0% | **67.3%** | — |
| `marks-read` | the marks, asked to read the pitch | 45.0% | 78.9% | 53.8% | **69.7%** |

The `l2-v3` arm **reproduces E4b exactly** — 45.0% / 90.6% / 50.0%, and
88.2% on the read question — so the control is sound and the two runs are
comparable clip by clip.

### 6.1 The marks beat the line where the line was stuck

On order/question, the 7-field line scores **exactly the chance line, 50.0%,
by answering "question" to all 340 clips**. The marks do not: 260 question
and 80 order, **54.7%**, and with the criteria written in the marks' own
vocabulary, **67.3%** (151 order / 189 question) against E4b's 65.6% for the
equivalent L2 arm.

So the marks **break the constant answer**. The reason is visible in the
vocabulary: the marks' contour family says *falling* or *questioning*, and
the order/question criteria can be written in exactly those words. E4b's
lesson was that a gate whose criteria share no vocabulary with the line
falls back on a constant; the marks give it a vocabulary to share.

### 6.2 The marks lose the delivery axis

On urgent-versus-not the marks score **80.0% against the line's 90.6%**. The
confusion shows exactly where it goes:

| arm | urgent caught | false alarms |
|---|---|---|
| `l2-v3` | 44/60 | **1/120** |
| `marks` | **46/60** | 22/120 |

The marks arm is *more* sensitive — it catches two more of the urgent clips —
and much less specific: it calls 22 non-urgent clips urgent. Those 22 are
**16 Neutral and 6 Calm** renders, and they have a common feature: **21 of
the 22 have a pace mark of `rushed` or `brisk`**, while 18 of the 22 are
quiet (`soft`, `level` or `hushed`) and dark. The E4 urgency criteria lead
with *"fast, loud and sharp"*, and the gate reads "fast" on its own as
urgent.

Which raises the question of why a Neutral or Calm render is fast at all.
The answer is the profile: a speaker's tempo baseline is the median over
that speaker's **whole** corpus, and the corpus mixes six styles, so a
quick Neutral render is measured against a mixed baseline and comes out
`rushed`. The same profile artifact shows up in the loudness axis (Neutral
reads quieter than Calm, §4.2).

Meanwhile the clips the gate *did* catch are overwhelmingly bright — 43 of
the 46 — so on the urgent side the gate is reading "sharp". The marks give
it a word for "sharp" it can lean on, and it leans on it; what it lacks is
a word that says "fast but not urgent".

The line loses less of this because it carries the number as well as the
word, and a reader weighing `tempo 5.4/s` against `tempo 4.2/s` is looking
at a different thing from a reader weighing `rushed` against `measured`.

**This is the price of quantizing, and it is the same price E4 paid**: the
text line loses ~40 points against a classifier over the same measurements,
and here the categorical marks lose ~10 points against the continuous line
on the axis where the numbers matter most. The categorical layer is a
*different* encoding, not a better one — see §6.4.

### 6.3 The gate can read the line better than it can read the marks

The diagnostic arm — *judging only by the description: where is the pitch at
the end of the delivery, compared with where it started?* — reads the 7-field
line at **88.2%** and the marks at **69.7%**.

E4b's read arm found the same thing about the line: of the 78 clips the gate
called "higher", 86% are questions. The line carries the contour as
continuous, profile-relative numbers, and a gate can compare them. The marks
carry it as five words, and five words are a lossy code for a number.

### 6.4 Which is more accurate, and which is shorter

**More accurate, on the lexical gate: the marks** (54.7% against 50.0% with
identical criteria; 67.3% against 65.6% with matched criteria).
**More accurate, on the delivery gate: the line** (90.6% against 80.0%).
**More accurate, at reading the pitch: the line** (88.2% against 69.7%).

**Shorter: the marks, by a lot.**

| | words in the line | words in the legend | words in the whole gate state |
|---|---|---|---|
| the marks | **6** (always 6) | 152 | 158 |
| the 7-field L2 line | 24 | 149 | 173 |

The marks line is **a quarter of the length** of the L2 line. But the legend
is the same size either way, so the *state* the gate reads is only 15 words
shorter — the saving is in the line, and it is eaten by the legend that has
to explain it. A reader that already knows the vocabulary gets the marks for
6 words; a reader that does not gets 158.

---

## 7. The System Two readability test

Twenty pairs of clips — the same words, two different deliveries, one of them
the urgent rendering — were shown to **qwen3-coder** (30B-A3B, Q4) through
Ollama, **as marks only, with no audio**, and it was asked which of the two
was the urgent one and why. The pairs cover all 20 lines and all three
speakers, and the urgent side is on the left in half the pairs by
construction. The 7-field L2 line was run over the same pairs as a control
(`tools/vox-e4/e6_readability.py`).

| what the model was shown | correct | accuracy | chance |
|---|---|---|---|
| **the marks** | 14/20 | **70.0%** | 50.0% |
| the 7-field L2 line (control) | 18/20 | **90.0%** | 50.0% |

**The marks work.** A 30B reasoning model, with no audio at all, picks the
urgent delivery out of the mark set four times in five, and it says why in
the marks' own vocabulary. That is the thing §11 asks for: *nuance that can
be named can be discussed, compared and deliberately reproduced*.

**But the dials are still better at it** — 90.0% against 70.0%. The marks
are not a replacement for the L2 line; they are a *different* view of it, and
the difference is measurable. Six of the twenty pairs the marks get wrong
are pairs where the urgent and the calm rendering differ on an axis the
marks quantize coarsely, or where the urgent side is not loud at all (Riva's
Angry style is not always louder — the marks catch the urgent clips through
`bright` and `pressed` rather than through level).

Three of the explanations, verbatim:

> **Delivery B was the urgent one. The combination of 'strong' dynamics
> (louder than usual), 'clipped' articulation (cut short), and 'bright' color
> (sharper timbre) indicates an urgent, forceful delivery that breaks from
> the speaker's normal pattern.** Delivery A's 'drawn' articulation and
> 'dark' color suggest a more relaxed, subdued delivery.

> **Delivery A was urgent because it was described as 'clipped' (cut short)
> and 'level' (usual loudness), which together suggest a sharp, immediate
> command delivered with force and urgency.** The 'clipped' articulation
> indicates the speaker was being brief and decisive, while 'level' loudness
> suggests they were speaking at normal volume but with intensity, rather
> than shouting or whispering.

> **Delivery B was the urgent one because it used 'shouted' (much louder than
> usual), 'pressed' (pushed and held above usual level), 'brisk' (faster
> pace), and 'bright' (brighter timbre), all of which indicate urgency and
> intensity.** Delivery A's 'flat' pitch, 'rushed' pace, and 'dark' timbre
> suggest a more subdued but still hurried delivery, while Delivery B's
> combination of loudness, pressure, speed, and brightness clearly indicates
> an urgent, forceful delivery.

Note what the model does with the vocabulary: it uses `pressed` against
`drawn` and `bright` against `dark` to make the call, and it reads `level`
loudness as *intense* rather than *loud* — the word is doing work that a
number alone would not.

---

## 8. What this changes in the design

1. **§11's vocabulary table is replaced by the measured one.** Five families
   and 21 marks, not six families and 28 marks. Steadiness goes, and with it
   three marks that could never fire.
2. **A rule is added: a mark that cannot fire is not in the vocabulary.**
   Vibrato, tremolo and pauses are all zero on real speech; a vocabulary that
   contains words for them is a vocabulary that teaches the reader three
   words they will never see.
3. **The marks are the notation, not the measurement.** E6 measured the two
   against each other three ways and the marks lost the delivery axis and the
   pitch read, and won the lexical one. The honest description of L2.5 is:
   **a named, categorical, human- and System-Two-readable view of the L2
   line**, not a better encoding of the measurement.
4. **The ordering rule is preserved.** The marks are written in family order
   and the marks are the *vocabulary*, so a reader can be told which axis it
   is being asked about — the E4b lesson, carried one layer up.
5. **The system must keep both.** The L2 line is what the loop reads when it
   wants the measurement; the marks are what the loop reads when it wants to
   *say* something about the delivery, and what System Two reads when it
   wants to reason about it in words. The routing fusion E4b settled on
   applies here too: the marks answer the lexical question, the line answers
   the delivery question.
6. **The marks layer still needs its inter-rater test.** E6(a) is the one
   arm that did not finish, and until it does, the claim "the marks agree
   with an independent audio model" is untested. What E6 did establish is
   the method and the trap: an LFM3 answer read without letting it reason
   is a constant per question, and a constant per question will produce a
   plausible-looking agreement rate that measures nothing.

---

## 9. What this does *not* establish

* **Nothing about human voices.** Every clip is Riva. A human delivery has
  vibrato, jitter, real pauses and a much wider dynamic range, and the
  steadiness family and the `halting` and `trailing` marks may well be
  exactly where a human corpus would need them. They are dropped *for this
  corpus*, and the config is where they come back.
* **Nothing about the *writing* direction.** E6 measures whether marks can be
  read. Whether a decision model can write a mark set that renders back to
  the delivery it came from is a separate experiment — the round trip E5
  already found lossy at the level of the L2 line.
* **Nothing about whether the marks agree with an audio model.** E6(a) is
  incomplete (§5.2). The arm got 15 of 340 clips, every confidence interval
  contains the chance line, and §11's rule — families below chance are
  reworked or dropped — was not applied on this evidence and cannot be.
* **Nothing about which mark names a human would pick.** The vocabulary here
  was named from the measurement and checked against LFM3 and against the
  style labels, but no human listener was asked. The design's own "a human
  voice test comes later, with Casey's consent" still stands.
* **Nothing about the marks as a *gate* for L3.** The d1-2B gate is System
  One. The point of the marks is System Two, and the readability test in §7
  is a proxy for that, not a test of it.

---

## 10. Reproducing this

```bash
# the marks for every clip (re-renders E4b's L2 line and checks it)
python3 tools/vox-e4/e6_marks.py --dataset evidence/vox-e4b \
    --clip-root ~/lm-evidence/vox-e4 --out evidence/vox-e6

# E6(a): LFM3 answers the same closed question per family
python3 tools/vox-e4/e6_lfm3_families.py --marks evidence/vox-e6/marks.csv \
    --clip-root ~/lm-evidence/vox-e4 --workers 4 --resume

# E6(a) and E6(b): the agreement tables
python3 tools/vox-e4/e6_agreement.py --marks evidence/vox-e6/marks.csv \
    --lfm3 evidence/vox-e6/lfm3_families.csv --out evidence/vox-e6

# E6(c): the gate arms
python3 tools/vox-e4/e6_gate.py --marks evidence/vox-e6/marks.csv \
    --out evidence/vox-e6 --workers 4

# the System Two readability test
python3 tools/vox-e4/e6_readability.py --marks evidence/vox-e6/marks.csv \
    --out evidence/vox-e6 --model qwen3-coder

# the quantizer's own checks
python3 tools/vox-ref/tests/test_marks.py
```

Services used: LFM3-Audio `:8102`, d1-2B System One `:8094`, Ollama
`:11434` (qwen3-coder, loaded on first use and unloaded with `keep_alive: 0`
when the run finishes).

The audio stays outside the repository; `--clip-root` says where it is.
