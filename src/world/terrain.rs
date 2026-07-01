use noise::{NoiseFn, OpenSimplex, Perlin};
use super::constants::*;
use super::coords::{WorldPos, world_index};

/// The terrain bitmap for the entire 3200×480 world.
///
/// `solid[i] == true` means that pixel is solid ground.
/// Indexed row-major: index = y * WORLD_W + x.
///
/// Water rows at the bottom are NOT marked solid — they are a separate
/// kill zone checked via WorldPos::in_water(). This keeps collision
/// detection simple: solid == terrain you can stand on or blow up.
pub struct Terrain {
    solid: Vec<bool>,
    /// Per-tick object layer: barrels and armed mines stamp their footprint here.
    /// Cleared and re-stamped every tick. Checked alongside `solid` for collision.
    pub objects: Vec<bool>,
    /// 256×256 procedural dirt texture. Sampled at (world_x & 255, world_y & 255).
    /// None = use flat color rendering (old look).
    pub texture: Option<Vec<[u8; 4]>>, // [B, G, R, A]
    /// Heightmap-derived surface Y for each column — safe spawn positions
    /// guaranteed to be above caves and below islands.
    pub spawn_y: Vec<u32>,
    /// Unclamped topmost-solid-y per column (or WATER_Y if the column has no
    /// solid pixels above water). Unlike `spawn_y` (clamped to TERRAIN_MIN_Y
    /// for texture-depth purposes), this is exact — the renderer uses it as
    /// "y below this is guaranteed sky" when drawing atmospheric backgrounds
    /// behind the terrain, so overhangs/islands above TERRAIN_MIN_Y aren't
    /// hidden.
    pub sky_limit: Vec<u32>,
    /// True if column `x` is solid contiguously from `sky_limit[x]` down to
    /// `WATER_Y` (no caves/chasm gaps). Lets the renderer block-copy these
    /// columns without a per-pixel `is_solid` check.
    pub solid_to_water: Vec<bool>,
    /// For columns where `solid_to_water[x] == false` (caves/chasms/overhangs),
    /// the contiguous solid [start, end) spans between `sky_limit[x]` and
    /// `WATER_Y`. Lets the renderer's sky-aware viewport copy memcpy each span
    /// directly instead of testing `is_solid` for every pixel in the column.
    /// Empty for `solid_to_water[x] == true` columns (handled by a single
    /// block-copy there instead).
    pub solid_runs: Vec<Vec<(u32, u32)>>,
    /// Index (0–23) into the terrain texture atlas, chosen per map from the seed.
    /// Renderer samples this tile to texture the solid silhouette.
    pub surface_texture: u8,
    /// Which landform style this map was generated as.
    /// 0=hills 1=cliffs/overhangs 2=floating islands 3=caverns 4=canyon/mesa.
    /// Drives spawn placement (caverns put some soldiers underground).
    pub archetype: u8,
    /// Decorative scenery objects placed seed-deterministically on the terrain surface.
    /// Purely cosmetic — no collision effect.
    pub scenery: Vec<SceneryObject>,
}

/// A single decorative scenery object placed on the terrain surface.
/// `x`/`y` are world-space pixel coordinates of the bottom-center of the sprite.
/// `sprite` is the variant index within the archetype's object set.
#[derive(Clone, Copy)]
pub struct SceneryObject {
    pub x: u32,
    pub y: u32,
    pub sprite: u8,
}

impl Terrain {
    /// Allocate an empty terrain (all air).
    pub fn empty() -> Self {
        Self {
            solid: vec![false; WORLD_PIXELS],
            objects: vec![false; WORLD_PIXELS],
            texture: None,
            spawn_y: vec![TERRAIN_MAX_Y; WORLD_W as usize],
            sky_limit: vec![WATER_Y; WORLD_W as usize],
            solid_to_water: vec![false; WORLD_W as usize],
            solid_runs: vec![Vec::new(); WORLD_W as usize],
            surface_texture: 0,
            archetype: 0,
            scenery: Vec::new(),
        }
    }

    /// Returns true if the pixel at (x, y) is solid ground.
    /// Out-of-bounds always returns false — never panics.
    pub fn is_solid(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= WORLD_W as i32 || y >= WORLD_H as i32 {
            return false;
        }
        self.solid[world_index(x as u32, y as u32)]
    }

    /// Set the solid state of a pixel.
    /// Out-of-bounds writes are silently ignored — never panics.
    pub fn set_solid(&mut self, x: i32, y: i32, solid: bool) {
        if x < 0 || y < 0 || x >= WORLD_W as i32 || y >= WORLD_H as i32 {
            return;
        }
        self.solid[world_index(x as u32, y as u32)] = solid;
    }

    /// Returns true if the WorldPos lands on solid terrain.
    pub fn is_solid_pos(&self, pos: WorldPos) -> bool {
        self.is_solid(pos.x as i32, pos.y as i32)
    }

    /// Total number of solid pixels. Useful for tests and debugging.
    pub fn solid_count(&self) -> usize {
        self.solid.iter().filter(|&&s| s).count()
    }

    /// Returns true if this terrain has no solid pixels at all.
    pub fn is_empty(&self) -> bool {
        self.solid.iter().all(|&s| !s)
    }

    /// Clear all object-layer stamps. Call once per tick before re-stamping.
    pub fn clear_objects(&mut self) {
        self.objects.iter_mut().for_each(|v| *v = false);
    }

    /// Mark a pixel in the object layer (barrel/mine footprint).
    pub fn stamp_object(&mut self, x: i32, y: i32) {
        if x < 0 || y < 0 || x >= WORLD_W as i32 || y >= WORLD_H as i32 { return; }
        self.objects[world_index(x as u32, y as u32)] = true;
    }

    /// True if either solid terrain OR an object occupies this pixel.
    /// Use this for all collision checks instead of is_solid.
    pub fn is_blocked(&self, x: i32, y: i32) -> bool {
        if x < 0 || y < 0 || x >= WORLD_W as i32 || y >= WORLD_H as i32 { return false; }
        let i = world_index(x as u32, y as u32);
        self.solid[i] || self.objects[i]
    }

    /// Like `is_solid`, but skips the bounds check — caller must guarantee
    /// `x < WORLD_W` and `y < WORLD_H`. Used in hot per-pixel render loops.
    pub fn is_solid_unchecked(&self, x: u32, y: u32) -> bool {
        self.solid[world_index(x, y)]
    }

    /// Recompute `sky_limit[x]` and `solid_to_water[x]` from the current
    /// `solid` bits — same logic as the post-generation pass. Call for every
    /// column affected by terrain destruction (e.g. `Crater::carve`) so these
    /// caches don't go stale when an explosion opens a new air gap.
    pub fn recompute_column_cache(&mut self, x: i32) {
        if x < 0 || x >= WORLD_W as i32 { return; }
        let topmost = (0..WATER_Y).find(|&y| self.is_solid(x, y as i32));
        let sky_limit = topmost.unwrap_or(WATER_Y);
        self.sky_limit[x as usize] = sky_limit;
        let solid_to_water = sky_limit < WATER_Y
            && (sky_limit..WATER_Y).all(|y| self.is_solid(x, y as i32));
        self.solid_to_water[x as usize] = solid_to_water;
        self.solid_runs[x as usize] = if solid_to_water {
            Vec::new()
        } else {
            self.solid_runs_for_column(x, sky_limit)
        };
    }

    /// Contiguous solid [start, end) spans in column `x` between `y0` and
    /// `WATER_Y`. Used to populate `solid_runs` for caves/chasm columns.
    fn solid_runs_for_column(&self, x: i32, y0: u32) -> Vec<(u32, u32)> {
        let mut runs = Vec::new();
        let mut run_start: Option<u32> = None;
        for y in y0..WATER_Y {
            if self.is_solid(x, y as i32) {
                if run_start.is_none() { run_start = Some(y); }
            } else if let Some(s) = run_start.take() {
                runs.push((s, y));
            }
        }
        if let Some(s) = run_start { runs.push((s, WATER_Y)); }
        runs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_terrain_has_no_solid_pixels() {
        let t = Terrain::empty();
        assert!(t.is_empty());
        assert_eq!(t.solid_count(), 0);
    }

    #[test]
    fn set_and_read_solid() {
        let mut t = Terrain::empty();
        t.set_solid(100, 200, true);
        assert!(t.is_solid(100, 200));
        assert_eq!(t.solid_count(), 1);
    }

    #[test]
    fn set_solid_false_clears_pixel() {
        let mut t = Terrain::empty();
        t.set_solid(50, 50, true);
        assert!(t.is_solid(50, 50));
        t.set_solid(50, 50, false);
        assert!(!t.is_solid(50, 50));
        assert_eq!(t.solid_count(), 0);
    }

    #[test]
    fn adjacent_pixels_are_independent() {
        let mut t = Terrain::empty();
        t.set_solid(10, 10, true);
        assert!(t.is_solid(10, 10));
        assert!(!t.is_solid(11, 10));
        assert!(!t.is_solid(9,  10));
        assert!(!t.is_solid(10, 11));
        assert!(!t.is_solid(10,  9));
    }

    #[test]
    fn out_of_bounds_read_returns_false() {
        let t = Terrain::empty();
        assert!(!t.is_solid(-1, 0));
        assert!(!t.is_solid(0, -1));
        assert!(!t.is_solid(WORLD_W as i32, 0));
        assert!(!t.is_solid(0, WORLD_H as i32));
        assert!(!t.is_solid(-9999, -9999));
        assert!(!t.is_solid(99999, 99999));
    }

    #[test]
    fn out_of_bounds_write_does_not_panic() {
        let mut t = Terrain::empty();
        t.set_solid(-1, 0, true);
        t.set_solid(0, -1, true);
        t.set_solid(WORLD_W as i32, 0, true);
        t.set_solid(0, WORLD_H as i32, true);
        t.set_solid(-9999, -9999, true);
        t.set_solid(99999, 99999, true);
        // None of those should have set anything
        assert_eq!(t.solid_count(), 0);
    }

    #[test]
    fn corners_are_settable() {
        let mut t = Terrain::empty();
        t.set_solid(0, 0, true);
        t.set_solid(WORLD_W as i32 - 1, 0, true);
        t.set_solid(0, WORLD_H as i32 - 1, true);
        t.set_solid(WORLD_W as i32 - 1, WORLD_H as i32 - 1, true);
        assert!(t.is_solid(0, 0));
        assert!(t.is_solid(WORLD_W as i32 - 1, 0));
        assert!(t.is_solid(0, WORLD_H as i32 - 1));
        assert!(t.is_solid(WORLD_W as i32 - 1, WORLD_H as i32 - 1));
        assert_eq!(t.solid_count(), 4);
    }

    #[test]
    fn is_solid_pos_matches_is_solid() {
        let mut t = Terrain::empty();
        t.set_solid(300, 250, true);
        assert!(t.is_solid_pos(WorldPos::new(300.0, 250.0)));
        assert!(!t.is_solid_pos(WorldPos::new(301.0, 250.0)));
    }

    #[test]
    fn solid_count_tracks_correctly() {
        let mut t = Terrain::empty();
        assert_eq!(t.solid_count(), 0);
        t.set_solid(1, 1, true);
        assert_eq!(t.solid_count(), 1);
        t.set_solid(2, 2, true);
        assert_eq!(t.solid_count(), 2);
        t.set_solid(1, 1, false);
        assert_eq!(t.solid_count(), 1);
    }
}

// ── Step 4: fill from heightmap ───────────────────────────────────────────────

use super::heightmap::Heightmap;

impl Terrain {
    /// Build a terrain bitmap from a heightmap.
    ///
    /// For every column x, every pixel at y >= surface_y[x] and
    /// below the water line is marked solid.
    /// Water rows themselves are left non-solid (they are a kill zone,
    /// not terrain you can stand on or destroy).
    pub fn from_heightmap(hm: &Heightmap) -> Self {
        let mut terrain = Self::empty();
        for x in 0..WORLD_W {
            let surface_y = hm.surface_at(x);
            terrain.spawn_y[x as usize] = surface_y;
            terrain.sky_limit[x as usize] = surface_y;
            terrain.solid_to_water[x as usize] = true;
            for y in surface_y..WATER_Y {
                terrain.set_solid(x as i32, y as i32, true);
            }
        }
        terrain
    }

    /// Multi-pass tactical terrain with 4 archetype-based landform styles.
    /// 0=hills  1=cliffs/overhangs  2=floating islands  3=caverns  4=canyon/mesa
    /// Phases 6 & 7 (smoothing + spawn guarantee) are always applied.
    pub fn generate_tactical(seed: u64) -> Self {
        fn lcg(s: &mut u64) -> u64 {
            *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *s >> 33
        }
        fn carve_ellipse(t: &mut Terrain, cx: i32, cy: i32, rx: i32, ry: i32) {
            for dy in -ry..=ry {
                let span = (rx as f32 * (1.0 - (dy as f32 / ry as f32).powi(2)).sqrt()) as i32;
                for dx in -span..=span {
                    t.set_solid(cx + dx, cy + dy, false);
                }
            }
        }
        fn dig_tunnel(t: &mut Terrain, x1: i32, y1: i32, x2: i32, y2: i32, r: i32) {
            let steps = ((x2 - x1).abs().max((y2 - y1).abs())) as usize + 1;
            for i in 0..=steps {
                let tx = x1 + (x2 - x1) * i as i32 / steps.max(1) as i32;
                let ty = y1 + (y2 - y1) * i as i32 / steps.max(1) as i32;
                for dy in -r..=r { for dx in -r..=r {
                    if dx*dx + dy*dy <= r*r { t.set_solid(tx+dx, ty+dy, false); }
                }}
            }
        }
        // Carve a vertical slot (chasm/pit) of half-width `half_w` from `top_y` down to
        // `bottom_y`, leaning by `drift` px over its depth. bottom_y == WATER_Y makes a
        // water chasm; a shallower bottom_y leaves a solid floor (a pit you fall into).
        fn carve_chasm(t: &mut Terrain, cx: i32, half_w: i32, top_y: i32, bottom_y: i32, drift: i32) {
            let span = (bottom_y - top_y).max(1);
            for y in top_y..bottom_y {
                let f = (y - top_y) as f32 / span as f32; // 0 at top → 1 at bottom
                let c = cx + (drift as f32 * f) as i32;
                for dx in -half_w..=half_w { t.set_solid(c + dx, y, false); }
            }
        }

        use super::coords::world_index;

        let mut rng = seed;
        let mut terrain = Self::empty();
        let terrain_range_f = (TERRAIN_MAX_Y - TERRAIN_MIN_Y) as f64;

        // 16-bit-resolution random float in [lo, lo+span)
        let rnd = |r: &mut u64, lo: f64, span: f64|
            lo + (lcg(r) & 0xFFFF) as f64 / 65535.0 * span;

        // ── Noise sources ────────────────────────────────────────────────────────
        let base   = OpenSimplex::new(seed as u32);
        let warp_a = OpenSimplex::new(seed.wrapping_add(3000) as u32);
        let warp_b = OpenSimplex::new(seed.wrapping_add(4000) as u32);
        let cave_a = OpenSimplex::new(seed.wrapping_add(5000) as u32);
        let cave_b = OpenSimplex::new(seed.wrapping_add(6000) as u32);
        // Low-frequency macro elevation: guarantees rolling hills/valleys on every
        // (non-island, non-cavern) map regardless of how flat the local FBM noise is.
        let hill = OpenSimplex::new(seed.wrapping_add(7000) as u32);

        // ── Phase 0: Archetype + feature selection (all seed-driven) ─────────────
        // 0=hills 1=cliffs/overhangs 2=floating islands 3=caverns 4=canyon/mesa
        let archetype = lcg(&mut rng) % 5;
        terrain.archetype = archetype as u8;
        // Surface texture: seed-driven raw selector; the renderer maps it modulo
        // the pooled atlas tile count (client/server agree from the same seed).
        terrain.surface_texture = lcg(&mut rng) as u8;

        // Feature defaults
        let octaves = 3usize;
        let warp_freq = 2.5;
        let mut warp_amp = 0.07;
        let fade;
        let threshold;
        let scale_x;
        let mut scale_y = 2.5;
        let mut ridged = false;
        let contrast;
        let mut cliff_bias = 0.0;
        let mut terrace: Option<f64> = None;
        let mut terrace_mix = 0.6;
        let mut cave = false;
        // |normalized noise| < cave_thresh carves air. OpenSimplex output clusters
        // near 0, so keep this SMALL — 0.16 ≈ WA-style caves; 0.30+ obliterates terrain.
        let mut cave_thresh = 0.16;
        let cave_sx = 5.5;
        let cave_sy = 5.0;
        let mut blob = false;
        let mut big_void = false;
        let mut overhang = false;       // cliffs: stamp cantilevered ceiling shelves
        let mut surface_caves = false;  // caverns: let cave punch break the top crust
        let mut void_shafts = 0usize;   // caverns: vertical entrance shafts into the void
        // Per-archetype rolling-hill amplitude. Hills = full; cliffs/canyons = reduced
        // so ridged noise / terracing define the silhouette instead of mound-on-mound.
        let mut hill_amp: f64 = 0.24;

        match archetype {
            0 => { // Rolling hills — sub-variants: standard / flat plains / plateau / twin-peak
                let sub = lcg(&mut rng) % 4;
                match sub {
                    0 => { // Standard rolling hills
                        fade = rnd(&mut rng, 0.40, 0.12);
                        threshold = rnd(&mut rng, 0.49, 0.05);
                        scale_x = rnd(&mut rng, 2.5, 2.0);
                        contrast = 1.25;
                        cliff_bias = rnd(&mut rng, -0.10, 0.20);
                    }
                    1 => { // Flat plains: sparse low terrain, lots of open floor space (WA Rocky/Desert style)
                        fade = 0.0;  // no vertical gradient — terrain is uniformly low
                        threshold = rnd(&mut rng, 0.63, 0.04); // high threshold = sparse ground coverage
                        scale_x = rnd(&mut rng, 4.0, 2.0);
                        contrast = 1.8; // sharp cutoff → isolated lumps, not continuous hills
                        cliff_bias = 0.0;
                        warp_amp = rnd(&mut rng, 0.18, 0.10);
                        hill_amp = 0.08; // rolling macro barely visible — bumps define variety
                        cave = lcg(&mut rng) % 100 < 30; // fewer caves — mostly open
                    }
                    2 => { // Plateau: broad flat mesa with drop-offs at sides
                        fade = rnd(&mut rng, 0.55, 0.08);
                        threshold = rnd(&mut rng, 0.53, 0.03);
                        scale_x = rnd(&mut rng, 1.5, 1.0);
                        contrast = 1.6;
                        cliff_bias = 0.0;
                        terrace = Some(3.0);
                        terrace_mix = 0.50;
                    }
                    _ => { // Twin peaks: strong directional bias + high frequency
                        fade = rnd(&mut rng, 0.38, 0.10);
                        threshold = rnd(&mut rng, 0.50, 0.04);
                        scale_x = rnd(&mut rng, 5.0, 2.0);
                        contrast = 1.45;
                        let dir = if lcg(&mut rng) & 1 == 0 { 1.0f64 } else { -1.0 };
                        cliff_bias = dir * rnd(&mut rng, 0.14, 0.12);
                        warp_amp = rnd(&mut rng, 0.12, 0.08);
                    }
                }
                cave = lcg(&mut rng) % 100 < 65;
            }
            1 => { // Cliffs — sub-variants: craggy-face / arch-bridge / one-sided mesa
                let sub = lcg(&mut rng) % 3;
                ridged = true;
                overhang = true;
                hill_amp = 0.08; // ridged noise dominates; rolling hills barely visible
                match sub {
                    0 => { // Craggy cliff face: standard ridged with strong warp
                        fade = rnd(&mut rng, 0.30, 0.12);
                        threshold = rnd(&mut rng, 0.52, 0.06);
                        scale_x = rnd(&mut rng, 5.0, 4.0);
                        scale_y = 2.2;
                        contrast = 1.25;
                        let dir = if lcg(&mut rng) & 1 == 0 { 1.0f64 } else { -1.0 };
                        cliff_bias = dir * rnd(&mut rng, 0.18, 0.24);
                        warp_amp = rnd(&mut rng, 0.40, 0.20); // raised minimum for more dramatic warping
                    }
                    1 => { // Arch-bridge: extreme warp + moderate bias → arches and tunnels
                        fade = rnd(&mut rng, 0.28, 0.10);
                        threshold = rnd(&mut rng, 0.54, 0.05);
                        scale_x = rnd(&mut rng, 4.0, 3.0);
                        scale_y = 1.8;
                        contrast = 1.15;
                        let dir = if lcg(&mut rng) & 1 == 0 { 1.0f64 } else { -1.0 };
                        cliff_bias = dir * rnd(&mut rng, 0.10, 0.14);
                        warp_amp = rnd(&mut rng, 0.50, 0.16); // very strong warp
                    }
                    _ => { // One-sided mesa: strong lean, high cliff on one side, slope on other
                        fade = rnd(&mut rng, 0.40, 0.10);
                        threshold = rnd(&mut rng, 0.50, 0.05);
                        scale_x = rnd(&mut rng, 3.5, 2.5);
                        scale_y = 2.5;
                        contrast = 1.4;
                        let dir = if lcg(&mut rng) & 1 == 0 { 1.0f64 } else { -1.0 };
                        cliff_bias = dir * rnd(&mut rng, 0.32, 0.14); // very strong lean
                        warp_amp = rnd(&mut rng, 0.35, 0.15);
                        terrace = Some(rnd(&mut rng, 3.0, 2.0).round());
                        terrace_mix = 0.25;
                    }
                }
                cave = lcg(&mut rng) % 100 < 70;
            }
            2 => { // Floating islands — sub-variants: archipelago / titan / staircase
                fade = 0.0;
                blob = true;
                let sub = lcg(&mut rng) % 3;
                match sub {
                    0 => { // Archipelago: many medium islands spread wide
                        threshold = rnd(&mut rng, 0.26, 0.05);
                        scale_x = rnd(&mut rng, 2.0, 1.0);
                        scale_y = 1.6;
                        contrast = 1.4;
                        warp_amp = rnd(&mut rng, 0.14, 0.08);
                    }
                    1 => { // Titan: one or two giant islands dominating the map
                        threshold = rnd(&mut rng, 0.20, 0.04);
                        scale_x = rnd(&mut rng, 1.2, 0.6);
                        scale_y = 1.2;
                        contrast = 1.6;
                        warp_amp = rnd(&mut rng, 0.20, 0.10);
                    }
                    _ => { // Staircase: islands arranged at varying heights with gaps
                        threshold = rnd(&mut rng, 0.28, 0.04);
                        scale_x = rnd(&mut rng, 2.5, 1.5);
                        scale_y = 2.0;
                        contrast = 1.3;
                        warp_amp = rnd(&mut rng, 0.10, 0.06);
                        terrace = Some(4.0);
                        terrace_mix = 0.20;
                    }
                }
            }
            3 => { // Caverns: fill+carve in Phase 2 below; density-field is skipped
                // Unused by density field but required by compiler.
                fade = 0.0; threshold = 0.5; scale_x = 1.0; contrast = 1.0;
            }
            _ => { // Canyon / mesa — sub-variants: slot canyon / badlands / fortress
                let sub = lcg(&mut rng) % 3;
                hill_amp = 0.10; // terracing defines silhouette, not rolling humps
                match sub {
                    0 => { // Slot canyon: deep narrow trenches, strong terracing
                        fade = rnd(&mut rng, 0.46, 0.10);
                        threshold = rnd(&mut rng, 0.49, 0.04);
                        scale_x = rnd(&mut rng, 2.5, 1.5);
                        contrast = 1.25;
                        let dir = if lcg(&mut rng) & 1 == 0 { 1.0f64 } else { -1.0 };
                        cliff_bias = dir * rnd(&mut rng, 0.12, 0.16);
                        terrace = Some(rnd(&mut rng, 4.0, 3.0).round());
                        terrace_mix = 0.50; // was 0.30 — terracing now dominant
                        warp_amp = 0.08;
                    }
                    1 => { // Badlands: heavy terracing, eroded pillars, strong warp
                        fade = rnd(&mut rng, 0.42, 0.10);
                        threshold = rnd(&mut rng, 0.51, 0.04);
                        scale_x = rnd(&mut rng, 4.0, 2.0);
                        contrast = 1.50;
                        cliff_bias = rnd(&mut rng, -0.08, 0.16);
                        terrace = Some(rnd(&mut rng, 5.0, 3.0).round());
                        terrace_mix = 0.72; // was 0.55 — mesas dominate over mounds
                        warp_amp = rnd(&mut rng, 0.14, 0.10);
                    }
                    _ => { // Fortress: flat-topped mesa with sheer walls + moat
                        fade = rnd(&mut rng, 0.52, 0.08);
                        threshold = rnd(&mut rng, 0.53, 0.03);
                        scale_x = rnd(&mut rng, 1.8, 1.0);
                        contrast = 1.70;
                        let dir = if lcg(&mut rng) & 1 == 0 { 1.0f64 } else { -1.0 };
                        cliff_bias = dir * rnd(&mut rng, 0.20, 0.16);
                        terrace = Some(2.0); // just two levels: mesa top + ground
                        terrace_mix = 0.82; // was 0.65 — very hard step between levels
                        warp_amp = rnd(&mut rng, 0.06, 0.04);
                    }
                }
                cave = lcg(&mut rng) % 100 < 50;
            }
        }

        // Water margins always applied — wide enough to be visible on both sides
        let water_end_px: f64 = 180.0 + (lcg(&mut rng) & 0xFF) as f64 / 255.0 * 170.0; // 180–350px

        // Island blob centers — masses floating high in the sky (islands only).
        // cy is kept in the upper band (0.18–0.50 of WORLD_H ≈ y 86–240) so there is
        // a clear air/water gap beneath every island. A few big islands plus some
        // small "stepping-stone" islands for traversal.
        let island_blobs: Vec<(f64, f64, f64)> = if blob {
            let big = 3 + (lcg(&mut rng) % 3) as usize;   // 3–5 main islands
            let small = 2 + (lcg(&mut rng) % 3) as usize; // 2–4 stepping stones
            let mut v = Vec::with_capacity(big + small);
            for _ in 0..big {
                v.push((
                    rnd(&mut rng, 0.10, 0.80), // cx 0.10–0.90
                    rnd(&mut rng, 0.18, 0.32), // cy 0.18–0.50 (high in the sky)
                    rnd(&mut rng, 0.14, 0.12), // r  0.14–0.26
                ));
            }
            for _ in 0..small {
                v.push((
                    rnd(&mut rng, 0.10, 0.80),
                    rnd(&mut rng, 0.22, 0.30), // 0.22–0.52
                    rnd(&mut rng, 0.05, 0.05), // r  0.05–0.10 (small)
                ));
            }
            v
        } else {
            vec![]
        };

        // Macro shaping (rolling hills + top headroom). Applied to every archetype
        // EXCEPT floating islands (they float high by design) and caverns (exempt for
        // now — caverns keeps its original surface shape; only its spawns change).
        let rolling = !blob && archetype != 3;
        let hill_freq = rnd(&mut rng, 2.8, 1.8);   // 2.8–4.6 cycles: several hills per map (visible on-screen)
        // Per-archetype: hills get full relief; cliffs/canyons get much less so their
        // ridged/terraced features dominate instead of producing mound-on-mound maps.
        #[allow(non_snake_case)]
        let HILL_AMP: f64 = hill_amp;
        const SKY_BAND: f64 = 0.12;                // top 12% tapers off → ~84px guaranteed headroom

        // Precompute per-column hill relief (only depends on x, not y) to avoid
        // redundant noise evaluations inside the hot y-loop. Saves ~2×WORLD_W×region_h
        // noise calls on rolling archetypes.
        let hill_col: Vec<f64> = if rolling {
            (0..WORLD_W as usize).map(|x| {
                let nx = x as f64 / WORLD_W as f64;
                let relief = hill.get([nx * hill_freq,       0.7])
                           + 0.30 * hill.get([nx * hill_freq * 4.0, 3.1]);
                (relief / 1.30) * HILL_AMP
            }).collect()
        } else {
            Vec::new()
        };

        // ── Phase 2: Density field (skipped for cave maps — they use fill+carve) ──
        if archetype == 3 {
            // Cave maps start from a completely solid rock band and carve air out.
            // This gives the Worms Armageddon signature look: enclosed chambers,
            // textured rock walls, vertical shafts from sky to cave system.

            // proportional to terrain height so cave maps scale with WORLD_H
            let sky_floor: i32 = TERRAIN_MIN_Y as i32 + (terrain_range_f * 0.14) as i32; // ~178px: sky opening above
            let cave_floor: i32 = WATER_Y as i32 - (terrain_range_f * 0.10) as i32;      // ~770px: solid base below
            #[allow(non_snake_case)] let SKY_FLOOR = sky_floor;
            #[allow(non_snake_case)] let CAVE_FLOOR = cave_floor;

            // A — Fill solid rock: entire map above water is solid to start.
            // The top zone (0..SKY_FLOOR) is pure solid rock — no surface layer, no sky.
            // WA cavern maps are fully enclosed; everything is underground.
            for y in 0..CAVE_FLOOR {
                for x in 0..WORLD_W as i32 {
                    terrain.set_solid(x, y, true);
                }
            }

            // B — Organic noise seeding (Worms-style). Carve air where a layered
            // OpenSimplex field falls below a threshold, biased into 2–3 stacked
            // horizontal layers so the result has multiple vertical levels rather
            // than one blob. Cellular automata (step C) then rounds it into caverns,
            // pillars and overhangs.
            let t_air = rnd(&mut rng, 0.40, 0.05);                 // ~0.40–0.45 air threshold (solid-dominant rock)
            let fx = 6.0 + (lcg(&mut rng) % 5) as f64;             // 6–10 horizontal cycles
            let fy = 5.0 + (lcg(&mut rng) % 4) as f64;             // 5–8 vertical cycles
            let layers = 2.0 + (lcg(&mut rng) % 2) as f64;         // 2–3 stacked levels
            const CONTRAST: f64 = 2.0;                             // expand noise spread → clean chambers/tunnels
            let band_span = (CAVE_FLOOR - SKY_FLOOR) as f64;
            for y in SKY_FLOOR..CAVE_FLOOR {
                let ny = y as f64 / WORLD_H as f64;
                let band = (y - SKY_FLOOR) as f64 / band_span;     // 0 at ceiling → 1 at floor
                for x in 6..WORLD_W as i32 - 6 {                    // keep a solid edge guard
                    let nx = x as f64 / WORLD_W as f64;
                    // 3-octave FBM from cave_a + a low-freq cave_b warp octave.
                    let mut v = 0.0f64;
                    let mut amp = 1.0f64;
                    let mut fr = 1.0f64;
                    let mut norm = 0.0f64;
                    for _ in 0..3 {
                        v += cave_a.get([nx * fx * fr, ny * fy * fr]) * amp;
                        norm += amp;
                        amp *= 0.5;
                        fr *= 2.0;
                    }
                    v += cave_b.get([nx * 1.7, ny * 3.0]) * 0.4;
                    norm += 0.4;
                    let mut d = (v / norm + 1.0) * 0.5;             // normalize to ~[0,1]
                    // Contrast-stretch so the field spans the full range — this is what
                    // turns the noise into distinct caverns, pillars and overhangs
                    // rather than a flat sheet hovering near the threshold.
                    d = ((d - 0.5) * CONTRAST + 0.5).clamp(0.0, 1.0);
                    // Vertical-layering as a SUBTLE threshold nudge only: air gathers a
                    // little more toward band centers and solid toward band edges
                    // (floors/ceilings), but noise still dictates the actual shapes.
                    let layer = (band * layers * std::f64::consts::PI).sin().abs(); // 0 edges → 1 centers
                    let local_thresh = t_air + (layer - 0.5) * 0.18;
                    if d < local_thresh {
                        terrain.set_solid(x, y, false);
                    }
                }
            }

            // C — Cellular automata (Moore/8-neighbour) for rounded WA cave walls.
            // Rows outside the rock band count as solid so the sky crust / base
            // aren't eroded away. 3–4 passes round the noise field into smooth
            // caverns, leaving freestanding pillars where cells stay locally dense.
            let ca_passes = 3 + (lcg(&mut rng) % 2);
            for _ in 0..ca_passes {
                let snap = terrain.solid.clone();
                for y in 1..CAVE_FLOOR - 1 {
                    for x in 1..WORLD_W as i32 - 1 {
                        let mut solid_n = 0;
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                if dx == 0 && dy == 0 { continue; }
                                let yy = y + dy;
                                let s = if yy < 0 || yy >= CAVE_FLOOR {
                                    true // outside the band counts as solid
                                } else {
                                    snap[world_index((x + dx) as u32, yy as u32)]
                                };
                                if s { solid_n += 1; }
                            }
                        }
                        terrain.set_solid(x, y, solid_n >= 5);
                    }
                }
            }

            // C.5 — Jagged ceiling: carve irregular stalactite-like bumps into the
            // ceiling bottom (the SKY_FLOOR boundary) so it reads as rock, not a
            // flat cut. Two noise octaves: broad humps (20–40px deep) + fine spikes (5–15px).
            {
                let stala_depth_broad = 20.0 + (lcg(&mut rng) % 20) as f64;
                let stala_depth_fine  = 5.0  + (lcg(&mut rng) % 10) as f64;
                for x in 0..WORLD_W as i32 {
                    let nx = x as f64 / WORLD_W as f64;
                    let broad = cave_a.get([nx * 4.0, 77.3]);  // -1..1
                    let fine  = cave_b.get([nx * 14.0, 33.1]); // -1..1
                    let drop = (broad * stala_depth_broad + fine * stala_depth_fine).max(0.0) as i32;
                    // Carve air into the ceiling below SKY_FLOOR down to SKY_FLOOR+drop.
                    // Only carve — never expose pixels above SKY_FLOOR (they stay solid rock).
                    for y in SKY_FLOOR..=(SKY_FLOOR + drop).min(SKY_FLOOR + 45) {
                        terrain.set_solid(x, y, false);
                    }
                }
            }

            // C.6 — Air dilation: widen all air passages so soldiers (14px wide, 20px tall)
            // can traverse them. Two passes of Moore-neighborhood dilation — each pass
            // expands existing air by 1px on all sides, ONLY within the rock band (SKY_FLOOR
            // and below). Never touch the sealed top zone (y < SKY_FLOOR).
            for _ in 0..2 {
                let snap = terrain.solid.clone();
                for y in SKY_FLOOR..CAVE_FLOOR - 1 {
                    for x in 1..WORLD_W as i32 - 1 {
                        if !snap[world_index(x as u32, y as u32)] { continue; }
                        let has_air_neighbor = (-1i32..=1).any(|dy| {
                            let yy = y + dy;
                            if yy < SKY_FLOOR || yy >= CAVE_FLOOR { return false; }
                            (-1i32..=1).any(|dx| {
                                if dx == 0 && dy == 0 { return false; }
                                let xx = x + dx;
                                if xx < 0 || xx >= WORLD_W as i32 { return false; }
                                !snap[world_index(xx as u32, yy as u32)]
                            })
                        });
                        if has_air_neighbor { terrain.set_solid(x, y, false); }
                    }
                }
            }

            // D — Ceiling shafts: tall chimneys punched up through the rock ceiling
            // into the sealed top, giving tall vertical climbing space and letting
            // ropes reach high. They stop short of the very top (always enclosed).
            let n_shafts = 3 + (lcg(&mut rng) % 3) as usize;
            let upper_third = SKY_FLOOR + (CAVE_FLOOR - SKY_FLOOR) / 3;
            let shaft_ceil  = 18; // shafts reach down from y=18 (never punch out)
            for _ in 0..n_shafts {
                let sx = 100 + (lcg(&mut rng) % (WORLD_W as u64 - 200)) as i32;
                let drift = (lcg(&mut rng) % 40) as i32 - 20;
                let shaft_r = 14 + (lcg(&mut rng) % 5) as i32; // 14–18 → 29–37px wide, fits a soldier
                dig_tunnel(&mut terrain, sx, shaft_ceil, sx + drift, upper_third, shaft_r);
            }

            // Re-seal: shafts may punch into the top zone. Fill it all solid.
            for y in 0..SKY_FLOOR {
                for x in 0..WORLD_W as i32 {
                    terrain.set_solid(x, y, true);
                }
            }

            // Water-margin erosion (same as other archetypes): taper rock near world edges
            let water_end_px_cave: f64 = 120.0 + (lcg(&mut rng) & 0xFF) as f64 / 255.0 * 80.0;
            for y in SKY_FLOOR as u32..CAVE_FLOOR as u32 {
                for x in 0..WORLD_W {
                    let edge_dist = (x as f64).min(WORLD_W as f64 - 1.0 - x as f64);
                    if edge_dist < water_end_px_cave {
                        let t = (edge_dist / water_end_px_cave) as f64;
                        let smooth_t = t * t * (3.0 - 2.0 * t);
                        if smooth_t < 0.55 {
                            terrain.set_solid(x as i32, y as i32, false);
                        }
                    }
                }
            }

            // E — Air-region connectivity guarantee. Flood-fill every air pocket in
            // the rock band; keep the largest as the main traversable region. Small
            // sealed pockets are filled solid (no soldier stranded inside); larger
            // isolated pockets are tunnel-connected to the main region so every
            // spawnable cave floor is reachable. Deterministic: index-based flood
            // fill in scan order, no HashSet.
            {
                const POCKET_MIN: usize = 400;
                let mut visited = vec![false; WORLD_PIXELS];
                let mut comps: Vec<Vec<(i32, i32)>> = Vec::new();
                let mut stack: Vec<(i32, i32)> = Vec::new();
                for sy in SKY_FLOOR..CAVE_FLOOR {
                    for sx in 0..WORLD_W as i32 {
                        let i0 = world_index(sx as u32, sy as u32);
                        if terrain.solid[i0] || visited[i0] { continue; }
                        stack.clear();
                        let mut comp: Vec<(i32, i32)> = Vec::new();
                        stack.push((sx, sy));
                        visited[i0] = true;
                        while let Some((cxp, cyp)) = stack.pop() {
                            comp.push((cxp, cyp));
                            for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                                let nxp = cxp + dx;
                                let nyp = cyp + dy;
                                if nxp < 0 || nxp >= WORLD_W as i32 { continue; }
                                if nyp < 0 || nyp >= CAVE_FLOOR { continue; }
                                let j = world_index(nxp as u32, nyp as u32);
                                if !terrain.solid[j] && !visited[j] {
                                    visited[j] = true;
                                    stack.push((nxp, nyp));
                                }
                            }
                        }
                        comps.push(comp);
                    }
                }
                if !comps.is_empty() {
                    let main_idx = (0..comps.len())
                        .max_by_key(|&i| comps[i].len())
                        .unwrap();
                    for i in 0..comps.len() {
                        if i == main_idx { continue; }
                        if comps[i].len() < POCKET_MIN {
                            for (cxp, cyp) in &comps[i] { terrain.set_solid(*cxp, *cyp, true); }
                        } else {
                            // Tunnel from the pocket centroid to the nearest main cell.
                            let (mut sxs, mut sys) = (0i64, 0i64);
                            for (cxp, cyp) in &comps[i] { sxs += *cxp as i64; sys += *cyp as i64; }
                            let n = comps[i].len() as i64;
                            let pcx = (sxs / n) as i32;
                            let pcy = (sys / n) as i32;
                            let mut best = comps[main_idx][0];
                            let mut best_d = i64::MAX;
                            for &(mxp, myp) in &comps[main_idx] {
                                let dd = (mxp - pcx) as i64 * (mxp - pcx) as i64
                                    + (myp - pcy) as i64 * (myp - pcy) as i64;
                                if dd < best_d { best_d = dd; best = (mxp, myp); }
                            }
                            dig_tunnel(&mut terrain, pcx, pcy, best.0, best.1, 10);
                        }
                    }
                }
            }

            // F — Final ceiling re-seal: connectivity tunnels may punch through.
            for y in 0..SKY_FLOOR {
                for x in 0..WORLD_W as i32 {
                    terrain.set_solid(x, y, true);
                }
            }
        }

        if archetype != 3 {
        // Continuous density buffer for the terrain region [TERRAIN_MIN_Y, WATER_Y).
        // We fill this per-pixel below, then box-blur it before thresholding so the
        // silhouette comes out smooth/organic instead of following every noise wiggle.
        // 0.0 doubles as the AIR pad value for out-of-region neighbours (it sits well
        // below `threshold` ≈ 0.5). Deterministic f64 math, no RNG draws → identical
        // on client and server for a given seed.
        let region_w = WORLD_W as usize;
        let region_h = (WATER_Y - TERRAIN_MIN_Y) as usize;
        let mut dens = vec![0.0f64; region_w * region_h];

        for y in TERRAIN_MIN_Y as usize..WATER_Y as usize {
            let ny = y as f64 / WORLD_H as f64;
            let ty = (y as f64 - TERRAIN_MIN_Y as f64) / terrain_range_f;
            for x in 0..WORLD_W as usize {
                let nx = x as f64 / WORLD_W as f64;

                // 1. Domain warp (universal — breaks horizontal banding)
                let wx = warp_a.get([nx * warp_freq, ny * warp_freq]) * warp_amp;
                let wy = warp_b.get([nx * warp_freq + 5.3, ny * warp_freq]) * warp_amp;
                let sx = nx + wx;
                let sy = ny + wy;

                // 2. FBM, each octave mapped to [0,1], weighted-average normalized
                let mut val = 0.0f64;
                let mut amp = 1.0f64;
                let mut fr = 1.0f64;
                let mut norm = 0.0f64;
                for _ in 0..octaves {
                    let n = base.get([sx * scale_x * fr, sy * scale_y * fr]);
                    let n = if ridged { 1.0 - n.abs() } else { (n + 1.0) * 0.5 };
                    val += n * amp;
                    norm += amp;
                    amp *= 0.5;
                    fr *= 2.0;
                }
                let mut noise = val / norm;

                // 3. Contrast/gain — restores variation amplitude (fixes the plateau)
                noise = (((noise - 0.5) * contrast) + 0.5).clamp(0.0, 1.0);

                // 4. Density: sourced from a real Worms Armageddon terrain silhouette
                // (see wa_templates) so every seed's macro shape is WA-styled. The old
                // per-archetype noise/blob gradient is folded in at reduced weight —
                // it still contributes fine edge texture and per-archetype lean/relief,
                // but no longer defines the silhouette itself.
                let mut density = super::wa_templates::wa_density(seed, nx, ty)
                    + (noise - 0.5) * 0.15
                    + (nx - 0.5) * cliff_bias * 0.3;
                if rolling {
                    density += hill_col[x] * 0.3;
                }
                let _ = (fade, blob, &island_blobs); // superseded by the WA silhouette

                // 4b. Top sky-margin (ALL archetypes): erode density near the top so
                // terrain tapers off below the ceiling instead of clamping flat against
                // TERRAIN_MIN_Y. Guarantees headroom and kills flat top-edge plateaus;
                // also pulls floating islands down off the very top of the screen.
                if ty < SKY_BAND {
                    let t = ty / SKY_BAND;
                    let smooth_t = t * t * (3.0 - 2.0 * t);
                    density -= (1.0 - smooth_t) * 0.85;
                }

                // 5. Terracing was designed to carve steps into a continuous synthetic
                // gradient; it fragments the real WA silhouette instead, so it's not
                // applied to the WA-sourced density (terrace/terrace_mix are unused now).
                let _ = (terrace, terrace_mix);

                // 6. Edge erosion for water on ends
                if water_end_px > 0.0 {
                    let edge_dist = (x as f64).min(WORLD_W as f64 - 1.0 - x as f64);
                    if edge_dist < water_end_px {
                        let t = edge_dist / water_end_px;
                        let smooth_t = t * t * (3.0 - 2.0 * t);
                        density -= (1.0 - smooth_t) * 0.55;
                    }
                }

                dens[(y - TERRAIN_MIN_Y as usize) * region_w + x] = density;
            }
        }

        // ── Phase 2c: Separable box blur of the density field, then threshold ─────
        // Rounds the contour where the field crosses `threshold` (metaball-style),
        // killing sub-~14px jaggedness while leaving the ≥104px relief that forces
        // jump/backflip intact. Blurring the CONTINUOUS field (not the binary mask)
        // keeps thin bridges / small stepping-stone islands that sit above threshold
        // solid — only their edges round — instead of eroding them away.
        // Islands use a gentler radius (their blobs are smaller and already rounded).
        let r: i32 = if blob { 4 } else { 5 };
        let mut tmp = vec![0.0f64; region_w * region_h];
        // Horizontal pass: clamp x at the region edges (terrain continues sideways).
        for ry in 0..region_h {
            let row = ry * region_w;
            for rx in 0..region_w as i32 {
                let mut sum = 0.0;
                let mut cnt = 0.0;
                for dx in -r..=r {
                    let xx = (rx + dx).clamp(0, region_w as i32 - 1) as usize;
                    sum += dens[row + xx];
                    cnt += 1.0;
                }
                tmp[row + rx as usize] = sum / cnt;
            }
        }
        // Vertical pass: rows outside [0, region_h) read as AIR (0.0) so the top
        // tapers to sky and the bottom to water rather than smearing solid.
        for rx in 0..region_w {
            for ry in 0..region_h as i32 {
                let mut sum = 0.0;
                let mut cnt = 0.0;
                for dy in -r..=r {
                    let yy = ry + dy;
                    let v = if yy < 0 || yy >= region_h as i32 {
                        0.0
                    } else {
                        tmp[yy as usize * region_w + rx]
                    };
                    sum += v;
                    cnt += 1.0;
                }
                dens[ry as usize * region_w + rx] = sum / cnt;
            }
        }
        // Threshold the smoothed field into the solid bitmap. Islands drop the
        // threshold slightly: the blur pulls a small blob's contour inward, so this
        // keeps the smallest stepping-stones above threshold (and above the
        // min_frag=50 cleanup floor) instead of vanishing.
        let thr = if blob { threshold - 0.03 } else { threshold - 0.02 };
        for ry in 0..region_h {
            let y = TERRAIN_MIN_Y as usize + ry;
            for x in 0..region_w {
                terrain.set_solid(x as i32, y as i32, dens[ry * region_w + x] >= thr);
            }
        }
        } // end if archetype != 3

        // ── Phase 2a: Sky clearance (hills + canyon only) ────────────────────────
        // Keep the top 40% of the screen (y < 192) free of terrain on flat maps.
        // Overhangs (1), islands (2), and caverns (3) are exempt — they intentionally
        // use the upper screen area.
        if matches!(archetype, 0 | 4) {
            // Keep the top portion of the terrain zone clear on flat maps; scale with world height.
            let sky_floor_clear: u32 = TERRAIN_MIN_Y + (terrain_range_f * 0.14) as u32;
            for y in TERRAIN_MIN_Y..sky_floor_clear.min(WATER_Y) {
                for x in 0..WORLD_W as i32 {
                    terrain.set_solid(x, y as i32, false);
                }
            }
        }

        // ── Phase 2b: Overhang shelves (cliffs archetype) ─────────────────────────
        // A monotonic density field can't fold over itself, so genuine overhangs are
        // stamped explicitly: a horizontal slab floats above the local surface with an
        // air gap beneath, and one end is anchored to the ground by a support column so
        // the slab is part of the main terrain (survives flood-fill) and reads as a
        // cantilevered ledge / arch.
        if overhang {
            let shelves = 2 + (lcg(&mut rng) % 3) as usize; // 2–4
            for _ in 0..shelves {
                let cx = (rnd(&mut rng, 0.18, 0.64) * WORLD_W as f64) as i32; // 0.18–0.82
                let ground = terrain.surface_y_at(cx as u32)
                    .unwrap_or(TERRAIN_MAX_Y) as i32;
                let gap   = rnd(&mut rng, 30.0, 45.0) as i32;  // 30–75px air gap
                let shelf_y = (ground - gap).max(TERRAIN_MIN_Y as i32 + 6);
                let half_w = rnd(&mut rng, 45.0, 55.0) as i32; // 45–100px reach
                let th     = (rnd(&mut rng, 9.0, 10.0) as i32).max(6); // 9–19px thick
                let dir: i32 = if lcg(&mut rng) & 1 == 0 { 1 } else { -1 };

                for dx in -half_w..=half_w {
                    let x = cx + dx;
                    if x < 4 || x >= WORLD_W as i32 - 4 { continue; }
                    // Thin toward the free (cantilever) tip, full at the anchor.
                    let tnorm = (dx * dir) as f32 / half_w as f32; // -1 anchor .. +1 tip
                    let taper = (1.0 - ((tnorm + 1.0) * 0.5) * 0.55).max(0.4);
                    let bot = shelf_y + ((th as f32) * taper) as i32;
                    for y in shelf_y..=bot { terrain.set_solid(x, y, true); }
                }
                // Support column at the anchor end → connects slab to main terrain.
                let anchor_x = cx - dir * half_w;
                let col_bot = ground.max(shelf_y);
                for ax in (anchor_x - 5)..=(anchor_x + 5) {
                    if ax < 4 || ax >= WORLD_W as i32 - 4 { continue; }
                    for y in shelf_y..=col_bot { terrain.set_solid(ax, y, true); }
                }
            }
        }

        // ── Phase 3: Cave punch ───────────────────────────────────────────────────
        // Carve air tunnels where two-layer cave noise lands in a band. Skip the
        // surface crust (ty<=0.18) so spawning stays reliable.
        if cave {
            // Caverns let caves reach daylight (thin crust); others keep a thick crust
            // so spawning on the surface stays reliable.
            let crust = if surface_caves { 0.10 } else { 0.18 };
            for y in TERRAIN_MIN_Y..WATER_Y {
                let ty = (y as f64 - TERRAIN_MIN_Y as f64) / terrain_range_f;
                if ty <= crust { continue; }
                let ny = y as f64 / WORLD_H as f64;
                for x in 0..WORLD_W {
                    if !terrain.is_solid(x as i32, y as i32) { continue; }
                    let nx = x as f64 / WORLD_W as f64;
                    let c = (cave_a.get([nx * cave_sx, ny * cave_sy])
                           + cave_b.get([nx * cave_sx * 0.6 + 100.0, ny * cave_sy * 0.6 + 100.0]) * 0.5) / 1.5;
                    if c.abs() < cave_thresh {
                        terrain.set_solid(x as i32, y as i32, false);
                    }
                }
            }
        }

        // Phase 3.5 — Air dilation for cave-punched non-cavern maps.
        // Same 2-pass Moore dilation as archetype 3, but clamped below the crust
        // (ty > 0.18) so surface terrain is not eroded.
        if cave && archetype != 3 {
            let cave_min_y = TERRAIN_MIN_Y as i32 + (terrain_range_f * 0.18) as i32 + 1;
            for _ in 0..2 {
                let snap = terrain.solid.clone();
                for y in cave_min_y..WATER_Y as i32 - 1 {
                    for x in 1..WORLD_W as i32 - 1 {
                        if !snap[world_index(x as u32, y as u32)] { continue; }
                        let has_air_neighbor = (-1i32..=1).any(|dy| {
                            let yy = y + dy;
                            if yy < cave_min_y || yy >= WATER_Y as i32 { return false; }
                            (-1i32..=1).any(|dx| {
                                if dx == 0 && dy == 0 { return false; }
                                let xx = x + dx;
                                if xx < 0 || xx >= WORLD_W as i32 { return false; }
                                !snap[world_index(xx as u32, yy as u32)]
                            })
                        });
                        if has_air_neighbor { terrain.set_solid(x, y, false); }
                    }
                }
            }
        }

        // ── Phase 4: Big void (caverns) ───────────────────────────────────────────
        if big_void {
            let void_center_y = TERRAIN_MIN_Y as f64 + terrain_range_f * 0.40;
            let void_half_h   = terrain_range_f * 0.24; // ≈69px half-height
            for y in TERRAIN_MIN_Y..WATER_Y {
                let ny = y as f64 / WORLD_H as f64;
                let dy = (y as f64 - void_center_y).abs();
                for x in 0..WORLD_W {
                    if !terrain.is_solid(x as i32, y as i32) { continue; }
                    let nx = x as f64 / WORLD_W as f64;
                    if nx < 0.10 || nx > 0.90 { continue; } // keep 10% margins solid
                    let void_noise = cave_a.get([nx * 3.0, ny * 5.0]) * 22.0;
                    if dy < void_half_h + void_noise {
                        terrain.set_solid(x as i32, y as i32, false);
                    }
                }
            }
        }

        // ── Phase 4b: Cavern entrance shafts ──────────────────────────────────────
        // Vertical tunnels from the surface down into the big void so the chamber is
        // visible from outside and soldiers can rope/climb in.
        for _ in 0..void_shafts {
            let sxn = (rnd(&mut rng, 0.15, 0.70) * WORLD_W as f64) as i32;
            let void_center_y = TERRAIN_MIN_Y as i32 + (terrain_range_f * 0.40) as i32;
            let top = terrain.surface_y_at(sxn as u32).unwrap_or(TERRAIN_MIN_Y) as i32;
            let r = 7 + (lcg(&mut rng) % 8) as i32;        // 7–14 wide
            let drift = rnd(&mut rng, -25.0, 50.0) as i32; // winding
            dig_tunnel(&mut terrain, sxn, top.max(TERRAIN_MIN_Y as i32),
                       sxn + drift, void_center_y, r);
        }

        // ── Phase 5: Chasms (hills / cliffs / canyon) ─────────────────────────────
        // Vertical slots that make crossing the map a skill challenge: some cut to the
        // water (a mis-judged jump drowns), some are floored pits; widths span jumpable
        // (≤56px) to grapple-only (80–160px). Confined to the CENTRAL contested zone so
        // each team keeps a large chasm-free home landform of comparable size on its own
        // side — neither team is stranded on smaller bits than the other. The bumpy
        // 3-octave relief still makes the home sides a ledge-hopping challenge.
        let n_chasms = if matches!(archetype, 0 | 1 | 4) {
            3 + (lcg(&mut rng) % 3) as usize // 3–5
        } else { 0 };
        // Spread chasms across more of the map for WA-style terrain variety.
        let zone_lo = (0.22 * WORLD_W as f64) as i32; // ~422
        let zone_hi = (0.78 * WORLD_W as f64) as i32; // ~1498
        for _ in 0..n_chasms {
            // Width: ~55% jumpable, ~45% grapple-only.
            let half_w = if lcg(&mut rng) % 100 < 55 {
                10 + (lcg(&mut rng) % 19) as i32 // 10–28  → gap 20–56px
            } else {
                40 + (lcg(&mut rng) % 41) as i32 // 40–80  → gap 80–160px
            };
            // Keep the whole slot inside the central zone (home sides stay solid).
            let lo_c = zone_lo + half_w;
            let hi_c = (zone_hi - half_w).max(lo_c + 1);
            let cx = lo_c + (lcg(&mut rng) % (hi_c - lo_c) as u64) as i32;
            let drift = rnd(&mut rng, -40.0, 80.0) as i32; // ±40px lean
            // Depth: ~45% straight to the water (drowning), else a floored pit whose
            // walls (60–95px) are too tall to jump out of → backflip-chain or grapple.
            let bottom_y = if lcg(&mut rng) % 100 < 45 {
                WATER_Y as i32
            } else {
                let surf = terrain.surface_y_at(cx as u32).unwrap_or(TERRAIN_MAX_Y) as i32;
                (surf + rnd(&mut rng, 60.0, 35.0) as i32).min(WATER_Y as i32 - 6)
            };
            let chasm_top = if matches!(archetype, 0 | 4) {
                TERRAIN_MIN_Y as i32 + (terrain_range_f * 0.14) as i32
            } else { TERRAIN_MIN_Y as i32 };
            carve_chasm(&mut terrain, cx, half_w, chasm_top, bottom_y, drift);
        }

        // ── Phase 6a: Isolated-pixel removal (remove 1-pixel spikes) ──────────────
        for _ in 0..2 {
            let snap = terrain.solid.clone();
            for y in 1..WATER_Y as i32 - 1 {
                for x in 1..WORLD_W as i32 - 1 {
                    let i = world_index(x as u32, y as u32);
                    let neighbors = [(-1,0),(1,0),(0,-1),(0,1)].iter()
                        .filter(|(dx,dy)| snap[world_index((x+dx) as u32, (y+dy) as u32)])
                        .count();
                    if snap[i] && neighbors == 0 { terrain.solid[i] = false; }
                }
            }
        }

        // ── Phase 6b: Flood-fill fragment cleanup ─────────────────────────────────
        // Remove solid components smaller than min_frag (noise junk / tiny floaters).
        // Islands use a lower bar so small stepping-stones survive.
        {
            // Islands use a low bar so small stepping-stone islands survive; the
            // isolated-pixel pass above already removes single-pixel noise.
            let min_frag: usize = if blob { 50 } else { 200 };
            let mut visited = vec![false; WORLD_PIXELS];
            let mut stack: Vec<(i32, i32)> = Vec::new();
            let mut comp: Vec<(i32, i32)> = Vec::new();
            for y0 in TERRAIN_MIN_Y..WATER_Y {
                for x0 in 0..WORLD_W {
                    let i0 = world_index(x0, y0);
                    if !terrain.solid[i0] || visited[i0] { continue; }
                    stack.clear();
                    comp.clear();
                    stack.push((x0 as i32, y0 as i32));
                    visited[i0] = true;
                    while let Some((cx, cy)) = stack.pop() {
                        comp.push((cx, cy));
                        for (dx, dy) in [(-1,0),(1,0),(0,-1),(0,1)] {
                            let nxp = cx + dx;
                            let nyp = cy + dy;
                            if nxp < 0 || nxp >= WORLD_W as i32 { continue; }
                            if nyp < TERRAIN_MIN_Y as i32 || nyp >= WATER_Y as i32 { continue; }
                            let j = world_index(nxp as u32, nyp as u32);
                            if terrain.solid[j] && !visited[j] {
                                visited[j] = true;
                                stack.push((nxp, nyp));
                            }
                        }
                    }
                    if comp.len() < min_frag {
                        for (cx, cy) in &comp { terrain.set_solid(*cx, *cy, false); }
                    }
                }
            }
        }

        // Populate spawn_y from topmost solid pixel per column.
        // Scan from y=0 so solid pixels above TERRAIN_MIN_Y (sky floaters, overhangs)
        // don't cause a wrong depth=0 fallback for the entire column.
        // Clamp the reference to TERRAIN_MIN_Y so rare high-altitude pixels don't
        // make deeper pixels appear absurdly deep in the texture.
        for x in 0..WORLD_W as usize {
            let topmost = (0..WATER_Y).find(|&y| terrain.is_solid(x as i32, y as i32));
            terrain.spawn_y[x] = topmost.map(|y| y.max(TERRAIN_MIN_Y)).unwrap_or(TERRAIN_MAX_Y as u32);
            let sky_limit = topmost.unwrap_or(WATER_Y);
            terrain.sky_limit[x] = sky_limit;
            let solid_to_water = sky_limit < WATER_Y
                && (sky_limit..WATER_Y).all(|y| terrain.is_solid(x as i32, y as i32));
            terrain.solid_to_water[x] = solid_to_water;
            terrain.solid_runs[x] = if solid_to_water {
                Vec::new()
            } else {
                terrain.solid_runs_for_column(x as i32, sky_limit)
            };
        }

        // Phase 7 (per-column spawn mounds) intentionally removed: spawns are now
        // chosen after generation by `find_team_spawns`, which lands soldiers on the
        // real terrain (island tops, cliff ledges, cavern floors) without re-grounding
        // the map. This is what lets islands/caverns/overhangs survive to the screen.

        terrain.texture = Some(Self::generate_dirt_texture(seed));

        // ── Scenery object placement ─────────────────────────────────────────
        // Derived entirely from seed — same on client and server, no StateMsg needed.
        {
            let sprite_counts: [u8; 5] = [8, 7, 7, 7, 7]; // per archetype
            let count = sprite_counts[terrain.archetype as usize];
            let mut srng = seed ^ 0xDECA_FBAB_E000_1234u64;
            let margin = (WORLD_W as f64 * 0.05) as u32;
            let usable_w = WORLD_W - 2 * margin;
            const NUM_OBJECTS: u32 = 28;
            const MIN_SPACING: u32 = 110;
            let mut placed: Vec<SceneryObject> = Vec::with_capacity(NUM_OBJECTS as usize);
            for _ in 0..NUM_OBJECTS * 6 {
                srng = srng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let col = margin + (srng >> 33) as u32 % usable_w;
                let surface_y = terrain.spawn_y[col as usize];
                // Skip columns with no real ground (sky_limit == WATER_Y means bare water column)
                if terrain.sky_limit[col as usize] >= WATER_Y { continue; }
                if surface_y >= WATER_Y { continue; }
                // Enforce minimum horizontal spacing
                if placed.iter().any(|o| o.x.abs_diff(col) < MIN_SPACING) { continue; }
                srng = srng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let sprite = (srng >> 33) as u8 % count;
                placed.push(SceneryObject { x: col, y: surface_y, sprite });
                if placed.len() == NUM_OBJECTS as usize { break; }
            }
            terrain.scenery = placed;
        }

        terrain
    }

    /// Pick deterministic spawn positions for a team, scanning the real
    /// post-generation terrain. Pure function of the terrain (identical on client
    /// and server). Candidates are restricted to the interior `[x_lo, x_hi]` band
    /// (callers pass left/right halves) and never within `SPAWN_EDGE_MARGIN` of a
    /// world edge. Returns up to `count` well-separated standable spots; if the
    /// terrain is too sparse it stamps a small interior platform as a last resort so
    /// a team always gets its soldiers.
    pub fn find_team_spawns(&mut self, x_lo: u32, x_hi: u32, count: usize) -> Vec<WorldPos> {
        // 140px keeps same-team soldiers far enough apart that one explosion can't
        // gut two of them: TNT (the biggest blast, r=75) centred between two does only
        // ~7 dmg each, and every other weapon does 0 to a neighbour.
        const MIN_SEP: i32 = 140;  // horizontal spacing between a team's soldiers
        let lo = x_lo.max(SPAWN_EDGE_MARGIN) as i32;
        let hi = (x_hi.min(WORLD_W - SPAWN_EDGE_MARGIN) as i32).max(lo + 1);

        let mut spawns: Vec<WorldPos> = Vec::with_capacity(count);
        let mut used_x: Vec<i32> = Vec::with_capacity(count);

        // Cave maps (WA style): all soldiers spawn underground. No surface layer exists.
        if self.archetype == 3 {
            let mut cave_cands: Vec<(i32, i32)> = Vec::new();
            let mut x = lo;
            while x <= hi {
                if let Some(fy) = self.standable_cave_foot_y(x) {
                    cave_cands.push((x, fy));
                }
                x += 1;
            }
            for _i in 0..count {
                let pools: [&[(i32, i32)]; 1] = [&cave_cands];
                'slot: for pool in pools {
                    for &(cx, cy) in pool {
                        if used_x.iter().all(|&ux| (ux - cx).abs() >= MIN_SEP) {
                            spawns.push(WorldPos::new(cx as f32, cy as f32));
                            used_x.push(cx);
                            break 'slot;
                        }
                    }
                }
            }
            if spawns.len() >= count { return spawns; }
            // Fall through to generic surface spawning if still short.
        }

        // Pre-scan for underground cave floors so we can reserve slots for them.
        // Maps with punched caves (archetypes 0,1,2,4) should seat some soldiers
        // underground for vertical variety; cap the surface pass so those slots stay open.
        let cave_quota = if self.archetype != 3 {
            let mut n = 0usize;
            let mut cx = lo + 60;
            while cx <= hi - 60 {
                if self.standable_cave_foot_simple(cx).is_some() { n += 1; }
                cx += 120;
            }
            (count / 2).min(n)
        } else { 0 };
        let surface_cap = count.saturating_sub(cave_quota);

        // ── Surface spawns on substantial landforms of similar size ───────────────
        // Group standable surface columns into landform "tops": runs of columns whose
        // surface is continuous (no chasm gap) and at a similar height. Thin tops
        // (pillars/columns left by chasms) are rejected, and the team is placed on the
        // WIDEST tops first — so soldiers share comparable ground instead of each being
        // marooned on its own column.
        const MIN_LAND_W:    i32 = 60; // a spawn landform top must be at least this wide
        const GAP_TOL:       i32 = 12; // x-gap (px) that breaks a landform (a chasm)
        const STEP_TOL:      i32 = 45; // surface y-jump (px) that breaks a landform (a wall)
        const GROUND_DEPTH:  i32 = 26; // solid px required below the foot (excludes thin
                                       // floating shelves / cantilever tips — not real ground)

        let mut cands: Vec<(i32, i32)> = Vec::new(); // (x, foot_y), left→right
        let mut x = lo;
        while x <= hi {
            if let Some(fy) = self.standable_foot_y(x) {
                // Must stand on a solid mass, not a thin slab/ledge.
                if (1..=GROUND_DEPTH).all(|d| self.is_solid(x, fy + d)) {
                    cands.push((x, fy));
                }
            }
            x += 4;
        }
        // Segment candidates into landform tops.
        let mut segments: Vec<Vec<(i32, i32)>> = Vec::new();
        for &(cx, cy) in &cands {
            let split = match segments.last().and_then(|s| s.last()) {
                Some(&(px, py)) => (cx - px) > GAP_TOL || (cy - py).abs() > STEP_TOL,
                None => true,
            };
            if split { segments.push(Vec::new()); }
            segments.last_mut().unwrap().push((cx, cy));
        }
        // Keep tops wide enough to not be pillars; widest first ⇒ "similar size".
        let seg_w   = |s: &Vec<(i32, i32)>| s.last().unwrap().0 - s[0].0;
        let seg_top = |s: &Vec<(i32, i32)>| s.iter().map(|&(_, y)| y).min().unwrap();
        let hi_y = cands.iter().map(|&(_, y)| y).min().unwrap_or(TERRAIN_MIN_Y as i32);
        let mut wide: Vec<&Vec<(i32, i32)>> =
            segments.iter().filter(|s| seg_w(s) >= MIN_LAND_W).collect();
        // Widest first, but bias toward higher ground so a team lands on a hilltop/mesa
        // rather than down in a wide hollow when both exist.
        wide.sort_by_key(|s| -(seg_w(s) - (seg_top(s) - hi_y) / 3));

        // Spread soldiers EVENLY across each landform top (widest first), filling its
        // whole width instead of bunching them at one end — so a team is distributed
        // throughout its half. Never closer than MIN_SEP (one blast can't catch two).
        for seg in &wide {
            if spawns.len() >= surface_cap { break; }
            let x0 = seg[0].0;
            let x1 = seg.last().unwrap().0;
            let remaining = surface_cap - spawns.len();
            // How many fit on this top at the safe spacing, capped to what's still needed.
            let cap = ((x1 - x0) / MIN_SEP + 1).clamp(1, remaining as i32);
            let gap = ((x1 - x0) as f32 / (cap - 1).max(1) as f32).max(MIN_SEP as f32);
            for i in 0..cap {
                if spawns.len() >= surface_cap { break; }
                let target = x0 + (gap * i as f32) as i32;
                // Snap the evenly-spaced target to the nearest standable column that is
                // still ≥ MIN_SEP from everyone already placed.
                if let Some(&(cx, cy)) = seg.iter()
                    .filter(|&&(px, _)| used_x.iter().all(|&u| (u - px).abs() >= MIN_SEP))
                    .min_by_key(|&&(px, _)| (px - target).abs())
                {
                    spawns.push(WorldPos::new(cx as f32, cy as f32));
                    used_x.push(cx);
                }
            }
        }

        // Cave-floor spawns: for maps with punched caves (non-cavern archetypes),
        // actively mix underground positions in to spread soldiers vertically.
        // We reserved some slots from the surface pass (capped above); fill them here.
        if self.archetype != 3 && spawns.len() < count {
            let mut cx = lo + 60;
            while cx <= hi - 60 && spawns.len() < count {
                if let Some(fy) = self.standable_cave_foot_simple(cx) {
                    if used_x.iter().all(|&u| (u - cx).abs() >= MIN_SEP) {
                        spawns.push(WorldPos::new(cx as f32, fy as f32));
                        used_x.push(cx);
                    }
                }
                cx += 80;
            }
        }

        // Last resort (very fragmented/sparse half): the natural tops couldn't seat the
        // whole team (e.g. only a couple of narrow ridges exist). Rather than stamp ONE
        // flat slab and line the leftovers up on it — which bunches the team in a boxy
        // void — raise a SEPARATE rounded mound per leftover soldier, each dropped into
        // the emptiest gap across the half so the team stays spread out and the added
        // terrain reads as hills, not a platform.
        const MOUND_HW:   i32 = 70;  // mound half-width
        const MOUND_DROP: i32 = 55;  // crown→edge fall (rounded profile)
        const PAD:        i32 = MOUND_HW + 8;
        while spawns.len() < count {
            // Pick x at the midpoint of the widest gap between already-used soldiers
            // (and the half's ends), keeping ≥ MIN_SEP from every neighbour where the
            // half is wide enough to allow it.
            let mut px = (lo + hi) / 2;
            let mut best_d = -1;
            let mut probe = lo + PAD;
            while probe <= hi - PAD {
                let d = used_x.iter().map(|&u| (u - probe).abs()).min().unwrap_or(i32::MAX);
                if d > best_d { best_d = d; px = probe; }
                probe += 6;
            }
            px = px.clamp(lo + PAD, (hi - PAD).max(lo + PAD));

            // Crown height: blend with nearby soldiers' footing if any, else mid-terrain,
            // nudged per-mound so neighbouring hillocks differ in height.
            let near = used_x.iter().cloned()
                .filter(|&u| (u - px).abs() < 360)
                .min_by_key(|&u| (u - px).abs());
            let base_from_spawn = near.and_then(|u| spawns.iter()
                .min_by_key(|s| (s.x as i32 - u).abs())
                .map(|s| s.y as i32));
            let local_surf = self.surface_y_at(px as u32).map(|y| y as i32);
            let wobble = (((px as i64 * 2654435761) >> 6) & 31) as i32 - 15;
            let crown_y = base_from_spawn
                .or(local_surf.filter(|&y| y < WATER_Y as i32 - 30))
                .unwrap_or((TERRAIN_MIN_Y as i32 + TERRAIN_MAX_Y as i32) / 2 + 20)
                .saturating_add(wobble)
                .clamp(TERRAIN_MIN_Y as i32 + 50, WATER_Y as i32 - 96);

            // Raise the mound: rounded solid top, only ADD dirt (never gouge a taller
            // existing hill), and clear a tapered dome of sky above it for headroom.
            for dx in -MOUND_HW..=MOUND_HW {
                let cx = px + dx;
                if cx < 4 || cx >= WORLD_W as i32 - 4 { continue; }
                let f = (dx * dx) as f32 / (MOUND_HW * MOUND_HW) as f32; // 0 centre → 1 edge
                let top = crown_y + (f * MOUND_DROP as f32) as i32;
                for y in top..WATER_Y as i32 { self.set_solid(cx, y, true); }   // dirt mass
                let head = (110.0 * (1.0 - f)) as i32;                           // tapered sky
                for dy in 1..=head { self.set_solid(cx, top - dy, false); }
                // The headroom clear above can punch a hole into ground that was
                // previously solid between the old sky_limit and the new mound top
                // (when the mound doesn't raise the column's visible top), leaving
                // sky_limit/solid_to_water stale — recompute from the actual solid
                // bits so the renderer's sky-aware viewport copy doesn't block-copy
                // a cached placeholder over that new gap (and so a previously-empty
                // column's new mound is correctly picked up too).
                self.recompute_column_cache(cx);
                let top_u = top as u32;
                if top_u < self.spawn_y[cx as usize] { self.spawn_y[cx as usize] = top_u.max(TERRAIN_MIN_Y); }
            }
            spawns.push(WorldPos::new(px as f32, (crown_y - 1) as f32));
            used_x.push(px);
        }

        spawns
    }

    /// Highest foot Y at column `x` where a soldier can stand: foot pixel is air,
    /// the pixel below is solid, there's a ≥7px platform under the feet, and ≥100px
    /// of open sky above (rejects ceilings / enclosed caves). None if no such spot.
    pub fn standable_foot_y(&self, x: i32) -> Option<i32> {
        use crate::renderer::draw_sprites::SOLDIER_HALF_W;
        const CLEAR_H: i32 = 24; // soldier body + clearance
        const SKY_H:   i32 = 100;
        if x < 0 || x >= WORLD_W as i32 { return None; }
        let x_l = x - SOLDIER_HALF_W as i32;
        let x_r = x + SOLDIER_HALF_W as i32;
        let ok = |foot_y: i32| -> bool {
            // Body must fit in-world; high islands are fine (their open sky is
            // verified by the all-air scan below, not by a hard Y floor).
            if foot_y < CLEAR_H || foot_y >= WATER_Y as i32 { return false; }
            if !self.is_solid(x, foot_y + 1) { return false; }
            let platform = (-4..=4).filter(|&dx| self.is_solid(x + dx, foot_y + 1)).count() >= 7;
            if !platform { return false; }
            // Full body footprint must be clear, matching the tightened movement
            // collision (try_move_horizontal / airborne terrain_hit), so a soldier
            // never spawns wedged in a passage it can't legally move out of.
            (foot_y - CLEAR_H - SKY_H + 1 ..= foot_y).all(|y| {
                let y = y.max(0);
                !self.is_solid(x_l, y) && !self.is_solid(x, y) && !self.is_solid(x_r, y)
            })
        };
        // Scan top-down from the very top so we can land on high sky-islands (whose
        // tops sit above CLEAR_H+SKY_H) before any ground far below.
        (CLEAR_H..WATER_Y as i32).find(|&foot_y| ok(foot_y))
    }

    /// Highest *enclosed* foot Y at column `x`: a cave/void floor a soldier can stand
    /// on. Like `standable_foot_y` but the spot is roofed — there must be a solid
    /// ceiling overhead (so it is genuinely underground, not the open surface).
    /// Scans bottom-up so soldiers land on the main void floor, not a shallow tunnel.
    /// None if no roofed standing spot exists.
    /// Like standable_cave_foot_y but skips the escape-connectivity check.
    /// Used for archetype-3 (cave) spawns where vertical shafts guarantee reachability.
    pub fn standable_cave_foot_simple(&self, x: i32) -> Option<i32> {
        use crate::renderer::draw_sprites::SOLDIER_HALF_W;
        const HEAD_H: i32 = 26;
        const CEIL_MAX: i32 = 220;
        if x < 0 || x >= WORLD_W as i32 { return None; }
        let x_l = x - SOLDIER_HALF_W as i32;
        let x_r = x + SOLDIER_HALF_W as i32;
        let ok = |foot_y: i32| -> bool {
            if foot_y < HEAD_H || foot_y >= WATER_Y as i32 { return false; }
            if !self.is_solid(x, foot_y + 1) { return false; }
            let platform = (-4..=4).filter(|&dx| self.is_solid(x + dx, foot_y + 1)).count() >= 7;
            if !platform { return false; }
            if !(foot_y - HEAD_H + 1 ..= foot_y).all(|y| {
                let y = y.max(0);
                !self.is_solid(x_l, y) && !self.is_solid(x, y) && !self.is_solid(x_r, y)
            }) { return false; }
            // Must be underground (roofed), not on the open surface.
            ((foot_y - CEIL_MAX).max(0) ..= foot_y - HEAD_H).any(|y| self.is_solid(x, y))
        };
        // Bottom-up: prefer deeper floors (main chambers) over high thin tunnels.
        (HEAD_H..WATER_Y as i32).rev().find(|&foot_y| ok(foot_y))
    }

    pub fn standable_cave_foot_y(&self, x: i32) -> Option<i32> {
        use crate::renderer::draw_sprites::SOLDIER_HALF_W;
        const HEAD_H: i32 = 26;   // body + small clearance above the foot
        const CEIL_MAX: i32 = 220; // a ceiling must sit within this height to count as a cave
        if x < 0 || x >= WORLD_W as i32 { return None; }
        let x_l = x - SOLDIER_HALF_W as i32;
        let x_r = x + SOLDIER_HALF_W as i32;
        let ok = |foot_y: i32| -> bool {
            if foot_y < HEAD_H || foot_y >= WATER_Y as i32 { return false; }
            if !self.is_solid(x, foot_y + 1) { return false; }
            let platform = (-4..=4).filter(|&dx| self.is_solid(x + dx, foot_y + 1)).count() >= 7;
            if !platform { return false; }
            // Full body footprint clearance, matching the tightened movement collision.
            if !(foot_y - HEAD_H + 1 ..= foot_y).all(|y| {
                let y = y.max(0);
                !self.is_solid(x_l, y) && !self.is_solid(x, y) && !self.is_solid(x_r, y)
            }) { return false; }
            // Roofed: a solid ceiling somewhere above the head within CEIL_MAX.
            if !((foot_y - CEIL_MAX).max(0) ..= foot_y - HEAD_H).any(|y| self.is_solid(x, y)) {
                return false;
            }
            // Never spawn in a sealed pocket: there must be a way out within walking
            // distance — a nearby floor at a similar height that opens to the sky
            // (unroofed), reachable along the cave/tunnel.
            self.cave_has_escape(x, foot_y)
        };
        // Bottom-up: prefer the deepest (main chamber) floor over thin upper tunnels.
        (HEAD_H..WATER_Y as i32).rev().find(|&foot_y| ok(foot_y))
    }

    /// True if a soldier standing at `(x, foot_y)` can walk/fall/jump (via a
    /// flood-fill over nearby standable floors) to some floor that is open to
    /// the sky (not roofed within `OPEN_CLEAR`) — i.e. the cave/tunnel
    /// containing `(x, foot_y)` actually connects to a way out, rather than
    /// just having one somewhere nearby with walls in between.
    fn cave_has_escape(&self, x: i32, foot_y: i32) -> bool {
        use crate::renderer::draw_sprites::SOLDIER_HALF_W;
        use std::collections::{HashSet, VecDeque};

        const STEP:       i32 = 8;   // grid step for the flood fill (px)
        const HEAD_H:     i32 = 26;  // body height above the foot
        const MAX_STEP:   i32 = 8;   // walk step-up/down allowance
        const JUMP_H:     i32 = 48;  // max jump height
        const FALL_MAX:   i32 = 120; // max fall the search will follow in one hop
        const OPEN_CLEAR: i32 = 220; // clear space above a floor = "open to sky"
        const MAX_VISITED: usize = 300;

        let half = SOLDIER_HALF_W as i32;
        let clear_col = |cx: i32, y0: i32, y1: i32| -> bool {
            let (lo, hi) = (y0.min(y1), y0.max(y1));
            (lo..=hi).all(|y| !self.is_solid(cx, y.max(0)))
        };
        let body_clear = |cx: i32, fy: i32| -> bool {
            (fy - HEAD_H + 1..=fy).all(|y| {
                let y = y.max(0);
                !self.is_solid(cx - half, y) && !self.is_solid(cx, y) && !self.is_solid(cx + half, y)
            })
        };
        let is_floor = |cx: i32, fy: i32| -> bool {
            cx >= 0 && cx < WORLD_W as i32
                && fy >= HEAD_H && fy < WATER_Y as i32
                && self.is_solid(cx, fy + 1) && !self.is_solid(cx, fy)
                && body_clear(cx, fy)
        };
        let is_open = |cx: i32, fy: i32| -> bool {
            !((fy - OPEN_CLEAR).max(0)..=fy - HEAD_H).any(|y| self.is_solid(cx, y))
        };

        let mut visited: HashSet<(i32, i32)> = HashSet::new();
        let mut queue: VecDeque<(i32, i32)> = VecDeque::new();
        visited.insert((x, foot_y));
        queue.push_back((x, foot_y));

        while let Some((cx, fy)) = queue.pop_front() {
            if is_open(cx, fy) { return true; }
            if visited.len() >= MAX_VISITED { break; }
            for &ncx in &[cx - STEP, cx + STEP, cx] {
                if ncx < 0 || ncx >= WORLD_W as i32 { continue; }
                for nfy in ((fy - JUMP_H)..=(fy + FALL_MAX)).step_by(STEP as usize) {
                    if !is_floor(ncx, nfy) || !visited.insert((ncx, nfy)) { continue; }
                    let reachable = if nfy <= fy + MAX_STEP {
                        clear_col(ncx, nfy - HEAD_H + 1, fy)
                    } else {
                        clear_col(ncx, fy + 1, nfy)
                    };
                    if reachable {
                        queue.push_back((ncx, nfy));
                    }
                }
            }
        }
        false
    }

    /// Generate a 256×256 tiling dirt texture using layered Perlin noise.
    /// Returns a flat Vec of [B,G,R,A] pixels, row-major.
    /// Sample at: pixel[(world_y & 255) * 256 + (world_x & 255)]
    pub fn generate_dirt_texture(seed: u64) -> Vec<[u8; 4]> {
        let p_large  = Perlin::new(seed.wrapping_add(11) as u32);
        let p_medium = Perlin::new(seed.wrapping_add(22) as u32);
        let p_fine   = Perlin::new(seed.wrapping_add(33) as u32);

        // Dirt palette in [R, G, B, A] order — warm chocolate browns, tight range for uniform look
        let palette: [[u8; 4]; 7] = [
            [68,  40, 16, 255], // darkest — deep pockets
            [84,  52, 20, 255], // dark
            [98,  62, 24, 255], // main dirt (dark)
            [112, 72, 28, 255], // main dirt (mid) — most common
            [126, 82, 33, 255], // main dirt (light)
            [142, 94, 40, 255], // lighter patch
            [158, 108, 48, 255], // highlight — shallowest
        ];

        let mut pixels = Vec::with_capacity(256 * 256);
        for ty in 0u32..256 {
            for tx in 0u32..256 {
                let x = tx as f64;
                let y = ty as f64;
                let large  = p_large .get([x * 0.020, y * 0.020]) * 0.55;
                let medium = p_medium.get([x * 0.065, y * 0.065]) * 0.30;
                let fine   = p_fine  .get([x * 0.190, y * 0.190]) * 0.15;
                let v = ((large + medium + fine + 1.0) * 0.5).clamp(0.0, 1.0);
                let idx = (v * 6.0).round() as usize;
                pixels.push(palette[idx.min(6)]);
            }
        }
        pixels
    }

    /// Return the surface Y at column x — the topmost solid pixel.
    /// Returns None if the column is entirely air (shouldn't happen
    /// after from_heightmap, but safe to handle).
    pub fn surface_y_at(&self, x: u32) -> Option<u32> {
        if x >= WORLD_W { return None; }
        for y in 0..WATER_Y {
            if self.is_solid(x as i32, y as i32) {
                return Some(y);
            }
        }
        None
    }
}

#[cfg(test)]
mod step4_tests {
    use super::*;

    fn make_terrain(seed: u64) -> (Terrain, Heightmap) {
        let hm = Heightmap::generate(seed);
        let t = Terrain::from_heightmap(&hm);
        (t, hm)
    }

    #[test]
    fn surface_pixel_is_solid() {
        let (t, hm) = make_terrain(42);
        for x in 0..WORLD_W {
            let sy = hm.surface_at(x) as i32;
            assert!(
                t.is_solid(x as i32, sy),
                "x={x} surface_y={sy} should be solid"
            );
        }
    }

    #[test]
    fn pixel_above_surface_is_air() {
        let (t, hm) = make_terrain(42);
        for x in (0..WORLD_W).step_by(10) {
            let sy = hm.surface_at(x) as i32;
            if sy > 0 {
                assert!(
                    !t.is_solid(x as i32, sy - 1),
                    "x={x} y={} should be air", sy - 1
                );
            }
        }
    }

    #[test]
    fn pixels_below_surface_are_solid_down_to_water() {
        let (t, hm) = make_terrain(7);
        for x in (0..WORLD_W).step_by(50) {
            let sy = hm.surface_at(x) as i32;
            for y in sy..WATER_Y as i32 {
                assert!(
                    t.is_solid(x as i32, y),
                    "x={x} y={y} should be solid (below surface)"
                );
            }
        }
    }

    #[test]
    fn water_rows_are_not_solid() {
        let (t, _) = make_terrain(1);
        for x in 0..WORLD_W {
            for y in WATER_Y..WORLD_H {
                assert!(
                    !t.is_solid(x as i32, y as i32),
                    "x={x} y={y} is in water zone and must not be solid"
                );
            }
        }
    }

    #[test]
    fn surface_y_at_matches_heightmap() {
        let (t, hm) = make_terrain(99);
        for x in (0..WORLD_W).step_by(20) {
            let hm_y = hm.surface_at(x);
            let t_y  = t.surface_y_at(x).expect("column should have a surface");
            assert_eq!(
                hm_y, t_y,
                "x={x}: heightmap says {hm_y}, terrain says {t_y}"
            );
        }
    }

    #[test]
    fn surface_y_at_out_of_bounds_returns_none() {
        let (t, _) = make_terrain(1);
        assert!(t.surface_y_at(WORLD_W).is_none());
        assert!(t.surface_y_at(WORLD_W + 999).is_none());
    }

    #[test]
    fn solid_count_is_plausible() {
        let (t, hm) = make_terrain(42);
        // Rough expected: sum of (WATER_Y - surface_y) across all columns
        let expected: u32 = (0..WORLD_W)
            .map(|x| WATER_Y - hm.surface_at(x))
            .sum();
        assert_eq!(t.solid_count() as u32, expected);
    }

    #[test]
    fn same_seed_produces_same_bitmap() {
        let (a, _) = make_terrain(12345);
        let (b, _) = make_terrain(12345);
        assert_eq!(a.solid_count(), b.solid_count());
        // Spot check a few hundred pixels
        for x in (0..WORLD_W).step_by(16) {
            for y in (0..WORLD_H).step_by(16) {
                assert_eq!(
                    a.is_solid(x as i32, y as i32),
                    b.is_solid(x as i32, y as i32),
                    "x={x} y={y} differs between identical seeds"
                );
            }
        }
    }
}

#[cfg(test)]
mod spawn_tests {
    use super::*;

    /// Every map (all archetypes across seeds 0..30) must give each team 4 spawns
    /// inside the interior band, never within SPAWN_EDGE_MARGIN of an edge, and on
    /// real footing (solid pixel directly below the foot).
    #[test]
    fn spawns_are_interior_and_standable() {
        for seed in 0..30u64 {
            let mut t = Terrain::generate_tactical(seed);
            let team0 = t.find_team_spawns(0, WORLD_W / 2 - 40, 4);
            let team1 = t.find_team_spawns(WORLD_W / 2 + 40, WORLD_W, 4);
            assert_eq!(team0.len(), 4, "seed {seed} team0 spawn count");
            assert_eq!(team1.len(), 4, "seed {seed} team1 spawn count");

            for sp in team0.iter().chain(team1.iter()) {
                let x = sp.x as i32;
                let y = sp.y as i32;
                assert!(
                    x >= SPAWN_EDGE_MARGIN as i32
                        && x <= (WORLD_W - SPAWN_EDGE_MARGIN) as i32,
                    "seed {seed}: spawn x={x} not in interior band"
                );
                assert!(
                    t.is_solid(x, y + 1),
                    "seed {seed}: spawn ({x},{y}) has no ground below"
                );
                assert!(!t.is_solid(x, y), "seed {seed}: spawn ({x},{y}) is inside terrain");
            }
        }
    }

    /// Caverns maps must actually place some soldiers underground: across seeds that
    /// generate the caverns archetype, at least one team should get a spawn whose foot
    /// is roofed — there is solid terrain overhead within the cave ceiling range.
    #[test]
    fn caverns_spawn_some_soldiers_underground() {
        // A spawn is underground if it stands on solid ground but has a solid ceiling
        // overhead (vs the open surface, which has clear sky all the way up).
        let roofed = |t: &Terrain, x: i32, y: i32| -> bool {
            t.is_solid(x, y + 1)
                && !t.is_solid(x, y)
                && ((y - 220).max(0)..=(y - 26).max(0)).any(|cy| t.is_solid(x, cy))
        };
        let mut saw_caverns = false;
        let mut saw_underground = false;
        for seed in 0..200u64 {
            let mut t = Terrain::generate_tactical(seed);
            if t.archetype != 3 { continue; }
            saw_caverns = true;
            let spawns = t.find_team_spawns(0, WORLD_W / 2 - 40, 4);
            if spawns.iter().any(|sp| roofed(&t, sp.x as i32, sp.y as i32)) {
                saw_underground = true;
            }
        }
        assert!(saw_caverns, "no caverns archetype generated in seeds 0..200");
        assert!(saw_underground, "caverns maps never placed a soldier underground");
    }

    /// Determinism guard: spawns must match for identical seeds (client/server sync).
    #[test]
    fn spawns_are_deterministic() {
        let mut a = Terrain::generate_tactical(777);
        let mut b = Terrain::generate_tactical(777);
        assert_eq!(
            a.find_team_spawns(0, WORLD_W / 2 - 40, 4),
            b.find_team_spawns(0, WORLD_W / 2 - 40, 4)
        );
    }
}
