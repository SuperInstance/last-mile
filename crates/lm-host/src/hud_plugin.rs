//! The top bar: what the round is doing, right now.
//!
//! The bar is one line of numbers -- the tick, the time left, the score of
//! every squad, the counters of the sequencer and the counters of the
//! reflexes -- and it is the only thing that says how the round is going
//! without the reader having to watch it.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use lm_host_model::input::{InputState, Viewer};

use crate::input_plugin::{Input, Status};
use crate::map_plugin::{egui_color, squad_color, Layout};
use crate::sim_plugin::Round;

/// The HUD plugin: the bar at the top of the window.
pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, hud_bar);
    }
}

/// The bar itself.
fn hud_bar(
    mut contexts: EguiContexts,
    round: Res<Round>,
    input: Res<Input>,
    status: Res<Status>,
    layout: Res<Layout>,
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    // egui 0.36 panels show into a `&mut Ui`, not a `&Context`: build the
    // background ui the panel docks against.
    let mut root = egui::Ui::new(
        ctx.clone(),
        "hud_root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::Panel::top("hud_bar")
        .resizable(false)
        .exact_size(layout.top)
        .show(&mut root, |ui| {
            draw_hud(ui, &round, &input.0, &status);
        });
    Ok(())
}

/// The numbers of the round.
fn draw_hud(ui: &mut egui::Ui, round: &Round, input: &InputState, status: &Status) {
    // A file that could not be opened: the error is the whole bar, because
    // there is no round to count.
    if let Some(err) = &round.error {
        ui.colored_label(
            egui::Color32::from_rgb(255, 120, 110),
            format!("the file cannot be replayed: {err}"),
        );
        return;
    }

    let Some(header) = round.header() else {
        return;
    };
    let Some(sim) = round.sim() else {
        return;
    };
    let tick = round.tick();
    let params = &header.params;
    let left = u64::from(params.time_limit_ticks).saturating_sub(tick);
    let seconds = left as f32 / f32::from(params.tick_hz);
    let stats = sim.stats();

    ui.horizontal_wrapped(|ui| {
        ui.label(format!("tick {tick}"));
        ui.label(format!("time {seconds:.0}s"));

        // The score of every squad, in the colour of the squad.
        for (squad, score) in round.scores().iter().enumerate() {
            ui.colored_label(
                egui_color(squad_color(squad as u8)),
                format!("s{squad} {score}"),
            );
        }

        ui.separator();

        // The counters of the sequencer are the counters of a round being
        // played: a replay has no sequencer, it has a log.
        if let Some(host) = &round.live {
            let sched = host.sched_stats();
            ui.label(format!(
                "sched on-time {} late {} stale {}",
                sched.on_time, sched.late, sched.stale
            ));
        } else {
            ui.label("replay");
        }

        ui.separator();
        ui.label(format!(
            "reflex fuel {} err {} refused {}",
            stats.reflex_out_of_fuel, stats.reflex_error, stats.reflex_refused
        ));

        ui.separator();
        ui.label(viewer_label(input));

        // How the round ended, when it has.
        if let Some(outcome) = round.outcome() {
            match outcome.winner {
                Some(squad) => {
                    ui.colored_label(egui_color(squad_color(squad)), "round over");
                }
                None => {
                    ui.label("round over: a tie");
                }
            }
        }

        // What the host said about the last input.
        if let Some(message) = &status.0 {
            ui.colored_label(egui::Color32::from_rgb(255, 120, 110), message.clone());
        }
    });
}

/// Who is looking at the round.
fn viewer_label(input: &InputState) -> String {
    match input.viewer {
        Viewer::Possessing(agent) => format!("playing agent {agent}"),
        Viewer::Spectator {
            follow: Some(agent),
        } => format!("watching agent {agent}"),
        Viewer::Spectator { follow: None } => "watching".to_owned(),
    }
}
