#!/usr/bin/env python3
"""Capture the shipped `attention` gate's answers from the Thor's System One API.

The Thor is the local decision-model host (crates/lm-agents/src/sysone.rs).
The endpoint is POST /v1/systemone with {"model", "state", "questions"}; the
answer carries a probability per option under "probabilities" and a
"confidence" (crates/lm-agents/src/sysone.rs, parse_answers / parse_answer).

Fidelity notes:
 - The question text is the shipped default-harness `attention` gate VERBATIM
   from assets/harness/default/gates.toml.
 - The model names and ports are the shipped ones
   (crates/lm-agents/src/config.rs): d1-2b on 8094, clef-flash on 8096.
 - The states are facts-text in the wording crates/lm-agents/src/facts.rs
   produces ("You are standing on beacon 0, which your squad holds.").
 - Roster size varies because the harness ships "a harness for another roster
   names its own"; the default names agent0..agent7.
 - Repeats test whether the endpoint is deterministic.

Nothing here reads or runs Casey's other repositories.
"""

import json
import os
import sys
import time
import urllib.error
import urllib.request

THOR = os.environ.get("THOR_BASE", "http://192.168.0.78")
PORTS = {"d1-2b": 8094, "clef-flash": 8096}

INSTRUCTIONS = (
    "Who the line is for, in order: (1) If the whole squad has to act on it, your squad. "
    "(2) If one agent in particular needs it, that agent by number. "
    "(3) Only if the whole field must hear it, everybody."
)

SQUAD = "your squad (the doctrine's first branch: the whole squad has to act on it)"
ALL = "everybody who can hear (the doctrine's third branch: the whole field must hear it)"

STATES = {
    "holding_beacon": (
        "You are agent 0. You are standing on beacon 0, which your squad holds.\n"
        "Beacon 0 at column 4, row 2 is held by your squad; you are standing on it.\n"
        "Beacon 1 at column 9, row 7 is not held by your squad; it is the nearest beacon your squad does not hold.\n"
        "You have 2 allies within 4 cells of you. There are no enemies within 6 cells of you.\n"
        "Your squad is holding 1 beacon and has 2 agents alive."
    ),
    "retreat_danger": (
        "You are agent 3. You are at column 7, row 5.\n"
        "There are 3 enemies within 6 cells of you, and 2 of them can see you.\n"
        "You have 1 ally within 4 cells of you.\n"
        "Beacon 0 at column 2, row 2 is not held by your squad; it is the nearest beacon your squad does not hold.\n"
        "Your squad is holding 0 beacons and has 4 agents alive."
    ),
    "contested": (
        "You are agent 1. You are at column 5, row 5.\n"
        "Beacon 0 at column 4, row 2 is held by the other squad.\n"
        "Beacon 1 at column 9, row 7 is held by your squad.\n"
        "You have 2 allies within 4 cells of you. There are 1 enemies within 6 cells of you, and 1 of them can see you.\n"
        "Your squad is holding 1 beacon and has 3 agents alive."
    ),
    "scout_alone": (
        "You are agent 5. You are at column 11, row 3.\n"
        "You have 0 allies within 4 cells of you. There are no enemies within 6 cells of you.\n"
        "Beacon 2 at column 13, row 1 is not held by your squad; it is the nearest beacon your squad does not hold.\n"
        "Your squad is holding 1 beacon and has 5 agents alive."
    ),
    "last_standing": (
        "You are agent 7. You are at column 1, row 9.\n"
        "You have 0 allies within 4 cells of you. There are 2 enemies within 6 cells of you, and 2 of them can see you.\n"
        "Your squad is holding 0 beacons and has 1 agent alive.\n"
        "Your squad is losing."
    ),
}

ROSTERS = {"r8": list(range(8)), "r4": list(range(4)), "r2": [0, 1]}


def question_for(roster):
    criteria = {"squad": SQUAD, "all": ALL}
    for i in roster:
        criteria[f"agent{i}"] = (
            f"agent {i} in particular (the doctrine's second branch: that agent in particular needs it)")
    return {"type": "choice", "instructions": INSTRUCTIONS, "criteria": criteria}


def ask_with(model, port, state, question, timeout=90):
    body = json.dumps({"model": model, "state": state,
                       "questions": {"attention": question}}).encode()
    req = urllib.request.Request(
        f"{THOR}:{port}/v1/systemone", data=body,
        headers={"Content-Type": "application/json"}, method="POST")
    t0 = time.time()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read().decode()
        j = json.loads(raw)
        a = j.get("answers", {}).get("attention", {})
        return {"ok": True, "ms": int((time.time() - t0) * 1000),
                "choice": a.get("choice"), "probs": a.get("probabilities", {}),
                "confidence": a.get("confidence"), "usage": j.get("usage", {})}
    except Exception as e:
        b = ""
        if isinstance(e, urllib.error.HTTPError):
            b = e.read().decode()[:400]
        return {"ok": False, "ms": int((time.time() - t0) * 1000), "error": repr(e), "body": b}


def main():
    out_path = sys.argv[1] if len(sys.argv) > 1 else "attention_capture.json"
    out = []
    for rname, roster in ROSTERS.items():
        q = question_for(roster)
        for sname, state in STATES.items():
            for model, port in PORTS.items():
                for rep in range(2):
                    res = ask_with(model, port, state, q)
                    rec = {"roster": rname, "roster_size": len(roster),
                           "situation": sname, "model": model, "rep": rep}
                    rec.update(res)
                    out.append(rec)
                    print(json.dumps({k: rec.get(k) for k in
                                      ("roster", "situation", "model", "rep", "ok",
                                       "choice", "confidence", "ms")}), flush=True)
    with open(out_path, "w") as f:
        json.dump(out, f, indent=1)
    print("WROTE", out_path, len(out))


if __name__ == "__main__":
    main()
