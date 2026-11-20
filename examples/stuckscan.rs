//! For many seeds: walk every spawned soldier left then right for 40 ticks; report spawns that cannot move either way.
use arty::game::loop_runner::server_tick;
use arty::input::{Button, InputState};
fn main() {
    let n: u64 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(100);
    let base: u64 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(0);
    let (mut stuck, mut total) = (0, 0);
    for seed in base..base + n {
        let g0 = arty::replay::new_game(seed.wrapping_mul(0x9E3779B97F4A7C15), &[0, 1]);
        let (nt, ns) = (g0.teams.len(), g0.teams[0].soldiers.len());
        for ti in 0..nt { for si in 0..ns {
            total += 1;
            let mut moved = false;
            for dir in [Button::Left, Button::Right] {
                let mut g = arty::replay::new_game(seed.wrapping_mul(0x9E3779B97F4A7C15), &[0, 1]);
                g.teams[ti].active = si;
                let _ = ti;
                let p0 = g.teams[ti].soldiers[si].pos;
                // make this team the active one by direct field if available
                for _ in 0..40 {
                    let (prev, now) = (0u16, 1u16 << Button::ALL.iter().position(|&x| x == dir).unwrap());
                    server_tick(&mut g, &InputState::from_bits(prev, now), None, None);
                }
                let p1 = g.teams[ti].soldiers[si].pos;
                if (p1.x - p0.x).abs() > 2.0 { moved = true; }
            }
            if !moved && ti == g0.active_team() { stuck += 1; let p = g0.teams[ti].soldiers[si].pos; println!("seed {seed} team {ti} s{si} at ({:.0},{:.0}) cannot move", p.x, p.y); }
        } }
    }
    println!("{stuck} stuck active-team spawns of {total} checked");
}
