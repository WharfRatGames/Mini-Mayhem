//! Baked WA sprite animations (WSPR format, produced by tools/extract_wa_sprite.py).
//!
//! Format: "WSPR" magic, u16 frame_w / frame_h / frame_count / pal_count (LE),
//! pal_count×3 RGB bytes, then frame_count × frame_w × frame_h palette indices
//! (0 = transparent, v > 0 → palette[v-1]).

use std::sync::OnceLock;

use super::buffer::WorldBuffer;
use super::fb::Bgra;

pub struct WaSprite {
    pub frame_w: u32,
    pub frame_h: u32,
    pub frame_count: u32,
    pal: Vec<Bgra>,
    /// frame_count × frame_w × frame_h palette indices.
    px: &'static [u8],
    /// Per-frame tight bounds of non-transparent pixels: (x0, y0, x1, y1) inclusive.
    bounds: Vec<(u32, u32, u32, u32)>,
}

impl WaSprite {
    fn parse(data: &'static [u8]) -> WaSprite {
        assert_eq!(&data[..4], b"WSPR", "bad WSPR magic");
        let rd16 = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]) as u32;
        let (w, h, n, pc) = (rd16(4), rd16(6), rd16(8), rd16(10));
        let mut off = 12;
        let pal: Vec<Bgra> = (0..pc)
            .map(|i| {
                let p = off + (i as usize) * 3;
                Bgra::new(data[p], data[p + 1], data[p + 2])
            })
            .collect();
        off += pc as usize * 3;
        let px = &data[off..off + (n * w * h) as usize];
        let bounds = (0..n)
            .map(|f| {
                let fr = &px[(f * w * h) as usize..((f + 1) * w * h) as usize];
                let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
                for y in 0..h {
                    for x in 0..w {
                        if fr[(y * w + x) as usize] != 0 {
                            x0 = x0.min(x);
                            y0 = y0.min(y);
                            x1 = x1.max(x);
                            y1 = y1.max(y);
                        }
                    }
                }
                (x0, y0, x1, y1)
            })
            .collect();
        WaSprite { frame_w: w, frame_h: h, frame_count: n, pal, px, bounds }
    }

    /// Blit one frame in world coordinates, horizontally centred on `cx` with
    /// the bottom of the visible blob resting on `base_y`. Out-of-bounds pixels
    /// clip via `set_pixel`; index 0 is transparent.
    pub fn draw_frame(&self, buf: &mut WorldBuffer, frame: u32, cx: i32, base_y: i32) {
        let frame = frame % self.frame_count;
        let (x0, y0, x1, y1) = self.bounds[frame as usize];
        if x1 < x0 {
            return; // fully transparent frame
        }
        let fr = &self.px[(frame * self.frame_w * self.frame_h) as usize..];
        let bw = (x1 - x0 + 1) as i32;
        let left = cx - bw / 2;
        let top = base_y - (y1 - y0) as i32;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let v = fr[(y * self.frame_w + x) as usize];
                if v != 0 {
                    let col = self.pal[(v - 1) as usize];
                    buf.set_pixel(left + (x - x0) as i32, top + (y - y0) as i32, col);
                }
            }
        }
    }
}

static FLAME1: OnceLock<WaSprite> = OnceLock::new();
static FLAME2: OnceLock<WaSprite> = OnceLock::new();

/// The two WA flame animations (32 frames each: ~0-21 flicker loop,
/// ~22-31 shrink-to-spark die-out). Two variants so neighbouring flames
/// don't animate in lockstep.
pub fn flame(variant: u32) -> &'static WaSprite {
    if variant & 1 == 0 {
        FLAME1.get_or_init(|| WaSprite::parse(include_bytes!("wa_sprites/flame1.bin")))
    } else {
        FLAME2.get_or_init(|| WaSprite::parse(include_bytes!("wa_sprites/flame2.bin")))
    }
}
