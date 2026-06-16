use crate::world::{SCREEN_W, SCREEN_H, WORLD_H};
use crate::physics::Wind;
use super::buffer::WorldBuffer;
use super::fb::Bgra;
use super::font::{draw_str, draw_str_shadow, draw_str_scaled, str_width, str_width_scaled};
use super::draw_sprites::TEAM_COLOURS;

/// Height of the HUD bar in pixels.
pub const HUD_H: u32 = 18;

/// Y coordinate where the HUD bar starts.
pub const HUD_Y: i32 = SCREEN_H as i32 - HUD_H as i32;

const HUD_BG:     Bgra = Bgra::new(15, 15, 25);
const HUD_TEXT:   Bgra = Bgra::new(220, 220, 220);
const HUD_YELLOW: Bgra = Bgra::new(255, 220, 0);
const HUD_RED:    Bgra = Bgra::new(220, 60, 60);

/// Draw the full HUD bar at the bottom of the screen.
///
/// `wind`         — current wind state
/// `turn_secs`    — seconds remaining in this turn
/// `turn_number`  — current turn count
/// `active_team`  — which team slot (0-3) is currently acting
/// `team_alive`   — how many soldiers alive per team (len 4)
/// `total_hp`     — total HP remaining per team (len 4)
pub fn draw_hud(
    buf:         &mut WorldBuffer,
    wind:        &Wind,
    turn_secs:   u32,
    turn_number: u32,
    active_team: usize,
    team_alive:  &[u32; 4],
    total_hp:    &[u32; 4],
) {
    // ── Background bar ────────────────────────────────────────────────────────
    buf.fill_rect(0, HUD_Y, SCREEN_W, HUD_H, HUD_BG);

    // ── Turn timer (right side) ───────────────────────────────────────────────
    let timer_str = format!("{:02}", turn_secs);
    let timer_colour = if turn_secs <= 5 { HUD_RED } else { HUD_YELLOW };
    let timer_x = SCREEN_W as i32 - str_width(&timer_str) - 4;
    draw_str(buf, &timer_str, timer_x, HUD_Y + 6, timer_colour);

    // ── Turn number (left of timer) ───────────────────────────────────────────
    let turn_str = format!("T{}", turn_number);
    draw_str(buf, &turn_str, timer_x - str_width(&turn_str) - 6, HUD_Y + 6, HUD_TEXT);

    // ── Wind indicator (centre) ───────────────────────────────────────────────
    draw_wind_indicator(buf, wind);

    // ── Team health strips (left side) ───────────────────────────────────────
    draw_team_strips(buf, active_team, team_alive, total_hp);
}

/// Draw the wind indicator in the centre of the HUD.
fn draw_wind_indicator(buf: &mut WorldBuffer, wind: &Wind) {
    let centre_x = SCREEN_W as i32 / 2;
    let y = HUD_Y + 8;
    let bar_w = 60i32;
    let bar_h = 4u32;
    let bar_x = centre_x - bar_w / 2;

    let strength = (wind.value().abs() * 10.0).round() as u32;
    let colour = if wind.value() >= 0.0 { Bgra::new(80, 180, 255) } else { Bgra::new(255, 140, 60) };

    // Empty background track
    buf.fill_rect(bar_x, y, bar_w as u32, bar_h, Bgra::new(40, 40, 60));

    // Fill from left edge — length proportional to magnitude, no empty gap on either side
    let fill = (wind.value().abs() * bar_w as f32) as i32;
    if fill > 0 {
        buf.fill_rect(bar_x, y, fill as u32, bar_h, colour);
    }

    // Direction label left or right of bar (e.g. "<7" or "7>")
    let label = if wind.value() < -0.05 {
        format!("<{}", strength)
    } else if wind.value() > 0.05 {
        format!("{}>", strength)
    } else {
        "~".to_string()
    };
    let lw = str_width(&label);
    draw_str(buf, &label, centre_x - lw / 2, HUD_Y + 4, colour);
}

/// Draw the four team health strips on the left of the HUD.
/// Each strip is a coloured bar showing remaining soldiers and HP.
fn draw_team_strips(
    buf:         &mut WorldBuffer,
    active_team: usize,
    team_alive:  &[u32; 4],
    total_hp:    &[u32; 4],
) {
    for team in 0..4usize {
        if team_alive[team] == 0 { continue; } // skip eliminated teams

        let strip_x = 4 + team as i32 * 36;
        let strip_y = HUD_Y + 3;
        let colour  = TEAM_COLOURS[team];

        // Active team gets a bright border
        if team == active_team {
            buf.fill_rect(strip_x - 1, strip_y - 1, 34, 16, HUD_YELLOW);
        }

        // Background
        buf.fill_rect(strip_x, strip_y, 32, 14, HUD_BG);

        // Alive count
        let alive_str = format!("x{}", team_alive[team]);
        draw_str(buf, &alive_str, strip_x + 1, strip_y + 1, colour);

        // HP bar
        let max_hp  = team_alive[team] * 100;
        let hp_frac = if max_hp > 0 {
            (total_hp[team] as f32 / max_hp as f32).clamp(0.0, 1.0)
        } else { 0.0 };
        let bar_w = (28.0 * hp_frac) as u32;

        buf.fill_rect(strip_x + 2, strip_y + 9, 28, 3, Bgra::new(40, 40, 40));
        if bar_w > 0 {
            buf.fill_rect(strip_x + 2, strip_y + 9, bar_w, 3, colour);
        }
    }
}

/// Draw a game-over overlay centred on screen.
/// `winner_team`: which team won (None = draw).
/// `my_team`: the local player's team slot (None = hotseat / no single local player).
pub fn draw_game_over(
    buf:          &mut WorldBuffer,
    winner_team:  Option<usize>,
    my_team:      Option<usize>,
    cam_x:        i32,
    winner_avatar: u8,
    elo_delta:    i32,
    kills:        [u32; 2],  // [team0_kills, team1_kills]
    hp_left:      [u32; 2],  // [team0_hp, team1_hp]
    memo_line:    &str,
    winner_color: u8,        // colour identity (0-3) of the winning team
) {
    let sw  = SCREEN_W as i32;
    let sh  = SCREEN_H as i32;
    let cx0 = cam_x;
    let mid = cx0 + sw / 2;

    // Full-screen dark panel; avatar draws on top of it
    const AV: u32 = 90;
    let av_y      = 5i32;
    let dark_top  = av_y + AV as i32 + 4; // ~99 — used for element anchoring
    buf.fill_rect(cx0, 0, SCREEN_W, sh as u32, Bgra::new(6, 8, 22));
    buf.fill_rect(cx0, dark_top, SCREEN_W, 2, Bgra::new(50, 50, 90));

    // Fixed y anchors below the dark line
    let y_headline = dark_top + 12;   // ~111
    let y_subtext  = dark_top + 48;   // ~147
    let y_divider  = dark_top + 67;   // ~166
    let y_stats    = dark_top + 86;   // ~185
    let y_memo     = dark_top + 140;  // ~239  (near screen centre)
    let y_elo      = dark_top + 168;  // ~267
    let y_hint     = sh - 26;         // 454

    match winner_team {
        None => {
            // Draw — no avatar needed
            let msg = "IT'S A DRAW!";
            let mw = str_width_scaled(msg, 3);
            draw_str_scaled(buf, msg, mid - mw/2 + 1, y_headline + 1, Bgra::new(0,0,0), 3);
            draw_str_scaled(buf, msg, mid - mw/2,     y_headline,     HUD_TEXT, 3);
            let hint = "PRESS A TO CONTINUE";
            draw_str(buf, hint, mid - str_width(hint)/2, y_hint, Bgra::new(100,100,140));
        }
        Some(winner) => {
            let team_col  = TEAM_COLOURS[winner_color.min(3) as usize];
            let team_name = match winner_color { 0 => "RED", 1 => "BLUE", 2 => "GREEN", _ => "YELLOW" };

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
            let hw = str_width_scaled(headline, 3);
            draw_str_scaled(buf, headline, mid - hw/2 + 1, y_headline + 1, Bgra::new(0,0,0), 3);
            draw_str_scaled(buf, headline, mid - hw/2,     y_headline,     headline_col, 3);

            // Team wins bar
            let sub  = format!("{} TEAM WINS", team_name);
            let subw = str_width_scaled(&sub, 2);
            buf.fill_rect(mid - subw/2 - 10, y_subtext - 2, (subw + 20) as u32, 20, team_col);
            draw_str_scaled(buf, &sub, mid - subw/2, y_subtext, Bgra::new(0, 0, 0), 2);

            // Divider
            buf.fill_rect(cx0 + 20, y_divider, (sw - 40) as u32, 1, Bgra::new(50, 50, 80));

            // Kill/HP stats — centered
            let stats0 = format!("RED   {} kills  {} HP", kills[0], hp_left[0]);
            let stats1 = format!("BLUE  {} kills  {} HP", kills[1], hp_left[1]);
            let sw0 = str_width_scaled(&stats0, 2) as i32;
            let sw1 = str_width_scaled(&stats1, 2) as i32;
            draw_str_scaled(buf, &stats0, mid - sw0/2, y_stats,      Bgra::new(150, 150, 180), 2);
            draw_str_scaled(buf, &stats1, mid - sw1/2, y_stats + 24, Bgra::new(150, 150, 180), 2);

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

            let hint = "PRESS A TO CONTINUE";
            draw_str(buf, hint, mid - str_width(hint)/2, y_hint, Bgra::new(100,100,140));
        }
    }
}

/// Draw a countdown overlay (3... 2... 1... GO!).
pub fn draw_countdown(buf: &mut WorldBuffer, secs: u32) {
    let label = if secs == 0 { "GO!".to_string() } else { format!("{}", secs) };
    let cx = SCREEN_W as i32 / 2 - str_width(&label) / 2;
    let cy = SCREEN_H as i32 / 2 - 4;
    draw_str_shadow(buf, &label, cx, cy, HUD_YELLOW);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::Wind;

    fn buf() -> WorldBuffer { WorldBuffer::new() }

    fn default_hud(b: &mut WorldBuffer) {
        draw_hud(
            b,
            &Wind::calm(),
            45,
            1,
            0,
            &[4, 4, 0, 0],
            &[400, 350, 0, 0],
        );
    }

    // ── HUD bar ───────────────────────────────────────────────────────────────

    #[test]
    fn hud_draws_background_at_bottom() {
        let mut b = buf();
        default_hud(&mut b);
        // HUD bar should be HUD_BG colour at HUD_Y
        assert_eq!(b.get_pixel(0, HUD_Y), HUD_BG);
        assert_eq!(b.get_pixel(SCREEN_W as i32 - 1, HUD_Y), HUD_BG);
    }

    #[test]
    fn hud_does_not_draw_above_hud_y() {
        let mut b = buf();
        default_hud(&mut b);
        // Pixel just above HUD should still be black (untouched)
        assert_eq!(b.get_pixel(0, HUD_Y - 1), Bgra::black());
    }

    #[test]
    fn hud_draws_without_panic_all_teams_alive() {
        let mut b = buf();
        draw_hud(&mut b, &Wind::new(0.5), 30, 5, 2, &[4,4,4,4], &[400,400,400,400]);
    }

    #[test]
    fn hud_draws_without_panic_all_teams_dead() {
        let mut b = buf();
        draw_hud(&mut b, &Wind::calm(), 0, 99, 0, &[0,0,0,0], &[0,0,0,0]);
    }

    #[test]
    fn timer_turns_red_at_5_seconds() {
        let mut b_safe   = buf();
        let mut b_danger = buf();
        draw_hud(&mut b_safe,   &Wind::calm(), 10, 1, 0, &[4,0,0,0], &[400,0,0,0]);
        draw_hud(&mut b_danger, &Wind::calm(),  5, 1, 0, &[4,0,0,0], &[400,0,0,0]);
        // Timer is on the right — check that danger has red pixels
        let right_x = SCREEN_W as i32 - 20;
        let mut has_red = false;
        for dx in 0..20i32 {
            let px = b_danger.get_pixel(right_x + dx, HUD_Y + 6);
            if px == HUD_RED { has_red = true; break; }
        }
        assert!(has_red, "timer at 5s should have red pixels");
    }

    // ── Wind indicator ────────────────────────────────────────────────────────

    #[test]
    fn wind_indicator_draws_without_panic() {
        let mut b = buf();
        draw_wind_indicator(&mut b, &Wind::new(0.8));
        draw_wind_indicator(&mut b, &Wind::calm());
        draw_wind_indicator(&mut b, &Wind::new(-1.0));
    }

    // ── Team strips ───────────────────────────────────────────────────────────

    #[test]
    fn team_strips_skip_eliminated_teams() {
        let mut b_all  = buf();
        let mut b_some = buf();
        draw_hud(&mut b_all,  &Wind::calm(), 30, 1, 0, &[4,4,4,4], &[400,400,400,400]);
        draw_hud(&mut b_some, &Wind::calm(), 30, 1, 0, &[4,0,0,0], &[400,0,0,0]);
        // With only one team alive there should be fewer coloured pixels on left
        // Just verify no panic
    }

    #[test]
    fn active_team_gets_yellow_border() {
        let mut b = buf();
        draw_hud(&mut b, &Wind::calm(), 30, 1, 2, &[4,4,4,4], &[400,400,400,400]);
        // Active team 2: strip_x = 4 + 2*36 = 76
        // Border at strip_x-1, strip_y-1
        let strip_x = 4 + 2 * 36 - 1;
        let strip_y = HUD_Y + 2;
        assert_eq!(b.get_pixel(strip_x, strip_y), HUD_YELLOW,
            "active team should have yellow border");
    }

    // ── Game over ────────────────────────────────────────────────────────────

    #[test]
    fn game_over_winner_draws_without_panic() {
        let mut b = buf();
        draw_game_over(&mut b, Some(0), Some(0), 0, 0, 0, [0,0], [0,0], "", 0); // winner sees win message
        draw_game_over(&mut b, Some(3), Some(0), 0, 3, 0, [0,0], [0,0], "", 3); // loser sees lose message
        draw_game_over(&mut b, Some(1), None,    0, 1, 0, [0,0], [0,0], "", 1); // hotseat — team name
    }

    #[test]
    fn game_over_draw_draws_without_panic() {
        let mut b = buf();
        draw_game_over(&mut b, None, None, 0, 0, 0, [0,0], [0,0], "", 0);
    }

    // ── Countdown ────────────────────────────────────────────────────────────

    #[test]
    fn countdown_draws_without_panic() {
        let mut b = buf();
        draw_countdown(&mut b, 3);
        draw_countdown(&mut b, 1);
        draw_countdown(&mut b, 0); // "GO!"
    }
}

/// Draw the pause menu overlay.
/// Returns true if Quit was selected, false if Resume.
/// `cursor` is 0 = Resume, 1 = Quit.
pub fn draw_pause_menu(buf: &mut WorldBuffer, cursor: u8, cam_x: i32) {
    use super::font::{draw_str_scaled, str_width_scaled};
    let panel_w = 230u32;
    let panel_h = 110u32;
    let panel_x = cam_x + (SCREEN_W - panel_w) as i32 / 2;
    let panel_y = (SCREEN_H - panel_h) as i32 / 2;

    // Background + border
    buf.fill_rect(panel_x, panel_y, panel_w, panel_h, Bgra::new(8, 10, 24));
    buf.fill_rect(panel_x, panel_y, panel_w, 2, Bgra::new(80, 80, 140));
    buf.fill_rect(panel_x, panel_y + panel_h as i32 - 2, panel_w, 2, Bgra::new(80, 80, 140));
    buf.fill_rect(panel_x, panel_y, 2, panel_h, Bgra::new(80, 80, 140));
    buf.fill_rect(panel_x + panel_w as i32 - 2, panel_y, 2, panel_h, Bgra::new(80, 80, 140));

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
            buf.fill_rect(panel_x + 10, item_y - 2, panel_w - 20, 20, Bgra::new(30, 35, 70));
        }
        let col = if selected { Bgra::new(255, 220, 50) } else { Bgra::new(170, 170, 200) };
        let iw = str_width_scaled(item, 2);
        if selected {
            draw_str_scaled(buf, ">", panel_x + (panel_w as i32 - iw) / 2 - 22, item_y, Bgra::new(255, 180, 0), 2);
        }
        draw_str_scaled(buf, item, panel_x + (panel_w as i32 - iw) / 2, item_y, col, 2);
    }
}
