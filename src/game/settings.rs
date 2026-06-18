use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str_shadow_scaled, str_width_scaled, draw_str, str_width};
use crate::renderer::hud::{COLOR_DARK_BG, COLOR_PANEL_BG, COLOR_BORDER, draw_button_hints, draw_menu_selection};
use crate::world::{SCREEN_W, SCREEN_H};

const SETTINGS_PATH: &str = "/mnt/SDCARD/.arty_settings";

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
}

impl SettingsScreen {
    pub fn new() -> Self {
        Self { cursor: 0, settings: Settings::load() }
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer) -> Option<SettingsAction> {
        const N: usize = 1; // number of toggle items
        if input.just_pressed(Button::B) { self.settings.save(); return Some(SettingsAction::Back); }
        if input.just_pressed(Button::Up)   { self.cursor = if self.cursor == 0 { N - 1 } else { self.cursor - 1 }; }
        if input.just_pressed(Button::Down) { self.cursor = (self.cursor + 1) % N; }
        if input.just_pressed(Button::A) || input.just_pressed(Button::Left) || input.just_pressed(Button::Right) {
            match self.cursor {
                0 => {
                    self.settings.sound_on = !self.settings.sound_on;
                    crate::audio::set_muted(!self.settings.sound_on);
                }
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

        // Header bar
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
            if selected {
                draw_menu_selection(buf, panel_x, iy - 4, panel_w, item_h);
            }
            let col = if selected { Bgra::new(255, 225, 55) } else { Bgra::new(200, 200, 220) };
            let label = "SOUND";
            let lw = str_width_scaled(label, 2);
            draw_str_shadow_scaled(buf, label, panel_x + 30, iy + 4, col, 2);
            let val = if self.settings.sound_on { "ON" } else { "OFF" };
            let val_col = if self.settings.sound_on { Bgra::new(80, 220, 120) } else { Bgra::new(180, 80, 80) };
            let vw = str_width_scaled(val, 2);
            draw_str_shadow_scaled(buf, val, panel_x + panel_w - vw - 16, iy + 4, val_col, 2);
            let _ = lw; // suppress unused warning
        }

        buf.fill_rect(panel_x, start_y + item_h, panel_w as u32, 2, COLOR_BORDER);

        // Hint
        draw_button_hints(buf, &[("A", "TOGGLE"), ("B", "BACK")], 0);
        let saved_str = "SAVES ON EXIT";
        let saved_w = str_width(saved_str);
        draw_str(buf, saved_str, sw/2 - saved_w/2, sh - 26, Bgra::new(70, 70, 100));
    }
}
