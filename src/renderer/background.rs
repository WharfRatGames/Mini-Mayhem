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

// ── Parallax backdrop ─────────────────────────────────────────────────────────

/// Parallax factors (< 1.0 = scrolls slower than the world → reads as distant).
const PAR_FAR:  f32 = 0.25;
const PAR_NEAR: f32 = 0.45;
const PAR_SUN:  f32 = 0.12;

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

// ── Wind-driven ambient debris ──────────────────────────────────────────────

/// One ambient air particle, in viewport-relative coords (0..SCREEN_W/H).
pub struct BgParticle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    size: u8,
    glow: bool,
}

struct DebrisStyle {
    colour: Bgra,
    fall: f32,        // base downward speed (px/tick)
    drift: f32,       // wind sensitivity
    count: usize,     // target particle count
    big_chance: u8,   // 0..100 chance of a 2px particle
    glow_chance: u8,  // 0..100 chance of being a glowing ember
}

/// Per-archetype debris look: hills→pollen, cliffs→snow, islands→mist,
/// caverns→dust+embers, canyon→dust.
fn debris_style(archetype: u8) -> DebrisStyle {
    match archetype {
        1 => DebrisStyle { colour: Bgra::new(238, 242, 250), fall: 0.55, drift: 1.2, count: 70, big_chance: 30, glow_chance: 0 }, // snow
        2 => DebrisStyle { colour: Bgra::new(200, 220, 236), fall: 0.16, drift: 1.5, count: 40, big_chance: 10, glow_chance: 0 }, // sea mist
        3 => DebrisStyle { colour: Bgra::new(96,  88,  82),  fall: 0.24, drift: 0.7, count: 52, big_chance: 8,  glow_chance: 20 }, // dust + embers
        4 => DebrisStyle { colour: Bgra::new(202, 176, 134), fall: 0.20, drift: 1.0, count: 46, big_chance: 12, glow_chance: 0 }, // canyon dust
        _ => DebrisStyle { colour: Bgra::new(212, 200, 140), fall: 0.10, drift: 0.9, count: 36, big_chance: 8,  glow_chance: 0 }, // pollen
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
        vx: wind * style.drift,
        vy,
        size: if (rng(state) % 100) < style.big_chance as u32 { 2 } else { 1 },
        glow: (rng(state) % 100) < style.glow_chance as u32,
    }
}

/// Advance debris one tick: wind nudges horizontal drift, gravity pulls down,
/// off-screen particles are recycled and the set is topped up to the target count.
pub fn update_debris(particles: &mut Vec<BgParticle>, terrain: &Terrain, wind: f32, tick: u32) {
    let style = debris_style(terrain.archetype);
    let mut state = tick.wrapping_mul(2654435761).wrapping_add(0x9E3779B9) | 1;

    for p in particles.iter_mut() {
        p.vx += wind * style.drift * 0.05;
        p.vx *= 0.97;                       // damp so drift tracks wind, not runaway
        p.x += p.vx;
        p.y += p.vy;
    }
    particles.retain(|p| {
        p.y < SCREEN_H as f32 + 4.0 && p.x > -8.0 && p.x < SCREEN_W as f32 + 8.0
    });

    let spread = particles.is_empty();
    while particles.len() < style.count {
        particles.push(spawn(&mut state, &style, wind, spread));
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

        let r = p.size as i32;
        for oy in 0..r {
            for ox in 0..r {
                let px = sx + ox;
                let py = sy + oy;
                if py < 0 || py >= water_y || px < 0 || px >= SCREEN_W as i32 { continue; }
                let wx = cam_x as i32 + px;
                if terrain.is_solid(wx, py) { continue; }
                buf.set_pixel(wx, py, colour);
            }
        }
    }
}
