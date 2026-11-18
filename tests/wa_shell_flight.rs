//! One real WA bazooka flight captured on the Wine rig (per-frame entry state of the shell + the land bitmap at fire time),
//! replayed through the Rust port: v += (wind, gravity); sweep(shape 2 box vs land). Checks every frame and the hit point.
use arty::physics::wa_body::{sweep, Body};
use arty::physics::wa_collide::overlaps_land;

#[test]
fn replays_wa_shell_flight() {
    let land = std::fs::read("tests/data/wa_shell_land.bin").unwrap();
    let solid = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= 1920 || y >= 696 { return false; }
        (land[240 * y as usize + (x as usize >> 3)] >> (x & 7)) & 1 == 1
    };
    let blocked = |x16: i32, y16: i32| overlaps_land(&solid, 2, x16 >> 16, y16 >> 16);
    let txt = std::fs::read_to_string("tests/data/wa_shell_flight.txt").unwrap();
    let mut frames: Vec<[i32; 6]> = vec![];
    let mut hits: Vec<[i32; 4]> = vec![];
    for l in txt.lines() {
        let v: Vec<i32> = l.split_whitespace().skip(1).map(|t| t.parse().unwrap()).collect();
        if l.starts_with('F') { frames.push([v[0], v[1], v[2], v[3], v[4], v[5]]); } else { hits.push([v[0], v[1], v[2], v[3]]); }
    }
    // F fields: x y vx vy wind gravity
    let (wind, grav) = (11234, 15728);
    let mut b = Body { x: frames[0][0], y: frames[0][1], vx: frames[0][2], vy: frames[0][3], ..Default::default() };
    for i in 1..frames.len() {
        b.vx += wind; b.vy += grav;
        let mut frac = 0x10000;
        let s = sweep(&mut b, &mut frac, 0x200, &blocked);
        assert!(!s.hit, "unexpected hit at frame {i}");
        assert_eq!((b.x, b.y, b.vx, b.vy), (frames[i][0], frames[i][1], frames[i][2], frames[i][3]), "frame {i}");
    }
    // final frame: WA's mover hit
    b.vx += wind; b.vy += grav;
    let mut frac = 0x10000;
    let s = sweep(&mut b, &mut frac, 0x200, &blocked);
    assert!(s.hit, "final frame must hit");
    assert_eq!((b.x, b.y), (hits[0][0], hits[0][1]), "hit position (WA: {:?})", hits[0]);
}

/// Same flight through the game's own `step_projectile` (30 Hz ticks, float state shadowed by the WA fixed-point
/// engine): the shell must explode where WA's did (float init + 5/3 frame phase allow a pixel or two).
#[test]
fn game_step_projectile_matches_wa_hit() {
    use arty::physics::{step_projectile, Projectile, StepResult, WeaponKind};
    use arty::world::{Terrain, Vec2, WorldPos};
    let land = std::fs::read("tests/data/wa_shell_land.bin").unwrap();
    let mut t = Terrain::empty();
    for y in 0..696usize { for x in 0..1920usize {
        if (land[240 * y + (x >> 3)] >> (x & 7)) & 1 == 1 { t.set_solid(x as i32, y as i32, true); }
    } }
    let txt = std::fs::read_to_string("tests/data/wa_shell_flight.txt").unwrap();
    let f0: Vec<i32> = txt.lines().next().unwrap().split_whitespace().skip(1).map(|v| v.parse().unwrap()).collect();
    let hit: Vec<i32> = txt.lines().find(|l| l.starts_with('H')).unwrap().split_whitespace().skip(1).map(|v| v.parse().unwrap()).collect();
    let sc = 50.0f32 / 30.0;
    let mut p = Projectile::new(
        WorldPos::new(f0[0] as f32 / 65536.0, f0[1] as f32 / 65536.0),
        Vec2::new(f0[2] as f32 / 65536.0 * sc, f0[3] as f32 / 65536.0 * sc), WeaponKind::Bazooka);
    let wind = 0.6f32; // 6 wind units, as in the capture (11234 per frame)
    let mut got = None;
    for _ in 0..60 {
        if let StepResult::Explode(e) = step_projectile(&mut p, &t, wind) { got = Some(e); break; }
    }
    let e = got.expect("shell must explode on land");
    let (wx, wy) = (hit[0] as f32 / 65536.0, hit[1] as f32 / 65536.0);
    assert!((e.x - wx).abs() < 3.0 && (e.y - wy).abs() < 3.0, "exploded at ({}, {}), WA at ({}, {})", e.x, e.y, wx, wy);
}
