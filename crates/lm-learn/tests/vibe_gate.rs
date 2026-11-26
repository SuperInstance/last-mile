//! The vibe gate's measurement: the fixture is the measurement it claims
//! to be (offline, always run), and the table on the Thor (live, ignored).
//!
//! The fixture (`vibe-gate-phrasings.toml`) holds 55 kid-style phrasings:
//! 10 per dictionary entry, 10 that match nothing, and 5 honestly
//! ambiguous ones. The live test runs every case through the vibe gate
//! against the decision server the pack names and prints the accuracy
//! table that docs/design/vibe-gate.md carries:
//!
//! ```text
//! cargo test --release -p lm-learn --test vibe_gate \
//!   the_vibe_gate_table_on_the_thor -- --ignored --nocapture
//! ```

use lm_agents::config::AgentsConfig;
use lm_agents::sysone::{SysOne, ThorBackend};
use lm_learn::hear::{hear, GatePack, HearError, WORDS_MAX};
use lm_learn::vibe::Dictionary;
use serde::Deserialize;

/// The fixture, embedded so the offline shape test cannot drift from the
/// file the live test reads.
const FIXTURE: &str = include_str!("vibe-gate-phrasings.toml");

/// The command that reproduces the live table.
pub const REPRODUCE: &str = "cargo test --release -p lm-learn --test vibe_gate \
     the_vibe_gate_table_on_the_thor -- --ignored --nocapture";

/// One labelled phrasing.
#[derive(Clone, Debug, Deserialize)]
struct Case {
    /// What the kid says.
    words: String,
    /// The one right entry, or `none` when the dictionary holds nothing
    /// for the words.
    #[serde(default)]
    entry: Option<String>,
    /// The entries a fair hearing could pick when the words are honestly
    /// ambiguous; anything else the gate picks is a confusion.
    #[serde(default)]
    any: Vec<String>,
}

impl Case {
    /// The set the case belongs to, for the table's rows: an entry's name,
    /// `none`, or `ambiguous`.
    fn set(&self) -> String {
        match &self.entry {
            Some(entry) => entry.clone(),
            None => "ambiguous".to_owned(),
        }
    }

    /// Every answer that counts as right.
    fn wanted(&self) -> Vec<String> {
        match (&self.entry, &self.any) {
            (Some(entry), _) => vec![entry.clone()],
            (None, any) => any.clone(),
        }
    }

    /// What the case wanted, in one word for the confusion column.
    fn wanted_label(&self) -> String {
        match (&self.entry, &self.any) {
            (Some(entry), _) => entry.clone(),
            (None, any) => any.join("|"),
        }
    }
}

/// The fixture's shape: a table of `[[case]]` rows.
#[derive(Debug, Deserialize)]
struct Fixture {
    #[serde(rename = "case")]
    cases: Vec<Case>,
}

/// The fixture, parsed. It is data: if it does not read, nothing measures.
fn cases() -> Vec<Case> {
    toml::from_str::<Fixture>(FIXTURE)
        .expect("the fixture reads")
        .cases
}

/// The fixture is the measurement it claims to be: 10 phrasings per
/// dictionary entry, 10 that match nothing, a few honestly ambiguous, all
/// distinct, all short enough for one hearing, and every entry they name
/// is one the dictionary holds.
#[test]
fn the_fixture_is_the_measurement_it_claims_to_be() {
    let cases = cases();
    assert_eq!(cases.len(), 55, "the fixture holds 55 cases");

    let dict = Dictionary::default_dictionary().expect("the dictionary loads");
    let per_entry = [
        "unit.vision",
        "unit.speed",
        "unit.tag_range",
        "unit.stun_time",
    ];
    for entry in per_entry {
        let count = cases
            .iter()
            .filter(|case| case.entry.as_deref() == Some(entry))
            .count();
        assert_eq!(count, 10, "{entry} wants exactly 10 phrasings");
    }
    let none = cases
        .iter()
        .filter(|case| case.entry.as_deref() == Some("none"))
        .count();
    assert_eq!(none, 10, "exactly 10 phrasings match nothing");
    let ambiguous = cases.iter().filter(|case| case.entry.is_none()).count();
    assert!(
        (3..=8).contains(&ambiguous),
        "a few ambiguous cases, not {ambiguous}"
    );

    // Every name a case wants is a name the gate could answer with.
    for case in &cases {
        for wanted in case.wanted() {
            if wanted != "none" {
                assert!(
                    dict.find(&wanted).is_some(),
                    "the fixture names `{wanted}`, which the dictionary does not hold"
                );
            }
        }
    }

    // Every phrasing is a phrasing: said once, short enough to hear.
    let mut seen: Vec<&str> = Vec::new();
    for case in &cases {
        let words = case.words.trim();
        assert!(!words.is_empty(), "a case says nothing");
        assert!(
            words.chars().count() <= WORDS_MAX,
            "\"{words}\" is too long for one hearing"
        );
        assert!(
            !seen.contains(&words),
            "\"{words}\" is said twice: one phrasing, one case"
        );
        seen.push(words);
    }
}

/// What one case's hearing came to.
#[derive(Clone, Debug)]
enum Outcome {
    /// A proposal: the entry (and the step, with both confidences).
    Heard(String, String, f32, f32),
    /// The gate refused: the words ask for nothing the dictionary holds.
    NoMatch,
    /// The gate refused: unsure, under the floor.
    Unsure,
    /// Some other refusal, named.
    Other(String),
}

/// The tally of one run: the table's numbers, and every confusion.
#[derive(Default)]
struct Tally {
    /// (set, right) for every case that heard the change it meant.
    right: Vec<(String, String)>,
    /// (set, wanted, got) for every case that heard something else.
    wrong: Vec<(String, String, String)>,
    /// (set, wanted) for every case the gate refused as unsure.
    unsure: Vec<(String, String)>,
    /// (set, refusal) for the other refusals.
    other: Vec<(String, String)>,
    /// The latency of every entry question and every value question, ms.
    entry_ms: Vec<u32>,
    value_ms: Vec<u32>,
}

impl Tally {
    fn count_right(&self, set: &str) -> usize {
        self.right.iter().filter(|(s, _)| s == set).count()
    }

    fn count_of(&self, set: &str) -> usize {
        self.count_right(set)
            + self.wrong.iter().filter(|(s, _, _)| s == set).count()
            + self.unsure.iter().filter(|(s, _)| s == set).count()
            + self.other.iter().filter(|(s, _)| s == set).count()
    }

    /// The confusions of one set, as "wanted -> got" pairs.
    fn confusions(&self, set: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .wrong
            .iter()
            .filter(|(s, _, _)| s == set)
            .map(|(_, wanted, got)| format!("{wanted} -> {got}"))
            .collect();
        out.extend(
            self.unsure
                .iter()
                .filter(|(s, _)| s == set)
                .map(|(_, wanted)| format!("{wanted} -> unsure")),
        );
        out.extend(
            self.other
                .iter()
                .filter(|(s, _)| s == set)
                .map(|(_, why)| why.split(" — ").next().unwrap_or(why).to_owned()),
        );
        out.sort();
        out.dedup();
        out
    }
}

/// The mean of a latency list, in ms. It is a measurement of a box, not a
/// number the game depends on.
fn mean_ms(ms: &[u32]) -> u32 {
    if ms.is_empty() {
        return 0;
    }
    ms.iter().map(|m| u64::from(*m)).sum::<u64>() as u32 / ms.len() as u32
}

/// The box's load average, if the box says (Linux: /proc/loadavg).
fn load_average() -> String {
    match std::fs::read_to_string("/proc/loadavg") {
        Ok(text) => text.split_whitespace().next().unwrap_or("?").to_owned(),
        Err(_) => "?".to_owned(),
    }
}

/// The vibe gate's accuracy table, measured live on the Thor's decision
/// server (d1-2B :8094, the model the shipped pack names).
///
/// Opt-in, like every live test: the Thor is a box on the LAN, not a
/// dependency of the build. Prints the table docs/design/vibe-gate.md
/// carries, and holds the lane's floors: a gate that cannot hear three
/// quarters of the labelled phrasings, or that matches six of the ten
/// no-match asks to changes that do not exist, is a regression in the
/// pack, whatever the model did.
#[test]
#[ignore]
fn the_vibe_gate_table_on_the_thor() {
    let dict = Dictionary::default_dictionary().expect("the dictionary loads");
    let pack = GatePack::default_pack().expect("the policy pack loads");
    let config = AgentsConfig::default();
    let port = config
        .ports
        .get(&pack.model)
        .unwrap_or_else(|| panic!("the config has no port for {}", pack.model));
    let backend = ThorBackend::new(&config.thor_base, *port, &pack.model)
        .with_min_confidence(pack.min_confidence)
        .with_timeout(pack.timeout);

    let cases = cases();
    println!(
        "==> the vibe gate on {}:{} ({}), {} cases, load average {} when this ran",
        config.thor_base,
        port,
        backend.name(),
        cases.len(),
        load_average(),
    );

    let mut tally = Tally::default();
    for case in &cases {
        let outcome = match hear(&backend, &pack, &dict, &case.words) {
            Ok(heard) => {
                tally.entry_ms.push(heard.latency_ms[0]);
                tally.value_ms.push(heard.latency_ms[1]);
                Outcome::Heard(
                    heard.entry.clone(),
                    format!(
                        "{} = {} \"{}\"",
                        heard.entry, heard.step.value, heard.step.say
                    ),
                    heard.entry_confidence,
                    heard.value_confidence,
                )
            }
            Err(HearError::NoMatch) => Outcome::NoMatch,
            Err(HearError::EntryUnsure { .. }) | Err(HearError::ValueUnsure { .. }) => {
                Outcome::Unsure
            }
            Err(other) => Outcome::Other(other.to_string()),
        };
        let (set, wanted) = (case.set(), case.wanted_label());
        match &outcome {
            Outcome::Heard(entry, proposal, entry_conf, value_conf) => {
                println!(
                    "  {:<52} -> {:<44} ({:.2}/{:.2})",
                    format!("\"{}\"", case.words),
                    proposal,
                    entry_conf,
                    value_conf
                );
                if case.wanted().contains(entry) {
                    tally.right.push((set, wanted.clone()));
                } else {
                    tally.wrong.push((set, wanted.clone(), entry.clone()));
                }
            }
            Outcome::NoMatch => {
                println!("  {:<52} -> none", format!("\"{}\"", case.words));
                if case.wanted().iter().any(|w| w == "none") {
                    tally.right.push((set, wanted.clone()));
                } else {
                    tally.wrong.push((set, wanted.clone(), "none".to_owned()));
                }
            }
            Outcome::Unsure => {
                println!(
                    "  {:<52} -> unsure (under the floor)",
                    format!("\"{}\"", case.words)
                );
                tally.unsure.push((set, wanted));
            }
            Outcome::Other(why) => {
                println!("  {:<52} -> {why}", format!("\"{}\"", case.words));
                tally.other.push((set, why.clone()));
            }
        }
    }

    // The table.
    println!("\n| set | right | of | unsure | wrong (heard instead) |");
    println!("|---|---|---|---|---|");
    for set in [
        "unit.vision",
        "unit.speed",
        "unit.tag_range",
        "unit.stun_time",
        "none",
        "ambiguous",
    ] {
        let of = tally.count_of(set);
        if of == 0 {
            continue;
        }
        let confusions = tally.confusions(set);
        println!(
            "| {set} | {} | {of} | {} | {} |",
            tally.count_right(set),
            tally.unsure.iter().filter(|(s, _)| s == set).count(),
            if confusions.is_empty() {
                "—".to_owned()
            } else {
                confusions.join(", ")
            }
        );
    }

    let labelled: [&str; 4] = [
        "unit.vision",
        "unit.speed",
        "unit.tag_range",
        "unit.stun_time",
    ];
    let labelled_right: usize = labelled.iter().map(|set| tally.count_right(set)).sum();
    let labelled_of: usize = labelled.iter().map(|set| tally.count_of(set)).sum();
    let refusals = tally.unsure.len() + tally.other.len();
    println!(
        "\nthe vibe gate measured: {labelled_right}/{labelled_of} labelled right, \
         {}/10 none right, {}/5 ambiguous accepted, {refusals} refusals of {} cases; \
         mean latency {} ms (entry) / {} ms (value) over the answered cases",
        tally.count_right("none"),
        tally.count_right("ambiguous"),
        cases.len(),
        mean_ms(&tally.entry_ms),
        mean_ms(&tally.value_ms),
    );
    println!("reproduce: {REPRODUCE}");

    // The lane's floors: catch a pack that stopped hearing, without
    // hanging the lane on one model's off day.
    assert!(
        labelled_right * 4 >= labelled_of * 3,
        "the gate heard {labelled_right} of {labelled_of} labelled phrasings"
    );
    assert!(
        tally.count_right("none") >= 6,
        "the gate matched {} of the 10 no-match asks to changes that do not exist",
        10 - tally.count_right("none")
    );
}
