use crate::world::{WorldPos, Vec2, Terrain};
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

/// Below this total speed the grenade stops bouncing and just slides.
const MIN_BOUNCE_SPEED: f32   = 1.8;
/// Legacy alias kept so wall-bounce code compiles without changes.
const WALL_BOUNCE_DAMPEN: f32 = 0.32;
/// Restitution for wall (left/right edge) bounces — kept at old value.
const BOUNCE_RESTITUTION: f32 = 0.32;
/// Friction for wall bounces on the non-bouncing axis.
const BOUNCE_FRICTION: f32    = 0.75;

/// Worms-style surface normal from a 5×5 pixel gradient around the hit point.
/// Returns a unit vector pointing away from solid terrain.
fn terrain_normal(terrain: &Terrain, pos: WorldPos) -> (f32, f32) {
    let x = pos.x as i32;
    let y = pos.y as i32;
    let mut nx = 0.0f32;
    let mut ny = 0.0f32;
    for ky in -2i32..=2 {
        for kx in -2i32..=2 {
            if kx == 0 && ky == 0 { continue; }
            let s = terrain.is_blocked(x + kx, y + ky) as i32 as f32;
            nx -= kx as f32 * s;
            ny -= ky as f32 * s;
        }
    }
    let len = (nx * nx + ny * ny).sqrt();
    if len < 0.001 { (0.0, -1.0) } else { (nx / len, ny / len) }
}

/// Worms-style bounce: reflect off terrain normal, apply restitution on normal
/// component and friction on tangential component. Returns true if settled.
fn worms_bounce(proj: &mut Projectile, pos: WorldPos, terrain: &Terrain) {
    let speed = (proj.vel.x * proj.vel.x + proj.vel.y * proj.vel.y).sqrt();
    if speed < MIN_BOUNCE_SPEED {
        proj.vel.x = 0.0;
        proj.vel.y = 0.0;
        let (nx, ny) = terrain_normal(terrain, pos);
        proj.pos = WorldPos::new(pos.x + nx, pos.y + ny);
        return;
    }
    let (nx, ny) = terrain_normal(terrain, pos);
    let dot = proj.vel.x * nx + proj.vel.y * ny; // negative = moving into surface
    // Reflected velocity: v - 2*(v·n)*n
    let rx = proj.vel.x - 2.0 * dot * nx;
    let ry = proj.vel.y - 2.0 * dot * ny;
    // Split into outward-normal and tangential components
    let v_n_x = -dot * nx;
    let v_n_y = -dot * ny;
    let v_t_x = rx - v_n_x;
    let v_t_y = ry - v_n_y;
    const RESTITUTION: f32 = 0.30;
    const FRICTION: f32    = 0.80;
    proj.vel.x = v_n_x * RESTITUTION + v_t_x * FRICTION;
    proj.vel.y = v_n_y * RESTITUTION + v_t_y * FRICTION;
    // Push out along normal so the projectile clears the surface
    proj.pos = WorldPos::new(pos.x + nx * 2.0, pos.y + ny * 2.0);
}

/// Deflect a bee off a terrain/wall hit using proper normal reflection.
fn bee_deflect(proj: &mut Projectile, hit: WorldPos, terrain: &Terrain) {
    let (nx, ny) = terrain_normal(terrain, hit);
    let dot = proj.vel.x * nx + proj.vel.y * ny;
    proj.vel.x = (proj.vel.x - 2.0 * dot * nx) * 0.45;
    proj.vel.y = (proj.vel.y - 2.0 * dot * ny) * 0.45 - 0.4; // slight upward bias
    proj.pos = WorldPos::new(hit.x + nx * 2.0, hit.y + ny * 2.0);
}

/// Resolve what happens to `proj` given a collision result.
///
/// Mutates the projectile's position and velocity if it bounces.
/// Returns the outcome for the caller to act on (spawn explosion, despawn, etc).
pub fn resolve(proj: &mut Projectile, collision: &CollisionResult, terrain: &Terrain) -> Outcome {
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
                bee_deflect(proj, *pos, terrain);
                return Outcome::Bounced;
            }
            match proj.kind {
                WeaponKind::Grenade | WeaponKind::HolyHandGrenade => {
                    // Worms-style: simple x-flip for flat vertical world edges
                    proj.vel.x = -proj.vel.x * BOUNCE_RESTITUTION;
                    proj.vel.y *= BOUNCE_FRICTION;
                    proj.pos.x = proj.pos.x.clamp(1.0, crate::world::WORLD_W as f32 - 2.0);
                    Outcome::Bounced
                }
                WeaponKind::ClusterBomb | WeaponKind::Tnt => {
                    proj.vel.x = -proj.vel.x * BOUNCE_RESTITUTION;
                    proj.vel.y *= BOUNCE_FRICTION;
                    proj.pos.x = proj.pos.x.clamp(1.0, crate::world::WORLD_W as f32 - 2.0);
                    Outcome::Bounced
                }
                _ => Outcome::Explode(*pos),
            }
        }

        CollisionResult::Terrain(pos) => {
            if proj.kind == WeaponKind::Blasthive && proj.is_fragment {
                bee_deflect(proj, *pos, terrain);
                return Outcome::Bounced;
            }
            match proj.kind {
                WeaponKind::Grenade | WeaponKind::HolyHandGrenade => {
                    worms_bounce(proj, *pos, terrain);
                    Outcome::Bounced
                }
                WeaponKind::ClusterBomb | WeaponKind::Tnt => {
                    let speed = (proj.vel.x * proj.vel.x + proj.vel.y * proj.vel.y).sqrt();
                    if speed < MIN_BOUNCE_SPEED {
                        proj.vel.y = 0.0;
                        proj.vel.x *= 0.7;
                        proj.pos = WorldPos::new(pos.x, pos.y - 1.0);
                    } else if proj.vel.y > 0.0 && proj.vel.y.abs() >= proj.vel.x.abs() * 1.2 {
                        proj.vel.y = -proj.vel.y * BOUNCE_RESTITUTION;
                        proj.vel.x *= BOUNCE_FRICTION;
                        proj.pos = WorldPos::new(pos.x, pos.y - 1.0);
                    } else if proj.vel.y < 0.0 && proj.vel.y.abs() >= proj.vel.x.abs() * 1.2 {
                        proj.vel.y = proj.vel.y.abs() * BOUNCE_RESTITUTION;
                        proj.vel.x *= BOUNCE_FRICTION;
                        proj.pos = WorldPos::new(pos.x, pos.y + 1.0);
                    } else {
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
    use crate::world::{WorldPos, Terrain};
    use crate::physics::projectile::FuseState;

    fn pos() -> WorldPos { WorldPos::new(100.0, 100.0) }
    fn vel() -> Vec2     { Vec2::new(5.0, 3.0) }
    fn terrain() -> Terrain { Terrain::empty() }

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
        let out = resolve(&mut p, &CollisionResult::None, &terrain());
        assert_eq!(out, Outcome::Continue);
    }

    // ── Fuse expiry ───────────────────────────────────────────────────────────

    #[test]
    fn expired_fuse_returns_fuse_explode() {
        let mut p = make_expired_fuse(WeaponKind::Grenade);
        let out = resolve(&mut p, &CollisionResult::None, &terrain());
        assert!(matches!(out, Outcome::FuseExplode(_)));
    }

    #[test]
    fn expired_fuse_takes_priority_over_no_collision() {
        let mut p = make_expired_fuse(WeaponKind::Grenade);
        let out = resolve(&mut p, &CollisionResult::None, &terrain());
        assert!(out.explosion_pos().is_some());
    }

    // ── Water ─────────────────────────────────────────────────────────────────

    #[test]
    fn water_collision_returns_drowned() {
        let mut p = make(WeaponKind::Bazooka);
        let hit = WorldPos::new(100.0, 465.0);
        let out = resolve(&mut p, &CollisionResult::Water(hit), &terrain());
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
        resolve(&mut p, &CollisionResult::Water(hit), &terrain());
        assert_eq!(p.pos.x, 200.0);
        assert_eq!(p.pos.y, 470.0);
    }

    // ── Wall bounce ───────────────────────────────────────────────────────────

    #[test]
    fn grenade_bounces_off_wall() {
        let mut p = make(WeaponKind::Grenade);
        let hit = WorldPos::new(0.0, 100.0);
        let out = resolve(&mut p, &CollisionResult::Wall(hit), &terrain());
        assert_eq!(out, Outcome::Bounced);
    }

    #[test]
    fn grenade_wall_bounce_reverses_x_velocity() {
        let mut p = Projectile::new(pos(), Vec2::new(5.0, 2.0), WeaponKind::Grenade);
        resolve(&mut p, &CollisionResult::Wall(WorldPos::new(0.0, 100.0)), &terrain());
        assert!(p.vel.x < 0.0, "x velocity should reverse after wall bounce");
    }

    #[test]
    fn grenade_wall_bounce_dampens_y_velocity() {
        let mut p = Projectile::new(pos(), Vec2::new(5.0, 4.0), WeaponKind::Grenade);
        let vy_before = p.vel.y;
        resolve(&mut p, &CollisionResult::Wall(WorldPos::new(0.0, 100.0)), &terrain());
        assert!(p.vel.y.abs() < vy_before.abs(), "y should be reduced after wall bounce");
    }

    #[test]
    fn bazooka_explodes_on_wall() {
        let mut p = make(WeaponKind::Bazooka);
        let hit = WorldPos::new(0.0, 100.0);
        let out = resolve(&mut p, &CollisionResult::Wall(hit), &terrain());
        assert!(matches!(out, Outcome::Explode(_)));
    }

    #[test]
    fn shotgun_explodes_on_wall() {
        let mut p = make(WeaponKind::Shotgun);
        let out = resolve(&mut p, &CollisionResult::Wall(WorldPos::new(0.0, 100.0)), &terrain());
        assert!(matches!(out, Outcome::Explode(_)));
    }

    // ── Terrain bounce ────────────────────────────────────────────────────────

    #[test]
    fn grenade_bounces_off_terrain() {
        let mut p = make(WeaponKind::Grenade);
        let hit = WorldPos::new(100.0, 300.0);
        let out = resolve(&mut p, &CollisionResult::Terrain(hit), &terrain());
        assert_eq!(out, Outcome::Bounced);
    }

    #[test]
    fn grenade_terrain_bounce_reverses_y_velocity() {
        // Grenade falling into floor (vy > 0, dominantly vertical) → vy should flip negative
        let mut p = Projectile::new(pos(), Vec2::new(3.0, 8.0), WeaponKind::Grenade);
        resolve(&mut p, &CollisionResult::Terrain(WorldPos::new(100.0, 300.0)), &terrain());
        assert!(p.vel.y < 0.0, "y velocity should reverse on floor bounce");
    }

    #[test]
    fn grenade_bounce_pushes_above_impact() {
        // Vertical-dominant velocity so the impact is classified as a floor hit
        // (the branch that repositions the grenade just above the impact point).
        let mut p = Projectile::new(pos(), Vec2::new(2.0, 8.0), WeaponKind::Grenade);
        let hit = WorldPos::new(100.0, 300.0);
        resolve(&mut p, &CollisionResult::Terrain(hit), &terrain());
        assert!(p.pos.y < 300.0, "grenade should be pushed above impact point");
    }

    #[test]
    fn bazooka_explodes_on_terrain() {
        let mut p = make(WeaponKind::Bazooka);
        let hit = WorldPos::new(100.0, 300.0);
        let out = resolve(&mut p, &CollisionResult::Terrain(hit), &terrain());
        assert!(matches!(out, Outcome::Explode(_)));
    }

    #[test]
    fn homing_missile_explodes_on_terrain() {
        let mut p = make(WeaponKind::HomingMissile);
        let out = resolve(&mut p, &CollisionResult::Terrain(WorldPos::new(100.0, 300.0)), &terrain());
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
