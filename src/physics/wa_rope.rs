//! Port of Worms Armageddon's ninja rope (decompiled from WA.exe; see
//! `findings/rope.md` in the decomp workspace).
//!
//! The rope is a chain of up to 64 fixed 8 px segments whose directions are
//! stored as bend angles; wraps happen only on that 8 px lattice. Each frame
//! the roper's velocity is projected onto the tangent (the radial part is
//! discarded in both directions), damped, and the roper moves along the arc in
//! ~1 px sub-steps at constant radius around the current pivot.
//!
//! All arithmetic is 16.16 fixed point / integer, so it is deterministic on
//! every target. One Arty tick stands in for one WA frame: the per-frame
//! constants below are pre-scaled by `WA_TIME_SCALE` (50 fps -> 30 Hz).

pub const SEG: i32 = 0x80000; // 8 px
pub const MIN_LEN: i32 = 0xA0000; // 10 px
pub const MAX_SEGS: usize = 64;
pub const MAX_LEN: i32 = (MAX_SEGS as i32) * SEG; // 512 px

/// Velocity retention per tick: 0xfc28/65536 (0.98547 per 50 fps frame) ^ (50/30).
pub const TICK_DAMP: i32 = 63904;
/// World gravity 0.24 px/frame^2 (x S^2) — applied to the roper's vy.
pub const TICK_GRAVITY: i32 = 43691;
/// Swing impulse 0x1999 (0.1 px/frame^2, x S^2), horizontal AND +vy while a direction is held.
pub const TICK_SWING: i32 = 18203;
/// Reel speed: 4 px/frame x S.
pub const TICK_REEL: i32 = 436907;

static TRIG: &[u8] = include_bytes!("wa_trig.bin"); // 1025 sin then 1025 cos, i32 LE

#[inline]
fn tab(off: usize, i: usize) -> i32 {
    let p = (off + i) * 4;
    i32::from_le_bytes([TRIG[p], TRIG[p + 1], TRIG[p + 2], TRIG[p + 3]])
}
#[inline]
fn interp(off: usize, a: i32) -> i32 {
    let i = ((a >> 6) & 0x3ff) as usize;
    let f = ((a & 0x3f) << 10) as i64;
    let t0 = tab(off, i);
    let t1 = tab(off, i + 1);
    ((((t1 - t0) as i64 * f) >> 16) as i32).wrapping_add(t0)
}
/// sin of a 16-bit-turn angle, 16.16.
#[inline]
pub fn sin16(a: i32) -> i32 { interp(0, a) }
/// cos of a 16-bit-turn angle, 16.16.
#[inline]
pub fn cos16(a: i32) -> i32 { interp(1025, a) }
#[inline]
fn mul(a: i32, b: i32) -> i32 { ((a as i64 * b as i64) >> 16) as i32 }
#[inline]
fn fdiv(a: i32, b: i32) -> i32 { (((a as i64) << 16) / b as i64) as i32 }

/// World queries the rope needs.
pub trait Env {
    /// Is the terrain pixel at (x, y) solid?
    fn solid_px(&self, x: i32, y: i32) -> bool;
    /// Would the roper's body collide at this 16.16 rope-point position?
    fn blocked(&self, x16: i32, y16: i32) -> bool;
}

#[derive(Clone, Debug)]
pub struct Chain {
    /// Fixed anchor (16.16).
    pub ax: i32,
    pub ay: i32,
    /// Total rope length (16.16).
    pub len: i32,
    /// Per-segment {a: wrap marker, b: bend angle delta}.
    pub tbl: [(i32, i32); MAX_SEGS],
    pub nseg: usize,
}

struct Geom {
    xs: [i32; MAX_SEGS + 1],
    ys: [i32; MAX_SEGS + 1],
    ang: [i32; MAX_SEGS],
    theta: i32,
    free: i32,
    piv: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    /// Anchor or a wrap pivot is no longer solid (terrain destroyed): drop the rope.
    Released,
}

enum Sw {
    Done,
    Event,
    Blocked,
}

impl Chain {
    /// FUN_005008d0. `theta` = direction (16-bit turns) of segment 0 from the anchor toward the roper.
    pub fn attach(ax: i32, ay: i32, len: i32, theta: i32) -> Option<Chain> {
        if len < MIN_LEN || len > MAX_LEN {
            return None;
        }
        let nseg = ((len >> 19) + 1) as usize;
        if nseg > MAX_SEGS {
            return None;
        }
        let mut c = Chain { ax, ay, len, tbl: [(0, 0); MAX_SEGS], nseg };
        c.tbl[0].1 = theta;
        Some(c)
    }

    /// Direction (16-bit turns, 0 = up, clockwise) such that
    /// `anchor + len*(sin, -cos)` points along `(dx, dy)`. Integer-only.
    pub fn angle_of(dx: i32, dy: i32) -> i32 {
        let d = ((dx as i64 * dx as i64 + dy as i64 * dy as i64) as u64).isqrt() as i64;
        if d == 0 {
            return 0x8000;
        }
        let ux = ((dx as i64) << 16) / d;
        let uy = ((dy as i64) << 16) / d;
        let err = |a: i32| -> i64 {
            let ex = sin16(a) as i64 - ux;
            let ey = -(cos16(a) as i64) - uy;
            ex * ex + ey * ey
        };
        let (mut best, mut be) = (0i32, i64::MAX);
        let mut a = 0;
        while a < 0x10000 {
            let e = err(a);
            if e < be { be = e; best = a; }
            a += 64;
        }
        let (lo, hi) = (best - 64, best + 64);
        for a in lo..=hi {
            let e = err(a & 0xffff);
            if e < be { be = e; best = a & 0xffff; }
        }
        best
    }

    /// FUN_004fd360.
    fn geom(&self) -> Geom {
        let mut g = Geom { xs: [0; MAX_SEGS + 1], ys: [0; MAX_SEGS + 1], ang: [0; MAX_SEGS], theta: 0, free: 0, piv: 0 };
        let (mut x, mut y, mut ang) = (self.ax, self.ay, 0i32);
        for j in 0..self.nseg {
            ang = ang.wrapping_add(self.tbl[j].1);
            g.xs[j] = x;
            g.ys[j] = y;
            g.ang[j] = ang;
            x = x.wrapping_add(sin16(ang).wrapping_mul(8));
            y = y.wrapping_sub(cos16(ang).wrapping_mul(8));
        }
        g.xs[self.nseg] = x;
        g.ys[self.nseg] = y;
        g.theta = ang;
        g.free = self.len & 0x7ffff;
        g.piv = self.nseg - 1;
        while g.piv > 0 && self.tbl[g.piv - 1].0 == 0 {
            g.piv -= 1;
            g.free += SEG;
        }
        g
    }

    /// Current free swing radius (px) and the rope polyline's wrap pivots
    /// (anchor -> roper order, 16.16), for drawing / sync.
    pub fn pivots(&self) -> (f32, Vec<(i32, i32)>) {
        let g = self.geom();
        let mut v = Vec::new();
        for j in 0..g.piv {
            if self.tbl[j].0 != 0 {
                v.push((g.xs[j + 1], g.ys[j + 1]));
            }
        }
        (g.free as f32 / 65536.0, v)
    }

    /// FUN_004fd4c0: the anchor and every wrap pivot must still be solid.
    fn valid(&self, g: &Geom, env: &dyn Env) -> bool {
        if !env.solid_px(self.ax >> 16, self.ay >> 16) {
            return false;
        }
        let (mut last, mut cum) = (0usize, 0i32);
        for j in 0..g.piv {
            cum = cum.wrapping_add(self.tbl[j].1);
            if self.tbl[j].0 != 0 {
                let a2 = self.tbl[j].0.wrapping_add(cum);
                let k = (j - last + 1) as i32;
                let px = g.xs[last].wrapping_add(sin16(a2).wrapping_mul(k).wrapping_mul(8));
                let py = g.ys[last].wrapping_sub(cos16(a2).wrapping_mul(k).wrapping_mul(8));
                if !env.solid_px(px >> 16, py >> 16) {
                    return false;
                }
                last = j + 1;
            }
        }
        true
    }

    /// One tick of rope physics: damping, tangential arc motion, wrap/unwrap.
    /// `pos`/`vel` are the roper's rope-point position / velocity (16.16).
    /// Gravity and swing impulses are applied by the caller before this.
    pub fn step(&mut self, pos: &mut (i32, i32), vel: &mut (i32, i32), max_speed: i32, env: &dyn Env) -> Outcome {
        self.step_with(pos, vel, max_speed, env, TICK_DAMP)
    }

    /// `step` with an explicit damping factor (16.16 retention per step).
    pub fn step_with(&mut self, pos: &mut (i32, i32), vel: &mut (i32, i32), max_speed: i32, env: &dyn Env, damp: i32) -> Outcome {
        let g0 = self.geom();
        if !self.valid(&g0, env) {
            return Outcome::Released;
        }
        vel.0 = mul(vel.0, damp);
        vel.1 = mul(vel.1, damp);
        let mut time = 0x10000i32;
        let mut factor = 0x10000i32;
        for _ in 0..10 {
            let g = self.geom();
            match self.swing(&g, pos, vel, &mut time, &mut factor, max_speed, env) {
                Sw::Done | Sw::Blocked => break,
                Sw::Event => {}
            }
            if time == 0 || factor == 0 {
                break;
            }
        }
        Outcome::Ok
    }

    /// FUN_004fd640.
    #[allow(clippy::too_many_arguments)]
    fn swing(&mut self, g: &Geom, pos: &mut (i32, i32), vel: &mut (i32, i32), time: &mut i32,
             factor: &mut i32, max_speed: i32, env: &dyn Env) -> Sw {
        if g.free <= 0x10000 {
            *time = 0;
            return Sw::Done;
        }
        let th = g.theta;
        let (s0, c0) = (sin16(th), cos16(th));
        let mut vt = mul(vel.1, s0).wrapping_add(mul(vel.0, c0));
        if vt == 0 {
            *vel = (0, 0);
            *time = 0;
            return Sw::Done;
        }
        if max_speed != 0 {
            vt = vt.clamp(-max_speed, max_speed);
        }
        let sgn = if vt >= 0 { 1 } else { -1 };
        let denom = mul(g.free, 0x6487e);
        let dth = fdiv(vt, denom).clamp(-0x1999, 0x1999);
        let n = (vt.abs() >> 16) + 1;
        let local14 = factor.wrapping_mul(n);
        let m = (local14 + 0x8000) >> 16;
        if m < 1 {
            *time = 0;
            return Sw::Done;
        }
        let piv = g.piv;
        let mut local20 = 0x10000i32;
        let mut last_frac = 0i32;
        let mut k = 1i32;
        let remaining = |k: i32| -> i32 { (local14 - k * 0x10000 + 0x10000) / (n - k + 1) };
        loop {
            let frac = local20 / n;
            let ang_k = mul(dth, frac).wrapping_add(th);
            let (sk, ck) = (sin16(ang_k), cos16(ang_k));
            // Unwrap: crossing the previous segment's direction against the wrap sense.
            if piv != 0 && (self.tbl[piv - 1].0 < 0) != (sgn < 0) {
                let p = g.ang[piv - 1];
                let cross = if th > p { ang_k < p } else if th < p { ang_k > p } else { ang_k != p };
                if cross {
                    let ty = g.ys[piv].wrapping_sub(mul(cos16(p), g.free));
                    let tx = mul(sin16(p), g.free).wrapping_add(g.xs[piv]);
                    if !env.blocked(tx, ty) {
                        self.tbl[piv].1 = 0;
                        self.tbl[piv - 1].0 = 0;
                        *factor = remaining(k);
                        return Sw::Event;
                    }
                }
            }
            // Wrap: a joint on the 8 px lattice (along the new direction) landed in terrain.
            let (mut x, mut y) = (g.xs[piv], g.ys[piv]);
            let mut j = piv;
            while j + 1 < self.nseg {
                y = y.wrapping_sub(ck.wrapping_mul(8));
                x = x.wrapping_add(sk.wrapping_mul(8));
                if env.solid_px(x >> 16, y >> 16) {
                    let prev = if piv == 0 { 0 } else { g.ang[piv - 1] };
                    let mut a = ang_k.wrapping_sub(self.tbl[piv].1).wrapping_sub(prev);
                    if a == 0 {
                        a = sgn * 0x10000;
                    }
                    self.tbl[j].0 = a;
                    *time -= mul(*time, last_frac);
                    *factor = remaining(k);
                    return Sw::Event;
                }
                j += 1;
            }
            // Move along the arc at constant radius around the pivot.
            let nx = mul(sk, g.free).wrapping_add(g.xs[piv]);
            let ny = g.ys[piv].wrapping_sub(mul(ck, g.free));
            if env.blocked(nx, ny) {
                *time -= mul(*time, last_frac);
                *factor = remaining(k);
                return Sw::Blocked;
            }
            if piv == 0 {
                self.tbl[0].1 = ang_k;
            } else {
                self.tbl[piv].1 = ang_k.wrapping_sub(g.ang[piv - 1]);
            }
            local20 += 0x10000;
            *vel = (mul(ck, vt), mul(sk, vt));
            *pos = (nx, ny);
            k += 1;
            if m < k {
                *time = 0;
                return Sw::Done;
            }
            last_frac = frac;
        }
    }

    /// Position of the roper if the rope were `len` long, measured along the chain (FUN_00500d00).
    fn pos_at(&self, len: i32, nseg_used: usize, partial_seg: usize) -> (i32, i32) {
        let (mut x, mut y, mut ang) = (self.ax, self.ay, 0i32);
        for j in 0..nseg_used {
            ang = ang.wrapping_add(self.tbl[j].1);
            x = x.wrapping_add(sin16(ang).wrapping_mul(8));
            y = y.wrapping_sub(cos16(ang).wrapping_mul(8));
        }
        ang = ang.wrapping_add(self.tbl[partial_seg].1);
        let d = len.wrapping_sub(nseg_used as i32 * SEG);
        (x.wrapping_add(mul(sin16(ang), d)), y.wrapping_sub(mul(cos16(ang), d)))
    }

    /// FUN_00500d00: change the rope length (reel). Moves the roper along the chain
    /// when the destination is free, and scales velocity by old/new length.
    pub fn set_length(&mut self, new_len: i32, pos: &mut (i32, i32), vel: &mut (i32, i32), env: &dyn Env) -> bool {
        if new_len < MIN_LEN || new_len > MAX_LEN {
            return false;
        }
        let seg_new = (new_len >> 19) as usize;
        let n3 = seg_new + 1;
        if new_len < self.len {
            let p = self.pos_at(new_len, seg_new, seg_new.min(MAX_SEGS - 1));
            if env.blocked(p.0, p.1) {
                return false;
            }
            *pos = p;
            self.nseg = n3;
        } else {
            if n3 > MAX_SEGS {
                return false;
            }
            let old = self.nseg;
            let used = old.saturating_sub(1);
            let p = self.pos_at(new_len, used, used);
            if env.blocked(p.0, p.1) {
                return false;
            }
            *pos = p;
            for j in old..n3 {
                self.tbl[j] = (0, 0);
            }
            self.nseg = n3;
        }
        let ratio = fdiv(self.len, new_len);
        vel.0 = mul(vel.0, ratio);
        vel.1 = mul(vel.1, ratio);
        self.len = new_len;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Open;
    impl Env for Open {
        fn solid_px(&self, x: i32, y: i32) -> bool { x == 500 && y == 100 } // only the anchor pixel
        fn blocked(&self, _: i32, _: i32) -> bool { false }
    }

    fn setup(len_px: i32, ang: i32) -> (Chain, (i32, i32), (i32, i32)) {
        let c = Chain::attach(500 << 16, 100 << 16, len_px << 16, ang).unwrap();
        let g = c.geom();
        let pos = (mul(sin16(g.theta), c.len).wrapping_add(c.ax), c.ay.wrapping_sub(mul(cos16(g.theta), c.len)));
        (c, pos, (0, 0))
    }

    #[test]
    fn trig_matches_float() {
        for a in [0, 1000, 0x4000, 0x8000, 0xC123, 0xffff] {
            let f = (a as f64) / 65536.0 * std::f64::consts::TAU;
            assert!((sin16(a) as f64 / 65536.0 - f.sin()).abs() < 2e-4);
            assert!((cos16(a) as f64 / 65536.0 - f.cos()).abs() < 2e-4);
        }
    }

    #[test]
    fn angle_of_roundtrip() {
        for (dx, dy) in [(0, 100 << 16), (100 << 16, 0), (-70 << 16, -70 << 16), (30 << 16, 90 << 16)] {
            let a = Chain::angle_of(dx, dy);
            let d = (((dx as f64).powi(2) + (dy as f64).powi(2)).sqrt()) as i32;
            let (px, py) = (mul(sin16(a), d), -mul(cos16(a), d));
            assert!((px - dx).abs() < 3 << 16 >> 4 && (py - dy).abs() < 3 << 16 >> 4, "{dx} {dy} -> {a}: {px} {py}");
        }
    }

    #[test]
    fn rope_length_is_constant_while_swinging() {
        let (mut c, mut pos, mut vel) = setup(120, Chain::angle_of(80 << 16, 90 << 16));
        for _ in 0..400 {
            vel.1 += TICK_GRAVITY;
            assert_eq!(c.step(&mut pos, &mut vel, 60 << 16, &Open), Outcome::Ok);
            let (dx, dy) = ((pos.0 - c.ax) as f64 / 65536.0, (pos.1 - c.ay) as f64 / 65536.0);
            let r = (dx * dx + dy * dy).sqrt();
            assert!((r - 120.0).abs() < 0.6, "radius drifted to {r}");
        }
    }

    #[test]
    fn swing_oscillates_and_damps() {
        let (mut c, mut pos, mut vel) = setup(120, Chain::angle_of(90 << 16, 80 << 16));
        let mut max_x_first = i32::MIN;
        let mut max_x_late = i32::MIN;
        for t in 0..600 {
            vel.1 += TICK_GRAVITY;
            c.step(&mut pos, &mut vel, 60 << 16, &Open);
            if t < 150 { max_x_first = max_x_first.max(pos.0 - c.ax); }
            if t >= 450 { max_x_late = max_x_late.max(pos.0 - c.ax); }
        }
        assert!(max_x_first > 0 && max_x_late > 0, "it should keep swinging past the anchor");
        assert!(max_x_late < max_x_first, "damping must shrink the swing: {max_x_first} -> {max_x_late}");
    }

    #[test]
    fn reel_in_shortens_and_speeds_up() {
        let (mut c, mut pos, mut vel) = setup(150, Chain::angle_of(60 << 16, 130 << 16));
        vel.0 = 2 << 16;
        let v0 = vel.0;
        assert!(c.set_length(c.len - TICK_REEL, &mut pos, &mut vel, &Open));
        assert!(vel.0 > v0, "shorter rope => faster (angular momentum)");
        assert!(c.len < 150 << 16);
        assert!(!c.set_length(MIN_LEN - 1, &mut pos, &mut vel, &Open));
    }

    struct Wall; // solid column at x >= 520 (rope joints / anchor), nothing blocks the body
    impl Env for Wall {
        fn solid_px(&self, x: i32, y: i32) -> bool { (x == 500 && y == 100) || x >= 520 }
        fn blocked(&self, _: i32, _: i32) -> bool { false }
    }

    #[test]
    fn rope_wraps_on_a_wall_and_pivot_moves() {
        let (mut c, mut pos, mut vel) = setup(100, Chain::angle_of(10 << 16, 99 << 16));
        for _ in 0..80 {
            vel.1 += TICK_GRAVITY;
            vel.0 += TICK_SWING * 3;
            if c.step(&mut pos, &mut vel, 60 << 16, &Wall) == Outcome::Released { break; }
            if !c.pivots().1.is_empty() { return; }
        }
        // Either wrapped (returned above) or released because the pivot was not re-verified solid.
        panic!("rope never wrapped over the wall");
    }

    /// Real WA 3.8 capture (Wine rig, 2026-11-17): rope length 30 px, anchor 30 px left of the worm,
    /// released from rest. Each row is the object's (x, y, vx, vy) as it ENTERED the swing routine on
    /// consecutive 50 fps frames (after gravity 0.24 + damping 0xfc28 were applied).
    struct Anchor1035;
    impl Env for Anchor1035 {
        fn solid_px(&self, x: i32, y: i32) -> bool { x == 1035 && y == 337 }
        fn blocked(&self, _: i32, _: i32) -> bool { false }
    }

    #[test]
    fn matches_captured_wa_swing_frames() {
        let cap: [(i32, i32, i32, i32); 4] = [
            (69795840, 22085632, 0, 15491),
            (69795750, 22101112, -121, 30749),
            (69795270, 22131802, -713, 45768),
            (69793680, 22177402, -2105, 60524),
        ];
        let c0 = Chain::attach(1035 << 16, 337 << 16, 30 << 16, Chain::angle_of(30 << 16, 0)).unwrap();
        let mut c = c0;
        let mut pos = (cap[0].0, cap[0].1);
        let mut vel = (0, 0);
        for f in 0..3 {
            vel.1 += 15728; // world gravity 0.24 (0x3d70)
            assert_eq!(c.step_with(&mut pos, &mut vel, 0, &Anchor1035, 64552), Outcome::Ok);
            // next frame's entry state = this frame's output, then gravity + damping again
            let (ex, ey) = (cap[f + 1].0, cap[f + 1].1);
            assert!((pos.0 - ex).abs() < 200 && (pos.1 - ey).abs() < 200, "frame {f}: pos {pos:?} vs {ex},{ey}");
            let ev = (mul(vel.0, 64552), mul(vel.1 + 15728, 64552));
            assert!((ev.0 - cap[f + 1].2).abs() < 200 && (ev.1 - cap[f + 1].3).abs() < 200, "frame {f}: vel {ev:?} vs {:?}", &cap[f + 1]);
        }
    }

    /// Holding Right in the direction of travel pumps the swing (vx and +vy impulse each tick);
    /// alternating with the swing builds amplitude well beyond the unpumped, damped swing.
    #[test]
    fn swing_keys_pump_amplitude() {
        let run = |pump: bool| -> i32 {
            let (mut c, mut pos, mut vel) = setup(120, Chain::angle_of(40 << 16, 110 << 16));
            let mut amp = 0;
            for t in 0..900 {
                if pump {
                    // push in the direction of horizontal motion, as a player would
                    let dir = if vel.0 > 0 { 1 } else if vel.0 < 0 { -1 } else { 1 };
                    vel.0 += dir * TICK_SWING;
                    vel.1 += TICK_SWING;
                }
                vel.1 += TICK_GRAVITY;
                assert_eq!(c.step(&mut pos, &mut vel, 60 << 16, &Open), Outcome::Ok);
                if t > 600 { amp = amp.max((pos.0 - c.ax).abs()); }
            }
            amp
        };
        let (still, pumped) = (run(false), run(true));
        assert!(pumped > still * 2, "pumped amplitude {pumped} should far exceed unpumped {still}");
    }

    /// Replay of randomized scenarios recorded from WA.exe's own rope machine code
    /// (`emu/wa_emu.py` in the decomp workspace runs 0x4fd360/0x4fd4c0/0x4fd640/0x500d00 under Unicorn).
    /// Set WA_ROPE_ORACLE=<file> to run a larger recording.
    #[test]
    fn matches_wa_machine_code_oracle() {
        struct Rects(Vec<[i32; 4]>, Vec<[i32; 4]>);
        impl Env for Rects {
            fn solid_px(&self, x: i32, y: i32) -> bool { self.0.iter().any(|r| r[0] <= x && x < r[2] && r[1] <= y && y < r[3]) }
            fn blocked(&self, x16: i32, y16: i32) -> bool {
                let (x, y) = (x16 >> 16, y16 >> 16);
                self.1.iter().any(|r| r[0] <= x && x < r[2] && r[1] <= y && y < r[3])
            }
        }
        let big = std::env::var("WA_ROPE_ORACLE").ok().map(|p| std::fs::read_to_string(p).unwrap());
        let text = big.as_deref().unwrap_or(include_str!("../../tests/data/wa_rope_oracle.txt"));
        let (mut scen, mut bad, mut first, mut fr, mut frames) = (0, 0, String::new(), 0, 0);
        let mut cur: Option<(Chain, (i32, i32), (i32, i32), Rects, bool)> = None;
        for line in text.lines() {
            let t: Vec<i32> = line[2..].split_whitespace().map(|x| x.parse().unwrap()).collect();
            if line.starts_with('S') {
                scen += 1; fr = 0;
                let (n, nb) = (t[8] as usize, t[9] as usize);
                let rd = |o: usize, k: usize| [t[o + 4 * k], t[o + 4 * k + 1], t[o + 4 * k + 2], t[o + 4 * k + 3]];
                let rects = Rects((0..n).map(|k| rd(10, k)).collect(), (0..nb).map(|k| rd(10 + 4 * n, k)).collect());
                let c = Chain::attach(t[0] << 16, t[1] << 16, t[2], t[3]).unwrap();
                cur = Some((c, (t[4], t[5]), (t[6], t[7]), rects, false));
            } else if let Some((c, pos, vel, env, dead)) = cur.as_mut() {
                fr += 1;
                if *dead { continue; }
                frames += 1;
                let (released, sw, rl) = (t[0] != 0, t[1], t[2]);
                if sw != 0 { vel.0 += sw; vel.1 += 0x1999; }
                if rl < 0 { c.set_length(c.len - 0x40000, pos, vel, env); }
                else if rl > 0 && c.len < 512 << 16 { c.set_length(c.len + 0x40000, pos, vel, env); }
                vel.1 += 15728;
                let out = c.step_with(pos, vel, 0, env, 64552);
                if released {
                    if out != Outcome::Released { bad += 1; if first.is_empty() { first = format!("scenario {scen} f{fr}: oracle released, port did not"); } }
                    *dead = true;
                    continue;
                }
                let want = ((t[3], t[4]), (t[5], t[6]), t[7]);
                let nseg = t[8] as usize;
                let tbl_ok = c.nseg == nseg && (0..nseg).all(|i| c.tbl[i] == (t[9 + 2 * i], t[10 + 2 * i]));
                if out == Outcome::Released || (*pos, *vel, c.len) != want || !tbl_ok {
                    bad += 1; *dead = true;
                    if first.is_empty() {
                        first = format!("scenario {scen} f{fr}: got pos {pos:?} vel {vel:?} len {} want {want:?} nseg {} vs {nseg} tbl_ok={tbl_ok} out={out:?}", c.len, c.nseg);
                    }
                }
            }
        }
        assert!(frames > 1000);
        assert_eq!(bad, 0, "{bad}/{scen} scenarios diverge from WA machine code; first: {first}");
    }
}
