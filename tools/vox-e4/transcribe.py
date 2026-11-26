"""Transcribe every clip in the E4 dataset with LFM3 (the Thor's ears).

The words channel has to be measured, not assumed: this writes what LFM3
actually heard, per clip, with its latency, so the evaluation can use the
real transcript instead of the text that was rendered.

    python tools/vox-e4/transcribe.py --dataset evidence/vox-e4
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import re
import time
import urllib.request


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
    return " ".join(re.findall(r"[a-z0-9']+", text.lower()))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dataset", default="evidence/vox-e4")
    args = parser.parse_args()

    dataset = args.dataset
    manifest = list(csv.DictReader(open(f"{dataset}/manifest.csv", encoding="utf-8")))
    manifest = [row for row in manifest if not row.get("error")]
    print(f"transcribing {len(manifest)} clips")

    rows = []
    started = time.perf_counter()
    for n, row in enumerate(manifest, start=1):
        try:
            result = post_multipart(
                "http://localhost:8102/v1/audio/transcriptions",
                row["file"],
                {"model": "LFM3-Audio-2B"},
            )
            heard = (result.get("text") or "").strip()
            latency = result.get("latency_ms")
            error = ""
        except Exception as exc:  # noqa: BLE001
            heard, latency, error = "", None, str(exc)
        truth = row["text"]
        rows.append({
            "file": row["file"],
            "speaker": row["speaker"],
            "style": row["style"],
            "intent": row["intent"],
            "rendered_text": truth,
            "heard_text": heard,
            "exact": int(normalise(heard) == normalise(truth)),
            "latency_ms": latency if latency is not None else "",
            "error": error,
        })
        if n % 40 == 0 or n == len(manifest):
            print(f"  {n}/{len(manifest)} transcribed")

    fields = list(rows[0].keys())
    with open(f"{dataset}/transcripts.csv", "w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=fields)
        writer.writeheader()
        writer.writerows(rows)

    exact = sum(r["exact"] for r in rows)
    latencies = [float(r["latency_ms"]) for r in rows if r["latency_ms"] != ""]
    elapsed = time.perf_counter() - started
    print(f"exact transcripts: {exact}/{len(rows)} ({exact/max(1,len(rows))*100:.1f}%)")
    if latencies:
        latencies.sort()
        print(f"ASR latency: median {latencies[len(latencies)//2]:.0f} ms, "
              f"p95 {latencies[int(len(latencies)*0.95)]:.0f} ms")
    print(f"total {elapsed:.1f}s -> {dataset}/transcripts.csv")

    by_style: dict[str, list[int]] = {}
    for row in rows:
        by_style.setdefault(row["style"], []).append(row["exact"])
    for style in sorted(by_style):
        values = by_style[style]
        print(f"  {style:8s} exact {sum(values)}/{len(values)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
