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
    use super::font::{draw_str_scaled, str_width_scaled};
    let panel_w = 230u32;
    let panel_h = 110u32;
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
    let items = ["RESUME", "EXIT MATCH"];
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
