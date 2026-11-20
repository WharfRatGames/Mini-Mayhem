//! Scale contract (docs/EXPERIMENTAL_PLAN.md step 4). The drawn soldier (~46 px) is about 3x the collision body (16 px), so jump
//! height and walk pace are judged against what the player SEES. These tests fail if the art scale and the movement scales drift
//! apart again, as happened when the jump arc stayed shrunk for 23 px soldiers after the art doubled.
use arty::game::loop_runner::{walk_frames_for_age, JUMP_SCALE};
use arty::physics::wa_worm as ww;
use arty::renderer::draw_sprites::{drawn_soldier_height, BODY_H};

/// Apex height (px above takeoff) of a WA jump with the given takeoff velocities (16.16 px/frame), on open air.
fn apex(vx16: i32, vy16: i32) -> f32 {
    let k = JUMP_SCALE;
    let (vx16, vy16) = ((vx16 as f64 * k).round() as i32, (vy16 as f64 * k).round() as i32);
    let mut b = ww::body_from_foot(500.0, 500.0, vx16, vy16);
    let y0 = b.y >> 16;
    let mut top = y0;
    let open = |_x: i32, _y: i32| false;
    for _ in 0..200 {
        b.step_air(&open);
        top = top.min(b.y >> 16);
        if b.vy > 0 { break; }
    }
    (y0 - top) as f32
}

#[test]
fn backflip_clears_most_of_a_body_height() {
    let h = drawn_soldier_height();
    let a = apex(ww::JUMP_BACKFLIP_VX, ww::JUMP_BACKFLIP_VY);
    println!("drawn soldier {h:.0} px, collision body {BODY_H} px, backflip apex {a:.0} px ({:.2} drawn heights)", a / h);
    assert!(a >= 0.75 * h, "backflip apex {a} px is under 0.75 x the drawn soldier ({h} px): it would not clear a body-high obstacle");
}

#[test]
fn forward_jump_is_not_a_hop() {
    let h = drawn_soldier_height();
    let a = apex(ww::JUMP_FWD_VX, ww::JUMP_FWD_VY);
    println!("forward jump apex {a:.0} px ({:.2} drawn heights)", a / h);
    assert!(a >= 0.5 * h, "forward jump apex {a} px is under half the drawn soldier ({h} px)");
}

/// Walk pace in px/s on flat ground using the real frame pattern (`walk_frames_for_age`) at 30 ticks/s.
fn walk_px_per_s() -> f32 {
    let ground = 1000;
    let solid = |_x: i32, y: i32| y >= ground;
    let mut body = ww::body_from_foot(500.0, (ground - 1) as f32, 0, 0);
    let mut ws = ww::WalkState::start(body.x, 1);
    let x0 = body.x;
    let ticks = 90u32; // 3 s
    for age in 0..ticks { for _ in 0..walk_frames_for_age(age) { body.walk_frame(&mut ws, &solid); } }
    ((body.x - x0) as f32 / 65536.0) / (ticks as f32 / 30.0)
}

#[test]
fn walk_pace_reads_as_walking_not_crawling() {
    let h = drawn_soldier_height();
    let v = walk_px_per_s();
    println!("walk {v:.0} px/s = {:.2} drawn heights/s ({:.2} collision bodies/s; WA itself is ~1.75)", v / h, v / BODY_H as f32);
    // Floor from play feedback: 0.6 heights/s (WA's own pixel pace on the 46 px soldiers) read as too slow, 0.88 (1.5x) is the current
    // setting under test. Raise this floor when the pace is retuned.
    assert!(v / h >= 0.8, "walking {v:.0} px/s is {:.2} drawn heights/s, below the 0.8 floor", v / h);
}

#[test]
fn walk_frame_pattern_averages_the_configured_rate() {
    let n = 600u32;
    let total: u32 = (0..n).map(walk_frames_for_age).sum();
    // 5 frames per 2 ticks
    assert_eq!(total, n * 5 / 2);
    assert!((0..n).all(|a| matches!(walk_frames_for_age(a), 2 | 3)));
}

#[test]
fn map_objects_keep_wa_proportions_and_spawn_from_the_shared_pool() {
    // Art scale ~1.2x (trimmed from the full 1.7x after on-device feedback): a drum is about as tall as a soldier.
    let k = arty::game::loop_runner::object_art_scale();
    assert!(k > 1.0 && k < 2.2, "scale {k}");
    let drum = 36.0 * k / arty::renderer::draw_sprites::drawn_soldier_height();
    assert!(drum > 0.8 && drum < 1.3, "drum/soldier height {drum}");
    // Objects continue after the worms in WA's one placement pass: never within the 107 px exclusion of a worm.
    for seed in [1u64, 7, 42, 1234] {
        let g = arty::game::setup::build_game_n(seed, &[0, 1]);
        for m in &g.mines {
            for s in g.teams.iter().flat_map(|t| t.soldiers.iter()) {
                assert!((m.pos.x - s.pos.x).abs() > 40.0 || (m.pos.y - s.pos.y).abs() > 40.0, "seed {seed}: mine on a worm");
            }
        }
        assert!(g.mines.len() <= arty::game::setup::MAX_MINES && g.barrels.len() <= arty::game::setup::MAX_BARRELS);
    }
}
