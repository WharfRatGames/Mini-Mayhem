//! Dev verification: dump generated terrain to PNGs so the map archetypes can be
//! eyeballed (caves, overhangs, sky islands) without a device.
//!
//!   cargo run --example terrain_dump            # seeds 0..30 -> /tmp/arty_terrain
//!   cargo run --example terrain_dump 7 12 99    # specific seeds
//!
//! Spawns are drawn as red squares so you can confirm they land on real terrain in
//! the interior band (never at the very edges).

use std::fs::{self, File};
use std::io::BufWriter;

use arty::world::{Terrain, WORLD_W, WORLD_H};
use arty::renderer::buffer::WorldBuffer;
use arty::renderer::draw_terrain::draw_terrain;

fn archetype_of(seed: u64) -> u64 {
    // Mirror generate_tactical's first lcg draw: archetype = lcg(seed) % 5.
    let s = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (s >> 33) % 5
}

const NAMES: [&str; 5] = [
    "hills", "cliffs", "islands", "caverns", "canyon",
];

fn main() {
    let out_dir = "/tmp/arty_terrain";
    fs::create_dir_all(out_dir).expect("create out dir");

    let args: Vec<String> = std::env::args().skip(1).collect();
    let seeds: Vec<u64> = if args.is_empty() {
        (0..30u64).collect()
    } else {
        args.iter().filter_map(|a| a.parse().ok()).collect()
    };

    for seed in seeds {
        let mut terrain = Terrain::generate_tactical(seed);
        let team0 = terrain.find_team_spawns(0, WORLD_W / 2 - 40, 4);
        let team1 = terrain.find_team_spawns(WORLD_W / 2 + 40, WORLD_W, 4);

        let w = WORLD_W as usize;
        let h = WORLD_H as usize;
        let mut img = vec![0u8; w * h * 3];

        // Render through the real game renderer (dirt texture, archetype sky,
        // edge shading, wet zone) so the preview matches in-game appearance.
        let mut buf = WorldBuffer::new();
        draw_terrain(&mut buf, &terrain);
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 3;
                let px = buf.get_pixel(x as i32, y as i32);
                img[i] = px.r;
                img[i + 1] = px.g;
                img[i + 2] = px.b;
            }
        }

        // Spawn markers (red squares).
        for sp in team0.iter().chain(team1.iter()) {
            let sx = sp.x as i32;
            let sy = sp.y as i32;
            for dy in -3..=3 {
                for dx in -3..=3 {
                    let px = sx + dx;
                    let py = sy + dy;
                    if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 { continue; }
                    let i = (py as usize * w + px as usize) * 3;
                    img[i] = 255;
                    img[i + 1] = 30;
                    img[i + 2] = 30;
                }
            }
        }

        let arch = archetype_of(seed);
        let path = format!("{out_dir}/seed{seed:03}_{}.png", NAMES[arch as usize]);
        let file = File::create(&path).expect("create png");
        let mut enc = png::Encoder::new(BufWriter::new(file), WORLD_W, WORLD_H);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&img).unwrap();
        println!(
            "seed {seed:>3} -> {:8} solid={:>7} (team0 {}, team1 {}) {path}",
            NAMES[arch as usize], terrain.solid_count(), team0.len(), team1.len()
        );
    }
}
