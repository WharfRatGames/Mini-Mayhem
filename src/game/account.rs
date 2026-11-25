use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::cosmetic_sprites;
use crate::renderer::font::{draw_str, draw_str_scaled, draw_str_shadow_scaled, str_width, str_width_scaled};
use crate::renderer::hud::{draw_button_hints, draw_menu_selection};
use crate::renderer::hud::COLOR_DARK_BG;
use crate::renderer::keyboard::Keyboard;
use crate::renderer::skeleton::{draw_soldier_skeletal, SoldierAnim};
use crate::world::{SCREEN_W, SCREEN_H};
use crate::world::WorldPos;

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

enum LoginScreen { Choice, Username, Password, ConfirmPassword, ChangePassword { token: String, username: String, rosters: Vec<Roster> } }

pub struct AccountScreen {
    screen:           LoginScreen,
    is_register:      bool,
    username:         Keyboard,
    password:         Keyboard,
    confirm_password: Keyboard,
    error:            String,
    cursor:           usize,  // 0 = LOG IN, 1 = NEW ACCOUNT
}

impl AccountScreen {
    pub fn new() -> Self {
        Self {
            screen:           LoginScreen::Choice,
            is_register:      false,
            username:         Keyboard::new(16),
            password:         Keyboard::new(32),
            confirm_password: Keyboard::new(32),
            error:            String::new(),
            cursor:           0,
        }
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer, cam_x: i32) -> Option<AccountAction> {
        match self.screen {
            LoginScreen::Choice => {
                if input.just_pressed(Button::Start) || input.just_pressed(Button::B) {
                    return Some(AccountAction::Back);
                }
                if input.just_pressed(Button::Up) || input.just_pressed(Button::Down) {
                    self.cursor = 1 - self.cursor;
                }
                if input.just_pressed(Button::A) {
                    self.is_register = self.cursor == 1;
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
                    if self.is_register {
                        self.confirm_password = Keyboard::new(32);
                        self.screen = LoginScreen::ConfirmPassword;
                    } else {
                        let u = self.username.text.clone();
                        let p = self.password.text.clone();
                        match try_login(&u, &p) {
                            Ok((token, username, rosters, needs_pw_reset)) if needs_pw_reset => {
                                self.password = Keyboard::new(32);
                                self.error.clear();
                                self.screen = LoginScreen::ChangePassword { token, username, rosters };
                            }
                            Ok((token, username, rosters, _)) => {
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
            LoginScreen::ConfirmPassword => {
                if input.just_pressed(Button::B) && self.confirm_password.text.is_empty() {
                    self.screen = LoginScreen::Password;
                    self.confirm_password = Keyboard::new(32);
                    return None;
                }
                if self.confirm_password.update(input) {
                    if self.confirm_password.text != self.password.text {
                        self.error = "PASSWORDS DO NOT MATCH".to_string();
                        self.password = Keyboard::new(32);
                        self.confirm_password = Keyboard::new(32);
                        self.screen = LoginScreen::Password;
                    } else {
                        let u = self.username.text.clone();
                        let p = self.password.text.clone();
                        match try_register(&u, &p) {
                            Ok((token, username, rosters, _)) => {
                                return Some(AccountAction::LoggedIn { token, username, rosters });
                            }
                            Err(e) => {
                                self.error = e;
                                self.screen = LoginScreen::Choice;
                                self.username = Keyboard::new(16);
                                self.password = Keyboard::new(32);
                                self.confirm_password = Keyboard::new(32);
                            }
                        }
                    }
                }
            }
            LoginScreen::ChangePassword { .. } => {
                if self.password.update(input) {
                    let new_pw = self.password.text.clone();
                    if let LoginScreen::ChangePassword { token, username, rosters } = &self.screen {
                        match try_change_password(token, &new_pw) {
                            Ok(()) => {
                                let token = token.clone();
                                let username = username.clone();
                                let rosters = rosters.clone();
                                save_creds(&username, &token);
                                return Some(AccountAction::LoggedIn { token, username, rosters });
                            }
                            Err(e) => {
                                self.error = e;
                                self.password = Keyboard::new(32);
                            }
                        }
                    }
                }
            }
        }
        self.draw(buf, cam_x);
        None
    }

    fn draw(&self, buf: &mut WorldBuffer, _cam_x: i32) {
        use crate::renderer::hud::{draw_side_menu, side_panel, SideItem, SideCard, SIDE_RED};
        let sw = SCREEN_W as i32;
        let title = match self.screen {
            LoginScreen::Choice                => "ACCOUNT",
            LoginScreen::Username | LoginScreen::Password => if self.is_register { "NEW ACCOUNT" } else { "LOG IN" },
            LoginScreen::ConfirmPassword       => "NEW ACCOUNT",
            LoginScreen::ChangePassword { .. } => "CHOOSE PASSWORD",
        };
        match self.screen {
            LoginScreen::Choice => {
                let rows = [SideItem::new("LOG IN"), SideItem::new("NEW ACCOUNT")];
                let body = if !self.error.is_empty() { self.error.as_str() }
                    else if self.cursor == 0 { "Log in with your username and password. Your teams, scrap and stats come with you." }
                    else { "Make an account to play online, earn scrap and keep your stats." };
                let card = SideCard { title: rows[self.cursor.min(1)].label, body, stats: &[] };
                draw_side_menu(buf, title, "", &rows, self.cursor, &card, &[("A", "SELECT"), ("B", "BACK")]);
            }
            LoginScreen::Username => self.username.draw_screen(buf, title, "ENTER USERNAME"),
            LoginScreen::Password => self.password.draw_screen(buf, title, "ENTER PASSWORD"),
            LoginScreen::ConfirmPassword => self.confirm_password.draw_screen(buf, title, "TYPE THE PASSWORD AGAIN"),
            LoginScreen::ChangePassword { .. } => self.password.draw_screen(buf, title, "ADMIN RESET YOUR PASSWORD - PICK A NEW ONE"),
        }
        if !self.error.is_empty() && !matches!(self.screen, LoginScreen::Choice) {
            let ew = str_width_scaled(&self.error, 2);
            side_panel(buf, sw/2 - ew/2 - 16, 96, ew + 32, 34);
            draw_str_shadow_scaled(buf, &self.error, sw/2 - ew/2, 106, SIDE_RED, 2);
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
    /// Picking the team for a new TAT game: B goes back (no "skip"), header says PICK TEAM.
    for_game:    bool,
    /// MY TEAMS > ROSTERS: managing teams, not picking one. A edits, B goes back.
    manage:      bool,
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
        Self { rosters, cursor: 0, token, screen, error: String::new(), for_game: false, manage: false }
    }

    /// Like `new` but always shows the list — no auto-open of the editor.
    /// Used for in-game team selection where the player just needs to pick and go.
    pub fn new_list_only(token: String, rosters: Vec<Roster>) -> Self {
        Self { rosters, cursor: 0, token, screen: RosterPickerScreen::List, error: String::new(), for_game: false, manage: false }
    }

    /// MY TEAMS > ROSTERS: edit, add and delete teams. A opens the editor; B (or START) leaves.
    pub fn new_manage(token: String, rosters: Vec<Roster>) -> Self {
        Self { manage: true, ..Self::new_list_only(token, rosters) }
    }

    /// The team for a new TAT game, chosen before matchmaking / code / join: B = back (never skip).
    pub fn new_for_game(token: String, rosters: Vec<Roster>) -> Self {
        Self { for_game: true, ..Self::new_list_only(token, rosters) }
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

        if input.just_pressed(Button::A) && !self.rosters.is_empty() && self.manage {
            let r = self.rosters[self.cursor].clone();
            self.screen = RosterPickerScreen::EditingRoster { editor: RosterEditor::new(r), is_new: false };
            self.error.clear();
            return None;
        }
        if input.just_pressed(Button::A) && !self.rosters.is_empty() {
            return Some(RosterAction::Selected(self.rosters[self.cursor].clone()));
        }
        if input.just_pressed(Button::B)     { return Some(if self.for_game || self.manage { RosterAction::Back } else { RosterAction::Skip }); }
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


    fn draw(&self, buf: &mut WorldBuffer, _cam_x: i32) {
        // Same full-screen side layout as the menus: team names left, the team's card right.
        use crate::renderer::hud::{side_frame, side_rows, side_card, side_card_rect, SideItem, SideCard,
                                   SIDE_GOLD, SIDE_WHITE, SIDE_RED};
        let b = if self.for_game || self.manage { "BACK" } else { "SKIP" };
        let hints: &[(&str, &str)] = match (self.manage, self.rosters.len() > 1) {
            (true, true)   => &[("A", "EDIT"), ("X", "NEW"), ("L1/R1", "AVATAR"), ("SEL", "DELETE"), ("B", "BACK")],
            (true, false)  => &[("A", "EDIT"), ("X", "NEW"), ("L1/R1", "AVATAR"), ("B", "BACK")],
            (false, true)  => &[("A", "USE"), ("Y", "EDIT"), ("X", "NEW"), ("L1/R1", "AVATAR"), ("SEL", "DEL"), ("B", b)],
            (false, false) => &[("A", "USE"), ("Y", "EDIT"), ("X", "NEW"), ("L1/R1", "AVATAR"), ("B", b)],
        };
        let crumb = if self.manage { "ROSTERS" } else if self.for_game { "TEAM FOR THIS GAME" } else { "SELECT TEAM" };
        side_frame(buf, crumb, &format!("{} TEAMS", self.rosters.len()), hints);
        let names: Vec<String> = self.rosters.iter().map(|r| r.name.to_uppercase()).collect();
        let rows: Vec<SideItem> = names.iter().map(|n| SideItem::new(n.as_str())).collect();
        side_rows(buf, &rows, self.cursor);

        let Some(r) = self.rosters.get(self.cursor) else {
            side_card(buf, &SideCard { title: "NO TEAMS", body: "Press X to make a team.", stats: &[] });
            return;
        };
        let body = if self.error.is_empty() { "" } else { self.error.as_str() };
        let title = names[self.cursor].as_str();
        let y = side_card(buf, &SideCard { title, body, stats: &[] });
        let (cx, _cy, cw, _ch) = side_card_rect();
        // Avatar, then the four soldiers
        use crate::renderer::avatar::draw_avatar;
        const AV: u32 = 72;
        draw_avatar(buf, cx + cw / 2 - AV as i32 / 2, y + 4, AV, r.avatar_id);
        let mut ly = y + AV as i32 + 20;
        for (i, w) in r.worm_names.iter().enumerate() {
            buf.fill_rect(cx + 12, ly - 6, (cw - 24) as u32, 1, Bgra::new(60, 64, 110));
            draw_str(buf, &format!("{}", i + 1), cx + 12, ly + 4, Bgra::new(150, 155, 195));
            let ww = str_width_scaled(w, 2);
            draw_str_shadow_scaled(buf, w, cx + cw - 12 - ww, ly, SIDE_WHITE, 2);
            ly += 30;
        }
        if !self.error.is_empty() { draw_str(buf, &self.error, cx + 12, ly + 4, SIDE_RED); }
        let _ = SIDE_GOLD;
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
                let prompt = match self.field { 0 => "TEAM NAME", 1..=4 => "SOLDIER NAME", _ => "" };
                kb.draw_screen(buf, if self.is_new { "NEW TEAM" } else { "EDIT TEAM" }, prompt);
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

    fn draw(&self, buf: &mut WorldBuffer, _cam_x: i32) {
        // Same full-screen side layout as the menus: fields left (label + value), help / preview card right.
        use crate::renderer::hud::{side_frame, side_rows, side_card, side_card_rect, SideItem, SideCard, SIDE_GOLD};
        let hints: &[(&str, &str)] = match self.field {
            5 | 6 => &[("LEFT/RIGHT", "CHANGE"), ("START", "SAVE"), ("B", "CANCEL")],
            7     => &[("A", "SAVE TEAM"), ("B", "CANCEL")],
            _     => &[("A", "TYPE NAME"), ("START", "SAVE"), ("B", "CANCEL")],
        };
        side_frame(buf, if self.is_new { "NEW TEAM" } else { "EDIT TEAM" }, &self.roster.name.to_uppercase(), hints);
        let labels = ["TEAM NAME", "SOLDIER 1", "SOLDIER 2", "SOLDIER 3", "SOLDIER 4", "AVATAR", "HEADSTONE", "SAVE"];
        let av = format!("< {} >", self.roster.avatar_id as u32 + 1);
        let hs = format!("< {} >", self.roster.headstone_id as u32 + 1);
        let values = [
            self.roster.name.as_str(),
            self.roster.worm_names[0].as_str(),
            self.roster.worm_names[1].as_str(),
            self.roster.worm_names[2].as_str(),
            self.roster.worm_names[3].as_str(),
            av.as_str(), hs.as_str(), "",
        ];
        let rows: Vec<SideItem> = labels.iter().zip(values.iter())
            .map(|(&l, &v)| SideItem { detail: v, ..SideItem::new(l) }).collect();
        side_rows(buf, &rows, self.field);

        let body = match self.field {
            0     => "The name shown for your team in matches. A opens the keyboard.",
            1..=4 => "This soldier's name, shown above them in matches. A opens the keyboard.",
            5     => "Your team's picture in the lobby and on the scoreboard. LEFT/RIGHT to change.",
            6     => "The grave your soldiers leave behind. LEFT/RIGHT to change.",
            _     => "Save the team. START saves from any field.",
        };
        let y = side_card(buf, &SideCard { title: labels[self.field], body, stats: &[] });
        let (cx, _cy, cw, _ch) = side_card_rect();
        let mid = cx + cw / 2;
        match self.field {
            5 => {
                use crate::renderer::avatar::draw_avatar;
                const AV: u32 = 96;
                draw_avatar(buf, mid - AV as i32 / 2, y + 10, AV, self.roster.avatar_id);
                draw_str_shadow_scaled(buf, "<", cx + 14, y + 10 + AV as i32 / 2 - 8, SIDE_GOLD, 2);
                draw_str_shadow_scaled(buf, ">", cx + cw - 30, y + 10 + AV as i32 / 2 - 8, SIDE_GOLD, 2);
            }
            6 => {
                use crate::renderer::draw_sprites::draw_headstone;
                use crate::world::WorldPos;
                draw_headstone(buf, WorldPos::new(mid as f32, (y + 70) as f32), 0, self.roster.headstone_id);
                draw_str_shadow_scaled(buf, "<", cx + 14, y + 40, SIDE_GOLD, 2);
                draw_str_shadow_scaled(buf, ">", cx + cw - 30, y + 40, SIDE_GOLD, 2);
            }
            0..=4 => {
                let v = values[self.field];
                let vs = if str_width_scaled(v, 3) <= cw - 24 { 3 } else { 2 };
                draw_str_shadow_scaled(buf, v, mid - str_width_scaled(v, vs) / 2, y + 20, SIDE_GOLD, vs);
            }
            _ => {
                // Summary of what will be saved
                let mut ly = y + 10;
                for w in &self.roster.worm_names {
                    draw_str_shadow_scaled(buf, w, mid - str_width_scaled(w, 2) / 2, ly, Bgra::new(240, 240, 255), 2);
                    ly += 26;
                }
            }
        }
    }
}

// ── HTTP helpers ──────────────────────────────────────────────────────────────

const API_HOST: &str = "crumbonium.duckdns.org";

pub fn http_post(path: &str, body: &str) -> Result<String, String> {
    crate::https::https_post(API_HOST, path, body, 5, 10)
}

pub fn http_get(path: &str) -> Result<String, String> {
    crate::https::https_get(API_HOST, path, 5, 10)
        .and_then(|b| String::from_utf8(b).map_err(|e| e.to_string()))
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
pub fn fetch_profile(token: &str) -> Option<(u32, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, u32)> {
    let path = format!("/api/profile?token={}", token);
    let resp = http_get(&path).ok()?;
    let scrap:    u32 = json_field(&resp, "scrap").and_then(|s| s.parse().ok()).unwrap_or(0);
    let warbonds: u32 = json_field(&resp, "warbonds").and_then(|s| s.parse().ok()).unwrap_or(0);

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
    let result = (scrap, hats, guns, uniforms, boots, warbonds);
    save_cached_profile(&result);
    Some(result)
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

pub type ProfileData = (u32, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, u32);

pub fn save_cached_profile(p: &ProfileData) {
    let ids = |v: &Vec<u8>| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
    let s = format!("{}|{}|{}|{}|{}|{}", p.0, ids(&p.1), ids(&p.2), ids(&p.3), ids(&p.4), p.5);
    #[cfg(feature = "desktop")]
    { let _ = std::fs::write(data_dir().join("profile_cache.txt"), &s); }
    #[cfg(not(feature = "desktop"))]
    for path in &["/mnt/SDCARD/App/Arty/profile_cache.txt", "/tmp/arty_profile_cache.txt"] {
        if std::fs::write(path, &s).is_ok() { break; }
    }
}

pub fn load_cached_profile() -> Option<ProfileData> {
    fn parse_ids(s: &str) -> Vec<u8> {
        s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
    }
    #[cfg(feature = "desktop")]
    let raw = std::fs::read_to_string(data_dir().join("profile_cache.txt")).ok()?;
    #[cfg(not(feature = "desktop"))]
    let raw = ["/mnt/SDCARD/App/Arty/profile_cache.txt", "/tmp/arty_profile_cache.txt"]
        .iter().find_map(|p| std::fs::read_to_string(p).ok())?;
    let mut parts = raw.trim().splitn(6, '|');
    let scrap:    u32 = parts.next()?.trim().parse().ok()?;
    let hats      = parse_ids(parts.next().unwrap_or(""));
    let guns      = parse_ids(parts.next().unwrap_or(""));
    let uniforms  = parse_ids(parts.next().unwrap_or(""));
    let boots     = parse_ids(parts.next().unwrap_or(""));
    let warbonds: u32 = parts.next().unwrap_or("0").trim().parse().unwrap_or(0);
    Some((scrap, hats, guns, uniforms, boots, warbonds))
}

pub fn fetch_rosters(token: &str) -> Result<Vec<Roster>, String> {
    let path = format!("/api/rosters?token={}", token);
    let resp = http_get(&path)?;
    // Response is a JSON array directly
    let wrapped = format!("{{\"rosters\":{}}}", resp);
    Ok(parse_rosters_from_json(&wrapped))
}

// ── Data directory ────────────────────────────────────────────────────────────

/// Returns the directory where save data lives.
/// Desktop: <exe dir>/arty_data/   Miyoo: /mnt/SDCARD/App/Arty/
fn data_dir() -> std::path::PathBuf {
    #[cfg(feature = "desktop")]
    {
        let base = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let dir = base.join("arty_data");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }
    #[cfg(not(feature = "desktop"))]
    std::path::PathBuf::from("/mnt/SDCARD/App/Arty")
}

// ── Credentials storage ───────────────────────────────────────────────────────

pub fn save_creds(username: &str, token: &str) {
    let content = format!("{}\n{}\n", username, token);
    #[cfg(feature = "desktop")]
    { let _ = std::fs::write(data_dir().join("creds.txt"), &content); }
    #[cfg(not(feature = "desktop"))]
    for path in &["/mnt/SDCARD/App/Arty/creds.txt", "/tmp/arty_creds.txt"] {
        if std::fs::write(path, &content).is_ok() { break; }
    }
}

pub fn clear_saved_creds() {
    #[cfg(feature = "desktop")]
    { let _ = std::fs::remove_file(data_dir().join("creds.txt")); }
    #[cfg(not(feature = "desktop"))]
    {
        let _ = std::fs::remove_file("/mnt/SDCARD/App/Arty/creds.txt");
        let _ = std::fs::remove_file("/tmp/arty_creds.txt");
    }
}

pub fn load_saved_creds() -> Option<(String, String)> {
    #[cfg(feature = "desktop")]
    let content = std::fs::read_to_string(data_dir().join("creds.txt")).ok()?;
    #[cfg(not(feature = "desktop"))]
    let content = std::fs::read_to_string("/mnt/SDCARD/App/Arty/creds.txt")
        .or_else(|_| std::fs::read_to_string("/tmp/arty_creds.txt")).ok()?;
    let mut lines = content.lines();
    Some((lines.next()?.to_string(), lines.next()?.to_string()))
}

// ── Pending reconnect persistence ──────────────────────────────────────────
// Survives a full app restart (the common case after "LOST CONNECTION" on the
// Miyoo, where players relaunch rather than waiting at the title screen).

/// Save the session token, game port, and unix timestamp when the drop happened.
pub fn save_pending_reconnect(session_token: &str, port: u16, since_unix: u64) {
    let content = format!("{}\n{}\n{}\n", session_token, port, since_unix);
    #[cfg(feature = "desktop")]
    { let _ = std::fs::write(data_dir().join("reconnect.txt"), &content); }
    #[cfg(not(feature = "desktop"))]
    for path in &["/mnt/SDCARD/App/Arty/reconnect.txt", "/tmp/arty_reconnect.txt"] {
        if std::fs::write(path, &content).is_ok() { break; }
    }
}

/// Load and clear any pending reconnect state. Returns None if absent, expired
/// (>180s old), or unparseable.
pub fn take_pending_reconnect() -> Option<(String, u16, u64)> {
    let content;
    #[cfg(feature = "desktop")]
    {
        let p = data_dir().join("reconnect.txt");
        content = std::fs::read_to_string(&p).ok();
        if content.is_some() { let _ = std::fs::remove_file(&p); }
    }
    #[cfg(not(feature = "desktop"))]
    {
        let paths = ["/mnt/SDCARD/App/Arty/reconnect.txt", "/tmp/arty_reconnect.txt"];
        let mut c = None;
        for path in &paths {
            if let Ok(s) = std::fs::read_to_string(path) {
                let _ = std::fs::remove_file(path);
                c = Some(s);
                break;
            }
        }
        content = c;
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
    #[cfg(feature = "desktop")]
    { let _ = std::fs::remove_file(data_dir().join("reconnect.txt")); }
    #[cfg(not(feature = "desktop"))]
    for path in &["/mnt/SDCARD/App/Arty/reconnect.txt", "/tmp/arty_reconnect.txt"] {
        let _ = std::fs::remove_file(path);
    }
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
    #[cfg(feature = "desktop")]
    { let _ = std::fs::write(data_dir().join("rosters.txt"), &json); }
    #[cfg(not(feature = "desktop"))]
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
    #[cfg(feature = "desktop")]
    let (path_owned, path2_owned) = (data_dir().join("match_rosters.txt"), std::path::PathBuf::new());
    #[cfg(not(feature = "desktop"))]
    let (path_owned, path2_owned) = (
        std::path::PathBuf::from("/mnt/SDCARD/App/Arty/match_rosters.txt"),
        std::path::PathBuf::from("/tmp/arty_match_rosters.txt"),
    );
    let existing = std::fs::read_to_string(&path_owned)
        .or_else(|_| if path2_owned.as_os_str().is_empty() { Err(std::io::Error::other("")) } else { std::fs::read_to_string(&path2_owned) })
        .unwrap_or_default();
    let filtered: String = existing.lines()
        .filter(|l| !l.starts_with(&format!("{}|", match_id)))
        .map(|l| format!("{}\n", l))
        .collect();
    let content = filtered + &line;
    if std::fs::write(&path_owned, &content).is_err() && !path2_owned.as_os_str().is_empty() {
        let _ = std::fs::write(&path2_owned, &content);
    }
}

pub fn load_match_roster(match_id: i64) -> Option<Roster> {
    #[cfg(feature = "desktop")]
    let content = std::fs::read_to_string(data_dir().join("match_rosters.txt")).ok()?;
    #[cfg(not(feature = "desktop"))]
    let content = std::fs::read_to_string("/mnt/SDCARD/App/Arty/match_rosters.txt")
        .or_else(|_| std::fs::read_to_string("/tmp/arty_match_rosters.txt")).ok()?;
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

/// A team as the JSON object the API stores per TAT game (`roster` in /match/create, /match/join,
/// /match/<id>/roster; `my_roster` / `opponent_roster` in the match state).
pub fn roster_json(r: &Roster) -> String {
    let a = |v: [u8; 4]| format!("[{},{},{},{}]", v[0], v[1], v[2], v[3]);
    format!(r#"{{"name":{},"worm_names":[{},{},{},{}],"avatar_id":{},"headstone_id":{},"hat_ids":{},"uniform_color_ids":{},"boot_color_ids":{},"gun_style_ids":{}}}"#,
        json_str(&r.name), json_str(&r.worm_names[0]), json_str(&r.worm_names[1]), json_str(&r.worm_names[2]), json_str(&r.worm_names[3]),
        r.avatar_id, r.headstone_id, a(r.hat_ids), a(r.uniform_color_ids), a(r.boot_color_ids), a(r.gun_style_ids))
}

/// The `{...}` value of `key` in `json` (brace-matched, string-aware), or None if missing / null.
pub fn json_object<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let tag = format!("\"{}\":", key);
    let start = json.find(&tag)? + tag.len();
    let rest = json[start..].trim_start();
    if !rest.starts_with('{') { return None; }
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, c) in rest.char_indices() {
        if in_str { if esc { esc = false } else if c == '\\' { esc = true } else if c == '"' { in_str = false } continue; }
        match c { '"' => in_str = true, '{' => depth += 1, '}' => { depth -= 1; if depth == 0 { return Some(&rest[..=i]); } } _ => {} }
    }
    None
}

/// Parse a team object written by `roster_json` / stored by the API.
pub fn roster_from_json(obj: &str) -> Option<Roster> {
    fn arr(obj: &str, key: &str) -> [u8; 4] {
        let tag = format!("\"{}\":", key);
        let Some(i) = obj.find(&tag) else { return [0; 4] };
        let rest = &obj[i + tag.len()..];
        let (Some(a), Some(b)) = (rest.find('['), rest.find(']')) else { return [0; 4] };
        let v: Vec<u8> = rest[a + 1..b].split(',').filter_map(|x| x.trim().parse().ok()).collect();
        if v.len() >= 4 { [v[0], v[1], v[2], v[3]] } else { [0; 4] }
    }
    fn names(obj: &str) -> Option<[String; 4]> {
        let i = obj.find("\"worm_names\":")?;
        let rest = &obj[i + 13..];
        let a = rest.find('[')?;
        let mut out: Vec<String> = Vec::new();
        let (mut cur, mut in_str, mut esc) = (String::new(), false, false);
        for c in rest[a + 1..].chars() {
            if in_str {
                if esc { cur.push(c); esc = false } else if c == '\\' { esc = true } else if c == '"' { in_str = false; out.push(std::mem::take(&mut cur)) } else { cur.push(c) }
            } else if c == '"' { in_str = true } else if c == ']' { break }
        }
        if out.len() < 4 { return None; }
        Some([out[0].clone(), out[1].clone(), out[2].clone(), out[3].clone()])
    }
    let name = json_field(obj, "name")?;
    Some(Roster {
        id: 0,
        name,
        worm_names: names(obj)?,
        avatar_id: json_field(obj, "avatar_id").and_then(|s| s.parse().ok()).unwrap_or(0),
        headstone_id: json_field(obj, "headstone_id").and_then(|s| s.parse().ok()).unwrap_or(0),
        hat_ids: arr(obj, "hat_ids"),
        uniform_color_ids: arr(obj, "uniform_color_ids"),
        boot_color_ids: arr(obj, "boot_color_ids"),
        gun_style_ids: arr(obj, "gun_style_ids"),
    })
}

/// Load rosters from local cache — instant, no network.
pub fn load_cached_rosters() -> Vec<Roster> {
    #[cfg(feature = "desktop")]
    let json = std::fs::read_to_string(data_dir().join("rosters.txt")).unwrap_or_default();
    #[cfg(not(feature = "desktop"))]
    let json = std::fs::read_to_string("/mnt/SDCARD/App/Arty/rosters.txt")
        .or_else(|_| std::fs::read_to_string("/tmp/arty_rosters.txt"))
        .unwrap_or_default();
    if json.trim().is_empty() { return Vec::new(); }
    parse_rosters_from_json(&format!("{{\"rosters\":{}}}", json))
}

// ── Login / register ──────────────────────────────────────────────────────────

fn try_login(username: &str, password: &str) -> Result<(String, String, Vec<Roster>, bool), String> {
    let body = format!("{{\"username\":\"{}\",\"password\":\"{}\"}}", username, password);
    let resp = http_post("/api/login", &body).map_err(|e| {
        let _ = std::fs::write("/tmp/arty_login_err.txt", &e);
        format!("{}", &e[..e.len().min(50)])
    })?;
    if let Some(token) = json_field(&resp, "token") {
        let stored_name = json_field(&resp, "username").unwrap_or_else(|| username.to_string());
        let pw_reset = json_field(&resp, "password_reset").map(|v| v == "true").unwrap_or(false);
        if !pw_reset {
            save_creds(&stored_name, &token);
        }
        let rosters = parse_rosters_from_json(&resp);
        save_cached_rosters(&rosters);
        return Ok((token, stored_name, rosters, pw_reset));
    }
    if resp.contains("too many attempts") {
        return Err("TOO MANY ATTEMPTS, TRY AGAIN LATER".to_string());
    }
    Err("WRONG USERNAME OR PASSWORD".to_string())
}

fn try_register(username: &str, password: &str) -> Result<(String, String, Vec<Roster>, bool), String> {
    let body = format!("{{\"username\":\"{}\",\"password\":\"{}\"}}", username, password);
    let resp = http_post("/api/register", &body).map_err(|e| format!("NET: {}", &e[..e.len().min(28)]))?;
    if let Some(token) = json_field(&resp, "token") {
        save_creds(username, &token);
        let rosters = parse_rosters_from_json(&resp);
        save_cached_rosters(&rosters);
        return Ok((token, username.to_string(), rosters, false));
    }
    if resp.contains("username taken") {
        Err("USERNAME ALREADY TAKEN".to_string())
    } else if resp.contains("too many attempts") {
        Err("TOO MANY ATTEMPTS, TRY AGAIN LATER".to_string())
    } else {
        Err("REGISTRATION FAILED".to_string())
    }
}

fn try_change_password(token: &str, new_password: &str) -> Result<(), String> {
    let body = format!("{{\"token\":\"{}\",\"new_password\":\"{}\"}}", token, new_password);
    let resp = http_post("/api/change_password", &body).map_err(|e| format!("NET: {}", &e[..e.len().min(28)]))?;
    if resp.contains("\"ok\":true") {
        Ok(())
    } else {
        let msg = json_field(&resp, "error").unwrap_or_else(|| "failed".to_string());
        Err(msg.to_uppercase())
    }
}

// ── Cosmetics equip screen ────────────────────────────────────────────────────

pub enum CosmeticsAction { Saved(Roster), Back }

/// EQUIP: L1/R1 picks the soldier, LEFT/RIGHT the slot, UP/DOWN puts the next owned item on right away.
/// Only owned items are offered (buying is the STORE's job). START or B saves.
pub struct CosmeticsScreen {
    roster:         Roster,
    owned_hats:     Vec<u8>,
    owned_uniforms: Vec<u8>,
    owned_boots:    Vec<u8>,
    owned_guns:     Vec<u8>,
    soldier:        usize, // 0-3
    col:            usize, // 0=hat 1=uniform 2=boots 3=gun 4=look
    first:          usize, // first visible tile of the row
    scrap:          u32,
}

const EQUIP_SLOTS: [(&str, &str); 5] = [("HAT", "hat"), ("UNIFORM", "uniform"), ("BOOTS", "boots"), ("GUN", "gun_style"), ("LOOK", "look")];

impl CosmeticsScreen {
    pub fn new(
        roster:         Roster,
        owned_hats:     Vec<u8>,
        owned_guns:     Vec<u8>,
        owned_uniforms: Vec<u8>,
        owned_boots:    Vec<u8>,
        _token:         String,
        scrap:          u32,
    ) -> Self {
        let mut s = Self { roster, owned_hats, owned_uniforms, owned_boots, owned_guns, soldier: 0, col: 0, first: 0, scrap };
        s.follow_worn();
        s
    }

    pub fn set_profile(&mut self, scrap: u32, hats: Vec<u8>, guns: Vec<u8>, uniforms: Vec<u8>, boots: Vec<u8>) {
        self.scrap = scrap;
        self.owned_hats = hats;
        self.owned_guns = guns;
        self.owned_uniforms = uniforms;
        self.owned_boots = boots;
        self.follow_worn();
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer) -> Option<CosmeticsAction> {
        if input.just_pressed(Button::B) || input.just_pressed(Button::Start) {
            return Some(CosmeticsAction::Saved(self.roster.clone()));
        }
        let mut moved = false;
        if input.just_pressed(Button::L1) { self.soldier = (self.soldier + 3) % 4; moved = true; }
        if input.just_pressed(Button::R1) { self.soldier = (self.soldier + 1) % 4; moved = true; }
        if input.just_pressed(Button::Left)  { self.col = (self.col + 4) % 5; moved = true; }
        if input.just_pressed(Button::Right) { self.col = (self.col + 1) % 5; moved = true; }
        if moved { self.first = 0; }
        if input.just_pressed(Button::Up)   { self.step(-1); }
        if input.just_pressed(Button::Down) { self.step(1); }
        self.follow_worn();
        self.draw(buf);
        None
    }

    /// Choices for a slot: none/standard first, then everything owned (catalog order, then extras like
    /// warbond hats and exclusives). LOOK is free: all looks.
    fn options(&self, col: usize) -> Vec<u8> {
        if col == 4 { return (0..crate::renderer::soldier_looks::N_LOOKS as u8).collect(); }
        let cat = EQUIP_SLOTS[col].1;
        let owned = self.owned_for_col(col);
        let mut opts = vec![0u8];
        for &(ct, cid, _, _) in crate::game::store::CATALOG {
            if ct == cat && owned.contains(&cid) { opts.push(cid); }
        }
        // Only ids that have art: an owned list can carry ids of other kinds or from newer servers.
        let max = match col { 0 => crate::renderer::soldier_looks::N_HATS, 1 => 8, 2 => 6, _ => crate::renderer::soldier_looks::N_GUNS } as u8;
        let mut extra: Vec<u8> = owned.iter().copied().filter(|id| *id != 0 && *id < max && !opts.contains(id)).collect();
        extra.sort_unstable();
        extra.dedup();
        opts.extend(extra);
        // Something worn but not (or no longer) owned still shows, so the cursor has a place.
        let cur = self.current_id(col, self.soldier);
        if !opts.contains(&cur) { opts.push(cur); }
        opts
    }

    fn step(&mut self, dir: i32) {
        let (col, si) = (self.col, self.soldier);
        let opts = self.options(col);
        let pos = opts.iter().position(|&x| x == self.current_id(col, si)).unwrap_or(0) as i32;
        let next = (pos + dir).clamp(0, opts.len() as i32 - 1) as usize;
        self.set_id(col, si, opts[next]);
    }

    /// Keep the worn item on screen.
    fn follow_worn(&mut self) {
        let opts = self.options(self.col);
        let pos = opts.iter().position(|&x| x == self.current_id(self.col, self.soldier)).unwrap_or(0);
        self.first = crate::game::shop_ui::scroll_to(self.first, pos, opts.len());
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
            1 => self.roster.uniform_color_ids[si] & 0x1f,
            2 => self.roster.boot_color_ids[si],
            3 => self.roster.gun_style_ids[si],
            4 => self.roster.uniform_color_ids[si] >> 5,
            _ => 0,
        }
    }

    fn set_id(&mut self, col: usize, si: usize, id: u8) {
        match col {
            0 => self.roster.hat_ids[si] = id,
            1 => self.roster.uniform_color_ids[si] = (self.roster.uniform_color_ids[si] & 0xe0) | (id & 0x1f),
            2 => self.roster.boot_color_ids[si] = id,
            3 => self.roster.gun_style_ids[si] = id,
            4 => self.roster.uniform_color_ids[si] = (self.roster.uniform_color_ids[si] & 0x1f) | ((id % crate::renderer::soldier_looks::N_LOOKS as u8) << 5),
            _ => {}
        }
    }

    /// Preview / test hook: jump to a soldier and slot, scrolled so tile `first` is the first shown.
    pub fn set_view(&mut self, soldier: usize, col: usize, first: usize) {
        self.soldier = soldier % 4;
        self.col = col % 5;
        self.first = first.min(self.options(self.col).len().saturating_sub(crate::game::shop_ui::VISIBLE));
    }

    /// Number of tiles in a slot's row for the current soldier.
    pub fn slot_len(&self, col: usize) -> usize { self.options(col).len() }

    pub fn draw(&self, buf: &mut WorldBuffer) {
        use crate::game::shop_ui::{self as ui, Tile, ShopView};
        let (col, si) = (self.col, self.soldier);
        let cat = EQUIP_SLOTS[col].1;
        let worn = self.current_id(col, si);
        let opts = self.options(col);
        let tiles: Vec<Tile> = opts.iter().map(|&id| Tile {
            cat, id, name: crate::game::store::cosmetic_name(cat, id),
            tag: if id == worn { "WEARING".into() } else { String::new() }, tag_col: ui::GREEN,
        }).collect();
        let soldiers = ui::soldiers_of(&self.roster);
        let info = format!("{}   SCRAP {}", self.roster.name.to_uppercase(), self.scrap);
        let tabs: Vec<&str> = EQUIP_SLOTS.iter().map(|e| e.0).collect();
        ui::draw(buf, &ShopView {
            title: "EQUIP", info: &info,
            hints: &[("L1/R1", "SOLDIER"), ("LEFT/RIGHT", "SLOT"), ("UP/DOWN", "CHANGE"), ("START", "SAVE"), ("B", "BACK")],
            soldiers: &soldiers, cur: si, tabs: &tabs, tab: col, tiles: &tiles,
            first: self.first, sel: opts.iter().position(|&x| x == worn), what: if col == 4 { "LOOKS" } else { "OWNED" },
            status: None,
        });
    }
}

/// Same at an integer zoom (1 = native 24x32).
pub fn draw_worn_icon_z(buf: &mut WorldBuffer, look: usize, cx: i32, foot_y: i32, body: Bgra, hat: u8, boot: u8, gun: u8, z: i32) {
    use crate::renderer::soldier_looks as L;
    let hat = if (hat as usize) < L::N_HATS { hat as usize } else { 0 };
    let gun = if (gun as usize) < L::N_GUNS { gun as usize } else { 0 };
    let variant = (if hat != 0 { 2 } else { 0 }) + (if gun != 0 { 1 } else { 0 });
    let sheet = &L::DATA[look.min(L::N_LOOKS - 1)][variant][0];
    let pal = crate::renderer::skeleton::look_palette(body, boot);
    for y in 0..L::H {
        for x in 0..L::W {
            let i = y * L::W + x;
            let b = sheet[i];
            let mut c = b;
            if hat != 0 { let h = L::HATS[hat][i]; if h != 0 && !(h == L::OUT_IDX && b != 0) { c = h; } }
            if gun != 0 { let g = L::GUNS[gun][i]; if g != 0 && !(g == L::OUT_IDX && b != 0) { c = g; } }
            if c == 0 { continue; }
            let col = pal[c as usize];
            for oy in 0..z {
                for ox in 0..z {
                    buf.set_pixel(cx - z * L::W as i32 / 2 + z * x as i32 + ox, foot_y - z * (L::H as i32 - 1) + z * y as i32 + oy, col);
                }
            }
        }
    }
}

/// Painted bounds of a worn icon in sprite pixels: (left, top, right, bottom), inclusive. Hats and guns
/// stick out past the body, so callers that must keep the whole soldier inside a box measure first.
pub fn worn_icon_bounds(look: usize, hat: u8, gun: u8) -> (i32, i32, i32, i32) {
    use crate::renderer::soldier_looks as L;
    let hat = if (hat as usize) < L::N_HATS { hat as usize } else { 0 };
    let gun = if (gun as usize) < L::N_GUNS { gun as usize } else { 0 };
    let variant = (if hat != 0 { 2 } else { 0 }) + (if gun != 0 { 1 } else { 0 });
    let sheet = &L::DATA[look.min(L::N_LOOKS - 1)][variant][0];
    let (mut l, mut t, mut r, mut b) = (L::W as i32, L::H as i32, -1, -1);
    for y in 0..L::H {
        for x in 0..L::W {
            let i = y * L::W + x;
            let on = sheet[i] != 0 || (hat != 0 && L::HATS[hat][i] != 0) || (gun != 0 && L::GUNS[gun][i] != 0);
            if on { l = l.min(x as i32); r = r.max(x as i32); t = t.min(y as i32); b = b.max(y as i32); }
        }
    }
    if r < 0 { (0, 0, L::W as i32 - 1, L::H as i32 - 1) } else { (l, t, r, b) }
}

/// Union of worn_icon_bounds over all looks, hats and guns (checked by a test).
pub const WORN_UNION: (i32, i32, i32, i32) = (1, 0, 23, 31);

/// Draw a worn icon as big as possible (up to `max_z`) INSIDE the box (x, y, w, h): never cut off, hat
/// included. Feet rest on the box's bottom edge, the painted part is centred horizontally. Returns the zoom.
pub fn draw_worn_icon_fit(buf: &mut WorldBuffer, look: usize, body: Bgra, hat: u8, boot: u8, gun: u8,
                          x: i32, y: i32, w: i32, h: i32, max_z: i32) -> i32 {
    use crate::renderer::soldier_looks as L;
    // Size and place by the bounds of EVERY hat/gun/look combined (not this item's), so every soldier
    // in the same box is drawn at the same zoom and stands in the same spot whatever he wears.
    let _ = (hat, gun);
    let (l, b) = (WORN_UNION.0, WORN_UNION.3);
    let (pw, ph) = (WORN_UNION.2 - WORN_UNION.0 + 1, WORN_UNION.3 - WORN_UNION.1 + 1);
    let mut z = max_z.max(1);
    while z > 1 && (pw * z > w || ph * z > h) { z -= 1; }
    // draw_worn_icon_z paints sprite pixel (sx, sy) as the z*z block starting at
    // (cx - z*W/2 + z*sx, foot_y - z*(H-1) + z*sy). Place the painted bounds exactly:
    // left edge centred in the box, bottom edge on the box's last row.
    let left = x + (w - pw * z) / 2;
    let cx = left + z * L::W as i32 / 2 - z * l;
    let foot_y = (y + h - z) + z * (L::H as i32 - 1 - b);
    draw_worn_icon_z(buf, look, cx, foot_y, body, hat, boot, gun, z);
    crate::renderer::font::layout_note_picture(left, y + h - ph * z, pw * z, ph * z);
    z
}

#[cfg(test)]
mod worn_fit_tests {
    use super::*;
    /// Every hat, every look: the fitted icon never paints outside its box.
    #[test]
    fn worn_union_covers_every_combination() {
        use crate::renderer::soldier_looks as L;
        let (mut l, mut t, mut r, mut b) = (99, 99, -1, -1);
        for look in 0..L::N_LOOKS { for h in 0..L::N_HATS as u8 { for g in 0..L::N_GUNS as u8 {
            let (a, c, d, e) = worn_icon_bounds(look, h, g);
            l = l.min(a); t = t.min(c); r = r.max(d); b = b.max(e);
        }}}
        assert_eq!((l, t, r, b), WORN_UNION);
    }

    #[test]
    fn fitted_icon_stays_inside_its_box() {
        use crate::renderer::soldier_looks as L;
        let (bx, by, bw, bh) = (100, 100, 140, 96);
        for look in 0..L::N_LOOKS {
            for hat in 0..L::N_HATS as u8 {
                let mut buf = WorldBuffer::new();
                draw_worn_icon_fit(&mut buf, look, Bgra::new(200, 50, 50), hat, 0, 17, bx, by, bw, bh, 4);
                for yy in 60..240 { for xx in 40..320 {
                    let inside = xx >= bx && xx < bx + bw && yy >= by && yy < by + bh;
                    let p = buf.get_pixel(xx, yy);
                    if !inside { assert!(p.r == 0 && p.g == 0 && p.b == 0, "look {look} hat {hat} painted outside at ({xx},{yy})"); }
                }}
            }
        }
    }
}

#[cfg(test)]
mod match_roster_json_tests {
    use super::*;
    #[test]
    fn roster_json_round_trips_and_is_found_inside_a_state_reply() {
        let mut r = Roster::default_named(0);
        r.name = "FRAGS".into();
        r.worm_names = ["GREB".into(), "THL\"ONK".into(), "GIBS".into(), "SMEG".into()];
        r.avatar_id = 3; r.headstone_id = 2;
        r.hat_ids = [36, 22, 34, 33]; r.uniform_color_ids = [4, 2, 1, 3]; r.boot_color_ids = [3, 4, 2, 3]; r.gun_style_ids = [17, 18, 21, 14];
        let state = format!(r#"{{"seed": 5, "my_roster": {}, "opponent_roster": null, "hat_ids": [9,9,9,9]}}"#, roster_json(&r));
        let obj = json_object(&state, "my_roster").expect("object");
        let back = roster_from_json(obj).expect("parse");
        assert_eq!(back.name, "FRAGS");
        assert_eq!(back.worm_names[1], "THL\"ONK");
        assert_eq!(back.hat_ids, [36, 22, 34, 33]);
        assert_eq!(back.gun_style_ids, [17, 18, 21, 14]);
        assert_eq!((back.avatar_id, back.headstone_id), (3, 2));
        assert!(json_object(&state, "opponent_roster").is_none());
    }
}
