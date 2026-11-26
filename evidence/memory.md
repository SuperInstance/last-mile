# What the ears hold

5 samples per state; the median of each is the number.

| state | free GPU GiB (median) | all GPU used MiB (median) | ears GPU MiB | unit RAM (cgroup) |
|---|---|---|---|---|
| ears running | 4.0 | 53374 | 3715 | 939589632 |
| ears stopped | 8.276 | 49659 | 0 | [not set] |
| ears running again | 4.155 | 53270 | 3611 | 760877056 |

- The GPU memory the ears take off the box: **4.198 GiB** (free with the ears minus free without them, median of the two running states against the stopped one).
- The ears' own processes hold 3715 MiB of GPU memory.
