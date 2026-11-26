//! Property tests: 50 seeded random decision streams (late and stale
//! arrivals included) must replay bit-exactly, and a log that is truncated at
//! a random record boundary must survive the truncation.

use schedproto::driver::Driver;
use schedproto::log::{decode_log, encode_log, record_count, split_log};
use schedproto::replay::{replay_bytes, replay_records, Replayer};
use schedproto::rng::XorShift64Star;
use schedproto::seq::DEFAULT_STALE_WINDOW;
use schedproto::types::{Decision, Kind, Stamped};

const N_AGENTS: usize = 8;
const N_GATES: u8 = 2;
const SEEDS: u64 = 50;

fn random_kind(rng: &mut XorShift64Star) -> Kind {
    match rng.below(10) {
        0..=4 => Kind::ActionOn { action: rng.below(4) as u8 },
        5..=7 => Kind::Lane {
            gate: rng.below(N_GATES as u64) as u8,
            p: [
                rng.below(101) as u8,
                rng.below(101) as u8,
                rng.below(101) as u8,
                rng.below(101) as u8,
            ],
        },
        8 => {
            let plan_len = 1 + rng.below(3) as usize;
            let safe_len = 1 + rng.below(3) as usize;
            let mut plan = Vec::with_capacity(plan_len);
            for _ in 0..plan_len {
                plan.push((rng.range(-20, 20) as i16, rng.range(-20, 20) as i16));
            }
            let mut safe = Vec::with_capacity(safe_len);
            for _ in 0..safe_len {
                safe.push((rng.range(-20, 20) as i16, rng.range(-20, 20) as i16));
            }
            Kind::Intent { ttl: 1 + rng.below(30) as u16, plan, safe }
        }
        _ => Kind::Marker(rng.next_u64() as u32),
    }
}

/// Arrival tick relative to the requested tick: on time, late (inside the
/// stale window) or stale (dropped).
fn random_arrival(rng: &mut XorShift64Star, apply_tick: u64) -> u64 {
    match rng.below(10) {
        0..=4 => apply_tick,
        5..=7 => apply_tick + 1 + rng.below(DEFAULT_STALE_WINDOW) as u64,
        _ => apply_tick + DEFAULT_STALE_WINDOW + 1 + rng.below(50) as u64,
    }
}

/// Builds one random decision stream and returns the driver that consumed it.
fn build_stream(seed: u64) -> Driver {
    let mut rng = XorShift64Star::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut driver = Driver::new(N_AGENTS, N_GATES as usize);
    let n_decisions = 200 + rng.below(200) as u64;
    let horizon = 600u64;

    for i in 0..n_decisions {
        let source = rng.below(N_AGENTS as u64) as u16;
        let seq = i as u32;
        let kind = random_kind(&mut rng);
        let apply_tick = rng.below(horizon);
        let arrival = random_arrival(&mut rng, apply_tick);
        driver.submit(
            Decision { apply_tick, source, seq, kind },
            arrival,
        );
    }
    driver.run_all();
    driver
}

#[test]
fn replay_of_log_reproduces_the_live_hash_sequence() {
    for seed in 1..=SEEDS {
        let driver = build_stream(seed);
        assert!(driver.hashes.len() > 1, "seed {seed}: no hashes produced");
        let replayed = replay_bytes(&driver.log, N_AGENTS, N_GATES as usize)
            .unwrap_or_else(|e| panic!("seed {seed}: decode failed: {e}"));
        assert_eq!(
            driver.hashes, replayed.hashes,
            "seed {seed}: replay hash sequence differs from the live one"
        );
        assert_eq!(
            driver.hashes.last().copied(),
            Some(replayed.final_hash),
            "seed {seed}: final hash differs"
        );
        assert_eq!(replayed.ticks, driver.world.tick, "seed {seed}: tick count differs");
    }
}

#[test]
fn replay_from_decoded_records_matches_replay_from_bytes() {
    for seed in 1..=SEEDS {
        let driver = build_stream(seed);
        let decoded = decode_log(&driver.log).unwrap();
        assert_eq!(decoded.len(), record_count(&driver.log).unwrap());
        let direct = replay_records(&decoded, N_AGENTS, N_GATES as usize);
        let from_bytes = replay_bytes(&driver.log, N_AGENTS, N_GATES as usize).unwrap();
        assert_eq!(direct.hashes, from_bytes.hashes, "seed {seed}");
        assert_eq!(direct.final_hash, from_bytes.final_hash, "seed {seed}");
        // the decoded records re-encode to the very same bytes
        assert_eq!(encode_log(&decoded), driver.log, "seed {seed}: log is not canonical");
    }
}

#[test]
fn stale_decisions_are_dropped_and_late_ones_are_counted() {
    for seed in 1..=SEEDS {
        let driver = build_stream(seed);
        assert!(
            driver.seq.stats.dropped > 0,
            "seed {seed}: the stream contains no stale arrival"
        );
        assert!(
            driver.seq.stats.late > 0,
            "seed {seed}: the stream contains no late arrival"
        );
        assert!(driver.seq.stats.accepted > driver.seq.stats.dropped);
    }
}

#[test]
fn kill_test_truncate_at_a_record_boundary_and_replay_the_prefix() {
    for seed in 1..=SEEDS {
        let driver = build_stream(seed);
        let full = replay_bytes(&driver.log, N_AGENTS, N_GATES as usize).unwrap();
        let total = record_count(&driver.log).unwrap();
        assert!(total > 1, "seed {seed}: log too small");

        let mut rng = XorShift64Star::new(seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
        let cut = rng.below(total as u64 + 1) as usize;

        let (prefix, rest) = split_log(&driver.log, cut).unwrap();
        assert_eq!(record_count(&prefix).unwrap(), cut);

        // (a) the log is truncated, the prefix is replayed, the rest is
        //     appended again -> the result is the full replay
        let prefix_result = replay_bytes(&prefix, N_AGENTS, N_GATES as usize).unwrap();

        // The prefix may stop in the middle of a tick group. Replaying the
        // prefix on its own finishes that tick with only the decisions that
        // survived the cut, so its last hash may differ. Every hash taken
        // before that tick is unaffected and has to match.
        let cut_tick = if cut == 0 {
            0
        } else {
            decode_log(&prefix).unwrap().last().unwrap().apply_tick
        };
        let settled = (cut_tick / schedproto::HASH_EVERY) as usize;
        assert_eq!(
            prefix_result.hashes[..settled.min(prefix_result.hashes.len())],
            full.hashes[..settled],
            "seed {seed} cut {cut}: the prefix hash sequence is not a prefix of the full one"
        );

        let mut rejoined = prefix;
        rejoined.extend_from_slice(&rest);
        let rejoined_result = replay_bytes(&rejoined, N_AGENTS, N_GATES as usize).unwrap();
        assert_eq!(
            rejoined_result.hashes, full.hashes,
            "seed {seed} cut {cut}: re-appending the rest changed the replay"
        );
        assert_eq!(rejoined_result.final_hash, full.final_hash, "seed {seed} cut {cut}");
    }
}

#[test]
fn kill_test_resume_from_the_truncated_state() {
    for seed in 1..=SEEDS {
        let driver = build_stream(seed);
        let full = replay_bytes(&driver.log, N_AGENTS, N_GATES as usize).unwrap();
        let total = record_count(&driver.log).unwrap();

        let mut rng = XorShift64Star::new(seed.wrapping_mul(0xA076_1D64_78BD_642F) | 1);
        let cut = rng.below(total as u64 + 1) as usize;

        let (prefix, rest) = split_log(&driver.log, cut).unwrap();

        // Replay the prefix only, then continue with the remaining records
        // from that very state (the prefix may stop in the middle of a tick).
        let mut replayer = Replayer::new(N_AGENTS, N_GATES as usize);
        assert_eq!(replayer.push_bytes(&prefix).unwrap(), cut);
        replayer.run_until_index(cut);
        let prefix_hashes = replayer.hashes.clone();

        assert_eq!(
            prefix_hashes,
            full.hashes[..prefix_hashes.len()],
            "seed {seed} cut {cut}: prefix hashes differ"
        );

        assert_eq!(replayer.push_bytes(&rest).unwrap(), total - cut);
        replayer.run_all();

        assert_eq!(
            replayer.hashes, full.hashes,
            "seed {seed} cut {cut}: resumed hash sequence differs from the full one"
        );
        assert_eq!(
            replayer.result().final_hash,
            full.final_hash,
            "seed {seed} cut {cut}: resumed final hash differs"
        );
    }
}

#[test]
fn the_log_is_written_in_canonical_order() {
    for seed in 1..=SEEDS {
        let driver = build_stream(seed);
        let decoded: Vec<Stamped> = decode_log(&driver.log).unwrap();
        for w in decoded.windows(2) {
            let a = schedproto::seq::canonical_key(&w[0], N_AGENTS);
            let b = schedproto::seq::canonical_key(&w[1], N_AGENTS);
            assert!(a <= b, "seed {seed}: log is not in canonical order ({a:?} > {b:?})");
        }
    }
}
