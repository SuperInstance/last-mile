use serde::{Deserialize, Serialize};

/// A color representation with RGBA components as f32 values in the range [0.0, 1.0].
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
}

/// Render configuration that can be loaded from a TOML file.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum UpscaleMode {
    #[default]
    Integer,
    Dlss,
    Off,
}

/// Palette mode for the renderer
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum PaletteMode {
    #[default]
    Default,
}

impl PaletteMode {
    /// The version of the palette, as the render cache's structure hash sees it.
    ///
    /// A palette is part of every mesh, so a changed palette is a changed
    /// mesh: the version joins the chunk's structure hash, and it has to be
    /// bumped whenever the colours of a variant change.
    pub const fn version(&self) -> u32 {
        match self {
            PaletteMode::Default => 1,
        }
    }
}

/// DLSS quality settings
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum DlssQuality {
    #[default]
    Quality,
    Performance,
    UltraPerformance,
}

/// The palette for rendering, containing color mappings for materials and squads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Palette {
    /// Squad colors indexed by squad ID (0-3)
    pub squad_colors: [Color; 4],
    /// Material class colors
    pub material_colors: MaterialColors,
    /// Leader rim color for highlighting leaders
    pub leader_rim: Color,
}

impl Palette {
    /// Get the color for a specific squad
    pub fn squad_color(&self, squad: u8) -> Color {
        self.squad_colors[(squad % 4) as usize]
    }

    /// Get the color for a specific material class
    pub fn class_color(&self, class: MaterialClass) -> Color {
        match class {
            MaterialClass::Floor => self.material_colors.floor,
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MaterialClass {
    Floor,
    Wall,
    Cover,
    BeaconHeld,
    BeaconFree,
}

/// Material colors structure
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MaterialColors {
    pub floor: Color,
    pub wall: Color,
    pub cover: Color,
    pub beacon_held: Color,
    pub beacon_free: Color,
}
