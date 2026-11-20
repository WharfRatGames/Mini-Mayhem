//! Replay one WA thrown grenade through `physics::wa_thrown` on WA's own land bitmap (wa_mapgen) and print one JSON line per
//! 50 Hz frame, for tools/wa_rig `compare-grenade`.
//!   cargo run --release --example wa_flight -- --seed 12345 --cavern 0 --style 0 --pos X,Y --vel VX,VY --fuse MS [--frames N] [--wind16 W] [--max-bounce]
//! pos/vel are WA 16.16 integers (as logged by the rig).
use arty::physics::wa_thrown::Thrown;
use arty::world::wa_mapgen;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let pair = |k: &str| -> (i32, i32) { let v = get(k).unwrap_or_else(|| panic!("missing {k}")); let mut it = v.split(','); (it.next().unwrap().parse().unwrap(), it.next().unwrap().parse().unwrap()) };
    let seed: u32 = get("--seed").unwrap().parse().unwrap();
    let cavern = get("--cavern").map(|v| v == "1").unwrap_or(false);
    let style: usize = get("--style").map(|v| v.parse().unwrap()).unwrap_or(0);
    let (x, y) = pair("--pos");
    let (vx, vy) = pair("--vel");
    let fuse: i32 = get("--fuse").map(|v| v.parse().unwrap()).unwrap_or(3000);
    let frames: usize = get("--frames").map(|v| v.parse().unwrap()).unwrap_or(200);
    let wind16: i32 = get("--wind16").map(|v| v.parse().unwrap()).unwrap_or(0);
    let max_bounce = a.iter().any(|v| v == "--max-bounce");
    let (w, h) = (1920i32, 696i32);
    let bits: Option<Vec<u8>> = get("--land").map(|f| std::fs::read(f).expect("land file")); // WA canvas: 1-bpp, stride 240, LSB = left pixel
    let gen = if bits.is_none() { Some(wa_mapgen::generate(seed, cavern, style)) } else { None };
    let is_land = |px: usize, py: usize| -> bool {
        match (&bits, &gen) { (Some(b), _) => (b[py * 240 + (px >> 3)] >> (px & 7)) & 1 == 1, (_, Some(g)) => g.land[py * g.w + px], _ => false }
    };
    // outside the bitmap: left/right/top open, below the bitmap = water (open); tune if the capture disagrees
    let solid = |px: i32, py: i32| px >= 0 && px < w && py >= 0 && py < h && is_land(px as usize, py as usize);
    if let Some(c) = get("--col") { // debug: run lengths of land along one column
        let c: i32 = c.parse().unwrap(); let mut prev = false; let mut out = vec![];
        for yy in 0..h { let v = solid(c, yy); if v != prev { out.push(yy); prev = v; } }
        eprintln!("column {c}: land toggles at y = {:?} (land {}x{}, style {style} cavern {cavern})", out, w, h);
    }
    let worms: Vec<(i32, i32)> = get("--worms").map(|v| v.split(';').filter(|t| !t.is_empty()).map(|t| { let mut it = t.split(','); (it.next().unwrap().parse().unwrap(), it.next().unwrap().parse().unwrap()) }).collect()).unwrap_or_default();
    let mut g = Thrown::grenade(x, y, vx, vy, fuse, max_bounce);
    println!("{{\"i\":0,\"x\":{},\"y\":{},\"vx\":{},\"vy\":{},\"fuse\":{},\"hits\":0}}", g.x, g.y, g.vx, g.vy, g.fuse_ms);
    for i in 1..=frames {
        let f = g.step_with(&solid, wind16, &worms);
        println!("{{\"i\":{i},\"x\":{},\"y\":{},\"vx\":{},\"vy\":{},\"fuse\":{},\"hits\":{}}}", g.x, g.y, g.vx, g.vy, g.fuse_ms, f.hits);
        if f.exploded { break; }
    }
}
