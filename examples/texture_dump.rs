//! Dev verification: render terrain WITH the real texture-atlas path to PNGs, so
//! texture garble (mega-tiles, bad sampling) can be eyeballed off-device.
//!
//! Unlike terrain_dump (flat dirt), this uses the exact device render —
//! `build_world_cache` → `terrain_pixel` → `atlas_sample` → `tiles()`.
//!
//!   cargo run --example texture_dump -- 0xBF6B76EB            # one seed (hex or dec)
//!   cargo run --example texture_dump -- --sweep 300           # 300 random seeds
//!
//! Each PNG is named with the FULL u64 seed (hex) and the selected tile index
//! (surface_texture % pool_len) so a garbled map maps straight to its tile.

use std::fs::{self, File};
use std::io::BufWriter;

use arty::world::{Terrain, WORLD_W, WORLD_H};
use arty::renderer::WorldBuffer;
use arty::renderer::draw_terrain::build_world_cache;

/// Mirror generate_tactical's first two lcg draws to recover the selected tile.
/// archetype = lcg(seed); surface_texture = lcg(again) as u8.
fn surface_texture(seed: u64) -> u8 {
    let mut s = seed;
    let mut lcg = |s: &mut u64| {
        *s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        *s >> 33
    };
    let _arch = lcg(&mut s);
    lcg(&mut s) as u8
}

fn parse_seed(a: &str) -> Option<u64> {
    let a = a.trim();
    if let Some(h) = a.strip_prefix("0x").or_else(|| a.strip_prefix("0X")) {
        u64::from_str_radix(h, 16).ok()
    } else if a.chars().all(|c| c.is_ascii_digit()) {
        a.parse().ok()
    } else {
        // bare hex (e.g. a 16-digit seed copied from the HUD)
        u64::from_str_radix(a, 16).ok()
    }
}

fn render_seed(out_dir: &str, seed: u64) {
    let terrain = Terrain::generate_tactical(seed);
    let mut cache = WorldBuffer::new();
    build_world_cache(&mut cache, &terrain);

    let (w, h) = (WORLD_W as usize, WORLD_H as usize);
    let mut img = vec![0u8; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let p = cache.get_pixel(x as i32, y as i32);
            let i = (y * w + x) * 3;
            img[i] = p.r;
            img[i + 1] = p.g;
            img[i + 2] = p.b;
        }
    }

    let tex = surface_texture(seed);
    let path = format!("{out_dir}/seed_{seed:016X}_tex{tex:03}.png");
    let file = File::create(&path).expect("create png");
    let mut enc = png::Encoder::new(BufWriter::new(file), WORLD_W, WORLD_H);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&img).unwrap();
    println!("seed {seed:016X}  surface_texture={tex}  -> {path}");
}

fn main() {
    let out_dir = "/tmp/arty_texture";
    fs::create_dir_all(out_dir).expect("create out dir");

    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(|s| s == "--tiles").unwrap_or(false) {
        // Render ONE fixed terrain with every tile index 0..pool forced, to isolate
        // a garbled *tile* from terrain shape. Pool is 32 today; render 0..40 to be safe.
        let seed = args.get(1).and_then(|s| parse_seed(s)).unwrap_or(7);
        for idx in 0u8..40 {
            let mut terrain = Terrain::generate_tactical(seed);
            terrain.surface_texture = idx;
            let mut cache = WorldBuffer::new();
            build_world_cache(&mut cache, &terrain);
            let (w, h) = (WORLD_W as usize, WORLD_H as usize);
            let mut img = vec![0u8; w * h * 3];
            for y in 0..h { for x in 0..w {
                let p = cache.get_pixel(x as i32, y as i32);
                let i = (y * w + x) * 3;
                img[i] = p.r; img[i + 1] = p.g; img[i + 2] = p.b;
            }}
            let path = format!("{out_dir}/tile_{idx:03}.png");
            let file = File::create(&path).expect("create png");
            let mut enc = png::Encoder::new(BufWriter::new(file), WORLD_W, WORLD_H);
            enc.set_color(png::ColorType::Rgb);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&img).unwrap();
            println!("tile {idx} -> {path}");
        }
    } else if args.first().map(|s| s == "--sweep").unwrap_or(false) {
        let n: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(200);
        // splitmix64 over a time-ish base → spread of full-u64 seeds.
        let mut s = 0x9E3779B97F4A7C15u64;
        for _ in 0..n {
            s = s.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = s;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^= z >> 31;
            render_seed(out_dir, z);
        }
    } else if args.is_empty() {
        println!("usage: texture_dump -- <seed|0xSEED>...  |  texture_dump -- --sweep N");
    } else {
        for a in &args {
            match parse_seed(a) {
                Some(seed) => render_seed(out_dir, seed),
                None => eprintln!("could not parse seed: {a}"),
            }
        }
    }
}
