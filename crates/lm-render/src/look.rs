//! The look: every colour, size and shadow a person can see, in one file.
//!
//! The world is drawn twice — the 2D map and the 3D view — and the panels
//! around them speak in the same colours again. Before this module each of
//! them spelled its own numbers, and a squad could be orange in the map and
//! a different orange in the 3D view. Now there is one place that says what
//! the game looks like, and it is **data**: `assets/look/look.toml`.
//!
//! The file may name any subset of the knobs; whatever it leaves out keeps
//! its default, the way `render.toml` reads. A file that exists but cannot
//! be read or parsed is an error, not a silent fallback: a renderer running
//! on defaults nobody asked for is a lie about what is on screen.
//!
//! The colours are spelled in the sRGB space a person means (`#e8dec6`),
//! because that is the space a colour picker, a screenshot and a colour
//!-blindness test all speak. The 3D view consumes linear light, so the
//! conversion happens once, here, at the boundary.

use serde::{Deserialize, Serialize};

use crate::palette::{Color, MaterialClass, Palette};

/// A colour in the sRGB space a person means.
///
/// The components are the ones a hex code spells, with an alpha that
/// defaults to opaque. The 3D view wants linear light, so a colour is
/// converted at the boundary rather than stored twice.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb {
    /// The red component, 0 to 1.
    pub r: f32,
    /// The green component, 0 to 1.
    pub g: f32,
    /// The blue component, 0 to 1.
    pub b: f32,
    /// The alpha: how much of what is behind shows through.
    pub a: f32,
}

impl Rgb {
    /// A colour from its sRGB components, opaque.
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// A colour from its sRGB components, with an alpha.
    pub const fn with_alpha(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// A colour from a hex string: `#rrggbb`, or `#rrggbbaa`.
    ///
    /// The three-digit shorthand is not spelled: a look file is read by
    /// people, and one spelling is one thing to get wrong.
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().strip_prefix('#')?;
        let (r, g, b, a) = match hex.len() {
            6 => (
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
                255,
            ),
            8 => (
                u8::from_str_radix(&hex[0..2], 16).ok()?,
                u8::from_str_radix(&hex[2..4], 16).ok()?,
                u8::from_str_radix(&hex[4..6], 16).ok()?,
                u8::from_str_radix(&hex[6..8], 16).ok()?,
            ),
            _ => return None,
        };
        Some(Self {
            r: f32::from(r) / 255.0,
            g: f32::from(g) / 255.0,
            b: f32::from(b) / 255.0,
            a: f32::from(a) / 255.0,
        })
    }

    /// The colour as a hex string, the way a look file spells it.
    pub fn to_hex(self) -> String {
        let channel = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            channel(self.r),
            channel(self.g),
            channel(self.b),
            channel(self.a)
        )
    }

    /// The colour in the linear light the 3D view consumes.
    pub fn to_linear(self) -> Color {
        let mut color = Color::from_srgb(self.r, self.g, self.b);
        color.a = self.a;
        color
    }

    /// The colour's components as bytes, the way a screen wants them.
    pub fn to_rgba8(self) -> [u8; 4] {
        let channel = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        [
            channel(self.r),
            channel(self.g),
            channel(self.b),
            channel(self.a),
        ]
    }

    /// The same colour with another alpha.
    pub fn alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }
}

/// A colour from a hex string, for the defaults.
///
/// A default that is not a colour is a bug in this file, and it should say
/// so where it is written rather than where it is read.
fn hex(text: &str) -> Rgb {
    Rgb::parse(text).unwrap_or_else(|| panic!("{text} is not a colour"))
}

impl TryFrom<String> for Rgb {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Rgb::parse(&text).ok_or_else(|| format!("{text:?} is not a colour: it wants #rrggbb"))
    }
}

impl From<Rgb> for String {
    fn from(color: Rgb) -> Self {
        color.to_hex()
    }
}

/// One of the sandbox's four paints.
///
/// The paints are the loud colour on a calm world: the world is sand and
/// terracotta and the figures are paint. Which paint a squad wears is a
/// parameter, so a round can be ochre against teal or saffron against
/// violet without a rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Paint {
    /// A burnt orange.
    Ochre,
    /// A blue-green.
    Teal,
    /// A blue-purple.
    Violet,
    /// A bright yellow.
    Saffron,
}

impl Paint {
    /// The paints, in the order they are listed in a look file.
    pub const ALL: [Paint; 4] = [Paint::Ochre, Paint::Teal, Paint::Violet, Paint::Saffron];

    /// The paint's name, as a look file spells it.
    pub const fn name(self) -> &'static str {
        match self {
            Paint::Ochre => "ochre",
            Paint::Teal => "teal",
            Paint::Violet => "violet",
            Paint::Saffron => "saffron",
        }
    }
}

/// The colours of the world the city stands on.
///
/// The world is calm on purpose: warm paper, soft terracotta, a pale street
/// network. Nothing here is loud, so that the paint — the figures and the
/// splats they leave — is the only thing that shouts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldLook {
    /// The open floor of the city: the plaza the blocks stand on.
    pub ground: Rgb,
    /// The streets between the blocks.
    pub road: Rgb,
    /// The walls of a building.
    pub building: Rgb,
    /// The roof of a building.
    pub building_top: Rgb,
    /// A crate or a hedge: the cover an agent hides behind.
    pub cover: Rgb,
    /// The lid of a crate.
    pub cover_top: Rgb,
    /// The sky the far ground fades into.
    pub sky: Rgb,
    /// The ground the board stands on: the table the diorama sits on.
    pub void: Rgb,
    /// A beacon nobody holds.
    pub beacon_free: Rgb,
    /// A beacon a squad holds.
    pub beacon_held: Rgb,
    /// The fog over a cell that has been seen and forgotten.
    pub fog_explored: Rgb,
    /// The fog over a cell nobody has seen.
    pub fog_unknown: Rgb,
}

impl Default for WorldLook {
    fn default() -> Self {
        Self {
            ground: hex("#e8dec6"),
            road: hex("#cec1a1"),
            building: hex("#cf8f6b"),
            building_top: hex("#e2a987"),
            cover: hex("#83906a"),
            cover_top: hex("#97a37c"),
            sky: hex("#a9c9e0"),
            void: hex("#b3ac9d"),
            beacon_free: hex("#f0e2a8"),
            beacon_held: hex("#f5c518"),
            fog_explored: hex("#2c2a24").alpha(0.45),
            fog_unknown: hex("#191813").alpha(0.92),
        }
    }
}

/// The four paints of the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Paints {
    /// A burnt orange.
    pub ochre: Rgb,
    /// A blue-green.
    pub teal: Rgb,
    /// A blue-purple.
    pub violet: Rgb,
    /// A bright yellow.
    pub saffron: Rgb,
}

impl Default for Paints {
    fn default() -> Self {
        Self {
            ochre: Rgb::parse("#d97f14").expect("the default is a colour"),
            teal: Rgb::parse("#0fa3a8").expect("the default is a colour"),
            violet: Rgb::parse("#7a4ec8").expect("the default is a colour"),
            saffron: Rgb::parse("#f5c518").expect("the default is a colour"),
        }
    }
}

impl Paints {
    /// The colour of a paint.
    pub fn color(&self, paint: Paint) -> Rgb {
        match paint {
            Paint::Ochre => self.ochre,
            Paint::Teal => self.teal,
            Paint::Violet => self.violet,
            Paint::Saffron => self.saffron,
        }
    }
}

/// Which paint each squad wears.
///
/// A round has two teams, and the teams take two of the four paints: the
/// pairing is the array, in squad order, so the choice is a parameter and
/// not a constant in the code.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Teams {
    /// The paint of every squad, in squad order.
    pub paint: [Paint; 4],
}

impl Default for Teams {
    fn default() -> Self {
        Self {
            paint: [Paint::Ochre, Paint::Teal, Paint::Violet, Paint::Saffron],
        }
    }
}

impl Teams {
    /// The paint of a squad. A squad past the fourth wears the first paint
    /// again, the way the palette has always wrapped.
    pub fn paint(&self, squad: u8) -> Paint {
        self.paint[usize::from(squad) % self.paint.len()]
    }
}

/// The colours of the panels around the world.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiLook {
    /// The surface a panel sits on.
    pub surface: Rgb,
    /// A panel that sits on another panel.
    pub surface_raised: Rgb,
    /// The text on a surface.
    pub text: Rgb,
    /// The text that is not saying anything.
    pub text_dim: Rgb,
    /// The one colour the panels use to point at something.
    pub accent: Rgb,
    /// The line between two things.
    pub divider: Rgb,
    /// Something is wrong.
    pub danger: Rgb,
    /// The mark of the agent who leads a squad.
    pub leader: Rgb,
    /// An agent that is stunned.
    pub stunned: Rgb,
}

impl Default for UiLook {
    fn default() -> Self {
        Self {
            surface: Rgb::parse("#1e2028").expect("the default is a colour"),
            surface_raised: Rgb::parse("#2a2d38").expect("the default is a colour"),
            text: Rgb::parse("#f4efe3").expect("the default is a colour"),
            text_dim: Rgb::parse("#9a958b").expect("the default is a colour"),
            accent: Rgb::parse("#f5c518").expect("the default is a colour"),
            divider: Rgb::parse("#3a3e4a").expect("the default is a colour"),
            danger: Rgb::parse("#e0655a").expect("the default is a colour"),
            leader: Rgb::parse("#f7f2e4").expect("the default is a colour"),
            stunned: Rgb::parse("#8b8f98").expect("the default is a colour"),
        }
    }
}

/// The light of the diorama.
///
/// The world is flat-coloured and unlit, so its shadow is a *shade*: the
/// sides of a box are the same colour as its top, dimmed. The figures are
/// lit, so the light is theirs: a warm sun and a soft ambient, and the
/// contact shadow each figure drops on the ground.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightLook {
    /// How much light falls on everything, from everywhere.
    ///
    /// The number is in candelas per square metre — the unit the renderer
    /// wants — so it is large: the fill is a whisper next to the sun, not
    /// a second sun.
    pub ambient_brightness: f32,
    /// The colour of the sun.
    pub sun_color: Rgb,
    /// How strong the sun is, in lux.
    pub sun_illuminance: f32,
    /// The sun's direction, as Euler angles in radians.
    pub sun_rotation: [f32; 3],
    /// How much of its own colour a side face carries, beside the top's
    /// full colour. This is the shadow of a box in a world with no other.
    pub side_shade: f32,
    /// How dark the shadow under a figure is.
    pub shadow_alpha: f32,
}

impl Default for LightLook {
    fn default() -> Self {
        Self {
            ambient_brightness: 40.0,
            sun_color: hex("#fff4dc"),
            sun_illuminance: 10_000.0,
            // The sun comes from over the left shoulder: the yaw is a quarter
            // turn, so the shadow falls to the right of a figure and the
            // player can tell which way the light is.
            sun_rotation: [-1.099_557, std::f32::consts::FRAC_PI_4, 0.0],
            side_shade: 0.74,
            shadow_alpha: 0.26,
        }
    }
}

/// The figures: the pieces on the board.
///
/// A figure is a game piece: small, chunky, and readable against anything
/// the world puts behind it. The leader is told apart by its **silhouette**,
/// not by its colour — a hat above the head — so that it is still the
/// leader when the colour is in shadow or the figure is two pixels wide.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FigureLook {
    /// How tall a figure stands, in metres.
    pub height_m: f32,
    /// How wide the leader's hat is, in metres.
    ///
    /// The hat is a brim: a disc that sits on the head and reads as a
    /// silhouette from above and from the side. Its width is the leader's
    /// mark, so it is wide enough to be seen at a glance and not so wide
    /// that it hides the figure under it.
    pub leader_hat_m: f32,
    /// Whether the leader wears a hat at all.
    pub leader_hat: bool,
}

impl Default for FigureLook {
    fn default() -> Self {
        Self {
            height_m: 1.15,
            leader_hat_m: 0.38,
            leader_hat: true,
        }
    }
}

/// The dark edge that separates a thing from what is behind it.
///
/// The 2D map draws it as a ring of pixels around a body; the 3D view draws
/// it as a shell that stands off the figure, so the figure reads as a
/// piece on a board rather than a shape floating in one.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OutlineLook {
    /// The ring around a body in the 2D map, in pixels.
    pub px: f32,
    /// How far the shell stands off a figure in the 3D view, in metres.
    pub width_m: f32,
    /// The colour of the edge.
    pub color: Rgb,
}

impl Default for OutlineLook {
    fn default() -> Self {
        Self {
            px: 2.0,
            width_m: 0.055,
            color: Rgb::parse("#241d16").expect("the default is a colour"),
        }
    }
}

/// One type scale for the whole game.
///
/// Five sizes, and no sixth: a panel that needs a sixth size is a panel
/// that has stopped saying what it means. The names are what the size is
/// *for*, not how big it is — the number is the file's business.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TypeScale {
    /// The number of a round: the score, the winner, the headline.
    pub huge: f32,
    /// A title: a section, a panel's own name.
    pub big: f32,
    /// The body: what a label says.
    pub body: f32,
    /// A quiet note under a label.
    pub small: f32,
    /// A counter, a tag, a tick on a scale.
    pub tiny: f32,
}

impl Default for TypeScale {
    fn default() -> Self {
        Self {
            huge: 34.0,
            big: 22.0,
            body: 17.0,
            small: 14.0,
            tiny: 12.0,
        }
    }
}

/// The camera of the diorama.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraLook {
    /// How far the 3D camera tilts above the ground, in radians. A diorama
    /// is looked at from above, not from the street.
    pub pitch_rad: f32,
}

impl Default for CameraLook {
    fn default() -> Self {
        Self { pitch_rad: 0.45 }
    }
}

/// The whole look: the tokens every view of the game reads.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    /// The colours of the world.
    pub world: WorldLook,
    /// The four paints.
    pub paints: Paints,
    /// Which paint each squad wears.
    pub teams: Teams,
    /// The colours of the panels.
    pub ui: UiLook,
    /// The light of the diorama.
    pub light: LightLook,
    /// The figures.
    pub figure: FigureLook,
    /// The dark edge around a thing.
    pub outline: OutlineLook,
    /// The type scale.
    pub type_scale: TypeScale,
    /// The camera.
    pub camera: CameraLook,
}

impl Look {
    /// The look from a `look.toml`, or the defaults when there is no such
    /// file.
    ///
    /// A file that exists but cannot be read or parsed is an error: a
    /// renderer running on defaults nobody asked for is a lie about what is
    /// on screen.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(format!("{} cannot be read: {err}", path.display())),
            Ok(text) => toml::from_str(&text)
                .map_err(|err| format!("{} is not a look.toml: {err}", path.display())),
        }
    }

    /// The colour of a paint.
    pub fn paint_color(&self, paint: Paint) -> Rgb {
        self.paints.color(paint)
    }

    /// The colour a squad wears.
    pub fn squad_color(&self, squad: u8) -> Rgb {
        self.paints.color(self.teams.paint(squad))
    }

    /// The 3D palette of this look.
    ///
    /// The 3D view and the 2D map are the same world drawn twice, so they
    /// take their colours from the same file: this is where the look
    /// becomes the palette the meshes are built with.
    pub fn palette(&self) -> Palette {
        Palette {
            squad_colors: [
                self.squad_color(0).to_linear(),
                self.squad_color(1).to_linear(),
                self.squad_color(2).to_linear(),
                self.squad_color(3).to_linear(),
            ],
            material_colors: crate::palette::MaterialColors {
                floor: self.world.ground.to_linear(),
                road: self.world.road.to_linear(),
                wall: self.world.building.to_linear(),
                cover: self.world.cover.to_linear(),
                beacon_held: self.world.beacon_held.to_linear(),
                beacon_free: self.world.beacon_free.to_linear(),
            },
            leader_rim: self.ui.leader.to_linear(),
            side_shade: self.light.side_shade,
        }
    }

    /// The colour of a material class, in linear light.
    pub fn class_color(&self, class: MaterialClass) -> Color {
        match class {
            MaterialClass::Floor => self.world.ground.to_linear(),
            MaterialClass::Road => self.world.road.to_linear(),
            MaterialClass::Wall => self.world.building.to_linear(),
            MaterialClass::Cover => self.world.cover.to_linear(),
            MaterialClass::BeaconHeld => self.world.beacon_held.to_linear(),
            MaterialClass::BeaconFree => self.world.beacon_free.to_linear(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_is_read_as_a_person_writes_it() {
        let color = Rgb::parse("#e8dec6").expect("a hex colour parses");
        assert!((color.r - 232.0 / 255.0).abs() < 1e-6);
        assert!((color.g - 222.0 / 255.0).abs() < 1e-6);
        assert!((color.b - 198.0 / 255.0).abs() < 1e-6);
        assert!((color.a - 1.0).abs() < 1e-6);

        // The alpha is optional, and spelled as a fourth pair.
        let faded = Rgb::parse("#00000080").expect("a hex colour with alpha parses");
        assert!((faded.a - 128.0 / 255.0).abs() < 1e-6);

        // And it is not a colour at all.
        assert!(Rgb::parse("e8dec6").is_none(), "the # is the prefix");
        assert!(Rgb::parse("#e8dec").is_none(), "six digits, not five");
        assert!(Rgb::parse("#zzzzzz").is_none(), "hex, not letters");
    }

    #[test]
    fn a_colour_round_trips_through_its_hex() {
        for color in [
            Rgb::new(1.0, 0.0, 0.0),
            Rgb::with_alpha(0.1, 0.2, 0.3, 0.4),
            Rgb::new(0.5, 0.5, 0.5),
        ] {
            let text = color.to_hex();
            let back = Rgb::parse(&text).expect("the hex parses");
            assert!(
                (back.r - color.r).abs() < 0.01
                    && (back.g - color.g).abs() < 0.01
                    && (back.b - color.b).abs() < 0.01
                    && (back.a - color.a).abs() < 0.01,
                "{text} round trips"
            );
        }
    }

    #[test]
    fn a_partial_look_toml_keeps_the_defaults() {
        // A look file that names one thing: everything else keeps its
        // default. (The visual acceptance of the look runs exactly this
        // file, and a missing field used to refuse it.)
        let dir = std::env::temp_dir().join("lm-render-look-toml");
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let path = dir.join("partial.toml");
        std::fs::write(&path, "[world]\nground = \"#101010\"\n").expect("the file is written");

        let look = Look::load(&path).expect("a partial file loads");
        assert_eq!(
            look.world.ground,
            Rgb::new(16.0 / 255.0, 16.0 / 255.0, 16.0 / 255.0)
        );
        assert_eq!(
            look.world.road,
            WorldLook::default().road,
            "the unnamed keep defaults"
        );
        assert_eq!(look.type_scale.body, TypeScale::default().body);
    }

    #[test]
    fn a_missing_look_file_is_the_defaults() {
        let dir = std::env::temp_dir().join("lm-render-look-toml");
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let path = dir.join("there-is-no-such-look.toml");
        let _ = std::fs::remove_file(&path);
        let look = Look::load(&path).expect("a missing file is the defaults");
        assert_eq!(look, Look::default());
    }

    #[test]
    fn a_broken_look_file_is_an_error() {
        let dir = std::env::temp_dir().join("lm-render-look-toml");
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let path = dir.join("broken.toml");
        std::fs::write(&path, "[world]\nground = \"not a colour\"\n").expect("the file is written");
        let err = Look::load(&path).expect_err("a broken file is an error");
        assert!(
            err.contains("look.toml"),
            "the error says what is wrong: {err}"
        );
    }

    #[test]
    fn the_team_pairing_is_a_parameter() {
        // Two teams take two of the four paints, and which two is the file's
        // business, not the code's.
        let look = Look::default();
        assert_eq!(look.teams.paint(0), Paint::Ochre);
        assert_eq!(look.teams.paint(1), Paint::Teal);

        let mut other = look;
        other.teams.paint = [Paint::Saffron, Paint::Violet, Paint::Ochre, Paint::Teal];
        assert_eq!(other.teams.paint(0), Paint::Saffron);
        assert_eq!(other.teams.paint(1), Paint::Violet);
        // The paints themselves do not move: only the pairing does.
        assert_eq!(other.paints.ochre, look.paints.ochre);
    }

    #[test]
    fn the_palette_and_the_map_agree_on_a_squad() {
        // The 3D view and the 2D map are the same world drawn twice: a squad
        // is one colour in both, and the look is where they agree.
        let look = Look::default();
        let palette = look.palette();
        for squad in 0..4u8 {
            // The palette is linear light and the look is sRGB, so the two
            // are compared in the space the palette lives in.
            let from_look = look.squad_color(squad).to_linear();
            let from_palette = palette.squad_color(squad);
            assert!(
                (from_look.r - from_palette.r).abs() < 1e-6
                    && (from_look.g - from_palette.g).abs() < 1e-6
                    && (from_look.b - from_palette.b).abs() < 1e-6,
                "squad {squad} is one colour in both views"
            );
        }
    }
}
