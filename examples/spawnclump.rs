//! Print soldier spawn positions + closest pairs for a seed (hex): cargo run --release --example spawnclump -- 18EADEFDADB48624 [teams]
fn main() {
    let seed = u64::from_str_radix(&std::env::args().nth(1).unwrap_or("1".into()), 16).unwrap_or(1);
    let teams: usize = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(2);
    let ids: Vec<u8> = (0..teams as u8).collect();
    let g = arty::replay::new_game(seed, &ids);
    let mut g = g;
    let (raw, _) = g.terrain.find_spawn_pool(8, 0);
    println!("raw WA pool (before nudge/settle):");
    for r in &raw { println!("  ({:.0},{:.0})", r.x, r.y); }
    for y in (370..=420).step_by(5) {
        let row: String = (150..=190).step_by(2).map(|x| if g.terrain.is_solid(x, y) { '#' } else { '.' }).collect();
        println!("y{y} {row}");
    }
    for x in [160, 168, 172, 176, 180, 186, 190, 200] { println!("levels x={x}: {:?}", g.terrain.standable_foot_levels(x)); }
    let mut pts = vec![];
    for (ti, t) in g.teams.iter().enumerate() { for (si, s) in t.soldiers.iter().enumerate() { pts.push((ti, si, s.pos.x, s.pos.y)); } }
    pts.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap());
    for p in &pts { println!("team {} soldier {} at ({:.0},{:.0})", p.0, p.1, p.2, p.3); }
    let mut close = 0;
    for i in 0..pts.len() { for j in i + 1..pts.len() { if ((pts[i].2 - pts[j].2).powi(2) + (pts[i].3 - pts[j].3).powi(2)).sqrt() < 40.0 { close += 1; } } }
    println!("{} soldiers, {} pairs closer than 40 px", pts.len(), close);
}
