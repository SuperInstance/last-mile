# Word accuracy of the ears, per voice

20 phrasebook lines per voice, rendered by skald, transcribed by
LFM3-Audio-2B on :8102. WER is the word error rate; word accuracy
is 1 - WER.

Two columns of scores: **strict** compares the words as written
(the phrasebook writes "beacon 2", the model hears "beacon two"),
and **numbers spelled** puts both sides in the same spelling, so a
digit that came out as a word is not counted as a hearing error.

| voice | clips | ref words | S | D | I | WER | word accuracy | WER (numbers spelled) | accuracy (numbers spelled) | exact lines | median latency ms |
|---|---|---|---|---|---|---|---|---|---|---|---|
| aria | 20 | 55 | 4 | 0 | 0 | 0.073 | 0.927 | 0.055 | 0.945 | 16/20 | 119 |
| jason | 20 | 55 | 4 | 0 | 0 | 0.073 | 0.927 | 0.036 | 0.964 | 16/20 | 117 |
| mia | 20 | 55 | 8 | 0 | 0 | 0.145 | 0.855 | 0.145 | 0.855 | 14/20 | 118 |
| **all** | 60 | 165 | 16 | 0 | 0 | 0.097 | 0.903 | 0.079 | 0.921 | 46/60 | 118 |

## The lines that were not exact

| voice | reference | heard | WER |
|---|---|---|---|
| mia | take beacon 2 now | Take bacon too now. | 0.50 |
| jason | take beacon 2 now | Take beacon two now. | 0.25 |
| mia | beacon 2 is yours | Weekend 2 is yours. | 0.25 |
| jason | beacon 2 is yours | Egan 2 is yours. | 0.25 |
| aria | beacon 2 is yours | Weekend 2 is yours. | 0.25 |
| mia | leave beacon 2 | Leaf beacon too. | 0.67 |
| aria | leave beacon 2 | Leave beacon too. | 0.33 |
| mia | where is beacon 2 | Where is Beacon to? | 0.25 |
| jason | where is beacon 2 | Where is Beacon to? | 0.25 |
| aria | where is beacon 2 | Where is Beacon to? | 0.25 |
| mia | squad, on me | Squad on knee. | 0.33 |
| mia | they are at enemy 4 | They are at enemy for | 0.20 |
| jason | they are at enemy 4 | They are at enemy four. | 0.20 |
| aria | they are at enemy 4 | They are at enemy four. | 0.20 |
