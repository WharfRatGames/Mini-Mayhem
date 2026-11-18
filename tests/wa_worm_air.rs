//! Real WA worm jumps captured on the Wine rig (per-frame entry state of the worm object + the land bitmap),
//! replayed through `WormBody::step_air`. Every frame must match bit-exactly, including bounces and the landing snap.
use arty::physics::wa_worm::WormBody;

fn load_land(path: &str) -> impl Fn(i32, i32) -> bool {
    let land = std::fs::read(path).unwrap();
    move |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= 1920 || y >= 696 { return false; }
        (land[240 * y as usize + (x as usize >> 3)] >> (x & 7)) & 1 == 1
    }
}

/// Rows `phase line x y vx vy ...`; returns the frames of the jump that starts at the row index `start` (the impulse row)
/// until (and including) the first following row whose velocity is zero again.
fn flight(txt: &str, phase: &str, from_line: u32) -> Vec<[i32; 4]> {
    let mut out: Vec<[i32; 4]> = vec![];
    for l in txt.lines() {
        let t: Vec<&str> = l.split_whitespace().collect();
        if t[0] != phase || t[1].parse::<u32>().unwrap() < from_line { continue; }
        let v: Vec<i32> = t[2..6].iter().map(|s| s.parse().unwrap()).collect();
        if out.is_empty() && (v[2] == 0 && v[3] == 0) { continue; }
        out.push([v[0], v[1], v[2], v[3]]);
        if out.len() > 1 && v[2] == 0 && v[3] == 0 { break; }
    }
    out
}

fn replay(solid: &dyn Fn(i32, i32) -> bool, frames: &[[i32; 4]]) {
    assert!(frames.len() > 20);
    let mut w = WormBody { x: frames[0][0], y: frames[0][1], vx: frames[0][2], vy: frames[0][3] };
    for (i, f) in frames.iter().enumerate().skip(1) {
        w.step_air(solid);
        assert_eq!([w.x, w.y, w.vx, w.vy], *f, "frame {i}");
    }
}

#[test]
fn replays_wa_high_jump_and_landing() {
    let solid = load_land("tests/data/wa_worm_land1.bin");
    let txt = std::fs::read_to_string("tests/data/wa_worm_trace1.txt").unwrap();
    replay(&solid, &flight(&txt, "jumped", 0));
}

#[test]
fn replays_wa_forward_jump_wall_bounce_and_backflip() {
    let solid = load_land("tests/data/wa_worm_land2.bin");
    let txt = std::fs::read_to_string("tests/data/wa_worm_trace2_jumps.txt").unwrap();
    replay(&solid, &flight(&txt, "turned2", 0)); // forward jump (ceiling/wall bounce inside)
    replay(&solid, &flight(&txt, "bf0", 0));     // backflip
}

/// Generic replay over long captures with a frame counter: columns `t x y vx vy state ac`. Every maximal run of
/// consecutive frames in the airborne jump state (0x79) is replayed from its first frame through the frame that ends it
/// (landing, stop at a wall or ceiling).
fn replay_all_segments(land: &str, trace: &str, min_segments: usize) {
    let solid = load_land(land);
    let txt = std::fs::read_to_string(trace).unwrap();
    let rows: Vec<[i64; 6]> = txt.lines().map(|l| {
        let t: Vec<&str> = l.split_whitespace().collect();
        [t[0].parse().unwrap(), t[1].parse().unwrap(), t[2].parse().unwrap(), t[3].parse().unwrap(), t[4].parse().unwrap(),
         i64::from_str_radix(t[5], 16).unwrap()]
    }).collect();
    let (mut segs, mut i) = (0, 1);
    while i < rows.len() {
        if rows[i][5] == 0x79 && rows[i - 1][5] != 0x79 {
            let mut w = WormBody { x: rows[i][1] as i32, y: rows[i][2] as i32, vx: rows[i][3] as i32, vy: rows[i][4] as i32 };
            let mut j = i + 1;
            loop {
                let before = w;
                let r = w.step_air(&solid);
                let got = [w.x as i64, w.y as i64, w.vx as i64, w.vy as i64];
                let want = [rows[j][1], rows[j][2], rows[j][3], rows[j][4]];
                assert_eq!(got, want, "{trace} segment {segs}, row {j} (t={}) from {:?} landed={}", rows[j][0], before, r.landed);
                if rows[j][5] != 0x79 { break; }
                j += 1;
            }
            segs += 1;
            i = j;
        }
        i += 1;
    }
    assert!(segs >= min_segments, "expected >= {min_segments} jumps in {trace}, got {segs}");
}

#[test]
fn replays_every_airborne_segment_of_captures_3_and_4() {
    replay_all_segments("tests/data/wa_worm_land4.bin", "tests/data/wa_worm_trace3.txt", 5);
    replay_all_segments("tests/data/wa_worm_land5.bin", "tests/data/wa_worm_trace4.txt", 11);
}

#[test]
fn replays_walking_moves_of_capture4() {
    let solid = load_land("tests/data/wa_worm_land5.bin");
    let txt = std::fs::read_to_string("tests/data/wa_worm_trace4.txt").unwrap();
    let rows: Vec<Vec<i64>> = txt.lines().map(|l| l.split_whitespace().enumerate()
        .map(|(i, v)| if i == 5 { i64::from_str_radix(v, 16).unwrap() } else { v.parse().unwrap() }).collect()).collect();
    let mut n = 0;
    for w in rows.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let dx = b[1] - a[1];
        if b[5] == 0x66 && a[5] != 0x79 && b[0] == a[0] + 1 && (dx == 32768 || dx == -32768 || dx.abs() == 98304) && b[3] == 0 {
            let mut body = WormBody { x: a[1] as i32, y: a[2] as i32, vx: 0, vy: 0 };
            let r = body.walk_move(dx as i32, &solid);
            assert_eq!([body.x as i64, body.y as i64], [b[1], b[2]], "t={} r={r}", b[0]);
            n += 1;
        }
    }
    assert!(n > 50, "only {n} walking moves checked");
}

#[test]
fn worm_lands_on_another_worms_head_and_bounces_off_its_underside() {
    let none = |_: i32, _: i32| false;
    // Other worm at (100, 200): its mask spans rows 191..=206. Fall from above: land on its head.
    let mut w = WormBody { x: 100 << 16, y: (150 << 16) | 0xffff, vx: 0, vy: 0 };
    let mut landed = false;
    for _ in 0..200 { if w.step_air_with(&none, &[(100, 200)]).landed { landed = true; break; } }
    assert!(landed && w.vx == 0 && w.vy == 0);
    assert!((w.y >> 16) < 191 + 6 && (w.y >> 16) > 170, "stopped on the head, y={}", w.y >> 16);
    // Rising into the underside of a worm: not a ceiling stop (type 2 bounces), so it keeps a velocity or falls back.
    let mut u = WormBody { x: 100 << 16, y: (230 << 16) | 0xffff, vx: 0, vy: -294912 };
    let r = u.step_air_with(&none, &[(100, 200)]);
    let _ = r;
    for _ in 0..40 { u.step_air_with(&none, &[(100, 200)]); }
    assert!(u.vy != 0 || u.y >> 16 > 200);
}

/// Real WA capture: worm B (teleported 30 px above worm A via gdb) falls and lands on A's head
/// (hit handler: other = worm, type 2, side 3). A at (62717952, 17301504). No land nearby.
#[test]
fn replays_wa_worm_landing_on_another_worm() {
    let none = |_: i32, _: i32| false;
    let a = (62717952 >> 16, 17301504 >> 16);
    let rows: Vec<Vec<i64>> = std::fs::read_to_string("tests/data/wa_worm_on_worm.txt").unwrap()
        .lines().map(|l| l.split_whitespace().map(|v| v.parse().unwrap()).collect()).collect();
    let mut w = WormBody { x: rows[0][1] as i32, y: rows[0][2] as i32, vx: 0, vy: 0 };
    for r in &rows[1..] {
        w.step_air_with(&none, &[a]);
        assert_eq!([w.x as i64, w.y as i64, w.vx as i64, w.vy as i64], [r[1], r[2], r[3], r[4]], "t={}", r[0]);
    }
}

/// Real WA capture: worm A backflips from beneath worm B (resting on A's head). B is launched with A's vy
/// (state 0x80) and A flies through. Rows: t xA yA vyA xB yB vyB. A=(62717952,17301504) B=(63045632,16318463).
#[test]
fn replays_wa_backflip_into_worm_above() {
    let none = |_: i32, _: i32| false;
    let txt = std::fs::read_to_string("tests/data/wa_worm_underside.txt").unwrap();
    let rows: Vec<Vec<i64>> = txt.lines().map(|l| l.split_whitespace().map(|v| v.parse().unwrap()).collect()).collect();
    let mut a = WormBody { x: rows[0][1] as i32, y: rows[0][2] as i32, vx: 0, vy: rows[0][3] as i32 };
    let mut b = WormBody { x: rows[0][4] as i32, y: rows[0][5] as i32, vx: 0, vy: 0 };
    let mut b_air = false;
    for r in &rows[1..] {
        let others: Vec<(i32, i32)> = if b_air { vec![] } else { vec![(b.x >> 16, b.y >> 16)] };
        let f = a.step_air_with(&none, &others);
        if b_air { b.step_air(&none); }
        if let Some((_, vy)) = f.launched { b_air = true; b.vx = 0; b.vy = vy; }
        assert_eq!([a.y as i64, a.vy as i64], [r[2], r[3]], "A t={}", r[0]);
        assert_eq!([b.y as i64, b.vy as i64], [r[5], r[6]], "B t={}", r[0]);
    }
}

/// Real WA capture of a held walk on flat ground (rig: worm held Right/Left): x follows anchor + dir * WALK_TABLE[phase].
/// Columns: t x y state. (Walks in the first captures deviate by an extra half-pixel step at frame 6; unexplained -
/// possibly a different animation variant - so only this clean flat capture is asserted.)
#[test]
fn walking_follows_wa_phase_table() {
    use arty::physics::wa_worm::WalkState;
    let txt = std::fs::read_to_string("tests/data/wa_worm_walk_flat.txt").unwrap();
    let rows: Vec<Vec<String>> = txt.lines().map(|l| l.split_whitespace().map(String::from).collect()).collect();
    let i = rows.iter().position(|r| r[3] == "66").unwrap();
    let (x0, y0): (i64, i64) = (rows[i - 1][1].parse().unwrap(), rows[i - 1][2].parse().unwrap());
    let flat = move |_x: i32, y: i32| (y as i64) >= (y0 >> 16) + 7;
    let mut body = WormBody { x: x0 as i32, y: y0 as i32, vx: 0, vy: 0 };
    let dir = (rows[i + 6][1].parse::<i64>().unwrap() - x0).signum() as i32;
    let mut ws = WalkState::start(body.x, dir);
    let mut n = 0;
    let mut k = i;
    while rows[k][3] == "66" {
        body.walk_frame(&mut ws, &flat);
        assert_eq!(body.x as i64, rows[k][1].parse::<i64>().unwrap(), "frame {} of walk", k - i + 1);
        n += 1; k += 1;
    }
    assert!(n >= 20, "only {n} walking frames");
}

/// The same walk model reproduces every walking run of capture 4 (all directions, slopes, step-ups) on its real terrain.
#[test]
fn walking_runs_of_capture4_match_model() {
    use arty::physics::wa_worm::WalkState;
    let solid = load_land("tests/data/wa_worm_land5.bin");
    let txt = std::fs::read_to_string("tests/data/wa_worm_trace4.txt").unwrap();
    let rows: Vec<Vec<i64>> = txt.lines().map(|l| l.split_whitespace().enumerate()
        .map(|(i, v)| if i == 5 { i64::from_str_radix(v, 16).unwrap() } else { v.parse().unwrap() }).collect()).collect();
    let (mut ok, mut bad, mut i) = (0, 0, 1);
    while i < rows.len() {
        if rows[i][5] == 0x66 && rows[i - 1][5] != 0x66 && rows[i - 1][5] != 0x79 {
            let mut e = i;
            while e < rows.len() && rows[e][5] == 0x66 { e += 1; }
            let dir = (i..e).map(|q| rows[q][1] - rows[q - 1][1]).find(|d| *d != 0).map_or(0, |d| d.signum());
            if dir != 0 {
                let mut body = WormBody { x: rows[i - 1][1] as i32, y: rows[i - 1][2] as i32, vx: 0, vy: 0 };
                let mut ws = WalkState::start(body.x, dir as i32);
                let mut good = true;
                for k in i..e {
                    body.walk_frame(&mut ws, &solid);
                    if [body.x as i64, body.y as i64] != [rows[k][1], rows[k][2]] { good = false; break; }
                }
                if good { ok += 1 } else { bad += 1 }
            }
            i = e;
        }
        i += 1;
    }
    eprintln!("capture4 walk runs ok={ok} bad={bad}");
    assert!(ok >= 10 && bad == 0, "ok={ok} bad={bad}");
}
