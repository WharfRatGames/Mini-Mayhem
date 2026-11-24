//! Regression: seed 18EADEFDADB48624 (a cavern map) used to discard every WA spawn because one candidate (168,395) sat on a slope with
//! no standing spot within 40 px, then fell back to Arty's spawner and put 7 of 8 soldiers on one bridge (19-38 px apart).
#[test]
fn cavern_seed_spawns_are_spread() {
    let g = arty::replay::new_game(0x18EADEFDADB48624, &[0, 1]);
    let pts: Vec<(f32, f32)> = g.teams.iter().flat_map(|t| t.soldiers.iter().map(|s| (s.pos.x, s.pos.y))).collect();
    assert_eq!(pts.len(), 8);
    for i in 0..pts.len() { for j in i + 1..pts.len() {
        let d = ((pts[i].0 - pts[j].0).powi(2) + (pts[i].1 - pts[j].1).powi(2)).sqrt();
        assert!(d >= 40.0, "soldiers {i} and {j} only {d:.0}px apart: {:?} {:?}", pts[i], pts[j]);
    } }
}
