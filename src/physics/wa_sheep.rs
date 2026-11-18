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
pub const HOP_ANGLE_DEG: f64 = 25.0;
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
    pub fn hop(&mut self, angle_deg: f64, percent: i32) {
        self.rng = self.frame.wrapping_add(self.rng).wrapping_mul(0x19660d).wrapping_add(0x3c6ef35f);
        let m = (((self.rng & 0xffff) as i64 + 0x28000) * percent as i64 * 2 / 100) as f64;
        let a = angle_deg.to_radians();
        self.vx = (a.cos() * m).round() as i32 * self.dir;
        self.vy = -(a.sin() * m).round() as i32;
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

    /// FUN_0050a900 (walking mode).
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
        if (self.y >> 16) >= water_row { jump = true; }
        // rec+0xc4 > 0: steep change, or an obstacle within 30/60 px ahead.
        if (self.y - y0).abs() > 0x40000 { jump = true; }
        let ahead = |px: i32| Self::blocked(solid, self.x.wrapping_add(self.dir * px * 0x10000), self.y);
        if ahead(PROBE_FAR) || ahead(PROBE_NEAR) { jump = true; }
        // Embedded in terrain right here: never hop.
        if Self::blocked(solid, self.x, self.y) { jump = false; }
        if jump {
            self.hop(HOP_ANGLE_DEG, HOP_PERCENT);
        }
    }

    /// Ballistic flight. Gravity, then move one pixel at a time so thin walls can't be tunnelled.
    fn air(&mut self, solid: &dyn Fn(i32, i32) -> bool, out: &mut Frame) {
        self.vy = self.vy.wrapping_add(GRAVITY16);
        // Horizontal.
        let mut rem = self.vx as i64;
        let sx = if rem < 0 { -1 } else { 1 };
        let mut x = self.x as i64;
        while rem != 0 {
            let step = if rem.abs() > 0x10000 { 0x10000 * sx } else { rem };
            let nx = (x + step) as i32;
            if Self::blocked(solid, nx, self.y) { self.vx = 0; break; }
            x += step;
            rem -= step;
        }
        self.x = x as i32;
        // Vertical.
        let mut rem = self.vy as i64;
        let sy = if rem < 0 { -1 } else { 1 };
        let mut y = self.y as i64;
        while rem != 0 {
            let step = if rem.abs() > 0x10000 { 0x10000 * sy } else { rem };
            let ny = (y + step) as i32;
            if Self::blocked(solid, self.x, ny) {
                if self.vy > 0 { out.landed = true; }
                self.vy = 0;
                if out.landed { self.vx = 0; }
                break;
            }
            y += step;
            rem -= step;
        }
        self.y = y as i32;
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
        // speed in [5.0, 7.0] px/frame at 25 degrees
        let mag = ((s.vx as f64).powi(2) + (s.vy as f64).powi(2)).sqrt() / 65536.0;
        assert!(mag > 4.9 && mag < 7.1, "hop speed {mag}");
        assert!(s.vx > 0 && s.vy < 0);
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
