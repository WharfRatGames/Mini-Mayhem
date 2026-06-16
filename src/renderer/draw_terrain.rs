use crate::world::{Terrain, WORLD_W, WORLD_H, WATER_Y};
use super::buffer::WorldBuffer;
use super::fb::Bgra;

/// Set to false to revert to flat-colour terrain rendering.
const USE_TEXTURE: bool = true;

/// Colour palette for terrain rendering.
const GRASS: Bgra = Bgra::grass();
const EARTH: Bgra = Bgra::earth();
const WATER: Bgra = Bgra::water();

/// Per-archetype top-of-sky colour. The horizon (bottom) colour is kept identical
/// across biomes so the waterline reads consistently and the water-trough sky
/// restore in `draw_water_surface` matches regardless of map. Only the upper sky
/// tint varies: hills (default), cliffs (cold pale), islands (bright teal),
/// caverns (dim grey-blue), canyon (warm dusty).
fn sky_top(archetype: u8) -> (f32, f32, f32) {
    match archetype {
        1 => (95.0, 120.0, 165.0),  // cliffs — cold, pale
        2 => (40.0, 120.0, 165.0),  // islands — bright teal
        3 => (45.0, 60.0,  92.0),   // caverns — dim grey-blue
        4 => (98.0, 92.0,  130.0),  // canyon — warm dusty
        _ => (50.0, 85.0,  145.0),  // hills / default — original
    }
}

/// Faint baked cloud bands: soft additive whitening in the upper sky only, from a
/// cheap layered sine of (x, y). Deterministic, so it bakes into the world cache at
/// zero per-frame cost. Returns 0 below the cloud zone so the horizon stays clean.
fn cloud_amount(x: i32, y: i32) -> f32 {
    let zone = WATER_Y as f32 * 0.55;
    if (y as f32) >= zone { return 0.0; }
    let xf = x as f32;
    let yf = y as f32;
    // Two slow waves → broad, drifting band shapes.
    let n = (xf * 0.0060 + yf * 0.020).sin() * 0.5
          + (xf * 0.0021 - yf * 0.012).sin() * 0.5;
    // Keep only the band crests, soften the edges.
    let band = ((n - 0.28) / 0.72).clamp(0.0, 1.0);
    // Fade clouds out toward the bottom of the zone and the very top.
    let fade = 1.0 - (yf / zone);
    band * fade * 30.0
}

/// Sky gradient: deep blue at top → light hazy blue at water horizon, tinted per
/// map archetype, with faint baked cloud bands in the upper sky.
pub fn sky_colour(x: i32, y: i32, archetype: u8) -> Bgra {
    let t = (y as f32 / WATER_Y as f32).clamp(0.0, 1.0);
    let (tr, tg, tb) = sky_top(archetype);
    // Bottom (horizon) endpoint is biome-independent.
    let c = cloud_amount(x, y);
    Bgra::new(
        (tr + (120.0 - tr) * t + c).clamp(0.0, 255.0) as u8,
        (tg + (180.0 - tg) * t + c).clamp(0.0, 255.0) as u8,
        (tb + (230.0 - tb) * t + c).clamp(0.0, 255.0) as u8,
    )
}

/// Wet terrain tint for pixels near the waterline.
/// Progressively darkens and adds a slight blue-grey tinge approaching WATER_Y.
/// Returns the color unchanged if outside the wet zone.
fn wet_tint(color: Bgra, y: i32) -> Bgra {
    const WET_ZONE: f32 = 40.0;
    let dist = (WATER_Y as i32 - y).max(0) as f32;
    if dist >= WET_ZONE { return color; }
    let wet = 1.0 - dist / WET_ZONE;
    Bgra::new(
        (color.r as f32 * (1.0 - wet * 0.45)) as u8,
        (color.g as f32 * (1.0 - wet * 0.35)) as u8,
        ((color.b as f32 * (1.0 - wet * 0.10)) + 10.0 * wet) as u8,
    )
}

/// Compute the colour for a single pixel at (x, y) based on terrain state.
/// Shared by draw_terrain, draw_terrain_viewport, build_world_cache, update_cache_region.
#[inline(always)]
fn terrain_pixel(terrain: &Terrain, x: i32, y: i32) -> Bgra {
    if y >= WATER_Y as i32 {
        return WATER;
    }
    if !terrain.is_solid(x, y) {
        return sky_colour(x, y, terrain.archetype);
    }
    let air_above = !terrain.is_solid(x, y - 1);
    let air_below = !terrain.is_solid(x, y + 1);
    let air_left  = !terrain.is_solid(x - 1, y);
    let air_right = !terrain.is_solid(x + 1, y);
    let exposed_face = (air_left || air_right) && !air_above && !air_below;

    // Base material colour: sample the per-map texture-atlas tile by depth from
    // the column surface (fill-silhouette). Falls back to the procedural dirt
    // texture, then flat colours, if the atlas is unavailable.
    let (r, g, b) = if USE_TEXTURE {
        if let Some((r, g, b)) = atlas_sample(terrain, x, y) {
            (r, g, b)
        } else if let Some(tex) = &terrain.texture {
            let [r, g, b, _] = tex[(y as usize & 255) * 256 + (x as usize & 255)];
            (r, g, b)
        } else if air_above {
            return wet_or_not(GRASS, y, air_above);
        } else {
            (EARTH.r, EARTH.g, EARTH.b)
        }
    } else {
        return wet_or_not(if air_above { GRASS } else { EARTH }, y, air_above);
    };

    // Edge shading: brighten exposed vertical faces, darken overhang undersides.
    let raw = if exposed_face {
        Bgra::new(
            (r as u16 + 22).min(255) as u8,
            (g as u16 + 14).min(255) as u8,
            (b as u16 + 6).min(255) as u8,
        )
    } else if air_below {
        Bgra::new(r.saturating_sub(28), g.saturating_sub(20), b.saturating_sub(10))
    } else {
        Bgra::new(r, g, b)
    };
    // Surface row is never wet-tinted (matches old grass behaviour).
    wet_or_not(raw, y, air_above)
}

/// Apply wet-tint unless this is the exposed surface row.
#[inline(always)]
fn wet_or_not(c: Bgra, y: i32, air_above: bool) -> Bgra {
    if air_above { c } else { wet_tint(c, y) }
}

/// Surface-band height (px) that always maps 1:1 from the column surface before
/// the body texture begins repeating.
const SURFACE_BAND: usize = 24;

/// Reflective (ping-pong) tiling index: 0..size-1 then size-1..0, so adjacent
/// tiles mirror each other and their shared edges match — eliminates the hard
/// seam a plain modulo wrap produces every `size` pixels.
#[inline]
fn mirror_index(coord: i32, size: usize) -> usize {
    if size == 0 { return 0; }
    let period = 2 * size as i32;
    let m = coord.rem_euclid(period);
    if m < size as i32 { m as usize } else { (period - 1 - m) as usize }
}

/// Sample the chosen texture-atlas tile for a solid pixel, returning (r, g, b).
/// Returns None if the atlas isn't loaded so callers can fall back.
#[inline]
fn atlas_sample(terrain: &Terrain, x: i32, y: i32) -> Option<(u8, u8, u8)> {
    let tile = super::terrain_textures::tile(terrain.surface_texture)?;
    if tile.w == 0 || tile.h < 4 { return None; }
    // Texture depth is measured from THIS pixel's own landform top — the nearest
    // air directly above it — not from `spawn_y[x]` (the topmost solid in the whole
    // column). A column can hold several disconnected landforms stacked over air
    // (a floating overhang/slab above a lower ridge in chasm/cliff terrain); keying
    // off `spawn_y` measured the lower landform from the high slab, sampling deep
    // into the tile's bare body — so adjacent columns showed vertical strips of
    // different browns with the top-of-tile surface treatment (grass/flowers)
    // missing. Scanning to the local run top puts the surface band on every piece.
    // Capped at the terrain height so the worst case stays bounded.
    let mut surf = y;
    let cap = (crate::world::TERRAIN_MAX_Y as i32 - crate::world::TERRAIN_MIN_Y as i32).max(1);
    let mut steps = 0;
    while surf > 0 && steps < cap && terrain.is_solid(x, surf - 1) {
        surf -= 1;
        steps += 1;
    }
    let depth = (y - surf).max(0) as usize;
    // Mirror horizontally so tile boundaries don't show a vertical seam line.
    let tx = mirror_index(x, tile.w);
    let body_h = tile.h.saturating_sub(SURFACE_BAND);

    // First `tile.h` px of depth show the tile top-to-bottom once (surface
    // treatment included); below that, mirror-repeat only the body band so the
    // vertical repeat has no horizontal seam line either.
    let pick = |d: usize| -> [u8; 4] {
        let ty = if d < tile.h {
            d
        } else if body_h > 0 {
            SURFACE_BAND + mirror_index((d - tile.h) as i32, body_h)
        } else {
            d % tile.h
        };
        tile.sample(tx, ty)
    };

    let mut p = pick(depth);
    // Transparent gap in the cap → fill with the body band so the silhouette
    // never shows a hole.
    if p[3] < 128 {
        p = pick(depth.max(tile.h));
        if p[3] < 128 { return Some((EARTH.r, EARTH.g, EARTH.b)); }
    }
    Some((p[0], p[1], p[2]))
}

/// Build the world cache — render sky + terrain for the entire world.
/// Expensive: O(WORLD_W × WORLD_H). Called once per game start.
pub fn build_world_cache(cache: &mut WorldBuffer, terrain: &Terrain) {
    for x in 0..WORLD_W as i32 {
        for y in 0..WORLD_H as i32 {
            cache.set_pixel(x, y, terrain_pixel(terrain, x, y));
        }
    }
}

/// Patch a rectangular dirty region in the world cache after an explosion.
/// `cx`, `cy`, `r` define the explosion — patch [cx−r .. cx+r] × [cy−r .. cy+r].
pub fn update_cache_region(cache: &mut WorldBuffer, terrain: &Terrain, cx: f32, cy: f32, r: f32) {
    let x0 = (cx - r).floor() as i32;
    let x1 = (cx + r).ceil()  as i32;
    let y0 = (cy - r).floor() as i32;
    let y1 = (cy + r).ceil()  as i32;
    let x0 = x0.max(0);
    let x1 = x1.min(WORLD_W as i32 - 1);
    let y0 = y0.max(0);
    let y1 = y1.min(WORLD_H as i32 - 1);
    for x in x0..=x1 {
        for y in y0..=y1 {
            cache.set_pixel(x, y, terrain_pixel(terrain, x, y));
        }
    }
}

/// Draw the full terrain into the world buffer.
///
/// For each column x:
///   - Above surface_y:        sky
///   - At surface_y:           grass
///   - Below surface_y to WATER_Y: earth
///   - WATER_Y to WORLD_H:    water
///
/// This is called once per frame after clearing the buffer.
/// Superseded by build_world_cache + copy_viewport_from in the caching renderer.
pub fn draw_terrain(buf: &mut WorldBuffer, terrain: &Terrain) {
    for x in 0..WORLD_W as i32 {
        for y in 0..WORLD_H as i32 {
            buf.set_pixel(x, y, terrain_pixel(terrain, x, y));
        }
    }
}

/// Draw only the columns within the camera viewport.
/// More efficient than drawing the full 3200px world every frame.
/// `cam_x` is the left edge of the 640px viewport in world pixels.
pub fn draw_terrain_viewport(
    buf:     &mut WorldBuffer,
    terrain: &Terrain,
    cam_x:   u32,
) {
    let x_start = cam_x as i32;
    let x_end   = (cam_x + crate::world::SCREEN_W).min(WORLD_W) as i32;

    for x in x_start..x_end {
        for y in 0..WORLD_H as i32 {
            buf.set_pixel(x, y, terrain_pixel(terrain, x, y));
        }
    }
}

/// Find the topmost solid pixel in column x.
/// Returns None if the column is entirely air.
fn find_surface(terrain: &Terrain, x: i32) -> Option<i32> {
    for y in 0..WATER_Y as i32 {
        if terrain.is_solid(x, y) {
            return Some(y);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Heightmap, Terrain};

    fn real(seed: u64) -> Terrain {
        Terrain::from_heightmap(&Heightmap::generate(seed))
    }

    fn empty() -> Terrain { Terrain::empty() }

    // ── find_surface ─────────────────────────────────────────────────────────

    #[test]
    fn find_surface_returns_first_solid_row() {
        let mut t = empty();
        t.set_solid(10, 50, true);
        t.set_solid(10, 51, true);
        t.set_solid(10, 52, true);
        assert_eq!(find_surface(&t, 10), Some(50));
    }

    #[test]
    fn find_surface_empty_column_returns_none() {
        let t = empty();
        assert_eq!(find_surface(&t, 100), None);
    }

    #[test]
    fn find_surface_ignores_water_rows() {
        let mut t = empty();
        // Solid only in water zone — should return None
        t.set_solid(10, WATER_Y as i32, true);
        assert_eq!(find_surface(&t, 10), None);
    }

    // ── draw_terrain ─────────────────────────────────────────────────────────

    #[test]
    fn surface_pixel_is_terrain() {
        let mut t = empty();
        t.set_solid(100, 200, true);
        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &t);
        let p = buf.get_pixel(100, 200);
        assert_ne!(p, sky_colour(100, 200, t.archetype), "surface pixel should be terrain, not sky");
        assert_ne!(p, WATER, "surface pixel should be terrain, not water");
    }

    #[test]
    fn pixel_above_surface_is_sky() {
        let mut t = empty();
        t.set_solid(100, 200, true);
        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &t);
        assert_eq!(buf.get_pixel(100, 199), sky_colour(100, 199, t.archetype), "pixel above surface should be sky gradient");
        assert_eq!(buf.get_pixel(100, 0),   sky_colour(100, 0, t.archetype),   "top of world should be sky gradient");
    }

    #[test]
    fn pixel_below_surface_is_earth() {
        let mut t = empty();
        t.set_solid(100, 200, true);
        t.set_solid(100, 201, true);
        t.set_solid(100, 202, true);
        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &t);
        // Below-surface pixels render as terrain (textured), not sky or water.
        for y in [201, 202] {
            let p = buf.get_pixel(100, y);
            assert_ne!(p, sky_colour(100, y, t.archetype), "y={y} below surface should be terrain, not sky");
            assert_ne!(p, WATER, "y={y} below surface should be terrain, not water");
        }
    }

    #[test]
    fn water_rows_are_water_colour() {
        let t = empty();
        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &t);
        for x in [0, 100, 640, 1600, WORLD_W as i32 - 1] {
            for y in WATER_Y as i32..WORLD_H as i32 {
                assert_eq!(
                    buf.get_pixel(x, y), WATER,
                    "x={x} y={y} should be water"
                );
            }
        }
    }

    #[test]
    fn empty_terrain_column_is_all_sky_above_water() {
        let t = empty();
        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &t);
        for y in 0..WATER_Y as i32 {
            assert_eq!(buf.get_pixel(500, y), sky_colour(500, y, t.archetype),
                "y={y} should be sky gradient in empty column");
        }
    }

    #[test]
    fn crater_gap_shows_sky_inside_earth() {
        let mut t = empty();
        for y in 200..300i32 { t.set_solid(100, y, true); }
        t.set_solid(100, 230, false);
        t.set_solid(100, 231, false);

        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &t);

        assert_ne!(buf.get_pixel(100, 200), sky_colour(100, 200, t.archetype), "surface should be terrain");
        // Crater gap shows sky gradient
        assert_eq!(buf.get_pixel(100, 230), sky_colour(100, 230, t.archetype));
        assert_eq!(buf.get_pixel(100, 231), sky_colour(100, 231, t.archetype));
        // Solid pixels render as terrain, not sky
        assert_ne!(buf.get_pixel(100, 220), sky_colour(100, 220, t.archetype), "y=220 should be terrain");
        assert_ne!(buf.get_pixel(100, 250), sky_colour(100, 250, t.archetype), "y=250 should be terrain");
    }

    #[test]
    fn fully_cratered_column_is_all_sky() {
        let t = empty();
        // Set then clear — column has no solid pixels
        // (simulating a column fully destroyed by explosions)
        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &t);
        for y in 0..WATER_Y as i32 {
            assert_eq!(buf.get_pixel(200, y), sky_colour(200, y, t.archetype));
        }
    }

    #[test]
    fn real_terrain_surface_is_solid_and_above_is_sky() {
        let t = real(42);
        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &t);

        // Sample several columns: surface pixel is terrain, pixel above is sky.
        for x in (0..WORLD_W as i32).step_by(100) {
            if let Some(sy) = find_surface(&t, x) {
                let p = buf.get_pixel(x, sy);
                assert_ne!(p, sky_colour(x, sy, t.archetype), "x={x} surface y={sy} should be terrain, not sky");
                assert_ne!(p, WATER, "x={x} surface y={sy} should be terrain, not water");
                if sy > 0 {
                    assert_eq!(
                        buf.get_pixel(x, sy - 1), sky_colour(x, sy - 1, t.archetype),
                        "x={x} y={} should be sky", sy - 1
                    );
                }
            }
        }
    }

    #[test]
    fn real_terrain_water_is_always_water_colour() {
        let t = real(99);
        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &t);
        for x in (0..WORLD_W as i32).step_by(200) {
            for y in WATER_Y as i32..WORLD_H as i32 {
                assert_eq!(buf.get_pixel(x, y), WATER);
            }
        }
    }

    // ── draw_terrain_viewport ─────────────────────────────────────────────────

    #[test]
    fn viewport_draw_matches_full_draw_in_visible_range() {
        let t = real(7);
        let cam_x = 640u32;

        let mut buf_full     = WorldBuffer::new();
        let mut buf_viewport = WorldBuffer::new();

        draw_terrain(&mut buf_full, &t);
        draw_terrain_viewport(&mut buf_viewport, &t, cam_x);

        // Within the viewport columns should be identical
        for x in cam_x as i32..(cam_x + crate::world::SCREEN_W) as i32 {
            for y in (0..WORLD_H as i32).step_by(10) {
                assert_eq!(
                    buf_full.get_pixel(x, y),
                    buf_viewport.get_pixel(x, y),
                    "x={x} y={y} should match between full and viewport draw"
                );
            }
        }
    }

    #[test]
    fn viewport_draw_does_not_draw_outside_viewport() {
        let t = real(5);
        let cam_x = 1000u32;
        let mut buf = WorldBuffer::new();
        // Buffer starts black — if viewport draw touches columns outside range
        // the test pixels at x=0 would change
        draw_terrain_viewport(&mut buf, &t, cam_x);
        // Column 0 is outside the viewport (cam_x=1000) — should still be black
        assert_eq!(buf.get_pixel(0, 100), Bgra::black(),
            "viewport draw should not touch columns outside viewport");
    }
}
