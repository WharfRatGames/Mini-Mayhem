/// Total width of the game world in pixels.
/// 3 screens wide at 640px each (20% larger than the previous 1600px maps).
pub const WORLD_W: u32 = 1920;

/// Total height of the game world in pixels.
pub const WORLD_H: u32 = 480;

/// Width of the Miyoo screen viewport in pixels.
pub const SCREEN_W: u32 = 640;

/// Height of the Miyoo screen viewport in pixels.
pub const SCREEN_H: u32 = 480;

/// How many rows at the bottom of the world are water.
/// Worms that enter this zone drown instantly.
pub const WATER_ROWS: u32 = 120;

/// The Y coordinate at which water begins (inclusive).
/// Anything at or below this line is water.
pub const WATER_Y: u32 = WORLD_H - WATER_ROWS;

/// Minimum terrain surface Y. Terrain will never reach above this line,
/// leaving clear sky at the top of the world.
pub const TERRAIN_MIN_Y: u32 = 80;

/// Maximum terrain surface Y. Terrain will never reach into the water zone.
pub const TERRAIN_MAX_Y: u32 = WATER_Y - 40;

/// Total number of pixels in the world bitmap.
pub const WORLD_PIXELS: usize = (WORLD_W * WORLD_H) as usize;

/// Soldiers never spawn within this many pixels of either world edge.
/// Keeps spawns out of the eroded water margins and away from the very edge.
pub const SPAWN_EDGE_MARGIN: u32 = 180;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_dimensions_are_sane() {
        assert_eq!(WORLD_W, 1920);
        assert_eq!(WORLD_H, 480);
        assert_eq!(SCREEN_W, 640);
        assert_eq!(SCREEN_H, 480);
        // World is wider than the screen so the camera scrolls horizontally
        assert!(WORLD_W > SCREEN_W);
    }

    #[test]
    fn water_zone_is_within_world() {
        assert!(WATER_Y < WORLD_H);
        assert!(WATER_ROWS > 0);
        assert_eq!(WATER_Y + WATER_ROWS, WORLD_H);
    }

    #[test]
    fn terrain_bounds_are_within_world() {
        assert!(TERRAIN_MIN_Y < TERRAIN_MAX_Y);
        assert!(TERRAIN_MAX_Y < WATER_Y);
        assert!(TERRAIN_MIN_Y > 0);
    }

    #[test]
    fn pixel_count_matches_dimensions() {
        assert_eq!(WORLD_PIXELS, (WORLD_W * WORLD_H) as usize);
        assert_eq!(WORLD_PIXELS, 921_600);
    }
}
