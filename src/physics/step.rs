use crate::world::{Terrain, WorldPos};
use super::projectile::{Projectile, FuseState, WeaponKind};
use super::tick::tick;
use super::collision::swept_collision;
use super::outcome::{resolve, Outcome};

/// The result of stepping one projectile forward by one physics tick.
#[derive(Debug, Clone, PartialEq)]
pub enum StepResult {
    /// Projectile is still in flight. New position is in proj.pos.
    Flying,
    /// Projectile bounced off terrain or a wall. Still alive.
    Bounced,
    /// Projectile hit terrain or wall and should explode at this position.
    Explode(WorldPos),
    /// Projectile fuse expired mid-air. Explode at this position.
    FuseExplode(WorldPos),
    /// Projectile entered water. Despawn silently, no explosion.
    Drowned,
    /// Projectile exceeded maximum age. Despawn silently.
    Expired,
    /// HHG just came to rest — emit hallelujah sound. Projectile stays alive.
    HHGArmed,
}

impl StepResult {
    /// Returns true if the projectile should be removed from the world.
    pub fn should_despawn(&self) -> bool {
        matches!(
            self,
            Self::Explode(_) | Self::FuseExplode(_) | Self::Drowned | Self::Expired
        )
    }

    /// Returns the explosion position if this result triggers one.
    pub fn explosion_pos(&self) -> Option<WorldPos> {
        match self {
            Self::Explode(p) | Self::FuseExplode(p) => Some(*p),
            _ => None,
        }
    }
}

/// Advance one projectile by one physics tick.
///
/// Order of operations:
///   1. Check if already expired (age or fuse) before doing anything
///   2. Save current position
///   3. Apply gravity, wind, update position (tick)
///   4. Swept collision check from old pos to new pos
///   5. Resolve outcome (bounce, explode, drown, continue)
///
/// `wind` is in [-1.0, 1.0].
pub fn step_projectile(
    proj:    &mut Projectile,
    terrain: &Terrain,
    wind:    f32,
) -> StepResult {
    // Already expired before we even start this tick
    if proj.is_expired() {
        return StepResult::Expired;
    }

    let pos_before  = proj.pos;
    let fuse_before = proj.fuse;

    // Apply physics (updates pos, vel, age, fuse)
    tick(proj, wind);

    // Check if fuse just expired this tick
    if proj.fuse.is_expired() {
        // Only redirect to Armed if the fuse burned down normally (Burning → Expired).
        // If Detonating counted down to Expired, that's the actual detonation.
        if proj.kind == WeaponKind::HolyHandGrenade
            && !matches!(fuse_before, FuseState::Detonating(_))
        {
            proj.fuse = FuseState::Armed;
        } else {
            return StepResult::FuseExplode(proj.pos);
        }
    }

    // Check if age limit hit this tick
    if proj.age_ticks >= Projectile::MAX_AGE_TICKS {
        // Armed HHG that timed out still explodes
        if proj.fuse == FuseState::Armed {
            return StepResult::FuseExplode(proj.pos);
        }
        return StepResult::Expired;
    }

    // Swept collision from where we were to where we are now
    let collision = swept_collision(pos_before, proj.pos, terrain);

    // Resolve what happens
    let step = match resolve(proj, &collision) {
        Outcome::Continue  => StepResult::Flying,
        Outcome::Bounced   => StepResult::Bounced,
        Outcome::Drowned   => StepResult::Drowned,
        Outcome::Explode(p)      => StepResult::Explode(p),
        Outcome::FuseExplode(p) => StepResult::FuseExplode(p),
    };

    // HHG armed: check if it has come to rest (vel.y ≈ 0 = on ground, speed < 1)
    if proj.fuse == FuseState::Armed {
        let speed = (proj.vel.x * proj.vel.x + proj.vel.y * proj.vel.y).sqrt();
        if speed < 1.0 && proj.vel.y.abs() < 0.5 {
            proj.fuse = FuseState::Detonating(15); // 0.5 s at 30 Hz
            return StepResult::HHGArmed;
        }
    }

    step
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Heightmap, Terrain, Vec2, WorldPos, WATER_Y, WORLD_W};
    use crate::physics::projectile::{FuseState, WeaponKind};

    fn empty() -> Terrain { Terrain::empty() }
    fn real(seed: u64) -> Terrain {
        Terrain::from_heightmap(&Heightmap::generate(seed))
    }

    fn bazooka(x: f32, y: f32, vx: f32, vy: f32) -> Projectile {
        Projectile::new(WorldPos::new(x, y), Vec2::new(vx, vy), WeaponKind::Bazooka)
    }

    fn grenade(x: f32, y: f32, vx: f32, vy: f32) -> Projectile {
        Projectile::new(WorldPos::new(x, y), Vec2::new(vx, vy), WeaponKind::Grenade)
    }

    // ── Flying ────────────────────────────────────────────────────────────────

    #[test]
    fn projectile_in_clear_air_returns_flying() {
        let mut p = bazooka(100.0, 50.0, 5.0, 0.0);
        let result = step_projectile(&mut p, &empty(), 0.0);
        assert_eq!(result, StepResult::Flying);
    }

    #[test]
    fn flying_projectile_is_not_despawned() {
        assert!(!StepResult::Flying.should_despawn());
    }

    #[test]
    fn position_advances_each_step() {
        let mut p = bazooka(100.0, 50.0, 5.0, 0.0);
        let x0 = p.pos.x;
        step_projectile(&mut p, &empty(), 0.0);
        assert!(p.pos.x > x0, "position should advance");
    }

    // ── Terrain hit ───────────────────────────────────────────────────────────

    #[test]
    fn bazooka_hitting_terrain_returns_explode() {
        let mut t = empty();
        // Solid wall ahead
        for y in 0..300i32 { t.set_solid(120, y, true); }
        let mut p = bazooka(100.0, 50.0, 10.0, 0.0);
        // Step until we hit or give up
        let mut result = StepResult::Flying;
        for _ in 0..20 {
            result = step_projectile(&mut p, &t, 0.0);
            if result != StepResult::Flying { break; }
        }
        assert!(
            matches!(result, StepResult::Explode(_)),
            "bazooka should explode on terrain, got {result:?}"
        );
    }

    #[test]
    fn grenade_hitting_terrain_returns_bounced() {
        let mut t = empty();
        for x in 0..300i32 { t.set_solid(x, 200, true); }
        let mut p = grenade(100.0, 180.0, 2.0, 5.0);
        let mut result = StepResult::Flying;
        for _ in 0..20 {
            result = step_projectile(&mut p, &t, 0.0);
            if result != StepResult::Flying { break; }
        }
        assert!(
            matches!(result, StepResult::Bounced),
            "grenade should bounce on terrain, got {result:?}"
        );
    }

    // ── Water ─────────────────────────────────────────────────────────────────

    #[test]
    fn projectile_falling_into_water_returns_drowned() {
        let mut p = bazooka(100.0, WATER_Y as f32 - 3.0, 0.0, 5.0);
        let result = step_projectile(&mut p, &empty(), 0.0);
        assert_eq!(result, StepResult::Drowned);
    }

    #[test]
    fn drowned_should_despawn() {
        assert!(StepResult::Drowned.should_despawn());
    }

    // Side-wall step tests removed: open-sided maps mean projectiles at the map
    // edge keep Flying (no hard wall to explode/bounce against).

    // ── Fuse expiry ───────────────────────────────────────────────────────────

    #[test]
    fn grenade_with_expired_fuse_returns_fuse_explode() {
        let mut p = grenade(100.0, 100.0, 2.0, -5.0);
        p.fuse = FuseState::Burning(0);
        let result = step_projectile(&mut p, &empty(), 0.0);
        assert!(
            matches!(result, StepResult::FuseExplode(_)),
            "grenade fuse should expire and explode, got {result:?}"
        );
    }

    #[test]
    fn fuse_explode_should_despawn() {
        assert!(StepResult::FuseExplode(WorldPos::new(0.0, 0.0)).should_despawn());
    }

    #[test]
    fn grenade_counts_down_fuse_over_ticks() {
        let mut p = grenade(100.0, 100.0, 1.0, 0.0);
        // Should fly for many ticks before fuse expires
        let mut exploded = false;
        for _ in 0..100 {
            let r = step_projectile(&mut p, &empty(), 0.0);
            if matches!(r, StepResult::FuseExplode(_)) {
                exploded = true;
                break;
            }
        }
        assert!(exploded, "grenade should eventually fuse-explode");
    }

    // ── Age expiry ────────────────────────────────────────────────────────────

    #[test]
    fn projectile_at_max_age_returns_expired() {
        let mut p = bazooka(100.0, 50.0, 1.0, 0.0);
        p.age_ticks = Projectile::MAX_AGE_TICKS;
        let result = step_projectile(&mut p, &empty(), 0.0);
        assert_eq!(result, StepResult::Expired);
    }

    #[test]
    fn expired_should_despawn() {
        assert!(StepResult::Expired.should_despawn());
    }

    // ── Explosion position ────────────────────────────────────────────────────

    #[test]
    fn explode_result_carries_position() {
        let mut t = empty();
        for y in 0..300i32 { t.set_solid(120, y, true); }
        let mut p = bazooka(100.0, 50.0, 15.0, 0.0);
        let mut result = StepResult::Flying;
        for _ in 0..20 {
            result = step_projectile(&mut p, &t, 0.0);
            if result.should_despawn() { break; }
        }
        assert!(result.explosion_pos().is_some());
    }

    #[test]
    fn flying_has_no_explosion_pos() {
        assert!(StepResult::Flying.explosion_pos().is_none());
    }

    #[test]
    fn drowned_has_no_explosion_pos() {
        assert!(StepResult::Drowned.explosion_pos().is_none());
    }

    // ── On real terrain ───────────────────────────────────────────────────────

    #[test]
    fn bazooka_fired_at_terrain_eventually_explodes() {
        let t = real(42);
        let surface = t.surface_y_at(1600).unwrap() as f32;
        // Fire downward into terrain
        let mut p = bazooka(1600.0, surface - 60.0, 1.0, 8.0);
        let mut result = StepResult::Flying;
        for _ in 0..60 {
            result = step_projectile(&mut p, &t, 0.0);
            if result.should_despawn() { break; }
        }
        assert!(
            matches!(result, StepResult::Explode(_)),
            "bazooka should hit real terrain and explode, got {result:?}"
        );
    }

    #[test]
    fn wind_affects_projectile_trajectory() {
        // Fire two bazookas identically, one with strong wind
        let mut p_no_wind   = bazooka(100.0, 100.0, 5.0, -8.0);
        let mut p_with_wind = bazooka(100.0, 100.0, 5.0, -8.0);

        for _ in 0..40 {
            step_projectile(&mut p_no_wind,   &empty(), 0.0);
            step_projectile(&mut p_with_wind, &empty(), 1.0);
        }

        assert!(
            p_with_wind.pos.x > p_no_wind.pos.x,
            "positive wind should push projectile further right"
        );
    }
}
