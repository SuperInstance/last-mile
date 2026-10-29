#!/usr/bin/env python3
"""How much memory the ears hold.

Three states, five samples each, of the GPU memory CUDA says is free and
of the RAM the unit's cgroup holds:

  * ears running
  * ears stopped
  * ears running again

The box is shared, so the free memory moves on its own; the median of
five samples is the number to read, and the "again" state is the check
that the first measurement was not a fluke.

Writes: evidence/memory.json, evidence/memory.md
"""
from __future__ import annotations

import json
import os
import statistics
import subprocess
import time

EVIDENCE = os.path.dirname(os.path.abspath(__file__))
SAMPLES = 5


def gpu_free_gib() -> float:
    code = "import torch;f,t=torch.cuda.mem_get_info();print('%.3f'%(f/2**30))"
    out = subprocess.run(
        ["/home/caseydigennaro/ml/.venv/bin/python", "-c", code],
        capture_output=True,
        text=True,
    ).stdout.strip()
    try:
        return float(out)
    except ValueError:
        return float("nan")


def gpu_used_mib() -> int:
    out = subprocess.run(
        [
            "nvidia-smi",
            "--query-compute-apps=used_gpu_memory",
            "--format=csv,noheader,nounits",
        ],
        capture_output=True,
        text=True,
    ).stdout
    total = 0
    for line in out.splitlines():
        line = line.strip()
        if line:
            total += int(line)
    return total


def ears_gpu_mib() -> int:
    """The GPU memory the ears' own processes hold."""
    pids = subprocess.run(
        ["pgrep", "-f", "llama-server.*--port 8103"], capture_output=True, text=True
    ).stdout.split()
    out = subprocess.run(
        [
            "nvidia-smi",
            "--query-compute-apps=pid,used_gpu_memory",
            "--format=csv,noheader,nounits",
        ],
        capture_output=True,
        text=True,
    ).stdout
    total = 0
    for line in out.splitlines():
        if not line.strip():
            continue
        pid, mem = [part.strip() for part in line.split(",")]
        if int(pid) in {int(p) for p in pids}:
            total += int(mem)
    return total


def unit_memory() -> str:
    return subprocess.run(
        [
            "systemctl",
            "--user",
            "show",
            "lm-ears.service",
            "-p",
            "MemoryCurrent",
            "--value",
        ],
        capture_output=True,
        text=True,
    ).stdout.strip()


def sample(label: str) -> dict:
    frees = []
    used = []
    for _ in range(SAMPLES):
        frees.append(gpu_free_gib())
        used.append(gpu_used_mib())
        time.sleep(1.0)
    good = [value for value in frees if value == value]
    return {
        "label": label,
        "gpu_free_gib_samples": [round(value, 3) for value in frees],
        "gpu_free_gib_median": round(statistics.median(good), 3) if good else None,
        "gpu_used_mib_median": statistics.median(used),
        "ears_gpu_mib": ears_gpu_mib(),
        "unit_memory_current": unit_memory(),
    }


def systemctl(*args: str) -> None:
    subprocess.run(["systemctl", "--user", *args], capture_output=True, text=True)


def main() -> None:
    results = []

    results.append(sample("ears running"))
    systemctl("stop", "lm-ears.service")
    time.sleep(5)
    results.append(sample("ears stopped"))
    systemctl("start", "lm-ears.service")
    for _ in range(120):
        if subprocess.run(
            ["curl", "-fsS", "--noproxy", "*", "--max-time", "3",
             "http://127.0.0.1:8102/health"],
            capture_output=True,
        ).returncode == 0:
            break
        time.sleep(1)
    time.sleep(3)
    results.append(sample("ears running again"))

    with open(os.path.join(EVIDENCE, "memory.json"), "w") as handle:
        json.dump(results, handle, indent=1)

    running = [entry for entry in results if "running" in entry["label"]]
    stopped = [entry for entry in results if entry["label"] == "ears stopped"]
    delta = None
    if running and stopped:
        free_up = statistics.median(e["gpu_free_gib_median"] for e in running)
        free_down = stopped[0]["gpu_free_gib_median"]
        if free_up is not None and free_down is not None:
            delta = round(free_down - free_up, 3)

    lines = [
        "# What the ears hold",
        "",
        f"{SAMPLES} samples per state; the median of each is the number.",
        "",
        "| state | free GPU GiB (median) | all GPU used MiB (median) | ears GPU MiB | unit RAM (cgroup) |",
        "|---|---|---|---|---|",
    ]
    for entry in results:
        lines.append(
            f"| {entry['label']} | {entry['gpu_free_gib_median']} | "
            f"{entry['gpu_used_mib_median']} | {entry['ears_gpu_mib']} | "
            f"{entry['unit_memory_current']} |"
        )
    lines += [
        "",
        f"- The GPU memory the ears take off the box: **{delta} GiB** "
        f"(free with the ears minus free without them, median of the two "
        f"running states against the stopped one)."
        if delta is not None
        else "- The GPU memory the ears take off the box: not measurable this run.",
        f"- The ears' own processes hold "
        f"{max(e['ears_gpu_mib'] for e in results)} MiB of GPU memory.",
        "",
    ]
    with open(os.path.join(EVIDENCE, "memory.md"), "w") as handle:
        handle.write("\n".join(lines))
    print("\n".join(lines))


if __name__ == "__main__":
    main()
