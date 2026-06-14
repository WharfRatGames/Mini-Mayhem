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

    pub fn draw(&self, buf: &mut WorldBuffer, cam_x: i32) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;

        // Total keyboard width based on longest row (QWERTYUIOP = 10 keys)
        let total_w = 10 * (KEY_W + KEY_GAP) - KEY_GAP;
        let kb_x = cam_x + sw / 2 - total_w / 2;
        let kb_y = sh - (ROWS.len() as i32 * (KEY_H + KEY_GAP)) - 44; // extra room for spacebar label

        // Text field at top
        let field_y = kb_y - 28;
        buf.fill_rect(kb_x, field_y - 4, total_w as u32, 20, Bgra::new(20, 20, 40));
        let display = format!("{}_", self.text);
        draw_str(buf, &display, kb_x + 4, field_y, Bgra::new(255, 220, 0));

        // Caps indicator
        let caps_col = if self.caps { Bgra::new(80, 180, 255) } else { Bgra::new(80, 80, 100) };
        draw_str(buf, "L=CAPS", kb_x + total_w - str_width("L=CAPS"), field_y, caps_col);

        // Character keys
        for (ri, row) in ROWS.iter().enumerate() {
            let row_w = row.len() as i32 * (KEY_W + KEY_GAP) - KEY_GAP;
            let row_x = cam_x + sw / 2 - row_w / 2;
            let row_y = kb_y + ri as i32 * (KEY_H + KEY_GAP);
            for (ci, ch) in row.chars().enumerate() {
                let x = row_x + ci as i32 * (KEY_W + KEY_GAP);
                let y = row_y;
                let selected = ri == self.cursor_row && ci == self.cursor_col;
                let bg = if selected { Bgra::new(60, 60, 120) } else { Bgra::new(25, 25, 45) };
                let fg = if selected { Bgra::new(255, 220, 0) } else { Bgra::new(200, 200, 200) };
                buf.fill_rect(x, y, KEY_W as u32, KEY_H as u32, bg);
                if selected {
                    for bx in x..x+KEY_W { buf.set_pixel(bx, y, Bgra::new(100, 100, 200)); buf.set_pixel(bx, y+KEY_H-1, Bgra::new(100, 100, 200)); }
                    for by in y..y+KEY_H { buf.set_pixel(x, by, Bgra::new(100, 100, 200)); buf.set_pixel(x+KEY_W-1, by, Bgra::new(100, 100, 200)); }
                }
                let display = if self.caps { ch.to_ascii_uppercase() } else { ch.to_ascii_lowercase() };
                let s = display.to_string();
                let tx = x + KEY_W/2 - str_width(&s)/2;
                let ty = y + KEY_H/2 - 4;
                draw_str(buf, &s, tx, ty, fg);
            }
        }

        // Spacebar — wide key below the character rows
        let space_y = kb_y + ROWS.len() as i32 * (KEY_H + KEY_GAP);
        let space_w = total_w;
        let space_selected = self.on_spacebar();
        let space_bg = if space_selected { Bgra::new(60, 60, 120) } else { Bgra::new(25, 25, 45) };
        let space_fg = if space_selected { Bgra::new(255, 220, 0) } else { Bgra::new(200, 200, 200) };
        buf.fill_rect(kb_x, space_y, space_w as u32, KEY_H as u32, space_bg);
        if space_selected {
            for bx in kb_x..kb_x+space_w { buf.set_pixel(bx, space_y, Bgra::new(100, 100, 200)); buf.set_pixel(bx, space_y+KEY_H-1, Bgra::new(100, 100, 200)); }
            for by in space_y..space_y+KEY_H { buf.set_pixel(kb_x, by, Bgra::new(100, 100, 200)); buf.set_pixel(kb_x+space_w-1, by, Bgra::new(100, 100, 200)); }
        }
        // "SPACE" label below the key
        let spc = "SPACE";
        draw_str(buf, spc, kb_x + space_w/2 - str_width(spc)/2, space_y + KEY_H + 3, space_fg);

        // Hints
        draw_str(buf, "A=type  B=del  Start=confirm", cam_x + sw/2 - str_width("A=type  B=del  Start=confirm")/2, sh - 6, Bgra::new(60, 60, 90));
    }
}
