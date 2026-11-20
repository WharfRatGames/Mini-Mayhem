//! A real WA grenade throw (Wine rig capture, tests/data/wa_grenade_flight.txt) replayed through `physics::wa_thrown`.
use arty::physics::wa_thrown::Thrown;

#[test]
fn grenade_matches_wa_frame_by_frame() {
    let rows: Vec<Vec<i64>> = include_str!("data/wa_grenade_flight.txt").lines().filter(|l| !l.starts_with('#'))
        .map(|l| l.split_whitespace().map(|v| v.parse().unwrap()).collect()).collect();
    let solid = |_x: i32, y: i32| y >= 652; // flat floor measured from the capture
    let r0 = &rows[0];
    let mut g = Thrown::grenade(r0[0] as i32, r0[1] as i32, r0[2] as i32, r0[3] as i32, r0[4] as i32, false);
    let mut hits = 0;
    for (i, r) in rows.iter().enumerate().skip(1) {
        let f = g.step(&solid, 0);
        hits += f.hits as u32;
        assert_eq!([g.x as i64, g.y as i64, g.vx as i64, g.vy as i64, g.fuse_ms as i64], [r[0], r[1], r[2], r[3], r[4]], "frame {i}");
    }
    assert!(hits >= 40, "expected the bounce and the rolling contacts, got {hits}");
    let last = g.step(&solid, 0);
    assert!(last.exploded, "fuse ran out on the last captured frame");
}
