use arty::world::{Terrain, WORLD_W};
use arty::game::state::GameState;
use arty::game::team::{Team, Difficulty};
use arty::physics::wa_worm as ww;

fn build(seed: u64) -> GameState {
    let terrain = Terrain::generate_tactical(seed);
    let mut t = terrain;
    let all = t.find_team_spawns(0, WORLD_W, 8);
    let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i,_)| i%2==0).map(|(_,s)| s).collect();
    let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i,_)| i%2==1).map(|(_,s)| s).collect();
    let teams = vec![Team::new(0,false,Difficulty::Medium,&t0), Team::new(1,false,Difficulty::Medium,&t1)];
    GameState::new(seed, t, teams, 2)
}

#[test]
fn probe() {
    for seed in [0x18BD410020ECA8CDu64] {
        let g = build(seed);
        for (ti, t) in g.teams.iter().enumerate() { for (si, s) in t.soldiers.iter().enumerate() {
            let mut res = vec![];
            for dir in [-1i32, 1] {
                let mut body = ww::body_from_foot(s.pos.x, s.pos.y, 0, 0);
                let mut ws = ww::WalkState::start(body.x, dir);
                let solid = |x: i32, y: i32| g.terrain.is_blocked(x, y);
                let x0 = ww::foot_of(&body).0;
                let mut fell = false;
                for _ in 0..150 { if body.walk_frame(&mut ws, &solid) == -1 { fell = true; break; } }
                res.push((dir, ww::foot_of(&body).0 - x0, fell));
            }
            println!("team{} s{} pos=({:.1},{:.1}) walk: {:?}", ti, si, s.pos.x, s.pos.y, res);
        }}
    }
}

#[test]
fn probe_on_ground() {
    use arty::game::loop_runner::{is_on_ground, is_on_ground_lenient};
    let mut bad = 0;
    let mut seeds = vec![0x18BD410020ECA8CDu64, 0x18e913e785709108];
    seeds.extend((1..200u64).map(|i| i.wrapping_mul(0x9E3779B97F4A7C15)));
    for seed in seeds {
        let g = build(seed);
        for (ti, t) in g.teams.iter().enumerate() { for (si, s) in t.soldiers.iter().enumerate() {
            let a = is_on_ground(&g, ti, si);
            if !a { bad += 1; println!("seed {:x} team{} s{} pos=({:.2},{:.2}) on_ground=false lenient={}", seed, ti, si, s.pos.x, s.pos.y, is_on_ground_lenient(&g, ti, si)); }
        }}
    }
    println!("bad={}", bad);
}

#[test]
fn sheep_on_real_map() {
    use arty::game::loop_runner::fire_jumpbot;
    let mut g = build(0x18BD410020ECA8CDu64);
    g.teams[0].selected_weapon = 0;
    let (px, py) = (g.teams[0].soldiers[0].pos.x, g.teams[0].soldiers[0].pos.y);
    // place directly
    use arty::game::state::JumpbotState;
    g.jumpbot = Some(JumpbotState { x: px + 8.0, y: py - 4.0, vel_y: 0.0, vel_x: 0.0, facing: 1, fuse_ticks: 300, grounded: false, walk_ticks: 0, owner_team: 0, just_jumped: false, wa: None });
    let _ = fire_jumpbot;
    let inp = arty::input::InputState::new();
    let mut hops = 0; let mut last_air = false; let mut xs = vec![];
    for t in 0..300 {
        arty::game::loop_runner::step_jumpbot(&mut g, &inp);
        if let Some(r) = g.jumpbot.as_ref() {
            if !r.grounded && !last_air { hops += 1; }
            last_air = !r.grounded;
            if t % 30 == 0 { xs.push((r.x as i32, r.y as i32, r.facing)); }
        } else { println!("exploded at tick {t}"); break; }
    }
    println!("hops={hops} track={:?}", xs);
}
