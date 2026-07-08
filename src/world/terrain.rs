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
    /// The dominant WA source mask of this map's collage (see
    /// wa_templates::collage_params / dominant_template_id). Drives cosmetic
    /// dispatch via `Theme::of` (sky/debris/dirt tint/scenery theme) so the
    /// visual theme follows the underlying real map art.
    pub template_id: u8,
    /// True for the occasional carved-cavern map (~20% of seeds): solid rock
    /// with chambers carved by the inverted WA collage. Drives spawn
    /// placement (caverns put some soldiers underground) and cosmetic
    /// dispatch.
    pub is_cavern: bool,
    /// Decorative scenery objects placed seed-deterministically on the terrain surface.
    /// Solid: stamped into the object mask each tick (see `stamp_objects`), so
    /// soldiers can stand on them and projectiles collide with them.
    pub scenery: Vec<SceneryObject>,
}

/// Cosmetic biome theme for a map (sky gradient, debris, dirt tint, scenery
/// set). Derived from `is_cavern` + `template_id` (the dominant WA source
/// mask of the collage) — single source of truth for every dispatch site so
/// scenery sprite counts, renderers, etc. can never disagree.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    Underground,
    Pastoral,
    Rugged,
}

impl Theme {
    pub fn of(is_cavern: bool, template_id: u8) -> Theme {
        if is_cavern {
            Theme::Underground
        } else if template_id % 2 == 0 {
            Theme::Pastoral
        } else {
            Theme::Rugged
        }
    }
}

/// A single decorative scenery object placed on the terrain surface.
/// `x`/`y` are world-space pixel coordinates of the bottom-center of the sprite.
/// `sprite` is the variant index within the map's scenery theme (see scenery.rs).
#[derive(Clone)]
pub struct SceneryObject {
    pub x: u32,
    pub y: u32,
    pub sprite: u8,
    /// Per-pixel destruction state over the collision footprint box, in
    /// world-pixel (post-scale) resolution: row-major over
    /// `(y-height..=y) x (x-half_w..=x+half_w)`, true = still intact.
    /// `None` means fully intact — the common case, so untouched objects
    /// don't pay for an allocation. Craters clear bits within their radius
    /// exactly like they clear terrain pixels (see `carve`), so an explosion
    /// only eats the part of the object it actually overlaps.
    pub mask: Option<Vec<bool>>,
}

impl SceneryObject {
    /// Integer draw/collision scale for this sprite: small props get 3×, tall
    /// ones 2×, so everything lands in a similar on-screen size band. Shared by
    /// the renderer (scenery.rs) and `footprint` — MUST stay deterministic and
    /// identical on client and server.
    pub fn scale(&self, theme: Theme) -> i32 {
        let (_, h) = self.base_footprint(theme);
        if h <= 18 { 3 } else { 2 }
    }

    /// Collision footprint as (half_width, height) in pixels — already scaled
    /// by `scale()`: the solid box is `x-half_w ..= x+half_w`, `y-height ..= y`.
    /// Approximates the drawn sprite (see renderer/scenery.rs); thin decorations
    /// (flames, thorns, ropes) are deliberately excluded so soldiers stand on the
    /// visual bulk of the object. Used both for placement clearance at generation
    /// time and per-tick object stamping — MUST stay deterministic and identical
    /// on client and server.
    pub fn footprint(&self, theme: Theme) -> (i32, i32) {
        let s = self.scale(theme);
        let (hw, h) = self.base_footprint(theme);
        (hw * s, h * s)
    }

    /// Unscaled (1×) sprite footprint, matching the raw pixel-art dimensions
    /// in renderer/scenery.rs.
    /// True if the pixel at world (wx, wy) is still intact (not yet carved
    /// away by a crater). Pixels outside the footprint box (thin decorative
    /// overflow like torch flames, which the box deliberately excludes) are
    /// always intact — only the tracked collision box is ever masked.
    pub fn pixel_intact(&self, wx: i32, wy: i32, theme: Theme) -> bool {
        let (half_w, height) = self.footprint(theme);
        let lx = wx - self.x as i32 + half_w;
        let ly = self.y as i32 - wy;
        if lx < 0 || ly < 0 || lx > 2 * half_w || ly > height {
            return true;
        }
        match &self.mask {
            None => true,
            Some(m) => {
                let w = 2 * half_w + 1;
                m.get((ly * w + lx) as usize).copied().unwrap_or(true)
            }
        }
    }

    /// Clear mask bits within a crater (ccx, ccy, sqrt(r2)) that fall inside
    /// this object's footprint box — same per-pixel rule `Crater::carve` uses
    /// on terrain. Returns true once every tracked pixel is destroyed (caller
    /// should then drop the object).
    pub fn carve(&mut self, theme: Theme, ccx: f32, ccy: f32, r2: f32) -> bool {
        let (half_w, height) = self.footprint(theme);
        let w = 2 * half_w + 1;
        let mask = self.mask.get_or_insert_with(|| vec![true; (w * (height + 1)) as usize]);
        let mut any_left = false;
        for ly in 0..=height {
            let wy = self.y as i32 - ly;
            for lx in 0..w {
                let idx = (ly * w + lx) as usize;
                if !mask[idx] { continue; }
                let wx = self.x as i32 - half_w + lx;
                let (dx, dy) = (wx as f32 - ccx, wy as f32 - ccy);
                if dx * dx + dy * dy <= r2 {
                    mask[idx] = false;
                } else {
                    any_left = true;
                }
            }
        }
        !any_left
    }

    fn base_footprint(&self, theme: Theme) -> (i32, i32) {
        match theme {
            // Round/organic sprites (rocks, bushes, piles) get a tighter box than
            // their full visual spread — a rectangle around an irregular sprite
            // always overhangs the corners, so these are trimmed toward the solid
            // core instead of the decorative edges. Blocky sprites (posts, crates,
            // walls, logs) already match their visual bounds closely and are left
            // as-is. A fully precise fix needs a real per-pixel mask per sprite
            // (SceneryObject::mask is currently only populated by crater carving,
            // never at spawn) — this is a pragmatic tightening, not that.
            Theme::Pastoral => match self.sprite {
                0 => (6, 24),  // flower
                1 => (10, 16), // mushroom
                2 => (9, 8),   // mossy rock (was 12,10)
                3 => (14, 25), // fence post + rails
                4 => (10, 14), // bush (was 13,18)
                5 => (6, 32),  // sunflower
                6 => (18, 10), // log
                _ => (9, 7),   // pebble cluster (was 12,9)
            },
            Theme::Rugged => match self.sprite {
                0 => (9, 38),  // pine tree canopy (was 11,40)
                1 => (12, 11), // boulder (was 15,20; 16 scaled to 48px was just over the
                               // ~46px backflip apex — too tall to climb onto)
                2 => (5, 9),   // wooden crate (half size)
                3 => (12, 14), // dead stump
                4 => (18, 22), // broken wall
                5 => (11, 12), // lichen rock (was 14,15)
                _ => (8, 18),  // cairn (was 10,21)
            },
            Theme::Underground => match self.sprite {
                0 => (11, 30), // crystal cluster (was 14,32)
                1 => (12, 11), // bone pile (was 16,14)
                2 => (3, 22),  // torch (handle+coal; flame not solid)
                3 => (7, 12),  // skull (was 9,14)
                4 => (20, 9),  // fallen stalactite shard
                5 => (10, 16), // rusted chain pile
                _ => (9, 15),  // ribcage (was 12,18)
            },
        }
    }
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
            template_id: 0,
            is_cavern: false,
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

    /// Topmost solid y of the contiguous run containing (x, y), from the
    /// per-column run cache — O(#runs) instead of a pixel-by-pixel upward scan.
    /// Returns None if (x, y) isn't covered by the cache (out of bounds, below
    /// WATER_Y, or a stale cache after a direct `set_solid`).
    pub fn run_top(&self, x: i32, y: i32) -> Option<u32> {
        if x < 0 || x >= WORLD_W as i32 || y < 0 || y >= WATER_Y as i32 {
            return None;
        }
        let y = y as u32;
        if self.solid_to_water[x as usize] {
            let top = self.sky_limit[x as usize];
            return if y >= top { Some(top) } else { None };
        }
        self.solid_runs[x as usize].iter()
            .find(|&&(s, e)| y >= s && y < e)
            .map(|&(s, _)| s)
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

    /// Multi-pass tactical terrain: every seed composes a novel silhouette as a
    /// collage of real Worms Armageddon terrain art (wa_templates::collage_density
    /// — segments spliced/warped/crossfaded per seed). Cavern maps (~20% of seeds)
    /// carve chambers from the same collage, inverted. Phases 6 & 7 (smoothing +
    /// spawn guarantee) are always applied.
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

        // ── Phase 0: Feature selection (all seed-driven) ──────────────────────────
        // Occasional cavern map (~20% odds), carved from the same WA source
        // art (inverted) instead of the old independent fill+carve generator.
        let is_cavern = lcg(&mut rng) % 5 == 0;
        terrain.is_cavern = is_cavern;
        // Seed-based collage over the real WA masks: every seed composes a
        // novel silhouette from segments of the source art (see wa_templates).
        // Uses its own private RNG stream — draw order here is unaffected.
        let cparams = super::wa_templates::collage_params(seed, is_cavern);
        // Cosmetic theme follows the collage's dominant source mask.
        terrain.template_id = super::wa_templates::dominant_template_id(&cparams);
        // Surface texture: seed-driven raw selector; the renderer maps it modulo
        // the pooled atlas tile count (client/server agree from the same seed).
        terrain.surface_texture = lcg(&mut rng) as u8;

        // Feature parameters — seed-random, no longer gated by a landform style.
        // The macro silhouette itself comes from the real WA masks (wa_density,
        // Phase 2 below); these only shape the residual fine noise texture/lean
        // folded in at reduced weight, plus is_cavern's own fill+carve thresholds.
        let octaves = 3usize;
        let warp_freq = 2.5;
        let warp_amp = rnd(&mut rng, 0.14, 0.16);
        let threshold = rnd(&mut rng, 0.46, 0.08);
        let scale_x = rnd(&mut rng, 3.0, 2.5);
        let scale_y = 2.2;
        let ridged = lcg(&mut rng) & 1 == 0;
        let contrast = 1.3;
        let cliff_bias = {
            let dir = if lcg(&mut rng) & 1 == 0 { 1.0f64 } else { -1.0 };
            dir * rnd(&mut rng, 0.10, 0.14)
        };
        let hill_amp: f64 = rnd(&mut rng, 0.10, 0.14);
        // |normalized noise| < cave_thresh carves air (is_cavern's fill+carve pass).
        // OpenSimplex output clusters near 0, so keep this SMALL — 0.16 ≈ WA-style
        // caves; 0.30+ obliterates terrain.
        let cave_thresh = 0.16;
        let cave_sx = 5.5;
        let cave_sy = 5.0;
        // Cave-punch tunnels / cantilevered overhang shelves: seed-random on any
        // non-cavern map instead of being gated by a landform style.
        let cave = !is_cavern && lcg(&mut rng) % 100 < 45;
        let overhang = !is_cavern && lcg(&mut rng) % 100 < 25;
        // Sky-clearance (erode the top ~14% of the terrain band) and its paired
        // chasm-top offset must move together — seed-random on non-cavern maps.
        let sky_clear = !is_cavern && lcg(&mut rng) % 100 < 50;

        // Water margins always applied — wide enough to be visible on both sides
        let water_end_px: f64 = 180.0 + (lcg(&mut rng) & 0xFF) as f64 / 255.0 * 170.0; // 180–350px

        // Macro shaping (rolling hills + top headroom). Applied to every non-cavern
        // map (caverns keep their original surface shape; only their spawns change).
        // Disabled: layering this low-weight sine-like relief onto every non-cavern
        // map produced a repeating "series of mounds" look, especially on flatter
        // WA collage silhouettes. The collage itself is already the macro shape
        // (see Phase 2 below); no additional hill relief is folded in now.
        let rolling = false;
        let hill_freq = rnd(&mut rng, 2.8, 1.8);   // 2.8–4.6 cycles: several hills per map (visible on-screen)
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

        // ── Phase 2: Density field ────────────────────────────────────────────────
        if is_cavern {
            // Cavern maps: solid rock band with chambers carved by the same WA
            // collage that shapes island maps — inverted, so real WA land
            // silhouettes become the air chambers (authentic WA edge character
            // on every cave wall). Enclosed: sealed rock cap above, solid sides.

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

            // B — WA-collage density over the rock band [SKY_FLOOR, CAVE_FLOOR):
            // solid-ness comes from the inverted WA collage (real WA land shapes
            // become carved chambers), plus a low-weight noise octave for per-seed
            // edge texture, a floor bias so chambers keep a solid base above the
            // water gap, and a hard side seal. Same box-blur+threshold treatment
            // as the island branch so cave walls come out rounded/organic.
            // Deterministic f64 math, no RNG draws inside the loop.
            let band_h = (CAVE_FLOOR - SKY_FLOOR) as usize;
            let band_w = WORLD_W as usize;
            let band_span = band_h as f64;
            let mut cdens = vec![1.0f64; band_w * band_h];
            const SIDE_SEAL: i32 = 12; // solid side walls (enclosed map)
            // Parallel fill — pure per-pixel math over disjoint row chunks;
            // bit-identical to the serial loop (see island branch note).
            let n_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
            let rows_per = band_h.div_ceil(n_threads);
            std::thread::scope(|scope| {
                for (chunk_i, chunk) in cdens.chunks_mut(rows_per * band_w).enumerate() {
                    let y_start = SKY_FLOOR + (chunk_i * rows_per) as i32;
                    let cave_a = &cave_a; let cparams = &cparams;
                    scope.spawn(move || {
            for (row_i, row) in chunk.chunks_mut(band_w).enumerate() {
                let y = y_start + row_i as i32;
                let ny = y as f64 / WORLD_H as f64;
                let ty = (y - SKY_FLOOR) as f64 / band_span; // 0 ceiling → 1 floor
                for x in 0..WORLD_W as i32 {
                    let nx = x as f64 / WORLD_W as f64;
                    let mut d = super::wa_templates::collage_density(cparams, nx, ty)
                        + cave_a.get([nx * 6.0, ny * 6.0]) * 0.10;
                    // Floor bias: guarantee a solid base in the bottom ~12% of the
                    // band so inverted-mask holes can't open straight into water.
                    if ty > 0.88 {
                        let t = (ty - 0.88) / 0.12;
                        d += t * t * (3.0 - 2.0 * t) * 1.2;
                    }
                    // Ceiling bias: keep the top of the band attached to the cap.
                    if ty < 0.04 {
                        d += (1.0 - ty / 0.04) * 1.2;
                    }
                    if x < SIDE_SEAL || x >= WORLD_W as i32 - SIDE_SEAL {
                        d = 2.0;
                    }
                    row[x as usize] = d;
                }
            }
                    });
                }
            });
            // Separable box blur (r=4), padding with SOLID (1.0) outside the band
            // — the cap above and base below are rock, unlike the island branch's
            // air padding.
            let rb: i32 = 4;
            let mut ctmp = vec![0.0f64; band_w * band_h];
            // Parallel over disjoint row chunks — bit-identical to serial.
            std::thread::scope(|scope| {
                for (chunk_i, chunk) in ctmp.chunks_mut(rows_per * band_w).enumerate() {
                    let cdens = &cdens;
                    scope.spawn(move || {
                        for (row_i, out_row) in chunk.chunks_mut(band_w).enumerate() {
                            let row = (chunk_i * rows_per + row_i) * band_w;
                            for rx in 0..band_w as i32 {
                                let mut sum = 0.0;
                                let mut cnt = 0.0;
                                for dx in -rb..=rb {
                                    let xx = (rx + dx).clamp(0, band_w as i32 - 1) as usize;
                                    sum += cdens[row + xx];
                                    cnt += 1.0;
                                }
                                out_row[rx as usize] = sum / cnt;
                            }
                        }
                    });
                }
            });
            std::thread::scope(|scope| {
                for (chunk_i, chunk) in cdens.chunks_mut(rows_per * band_w).enumerate() {
                    let ctmp = &ctmp;
                    scope.spawn(move || {
                        for (row_i, out_row) in chunk.chunks_mut(band_w).enumerate() {
                            let ry = (chunk_i * rows_per + row_i) as i32;
                            for rx in 0..band_w {
                                let mut sum = 0.0;
                                let mut cnt = 0.0;
                                for dy in -rb..=rb {
                                    let yy = ry + dy;
                                    let v = if yy < 0 || yy >= band_h as i32 {
                                        1.0
                                    } else {
                                        ctmp[yy as usize * band_w + rx]
                                    };
                                    sum += v;
                                    cnt += 1.0;
                                }
                                out_row[rx] = sum / cnt;
                            }
                        }
                    });
                }
            });
            // Threshold into the bitmap: carve air where the smoothed field is
            // below 0.5 (the inverted collage marks chambers as low density).
            for ry in 0..band_h {
                let y = SKY_FLOOR + ry as i32;
                for x in 0..band_w {
                    if cdens[ry * band_w + x] < 0.5 {
                        terrain.set_solid(x as i32, y, false);
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

            // (No side water erosion: WA caverns are enclosed — the collage
            // pass's SIDE_SEAL keeps solid walls at both world edges.)

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

        if !is_cavern {
        // Continuous density buffer for the terrain region [TERRAIN_MIN_Y, WATER_Y).
        // We fill this per-pixel below, then box-blur it before thresholding so the
        // silhouette comes out smooth/organic instead of following every noise wiggle.
        // 0.0 doubles as the AIR pad value for out-of-region neighbours (it sits well
        // below `threshold` ≈ 0.5). Deterministic f64 math, no RNG draws → identical
        // on client and server for a given seed.
        let region_w = WORLD_W as usize;
        let region_h = (WATER_Y - TERRAIN_MIN_Y) as usize;
        let mut dens = vec![0.0f64; region_w * region_h];

        // Parallel fill: rows are independent and the per-pixel math is a pure
        // function of (x, y), so splitting the buffer into row chunks produces
        // bit-identical output to the serial loop on every machine/thread count.
        let n_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        let rows_per = region_h.div_ceil(n_threads);
        std::thread::scope(|scope| {
            for (chunk_i, chunk) in dens.chunks_mut(rows_per * region_w).enumerate() {
                let y_start = TERRAIN_MIN_Y as usize + chunk_i * rows_per;
                let base = &base; let warp_a = &warp_a; let warp_b = &warp_b;
                let cparams = &cparams; let hill_col = &hill_col;
                scope.spawn(move || {
        for (row_i, row) in chunk.chunks_mut(region_w).enumerate() {
            let y = y_start + row_i;
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

                // 4. Density: a per-seed collage of real Worms Armageddon terrain
                // silhouettes (see wa_templates) so every seed's macro shape is a
                // novel WA-styled composition. The residual noise/lean gradient is
                // folded in at reduced weight — it contributes fine edge texture
                // but no longer defines the silhouette.
                let mut density = super::wa_templates::collage_density(&cparams, nx, ty)
                    + (noise - 0.5) * 0.15
                    + (nx - 0.5) * cliff_bias * 0.3;
                if rolling {
                    density += hill_col[x] * 0.3;
                }

                // 4a. Depth ramp: solidness grows with depth so every column has
                // connected ground near GROUND_T, high floating collage chunks
                // melt into air, and the total surface-height band collapses to
                // roughly terrain_range / DEPTH_RAMP px. This is what keeps
                // valley soldiers able to walk/backflip to any top — without it
                // the raw collage yields marooned pillars and floating islands.
                // GROUND_T raised and DEPTH_RAMP relaxed to widen the surface band
                // (~84px → ~190px) for taller, more vertical maps — the grapple now
                // reaches isolated tops, so a gentler ramp is acceptable.
                const GROUND_T: f64 = 0.66;
                const DEPTH_RAMP: f64 = 4.0;
                density += (ty - GROUND_T) * DEPTH_RAMP;

                // 4b. Top sky-margin: erode density near the top so terrain tapers
                // off below the ceiling instead of clamping flat against
                // TERRAIN_MIN_Y. Guarantees headroom and kills flat top-edge plateaus;
                // also pulls floating islands down off the very top of the screen.
                if ty < SKY_BAND {
                    let t = ty / SKY_BAND;
                    let smooth_t = t * t * (3.0 - 2.0 * t);
                    density -= (1.0 - smooth_t) * 0.85;
                }

                // 5. Edge erosion for water on ends
                if water_end_px > 0.0 {
                    let edge_dist = (x as f64).min(WORLD_W as f64 - 1.0 - x as f64);
                    if edge_dist < water_end_px {
                        let t = edge_dist / water_end_px;
                        let smooth_t = t * t * (3.0 - 2.0 * t);
                        density -= (1.0 - smooth_t) * 0.55;
                    }
                }

                row[x] = density;
            }
        }
                });
            }
        });

        // ── Phase 2c: Separable box blur of the density field, then threshold ─────
        // Rounds the contour where the field crosses `threshold` (metaball-style),
        // killing sub-~14px jaggedness while leaving the ≥104px relief that forces
        // jump/backflip intact. Blurring the CONTINUOUS field (not the binary mask)
        // keeps thin bridges / small stepping-stone islands that sit above threshold
        // solid — only their edges round — instead of eroding them away.
        // A gentle radius keeps small WA-silhouette stepping-stones rounded without
        // eroding them away.
        let r: i32 = 4;
        let mut tmp = vec![0.0f64; region_w * region_h];
        // Both passes parallelized over disjoint row chunks — pure reads of the
        // other buffer, so output is bit-identical to the serial loops.
        // Horizontal pass: clamp x at the region edges (terrain continues sideways).
        std::thread::scope(|scope| {
            for (chunk_i, chunk) in tmp.chunks_mut(rows_per * region_w).enumerate() {
                let dens = &dens;
                scope.spawn(move || {
                    for (row_i, out_row) in chunk.chunks_mut(region_w).enumerate() {
                        let row = (chunk_i * rows_per + row_i) * region_w;
                        for rx in 0..region_w as i32 {
                            let mut sum = 0.0;
                            let mut cnt = 0.0;
                            for dx in -r..=r {
                                let xx = (rx + dx).clamp(0, region_w as i32 - 1) as usize;
                                sum += dens[row + xx];
                                cnt += 1.0;
                            }
                            out_row[rx as usize] = sum / cnt;
                        }
                    }
                });
            }
        });
        // Vertical pass: rows outside [0, region_h) read as AIR (0.0) so the top
        // tapers to sky and the bottom to water rather than smearing solid.
        std::thread::scope(|scope| {
            for (chunk_i, chunk) in dens.chunks_mut(rows_per * region_w).enumerate() {
                let tmp = &tmp;
                scope.spawn(move || {
                    for (row_i, out_row) in chunk.chunks_mut(region_w).enumerate() {
                        let ry = (chunk_i * rows_per + row_i) as i32;
                        for rx in 0..region_w {
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
                            out_row[rx] = sum / cnt;
                        }
                    }
                });
            }
        });
        // Threshold the smoothed field into the solid bitmap. Dropped slightly below
        // `threshold`: the blur pulls small WA-silhouette stepping-stones' contours
        // inward, so this keeps them above threshold (and above the min_frag=50
        // cleanup floor below) instead of vanishing.
        let thr = threshold - 0.03;
        for ry in 0..region_h {
            let y = TERRAIN_MIN_Y as usize + ry;
            for x in 0..region_w {
                terrain.set_solid(x as i32, y as i32, dens[ry * region_w + x] >= thr);
            }
        }
        } // end if !is_cavern

        // ── Phase 2a: Sky clearance (seed-random) ─────────────────────────────────
        // Keep the top portion of the terrain zone clear on some maps, scaled with
        // world height; other maps use the upper screen area (overhangs, floating
        // WA silhouette chunks). Caverns are exempt — sealed rock cap, no sky.
        if sky_clear {
            let sky_floor_clear: u32 = TERRAIN_MIN_Y + (terrain_range_f * 0.14) as u32;
            for y in TERRAIN_MIN_Y..sky_floor_clear.min(WATER_Y) {
                for x in 0..WORLD_W as i32 {
                    terrain.set_solid(x, y as i32, false);
                }
            }
        }

        // ── Phase 2b: Overhang shelves (seed-random) ──────────────────────────────
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
                let gap   = rnd(&mut rng, 30.0, 30.0) as i32;  // 30–60px air gap — taller overhang shelves (grapple reaches the higher ones)
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
        // Carve air tunnels where two-layer cave noise lands in a band. Keep
        // CRUST_PX of rock below each column's ACTUAL surface. The old fixed-band
        // crust (ty <= 0.18) assumed terrain reached the top of the band; with the
        // depth-ramped/compressed relief all terrain sits below that line, so a
        // fixed crust protects nothing and the punch hollows out the ground right
        // under soldiers' feet.
        const CRUST_PX: u32 = 44;
        let carve_floor: Vec<u32> = if cave {
            (0..WORLD_W)
                .map(|x| {
                    let top = (TERRAIN_MIN_Y..WATER_Y)
                        .find(|&y| terrain.is_solid(x as i32, y as i32))
                        .unwrap_or(WATER_Y);
                    (top + CRUST_PX).min(WATER_Y)
                })
                .collect()
        } else {
            Vec::new()
        };
        if cave {
            for y in TERRAIN_MIN_Y..WATER_Y {
                let ny = y as f64 / WORLD_H as f64;
                for x in 0..WORLD_W {
                    if y <= carve_floor[x as usize] { continue; }
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
        // Same 2-pass Moore dilation as the is_cavern fill+carve path, clamped to
        // the same per-column carve floor as the punch so surface terrain (and the
        // crust under it) is never eroded.
        if cave && !is_cavern {
            for _ in 0..2 {
                let snap = terrain.solid.clone();
                for y in TERRAIN_MIN_Y as i32..WATER_Y as i32 - 1 {
                    for x in 1..WORLD_W as i32 - 1 {
                        if y <= carve_floor[x as usize] as i32 { continue; }
                        if !snap[world_index(x as u32, y as u32)] { continue; }
                        let has_air_neighbor = (-1i32..=1).any(|dy| {
                            let yy = y + dy;
                            if yy <= carve_floor[x as usize] as i32 || yy >= WATER_Y as i32 { return false; }
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

        // ── Phase 5: Chasms ────────────────────────────────────────────────────────
        // Vertical slots that make crossing the map a skill challenge: some cut to the
        // water (a mis-judged jump drowns), some are floored pits; widths span jumpable
        // (≤56px) to grapple-only (80–160px). Confined to the CENTRAL contested zone so
        // each team keeps a large chasm-free home landform of comparable size on its own
        // side — neither team is stranded on smaller bits than the other. The bumpy
        // 3-octave relief still makes the home sides a ledge-hopping challenge.
        let n_chasms = if !is_cavern && lcg(&mut rng) % 100 < 45 {
            2 + (lcg(&mut rng) % 3) as usize // 2–4
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
            let chasm_top = if sky_clear {
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
        // A low bar keeps the WA silhouettes' small floating stepping-stone chunks
        // alive; the isolated-pixel pass above already removes single-pixel noise.
        // Caverns are solid rock carved into large connected chambers, so they can
        // afford a higher bar to clean up carve-noise debris.
        {
            let min_frag: usize = if is_cavern { 200 } else { 50 };
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
            // Sprite-variant count must match the theme scenery.rs::draw_scenery
            // will pick for this map (same Theme::of dispatch): draw_underground /
            // draw_pastoral / draw_rugged — 7 / 8 / 7 sprites respectively.
            let count: u8 = match Theme::of(terrain.is_cavern, terrain.template_id) {
                Theme::Underground => 7,
                Theme::Pastoral => 8,
                Theme::Rugged => 7,
            };
            let mut srng = seed ^ 0xDECA_FBAB_E000_1234u64;
            let margin = (WORLD_W as f64 * 0.05) as u32;
            let usable_w = WORLD_W - 2 * margin;
            const NUM_OBJECTS: u32 = 28;
            const MIN_SPACING: u32 = 110;
            let mut placed: Vec<SceneryObject> = Vec::with_capacity(NUM_OBJECTS as usize);
            for _ in 0..NUM_OBJECTS * 14 {
                srng = srng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let col = margin + (srng >> 33) as u32 % usable_w;
                // Cavern maps: the "surface" is the sealed rock cap, so scenery
                // (crystals, bones, torches...) goes on standable cave floors.
                let surface_y = if terrain.is_cavern {
                    match terrain.standable_cave_foot_simple(col as i32) {
                        Some(y) => y as u32,
                        None => continue,
                    }
                } else {
                    // Skip columns with no real ground (sky_limit == WATER_Y means bare water column)
                    if terrain.sky_limit[col as usize] >= WATER_Y { continue; }
                    terrain.spawn_y[col as usize]
                };
                if surface_y >= WATER_Y { continue; }
                // Enforce minimum horizontal spacing
                if placed.iter().any(|o| o.x.abs_diff(col) < MIN_SPACING) { continue; }
                srng = srng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let sprite = (srng >> 33) as u8 % count;
                let obj = SceneryObject { x: col, y: surface_y, sprite, mask: None };
                let (half_w, height) = obj.footprint(Theme::of(terrain.is_cavern, terrain.template_id));
                let base = surface_y as i32;
                // Objects sit ON the terrain surface, never inside it — but real
                // ground is bumpy, so the bottom EMBED_TOL rows may overlap a
                // slope (the object sinks in a little, which reads as natural).
                // Above that, the central ¾ of the footprint box must be air:
                // rejects spots against hillsides/cliffs and under low overhangs
                // while tolerating a slope grazing the outer edge.
                const EMBED_TOL: i32 = 5;
                let cw = (half_w * 3 / 4).max(1);
                let clear = (EMBED_TOL + 1..=height).all(|dy| {
                    (-cw..=cw).all(|dx| !terrain.is_solid(col as i32 + dx, base - dy))
                });
                if !clear { continue; }
                // Grounded: the central half of the footprint must have solid
                // ground within a few px below the base row — no floating props
                // on ledge lips or chasm edges. (Outer edges may overhang a
                // slope slightly; the visual bulk stays connected to ground.)
                let grounded = (-half_w / 2..=half_w / 2).all(|dx| {
                    (1..=EMBED_TOL).any(|dy| terrain.is_solid(col as i32 + dx, base + dy))
                });
                if !grounded { continue; }
                placed.push(obj);
                if placed.len() == NUM_OBJECTS as usize { break; }
            }
            terrain.scenery = placed;
        }

        terrain
    }

    /// Pick deterministic spawn positions for a team, scanning the real
    /// post-generation terrain. Pure function of the terrain (identical on client
    /// and server) — it never mutates the map. Candidates are restricted to the
    /// interior `[x_lo, x_hi]` band (callers pass left/right halves) and never
    /// within `SPAWN_EDGE_MARGIN` of a world edge. Returns up to `count`
    /// well-separated standable spots, actively spread across the map's vertical
    /// range; on very sparse terrain separation constraints relax rather than
    /// stamping artificial platforms.
    pub fn find_team_spawns(&mut self, x_lo: u32, x_hi: u32, count: usize) -> Vec<WorldPos> {
        // Loosened from an earlier 140/120: dynamic analysis of real WA (Deathmatch
        // under Wine) showed it does NOT enforce meaningful separation between a
        // team's worms — e.g. "Prince Charles"/"Prince Andrew" spawned ~20-30px
        // apart on the same ledge. We still keep a modest floor (unlike WA's fully
        // loose placement) so a single centred blast can't reliably gut two
        // teammates at once, but allow the tighter, more WA-like clustering that
        // 140/120 always ruled out.
        const MIN_SEP:   i32 = 70;  // horizontal spacing between a team's soldiers
        const MIN_SEP_V: i32 = 60;  // vertical spacing that also counts as "separated"
        let lo = x_lo.max(SPAWN_EDGE_MARGIN) as i32;
        let hi = (x_hi.min(WORLD_W - SPAWN_EDGE_MARGIN) as i32).max(lo + 1);

        let mut spawns: Vec<WorldPos> = Vec::with_capacity(count);
        let mut used: Vec<(i32, i32)> = Vec::with_capacity(count);
        // Two spots are separated if far apart horizontally OR vertically.
        let sep_ok = |used: &[(i32, i32)], cx: i32, cy: i32, sh: i32, sv: i32|
            used.iter().all(|&(ux, uy)| (ux - cx).abs() >= sh || (uy - cy).abs() >= sv);

        // Scenery objects are solid (stamped into the object mask each tick) —
        // never seat a soldier overlapping one, AND never seat one in a gap too
        // narrow to actually stand/move in between two nearby objects. Soldier
        // is ~14px wide; a margin of just footprint+10 only prevented direct
        // overlap, so two objects placed ~20-30px apart could still leave a
        // technically-"clear" sliver between them that satisfied each object's
        // individual check while being far too cramped to play in. Widening the
        // per-object margin to footprint+24 (~SOLDIER_W + 10px of breathing
        // room) makes each object's exclusion zone wide enough that two objects
        // closer than ~48px together have their zones merge and swallow the
        // whole gap — so no candidate column exists there at all, same
        // mechanism, just enough margin to also rule out cramped in-between spots.
        let theme = Theme::of(self.is_cavern, self.template_id);
        let scenery_boxes: Vec<(i32, i32)> = self.scenery.iter()
            .map(|o| (o.x as i32, o.footprint(theme).0 + 24))
            .collect();
        let clear_of_scenery = |x: i32| scenery_boxes.iter().all(|&(ox, hw)| (x - ox).abs() > hw);

        // Cave maps (WA style): all soldiers spawn underground. No surface layer exists.
        if self.is_cavern {
            let mut cave_cands: Vec<(i32, i32)> = Vec::new();
            let mut x = lo;
            while x <= hi {
                if let Some(fy) = self.standable_cave_foot_y(x) {
                    if clear_of_scenery(x) { cave_cands.push((x, fy)); }
                }
                x += 1;
            }
            // Greedily disperse: each pick maximizes the worst-case separation (as
            // a fraction of MIN_SEP/MIN_SEP_V, matching sep_ok's OR rule) to
            // whoever's already placed — same idea as the "last resort" dispersion
            // pass further down. A strict single-pass sep_ok scan used to silently
            // skip a soldier outright the moment the chamber was too small to fit
            // everyone with full separation — and the surface-oriented fallbacks
            // below can't rescue cavern maps (no surface/sky-clearance exists there),
            // so the skipped soldiers ended up landing on top of each other via the
            // last-resort's fixed-midpoint default. Maximizing spread within the
            // real cave candidate pool instead means a cramped chamber still spreads
            // its team as far apart as the chamber allows, never duplicates a spot.
            while spawns.len() < count && !cave_cands.is_empty() {
                let pick = cave_cands.iter()
                    .filter(|&&(cx, cy)| used.iter().all(|&(ux, uy)| ux != cx || uy != cy))
                    .max_by_key(|&&(cx, cy)| {
                        used.iter().map(|&(ux, uy)| {
                            let dx = (ux - cx).abs() * 1000 / MIN_SEP;
                            let dy = (uy - cy).abs() * 1000 / MIN_SEP_V;
                            dx.max(dy)
                        }).min().unwrap_or(i32::MAX)
                    })
                    .copied();
                match pick {
                    Some((cx, cy)) => { spawns.push(WorldPos::new(cx as f32, cy as f32)); used.push((cx, cy)); }
                    None => break,
                }
            }
            if spawns.len() >= count { return spawns; }
            // Fall through to generic surface spawning if still short.
        }

        // Pre-scan for underground cave floors so we can reserve slots for them.
        // Non-cavern maps with punched caves should seat some soldiers underground
        // for vertical variety; cap the surface pass so those slots stay open.
        let cave_quota = if !self.is_cavern {
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

        // Every standable level per column enters the pool — the top of a hill AND
        // the valley floor / ledge beneath an overhang — so lower ground competes.
        let mut cands: Vec<(i32, i32)> = Vec::new(); // (x, foot_y): x asc, then y asc
        let mut x = lo;
        while x <= hi {
            for fy in self.standable_foot_levels(x) {
                // Must stand on a solid mass, not a thin slab/ledge.
                if (1..=GROUND_DEPTH).all(|d| self.is_solid(x, fy + d)) && clear_of_scenery(x) {
                    cands.push((x, fy));
                }
            }
            x += 4;
        }
        // Segment candidates into landform tops. Multiple y-levels can coexist over
        // the same x-range, so run an open-segment sweep instead of a single chain:
        // a candidate extends the open segment whose last point is nearest in y
        // (within STEP_TOL, not already extended at this column), else starts a new
        // one. Purely index-ordered — deterministic on client and server.
        let mut open: Vec<(Vec<(i32, i32)>, bool)> = Vec::new(); // (points, extended-at-this-x)
        let mut segments: Vec<Vec<(i32, i32)>> = Vec::new();
        let mut last_x = i32::MIN;
        for &(cx, cy) in &cands {
            if cx != last_x {
                let mut i = 0;
                while i < open.len() {
                    if cx - open[i].0.last().unwrap().0 > GAP_TOL {
                        segments.push(open.remove(i).0);
                    } else {
                        open[i].1 = false;
                        i += 1;
                    }
                }
                last_x = cx;
            }
            let best = (0..open.len())
                .filter(|&i| !open[i].1)
                .map(|i| (i, (open[i].0.last().unwrap().1 - cy).abs()))
                .filter(|&(_, dy)| dy <= STEP_TOL)
                .min_by_key(|&(i, dy)| (dy, i));
            match best {
                Some((i, _)) => { open[i].0.push((cx, cy)); open[i].1 = true; }
                None => open.push((vec![(cx, cy)], true)),
            }
        }
        segments.extend(open.into_iter().map(|(s, _)| s));
        // Keep tops wide enough to not be pillars.
        let seg_w = |s: &Vec<(i32, i32)>| s.last().unwrap().0 - s[0].0;
        let wide: Vec<&Vec<(i32, i32)>> =
            segments.iter().filter(|s| seg_w(s) >= MIN_LAND_W).collect();
        // Integer mean height per landform, for the vertical-dispersion score.
        let seg_y: Vec<i32> = wide.iter()
            .map(|s| s.iter().map(|&(_, y)| y).sum::<i32>() / s.len() as i32)
            .collect();
        // ── Greedy vertically-dispersed selection ────────────────────────────────
        // First pick: the widest landform. Every later pick maximizes the minimum
        // vertical distance to the soldiers already placed, with (capped) width as
        // a secondary term — so the team spreads DOWN the map instead of stacking
        // on the highest tops. 100px of new vertical ground outweighs 400px of
        // extra width. Integer math only.
        let mut seg_used: Vec<bool> = vec![false; wide.len()];
        while spawns.len() < surface_cap {
            let pick = (0..wide.len()).filter(|&i| !seg_used[i]).max_by_key(|&i| {
                let vdist = used.iter()
                    .map(|&(_, uy)| (uy - seg_y[i]).abs())
                    .min().unwrap_or(10_000);
                (vdist.min(10_000) * 4 + seg_w(wide[i]).min(600),
                 -wide[i][0].0,      // tie: leftmost
                 -(i as i32))        // tie: lowest index
            });
            let Some(si) = pick else { break };
            seg_used[si] = true;
            let seg = wide[si];

            // Spread soldiers EVENLY across the landform, filling its whole width
            // instead of bunching them at one end. Never closer than MIN_SEP
            // horizontally unless MIN_SEP_V apart vertically (one blast can't catch two).
            let x0 = seg[0].0;
            let x1 = seg.last().unwrap().0;
            let remaining = surface_cap - spawns.len();
            let cap = ((x1 - x0) / MIN_SEP + 1).clamp(1, remaining as i32);
            let gap = ((x1 - x0) as f32 / (cap - 1).max(1) as f32).max(MIN_SEP as f32);
            for i in 0..cap {
                if spawns.len() >= surface_cap { break; }
                let target = x0 + (gap * i as f32) as i32;
                // Snap the evenly-spaced target to the nearest standable column that
                // is still separated from everyone already placed.
                if let Some(&(cx, cy)) = seg.iter()
                    .filter(|&&(px, py)| sep_ok(&used, px, py, MIN_SEP, MIN_SEP_V))
                    .min_by_key(|&&(px, _)| (px - target).abs())
                {
                    spawns.push(WorldPos::new(cx as f32, cy as f32));
                    used.push((cx, cy));
                }
            }
        }

        // Cave-floor spawns: for non-cavern maps with punched caves, actively mix
        // underground positions in to spread soldiers vertically. We reserved some
        // slots from the surface pass (capped above); fill them here.
        if !self.is_cavern && spawns.len() < count {
            let mut cx = lo + 60;
            while cx <= hi - 60 && spawns.len() < count {
                if let Some(fy) = self.standable_cave_foot_simple(cx) {
                    if sep_ok(&used, cx, fy, MIN_SEP, MIN_SEP_V) && clear_of_scenery(cx) {
                        spawns.push(WorldPos::new(cx as f32, fy as f32));
                        used.push((cx, fy));
                    }
                }
                cx += 80;
            }
        }

        // Last resort (very fragmented/sparse half): the natural landforms couldn't
        // seat the whole team. NEVER mutate the terrain (no artificial mounds or
        // platforms) — instead greedily disperse across the same candidate pool,
        // same idea as the main wide-landform pass: each pick maximizes the worst-
        // case separation (as a fraction of MIN_SEP/MIN_SEP_V, matching sep_ok's
        // OR rule) to whoever's already placed. A first-fit left-to-right scan here
        // would clump the whole team on the first usable cluster of columns on a
        // badly fragmented map (e.g. a seed with zero landforms >=60px wide) even
        // though a lone usable column exists far away — greedy dispersion picks
        // that far column instead. Integer math only (no floats — must stay
        // identical across x86/ARM).
        if spawns.len() < count {
            while spawns.len() < count {
                let pick = cands.iter()
                    .filter(|&&(cx, cy)| used.iter().all(|&(ux, uy)| ux != cx || uy != cy))
                    .max_by_key(|&&(cx, cy)| {
                        used.iter().map(|&(ux, uy)| {
                            let dx = (ux - cx).abs() * 1000 / MIN_SEP;
                            let dy = (uy - cy).abs() * 1000 / MIN_SEP_V;
                            dx.max(dy)
                        }).min().unwrap_or(i32::MAX)
                    })
                    .copied();
                match pick {
                    Some((cx, cy)) => { spawns.push(WorldPos::new(cx as f32, cy as f32)); used.push((cx, cy)); }
                    None => break,
                }
            }
        }
        while spawns.len() < count {
            // Step 3 (pathological — the strict candidate pool ran dry): from an
            // evenly spaced base column, search outward for ANY standable level
            // (relaxed headroom, no ground-depth/scenery checks), else the raw
            // surface (foot on the topmost solid pixel), else — only if the whole
            // band is empty air — mid-air over mid-terrain (the soldier falls).
            let i = spawns.len() as i32;
            let base = (lo + (hi - lo) * (2 * i + 1) / (2 * count as i32)).clamp(lo, hi);
            let mut spot: Option<(i32, i32)> = None;
            'search: for d in 0..=(hi - lo) {
                for px in [base + d, base - d] {
                    if px < lo || px > hi { continue; }
                    for fy in self.standable_foot_levels(px) {
                        if used.iter().all(|&(ux, uy)| ux != px || uy != fy) {
                            spot = Some((px, fy));
                            break 'search;
                        }
                    }
                }
            }
            if spot.is_none() {
                'surf: for d in 0..=(hi - lo) {
                    for px in [base + d, base - d] {
                        if px < lo || px > hi { continue; }
                        if let Some(sy) = self.surface_y_at(px as u32) {
                            let fy = sy as i32 - 1;
                            if fy > 0 && used.iter().all(|&(ux, uy)| ux != px || uy != fy) {
                                spot = Some((px, fy));
                                break 'surf;
                            }
                        }
                    }
                }
            }
            let mut fallback = spot.unwrap_or((base, (TERRAIN_MIN_Y as i32 + TERRAIN_MAX_Y as i32) / 2));
            // Guarantee distinctness even in this fully-pathological branch: integer
            // division can map two different spawn indices to the same `base` column
            // on a narrow/fragmented band, and if both inner searches above also come
            // up empty, they'd otherwise collide on the exact same point. Nudge
            // vertically until clear of every already-placed spawn — exact placement
            // doesn't matter here (no standable terrain was found anyway), only that
            // no two soldiers ever land on the same pixel.
            while used.iter().any(|&(ux, uy)| ux == fallback.0 && uy == fallback.1) {
                fallback.1 += 24;
            }
            let (px, py) = fallback;
            spawns.push(WorldPos::new(px as f32, py as f32));
            used.push((px, py));
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
        // Scan top-down from the very top so we can land on high sky-islands (whose
        // tops sit above CLEAR_H+SKY_H) before any ground far below.
        (CLEAR_H..WATER_Y as i32).find(|&foot_y| self.foot_ok(x, foot_y, SKY_H))
    }

    /// Standing check shared by `standable_foot_y` / `standable_foot_levels`:
    /// solid platform under the feet, body footprint clear for `CLEAR_H + sky_h`
    /// px above (matching movement collision), and a walkable escape column on at
    /// least one side over the same height.
    fn foot_ok(&self, x: i32, foot_y: i32, sky_h: i32) -> bool {
        use crate::renderer::draw_sprites::SOLDIER_HALF_W;
        const CLEAR_H: i32 = 24; // soldier body + clearance
        let x_l = x - SOLDIER_HALF_W as i32;
        let x_r = x + SOLDIER_HALF_W as i32;
        // Body must fit in-world; high islands are fine (their open sky is
        // verified by the all-air scan below, not by a hard Y floor).
        if foot_y < CLEAR_H || foot_y >= WATER_Y as i32 { return false; }
        if !self.is_solid(x, foot_y + 1) { return false; }
        let platform = (-4..=4).filter(|&dx| self.is_solid(x + dx, foot_y + 1)).count() >= 7;
        if !platform { return false; }
        // Full body footprint must be clear, matching the tightened movement
        // collision (try_move_horizontal / airborne terrain_hit), so a soldier
        // never spawns wedged in a passage it can't legally move out of.
        let body_clear = (foot_y - CLEAR_H - sky_h + 1 ..= foot_y).all(|y| {
            let y = y.max(0);
            !self.is_solid(x_l, y) && !self.is_solid(x, y) && !self.is_solid(x_r, y)
        });
        if !body_clear { return false; }
        // Escape room: the exact footprint can be clear yet still be exactly
        // SOLDIER_W wide with solid walls flush against both edges — a soldier
        // there could stand but never take a single step (try_move_horizontal
        // requires the column just beyond the footprint edge to be open too).
        // Require at least one direction to have room to walk out.
        let clear_col = |cx: i32| (foot_y - CLEAR_H - sky_h + 1 ..= foot_y)
            .all(|y| !self.is_solid(cx, y.max(0)));
        clear_col(x_l - 2) || clear_col(x_r + 2)
    }

    /// ALL standable foot Ys at column `x`, top→bottom (≤ 3). The topmost level
    /// keeps the full 100px open-sky rule (same spot `standable_foot_y` returns);
    /// lower levels — ledges and floors under overhangs — only need 16px of
    /// clearance above the body, so terrain lower down the map actually enters
    /// the spawn candidate pool instead of being shadowed by whatever sits above.
    pub fn standable_foot_levels(&self, x: i32) -> Vec<i32> {
        const CLEAR_H:     i32 = 24;
        const SKY_RELAXED: i32 = 16; // body 24 + 16 = 40px total headroom
        const MAX_LEVELS: usize = 3;
        if x < 0 || x >= WORLD_W as i32 { return Vec::new(); }
        let mut out = Vec::new();
        let mut fy = CLEAR_H;
        while fy < WATER_Y as i32 && out.len() < MAX_LEVELS {
            let sky = if out.is_empty() { 100 } else { SKY_RELAXED };
            if self.foot_ok(x, fy, sky) {
                out.push(fy);
                fy += CLEAR_H + 8; // skip past this body before looking lower
            } else {
                fy += 1;
            }
        }
        out
    }

    /// Highest *enclosed* foot Y at column `x`: a cave/void floor a soldier can stand
    /// on. Like `standable_foot_y` but the spot is roofed — there must be a solid
    /// ceiling overhead (so it is genuinely underground, not the open surface).
    /// Scans bottom-up so soldiers land on the main void floor, not a shallow tunnel.
    /// None if no roofed standing spot exists.
    /// Like standable_cave_foot_y but skips the escape-connectivity check.
    /// Used for is_cavern spawns where vertical shafts guarantee reachability.
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
            if !((foot_y - CEIL_MAX).max(0) ..= foot_y - HEAD_H).any(|y| self.is_solid(x, y)) {
                return false;
            }
            // Escape room: reject spots exactly SOLDIER_W wide with solid walls
            // flush against both footprint edges — standable but unwalkable.
            let clear_col = |cx: i32| (foot_y - HEAD_H + 1 ..= foot_y)
                .all(|y| !self.is_solid(cx, y.max(0)));
            clear_col(x_l - 2) || clear_col(x_r + 2)
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
            // Escape room: reject spots exactly SOLDIER_W wide with solid walls
            // flush against both footprint edges — standable but unwalkable.
            let clear_col = |cx: i32| (foot_y - HEAD_H + 1 ..= foot_y)
                .all(|y| !self.is_solid(cx, y.max(0)));
            if !(clear_col(x_l - 2) || clear_col(x_r + 2)) { return false; }
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

    /// Like `surface_y_at`, but accounts for scenery objects (trees, rocks,
    /// crystals, etc.) whose footprint spans column x — returns the topmost
    /// of the terrain surface and any overlapping scenery object's top edge,
    /// so barrel/mine spawn placement lands ON TOP of scenery instead of
    /// embedding inside its collision box. Deterministic — MUST stay
    /// identical on client and server (same inputs as `stamp_objects`).
    pub fn surface_y_at_with_scenery(&self, x: u32) -> Option<u32> {
        let ground_y = self.surface_y_at(x)?;
        let theme = Theme::of(self.is_cavern, self.template_id);
        let mut top_y = ground_y;
        for obj in &self.scenery {
            let (half_w, height) = obj.footprint(theme);
            let (ox, oy) = (obj.x as i32, obj.y as i32);
            if (x as i32 - ox).abs() <= half_w {
                let obj_top = (oy - height).max(0) as u32;
                if obj_top < top_y {
                    top_y = obj_top;
                }
            }
        }
        Some(top_y)
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
    /// roll is_cavern, at least one team should get a spawn whose foot is roofed —
    /// there is solid terrain overhead within the cave ceiling range.
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
            if !t.is_cavern { continue; }
            saw_caverns = true;
            let spawns = t.find_team_spawns(0, WORLD_W / 2 - 40, 4);
            if spawns.iter().any(|sp| roofed(&t, sp.x as i32, sp.y as i32)) {
                saw_underground = true;
            }
        }
        assert!(saw_caverns, "no is_cavern map generated in seeds 0..200");
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

    /// Spawns must spread down the map, not cluster on the highest landforms:
    /// on maps whose standable ground spans a real vertical range, the chosen
    /// spawn Ys must cover at least a quarter of that offered range, on at
    /// least 3/4 of such seeds (individual awkward maps are tolerated).
    #[test]
    fn spawns_disperse_vertically() {
        let mut exercised = 0u32;
        let mut passed = 0u32;
        for seed in 0..20u64 {
            let mut t = Terrain::generate_tactical(seed);
            if t.is_cavern { continue; } // cavern maps have their own placement
            let mut cys: Vec<i32> = Vec::new();
            let mut x = SPAWN_EDGE_MARGIN as i32;
            while x <= (WORLD_W - SPAWN_EDGE_MARGIN) as i32 {
                cys.extend(t.standable_foot_levels(x));
                x += 4;
            }
            let (Some(&cmin), Some(&cmax)) = (cys.iter().min(), cys.iter().max()) else { continue };
            let offered = cmax - cmin;
            if offered < 250 { continue; } // genuinely flat map — nothing to disperse over
            exercised += 1;
            let spawns = t.find_team_spawns(0, WORLD_W, 8);
            let ys: Vec<i32> = spawns.iter().map(|s| s.y as i32).collect();
            let spread = ys.iter().max().unwrap() - ys.iter().min().unwrap();
            if spread * 4 >= offered { passed += 1; }
        }
        assert!(exercised >= 5, "too few seeds offered vertical range ({exercised})");
        assert!(
            passed * 4 >= exercised * 3,
            "vertical dispersion too low: only {passed}/{exercised} seeds spread >= 25% of offered range"
        );
    }

    /// Manual eyeball helper: `cargo test --lib print_spawn_dist -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn print_spawn_dist() {
        for seed in 0..10u64 {
            let mut t = Terrain::generate_tactical(seed);
            let spawns = t.find_team_spawns(0, WORLD_W, 8);
            let mut ys: Vec<i32> = spawns.iter().map(|s| s.y as i32).collect();
            ys.sort();
            println!("seed {seed:2} cavern={} ys={:?}", t.is_cavern, ys);
        }
    }
}
