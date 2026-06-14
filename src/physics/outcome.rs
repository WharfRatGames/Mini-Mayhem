use crate::world::{WorldPos, Vec2};
use super::projectile::{Projectile, WeaponKind};
use super::collision::CollisionResult;

/// What should happen to a projectile after a collision is resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Projectile continues flying — no collision this tick.
    Continue,
    /// Projectile hit a wall and bounced. New velocity applied.
    Bounced,
    /// Projectile entered water. Despawn it, no explosion.
    Drowned,
    /// Projectile hit terrain or a wall without bouncing. Explode at this pos.
    Explode(WorldPos),
    /// Projectile fuse expired in mid-air. Explode at current position.
    FuseExplode(WorldPos),
}

impl Outcome {
    pub fn should_despawn(&self) -> bool {
        matches!(self, Self::Drowned | Self::Explode(_) | Self::FuseExplode(_))
    }

    pub fn explosion_pos(&self) -> Option<WorldPos> {
        match self {
            Self::Explode(p) | Self::FuseExplode(p) => Some(*p),
            _ => None,
        }
    }
}

/// Restitution when a grenade bounces vertically (hits floor/ceiling).
/// 0.5 = half energy lost per bounce → ~3 meaningful bounces before settling.
const BOUNCE_RESTITUTION: f32 = 0.32;
/// Rolling friction applied to the non-bouncing axis each bounce.
const BOUNCE_FRICTION: f32    = 0.75;
/// Below this total speed the grenade stops bouncing and just slides.
const MIN_BOUNCE_SPEED: f32   = 1.8;
/// Legacy alias used by tests.
const WALL_BOUNCE_DAMPEN: f32 = BOUNCE_RESTITUTION;

/// Deflect a bee off a terrain/wall hit so it survives and keeps navigating:
/// soften and reverse velocity, with a slight upward bias, and lift clear of the
/// surface. The steering pass re-aims it toward its target on the next tick.
fn bee_deflect(proj: &mut Projectile, hit: WorldPos) {
    proj.vel.x = -proj.vel.x * 0.4;
    proj.vel.y = -proj.vel.y * 0.4 - 0.6; // bias away from the ground
    proj.pos = WorldPos::new(hit.x, hit.y - 2.0);
}

/// Resolve what happens to `proj` given a collision result.
///
/// Mutates the projectile's position and velocity if it bounces.
/// Returns the outcome for the caller to act on (spawn explosion, despawn, etc).
pub fn resolve(proj: &mut Projectile, collision: &CollisionResult) -> Outcome {
    // Fuse expired in mid-air — explode regardless of collision
    if proj.fuse.is_expired() {
        return Outcome::FuseExplode(proj.pos);
    }

    match collision {
        CollisionResult::None => Outcome::Continue,

        CollisionResult::Water(pos) => {
            proj.pos = *pos;
            Outcome::Drowned
        }

        CollisionResult::Wall(pos) => {
            // Bees never detonate on terrain/walls — they deflect and keep flying
            // toward their target (only a soldier hit or fuse expiry detonates them).
            if proj.kind == WeaponKind::Blasthive && proj.is_fragment {
                bee_deflect(proj, *pos);
                return Outcome::Bounced;
            }
            match proj.kind {
                WeaponKind::Grenade
                | WeaponKind::ClusterBomb
                | WeaponKind::HolyHandGrenade
                | WeaponKind::Tnt => {
                    proj.vel.x = -proj.vel.x * BOUNCE_RESTITUTION;
                    proj.vel.y *= BOUNCE_FRICTION;
                    proj.pos.x = proj.pos.x.clamp(1.0, crate::world::WORLD_W as f32 - 2.0);
                    Outcome::Bounced
                }
                _ => Outcome::Explode(*pos),
            }
        }

        CollisionResult::Terrain(pos) => {
            // Bees deflect off terrain (see Wall arm) — they fly around, never tunnel.
            if proj.kind == WeaponKind::Blasthive && proj.is_fragment {
                bee_deflect(proj, *pos);
                return Outcome::Bounced;
            }
            match proj.kind {
                WeaponKind::Grenade
                | WeaponKind::ClusterBomb
                | WeaponKind::HolyHandGrenade
                | WeaponKind::Tnt => {
                    let speed = (proj.vel.x * proj.vel.x + proj.vel.y * proj.vel.y).sqrt();
                    if speed < MIN_BOUNCE_SPEED {
                        // Fuse weapons settle and slide until fuse expires
                        proj.vel.y = 0.0;
                        proj.vel.x *= 0.7;
                        proj.pos = WorldPos::new(pos.x, pos.y - 1.0);
                    } else if proj.vel.y > 0.0 && proj.vel.y.abs() >= proj.vel.x.abs() * 1.2 {
                        // Mostly-downward impact into a floor — flip Y, slide X.
                        // Threshold 1.2 means vy must dominate vx to be called a floor;
                        // lower values incorrectly classify horizontal wall hits as floors
                        // and cause upward climbing.
                        proj.vel.y = -proj.vel.y * BOUNCE_RESTITUTION;
                        proj.vel.x *= BOUNCE_FRICTION;
                        proj.pos = WorldPos::new(pos.x, pos.y - 1.0);
                    } else if proj.vel.y < 0.0 && proj.vel.y.abs() >= proj.vel.x.abs() * 1.2 {
                        // Mostly-upward into a ceiling — flip Y downward, slide X.
                        proj.vel.y = proj.vel.y.abs() * BOUNCE_RESTITUTION;
                        proj.vel.x *= BOUNCE_FRICTION;
                        proj.pos = WorldPos::new(pos.x, pos.y + 1.0);
                    } else {
                        // Side / wall impact — flip X, push clear in x direction.
                        // Do NOT change pos.y here; an upward y-push is what causes
                        // the grenade to climb vertical surfaces.
                        proj.vel.x = -proj.vel.x * BOUNCE_RESTITUTION;
                        proj.vel.y *= BOUNCE_FRICTION;
                        let push_x = if proj.vel.x > 0.0 { 4.0 } else { -4.0 };
                        proj.pos = WorldPos::new(pos.x + push_x, pos.y);
                    }
                    Outcome::Bounced
                }
                _ => Outcome::Explode(*pos),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::WorldPos;
    use crate::physics::projectile::FuseState;

    fn pos() -> WorldPos { WorldPos::new(100.0, 100.0) }
    fn vel() -> Vec2     { Vec2::new(5.0, 3.0) }

    fn make(kind: WeaponKind) -> Projectile {
        Projectile::new(pos(), vel(), kind)
    }

    fn make_expired_fuse(kind: WeaponKind) -> Projectile {
        let mut p = make(kind);
        p.fuse = FuseState::Expired;
        p
    }

    // ── No collision ──────────────────────────────────────────────────────────

    #[test]
    fn no_collision_returns_continue() {
        let mut p = make(WeaponKind::Bazooka);
        let out = resolve(&mut p, &CollisionResult::None);
        assert_eq!(out, Outcome::Continue);
    }

    // ── Fuse expiry ───────────────────────────────────────────────────────────

    #[test]
    fn expired_fuse_returns_fuse_explode() {
        let mut p = make_expired_fuse(WeaponKind::Grenade);
        let out = resolve(&mut p, &CollisionResult::None);
        assert!(matches!(out, Outcome::FuseExplode(_)));
    }

    #[test]
    fn expired_fuse_takes_priority_over_no_collision() {
        let mut p = make_expired_fuse(WeaponKind::Grenade);
        let out = resolve(&mut p, &CollisionResult::None);
        assert!(out.explosion_pos().is_some());
    }

    // ── Water ─────────────────────────────────────────────────────────────────

    #[test]
    fn water_collision_returns_drowned() {
        let mut p = make(WeaponKind::Bazooka);
        let hit = WorldPos::new(100.0, 465.0);
        let out = resolve(&mut p, &CollisionResult::Water(hit));
        assert_eq!(out, Outcome::Drowned);
    }

    #[test]
    fn drowned_projectile_should_despawn() {
        assert!(Outcome::Drowned.should_despawn());
    }

    #[test]
    fn water_moves_projectile_to_hit_position() {
        let mut p = make(WeaponKind::Bazooka);
        let hit = WorldPos::new(200.0, 470.0);
        resolve(&mut p, &CollisionResult::Water(hit));
        assert_eq!(p.pos.x, 200.0);
        assert_eq!(p.pos.y, 470.0);
    }

    // ── Wall bounce ───────────────────────────────────────────────────────────

    #[test]
    fn grenade_bounces_off_wall() {
        let mut p = make(WeaponKind::Grenade);
        let hit = WorldPos::new(0.0, 100.0);
        let out = resolve(&mut p, &CollisionResult::Wall(hit));
        assert_eq!(out, Outcome::Bounced);
    }

    #[test]
    fn grenade_wall_bounce_reverses_x_velocity() {
        let mut p = Projectile::new(pos(), Vec2::new(5.0, 2.0), WeaponKind::Grenade);
        resolve(&mut p, &CollisionResult::Wall(WorldPos::new(0.0, 100.0)));
        assert!(p.vel.x < 0.0, "x velocity should reverse after wall bounce");
    }

    #[test]
    fn grenade_wall_bounce_dampens_y_velocity() {
        let mut p = Projectile::new(pos(), Vec2::new(5.0, 4.0), WeaponKind::Grenade);
        let vy_before = p.vel.y;
        resolve(&mut p, &CollisionResult::Wall(WorldPos::new(0.0, 100.0)));
        assert!(p.vel.y.abs() < vy_before.abs(), "y should be reduced after wall bounce");
    }

    #[test]
    fn bazooka_explodes_on_wall() {
        let mut p = make(WeaponKind::Bazooka);
        let hit = WorldPos::new(0.0, 100.0);
        let out = resolve(&mut p, &CollisionResult::Wall(hit));
        assert!(matches!(out, Outcome::Explode(_)));
    }

    #[test]
    fn shotgun_explodes_on_wall() {
        let mut p = make(WeaponKind::Shotgun);
        let out = resolve(&mut p, &CollisionResult::Wall(WorldPos::new(0.0, 100.0)));
        assert!(matches!(out, Outcome::Explode(_)));
    }

    // ── Terrain bounce ────────────────────────────────────────────────────────

    #[test]
    fn grenade_bounces_off_terrain() {
        let mut p = make(WeaponKind::Grenade);
        let hit = WorldPos::new(100.0, 300.0);
        let out = resolve(&mut p, &CollisionResult::Terrain(hit));
        assert_eq!(out, Outcome::Bounced);
    }

    #[test]
    fn grenade_terrain_bounce_reverses_y_velocity() {
        // Grenade falling into floor (vy > 0, dominantly vertical) → vy should flip negative
        let mut p = Projectile::new(pos(), Vec2::new(3.0, 8.0), WeaponKind::Grenade);
        resolve(&mut p, &CollisionResult::Terrain(WorldPos::new(100.0, 300.0)));
        assert!(p.vel.y < 0.0, "y velocity should reverse on floor bounce");
    }

    #[test]
    fn grenade_bounce_pushes_above_impact() {
        let mut p = make(WeaponKind::Grenade);
        let hit = WorldPos::new(100.0, 300.0);
        resolve(&mut p, &CollisionResult::Terrain(hit));
        assert!(p.pos.y < 300.0, "grenade should be pushed above impact point");
    }

    #[test]
    fn bazooka_explodes_on_terrain() {
        let mut p = make(WeaponKind::Bazooka);
        let hit = WorldPos::new(100.0, 300.0);
        let out = resolve(&mut p, &CollisionResult::Terrain(hit));
        assert!(matches!(out, Outcome::Explode(_)));
    }

    #[test]
    fn homing_missile_explodes_on_terrain() {
        let mut p = make(WeaponKind::HomingMissile);
        let out = resolve(&mut p, &CollisionResult::Terrain(WorldPos::new(100.0, 300.0)));
        assert!(matches!(out, Outcome::Explode(_)));
    }

    // ── Outcome helpers ───────────────────────────────────────────────────────

    #[test]
    fn explode_should_despawn() {
        assert!(Outcome::Explode(pos()).should_despawn());
    }

    #[test]
    fn fuse_explode_should_despawn() {
        assert!(Outcome::FuseExplode(pos()).should_despawn());
    }

    #[test]
    fn continue_should_not_despawn() {
        assert!(!Outcome::Continue.should_despawn());
    }

    #[test]
    fn bounced_should_not_despawn() {
        assert!(!Outcome::Bounced.should_despawn());
    }

    #[test]
    fn explode_returns_position() {
        let p = WorldPos::new(200.0, 300.0);
        assert_eq!(Outcome::Explode(p).explosion_pos(), Some(p));
    }

    #[test]
    fn continue_returns_no_position() {
        assert_eq!(Outcome::Continue.explosion_pos(), None);
    }

    #[test]
    fn drowned_returns_no_explosion_pos() {
        assert_eq!(Outcome::Drowned.explosion_pos(), None);
    }
}
