//! Seed-based collage synthesizer over real Worms Armageddon terrain art.
//!
//! Source masks were extracted offline from the game's own
//! `assets/Worms Armageddon/DATA/land.dat` (see `tools/extract_wa_mask.py`):
//! 1-bit-per-pixel, row-major, MSB-first bitmaps, baked here as static byte
//! arrays so `Terrain::generate_tactical` stays pure/seed-derived with no
//! runtime file I/O (see CLAUDE.md).
//!
//! Instead of sampling one fixed mask per map, every seed *composes a new
//! silhouette*: the map is split into 2–4 horizontal segments, each sourced
//! from a (mask, x-shift, mirror, y-offset, y-scale) tuple, crossfaded at the
//! seams and bent by a low-frequency domain warp. The pixel-level edge
//! character is authentic WA art; the macro shape is novel per seed.
//!
//! Determinism contract: `collage_params` does all its RNG up front from a
//! private LCG stream (seed-derived, independent of terrain.rs's draw order);
//! `collage_density` is a pure function — fixed-order f64 math, no RNG —
//! identical on client and server for a given seed.

use noise::{NoiseFn, OpenSimplex};

pub const WA_MASK_W: u32 = 1920;
pub const WA_MASK_H: u32 = 696;

/// Open/island-style WA silhouettes (terrain surrounded by sky + water).
static WA_ISLAND_MASKS: [&[u8]; 2] = [
    include_bytes!("wa_masks/island0.bin"),
    include_bytes!("wa_masks/island1.bin"),
];

/// Enclosed cavern-style WA silhouettes (solid border, play area carved
/// inside). None extracted yet — until real cavern masks are added via
/// `tools/extract_wa_mask.py --cavern`, cavern maps collage the island art
/// *inverted* (WA land shapes become carved chambers), which keeps the WA
/// edge character. Drop `cavernN.bin` files in `wa_masks/` and list them
/// here to switch over; `collage_params` picks this set automatically once
/// it is non-empty.
static WA_CAVERN_MASKS: [&[u8]; 0] = [];

fn mask_bit(mask: &[u8], x: u32, y: u32) -> bool {
    let row_bytes = WA_MASK_W / 8;
    let byte = mask[(y * row_bytes + x / 8) as usize];
    (byte >> (7 - (x % 8))) & 1 != 0
}

/// One horizontal segment's source: which mask it samples and how.
#[derive(Clone, Copy)]
struct SegmentSource {
    /// Resolved source bitmap (points into the active static mask set).
    data: &'static [u8],
    /// Index of `data` within its set — drives the cosmetic theme.
    mask: u8,
    shift: u32,
    mirror: bool,
    /// Vertical offset in normalized mask space (± ≈0.05).
    y_off: f64,
    /// Vertical scale (0.9–1.1).
    y_scale: f64,
}

const MAX_SEGMENTS: usize = 4;

/// All seed-derived collage parameters, computed once before the pixel loop.
pub struct CollageParams {
    n_segs: usize,
    /// Normalized x boundaries: bounds[0] = 0.0, bounds[n_segs] = 1.0.
    bounds: [f64; MAX_SEGMENTS + 1],
    segs: [SegmentSource; MAX_SEGMENTS],
    /// Crossfade half-width around interior boundaries (normalized x).
    fade: f64,
    /// True when synthesizing a cavern from island art: sample is inverted
    /// (WA land → carved air) so chambers keep the WA silhouette character.
    invert: bool,
    warp_x: OpenSimplex,
    warp_y: OpenSimplex,
    warp_freq: f64,
    warp_amp_x: f64,
    warp_amp_y: f64,
}

fn lcg(s: &mut u64) -> u64 {
    *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *s >> 33
}

fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Build the per-map collage recipe from the seed. `cavern` selects the
/// cavern mask set (falling back to inverted island art while that set is
/// empty). Uses a private LCG stream — adding/removing draws in terrain.rs
/// does not shift these picks and vice versa.
pub fn collage_params(seed: u64, cavern: bool) -> CollageParams {
    let (set, invert): (&'static [&'static [u8]], bool) =
        if cavern && WA_CAVERN_MASKS.is_empty() {
            (&WA_ISLAND_MASKS, true)
        } else if cavern {
            (&WA_CAVERN_MASKS, false)
        } else {
            (&WA_ISLAND_MASKS, false)
        };
    let n_masks = set.len() as u64;

    let mut r = seed ^ 0xC011_A6E5_EEDB_A5E5u64;
    let rnd = |r: &mut u64, lo: f64, span: f64| lo + (lcg(r) & 0xFFFF) as f64 / 65535.0 * span;

    let n_segs = 2 + (lcg(&mut r) % (MAX_SEGMENTS as u64 - 1)) as usize; // 2–4

    // Boundaries: even split, jittered ±8% of map width.
    let mut bounds = [0.0f64; MAX_SEGMENTS + 1];
    for i in 1..n_segs {
        bounds[i] = i as f64 / n_segs as f64 + rnd(&mut r, -0.08, 0.16);
    }
    bounds[n_segs] = 1.0;

    // Per-segment sources. Force adjacent segments onto different masks when
    // the library allows, so seams always splice *different* WA art.
    let mut segs = [SegmentSource {
        data: set[0],
        mask: 0,
        shift: 0,
        mirror: false,
        y_off: 0.0,
        y_scale: 1.0,
    }; MAX_SEGMENTS];
    for i in 0..n_segs {
        let mut mask = (lcg(&mut r) % n_masks) as u8;
        if n_masks > 1 && i > 0 && mask == segs[i - 1].mask {
            mask = (mask + 1) % n_masks as u8;
        }
        // Draw candidate shifts until the segment's window actually contains
        // WA land: a shift can park the window on an all-ocean stretch of the
        // source art, which reads as an empty map slice. Keep the
        // best-covered candidate if none clears the bar (deterministic —
        // bounded loop of LCG draws).
        let lo = bounds[i];
        let hi = bounds[i + 1];
        let mut best = SegmentSource {
            data: set[mask as usize],
            mask,
            shift: 0,
            mirror: false,
            y_off: 0.0,
            y_scale: 1.0,
        };
        let mut best_cov = -1.0f64;
        for _ in 0..8 {
            let cand = SegmentSource {
                data: set[mask as usize],
                mask,
                shift: (lcg(&mut r) % WA_MASK_W as u64) as u32,
                mirror: lcg(&mut r) & 1 == 1,
                y_off: rnd(&mut r, -0.05, 0.10),
                y_scale: rnd(&mut r, 0.90, 0.20),
            };
            let cov = segment_coverage(&cand, lo, hi);
            if cov > best_cov {
                best_cov = cov;
                best = cand;
            }
            if cov >= 0.22 {
                break;
            }
        }
        segs[i] = best;
    }

    // Crossfade half-width: ~30–60px of the 1920px mask space per side
    // (total blend window 60–120px).
    let fade = rnd(&mut r, 0.016, 0.016);

    CollageParams {
        n_segs,
        bounds,
        segs,
        fade,
        invert,
        warp_x: OpenSimplex::new(seed.wrapping_add(8000) as u32),
        warp_y: OpenSimplex::new(seed.wrapping_add(9000) as u32),
        warp_freq: rnd(&mut r, 5.0, 3.0),
        // Bend amplitude ~10–25px, expressed in normalized map units.
        warp_amp_x: rnd(&mut r, 10.0, 15.0) / WA_MASK_W as f64,
        warp_amp_y: rnd(&mut r, 10.0, 15.0) / WA_MASK_H as f64,
    }
}

/// Mask index of the widest segment — drives the map's cosmetic theme
/// (`Terrain.template_id`), so the dominant source art picks the look.
pub fn dominant_template_id(p: &CollageParams) -> u8 {
    let mut best = 0usize;
    let mut best_w = -1.0f64;
    for i in 0..p.n_segs {
        let w = p.bounds[i + 1] - p.bounds[i];
        if w > best_w {
            best_w = w;
            best = i;
        }
    }
    p.segs[best].mask
}

fn sample_segment(seg: &SegmentSource, nx: f64, ny: f64) -> f64 {
    let my_f = (ny * seg.y_scale + seg.y_off).clamp(0.0, 1.0 - 1e-9);
    let mut mx = ((nx.rem_euclid(1.0) * WA_MASK_W as f64) as u32 + seg.shift) % WA_MASK_W;
    if seg.mirror {
        mx = WA_MASK_W - 1 - mx;
    }
    let my = ((my_f * WA_MASK_H as f64) as u32).min(WA_MASK_H - 1);
    if mask_bit(seg.data, mx, my) { 1.0 } else { 0.0 }
}

/// Fraction of a segment's sampled window that is WA land, on a coarse grid.
/// Used by `collage_params` to reject shifts that land on empty stretches.
fn segment_coverage(seg: &SegmentSource, lo: f64, hi: f64) -> f64 {
    const SX: usize = 24;
    const SY: usize = 16;
    let mut solid = 0usize;
    for i in 0..SX {
        let nx = lo + (hi - lo) * (i as f64 + 0.5) / SX as f64;
        for j in 0..SY {
            let ny = (j as f64 + 0.5) / SY as f64;
            if sample_segment(seg, nx, ny) > 0.5 {
                solid += 1;
            }
        }
    }
    solid as f64 / (SX * SY) as f64
}

/// Sample the collaged WA silhouette at normalized `(nx, ny)` in
/// `[0,1)×[0,1)`. Continuous in the crossfade windows (0.0–1.0), binary
/// elsewhere; the caller's box blur + threshold smooths the rest. Pure — no
/// RNG, fixed-order f64 math.
pub fn collage_density(p: &CollageParams, nx: f64, ny: f64) -> f64 {
    // Low-frequency domain warp bends the spliced art so seams and repeats
    // don't read as copies.
    let wx = p.warp_x.get([nx * p.warp_freq, ny * p.warp_freq]) * p.warp_amp_x;
    let wy = p.warp_y.get([nx * p.warp_freq + 7.7, ny * p.warp_freq]) * p.warp_amp_y;
    let nx = (nx + wx).rem_euclid(1.0);
    let ny = (ny + wy).clamp(0.0, 1.0 - 1e-9);

    // Weight-blend the (at most two) segments whose fade window covers nx.
    let mut num = 0.0f64;
    let mut den = 0.0f64;
    for i in 0..p.n_segs {
        let lo = p.bounds[i];
        let hi = p.bounds[i + 1];
        let wl = if i == 0 { 1.0 } else { smoothstep((nx - (lo - p.fade)) / (2.0 * p.fade)) };
        let wr = if i == p.n_segs - 1 {
            1.0
        } else {
            smoothstep(((hi + p.fade) - nx) / (2.0 * p.fade))
        };
        let w = wl.min(wr);
        if w > 0.0 {
            num += w * sample_segment(&p.segs[i], nx, ny);
            den += w;
        }
    }
    let d = if den > 0.0 { num / den } else { 0.0 };
    if p.invert { 1.0 - d } else { d }
}
