use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str_shadow_scaled, str_width_scaled, draw_str, str_width};
use crate::renderer::hud::{COLOR_DARK_BG, COLOR_PANEL_BG, COLOR_BORDER, draw_button_hints, draw_menu_selection};
use crate::world::{SCREEN_W, SCREEN_H};

const SETTINGS_PATH: &str = "/mnt/SDCARD/.arty_settings";

const CONTROLS_PAGES: &[(&str, &[&str])] = &[
    ("CONTROLS", &[
        "D-PAD LEFT/RIGHT   Move",
        "D-PAD UP/DOWN      Aim angle",
        "HOLD A + RELEASE   Charge and fire",
        "B                  Jump forward",
        "Y                  Backflip",
        "SELECT             Weapon menu",
        "START              Pause / menu",
        "",
        "R1 + D-PAD         Pan camera (snaps back)",
        "L1 + D-PAD         Pan camera (stays put)",
        "",
        "WEAPON MENU",
        "  D-PAD  Browse    A  Confirm    B  Cancel",
        "  L1 / R1  Adjust grenade fuse",
    ]),
    ("SPECIAL CONTROLS", &[
        "GRAPPLE HOOK",
        "  A          Fire / Release / Re-rope",
        "  UP / DOWN  Shorten / Lengthen rope",
        "  LEFT / RIGHT  Swing while attached",
        "",
        "PLASMA TORCH",
        "  HOLD A       Burn  (release to stop)",
        "  UP / DOWN    Aim angle",
        "",
        "AIR STRIKE",
        "  UP / DOWN    Move cursor",
        "  A            Call strike",
        "",
        "REVOLVER / SHOTGUN",
        "  A            Fire (up to 6 / 2 shots)",
    ]),
];

#[derive(Clone)]
pub struct Settings {
    pub sound_on: bool,
}

impl Default for Settings {
    fn default() -> Self { Self { sound_on: true } }
}

impl Settings {
    pub fn load() -> Self {
        let mut s = Self::default();
        if let Ok(data) = std::fs::read_to_string(SETTINGS_PATH) {
            for line in data.lines() {
                let mut parts = line.splitn(2, '=');
                match (parts.next(), parts.next()) {
                    (Some("sound"), Some("0")) => s.sound_on = false,
                    (Some("sound"), Some("1")) => s.sound_on = true,
                    _ => {}
                }
            }
        }
        s
    }

    pub fn save(&self) {
        let data = format!("sound={}\n", if self.sound_on { 1 } else { 0 });
        let _ = std::fs::write(SETTINGS_PATH, data);
    }
}

pub enum SettingsAction { Back }

pub struct SettingsScreen {
    cursor: usize,
    pub settings: Settings,
    // Controls sub-screen: None = main settings, Some(page) = controls page
    controls_page: Option<usize>,
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self { cursor: 0, settings: Settings::load(), controls_page: None }
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer) -> Option<SettingsAction> {
        if let Some(page) = self.controls_page {
            // Controls viewer
            if input.just_pressed(Button::B) || input.just_pressed(Button::Start) {
                self.controls_page = None;
            } else if input.just_pressed(Button::Right) || input.just_pressed(Button::A) {
                if page + 1 < CONTROLS_PAGES.len() {
                    self.controls_page = Some(page + 1);
                }
            } else if input.just_pressed(Button::Left) {
                if page > 0 { self.controls_page = Some(page - 1); }
            }
            self.draw_controls(buf, page);
            return None;
        }

        const N: usize = 2; // SOUND toggle + CONTROLS entry
        if input.just_pressed(Button::B) { self.settings.save(); return Some(SettingsAction::Back); }
        if input.just_pressed(Button::Up)   { self.cursor = (self.cursor + N - 1) % N; }
        if input.just_pressed(Button::Down) { self.cursor = (self.cursor + 1) % N; }
        if input.just_pressed(Button::A) || input.just_pressed(Button::Left) || input.just_pressed(Button::Right) {
            match self.cursor {
                0 => {
                    self.settings.sound_on = !self.settings.sound_on;
                    crate::audio::set_muted(!self.settings.sound_on);
                }
                1 => { self.controls_page = Some(0); }
                _ => {}
            }
        }
        self.draw(buf);
        None
    }

    fn draw(&self, buf: &mut WorldBuffer) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;

        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H as u32, COLOR_DARK_BG);

        let panel_w = 300i32;
        let panel_x = (sw - panel_w) / 2;
        let panel_y = 60i32;
        buf.fill_rect(panel_x, panel_y, panel_w as u32, 2, COLOR_BORDER);

        let title = "SETTINGS";
        let tw = str_width_scaled(title, 2);
        draw_str_shadow_scaled(buf, title, sw/2 - tw/2, panel_y - 22, Bgra::new(200, 200, 230), 2);

        let item_h = 36i32;
        let start_y = panel_y + 14;

        // Sound toggle
        {
            let iy = start_y;
            let selected = self.cursor == 0;
            if selected { draw_menu_selection(buf, panel_x, iy - 4, panel_w, item_h); }
            let col = if selected { Bgra::new(255, 225, 55) } else { Bgra::new(200, 200, 220) };
            draw_str_shadow_scaled(buf, "SOUND", panel_x + 30, iy + 4, col, 2);
            let val = if self.settings.sound_on { "ON" } else { "OFF" };
            let val_col = if self.settings.sound_on { Bgra::new(80, 220, 120) } else { Bgra::new(180, 80, 80) };
            let vw = str_width_scaled(val, 2);
            draw_str_shadow_scaled(buf, val, panel_x + panel_w - vw - 16, iy + 4, val_col, 2);
        }

        buf.fill_rect(panel_x, start_y + item_h, panel_w as u32, 1, COLOR_BORDER);

        // Controls entry
        {
            let iy = start_y + item_h + 8;
            let selected = self.cursor == 1;
            if selected { draw_menu_selection(buf, panel_x, iy - 4, panel_w, item_h); }
            let col = if selected { Bgra::new(255, 225, 55) } else { Bgra::new(200, 200, 220) };
            draw_str_shadow_scaled(buf, "CONTROLS", panel_x + 30, iy + 4, col, 2);
            let arrow = ">";
            let aw = str_width_scaled(arrow, 2);
            draw_str_shadow_scaled(buf, arrow, panel_x + panel_w - aw - 16, iy + 4, Bgra::new(140, 140, 170), 2);
        }

        buf.fill_rect(panel_x, start_y + item_h * 2 + 8, panel_w as u32, 2, COLOR_BORDER);

        draw_button_hints(buf, &[("A", "SELECT"), ("B", "BACK")], 0);
        let saved_str = "SAVES ON EXIT";
        let saved_w = str_width(saved_str);
        draw_str(buf, saved_str, sw/2 - saved_w/2, sh - 26, Bgra::new(70, 70, 100));
    }

    fn draw_controls(&self, buf: &mut WorldBuffer, page: usize) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        let (title, lines) = CONTROLS_PAGES[page];

        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H as u32, COLOR_DARK_BG);

        // Title bar
        buf.fill_rect(0, 0, SCREEN_W, 28, COLOR_PANEL_BG);
        buf.fill_rect(0, 28, SCREEN_W, 2, COLOR_BORDER);
        let tw = str_width_scaled(title, 2);
        draw_str_shadow_scaled(buf, title, sw/2 - tw/2, 6, Bgra::new(200, 200, 230), 2);

        // Page indicator
        let total = CONTROLS_PAGES.len();
        let indicator = format!("{}/{}", page + 1, total);
        let iw = str_width(&indicator);
        draw_str(buf, &indicator, sw - iw as i32 - 8, 10, Bgra::new(100, 100, 140));

        // Lines
        let line_h = 16i32;
        let mut y = 40i32;
        for line in lines.iter() {
            if line.is_empty() { y += 6; continue; }
            let col = if line.starts_with("  ") {
                Bgra::new(160, 200, 255)
            } else {
                Bgra::new(220, 220, 240)
            };
            draw_str(buf, line, 12, y, col);
            y += line_h;
            if y > sh - 30 { break; }
        }

        let hints = if page + 1 < total {
            &[("LEFT", "BACK"), ("RIGHT", "NEXT"), ("B", "CLOSE")][..]
        } else {
            &[("LEFT", "PREV"), ("B", "CLOSE")][..]
        };
        draw_button_hints(buf, hints, 0);
    }
}
