use noise::{NoiseFn, Perlin};
use super::constants::*;

/// A heightmap across the full world width.
/// `surface_y[x]` is the Y coordinate of the terrain surface at column x.
/// Lower Y = higher up the screen (origin is top-left).
/// Values are guaranteed to be within [TERRAIN_MIN_Y, TERRAIN_MAX_Y].
pub struct Heightmap {
    pub surface_y: Vec<u32>,
    pub seed: u64,
}
fn lcg(s: &mut u64) -> u64 { *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); *s >> 33 }

impl Heightmap {
    /// Generate a heightmap from a seed using multi-octave Perlin noise.
    ///
    /// Three octaves layered together:
    ///   - Large slow waves    → overall hill shape
    ///   - Medium waves        → ridges and valleys
    ///   - Small fast waves    → surface roughness


    pub fn generate(seed: u64) -> Self {
        let perlin  = Perlin::new(seed as u32);
        let perlin2 = Perlin::new(seed.wrapping_add(1337) as u32);

        let terrain_range = (TERRAIN_MAX_Y - TERRAIN_MIN_Y) as f64;
        let mid_y         = TERRAIN_MIN_Y as f64 + terrain_range * 0.5;

        // Pass 1: base heightmap with 4 octaves
        let mut heights: Vec<f64> = (0..WORLD_W)
            .map(|x| {
                let nx = x as f64 / WORLD_W as f64;
                let o1 = perlin.get([nx * 2.2, 0.0, seed as f64 * 0.001]) * 0.65;
                let o2 = perlin.get([nx * 5.5, 1.5, seed as f64 * 0.001]) * 0.30;
                // Reduced high-frequency octaves to avoid narrow trapping gaps
                let o3 = perlin.get([nx * 13.0, 3.0, seed as f64 * 0.001]) * 0.10;
                let o4 = perlin.get([nx * 28.0, 5.0, seed as f64 * 0.001]) * 0.03;
                o1 + o2 + o3 + o4
            })
            .collect();

        // Pass 2: plateau carving
        let mut rng = seed;
        let n_plateaus = 3 + (lcg(&mut rng) % 4) as usize;
        for _ in 0..n_plateaus {
            let cx   = (lcg(&mut rng) % WORLD_W as u64) as usize;
            let w    = 60 + (lcg(&mut rng) % 120) as usize;
            let flat = heights[cx];
            let half = w / 2;
            let x0   = cx.saturating_sub(half);
            let x1   = (cx + half).min(WORLD_W as usize - 1);
            for x in x0..=x1 {
                let d = (x as isize - cx as isize).abs() as f64 / half as f64;
                let t = { let d = d.clamp(0.0,1.0); d*d*(3.0-2.0*d) };
                heights[x] = heights[x] * t + flat * (1.0 - t);
            }
        }

        // Pass 3: valley punching — wider valleys, fewer narrow traps
        let n_valleys = 1 + (lcg(&mut rng) % 2) as usize; // 1-2 valleys (was 1-3)
        for _ in 0..n_valleys {
            let cx   = (lcg(&mut rng) % WORLD_W as u64) as usize;
            let w    = 100 + (lcg(&mut rng) % 120) as usize; // wider (was 40-120)
            let half = w / 2;
            let x0   = cx.saturating_sub(half);
            let x1   = (cx + half).min(WORLD_W as usize - 1);
            for x in x0..=x1 {
                let d = (x as isize - cx as isize).abs() as f64 / half as f64;
                heights[x] -= 0.45 * (1.0 - d * d); // shallower (was 0.55)
            }
        }

        // Pass 4: hill bumps
        let n_bumps = 4 + (lcg(&mut rng) % 5) as usize;
        for _ in 0..n_bumps {
            let cx    = (lcg(&mut rng) % WORLD_W as u64) as usize;
            let height = 0.2 + (lcg(&mut rng) % 100) as f64 / 200.0;
            let w2    = 30 + (lcg(&mut rng) % 100) as usize;
            let half2 = w2 / 2;
            let x0b = cx.saturating_sub(half2);
            let x1b = (cx + half2).min(WORLD_W as usize - 1);
            for x in x0b..=x1b {
                let d2 = (x as isize - cx as isize).abs() as f64 / half2 as f64;
                heights[x] -= height * (1.0 - d2 * d2).max(0.0);
            }
        }

        // Map to Y coords
        let mut surface_y: Vec<u32> = heights.iter()
            .map(|&h| {
                let y = mid_y + h * terrain_range * 0.48;
                (y.round() as u32).clamp(TERRAIN_MIN_Y, TERRAIN_MAX_Y)
            })
            .collect();

        // Gap-fill pass: ensure no narrow pits where soldiers get stuck.
        // Soldiers are ~16px wide (SOLDIER_W). Any local dip narrower than that
        // radius gets raised to match its neighbours.
        let gap_radius: usize = 10; // half-width — fills gaps up to ~20px wide
        let orig = surface_y.clone();
        for i in 0..WORLD_W as usize {
            let lo = i.saturating_sub(gap_radius);
            let hi = (i + gap_radius).min(WORLD_W as usize - 1);
            // Minimum Y (= highest point) in the window
            let min_y = orig[lo..=hi].iter().copied().min().unwrap_or(orig[i]);
            // Only fill upward (lower Y = higher), never pull terrain down
            if surface_y[i] > min_y + 12 {
                surface_y[i] = min_y + 12;
            }
        }
        // Light smoothing to remove jagged edges left by gap fill
        let prev = surface_y.clone();
        for i in 1..WORLD_W as usize - 1 {
            let avg = (prev[i-1] as i32 + prev[i] as i32 * 2 + prev[i+1] as i32) / 4;
            surface_y[i] = (avg as u32).clamp(TERRAIN_MIN_Y, TERRAIN_MAX_Y);
        }

        Self { surface_y, seed }
    }
    pub fn surface_at(&self, x: u32) -> u32 {
        let x = x.min(WORLD_W - 1) as usize;
        self.surface_y[x]
    }

    /// Highest point (lowest Y value) across the whole map.
    pub fn peak_y(&self) -> u32 {
        *self.surface_y.iter().min().unwrap()
    }

    /// Lowest point (highest Y value) across the whole map.
    pub fn valley_y(&self) -> u32 {
        *self.surface_y.iter().max().unwrap()
    }

    /// Maximum height difference between any two adjacent columns.
    /// Useful for checking the terrain isn't too jagged.
    pub fn max_step(&self) -> u32 {
        self.surface_y
            .windows(2)
            .map(|w| w[0].abs_diff(w[1]))
            .max()
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(seed: u64) -> Heightmap {
        Heightmap::generate(seed)
    }

    #[test]
    fn heightmap_has_correct_length() {
        let h = map(42);
        assert_eq!(h.surface_y.len(), WORLD_W as usize);
    }

    #[test]
    fn all_values_within_terrain_bounds() {
        // Test a handful of seeds to catch edge cases
        for seed in [0, 1, 42, 999, u64::MAX / 2] {
            let h = map(seed);
            for (x, &y) in h.surface_y.iter().enumerate() {
                assert!(
                    y >= TERRAIN_MIN_Y && y <= TERRAIN_MAX_Y,
                    "seed={seed} x={x} y={y} out of bounds [{TERRAIN_MIN_Y},{TERRAIN_MAX_Y}]"
                );
            }
        }
    }

    #[test]
    fn same_seed_produces_identical_terrain() {
        let a = map(12345);
        let b = map(12345);
        assert_eq!(a.surface_y, b.surface_y);
    }

    #[test]
    fn different_seeds_produce_different_terrain() {
        let a = map(1);
        let b = map(2);
        assert_ne!(a.surface_y, b.surface_y);
    }

    #[test]
    fn peak_is_within_bounds() {
        let h = map(7);
        assert!(h.peak_y() >= TERRAIN_MIN_Y);
        assert!(h.peak_y() <= TERRAIN_MAX_Y);
    }

    #[test]
    fn valley_is_within_bounds() {
        let h = map(7);
        assert!(h.valley_y() >= TERRAIN_MIN_Y);
        assert!(h.valley_y() <= TERRAIN_MAX_Y);
    }

    #[test]
    fn peak_is_higher_than_or_equal_to_valley() {
        let h = map(99);
        assert!(h.peak_y() <= h.valley_y()); // lower Y = higher on screen
    }

    #[test]
    fn surface_at_clamps_to_bounds() {
        let h = map(1);
        // These should not panic and should return edge values
        assert_eq!(h.surface_at(0), h.surface_y[0]);
        assert_eq!(h.surface_at(WORLD_W - 1), h.surface_y[WORLD_W as usize - 1]);
        assert_eq!(h.surface_at(WORLD_W + 999), h.surface_y[WORLD_W as usize - 1]);
    }

    #[test]
    fn terrain_is_not_completely_flat() {
        // With any reasonable seed the terrain should have some variation
        let h = map(42);
        let min = h.peak_y();
        let max = h.valley_y();
        assert!(
            max - min >= 10,
            "terrain too flat: min_y={min} max_y={max}"
        );
    }

    #[test]
    fn adjacent_column_step_is_not_too_extreme() {
        // No single-pixel columns should have a cliff more than 40px tall.
        // This keeps worms from getting permanently trapped.
        for seed in [0, 42, 999, 8675309] {
            let h = map(seed);
            let step = h.max_step();
            assert!(
                step <= 40,
                "seed={seed} max adjacent step={step} is too large"
            );
        }
    }

    #[test]
    fn seed_stored_correctly() {
        let h = map(777);
        assert_eq!(h.seed, 777);
    }
}
