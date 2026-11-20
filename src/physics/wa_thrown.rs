//! WA thrown weapon (class 2/5 projectile, vtable 0x664438: grenade, cluster, ...) in flight, 16.16 fixed point at 50 frames/s.
//! Per frame (FUN_004fcdf0): resting bodies (v == 0 and land one pixel below) do nothing; otherwise gravity, then up to two swept
//! moves (`wa_body::sweep`, oracle-verified); a hit runs the generic bounce FUN_004ff490 + the stop rule FUN_004ff690.
//! Verified bit-exact against a real WA grenade throw (tests/wa_grenade_flight.rs): free flight, floor bounce, rolling decay, stop.
//! Constants are the live values read from the rig for the grenade with "MIN bounce" under the Intermediate scheme.

use super::wa_body::{sweep, Body};
use super::wa_collide::{overlaps_body, overlaps_land};

/// WA shape id of a standing/walking worm (9x16), what a thrown weapon tests against (rig-verified: a grenade bounces off worm bodies).
pub const WORM_SHAPE: u8 = 3;

/// 7x5 box (rig: the shell/grenade is tested with collision shape 2).
pub const SHAPE: u8 = 2;
pub const GRAVITY16: i32 = 15728;
/// Below this |vx|+|vy| a bounce kills all velocity (world+0x23c).
pub const STOP_SPEED: i32 = 26214;
/// Wall restitution obj+0x6c (fixed, ctor FUN_00507d10).
pub const REST_WALL: i32 = 0x9999;
/// Slide factor obj+0x74 = scheme[0xd780] (0xf5c2) * rec+0xb0 (100) / 100.
pub const SLIDE: i32 = 0xf5c2;
/// Floor restitution obj+0x70 from the launch struct: 30 % ("MIN bounce"), 60 % otherwise.
pub const FLOOR_REST_MIN: i32 = 19660;
pub const FLOOR_REST_MAX: i32 = 39321;
/// Fuse countdown per 50 Hz frame (ms).
pub const FUSE_STEP_MS: i32 = 20;
const SCHEME_VER: i32 = 0x200;

#[inline] fn mul16(a: i32, b: i32) -> i32 { ((a as i64 * b as i64) >> 16) as i32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thrown {
    pub x: i32, pub y: i32, pub vx: i32, pub vy: i32,
    pub fuse_ms: i32,
    /// obj+0x70 (floor restitution, 16.16).
    pub floor_rest: i32,
    /// Wind factor applied to world wind (rec+0x7c %), gravity factor (rec+0x84 %).
    pub wind_pct: i32, pub gravity_pct: i32,
    /// Worms (index into the `others` slice) the weapon is still inside of since launch: ignored until it has left them once
    /// (rig: a grenade thrown from a worm's centre is not blocked by it, but bounces off it when it comes back).
    pub exempt: u32,
    pub started: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Frame { pub hits: u8, pub exploded: bool }

impl Thrown {
    pub fn grenade(x: i32, y: i32, vx: i32, vy: i32, fuse_ms: i32, max_bounce: bool) -> Self {
        Self { x, y, vx, vy, fuse_ms, floor_rest: if max_bounce { FLOOR_REST_MAX } else { FLOOR_REST_MIN }, wind_pct: 0, gravity_pct: 100, exempt: 0, started: false }
    }

    fn blocked(solid: &dyn Fn(i32, i32) -> bool, x16: i32, y16: i32) -> bool { overlaps_land(solid, SHAPE, x16 >> 16, y16 >> 16) }

    pub fn resting(&self, solid: &dyn Fn(i32, i32) -> bool) -> bool {
        self.vx == 0 && self.vy == 0 && Self::blocked(solid, self.x, self.y.wrapping_add(0x10000))
    }

    /// Generic bounce FUN_004ff490 (obj+0x40 == 0 path) then the stop rule FUN_004ff690.
    fn bounce(&mut self, side: i32) {
        match side {
            1 | 3 => { self.vx = mul16(self.vx, SLIDE); self.vy = -mul16(self.vy, self.floor_rest); }
            _ => { self.vx = -mul16(self.vx, REST_WALL); self.vy = mul16(self.vy, SLIDE); }
        }
        if self.vx.wrapping_abs().wrapping_add(self.vy.wrapping_abs()) < STOP_SPEED { self.vx = 0; self.vy = 0; }
    }

    /// One 50 Hz frame. `wind16` = world wind (16.16 per frame). The fuse counts down every frame, resting or not.
    pub fn step(&mut self, solid: &dyn Fn(i32, i32) -> bool, wind16: i32) -> Frame { self.step_with(solid, wind16, &[]) }

    /// `others` = integer pixel centres of worm bodies (WA tests the weapon's box against their masks as well as the land).
    pub fn step_with(&mut self, solid: &dyn Fn(i32, i32) -> bool, wind16: i32, others: &[(i32, i32)]) -> Frame {
        let mut out = Frame::default();
        let touching = |x16: i32, y16: i32, i: usize| overlaps_body(SHAPE, x16 >> 16, y16 >> 16, WORM_SHAPE, others[i].0, others[i].1);
        if !self.started {
            self.started = true;
            for i in 0..others.len().min(32) { if touching(self.x, self.y, i) { self.exempt |= 1 << i; } }
        }
        let exempt = self.exempt;
        let body_blocked = |x16: i32, y16: i32| (0..others.len().min(32)).any(|i| exempt & (1 << i) == 0 && touching(x16, y16, i));
        if !(self.vx == 0 && self.vy == 0 && (Self::blocked(solid, self.x, self.y.wrapping_add(0x10000)) || body_blocked(self.x, self.y.wrapping_add(0x10000)))) {
            self.vy = self.vy.wrapping_add(mul16(GRAVITY16, self.gravity_pct * 0x10000 / 100));
            self.vx = self.vx.wrapping_add(mul16(wind16, self.wind_pct * 0x10000 / 100));
            let blocked = |x16: i32, y16: i32| Self::blocked(solid, x16, y16) || body_blocked(x16, y16);
            let mut frac = 0x10000;
            let mut count = 0;
            while frac != 0 && count < 2 {
                let mut b = Body { x: self.x, y: self.y, vx: self.vx, vy: self.vy, f30: 0xb, f38: SHAPE as i32, fb0: 0 };
                let r = sweep(&mut b, &mut frac, SCHEME_VER, &blocked);
                self.x = b.x; self.y = b.y;
                if r.hit { out.hits += 1; self.bounce(r.side); }
                count += 1;
            }
        }
        for i in 0..others.len().min(32) { if self.exempt & (1 << i) != 0 && !touching(self.x, self.y, i) { self.exempt &= !(1 << i); } }
        self.fuse_ms -= FUSE_STEP_MS;
        if self.fuse_ms < 1 { out.exploded = true; }
        out
    }
}

/// Charge (power) per 50 Hz frame while fire is held: worm+0x248 += 873 (= 65536/75), clamped to 0x10000.
pub const CHARGE_STEP: i32 = 873;
pub const CHARGE_MAX: i32 = 0x10000;
/// Launch speed = power * 0x18 (FUN_0051c3d0), so a full charge is 24.0 px/frame.
pub const POWER_TO_SPEED: i32 = 0x18;

/// Power after `frames` frames of holding fire (rig-captured: 4/6/8/26/36 frames and the clamp at 75).
pub fn charge_after(frames: u32) -> i32 { (frames as i64 * CHARGE_STEP as i64).min(CHARGE_MAX as i64) as i32 }
/// Launch speed (16.16 px/frame) for a power value.
pub fn launch_speed(power: i32) -> i32 { power * POWER_TO_SPEED }

#[cfg(test)]
mod charge_tests {
    use super::*;
    #[test]
    fn charge_curve_matches_the_rig() {
        for (frames, speed) in [(4, 83808), (6, 125712), (8, 167616), (26, 544752), (36, 754272)] {
            assert_eq!(launch_speed(charge_after(frames)), speed, "{frames} frames");
        }
        assert_eq!(charge_after(75), 65475);
        assert_eq!(charge_after(76), CHARGE_MAX);
        assert_eq!(launch_speed(CHARGE_MAX), 1_572_864); // 24.0 px/frame, captured
    }
}

#[cfg(test)]
mod worm_body_tests {
    use super::*;

    /// Rig capture (tools/wa_rig grenade_flight, aim ~straight up from a worm at (732,608)): the grenade leaves the thrower, falls back
    /// onto its head and rests there at y = worm_y - 11 (597.0 vs 608). Without worm bodies it would drop straight through.
    #[test]
    fn grenade_lands_on_the_thrower_like_wa() {
        let worms = [(732, 608)];
        let mut g = Thrown::grenade((732 << 16) | 0x8000, 608 << 16, 0, -691_405, 3000, false); // vy = -10.55 px/frame (16.16)
        let open = |_x: i32, _y: i32| false;
        for _ in 0..140 { g.step_with(&open, 0, &worms); }
        assert_eq!((g.vx, g.vy), (0, 0), "stopped on the worm");
        assert!((596..=597).contains(&(g.y >> 16)), "rests on the worm's head (capture: 597.0), got {}", g.y >> 16);
        let mut free = Thrown::grenade((732 << 16) | 0x8000, 608 << 16, 0, -691_405, 3000, false);
        for _ in 0..140 { free.step(&open, 0); }
        assert!(free.y >> 16 > 700, "no worm: falls through ({})", free.y >> 16);
    }
}
