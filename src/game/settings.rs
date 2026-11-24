use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str_shadow_scaled, str_width_scaled, draw_str, str_width};
use crate::renderer::hud::{COLOR_DARK_BG, COLOR_PANEL_BG, COLOR_BORDER, draw_button_hints, draw_menu_selection};
use crate::world::{SCREEN_W, SCREEN_H};

const SETTINGS_PATH: &str = "/mnt/SDCARD/.arty_settings";

// Controls reference text is shared with the title's HOW TO PLAY pager —
// single source in title.rs.
use super::title::CONTROLS_PAGES;

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
        use crate::renderer::hud::{draw_side_menu, SideItem, SideCard};
        let sound = if self.settings.sound_on { "ON" } else { "OFF" };
        let rows = [
            SideItem { detail: sound, ..SideItem::new("SOUND") },
            SideItem { detail: ">", ..SideItem::new("CONTROLS") },
        ];
        let body = if self.cursor == 0 { "Sound effects on or off. A or LEFT/RIGHT to switch." }
                   else { "Buttons for moving, aiming and every weapon." };
        let card = SideCard { title: rows[self.cursor.min(1)].label, body, stats: &[] };
        draw_side_menu(buf, "SETTINGS", "SAVES ON EXIT", &rows, self.cursor, &card, &[("A", "SELECT"), ("B", "BACK")]);
    }

    fn draw_controls(&self, buf: &mut WorldBuffer, page: usize) {
        super::title::draw_help_page(buf, CONTROLS_PAGES, page, "CONTROLS");
    }
}
