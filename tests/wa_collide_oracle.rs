//! WA body-vs-land overlap, checked against 312 live tests captured from WA.exe (Wine rig) on a real map.
use arty::physics::wa_collide::overlaps_land;

fn shape_id(addr: &str) -> u8 {
    match addr { "21d8188" => 9, "21d7d78" => 2, "21d7e48" => 3, "21d7ca8" => 4, "21d7f18" => 5, a => panic!("{a}") }
}

#[test]
fn matches_wa_land_tests() {
    let land = std::fs::read("tests/data/wa_land_1920x696.bin").unwrap();
    let solid = |x: i32, y: i32| -> bool {
        if x < 0 || y < 0 || x >= 1920 || y >= 696 { return false; }
        (land[240 * y as usize + (x as usize >> 3)] >> (x & 7)) & 1 == 1
    };
    let (mut n, mut pos) = (0, 0);
    for l in std::fs::read_to_string("tests/data/wa_collide_samples.txt").unwrap().lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        let (x, y): (i32, i32) = (f[0].parse().unwrap(), f[1].parse().unwrap());
        let exp = i32::from_str_radix(f[3], 16).unwrap() & 1 == 1;
        assert_eq!(overlaps_land(&solid, shape_id(f[2]), x, y), exp, "x={x} y={y} line={l}");
        n += 1; if exp { pos += 1; }
    }
    assert!(n >= 300 && pos >= 20, "n={n} pos={pos}");
}
