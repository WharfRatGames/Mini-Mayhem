/// Scenery object renderer — original pixel-art decorations placed on the terrain surface.
/// One theme per terrain archetype; objects are drawn with fill_rect/set_pixel calls.
/// All coordinates passed as bottom-center of the object in world space.

use super::buffer::WorldBuffer;
use super::fb::Bgra;
use crate::world::terrain::Terrain;
use crate::world::constants::{SCREEN_W, SCREEN_H, WATER_Y};

pub fn draw_scenery(buf: &mut WorldBuffer, terrain: &Terrain, cam_x: i32, cam_y: i32) {
    for obj in &terrain.scenery {
        let wx = obj.x as i32;
        let wy = obj.y as i32;
        // Rough viewport cull (objects are at most ~60px tall and 32px wide)
        if wx < cam_x - 64 || wx > cam_x + SCREEN_W as i32 + 64 { continue; }
        if wy < cam_y - 80 || wy > cam_y + SCREEN_H as i32 + 16 { continue; }
        match terrain.archetype {
            0 => draw_pastoral(buf, wx, wy, obj.sprite),
            1 => draw_rugged(buf, wx, wy, obj.sprite),
            2 => draw_tropical(buf, wx, wy, obj.sprite),
            3 => draw_underground(buf, wx, wy, obj.sprite),
            4 => draw_arid(buf, wx, wy, obj.sprite),
            _ => {}
        }
    }
}

// ── Archetype 0: Pastoral (hills) ─────────────────────────────────────────────

fn draw_pastoral(buf: &mut WorldBuffer, cx: i32, by: i32, sprite: u8) {
    match sprite {
        0 => draw_flower(buf, cx, by),
        1 => draw_mushroom(buf, cx, by),
        2 => draw_mossy_rock(buf, cx, by),
        3 => draw_fence_post(buf, cx, by),
        _ => draw_bush(buf, cx, by),
    }
}

fn draw_flower(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let stem   = Bgra::new(50, 160, 50);
    let dark   = Bgra::new(20, 90, 20);
    let petal  = Bgra::new(80, 80, 240);
    let petal2 = Bgra::new(120, 120, 255);
    let center = Bgra::new(40, 220, 240);
    // Stem
    buf.fill_rect(cx,     by - 16, 2, 16, dark);
    buf.fill_rect(cx + 1, by - 15, 1, 14, stem);
    // Leaf
    buf.fill_rect(cx - 4, by - 9, 4, 2, stem);
    buf.fill_rect(cx - 3, by - 10, 2, 1, stem);
    // Petals (5-point arrangement using pairs of rects)
    buf.fill_rect(cx - 1, by - 24, 4, 4, dark);
    buf.fill_rect(cx,     by - 23, 2, 3, petal2);
    buf.fill_rect(cx - 5, by - 21, 4, 4, dark);
    buf.fill_rect(cx - 4, by - 20, 2, 3, petal);
    buf.fill_rect(cx + 3, by - 21, 4, 4, dark);
    buf.fill_rect(cx + 4, by - 20, 2, 3, petal);
    buf.fill_rect(cx - 4, by - 17, 4, 4, dark);
    buf.fill_rect(cx - 3, by - 16, 2, 3, petal);
    buf.fill_rect(cx + 2, by - 17, 4, 4, dark);
    buf.fill_rect(cx + 3, by - 16, 2, 3, petal);
    // Center
    buf.fill_rect(cx - 1, by - 20, 4, 4, dark);
    buf.fill_rect(cx,     by - 19, 2, 2, center);
}

fn draw_mushroom(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let stem  = Bgra::new(235, 230, 215);
    let sdark = Bgra::new(170, 160, 140);
    let cap   = Bgra::new(50, 50, 210);
    let cark  = Bgra::new(20, 20, 140);
    let spot  = Bgra::new(240, 240, 235);
    // Stem
    buf.fill_rect(cx - 4, by - 10, 9, 10, sdark);
    buf.fill_rect(cx - 3, by - 10, 7,  9, stem);
    buf.fill_rect(cx - 3, by - 10, 3,  9, Bgra::new(245, 245, 230));
    // Cap outline
    buf.fill_rect(cx - 7, by - 16, 15, 2, cark);
    buf.fill_rect(cx - 9, by - 14, 19, 5, cark);
    buf.fill_rect(cx - 8, by - 11, 17, 2, cark);
    // Cap fill
    buf.fill_rect(cx - 6, by - 15, 13, 2, cap);
    buf.fill_rect(cx - 8, by - 13, 17, 4, cap);
    buf.fill_rect(cx - 7, by - 10, 15, 1, cap);
    // Highlight
    buf.fill_rect(cx - 5, by - 14, 4, 3, Bgra::new(100, 100, 240));
    // Spots
    buf.fill_rect(cx - 1, by - 14, 3, 3, spot);
    buf.fill_rect(cx + 4, by - 13, 2, 2, spot);
    buf.fill_rect(cx - 5, by - 12, 2, 2, spot);
}

fn draw_mossy_rock(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let rock  = Bgra::new(120, 115, 108);
    let rdark = Bgra::new(70, 65, 60);
    let rhi   = Bgra::new(165, 160, 150);
    let moss  = Bgra::new(60, 140, 55);
    let mdark = Bgra::new(35, 90, 30);
    // Rock silhouette
    buf.fill_rect(cx - 10, by - 4,  21, 2, rdark);
    buf.fill_rect(cx - 12, by - 8,  25, 6, rdark);
    buf.fill_rect(cx - 11, by - 7,  23, 5, rock);
    buf.fill_rect(cx - 9,  by - 9,  19, 3, rock);
    buf.fill_rect(cx - 8,  by - 3,  17, 2, rdark);
    // Highlight
    buf.fill_rect(cx - 8,  by - 8,   6, 2, rhi);
    buf.fill_rect(cx - 9,  by - 7,   3, 3, rhi);
    // Moss patches
    buf.fill_rect(cx - 4,  by - 10,  9, 3, mdark);
    buf.fill_rect(cx - 3,  by - 9,   7, 2, moss);
    buf.fill_rect(cx + 4,  by - 8,   4, 2, moss);
    buf.fill_rect(cx - 8,  by - 8,   3, 2, moss);
}

fn draw_fence_post(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let wood  = Bgra::new(180, 140, 90);
    let wdark = Bgra::new(110, 80, 45);
    let wtop  = Bgra::new(210, 170, 110);
    // Post body
    buf.fill_rect(cx - 2, by - 22, 5, 22, wdark);
    buf.fill_rect(cx - 1, by - 22, 3, 21, wood);
    buf.fill_rect(cx,     by - 22, 1, 20, wtop);
    // Pointed top
    buf.fill_rect(cx - 1, by - 25, 3, 3, wdark);
    buf.fill_rect(cx,     by - 25, 1, 2, wood);
    // Horizontal rail (left crossbar)
    buf.fill_rect(cx - 14, by - 16, 12, 3, wdark);
    buf.fill_rect(cx - 13, by - 16, 11, 2, wood);
    // Horizontal rail (right crossbar)
    buf.fill_rect(cx + 3,  by - 16, 12, 3, wdark);
    buf.fill_rect(cx + 4,  by - 16, 11, 2, wood);
    // Lower rail
    buf.fill_rect(cx - 14, by - 8,  12, 3, wdark);
    buf.fill_rect(cx - 13, by - 8,  11, 2, wood);
    buf.fill_rect(cx + 3,  by - 8,  12, 3, wdark);
    buf.fill_rect(cx + 4,  by - 8,  11, 2, wood);
}

fn draw_bush(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let green  = Bgra::new(50, 155, 45);
    let dark   = Bgra::new(20, 80, 18);
    let light  = Bgra::new(85, 195, 75);
    // Dark outline
    buf.fill_rect(cx - 11, by - 8,  23, 2, dark);
    buf.fill_rect(cx - 13, by - 12, 27, 6, dark);
    buf.fill_rect(cx - 12, by - 14, 25, 4, dark);
    buf.fill_rect(cx - 10, by - 16, 21, 3, dark);
    buf.fill_rect(cx - 7,  by - 18, 15, 3, dark);
    // Green fill
    buf.fill_rect(cx - 10, by - 7,  21, 1, green);
    buf.fill_rect(cx - 12, by - 11, 25, 5, green);
    buf.fill_rect(cx - 11, by - 13, 23, 3, green);
    buf.fill_rect(cx - 9,  by - 15, 19, 2, green);
    buf.fill_rect(cx - 6,  by - 17, 13, 2, green);
    // Light highlights (lumpy clusters)
    buf.fill_rect(cx - 7,  by - 16, 6, 3, light);
    buf.fill_rect(cx + 1,  by - 15, 5, 4, light);
    buf.fill_rect(cx - 10, by - 12, 5, 3, light);
}

// ── Archetype 1: Rugged (cliffs) ──────────────────────────────────────────────

fn draw_rugged(buf: &mut WorldBuffer, cx: i32, by: i32, sprite: u8) {
    match sprite {
        0 => draw_pine_tree(buf, cx, by),
        1 => draw_boulder(buf, cx, by),
        2 => draw_wooden_crate(buf, cx, by),
        _ => draw_dead_stump(buf, cx, by),
    }
}

fn draw_pine_tree(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let trunk  = Bgra::new(100, 65, 30);
    let tdark  = Bgra::new(55, 35, 12);
    let green  = Bgra::new(38, 120, 42);
    let dark   = Bgra::new(18, 65, 20);
    let light  = Bgra::new(70, 165, 65);
    // Trunk
    buf.fill_rect(cx - 2, by - 10, 5, 10, tdark);
    buf.fill_rect(cx - 1, by - 10, 3,  9, trunk);
    // Bottom tier (widest)
    buf.fill_rect(cx - 11, by - 16, 23, 2, dark);
    buf.fill_rect(cx - 10, by - 18, 21, 4, dark);
    buf.fill_rect(cx - 9,  by - 17, 19, 3, green);
    buf.fill_rect(cx - 10, by - 16, 21, 1, green);
    // Middle tier
    buf.fill_rect(cx - 8, by - 24, 17, 2, dark);
    buf.fill_rect(cx - 7, by - 27, 15, 5, dark);
    buf.fill_rect(cx - 6, by - 26, 13, 4, green);
    buf.fill_rect(cx - 7, by - 24, 15, 1, green);
    // Top tier
    buf.fill_rect(cx - 5, by - 32, 11, 2, dark);
    buf.fill_rect(cx - 4, by - 35, 9,  5, dark);
    buf.fill_rect(cx - 3, by - 34, 7,  4, green);
    buf.fill_rect(cx - 4, by - 32, 9,  1, green);
    // Tip
    buf.fill_rect(cx - 2, by - 39, 5, 5, dark);
    buf.fill_rect(cx - 1, by - 38, 3, 4, light);
    buf.fill_rect(cx,     by - 40, 1, 2, light);
    // Light patches
    buf.fill_rect(cx - 6, by - 25, 4, 2, light);
    buf.fill_rect(cx + 1, by - 26, 3, 2, light);
    buf.fill_rect(cx - 8, by - 17, 4, 2, light);
}

fn draw_boulder(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let rock  = Bgra::new(105, 100, 95);
    let rdark = Bgra::new(55, 50, 48);
    let rhi   = Bgra::new(158, 152, 144);
    let crack = Bgra::new(45, 42, 40);
    // Silhouette outline
    buf.fill_rect(cx - 12, by - 3,  25, 2, rdark);
    buf.fill_rect(cx - 15, by - 9,  31, 8, rdark);
    buf.fill_rect(cx - 14, by - 15, 29, 8, rdark);
    buf.fill_rect(cx - 11, by - 18, 23, 4, rdark);
    buf.fill_rect(cx - 7,  by - 20, 15, 3, rdark);
    // Rock fill
    buf.fill_rect(cx - 11, by - 2,  23, 1, rock);
    buf.fill_rect(cx - 14, by - 8,  29, 7, rock);
    buf.fill_rect(cx - 13, by - 14, 27, 7, rock);
    buf.fill_rect(cx - 10, by - 17, 21, 3, rock);
    buf.fill_rect(cx - 6,  by - 19, 13, 2, rock);
    // Highlights
    buf.fill_rect(cx - 10, by - 16,  8, 3, rhi);
    buf.fill_rect(cx - 12, by - 12,  5, 5, rhi);
    buf.fill_rect(cx - 11, by - 8,   4, 2, rhi);
    // Cracks
    buf.fill_rect(cx + 3,  by - 14,  1, 7, crack);
    buf.fill_rect(cx + 4,  by - 12,  1, 4, crack);
    buf.fill_rect(cx - 4,  by - 9,   1, 5, crack);
}

fn draw_wooden_crate(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let wood  = Bgra::new(185, 145, 80);
    let dark  = Bgra::new(90, 65, 28);
    let light = Bgra::new(220, 185, 115);
    let plank = Bgra::new(160, 120, 60);
    // Box outline
    buf.fill_rect(cx - 10, by - 18, 21, 18, dark);
    // Box fill
    buf.fill_rect(cx - 9, by - 17, 19, 16, wood);
    // Top highlight
    buf.fill_rect(cx - 9, by - 17, 19, 2, light);
    buf.fill_rect(cx - 9, by - 17, 2, 16, light);
    // Plank lines (horizontal)
    buf.fill_rect(cx - 9, by - 11, 19, 1, dark);
    buf.fill_rect(cx - 9, by - 10, 19, 1, plank);
    // Cross braces (X pattern)
    for i in 0..14i32 {
        buf.set_pixel(cx - 9 + i, by - 16 + i, dark);
        buf.set_pixel(cx + 9 - i, by - 16 + i, dark);
    }
    // Nail dots at corners
    buf.fill_rect(cx - 8, by - 16, 2, 2, dark);
    buf.fill_rect(cx + 7, by - 16, 2, 2, dark);
    buf.fill_rect(cx - 8, by - 3,  2, 2, dark);
    buf.fill_rect(cx + 7, by - 3,  2, 2, dark);
}

fn draw_dead_stump(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let wood  = Bgra::new(125, 88, 50);
    let dark  = Bgra::new(65, 42, 22);
    let bark  = Bgra::new(90, 62, 33);
    let rot   = Bgra::new(70, 90, 50);
    // Stump body outline
    buf.fill_rect(cx - 8,  by - 2,  17, 2, dark);
    buf.fill_rect(cx - 9,  by - 12, 19, 12, dark);
    buf.fill_rect(cx - 8,  by - 14, 17, 3, dark);
    // Stump body fill
    buf.fill_rect(cx - 8,  by - 11, 17, 10, wood);
    buf.fill_rect(cx - 7,  by - 13, 15, 3, bark);
    // Bark texture lines
    buf.fill_rect(cx - 7, by - 10, 1, 8, dark);
    buf.fill_rect(cx - 3, by - 9,  1, 7, dark);
    buf.fill_rect(cx + 1, by - 11, 1, 9, dark);
    buf.fill_rect(cx + 5, by - 10, 1, 7, dark);
    // Rotting top
    buf.fill_rect(cx - 7, by - 13, 15, 2, rot);
    buf.fill_rect(cx - 5, by - 14, 11, 1, rot);
    // Roots
    buf.fill_rect(cx - 12, by - 4,  4, 2, bark);
    buf.fill_rect(cx + 9,  by - 4,  4, 2, bark);
    buf.fill_rect(cx - 11, by - 3,  3, 2, dark);
    buf.fill_rect(cx + 9,  by - 3,  3, 2, dark);
}

// ── Archetype 2: Tropical (islands) ───────────────────────────────────────────

fn draw_tropical(buf: &mut WorldBuffer, cx: i32, by: i32, sprite: u8) {
    match sprite {
        0 => draw_palm_tree(buf, cx, by),
        1 => draw_barrel(buf, cx, by),
        2 => draw_tent_shanty(buf, cx, by),
        _ => draw_anchor(buf, cx, by),
    }
}

fn draw_palm_tree(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let trunk  = Bgra::new(160, 120, 60);
    let tdark  = Bgra::new(100, 72, 30);
    let ring   = Bgra::new(130, 95, 45);
    let green  = Bgra::new(40, 180, 60);
    let gdark  = Bgra::new(18, 110, 30);
    let glight = Bgra::new(80, 220, 90);
    // Trunk (slightly curved — offset cx by a pixel at different heights)
    for seg in 0..7i32 {
        let ty = by - seg * 6 - 6;
        let tx = cx + if seg < 3 { 0 } else { 1 };
        buf.fill_rect(tx - 3, ty, 7, 7, tdark);
        buf.fill_rect(tx - 2, ty, 5, 6, trunk);
        buf.fill_rect(tx - 1, ty + 1, 3, 2, ring);
    }
    // Crown fronds (6 fronds radiating outward)
    let top_y = by - 42;
    let top_x = cx + 1;
    // Left-droop frond
    for i in 0..12i32 {
        buf.fill_rect(top_x - i * 2 - 2, top_y + i, 5, 2, gdark);
        buf.fill_rect(top_x - i * 2 - 1, top_y + i, 3, 1, green);
    }
    // Right-droop frond
    for i in 0..12i32 {
        buf.fill_rect(top_x + i * 2 - 2, top_y + i, 5, 2, gdark);
        buf.fill_rect(top_x + i * 2 - 1, top_y + i, 3, 1, green);
    }
    // Upright fronds
    for i in 0..8i32 {
        buf.fill_rect(top_x - 2, top_y - i * 2, 5, 3, gdark);
        buf.fill_rect(top_x - 1, top_y - i * 2, 3, 2, if i < 4 { glight } else { green });
    }
    // Left-up frond
    for i in 0..8i32 {
        buf.fill_rect(top_x - i - 3, top_y - i * 2 - 1, 5, 2, gdark);
        buf.fill_rect(top_x - i - 2, top_y - i * 2 - 1, 3, 1, green);
    }
    // Right-up frond
    for i in 0..8i32 {
        buf.fill_rect(top_x + i - 1, top_y - i * 2 - 1, 5, 2, gdark);
        buf.fill_rect(top_x + i,     top_y - i * 2 - 1, 3, 1, green);
    }
}

fn draw_barrel(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let wood  = Bgra::new(155, 105, 48);
    let dark  = Bgra::new(80, 50, 18);
    let hoop  = Bgra::new(85, 80, 75);
    let hdark = Bgra::new(40, 38, 35);
    let hi    = Bgra::new(195, 150, 80);
    // Barrel body outline
    buf.fill_rect(cx - 6, by - 2,  13, 2, dark);
    buf.fill_rect(cx - 7, by - 4,  15, 4, dark);
    buf.fill_rect(cx - 7, by - 16, 15, 14, dark);
    buf.fill_rect(cx - 6, by - 18, 13, 3, dark);
    // Barrel body fill (bulges in middle)
    buf.fill_rect(cx - 5, by - 1,  11, 1, wood);
    buf.fill_rect(cx - 6, by - 3,  13, 1, wood);
    buf.fill_rect(cx - 6, by - 15, 13, 12, wood);
    buf.fill_rect(cx - 5, by - 17, 11, 2, wood);
    // Highlight (left side)
    buf.fill_rect(cx - 5, by - 15, 2, 12, hi);
    buf.fill_rect(cx - 4, by - 16, 1, 12, hi);
    // Hoops
    buf.fill_rect(cx - 7, by - 5,  15, 2, hdark);
    buf.fill_rect(cx - 6, by - 5,  13, 1, hoop);
    buf.fill_rect(cx - 7, by - 12, 15, 2, hdark);
    buf.fill_rect(cx - 6, by - 12, 13, 1, hoop);
    // Lid top
    buf.fill_rect(cx - 5, by - 17, 11, 2, hoop);
    buf.fill_rect(cx - 4, by - 17,  9, 1, Bgra::new(120, 115, 108));
}

fn draw_tent_shanty(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let canvas = Bgra::new(200, 180, 130);
    let dark   = Bgra::new(80, 65, 40);
    let shadow = Bgra::new(140, 120, 85);
    let pole   = Bgra::new(150, 110, 55);
    let pdark  = Bgra::new(80, 55, 22);
    let rope   = Bgra::new(180, 160, 100);
    // Left tent face (slanted)
    for i in 0..14i32 {
        let w = (14 - i) as u32;
        buf.fill_rect(cx - 14 + i, by - 2 - i, w + 1, 2, dark);
        if w > 1 { buf.fill_rect(cx - 13 + i, by - 2 - i, w - 1, 1, if i > 6 { shadow } else { canvas }); }
    }
    // Right tent face (slanted)
    for i in 0..14i32 {
        let w = (14 - i) as u32;
        buf.fill_rect(cx + 1, by - 2 - i, w + 1, 2, dark);
        if w > 1 { buf.fill_rect(cx + 2, by - 2 - i, w - 1, 1, if i > 6 { shadow } else { canvas }); }
    }
    // Center ridge pole
    buf.fill_rect(cx - 1, by - 18, 3, 18, pdark);
    buf.fill_rect(cx,     by - 18, 1, 17, pole);
    // Ground stakes + ropes (left side)
    buf.fill_rect(cx - 18, by - 2, 2, 2, pdark);
    for i in 0..5i32 { buf.set_pixel(cx - 17 + i, by - 3 - i, rope); }
    // Ground stakes + ropes (right side)
    buf.fill_rect(cx + 17, by - 2, 2, 2, pdark);
    for i in 0..5i32 { buf.set_pixel(cx + 17 - i, by - 3 - i, rope); }
    // Entry opening shadow
    buf.fill_rect(cx - 4, by - 8, 9, 8, dark);
    buf.fill_rect(cx - 3, by - 7, 7, 7, Bgra::new(30, 25, 20));
}

fn draw_anchor(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let metal = Bgra::new(75, 80, 88);
    let dark  = Bgra::new(35, 38, 45);
    let rust  = Bgra::new(130, 75, 40);
    // Ring at top
    buf.fill_rect(cx - 5, by - 28, 11, 2, dark);
    buf.fill_rect(cx - 6, by - 26, 13, 4, dark);
    buf.fill_rect(cx - 5, by - 25, 11, 2, metal);
    buf.fill_rect(cx - 3, by - 27, 7,  4, metal);
    buf.fill_rect(cx - 1, by - 27, 3,  2, dark); // center hole
    // Shaft
    buf.fill_rect(cx - 2, by - 22, 5, 14, dark);
    buf.fill_rect(cx - 1, by - 22, 3, 13, metal);
    // Stock (horizontal bar near top)
    buf.fill_rect(cx - 10, by - 21, 21, 3, dark);
    buf.fill_rect(cx - 9,  by - 20, 19, 1, metal);
    // Flukes (curved arms at bottom)
    buf.fill_rect(cx - 8, by - 10, 5, 5, dark);
    buf.fill_rect(cx - 7, by - 9,  4, 3, metal);
    buf.fill_rect(cx - 10, by - 8, 5, 3, dark);
    buf.fill_rect(cx - 9,  by - 7, 4, 2, rust);
    buf.fill_rect(cx + 4,  by - 10, 5, 5, dark);
    buf.fill_rect(cx + 4,  by - 9,  4, 3, metal);
    buf.fill_rect(cx + 6,  by - 8,  5, 3, dark);
    buf.fill_rect(cx + 6,  by - 7,  4, 2, rust);
    // Bottom crown
    buf.fill_rect(cx - 3, by - 8, 7, 3, dark);
    buf.fill_rect(cx - 2, by - 7, 5, 2, rust);
}

// ── Archetype 3: Underground (caverns) ────────────────────────────────────────

fn draw_underground(buf: &mut WorldBuffer, cx: i32, by: i32, sprite: u8) {
    match sprite {
        0 => draw_crystal_cluster(buf, cx, by),
        1 => draw_bone_pile(buf, cx, by),
        2 => draw_torch(buf, cx, by),
        _ => draw_skull(buf, cx, by),
    }
}

fn draw_crystal_cluster(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let crys  = Bgra::new(180, 80, 220);
    let cdark = Bgra::new(90, 30, 130);
    let clight= Bgra::new(230, 160, 255);
    let crys2 = Bgra::new(100, 140, 240);
    let c2dk  = Bgra::new(40, 60, 160);
    // Left crystal (teal)
    buf.fill_rect(cx - 14, by - 20, 5, 20, c2dk);
    buf.fill_rect(cx - 13, by - 20, 3, 18, crys2);
    buf.fill_rect(cx - 13, by - 22, 2, 3, c2dk);
    buf.fill_rect(cx - 12, by - 24, 1, 3, crys2);
    // Center crystal (purple, tallest)
    buf.fill_rect(cx - 4, by - 28, 9, 28, cdark);
    buf.fill_rect(cx - 3, by - 28, 7, 26, crys);
    buf.fill_rect(cx - 2, by - 26, 3, 20, clight);
    buf.fill_rect(cx - 3, by - 30, 5, 4, cdark);
    buf.fill_rect(cx - 2, by - 32, 3, 4, crys);
    buf.fill_rect(cx - 1, by - 33, 1, 2, clight);
    // Right crystal (purple, medium)
    buf.fill_rect(cx + 7, by - 22, 6, 22, cdark);
    buf.fill_rect(cx + 8, by - 22, 4, 20, crys);
    buf.fill_rect(cx + 8, by - 24, 3, 3, cdark);
    buf.fill_rect(cx + 9, by - 25, 1, 2, crys);
    // Small front shard
    buf.fill_rect(cx + 2, by - 12, 4, 12, cdark);
    buf.fill_rect(cx + 3, by - 11, 2, 10, clight);
    buf.fill_rect(cx + 3, by - 13, 2, 2, cdark);
}

fn draw_bone_pile(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let bone  = Bgra::new(230, 225, 200);
    let bdark = Bgra::new(155, 148, 120);
    let bhi   = Bgra::new(245, 242, 225);
    // Scattered bones arrangement
    // Long bone (diagonal-ish, left)
    buf.fill_rect(cx - 14, by - 5, 12, 3, bdark);
    buf.fill_rect(cx - 13, by - 5, 10, 2, bone);
    buf.fill_rect(cx - 16, by - 7, 4, 4, bdark);
    buf.fill_rect(cx - 15, by - 6, 2, 3, bone);
    buf.fill_rect(cx - 2,  by - 7, 4, 4, bdark);
    buf.fill_rect(cx - 1,  by - 6, 2, 3, bone);
    // Long bone (right, slightly raised)
    buf.fill_rect(cx + 3,  by - 6, 12, 3, bdark);
    buf.fill_rect(cx + 4,  by - 6, 10, 2, bone);
    buf.fill_rect(cx + 1,  by - 8, 4, 4, bdark);
    buf.fill_rect(cx + 2,  by - 7, 2, 3, bone);
    buf.fill_rect(cx + 14, by - 8, 4, 4, bdark);
    buf.fill_rect(cx + 15, by - 7, 2, 3, bhi);
    // Skull on top
    buf.fill_rect(cx - 5, by - 14, 11, 2, bdark);
    buf.fill_rect(cx - 6, by - 12, 13, 5, bdark);
    buf.fill_rect(cx - 5, by - 11, 11, 4, bone);
    buf.fill_rect(cx - 3, by - 13, 7,  2, bone);
    // Eye sockets
    buf.fill_rect(cx - 4, by - 11, 2, 2, bdark);
    buf.fill_rect(cx + 3, by - 11, 2, 2, bdark);
    // Jaw line
    buf.fill_rect(cx - 5, by - 8, 11, 1, bdark);
}

fn draw_torch(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let wood  = Bgra::new(140, 100, 48);
    let wdark = Bgra::new(75, 50, 18);
    let wrap  = Bgra::new(170, 130, 70);
    let fire1 = Bgra::new(50, 140, 250);  // outer flame (blue-white)
    let fire2 = Bgra::new(30, 200, 255);  // bright core
    let fire3 = Bgra::new(18, 80, 220);   // dark flame base
    let coal  = Bgra::new(50, 55, 60);
    // Handle
    buf.fill_rect(cx - 1, by - 18, 3, 14, wdark);
    buf.fill_rect(cx,     by - 18, 1, 13, wood);
    // Wrap bands
    for y in (by - 17..by - 5).step_by(4) {
        buf.fill_rect(cx - 2, y, 5, 2, wdark);
        buf.fill_rect(cx - 1, y, 3, 1, wrap);
    }
    // Coal/head
    buf.fill_rect(cx - 3, by - 22, 7, 4, coal);
    buf.fill_rect(cx - 2, by - 21, 5, 3, Bgra::new(70, 75, 80));
    // Flame (blue-white, cave torch style)
    buf.fill_rect(cx - 2, by - 28, 5, 6, fire3);
    buf.fill_rect(cx - 1, by - 30, 3, 4, fire1);
    buf.fill_rect(cx,     by - 32, 1, 3, fire2);
    buf.fill_rect(cx - 3, by - 26, 7, 2, fire3);
    buf.fill_rect(cx - 2, by - 26, 5, 1, fire1);
}

fn draw_skull(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let bone  = Bgra::new(220, 215, 195);
    let bdark = Bgra::new(130, 123, 105);
    let bhi   = Bgra::new(245, 240, 222);
    let dark  = Bgra::new(35, 30, 28);
    // Cranium outline
    buf.fill_rect(cx - 6, by - 14, 13, 2, bdark);
    buf.fill_rect(cx - 8, by - 12, 17, 6, bdark);
    buf.fill_rect(cx - 7, by - 6,  15, 2, bdark);
    // Cranium fill
    buf.fill_rect(cx - 5, by - 13, 11, 2, bone);
    buf.fill_rect(cx - 7, by - 11, 15, 5, bone);
    buf.fill_rect(cx - 6, by - 6,  13, 1, bone);
    // Cheekbones
    buf.fill_rect(cx - 8, by - 6,  4, 2, bdark);
    buf.fill_rect(cx + 5, by - 6,  4, 2, bdark);
    // Jaw / teeth area
    buf.fill_rect(cx - 5, by - 4,  11, 4, bdark);
    buf.fill_rect(cx - 4, by - 4,   9, 3, bone);
    // Tooth gaps
    for t in [-2i32, 0, 2, 4].iter() {
        buf.fill_rect(cx + t, by - 2, 1, 2, dark);
    }
    // Eye sockets
    buf.fill_rect(cx - 6, by - 11, 5, 4, dark);
    buf.fill_rect(cx + 2, by - 11, 5, 4, dark);
    buf.fill_rect(cx - 5, by - 12, 3, 1, dark);
    buf.fill_rect(cx + 3, by - 12, 3, 1, dark);
    // Nasal cavity
    buf.fill_rect(cx - 2, by - 8, 5, 3, dark);
    buf.fill_rect(cx - 1, by - 7, 3, 2, bdark);
    // Highlight on cranium
    buf.fill_rect(cx - 5, by - 12, 4, 3, bhi);
}

// ── Archetype 4: Arid (canyon/mesa) ──────────────────────────────────────────

fn draw_arid(buf: &mut WorldBuffer, cx: i32, by: i32, sprite: u8) {
    match sprite {
        0 => draw_cactus(buf, cx, by),
        1 => draw_bleached_skull(buf, cx, by),
        2 => draw_crumbling_pillar(buf, cx, by),
        _ => draw_tumbleweed(buf, cx, by),
    }
}

fn draw_cactus(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let green  = Bgra::new(40, 145, 50);
    let gdark  = Bgra::new(18, 85, 25);
    let glight = Bgra::new(75, 190, 80);
    let spine  = Bgra::new(230, 220, 195);
    // Main column
    buf.fill_rect(cx - 5, by - 36, 11, 36, gdark);
    buf.fill_rect(cx - 4, by - 36,  9, 34, green);
    buf.fill_rect(cx - 3, by - 35,  5, 30, glight);
    // Top dome
    buf.fill_rect(cx - 3, by - 38,  7, 3, gdark);
    buf.fill_rect(cx - 2, by - 38,  5, 2, green);
    // Left arm
    buf.fill_rect(cx - 14, by - 24, 10, 5, gdark);
    buf.fill_rect(cx - 13, by - 23,  8, 3, green);
    buf.fill_rect(cx - 14, by - 28,  6, 5, gdark);
    buf.fill_rect(cx - 13, by - 27,  4, 4, green);
    buf.fill_rect(cx - 13, by - 29,  4, 2, gdark);
    buf.fill_rect(cx - 12, by - 28,  2, 1, green);
    // Right arm
    buf.fill_rect(cx + 5,  by - 20, 10, 5, gdark);
    buf.fill_rect(cx + 6,  by - 19,  8, 3, green);
    buf.fill_rect(cx + 9,  by - 24,  6, 5, gdark);
    buf.fill_rect(cx + 10, by - 23,  4, 4, green);
    buf.fill_rect(cx + 10, by - 25,  4, 2, gdark);
    buf.fill_rect(cx + 11, by - 24,  2, 1, green);
    // Spines
    for (sx, sy) in [(-6,-30),(-6,-22),(-6,-14),(6,-28),(6,-20),(6,-12),(-15,-26),(10,-22)] {
        buf.set_pixel(cx + sx, by + sy, spine);
        buf.set_pixel(cx + sx - 1, by + sy, spine);
    }
}

fn draw_bleached_skull(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let bone  = Bgra::new(240, 232, 205);
    let bdark = Bgra::new(160, 150, 125);
    let sand  = Bgra::new(200, 185, 145);
    let dark  = Bgra::new(40, 36, 30);
    // Cranium outline
    buf.fill_rect(cx - 7, by - 16, 15, 2, bdark);
    buf.fill_rect(cx - 9, by - 14, 19, 7, bdark);
    buf.fill_rect(cx - 8, by - 7,  17, 2, bdark);
    // Cranium fill
    buf.fill_rect(cx - 6, by - 15, 13, 2, bone);
    buf.fill_rect(cx - 8, by - 13, 17, 6, bone);
    buf.fill_rect(cx - 7, by - 7,  15, 1, bone);
    // Highlight (bleached top)
    buf.fill_rect(cx - 5, by - 14, 6, 3, Bgra::new(252, 250, 235));
    // Sand fill at base
    buf.fill_rect(cx - 6, by - 5,  13, 3, sand);
    buf.fill_rect(cx - 5, by - 4,  11, 2, Bgra::new(210, 196, 158));
    // Eye sockets
    buf.fill_rect(cx - 7, by - 13, 6, 5, dark);
    buf.fill_rect(cx + 2, by - 13, 6, 5, dark);
    // Nasal
    buf.fill_rect(cx - 2, by - 9, 5, 3, dark);
    // Partially buried — just jaw line visible
    buf.fill_rect(cx - 7, by - 6, 15, 2, bdark);
    buf.fill_rect(cx - 6, by - 5, 13, 1, bone);
}

fn draw_crumbling_pillar(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let stone  = Bgra::new(165, 155, 140);
    let sdark  = Bgra::new(95, 88, 78);
    let shi    = Bgra::new(200, 192, 178);
    let crack  = Bgra::new(55, 50, 45);
    let rubble = Bgra::new(130, 122, 110);
    // Column body
    buf.fill_rect(cx - 6, by - 32, 13, 32, sdark);
    buf.fill_rect(cx - 5, by - 32, 11, 30, stone);
    buf.fill_rect(cx - 4, by - 31,  5, 28, shi);
    // Capital (top block)
    buf.fill_rect(cx - 8, by - 34, 17, 4, sdark);
    buf.fill_rect(cx - 7, by - 33, 15, 3, stone);
    buf.fill_rect(cx - 7, by - 33, 15, 1, shi);
    // Missing chunk (top-right broken off)
    buf.fill_rect(cx + 2, by - 34, 7, 6, Bgra::new(0, 0, 0)); // "air"
    // Rubble pieces at base
    buf.fill_rect(cx + 6,  by - 4,  6, 3, sdark);
    buf.fill_rect(cx + 7,  by - 3,  5, 2, rubble);
    buf.fill_rect(cx - 13, by - 3,  5, 3, sdark);
    buf.fill_rect(cx - 12, by - 2,  4, 2, rubble);
    buf.fill_rect(cx + 4,  by - 2,  4, 2, sdark);
    // Cracks in body
    buf.fill_rect(cx + 2,  by - 28,  1, 10, crack);
    buf.fill_rect(cx + 3,  by - 24,  1,  6, crack);
    buf.fill_rect(cx - 3,  by - 16,  1,  8, crack);
    buf.fill_rect(cx - 2,  by - 11,  1,  5, crack);
    // Horizontal band detail
    buf.fill_rect(cx - 5, by - 18, 11, 1, sdark);
    buf.fill_rect(cx - 5, by - 8,  11, 1, sdark);
}

fn draw_tumbleweed(buf: &mut WorldBuffer, cx: i32, by: i32) {
    let brown  = Bgra::new(155, 118, 62);
    let bdark  = Bgra::new(85, 60, 25);
    let tan    = Bgra::new(195, 165, 105);
    let dry    = Bgra::new(175, 145, 80);
    // Outer circle outline
    buf.fill_rect(cx - 7,  by - 18, 15, 2, bdark);
    buf.fill_rect(cx - 9,  by - 16, 19, 3, bdark);
    buf.fill_rect(cx - 10, by - 13, 21, 7, bdark);
    buf.fill_rect(cx - 9,  by - 6,  19, 3, bdark);
    buf.fill_rect(cx - 7,  by - 3,  15, 2, bdark);
    // Fill
    buf.fill_rect(cx - 6,  by - 17, 13, 2, brown);
    buf.fill_rect(cx - 8,  by - 15, 17, 3, brown);
    buf.fill_rect(cx - 9,  by - 12, 19, 6, brown);
    buf.fill_rect(cx - 8,  by - 6,  17, 3, brown);
    buf.fill_rect(cx - 6,  by - 3,  13, 2, brown);
    // Twig pattern inside (crossing lines)
    for i in -6i32..=6 {
        buf.set_pixel(cx + i, by - 10 + i / 2, bdark);
        buf.set_pixel(cx + i, by - 10 - i / 2, bdark);
    }
    buf.fill_rect(cx - 9, by - 10, 19, 1, bdark);
    buf.fill_rect(cx - 2, by - 17,  5, 14, bdark);
    // Highlight
    buf.fill_rect(cx - 6, by - 16, 5, 3, tan);
    buf.fill_rect(cx - 7, by - 13, 4, 4, tan);
    buf.fill_rect(cx - 5, by - 12, 5, 2, dry);
}
