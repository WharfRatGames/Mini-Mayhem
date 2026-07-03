//! Baked Worms Armageddon sprite animations (extracted by tools/extract_wa_sprite.py).
//!
//! WSPR format: "WSPR" magic, u16 frame_w/frame_h/frame_count/pal_count (LE),
//! pal_count×3 RGB bytes, then frame_count full frame_w×frame_h index buffers
//! (0 = transparent, v > 0 → palette[v-1]).

use super::buffer::WorldBuffer;
use super::fb::Bgra;
use super::draw_sprites::TEAM_COLOURS;

const WBACKFLP_BIN: &[u8] = include_bytes!("wa_sprites/wbackflp.bin");

/// Frame bottom sits this far below pos.y (the worm art doesn't reach the
/// frame's bottom edge; tuned so the sprite reads as flipping from the feet).
const FOOT_OFF: i32 = 12;

struct WaSprite {
    w: usize,
    h: usize,
    frames: usize,
    px: &'static [u8],          // frames × w × h palette indices
    team_pal: [Vec<Bgra>; 4],   // per-team tinted palette (index v-1)
}

fn backflip_sprite() -> &'static WaSprite {
    static SPRITE: std::sync::OnceLock<WaSprite> = std::sync::OnceLock::new();
    SPRITE.get_or_init(|| {
        let d = WBACKFLP_BIN;
        assert_eq!(&d[..4], b"WSPR", "bad wbackflp.bin");
        let u16le = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]) as usize;
        let (w, h, frames, pal_count) = (u16le(4), u16le(6), u16le(8), u16le(10));
        let pal_base = 12;
        let px_base = pal_base + pal_count * 3;
        assert_eq!(d.len(), px_base + frames * w * h, "wbackflp.bin size mismatch");
        // Tint the pink worm palette per team: keep the original shading
        // (luma) but pull the hue toward the team colour.
        let team_pal = std::array::from_fn(|t| {
            let tc = TEAM_COLOURS[t];
            (0..pal_count).map(|i| {
                let (r, g, b) = (d[pal_base + i * 3] as u32,
                                 d[pal_base + i * 3 + 1] as u32,
                                 d[pal_base + i * 3 + 2] as u32);
                let luma = (r * 3 + g * 6 + b) / 10; // 0..255
                let tint = |c: u8| ((c as u32 * luma / 160).min(255)) as u8;
                Bgra::new(tint(tc.r), tint(tc.g), tint(tc.b))
            }).collect()
        });
        WaSprite { w, h, frames, px: &d[px_base..], team_pal }
    })
}

/// Draw one WA backflip frame, horizontally centred at `cx` with the frame's
/// bottom edge at `foot_y + FOOT_OFF`. The baked frames face left natively;
/// they are mirrored when `facing` is right. Frame advances one per tick.
pub fn draw_backflip(buf: &mut WorldBuffer, cx: i32, foot_y: i32, team: usize, facing: i8, airtime: u32) {
    let sp = backflip_sprite();
    let pal = &sp.team_pal[team.min(3)];
    let frame = airtime as usize % sp.frames;
    let base = frame * sp.w * sp.h;
    let x0 = cx - sp.w as i32 / 2;
    let y0 = foot_y + FOOT_OFF - sp.h as i32;
    let mirror = facing >= 0;
    for dy in 0..sp.h {
        let row = base + dy * sp.w;
        for dx in 0..sp.w {
            let sx = if mirror { sp.w - 1 - dx } else { dx };
            let v = sp.px[row + sx];
            if v == 0 { continue; }
            buf.set_pixel(x0 + dx as i32, y0 + dy as i32, pal[v as usize - 1]);
        }
    }
}
