//! A worm touching a slope on a short rope (user map 18eafd934ee6cadd, hook (1757,425), report 2026-11-24): the swing step cannot move it, and
//! gravity used to bank up to ~25 px/tick of downward speed, so releasing the rope slammed it into the ground (fall dmg 21 @ 15.6 px/frame).
use arty::game::state::{GameState, RopeState};
use arty::game::soldier::SoldierState;
use arty::game::team::{Difficulty, Team};
use arty::game::loop_runner::server_tick;
use arty::input::{Button, InputState};
use arty::physics::wa_rope::Chain;
use arty::world::{Terrain, Vec2, WorldPos, WORLD_W};

fn setup(sx: f32, sy: f32) -> (GameState, usize, usize) {
    let mut terrain = Terrain::generate_tactical(0x18eafd934ee6cadd);
    let all = terrain.find_team_spawns(0, WORLD_W, 8);
    let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, s)| s).collect();
    let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s).collect();
    let teams = vec![Team::new(0, false, Difficulty::Medium, &t0), Team::new(1, false, Difficulty::Medium, &t1)];
    let mut g = GameState::new(1, terrain, teams, 2);
    let (ti, si) = (g.active_team(), g.teams[g.active_team()].active);
    let (ax, ay) = (1757.0f32, 425.0f32);
    g.teams[ti].soldiers[si].pos = WorldPos::new(sx, sy);
    g.teams[ti].soldiers[si].state = SoldierState::Airborne { vel: Vec2::new(0.0, 0.0), spinning: false };
    g.rope_session = true;
    let (dx, dy) = (sx - ax, sy - ay);
    let len = (dx * dx + dy * dy).sqrt();
    let theta = Chain::angle_of((dx * 65536.0) as i32, (dy * 65536.0) as i32);
    let chain = Chain::attach((ax * 65536.0) as i32, (ay * 65536.0) as i32, (len * 65536.0) as i32, theta);
    g.rope = Some(RopeState { anchor: WorldPos::new(ax, ay), length: len, flying: false, hook: WorldPos::new(ax, ay),
        hook_vel: Vec2::new(0.0, 0.0), wrap: Vec::new(), chain });
    g.turn.on_fired();
    (g, ti, si)
}

#[test]
fn pinned_worm_on_a_slope_does_not_bank_speed() {
    let mut worst = 0.0f32;
    let idx = |b: Button| Button::ALL.iter().position(|x| *x == b).unwrap();
    for sy in [428.0f32, 430.0, 432.0, 434.0, 436.0, 440.0, 445.0] {
        for sx in (1750..1775).step_by(3) {
            for pat in 0..4 {
                let (mut g, ti, si) = setup(sx as f32, sy);
                let mut max_vy = 0.0f32; let mut moved = 0.0f32; let (px, py) = (sx as f32, sy);
                for t in 0..150 {
                    let held: u16 = match pat { 0 => 0, 1 => 1 << idx(Button::Down), 2 => 1 << idx(Button::Left), _ => (1 << idx(Button::Down)) | (1 << idx(Button::Left)) };
                    server_tick(&mut g, &InputState::from_sets(held, 0, 0), None, None);
                    if let SoldierState::Airborne { vel, .. } = g.teams[ti].soldiers[si].state { max_vy = max_vy.max(vel.y); }
                    let p = g.teams[ti].soldiers[si].pos; moved = moved.max(((p.x - px).powi(2) + (p.y - py).powi(2)).sqrt());
                    if g.rope.is_none() { break; }
                    let _ = t;
                }
                let _ = moved; worst = worst.max(max_vy);
                assert!(max_vy < 8.0, "sx {sx} sy {sy} pat {pat}: banked {max_vy:.1} px/tick");
            }
        }
    }
}
