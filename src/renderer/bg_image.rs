//! Static background image — the lowest render layer, drawn first (behind
//! everything else: clouds, landform, debris, terrain). A pool of background
//! PNGs (sliced from `assets/BG/BG2.png`) embedded at build time; one is chosen
//! per map from the map seed, so the background varies match-to-match across all
//! archetypes rather than being fixed per archetype. If a PNG is missing or a
//! 1x1 placeholder, nothing is drawn and the procedural backdrop shows through.
//!
//! To refresh the art: re-slice the source sheet into
//! `deploy/assets/backgrounds/bg2_<n>.png`. Each image (any size, RGB or RGBA)
//! is tiled horizontally across the world and scaled vertically to SCREEN_H,
//! scrolling at a slow parallax factor. Only the sky region above each column's
//! terrain top is drawn (the terrain viewport copy covers the rest).

use super::buffer::WorldBuffer;
use super::fb::Bgra;
use crate::world::{Terrain, SCREEN_H, SCREEN_W, WORLD_W, WATER_Y};
use std::sync::OnceLock;

/// Number of backgrounds in the pool (BG2.png is a 3×3 contact sheet, BG1.png
/// is a 2×3 sheet contributing 6 more slices).
const BG_COUNT: usize = 15;

struct Decoded {
    w: u32,
    h: u32,
    pixels: Vec<u8>, // RGBA8
}

static PNGS: [&[u8]; BG_COUNT] = [
    include_bytes!("../../deploy/assets/backgrounds/bg2_0.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg2_1.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg2_2.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg2_3.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg2_4.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg2_5.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg2_6.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg2_7.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg2_8.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg_0.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg_1.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg_2.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg_3.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg_extra_city.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg_extra_pyramids.png"),
];

/// Pick which background to use for a map. Deterministic from the seed so client
/// and server (and every client in a live match) agree.
pub fn bg_index_for_seed(seed: u64) -> usize {
    (seed.wrapping_mul(2654435761) >> 33) as usize % BG_COUNT
}

static DECODED: OnceLock<[Option<Decoded>; BG_COUNT]> = OnceLock::new();

fn decode(bytes: &[u8]) -> Option<Decoded> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let (w, h) = (info.width, info.height);
    if w <= 1 || h <= 1 {
        return None; // placeholder, not real art
    }
    let color_type = info.color_type;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    reader.next_frame(&mut buf).ok()?;
    let pixels = match color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255u8]).collect(),
        _ => return None,
    };
    Some(Decoded { w, h, pixels })
}

fn decoded() -> &'static [Option<Decoded>; BG_COUNT] {
    DECODED.get_or_init(|| std::array::from_fn(|i| decode(PNGS[i])))
}

static SCALED: OnceLock<[Option<Decoded>; BG_COUNT]> = OnceLock::new();

/// Pre-scale each archetype's image to SCREEN_H once, so the per-frame draw
/// is a plain integer index/modulo instead of a float rescale per pixel.
fn scaled() -> &'static [Option<Decoded>; BG_COUNT] {
    SCALED.get_or_init(|| std::array::from_fn(|i| {
        let img = decoded()[i].as_ref()?;
        let scale = SCREEN_H as f32 / img.h as f32;
        let dst_w = ((img.w as f32) * scale).round().max(1.0) as u32;
        let dst_h = SCREEN_H;
        let mut pixels = vec![0u8; (dst_w * dst_h * 4) as usize];
        // Bilinear sample so upscaling (typically ~1.5x from source art to
        // SCREEN_H) doesn't look blocky/pixelated — this is a one-time
        // precompute, so the extra cost per pixel is free at runtime.
        for dy in 0..dst_h {
            let sy_f = ((dy as f32 + 0.5) / scale - 0.5).clamp(0.0, (img.h - 1) as f32);
            let sy0 = sy_f as u32;
            let sy1 = (sy0 + 1).min(img.h - 1);
            let fy = sy_f - sy0 as f32;
            for dx in 0..dst_w {
                let sx_f = ((dx as f32 + 0.5) / scale - 0.5).clamp(0.0, (img.w - 1) as f32);
                let sx0 = sx_f as u32;
                let sx1 = (sx0 + 1).min(img.w - 1);
                let fx = sx_f - sx0 as f32;

                let p00 = ((sy0 * img.w + sx0) * 4) as usize;
                let p10 = ((sy0 * img.w + sx1) * 4) as usize;
                let p01 = ((sy1 * img.w + sx0) * 4) as usize;
                let p11 = ((sy1 * img.w + sx1) * 4) as usize;

                let dst = ((dy * dst_w + dx) * 4) as usize;
                for c in 0..4 {
                    let top = img.pixels[p00 + c] as f32 * (1.0 - fx) + img.pixels[p10 + c] as f32 * fx;
                    let bot = img.pixels[p01 + c] as f32 * (1.0 - fx) + img.pixels[p11 + c] as f32 * fx;
                    pixels[dst + c] = (top * (1.0 - fy) + bot * fy).round() as u8;
                }
            }
        }
        Some(Decoded { w: dst_w, h: dst_h, pixels })
    }))
}

/// Pre-render the seed-chosen background for the whole world (once, at map
/// load) into a world-space cache, so the per-frame draw is a handful of row
/// memcpys via `copy_bg_viewport` instead of a per-pixel redraw from the
/// source image (which, for every cave/chasm/floating-island column, used to
/// paint down to the waterline — up to ~640x400px/frame).
///
/// The background must cover every pixel the terrain viewport copy leaves
/// untouched, because the frame buffer is reused across frames (otherwise stale
/// content — title screen, old debris/explosions, uninitialized black — ghosts
/// through). The viewport copy fills: all water rows, plus the solid pixels in
/// the sky band. So per column:
///   * fully-solid column (`solid_to_water`): the copy block-fills `sky_limit..
///     WATER_Y`, so we only need to paint the sky band above `sky_limit`.
///   * any column with an air gap below the top (caves, chasms, overhangs,
///     fresh craters): the copy skips those air pixels, so we must paint the
///     whole air region down to the waterline.
///
/// Each world column maps 1:1 to an image column (tiled with `% dst_w`) —
/// no parallax stretch, so the art isn't blown up/blockier than its native
/// resolution. This layer scrolls 1:1 with the world, like the terrain.
pub fn build_bg_cache(terrain: &Terrain, seed: u64) -> WorldBuffer {
    let mut cache = WorldBuffer::new();
    update_bg_cache_columns(&mut cache, terrain, seed, 0, WORLD_W as i32);
    cache
}

/// Repaint the cached background for world columns `x0..x1` (clamped to world
/// bounds). Call after a crater carve changes `sky_limit`/`solid_to_water` for
/// those columns, so newly-opened air gaps get background instead of stale
/// (black) cache pixels.
pub fn update_bg_cache_columns(cache: &mut WorldBuffer, terrain: &Terrain, seed: u64, x0: i32, x1: i32) {
    let slot = bg_index_for_seed(seed);
    let img = match scaled()[slot].as_ref() {
        Some(img) => img,
        None => return,
    };
    let dst_w = img.w;
    let dst_h = img.h;

    let x0 = x0.max(0) as u32;
    let x1 = (x1.max(0) as u32).min(WORLD_W);
    for wx in x0..x1 {
        let dx = wx % dst_w.max(1);
        // Only the sky band on contiguous-solid columns (the viewport copy
        // block-fills the rest); otherwise the whole air region down to the
        // waterline, so air gaps below the surface aren't left stale.
        let y_end = if terrain.solid_to_water[wx as usize] {
            terrain.sky_limit[wx as usize].min(dst_h)
        } else {
            WATER_Y.min(dst_h)
        };
        for dy in 0..y_end {
            let idx = ((dy * img.w + dx) * 4) as usize;
            let a = img.pixels[idx + 3];
            if a == 0 { continue; }
            let col = Bgra::new(img.pixels[idx], img.pixels[idx + 1], img.pixels[idx + 2]);
            cache.set_pixel_unchecked(wx, dy, col);
        }
    }
}

/// Copy the cached background into the viewport. Cheap row-range memcpys —
/// see `build_bg_cache`.
pub fn copy_bg_viewport(buf: &mut WorldBuffer, cache: &WorldBuffer, cam_x: u32) {
    buf.copy_viewport_rows_from(cache, cam_x, 0, WATER_Y);
}
