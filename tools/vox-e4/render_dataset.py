"""Render the E4 dataset with the skald daemon.

The grid is: utterances x speakers x Riva styles. The words are held fixed
across styles, so the delivery class (calm / ordinary / urgent) is the only
thing that varies within an utterance, and a words-only reader cannot do
better than chance on the urgency gate.

    python tools/vox-e4/render_dataset.py --out evidence/vox-e4
"""

from __future__ import annotations

import argparse
import csv
import json
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

SKALD = "http://localhost:8000"

#: Twenty lines: ten orders/statements and ten questions.
#:
#: Every one of these was checked against LFM3's ASR first (see
#: `probe_utterances.py`): the words have to survive the ears, or the
#: words-only arm would be measuring ASR error instead of dials.
UTTERANCES = [
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
]

#: The three Riva voices used as speakers.
SPEAKERS = ["Mia", "Jason", "Aria"]

#: Styles every one of the three speakers carries.
STYLES_COMMON = ["Neutral", "Calm", "Happy", "Angry"]

#: Styles only some speakers carry; rendered where available.
STYLES_EXTRA = ["Sad"]


def voice_id(speaker: str, style: str) -> str:
    """The canonical Riva voice id for a speaker and style.

    Riva exposes the plain style as the un-suffixed voice id.
    """
    if style == "Default":
        return f"Magpie-Multilingual.EN-US.{speaker}"
    return f"Magpie-Multilingual.EN-US.{speaker}.{style}"


def post_json(url: str, payload: dict, timeout: float = 120.0) -> tuple[int, bytes, str]:
    """POST JSON with the standard library. Returns (status, body, error)."""
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"}, method="POST"
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return response.status, response.read(), ""
    except urllib.error.HTTPError as exc:
        return exc.code, exc.read(), exc.read().decode("utf-8", "replace")[:200]
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        return 0, b"", str(exc)


def get_json(url: str, timeout: float = 30.0) -> dict:
    with urllib.request.urlopen(url, timeout=timeout) as response:
        return json.loads(response.read().decode("utf-8"))


def render_one(out_dir: Path, speaker: str, style: str, text: str, intent: str,
               index: int) -> dict:
    """Render one clip and write it to disk."""
    vid = voice_id(speaker, style)
    name = f"{index:03d}_{speaker}_{style}_{intent}.wav"
    path = out_dir / "clips" / name
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = {"input": text, "voice": vid, "response_format": "wav"}
    started = time.perf_counter()
    status, body, error = post_json(f"{SKALD}/v1/audio/speech", payload)
    elapsed = time.perf_counter() - started
    if status != 200 or error:
        return {"file": str(path), "speaker": speaker, "style": style, "text": text,
                "intent": intent, "voice_id": vid, "error": error or f"http {status}",
                "render_ms": round(elapsed * 1000, 1)}
    path.write_bytes(body)
    return {
        "file": str(path),
        "speaker": speaker,
        "style": style,
        "text": text,
        "intent": intent,
        "voice_id": vid,
        "bytes": len(body),
        "render_ms": round(elapsed * 1000, 1),
        "error": "",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", default="evidence/vox-e4")
    parser.add_argument("--workers", type=int, default=6)
    args = parser.parse_args()

    # Guard: the utterance tuples are (intent, text). If they are ever
    # swapped, the dataset silently becomes one word per class and every
    # later measurement is meaningless.
    for intent, text in UTTERANCES:
        if text.lower() in ("order", "question", "calm", "urgent", "ordinary"):
            raise SystemExit(
                f"utterance {text!r} looks like a label, not a line to speak; "
                "the (intent, text) tuples are probably swapped"
            )
        if intent not in ("order", "question"):
            raise SystemExit(f"unknown intent {intent!r}")
        if len(text.split()) < 2:
            raise SystemExit(f"utterance {text!r} is too short to be a line")

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    # Which (speaker, style) pairs the server actually offers.
    voices = get_json(f"{SKALD}/v1/voices")["voices"]
    available = {v["voice_id"] for v in voices}

    jobs = []
    index = 0
    for intent, text in UTTERANCES:
        for speaker in SPEAKERS:
            for style in ["Default"] + STYLES_COMMON + STYLES_EXTRA:
                vid = voice_id(speaker, style)
                if vid not in available:
                    continue
                index += 1
                jobs.append((speaker, style, text, intent, index))
    print(f"{len(jobs)} clips to render ({len(UTTERANCES)} utterances x "
          f"{len(SPEAKERS)} speakers x styles)")

    rows = []
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        futures = [
            pool.submit(render_one, out_dir, speaker, style, text, intent, idx)
            for speaker, style, text, intent, idx in jobs
        ]
        for n, future in enumerate(futures, start=1):
            row = future.result()
            rows.append(row)
            if n % 20 == 0 or n == len(futures):
                print(f"  {n}/{len(futures)} rendered")

    rows.sort(key=lambda r: r["file"])
    fields = ["file", "speaker", "style", "text", "intent", "voice_id", "bytes",
              "render_ms", "error"]
    with (out_dir / "manifest.csv").open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields, extrasaction="ignore")
        writer.writeheader()
        writer.writerows(rows)

    failures = [r for r in rows if r["error"]]
    print(f"rendered {len(rows) - len(failures)}/{len(rows)} clips; "
          f"{len(failures)} failures")
    if failures:
        for row in failures[:5]:
            print("  ", row["file"], row["error"])
        (out_dir / "render-failures.json").write_text(
            json.dumps(failures, indent=2), encoding="utf-8"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
