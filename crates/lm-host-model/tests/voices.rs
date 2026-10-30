//! The agents' voices in the host, wired (D73): the plan prefetches, the
//! airtime looks up, and a line that is not ready plays the tone — never
//! a wait.
//!
//! Every proof runs against the loopback mock skald daemon (`lm-voice`'s
//! `mocks` feature): no test here touches a network, and the live facts
//! keep their own lane (`scripts/live.sh`).
//!
//! * [`a_line_is_mapped_through_the_voice_map`] — the squad's voice and
//!   the score's style reach skald as the one ask;
//! * [`a_planned_line_renders_and_the_due_line_is_a_hit`] — the plan
//!   renders ahead, and the airtime plays the WAV the service answered;
//! * [`a_held_render_is_pending_and_plays_the_tone`] — the fallback
//!   contract while the worker is busy;
//! * [`a_line_nobody_planned_is_a_miss_tone`] — and when nobody planned;
//! * [`the_host_prefetches_at_the_plan_and_plays_at_the_airtime`] — the
//!   whole path in the host loop, with the stub minds of the shipped
//!   harness;
//! * [`the_prewarm_renders_the_phrasebook_s_lines_into_the_bank`] — the
//!   bank is warm before the round's own lines arrive;
//! * [`the_bank_is_a_per_user_cache`] — where the bank lives, and whose
//!   word wins.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lm_agents::config::AgentsConfig;
use lm_agents::harness::Harness;
use lm_agents::mind::Policy;
use lm_agents::sysone::{StubBackend, SysOne};
use lm_agents::Agents;
use lm_core::header::{Header, Role, RosterEntry};
use lm_core::utterance::{
    Addressee, Affect, Channel, Intent, Prosody, SpeechAct, Subject, Utterance, Word,
};
use lm_host_model::host_loop::HostLoop;
use lm_host_model::talk::{Listener, TalkConfig};
use lm_host_model::voices::{
    prewarm, resolve_bank_root, speech_of, squad_voices, user_bank_root, Voices, VoicesSettings,
};
use lm_tools::runner;
use lm_voice::mockhttp::{MockSkald, Plan};
use lm_voice::voicemap::VoiceMap;

/// The shipped harness, read into memory: the stub minds think with it.
const DEFAULT_GATES: &str = include_str!("../../../assets/harness/default/gates.toml");
const DEFAULT_POLICY: &str = include_str!("../../../assets/harness/default/policy.toml");
const DEFAULT_PHRASEBOOK: &str = include_str!("../../../assets/harness/default/phrasebook.toml");

// ------------------------------------------------------------------ helpers

/// The WAV the mock answers with: half a second of a wave whose every
/// sample is a marker, so a hit is provably the service's clip and not
/// the stub's tone (the tone's samples are a ramp that crosses zero).
fn mock_wav() -> Vec<u8> {
    lm_voice::prosody::wav_bytes(&vec![12_345i16; 22_050 / 2], 22_050)
}

/// A fresh bank directory of this test's own.
fn bank_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lm-voices-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// The voices over a mock daemon and a fresh bank: the map names only the
/// daemon (on the mock's own port) and the speakers, so the styles come
/// from the map's own defaults.
fn voices_over(tag: &str, plan: Plan) -> (MockSkald, Arc<Voices>) {
    let mock = MockSkald::start(plan);
    let map = VoiceMap::parse(&format!(
        "[skald]\nbase = \"http://127.0.0.1\"\nport = {}\nengine_id = \"engine-test\"\n\
         [agents]\ndefault = \"mia\"\n\"4\" = \"aria\"\n\
         [styles.tone]\ncalm = \"Neutral\"\nurgent = \"Angry\"\nuncertain = \"Calm\"",
        mock.port()
    ))
    .expect("the map parses");
    let voices = Voices::open(VoicesSettings {
        map,
        bank_root: bank_dir(tag),
        cap_bytes: None,
        base: None,
        port: None,
    })
    .expect("the voices open");
    (mock, voices)
}

/// A line an agent speaks, as the mind planned it.
fn a_line(speaker: u16, affect: Affect, intent: Intent) -> Utterance {
    Utterance {
        speaker,
        channel: Channel::Radio,
        addressee: Addressee::Squad,
        act: SpeechAct::Command,
        intent,
        affect,
        subject: Subject::Beacon(2),
        prosody: Prosody::Synthesized,
        words: vec![
            Word::new("hold", 0, 10),
            Word::new("beacon", 10, 20),
            Word::new("two", 20, 30),
        ],
    }
}

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

/// The round's agents, thinking on the stub.
fn stub_agents(header: &Header) -> Agents {
    let harness = default_harness();
    let policy = Policy::for_harness(&harness.policy_toml);
    let backends: BTreeMap<String, Box<dyn SysOne + Send + Sync>> = policy
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
        .collect();
    Agents::new(&AgentsConfig::default(), &harness, header, backends)
}

/// Clean up after a test's mock and bank.
fn clean_up(mock: MockSkald, voices: &Arc<Voices>) {
    mock.join();
    let root = voices.bank().root().to_owned();
    let _ = std::fs::remove_dir_all(&root);
}

// ------------------------------------------------------------------- proofs

/// The voices live in an ECS resource of the host, beside the round: they
/// have to be shareable between the systems that read them and the
/// pre-warm thread that warms them.
#[test]
fn the_voices_can_be_shared_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Voices>();
}

#[test]
fn a_line_is_mapped_through_the_voice_map() {
    let map = VoiceMap::shipped();
    // The speaker per agent: squad 0's leader is mia, squad 1's is aria,
    // and a member falls to the default.
    let urgent_order = speech_of(&map, &a_line(0, Affect::Urgent, Intent::Order));
    assert_eq!(urgent_order.voice, "mia");
    assert_eq!(urgent_order.style, "Angry", "urgent is the pressed style");
    assert_eq!(urgent_order.text, "hold beacon two");
    let squad_one = speech_of(&map, &a_line(4, Affect::Urgent, Intent::Order));
    assert_eq!(squad_one.voice, "aria");
    let member = speech_of(&map, &a_line(1, Affect::Urgent, Intent::Order));
    assert_eq!(member.voice, "jason");
    // The tone chooses the style, and the calm line is the everyday voice.
    let calm = speech_of(&map, &a_line(0, Affect::Calm, Intent::Report));
    assert_eq!(calm.style, "Neutral");
    // The HUD's legend: each squad's voices, in squad order.
    let roster: Vec<RosterEntry> = (0..8u16)
        .map(|agent| RosterEntry {
            agent,
            squad: u8::try_from(agent / 4).unwrap_or(0),
            role: if agent % 4 == 0 {
                Role::Leader
            } else {
                Role::Member
            },
            voice: "default".to_owned(),
            harness_commit: "none".to_owned(),
            reflex: "default".to_owned(),
        })
        .collect();
    assert_eq!(
        squad_voices(&map, &roster),
        vec![(0, "mia+jason".to_owned()), (1, "aria+jason".to_owned())]
    );
}

#[test]
fn a_planned_line_renders_and_the_due_line_is_a_hit() {
    let (mock, voices) = voices_over("hit", Plan::Wav(mock_wav()));
    let line = a_line(0, Affect::Urgent, Intent::Order);

    // The plan: the submit happens the moment the host knows the line.
    voices.prefetch(&line);
    assert!(
        voices.queue_stats().submitted >= 1,
        "the line reached the worker"
    );

    // The render lands (a test's wait is a report's question, never the
    // game's — the game would have looked the line up and moved on).
    assert!(voices.wait(Duration::from_secs(5)), "the worker drains");

    // The airtime: the clip is the service's own WAV, and the ask rides
    // along with it.
    let (clip, spoken) = voices.speak(&line);
    assert_eq!(spoken.origin, lm_host_model::voices::SpokenOrigin::Hit);
    assert_eq!(spoken.voice, "mia");
    assert_eq!(spoken.style, "Angry");
    assert!(
        spoken.latency_ms.is_some(),
        "a hit carries its render's latency"
    );
    assert_eq!(clip.sample_rate, 22_050, "the daemon's rate");
    assert!(
        clip.pcm.iter().all(|&sample| sample == 12_345),
        "the samples are the service's, not the tone's ramp"
    );
    assert_eq!(voices.counters().hits, 1);
    assert_eq!(voices.counters().tones, 0);
    // And the daemon was asked for the canonical id: the style rides in
    // the voice id, as the service wants it.
    let asks = mock.requests();
    assert!(!asks.is_empty(), "the daemon was asked");
    assert!(
        asks[0].contains("Magpie-Multilingual.EN-US.Mia.Angry"),
        "the ask: {}",
        asks[0]
    );
    clean_up(mock, &voices);
}

#[test]
fn a_held_render_is_pending_and_plays_the_tone() {
    let (mock, voices) = voices_over("pending", Plan::Wav(mock_wav()));
    mock.hold();
    let line = a_line(0, Affect::Calm, Intent::Report);
    voices.prefetch(&line);

    // Due while the worker holds the render: the tone, the pending count,
    // and never a wait.
    let (clip, spoken) = voices.speak(&line);
    assert_eq!(
        spoken.origin,
        lm_host_model::voices::SpokenOrigin::PendingTone
    );
    assert!(spoken.latency_ms.is_none(), "no clip, no render number");
    assert_eq!(clip.sample_rate, 16_000, "the tone is the stub's wave");
    let counters = voices.counters();
    assert_eq!(counters.tones, 1);
    assert_eq!(counters.pending, 1);
    assert_eq!(counters.hits, 0);

    // Released, the line lands, and the next due is the real clip.
    mock.release();
    assert!(voices.wait(Duration::from_secs(5)), "the held render lands");
    let (clip, spoken) = voices.speak(&line);
    assert_eq!(spoken.origin, lm_host_model::voices::SpokenOrigin::Hit);
    assert!(clip.pcm.iter().all(|&sample| sample == 12_345));
    let counters = voices.counters();
    assert_eq!(counters.hits, 1);
    assert_eq!(counters.tones, 1, "the earlier miss stays counted");
    clean_up(mock, &voices);
}

#[test]
fn a_line_nobody_planned_is_a_miss_tone() {
    let (mock, voices) = voices_over("miss", Plan::Wav(mock_wav()));
    let line = a_line(4, Affect::Calm, Intent::Ack);
    // No prefetch at all: the ear is due a line nobody submitted.
    let (clip, spoken) = voices.speak(&line);
    assert_eq!(spoken.origin, lm_host_model::voices::SpokenOrigin::MissTone);
    assert_eq!(clip.sample_rate, 16_000, "the tone");
    let counters = voices.counters();
    assert_eq!(counters.tones, 1);
    assert_eq!(counters.pending, 0, "nobody had it in flight");
    clean_up(mock, &voices);
}

#[test]
fn the_machine_s_own_address_wins_over_the_map() {
    // The map names a daemon that is not there; the machine's word (the
    // environment's lane, here the settings') names the mock. The settings
    // win, or every ProArt would have to edit the harness file to say
    // where the Thor is.
    let mock = MockSkald::start(Plan::Wav(mock_wav()));
    let map = VoiceMap::parse(
        "[skald]\nbase = \"http://127.0.0.1\"\nport = 1\nengine_id = \"engine-test\"\n\
         [agents]\ndefault = \"mia\"",
    )
    .expect("the map parses");
    let voices = Voices::open(VoicesSettings {
        map,
        bank_root: bank_dir("override"),
        cap_bytes: None,
        base: Some(mock.base()),
        port: Some(mock.port()),
    })
    .expect("the voices open");
    let line = a_line(0, Affect::Urgent, Intent::Order);
    voices.prefetch(&line);
    assert!(voices.wait(Duration::from_secs(5)), "the worker drains");
    let (clip, spoken) = voices.speak(&line);
    assert_eq!(spoken.origin, lm_host_model::voices::SpokenOrigin::Hit);
    assert!(clip.pcm.iter().all(|&sample| sample == 12_345));
    assert_eq!(mock.requests().len(), 1, "the mock was asked, not port 1");
    clean_up(mock, &voices);
}

/// The whole path in the host loop: the stub minds of the shipped harness
/// plan lines, the plans prefetch, and the clips that reach the player
/// carry their spoken origin — hit or tone, never a wait.
#[test]
fn the_host_prefetches_at_the_plan_and_plays_at_the_airtime() {
    let (mock, voices) = voices_over("hostloop", Plan::Wav(mock_wav()));
    let header = Header::default_round(1, runner::DEFAULT_REFLEX);
    let mut host = HostLoop::new(header.clone()).expect("the round starts");
    host.attach_agents(stub_agents(&header));
    // The person listens to squad 0's radio: the lines of the round reach
    // their ear.
    host.attach_talk(TalkConfig::default().with_voices(Arc::clone(&voices)));
    host.set_listener(Listener::Squad(0));

    let mut clips = 0usize;
    let mut spoken = 0usize;
    for _ in 0..600 {
        host.tick();
        for clip in host.take_clips() {
            clips += 1;
            let said = clip.spoken.expect("the skald path says how it was spoken");
            assert!(
                said.origin == lm_host_model::voices::SpokenOrigin::Hit
                    || said.origin == lm_host_model::voices::SpokenOrigin::PendingTone
                    || said.origin == lm_host_model::voices::SpokenOrigin::MissTone,
                "every due line is one of the three origins"
            );
            assert!(!said.voice.is_empty(), "the ask names its speaker");
            spoken += 1;
        }
    }
    // The round talked, the plans reached the worker, and the player heard
    // clips — how many of each is the round's own doing; THAT they are
    // nonzero is the wiring.
    let talk = host.talk_counters();
    assert!(talk.utterances > 0, "the stub round talks");
    assert!(
        voices.queue_stats().submitted >= 1,
        "a planned line reached the worker"
    );
    assert!(clips > 0, "the player heard clips ({clips})");
    assert_eq!(spoken, clips);
    assert_eq!(
        talk.voice_hits + talk.voice_tones,
        u64::try_from(clips).unwrap_or(u64::MAX),
        "the counters count exactly what was due"
    );
    // The legend the HUD colours by squad came from the map.
    assert_eq!(talk.squad_voices.len(), 2, "two squads, two legends");
    assert!(!mock.requests().is_empty(), "the daemon rendered");
    clean_up(mock, &voices);
}

/// The pre-warm: a background thread plays the round's seed with the stub
/// minds and asks the bank for every line it speaks, so the round's own
/// first lines find a warm bank.
#[test]
fn the_prewarm_renders_the_phrasebook_s_lines_into_the_bank() {
    let (mock, voices) = voices_over("prewarm", Plan::Wav(mock_wav()));
    let header = Header::default_round(1, runner::DEFAULT_REFLEX);
    prewarm(Arc::clone(&voices), header.clone(), 600)
        .join()
        .expect("the prewarm ran");
    // A test's wait is a report's question, never the game's.
    assert!(voices.wait(Duration::from_secs(10)), "the worker drains");

    // The bank is warm: the stub round's unique lines are rendered, and
    // every ask the daemon saw was a canonical magpie id.
    let bank_len = voices.bank().len();
    assert!(bank_len >= 1, "the prewarm rendered something");
    assert!(
        voices.queue_stats().rendered >= 1,
        "the worker rendered the asks"
    );
    for ask in mock.requests() {
        assert!(
            ask.contains("Magpie-Multilingual.EN-US."),
            "the daemon heard canonical ids: {ask}"
        );
    }

    // And the warm bank answers the same round's lines as hits: replay the
    // lines the prewarm played and look each one up.
    let lines = lm_tools::voice::stub_round_lines(&header, 600);
    assert!(!lines.is_empty(), "the stub round spoke");
    let mut hits = 0usize;
    for line in &lines {
        let (_, spoken) = voices.speak(line);
        if spoken.origin == lm_host_model::voices::SpokenOrigin::Hit {
            hits += 1;
        }
    }
    assert!(hits > 0, "a warm bank hits the round's own lines ({hits})");
    clean_up(mock, &voices);
}

/// Where the bank lives: the machine's per-user cache by default, the
/// map's root over that, and the command line's word over everything.
#[test]
fn the_bank_is_a_per_user_cache() {
    let root = user_bank_root();
    assert!(
        root.ends_with(std::path::Path::new("last-mile").join("voice-bank")),
        "the per-user default: {}",
        root.display()
    );
    // The map's word...
    let mut map = VoiceMap::shipped();
    map.bank.root = "/tmp/a-map-bank".to_owned();
    assert_eq!(
        resolve_bank_root(&map, None),
        Path::new("/tmp/a-map-bank"),
        "the map's root wins over the default"
    );
    // ... and the command line's word over the map's.
    assert_eq!(
        resolve_bank_root(&map, Some(Path::new("/tmp/a-cli-bank"))),
        Path::new("/tmp/a-cli-bank")
    );
    // The cap is the map's to say too, and it reaches the bank.
    map.bank.cap_mb = 1;
    let voices = Voices::open(VoicesSettings {
        map: map.clone(),
        bank_root: bank_dir("capped"),
        cap_bytes: Some(u64::from(map.bank.cap_mb) * 1024 * 1024),
        base: None,
        port: None,
    })
    .expect("the voices open");
    assert_eq!(voices.bank().cap_bytes(), Some(1024 * 1024));
    drop(voices);
    let _ = std::fs::remove_dir_all(bank_dir("capped"));
}
