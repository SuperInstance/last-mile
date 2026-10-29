"""Minimal Jev client. Key read from ~/.config/last-mile/secrets.env, sent only to TypeSafe. Never printed."""
import json, os, sys, time, urllib.request, urllib.error
def key():
    for line in open(os.path.expanduser("~/.config/last-mile/secrets.env")):
        if line.startswith("TYPESAFE_API_KEY="): return line.split("=",1)[1].strip()
URL = os.environ.get("JEV_URL", "https://api.typesafe.ai/v1/systemone")
def ask(state, questions, model="jev-latest", timeout=30):
    body = json.dumps({"model": model, "state": state, "questions": questions}).encode()
    req = urllib.request.Request(URL, data=body, method="POST", headers={
        "Authorization": "Bearer " + key(), "Content-Type": "application/json"})
    t = time.perf_counter()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.status, json.loads(r.read()), (time.perf_counter()-t)*1000
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode(errors="replace")[:600], (time.perf_counter()-t)*1000
if __name__ == "__main__":
    st = {"scene": "Agent A at (3,4). Enemy at (5,4). A wall between columns 4 and 5 on row 4."}
    qs = {"enemy_visible": {"type": "boolean", "description": "Can agent A see the enemy?"},
          "action": {"type": "choice", "options": ["hide", "advance", "scout"], "description": "Best action for A"}}
    code, resp, ms = ask(st, qs)
    print("HTTP", code, f"{ms:.0f} ms"); print(json.dumps(resp, indent=1)[:1500] if not isinstance(resp, str) else resp)
