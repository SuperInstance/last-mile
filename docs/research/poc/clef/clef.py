"""Clef via a local `wrangler dev --remote` Worker proxy (no keys here; auth is wrangler's)."""
import json, os, time, urllib.request, urllib.error
URL = os.environ.get("CLEF_URL", "http://localhost:8797")
def ask(state, questions, model="@cf/cloudflare/clef-flash", timeout=60):
    body = json.dumps({"model": model, "input": {"state": state, "questions": questions}}).encode()
    req = urllib.request.Request(URL, data=body, method="POST", headers={"Content-Type": "application/json", "User-Agent": "lm-e4/0.1 curl/8"})
    t = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            j = json.loads(r.read()); return (200 if "out" in j else 500), j.get("out", j), (time.perf_counter()-t)*1000
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode(errors="replace")[:400], (time.perf_counter()-t)*1000
