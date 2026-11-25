//! Render the full-screen submenu layout (hud::draw_side_menu) with sample data:
//! `menulook <out_dir>` writes mp.png, tat.png, teams.png.
use arty::renderer::{buffer::WorldBuffer, hud::{self, SideItem, SideCard, SIDE_GREEN, SIDE_WHITE, SIDE_SCRAP}};
use arty::world::{SCREEN_H, SCREEN_W, WORLD_W};

fn save(buf: &WorldBuffer, path: &str) {
    // Overlap check of everything drawn since the previous save, then start recording the next screen.
    for l in arty::renderer::font::layout_overlaps(&arty::renderer::font::layout_record_take()) { println!("OVERLAP {path}: {l}"); }
    arty::renderer::font::layout_record_start();
    let (sw, sh) = (SCREEN_W as usize, SCREEN_H as usize);
    let mut img = vec![0u8; sw * sh * 3];
    let d = buf.raw_data();
    for y in 0..sh { for x in 0..sw { let o = (y * WORLD_W as usize + x) * 4; let p = (y * sw + x) * 3; img[p] = d[o + 2]; img[p + 1] = d[o + 1]; img[p + 2] = d[o]; } }
    let f = std::fs::File::create(path).unwrap();
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), sw as u32, sh as u32);
    e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight);
    e.write_header().unwrap().write_image_data(&img).unwrap();
}
fn it(label: &str) -> SideItem<'_> { SideItem::new(label) }
fn st(k: &str, v: &str, c: arty::renderer::Bgra) -> (String, String, arty::renderer::Bgra) { (k.into(), v.into(), c) }

fn main() {
    let dir = std::env::args().nth(1).unwrap_or("/tmp".into());
    arty::renderer::font::layout_record_start();
    let ab = [("A", "SELECT"), ("B", "BACK")];
    let mut buf = WorldBuffer::new();
    hud::draw_side_menu(&mut buf, "MULTIPLAYER", "DUSTY",
        &[it("LIVE MATCH"), it("TAKE A TURN"), it("MISSIONS"), it("LEADERBOARDS")], 1,
        &SideCard { title: "TAKE A TURN", body: "Async games: play your turn whenever you like. Up to 15 at once, 14 days per turn.", stats: &[] }, &ab);
    save(&buf, &format!("{dir}/mp.png"));
    hud::draw_side_menu(&mut buf, "TAKE A TURN", "DUSTY  ELO 1042",
        &[SideItem { badge: "2", ..SideItem::new("MY MATCHES") }, it("MATCHMAKE"), it("RANKED MATCH"), it("GET CODE"), it("JOIN MATCH")], 0,
        &SideCard { title: "MY MATCHES", body: "Games in progress. R = ranked. A on one where it is your turn to play it.",
            stats: &[st("VS ROBOTNIK", "GO 3D", SIDE_GREEN), st("VS KEV  R", "GO 9D", SIDE_GREEN), st("VS MOOSE", "WAIT 12D", SIDE_WHITE)] }, &ab);
    save(&buf, &format!("{dir}/tat.png"));
    hud::draw_side_menu(&mut buf, "MY TEAMS", "DUSTY",
        &[it("ROSTERS"), it("STORE"), it("EQUIP"), it("PROFILE"), SideItem { danger: true, ..SideItem::new("LOG OUT") }], 1,
        &SideCard { title: "STORE", body: "Spend scrap on hats, uniforms, boots and guns.", stats: &[st("SCRAP", "1240", SIDE_SCRAP)] }, &ab);
    save(&buf, &format!("{dir}/teams.png"));

    // Restyled roster / equip / TAT screens, drawn by the real code.
    use arty::game::account::{Roster, RosterPicker, RosterEditor, CosmeticsScreen};
    let input = arty::input::InputState::new();
    let mut fr = Roster::default_named(0);
    fr.name = "FRAGS".into();
    fr.worm_names = ["GREB".into(), "THLONK".into(), "GIBS".into(), "SMEG".into()];
    fr.hat_ids = [36, 22, 34, 33]; fr.uniform_color_ids = [4, 2, 1, 3]; fr.boot_color_ids = [3, 4, 2, 3]; fr.gun_style_ids = [17, 18, 21, 14];
    let mut bb = Roster::default_named(1);
    bb.name = "BUNGBOIS".into();
    bb.worm_names = ["BEN".into(), "GUY".into(), "MIKEY".into(), "RIAN".into()];
    let mut picker = RosterPicker::new_list_only(String::new(), vec![fr.clone(), bb]);
    picker.update(&input, &mut buf, 0);
    save(&buf, &format!("{dir}/teams_pick.png"));
    let mut ed = RosterEditor::new(fr.clone());
    ed.update(&input, &mut buf, 0);
    save(&buf, &format!("{dir}/team_edit.png"));
    let all: Vec<u8> = (1..=43).collect();
    let mut eq = CosmeticsScreen::new(fr.clone(), all.clone(), all.clone(), all.clone(), all.clone(), String::new(), 1015);
    eq.update(&input, &mut buf);
    save(&buf, &format!("{dir}/equip.png"));
    eq.set_view(2, 3, 15);
    eq.draw(&mut buf);
    save(&buf, &format!("{dir}/equip_gun.png"));
    let lob = arty::game::lobby::LobbyScreen::preview_matches("DUSTY",
        &[("ROBOTNIK", false, true, 3), ("KEV", true, true, 9), ("MOOSE", false, false, 12), ("PIP", true, false, 6), ("GRUNKUS", false, false, 1)], false);
    lob.preview_draw(&mut buf);
    save(&buf, &format!("{dir}/tat_matches.png"));

    // Everything else that was restyled.
    let mut set = arty::game::settings::SettingsScreen::new();
    set.update(&input, &mut buf);
    save(&buf, &format!("{dir}/settings.png"));
    arty::game::title::draw_help_page(&mut buf, arty::game::title::CONTROLS_PAGES, 1, "CONTROLS");
    save(&buf, &format!("{dir}/controls.png"));
    let mut acct = arty::game::account::AccountScreen::new();
    acct.update(&input, &mut buf, 0);
    save(&buf, &format!("{dir}/account.png"));
    let mut kb = arty::renderer::keyboard::Keyboard::new(6);
    kb.text = "AB12".into();
    kb.draw_screen(&mut buf, "JOIN MATCH", "TYPE YOUR FRIEND'S 6-LETTER CODE");
    save(&buf, &format!("{dir}/keyboard.png"));
    let mut store = arty::game::store::StoreScreen::new(String::new(), 1015, &[1, 2, 3], &[1], &[], &[], 40, &fr);
    store.set_view(1, 0, 3);
    store.draw(&mut buf);
    save(&buf, &format!("{dir}/store.png"));
    store.set_view(3, 4, 0);
    store.draw(&mut buf);
    save(&buf, &format!("{dir}/store_wb.png"));
    let mut ms = arty::game::missions::MissionsScreen::preview(&[
        ("Win 2 matches", 2, 2, 60, false, false), ("Kill 5 soldiers", 3, 5, 40, false, false), ("Use the bazooka 10 times", 10, 10, 25, false, true),
        ("Win 10 matches", 4, 10, 250, true, false), ("Drown 6 soldiers", 1, 6, 180, true, false)], 1015);
    ms.update(&input, &mut buf);
    save(&buf, &format!("{dir}/missions.png"));
    arty::game::lobby::LobbyScreen::preview_found(true, false).preview_draw(&mut buf);
    save(&buf, &format!("{dir}/found_ranked.png"));
    arty::game::lobby::LobbyScreen::preview_found(false, true).preview_draw(&mut buf);
    save(&buf, &format!("{dir}/found_casual.png"));
    hud::side_message(&mut buf, "", "LOGGED OUT", hud::SIDE_GOLD, &[]);
    save(&buf, &format!("{dir}/message.png"));
}
