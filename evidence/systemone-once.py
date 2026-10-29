#!/usr/bin/env python3
"""One System One pack to :8094, the way last-mile sends it."""
import json
import sys
import time
import urllib.request

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8094
MODEL = sys.argv[2] if len(sys.argv) > 2 else "d1-2B"

STATE = (
    "Agent A is at column 3, row 4. The beacon B is at column 7, row 4. "
    "A walking path to the beacon exists and is 4 steps long. "
    "No enemies are on the map."
)
QUESTIONS = {
    "action": {
        "type": "choice",
        "instructions": "What should agent A do next?",
        "criteria": {
            "hide": "an enemy can see A",
            "advance": "no enemy is visible and the beacon is close",
            "scout": "nothing is known",
        },
    }
}

body = json.dumps({"model": MODEL, "state": STATE, "questions": QUESTIONS}).encode()
req = urllib.request.Request(
    f"http://127.0.0.1:{PORT}/v1/systemone",
    data=body,
    headers={"Content-Type": "application/json"},
)
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
started = time.perf_counter()
with opener.open(req, timeout=60) as response:
    payload = response.read()
latency = (time.perf_counter() - started) * 1000.0
print(f"status ok, {latency:.1f} ms")
print(payload[:600].decode("utf-8", "replace"))
