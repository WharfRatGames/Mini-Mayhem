use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str, draw_str_shadow, draw_str_scaled, draw_str_shadow_scaled, str_width, str_width_scaled};
use crate::renderer::hud::COLOR_DARK_BG;
use crate::renderer::keyboard::Keyboard;
use crate::world::{SCREEN_W, SCREEN_H};
use crate::game::account::{http_post, http_get, json_field};

pub enum LobbyAction {
    StartMatch { match_id: i64, seed: u64, my_slot: usize, moves: Vec<Move>, my_elo: i32, opp_elo: i32, has_mines: bool, has_barrels: bool, opp_name: String, opp_worm_names: [String; 4], opp_hat_ids: [u8; 4], opp_uniform_color_ids: [u8; 4], opp_boot_color_ids: [u8; 4], opp_gun_style_ids: [u8; 4], days_remaining: i32,
                 /// The team this player picked for this game, as stored by the server (None: older game / client).
                 my_roster: Option<crate::game::account::Roster> },
    Back,
}

enum LoadResult {
    Matches(Vec<PendingMatch>),
    MatchState(Option<LobbyAction>),
    /// The game loaded fine but the opponent moves first (who pairs into a waiting player, or joins by code, is p1).
    NotYourTurn { opponent: String, ranked: bool, opp_elo: i32 },
    Err(String),
}

/// A new game was just paired / joined: shown big until the player moves on (MATCH FOUND screen).
struct Found {
    ranked:    bool,
    since:     std::time::Instant,
    opponent:  String,
    opp_elo:   i32,
    /// None while the game is still loading; Some(true) = A plays it now; Some(false) = their move first.
    your_turn: Option<bool>,
    action:    Option<LobbyAction>,
    team:      String,
}

#[derive(Clone)]
pub struct Move {
    pub angle:         f32,
    pub power:         f32,
    pub facing:        i8,
    pub active_soldier: usize,
    pub inputs:        Vec<u16>,
}

#[derive(Clone)]
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
    /// MATCH FOUND (see `Found`).
    Found,
    /// Choosing the team for a new game; `action` = the MENU_ITEMS index to run once a team is picked.
    PickTeam { picker: crate::game::account::RosterPicker, action: usize },
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
    check_rx:  Option<std::sync::mpsc::Receiver<Vec<PendingMatch>>>,
    has_ready: bool,
    /// The player's teams (for PICK TEAM) and the team picked for the game being started.
    rosters:   Vec<crate::game::account::Roster>,
    team:      Option<crate::game::account::Roster>,
    found:     Option<Found>,
    /// Last known match list (background check / MY MATCHES), for the info card.
    known:     Vec<PendingMatch>,
    my_elo:    i32,
    elo_rx:    Option<std::sync::mpsc::Receiver<i32>>,
}

/// One lobby for casual and ranked TAT. MY MATCHES lists both kinds (ranked
/// rows are tagged [R]); log in/out lives in MY TEAMS.
const MENU_ITEMS: &[&str] = &["MY MATCHES", "MATCHMAKE", "RANKED MATCH", "GET CODE", "JOIN MATCH"];

impl LobbyScreen {
    pub fn new(token: String, username: String, version: &'static str) -> Self {
        let check_rx = Self::start_turn_check(&token);
        // Fetch user ELO from profile endpoint in background (shown in the header)
        let elo_rx = {
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
        };
        Self {
            screen: Screen::Menu,
            token, username,
            cursor: 0,
            error: String::new(),
            version,
            load_rx: None,
            check_rx: Some(check_rx),
            has_ready: false,
            rosters: Vec::new(),
            team: None,
            found: None,
            known: Vec::new(),
            my_elo: 0,
            elo_rx,
        }
    }

    /// The player's teams, offered when starting a new game (one team: used without asking).
    pub fn with_rosters(mut self, rosters: Vec<crate::game::account::Roster>) -> Self { self.rosters = rosters; self }

    /// `"roster":{...},` for create/join bodies (empty when no team picked).
    fn roster_field(&self) -> String {
        self.team.as_ref().map(|r| format!(r#""roster":{},"#, crate::game::account::roster_json(r))).unwrap_or_default()
    }

    /// A new game now has an id: remember the picked team for it (locally, and on the server in case the
    /// create/join request could not carry it). Only for games started from this lobby, never MY MATCHES.
    fn claim_team(&mut self, mid: i64) {
        let Some(r) = self.team.take() else { return };
        if let Some(f) = self.found.as_mut() { f.team = r.name.to_uppercase(); }
        crate::game::account::save_match_roster(mid, &r);
        let body = format!(r#"{{"token":"{}","roster":{}}}"#, self.token, crate::game::account::roster_json(&r));
        std::thread::spawn(move || { let _ = http_post(&format!("/api/match/{}/roster", mid), &body); });
    }

    /// A new game was paired / joined: load it behind a big MATCH FOUND screen (with a beep), never straight
    /// into the game or an error. `mid` must already be claimed (claim_team).
    fn match_found(&mut self, mid: i64, ranked: bool) -> Option<LobbyAction> {
        let team = self.team.as_ref().map(|r| r.name.to_uppercase()).unwrap_or_default();
        self.found = Some(Found { ranked, since: std::time::Instant::now(), opponent: String::new(), opp_elo: 0,
                                  your_turn: None, action: None, team });
        self.claim_team(mid);
        crate::audio::play(crate::audio::Sfx::MineArm);
        let r = self.load_match_state_async(mid);
        self.screen = Screen::Found;
        r
    }

    /// Run a new-game menu action (1 MATCHMAKE, 2 RANKED MATCH, 3 GET CODE, 4 JOIN MATCH) with `self.team` set.
    fn start_action(&mut self, action: usize) {
        let rf = self.roster_field();
        match action {
            1 => { let body = format!(r#"{{{}"token":"{}","ranked":false}}"#, rf, self.token); self.enter_matchmaking_pool(body, false); }
            2 => { let body = format!(r#"{{{}"token":"{}","ranked":true}}"#, rf, self.token); self.enter_matchmaking_pool(body, true); }
            3 => { let body = format!(r#"{{{}"token":"{}","ranked":false,"force_code":true}}"#, rf, self.token); self.enter_matchmaking_pool(body, false); }
            _ => { self.screen = Screen::Join(Keyboard::new(6)); }
        }
    }

    /// Preview only (examples/menulook): a lobby showing MY MATCHES with made-up games, no network.
    /// Each game: (opponent, ranked, your_turn, days_left).
    #[doc(hidden)]
    pub fn preview_matches(username: &str, games: &[(&str, bool, bool, i32)], menu: bool) -> Self {
        let list: Vec<PendingMatch> = games.iter().enumerate().map(|(i, &(o, r, t, d))| PendingMatch {
            match_id: i as i64 + 1, code: format!("AB{:04}", i * 37), opponent: o.to_string(),
            your_turn: t, ranked: r, opp_elo: if r { 1100 } else { 0 }, days_remaining: d }).collect();
        Self {
            screen: if menu { Screen::Menu } else { Screen::Pending(list.clone(), 1, 0) },
            token: String::new(), username: username.to_string(), cursor: 0, error: String::new(),
            version: "", load_rx: None, check_rx: None, has_ready: true, known: list, my_elo: 1042, elo_rx: None,
            rosters: Vec::new(), team: None, found: None,
        }
    }

    /// Preview only: the MATCH FOUND screen.
    #[doc(hidden)]
    pub fn preview_found(ranked: bool, your_turn: bool) -> Self {
        let mut l = Self::preview_matches("DUSTY", &[], true);
        l.found = Some(Found { ranked, since: std::time::Instant::now(), opponent: "ROBOTNIK".into(), opp_elo: 1100,
                               your_turn: Some(your_turn), action: None, team: "FRAGS".into() });
        l.screen = Screen::Found;
        l
    }

    /// Draw without input (previews).
    #[doc(hidden)]
    pub fn preview_draw(&self, buf: &mut WorldBuffer) { self.draw(buf, 0); }

    fn start_turn_check(token: &str) -> std::sync::mpsc::Receiver<Vec<PendingMatch>> {
        let token = token.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let path = format!("/api/matches/pending?token={}", token);
            let list = http_get(&path).map(|r| parse_matches(&r)).unwrap_or_default();
            let _ = tx.send(list);
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
            if let Ok(list) = rx.try_recv() {
                self.has_ready = list.iter().any(|m| m.your_turn);
                self.known = list;
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
                    self.known = m.clone();
                    self.screen = Screen::Pending(m, 0, 0);
                }
                Ok(LoadResult::MatchState(Some(action))) if self.found.is_some() => {
                    self.load_rx = None;
                    let f = self.found.as_mut().unwrap();
                    if let LobbyAction::StartMatch { ref opp_name, opp_elo, .. } = action {
                        f.opponent = opp_name.to_uppercase();
                        f.opp_elo = opp_elo;
                    }
                    f.your_turn = Some(true);
                    f.action = Some(action);
                    self.screen = Screen::Found;
                }
                Ok(LoadResult::NotYourTurn { opponent, ranked, opp_elo }) => {
                    self.load_rx = None;
                    if let Some(f) = self.found.as_mut() {
                        f.opponent = opponent.to_uppercase();
                        f.ranked = ranked;
                        f.opp_elo = opp_elo;
                        f.your_turn = Some(false);
                        self.screen = Screen::Found;
                    } else {
                        self.error = "NOT YOUR TURN YET".to_string();
                        self.screen = Screen::Menu;
                    }
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
                    self.found = None;
                    self.error = e;
                    self.screen = Screen::Menu;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Still loading — just draw and wait
                    if self.found.is_some() { self.screen = Screen::Found; }
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
                let n = MENU_ITEMS.len();
                if input.just_pressed(Button::Up)   { self.cursor = if self.cursor == 0 { n - 1 } else { self.cursor - 1 }; }
                if input.just_pressed(Button::Down) { self.cursor = (self.cursor + 1) % n; }
                if input.just_pressed(Button::B) || input.just_pressed(Button::Start) { return Some(LobbyAction::Back); }
                if input.just_pressed(Button::A) {
                    match self.cursor {
                        0 => self.start_load_matches(),
                        // New game: pick the team first (kept for the whole game), then matchmake / code / join.
                        a if self.rosters.len() > 1 => {
                            self.error.clear();
                            self.screen = Screen::PickTeam {
                                picker: crate::game::account::RosterPicker::new_for_game(self.token.clone(), self.rosters.clone()),
                                action: a,
                            };
                        }
                        a => {
                            self.team = self.rosters.first().cloned();
                            self.start_action(a);
                        }
                    }
                }
            }
            Screen::Found => {
                let f = self.found.as_mut().unwrap();
                let settled = f.since.elapsed() >= std::time::Duration::from_millis(1200); // don't skip it by accident
                if settled && (input.just_pressed(Button::A) || input.just_pressed(Button::Start)) {
                    match f.your_turn {
                        Some(true) => { let a = f.action.take(); self.found = None; self.screen = Screen::Menu; return a; }
                        Some(false) => { self.found = None; self.screen = Screen::Menu; self.check_rx = Some(Self::start_turn_check(&self.token)); }
                        None => {}
                    }
                } else if settled && input.just_pressed(Button::B) && f.your_turn.is_some() {
                    // Leave it for later: it is in MY MATCHES.
                    self.found = None;
                    self.screen = Screen::Menu;
                    self.check_rx = Some(Self::start_turn_check(&self.token));
                }
            }
            Screen::PickTeam { picker, action } => {
                let action = *action;
                match picker.update(input, buf, cam_x) {
                    Some(crate::game::account::RosterAction::Selected(r)) => {
                        self.rosters = picker.rosters.clone(); // edits / new teams made in the picker
                        self.team = Some(r);
                        self.screen = Screen::Menu;
                        self.start_action(action);
                    }
                    Some(_) => { self.rosters = picker.rosters.clone(); self.screen = Screen::Menu; }
                    None => return None, // the picker drew itself
                }
            }
            Screen::Join(kb) => {
                if input.just_pressed(Button::B) && kb.text.is_empty() { self.screen = Screen::Menu; return None; }
                if kb.update(input) {
                    let code = kb.text.clone().to_uppercase();
                    let body = format!(r#"{{{}"token":"{}","code":"{}"}}"#, self.roster_field(), self.token, code);
                    match http_post("/api/match/join", &body) {
                        Ok(resp) => {
                            if let Some(mid_str) = json_field(&resp, "match_id") {
                                let mid: i64 = mid_str.parse().unwrap_or(0);
                                return self.match_found(mid, false);
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
                        // Found — MATCH FOUND screen while the game loads
                        let ranked = *ranked;
                        return self.match_found(match_id, ranked);
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
                        if let Some(action) = self.match_found(mid, ranked) {
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
                // An error reply ({"error": ...}) or a garbled one has no seed: never start a game from it. Before this,
                // the defaults (seed 0, turn 0 == slot 0) started a fresh game on seed 0, where all 8 soldiers spawn
                // in one row (user report, -exp44 log "generate map seed=0000000000000000").
                Ok(resp) if json_field(&resp, "seed").and_then(|s| s.parse::<u64>().ok()).filter(|&v| v != 0).is_none()
                         || json_field(&resp, "my_slot").is_none() || json_field(&resp, "turn").is_none() => {
                    let e = json_field(&resp, "error").unwrap_or_else(|| "BAD MATCH DATA".to_string());
                    crate::dlog!("tat", "match {} state rejected: {}", mid, resp.chars().take(120).collect::<String>());
                    LoadResult::Err(e.to_uppercase())
                }
                Ok(resp) => {
                    let seed: u64 = json_field(&resp, "seed").and_then(|s| s.parse().ok()).unwrap_or(0);
                    let my_slot: usize = json_field(&resp, "my_slot").and_then(|s| s.parse().ok()).unwrap_or(0);
                    let turn: usize = json_field(&resp, "turn").and_then(|s| s.parse().ok()).unwrap_or(0);
                    if turn != my_slot {
                        LoadResult::NotYourTurn {
                            opponent: json_field(&resp, "opponent").unwrap_or_default(),
                            ranked: json_field(&resp, "ranked").map(|s| s == "true" || s == "1").unwrap_or(false),
                            opp_elo: json_field(&resp, "opponent_elo").and_then(|s| s.parse().ok()).unwrap_or(0),
                        }
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
                        let my_roster = crate::game::account::json_object(&resp, "my_roster")
                            .and_then(crate::game::account::roster_from_json);
                        LoadResult::MatchState(Some(LobbyAction::StartMatch { match_id: mid, seed, my_slot, moves, my_elo, opp_elo, has_mines, has_barrels, opp_name, opp_worm_names, opp_hat_ids, opp_uniform_color_ids, opp_boot_color_ids, opp_gun_style_ids, days_remaining, my_roster }))
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
        use crate::renderer::hud::{side_frame, side_panel, side_row_bg, SIDE_GOLD, SIDE_GREEN, SIDE_WHITE, SIDE_DIM, SIDE_RED, SIDE_TOP, SIDE_BOTTOM};
        let sw = SCREEN_W as i32;
        let who = if self.my_elo > 0 {
            format!("{}  ELO {}", self.username.to_uppercase(), self.my_elo)
        } else { self.username.to_uppercase() };
        // A centred message panel (loading / searching / errors).
        let msg_panel = |buf: &mut WorldBuffer, msg: &str, col: Bgra| {
            let mw = str_width_scaled(msg, 2);
            let pw = (mw + 60).max(360);
            side_panel(buf, sw/2 - pw/2, 190, pw, 80);
            draw_str_shadow_scaled(buf, msg, sw/2 - mw/2, 222, col, 2);
        };
        match &self.screen {
            Screen::Menu => {
                // Same full-screen side layout as the title submenus.
                use crate::renderer::hud::{draw_side_menu, SideItem, SideCard, SIDE_GREEN, SIDE_WHITE};
                let ready_n = self.known.iter().filter(|m| m.your_turn).count();
                let badge = if ready_n > 0 { ready_n.to_string() } else { String::new() };
                let rows: Vec<SideItem> = MENU_ITEMS.iter().enumerate()
                    .map(|(i, &label)| SideItem { badge: if i == 0 { &badge } else { "" }, ..SideItem::new(label) })
                    .collect();
                const INFO: [&str; 5] = [
                    "Games in progress. R = ranked. A on one where it is your turn to play it.",
                    "Get paired with a random opponent. No ELO at stake.",
                    "Get paired with a player near your ELO. Wins and losses change your rank.",
                    "Make a casual game and get a code to give a friend.",
                    "Type a friend's code to join their game.",
                ];
                let mut stats: Vec<(String, String, Bgra)> = Vec::new();
                match self.cursor {
                    0 => {
                        // Your-turn games first
                        let mut ms: Vec<&PendingMatch> = self.known.iter().collect();
                        ms.sort_by_key(|m| (!m.your_turn, m.days_remaining));
                        for m in ms {
                            let name = format!("VS {}{}", m.opponent.to_uppercase(), if m.ranked { "  R" } else { "" });
                            let days = if m.days_remaining >= 0 { format!(" {}D", m.days_remaining) } else { String::new() };
                            let (v, c) = if m.your_turn { (format!("GO{}", days), SIDE_GREEN) } else { (format!("WAIT{}", days), SIDE_WHITE) };
                            stats.push((name, v, c));
                        }
                        if self.known.is_empty() && self.check_rx.is_none() {
                            stats.push(("NO GAMES YET".to_string(), "-".to_string(), SIDE_WHITE));
                        }
                    }
                    2 if self.my_elo > 0 => stats.push(("ELO".to_string(), self.my_elo.to_string(), SIDE_WHITE)),
                    _ => {}
                }
                let body = if self.error.is_empty() { INFO[self.cursor] } else { self.error.as_str() };
                let card = SideCard { title: MENU_ITEMS[self.cursor], body, stats: &stats };
                let who = if self.my_elo > 0 {
                    format!("{}  ELO {}", self.username.to_uppercase(), self.my_elo)
                } else { self.username.to_uppercase() };
                draw_side_menu(buf, "TAKE A TURN", &who, &rows, self.cursor, &card, &[("A", "SELECT"), ("B", "BACK")]);
            }
            Screen::PickTeam { .. } => {} // RosterPicker::update draws it
            Screen::Found => {
                let Some(f) = &self.found else { return };
                let hints: &[(&str, &str)] = match f.your_turn {
                    Some(true)  => &[("A", "PLAY NOW"), ("B", "LATER")],
                    Some(false) => &[("A", "OK"), ("B", "BACK")],
                    None        => &[],
                };
                side_frame(buf, if f.ranked { "RANKED MATCH" } else { "MATCHMAKE" }, &who, hints);
                let (x, w) = (40, sw - 80);
                side_panel(buf, x, SIDE_TOP + 10, w, SIDE_BOTTOM - SIDE_TOP - 20);
                // Big blinking headline
                let blink = (f.since.elapsed().as_millis() / 400) % 2 == 0;
                let head = "MATCH FOUND!";
                let hc = if blink { SIDE_GREEN } else { Bgra::new(160, 255, 180) };
                draw_str_shadow_scaled(buf, head, sw/2 - str_width_scaled(head, 4)/2, SIDE_TOP + 34, hc, 4);
                // Opponent
                let vs = if f.opponent.is_empty() { "LOADING...".to_string() } else { format!("VS {}", f.opponent) };
                let vsc = if str_width_scaled(&vs, 3) <= w - 30 { 3 } else { 2 };
                draw_str_shadow_scaled(buf, &vs, sw/2 - str_width_scaled(&vs, vsc)/2, SIDE_TOP + 110, SIDE_WHITE, vsc);
                // Ranked: tag + both ELOs on their own line; casual: tag
                let tag = if f.ranked { "RANKED" } else { "CASUAL - NO ELO" };
                draw_str_shadow_scaled(buf, tag, sw/2 - str_width_scaled(tag, 2)/2, SIDE_TOP + 158, SIDE_GOLD, 2);
                if f.ranked && f.opp_elo > 0 && self.my_elo > 0 {
                    let elo = format!("ELO {}  VS  {}", self.my_elo, f.opp_elo);
                    draw_str_shadow_scaled(buf, &elo, sw/2 - str_width_scaled(&elo, 2)/2, SIDE_TOP + 184, SIDE_GOLD, 2);
                }
                if !f.team.is_empty() {
                    let t = format!("YOUR TEAM: {}", f.team);
                    draw_str_shadow_scaled(buf, &t, sw/2 - str_width_scaled(&t, 2)/2, SIDE_TOP + 216, SIDE_DIM, 2);
                }
                let (line, sub, lc) = match f.your_turn {
                    Some(true)  => ("YOUR MOVE FIRST", "PRESS A TO PLAY NOW  -  B FOR LATER", SIDE_GREEN),
                    Some(false) => ("THEY MOVE FIRST", "YOUR TURN WILL SHOW IN MY MATCHES", SIDE_WHITE),
                    None        => ("LOADING THE GAME...", "", SIDE_DIM),
                };
                draw_str_shadow_scaled(buf, line, sw/2 - str_width_scaled(line, 3)/2, SIDE_TOP + 262, lc, 3);
                if !sub.is_empty() { draw_str(buf, sub, sw/2 - str_width(sub)/2, SIDE_TOP + 300, SIDE_DIM); }
            }
            Screen::Join(kb) => { kb.draw_screen(buf, "JOIN MATCH", "TYPE YOUR FRIEND'S 6-LETTER CODE"); }
            Screen::Pending(matches, cursor, scroll_offset) => {
                let ready_n = matches.iter().filter(|m| m.your_turn).count();
                let crumb = "MY MATCHES";
                if matches.is_empty() {
                    side_frame(buf, crumb, &who, &[("SELECT", "REFRESH"), ("B", "BACK")]);
                    msg_panel(buf, "NO MATCHES YET", SIDE_DIM);
                    return;
                }
                side_frame(buf, crumb, &who, &[("A", "PLAY"), ("SELECT", "REFRESH"), ("B", "BACK")]);
                // One full-width row per game: opponent big, status + days left on the right.
                const VISIBLE: usize = 8;
                let row_h = (SIDE_BOTTOM - SIDE_TOP) / VISIBLE as i32;
                let (x, w) = (14, sw - 28);
                for (i, m) in matches.iter().enumerate().skip(*scroll_offset).take(VISIBLE) {
                    let y = SIDE_TOP + (i - *scroll_offset) as i32 * row_h;
                    let sel = i == *cursor;
                    side_row_bg(buf, x, y, w, row_h - 4, sel);
                    let mid = y + (row_h - 4) / 2;
                    let opp = format!("VS {}", m.opponent.to_uppercase());
                    draw_str_shadow_scaled(buf, &opp, x + 16, mid - 8, if sel { SIDE_GOLD } else { SIDE_WHITE }, 2);
                    let mut tx = x + 16 + str_width_scaled(&opp, 2) + 14;
                    if m.ranked {
                        let tag = if m.opp_elo > 0 { format!("RANKED {}", m.opp_elo) } else { "RANKED".to_string() };
                        let tw = str_width(&tag) + 10;
                        buf.fill_rect(tx, mid - 7, tw as u32, 14, Bgra::new(120, 80, 20));
                        draw_str(buf, &tag, tx + 5, mid - 4, SIDE_GOLD);
                        tx += tw + 10;
                    }
                    let code_str = format!("#{}", m.code);
                    draw_str(buf, &code_str, tx, mid - 4, Bgra::new(110, 115, 160));
                    // Right side: status and days left
                    let (status, scol) = if m.your_turn { ("YOUR TURN", SIDE_GREEN) } else { ("WAITING", SIDE_DIM) };
                    let days = if m.days_remaining >= 0 { format!("{}D", m.days_remaining) } else { String::new() };
                    let dcol = if m.days_remaining >= 0 && m.days_remaining <= 3 { SIDE_RED }
                               else if m.days_remaining >= 0 && m.days_remaining <= 7 { Bgra::new(230, 180, 60) }
                               else { SIDE_DIM };
                    let dw = str_width_scaled(&days, 2);
                    draw_str_shadow_scaled(buf, &days, x + w - 14 - dw, mid - 8, dcol, 2);
                    let sw2 = str_width_scaled(status, 2);
                    draw_str_shadow_scaled(buf, status, x + w - 14 - dw - 20 - sw2, mid - 8, scol, 2);
                }
                if matches.len() > VISIBLE {
                    // Scroll thumb on the right edge
                    let track = SIDE_BOTTOM - SIDE_TOP;
                    let pct = *scroll_offset as f32 / (matches.len() - VISIBLE) as f32;
                    buf.fill_rect(sw - 8, SIDE_TOP, 4, track as u32, Bgra::new(25, 25, 40));
                    buf.fill_rect(sw - 8, SIDE_TOP + ((track - 24) as f32 * pct) as i32, 4, 24, SIDE_GOLD);
                }
                if !self.error.is_empty() { msg_panel(buf, &self.error, SIDE_RED); }
                let _ = ready_n;
            }
            Screen::Loading(msg) => {
                side_frame(buf, "TAKE A TURN", &who, &[("A", "REFRESH"), ("B", "BACK")]);
                msg_panel(buf, msg, SIDE_GOLD);
            }
            Screen::Searching { ranked, .. } => {
                side_frame(buf, if *ranked { "RANKED MATCH" } else { "MATCHMAKE" }, &who, &[("B", "CANCEL")]);
                let msg = if *ranked { "FINDING RANKED MATCH..." } else { "FINDING OPPONENT..." };
                msg_panel(buf, msg, SIDE_GREEN);
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
