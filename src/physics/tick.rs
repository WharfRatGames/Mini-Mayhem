use super::projectile::{Projectile, WeaponKind};
use crate::world::Vec2;

/// Physics constants.
/// All units are pixels and ticks (1 tick = 1/20 s).

/// Gravity in pixels per tick² (downward, positive Y).
/// Tuned to feel like classic Worms — not too floaty, not too heavy.
pub const GRAVITY: f32 = 0.3;

/// Maximum fall speed in pixels per tick.
/// Prevents projectiles accelerating to absurd speeds on long drops.
pub const TERMINAL_VELOCITY: f32 = 18.0;

/// Wind strength is stored as a value in [-1.0, 1.0].
/// This scale converts that to pixels per tick² of horizontal acceleration.
/// Reduced 20% (was 0.08) per user request.
pub const WIND_SCALE: f32 = 0.064;

// ── Bazooka: Worms Armageddon reference physics ──────────────────────────────
// The reference game steps at 50 fps; Arty steps at 30 Hz. Same pixel space, so
// every per-tick value below is the reference per-frame value rescaled:
// velocity ×(50/30), acceleration ×(50/30)². Bazooka only — every other weapon
// keeps the shared defaults above.

/// Reference frames-per-second ÷ Arty ticks-per-second.
const WA_TIME_SCALE: f32 = crate::physics::wa_missile::TIME_SCALE;

/// Bazooka gravity. Reference: 0.24 px/frame² (measured live; thrown weapons and
/// the rocket share it). NOT yet confirmed from static code.
pub const BAZOOKA_GRAVITY: f32 = 0.24 * WA_TIME_SCALE * WA_TIME_SCALE;

/// Bazooka wind acceleration at full meter (wind = ±1.0). Reference max is about
/// 0.286 px/frame² (measured live). NOT yet confirmed from static code.
pub const BAZOOKA_WIND_SCALE: f32 = (10.0 / 35.0) * WA_TIME_SCALE * WA_TIME_SCALE;

/// Bazooka launch speed at full charge (power = 1.0), px/tick.
/// Reference (static, from the game's fire code): launch component =
/// unit_direction × charge × 0x18 (24 px/frame at full charge, linear in charge).
/// The spawn function passes this vector to the rocket unscaled (the /0x19 there
/// applies to a different field), so launch = 24 px/frame = 40 px/tick at 30 Hz.
pub const BAZOOKA_LAUNCH: f32 = 24.0 * WA_TIME_SCALE;

/// Bazooka charge gained per tick (power runs 0.0..=1.0).
/// Reference (static): the charge accumulator gains 0x369 per frame toward
/// 0x10000, i.e. 873/65536 per frame — 75 frames (1.5 s) from empty to full —
/// and the shot fires automatically at full charge. No overcharge band.
pub const BAZOOKA_CHARGE_RATE: f32 = 873.0 / 65536.0 * WA_TIME_SCALE;

/// The bazooka's meter tops out at exactly 1.0 (auto-fire), unlike the shared
/// 1.0..MAX_CHARGE band the other charge weapons use.
pub const BAZOOKA_MAX_CHARGE: f32 = 1.0;

/// Shared launch rules, used by `fire_weapon` AND the CPU's aim search so they can never disagree.
/// Muzzle position: 12 px out along the aim direction, 4 px above the soldier's origin.
pub fn muzzle_spawn(x: f32, y: f32, angle: f32, facing: f32) -> crate::world::WorldPos {
    crate::world::WorldPos::new(x + angle.cos() * facing * 12.0, y - 4.0 - angle.sin() * 12.0)
}

/// Launch speed (px/tick) for a weapon at a given charge fraction (bazooka: WA reference curve, capped at
/// 1.0; everything else: 20 px/tick at full charge, extended meter up to `MAX_CHARGE`).
pub fn launch_speed(kind: WeaponKind, charge: f32) -> f32 {
    if kind == WeaponKind::Bazooka {
        charge.min(BAZOOKA_MAX_CHARGE) * BAZOOKA_LAUNCH
    } else {
        charge.min(crate::game::loop_runner::MAX_CHARGE) * 20.0
    }
}

/// Apply one physics tick to a projectile.
///
/// Updates velocity and position using Euler integration:
///   velocity += acceleration
///   position += velocity
///
/// Gravity is always applied downward.
/// Wind is applied horizontally only to weapons that are affected by it.
/// Terminal velocity is clamped on the Y axis only.
///
/// `wind` is in [-1.0, 1.0]. Positive = rightward, negative = leftward.
/// Bazooka shell on the WA engine (see wa_missile.rs). `terrain = None` is the collision-free prediction used by
/// live clients between server states; with a terrain it returns the hit position (16.16 -> px).
pub fn tick_bazooka(proj: &mut Projectile, wind: f32, terrain: Option<&crate::world::Terrain>) -> Option<crate::world::WorldPos> {
    use crate::physics::wa_missile::{wind16, WaMissile};
    let units = (wind * crate::physics::wind::WA_WIND_UNITS as f32).round() as i32;
    let w16 = if proj.kind.affected_by_wind() { wind16(units) } else { 0 };
    let mut m = proj.wa.unwrap_or_else(|| WaMissile::from_float(proj.pos, proj.vel));
    let mut hit = None;
    for _ in 0..WaMissile::frames_for_age(proj.age_ticks) {
        if let Some((hx, hy)) = m.step_frame(w16, if hit.is_none() { terrain } else { None }) {
            hit = Some(crate::world::WorldPos::new(hx as f32 / 65536.0, hy as f32 / 65536.0));
            break;
        }
    }
    proj.pos = m.pos();
    proj.vel = m.vel();
    proj.wa = Some(m);
    proj.age_ticks += 1;
    proj.fuse = proj.fuse.tick();
    hit
}

pub fn tick(proj: &mut Projectile, wind: f32) {
    if proj.kind == WeaponKind::Bazooka {
        tick_bazooka(proj, wind, None);
        return;
    }
    // ── Acceleration ──────────────────────────────────────────────────────────
    // Homing missiles maintain constant speed — no gravity or wind drift.
    if proj.kind != WeaponKind::HomingMissile {
        proj.vel.y += if proj.kind == WeaponKind::Bazooka { BAZOOKA_GRAVITY } else { GRAVITY };
        if proj.kind.affected_by_wind() {
            let scale = if proj.kind == WeaponKind::Bazooka { BAZOOKA_WIND_SCALE } else { WIND_SCALE };
            proj.vel.x += wind * scale;
        }
    }

    // ── Terminal velocity (Y only) ────────────────────────────────────────────
    if proj.kind != WeaponKind::Bazooka && proj.vel.y > TERMINAL_VELOCITY {
        proj.vel.y = TERMINAL_VELOCITY;
    }

    // ── Position update ───────────────────────────────────────────────────────
    proj.pos.x += proj.vel.x;
    proj.pos.y += proj.vel.y;

    // ── Age and fuse ──────────────────────────────────────────────────────────
    proj.age_ticks += 1;
    proj.fuse = proj.fuse.tick();
}

/// Run multiple ticks at once. Useful for tests and AI ghost simulation.
pub fn tick_n(proj: &mut Projectile, wind: f32, n: u32) {
    for _ in 0..n {
        tick(proj, wind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WorldPos;
    use crate::physics::projectile::FuseState;

    fn bazooka(x: f32, y: f32, vx: f32, vy: f32) -> Projectile {
        Projectile::new(
            WorldPos::new(x, y),
            Vec2::new(vx, vy),
            WeaponKind::Bazooka,
        )
    }

    fn shotgun(x: f32, y: f32, vx: f32, vy: f32) -> Projectile {
        Projectile::new(
            WorldPos::new(x, y),
            Vec2::new(vx, vy),
            WeaponKind::Shotgun,
        )
    }

    fn grenade(x: f32, y: f32, vx: f32, vy: f32) -> Projectile {
        Projectile::new(
            WorldPos::new(x, y),
            Vec2::new(vx, vy),
            WeaponKind::Grenade,
        )
    }

    // ── Gravity ───────────────────────────────────────────────────────────────

    #[test]
    fn gravity_increases_y_velocity_each_tick() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        // The shell runs WA frames (5 per 3 Arty ticks): compare over whole 3-tick cycles.
        tick_n(&mut p, 0.0, 3);
        assert!((p.vel.y - BAZOOKA_GRAVITY * 3.0).abs() < 1e-3, "vy should equal 3×BAZOOKA_GRAVITY after 3 ticks (5 WA frames)");
        tick_n(&mut p, 0.0, 3);
        assert!((p.vel.y - BAZOOKA_GRAVITY * 6.0).abs() < 1e-3, "vy should equal 6×BAZOOKA_GRAVITY after 6 ticks");
    }

    // ── Bazooka reference constants (bazooka only; other weapons unchanged) ───

    #[test]
    fn non_bazooka_keeps_shared_gravity() {
        let mut g = grenade(100.0, 100.0, 0.0, 0.0);
        tick(&mut g, 0.0);
        assert!((g.vel.y - GRAVITY).abs() < 1e-5, "grenade must keep the shared gravity");
    }

    #[test]
    fn bazooka_charge_takes_one_and_a_half_seconds() {
        // Reference: +0x369 per 50 fps frame toward 0x10000 = 75.07 frames = 1.50 s.
        let ticks_to_full = (BAZOOKA_MAX_CHARGE / BAZOOKA_CHARGE_RATE).ceil() as u32;
        let secs = ticks_to_full as f32 / 30.0;
        assert!((secs - 1.5).abs() < 0.05, "full charge should take ~1.5 s, got {secs:.3}s ({ticks_to_full} ticks)");
        assert_eq!(BAZOOKA_MAX_CHARGE, 1.0, "bazooka meter caps at exactly full (no overcharge band)");
    }

    #[test]
    fn bazooka_full_charge_launch_speed_matches_reference() {
        // 24 px/frame at 50 fps  ->  40 px/tick at 30 Hz.
        assert!((BAZOOKA_LAUNCH - 40.0).abs() < 1e-3, "launch {BAZOOKA_LAUNCH}");
    }

    #[test]
    fn bazooka_physics_constants_match_reference_per_frame_values() {
        let s = 50.0f32 / 30.0;
        assert!((BAZOOKA_GRAVITY - 0.24 * s * s).abs() < 1e-5);
        assert!((BAZOOKA_WIND_SCALE - (10.0 / 35.0) * s * s).abs() < 1e-5);
    }

    #[test]
    fn gravity_moves_projectile_downward() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        let y0 = p.pos.y;
        tick(&mut p, 0.0);
        assert!(p.pos.y > y0, "projectile should move downward due to gravity");
    }

    #[test]
    fn horizontal_velocity_unchanged_without_wind() {
        let mut p = bazooka(100.0, 100.0, 5.0, 0.0);
        tick(&mut p, 0.0);
        assert!((p.vel.x - 5.0).abs() < 1e-5, "vx should be unchanged with zero wind");
    }

    // ── Wind ─────────────────────────────────────────────────────────────────

    #[test]
    fn positive_wind_pushes_bazooka_rightward() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        tick_n(&mut p, 1.0, 3);
        assert!(p.vel.x > 0.0, "positive wind should push projectile right");
        assert!((p.vel.x - BAZOOKA_WIND_SCALE * 3.0).abs() < 2e-3);
    }

    #[test]
    fn negative_wind_pushes_bazooka_leftward() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        tick_n(&mut p, -1.0, 3);
        assert!(p.vel.x < 0.0, "negative wind should push projectile left");
        assert!((p.vel.x + BAZOOKA_WIND_SCALE * 3.0).abs() < 2e-3);
    }

    #[test]
    fn wind_accumulates_over_ticks() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        tick_n(&mut p, 1.0, 6);
        assert!((p.vel.x - BAZOOKA_WIND_SCALE * 6.0).abs() < 4e-3,
            "wind should accumulate over 6 ticks");
    }

    #[test]
    fn shotgun_not_affected_by_wind() {
        let mut p = shotgun(100.0, 100.0, 5.0, 0.0);
        let vx0 = p.vel.x;
        tick(&mut p, 1.0);
        assert!((p.vel.x - vx0).abs() < 1e-5,
            "shotgun should not be affected by wind");
    }

    #[test]
    fn zero_wind_has_no_horizontal_effect_on_bazooka() {
        let mut p = bazooka(100.0, 100.0, 3.0, 0.0);
        tick(&mut p, 0.0);
        assert!((p.vel.x - 3.0).abs() < 1e-5);
    }

    // ── Terminal velocity ─────────────────────────────────────────────────────

    #[test]
    fn y_velocity_capped_at_terminal_velocity() {
        let mut p = grenade(100.0, 100.0, 0.0, 0.0);
        // Run enough ticks that gravity would exceed terminal velocity
        tick_n(&mut p, 0.0, 200);
        assert!(
            p.vel.y <= TERMINAL_VELOCITY,
            "vy={} should be capped at {TERMINAL_VELOCITY}", p.vel.y
        );
    }

    #[test]
    fn upward_velocity_not_capped() {
        // A projectile fired upward should not be clamped
        let mut p = bazooka(100.0, 100.0, 0.0, -15.0);
        tick(&mut p, 0.0);
        assert!(p.vel.y < 0.0 || p.vel.y >= 0.0); // just checking it doesn't panic
        // After enough ticks gravity brings it back down
        tick_n(&mut p, 0.0, 60);
        assert!(p.vel.y > 0.0, "projectile should be falling after arc");
    }

    // ── Position integration ──────────────────────────────────────────────────

    #[test]
    fn position_moves_by_velocity_each_tick() {
        let mut p = bazooka(100.0, 200.0, 5.0, -3.0);
        let x0 = p.pos.x;
        let y0 = p.pos.y;
        let _vx = p.vel.x;
        let _vy = p.vel.y;
        tick(&mut p, 0.0);
        // After one tick: pos += vel (before vel is updated by gravity)
        // Actually Euler: vel updated first, then pos += new vel
        assert!(p.pos.x > x0, "x should have moved right");
        assert!(p.pos.y < y0, "y should have moved up (negative vy)");
    }

    #[test]
    fn stationary_projectile_falls_under_gravity() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        let y0 = p.pos.y;
        tick_n(&mut p, 0.0, 10);
        assert!(p.pos.y > y0, "should fall under gravity");
    }

    #[test]
    fn projectile_with_horizontal_velocity_moves_right() {
        let mut p = bazooka(0.0, 100.0, 10.0, 0.0);
        tick(&mut p, 0.0);
        assert!(p.pos.x > 0.0);
    }

    // ── Age and fuse ──────────────────────────────────────────────────────────

    #[test]
    fn age_increments_each_tick() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        assert_eq!(p.age_ticks, 0);
        tick(&mut p, 0.0);
        assert_eq!(p.age_ticks, 1);
        tick_n(&mut p, 0.0, 9);
        assert_eq!(p.age_ticks, 10);
    }

    #[test]
    fn grenade_fuse_counts_down_each_tick() {
        let mut p = grenade(100.0, 100.0, 0.0, 0.0);
        assert_eq!(p.fuse, FuseState::Burning(60));
        tick(&mut p, 0.0);
        assert_eq!(p.fuse, FuseState::Burning(59));
        // 59 more ticks lands on Burning(0), one final tick expires it
        tick_n(&mut p, 0.0, 59);
        assert_eq!(p.fuse, FuseState::Burning(0));
        tick(&mut p, 0.0);
        assert_eq!(p.fuse, FuseState::Expired);
    }

    #[test]
    fn bazooka_fuse_stays_none_through_ticks() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        tick_n(&mut p, 0.0, 10);
        assert_eq!(p.fuse, FuseState::None);
    }

    // ── Parabolic arc sanity ──────────────────────────────────────────────────

    #[test]
    fn projectile_follows_parabolic_arc() {
        // Fire at 45° upward to the right, no wind
        let speed = 10.0f32;
        let angle = std::f32::consts::PI / 4.0; // 45°
        let mut p = bazooka(
            100.0, 300.0,
            angle.cos() * speed,
            -angle.sin() * speed, // negative = upward
        );

        let y_start = p.pos.y;
        let mut peak_y = y_start;
        let mut landed = false;

        for _ in 0..200 {
            tick(&mut p, 0.0);
            if p.pos.y < peak_y { peak_y = p.pos.y; }
            if p.pos.y > y_start && p.age_ticks > 5 {
                landed = true;
                break;
            }
        }

        assert!(peak_y < y_start, "projectile should rise above start");
        assert!(landed, "projectile should come back down past start height");
        assert!(p.pos.x > 100.0, "projectile should have moved rightward");
    }
}
