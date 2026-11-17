//! Port of Worms Armageddon's built-in terrain generator ("landgen", WA.exe
//! 0x5775b0 / 0x5770f0 / 0x576e90 / 0x5765b0), bit-exact in 16.16 fixed point.
//!
//! The generator takes one of 8 fixed control polygons (4 island, 4 cavern),
//! recursively midpoint-displaces every edge with WA's LCG, and fills the
//! resulting uniform cubic B-spline with an even-odd column-toggle fill.
//! Verified knot-for-knot against 41 maps captured from the real game
//! (`tests/data/wa_landgen/*.svg`, see `findings/mapgen.md` in the decomp
//! workspace).

const M_W: usize = 1920;
/// WA's collision/raster grid height (islands print an SVG height of 695, but
/// the grid is 696 for both kinds).
const M_H: usize = 696;

static T1: &[u8; 0x4000] = include_bytes!("sqrt_t1.bin");
static T2: &[u8; 0x4000] = include_bytes!("sqrt_t2.bin");

const ISLAND: [&[(i32, i32)]; 4] = [
    &[(120, 695), (240, 261), (1680, 261), (1800, 695)],
    &[
        (120, 695), (240, 261), (600, 261), (720, 696),
        (1200, 696), (1320, 261), (1680, 261), (1800, 695),
    ],
    &[(120, 695), (240, 522), (1680, 522), (1800, 695)],
    &[(120, 695), (240, 174), (960, 739), (1680, 174), (1800, 695)],
];
const CAVERN: [&[(i32, i32)]; 4] = [
    &[(120, 43), (1800, 43), (1800, 565), (120, 565)],
    &[
        (-120, 43), (2040, 43), (2040, 261), (-120, 261),
        (-120, 478), (2040, 478), (2040, 652), (-120, 652),
    ],
    &[(120, 43), (1800, 43), (1440, 696), (360, 696)],
    &[(-120, 43), (2040, 43), (2040, 652), (-120, 652)],
];

/// WA's game LCG.
#[inline]
pub fn lcg(s: u32) -> u32 {
    s.wrapping_mul(0x0019_660d).wrapping_add(0x3c6e_f35f)
}

#[inline]
fn mul(a: i32, b: i32) -> i32 {
    ((a as i64 * b as i64) >> 16) as i32
}
#[inline]
fn fdiv(a: i32, b: i32) -> i32 {
    (((a as i64) << 16) / b as i64) as i32
}

/// FUN_005b3581: table-driven integer sqrt of a 64-bit value (WA's exact
/// approximation, not a true sqrt).
fn sqrt64(v: u64) -> u32 {
    let lo = v as u32;
    let hi = (v >> 32) as u32;
    let bsr = |x: u32| 31 - x.leading_zeros() as i32;
    if hi == 0 {
        if lo == 0 {
            return 0;
        }
        let c = bsr(lo) - 14;
        if c >= 0 {
            let e = ((lo >> c) & 0x3fff) as usize;
            let t = if c & 1 != 0 { T2 } else { T1 };
            (t[e] as u32) << (c >> 1)
        } else {
            let c = -c;
            let e = ((lo << c) & 0x3fff) as usize;
            let c = c + 1;
            let t = if c & 1 != 0 { T1 } else { T2 };
            (t[e] as u32) >> (c >> 1)
        }
    } else {
        let c = bsr(hi) - 14;
        if c >= 0 {
            let e = ((hi >> c) & 0x3fff) as usize;
            let t = if c & 1 != 0 { T2 } else { T1 };
            ((t[e] as u32) << 16) << (c >> 1)
        } else {
            let c = -c;
            let e = (((hi << c) | (lo >> (32 - c))) & 0x3fff) as usize;
            let c = c + 1;
            let t = if c & 1 != 0 { T1 } else { T2 };
            ((t[e] as u32) << 16) >> (c >> 1)
        }
    }
}

fn hypot(a: i32, b: i32) -> i32 {
    let s = (a as i64 * a as i64 + b as i64 * b as i64) as u64;
    sqrt64(s) as i32
}

struct Gen {
    grid: Vec<u8>,
    pts: Vec<(i32, i32)>,
    c0: i32,
    c1: i32,
    c2: i32,
    c3: i32,
}

impl Gen {
    fn cell(&self, x: i32, y: i32) -> Option<usize> {
        if x >= 0 && y >= 0 && (x as usize) < M_W && (y as usize) < M_H {
            Some(y as usize * M_W + x as usize)
        } else {
            None
        }
    }

    /// FUN_005763e0: raster a 24.8 segment into the grid (`draw`) or count
    /// occupied cells along it (`!draw`).
    fn line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32, draw: bool, val: u8) -> i32 {
        let (x1, y1, x2, y2) = (x1 >> 8, y1 >> 8, x2 >> 8, y2 >> 8);
        if (x2 - x1).abs() < (y2 - y1).abs() {
            self.line_axis(y1, x1, y2, x2, draw, val, true)
        } else {
            self.line_axis(x1, y1, x2, y2, draw, val, false)
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn line_axis(&mut self, a1: i32, b1: i32, a2: i32, b2: i32, draw: bool, val: u8, swap: bool) -> i32 {
        let i0 = (a1 + 0x7f) >> 8;
        let i1 = (a2 + 0x7f) >> 8;
        if i0 == i1 {
            return 0;
        }
        let sl = b2.wrapping_mul(256).wrapping_sub(b1.wrapping_mul(256)) / (a2 - a1);
        let y0 = b1
            .wrapping_mul(256)
            .wrapping_add((i0 * 256 - a1 + 0x80).wrapping_mul(sl));
        let n = (i1 - i0).abs();
        let st = if i1 - i0 >= 0 { 1 } else { -1 };
        let s2 = ((i1 * 256 - a2 + 0x80)
            .wrapping_mul(sl)
            .wrapping_sub(y0)
            .wrapping_add(b2.wrapping_mul(256)))
            / n;
        let mut b = y0.wrapping_add(0x8000);
        let mut i = i0;
        let pos = |i: i32, bb: i32| if swap { (bb, i) } else { (i, bb) };
        if draw {
            for _ in 0..n {
                let (x, y) = pos(i, b >> 16);
                if let Some(k) = self.cell(x, y) {
                    self.grid[k] = val;
                }
                b = b.wrapping_add(s2);
                i += st;
            }
            return 0;
        }
        let mut tot = 0;
        let mut cur = b >> 16;
        for _ in 0..n {
            let (x, y) = pos(i, cur);
            if let Some(k) = self.cell(x, y) {
                tot += self.grid[k] as i32;
            }
            b = b.wrapping_add(s2);
            let nb = b >> 16;
            if nb != cur {
                let (x, y) = pos(i, nb);
                if let Some(k) = self.cell(x, y) {
                    tot += self.grid[k] as i32;
                }
            }
            i += st;
            cur = nb;
        }
        tot
    }

    /// FUN_00576e90: recursive midpoint displacement.
    #[allow(clippy::too_many_arguments)]
    fn disp(&mut self, mut x1: i32, mut y1: i32, x2: i32, y2: i32, minseg: i32, mut rng: u32) {
        let (w, h) = (M_W as i32, M_H as i32);
        loop {
            let dx = x2.wrapping_sub(x1);
            let dy = y2.wrapping_sub(y1);
            let l = hypot(dx, dy);
            let (ux, uy) = if l != 0 { (fdiv(dx, l), fdiv(dy, l)) } else { (dx, dy) };
            if (l >> 16) < minseg {
                self.pts.push((x1, y1));
                return;
            }
            self.line(x1, y1, x2, y2, true, 0);
            let mx = x1.wrapping_add(x2) / 2;
            let my = y1.wrapping_add(y2) / 2;
            let (mut nx, mut ny) = (mx, my);
            if (mx >> 16) > 15 && (my >> 16) > 15 && (mx >> 16) < w - 16 && (my >> 16) < h - 16 {
                let t = (l / w).clamp(0, 0x10000);
                let f = mul(self.c1 - self.c0, t);
                let amp = mul(f + self.c0, l);
                let r = (rng & 0xffff) as i32 - 0x8000;
                let d = mul(r, amp);
                let tx = mx - mul(mul(uy, d), self.c2);
                let ty = my + mul(mul(ux, d), self.c3);
                if (tx >> 16) > 15
                    && (ty >> 16) > 15
                    && (tx >> 16) < w - 16
                    && (ty >> 16) < h - 16
                    && self.line(x1, y1, tx, ty, false, 0) == 0
                    && self.line(tx, ty, x2, y2, false, 0) == 0
                {
                    nx = tx;
                    ny = ty;
                }
            }
            self.line(x1, y1, nx, ny, true, 1);
            self.line(nx, ny, x2, y2, true, 1);
            self.disp(x1, y1, nx, ny, minseg, lcg(rng));
            rng = rng.wrapping_mul(0x0011_c4ed).wrapping_add(0xb3a4_875f);
            x1 = nx;
            y1 = ny;
        }
    }
}

/// B-spline control points (16.16) WA builds for a map. `seed` is the 31-bit
/// map seed (the number printed in WA's `landgen.svg` header).
pub fn control_points(seed: u32, cavern: bool, style: usize) -> Vec<(i32, i32)> {
    let r1 = lcg(seed);
    let r2 = lcg(r1);
    let mut g = Gen {
        grid: vec![0u8; M_W * M_H],
        pts: Vec::new(),
        c0: 0x28000,
        c1: 0x28000,
        c2: (r1 & 0xffff) as i32 + 0x10000,
        c3: ((r2 >> 1) & 0x7fff) as i32 + 0x10000,
    };
    let mut rng = r2;
    let poly: Vec<(i32, i32)> = (if cavern { CAVERN[style] } else { ISLAND[style] })
        .iter()
        .map(|&(x, y)| (x << 16, y << 16))
        .collect();
    let n = poly.len();
    if !cavern {
        g.pts = vec![poly[0]; 3];
        for i in 1..n {
            g.line(poly[i - 1].0, poly[i - 1].1, poly[i].0, poly[i].1, true, 1);
        }
        for i in 1..n {
            rng = lcg(rng);
            g.disp(poly[i - 1].0, poly[i - 1].1, poly[i].0, poly[i].1, 0x40, rng);
        }
        let last = poly[n - 1];
        g.pts.extend_from_slice(&[last; 4]);
    } else {
        for i in 0..n {
            let p = poly[(i + n - 1) % n];
            g.line(p.0, p.1, poly[i].0, poly[i].1, true, 1);
        }
        for i in 0..n {
            rng = lcg(rng);
            let p = poly[(i + n - 1) % n];
            g.disp(p.0, p.1, poly[i].0, poly[i].1, 0x40, rng);
        }
        let head: Vec<_> = g.pts[0..4].to_vec();
        g.pts.extend(head);
    }
    g.pts
}

/// SVG knot (in px) `i` of the B-spline — what WA writes to `landgen.svg`.
pub fn svg_knots(pts: &[(i32, i32)]) -> Vec<(f64, f64)> {
    let n = pts.len();
    (3..n)
        .map(|i| {
            let (p0, p1, p2) = (pts[(i - 3) % n], pts[(i - 2) % n], pts[(i - 1) % n]);
            (
                (p0.0 as f64 + 4.0 * p1.0 as f64 + p2.0 as f64) / 6.0 / 65536.0,
                (p0.1 as f64 + 4.0 * p1.1 as f64 + p2.1 as f64) / 6.0 / 65536.0,
            )
        })
        .collect()
}

/// FUN_00576090: cubic B-spline point at `t` (16.16) for 4 control points.
fn spline(p: &[(i32, i32); 4], t: i32) -> (i32, i32) {
    let t2 = mul(t, t);
    let t3 = mul(t2, t);
    let w0 = (t2 - t) * 3 - t3 + 0x10000;
    let w1 = (t3 - 2 * t2) * 3 + 0x40000;
    let w2 = ((t2 - t3) + t) * 3 + 0x10000;
    let f = |a: i32, b: i32, c: i32, d: i32| {
        (mul(a, w0).wrapping_add(mul(b, w1)).wrapping_add(mul(c, w2)).wrapping_add(mul(d, t3))) / 6
    };
    (
        f(p[0].0, p[1].0, p[2].0, p[3].0),
        f(p[0].1, p[1].1, p[2].1, p[3].1),
    )
}

/// A generated WA landmass: `land[y * w + x]` (1920 x 696).
pub struct WaLand {
    pub w: usize,
    pub h: usize,
    pub land: Vec<bool>,
}

/// Generate WA's land bitmap for (`seed`, `cavern`, `style` 0..=3).
pub fn generate(seed: u32, cavern: bool, style: usize) -> WaLand {
    let pts = control_points(seed, cavern, style);
    let n = pts.len();
    let mut bm = vec![0u8; M_W * M_H];
    // FUN_005765b0: sample each B-spline segment at t = 0, .2, .4, .6, .8 and
    // toggle one pixel per column along every chord (FUN_00576460).
    let mut prev: Option<(i32, i32)> = None;
    if n as i64 - 4 > 0 {
        let mut i = 3usize;
        loop {
            let win = [pts[(i - 3) % n], pts[(i - 2) % n], pts[(i - 1) % n], pts[i % n]];
            let mut t = 0i32;
            while t < 0x10000 {
                let s = spline(&win, t);
                if let Some(p) = prev {
                    toggle_edge(&mut bm, p, s);
                }
                prev = Some(s);
                t += 0x3333;
            }
            let again = (i as i64 - 2) < n as i64 - 4;
            i += 1;
            if !again {
                break;
            }
        }
    }
    // Column XOR fill (FUN_005b3670): start value = cavern flag.
    let init = cavern as u8;
    for x in 0..M_W {
        let mut v = init;
        for y in 0..M_H {
            let k = y * M_W + x;
            v ^= bm[k];
            bm[k] = v;
        }
    }
    WaLand { w: M_W, h: M_H, land: bm.iter().map(|&b| b != 0).collect() }
}

/// FUN_00576460: XOR-toggle one pixel per column along a 16.16 chord.
fn toggle_edge(bm: &mut [u8], a: (i32, i32), b: (i32, i32)) {
    let (xa, ya, xb, yb) = (a.0 >> 8, a.1 >> 8, b.0 >> 8, b.1 >> 8);
    let (xlo, ylo, xhi, yhi) = if xb < xa { (xb, yb, xa, ya) } else { (xa, ya, xb, yb) };
    let i0 = (xlo + 0x7f) >> 8;
    let i1 = (xhi + 0x7f) >> 8;
    if i0 == i1 {
        return;
    }
    let s = (yhi.wrapping_mul(256).wrapping_sub(ylo.wrapping_mul(256))) / (xhi - xlo);
    let mut y = ylo
        .wrapping_mul(256)
        .wrapping_add((i0 * 256 - xlo + 0x80).wrapping_mul(s))
        .wrapping_add(0x8000);
    let n = i1 - i0;
    let s2 = ((i1 * 256 - xhi + 0x80)
        .wrapping_mul(s)
        .wrapping_sub(y)
        .wrapping_add(yhi.wrapping_mul(256)))
        / n;
    let mut col = i0;
    for _ in 0..n {
        let yy = (y >> 16).max(0);
        if col >= 0 && (col as usize) < M_W && (yy as usize) < M_H {
            bm[yy as usize * M_W + col as usize] ^= 1;
        }
        y = y.wrapping_add(s2);
        col += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(path: &str) -> (bool, usize, u32, Vec<f64>) {
        let t = std::fs::read_to_string(path).unwrap();
        let h = t.find("style=").unwrap();
        let rest = &t[h + 6..];
        let cavern = rest.starts_with("cavern");
        let style: usize = rest[if cavern { 6 } else { 6 }..][..1].parse().unwrap();
        let s = rest.find("seed=0x").unwrap() + 7;
        let seed = u32::from_str_radix(&rest[s..s + 8], 16).unwrap();
        let d0 = t.find(" d=\"").unwrap();
        let d = &t[d0..];
        let nums: Vec<f64> = d
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter(|s| s.contains('.'))
            .filter_map(|s| s.parse().ok())
            .collect();
        (cavern, style, seed, nums)
    }

    #[test]
    fn matches_real_wa_maps() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/wa_landgen");
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            let (cav, style, seed, nums) = parse(p.to_str().unwrap());
            let knots = svg_knots(&control_points(seed, cav, style));
            let want: Vec<(f64, f64)> = (0..nums.len() / 6).map(|j| (nums[6 * j], nums[6 * j + 1])).collect();
            assert!(knots.len() >= want.len(), "{p:?}: too few knots {} < {}", knots.len(), want.len());
            for (j, (a, b)) in want.iter().zip(&knots).enumerate() {
                assert!(
                    (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01,
                    "{p:?}: knot {j} differs: want {a:?} got {b:?}"
                );
            }
            n += 1;
        }
        assert!(n >= 6);
    }

    #[test]
    fn land_is_nonempty_and_sane() {
        for (cav, st) in [(false, 0), (false, 3), (true, 0), (true, 2)] {
            let m = generate(0x5FB9_3810, cav, st);
            let c = m.land.iter().filter(|&&b| b).count();
            let frac = c as f64 / m.land.len() as f64;
            assert!(frac > 0.05 && frac < 0.95, "cavern={cav} style={st} land fraction {frac}");
        }
    }
}

#[cfg(test)]
mod dump {
    /// `WA_DUMP=<dir> cargo test --lib wa_mapgen::dump -- --ignored` writes PBMs.
    #[test]
    #[ignore]
    fn dump_pbm() {
        let Ok(dir) = std::env::var("WA_DUMP") else { return };
        for (name, cav, st, seed) in [("i0", false, 0, 0x7CD84F8Fu32), ("i1", false, 1, 0x5FB93810), ("i3", false, 3, 0x7D63EEE8), ("c0", true, 0, 0x7F2C7815), ("c2", true, 2, 0x7CF68201), ("c1", true, 1, 0x5F9DAB07)] {
            let m = super::generate(seed, cav, st);
            let mut out = format!("P1\n{} {}\n", m.w, m.h).into_bytes();
            for y in 0..m.h {
                for x in 0..m.w {
                    out.push(if m.land[y * m.w + x] { b'1' } else { b'0' });
                    out.push(b' ');
                }
                out.push(b'\n');
            }
            std::fs::write(format!("{dir}/{name}.pbm"), out).unwrap();
        }
    }
}

#[cfg(test)]
mod integration {
    use crate::world::{Terrain, WORLD_W};

    #[test]
    fn wa_terrain_has_spawns_on_both_halves() {
        let mut caverns = 0;
        for seed in 1..=24u64 {
            let mut t = Terrain::generate_tactical(seed * 7919);
            caverns += t.is_cavern as u32;
            let l = t.find_team_spawns(0, WORLD_W / 2, 4);
            let r = t.find_team_spawns(WORLD_W / 2, WORLD_W, 4);
            assert!(l.len() >= 3 && r.len() >= 3, "seed {seed}: spawns {} / {}", l.len(), r.len());
        }
        assert!(caverns > 0 && caverns < 24, "style mix should include both kinds ({caverns})");
    }

    #[test]
    fn wa_spawns_are_standable_and_enough() {
        let mut short = 0;
        for seed in 1..=24u64 {
            let mut t = Terrain::generate_tactical(seed * 104729);
            let sp = t.find_team_spawns(0, WORLD_W, 8);
            assert_eq!(sp.len(), 8, "seed {seed}");
            let mut wa_hits = 0;
            for p in &sp {
                let (x, y) = (p.x as i32, p.y as i32);
                if t.is_solid(x, y + 1) && !t.is_solid(x, y - 20) { wa_hits += 1; }
            }
            if wa_hits < 6 { short += 1; }
        }
        assert!(short <= 2, "too many maps with unstandable spawns: {short}");
    }

    #[test]
    fn wa_seed_identity_below_2_pow_31() {
        // A seed printed by WA reproduces WA's exact shape.
        let t = Terrain::generate_tactical(0x5FB9_3810);
        let wa = super::generate(0x5FB9_3810, t.is_cavern, t.template_id as usize);
        let dy = crate::world::WATER_Y as i32 - wa.h as i32;
        // Scenery only adds solid pixels, so every WA land pixel must be solid.
        for y in 0..wa.h {
            let wy = y as i32 + dy;
            for x in 0..wa.w {
                if wa.land[y * wa.w + x] && wy < crate::world::WATER_Y as i32 {
                    assert!(t.is_solid(x as i32, wy), "land pixel ({x},{y}) missing");
                }
            }
        }
    }
}

/// WA's worm start placement (FUN_00578570 / FUN_005780b0 / FUN_00577fa0 /
/// FUN_00578030), on a land mask given in WA's 1920x696 frame. Returns up to
/// `max` `(x, ground_y)` positions sorted by x (FUN_00578520). The position
/// WA stores is (x+20, y+20); the ground it stands over is y+60.
/// Scenery placement of WA's own object art is not ported, so the 8x8-block
/// occupancy grid is built from whatever `solid` reports (land + Arty scenery).
/// Fewer than `max` results means WA's random-platform fallback would have
/// fired; callers handle that.
pub fn spawn_positions(
    solid: &dyn Fn(i32, i32) -> bool,
    wa_seed: u32,
    x_lo: i32,
    x_hi: i32,
    max: usize,
) -> Vec<(i32, i32)> {
    let (w, h) = (M_W as i32, M_H as i32);
    let (gw, gh) = ((w / 8) as usize, (h / 8) as usize);
    let mut grid = vec![0u8; gw * gh];
    for by in 0..gh {
        for bx in 0..gw {
            let mut any = false;
            'b: for dy in 0..8 {
                for dx in 0..8 {
                    if solid(bx as i32 * 8 + dx, by as i32 * 8 + dy) {
                        any = true;
                        break 'b;
                    }
                }
            }
            if any {
                grid[by * gw + bx] = 1;
            }
        }
    }
    let lo = x_lo.max(0x14);
    let hi = x_hi.min(w - 0x3c);
    let span = hi - lo;
    if span <= 0 {
        return Vec::new();
    }
    let wl = h; // water line at the bitmap bottom (water level 0%)
    let y0 = (wl / 2).min(h - 1);
    let ymax = (wl - 0x3c).min(h - 1);
    let free = |grid: &Vec<u8>, x: i32, y: i32| -> bool {
        // FUN_00577fa0: 47px square starting at (x, y) must hold no occupied 8x8 block.
        for by in (y / 8)..((y + 0x2f) / 8) {
            for bx in (x / 8)..((x + 0x2f) / 8) {
                if bx >= 0 && by >= 0 && (bx as usize) < gw && (by as usize) < gh && grid[by as usize * gw + bx as usize] != 0 {
                    return false;
                }
            }
        }
        true
    };
    let mut rng = lcg(wa_seed ^ 0x6A09_E667);
    let mut x = (rng >> 16) as i32;
    rng = lcg(rng);
    let _ = rng;
    let mut out: Vec<(i32, i32)> = Vec::new();
    while out.len() < max {
        let mut found = None;
        'attempt: for _ in 0..500 {
            x += w / 0x46;
            x = (x - lo).rem_euclid(span) + lo;
            let col = x + 0x14;
            let mut y = y0;
            while y < ymax {
                if solid(col, y + 0x3c) && free(&grid, x, y) {
                    found = Some(y);
                    break 'attempt;
                }
                y += 1;
            }
            let mut y = y0;
            while y > 0x14 {
                if solid(col, y + 0x3c) && free(&grid, x, y) {
                    found = Some(y);
                    break 'attempt;
                }
                y -= 1;
            }
        }
        let Some(y) = found else { break };
        // FUN_00578030: 107px exclusion square.
        for by in (y / 8)..((y + 0x6b) / 8) {
            for bx in (x / 8)..((x + 0x6b) / 8) {
                if bx >= 0 && by >= 0 && (bx as usize) < gw && (by as usize) < gh {
                    grid[by as usize * gw + bx as usize] = 3;
                }
            }
        }
        // Stand on the true top of the surface found: walk up to the first air
        // pixel above this land column, return that air pixel as the foot.
        let col = x + 0x14;
        let mut gy = y + 0x3c;
        while gy > 0 && solid(col, gy - 1) {
            gy -= 1;
        }
        out.push((col, gy - 1));
    }
    out.sort_by_key(|p| p.0);
    out
}
