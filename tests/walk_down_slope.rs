//! A soldier must be able to walk DOWN a steep slope: the headroom rule once counted the uphill hillside as a zero-height ceiling.
use arty::game::loop_runner::{headroom, walk_headroom_ok, HEAD_CLEAR};
use arty::world::terrain::Terrain;

#[test]
fn steep_downhill_step_is_not_a_low_ceiling() {
    let mut t = Terrain::empty();
    // 45 degree slope falling to the left: ground surface y = 500 + x (x in 0..100), solid below it.
    for x in 0..200 { for y in (500 + x.min(100))..700 { t.set_solid(x, y, true); } }
    let (ox, oy) = (100, 600); // foot on the slope (surface row 600 is solid, foot row 599)
    let (nx, ny) = (99, 599);
    assert_eq!(headroom(&t, ox, oy - 1, HEAD_CLEAR), HEAD_CLEAR);
    assert!(walk_headroom_ok(&t, ox, oy - 1, nx, ny - 1));
}
