use arty::renderer::{buffer::WorldBuffer, fb::Bgra, skeleton::{draw_soldier_skeletal, SoldierAnim}};
use arty::world::WorldPos;

fn extent(scale: Option<f32>) -> (i32, i32) {
    let mut b = WorldBuffer::new();
    b.clear(Bgra::new(0, 0, 0));
    let pos = WorldPos { x: 300.0, y: 400.0 };
    if let Some(k) = scale { b.begin_scale(pos.x, pos.y, k); }
    draw_soldier_skeletal(&mut b, pos, 0, 1, 100, &SoldierAnim::Idle, None, false, 0, 0, 0, 0, None, 0.0, 0, 0);
    b.end_scale();
    let (mut y0, mut y1, mut x0, mut x1) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for y in 300..420 { for x in 270..330 {
        if b.get_pixel(x, y) != Bgra::new(0, 0, 0) { y0 = y0.min(y); y1 = y1.max(y); x0 = x0.min(x); x1 = x1.max(x); }
    } }
    (y1 - y0 + 1, x1 - x0 + 1)
}

#[test]
fn pixel_art_soldier_ignores_the_caller_scale_and_keeps_its_feet() {
    // The pixel-art looks are drawn 1:1 (at LOOK_SCALE) even when the caller has a downscale active, which would wreck the art.
    let (h1, w1) = extent(None);
    let (h2, w2) = extent(Some(0.6));
    println!("unscaled {w1}x{h1}, with 0.6 caller scale {w2}x{h2}");
    assert_eq!((h1, w1), (h2, w2));
    // ~31 sprite rows at LOOK_SCALE (plus outline rounding)
    let expect = (31.0 * arty::renderer::draw_sprites::LOOK_SCALE) as i32;
    assert!((h1 - expect).abs() <= 6, "height {h1} vs expected ~{expect}");
}

#[test]
#[ignore]
fn bench_scaled_vs_unscaled() {
    use std::time::Instant;
    let pos = WorldPos { x: 300.0, y: 400.0 };
    let mut b = WorldBuffer::new();
    for (name, k) in [("unscaled", None), ("scaled 0.6", Some(0.6f32))] {
        let t = Instant::now();
        for _ in 0..5000 {
            if let Some(k) = k { b.begin_scale(pos.x, pos.y, k); }
            draw_soldier_skeletal(&mut b, pos, 0, 1, 100, &SoldierAnim::Idle, Some(0.4), true, 3, 2, 1, 1, None, 0.0, 0, 0);
            b.end_scale();
        }
        println!("{name}: {:.1} us/soldier", t.elapsed().as_secs_f64() * 1e6 / 5000.0);
    }
}

fn rows(anim: &SoldierAnim) -> (i32, i32) {
    let mut b = WorldBuffer::new();
    b.clear(Bgra::new(0, 0, 0));
    let pos = WorldPos { x: 300.0, y: 400.0 };
    draw_soldier_skeletal(&mut b, pos, 0, 1, 100, anim, None, false, 0, 0, 0, 0, None, 0.0, 0, 0);
    let (mut y0, mut y1) = (i32::MAX, i32::MIN);
    for y in 300..420 { for x in 270..330 { if b.get_pixel(x, y) != Bgra::new(0, 0, 0) { y0 = y0.min(y); y1 = y1.max(y); } } }
    (y0, y1)
}

#[test]
fn crouch_squashes_the_soldier_toward_his_feet() {
    let (top_idle, feet_idle) = rows(&SoldierAnim::Idle);
    let (top_mid, _) = rows(&SoldierAnim::Crouch { depth: 0.5 });
    let (top_full, feet_full) = rows(&SoldierAnim::Crouch { depth: 1.0 });
    let h = feet_idle - top_idle + 1;
    // Standing height is the whole sprite (the draw window used to clip the helmet top: 40 px instead of ~46).
    assert!(h >= 44, "soldier drawn only {h} px tall (helmet clipped?)");
    // Feet stay on the ground; the head comes down by about (1 - CROUCH_SQUASH) of his height, progressively.
    assert!((feet_full - feet_idle).abs() <= 1, "feet moved: {feet_idle} -> {feet_full}");
    let drop = top_full - top_idle;
    let want = ((1.0 - arty::renderer::skeleton::CROUCH_SQUASH) * h as f32) as i32;
    assert!((drop - want).abs() <= 3, "head dropped {drop}px, expected ~{want} (height {h})");
    assert!(top_idle < top_mid && top_mid < top_full, "crouch should be progressive: {top_idle} {top_mid} {top_full}");
    // Depth ramps in over three ticks and holds.
    use arty::renderer::skeleton::crouch_depth;
    assert!(crouch_depth(0) < crouch_depth(1) && crouch_depth(2) == 1.0 && crouch_depth(10) == 1.0);
}
