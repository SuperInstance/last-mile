#!/usr/bin/env python3
"""One audio-understanding question, to see what the shape is."""
import base64
import json
import sys
import urllib.request

wav = sys.argv[1]
question = sys.argv[2] if len(sys.argv) > 2 else "How urgent does the speaker sound? Answer with exactly one word: calm, ordinary, or urgent."

with open(wav, "rb") as handle:
    data = base64.b64encode(handle.read()).decode()

body = {
    "model": "LFM3-Audio-2B",
    "messages": [
        {
            "role": "system",
            "content": "Answer questions about the provided audio clip. Base every answer only on what can be heard.",
        },
        {
            "role": "user",
            "content": [
                {"type": "input_audio", "input_audio": {"data": data, "format": "wav"}},
                {"type": "text", "text": question},
            ],
        },
    ],
    "temperature": 0,
    "max_tokens": 32,
}
request = urllib.request.Request(
    "http://127.0.0.1:8102/v1/chat/completions",
    data=json.dumps(body).encode(),
    headers={"Content-Type": "application/json"},
)
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
with opener.open(request, timeout=120) as response:
    print(json.dumps(json.loads(response.read()), indent=1)[:1500])
