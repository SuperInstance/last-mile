# MIG on the Thor: what it supports, what it costs, and a maintenance window (2026-10-29)

Why this doc: three independent findings point at the same wall. GPU contention
raised decision refusals 8 → 24 while the realizer worked; d1-2B move decisions
went from 164 ms (quiet box, D74) to 1.1–1.8 s under concurrent studio load
([12-thor-models](12-thor-models.md), the live-lane tables in
[batched_sysone](../../../crates/lm-agents/tests/batched_sysone.md)); and the
batched System One server (:8104) turns its 6.2x model-time overlap into latency
only when the decision model has GPU headroom. `nvidia-smi` on this Thor reports
`mig.mode.current = Disabled` with `Pending: Disabled`; the box runs JetPack
7.2.1 / L4T R39.2.1, driver 595.78, CUDA 13.2, kernel 6.8.12-1021-tegra.

Everything in this doc was researched **without enabling MIG, without resetting
the GPU, and without stopping any service**. All local numbers are read-only
probes taken 2026-10-29 ~22:38. Every claim about NVIDIA behaviour carries a
source link; what I could not confirm is listed in §9.

---

## 1. What the Thor's MIG actually supports

MIG on Jetson arrived in JetPack 7.2 / L4T r39.2 as a **technology preview** on
Thor T5000: the r39.2.0 GA release notes say "MIG is a technology preview
feature. Support for MIG is still a work in progress", and "MIG on Jetson AGX
Thor is recommended only for profiles 78 and 83 with this initial release"
([r39.2 release notes, PDF](https://docs.nvidia.com/jetson/archives/r39.2/ReleaseNotes/Jetson_Linux_Release_Notes_r39.2.pdf)).
JetPack 7.0/7.1 had the hardware but no software: NVIDIA staff on the forums —
"It is not supported on Jetpack 7.0GA. We are planning it and will update"
([forum 344978](https://forums.developer.nvidia.com/t/how-to-use-mig-technology-to-divide-computing-units-in-thor/344978)).
This box is on 7.2.1, which has it.

The profile table the r39.2 docs show for `nvidia-smi mig -lgip` (with MIG
enabled):

| Profile | ID | SMs | DEC/ENC/CE/JPEG/OFA | Memory | Note |
|---|---|---|---|---|---|
| MIG 1g.0gb+me | **78** | 6 | 1/1/1/1/1 | **0.00 GiB** | recommended; carries the media engines |
| MIG 1g.0gb | 80 | 6 | 1/1/1/1/0 | 0.00 GiB | **hangs** (see §8) |
| MIG 1g.0gb+gfx | 81 | 6 | 1/1/1/1/0 | 0.00 GiB | **hangs** (see §8) |
| MIG 2g.0gb | 82 | 12 | 1/1/1/1/0 | 0.00 GiB | works, but not the recommendation |
| MIG 2g.0gb+gfx | **83** | 12 | 1/1/1/1/0 | 0.00 GiB | recommended; hosts graphics |
| MIG 3g.0gb | 0 | 20 | 2/2/2/1/1 | 0.00 GiB | the whole GPU as one instance — no split |
| MIG 3g.0gb+gfx | 32 | 20 | 2/2/2/1/1 | 0.00 GiB | same |

Source: [Jetson r39.2 MIG guide](https://docs.nvidia.com/jetson/archives/r39.2/DeveloperGuide/SD/MiG.html).
The GPU is 20 SMs (10 TPCs — the AGX Thor spec's "MIG with 10 TPCs", quoted in
[forum 344978](https://forums.developer.nvidia.com/t/how-to-use-mig-technology-to-divide-computing-units-in-thor/344978)),
so the recommended pair **83 + 78** splits the GPU **12 SMs (60%) + 6 SMs
(30%), with 2 SMs (10%) unallocated**. A user on r39.2.0 measured the 2g
profiles at 118–127 TFLOP/s while 1g profiles hung
([forum 384828](https://forums.developer.nvidia.com/t/agx-thor-mig-1g-instances-enumerate-as-cuda-devices-but-hang-on-the-first-kernel/384828)).

**Memory is not partitioned.** Every profile shows `Memory 0.00 GiB` and
`P2P No`; per-process GPU memory reports as 0 MiB under MIG and NVIDIA staff
call that "expected"
([r39.2 MIG guide](https://docs.nvidia.com/jetson/archives/r39.2/DeveloperGuide/SD/MiG.html),
[forum 383262](https://forums.developer.nvidia.com/t/trouble-starting-docker-container-with-cuda-after-enabling-mig/383262)).
On the Thor, MIG isolates **SMs and the copy/decode/encode engines — it does
not carve the 122 GiB of unified LPDDR5x, and it does not partition memory
bandwidth**. Both instances allocate from the same physical pool. This is the
single most important fact for us: MIG here buys *latency isolation of
compute*, not memory capacity isolation. A studio job that eats unified memory
can still OOM the runtime slice; what it can no longer do is starve the runtime
slice of SMs.

**Within one instance, the device is a normal CUDA device**: streams, cuBLAS,
TensorRT all work — a user ran "a TensorRT service on 78 and a 3B PyTorch
model on 83" side by side after the fix
([forum 384828](https://forums.developer.nvidia.com/t/agx-thor-mig-1g-instances-enumerate-as-cuda-devices-but-hang-on-the-first-kernel/384828)).
The batched server's pool of per-context CUDA streams
([batched_sysone §2](../../../crates/lm-agents/tests/batched_sysone.md))
needs nothing special inside an instance. One process addresses one compute
instance — `CUDA_VISIBLE_DEVICES` accepts `MIG-<uuid>` values, at most one per
GPU instance ([MIG device names](https://docs.nvidia.com/datacenter/tesla/mig-user-guide/latest/mig-device-names.html)).
llama.cpp itself has no published Thor-MIG report that I could find — it is
plain CUDA with no NVDEC/VIC dependency, so nothing in its stack is excluded,
but **mark it untested until the window's first verification step passes**
(§5 step 8).

**The Riva container is the fragile one.** Docker + MIG on exactly this stack
(JetPack 7.2.1, driver 595.78, L4T R39.2.1, kernel 6.8.12-1021-tegra) is a
NVIDIA-acknowledged known issue: MIG devices on the host, "CUDA failed to
initialize" / "No CUDA-capable device is detected" in the container, and MIG
UUIDs rejected by `NVIDIA_VISIBLE_DEVICES` ("unsupported device id: MIG-…",
a CDI/CSV-mode limitation). Working workarounds, confirmed by users: run with
`--privileged`, or pass the MIG device explicitly —
`--runtime=nvidia -e NVIDIA_VISIBLE_DEVICES=0 -e CUDA_VISIBLE_DEVICES=MIG-<uuid>
--device=/dev/nvidia-caps/nvidia-cap12 --device=/dev/nvidia-caps/nvidia-cap13`
(the cap numbers must match the instance; mismatched caps reproduce the
initialization errors). NVIDIA: "our internal team is working on the official
fix" ([forum 380864](https://forums.developer.nvidia.com/t/can-not-run-mig-in-docker-container-on-jetson-thor/380864),
[forum 383262](https://forums.developer.nvidia.com/t/trouble-starting-docker-container-with-cuda-after-enabling-mig/383262)).
The datacenter `--gpus "device=MIG-…"` syntax is **not confirmed to work under
L4T's nvidia-container-runtime** — assume the env-var + cap-node form.

**Reboot behaviour**: MIG *mode* persists across reboots; the created GPU /
compute instances **do not** — "the created MIG devices are not persistent
across system reboots… the user or system administrator needs to recreate the
desired MIG configuration"
([MIG getting started](https://docs.nvidia.com/datacenter/tesla/mig-user-guide/latest/getting-started-with-mig.html);
same observation in [NVIDIA/gpu-operator#343](https://github.com/NVIDIA/gpu-operator/issues/343)).
That is the datacenter-driver statement; the Jetson r39.2 MIG guide does not
discuss reboot at all, so the Jetson-specific behaviour is **unconfirmed** —
which is why §5 installs a boot-time re-creation unit and tests a reboot inside
the window rather than trusting the docs.

**Graphics**: profiles with `+gfx` host the desktop. The r39.2 guide's demo
enables MIG with the display stopped, creates 83 + 78, then starts gdm3 — and
Xorg, gnome-shell and vkcube all land on the gfx instance
([r39.2 MIG guide](https://docs.nvidia.com/jetson/archives/r39.2/DeveloperGuide/SD/MiG.html)).
So the GNOME desktop (148 MiB tonight) can live on the same instance as the
runtime slice.

On this box, with MIG disabled, `nvidia-smi mig -lgip` runs but prints an
**empty profile table** (measured tonight) — profiles only enumerate once MIG
is enabled. Don't pre-flight profile availability with `-lgip` while disabled;
preflight with the modprobe-file check in §5 instead.

---

## 2. The services, measured tonight (2026-10-29 ~22:38)

Read-only probes: `nvidia-smi`, `ps`, `ss -ltnp`, `~/lm-services/last-mile.sh
status`, `tegrastats`. All five last-mile units up; the studio load (Glimmer +
Qwen3-Coder) is loaded — this is the "tonight's concurrent load" the brief
describes, caught in the act:

| Service | Port | Unit / holder | GPU MiB | Slice |
|---|---|---|---|---|
| d1-2B (System One, 8 slots) | 8094 | `d1-2b.service` | 2516 | runtime |
| sysone-batched (8 workers) | 8104 | `lm-sysone-batched.service` | 10566 | runtime |
| clef-flash (escalation) | 8096 | `clef-flash.service` | 7590 | runtime |
| LFM2-1.2B realizer | 8092 | `lm-realizer.service` | 1476 | runtime |
| LFM3-Audio-2B (ears; llama-server :8103 behind the :8102 proxy) | 8102 | `lm-ears.service` | 3723 | runtime |
| d1-3B — **not in last-mile's port map**, still running | 8093 | `d1-3b.service` | 3934 | (drop; see below) |
| Riva/Triton (skald-riva container: tritonserver + python stub) | 50000/50051 | docker `skald-riva` | 1058 + 4320 = 5378 | voice/studio |
| Qwen3-Coder-30B (studio, 2×32k slots) | 8001 | `llama-coder30.service` | 24015 | voice/studio |
| Muse-Glimmer-30B (studio, 2×32k slots) | 8005 | `lm-glimmer.service` | 16595 | voice/studio |
| GNOME desktop | — | Xorg + gnome-shell | 148 | → gfx instance |

Totals: the GPU-holding processes sum to **~74 GiB of the 122 GiB** unified
pool; `tegrastats` shows RAM 102.6/125.7 GB and `last-mile.sh status` reports
17 GiB available. The runtime slice (five last-mile services) is **~25.9 GiB**;
the voice/studio slice is **~46 GiB**; d1-3B is 3.9 GiB of dead weight the hot
list already proposes stopping ([12-thor-models Part 1](12-thor-models.md)).

---

## 3. The proposed split

One MIG GPU, two instances, exactly the NVIDIA-recommended pair:

**Runtime slice → profile 83 (`2g.0gb+gfx`, 12 SMs, 60%):**
d1-2B :8094, sysone-batched :8104, clef-flash :8096, the realizer :8092, the
ears :8102 — plus the GNOME desktop, which needs the `+gfx` engines and costs
148 MiB. Total ≈ 26 GiB of unified memory (shared pool, fits trivially).

**Voice/studio slice → profile 78 (`1g.0gb+me`, 6 SMs, 30%):**
skald-riva (docker, via the cap-node workaround), Qwen3-Coder-30B :8001,
Muse-Glimmer-30B :8005, and any ComfyUI studio run. Total ≈ 46 GiB now, up to
~90 GiB with a big studio pipeline — the pool is shared, so this is the number
to watch, not the profile's "0 GiB".

**d1-3B :8093 stays stopped** — not in last-mile's port map, "slower and no
better" per the Thor index; the window is the moment to take its 3.9 GiB back
([12-thor-models](12-thor-models.md)).

**What this buys.** The decision path stops queueing behind studio kernels:
83's 12 SMs are hardware-reserved for it, so the move pack should sit near its
quiet-box envelope (D74: p50 164 ms) instead of tonight's 1.1–1.8 s, *while
Glimmer and Coder generate*, and the batched server's 6.2x model-time overlap
finally converts to wall clock because the GPU under it is no longer 95–98%
busy. Refusals that are really timeouts should fall back toward the single
digits.

**What it costs — said plainly.** The studio slice gets 6 of 20 SMs: expect
roughly a third of today's throughput on Coder and Glimmer (82 tok/s class →
~25–30 tok/s class), worse when two studio models share the 78 instance. The
runtime slice is capped at 12 of 20 SMs even when the studio is idle — a
single d1-2B forward pass has the parallelism to use more, so the quiet-box
move pack may land somewhat above 164 ms (my estimate, ~1.3–1.6x for 60% of
the SMs; unmeasured). MIG is a bet that *stable-and-fast-enough decisions*
are worth more than *fast studio batches* — which is the same bet the brief's
three findings make.

---

## 4. What cannot work, and the bugs to expect

- **The 1g compute profiles 80 and 81 hang on the first CUDA kernel**
  (enumerate fine, first kernel never returns; `cudaMalloc` hangs likewise) on
  r39.2.0/r39.2.1. NVIDIA staff: use 78 and 83 only, per the release notes.
  2g profiles were measured fine
  ([forum 384828](https://forums.developer.nvidia.com/t/agx-thor-mig-1g-instances-enumerate-as-cuda-devices-but-hang-on-the-first-kernel/384828),
  [forum 379790](https://forums.developer.nvidia.com/t/cudamalloc-hang-when-mig-is-enabled-on-jetson-thor/379790)).
  Note 78 is also "1g"-named but is the `+me` variant and works.
- **Upgraded boxes lose the 1g/2g profiles** (only 3g shows in `-lgip`,
  `-cgi 83` fails "Invalid Argument") when the R38-era
  `/etc/modprobe.d/nvidia-unifiedgpudisp.conf` survives a System Upgrade to
  7.2.1; fix is `sudo rm` of the file + reboot
  ([forum 380199](https://forums.developer.nvidia.com/t/jetson-agx-thor-jetpack-7-2-1-mig-only-3g-mig-profiles-are-available-1g-2g-profiles-missing-after-system-upgrade/380199)).
  **Checked tonight on this box: the file is absent** (`/etc/modprobe.d/` holds
  only `nvidia-display.conf` and `nvidia-preferred-oot-modules.conf`) — but
  re-check in preflight.
- **`nvidia-smi` output is wrong under MIG** — release-notes known issue
  #6162096, "Output of nvidia-smi is incorrect when using MIG"
  ([r39.2 release notes](https://docs.nvidia.com/jetson/archives/r39.2/ReleaseNotes/Jetson_Linux_Release_Notes_r39.2.pdf);
  also seen in [forum 372822](https://forums.developer.nvidia.com/t/jetson-thor-mig-support-first-impression/372822)).
  Verification must lean on `nvidia-smi -L` / `nvidia-smi mig -lgi`, health
  endpoints, and `tegrastats`, not the usual per-process columns.
- **SoC engines (VIC/PVA/OFA) stop working while MIG is enabled** — NVIDIA
  confirmed, fix planned
  ([forum 379346](https://forums.developer.nvidia.com/t/soc-engines-dont-work-when-mig-is-enabled-on-jetson-thor/379346)).
  Nothing in the last-mile runtime lane uses them (LLM + audio + desktop), and
  profile 78 nominally carries the media engines — but any future camera /
  video-encode pipeline must re-test with MIG on.
- **Docker + MIG** — the known issue and workarounds are in §1. This is the
  highest-probability failure of the window; it only affects `skald-riva`.

---

## 5. The maintenance window, step by step

Nothing below runs itself; every command is listed. Assumed window: ~90 min,
no live game running, the LAN's ProArt not sending traffic. The desktop goes
down in the middle (MIG enable requires the display stopped — the r39.2
guide's own flow), so have an SSH session open before step 3.

**Step 0 — preflight (read-only, can be done any time before).**
```
ls /etc/modprobe.d/nvidia-unifiedgpudisp.conf   # must NOT exist (checked 2026-10-29: absent)
nvidia-smi | grep -E 'Driver|Thor'              # 595.78, CUDA 13.2
cat /etc/nv_tegra_release                        # R39 REVISION 2.1
~/lm-services/last-mile.sh status                # all five up
systemctl --user stop lm-soak.timer              # it fires every 3h (00/3:00) and would run live.sh mid-window
```

**Step 1 — baseline, MIG off, studio load up** (§7's "without" point; reuse
tonight's numbers only if the studio load is identical).

**Step 2 — stop every GPU holder, userspace first:**
```
~/lm-services/last-mile.sh stop                                  # d1-2b, clef-flash, lm-ears, lm-realizer
systemctl --user stop lm-sysone-batched lm-glimmer llama-coder30 d1-3b
docker stop skald-riva
systemctl --user stop skald-daemon                                # if it holds no GPU it can stay; verify with nvidia-smi
nvidia-smi                                                        # expect only Xorg/gnome-shell
```

**Step 3 — desktop down, then enable + create** (the guide's flow, with the
cold-boot preface the forum fix found necessary
([forum 384828](https://forums.developer.nvidia.com/t/agx-thor-mig-1g-instances-enumerate-as-cuda-devices-but-hang-on-the-first-kernel/384828)) —
if the box has been up for days or was ever upgraded, reboot first):
```
sudo reboot                        # fresh state; back via SSH
sudo systemctl isolate multi-user.target
sudo nvidia-smi -pm 1              # persistence mode
sudo nvidia-smi -i 0 -mig 1        # MIG mode on
sudo nvidia-smi mig -i 0 -cgi 83 -C   # runtime slice, created FIRST → device index 0
sudo nvidia-smi mig -i 0 -cgi 78 -C   # studio slice, second → device index 1
nvidia-smi -L && nvidia-smi mig -lgi  # two MIG devices; record both MIG-<uuid>s
sudo ls -la /dev/nvidia-caps/         # record which cap numbers appeared (for Riva)
```
If `-cgi 83` says Invalid Argument → the upgrade bug; check step 0's file
again and see [forum 380199](https://forums.developer.nvidia.com/t/jetson-agx-thor-jetpack-7-2-1-mig-only-3g-mig-profiles-are-available-1g-2g-profiles-missing-after-system-upgrade/380199).

**Step 4 — install the boot-time recreation + pinning, before anything restarts.**
Instances do not survive reboot (§1), so a root oneshot recreates them and
writes the UUIDs where the user units can read them:

`/usr/local/sbin/last-mile-mig-configure.sh`:
```bash
#!/bin/bash
set -euo pipefail
if ! nvidia-smi -L | grep -q 'MIG-'; then
    nvidia-smi -pm 1
    nvidia-smi -i 0 -mig 1
    nvidia-smi mig -i 0 -cgi 83 -C   # order fixes the indexes: 83→0, 78→1
    nvidia-smi mig -i 0 -cgi 78 -C
fi
RUNTIME=$(nvidia-smi -L | grep -o 'MIG-[0-9a-f-]*' | sed -n 1p)   # 83, index 0
STUDIO=$(nvidia-smi -L | grep -o 'MIG-[0-9a-f-]*' | sed -n 2p)    # 78, index 1
printf 'CUDA_VISIBLE_DEVICES=%s\n' "$RUNTIME" > /run/last-mile-mig-runtime.env
printf 'CUDA_VISIBLE_DEVICES=%s\n' "$STUDIO"  > /run/last-mile-mig-studio.env
```
`/etc/systemd/system/nvidia-mig-configure.service`:
```ini
[Unit]
Description=Recreate last-mile MIG instances (Thor profiles 83+78)
Before=gdm.service
ConditionPathExists=/etc/last-mile/mig-enabled
[Service]
Type=oneshot
ExecStart=/usr/local/sbin/last-mile-mig-configure.sh
RemainAfterExit=yes
[Install]
WantedBy=graphical.target
```
(`sudo touch /etc/last-mile/mig-enabled`; `sudo systemctl daemon-reload &&
sudo systemctl enable nvidia-mig-configure.service`. The flag file is the
rollback switch — removing it disables recreation after MIG is turned off.)

Pinning the llama.cpp user units — one drop-in each, the same reversible
pattern the d1-2b slot drop-in already uses. Runtime services
(`d1-2b`, `clef-flash`, `lm-realizer`, `lm-ears`, `lm-sysone-batched`):
```
# ~/.config/systemd/user/<unit>.service.d/mig.conf
[Service]
EnvironmentFile=-/run/last-mile-mig-runtime.env
```
Studio units (`lm-glimmer`, `llama-coder30`): same with
`/run/last-mile-mig-studio.env`. No `Environment=` expansion games; the env
file already carries the resolved `CUDA_VISIBLE_DEVICES=MIG-<uuid>`. Simpler
fallback if the UUID dance ever misbehaves: hard-code
`Environment=CUDA_VISIBLE_DEVICES=0` / `=1` — the r39.2 guide itself pins by
index
([r39.2 MIG guide](https://docs.nvidia.com/jetson/archives/r39.2/DeveloperGuide/SD/MiG.html))
— at the cost of silently swapping slices if the creation order ever changes.

**Step 5 — desktop back, on the gfx instance:**
```
sudo systemctl start gdm3
nvidia-smi    # Xorg + gnome-shell should show on the 83 instance (GI column)
```

**Step 6 — studio slice first** (it is the slice most likely to fail, and it
must not be allowed to fall back to the whole GPU):
```
systemctl --user start lm-glimmer llama-coder30
# skald-riva with the MIG workaround (forum 383262):
docker run -d --name skald-riva --runtime=nvidia \
  -e NVIDIA_VISIBLE_DEVICES=0 \
  -e CUDA_VISIBLE_DEVICES=MIG-<uuid-78> \
  --device=/dev/nvidia-caps/nvidia-cap<N1> --device=/dev/nvidia-caps/nvidia-cap<N2> \
  ... (the rest of the existing skald-riva invocation unchanged)
```
If CUDA still fails to initialize in the container, the fallbacks in order:
add the *other* instance's cap nodes too (a `;`-joined UUID list), then
`--privileged`, then — decision point — leave Riva on the whole-GPU path with
MIG not enabled for it is **not** possible (there is no whole GPU once MIG
mode is on), so the last resort is rollback (§6) or Riva moves to CPU/NIM
pending NVIDIA's fix. Sources: [forum 383262](https://forums.developer.nvidia.com/t/trouble-starting-docker-container-with-cuda-after-enabling-mig/383262),
[forum 380864](https://forums.developer.nvidia.com/t/can-not-run-mig-in-docker-container-on-jetson-thor/380864).

**Step 7 — runtime slice, one at a time, watching each:**
```
systemctl --user daemon-reload
systemctl --user start d1-2b && sleep 5 && curl -s localhost:8094/health
curl -s -X POST localhost:8094/v1/systemone -d @<a captured pack> | head -c 400   # sane answer?
systemctl --user start clef-flash lm-realizer lm-ears lm-sysone-batched
```
This is where llama.cpp-in-MIG gets its first confirmation (§1's open item):
if the d1-2B hangs loading or answering, stop, capture `dmesg | tail`, and go
to rollback — do not proceed to step 8.

**Step 8 — verify the lane:**
```
~/lm-services/last-mile.sh status                        # five green
nvidia-smi -L && nvidia-smi mig -lgi                     # instances live; ignore per-process MiB (known-wrong)
python3 tools/sysone-batched/compare.py 8094 8104 /tmp/packs-mig.jsonl 500   # still bit-identical
bash scripts/live.sh                                     # the whole live lane on the Thor
```
`scripts/live.sh` is the lane that exists because the Thor is not a build
dependency — it asks every port the game really uses. Its green is the gate.

**Step 9 — prove the boot chain, inside the window:**
```
sudo reboot
# after boot: nvidia-smi -L (two MIG devices), last-mile.sh status (five up),
# skald-riva healthy, desktop on 83 — all with no manual steps
```
This simultaneously verifies the recreation unit and settles the
reboot-persistence question for Jetson empirically (§9 item 2).

**Step 10 — measure (§7's "with" point), then decide: keep or roll back.**
Re-arm the soak: `systemctl --user start lm-soak.timer`.

---

## 6. Rollback to MIG disabled

Reverse order, drop-ins first so nothing can pin to a dying instance:
```
~/lm-services/last-mile.sh stop
systemctl --user stop lm-sysone-batched lm-glimmer llama-coder30
docker stop skald-riva
rm ~/.config/systemd/user/{d1-2b,clef-flash,lm-realizer,lm-ears,lm-sysone-batched,lm-glimmer,llama-coder30}.service.d/mig.conf
systemctl --user daemon-reload
sudo systemctl stop gdm3 2>/dev/null; sudo systemctl isolate multi-user.target
sudo rm -f /etc/last-mile/mig-enabled            # kill the boot recreation
sudo systemctl disable nvidia-mig-configure.service
sudo nvidia-smi mig -dci                          # delete compute instances
sudo nvidia-smi mig -dgi                          # delete GPU instances
sudo nvidia-smi -i 0 -mig 0                       # MIG mode off
sudo reboot                                       # the clean GPU reset on Jetson
# after reboot: nvidia-smi shows MIG Disabled; last-mile.sh start; live.sh green
```
A bare `-mig 0` without instance deletion, or without a reboot, is not a state
I have a source for on Jetson — the reboot makes rollback deterministic.

---

## 7. The measurement that proves it worth it

Paired runs of the two harnesses under studio load, only the MIG state
differing. The studio load is `lm-glimmer` + `llama-coder30` both serving
(e.g. a pair of generation loops), same prompts both nights.

**Without MIG** (baseline, before the window — or tonight's numbers if the
load matches):
```
LM_THOR_PORT=8094 LM_CAPTURE_PACKS=/tmp/packs-off.jsonl \
  cargo test --release -p lm-agents --test talk_on_thor -- --ignored --nocapture
LM_THOR_PORT=8104 LM_CAPTURE_PACKS=/tmp/packs-off2.jsonl \
  cargo test --release -p lm-agents --test talk_on_thor -- --ignored --nocapture
python3 tools/sysone-batched/bench.py 8094 8104 /tmp/packs-off.jsonl 15
```

**With MIG** (after step 8): identical commands, `packs-on*` files.

Read out, per port, per run: move-pack p50/p95, talk-pack p50/p95 (the harness
tables and `bench.py` print these with load/GPU context), refusal counts from
the lane logs, and `tegrastats` RAM headroom.

**Success criteria** (adopt MIG):
- move pack under studio load: p50 ≤ 300 ms and p95 ≤ 600 ms (vs 1.1–1.8 s
  tonight; quiet box 164/262 ms);
- talk pack: p50 ≤ ~1 s (quiet box 770 ms), and :8104 beats :8094 on the talk
  pack — the overlap finally paying in wall clock;
- refusals back to single digits; `compare.py` still 100% / 0.0 difference;
- studio slice still serves (Coder ≥ ~20 tok/s is fine for its job).

**The confound to state with the numbers:** under MIG the studio slice runs on
6 SMs, so its *offered load* is lighter than in the baseline — the honest claim
is "decision latency is isolated from studio load", demonstrated by the
runtime percentiles holding while studio runs, not "everything got faster".
If the move pack does not return to near-quiet envelope, MIG is not buying
what we need and the split should roll back — 12 SMs should be plenty for a
2B-Q8 forward pass, but that is exactly what this measures.

---

## 8. Top risks, ranked

1. **Docker + MIG breaks skald-riva** (known issue on this exact stack, no
   official fix yet). Mitigation: the cap-node / privileged workarounds; if
   all fail, rollback is the only out, since with MIG mode on there is no
   whole GPU left for the container to fall back to.
2. **Technology preview, 78+83 only.** 1g profiles hang; nvidia-smi lies under
   MIG; SoC engines (VIC/PVA/OFA) break while MIG is on. Nothing in the
   runtime lane uses the broken pieces today — the moment something does, MIG
   has to come off.
3. **Studio throughput drops to ~30%** of SMs. If Coder/Glimmer seats become
   time-critical, this split is the wrong trade — re-measure before believing
   it is free.
4. **llama.cpp inside a Thor MIG instance has no public confirmation** — it is
   plain CUDA, and PyTorch/TensorRT worked on 78/83, but step 7 is the first
   llama.cpp data point. Budget the window for it to fail once.
5. **Instances vanish on reboot**; the recreation unit is untested until the
   window's step 9. If it misorders, the desktop or the services can start
   against a GPU with no instances and come up broken or on the wrong slice.
6. **Memory is shared, not partitioned** — MIG does not protect the runtime
   slice from a studio OOM, and per-process memory telemetry under MIG reads
   0 MiB. Keep the ~30 GiB RAM guard the hot list already keeps.

---

## 9. What I could not confirm

1. **Whether MIG mode itself survives reboot on Jetson** — datacenter docs say
   yes, Jetson docs are silent; the window's step 9 answers it for this box.
2. **Jetson-specific instance persistence** — same; the recreation unit makes
   the answer moot either way.
3. **llama.cpp / llama-server inside a Thor MIG instance** — no public report
   found; step 7 is the test.
4. **The datacenter `docker --gpus "device=MIG-…"` syntax under L4T's
   nvidia-container-runtime** — the L4T threads only show
   `NVIDIA_VISIBLE_DEVICES`/cap-node forms; assume the env-var form.
5. **Whether more than one 78 instance may be created** (6+6+12 = 18 SMs would
   fit) — every NVIDIA example is exactly one 83 + one 78; the release notes
   say "profiles 78 and 83", plural-ambiguous. Untested; do not depend on it.
6. **Why two SMs are unallocated** in the 78+83 pair — no doc explains the
   remaining 10%; measured only as 12+6 of 20.
7. **SM-count discrepancy for profile 82**: the r39.2 guide's table says 12
   SMs; one forum post says 14 for `2g.0gb`. I used the guide; neither value
   matters for the recommended pair.
8. **`nvidia-smi mig -lgip` on this box while MIG is disabled** prints an
   empty table (measured) — so the full local profile table above is the
   r39.2 guide's, not this box's own printout. It will be this box's own
   printout at step 3; if it differs from the guide, stop and re-read.

---

## Sources

- [Jetson Linux r39.2 Developer Guide — Multi-Instance GPU](https://docs.nvidia.com/jetson/archives/r39.2/DeveloperGuide/SD/MiG.html) — profiles, 0 GiB memory, enable flow, gfx instance, index pinning
- [Jetson Linux r39.2.0 GA Release Notes (PDF)](https://docs.nvidia.com/jetson/archives/r39.2/ReleaseNotes/Jetson_Linux_Release_Notes_r39.2.pdf) — technology preview, "profiles 78 and 83", nvidia-smi known issue #6162096
- [NVIDIA MIG User Guide — Getting Started](https://docs.nvidia.com/datacenter/tesla/mig-user-guide/latest/getting-started-with-mig.html) — instances not persistent across reboot
- [NVIDIA MIG User Guide — MIG Device Names](https://docs.nvidia.com/datacenter/tesla/mig-user-guide/latest/mig-device-names.html) — `CUDA_VISIBLE_DEVICES=MIG-<uuid>`, one CI per GI
- [NVIDIA/gpu-operator#343](https://github.com/NVIDIA/gpu-operator/issues/343) — mode persists, instances don't
- [Forum: MIG on Thor, JetPack 7.0 not supported](https://forums.developer.nvidia.com/t/how-to-use-mig-technology-to-divide-computing-units-in-thor/344978)
- [Forum: 1g instances hang; 78+83 resolved, TensorRT+PyTorch working](https://forums.developer.nvidia.com/t/agx-thor-mig-1g-instances-enumerate-as-cuda-devices-but-hang-on-the-first-kernel/384828)
- [Forum: cudaMalloc hang on 1g, 2g fine at 118–127 TFLOP/s](https://forums.developer.nvidia.com/t/cudamalloc-hang-when-mig-is-enabled-on-jetson-thor/379790)
- [Forum: upgrade loses 1g/2g profiles; modprobe fix](https://forums.developer.nvidia.com/t/jetson-agx-thor-jetpack-7-2-1-mig-only-3g-mig-profiles-are-available-1g-2g-profiles-missing-after-system-upgrade/380199)
- [Forum: docker + MIG known issue](https://forums.developer.nvidia.com/t/can-not-run-mig-in-docker-container-on-jetson-thor/380864)
- [Forum: docker cap-node workaround](https://forums.developer.nvidia.com/t/trouble-starting-docker-container-with-cuda-after-enabling-mig/383262)
- [Forum: SoC engines broken under MIG](https://forums.developer.nvidia.com/t/soc-engines-dont-work-when-mig-is-enabled-on-jetson-thor/379346)
- [Forum: MIG first impression; nvidia-smi incorrect](https://forums.developer.nvidia.com/t/jetson-thor-mig-support-first-impression/372822)

Local evidence, this box, 2026-10-29 ~22:38: `nvidia-smi` (12 GPU-holding
processes, ~74 GiB), `nvidia-smi mig -lgip` (empty, MIG disabled), `nvidia-smi
-q` (MIG Current/Pending Disabled, GPU UUID GPU-a7c66ad2-6dbb-0ab8-c1a2-37ba6dba3600),
`ps`/`ss -ltnp` (service→port→PID map), `tegrastats` (RAM 102.6/125.7 GB),
`/etc/nv_tegra_release` (R39.2.1), `apt` (nvidia-jetpack 7.2.1-b49),
`/etc/modprobe.d` (no `nvidia-unifiedgpudisp.conf`), systemd user units and
drop-ins, `~/lm-services/last-mile.sh status`. No state was changed.
