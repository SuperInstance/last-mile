# E0 — Prior art survey for Vox Score

*2026-10-29. Question (design §9 E0): does an existing protocol already do layered,
interpretable, bidirectional speech performance for AI loops?*

## 0. Method and what this survey can and cannot claim

Fifteen web searches against primary sources (specifications, papers, official
repositories). Where a claim below rests on a source I did not open directly, the
source is named and the claim is kept to what the source itself states. Every
entry gives: what the artifact does, the URL, and **exactly how it overlaps the
§3 claim**.

The §3 novelty claim has four parts, and the survey treats them separately
because they have very different levels of prior art:

1. a layered, deterministic, interpretable performance protocol for speech;
2. designed as the interchange in an AI decision loop, where small models both
   read and write it as closed-choice dials;
3. a per-speaker calibration profile, learned over time, kept only when
   outcomes measurably improve;
4. logged so that replays are bit-exact.

**Verdict: (1) is mostly not novel, (2) is the defensible novelty, (3) is
partly novel, (4) is mostly a repo-internal property.** Details in §4.

This is a survey, not a proof of absence. Searches do not establish that no such
protocol exists; they establish that the well-known candidates in the design
doc's own list, plus the closest neighbours found here, do not cover parts 1–4
together.

---

## 1. Acoustic feature sets — the channel inventory

### eGeMAPS / eGeMAPSv2
- **Does:** the minimal standard paralinguistic feature set: 88 LLDs (pitch,
  energy, spectral balance, voice quality), 24 functionals and 24 deltas
  (voicing, shimmer, HNR, …). Computed by openSMILE.
- **Sources:** <https://github.com/rmspeeleu/eGeMAPS> ·
  <https://pypi.org/project/opensmile/> ·
  <https://audeering.github.io/opensmile-python/api/opensmile.FeatureSet.html> ·
  <https://www.mdpi.com/2624-6120/7/3/41> (evaluation of the feature set) ·
  <https://arxiv.org/pdf/2412.11896>
- **Overlap with §3:** eGeMAPS is the closest existing definition of *which
  channels* Vox Score's L0 should carry. It standardises F0, loudness, spectral
  tilt/balance, and voice-quality measures — the same list the design doc
  proposes for L0. Vox Score's claim to difference is *not* the channel set; it
  is that the channels become a **layered, time-stamped event stream** rather
  than a feature vector per clip, and that the vector is never the unit of
  interchange. The design doc's own table says this ("not a feature vector per
  clip"); the survey confirms the channel list is settled prior art and should
  be cited, not re-invented.

### ComParE (Computational Paralinguistics Corpus)
- **Does:** a challenge series with a common corpus and fixed descriptor
  sets — e.g. arousal, valence, dominance, emotion, likelihood of interruption,
  turns, overlap — computed from audio over fixed-length windows.
- **Sources:** <https://www.isca-archive.org/interspeech_2014/schuller14_interspeech.pdf>
  (the INTERSPEECH 2014 description) · <http://www.compare.openaudio.eu/> (the
  running challenge site) ·
  <https://hal.science/hal-01993250/document> (the retrospective on the
  challenge series)
- **Overlap with §3:** ComParE is the strongest existing precedent for
  **"an LLM-facing summary of how a line was said"**, because its descriptors
  are literally per-window arousal/valence/dominance summaries — the same role
  L2 plays. It is also a precedent for *the measurement being mechanical and
  reproducible*. Where ComParE is flat and Vox Score is layered: ComParE
  descriptors are scalars per window, whereas L2 is a patch with named axes.
  Crucially ComParE is **analysis-only** — there is no path from a descriptor
  set back to a rendered performance. Part 2 of the claim is untouched by it.

### Prosody feature tooling (DisVoice; Dehak joint factor analysis)
- **Does:** off-the-shelf prosody feature extractors; e.g. DisVoice's static
  prosody matrix is 103 features (F0 mean/std/max/min/skew/kurtosis, tilt and
  MSE of a linear fit to F0 and energy, voiced/unvoiced rates, pause
  durations, duration ratios), and the underlying statistics come from Najim
  Dehak's joint factor analysis work on prosodic features for speaker
  verification.
- **Sources:** <https://disvoice.readthedocs.io/en/latest/Prosody.html> ·
  <https://www.isca-archive.org/interspeech_2014/kaya14_interspeech.pdf>
- **Overlap with §3:** this is the most direct existing implementation of an
  "L2 patch" as a fixed vector of interpretable statistics — including
  contour shape (tilt), tempo proxies (voiced rate, pause durations) and
  envelope shape. Vox Score's L2 is not novel *as a feature set*. The
  difference is that Vox Score's L2 is (a) **prescribed text** that a small
  decision model can read and a human can edit, (b) **round-trippable** (the
  same patch can be written and then re-rendered), and (c) defined relative to
  a speaker profile rather than absolute.

---

## 2. Parametric vocoders — the renderer and the bandwidth comparison

### WORLD
- **Does:** a high-quality neural-free vocoder that analyses speech into three
  streams — F0, spectral envelope, aperiodicity — and resynthesises from them.
  Widely reimplemented (PyWORLD, WORLD in many toolchains).
- **Sources:** <https://pypi.org/project/pyworld/> ·
  <https://www.jstage.jst.go.jp/article/transinf/E99.D/7/E99.D_2015EDP7457/_pdf/-char/en>
  (WORLD: a vocoder-based high-quality speech synthesis system) ·
  <https://pmc.ncbi.nlm.nih.gov/articles/PMC12048035/>
- **Overlap with §3:** WORLD is nearly Vox Score's L0 in content. L0 proposes
  F0, RMS, F1/F2/F3, spectral tilt, voicing/noise — WORLD's F0 + spectral
  envelope + aperiodicity is a partition of the same territory, and it is
  *already* a renderer, which is exactly the "LPC/formant voice driven by the
  same L0/L1" rung in design §6.2. So the design's "LPC voice" rung is really
  a WORLD/DDSP voice, and the design should say so. WORLD also **answers the
  §5 bandwidth comparison**: a WORLD-parameterised frame set is a compact
  stream well inside 300 B/s, which is a challenge to Vox Score's claimed
  novelty on bandwidth alone. Vox Score's differentiator cannot be "small"; it
  must be "layered, machine-writable and bidirectional".

### LPC-10 (FIPS 137 / FED-STD-1015, NATO STANAG 4198)
- **Does:** the US/NATO linear-predictive-coding vocoder standard. 8 kHz
  target, 180-sample frames at 44.44 frames/s, order-10 LPC, voiced/unvoiced
  decision, pitch, gain — 54 bits per frame, 2,400 bit/s (0.3 bit/sample,
  ~26.6:1). Used in secure voice equipment (STU-III, KY-57); LPC-10e added a
  better voicing classifier. Often described as "buzzy", and poor in
  background noise.
- **Sources:** <https://www.cryptomuseum.com/ref/codec/lpc10/> (spec sheet,
  frame layout, bit budget) ·
  <https://github.com/jafingerhut/lpc10/blob/main/lpc10-1.5/FAQ> (Tremain,
  "The Government Standard Linear Predictive Coding Algorithm: LPC-10") ·
  <https://ee.stanford.edu/~gray/lpcip/nclpcip.pdf> (Gray, "Packet speech on
  the Arpanet") · <https://dspini.com/files/datasheets/melpe2400.pdf>
- **Overlap with §3:** LPC-10 is the design's bandwidth yardstick (2,400 bit/s
  = 300 B/s) *and* a precedent for exactly the design's privacy argument:
  §8's claim that "L0 with formants can be resynthesized into intelligible
  speech, so it is not anonymous" is the LPC-10 argument verbatim — LPC-10
  exists to reconstruct intelligible speech from a formant+pitch parameter
  stream. The design's `prosody-only` tier is therefore not a novel idea
  either; it is the familiar "drop the formants" degradation ladder. Also note
  LPC-10's 54-bit frame is a *better* precedent for the encoding than anything
  Vox Score proposes: a **fixed-layout frame**, not a delta/running-status
  scheme. The design should consider it as a competitor to §5.

### Related low-bitrate codecs (for the §5 comparison table)
- **MELPe:** 2,400 / 1,200 / 600 bit/s military vocoder —
  <https://dspini.com/files/datasheets/melpe2400.pdf>
- **Codec 2:** 700 bit/s open-source OFDM vocoder, the modern low end —
  <https://github.com/meshtastic/codec2> · <https://www.rowetel.com/wordpress/?page_id=452>
- **LPCNet meets Codec 2:** neural-assisted Codec 2 —
  <https://www.rowetel.com/wordpress/?p=6513>
- **Overlap:** these bracket the design's comparison point from below. A
  700 bit/s intelligible vocoder is far under Vox Score's 150–300 B/s raw-L0
  budget, so "small" is not a defensible differentiator against *voice*; it is
  only defensible against *neural codec tokens* (below).

---

## 3. Prosody annotation and markup — the closest structural precedents

### ToBI and RaP (Tones and Break Indices, plus Rhythm and Pitch)
- **Does:** ToBI annotates speech with **four time-aligned tiers**: an
  orthographic tier, a tonal tier labelling pitch events, a break-index tier
  labelling perceived disjuncture, and a miscellaneous tier. RaP adds
  rhythm-and-pitch labels on the same aligned-grid principle.
- **Sources:** <https://catalog.ldc.upenn.edu/docs/LDC2018S04/Inter-Transcriber_Reliability_for_Two_Systems_of_Prosodic_Annotation.pdf>
  (the ToBI/RaP transcription description) ·
  <https://www.ling.ohio-state.edu/research/phonetics/E_ToBI/> (labelling
  guidelines) · <https://academic.oup.com/book/4251/chapter/146095715>
  (history of ToBI)
- **Overlap with §3:** **this is the single closest structural precedent for
  the layering claim.** ToBI already is "a layered, time-aligned,
  interpretable performance description of speech". The design's "MIDI of
  speech" framing is, in ToBI terms, "ToBI with machine-writable tiers".
  Where Vox Score genuinely differs: ToBI is a **hand annotation scheme**
  (the LDC document above is literally about inter-transcriber reliability),
  it is **symbolic and categorical**, and it is **one-way** — nothing renders
  from ToBI. Vox Score's measured-continuously, renderer-driving, bidirectional
  claim is a real difference, but it is a difference *from ToBI*, not from
  nothing. **The design doc should cite ToBI as the ancestor of the layering
  idea and stop claiming the layering itself is novel.**

### SSML (W3C Speech Synthesis Markup Language) and `<prosody>`
- **Does:** the W3C standard for annotating text for synthesis. `<prosody>`
  carries **rate, pitch, contour, range and volume** — a five-axis performance
  patch, with pitch in absolute Hz / relative st / relative % / enumerated
  levels (x-low…x-high), and `contour` as a list of (time %, pitch) targets.
  `<voice>` carries voice, language, style and role; `<phoneme>` and
  `<lexicon>` carry pronunciation.
- **Sources:** <https://www.w3.org/TR/2001/WD-speech-synthesis-20010103/> ·
  <https://www.w3.org/Voice/2007/speech-synthesis11/WD-speech-synthesis11-20070611diff.html>
  · <https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-synthesis-markup-voice>
  (the concrete `<prosody>` attribute semantics) ·
  <https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-synthesis-markup-pronunciation>
  · <https://w3c.github.io/pronunciation/technical-approach/> (SSML in HTML)
- **Overlap with §3:** **SSML is the strongest existing claim on the design.**
  It is a decade-old W3C standard; it is layered (document → voice → prosody →
  phoneme); it is human- and machine-writable; it is a **closed set of named
  axes**; it is **per-utterance and machine-writable at will**; and it is
  *bidirectional in spirit* — humans author it, machines consume it. It also
  already supports **styles as closed presets** (`mstts:express-as`, Azure's
  `<voice name=… style=…>`), which is exactly the "gesture presets as
  closed-choice gates" the design wants.
  Three honest differences remain, and they are the design's real ground:
  (a) SSML's axes are **coarse categorical/relative** — "pitch +2st", "rate
  +40%" — with no measured ground truth; Vox Score's are **measured from the
  signal** and expressed **relative to a speaker profile**; (b) SSML has **no
  analysis path** — nothing measures a performance and emits SSML, and
  nothing measures the render to check SSML was honoured; (c) SSML has **no
  closed-set gate interface** — it is not shaped for a small decision model to
  read or write as a few choices.
  Also relevant to E5: because SSML/renderer styles already *are* closed
  emotion presets, E5 must show that a **d1-2B-written L2 patch** lands in a
  measurable class, not merely that it selects a different Riva style.

### Azure custom lexicon (PLS) — the "lexicon" precedent
- **Does:** a per-voice XML/PLS lexicon file that overrides how specific
  entities are read (grapheme → phoneme/alias), cached per voice and locale.
- **Source:** <https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-synthesis-markup-pronunciation>
- **Overlap with §3:** design §7's "prosody lexicon" has a commercial
  precedent in *shape* (a per-voice, human-readable, machine-loaded override
  file). The precedents do not carry the design's **outcome-verified**
  element: the design's entries are "sealed with a prediction, tested on
  held-out seeds, and adopted only if outcomes move as promised". That
  sealed-prediction discipline is the part with no prior art found here.

### EmotionML (W3C)
- **Does:** a W3C vocabulary for annotating emotion in speech and other media
  — dimensional (valence/arousal/dominance) and categorical appraisals, with
  confidence and role attributes.
- **Sources:** <https://www.w3.org/TR/emotionml/> ·
  <https://www.w3.org/TR/2011/WD-emotionml-20110407/> ·
  <https://www.w3.org/WAI/GL/task-forces/coga/wiki/Emotion_Markup>
- **Overlap with §3:** EmotionML is a direct precedent for a **shared,
  standards-based, machine-readable annotation of affect** — i.e. the design's
  L3 intent layer and the "emotion preset" axis have a W3C vocabulary already.
  It is annotation only, one-way, and not a renderer input; it does not
  threaten parts 1 or 2 of the claim, but it does mean the design should not
  present "how to express urgency/valence in a machine-readable way" as novel.

### Compact parametric contour models (Tilt, Tiltinton, TSM)
- **Does:** reduced-dimensional, interpretable descriptions of intonation.
  The **Tilt** model replaces rise/fall/duration/amplitude parameters with
  three per event (duration, amplitude, dimensionless *tilt*, where +1 is a
  pure rise, 0 a rise-fall, −1 a pure fall); the **Tiltinton** model adds
  pitch range and voice setting; the **TSM** model represents an intonation
  movement with five parameters (base level, amplitude, movement type, peak
  alignment, steepness).
- **Sources:** <https://www.cstr.ed.ac.uk/downloads/publications/2000/Taylor_2000_b.pdf>
  (Taylor, "Analysis and synthesis of intonation using the Tilt model") ·
  <https://www.isca-archive.org/icslp_1998/mohler98_icslp.pdf> ·
  <https://era.ed.ac.uk/server/api/core/bitstreams/6c106d5a-ca39-4686-8ff4-bb1d3bda8b0c/content>
- **Overlap with §3:** this is the strongest precedent for the design's
  *contour class* axis (rise/fall/flat/rise-fall) and for the whole idea of
  summarising a performance in a handful of named, interpretable parameters.
  Vox Score's L2 "contour class" is a coarse, categorical cousin of Tilt's
  dimensionless tilt parameter. The design should treat the Tilt family as the
  scholarly ancestor of L2's contour axis, and — because Tiltinton already
  pairs a contour with a **per-speaker pitch range and voice setting**, i.e.
  a profile — it is also precedent for §7.

---

## 4. Singing synthesis — writing a performance, but in music

### Vocaloid / UTAU
- **Does:** voice-bank singing software where a performance is authored as
  **notes + per-note expression parameters** (pitch curve, vibrato depth and
  delay, dynamics/attack-decay envelope, timing) over a phoneme track; the
  result is editable, human-readable and re-renderable in any compatible
  voice bank.
- **Sources:** <https://www.vocaloid.com/en/articles/vocaloid4_feature> ·
  <https://www.vocaloid.com/en/learn/ln6214/> ·
  <https://www.vocaloid.com/en/learn/ln6216/> ·
  <https://www.scribd.com/document/998556783/VOCALOID-Reference-Manual-ENG>
- **Overlap with §3:** the design's own table names this, and the survey
  confirms the overlap is deep: a note + expression + timing grid that a human
  edits and a renderer consumes **is** the "MIDI of singing", and it is
  twenty-five years old. Two differences hold: Vocaloid's expression
  parameters are **hand-authored per note**, not measured from a signal; and
  the authoring surface is a **GUI timeline**, not a text line a small
  decision model can read and write as closed choices. The design's claim is
  therefore specifically about the *interface* to the model, not about the
  existence of editable performance data.

### MIDI-driven singing synthesis (MIDI-DDSP; VISinger; DiffSinger; ExpressiveSinger)
- **Does:** the closest thing to "write a speech performance and hear it".
  - **MIDI-DDSP** synthesises vocals from MIDI with DDSP, disentangling F0,
    aperiodicity and loudness, so the *performance* (MIDI) and the *sound*
    (timbre) are separate — which is literally the design's opening sentence,
    already realised for singing.
  - **VISinger** (VITS-based) generates singing directly from lyrics + musical
    score, with an F0 predictor and note-duration normalisation to stabilise
    intonation.
  - **ExpressiveSinger** (ACM MM '24) takes **score + lyrics + style tokens +
    singer embedding** and generates three explicit expressive performance
    control signals — **phoneme-level timing, F0 contour, and amplitude
    envelope** — via a cascade of diffusion models.
- **Sources:** <https://pure.korea.ac.kr/en/publications/midi-voice-expressive-zero-shot-singing-voice-synthesis-via-midi-/>
  (MIDI-DDSP) · <https://arxiv.org/html/2110.08813v1> (VISinger) ·
  <https://dl.acm.org/doi/pdf/10.1145/3664647.3681642> (ExpressiveSinger) ·
  <https://neural-audio-theory.vercel.app/docs/producer-handbook/vocal-synthesis>
- **Overlap with §3:** **this is the most damaging finding for the design's
  framing.** ExpressiveSinger's three control signals — timing, pitch contour,
  loudness envelope — are Vox Score's L0/L1 content, in a *learned renderer*,
  with a *per-singer embedding* that is Vox Score's speaker profile, and with
  the same score/performance split the design opens with. The design's
  differentiators reduce to: **speech rather than singing**, **measured rather
  than authored**, **a text line rather than a score**, **and an AI decision
  loop rather than a human editor**. That is a narrower and more defensible
  claim than "the MIDI of speech", and the design should be rewritten to say
  it plainly. Note also the paper's own warning that "using performance MIDI
  while claiming to be score-based is misleading" — a direct caution against
  Vox Score calling a measured patch "a score".

---

## 5. Neural codecs — the "sampler" side of the split

### EnCodec
- **Does:** a real-time streaming neural audio codec; 24 kHz mono at
  1.5/3/6/12/24 kbps, 48 kHz stereo at 3/6/12/24 kbps, residual vector
  quantisation (2/4/8/16/32 codebooks), 75 latent steps per second at 24 kHz,
  ~13.3 ms initial latency, plus an optional language model for a further
  25–40% reduction.
- **Sources:** <https://github.com/facebookresearch/encodec> ·
  <https://ar5iv.labs.arxiv.org/html/2210.13438> ·
  <https://facebookresearch.github.io/audiocraft/docs/ENCODEC.html>
- **Overlap with §3:** EnCodec is the design's "sampler" — audio as opaque
  tokens. It overlaps with Vox Score in the *one* way that matters: it is
  fast, small, and lossy-but-complete. Vox Score's answer is that its tokens
  are *interpretable and editable*, which EnCodec's are not (a codebook index
  is not a pitch). EnCodec's paper also reports EnCodec+LM beating Opus at
  6 kbps on MUSHRA, so the "opaque tokens are wasteful" argument must be made
  carefully.

### SoundStream (Google) and Lyra
- **Does:** the fully-convolutional encoder/decoder with RVQ that EnCodec
  benchmarks against; Lyra is Google's deployed neural codec, referenced in
  the EnCodec paper at 3.2 and 6 kbps.
- **Sources:** <https://ar5iv.labs.arxiv.org/html/2210.13438> (SoundStream
  described and re-implemented) · <https://research.google/pubs/ddsp-differentiable-digital-signal-processing/>
- **Overlap with §3:** the same category as EnCodec; also the source of the
  "neural codec at a few kbps" benchmark the design's §5 must acknowledge
  rather than pretend away.

### Mimi (Kyutai) / Moshi
- **Does:** a speech codec at **12.5 Hz** (one frame per 80 ms), the audio
  token stream of the Moshi duplex speech model.
- **Sources:** <https://github.com/kyutai-labs/moshi/blob/fa205be6/README.md?plain=1>
- **Overlap with §3:** 12.5 frames/s is *below* the design's 100 Hz L0 rate by
  8×, so Mimi is a direct threat to any bandwidth argument. But Mimi's tokens
  are not editable, not interpretable and not re-writable by a decision model,
  so the "sampler vs MIDI" split in the design's table holds — as long as the
  argument is made on **editability and interpretability**, never on size.

### WavTokenizer
- **Does:** a discrete audio codec at **40 or 75 tokens per second** (0.5 or
  0.9 kbps), explicitly framed as an **acoustic codec for audio language
  models** whose tokens "inherently contain richer semantic information" and
  are "built for audio language models such as GPT-4o".
- **Sources:** <https://github.com/Rita-zi/WavTokenizer> ·
  <https://github.com/jishengpeng/wavtokenizer> ·
  <https://huggingface.co/Rita-zi/WavTokenizer-small-600-24k-4096>
- **Overlap with §3:** the most aggressive challenge to the design's premise.
  WavTokenizer asserts that a *single* learned code stream can be both very
  small and semantically rich enough for an LLM to consume directly — i.e.
  that "audio as opaque tokens" is not necessarily the waste the design
  assumes. Vox Score's answer must be that WavTokenizer's tokens are still
  not *writable* as intent and not *editable* as performance; it is a
  better listener's format, not a writer's.

### LiquidCodec (Liquid AI)
- **Does:** the in-house audio tokenizer of LFM3-Audio-2B: **12 tokens per
  80 ms**, streaming decoding, ~87× real time on one CPU thread, with the
  backbone emitting speech tokens directly.
- **Sources:** <https://huggingface.co/LiquidAI/LFM3-Audio-2B> ·
  <https://huggingface.co/blog/LiquidAI/lfm3>
- **Overlap with §3:** LiquidCodec is the design's own E4 tool (LFM3-Audio) and
  is simultaneously prior art for the "sampler" side. It is the concrete
  demonstration that an audio-token stream and a decision model can coexist —
  but it is unidirectional (audio → tokens → audio), with no intent layer, no
  gesture layer and no closed-choice dial interface.

### Probing neural audio codecs with speaker traits (2026)
- **Does:** analyses what speaker-level information neural codecs retain or
  discard — directly relevant to design §8's privacy tiers.
- **Source:** <https://arxiv.org/html/2610.20955>
- **Overlap with §3:** evidence that "which traits does the token stream
  keep" is an active, published question — i.e. the design's claim that a
  `prosody-only` tier is "not anonymous" and that a formant-bearing tier *is*
  anonymous-ish has to be argued against codec literature, not just LPC-10
  folklore.

---

## 6. Differentiable DSP and analysis-by-synthesis

### DDSP (Google Magenta)
- **Does:** a differentiable synthesis framework: a **harmonic-plus-noise**
  oscillator bank plus a learned filter, with the controls (frequencies,
  amplitudes, noise/decay rates) exposed as differentiable tensors, so
  timbre can be learned end-to-end and the synthesis graph can be rendered to
  audio.
- **Sources:** <https://research.google/pubs/ddsp-differentiable-digital-signal-processing/>
  · <https://magenta.withgoogle.com/ddsp>
- **Overlap with §3:** DDSP is the design's "honesty check" (design §6.3:
  render a Vox Score, analyse it again, measure round-trip error per channel)
  and its rationale for a **harmonic/noise** signal model — which is precisely
  Vox Score's L0 "voicing/noise ratio" channel, already given a canonical
  parameterisation. DDSP also demonstrates that the analysis-synthesis
  parameter set can be made differentiable, which means the design's §6.3
  check has a strong existing implementation to compare against.

---

## 7. MIDI 2.0 and MPE — the borrowed machinery

### MIDI 2.0 / UMP
- **Does:** the current MIDI specification: 32-bit data words (vs 16), the
  **Universal MIDI Packet (UMP)**, **per-note controllers**, **MIDI 2.0
  Profiles** (a profile groups message types and declares which are
  supported), and per-note attribute types (including type 2 per-note
  attribute messages carrying types such as pitch/slide and other per-note
  attributes).
- **Sources:** <https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification>
  · <https://midi.org/midi-2-0-protocol> · <https://midi.org/midi-2-0> ·
  <https://midi.community/docs/midi-2/> ·
  <https://midi.org/the-state-of-midi-2-0-high-resolution-performance-and-the-rise-of-profiles-update-feb-2026>
- **Overlap with §3:** the design says it borrows running status, per-note
  expression and profiles. That borrowing is well-founded and the debt should
  be stated: **running status** (the design's §5 "send only the fields that
  changed") is MIDI 1.0's byte-saving trick and is exactly right for L0's
  9-byte frames; **per-note expression** is the L1 gesture model; and
  **profiles** are the design's §7 speaker profile, which MIDI solved first
  (a profile is a per-device description of what messages a device supports).
  The design's "profile" is richer — it is a *measured calibration* rather
  than a *capability declaration* — but the concept is MIDI's.

### MPE (MIDI Polyphonic Expression)
- **Does:** a MIDI specification layer giving **per-note expression** for
  slide, pressure, pitch bend, timbre and duration, so that each note carries
  its own expression independent of the performance-wide controls.
- **Sources:** <https://midi.org/mpe-midi-polyphonic-expression> ·
  <https://midi.community/docs/mpe/> ·
  <https://polyphonicexpression.com/mpe-how-it-works> ·
  <https://www.ableton.com/en/manual/editing-mpe/>
- **Overlap with §3:** MPE is the precedent for the design's L1 gesture layer
  (onset, pitch target and glide, velocity, articulation) and for the
  separation of *per-note* expression from *global* expression — which is
  Vox Score's L1/L2 split. It also shows the design's layering is not
  idiosyncratic: per-note + global + profile is a proven three-tier design.

### Audio-generation "dials" (MusicGen steering)
- **Does:** MusicGen (a single-stage autoregressive Transformer over a 32 kHz
  EnCodec tokenisation at 50 Hz) is controllable at generation time through
  **real-time steering** — a control surface of continuous parameters
  (continuity, density, loudness, text/melody) fed over OSC while audio
  streams.
- **Sources:** <https://github.com/facebookresearch/audiocraft/blob/main/docs/MUSICGEN.md>
  · <https://facebookresearch.github.io/audiocraft/docs/MUSICGEN.html>
- **Overlap with §3:** a strong precedent for the design's "dials" intuition
  applied to *audio generation*: continuous knobs, steered live, driving a
  generative model. The differences: MusicGen's dials steer *one model
  directly*, with no shared protocol, no text representation, no measurement
  layer and no separate analysis of what was produced.

---

## 8. Voice profiles, calibration and personalization

### Speaker embeddings and speaker verification
- **Does:** speaker embeddings (x-vectors, ECAPA-TDNN and successors) reduce a
  speaker to a fixed vector used for verification, clustering and adaptation;
  Dehak's joint factor analysis of prosodic features (cited in §1) does the
  analogous thing for prosody.
- **Sources:** <https://www.isca-archive.org/interspeech_2014/kaya14_interspeech.pdf>
  · <https://pypi.org/project/pyworld/> (see the WORLD/Dehak lineage)
- **Overlap with §3:** prior art for **normalising a performance against a
  speaker reference** — exactly the design's §7 "loud means loud *for this
  player*". The design's profile is however required to be **human-readable
  and editable** (design §7: "the player can open them and see how the squad
  has learned to hear them"), which is the opposite of a 192-dimension
  embedding. Readability is the differentiator, not calibration.

### Zero-shot / personalised TTS and speaker-conditioned synthesis
- **Does:** cloning and personalisation systems condition a synthesis model on
  a short reference utterance or a speaker embedding; ExpressiveSinger (§4)
  uses a "speaker/singer embedding instead of the traditional singer ID".
- **Sources:** <https://dl.acm.org/doi/pdf/10.1145/3664647.3681642> ·
  <https://pure.korea.ac.kr/en/publications/midi-voice-expressive-zero-shot-singing-voice-synthesis-via-midi-/>
- **Overlap with §3:** these learn a *latent* per-speaker adaptation;
  Vox Score's profile is a *measured, interpretable, editable* set of
  statistics, and the design's learning flow is gated on **measured outcome
  improvement** ("kept only when outcomes measurably improve"), which is a
  property of the learning loop rather than of the profile format.

---

## 9. What the survey changes

### 9.1 The novelty claim, narrowed

| Part of §3's claim | Status after E0 | Why |
|---|---|---|
| 1. layered, interpretable performance protocol | **Not novel as such.** Narrowed. | ToBI/RaP already layer prosody over time; SSML already layers markup; Tilt/Tiltinton already compress a contour to named parameters; eGeMAPS/ComParE/DisVoice already fix the channel set and the summary statistics. Vox Score's remaining difference: **measured continuously, in one document, with a renderer that consumes it.** |
| 2. interchange in an AI decision loop, models read and write it as closed-choice dials | **Novel — the defensible core.** | Nothing found does this. SSML/EmotionML are human-authored annotations; ToBI is hand-labelled; Ctrl-P is a *model* that consumes prosody controls, not a protocol; MusicGen's dials steer one model and are not a shared, logged, analysable document. **This is where Vox Score should stake its claim.** |
| 3. per-speaker calibration, learned, kept only when outcomes measurably improve | **Partly novel.** | Calibration relative to a speaker reference is old (speaker embeddings, Tiltinton's voice setting, keyboard velocity curves). The *outcome-gated, sealed-prediction* learning discipline and the *human-readable editable* profile format were not found in any prior source. |
| 4. logged so replays are bit-exact | **Not novel in kind; not found in this form for speech.** | MIDI's running status and event log, and this repo's existing hash-chained L3 log, already do the logging; what is missing is bit-exact logging of a *measured performance*. The determinism requirement mostly restates design §8's D67/D69, and should be presented as an engineering constraint, not a novelty. |

### 9.2 Corrections the survey forces on the design doc

1. **"The MIDI of speech" overclaims.** MIDI-DDSP, ExpressiveSinger and
   Vocaloid already split performance from sound for *singing*, with per-note
   expression and per-singer adaptation. The honest claim is: *the MIDI of
   **speech**, and with a decision model on both ends.*
2. **SSML `<prosody>` is the real ancestor of L2**, not ToBI alone. L2's axes
   (rate, pitch, contour, range, volume) are SSML's five axes, with
   continuous measured values and a speaker-relative reference added. The
   design should say "SSML, measured".
3. **"Small" is not a differentiator.** LPC-10 (2,400 bit/s), MELPe
   (600 bit/s), Codec 2 (700 bit/s), Mimi (12.5 frames/s), WavTokenizer
   (40 tokens/s) and LiquidCodec (150 tokens/s) are all smaller than or
   comparable to the design's 150–300 B/s budget. The differentiator must be
   **interpretability, editability and bidirectional writability**, and §5's
   comparison table should be rebuilt around that.
4. **The §6.2 "LPC voice" rung should be named.** WORLD/DDSP is the honest
   choice for an L0-driven voice, not raw LPC-10; the design's §6.3
   "analysis by synthesis" check should be measured against DDSP's
   harmonic+noise parameterisation, which is the same family the L0 channels
   live in.
5. **§8's privacy tiers are LPC-10 folklore**, and are corroborated by codec
   probing work. State it as such rather than as a new observation.
6. **L3's affect vocabulary overlaps W3C EmotionML.** Use EmotionML's
   dimensional vocabulary rather than inventing urgency/confidence/addressee
   names from scratch, or the design will have a private vocabulary where a
   standard one exists.

### 9.3 Threats to the experiment plan, not the idea

- **E4's four arms are the right experiment.** Arm 3 (words-only) versus arm 4
  (words+L2) is precisely the test of part 2, and it is the one comparison no
  prior art offers, because no prior art produces an L2 line a decision model
  can read. If arm 4 does not beat arm 3 clearly, the claim in part 2 has no
  support at all, and the design should say so.
- **E5 is at risk of measuring the renderer, not the protocol.** Riva exposes
  emotion styles as separate voice IDs, and SSML-style presets are a closed
  set. If d1-2B can pick a style, one can argue it "wrote a performance"
  without any L2 protocol. E5 must therefore verify that the *L2 patch
  written by the model*, when rendered, lands in the intended measured class
  — i.e. that the patch carries the class, not that the preset name does.
- **The arms must not leak.** Arm 4's L2 line must contain no word content;
  arm 2's LFM3 input must be the audio only; the confusion matrices must be
  computed over identical clip sets.

### 9.4 What this survey does not settle

- Whether some non-public or non-standard "speech performance" interchange
  exists (the search space of industrial voice-agent protocols was not
  exhaustively covered).
- Whether a W3C/IETF-style standard for continuous, machine-writable speech
  performance exists under a name the searches did not hit.
- The quality of the *implementation* of any of the above — this survey
  compares formats and ideas, not systems.
- Whether the **Ctrl-P** line of work (a TTS model with disentangled,
  continuous style and prosody control) is closer to Vox Score than the
  sources here suggest. The Edinburgh record for Ctrl-P was found
  (<https://www.research.ed.ac.uk/en/publications/ctrl-p-temporal-control-of-prosodic-variation-for-speech-synthesi/>)
  but only the abstract was read; it deserves a follow-up read, because a
  model that *both accepts and produces* continuous prosody control signals
  is the nearest thing to Vox Score's loop that was found.

---

## 10. What E4 then found

The survey's conclusions were tested in `E4-E5.md`. Briefly, against the
table in §9.1:

* **Part 1 (the protocol itself) is confirmed as prior art.** The channel
  set, the parametric contour summaries and the markup all have direct
  ancestors, exactly as surveyed.
* **Part 2 (the AI decision loop) held up, with a caveat.** A profile-
  relative L2 line read by the d1-2B gate reached 82.2% on urgent-versus-
  not where the words alone were stuck at the 33.3% chance line and
  LFM3's own audio understanding reached 60.6% — and at 105 ms against
  1518 ms. The caveat is that the *text* line loses about 38 points against
  a classifier over the same measurements (83.3%), so the loop needs the
  machine-readable form, not the prompt.
* **Part 3 (the profile) turned out to be load-bearing.** Making every L2
  axis profile-relative — design §7's principle, which §9.1 had assumed
  applied only to L0 and L1 — moved the urgent-versus-not figure from
  53.9% to 82.2%. The profile is not a convenience; it is what makes the
  dials readable.
* **Part 4 (bit-exact replay) is in worse shape than the design assumes.**
  The analyzer is deterministic, but skald/Riva's synthesis is not: the
  same text rendered twice gives two different waveforms, and E5's round
  trip is therefore a round trip through a new sample each time.
* **E5 did not clear its 80% bar** (52.2%), and the decomposition says the
  binding constraint is measurability of the delivery, not the model's
  ability to write one.

The survey's own warning in §9.2 — that "the MIDI of speech" overclaims —
was borne out. MIDI-DDSP, ExpressiveSinger and Vocaloid already split
performance from sound for singing; SSML already carries a five-axis
performance patch; ToBI already layers prosody over time. What the
experiments support is narrower and more specific: a small, profile-
relative, machine-readable channel into a decision loop, which the audio
model itself does not provide.

---

## 11. Source index
| # | Artifact | URL |
|---|---|---|
| 1 | eGeMAPS (feature set, official repo) | <https://github.com/rmspeeleu/eGeMAPS> |
| 2 | openSMILE python (feature sets) | <https://audeering.github.io/opensmile-python/api/opensmile.FeatureSet.html> |
| 3 | Evaluating eGeMAPS features (MDPI) | <https://www.mdpi.com/2624-6120/7/3/41> |
| 4 | ComParE / INTERSPEECH 2014 challenge | <https://www.isca-archive.org/interspeech_2014/schuller14_interspeech.pdf> |
| 5 | compare.openaudio.eu | <http://www.compare.openaudio.eu/> |
| 6 | DisVoice prosody features | <https://disvoice.readthedocs.io/en/latest/Prosody.html> |
| 7 | Dehak, joint factor analysis (ISCA) | <https://www.isca-archive.org/interspeech_2014/kaya14_interspeech.pdf> |
| 8 | PyWORLD | <https://pypi.org/project/pyworld/> |
| 9 | WORLD paper (JSTAGE) | <https://www.jstage.jst.go.jp/article/transinf/E99.D/7/E99.D_2015EDP7457/_pdf/-char/en> |
| 10 | LPC-10 spec sheet (Crypto Museum) | <https://www.cryptomuseum.com/ref/codec/lpc10/> |
| 11 | LPC-10 FAQ / Tremain (GitHub) | <https://github.com/jafingerhut/lpc10/blob/main/lpc10-1.5/FAQ> |
| 12 | Gray, packet speech on the ARPANET | <https://ee.stanford.edu/~gray/lpcip/nclpcip.pdf> |
| 13 | MELPe 2400 bps vocoder | <https://dspini.com/files/datasheets/melpe2400.pdf> |
| 14 | Codec 2 | <https://github.com/meshtastic/codec2> |
| 15 | LPCNet meets Codec 2 | <https://www.rowetel.com/wordpress/?p=6513> |
| 16 | ToBI/RaP transcription (LDC) | <https://catalog.ldc.upenn.edu/docs/LDC2018S04/Inter-Transcriber_Reliability_for_Two_Systems_of_Prosodic_Annotation.pdf> |
| 17 | ToBI labelling guidelines | <https://www.ling.ohio-state.edu/research/phonetics/E_ToBI/> |
| 18 | SSML 1.0 (W3C) | <https://www.w3.org/TR/2001/WD-speech-synthesis-20010103/> |
| 19 | SSML 1.1 working draft (W3C) | <https://www.w3.org/Voice/2007/speech-synthesis11/WD-speech-synthesis11-20070611diff.html> |
| 20 | SSML `<prosody>` semantics (Azure) | <https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-synthesis-markup-voice> |
| 21 | SSML pronunciation / custom lexicon (Azure) | <https://learn.microsoft.com/en-us/azure/ai-services/speech-service/speech-synthesis-markup-pronunciation> |
| 22 | SSML in HTML (W3C) | <https://w3c.github.io/pronunciation/technical-approach/> |
| 23 | EmotionML 1.0 (W3C) | <https://www.w3.org/TR/emotionml/> |
| 24 | EmotionML WD (W3C) | <https://www.w3.org/TR/2011/WD-emotionml-20110407/> |
| 25 | Taylor, Tilt model (CSTR) | <https://www.cstr.ed.ac.uk/downloads/publications/2000/Taylor_2000_b.pdf> |
| 26 | Mohler, TSM 5-parameter model (ISCA) | <https://www.isca-archive.org/icslp_1998/mohler98_icslp.pdf> |
| 27 | VOCALOID4 features | <https://www.vocaloid.com/en/articles/vocaloid4_feature> |
| 28 | VOCALOID 6.5 notes | <https://www.vocaloid.com/en/learn/ln6214/> |
| 29 | MIDI-DDSP (zero-shot singing via MIDI) | <https://pure.korea.ac.kr/en/publications/midi-voice-expressive-zero-shot-singing-voice-synthesis-via-midi-/> |
| 30 | VISinger (arXiv) | <https://arxiv.org/html/2110.08813v1> |
| 31 | ExpressiveSinger (ACM MM '24) | <https://dl.acm.org/doi/pdf/10.1145/3664647.3681642> |
| 32 | Singing synthesis pipeline overview | <https://neural-audio-theory.vercel.app/docs/producer-handbook/vocal-synthesis> |
| 33 | EnCodec (GitHub) | <https://github.com/facebookresearch/encodec> |
| 34 | EnCodec paper | <https://ar5iv.labs.arxiv.org/html/2210.13438> |
| 35 | Mimi / Moshi (Kyutai) | <https://github.com/kyutai-labs/moshi/blob/fa205be6/README.md?plain=1> |
| 36 | WavTokenizer | <https://github.com/Rita-zi/WavTokenizer> |
| 37 | LiquidCodec / LFM3-Audio-2B | <https://huggingface.co/LiquidAI/LFM3-Audio-2B> |
| 38 | LFM3 blog (Liquid AI) | <https://huggingface.co/blog/LiquidAI/lfm3> |
| 39 | Probing neural audio codecs with speaker traits | <https://arxiv.org/html/2610.20955> |
| 40 | DDSP (Google Research) | <https://research.google/pubs/ddsp-differentiable-digital-signal-processing/> |
| 41 | DDSP (Magenta) | <https://magenta.withgoogle.com/ddsp> |
| 42 | MIDI 2.0 / UMP specification | <https://midi.org/universal-midi-packet-ump-and-midi-2-0-protocol-specification> |
| 43 | MIDI 2.0 protocol overview | <https://midi.org/midi-2-0-protocol> |
| 44 | MIDI 2.0 (MIDI Association) | <https://midi.org/midi-2-0> |
| 45 | MIDI 2.0 field guide | <https://midi.community/docs/midi-2/> |
| 46 | State of MIDI 2.0 / profiles | <https://midi.org/the-state-of-midi-2-0-high-resolution-performance-and-the-rise-of-profiles-update-feb-2026> |
| 47 | MPE (MIDI Association) | <https://midi.org/mpe-midi-polyphonic-expression> |
| 48 | MPE field guide | <https://midi.community/docs/mpe/> |
| 49 | MPE how it works | <https://polyphonicexpression.com/mpe-how-it-works> |
| 50 | Ableton manual, editing MPE | <https://www.ableton.com/en/manual/editing-mpe/> |
| 51 | MusicGen docs (AudioCraft) | <https://github.com/facebookresearch/audiocraft/blob/main/docs/MUSICGEN.md> |
| 52 | Ctrl-P (Edinburgh research portal) | <https://www.research.ed.ac.uk/en/publications/ctrl-p-temporal-control-of-prosodic-variation-for-speech-synthesi/> |
