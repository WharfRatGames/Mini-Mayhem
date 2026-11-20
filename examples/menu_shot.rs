//! Render the TEST-mode weapon menu (all weapons) at a given cursor index: `menu_shot [cursor] [out.png]`.
use arty::game::loop_runner::draw_weapon_menu;
use arty::physics::projectile::WeaponKind as W;
use arty::renderer::buffer::WorldBuffer;
use arty::world::{SCREEN_H, SCREEN_W, WORLD_W};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let cursor: usize = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let out = a.get(2).cloned().unwrap_or("/tmp/menu.png".into());
    let mut w: Vec<(W, Option<u32>)> = [W::Bazooka, W::Grenade, W::Shotgun, W::Pistol, W::NinjaRope, W::Tnt, W::Landmine, W::BaseballBat, W::BananaBomb,
        W::ClusterBomb, W::MolotovCocktail, W::Revolver, W::Blasthive, W::BlackHoleBomb, W::PlasmaTorch, W::Jackhammer, W::Garcia, W::Jumpbot, W::SuperSheep,
        W::AirStrike, W::HolyHandGrenade, W::Minigun, W::Uzi, W::HomingMissile].iter().map(|k| (*k, None)).collect();
    w.sort_by_key(|(k, _)| k.menu_sort_key());
    for (i, (k, _)) in w.iter().enumerate() { println!("{i:2} {:?} key {}", k, k.menu_sort_key()); }
    let mut buf = WorldBuffer::new();
    draw_weapon_menu(&mut buf, &w, cursor, 0, 0, 0, 100, 2);
    let (sw, sh) = (SCREEN_W as usize, SCREEN_H as usize);
    let mut img = vec![0u8; sw * sh * 3];
    let d = buf.raw_data();
    for y in 0..sh { for x in 0..sw { let o = (y * WORLD_W as usize + x) * 4; let p = (y * sw + x) * 3; img[p] = d[o + 2]; img[p + 1] = d[o + 1]; img[p + 2] = d[o]; } }
    let f = std::fs::File::create(&out).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), sw as u32, sh as u32);
    e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img).unwrap();
}
