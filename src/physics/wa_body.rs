//! WA physics-object movement (16.16 fixed point, 50 fps), ported from WA.exe.
//! `sweep` = FUN_004fbdc0 (swept integer-pixel mover), oracle-tested bit-exact vs the real machine code
//! (tests/wa_mover_oracle.rs, data from emu/gen_mover_oracle.py). `blocked(x16, y16)` stands in for FUN_004fb9d0
//! (body-vs-land/objects overlap at that 16.16 position; see wa_collide.rs).

/// Fields of a WA physics object used by the mover. Offsets are the WA object offsets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Body {
    pub x: i32,   // +0x84 (16.16)
    pub y: i32,   // +0x88
    pub vx: i32,  // +0x90
    pub vy: i32,  // +0x94
    pub f30: i32, // +0x30 object kind
    pub f38: i32, // +0x38 shape id
    pub fb0: i32, // +0xb0
}

#[inline] fn mul16(a: i32, b: i32) -> i32 { ((a as i64 * b as i64) >> 16) as i32 }
/// FUN_005b3501: (a << 16) / b, signed 64-bit.
#[inline] fn ratio(a: i32, b: i32) -> i32 { (((a as i64) << 16) / b as i64) as i32 }
#[inline] fn sat_add(a: i32, d: i32) -> i32 {
    let s = a.wrapping_add(d);
    if a < 0 && d < 0 && s > 0 { i32::MIN } else if a > 0 && d > 0 && s < 0 { i32::MAX } else { s }
}

/// Result of one mover call. `side`: 1 up, 2 right, 3 down, 4 left (valid when `hit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sweep { pub hit: bool, pub side: i32 }

/// FUN_004fbdc0. `frac` (16.16) is the fraction of this frame's movement still to apply; on a hit it is
/// reduced by the fraction consumed, on a clean move it becomes 0. `ver` = scheme/game version (`SCH+0xd778`).
pub fn sweep(b: &mut Body, frac: &mut i32, ver: i32, blocked: &dyn Fn(i32, i32) -> bool) -> Sweep {
    let dx = mul16(b.vx, *frac);
    let dy = mul16(b.vy, *frac);
    let (x0, y0) = (b.x, b.y);
    let (nx, ny) = if ver < 0x3c { (dx.wrapping_add(x0), dy.wrapping_add(y0)) } else { (sat_add(x0, dx), sat_add(y0, dy)) };

    let skip_sweep_flag = !(ver < 8 || b.fb0 == 0 || ((ver < 0x23 || b.f30 != 0xc) && b.f38 != 5 && b.f38 != 9));
    let pixel_moved = ((nx ^ x0) as u32 & 0xffff0000) != 0 || ((ny ^ y0) as u32 & 0xffff0000) != 0;
    if skip_sweep_flag || !pixel_moved {
        b.x = nx; b.y = ny; *frac = 0; // LAB_004fc1b5 path for the no-sweep case
        return Sweep { hit: false, side: 0 };
    }

    let ddx = nx.wrapping_sub(x0);
    let ddy = ny.wrapping_sub(y0);
    let (ady, adx) = (ddy.wrapping_abs(), ddx.wrapping_abs());
    let mut ratio_done = 0i32; // local_1c
    let consume = |frac: &mut i32, r: i32| { *frac = frac.wrapping_sub(mul16(*frac, r)); };

    if adx < ady {
        // y-dominant
        let step: i32 = if ny <= y0 { -0x10000 } else { 0x10000 };
        let mut cur_y = y0; // uVar6
        let mut x_next = x0; // uVar2
        loop {
            // inner do-while
            let (x_test, ty);
            loop {
                let ux = x_next; // uVar5
                if cur_y == ny { b.x = nx; b.y = ny; *frac = 0; return Sweep { hit: false, side: 0 }; }
                let rem = ny.wrapping_sub(cur_y);
                let local_14 = if rem.wrapping_abs() < 0x10000 { ny } else { (cur_y.wrapping_add(step)) & !0xffff };
                if blocked(ux, local_14) {
                    let (side, ny2) = if step < 0 { (1, cur_y & !0xffff) } else { (3, cur_y | 0xffff) };
                    consume(frac, ratio_done);
                    b.x = ux; b.y = ny2;
                    return Sweep { hit: true, side };
                }
                ratio_done = ratio(ady.wrapping_sub(ny.wrapping_sub(local_14).wrapping_abs()), ady);
                x_next = mul16(ddx, ratio_done).wrapping_add(x0);
                cur_y = local_14;
                if ((x_next ^ ux) as u32 & 0xffff0000) != 0 { x_test = ux; ty = local_14; break; }
            }
            if blocked(x_next, ty) {
                let (side, lx) = if nx < x0 { (4, x_test & !0xffff) } else { (2, x_test | 0xffff) };
                consume(frac, ratio_done);
                b.x = lx; b.y = ty;
                return Sweep { hit: true, side };
            }
            // second test missed: continue the outer loop (x_next becomes the new test x)
        }
    } else {
        // x-dominant
        let step: i32 = if nx <= x0 { -0x10000 } else { 0x10000 };
        let mut cur_x = x0;
        let mut cur_y = y0; // local_24
        if cur_x != nx {
            loop {
                let prev = cur_x;
                let rem = nx.wrapping_sub(cur_x);
                cur_x = if rem.wrapping_abs() < 0x10000 { nx } else { (cur_x.wrapping_add(step)) & !0xffff };
                if blocked(cur_x, cur_y) {
                    let (side, px) = if step < 0 { (4, prev & !0xffff) } else { (2, prev | 0xffff) };
                    consume(frac, ratio_done);
                    b.x = px; b.y = cur_y;
                    return Sweep { hit: true, side };
                }
                ratio_done = ratio(adx.wrapping_sub(nx.wrapping_sub(cur_x).wrapping_abs()), adx);
                let ty = mul16(ddy, ratio_done).wrapping_add(y0);
                if ((ty ^ cur_y) as u32 & 0xffff0000) != 0 && blocked(cur_x, ty) {
                    let (side, ly) = if ny < y0 { (1, cur_y & !0xffff) } else { (3, cur_y | 0xffff) };
                    consume(frac, ratio_done);
                    b.x = cur_x; b.y = ly;
                    return Sweep { hit: true, side };
                }
                cur_y = ty;
                if cur_x == nx { break; }
            }
        }
        b.x = nx; b.y = ny; *frac = 0;
        Sweep { hit: false, side: 0 }
    }
}
