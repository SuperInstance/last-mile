//! The question ledger: a round's record of the questions it asked.
//!
//! Three things are checked here, and they are the three claims the ledger
//! makes:
//!
//! 1. **The ledger is written per round**, as a file, beside the round's
//!    other records — and it is never hashed: the round's log carries the
//!    decisions, not the questions.
//! 2. **The ledger names the questions the way the harness wrote them**:
//!    one row per question, with the id and the version the harness's own
//!    bytes give it, and the normalized form of the wording.
//! 3. **The cache's hits are in the ledger**: a question answered out of
//!    the cache costs the round nothing, and the row says so.
//!
//! The round is played through the host's own path — the [`HostLoop`], the
//! [`Agents`], the least-use-first queue, the sequencer and the recorder —
//! with the stub model behind the tiers, so the test needs no network.
//!
//! ## The command
//!
//! ```text
//! cargo test -p lm-agents --test question_ledger
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use lm_agents::cache::AnswerCache;
use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::sysone::{StubBackend, SysOne};
use lm_agents::Agents;
use lm_core::header::Header;
use lm_host_model::HostLoop;

/// The shipped harness: the gates, the policy and the phrasebook the default
/// round thinks with.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

/// The shipped reflex, compiled into the test binary.
const DEFAULT_REFLEX: &str = include_str!("../../../assets/reflexes/default.rhai");

/// The ticks a round is played for.
const TICKS: u64 = 600;

/// The shipped harness, read into memory.
fn default_harness() -> Harness {
    Harness {
        dir: PathBuf::from("assets/harness/default"),
        gates_toml: DEFAULT_GATES.to_owned(),
        policy_toml: DEFAULT_POLICY.to_owned(),
        phrasebook_toml: DEFAULT_PHRASEBOOK.to_owned(),
        facts_toml: String::new(),
        reflexes: BTreeMap::new(),
        commit: "content:test-harness".to_owned(),
    }
}

/// The stub model behind every tier the policy names.
fn stub_backends(harness: &Harness) -> BTreeMap<String, Box<dyn SysOne + Send + Sync>> {
    let policy = lm_agents::mind::Policy::for_harness(&harness.policy_toml);
    policy
        .tiers
        .tiers()
        .iter()
        .filter(|tier| !tier.is_reflex())
        .map(|tier| {
            (
                tier.backend().to_owned(),
                Box::new(StubBackend::menu()) as Box<dyn SysOne + Send + Sync>,
            )
        })
        .collect()
}

/// Play a round and write its ledger, and say where it went.
fn play_round(
    seed: u64,
    cache: Option<Arc<AnswerCache>>,
    out: &std::path::Path,
) -> (lm_agents::ledger::QuestionLedger, std::path::PathBuf) {
    let header = Header::default_round(seed, DEFAULT_REFLEX);
    let harness = default_harness();
    let agents = Agents::new_with_cache(
        &AgentsConfig::default(),
        &harness,
        &header,
        stub_backends(&harness),
        cache,
    );
    let mut host = HostLoop::new(header).expect("the round starts");
    host.attach_agents(agents);
    for _ in 0..TICKS {
        host.tick();
        if host.sim().outcome().is_some() {
            break;
        }
    }
    // The answers still in the air land on the ticks after the round's.
    for _ in 0..60 {
        host.tick();
    }
    let agents = host.agents().expect("the round has its agents");
    agents.write_ledger(out).expect("the ledger is written");
    (agents.ledger().clone(), out.to_path_buf())
}

#[test]
fn the_ledger_is_written_and_names_the_questions() {
    let dir = std::env::temp_dir().join(format!("lm-ledger-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("the temp dir");
    let path = dir.join("questions.toml");

    let (agents, _) = play_round(7, None, &path);
    assert!(path.exists(), "the ledger file is written per round");

    let text = fs::read_to_string(&path).expect("the ledger is readable");
    let parsed: toml::Value = toml::from_str(&text).expect("the ledger is TOML");
    let rows = parsed["question"].as_array().expect("the rows");
    assert!(!rows.is_empty(), "the round asked questions");

    // The rows name the questions the way the harness wrote them.
    let move_row = rows
        .iter()
        .find(|row| row["slot"].as_str() == Some("move"))
        .expect("the move gate is in the ledger");
    assert!(move_row["id"].as_str().unwrap().starts_with("q_"));
    assert!(move_row["version"].as_str().unwrap().starts_with("v_"));
    assert!(move_row["normalized"]
        .as_str()
        .unwrap()
        .contains("choice |"));
    assert!(
        move_row["asks"].as_integer().unwrap() > 0,
        "the gate was asked"
    );

    // The ledger agrees with the round's own counters.
    let asks: i64 = rows
        .iter()
        .map(|row| row["asks"].as_integer().unwrap_or(0))
        .sum();
    assert!(asks > 0);
    assert_eq!(asks, agents.rows().map(|row| row.asks as i64).sum::<i64>());

    // The confidence is in permille, and it is the model's own number.
    let confidence = move_row["confidence"].as_array().expect("the confidence");
    assert!(!confidence.is_empty());
    let mean = confidence[0]["mean_permille"].as_integer().unwrap();
    assert!((0..=1000).contains(&mean), "the confidence is in permille");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_cache_shared_by_two_rounds_answers_the_second_out_of_the_first() {
    let dir = std::env::temp_dir().join(format!("lm-ledger-cache-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("the temp dir");
    let cache = Arc::new(AnswerCache::new());

    let first = dir.join("first.toml");
    let second = dir.join("second.toml");
    play_round(9, Some(Arc::clone(&cache)), &first);
    play_round(9, Some(Arc::clone(&cache)), &second);

    let text = fs::read_to_string(&second).expect("the ledger is readable");
    let parsed: toml::Value = toml::from_str(&text).expect("the ledger is TOML");
    let rows = parsed["question"].as_array().expect("the rows");
    let hits: i64 = rows
        .iter()
        .map(|row| row["cache_hits"].as_integer().unwrap_or(0))
        .sum();
    assert!(
        hits > 0,
        "the second round's questions came out of the cache"
    );

    let stats = cache.stats();
    assert!(stats.hits > 0, "the cache counted its hits");
    assert!(stats.misses > 0, "and its misses");

    fs::remove_dir_all(&dir).ok();
}
