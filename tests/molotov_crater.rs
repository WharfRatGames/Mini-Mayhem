//! WA (wa_rig molo_land): Molotov impact = one instant radius-18 crater; the burning flames never touch terrain afterwards.
use arty::world::WorldPos;
fn solid_count(g: &arty::game::state::GameState, cx: f32, cy: f32) -> usize {
    let mut n = 0;
    for y in (cy as i32 - 30)..(cy as i32 + 30) { for x in (cx as i32 - 30)..(cx as i32 + 30) { if g.terrain.is_solid(x, y) { n += 1; } } }
    n
}
#[test]
fn molotov_carves_radius_18_once_and_fire_carves_nothing() {
    let mut g = arty::replay::new_game(5, &[0, 1]);
    let p = g.teams[1].soldiers[0].pos;
    let at = WorldPos::new(p.x, p.y + 6.0);
    let before = solid_count(&g, at.x, at.y);
    g.spawn_molotov_fire(at);
    let after = solid_count(&g, at.x, at.y);
    let removed = before - after;
    assert!(removed > 200 && removed < 1100, "impact crater should be ~r18 (<=1018 px), removed {removed}");
    for _ in 0..400 { g.step_fire_patches(); }
    assert_eq!(solid_count(&g, at.x, at.y), after, "burning must not carve terrain");
}
