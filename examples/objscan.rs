//! Mines/barrels placed by WA's algorithm: how many sit inside solid terrain, float in air, or touch a girder/scenery?
fn main() {
    let n: u64 = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(100);
    let (mut tot, mut buried, mut floating, mut deco) = (0, 0, 0, 0);
    for seed in 0..n {
        let g = arty::replay::new_game(seed.wrapping_mul(0x9E3779B97F4A7C15), &[0, 1]);
        let t = &g.terrain;
        let mut pts: Vec<(&str, f32, f32)> = g.mines.iter().map(|m| ("mine", m.pos.x, m.pos.y)).collect();
        pts.extend(g.barrels.iter().map(|b| ("barrel", b.pos.x, b.pos.y)));
        for (k, x, y) in pts {
            tot += 1;
            let (xi, yi) = (x as i32, y as i32);
            let in_solid = t.is_solid(xi, yi) || t.is_solid(xi, yi - 4);
            let ground = (0..=8).any(|d| t.is_solid(xi, yi + d));
            if in_solid { buried += 1; if buried <= 6 { println!("seed {seed} {k} BURIED at ({x:.0},{y:.0})"); } }
            else if !ground { floating += 1; }
            if (-6..=6).any(|d| t.deco[((yi + 2).clamp(0, 959) as usize) * 1920 + (xi + d).clamp(0, 1919) as usize] != 0) { deco += 1; }
        }
    }
    println!("{tot} objects: {buried} buried, {floating} floating (>8px above ground), {deco} resting on/at scenery or girders (legit in WA)");
}
