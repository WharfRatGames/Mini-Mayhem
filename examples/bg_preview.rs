//! Dev preview: composite background + terrain like the live renderer (minus
//! soldiers/HUD) and dump a viewport PNG so backgrounds can be eyeballed.
//!   cargo run --example bg_preview <seed-decimal> [cam_x]

use std::fs::File;
use std::io::BufWriter;

use arty::renderer::{bg_image, draw_terrain};
use arty::renderer::buffer::WorldBuffer;
use arty::world::{Terrain, SCREEN_W, SCREEN_H, WORLD_W};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seed: u64 = args.get(0).and_then(|a| a.parse().ok()).unwrap_or(0);
    let cam_x: u32 = args.get(1).and_then(|a| a.parse().ok())
        .unwrap_or(WORLD_W / 2 - SCREEN_W / 2);

    let terrain = Terrain::generate_tactical(seed);
    let mut cache = WorldBuffer::new();
    draw_terrain::build_world_cache(&mut cache, &terrain);

    let mut frame = WorldBuffer::new();
    // Simulate stale prior-frame content (e.g. title screen). After a correct
    // render no sentinel pixel should survive inside the viewport.
    frame.clear(arty::renderer::Bgra::new(255, 0, 255));
    bg_image::draw_static_bg(&mut frame, &terrain, seed, cam_x as i32);
    frame.copy_viewport_from_sky_aware(&cache, cam_x, &terrain);

    // Extract the viewport region into an RGB image.
    let w = SCREEN_W as usize;
    let h = SCREEN_H as usize;
    let raw = frame.raw(); // BGRA, full-world rows of WORLD_W
    let mut img = vec![0u8; w * h * 3];
    let mut ghosts = 0u32;
    for y in 0..h {
        for x in 0..w {
            let src = ((y as u32 * WORLD_W + cam_x + x as u32) * 4) as usize;
            let dst = (y * w + x) * 3;
            let (b, g, r) = (raw[src], raw[src + 1], raw[src + 2]);
            if b == 255 && g == 0 && r == 255 { ghosts += 1; }
            img[dst] = r;
            img[dst + 1] = g;
            img[dst + 2] = b;
        }
    }
    println!("sentinel (stale) pixels left in viewport: {ghosts}");

    let path = format!("/tmp/bg_preview_{seed}.png");
    let file = File::create(&path).expect("create png");
    let mut enc = png::Encoder::new(BufWriter::new(file), SCREEN_W, SCREEN_H);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&img).unwrap();
    println!("bg slot {} -> {path}", bg_image::bg_index_for_seed(seed));
}
