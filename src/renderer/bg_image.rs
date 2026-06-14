//! Static background image — the lowest render layer, drawn first (behind
//! everything else: clouds, landform, debris, terrain). One PNG per terrain
//! archetype, embedded at build time. If the PNG for an archetype is missing
//! or a 1x1 placeholder, nothing is drawn and the procedural backdrop shows
//! through unobstructed.
//!
//! To supply real art: drop a PNG (any size, RGB or RGBA) into
//! `deploy/assets/backgrounds/bg_<archetype>.png` (0=plains/default,
//! 1=mountains, 2=desert, 3=cave — see Terrain::archetype). The image is
//! tiled horizontally across the world and scaled vertically to SCREEN_H,
//! scrolling at a slow parallax factor.

use super::buffer::WorldBuffer;
use super::fb::Bgra;
use crate::world::{SCREEN_H, SCREEN_W};
use std::sync::OnceLock;

const ARCHETYPE_COUNT: usize = 4;
const PAR_BG: f32 = 0.10;

struct Decoded {
    w: u32,
    h: u32,
    pixels: Vec<u8>, // RGBA8
}

static PNGS: [&[u8]; ARCHETYPE_COUNT] = [
    include_bytes!("../../deploy/assets/backgrounds/bg_0.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg_1.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg_2.png"),
    include_bytes!("../../deploy/assets/backgrounds/bg_3.png"),
];

static DECODED: OnceLock<[Option<Decoded>; ARCHETYPE_COUNT]> = OnceLock::new();

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

fn decoded() -> &'static [Option<Decoded>; ARCHETYPE_COUNT] {
    DECODED.get_or_init(|| std::array::from_fn(|i| decode(PNGS[i])))
}

/// Draw the static background image for `archetype`, scaled to SCREEN_H and
/// tiled horizontally, at the lowest parallax (slowest scroll). No-op if no
/// real image is supplied for this archetype.
pub fn draw_static_bg(buf: &mut WorldBuffer, archetype: u8, cam_x: i32) {
    let slot = (archetype as usize).min(ARCHETYPE_COUNT - 1);
    let img = match decoded()[slot].as_ref() {
        Some(img) => img,
        None => return,
    };

    let scale = SCREEN_H as f32 / img.h as f32;
    let dst_w = ((img.w as f32) * scale).round().max(1.0) as i32;
    let dst_h = SCREEN_H as i32;

    let par_x = (cam_x as f32 * PAR_BG) as i32;
    // Tile horizontally to cover the viewport plus one extra tile each side.
    let start_tile = (par_x - dst_w) / dst_w.max(1) - 1;
    let end_tile = (par_x + SCREEN_W as i32 + dst_w) / dst_w.max(1) + 1;

    for tile in start_tile..=end_tile {
        let tile_x0 = tile * dst_w - par_x;
        for dy in 0..dst_h {
            let sy = ((dy as f32) / scale) as u32;
            if sy >= img.h { continue; }
            for dx in 0..dst_w {
                let sx = ((dx as f32) / scale) as u32;
                if sx >= img.w { continue; }
                let idx = ((sy * img.w + sx) * 4) as usize;
                if idx + 3 >= img.pixels.len() { continue; }
                let a = img.pixels[idx + 3];
                if a == 0 { continue; }
                let col = Bgra::new(img.pixels[idx], img.pixels[idx + 1], img.pixels[idx + 2]);
                buf.set_pixel(cam_x + tile_x0 + dx, dy, col);
            }
        }
    }
}
