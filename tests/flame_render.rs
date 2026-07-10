//! Fire patches must render as the WA flame sprite: visible pixels in the
//! flame palette (red/orange/yellow/white), animated across ticks, shrinking
//! near expiry. Set FLAME_DUMP=/path/prefix to dump PPM crops for eyeballing.

use arty::game::loop_runner::{render, LoopState};
use arty::game::state::{FirePatch, GameState};
use arty::game::team::{Difficulty, Team};
use arty::renderer::buffer::WorldBuffer;
use arty::renderer::camera::Camera;
use arty::world::{Terrain, Vec2, WorldPos, WORLD_W};

fn build(seed: u64) -> GameState {
    let mut terrain = Terrain::generate_tactical(seed);
    let all = terrain.find_team_spawns(0, WORLD_W, 8);
    let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, s)| s).collect();
    let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s).collect();
    let teams = vec![
        Team::new(0, false, Difficulty::Medium, &t0),
        Team::new(1, false, Difficulty::Medium, &t1),
    ];
    GameState::new(seed, terrain, teams, 2)
}

/// Count pixels near (cx, cy) that differ from a baseline render without fire
/// AND are flame-coloured (red-dominant) — immune to red HUD text, which is
/// identical in both renders.
fn flame_pixels(buf: &WorldBuffer, base: &WorldBuffer, cx: i32, cy: i32) -> usize {
    let mut n = 0;
    for dy in -22i32..4 {
        for dx in -14i32..=14 {
            let p = buf.get_pixel(cx + dx, cy + dy);
            if p != base.get_pixel(cx + dx, cy + dy) && p.r > 150 && p.r > p.b.saturating_add(60) {
                n += 1;
            }
        }
    }
    n
}

fn dump(buf: &WorldBuffer, cx: i32, cy: i32, name: &str) {
    let Ok(prefix) = std::env::var("FLAME_DUMP") else { return };
    let (w, h) = (80i32, 60i32);
    let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
    for y in 0..h {
        for x in 0..w {
            let p = buf.get_pixel(cx - w / 2 + x, cy - h + 10 + y);
            out.extend([p.r, p.g, p.b]);
        }
    }
    std::fs::write(format!("{prefix}{name}.ppm"), out).unwrap();
}

#[test]
fn fire_patches_render_wa_flames() {
    let mut game = build(3);
    let pos = game.teams[0].soldiers[0].pos;
    let spot = WorldPos { x: pos.x + 40.0, y: pos.y };
    // Landed long-lived flame, landed dying flame, airborne spark.
    game.fire_patches = vec![
        FirePatch { pos: spot, vel: Vec2 { x: 1.3, y: -0.7 }, landed: true, lifetime: 500, landed_ticks: 0, carves: false },
        FirePatch { pos: WorldPos { x: spot.x + 30.0, y: spot.y }, vel: Vec2 { x: -2.1, y: 0.4 }, landed: true, lifetime: 20, landed_ticks: 0, carves: false },
        FirePatch { pos: WorldPos { x: spot.x - 30.0, y: spot.y - 40.0 }, vel: Vec2 { x: 0.5, y: 2.0 }, landed: false, lifetime: 300, landed_ticks: 0, carves: false },
    ];

    let mut cam = Camera::new(spot.x, spot.y);
    cam.snap_to(spot);
    let mut buf = WorldBuffer::new();
    render(&game, &mut buf, &cam, &mut LoopState::new());

    // Baseline render without fire for diff-based pixel counting.
    let patches = std::mem::take(&mut game.fire_patches);
    let mut base = WorldBuffer::new();
    render(&game, &mut base, &cam, &mut LoopState::new());
    game.fire_patches = patches;

    let full = flame_pixels(&buf, &base, spot.x as i32, spot.y as i32);
    let dying = flame_pixels(&buf, &base, spot.x as i32 + 30, spot.y as i32);
    let spark = flame_pixels(&buf, &base, spot.x as i32 - 30, spot.y as i32 - 40);
    dump(&buf, spot.x as i32, spot.y as i32, "landed");
    dump(&buf, spot.x as i32 + 30, spot.y as i32, "dying");
    dump(&buf, spot.x as i32 - 30, spot.y as i32 - 40, "spark");
    assert!(full > 60, "landed flame too small/invisible ({full} px)");
    assert!(dying > 3 && dying < full, "dying flame should be a small spark ({dying} vs {full})");
    assert!(spark > 3 && spark < full, "airborne spark should be small ({spark} vs {full})");

    // Animation: advancing lifetime must change the drawn frame.
    game.fire_patches[0].lifetime = 498;
    let mut buf2 = WorldBuffer::new();
    render(&game, &mut buf2, &cam, &mut LoopState::new());
    let mut diff = 0;
    for dy in -22i32..4 {
        for dx in -14i32..=14 {
            if buf.get_pixel(spot.x as i32 + dx, spot.y as i32 + dy)
                != buf2.get_pixel(spot.x as i32 + dx, spot.y as i32 + dy)
            {
                diff += 1;
            }
        }
    }
    assert!(diff > 10, "flame doesn't animate (diff={diff})");
}
