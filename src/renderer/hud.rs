use crate::world::{SCREEN_W, SCREEN_H, WORLD_H};
use super::buffer::WorldBuffer;
use super::fb::Bgra;
use super::font::{draw_str, draw_str_shadow, draw_str_scaled, draw_str_shadow_scaled, str_width, str_width_scaled};
use super::draw_sprites::TEAM_COLOURS;

/// Height of the HUD bar in pixels.
pub const HUD_H: u32 = 18;

/// Screen-relative Y offset where the HUD bar starts (from viewport top).
pub const HUD_Y: i32 = SCREEN_H as i32 - HUD_H as i32;

/// World Y where the HUD bar starts, given the camera top edge.
#[inline]
pub fn hud_world_y(cam_y: u32) -> i32 { cam_y as i32 + HUD_Y }

const HUD_BG:     Bgra = Bgra::new(15, 15, 25);
const HUD_TEXT:   Bgra = Bgra::new(220, 220, 220);
const HUD_YELLOW: Bgra = Bgra::new(255, 220, 0);
const HUD_RED:    Bgra = Bgra::new(220, 60, 60);

// Shared palette used across all menus/screens
pub const COLOR_DARK_BG:  Bgra = Bgra::new(8, 10, 22);
pub const COLOR_PANEL_BG: Bgra = Bgra::new(14, 18, 40);
pub const COLOR_BORDER:   Bgra = Bgra::new(50, 50, 90);
pub const COLOR_DIM_TEXT: Bgra = Bgra::new(140, 140, 170);
pub const COLOR_GOLD:     Bgra = Bgra::new(220, 180, 50);

/// Per-team end-of-match stats shown on the game-over screen.
/// Kills aren't attributed per team (the sim doesn't track killers), so the
/// screen reports survivors + HP, which generalizes to any team count.
pub struct TeamEndStat {
    pub color_id: u8,
    pub alive:    u32,
    pub total:    u32,
    pub hp:       u32,
}

/// Colour-identity display name (0-3 = Red/Blue/Green/Yellow).
pub fn team_colour_name(color_id: u8) -> &'static str {
    match color_id { 0 => "RED", 1 => "BLUE", 2 => "GREEN", _ => "YELLOW" }
}

/// Draw a game-over overlay centred on screen.
/// `winner_team`: which team won (None = draw).
/// `my_team`: the local player's team slot (None = hotseat / no single local player).
pub fn draw_game_over(
    buf:          &mut WorldBuffer,
    winner_team:  Option<usize>,
    my_team:      Option<usize>,
    cam_x:        i32,
    cam_y:        u32,
    winner_avatar: u8,
    elo_delta:    i32,
    scrap_earned: u32,
    stats:        &[TeamEndStat],
    memo_line:    &str,
    winner_color: u8,        // colour identity (0-3) of the winning team
) {
    let sw  = SCREEN_W as i32;
    let sh  = SCREEN_H as i32;
    let oy  = cam_y as i32; // world Y origin of screen top
    let cx0 = cam_x;
    let mid = cx0 + sw / 2;

    const AV: u32 = 90;
    let av_y      = oy + 5i32;
    let dark_top  = av_y + AV as i32 + 4;
    buf.fill_rect(cx0, oy, SCREEN_W, sh as u32, Bgra::new(6, 8, 22));
    buf.fill_rect(cx0, dark_top, SCREEN_W, 2, COLOR_BORDER);

    let y_headline = dark_top + 14;
    let y_subtext  = dark_top + 62;
    let y_divider  = dark_top + 92;
    let y_stats    = dark_top + 112;
    // Stat rows cascade: tighter spacing when 3-4 teams so everything below
    // still fits on screen. 2 teams keeps the roomy legacy layout.
    let row_h      = if stats.len() <= 2 { 32 } else { 24 };
    let y_memo     = (y_stats + stats.len().max(2) as i32 * row_h + 24).max(dark_top + 185);
    let y_elo      = y_memo + 55;
    let y_hint     = oy + sh - 26;

    match winner_team {
        None => {
            // Draw — no avatar needed
            let msg = "IT'S A DRAW!";
            let mw = str_width_scaled(msg, 4);
            draw_str_shadow_scaled(buf, msg, mid - mw/2, y_headline, HUD_TEXT, 4);
            draw_button_hints(buf, &[("A", "CONTINUE")], cx0, cam_y);
        }
        Some(winner) => {
            let team_col  = TEAM_COLOURS[winner_color.min(3) as usize];
            let team_name = team_colour_name(winner_color);

            // Avatar
            {
                use super::avatar::draw_avatar;
                draw_avatar(buf, mid - AV as i32 / 2, av_y, AV, winner_avatar);
            }

            // Headline
            let (headline, headline_col) = match my_team {
                Some(me) if me == winner => ("YOU'RE A WINNER!", Bgra::new(255, 230, 50)),
                Some(_)                  => ("YOU'RE A LOSER!",  Bgra::new(220, 70, 70)),
                None => {
                    let s: &'static str = match winner_color {
                        0 => "RED TEAM WINS!",
                        1 => "BLUE TEAM WINS!",
                        2 => "GREEN TEAM WINS!",
                        _ => "YELLOW TEAM WINS!",
                    };
                    (s, team_col)
                }
            };
            let hw = str_width_scaled(headline, 4);
            draw_str_shadow_scaled(buf, headline, mid - hw/2, y_headline, headline_col, 4);

            // Team wins bar
            let sub  = format!("{} TEAM WINS", team_name);
            let subw = str_width_scaled(&sub, 2);
            buf.fill_rect(mid - subw/2 - 10, y_subtext - 2, (subw + 20) as u32, 20, team_col);
            draw_str_scaled(buf, &sub, mid - subw/2, y_subtext, Bgra::new(0, 0, 0), 2);

            // Divider
            buf.fill_rect(cx0 + 20, y_divider, (sw - 40) as u32, 1, Bgra::new(50, 50, 80));

            // Per-team survivor/HP stats — one centered row per team, name in
            // team colour. Works for 2-4 team matches.
            for (i, ts) in stats.iter().enumerate() {
                let name = team_colour_name(ts.color_id);
                let row  = format!("{:<7}{}/{} alive  {} HP", name, ts.alive, ts.total, ts.hp);
                let rw   = str_width_scaled(&row, 2) as i32;
                let ry   = y_stats + i as i32 * row_h;
                draw_str_scaled(buf, &row, mid - rw/2, ry, Bgra::new(150, 150, 180), 2);
                // Re-draw the name portion in the team's colour.
                draw_str_scaled(buf, name, mid - rw/2, ry, TEAM_COLOURS[ts.color_id.min(3) as usize], 2);
            }

            // Fun stat + quip — near screen centre; scale down if too wide
            if !memo_line.is_empty() {
                let scale = if str_width_scaled(memo_line, 2) as i32 <= sw - 40 { 2 } else { 1 };
                let mw = str_width_scaled(memo_line, scale) as i32;
                draw_str_scaled(buf, memo_line, mid - mw/2, y_memo, Bgra::new(190, 200, 230), scale);
            }

            // ELO delta
            if elo_delta != 0 {
                let sign    = if elo_delta > 0 { "+" } else { "" };
                let elo_str = format!("{}{}  ELO", sign, elo_delta);
                let elo_col = if elo_delta > 0 { Bgra::new(80, 220, 120) } else { Bgra::new(220, 80, 80) };
                let ew = str_width_scaled(&elo_str, 2);
                draw_str_scaled(buf, &elo_str, mid - ew/2, y_elo, elo_col, 2);
            }

            // Scrap earned
            if scrap_earned > 0 {
                let scrap_str = format!("+{}  SCRAP", scrap_earned);
                let sw = str_width_scaled(&scrap_str, 2);
                draw_str_scaled(buf, &scrap_str, mid - sw/2, y_elo + 36, COLOR_GOLD, 2);
            }

            draw_button_hints(buf, &[("A", "CONTINUE")], cx0, cam_y);
        }
    }
}

/// Draw a row of button hints centered at the bottom of the screen.
/// `hints` is a slice of (button_label, action_label) pairs, e.g. `&[("A", "SELECT"), ("B", "BACK")]`.
pub fn draw_button_hints(buf: &mut WorldBuffer, hints: &[(&str, &str)], cam_x: i32, cam_y: u32) {
    use super::font::{draw_str, str_width};
    let parts: Vec<String> = hints.iter().map(|(b, a)| format!("{}={}", b, a)).collect();
    let line = parts.join("  ");
    let x = cam_x + (SCREEN_W as i32 - str_width(&line)) / 2;
    let y = cam_y as i32 + SCREEN_H as i32 - 14;
    draw_str(buf, &line, x, y, COLOR_DIM_TEXT);
}

/// Draw a unified menu selection highlight for a list item row.
/// Fills with dark panel color, adds a 3px gold left border, and draws a ">" arrow.
/// Call before drawing the item text.
pub fn draw_menu_selection(buf: &mut WorldBuffer, x: i32, y: i32, w: i32, h: i32) {
    use super::font::{draw_str_shadow_scaled, str_width_scaled};
    buf.fill_rect(x, y, w as u32, h as u32, Bgra::new(20, 30, 70));
    buf.fill_rect(x, y, 3, h as u32, Bgra::new(255, 180, 0));
    let arrow_x = x + 8;
    let arrow_y = y + (h - 16) / 2;
    draw_str_shadow_scaled(buf, ">", arrow_x, arrow_y, Bgra::new(255, 180, 0), 2);
}

/// Unified layout for the centered scrolling list menus (title, submenus, my-teams).
pub struct ListStyle {
    pub panel_y:     i32,
    pub item_h:      i32,
    pub max_visible: usize,
    /// true = drawn over the title image (unselected text black);
    /// false = drawn over a dark screen (unselected text light).
    pub on_image:    bool,
}

impl Default for ListStyle {
    fn default() -> Self {
        Self { panel_y: 281, item_h: 38, max_visible: 4, on_image: true }
    }
}

/// Scroll offset that keeps `cursor` inside the visible window.
pub fn list_scroll_for(cursor: usize, max_visible: usize) -> usize {
    cursor.saturating_sub(max_visible - 1)
}

/// Draw a centered scrolling list menu with the unified selection highlight.
/// Items are (label, is_danger) — danger items (e.g. LOG OUT) draw red.
/// `header` is an optional dim line above the list (e.g. logged-in username).
/// Caller handles input; pass `scroll` from `list_scroll_for`.
pub fn draw_list_panel(
    buf:    &mut WorldBuffer,
    header: Option<&str>,
    items:  &[(&str, bool)],
    cursor: usize,
    scroll: usize,
    style:  &ListStyle,
) {
    let sw = SCREEN_W as i32;
    let start_y = style.panel_y + 8;
    if let Some(h) = header {
        draw_str(buf, h, sw/2 - str_width(h)/2, style.panel_y - 14, Bgra::new(110, 115, 165));
    }
    if scroll > 0 {
        draw_str(buf, "^", sw/2 - 8, start_y - 16, Bgra::new(180, 180, 220));
    }
    let visible = items.iter().enumerate().skip(scroll).take(style.max_visible);
    let mut shown = 0i32;
    for (i, &(item, danger)) in visible {
        let iy = start_y + shown * style.item_h;
        shown += 1;
        let iw = str_width_scaled(item, 2);
        let selected = i == cursor;
        if selected {
            draw_menu_selection(buf, sw/2 - 155, iy - 4, 310, 28);
        }
        let col = match (selected, danger) {
            (true,  true)  => Bgra::new(255, 100, 80),
            (true,  false) => Bgra::new(255, 225, 55),
            (false, true)  => Bgra::new(180, 70, 60),
            (false, false) => if style.on_image { Bgra::new(0, 0, 0) } else { Bgra::new(170, 170, 200) },
        };
        draw_str_shadow_scaled(buf, item, sw/2 - iw/2, iy, col, 2);
    }
    if scroll + style.max_visible < items.len() {
        let arrow_y = start_y + shown * style.item_h;
        draw_str(buf, "v", sw/2 - 8, arrow_y, Bgra::new(180, 180, 220));
    }
}

/// One row of a [`draw_side_menu`] list.
pub struct SideItem<'a> {
    pub label:  &'a str,
    /// Small red count badge at the row's right end ("" = none).
    pub badge:  &'a str,
    /// Red text (e.g. LOG OUT).
    pub danger: bool,
    /// Value shown right-aligned in the row (e.g. a soldier's name in the team editor); "" = none.
    /// With a detail the label is drawn small and dim, the detail big.
    pub detail: &'a str,
}

impl<'a> SideItem<'a> {
    pub fn new(label: &'a str) -> Self { Self { label, badge: "", danger: false, detail: "" } }
}

/// Info card shown beside the list for the highlighted item.
pub struct SideCard<'a> {
    pub title: &'a str,
    pub body:  &'a str,
    /// (label, value, value colour) rows under the body.
    pub stats: &'a [(String, String, Bgra)],
}

pub const SIDE_GOLD:   Bgra = Bgra::new(255, 225, 55);
pub const SIDE_GREEN:  Bgra = Bgra::new(90, 230, 120);
pub const SIDE_WHITE:  Bgra = Bgra::new(240, 240, 255);
pub const SIDE_SCRAP:  Bgra = Bgra::new(230, 200, 80);
pub const SIDE_RED:    Bgra = Bgra::new(230, 70, 60);
pub const SIDE_DIM:    Bgra = Bgra::new(150, 155, 195);
pub const SIDE_TEXT:   Bgra = Bgra::new(205, 210, 235);
pub const SIDE_ACCENT: Bgra = Bgra::new(255, 180, 0);

/// Layout of the side screens (640x480): content between the header and the footer.
pub const SIDE_TOP: i32 = 58;
pub const SIDE_BOTTOM: i32 = SCREEN_H as i32 - 30;
/// Item column (x, width) and the info card's x.
pub const SIDE_COL_X: i32 = 14;
pub const SIDE_COL_W: i32 = 372;
pub const SIDE_CARD_X: i32 = SIDE_COL_X + SIDE_COL_W + 10;

/// Darken + tint a rect in place (cheap "glass" panel over the title art).
pub fn side_shade(buf: &mut WorldBuffer, x: i32, y: i32, w: i32, h: i32, keep: u32, tint: (u32, u32, u32)) {
    let (sw, sh) = (SCREEN_W as i32, SCREEN_H as i32);
    for yy in y.max(0)..(y + h).min(sh) {
        for xx in x.max(0)..(x + w).min(sw) {
            let p = buf.get_pixel(xx, yy);
            let m = |c: u8, t: u32| ((c as u32 * keep + t * (256 - keep)) / 256) as u8;
            buf.set_pixel(xx, yy, Bgra::new(m(p.r, tint.0), m(p.g, tint.1), m(p.b, tint.2)));
        }
    }
}

/// A dark glass panel with the gold top edge (cards, popups).
pub fn side_panel(buf: &mut WorldBuffer, x: i32, y: i32, w: i32, h: i32) {
    side_shade(buf, x, y, w, h, 45, (10, 14, 40));
    buf.fill_rect(x, y, w.max(0) as u32, 2, SIDE_ACCENT);
}

/// A row-style highlight: lighter glass + gold bar on the left when `selected`, faint glass otherwise.
pub fn side_row_bg(buf: &mut WorldBuffer, x: i32, y: i32, w: i32, h: i32, selected: bool) {
    if selected {
        side_shade(buf, x, y, w, h, 60, (20, 30, 90));
        buf.fill_rect(x, y, 4, h.max(0) as u32, SIDE_ACCENT);
    } else {
        side_shade(buf, x, y, w, h, 150, (8, 10, 30));
    }
}

/// Background + header + footer of every side screen: dimmed title art, the screen name big on the left,
/// `who` (username, ELO, scrap...) on the right, and button hints at the bottom.
pub fn side_frame(buf: &mut WorldBuffer, crumb: &str, who: &str, hints: &[(&str, &str)]) {
    let (sw, sh) = (SCREEN_W as i32, SCREEN_H as i32);
    super::title_bg::draw_title_bg(buf, 0);
    side_shade(buf, 0, 0, sw, sh, 55, (6, 8, 24));
    side_shade(buf, 0, 0, sw, 44, 40, (10, 14, 40));
    buf.fill_rect(0, 44, SCREEN_W, 2, SIDE_ACCENT);
    let scale = if str_width_scaled(crumb, 3) + str_width(who) + 50 <= sw { 3 } else { 2 };
    draw_str_shadow_scaled(buf, crumb, 18, if scale == 3 { 10 } else { 14 }, SIDE_GOLD, scale);
    draw_str(buf, who, sw - str_width(who) - 14, 17, Bgra::new(200, 205, 235));
    side_shade(buf, 0, sh - 24, sw, 24, 40, (10, 14, 40));
    draw_button_hints(buf, hints, 0, 4);
}

/// The big item rows in the left column, sharing the full height (no scrolling, max 96 px each).
pub fn side_rows(buf: &mut WorldBuffer, items: &[SideItem], cursor: usize) {
    let (col_x, col_w, top) = (SIDE_COL_X, SIDE_COL_W, SIDE_TOP);
    let n = items.len().max(1) as i32;
    let row_h = ((SIDE_BOTTOM - top) / n).min(96);
    let any_detail = items.iter().any(|it| !it.detail.is_empty());
    // One label size per screen: biggest that fits every row (badge included).
    let scale = if !any_detail && items.iter().all(|it| str_width_scaled(it.label, 3)
        + if it.badge.is_empty() { 10 } else { 50 } <= col_w - 22) { 3 } else { 2 };
    for (i, it) in items.iter().enumerate() {
        let y = top + i as i32 * row_h;
        let sel = i == cursor;
        side_row_bg(buf, col_x, y, col_w, row_h - 6, sel);
        let col = match (sel, it.danger) {
            (true, true)   => Bgra::new(255, 100, 80),
            (true, false)  => SIDE_GOLD,
            (false, true)  => Bgra::new(200, 80, 70),
            (false, false) => Bgra::new(225, 228, 245),
        };
        let mid = y + (row_h - 6) / 2;
        if it.detail.is_empty() && any_detail {
            // Action row (e.g. SAVE) among value rows: same size as the values, so the column stays even.
            draw_str_shadow_scaled(buf, it.label, col_x + 18, mid - 8, col, 2);
        } else if it.detail.is_empty() {
            draw_str_shadow_scaled(buf, it.label, col_x + 18, mid - 4 * scale, col, scale);
        } else {
            // Label on the left (2x when it fits beside the value, else small), value big on the right.
            let dw = str_width_scaled(it.detail, 2);
            let lc = if sel { SIDE_GOLD } else { SIDE_DIM };
            if str_width_scaled(it.label, 2) + dw + 50 <= col_w {
                draw_str_shadow_scaled(buf, it.label, col_x + 18, mid - 8, lc, 2);
            } else {
                draw_str(buf, it.label, col_x + 18, mid - 4, lc);
            }
            draw_str_shadow_scaled(buf, it.detail, col_x + col_w - 14 - dw, mid - 8, if sel { SIDE_GOLD } else { SIDE_WHITE }, 2);
        }
        if !it.badge.is_empty() {
            let bw = str_width_scaled(it.badge, 2) + 12;
            let bx = col_x + col_w - bw - 10;
            let by = mid - 11;
            buf.fill_rect(bx, by, bw as u32, 22, Bgra::new(210, 60, 50));
            draw_str_shadow_scaled(buf, it.badge, bx + 6, by + 3, Bgra::new(255, 255, 255), 2);
        }
    }
}

/// The info card's rect (x, y, w, h).
pub fn side_card_rect() -> (i32, i32, i32, i32) {
    let cx = SIDE_CARD_X;
    (cx, SIDE_TOP, SCREEN_W as i32 - cx - 10, SIDE_BOTTOM - SIDE_TOP - 6)
}

/// Draw the info card; returns the y below what it drew, for screens that add their own art.
pub fn side_card(buf: &mut WorldBuffer, card: &SideCard) -> i32 {
    use super::font::wrap_text;
    let (cx, cy, cw, ch) = side_card_rect();
    side_panel(buf, cx, cy, cw, ch);
    let ts = if str_width_scaled(card.title, 2) <= cw - 24 { 2 } else { 1 };
    draw_str_shadow_scaled(buf, card.title, cx + 12, cy + 14, SIDE_GOLD, ts);
    let mut y = cy + 44;
    for line in wrap_text(card.body, 1, cw - 24) {
        draw_str(buf, &line, cx + 12, y, SIDE_TEXT);
        y += 15;
    }
    if !card.stats.is_empty() { y += 14; }
    for (k, v, c) in card.stats {
        if y + 40 > cy + ch { break; }
        buf.fill_rect(cx + 12, y - 6, (cw - 24) as u32, 1, Bgra::new(60, 64, 110));
        draw_str(buf, k, cx + 12, y, SIDE_DIM);
        y += 14;
        let vs = if str_width_scaled(v, 2) <= cw - 24 { 2 } else { 1 };
        let vw = str_width_scaled(v, vs);
        draw_str_shadow_scaled(buf, v, cx + cw - 12 - vw, y, *c, vs);
        y += 26;
    }
    y
}

/// A full screen that just says something (loading, saved, errors...): frame + one centred glass panel.
pub fn side_message(buf: &mut WorldBuffer, crumb: &str, msg: &str, col: Bgra, hints: &[(&str, &str)]) {
    let sw = SCREEN_W as i32;
    side_frame(buf, crumb, "", hints);
    let ms = if str_width_scaled(msg, 2) <= sw - 80 { 2 } else { 1 };
    let mw = str_width_scaled(msg, ms);
    let pw = (mw + 80).max(360).min(sw - 40);
    side_panel(buf, sw / 2 - pw / 2, 196, pw, 76);
    draw_str_shadow_scaled(buf, msg, sw / 2 - mw / 2, 226, col, ms);
}

/// Full-screen submenu layout used by every menu below the title screen:
/// `side_frame` + `side_rows` + `side_card`.
pub fn draw_side_menu(
    buf:    &mut WorldBuffer,
    crumb:  &str,
    who:    &str,
    items:  &[SideItem],
    cursor: usize,
    card:   &SideCard,
    hints:  &[(&str, &str)],
) {
    side_frame(buf, crumb, who, hints);
    side_rows(buf, items, cursor);
    side_card(buf, card);
}

/// Standard full-width screen header bar with a centered 2x title.
pub fn draw_screen_header(buf: &mut WorldBuffer, title: &str) {
    let sw = SCREEN_W as i32;
    buf.fill_rect(0, 0, SCREEN_W, 36, Bgra::new(18, 22, 50));
    buf.fill_rect(0, 36, SCREEN_W, 1, Bgra::new(60, 60, 120));
    let tw = str_width_scaled(title, 2);
    draw_str_shadow_scaled(buf, title, sw/2 - tw/2, 9, Bgra::new(255, 220, 50), 2);
}

/// Draw the pause menu overlay.
/// Returns true if Quit was selected, false if Resume.
/// `cursor` is 0 = Resume, 1 = Quit.
pub fn draw_pause_menu(buf: &mut WorldBuffer, cursor: u8, cam_x: i32, cam_y: u32) {
    draw_pause_menu_items(buf, &["RESUME", "EXIT MATCH"], cursor, cam_x, cam_y);
}

/// Pause menu with a caller-supplied item list (TEST mode adds "PAUSE GAME", a full freeze that also stops the turn timer).
pub fn draw_pause_menu_items(buf: &mut WorldBuffer, items: &[&str], cursor: u8, cam_x: i32, cam_y: u32) {
    use super::font::{draw_str_scaled, str_width_scaled};
    let panel_w = 230u32;
    let panel_h = 50 + 30 * items.len() as u32;
    let panel_x = cam_x + (SCREEN_W - panel_w) as i32 / 2;
    let panel_y = cam_y as i32 + (SCREEN_H - panel_h) as i32 / 2;

    // Background + border (shared menu palette)
    buf.fill_rect(panel_x, panel_y, panel_w, panel_h, COLOR_PANEL_BG);
    buf.fill_rect(panel_x, panel_y, panel_w, 2, COLOR_BORDER);
    buf.fill_rect(panel_x, panel_y + panel_h as i32 - 2, panel_w, 2, COLOR_BORDER);
    buf.fill_rect(panel_x, panel_y, 2, panel_h, COLOR_BORDER);
    buf.fill_rect(panel_x + panel_w as i32 - 2, panel_y, 2, panel_h, COLOR_BORDER);

    // Title — 2x scaled to match other menus
    let title = "PAUSED";
    let tw = str_width_scaled(title, 2);
    draw_str_scaled(buf, title,
        panel_x + (panel_w as i32 - tw) / 2,
        panel_y + 12,
        Bgra::new(255, 210, 50), 2);

    // Menu items — 2x scaled
    for (i, &item) in items.iter().enumerate() {
        let item_y = panel_y + 46 + i as i32 * 30;
        let selected = i as u8 == cursor;
        if selected {
            draw_menu_selection(buf, panel_x + 10, item_y - 2, panel_w as i32 - 20, 20);
        }
        let col = if selected { Bgra::new(255, 225, 55) } else { Bgra::new(170, 170, 200) };
        let iw = str_width_scaled(item, 2);
        draw_str_scaled(buf, item, panel_x + (panel_w as i32 - iw) / 2, item_y, col, 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf() -> WorldBuffer { WorldBuffer::new() }

    fn stats_n(n: usize) -> Vec<TeamEndStat> {
        (0..n).map(|i| TeamEndStat {
            color_id: i as u8,
            alive:    if i == 0 { 4 } else { 0 },
            total:    4,
            hp:       if i == 0 { 400 } else { 0 },
        }).collect()
    }

    // ── Game over ────────────────────────────────────────────────────────────

    #[test]
    fn game_over_winner_draws_without_panic() {
        let mut b = buf();
        draw_game_over(&mut b, Some(0), Some(0), 0, 0, 0, 0, 0, &stats_n(2), "", 0);
        draw_game_over(&mut b, Some(3), Some(0), 0, 0, 3, 0, 0, &stats_n(4), "memo line", 3);
        draw_game_over(&mut b, Some(1), None,    0, 0, 1, 0, 0, &stats_n(3), "", 1);
    }

    #[test]
    fn game_over_draw_draws_without_panic() {
        let mut b = buf();
        draw_game_over(&mut b, None, None, 0, 0, 0, 0, 0, &stats_n(2), "", 0);
        draw_game_over(&mut b, None, None, 0, 0, 0, 0, 0, &[], "", 0);
    }

    #[test]
    fn team_colour_names_cover_all_ids() {
        assert_eq!(team_colour_name(0), "RED");
        assert_eq!(team_colour_name(1), "BLUE");
        assert_eq!(team_colour_name(2), "GREEN");
        assert_eq!(team_colour_name(3), "YELLOW");
        assert_eq!(team_colour_name(9), "YELLOW"); // out-of-range falls through
    }

    // ── Pause menu / list panel ──────────────────────────────────────────────

    #[test]
    fn pause_menu_draws_without_panic() {
        let mut b = buf();
        draw_pause_menu(&mut b, 0, 0, 0);
        draw_pause_menu(&mut b, 1, 0, 0);
    }

    #[test]
    fn list_panel_draws_with_scroll_and_danger() {
        let mut b = buf();
        let items = [("ONE", false), ("TWO", false), ("THREE", false),
                     ("FOUR", false), ("LOG OUT", true)];
        let style = ListStyle::default();
        let scroll = list_scroll_for(4, style.max_visible);
        assert_eq!(scroll, 1);
        draw_list_panel(&mut b, Some("header"), &items, 4, scroll, &style);
    }
}
