//! Input: what the person did, and what the host does with it.
//!
//! Every key and every click is turned into an [`InputEvent`] and handed to
//! the [`InputState`], which turns it into a decision and submits it through
//! the same sequencer the agents use (spec §6, D10 lesson 10). The host
//! never moves an agent by itself: a click is an order, and the order goes
//! through the rules like any other.
//!
//! ## The keys
//!
//! * `1`..`8` play as an agent (agent 0 is `1`).
//! * `0` or `Esc` let go of the agent being played.
//! * `H` holds the ground of the agent being played.
//! * `F` follows the agent under the pointer.
//! * `Tab` toggles between watching and playing.
//!
//! ## The mouse
//!
//! A click on the map is a cell to walk to. A drag is the camera, and the
//! wheel is the zoom. The camera is only free when nobody is being played or
//! followed; otherwise it follows.

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::input::mouse::{MouseButton, MouseWheel};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use lm_host_model::input::{InputEvent, InputState, Viewer};

use crate::map_plugin::{cursor_cell, hovered_agent, CameraRig, CurrentMap, Layout, TileLayer};
use crate::sim_plugin::Round;

/// The systems of the input, as one set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum InputSet {
    /// What the person did is read and applied.
    Read,
}

/// The input state of the round.
///
/// The model cannot be a Bevy resource itself -- the model does not know
/// about Bevy -- so the host keeps it in a wrapper of its own.
#[derive(Resource, Default)]
pub struct Input(pub InputState);

/// What the host said about the last input.
#[derive(Resource, Default)]
pub struct Status(pub Option<String>);

/// The agent the human was playing last, so `Tab` can go back to it.
#[derive(Resource, Default)]
pub struct LastPossessed(pub Option<u16>);

/// Where the pointer is, and what it was doing.
#[derive(Resource, Default)]
pub struct Pointer {
    /// Where the button went down.
    pressed_at: Option<Vec2>,
    /// Whether the pointer has travelled far enough to be a drag.
    dragged: bool,
    /// Where the pointer was the last time it was seen.
    last: Option<Vec2>,
}

/// How far the pointer may travel between the press and the release and
/// still count as a click, in pixels.
const CLICK_SLOP: f32 = 5.0;
/// How fast the camera pans, in pixels a second.
const PAN_SPEED_PX: f32 = 600.0;
/// How much one notch of the wheel zooms.
const ZOOM_STEP: f32 = 1.15;
/// The closest the camera may come, in pixels per metre.
const MIN_PX_PER_M: f32 = 0.2;
/// The furthest the camera may go, in pixels per metre.
const MAX_PX_PER_M: f32 = 40.0;

/// The input plugin: the keyboard, the mouse, and the decisions they make.
pub struct InputPlugin;

impl Plugin for InputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Input>()
            .init_resource::<Status>()
            .init_resource::<LastPossessed>()
            .init_resource::<Pointer>()
            .add_systems(Update, read_input.in_set(InputSet::Read));
    }
}

/// Read what the person did, and let the host act on it.
#[allow(clippy::too_many_arguments)]
fn read_input(
    mut input: ResMut<Input>,
    mut round: ResMut<Round>,
    mut status: ResMut<Status>,
    mut last: ResMut<LastPossessed>,
    mut pointer: ResMut<Pointer>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    mut rig: ResMut<CameraRig>,
    layout: Res<Layout>,
    layer: Res<TileLayer>,
    map: Res<CurrentMap>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    views: Option<Res<crate::Views>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let cursor = window.cursor_position();

    // The keys that play an agent.
    for (key, agent) in [
        (KeyCode::Digit1, 0u16),
        (KeyCode::Digit2, 1),
        (KeyCode::Digit3, 2),
        (KeyCode::Digit4, 3),
        (KeyCode::Digit5, 4),
        (KeyCode::Digit6, 5),
        (KeyCode::Digit7, 6),
        (KeyCode::Digit8, 7),
    ] {
        if keys.just_pressed(key) {
            last.0 = Some(agent);
            apply(
                &mut input,
                &mut round,
                InputEvent::Possess(agent),
                &mut status,
            );
        }
    }

    // Letting go of the agent being played.
    if keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::Escape) {
        apply(&mut input, &mut round, InputEvent::Release, &mut status);
    }

    // Standing still.
    if keys.just_pressed(KeyCode::KeyH) {
        apply(&mut input, &mut round, InputEvent::Hold, &mut status);
    }

    // Watching the agent under the pointer.
    if keys.just_pressed(KeyCode::KeyF) {
        if let Some(cursor) = cursor {
            if let Some(agent) = hovered_agent(&rig, &layout, &map, cursor) {
                if round.live.is_some() {
                    apply(
                        &mut input,
                        &mut round,
                        InputEvent::FollowAgent(agent),
                        &mut status,
                    );
                } else {
                    // A replay is watched, not played: the camera may follow
                    // an agent, but nobody is playing one.
                    input.0.viewer = Viewer::Spectator {
                        follow: Some(agent),
                    };
                    status.0 = None;
                }
            }
        }
    }

    // Watching and playing, one or the other.
    if keys.just_pressed(KeyCode::Tab) {
        match input.0.possessing() {
            Some(agent) => {
                last.0 = Some(agent);
                apply(&mut input, &mut round, InputEvent::Spectate, &mut status);
            }
            None => {
                if let Some(agent) = last.0 {
                    apply(
                        &mut input,
                        &mut round,
                        InputEvent::Possess(agent),
                        &mut status,
                    );
                }
            }
        }
    }

    // The pointer and the 2D camera belong to the 2D map. When the 3D view
    // is the one on screen (`--view both`, flipped with `V`), its own
    // camera controller answers these same verbs in depth — this plugin
    // must not read them against a map that is not drawn.
    let map_shown = views.map(|views| !views.0.showing_3d()).unwrap_or(true);
    if !map_shown {
        return;
    }

    // The mouse: a click on the map is a cell to walk to, a drag is the
    // camera.
    if mouse.just_pressed(MouseButton::Left) {
        pointer.pressed_at = cursor;
        pointer.dragged = false;
    }
    if mouse.pressed(MouseButton::Left) {
        if let Some(start) = pointer.pressed_at {
            if let Some(now) = cursor {
                if (now - start).length() > CLICK_SLOP {
                    pointer.dragged = true;
                }
            }
        }
    }
    if mouse.just_released(MouseButton::Left) {
        let clicked = !pointer.dragged;
        pointer.pressed_at = None;
        pointer.dragged = false;
        if clicked {
            if let Some(cursor) = cursor {
                if !over_panel(&layout, cursor) {
                    if let Some(cell) = cursor_cell(&rig, &layout, &layer, cursor) {
                        apply(
                            &mut input,
                            &mut round,
                            InputEvent::ClickCell(cell),
                            &mut status,
                        );
                    }
                }
            }
        }
    }

    // The camera is free only when nobody is being played or watched.
    let free = input.0.possessing().is_none() && input.0.following().is_none();
    let scale = 1.0 / rig.px_per_m;

    // Dragging pans the camera.
    if pointer.dragged && free {
        if let (Some(now), Some(previous)) = (cursor, pointer.last) {
            let delta = now - previous;
            rig.focus += Vec2::new(-delta.x * scale, delta.y * scale);
        }
    }
    pointer.last = cursor;

    // The keys pan the camera.
    let mut pan = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        pan.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        pan.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        pan.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        pan.x += 1.0;
    }
    if free && pan != Vec2::ZERO {
        let speed = PAN_SPEED_PX / rig.px_per_m;
        rig.focus += pan.normalize() * speed * time.delta_secs();
    }

    // The wheel zooms.
    let scroll: f32 = wheel.read().map(|wheel| wheel.y).sum();
    if scroll != 0.0 {
        let zoom = rig.px_per_m * ZOOM_STEP.powf(scroll);
        rig.zoom = Some(zoom.clamp(MIN_PX_PER_M, MAX_PX_PER_M));
    }
}

/// Apply what the person did, and say what the host said about it.
fn apply(input: &mut Input, round: &mut Round, event: InputEvent, status: &mut Status) {
    // A round being replayed is watched, not played: there is no sequencer
    // to take a decision, so the host says so instead of pretending.
    let Some(host) = round.live.as_mut() else {
        status.0 = Some(String::from(
            "a replay is watched, not played: scrub it instead",
        ));
        return;
    };
    match input.0.apply(event, host) {
        Ok(()) => status.0 = None,
        Err(err) => status.0 = Some(err.to_string()),
    }
}

/// Whether the cursor is over one of the panels rather than over the map.
fn over_panel(layout: &Layout, cursor: Vec2) -> bool {
    cursor.y < layout.top || cursor.y > layout.size.y - layout.bottom
}
