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
use crate::world::{Terrain, SCREEN_H, SCREEN_W};
use std::sync::OnceLock;

/// Number of backgrounds in the pool (BG2.png is a 3×3 contact sheet).
const BG_COUNT: usize = 9;
const PAR_BG: f32 = 0.10;

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
        for dy in 0..dst_h {
            let sy = ((dy as f32) / scale) as u32;
            let sy = sy.min(img.h - 1);
            for dx in 0..dst_w {
                let sx = ((dx as f32) / scale) as u32;
                let sx = sx.min(img.w - 1);
                let src = ((sy * img.w + sx) * 4) as usize;
                let dst = ((dy * dst_w + dx) * 4) as usize;
                pixels[dst..dst + 4].copy_from_slice(&img.pixels[src..src + 4]);
            }
        }
        Some(Decoded { w: dst_w, h: dst_h, pixels })
    }))
}

/// Draw the seed-chosen static background, pre-scaled to SCREEN_H and tiled
/// horizontally at the lowest parallax (slowest scroll). No-op if no real image
/// is available.
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
pub fn draw_static_bg(buf: &mut WorldBuffer, terrain: &Terrain, seed: u64, cam_x: i32) {
    let slot = bg_index_for_seed(seed);
    let img = match scaled()[slot].as_ref() {
        Some(img) => img,
        None => return,
    };

    let dst_w = img.w as i32;
    let dst_h = img.h as i32;

    let par_x = (cam_x as f32 * PAR_BG) as i32;
    // Tile horizontally to cover the viewport plus one extra tile each side.
    let start_tile = (par_x - dst_w) / dst_w.max(1) - 1;
    let end_tile = (par_x + SCREEN_W as i32 + dst_w) / dst_w.max(1) + 1;

    for tile in start_tile..=end_tile {
        let tile_x0 = tile * dst_w - par_x;
        // Clip to the columns of this tile that actually land on screen.
        let dx_lo = (-tile_x0).max(0);
        let dx_hi = (SCREEN_W as i32 - tile_x0).min(dst_w);
        if dx_lo >= dx_hi { continue; }
        for dx in dx_lo..dx_hi {
            let wx = cam_x + tile_x0 + dx;
            if wx < 0 || wx >= crate::world::WORLD_W as i32 { continue; }
            // Only clip to the sky band on contiguous-solid columns (the copy
            // block-fills the rest); otherwise paint down to the waterline so
            // air gaps below the surface aren't left stale.
            let y_end = if terrain.solid_to_water[wx as usize] {
                terrain.sky_limit[wx as usize].min(dst_h as u32) as i32
            } else {
                (crate::world::WATER_Y).min(dst_h as u32) as i32
            };
            for dy in 0..y_end {
                let idx = ((dy as u32 * img.w + dx as u32) * 4) as usize;
                let a = img.pixels[idx + 3];
                if a == 0 { continue; }
                let col = Bgra::new(img.pixels[idx], img.pixels[idx + 1], img.pixels[idx + 2]);
                buf.set_pixel(wx, dy, col);
            }
        }
    }
}
