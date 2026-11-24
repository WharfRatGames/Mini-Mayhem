//! Render the full-screen submenu layout (hud::draw_side_menu) with sample data:
//! `menulook <out_dir>` writes mp.png, tat.png, teams.png.
use arty::renderer::{buffer::WorldBuffer, hud::{self, SideItem, SideCard, SIDE_GREEN, SIDE_WHITE, SIDE_SCRAP}};
use arty::world::{SCREEN_H, SCREEN_W, WORLD_W};

fn save(buf: &WorldBuffer, path: &str) {
    let (sw, sh) = (SCREEN_W as usize, SCREEN_H as usize);
    let mut img = vec![0u8; sw * sh * 3];
    let d = buf.raw_data();
    for y in 0..sh { for x in 0..sw { let o = (y * WORLD_W as usize + x) * 4; let p = (y * sw + x) * 3; img[p] = d[o + 2]; img[p + 1] = d[o + 1]; img[p + 2] = d[o]; } }
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), sw as u32, sh as u32);
    e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img).unwrap();
}
fn it(label: &str) -> SideItem<'_> { SideItem { label, badge: "", danger: false } }
fn st(k: &str, v: &str, c: arty::renderer::Bgra) -> (String, String, arty::renderer::Bgra) { (k.into(), v.into(), c) }

fn main() {
    let dir = std::env::args().nth(1).unwrap_or("/tmp".into());
    let ab = [("A", "SELECT"), ("B", "BACK")];
    let mut buf = WorldBuffer::new();
    hud::draw_side_menu(&mut buf, "MULTIPLAYER", "DUSTY",
        &[it("LIVE MATCH"), it("TAKE A TURN"), it("MISSIONS"), it("LEADERBOARDS")], 1,
        &SideCard { title: "TAKE A TURN", body: "Async games: play your turn whenever you like. Up to 15 at once, 14 days per turn.", stats: &[] }, &ab);
    save(&buf, &format!("{dir}/mp.png"));
    hud::draw_side_menu(&mut buf, "TAKE A TURN", "DUSTY  ELO 1042",
        &[SideItem { label: "MY MATCHES", badge: "2", danger: false }, it("MATCHMAKE"), it("RANKED MATCH"), it("GET CODE"), it("JOIN MATCH")], 0,
        &SideCard { title: "MY MATCHES", body: "Games in progress. R = ranked. A on one where it is your turn to play it.",
            stats: &[st("VS ROBOTNIK", "GO 3D", SIDE_GREEN), st("VS KEV  R", "GO 9D", SIDE_GREEN), st("VS MOOSE", "WAIT 12D", SIDE_WHITE)] }, &ab);
    save(&buf, &format!("{dir}/tat.png"));
    hud::draw_side_menu(&mut buf, "MY TEAMS", "DUSTY",
        &[it("ROSTERS"), it("STORE"), it("EQUIP"), it("PROFILE"), SideItem { label: "LOG OUT", badge: "", danger: true }], 1,
        &SideCard { title: "STORE", body: "Spend scrap on hats, uniforms, boots and guns.", stats: &[st("SCRAP", "1240", SIDE_SCRAP)] }, &ab);
    save(&buf, &format!("{dir}/teams.png"));
}
