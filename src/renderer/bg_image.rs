//! Static background image — the lowest render layer, drawn first (behind
//! everything else: clouds, landform, debris, terrain). A pool of background
//! PNGs (sliced from `assets/BG/BG2.png`) embedded at build time; one is chosen
//! per map from the map seed, so the background varies match-to-match across all
//! archetypes rather than being fixed per archetype. If a PNG is missing or a
//! 1x1 placeholder, nothing is drawn and the procedural backdrop shows through.
//!
//! To refresh the art: re-slice the source sheet into
//! `deploy/assets/backgrounds/bg2_<n>.png`. Each image (any size, RGB or RGBA)
//! is scaled vertically to SCREEN_H and cached 1:1 (no stretching), then tiled
//! horizontally and scrolled at a slow parallax factor. Only the sky region
//! above each column's terrain top is drawn (the terrain viewport copy covers
//! the rest).

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

/// Parallax factor: this layer scrolls at this fraction of camera movement
/// (slow horizontal scroll relative to the foreground).
const PAR_BG: f32 = 0.10;

/// Pre-render the seed-chosen background image into a small world-space cache
/// (one cache column per source-image column, 1:1 — no stretching, so the art
/// isn't blown up/blockier than its native resolution), once at map load.
/// `copy_bg_viewport` re-samples this cache each frame with a parallax-shifted
/// offset, so the per-frame cost is a cheap column copy instead of a per-pixel
/// redraw from the source image.
pub fn build_bg_cache(seed: u64) -> WorldBuffer {
    let mut cache = WorldBuffer::new();
    let slot = bg_index_for_seed(seed);
    let img = match scaled()[slot].as_ref() {
        Some(img) => img,
        None => return cache,
    };
    let dst_w = img.w.min(WORLD_W);
    let dst_h = img.h.min(WATER_Y);
    for dx in 0..dst_w {
        for dy in 0..dst_h {
            let idx = ((dy * img.w + dx) * 4) as usize;
            let a = img.pixels[idx + 3];
            if a == 0 { continue; }
            let col = Bgra::new(img.pixels[idx], img.pixels[idx + 1], img.pixels[idx + 2]);
            cache.set_pixel_unchecked(dx, dy, col);
        }
    }
    cache
}

/// Copy the cached background into the viewport with a slow parallax scroll
/// (`PAR_BG`).
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
pub fn copy_bg_viewport(buf: &mut WorldBuffer, cache: &WorldBuffer, terrain: &Terrain, seed: u64, cam_x: u32) {
    let slot = bg_index_for_seed(seed);
    let dst_w = match scaled()[slot].as_ref() {
        Some(img) => img.w.min(WORLD_W),
        None => return,
    };
    if dst_w == 0 { return; }

    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
    let par_x = ((cam_x as f32) * PAR_BG) as u32 % dst_w;
    for sx in 0..SCREEN_W {
        let wx = cam_x + sx;
        let src_x = (par_x + sx) % dst_w;
        let y_end = if terrain.solid_to_water[wx as usize] {
            terrain.sky_limit[wx as usize].min(WATER_Y)
        } else {
            WATER_Y
        };
        for y in 0..y_end {
            let c = cache.get_pixel_unchecked(src_x, y);
            buf.set_pixel_unchecked(wx, y, c);
        }
    }
}
