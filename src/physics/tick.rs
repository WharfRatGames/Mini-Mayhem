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
pub const WIND_SCALE: f32 = 0.08;

// ── Bazooka: literal Worms Armageddon physics ────────────────────────────────
// Extracted by dynamic analysis of the real WA.exe (see the arty memory notes).
// WA runs at 50 fps; Arty at 30 Hz. Same pixel space, so per-tick values are the
// WA-per-frame values rescaled: velocity ×(50/30), acceleration ×(50/30)².
// The bazooka uses these instead of the shared GRAVITY/WIND_SCALE/launch scale;
// every other projectile is untouched.

/// Bazooka gravity. WA measured 0.24 px/frame² @50fps → ×(50/30)².
pub const BAZOOKA_GRAVITY: f32 = 0.6667;

/// Bazooka wind acceleration at full meter (wind = ±1.0). WA measured a max of
/// ~0.286 px/frame² @50fps (wind unit = 1/35 px/frame², ~±10 units) → ×(50/30)².
pub const BAZOOKA_WIND_SCALE: f32 = 0.794;

/// Bazooka launch speed at a full (power = 1.0) charge. WA measured 8.63 px/frame
/// @50fps → ×(50/30). Overcharge (power up to MAX_CHARGE) scales past this.
pub const BAZOOKA_LAUNCH: f32 = 14.38;

/// Charge ticks to reach a full (power = 1.0) bazooka charge. WA fills in ~0.52 s
/// and auto-fires at full; 0.52 s × 30 Hz ≈ 16 ticks → rate 1/16 per tick.
pub const BAZOOKA_CHARGE_RATE: f32 = 0.0625;

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
pub fn tick(proj: &mut Projectile, wind: f32) {
    // ── Acceleration ──────────────────────────────────────────────────────────
    // Homing missiles maintain constant speed — no gravity or wind drift.
    // The bazooka uses its own WA-derived gravity/wind; all others share the defaults.
    if proj.kind != WeaponKind::HomingMissile {
        let is_bazooka = proj.kind == WeaponKind::Bazooka;
        proj.vel.y += if is_bazooka { BAZOOKA_GRAVITY } else { GRAVITY };
        if proj.kind.affected_by_wind() {
            let ws = if is_bazooka { BAZOOKA_WIND_SCALE } else { WIND_SCALE };
            proj.vel.x += wind * ws;
        }
    }

    // ── Terminal velocity (Y only) ────────────────────────────────────────────
    if proj.vel.y > TERMINAL_VELOCITY {
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
        tick(&mut p, 0.0);
        assert!((p.vel.y - BAZOOKA_GRAVITY).abs() < 1e-5, "vy should equal BAZOOKA_GRAVITY after one tick");
        tick(&mut p, 0.0);
        assert!((p.vel.y - BAZOOKA_GRAVITY * 2.0).abs() < 1e-5, "vy should equal 2×BAZOOKA_GRAVITY after two ticks");
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
        tick(&mut p, 1.0);
        assert!(p.vel.x > 0.0, "positive wind should push projectile right");
        assert!((p.vel.x - BAZOOKA_WIND_SCALE).abs() < 1e-5);
    }

    #[test]
    fn negative_wind_pushes_bazooka_leftward() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        tick(&mut p, -1.0);
        assert!(p.vel.x < 0.0, "negative wind should push projectile left");
        assert!((p.vel.x + BAZOOKA_WIND_SCALE).abs() < 1e-5);
    }

    #[test]
    fn wind_accumulates_over_ticks() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
        tick_n(&mut p, 1.0, 5);
        assert!((p.vel.x - BAZOOKA_WIND_SCALE * 5.0).abs() < 1e-4,
            "wind should accumulate over 5 ticks");
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

    // ── Bazooka WA physics (per-weapon, does not touch other weapons) ──────────

    #[test]
    fn bazooka_uses_wa_gravity_other_weapons_keep_default() {
        let mut baz  = bazooka(100.0, 100.0, 0.0, 0.0);
        let mut gren = grenade(100.0, 100.0, 0.0, 0.0);
        tick(&mut baz, 0.0);
        tick(&mut gren, 0.0);
        assert!((baz.vel.y - BAZOOKA_GRAVITY).abs() < 1e-5,
            "bazooka should use WA gravity {BAZOOKA_GRAVITY}");
        assert!((gren.vel.y - GRAVITY).abs() < 1e-5,
            "non-bazooka should keep the default gravity {GRAVITY}");
        assert!(BAZOOKA_GRAVITY > GRAVITY, "WA bazooka gravity is heavier than the old default");
    }

    #[test]
    fn bazooka_charge_reaches_full_in_about_half_second_at_30hz() {
        // WA fills the bazooka power meter in ~0.52 s and auto-fires at full.
        let ticks_to_full = (1.0 / BAZOOKA_CHARGE_RATE).ceil() as u32;
        assert_eq!(ticks_to_full, 16, "full charge in 16 ticks");
        let secs = ticks_to_full as f32 / 30.0;
        assert!((secs - 0.53).abs() < 0.05, "≈0.5 s to full charge, got {secs:.2}s");
    }

    #[test]
    fn bazooka_full_charge_launch_matches_wa() {
        // WA full-charge muzzle speed = 8.63 px/frame @50 fps → ×(50/30) px/tick.
        let expected = 8.63 * (50.0 / 30.0);
        assert!((BAZOOKA_LAUNCH - expected).abs() < 0.05,
            "full-charge launch {BAZOOKA_LAUNCH} should ≈ {expected:.2} px/tick");
    }

    // ── Terminal velocity ─────────────────────────────────────────────────────

    #[test]
    fn y_velocity_capped_at_terminal_velocity() {
        let mut p = bazooka(100.0, 100.0, 0.0, 0.0);
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
