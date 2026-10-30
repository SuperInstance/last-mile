#!/usr/bin/env python3
"""The latency of two System One servers, on the same packs.

For the move pack and for the full eleven-question pack, at one, four and
eight requests in flight, it reports the p50 and the p95 of the round trip and
the throughput of each server.

The two servers are asked the same pack at the same time, in turn, so they
see the same load of the box. The box is shared with other seats, so the load
average and the GPU use are sampled while the measurement runs and reported
with it: absolute numbers move with the load, the ratio of the two servers is
what counts.

usage: bench.py PORT_A PORT_B [PACKS.jsonl] [ROUNDS]
"""
import json
import subprocess
import sys
import threading
import time
import urllib.request

PORT_A = int(sys.argv[1]) if len(sys.argv) > 1 else 8094
PORT_B = int(sys.argv[2]) if len(sys.argv) > 2 else 8104
PACKS = sys.argv[3] if len(sys.argv) > 3 else "/tmp/packs-8104.jsonl"
ROUNDS = int(sys.argv[4]) if len(sys.argv) > 4 else 12

OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def ask(port, state, questions, timeout=180):
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
        r.read()
    return (time.perf_counter() - t0) * 1000.0


def burst(port, state, questions, n):
    """Fire n requests at once and return their round trips."""
    out = [0.0] * n
    barrier = threading.Barrier(n)

    def one(i):
        barrier.wait()
        out[i] = ask(port, state, questions)

    threads = [threading.Thread(target=one, args=(i,)) for i in range(n)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    return out


def pct(xs, q):
    xs = sorted(xs)
    if not xs:
        return 0.0
    return xs[min(len(xs) - 1, int((len(xs) - 1) * q))]


def gpu_util():
    """The GPU use, as nvidia-smi reports it; None when it cannot be read."""
    try:
        out = subprocess.run(
            ["nvidia-smi", "--query-gpu=utilization.gpu", "--format=csv,noheader"],
            capture_output=True, text=True, timeout=5,
        )
        return float(out.stdout.strip().split("\n")[0].rstrip("% "))
    except Exception:
        return None


def load_avg():
    """The one-minute load average; None when it cannot be read."""
    try:
        with open("/proc/loadavg") as f:
            return float(f.read().split()[0])
    except Exception:
        return None


class Load:
    """Samples the load average and the GPU use while the bench runs.

    The box is shared, and the load on it moves: a number without the load it
    was taken under is noise. This thread samples both once a second, and the
    bench prints them with every point.
    """

    def __init__(self):
        self.loads = []
        self.gpus = []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True)

    def _run(self):
        while not self.stop.is_set():
            load = load_avg()
            gpu = gpu_util()
            if load is not None:
                self.loads.append(load)
            if gpu is not None:
                self.gpus.append(gpu)
            self.stop.wait(1.0)

    def __enter__(self):
        self.thread.start()
        return self

    def __exit__(self, *exc):
        self.stop.set()
        self.thread.join(timeout=3)
        return False

    def text(self):
        load = f"load {min(self.loads):.1f}-{max(self.loads):.1f}" if self.loads else "load n/a"
        gpu = f"gpu {min(self.gpus):.0f}-{max(self.gpus):.0f}%" if self.gpus else "gpu n/a"
        return f"{load}, {gpu}"


def measure_pair(port_a, port_b, state, questions, n, rounds):
    """Ask both servers, one round at a time, and take the round trips.

    The two servers are asked in turn and not in blocks: the box is shared
    with a live game, and the load on it moves. A block of A and then a block
    of B would measure the load of two different minutes.

    The wall clock is kept per server, so each server's requests/s is its own
    time under load, not the sum of both.
    """
    lat_a = []
    lat_b = []
    wall_a = 0.0
    wall_b = 0.0
    for _ in range(rounds):
        t0 = time.perf_counter()
        lat_a.extend(burst(port_a, state, questions, n))
        wall_a += time.perf_counter() - t0
        t0 = time.perf_counter()
        lat_b.extend(burst(port_b, state, questions, n))
        wall_b += time.perf_counter() - t0
    out = []
    for lat, wall in ((lat_a, wall_a), (lat_b, wall_b)):
        out.append({
            "p50": pct(lat, 0.5),
            "p95": pct(lat, 0.95),
            "min": min(lat),
            "max": max(lat),
            "rps": len(lat) / wall,
        })
    return out


def main():
    packs = []
    with open(PACKS) as f:
        for line in f:
            line = line.strip()
            if line:
                packs.append(json.loads(line))
    if not packs:
        print("no packs")
        return 1

    # the move pack: one question, the one the agent asks on every think
    move = None
    full = None
    for pack in packs:
        if move is None and len(pack["questions"]) <= 2:
            move = pack
        if full is None and len(pack["questions"]) >= 8:
            full = pack
    if move is None:
        move = packs[0]
    if full is None:
        full = packs[0]

    kinds = [("move pack", move), ("full pack", full)]
    print(f"ports: {PORT_A} and {PORT_B}, {ROUNDS} rounds each")
    for name, pack in kinds:
        nq = len(pack["questions"])
        print(f"\n== {name} ({nq} questions) ==")
        for n in (1, 4, 8):
            with Load() as load:
                a, b = measure_pair(PORT_A, PORT_B, pack["state"], pack["questions"], n, ROUNDS)
                line = load.text()
            print(
                f"  {n} in flight: "
                f"{PORT_A} p50 {a['p50']:7.1f} p95 {a['p95']:7.1f} ms "
                f"{a['rps']:5.2f} req/s   |   "
                f"{PORT_B} p50 {b['p50']:7.1f} p95 {b['p95']:7.1f} ms "
                f"{b['rps']:5.2f} req/s   |   "
                f"speed {a['p50'] / b['p50']:.2f}x p50, {a['p95'] / b['p95']:.2f}x p95   |   "
                f"while: {line}"
            )
    return 0


if __name__ == "__main__":
    sys.exit(main())
