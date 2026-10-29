#!/usr/bin/env python3
"""Render the corpus: 20 last-mile squad lines, in three voices.

The lines are the harness's own phrasebook (assets/harness/default/
phrasebook.toml), with the placeholders filled the way the gates would
fill them: {subject} is a beacon or an enemy, {direction} is a compass
word. The lines are the words the squad actually says, so the ears are
measured on the speech the game makes, not on a generic phrase list.

Each line is rendered three times, through skald, with the aliases the
brief names: mia, jason, aria. The WAVs land in evidence/wavs/ and the
manifest in evidence/corpus.csv.

Writes: evidence/wavs/*.wav, evidence/corpus.csv
"""
from __future__ import annotations

import csv
import os
import time
import urllib.request

EVIDENCE = os.path.dirname(os.path.abspath(__file__))
WAVS = os.path.join(EVIDENCE, "wavs")
SKALD = "http://localhost:8000/v1/audio/speech"

# The phrasebook's templates, filled. The comment on each line is the
# phrasebook entry it comes from.
LINES: list[tuple[str, str]] = [
    ("Command.Order", "move up now"),
    ("Command.Order", "hold the line"),
    ("Command.Order", "take it"),
    ("Command.Order.Beacon", "take beacon 2 now"),
    ("Command.Order.Beacon", "move to beacon 2"),
    ("Command.Order.Beacon", "beacon 2 is yours"),
    ("Command.Order.Enemy", "watch enemy 4"),
    ("Command.Order.Direction", "head north now"),
    ("Command.Order.Direction", "go east"),
    ("Command.Ack", "yes"),
    ("Command.Ack", "on it"),
    ("Command.Ack", "understood"),
    ("Command.Warn", "stop"),
    ("Command.Warn", "look out"),
    ("Command.Warn.Beacon", "leave beacon 2"),
    ("Command.Warn.Enemy", "enemy 4 is coming"),
    ("Command.Query.Beacon", "where is beacon 2"),
    ("Command.Report", "reporting"),
    ("Callout.Order", "squad, on me"),
    ("Callout.Warn.Enemy", "they are at enemy 4"),
]

VOICES = ["mia", "jason", "aria"]


def render(text: str, voice: str) -> bytes:
    body = '{"input": %s, "voice": "%s"}' % (json_str(text), voice)
    request = urllib.request.Request(
        SKALD,
        data=body.encode(),
        headers={"Content-Type": "application/json"},
    )
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with opener.open(request, timeout=60) as response:
        return response.read()


def json_str(value: str) -> str:
    import json

    return json.dumps(value)


def wav_seconds(data: bytes) -> float:
    import io
    import wave

    with wave.open(io.BytesIO(data)) as handle:
        return handle.getnframes() / handle.getframerate()


def main() -> None:
    os.makedirs(WAVS, exist_ok=True)
    rows = []
    for index, (entry, text) in enumerate(LINES):
        for voice in VOICES:
            data = render(text, voice)
            name = f"{index:02d}_{entry.replace('.', '-')}_{voice}.wav"
            path = os.path.join(WAVS, name)
            with open(path, "wb") as handle:
                handle.write(data)
            rows.append(
                {
                    "id": index,
                    "phrasebook_entry": entry,
                    "reference": text,
                    "voice": voice,
                    "wav": os.path.join("wavs", name),
                    "seconds": round(wav_seconds(data), 3),
                    "bytes": len(data),
                }
            )
            print(f"{name:52s} {len(data):7d} B  {wav_seconds(data):.2f}s")
            time.sleep(0.05)

    # the load clip the interference measurement hammers with
    with open(os.path.join(WAVS, "_load.wav"), "wb") as handle:
        handle.write(render("hold the line", "mia"))

    with open(os.path.join(EVIDENCE, "corpus.csv"), "w", newline="") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=[
                "id",
                "phrasebook_entry",
                "reference",
                "voice",
                "wav",
                "seconds",
                "bytes",
            ],
        )
        writer.writeheader()
        writer.writerows(rows)
    print(f"\n{len(rows)} clips, manifest in evidence/corpus.csv")


if __name__ == "__main__":
    main()
