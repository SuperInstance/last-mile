//! The look, as the host holds it.
//!
//! The look is **data**: `assets/look/look.toml` says what the game looks
//! like, and the host reads it once, at start-up. The 2D map, the 3D view
//! and the panels around them all take their colours from it, so the world
//! looks the same in every view — and a different look is a different file,
//! not a different build.
//!
//! This module is the host's half of that: the file is read here, the
//! tokens live in [`lm_render::look`], and the colours are handed to Bevy
//! and egui in the space each of them wants.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

use lm_render::look::{Look as Tokens, Rgb};

/// The default place the look file is looked for.
///
/// It is a path relative to the working directory, the way `render.toml`
/// and `voice.toml` are found: the host is run from the checkout.
pub const DEFAULT_LOOK: &str = "assets/look/look.toml";

/// The look of the game: the colours, sizes and shadows every view reads.
#[derive(Resource, Clone, Debug, Default)]
pub struct Look(pub Tokens);

impl Look {
    /// The look from a file, or the defaults when there is no such file.
    ///
    /// A file that exists but cannot be read or parsed is an error: a
    /// renderer running on defaults nobody asked for is a lie about what is
    /// on screen.
    pub fn load(path: &Path) -> Result<Self, String> {
        Ok(Look(Tokens::load(path)?))
    }

    /// The colour a squad wears.
    pub fn squad_color(&self, squad: u8) -> Rgb {
        self.0.squad_color(squad)
    }

    /// A colour of the look, as egui wants it.
    pub fn egui(&self, color: Rgb) -> bevy_egui::egui::Color32 {
        let [r, g, b, a] = color.to_rgba8();
        bevy_egui::egui::Color32::from_rgba_unmultiplied(r, g, b, a)
    }

    /// A colour of the look, as Bevy wants it: sRGB, the space the colour
    /// was spelled in.
    pub fn bevy(&self, color: Rgb) -> bevy::color::Color {
        bevy::color::Color::srgba(color.r, color.g, color.b, color.a)
    }

    /// The panels' theme: one type scale and one set of colours.
    ///
    /// egui has five text styles and the look names five sizes, and each
    /// style takes one of them — so a panel cannot invent a sixth size.
    /// The panels sit on the look's surface and say things in the look's
    /// text, which is what makes the panels and the world one picture.
    pub fn apply_theme(&self, ctx: &bevy_egui::egui::Context) {
        let scale = &self.0.type_scale;
        let ui = &self.0.ui;
        ctx.all_styles_mut(|style| {
            style.text_styles.insert(
                bevy_egui::egui::TextStyle::Heading,
                bevy_egui::egui::FontId::proportional(scale.big),
            );
            style.text_styles.insert(
                bevy_egui::egui::TextStyle::Body,
                bevy_egui::egui::FontId::proportional(scale.body),
            );
            style.text_styles.insert(
                bevy_egui::egui::TextStyle::Small,
                bevy_egui::egui::FontId::proportional(scale.small),
            );
            style.text_styles.insert(
                bevy_egui::egui::TextStyle::Monospace,
                bevy_egui::egui::FontId::proportional(scale.tiny),
            );
            style.text_styles.insert(
                bevy_egui::egui::TextStyle::Button,
                bevy_egui::egui::FontId::proportional(scale.body),
            );
            // The surface the panels sit on, and the text they say things
            // in. A panel is a piece of the same picture as the world.
            let surface = self.egui(ui.surface);
            let raised = self.egui(ui.surface_raised);
            style.visuals.panel_fill = surface;
            style.visuals.window_fill = raised;
            style.visuals.extreme_bg_color = raised;
            style.visuals.override_text_color = Some(self.egui(ui.text));
            style.visuals.weak_text_color = Some(self.egui(ui.text_dim));
            style.visuals.hyperlink_color = self.egui(ui.accent);
            style.visuals.selection.bg_fill = self.egui(ui.accent).gamma_multiply(0.4);
            style.visuals.selection.stroke =
                bevy_egui::egui::Stroke::new(1.0, self.egui(ui.accent));
        });
    }
}

/// The path the look file is read from, when the command line does not
/// name one.
pub fn default_path() -> PathBuf {
    PathBuf::from(DEFAULT_LOOK)
}
