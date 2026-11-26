use serde::{Deserialize, Serialize};

/// A color representation with RGBA components as f32 values in the range [0.0, 1.0].
///
/// The components are **linear-light** values, because that is what the
/// renderer consumes: a mesh's vertex colours and a material's base colour
/// are multiplied in linear space, so the palette stores linear space and
/// nobody converts twice. Spell a colour in the sRGB space a person means
/// with [`Color::from_srgb`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    /// A colour from sRGB components (the numbers a person means), opaque.
    ///
    /// The transfer function is the sRGB one, piecewise, the same conversion
    /// `bevy_color` makes of `Color::srgb` — so a palette spelled in sRGB
    /// shows on screen as the sRGB colour it spells.
    pub fn from_srgb(r: f32, g: f32, b: f32) -> Self {
        let linear = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        Color {
            r: linear(r.clamp(0.0, 1.0)),
            g: linear(g.clamp(0.0, 1.0)),
            b: linear(b.clamp(0.0, 1.0)),
            a: 1.0,
        }
    }
}

/// Render configuration that can be loaded from a TOML file.
///
/// Every tunable of the 3D view lives here (D19); the defaults are the
/// plan's (phase 6 Task 1), so a plain `RenderConfig::default()` is the
/// retro-modern look the spec describes — 960×540 inside, an integer
/// upscale out, and three chunks of view distance.
/// A `render.toml` may name any subset of the knobs: whatever it leaves
/// out keeps its default, the way the plan's table reads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderConfig {
    /// Internal rendering resolution width
    pub internal_width: u32,
    /// Internal rendering resolution height
    pub internal_height: u32,
    /// Upscaling mode
    pub upscale: UpscaleMode,
    /// Palette selection
    pub palette: PaletteMode,
    /// View distance in chunks
    pub view_distance_chunks: u32,
    /// Near fog distance in meters
    pub fog_near_m: f32,
    /// Far fog distance in meters
    pub fog_far_m: f32,
    /// Wall height in meters
    pub wall_height_m: f32,
    /// Cover height in meters
    pub cover_height_m: f32,
    /// Whether to use greedy meshing
    pub greedy: bool,
    /// Maximum number of chunks to render
    pub max_chunks: u32,
    /// DLSS quality setting
    pub dlss_quality: DlssQuality,
    /// DLSS project ID
    pub dlss_project_id: String,
}

/// Upscaling mode for the renderer
///
/// The TOML spellings are the plan's own lowercase ones (`upscale =
/// "integer" | "dlss" | "off"`), so a `render.toml` reads the way the
/// documentation does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UpscaleMode {
    #[default]
    Integer,
    Dlss,
    Off,
}

/// Palette mode for the renderer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PaletteMode {
    #[default]
    Default,
}

impl PaletteMode {
    /// The version of the palette, as the render cache's structure hash sees it.
    ///
    /// A palette is part of every mesh, so a changed palette is a changed
    /// mesh: the version joins the chunk's structure hash, and it has to be
    /// bumped whenever the colours of a variant change. (Version 2 added
    /// the flat face-direction shade the meshes carry, Task 8's reviewer
    /// finding 2. Version 3 is the look file: the colours are no longer a
    /// constant in this crate but a file the host reads, so the hash covers
    /// the palette's own values as well as this version.)
    pub const fn version(&self) -> u32 {
        match self {
            PaletteMode::Default => 3,
        }
    }
}

impl Default for RenderConfig {
    fn default() -> Self {
        RenderConfig {
            internal_width: 960,
            internal_height: 540,
            upscale: UpscaleMode::Integer,
            palette: PaletteMode::Default,
            view_distance_chunks: 3,
            fog_near_m: 40.0,
            fog_far_m: 120.0,
            wall_height_m: 2.6,
            cover_height_m: 1.1,
            greedy: true,
            max_chunks: 64,
            dlss_quality: DlssQuality::Quality,
            dlss_project_id: String::new(),
        }
    }
}

impl RenderConfig {
    /// The settings from a `render.toml`, or the defaults when there is no
    /// such file.
    ///
    /// A file that exists but cannot be read or parsed is an error, not a
    /// silent fallback: a renderer running on defaults nobody asked for is a
    /// lie about what is on screen.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(format!("{} cannot be read: {err}", path.display())),
            Ok(text) => toml::from_str(&text)
                .map_err(|err| format!("{} is not a render.toml: {err}", path.display())),
        }
    }
}

/// DLSS quality settings
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DlssQuality {
    #[default]
    Quality,
    Performance,
    UltraPerformance,
}

/// The palette for rendering, containing color mappings for materials and squads.
///
/// This is the **single shared palette** (D2): a squad is the same colour on
/// the 2D map, the piano roll and the 3D view alike. The colours come from
/// the look file (`assets/look/look.toml`), so the two views of the same
/// world agree without either asking the other, and a different look is a
/// different file and not a different build.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Palette {
    /// Squad colors indexed by squad ID (0-3)
    pub squad_colors: [Color; 4],
    /// Material class colors
    pub material_colors: MaterialColors,
    /// Leader rim color for highlighting leaders
    pub leader_rim: Color,
    /// How much of its own colour a side face carries, beside the tops'
    /// full colour.
    ///
    /// The world is flat-coloured and unlit, so its shadow is a *shade*:
    /// the sides of a box are its top's colour, dimmed. The factor is
    /// constant per direction, so a merged quad carries the same shade as
    /// the cells it was merged from.
    pub side_shade: f32,
}

impl Default for Palette {
    fn default() -> Self {
        // The default palette is the default look: one place says what the
        // game looks like, and the palette is what the meshes are built with.
        crate::look::Look::default().palette()
    }
}

impl Palette {
    /// The palette a configuration asks for by name.
    pub fn for_mode(mode: &PaletteMode) -> Palette {
        match mode {
            PaletteMode::Default => Palette::default(),
        }
    }

    /// Get the color for a specific squad
    pub fn squad_color(&self, squad: u8) -> Color {
        self.squad_colors[(squad % 4) as usize]
    }

    /// Get the color for a specific material class
    pub fn class_color(&self, class: MaterialClass) -> Color {
        match class {
            MaterialClass::Floor => self.material_colors.floor,
            MaterialClass::Road => self.material_colors.road,
            MaterialClass::Wall => self.material_colors.wall,
            MaterialClass::Cover => self.material_colors.cover,
            MaterialClass::BeaconHeld => self.material_colors.beacon_held,
            MaterialClass::BeaconFree => self.material_colors.beacon_free,
        }
    }

    /// Get the color for an agent (squad + leader status)
    pub fn agent_color(&self, squad: u8, leader: bool) -> Color {
        if leader {
            self.leader_rim
        } else {
            self.squad_color(squad)
        }
    }
}

/// Material class types for rendering
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MaterialClass {
    Floor,
    Road,
    Wall,
    Cover,
    BeaconHeld,
    BeaconFree,
}

/// Material colors structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MaterialColors {
    pub floor: Color,
    pub road: Color,
    pub wall: Color,
    pub cover: Color,
    pub beacon_held: Color,
    pub beacon_free: Color,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `render.toml` that names one knob: everything else keeps its
    /// default. (Found the hard way: the visual acceptance of Task 7 runs
    /// exactly this file, and a missing field used to refuse it.)
    #[test]
    fn a_partial_render_toml_keeps_the_defaults() {
        let dir = std::env::temp_dir().join("lm-render-render-toml");
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let path = dir.join("partial.toml");
        std::fs::write(&path, "internal_width = 640\ninternal_height = 360\n")
            .expect("the file is written");

        let cfg = RenderConfig::load(&path).expect("a partial file loads");
        assert_eq!((cfg.internal_width, cfg.internal_height), (640, 360));
        assert_eq!(
            cfg.upscale,
            UpscaleMode::Integer,
            "the unnamed keep defaults"
        );
        assert_eq!(cfg.view_distance_chunks, 3);
        assert_eq!(cfg.fog_far_m, 120.0);

        // The plan's own lowercase spellings parse.
        std::fs::write(&path, "upscale = \"dlss\"\ndlss_project_id = \"x\"\n")
            .expect("the file is written");
        let cfg = RenderConfig::load(&path).expect("the dlss spelling loads");
        assert_eq!(cfg.upscale, UpscaleMode::Dlss);
    }
}
