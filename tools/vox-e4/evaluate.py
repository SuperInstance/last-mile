"""E4: do the dials carry intent?

Four arms, as the brief names them:

  l2         the dials alone: the d1-2B gate sees only the L2 line
  lfm3       LFM3 alone: LFM3 answers the gate from the audio itself,
             with no d1-2B in the loop
  words      the words alone: the d1-2B gate sees only LFM3's transcript
  l2+words   both: the d1-2B gate sees the L2 line and the transcript

Two supplementary arms are also run and clearly labelled:
  oracle-words  the gate sees the words that were rendered, not the words
                LFM3 heard, which upper-bounds the lexical channel
  lfm3->gate    LFM3's transcript fed to the d1-2B gate, so the transcript
                can be compared like for like against the other gate arms

Every arm answers the same two gates:

  urgency          calm / ordinary / urgent      (the delivery)
  order_or_question  order / question            (the words)

    python tools/vox-e4/evaluate.py --dataset evidence/vox-e4
"""

from __future__ import annotations

import argparse
import base64
import csv
import json
import re
import time
import urllib.request
from collections import Counter, defaultdict

D1_2B = "http://localhost:8094/v1/systemone"
LFM3 = "http://localhost:8102/v1/chat/completions"

#: The delivery each Riva style stands for. Happy and Sad are not part of
#: the brief's calm/ordinary/urgent vocabulary, so they are excluded from
#: the urgency gate but kept for the order/question gate.
STYLE_TO_DELIVERY = {
    "Calm": "calm",
    "Neutral": "ordinary",
    "Angry": "urgent",
}

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

LFM3_PROMPTS = {
    "urgency": "Rate the urgency of this delivery: calm, ordinary, or urgent? One word only.",
    "order_or_question": "Is the speaker asking a question or giving an order? Reply with exactly one word.",
}

#: The vocabulary of the L2 line, spelled out. A format that a small model
#: has to guess the meaning of is not an interchange format.
LEGEND = (
    "The performance line describes how the voice was delivered, against this "
    "speaker's own usual delivery. 'soft' means quieter than this speaker "
    "usually is, 'normal' means the usual level, 'loud' means louder than "
    "usual. 'fast' means the loudness falls away quicker than usual, 'slow' "
    "means it holds. 'hard' means the voice stops abruptly, 'soft' means it "
    "trails off. 'bright' describes the timbre, 'tempo' the pace."
)

DELIVERY_CLASSES = ["calm", "ordinary", "urgent"]
INTENT_CLASSES = ["order", "question"]


def post_json(url: str, payload: dict, timeout: float = 120.0) -> dict:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"}, method="POST"
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read())


def chat(messages: list, max_tokens: int = 400, timeout: float = 300.0) -> dict:
    payload = {
        "model": "LFM3-Audio-2B",
        "messages": messages,
        "max_tokens": max_tokens,
        "temperature": 0,
    }
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url=LFM3, data=data,
        headers={"Content-Type": "application/json"}, method="POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read())


def message_text(out: dict) -> tuple[str, str]:
    message = out["choices"][0]["message"]
    content = message.get("content") or ""
    if isinstance(content, list):
        content = " ".join(part.get("text", "") for part in content)
    return content.strip(), (message.get("reasoning_content") or "").strip()


def pick_option(text: str, options: list[str]) -> str:
    """First option mentioned in the text, preferring the last mention."""
    lowered = text.lower()
    best = ""
    best_pos = -1
    for option in options:
        for match in re.finditer(rf"\b{re.escape(option)}\b", lowered):
            best, best_pos = option, match.start()
    return best if best_pos >= 0 else ""


def ask_gate(state: str, timeout: float = 120.0) -> tuple[dict, float]:
    """Ask the d1-2B gate the two questions. Returns (answers, latency_ms)."""
    payload = {"model": "d1-2B", "state": state, "questions": QUESTIONS}
    started = time.perf_counter()
    out = post_json(D1_2B, payload, timeout=timeout)
    latency = (time.perf_counter() - started) * 1000.0
    return out.get("answers", {}), latency


def arm_state(arm: str, l2_abs: str, l2_rel: str, heard: str, truth: str) -> str:
    """The plain-language state each arm hands to the gate."""
    if arm in ("l2-abs", "l2"):
        line = l2_abs if arm == "l2-abs" else l2_rel
        return ("A voice delivery was measured by an analyzer. Its performance "
                f"line reads:\n{line}\n{LEGEND}")
    if arm == "words":
        return ("A voice delivery was heard. The words spoken were:\n"
                f"{heard}")
    if arm == "oracle-words":
        return ("A voice delivery was heard. The words spoken were:\n"
                f"{truth}")
    if arm in ("l2-abs+words", "l2+words"):
        line = l2_abs if arm == "l2-abs+words" else l2_rel
        return ("A voice delivery was measured by an analyzer and heard. Its "
                f"performance line reads:\n{line}\n{LEGEND}\n"
                f"The words spoken were:\n{heard}")
    raise ValueError(arm)


def confusion(pairs: list[tuple[str, str]], classes: list[str]) -> dict:
    matrix = {truth: {pred: 0 for pred in classes} for truth in classes}
    for truth, pred in pairs:
        if truth in matrix and pred in matrix[truth]:
            matrix[truth][pred] += 1
    return matrix


def accuracy(pairs: list[tuple[str, str]]) -> float:
    if not pairs:
        return 0.0
    return sum(1 for truth, pred in pairs if truth == pred) / len(pairs)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", default="evidence/vox-e4")
    parser.add_argument("--out", default=None)
    parser.add_argument("--arms", default="",
                        help="comma-separated subset of arms to run "
                             "(default: all)")
    parser.add_argument("--results", default=None,
                        help="read existing answers from this CSV instead of "
                             "asking again (used to add an arm later)")
    parser.add_argument("--summarize-only", action="store_true",
                        help="rebuild summary.json from an existing results CSV")
    args = parser.parse_args()
    dataset = args.dataset

    wanted = {a.strip() for a in args.arms.split(",") if a.strip()}
    gate_arms = ["l2-abs", "l2", "words", "l2-abs+words", "l2+words",
                 "oracle-words"]
    if wanted:
        gate_arms = [a for a in gate_arms if a in wanted]
    run_lfm3 = (not wanted) or ("lfm3" in wanted)
    if args.summarize_only:
        gate_arms = []
        run_lfm3 = False

    manifest = {row["file"]: row for row in
                csv.DictReader(open(f"{dataset}/manifest.csv", encoding="utf-8"))
                if not row.get("error")}
    l2_rows = {row["file"]: row for row in
               csv.DictReader(open(f"{dataset}/l2.csv", encoding="utf-8"))}
    transcripts = {row["file"]: row for row in
                   csv.DictReader(open(f"{dataset}/transcripts.csv", encoding="utf-8"))}

    files = [f for f in manifest if f in l2_rows and f in transcripts]
    print(f"{len(files)} clips in the evaluation set")

    # Guard: the manifest must carry the line that was spoken and the label
    # that describes it. If these were ever swapped, every number below
    # would be about the wrong thing.
    for path, row in list(manifest.items())[:5]:
        if row["text"].lower() in ("order", "question"):
            raise SystemExit(
                f"manifest for {path} holds the label {row['text']!r} where the "
                "spoken line should be; the render step swapped text and intent"
            )
        if row["intent"] not in ("order", "question"):
            raise SystemExit(f"manifest for {path} has intent {row['intent']!r}")

    results: list[dict] = []
    existing_path = args.results or f"{dataset}/arm_results.csv"
    if args.summarize_only or args.results:
        existing = list(csv.DictReader(open(existing_path, encoding="utf-8")))
        results.extend(existing)
        print(f"loaded {len(existing)} existing answers from {existing_path}")

    # --- arms that go through the d1-2B gate -------------------------------
    for arm in gate_arms:
        print(f"arm {arm}: asking the d1-2B gate on {len(files)} clips")
        for n, path in enumerate(files, start=1):
            row = manifest[path]
            l2_abs = l2_rows[path]["l2_abs"]
            l2_rel = l2_rows[path]["l2_rel"]
            heard = transcripts[path]["heard_text"]
            truth = row["text"]
            state = arm_state(arm, l2_abs, l2_rel, heard, truth)
            try:
                answers, latency = ask_gate(state)
                error = ""
            except Exception as exc:  # noqa: BLE001
                answers, latency, error = {}, 0.0, str(exc)
            for gate, classes in (("urgency", DELIVERY_CLASSES),
                                  ("order_or_question", INTENT_CLASSES)):
                answer = answers.get(gate, {})
                choice = answer.get("choice", "")
                results.append({
                    "arm": arm,
                    "gate": gate,
                    "file": path,
                    "speaker": row["speaker"],
                    "style": row["style"],
                    "intent": row["intent"],
                    "truth": STYLE_TO_DELIVERY.get(row["style"], "") if gate == "urgency"
                             else row["intent"],
                    "pred": choice,
                    "confidence": answer.get("confidence", ""),
                    "probabilities": json.dumps(answer.get("probabilities", {})),
                    "latency_ms": round(latency, 1),
                    "error": error,
                })
            if n % 40 == 0 or n == len(files):
                print(f"  {n}/{len(files)}")

    # --- the LFM3-only arm: LFM3 answers the gate from the audio ----------
    if run_lfm3:
        print("arm lfm3: LFM3 answers the gates from the audio itself")
        for n, path in enumerate(files, start=1):
            row = manifest[path]
            with open(path, "rb") as handle:
                audio = base64.b64encode(handle.read()).decode()
            for gate, classes in (("urgency", DELIVERY_CLASSES),
                                  ("order_or_question", INTENT_CLASSES)):
                message = {"role": "user", "content": [
                    {"type": "text", "text": LFM3_PROMPTS[gate]},
                    {"type": "input_audio",
                     "input_audio": {"data": audio, "format": "wav"}},
                ]}
                started = time.perf_counter()
                try:
                    out = chat([message])
                    latency = (time.perf_counter() - started) * 1000.0
                    content, reasoning = message_text(out)
                    choice = pick_option(content, classes)
                    source = "content"
                    if not choice:
                        choice = pick_option(reasoning, classes)
                        source = "reasoning"
                    error = "" if choice else "no answer"
                except Exception as exc:  # noqa: BLE001
                    latency, choice, source, error = 0.0, "", "", str(exc)
                results.append({
                    "arm": "lfm3",
                    "gate": gate,
                    "file": path,
                    "speaker": row["speaker"],
                    "style": row["style"],
                    "intent": row["intent"],
                    "truth": STYLE_TO_DELIVERY.get(row["style"], "") if gate == "urgency"
                             else row["intent"],
                    "pred": choice,
                    "confidence": source,
                    "probabilities": "",
                    "latency_ms": round(latency, 1),
                    "error": error,
                })
            if n % 20 == 0 or n == len(files):
                print(f"  {n}/{len(files)}")

    # --- write the per-answer table ---------------------------------------
    fields = ["arm", "gate", "file", "speaker", "style", "intent", "truth",
              "pred", "confidence", "probabilities", "latency_ms", "error"]
    out_path = args.out or f"{dataset}/arm_results.csv"
    if args.summarize_only:
        out_path = args.out or f"{dataset}/arm_results.csv"
    with open(out_path, "w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(results)
    print(f"wrote {out_path} ({len(results)} answers)")

    # --- summarise ----------------------------------------------------------
    summary: dict = {"arms": {}, "totals": {}}
    for arm in sorted({r["arm"] for r in results}):
        summary["arms"][arm] = {}
        for gate, classes in (("urgency", DELIVERY_CLASSES),
                              ("order_or_question", INTENT_CLASSES)):
            rows = [r for r in results if r["arm"] == arm and r["gate"] == gate]
            if gate == "urgency":
                rows = [r for r in rows if r["truth"] in classes]
            pairs = [(r["truth"], r["pred"]) for r in rows]
            answered = [(t, p) for t, p in pairs if p]
            counts = Counter(t for t, _ in pairs)
            majority = max(counts.values()) / len(pairs) if pairs else 0.0
            latencies = [float(r["latency_ms"]) for r in rows if r["latency_ms"]]
            entry = {
                "n": len(pairs),
                "answered": len(answered),
                "accuracy": round(accuracy(pairs), 4),
                "accuracy_answered_only": round(accuracy(answered), 4),
                "majority_baseline": round(majority, 4),
                "confusion": confusion(pairs, classes),
                "median_latency_ms": round(sorted(latencies)[len(latencies)//2], 1)
                                    if latencies else 0.0,
                "mean_latency_ms": round(sum(latencies)/len(latencies), 1)
                                   if latencies else 0.0,
            }
            if gate == "urgency":
                # The measured evidence separates high arousal from low
                # arousal but not always Riva's Calm from its Neutral, so the
                # collapsed binary is reported alongside the three classes.
                # The pair is (truth, prediction) for the confusion matrix.
                binary = [("urgent" if t == "urgent" else "not",
                           ("urgent" if p == "urgent" else "not") if p else "")
                          for t, p in pairs]
                entry["binary_urgent_vs_not"] = {
                    "accuracy": round(accuracy(binary), 4),
                    "confusion": confusion(binary, ["not", "urgent"]),
                }
            summary["arms"][arm][gate] = entry

    # ASR accuracy per style, which is a confound worth reporting.
    asr = defaultdict(list)
    for path, row in transcripts.items():
        asr[row["style"]].append(int(row["exact"]))
    summary["asr_exact_by_style"] = {
        style: {"n": len(values), "exact": sum(values),
                "rate": round(sum(values)/len(values), 4)}
        for style, values in sorted(asr.items())
    }
    summary["totals"] = {
        "clips": len(files),
        "answers": len(results),
        "asr_exact_overall": round(
            sum(int(r["exact"]) for r in transcripts.values())/len(transcripts), 4),
    }

    with open(f"{dataset}/summary.json", "w", encoding="utf-8") as handle:
        json.dump(summary, handle, indent=2)

    print()
    header = (f"{'arm':16s} {'gate':18s} {'n':>4s} {'acc':>7s} {'answered':>9s} "
              f"{'base':>7s} {'bin%':>7s} {'ms':>7s}")
    print(header)
    print("-" * len(header))
    for arm in sorted(summary["arms"]):
        for gate in ("urgency", "order_or_question"):
            s = summary["arms"][arm][gate]
            binary = s.get("binary_urgent_vs_not")
            binary_text = (f"{binary['accuracy']*100:6.1f}%" if binary else "      -")
            print(f"{arm:16s} {gate:18s} {s['n']:4d} {s['accuracy']*100:6.1f}% "
                  f"{s['answered']:4d}/{s['n']:<4d} {s['majority_baseline']*100:6.1f}% "
                  f"{binary_text} {s['median_latency_ms']:7.0f}")
    print(f"\nwrote {dataset}/summary.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
