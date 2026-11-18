//! WA object-vs-land pixel collision (findings/terrain_collision.md, verified live on the Wine rig:
//! 312 real WA land tests reproduced with 0 mismatches, see tests/wa_collide_oracle.rs).
//!
//! WA treats land as an object holding a 1920x696 1-bpp bitmap (1 = solid). A body is a small 1-bpp mask
//! (`BodyShape`); it collides iff any set mask pixel lies on a solid land pixel. The mask's top-left corner is
//! at (x - half_w, y - half_h) where (x, y) is the integer pixel position (16.16 >> 16). Row bit i = column i.

/// A WA body mask. `rows[r]` bit `c` (LSB = leftmost) = pixel (c, r) of the mask.
#[derive(Debug, Clone, Copy)]
pub struct BodyShape {
    pub id: u8,
    pub half_w: i32,
    pub half_h: i32,
    pub width: i32,
    pub rows: &'static [u32],
}

/// Shape ids as stored in WA's shape table (T[id]); id 1 is the land itself.
pub const SHAPE_WORM: u8 = 9;

const R2: [u32; 5] = [0x7f, 0x7f, 0x7f, 0x7f, 0x7f];
const R3: [u32; 16] = [0xfe, 0xfe, 0xfe, 0xfe, 0x1ff, 0x1ff, 0x1ff, 0x1ff, 0x1ff, 0x1ff, 0xfe, 0xfe, 0xfe, 0x7c, 0x7c, 0x7c];
const R4: [u32; 18] = [0x1fc0, 0x7ff0, 0xfff8, 0x1fffc, 0x3fffe, 0x3fffe, 0x7ffff, 0x7ffff, 0x7ffff, 0x7ffff, 0x7ffff, 0x7ffff, 0x7ffff, 0x3fffe, 0x3fffe, 0x1fffc, 0xfff8, 0x7ff0];
const R5: [u32; 29] = [0x7ffffe, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0x7ffffe];
const R6: [u32; 1] = [0x0];
const R7: [u32; 19] = [0x1, 0xffffffff, 0xffffffff, 0xffffffff, 0x1f, 0xffffffff, 0xffffffff, 0xffffffff, 0x1f, 0xffffffff, 0xffffffff, 0xffffffff, 0x1f, 0xffffffff, 0xffffffff, 0xffffffff, 0x1f, 0xffffffff, 0xffffffff];
const R9: [u32; 31] = [0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0xfffff, 0x7fffe, 0x3fffc, 0x1fff8, 0xfff0, 0x7fe0, 0x3fc0, 0x0, 0x0, 0x0, 0x0];
const R10: [u32; 20] = [0x0, 0x0, 0x600, 0xf00, 0xf00, 0xf00, 0x600, 0x600, 0x600, 0x600, 0x600, 0x600, 0xf00, 0xf00, 0xf00, 0x1f80, 0x1f80, 0xf00, 0x0, 0x0];
const R11: [u32; 20] = [0x0, 0x0, 0x800, 0x1c00, 0x1e00, 0x1e00, 0xc00, 0xc00, 0x600, 0x600, 0x600, 0x700, 0x700, 0x780, 0xf80, 0xfc0, 0x7c0, 0x780, 0x0, 0x0];

pub const SHAPES: [BodyShape; 9] = [
    BodyShape { id: 2, half_w: 3, half_h: 2, width: 7, rows: &R2 },
    BodyShape { id: 3, half_w: 4, half_h: 9, width: 9, rows: &R3 },
    BodyShape { id: 4, half_w: 9, half_h: 9, width: 19, rows: &R4 },
    BodyShape { id: 5, half_w: 12, half_h: 19, width: 24, rows: &R5 },
    BodyShape { id: 6, half_w: 0, half_h: 0, width: 1, rows: &R6 },
    BodyShape { id: 7, half_w: 0, half_h: 0, width: 1, rows: &R7 },
    BodyShape { id: 9, half_w: 10, half_h: 15, width: 20, rows: &R9 },
    BodyShape { id: 10, half_w: 10, half_h: 10, width: 20, rows: &R10 },
    BodyShape { id: 11, half_w: 10, half_h: 10, width: 20, rows: &R11 },
];

/// Look up a body shape by WA shape id (2..=11).
pub fn shape(id: u8) -> Option<&'static BodyShape> {
    SHAPES.iter().find(|s| s.id == id)
}

/// WA's body-vs-land overlap test. `solid(x, y)` is the land bit (out-of-bounds must return false).
pub fn overlaps_land(solid: &dyn Fn(i32, i32) -> bool, shape_id: u8, x: i32, y: i32) -> bool {
    let Some(s) = shape(shape_id) else { return false };
    let (ox, oy) = (x - s.half_w, y - s.half_h);
    for (r, row) in s.rows.iter().enumerate() {
        let mut bits = *row;
        while bits != 0 {
            let c = bits.trailing_zeros() as i32;
            if solid(ox + c, oy + r as i32) { return true; }
            bits &= bits - 1;
        }
    }
    false
}

/// Body-vs-body overlap (WA's shape vtable+0x10 against another object): the two 1-bpp masks, each placed at
/// (pos - half extents), share a set pixel. `x,y` / `ox,oy` are integer pixel positions.
pub fn overlaps_body(shape_id: u8, x: i32, y: i32, other_id: u8, ox: i32, oy: i32) -> bool {
    let (Some(a), Some(b)) = (shape(shape_id), shape(other_id)) else { return false };
    let (ax, ay) = (x - a.half_w, y - a.half_h);
    let (bx, by) = (ox - b.half_w, oy - b.half_h);
    for (r, row) in a.rows.iter().enumerate() {
        let gy = ay + r as i32;
        let br = gy - by;
        if br < 0 || br as usize >= b.rows.len() { continue; }
        let mut bits = *row;
        while bits != 0 {
            let c = bits.trailing_zeros() as i32;
            let bc = ax + c - bx;
            if (0..32).contains(&bc) && (b.rows[br as usize] >> bc) & 1 == 1 { return true; }
            bits &= bits - 1;
        }
    }
    false
}
