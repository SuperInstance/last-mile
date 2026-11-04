//! A soak test: how much do agents talk? Playing 3000 ticks with the stub
//! and with the real Thor.
//!
//! One CSV line per run, with these columns:
//!
//! | # | column | what it is |
//! |---|---|---|
//! | 1 | `backend` | `stub` or `thor-d1-2b` |
//! | 2 | `seed` | the seed of the round |
//! | 3 | `utterances` | the lines the agents spoke in the round |
//! | 4 | `airtime_ticks` | the ticks of speech the round spent |
//! | 5 | `utterances_per_1k_ticks` | the utterances per 1000 ticks |
//! | 6 | `decisions` | the decisions the agents' answers became |
//! | 7 | `refusals` | the thoughts the model refused |
//! | 8 | `scores` | the round's scores, as the debug rendering of the map |
//!
//! The last column is the last one: the scores are a debug rendering and
//! may themselves contain commas, so a parser that wants the first seven
//! columns splits the line seven times, not eight.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::mind::Policy;
use lm_agents::sysone::{StubBackend, SysOne, ThorBackend};
use lm_agents::Agents;
use lm_core::header::Header;

use lm_core::talk::talk_metrics;
use lm_host_model::HostLoop;
use lm_tools::runner;

/// The shipped harness: the gates and the policy the default round thinks
/// with. The agents of this test are the ones the game would use; only the
/// model behind them is a stub.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// A standard round with the shipped reflex.
fn a_round(seed: u64) -> Header {
    Header::default_round(seed, runner::DEFAULT_REFLEX)
}

/// The shipped harness, read into memory.
fn default_harness() -> Harness {
    Harness {
        dir: std::path::PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// A stub behind every tier the policy names.
///
/// The tiers stay the ones the game would use — the same gates, the same
/// policy, the same ladder — and only the model behind them changes.
fn stub_backends(harness: &Harness) -> BTreeMap<String, Box<dyn SysOne + Send>> {
    let policy = Policy::for_harness(&harness.policy_toml);
    policy
        .tiers
        .tiers()
        .iter()
        .filter(|tier| !tier.is_reflex())
        .map(|tier| {
            (
                tier.backend().to_owned(),
                Box::new(StubBackend::menu()) as Box<dyn SysOne + Send>,
            )
        })
        .collect()
}

/// The round's agents, thinking on the stub.
fn stub_agents(header: &Header, harness: &Harness) -> Agents {
    Agents::new(
        &AgentsConfig::default(),
        harness,
        header,
        stub_backends(harness),
    )
}

/// Give the thinker thread a chance to answer before the next tick.
///
/// The host never waits for a model — that is the whole point of the loop —
/// but a test that plays ticks as fast as the CPU allows would outrun the
/// worker and lose the answers at the end of the round. The test waits here,
/// not the host.
fn settle(host: &mut HostLoop) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.agents_pending() > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The live round: the Thor's own d1-2b, on the LAN.
///
/// It is ignored by default: it needs the Thor to be up and it talks to a
/// real model. Run it with `cargo test -p lm-host-model -- --ignored`.
///
/// The round plays at its own rate — 300 ticks at 30 Hz is ten seconds — so
/// the models get the time a model needs. A round played faster than the
/// clock would let the answers land many ticks late, which is not how the
/// host plays.
///
/// The confidence floor is pinned low on purpose. The test proves the
/// *pipeline* — the question, the answer, the decision, the log — and
/// leaves the calibration where it belongs: a decision about the floor is
/// Casey's, not the test's. The floor is a rule per gate now, so at 0.15 no
/// gate of the d1-2B is ever dropped and the presence gates alone decide
/// whether a line is spoken; the model's own numbers are measured in
/// `lm-agents`'s `tests/talk_on_thor.rs` and written up in
/// crates/lm-agents/tests/talk_on_thor.md.
fn live_round_on_thor(header: &Header) -> Agents {
    let harness = default_harness();
    let cfg = AgentsConfig::default();

    let port = cfg.ports["d1-2b"];
    let backend = ThorBackend::new(&cfg.thor_base, port, "d1-2b")
        .with_min_confidence(0.15)
        .with_timeout(Duration::from_millis(u64::from(cfg.request_timeout_ms)));
    let backends = BTreeMap::from([(
        "thor-d1-2b".to_owned(),
        Box::new(backend) as Box<dyn SysOne + Send>,
    )]);

    Agents::new(&cfg, &harness, header, backends)
}

/// The length of each soak round, in ticks.
const TICKS: u64 = 3000;

/// The seeds the soak plays: three rounds per backend.
const SEEDS: [u64; 3] = [1, 2, 3];

/// The backends the soak plays: the stub first, then the Thor's d1-2B.
const BACKENDS: [&str; 2] = ["stub", "thor-d1-2b"];

/// Play one soak round on `backend` with `seed`, and print its line.
///
/// The stub answers at once; the Thor answers in real time, so its round is
/// played at the round's own rate (as `live_round_on_thor` does), or every
/// thought would arrive after its deadline.
fn soak(backend: &str, seed: u64) {
    let header = a_round(seed);
    let harness = default_harness();

    let mut host = HostLoop::new(header.clone()).expect("the round starts");

    let agents = match backend {
        "stub" => stub_agents(&header, &harness),
        "thor-d1-2b" => live_round_on_thor(&header),
        _ => panic!("unknown backend"),
    };

    host.attach_agents(agents);

    let period = Duration::from_millis(1000 / u64::from(header.params.tick_hz));
    for _ in 0..TICKS {
        if backend == "stub" {
            host.tick();
            settle(&mut host);
        } else {
            std::thread::sleep(period);
            host.tick();
        }
    }

    let talk_records = host.sim().talk_records();
    let metrics = talk_metrics(talk_records);
    let stats = host.agent_stats().expect("the round has agents");

    // Print one line per (backend, seed)
    println!(
        "{},{},{},{},{},{},{},{:?}",
        backend,
        seed,
        metrics.utterances,
        metrics.airtime_ticks,
        metrics.utterances * 1000 / TICKS,
        stats.decisions,
        stats.refusals,
        host.sim().scores()
    );

    // Assert only that the round finished
    assert!(!host.sim().scores().is_empty());
}

/// The stub half of the soak.
///
/// It needs no LAN, so it is not ignored: the gate runs it, and the harness
/// of the soak itself is checked on every change and not only when the Thor
/// is up. The Thor half is [`talk_soak`].
#[test]
fn talk_soak_stub() {
    for seed in SEEDS {
        soak("stub", seed);
    }
}

/// The whole soak: the stub and the Thor's d1-2B, on the LAN.
///
/// It is ignored by default: the Thor half needs the Thor to be up and it
/// talks to a real model. Run it with:
///
/// ```text
/// cargo test --release -p lm-host-model --test talk_soak -- --ignored --nocapture
/// ```
#[test]
#[ignore = "needs the Thor at 192.168.0.78; run with --release --ignored --nocapture"]
fn talk_soak() {
    for backend in BACKENDS {
        for seed in SEEDS {
            soak(backend, seed);
        }
    }
}
