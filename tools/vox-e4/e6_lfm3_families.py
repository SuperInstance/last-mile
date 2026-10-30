"""E6(a): does each mark agree with LFM3-Audio's own answer?

For every family, LFM3-Audio is given the **audio** and asked the same closed
question the mark answers in words — "hushed, soft, level, strong, or
shouted?" for the dynamics, and so on. The two answers are then compared
clip by clip.

This is the inter-rater test: the marks are one rater (a deterministic
quantizer over measured signal features) and LFM3 is the other (an audio
model that has never seen the analyzer). They share no machinery, so where
they agree the word is probably describing the delivery rather than an
artifact of either one.

    python tools/vox-e4/e6_lfm3_families.py --marks evidence/vox-e6/marks.csv \
        --clip-root ~/lm-evidence/vox-e4 --workers 4
"""

from __future__ import annotations

import argparse
import base64
import csv
import json
import re
import sys
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

#: The LFM3 endpoint. :8102 is the ears proxy; :8103 is the model server
#: behind it. The proxy is restarted often enough under load that the
#: upstream is reachable directly.
LFM3 = "http://localhost:8102/v1/chat/completions"
LFM3_UPSTREAM = "http://localhost:8103/v1/chat/completions"

#: Appended to the prompt when the arm is run in `--direct` mode. It makes
#: LFM3 answer with the word instead of deliberating first, which is 3-10x
#: cheaper; what it measures is a snap judgement rather than a considered
#: one, so the two modes are tagged separately in the output.
DIRECT_SUFFIX = " Answer directly with the word, without explaining."

#: The closed question for each family, and the vocabulary of its answers.
#: The wording is the mark's own vocabulary — that is the point: the reader
#: is asked the question the format is written in.
FAMILY_QUESTIONS: dict[str, dict] = {
    "dynamics": {
        "prompt": ("Rate the volume of this voice delivery: hushed, soft, "
                   "level, strong, or shouted? One word only."),
        "options": ["hushed", "soft", "level", "strong", "shouted"],
    },
    "articulation": {
        "prompt": ("How is this voice delivery articulated: clipped, "
                   "pressed, even, or drawn? One word only."),
        "options": ["clipped", "pressed", "even", "drawn"],
    },
    "contour": {
        "prompt": ("How does the pitch move in this voice delivery: "
                   "falling, flat, lifting, rise-fall, or questioning? "
                   "One word only."),
        "options": ["falling", "flat", "lifting", "rise-fall", "questioning"],
    },
    "pace": {
        "prompt": ("What is the pace of this voice delivery: rushed, brisk, "
                   "regular, or measured? One word only."),
        "options": ["rushed", "brisk", "regular", "measured"],
    },
    "colour": {
        "prompt": ("What is the timbre of this voice delivery: dark, plain, "
                   "or bright? One word only."),
        "options": ["dark", "plain", "bright"],
    },
}


def post_json(url: str, payload: dict, timeout: float = 300.0) -> dict:
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url, data=data, headers={"Content-Type": "application/json"},
        method="POST")
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return json.loads(response.read())


def message_text(out: dict) -> tuple[str, str]:
    message = out["choices"][0]["message"]
    content = message.get("content") or ""
    if isinstance(content, list):
        content = " ".join(part.get("text", "") for part in content)
    return content.strip(), (message.get("reasoning_content") or "").strip()


def pick_option(text: str, options: list[str]) -> str:
    """The option the text names, preferring the last mention.

    Options that contain another option are masked out **first**: "rise-fall"
    contains "fall", and a word search over the raw text would find the
    "fall" inside it and answer "fall" for a clip whose answer was "rise-fall".
    """
    lowered = text.lower()
    masked = lowered
    for option in sorted(options, key=len, reverse=True):
        if len(option) < 5:
            continue
        masked = masked.replace(option, " " * len(option))
    best, best_pos = "", -1
    for option in options:
        for match in re.finditer(rf"\b{re.escape(option)}\b", masked):
            if match.start() >= best_pos:
                best, best_pos = option, match.start()
    return best


def ask(family: str, audio_b64: str, timeout: float = 180.0,
        url: str = LFM3, direct: bool = False) -> tuple[str, str, float]:
    """Ask LFM3 one family's question about one clip.

    LFM3 is a *reasoning* model: it thinks before it answers, and on this
    corpus the thinking runs to several hundred tokens. Two things follow.

    First, the answer is read with ``guided_json`` — a JSON schema whose
    ``answer`` field is an enum of the family's words. The schema constrains
    the *answer* only; the reasoning is left free, so the model still listens
    and thinks, and the answer it lands on is still one of the closed words.
    (A ``grammar`` would constrain the whole generation and force the answer
    at the first token, before the model has heard anything: that answers
    "hushed" to almost every clip and measures nothing.)

    Second, the token budget is generous (1024). A truncated reasoning trace
    has no answer in it, and scraping one out of a half-finished trace
    measures where the trace was cut, not what the model thought.
    """
    spec = FAMILY_QUESTIONS[family]
    prompt = spec["prompt"] + (DIRECT_SUFFIX if direct else "")
    schema = {
        "type": "object",
        "properties": {"answer": {"type": "string", "enum": list(spec["options"])}},
        "required": ["answer"],
        "additionalProperties": False,
    }
    payload = {
        "model": "LFM3-Audio-2B",
        "messages": [{"role": "user", "content": [
            {"type": "text", "text": prompt},
            {"type": "input_audio",
             "input_audio": {"data": audio_b64, "format": "wav"}},
        ]}],
        "max_tokens": 1024,
        "temperature": 0,
        "guided_json": schema,
    }
    started = time.perf_counter()
    answer, source, latency = "", "", 0.0
    # The proxy on :8102 wedges and restarts often enough that a run has to
    # be able to fail over to the model server behind it, and back again.
    candidates = [url] + [other for other in (LFM3, LFM3_UPSTREAM) if other != url]
    for attempt in range(3):
        target = candidates[attempt % len(candidates)]
        try:
            out = post_json(target, payload, timeout=timeout)
        except Exception as exc:  # noqa: BLE001
            # A refused connection or a hung proxy is a normal event here,
            # not a failure of the experiment: wait and try the next one.
            last = f"error: {exc}"
            time.sleep(5 * (attempt + 1))
            continue
        latency = (time.perf_counter() - started) * 1000.0
        choice = out["choices"][0].get("finish_reason", "")
        content, reasoning = message_text(out)
        answer = ""
        try:
            answer = json.loads(content).get("answer", "") or ""
        except (ValueError, AttributeError):
            answer = pick_option(content, spec["options"])
        if not answer:
            # The model finished but put the word in its reasoning rather
            # than in the JSON. That is still an answer - the same one E4's
            # harness read out of the trace - and it is only untrustworthy
            # when the trace was cut short.
            answer = pick_option(reasoning, spec["options"])
            if answer:
                source = "reasoning"
        if not answer:
            return "", f"no answer ({choice})", latency
        if choice != "stop":
            return "", f"truncated ({choice})", latency
        return answer, "guided", latency
    return "", last, latency


def pick_endpoint() -> str:
    """The proxy if it answers, otherwise the model server behind it.

    The proxy on :8102 is restarted underneath this work often enough that
    it is sometimes listening but wedged. The model server on :8103 is the
    same model behind the same OpenAI-shaped contract, so falling through
    to it costs nothing but a health check.
    """
    for candidate, health in ((LFM3, "http://localhost:8102/health"),
                              (LFM3_UPSTREAM, "http://localhost:8103/health")):
        try:
            with urllib.request.urlopen(health, timeout=15) as response:
                if response.status == 200:
                    return candidate
        except Exception:  # noqa: BLE001
            continue
    print("neither the proxy nor the upstream answered; using the proxy")
    return LFM3


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--marks", default="evidence/vox-e6/marks.csv")
    parser.add_argument("--clip-root", default=None)
    parser.add_argument("--out", default="evidence/vox-e6/lfm3_families.csv")
    parser.add_argument("--workers", type=int, default=4,
                        help="how many LFM3 requests may be in flight at once")
    parser.add_argument("--families", default="",
                        help="comma-separated subset of families to run")
    parser.add_argument("--resume", action="store_true",
                        help="keep the answers already in the output file")
    parser.add_argument("--limit", type=int, default=0,
                        help="use only the first N clips of the corpus")
    parser.add_argument("--direct", action="store_true",
                        help="ask for the word without explaining (3-10x "
                             "cheaper; a snap judgement rather than a "
                             "considered one)")
    parser.add_argument("--url", default="auto",
                        help="the LFM3 endpoint: 'auto' (the proxy if it "
                             "answers, otherwise the model server behind it), "
                             "or a full URL")
    args = parser.parse_args()

    rows = list(csv.DictReader(Path(args.marks).open(encoding="utf-8")))
    wanted = {f.strip() for f in args.families.split(",") if f.strip()}
    families = [f for f in FAMILY_QUESTIONS if not wanted or f in wanted]
    if args.limit:
        rows = rows[:args.limit]
    if args.url == "auto":
        args.url = pick_endpoint()
    print(f"endpoint: {args.url}")
    print(f"{len(rows)} clips x {len(families)} families = "
          f"{len(rows)*len(families)} LFM3 requests")
    print("(families are interleaved clip by clip, so a run that is cut short "
          "still gives every family the same clips)")

    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    done: dict[tuple[str, str], dict] = {}
    keep: list[dict] = []
    if args.resume and out_path.exists():
        want = "direct" if args.direct else "reasoned"
        for row in csv.DictReader(out_path.open(encoding="utf-8")):
            if not row.get("lfm3"):
                # A row that errored out because the model server was
                # restarting has to be asked again.
                continue
            style = row.get("prompt_style") or "reasoned"
            keep.append(row)
            if style == want:
                done[(row["file"], row["family"])] = row
        print(f"loaded {len(done)} existing answers")

    # Every answer already on disk is kept, whatever prompt style it was
    # asked with; the style is in the row, so the two can be told apart.
    results: list[dict] = keep
    fields = ["file", "family", "mark", "lfm3", "source", "prompt_style",
              "latency_ms", "error"]

    def flush() -> None:
        """Write what is done so far, so a long run is never lost."""
        with out_path.open("w", newline="", encoding="utf-8") as handle:
            writer = csv.DictWriter(handle, fieldnames=fields)
            writer.writeheader()
            writer.writerows(results)

    pool = ThreadPoolExecutor(max_workers=args.workers)
    try:
        # The clips are the outer loop and the families the inner one, so
        # that every family is asked about the same clips and a run that is
        # cut short still leaves all five families balanced.
        def one(job: tuple[dict, str]) -> dict:
            row, family = job
            path = row["file"]
            if args.clip_root:
                path = str(Path(args.clip_root) / path)
            with open(path, "rb") as handle:
                audio = base64.b64encode(handle.read()).decode()
            choice, source, latency = ask(family, audio, url=args.url,
                                          direct=args.direct)
            return {
                "file": row["file"],
                "family": family,
                "mark": row[family],
                "lfm3": choice,
                "source": source,
                "prompt_style": "direct" if args.direct else "reasoned",
                "latency_ms": round(latency, 1),
                "error": "" if choice else source,
            }

        jobs = [(row, family) for row in rows for family in families
                if (row["file"], family) not in done]
        if not jobs:
            print("nothing to do")
            return 0
        n = 0
        futures = [pool.submit(one, job) for job in jobs]
        for future in as_completed(futures):
            result = future.result()
            results.append(result)
            n += 1
            if n % 10 == 0 or n == len(jobs):
                print(f"  {n}/{len(jobs)}", flush=True)
                flush()
    finally:
        pool.shutdown(wait=True)

    flush()
    print(f"wrote {out_path} ({len(results)} answers)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
