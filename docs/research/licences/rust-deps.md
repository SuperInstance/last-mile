# Rust Dependency Licenses

Total packages: 624
Permissive licenses: 619
Review required: 5

## Packages Requiring Review

| Package | Version | License | Dependent Crates |
|--------|---------|---------|------------------|
| epaint_default_fonts | 0.36.2 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | epaint |
| lm-agents | 0.1.0 | UNKNOWN | lm-host, lm-host-model, lm-learn, lm-tools |
| lm-learn | 0.1.0 | UNKNOWN | lm-agents, lm-host, lm-host-model, lm-tools |
| smartstring | 1.0.1 | MPL-2.0+ | rhai |
| webpki-roots | 1.0.11 | CDLA-Permissive-2.0 | rustls, ureq |

## All Packages

| Package | Version | License | License Class | Dependent Crates |
|--------|---------|---------|---------------|------------------|
| ab_glyph | 0.2.32 | Apache-2.0 | PERMISSIVE | sctk-adwaita |
| ab_glyph_rasterizer | 0.1.10 | Apache-2.0 | PERMISSIVE | ab_glyph |
| accesskit | 0.24.1 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_consumer, accesskit_ios, accesskit_macos, accesskit_windows, accesskit_winit, bevy, bevy_a11y, bevy_feathers, bevy_ui, bevy_ui_widgets, bevy_winit, egui, parley |
| accesskit_consumer | 0.38.0 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_ios, accesskit_macos, accesskit_windows |
| accesskit_ios | 0.1.2 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_winit |
| accesskit_macos | 0.26.3 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_winit |
| accesskit_windows | 0.34.0 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_winit |
| accesskit_winit | 0.33.2 | Apache-2.0 | PERMISSIVE | bevy_winit |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | PERMISSIVE | miniz_oxide, simd-adler32 |
| ahash | 0.8.12 | MIT OR Apache-2.0 | PERMISSIVE | egui, epaint, foldhash, petgraph, rhai, tracing-log, weak-table, winit |
| aho-corasick | 1.1.5 | Unlicense OR MIT | PERMISSIVE | regex, regex-automata |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | PERMISSIVE | bumpalo, hashbrown |
| alsa | 0.11.0 | Apache-2.0/MIT | PERMISSIVE | cpal |
| alsa-sys | 0.4.0 | MIT | PERMISSIVE | alsa |
| android-activity | 0.6.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_android, webbrowser, winit |
| android-properties | 0.2.2 | MIT | PERMISSIVE | android-activity |
| android_log-sys | 0.3.2 | MIT OR Apache-2.0 | PERMISSIVE | bevy_log |
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | PERMISSIVE | wgpu-hal |
| annotate-snippets | 0.12.17 | MIT OR Apache-2.0 | PERMISSIVE | wesl, wgsl-parse, winnow |
| anstream | 1.0.0 | MIT OR Apache-2.0 | PERMISSIVE | annotate-snippets, clap_builder, toml, toml_edit, toml_parser, wesl, winnow |
| anstyle | 1.0.14 | MIT OR Apache-2.0 | PERMISSIVE | annotate-snippets, anstream, anstyle-wincon, clap_builder, clap_derive, toml, toml_edit, toml_parser, winnow |
| anstyle-parse | 1.0.0 | MIT OR Apache-2.0 | PERMISSIVE | anstream |
| anstyle-query | 1.1.5 | MIT OR Apache-2.0 | PERMISSIVE | anstream |
| anstyle-wincon | 3.0.11 | MIT OR Apache-2.0 | PERMISSIVE | anstream |
| approx | 0.5.1 | Apache-2.0 | PERMISSIVE | bevy_curve, bevy_math, bevy_mesh, bevy_shape, bevy_sprite, bevy_transform, bevy_winit, glam, gltf, png, rodio |
| arboard | 3.7.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_clipboard, bevy_egui |
| arrayref | 0.3.9 | BSD-2-Clause | PERMISSIVE | tiny-skia, tiny-skia-path |
| arrayvec | 0.7.8 | MIT OR Apache-2.0 | PERMISSIVE | anstyle-parse, bevy_ecs, bevy_pbr, bevy_shape, blake3, const_panic, kurbo, naga, polycool, taffy, tiny-skia, typewit, wgpu, wgpu-core, wgpu-hal |
| as-raw-xcb-connection | 1.0.1 | MIT OR Apache-2.0 | PERMISSIVE | x11rb |
| ash | 0.38.0+1.3.281 | MIT OR Apache-2.0 | PERMISSIVE | gpu-allocator, wgpu-hal |
| assert_type_match | 0.1.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_reflect |
| async-broadcast | 0.7.2 | MIT OR Apache-2.0 | PERMISSIVE | bevy_asset |
| async-channel | 2.5.0 | Apache-2.0 OR MIT | PERMISSIVE | async-executor, async-io, bevy_asset, bevy_render, bevy_tasks, bevy_utils, blocking, piper |
| async-executor | 1.14.0 | Apache-2.0 OR MIT | PERMISSIVE | bevy_tasks, piper |
| async-fs | 2.2.0 | Apache-2.0 OR MIT | PERMISSIVE | bevy_asset |
| async-io | 2.6.0 | Apache-2.0 OR MIT | PERMISSIVE | async-executor, bevy_asset, bevy_platform, piper |
| async-lock | 3.4.2 | Apache-2.0 OR MIT | PERMISSIVE | async-executor, async-fs, bevy_asset, bevy_gltf |
| async-task | 4.7.1 | Apache-2.0 OR MIT | PERMISSIVE | async-executor, bevy_tasks, blocking, calloop, web-task |
| atomic-waker | 1.1.2 | Apache-2.0 OR MIT | PERMISSIVE | async-task, bevy_diagnostic, bevy_tasks, piper, winit |
| atomicow | 1.2.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_asset |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | PERMISSIVE | async-io, num-traits, smartstring |
| base64 | 0.23.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_gltf, cookie, gltf, ron, rustls, rustls-webpki, ttf-parser, ureq, ureq-proto |
| bevy | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_egui, lm-host |
| bevy_a11y | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_egui, bevy_feathers, bevy_internal, bevy_ui, bevy_ui_render, bevy_ui_widgets, bevy_winit |
| bevy_android | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_asset, bevy_internal, bevy_winit |
| bevy_animation | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_dev_tools, bevy_gltf, bevy_internal |
| bevy_animation_macros | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation |
| bevy_anti_alias | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_internal |
| bevy_app | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_a11y, bevy_animation, bevy_anti_alias, bevy_asset, bevy_audio, bevy_camera, bevy_clipboard, bevy_core_pipeline, bevy_dev_tools, bevy_diagnostic, bevy_egui, bevy_extract, bevy_feathers, bevy_gilrs, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_image, bevy_input, bevy_input_focus, bevy_internal, bevy_light, bevy_log, bevy_mesh, bevy_pbr, bevy_picking, bevy_post_process, bevy_render, bevy_scene, bevy_sprite, bevy_sprite_render, bevy_state, bevy_text, bevy_time, bevy_transform, bevy_ui, bevy_ui_render, bevy_ui_widgets, bevy_window, bevy_winit, bevy_world_serialization |
| bevy_asset | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_animation, bevy_anti_alias, bevy_audio, bevy_camera, bevy_clipboard, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_feathers, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_image, bevy_internal, bevy_light, bevy_material, bevy_mesh, bevy_pbr, bevy_picking, bevy_post_process, bevy_render, bevy_scene, bevy_shader, bevy_sprite, bevy_sprite_render, bevy_text, bevy_ui, bevy_ui_render, bevy_window, bevy_winit, bevy_world_serialization |
| bevy_asset_macros | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_asset |
| bevy_audio | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_internal |
| bevy_camera | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_anti_alias, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_extract, bevy_feathers, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_input_focus, bevy_internal, bevy_light, bevy_pbr, bevy_picking, bevy_post_process, bevy_render, bevy_sprite, bevy_sprite_render, bevy_ui, bevy_ui_render, bevy_ui_widgets, bevy_world_serialization |
| bevy_clipboard | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_internal, bevy_text |
| bevy_color | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_camera, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_feathers, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_image, bevy_internal, bevy_light, bevy_pbr, bevy_post_process, bevy_render, bevy_sprite, bevy_sprite_render, bevy_text, bevy_ui, bevy_ui_render |
| bevy_core_pipeline | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_anti_alias, bevy_dev_tools, bevy_egui, bevy_gizmos_render, bevy_internal, bevy_pbr, bevy_post_process, bevy_sprite_render, bevy_ui, bevy_ui_render |
| bevy_curve | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_color, bevy_gizmos, bevy_gltf, bevy_internal, bevy_light, bevy_post_process |
| bevy_derive | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_a11y, bevy_animation, bevy_anti_alias, bevy_app, bevy_camera, bevy_core_pipeline, bevy_egui, bevy_extract, bevy_feathers, bevy_internal, bevy_material, bevy_mesh, bevy_pbr, bevy_picking, bevy_post_process, bevy_render, bevy_scene, bevy_sprite, bevy_sprite_render, bevy_text, bevy_ui, bevy_ui_render, bevy_winit, bevy_world_serialization |
| bevy_dev_tools | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_internal |
| bevy_diagnostic | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_anti_alias, bevy_asset, bevy_core_pipeline, bevy_dev_tools, bevy_internal, bevy_pbr, bevy_render |
| bevy_ecs | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_a11y, bevy_animation, bevy_anti_alias, bevy_app, bevy_asset, bevy_audio, bevy_camera, bevy_clipboard, bevy_core_pipeline, bevy_dev_tools, bevy_diagnostic, bevy_egui, bevy_extract, bevy_feathers, bevy_gilrs, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_image, bevy_input, bevy_input_focus, bevy_internal, bevy_light, bevy_log, bevy_material, bevy_mesh, bevy_pbr, bevy_picking, bevy_post_process, bevy_render, bevy_scene, bevy_sprite, bevy_sprite_render, bevy_state, bevy_text, bevy_time, bevy_transform, bevy_ui, bevy_ui_render, bevy_ui_widgets, bevy_window, bevy_winit, bevy_world_serialization |
| bevy_ecs_macro_logic | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_ecs_macros, bevy_scene_macros |
| bevy_ecs_macros | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_ecs |
| bevy_egui | 0.43.1 | MIT | PERMISSIVE | lm-host |
| bevy_encase_derive | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_mesh, bevy_render |
| bevy_extract | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_anti_alias, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_gizmos_render, bevy_internal, bevy_pbr, bevy_post_process, bevy_render, bevy_sprite_render, bevy_ui_render |
| bevy_extract_macros | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_extract, bevy_render, bevy_render_macros |
| bevy_feathers | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_internal |
| bevy_gilrs | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_internal |
| bevy_gizmos | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_gizmos_render, bevy_internal, bevy_light |
| bevy_gizmos_macros | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_gizmos |
| bevy_gizmos_render | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_internal |
| bevy_gltf | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_internal, bevy_pbr |
| bevy_image | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_anti_alias, bevy_camera, bevy_clipboard, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_gizmos_render, bevy_gltf, bevy_internal, bevy_light, bevy_pbr, bevy_post_process, bevy_render, bevy_sprite, bevy_sprite_render, bevy_text, bevy_ui, bevy_ui_render, bevy_window, bevy_winit |
| bevy_input | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_egui, bevy_feathers, bevy_gilrs, bevy_gizmos, bevy_input_focus, bevy_internal, bevy_picking, bevy_ui, bevy_ui_widgets, bevy_window, bevy_winit |
| bevy_input_focus | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_feathers, bevy_internal, bevy_ui, bevy_ui_render, bevy_ui_widgets, bevy_winit |
| bevy_internal | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy |
| bevy_light | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_core_pipeline, bevy_dev_tools, bevy_gltf, bevy_internal, bevy_pbr |
| bevy_log | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_clipboard, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_extract, bevy_feathers, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_internal, bevy_light, bevy_pbr, bevy_render, bevy_scene, bevy_sprite, bevy_text, bevy_ui, bevy_ui_widgets, bevy_winit |
| bevy_macro_utils | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation_macros, bevy_asset_macros, bevy_derive, bevy_ecs_macro_logic, bevy_ecs_macros, bevy_encase_derive, bevy_extract_macros, bevy_gizmos_macros, bevy_material_macros, bevy_reflect_derive, bevy_render_macros, bevy_scene_macros, bevy_state_macros |
| bevy_material | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_gizmos_render, bevy_gltf, bevy_internal, bevy_pbr, bevy_render, bevy_sprite_render |
| bevy_material_macros | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_material, bevy_render |
| bevy_math | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_anti_alias, bevy_audio, bevy_camera, bevy_color, bevy_core_pipeline, bevy_curve, bevy_dev_tools, bevy_egui, bevy_feathers, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_image, bevy_input, bevy_input_focus, bevy_internal, bevy_light, bevy_mesh, bevy_pbr, bevy_picking, bevy_post_process, bevy_render, bevy_shape, bevy_sprite, bevy_sprite_render, bevy_text, bevy_transform, bevy_ui, bevy_ui_render, bevy_ui_widgets, bevy_window, bevy_winit |
| bevy_mesh | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_camera, bevy_dev_tools, bevy_egui, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_internal, bevy_light, bevy_material, bevy_pbr, bevy_picking, bevy_render, bevy_sprite, bevy_sprite_render, bevy_ui_render |
| bevy_mikktspace | 1.0.0 | Zlib AND (MIT OR Apache-2.0) | PERMISSIVE | bevy_mesh |
| bevy_pbr | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_gizmos_render, bevy_internal |
| bevy_picking | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_egui, bevy_feathers, bevy_input_focus, bevy_internal, bevy_sprite, bevy_ui, bevy_ui_widgets |
| bevy_platform | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_app, bevy_asset, bevy_clipboard, bevy_core_pipeline, bevy_dev_tools, bevy_diagnostic, bevy_ecs, bevy_egui, bevy_extract, bevy_feathers, bevy_gilrs, bevy_gltf, bevy_image, bevy_input, bevy_internal, bevy_light, bevy_log, bevy_material, bevy_mesh, bevy_pbr, bevy_picking, bevy_post_process, bevy_reflect, bevy_render, bevy_scene, bevy_shader, bevy_sprite_render, bevy_state, bevy_tasks, bevy_text, bevy_time, bevy_ui, bevy_ui_render, bevy_ui_widgets, bevy_utils, bevy_window, bevy_winit, bevy_world_serialization |
| bevy_post_process | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_internal |
| bevy_ptr | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_ecs, bevy_internal, bevy_reflect |
| bevy_reflect | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_a11y, bevy_animation, bevy_anti_alias, bevy_app, bevy_asset, bevy_audio, bevy_camera, bevy_color, bevy_core_pipeline, bevy_curve, bevy_dev_tools, bevy_ecs, bevy_egui, bevy_extract, bevy_feathers, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_image, bevy_input, bevy_input_focus, bevy_internal, bevy_light, bevy_material, bevy_math, bevy_mesh, bevy_pbr, bevy_picking, bevy_post_process, bevy_render, bevy_scene, bevy_shader, bevy_shape, bevy_sprite, bevy_sprite_render, bevy_state, bevy_text, bevy_time, bevy_transform, bevy_ui, bevy_ui_render, bevy_ui_widgets, bevy_window, bevy_winit, bevy_world_serialization, compact_str |
| bevy_reflect_derive | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_reflect |
| bevy_render | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_anti_alias, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_feathers, bevy_gizmos_render, bevy_internal, bevy_pbr, bevy_post_process, bevy_sprite_render, bevy_ui_render |
| bevy_render_macros | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_render |
| bevy_scene | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_camera, bevy_feathers, bevy_internal |
| bevy_scene_macros | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_scene |
| bevy_shader | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_anti_alias, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_feathers, bevy_gizmos_render, bevy_internal, bevy_material, bevy_pbr, bevy_post_process, bevy_render, bevy_sprite_render, bevy_ui_render |
| bevy_shape | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_camera, bevy_egui, bevy_gizmos, bevy_gizmos_render, bevy_internal, bevy_light, bevy_mesh, bevy_pbr, bevy_picking, bevy_render, bevy_sprite_render |
| bevy_sprite | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_internal, bevy_sprite_render, bevy_ui, bevy_ui_render |
| bevy_sprite_render | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_gizmos_render, bevy_internal, bevy_ui_render |
| bevy_state | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_dev_tools, bevy_internal |
| bevy_state_macros | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_state |
| bevy_tasks | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_app, bevy_asset, bevy_diagnostic, bevy_ecs, bevy_gltf, bevy_internal, bevy_pbr, bevy_render, bevy_transform, bevy_winit |
| bevy_text | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_feathers, bevy_internal, bevy_sprite, bevy_sprite_render, bevy_ui, bevy_ui_render, bevy_ui_widgets |
| bevy_time | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_dev_tools, bevy_diagnostic, bevy_egui, bevy_extract, bevy_gilrs, bevy_gizmos, bevy_internal, bevy_picking, bevy_render, bevy_ui, bevy_ui_widgets |
| bevy_transform | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_audio, bevy_camera, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_gizmos, bevy_gizmos_render, bevy_gltf, bevy_internal, bevy_light, bevy_mesh, bevy_pbr, bevy_picking, bevy_render, bevy_sprite, bevy_sprite_render, bevy_ui, bevy_ui_render, bevy_world_serialization |
| bevy_ui | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_feathers, bevy_internal, bevy_ui_render, bevy_ui_widgets |
| bevy_ui_render | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_egui, bevy_feathers, bevy_internal |
| bevy_ui_widgets | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_feathers, bevy_internal |
| bevy_utils | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_anti_alias, bevy_app, bevy_asset, bevy_camera, bevy_core_pipeline, bevy_dev_tools, bevy_ecs, bevy_egui, bevy_extract, bevy_gizmos, bevy_gizmos_render, bevy_image, bevy_internal, bevy_light, bevy_log, bevy_material, bevy_pbr, bevy_post_process, bevy_reflect, bevy_render, bevy_scene, bevy_shader, bevy_sprite_render, bevy_state, bevy_text, bevy_transform, bevy_ui, bevy_ui_render, bevy_world_serialization |
| bevy_window | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_camera, bevy_core_pipeline, bevy_dev_tools, bevy_egui, bevy_feathers, bevy_gizmos, bevy_input_focus, bevy_internal, bevy_picking, bevy_render, bevy_sprite, bevy_ui, bevy_ui_widgets, bevy_winit |
| bevy_winit | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_egui, bevy_internal |
| bevy_world_serialization | 0.20.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_dev_tools, bevy_gltf, bevy_internal |
| bit-set | 0.10.0 | Apache-2.0 OR MIT | PERMISSIVE | naga, wgpu-core, wgpu-hal |
| bit-vec | 0.9.1 | Apache-2.0 OR MIT | PERMISSIVE | bit-set, wgpu-core |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | PERMISSIVE | alsa, android-activity, bevy_asset, bevy_core_pipeline, bevy_ecs, bevy_image, bevy_mesh, bevy_pbr, bevy_render, bevy_sprite_render, calloop, core-graphics, core-graphics-types, coreaudio-rs, dispatch2, egui, harfrust, inotify, ktx2, libredox, naga, ndk, nix, objc2-app-kit, objc2-audio-toolbox, objc2-avf-audio, objc2-cloud-kit, objc2-core-audio-types, objc2-core-data, objc2-core-foundation, objc2-core-graphics, objc2-foundation, objc2-io-kit, objc2-io-surface, objc2-metal, objc2-quartz-core, objc2-ui-kit, objc2-user-notifications, png, redox_syscall, rhai, ron, rustix, smithay-client-toolkit, spirv, wayland-client, wayland-csd-frame, wayland-protocols, wayland-protocols-experimental, wayland-protocols-misc, wayland-protocols-plasma, wayland-protocols-wlr, wgpu, wgpu-core, wgpu-hal, wgpu-types, winit, wit-bindgen, xkbcommon-dl |
| bitvec | 1.1.1 | MIT | PERMISSIVE | bevy_animation, bevy_pbr |
| blake3 | 1.8.7 | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | PERMISSIVE | bevy_animation, bevy_asset |
| block2 | 0.6.2 | MIT | PERMISSIVE | dispatch2, objc2-app-kit, objc2-audio-toolbox, objc2-avf-audio, objc2-cloud-kit, objc2-contacts, objc2-core-audio, objc2-core-data, objc2-core-foundation, objc2-core-graphics, objc2-core-image, objc2-core-location, objc2-foundation, objc2-io-kit, objc2-link-presentation, objc2-metal, objc2-quartz-core, objc2-ui-kit, objc2-uniform-type-identifiers, objc2-user-notifications, wgpu-hal, winit |
| blocking | 1.7.0 | Apache-2.0 OR MIT | PERMISSIVE | async-fs, async-io, bevy_asset |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | PERMISSIVE | bevy_ecs, hashbrown, wasm-bindgen-macro-support |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | bevy, bevy_color, bevy_egui, bevy_gizmos_render, bevy_image, bevy_mesh, bevy_pbr, bevy_platform, bevy_render, bevy_sprite_render, bevy_ui_render, bevy_winit, bitflags, color, ecolor, emath, epaint, euclid, font-types, glam, glifo, half, harfrust, image, jni, naga, ordered-float, parlance, peniko, read-fonts, skrifa, smithay-client-toolkit, tiny-skia, tiny-skia-path, uuid, vello_common, vello_cpu, wgpu, wgpu-core, wgpu-hal, wgpu-types, winit, xkeysym |
| bytemuck_derive | 1.12.1 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | bytemuck |
| byteorder | 1.5.0 | Unlicense OR MIT | PERMISSIVE | gltf, hash32, lewton, ogg, png |
| byteorder-lite | 0.1.0 | Unlicense OR MIT | PERMISSIVE | image |
| bytes | 1.12.1 | MIT | PERMISSIVE | combine, compact_str, heapless, http, ogg, ron, tokio |
| calloop | 0.14.5 | MIT | PERMISSIVE | calloop-wayland-source, smithay-client-toolkit, winit |
| calloop-wayland-source | 0.4.1 | MIT | PERMISSIVE | smithay-client-toolkit |
| castaway | 0.2.4 | MIT | PERMISSIVE | compact_str |
| cc | 1.10.0 | MIT OR Apache-2.0 | PERMISSIVE | android-activity, blake3, objc-sys, ring, tracing-oslog, wayland-backend |
| cesu8 | 1.1.0 | Apache-2.0/MIT | PERMISSIVE | jni, simd_cesu8 |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | PERMISSIVE | ahash, alsa, async-io, blake3, compact_str, console_error_panic_hook, const_soft_float, crc32fast, getrandom, half, jni, js-sys, libloading, naga, nix, parking_lot_core, polling, ring, thread_local, tiny-skia, tracing-oslog, wasm-bindgen, weak-table, web-task, wgpu, wgpu-hal |
| cfg_aliases | 0.2.2 | MIT | PERMISSIVE | naga, nix, wgpu, wgpu-core, wgpu-hal, winit |
| clap | 4.6.7 | MIT OR Apache-2.0 | PERMISSIVE | base64, cpal, lm-tools, png |
| clap_builder | 4.6.7 | MIT OR Apache-2.0 | PERMISSIVE | clap |
| clap_derive | 4.6.7 | MIT OR Apache-2.0 | PERMISSIVE | clap |
| clap_lex | 1.1.1 | MIT OR Apache-2.0 | PERMISSIVE | clap_builder |
| clipboard-win | 5.4.3 | BSL-1.0 | PERMISSIVE | arboard |
| cobs | 0.3.0 | MIT OR Apache-2.0 | PERMISSIVE | postcard |
| codespan-reporting | 0.13.1 | Apache-2.0 | PERMISSIVE | naga |
| color | 0.3.3 | Apache-2.0 OR MIT | PERMISSIVE | peniko |
| colorchoice | 1.0.5 | MIT OR Apache-2.0 | PERMISSIVE | anstream |
| combine | 4.6.9 | MIT | PERMISSIVE | jni |
| compact_str | 0.10.0 | MIT | PERMISSIVE | rhai |
| concurrent-queue | 2.5.0 | Apache-2.0 OR MIT | PERMISSIVE | async-channel, async-executor, async-io, bevy_ecs, bevy_tasks, polling, winit |
| console_error_panic_hook | 0.1.7 | Apache-2.0/MIT | PERMISSIVE | bevy_app, gilrs, winit |
| const-fnv1a-hash | 1.1.0 | MIT | PERMISSIVE | bevy_diagnostic |
| const-random | 0.1.18 | MIT OR Apache-2.0 | PERMISSIVE | ahash |
| const-random-macro | 0.1.16 | MIT OR Apache-2.0 | PERMISSIVE | const-random |
| const_panic | 0.2.17 | Zlib | PERMISSIVE | encase, stackfuture |
| const_soft_float | 0.1.4 | MIT OR Apache-2.0 | PERMISSIVE | constgebra |
| constant_time_eq | 0.4.2 | CC0-1.0 OR MIT-0 OR Apache-2.0 | PERMISSIVE | blake3 |
| constgebra | 0.1.4 | MIT OR Apache-2.0 | PERMISSIVE | hexasphere |
| convert_case | 0.10.0 | MIT | PERMISSIVE | derive_more-impl |
| cookie | 0.18.2 | MIT OR Apache-2.0 | PERMISSIVE | cookie_store |
| cookie_store | 0.22.1 | MIT OR Apache-2.0 | PERMISSIVE | ureq |
| core-foundation | 0.9.4 | MIT OR Apache-2.0 | PERMISSIVE | core-graphics, core-graphics-types, objc2, winit |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | PERMISSIVE | core-foundation |
| core-graphics | 0.23.2 | MIT OR Apache-2.0 | PERMISSIVE | winit |
| core-graphics-types | 0.1.3 | MIT OR Apache-2.0 | PERMISSIVE | core-graphics |
| coreaudio-rs | 0.14.2 | MIT/Apache-2.0 | PERMISSIVE | cpal |
| cpal | 0.17.3 | Apache-2.0 | PERMISSIVE | hound, rodio |
| cpufeatures | 0.3.1 | MIT OR Apache-2.0 | PERMISSIVE | blake3 |
| crc32fast | 1.5.3 | MIT OR Apache-2.0 | PERMISSIVE | flate2, image, png, tiff, zlib-rs |
| critical-section | 1.2.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_platform, event-listener, heapless, once_cell, portable-atomic |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_app, bevy_asset, bevy_egui, bevy_picking, bevy_time, rodio, vello_cpu, webbrowser |
| crossbeam-queue | 0.3.14 | MIT OR Apache-2.0 | PERMISSIVE | bevy_tasks |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | PERMISSIVE | concurrent-queue, crossbeam-channel, crossbeam-queue, portable-atomic |
| crunchy | 0.2.4 | MIT | PERMISSIVE | half, tiny-keccak |
| ctrlc | 3.5.2 | MIT/Apache-2.0 | PERMISSIVE | bevy_app |
| cursor-icon | 1.2.0 | MIT OR Apache-2.0 OR Zlib | PERMISSIVE | smithay-client-toolkit, wayland-csd-frame, winit |
| dasp_sample | 0.11.0 | MIT OR Apache-2.0 | PERMISSIVE | cpal, rodio |
| deranged | 0.5.9 | MIT OR Apache-2.0 | PERMISSIVE | time |
| derive_more | 2.1.1 | MIT | PERMISSIVE | bevy_animation, bevy_asset, bevy_camera, bevy_color, bevy_ecs, bevy_input, bevy_math, bevy_mesh, bevy_pbr, bevy_reflect, bevy_render, bevy_shape, bevy_sprite_render, bevy_tasks, bevy_transform, bevy_ui, bevy_ui_render, bevy_world_serialization, wesl, wgsl-parse |
| derive_more-impl | 2.1.1 | MIT | PERMISSIVE | derive_more |
| dispatch | 0.2.0 | MIT | PERMISSIVE | objc2-foundation |
| dispatch2 | 0.3.1 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | ctrlc, objc2-audio-toolbox, objc2-core-audio, objc2-core-foundation, objc2-core-graphics, objc2-io-kit, objc2-metal |
| displaydoc | 0.2.7 | MIT OR Apache-2.0 | PERMISSIVE | icu_collections, icu_locale_core, icu_properties, icu_provider, tinystr, zerotrie |
| disqualified | 1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_asset, bevy_reflect, bevy_utils |
| dlib | 0.5.3 | MIT | PERMISSIVE | wayland-sys, xkbcommon-dl |
| document-features | 0.2.12 | MIT OR Apache-2.0 | PERMISSIVE | cookie_store, ecolor, egui, emath, epaint, flate2, quick-xml, rhai, taffy, wgpu, wgpu-core |
| downcast-rs | 2.0.2 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_app, bevy_asset, bevy_camera, bevy_reflect, bevy_render, wayland-backend |
| dpi | 0.1.2 | Apache-2.0 AND MIT | PERMISSIVE | winit |
| ecolor | 0.36.2 | MIT OR Apache-2.0 | PERMISSIVE | epaint |
| egui | 0.36.2 | MIT OR Apache-2.0 | PERMISSIVE | bevy_egui, gpu-allocator |
| either | 1.19.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_asset, itertools, writeable |
| emath | 0.36.2 | MIT OR Apache-2.0 | PERMISSIVE | ecolor, egui, epaint |
| embedded-io | 0.6.1 | MIT OR Apache-2.0 | PERMISSIVE | heapless, postcard |
| encase | 0.12.1 | MIT-0 | PERMISSIVE | bevy_color, bevy_egui, bevy_material, bevy_mesh, bevy_render, glam |
| encase_derive | 0.12.1 | MIT-0 | PERMISSIVE | encase |
| encase_derive_impl | 0.12.1 | MIT-0 | PERMISSIVE | bevy_encase_derive, encase_derive |
| epaint | 0.36.2 | MIT OR Apache-2.0 | PERMISSIVE | egui |
| epaint_default_fonts | 0.36.2 | (MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0 | REVIEW | epaint |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | PERMISSIVE | hashbrown, indexmap |
| erased-serde | 0.4.11 | MIT OR Apache-2.0 | PERMISSIVE | bevy_reflect, icu_provider |
| errno | 0.3.14 | MIT OR Apache-2.0 | PERMISSIVE | rustix |
| error-code | 3.4.0 | BSL-1.0 | PERMISSIVE | clipboard-win |
| euclid | 0.22.14 | MIT OR Apache-2.0 | PERMISSIVE | guillotiere, kurbo |
| event-listener | 5.4.2 | Apache-2.0 OR MIT | PERMISSIVE | async-broadcast, async-lock, bevy, event-listener-strategy |
| event-listener-strategy | 0.5.4 | Apache-2.0 OR MIT | PERMISSIVE | async-broadcast, async-channel, async-lock |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | PERMISSIVE | async-executor, async-lock, concurrent-queue, futures-lite, indexmap, piper, polling, portable-atomic, ruzstd, zeno |
| fax | 0.2.7 | MIT | PERMISSIVE | tiff |
| fdeflate | 0.3.7 | MIT OR Apache-2.0 | PERMISSIVE | png |
| fearless_simd | 0.4.1 | Apache-2.0 OR MIT | PERMISSIVE | vello_common |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | PERMISSIVE | cc |
| fixedbitset | 0.5.7 | MIT OR Apache-2.0 | PERMISSIVE | bevy_ecs, bevy_gltf, bevy_pbr, bevy_sprite_render, petgraph |
| flate2 | 1.1.10 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_image, png, proc-macro2, rustix, syn, tiff, ureq |
| fnv | 1.0.7 | Apache-2.0 / MIT | PERMISSIVE | ahash, gilrs, hashbrown, indexmap, logos-codegen |
| foldhash | 0.2.0 | Zlib | PERMISSIVE | bevy_platform, bevy_reflect, glifo, hashbrown, serde_json, toml |
| font-types | 0.12.7 | MIT OR Apache-2.0 | PERMISSIVE | epaint, read-fonts |
| fontique | 0.11.1 | Apache-2.0 OR MIT | PERMISSIVE | parley |
| foreign-types | 0.5.0 | MIT/Apache-2.0 | PERMISSIVE | core-graphics |
| foreign-types-macros | 0.2.4 | MIT/Apache-2.0 | PERMISSIVE | foreign-types |
| foreign-types-shared | 0.3.1 | MIT/Apache-2.0 | PERMISSIVE | foreign-types |
| form_urlencoded | 1.2.2 | MIT OR Apache-2.0 | PERMISSIVE | url |
| funty | 2.0.0 | MIT | PERMISSIVE | bitvec |
| futures-channel | 0.3.34 | MIT OR Apache-2.0 | PERMISSIVE | bevy_platform, futures-util, js-sys, wasm-bindgen-futures, wayland-client, web-task, web-time |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | PERMISSIVE | async-broadcast, async-channel, calloop, combine, futures-channel, futures-lite, futures-util, js-sys, send_wrapper |
| futures-io | 0.3.34 | MIT OR Apache-2.0 | PERMISSIVE | async-io, bevy_asset, blocking, calloop, combine, futures-lite, futures-util, piper |
| futures-lite | 2.6.1 | Apache-2.0 OR MIT | PERMISSIVE | async-broadcast, async-channel, async-executor, async-fs, async-io, async-lock, async-task, bevy, bevy_asset, bevy_image, bevy_platform, bevy_tasks, blocking, event-listener, event-listener-strategy, js-sys, piper, wasm-bindgen-futures |
| futures-macro | 0.3.34 | MIT OR Apache-2.0 | PERMISSIVE | futures-util |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | PERMISSIVE | futures-util |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | PERMISSIVE | async-broadcast, bevy_asset, inotify, js-sys, send_wrapper, wayland-client, web-time |
| gethostname | 1.1.0 | Apache-2.0 | PERMISSIVE | x11rb |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | PERMISSIVE | ahash, async-io, bevy, bevy_egui, bevy_math, const-random-macro, fastrand, jobserver, kurbo, rand, rhai, ring, ureq, uuid, web-time, zerovec |
| gilrs | 0.11.2 | Apache-2.0/MIT | PERMISSIVE | bevy_gilrs |
| gilrs-core | 0.6.8 | Apache-2.0/MIT | PERMISSIVE | gilrs |
| gl_generator | 0.14.0 | Apache-2.0 | PERMISSIVE | glutin_wgl_sys |
| glam | 0.33.12 | MIT OR Apache-2.0 | PERMISSIVE | bevy_math, bevy_mesh, bevy_reflect, bevy_render, hexasphere, wgpu-hal |
| glifo | 0.2.0 | Apache-2.0 OR MIT | PERMISSIVE | vello_cpu |
| glob | 0.3.4 | MIT OR Apache-2.0 | PERMISSIVE | image, png, wesl |
| glow | 0.17.0 | MIT OR Apache-2.0 OR Zlib | PERMISSIVE | wgpu-hal |
| gltf | 1.4.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_gltf |
| gltf-derive | 1.4.1 | MIT OR Apache-2.0 | PERMISSIVE | gltf-json |
| gltf-json | 1.4.1 | MIT OR Apache-2.0 | PERMISSIVE | gltf |
| glutin_wgl_sys | 0.6.1 | Apache-2.0 | PERMISSIVE | wgpu-hal |
| gpu-allocator | 0.28.0 | MIT OR Apache-2.0 | PERMISSIVE | wgpu-hal |
| guillotiere | 0.7.0 | MIT/Apache-2.0 | PERMISSIVE | bevy_image, vello_common |
| half | 2.7.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_image, bevy_light, bevy_mesh, js-sys, naga, tiff, wesl, wgsl-types |
| harfrust | 0.12.0 | MIT | PERMISSIVE | epaint, parley |
| hash32 | 0.3.1 | MIT OR Apache-2.0 | PERMISSIVE | heapless |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_consumer, accesskit_ios, accesskit_macos, accesskit_windows, ahash, bevy_platform, bevy_reflect, foldhash, fontique, glifo, gpu-allocator, indexmap, naga, naga-types, no-std-compat, parley, petgraph, rhai, rustls, vello_cpu, weak-table, wgpu, wgpu-core, wgpu-hal, wgpu-types |
| heapless | 0.9.3 | MIT OR Apache-2.0 | PERMISSIVE | bevy_tasks, postcard |
| heck | 0.5.0 | MIT OR Apache-2.0 | PERMISSIVE | clap_derive |
| hermit-abi | 0.5.3 | MIT OR Apache-2.0 | PERMISSIVE | polling |
| hexasphere | 19.0.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_mesh |
| hound | 3.5.1 | Apache-2.0 | PERMISSIVE | cpal, rodio |
| http | 1.5.0 | MIT OR Apache-2.0 | PERMISSIVE | ureq-proto |
| httparse | 1.10.1 | MIT OR Apache-2.0 | PERMISSIVE | ureq-proto |
| icu_collections | 2.4.0 | Unicode-3.0 | PERMISSIVE | icu_normalizer, icu_properties, icu_segmenter |
| icu_locale_core | 2.4.0 | Unicode-3.0 | PERMISSIVE | icu_locale_fallback, icu_properties, icu_provider, icu_segmenter, zerotrie |
| icu_locale_fallback | 2.4.0 | Unicode-3.0 | PERMISSIVE | icu_segmenter |
| icu_locale_fallback_data | 2.4.0 | Unicode-3.0 | PERMISSIVE | icu_locale_fallback |
| icu_normalizer | 2.4.0 | Unicode-3.0 | PERMISSIVE | idna_adapter, parley |
| icu_normalizer_data | 2.4.0 | Unicode-3.0 | PERMISSIVE | icu_normalizer |
| icu_properties | 2.4.0 | Unicode-3.0 | PERMISSIVE | icu_normalizer, idna_adapter, parley, parley_data |
| icu_properties_data | 2.4.0 | Unicode-3.0 | PERMISSIVE | icu_properties |
| icu_provider | 2.4.0 | Unicode-3.0 | PERMISSIVE | icu_locale_fallback, icu_normalizer, icu_properties, icu_segmenter |
| icu_segmenter | 2.4.0 | Unicode-3.0 | PERMISSIVE | parley |
| icu_segmenter_data | 2.4.0 | Unicode-3.0 | PERMISSIVE | icu_segmenter |
| idna | 1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | cookie_store, url |
| idna_adapter | 1.2.2 | Apache-2.0 OR MIT | PERMISSIVE | idna |
| image | 0.25.10 | MIT OR Apache-2.0 | PERMISSIVE | arboard, bevy_egui, bevy_image, bevy_render, gltf, orbclient, smithay-client-toolkit, winit |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | PERMISSIVE | bevy, bevy_core_pipeline, bevy_ecs, bevy_pbr, bevy_reflect, bevy_reflect_derive, bevy_render, bevy_ui_render, bevy_utils, cookie_store, naga, naga-types, petgraph, ron, serde_json, sharded-slab, toml, toml_edit, wgpu-core |
| inflections | 1.1.1 | MIT | PERMISSIVE | gltf-derive |
| inotify | 0.11.5 | ISC | PERMISSIVE | async-io, gilrs-core |
| inotify-sys | 0.1.8 | ISC | PERMISSIVE | inotify |
| inventory | 0.3.25 | MIT OR Apache-2.0 | PERMISSIVE | bevy_reflect |
| is_terminal_polyfill | 1.70.2 | MIT OR Apache-2.0 | PERMISSIVE | anstream, winnow |
| itertools | 0.15.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_curve, bevy_egui, bevy_gltf, bevy_pbr, bevy_render, derive_more-impl, egui, icu_segmenter, indexmap, naga, petgraph, sysinfo, toml, wesl, wesl-macros, wgsl-parse, wgsl-types, zerocopy, zerotrie |
| itoa | 1.0.18 | MIT OR Apache-2.0 | PERMISSIVE | compact_str, http, rustix, serde_json |
| jni | 0.22.4 | MIT OR Apache-2.0 | PERMISSIVE | android-activity, cpal, ndk, webbrowser |
| jni-macros | 0.22.4 | MIT OR Apache-2.0 | PERMISSIVE | jni |
| jni-sys | 0.4.1 | MIT OR Apache-2.0 | PERMISSIVE | jni, jni-sys, ndk, ndk-sys |
| jni-sys-macros | 0.4.1 | MIT OR Apache-2.0 | PERMISSIVE | jni-sys |
| jobserver | 0.1.35 | MIT OR Apache-2.0 | PERMISSIVE | cc |
| js-sys | 0.3.106 | MIT OR Apache-2.0 | PERMISSIVE | bevy_asset, bevy_egui, bevy_platform, bevy_render, bevy_winit, cpal, getrandom, gilrs-core, glow, sys-locale, time, uuid, wasm-bindgen-futures, web-sys, web-task, web-time, wgpu, wgpu-hal, wgpu-types, winit |
| khronos-egl | 6.0.0 | MIT/Apache-2.0 | PERMISSIVE | wgpu-hal |
| khronos_api | 3.1.0 | Apache-2.0 | PERMISSIVE | gl_generator |
| ktx2 | 0.5.0 | Apache-2.0 | PERMISSIVE | bevy_image |
| kurbo | 0.13.1 | Apache-2.0 OR MIT | PERMISSIVE | peniko, skrifa |
| lalrpop-util | 0.23.2 | Apache-2.0 OR MIT | PERMISSIVE | wgsl-parse |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | PERMISSIVE | gltf, hashbrown, jni, sharded-slab |
| lewton | 0.10.2 | MIT OR Apache-2.0 | PERMISSIVE | rodio |
| lexical | 7.0.5 | MIT/Apache-2.0 | PERMISSIVE | wgsl-parse |
| lexical-core | 1.0.6 | MIT/Apache-2.0 | PERMISSIVE | lexical |
| lexical-parse-float | 1.0.6 | MIT/Apache-2.0 | PERMISSIVE | lexical-core |
| lexical-parse-integer | 1.0.6 | MIT/Apache-2.0 | PERMISSIVE | lexical-core, lexical-parse-float |
| lexical-util | 1.0.7 | MIT/Apache-2.0 | PERMISSIVE | lexical-core, lexical-parse-float, lexical-parse-integer, lexical-write-float, lexical-write-integer |
| lexical-write-float | 1.0.6 | MIT/Apache-2.0 | PERMISSIVE | lexical-core |
| lexical-write-integer | 1.0.6 | MIT/Apache-2.0 | PERMISSIVE | lexical-core, lexical-write-float |
| libc | 0.2.191 | MIT OR Apache-2.0 | PERMISSIVE | alsa, alsa-sys, android-activity, android_system_properties, async-fs, cc, core-foundation, core-graphics, core-graphics-types, coreaudio-rs, cpal, cpufeatures, dispatch2, displaydoc, errno, futures-util, getrandom, gilrs-core, hashbrown, inotify, inotify-sys, jobserver, khronos-egl, libloading, libredox, libudev-sys, linux-raw-sys, mach2, memmap2, ndk, nix, no-std-compat, objc2, objc2-app-kit, objc2-audio-toolbox, objc2-core-audio, objc2-core-foundation, objc2-core-graphics, objc2-foundation, objc2-io-kit, objc2-io-surface, objc2-quartz-core, orbclient, os_pipe, parking_lot_core, polling, portable-atomic, ring, rustix, smithay-client-toolkit, smithay-clipboard, sys-locale, sysinfo, time, tokio, wayland-sys, wgpu-hal, winit, wl-clipboard-rs, x11-dl, x11rb |
| libloading | 0.8.9 | ISC | PERMISSIVE | ash, dlib, jni, khronos-egl, wgpu-hal, x11rb |
| libm | 0.2.16 | MIT | PERMISSIVE | ab_glyph, ab_glyph_rasterizer, bevy_math, bevy_mikktspace, color, fearless_simd, glam, hexasphere, kurbo, naga, num-traits, polycool, rhai, tiny-skia-path, vello_common, zeno |
| libredox | 0.1.25 | MIT | PERMISSIVE | orbclient |
| libudev-sys | 0.1.4 | MIT | PERMISSIVE | gilrs-core |
| linebender_resource_handle | 0.1.1 | Apache-2.0 OR MIT | PERMISSIVE | fontique, parley, peniko |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | PERMISSIVE | rustix |
| litemap | 0.8.3 | Unicode-3.0 | PERMISSIVE | icu_locale_core, zerotrie |
| litrs | 1.0.1 | MIT OR Apache-2.0 | PERMISSIVE | document-features |
| lm-agents | 0.1.0 | UNKNOWN | REVIEW | lm-host, lm-host-model, lm-learn, lm-tools |
| lm-archive | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | None |
| lm-core | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | lm-agents, lm-archive, lm-host, lm-host-model, lm-learn, lm-proto, lm-render, lm-tools, lm-voice |
| lm-host | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | None |
| lm-host-model | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | lm-agents, lm-host |
| lm-learn | 0.1.0 | UNKNOWN | REVIEW | lm-agents, lm-host, lm-host-model, lm-tools |
| lm-proto | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | lm-archive |
| lm-render | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | lm-host, lm-host-model |
| lm-router | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | lm-learn |
| lm-tools | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | lm-archive, lm-host, lm-host-model |
| lm-voice | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | lm-host, lm-host-model, lm-tools |
| lm-vox | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | lm-tools |
| lock_api | 0.4.14 | MIT OR Apache-2.0 | PERMISSIVE | parking_lot, spin |
| log | 0.4.34 | MIT OR Apache-2.0 | PERMISSIVE | aho-corasick, android-activity, arboard, bevy_app, bevy_diagnostic, bevy_ecs, bevy_input, bevy_input_focus, bevy_state, bevy_time, bevy_window, calloop, calloop-wayland-source, cookie_store, egui, embedded-io, epaint, gilrs, gilrs-core, gl_generator, glifo, glow, gpu-allocator, icu_properties, icu_provider, jni, memchr, naga, ndk, offset-allocator, regex-automata, rustls, sctk-adwaita, smithay-client-toolkit, tiny-skia, tracing, tracing-log, tracing-subscriber, ureq, ureq-proto, vello_common, wayland-backend, wayland-client, wayland-sys, webbrowser, wgpu, wgpu-core, wgpu-hal, wgpu-types, wl-clipboard-rs, xkbcommon-dl, zune-core |
| logos | 0.16.1 | MIT OR Apache-2.0 | PERMISSIVE | wgsl-parse |
| logos-codegen | 0.16.1 | MIT OR Apache-2.0 | PERMISSIVE | logos-derive |
| logos-derive | 0.16.1 | MIT OR Apache-2.0 | PERMISSIVE | logos |
| mach2 | 0.5.0 | BSD-2-Clause OR MIT OR Apache-2.0 | PERMISSIVE | cpal |
| matchers | 0.2.0 | MIT | PERMISSIVE | tracing-subscriber |
| memchr | 2.8.3 | Unlicense OR MIT | PERMISSIVE | aho-corasick, annotate-snippets, combine, futures-lite, futures-util, nom, quick-xml, regex, regex-automata, serde_json, sysinfo, tree_magic_mini, winnow |
| memmap2 | 0.9.12 | MIT OR Apache-2.0 | PERMISSIVE | blake3, fontique, sctk-adwaita, smithay-client-toolkit, winit |
| mime | 0.3.17 | MIT OR Apache-2.0 | PERMISSIVE | mime_guess |
| mime_guess | 2.0.5 | MIT | PERMISSIVE | ureq |
| miniz_oxide | 0.9.1 | MIT OR Zlib OR Apache-2.0 | PERMISSIVE | fdeflate, flate2, png |
| moxcms | 0.8.1 | BSD-3-Clause OR Apache-2.0 | PERMISSIVE | image |
| mutants | 0.0.3 | MIT | PERMISSIVE | unsynn |
| naga | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | wgpu, wgpu-core, wgpu-hal, wgpu-naga-bridge |
| naga-types | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | naga, wgpu-core, wgpu-hal, wgpu-types |
| ndk | 0.9.0 | MIT OR Apache-2.0 | PERMISSIVE | android-activity, cpal, winit |
| ndk-context | 0.1.1 | MIT OR Apache-2.0 | PERMISSIVE | android-activity, cpal, webbrowser |
| ndk-sys | 0.6.0+11769913 | MIT OR Apache-2.0 | PERMISSIVE | android-activity, ndk, wgpu-hal |
| nix | 0.31.3 | MIT | PERMISSIVE | calloop, ctrlc, gilrs-core, jobserver, tokio |
| no-std-compat | 0.4.1 | MIT | PERMISSIVE | rhai |
| nohash-hasher | 0.2.0 | Apache-2.0 OR MIT | PERMISSIVE | egui, epaint |
| nom | 8.0.0 | MIT | PERMISSIVE | tree_magic_mini |
| nonmax | 0.5.5 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_core_pipeline, bevy_ecs, bevy_pbr, bevy_render, bevy_sprite_render, offset-allocator |
| ntapi | 0.4.3 | Apache-2.0 OR MIT | PERMISSIVE | sysinfo |
| nu-ansi-term | 0.50.3 | MIT | PERMISSIVE | tracing-subscriber |
| num-bigint | 0.4.8 | MIT OR Apache-2.0 | PERMISSIVE | num-rational, rustls, zmij |
| num-conv | 0.2.2 | MIT OR Apache-2.0 | PERMISSIVE | time, time-macros |
| num-derive | 0.4.2 | MIT OR Apache-2.0 | PERMISSIVE | cpal |
| num-integer | 0.1.47 | MIT OR Apache-2.0 | PERMISSIVE | num-bigint, num-rational, zmij |
| num-rational | 0.4.2 | MIT OR Apache-2.0 | PERMISSIVE | rodio |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | PERMISSIVE | approx, cpal, deranged, euclid, half, image, moxcms, naga, num-bigint, num-derive, num-integer, num-rational, ordered-float, rand_distr, rhai, wesl, wgsl-types |
| num_enum | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | PERMISSIVE | android-activity, ndk |
| num_enum_derive | 0.7.6 | BSD-3-Clause OR MIT OR Apache-2.0 | PERMISSIVE | num_enum |
| objc-sys | 0.3.5 | MIT | PERMISSIVE | objc2 |
| objc2 | 0.6.5 | MIT | PERMISSIVE | accesskit_ios, accesskit_macos, arboard, block2, cpal, dispatch2, fontique, gpu-allocator, objc2-app-kit, objc2-audio-toolbox, objc2-avf-audio, objc2-cloud-kit, objc2-contacts, objc2-core-audio, objc2-core-audio-types, objc2-core-data, objc2-core-foundation, objc2-core-graphics, objc2-core-image, objc2-core-location, objc2-foundation, objc2-io-kit, objc2-io-surface, objc2-link-presentation, objc2-metal, objc2-quartz-core, objc2-symbols, objc2-ui-kit, objc2-uniform-type-identifiers, objc2-user-notifications, raw-window-metal, webbrowser, wgpu-hal, winit |
| objc2-app-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | accesskit_macos, arboard, objc2-link-presentation, raw-window-metal, webbrowser, winit |
| objc2-audio-toolbox | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | coreaudio-rs, cpal, objc2-avf-audio |
| objc2-avf-audio | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | coreaudio-rs, cpal |
| objc2-cloud-kit | 0.2.2 | MIT | PERMISSIVE | objc2-app-kit, objc2-ui-kit |
| objc2-contacts | 0.2.2 | MIT | PERMISSIVE | objc2-core-location |
| objc2-core-audio | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | coreaudio-rs, cpal, objc2-audio-toolbox |
| objc2-core-audio-types | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | coreaudio-rs, cpal, objc2-audio-toolbox, objc2-avf-audio, objc2-core-audio |
| objc2-core-data | 0.2.2 | MIT | PERMISSIVE | objc2-app-kit, objc2-ui-kit |
| objc2-core-foundation | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | arboard, coreaudio-rs, cpal, fontique, gilrs-core, harfrust, objc2-app-kit, objc2-audio-toolbox, objc2-core-audio, objc2-core-graphics, objc2-foundation, objc2-io-kit, objc2-io-surface, objc2-metal, objc2-quartz-core, raw-window-metal, sysinfo, wgpu-hal |
| objc2-core-graphics | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | arboard, objc2-app-kit, objc2-quartz-core, wgpu-hal |
| objc2-core-image | 0.2.2 | MIT | PERMISSIVE | objc2-app-kit, objc2-ui-kit |
| objc2-core-location | 0.2.2 | MIT | PERMISSIVE | objc2-cloud-kit, objc2-ui-kit, objc2-user-notifications |
| objc2-encode | 4.1.0 | MIT | PERMISSIVE | objc2 |
| objc2-foundation | 0.3.2 | MIT | PERMISSIVE | accesskit_ios, accesskit_macos, arboard, cpal, fontique, gpu-allocator, objc2-app-kit, objc2-audio-toolbox, objc2-avf-audio, objc2-cloud-kit, objc2-contacts, objc2-core-audio, objc2-core-data, objc2-core-image, objc2-core-location, objc2-io-surface, objc2-link-presentation, objc2-metal, objc2-quartz-core, objc2-symbols, objc2-ui-kit, objc2-uniform-type-identifiers, objc2-user-notifications, raw-window-metal, webbrowser, wgpu-hal, winit |
| objc2-io-kit | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | gilrs-core, sysinfo |
| objc2-io-surface | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | objc2-core-graphics, objc2-metal |
| objc2-link-presentation | 0.2.2 | MIT | PERMISSIVE | objc2-ui-kit |
| objc2-metal | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | gpu-allocator, objc2-core-graphics, objc2-core-image, objc2-quartz-core, wgpu-hal |
| objc2-quartz-core | 0.3.2 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | objc2-app-kit, objc2-ui-kit, raw-window-metal, wgpu-hal |
| objc2-symbols | 0.2.2 | MIT | PERMISSIVE | objc2-ui-kit |
| objc2-ui-kit | 0.2.2 | MIT | PERMISSIVE | accesskit_ios, raw-window-metal, winit |
| objc2-uniform-type-identifiers | 0.2.2 | MIT | PERMISSIVE | objc2-app-kit, objc2-ui-kit |
| objc2-user-notifications | 0.2.2 | MIT | PERMISSIVE | objc2-ui-kit |
| offset-allocator | 0.2.0 | MIT | PERMISSIVE | bevy_pbr, bevy_render |
| ogg | 0.8.0 | BSD-3-Clause | PERMISSIVE | lewton |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_windows, ahash, async-executor, async-task, combine, const-random-macro, naga, read-fonts, rhai, rustix, rustls, rustls-webpki, self_cell, tracing-core, tracing-log, tracing-subscriber, wasm-bindgen, wayland-sys, wgpu-core, wgpu-hal, wyz, x11-dl, x11rb, xkbcommon-dl |
| once_cell_polyfill | 1.70.2 | MIT OR Apache-2.0 | PERMISSIVE | anstyle-wincon |
| orbclient | 0.3.55 | MIT | PERMISSIVE | winit |
| ordered-float | 5.5.1 | MIT | PERMISSIVE | wgpu-hal |
| os_pipe | 1.2.3 | MIT | PERMISSIVE | wl-clipboard-rs |
| owned_ttf_parser | 0.25.1 | Apache-2.0 | PERMISSIVE | ab_glyph |
| parking | 2.2.1 | Apache-2.0 OR MIT | PERMISSIVE | async-io, event-listener, futures-lite |
| parking_lot | 0.12.5 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_windows, arboard, epaint, nix, tokio, tracing-subscriber, wgpu, wgpu-core, wgpu-hal |
| parking_lot_core | 0.9.12 | MIT OR Apache-2.0 | PERMISSIVE | once_cell, parking_lot |
| parlance | 0.1.1 | Apache-2.0 OR MIT | PERMISSIVE | fontique, parley |
| parley | 0.11.1 | Apache-2.0 OR MIT | PERMISSIVE | bevy_text, bevy_ui, bevy_ui_widgets |
| parley_data | 0.11.1 | Apache-2.0 OR MIT | PERMISSIVE | parley |
| peniko | 0.6.1 | Apache-2.0 OR MIT | PERMISSIVE | glifo, parley, vello_common |
| percent-encoding | 2.3.2 | MIT OR Apache-2.0 | PERMISSIVE | arboard, bevy_gltf, cookie, form_urlencoded, ureq, url, webpki-roots, winit |
| petgraph | 0.8.3 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_reflect, naga, parking_lot_core, tree_magic_mini |
| pin-project | 1.1.13 | Apache-2.0 OR MIT | PERMISSIVE | winit |
| pin-project-internal | 1.1.13 | Apache-2.0 OR MIT | PERMISSIVE | pin-project |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | PERMISSIVE | async-broadcast, async-channel, async-executor, async-lock, async-task, combine, event-listener, event-listener-strategy, futures-lite, futures-util, polling, tokio, tracing |
| piper | 0.2.5 | MIT OR Apache-2.0 | PERMISSIVE | blocking |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | PERMISSIVE | alsa-sys, khronos-egl, libudev-sys, smithay-client-toolkit, wayland-sys, x11-dl |
| plain | 0.2.3 | MIT/Apache-2.0 | PERMISSIVE | libredox |
| png | 0.18.1 | MIT OR Apache-2.0 | PERMISSIVE | glifo, image, tiny-skia, vello_common, vello_cpu |
| polling | 3.11.0 | Apache-2.0 OR MIT | PERMISSIVE | async-io, calloop, x11rb |
| polycool | 0.4.0 | MIT OR Apache-2.0 | PERMISSIVE | kurbo |
| portable-atomic | 1.15.0 | Apache-2.0 OR MIT | PERMISSIVE | ahash, async-channel, async-task, atomic-waker, atomicow, bevy_platform, bytes, concurrent-queue, event-listener, futures-core, futures-util, heapless, once_cell, piper, portable-atomic-util, spin, wgpu, wgpu-core, wgpu-hal |
| portable-atomic-util | 0.2.8 | Apache-2.0 OR MIT | PERMISSIVE | async-channel, atomicow, bevy_platform, event-listener, futures-util, piper, wgpu-hal |
| postcard | 1.1.3 | MIT OR Apache-2.0 | PERMISSIVE | bevy_reflect, bevy_world_serialization, icu_collections, icu_locale_core, icu_provider, litemap, lm-core, lm-proto, rand, tinystr, typewit, yoke, zerotrie, zerovec |
| potential_utf | 0.1.6 | Unicode-3.0 | PERMISSIVE | icu_collections, icu_locale_core, icu_locale_fallback, icu_segmenter |
| powerfmt | 0.2.1 | MIT OR Apache-2.0 | PERMISSIVE | deranged, time |
| presser | 0.3.1 | MIT OR Apache-2.0 | PERMISSIVE | gpu-allocator |
| proc-macro-crate | 3.5.0 | MIT OR Apache-2.0 | PERMISSIVE | num_enum_derive |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | PERMISSIVE | assert_type_match, bevy_asset_macros, bevy_ecs_macro_logic, bevy_ecs_macros, bevy_extract_macros, bevy_macro_utils, bevy_reflect_derive, bevy_render_macros, bevy_scene_macros, bytemuck_derive, clap_derive, derive_more-impl, displaydoc, encase_derive_impl, foreign-types-macros, futures-macro, gltf-derive, jni-macros, litrs, log, logos-codegen, num-derive, num_enum_derive, pin-project-internal, proc-macro-crate, quote, rhai_codegen, serde_derive, syn, synstructure, thiserror-impl, tracing-attributes, unsynn, wasm-bindgen-macro-support, wayland-scanner, wesl, wesl-macros, windows-implement, windows-interface, yoke-derive, zerocopy-derive, zerofrom-derive, zerovec-derive |
| profiling | 1.0.18 | MIT OR Apache-2.0 | PERMISSIVE | bevy_render, egui, epaint, wgpu, wgpu-core, wgpu-hal |
| pxfm | 0.1.30 | BSD-3-Clause OR Apache-2.0 | PERMISSIVE | moxcms |
| quick-error | 2.0.1 | MIT/Apache-2.0 | PERMISSIVE | combine, tiff |
| quick-xml | 0.41.0 | MIT | PERMISSIVE | wayland-scanner |
| quote | 1.0.47 | MIT OR Apache-2.0 | PERMISSIVE | assert_type_match, bevy_animation_macros, bevy_asset_macros, bevy_derive, bevy_ecs_macro_logic, bevy_ecs_macros, bevy_extract_macros, bevy_gizmos_macros, bevy_macro_utils, bevy_material_macros, bevy_reflect_derive, bevy_render_macros, bevy_scene_macros, bevy_state_macros, bytemuck_derive, clap_derive, derive_more-impl, displaydoc, encase_derive_impl, foreign-types-macros, futures-macro, gltf-derive, jni-macros, jni-sys-macros, logos-codegen, num-derive, num_enum_derive, pin-project-internal, proc-macro-crate, proc-macro2, rhai_codegen, serde_derive, syn, synstructure, thiserror-impl, tracing-attributes, variadics_please, wasm-bindgen-macro, wasm-bindgen-macro-support, wayland-scanner, wesl, wesl-macros, windows-implement, windows-interface, yoke-derive, zerocopy-derive, zerofrom-derive, zerovec-derive |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | PERMISSIVE | getrandom |
| radium | 0.7.0 | MIT | PERMISSIVE | bitvec |
| radsort | 0.1.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_sprite |
| rand | 0.10.3 | MIT OR Apache-2.0 | PERMISSIVE | ahash, base64, bevy, bevy_ecs, bevy_math, bevy_shape, bit-set, bit-vec, bitvec, blake3, bumpalo, byteorder, byteorder-lite, const_panic, cookie, crc32fast, crossbeam-channel, crossbeam-queue, crossbeam-utils, deranged, encase, fastrand, fdeflate, flate2, foldhash, glam, half, hashbrown, http, httparse, inotify, itertools, kurbo, litemap, moxcms, nix, num-bigint, ogg, ordered-float, parking_lot, petgraph, png, rand_distr, read-fonts, rodio, rustc-hash, ruzstd, simd-adler32, smartstring, subtle, time, tinystr, tokio, twox-hash, unicode-ident, uuid, web-time, webbrowser, writeable, zerocopy, zerotrie, zerovec, zmij |
| rand_core | 0.10.1 | MIT OR Apache-2.0 | PERMISSIVE | getrandom, rand |
| rand_distr | 0.6.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_shape, half, rodio, zerovec |
| range-alloc | 0.1.5 | MIT OR Apache-2.0 | PERMISSIVE | bevy_pbr, wgpu-hal |
| raw-window-handle | 0.6.2 | MIT OR Apache-2.0 OR Zlib | PERMISSIVE | accesskit_winit, bevy_window, ndk, orbclient, raw-window-metal, smithay-client-toolkit, wayland-backend, wgpu, wgpu-core, wgpu-hal, wgpu-types, winit, x11rb |
| raw-window-metal | 1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | wgpu-hal |
| read-fonts | 0.41.0 | MIT OR Apache-2.0 | PERMISSIVE | fontique, harfrust, skrifa |
| rectangle-pack | 0.4.2 | MIT/Apache-2.0 | PERMISSIVE | bevy_image |
| redox_syscall | 0.9.5 | MIT | PERMISSIVE | libredox, parking_lot_core, winit |
| regex | 1.13.1 | MIT OR Apache-2.0 | PERMISSIVE | combine, lm-learn, once_cell, quick-xml, tracing-subscriber, zerocopy |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | PERMISSIVE | lalrpop-util, logos-codegen, matchers, regex, tracing-subscriber |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | PERMISSIVE | logos-codegen, regex, regex-automata |
| renderdoc-sys | 1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | wgpu-hal |
| rhai | 1.27.0 | MIT OR Apache-2.0 | PERMISSIVE | lm-core |
| rhai_codegen | 3.3.0 | MIT OR Apache-2.0 | PERMISSIVE | rhai |
| ring | 0.17.14 | Apache-2.0 AND ISC | PERMISSIVE | rustls, rustls-webpki |
| rodio | 0.22.2 | MIT OR Apache-2.0 | PERMISSIVE | bevy_audio |
| ron | 0.12.3 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_animation, bevy_asset, bevy_dev_tools, bevy_reflect, bevy_transform, bevy_world_serialization, cookie_store, egui, naga, wgpu-core |
| rustc-hash | 2.1.3 | Apache-2.0 OR MIT | PERMISSIVE | naga, naga-types, unsynn, wgpu-core, winnow |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 | PERMISSIVE | derive_more, derive_more-impl, jni-macros, simd_cesu8 |
| rustix | 1.1.6 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | PERMISSIVE | async-io, calloop, calloop-wayland-source, gethostname, polling, smithay-client-toolkit, wayland-backend, wayland-client, wayland-cursor, winit, wl-clipboard-rs, x11rb |
| rustls | 0.23.46 | Apache-2.0 OR ISC OR MIT | PERMISSIVE | ureq, webpki-roots |
| rustls-pki-types | 1.15.1 | MIT OR Apache-2.0 | PERMISSIVE | rustls, rustls-webpki, ureq, webpki-roots |
| rustls-webpki | 0.103.16 | ISC | PERMISSIVE | rustls, webpki-roots |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | PERMISSIVE | bitflags, bytemuck, castaway, clap, crossbeam-utils, derive_more, displaydoc, erased-serde, inventory, num_enum, pin-project, pin-project-lite, portable-atomic, proc-macro2, quote, rustls, self_cell, serde_json, syn, thiserror, tracing-attributes, uuid, wasm-bindgen, zerocopy, zerocopy-derive |
| ruzstd | 0.9.1 | MIT | PERMISSIVE | bevy_image |
| same-file | 1.0.6 | Unlicense/MIT | PERMISSIVE | walkdir |
| scoped-tls | 1.0.1 | MIT/Apache-2.0 | PERMISSIVE | wayland-backend |
| scopeguard | 1.2.0 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_windows, lock_api |
| sctk-adwaita | 0.10.1 | MIT | PERMISSIVE | winit |
| self_cell | 1.3.0 | Apache-2.0 OR GPL-2.0-only | PERMISSIVE | epaint |
| semver | 1.0.28 | MIT OR Apache-2.0 | PERMISSIVE | clap, nix, rustc_version |
| send_wrapper | 0.6.0 | MIT/Apache-2.0 | PERMISSIVE | bevy_render |
| serde | 1.0.229 | MIT OR Apache-2.0 | PERMISSIVE | accesskit, ahash, allocator-api2, arrayvec, bevy, bevy_a11y, bevy_animation, bevy_asset, bevy_camera, bevy_color, bevy_curve, bevy_dev_tools, bevy_diagnostic, bevy_ecs, bevy_gltf, bevy_image, bevy_input, bevy_math, bevy_mesh, bevy_platform, bevy_reflect, bevy_shader, bevy_shape, bevy_text, bevy_time, bevy_transform, bevy_ui, bevy_utils, bevy_window, bevy_world_serialization, bit-set, bit-vec, bitflags, bitvec, blake3, bumpalo, bytes, cobs, codespan-reporting, color, compact_str, cookie_store, cursor-icon, dpi, ecolor, egui, either, emath, epaint, erased-serde, euclid, fixedbitset, font-types, gilrs, gilrs-core, gltf-json, guillotiere, half, hashbrown, http, icu_collections, icu_locale_core, icu_locale_fallback, icu_normalizer, icu_properties, icu_provider, icu_segmenter, image, indexmap, kurbo, linebender_resource_handle, lm-agents, lm-archive, lm-core, lm-learn, lm-proto, lm-render, lm-router, lm-tools, lm-voice, lock_api, log, miniz_oxide, naga, naga-types, nonmax, nu-ansi-term, num-bigint, num-rational, ordered-float, peniko, petgraph, portable-atomic, portable-atomic-util, postcard, quick-xml, rand, rand_distr, read-fonts, rhai, ron, rustls, rustls-webpki, semver, serde_core, serde_derive, serde_json, serde_spanned, skrifa, slab, slotmap, smallvec, smartstring, smol_str, spirv, sysinfo, taffy, thin-vec, time, toml, toml_edit, tracing-subscriber, twox-hash, typewit, ureq, url, uuid, vec_map, wasm-bindgen, web-time, wesl, wgpu-core, wgpu-types, wgsl-parse, wgsl-types, winit, x11rb-protocol, xkeysym, yoke, zeroize, zerotrie, zerovec, zerovec-derive, zune-core |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | PERMISSIVE | bitflags, deranged, erased-serde, glam, hashbrown, heapless, indexmap, litemap, log, potential_utf, semver, serde, serde_json, serde_spanned, time, tinystr, tinyvec, toml, toml_datetime, toml_edit, uuid, zerotrie |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | PERMISSIVE | bitflags, cookie_store, erased-serde, gltf-json, petgraph, quick-xml, ron, serde, serde_core, serde_json, serde_spanned, slotmap, url, uuid, wasm-bindgen |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | PERMISSIVE | accesskit, ahash, bevy, bevy_gltf, bevy_image, bevy_mesh, bevy_reflect, bit-set, bit-vec, bitflags, bitvec, blake3, bumpalo, compact_str, cookie_store, deranged, either, erased-serde, fixedbitset, font-types, glam, gltf, gltf-json, http, icu_collections, icu_locale_core, icu_locale_fallback, icu_provider, icu_segmenter, idna, litemap, lm-agents, lm-archive, lm-host-model, lm-learn, lm-router, lm-tools, lm-voice, lm-vox, log, nu-ansi-term, potential_utf, rand, read-fonts, rhai, ron, rustls, rustls-webpki, skrifa, slotmap, smol_str, sysinfo, taffy, time, tinystr, toml, toml_edit, tracing-subscriber, twox-hash, typewit, ureq, url, uuid, wasm-bindgen, web-time, wgpu-types, zerotrie, zerovec, zerovec-derive |
| serde_spanned | 1.1.2 | MIT OR Apache-2.0 | PERMISSIVE | toml, toml_edit |
| sharded-slab | 0.1.7 | MIT | PERMISSIVE | tracing-subscriber |
| shlex | 2.0.1 | MIT OR Apache-2.0 | PERMISSIVE | cc, clap |
| simd-adler32 | 0.3.10 | MIT | PERMISSIVE | fdeflate, miniz_oxide |
| simd_cesu8 | 1.2.0 | Apache-2.0 OR MIT | PERMISSIVE | android-activity, jni, jni-macros |
| simdutf8 | 0.1.5 | MIT OR Apache-2.0 | PERMISSIVE | simd_cesu8 |
| skrifa | 0.44.0 | MIT OR Apache-2.0 | PERMISSIVE | epaint, glifo, parley, swash |
| slab | 0.4.12 | MIT | PERMISSIVE | async-executor, async-io, calloop, futures-util, sharded-slab, tokio |
| slotmap | 1.1.1 | Zlib | PERMISSIVE | bevy_ecs, glow, taffy |
| smallvec | 1.16.4 | MIT OR Apache-2.0 | PERMISSIVE | ahash, bevy_animation, bevy_camera, bevy_ecs, bevy_gltf, bevy_light, bevy_material, bevy_pbr, bevy_post_process, bevy_reflect, bevy_render, bevy_scene, bevy_sprite_render, bevy_text, bevy_ui, bevy_ui_render, compact_str, egui, epaint, fontique, glifo, harfrust, icu_normalizer, icu_segmenter, idna, kurbo, parking_lot_core, peniko, rhai, taffy, tinyvec, tracing-subscriber, vello_common, wayland-backend, wgpu, wgpu-core, wgpu-hal |
| smartstring | 1.0.1 | MPL-2.0+ | REVIEW | rhai |
| smithay-client-toolkit | 0.20.0 | MIT | PERMISSIVE | sctk-adwaita, smithay-clipboard, winit |
| smithay-clipboard | 0.7.3 | MIT | PERMISSIVE | bevy_egui |
| smol_str | 0.2.2 | MIT OR Apache-2.0 | PERMISSIVE | bevy_feathers, bevy_input, bevy_reflect, bevy_text, bevy_ui_widgets, winit |
| spin | 0.12.3 | MIT | PERMISSIVE | bevy_platform, futures-util, lazy_static, no-std-compat |
| spirv | 0.4.0+sdk-1.4.341.0 | Apache-2.0 | PERMISSIVE | naga |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | PERMISSIVE | heapless, icu_provider, memmap2, yoke |
| stackfuture | 0.3.1 | MIT | PERMISSIVE | bevy_asset |
| static_assertions | 1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_windows, bevy_ecs, bevy_pbr, bevy_reflect, bevy_render, bevy_sprite_render, bevy_utils, bitvec, clap_builder, compact_str, derive_more, dispatch2, displaydoc, funty, heapless, jni, libloading, linux-raw-sys, objc2, objc2-core-foundation, objc2-foundation, pin-project, pin-project-lite, portable-atomic, radium, raw-window-handle, rustix, smartstring, variadics_please, web-time, wgpu, wgpu-hal, wgpu-types, zerocopy, zerocopy-derive |
| strict-num | 0.1.1 | MIT | PERMISSIVE | tiny-skia-path |
| strsim | 0.11.1 | MIT | PERMISSIVE | clap_builder |
| subtle | 2.6.1 | BSD-3-Clause | PERMISSIVE | cookie, rustls |
| swash | 0.2.10 | Apache-2.0 OR MIT | PERMISSIVE | bevy_text, bevy_ui |
| syn | 3.0.9 | MIT OR Apache-2.0 | PERMISSIVE | assert_type_match, bevy_animation_macros, bevy_asset_macros, bevy_derive, bevy_ecs_macro_logic, bevy_ecs_macros, bevy_extract_macros, bevy_gizmos_macros, bevy_macro_utils, bevy_material_macros, bevy_reflect_derive, bevy_render_macros, bevy_scene_macros, bevy_state_macros, bytemuck_derive, clap_derive, derive_more-impl, displaydoc, encase_derive_impl, foreign-types-macros, futures-macro, gltf-derive, jni-macros, jni-sys-macros, logos-codegen, num-derive, num_enum_derive, pin-project-internal, proc-macro-crate, rhai_codegen, serde_derive, synstructure, thiserror-impl, tracing-attributes, wasm-bindgen-macro-support, wesl-macros, windows-implement, windows-interface, yoke-derive, zerocopy-derive, zerofrom-derive, zerovec-derive |
| synstructure | 0.14.0 | MIT | PERMISSIVE | yoke-derive, zerofrom-derive |
| sys-locale | 0.3.2 | MIT OR Apache-2.0 | PERMISSIVE | bevy_text |
| sysinfo | 0.39.6 | MIT | PERMISSIVE | bevy_diagnostic |
| taffy | 0.14.0 | MIT | PERMISSIVE | bevy_ui |
| tap | 1.0.1 | MIT | PERMISSIVE | bitvec, wyz |
| thin-vec | 0.2.21 | MIT OR Apache-2.0 | PERMISSIVE | rhai |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | PERMISSIVE | android-activity, bevy, bevy_animation, bevy_app, bevy_asset, bevy_camera, bevy_color, bevy_curve, bevy_dev_tools, bevy_ecs, bevy_gilrs, bevy_gltf, bevy_image, bevy_input, bevy_input_focus, bevy_material, bevy_math, bevy_mesh, bevy_pbr, bevy_post_process, bevy_reflect, bevy_render, bevy_scene, bevy_shader, bevy_shape, bevy_text, bevy_transform, bevy_ui, bevy_world_serialization, calloop, cobs, displaydoc, encase, gpu-allocator, jni, jni-macros, jni-sys, lm-proto, lm-router, naga, naga-types, ndk, rodio, smithay-client-toolkit, vello_common, wesl, wgpu-core, wgpu-hal, wgsl-parse, wl-clipboard-rs |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | PERMISSIVE | thiserror |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | PERMISSIVE | bevy_animation, bevy_egui, bevy_utils, tracing-subscriber, vello_cpu |
| tiff | 0.11.3 | MIT | PERMISSIVE | fax, image |
| time | 0.3.55 | MIT OR Apache-2.0 | PERMISSIVE | cookie, cookie_store, rustls, tracing-subscriber |
| time-core | 0.1.9 | MIT OR Apache-2.0 | PERMISSIVE | time, time-macros |
| time-macros | 0.2.32 | MIT OR Apache-2.0 | PERMISSIVE | time |
| tiny-keccak | 2.0.2 | CC0-1.0 | PERMISSIVE | const-random-macro |
| tiny-skia | 0.11.4 | BSD-3-Clause | PERMISSIVE | sctk-adwaita |
| tiny-skia-path | 0.11.4 | BSD-3-Clause | PERMISSIVE | tiny-skia, ttf-parser |
| tinystr | 0.8.5 | Unicode-3.0 | PERMISSIVE | icu_locale_core, icu_locale_fallback |
| tinyvec | 1.13.3 | Zlib OR Apache-2.0 OR MIT | PERMISSIVE | hexasphere, lewton |
| tokio | 1.54.1 | MIT | PERMISSIVE | combine, embedded-io, futures-util, inotify, quick-xml, tracing-subscriber, wasm-bindgen, wasm-bindgen-futures, webbrowser, webpki-roots, weezl, wesl |
| toml | 1.1.8+spec-1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | icu_collections, lm-agents, lm-archive, lm-render, lm-router, lm-tools, lm-voice, toml_writer, wesl |
| toml_datetime | 1.1.2+spec-1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | toml, toml_edit |
| toml_edit | 0.25.17+spec-1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_macro_utils, proc-macro-crate |
| toml_parser | 1.1.5+spec-1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | toml, toml_edit |
| toml_writer | 1.1.3+spec-1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | toml, toml_edit |
| tracing | 0.1.44 | MIT | PERMISSIVE | async-io, bevy, bevy_animation, bevy_anti_alias, bevy_app, bevy_asset, bevy_audio, bevy_dev_tools, bevy_ecs, bevy_gilrs, bevy_gizmos_render, bevy_gltf, bevy_image, bevy_light, bevy_log, bevy_material, bevy_mesh, bevy_pbr, bevy_picking, bevy_post_process, bevy_render, bevy_scene, bevy_shader, bevy_sprite, bevy_sprite_render, bevy_text, bevy_transform, bevy_ui, bevy_ui_render, bevy_winit, blocking, calloop, polling, profiling, rodio, tokio, tracing-attributes, tracing-log, tracing-subscriber, tracing-wasm, winit, x11rb |
| tracing-attributes | 0.1.31 | MIT | PERMISSIVE | tracing |
| tracing-core | 0.1.36 | MIT | PERMISSIVE | tracing, tracing-log, tracing-oslog, tracing-subscriber |
| tracing-log | 0.2.0 | MIT | PERMISSIVE | bevy_log, tracing-subscriber |
| tracing-oslog | 0.3.0 | Zlib | PERMISSIVE | bevy_log |
| tracing-subscriber | 0.3.23 | MIT | PERMISSIVE | bevy_log, profiling, tracing-attributes, tracing-oslog, tracing-wasm, winit, x11rb |
| tracing-wasm | 0.2.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_log |
| tree_magic_mini | 3.2.2 | MIT | PERMISSIVE | wl-clipboard-rs |
| ttf-parser | 0.25.1 | MIT OR Apache-2.0 | PERMISSIVE | owned_ttf_parser |
| twox-hash | 2.1.5 | MIT | PERMISSIVE | ruzstd, zerovec |
| typeid | 1.0.3 | MIT OR Apache-2.0 | PERMISSIVE | erased-serde, ron |
| typewit | 1.15.2 | Zlib | PERMISSIVE | const_panic |
| unicase | 2.10.0 | MIT OR Apache-2.0 | PERMISSIVE | clap_builder, mime_guess |
| unicode-general-category | 1.1.0 | Apache-2.0 | PERMISSIVE | epaint |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | PERMISSIVE | naga, proc-macro2, ron, syn, wasm-bindgen-shared |
| unicode-segmentation | 1.13.3 | MIT OR Apache-2.0 | PERMISSIVE | convert_case, egui, epaint, ron, winit |
| unicode-width | 0.2.3 | MIT OR Apache-2.0 | PERMISSIVE | annotate-snippets, clap_builder, codespan-reporting |
| unicode-xid | 0.2.6 | MIT OR Apache-2.0 | PERMISSIVE | derive_more-impl, litrs, rhai, unicode-ident |
| unsynn | 0.3.0 | MIT OR Apache-2.0 | PERMISSIVE | variadics_please |
| untrusted | 0.9.0 | ISC | PERMISSIVE | ring, rustls-webpki |
| ureq | 3.5.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_asset, lm-agents, lm-router, lm-voice, lm-vox |
| ureq-proto | 0.6.5 | MIT OR Apache-2.0 | PERMISSIVE | ureq |
| url | 2.5.8 | MIT OR Apache-2.0 | PERMISSIVE | cookie_store, ureq, webbrowser |
| utf8-zero | 0.8.1 | MIT OR Apache-2.0 | PERMISSIVE | ureq |
| utf8_iter | 1.0.4 | Apache-2.0 OR MIT | PERMISSIVE | icu_collections, icu_normalizer, icu_segmenter, idna |
| utf8parse | 0.2.2 | Apache-2.0 OR MIT | PERMISSIVE | anstream, anstyle-parse |
| uuid | 1.29.0 | Apache-2.0 OR MIT | PERMISSIVE | accesskit, bevy_animation, bevy_anti_alias, bevy_asset, bevy_picking, bevy_reflect, bevy_reflect_derive, bevy_ui, bevy_world_serialization, core-foundation, foldhash, gilrs, gilrs-core, lm-host |
| valuable | 0.1.1 | MIT | PERMISSIVE | compact_str, tracing-core, tracing-subscriber |
| variadics_please | 2.0.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_app, bevy_curve, bevy_ecs, bevy_material, bevy_math, bevy_reflect, bevy_render, bevy_scene, bevy_state |
| vec_map | 0.8.2 | MIT/Apache-2.0 | PERMISSIVE | gilrs, gilrs-core |
| vello_common | 0.1.0 | Apache-2.0 OR MIT | PERMISSIVE | glifo, vello_cpu |
| vello_cpu | 0.1.0 | Apache-2.0 OR MIT | PERMISSIVE | epaint |
| version_check | 0.9.5 | MIT/Apache-2.0 | PERMISSIVE | ahash, cookie, slotmap, smartstring |
| walkdir | 2.5.0 | Unlicense/MIT | PERMISSIVE | bitflags, jni, naga, num_enum, syn, toml, toml_edit |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | PERMISSIVE | getrandom |
| wasip2 | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | PERMISSIVE | getrandom |
| wasm-bindgen | 0.2.129 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_app, bevy_asset, bevy_clipboard, bevy_egui, bevy_platform, bevy_render, bevy_winit, console_error_panic_hook, cpal, getrandom, gilrs-core, glow, js-sys, raw-window-handle, sys-locale, tracing-wasm, uuid, wasm-bindgen-futures, web-sys, web-task, web-time, wgpu, wgpu-hal, winit |
| wasm-bindgen-futures | 0.4.79 | MIT OR Apache-2.0 | PERMISSIVE | bevy_asset, bevy_clipboard, bevy_egui, bevy_platform, cpal, gilrs, web-task, web-time, wgpu, winit |
| wasm-bindgen-macro | 0.2.129 | MIT OR Apache-2.0 | PERMISSIVE | wasm-bindgen |
| wasm-bindgen-macro-support | 0.2.129 | MIT OR Apache-2.0 | PERMISSIVE | wasm-bindgen-macro |
| wasm-bindgen-shared | 0.2.129 | MIT OR Apache-2.0 | PERMISSIVE | wasm-bindgen, wasm-bindgen-macro-support |
| wayland-backend | 0.3.17 | MIT | PERMISSIVE | calloop-wayland-source, smithay-client-toolkit, smithay-clipboard, wayland-client, wayland-csd-frame, wayland-protocols, wayland-protocols-experimental, wayland-protocols-misc, wayland-protocols-plasma, wayland-protocols-wlr, winit, wl-clipboard-rs |
| wayland-client | 0.31.15 | MIT | PERMISSIVE | calloop-wayland-source, cursor-icon, khronos-egl, smithay-client-toolkit, wayland-cursor, wayland-protocols, wayland-protocols-experimental, wayland-protocols-misc, wayland-protocols-plasma, wayland-protocols-wlr, winit, wl-clipboard-rs |
| wayland-csd-frame | 0.3.0 | MIT | PERMISSIVE | smithay-client-toolkit |
| wayland-cursor | 0.31.14 | MIT | PERMISSIVE | cursor-icon, smithay-client-toolkit |
| wayland-protocols | 0.32.13 | MIT | PERMISSIVE | calloop-wayland-source, khronos-egl, smithay-client-toolkit, wayland-protocols-experimental, wayland-protocols-misc, wayland-protocols-plasma, wayland-protocols-wlr, winit, wl-clipboard-rs |
| wayland-protocols-experimental | 20250721.0.1 | MIT | PERMISSIVE | smithay-client-toolkit |
| wayland-protocols-misc | 0.3.12 | MIT | PERMISSIVE | smithay-client-toolkit |
| wayland-protocols-plasma | 0.3.12 | MIT | PERMISSIVE | winit |
| wayland-protocols-wlr | 0.3.12 | MIT | PERMISSIVE | smithay-client-toolkit, wl-clipboard-rs |
| wayland-scanner | 0.31.11 | MIT | PERMISSIVE | smithay-client-toolkit, wayland-client, wayland-protocols, wayland-protocols-experimental, wayland-protocols-misc, wayland-protocols-plasma, wayland-protocols-wlr |
| wayland-sys | 0.31.11 | MIT | PERMISSIVE | wayland-backend, wgpu-hal |
| weak-table | 0.4.0 | MIT | PERMISSIVE | bevy_render |
| web-sys | 0.3.106 | MIT OR Apache-2.0 | PERMISSIVE | bevy, bevy_app, bevy_asset, bevy_clipboard, bevy_egui, bevy_render, bevy_ui_widgets, bevy_winit, cpal, egui, gilrs, gilrs-core, glow, sys-locale, web-time, webbrowser, wgpu, wgpu-hal, wgpu-types, winit |
| web-task | 1.1.3 | MIT OR Apache-2.0 | PERMISSIVE | bevy_tasks |
| web-time | 1.1.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_platform, rhai, rustls-pki-types, winit |
| webbrowser | 1.2.5 | MIT OR Apache-2.0 | PERMISSIVE | bevy_egui |
| webpki-roots | 1.0.11 | CDLA-Permissive-2.0 | REVIEW | rustls, ureq |
| weezl | 0.1.12 | MIT OR Apache-2.0 | PERMISSIVE | tiff |
| wesl | 0.6.0 | MIT OR Apache-2.0 | PERMISSIVE | bevy_shader |
| wesl-core | 0.1.0 | MIT OR Apache-2.0 | PERMISSIVE | wesl |
| wesl-macros | 0.6.0 | MIT OR Apache-2.0 | PERMISSIVE | wesl |
| wgpu | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_render, encase, smithay-client-toolkit |
| wgpu-core | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | wgpu |
| wgpu-core-deps-apple | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | wgpu-core |
| wgpu-core-deps-wasm | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | wgpu-core |
| wgpu-core-deps-windows-linux-android | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | wgpu-core |
| wgpu-hal | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | wgpu, wgpu-core, wgpu-core-deps-apple, wgpu-core-deps-wasm, wgpu-core-deps-windows-linux-android |
| wgpu-naga-bridge | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | wgpu-core, wgpu-hal |
| wgpu-types | 30.0.1 | MIT OR Apache-2.0 | PERMISSIVE | bevy_camera, bevy_clipboard, bevy_color, bevy_egui, bevy_gltf, bevy_image, bevy_light, bevy_material, bevy_mesh, bevy_pbr, bevy_reflect, bevy_render, bevy_shader, bevy_sprite, bevy_text, bevy_winit, wgpu, wgpu-core, wgpu-hal, wgpu-naga-bridge |
| wgsl-parse | 0.6.0 | MIT OR Apache-2.0 | PERMISSIVE | wesl |
| wgsl-types | 0.6.0 | MIT OR Apache-2.0 | PERMISSIVE | wesl, wgsl-parse |
| winapi | 0.3.9 | MIT/Apache-2.0 | PERMISSIVE | gilrs-core, ntapi |
| winapi-i686-pc-windows-gnu | 0.4.0 | MIT/Apache-2.0 | PERMISSIVE | winapi |
| winapi-util | 0.1.11 | Unlicense OR MIT | PERMISSIVE | same-file, walkdir |
| winapi-x86_64-pc-windows-gnu | 0.4.0 | MIT/Apache-2.0 | PERMISSIVE | winapi |
| windows | 0.62.2 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_windows, cpal, fontique, gilrs-core, gpu-allocator, sysinfo, wgpu-hal |
| windows-collections | 0.3.2 | MIT OR Apache-2.0 | PERMISSIVE | windows |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | PERMISSIVE | accesskit_windows, fontique, wgpu-hal, windows, windows-collections, windows-future, windows-numerics |
| windows-future | 0.3.2 | MIT OR Apache-2.0 | PERMISSIVE | windows |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | PERMISSIVE | windows-core |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | PERMISSIVE | windows-core |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | PERMISSIVE | gethostname, jni, libloading, parking_lot_core, windows-core, windows-future, windows-numerics, windows-result, windows-strings, windows-sys, windows-threading |
| windows-numerics | 0.3.1 | MIT OR Apache-2.0 | PERMISSIVE | windows |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | PERMISSIVE | wgpu-hal, windows-core |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | PERMISSIVE | windows-collections, windows-core |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | PERMISSIVE | anstyle-query, anstyle-wincon, arboard, async-fs, async-io, bevy_platform, ctrlc, errno, jni, libloading, nu-ansi-term, os_pipe, polling, portable-atomic, ring, rustix, tokio, winapi-util, winit |
| windows-targets | 0.52.6 | MIT OR Apache-2.0 | PERMISSIVE | windows-sys |
| windows-threading | 0.2.1 | MIT OR Apache-2.0 | PERMISSIVE | windows-future |
| windows_aarch64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | PERMISSIVE | windows-targets |
| windows_aarch64_msvc | 0.52.6 | MIT OR Apache-2.0 | PERMISSIVE | windows-targets |
| windows_i686_gnu | 0.52.6 | MIT OR Apache-2.0 | PERMISSIVE | windows-targets |
| windows_i686_gnullvm | 0.52.6 | MIT OR Apache-2.0 | PERMISSIVE | windows-targets |
| windows_i686_msvc | 0.52.6 | MIT OR Apache-2.0 | PERMISSIVE | windows-targets |
| windows_x86_64_gnu | 0.52.6 | MIT OR Apache-2.0 | PERMISSIVE | windows-targets |
| windows_x86_64_gnullvm | 0.52.6 | MIT OR Apache-2.0 | PERMISSIVE | windows-targets |
| windows_x86_64_msvc | 0.52.6 | MIT OR Apache-2.0 | PERMISSIVE | windows-targets |
| winit | 0.30.13 | Apache-2.0 | PERMISSIVE | accesskit_windows, accesskit_winit, bevy_egui, bevy_winit, wgpu-hal |
| winnow | 1.0.4 | MIT | PERMISSIVE | toml, toml_edit, toml_parser |
| wit-bindgen | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | PERMISSIVE | wasip2 |
| wl-clipboard-rs | 0.9.5 | MIT OR Apache-2.0 | PERMISSIVE | arboard |
| writeable | 0.6.4 | Unicode-3.0 | PERMISSIVE | icu_locale_core, icu_provider, potential_utf |
| wyz | 0.5.1 | MIT | PERMISSIVE | bitvec |
| x11-dl | 2.21.0 | MIT | PERMISSIVE | winit |
| x11rb | 0.13.2 | MIT OR Apache-2.0 | PERMISSIVE | arboard, winit, xkeysym |
| x11rb-protocol | 0.13.2 | MIT OR Apache-2.0 | PERMISSIVE | x11rb |
| xcursor | 0.3.11 | MIT | PERMISSIVE | wayland-cursor, x11rb |
| xkbcommon-dl | 0.4.2 | MIT | PERMISSIVE | winit |
| xkeysym | 0.2.1 | MIT OR Apache-2.0 OR Zlib | PERMISSIVE | smithay-client-toolkit, xkbcommon-dl |
| xml-rs | 0.8.29 | MIT | PERMISSIVE | gl_generator |
| yazi | 0.2.1 | Apache-2.0 OR MIT | PERMISSIVE | swash |
| yoke | 0.8.4 | Unicode-3.0 | PERMISSIVE | icu_collections, icu_provider, litemap, zerotrie, zerovec |
| yoke-derive | 0.8.4 | Unicode-3.0 | PERMISSIVE | yoke |
| zeno | 0.3.3 | Apache-2.0 OR MIT | PERMISSIVE | swash |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | PERMISSIVE | ahash, bitflags, glam, half, uuid |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | PERMISSIVE | glam, zerocopy |
| zerofrom | 0.1.8 | Unicode-3.0 | PERMISSIVE | icu_collections, icu_provider, yoke, zerotrie, zerovec |
| zerofrom-derive | 0.1.8 | Unicode-3.0 | PERMISSIVE | zerofrom |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | PERMISSIVE | arrayvec, blake3, compact_str, heapless, rustls, rustls-pki-types |
| zerotrie | 0.2.5 | Unicode-3.0 | PERMISSIVE | icu_properties, icu_provider, icu_segmenter |
| zerovec | 0.11.9 | Unicode-3.0 | PERMISSIVE | icu_collections, icu_locale_core, icu_locale_fallback, icu_normalizer, icu_properties, icu_provider, icu_segmenter, potential_utf, tinystr, zerotrie |
| zerovec-derive | 0.11.7 | Unicode-3.0 | PERMISSIVE | zerovec |
| zlib-rs | 0.6.8 | Zlib | PERMISSIVE | flate2, rustls |
| zmij | 1.0.23 | MIT | PERMISSIVE | compact_str, serde_json |
| zune-core | 0.5.3 | MIT OR Apache-2.0 OR Zlib | PERMISSIVE | image, zune-jpeg |
| zune-jpeg | 0.5.15 | MIT OR Apache-2.0 OR Zlib | PERMISSIVE | image, tiff |
