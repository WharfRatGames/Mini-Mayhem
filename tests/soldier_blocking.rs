//! A soldier that walks into another soldier is held back; it must never jump to the far side (walk-state run-ahead bug).
use arty::input::{InputState, Button};
use arty::game::soldier::SoldierState;
#[test]
fn walking_into_a_soldier_never_teleports_past_him() {
    for seed in [7u64, 11, 99] {
    let mut g = arty::replay::new_game(seed, &[0, 1]);
    g.mines.clear(); // only soldier-vs-soldier blocking is under test
    let ti = g.active_team(); let si = g.teams[ti].active;
    let (x, y) = { let s = &g.teams[ti].soldiers[si]; (s.pos.x as i32, s.pos.y as i32) };
    // flat floor + open air 80 px high around
    for dx in -80..=240 { for dy in 1..=80 { g.terrain.set_solid(x + dx, y - dy, false); } for dy in 0..=30 { g.terrain.set_solid(x + dx, y + dy, true); } }
    // park every other soldier far away, put one neighbour 20 px to the right
    let (oti, osi) = if ti == 0 { (1, 0) } else { (0, 0) };
    for t in 0..g.teams.len() { for s in 0..g.teams[t].soldiers.len() { if (t, s) != (ti, si) { g.teams[t].soldiers[s].pos.x = 1800.0; } } }
    g.teams[oti].soldiers[osi].pos.x = (x + 22) as f32; g.teams[oti].soldiers[osi].pos.y = y as f32;
    g.teams[oti].soldiers[osi].state = SoldierState::Idle;
    let right = Button::ALL.iter().position(|&b| b == Button::Right).unwrap();
    let mut last = g.teams[ti].soldiers[si].pos.x;
    for t in 0..120 {
        let inp = InputState::from_sets(1 << right, if t == 0 { 1 << right } else { 0 }, 0);
        arty::game::loop_runner::simulate(&mut g, &inp);
        let s = &g.teams[ti].soldiers[si]; let o = &g.teams[oti].soldiers[osi];
        assert!((s.pos.x - last).abs() < 8.0, "jumped {} px at tick {t}", (s.pos.x - last).abs()); assert!(s.pos.x < o.pos.x, "ended up past the other soldier at tick {t}");
        last = s.pos.x;
    }
    }
}
