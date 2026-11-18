//! Bazooka shell on WA's own engine: 16.16 fixed point at WA's 50 frames/s, advanced inside Arty's 30 Hz tick
//! (5 WA frames per 3 Arty ticks). Per WA frame: `v += (wind, gravity); sweep(shape-2 box vs land)`.
//! Verified bit-exact against a real WA flight (tests/wa_shell_flight.rs) and the mover oracle (wa_mover_oracle.rs).

use crate::physics::wa_body::{sweep, Body};
use crate::physics::wa_collide::overlaps_land;
use crate::world::{Terrain, Vec2, WorldPos};

/// Reference frames-per-second / Arty ticks-per-second = 5/3.
pub const TIME_SCALE: f32 = 50.0 / 30.0;
/// World gravity in WA's scheme (16.16 px/frame^2) = 0.24.
pub const GRAVITY16: i32 = 15728;
/// WA collision shape id of the bazooka shell: 7x5 box (rig capture, findings/terrain_collision.md).
pub const SHELL_SHAPE: u8 = 2;
/// Scheme version passed to the mover (selects WA's branch for saturating add etc.).
const SCHEME_VER: i32 = 0x200;

/// Per-frame horizontal wind acceleration for a whole wind unit count (-10..=10).
/// Matches the rig: W=1 -> 1872, 2 -> 3744, 6 -> 11234, 7 -> 13107, 8 -> 14979 (= W*65535/35, truncated).
pub fn wind16(units: i32) -> i32 { units * 65535 / 35 }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaMissile { pub x: i32, pub y: i32, pub vx: i32, pub vy: i32 }

impl WaMissile {
    pub fn from_float(pos: WorldPos, vel: Vec2) -> Self {
        let s = 65536.0f64;
        Self {
            x: (pos.x as f64 * s).round() as i32,
            y: (pos.y as f64 * s).round() as i32,
            vx: (vel.x as f64 / TIME_SCALE as f64 * s).round() as i32,
            vy: (vel.y as f64 / TIME_SCALE as f64 * s).round() as i32,
        }
    }
    pub fn pos(&self) -> WorldPos { WorldPos::new(self.x as f32 / 65536.0, self.y as f32 / 65536.0) }
    pub fn vel(&self) -> Vec2 { Vec2::new(self.vx as f32 / 65536.0 * TIME_SCALE, self.vy as f32 / 65536.0 * TIME_SCALE) }

    /// How many WA frames to run on the Arty tick that moves a projectile of age `age_ticks` (1 or 2; averages 5/3).
    /// A pure function of the (networked) age, so server, live-client prediction and TAT replays always agree.
    pub fn frames_for_age(age_ticks: u32) -> u32 { (5 * (age_ticks + 1)) / 3 - (5 * age_ticks) / 3 }

    /// One WA frame. With a terrain, returns `Some((x16, y16))` where the shell hit land/objects (WA snaps the
    /// position to the pixel edge it stopped at); without one (client prediction) it just integrates.
    pub fn step_frame(&mut self, wind16: i32, terrain: Option<&Terrain>) -> Option<(i32, i32)> {
        self.vx = self.vx.wrapping_add(wind16);
        self.vy = self.vy.wrapping_add(GRAVITY16);
        let Some(t) = terrain else {
            self.x = self.x.wrapping_add(self.vx);
            self.y = self.y.wrapping_add(self.vy);
            return None;
        };
        let solid = |x: i32, y: i32| t.is_blocked(x, y);
        let blocked = |x16: i32, y16: i32| overlaps_land(&solid, SHELL_SHAPE, x16 >> 16, y16 >> 16);
        let mut b = Body { x: self.x, y: self.y, vx: self.vx, vy: self.vy, ..Default::default() };
        let mut frac = 0x10000;
        let r = sweep(&mut b, &mut frac, SCHEME_VER, &blocked);
        self.x = b.x; self.y = b.y;
        if r.hit { Some((b.x, b.y)) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn five_frames_per_three_ticks() {
        let total: u32 = (0..300).map(WaMissile::frames_for_age).sum();
        assert_eq!(total, 500);
        assert!((0..300).all(|a| (1..=2).contains(&WaMissile::frames_for_age(a))));
    }
    #[test]
    fn prediction_and_server_agree_from_synced_state() {
        // A client rebuilds the shadow from the synced pos/vel/age; stepping it must match the server's own shadow.
        use crate::physics::{Projectile, WeaponKind};
        let mut srv = Projectile::new(WorldPos::new(300.0, 500.0), Vec2::new(9.0, -12.0), WeaponKind::Bazooka);
        for _ in 0..40 { crate::physics::tick::tick_bazooka(&mut srv, 0.4, None); }
        let mut cli = Projectile::new(srv.pos, srv.vel, WeaponKind::Bazooka);
        cli.age_ticks = srv.age_ticks;
        for _ in 0..9 {
            crate::physics::tick::tick_bazooka(&mut srv, 0.4, None);
            crate::physics::tick::tick_bazooka(&mut cli, 0.4, None);
            assert!((srv.pos.x - cli.pos.x).abs() < 0.01 && (srv.pos.y - cli.pos.y).abs() < 0.01, "{:?} vs {:?}", srv.pos, cli.pos);
        }
    }
}
