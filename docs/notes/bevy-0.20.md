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

## Gizmos — **[source only]** (first map-drawing use: Task 6)

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

## Version pins

- `bevy = "=0.20.1"`, `bevy_egui = "=0.43.1"` (exact pins; bevy_egui 0.43.1
  is built against bevy 0.20 subcrates and egui 0.36.2).
- The spike's full tree resolves to 626 locked packages in `Cargo.lock`.
