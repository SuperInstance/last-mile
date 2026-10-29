"""Local System One client for the Thor's decision models. No auth, no keys (LAN only)."""
import json, os, time, urllib.request, urllib.error
URL = os.environ.get("THOR_URL", "http://192.168.0.78:8095")
MODEL = os.environ.get("THOR_MODEL", "d1-omni-600M")
def ask(state, questions, timeout=60):
    body = json.dumps({"model": MODEL, "state": state, "questions": questions}).encode()
    req = urllib.request.Request(URL + "/v1/systemone", data=body, method="POST", headers={"Content-Type": "application/json"})
    t = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read()), (time.perf_counter()-t)*1000
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode(errors="replace")[:300], (time.perf_counter()-t)*1000
    except Exception as e:
        return 0, str(e)[:300], (time.perf_counter()-t)*1000
