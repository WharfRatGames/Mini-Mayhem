//! Event-driven effect particles — the gameplay "juice" flung by explosions,
//! footsteps, landings and digging. Cheap fake-physics (gravity + wind drift +
//! fade), short-lived, drawn over the terrain but under the soldiers.
//!
//! These are client-only visuals: they live in `GameState` next to
//! `smoke_particles`/`explosions` and are spawned at the event sites, but they
//! are NOT networked (the netcode ships explicit `msg.rs` structs, not the whole
//! `GameState`), exactly like `smoke_particles`. Spawning runs in `simulate()`
//! on both client and server; only the client draws.

use crate::world::{Terrain, WorldPos, Vec2, WATER_Y, WORLD_W, SCREEN_W};
use super::buffer::WorldBuffer;
use super::fb::Bgra;
use serde::{Serialize, Deserialize};

/// Hard cap on live effect particles — bounds cost on the Miyoo when several big
/// explosions overlap.
pub const FX_MAX: usize = 600;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FxKind {
    DirtChunk, // terrain-coloured, gravity, dies on terrain contact
    Spark,     // bright, near-weightless, fast fade
    Dust,      // soft grey puff, light gravity, drifts on wind
    Splash,    // white-blue water droplet, up-arc then falls
}

pub struct FxParticle {
    pub pos:  WorldPos,
    pub vel:  Vec2,
    pub age:  u32,
    pub life: u32,
    pub kind: FxKind,
    pub col:  Bgra,
}

/// Tiny xorshift PRNG — these are non-networked visuals, determinism unneeded.
#[inline]
fn rng(state: &mut u32) -> u32 {
    let mut x = *state;
    x ^= x << 13; x ^= x >> 17; x ^= x << 5;
    *state = x; x
}
#[inline]
fn rf(state: &mut u32) -> f32 { (rng(state) >> 8) as f32 / (1u32 << 24) as f32 }

/// Seed a PRNG from a position so each burst looks different.
fn seed_at(pos: WorldPos, salt: u32) -> u32 {
    (pos.x as u32)
        .wrapping_mul(2654435761)
        .wrapping_add((pos.y as u32).wrapping_mul(40503))
        .wrapping_add(salt.wrapping_mul(0x9E3779B9))
        | 1
}

/// Push particles, respecting the global cap.
fn push(fx: &mut Vec<FxParticle>, p: FxParticle) {
    if fx.len() < FX_MAX { fx.push(p); }
}

// ── Spawners ─────────────────────────────────────────────────────────────────

/// Explosion fallout: dirt chunks flung outward+up plus a few bright sparks.
/// `radius` scales the count and spread. `dirt` is a biome dirt tone.
pub fn explosion(fx: &mut Vec<FxParticle>, pos: WorldPos, radius: f32, dirt: Bgra) {
    let mut s = seed_at(pos, 0xE7);
    let chunks = (8.0 + radius * 0.8).min(40.0) as u32;
    for _ in 0..chunks {
        let ang = rf(&mut s) * std::f32::consts::TAU;
        let spd = 2.0 + rf(&mut s) * 4.0;
        push(fx, FxParticle {
            pos,
            vel: Vec2::new(ang.cos() * spd, ang.sin() * spd - (1.0 + rf(&mut s) * 3.0)),
            age: 0,
            life: 22 + (rf(&mut s) * 24.0) as u32,
            kind: FxKind::DirtChunk,
            col: dirt,
        });
    }
    let sparks = (6.0 + radius * 0.4).min(20.0) as u32;
    for _ in 0..sparks {
        let ang = rf(&mut s) * std::f32::consts::TAU;
        let spd = 3.0 + rf(&mut s) * 5.0;
        push(fx, FxParticle {
            pos,
            vel: Vec2::new(ang.cos() * spd, ang.sin() * spd * 0.6),
            age: 0,
            life: 6 + (rf(&mut s) * 8.0) as u32,
            kind: FxKind::Spark,
            col: Bgra::new(255, 230, 120),
        });
    }
}

/// Water splash: droplets arcing up from `pos` (call when a blast hits water).
pub fn splash(fx: &mut Vec<FxParticle>, pos: WorldPos) {
    let mut s = seed_at(pos, 0x5A);
    for _ in 0..10 {
        let spread = (rf(&mut s) - 0.5) * 4.0;
        push(fx, FxParticle {
            pos,
            vel: Vec2::new(spread, -(2.5 + rf(&mut s) * 3.5)),
            age: 0,
            life: 18 + (rf(&mut s) * 14.0) as u32,
            kind: FxKind::Splash,
            col: Bgra::new(210, 232, 248),
        });
    }
}

/// Small dust puff(s) at the feet — landings and footsteps. `count` puffs,
/// `kick` adds outward speed (scale by fall damage for landings).
pub fn dust(fx: &mut Vec<FxParticle>, pos: WorldPos, count: u32, kick: f32, dir: f32) {
    let mut s = seed_at(pos, 0xD0 ^ count);
    for _ in 0..count {
        let spread = (rf(&mut s) - 0.5) * 1.6 - dir * (0.4 + rf(&mut s) * kick);
        push(fx, FxParticle {
            pos,
            vel: Vec2::new(spread, -(0.3 + rf(&mut s) * 0.8)),
            age: 0,
            life: 10 + (rf(&mut s) * 10.0) as u32,
            kind: FxKind::Dust,
            col: Bgra::new(176, 168, 150),
        });
    }
}

/// Dirt chips ejected from a dig tip (plasma torch / drill), opposite the dig dir.
pub fn dig(fx: &mut Vec<FxParticle>, pos: WorldPos, dir: f32, dirt: Bgra) {
    let mut s = seed_at(pos, 0x16);
    for _ in 0..3 {
        push(fx, FxParticle {
            pos,
            vel: Vec2::new(-dir * (1.0 + rf(&mut s) * 2.0), -(0.5 + rf(&mut s) * 2.0)),
            age: 0,
            life: 16 + (rf(&mut s) * 14.0) as u32,
            kind: FxKind::DirtChunk,
            col: dirt,
        });
    }
}

// ── Networked spawn events ───────────────────────────────────────────────────

/// A request to spawn one of the bursts above. Recorded by `GameState::emit_fx`
/// during `simulate()` and shipped to live clients in `StateMsg.fx_events`, so
/// effects spawned in the shared sim appear in every mode — mirroring the
/// `Sfx`/`sounds` channel. Route ALL gameplay-event fx through `emit_fx`; the
/// live client never runs `simulate()`, so direct `fx::` spawns are invisible to it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum FxEvent {
    Explosion { x: f32, y: f32, radius: f32, col: [u8; 3] },
    Splash    { x: f32, y: f32 },
    Dust      { x: f32, y: f32, count: u32, kick: f32, dir: f32 },
    Dig       { x: f32, y: f32, dir: f32, col: [u8; 3] },
}

/// Spawn the particles described by `ev` into `fx` (used both at the local
/// emit site and when a live client replays a received event).
pub fn apply_event(fx: &mut Vec<FxParticle>, ev: &FxEvent) {
    match *ev {
        FxEvent::Explosion { x, y, radius, col } =>
            explosion(fx, WorldPos::new(x, y), radius, Bgra::new(col[0], col[1], col[2])),
        FxEvent::Splash { x, y } =>
            splash(fx, WorldPos::new(x, y)),
        FxEvent::Dust { x, y, count, kick, dir } =>
            dust(fx, WorldPos::new(x, y), count, kick, dir),
        FxEvent::Dig { x, y, dir, col } =>
            dig(fx, WorldPos::new(x, y), dir, Bgra::new(col[0], col[1], col[2])),
    }
}

// ── Update ───────────────────────────────────────────────────────────────────

/// Advance all fx one tick: gravity + wind drift + ageing; dirt chunks settle on
/// terrain contact. Called from `simulate()` alongside `step_explosions()`.
pub fn step_fx(fx: &mut Vec<FxParticle>, terrain: &Terrain, wind: f32) {
    for p in fx.iter_mut() {
        let (grav, wind_k, drag) = match p.kind {
            FxKind::DirtChunk => (0.45, 0.02, 1.0),
            FxKind::Spark     => (0.08, 0.01, 0.92),
            FxKind::Dust      => (0.06, 0.05, 0.94),
            FxKind::Splash    => (0.40, 0.01, 1.0),
        };
        p.vel.y += grav;
        p.vel.x = p.vel.x * drag + wind * wind_k;
        p.pos.x += p.vel.x;
        p.pos.y += p.vel.y;
        p.age += 1;

        // Dirt chunks die when they hit solid ground (so they don't tunnel through).
        if matches!(p.kind, FxKind::DirtChunk)
            && p.vel.y > 0.0
            && terrain.is_solid(p.pos.x as i32, p.pos.y as i32)
        {
            p.age = p.life;
        }
    }
    fx.retain(|p| {
        p.age < p.life
            && p.pos.x > -4.0 && p.pos.x < WORLD_W as f32 + 4.0
            && p.pos.y < WATER_Y as f32 + 8.0
    });
}

// ── Draw ─────────────────────────────────────────────────────────────────────

/// Draw all fx in world space, culled to the viewport, over terrain.
pub fn draw_fx(buf: &mut WorldBuffer, fx: &[FxParticle], cam_x: u32) {
    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
    let vx0 = cam_x as f32;
    let vx1 = vx0 + SCREEN_W as f32;

    for p in fx {
        if p.pos.x < vx0 - 4.0 || p.pos.x >= vx1 + 4.0 { continue; }
        let x = p.pos.x as i32;
        let y = p.pos.y as i32;
        let t = 1.0 - p.age as f32 / p.life.max(1) as f32; // 1 fresh → 0 dead

        match p.kind {
            FxKind::Spark => {
                // Hot core fading yellow → orange → red.
                let c = if t > 0.6 { Bgra::new(255, 240, 160) }
                        else if t > 0.3 { Bgra::new(255, 150, 40) }
                        else { Bgra::new(190, 40, 20) };
                buf.set_pixel(x, y, c);
            }
            FxKind::DirtChunk => {
                buf.set_pixel(x, y, p.col);
                if t > 0.5 { buf.set_pixel(x + 1, y, p.col); }
            }
            FxKind::Dust => {
                // Soft expanding puff that fades; bigger early in life.
                let r = if t > 0.6 { 2 } else { 1 };
                buf.fill_circle(x, y, r, p.col);
            }
            FxKind::Splash => {
                buf.set_pixel(x, y, p.col);
                if t > 0.5 { buf.set_pixel(x, y - 1, p.col); }
            }
        }
    }
}
