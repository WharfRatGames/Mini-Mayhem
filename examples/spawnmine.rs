//! How many soldiers spawn on/next to a mine or barrel (distance in px)?
fn main() {
    let n: u64 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(100);
    let (mut worms, mut near, mut on) = (0, 0, 0);
    for seed in 0..n {
        let g = arty::replay::new_game(seed.wrapping_mul(0x9E3779B97F4A7C15), &[0, 1]);
        let objs: Vec<(&str, f32, f32)> = g.mines.iter().map(|m| ("mine", m.pos.x, m.pos.y)).chain(g.barrels.iter().map(|b| ("barrel", b.pos.x, b.pos.y))).collect();
        for t in &g.teams { for s in &t.soldiers {
            worms += 1;
            let d = objs.iter().map(|(k, x, y)| (((s.pos.x - x).powi(2) + (s.pos.y - y).powi(2)).sqrt(), *k)).fold((1e9f32, ""), |a, b| if b.0 < a.0 { b } else { a });
            if d.0 < 25.0 { near += 1; if d.0 < 10.0 { on += 1; } if near <= 8 { println!("seed {seed}: worm at ({:.0},{:.0}) {:.1}px from a {}", s.pos.x, s.pos.y, d.0, d.1); } }
        } }
    }
    println!("{worms} soldiers: {near} within 25 px of a mine/barrel, {on} within 10 px");
}
