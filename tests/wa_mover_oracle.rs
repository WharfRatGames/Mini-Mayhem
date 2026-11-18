//! WA swept mover (FUN_004fbdc0) vs the real machine code under Unicorn (emu/gen_mover_oracle.py).
use arty::physics::wa_body::{sweep, Body};

#[test]
fn matches_wa_mover() {
    let txt = std::fs::read_to_string("tests/data/wa_mover_oracle.txt").unwrap();
    let mut lines = txt.lines();
    let (mut n, mut hits, mut sides) = (0, 0, [0; 5]);
    while let (Some(s), Some(r)) = (lines.next(), lines.next()) {
        let v: Vec<i64> = s.split_whitespace().skip(1).map(|t| t.parse().unwrap()).collect();
        let ver = v[0] as i32;
        let mut b = Body { f30: v[1] as i32, f38: v[2] as i32, fb0: v[3] as i32, x: v[4] as i32, y: v[5] as i32, vx: v[6] as i32, vy: v[7] as i32 };
        let mut frac = v[8] as i32;
        let nr = v[9] as usize;
        let rects: Vec<[i32; 4]> = (0..nr).map(|i| [v[10 + 4 * i] as i32, v[11 + 4 * i] as i32, v[12 + 4 * i] as i32, v[13 + 4 * i] as i32]).collect();
        let blocked = |x16: i32, y16: i32| { let (x, y) = (x16 >> 16, y16 >> 16); rects.iter().any(|r| r[0] <= x && x < r[2] && r[1] <= y && y < r[3]) };
        let e: Vec<i32> = r.split_whitespace().skip(1).map(|t| t.parse().unwrap()).collect();
        let out = sweep(&mut b, &mut frac, ver, &blocked);
        assert_eq!((out.hit as i32, if out.hit { out.side } else { e[1] }, b.x, b.y, frac), (e[0], e[1], e[2], e[3], e[4]), "scenario {n}: {s}");
        n += 1; hits += e[0]; sides[e[1] as usize] += 1;
    }
    assert!(n >= 4000 && hits >= 200, "n={n} hits={hits} sides={sides:?}");
    eprintln!("mover oracle: {n} scenarios, {hits} hits, sides {sides:?}");
}
