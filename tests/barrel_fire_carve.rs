//! Measures how much terrain a barrel's burning removes AFTER its own blast crater (WA: ~1140 px per drum over ~6 s, wa_rig drum_land).
use arty::world::WorldPos;
fn solid(g: &arty::game::state::GameState) -> usize {
    let mut n = 0; for y in 0..960 { for x in 0..1920 { if g.terrain.is_solid(x, y) { n += 1; } } } n
}
#[test]
fn barrel_fire_carve_amount() {
    let mut tot = Vec::new();
    for seed in [5u64, 9, 21, 33] {
        let mut g = arty::replay::new_game(seed, &[0, 1]);
        g.barrels.clear();
        let p = g.teams[1].soldiers[0].pos;
        g.explode_barrel_pub(WorldPos::new(p.x + 200.0, p.y));
        let after_blast = solid(&g);
        let mut marks = Vec::new();
        for t in 0..600 { g.tick += 1; g.step_fire_patches(); if t % 60 == 59 { marks.push(after_blast - solid(&g)); } }
        println!("seed {seed}: fire-carved px every 2s (30Hz ticks): {:?}", marks);
        tot.push(*marks.last().unwrap());
    }
    println!("TOTALS {:?}", tot);
    let avg = tot.iter().sum::<usize>() / tot.len();
    assert!((600..1800).contains(&avg), "barrel fire should carve ~1100 px like WA, got avg {avg}");
}
