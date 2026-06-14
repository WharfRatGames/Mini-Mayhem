use super::constants::*;

/// A position in world space (pixels, origin top-left).
/// X goes 0..WORLD_W left to right.
/// Y goes 0..WORLD_H top to bottom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldPos {
    pub x: f32,
    pub y: f32,
}

impl WorldPos {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns true if this position is within the world bounds.
    pub fn in_bounds(self) -> bool {
        self.x >= 0.0
            && self.x < WORLD_W as f32
            && self.y >= 0.0
            && self.y < WORLD_H as f32
    }

    /// Returns true if this position is in the water zone.
    pub fn in_water(self) -> bool {
        self.y >= WATER_Y as f32
    }

    /// Returns true if this position is at the left or right hard wall.
    pub fn at_wall(self) -> bool {
        self.x < 0.0 || self.x >= WORLD_W as f32
    }

    /// Clamp this position to world bounds.
    pub fn clamped(self) -> Self {
        Self {
            x: self.x.clamp(0.0, (WORLD_W - 1) as f32),
            y: self.y.clamp(0.0, (WORLD_H - 1) as f32),
        }
    }

    /// Convert to a bitmap index. Returns None if out of bounds.
    pub fn to_index(self) -> Option<usize> {
        if !self.in_bounds() {
            return None;
        }
        Some((self.y as u32 * WORLD_W + self.x as u32) as usize)
    }
}

/// A 2D velocity or direction vector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn zero() -> Self {
        Self { x: 0.0, y: 0.0 }
    }

    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    pub fn normalized(self) -> Self {
        let l = self.length();
        if l < 1e-6 {
            Self::zero()
        } else {
            Self::new(self.x / l, self.y / l)
        }
    }

    pub fn scale(self, s: f32) -> Self {
        Self::new(self.x * s, self.y * s)
    }

    pub fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }
}

/// Convert world X column to a bitmap row-major index at a given Y.
pub fn world_index(x: u32, y: u32) -> usize {
    (y * WORLD_W + x) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_pos_in_bounds() {
        assert!(WorldPos::new(0.0, 0.0).in_bounds());
        assert!(WorldPos::new(3199.0, 479.0).in_bounds());
        assert!(!WorldPos::new(-1.0, 0.0).in_bounds());
        assert!(!WorldPos::new(0.0, -1.0).in_bounds());
        assert!(!WorldPos::new(3200.0, 0.0).in_bounds());
        assert!(!WorldPos::new(0.0, 480.0).in_bounds());
    }

    #[test]
    fn world_pos_water_zone() {
        // Just above water
        assert!(!WorldPos::new(100.0, WATER_Y as f32 - 1.0).in_water());
        // At water line
        assert!(WorldPos::new(100.0, WATER_Y as f32).in_water());
        // Deep in water
        assert!(WorldPos::new(100.0, (WORLD_H - 1) as f32).in_water());
    }

    #[test]
    fn world_pos_walls() {
        assert!(WorldPos::new(-1.0, 100.0).at_wall());
        assert!(WorldPos::new(3200.0, 100.0).at_wall());
        assert!(!WorldPos::new(0.0, 100.0).at_wall());
        assert!(!WorldPos::new(3199.0, 100.0).at_wall());
    }

    #[test]
    fn world_pos_to_index() {
        // Top-left corner
        assert_eq!(WorldPos::new(0.0, 0.0).to_index(), Some(0));
        // One pixel right
        assert_eq!(WorldPos::new(1.0, 0.0).to_index(), Some(1));
        // Second row
        assert_eq!(WorldPos::new(0.0, 1.0).to_index(), Some(WORLD_W as usize));
        // Out of bounds
        assert_eq!(WorldPos::new(-1.0, 0.0).to_index(), None);
        assert_eq!(WorldPos::new(0.0, 480.0).to_index(), None);
    }

    #[test]
    fn world_pos_clamp() {
        let clamped = WorldPos::new(-50.0, 600.0).clamped();
        assert_eq!(clamped.x, 0.0);
        assert_eq!(clamped.y, 479.0);
    }

    #[test]
    fn vec2_length() {
        assert!((Vec2::new(3.0, 4.0).length() - 5.0).abs() < 1e-5);
        assert_eq!(Vec2::zero().length(), 0.0);
    }

    #[test]
    fn vec2_normalize() {
        let v = Vec2::new(3.0, 4.0).normalized();
        assert!((v.length() - 1.0).abs() < 1e-5);
        // Zero vector normalizes to zero safely
        let z = Vec2::zero().normalized();
        assert_eq!(z.x, 0.0);
        assert_eq!(z.y, 0.0);
    }

    #[test]
    fn vec2_arithmetic() {
        let a = Vec2::new(1.0, 2.0);
        let b = Vec2::new(3.0, 4.0);
        let sum = a.add(b);
        assert_eq!(sum.x, 4.0);
        assert_eq!(sum.y, 6.0);
        let scaled = a.scale(2.0);
        assert_eq!(scaled.x, 2.0);
        assert_eq!(scaled.y, 4.0);
    }

    #[test]
    fn world_index_matches_manual() {
        assert_eq!(world_index(0, 0), 0);
        assert_eq!(world_index(1, 0), 1);
        assert_eq!(world_index(0, 1), WORLD_W as usize);
        assert_eq!(world_index(WORLD_W - 1, WORLD_H - 1), WORLD_PIXELS - 1);
    }
}
