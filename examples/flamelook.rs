//! Render a Molotov's burning patches on flat ground into a zoomed PNG to compare with WA's (wa_rig flame_look).
use arty::game::loop_runner::{render, server_tick, LoopState};
use arty::input::InputState;
use arty::renderer::{buffer::WorldBuffer, camera::Camera};
use arty::world::{WorldPos, WORLD_W};
fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0x18BD410020ECA8CD);
    let mut g = arty::replay::new_game(seed, &[0, 1]);
    let vp = g.teams[1].soldiers[0].pos;
    g.spawn_molotov_fire(WorldPos::new(vp.x, vp.y - 4.0)); // the real Molotov burst
    g.turn.phase = arty::game::turn::TurnPhase::Watching;
    let mut ls = LoopState::new(); let mut buf = WorldBuffer::new();
    for t in 0..110 { server_tick(&mut g, &InputState::default(), None, None); let _ = t; }
    let l: Vec<_> = g.fire_patches.iter().filter(|p| p.landed).collect(); let n = l.len().max(1) as f32; let (mx, my) = (l.iter().map(|p| p.pos.x).sum::<f32>() / n, l.iter().map(|p| p.pos.y).sum::<f32>() / n); println!("landed {} at ({mx:.0},{my:.0})", l.len());
    let cam = Camera::new(mx, my); ls.tick = 110;
    render(&g, &mut buf, &cam, &mut ls);
    let _ = (cam.left_edge(), cam.top_edge());
    let (w, h, z) = (80usize, 40usize, 8usize);
    let mut img = vec![0u8; w * z * h * z * 3];
    let d = buf.raw_data();
    for y in 0..h * z { for x in 0..w * z {
        let (sx, sy) = ((mx as usize).saturating_sub(w / 2) + x / z, (my as usize).saturating_sub(24) + y / z);
        let o = (sy * WORLD_W as usize + sx) * 4; let p = (y * w * z + x) * 3;
        img[p] = d[o + 2]; img[p + 1] = d[o + 1]; img[p + 2] = d[o];
    } }
    let f = std::fs::File::create("/tmp/flamelook.png").unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), (w * z) as u32, (h * z) as u32);
    e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img).unwrap();
    println!("fire patches alive: {}", g.fire_patches.len());
}
