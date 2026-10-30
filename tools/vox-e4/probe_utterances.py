"""Pick the E4 utterance set by measuring what LFM3 actually transcribes.

The E4 comparison is "dials plus words" against "words alone", so the words
have to be ones the ears really hear: if LFM3 mis-transcribes a line, the
lexical channel is noisy and the comparison measures ASR error rather than
dials. This script renders every candidate once and reports the transcript,
so the set can be chosen on evidence.

    python tools/vox-e4/probe_utterances.py
"""

from __future__ import annotations

import json
import os
import time
import urllib.request

CANDIDATES = [
    ("order", "Hold the line."),
    ("order", "Move to the beacon."),
    ("order", "Fall back to cover."),
    ("order", "Wait for my signal."),
    ("order", "We need that supply crate."),
    ("order", "Cover the north approach."),
    ("order", "Get behind the barricade."),
    ("order", "Take the east passage."),
    ("order", "Keep them off the ridge."),
    ("order", "Stay on the road."),
    ("order", "Bring the rover around."),
    ("order", "Pick up the radio."),
    ("order", "Hold at the treeline."),
    ("order", "Watch the south bridge."),
    ("question", "Do you see the beacon?"),
    ("question", "Where is the enemy?"),
    ("question", "Can you hear me?"),
    ("question", "Which way is north?"),
    ("question", "Are you in position?"),
    ("question", "What is your status?"),
    ("question", "Who has the map?"),
    ("question", "Is the road clear?"),
    ("question", "How many contacts?"),
    ("question", "Did you get that?"),
    ("question", "Any movement on the ridge?"),
]


def post_json(url: str, payload: dict, timeout: float = 120.0) -> bytes:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"}, method="POST"
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


def post_multipart(url: str, filepath: str, fields: dict, timeout: float = 180.0) -> dict:
    boundary = "----voxeval"
    body = b""
    for key, value in fields.items():
        body += (
            f'--{boundary}\r\nContent-Disposition: form-data; name="{key}"\r\n\r\n'
            f"{value}\r\n"
        ).encode()
    body += (
        f'--{boundary}\r\nContent-Disposition: form-data; name="file"; '
        f'filename="{os.path.basename(filepath)}"\r\nContent-Type: audio/wav\r\n\r\n'
    ).encode()
    with open(filepath, "rb") as handle:
        body += handle.read()
    body += f"\r\n--{boundary}--\r\n".encode()
    request = urllib.request.Request(
        url,
        data=body,
        headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read())


def normalise(text: str) -> str:
    return "".join(ch for ch in text.lower() if ch.isalnum() or ch == " ").strip()


def main() -> int:
    out_dir = "/tmp/vox_candidates"
    os.makedirs(out_dir, exist_ok=True)
    rows = []
    for index, (intent, text) in enumerate(CANDIDATES):
        path = f"{out_dir}/{index:02d}.wav"
        payload = {"input": text, "voice": "Magpie-Multilingual.EN-US.Mia.Neutral"}
        started = time.perf_counter()
        body = post_json("http://localhost:8000/v1/audio/speech", payload)
        render_ms = (time.perf_counter() - started) * 1000.0
        with open(path, "wb") as handle:
            handle.write(body)
        result = post_multipart(
            "http://localhost:8102/v1/audio/transcriptions", path, {"model": "LFM3-Audio-2B"}
        )
        heard = (result.get("text") or "").strip()
        exact = normalise(heard) == normalise(text)
        rows.append({"intent": intent, "text": text, "heard": heard, "exact": exact,
                     "render_ms": round(render_ms, 1),
                     "asr_latency_ms": result.get("latency_ms")})
        flag = "OK " if exact else "BAD"
        print(f"[{flag}] {intent:8s} {text!r:38s} -> {heard!r}")

    exact_orders = [r for r in rows if r["exact"] and r["intent"] == "order"]
    exact_questions = [r for r in rows if r["exact"] and r["intent"] == "question"]
    print(f"\nexact: {len(exact_orders)} orders, {len(exact_questions)} questions")
    with open("/tmp/vox_candidates/report.json", "w", encoding="utf-8") as handle:
        json.dump(rows, handle, indent=2)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
