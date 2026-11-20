//! The sheep walker (`physics::wa_sheep`) against WA's real FUN_0050a900, run under Unicorn (emu/gen_sheep_oracle.py).
//! Each scenario is one 50 Hz walking frame of a grounded sheep on a rectangle terrain; the oracle gives the resulting position,
//! direction, whether it hopped, the hop velocity and the RNG state.
use arty::physics::wa_sheep::SheepBody;

struct Sc { x: i32, y: i32, d: i32, rng: u32, frame: u32, water: i32, rects: Vec<[i32; 4]> }
struct Exp { x: i32, y: i32, d: i32, hopped: bool, vx: i32, vy: i32, rng: u32 }

fn load() -> Vec<(Sc, Exp)> {
    let text = include_str!("data/wa_sheep_oracle.txt");
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(s) = lines.next() {
        let r = lines.next().expect("R line");
        let v: Vec<i64> = s.split_whitespace().skip(1).map(|t| t.parse().unwrap()).collect();
        let n = v[7] as usize;
        let rects = (0..n).map(|i| [v[8 + 4 * i] as i32, v[9 + 4 * i] as i32, v[10 + 4 * i] as i32, v[11 + 4 * i] as i32]).collect();
        let sc = Sc { x: v[0] as i32, y: v[1] as i32, d: v[2] as i32, rng: v[3] as u32, frame: v[4] as u32, water: v[6] as i32, rects };
        let e: Vec<i64> = r.split_whitespace().skip(1).map(|t| t.parse().unwrap()).collect();
        out.push((sc, Exp { x: e[0] as i32, y: e[1] as i32, d: e[2] as i32, hopped: e[3] != 0, vx: e[4] as i32, vy: e[5] as i32, rng: e[6] as u32 }));
    }
    out
}

#[test]
fn sheep_walker_matches_wa() {
    let cases = load();
    let mut bad = Vec::new();
    for (i, (sc, e)) in cases.iter().enumerate() {
        let rects = sc.rects.clone();
        let solid = move |x: i32, y: i32| rects.iter().any(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]);
        let mut s = SheepBody::new(sc.x, sc.y, sc.d, sc.rng);
        s.frame = sc.frame.wrapping_sub(1); // `frame()` increments before walking; WA's counter is already `frame` when the walker runs
        s.frame(&solid, sc.water);
        let got = (s.x, s.y, s.dir, s.hopped, if s.hopped { s.vx } else { 0 }, if s.hopped { s.vy } else { 0 }, if s.hopped { s.rng } else { sc.rng });
        let want = (e.x, e.y, e.d, e.hopped, e.vx, e.vy, if e.hopped { e.rng } else { sc.rng });
        if got != want { bad.push(format!("#{i}: got {got:?} want {want:?}  (start x={} dir={})", sc.x, sc.d)); }
    }
    assert!(bad.is_empty(), "{} of {} scenarios differ from WA; first: {:?}", bad.len(), cases.len(), &bad[..bad.len().min(6)]);
}
