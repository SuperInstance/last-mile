#!/usr/bin/env python3
"""What the ears cost the box.

Four phases, the same System One pack to the d1-2B on :8094 in each:

    A  ears running, idle
    B  ears stopped            (the box without the ears)
    C  ears running, idle      (again, to catch drift)
    D  ears running and busy   (the ears transcribing while the d1-2B answers)

and the memory the ears hold, measured with and without them.

The ears are this repo's own unit (lm-ears), so stopping and starting it
here touches nothing else on the box. The d1-2B, the other d1 models,
skald and the ports 8097-8100 are only ever read from.

Writes: evidence/interference.json and evidence/interference.md
"""
from __future__ import annotations

import json
import os
import statistics
import subprocess
import threading
import time
import urllib.request

EVIDENCE = os.path.dirname(os.path.abspath(__file__))
D1_PORT = 8094
EARS_PORT = 8102
N_REQUESTS = 30

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
BODY = json.dumps({"model": "d1-2B", "state": STATE, "questions": QUESTIONS}).encode()
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def ask_d1() -> float:
    """One System One pack; returns the round trip in ms."""
    request = urllib.request.Request(
        f"http://127.0.0.1:{D1_PORT}/v1/systemone",
        data=BODY,
        headers={"Content-Type": "application/json"},
    )
    started = time.perf_counter()
    with OPENER.open(request, timeout=60) as response:
        response.read()
    return (time.perf_counter() - started) * 1000.0


def phase(label: str, n: int = N_REQUESTS) -> dict:
    """N sequential asks; returns the latency summary."""
    latencies = []
    for _ in range(n):
        latencies.append(ask_d1())
        time.sleep(0.05)
    ordered = sorted(latencies)
    p90 = ordered[min(len(ordered) - 1, int(round(0.90 * (len(ordered) - 1))))]
    return {
        "label": label,
        "n": n,
        "median_ms": round(statistics.median(latencies), 1),
        "mean_ms": round(statistics.mean(latencies), 1),
        "p90_ms": round(p90, 1),
        "min_ms": round(min(latencies), 1),
        "max_ms": round(max(latencies), 1),
        "raw_ms": [round(value, 1) for value in latencies],
    }


def systemctl(*args: str) -> str:
    return subprocess.run(
        ["systemctl", "--user", *args],
        capture_output=True,
        text=True,
    ).stdout.strip()


def ears_pids() -> list[int]:
    """The pids of the unit's processes."""
    out = subprocess.run(
        ["systemctl", "--user", "show", "lm-ears.service", "-p", "MainPID", "--value"],
        capture_output=True,
        text=True,
    ).stdout.strip()
    pids = []
    if out and out != "0":
        pids.append(int(out))
    listing = subprocess.run(
        ["pgrep", "-f", "llama-server.*--port 8103"], capture_output=True, text=True
    ).stdout.split()
    pids.extend(int(value) for value in listing)
    return sorted(set(pids))


def gpu_memory_by_pid() -> dict:
    """The GPU memory each pid holds, in MiB."""
    out = subprocess.run(
        [
            "nvidia-smi",
            "--query-compute-apps=pid,used_gpu_memory",
            "--format=csv,noheader,nounits",
        ],
        capture_output=True,
        text=True,
    ).stdout.strip()
    usage: dict[int, int] = {}
    for line in out.splitlines():
        if not line.strip():
            continue
        pid, mem = [part.strip() for part in line.split(",")]
        usage[int(pid)] = int(mem)
    return usage


def gpu_free_gib() -> float:
    """The free GPU memory, the way CUDA sees it."""
    code = (
        "import torch;"
        "f,t=torch.cuda.mem_get_info();"
        "print('%.2f'%(f/2**30))"
    )
    out = subprocess.run(
        ["/home/caseydigennaro/ml/.venv/bin/python", "-c", code],
        capture_output=True,
        text=True,
    ).stdout.strip()
    try:
        return float(out)
    except ValueError:
        return float("nan")


def memory_snapshot(label: str) -> dict:
    """The memory the ears hold, and the box's free GPU memory."""
    pids = ears_pids()
    usage = gpu_memory_by_pid()
    ears_gpu = sum(usage.get(pid, 0) for pid in pids)
    rss = {}
    for pid in pids:
        try:
            with open(f"/proc/{pid}/status") as handle:
                for line in handle:
                    if line.startswith("VmRSS:"):
                        rss[str(pid)] = int(line.split()[1]) // 1024
                        break
        except OSError:
            continue
    unit_mem = systemctl("show", "lm-ears.service", "-p", "MemoryCurrent", "--value")
    return {
        "label": label,
        "ears_pids": pids,
        "ears_gpu_mib": ears_gpu,
        "ears_rss_mib": rss,
        "unit_memory_current": unit_mem,
        "gpu_free_gib": gpu_free_gib(),
        "total_gpu_used_mib": sum(usage.values()),
    }


class EarLoad:
    """Transcriptions hammering the ears while the d1-2B answers."""

    def __init__(self, threads: int = 4) -> None:
        self.threads = threads
        self.stop = threading.Event()
        self.count = 0
        self.workers: list[threading.Thread] = []
        self.wav = os.path.join(EVIDENCE, "wavs", "_load.wav")

    def _one(self) -> None:
        while not self.stop.is_set():
            try:
                with open(self.wav, "rb") as handle:
                    data = handle.read()
                request = urllib.request.Request(
                    f"http://127.0.0.1:{EARS_PORT}/transcribe?language=en",
                    data=data,
                    headers={"Content-Type": "audio/wav"},
                )
                with OPENER.open(request, timeout=60) as response:
                    response.read()
                self.count += 1
            except Exception:  # noqa: BLE001 - the load is best effort
                time.sleep(0.2)

    def __enter__(self) -> "EarLoad":
        for _ in range(self.threads):
            worker = threading.Thread(target=self._one, daemon=True)
            worker.start()
            self.workers.append(worker)
        time.sleep(1.0)
        return self

    def __exit__(self, *_exc: object) -> None:
        self.stop.set()
        for worker in self.workers:
            worker.join(timeout=5)


def main() -> None:
    results = {"phases": [], "memory": []}

    print("A: the ears running, idle")
    results["memory"].append(memory_snapshot("ears running"))
    results["phases"].append(phase("A ears running, idle"))

    print("B: the ears stopped")
    systemctl("stop", "lm-ears.service")
    time.sleep(4)
    results["memory"].append(memory_snapshot("ears stopped"))
    results["phases"].append(phase("B ears stopped"))

    print("C: the ears running again, idle")
    systemctl("start", "lm-ears.service")
    for _ in range(120):
        try:
            with OPENER.open(
                urllib.request.Request(f"http://127.0.0.1:{EARS_PORT}/health"), timeout=3
            ) as response:
                if response.status == 200:
                    break
        except Exception:  # noqa: BLE001
            pass
        time.sleep(1)
    time.sleep(2)
    results["memory"].append(memory_snapshot("ears running again"))
    results["phases"].append(phase("C ears running again, idle"))

    print("D: the ears running and busy")
    with EarLoad() as load:
        results["phases"].append(phase("D ears running and busy"))
        results["ear_transcriptions_during_D"] = load.count
    results["memory"].append(memory_snapshot("after the load"))

    with open(os.path.join(EVIDENCE, "interference.json"), "w") as handle:
        json.dump(results, handle, indent=1)

    # the table
    lines = [
        "# What the ears cost the box",
        "",
        f"d1-2B on :{D1_PORT}, {N_REQUESTS} sequential System One packs per phase,",
        "the same pack every time.",
        "",
        "| phase | median ms | mean ms | p90 ms | min ms | max ms |",
        "|---|---|---|---|---|---|",
    ]
    for entry in results["phases"]:
        lines.append(
            f"| {entry['label']} | {entry['median_ms']} | {entry['mean_ms']} | "
            f"{entry['p90_ms']} | {entry['min_ms']} | {entry['max_ms']} |"
        )
    lines += [
        "",
        "| when | ears GPU MiB | ears RSS MiB (pid) | free GPU GiB | all GPU used MiB |",
        "|---|---|---|---|---|",
    ]
    for entry in results["memory"]:
        rss = ", ".join(f"{pid}:{mib}" for pid, mib in entry["ears_rss_mib"].items())
        lines.append(
            f"| {entry['label']} | {entry['ears_gpu_mib']} | {rss or '-'} | "
            f"{entry['gpu_free_gib']} | {entry['total_gpu_used_mib']} |"
        )
    lines.append("")
    lines.append(
        f"Transcriptions the ears did while the d1-2B was measured in phase D: "
        f"{results.get('ear_transcriptions_during_D')}"
    )
    lines.append("")
    with open(os.path.join(EVIDENCE, "interference.md"), "w") as handle:
        handle.write("\n".join(lines))
    print("\n".join(lines))


if __name__ == "__main__":
    main()
