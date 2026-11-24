use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str, draw_str_shadow, str_width};
use crate::world::{SCREEN_W, SCREEN_H};

const ROWS: &[&str] = &[
    "QWERTYUIOP",
    "ASDFGHJKL.",
    "ZXCVBNM123",
    "0456789_-'",
];
// Row index ROWS.len() is the spacebar row

const KEY_W: i32 = 58;
const KEY_H: i32 = 36;
const KEY_GAP: i32 = 2;

pub struct Keyboard {
    pub text:    String,
    pub max_len: usize,
    cursor_col:  usize,
    cursor_row:  usize,
    caps:        bool,
}

impl Keyboard {
    pub fn new(max_len: usize) -> Self {
        Self { text: String::new(), max_len, cursor_col: 0, cursor_row: 0, caps: true }
    }

    pub fn with_text(text: &str, max_len: usize) -> Self {
        Self { text: text.to_string(), max_len, cursor_col: 0, cursor_row: 0, caps: true }
    }

    fn on_spacebar(&self) -> bool { self.cursor_row == ROWS.len() }

    pub fn update(&mut self, input: &InputState) -> bool {
        if input.just_pressed(Button::Up) && self.cursor_row > 0 { self.cursor_row -= 1; }
        if input.just_pressed(Button::Down) && self.cursor_row < ROWS.len() { self.cursor_row += 1; }
        if !self.on_spacebar() {
            if input.just_pressed(Button::Left)  && self.cursor_col > 0 { self.cursor_col -= 1; }
            if input.just_pressed(Button::Right) && self.cursor_col < 9 { self.cursor_col += 1; }
        }
        if input.just_pressed(Button::L1) { self.caps = !self.caps; }
        if input.just_pressed(Button::B) { self.text.pop(); }
        if input.just_pressed(Button::A) && self.text.len() < self.max_len {
            let ch = if self.on_spacebar() {
                ' '
            } else {
                let c = ROWS[self.cursor_row].chars().nth(self.cursor_col).unwrap_or(' ');
                if self.caps { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() }
            };
            self.text.push(ch);
        }
        if input.just_pressed(Button::Start) { return true; }
        false
    }

    /// The keyboard as a full screen in the menu style: frame (title, hints), `prompt` above the text field.
    pub fn draw_screen(&self, buf: &mut WorldBuffer, title: &str, prompt: &str) {
        use crate::renderer::hud::{side_frame, SIDE_WHITE};
        use crate::renderer::font::{draw_str_shadow_scaled, str_width_scaled};
        side_frame(buf, title, "", &[("A", "TYPE"), ("B", "DELETE"), ("L1", "CAPS"), ("START", "DONE")]);
        if !prompt.is_empty() {
            let sw = SCREEN_W as i32;
            let ps = if str_width_scaled(prompt, 2) <= sw - 40 { 2 } else { 1 };
            draw_str_shadow_scaled(buf, prompt, sw / 2 - str_width_scaled(prompt, ps) / 2, 70, SIDE_WHITE, ps);
        }
        self.draw(buf, 0, 0);
    }

    /// Draw just the keyboard (text field + keys), in the menu style. Callers draw the frame (`draw_screen`).
    pub fn draw(&self, buf: &mut WorldBuffer, cam_x: i32, cam_y: i32) {
        use crate::renderer::hud::{side_shade, side_panel, SIDE_GOLD, SIDE_ACCENT, SIDE_DIM};
        use crate::renderer::font::{draw_str_shadow_scaled, str_width_scaled};
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;

        // Total keyboard width based on longest row (QWERTYUIOP = 10 keys)
        let total_w = 10 * (KEY_W + KEY_GAP) - KEY_GAP;
        let kb_x = cam_x + sw / 2 - total_w / 2;
        // Above the menu footer (24 px) with room for the SPACE label.
        let kb_y = cam_y + sh - (ROWS.len() as i32 * (KEY_H + KEY_GAP)) - KEY_H - 46;

        // Text field: glass panel, text big
        let field_y = kb_y - 52;
        side_panel(buf, kb_x, field_y, total_w, 40);
        let display = format!("{}_", self.text);
        draw_str_shadow_scaled(buf, &display, kb_x + 12, field_y + 13, SIDE_GOLD, 2);
        let caps = if self.caps { "CAPS ON" } else { "caps off" };
        let caps_col = if self.caps { SIDE_GOLD } else { SIDE_DIM };
        draw_str(buf, caps, kb_x + total_w - str_width(caps) - 10, field_y + 17, caps_col);

        let key = |buf: &mut WorldBuffer, x: i32, y: i32, w: i32, selected: bool| {
            if selected {
                side_shade(buf, x, y, w, KEY_H, 60, (20, 30, 90));
                buf.fill_rect(x, y, w as u32, 2, SIDE_ACCENT);
                buf.fill_rect(x, y + KEY_H - 2, w as u32, 2, SIDE_ACCENT);
                buf.fill_rect(x, y, 2, KEY_H as u32, SIDE_ACCENT);
                buf.fill_rect(x + w - 2, y, 2, KEY_H as u32, SIDE_ACCENT);
            } else {
                side_shade(buf, x, y, w, KEY_H, 110, (8, 10, 30));
            }
        };

        // Character keys
        for (ri, row) in ROWS.iter().enumerate() {
            let row_w = row.len() as i32 * (KEY_W + KEY_GAP) - KEY_GAP;
            let row_x = cam_x + sw / 2 - row_w / 2;
            let row_y = kb_y + ri as i32 * (KEY_H + KEY_GAP);
            for (ci, ch) in row.chars().enumerate() {
                let x = row_x + ci as i32 * (KEY_W + KEY_GAP);
                let selected = ri == self.cursor_row && ci == self.cursor_col;
                key(buf, x, row_y, KEY_W, selected);
                let display = if self.caps { ch.to_ascii_uppercase() } else { ch.to_ascii_lowercase() };
                let s = display.to_string();
                let fg = if selected { SIDE_GOLD } else { Bgra::new(225, 228, 245) };
                draw_str_shadow_scaled(buf, &s, x + KEY_W/2 - str_width_scaled(&s, 2)/2, row_y + KEY_H/2 - 8, fg, 2);
            }
        }

        // Spacebar — wide key below the character rows, label inside
        let space_y = kb_y + ROWS.len() as i32 * (KEY_H + KEY_GAP);
        let space_selected = self.on_spacebar();
        key(buf, kb_x, space_y, total_w, space_selected);
        let spc = "SPACE";
        let fg = if space_selected { SIDE_GOLD } else { Bgra::new(225, 228, 245) };
        draw_str_shadow_scaled(buf, spc, kb_x + total_w/2 - str_width_scaled(spc, 2)/2, space_y + KEY_H/2 - 8, fg, 2);
        let _ = draw_str_shadow;
    }
}
