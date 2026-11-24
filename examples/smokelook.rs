//! Render steam puffs (flame hitting water) at a few ages into /tmp/smokelook.png.
use arty::game::loop_runner::{render, LoopState};
use arty::renderer::{buffer::WorldBuffer, camera::Camera, fx::{FxEvent, step_fx}};
use arty::world::{WATER_Y, WORLD_W};
fn main() {
    let mut g = arty::replay::new_game(0x18BD410020ECA8CD, &[0, 1]);
    let (cx, cy) = (900.0f32, WATER_Y as f32 - 2.0);
    g.emit_fx(FxEvent::Smoke { x: cx, y: cy, count: 12 });
    let mut ls = LoopState::new(); let mut buf = WorldBuffer::new();
    let cam = Camera::new(cx, cy - 30.0);
    let (w, h, z, frames) = (48usize, 48usize, 6usize, 4usize);
    let mut img = vec![0u8; w * z * frames * h * z * 3];
    for f in 0..frames {
        for _ in 0..(if f == 0 { 6 } else { 12 }) { let t = std::mem::take(&mut g.fx); let mut t = t; step_fx(&mut t, &g.terrain, 0.0); g.fx = t; }
        render(&g, &mut buf, &cam, &mut ls);
        let d = buf.raw_data();
        for y in 0..h * z { for x in 0..w * z {
            let (sx, sy) = ((cx as usize - w / 2) + x / z, (cy as usize - 40) + y / z);
            let o = (sy * WORLD_W as usize + sx) * 4; let p = (y * w * z * frames + f * w * z + x) * 3;
            img[p] = d[o + 2]; img[p + 1] = d[o + 1]; img[p + 2] = d[o];
        } }
    }
    let f = std::fs::File::create("/tmp/smokelook.png").unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), (w * z * frames) as u32, (h * z) as u32);
    e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img).unwrap();
}
