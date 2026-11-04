//! The 3D view: the world of `lm-render` drawn by Bevy (phase 6 Task 6).
//!
//! The view is a *view*, not a second source of truth: it renders the same
//! [`crate::sim_plugin::Round`] the 2D map renders, and every decision about
//! *what* to draw — which chunks, which figures, which colour, whether the
//! round may start — is made by `lm-render`'s scene model, in plain data,
//! off the GPU. This module is the mapping: scene data in, entities out.
//! Any disagreement between the 2D map and the 3D view is a bug here.
//!
//! Two rules hold the whole thing together:
//!
//! * **Nothing is compiled mid-round.** The materials are fixed — one white
//!   unlit material for every chunk quad (the vertex colour carries every
//!   surface's identity, spec §6's clean proxy-friendly output), one lit
//!   material per squad — and they are warmed on a card the camera looks at
//!   *before* the round starts. See [`warmup`].
//! * **The entities are stable.** A chunk is one `Mesh3d` entity, added when
//!   its chunk enters the view and despawned when it leaves — never re-added
//!   every frame, or the pipeline cache churns. The agents are one entity
//!   each, moved by their `Transform` and instanced by the engine's own
//!   batching (there is no `Instance` component to write).
//!
//! The camera is a spectator's overview; `main` picks this plugin instead of
//! the 2D map's when the command line says `--view 3d` (the `both` mode and
//! the fog are Task 8's, not this one's).

pub mod agents;
pub mod chunks;
pub mod warmup;

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::*;

use lm_render::palette::{Palette, RenderConfig};

/// The stage of the 3D view: the warmup card, then the round.
///
/// A plain resource rather than a `States` machine — the crate needs exactly
/// one transition, and `bevy_state` is not among bevy's default features.
/// While the card is up, virtual time is paused: the round held behind it
/// starts from its first tick, after the pipelines exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Resource, Default)]
pub enum ViewStage {
    /// The warmup card is on screen and the pipelines are compiling.
    #[default]
    Warming,
    /// The round: the chunks and the figures.
    Round,
}

/// Whether the round is on — the run condition of the round's systems.
fn in_round(stage: Res<ViewStage>) -> bool {
    *stage == ViewStage::Round
}

/// The settings of the 3D view: everything tunable, loaded once (D19).
#[derive(Clone, Debug, Resource)]
pub struct View3dSettings {
    /// The render configuration, from `render.toml` or its defaults.
    pub render: RenderConfig,
    /// The palette the configuration names.
    pub palette: Palette,
}

impl View3dSettings {
    /// The settings from a `render.toml`, or the plan's defaults when there
    /// is no such file.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let render = RenderConfig::load(path)?;
        Ok(View3dSettings {
            palette: Palette::for_mode(&render.palette),
            render,
        })
    }
}

/// The materials of the 3D view: fixed, and few, on purpose.
///
/// Chunk quads are `unlit` and white — every surface's identity rides in the
/// mesh's vertex colours, so a future neural renderer can read the same
/// buffer and no texture is ever sampled. The figures are lit, in the
/// squads' own colours, so they read as solid bodies. Nothing here is
/// created after startup, so nothing is compiled mid-round.
#[derive(Resource)]
pub struct View3dMaterials {
    /// The material every chunk quad wears: white, unlit.
    pub chunk: Handle<StandardMaterial>,
    /// The figure material of each squad.
    pub squads: [Handle<StandardMaterial>; 4],
}

/// The 3D view plugin: the warmup, the chunks, the figures.
pub struct View3dPlugin {
    /// The settings, loaded by `main` from `render.toml` (or its defaults).
    pub settings: View3dSettings,
}

impl Plugin for View3dPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.settings.clone())
            .init_resource::<ViewStage>()
            .init_resource::<warmup::WarmupFrames>()
            .insert_resource(warmup::WarmupModel(lm_render::scene::Warmup::new()))
            .init_resource::<chunks::ChunkStore>()
            .init_resource::<agents::FigureEntities>()
            // The retro-modern look starts dark; the fog (Task 8) ramps to it.
            .insert_resource(ClearColor(Color::linear_rgb(0.02, 0.03, 0.05)))
            .add_systems(Startup, warmup::setup)
            .add_systems(
                Update,
                (
                    // The warmup advances only while it holds; the round's
                    // systems run only once it has let go. The chain is the
                    // handover: the card clears in the same frame the stage
                    // flips, and the first round frame draws a warm world.
                    warmup::advance,
                    chunks::sync.run_if(in_round),
                    agents::update_figures.run_if(in_round),
                )
                    .chain(),
            );
    }
}
