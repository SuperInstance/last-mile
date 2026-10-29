#!/usr/bin/env python3
"""Append what the ears changed to ~/last-mile-ops.md.

Every change goes in the log with what it was, the command that made it
and the command that undoes it. The file is the box's record of what
last-mile's boxes have been made to do.
"""
from __future__ import annotations

import os
import subprocess
import time

OPS = os.path.expanduser("~/last-mile-ops.md")
STAMP = time.strftime("%Y-%m-%dT%H:%M:%S%z", time.localtime())

ENTRY = f"""
## {STAMP} lm-ears: LFM3-Audio-2B speech-to-text on :8102 (new unit, new port)

- what: the Thor grows ears. LFM3-Audio-2B (GGUF, official
  LiquidAI/LFM3-Audio-2B-GGUF, Q8_0) served by the llama.cpp build that
  already runs the d1 servers, behind a small proxy that shapes the
  answer into {{text, words, latency_ms}}.
- why: the brief -- last-mile needs a speech-to-text service on :8102 and
  a probe of whether the delivery of an order carries meaning the words
  do not.
- where:
  - the unit: ~/.config/systemd/user/lm-ears.service (a copy of
    lm-wt/ears/tools/ears/lm-ears.service)
  - the door: 0.0.0.0:8102, OpenAI-shaped /v1/audio/transcriptions and
    /v1/chat/completions (audio understanding)
  - the model server: 127.0.0.1:8103, localhost only, not a door
  - the weights: ~/models/LFM3-Audio-2B/LFM3-Audio-2B-Q8_0.gguf and
    mmproj-LFM3-Audio-2B-Q8_0.gguf
  - the code: lm-wt/ears/tools/ears/ (ears.py, lm-ears.sh, lm-ears.service)
  - the evidence: lm-wt/ears/evidence/ (the WAVs, the CSVs, the reports)
- command: bash ~/lm-wt/ears/tools/ears/bring-up-ears.sh
  (installs the unit, daemon-reload, enable, restart, waits for the door)
- revert:
  systemctl --user disable --now lm-ears.service
  rm ~/.config/systemd/user/lm-ears.service
  systemctl --user daemon-reload
  rm -rf ~/models/LFM3-Audio-2B
  (the files in lm-wt/ears/ are in git and go with the branch)
- nothing else on the box was touched: the d1 models (:8093-:8096),
  skald (:8000), :8097-:8100 and the other units were only read from.
- what it costs: about 4.2 GiB of GPU memory (free GPU 8.28 GiB without
  the ears, 4.0-4.16 GiB with them) and ~0.9 GiB of RAM in the unit's
  cgroup. The d1-2B on :8094 answered the same System One pack in 27.1 ms
  median with the ears idle, 26.1 ms with them stopped (noise), and
  32.3 ms median / 42.4 ms p90 with the ears transcribing at the same
  time.
"""


def main() -> None:
    if os.path.exists(OPS):
        with open(OPS) as handle:
            before = handle.read()
    else:
        before = ""
    with open(OPS, "a") as handle:
        handle.write(ENTRY)
    with open(OPS) as handle:
        after = handle.read()
    print(f"{OPS}: {len(before)} -> {len(after)} bytes")
    print(subprocess.run(["tail", "-6", OPS], capture_output=True, text=True).stdout)


if __name__ == "__main__":
    main()
