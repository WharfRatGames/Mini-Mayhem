use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::cosmetic_sprites;
use crate::renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
use crate::renderer::keyboard::Keyboard;
use crate::world::{SCREEN_W, SCREEN_H};

// ── Roster ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Roster {
    pub id:           i64,
    pub name:         String,
    pub worm_names:   [String; 4],
    pub avatar_id:    u8,
    pub headstone_id: u8,  // 0–5; see HEADSTONE_COUNT
    // Per-soldier cosmetics (one entry per soldier, 0 = default)
    pub hat_ids:           [u8; 4],
    pub uniform_color_ids: [u8; 4],
    pub boot_color_ids:    [u8; 4],
    pub gun_style_ids:     [u8; 4],
}

impl Roster {
    pub fn default_named(n: usize) -> Self {
        Self {
            id: 0,
            name: "New Team".to_string(),
            worm_names: std::array::from_fn(|i| format!("Soldier {}", i + 1 + n * 4)),
            avatar_id: (n % 4) as u8,
            headstone_id: 0,
            hat_ids:           [0; 4],
            uniform_color_ids: [0; 4],
            boot_color_ids:    [0; 4],
            gun_style_ids:     [0; 4],
        }
    }

    /// True if this roster still has all-default names — player hasn't customised it yet.
    pub fn is_uncustomised(&self) -> bool {
        self.worm_names.iter().enumerate().all(|(i, n)| {
            n.starts_with("Soldier ") || n == &format!("Worm {}", i + 1)
        }) && (self.name.ends_with("'s Team") || self.name == "New Team" || self.name == "My Team")
    }
}

// ── Account action returned by AccountScreen ──────────────────────────────────

pub enum AccountAction {
    LoggedIn { token: String, username: String, rosters: Vec<Roster> },
    Back,
}

// ── Login / register screen ───────────────────────────────────────────────────

enum LoginScreen { Choice, Username, Password }

pub struct AccountScreen {
    screen:      LoginScreen,
    is_register: bool,
    username:    Keyboard,
    password:    Keyboard,
    error:       String,
}

impl AccountScreen {
    pub fn new() -> Self {
        Self {
            screen:      LoginScreen::Choice,
            is_register: false,
            username:    Keyboard::new(16),
            password:    Keyboard::new(32),
            error:       String::new(),
        }
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer, cam_x: i32) -> Option<AccountAction> {
        match self.screen {
            LoginScreen::Choice => {
                if input.just_pressed(Button::Start) || input.just_pressed(Button::B) {
                    return Some(AccountAction::Back);
                }
                if input.just_pressed(Button::A) {
                    self.is_register = false;
                    self.error.clear();
                    self.screen = LoginScreen::Username;
                }
                if input.just_pressed(Button::Y) {
                    self.is_register = true;
                    self.error.clear();
                    self.screen = LoginScreen::Username;
                }
            }
            LoginScreen::Username => {
                if input.just_pressed(Button::B) && self.username.text.is_empty() {
                    self.screen = LoginScreen::Choice;
                    return None;
                }
                if self.username.update(input) { self.screen = LoginScreen::Password; }
            }
            LoginScreen::Password => {
                if input.just_pressed(Button::B) && self.password.text.is_empty() {
                    self.screen = LoginScreen::Username;
                    self.password = Keyboard::new(32);
                    return None;
                }
                if self.password.update(input) {
                    let u = self.username.text.clone();
                    let p = self.password.text.clone();
                    let result = if self.is_register {
                        try_register(&u, &p)
                    } else {
                        try_login(&u, &p)
                    };
                    match result {
                        Ok((token, username, rosters)) => {
                            return Some(AccountAction::LoggedIn { token, username, rosters });
                        }
                        Err(e) => {
                            self.error = e;
                            self.screen = LoginScreen::Choice;
                            self.username = Keyboard::new(16);
                            self.password = Keyboard::new(32);
                        }
                    }
                }
            }
        }
        self.draw(buf, cam_x);
        None
    }

    fn draw(&self, buf: &mut WorldBuffer, cam_x: i32) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        buf.fill_rect(cam_x, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(8, 10, 22));
        buf.fill_rect(cam_x, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
        let title = match self.screen {
            LoginScreen::Choice   => "ACCOUNT",
            LoginScreen::Username => if self.is_register { "NEW ACCOUNT" } else { "LOG IN" },
            LoginScreen::Password => "ENTER PASSWORD",
        };
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, cam_x + sw/2 - tw/2, 10, Bgra::new(255, 210, 50), 2);
        if !self.error.is_empty() {
            let ew = str_width_scaled(&self.error, 2);
            draw_str_scaled(buf, &self.error, cam_x + sw/2 - ew/2, sh/4, Bgra::new(220, 60, 60), 2);
        }
        match self.screen {
            LoginScreen::Choice => {
                let mid = cam_x + sw/2;
                draw_str_scaled(buf, "A  LOG IN",      mid - str_width_scaled("A  LOG IN", 2)/2,      sh/2 - 20, Bgra::new(80, 200, 80),   2);
                draw_str_scaled(buf, "Y  NEW ACCOUNT", mid - str_width_scaled("Y  NEW ACCOUNT", 2)/2, sh/2 + 10, Bgra::new(100, 160, 255), 2);
                draw_str_scaled(buf, "B  BACK",        mid - str_width_scaled("B  BACK", 2)/2,        sh/2 + 40, Bgra::new(140, 140, 140), 2);
            }
            LoginScreen::Username => self.username.draw(buf, cam_x),
            LoginScreen::Password => self.password.draw(buf, cam_x),
        }
    }
}

// ── Roster picker ─────────────────────────────────────────────────────────────

enum RosterPickerScreen {
    List,
    EditingRoster { editor: RosterEditor, is_new: bool },
}

pub enum RosterAction {
    Selected(Roster),
    /// Play with default generic soldier names — no roster applied.
    Skip,
    Back,
}

pub struct RosterPicker {
    pub rosters: Vec<Roster>,
    cursor:      usize,
    token:       String,
    screen:      RosterPickerScreen,
    error:       String,
}

impl RosterPicker {
    pub fn new(token: String, rosters: Vec<Roster>) -> Self {
        // If the player only has one roster and hasn't customised it, open the editor immediately
        let screen = if rosters.len() == 1 && rosters[0].is_uncustomised() {
            let editor = RosterEditor::new(rosters[0].clone());
            // id == 0 means local placeholder never saved to server → must CREATE, not UPDATE
            let is_new = rosters[0].id == 0;
            RosterPickerScreen::EditingRoster { editor, is_new }
        } else {
            RosterPickerScreen::List
        };
        Self { rosters, cursor: 0, token, screen, error: String::new() }
    }

    /// Like `new` but always shows the list — no auto-open of the editor.
    /// Used for in-game team selection where the player just needs to pick and go.
    pub fn new_list_only(token: String, rosters: Vec<Roster>) -> Self {
        Self { rosters, cursor: 0, token, screen: RosterPickerScreen::List, error: String::new() }
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer, cam_x: i32) -> Option<RosterAction> {
        // ── Editor state ──────────────────────────────────────────────────────
        if let RosterPickerScreen::EditingRoster { editor, is_new } = &mut self.screen {
            let is_new = *is_new;
            let result = editor.update(input, buf, cam_x);
            if editor.cancelled { self.screen = RosterPickerScreen::List; return None; }
            if let Some(r) = result {
                // Update local list immediately — no waiting for server
                if is_new {
                    self.rosters.push(r.clone());
                    self.cursor = self.rosters.len() - 1;
                } else if let Some(pos) = self.rosters.iter().position(|x| x.id == r.id) {
                    self.rosters[pos] = r.clone();
                    self.cursor = pos;
                }
                save_cached_rosters(&self.rosters);
                self.screen = RosterPickerScreen::List;
                // Fire-and-forget to server in background
                // Use CREATE whenever id==0 (roster never saved to server yet)
                let creating = is_new || r.id == 0;
                let body = if creating {
                    format!(r#"{{"token":"{}","name":"{}","worm_names":[{},{},{},{}]}}"#,
                        self.token, r.name,
                        json_str(&r.worm_names[0]), json_str(&r.worm_names[1]),
                        json_str(&r.worm_names[2]), json_str(&r.worm_names[3]))
                } else {
                    format!(r#"{{"token":"{}","id":{},"name":"{}","worm_names":[{},{},{},{}]}}"#,
                        self.token, r.id, r.name,
                        json_str(&r.worm_names[0]), json_str(&r.worm_names[1]),
                        json_str(&r.worm_names[2]), json_str(&r.worm_names[3]))
                };
                let endpoint = if creating { "/api/rosters/create" } else { "/api/rosters/update" };
                std::thread::spawn(move || { http_post(endpoint, &body).ok(); });
            }
            return None;
        }

        // ── List state ────────────────────────────────────────────────────────
        let n = self.rosters.len();
        if input.just_pressed(Button::Up)   { self.cursor = if self.cursor == 0 { n.saturating_sub(1) } else { self.cursor - 1 }; }
        if input.just_pressed(Button::Down) { self.cursor = if n > 0 { (self.cursor + 1) % n } else { 0 }; }

        if input.just_pressed(Button::A) && !self.rosters.is_empty() {
            return Some(RosterAction::Selected(self.rosters[self.cursor].clone()));
        }
        if input.just_pressed(Button::B)     { return Some(RosterAction::Skip); }
        if input.just_pressed(Button::Start) { return Some(RosterAction::Back); }

        // L1/R1 = quick-cycle avatar on selected roster without opening editor
        if !self.rosters.is_empty() {
            let n = crate::renderer::avatar::AVATAR_COUNT as u8;
            if input.just_pressed(Button::L1) {
                self.rosters[self.cursor].avatar_id = (self.rosters[self.cursor].avatar_id + n - 1) % n;
                save_cached_rosters(&self.rosters);
                let r = self.rosters[self.cursor].clone();
                let token = self.token.clone();
                std::thread::spawn(move || {
                    let body = format!(r#"{{"token":"{}","id":{},"name":"{}","worm_names":["{}","{}","{}","{}"]}}"#,
                        token, r.id, r.name, r.worm_names[0], r.worm_names[1], r.worm_names[2], r.worm_names[3]);
                    crate::game::account::http_post("/api/rosters/update", &body).ok();
                });
            }
            if input.just_pressed(Button::R1) {
                self.rosters[self.cursor].avatar_id = (self.rosters[self.cursor].avatar_id + 1) % n;
                save_cached_rosters(&self.rosters);
                let r = self.rosters[self.cursor].clone();
                let token = self.token.clone();
                std::thread::spawn(move || {
                    let body = format!(r#"{{"token":"{}","id":{},"name":"{}","worm_names":["{}","{}","{}","{}"]}}"#,
                        token, r.id, r.name, r.worm_names[0], r.worm_names[1], r.worm_names[2], r.worm_names[3]);
                    crate::game::account::http_post("/api/rosters/update", &body).ok();
                });
            }
        }

        if input.just_pressed(Button::X) {
            self.screen = RosterPickerScreen::EditingRoster { editor: RosterEditor::new_blank(), is_new: true };
            self.error.clear();
            return None;
        }
        if input.just_pressed(Button::Y) && !self.rosters.is_empty() {
            let r = self.rosters[self.cursor].clone();
            self.screen = RosterPickerScreen::EditingRoster { editor: RosterEditor::new(r), is_new: false };
            self.error.clear();
            return None;
        }
        // Select = delete immediately (fire-and-forget, update list now)
        if input.just_pressed(Button::Select) && n > 1 {
            let rid = self.rosters[self.cursor].id;
            let token = self.token.clone();
            std::thread::spawn(move || {
                let body = format!(r#"{{"token":"{}","id":{}}}"#, token, rid);
                http_post("/api/rosters/delete", &body).ok();
            });
            self.rosters.remove(self.cursor);
            self.cursor = self.cursor.min(self.rosters.len().saturating_sub(1));
            save_cached_rosters(&self.rosters);
        }

        self.draw(buf, cam_x);
        None
    }


    fn draw(&self, buf: &mut WorldBuffer, cam_x: i32) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        buf.fill_rect(cam_x, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(8, 10, 22));
        buf.fill_rect(cam_x, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
        let title = "SELECT TEAM";
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, cam_x + sw/2 - tw/2, 10, Bgra::new(255, 210, 50), 2);

        if !self.error.is_empty() {
            draw_str(buf, &self.error, cam_x + 10, 50, Bgra::new(220, 60, 60));
        }

        const AV_PICK: u32 = 64;
        let row_h    = AV_PICK as i32 + 12;
        let list_top = 56i32;
        for (i, roster) in self.rosters.iter().enumerate() {
            let y = list_top + i as i32 * row_h;
            if y + row_h > sh - 30 { break; }
            let selected = i == self.cursor;
            if selected {
                buf.fill_rect(cam_x + 4, y - 2, (sw - 8) as u32, (row_h - 2) as u32, Bgra::new(28, 35, 70));
            }
            // Avatar on left
            use crate::renderer::avatar::draw_avatar;
            draw_avatar(buf, cam_x + 12, y + 4, AV_PICK, roster.avatar_id);
            // Team name + soldier names to the right
            let tx = cam_x + 12 + AV_PICK as i32 + 12;
            let nc = if selected { Bgra::new(255, 220, 50) } else { Bgra::new(200, 200, 230) };
            draw_str_scaled(buf, &roster.name, tx, y + 6, nc, 2);
            let preview = roster.worm_names.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("  ");
            draw_str(buf, &preview, tx, y + 32, Bgra::new(120, 120, 160));
        }

        let hint_y = sh - 22;
        let hint = if self.rosters.len() > 1 {
            "A=USE  L1/R1=AVATAR  X=NEW  Y=EDIT  SEL=DEL  B=SKIP"
        } else {
            "A=USE  L1/R1=AVATAR  X=NEW TEAM  Y=EDIT  B=SKIP"
        };
        draw_str(buf, hint, cam_x + sw/2 - str_width(hint)/2, hint_y, Bgra::new(60, 60, 90));
    }
}

// ── Roster editor (name + 4 worm names) ──────────────────────────────────────

enum EditorField { TeamName, Worm(usize), Done }

pub struct RosterEditor {
    roster:    Roster,
    field:     usize, // 0=team name, 1-4=soldiers, 5=save
    keyboard:  Option<Keyboard>,
    pub cancelled: bool,
    /// True when creating a brand-new team — fields open blank.
    is_new: bool,
}

impl RosterEditor {
    pub fn new(roster: Roster) -> Self {
        Self { roster, field: 0, keyboard: None, cancelled: false, is_new: false }
    }

    pub fn new_blank() -> Self {
        Self {
            roster: Roster::default_named(0),
            field: 0,
            keyboard: None,
            cancelled: false,
            is_new: true,
        }
    }

    /// Returns the finished roster when saved, None while editing.
    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer, cam_x: i32) -> Option<Roster> {
        if let Some(kb) = &mut self.keyboard {
            if kb.update(input) {
                let text = kb.text.clone();
                match self.field {
                    0 => self.roster.name = text,
                    1..=4 => self.roster.worm_names[self.field - 1] = text,
                    _ => {}
                }
                self.keyboard = None;
                // Auto-advance to next field so user naturally reaches SAVE
                self.field = (self.field + 1).min(5);
            }
            if let Some(kb) = &self.keyboard {
                kb.draw(buf, cam_x);
            }
            return None;
        }

        let fields = 8; // 0=team name, 1-4=soldier names, 5=avatar, 6=headstone, 7=save
        if input.just_pressed(Button::Up)   { self.field = if self.field == 0 { fields-1 } else { self.field-1 }; }
        if input.just_pressed(Button::Down) { self.field = (self.field + 1) % fields; }

        // START = save shortcut; field 7 A = save
        if input.just_pressed(Button::Start) || (input.just_pressed(Button::A) && self.field == 7) {
            return Some(self.roster.clone());
        }

        // Avatar field (5): L/R cycles avatar
        if self.field == 5 {
            let n = crate::renderer::avatar::AVATAR_COUNT as u8;
            if input.just_pressed(Button::Left) || input.just_pressed(Button::L1) {
                self.roster.avatar_id = (self.roster.avatar_id + n - 1) % n;
            }
            if input.just_pressed(Button::Right) || input.just_pressed(Button::R1) {
                self.roster.avatar_id = (self.roster.avatar_id + 1) % n;
            }
        }

        // Headstone field (6): L/R cycles headstone design
        if self.field == 6 {
            let n = crate::renderer::draw_sprites::HEADSTONE_COUNT;
            if input.just_pressed(Button::Left) || input.just_pressed(Button::L1) {
                self.roster.headstone_id = (self.roster.headstone_id + n - 1) % n;
            }
            if input.just_pressed(Button::Right) || input.just_pressed(Button::R1) {
                self.roster.headstone_id = (self.roster.headstone_id + 1) % n;
            }
        }

        if input.just_pressed(Button::A) && self.field != 5 && self.field != 6 {
            let max = if self.field == 0 { 24 } else { 16 };
            // Always open blank — player types the name they want from scratch
            self.keyboard = Some(Keyboard::new(max));
        }

        if input.just_pressed(Button::B) {
            self.cancelled = true;
            return None;
        }

        self.draw(buf, cam_x);
        None
    }

    fn draw(&self, buf: &mut WorldBuffer, cam_x: i32) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        buf.fill_rect(cam_x, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(8, 10, 22));
        buf.fill_rect(cam_x, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
        let title = "EDIT TEAM";
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, cam_x + sw/2 - tw/2, 10, Bgra::new(255, 210, 50), 2);

        let labels = ["TEAM NAME", "SOLDIER 1", "SOLDIER 2", "SOLDIER 3", "SOLDIER 4", "AVATAR", "HEADSTONE", "SAVE"];
        let values: [&str; 6] = [
            self.roster.name.as_str(),
            self.roster.worm_names[0].as_str(),
            self.roster.worm_names[1].as_str(),
            self.roster.worm_names[2].as_str(),
            self.roster.worm_names[3].as_str(),
            "",
        ];

        let top   = 50i32;
        // Avatar row (5) and Headstone row (6) are taller for previews
        let row_h_for = |i: usize| if i == 5 || i == 6 { 56i32 } else { 44i32 };
        let mut y_cursor = top;
        for i in 0..8usize {
            let row_h = row_h_for(i);
            let y = y_cursor;
            if y > sh - 20 { break; }
            let selected = i == self.field;
            if selected {
                buf.fill_rect(cam_x + 8, y - 2, (sw - 16) as u32, (row_h - 4) as u32, Bgra::new(28, 35, 70));
            }
            let lc = if selected { Bgra::new(180, 180, 200) } else { Bgra::new(80, 80, 110) };
            draw_str(buf, labels[i], cam_x + 20, y + 4, lc);
            if i < 5 {
                let vc = if selected { Bgra::new(255, 220, 50) } else { Bgra::new(200, 200, 230) };
                let vw = str_width_scaled(values[i], 2);
                draw_str_scaled(buf, values[i], cam_x + sw - vw - 20, y - 2, vc, 2);
            } else if i == 5 {
                // Avatar: preview + cycle hint
                use crate::renderer::avatar::draw_avatar;
                let av_size = 48u32;
                let px = cam_x + sw / 2 - av_size as i32 / 2;
                draw_avatar(buf, px, y, av_size, self.roster.avatar_id);
                if selected {
                    let arrow = "< L/R >";
                    draw_str(buf, arrow, cam_x + sw/2 - str_width(arrow)/2, y + av_size as i32 + 2, Bgra::new(120, 120, 180));
                }
            } else if i == 6 {
                // Headstone: small preview centred in row
                use crate::renderer::draw_sprites::draw_headstone;
                use crate::world::WorldPos;
                let hx = cam_x + sw / 2;
                let hy = y + row_h - 8;
                draw_headstone(buf, WorldPos::new(hx as f32, hy as f32), 0, self.roster.headstone_id);
                if selected {
                    let arrow = "< L/R >";
                    draw_str(buf, arrow, hx + 20, y + 10, Bgra::new(120, 120, 180));
                }
            } else {
                let sc = if selected { Bgra::new(80, 220, 120) } else { Bgra::new(50, 120, 70) };
                let sw2 = str_width_scaled("[ SAVE ]", 2);
                draw_str_scaled(buf, "[ SAVE ]", cam_x + sw/2 - sw2/2, y + 4, sc, 2);
            }
            y_cursor += row_h;
        }
        let hint = if self.field == 7 { "A or START = SAVE TEAM    B=CANCEL" } else if self.field == 6 { "LEFT/RIGHT = CHANGE HEADSTONE   START=SAVE" } else if self.field == 5 { "LEFT/RIGHT = CHANGE AVATAR   START=SAVE" } else { "A=NAME FIELD  START=SAVE  B=CANCEL" };
        draw_str(buf, hint, cam_x + sw/2 - str_width(hint)/2, sh - 18, Bgra::new(60, 60, 90));
    }
}

// ── HTTP helpers ──────────────────────────────────────────────────────────────

pub fn http_post(path: &str, body: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    let host = "crumbonium.duckdns.org";
    let req = format!("POST {} HTTP/1.0\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", path, host, body.len(), body);
    let sock = (host, 80u16).to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("no addr")?;
    let mut stream = TcpStream::connect_timeout(&sock, std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).ok();
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    Ok(resp.split("\r\n\r\n").nth(1).or_else(|| resp.split("\n\n").nth(1)).unwrap_or("").to_string())
}

pub fn http_get(path: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    let host = "crumbonium.duckdns.org";
    let req = format!("GET {} HTTP/1.0\r\nHost: {}\r\nConnection: close\r\n\r\n", path, host);
    let sock = (host, 80u16).to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("no addr")?;
    let mut stream = TcpStream::connect_timeout(&sock, std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).ok();
    stream.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp).map_err(|e| e.to_string())?;
    Ok(resp.split("\r\n\r\n").nth(1).or_else(|| resp.split("\n\n").nth(1)).unwrap_or("").to_string())
}

/// Hit /player/daily_login. Returns (scrap_awarded, weekly_bonus) if a new bonus was earned,
/// or None if already claimed today or the call failed.
pub fn claim_daily_login(token: &str) -> Option<(u32, u32)> {
    let body = format!(r#"{{"token":"{}"}}"#, token);
    let resp = http_post("/api/player/daily_login", &body).ok()?;
    let already: bool = json_field(&resp, "already_claimed").map(|s| s == "true").unwrap_or(false);
    if already { return None; }
    let earned: u32 = json_field(&resp, "scrap_awarded").and_then(|s| s.parse().ok()).unwrap_or(0);
    if earned == 0 { return None; }
    let weekly: u32 = json_field(&resp, "weekly_bonus").and_then(|s| s.parse().ok()).unwrap_or(0);
    Some((earned, weekly))
}

// ── JSON helpers ──────────────────────────────────────────────────────────────

pub fn json_field(json: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\":", key);
    let start = json.find(&search)? + search.len();
    let rest = json[start..].trim_start();
    if rest.starts_with('"') {
        let end = rest[1..].find('"')? + 1;
        Some(rest[1..end].to_string())
    } else {
        let end = rest.find(|c: char| c == ',' || c == '}').unwrap_or(rest.len());
        Some(rest[..end].trim().to_string())
    }
}

pub fn json_str(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\\\""))
}

/// Parse the `rosters` array from a login/register JSON response.
pub fn parse_rosters_from_json(json: &str) -> Vec<Roster> {
    let mut out = Vec::new();
    // Find "rosters":[...]
    let key = "\"rosters\":";
    let start = match json.find(key) {
        Some(i) => i + key.len(),
        None => return out,
    };
    let rest = json[start..].trim_start();
    if !rest.starts_with('[') { return out; }
    // Split on { } objects inside the array
    let mut depth = 0i32;
    let mut obj_start = None;
    for (i, ch) in rest.char_indices() {
        match ch {
            '{' => { depth += 1; if depth == 1 { obj_start = Some(i); } }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    if let Some(s) = obj_start {
                        let obj = &rest[s..=i];
                        if let Some(r) = parse_roster_obj(obj) { out.push(r); }
                    }
                    obj_start = None;
                }
            }
            ']' if depth == 0 => break,
            _ => {}
        }
    }
    out
}

fn parse_u8_arr4(obj: &str, key: &str) -> [u8; 4] {
    let k = format!("\"{}\":", key);
    if let Some(start) = obj.find(&k) {
        let rest = obj[start + k.len()..].trim_start();
        if rest.starts_with('[') {
            let end = rest.find(']').unwrap_or(rest.len());
            let vals: Vec<u8> = rest[1..end].split(',')
                .filter_map(|s| s.trim().parse().ok()).collect();
            if vals.len() >= 4 { return [vals[0], vals[1], vals[2], vals[3]]; }
        }
    }
    [0; 4]
}

fn parse_roster_obj(obj: &str) -> Option<Roster> {
    let id: i64 = json_field(obj, "id").and_then(|s| s.parse().ok()).unwrap_or(0);
    let name = json_field(obj, "name").unwrap_or_else(|| "My Team".to_string());
    let avatar_id: u8 = json_field(obj, "avatar_id").and_then(|s| s.parse().ok()).unwrap_or(0);
    let wn_key = "\"worm_names\":";
    let wn_start = obj.find(wn_key)? + wn_key.len();
    let wn_rest = obj[wn_start..].trim_start();
    let arr_end = wn_rest.find(']').unwrap_or(wn_rest.len());
    let arr = &wn_rest[..arr_end];
    let mut worm_names: Vec<String> = arr.split(',')
        .map(|s| s.trim().trim_matches(|c: char| c == '"' || c == '[' || c == ']').trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    while worm_names.len() < 4 { worm_names.push(format!("Worm {}", worm_names.len() + 1)); }
    let headstone_id: u8 = json_field(obj, "headstone_id").and_then(|s| s.parse().ok()).unwrap_or(0);
    Some(Roster {
        id,
        name,
        worm_names: std::array::from_fn(|i| worm_names[i].clone()),
        avatar_id,
        headstone_id,
        hat_ids:           parse_u8_arr4(obj, "hat_ids"),
        uniform_color_ids: parse_u8_arr4(obj, "uniform_color_ids"),
        boot_color_ids:    parse_u8_arr4(obj, "boot_color_ids"),
        gun_style_ids:     parse_u8_arr4(obj, "gun_style_ids"),
    })
}

/// Fetch scrap balance and owned cosmetic IDs from `/profile`.
/// Returns `(scrap, owned_hats, owned_gun_styles, owned_uniforms, owned_boots)`.
pub fn fetch_profile(token: &str) -> Option<(u32, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> {
    let path = format!("/api/profile?token={}", token);
    let resp = http_get(&path).ok()?;
    let scrap: u32 = json_field(&resp, "scrap").and_then(|s| s.parse().ok()).unwrap_or(0);

    fn parse_id_arr(json: &str, key: &str) -> Vec<u8> {
        let search = format!("\"{}\":", key);
        let after = match json.find(&search) { Some(s) => s + search.len(), None => return vec![] };
        let bracket = match json[after..].find('[') { Some(e) => after + e + 1, None => return vec![] };
        let end = match json[bracket..].find(']') { Some(e) => bracket + e, None => return vec![] };
        let start = bracket;
        json[start..end].split(',')
            .filter_map(|s| s.trim().parse::<u8>().ok())
            .collect()
    }

    let hats      = parse_id_arr(&resp, "unlocked_hats");
    let guns      = parse_id_arr(&resp, "unlocked_gun_styles");
    let uniforms  = parse_id_arr(&resp, "unlocked_uniforms");
    let boots     = parse_id_arr(&resp, "unlocked_boots");
    Some((scrap, hats, guns, uniforms, boots))
}

/// POST to `/shop/buy`. Returns Ok(new_scrap_balance) or Err(message).
pub fn shop_buy(token: &str, cosm_type: &str, cosm_id: u8) -> Result<(), String> {
    let body = format!(
        "{{\"token\":\"{}\",\"cosmetic_type\":\"{}\",\"cosmetic_id\":{}}}",
        token, cosm_type, cosm_id
    );
    let resp = http_post("/api/shop/buy", &body)?;
    if resp.contains("error") {
        let msg = json_field(&resp, "error").unwrap_or_else(|| "purchase failed".to_string());
        Err(msg)
    } else {
        Ok(())
    }
}

pub fn fetch_rosters(token: &str) -> Result<Vec<Roster>, String> {
    let path = format!("/api/rosters?token={}", token);
    let resp = http_get(&path)?;
    // Response is a JSON array directly
    let wrapped = format!("{{\"rosters\":{}}}", resp);
    Ok(parse_rosters_from_json(&wrapped))
}

// ── Credentials storage ───────────────────────────────────────────────────────

pub fn save_creds(username: &str, token: &str) {
    let content = format!("{}\n{}\n", username, token);
    for path in &["/mnt/SDCARD/App/Arty/creds.txt", "/tmp/arty_creds.txt"] {
        if std::fs::write(path, &content).is_ok() { break; }
    }
}

pub fn load_saved_creds() -> Option<(String, String)> {
    let content = std::fs::read_to_string("/mnt/SDCARD/App/Arty/creds.txt")
        .or_else(|_| std::fs::read_to_string("/tmp/arty_creds.txt")).ok()?;
    let mut lines = content.lines();
    Some((lines.next()?.to_string(), lines.next()?.to_string()))
}

// ── Pending reconnect persistence ──────────────────────────────────────────
// Survives a full app restart (the common case after "LOST CONNECTION" on the
// Miyoo, where players relaunch rather than waiting at the title screen).

const RECONNECT_PATHS: [&str; 2] = ["/mnt/SDCARD/App/Arty/reconnect.txt", "/tmp/arty_reconnect.txt"];

/// Save the session token, game port, and unix timestamp when the drop happened.
pub fn save_pending_reconnect(session_token: &str, port: u16, since_unix: u64) {
    let content = format!("{}\n{}\n{}\n", session_token, port, since_unix);
    for path in &RECONNECT_PATHS {
        if std::fs::write(path, &content).is_ok() { break; }
    }
}

/// Load and clear any pending reconnect state. Returns None if absent, expired
/// (>180s old), or unparseable.
pub fn take_pending_reconnect() -> Option<(String, u16, u64)> {
    let mut content = None;
    for path in &RECONNECT_PATHS {
        if let Ok(c) = std::fs::read_to_string(path) {
            let _ = std::fs::remove_file(path);
            content = Some(c);
            break;
        }
    }
    let content = content?;
    let mut lines = content.lines();
    let tok = lines.next()?.to_string();
    let port: u16 = lines.next()?.parse().ok()?;
    let since: u64 = lines.next()?.parse().ok()?;
    if tok.is_empty() { return None; }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    if now.saturating_sub(since) >= 180 { return None; }
    Some((tok, port, since))
}

pub fn clear_pending_reconnect() {
    for path in &RECONNECT_PATHS { let _ = std::fs::remove_file(path); }
}

/// Persist the roster list locally so the next launch loads instantly.
pub fn save_cached_rosters(rosters: &[Roster]) {
    let body = rosters.iter().map(|r| {
        let h = r.hat_ids;
        let u = r.uniform_color_ids;
        let b = r.boot_color_ids;
        let g = r.gun_style_ids;
        format!(r#"{{"id":{},"name":{},"avatar_id":{},"headstone_id":{},"worm_names":[{},{},{},{}],"hat_ids":[{},{},{},{}],"uniform_color_ids":[{},{},{},{}],"boot_color_ids":[{},{},{},{}],"gun_style_ids":[{},{},{},{}]}}"#,
            r.id, json_str(&r.name), r.avatar_id, r.headstone_id,
            json_str(&r.worm_names[0]), json_str(&r.worm_names[1]),
            json_str(&r.worm_names[2]), json_str(&r.worm_names[3]),
            h[0],h[1],h[2],h[3], u[0],u[1],u[2],u[3],
            b[0],b[1],b[2],b[3], g[0],g[1],g[2],g[3])
    }).collect::<Vec<_>>().join(",");
    let json = format!("[{}]", body);
    for path in &["/mnt/SDCARD/App/Arty/rosters.txt", "/tmp/arty_rosters.txt"] {
        if std::fs::write(path, &json).is_ok() { break; }
    }
}

/// Save the chosen roster for a specific match so it can't be changed between turns.
pub fn save_match_roster(match_id: i64, roster: &Roster) {
    let h = roster.hat_ids;
    let u = roster.uniform_color_ids;
    let b = roster.boot_color_ids;
    let g = roster.gun_style_ids;
    let line = format!("{}|{}|{}|{}|{}|{}|{}|{}|{},{},{},{}|{},{},{},{}|{},{},{},{}|{},{},{},{}\n",
        match_id, roster.avatar_id,
        roster.name.replace('|', "_"),
        roster.worm_names[0].replace('|', "_"),
        roster.worm_names[1].replace('|', "_"),
        roster.worm_names[2].replace('|', "_"),
        roster.worm_names[3].replace('|', "_"),
        roster.headstone_id,
        h[0],h[1],h[2],h[3], u[0],u[1],u[2],u[3],
        b[0],b[1],b[2],b[3], g[0],g[1],g[2],g[3]);
    // Append to the file (deduplicate by rewriting)
    let path = "/mnt/SDCARD/App/Arty/match_rosters.txt";
    let path2 = "/tmp/arty_match_rosters.txt";
    let existing = std::fs::read_to_string(path).or_else(|_| std::fs::read_to_string(path2)).unwrap_or_default();
    let filtered: String = existing.lines()
        .filter(|l| !l.starts_with(&format!("{}|", match_id)))
        .map(|l| format!("{}\n", l))
        .collect();
    let content = filtered + &line;
    if std::fs::write(path, &content).is_err() { let _ = std::fs::write(path2, &content); }
}

pub fn load_match_roster(match_id: i64) -> Option<Roster> {
    let path = "/mnt/SDCARD/App/Arty/match_rosters.txt";
    let path2 = "/tmp/arty_match_rosters.txt";
    let content = std::fs::read_to_string(path).or_else(|_| std::fs::read_to_string(path2)).ok()?;
    let prefix = format!("{}|", match_id);
    let line = content.lines().find(|l| l.starts_with(&prefix))?;
    let parts: Vec<&str> = line.splitn(13, '|').collect();
    if parts.len() < 7 { return None; }
    fn parse4(s: &str) -> [u8; 4] {
        let v: Vec<u8> = s.split(',').filter_map(|x| x.parse().ok()).collect();
        if v.len() >= 4 { [v[0],v[1],v[2],v[3]] } else { [0;4] }
    }
    Some(Roster {
        id: 0,
        avatar_id:         parts[1].parse().unwrap_or(0),
        name:              parts[2].to_string(),
        worm_names:        [parts[3].to_string(), parts[4].to_string(), parts[5].to_string(), parts[6].to_string()],
        headstone_id:      parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0),
        hat_ids:           parts.get(8).map(|s| parse4(s)).unwrap_or([0;4]),
        uniform_color_ids: parts.get(9).map(|s| parse4(s)).unwrap_or([0;4]),
        boot_color_ids:    parts.get(10).map(|s| parse4(s)).unwrap_or([0;4]),
        gun_style_ids:     parts.get(11).map(|s| parse4(s)).unwrap_or([0;4]),
    })
}

/// Load rosters from local cache — instant, no network.
pub fn load_cached_rosters() -> Vec<Roster> {
    let json = std::fs::read_to_string("/mnt/SDCARD/App/Arty/rosters.txt")
        .or_else(|_| std::fs::read_to_string("/tmp/arty_rosters.txt"))
        .unwrap_or_default();
    if json.trim().is_empty() { return Vec::new(); }
    parse_rosters_from_json(&format!("{{\"rosters\":{}}}", json))
}

// ── Login / register ──────────────────────────────────────────────────────────

fn try_login(username: &str, password: &str) -> Result<(String, String, Vec<Roster>), String> {
    let body = format!("{{\"username\":\"{}\",\"password\":\"{}\"}}", username, password);
    let resp = http_post("/api/login", &body).map_err(|_| "NETWORK ERROR".to_string())?;
    if let Some(token) = json_field(&resp, "token") {
        let stored_name = json_field(&resp, "username").unwrap_or_else(|| username.to_string());
        save_creds(&stored_name, &token);
        let rosters = parse_rosters_from_json(&resp);
        save_cached_rosters(&rosters);
        return Ok((token, stored_name, rosters));
    }
    Err("WRONG USERNAME OR PASSWORD".to_string())
}

fn try_register(username: &str, password: &str) -> Result<(String, String, Vec<Roster>), String> {
    let body = format!("{{\"username\":\"{}\",\"password\":\"{}\"}}", username, password);
    let resp = http_post("/api/register", &body).map_err(|_| "NETWORK ERROR".to_string())?;
    if let Some(token) = json_field(&resp, "token") {
        save_creds(username, &token);
        let rosters = parse_rosters_from_json(&resp);
        save_cached_rosters(&rosters);
        return Ok((token, username.to_string(), rosters));
    }
    if resp.contains("username taken") {
        Err("USERNAME ALREADY TAKEN".to_string())
    } else {
        Err("REGISTRATION FAILED".to_string())
    }
}

// ── Cosmetics equip screen ────────────────────────────────────────────────────

pub enum CosmeticsAction { Saved(Roster), Back }

pub struct CosmeticsScreen {
    roster:         Roster,
    owned_hats:     Vec<u8>,
    owned_uniforms: Vec<u8>,
    owned_boots:    Vec<u8>,
    owned_guns:     Vec<u8>,
    soldier:        usize, // 0-3
    col:            usize, // 0=hat 1=uniform 2=boots 3=gun
}

impl CosmeticsScreen {
    pub fn new(
        roster:         Roster,
        owned_hats:     Vec<u8>,
        owned_guns:     Vec<u8>,
        owned_uniforms: Vec<u8>,
        owned_boots:    Vec<u8>,
    ) -> Self {
        Self { roster, owned_hats, owned_uniforms, owned_boots, owned_guns, soldier: 0, col: 0 }
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer) -> Option<CosmeticsAction> {
        if input.just_pressed(Button::B) { return Some(CosmeticsAction::Back); }
        if input.just_pressed(Button::A) { return Some(CosmeticsAction::Saved(self.roster.clone())); }

        if input.just_pressed(Button::Up)    { self.soldier = self.soldier.saturating_sub(1); }
        if input.just_pressed(Button::Down)  { if self.soldier < 3 { self.soldier += 1; } }
        if input.just_pressed(Button::Left)  { self.col = self.col.saturating_sub(1); }
        if input.just_pressed(Button::Right) { if self.col < 3 { self.col += 1; } }

        let si = self.soldier;
        if input.just_pressed(Button::L1) { self.cycle(-1, si); }
        if input.just_pressed(Button::R1) { self.cycle( 1, si); }

        self.draw(buf);
        None
    }

    fn owned_for_col(&self, col: usize) -> &[u8] {
        match col {
            0 => &self.owned_hats,
            1 => &self.owned_uniforms,
            2 => &self.owned_boots,
            3 => &self.owned_guns,
            _ => &[],
        }
    }

    fn current_id(&self, col: usize, si: usize) -> u8 {
        match col {
            0 => self.roster.hat_ids[si],
            1 => self.roster.uniform_color_ids[si],
            2 => self.roster.boot_color_ids[si],
            3 => self.roster.gun_style_ids[si],
            _ => 0,
        }
    }

    fn set_id(&mut self, col: usize, si: usize, id: u8) {
        match col {
            0 => self.roster.hat_ids[si] = id,
            1 => self.roster.uniform_color_ids[si] = id,
            2 => self.roster.boot_color_ids[si] = id,
            3 => self.roster.gun_style_ids[si] = id,
            _ => {}
        }
    }

    fn cycle(&mut self, dir: i32, si: usize) {
        let col = self.col;
        let owned = self.owned_for_col(col);
        if owned.is_empty() { return; }
        // Options: 0 (none) + owned ids
        let mut opts = vec![0u8];
        opts.extend_from_slice(owned);
        let cur = self.current_id(col, si);
        let pos = opts.iter().position(|&x| x == cur).unwrap_or(0);
        let next = ((pos as i32 + dir).rem_euclid(opts.len() as i32)) as usize;
        self.set_id(col, si, opts[next]);
    }

    fn draw(&self, buf: &mut WorldBuffer) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(8, 10, 22));
        buf.fill_rect(0, 0, SCREEN_W, 28, Bgra::new(18, 22, 48));

        let title = "EQUIP COSMETICS";
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, sw/2 - tw/2, 6, Bgra::new(255, 210, 50), 2);

        let col_labels = ["HAT", "UNIFORM", "BOOTS", "GUN"];
        let col_w = sw / 4;

        // Column headers
        for (c, lbl) in col_labels.iter().enumerate() {
            let cx = c as i32 * col_w + col_w / 2;
            let lw = str_width(lbl) as i32;
            let hdr_col = if c == self.col { Bgra::new(255, 220, 50) } else { Bgra::new(100, 110, 160) };
            draw_str(buf, lbl, cx - lw/2, 32, hdr_col);
        }

        // Soldier rows
        let row_h = (sh - 80) / 4;
        let top = 48i32;

        for si in 0..4usize {
            let ry = top + si as i32 * row_h;
            let selected = si == self.soldier;

            // Row bg
            let row_bg = if selected { Bgra::new(24, 34, 72) } else { Bgra::new(12, 14, 32) };
            buf.fill_rect(0, ry, SCREEN_W, row_h as u32, row_bg);
            if selected {
                buf.fill_rect(0, ry, SCREEN_W, 1, Bgra::new(60, 90, 200));
                buf.fill_rect(0, ry + row_h - 1, SCREEN_W, 1, Bgra::new(60, 90, 200));
            }

            // Soldier name
            let name = &self.roster.worm_names[si];
            draw_str(buf, name, 6, ry + 4, if selected { Bgra::new(255, 220, 60) } else { Bgra::new(160, 160, 200) });

            // Cosmetic cells
            for c in 0..4usize {
                let cx = c as i32 * col_w;
                let cell_selected = selected && c == self.col;
                if cell_selected {
                    buf.fill_rect(cx + 1, ry + 1, (col_w - 2) as u32, (row_h - 2) as u32, Bgra::new(30, 50, 110));
                    buf.fill_rect(cx + 1, ry + 1, (col_w - 2) as u32, 1, Bgra::new(80, 140, 255));
                    buf.fill_rect(cx + 1, ry + 1, 1, (row_h - 2) as u32, Bgra::new(80, 140, 255));
                    buf.fill_rect(cx + 1, ry + row_h - 2, (col_w - 2) as u32, 1, Bgra::new(80, 140, 255));
                    buf.fill_rect(cx + col_w - 2, ry + 1, 1, (row_h - 2) as u32, Bgra::new(80, 140, 255));
                }

                let id = self.current_id(c, si);
                let owned = self.owned_for_col(c);
                let center_x = cx + col_w / 2;
                let icon_cy = ry + row_h / 2;
                let icon_w = col_w - 12;
                let icon_h = row_h - 10;

                // Cosmetic icon (sprite for hat/boots/gun, swatch for uniform)
                match c {
                    0 => if id > 0 { cosmetic_sprites::draw_hat(buf, id, center_x, icon_cy, icon_w, icon_h); },
                    1 => if id > 0 {
                        let col = uniform_swatch_color(id);
                        let sw = (icon_w / 2) as u32;
                        let sh = (icon_h / 2) as u32;
                        buf.fill_rect(center_x - sw as i32 / 2, icon_cy - sh as i32 / 2, sw, sh, col);
                    },
                    2 => cosmetic_sprites::draw_boot(buf, id, center_x, icon_cy, icon_w, icon_h, false),
                    3 => cosmetic_sprites::draw_gun(buf, id, center_x, icon_cy, icon_w, icon_h),
                    _ => {}
                }
                if id == 0 && (c == 0 || c == 1) {
                    let dash = "--";
                    let dw = str_width(dash) as i32;
                    draw_str(buf, dash, center_x - dw/2, icon_cy - 3, Bgra::new(60, 65, 90));
                }

                let text_y = ry + row_h - 12;
                // L1/R1 arrows on selected cell
                if cell_selected && !owned.is_empty() {
                    draw_str(buf, "<", cx + 3, text_y, Bgra::new(140, 160, 255));
                    draw_str(buf, ">", cx + col_w - 10, text_y, Bgra::new(140, 160, 255));
                }
            }
        }

        // Hint bar
        buf.fill_rect(0, sh - 22, SCREEN_W, 22, Bgra::new(12, 14, 35));
        let hint = "L1/R1=CYCLE  ARROWS=NAVIGATE  A=SAVE  B=CANCEL";
        draw_str(buf, hint, sw/2 - str_width(hint)/2, sh - 16, Bgra::new(60, 70, 110));
    }
}

fn uniform_swatch_color(id: u8) -> Bgra {
    match id {
        1 => Bgra::new( 60, 100,  50), // Camo Green
        2 => Bgra::new(190, 155,  90), // Desert Tan
        3 => Bgra::new( 30,  30,  35), // Midnight Black
        4 => Bgra::new(230, 230, 235), // Snow White
        5 => Bgra::new( 30,  40, 120), // Navy
        6 => Bgra::new(200, 120, 160), // Pink Camo
        7 => Bgra::new(200, 165,  40), // Gold Plate
        _ => Bgra::new(120, 120, 120),
    }
}
