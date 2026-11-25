//! The conductor's score: the bottom panel's other mode, one system of
//! staves per agent (Vox Score §12, E7).
//!
//! Where the piano roll draws one lane per agent, the score draws each
//! agent as a small **system** — a stack of four staves read on one shared
//! time axis, like one instrument's staves in an orchestral score:
//!
//! ```text
//! a0 ▌ voice   the words, at their ticks, pitched as sung · the note bar
//!             of the airtime · the marks: [urgent, order, to-squad]
//!      intent  the gestures: every move and action decision, by whose
//!              hand; the hollow marks where a mind refused or was unsure
//!      gaze    the attention target over time: the move its lanes hold,
//!              and who its line was for while it spoke
//!      tier    the engine and the gamepad: the reflex where the lanes
//!              held nothing, System One where they held a move
//! ```
//!
//! Everything on the staves comes from [`CurrentScore`], which a played
//! round rebuilds once a tick and a replayed round rebuilds on every seek,
//! so the score is the same picture in both modes of the round. The panel
//! itself — the replay scrubber above, the axis below — is the roll's
//! panel; only the drawing inside changes, because the two modes are two
//! readings of one round on one clock.

use bevy::prelude::*;
use bevy_egui::egui;
use lm_core::utterance::{Addressee, Affect, Q16_ONE};
use lm_core::view::MenuOption;
use lm_host_model::score::{GestureKind, HollowKind, System};
use lm_host_model::view::BottomMode;

use crate::look::Look;
use crate::map_plugin::{egui_color, squad_color};
use crate::roll_plugin::{mark_color, speaking_color};
use crate::sim_plugin::{CurrentScore, Round, ScoreZoom};

/// The width of the label column: the agent's name and its squad's colour.
const LABEL_W: f32 = 46.0;

/// How the system's height splits into staves: the voice staff is the tall
/// one, because its words and marks have to be read.
const VOICE_SHARE: f32 = 0.52;
const INTENT_SHARE: f32 = 0.17;
const GAZE_SHARE: f32 = 0.16;

/// The colours of the beacons a gaze staff may attend to, in beacon order.
const BEACON_COLORS: [egui::Color32; 4] = [
    egui::Color32::from_rgb(235, 200, 70),
    egui::Color32::from_rgb(150, 210, 235),
    egui::Color32::from_rgb(215, 150, 220),
    egui::Color32::from_rgb(140, 220, 160),
];

/// The engine tier's colour (the reflex).
const ENGINE: egui::Color32 = egui::Color32::from_rgb(58, 64, 76);
/// The gamepad tier's colour (System One).
const GAMEPAD: egui::Color32 = egui::Color32::from_rgb(70, 170, 160);
/// A late decision: the mark that says System One answered after the
/// moment it answered for.
const LATE: egui::Color32 = egui::Color32::from_rgb(240, 170, 70);

/// The tone of a line, as the marks under its note bar are coloured.
fn affect_color(affect: Affect) -> egui::Color32 {
    match affect {
        Affect::Calm => egui::Color32::from_rgb(160, 180, 200),
        Affect::Urgent => egui::Color32::from_rgb(255, 120, 90),
        Affect::Uncertain => egui::Color32::from_rgb(240, 190, 90),
    }
}

/// The colour of a gaze target: the beacon it walks to, the hold, the
/// follow, the retreat.
fn target_color(target: MenuOption) -> egui::Color32 {
    match target {
        MenuOption::GoToBeacon(beacon) => BEACON_COLORS[usize::from(beacon) % BEACON_COLORS.len()],
        MenuOption::Hold => egui::Color32::from_rgb(120, 130, 145),
        MenuOption::FollowLeader => egui::Color32::from_rgb(110, 190, 130),
        MenuOption::Retreat => egui::Color32::from_rgb(170, 90, 90),
    }
}

/// The word a gaze target is named by, when there is room to name it.
fn target_word(target: MenuOption) -> String {
    match target {
        MenuOption::GoToBeacon(beacon) => format!("b{beacon}"),
        MenuOption::Hold => "hold".to_owned(),
        MenuOption::FollowLeader => "follow".to_owned(),
        MenuOption::Retreat => "retreat".to_owned(),
    }
}

/// The colour of an address: the speaker's squad for its own squad, white
/// for everybody, the spoken-to agent's squad for one agent.
fn addressee_color(
    look: &Look,
    score: &CurrentScore,
    system: &System,
    addressee: Addressee,
) -> egui::Color32 {
    match addressee {
        Addressee::Squad => egui_color(squad_color(look, system.squad)),
        Addressee::All => egui::Color32::WHITE,
        Addressee::Agent(agent) => score
            .model
            .system(agent)
            .map(|spoken_to| egui_color(squad_color(look, spoken_to.squad)))
            .unwrap_or_else(|| egui::Color32::WHITE),
    }
}

/// The score's header: what it draws, over which ticks, and the key that
/// walks back to the roll.
fn score_header(ui: &mut egui::Ui, score: &CurrentScore, zoom: &ScoreZoom, mode: BottomMode) {
    ui.horizontal(|ui| {
        ui.strong(format!(
            "score — ticks {}..{} — last {}s",
            score.window.start,
            score.window.end.saturating_sub(1),
            zoom.0
        ));
        ui.weak(format!(
            "voice · intent · gaze · tier — {} systems",
            score.model.systems.len()
        ));
        if mode == BottomMode::Score {
            ui.weak("E: the roll");
        }
    });
}

/// Draw the score: the systems stacked, every staff on the one axis.
pub fn draw_score(
    ui: &mut egui::Ui,
    score: &CurrentScore,
    round: &Round,
    zoom: &ScoreZoom,
    mode: BottomMode,
    look: &Look,
) {
    score_header(ui, score, zoom, mode);

    let height = (ui.available_height() - 2.0).max(10.0);
    let (track, _response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter();

    // The label column, and the track of ticks beside it — the roll's own
    // geometry, so the two modes read on the same scale.
    let label_w = LABEL_W.min(track.width() * 0.2);
    let x0 = track.min.x + label_w;
    let width = (track.width() - label_w).max(1.0);
    let ticks = score.window.end.saturating_sub(score.window.start).max(1) as f32;
    let x_of = |tick: u64| x0 + (tick.saturating_sub(score.window.start) as f32 / ticks) * width;

    painter.rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(x0, track.min.y),
            egui::vec2(width, track.height()),
        ),
        0.0,
        egui::Color32::from_black_alpha(60),
    );

    // The second marks: a score is read against the clock, so every whole
    // second of the round gets a faint line through every system.
    let tick_hz = round
        .header()
        .map(|header| u64::from(header.params.tick_hz))
        .unwrap_or(30);
    let mut second = score.window.start.div_ceil(tick_hz) * tick_hz;
    while second < score.window.end {
        let x = x_of(second);
        painter.line_segment(
            [egui::pos2(x, track.min.y), egui::pos2(x, track.max.y)],
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(14)),
        );
        second += tick_hz;
    }

    let systems = &score.model.systems;
    let system_h = track.height() / systems.len().max(1) as f32;
    for (index, system) in systems.iter().enumerate() {
        let top = track.min.y + index as f32 * system_h;
        draw_system(
            painter, score, system, top, system_h, &x_of, x0, width, label_w, look,
        );
    }

    // The current-tick cursor: one line through every system, on the tick
    // the round is at — the same tick the replay scrubber names.
    let now = round.tick();
    if score.window.contains(&now) || now == score.window.end {
        let x = x_of(now);
        painter.line_segment(
            [egui::pos2(x, track.min.y), egui::pos2(x, track.max.y)],
            egui::Stroke::new(1.0, look.egui(look.0.ui.accent)),
        );
    }
}

/// Draw one agent's system: the label column entry, then the four staves.
#[allow(clippy::too_many_arguments)]
fn draw_system(
    painter: &egui::Painter,
    score: &CurrentScore,
    system: &System,
    top: f32,
    height: f32,
    x_of: &dyn Fn(u64) -> f32,
    x0: f32,
    width: f32,
    label_w: f32,
    look: &Look,
) {
    let squad = egui_color(squad_color(look, system.squad));

    // Whose system this is: the squad's colour down the left edge, and the
    // agent's name beside it, like the instrument names of an orchestral
    // score.
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(x0 - label_w, top + 1.0),
            egui::pos2(x0 - label_w + 3.0, top + height - 1.0),
        ),
        0.0,
        squad,
    );
    let role = if system.role == lm_core::header::Role::Leader {
        "L"
    } else {
        "m"
    };
    painter.text(
        egui::pos2(x0 - label_w + 6.0, top + height * 0.5),
        egui::Align2::LEFT_CENTER,
        format!("a{} {role}", system.agent),
        egui::FontId::proportional((height * 0.34).clamp(8.0, 12.0)),
        squad,
    );

    // The staves, the voice staff the tallest, each inset a hair from its
    // neighbours so the four read as four and not as one striped bar.
    let gap = if height > 24.0 { 1.0 } else { 0.0 };
    let voice_h = height * VOICE_SHARE - gap;
    let intent_h = height * INTENT_SHARE - gap;
    let gaze_h = height * GAZE_SHARE - gap;
    let tier_h = (height - voice_h - intent_h - gaze_h - 3.0 * gap).max(2.0);
    let voice = egui::Rect::from_min_size(egui::pos2(x0, top), egui::vec2(width, voice_h));
    let intent = egui::Rect::from_min_size(
        egui::pos2(x0, top + voice_h + gap),
        egui::vec2(width, intent_h),
    );
    let gaze = egui::Rect::from_min_size(
        egui::pos2(x0, top + voice_h + gap + intent_h + gap),
        egui::vec2(width, gaze_h),
    );
    let tier = egui::Rect::from_min_size(
        egui::pos2(x0, top + voice_h + gap + intent_h + gap + gaze_h + gap),
        egui::vec2(width, tier_h),
    );

    draw_voice(painter, system, &voice, x_of);
    draw_intent(painter, system, &intent, &tier, x_of);
    draw_gaze(painter, score, system, &gaze, x_of, look);
    draw_tier(painter, system, &tier, x_of);
}

/// The voice staff: each line a note bar of its airtime, its words above
/// it at the ticks the score timed them to and pitched them at, and the
/// gates' own answer — the tone, the intention, the address — as marks
/// below it.
fn draw_voice(
    painter: &egui::Painter,
    system: &System,
    band: &egui::Rect,
    x_of: &dyn Fn(u64) -> f32,
) {
    // The staff's three zones: the words on top, the bar under them, the
    // marks under the bar.
    let marks_font = egui::FontId::proportional((band.height() * 0.38).clamp(5.5, 8.0));
    let marks_h = painter
        .layout_no_wrap(
            "urgent order".to_owned(),
            marks_font.clone(),
            egui::Color32::WHITE,
        )
        .size()
        .y
        + 1.0;
    let bar_h = 2.5;
    let words_h = (band.height() - marks_h - bar_h).max(4.0);
    let words_font = egui::FontId::proportional((words_h * 0.92).clamp(6.0, 11.0));

    for note in &system.voice {
        let from = x_of(note.from);
        let to = (x_of(note.to.saturating_add(1))).max(from + 2.0);

        // The note bar: the line's airtime, in the dimmed colour of the
        // channel it was spoken on (the same colour the roll and the map
        // speak in), with a hair of the gates' tone over its top edge so
        // the urgency is readable before the words are.
        let bar = egui::Rect::from_min_max(
            egui::pos2(from, band.min.y + words_h),
            egui::pos2(to, band.min.y + words_h + bar_h),
        );
        painter.rect_filled(bar, 0.0, speaking_color(note.channel));
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(from, bar.min.y), egui::pos2(to, bar.min.y + 1.0)),
            0.0,
            affect_color(note.affect),
        );

        // The words, at their ticks and at their pitch: a word sung high
        // sits high on the staff. A word whose ticks are narrower than the
        // word is long gives up its tail, then all but its first letter; a
        // word whose centre has left the window is not drawn at all, so a
        // line crossing the edge does not smear its tail over the edge.
        let half = (words_h * 0.5 - words_font.size * 0.5).max(0.0);
        for word in &note.words {
            let on = x_of(note.from.saturating_add(word.t_on));
            let off = x_of(note.from.saturating_add(word.t_off));
            let centre = (on + off) * 0.5;
            if centre < band.min.x || centre > band.max.x {
                continue;
            }
            let room = (off - on).max(words_font.size);
            let lift = -f32::from(word.pitch_steps) / 24.0 * half;
            let y = band.min.y + words_h * 0.5 + lift;
            let color = if word.emphasis_q16 >= Q16_ONE / 2 {
                egui::Color32::from_rgb(255, 220, 120)
            } else {
                egui::Color32::from_white_alpha(225)
            };
            let text = fit_word(painter, &word.text, room, &words_font);
            painter.text(
                egui::pos2(centre, y),
                egui::Align2::CENTER_CENTER,
                text,
                words_font.clone(),
                color,
            );
        }

        // The marks: what the line was for, its tone, who it was for and
        // what it was about — dropped from the end when the bar is too
        // narrow to hold them all, and not drawn at all when the bar began
        // too near the window's edge to hold even one.
        if from < band.max.x - 6.0 {
            let marks = fit_marks(painter, &note.marks(), to - from, &marks_font);
            painter.text(
                egui::pos2(from + 1.0, band.max.y - marks_h * 0.5),
                egui::Align2::LEFT_CENTER,
                marks,
                marks_font.clone(),
                affect_color(note.affect),
            );
        }
    }
}

/// A word narrowed to the room its ticks give it: the whole word, then its
/// first three letters, then its first letter.
fn fit_word(painter: &egui::Painter, word: &str, room: f32, font: &egui::FontId) -> String {
    let width = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    };
    if width(word) <= room {
        return word.to_owned();
    }
    let head: String = word.chars().take(3).collect();
    if width(&head) <= room {
        return head;
    }
    word.chars().take(1).collect()
}

/// The marks narrowed to the room the note bar gives them: the whole list,
/// then the tone and the intention, then the tone alone.
fn fit_marks(painter: &egui::Painter, marks: &[String], room: f32, font: &egui::FontId) -> String {
    let width = |text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
            .size()
            .x
    };
    for keep in [marks.len(), 2, 1] {
        if keep > marks.len() {
            continue;
        }
        let joined = marks[..keep].join(" ");
        if width(&joined) <= room {
            return joined;
        }
    }
    marks
        .first()
        .map(|mark| mark.chars().take(1).collect())
        .unwrap_or_default()
}

/// The intent staff: the mind's hand on the body. Every move and action
/// decision is a gesture, coloured by whose hand made it; every refusal
/// and unsure gate is a hollow mark — a tick with nothing in it, where the
/// mind asked and did not answer.
fn draw_intent(
    painter: &egui::Painter,
    system: &System,
    band: &egui::Rect,
    tier: &egui::Rect,
    x_of: &dyn Fn(u64) -> f32,
) {
    painter.rect_filled(*band, 0.0, egui::Color32::from_black_alpha(25));

    for gesture in &system.intent {
        let x = x_of(gesture.tick);
        let width = match gesture.what {
            GestureKind::Move => 2.5,
            GestureKind::Action | GestureKind::Release => 3.5,
        };
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x - width * 0.5, band.min.y + 0.5),
                egui::pos2(x + width * 0.5, band.max.y - 0.5),
            ),
            0.0,
            mark_color(gesture.who),
        );
        // A late answer is drawn again on the tier staff, in the colour
        // that says the moment had already passed.
        if gesture.late_by > 0 {
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x - 0.5, tier.min.y + 0.5),
                    egui::pos2(x + 0.5, tier.max.y - 0.5),
                ),
                0.0,
                LATE,
            );
        }
    }

    for hollow in &system.hollow {
        let x = x_of(hollow.tick);
        let stroke = if hollow.why == HollowKind::Unsure {
            egui::Color32::from_rgb(240, 190, 90)
        } else {
            egui::Color32::from_white_alpha(170)
        };
        painter.rect_stroke(
            egui::Rect::from_min_max(
                egui::pos2(x - 2.0, band.min.y + 0.5),
                egui::pos2(x + 2.0, band.max.y - 0.5),
            ),
            0.0,
            egui::Stroke::new(1.0, stroke),
            egui::StrokeKind::Inside,
        );
    }
}

/// The gaze staff: where the agent's attention stood — the move its lanes
/// hold, as a lane of target colours and ids — and, at the tick each line
/// began, who that line was for.
fn draw_gaze(
    painter: &egui::Painter,
    score: &CurrentScore,
    system: &System,
    band: &egui::Rect,
    x_of: &dyn Fn(u64) -> f32,
    look: &Look,
) {
    // The attention runs: only the moves are drawn; the reflex's stretches
    // stay the staff's own dark, which is what makes them legible.
    let font = egui::FontId::proportional((band.height() * 0.95).clamp(5.0, 8.0));
    for run in &system.runs {
        let lm_host_model::score::Attention::Move(target) = run.what else {
            continue;
        };
        let from = x_of(run.from);
        let to = x_of(run.to.saturating_add(1)).max(from + 2.0);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(from, band.min.y + 0.5),
                egui::pos2(to, band.max.y - 0.5),
            ),
            0.0,
            target_color(target),
        );
        let word = target_word(target);
        let label =
            painter.layout_no_wrap(word, font.clone(), egui::Color32::from_black_alpha(220));
        if to - from > label.size().x + 4.0 && band.height() >= 6.0 {
            painter.galley(
                egui::pos2(
                    from + 2.0,
                    band.min.y + (band.height() - label.size().y) * 0.5,
                ),
                label,
                egui::Color32::from_black_alpha(220),
            );
        }
    }

    // The address: a bright tick at the tick each line began, in the colour
    // of who the line was for — the gaze of the voice, at the moment it
    // spoke. While the line runs, a hair of the same colour rides the top
    // of the staff.
    for note in &system.voice {
        let color = addressee_color(look, score, system, note.addressee);
        let from = x_of(note.from);
        let to = x_of(note.to.saturating_add(1));
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(from - 1.0, band.min.y + 0.5),
                egui::pos2(from + 1.5, band.max.y - 0.5),
            ),
            0.0,
            color,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(from, band.min.y + 0.5),
                egui::pos2(to.max(from + 2.0), band.min.y + 1.5),
            ),
            0.0,
            color,
        );
    }
}

/// The tier staff: which tier was playing the body — the engine's reflex
/// (§13's tier 0) where the lanes held nothing, System One's gamepad where
/// they held a move.
fn draw_tier(
    painter: &egui::Painter,
    system: &System,
    band: &egui::Rect,
    x_of: &dyn Fn(u64) -> f32,
) {
    for run in &system.runs {
        let from = x_of(run.from);
        let to = x_of(run.to.saturating_add(1)).max(from + 2.0);
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(from, band.min.y + 0.5),
                egui::pos2(to, band.max.y - 0.5),
            ),
            0.0,
            if run.what.is_move() { GAMEPAD } else { ENGINE },
        );
    }
}
