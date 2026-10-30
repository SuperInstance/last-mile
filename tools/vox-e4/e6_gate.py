"""E6(c): the marks as a gate input, against the 7-field L2 line.

The d1-2B gate (System One, :8094) is asked the same two questions E4 and
E4b asked, and answers them from four different states:

  l2-v3        the 7-field L2 line and the E4/E4b criteria (the control:
               this is the arm E4b measured at 45.0% / 90.6% / 50.0%)
  l2-v3-read   the same line, with the diagnostic question — E4b's arm,
               re-run here so the two encodings can be compared on how well
               the gate can *read* them
  marks        the marks line and the **same** criteria as l2-v3, so the
               only thing that changes is the encoding
  marks-vocab  the marks line with the order/question criteria rewritten in
               the marks' own vocabulary — the E4b lesson, applied to marks
  marks-read   the same marks, with the diagnostic question

Both channels are asked about the same 340 clips, with at most four requests
in flight.

    python tools/vox-e4/e6_gate.py --marks evidence/vox-e6/marks.csv \
        --workers 4
"""

from __future__ import annotations

import argparse
import csv
import json
import sys
import time
import urllib.request
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "vox-ref"))

from vox_ref.marks import MARKS_LEGEND  # noqa: E402

D1_2B = "http://localhost:8094/v1/systemone"

#: The E4/E4b questions, verbatim. Both the l2-v3 arm and the marks arm use
#: these, so the only difference between them is the encoding.
QUESTIONS = {
    "urgency": {
        "type": "choice",
        "instructions": "How urgent is this voice delivery?",
        "criteria": {
            "calm": "The voice is slow, soft and relaxed, with no pressure.",
            "ordinary": "The voice is at a normal pace and a normal volume.",
            "urgent": "The voice is fast, loud and sharp, demanding immediate action.",
        },
    },
    "order_or_question": {
        "type": "choice",
        "instructions": "Is the speaker giving an order or asking a question?",
        "criteria": {
            "order": "The speaker is telling someone to do something.",
            "question": "The speaker is asking for information.",
        },
    },
}

#: The order/question criteria in the marks' own vocabulary.
MARKS_VOCAB_QUESTIONS = dict(QUESTIONS)
MARKS_VOCAB_QUESTIONS["order_or_question"] = {
    "type": "choice",
    "instructions": "Is the speaker giving an order or asking a question? "
                    "Judge it by how the delivery ends.",
    "criteria": {
        "order": "The speaker is telling someone to do something. The pitch "
                 "ends falling or stays level: the mark is falling or flat.",
        "question": "The speaker is asking for information. The pitch ends "
                    "rising: the mark is questioning or lifting.",
    },
}

#: The diagnostic: read a mark, do not guess an intent.
READ_QUESTIONS = dict(QUESTIONS)
READ_QUESTIONS["pitch_end"] = {
    "type": "choice",
    "instructions": "Judging only by the marks: where is the pitch at the "
                    "end of the delivery, compared with where it started?",
    "criteria": {
        "higher": "The pitch ends higher than it started.",
        "lower": "The pitch ends lower than it started.",
    },
}

DELIVERY_CLASSES = ["calm", "ordinary", "urgent"]
INTENT_CLASSES = ["order", "question"]

STYLE_TO_DELIVERY = {"Calm": "calm", "Neutral": "ordinary", "Angry": "urgent"}

#: The legend for the 7-field L2 line (E4b's, verbatim).
LEGEND_V3 = (
    "The performance line describes how the voice was delivered, measured "
    "against this speaker's own usual delivery. 'soft' means quieter than "
    "this speaker usually is, 'normal' means the usual level, 'loud' means "
    "louder than usual. 'bright' describes the timbre, 'tempo' the pace in "
    "syllables per second, and 'dur' how long the whole delivery lasted, in "
    "seconds. 'contour' is how far the pitch moved over the whole delivery, "
    "in fractions of this speaker's usual pitch range: a negative number "
    "means the pitch ended lower than it started, a positive number means it "
    "ended higher, and the word 'level' means it ended about where it "
    "started. 'end' is where the pitch ended, again in fractions of this "
    "speaker's usual pitch range: negative means the last word ended below "
    "this speaker's usual pitch, positive means above it. 'tone' is the "
    "movement of the very last moment of the voice: 'rise', 'fall' or "
    "'level'."
)

ARMS = ("l2-v3", "l2-v3-read", "marks", "marks-vocab", "marks-read")


def post_json(url: str, payload: dict, timeout: float = 120.0) -> dict:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"},
        method="POST")
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read())


def ask_gate(state: str, questions: dict, timeout: float = 120.0):
    payload = {"model": "d1-2B", "state": state, "questions": questions}
    started = time.perf_counter()
    try:
        out = post_json(D1_2B, payload, timeout=timeout)
    except Exception as exc:  # noqa: BLE001
        return {}, 0.0, str(exc)
    latency = (time.perf_counter() - started) * 1000.0
    return out.get("answers", {}), latency, ""


def arm_state(arm: str, row: dict) -> str:
    """The state each arm hands to the gate."""
    if arm in ("l2-v3", "l2-v3-read"):
        return ("A voice delivery was measured by an analyzer. Its performance "
                f"line reads:\n{row['l2_v3']}\n{LEGEND_V3}")
    if arm in ("marks", "marks-vocab", "marks-read"):
        return ("A voice delivery was measured by an analyzer. Its performance "
                f"marks read:\n{row['marks']}\n{MARKS_LEGEND}")
    raise ValueError(arm)


def accuracy(pairs: list[tuple[str, str]]) -> float:
    if not pairs:
        return 0.0
    return sum(1 for t, p in pairs if t == p) / len(pairs)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--marks", default="evidence/vox-e6/marks.csv")
    parser.add_argument("--out", default="evidence/vox-e6")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--arms", default="", help="comma-separated subset")
    args = parser.parse_args()

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = list(csv.DictReader(Path(args.marks).open(encoding="utf-8")))
    wanted = {a.strip() for a in args.arms.split(",") if a.strip()}
    arms = [a for a in ARMS if not wanted or a in wanted]
    print(f"{len(rows)} clips x {len(arms)} arms = {len(rows)*len(arms)} gate calls")

    results: list[dict] = []
    pool = ThreadPoolExecutor(max_workers=args.workers)
    try:
        for arm in arms:
            pack = (READ_QUESTIONS if arm in ("marks-read", "l2-v3-read") else
                    MARKS_VOCAB_QUESTIONS if arm == "marks-vocab" else QUESTIONS)

            def one(row: dict, arm: str = arm) -> list[dict]:
                state = arm_state(arm, row)
                answers, latency, error = ask_gate(state, pack)
                out = []
                for gate, classes in (("urgency", DELIVERY_CLASSES),
                                      ("order_or_question", INTENT_CLASSES),
                                      ("pitch_end", ["higher", "lower"])):
                    answer = answers.get(gate, {})
                    choice = answer.get("choice", "")
                    if gate == "pitch_end":
                        truth = ("higher"
                                 if float(row["contour_median_norm"]) > 0 else "lower")
                    elif gate == "urgency":
                        truth = STYLE_TO_DELIVERY.get(row["style"], "")
                    else:
                        truth = row["intent"]
                    out.append({
                        "arm": arm, "gate": gate, "file": row["file"],
                        "speaker": row["speaker"], "style": row["style"],
                        "intent": row["intent"], "truth": truth, "pred": choice,
                        "confidence": answer.get("confidence", ""),
                        "probabilities": json.dumps(answer.get("probabilities", {})),
                        "latency_ms": round(latency, 1), "error": error,
                    })
                return out

            n = 0
            for out in pool.map(one, rows):
                results.extend(out)
                n += 1
                if n % 80 == 0 or n == len(rows):
                    print(f"  arm {arm}: {n}/{len(rows)}")
    finally:
        pool.shutdown(wait=True)

    fields = ["arm", "gate", "file", "speaker", "style", "intent", "truth",
              "pred", "confidence", "probabilities", "latency_ms", "error"]
    with (out_dir / "gate_results.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(results)

    # ---- summarise --------------------------------------------------------
    summary: dict = {"arms": {}}
    print()
    print(f"{'arm':12s} {'gate':18s} {'n':>4s} {'acc':>7s} {'base':>7s} "
          f"{'bin%':>7s} {'ms':>6s}  answered")
    print("-" * 84)
    for arm in arms:
        summary["arms"][arm] = {}
        for gate in ("urgency", "order_or_question", "pitch_end"):
            arm_rows = [r for r in results if r["arm"] == arm and r["gate"] == gate]
            if not arm_rows:
                continue
            if gate == "urgency":
                arm_rows = [r for r in arm_rows if r["truth"] in DELIVERY_CLASSES]
            # A gate that was never asked this question has no answers, and an
            # empty prediction is not a wrong one: skip the row rather than
            # report it as 0%.
            if not any(r["pred"] for r in arm_rows):
                continue
            pairs = [(r["truth"], r["pred"]) for r in arm_rows]
            counts = Counter(t for t, _ in pairs)
            majority = max(counts.values()) / len(pairs) if pairs else 0.0
            latencies = sorted(float(r["latency_ms"]) for r in arm_rows
                               if r["latency_ms"])
            entry = {
                "n": len(pairs),
                "accuracy": round(accuracy(pairs), 4),
                "majority_baseline": round(majority, 4),
                "answered": sum(1 for _, p in pairs if p),
                "predictions": dict(Counter(p for _, p in pairs)),
                "median_latency_ms": round(latencies[len(latencies)//2], 1)
                                    if latencies else 0.0,
            }
            if gate == "urgency":
                binary = [("urgent" if t == "urgent" else "not",
                           ("urgent" if p == "urgent" else "not") if p else "")
                          for t, p in pairs]
                entry["binary_urgent_vs_not"] = {
                    "accuracy": round(accuracy(binary), 4)}
            summary["arms"][arm][gate] = entry
            binary_text = (f"{entry['binary_urgent_vs_not']['accuracy']*100:6.1f}%"
                           if "binary_urgent_vs_not" in entry else "      -")
            print(f"{arm:12s} {gate:18s} {entry['n']:4d} "
                  f"{entry['accuracy']*100:6.1f}% {majority*100:6.1f}% "
                  f"{binary_text} {entry['median_latency_ms']:6.0f}")

    # ---- how long is each encoding? --------------------------------------
    marks_words = [len(row["marks"].split()) for row in rows]
    l2_words = [len(row["l2_v3"].split()) for row in rows]
    legend_words = len(MARKS_LEGEND.split())
    legend_v3_words = len(LEGEND_V3.split())
    summary["length"] = {
        "marks_line_words": round(sum(marks_words)/len(marks_words), 2),
        "marks_line_words_min": min(marks_words),
        "marks_line_words_max": max(marks_words),
        "l2_v3_line_words": round(sum(l2_words)/len(l2_words), 2),
        "marks_legend_words": legend_words,
        "l2_v3_legend_words": legend_v3_words,
        "marks_state_words": round(sum(marks_words)/len(marks_words), 2) + legend_words,
        "l2_v3_state_words": round(sum(l2_words)/len(l2_words), 2) + legend_v3_words,
    }
    print()
    print(f"marks line: {summary['length']['marks_line_words']} words "
          f"({summary['length']['marks_line_words_min']}-"
          f"{summary['length']['marks_line_words_max']}), "
          f"legend {legend_words} words, state "
          f"{summary['length']['marks_state_words']:.0f} words")
    print(f"L2 v3 line: {summary['length']['l2_v3_line_words']} words, "
          f"legend {legend_v3_words} words, state "
          f"{summary['length']['l2_v3_state_words']:.0f} words")

    (out_dir / "gate_summary.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8")
    print(f"\nwrote {out_dir/'gate_results.csv'} and {out_dir/'gate_summary.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
