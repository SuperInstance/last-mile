//! `lm qcache`: the answer cache's ceiling, measured on a pack corpus.
//!
//! The answer cache (`lm_agents::cache`) serves an answer when the same
//! question is asked about the same situation again. How often that
//! happens is the cache's **ceiling**: it is the share of asks that need
//! no model at all, and it is a property of the corpus, not of the cache.
//!
//! The corpus is the pack capture (`~/lm-batch/packs`): one JSONL file per
//! seed, one line per pack the agents actually asked. Every line is
//! `{"pack": "move", "questions": {…}, "state": "…"}` — the questions the
//! model was asked and the facts it was asked over.
//!
//! The measurement uses the **same** key the live cache uses:
//! `SHA-256(question id | version | context digest)`. The corpus carries no
//! harness, so the version term is the question's own content digest
//! rather than the harness's; the hit/miss partition is the same either
//! way, because the context digest already covers the question as asked.
//!
//! The report is per gate: how often each question recurs, which is what
//! the cache's hit rate would be for that gate.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lm_agents::question::{cache_key, context_digest, stamp_of};

/// What one gate's asks look like.
#[derive(Clone, Debug, Default)]
struct GateCounts {
    /// How many times the gate was asked.
    asks: u64,
    /// How many different (question, context) pairs the gate was asked with.
    distinct: u64,
    /// How many asks were a repeat of an earlier one.
    hits: u64,
}

/// What the corpus says.
#[derive(Clone, Debug, Default)]
struct Corpus {
    /// The files that were read.
    files: usize,
    /// The packs that were read.
    packs: u64,
    /// The questions that were asked.
    asks: u64,
    /// The different (question, context) pairs.
    distinct: u64,
    /// The asks that were a repeat of an earlier one.
    hits: u64,
    /// The packs where every question was a repeat.
    packs_all_hit: u64,
    /// The packs where every question was asked for the first time.
    packs_all_miss: u64,
    /// The per-gate counts.
    gates: BTreeMap<String, GateCounts>,
    /// The counts per pack kind (`move`, `talk`).
    kinds: BTreeMap<String, KindCounts>,
}

/// One pack kind's counts.
#[derive(Clone, Debug, Default)]
struct KindCounts {
    /// How many packs of this kind were read.
    packs: u64,
    /// How many questions were asked.
    asks: u64,
    /// How many asks were a repeat of an earlier one.
    hits: u64,
    /// How many packs were fully held.
    packs_all_hit: u64,
}

/// The `lm qcache` command's arguments.
#[derive(Clone, Debug)]
pub struct QcacheArgs {
    /// The corpus: a directory of `*.jsonl` pack files, or one file.
    pub packs: PathBuf,
    /// How many packs to print, per gate, in the detail.
    pub detail: usize,
}

/// Run the measurement.
pub fn cmd_qcache(args: &QcacheArgs) -> ExitCode {
    let files = match corpus_files(&args.packs) {
        Ok(files) => files,
        Err(err) => {
            eprintln!("lm: the corpus: {err}");
            return ExitCode::FAILURE;
        }
    };
    if files.is_empty() {
        eprintln!("lm: no packs under {}", args.packs.display());
        return ExitCode::FAILURE;
    }

    let mut corpus = Corpus::default();
    let mut seen: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for file in &files {
        match read_corpus(file, &mut seen, &mut corpus) {
            Ok(()) => corpus.files += 1,
            Err(err) => {
                eprintln!("lm: {}: {err}", file.display());
                return ExitCode::FAILURE;
            }
        }
    }

    // The per-gate counts, from the distinct keys.
    for (gate, keys) in &seen {
        let counts = corpus.gates.entry(gate.clone()).or_default();
        counts.asks = keys.values().sum();
        counts.distinct = keys.len() as u64;
        counts.hits = counts.asks.saturating_sub(counts.distinct);
    }
    corpus.asks = corpus.gates.values().map(|counts| counts.asks).sum();
    corpus.distinct = corpus.gates.values().map(|counts| counts.distinct).sum();
    corpus.hits = corpus.gates.values().map(|counts| counts.hits).sum();

    report(&corpus, &seen, args.detail);
    ExitCode::SUCCESS
}

/// The corpus's files.
fn corpus_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = Vec::new();
    let entries = fs::read_dir(path).map_err(|err| format!("{}: {err}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("{}: {err}", path.display()))?;
        let file = entry.path();
        if file.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(file);
        }
    }
    files.sort();
    Ok(files)
}

/// Read one file of packs into the counts.
fn read_corpus(
    file: &Path,
    seen: &mut BTreeMap<String, BTreeMap<String, u64>>,
    corpus: &mut Corpus,
) -> Result<(), String> {
    let reader =
        BufReader::new(fs::File::open(file).map_err(|err| format!("{}: {err}", file.display()))?);
    for (number, line) in reader.lines().enumerate() {
        let line = line.map_err(|err| format!("{}:{}: {err}", file.display(), number + 1))?;
        if line.trim().is_empty() {
            continue;
        }
        let pack: serde_json::Value = serde_json::from_str(&line)
            .map_err(|err| format!("{}:{}: {err}", file.display(), number + 1))?;
        corpus.packs += 1;
        let state = pack
            .get("state")
            .and_then(|s| s.as_str())
            .unwrap_or_default();
        let Some(questions) = pack.get("questions").and_then(|q| q.as_object()) else {
            continue;
        };
        let kind = pack
            .get("pack")
            .and_then(|p| p.as_str())
            .unwrap_or("unknown")
            .to_string();
        let mut hits = 0;
        let mut misses = 0;
        for (slot, question) in questions {
            let stamp = stamp_of(slot, question);
            let context = context_digest(state, question);
            let key = cache_key(&stamp.id, &stamp.version, &context);
            let gate = seen.entry(slot.clone()).or_default();
            let count = gate.entry(key).or_insert(0);
            if *count > 0 {
                hits += 1;
            } else {
                misses += 1;
            }
            *count += 1;
        }
        if hits > 0 && misses == 0 {
            corpus.packs_all_hit += 1;
        }
        if hits == 0 && misses > 0 {
            corpus.packs_all_miss += 1;
        }
        let counts = corpus.kinds.entry(kind).or_default();
        counts.packs += 1;
        counts.asks += (hits + misses) as u64;
        counts.hits += hits as u64;
        if hits > 0 && misses == 0 {
            counts.packs_all_hit += 1;
        }
    }
    Ok(())
}

/// Print the report.
fn report(corpus: &Corpus, seen: &BTreeMap<String, BTreeMap<String, u64>>, detail: usize) {
    println!("the pack corpus");
    println!("  files:          {}", corpus.files);
    println!("  packs:          {}", corpus.packs);
    println!("  questions:      {}", corpus.asks);
    println!("  distinct pairs: {}", corpus.distinct);
    println!(
        "  recurring asks: {} ({:.1}%)",
        corpus.hits,
        percent(corpus.hits, corpus.asks)
    );
    println!(
        "  packs all hit:  {} ({:.1}%)",
        corpus.packs_all_hit,
        percent(corpus.packs_all_hit, corpus.packs)
    );
    println!(
        "  packs all miss: {} ({:.1}%)",
        corpus.packs_all_miss,
        percent(corpus.packs_all_miss, corpus.packs)
    );
    println!();
    println!("per pack kind:");
    println!(
        "  {:<28} {:>8} {:>10} {:>8} {:>8}",
        "pack", "packs", "asks", "hits", "rate"
    );
    for (kind, counts) in &corpus.kinds {
        println!(
            "  {:<28} {:>8} {:>10} {:>8} {:>7.1}%",
            kind,
            counts.packs,
            counts.asks,
            counts.hits,
            percent(counts.hits, counts.asks)
        );
        println!(
            "  {:<28} {:>8} fully held packs: {} ({:.1}%)",
            "",
            "",
            counts.packs_all_hit,
            percent(counts.packs_all_hit, counts.packs)
        );
    }
    println!();
    println!("per gate: the cache's ceiling for each question");
    println!(
        "  {:<28} {:>8} {:>10} {:>8} {:>8}",
        "gate", "asks", "distinct", "hits", "rate"
    );
    for (gate, counts) in &corpus.gates {
        println!(
            "  {:<28} {:>8} {:>10} {:>8} {:>7.1}%",
            gate,
            counts.asks,
            counts.distinct,
            counts.hits,
            percent(counts.hits, counts.asks)
        );
    }
    if detail > 0 {
        println!();
        println!("the most repeated pairs:");
        let mut rows: Vec<(String, String, u64)> = Vec::new();
        for (gate, keys) in seen {
            for (key, count) in keys {
                if *count > 1 {
                    rows.push((gate.clone(), key.clone(), *count));
                }
            }
        }
        rows.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
        for (gate, key, count) in rows.iter().take(detail) {
            println!("  {gate:<28} {count:>4}x {key}");
        }
    }
}

/// A percentage, to one decimal.
fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        (part as f64) * 100.0 / (whole as f64)
    }
}
