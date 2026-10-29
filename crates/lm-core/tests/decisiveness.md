# Decisiveness sweep (D60 follow-up, recorded as D63)

```bash
cargo test --release -p lm-core --test decisiveness -- --ignored --nocapture
```

Default reflex agents vs default reflex agents, 4 point-symmetric beacons (D61), seeds 1..=100 per cell.

| win_score | time_limit_ticks | squad 0 wins | squad 1 wins | draws | draw % | median end tick | draws before the limit |
|---|---|---|---|---|---|---|---|
| 900  | 9000  | 20 | 17 | 63 | 63% | 2984 | 63 |
| 900  | 12000 | 20 | 17 | 63 | 63% | 2984 | 63 |
| 1200 | 9000  | 20 | 17 | 63 | 63% | 3583 | 63 |
| 1200 | 12000 | 20 | 17 | 63 | 63% | 3583 | 63 |
| 1500 | 9000  | 20 | 17 | 63 | 63% | 4183 | 63 |
| 1500 | 12000 | 20 | 17 | 63 | 63% | 4183 | 63 |
| 1800 | 9000  | 21 | 16 | 63 | 63% | 4784 | 63 |
| 1800 | 12000 | 21 | 16 | 63 | 63% | 4784 | 63 |

## Reading

- **Every draw is a mirror tie, not a timeout.** All 63 draws per cell end before the time limit, at the same tick for both squads. The map is point-symmetric (D61), and two identical deterministic reflexes play the same game reflected, so both squads reach win_score on the same tick.
- **win_score and time_limit_ticks don't change decisiveness at all.** The draw rate is 63% in every cell; win_score only moves the end tick (about 600 ticks per +300 points).
- D60's "draws at the 9000-tick limit" is corrected by this. The tuning lever isn't the scoring parameters: something has to break the mirror. That could be agents that think (the model tiers, talk, learning), seeded per-squad variation, or a tie-break rule.
