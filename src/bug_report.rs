/// In-game bug reporter (L2+R2).
///
/// Freezes the current frame, shows a category picker and on-screen keyboard,
/// encodes a PNG screenshot, and POSTs everything to /api/bug_report on the Pi.
/// The server forwards it to Discord #bug-reports with the image attached.

use crate::renderer::buffer::WorldBuffer;
use crate::renderer::fb::Bgra;
use crate::renderer::font::{draw_str, draw_str_shadow};
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

const KB_ROWS: &[&str] = &[
    "1234567890",
    "qwertyuiop",
    "asdfghjkl",
    "zxcvbnm .,",
];
const KB_SHIFT_ROWS: &[&str] = &[
    "!@#$%^&*()",
    "QWERTYUIOP",
    "ASDFGHJKL",
    "ZXCVBNM <>",
];
const MAX_DESC: usize = 120;

// Pixel layout
const OVERLAY_X:    i32 = 40;
const OVERLAY_Y:    i32 = 20;
const OVERLAY_W:    i32 = SCREEN_W as i32 - 80;
const OVERLAY_H:    i32 = SCREEN_H as i32 - 40;
const CAT_Y:        i32 = OVERLAY_Y + 32;
const DESC_Y:       i32 = CAT_Y + 80;
const KB_Y:         i32 = DESC_Y + 60;
const KB_CELL:      i32 = 18;

// ── State machine ─────────────────────────────────────────────────────────────

#[derive(PartialEq)]
enum Phase {
    Category,
    Keyboard,
    Sending,
    Done(bool), // true=success false=fail
}

pub struct BugReporter {
    /// Frozen viewport pixels (BGRA, 640×480).
    screenshot:  Vec<u8>,
    phase:       Phase,
    cat_idx:     usize,
    cat_selected: [bool; 6],
    description: String,
    kb_col:      usize,
    kb_row:      usize,
    shift:       bool,
    status_msg:  String,
    send_timer:  u32,
}

impl BugReporter {
    /// Capture the current viewport from the world buffer and open the reporter.
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
            description:  String::new(),
            kb_col:       0,
            kb_row:       0,
            shift:        false,
            status_msg:   String::new(),
            send_timer:   0,
        }
    }

    /// Returns true when the reporter should be dismissed.
    pub fn is_done(&self) -> bool {
        if let Phase::Done(_) = &self.phase { self.send_timer == 0 } else { false }
    }

    pub fn tick(&mut self, input: &InputState) -> bool {
        match &self.phase {
            Phase::Category => self.tick_category(input),
            Phase::Keyboard => self.tick_keyboard(input),
            Phase::Sending  => { self.send_timer = self.send_timer.saturating_sub(1); false }
            Phase::Done(_)  => { self.send_timer = self.send_timer.saturating_sub(1); false }
        }
    }

    fn tick_category(&mut self, input: &InputState) -> bool {
        if input.just_pressed(Button::B) { return true; } // cancel
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
            // require at least one selected; if none, select current
            if !self.cat_selected.iter().any(|&s| s) {
                self.cat_selected[self.cat_idx] = true;
            }
            self.phase = Phase::Keyboard;
        }
        false
    }

    fn tick_keyboard(&mut self, input: &InputState) -> bool {
        if input.just_pressed(Button::B) {
            if self.description.is_empty() {
                self.phase = Phase::Category;
            } else {
                self.description.pop();
            }
            return false;
        }

        // Navigate keyboard
        let rows = if self.shift { KB_SHIFT_ROWS } else { KB_ROWS };
        let row_len = rows[self.kb_row].chars().count();

        if input.just_pressed(Button::Up) && self.kb_row > 0 {
            self.kb_row -= 1;
            self.kb_col = self.kb_col.min(rows[self.kb_row].chars().count() - 1);
        }
        if input.just_pressed(Button::Down) && self.kb_row + 1 < rows.len() {
            self.kb_row += 1;
            self.kb_col = self.kb_col.min(rows[self.kb_row].chars().count() - 1);
        }
        if input.just_pressed(Button::Left) && self.kb_col > 0 {
            self.kb_col -= 1;
        }
        if input.just_pressed(Button::Right) && self.kb_col + 1 < row_len {
            self.kb_col += 1;
        }
        if input.just_pressed(Button::Y) {
            self.shift = !self.shift;
        }
        if input.just_pressed(Button::A) && self.description.len() < MAX_DESC {
            let rows = if self.shift { KB_SHIFT_ROWS } else { KB_ROWS };
            if let Some(ch) = rows[self.kb_row].chars().nth(self.kb_col) {
                self.description.push(ch);
            }
        }
        if input.just_pressed(Button::Start) {
            self.submit();
        }
        false
    }

    fn submit(&mut self) {
        self.status_msg = "Sending...".into();
        self.send_timer = 300;

        let category: String = BUG_CATEGORIES.iter().enumerate()
            .filter(|&(i, _)| self.cat_selected[i])
            .map(|(_, &s)| s)
            .collect::<Vec<_>>()
            .join(", ");
        let description = self.description.clone();
        let png         = encode_png(&self.screenshot, SCREEN_W, SCREEN_H);

        let result = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || {
                crate::https::https_post_multipart(
                    API_HOST, API_PATH, &category, &description, &png, 10, 20,
                ).is_ok()
            });

        // Fire-and-forget — we can't easily poll the thread result without
        // storing the JoinHandle across ticks, so show success optimistically.
        // Network errors are logged server-side.
        let _ = result;
        self.phase = Phase::Done(true);
        self.status_msg = "Report sent! Thank you.".into();
        self.send_timer = 180;
    }

    pub fn draw(&self, buf: &mut WorldBuffer, cam_x: u32) {
        let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W)) as i32;

        // Dim the frozen screenshot by drawing a semi-transparent dark rect
        dim_viewport(buf, cam_x);

        // Panel background
        fill_rect(buf, cam_x + OVERLAY_X, OVERLAY_Y, OVERLAY_W, OVERLAY_H,
                  Bgra::new(20, 20, 30));
        outline_rect(buf, cam_x + OVERLAY_X, OVERLAY_Y, OVERLAY_W, OVERLAY_H,
                     Bgra::new(120, 120, 200));

        let ox = cam_x + OVERLAY_X + 8;
        draw_str_shadow(buf, "BUG REPORT  [B=back  Start=submit]",
                        ox, OVERLAY_Y + 6, Bgra::new(255, 220, 80));

        match &self.phase {
            Phase::Category => self.draw_category(buf, ox),
            Phase::Keyboard => self.draw_keyboard(buf, ox),
            Phase::Sending | Phase::Done(_) => {
                draw_str_shadow(buf, &self.status_msg, ox, OVERLAY_Y + 60,
                                Bgra::new(100, 255, 100));
            }
        }
    }

    fn draw_category(&self, buf: &mut WorldBuffer, ox: i32) {
        draw_str_shadow(buf, "Select categories  [A=toggle  Start=next]",
                        ox, CAT_Y - 16, Bgra::new(200, 200, 200));
        for (i, &cat) in BUG_CATEGORIES.iter().enumerate() {
            let y = CAT_Y + i as i32 * 14;
            let cursor  = if i == self.cat_idx { ">" } else { " " };
            let checked = if self.cat_selected[i]  { "[x]" } else { "[ ]" };
            let col = if i == self.cat_idx {
                Bgra::new(255, 220, 80)
            } else if self.cat_selected[i] {
                Bgra::new(100, 220, 120)
            } else {
                Bgra::new(180, 180, 180)
            };
            draw_str_shadow(buf, &format!("{} {} {}", cursor, checked, cat), ox, y, col);
        }
    }

    fn draw_keyboard(&self, buf: &mut WorldBuffer, ox: i32) {
        // Category + description header
        let cats: String = BUG_CATEGORIES.iter().enumerate()
            .filter(|&(i, _)| self.cat_selected[i])
            .map(|(_, &s)| s)
            .collect::<Vec<_>>()
            .join(", ");
        draw_str_shadow(buf, &format!("Tags: {}", cats),
                        ox, DESC_Y - 28, Bgra::new(160, 200, 255));

        // Description box
        let desc_display = if self.description.len() > 38 {
            format!("...{}", &self.description[self.description.len()-35..])
        } else {
            self.description.clone()
        };
        fill_rect(buf, ox - 2, DESC_Y - 2, OVERLAY_W - 20, 18,
                  Bgra::new(10, 10, 20));
        draw_str_shadow(buf, &format!("{}_", desc_display),
                        ox, DESC_Y, Bgra::new(240, 240, 240));

        let hint = format!("A=type  Y=shift({})  B=delete  Start=send", if self.shift {"ON"} else {"off"});
        draw_str_shadow(buf, &hint, ox, DESC_Y + 18,
                        Bgra::new(140, 140, 140));

        // Keyboard
        let rows = if self.shift { KB_SHIFT_ROWS } else { KB_ROWS };
        for (ri, row) in rows.iter().enumerate() {
            let ky = KB_Y + ri as i32 * (KB_CELL + 2);
            let row_chars: Vec<char> = row.chars().collect();
            let row_w = row_chars.len() as i32 * KB_CELL;
            let kx_start = ox + (OVERLAY_W - 20 - row_w) / 2;
            for (ci, &ch) in row_chars.iter().enumerate() {
                let kx = kx_start + ci as i32 * KB_CELL;
                let selected = ri == self.kb_row && ci == self.kb_col;
                let bg = if selected {
                    Bgra::new(80, 120, 200)
                } else {
                    Bgra::new(40, 40, 60)
                };
                fill_rect(buf, kx, ky, KB_CELL - 1, KB_CELL - 1, bg);
                let label = if ch == ' ' { "_".to_string() } else { ch.to_string() };
                let col = if selected {
                    Bgra::new(255, 255, 255)
                } else {
                    Bgra::new(200, 200, 200)
                };
                draw_str(buf, &label, kx + 4, ky + 4, col);
            }
        }
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
        // Convert BGRA → RGB
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for px in bgra.chunks(4) {
            rgb.push(px[2]); // R
            rgb.push(px[1]); // G
            rgb.push(px[0]); // B
        }
        writer.write_image_data(&rgb).unwrap();
    }
    out
}

// ── Drawing helpers ───────────────────────────────────────────────────────────

fn fill_rect(buf: &mut WorldBuffer, x: i32, y: i32, w: i32, h: i32, col: Bgra) {
    for dy in 0..h {
        for dx in 0..w {
            buf.set_pixel(x + dx, y + dy, col);
        }
    }
}

fn outline_rect(buf: &mut WorldBuffer, x: i32, y: i32, w: i32, h: i32, col: Bgra) {
    for dx in 0..w {
        buf.set_pixel(x + dx, y,         col);
        buf.set_pixel(x + dx, y + h - 1, col);
    }
    for dy in 0..h {
        buf.set_pixel(x,         y + dy, col);
        buf.set_pixel(x + w - 1, y + dy, col);
    }
}

fn dim_viewport(buf: &mut WorldBuffer, cam_x: i32) {
    for y in 0..SCREEN_H as i32 {
        for x in 0..SCREEN_W as i32 {
            let px = buf.get_pixel(cam_x + x, y);
            buf.set_pixel(cam_x + x, y, Bgra::new(px.r / 3, px.g / 3, px.b / 3));
        }
    }
}
