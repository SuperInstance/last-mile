//! The top bar: what the round is doing, right now.
//!
//! The bar is one line of numbers -- the tick, the time left, the score of
//! every squad, the counters of the sequencer and the counters of the
//! reflexes -- and it is the only thing that says how the round is going
//! without the reader having to watch it.
//!
//! Under it is a second row, the talk's own: the lines the round has
//! spoken, the airtime they took, the orders and how many of them were
//! taken up, the microphone, the prosody service and the half-duplex gate.
//! It is a row of its own because those numbers are read together, and
//! because a bar that has to hold everything squeezes everything.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use lm_host_model::input::{InputState, Viewer};

use crate::audio_plugin::{Playback, VoiceState};
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
    voice: Res<VoiceState>,
    playback: Res<Playback>,
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
    // The round's own numbers are in the bar; the talk's numbers are in the
    // row under it. Two panels, so neither one has to be squeezed.
    egui::Panel::top("hud_bar")
        .resizable(false)
        .exact_size(layout.bar)
        .show(&mut root, |ui| {
            draw_hud(ui, &round, &input.0, &status);
        });
    egui::Panel::top("hud_talk")
        .resizable(false)
        .exact_size(layout.talk)
        .show(&mut root, |ui| {
            draw_talk(ui, &round, &voice, &playback);
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

            // The agents' counters: what they asked, what they refused, what
            // is waiting, and how much model each of them used.
            if let Some(stats) = host.agent_stats() {
                ui.separator();
                ui.label(format!(
                    "agents think {} refused {} escalated {} queue {}",
                    stats.thinks, stats.refusals, stats.escalations, stats.queue_len
                ));
                let usage: Vec<String> = stats
                    .usage
                    .iter()
                    .map(|(agent, calls)| format!("a{agent}:{calls}"))
                    .collect();
                if !usage.is_empty() {
                    ui.label(format!("calls {}", usage.join(" ")));
                }
            }
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

/// The talk's numbers: the row under the bar.
///
/// The talk is read together: the lines the round has spoken, the airtime
/// they took, the orders and how many of them were taken up, the
/// microphone, the prosody service and the half-duplex gate.
fn draw_talk(ui: &mut egui::Ui, round: &Round, voice: &VoiceState, playback: &Playback) {
    // A round being replayed is watched, not played: it has no talk of its
    // own to count.
    let Some(host) = round.live.as_ref() else {
        ui.label("talk — the round is a replay");
        return;
    };
    let talk = host.talk_counters();

    ui.horizontal_wrapped(|ui| {
        // The lines the round has spoken, and the airtime they took.
        ui.label(format!(
            "talk {} air {}",
            talk.utterances, talk.airtime_ticks
        ));
        // The orders and the uptake: the uptake counter is the coordination
        // proof -- it climbs when one agent's order is heard and obeyed by
        // another.
        ui.label(format!("orders {}/{}", talk.orders_taken_up, talk.orders));
        // The airtime of every agent that spoke.
        let airtime: Vec<String> = talk
            .airtime_by_agent
            .iter()
            .map(|(agent, ticks)| format!("a{agent}:{ticks}"))
            .collect();
        if !airtime.is_empty() {
            ui.label(format!("airtime {}", airtime.join(" ")));
        }
        // The voice: the clips that were asked for, and what the bark cache
        // answered from itself.
        if talk.clips > 0 {
            ui.label(format!(
                "clips {} bark {}/{}",
                talk.clips, talk.bark_hits, talk.bark_misses
            ));
        }
        // The microphone: dead or not. That is the health check, and a dead
        // microphone submits nothing. Before anything has been captured the
        // microphone is simply idle — it has not been heard from yet, which
        // is not the same as being broken.
        ui.label(format!(
            "mic {}",
            if voice.mic.samples == 0 {
                "idle"
            } else if voice.mic.dead {
                "dead"
            } else {
                "live"
            }
        ));
        // The prosody service: "absent" means there is no service, "not
        // measured" means the service did not answer, and "measured" means
        // the line was scored with it.
        ui.label(format!(
            "prosody {}",
            if !voice.path.prosody_configured() {
                "absent"
            } else if voice.path.prosody_measured() {
                "measured"
            } else {
                "not measured"
            }
        ));
        // The gate: how much was played, and how much the human took by
        // talking. A dropped clip is dropped for good.
        ui.label(format!(
            "gate played {} cut {}",
            playback.played, playback.dropped
        ));
        // What the voice backend said last, when it said something.
        if let Some(said) = &talk.voice {
            ui.colored_label(egui::Color32::from_rgb(255, 190, 110), said.clone());
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
