fn main() {
    let mut i = 0u32;
    loop {
        // tile(id) wraps modulo the pool; walk ids until pool wraps (identify by pointer)
        let t = arty::renderer::terrain_textures::tile(i as u8).unwrap();
        if i > 0 && std::ptr::eq(t, arty::renderer::terrain_textures::tile(0).unwrap()) { break; }
        let (w, h) = (t.w, t.h);
        let mut hit = 0usize; let mut n = 0usize;
        for y in 1..h - 1 {
            for x in 0..w - 9 {
                let p = |x: usize, y: usize| { let c = t.px[y * w + x]; c[0] as i32 + c[1] as i32 + c[2] as i32 };
                let hflat = (0..8).all(|k| (p(x + k, y) - p(x + k + 1, y)).abs() <= 6);
                let v = (p(x, y) - p(x, y - 1)).abs().max((p(x, y) - p(x, y + 1)).abs());
                n += 1; if hflat && v >= 30 { hit += 1; }
            }
        }
        println!("tile {:3} {}x{} smear={:.4}", i, w, h, hit as f64 / n as f64);
        i += 1;
        if i > 200 { break; }
    }
}
