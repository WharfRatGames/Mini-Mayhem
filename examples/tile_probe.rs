fn main() {
    let ids: Vec<u8> = std::env::args().skip(1).map(|a| a.parse().unwrap()).collect();
    let mut tiles = vec![];
    for id in &ids { tiles.push(arty::renderer::terrain_textures::tile(*id).unwrap()); }
    let wtot: usize = tiles.iter().map(|t| t.w).sum::<usize>() + 4 * tiles.len();
    let h = tiles.iter().map(|t| t.h).max().unwrap();
    let mut img = vec![0u8; wtot * h * 3];
    let mut x0 = 0;
    for t in &tiles {
        for y in 0..t.h { for x in 0..t.w { let p = t.px[y * t.w + x]; let o = (y * wtot + x0 + x) * 3; img[o..o + 3].copy_from_slice(&p[..3]); } }
        x0 += t.w + 4;
    }
    let mut ppm = format!("P6\n{} {}\n255\n", wtot, h).into_bytes(); ppm.extend(img);
    std::fs::write("/tmp/tile_probe.ppm", ppm).unwrap();
}
