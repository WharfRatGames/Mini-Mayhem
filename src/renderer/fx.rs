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
pub const FX_MAX: usize = 300;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FxKind {
    DirtChunk, // terrain-coloured, gravity, dies on terrain contact
    Spark,     // bright, near-weightless, fast fade
    Dust,      // soft grey puff, light gravity, drifts on wind
    Splash,    // white-blue water droplet, up-arc then falls
    Smoke,     // WA flame-in-water steam puff: rises, jitters, drag 0.95, grows and fades (vel in px per 50 Hz frame)
}

pub struct FxParticle {
    pub pos:  WorldPos,
    pub vel:  Vec2,
    pub age:  u32,
    pub life: u32,
    pub kind: FxKind,
    pub col:  Bgra,
}

/// A floating "-N" damage number popping up over a soldier's HP counter,
/// Worms-style. Rises fast then decelerates, fading out near the end of life.
pub struct FxDamageText {
    pub pos:    WorldPos,
    pub vy:     f32,
    pub age:    u32,
    pub life:   u32,
    pub amount: u8,
    pub color_id:   u8,
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
        .wrapping_mul(2654435761_u32)
        .wrapping_add((pos.y as u32).wrapping_mul(40503))
        .wrapping_add(salt.wrapping_mul(0x9E3779B9))
        | 1
}

/// Push particles, respecting the global cap.
fn push(fx: &mut Vec<FxParticle>, p: FxParticle) {
    if fx.len() < FX_MAX { fx.push(p); }
}

/// Push damage texts, respecting the global cap.
fn push_text(texts: &mut Vec<FxDamageText>, t: FxDamageText) {
    if texts.len() < FX_MAX { texts.push(t); }
}

// ── Spawners ─────────────────────────────────────────────────────────────────

/// Explosion fallout: dirt chunks flung outward+up plus a few bright sparks.
/// `radius` scales the count and spread. `dirt` is a biome dirt tone.
fn explosion(fx: &mut Vec<FxParticle>, pos: WorldPos, radius: f32, dirt: Bgra) {
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
fn splash(fx: &mut Vec<FxParticle>, pos: WorldPos) {
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

/// Steam puffs where a flame dies in water. WA (decompile 0x54fd40 spawner, 0x555280 particle step): each puff starts at the flame with
/// a random +-0.5 px/frame velocity, rises (accel -0.15 px/frame^2), gets +-0.5 px/frame jitter and x0.95 drag every frame, and lives
/// 65536/rate frames with rate 0x20c..0x49b (55..125 frames = 33..75 ticks). `count` = 4 + 4 * the flame's remaining splits in WA.
fn smoke(fx: &mut Vec<FxParticle>, pos: WorldPos, count: u32) {
    let mut s = seed_at(pos, 0x5B0 ^ count);
    for _ in 0..count.min(24) {
        let rate = 524.0 + rf(&mut s) * 655.0;
        push(fx, FxParticle {
            pos,
            vel: Vec2::new(rf(&mut s) - 0.5, rf(&mut s) - 0.5),
            age: 0,
            life: ((65536.0 / rate) * 0.6) as u32,
            kind: FxKind::Smoke,
            col: Bgra::new(206, 208, 214),
        });
    }
}

/// Small dust puff(s) at the feet — landings and footsteps. `count` puffs,
/// `kick` adds outward speed (scale by fall damage for landings).
fn dust(fx: &mut Vec<FxParticle>, pos: WorldPos, count: u32, kick: f32, dir: f32) {
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
fn dig(fx: &mut Vec<FxParticle>, pos: WorldPos, dir: f32, dirt: Bgra) {
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

/// Vertical offset from a soldier's foot position up to where the HP counter
/// box is drawn — mirrors `draw_hp_number_lifted`'s `fy - SOLDIER_H - 28`.
const HP_COUNTER_LIFT: f32 = 56.0;

/// Spawn a floating "-N" damage number over the HP counter above `pos`
/// (soldier's foot position). Pops up fast then decelerates, so it visibly
/// separates from the counter before fading — matching the HP counter's own
/// tick-down (`Soldier::displayed_hp`).
fn damage_popup(texts: &mut Vec<FxDamageText>, pos: WorldPos, amount: u8, color_id: u8) {
    let start = WorldPos::new(pos.x, pos.y - HP_COUNTER_LIFT);
    push_text(texts, FxDamageText { pos: start, vy: -1.2, age: 0, life: 90, amount, color_id });
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
    DamagePopup { x: f32, y: f32, amount: u8, color_id: u8 },
    /// Steam puffs where a flame hits water (WA 0x5501a0: particles spawned by 0x54fd40).
    Smoke     { x: f32, y: f32, count: u32 },
}

/// Spawn the particles described by `ev` into `fx`/`texts` (used both at the
/// local emit site and when a live client replays a received event).
pub fn apply_event(fx: &mut Vec<FxParticle>, texts: &mut Vec<FxDamageText>, ev: &FxEvent) {
    match *ev {
        FxEvent::Explosion { x, y, radius, col } =>
            explosion(fx, WorldPos::new(x, y), radius, Bgra::new(col[0], col[1], col[2])),
        FxEvent::Splash { x, y } =>
            splash(fx, WorldPos::new(x, y)),
        FxEvent::Dust { x, y, count, kick, dir } =>
            dust(fx, WorldPos::new(x, y), count, kick, dir),
        FxEvent::Dig { x, y, dir, col } =>
            dig(fx, WorldPos::new(x, y), dir, Bgra::new(col[0], col[1], col[2])),
        FxEvent::DamagePopup { x, y, amount, color_id } =>
            damage_popup(texts, WorldPos::new(x, y), amount, color_id),
        FxEvent::Smoke { x, y, count } =>
            smoke(fx, WorldPos::new(x, y), count),
    }
}

// ── Update ───────────────────────────────────────────────────────────────────

/// Advance all fx one tick: gravity + wind drift + ageing; dirt chunks settle on
/// terrain contact. Called from `simulate()` alongside `step_explosions()`.
pub fn step_fx(fx: &mut Vec<FxParticle>, terrain: &Terrain, wind: f32) {
    for p in fx.iter_mut() {
        if p.kind == FxKind::Smoke {
            // 50 Hz WA frames: 1 or 2 per 30 Hz tick (5 per 3 ticks)
            let frames = ((p.age + 1) * 5 / 3) - (p.age * 5 / 3);
            for k in 0..frames {
                let mut h = (p.age * 2 + k).wrapping_mul(0x9E37_79B1) ^ (p.pos.x as u32).wrapping_mul(0x85EB_CA6B) ^ (p.pos.y as u32).wrapping_mul(0xC2B2_AE35);
                h ^= h >> 15; h = h.wrapping_mul(0x2C1B_3C6D); h ^= h >> 12;
                let (jx, jy) = ((h & 0xffff) as f32 / 65536.0 - 0.5, ((h >> 16) & 0xffff) as f32 / 65536.0 - 0.5);
                p.pos.x += p.vel.x;
                p.pos.y += p.vel.y;
                p.vel.y -= 0.15;
                p.vel.x = (p.vel.x + jx) * 0.95;
                p.vel.y = (p.vel.y + jy) * 0.95;
            }
            p.age += 1;
            continue;
        }
        let (grav, wind_k, drag) = match p.kind {
            FxKind::DirtChunk => (0.45, 0.02, 1.0),
            FxKind::Spark     => (0.08, 0.01, 0.92),
            FxKind::Dust      => (0.06, 0.05, 0.94),
            FxKind::Splash    => (0.40, 0.01, 1.0),
            FxKind::Smoke     => (0.0, 0.0, 1.0), // handled above
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

/// Advance damage-number popups: rise, decelerate, age out.
pub fn step_fx_text(texts: &mut Vec<FxDamageText>) {
    for t in texts.iter_mut() {
        t.pos.y += t.vy;
        t.vy *= 0.96; // decelerate — quick ~30px rise, then hold until fade
        t.age += 1;
    }
    texts.retain(|t| t.age < t.life);
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
            FxKind::Smoke => {
                // grows from 1 to 4 px radius, then thins out (checker dither stands in for alpha) over the last third
                let age = 1.0 - t;
                let r = 1 + (age * 3.4) as i32;
                let thin = t < 0.35;
                for dy in -r..=r { for dx in -r..=r {
                    if dx * dx + dy * dy > r * r + r / 2 { continue; }
                    if thin && ((x + dx + y + dy) & 1) == 0 { continue; }
                    buf.set_pixel(x + dx, y + dy, p.col);
                } }
            }
        }
    }
}

/// Draw floating damage-number popups, culled to the viewport. Bare number in
/// the damaged soldier's team colour (matching its HP counter box), darkening
/// toward black over the last quarter of life instead of true alpha blending.
pub fn draw_fx_text(buf: &mut WorldBuffer, texts: &[FxDamageText], cam_x: u32) {
    let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
    let vx0 = cam_x as f32;
    let vx1 = vx0 + SCREEN_W as f32;

    for t in texts {
        if t.pos.x < vx0 - 20.0 || t.pos.x >= vx1 + 20.0 { continue; }
        let frac = 1.0 - t.age as f32 / t.life.max(1) as f32; // 1 fresh → 0 dead
        let text = format!("{}", t.amount);
        let scale = 1;
        let w = super::font::str_width_scaled(&text, scale);
        let x = t.pos.x as i32 - w / 2;
        let y = t.pos.y as i32;
        let base = super::draw_sprites::TEAM_COLOURS[(t.color_id as usize).min(3)];
        let k = if frac > 0.25 { 1.0 } else { frac * 4.0 }; // fade last 25% of life
        let col = Bgra::new(
            (base.r as f32 * k) as u8,
            (base.g as f32 * k) as u8,
            (base.b as f32 * k) as u8,
        );
        super::font::draw_str_shadow_scaled(buf, &text, x, y, col, scale);
    }
}


/// One flame: a licking teardrop ~6 px wide and 8-12 px tall that sways and flickers (user: the star blobs read as sparks). Hot pale-yellow
/// core, yellow body, orange edge and a red-orange tip. `frame` 0..21 = flicker loop (height/sway/tongue wobble), 22..31 = die-out
/// (shrinks and goes red) like WA's 32-frame flame1/flame2 animation. Anchored bottom-centre at (cx, base_y). Cosmetic only.
pub fn draw_flame_star(buf: &mut WorldBuffer, frame: u32, cx: i32, base_y: i32) {
    let tau = std::f32::consts::TAU;
    let (h, w0, ph, dying) = if frame < 22 {
        let t = frame as f32 / 22.0;
        (9.5 + 2.2 * (t * 2.0 * tau).sin() + 1.0 * (t * 3.0 * tau).cos(), 3.2 + 0.5 * (t * tau).cos(), t * tau, 0.0)
    } else {
        let d = (frame - 22) as f32 / 9.0;
        ((9.0 * (1.0 - d * 0.85)).max(1.5), (3.0 * (1.0 - d * 0.7)).max(0.8), frame as f32 * 0.7, d)
    };
    let rows = h.ceil() as i32;
    for r in 0..=rows {
        let t = r as f32 / h.max(1.0); // 0 at the base .. 1 at the tip
        if t > 1.0 { break; }
        // teardrop: widest a little above the ground, pinching to the tip; the whole tongue leans and wobbles with height
        let bulge = (t * 3.1).min(1.0).sqrt() * (1.0 - t).powf(0.85) * 1.35;
        let half = (w0 * bulge).max(if t < 0.9 { 0.45 } else { 0.0 });
        let sway = (ph + t * 2.6).sin() * 1.6 * t * t + (ph * 2.0 + t * 5.0).sin() * 0.5 * t;
        let y = base_y - r;
        let lo = (sway - half).round() as i32;
        let hi = (sway + half).round() as i32;
        for dx in lo..=hi {
            let k = if half > 0.01 { ((dx as f32 - sway).abs() / half).min(1.0) } else { 1.0 }; // 0 axis .. 1 edge
            let heat = 1.0 - (k * 0.7 + t * 0.55); // hotter at the core and low down
            let col = if dying > 0.6 { if heat > 0.25 { Bgra::new(224, 100, 10) } else { Bgra::new(150, 40, 10) } }
                      else if heat > 0.62 { Bgra::new(252, 240, 170) }
                      else if heat > 0.38 { Bgra::new(250, 204, 40) }
                      else if heat > 0.16 { Bgra::new(240, 140, 20) }
                      else { Bgra::new(196, 62, 10) };
            buf.set_pixel(cx + dx, y, col);
        }
    }
}
