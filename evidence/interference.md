# What the ears cost the box

d1-2B on :8094, 30 sequential System One packs per phase,
the same pack every time.

| phase | median ms | mean ms | p90 ms | min ms | max ms |
|---|---|---|---|---|---|
| A ears running, idle | 27.1 | 28.7 | 28.7 | 24.5 | 73.4 |
| B ears stopped | 26.1 | 26.7 | 27.6 | 24.1 | 50.8 |
| C ears running again, idle | 26.3 | 27.0 | 28.2 | 24.1 | 50.7 |
| D ears running and busy | 32.3 | 33.3 | 42.4 | 25.0 | 43.7 |

| when | ears GPU MiB | ears RSS MiB (pid) | free GPU GiB | all GPU used MiB |
|---|---|---|---|---|
| ears running | 3717 | 3267568:54, 3267575:1149 | nan | 53376 |
| ears stopped | 0 | - | 7.71 | 49659 |
| ears running again | 3611 | 3284671:54, 3284676:800 | 3.94 | 53270 |
| after the load | 3715 | 3284671:54, 3284676:910 | 3.45 | 53374 |

Transcriptions the ears did while the d1-2B was measured in phase D: 44
