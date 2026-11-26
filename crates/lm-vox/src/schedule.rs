//! The words' timing: where each word (and roughly each phone class)
//! falls, so the renderer can say the right words in the performance's
//! rhythm (design §6).
//!
//! This is a sketch, not a phonetic lexicon. The letters of each word map
//! to vowel formant targets and consonant classes — a /a/-/i/-/u/ vowel
//! table from Peterson & Barney's measurements, and the consonant classes
//! (stop, fricative, nasal, liquid) a formant synthesizer needs to be
//! intelligible. The **performance** — pitch, level, timing — still comes
//! from L0; this module only says which shape the vocal tract makes and
//! when, which is the part L0 cannot carry.

use crate::frame::{Frame, FrameFlags};

/// One segment of the vocal-tract schedule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PhoneKind {
    /// A voiced segment: formant targets in Hz and a level scale 0-255.
    Voiced { f1: u16, f2: u16, f3: u16, amp: u8 },
    /// A noise segment: level 0-255 and whether it is high-frequency
    /// weighted (an /s/ rather than an /f/).
    Noise { level: u8, bright: bool },
    /// A stop's closure: silence.
    Silent,
}

/// One scheduled segment with its time span, in ms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PhoneSeg {
    /// The kind of sound to make.
    pub kind: PhoneKind,
    /// When it starts, in ms from the utterance's start.
    pub start_ms: u32,
    /// When it ends, in ms from the utterance's start.
    pub end_ms: u32,
}

/// A word and the time it was said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordTiming {
    /// The word itself, lowercase letters.
    pub word: String,
    /// When it starts, in ms from the utterance's start.
    pub start_ms: u32,
    /// When it ends, in ms from the utterance's start.
    pub end_ms: u32,
}

/// Give each word of a line its time span, over the performance's sounding
/// span, in proportion to each word's syllable count.
///
/// The line's words are known (the score knows what it said); the timing is
/// the performance's: the first word starts when the sound starts, the last
/// ends when it ends, and the share in between follows the syllables.
pub fn word_timings_from_gestures(text: &str, frames: &[Frame]) -> Vec<WordTiming> {
    let words: Vec<&str> = text
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let live: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.flags.has(FrameFlags::REST))
        .map(|(i, _)| i)
        .collect();
    if live.is_empty() {
        return Vec::new();
    }
    let start_ms = live[0] as u32 * 10;
    let end_ms = (live[live.len() - 1] as u32 + 1) * 10;
    let span = (end_ms - start_ms).max(1);

    let syllables: Vec<u32> = words.iter().map(|w| syllables_of(w)).collect();
    let total: u32 = syllables.iter().sum::<u32>().max(1);

    let mut out = Vec::with_capacity(words.len());
    let mut cursor = start_ms;
    for (word, &syl) in words.iter().zip(&syllables) {
        let share = span as u64 * syl as u64 / total as u64;
        let end = (cursor + share as u32).min(end_ms);
        out.push(WordTiming {
            word: word.to_ascii_lowercase(),
            start_ms: cursor,
            end_ms: end,
        });
        cursor = end;
    }
    if let Some(last) = out.last_mut() {
        last.end_ms = end_ms;
    }
    out
}

/// A rough syllable count: runs of vowels, with the usual silent-e and
/// vowel-pair discounts. One syllable per vowel group is close enough for
/// sharing out time.
fn syllables_of(word: &str) -> u32 {
    let w: Vec<char> = word.chars().collect();
    let mut groups = 0u32;
    let mut in_group = false;
    for &c in &w {
        let vowel = matches!(c, 'a' | 'e' | 'i' | 'o' | 'u');
        if vowel && !in_group {
            groups += 1;
        }
        in_group = vowel;
    }
    // A trailing silent e ("time") is usually not its own syllable.
    if w.len() > 2 && w[w.len() - 1] == 'e' && !is_vowel(w[w.len() - 2]) {
        groups = groups.saturating_sub(1);
    }
    groups.max(1)
}

fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u')
}

/// The phone sketch of a whole scheduled line.
pub fn phone_sketch(words: &[WordTiming]) -> Vec<PhoneSeg> {
    let mut out = Vec::new();
    for w in words {
        out.extend(word_segs(&w.word, w.start_ms, w.end_ms));
    }
    out
}

/// One word's segments, stretched to its time span.
fn word_segs(word: &str, start_ms: u32, end_ms: u32) -> Vec<PhoneSeg> {
    let raw = word_phones(word);
    if raw.is_empty() {
        return vec![PhoneSeg {
            kind: PhoneKind::Voiced {
                f1: 640,
                f2: 1200,
                f3: 2400,
                amp: 160,
            },
            start_ms,
            end_ms,
        }];
    }
    let total_weight: u32 = raw.iter().map(|p| p.1).sum();
    let span = end_ms.saturating_sub(start_ms).max(total_weight.min(40));
    let mut cursor = start_ms;
    let mut out = Vec::with_capacity(raw.len());
    for (kind, weight) in raw {
        let len = span as u64 * weight as u64 / total_weight as u64;
        let end = (cursor + len as u32).min(end_ms);
        out.push(PhoneSeg {
            kind,
            start_ms: cursor,
            end_ms: end,
        });
        cursor = end;
    }
    if let Some(last) = out.last_mut() {
        last.end_ms = end_ms;
    }
    out
}

/// A raw phone with the share of the word's time it wants, in ms at a
/// nominal speaking rate.
type RawPhone = (PhoneKind, u32);

/// The letters of a word as a phone sketch.
///
/// Digraphs are matched first, then single letters; anything unknown is
/// skipped. The vowel formants are Peterson & Barney's measured means for
/// the nearest vowel.
fn word_phones(word: &str) -> Vec<RawPhone> {
    let chars: Vec<char> = word.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let digraph: Option<(&str, usize)> = if let Some(n) = next {
            match (c, n) {
                ('s', 'h') => Some(("sh", 2)),
                ('c', 'h') => Some(("ch", 2)),
                ('t', 'h') => Some(("th", 2)),
                ('p', 'h') => Some(("ph", 2)),
                ('n', 'g') => Some(("ng", 2)),
                ('w', 'h') => Some(("wh", 2)),
                ('c', 'k') => Some(("ck", 2)),
                ('q', 'u') => Some(("qu", 2)),
                ('e', 'e') => Some(("ee", 2)),
                ('o', 'o') => Some(("oo", 2)),
                ('a', 'i') => Some(("ai", 2)),
                ('a', 'y') => Some(("ay", 2)),
                ('o', 'a') => Some(("oa", 2)),
                ('o', 'u') => Some(("ou", 2)),
                ('e', 'a') => Some(("ea", 2)),
                ('o', 'w') => Some(("ow", 2)),
                _ => None,
            }
        } else {
            None
        };

        if let Some((name, skip)) = digraph {
            out.push(phone_of_digraph(name, i == 0, chars.len()));
            i += skip;
            continue;
        }

        let last = i + 1 == chars.len();
        out.push(phone_of_letter(c, last, next));
        i += 1;
    }
    out
}

/// Digraph phones.
fn phone_of_digraph(name: &str, first: bool, _len: usize) -> RawPhone {
    match name {
        "sh" => noise(80, true),
        "ch" => stop_burst(60, true),
        "th" => noise(40, false),
        "ph" => noise(35, false),
        "ng" => voiced(250, 2000, 2600, 90),
        "wh" => voiced(300, 700, 2200, 140),
        "ck" => stop_burst(50, false),
        "qu" => (
            PhoneKind::Voiced {
                f1: 300,
                f2: 700,
                f3: 2200,
                amp: 140,
            },
            40,
        ),
        "ee" => long_vowel(270, 2290, 3010),
        "oo" => long_vowel(300, 870, 2240),
        "ai" | "ay" | "ea" => long_vowel(530, 1840, 2480),
        "oa" | "ow" => long_vowel(570, 840, 2410),
        "ou" => long_vowel(640, 1200, 2400),
        _ => {
            let _ = first;
            voiced(640, 1200, 2400, 180)
        }
    }
}

/// Single-letter phones.
fn phone_of_letter(c: char, last: bool, next: Option<char>) -> RawPhone {
    match c {
        'a' => vowel(730, 1090, 2440),
        'e' => {
            if last {
                // A final e is usually silent: hold the vowel only briefly.
                (
                    PhoneKind::Voiced {
                        f1: 530,
                        f2: 1840,
                        f3: 2480,
                        amp: 120,
                    },
                    30,
                )
            } else {
                vowel(530, 1840, 2480)
            }
        }
        'i' => vowel(400, 1900, 2550),
        'o' => vowel(570, 840, 2410),
        'u' => vowel(640, 1200, 2400),
        'y' => {
            if next.is_none_or(is_consonantish) {
                vowel(400, 1900, 2550)
            } else {
                voiced(300, 2200, 3000, 150)
            }
        }
        'b' => voiced_stop(15),
        'd' => voiced_stop(15),
        'g' => voiced_stop(15),
        'p' => stop_burst(30, false),
        't' => stop_burst(15, true),
        'k' => stop_burst(50, false),
        'c' => {
            if next.is_some_and(|n| matches!(n, 'e' | 'i' | 'y')) {
                noise(90, true)
            } else {
                stop_burst(50, false)
            }
        }
        's' => noise(90, true),
        'z' => noise(80, true),
        'f' => noise(35, false),
        'v' => noise(35, false),
        'h' => noise(40, false),
        'x' => stop_burst(50, false),
        'm' => voiced(250, 1100, 2300, 110),
        'n' => voiced(250, 1600, 2600, 110),
        'l' => voiced(350, 1100, 2600, 160),
        'r' => voiced(350, 1300, 1600, 160),
        'w' => voiced(300, 700, 2200, 150),
        'j' => noise(70, true),
        _ => vowel(640, 1200, 2400),
    }
}

fn is_consonantish(c: char) -> bool {
    !is_vowel(c)
}

/// A vowel's segment and weight.
fn vowel(f1: u16, f2: u16, f3: u16) -> RawPhone {
    (
        PhoneKind::Voiced {
            f1,
            f2,
            f3,
            amp: 200,
        },
        120,
    )
}

/// A long vowel (digraph or stressed): longer and no quieter.
fn long_vowel(f1: u16, f2: u16, f3: u16) -> RawPhone {
    (
        PhoneKind::Voiced {
            f1,
            f2,
            f3,
            amp: 200,
        },
        180,
    )
}

/// A nasal, liquid or glide: voiced, quieter, shorter.
fn voiced(f1: u16, f2: u16, f3: u16, amp: u8) -> RawPhone {
    (PhoneKind::Voiced { f1, f2, f3, amp }, 60)
}

/// A fricative: noise for as long as it hisses.
fn noise(level: u8, bright: bool) -> RawPhone {
    (PhoneKind::Noise { level, bright }, 80)
}

/// A voiceless stop: a closure, then a burst.
fn stop_burst(burst_level: u8, bright: bool) -> RawPhone {
    (
        PhoneKind::Noise {
            level: burst_level,
            bright,
        },
        25,
    )
}

/// A voiced stop: a short voice bar then a burst.
fn voiced_stop(_amp: u8) -> RawPhone {
    (
        PhoneKind::Voiced {
            f1: 250,
            f2: 900,
            f3: 2300,
            amp: 90,
        },
        35,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Words share the sounding span by syllables, without gaps or
    /// overlaps.
    #[test]
    fn timings_share_the_span() {
        let frames: Vec<Frame> = (0..100)
            .map(|_| Frame {
                f0_cents: 1500,
                rms: 140,
                flags: FrameFlags::VOICED,
                ..Frame::default()
            })
            .collect();
        let t = word_timings_from_gestures("go behind now", &frames);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].start_ms, 0);
        assert_eq!(t[2].end_ms, 1000);
        for pair in t.windows(2) {
            assert_eq!(pair[0].end_ms, pair[1].start_ms);
        }
        // "behind" has two syllables to "go"'s and "now"'s one: it
        // gets more time.
        assert!(t[1].end_ms - t[1].start_ms > t[0].end_ms - t[0].start_ms);
    }

    /// The sketch of a word covers its whole span.
    #[test]
    fn sketch_covers_the_word() {
        let words = vec![WordTiming {
            word: "cat".to_string(),
            start_ms: 100,
            end_ms: 400,
        }];
        let segs = phone_sketch(&words);
        assert_eq!(segs.first().unwrap().start_ms, 100);
        assert_eq!(segs.last().unwrap().end_ms, 400);
        // /k/ closure-burst, /a/ vowel, /t/ burst: three segments.
        assert_eq!(segs.len(), 3);
        assert_eq!(
            segs[1].kind,
            PhoneKind::Voiced {
                f1: 730,
                f2: 1090,
                f3: 2440,
                amp: 200
            }
        );
    }

    /// Digraphs are one phone, not two.
    #[test]
    fn digraphs_are_one_phone() {
        let segs = word_segs("she", 0, 300);
        // /sh/ then /e/: two segments, the first noise.
        assert!(matches!(
            segs[0].kind,
            PhoneKind::Noise { bright: true, .. }
        ));
        assert_eq!(segs.len(), 2);
    }
}
