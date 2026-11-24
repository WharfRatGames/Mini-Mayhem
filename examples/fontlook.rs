//! Render the update-screen changelog text at the real scale into /tmp/fontlook.png.
use arty::renderer::{buffer::WorldBuffer, fb::Bgra, font::{draw_str_scaled, wrap_text}};
use arty::world::WORLD_W;
fn main() {
    let mut buf = WorldBuffer::new();
    buf.fill_rect(0, 0, 400, 130, Bgra::new(12, 14, 30));
    let lines = ["OTA test build: nothing new in the game.", "If you can read this, the update screen works.", "Donkey falls at WA speed (drag 0.9), flames: 12."];
    let mut y = 6;
    for l in lines { for w in wrap_text(l, 2, 364) { draw_str_scaled(&mut buf, &w, 18, y, Bgra::new(190, 205, 230), 2); y += 22; } }
    let (w, h, z) = (400usize, 130usize, 3usize);
    let mut img = vec![0u8; w * z * h * z * 3];
    let d = buf.raw_data();
    for yy in 0..h * z { for xx in 0..w * z {
        let o = ((yy / z) * WORLD_W as usize + xx / z) * 4; let p = (yy * w * z + xx) * 3;
        img[p] = d[o + 2]; img[p + 1] = d[o + 1]; img[p + 2] = d[o];
    } }
    let f = std::fs::File::create("/tmp/fontlook.png").unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), (w * z) as u32, (h * z) as u32);
    e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img).unwrap();
}
