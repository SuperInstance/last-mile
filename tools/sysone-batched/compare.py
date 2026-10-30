#!/usr/bin/env python3
"""Compare two System One servers on the same packs.

Reads packs from a JSONL file (one pack per line, the shape the harness
writes: {"pack": ..., "state": ..., "questions": {...}}) and asks both
servers the same pack, then reports:

  * the agreement of the chosen option, per question;
  * the largest difference of the reported probability, per question;
  * the round trip of each server.

usage: compare.py PORT_A PORT_B [PACKS.jsonl] [LIMIT]
"""
import json
import sys
import time
import urllib.request

PORT_A = int(sys.argv[1]) if len(sys.argv) > 1 else 8094
PORT_B = int(sys.argv[2]) if len(sys.argv) > 2 else 8104
PACKS = sys.argv[3] if len(sys.argv) > 3 else "/tmp/packs.jsonl"
LIMIT = int(sys.argv[4]) if len(sys.argv) > 4 else 500

OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def ask(port, state, questions, timeout=120):
    body = json.dumps({
        "model": "d1-2B",
        "state": state,
        "questions": questions,
    }).encode()
    req = urllib.request.Request(
        f"http://127.0.0.1:{port}/v1/systemone",
        data=body,
        headers={"Content-Type": "application/json"},
    )
    t0 = time.perf_counter()
    with OPENER.open(req, timeout=timeout) as r:
        payload = json.loads(r.read())
    return (time.perf_counter() - t0) * 1000.0, payload


def main():
    packs = []
    with open(PACKS) as f:
        for line in f:
            line = line.strip()
            if line:
                packs.append(json.loads(line))
    packs = packs[:LIMIT]
    if not packs:
        print("no packs")
        return 1

    n_q = 0
    n_same = 0
    worst = 0.0
    worst_where = ""
    n_bad = 0
    lat_a = []
    lat_b = []

    for i, pack in enumerate(packs):
        ms_a, a = ask(PORT_A, pack["state"], pack["questions"])
        ms_b, b = ask(PORT_B, pack["state"], pack["questions"])
        lat_a.append(ms_a)
        lat_b.append(ms_b)

        aa = a.get("answers", {})
        bb = b.get("answers", {})
        for qid, ans_a in aa.items():
            ans_b = bb.get(qid)
            if ans_b is None:
                n_bad += 1
                continue
            n_q += 1
            key_a = ans_a.get("choice", ans_a.get("score", ans_a.get("noul")))
            key_b = ans_b.get("choice", ans_b.get("score", ans_b.get("noul")))
            if key_a == key_b:
                n_same += 1
            else:
                n_bad += 1
            pa = ans_a.get("probabilities") or {}
            pb = ans_b.get("probabilities") or {}
            for k in pa:
                if k in pb:
                    d = abs(pa[k] - pb[k])
                    if d > worst:
                        worst = d
                        worst_where = f"pack {i} question {qid} option {k}"

    def pct(xs, q):
        xs = sorted(xs)
        if not xs:
            return 0.0
        return xs[min(len(xs) - 1, int((len(xs) - 1) * q))]

    print(f"packs: {len(packs)}")
    print(f"questions compared: {n_q}")
    print(f"same answer: {n_same} ({100.0 * n_same / max(1, n_q):.3f}%)")
    print(f"different or missing: {n_bad}")
    print(f"largest probability difference: {worst:.9f} {worst_where}")
    print(f"port {PORT_A}: p50 {pct(lat_a, 0.5):.1f} ms, p95 {pct(lat_a, 0.95):.1f} ms")
    print(f"port {PORT_B}: p50 {pct(lat_b, 0.5):.1f} ms, p95 {pct(lat_b, 0.95):.1f} ms")
    return 0


if __name__ == "__main__":
    sys.exit(main())
