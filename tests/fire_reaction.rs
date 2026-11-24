//! WA flame model (findings/physics.md "WA flame particle"): a flame hit costs 1 HP and shoves the worm away from the flame
//! (up and away, plus the flame's own v/8); there is no burn state, hop or slide. Carving rules are tested too.

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


/// Flat 40 px floor and open sky around the first soldier, every other soldier parked far away (isolates the fire logic from the map).
fn flatten_around(game: &mut GameState) -> WorldPos {
    let b = game.teams[0].soldiers[0].pos;
    for dx in -200..=200 { for dy in 1..=90 { game.terrain.set_solid(b.x as i32 + dx, b.y as i32 - dy, false); } for dy in 0..=40 { game.terrain.set_solid(b.x as i32 + dx, b.y as i32 + dy, true); } }
    for t in 0..game.teams.len() { for i in 0..game.teams[t].soldiers.len() { if (t, i) != (0, 0) && (t, i) != (0, 1) { game.teams[t].soldiers[i].pos.x = 1800.0 + 20.0 * i as f32; } } }
    game.teams[0].soldiers[1].pos = WorldPos { x: b.x - 150.0, y: b.y };
    b
}

#[test]
fn only_carving_fire_eats_terrain() {
    // WA parity (assets/barrel.mp4): barrel fire (carves:true) gradually eats
    // terrain while landed; molotov/petrol fire (carves:false) never does.
    let input = InputState::new();

    // Find a solid pixel to sit a flame on.
    let mut g = build();
    let fx = g.teams[0].soldiers[0].pos.x as i32;
    let fy0 = g.teams[0].soldiers[0].pos.y as i32;
    let (sx, sy) = (fx, (fy0..fy0 + 40).find(|&y| g.terrain.is_solid(fx, y))
        .expect("no solid ground under soldier"));

    // Non-carving flame: terrain stays intact, nothing logged to clients.
    let before = g.crater_log.len();
    g.fire_patches = vec![FirePatch {
        pos: WorldPos { x: sx as f32, y: sy as f32 }, vel: Vec2 { x: 0.0, y: 0.0 },
        landed: true, lifetime: 400, landed_ticks: 0, carves: false, splits: 0, dir: 0, timer: 0, cool: 0, wisp: 0, turns: false,
    }];
    // server_tick increments game.tick before the fire-patch update runs, so
    // tick only hits a BURN_CARVE_INTERVAL (32) multiple starting on the 32nd
    // call — run past that, not just 30 ticks, or the carve window never opens.
    g.tick = 0;
    for _ in 0..40 {
        if g.fire_patches.is_empty() {
            g.fire_patches.push(FirePatch {
                pos: WorldPos { x: sx as f32, y: sy as f32 }, vel: Vec2 { x: 0.0, y: 0.0 },
                landed: true, lifetime: 400, landed_ticks: 0, carves: false, splits: 0, dir: 0, timer: 0, cool: 0, wisp: 0, turns: false });
        }
        server_tick(&mut g, &input, None, None);
    }
    assert_eq!(g.crater_log.len(), before, "non-carving fire logged a crater");

    // Carving flame on the same spot: terrain erodes (crater(s) logged).
    let mut g2 = build();
    let before2 = g2.crater_log.len();
    g2.fire_patches = vec![FirePatch {
        pos: WorldPos { x: sx as f32, y: sy as f32 }, vel: Vec2 { x: 0.0, y: 0.0 },
        landed: true, lifetime: 400, landed_ticks: 0, carves: true, splits: 0, dir: 0, timer: 0, cool: 0, wisp: 0, turns: false,
    }];
    g2.tick = 0;
    for _ in 0..40 {
        if g2.fire_patches.is_empty() {
            g2.fire_patches.push(FirePatch {
                pos: WorldPos { x: sx as f32, y: sy as f32 }, vel: Vec2 { x: 0.0, y: 0.0 },
                landed: true, lifetime: 400, landed_ticks: 0, carves: true, splits: 0, dir: 0, timer: 0, cool: 0, wisp: 0, turns: false });
        }
        server_tick(&mut g2, &input, None, None);
    }
    assert!(g2.crater_log.len() > before2, "carving (barrel) fire did not eat terrain");
}

#[test]
fn molotov_cuts_one_wa_crater_and_spawns_fire() {
    // WA (wa_rig molo_land): the impact carves one radius-18 crater and is logged so clients carve it too; the fire spawns.
    let mut game = build();
    let fx = game.teams[0].soldiers[0].pos.x as i32;
    let fy0 = game.teams[0].soldiers[0].pos.y as i32;
    let (tx, ty) = (fx, (fy0..fy0 + 20).find(|&y| game.terrain.is_solid(fx, y))
        .expect("no solid ground under soldier"));
    let before = game.crater_log.len();
    game.spawn_molotov_fire(WorldPos { x: tx as f32, y: ty as f32 });
    assert!(!game.terrain.is_solid(tx, ty), "molotov did not carve its impact crater");
    assert_eq!(game.crater_log.len(), before + 1, "molotov crater not pushed to the sync log");
    assert_eq!(game.crater_log.last().unwrap().2, 18.0, "WA molotov crater radius is 18");
    assert!(!game.fire_patches.is_empty(), "molotov spawned no fire");
}


/// Run the full sim and report (hp lost, max |dx| of the worm from its start, did it ever leave the ground).
fn burn(flames: &[(f32, f32)], ticks: u32) -> (u32, f32, bool) {
    let mut game = build();
    let base = flatten_around(&mut game);
    game.teams[0].soldiers[0].pos = WorldPos { x: base.x, y: base.y };
    game.teams[0].soldiers[0].state = SoldierState::Idle;
    game.teams[0].soldiers[0].hp = 100;
    let hp0 = game.teams[0].soldiers[0].hp;
    game.fire_patches = flames.iter().map(|&(dx, dy)| FirePatch {
        pos: WorldPos { x: base.x + dx, y: base.y + dy }, vel: Vec2 { x: 0.0, y: 0.0 },
        landed: true, lifetime: 4000, landed_ticks: 999, carves: false, splits: 0, dir: 0, timer: 0, cool: 0, wisp: 0, turns: false,
    }).collect();
    let input = InputState::new();
    game.tick = 0;
    let (mut far, mut air) = (0.0f32, false);
    for _ in 0..ticks {
        server_tick(&mut game, &input, None, None);
        let s = &game.teams[0].soldiers[0];
        far = far.max((s.pos.x - base.x).abs());
        air |= matches!(s.state, SoldierState::Airborne { .. });
    }
    ((hp0 - game.teams[0].soldiers[0].hp) as u32, far, air)
}

#[test]
fn flame_hit_shoves_the_worm_away_and_costs_one_hp_per_hit() {
    // A flame just left of the worm's body: hits shove it RIGHT and up (WA msg 0x4b: unit(flame->worm) + 0.3 up), 1 HP per hit.
    let (lost, far, air) = burn(&[(-3.0, 0.0)], 40);
    assert!(lost >= 1, "no flame hit in 40 ticks");
    assert!(air, "a hit must launch a grounded worm (shove)");
    assert!(far > 1.0, "worm was not shoved away (moved {far})");
    assert!(lost <= 12, "1 HP per hit, ~1 hit per 4-5 ticks per flame: lost {lost}");
}

#[test]
fn flames_out_of_reach_do_nothing() {
    let (lost, far, air) = burn(&[(60.0, 0.0), (-60.0, 0.0)], 60);
    assert_eq!((lost, air), (0, false));
    assert!(far < 0.5);
}

#[test]
fn many_flames_stay_bounded_one_hit_per_worm_per_frame() {
    // 30 flames on the worm: at most 1 HP per WA frame (1-2 per tick), never an instant kill.
    let flames: Vec<(f32, f32)> = (0..30).map(|k| (-15.0 + k as f32, 0.0)).collect();
    let (lost, _, _) = burn(&flames, 12);
    assert!(lost >= 1 && lost <= 24, "lost {lost} HP in 12 ticks");
}

#[test]
fn airborne_flame_falls_and_rests_without_sliding() {
    let mut game = build();
    let base = flatten_around(&mut game);
    game.teams[0].soldiers[0].pos.x = base.x - 150.0; // keep the worm out of the way
    game.fire_patches = vec![FirePatch {
        pos: WorldPos { x: base.x + 50.0, y: base.y - 40.0 }, vel: Vec2 { x: 0.0, y: 0.0 },
        landed: false, lifetime: 4000, landed_ticks: 0, carves: false, splits: 0, dir: 0, timer: 0, cool: 0, wisp: 0, turns: false,
    }];
    let input = InputState::new();
    game.tick = 0;
    for _ in 0..120 { server_tick(&mut game, &input, None, None); }
    let f = &game.fire_patches[0];
    assert!(f.landed, "flame never landed");
    assert!((f.pos.y - base.y).abs() <= 2.0, "flame did not rest on the floor (y {} vs {})", f.pos.y, base.y);
    assert!((f.pos.x - (base.x + 50.0)).abs() < 25.0, "flame drifted far (x {})", f.pos.x);
    let p = f.pos;
    for _ in 0..60 { server_tick(&mut game, &input, None, None); }
    assert_eq!(game.fire_patches[0].pos, p, "a resting flame must not move");
}

#[test]
fn molotov_is_a_wa_flame_chain_that_lasts_five_turns() {
    // wa_rig flame_spawn/flame_life: 1 flame falling at 4 px/frame, then a pair every 4 frames crawling left/right: 41 flames, 3-6 px
    // apart. They do NOT burn out by themselves (all 41 alive after 55 s): each loses a fifth per TURN END (msg 0x18) and ~a fifth per
    // worm hit, so they last 5 turn ends (gone at the 6th) unless worms keep walking through them.
    let mut game = build();
    let base = flatten_around(&mut game);
    game.teams[0].soldiers[0].pos.x = base.x + 160.0; // out of the way
    game.fire_patches.clear();
    game.spawn_molotov_fire(WorldPos { x: base.x, y: base.y - 12.0 });
    let input = InputState::new();
    game.tick = 0;
    let turn0 = game.turn.turn_number;
    let (mut max_n, mut minx, mut maxx) = (0usize, f32::MAX, f32::MIN);
    for _ in 0..1500 { // 50 s: nothing dies without turn ends
        server_tick(&mut game, &input, None, None);
        max_n = max_n.max(game.fire_patches.len());
        for f in &game.fire_patches { minx = minx.min(f.pos.x); maxx = maxx.max(f.pos.x); }
    }
    assert!(max_n >= 38 && max_n <= 41, "WA spawns 41 flames in all, saw {max_n} alive at once");
    assert!(game.fire_patches.len() >= 38, "flames must outlast a minute, {} left", game.fire_patches.len());
    println!("chain spread {:.1} px (left {:.1}, right {:.1}), max alive {max_n}", maxx - minx, base.x - minx, maxx - base.x);
    assert!(maxx - minx > 60.0 && maxx - minx < 260.0, "chain spread {}", maxx - minx);
    let already = (game.turn.turn_number - turn0) as usize; // turn ends the sim itself already delivered
    assert!(already <= 1, "test assumes at most one real turn end ({already})");
    for _ in already..5 { game.fire_turn_end(); server_tick(&mut game, &input, None, None); }
    assert!(game.fire_patches.len() >= 38, "five turn ends must not kill them ({} left)", game.fire_patches.len());
    game.fire_turn_end(); server_tick(&mut game, &input, None, None);
    assert!(game.fire_patches.is_empty(), "the sixth turn end removes them ({} left)", game.fire_patches.len());
}

#[test]
fn each_worm_hit_costs_a_molotov_flame_most_of_a_turn_step() {
    let mut game = build();
    let base = flatten_around(&mut game);
    game.teams[0].soldiers[0].pos = WorldPos { x: base.x, y: base.y };
    game.teams[0].soldiers[0].state = SoldierState::Idle;
    game.fire_patches = vec![FirePatch {
        pos: WorldPos { x: base.x - 3.0, y: base.y }, vel: Vec2 { x: 0.0, y: 0.0 }, landed: true,
        lifetime: arty::game::state::GameState::MOLOTOV_FLAME_LIFE, landed_ticks: 0, carves: false, splits: 0, dir: 0, timer: 0, cool: 0, wisp: 0, turns: true,
    }];
    let input = InputState::new();
    let before = game.fire_patches[0].lifetime;
    for _ in 0..40 { server_tick(&mut game, &input, None, None); if game.fire_patches.is_empty() { break; } }
    let lost = before - game.fire_patches.first().map(|f| f.lifetime).unwrap_or(0);
    assert!(lost >= 90, "a worm hit must cost the flame ~90 units (lost {lost})");
}

#[test]
#[ignore] // prints the Arty-side numbers compared with the wa_rig worm_fire run (victim 10 px left of the impact, dropped 14 frames after it: 5 hits / 5 HP)
fn rig_comparison_worm_dropped_into_molotov_fire() {
    for seed in 0..8u32 {
        let mut game = build();
        let base = flatten_around(&mut game);
        game.teams[0].soldiers[0].pos.x = base.x + 160.0;
        game.fire_patches.clear();
        game.tick = seed * 7;
        game.spawn_molotov_fire(WorldPos { x: base.x, y: base.y - 10.0 });
        let input = InputState::new();
        let mut placed = false;
        let (mut lost, mut ticks_hit, mut far) = (0u32, 0u32, 0.0f32);
        for t in 0..400 {
            server_tick(&mut game, &input, None, None);
            if t == 8 && !placed { // ~14 WA frames
                placed = true;
                game.teams[0].soldiers[1].pos = WorldPos { x: base.x - 10.0, y: base.y };
                game.teams[0].soldiers[1].state = SoldierState::Idle;
                game.teams[0].soldiers[1].hp = 100;
            }
            if placed {
                let s = &game.teams[0].soldiers[1];
                far = far.max((s.pos.x - (base.x - 10.0)).abs());
                let l = 100 - s.hp as i32; if l as u32 > lost { ticks_hit += 1; lost = l as u32; }
            }
        }
        println!("seed {seed}: HP lost {lost} ({ticks_hit} hit-ticks), max displacement {far:.1} px");
    }
}

#[test]
fn molotov_chain_matches_the_wa_rig_on_the_same_terrain() {
    // The terrain is the real WA land bitmap of the rig run (tests/data/wa_molotov_land.bin); the same impact point.
    // WA (tests/data/wa_molotov_flames.txt): 41 flames, x 673..733, y 607..632 (a pit with walls: chains reverse there).
    let land = std::fs::read("tests/data/wa_molotov_land.bin").unwrap();
    let mut game = build();
    for y in 0..696i32 { for x in 0..1920i32 {
        game.terrain.set_solid(x, y, (land[240 * y as usize + (x as usize >> 3)] >> (x & 7)) & 1 == 1);
    } }
    for t in 0..game.teams.len() { for i in 0..game.teams[t].soldiers.len() { game.teams[t].soldiers[i].pos = WorldPos { x: 100.0 + 5.0 * i as f32, y: 20.0 }; } }
    game.fire_patches.clear();
    game.spawn_molotov_fire(WorldPos { x: 719.08, y: 616.0 });
    let input = InputState::new();
    game.tick = 0;
    let (mut seen, mut minx, mut maxx, mut miny, mut maxy) = (0usize, f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    let mut peak = 0usize;
    for _ in 0..200 {
        server_tick(&mut game, &input, None, None);
        peak = peak.max(game.fire_patches.len());
        seen = seen.max(game.fire_patches.len());
        for f in &game.fire_patches { minx = minx.min(f.pos.x); maxx = maxx.max(f.pos.x); miny = miny.min(f.pos.y); maxy = maxy.max(f.pos.y); }
    }
    println!("Arty on the rig terrain: peak {peak} flames, x {minx:.0}..{maxx:.0}, y {miny:.0}..{maxy:.0}   (WA: 41 flames, x 673..733, y 607..632)");
    assert!(peak >= 36 && peak <= 41);
}
