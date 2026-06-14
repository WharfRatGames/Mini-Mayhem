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
    /// Index (0–23) into the terrain texture atlas, chosen per map from the seed.
    /// Renderer samples this tile to texture the solid silhouette.
    pub surface_texture: u8,
    /// Which landform style this map was generated as.
    /// 0=hills 1=cliffs/overhangs 2=floating islands 3=caverns 4=canyon/mesa.
    /// Drives spawn placement (caverns put some soldiers underground).
    pub archetype: u8,
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
            surface_texture: 0,
            archetype: 0,
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
            for y in surface_y..WATER_Y {
                terrain.set_solid(x as i32, y as i32, true);
            }
        }
        terrain
    }

    /// Generate Worms-style terrain using full 2D Perlin noise.
    ///
    /// Each pixel is independently solid/air based on layered noise + a depth
    /// bias that keeps sky at top and ground at bottom. Caves and overhangs
    /// emerge naturally from the 2D nature of the noise.
    pub fn generate_worms(seed: u64) -> Self {
        let mut terrain = Self::empty();
        let p_large  = Perlin::new(seed as u32);
        let p_medium = Perlin::new(seed.wrapping_add(1337) as u32);
        let p_detail = Perlin::new(seed.wrapping_add(2674) as u32);
        let p_cave   = Perlin::new(seed.wrapping_add(9999) as u32);

        let w = WORLD_W as f64;
        let h = WORLD_H as f64;
        // Y where depth_bias = 0 (50/50 solid/air by noise alone)
        let mid_y = TERRAIN_MIN_Y as f64 + (TERRAIN_MAX_Y - TERRAIN_MIN_Y) as f64 * 0.45;
        // Half-range: at mid ± scale we're fully air or fully solid
        let scale  = (TERRAIN_MAX_Y - TERRAIN_MIN_Y) as f64 * 0.55;

        for y in TERRAIN_MIN_Y..WATER_Y {
            for x in 0..WORLD_W {
                let nx = x as f64 / w;
                let ny = y as f64 / h;

                // Layered 2D noise: large shapes + medium caves + surface roughness
                let large  = p_large .get([nx * 2.8,  ny * 2.2 ]) * 0.55;
                let medium = p_medium.get([nx * 7.5,  ny * 6.0 ]) * 0.30;
                let detail = p_detail.get([nx * 18.0, ny * 14.0]) * 0.10;
                let noise  = large + medium + detail;

                // Depth bias — creates sky/ground separation
                let depth  = ((y as f64) - mid_y) / scale;

                let solid = noise + depth > 0.0;

                if solid {
                    // Optional cave punch: carve holes inside solid ground
                    // Caves only form when well below surface (not near the top)
                    let cave = p_cave.get([nx * 6.0, ny * 5.0]);
                    let above_mid = ((y as f64) - mid_y) / scale; // >0 = below mid
                    let cave_solid = cave < 0.45 || above_mid < 0.15;
                    terrain.set_solid(x as i32, y as i32, cave_solid);
                }
            }
        }

        // Guarantee: every column has solid ground somewhere in the lower half
        // (prevents floating-only islands from breaking spawning)
        let floor_y = mid_y as u32 + (scale * 0.7) as u32;
        for x in 0..WORLD_W {
            if terrain.surface_y_at(x).is_none() {
                // Column is all air — fill a strip at the floor line
                for y in floor_y..(floor_y + 20).min(WATER_Y) {
                    terrain.set_solid(x as i32, y as i32, true);
                }
            }
        }

        terrain.texture = Some(Self::generate_dirt_texture(seed));
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
        let octaves = 4usize;
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
        // near 0, so keep this SMALL — 0.12 ≈ thin tunnels; 0.30+ obliterates terrain.
        let mut cave_thresh = 0.12;
        let cave_sx = 7.0;
        let cave_sy = 6.0;
        let mut blob = false;
        let mut big_void = false;
        let mut overhang = false;       // cliffs: stamp cantilevered ceiling shelves
        let mut surface_caves = false;  // caverns: let cave punch break the top crust
        let mut void_shafts = 0usize;   // caverns: vertical entrance shafts into the void

        match archetype {
            0 => { // Rolling hills
                fade = rnd(&mut rng, 0.40, 0.10);
                threshold = rnd(&mut rng, 0.49, 0.04);
                scale_x = rnd(&mut rng, 2.5, 1.5);
                contrast = 1.25;
                cliff_bias = rnd(&mut rng, -0.10, 0.20);
                cave = lcg(&mut rng) % 100 < 25;
            }
            1 => { // Cliffs & overhangs: ridged faces + cantilevered ceiling shelves
                fade = rnd(&mut rng, 0.30, 0.10);
                // Higher threshold so the ridged noise doesn't saturate the surface
                // into a flat solid top — lets valleys/notches cut in for a rolling look.
                threshold = rnd(&mut rng, 0.52, 0.06);
                // Higher horizontal frequency → frequent ridges/notches across the top
                // (breaks the broad flat-mesa silhouette into a craggy rolling skyline).
                scale_x = rnd(&mut rng, 5.0, 3.0);
                scale_y = 2.2;
                ridged = true;
                contrast = 1.2;
                let dir = if lcg(&mut rng) & 1 == 0 { 1.0 } else { -1.0 };
                cliff_bias = dir * rnd(&mut rng, 0.18, 0.22);
                // Strong domain warp gives genuinely craggy, leaning faces.
                warp_amp = rnd(&mut rng, 0.28, 0.14); // 0.28–0.42
                overhang = true;
                cave = lcg(&mut rng) % 100 < 35;
            }
            2 => { // Floating islands (radial blob masks)
                fade = 0.0;
                threshold = rnd(&mut rng, 0.28, 0.05);
                scale_x = rnd(&mut rng, 1.8, 1.0);
                scale_y = 1.5;
                contrast = 1.4;
                warp_amp = 0.16;
                blob = true;
            }
            3 => { // Caverns: mostly solid with hollow chamber + cave punch + entrances
                fade = rnd(&mut rng, 0.68, 0.10);
                threshold = rnd(&mut rng, 0.36, 0.07);
                scale_x = rnd(&mut rng, 2.0, 1.5);
                contrast = 1.5;
                warp_amp = 0.08;
                big_void = true;
                cave = true;
                cave_thresh = 0.12;                 // light tunnels; void is the main chamber
                surface_caves = true;               // caves reach daylight
                void_shafts = 2 + (lcg(&mut rng) % 3) as usize; // 2–4 entrance shafts
            }
            _ => { // Canyon / mesa: terraced + vertical trenches
                fade = rnd(&mut rng, 0.46, 0.10);
                threshold = rnd(&mut rng, 0.49, 0.04);
                scale_x = rnd(&mut rng, 2.5, 1.5);
                contrast = 1.25;
                let dir = if lcg(&mut rng) & 1 == 0 { 1.0 } else { -1.0 };
                cliff_bias = dir * rnd(&mut rng, 0.12, 0.16);
                terrace = Some(rnd(&mut rng, 4.0, 3.0).round());
                terrace_mix = 0.30;
                warp_amp = 0.08;
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
        const HILL_AMP: f64 = 0.24;                // strong relief: frequent jump-height ledges
        const SKY_BAND: f64 = 0.30;                // top ~135px+ tapers off → guaranteed headroom (all archetypes)

        // ── Phase 2: Density field ────────────────────────────────────────────────
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

                // 4. Density: blob mask for islands, else gradient + directional cliff bias
                let mut density = if blob {
                    let b: f64 = island_blobs.iter()
                        .map(|(cx, cy, r)| {
                            let dx = nx - cx;
                            let dy = ny - cy;
                            (1.0 - ((dx * dx + dy * dy).sqrt() / r).min(1.0)).max(0.0)
                        })
                        .fold(0.0f64, f64::max);
                    b * (noise + 0.25) // 0 outside blobs; noise-textured inside
                } else {
                    let mut d = fade * ty + (1.0 - fade) * noise + (nx - 0.5) * cliff_bias;
                    if rolling {
                        // Three octaves of elevation: broad hills + medium bumps + fine
                        // steps. The medium/fine octaves make the surface rise and fall
                        // fast enough (>4px/px) that walking can't auto-climb it — you
                        // must jump/backflip — so there are no long flat walkable stretches.
                        let relief = hill.get([nx * hill_freq,       0.7])
                                   + 0.6  * hill.get([nx * hill_freq * 4.0, 3.1])
                                   + 0.35 * hill.get([nx * hill_freq * 9.0, 9.4]);
                        d += (relief / 1.95) * HILL_AMP;
                    }
                    d
                };

                // 4b. Top sky-margin (ALL archetypes): erode density near the top so
                // terrain tapers off below the ceiling instead of clamping flat against
                // TERRAIN_MIN_Y. Guarantees headroom and kills flat top-edge plateaus;
                // also pulls floating islands down off the very top of the screen.
                if ty < SKY_BAND {
                    let t = ty / SKY_BAND;
                    let smooth_t = t * t * (3.0 - 2.0 * t);
                    density -= (1.0 - smooth_t) * 0.85;
                }

                // 5. Terracing — stepped cliffs / mesa shelves
                if let Some(levels) = terrace {
                    let q = (density * levels).floor() / levels;
                    density = density * (1.0 - terrace_mix) + q * terrace_mix;
                }

                // 6. Edge erosion for water on ends
                if water_end_px > 0.0 {
                    let edge_dist = (x as f64).min(WORLD_W as f64 - 1.0 - x as f64);
                    if edge_dist < water_end_px {
                        let t = edge_dist / water_end_px;
                        let smooth_t = t * t * (3.0 - 2.0 * t);
                        density -= (1.0 - smooth_t) * 0.55;
                    }
                }

                terrain.set_solid(x as i32, y as i32, density >= threshold);
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
            2 + (lcg(&mut rng) % 3) as usize // 2–4
        } else { 0 };
        // Central zone is kept narrower (0.37–0.63) so each team's chasm-free home
        // landform is wide enough to spread 4 soldiers at the 140px safe spacing.
        let zone_lo = (0.37 * WORLD_W as f64) as i32; // ~710: left edge of central zone
        let zone_hi = (0.63 * WORLD_W as f64) as i32; // ~1210: right edge of central zone
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
            carve_chasm(&mut terrain, cx, half_w, TERRAIN_MIN_Y as i32, bottom_y, drift);
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
            terrain.sky_limit[x] = topmost.unwrap_or(WATER_Y);
        }

        // Phase 7 (per-column spawn mounds) intentionally removed: spawns are now
        // chosen after generation by `find_team_spawns`, which lands soldiers on the
        // real terrain (island tops, cliff ledges, cavern floors) without re-grounding
        // the map. This is what lets islands/caverns/overhangs survive to the screen.

        terrain.texture = Some(Self::generate_dirt_texture(seed));
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

        // On caverns maps, place half the team inside the cave system; the rest land
        // on the surface below. If a half lacks cave footing, the surface scan + island
        // fallback below still fill every slot, so a team always gets `count` soldiers.
        let n_cave = if self.archetype == 3 { count / 2 } else { 0 };
        if n_cave > 0 {
            let mut x = lo;
            while x <= hi && spawns.len() < n_cave {
                if used_x.iter().all(|&ux| (ux - x).abs() >= MIN_SEP) {
                    if let Some(foot_y) = self.standable_cave_foot_y(x) {
                        spawns.push(WorldPos::new(x as f32, foot_y as f32));
                        used_x.push(x);
                    }
                }
                x += 4;
            }
        }

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
            if spawns.len() >= count { break; }
            let x0 = seg[0].0;
            let x1 = seg.last().unwrap().0;
            let remaining = count - spawns.len();
            // How many fit on this top at the safe spacing, capped to what's still needed.
            let cap = ((x1 - x0) / MIN_SEP + 1).clamp(1, remaining as i32);
            let gap = ((x1 - x0) as f32 / (cap - 1).max(1) as f32).max(MIN_SEP as f32);
            for i in 0..cap {
                if spawns.len() >= count { break; }
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
                // This column was previously empty (spawn_y/sky_limit pointed past
                // WATER_Y, computed before mounds were raised) — without updating
                // them, the renderer's sky-aware viewport copy would skip this whole
                // new mound, leaving it invisible (soldier floating "above terrain").
                let top_u = top as u32;
                if top_u < self.sky_limit[cx as usize] { self.sky_limit[cx as usize] = top_u; }
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
        const CLEAR_H: i32 = 24; // soldier body + clearance
        const SKY_H:   i32 = 100;
        if x < 0 || x >= WORLD_W as i32 { return None; }
        let ok = |foot_y: i32| -> bool {
            // Body must fit in-world; high islands are fine (their open sky is
            // verified by the all-air scan below, not by a hard Y floor).
            if foot_y < CLEAR_H || foot_y >= WATER_Y as i32 { return false; }
            if !self.is_solid(x, foot_y + 1) { return false; }
            let platform = (-4..=4).filter(|&dx| self.is_solid(x + dx, foot_y + 1)).count() >= 7;
            if !platform { return false; }
            (foot_y - CLEAR_H - SKY_H + 1 ..= foot_y).all(|y| !self.is_solid(x, y.max(0)))
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
    pub fn standable_cave_foot_y(&self, x: i32) -> Option<i32> {
        const HEAD_H: i32 = 26;   // body + small clearance above the foot
        const CEIL_MAX: i32 = 220; // a ceiling must sit within this height to count as a cave
        if x < 0 || x >= WORLD_W as i32 { return None; }
        let ok = |foot_y: i32| -> bool {
            if foot_y < HEAD_H || foot_y >= WATER_Y as i32 { return false; }
            if !self.is_solid(x, foot_y + 1) { return false; }
            let platform = (-4..=4).filter(|&dx| self.is_solid(x + dx, foot_y + 1)).count() >= 7;
            if !platform { return false; }
            // Body clearance: open air directly above the feet.
            if !(foot_y - HEAD_H + 1 ..= foot_y).all(|y| !self.is_solid(x, y.max(0))) { return false; }
            // Roofed: a solid ceiling somewhere above the head within CEIL_MAX.
            ((foot_y - CEIL_MAX).max(0) ..= foot_y - HEAD_H).any(|y| self.is_solid(x, y))
        };
        // Bottom-up: prefer the deepest (main chamber) floor over thin upper tunnels.
        (HEAD_H..WATER_Y as i32).rev().find(|&foot_y| ok(foot_y))
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
