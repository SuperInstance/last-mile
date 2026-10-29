#!/usr/bin/env python3
"""Transcribe the corpus on :8102 and score it per voice.

Every clip of evidence/corpus.csv goes to the ears; the answer is scored
against the phrasebook line that was rendered, with the standard word
error rate: the edit distance over the word sequences (substitutions,
deletions, insertions) divided by the number of reference words. Word
accuracy is 1 - WER.

The words are normalized the way the bark cache normalizes a line
(crates/lm-voice/src/bark.rs): lower-cased, the punctuation stripped, the
whitespace collapsed. So "Hold the line." and "hold the line" are one
line and one score.

Writes: evidence/transcripts.csv, evidence/accuracy.md
"""
from __future__ import annotations

import csv
import json
import os
import re
import statistics
import urllib.request

EVIDENCE = os.path.dirname(os.path.abspath(__file__))
EARS = "http://127.0.0.1:8102"
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))

_WORD_RE = re.compile(r"[A-Za-z0-9']+")

# The model spells numbers out ("beacon 2" -> "beacon two"), the phrasebook
# writes them as digits. That is a format difference, not a hearing
# difference, so the scores are given twice: strict, and with the digits
# of both sides spelled out.
_NUMBER_WORDS = {
    "0": "zero",
    "1": "one",
    "2": "two",
    "3": "three",
    "4": "four",
    "5": "five",
    "6": "six",
    "7": "seven",
    "8": "eight",
    "9": "nine",
}


def normalize(text: str) -> list[str]:
    """The words of a line, normalized."""
    return [match.group(0).lower() for match in _WORD_RE.finditer(text)]


def normalize_numbers(words: list[str]) -> list[str]:
    """The same words, with the digits spelled out."""
    return [_NUMBER_WORDS.get(word, word) for word in words]


def edit_counts(reference: list[str], hypothesis: list[str]) -> tuple[int, int, int]:
    """(substitutions, deletions, insertions) over word sequences."""
    rows, cols = len(reference) + 1, len(hypothesis) + 1
    # cost table: (distance, substitutions, deletions, insertions)
    table = [[(0, 0, 0, 0)] * cols for _ in range(rows)]
    for i in range(1, rows):
        table[i][0] = (i, 0, i, 0)
    for j in range(1, cols):
        table[0][j] = (j, 0, 0, j)
    for i in range(1, rows):
        for j in range(1, cols):
            if reference[i - 1] == hypothesis[j - 1]:
                table[i][j] = table[i - 1][j - 1]
                continue
            sub = table[i - 1][j - 1]
            delete = table[i - 1][j]
            insert = table[i][j - 1]
            best = min((sub[0] + 1, sub[1] + 1, sub[2], sub[3]),
                       (delete[0] + 1, delete[1], delete[2] + 1, delete[3]),
                       (insert[0] + 1, insert[1], insert[2], insert[3] + 1))
            table[i][j] = best
    return table[rows - 1][cols - 1][1:]


def transcribe(path: str) -> tuple[str, float]:
    """The ears' answer for one clip."""
    with open(path, "rb") as handle:
        data = handle.read()
    request = urllib.request.Request(
        f"{EARS}/transcribe?language=en",
        data=data,
        headers={"Content-Type": "audio/wav"},
    )
    started = __import__("time").perf_counter()
    with OPENER.open(request, timeout=120) as response:
        answer = json.loads(response.read())
    latency = (__import__("time").perf_counter() - started) * 1000.0
    return answer.get("text", ""), latency


def main() -> None:
    with open(os.path.join(EVIDENCE, "corpus.csv")) as handle:
        rows = list(csv.DictReader(handle))

    results = []
    for row in rows:
        text, latency = transcribe(os.path.join(EVIDENCE, row["wav"]))
        reference_words = normalize(row["reference"])
        hypothesis_words = normalize(text)
        subs, dels, ins = edit_counts(reference_words, hypothesis_words)
        errors = subs + dels + ins
        wer = errors / len(reference_words) if reference_words else 0.0
        # the same score with the digits spelled out on both sides
        n_subs, n_dels, n_ins = edit_counts(
            normalize_numbers(reference_words), normalize_numbers(hypothesis_words)
        )
        n_errors = n_subs + n_dels + n_ins
        n_wer = n_errors / len(reference_words) if reference_words else 0.0
        results.append(
            {
                **row,
                "hypothesis": text,
                "reference_words": len(reference_words),
                "substitutions": subs,
                "deletions": dels,
                "insertions": ins,
                "errors": errors,
                "wer": round(wer, 4),
                "wer_numbers_spelled": round(n_wer, 4),
                "exact": reference_words == hypothesis_words,
                "exact_numbers_spelled": normalize_numbers(reference_words)
                == normalize_numbers(hypothesis_words),
                "latency_ms": round(latency, 1),
            }
        )
        print(f"{row['voice']:6s} {row['reference'][:32]:34s} -> {text[:40]:42s} wer {wer:.2f}")

    with open(os.path.join(EVIDENCE, "transcripts.csv"), "w", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=list(results[0].keys()))
        writer.writeheader()
        writer.writerows(results)

    # the table
    lines = [
        "# Word accuracy of the ears, per voice",
        "",
        "20 phrasebook lines per voice, rendered by skald, transcribed by",
        "LFM3-Audio-2B on :8102. WER is the word error rate; word accuracy",
        "is 1 - WER.",
        "",
        "Two columns of scores: **strict** compares the words as written",
        "(the phrasebook writes \"beacon 2\", the model hears \"beacon two\"),",
        "and **numbers spelled** puts both sides in the same spelling, so a",
        "digit that came out as a word is not counted as a hearing error.",
        "",
        "| voice | clips | ref words | S | D | I | WER | word accuracy | WER (numbers spelled) | accuracy (numbers spelled) | exact lines | median latency ms |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for voice in sorted({row["voice"] for row in results}):
        subset = [row for row in results if row["voice"] == voice]
        subs = sum(row["substitutions"] for row in subset)
        dels = sum(row["deletions"] for row in subset)
        ins = sum(row["insertions"] for row in subset)
        ref_words = sum(row["reference_words"] for row in subset)
        errors = subs + dels + ins
        wer = errors / ref_words if ref_words else 0.0
        n_errors = sum(
            round(row["wer_numbers_spelled"] * row["reference_words"]) for row in subset
        )
        n_wer = n_errors / ref_words if ref_words else 0.0
        exact = sum(1 for row in subset if row["exact"])
        median_latency = statistics.median(row["latency_ms"] for row in subset)
        lines.append(
            f"| {voice} | {len(subset)} | {ref_words} | {subs} | {dels} | {ins} | "
            f"{wer:.3f} | {1 - wer:.3f} | {n_wer:.3f} | {1 - n_wer:.3f} | "
            f"{exact}/{len(subset)} | {median_latency:.0f} |"
        )

    total_ref = sum(row["reference_words"] for row in results)
    total_err = sum(row["errors"] for row in results)
    total_exact = sum(1 for row in results if row["exact"])
    total_n_err = sum(
        round(row["wer_numbers_spelled"] * row["reference_words"]) for row in results
    )
    lines.append(
        f"| **all** | {len(results)} | {total_ref} | "
        f"{sum(row['substitutions'] for row in results)} | "
        f"{sum(row['deletions'] for row in results)} | "
        f"{sum(row['insertions'] for row in results)} | "
        f"{total_err / total_ref:.3f} | {1 - total_err / total_ref:.3f} | "
        f"{total_n_err / total_ref:.3f} | {1 - total_n_err / total_ref:.3f} | "
        f"{total_exact}/{len(results)} | "
        f"{statistics.median(row['latency_ms'] for row in results):.0f} |"
    )

    lines += ["", "## The lines that were not exact", "", "| voice | reference | heard | WER |", "|---|---|---|---|"]
    for row in results:
        if not row["exact"]:
            lines.append(
                f"| {row['voice']} | {row['reference']} | {row['hypothesis']} | {row['wer']:.2f} |"
            )
    lines.append("")

    with open(os.path.join(EVIDENCE, "accuracy.md"), "w") as handle:
        handle.write("\n".join(lines))
    print()
    print("\n".join(lines))


if __name__ == "__main__":
    main()
