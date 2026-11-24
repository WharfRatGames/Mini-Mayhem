//! Render the first oil drum of a seed into a zoomed PNG (/tmp/drumlook.png) to check the drawn art.
use arty::game::loop_runner::{render, LoopState};
use arty::renderer::{buffer::WorldBuffer, camera::Camera};
use arty::world::WORLD_W;
fn main() {
    let seed: u64 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0x18BD410020ECA8CD);
    let g = arty::replay::new_game(seed, &[0, 1]);
    let Some(b) = g.barrels.first() else { println!("no barrels"); return };
    let (mx, my) = (b.pos.x, b.pos.y);
    let cam = Camera::new(mx, my);
    let (mut ls, mut buf) = (LoopState::new(), WorldBuffer::new());
    render(&g, &mut buf, &cam, &mut ls);
    let (w, h, z) = (40usize, 48usize, 10usize);
    let mut img = vec![0u8; w * z * h * z * 3];
    let d = buf.raw_data();
    for y in 0..h * z { for x in 0..w * z {
        let (sx, sy) = ((mx as usize).saturating_sub(w / 2) + x / z, (my as usize).saturating_sub(38) + y / z);
        let o = (sy * WORLD_W as usize + sx) * 4; let p = (y * w * z + x) * 3;
        img[p] = d[o + 2]; img[p + 1] = d[o + 1]; img[p + 2] = d[o];
    } }
    let f = std::fs::File::create("/tmp/drumlook.png").unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), (w * z) as u32, (h * z) as u32);
    e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img).unwrap();
    println!("drum at ({mx:.0},{my:.0})");
}
