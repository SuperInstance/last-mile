//! The hearing physics: how far a line carries, what the distance and the
//! walls take from it, and what the listener is left with.
//!
//! The score is the truth; the *heard* copy is the truth as degraded
//! (research/09). The log carries the score and replays exactly; the
//! degradation is recomputed from the world, so it is reproducible too.
//! Nothing is ever inferred from audio.
//!
//! Every quantity here is an integer: the distances are in 1/1024 m units
//! and the clarity is a Q16 fraction (65 536 is 1.0). There is no float, no
//! clock and no random source — the radio's word dropouts are a *hash*, so
//! the same line drops the same words on every machine.
//!
//! The law, in one line: a line starts at full clarity, the distance takes
//! a share of it away, every wall takes half of what is left, and what
//! survives decides what the listener understands — the words first, then
//! the intent, then the gesture.
//!
//! The ladder a listener climbs, from the loudest line to the quietest:
//!
//! - at full clarity: the words, the gesture and the point;
//! - above `intent_clarity_q16`: the words and the point, but the gesture
//!   is gone — the listener knows the words and not the point;
//! - above `word_drop_q16`: the words alone;
//! - above `min_clarity_q16`: the line is there, and nothing in it;
//! - below that: nothing at all. The line was never spoken, as far as
//!   that listener is concerned.

use crate::decision::Tick;
use crate::fixed::{dist2, isqrt, Pos, UNITS_PER_M};
use crate::hash::{Fnv64, StateHash};
use crate::talk::TalkParams;
use crate::utterance::{Channel, Heard, Utterance, Word};
use crate::world::{Grid, World};

/// Q16: one whole.
pub const Q16_ONE: u32 = 65_536;

/// The world's default cell edge, in 1/1024 m units (2 m: the `cell_m`
/// default).
///
/// [`audible_to`] carries no cell edge of its own, so it assumes this one;
/// a caller that knows the round's `cell_m` uses [`audible_to_in_world`].
pub const DEFAULT_CELL_UNITS: i32 = 2 * UNITS_PER_M;

/// How far a channel carries, in 1/1024 m units.
///
/// The radio has no range: it reaches the speaker's squad through any
/// distance and any wall.
pub fn range_units(channel: Channel, p: &TalkParams) -> u32 {
    match channel {
        Channel::Shout => p.range_shout_units,
        Channel::Voice => p.range_voice_units,
        Channel::Whisper => p.range_whisper_units,
        Channel::Radio => u32::MAX,
    }
}

/// The clarity of a line at `dist_units` from the listener, in Q16.
///
/// The law:
/// - the radio is `65_536` — no distance, no walls;
/// - otherwise the clarity is `65_536 - (dist_units << 16) / range_units`,
///   so it reaches zero exactly at the channel's range;
/// - then every wall crossed multiplies it by `wall_loss_q16` (half, by
///   default), one wall at a time;
/// - a distance beyond the range is `0`: nothing is heard at all.
///
/// The arithmetic is in u64 so that a long distance cannot overflow the
/// shift; the result is a Q16 fraction and never leaves the range.
pub fn clarity(channel: Channel, dist_units: u32, walls: u8, p: &TalkParams) -> u32 {
    if channel == Channel::Radio {
        return Q16_ONE;
    }
    let range = range_units(channel, p);
    if range == 0 || dist_units >= range {
        return 0;
    }
    // The distance's share of the clarity: the whole range is one whole.
    let loss = (u64::from(dist_units) << 16) / u64::from(range);
    let mut clarity = u64::from(Q16_ONE) - loss;
    for _ in 0..walls {
        clarity = (clarity * u64::from(p.wall_loss_q16)) >> 16;
    }
    clarity.min(u64::from(Q16_ONE)) as u32
}

/// The distance between two positions, in 1/1024 m units.
///
/// The straight-line distance, rounded down to the unit: the same number on
/// every machine, with no square root in a float.
pub fn dist_units(a: Pos, b: Pos) -> u32 {
    let squared = dist2(a, b);
    let units = isqrt(squared as u64);
    u32::try_from(units).unwrap_or(u32::MAX)
}

/// The clarity of a line between two positions, walls included.
pub fn clarity_between(
    channel: Channel,
    speaker: Pos,
    listener: Pos,
    grid: &Grid,
    cell_units: i32,
    p: &TalkParams,
) -> u32 {
    let walls = crate::world::los::walls_between(
        grid,
        grid.cell_of(speaker, cell_units),
        grid.cell_of(listener, cell_units),
    );
    clarity(channel, dist_units(speaker, listener), walls, p)
}

/// The listener's copy of a line that starts at tick zero.
///
/// The copy is the score as degraded: the words that survive, with their
/// lanes flattened by the distance, and the tags the listener understood.
/// `degrade` does not know the clock, so the copy it returns is not
/// *ongoing*; the sim, which does know the tick, says so.
pub fn degrade(u: &Utterance, clarity_q16: u32, listener: u16, p: &TalkParams) -> Heard {
    degrade_at(u, 0, clarity_q16, listener, p)
}

/// The listener's copy of a line that starts at `started_tick`.
///
/// The words keep the score's order and their own lanes, degraded:
///
/// - `energy_q16' = energy_q16 * clarity >> 16`; a word whose degraded
///   energy falls below `word_drop_q16` is **dropped** — the listener hears
///   the line without that word;
/// - `pitch_steps'` is the pitch flattened toward zero by the ratio
///   `clarity / clarity_pitch_q16` when the clarity is below it; on the
///   radio the pitch is squeezed to `radio_pitch_steps` either side of the
///   baseline;
/// - the pace and the pauses survive above `clarity_rate_q16`; below it the
///   pace is lost (the words come out at the neutral rate) and the rests
///   are gone;
/// - `act` and `intent` are `Some` only above `act_clarity_q16` and
///   `intent_clarity_q16`: below them the listener **knows the words but
///   not the point**;
/// - the radio drops a word deterministically when
///   `fnv1a(id, word_index, listener) % 65_536 < radio_drop_q16` — a hash,
///   never a random number, so the dropout replays.
pub fn degrade_at(
    u: &Utterance,
    started_tick: Tick,
    clarity_q16: u32,
    listener: u16,
    p: &TalkParams,
) -> Heard {
    let id = u.state_hash();
    let mut words: Vec<Word> = Vec::new();
    for (index, word) in u.words.iter().enumerate() {
        // The radio's dropout comes first: it is the channel's own price,
        // and it is decided by the hash, not by the loudness.
        if u.channel == Channel::Radio && radio_drops(id, index, listener, p) {
            continue;
        }
        let heard_energy = (u64::from(word.energy_q16) * u64::from(clarity_q16)) >> 16;
        if heard_energy < u64::from(p.word_drop_q16) {
            continue;
        }
        let mut degraded = word.clone();
        degraded.energy_q16 = u32::try_from(heard_energy).unwrap_or(u32::MAX);
        degraded.pitch_steps = degraded_pitch(word.pitch_steps, clarity_q16, u.channel, p);
        if clarity_q16 < p.clarity_rate_q16 {
            // The pace is lost: the words come out at the neutral rate and
            // the rests between them are gone.
            degraded.rate_q16 = Q16_ONE;
            degraded.pause_after_ticks = 0;
        }
        // The words sit on the round's timeline, not on the line's own.
        degraded.t_on += started_tick;
        degraded.t_off += started_tick;
        words.push(degraded);
    }

    Heard {
        id,
        speaker: u.speaker,
        channel: u.channel,
        addressee: u.addressee,
        act: if clarity_q16 >= p.act_clarity_q16 {
            Some(u.act)
        } else {
            None
        },
        intent: if clarity_q16 >= p.intent_clarity_q16 {
            Some(u.intent)
        } else {
            None
        },
        affect: u.affect,
        subject: u.subject,
        clarity_q16,
        spoken_words: u16::try_from(u.words.len()).unwrap_or(u16::MAX),
        words,
        started_tick,
        ended_tick: started_tick + u64::from(u.duration_ticks()),
        ongoing: false,
    }
}

/// The pitch a listener hears: flattened toward zero by the clarity, and
/// squeezed to the radio's own narrow band.
fn degraded_pitch(pitch_steps: i8, clarity_q16: u32, channel: Channel, p: &TalkParams) -> i8 {
    let mut steps = i32::from(pitch_steps);
    if clarity_q16 < p.clarity_pitch_q16 {
        let factor = i32::try_from(clarity_q16).unwrap_or(0);
        let full = i32::try_from(p.clarity_pitch_q16).unwrap_or(1).max(1);
        steps = steps * factor / full;
    }
    if channel == Channel::Radio {
        let cap = i32::from(p.radio_pitch_steps);
        steps = steps.clamp(-cap, cap);
    }
    i8::try_from(steps).unwrap_or(0)
}

/// Whether the radio drops this word for this listener.
///
/// The decision is a hash of the line, the word and the listener, so the
/// same line drops the same words on every run and on every machine — the
/// dropout replays, it is not a coin toss.
fn radio_drops(id: u64, word_index: usize, listener: u16, p: &TalkParams) -> bool {
    let mut h = Fnv64::new();
    h.u64(id);
    h.u32(u32::try_from(word_index).unwrap_or(u32::MAX));
    h.u16(listener);
    (h.finish() % u64::from(Q16_ONE)) < u64::from(p.radio_drop_q16)
}

/// Whether the listener can hear the line at all.
///
/// The radio reaches the speaker's squad, through any wall and over any
/// distance, and nobody else (unless the round says the radio is open);
/// every other channel is heard when the clarity clears
/// `min_clarity_q16`.
///
/// The cell edge is the world's default here — this signature carries none
/// — so a caller that knows the round's `cell_m` uses
/// [`audible_to_in_world`].
pub fn audible_to(
    u: &Utterance,
    speaker: Pos,
    listener: Pos,
    listener_squad: u8,
    speaker_squad: u8,
    grid: &Grid,
    p: &TalkParams,
) -> bool {
    audible_in(
        u,
        speaker,
        listener,
        listener_squad,
        speaker_squad,
        grid,
        DEFAULT_CELL_UNITS,
        p,
    )
}

/// Whether the listener can hear the line at all, in a round whose cell
/// edge is the world's.
pub fn audible_to_in_world(
    u: &Utterance,
    speaker: Pos,
    listener: Pos,
    listener_squad: u8,
    speaker_squad: u8,
    world: &World,
    p: &TalkParams,
) -> bool {
    audible_in(
        u,
        speaker,
        listener,
        listener_squad,
        speaker_squad,
        &world.grid,
        world.cell_units,
        p,
    )
}

/// The shared body of the audibility queries: the radio is the squad's
/// channel, every other channel is heard when the clarity clears the
/// floor.
#[allow(clippy::too_many_arguments)] // a query is a list of facts, not a bundle
fn audible_in(
    u: &Utterance,
    speaker: Pos,
    listener: Pos,
    listener_squad: u8,
    speaker_squad: u8,
    grid: &Grid,
    cell_units: i32,
    p: &TalkParams,
) -> bool {
    if u.channel == Channel::Radio {
        // The radio is the squad's channel: it carries through the walls and
        // over any distance, but only to the squad.
        return !p.radio_is_squad || listener_squad == speaker_squad;
    }
    let clarity = clarity_between(u.channel, speaker, listener, grid, cell_units, p);
    clarity >= p.min_clarity_q16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utterance::{Addressee, Affect, Intent, Prosody, SpeechAct, Subject};
    use crate::world::grid::{CellKind, CellXY, Grid};

    /// A four-word order, one word per beat.
    fn line() -> Utterance {
        Utterance {
            speaker: 0,
            channel: Channel::Voice,
            addressee: Addressee::Squad,
            act: SpeechAct::Command,
            intent: Intent::Order,
            affect: Affect::Urgent,
            subject: Subject::Beacon(1),
            prosody: Prosody::Synthesized,
            words: vec![
                Word::new("hold", 0, 10).with_pitch(3),
                Word::new("the", 10, 15),
                Word::new("north", 15, 30).with_pitch(-3),
                Word::new("beacon", 30, 40).with_pause(5),
            ],
        }
    }

    /// The same line, spoken at double the nominal loudness.
    ///
    /// The energy lane is a Q16 fraction of the speaker's loudness, and
    /// the score may carry more than one: a shout is louder than a voice.
    fn loud_line() -> Utterance {
        let mut u = line();
        for word in &mut u.words {
            word.energy_q16 = 2 * Q16_ONE;
        }
        u
    }

    /// 24.0 m in 1/1024 m units.
    const AT_24_M: u32 = 24_576;
    /// 24.5 m in 1/1024 m units.
    const AT_24_5_M: u32 = 25_088;

    #[test]
    fn voice_reaches_a_squad_mate_within_24_metres() {
        let p = TalkParams::default();

        // At 24.0 m the voice is still clear enough to be heard: the
        // clarity is 2_622, above the floor of 1_536.
        let at_24 = clarity(Channel::Voice, AT_24_M, 0, &p);
        assert_eq!(at_24, 2_622);
        assert!(at_24 > p.min_clarity_q16);

        // And the words of the line arrive. The arithmetic, in Q16: at
        // 24 m the clarity is 2_622 (4 % of a whole) and the word-drop
        // threshold is 3_276 (5 %), so a word spoken at the nominal
        // loudness of 1.0 lands on 2_622 — below the threshold, and gone
        // (see `a_far_listener_loses_the_words`). A line spoken at double
        // the nominal loudness lands on 5_244, above it, and its words
        // arrive.
        let heard = degrade(&loud_line(), at_24, 7, &p);
        assert!(
            !heard.words.is_empty(),
            "the words must arrive at 24 m: {heard:?}"
        );

        // Half a metre further the voice is below the floor of hearing:
        // the listener hears nothing at all.
        let at_24_5 = clarity(Channel::Voice, AT_24_5_M, 0, &p);
        assert_eq!(at_24_5, 1_311);
        assert!(at_24_5 < p.min_clarity_q16);
    }

    #[test]
    fn a_wall_halves_the_clarity() {
        let p = TalkParams::default();
        let open = clarity(Channel::Voice, AT_24_M, 0, &p);
        let one_wall = clarity(Channel::Voice, AT_24_M, 1, &p);
        let two_walls = clarity(Channel::Voice, AT_24_M, 2, &p);

        assert_eq!(one_wall, open / 2);
        assert_eq!(two_walls, open / 4);
        assert_eq!(one_wall, 1_311);
        assert_eq!(two_walls, 655);
    }

    #[test]
    fn radio_reaches_the_squad_through_walls() {
        let p = TalkParams::default();
        assert_eq!(clarity(Channel::Radio, AT_24_5_M, 0, &p), 65_536);
        assert_eq!(clarity(Channel::Radio, AT_24_5_M, 10, &p), 65_536);
        // And the radio has no range at all: it is the squad's channel.
        assert_eq!(clarity(Channel::Radio, 1_000_000, 0, &p), 65_536);
    }

    #[test]
    fn shout_outranges_voice_outranges_whisper() {
        let p = TalkParams::default();
        // 30 m in 1/1024 m units.
        let at_30_m = 30_720;

        let shout = clarity(Channel::Shout, at_30_m, 0, &p);
        assert!(shout >= p.min_clarity_q16, "a shout must reach 30 m");
        assert_eq!(shout, 32_768);
        assert_eq!(clarity(Channel::Voice, at_30_m, 0, &p), 0);
        assert_eq!(clarity(Channel::Whisper, at_30_m, 0, &p), 0);

        // The whisper reaches an arm's length, and no farther.
        assert!(clarity(Channel::Whisper, 4_096, 0, &p) >= p.min_clarity_q16);
        assert_eq!(clarity(Channel::Whisper, 5_120, 0, &p), 0);
    }

    #[test]
    fn a_distant_listener_loses_the_pitch() {
        let p = TalkParams::default();
        // Half the voice range: 12.5 m.
        let clarity = clarity(Channel::Voice, 12_800, 0, &p);
        assert_eq!(clarity, 32_768);
        assert!(clarity < p.clarity_pitch_q16);

        let heard = degrade(&line(), clarity, 7, &p);
        // The pitch is flattened toward zero, not lost: the word's +3
        // becomes +2.
        assert_eq!(heard.words[0].pitch_steps, 2);
        assert_eq!(heard.words[2].pitch_steps, -2);
        // The energy survives: the words are still there.
        assert_eq!(heard.words.len(), 4);
        assert!(heard.words[0].energy_q16 > 0);
    }

    #[test]
    fn a_far_listener_loses_the_words() {
        let p = TalkParams::default();
        // At the very edge of the voice range the clarity is 2_622 and the
        // words of a line spoken at the nominal loudness land on 2_622 —
        // below the drop threshold of 3_276: the listener hears the line,
        // and none of its words.
        let at_24 = clarity(Channel::Voice, AT_24_M, 0, &p);
        let heard = degrade(&line(), at_24, 7, &p);
        assert!(
            heard.words.is_empty(),
            "the words must be gone at the edge: {heard:?}"
        );
        assert_eq!(heard.spoken_words, 4);
        // An empty line, not a line of zero-energy words.
        assert_eq!(heard.words.len(), 0);
        assert_eq!(heard.clarity_q16, at_24);
    }

    #[test]
    fn a_murky_listener_hears_the_words_but_not_the_act() {
        let p = TalkParams::default();
        // Between the intent threshold (0.25) and the act threshold (0.5).
        let clarity = 20_000;
        assert!(clarity < p.act_clarity_q16);
        assert!(clarity >= p.intent_clarity_q16);

        let heard = degrade(&line(), clarity, 7, &p);
        assert_eq!(heard.act, None, "the gesture must not survive");
        assert_eq!(heard.intent, Some(Intent::Order), "the point survives");
        assert!(!heard.words.is_empty(), "the words survive");
    }

    #[test]
    fn radio_dropout_is_deterministic() {
        let p = TalkParams::default();
        let mut u = line();
        u.channel = Channel::Radio;

        // The same line and the same listener drop the same words, every
        // time and on every machine: the dropout is a hash, not a draw.
        let first = degrade(&u, 65_536, 7, &p);
        for _ in 0..100 {
            let again = degrade(&u, 65_536, 7, &p);
            assert_eq!(first.words, again.words);
        }

        // The radio squeezes the pitch to its own narrow band.
        let heard = degrade(&u, 65_536, 7, &p);
        for word in &heard.words {
            assert!(word.pitch_steps.abs() <= p.radio_pitch_steps);
        }

        // Another listener hears another set of words (or all of them):
        // the dropout is per listener, and it is still deterministic.
        let other = degrade(&u, 65_536, 9, &p);
        assert_eq!(other.words, degrade(&u, 65_536, 9, &p).words);
    }

    #[test]
    fn the_clarity_is_zero_beyond_the_range() {
        let p = TalkParams::default();
        assert_eq!(clarity(Channel::Voice, 25_600, 0, &p), 0);
        assert_eq!(clarity(Channel::Voice, 25_601, 0, &p), 0);
        assert_eq!(clarity(Channel::Shout, 61_440, 0, &p), 0);
        // And full at zero distance.
        assert_eq!(clarity(Channel::Voice, 0, 0, &p), 65_536);
    }

    #[test]
    fn the_degraded_copy_keeps_the_score_truth() {
        let p = TalkParams::default();
        let u = line();
        let heard = degrade(&u, 65_536, 7, &p);
        assert_eq!(heard.speaker, u.speaker);
        assert_eq!(heard.channel, u.channel);
        assert_eq!(heard.affect, u.affect);
        assert_eq!(heard.subject, u.subject);
        assert_eq!(heard.act, Some(u.act));
        assert_eq!(heard.intent, Some(u.intent));
        assert_eq!(heard.spoken_words, 4);
        assert_eq!(heard.words.len(), 4);
        assert_eq!(heard.id, u.state_hash());
        // The words keep their order and their timings.
        assert_eq!(
            heard
                .words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>(),
            vec!["hold", "the", "north", "beacon"]
        );
    }

    #[test]
    fn the_distances_are_measured_in_the_worlds_units() {
        // 24 m apart on the x axis.
        let a = Pos::from_units(0, 0);
        let b = Pos::from_units(24 * i64::from(UNITS_PER_M), 0);
        assert_eq!(dist_units(a, b), AT_24_M);
    }

    #[test]
    fn the_audibility_query_is_the_gate_the_sim_uses() {
        let p = TalkParams::default();
        let grid = Grid::new(16, 1);
        let speaker = Pos::from_units(0, 0);
        let near = Pos::from_units(i64::from(AT_24_M), 0);
        let far = Pos::from_units(i64::from(AT_24_5_M), 0);
        let u = line();

        // At 24 m the voice is audible; half a metre further it is not.
        assert!(audible_to(&u, speaker, near, 0, 0, &grid, &p));
        assert!(!audible_to(&u, speaker, far, 0, 0, &grid, &p));

        // The radio is the squad's channel: the squad hears it, the other
        // squad does not.
        let mut radio = line();
        radio.channel = Channel::Radio;
        assert!(audible_to(&radio, speaker, far, 0, 0, &grid, &p));
        assert!(!audible_to(&radio, speaker, far, 1, 0, &grid, &p));

        // And a wall takes the voice away: the line that reached 24 m
        // does not reach through the wall.
        let mut walled = Grid::new(16, 1);
        walled.set(CellXY { x: 3, y: 0 }, CellKind::Wall);
        assert!(!audible_to(&u, speaker, near, 0, 0, &walled, &p));
    }
}
