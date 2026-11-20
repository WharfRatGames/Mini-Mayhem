//! WA Sheep driver (weapon record 23), ported from the decompile of FUN_0050a900 (walker), FUN_0050a330 (hop) and
//! FUN_00500100 (stepping mover); see findings/physics.md "Sheep driver". 16.16 fixed point, 50 Hz frames.
//! Statically decoded — NOT yet verified against a live WA capture. Body shape is a guess (small shape 2).

use super::wa_collide::overlaps_land;

/// Collision mask used for the sheep (WA shape id 2, 7x5). UNVERIFIED guess: the record's shape field wasn't recovered.
pub const SHAPE: u8 = 2;
/// Half mask height: the lowest mask row is `y + HALF_H` for centre `y`.
pub const HALF_H: i32 = 2;
/// World gravity per frame (same as every other WA body).
pub const GRAVITY16: i32 = 15728;
/// Walk step = rec+0xb0 (100) * 0x20000 / 100 = 2.0 px per 50 Hz frame.
pub const WALK_DX: i32 = 0x20000;
/// Step-up / step-down height, rec+0xb4.
pub const STEP_PX: i32 = 4;
/// Hop angle (degrees) rec+0xc4 and speed percent rec+0xc8.
pub const HOP_ANGLE_DEG: i32 = 25;
pub const HOP_PERCENT: i32 = 100;
/// Look-ahead probes (px) for obstacles.
pub const PROBE_FAR: i32 = 60;
pub const PROBE_NEAR: i32 = 30;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SheepBody {
    pub x: i32,
    pub y: i32,
    pub vx: i32,
    pub vy: i32,
    /// +1 right, -1 left (obj[0xf2]).
    pub dir: i32,
    /// WA world[0x45ec] random state.
    pub rng: u32,
    /// WA frame counter (world+0x5cc) used to perturb the RNG.
    pub frame: u32,
    /// True on the frame a hop starts (for the sound).
    pub hopped: bool,
}

pub struct Frame { pub hopped: bool, pub turned: bool, pub landed: bool }

impl SheepBody {
    pub fn new(x: i32, y: i32, dir: i32, rng_seed: u32) -> Self {
        Self { x, y, vx: 0, vy: 0, dir, rng: rng_seed, frame: 0, hopped: false }
    }

    fn blocked(solid: &dyn Fn(i32, i32) -> bool, x: i32, y: i32) -> bool {
        overlaps_land(solid, SHAPE, x >> 16, y >> 16)
    }

    /// FUN_004fb580: unsupported (nothing solid directly below).
    pub fn airborne(&self, solid: &dyn Fn(i32, i32) -> bool) -> bool {
        !Self::blocked(solid, self.x, self.y.wrapping_add(0x10000))
    }

    /// FUN_00500100: move `dx`, stepping up (blocked) or down (free) at most `step` pixels.
    /// 1 = moved, 0 = blocked, -1 = moved horizontally but no ground within `step` px.
    pub fn step_move(&mut self, dx: i32, step: i32, solid: &dyn Fn(i32, i32) -> bool) -> i32 {
        let nx = self.x.wrapping_add(dx);
        let mut y = self.y;
        if Self::blocked(solid, nx, y) {
            for _ in 0..step {
                y -= 0x10000;
                if !Self::blocked(solid, nx, y) { self.x = nx; self.y = y; return 1; }
            }
            return 0;
        }
        for _ in 0..step {
            if Self::blocked(solid, nx, y + 0x10000) { self.x = nx; self.y = y; return 1; }
            y += 0x10000;
        }
        self.x = nx;
        -1
    }

    /// FUN_0050a330: random leap. Speed `((r&0xffff)+0x28000) * percent * 2 / 100`, direction `angle` above horizontal.
    /// The angle is measured FROM VERTICAL (oracle-verified against WA's real code): vx = sin * m, vy = -cos * m, with WA's
    /// interpolated trig tables. A 25 degree "hop angle" is therefore a steep, mostly upward leap.
    pub fn hop(&mut self, angle_deg: i32, percent: i32) {
        use super::wa_rope::{cos16, sin16};
        self.rng = self.frame.wrapping_add(self.rng).wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        let m = (((self.rng & 0xffff) as i64 + 0x28000) * percent as i64 * 2 / 100) as i64;
        let a = angle_deg.wrapping_mul(0x10000) / 0x168;
        self.vx = (((sin16(a) as i64 * m) >> 16) as i32).wrapping_mul(self.dir);
        self.vy = -(((cos16(a) as i64 * m) >> 16) as i32);
        self.hopped = true;
    }

    /// One 50 Hz frame. `water_row`: pixel row at/below which the sheep counts as off the bottom (hop flag, like WA's world-bottom test).
    pub fn frame(&mut self, solid: &dyn Fn(i32, i32) -> bool, water_row: i32) -> Frame {
        self.frame = self.frame.wrapping_add(1);
        self.hopped = false;
        let mut out = Frame { hopped: false, turned: false, landed: false };
        if self.airborne(solid) {
            self.air(solid, &mut out);
        } else {
            self.vx = 0;
            self.vy = 0;
            self.walk(solid, water_row, &mut out);
        }
        out.hopped = self.hopped;
        out
    }

    /// FUN_0050a900 (walking mode), ported from the instructions (oracle-verified, tests/wa_sheep_oracle.rs).
    fn walk(&mut self, solid: &dyn Fn(i32, i32) -> bool, water_row: i32, out: &mut Frame) {
        let y0 = self.y;
        let r = self.step_move(WALK_DX * self.dir, STEP_PX, solid);
        let mut jump = false;
        if r == 0 {
            self.dir = -self.dir; // blocked: turn around and hop
            out.turned = true;
            jump = true;
        } else if r == -1 {
            jump = true; // walked off a ledge (rec+0xc4 >= 0)
        }
        // Reached the water row: clamp to the row above it and hop.
        if (self.y >> 16) >= water_row {
            self.y = (water_row << 16).wrapping_sub(0x10000);
            jump = true;
        }
        // rec+0xc4 > 0 (sheep): steep change, or an obstacle ahead. WA: 0x3ffff < |dy|.
        if (self.y - y0).abs() >= 0x40000 { jump = true; }
        let free_at = |dx_px: i32, dy_px: i32| !Self::blocked(solid, self.x.wrapping_add(self.dir * dx_px * 0x10000), self.y.wrapping_add(dy_px * 0x10000));
        // 60 px ahead probed 8 px ABOVE the sheep, then (if clear) 30 px ahead at his own height.
        let mut check_ceiling = true;
        if !free_at(PROBE_FAR, -8) {
            jump = true;
        } else if !free_at(PROBE_NEAR, 0) {
            jump = true;
        } else if !jump {
            check_ceiling = false; // nothing ahead and no other reason: skip straight to the end
        }
        // A hop is cancelled when there is no room above: the spot 48 px straight up is blocked.
        if check_ceiling && Self::blocked(solid, self.x, self.y.wrapping_sub(0x300000)) {
            jump = false;
        }
        if jump {
            self.hop(HOP_ANGLE_DEG, HOP_PERCENT);
        }
    }

    /// Ballistic flight on WA's real mover (`wa_body::sweep`, oracle-verified): gravity, then up to two sweep+bounce rounds
    /// (FUN_004fcdf0). The sheep is weapon category 3, so FUN_004ff490 bounces with obj[0x6c]=0x9999 (walls), obj[0x70]=0 and
    /// obj[0x74]=0 (floor/ceiling): a floor or ceiling hit kills all velocity, a wall hit reflects vx at 0.6 and zeroes vy.
    fn air(&mut self, solid: &dyn Fn(i32, i32) -> bool, out: &mut Frame) {
        use super::wa_body::{sweep, Body};
        use super::wa_collide::overlaps_land;
        self.vy = self.vy.wrapping_add(GRAVITY16);
        let blocked = |x16: i32, y16: i32| overlaps_land(solid, SHAPE, x16 >> 16, y16 >> 16);
        let mut b = Body { x: self.x, y: self.y, vx: self.vx, vy: self.vy, f30: 0xb, f38: SHAPE as i32, fb0: 0 };
        let mut frac = 0x10000;
        let mut rounds = 0;
        while frac != 0 && rounds < 2 {
            let r = sweep(&mut b, &mut frac, 0x200, &blocked);
            if r.hit && blocked(b.x, b.y.wrapping_add(0x10000)) {
                // FUN_00508c90, category 3: touching ground one pixel below (FUN_005018f0 == 0) kills all velocity, whatever the side.
                b.vx = 0; b.vy = 0; out.landed = true;
            } else if r.hit {
                let vx_before = b.vx;
                // FUN_00508c90 0x508f09: category 3 draws the RNG after every bounce; one time in 16 a WALL hit (sides 2/4) turns the sheep
                // to face the way it was travelling.
                self.rng = self.frame.wrapping_add(self.rng).wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
                if self.rng & 0xf000_0000 == 0 && (r.side == 2 || r.side == 4) { self.dir = if vx_before >= 0 { 1 } else { -1 }; }
                match r.side {
                    1 | 3 => { if r.side == 3 { out.landed = true; } b.vx = 0; b.vy = 0; }
                    _ => { b.vx = -(((b.vx as i64 * 0x9999) >> 16) as i32); b.vy = 0; }
                }
            }
            rounds += 1;
        }
        self.x = b.x; self.y = b.y; self.vx = b.vx; self.vy = b.vy;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(y_ground: i32) -> impl Fn(i32, i32) -> bool { move |_x, y| y >= y_ground }

    fn settled(x: i32, ground: i32) -> SheepBody {
        let mut s = SheepBody::new(x << 16, (ground - 3) << 16, 1, 1);
        let solid = flat(ground);
        for _ in 0..5 { s.frame(&solid, 10_000); }
        s
    }

    #[test]
    fn walks_two_px_per_frame_on_flat_ground() {
        let solid = flat(100);
        let mut s = settled(50, 100);
        let x0 = s.x;
        for _ in 0..10 { s.frame(&solid, 10_000); }
        assert_eq!(s.x - x0, 10 * WALK_DX);
        assert!(!s.hopped);
    }

    #[test]
    fn hops_when_a_wall_is_ahead_and_obeys_the_hop_formula() {
        // Wall 40 px ahead rising 30 px: the 60 px probe sees it.
        let solid = |x: i32, y: i32| y >= 100 || (x >= 90 && y >= 70);
        let mut s = settled(50, 100);
        let mut hopped = false;
        for _ in 0..3 { if s.frame(&solid, 10_000).hopped { hopped = true; break; } }
        assert!(hopped, "no hop with wall within 60px");
        // speed in [5.0, 7.0] px/frame, 25 degrees from vertical (steep)
        let mag = ((s.vx as f64).powi(2) + (s.vy as f64).powi(2)).sqrt() / 65536.0;
        assert!(mag > 4.9 && mag < 7.1, "hop speed {mag}");
        assert!(s.vx > 0 && s.vy < 0 && -s.vy > s.vx, "hop should be steeper than 45 degrees");
    }

    #[test]
    fn blocked_turns_around() {
        let solid = |x: i32, y: i32| y >= 100 || (x >= 60 && y >= 40);
        let mut s = settled(55, 100);
        let mut turned = false;
        for _ in 0..40 { if s.frame(&solid, 10_000).turned { turned = true; break; } }
        assert!(turned);
        assert_eq!(s.dir, -1);
    }

    #[test]
    fn lands_and_resumes_walking() {
        let solid = flat(100);
        let mut s = SheepBody::new(50 << 16, 80 << 16, 1, 1);
        for _ in 0..60 { s.frame(&solid, 10_000); }
        assert_eq!(s.vy, 0);
        assert!(!s.airborne(&solid));
    }
}
