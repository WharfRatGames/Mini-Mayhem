//! WA-style scenery objects + girder bridges, drawn INTO the land (solid, destructible), as WA's generator does.
//!
//! Decoded from WA.exe (see `findings/mapgen.md`, "WA scenery placement"):
//!  * scenery  = FUN_00577e70 (count 5 by default), block fit test FUN_00577b00 / mask FUN_0057a1d0+0057a300, sweep FUN_00577d40;
//!  * bridges  = FUN_00578c00 (21 tries by default), gap test FUN_00578a80/00578980, draw FUN_00578810.
//! Deviations: WA's RNG state at this stage is not reproducible (it depends on the whole generator's stream), so this uses its own
//! LCG (same constants) seeded from the map seed; the sprite art is Arty's own (placeholders at WA's real sizes, not WA's pixels);
//! (the V-shaped clearance test FUN_005789e0 IS ported).

use super::constants::{WATER_Y, WORLD_W};
use super::terrain::{Terrain, Theme};

/// Land bitmap height WA uses (rows); the land is bottom-aligned on `WATER_Y`.
const LAND_H: usize = 696;
const GW: i32 = (WORLD_W as usize / 8) as i32; // 240 block columns
const GH: i32 = (LAND_H / 8) as i32; // 87 block rows
const SCENERY_COUNT: u32 = 5;
const BRIDGE_COUNT: u32 = 21;

// ---------------------------------------------------------------------------------------------------- palette (index 0 = none)
pub const PAL: [[u8; 3]; 28] = [
    [0, 0, 0],       // 0 unused
    [74, 138, 66],   // 1 leaf
    [52, 108, 56],   // 2 leaf dark
    [108, 78, 52],   // 3 trunk
    [126, 122, 120], // 4 rock
    [98, 94, 96],    // 5 rock dark
    [224, 176, 60],  // 6 flower
    [214, 176, 112], // 7 sand
    [86, 132, 84],   // 8 cactus
    [62, 104, 66],   // 9 cactus dark
    [232, 224, 204], // 10 bone
    [120, 196, 220], // 11 crystal
    [78, 150, 188],  // 12 crystal dark
    [112, 100, 112], // 13 stone
    [84, 74, 88],    // 14 stone dark
    [40, 34, 40],    // 15 dark
    [214, 96, 148],  // 16 gem
    [196, 84, 72],   // 17 mushroom
    [176, 138, 88],  // 18 sand stripe
    [150, 156, 164], // 19 steel
    [100, 106, 116], // 20 steel dark
    [150, 92, 60],   // 21 rust
    [88, 62, 42],    // 22 trunk dark
    [96, 150, 78],   // 23 leaf light
    [156, 224, 244], // 24 crystal light
    [170, 176, 184], // 25 steel light
    [146, 142, 140], // 26 rock light
    [236, 204, 150], // 27 sand light
];

// ---------------------------------------------------------------------------------------------------- sprite canvas
#[derive(Clone)]
pub struct Sprite {
    pub name: &'static str,
    pub w: usize,
    pub h: usize,
    /// Attach direction (WA .inf field): 3 floor, 2 ceiling, 0 base on the left side (sweeps left), 1 base on the right (sweeps right).
    pub dir: u8,
    pub pix: Vec<u8>,
}

struct Cv {
    w: i32,
    h: i32,
    pix: Vec<u8>,
}

impl Cv {
    fn new(w: usize, h: usize) -> Cv { Cv { w: w as i32, h: h as i32, pix: vec![0; w * h] } }
    fn set(&mut self, x: i32, y: i32, c: u8) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h { self.pix[(y * self.w + x) as usize] = c; }
    }
    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: u8) {
        for y in y0.round() as i32..y1.round() as i32 { for x in x0.round() as i32..x1.round() as i32 { self.set(x, y, c); } }
    }
    fn ellipse(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: u8) {
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (rx, ry) = (((x1 - x0) / 2.0).max(0.5), ((y1 - y0) / 2.0).max(0.5));
        for y in y0.floor() as i32..=y1.ceil() as i32 {
            for x in x0.floor() as i32..=x1.ceil() as i32 {
                let (dx, dy) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
                if dx * dx + dy * dy <= 1.0 { self.set(x, y, c); }
            }
        }
    }
    /// Even-odd scanline polygon fill.
    fn poly(&mut self, pts: &[(f32, f32)], c: u8) {
        let (mut ymin, mut ymax) = (f32::MAX, f32::MIN);
        for p in pts { ymin = ymin.min(p.1); ymax = ymax.max(p.1); }
        for y in ymin.floor() as i32..=ymax.ceil() as i32 {
            let sy = y as f32 + 0.5;
            let mut xs: Vec<f32> = Vec::new();
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if (a.1 <= sy && b.1 > sy) || (b.1 <= sy && a.1 > sy) { xs.push(a.0 + (sy - a.1) / (b.1 - a.1) * (b.0 - a.0)); }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for pair in xs.chunks(2) {
                if pair.len() == 2 { for x in pair[0].round() as i32..pair[1].round() as i32 { self.set(x, y, c); } }
            }
        }
    }
    fn flip_h(&mut self) {
        for y in 0..self.h { for x in 0..self.w / 2 {
            let (a, b) = ((y * self.w + x) as usize, (y * self.w + self.w - 1 - x) as usize);
            self.pix.swap(a, b);
        } }
    }
    fn done(self, name: &'static str, dir: u8) -> Sprite { Sprite { name, w: self.w as usize, h: self.h as usize, dir, pix: self.pix } }
}

// ---------------------------------------------------------------------------------------------------- painters (sizes = WA theme sets)
fn tree(w: usize, h: usize, canopy: f32, lean: f32, name: &'static str) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    let tw = (wf * 0.16).max(8.0); let cx = wf / 2.0 + lean;
    c.poly(&[(cx - tw / 2.0, hf), (cx + tw / 2.0, hf), (cx + tw / 3.0, hf * (1.0 - canopy) + 10.0), (cx - tw / 3.0, hf * (1.0 - canopy) + 10.0)], 3);
    c.rect(cx + tw * 0.1, hf * (1.0 - canopy) + 12.0, cx + tw / 2.0, hf, 22);
    let ch = hf * canopy;
    c.ellipse(4.0, 0.0, wf - 4.0, ch * 0.95, 2);
    c.ellipse(wf * 0.12, ch * 0.18, wf * 0.78, ch * 0.8, 1);
    c.ellipse(wf * 0.4, ch * 0.3, wf * 0.95, ch * 0.9, 1);
    c.ellipse(wf * 0.2, ch * 0.22, wf * 0.5, ch * 0.45, 23);
    c.done(name, 3)
}
fn pine(w: usize, h: usize) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.rect(wf / 2.0 - 5.0, hf * 0.82, wf / 2.0 + 5.0, hf, 3);
    for i in 0..4 {
        let top = hf * 0.02 + i as f32 * hf * 0.19; let bot = top + hf * 0.36; let half = wf * (0.26 + 0.24 * (i as f32 + 1.0) / 4.0);
        c.poly(&[(wf / 2.0, top), (wf / 2.0 + half, bot), (wf / 2.0 - half, bot)], if i % 2 == 1 { 2 } else { 1 });
    }
    c.done("Pine", 3)
}
fn rock(w: usize, h: usize, tone: u8, name: &'static str) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.poly(&[(0.0, hf), (wf * 0.08, hf * 0.45), (wf * 0.3, hf * 0.08), (wf * 0.62, 0.0), (wf * 0.88, hf * 0.3), (wf, hf)], tone);
    c.poly(&[(wf * 0.3, hf * 0.08), (wf * 0.62, 0.0), (wf * 0.5, hf * 0.55), (wf * 0.28, hf * 0.9)], if tone == 4 { 5 } else { 18 });
    c.done(name, 3)
}
fn bush(w: usize, h: usize) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.ellipse(0.0, hf * 0.2, wf * 0.55, hf * 1.3, 2); c.ellipse(wf * 0.3, 0.0, wf, hf * 1.4, 1);
    for (fx, fy) in [(0.3, 0.45), (0.62, 0.35), (0.78, 0.6)] { c.ellipse(wf * fx - 3.0, hf * fy - 3.0, wf * fx + 3.0, hf * fy + 3.0, 6); }
    c.done("Bush", 3)
}
fn cactus(w: usize, h: usize, arms: u8, name: &'static str) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    let tw = (wf * 0.3).max(14.0); let cx = wf / 2.0;
    c.rect(cx - tw / 2.0, hf * 0.05, cx + tw / 2.0, hf, 8); c.ellipse(cx - tw / 2.0, 0.0, cx + tw / 2.0, hf * 0.14, 8);
    c.rect(cx - tw / 2.0, hf * 0.05, cx - tw / 6.0, hf, 9);
    if arms >= 1 { c.rect(2.0, hf * 0.38, cx - tw / 2.0 + 2.0, hf * 0.38 + tw * 0.7, 9); c.rect(2.0, hf * 0.16, 2.0 + tw * 0.7, hf * 0.38 + tw * 0.7, 9); }
    if arms >= 2 { c.rect(cx + tw / 2.0 - 2.0, hf * 0.52, wf - 2.0, hf * 0.52 + tw * 0.7, 9); c.rect(wf - 2.0 - tw * 0.7, hf * 0.3, wf - 2.0, hf * 0.52 + tw * 0.7, 9); }
    c.done(name, 3)
}
fn butte(w: usize, h: usize) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.poly(&[(0.0, hf), (wf * 0.06, hf * 0.5), (wf * 0.2, hf * 0.34), (wf * 0.22, 0.0), (wf * 0.78, 0.0), (wf * 0.82, hf * 0.34), (wf * 0.94, hf * 0.5), (wf, hf)], 7);
    for i in 1..5 { let y = hf * (0.2 + 0.17 * i as f32); c.rect(wf * 0.1, y, wf * 0.9, y + 4.0, 18); }
    c.rect(wf * 0.24, 0.0, wf * 0.76, 5.0, 27);
    c.done("Butte", 3)
}
fn skull(w: usize, h: usize) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.ellipse(wf * 0.1, 0.0, wf * 0.9, hf * 0.75, 10); c.rect(wf * 0.28, hf * 0.6, wf * 0.72, hf, 10);
    c.ellipse(wf * 0.22, hf * 0.3, wf * 0.44, hf * 0.58, 15); c.ellipse(wf * 0.56, hf * 0.3, wf * 0.78, hf * 0.58, 15);
    for i in 0..4 { let x = wf * (0.33 + 0.1 * i as f32); c.rect(x, hf * 0.78, x + 3.0, hf, 15); }
    c.done("Skull", 3)
}
fn spire(w: usize, h: usize, up: bool, name: &'static str) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    if up {
        c.poly(&[(0.0, hf), (wf * 0.22, hf * 0.35), (wf * 0.5, 0.0), (wf * 0.78, hf * 0.4), (wf, hf)], 13);
        c.poly(&[(wf * 0.5, 0.0), (wf * 0.78, hf * 0.4), (wf, hf), (wf * 0.55, hf)], 14);
        c.done(name, 3)
    } else {
        c.poly(&[(0.0, 0.0), (wf * 0.22, hf * 0.65), (wf * 0.5, hf), (wf * 0.78, hf * 0.6), (wf, 0.0)], 13);
        c.poly(&[(wf * 0.5, hf), (wf * 0.78, hf * 0.6), (wf, 0.0), (wf * 0.55, 0.0)], 14);
        c.done(name, 2)
    }
}
fn sand_spire(w: usize, h: usize) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.poly(&[(0.0, hf), (wf * 0.22, hf * 0.35), (wf * 0.5, 0.0), (wf * 0.78, hf * 0.4), (wf, hf)], 7);
    c.poly(&[(wf * 0.5, 0.0), (wf * 0.78, hf * 0.4), (wf, hf), (wf * 0.55, hf)], 18);
    c.done("Spire rock", 3)
}
fn crystals(w: usize, h: usize, name: &'static str) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    for (fx, fw, fh, col) in [(0.1f32, 0.3f32, 0.6f32, 12u8), (0.32, 0.36, 1.0, 11), (0.62, 0.3, 0.72, 12)] {
        let (x0, ww, top) = (wf * fx, wf * fw, hf * (1.0 - fh));
        c.poly(&[(x0, hf), (x0 + ww * 0.15, top + ww * 0.5), (x0 + ww * 0.5, top), (x0 + ww * 0.85, top + ww * 0.5), (x0 + ww, hf)], col);
        c.poly(&[(x0 + ww * 0.5, top), (x0 + ww * 0.85, top + ww * 0.5), (x0 + ww * 0.6, top + ww * 0.5)], 24);
    }
    c.done(name, 3)
}
fn bones(w: usize, h: usize) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.ellipse(wf * 0.05, hf * 0.05, wf * 0.55, hf * 0.55, 10);
    c.ellipse(wf * 0.18, hf * 0.2, wf * 0.28, hf * 0.32, 15); c.ellipse(wf * 0.32, hf * 0.2, wf * 0.42, hf * 0.32, 15);
    for i in 0..5 { let y = hf * (0.45 + 0.11 * i as f32); c.rect(wf * 0.35, y, wf * 0.95, y + 7.0, 10); c.ellipse(wf * 0.9, y - 3.0, wf * 0.98, y + 10.0, 10); }
    c.done("Bones", 3)
}
fn mushroom(w: usize, h: usize) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.rect(wf * 0.4, hf * 0.45, wf * 0.6, hf, 10); c.ellipse(0.0, 0.0, wf, hf * 0.62, 17);
    for fx in [0.25f32, 0.5, 0.72] { c.ellipse(wf * fx - 5.0, hf * 0.18, wf * fx + 5.0, hf * 0.18 + 10.0, 10); }
    c.done("Mushroom", 3)
}
/// Wall piece, base on the LEFT edge (dir 0): a plate of rock 8 px wide embedded in the wall, crystals pointing right.
fn wall_crystals(w: usize, h: usize) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.rect(0.0, hf * 0.12, 12.0, hf * 0.9, 13);
    for (fy, fl, fh, col) in [(0.12f32, 0.7f32, 0.3f32, 12u8), (0.36, 1.0, 0.34, 11), (0.62, 0.62, 0.28, 12)] {
        let (y0, len, th) = (hf * fy, wf * fl, hf * fh);
        c.poly(&[(6.0, y0), (len * 0.7, y0 + th * 0.12), (len, y0 + th * 0.5), (len * 0.7, y0 + th * 0.88), (6.0, y0 + th)], col);
        c.poly(&[(len * 0.35, y0 + th * 0.1), (len, y0 + th * 0.5), (len * 0.5, y0 + th * 0.4)], 24);
    }
    let mut s = c.done("Wall crystals", 0); s.dir = 0; s
}
/// Wall piece, base on the LEFT edge: a skull plaque jutting from the wall.
fn wall_skull(w: usize, h: usize) -> Sprite {
    let mut c = Cv::new(w, h); let (wf, hf) = (w as f32, h as f32);
    c.rect(0.0, hf * 0.1, 12.0, hf * 0.95, 14);
    c.rect(6.0, hf * 0.3, wf * 0.55, hf * 0.62, 13);
    c.ellipse(wf * 0.3, hf * 0.05, wf * 0.97, hf * 0.8, 10); c.rect(wf * 0.42, hf * 0.62, wf * 0.9, hf, 10);
    c.ellipse(wf * 0.46, hf * 0.28, wf * 0.62, hf * 0.52, 15); c.ellipse(wf * 0.7, hf * 0.28, wf * 0.86, hf * 0.52, 15);
    for i in 0..4 { let x = wf * (0.5 + 0.09 * i as f32); c.rect(x, hf * 0.72, x + 3.0, hf, 15); }
    c.done("Wall skull", 0)
}
fn mirrored(mut s: Sprite, name: &'static str) -> Sprite {
    for y in 0..s.h { let row = &mut s.pix[y * s.w..(y + 1) * s.w]; row.reverse(); }
    s.dir = 1; s.name = name; s
}

pub fn set_for(theme: Theme) -> Vec<Sprite> {
    match theme {
        Theme::Pastoral => vec![
            tree(116, 203, 0.5, 0.0, "Tall tree"), tree(112, 118, 0.62, 0.0, "Round tree"), tree(68, 128, 0.55, 2.0, "Slim tree"), pine(116, 150),
            tree(92, 96, 0.6, 0.0, "Small tree"), rock(60, 44, 4, "Rock"), rock(60, 44, 4, "Rock (flat)"), bush(84, 40),
        ],
        Theme::Rugged => vec![
            cactus(68, 104, 2, "Cactus"), cactus(64, 70, 1, "Cactus (1 arm)"), cactus(80, 118, 2, "Tall cactus"), rock(72, 41, 7, "Boulder"),
            rock(92, 51, 7, "Wide boulder"), sand_spire(76, 133), butte(184, 115), skull(80, 55),
        ],
        Theme::Underground => {
            let wc = wall_crystals(100, 70); let ws = wall_skull(96, 66);
            vec![
                spire(72, 159, true, "Stalagmite"), spire(68, 115, true, "Stalagmite (short)"), crystals(96, 88, "Crystals"), crystals(100, 118, "Crystals (tall)"),
                { let mut s = skull(100, 75); s.name = "Skull"; s }, bones(148, 147), mushroom(72, 80),
                spire(48, 120, false, "Stalactite"), spire(60, 140, false, "Stalactite (long)"),
                mirrored(wc.clone(), "Wall crystals (R)"), wc, mirrored(ws.clone(), "Wall skull (R)"), ws,
            ]
        }
    }
}

// girder pieces: WA's are 64x50 caps and a 64x19 middle strip
fn girder_mid() -> Sprite {
    let mut c = Cv::new(64, 19);
    c.rect(0.0, 0.0, 64.0, 4.0, 25); c.rect(0.0, 4.0, 64.0, 7.0, 19); c.rect(0.0, 15.0, 64.0, 19.0, 20);
    for i in 0..4 { let x = i as f32 * 16.0; c.poly(&[(x, 15.0), (x + 4.0, 15.0), (x + 12.0, 7.0), (x + 8.0, 7.0)], 20); c.poly(&[(x + 8.0, 7.0), (x + 12.0, 7.0), (x + 20.0, 15.0), (x + 16.0, 15.0)], 20); }
    c.done("Girder", 3)
}
fn girder_cap(left: bool) -> Sprite {
    // pillar embedded in the cliff (x 0..24), deck continues to the opposite edge
    let mut c = Cv::new(64, 50);
    c.rect(0.0, 0.0, 64.0, 4.0, 25); c.rect(0.0, 4.0, 64.0, 7.0, 19); c.rect(24.0, 15.0, 64.0, 19.0, 20);
    c.rect(0.0, 0.0, 24.0, 50.0, 19); c.rect(0.0, 0.0, 24.0, 4.0, 25); c.rect(20.0, 4.0, 24.0, 50.0, 20);
    for k in 0..3 { c.rect(2.0, 14.0 + k as f32 * 14.0, 18.0, 17.0 + k as f32 * 14.0, 21); }
    for i in 0..3 { let x = 24.0 + i as f32 * 16.0; c.poly(&[(x, 15.0), (x + 4.0, 15.0), (x + 12.0, 7.0), (x + 8.0, 7.0)], 20); }
    if !left { c.flip_h(); }
    c.done(if left { "Girder cap L" } else { "Girder cap R" }, 3)
}

// ---------------------------------------------------------------------------------------------------- WA block grid + RNG
struct Lcg(u32);
impl Lcg {
    fn next(&mut self) -> u32 { self.0 = self.0.wrapping_mul(0x0019_660D).wrapping_add(0x3C6E_F35F); self.0 & 0xFFFF }
}

struct Grid {
    st: Vec<u8>,   // 0 empty / 1 partial / 2 full (8x8 blocks of the land)
    occ: Vec<bool>, // state 3: taken by an object
}
impl Grid {
    fn get(&self, x: i32, y: i32) -> i8 {
        if x < 0 || y < 0 || x >= GW || y >= GH { return -1; }
        let i = (y * GW + x) as usize;
        if self.occ[i] { 3 } else { self.st[i] as i8 }
    }
    fn take(&mut self, x: i32, y: i32) { if x >= 0 && y >= 0 && x < GW && y < GH { self.occ[(y * GW + x) as usize] = true; } }
}

fn land_y0() -> i32 { WATER_Y as i32 - LAND_H as i32 }

fn build_grid(t: &Terrain) -> Grid {
    let dy = land_y0();
    let mut st = vec![0u8; (GW * GH) as usize];
    for by in 0..GH { for bx in 0..GW {
        let mut n = 0;
        for y in 0..8 { for x in 0..8 { if t.is_solid(bx * 8 + x, dy + by * 8 + y) { n += 1; } } }
        st[(by * GW + bx) as usize] = if n == 0 { 0 } else if n == 64 { 2 } else { 1 };
    } }
    Grid { st, occ: vec![false; (GW * GH) as usize] }
}

/// WA mask (FUN_0057a1d0 + FUN_0057a300). Returns (block w, block h, mask) with mask bytes b'0' (must be empty), b'2' (must be full), b'1' (any).
fn make_mask(s: &Sprite) -> (i32, i32, Vec<u8>) {
    let (bw, bh) = (((s.w + 7) / 8) as i32, ((s.h + 7) / 8) as i32);
    let mut orig = vec![b'0'; (bw * bh) as usize];
    for by in 0..bh { for bx in 0..bw {
        let mut n = 0;
        for y in (by * 8) as usize..((by * 8 + 8) as usize).min(s.h) { for x in (bx * 8) as usize..((bx * 8 + 8) as usize).min(s.w) { if s.pix[y * s.w + x] != 0 { n += 1; } } }
        orig[(by * bw + bx) as usize] = if n == 0 { b'0' } else if n == 64 { b'2' } else { b'1' };
    } }
    let mut mask: Vec<u8> = orig.iter().map(|&o| if o == b'0' { b'1' } else { b'0' }).collect();
    let (lc, iv) = if s.dir == 2 || s.dir == 3 { (bw, bh) } else { (bh, bw) };
    let n1 = ((lc + iv) / 16).max(1);
    let n2 = ((lc + iv) / 8).max(1);
    let cell = |k: i32, j: i32| -> usize {
        let (r, c) = match s.dir { 0 => (j, bw - k - 1), 1 => (j, k), 2 => (bh - k - 1, j), _ => (k, j) };
        (r * bw + c) as usize
    };
    let mut k = iv - 1;
    for _ in 0..n1 {
        if k < 1 { break; }
        for j in 0..lc { let i = cell(k, j); mask[i] = if orig[i] == b'0' { b'1' } else { b'2' }; }
        k -= 1;
    }
    for _ in 0..n2 {
        if k < 1 { break; }
        for j in 0..lc { mask[cell(k, j)] = b'1'; }
        k -= 1;
    }
    (bw, bh, mask)
}

fn fits(g: &Grid, mask: &[u8], bw: i32, bh: i32, x: i32, y: i32) -> bool {
    for j in 0..bh { for i in 0..bw {
        let s = g.get(x + i, y + j);
        if s == 3 || s < 0 { return false; }
        match mask[(j * bw + i) as usize] { b'0' if s != 0 => return false, b'2' if s != 2 => return false, _ => {} }
    } }
    true
}

fn sweep(g: &Grid, mask: &[u8], bw: i32, bh: i32, mut x: i32, mut y: i32, d: u8) -> Option<(i32, i32)> {
    while x >= 0 {
        if y < 0 || x + bw > GW || y + bh > GH { return None; }
        if fits(g, mask, bw, bh, x, y) { return Some((x, y)); }
        match d { 0 => x -= 1, 1 => x += 1, 2 => y -= 1, _ => y += 1 }
    }
    None
}

/// Draw a sprite into the land (solid + colour) with its top-left at land pixel (ox, oy).
fn blit(t: &mut Terrain, s: &Sprite, ox: i32, oy: i32) {
    let dy = land_y0();
    for j in 0..s.h { for i in 0..s.w {
        let c = s.pix[j * s.w + i];
        if c == 0 { continue; }
        let (wx, wy) = (ox + i as i32, dy + oy + j as i32);
        if wx < 0 || wy < 0 || wx >= WORLD_W as i32 || wy >= WATER_Y as i32 { continue; }
        t.set_solid(wx, wy, true);
        t.set_deco(wx, wy, c);
    } }
}

// ---------------------------------------------------------------------------------------------------- bridges (FUN_00578c00 / 00578a80)
/// Left abutment (3 full blocks at x, y+3..5), then n>=4 clear 8x7 block segments, then a full abutment 15 blocks beyond segment n.
/// FUN_005789e0: a V-shaped empty region under the span (width n*8, depth growing to n*4 block rows at mid-span).
/// This is what keeps WA's bridges to deep gaps and stops them stacking tightly.
fn v_clear(g: &Grid, x: i32, y: i32, n: i32) -> bool {
    let w = n * 8;
    for i in 0..w {
        let h = if i < w / 2 { i } else { w - i };
        for j in 0..h { if g.get(x + i + 8, y + j) != 0 { return false; } }
    }
    true
}

fn bridge_span(g: &Grid, x: i32, y: i32) -> Option<i32> {
    if !(g.get(x, y + 3) == 2 && g.get(x, y + 4) == 2 && g.get(x, y + 5) == 2) || x >= GW { return None; }
    let mut n = 0;
    loop {
        n += 1;
        let sx = x + n * 8;
        for i in 0..8 { for j in -3..4 { if g.get(sx + i, y + j) != 0 { return None; } } }
        let ax = x + n * 8 + 15;
        if g.get(ax, y + 3) == 2 && g.get(ax, y + 4) == 2 && g.get(ax, y + 5) == 2 {
            return if n >= 4 && v_clear(g, x, y, n) { Some(n) } else { None };
        }
        if x + n * 8 >= GW { return None; }
    }
}

fn bridges(t: &mut Terrain, g: &mut Grid, rng: &mut Lcg) {
    let (l, m, r) = (girder_cap(true), girder_mid(), girder_cap(false));
    for _ in 0..BRIDGE_COUNT {
        for _try in 0..16 {
            let x = (rng.next() % ((WORLD_W as u32 - 0x40) / 8)) as i32;
            let y = (rng.next() % ((LAND_H as u32 - 0x40) / 8)) as i32;
            let mut found = None;
            for dir in [-1i32, 1] {
                let mut cx = x;
                while cx >= 0 && cx < GW {
                    if let Some(n) = bridge_span(g, cx, y) { found = Some((cx, n)); break; }
                    cx += dir;
                }
                if found.is_some() { break; }
            }
            if let Some((bx, n)) = found {
                blit(t, &l, bx * 8, y * 8);
                for k in 1..=n { blit(t, &m, bx * 8 + 64 * k, y * 8); }
                blit(t, &r, bx * 8 + 64 * (n + 1), y * 8);
                for j in 0..6 { for i in 0..(n * 8 + 16) { g.take(bx + i, y + j); } }
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------- scenery (FUN_00577e70)
/// Place WA-style bridges and scenery into `t` (solid + colour layer). Deterministic from `wa_seed`.
pub fn apply(t: &mut Terrain, wa_seed: u32) {
    let sprites = set_for(Theme::of(t.is_cavern, t.template_id));
    let masks: Vec<(i32, i32, Vec<u8>)> = sprites.iter().map(make_mask).collect();
    let mut g = build_grid(t);
    let mut rng = Lcg(wa_seed ^ 0x5CE4_E5EE);
    bridges(t, &mut g, &mut rng);
    for _ in 0..SCENERY_COUNT {
        let k = (rng.next() as usize) % sprites.len();
        let (bw, bh, ref mask) = masks[k];
        if GW - bw <= 0 || GH - bh <= 0 { continue; }
        for _try in 0..16 {
            let x = (rng.next() % (GW - bw) as u32) as i32;
            let y = (rng.next() % (GH - bh) as u32) as i32;
            let d = sprites[k].dir;
            let got = sweep(&g, mask, bw, bh, x, y, d).or_else(|| sweep(&g, mask, bw, bh, x, y, d ^ 1));
            if let Some((gx, gy)) = got {
                for j in 0..bh { for i in 0..bw { g.take(gx + i, gy + j); } }
                blit(t, &sprites[k], gx * 8, gy * 8);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprite_sizes_match_wa_sets() {
        let p = set_for(Theme::Pastoral);
        assert_eq!((p[0].w, p[0].h), (116, 203));
        let r = set_for(Theme::Rugged);
        assert_eq!((r[6].w, r[6].h), (184, 115));
        let u = set_for(Theme::Underground);
        assert_eq!((u[0].w, u[0].h), (72, 159));
        assert!(u.iter().any(|s| s.dir == 2) && u.iter().any(|s| s.dir == 0) && u.iter().any(|s| s.dir == 1));
    }

    #[test]
    fn every_sprite_has_pixels_and_a_solid_base() {
        for th in [Theme::Pastoral, Theme::Rugged, Theme::Underground] {
            for s in set_for(th) {
                assert!(s.pix.iter().any(|&c| c != 0), "{} empty", s.name);
                let (bw, bh, mask) = make_mask(&s);
                assert!(mask.iter().any(|&m| m == b'2'), "{} has no base blocks ({}x{})", s.name, bw, bh);
            }
        }
    }

    #[test]
    fn placement_is_deterministic_and_objects_are_solid() {
        let a = Terrain::generate_tactical(5);
        let b = Terrain::generate_tactical(5);
        assert_eq!(a.deco, b.deco);
        assert_eq!(a.solid_count(), b.solid_count());
        let n = a.deco.iter().filter(|&&c| c != 0).count();
        assert!(n > 0, "no scenery drawn");
        for (i, &c) in a.deco.iter().enumerate() { if c != 0 { assert!(a.is_solid((i % WORLD_W as usize) as i32, (i / WORLD_W as usize) as i32)); } }
    }

    #[test]
    fn scenery_never_overlaps_and_stays_in_the_land() {
        for seed in 0..30u64 {
            let t = Terrain::generate_tactical(seed);
            let dy = land_y0() as usize;
            let above = t.deco[..dy * WORLD_W as usize].iter().filter(|&&c| c != 0).count();
            assert_eq!(above, 0, "seed {seed}: scenery above the land area");
        }
    }
}
