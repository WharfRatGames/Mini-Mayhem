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
fn scaled_worm_is_smaller_and_keeps_its_feet() {
    let (h1, w1) = extent(None);
    let (h2, w2) = extent(Some(0.7));
    println!("full {w1}x{h1}, scaled {w2}x{h2}");
    assert!(h2 < h1 && w2 <= w1);
    assert!((h2 as f32 / h1 as f32 - 0.7).abs() < 0.12);
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
