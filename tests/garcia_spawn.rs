use arty::game::state::{GameState, GarciaState};
use arty::game::team::{Difficulty, Team};
use arty::game::loop_runner::server_tick;
use arty::input::InputState;
use arty::world::{Terrain, WORLD_W, WATER_Y};

#[test]
fn donkey_never_jumps_on_spawn() {
    let start_y = WATER_Y as f32 - 696.0 - 208.0;
    for seed in 0..40u64 {
        let mut terrain = Terrain::generate_tactical(seed as _);
        let all = terrain.find_team_spawns(0, WORLD_W, 8);
        let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, s)| s).collect();
        let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s).collect();
        let teams = vec![Team::new(0, false, Difficulty::Medium, &t0), Team::new(1, false, Difficulty::Medium, &t1)];
        let mut g = GameState::new(seed as _, terrain, teams, 2);
        g.turn.on_fired();
        for x in [5.0f32, 300.0, 900.0, 1500.0, 1910.0] {
            g.garcia = Some(GarciaState { cursor_x: x, render_x: x, cursor_y: 100.0, render_y: 100.0, blink_timer: 0,
                falling: true, fall_y: start_y, vel_y: 0.0, bounce_count: 0, vel_x: 0.0 });
            let input = InputState::new();
            for _ in 0..30 {
                server_tick(&mut g, &input, None, None);
                // free fall (original exp behaviour) can reach the terrain top within these ticks and blast there: fine. What must
                // never happen is the sprite jumping sideways/downwards in one go (the old "teleport on spawn").
                let Some(d) = g.garcia.as_ref() else { break };
                assert!((d.cursor_x - x).abs() < 40.0 && d.fall_y < start_y + 340.0, "seed {seed}: jumped");
            }
        }
    }
}
