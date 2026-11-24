//! Match setup shared by the live server, the client and the replay harness (docs/EXPERIMENTAL_PLAN.md).
//!
//! Map objects (mines, barrels) and the N-team game builder used to live in the binaries, as two diverging copies: the server's
//! barrel placement lacked the client's cavern-floor and scenery handling, so live matches could put barrels on the unreachable
//! rock roof of a cave map. One implementation, here, now; a replay of a real match builds the game exactly as the server does.

use crate::game::state::GameState;
use crate::game::team::{Difficulty, Team};
use crate::world::{Terrain, WorldPos, WORLD_W};

/// Returns true if `pos` is within the mine's safe-spawn exclusion radius of any soldier.
/// Uses 50px — covers trigger radius (20) + blast radius (25) + margin.
pub fn too_close_to_soldiers(game: &crate::game::state::GameState, pos: WorldPos) -> bool {
    const EXCLUSION: f32 = 50.0;
    game.teams.iter().flat_map(|t| t.soldiers.iter()).any(|s| {
        let dx = s.pos.x - pos.x;
        let dy = s.pos.y - pos.y;
        (dx * dx + dy * dy).sqrt() < EXCLUSION
    })
}

/// Returns true if column `x` overlaps (or sits too close to) any scenery
/// object's footprint. Barrels only checked their landing Y against scenery
/// (via `surface_y_at_with_scenery`, so they rest on top instead of embedding)
/// but never checked X clearance, so one could land stacked directly against/
/// on top of a decoration with no visual gap. Mirrors `find_team_spawns`'
/// `clear_of_scenery` margin (footprint + 24px breathing room).
pub fn too_close_to_scenery(game: &crate::game::state::GameState, x: i32) -> bool {
    let theme = crate::world::terrain::Theme::of(game.terrain.is_cavern, game.terrain.template_id);
    game.terrain.scenery.iter().any(|o| {
        let hw = o.footprint(theme).0 + 24;
        (x - o.x as i32).abs() <= hw
    })
}

/// WA's placement pool for map objects: every worm, mine and oil drum is placed by the SAME spawn algorithm (random walk over
/// x, first free 47 px square over land, 107 px exclusion around each accepted spot), so objects keep clear of worms and of each
/// other and are skipped when the map has no room. Returns up to `mines + barrels` spots continuing after the worms' own spots
/// (interleaved: 3 of every 5 spots are mines, 2 are drums). Empty on maps without a WA seed.
fn object_pool(game: &mut GameState, want: usize) -> Vec<WorldPos> {
    let worms: usize = game.teams.iter().map(|t| t.soldiers.len()).sum();
    // The pool's own worm slots are only a model: the soldiers are really placed by find_team_spawns (nudged/fallback spots), so an
    // object spot can land on a worm (-exp35: 6% of soldiers within 10 px of a mine/drum). Drop any spot too close to a real soldier;
    // WA keeps a 107 px exclusion between spots, 40 px is the floor we enforce. Deterministic (same soldiers on server and client).
    let soldiers: Vec<WorldPos> = game.teams.iter().flat_map(|t| t.soldiers.iter().map(|s| s.pos)).collect();
    game.terrain.find_spawn_pool(worms, want + soldiers.len()).1.into_iter()
        .filter(|p| soldiers.iter().all(|s| (s.x - p.x).hypot(s.y - p.y) >= 40.0))
        .take(want)
        .collect()
}

pub const MAX_MINES: usize = 12;
pub const MAX_BARRELS: usize = 8;

/// Deterministically place 9–15 armed mines across the map using the game seed.
pub fn place_map_mines(game: &mut GameState) {
    use crate::game::state::{PlacedMine, MineState};
    let seed = game.map_seed;
    let mine_count = 16 + (seed % 9) as usize;  // 16–24 (legacy maps only)
    let pool = object_pool(game, MAX_MINES + MAX_BARRELS);
    if !pool.is_empty() {
        for p in pool.iter().enumerate().filter(|(i, _)| i % 5 < 3).map(|(_, p)| p).take(MAX_MINES) {
            game.mines.push(PlacedMine { pos: *p, state: MineState::Armed, arm_ticks: 0, trigger_ticks: 0 });
        }
        return;
    }

    // Simple LCG derived from seed
    let mut rng = seed.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
    let spread = WORLD_W / (mine_count as u32 + 1);

    for i in 1..=mine_count {
        rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
        let offset = (rng % spread as u64) as u32;
        let x = (spread * i as u32 + offset).clamp(20, WORLD_W - 20);
        if let Some(surf_y) = game.terrain.surface_y_at_with_scenery(x) {
            let mine_pos = WorldPos::new(x as f32, surf_y as f32 - 1.0);
            if (surf_y as f32) < crate::world::WATER_Y as f32 - 10.0
                && !too_close_to_soldiers(&game, mine_pos)
            {
                game.mines.push(PlacedMine {
                    pos: mine_pos,
                    state: MineState::Armed,
                    arm_ticks: 0,
                    trigger_ticks: 0,
                });
            }
        }
    }
}

/// Deterministically place 7–11 explosive barrels across the map using the game seed.
pub fn place_map_barrels(game: &mut crate::game::state::GameState) {
    use crate::game::state::{Barrel, BarrelState};
    let seed = game.map_seed;
    let count = 14 + (seed.wrapping_mul(0xDEAD_C0DE) % 7) as usize; // 14–20 (legacy maps only)
    let pool = object_pool(game, MAX_MINES + MAX_BARRELS);
    if !pool.is_empty() {
        for p in pool.iter().enumerate().filter(|(i, _)| i % 5 >= 3).map(|(_, p)| p).take(MAX_BARRELS) {
            game.barrels.push(Barrel { pos: *p, vel: crate::world::Vec2::new(0.0, 0.0), hp: 60, state: BarrelState::Normal });
        }
        return;
    }

    let mut rng = seed.wrapping_mul(0xBEEF_1234_5678_9ABCu64).wrapping_add(1442695040888963407);
    let spread = WORLD_W / (count as u32 + 1);

    for i in 1..=count {
        rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
        let offset = (rng % spread as u64) as u32;
        let x = (spread * i as u32 + offset).clamp(20, WORLD_W - 20);
        // On cave maps (is_cavern) the surface is the sealed rock cap — barrels
        // placed via surface_y_at_with_scenery land on the unreachable roof.
        // Use the same standable-cave-floor lookup as maybe_drop_crate so
        // barrels rest inside an actual accessible chamber.
        let pos = if game.terrain.is_cavern {
            match game.terrain.standable_cave_foot_simple(x as i32) {
                Some(foot_y) => WorldPos::new(x as f32, foot_y as f32 - 1.0),
                None => continue, // no standable cave floor at this x
            }
        } else {
            match game.terrain.surface_y_at_with_scenery(x) {
                // pos.y = first air pixel above terrain or scenery (surf_y - 1); physics rests here.
                Some(surf_y) => WorldPos::new(x as f32, surf_y as f32 - 1.0),
                None => continue,
            }
        };
        if pos.y < crate::world::WATER_Y as f32 - 10.0
            && !too_close_to_soldiers(game, pos)
            && !too_close_to_scenery(game, x as i32)
        {
            game.barrels.push(Barrel {
                pos,
                vel: crate::world::Vec2::new(0.0, 0.0),
                hp: 60,
                state: BarrelState::Normal,
            });
        }
    }
}


/// Build an N-team game (N = colors.len(), 2-4) exactly as the live server does. The map is divided into N vertical bands so each
/// team spawns in its own region; `colors[i]` sets team i's colour. The 2-team case reproduces the original left/right split.
pub fn build_game_n(seed: u64, colors: &[u8]) -> GameState {
    let mut terrain = Terrain::generate_tactical(seed);
    let n = colors.len().clamp(2, 4);
    let all_spawns = terrain.find_team_spawns(0, WORLD_W, n * 4);
    let mut teams = Vec::with_capacity(n);
    for i in 0..n {
        let spawns: Vec<_> = all_spawns.iter().cloned()
            .enumerate().filter(|(k, _)| k % n == i).map(|(_, s)| s).collect();
        let mut team = Team::new(i, false, Difficulty::Medium, &spawns);
        team.set_color(colors[i]);
        teams.push(team);
    }
    let mut game = GameState::new(seed, terrain, teams, n);
    place_map_mines(&mut game);
    place_map_barrels(&mut game);
    game
}
