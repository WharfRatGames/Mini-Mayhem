//! Dump `Terrain::generate_tactical` silhouettes as 1-bpp bitmaps for the
//! MapGen-similarity stats harness (`tools/terrain_stats.py`).
//!
//! Usage: cargo run --bin dump-terrain -- OUT_DIR [N_SEEDS] [START_SEED]
//! Writes OUT_DIR/{island|cavern}/seedNNNN.bin — 1920x960, row-major,
//! MSB-first (WORLD_W * WORLD_H / 8 bytes), plus a .meta line file.

use arty::world::constants::{WORLD_H, WORLD_W};
use arty::world::terrain::Terrain;
use std::io::Write;

fn main() {
    let mut args = std::env::args().skip(1);
    let out_dir = args.next().expect("usage: dump-terrain OUT_DIR [N] [START]");
    let n: u64 = args.next().map_or(200, |s| s.parse().unwrap());
    let start: u64 = args.next().map_or(1, |s| s.parse().unwrap());

    for kind in ["island", "cavern"] {
        std::fs::create_dir_all(format!("{out_dir}/{kind}")).unwrap();
    }
    let (w, h) = (WORLD_W as u32, WORLD_H as u32);
    for seed in start..start + n {
        let t = Terrain::generate_tactical(seed);
        let kind = if t.is_cavern { "cavern" } else { "island" };
        let mut bits = vec![0u8; (w * h / 8) as usize];
        for y in 0..h {
            for x in 0..w {
                if t.is_solid_unchecked(x, y) {
                    bits[(y * w / 8 + x / 8) as usize] |= 1 << (7 - (x & 7));
                }
            }
        }
        let path = format!("{out_dir}/{kind}/seed{seed:06}.bin");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bits)
            .unwrap();
        if seed % 50 == 0 {
            eprintln!("{seed}");
        }
    }
    eprintln!("done: {n} seeds -> {out_dir}");
}
