use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str, draw_str_shadow, draw_str_scaled, str_width, str_width_scaled};
use crate::renderer::keyboard::Keyboard;
use crate::world::{SCREEN_W, SCREEN_H};
use crate::game::account::{http_post, http_get, json_field};

pub enum LobbyAction {
    StartMatch { match_id: i64, seed: u64, my_slot: usize, moves: Vec<Move>, my_elo: i32, opp_elo: i32, has_mines: bool, has_barrels: bool, opp_name: String, opp_worm_names: [String; 4], opp_hat_ids: [u8; 4], opp_uniform_color_ids: [u8; 4], opp_boot_color_ids: [u8; 4], opp_gun_style_ids: [u8; 4], days_remaining: i32 },
    Back,
    LoggedOut,
}

enum LoadResult {
    Matches(Vec<PendingMatch>),
    MatchState(Option<LobbyAction>),
    Err(String),
}

#[derive(Clone)]
pub struct Move {
    pub angle:         f32,
    pub power:         f32,
    pub facing:        i8,
    pub active_soldier: usize,
    pub inputs:        Vec<u16>,
}

struct PendingMatch {
    match_id:      i64,
    code:          String,
    opponent:      String,
    your_turn:     bool,
    ranked:        bool,
    opp_elo:       i32,
    /// Days remaining before the current player forfeits. -1 = no limit (old match).
    days_remaining: i32,
}

enum Screen {
    Menu,
    Join(Keyboard),
    Pending(Vec<PendingMatch>, usize, usize),  // matches, cursor, scroll_offset
    Loading(String),
    /// Waiting for auto-matchmaker to find an opponent
    Searching {
        rx:     std::sync::mpsc::Receiver<Option<i64>>,
        ranked: bool,
    },
}

pub struct LobbyScreen {
    screen:    Screen,
    token:     String,
    username:  String,
    cursor:    usize,
    error:     String,
    version:   &'static str,
    load_rx:   Option<std::sync::mpsc::Receiver<LoadResult>>,
    check_rx:  Option<std::sync::mpsc::Receiver<bool>>,
    has_ready: bool,
    ranked:    bool,   // true = ranked lobby
    my_elo:    i32,
    elo_rx:    Option<std::sync::mpsc::Receiver<i32>>,
}

const MENU_ITEMS_CASUAL: &[&str] = &["MATCHMAKE", "GET CODE", "JOIN MATCH", "MY MATCHES", "LOG OUT"];
const MENU_ITEMS_RANKED: &[&str] = &["FIND RANKED MATCH", "MY MATCHES", "LOG OUT"];
// Alias used elsewhere in the file
const MENU_ITEMS: &[&str] = MENU_ITEMS_CASUAL;

impl LobbyScreen {
    pub fn new(token: String, username: String, version: &'static str) -> Self {
        Self::new_ranked(token, username, version, false)
    }

    pub fn new_ranked(token: String, username: String, version: &'static str, ranked: bool) -> Self {
        let check_rx = Self::start_turn_check(&token);
        // Fetch user ELO from profile endpoint in background (ranked lobby only)
        let elo_rx = if ranked {
            let tok = token.clone();
            let (tx, rx) = std::sync::mpsc::channel::<i32>();
            std::thread::spawn(move || {
                let path = format!("/api/profile?token={}", tok);
                if let Ok(resp) = http_get(&path) {
                    let elo: i32 = json_field(&resp, "elo").and_then(|s| s.parse().ok()).unwrap_or(0);
                    let _ = tx.send(elo);
                }
            });
            Some(rx)
        } else { None };
        Self {
            screen: Screen::Menu,
            token, username,
            cursor: 0,
            error: String::new(),
            version,
            load_rx: None,
            check_rx: Some(check_rx),
            ranked,
            has_ready: false,
            my_elo: 0,
            elo_rx,
        }
    }

    fn start_turn_check(token: &str) -> std::sync::mpsc::Receiver<bool> {
        let token = token.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let path = format!("/api/matches/pending?token={}", token);
            let ready = match http_get(&path) {
                Ok(resp) => parse_matches(&resp).iter().any(|m| m.your_turn),
                Err(_)   => false,
            };
            let _ = tx.send(ready);
        });
        rx
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer, cam_x: i32) -> Option<LobbyAction> {
        // ── Background ELO fetch ─────────────────────────────────────────────
        if let Some(ref rx) = self.elo_rx {
            if let Ok(elo) = rx.try_recv() { self.my_elo = elo; self.elo_rx = None; }
        }

        // ── Background turn-readiness check ───────────────────────────────────
        if let Some(rx) = &self.check_rx {
            if let Ok(ready) = rx.try_recv() {
                self.has_ready = ready;
                self.check_rx = None;
            }
        }

        // ── Async load result check ───────────────────────────────────────────
        if self.load_rx.is_some() {
            let result = self.load_rx.as_ref().unwrap().try_recv();
            match result {
                Ok(LoadResult::Matches(m)) => {
                    self.load_rx = None;
                    self.has_ready = m.iter().any(|m| m.your_turn);
                    self.screen = Screen::Pending(m, 0, 0);
                }
                Ok(LoadResult::MatchState(action)) => {
                    self.load_rx = None;
                    if let Some(action) = action {
                        self.draw(buf, cam_x);
                        return Some(action);
                    } else {
                        self.error = "FAILED TO LOAD MATCH".to_string();
                        self.screen = Screen::Menu;
                    }
                }
                Ok(LoadResult::Err(e)) => {
                    self.load_rx = None;
                    self.error = e;
                    self.screen = Screen::Menu;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Still loading — just draw and wait
                    self.draw(buf, cam_x);
                    return None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.load_rx = None;
                    self.error = "NETWORK ERROR".to_string();
                    self.screen = Screen::Menu;
                }
            }
        }

        match &mut self.screen {
            Screen::Menu => {
                let menu = if self.ranked { MENU_ITEMS_RANKED } else { MENU_ITEMS_CASUAL };
                let n = menu.len();
                if input.just_pressed(Button::Up)   { self.cursor = if self.cursor == 0 { n - 1 } else { self.cursor - 1 }; }
                if input.just_pressed(Button::Down) { self.cursor = (self.cursor + 1) % n; }
                if input.just_pressed(Button::B) || input.just_pressed(Button::Start) { return Some(LobbyAction::Back); }
                if input.just_pressed(Button::A) {
                    // Ranked menu: [0=FIND RANKED MATCH, 1=MY MATCHES, 2=LOG OUT]
                    // Casual menu: [0=MATCHMAKE, 1=GET CODE, 2=JOIN MATCH, 3=MY MATCHES, 4=LOG OUT]
                    let action_idx = self.cursor;
                    match (self.ranked, action_idx) {
                        // ── RANKED LOBBY ────────────────────────────────────────────
                        (true, 0) => { // FIND RANKED MATCH
                            let body = format!(r#"{{"token":"{}","ranked":true}}"#, self.token);
                            self.enter_matchmaking_pool(body, true);
                        }
                        // ── CASUAL LOBBY ─────────────────────────────────────────────
                        (false, 0) => { // MATCHMAKE — auto-pair from pool
                            let body = format!(r#"{{"token":"{}","ranked":false}}"#, self.token);
                            self.enter_matchmaking_pool(body, false);
                        }
                        (false, 1) => { // GET CODE — always create shareable code
                            let body = format!(r#"{{"token":"{}","ranked":false,"force_code":true}}"#, self.token);
                            self.enter_matchmaking_pool(body, false);
                        }
                        (false, 2) => { self.screen = Screen::Join(Keyboard::new(6)); }
                        // ── SHARED ───────────────────────────────────────────────────
                        (true, 1) | (false, 3) => { // MY MATCHES
                            self.start_load_matches();
                        }
                        _ => { crate::game::account::save_creds("", ""); return Some(LobbyAction::LoggedOut); }
                    }
                }
            }
            Screen::Join(kb) => {
                if input.just_pressed(Button::B) && kb.text.is_empty() { self.screen = Screen::Menu; return None; }
                if kb.update(input) {
                    let code = kb.text.clone().to_uppercase();
                    let body = format!(r#"{{"token":"{}","code":"{}"}}"#, self.token, code);
                    match http_post("/api/match/join", &body) {
                        Ok(resp) => {
                            if let Some(mid_str) = json_field(&resp, "match_id") {
                                let mid: i64 = mid_str.parse().unwrap_or(0);
                                return self.load_match_state_async(mid);
                            } else {
                                self.error = json_field(&resp, "error").unwrap_or("FAILED".to_string());
                                self.screen = Screen::Menu;
                            }
                        }
                        Err(_) => { self.error = "NETWORK ERROR".to_string(); self.screen = Screen::Menu; }
                    }
                }
            }
            Screen::Pending(matches, cursor, scroll_offset) => {
                const VISIBLE: usize = 8;
                let len = matches.len();
                if input.just_pressed(Button::Up) {
                    *cursor = if *cursor == 0 { len.saturating_sub(1) } else { *cursor - 1 };
                    if *cursor < *scroll_offset { *scroll_offset = *cursor; }
                    if len > 0 && *cursor == len - 1 { *scroll_offset = len.saturating_sub(VISIBLE); }
                }
                if input.just_pressed(Button::Down) {
                    *cursor = if len > 0 { (*cursor + 1) % len } else { 0 };
                    if *cursor >= *scroll_offset + VISIBLE { *scroll_offset = *cursor + 1 - VISIBLE; }
                    if *cursor == 0 { *scroll_offset = 0; }
                }
                if input.just_pressed(Button::B) || input.just_pressed(Button::Start) { self.screen = Screen::Menu; return None; }
                if input.just_pressed(Button::A) && !matches.is_empty() {
                    let your_turn = matches[*cursor].your_turn;
                    let mid = matches[*cursor].match_id;
                    if your_turn {
                        return self.load_match_state_async(mid);
                    } else {
                        self.error = "NOT YOUR TURN YET".to_string();
                    }
                }
                if input.just_pressed(Button::Select) { self.start_load_matches(); }
            }
            Screen::Loading(_) => {
                if input.just_pressed(Button::B) || input.just_pressed(Button::Start) {
                    self.load_rx = None;
                    self.screen = Screen::Menu;
                    return None;
                }
                if input.just_pressed(Button::A) && self.load_rx.is_none() {
                    self.start_load_matches();
                }
            }
            Screen::Searching { rx, ranked } => {
                // B = cancel matchmaking
                if input.just_pressed(Button::B) || input.just_pressed(Button::Start) {
                    let token = self.token.clone();
                    let is_ranked = *ranked;
                    std::thread::spawn(move || {
                        let endpoint = if is_ranked { "/api/ranked/tat/cancel" } else { "/api/casual/tat/cancel" };
                        let body = format!(r#"{{"token":"{}"}}"#, token);
                        http_post(endpoint, &body).ok();
                    });
                    self.screen = Screen::Menu;
                    return None;
                }
                // Poll for match
                match rx.try_recv() {
                    Ok(Some(match_id)) => {
                        // Found — load match state
                        return self.load_match_state_async(match_id);
                    }
                    Ok(None) => {
                        // Timed out
                        self.error = "NO OPPONENT FOUND".to_string();
                        self.screen = Screen::Menu;
                    }
                    Err(_) => {} // still searching
                }
            }
        }
        self.draw(buf, cam_x);
        None
    }

    fn start_load_matches(&mut self) {
        let token = self.token.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let path = format!("/api/matches/pending?token={}", token);
            let result = match http_get(&path) {
                Ok(resp) => LoadResult::Matches(parse_matches(&resp)),
                Err(e)   => LoadResult::Err(format!("NET: {}", e.chars().take(20).collect::<String>())),
            };
            let _ = tx.send(result);
        });
        self.load_rx = Some(rx);
        self.screen = Screen::Loading("LOADING MATCHES...".to_string());
    }

    fn enter_matchmaking_pool(&mut self, body: String, ranked: bool) {
        match http_post("/api/match/create", &body) {
            Ok(resp) if resp.contains("invalid token") => {
                crate::game::account::save_creds("", "");
                self.error = "SESSION EXPIRED - LOG IN AGAIN".to_string();
            }
            Ok(resp) if resp.contains("\"searching\": true") || resp.contains("\"searching\":true") => {
                // Added to pool — poll for match
                let token = self.token.clone();
                let (tx, rx) = std::sync::mpsc::channel::<Option<i64>>();
                std::thread::spawn(move || {
                    let status_path = if ranked {
                        format!("/api/ranked/tat/status?token={}", token)
                    } else {
                        format!("/api/casual/tat/status?token={}", token)
                    };
                    for _ in 0..240 { // poll up to 4 minutes
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                        if let Ok(r) = http_get(&status_path) {
                            if r.contains("\"matched\": true") || r.contains("\"matched\":true") {
                                let mid = json_field(&r, "match_id").and_then(|s| s.parse().ok());
                                let _ = tx.send(mid);
                                return;
                            }
                        }
                    }
                    let _ = tx.send(None);
                });
                self.error = String::new();
                self.screen = Screen::Searching { rx, ranked };
            }
            Ok(resp) => {
                // Paired immediately (match_id returned) or casual code-share
                if let Some(mid_str) = json_field(&resp, "match_id") {
                    if let Ok(mid) = mid_str.parse::<i64>() {
                        if let Some(action) = self.load_match_state_async(mid) {
                            // store for return — can't return here directly, store in screen
                            let _ = action; // handled by load_match_state_async setting load_rx
                        }
                        return;
                    }
                }
                let code = json_field(&resp, "code").unwrap_or_default();
                self.error = String::new();
                self.screen = Screen::Loading(format!("CODE: {}  WAITING...", code));
            }
            Err(_) => self.error = "NETWORK ERROR".to_string(),
        }
    }

    fn load_match_state_async(&mut self, mid: i64) -> Option<LobbyAction> {
        let token = self.token.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let path = format!("/api/match/{}/state?token={}", mid, token);
            let result = match http_get(&path) {
                Ok(resp) => {
                    let seed: u64 = json_field(&resp, "seed").and_then(|s| s.parse().ok()).unwrap_or(0);
                    let my_slot: usize = json_field(&resp, "my_slot").and_then(|s| s.parse().ok()).unwrap_or(0);
                    let turn: usize = json_field(&resp, "turn").and_then(|s| s.parse().ok()).unwrap_or(0);
                    if turn != my_slot {
                        LoadResult::MatchState(None)
                    } else {
                        let moves = parse_moves(&resp);
                        let is_ranked: bool = json_field(&resp, "ranked").map(|s| s == "true" || s == "1").unwrap_or(false);
                        // Only carry ELO for ranked matches; casual shows no ELO anywhere
                        let my_elo:  i32 = if is_ranked { json_field(&resp, "my_elo").and_then(|s| s.parse().ok()).unwrap_or(0) } else { 0 };
                        let opp_elo: i32 = if is_ranked { json_field(&resp, "opponent_elo").and_then(|s| s.parse().ok()).unwrap_or(0) } else { 0 };
                        let has_mines: bool = json_field(&resp, "has_mines").map(|s| s == "true" || s == "1").unwrap_or(false);
                        let opp_name: String = json_field(&resp, "opponent").unwrap_or_default();
                        let days_remaining: i32 = json_field(&resp, "days_remaining").and_then(|s| s.parse().ok()).unwrap_or(-1);
                        let has_barrels: bool = json_field(&resp, "has_barrels").map(|s| s == "true" || s == "1").unwrap_or(false);
                        let opp_worm_names        = parse_worm_names(&resp, "opponent_worm_names");
                        let opp_hat_ids            = parse_u8_arr(&resp, "opponent_hat_ids");
                        let opp_uniform_color_ids  = parse_u8_arr(&resp, "opponent_uniform_color_ids");
                        let opp_boot_color_ids     = parse_u8_arr(&resp, "opponent_boot_color_ids");
                        let opp_gun_style_ids      = parse_u8_arr(&resp, "opponent_gun_style_ids");
                        LoadResult::MatchState(Some(LobbyAction::StartMatch { match_id: mid, seed, my_slot, moves, my_elo, opp_elo, has_mines, has_barrels, opp_name, opp_worm_names, opp_hat_ids, opp_uniform_color_ids, opp_boot_color_ids, opp_gun_style_ids, days_remaining }))
                    }
                }
                Err(e) => LoadResult::Err(format!("NET: {}", e.chars().take(20).collect::<String>())),
            };
            let _ = tx.send(result);
        });
        self.load_rx = Some(rx);
        self.screen = Screen::Loading("LOADING MATCH...".to_string());
        None
    }

    fn draw(&self, buf: &mut WorldBuffer, cam_x: i32) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        // Background
        buf.fill_rect(cam_x, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(8, 10, 22));
        // Header bar
        buf.fill_rect(cam_x, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
        let title = "TAKE A TURN";
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, cam_x + sw/2 - tw/2, 10, Bgra::new(255, 210, 50), 2);
        draw_str(buf, &self.username, cam_x + sw - str_width(&self.username) as i32 - 8, 34, Bgra::new(140, 140, 180));
        if self.my_elo > 0 {
            let elo_str = format!("ELO {}", self.my_elo);
            draw_str(buf, &elo_str, cam_x + sw - str_width(&elo_str) as i32 - 8, 46, Bgra::new(255, 210, 50));
        }
        draw_str(buf, self.version, cam_x + 8, 34, Bgra::new(55, 55, 80));
        if !self.error.is_empty() {
            let ew = str_width_scaled(&self.error, 2);
            draw_str_scaled(buf, &self.error, cam_x + sw/2 - ew/2, 52, Bgra::new(220, 60, 60), 2);
        }
        let hint_y = sh - 18;
        match &self.screen {
            Screen::Menu => {
                if self.has_ready {
                    buf.fill_rect(cam_x, 44, SCREEN_W, 28, Bgra::new(20, 60, 30));
                    let banner = "YOUR TURN IS READY";
                    let bw = str_width_scaled(banner, 2);
                    draw_str_scaled(buf, banner, cam_x + sw/2 - bw/2, 48, Bgra::new(80, 220, 100), 2);
                }
                let menu = if self.ranked { MENU_ITEMS_RANKED } else { MENU_ITEMS_CASUAL };
                let item_h = 44;
                let total = menu.len() as i32 * item_h;
                let start_y = if self.has_ready { (sh - total) / 2 + 20 } else { (sh - total) / 2 + 10 };
                for (i, &item) in menu.iter().enumerate() {
                    let y = start_y + i as i32 * item_h;
                    let selected = i == self.cursor;
                    if selected {
                        buf.fill_rect(cam_x + sw/2 - 140, y - 4, 280, 26, Bgra::new(40, 44, 90));
                    }
                    let col = if selected { Bgra::new(255, 220, 50) } else { Bgra::new(170, 170, 200) };
                    let iw = str_width_scaled(item, 2);
                    if selected {
                        draw_str_scaled(buf, ">", cam_x + sw/2 - iw/2 - 22, y, Bgra::new(255, 180, 0), 2);
                    }
                    draw_str_scaled(buf, item, cam_x + sw/2 - iw/2, y, col, 2);
                }
                let hint = "B=BACK";
                draw_str(buf, hint, cam_x + sw/2 - str_width(hint)/2, hint_y, Bgra::new(70, 70, 100));
            }
            Screen::Join(kb) => { kb.draw(buf, cam_x); }
            Screen::Pending(matches, cursor, scroll_offset) => {
                let list_top = 52;
                if matches.is_empty() {
                    let msg = "NO PENDING MATCHES";
                    let mw = str_width_scaled(msg, 2);
                    draw_str_scaled(buf, msg, cam_x + sw/2 - mw/2, sh/2 - 8, Bgra::new(120, 120, 150), 2);
                    draw_str(buf, "B=BACK  SELECT=REFRESH", cam_x + sw/2 - str_width("B=BACK  SELECT=REFRESH")/2, hint_y, Bgra::new(70, 70, 100));
                } else {
                    // Scrollbar (right edge) when list overflows
                    const VISIBLE: usize = 8;
                    if matches.len() > VISIBLE {
                        let track_h = (hint_y - list_top).max(1);
                        let pct = *scroll_offset as f32 / (matches.len() - VISIBLE) as f32;
                        let thumb_y = list_top + (track_h as f32 * pct) as i32;
                        buf.fill_rect(cam_x + sw - 5, list_top, 4, track_h as u32, Bgra::new(25, 25, 40));
                        buf.fill_rect(cam_x + sw - 5, thumb_y, 4, 20, Bgra::new(80, 80, 150));
                    }
                    for (i, m) in matches.iter().enumerate().skip(*scroll_offset) {
                        let display_row = (i - *scroll_offset) as i32;
                        let y = list_top + display_row * 46;
                        if y + 46 > hint_y { break; }
                        let selected = i == *cursor;  // i is absolute index
                        if selected {
                            buf.fill_rect(cam_x + 8, y - 4, (sw - 16) as u32, 38, Bgra::new(28, 32, 65));
                        }
                        let name_col = if selected { Bgra::new(255, 220, 50) } else { Bgra::new(200, 200, 220) };
                        let rank_tag = if m.ranked { "[R] " } else { "" };
                        let opp = format!("{}VS {}", rank_tag, m.opponent.to_uppercase());
                        draw_str_scaled(buf, &opp, cam_x + 20, y, name_col, 2);
                        if m.ranked && m.opp_elo > 0 {
                            let elo_str = format!("ELO {}", m.opp_elo);
                            draw_str(buf, &elo_str, cam_x + 20, y + 16, Bgra::new(180, 180, 100));
                        }
                        let code_str = format!("#{}", m.code);
                        draw_str(buf, &code_str, cam_x + sw - str_width(&code_str) - 16, y + 4, Bgra::new(100, 100, 140));
                        let (status, scol) = if m.your_turn {
                            ("* YOUR TURN *", Bgra::new(80, 220, 120))
                        } else {
                            ("  waiting...", Bgra::new(100, 100, 130))
                        };
                        draw_str(buf, status, cam_x + 20, y + 22, scol);
                        // Days remaining until forfeit (right side, warning colour when low)
                        if m.days_remaining >= 0 {
                            let days_str = if m.days_remaining <= 1 {
                                format!("{}d LEFT!", m.days_remaining)
                            } else {
                                format!("{}d", m.days_remaining)
                            };
                            let days_col = if m.days_remaining <= 3 {
                                Bgra::new(220, 80, 60)   // red — urgent
                            } else if m.days_remaining <= 7 {
                                Bgra::new(220, 180, 60)  // amber — warning
                            } else {
                                Bgra::new(100, 100, 140) // dim — plenty of time
                            };
                            draw_str(buf, &days_str, cam_x + sw - str_width(&days_str) - 16, y + 22, days_col);
                        }
                    }
                    let hint = "A=PLAY  B=BACK  SEL=REFRESH";
                    draw_str(buf, hint, cam_x + sw/2 - str_width(hint)/2, hint_y, Bgra::new(70, 70, 100));
                }
            }
            Screen::Loading(msg) => {
                let mw = str_width_scaled(msg, 2);
                draw_str_scaled(buf, msg, cam_x + sw/2 - mw/2, sh/2 - 8, Bgra::new(255, 210, 50), 2);
                let hint = "A=REFRESH  B=BACK";
                draw_str(buf, hint, cam_x + sw/2 - str_width(hint)/2, hint_y, Bgra::new(70, 70, 100));
            }
            Screen::Searching { ranked, .. } => {
                let msg = if *ranked { "FINDING RANKED MATCH..." } else { "FINDING OPPONENT..." };
                let mw = str_width_scaled(msg, 2);
                draw_str_scaled(buf, msg, cam_x + sw/2 - mw/2, sh/2 - 8, Bgra::new(80, 220, 120), 2);
                let hint = "B=CANCEL";
                draw_str(buf, hint, cam_x + sw/2 - str_width(hint)/2, hint_y, Bgra::new(70, 70, 100));
            }
        }
    }
}

fn parse_matches(json: &str) -> Vec<PendingMatch> {
    let mut out = Vec::new();
    let mut rest = json.trim();
    if rest.starts_with('[') { rest = &rest[1..]; }
    for obj in rest.split('{') {
        let obj = obj.trim().trim_end_matches('}').trim_end_matches(',');
        if obj.is_empty() { continue; }
        let obj = format!("{{{}}}", obj);
        let match_id: i64 = json_field(&obj, "match_id").and_then(|s| s.parse().ok()).unwrap_or(0);
        let code = json_field(&obj, "code").unwrap_or_default();
        let opponent = json_field(&obj, "opponent").unwrap_or_default();
        let your_turn = json_field(&obj, "your_turn").map(|s| s == "true").unwrap_or(false);
        let ranked    = json_field(&obj, "ranked").map(|s| s == "true" || s == "1").unwrap_or(false);
        let opp_elo: i32 = json_field(&obj, "opponent_elo").and_then(|s| s.parse().ok()).unwrap_or(0);
        let days_remaining: i32 = json_field(&obj, "days_remaining").and_then(|s| s.parse().ok()).unwrap_or(-1);
        if match_id > 0 { out.push(PendingMatch { match_id, code, opponent, your_turn, ranked, opp_elo, days_remaining }); }
    }
    out
}

fn parse_inputs(obj: &str) -> Vec<u16> {
    let mut out = Vec::new();
    if let Some(start) = obj.find("\"inputs\":")  {
        let rest = &obj[start + 9..];
        if let Some(arr_start) = rest.find('[') {
            let rest = &rest[arr_start + 1..];
            if let Some(arr_end) = rest.find(']') {
                for tok in rest[..arr_end].split(',') {
                    let tok = tok.trim();
                    if let Ok(v) = tok.parse::<u16>() { out.push(v); }
                }
            }
        }
    }
    out
}

fn parse_u8_arr(json: &str, key: &str) -> [u8; 4] {
    let search = format!("\"{}\":", key);
    let mut out = [0u8; 4];
    if let Some(start) = json.find(&search) {
        let rest = &json[start + search.len()..];
        if let Some(arr_start) = rest.find('[') {
            if let Some(arr_end) = rest.find(']') {
                let arr = &rest[arr_start + 1..arr_end];
                for (i, v) in arr.split(',').enumerate().take(4) {
                    out[i] = v.trim().parse().unwrap_or(0);
                }
            }
        }
    }
    out
}

fn parse_worm_names(json: &str, key: &str) -> [String; 4] {
    let search = format!("\"{}\":", key);
    let mut names = std::array::from_fn(|i| format!("Soldier {}", i + 1));
    if let Some(start) = json.find(&search) {
        let rest = &json[start + search.len()..];
        if let Some(arr_start) = rest.find('[') {
            if let Some(arr_end) = rest.find(']') {
                let arr = &rest[arr_start + 1..arr_end];
                let parsed: Vec<String> = arr.split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                for (i, n) in parsed.into_iter().enumerate().take(4) {
                    names[i] = n;
                }
            }
        }
    }
    names
}

fn parse_moves(json: &str) -> Vec<Move> {
    let mut out = Vec::new();
    if let Some(start) = json.find("\"moves\":") {
        let rest = &json[start + 8..];
        for obj in rest.split('{') {
            let obj = obj.trim().trim_end_matches('}').trim_end_matches(',');
            if obj.is_empty() { continue; }
            // Skip pieces that aren't move objects (e.g. the [] prefix or trailing JSON)
            if !obj.contains("\"angle\":") { continue; }
            let obj = format!("{{{}}}", obj);
            let angle: f32 = json_field(&obj, "angle").and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let power: f32 = json_field(&obj, "power").and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let facing: i8 = json_field(&obj, "facing").and_then(|s| s.parse().ok()).unwrap_or(1);
            let active_soldier: usize = json_field(&obj, "active_soldier").and_then(|s| s.parse().ok()).unwrap_or(0);
            let inputs: Vec<u16> = parse_inputs(&obj);
            out.push(Move { angle, power, facing, active_soldier, inputs });
        }
    }
    out
}
