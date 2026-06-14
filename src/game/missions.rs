use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
use crate::world::{SCREEN_W, SCREEN_H};
use crate::game::account::{http_get, http_post, json_field};

pub enum MissionsAction { Back }

#[derive(Clone)]
struct Challenge {
    id:          String,
    desc:        String,
    target:      u32,
    scrap:       u32,
    period_type: String, // "daily" or "weekly"
    progress:    u32,
    claimed:     bool,
}

pub struct MissionsScreen {
    token:      String,
    challenges: Vec<Challenge>,
    cursor:     usize,
    message:    String,
    message_timer: u32,
    scrap:      u32,
}

impl MissionsScreen {
    pub fn new(token: String) -> Self {
        Self { token, challenges: Vec::new(), cursor: 0, message: String::new(), message_timer: 0, scrap: 0 }
    }

    pub fn load(&mut self) {
        let path = format!("/api/challenges?token={}", self.token);
        if let Ok(resp) = http_get(&path) {
            self.challenges = parse_challenges(&resp);
        }
        let profile_path = format!("/api/profile?token={}", self.token);
        if let Ok(resp) = http_get(&profile_path) {
            self.scrap = json_field(&resp, "scrap").and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer) -> Option<MissionsAction> {
        if input.just_pressed(Button::B) { return Some(MissionsAction::Back); }

        let claimable: Vec<usize> = self.challenges.iter().enumerate()
            .filter(|(_, c)| !c.claimed && c.progress >= c.target)
            .map(|(i, _)| i)
            .collect();

        if input.just_pressed(Button::Up)   { if self.cursor > 0 { self.cursor -= 1; } }
        if input.just_pressed(Button::Down) {
            let max = claimable.len().saturating_sub(1);
            if self.cursor < max { self.cursor += 1; }
        }

        if input.just_pressed(Button::A) {
            if let Some(&idx) = claimable.get(self.cursor) {
                self.claim(idx);
            }
        }

        if self.message_timer > 0 { self.message_timer -= 1; }

        self.draw(buf);
        None
    }

    fn claim(&mut self, idx: usize) {
        let ch = &self.challenges[idx];
        let body = format!(r#"{{"token":"{}","challenge_id":"{}"}}"#, self.token, ch.id);
        match http_post("/api/challenges/claim", &body) {
            Ok(resp) => {
                let earned: u32 = json_field(&resp, "scrap_earned").and_then(|s| s.parse().ok()).unwrap_or(0);
                let new_scrap: u32 = json_field(&resp, "new_scrap").and_then(|s| s.parse().ok()).unwrap_or(self.scrap + earned);
                self.challenges[idx].claimed = true;
                self.scrap = new_scrap;
                self.message = format!("+{} SCRAP CLAIMED!", earned);
                self.message_timer = 90;
                // clamp cursor
                let claimable_count = self.challenges.iter().filter(|c| !c.claimed && c.progress >= c.target).count();
                if self.cursor >= claimable_count && self.cursor > 0 { self.cursor -= 1; }
            }
            Err(_) => {
                self.message = "CLAIM FAILED".to_string();
                self.message_timer = 60;
            }
        }
    }

    fn draw(&self, buf: &mut WorldBuffer) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;

        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(8, 10, 22));
        buf.fill_rect(0, 0, SCREEN_W, 28, Bgra::new(18, 22, 48));

        let title = "MISSIONS";
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, sw/2 - tw/2, 6, Bgra::new(255, 210, 50), 2);

        // Scrap balance
        let bal = format!("SCRAP: {}", self.scrap);
        draw_str(buf, &bal, sw - str_width(&bal) as i32 - 6, 10, Bgra::new(255, 200, 50));

        if self.challenges.is_empty() {
            let msg = "LOADING...";
            let mw = str_width(msg) as i32;
            draw_str(buf, msg, sw/2 - mw/2, sh/2, Bgra::new(120, 120, 160));
            return;
        }

        let claimable: Vec<usize> = self.challenges.iter().enumerate()
            .filter(|(_, c)| !c.claimed && c.progress >= c.target)
            .map(|(i, _)| i)
            .collect();

        // Section headers + rows
        let mut y = 36i32;
        let row_h = 44i32;
        let mut claimable_cursor_idx = 0usize;

        for (section, label) in [("daily", "DAILY"), ("weekly", "WEEKLY")] {
            // Section header
            buf.fill_rect(0, y, SCREEN_W, 14, Bgra::new(14, 18, 40));
            let lw = str_width(label) as i32;
            draw_str(buf, label, 8, y + 3, Bgra::new(100, 120, 200));
            let _ = lw;
            y += 16;

            for ch in self.challenges.iter().filter(|c| c.period_type == section) {
                let is_done    = ch.progress >= ch.target;
                let is_claimed = ch.claimed;
                let ci = claimable.iter().position(|&i| self.challenges[i].id == ch.id);
                let is_selected = ci.map(|pos| pos == self.cursor).unwrap_or(false);

                // Row bg
                let bg = if is_selected { Bgra::new(24, 34, 72) } else { Bgra::new(12, 14, 32) };
                buf.fill_rect(0, y, SCREEN_W, row_h as u32, bg);
                if is_selected {
                    buf.fill_rect(0, y, 3, row_h as u32, Bgra::new(255, 180, 0));
                }

                // Description
                let desc_col = if is_claimed { Bgra::new(60, 70, 90) } else { Bgra::new(200, 210, 255) };
                draw_str(buf, &ch.desc, 10, y + 4, desc_col);

                // Scrap reward
                let reward_str = format!("+{}", ch.scrap);
                let rw = str_width(&reward_str) as i32;
                let reward_col = if is_claimed { Bgra::new(50, 60, 50) } else { Bgra::new(255, 210, 50) };
                draw_str(buf, &reward_str, sw - rw - 8, y + 4, reward_col);

                // Progress bar
                let bar_x = 10i32;
                let bar_w = (sw - 80) as u32;
                let bar_y = y + 18;
                let bar_h = 10u32;
                buf.fill_rect(bar_x, bar_y, bar_w, bar_h, Bgra::new(20, 22, 40));
                if ch.target > 0 {
                    let fill = ((ch.progress.min(ch.target) as u64 * bar_w as u64) / ch.target as u64) as u32;
                    if fill > 0 {
                        let bar_col = if is_claimed { Bgra::new(30, 70, 30) }
                                      else if is_done { Bgra::new(60, 200, 60) }
                                      else { Bgra::new(50, 100, 220) };
                        buf.fill_rect(bar_x, bar_y, fill, bar_h, bar_col);
                    }
                }
                // Border
                buf.fill_rect(bar_x, bar_y, bar_w, 1, Bgra::new(40, 44, 80));
                buf.fill_rect(bar_x, bar_y + bar_h as i32 - 1, bar_w, 1, Bgra::new(40, 44, 80));

                // Progress text
                let prog_str = if is_claimed {
                    "CLAIMED".to_string()
                } else if is_done {
                    "COMPLETE - PRESS A".to_string()
                } else {
                    format!("{}/{}", ch.progress, ch.target)
                };
                let pw = str_width(&prog_str) as i32;
                let prog_col = if is_claimed { Bgra::new(50, 80, 50) }
                               else if is_done { Bgra::new(100, 255, 100) }
                               else { Bgra::new(140, 160, 200) };
                draw_str(buf, &prog_str, sw - pw - 8, bar_y + 1, prog_col);

                y += row_h;
                if ci.is_some() { claimable_cursor_idx += 1; }
            }

            y += 4; // gap between sections
        }

        // Message overlay
        if self.message_timer > 0 {
            let mw = str_width_scaled(&self.message, 2);
            let mx = sw/2 - mw/2;
            let my = sh/2 - 10;
            buf.fill_rect(mx - 8, my - 6, (mw + 16) as u32, 28, Bgra::new(10, 30, 10));
            draw_str_scaled(buf, &self.message, mx, my, Bgra::new(80, 255, 80), 2);
        }

        // Hint bar
        buf.fill_rect(0, sh - 18, SCREEN_W, 18, Bgra::new(12, 14, 35));
        let hint = if claimable.is_empty() { "B=BACK" } else { "A=CLAIM  B=BACK" };
        draw_str(buf, hint, sw/2 - str_width(hint)/2, sh - 13, Bgra::new(60, 70, 110));
    }
}

fn parse_challenges(json: &str) -> Vec<Challenge> {
    let mut out = Vec::new();
    // Response is a JSON array: [{...},{...},...]
    let mut rest = json.trim();
    if !rest.starts_with('[') { return out; }
    rest = &rest[1..];
    // Split on top-level `},{`
    let mut depth = 0i32;
    let mut start = 0usize;
    let bytes = rest.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'{' => { if depth == 0 { start = i; } depth += 1; }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    let obj = &rest[start..=i];
                    if let Some(ch) = parse_challenge_obj(obj) { out.push(ch); }
                }
            }
            _ => {}
        }
    }
    out
}

fn parse_challenge_obj(obj: &str) -> Option<Challenge> {
    let id          = json_field(obj, "id")?;
    let desc        = json_field(obj, "desc")?;
    let target: u32 = json_field(obj, "target").and_then(|s| s.parse().ok()).unwrap_or(1);
    let scrap: u32  = json_field(obj, "scrap").and_then(|s| s.parse().ok()).unwrap_or(0);
    let period_type = json_field(obj, "period_type").unwrap_or_else(|| "daily".to_string());
    let progress: u32 = json_field(obj, "progress").and_then(|s| s.parse().ok()).unwrap_or(0);
    let claimed     = json_field(obj, "claimed").map(|s| s == "true").unwrap_or(false);
    Some(Challenge { id, desc, target, scrap, period_type, progress, claimed })
}
