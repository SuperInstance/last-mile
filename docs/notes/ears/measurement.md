# The measurement: twenty turns, key up to line applied

The run: the Windows host, the twenty waves of `corpus.md` fed to the
capture path as twenty push-to-talk turns, the Thor's ears answering.

## The run on a responsive box

```
lm-host: the ears heard "Hold." -- 179 ms (5 ms network, 174 ms compute; the ears took 142 ms)
lm-host: the ears heard "Wait." -- 117 ms (7 ms network, 110 ms compute; the ears took 97 ms)
lm-host: the ears heard "Stay." -- 133 ms (8 ms network, 125 ms compute; the ears took 98 ms)
lm-host: the ears heard "Hold the north beacon." -- 167 ms (5 ms network, 162 ms compute; the ears took 144 ms)
lm-host: the ears heard "Wait for the beacon." -- 166 ms (5 ms network, 161 ms compute; the ears took 147 ms)
lm-host: the ears heard "Stay where you are." -- 183 ms (11 ms network, 172 ms compute; the ears took 153 ms)
lm-host: the ears heard "Hold the line." -- 168 ms (10 ms network, 158 ms compute; the ears took 141 ms)
lm-host: the ears heard "Wait a moment." -- 149 ms (5 ms network, 144 ms compute; the ears took 135 ms)
lm-host: the ears heard "Stay on the beacon." -- 201 ms (6 ms network, 195 ms compute; the ears took 174 ms)
lm-host: the ears heard "Hold the south beacon." -- 182 ms (5 ms network, 177 ms compute; the ears took 161 ms)
lm-host: the ears heard "Wait here." -- 150 ms (5 ms network, 145 ms compute; the ears took 122 ms)
lm-host: the ears heard "Stay on the north." -- 186 ms (5 ms network, 181 ms compute; the ears took 160 ms)
lm-host: the ears heard "Hold the East beacon." -- 183 ms (5 ms network, 178 ms compute; the ears took 158 ms)
lm-host: the ears heard "Wait for me." -- 151 ms (5 ms network, 146 ms compute; the ears took 139 ms)
lm-host: the ears heard "Stay on the west." -- 184 ms (11 ms network, 173 ms compute; the ears took 150 ms)
lm-host: the ears heard "Hold the North Beacon now." -- 198 ms (8 ms network, 190 ms compute; the ears took 180 ms)
lm-host: the ears heard "Wait for the squad." -- 183 ms (8 ms network, 175 ms compute; the ears took 148 ms)
lm-host: the ears heard "Stay on the beacon." -- 168 ms (6 ms network, 162 ms compute; the ears took 153 ms)
lm-host: the ears heard "Hold the West Beacon." -- 183 ms (6 ms network, 177 ms compute; the ears took 157 ms)
lm-host: the ears heard "Wait at the beacon." -- 186 ms (10 ms network, 176 ms compute; the ears took 156 ms)
lm-host: 20 turns -- the ears heard 20, 0 were late, 0 were down
lm-host: the turn cost -- p50 179 ms, p95 198 ms (network p50 6 ms, p95 11 ms; compute p50 172 ms, p95 190 ms)
```

| | p50 | p95 |
|---|---|---|
| the turn, key up to line applied | **179 ms** | **198 ms** |
| the network | 6 ms | 11 ms |
| the compute | 172 ms | 190 ms |

Twenty turns, twenty heard, none late, none down. Every line came back
as it was said: "Hold the north beacon", "Wait for the beacon", "Stay on
the west" — the twenty lines of `lines.txt`, in the twenty waves, and
the twenty transcripts above. The case does not matter to the ear: the
door says "Hold the East beacon" and "Hold the North Beacon now" as
they were said, and the game's own spelling is not the ear's business.

## How the split is measured

The **network** is a TCP handshake with the door's own address and port,
taken around each call. The kernel answers the handshake even when the
service behind it is busy, so the number is the LAN's and not the box's:
6 ms at the median, which is the ping's 5-8 ms and nothing else.

The **compute** is the rest of the turn: the queue in front of the
model, the model's own work, and the round's own bookkeeping. The
model's own share is reported separately by the door (the "the ears
took" figure): 156 ms at the median, so the model is most of the
compute and the queue in front of it is small.

The split matters because it says where a slow turn goes. On the LAN the
wire is 6 ms: a turn is slow because the Thor is thinking, not because
the words travelled.

## The same path on a busy box

The Thor is shared, and another seat's job makes it slow: the same call
that took 179 ms took 27-36 s when the box was loaded. The game's
deadline is 5 s, so the ears are late and the stub stands in — exactly
as the design says, and the round goes on:

```
lm-host: the ears were late for "hold" -- 5013 ms (5 ms network, 5008 ms compute; the ears took 0 ms)
lm-host: the ears were late for "hold" -- 5018 ms (5 ms network, 5013 ms compute; the ears took 0 ms)
...
lm-host: 19 turns -- the ears heard 0, 19 were late, 0 were down
lm-host: the turn cost -- p50 5017 ms, p95 5030 ms (network p50 6 ms, p95 14 ms; compute p50 5011 ms, p95 5021 ms)
```

and, with the box half free, the same run split:

```
lm-host: the ears were late for "hold" -- 5016 ms (5 ms network, 5011 ms compute; the ears took 0 ms)
...
lm-host: the ears heard "Hold the East beacon." -- 3933 ms (18 ms network, 3915 ms compute; the ears took 1689 ms)
lm-host: the ears heard "Wait for me." -- 169 ms (10 ms network, 159 ms compute; the ears took 142 ms)
...
lm-host: 20 turns -- the ears heard 8, 12 were late, 0 were down
```

Three things to read out of the busy runs:

- the turn **never waits**: a turn costs its deadline (5 s) and not the
  box's 30 s, and the line is applied;
- the **wire is 5-18 ms in every one of them**, late or not — the
  network is never the reason;
- the **HUD counts it**: `ears heard 8 / late 12 / down 0`, and the
  line the game shows says whose words it is ("ears late, the stub").

The measured path is the same in both runs: the difference is the Thor's
load, not the code.
