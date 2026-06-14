//! Worms-style atmospheric background layers, drawn each frame *behind* the
//! terrain (only on sky pixels) between the world-cache viewport copy and the
//! water ripple. Two layers:
//!
//!   1. `draw_backdrop` — a soft sun glow + two parallax distant-hill ridges that
//!      scroll slower than the world, giving depth.
//!   2. `update_debris` / `draw_debris` — wind-driven ambient motes (snow / dust /
//!      embers / pollen) chosen per map archetype: the "alive air".
//!
//! All layers are client-only visuals (not networked) and are clipped to the
//! visible viewport. Anything that lands on solid terrain or below the waterline
//! is skipped, so the layers read as a true background.

use crate::world::{Terrain, WATER_Y, WORLD_W, SCREEN_W, SCREEN_H};
use super::buffer::WorldBuffer;
use super::fb::Bgra;
use noise::{NoiseFn, OpenSimplex};
use std::f32::consts::{PI, TAU};

// ── Parallax backdrop ─────────────────────────────────────────────────────────

/// Parallax factors (< 1.0 = scrolls slower than the world → reads as distant).
const PAR_FAR:  f32 = 0.25;
const PAR_NEAR: f32 = 0.45;
const PAR_SUN:  f32 = 0.12;
/// Drifting clouds sit furthest back (slowest); seed-generated landforms sit
/// closest (just behind the playable terrain, so it always occludes them).
const PAR_CLOUD: f32 = 0.15;
const PAR_LAND:  f32 = 0.65;

/// Smooth distant ridgeline from layered sines of the (parallax) world x.
fn ridge_y(wx: f32, base: f32, amp: f32, phase: f32) -> f32 {
    base
        + (wx * 0.0035 + phase).sin() * amp
        + (wx * 0.0090 + phase * 1.7).sin() * amp * 0.40
        + (wx * 0.0190 + phase * 2.3).sin() * amp * 0.18
}

/// Hazy silhouette colour for a ridge, tinted slightly by archetype.
/// `far` ridges are lighter (more atmospheric haze) than near ones.
fn ridge_colour(archetype: u8, far: bool) -> Bgra {
    let (mut r, mut g, mut b) = if far { (112, 126, 152) } else { (82, 96, 122) };
    match archetype {
        3 => { r = (r as i32 - 25).max(0) as u8; g = (g as i32 - 25).max(0) as u8; b = (b as i32 - 20).max(0) as u8; } // caverns: darker
        4 => { r = (r as i32 + 28).min(255) as u8; g = (g as i32 + 8).min(255) as u8; } // canyon: warmer
        1 => { r = (r as i32 + 14).min(255) as u8; g = (g as i32 + 14).min(255) as u8; b = (b as i32 + 16).min(255) as u8; } // cliffs: paler
        _ => {}
    }
    Bgra::new(r, g, b)
}

/// Draw the sun glow + two parallax hill ridges into the visible viewport, only on
/// sky pixels (skips terrain and water so the hills sit behind the landscape).
pub fn draw_backdrop(buf: &mut WorldBuffer, terrain: &Terrain, cam_x: u32) {
    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
    let water_y = WATER_Y as i32;

    // ── Sun glow: soft additive disc, slow horizontal parallax ──
    let sun_sx = 130.0 - cam_x as f32 * PAR_SUN;
    let sun_sy = 70.0_f32;
    let sun_r = 46.0_f32;
    if sun_sx > -sun_r && sun_sx < SCREEN_W as f32 + sun_r {
        let cx = sun_sx as i32;
        let cy = sun_sy as i32;
        let ri = sun_r as i32;
        for sy in (cy - ri).max(0)..(cy + ri).min(water_y) {
            for sx in (cx - ri).max(0)..(cx + ri).min(SCREEN_W as i32) {
                let dx = (sx - cx) as f32;
                let dy = (sy - cy) as f32;
                let d = (dx * dx + dy * dy).sqrt();
                if d >= sun_r { continue; }
                let wx = cam_x as i32 + sx;
                if terrain.is_solid(wx, sy) { continue; }
                let f = 1.0 - d / sun_r;       // 1 at centre → 0 at edge
                let add = (f * f * 70.0) as u16; // soft falloff
                let c = buf.get_pixel(wx, sy);
                buf.set_pixel(wx, sy, Bgra::new(
                    (c.r as u16 + add).min(255) as u8,
                    (c.g as u16 + add).min(255) as u8,
                    (c.b as u16 + (add / 2)).min(255) as u8, // warmer (less blue)
                ));
            }
        }
    }

    // ── Two hill ridges, far then near (near drawn over far) ──
    let far_col  = ridge_colour(terrain.archetype, true);
    let near_col = ridge_colour(terrain.archetype, false);
    for &(par, base, amp, phase, col) in &[
        (PAR_FAR,  water_y as f32 - 70.0,  34.0, 0.0, far_col),
        (PAR_NEAR, water_y as f32 - 42.0,  46.0, 2.1, near_col),
    ] {
        for sx in 0..SCREEN_W as i32 {
            let sample_x = cam_x as f32 * par + sx as f32;
            let top = ridge_y(sample_x, base, amp, phase) as i32;
            if top >= water_y { continue; }
            let wx = cam_x as i32 + sx;
            for sy in top.max(0)..water_y {
                if terrain.is_solid(wx, sy) { continue; }
                buf.set_pixel(wx, sy, col);
            }
        }
    }
}

// ── Wind gusts (make the wind visible) ───────────────────────────────────────

/// The map wind is fixed for a whole turn, so we synthesise gusts as a purely
/// visual modulation of it: a steady "breathing" plus occasional stronger gusts.
/// Returned value drives the ambient debris drift and the cloud scroll so every
/// background system breathes together. Magnitude can briefly exceed `base`.
pub fn gust_wind(base: f32, tick: u32) -> f32 {
    let t = tick as f32;
    let breath = 1.0 + 0.35 * (t * 0.030).sin() + 0.25 * (t * 0.011).sin();
    // Slow envelope that occasionally crests → a short gust burst.
    let env = (t * 0.006).sin();
    let gust = if env > 0.7 { (env - 0.7) / 0.3 * 0.8 } else { 0.0 };
    base * (breath + gust)
}

// ── Drifting clouds ──────────────────────────────────────────────────────────

/// One soft cloud blob, in world-x (parallax) space + sky-y.
pub struct Cloud {
    x: f32,    // cloud-layer x (wraps over WORLD_W); screen x = x - cam_x*PAR_CLOUD
    y: f32,    // sky y (top band)
    rx: f32,   // horizontal radius
    ry: f32,   // vertical radius
    soft: f32, // peak brightness add (0..~70)
}

/// Tint for the cloud's additive glow, by archetype.
fn cloud_tint(archetype: u8) -> (u16, u16, u16) {
    match archetype {
        1 => (70, 72, 76), // cliffs/snow: bright cool white
        2 => (58, 64, 70), // islands: hazy
        3 => (20, 18, 22), // caverns: faint murk
        4 => (66, 56, 44), // canyon: warm dust haze
        _ => (60, 62, 64), // hills: soft white
    }
}

fn cloud_count(archetype: u8) -> usize {
    match archetype { 3 => 2, 2 => 6, _ => 5 }
}

fn rand_cloud(state: &mut u32, archetype: u8) -> Cloud {
    let x = rand_f(state) * WORLD_W as f32;
    let y = 24.0 + rand_f(state) * 96.0;
    let rx = 26.0 + rand_f(state) * 40.0;
    let ry = rx * (0.32 + rand_f(state) * 0.18);
    let (tr, _, _) = cloud_tint(archetype);
    Cloud { x, y, rx, ry, soft: (tr as f32) * (0.7 + rand_f(state) * 0.5) }
}

/// Advance clouds: drift with the (gusting) wind, wrap around the world edges.
pub fn update_clouds(clouds: &mut Vec<Cloud>, terrain: &Terrain, gust: f32, tick: u32) {
    let target = cloud_count(terrain.archetype);
    let mut state = tick.wrapping_mul(2246822519).wrapping_add(0x9E3779B9) | 1;

    for c in clouds.iter_mut() {
        c.x += gust * 0.35 + 0.04;          // always creep a touch, even in calm
        let span = WORLD_W as f32 + 200.0;
        if c.x > WORLD_W as f32 + 100.0 { c.x -= span; }
        if c.x < -100.0 { c.x += span; }
    }
    while clouds.len() < target { clouds.push(rand_cloud(&mut state, terrain.archetype)); }
    clouds.truncate(target);
}

/// Draw clouds as soft additive blobs on sky pixels only (behind everything).
pub fn draw_clouds(buf: &mut WorldBuffer, terrain: &Terrain, clouds: &[Cloud], cam_x: u32) {
    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
    let water_y = WATER_Y as i32;
    let (tr, tg, tb) = cloud_tint(terrain.archetype);

    for c in clouds {
        let sx0 = c.x - cam_x as f32 * PAR_CLOUD;     // screen x of centre
        if sx0 < -c.rx || sx0 > SCREEN_W as f32 + c.rx { continue; }
        let cx = sx0 as i32;
        let cy = c.y as i32;
        let rx = c.rx as i32;
        let ry = c.ry as i32;
        for sy in (cy - ry).max(0)..(cy + ry).min(water_y) {
            for sx in (cx - rx).max(0)..(cx + rx).min(SCREEN_W as i32) {
                let dx = (sx - cx) as f32 / c.rx;
                let dy = (sy - cy) as f32 / c.ry;
                let d = (dx * dx + dy * dy).sqrt();
                if d >= 1.0 { continue; }
                let wx = cam_x as i32 + sx;
                if terrain.is_solid(wx, sy) { continue; }
                let f = 1.0 - d;
                let k = f * f * c.soft / tr.max(1) as f32; // 0..1 falloff weight
                let col = buf.get_pixel(wx, sy);
                buf.set_pixel(wx, sy, Bgra::new(
                    (col.r as u16 + (tr as f32 * k) as u16).min(255) as u8,
                    (col.g as u16 + (tg as f32 * k) as u16).min(255) as u8,
                    (col.b as u16 + (tb as f32 * k) as u16).min(255) as u8,
                ));
            }
        }
    }
}

// ── Seed-generated mid-ground landform silhouette ────────────────────────────

/// Sentinel column height meaning "no landform here" (a sky gap — used by islands).
const LAND_GAP: u16 = u16::MAX;

/// Hazy silhouette colour for the mid-ground landform — closer than the parallax
/// hills, so darker and more saturated than `ridge_colour(.., false)`.
fn landform_colour(archetype: u8) -> Bgra {
    let (mut r, mut g, mut b) = (66u32, 78, 100);
    match archetype {
        3 => { r = 34; g = 32; b = 38; }            // caverns: dark massif
        4 => { r = 96; g = 70; b = 50; }            // canyon: warm mesa
        1 => { r = 78; g = 86; b = 104; }           // cliffs: cool stone
        2 => { r = 70; g = 86; b = 102; }           // islands
        _ => {}
    }
    Bgra::new(r as u8, g as u8, b as u8)
}

/// Generate a per-world-column top-y silhouette from the map seed, flavoured by
/// archetype. Deterministic (same seed → same shape); regenerated only on a new
/// match. `LAND_GAP` marks columns with no landform (island sky gaps).
pub fn generate_landform(seed: u64, archetype: u8) -> Vec<u16> {
    use crate::world::{TERRAIN_MIN_Y, TERRAIN_MAX_Y};
    // +8000 offset keeps this distinct from the terrain noise (which uses +3000..+7000).
    let n0 = OpenSimplex::new(seed.wrapping_add(8000) as u32);
    let n1 = OpenSimplex::new(seed.wrapping_add(8100) as u32);

    // Silhouette band: a bit higher than the playable terrain's range so peaks
    // poke up behind valleys, but never above the sky headroom.
    let base_y  = (TERRAIN_MAX_Y as f32 - 30.0).min(WATER_Y as f32 - 24.0); // valley floor
    let top_min = (TERRAIN_MIN_Y as f32 + 30.0).max(120.0);                 // highest peak
    let amp = base_y - top_min;

    let ridged = archetype == 1;
    let scale = match archetype { 1 => 4.2, 4 => 2.6, _ => 3.0 } as f64;

    let mut out = vec![LAND_GAP; WORLD_W as usize];
    for x in 0..WORLD_W as usize {
        let nx = x as f64 / WORLD_W as f64;
        // 3-octave FBM in [0,1]
        let mut val = 0.0; let mut a = 1.0; let mut fr = 1.0; let mut norm = 0.0;
        for _ in 0..3 {
            let s = n0.get([nx * scale * fr, 1.7]) * 0.7 + n1.get([nx * scale * fr * 2.1, 4.3]) * 0.3;
            let s = if ridged { 1.0 - s.abs() } else { (s + 1.0) * 0.5 };
            val += s * a; norm += a; a *= 0.5; fr *= 2.0;
        }
        let mut h = (val / norm) as f32; // 0..1, taller = bigger

        match archetype {
            4 => { // canyon: flat-topped mesas (quantise height into terraces)
                let levels = 5.0;
                h = (h * levels).floor() / levels;
            }
            3 => { // caverns: low, squat massif
                h = h * 0.45 + 0.05;
            }
            2 => { // islands: only the tall humps survive; rest is sky gap
                if h < 0.55 { out[x] = LAND_GAP; continue; }
                h = (h - 0.55) / 0.45;
            }
            _ => {}
        }
        out[x] = (base_y - h * amp).round().clamp(top_min, base_y) as u16;
    }
    out
}

/// Draw the cached landform silhouette behind the playable terrain at PAR_LAND,
/// skipping pixels behind real terrain or below the waterline.
pub fn draw_landform(buf: &mut WorldBuffer, terrain: &Terrain, height: &[u16], cam_x: u32) {
    if height.is_empty() { return; }
    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
    let water_y = WATER_Y as i32;
    let col = landform_colour(terrain.archetype);
    // A slightly lighter rim near the silhouette crest for a touch of relief.
    let rim = Bgra::new(
        (col.r as u16 + 18).min(255) as u8,
        (col.g as u16 + 18).min(255) as u8,
        (col.b as u16 + 20).min(255) as u8,
    );

    for sx in 0..SCREEN_W as i32 {
        let sample = (cam_x as f32 * PAR_LAND + sx as f32) as i32;
        let idx = sample.clamp(0, WORLD_W as i32 - 1) as usize;
        let top = height[idx];
        if top == LAND_GAP { continue; }
        let top = top as i32;
        if top >= water_y { continue; }
        let wx = cam_x as i32 + sx;
        for sy in top.max(0)..water_y {
            if terrain.is_solid(wx, sy) { continue; }
            buf.set_pixel(wx, sy, if sy < top + 3 { rim } else { col });
        }
    }
}

// ── Wind-driven ambient debris ──────────────────────────────────────────────

/// One ambient air particle, in viewport-relative coords (0..SCREEN_W/H).
pub struct BgParticle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    size: u8,
    glow: bool,
    phase: f32, // sway clock
    spin: f32,  // rotation speed (rad/tick)
    rot: f32,   // current rotation (drives leaf/flake flutter shape)
}

struct DebrisStyle {
    colour: Bgra,
    fall: f32,        // base downward speed (px/tick)
    drift: f32,       // wind sensitivity
    count: usize,     // target particle count
    big_chance: u8,   // 0..100 chance of a 2px particle
    glow_chance: u8,  // 0..100 chance of being a glowing ember
    sway_amp: f32,    // lateral sway magnitude (px)
    sway_speed: f32,  // sway clock advance (rad/tick)
    spin: f32,        // max rotation speed for flutter
}

/// Per-archetype debris look: hills→pollen, cliffs→snow, islands→mist,
/// caverns→dust+embers, canyon→dust.
fn debris_style(archetype: u8) -> DebrisStyle {
    match archetype {
        1 => DebrisStyle { colour: Bgra::new(238, 242, 250), fall: 0.55, drift: 1.2, count: 70, big_chance: 30, glow_chance: 0,  sway_amp: 0.9, sway_speed: 0.10, spin: 0.18 }, // snow: flutters
        2 => DebrisStyle { colour: Bgra::new(200, 220, 236), fall: 0.16, drift: 1.5, count: 40, big_chance: 10, glow_chance: 0,  sway_amp: 0.5, sway_speed: 0.05, spin: 0.04 }, // sea mist
        3 => DebrisStyle { colour: Bgra::new(96,  88,  82),  fall: 0.24, drift: 0.7, count: 52, big_chance: 8,  glow_chance: 20, sway_amp: 0.3, sway_speed: 0.06, spin: 0.06 }, // dust + embers
        4 => DebrisStyle { colour: Bgra::new(202, 176, 134), fall: 0.20, drift: 1.0, count: 46, big_chance: 12, glow_chance: 0,  sway_amp: 0.4, sway_speed: 0.07, spin: 0.10 }, // canyon dust
        _ => DebrisStyle { colour: Bgra::new(212, 200, 140), fall: 0.10, drift: 0.9, count: 36, big_chance: 8,  glow_chance: 0,  sway_amp: 0.8, sway_speed: 0.09, spin: 0.14 }, // pollen: drifts in arcs
    }
}

/// Tiny xorshift PRNG — debris is a non-networked visual, so determinism is
/// unnecessary; we just want cheap variety.
#[inline]
fn rng(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}
#[inline]
fn rand_f(state: &mut u32) -> f32 { (rng(state) >> 8) as f32 / (1u32 << 24) as f32 }

/// Spawn one particle. `spread` (first fill) scatters across the whole screen;
/// otherwise particles enter from just above the top edge.
fn spawn(state: &mut u32, style: &DebrisStyle, wind: f32, spread: bool) -> BgParticle {
    let x = rand_f(state) * (SCREEN_W as f32 + 8.0) - 4.0;
    let y = if spread { rand_f(state) * SCREEN_H as f32 } else { -rand_f(state) * 6.0 };
    let vy = style.fall * (0.6 + rand_f(state) * 0.8);
    BgParticle {
        x, y,
        vx: wind * style.drift * 3.0,
        vy,
        size: if (rng(state) % 100) < style.big_chance as u32 { 2 } else { 1 },
        glow: (rng(state) % 100) < style.glow_chance as u32,
        phase: rand_f(state) * TAU,
        spin: (rand_f(state) - 0.5) * 2.0 * style.spin,
        rot: rand_f(state) * TAU,
    }
}

/// Advance debris one tick: wind nudges horizontal drift, gravity pulls down,
/// off-screen particles are recycled and the set is topped up to the target count.
pub fn update_debris(particles: &mut Vec<BgParticle>, terrain: &Terrain, wind: f32, tick: u32) {
    let style = debris_style(terrain.archetype);
    let mut state = tick.wrapping_mul(2654435761).wrapping_add(0x9E3779B9) | 1;

    for p in particles.iter_mut() {
        p.vx += wind * style.drift * 0.25;
        p.vx *= 0.97;                       // damp so drift tracks wind, not runaway
        p.phase += style.sway_speed;
        p.x += p.vx + p.phase.sin() * style.sway_amp; // wavy arc, not a straight fall
        p.y += p.vy;
        p.rot += p.spin;
    }
    particles.retain(|p| {
        p.y < SCREEN_H as f32 + 4.0 && p.x > -8.0 && p.x < SCREEN_W as f32 + 8.0
    });

    // A strong gust visibly throws more motes across the screen.
    let target = style.count + (wind.abs() * 22.0) as usize;
    while particles.len() < target {
        particles.push(spawn(&mut state, &style, wind, true));
    }
}

/// Draw debris into the visible viewport, skipping any pixel behind terrain or
/// below the waterline so motes vanish behind the landscape.
pub fn draw_debris(buf: &mut WorldBuffer, terrain: &Terrain, particles: &[BgParticle], cam_x: u32, tick: u32) {
    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
    let water_y = WATER_Y as i32;
    let style = debris_style(terrain.archetype);

    for p in particles {
        let sx = p.x as i32;
        let sy = p.y as i32;
        if sy < 0 || sy >= water_y { continue; }

        let colour = if p.glow {
            // Flickering ember: warm orange that pulses with tick + position.
            let flick = ((tick.wrapping_add(sx as u32 * 7 + sy as u32 * 3)) % 5) as i32 * 12;
            Bgra::new((220 + flick).min(255) as u8, (110 + flick / 2).min(255) as u8, 40)
        } else {
            style.colour
        };

        // 1px motes are a single pixel; 2px motes draw a 3-pixel "flake/leaf"
        // whose orientation flips with `rot`, giving a tumbling flutter.
        let mut put = |ox: i32, oy: i32| {
            let px = sx + ox;
            let py = sy + oy;
            if py < 0 || py >= water_y || px < 0 || px >= SCREEN_W as i32 { return; }
            let wx = cam_x as i32 + px;
            if terrain.is_solid(wx, py) { return; }
            buf.set_pixel(wx, py, colour);
        };
        put(0, 0);
        if p.size >= 2 {
            // L-shaped trio rotated through 4 orientations by the rotation phase.
            match (((p.rot / (PI * 0.5)) as i32) & 3).abs() {
                0 => { put(1, 0); put(0, 1); }
                1 => { put(-1, 0); put(0, 1); }
                2 => { put(-1, 0); put(0, -1); }
                _ => { put(1, 0); put(0, -1); }
            }
        }
    }
}
