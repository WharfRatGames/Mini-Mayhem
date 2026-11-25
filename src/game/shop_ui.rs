//! Shared STORE / EQUIP layout: a strip of the team's 4 soldiers on top (L1/R1), category tabs
//! (LEFT/RIGHT), then one big card of the chosen soldier in the highlighted item and a list of
//! items (UP/DOWN), each row showing him wearing it. Pictures are fitted to their box (hats never cut off) and every text sits
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
    /// First list row shown.
    pub first: usize,
    /// Highlighted row (index into `tiles`); the big card shows it.
    pub sel: Option<usize>,
    /// "OF 36 FOR SALE" / "OF 23 OWNED" suffix.
    pub what: &'a str,
    /// Feedback (e.g. BOUGHT CROWN!) shown in the card's top line instead of the count.
    pub status: Option<(&'a str, Bgra)>,
}

/// List rows on screen.
pub const VISIBLE: usize = 4;
pub const GREEN: Bgra = Bgra::new(90, 230, 120);
pub const RED: Bgra = Bgra::new(220, 80, 70);

/// First visible row so that `sel` is on screen, moving the window as little as possible from `first`.
pub fn scroll_to(first: usize, sel: usize, n: usize) -> usize {
    let mut f = first;
    if sel < f { f = sel; }
    if sel >= f + VISIBLE { f = sel + 1 - VISIBLE; }
    f.min(n.saturating_sub(VISIBLE))
}

const CARD_W: i32 = 200;
const ROW_H: i32 = 69;

/// Left: the soldier in the highlighted item (count / feedback on top, name + tag under him).
fn card(buf: &mut WorldBuffer, x: i32, y: i32, w: i32, h: i32, fit: Fit, top_line: (&str, Bgra), t: Option<&Tile>) {
    buf.fill_rect(x, y, w as u32, h as u32, Bgra::new(30, 42, 100));
    for (xx, yy, ww, hh) in [(x, y, w, 3), (x, y + h - 3, w, 3), (x, y, 3, h), (x + w - 3, y, 3, h)] {
        buf.fill_rect(xx, yy, ww as u32, hh as u32, hud::SIDE_ACCENT);
    }
    let mut line = top_line.0.to_string();
    while str_width(&line) > w - 12 && line.len() > 1 { line.pop(); }
    draw_str(buf, &line, x + w / 2 - str_width(&line) / 2, y + 10, top_line.1);
    let pic_top = 26;
    let text_top = y + h - 52;
    buf.fill_rect(x + 14, text_top - 6, (w - 28) as u32, 2, Bgra::new(50, 58, 110));
    let fit = match t { Some(t) => fit.with(t.cat, t.id), None => fit };
    fit.draw_in(buf, x + 10, y + pic_top, w - 20, text_top - 10 - y - pic_top, 8);
    let Some(t) = t else { return };
    let mut n = t.name.clone();
    let s = if str_width_scaled(&n, 2) <= w - 12 { 2 } else { 1 };
    while str_width_scaled(&n, s) > w - 12 && n.len() > 1 { n.pop(); }
    draw_str_shadow_scaled(buf, &n, x + w / 2 - str_width_scaled(&n, s) / 2, text_top + 4, hud::SIDE_GOLD, s);
    if !t.tag.is_empty() { draw_str(buf, &t.tag, x + w / 2 - str_width(&t.tag) / 2, text_top + 28, t.tag_col); }
}

pub fn draw(buf: &mut WorldBuffer, v: &ShopView) {
    let (sw, bot) = (SCREEN_W as i32, hud::SIDE_BOTTOM);
    hud::side_frame_plain(buf, v.title, v.info, v.hints);
    let tabs_y = draw_strip(buf, v.soldiers, v.cur) + 8;
    draw_tabs(buf, v.tabs, v.tab, tabs_y);

    let top = tabs_y + 36;
    let h = bot - top;
    let n = v.tiles.len();
    let me = v.soldiers[v.cur.min(3)].1;
    let sel = v.sel.filter(|&s| s < n);
    let count = match sel { Some(s) => format!("{} OF {} {}", s + 1, n, v.what), None => format!("{} {}", n, v.what) };
    let top_line = v.status.unwrap_or((count.as_str(), hud::SIDE_GOLD));
    card(buf, 14, top, CARD_W, h, me, top_line, sel.map(|s| &v.tiles[s]));

    // List: tall rows, the soldier wearing the item at 2x, name in the big font, tag on its own line.
    let lx = 14 + CARD_W + 10;
    let lw = sw - 14 - lx - 8;
    let first = v.first.min(n.saturating_sub(VISIBLE));
    for r in 0..VISIBLE {
        let Some(item) = v.tiles.get(first + r) else { break };
        let ry = top + r as i32 * ROW_H;
        let on = sel == Some(first + r);
        buf.fill_rect(lx, ry, lw as u32, (ROW_H - 4) as u32, if on { Bgra::new(30, 42, 100) } else { Bgra::new(20, 26, 60) });
        if on { buf.fill_rect(lx, ry, 4, (ROW_H - 4) as u32, hud::SIDE_ACCENT); }
        me.with(item.cat, item.id).draw_in(buf, lx + 6, ry, 56, ROW_H - 4, 2);
        let tx = lx + 70;
        let avail = lw - 70 - 8;
        let mut nm = item.name.clone();
        let s = if str_width_scaled(&nm, 2) <= avail { 2 } else { 1 };
        while str_width_scaled(&nm, s) > avail && nm.len() > 1 { nm.pop(); }
        draw_str_shadow_scaled(buf, &nm, tx, ry + 16, if on { hud::SIDE_GOLD } else { Bgra::new(215, 220, 240) }, s);
        if !item.tag.is_empty() { draw_str(buf, &item.tag, tx, ry + 40, item.tag_col); }
    }
    // Scrollbar
    buf.fill_rect(sw - 18, top, 3, h as u32, Bgra::new(30, 34, 70));
    if n > VISIBLE {
        let bar = (h * VISIBLE as i32 / n as i32).max(8);
        let by = top + (h - bar) * first as i32 / (n - VISIBLE) as i32;
        buf.fill_rect(sw - 18, by, 3, bar as u32, hud::SIDE_ACCENT);
    }
}

/// Soldier strip (4 tabs: picture left, name right). Returns its bottom y.
pub fn draw_strip(buf: &mut WorldBuffer, soldiers: &[(String, Fit); 4], cur: usize) -> i32 {
    let (sw, top) = (SCREEN_W as i32, hud::SIDE_TOP);
    let tab_w = (sw - 28 - 3 * 8) / 4;
    let tab_h = 70;
    for (i, (n, f)) in soldiers.iter().enumerate() {
        let x = 14 + i as i32 * (tab_w + 8);
        let on = i == cur;
        buf.fill_rect(x, top, tab_w as u32, tab_h as u32, if on { Bgra::new(30, 42, 100) } else { Bgra::new(20, 26, 60) });
        if on { buf.fill_rect(x, top + tab_h - 3, tab_w as u32, 3, hud::SIDE_ACCENT); }
        f.draw_in(buf, x + 4, top + 3, 54, tab_h - 6, 2);
        let mut nm = n.to_uppercase();
        while str_width(&nm) > tab_w - 66 && nm.len() > 1 { nm.pop(); }
        draw_str(buf, &nm, x + 62, top + tab_h / 2 - 4, if on { hud::SIDE_GOLD } else { Bgra::new(215, 220, 240) });
    }
    top + tab_h
}

/// Category / slot tabs at `y`, equal width, all labels at one scale. 28 px tall.
pub fn draw_tabs(buf: &mut WorldBuffer, tabs: &[&str], tab: usize, y: i32) {
    let sw = SCREEN_W as i32;
    let nt = tabs.len().max(1) as i32;
    let tabw = (sw - 28 - (nt - 1) * 3) / nt;
    let ls = if tabs.iter().all(|t| str_width_scaled(t, 2) <= tabw - 2) { 2 } else { 1 };
    for (i, label) in tabs.iter().enumerate() {
        let on = i == tab;
        let tx = 14 + i as i32 * (tabw + 3);
        buf.fill_rect(tx, y, tabw as u32, 28, if on { Bgra::new(30, 42, 100) } else { Bgra::new(20, 26, 60) });
        if on { buf.fill_rect(tx, y + 25, tabw as u32, 3, hud::SIDE_ACCENT); }
        draw_str_shadow_scaled(buf, label, tx + tabw / 2 - str_width_scaled(label, ls) / 2, y + 14 - 4 * ls,
                               if on { hud::SIDE_GOLD } else { hud::SIDE_DIM }, ls);
    }
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
