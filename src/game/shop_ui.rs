//! Shared STORE / EQUIP layout: a strip of the team's 4 soldiers on top (L1/R1), category tabs
//! (UP/DOWN), then one big row of 3 tiles (LEFT/RIGHT) showing each item worn by the chosen soldier
//! in their full outfit. Pictures are fitted to their box (hats never cut off) and every text sits
//! in its own space (nothing drawn behind a soldier); `tests` sweeps both screens through the
//! layout overlap checker.
use crate::renderer::{WorldBuffer, Bgra, hud};
use crate::renderer::font::{draw_str, draw_str_shadow_scaled, str_width, str_width_scaled};
use crate::world::SCREEN_W;
use super::account::Roster;

/// One soldier's outfit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit { pub look: usize, pub team: Bgra, pub uni: u8, pub hat: u8, pub boot: u8, pub gun: u8 }

impl Fit {
    pub fn of(r: &Roster, si: usize) -> Fit {
        let u = r.uniform_color_ids[si];
        Fit {
            look: ((u >> 5) as usize) % crate::renderer::soldier_looks::N_LOOKS,
            team: crate::renderer::draw_sprites::TEAM_COLOURS[si.min(3)],
            uni: u & 0x1f, hat: r.hat_ids[si], boot: r.boot_color_ids[si], gun: r.gun_style_ids[si],
        }
    }
    /// The same soldier with item `id` of category `cat` (hat / uniform / boots / gun_style / look) on.
    pub fn with(mut self, cat: &str, id: u8) -> Fit {
        match cat {
            "hat" => self.hat = id, "uniform" => self.uni = id, "boots" => self.boot = id, "gun_style" => self.gun = id,
            "look" => self.look = id as usize % crate::renderer::soldier_looks::N_LOOKS, _ => {}
        }
        self
    }
    fn body(&self) -> Bgra { if self.uni == 0 { self.team } else { crate::renderer::skeleton::uniform_color(self.uni) } }
    /// Whole soldier inside the box, as big as fits up to `z`.
    pub fn draw_in(&self, buf: &mut WorldBuffer, x: i32, y: i32, w: i32, h: i32, z: i32) {
        super::account::draw_worn_icon_fit(buf, self.look, self.body(), self.hat, self.boot, self.gun, x, y, w, h, z);
    }
}

/// One item tile: what it is, its name, and the tag under the name (price / OWNED / WEARING).
pub struct Tile { pub cat: &'static str, pub id: u8, pub name: String, pub tag: String, pub tag_col: Bgra }

/// Everything one STORE or EQUIP frame shows.
pub struct ShopView<'a> {
    pub title: &'a str,
    pub info: &'a str,
    pub hints: &'a [(&'a str, &'a str)],
    pub soldiers: &'a [(String, Fit); 4],
    pub cur: usize,
    pub tabs: &'a [&'a str],
    pub tab: usize,
    pub tiles: &'a [Tile],
    pub first: usize,
    /// Highlighted tile (index into `tiles`).
    pub sel: Option<usize>,
    /// "OF 36 FOR SALE" / "OF 23 OWNED" suffix.
    pub what: &'a str,
    /// Feedback line (left of the position text), e.g. PURCHASED!.
    pub status: Option<(&'a str, Bgra)>,
}

pub const VISIBLE: usize = 3;
pub const GREEN: Bgra = Bgra::new(90, 230, 120);
pub const RED: Bgra = Bgra::new(220, 80, 70);

/// First visible tile so that `sel` is on screen, moving the window as little as possible from `first`.
pub fn scroll_to(first: usize, sel: usize, n: usize) -> usize {
    let mut f = first;
    if sel < f { f = sel; }
    if sel >= f + VISIBLE { f = sel + 1 - VISIBLE; }
    f.min(n.saturating_sub(VISIBLE))
}

fn tile(buf: &mut WorldBuffer, x: i32, y: i32, w: i32, h: i32, fit: Fit, t: &Tile, sel: bool) {
    buf.fill_rect(x, y, w as u32, h as u32, if sel { Bgra::new(30, 42, 100) } else { Bgra::new(20, 26, 60) });
    if sel {
        for (xx, yy, ww, hh) in [(x, y, w, 3), (x, y + h - 3, w, 3), (x, y, 3, h), (x + w - 3, y, 3, h)] {
            buf.fill_rect(xx, yy, ww as u32, hh as u32, hud::SIDE_ACCENT);
        }
    }
    let text_top = y + h - 38;
    buf.fill_rect(x + 14, text_top - 6, (w - 28) as u32, 2, Bgra::new(50, 58, 110));
    fit.with(t.cat, t.id).draw_in(buf, x + 8, y + 8, w - 16, text_top - 16 - y, 5);
    let mut n = t.name.clone();
    while str_width(&n) > w - 10 && n.len() > 1 { n.pop(); }
    draw_str(buf, &n, x + w / 2 - str_width(&n) / 2, text_top + 4, if sel { hud::SIDE_GOLD } else { Bgra::new(215, 220, 240) });
    if !t.tag.is_empty() {
        draw_str(buf, &t.tag, x + w / 2 - str_width(&t.tag) / 2, text_top + 18, t.tag_col);
    }
}

pub fn draw(buf: &mut WorldBuffer, v: &ShopView) {
    let (sw, top, bot) = (SCREEN_W as i32, hud::SIDE_TOP, hud::SIDE_BOTTOM);
    hud::side_frame_plain(buf, v.title, v.info, v.hints);

    // Soldier strip: picture on the left of each tab, name on the right (own boxes, never on top of each other).
    let tab_w = (sw - 28 - 3 * 8) / 4;
    let tab_h = 70;
    for (i, (n, f)) in v.soldiers.iter().enumerate() {
        let x = 14 + i as i32 * (tab_w + 8);
        let on = i == v.cur;
        buf.fill_rect(x, top, tab_w as u32, tab_h as u32, if on { Bgra::new(30, 42, 100) } else { Bgra::new(20, 26, 60) });
        if on { buf.fill_rect(x, top + tab_h - 3, tab_w as u32, 3, hud::SIDE_ACCENT); }
        f.draw_in(buf, x + 4, top + 6, 54, tab_h - 12, 3);
        let mut nm = n.to_uppercase();
        while str_width(&nm) > tab_w - 66 && nm.len() > 1 { nm.pop(); }
        draw_str(buf, &nm, x + 62, top + tab_h / 2 - 4, if on { hud::SIDE_GOLD } else { Bgra::new(215, 220, 240) });
    }

    // Category tabs, equal width, all labels at one scale.
    let tabs_y = top + tab_h + 8;
    let nt = v.tabs.len().max(1) as i32;
    let tabw = (sw - 28 - (nt - 1) * 3) / nt;
    let ls = if v.tabs.iter().all(|t| str_width_scaled(t, 2) <= tabw - 2) { 2 } else { 1 };
    for (i, label) in v.tabs.iter().enumerate() {
        let on = i == v.tab;
        let tx = 14 + i as i32 * (tabw + 3);
        buf.fill_rect(tx, tabs_y, tabw as u32, 28, if on { Bgra::new(30, 42, 100) } else { Bgra::new(20, 26, 60) });
        if on { buf.fill_rect(tx, tabs_y + 25, tabw as u32, 3, hud::SIDE_ACCENT); }
        draw_str_shadow_scaled(buf, label, tx + tabw / 2 - str_width_scaled(label, ls) / 2, tabs_y + 14 - 4 * ls,
                               if on { hud::SIDE_GOLD } else { hud::SIDE_DIM }, ls);
    }

    // Position on the right, feedback on the left of the same line.
    let n = v.tiles.len();
    let first = v.first.min(n.saturating_sub(VISIBLE));
    let pos = if n == 0 { format!("0 {}", v.what) } else { format!("{}-{} OF {} {}", first + 1, (first + VISIBLE).min(n), n, v.what) };
    let pos_x = sw - 30 - str_width(&pos);
    draw_str(buf, &pos, pos_x, tabs_y + 34, hud::SIDE_GOLD);
    if let Some((msg, col)) = v.status {
        let mut m = msg.to_string();
        while str_width(&m) > pos_x - 30 - 12 && m.len() > 1 { m.pop(); }
        draw_str(buf, &m, 30, tabs_y + 34, col);
    }

    let row_y = tabs_y + 48;
    let gap = 10;
    let tw = (sw - 60 - 2 * gap) / VISIBLE as i32;
    let th = bot - row_y;
    let me = v.soldiers[v.cur.min(3)].1;
    for k in 0..VISIBLE {
        let Some(t) = v.tiles.get(first + k) else { break };
        tile(buf, 30 + k as i32 * (tw + gap), row_y, tw, th, me, t, v.sel == Some(first + k));
    }
    if first > 0 { draw_str_shadow_scaled(buf, "<", 10, row_y + th / 2 - 8, hud::SIDE_GOLD, 2); }
    if first + VISIBLE < n { draw_str_shadow_scaled(buf, ">", sw - 24, row_y + th / 2 - 8, hud::SIDE_GOLD, 2); }
}

/// The 4 soldiers of a roster as (name, outfit).
pub fn soldiers_of(r: &Roster) -> [(String, Fit); 4] {
    std::array::from_fn(|si| (r.worm_names[si].clone(), Fit::of(r, si)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::account::{CosmeticsScreen, Roster};
    use crate::game::store::StoreScreen;
    use crate::renderer::font::{layout_record_start, layout_record_take, layout_overlaps};

    fn team() -> Roster {
        let mut r = Roster::default_named(0);
        r.name = "FRAGS".into();
        r.worm_names = ["GREB".into(), "THLONKERSON".into(), "GIBS".into(), "SMEG".into()];
        r.hat_ids = [36, 22, 34, 43]; r.uniform_color_ids = [4, 2 | (1 << 5), 1 | (2 << 5), 7 | (4 << 5)];
        r.boot_color_ids = [3, 4, 2, 5]; r.gun_style_ids = [17, 18, 21, 14];
        r
    }

    fn check(name: &str, f: impl FnOnce()) -> Vec<String> {
        layout_record_start();
        f();
        layout_overlaps(&layout_record_take()).into_iter().map(|o| format!("{name}: {o}")).collect()
    }

    /// No text on top of text or on a soldier, in every soldier / tab / scroll position of both screens.
    #[test]
    fn store_and_equip_never_overlap() {
        let mut buf = WorldBuffer::new();
        let mut bad = Vec::new();
        let r = team();
        let all: Vec<u8> = (1..=43).collect();
        let mut st = StoreScreen::new(String::new(), 1015, &[1, 3, 5, 41], &[2], &[1], &[], 150, &r);
        for s in 0..4 { for t in 0..StoreScreen::TABS { for c in 0..StoreScreen::tab_len(t) {
            st.set_view(s, t, c);
            bad.extend(check(&format!("store s{s} t{t} c{c}"), || st.draw(&mut buf)));
        }}}
        let mut eq = CosmeticsScreen::new(r.clone(), all.clone(), all.clone(), all.clone(), all.clone(), String::new(), 1015);
        for s in 0..4 { for c in 0..5 {
            eq.set_view(s, c, 0);
            for f in 0..eq.slot_len(c) {
                eq.set_view(s, c, f);
                bad.extend(check(&format!("equip s{s} c{c} f{f}"), || eq.draw(&mut buf)));
            }
        }}
        assert!(bad.is_empty(), "{} overlaps, first: {:?}", bad.len(), &bad[..bad.len().min(5)]);
    }

    #[test]
    fn scroll_keeps_selection_visible() {
        for n in 0..10 { for sel in 0..n.max(1) { for first in 0..n + 2 {
            let f = scroll_to(first, sel, n);
            if n > 0 { assert!(sel >= f && sel < f + VISIBLE, "n{n} sel{sel} first{first} -> {f}"); }
            assert!(f + VISIBLE <= n.max(VISIBLE));
        }}}
    }
}
