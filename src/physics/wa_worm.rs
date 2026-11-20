//! WA worm body (vtable 0x6644c8, collision shape 3 = 9x16 mask) in the air, 16.16 fixed point at 50 frames/s.
//! Per frame (FUN_004fcdf0): if resting (v == 0 and the pixel below is blocked) nothing happens; otherwise gravity
//! (FUN_004fcb30: `vy += world gravity * obj+0x60`) and up to two swept moves (FUN_004fbdc0). A mover hit calls the worm's
//! hit handler (vtable+0x20 = FUN_0050d9a0 -> FUN_0050dc00 -> generic bounce FUN_004ffed0/FUN_004ff690/FUN_004ff490).
//! Verified against real WA jumps captured on the Wine rig (tests/wa_worm_air.rs): straight-up jump, forward jump with
//! a wall bounce, landings. Defaults below are the live values read from the rig (scheme version 0x1f4).
//! NOT yet ported: wind on worms (wind factor obj+0x5c is 0), falling state/fall damage, water, worm-vs-worm hits, walking.

use crate::physics::wa_body::{sweep, Body};
use crate::physics::wa_collide::{overlaps_body, overlaps_land};

pub const WORM_SHAPE: u8 = 3;
/// World gravity (WA scheme default), 16.16 px/frame^2.
pub const GRAVITY16: i32 = 15728;
/// Below this |vx|+|vy| a bounce kills the velocity (world+0x23c).
pub const STOP_SPEED: i32 = 26214;
/// Object bounce constants (obj+0x6c, +0x70, +0x74).
pub const REST_X: i32 = 0x9999;
pub const REST_Y: i32 = 0;
pub const FRICTION: i32 = 0xf5c2;
/// Jump impulses captured from WA (FUN_00500090 args): forward jump (4.5 px/frame at 60 degrees) and backflip (straight up).
pub const JUMP_FWD_VX: i32 = 147465;
pub const JUMP_FWD_VY: i32 = -255384;
pub const JUMP_BACKFLIP_VX: i32 = 0;
pub const JUMP_BACKFLIP_VY: i32 = -294912;
/// Backflip impulses when the key is pressed a second time during the windup (WA FUN_00515xxx via the 0x5756b0 trig table at
/// angle 0x3d70 / 0x38e3): (vx magnitude, vy). vx is directed AWAY from the facing side.
pub const JUMP_BACKFLIP2_BACKSPACE: (i32, i32) = (18531, -294322);
pub const JUMP_BACKFLIP2_ENTER: (i32, i32) = (51223, -290425);
const SCHEME_VER: i32 = 0x200;

#[inline] fn mul16(a: i32, b: i32) -> i32 { ((a as i64 * b as i64) >> 16) as i32 }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WormBody { pub x: i32, pub y: i32, pub vx: i32, pub vy: i32 }

/// FUN_004ff490's step-up branch is only taken when obj+0x40 != 0; in flight WA has it 0 (rig: the probes never ran),
/// so airborne wall hits are plain bounces. Kept for completeness / other states.
const CLIMB: bool = false;

/// What the worm did this frame (the caller drives the worm state machine from it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AirFrame {
    /// Worm touched down / stopped (WA state 0x7a): velocity is zero, position snapped.
    pub landed: bool,
    pub hits: u8,
    /// Index into `others` of a worm this one just bumped from below (WA launches it: state 0x80, vx=0, vy = our vy).
    pub launched: Option<(usize, i32)>,
}

impl WormBody {
    /// Worm is resting on something solid: no velocity and the pixel row below overlaps land.
    pub fn resting(&self, solid: &dyn Fn(i32, i32) -> bool) -> bool { self.resting_with(solid, &[]) }

    fn resting_with(&self, solid: &dyn Fn(i32, i32) -> bool, others: &[(i32, i32)]) -> bool {
        self.vx == 0 && self.vy == 0 && Self::blocked_with(solid, others, self.x, self.y.wrapping_add(0x10000))
    }

    /// WA's "supported" test (cf. FUN_004fb580): land or another worm overlaps this body shifted 1 px down, velocity ignored.
    pub fn supported_with(&self, solid: &dyn Fn(i32, i32) -> bool, others: &[(i32, i32)]) -> bool {
        Self::blocked_with(solid, others, self.x, self.y.wrapping_add(0x10000))
    }

    /// The worm half of WA's "supported" test (cf. FUN_004fb580): does another worm's mask overlap this body shifted 1 px down?
    /// `others` are their integer body-centre pixels. Lets a worm resting on another's shoulder or head count as supported even
    /// where a rectangular "feet at the head" test disagrees with the real mask shape.
    pub fn resting_on_worm(&self, others: &[(i32, i32)]) -> bool {
        others.iter().any(|&(ox, oy)| overlaps_body(WORM_SHAPE, self.x >> 16, self.y.wrapping_add(0x10000) >> 16, WORM_SHAPE, ox, oy))
    }

    fn blocked_at(solid: &dyn Fn(i32, i32) -> bool, x16: i32, y16: i32) -> bool {
        overlaps_land(solid, WORM_SHAPE, x16 >> 16, y16 >> 16)
    }

    /// Land OR another worm (`others` = their integer pixel positions). Land is tested first, as in WA's category order.
    fn blocked_with(solid: &dyn Fn(i32, i32) -> bool, others: &[(i32, i32)], x16: i32, y16: i32) -> bool {
        Self::blocked_at(solid, x16, y16)
            || others.iter().any(|&(ox, oy)| overlaps_body(WORM_SHAPE, x16 >> 16, y16 >> 16, WORM_SHAPE, ox, oy))
    }

    /// Generic bounce (FUN_004ff490 with obj+0x40 != 0, as for worms) for a hit on `side` (1 up, 2 right, 3 down, 4 left).
    fn bounce(&mut self, side: i32, solid: &dyn Fn(i32, i32) -> bool) {
        match side {
            1 | 3 => {
                self.vx = mul16(self.vx, FRICTION);
                self.vy = -mul16(self.vy, REST_Y);
            }
            _ if CLIMB => {
                // side 2/4: try to hop up / down one pixel diagonally instead of bouncing.
                let dx = if self.vx >= 0 { 0x10000 } else { -0x10000 };
                let mut dy = -0x10000;
                loop {
                    if !Self::blocked_at(solid, self.x.wrapping_add(dx), self.y.wrapping_add(dy)) {
                        self.x = self.x.wrapping_add(dx);
                        self.y = self.y.wrapping_add(dy);
                        let f = FRICTION * 30;
                        let f = (f + ((f >> 31) & 0x1f)) >> 5;
                        self.vx = mul16(self.vx, f);
                        return;
                    }
                    if dy >= 0 { break; }
                    dy = 0x10000;
                }
                self.vx = -mul16(self.vx, REST_X);
                self.vy = mul16(self.vy, FRICTION);
            }
            _ => {
                self.vx = -mul16(self.vx, REST_X);
                self.vy = mul16(self.vy, FRICTION);
            }
        }
    }

    /// Hit handler for the airborne jump state (WA state 0x79). Returns true when the stop rule fired (landed).
    fn on_hit_jump(&mut self, side: i32, solid: &dyn Fn(i32, i32) -> bool, others: &[(i32, i32)], launched: &mut Option<(usize, i32)>) -> bool {
        let below_blocked = Self::blocked_with(solid, others, self.x, self.y.wrapping_add(0x10000));
        // Hit object type: the blocker is a worm (type 2) when land does not overlap the tried position.
        let (tx, ty) = match side { 1 => (self.x, self.y.wrapping_sub(0x10000)), 2 => (self.x.wrapping_add(0x10000), self.y), 3 => (self.x, self.y.wrapping_add(0x10000)), _ => (self.x.wrapping_sub(0x10000), self.y) };
        let hit_worm = !Self::blocked_at(solid, tx, ty) && Self::blocked_with(solid, others, tx, ty);
        if side == 1 && hit_worm {
            // Rising into a worm from below (rig capture): if it is more than 8 px above and within a 45 degree cone,
            // it is launched with our vertical speed and we fly on unimpeded.
            let (tx16, ty16) = (self.x, self.y.wrapping_sub(0x10000));
            if let Some(i) = others.iter().position(|&(ox, oy)| overlaps_body(WORM_SHAPE, tx16 >> 16, ty16 >> 16, WORM_SHAPE, ox, oy)) {
                let (ox, oy) = others[i];
                let (dx, dy) = ((self.x >> 16) - ox, (self.y >> 16) - oy);
                if dy > 8 && dx.abs() < dy { *launched = Some((i, self.vy)); return false; }
            }
        }
        match side {
            3 => { // landing (scheme d970 != 0: only if the pixel below is really blocked)
                if below_blocked { self.vx = 0; self.vy = 0; return true; }
            }
            1 if !hit_worm => { // ceiling vs land: WA stops dead (vs a worm it bounces)
                self.vx = 0; self.vy = 0; return true;
            }
            1 => {}
            _ => { // wall: touching ground while not rising fast stops the worm
                if self.vy > -0x10000 && below_blocked { self.vx = 0; self.vy = 0; return true; }
            }
        }
        self.bounce(side, solid);
        if (self.vx.wrapping_abs()).wrapping_add(self.vy.wrapping_abs()) < STOP_SPEED { self.vx = 0; self.vy = 0; }
        false
    }

    /// One 50 fps WA frame of airborne physics (jump state).
    pub fn step_air(&mut self, solid: &dyn Fn(i32, i32) -> bool) -> AirFrame { self.step_air_with(solid, &[]) }

    /// As `step_air`, with other worms (integer pixel positions) acting as solid bodies.
    pub fn step_air_with(&mut self, solid: &dyn Fn(i32, i32) -> bool, others: &[(i32, i32)]) -> AirFrame {
        let mut out = AirFrame::default();
        let mut launched = None;
        if self.resting_with(solid, others) { return out; }
        self.vy = self.vy.wrapping_add(GRAVITY16);
        let ign: std::cell::Cell<Option<usize>> = std::cell::Cell::new(None);
        let blocked = |x16: i32, y16: i32| {
            Self::blocked_at(solid, x16, y16)
                || others.iter().enumerate().any(|(i, &(ox, oy))| ign.get() != Some(i)
                    && overlaps_body(WORM_SHAPE, x16 >> 16, y16 >> 16, WORM_SHAPE, ox, oy))
        };
        let mut frac = 0x10000;
        let mut count = 0;
        loop {
            if frac == 0 { break; }
            let mut b = Body { x: self.x, y: self.y, vx: self.vx, vy: self.vy, f38: WORM_SHAPE as i32, ..Default::default() };
            let r = sweep(&mut b, &mut frac, SCHEME_VER, &blocked);
            self.x = b.x; self.y = b.y;
            if !r.hit { count += 1; } else {
                out.hits += 1;
                if self.on_hit_jump(r.side, solid, others, &mut launched) { out.landed = true; }
                if launched.is_some() { out.launched = launched; ign.set(launched.map(|l| l.0)); }
                count += 1; // the handler returns 1 for worms vs land
            }
            if count >= 2 { break; }
        }
        out
    }

    /// FUN_00500100 + FUN_004fe070: one walking move of `dx` (16.16) with up/down terrain following (8 px).
    /// Returns 1 = moved (maybe stepped up/down), 0 = blocked (no move), -1 = moved horizontally but ground is
    /// gone (no support within 8 px: caller switches to the falling state).
    pub fn walk_move(&mut self, dx: i32, solid: &dyn Fn(i32, i32) -> bool) -> i32 {
        let nx = self.x.wrapping_add(dx);
        let mut y = self.y;
        if Self::blocked_at(solid, nx, y) {
            for _ in 0..8 {
                y -= 0x10000;
                if !Self::blocked_at(solid, nx, y) { self.x = nx; self.y = y; return 1; }
            }
            return 0;
        }
        for _ in 0..8 {
            if Self::blocked_at(solid, nx, y + 0x10000) { self.x = nx; self.y = y; return 1; }
            y += 0x10000;
        }
        self.x = nx;
        -1
    }
}

/// px/tick (30 Hz, f32) <-> 16.16 px/frame (50 Hz): one tick is 5/3 of a WA frame.
pub fn vel_to_16(v: f32) -> i32 { (v as f64 * 65536.0 * 3.0 / 5.0).round() as i32 }
pub fn vel_from_16(v: i32) -> f32 { (v as f64 / 65536.0 * 5.0 / 3.0) as f32 }
/// Arty soldier foot position <-> WA body centre (mask rows run y-9..=y+6, so foot row = y+6).
pub const FOOT_OFFSET: i32 = 6;
pub fn body_from_foot(px: f32, py: f32, vx: i32, vy: i32) -> WormBody {
    WormBody { x: (px as f64 * 65536.0) as i32, y: ((py.floor() as i32 - FOOT_OFFSET) << 16) | 0xffff, vx, vy }
}
pub fn foot_of(b: &WormBody) -> (f32, f32) { (b.x as f32 / 65536.0, ((b.y >> 16) + FOOT_OFFSET) as f32) }

/// Walk animation offsets (px, 16.16) by phase index — table at WA 0x6a9518 (15 entries), cumulative per 9 px stride.
pub const WALK_TABLE: [i32; 15] = [0x8000, 0x10000, 0x18000, 0x20000, 0x38000, 0x54000, 0x6c000, 0x80000, 0x84000, 0x88000,
    0x8c000, 0x90000, 0x90000, 0x90000, 0x90000];
/// Phase advance per 50 Hz frame while walking (obj+0x1f0 += 0xf5c).
pub const WALK_PHASE_STEP: u32 = 0xf5c;

/// Walk state: WA positions the worm at `anchor + dir * offset(phase)`; phase starts at 0 when a walk starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkState { pub anchor: i32, pub dir: i32, pub phase: u32, pub frame: u32 }

impl WalkState {
    pub fn start(x: i32, dir: i32) -> Self { WalkState { anchor: x, dir, phase: 0, frame: 0 } }
    /// Target x for the current frame, then advance the phase (the offset uses the phase *before* the advance).
    fn target_and_advance(&mut self) -> i32 {
        self.frame += 1;
        // Start-up quirk seen in every captured walk: on the 6th frame the phase does not advance and the
        // anchor moves half a pixel forward (the walk animation takes over from the start-up animation).
        let quirk = self.frame == 6;
        if quirk { self.anchor = self.anchor.wrapping_add(self.dir * 0x8000); self.phase -= WALK_PHASE_STEP; }
        let idx = (((self.phase & 0xffff) as u64 * 15) >> 16) as usize;
        let strides = (self.phase >> 16) as i32;
        let t = self.anchor.wrapping_add(self.dir * (strides * WALK_TABLE[14] + WALK_TABLE[idx]));
        self.phase += WALK_PHASE_STEP;
        t
    }
}

impl WormBody {
    /// One 50 fps walking frame (WA state 0x66): returns FUN_00500100's result (see `walk_move`).
    pub fn walk_frame(&mut self, w: &mut WalkState, solid: &dyn Fn(i32, i32) -> bool) -> i32 {
        let target = w.target_and_advance();
        self.walk_move(target.wrapping_sub(self.x), solid)
    }
}

/// Scheme "fall damage" setting (byte at scheme+0xd926; live WA value 50) and world damage multiplier (+0x5f0).
pub const FALL_DAMAGE_SETTING: i64 = 50;
pub const DAMAGE_MULTIPLIER: i32 = 1;
/// A flight faster than this (8 px/frame) puts WA's worm in its damaging fall state (0x80).
pub const FALL_DAMAGE_SPEED: i32 = 0x7ffff;

/// HP lost when a worm hits the ground at downward speed `vy` (16.16 px/frame, speed *after* that frame's gravity), from
/// FUN_0050dc00 (scheme version >= 0xe0): `p = (setting * (vy - 0x7ffff - gravity)) >> 16; hp = p / 18 + 1`.
/// Hits against other worms never hurt.
pub fn fall_damage(vy: i32) -> i32 {
    if vy <= FALL_DAMAGE_SPEED { return 0; }
    let d = vy as i64 - 0x7ffff - GRAVITY16 as i64;
    let p = (FALL_DAMAGE_SETTING * d) >> 16;
    let units = (p / 18) as i32 + if p < 0 { -1 } else { 0 } + 1;
    if units > 0 { units * DAMAGE_MULTIPLIER } else { 0 }
}

#[cfg(test)]
mod fall_tests {
    use super::*;
    #[test]
    fn fall_damage_curve() {
        assert_eq!(fall_damage(0x7ffff), 0);
        assert_eq!(fall_damage(0x80000 + GRAVITY16), 1);       // just over the threshold: 1 HP
        assert_eq!(fall_damage(12 << 16), 11);                  // 12 px/frame ~ a 300 px drop
        assert!(fall_damage(20 << 16) > fall_damage(12 << 16));
        // Real WA rig captures (state 0x80 forced at a chosen speed): impact vy (after gravity) -> HP lost.
        // Teleport drops on a mine-free column (wa_rig fall_damage c88_*, 2026-11-19): vy after gravity at impact -> HP lost,
        // falls of 146/152/160/180/200/220/240 px. (136 px lands at 8.16 px/frame -> 0.)
        for (vy, hp) in [(550_480, 1), (566_208, 2), (581_936, 2), (613_392, 4), (644_848, 5), (676_304, 6), (707_760, 8)] {
            assert_eq!(fall_damage(vy), hp, "drop vy {vy}");
        }
        for (vy, hp) in [(605_552, 3), (736_624, 9), (867_696, 14), (1_129_840, 26)] {
            assert_eq!(fall_damage(vy), hp, "vy {vy}");
        }
    }
}

/// WA drowns a worm (state 0x85) once its *centre* y pixel reaches the water level (FUN_004fcdf0: `water <= (short)(y>>16)`,
/// scheme ver >= 0xe0; rig: a worm teleported far below the water line entered 0x85 on the very next frame and sank at ~1.2 px/frame).
/// `water_row` is the water level in the same coordinates as `body.y`.
pub fn drowned(body: &WormBody, water_row: i32) -> bool { (body.y >> 16) >= water_row }
