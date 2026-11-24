//! Hand of Jerry (Garcia / Concrete Donkey): the falling sprite must follow the wind-drifted x it will explode at.
use arty::game::state::{GameState, GarciaState};
use arty::game::team::{Difficulty, Team};
use arty::game::loop_runner::server_tick;
use arty::input::InputState;
use arty::world::{Terrain, WORLD_W};

#[test]
fn falling_donkey_sprite_tracks_its_wind_drift() {
    let mut terrain = Terrain::generate_tactical(3);
    let all = terrain.find_team_spawns(0, WORLD_W, 8);
    let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, s)| s).collect();
    let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s).collect();
    let teams = vec![Team::new(0, false, Difficulty::Medium, &t0), Team::new(1, false, Difficulty::Medium, &t1)];
    let mut g = GameState::new(3, terrain, teams, 2);
    g.wind = arty::physics::wind::Wind::new(1.0);
    g.turn.on_fired(); // Watching
    g.garcia = Some(GarciaState {
        cursor_x: 900.0, render_x: 900.0, cursor_y: 100.0, render_y: 100.0, blink_timer: 0,
        falling: true, fall_y: -208.0, vel_y: 0.0, bounce_count: 0, vel_x: 0.0,
    });
    let input = InputState::new();
    let mut drift = 0.0f32;
    for _ in 0..1500 {
        server_tick(&mut g, &input, None, None);
        match &g.garcia {
            Some(d) => { assert_eq!(d.render_x, d.cursor_x, "sprite x must follow the drift"); drift = (d.cursor_x - 900.0).abs(); }
            None => break,
        }
    }
    assert!(g.garcia.is_none(), "donkey never landed");
    println!("wind drift before the hit: {drift:.1} px");
}

#[test]
fn donkey_explodes_once_on_first_contact_and_is_gone() {
    let mut terrain = Terrain::generate_tactical(3);
    let all = terrain.find_team_spawns(0, WORLD_W, 8);
    let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, s)| s).collect();
    let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s).collect();
    let teams = vec![Team::new(0, false, Difficulty::Medium, &t0), Team::new(1, false, Difficulty::Medium, &t1)];
    let mut g = GameState::new(3, terrain, teams, 2);
    g.turn.on_fired();
    g.garcia = Some(GarciaState {
        cursor_x: 900.0, render_x: 900.0, cursor_y: 100.0, render_y: 100.0, blink_timer: 0,
        falling: true, fall_y: -208.0, vel_y: 0.0, bounce_count: 0, vel_x: 0.0,
    });
    let input = InputState::new();
    let mut blasts = 0usize;
    for _ in 0..3000 {
        let before = g.explosions.len();
        server_tick(&mut g, &input, None, None);
        if g.explosions.len() > before { blasts += g.explosions.len() - before; }
        if g.garcia.is_none() { break; }
    }
    assert!(g.garcia.is_none(), "donkey never finished");
    assert_eq!(blasts, 1, "one blast on first contact");
}
