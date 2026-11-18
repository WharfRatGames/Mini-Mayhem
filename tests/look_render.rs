use arty::renderer::buffer::WorldBuffer;
use arty::renderer::skeleton::{draw_soldier_skeletal, SoldierAnim};
use arty::world::{WorldPos, WORLD_W};

fn draw(buf: &mut WorldBuffer, x: f32, y: f32, look: u8, facing: i8, hp: u8, anim: &SoldierAnim, aim: Option<f32>, hat: u8, boots: u8, gun: u8, wind: f32, tick: u32) {
    buf.begin_scale(x, y, 0.6);
    draw_soldier_skeletal(buf, WorldPos::new(x, y), 0, facing, hp, anim, aim, false, hat, look << 5, boots, gun, None, wind, tick, 0);
    buf.end_scale();
}

#[test]
fn render_looks_sheet() {
    let mut buf = WorldBuffer::new();
    // row 0: propeller hat, wind -0.8 / 0 / +0.8 at a few ticks
    for (i, (w, t)) in [(-0.8f32, 0u32), (-0.8, 4), (0.0, 0), (0.0, 4), (0.8, 0), (0.8, 4), (0.8, 8)].iter().enumerate() {
        draw(&mut buf, 30.0 + i as f32 * 40.0, 40.0, 0, 1, 100, &SoldierAnim::Idle, None, 2, 0, 0, *w, *t);
    }
    // row 1: aiming with plasma gun at several angles, right then left
    for (i, a) in [-0.6f32, -0.2, 0.0, 0.4, 0.9].iter().enumerate() {
        draw(&mut buf, 30.0 + i as f32 * 40.0, 90.0, 2, 1, 100, &SoldierAnim::Idle, Some(*a), 4, 3, 6, 0.0, 0);
    }
    draw(&mut buf, 240.0, 90.0, 2, -1, 100, &SoldierAnim::Idle, Some(0.5), 4, 3, 6, 0.0, 0);
    // row 2: walking Viking + shotgun, spin with top hat, dead with halo, rope with samurai
    draw(&mut buf, 30.0, 140.0, 1, 1, 100, &SoldierAnim::Walking { tick: 3 }, None, 15, 2, 2, 0.0, 0);
    draw(&mut buf, 70.0, 140.0, 3, 1, 100, &SoldierAnim::Walking { tick: 6 }, None, 31, 5, 10, 0.0, 0);
    draw(&mut buf, 110.0, 140.0, 4, 1, 100, &SoldierAnim::Airborne { vel_x: 1.0, vel_y: 0.0, airtime: 5, spinning: true }, None, 1, 0, 1, 0.0, 0);
    draw(&mut buf, 150.0, 140.0, 0, 1, 0, &SoldierAnim::Dead, None, 8, 0, 0, 0.0, 0);
    draw(&mut buf, 190.0, 140.0, 2, -1, 100, &SoldierAnim::Idle, None, 36, 4, 17, 0.0, 0);
    draw(&mut buf, 230.0, 140.0, 1, 1, 100, &SoldierAnim::Idle, None, 28, 1, 0, 0.0, 0);
    let (w, h) = (300usize, 170usize);
    let mut ppm = format!("P6\n{} {}\n255\n", w, h).into_bytes();
    for y in 0..h { for x in 0..w {
        let o = (y * WORLD_W as usize + x) * 4;
        ppm.extend_from_slice(&[buf.raw_data()[o+2], buf.raw_data()[o+1], buf.raw_data()[o]]);
    }}
    std::fs::write("/tmp/claude-1000/-home-dusty-worms-decomp/ab288af4-e5b4-418a-b639-953a43c8553a/scratchpad/looks.ppm", ppm).unwrap();
}
