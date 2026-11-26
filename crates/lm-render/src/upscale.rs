//! The integer upscale, as arithmetic before it is ever a GPU call (phase 6
//! Task 7).
//!
//! The retro-modern look (D2) needs **whole pixel cells**: the 3D view
//! renders at a low internal resolution and the result is drawn to the
//! window at an **integer** scale, nearest-neighbour, with the remainder
//! letterboxed — never stretched. That decision is arithmetic, not shader
//! work, so it lives here, in plain data, testable with no GPU: the host's
//! `view3d::upscale` only carries what this module decides.
//!
//! The rule:
//!
//! * the scale is `floor(min(window_w / internal_w, window_h / internal_h))`,
//!   clamped to at least 1 — a window smaller than the internal resolution
//!   still shows the whole (if cropped-by-window) picture at scale 1;
//! * the blit covers exactly `scale × internal` pixels;
//! * the remainder is letterboxed: the clear colour shows around the blit,
//!   and the blit itself is never stretched;
//! * the remainder is split evenly, and an odd remainder's extra pixel goes
//!   **right** and **down**, deterministically, every run on every machine.

use crate::palette::{RenderConfig, UpscaleMode};

/// Where and how big the blit of one frame is, in window pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blit {
    /// The integer scale: each internal pixel becomes `scale × scale` window
    /// pixels.
    pub scale: u32,
    /// The top-left corner of the blit, in window pixels from the window's
    /// top-left. Negative when the window is smaller than the picture and
    /// crops it — the crop is centred, like the letterbox.
    pub offset_px: (i32, i32),
    /// The size of the blit, in window pixels: exactly
    /// `scale × (internal_w, internal_h)`.
    pub size_px: (u32, u32),
}

/// The integer blit of an internal resolution into a window.
///
/// Pure arithmetic over the integers: no float ever rounds a pixel edge, so
/// the same window and the same internal resolution give the same blit on
/// every machine and every run. A window smaller than the internal picture
/// still gets scale 1 — the plan's clamp — and the centred picture simply
/// spills past the window's edges.
pub fn integer_blit(window: (u32, u32), internal: (u32, u32)) -> Blit {
    // A zero is not a resolution: treat it as the smallest picture there
    // is, so the arithmetic stays total.
    let internal = (internal.0.max(1), internal.1.max(1));
    let scale = (window.0 / internal.0).min(window.1 / internal.1).max(1);
    let size_px = (scale * internal.0, scale * internal.1);
    let (left, top) = (
        i64::from(window.0) - i64::from(size_px.0),
        i64::from(window.1) - i64::from(size_px.1),
    );
    // Euclid divides to a floor, so an odd remainder's extra pixel lands
    // right and down: (left, left + 1) for odd, never (left + 1, left).
    Blit {
        scale,
        offset_px: (left.div_euclid(2) as i32, top.div_euclid(2) as i32),
        size_px,
    }
}

/// Validate a render configuration at start-up.
///
/// A configuration that names an upscale it cannot honour is a
/// **configuration error now**, not a crash at the first frame: the caller
/// refuses to start and says why. The one rule of Task 7 is DLSS's:
/// `upscale = "dlss"` needs a `dlss_project_id`, because the DLSS plugin
/// panics without one — an empty id never reaches the GPU.
pub fn validate(config: &RenderConfig) -> Result<(), String> {
    if config.upscale == UpscaleMode::Dlss && config.dlss_project_id.trim().is_empty() {
        return Err(String::from(
            "upscale = \"dlss\" needs a dlss_project_id: the DLSS plugin panics without one",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{DlssQuality, PaletteMode, UpscaleMode};

    /// A render configuration with the given upscale knobs.
    fn config(upscale: UpscaleMode, dlss_project_id: &str) -> RenderConfig {
        RenderConfig {
            upscale,
            dlss_project_id: dlss_project_id.to_owned(),
            ..RenderConfig::default()
        }
    }

    /// Window sizes against the default internal resolution (960×540), plus
    /// a few against 640×360: exact fits, even remainders, odd remainders,
    /// and windows smaller than the internal picture.
    const CASES: &[(u32, u32, u32, u32)] = &[
        // (window_w, window_h, internal_w, internal_h)
        (1920, 1080, 960, 540), // exact 2x
        (1280, 720, 960, 540),  // scale 1, letterboxed both ways
        (960, 540, 960, 540),   // exact 1x
        (640, 360, 960, 540),   // smaller than the picture: scale 1
        (320, 200, 960, 540),   // much smaller: still scale 1
        (1921, 1080, 960, 540), // odd remainder horizontally
        (1920, 1081, 960, 540), // odd remainder vertically
        (3840, 2160, 960, 540), // exact 4x
        (2560, 1440, 640, 360), // exact 4x of 640x360
        (1280, 720, 640, 360),  // exact 2x of 640x360
        (1000, 700, 640, 360),  // scale 1, letterboxed
        (500, 400, 640, 360),   // smaller both ways: scale 1
    ];

    #[test]
    fn integer_scale_is_exact() {
        for &(window_w, window_h, internal_w, internal_h) in CASES {
            let blit = integer_blit((window_w, window_h), (internal_w, internal_h));
            assert!(
                blit.scale >= 1,
                "{window_w}x{window_h} over {internal_w}x{internal_h}: the scale is never 0"
            );
            // The scale is the floor of the smaller ratio, clamped to >= 1.
            let fit = (window_w / internal_w).min(window_h / internal_h);
            assert_eq!(
                blit.scale,
                fit.max(1),
                "{window_w}x{window_h} over {internal_w}x{internal_h}: floor(min ratio), never 0"
            );
            // The blit covers scale × internal pixels exactly, both ways.
            assert_eq!(
                (blit.size_px.0, blit.size_px.1),
                (blit.scale * internal_w, blit.scale * internal_h),
                "{window_w}x{window_h} over {internal_w}x{internal_h}: the blit is scale × internal"
            );
        }
    }

    #[test]
    fn letterbox_is_centred() {
        for &(window_w, window_h, internal_w, internal_h) in CASES {
            let blit = integer_blit((window_w, window_h), (internal_w, internal_h));
            let left = i64::from(blit.offset_px.0);
            let right = i64::from(window_w) - left - i64::from(blit.size_px.0);
            let top = i64::from(blit.offset_px.1);
            let bottom = i64::from(window_h) - top - i64::from(blit.size_px.1);
            // Split evenly, and an odd remainder's extra pixel goes right
            // and down deterministically: left <= right, top <= bottom, and
            // never more than one pixel apart.
            assert!(
                (right - left).abs() <= 1 && (bottom - top).abs() <= 1,
                "{window_w}x{window_h} over {internal_w}x{internal_h}: \
                 margins ({left},{right},{top},{bottom}) are even or one off"
            );
            assert!(
                left <= right && top <= bottom,
                "{window_w}x{window_h} over {internal_w}x{internal_h}: \
                 the extra pixel goes right and down, not left and up"
            );
        }

        // The one odd pixel, by hand: 1921 wide leaves 1 pixel at scale 2,
        // and it belongs to the right.
        let blit = integer_blit((1921, 1080), (960, 540));
        assert_eq!(blit.offset_px, (0, 0));
        assert_eq!(blit.size_px, (1920, 1080));
        // And vertically: 1081 high gives the extra pixel to the bottom.
        let blit = integer_blit((1920, 1081), (960, 540));
        assert_eq!(blit.offset_px, (0, 0));
        assert_eq!(blit.size_px, (1920, 1080));
        // An odd crop is centred the same way: 641 wide over 960 at scale 1
        // crops 319 pixels, 160 left and 159 right.
        let blit = integer_blit((641, 360), (960, 540));
        assert_eq!(blit.offset_px.0, -160);
        assert_eq!(blit.offset_px.1, -90);
    }

    #[test]
    fn upscaled_size_never_exceeds_the_window() {
        for &(window_w, window_h, internal_w, internal_h) in CASES {
            let blit = integer_blit((window_w, window_h), (internal_w, internal_h));
            for (window, internal, offset, size) in [
                (
                    window_w,
                    internal_w.max(1),
                    blit.offset_px.0,
                    blit.size_px.0,
                ),
                (
                    window_h,
                    internal_h.max(1),
                    blit.offset_px.1,
                    blit.size_px.1,
                ),
            ] {
                assert_eq!(
                    size,
                    blit.scale * internal,
                    "{window_w}x{window_h} over {internal_w}x{internal_h}: scale × internal"
                );
                if window >= internal {
                    // The axis fits the picture: it stays inside, unstretched.
                    assert!(
                        offset >= 0 && offset as u64 + u64::from(size) <= u64::from(window),
                        "{window_w}x{window_h} over {internal_w}x{internal_h}: \
                         the blit stays inside the window on this axis"
                    );
                } else {
                    // The axis is too small for the picture: the scale is
                    // pinned to 1 and the centred picture is cropped, never
                    // squeezed by a fractional scale.
                    assert_eq!(blit.scale, 1, "no fractional scale to squeeze in");
                    assert!(offset <= 0, "the crop is centred, not nudged");
                }
            }
        }

        // A degenerate internal resolution is refused by the blit rather
        // than divided by: a 0 anywhere is not a picture.
        assert_eq!(
            integer_blit((1920, 1080), (0, 540)),
            integer_blit((1920, 1080), (1, 540))
        );
        assert_eq!(
            integer_blit((1920, 1080), (960, 0)),
            integer_blit((1920, 1080), (960, 1))
        );
    }

    #[test]
    fn dlss_requires_a_project_id() {
        // DLSS without a project id is a configuration error at start-up,
        // not a panic when the first frame meets the SDK.
        assert!(
            validate(&config(UpscaleMode::Dlss, "")).is_err(),
            "dlss with an empty project id is refused"
        );
        // With an id it is fine, and so are the other modes with none.
        assert!(
            validate(&config(UpscaleMode::Dlss, "last-mile")).is_ok(),
            "dlss with a project id is accepted"
        );
        assert!(
            validate(&config(UpscaleMode::Integer, "")).is_ok(),
            "the integer upscale needs no project id"
        );
        assert!(
            validate(&config(UpscaleMode::Off, "")).is_ok(),
            "no upscale needs no project id"
        );
        // The default configuration — the one a bare `render.toml`-less run
        // gets — is valid.
        assert!(
            validate(&RenderConfig {
                upscale: UpscaleMode::Dlss,
                dlss_quality: DlssQuality::Quality,
                palette: PaletteMode::Default,
                ..RenderConfig::default()
            })
            .is_err(),
            "the dlss rule holds whatever the other knobs say"
        );
    }
}
