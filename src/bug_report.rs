/// In-game bug reporter (L2+R2).
///
/// Freezes the current frame, shows a category picker and on-screen keyboard,
/// encodes a PNG screenshot, and POSTs everything to /api/bug_report on the Pi.
/// The server forwards it to Discord #bug-reports with the image attached.

use crate::renderer::buffer::WorldBuffer;
use crate::renderer::fb::Bgra;
use crate::renderer::font::{draw_str, draw_str_shadow, draw_str_scaled, draw_str_shadow_scaled, str_width, str_width_scaled};
use crate::renderer::keyboard::Keyboard;
use crate::renderer::hud::COLOR_DARK_BG;
use crate::world::{SCREEN_W, SCREEN_H, WORLD_W};
use crate::input::buttons::Button;
use crate::input::state::InputState;

// ── Constants ─────────────────────────────────────────────────────────────────

const API_HOST: &str = "crumbonium.duckdns.org";
const API_PATH: &str = "/api/bug_report";

const BUG_CATEGORIES: &[&str] = &[
    "Crash / Freeze",
    "Visual Glitch",
    "Gameplay Bug",
    "Network Issue",
    "Audio Issue",
    "Other",
];

const MAX_DESC: usize = 120;

const HEADER_H: i32 = 36;
fn dim_line() -> Bgra { Bgra::new(40, 44, 70) }

// ── State machine ─────────────────────────────────────────────────────────────

enum Phase {
    Category,
    Keyboard,
    Sending(std::thread::JoinHandle<bool>),
    Done(bool),
}

impl PartialEq for Phase {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other),
            (Phase::Category, Phase::Category) |
            (Phase::Keyboard, Phase::Keyboard) |
            (Phase::Sending(_), Phase::Sending(_)) |
            (Phase::Done(_), Phase::Done(_))
        )
    }
}

pub struct BugReporter {
    /// Frozen viewport pixels (BGRA, 640×480).
    screenshot:   Vec<u8>,
    phase:        Phase,
    cat_idx:      usize,
    cat_selected: [bool; 6],
    keyboard:     Keyboard,
    status_msg:   String,
    send_timer:   u32,
}

impl BugReporter {
    pub fn capture(world: &WorldBuffer, cam_x: u32) -> Self {
        let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
        let mut pixels = Vec::with_capacity((SCREEN_W * SCREEN_H * 4) as usize);
        for y in 0..SCREEN_H {
            let src = (y * WORLD_W + cam_x) as usize * 4;
            pixels.extend_from_slice(&world.raw_data()[src..src + SCREEN_W as usize * 4]);
        }
        Self {
            screenshot:   pixels,
            phase:        Phase::Category,
            cat_idx:      0,
            cat_selected: [false; 6],
            keyboard:     Keyboard::new(MAX_DESC),
            status_msg:   String::new(),
            send_timer:   0,
        }
    }

    pub fn is_done(&self) -> bool {
        if let Phase::Done(_) = &self.phase { self.send_timer == 0 } else { false }
    }

    pub fn tick(&mut self, input: &InputState) -> bool {
        match &self.phase {
            Phase::Category    => self.tick_category(input),
            Phase::Keyboard    => self.tick_keyboard(input),
            Phase::Sending(_)  => self.tick_sending(),
            Phase::Done(_)     => { self.send_timer = self.send_timer.saturating_sub(1); false }
        }
    }

    fn tick_sending(&mut self) -> bool {
        let done = if let Phase::Sending(handle) = &self.phase {
            handle.is_finished()
        } else { false };
        if done {
            let ok = if let Phase::Sending(handle) = std::mem::replace(&mut self.phase, Phase::Done(false)) {
                handle.join().unwrap_or(false)
            } else { false };
            self.phase = Phase::Done(ok);
            self.status_msg = if ok { "Report sent! Thank you.".into() } else { "Send failed. Try again.".into() };
            self.send_timer = 180;
        }
        false
    }

    fn tick_category(&mut self, input: &InputState) -> bool {
        if input.just_pressed(Button::B) { return true; }
        if input.just_pressed(Button::Up) && self.cat_idx > 0 {
            self.cat_idx -= 1;
        }
        if input.just_pressed(Button::Down) && self.cat_idx + 1 < BUG_CATEGORIES.len() {
            self.cat_idx += 1;
        }
        if input.just_pressed(Button::A) {
            self.cat_selected[self.cat_idx] = !self.cat_selected[self.cat_idx];
        }
        if input.just_pressed(Button::Start) {
            if !self.cat_selected.iter().any(|&s| s) {
                self.cat_selected[self.cat_idx] = true;
            }
            self.phase = Phase::Keyboard;
        }
        false
    }

    fn tick_keyboard(&mut self, input: &InputState) -> bool {
        if input.just_pressed(Button::B) && self.keyboard.text.is_empty() {
            self.phase = Phase::Category;
            return false;
        }
        if self.keyboard.update(input) {
            self.submit();
        }
        false
    }

    fn submit(&mut self) {
        self.status_msg = "Sending...".into();

        let category: String = BUG_CATEGORIES.iter().enumerate()
            .filter(|&(i, _)| self.cat_selected[i])
            .map(|(_, &s)| s)
            .collect::<Vec<_>>()
            .join(", ");
        let description = self.keyboard.text.clone();
        let png = encode_png(&self.screenshot, SCREEN_W, SCREEN_H);

        let handle = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                crate::https::https_post_multipart(
                    API_HOST, API_PATH, &category, &description, &png, 10, 20,
                ).is_ok()
            });
        match handle {
            Ok(h) => { self.phase = Phase::Sending(h); }
            Err(_) => {
                self.phase = Phase::Done(false);
                self.status_msg = "Send failed. Try again.".into();
                self.send_timer = 180;
            }
        }
    }

    pub fn draw(&self, buf: &mut WorldBuffer, cam_x: u32) {
        let cam_xi = cam_x.min(WORLD_W.saturating_sub(SCREEN_W)) as i32;
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;

        // Frozen screenshot dimmed behind UI
        for y in 0..sh {
            for x in 0..sw {
                let px = buf.get_pixel(cam_xi + x, y);
                buf.set_pixel(cam_xi + x, y, Bgra::new(px.r / 4, px.g / 4, px.b / 4));
            }
        }

        // Header bar
        buf.fill_rect(cam_xi, 0, SCREEN_W, HEADER_H as u32, Bgra::new(18, 22, 48));
        buf.fill_rect(cam_xi, HEADER_H, SCREEN_W, 1, dim_line());

        match &self.phase {
            Phase::Category => self.draw_category(buf, cam_xi, sw, sh),
            Phase::Keyboard => self.draw_keyboard(buf, cam_xi, sw, sh),
            Phase::Sending(_) | Phase::Done(_) => self.draw_status(buf, cam_xi, sw, sh),
        }
    }

    fn draw_header(&self, buf: &mut WorldBuffer, cam_x: i32, sw: i32, title: &str) {
        let tw = str_width_scaled(title, 2);
        draw_str_shadow_scaled(buf, title, cam_x + sw/2 - tw/2, 9, Bgra::new(255, 210, 50), 2);
    }

    fn draw_hint_bar(&self, buf: &mut WorldBuffer, cam_x: i32, sw: i32, sh: i32, hints: &[(&str, &str)]) {
        buf.fill_rect(cam_x, sh - 20, SCREEN_W, 20, Bgra::new(12, 14, 35));
        buf.fill_rect(cam_x, sh - 21, SCREEN_W, 1, dim_line());
        let mut x = cam_x + 8;
        for (btn, label) in hints {
            let btn_w = str_width(btn);
            let lbl_w = str_width(label);
            draw_str(buf, btn,   x, sh - 14, Bgra::new(255, 210, 50));
            x += btn_w + 2;
            draw_str(buf, label, x, sh - 14, Bgra::new(140, 144, 180));
            x += lbl_w + 14;
        }
    }

    fn draw_category(&self, buf: &mut WorldBuffer, cam_x: i32, sw: i32, sh: i32) {
        self.draw_header(buf, cam_x, sw, "BUG REPORT");
        self.draw_hint_bar(buf, cam_x, sw, sh, &[
            ("A", "toggle"), ("Up/Dn", "move"), ("Start", "next"), ("B", "cancel"),
        ]);

        let row_h   = 32i32;
        let total_h = BUG_CATEGORIES.len() as i32 * row_h;
        let list_y  = (sh - total_h) / 2;

        for (i, &cat) in BUG_CATEGORIES.iter().enumerate() {
            let y       = list_y + i as i32 * row_h;
            let cursor  = i == self.cat_idx;
            let checked = self.cat_selected[i];

            // Full-width highlight row for selected cursor
            if cursor {
                buf.fill_rect(cam_x, y, SCREEN_W, row_h as u32, Bgra::new(22, 26, 55));
                buf.fill_rect(cam_x, y, 3, row_h as u32, Bgra::new(255, 210, 50));
            }

            let check_col = if checked { Bgra::new(80, 220, 100) } else { Bgra::new(60, 64, 90) };
            let text_col  = if cursor  { Bgra::new(255, 220, 80) }
                            else if checked { Bgra::new(160, 220, 160) }
                            else { Bgra::new(160, 164, 200) };

            let checkbox = if checked { "[x]" } else { "[ ]" };
            draw_str_shadow(buf, checkbox, cam_x + 12, y + 10, check_col);
            draw_str_shadow(buf, cat,     cam_x + 42, y + 10, text_col);
        }
    }

    fn draw_keyboard(&self, buf: &mut WorldBuffer, cam_x: i32, sw: i32, sh: i32) {
        self.draw_header(buf, cam_x, sw, "DESCRIBE THE BUG");

        // Tags line below header
        let cats: String = BUG_CATEGORIES.iter().enumerate()
            .filter(|&(i, _)| self.cat_selected[i])
            .map(|(_, &s)| s)
            .collect::<Vec<_>>()
            .join(", ");
        let tag_text = format!("Tags: {}", cats);
        let tw = str_width(&tag_text);
        draw_str(buf, &tag_text, cam_x + sw/2 - tw/2, HEADER_H + 6, Bgra::new(100, 160, 220));

        self.keyboard.draw(buf, cam_x);
    }

    fn draw_status(&self, buf: &mut WorldBuffer, cam_x: i32, sw: i32, sh: i32) {
        self.draw_header(buf, cam_x, sw, "BUG REPORT");
        let col = if matches!(self.phase, Phase::Done(true)) {
            Bgra::new(80, 220, 100)
        } else {
            Bgra::new(200, 200, 80)
        };
        let mw = str_width_scaled(&self.status_msg, 2);
        draw_str_shadow_scaled(buf, &self.status_msg, cam_x + sw/2 - mw/2, sh/2 - 8, col, 2);
    }
}

// ── PNG encoder ───────────────────────────────────────────────────────────────

fn encode_png(bgra: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for px in bgra.chunks(4) {
            rgb.push(px[2]);
            rgb.push(px[1]);
            rgb.push(px[0]);
        }
        writer.write_image_data(&rgb).unwrap();
    }
    out
}
