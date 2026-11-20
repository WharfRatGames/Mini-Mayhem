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
    /// WA landgen style (0..=3) the map was generated with (see `generate_tactical`;
    /// there are no Arty-specific map archetypes any more). Drives cosmetic dispatch
    /// via `Theme::of` (sky/debris/dirt tint/scenery theme).
    pub template_id: u8,
    /// True for a WA cavern-style map (seed-derived): solid rock with chambers
    /// carved by WA's own landgen. Drives spawn
    /// placement (caverns put some soldiers underground) and cosmetic
    /// dispatch.
    pub is_cavern: bool,
    /// Decorative scenery objects placed seed-deterministically on the terrain surface.
    /// Solid: stamped into the object mask each tick (see `stamp_objects`), so
    /// soldiers can stand on them and projectiles collide with them.
    pub scenery: Vec<SceneryObject>,
    /// WA 31-bit map seed this terrain was generated from (0 for legacy/empty);
    /// seeds WA's worm-placement stream.
    pub wa_seed: u32,
}

/// Cosmetic biome theme for a map (sky gradient, debris, dirt tint, scenery
/// set). Derived from `is_cavern` + `template_id` (the WA landgen style) — single source of truth for every dispatch site so
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
}

impl SceneryObject {
    /// Integer draw/collision scale for this sprite: small props get 3×, tall
    /// ones 2×, so everything lands in a similar on-screen size band. Shared by
    /// the renderer (scenery.rs) and `footprint` — MUST stay deterministic and
    /// identical on client and server.
    pub fn scale(&self, theme: Theme) -> i32 {
        // Bone pile (1) and ribcage (6): bones drawn near life-size read
        // wrong at 3× (a ribcage taller than a soldier) — cap them at 2×.
        if matches!(theme, Theme::Underground) && (self.sprite == 1 || self.sprite == 6) {
            return 2;
        }
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
    fn base_footprint(&self, theme: Theme) -> (i32, i32) {
        match theme {
            // Round/organic sprites (rocks, bushes, piles) get a tighter box than
            // their full visual spread — a rectangle around an irregular sprite
            // always overhangs the corners, so these are trimmed toward the solid
            // core instead of the decorative edges. Blocky sprites (posts, crates,
            // walls, logs) already match their visual bounds closely and are left
            // as-is. Only used for placement spacing/clearance now — the real
            // collision shape baked into terrain.solid is the sprite's exact
            // per-pixel silhouette (see `scenery_pixels`), not this box.
            Theme::Pastoral => match self.sprite {
                0 => (6, 24),  // flower
                1 => (10, 16), // mushroom
                2 => (9, 8),   // mossy rock (was 12,10)
                3 => (14, 25), // fence post + rails
                4 => (10, 14), // bush (was 13,18)
                5 => (6, 32),  // sunflower
                6 => (18, 10), // log
                7 => (9, 7),   // pebble cluster (was 12,9)
                8 => (8, 12),  // hay bale
                9 => (7, 24),  // scarecrow
                10 => (10, 20), // stone well
                11 => (10, 9),  // wheelbarrow
                12 => (6, 16),  // beehive on post
                13 => (5, 26),  // birdhouse on pole (roof peak excluded)
                14 => (7, 12),  // watering can (spout tip excluded)
                _ => (8, 9),   // pumpkin
            },
            Theme::Rugged => match self.sprite {
                0 => (9, 38),  // pine tree canopy (was 11,40)
                1 => (12, 11), // boulder (was 15,20; 16 scaled to 48px was just over the
                               // ~46px backflip apex — too tall to climb onto)
                2 => (5, 9),   // wooden crate (half size)
                3 => (12, 14), // dead stump
                4 => (18, 22), // broken wall
                5 => (11, 12), // lichen rock (was 14,15)
                6 => (8, 18),  // cairn (was 10,21)
                7 => (7, 26),  // menhir (standing stone)
                8 => (10, 20), // weathered signpost
                9 => (9, 8),   // campfire ring
                10 => (8, 16), // leaning cartwheel
                11 => (8, 7),  // ram skull
                12 => (9, 12), // anvil on stump (horn tip excluded)
                13 => (6, 28), // totem pole (wing boards excluded)
                _ => (12, 11), // firewood stack
            },
            Theme::Underground => match self.sprite {
                0 => (11, 30), // crystal cluster (was 14,32)
                1 => (12, 11), // bone pile (was 16,14)
                2 => (3, 22),  // torch (handle+coal; flame not solid)
                3 => (7, 12),  // skull (was 9,14)
                4 => (20, 9),  // fallen stalactite shard
                5 => (10, 16), // rusted chain pile
                6 => (9, 15),  // ribcage (was 12,18)
                7 => (10, 10), // glowing mushroom cluster
                8 => (8, 22),  // stalagmite
                9 => (11, 12), // minecart
                10 => (9, 11), // cracked geode
                11 => (5, 21), // lantern post
                12 => (8, 11), // treasure chest
                13 => (10, 12), // ore boulder (pick head above is decorative)
                _ => (8, 10),  // candle cluster (flames not solid)
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
            wa_seed: 0,
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

    /// Seed-deterministic scenery placement + silhouette bake.
    fn place_scenery(terrain: &mut Terrain, seed: u64) {
        // ── Scenery object placement ─────────────────────────────────────────
        // Derived entirely from seed — same on client and server, no StateMsg needed.
        {
            // Sprite-variant count must match the theme scenery.rs::draw_scenery
            // will pick for this map (same Theme::of dispatch): draw_underground /
            // draw_pastoral / draw_rugged — 15 / 16 / 15 sprites respectively.
            let count: u8 = match Theme::of(terrain.is_cavern, terrain.template_id) {
                Theme::Underground => 15,
                Theme::Pastoral => 16,
                Theme::Rugged => 15,
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
                let obj = SceneryObject { x: col, y: surface_y, sprite };
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

            // Bake each object's exact drawn silhouette into the solid
            // bitmap — scenery is real terrain now (WA-style), not a
            // decorative overlay: soldiers walk into it, explosions/torch
            // carve it the same way they carve dirt, and there's no separate
            // collision system to keep in sync with the renderer.
            let theme = Theme::of(terrain.is_cavern, terrain.template_id);
            for obj in terrain.scenery.clone() {
                let scale = obj.scale(theme);
                for (px, py) in crate::renderer::scenery::scenery_pixels(theme, obj.sprite, scale, obj.x as i32, obj.y as i32) {
                    terrain.set_solid(px, py, true);
                }
            }

            // Baked sprite silhouettes aren't guaranteed pixel-perfect flush
            // against the ground (the placement check above only requires a
            // solid pixel within EMBED_TOL below *some* columns of the
            // footprint, not full-width contact) — an object can bake in as
            // its own tiny disconnected component below the Phase 6b size
            // floor, invisible to the fragment filter because it didn't exist
            // yet when that pass ran. Re-running the same cleanup here (after
            // scenery, before it's too late to fix) catches these; a
            // disconnected sub-min_frag blob is debris same as pre-scenery
            // noise, and any object that's genuinely grounded stays part of
            // the connected landmass and is untouched.
            let is_cavern = terrain.is_cavern;
            Self::cleanup_solid_fragments(terrain, is_cavern);
        }

    }

    /// WA-exact terrain: Worms Armageddon's own landgen (see `wa_mapgen`), placed
    /// bottom-aligned on Arty's water line. `seed` folds to WA's 31-bit map seed
    /// (identity for seeds below 2^31, so a seed printed by WA's landgen.svg
    /// reproduces that exact shape). Style (island/cavern 0..=3) is derived from
    /// the seed as WA's UI picks it independently of the seed.
    pub fn generate_tactical(seed: u64) -> Self {
        use super::wa_mapgen;
        let wa_seed = ((seed ^ (seed >> 32)) as u32) & 0x7fff_ffff;
        let pick = wa_mapgen::lcg(wa_mapgen::lcg(wa_seed) ^ 0xA5A5_A5A5) >> 13;
        let k = (pick % 8) as usize;
        let (is_cavern, style) = (k >= 4, k % 4);
        let land = wa_mapgen::generate(wa_seed, is_cavern, style);
        let mut terrain = Self::empty();
        terrain.is_cavern = is_cavern;
        terrain.wa_seed = wa_seed;
        terrain.template_id = style as u8;
        terrain.surface_texture = (wa_mapgen::lcg(wa_seed ^ 0x5EED) >> 16) as u8;
        let dy = WATER_Y as i32 - land.h as i32;
        for y in 0..land.h {
            let wy = y as i32 + dy;
            if wy < 0 || wy >= WATER_Y as i32 { continue; }
            for x in 0..land.w.min(WORLD_W as usize) {
                if land.land[y * land.w + x] {
                    terrain.set_solid(x as i32, wy, true);
                }
            }
        }
        Self::recompute_spawn_derived(&mut terrain);
        terrain.texture = Some(Self::generate_dirt_texture(seed));
        Self::place_scenery(&mut terrain, seed);
        Self::recompute_spawn_derived(&mut terrain);
        terrain
    }

    /// Flood-fill fragment cleanup: remove solid components smaller than
    /// `min_frag` (noise junk / tiny floaters) outright, and drop mid-size
    /// components (below `big_frag`) once the kept (larger) components already
    /// cover most of the solid mass — so `n_chunks` comes down without ever
    /// hollowing out a map. Caverns are solid rock carved into large connected
    /// chambers, so they can afford a higher bar to clean up carve-noise debris.
    /// Island bar raised 50 → 4000 (MapGEN-corpus calibration 2026-07-10):
    /// reference maps hold 1-3 solid chunks; ours measured a median of 10,
    /// mostly confetti floaters. 4000px ≈ a 40x40 blob — anything smaller
    /// isn't a usable stepping stone (a soldier is 14x20), it's debris.
    /// A flat high cutoff guts fragmented collages (seed 4 dropped to 7% solid)
    /// — the mass-budget mid-tier and sole-ground guard below exist to avoid that.
    fn cleanup_solid_fragments(terrain: &mut Terrain, is_cavern: bool) {
        // 2026-07-13 expanded-corpus round: cavern big_frag 4000 -> 20000
        // (large free-floating rock blobs inside chambers survived; ref
        // caverns hold ~2 chunks vs our 9), island big_frag 16000 -> 24000
        // and mass budget 85 -> 80% (ours median 4 chunks vs ref 2). The
        // sole-ground guard below still protects coverage.
        // 2026-07-13 round 2: still 4 (island) / 8 (cavern) vs ref ~2 after
        // the above — pushed further: cavern big_frag 20000 -> 28000, island
        // 24000 -> 32000, mass budget 80 -> 72%. Turned out to be the wrong
        // knob: the extra chunks were sub-min_frag scenery-bake orphans
        // created *after* this pass ran (see the second call site, post
        // scenery bake) — these threshold bumps measurably did nothing.
        // Left at the round-2 values since they're harmless, but the real
        // fix is calling this function again after scenery is baked in.
        let min_frag: usize = if is_cavern { 1500 } else { 4000 };
        let big_frag: usize = if is_cavern { 28000 } else { 32000 };
        let mut comps: Vec<Vec<(i32, i32)>> = Vec::new();
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
                } else {
                    comps.push(comp.clone());
                }
            }
        }
        // Deterministic: components are found in scan order; stable sort by
        // size (descending) only.
        comps.sort_by(|a, b| b.len().cmp(&a.len()));
        let total: usize = comps.iter().map(|c| c.len()).sum();
        // Per-column count of components providing ground there: a chunk
        // that is the SOLE ground under many columns is load-bearing for
        // map coverage (chasms can sever the low waterline bridge into a
        // mid-size strip — dropping it once left 276 bare columns and
        // tripped island_relief_is_traversable) and must survive the
        // mid-tier cleanup regardless of the mass budget.
        let mut col_cover = vec![0u16; WORLD_W as usize];
        let comp_cols: Vec<Vec<u32>> = comps
            .iter()
            .map(|comp| {
                let mut seen = vec![false; WORLD_W as usize];
                for (cx, _) in comp { seen[*cx as usize] = true; }
                let cols: Vec<u32> = (0..WORLD_W).filter(|&c| seen[c as usize]).collect();
                for &c in &cols { col_cover[c as usize] += 1; }
                cols
            })
            .collect();
        let mut kept: usize = 0;
        for (comp, cols) in comps.iter().zip(&comp_cols) {
            let sole_ground = cols.iter().filter(|&&c| col_cover[c as usize] == 1).count();
            if kept * 100 >= total * 72 && comp.len() < big_frag && sole_ground <= 30 {
                for (cx, cy) in comp { terrain.set_solid(*cx, *cy, false); }
                for &c in cols { col_cover[c as usize] -= 1; }
            } else {
                kept += comp.len();
            }
        }
    }

    /// Populate spawn_y/sky_limit/solid_to_water/solid_runs from the current
    /// solid bitmap, for every column. Must be re-run any time solid pixels
    /// change after the initial fill (scenery bake, post-scenery fragment
    /// cleanup) — these caches drive spawn placement, the renderer's sky-aware
    /// viewport copy, and column-run lookups, and go stale silently otherwise.
    fn recompute_spawn_derived(terrain: &mut Terrain) {
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
    }

    /// Pick deterministic spawn positions for a team, scanning the real
    /// post-generation terrain. Pure function of the terrain (identical on client
    /// and server) — it never mutates the map. Candidates are restricted to the
    /// interior `[x_lo, x_hi]` band (callers pass left/right halves) and never
    /// within `SPAWN_EDGE_MARGIN` of a world edge. Returns up to `count`
    /// well-separated standable spots, actively spread across the map's vertical
    /// range; on very sparse terrain separation constraints relax rather than
    /// stamping artificial platforms.
    /// One WA placement pass for worms AND map objects (see `wa_mapgen::spawn_positions_ordered`): returns
    /// `(worm_spawns, object_spawns)`. `worm_spawns` is identical to `find_team_spawns(0, WORLD_W, worms)` (sorted by x);
    /// `object_spawns` are the next placements in WA's acceptance order, at most `objects` of them: fewer when the map has no room,
    /// exactly as WA skips mines and drums when there are too few placement coordinates. Without a WA seed (legacy maps) there are
    /// no object spawns.
    pub fn find_spawn_pool(&mut self, worms: usize, objects: usize) -> (Vec<WorldPos>, Vec<WorldPos>) {
        if self.wa_seed != 0 {
            let dy = WATER_Y as i32 - 696;
            let ordered = {
                let me: &Terrain = self;
                super::wa_mapgen::spawn_positions_ordered(&|x, y| me.is_solid(x, y + dy), self.wa_seed, 0, WORLD_W as i32, worms + objects)
            };
            if ordered.len() >= worms {
                let conv = |p: &(i32, i32)| WorldPos::new(p.0 as f32, (p.1 + dy) as f32);
                let mut w: Vec<(i32, i32)> = ordered[..worms].to_vec();
                w.sort_by_key(|p| p.0);
                let o: Vec<WorldPos> = ordered[worms..].iter().map(conv).collect();
                return (w.iter().map(conv).collect(), o);
            }
        }
        (self.find_team_spawns(0, WORLD_W, worms), Vec::new())
    }

    pub fn find_team_spawns(&mut self, x_lo: u32, x_hi: u32, count: usize) -> Vec<WorldPos> {
        if self.wa_seed != 0 {
            let dy = WATER_Y as i32 - 696;
            let found = {
                let me: &Terrain = self;
                super::wa_mapgen::spawn_positions(
                    &|x, y| me.is_solid(x, y + dy), self.wa_seed, x_lo as i32, x_hi as i32, count,
                )
            };
            if found.len() >= count {
                return found.iter().map(|&(x, y)| WorldPos::new(x as f32, (y + dy) as f32)).collect();
            }
        }
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
        // per-object margin to footprint+24 (~BODY_W + 10px of breathing
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
            // Quality floor (matches the "last resort" pass further down): a pick
            // scoring below this is still cramped — accepting it would exhaust this
            // chamber's own candidate pool on tightly-packed picks before falling
            // through to the generic-spawning section below, whose fine-grained
            // `standable_cave_foot_simple`-based pool often finds far better spread
            // in other chambers that this coarser `standable_cave_foot_y` pool missed.
            const MIN_ACCEPTABLE_SCORE: i32 = 250;
            while spawns.len() < count && !cave_cands.is_empty() {
                let pick = cave_cands.iter()
                    .filter(|&&(cx, cy)| used.iter().all(|&(ux, uy)| ux != cx || uy != cy))
                    .map(|&(cx, cy)| {
                        let score = used.iter().map(|&(ux, uy)| {
                            let dx = (ux - cx).abs() * 1000 / MIN_SEP;
                            let dy = (uy - cy).abs() * 1000 / MIN_SEP_V;
                            dx.max(dy)
                        }).min().unwrap_or(i32::MAX);
                        (score, cx, cy)
                    })
                    .max_by_key(|&(score, cx, _)| (score, -cx));
                match pick {
                    Some((score, cx, cy)) if score >= MIN_ACCEPTABLE_SCORE || used.is_empty() => {
                        spawns.push(WorldPos::new(cx as f32, cy as f32));
                        used.push((cx, cy));
                    }
                    _ => break, // no acceptably-spread pick left in this pool
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
        // platforms) — instead greedily disperse across a candidate pool, same idea
        // as the main wide-landform pass: each pick maximizes the worst-case
        // separation (as a fraction of MIN_SEP/MIN_SEP_V, matching sep_ok's OR rule)
        // to whoever's already placed. A first-fit left-to-right scan here would
        // clump the whole team on the first usable cluster of columns on a badly
        // fragmented map (e.g. a seed with zero landforms >=60px wide) even though a
        // lone usable column exists far away — greedy dispersion picks that far
        // column instead. Integer math only (no floats — must stay identical across
        // x86/ARM).
        //
        // The pool is `cands` (surface standable columns) densified with fine-grained
        // cave-floor points (every 4px, not the coarse 80px step used by the
        // cave-floor-fill pass above) — on a heavily fragmented surface, real cave
        // floor is often the only place left with enough spread to seat the whole
        // team without clustering; sampling it coarsely starved this pass of real
        // candidates and forced tightly-packed picks even when open cave floor was
        // available a few pixels to either side of the coarse samples. This applies
        // to true cavern maps too (previously skipped here): `cands` is built from
        // `standable_foot_levels`, which requires open sky above and so is nearly
        // empty underground — without cave-floor points in the pool this pass (and
        // Step 3 below) had almost nothing to disperse across on a cavern map,
        // forcing severe clustering onto whatever few sky-connected spots existed.
        let mut fallback_pool: Vec<(i32, i32)> = cands.clone();
        {
            let mut cx = lo;
            while cx <= hi {
                if let Some(fy) = self.standable_cave_foot_simple(cx) {
                    if clear_of_scenery(cx) { fallback_pool.push((cx, fy)); }
                }
                cx += 4;
            }
        }
        // Quality floor: a pick scoring below this (out of 1000 = full MIN_SEP/
        // MIN_SEP_V separation) is still "cramped" — accepting it would keep
        // piling soldiers onto the same narrow ledge just because it's the least-
        // bad option in THIS pool, even though Step 3 below can find real
        // macro-dispersion across the FULL map width (it divides lo..hi evenly
        // among the remaining soldiers rather than hill-climbing from `used`).
        // Without this floor, a map with only a handful of narrow footholds would
        // cram 3-4 soldiers onto each foothold instead of falling through to
        // Step 3's wide search — exactly the "5-6 soldiers in the same spot" bug.
        const MIN_ACCEPTABLE_SCORE: i32 = 250; // >=25% of MIN_SEP/MIN_SEP_V
        if spawns.len() < count {
            while spawns.len() < count {
                let pick = fallback_pool.iter()
                    .filter(|&&(cx, cy)| used.iter().all(|&(ux, uy)| ux != cx || uy != cy))
                    .map(|&(cx, cy)| {
                        let score = used.iter().map(|&(ux, uy)| {
                            let dx = (ux - cx).abs() * 1000 / MIN_SEP;
                            let dy = (uy - cy).abs() * 1000 / MIN_SEP_V;
                            dx.max(dy)
                        }).min().unwrap_or(i32::MAX);
                        (score, cx, cy)
                    })
                    .max_by_key(|&(score, cx, _)| (score, -cx));
                match pick {
                    Some((score, cx, cy)) if score >= MIN_ACCEPTABLE_SCORE || used.is_empty() => {
                        spawns.push(WorldPos::new(cx as f32, cy as f32));
                        used.push((cx, cy));
                    }
                    _ => break, // no acceptably-spread pick left — defer to Step 3
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
            // Two-pass search: first pass demands real separation (sep_ok, same rule
            // as every earlier pass) so this pathological fallback still spreads the
            // team out instead of clustering them just because the strict candidate
            // pool ran dry. Only if NO separated spot exists anywhere in the band
            // (a truly tiny/degenerate map) does the second pass fall back to merely
            // distinct — never re-using the exact same pixel, but no longer promising
            // real spacing.
            // On a cavern map `standable_foot_levels` requires open sky above and so
            // is almost always empty underground — use the cave-appropriate probe
            // instead, or this whole pathological fallback (and its "relaxed" pass
            // below) finds almost nothing and clusters everyone onto the rare
            // sky-connected spot it does find.
            let foot_levels_at = |px: i32| -> Vec<i32> {
                if self.is_cavern {
                    self.standable_cave_foot_simple(px).into_iter().collect()
                } else {
                    self.standable_foot_levels(px)
                }
            };
            let mut spot: Option<(i32, i32)> = None;
            'search: for d in 0..=(hi - lo) {
                for px in [base + d, base - d] {
                    if px < lo || px > hi { continue; }
                    for fy in foot_levels_at(px) {
                        if sep_ok(&used, px, fy, MIN_SEP, MIN_SEP_V) {
                            spot = Some((px, fy));
                            break 'search;
                        }
                    }
                }
            }
            // Relaxed fallback: no fully-separated spot exists in the band. Rather
            // than grabbing the literal first distinct point nearest `base` (which
            // let two different soldier indices independently converge on adjacent
            // columns of the same tiny leftover patch — e.g. 1px apart — since each
            // index's own outward search stops at its own first hit with no
            // awareness of the other), collect every distinct standable point in
            // the whole band and pick whichever maximizes the worst-case separation
            // to everyone already placed, same scoring as the "last resort" pass.
            if spot.is_none() {
                let mut best: Option<(i32, i32, i32)> = None; // (score, px, fy)
                for d in 0..=(hi - lo) {
                    for px in [base + d, base - d] {
                        if px < lo || px > hi { continue; }
                        for fy in foot_levels_at(px) {
                            if !used.iter().all(|&(ux, uy)| ux != px || uy != fy) { continue; }
                            let score = used.iter().map(|&(ux, uy)| {
                                let dx = (ux - px).abs() * 1000 / MIN_SEP;
                                let dy = (uy - fy).abs() * 1000 / MIN_SEP_V;
                                dx.max(dy)
                            }).min().unwrap_or(i32::MAX);
                            if best.map_or(true, |(bs, _, _)| score > bs) {
                                best = Some((score, px, fy));
                            }
                        }
                    }
                }
                spot = best.map(|(_, px, fy)| (px, fy));
            }
            if spot.is_none() {
                for require_sep in [true, false] {
                    if spot.is_some() { break; }
                    'surf: for d in 0..=(hi - lo) {
                        for px in [base + d, base - d] {
                            if px < lo || px > hi { continue; }
                            if let Some(sy) = self.surface_y_at(px as u32) {
                                let fy = sy as i32 - 1;
                                if fy <= 0 { continue; }
                                let ok = if require_sep {
                                    sep_ok(&used, px, fy, MIN_SEP, MIN_SEP_V)
                                } else {
                                    used.iter().all(|&(ux, uy)| ux != px || uy != fy)
                                };
                                if ok {
                                    spot = Some((px, fy));
                                    break 'surf;
                                }
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
        use crate::renderer::draw_sprites::BODY_HALF_W;
        const CLEAR_H: i32 = 24; // soldier body + clearance
        let x_l = x - BODY_HALF_W as i32;
        let x_r = x + BODY_HALF_W as i32;
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
        // BODY_W wide with solid walls flush against both edges — a soldier
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
        use crate::renderer::draw_sprites::BODY_HALF_W;
        const HEAD_H: i32 = 26;
        const CEIL_MAX: i32 = 220;
        if x < 0 || x >= WORLD_W as i32 { return None; }
        let x_l = x - BODY_HALF_W as i32;
        let x_r = x + BODY_HALF_W as i32;
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
            // Escape room: reject spots exactly BODY_W wide with solid walls
            // flush against both footprint edges — standable but unwalkable.
            let clear_col = |cx: i32| (foot_y - HEAD_H + 1 ..= foot_y)
                .all(|y| !self.is_solid(cx, y.max(0)));
            clear_col(x_l - 2) || clear_col(x_r + 2)
        };
        // Bottom-up: prefer deeper floors (main chambers) over high thin tunnels.
        (HEAD_H..WATER_Y as i32).rev().find(|&foot_y| ok(foot_y))
    }

    pub fn standable_cave_foot_y(&self, x: i32) -> Option<i32> {
        use crate::renderer::draw_sprites::BODY_HALF_W;
        const HEAD_H: i32 = 26;   // body + small clearance above the foot
        const CEIL_MAX: i32 = 220; // a ceiling must sit within this height to count as a cave
        if x < 0 || x >= WORLD_W as i32 { return None; }
        let x_l = x - BODY_HALF_W as i32;
        let x_r = x + BODY_HALF_W as i32;
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
            // Escape room: reject spots exactly BODY_W wide with solid walls
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
        use crate::renderer::draw_sprites::BODY_HALF_W;
        use std::collections::{HashSet, VecDeque};

        const STEP:       i32 = 8;   // grid step for the flood fill (px)
        const HEAD_H:     i32 = 26;  // body height above the foot
        const MAX_STEP:   i32 = 8;   // walk step-up/down allowance
        const JUMP_H:     i32 = 48;  // max jump height
        const FALL_MAX:   i32 = 120; // max fall the search will follow in one hop
        const OPEN_CLEAR: i32 = 220; // clear space above a floor = "open to sky"
        const MAX_VISITED: usize = 300;

        let half = BODY_HALF_W as i32;
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

    /// Every map (all styles across seeds 0..30) must give each team 4 spawns
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
                    // WA's own placement rule (FUN_005780b0): x in [20, W-60).
                    x >= 20 && x <= (WORLD_W - 40) as i32,
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
                && ((y - 420).max(0)..=(y - 26).max(0)).any(|cy| t.is_solid(x, cy))
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
