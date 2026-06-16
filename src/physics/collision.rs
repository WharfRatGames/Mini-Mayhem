use crate::world::{Terrain, WorldPos};

/// The result of a collision check for one physics tick.
#[derive(Debug, Clone, PartialEq)]
pub enum CollisionResult {
    /// No collision — projectile moved freely to its new position.
    None,
    /// Hit solid terrain at this world position.
    Terrain(WorldPos),
    /// Hit the left or right hard wall at this world position.
    Wall(WorldPos),
    /// Entered the water zone at this world position.
    Water(WorldPos),
}

impl CollisionResult {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn hit_pos(&self) -> Option<WorldPos> {
        match self {
            Self::None           => None,
            Self::Terrain(p)     => Some(*p),
            Self::Wall(p)        => Some(*p),
            Self::Water(p)       => Some(*p),
        }
    }
}

/// Maximum step size in pixels for the sweep check.
/// Smaller = more accurate but more iterations.
/// At 0.5px steps a projectile moving 18px/tick needs 36 checks — fine.
const SWEEP_STEP: f32 = 0.5;

/// Check for collisions along the path from `from` to `to`.
///
/// Sweeps along the movement vector in small steps, checking each point
/// against the terrain bitmap, walls, and water zone.
/// Returns the first collision found, or `None` if the path is clear.
///
/// This prevents fast projectiles tunnelling through thin terrain.
pub fn swept_collision(
    from:    WorldPos,
    to:      WorldPos,
    terrain: &Terrain,
) -> CollisionResult {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dist = (dx * dx + dy * dy).sqrt();

    if dist < 1e-6 {
        return CollisionResult::None;
    }

    let steps = (dist / SWEEP_STEP).ceil() as u32 + 1;
    let step_x = dx / steps as f32;
    let step_y = dy / steps as f32;

    let mut x = from.x;
    let mut y = from.y;

    for _ in 0..=steps {
        let pos = WorldPos::new(x, y);

        // Water check
        if pos.y >= crate::world::WATER_Y as f32 {
            return CollisionResult::Water(pos);
        }

        // Above world top — no collision, keep going
        if pos.y < 0.0 {
            x += step_x;
            y += step_y;
            continue;
        }

        // Terrain check (solid ground OR object mask: barrels, armed mines)
        if terrain.is_blocked(pos.x as i32, pos.y as i32) {
            return CollisionResult::Terrain(pos);
        }

        x += step_x;
        y += step_y;
    }

    CollisionResult::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Heightmap, Terrain, WORLD_W, WATER_Y};

    fn empty() -> Terrain { Terrain::empty() }

    fn real(seed: u64) -> Terrain {
        Terrain::from_heightmap(&Heightmap::generate(seed))
    }

    fn at(x: f32, y: f32) -> WorldPos { WorldPos::new(x, y) }

    // ── No collision ──────────────────────────────────────────────────────────

    #[test]
    fn clear_path_in_empty_terrain_returns_none() {
        let t = empty();
        let result = swept_collision(at(100.0, 100.0), at(110.0, 105.0), &t);
        assert_eq!(result, CollisionResult::None);
    }

    #[test]
    fn horizontal_move_in_clear_air_returns_none() {
        let t = empty();
        let result = swept_collision(at(100.0, 50.0), at(200.0, 50.0), &t);
        assert_eq!(result, CollisionResult::None);
    }

    #[test]
    fn upward_move_returns_none() {
        let t = empty();
        let result = swept_collision(at(100.0, 200.0), at(100.0, 100.0), &t);
        assert_eq!(result, CollisionResult::None);
    }

    // ── Terrain collision ─────────────────────────────────────────────────────

    #[test]
    fn moving_into_solid_pixel_detects_collision() {
        let mut t = empty();
        t.set_solid(150, 100, true);
        let result = swept_collision(at(100.0, 100.0), at(200.0, 100.0), &t);
        assert!(
            matches!(result, CollisionResult::Terrain(_)),
            "should detect terrain hit, got {result:?}"
        );
    }

    #[test]
    fn terrain_collision_returns_approximate_hit_position() {
        let mut t = empty();
        // Solid column at x=150
        for y in 0..200i32 { t.set_solid(150, y, true); }
        let result = swept_collision(at(100.0, 50.0), at(200.0, 50.0), &t);
        if let CollisionResult::Terrain(pos) = result {
            assert!(
                pos.x >= 149.0 && pos.x <= 152.0,
                "hit x={} should be near x=150", pos.x
            );
        } else {
            panic!("expected terrain collision, got {result:?}");
        }
    }

    #[test]
    fn fast_projectile_does_not_tunnel_through_thin_wall() {
        let mut t = empty();
        // One pixel thin vertical wall at x=150
        for y in 0..300i32 { t.set_solid(150, y, true); }
        // Move 100px in one step — would skip x=150 without sweep
        let result = swept_collision(at(100.0, 50.0), at(200.0, 50.0), &t);
        assert!(
            matches!(result, CollisionResult::Terrain(_)),
            "fast projectile should not tunnel through thin wall"
        );
    }

    #[test]
    fn diagonal_path_detects_terrain() {
        let mut t = empty();
        t.set_solid(120, 120, true);
        let result = swept_collision(at(100.0, 100.0), at(140.0, 140.0), &t);
        assert!(matches!(result, CollisionResult::Terrain(_)));
    }

    #[test]
    fn path_ending_exactly_on_solid_detects_collision() {
        let mut t = empty();
        t.set_solid(200, 200, true);
        let result = swept_collision(at(100.0, 200.0), at(200.0, 200.0), &t);
        assert!(matches!(result, CollisionResult::Terrain(_)));
    }

    // Side-wall collision tests removed: maps are open-sided (Worms-style), so
    // swept_collision no longer returns CollisionResult::Wall — projectiles fly
    // off the left/right edges instead of bouncing/exploding on a hard wall.

    // ── Water collision ───────────────────────────────────────────────────────

    #[test]
    fn falling_into_water_detects_water() {
        let t = empty();
        let result = swept_collision(
            at(100.0, WATER_Y as f32 - 5.0),
            at(100.0, WATER_Y as f32 + 5.0),
            &t,
        );
        assert!(
            matches!(result, CollisionResult::Water(_)),
            "should detect water, got {result:?}"
        );
    }

    #[test]
    fn water_collision_position_is_at_water_line() {
        let t = empty();
        let result = swept_collision(
            at(100.0, WATER_Y as f32 - 5.0),
            at(100.0, WATER_Y as f32 + 5.0),
            &t,
        );
        if let CollisionResult::Water(pos) = result {
            assert!(
                pos.y >= WATER_Y as f32 - 1.0 && pos.y <= WATER_Y as f32 + 1.0,
                "water hit y={} should be near WATER_Y={}", pos.y, WATER_Y
            );
        } else {
            panic!("expected water collision");
        }
    }

    // ── Priority: wall and water before terrain ────────────────────────────────

    #[test]
    fn stationary_projectile_returns_none() {
        let t = empty();
        let result = swept_collision(at(100.0, 100.0), at(100.0, 100.0), &t);
        assert_eq!(result, CollisionResult::None);
    }

    #[test]
    fn no_collision_above_world_top() {
        let t = empty();
        // Moving upward above y=0 should not collide
        let result = swept_collision(at(100.0, 5.0), at(100.0, -10.0), &t);
        assert_eq!(result, CollisionResult::None);
    }

    // ── On real terrain ───────────────────────────────────────────────────────

    #[test]
    fn projectile_falling_from_sky_hits_real_terrain() {
        let t = real(42);
        // Find the surface at x=1600 (middle of map)
        let surface = t.surface_y_at(1600).expect("should have surface") as f32;
        // Drop from well above the surface
        let result = swept_collision(
            at(1600.0, surface - 50.0),
            at(1600.0, surface + 10.0),
            &t,
        );
        assert!(
            matches!(result, CollisionResult::Terrain(_)),
            "falling projectile should hit terrain at surface"
        );
    }

    #[test]
    fn projectile_in_sky_above_terrain_has_no_collision() {
        let t = real(42);
        let surface = t.surface_y_at(1600).expect("should have surface") as f32;
        // Move horizontally well above the surface
        let result = swept_collision(
            at(1500.0, surface - 80.0),
            at(1700.0, surface - 80.0),
            &t,
        );
        assert_eq!(result, CollisionResult::None);
    }

    // ── hit_pos helper ────────────────────────────────────────────────────────

    #[test]
    fn hit_pos_returns_none_for_no_collision() {
        let t = empty();
        let r = swept_collision(at(0.0, 0.0), at(1.0, 0.0), &t);
        assert!(r.hit_pos().is_none());
    }

    #[test]
    fn hit_pos_returns_some_for_water() {
        let t = empty();
        let r = swept_collision(
            at(100.0, WATER_Y as f32 - 2.0),
            at(100.0, WATER_Y as f32 + 2.0),
            &t,
        );
        assert!(r.hit_pos().is_some());
    }
}
