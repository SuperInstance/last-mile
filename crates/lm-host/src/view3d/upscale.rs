//! The internal resolution and the integer upscale (phase 6 Task 7).
//!
//! The retro-modern look (D2) is made here: the 3D world renders into an
//! **offscreen image** at the internal resolution (default 960×540), and a
//! **blit camera** draws that image to the window at the integer scale
//! [`lm_render::upscale`] computed — nearest-neighbour, letterboxed, never
//! stretched. The world camera and the blit are ordered (`-1` and `0`), so
//! the window never shows a half-updated frame.
//!
//! The blit is a sprite, not a fullscreen shader: Bevy 0.20's
//! `FullscreenMaterial` path is a *per-camera post-process* — its plugin
//! binds the camera's own accumulated texture, and cannot sample another
//! camera's render target (verified against
//! `bevy_core_pipeline-0.20.1/src/fullscreen_material.rs`: the bind group
//! is built from `ViewTarget::post_process_write`). A sprite whose image is
//! the offscreen picture does exactly what the plan specifies — a
//! nearest-sampled copy at an integer scale, with the clear colour showing
//! in the letterbox — through public API and nothing else.
//!
//! The other two modes: `off` renders the world straight to the window,
//! and `dlss` hands the upscale to the DLSS plugins (an optional cargo
//! feature, RTX-only — see `main`'s wiring and `lm-render`'s validation).

use bevy::camera::visibility::RenderLayers;
use bevy::camera::RenderTarget;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::window::PrimaryWindow;

use lm_render::upscale::{integer_blit, Blit};

use crate::view3d::View3dSettings;

/// The camera that draws the world's picture to the window.
///
/// It lives on the blit's own render layer (`1`), so it draws the picture
/// and nothing else — not the 2D map's sprites, not the panels — and it
/// clears with the clear colour first, which is what makes the letterbox
/// bars.
#[derive(Component)]
pub struct BlitCamera;

/// The one quad the blit draws: the offscreen picture itself.
#[derive(Component)]
pub struct BlitQuad;

/// The layer the blit lives on, apart from every other view.
const BLIT_LAYER: usize = 1;

/// The upscale's half of the world: the offscreen image, when there is one.
///
/// `None` means the world camera renders straight to the window (`off` and
/// `dlss`); `Some(handle)` is the picture the world camera renders into and
/// the blit draws from.
#[derive(Resource, Default)]
pub struct UpscaleTargets {
    /// The offscreen image, when the upscale is the integer one.
    pub image: Option<Handle<Image>>,
}

/// Create the offscreen picture and, for the integer upscale, the blit
/// camera that shows it.
///
/// Runs before [`crate::view3d::warmup::setup`], so the world camera is
/// spawned with its target already in hand.
pub fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    settings: Res<View3dSettings>,
) {
    if settings.render.upscale != lm_render::palette::UpscaleMode::Integer {
        // `off` renders to the window; `dlss` does its own upscale. Either
        // way there is no offscreen picture here.
        commands.insert_resource(UpscaleTargets { image: None });
        return;
    }

    // The picture the world renders into: an sRGB-viewed render target,
    // sampled **nearest**, so one internal pixel is one chunky pixel at any
    // scale — never a smear.
    let mut image = Image::new_target_texture(
        settings.render.internal_width,
        settings.render.internal_height,
        TextureFormat::Rgba8Unorm,
        Some(TextureFormat::Rgba8UnormSrgb),
    );
    image.sampler = ImageSampler::nearest();
    let image = images.add(image);
    commands.insert_resource(UpscaleTargets {
        image: Some(image.clone()),
    });

    // The blit camera: a 2D camera over the window, above the world camera
    // (order 0 over -1), on the blit's own layer. Its projection is driven
    // per frame by [`update`], below, to keep one world unit one *physical*
    // pixel whatever the window's scale factor.
    commands
        .spawn((
            Camera2d,
            Camera {
                order: 0,
                ..default()
            },
            RenderLayers::layer(BLIT_LAYER),
            BlitCamera,
        ))
        .with_children(|parent| {
            parent.spawn((
                // A camera's render target arrives upright as a sprite's
                // image (Bevy's image convention is row-0-at-top, and the
                // world camera writes it that way) — `flip_y` here put the
                // sky at the bottom of the window and the near ground at
                // the top, which the first Task 8 review round caught once
                // the blit itself was the right size.
                Sprite {
                    image: image.clone(),
                    ..default()
                },
                RenderLayers::layer(BLIT_LAYER),
                BlitQuad,
            ));
        });
}

/// Keep the blit exact: its size and place from [`integer_blit`], in
/// physical pixels, whatever the window is at this frame.
///
/// The sprite's units are world units, and the camera's orthographic scale
/// is `scale_factor`, so one world unit is one physical pixel. The scale is
/// `scale_factor`, **not** its inverse: `OrthographicProjection::update`
/// takes the viewport in *logical* pixels and makes the visible world
/// `scale × logical` units (`bevy_camera-0.20.1/src/projection.rs`,
/// `CameraProjection::update`), and `physical = scale_factor × logical` —
/// so `scale = scale_factor` is what cancels the logical units out. (The
/// first Task 8 screenshots drew the blit at the inverse scale, which made
/// the sprite 5× the window on a 225% desktop: one zoomed pixel of the
/// picture where fifteen belonged.)
///
/// A window smaller than the internal picture crops it, centred — the model
/// says so, and the letterbox maths is the same.
pub fn update(
    settings: Res<View3dSettings>,
    mut cameras: Query<&mut Projection, With<BlitCamera>>,
    mut quads: Query<(&mut Transform, &mut Sprite), With<BlitQuad>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    if settings.render.upscale != lm_render::palette::UpscaleMode::Integer {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let scale_factor = window.scale_factor().max(0.001);
    let blit = integer_blit(
        (window.physical_width(), window.physical_height()),
        (
            settings.render.internal_width,
            settings.render.internal_height,
        ),
    );

    // One world unit per physical pixel, whatever the desktop's scale.
    for mut projection in &mut cameras {
        if let Projection::Orthographic(orthographic) = &mut *projection {
            orthographic.scale = scale_factor;
        }
    }

    let Ok((mut transform, mut sprite)) = quads.single_mut() else {
        return;
    };
    sprite.custom_size = Some(Vec2::new(blit.size_px.0 as f32, blit.size_px.1 as f32));
    let (window_w, window_h) = (
        window.physical_width() as f32,
        window.physical_height() as f32,
    );
    // The camera sits at the window's centre; the quad's centre is its
    // place in the letterbox, measured from the window's centre.
    let centre = Vec2::new(
        blit.offset_px.0 as f32 + blit.size_px.0 as f32 * 0.5 - window_w * 0.5,
        window_h * 0.5 - (blit.offset_px.1 as f32 + blit.size_px.1 as f32 * 0.5),
    );
    *transform = Transform::from_translation(centre.extend(0.0));
}

/// The blit of the window, as it stands — or `None` when the window is not
/// there to have one.
///
/// The 3D camera controller maps a window cursor into the world camera's
/// own (internal-resolution) viewport through this: the picture is the
/// window's content, and the cursor's place inside the picture is the
/// cursor's place inside the blit, scaled back down.
pub fn cursor_blit(window: &Window, cursor: Vec2, internal: (u32, u32)) -> Option<(Blit, Vec2)> {
    let scale_factor = window.scale_factor();
    let blit = integer_blit(
        (window.physical_width(), window.physical_height()),
        internal,
    );
    // The cursor is in logical window pixels; the blit is in physical ones.
    let physical = cursor * scale_factor;
    let inside = (physical.x >= blit.offset_px.0 as f32
        && physical.y >= blit.offset_px.1 as f32
        && physical.x < (blit.offset_px.0 + blit.size_px.0 as i32) as f32
        && physical.y < (blit.offset_px.1 + blit.size_px.1 as i32) as f32)
        && blit.scale > 0;
    if !inside {
        return None;
    }
    let viewport = Vec2::new(
        (physical.x - blit.offset_px.0 as f32) / blit.scale as f32,
        (physical.y - blit.offset_px.1 as f32) / blit.scale as f32,
    );
    Some((blit, viewport))
}

/// The render target of the world camera: the offscreen picture when the
/// integer upscale is on, the window otherwise.
pub fn world_target(targets: &UpscaleTargets) -> RenderTarget {
    match &targets.image {
        Some(image) => RenderTarget::Image(image.clone().into()),
        None => RenderTarget::default(),
    }
}
