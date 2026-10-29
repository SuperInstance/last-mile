# Bevy 0.20 / bevy_egui 0.43 — the API facts this host relies on

Every fact below was verified against the actual crate sources in the cargo
registry (`bevy 0.20.1`, `bevy_egui 0.43.1`, which pulls `egui 0.36.2`), and
every snippet marked **[compiled]** is exercised by the Task 1 spike in
`crates/lm-host/src/main.rs`, which builds and runs on Windows. Later tasks
extend the host from this file — not from older-Bevy memories. Anything
marked **[source only]** is verified from the sources but not yet compiled
by us; check it off when a task first compiles it.

The big renames that bite when coming from older Bevy:

- **Events are messages.** `EventWriter`/`EventReader` are gone; use
  `MessageWriter<T>` / `MessageReader<T>` (`bevy_ecs 0.20` renamed the whole
  event machinery to `message`; `EntityEvent` remains for observers).
- **Bundles are gone.** Spawn marker/scalar components and let
  `#[require(...)]` pull in the rest: `commands.spawn(Camera2d)`,
  `commands.spawn(Sprite::from_color(...))`.
- **`bevy_wgpu` no longer exists.** wgpu is a direct dependency of
  `bevy_render`, and adapter choice lives in
  `bevy::render::settings::WgpuSettings`.
- **bevy_egui UI systems run in `EguiPrimaryContextPass`,** not `Update`.
- **egui 0.36 has no `SidePanel`.** Panels are `egui::Panel::left(...)`
  and `show` takes a `&mut Ui`, not a `&Context`.

## App, plugins, window — **[compiled]**

```rust
use bevy::prelude::*; // App, DefaultPlugins, Window, WindowPlugin, AppExit, ...

let mut app = App::new();
app.add_plugins((
    DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "last-mile".to_owned(),
                resolution: (1280, 720).into(), // WindowResolution: From<(u32, u32)>
                ..default()
            }),
            ..default()
        }),
    EguiPlugin::default(),
))
.add_systems(Startup, setup)
.add_systems(Update, count_frames)
.run();
```

- `fn main() -> AppExit` works: `impl Termination for AppExit`
  (`bevy_app/src/app.rs:1598`), and `App::run` returns `AppExit`.
  `AppExit::Error(NonZeroU8)` for failures, `AppExit::Success` otherwise.
- App-level "events" go through `MessageWriter<AppExit>`
  (`app.add_message::<AppExit>()` is pre-registered by `App` itself).

## The high-performance GPU — **[compiled]**

```rust
use bevy::render::{RenderPlugin, settings::{PowerPreference, RenderCreation, WgpuSettings}};

DefaultPlugins.set(RenderPlugin {
    render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
        power_preference: PowerPreference::HighPerformance, // the RTX 4050, not the iGPU
        ..default()
    })),
    ..default()
})
```

- `RenderCreation::Automatic` takes a **`Box<WgpuSettings>`**
  (`bevy_render/src/settings.rs:226`).
- `WgpuSettings::default()` already picks `HighPerformance` *unless the
  `WGPU_POWER_PREF` env var overrides it* (`settings.rs:86`); setting it
  explicitly makes it immune to the environment. `PowerPreference` is
  re-exported at `bevy::render::settings::PowerPreference`.

## `Camera2d` — **[compiled]**

```rust
commands.spawn(Camera2d); // that's the whole camera
```

`Camera2d` is a marker whose `#[require(Camera, Projection::Orthographic(..),
Frustum)]` pulls in everything else
(`bevy_camera/src/components.rs:14-21`). It's in `bevy::prelude`. An app with
no windows exits by default (`exit_on_all_closed` in `WindowPlugin`).

## Sprites — **[compiled]**

```rust
commands.spawn(Sprite::from_color(Color::srgb(0.85, 0.30, 0.19), Vec2::splat(128.0)));
```

- `Sprite::from_color(impl Into<Color>, Vec2)` sets the tint and
  `custom_size` (`bevy_sprite/src/sprite.rs:73`); `Sprite` carries
  `Transform`/`Visibility`/`Anchor` via `#[require]`, so nothing else is
  needed to spawn a coloured square.
- `Color::srgb(r, g, b)` is a `const fn` (`bevy_color/src/color.rs:123`), so
  palette constants can be `const SQUARE_COLOR: Color = Color::srgb(..);`.

## `FixedUpdate`, its timestep, and the overstep fraction — **[compiled]**

```rust
app.insert_resource(Time::<Fixed>::from_hz(64.0))  // Params::tick_hz lands here (Task 2)
   .add_systems(FixedUpdate, |mut ticks: ResMut<FixedTicks>| ticks.0 += 1);

fn render_side(fixed: Res<Time<Fixed>>) {
    let alpha = fixed.overstep_fraction(); // f32 in 0..1 between the last and next fixed tick
}
```

- `Time::<Fixed>::from_hz(f64)` (`bevy_time/src/fixed.rs:105`); inserting the
  resource before or after `DefaultPlugins` both work — `TimePlugin` only
  `init_resource`s it when absent.
- `overstep_fraction() -> f32` and `overstep_fraction_f64() -> f64` live on
  `Time<Fixed>` (`fixed.rs:205`). `Fixed`, `FixedUpdate`, `Time` are all in
  `bevy::prelude`.
- If the window loses focus or frames drop, winit's runner drains however
  many `FixedUpdate`s are owed (capped by `max_delta_overflow`, default 8
  ticks) — the sim never silently skips ticks.

## Screenshots: capture and exit — **[compiled]**

```rust
use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};

commands
    .spawn(Screenshot::primary_window())
    .observe(save_to_disk("C:\\Users\\casey\\src\\shot1.png"))
    .observe(quit_after_capture);

fn quit_after_capture(_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>) {
    exit.write(AppExit::Success);
}
```

- `Screenshot(pub RenderTarget)` + constructors `window(Entity)`,
  `primary_window()`, `image(Handle<Image>)`
  (`bevy_render/src/view/window/screenshot.rs:100`).
- **Capture is asynchronous**: the PNG bytes arrive a frame or two later via
  the `ScreenshotCaptured` EntityEvent, and the screenshot entity is
  despawned after the observers run. Never exit on a frame count alone —
  exit from the observer, or the file may not be written yet.
- `ScreenshotCaptured` derefs to the bevy `Image` if you want the pixels
  in-memory instead of on disk.
- The capture includes everything composited to the window (sprites *and*
  egui panels), so it is how reviewers verify visuals without a screen.

## Observers — **[compiled]**

```rust
// Observers are systems: On<E> plus any other system params.
fn observe_it(_: On<ScreenshotCaptured>, mut commands: Commands, q: Query<&Transform>) { .. }
entity_commands.observe(observe_it);
```

`On<E>` is the trigger param (`bevy_ecs/src/observer/system_param.rs:25`);
observers take ordinary system params alongside it — the crate's own tests
use `ResMut` in observers (`bevy_ecs/src/observer/mod.rs:539`).

## Knowing the app is closing — **[compiled]** (Task 7)

```rust
use bevy::app::OnAppExitSystems;

app.add_systems(
    Last,
    write_the_round
        .in_set(OnAppExitSystems)
        .run_if(|messages: Res<Messages<AppExit>>| !messages.is_empty()),
);
```

- **`OnAppExitSystems` is a `SystemSet` in `bevy_app`**
  (`bevy_app/src/app.rs:1532`), reached as `bevy::app::OnAppExitSystems`; it
  is *not* in the prelude. The window plugin configures it in `Last`, after
  its own `ExitSystems` set (`bevy_window/src/lib.rs:154`).
- **The exit is a message, not an event**: `bevy_window`'s
  `exit_on_all_closed` writes `AppExit::Success` with a `MessageWriter`
  (`bevy_window/src/system.rs:18`), and the runner ends the app when it sees
  it. `Messages<AppExit>` is in the prelude.
- The run condition above is the idiom Bevy itself uses
  (`bevy_time/src/lib.rs:92`): the messages are still there when `Last`
  runs, because they are cleared by the next message-update cycle.
- `Last` runs inside the `Main` schedule, so a system there sees the frame
  the exit was asked for — and the app ends after that frame.
- An **exclusive system** (`fn sys(world: &mut World)`) is the way to take a
  resource *out* of the world: `world.remove_resource::<T>()`,
  `world.resource_mut::<T>()`, `world.resource::<T>()`.

## bevy_egui 0.43.1 panels — **[compiled]**

```rust
use bevy_egui::{egui, EguiContexts, EguiPlugin, EguiPrimaryContextPass};

app.add_plugins(EguiPlugin::default())
   .add_systems(Startup, |mut commands: Commands| commands.spawn(Camera2d)) // egui needs a camera
   .add_systems(EguiPrimaryContextPass, hello_panel);

fn hello_panel(mut contexts: EguiContexts) -> Result {
    let ctx = contexts.ctx_mut()?.clone();   // Result<&mut Context, QuerySingleError> — note the ?
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::Panel::left("spike_side")
        .resizable(true)
        .default_size(220.0)
        .show(&mut root, |ui| {
            ui.heading("last-mile");
            ui.label("hello");
        });
    Ok(())
}
```

- UI systems go in **`EguiPrimaryContextPass`** (a `ScheduleLabel` exported
  by bevy_egui): the plugin reruns that schedule for egui's extra passes
  (multi-pass immediate mode). Putting them in `Update` draws one stale
  pass.
- **`EguiPrimaryContextPass` runs in `PostUpdate`**, in the
  `EguiPostUpdateSet::EndPass` set (`bevy_egui/src/lib.rs:1147`): the panels
  are drawn after the frame's `Update` systems, so a slider that moves the
  world is seen on the *next* frame, not this one.
- **`egui::Slider` ignores the size it is given**: it allocates
  `ui.spacing().slider_width` (100 px by default,
  `egui/src/widgets/slider.rs:652`), so `add_sized` does not widen it — set
  `ui.spacing_mut().slider_width` before adding it.
- `EguiContexts::ctx_mut()` returns `Result<&mut egui::Context,
  QuerySingleError>`; the intended system return type is
  `bevy::prelude::Result` (= `Result<(), BevyError>`) and `ctx_mut()?`
  converts.
- The egui context is auto-attached to **the first camera** spawned
  (`EguiGlobalSettings::auto_create_primary_context` defaults true) — spawn
  the `Camera2d` in `Startup` before relying on the panel.
- `egui::Window::new("t").show(&ctx, |ui| ..)` still takes a `&Context`
  directly; only the edge-docked **panels** need the root-`Ui` construction
  above, because `egui 0.36`'s `Panel::show` takes `&mut Ui`
  (`egui/src/containers/panel.rs:422`) — `SidePanel`/`TopBottomPanel` no
  longer exist.
- Useful `Panel` builders: `.resizable(bool)`, `.default_size(f32)`,
  `.min_size`, `.max_size`, `.exact_size`, `.show_separator_line`.
- The egui crate itself is re-exported as `bevy_egui::egui` (0.36.2).

## Gizmos — **[compiled]** (Task 6: the map's rings and the agents' facing)

```rust
use bevy::prelude::*; // Gizmos is in the prelude via bevy_gizmos

fn draw_agents(mut gizmos: Gizmos) {
    gizmos.circle_2d(agent_pos_m, radius_m, squad_color);        // outline circle
    gizmos.circle_2d(agent_pos_m, radius_m, squad_color).resolution(32);
    gizmos.line_2d(from_m, to_m, color);
    gizmos.rect_2d(center_m, full_size_m, color);
}
```

- System param is plain `mut gizmos: Gizmos` (`bevy_gizmos/src/gizmos.rs:143`;
  `Gizmos` is in `bevy::prelude`). `GizmoPlugin` ships in `DefaultPlugins`.
- `circle_2d` takes `impl Into<Isometry2d>` — plain `Vec2` converts
  (`bevy_math/src/isometry.rs:195`) — plus `radius: f32` and color
  (`bevy_gizmos/src/circles.rs:167`); it returns a builder with
  `.resolution(n)`. `line_2d(Vec2, Vec2, color)` is at `gizmos.rs:722`;
  `rect_2d(Isometry2d, full_size: Vec2, color)` at `gizmos.rs:898`.
  Colors accept `impl Into<Color>`, so `Color::srgb` palette constants work.
  2D gizmo coordinates are world metres — exactly the `f32` metres
  `lm-host-model::map` produces.
- **The 2D shapes are outlines, not fills:** `circle_2d` draws a line loop.
  A filled disc is a sprite with a disc texture (below), not a gizmo.
- Gizmos are drawn **on top of the sprites**: they join the `Transparent2d`
  phase with `sort_key: FloatOrd(f32::INFINITY)`
  (`bevy_gizmos_render/src/pipeline_2d.rs:332`), and sprites are sorted by
  their own `translation().z` (`bevy_sprite_render/src/render/mod.rs:502`).
  That is also how the layers are ordered: give a sprite a bigger `z` to put
  it in front (the 2D sprite pipeline does not write depth:
  `depth_write_enabled: false`, `depth_compare: GreaterEqual`).

## Sprites as bodies: a disc of your own — **[compiled]** (Task 6)

```rust
use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageSampler};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

let mut data = vec![0u8; (SIZE * SIZE * 4) as usize];   // RGBA, one byte per channel
let mut image = Image::new(
    Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
    TextureDimension::D2,
    data,
    TextureFormat::Rgba8UnormSrgb,
    RenderAssetUsages::default(),
);
image.sampler = ImageSampler::linear();
let handle = images.add(image);        // Assets::add, not insert
```

- `Assets::insert(id, asset)` needs an id; **`Assets::add(asset) -> Handle`**
  is the one that hands back a handle (`bevy_asset/src/assets.rs:401`).
- `Image::new(size, dimension, data, format, usages)` wants the pixel bytes
  up front (`bevy_image/src/image.rs:1123`); `RenderAssetUsages::default()`
  is `MAIN_WORLD | RENDER_WORLD`, so the texture reaches the GPU.
- `Image` is in the prelude, but **`ImageSampler` is not** — it lives in
  `bevy::image::ImageSampler`. `Extent3d`, `TextureDimension` and
  `TextureFormat` are **not** in the prelude either: they come from
  `bevy::render::render_resource`.
- A sprite with a texture is `Sprite { image: handle, custom_size: Some(..),
  color: .., ..default() }`: the field is `image` (not `texture`), and
  `custom_size` is the size in world units.
- `Sprite::from_color(colour, size)` uses the default white texture and
  needs no image at all — that is how the tile layer is drawn.

## Two queries may not fight over a component — **[compiled]** (Task 6)

Two systems may not both write the same component; two *queries inside one
system* may not either, unless their filters make them disjoint:

```
error[B0001]: Query<(&mut Sprite, ...)> accesses component(s) Sprite in a
way that conflicts with a previous system parameter.
```

The way out is one query over a tag component, or `Without<T>` filters that
prove the sets apart. The map uses one query over a `Body` enum
(`Fog(index) | Agent(id) | Beacon(index)`), which is easier to read too.

## The camera's projection — **[compiled]** (Task 6)

```rust
for mut projection in &mut projections {          // Query<&mut Projection, With<Camera>>
    if let Projection::Orthographic(orthographic) = &mut *projection {
        orthographic.scale = scale;               // world units per pixel
    }
}
```

- `Projection` and `OrthographicProjection` are in the prelude
  (`bevy_camera::prelude`). `Projection::Orthographic(..)` is the variant;
  `Camera2d` requires it with `scale: 1.0` (`default_2d`).
- `scale` is **world units per pixel**: `scale = 0.2` shows 5 pixels a
  metre. The camera's `Transform::translation` is the world point at the
  centre of the window.

## Input: the wheel is a message — **[compiled]** (Task 6)

```rust
fn read_input(mouse: Res<ButtonInput<MouseButton>>, mut wheel: MessageReader<MouseWheel>) {
    let pressed = mouse.just_pressed(MouseButton::Left);
    let scroll: f32 = wheel.read().map(|wheel| wheel.y).sum();
}
```

- The buttons are a resource (`ButtonInput<T>`), with `pressed`,
  `just_pressed`, `just_released` and `get_pressed()` (there is no
  `read()` on `ButtonInput` in 0.20).
- **The wheel is a message, not a button**: `MouseWheel` is not `Eq`/`Hash`,
  so `ButtonInput<MouseWheel>` does not compile. Read it with
  `MessageReader<MouseWheel>`.
- `PrimaryWindow` is in `bevy::window::PrimaryWindow`;
  `window.cursor_position()` gives the pointer in logical pixels.

## egui 0.36: the corners of a rect — **[compiled]** (Task 6)

- `egui::Rect` keeps its corners in `min` and `max` (`emath/src/rect.rs:25`):
  `rect.min.x`, `rect.max.y`. There are no `x_min`/`y_min` **fields** (only
  `width()` and `height()` methods).
- `Painter::rect_filled(rect, corner_radius, colour)` takes
  `impl Into<CornerRadius>`, and `CornerRadius` has `From<f32>`, so `0.0` is
  a square corner.
- `Painter::text(pos, anchor, text, font_id, colour)` lays the text out
  itself; `FontId` is `Clone` but not `Copy`.
- `Ui::allocate_exact_size(size, sense) -> (Rect, Response)` is the way to
  get a canvas of a known size to paint in.
- egui has its own `Vec2` (emath) and Bevy has another (glam): they are
  different types. Use `egui::vec2(..)` and `egui::pos2(..)` inside egui
  calls, and `bevy::prelude::Vec2` everywhere else.
- `Panel::top(..)`/`Panel::bottom(..)` take the same builders as
  `Panel::left(..)` (`.resizable`, `.default_size`, `.exact_size`, ..), and
  `exact_size` on a top or bottom panel is its **height**.

## Building lm-host (why a Windows-only crate) — **[compiled]**

`lm-host` cannot build in this WSL toolchain: Bevy on Linux links alsa, udev
and x11 (bevy_egui additionally wants libxcb headers), and the toolchain has
no `pkg-config`, let alone those libraries. So:

- The workspace root sets `default-members = ["crates/lm-core",
  "crates/lm-proto", "crates/lm-tools"]`. Bare `cargo build/test/clippy` in
  WSL therefore skip `lm-host` while `cargo fmt --all` (parse-only) still
  formats it. `scripts/check.sh` accordingly runs clippy/tests without
  `--workspace` (that flag would override `default-members` and pull Bevy
  into WSL). `lm-host` is never touched by the no-float guard; floats are
  host-only by design (Phase 1 constraint D8).
- To build/run it: `bash scripts/win-build.sh <cargo args...>`. The script
  rsync-mirrors this worktree to `C:\Users\casey\src\last-mile-build\`
  (excluding `target/`, `.git/`, `runs/`; the Windows-side `target/` is kept
  so rebuilds are incremental) and runs Windows `cargo` there via
  `cmd.exe` — never across `\\wsl.localhost`.

```bash
bash scripts/win-build.sh check -p lm-host
bash scripts/win-build.sh clippy -p lm-host -- -D warnings
bash scripts/win-build.sh run -p lm-host --release -- \
    --screenshot 'C:\Users\casey\src\shot1.png' --frames 30
```

Toolchain note: both sides are cargo 1.99.x (Windows 1.99.1), matching
`rust-toolchain.toml`.

## Rendering to a texture, and the integer upscale — **[compiled]** (Tasks 7-8)

```rust
use bevy::camera::RenderTarget;
use bevy::image::ImageSampler;
use bevy::render::render_resource::TextureFormat;

// The offscreen picture: sRGB-viewed, zero-filled, sampled NEAREST.
let mut image = Image::new_target_texture(w, h, TextureFormat::Rgba8Unorm,
                                          Some(TextureFormat::Rgba8UnormSrgb));
image.sampler = ImageSampler::nearest();
let handle = images.add(image);

// The world camera: the target is its own COMPONENT, not a Camera field.
commands.spawn((
    Camera3d::default(),
    Camera { order: -1, ..default() },
    RenderTarget::Image(handle.into()),   // From<Handle<Image>> sets scale_factor 1.0
    DistanceFog {                          // bevy_pbr's fog, in the prelude
        color: Color::srgb(0.30, 0.24, 0.38),
        falloff: FogFalloff::Linear { start: 40.0, end: 120.0 },
        ..default()
    },
));
```

- **`Camera` has no `target` field in 0.20** (`bevy_camera-0.20.1/src/camera.rs`):
  `RenderTarget` is a `Component`, inserted alongside `Camera`; its default
  is `RenderTarget::Window(WindowRef::default())`. `Camera`'s real fields
  are `viewport`, `order`, `is_active`, `output_mode`, `msaa_writeback`,
  `clear_color`, …
- `Image::new_target_texture` (`bevy_image-0.20.1/src/image.rs:1253`) sets
  `TEXTURE_BINDING | COPY_DST | RENDER_ATTACHMENT` itself; the sRGB *view*
  format comes from the `Some(..)` argument. MSAA 4× into such a target is
  fine (the official `render_to_texture.rs` does it).
- **The blit is a sprite, not `FullscreenMaterial`.** 0.20's
  `FullscreenMaterialPlugin` is a *per-camera post-process*: its render
  system binds `ViewTarget::post_process_write()` — the camera's own
  accumulated texture — and cannot sample another camera's render target
  (`bevy_core_pipeline-0.20.1/src/fullscreen_material.rs`,
  `fullscreen_material_system`). A `Camera2d` (order above the world
  camera) drawing one `Sprite { image: handle }` sized `scale × internal`
  does the specified nearest integer blit through public API. Its
  `OrthographicProjection.scale = window.scale_factor()` keeps one world
  unit one **physical** pixel, so the integer scale stays integer on a
  HiDPI desktop. (The scale is the factor itself, **not** its inverse:
  `CameraProjection::update` receives the viewport in **logical** pixels
  and makes the visible world `scale × logical`
  (`bevy_camera-0.20.1/src/projection.rs`; the caller is `camera_system`
  in `bevy_render-0.20.1/src/camera.rs`, which passes
  `camera.logical_viewport_size()`), and `physical = scale_factor ×
  logical` — so `scale = scale_factor` is what cancels the logical units
  out. The Task 8 review caught an inverse scale here drawing the blit 5×
  the window on a 225% desktop.)
- **`DistanceFog` works on unlit materials**: it is applied in
  `main_pass_post_lighting_processing` (`bevy_pbr-0.20.1/src/render/
  pbr_functions.wesl:987`), gated by the material's `fog_enabled` flag,
  which defaults on. `DistanceFog`/`FogFalloff` are in `bevy::prelude`.

## Cameras over cameras, cursor picking — **[compiled]** (Task 8)

```rust
// An overlay camera that clears nothing and draws only its layer:
commands.spawn((Camera2d, Camera {
    order: 10,                       // above the blit camera (order 0)
    clear_color: ClearColorConfig::None,
    ..default()
}, RenderLayers::layer(2)));

// Switching views without touching the round: is_active flips cameras.
camera.is_active = !showing_3d;

// A cursor through the offscreen picture: window px → blit px → the world
// camera's own (internal-resolution) viewport → a ray → the ground.
let ray = camera.viewport_to_world(&global_transform, viewport_px)?;   // top-left origin
let hit = ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
```

- `bevy_egui` attaches its context to the **first camera the app creates**
  (`setup_primary_egui_context_system`, `Added<Camera>`). In a layout with
  several cameras, attach the context by hand and turn the auto-attach
  **off** — `EguiGlobalSettings { auto_create_primary_context: false, .. }`
  plus `EguiContext::default(), PrimaryEguiContext` on the panel carrier:
  the auto system runs in PreStartup *and* PreUpdate, each instance with its
  own "already have one" flag, so cameras spawning across both schedules
  can end up with a *second* `PrimaryEguiContext` (whichever the query
  yields first), and every panel system asking for "the" context finds two
  (`QuerySingleError::MultipleEntities`). The carrier still comes first —
  `app.world_mut().spawn(...)` before `app.run()` beats racing `Startup`
  systems — and on its own `RenderLayers`, so it draws the egui pass and
  nothing else.
- `Camera::viewport_to_world(&GlobalTransform, Vec2)` takes the cursor in
  the viewport's logical pixels, **top-left origin** (same as
  `Window::cursor_position()`; the official `3d_viewport_to_world.rs`
  passes one straight into the other). For a camera rendering offscreen,
  the viewport is the *image*, so map window px through the blit first.
- `Ray3d::plane_intersection_point(origin, InfinitePlane3d)` replaces the
  hand-rolled `t = -o.y/d.y`.

## Version pins

- `bevy = "=0.20.1"`, `bevy_egui = "=0.43.1"` (exact pins; bevy_egui 0.43.1
  is built against bevy 0.20 subcrates and egui 0.36.2).
- The spike's full tree resolves to 626 locked packages in `Cargo.lock`.
