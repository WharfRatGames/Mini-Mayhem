//! Replay WA's walking (state 0x66) through `physics::wa_worm` on a WA land bitmap, one JSON line per 50 Hz frame, for tools/wa_rig.
//!   cargo run --release --example wa_walk -- --land land.bin --pos X,Y --dir -1 --frames 150
//! pos = WA body centre in 16.16 as logged by the rig. Land bitmap: 1-bpp, stride 240, LSB = left pixel (same as wa_flight).
use arty::physics::wa_worm::{WalkState, WormBody};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let get = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let v = get("--pos").expect("--pos"); let mut it = v.split(',');
    let (x, y): (i32, i32) = (it.next().unwrap().parse().unwrap(), it.next().unwrap().parse().unwrap());
    let dir: i32 = get("--dir").map(|d| d.parse().unwrap()).unwrap_or(1);
    let frames: usize = get("--frames").map(|v| v.parse().unwrap()).unwrap_or(100);
    let bits = std::fs::read(get("--land").expect("--land")).expect("land file");
    let (w, h) = (1920i32, 696i32);
    let solid = |px: i32, py: i32| px >= 0 && px < w && py >= 0 && py < h && (bits[py as usize * 240 + (px as usize >> 3)] >> (px & 7)) & 1 == 1;
    let mut b = WormBody { x, y, vx: 0, vy: 0 };
    let mut ws = WalkState::start(x, dir);
    println!("{{\"i\":0,\"x\":{},\"y\":{},\"r\":1}}", b.x, b.y);
    for i in 1..=frames {
        let r = b.walk_frame(&mut ws, &solid);
        // WA (FUN_005140e0): a blocked step (0) drops the worm to state 0x65; the held key then restarts the walk at the current x.
        if r == 0 && a.iter().any(|v| v == "--restart") { ws = WalkState::start(b.x, dir); }
        println!("{{\"i\":{i},\"x\":{},\"y\":{},\"r\":{r}}}", b.x, b.y);
        if r < 0 { break; }
    }
}
