# The corpus: twenty lines the ears have to hear

The twenty waves in this folder are the verification corpus. Each one is
a line of the game spoken by the Thor's own voice (skald, the `mia`
voice) and brought to the capture path's rate — the same rate the
microphone gives the host — so it can be fed to `--mic-wav` exactly as a
human's push-to-talk would be.

The lines are in `lines.txt`, one per line, in the order of the waves:

```
hold
wait
stay
hold the north beacon
wait for the beacon
...
```

## How they were made

```
for i in $(seq 1 20); do
  line=$(sed -n "${i}p" lines.txt)
  curl -sS -X POST http://192.168.0.78:8000/v1/audio/speech \
    -H 'Content-Type: application/json' \
    -d "{\"input\": \"${line}\", \"voice\": \"mia\"}" \
    -o "line$(printf %02d $i).wav"
done
```

## What they are for

Two runs:

- **the screenshot** — one line, so the HUD can be seen with the words
  the ears heard in it;
- **the measurement** — all twenty, so the turn's latency has a
  distribution and not one number: the p50 and the p95, and the split
  between the wire and the Thor's own work.

The waves are kept, not regenerated, so a run measures the same bytes
every time. A fresh corpus from the same lines is the same audio: skald
speaks the same words the same way, and the resampling is the same
function.
