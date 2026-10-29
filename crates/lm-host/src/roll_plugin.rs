//! The piano roll: the last few hundred ticks, one lane per agent.
//!
//! The roll is the debugger of the round (spec §6). Every agent gets a lane:
//! the decisions it was given are the marks on the lane, and what the world
//! did to it -- the stun it lay in, the beacon its squad held -- are the
//! spans under them. The human's own decisions are the bright green marks,
//! so a reviewer can see at a glance when a person took over.
//!
//! The roll is drawn from the round log and the events of the round, exactly
//! like the roll of a round that is being replayed; nothing here reaches
//! back into the simulation.
//!
//! ## The score, one key away
//!
//! `E` flips the panel into its other mode, the conductor's score
//! ([`crate::score_plugin`]): the same round, the same scrubber, drawn as
//! one system of staves per agent instead of one lane.
//!
//! ## The replay controls
//!
//! When the app is showing a round that is being replayed, the panel grows a
//! row of controls above the lanes: a slider that seeks the round, and a
//! button that plays it at the round's own rate. A file that could not be
//! opened is not a crash either: the [`LogError`] it came back with is
//! written across the panel.

use crate::map_plugin::{channel_color, egui_color, squad_color, Bottom, Layout};
use crate::score_plugin::draw_score;
use crate::sim_plugin::{CurrentRoll, CurrentScore, Replay, Round, ScoreZoom};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use lm_core::utterance::Channel;
use lm_host_model::roll::{MarkKind, SpanKind};

/// The roll plugin: the panel at the bottom of the window.
///
/// The panel has two modes (Vox Score §12, E7): the piano roll, one lane
/// per agent, and the conductor's score, one system of staves per agent.
/// `E` walks between them; the replay scrubber above stays, because the
/// two are readings of one round on one clock.
pub struct RollPlugin;

impl Plugin for RollPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, roll_panel);
    }
}

/// The panel itself.
#[allow(clippy::too_many_arguments)]
fn roll_panel(
    mut contexts: EguiContexts,
    roll: Res<CurrentRoll>,
    score: Res<CurrentScore>,
    zoom: Res<ScoreZoom>,
    round: Res<Round>,
    mut replay: ResMut<Replay>,
    mut bottom: ResMut<Bottom>,
    keys: Res<ButtonInput<KeyCode>>,
    layout: Res<Layout>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let mut root = egui::Ui::new(
        ctx.clone(),
        "roll_root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    // The one key that walks the panel between its two modes. The mode is
    // data (`BottomSwitch`): nothing of the round is touched by a flip.
    if keys.just_pressed(crate::keys::SCORE_FLIP.key) {
        bottom.0.toggle();
    }
    let mode = bottom.mode();
    egui::Panel::bottom("roll_panel")
        .resizable(false)
        .exact_size(layout.bottom)
        .show(&mut root, |ui| {
            draw_replay(ui, &round, &mut replay);
            match mode {
                lm_host_model::view::BottomMode::Roll => draw_roll(ui, &roll),
                lm_host_model::view::BottomMode::Score => {
                    draw_score(ui, &score, &round, &zoom, mode)
                }
            }
        });
    Ok(())
}

/// The replay controls: a slider that seeks, and a button that plays.
///
/// The slider covers the whole round, like the roll below it, so the two
/// are on the same scale: the slider says where in the round the viewer is,
/// and the lanes show what happened there.
fn draw_replay(ui: &mut egui::Ui, round: &Round, replay: &mut Replay) {
    let Some(scrubber) = round.replay.as_ref() else {
        // A file that could not be opened: the error is the whole panel.
        if let Some(err) = &round.error {
            ui.colored_label(
                egui::Color32::from_rgb(255, 120, 110),
                format!("the file cannot be replayed: {err}"),
            );
        }
        return;
    };

    let start = scrubber.log().header.start_tick;
    let last = scrubber.last_tick();

    ui.horizontal(|ui| {
        if let Some(source) = &round.source {
            ui.label(format!("replay — {}", source.display()));
        } else {
            ui.label(format!("replay — {} ticks", scrubber.len()));
        }

        // The slider: it asks for a tick, and the tick is what the round
        // shows. It is wide, because a slider you cannot read is no slider.
        let width = (ui.available_width() * 0.5).clamp(120.0, 320.0);
        ui.spacing_mut().slider_width = width;
        let mut tick = replay.tick as f64;
        let response =
            ui.add(egui::Slider::new(&mut tick, start as f64..=last as f64).show_value(true));
        if response.changed() {
            replay.tick = tick as u64;
        }

        // The button: the round plays itself at its own rate.
        if ui
            .button(if replay.playing { "pause" } else { "play" })
            .clicked()
        {
            replay.playing = !replay.playing;
        }
    });
}

/// The lanes: one per agent, over the window of ticks.
fn draw_roll(ui: &mut egui::Ui, roll: &CurrentRoll) {
    let lanes = &roll.model.lanes;

    ui.horizontal(|ui| {
        ui.label(format!(
            "piano roll — ticks {}..{}",
            roll.window.start, roll.window.end
        ));
        ui.label(format!("{} lanes", lanes.len()));
    });

    let height = (ui.available_height() - 2.0).max(10.0);
    let (track, _response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter();

    // The label column, and the track of ticks beside it.
    let label_w = 24.0_f32.min(track.width() * 0.2);
    let x0 = track.min.x + label_w;
    let width = (track.width() - label_w).max(1.0);
    let ticks = roll.window.end.saturating_sub(roll.window.start).max(1) as f32;
    let x_of = |tick: u64| x0 + (tick.saturating_sub(roll.window.start) as f32 / ticks) * width;

    // The window the lanes are drawn in.
    painter.rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(x0, track.min.y),
            egui::vec2(width, track.height()),
        ),
        0.0,
        egui::Color32::from_black_alpha(60),
    );

    let lane_h = track.height() / lanes.len().max(1) as f32;
    // The label of a lane shrinks with the lane, so that a short window
    // does not pile the numbers on top of each other.
    let font = egui::FontId::proportional(lane_h.clamp(7.0, 11.0));
    for (index, lane) in lanes.iter().enumerate() {
        let top = track.min.y + index as f32 * lane_h;
        let bottom = top + lane_h;

        // Whose lane this is.
        painter.text(
            egui::pos2(track.min.x + 2.0, (top + bottom) * 0.5),
            egui::Align2::LEFT_CENTER,
            format!("{}", lane.agent),
            font.clone(),
            egui::Color32::WHITE,
        );

        // What happened to the agent: the runs it went through.
        for span in &lane.spans {
            let from = x_of(span.from);
            let to = x_of(span.to.saturating_add(1)).max(from + 1.0);
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(from, top + 1.0), egui::pos2(to, bottom - 1.0)),
                0.0,
                span_color(span.what),
            );
        }

        // What the agent was told: the decisions that were applied.
        for mark in &lane.marks {
            let x = x_of(mark.tick);
            painter.rect_filled(
                egui::Rect::from_min_max(
                    egui::pos2(x - 1.0, top + 1.0),
                    egui::pos2(x + 1.0, bottom - 1.0),
                ),
                0.0,
                mark_color(mark.kind),
            );
        }

        // What the agent was thinking: the bar from the question to the
        // answer. A thought that is still in flight ends where it was
        // expected to land, and the bar grows as the answer takes its time.
        if let Some(thoughts) = roll.thoughts.get(&lane.agent) {
            for thought in thoughts {
                let from = x_of(thought.asked);
                let to = x_of(thought.landed.unwrap_or(thought.expected_land)).max(from + 1.0);
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(from, bottom - 3.0),
                        egui::pos2(to, bottom - 1.0),
                    ),
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(240, 190, 90, 150),
                );
            }
        }
    }
}

/// The colour of a mark: where the decision on it came from.
pub(crate) fn mark_color(kind: MarkKind) -> egui::Color32 {
    match kind {
        MarkKind::Human => egui::Color32::from_rgb(150, 240, 90),
        MarkKind::System1 => egui::Color32::from_rgb(90, 160, 240),
        MarkKind::System2 => egui::Color32::from_rgb(190, 120, 240),
        MarkKind::Test => egui::Color32::from_rgb(150, 150, 150),
        MarkKind::Marker => egui::Color32::from_rgb(90, 90, 90),
    }
}

/// The colour of a span: what the run on it was.
fn span_color(what: SpanKind) -> egui::Color32 {
    match what {
        SpanKind::Stunned => egui::Color32::from_rgb(170, 60, 60),
        SpanKind::HoldingBeacon(squad) => egui_color(squad_color(squad)),
        SpanKind::OnIntent => egui::Color32::from_rgb(70, 170, 160),
        SpanKind::OnSafeBranch => egui::Color32::from_rgb(120, 130, 70),
        // A line: the span is the length of the line, and the colour is the
        // channel it was spoken on — the same colour the map draws its words
        // in.
        SpanKind::Speaking(channel) => speaking_color(channel),
    }
}

/// The colour of a speaking span: the channel's colour, dimmed.
///
/// The span is the whole length of the line, so a bright colour would
/// swallow the lane and the marks on it: the channel is still told apart,
/// but the marks stay readable.
pub(crate) fn speaking_color(channel: Channel) -> egui::Color32 {
    let srgba = channel_color(channel).to_srgba();
    egui::Color32::from_rgba_unmultiplied(
        (srgba.red * 255.0).round() as u8,
        (srgba.green * 255.0).round() as u8,
        (srgba.blue * 255.0).round() as u8,
        110,
    )
}
