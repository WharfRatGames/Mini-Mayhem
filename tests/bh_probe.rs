//! Black hole collapse damage must be a fixed function of distance at the moment of collapse (it used to hit only
//! soldiers inside the 13 px event horizon, so neighbours at 12 px and 14 px got 35 and 0).
use arty::world::{Terrain, WORLD_W, WorldPos};
use arty::game::state::{GameState, BlackHole};
use arty::game::team::{Team, Difficulty};
use arty::game::soldier::SoldierState;

fn build(seed: u64) -> GameState {
    let mut t = Terrain::generate_tactical(seed);
    let all = t.find_team_spawns(0, WORLD_W, 8);
    let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i,_)| i%2==0).map(|(_,s)| s).collect();
    let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i,_)| i%2==1).map(|(_,s)| s).collect();
    let teams = vec![Team::new(0,false,Difficulty::Medium,&t0), Team::new(1,false,Difficulty::Medium,&t1)];
    GameState::new(seed, t, teams, 2)
}

#[test]
fn collapse_damage_is_graded_by_distance() {
    let mut g = build(7);
    let hole = WorldPos { x: 600.0, y: 300.0 };
    // (distance from the hole centre, expected damage)
    let cases = [(5.0f32, 35), (12.0, 35), (14.0, 35), (40.0, 35), (60.0, 25), (90.0, 8), (110.0, 0), (200.0, 0)];
    let n = cases.len();
    for t in g.teams.iter_mut() { for s in t.soldiers.iter_mut() { s.pos = WorldPos { x: 5.0, y: 5.0 }; s.hp = 0; s.state = SoldierState::Dead; } }
    for (k, (d, _)) in cases.iter().enumerate() {
        let (ti, si) = (k % 2, k / 2);
        let s = &mut g.teams[ti].soldiers[si];
        s.pos = WorldPos { x: hole.x + d, y: hole.y };
        s.hp = 100; s.state = SoldierState::Airborne { vel: arty::world::Vec2::new(0.0, 0.0), spinning: false };
    }
    assert!(g.teams[0].soldiers.len() * 2 >= n);
    g.black_holes.push(BlackHole { pos: hole, lifetime: 1 });
    g.step_black_holes();
    for (k, (d, want)) in cases.iter().enumerate() {
        let (ti, si) = (k % 2, k / 2);
        let lost = 100 - g.teams[ti].soldiers[si].hp as i32;
        assert!((lost - want).abs() <= 1, "soldier at {d}px lost {lost}, expected ~{want}");
    }
}
