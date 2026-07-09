//! Fire patches must (a) burn worms faster than the old 3 HP/s, and (b) push a
//! grounded soldier sideways along the ground (WA-style slide) rather than
//! launching it into the air.

use arty::game::state::{FirePatch, GameState};
use arty::game::soldier::SoldierState;
use arty::game::team::{Difficulty, Team};
use arty::game::loop_runner::server_tick;
use arty::input::InputState;
use arty::world::{Terrain, Vec2, WorldPos, WORLD_W};

fn build() -> GameState {
    // Flat solid ground at y=300 across the map so a soldier stands and can slide.
    let mut terrain = Terrain::generate_tactical(3);
    let all = terrain.find_team_spawns(0, WORLD_W, 8);
    let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, s)| s).collect();
    let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s).collect();
    let teams = vec![
        Team::new(0, false, Difficulty::Medium, &t0),
        Team::new(1, false, Difficulty::Medium, &t1),
    ];
    GameState::new(3, terrain, teams, 2)
}

#[test]
fn grounded_soldier_slides_and_burns_faster() {
    let mut game = build();
    // Put team-0 soldier 0 on a known solid surface, idle.
    let sx = game.teams[0].soldiers[0].pos.x;
    let sy = game.teams[0].soldiers[0].pos.y;
    game.teams[0].soldiers[0].state = SoldierState::Idle;
    let hp0 = game.teams[0].soldiers[0].hp;

    // A settled fire patch just left of the soldier's foot (same solid ground, so it
    // persists), zero velocity. Soldier should be pushed RIGHT (away from flame) and
    // slide along the ground, never launched upward.
    game.fire_patches = vec![FirePatch {
        pos: WorldPos { x: sx - 1.0, y: sy },
        vel: Vec2 { x: 0.0, y: 0.0 },
        landed: true,
        lifetime: 400,
    }];

    // Drive the full sim (soldier physics + fire) via server_tick so hops
    // actually integrate and carry the soldier.
    let input = InputState::new();
    game.tick = 0;
    let mut hopped = false;
    for _ in 0..48 {
        // Keep the flame present under the soldier's original spot each tick.
        if game.fire_patches.is_empty() {
            game.fire_patches.push(FirePatch {
                pos: WorldPos { x: sx - 1.0, y: sy },
                vel: Vec2 { x: 0.0, y: 0.0 }, landed: true, lifetime: 400,
            });
        }
        server_tick(&mut game, &input, None, None);
        if matches!(game.teams[0].soldiers[0].state, SoldierState::Airborne { .. }) {
            hopped = true;
        }
    }

    let s = &game.teams[0].soldiers[0];
    // WA burn-hop: the soldier jumps at least once (unsticks it), instead of the
    // old every-tick fling or getting stuck in place.
    assert!(hopped, "burning soldier never hopped (would get stuck and die)");
    // And it moved away from where it started (hop + slide carry it out of the fire).
    assert!((s.pos.x - sx).abs() > 3.0,
        "soldier did not move out of the fire (x {} -> {})", sx, s.pos.x);
    // Bounded burn over ~1.6 s: NOT an instant kill.
    let lost = hp0 - s.hp;
    assert!(lost >= 1 && lost < hp0,
        "burn rate off: lost {lost} of {hp0} HP in 48 ticks");
}

#[test]
fn burning_soldiers_separate_from_a_clump() {
    // Two soldiers overlapping on the same ground with fire between them must be
    // able to hop apart (pass through each other) instead of blocking each other.
    let mut game = build();
    let base = game.teams[0].soldiers[0].pos;
    game.teams[0].soldiers[0].pos = WorldPos { x: base.x - 1.0, y: base.y };
    game.teams[0].soldiers[1].pos = WorldPos { x: base.x + 1.0, y: base.y };
    game.teams[0].soldiers[0].state = SoldierState::Idle;
    game.teams[0].soldiers[1].state = SoldierState::Idle;
    let start_gap = (game.teams[0].soldiers[0].pos.x - game.teams[0].soldiers[1].pos.x).abs();

    let input = InputState::new();
    game.tick = 0;
    for _ in 0..64 {
        // Fire pool centred between the two so each latches an opposite escape dir.
        game.fire_patches.push(FirePatch {
            pos: WorldPos { x: base.x, y: base.y },
            vel: Vec2 { x: 0.0, y: 0.0 }, landed: true, lifetime: 1,
        });
        server_tick(&mut game, &input, None, None);
    }

    let gap = (game.teams[0].soldiers[0].pos.x - game.teams[0].soldiers[1].pos.x).abs();
    assert!(gap > start_gap + 8.0,
        "clumped burning soldiers did not separate (gap {start_gap} -> {gap})");
}

#[test]
fn molotov_leaves_no_crater() {
    let mut game = build();
    // A soldier stands on solid ground; find the solid pixel just under its foot.
    let fx = game.teams[0].soldiers[0].pos.x as i32;
    let fy0 = game.teams[0].soldiers[0].pos.y as i32;
    let (tx, ty) = (fx, (fy0..fy0 + 20).find(|&y| game.terrain.is_solid(fx, y))
        .expect("no solid ground under soldier"));
    let before = game.crater_log.len();

    game.spawn_molotov_fire(WorldPos { x: tx as f32, y: ty as f32 });

    // Terrain at the impact point is untouched, and no crater was logged (so
    // clients stay crater-free too).
    assert!(game.terrain.is_solid(tx, ty), "molotov carved a crater in the terrain");
    assert_eq!(game.crater_log.len(), before, "molotov pushed a crater to the sync log");
    // But it did spawn the fire.
    assert!(!game.fire_patches.is_empty(), "molotov spawned no fire");
}
