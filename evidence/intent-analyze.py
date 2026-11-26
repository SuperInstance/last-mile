#!/usr/bin/env python3
"""The analysis of the intent probe: what the model actually tracked.

Reads evidence/intent.csv and writes evidence/intent.md with:

  * the confusion tables;
  * the analysis that matters: the clips whose *transcripts are identical*
    -- there the words are held fixed and only the delivery differs, so a
    difference in the answer came from the sound and not from the words;
  * the repeatability check (the same clip, asked again, greedy): if the
    answers move on their own, nothing else here means anything.
"""
from __future__ import annotations

import csv
import json
import os
import statistics
import urllib.request

EVIDENCE = os.path.dirname(os.path.abspath(__file__))
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


def ask(data: bytes, question: str) -> str:
    import base64

    body = {
        "model": "LFM3-Audio-2B",
        "messages": [
            {"role": "system", "content": SYSTEM},
            {
                "role": "user",
                "content": [
                    {
                        "type": "input_audio",
                        "input_audio": {
                            "data": base64.b64encode(data).decode(),
                            "format": "wav",
                        },
                    },
                    {"type": "text", "text": question},
                ],
            },
        ],
        "temperature": 0,
        "max_tokens": 32,
    }
    request = urllib.request.Request(
        "http://127.0.0.1:8102/v1/chat/completions",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
    )
    with OPENER.open(request, timeout=120) as response:
        answer = json.loads(response.read())
    return answer["choices"][0]["message"]["content"].strip()


def ask_words(words: str, question: str) -> str:
    """The same question with the words and no sound.

    The words are given as a transcript of a recording, so the only thing
    missing is the sound.
    """
    body = {
        "model": "LFM3-Audio-2B",
        "messages": [
            {"role": "system", "content": SYSTEM},
            {
                "role": "user",
                "content": [
                    {
                        "type": "text",
                        "text": f'The recording says: "{words}".\n{question}',
                    }
                ],
            },
        ],
        "temperature": 0,
        "max_tokens": 32,
    }
    request = urllib.request.Request(
        "http://127.0.0.1:8102/v1/chat/completions",
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
    )
    with OPENER.open(request, timeout=120) as response:
        answer = json.loads(response.read())
    return answer["choices"][0]["message"]["content"].strip()


def paired_control(rows: list[dict]) -> list[dict]:
    """Every clip, asked with the sound and asked with only the words.

    Two word controls per clip: the words that were *said* (the
    reference) and the words that were *heard* (the transcript). The
    first says what the sound added over the truth, the second says what
    it added over what the model itself heard.
    """
    out = []
    for row in rows:
        reference_words = ask_words(row["text"], Q_URGENCY)
        heard_words = ask_words(row["transcript"], Q_URGENCY)
        reference_order = ask_words(row["text"], Q_ORDER)
        heard_order = ask_words(row["transcript"], Q_ORDER)
        out.append(
            {
                "wav": row["wav"],
                "text": row["text"],
                "heard": row["transcript"],
                "style": row["style"],
                "voice": row["voice_alias"],
                "audio_urgency": row["urgency_answer"],
                "words_urgency": classify(reference_words),
                "heard_words_urgency": classify(heard_words),
                "audio_order": row["order_answer"],
                "words_order": classify(reference_order, ["order", "question"]),
                "heard_words_order": classify(heard_order, ["order", "question"]),
            }
        )
        print(
            f"paired {row['wav'].split('/')[-1]:38s} "
            f"audio {row['urgency_answer']:8s} words {classify(reference_words):8s}"
        )
    return out


def classify(text: str, choices: tuple[str, ...] | list[str] | None = None) -> str:
    """The class the answer names, or 'other'."""
    if choices is None:
        choices = ["calm", "ordinary", "urgent"]
    lowered = text.lower()
    for choice in choices:
        if choice in lowered:
            return choice
    return "other"


def repeatability(rows: list[dict], repeats: int = 3) -> list[dict]:
    """Ask again about a sample of clips; the answers must not move."""
    sample = [row for row in rows if row["grid"] == "1-words-fixed"][:6]
    out = []
    for row in sample:
        path = os.path.join(EVIDENCE, row["wav"])
        with open(path, "rb") as handle:
            data = handle.read()
        urgency = [ask(data, Q_URGENCY) for _ in range(repeats)]
        order = [ask(data, Q_ORDER) for _ in range(repeats)]
        out.append(
            {
                "wav": row["wav"],
                "text": row["text"],
                "style": row["style"],
                "voice": row["voice_alias"],
                "urgency": urgency,
                "order": order,
                "urgency_stable": len(set(urgency)) == 1,
                "order_stable": len(set(order)) == 1,
            }
        )
        print(
            f"repeat {row['wav']}: urgency {urgency} order {order}"
        )
    return out


def main() -> None:
    with open(os.path.join(EVIDENCE, "intent.csv")) as handle:
        rows = list(csv.DictReader(handle))
    for row in rows:
        row["seconds"] = float(row["seconds"])
        row["rms"] = float(row["rms"])

    grid1 = [row for row in rows if row["grid"] == "1-words-fixed"]
    grid23 = [row for row in rows if row["grid"] != "1-words-fixed"]
    with open(os.path.join(EVIDENCE, "intent-control.json")) as handle:
        control = json.load(handle)

    repeats = repeatability(rows)
    paired = paired_control(rows)
    with open(os.path.join(EVIDENCE, "intent-paired.json"), "w") as handle:
        json.dump(paired, handle, indent=1)

    # --- the words fixed, the delivery varying: the answers
    distinct = {}
    for text in sorted({row["text"] for row in grid1}):
        answers = [row["urgency_answer"] for row in grid1 if row["text"] == text]
        distinct[text] = sorted(set(answers))

    # --- the clips whose transcripts are identical: the words are held
    #     fixed *as heard*, so only the delivery can move the answer
    groups: dict[tuple[str, str], list[dict]] = {}
    for row in grid1:
        key = (row["text"], row["transcript"].strip().lower())
        groups.setdefault(key, []).append(row)
    held = {key: rows_ for key, rows_ in groups.items() if len(rows_) > 1}

    # --- the order/question question, per grid
    def oq_accuracy(subset: list[dict]) -> tuple[int, int]:
        ok = sum(
            1
            for row in subset
            if row["order_answer"] == row["expected_order_question"]
        )
        return ok, len(subset)

    # --- the model's prior on urgency
    all_urgent = sum(1 for row in rows if row["urgency_answer"] == "urgent")
    grid1_urgent = sum(1 for row in grid1 if row["urgency_answer"] == "urgent")

    lines = [
        "# The intent probe: does the ears hear the delivery?",
        "",
        "Every clip is rendered by skald from a word, a voice and a Riva",
        "style. Nothing was recorded from a person.",
        "",
        f"Clips: {len(grid1)} in grid 1 (words fixed), {len(grid23)} in grids 2-3.",
        "",
        "## What the model answered",
        "",
        "| grid | urgency: calm | ordinary | urgent | other | order | question | other |",
        "|---|---|---|---|---|---|---|---|",
    ]

    def table(subset: list[dict]) -> str:
        urgency = {"calm": 0, "ordinary": 0, "urgent": 0, "other": 0}
        order = {"order": 0, "question": 0, "other": 0}
        for row in subset:
            urgency[row["urgency_answer"]] += 1
            order[row["order_answer"]] += 1
        return (
            f"| {len(subset)} | {urgency['calm']} | {urgency['ordinary']} | "
            f"{urgency['urgent']} | {urgency['other']} | {order['order']} | "
            f"{order['question']} | {order['other']} |"
        )

    lines.append(f"| 1 words fixed | " + table(grid1).split("| ", 1)[1])
    for grid in ("2-text-variants", "3-order-question"):
        subset = [row for row in rows if row["grid"] == grid]
        lines.append(f"| {grid} | " + table(subset).split("| ", 1)[1])
    lines.append("")

    # --- grid 1 in full
    lines += [
        "## Grid 1: the words fixed, the delivery varying",
        "",
        "One order word, several styles, three voices. The last column is",
        "what the ears heard: where the transcripts differ, the *words*",
        "differ too, and a difference in the answer cannot be attributed",
        "to the delivery alone.",
        "",
    ]
    for text in sorted({row["text"] for row in grid1}):
        subset = [row for row in grid1 if row["text"] == text]
        lines += [
            f"### {text!r}",
            "",
            "| style | mia | jason | aria | transcripts heard |",
            "|---|---|---|---|---|",
        ]
        by_style: dict[str, dict[str, str]] = {}
        transcripts: dict[str, list[str]] = {}
        for row in subset:
            by_style.setdefault(row["style"], {})[row["voice_alias"]] = row[
                "urgency_answer"
            ]
            transcripts.setdefault(row["style"], []).append(row["transcript"])
        styles = ["Calm", "Neutral", "Default", "Angry", "Happy", "Sad"]
        for style in [s for s in styles if s in by_style]:
            answers = by_style[style]
            heard = sorted(set(transcripts[style]))
            lines.append(
                f"| {style} | {answers.get('mia', '-')} | {answers.get('jason', '-')} | "
                f"{answers.get('aria', '-')} | {'; '.join(heard)} |"
            )
        lines.append("")

    # --- the held-fixed analysis
    lines += [
        "## The words held fixed *as heard*",
        "",
        "The clips whose transcript is the same. Here the model heard the",
        "same words in every row, so the only thing left is the delivery.",
        "",
        "| words heard | clips | urgencies | orders |",
        "|---|---|---|---|",
    ]
    for (text, transcript), subset in sorted(held.items()):
        urgencies = [row["urgency_answer"] for row in subset]
        orders = [row["order_answer"] for row in subset]
        lines.append(
            f"| {transcript!r} | {len(subset)} | {', '.join(urgencies)} | "
            f"{', '.join(orders)} |"
        )
    lines.append("")

    moved = [
        (key, subset)
        for key, subset in held.items()
        if len({row["urgency_answer"] for row in subset}) > 1
    ]
    moved_answers = sum(
        1 for _key, subset in moved for row in subset
    )
    lines += [
        f"Of the {len(held)} transcript groups, {len(moved)} hold the words",
        f"fixed and still get more than one urgency answer between them "
        f"({moved_answers} clips).",
        "",
    ]

    # --- the confusion tables
    lines += [
        "### Confusion table: expected urgency against the answer (grid 1)",
        "",
        "| expected \\\\ heard | calm | ordinary | urgent | other |",
        "|---|---|---|---|---|",
    ]
    for expected in ("calm", "ordinary", "urgent"):
        subset = [row for row in grid1 if row["expected_urgency"] == expected]
        counts = {"calm": 0, "ordinary": 0, "urgent": 0, "other": 0}
        for row in subset:
            counts[row["urgency_answer"]] += 1
        lines.append(
            f"| {expected} | {counts['calm']} | {counts['ordinary']} | "
            f"{counts['urgent']} | {counts['other']} |"
        )
    lines.append("")

    lines += [
        "### Confusion table: expected order/question against the answer (all grids)",
        "",
        "| expected \\\\ heard | order | question | other |",
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

    # --- the text-only control
    lines += [
        "## The text-only control",
        "",
        "The same two questions with the words and no sound.",
        "",
        "| text | urgency (no audio) | order/question (no audio) |",
        "|---|---|---|",
    ]
    for text, answers in control.items():
        lines.append(f"| {text} | {answers['urgency']} | {answers['order']} |")
    lines.append("")

    # --- the paired control: the same clip, with and without the sound
    urgency_moved = sum(
        1 for entry in paired if entry["audio_urgency"] != entry["words_urgency"]
    )
    heard_moved = sum(
        1 for entry in paired if entry["audio_urgency"] != entry["heard_words_urgency"]
    )
    order_moved = sum(
        1 for entry in paired if entry["audio_order"] != entry["words_order"]
    )
    heard_order_moved = sum(
        1 for entry in paired if entry["audio_order"] != entry["heard_words_order"]
    )
    lines += [
        "## The paired control: the same clip, with and without the sound",
        "",
        "Every clip asked twice: once with the audio, once with the words",
        "it carries and no sound at all. `words` is what was said, `heard`",
        "is what the ears heard. A cell that differs is a case where the",
        "sound changed the answer.",
        "",
        "| | urgency | order/question |",
        "|---|---|---|",
        f"| the sound changed the answer vs the words said | {urgency_moved} of {len(paired)} | {order_moved} of {len(paired)} |",
        f"| the sound changed the answer vs the words heard | {heard_moved} of {len(paired)} | {heard_order_moved} of {len(paired)} |",
        "",
        "| clip | words said | heard | urgency: sound | urgency: words | urgency: heard words | order: sound | order: words |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for entry in paired:
        lines.append(
            f"| {entry['wav'].split('/')[-1]} | {entry['text']} | {entry['heard']} | "
            f"{entry['audio_urgency']} | {entry['words_urgency']} | "
            f"{entry['heard_words_urgency']} | {entry['audio_order']} | "
            f"{entry['words_order']} |"
        )
    lines.append("")

    # --- repeatability
    lines += [
        "## Repeatability",
        "",
        "The same clip, asked again, greedy (temperature 0).",
        "",
        "| clip | urgency answers | order answers | stable |",
        "|---|---|---|---|",
    ]
    for entry in repeats:
        lines.append(
            f"| {entry['wav']} | {', '.join(entry['urgency'])} | "
            f"{', '.join(entry['order'])} | "
            f"{'yes' if entry['urgency_stable'] and entry['order_stable'] else 'NO'} |"
        )
    stable = sum(
        1 for entry in repeats if entry["urgency_stable"] and entry["order_stable"]
    )
    lines += ["", f"Stable on {stable} of {len(repeats)} clips.", ""]

    # --- the numbers that decide it
    ok_all, n_all = oq_accuracy(rows)
    ok_q, n_q = oq_accuracy([row for row in rows if row["expected_order_question"] == "question"])
    ok_o, n_o = oq_accuracy([row for row in rows if row["expected_order_question"] == "order"])
    lines += [
        "## The numbers that decide it",
        "",
        f"- Distinct urgency answers for the *same words* (grid 1): "
        f"{', '.join(f'{text}: {len(answers)} ({', '.join(answers)})' for text, answers in sorted(distinct.items()))}. "
        "One answer means the delivery was ignored.",
        f"- The model's prior: {grid1_urgent} of {len(grid1)} grid-1 clips were "
        f"called urgent, and {all_urgent} of {len(rows)} clips overall.",
        f"- Order/question: {ok_all} of {n_all} clips right overall "
        f"({ok_o}/{n_o} on the orders, {ok_q}/{n_q} on the questions).",
        f"- The sound changed the urgency answer on {urgency_moved} of {len(paired)} "
        f"clips (vs the words said) and on {heard_moved} of {len(paired)} (vs the "
        "words heard).",
        f"- The sound changed the order/question answer on {order_moved} of "
        f"{len(paired)} clips (vs the words said) and on {heard_order_moved} of "
        f"{len(paired)} (vs the words heard).",
        f"- Repeatability: {stable}/{len(repeats)} clips gave the same answer twice.",
        "",
        "## What this means for the squad",
        "",
        "The order/question answer is the usable one: the words alone (the",
        "text-only control) never carry it, and the audio carries it for",
        "the questions it is asked about.",
        "",
        "The urgency answer is not yet usable as a score input: the model",
        "calls almost everything urgent, so the answer is mostly the",
        "model's prior and not the delivery.",
        "",
        "And the delivery axis is not word-preserving: the Riva styles",
        "change what the words *are* as heard, so a delivery signal and a",
        "mishearing are tangled until the TTS is constrained.",
        "",
    ]

    with open(os.path.join(EVIDENCE, "intent.md"), "w") as handle:
        handle.write("\n".join(lines))
    print("\n".join(lines))


if __name__ == "__main__":
    main()
