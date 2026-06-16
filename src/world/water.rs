use super::constants::*;
use super::coords::WorldPos;

/// Answers questions about the water kill zone and hard walls.
/// These are stateless — just geometry checks against world constants.
pub struct World;

impl World {
    /// Returns true if pos has entered the water zone.
    /// Water is instant death — no fall damage calc, just drown.
    pub fn in_water(pos: WorldPos) -> bool {
        pos.y >= WATER_Y as f32
    }

    /// Returns true if pos has hit the left or right hard wall.
    pub fn hit_wall(pos: WorldPos) -> bool {
        pos.x < 0.0 || pos.x >= WORLD_W as f32
    }

    /// Returns true if pos has left the world entirely (wall or above/below).
    pub fn out_of_bounds(pos: WorldPos) -> bool {
        !pos.in_bounds()
    }

    /// Clamp a position to stay within the horizontal walls.
    /// Y is not clamped — things can fall into water.
    pub fn clamp_to_walls(pos: WorldPos) -> WorldPos {
        WorldPos::new(
            pos.x.clamp(0.0, (WORLD_W - 1) as f32),
            pos.y,
        )
    }

    /// The Y coordinate where water begins.
    pub fn water_y() -> f32 {
        WATER_Y as f32
    }

    /// The right wall X coordinate (exclusive — at this x you are out).
    pub fn right_wall_x() -> f32 {
        WORLD_W as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Water zone ────────────────────────────────────────────────────────────

    #[test]
    fn above_water_line_is_not_water() {
        let pos = WorldPos::new(100.0, WATER_Y as f32 - 1.0);
        assert!(!World::in_water(pos));
    }

    #[test]
    fn at_water_line_is_water() {
        let pos = WorldPos::new(100.0, WATER_Y as f32);
        assert!(World::in_water(pos));
    }

    #[test]
    fn below_water_line_is_water() {
        let pos = WorldPos::new(100.0, (WORLD_H - 1) as f32);
        assert!(World::in_water(pos));
    }

    #[test]
    fn water_y_constant_is_correct() {
        assert_eq!(World::water_y(), WATER_Y as f32);
        assert_eq!(WATER_Y + WATER_ROWS, WORLD_H);
    }

    #[test]
    fn water_zone_spans_full_width() {
        // Water doesn't care about X
        assert!(World::in_water(WorldPos::new(0.0, WATER_Y as f32)));
        assert!(World::in_water(WorldPos::new((WORLD_W - 1) as f32, WATER_Y as f32)));
        assert!(World::in_water(WorldPos::new(1600.0, WATER_Y as f32)));
    }

    // ── Hard walls ───────────────────────────────────────────────────────────

    #[test]
    fn inside_left_wall_is_not_a_wall_hit() {
        assert!(!World::hit_wall(WorldPos::new(0.0, 100.0)));
        assert!(!World::hit_wall(WorldPos::new(1.0, 100.0)));
    }

    #[test]
    fn outside_left_wall_is_a_wall_hit() {
        assert!(World::hit_wall(WorldPos::new(-1.0, 100.0)));
        assert!(World::hit_wall(WorldPos::new(-100.0, 100.0)));
    }

    #[test]
    fn inside_right_wall_is_not_a_wall_hit() {
        assert!(!World::hit_wall(WorldPos::new((WORLD_W - 1) as f32, 100.0)));
        assert!(!World::hit_wall(WorldPos::new((WORLD_W - 2) as f32, 100.0)));
    }

    #[test]
    fn at_right_wall_is_a_wall_hit() {
        assert!(World::hit_wall(WorldPos::new(WORLD_W as f32, 100.0)));
        assert!(World::hit_wall(WorldPos::new(WORLD_W as f32 + 100.0, 100.0)));
    }

    #[test]
    fn walls_span_full_height() {
        // Walls don't care about Y
        assert!(World::hit_wall(WorldPos::new(-1.0, 0.0)));
        assert!(World::hit_wall(WorldPos::new(-1.0, (WORLD_H - 1) as f32)));
        assert!(World::hit_wall(WorldPos::new(WORLD_W as f32, 0.0)));
        assert!(World::hit_wall(WorldPos::new(WORLD_W as f32, (WORLD_H - 1) as f32)));
    }

    // ── Clamp to walls ───────────────────────────────────────────────────────

    #[test]
    fn clamp_keeps_valid_x_unchanged() {
        let pos = WorldPos::new(500.0, 200.0);
        let clamped = World::clamp_to_walls(pos);
        assert_eq!(clamped.x, 500.0);
        assert_eq!(clamped.y, 200.0);
    }

    #[test]
    fn clamp_pulls_left_overshoot_to_zero() {
        let clamped = World::clamp_to_walls(WorldPos::new(-50.0, 200.0));
        assert_eq!(clamped.x, 0.0);
        assert_eq!(clamped.y, 200.0);
    }

    #[test]
    fn clamp_pulls_right_overshoot_to_max() {
        let clamped = World::clamp_to_walls(WorldPos::new(9999.0, 200.0));
        assert_eq!(clamped.x, (WORLD_W - 1) as f32);
    }

    #[test]
    fn clamp_does_not_affect_y() {
        // A projectile falling into water should not have its Y clamped
        let pos = WorldPos::new(-10.0, WATER_Y as f32 + 5.0);
        let clamped = World::clamp_to_walls(pos);
        assert_eq!(clamped.x, 0.0);
        assert_eq!(clamped.y, WATER_Y as f32 + 5.0); // Y unchanged
    }

    // ── Out of bounds ────────────────────────────────────────────────────────

    #[test]
    fn valid_position_is_not_out_of_bounds() {
        assert!(!World::out_of_bounds(WorldPos::new(0.0, 0.0)));
        assert!(!World::out_of_bounds(WorldPos::new(1600.0, 240.0)));
        assert!(!World::out_of_bounds(WorldPos::new(WORLD_W as f32 - 1.0, WORLD_H as f32 - 1.0)));
    }

    #[test]
    fn wall_positions_are_out_of_bounds() {
        assert!(World::out_of_bounds(WorldPos::new(-1.0, 100.0)));
        assert!(World::out_of_bounds(WorldPos::new(3200.0, 100.0)));
    }

    #[test]
    fn above_world_is_out_of_bounds() {
        assert!(World::out_of_bounds(WorldPos::new(100.0, -1.0)));
    }

    #[test]
    fn water_zone_is_still_in_bounds() {
        // Water kills worms but projectiles still exist there until they sink
        let pos = WorldPos::new(100.0, WATER_Y as f32);
        assert!(!World::out_of_bounds(pos));
        assert!(World::in_water(pos));
    }
}
