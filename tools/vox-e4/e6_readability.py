"""E6 step 4: can System Two read a delivery out of the marks?

Twenty pairs of clips — the same words, two different deliveries — are shown
to a larger local model **as marks only**, with no audio, and it is asked
which of the two was the urgent one and why.

This is the test design §11 argues for. The marks are meant to be the words
System Two thinks in: "this player's urgent orders are always
strong-clipped-falling". If a reasoning model can say which line was urgent
from the marks alone, and can say *why* in terms of the marks, then the
notation is doing the job §11 wants it to do.

Two encodings are run over the same twenty pairs:

  marks  the mark set, with the legend that spells the vocabulary out
  l2-v3  the 7-field L2 line, with E4b's legend — the control, so the marks
         are compared with the dials they were quantized from rather than
         with nothing

The pairs are ordered at random, so position carries no information, and
the answer is scored against which side of the pair is actually the urgent
one.

The model is qwen3-coder through Ollama (:11434). It is loaded on first use
and unloaded with ``keep_alive: 0`` when the run is done.

    python tools/vox-e4/e6_readability.py --marks evidence/vox-e6/marks.csv \
        --model qwen3-coder
"""

from __future__ import annotations

import argparse
import csv
import json
import random
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "vox-ref"))

from vox_ref.marks import MARKS_LEGEND  # noqa: E402

OLLAMA = "http://localhost:11434"

#: The legend for the 7-field L2 line (E4b's, verbatim) — the control.
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

INSTRUCTIONS = (
    "Two recordings of the same sentence are described below. The audio is "
    "not available; only the description of how each one was delivered. "
    "Exactly one of them was spoken as an urgent delivery and the other was "
    "not. Which one was the urgent one? Answer with the letter, and then "
    "give your reason in one or two sentences, in terms of the description."
)

SCHEMA = {
    "type": "object",
    "properties": {
        "urgent": {"type": "string", "enum": ["A", "B"]},
        "reason": {"type": "string"},
    },
    "required": ["urgent", "reason"],
}


def post_json(url: str, payload: dict, timeout: float = 600.0) -> dict:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"},
        method="POST")
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read())


def build_pairs(rows: list[dict], count: int = 20, seed: int = 6) -> list[dict]:
    """Twenty same-word pairs, one per line, with the urgent side known.

    The urgent side is the Angry rendering; the other side is Calm or
    Neutral, cycling so the pairs are not all the same contrast.
    """
    by_key: dict[tuple[str, str, str], dict] = {}
    for row in rows:
        by_key[(row["text"], row["speaker"], row["style"])] = row
    lines = sorted({row["text"] for row in rows})
    speakers = sorted({row["speaker"] for row in rows})
    others = ["Calm", "Neutral"]

    pairs = []
    for index, text in enumerate(lines[:count]):
        speaker = speakers[index % len(speakers)]
        urgent = by_key.get((text, speaker, "Angry"))
        if urgent is None:
            continue
        other_style = others[(index // len(speakers)) % len(others)]
        other = by_key.get((text, speaker, other_style))
        if other is None:
            continue
        pairs.append({
            "text": text,
            "speaker": speaker,
            "urgent": urgent,
            "other": other,
            "other_style": other_style,
        })
    return pairs


def render(pair: dict, encoding: str, urgent_letter: str) -> tuple[str, dict]:
    """The prompt for one pair, and the mapping back to the clips."""
    first, second = pair["urgent"], pair["other"]
    if urgent_letter == "B":
        first, second = second, first
    if encoding == "marks":
        body = (f"Delivery A: {first['marks']}\n"
                f"Delivery B: {second['marks']}\n\n{MARKS_LEGEND}")
        state = {"A": first, "B": second}
    else:
        body = (f"Delivery A: {first['l2_v3']}\n"
                f"Delivery B: {second['l2_v3']}\n\n{LEGEND_V3}")
        state = {"A": first, "B": second}
    prompt = (f"{INSTRUCTIONS}\n\nThe sentence spoken in both: "
              f"\"{pair['text']}\"\n\n{body}")
    return prompt, state


def ask(model: str, prompt: str, timeout: float = 600.0) -> dict:
    payload = {
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "format": SCHEMA,
        "stream": False,
        "options": {"temperature": 0, "num_predict": 400},
        "keep_alive": "30m",
    }
    out = post_json(f"{OLLAMA}/api/chat", payload, timeout=timeout)
    message = out.get("message", {})
    content = message.get("content") or ""
    if isinstance(content, list):
        content = " ".join(part.get("text", "") for part in content)
    answer: dict = {}
    try:
        answer = json.loads(content)
    except ValueError:
        answer = {}
    return {
        "urgent": str(answer.get("urgent", "")).strip().upper()[:1],
        "reason": str(answer.get("reason", "")).strip(),
        "raw": content,
        "eval_count": out.get("eval_count", 0),
    }


def unload(model: str) -> None:
    """Take the model back off the GPU."""
    payload = {"model": model, "keep_alive": 0}
    try:
        post_json(f"{OLLAMA}/api/generate", payload, timeout=60.0)
    except Exception as exc:  # noqa: BLE001
        print(f"  (unload failed: {exc})")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--marks", default="evidence/vox-e6/marks.csv")
    parser.add_argument("--out", default="evidence/vox-e6")
    parser.add_argument("--model", default="qwen3-coder")
    parser.add_argument("--pairs", type=int, default=20)
    parser.add_argument("--seed", type=int, default=6)
    parser.add_argument("--encodings", default="marks,l2-v3")
    args = parser.parse_args()

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    rows = list(csv.DictReader(Path(args.marks).open(encoding="utf-8")))
    pairs = build_pairs(rows, count=args.pairs, seed=args.seed)
    print(f"{len(pairs)} pairs, encodings: {args.encodings}")
    for pair in pairs[:3]:
        print(f"  {pair['speaker']:6s} {pair['other_style']:8s} "
              f"{pair['urgent']['marks']}  vs  {pair['other']['marks']}")

    rng = random.Random(args.seed)
    results: list[dict] = []
    for encoding in args.encodings.split(","):
        encoding = encoding.strip()
        if not encoding:
            continue
        print(f"\nencoding {encoding}: asking {args.model}")
        for index, pair in enumerate(pairs):
            urgent_letter = "A" if rng.random() < 0.5 else "B"
            prompt, state = render(pair, encoding, urgent_letter)
            started = time.perf_counter()
            try:
                answer = ask(args.model, prompt)
                error = ""
            except Exception as exc:  # noqa: BLE001
                answer = {"urgent": "", "reason": "", "raw": "", "eval_count": 0}
                error = str(exc)
            latency = (time.perf_counter() - started) * 1000.0
            predicted = answer["urgent"]
            correct = predicted == urgent_letter
            results.append({
                "encoding": encoding,
                "pair": index,
                "text": pair["text"],
                "speaker": pair["speaker"],
                "urgent_style": "Angry",
                "other_style": pair["other_style"],
                "urgent_letter": urgent_letter,
                "predicted": predicted,
                "correct": int(correct),
                "reason": answer["reason"],
                "marks_urgent": pair["urgent"]["marks"],
                "marks_other": pair["other"]["marks"],
                "l2_urgent": pair["urgent"]["l2_v3"],
                "l2_other": pair["other"]["l2_v3"],
                "latency_ms": round(latency, 1),
                "eval_count": answer["eval_count"],
                "error": error,
            })
            flag = "ok " if correct else "MISS"
            print(f"  [{index:2d}] {flag} {encoding:6s} urgent={urgent_letter} "
                  f"pred={predicted or '-':2s} {latency:6.0f}ms "
                  f"| {answer['reason'][:90]}")

    fields = list(results[0].keys())
    with (out_dir / "readability.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(results)

    summary: dict = {"model": args.model, "pairs": len(pairs), "encodings": {}}
    print()
    for encoding in {r["encoding"] for r in results}:
        rows = [r for r in results if r["encoding"] == encoding]
        correct = sum(r["correct"] for r in rows)
        summary["encodings"][encoding] = {
            "n": len(rows),
            "correct": correct,
            "accuracy": round(correct / len(rows), 4) if rows else 0.0,
            "chance": 0.5,
            "median_latency_ms": round(
                sorted(r["latency_ms"] for r in rows)[len(rows)//2], 1) if rows else 0,
            "errors": sum(1 for r in rows if r["error"]),
        }
        print(f"{encoding:8s} {correct}/{len(rows)} = "
              f"{correct/len(rows)*100:.1f}%  (chance 50.0%)")
    (out_dir / "readability_summary.json").write_text(
        json.dumps(summary, indent=2), encoding="utf-8")

    print("\nunloading the model")
    unload(args.model)
    print(f"wrote {out_dir/'readability.csv'} and {out_dir/'readability_summary.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
