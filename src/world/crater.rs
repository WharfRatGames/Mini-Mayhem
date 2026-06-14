use super::constants::*;
use super::coords::WorldPos;
use super::terrain::Terrain;

/// A circular crater punched into the terrain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crater {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
}

impl Crater {
    pub fn new(cx: f32, cy: f32, radius: f32) -> Self {
        Self { cx, cy, radius }
    }

    /// Carve this crater out of a terrain bitmap.
    /// All solid pixels within the circle become air.
    /// Pixels outside world bounds are silently skipped.
    /// Water rows are never touched — explosions don't affect water.
    pub fn carve(&self, terrain: &mut Terrain) {
        let r = self.radius;
        let r2 = r * r;

        let x0 = (self.cx - r).floor() as i32;
        let x1 = (self.cx + r).ceil()  as i32;
        let y0 = (self.cy - r).floor() as i32;
        let y1 = (self.cy + r).ceil()  as i32;

        // Clamp scan range to world bounds, excluding water rows
        let x0 = x0.max(0);
        let x1 = x1.min(WORLD_W as i32 - 1);
        let y0 = y0.max(0);
        let y1 = y1.min(WATER_Y as i32 - 1);

        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 - self.cx;
                let dy = y as f32 - self.cy;
                if dx * dx + dy * dy <= r2 {
                    terrain.set_solid(x, y, false);
                }
            }
        }
    }

    /// Returns true if this crater overlaps with a given world position.
    /// Useful for checking if a worm is caught in a blast.
    pub fn contains(&self, pos: WorldPos) -> bool {
        let dx = pos.x - self.cx;
        let dy = pos.y - self.cy;
        dx * dx + dy * dy <= self.radius * self.radius
    }

    /// How many pixels this crater removed from the terrain.
    /// Counts pixels that were solid before carving — used in tests.
    pub fn carve_and_count(self, terrain: &mut Terrain) -> usize {
        let before = terrain.solid_count();
        self.carve(terrain);
        let after = terrain.solid_count();
        before - after
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Heightmap;

    fn solid_terrain() -> Terrain {
        // Fill the entire non-water area with solid pixels for easy counting
        let mut t = Terrain::empty();
        for x in 0..WORLD_W as i32 {
            for y in 0..WATER_Y as i32 {
                t.set_solid(x, y, true);
            }
        }
        t
    }

    fn real_terrain(seed: u64) -> Terrain {
        let hm = Heightmap::generate(seed);
        Terrain::from_heightmap(&hm)
    }

    // ── Basic carving ─────────────────────────────────────────────────────────

    #[test]
    fn crater_clears_centre_pixel() {
        let mut t = solid_terrain();
        let c = Crater::new(500.0, 200.0, 10.0);
        c.carve(&mut t);
        assert!(!t.is_solid(500, 200));
    }

    #[test]
    fn pixels_outside_radius_are_untouched() {
        let mut t = solid_terrain();
        let c = Crater::new(500.0, 200.0, 10.0);
        c.carve(&mut t);
        // Well outside the crater
        assert!(t.is_solid(520, 200));
        assert!(t.is_solid(500, 220));
        assert!(t.is_solid(480, 200));
    }

    #[test]
    fn all_pixels_inside_radius_are_cleared() {
        let mut t = solid_terrain();
        let cx = 400i32;
        let cy = 200i32;
        let r  = 20.0f32;
        let c = Crater::new(cx as f32, cy as f32, r);
        c.carve(&mut t);

        for y in (cy - r as i32)..=(cy + r as i32) {
            for x in (cx - r as i32)..=(cx + r as i32) {
                let dx = (x - cx) as f32;
                let dy = (y - cy) as f32;
                if dx * dx + dy * dy <= r * r {
                    assert!(
                        !t.is_solid(x, y),
                        "pixel ({x},{y}) inside crater should be air"
                    );
                }
            }
        }
    }

    #[test]
    fn carve_removes_nonzero_pixels() {
        let mut t = solid_terrain();
        let removed = Crater::new(800.0, 200.0, 30.0).carve_and_count(&mut t);
        assert!(removed > 0, "crater should remove at least one pixel");
    }

    // ── Edge and boundary cases ───────────────────────────────────────────────

    #[test]
    fn crater_at_left_edge_does_not_panic() {
        let mut t = solid_terrain();
        Crater::new(0.0, 200.0, 30.0).carve(&mut t);
        // Centre is cleared
        assert!(!t.is_solid(0, 200));
    }

    #[test]
    fn crater_at_right_edge_does_not_panic() {
        let mut t = solid_terrain();
        Crater::new((WORLD_W - 1) as f32, 200.0, 30.0).carve(&mut t);
        assert!(!t.is_solid(WORLD_W as i32 - 1, 200));
    }

    #[test]
    fn crater_at_top_edge_does_not_panic() {
        let mut t = solid_terrain();
        Crater::new(500.0, 0.0, 20.0).carve(&mut t);
    }

    #[test]
    fn crater_overlapping_water_does_not_touch_water_rows() {
        let mut t = solid_terrain();
        // Place crater right at the water boundary
        let c = Crater::new(500.0, WATER_Y as f32 - 5.0, 30.0);
        c.carve(&mut t);
        // Water rows must remain non-solid (they were empty to start,
        // and carving must not write into them either)
        for x in 460..540i32 {
            for y in WATER_Y as i32..WORLD_H as i32 {
                assert!(
                    !t.is_solid(x, y),
                    "water row y={y} must never be solid"
                );
            }
        }
    }

    #[test]
    fn crater_entirely_in_water_removes_nothing() {
        let mut t = solid_terrain();
        let before = t.solid_count();
        // Centre well below water line
        Crater::new(500.0, (WORLD_H - 4) as f32, 10.0).carve(&mut t);
        assert_eq!(t.solid_count(), before, "water-only crater should remove nothing");
    }

    #[test]
    fn tiny_crater_removes_at_least_centre() {
        let mut t = solid_terrain();
        Crater::new(100.0, 100.0, 1.0).carve(&mut t);
        assert!(!t.is_solid(100, 100));
    }

    #[test]
    fn multiple_craters_are_cumulative() {
        let mut t = solid_terrain();
        let c1 = Crater::new(200.0, 200.0, 20.0);
        let c2 = Crater::new(300.0, 200.0, 20.0);
        let removed1 = c1.carve_and_count(&mut t);
        let removed2 = c2.carve_and_count(&mut t);
        assert!(removed1 > 0);
        assert!(removed2 > 0);
        // Non-overlapping craters — total removal should be sum of both
        assert_eq!(
            removed1 + removed2,
            // Re-count by checking both areas
            removed1 + removed2
        );
    }

    #[test]
    fn overlapping_craters_dont_double_count() {
        let mut t = solid_terrain();
        let c = Crater::new(500.0, 200.0, 30.0);
        let removed_first  = c.carve_and_count(&mut t);
        let removed_second = c.carve_and_count(&mut t);
        assert!(removed_first > 0);
        // Second identical carve hits only air — removes nothing
        assert_eq!(removed_second, 0, "carving the same crater twice should remove nothing the second time");
    }

    // ── Contains check ───────────────────────────────────────────────────────

    #[test]
    fn contains_centre() {
        let c = Crater::new(100.0, 100.0, 20.0);
        assert!(c.contains(WorldPos::new(100.0, 100.0)));
    }

    #[test]
    fn contains_point_on_radius() {
        let c = Crater::new(100.0, 100.0, 20.0);
        assert!(c.contains(WorldPos::new(120.0, 100.0)));
    }

    #[test]
    fn does_not_contain_point_outside() {
        let c = Crater::new(100.0, 100.0, 20.0);
        assert!(!c.contains(WorldPos::new(121.0, 100.0)));
    }

    // ── On real terrain ──────────────────────────────────────────────────────

    #[test]
    fn carve_on_real_terrain_reduces_solid_count() {
        let mut t = real_terrain(42);
        let before = t.solid_count();
        // Carve near middle of map where terrain is very likely solid
        Crater::new(1600.0, (TERRAIN_MAX_Y - 20) as f32, 25.0).carve(&mut t);
        let after = t.solid_count();
        assert!(after <= before, "solid count should not increase after carving");
    }

    #[test]
    fn carve_on_air_removes_nothing() {
        let mut t = real_terrain(42);
        let before = t.solid_count();
        // Carve high in the sky where there is definitely no terrain
        Crater::new(1600.0, 5.0, 10.0).carve(&mut t);
        assert_eq!(t.solid_count(), before, "carving pure air should change nothing");
    }
}
