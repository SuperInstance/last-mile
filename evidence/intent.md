# The intent probe: does the ears hear the delivery?

Every clip is rendered by skald from a word, a voice and a Riva
style. Nothing was recorded from a person.

Clips: 34 in grid 1 (words fixed), 24 in grids 2-3.

## What the model answered

| grid | urgency: calm | ordinary | urgent | other | order | question | other |
|---|---|---|---|---|---|---|---|
| 1 words fixed | 34 | 5 | 0 | 29 | 0 | 27 | 7 | 0 |
| 2-text-variants | 15 | 1 | 0 | 14 | 0 | 15 | 0 | 0 |
| 3-order-question | 9 | 0 | 0 | 9 | 0 | 2 | 7 | 0 |

## Grid 1: the words fixed, the delivery varying

One order word, several styles, three voices. The last column is
what the ears heard: where the transcripts differ, the *words*
differ too, and a difference in the answer cannot be attributed
to the delivery alone.

### 'fall back'

| style | mia | jason | aria | transcripts heard |
|---|---|---|---|---|
| Calm | calm | urgent | urgent | Fall back. |
| Neutral | calm | urgent | calm | Fall back.; Pull back. |
| Default | urgent | calm | urgent | Call back.; Fall back. |
| Angry | urgent | urgent | urgent | All back!; Fall back!; It's all back. |
| Happy | urgent | urgent | urgent | Fall back.; Hallback. |
| Sad | urgent | - | urgent | Fall back!; Fall back. |

### 'hold'

| style | mia | jason | aria | transcripts heard |
|---|---|---|---|---|
| Calm | calm | urgent | urgent | Hold.; Oh. |
| Neutral | urgent | urgent | urgent | Hold.; old. |
| Default | urgent | urgent | urgent | Hold.; hold. |
| Angry | urgent | urgent | urgent | OLD!; Old! |
| Happy | urgent | urgent | urgent | Hold!; Hold.; Old! |
| Sad | urgent | - | urgent | Hold!; Hold. |

## The words held fixed *as heard*

The clips whose transcript is the same. Here the model heard the
same words in every row, so the only thing left is the delivery.

| words heard | clips | urgencies | orders |
|---|---|---|---|
| 'fall back!' | 2 | urgent, urgent | order, order |
| 'fall back.' | 10 | urgent, calm, urgent, calm, urgent, urgent, urgent, calm, urgent, urgent | order, order, order, order, order, order, order, order, order, order |
| 'hold!' | 2 | urgent, urgent | order, order |
| 'hold.' | 9 | urgent, urgent, calm, urgent, urgent, urgent, urgent, urgent, urgent | order, order, order, question, order, order, order, order, order |
| 'old!' | 4 | urgent, urgent, urgent, urgent | question, question, question, order |

Of the 5 transcript groups, 2 hold the words
fixed and still get more than one urgency answer between them (19 clips).

### Confusion table: expected urgency against the answer (grid 1)

| expected \\ heard | calm | ordinary | urgent | other |
|---|---|---|---|---|
| calm | 4 | 0 | 8 | 0 |
| ordinary | 1 | 0 | 15 | 0 |
| urgent | 0 | 0 | 6 | 0 |

### Confusion table: expected order/question against the answer (all grids)

| expected \\ heard | order | question | other |
|---|---|---|---|
| order | 42 | 7 | 0 |
| question | 2 | 7 | 0 |

## The text-only control

The same two questions with the words and no sound.

| text | urgency (no audio) | order/question (no audio) |
|---|---|---|
| hold | urgent | order |
| fall back | urgent | order |
| hold? | urgent | order |
| should we hold? | urgent | order |

## The paired control: the same clip, with and without the sound

Every clip asked twice: once with the audio, once with the words
it carries and no sound at all. `words` is what was said, `heard`
is what the ears heard. A cell that differs is a case where the
sound changed the answer.

| | urgency | order/question |
|---|---|---|
| the sound changed the answer vs the words said | 6 of 58 | 8 of 58 |
| the sound changed the answer vs the words heard | 6 of 58 | 6 of 58 |

| clip | words said | heard | urgency: sound | urgency: words | urgency: heard words | order: sound | order: words |
|---|---|---|---|---|---|---|---|
| 000_g1_hold_mia_Default.wav | hold | hold. | urgent | urgent | urgent | order | order |
| 001_g1_hold_mia_Neutral.wav | hold | Hold. | urgent | urgent | urgent | order | order |
| 002_g1_hold_mia_Calm.wav | hold | Hold. | calm | urgent | urgent | order | order |
| 003_g1_hold_mia_Angry.wav | hold | Old! | urgent | urgent | urgent | question | order |
| 004_g1_hold_mia_Happy.wav | hold | Old! | urgent | urgent | urgent | question | order |
| 005_g1_hold_mia_Sad.wav | hold | Hold. | urgent | urgent | urgent | question | order |
| 006_g1_hold_jason_Default.wav | hold | Hold. | urgent | urgent | urgent | order | order |
| 007_g1_hold_jason_Neutral.wav | hold | old. | urgent | urgent | urgent | order | order |
| 008_g1_hold_jason_Calm.wav | hold | Oh. | urgent | urgent | urgent | question | order |
| 009_g1_hold_jason_Angry.wav | hold | OLD! | urgent | urgent | urgent | question | order |
| 010_g1_hold_jason_Happy.wav | hold | Hold. | urgent | urgent | urgent | order | order |
| 011_g1_hold_aria_Default.wav | hold | Hold. | urgent | urgent | urgent | order | order |
| 012_g1_hold_aria_Neutral.wav | hold | Hold. | urgent | urgent | urgent | order | order |
| 013_g1_hold_aria_Calm.wav | hold | Hold. | urgent | urgent | urgent | order | order |
| 014_g1_hold_aria_Angry.wav | hold | OLD! | urgent | urgent | urgent | order | order |
| 015_g1_hold_aria_Happy.wav | hold | Hold! | urgent | urgent | urgent | order | order |
| 016_g1_hold_aria_Sad.wav | hold | Hold! | urgent | urgent | urgent | order | order |
| 017_g1_fall_back_mia_Default.wav | fall back | Fall back. | urgent | urgent | urgent | order | order |
| 018_g1_fall_back_mia_Neutral.wav | fall back | Pull back. | calm | urgent | urgent | order | order |
| 019_g1_fall_back_mia_Calm.wav | fall back | Fall back. | calm | urgent | urgent | order | order |
| 020_g1_fall_back_mia_Angry.wav | fall back | It's all back. | urgent | urgent | urgent | order | order |
| 021_g1_fall_back_mia_Happy.wav | fall back | Fall back. | urgent | urgent | urgent | order | order |
| 022_g1_fall_back_mia_Sad.wav | fall back | Fall back! | urgent | urgent | urgent | order | order |
| 023_g1_fall_back_jason_Default.wav | fall back | Fall back. | calm | urgent | urgent | order | order |
| 024_g1_fall_back_jason_Neutral.wav | fall back | Fall back. | urgent | urgent | urgent | order | order |
| 025_g1_fall_back_jason_Calm.wav | fall back | Fall back. | urgent | urgent | urgent | order | order |
| 026_g1_fall_back_jason_Angry.wav | fall back | All back! | urgent | urgent | urgent | order | order |
| 027_g1_fall_back_jason_Happy.wav | fall back | Fall back. | urgent | urgent | urgent | order | order |
| 028_g1_fall_back_aria_Default.wav | fall back | Call back. | urgent | urgent | urgent | question | order |
| 029_g1_fall_back_aria_Neutral.wav | fall back | Fall back. | calm | urgent | urgent | order | order |
| 030_g1_fall_back_aria_Calm.wav | fall back | Fall back. | urgent | urgent | urgent | order | order |
| 031_g1_fall_back_aria_Angry.wav | fall back | Fall back! | urgent | urgent | urgent | order | order |
| 032_g1_fall_back_aria_Happy.wav | fall back | Hallback. | urgent | urgent | urgent | question | order |
| 033_g1_fall_back_aria_Sad.wav | fall back | Fall back. | urgent | urgent | urgent | order | order |
| 034_g2_hold_mia_Default.wav | hold | Hold. | urgent | urgent | urgent | order | order |
| 035_g2_hold_jason_Default.wav | hold | old. | urgent | urgent | urgent | order | order |
| 036_g2_hold_aria_Default.wav | hold | Hold. | urgent | urgent | urgent | order | order |
| 037_g2_hold!_mia_Default.wav | hold! | Hold. | urgent | urgent | urgent | order | order |
| 038_g2_hold!_jason_Default.wav | hold! | Hold. | urgent | urgent | urgent | order | order |
| 039_g2_hold!_aria_Default.wav | hold! | Hold. | urgent | urgent | urgent | order | order |
| 040_g2_hold..._hold_mia_Default.wav | hold... hold | Hold, hold. | urgent | urgent | urgent | order | order |
| 041_g2_hold..._hold_jason_Default.wav | hold... hold | Hold, hold. | urgent | urgent | urgent | order | order |
| 042_g2_hold..._hold_aria_Default.wav | hold... hold | Hold, hold. | urgent | urgent | urgent | order | order |
| 043_g2_fall_back_mia_Default.wav | fall back | Fall back. | urgent | urgent | urgent | order | order |
| 044_g2_fall_back_jason_Default.wav | fall back | Fall back. | urgent | urgent | urgent | order | order |
| 045_g2_fall_back_aria_Default.wav | fall back | Fall back. | calm | urgent | urgent | order | order |
| 046_g2_fall_back_now!_mia_Default.wav | fall back now! | fall back now. | urgent | urgent | urgent | order | order |
| 047_g2_fall_back_now!_jason_Default.wav | fall back now! | Fall back now. | urgent | urgent | urgent | order | order |
| 048_g2_fall_back_now!_aria_Default.wav | fall back now! | Call back now. | urgent | urgent | urgent | order | order |
| 049_g3_hold?_mia_Default.wav | hold? | Hold. | urgent | urgent | urgent | order | order |
| 050_g3_hold?_jason_Default.wav | hold? | Hold. | urgent | urgent | urgent | order | order |
| 051_g3_hold?_aria_Default.wav | hold? | Hold? | urgent | urgent | urgent | question | order |
| 052_g3_should_we_hold?_mia_Default.wav | should we hold? | Should we hold? | urgent | urgent | urgent | question | question |
| 053_g3_should_we_hold?_jason_Default.wav | should we hold? | Should we hold? | urgent | urgent | urgent | question | question |
| 054_g3_should_we_hold?_aria_Default.wav | should we hold? | Should we hold? | urgent | urgent | urgent | question | question |
| 055_g3_are_you_with_me?_mia_Default.wav | are you with me? | Are you with me? | urgent | urgent | urgent | question | question |
| 056_g3_are_you_with_me?_jason_Default.wav | are you with me? | Are you with me? | urgent | urgent | urgent | question | question |
| 057_g3_are_you_with_me?_aria_Default.wav | are you with me? | Are you with me? | urgent | urgent | urgent | question | question |

## Repeatability

The same clip, asked again, greedy (temperature 0).

| clip | urgency answers | order answers | stable |
|---|---|---|---|
| intent/000_g1_hold_mia_Default.wav | urgent, urgent, urgent | order, order, order | yes |
| intent/001_g1_hold_mia_Neutral.wav | urgent, urgent, urgent | order, order, order | yes |
| intent/002_g1_hold_mia_Calm.wav | calm, calm, calm | order, order, order | yes |
| intent/003_g1_hold_mia_Angry.wav | urgent, urgent, urgent | question, question, question | yes |
| intent/004_g1_hold_mia_Happy.wav | urgent, urgent, urgent | question, question, question | yes |
| intent/005_g1_hold_mia_Sad.wav | urgent, urgent, urgent | question, question, question | yes |

Stable on 6 of 6 clips.

## The numbers that decide it

- Distinct urgency answers for the *same words* (grid 1): fall back: 2 (calm, urgent), hold: 2 (calm, urgent). One answer means the delivery was ignored.
- The model's prior: 29 of 34 grid-1 clips were called urgent, and 52 of 58 clips overall.
- Order/question: 49 of 58 clips right overall (42/49 on the orders, 7/9 on the questions).
- The sound changed the urgency answer on 6 of 58 clips (vs the words said) and on 6 of 58 (vs the words heard).
- The sound changed the order/question answer on 8 of 58 clips (vs the words said) and on 6 of 58 (vs the words heard).
- Repeatability: 6/6 clips gave the same answer twice.

## What this means for the squad

The order/question answer is the usable one: the words alone (the
text-only control) never carry it, and the audio carries it for
the questions it is asked about.

The urgency answer is not yet usable as a score input: the model
calls almost everything urgent, so the answer is mostly the
model's prior and not the delivery.

And the delivery axis is not word-preserving: the Riva styles
change what the words *are* as heard, so a delivery signal and a
mishearing are tangled until the TTS is constrained.
