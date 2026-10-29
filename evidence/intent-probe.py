#!/usr/bin/env python3
"""The intent probe: does the ears hear the delivery, or only the words?

The question last-mile actually wants answered: can the player's voice
become a score input whose *meaning* (how urgent, order or question) the
squad learns over time? For that, the model has to read something the
words do not carry -- the delivery.

Three grids, all rendered through skald (nothing is recorded from a
person):

  1. delivery, words fixed.  The same order word ("hold", "fall back")
     in three voices and six Riva styles. The words are identical inside
     a cell; only the delivery differs. This is the grid that can prove
     the answer tracks delivery: the transcript is the same, so any
     difference in the answer came from the sound.

  2. the text variants the brief names ("hold", "hold!", "hold... hold",
     "fall back", "fall back now!"), in three voices. Here the words
     differ too, so the answer is reported, not attributed.

  3. the order/question control: question wordings, so the second
     question has a positive class and not only orders.

Two closed questions per clip (urgency, order-or-question), plus the
transcript of each clip and its acoustic shape (duration, RMS energy,
peak) so the deliveries can be shown to be different sounds and not the
same sound twice.

A text-only control asks the same two questions with no audio at all: if
the answers follow the audio and not the text, the text-only answer is
one value per word and the audio answer is not.

Writes: evidence/intent/*.wav, evidence/intent.csv, evidence/intent.md
"""
from __future__ import annotations

import array
import base64
import csv
import io
import json
import math
import os
import statistics
import time
import urllib.request
import wave

EVIDENCE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(EVIDENCE, "intent")
SKALD_BASE = "http://localhost:8000"
SKALD = f"{SKALD_BASE}/v1/audio/speech"
EARS = "http://127.0.0.1:8102"
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))

SYSTEM = "Answer questions about the provided audio clip. Base every answer only on what can be heard."
Q_URGENCY = (
    "How urgent does the speaker sound? Answer with exactly one word: "
    "calm, ordinary, or urgent."
)
Q_ORDER = (
    "Is the speaker giving an order or asking a question? "
    "Answer with exactly one word: order or question."
)

SPEAKERS = {"mia": "Mia", "jason": "Jason", "aria": "Aria"}
STYLES = ["Default", "Neutral", "Calm", "Angry", "Happy", "Sad"]

# My reading of what each delivery is meant to be. This is an annotation,
# not a measurement: the measurement is what the model answered.
EXPECTED_URGENCY = {
    "Calm": "calm",
    "Neutral": "calm",
    "Default": "ordinary",
    "Angry": "urgent",
    "Happy": "ordinary",
    "Sad": "ordinary",
}

# The urgent-ness the word itself carries, for grid 1: "hold" and "fall
# back" are ordinary words. Any answer other than "ordinary" in the
# fixed-words grid is therefore a reading of the delivery.
EXPECTED_BY_TEXT = {
    "hold": "ordinary",
    "fall back": "ordinary",
    "hold!": "urgent",
    "hold... hold": "urgent",
    "fall back!": "urgent",
    "fall back now!": "urgent",
    "hold?": "ordinary",
    "should we hold?": "ordinary",
    "are you with me?": "ordinary",
}

EXPECTED_ORDER_QUESTION = {
    "hold": "order",
    "fall back": "order",
    "hold!": "order",
    "hold... hold": "order",
    "fall back!": "order",
    "fall back now!": "order",
    "hold?": "question",
    "should we hold?": "question",
    "are you with me?": "question",
}


def post_json(url: str, body: dict, timeout: int = 120) -> dict:
    request = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
    )
    with OPENER.open(request, timeout=timeout) as response:
        return json.loads(response.read())


def voice_id_for(speaker: str, style: str) -> str:
    """The Riva voice id of a speaker and a style.

    "Default" is the voice without a style suffix: Riva knows
    "Magpie-Multilingual.EN-US.Mia", and "Magpie-Multilingual.EN-US.Mia.Default"
    is not a voice (skald reports the suffix-less id as style "Default").
    """
    base = f"Magpie-Multilingual.EN-US.{speaker}"
    return base if style == "Default" else f"{base}.{style}"


def render(text: str, voice_id: str) -> bytes:
    body = json.dumps({"input": text, "voice": voice_id}).encode()
    request = urllib.request.Request(
        SKALD, data=body, headers={"Content-Type": "application/json"}
    )
    with OPENER.open(request, timeout=60) as response:
        return response.read()


def transcribe(data: bytes) -> str:
    request = urllib.request.Request(
        f"{EARS}/transcribe?language=en",
        data=data,
        headers={"Content-Type": "audio/wav"},
    )
    with OPENER.open(request, timeout=120) as response:
        return json.loads(response.read()).get("text", "")


def ask(data: bytes, question: str, text_only: str | None = None) -> str:
    """One closed question about the clip."""
    if text_only is None:
        content = [
            {
                "type": "input_audio",
                "input_audio": {
                    "data": base64.b64encode(data).decode(),
                    "format": "wav",
                },
            },
            {"type": "text", "text": question},
        ]
    else:
        content = [{"type": "text", "text": f"{text_only}\n{question}"}]
    body = {
        "model": "LFM3-Audio-2B",
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": content},
        ],
        "temperature": 0,
        "max_tokens": 32,
    }
    answer = post_json(f"{EARS}/v1/chat/completions", body)
    return answer["choices"][0]["message"]["content"].strip()


def acoustics(data: bytes) -> dict:
    """The shape of the sound: how long, how loud, how peaked."""
    with wave.open(io.BytesIO(data)) as handle:
        rate = handle.getframerate()
        frames = handle.getnframes()
        samples = array.array("h")
        samples.frombytes(handle.readframes(frames))
    if not samples:
        return {"seconds": 0.0, "rms": 0.0, "peak": 0.0}
    total = 0
    for value in samples:
        total += value * value
    rms = math.sqrt(total / len(samples))
    peak = max(abs(value) for value in samples)
    return {
        "seconds": round(frames / rate, 3),
        "rms": round(rms, 1),
        "peak": peak,
    }


def classify(text: str, choices: list[str]) -> str:
    """The class the answer names, or 'other'."""
    lowered = text.lower()
    for choice in choices:
        if choice in lowered:
            return choice
    return "other"


def available_styles(speaker: str) -> list[str]:
    """The styles Riva actually has for a speaker, EN-US.

    Not every speaker has every style (Jason has no Sad), so the grid is
    built from what the voice registry lists and not from what it should
    have.
    """
    request = urllib.request.Request(f"{SKALD_BASE}/v1/voices")
    with OPENER.open(request, timeout=30) as response:
        voices = json.loads(response.read())["voices"]
    styles = set()
    for voice in voices:
        if voice["speaker"] != speaker or voice["locale"] != "EN-US":
            continue
        styles.add(voice["style"])
    return [style for style in STYLES if style in styles]


def build_clips() -> list[dict]:
    """The clips of the three grids."""
    clips: list[dict] = []

    # grid 1: the words fixed, the delivery varying
    for text in ("hold", "fall back"):
        for alias, speaker in SPEAKERS.items():
            for style in available_styles(speaker):
                voice_id = voice_id_for(speaker, style)
                clips.append(
                    {
                        "grid": "1-words-fixed",
                        "text": text,
                        "voice_alias": alias,
                        "style": style,
                        "voice_id": voice_id,
                        "expected_urgency": EXPECTED_URGENCY[style],
                        "expected_order_question": "order",
                    }
                )

    # grid 2: the text variants the brief names
    for text in ("hold", "hold!", "hold... hold", "fall back", "fall back now!"):
        for alias, speaker in SPEAKERS.items():
            clips.append(
                {
                    "grid": "2-text-variants",
                    "text": text,
                    "voice_alias": alias,
                    "style": "Default",
                    "voice_id": voice_id_for(speaker, "Default"),
                    "expected_urgency": EXPECTED_BY_TEXT[text],
                    "expected_order_question": EXPECTED_ORDER_QUESTION[text],
                }
            )

    # grid 3: the order/question control
    for text in ("hold?", "should we hold?", "are you with me?"):
        for alias, speaker in SPEAKERS.items():
            clips.append(
                {
                    "grid": "3-order-question",
                    "text": text,
                    "voice_alias": alias,
                    "style": "Default",
                    "voice_id": voice_id_for(speaker, "Default"),
                    "expected_urgency": EXPECTED_BY_TEXT[text],
                    "expected_order_question": EXPECTED_ORDER_QUESTION[text],
                }
            )
    return clips


def main() -> None:
    os.makedirs(OUT, exist_ok=True)
    clips = build_clips()
    rows = []

    for index, clip in enumerate(clips):
        data = render(clip["text"], clip["voice_id"])
        name = (
            f"{index:03d}_g{clip['grid'][0]}_{clip['text'].replace(' ', '_')}"
            f"_{clip['voice_alias']}_{clip['style']}.wav"
        )
        with open(os.path.join(OUT, name), "wb") as handle:
            handle.write(data)

        transcript = transcribe(data)
        urgency_raw = ask(data, Q_URGENCY)
        order_raw = ask(data, Q_ORDER)
        shape = acoustics(data)

        rows.append(
            {
                **clip,
                "wav": os.path.join("intent", name),
                "transcript": transcript,
                "urgency_answer_raw": urgency_raw,
                "urgency_answer": classify(urgency_raw, ["calm", "ordinary", "urgent"]),
                "order_answer_raw": order_raw,
                "order_answer": classify(order_raw, ["order", "question"]),
                **shape,
            }
        )
        print(
            f"{clip['grid']:16s} {clip['text'][:18]:20s} {clip['voice_alias']:6s} "
            f"{clip['style']:8s} -> {rows[-1]['urgency_answer']:9s} "
            f"{rows[-1]['order_answer']:9s} | {transcript[:26]}"
        )

    # the text-only control: the same questions with no audio at all
    control = {}
    for text in ("hold", "fall back", "hold?", "should we hold?"):
        control[text] = {
            "urgency": ask(b"", Q_URGENCY, text_only=text),
            "order": ask(b"", Q_ORDER, text_only=text),
        }
        print(f"text-only control {text!r}: {control[text]}")

    with open(os.path.join(EVIDENCE, "intent.csv"), "w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)
    with open(os.path.join(EVIDENCE, "intent-control.json"), "w") as handle:
        json.dump(control, handle, indent=1)

    report(rows, control)
    print("\nwritten: evidence/intent.csv, evidence/intent.md")


def report(rows: list[dict], control: dict) -> None:
    """The confusion tables and the verdict."""
    lines = [
        "# The intent probe: does the ears hear the delivery?",
        "",
        "Every clip is rendered by skald from a word, a voice and a Riva",
        "style. Nothing was recorded from a person.",
        "",
        "## Grid 1: the words fixed, the delivery varying",
        "",
        "One order word, six styles, three voices. The transcript column",
        "shows the words the ears heard: where the transcripts agree, a",
        "difference in the answer can only have come from the sound.",
        "",
    ]

    grid1 = [row for row in rows if row["grid"] == "1-words-fixed"]
    for text in sorted({row["text"] for row in grid1}):
        subset = [row for row in grid1 if row["text"] == text]
        lines += [
            f"### {text!r}",
            "",
            "| style | expected urgency | mia | jason | aria | transcripts heard |",
            "|---|---|---|---|---|---|",
        ]
        by_style: dict[str, dict[str, str]] = {}
        transcripts: dict[str, list[str]] = {}
        for row in subset:
            by_style.setdefault(row["style"], {})[row["voice_alias"]] = row["urgency_answer"]
            transcripts.setdefault(row["style"], []).append(row["transcript"])
        present = [style for style in STYLES if style in by_style]
        for style in present:
            answers = by_style.get(style, {})
            heard = sorted(set(transcripts.get(style, [])))
            lines.append(
                f"| {style} | {EXPECTED_URGENCY[style]} | "
                f"{answers.get('mia', '-')} | {answers.get('jason', '-')} | "
                f"{answers.get('aria', '-')} | {'; '.join(heard)} |"
            )
        lines.append("")

    # the confusion table, expected against heard
    lines += [
        "### Confusion table: expected urgency against the answer",
        "",
        "| expected \\ heard | calm | ordinary | urgent | other |",
        "|---|---|---|---|---|",
    ]
    classes = ["calm", "ordinary", "urgent", "other"]
    for expected in classes:
        subset = [row for row in grid1 if row["expected_urgency"] == expected]
        if not subset:
            continue
        counts = {name: 0 for name in classes}
        for row in subset:
            counts[row["urgency_answer"]] += 1
        lines.append(
            f"| {expected} | {counts['calm']} | {counts['ordinary']} | "
            f"{counts['urgent']} | {counts['other']} |"
        )
    lines.append("")

    # the same table for the order/question question, all grids
    lines += [
        "### Confusion table: expected order/question against the answer",
        "",
        "| expected \\ heard | order | question | other |",
        "|---|---|---|---|",
    ]
    for expected in ("order", "question"):
        subset = [row for row in rows if row["expected_order_question"] == expected]
        counts = {"order": 0, "question": 0, "other": 0}
        for row in subset:
            counts[row["order_answer"]] += 1
        lines.append(
            f"| {expected} | {counts['order']} | {counts['question']} | {counts['other']} |"
        )
    lines.append("")

    # grid 2 and 3, in full
    lines += [
        "## Grid 2 and 3: the text variants and the order/question control",
        "",
        "| grid | text | voice | heard | urgency | expected | order/question | expected |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for row in rows:
        if row["grid"] == "1-words-fixed":
            continue
        lines.append(
            f"| {row['grid']} | {row['text']} | {row['voice_alias']} | "
            f"{row['transcript']} | {row['urgency_answer']} | {row['expected_urgency']} | "
            f"{row['order_answer']} | {row['expected_order_question']} |"
        )
    lines.append("")

    # the text-only control
    lines += [
        "## The text-only control",
        "",
        "The same two questions with the words and no sound. If the answer",
        "follows the audio and not the words, this is one value per word",
        "and the audio answer is not.",
        "",
        "| text | urgency (no audio) | order/question (no audio) |",
        "|---|---|---|",
    ]
    for text, answers in control.items():
        lines.append(
            f"| {text} | {answers['urgency']} | {answers['order']} |"
        )
    lines.append("")

    # the acoustics: the deliveries really are different sounds
    lines += [
        "## The deliveries are different sounds",
        "",
        "| style | mean seconds | mean RMS | mean peak |",
        "|---|---|---|---|",
    ]
    for style in [s for s in STYLES if any(r["style"] == s for r in grid1)]:
        subset = [row for row in grid1 if row["style"] == style]
        if not subset:
            continue
        lines.append(
            f"| {style} | {statistics.mean(row['seconds'] for row in subset):.2f} | "
            f"{statistics.mean(row['rms'] for row in subset):.0f} | "
            f"{statistics.mean(row['peak'] for row in subset):.0f} |"
        )
    lines.append("")

    # the numbers that decide it
    fixed = [row for row in grid1]
    by_text: dict[str, set[str]] = {}
    for row in fixed:
        by_text.setdefault(row["text"], set()).add(row["urgency_answer"])
    tracks = {text: len(answers) for text, answers in by_text.items()}
    order_ok = sum(
        1
        for row in rows
        if row["order_answer"] == row["expected_order_question"]
    )
    lines += [
        "## The numbers that decide it",
        "",
        f"- Distinct urgency answers for the *same words* in grid 1: "
        f"{', '.join(f'{text}: {count}' for text, count in sorted(tracks.items()))} "
        f"(1 means the answer ignored the delivery).",
        f"- Order/question answered as expected on {order_ok} of {len(rows)} clips.",
        "",
    ]
    with open(os.path.join(EVIDENCE, "intent.md"), "w") as handle:
        handle.write("\n".join(lines))


if __name__ == "__main__":
    main()
