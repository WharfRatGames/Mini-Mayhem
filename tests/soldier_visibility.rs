use arty::world::{Terrain, WorldPos, WORLD_W};
use arty::game::state::GameState;
use arty::game::team::{Team, Difficulty};
use arty::game::loop_runner::{render, LoopState};
use arty::renderer::buffer::WorldBuffer;
use arty::renderer::camera::Camera;

fn build(seed: u64) -> GameState {
    let mut terrain = Terrain::generate_tactical(seed);
    let all = terrain.find_team_spawns(0, WORLD_W, 8);
    let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i,_)| i%2==0).map(|(_,s)| s).collect();
    let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i,_)| i%2==1).map(|(_,s)| s).collect();
    let teams = vec![
        Team::new(0, false, Difficulty::Medium, &t0),
        Team::new(1, false, Difficulty::Medium, &t1),
    ];
    GameState::new(seed, terrain, teams, 2)
}

#[test]
fn soldiers_render_at_their_positions() {
    for seed in [1u64, 2, 3, 7, 17] {
        let game = build(seed);
        let is_cavern = game.terrain.is_cavern;
        let positions: Vec<WorldPos> = game.teams.iter().flat_map(|t| t.soldiers.iter().map(|s| s.pos)).collect();
        for (i, pos) in positions.iter().enumerate() {
            let mut cam = Camera::new(pos.x, pos.y);
            cam.snap_to(*pos);
            let mut buf = WorldBuffer::new();
            let mut ls = LoopState::new();
            render(&game, &mut buf, &cam, &mut ls);
            // Second render with this soldier moved away; pixel diff near the
            // spawn proves the soldier sprite was actually drawn there.
            let mut game2 = build(seed);
            for t in &mut game2.teams { for s in &mut t.soldiers {
                if (s.pos.x - pos.x).abs() < 0.1 && (s.pos.y - pos.y).abs() < 0.1 {
                    s.pos.x = if pos.x > 200.0 { pos.x - 150.0 } else { pos.x + 150.0 };
                }
            }}
            let mut buf2 = WorldBuffer::new();
            let mut ls2 = LoopState::new();
            render(&game2, &mut buf2, &cam, &mut ls2);
            let mut diff = 0usize;
            for dy in -24i32..8 { for dx in -12i32..12 {
                let x = pos.x as i32 + dx; let y = pos.y as i32 + dy;
                if buf.get_pixel(x, y) != buf2.get_pixel(x, y) { diff += 1; }
            }}
            assert!(diff > 20, "seed {seed} cavern={is_cavern} soldier {i} at ({:.0},{:.0}) invisible (diff={diff})", pos.x, pos.y);
        }
    }
}
