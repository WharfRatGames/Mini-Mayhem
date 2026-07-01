//! Worms-style atmospheric background layers, drawn each frame *behind* the
//! terrain (only on sky pixels) between the world-cache viewport copy and the
//! water ripple. Two layers:
//!
//!   1. `draw_backdrop` — a soft sun glow with slow horizontal parallax.
//!   2. `update_debris` / `draw_debris` — wind-driven ambient motes (snow / dust /
//!      embers / pollen) chosen per map look: the "alive air".
//!
//! All layers are client-only visuals (not networked) and are clipped to the
//! visible viewport. Anything that lands on solid terrain or below the waterline
//! is skipped, so the layers read as a true background.

use crate::world::{Terrain, WATER_Y, WORLD_W, SCREEN_W, SCREEN_H};
use super::buffer::WorldBuffer;
use super::fb::Bgra;
use super::sin_lut;
use std::f32::consts::{PI, TAU};

// ── Parallax backdrop ─────────────────────────────────────────────────────────

const PAR_SUN: f32 = 0.12;
const SUN_R:   i32 = 46;
const SUN_D:   usize = (SUN_R * 2 + 1) as usize; // 93

/// Precomputed sun glow disc: add[dy+R][dx+R] = additive brightness (0 = outside disc).
struct SunLut { add: [[u8; SUN_D]; SUN_D] }

fn sun_lut() -> &'static SunLut {
    static LUT: std::sync::OnceLock<SunLut> = std::sync::OnceLock::new();
    LUT.get_or_init(|| {
        let r2 = (SUN_R * SUN_R) as f32;
        let mut add = [[0u8; SUN_D]; SUN_D];
        for dy_i in 0..SUN_D {
            let dy = dy_i as f32 - SUN_R as f32;
            for dx_i in 0..SUN_D {
                let dx = dx_i as f32 - SUN_R as f32;
                let d2 = dx * dx + dy * dy;
                if d2 < r2 {
                    let f = 1.0 - d2 / r2;
                    add[dy_i][dx_i] = (f * f * 70.0) as u8;
                }
            }
        }
        SunLut { add }
    })
}

/// Draw the sun glow into the visible viewport, only on sky pixels (skips
/// terrain and water).
pub fn draw_backdrop(buf: &mut WorldBuffer, terrain: &Terrain, cam_x: u32, cam_y: u32) {
    use crate::world::SCREEN_H;
    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
    let water_y = WATER_Y as i32;

    // Sun is fixed in the upper-left of the world (world coords).
    // It's only visible when cam_y is near the top.
    let sun_sx = 130.0 - cam_x as f32 * PAR_SUN;
    let cx = sun_sx as i32;
    let wy_sun = 70i32; // world Y
    let vis_y0 = cam_y as i32;
    let vis_y1 = (cam_y + SCREEN_H) as i32;
    if sun_sx > -(SUN_R as f32) && sun_sx < SCREEN_W as f32 + SUN_R as f32 {
        let lut = sun_lut();
        let sx0 = (cx - SUN_R).max(0);
        let sx1 = (cx + SUN_R).min(SCREEN_W as i32);
        let wy0 = (wy_sun - SUN_R).max(vis_y0);
        let wy1 = (wy_sun + SUN_R).min(water_y).min(vis_y1);
        for wy in wy0..wy1 {
            let dy_i = (wy - wy_sun + SUN_R) as usize;
            let lut_row = &lut.add[dy_i];
            for sx in sx0..sx1 {
                let wx = (cam_x as i32 + sx) as u32;
                if wy >= terrain.sky_limit[wx as usize] as i32 { continue; }
                let dx_i = (sx - cx + SUN_R) as usize;
                let v = lut_row[dx_i];
                if v == 0 { continue; }
                let c = buf.get_pixel_unchecked(wx, wy as u32);
                buf.set_pixel_unchecked(wx, wy as u32, Bgra::new(
                    c.r.saturating_add(v),
                    c.g.saturating_add(v),
                    c.b.saturating_add(v >> 1),
                ));
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
    let breath = 1.0 + 0.35 * sin_lut(t * 0.030) + 0.25 * sin_lut(t * 0.011);
    // Slow envelope that occasionally crests → a short gust burst.
    let env = sin_lut(t * 0.006);
    let gust = if env > 0.7 { (env - 0.7) / 0.3 * 0.8 } else { 0.0 };
    base * (breath + gust)
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

/// Debris look: cavern maps get dust+embers; surface maps vary by which of the
/// 2 WA masks drove the silhouette (template_id) — pollen or snow.
fn debris_style(is_cavern: bool, template_id: u8) -> DebrisStyle {
    if is_cavern {
        DebrisStyle { colour: Bgra::new(96,  88,  82),  fall: 0.24, drift: 0.7, count: 220, big_chance: 8,  glow_chance: 20, sway_amp: 0.3, sway_speed: 0.06, spin: 0.06 } // dust + embers
    } else if template_id == 0 {
        DebrisStyle { colour: Bgra::new(212, 200, 140), fall: 0.10, drift: 0.9, count: 180, big_chance: 8,  glow_chance: 0,  sway_amp: 0.8, sway_speed: 0.09, spin: 0.14 } // pollen
    } else {
        DebrisStyle { colour: Bgra::new(238, 242, 250), fall: 0.55, drift: 1.2, count: 420, big_chance: 30, glow_chance: 0,  sway_amp: 0.9, sway_speed: 0.10, spin: 0.18 } // snow
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

/// Spawn one particle at a random position across the full screen height (0..SCREEN_H).
/// Always scattering maintains even vertical density — entering from the top only
/// caused the population to congregate near the top as bottom particles fell out.
fn spawn(state: &mut u32, style: &DebrisStyle, wind: f32) -> BgParticle {
    let x = rand_f(state) * (SCREEN_W as f32 + 8.0) - 4.0;
    let y = rand_f(state) * SCREEN_H as f32;
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
    let style = debris_style(terrain.is_cavern, terrain.template_id);
    let mut state = tick.wrapping_mul(2654435761_u32).wrapping_add(0x9E3779B9) | 1;

    for p in particles.iter_mut() {
        p.vx += wind * style.drift * 0.25;
        p.vx *= 0.97;                       // damp so drift tracks wind, not runaway
        p.phase += style.sway_speed;
        p.x += p.vx + sin_lut(p.phase) * style.sway_amp; // wavy arc, not a straight fall
        p.y += p.vy;
        p.rot += p.spin;
    }
    particles.retain(|p| {
        p.y < SCREEN_H as f32 + 4.0 && p.x > -8.0 && p.x < SCREEN_W as f32 + 8.0
    });

    // A strong gust visibly throws more motes across the screen.
    let target = style.count + (wind.abs() * 22.0) as usize;
    while particles.len() < target {
        particles.push(spawn(&mut state, &style, wind));
    }
}

/// Draw debris into the visible viewport, skipping any pixel behind terrain or
/// below the waterline so motes vanish behind the landscape.
pub fn draw_debris(buf: &mut WorldBuffer, terrain: &Terrain, particles: &[BgParticle], cam_x: u32, cam_y: u32, tick: u32) {
    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
    let style = debris_style(terrain.is_cavern, terrain.template_id);

    for p in particles {
        let sx = p.x as i32; // screen X
        let sy = p.y as i32; // screen Y (relative to viewport top)
        let wy0 = cam_y as i32 + sy; // world Y
        if sy < 0 || wy0 >= WATER_Y as i32 { continue; }

        let colour = if p.glow {
            let flick = ((tick.wrapping_add(sx as u32 * 7 + sy as u32 * 3)) % 5) as i32 * 12;
            Bgra::new((220 + flick).min(255) as u8, (110 + flick / 2).min(255) as u8, 40)
        } else {
            style.colour
        };

        let mut put = |ox: i32, oy: i32| {
            let px = sx + ox;
            let wy = wy0 + oy;
            if wy < 0 || wy >= WATER_Y as i32 || px < 0 || px >= SCREEN_W as i32 { return; }
            let wx = cam_x as i32 + px;
            if wx < 0 || wx >= WORLD_W as i32 { return; }
            if wy >= terrain.sky_limit[wx as usize] as i32 { return; }
            buf.set_pixel(wx, wy, colour);
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
