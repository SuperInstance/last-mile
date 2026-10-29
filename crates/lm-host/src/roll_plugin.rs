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

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use lm_host_model::roll::{MarkKind, SpanKind};

use crate::map_plugin::{egui_color, squad_color, Layout};
use crate::sim_plugin::CurrentRoll;

/// The roll plugin: the panel at the bottom of the window.
pub struct RollPlugin;

impl Plugin for RollPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, roll_panel);
    }
}

/// The panel itself.
fn roll_panel(mut contexts: EguiContexts, roll: Res<CurrentRoll>, layout: Res<Layout>) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let mut root = egui::Ui::new(
        ctx.clone(),
        "roll_root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::Panel::bottom("roll_panel")
        .resizable(false)
        .exact_size(layout.bottom)
        .show(&mut root, |ui| {
            draw_roll(ui, &roll);
        });
    Ok(())
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
    }
}

/// The colour of a mark: where the decision on it came from.
fn mark_color(kind: MarkKind) -> egui::Color32 {
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
    }
}
