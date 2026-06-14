mod world;
mod physics;
mod renderer;
mod input;
mod game;
mod net;
mod updater;
mod audio;
const VERSION: &str = "0.5.4.158";

use std::time::{Duration, Instant};
use world::{WorldPos, Heightmap, Terrain, WORLD_W};
use renderer::{Framebuffer, WorldBuffer, Camera};
use input::InputState;
use game::{
    title::{TitleScreen, CHOICE_QUIT, CHOICE_LIVE, CHOICE_TAKE_A_TURN,
             CHOICE_SP, CHOICE_HOTSEAT, CHOICE_VS_CPU},
    cpu::CpuState,
    state::GameState,
    team::{Team, Difficulty},
    loop_runner::{LoopState, tick},
};

/// Target tick rate — 20 Hz.
const TICK_HZ:       u64      = 30;
const TICK_DURATION: Duration = Duration::from_millis(1000 / TICK_HZ);

fn main() {
    // Release any audio/device fds inherited from the Onion launcher so that
    // aplay can open the ALSA device cleanly for sound effects.
    unsafe { for fd in 3i32..=255 { libc::close(fd); } }

    // ── Open hardware ─────────────────────────────────────────────────────────
    let mut fb = Framebuffer::open()
        .expect("Failed to open /dev/fb0");
    let mut input = InputState::new();
    input.open().expect("Failed to open /dev/input/event0");
    let mut buf    = WorldBuffer::new();
    let mut lstate = LoopState::new();

    // Initialise audio engine (rodio/ALSA on armv7; no-op elsewhere).
    audio::init();

    // Splash screen: show wharf.jpg while preloading SFX and warming texture atlas.
    std::thread::spawn(audio::preload);
    std::thread::spawn(|| { crate::renderer::terrain_textures::tile(0); });
    renderer::splash::draw_splash(&mut buf);
    buf.blit_to_fb(&mut fb, 0);
    {
        let splash_start = Instant::now();
        while splash_start.elapsed() < std::time::Duration::from_secs(5) {
            input.poll();
            if input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start) { break; }
            std::thread::sleep(TICK_DURATION);
        }
    }

    // If a binary update was attempted this boot session already, skip the check.
    // This breaks the update-loop that occurs when cp fails on FAT32:
    // old binary execs → sentinel exists → no retry → runs normally until reboot.
    let skip_update = updater::prior_update_attempted();

    // Update check runs silently in background from app launch.
    // By the time user navigates to MULTIPLAYER (~2-5s), it's done.
    // try_recv() is instant — no screen, no blocking.
    let (update_tx, update_rx) = std::sync::mpsc::channel::<bool>();
    if !skip_update {
        std::thread::spawn(move || { let _ = update_tx.send(updater::check_for_update(VERSION)); });
    } else {
        drop(update_tx); // channel disconnected immediately; recv_timeout returns Err right away
    }
    // Always sync missing assets (sfx etc.) in background, even when no binary update is needed.
    updater::sync_assets_bg();

    // After a live game ends, return to the MP submenu rather than full title.
    let mut return_to_mp = false;
    // Cached result from the background update-check thread.
    let mut update_available = false;

    // ── Pre-title update check ───────────────────────────────────────────────
    // Wait briefly for the background check before showing the title.
    // If an update is found, show the update screen (A=install, B=skip).
    // Skipping here keeps update_available=true so MP will still force it.
    if !skip_update {
        if let Ok(true) = update_rx.recv_timeout(std::time::Duration::from_millis(2500)) {
            update_available = true;
            use renderer::Bgra;
            use renderer::font::{draw_str_scaled, draw_str, str_width_scaled, str_width};
            use world::{SCREEN_W, SCREEN_H};
            let sw = SCREEN_W as i32; let sh = SCREEN_H as i32;
            let bar_x = 40i32; let bar_w = sw - 80;
            let bar_y = sh/2 + 10; let bar_h = 24i32;
            // Pull the changelog from the Pi once, so the list is always current
            // without rebuilding the app. Falls back if offline.
            let changelog = updater::fetch_changelog(3)
                .unwrap_or_else(|| vec!["update notes unavailable offline".to_string()]);
            'pre_update: loop {
                input.poll();
                if input.just_pressed(input::Button::A) {
                    let binary = updater::stream_binary(|done, total| {
                        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(8, 10, 22));
                        buf.fill_rect(0, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
                        let t = "DOWNLOADING UPDATE";
                        draw_str_scaled(&mut buf, t, sw/2 - str_width_scaled(t,2)/2, 10, Bgra::new(255,210,50), 2);
                        buf.fill_rect(bar_x-2, bar_y-2, (bar_w+4) as u32, (bar_h+4) as u32, Bgra::new(60,60,100));
                        buf.fill_rect(bar_x, bar_y, bar_w as u32, bar_h as u32, Bgra::new(20,20,40));
                        let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                        let filled = (bar_w as f32 * frac) as u32;
                        if filled > 0 { buf.fill_rect(bar_x, bar_y, filled, bar_h as u32, Bgra::new(80,200,120)); }
                        let pct = format!("{}%", (frac * 100.0) as u32);
                        draw_str(&mut buf, &pct, sw/2 - str_width(&pct)/2, bar_y + bar_h + 10, Bgra::new(180,180,200));
                        buf.blit_to_fb(&mut fb, 0);
                    });
                    match binary {
                        Some(b) if b.len() > 4 && b[0] == 0x7f && &b[1..4] == b"ELF" => {
                            draw_msg(&mut buf, &mut fb, "APPLYING UPDATE...");
                            updater::apply_binary(&b, &mut buf, &mut fb);
                            // exec failed — fall through to title
                        }
                        _ => { draw_msg(&mut buf, &mut fb, "DOWNLOAD FAILED"); std::thread::sleep(std::time::Duration::from_secs(2)); }
                    }
                    break 'pre_update;
                }
                if input.just_pressed(input::Button::B) || input.just_pressed(input::Button::Start) {
                    break 'pre_update; // skip — update_available stays true, MP will catch it
                }
                buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(8, 10, 22));
                buf.fill_rect(0, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
                let t = "UPDATE AVAILABLE";
                draw_str_scaled(&mut buf, t, sw/2 - str_width_scaled(t,2)/2, 10, Bgra::new(255, 210, 50), 2);
                let v = format!("VERSION {}", VERSION);
                draw_str_scaled(&mut buf, &v, sw/2 - str_width_scaled(&v, 1)/2, 34, Bgra::new(100, 100, 140), 1);
                for (i, line) in changelog.iter().enumerate() {
                    draw_str_scaled(&mut buf, line, 18, 54 + i as i32 * 12, Bgra::new(110, 130, 160), 1);
                }
                draw_str_scaled(&mut buf, "A = INSTALL NOW", sw/2 - str_width_scaled("A = INSTALL NOW",2)/2, sh - 70, Bgra::new(80, 220, 120), 2);
                draw_str_scaled(&mut buf, "B = SKIP", sw/2 - str_width_scaled("B = SKIP",2)/2, sh - 38, Bgra::new(140, 140, 160), 2);
                buf.blit_to_fb(&mut fb, 0);
                std::thread::sleep(TICK_DURATION);
            }
        }
    }

    // ── Title screen ────────────────────────────────────────────────────────
    'game: loop {
    let mut title = TitleScreen::new(VERSION);
    if return_to_mp { title.continue_to_submenu(); return_to_mp = false; }
    let choice = loop {
        let c = loop {
            let frame_start = Instant::now();
            input.poll();
            // Non-blocking poll — cache result once background thread finishes.
            if !update_available { if let Ok(true) = update_rx.try_recv() { update_available = true; } }
            if let Some(c) = title.update(&input, &mut buf) { break c; }
            buf.blit_to_fb(&mut fb, 0);
            let elapsed = frame_start.elapsed();
            if elapsed < TICK_DURATION { std::thread::sleep(TICK_DURATION - elapsed); }
        };
        if c == CHOICE_SP { title.continue_to_sp_submenu(); continue; }
        // MY TEAM — roster management, no login required
        if c == game::title::CHOICE_MY_TEAM {
            use game::account::{load_cached_rosters, load_saved_creds, Roster};
            let rosters = { let mut r = load_cached_rosters(); if r.is_empty() { r.push(Roster::default_named(0)); } r };
            let token = load_saved_creds().map(|(_, t)| t).unwrap_or_default();
            show_my_teams_menu(&mut fb, &mut input, &mut buf, &rosters, &token);
            continue; // back to title
        }
        if c == game::title::CHOICE_MISSIONS {
            let token = game::account::load_saved_creds().map(|(_, t)| t).unwrap_or_default();
            show_missions_screen(&mut fb, &mut input, &mut buf, &token);
            title.continue_to_submenu(); // return to MULTIPLAYER submenu
            continue;
        }
        if c != game::title::CHOICE_MULTI { break c; }
        input.poll();
        title.continue_to_submenu();
    };
    if choice == CHOICE_QUIT { return; }
    // ── Unified update gate: optional for SP, required for MP ────────────────
    // Non-blocking drain — catches results that arrived while in the title loop.
    if !update_available {
        if let Ok(true) = update_rx.try_recv() { update_available = true; }
    }
    let is_sp_mode = matches!(choice, CHOICE_HOTSEAT | CHOICE_VS_CPU)
        || choice == game::title::CHOICE_TEST;
    let is_mp_mode = choice == CHOICE_LIVE
        || choice == CHOICE_TAKE_A_TURN
        || choice == game::title::CHOICE_LIVE_RANKED
        || choice == game::title::CHOICE_TAT_RANKED;
    // Wait for background thread result; if channel is disconnected (sentinel dropped tx),
    // spawn a fresh dedicated check so SP/MP always gets a live result.
    if (is_sp_mode || is_mp_mode) && !update_available {
        let got = update_rx.recv_timeout(std::time::Duration::from_millis(500));
        if let Ok(true) = got {
            update_available = true;
        } else if !skip_update && (got == Err(std::sync::mpsc::RecvTimeoutError::Disconnected) || !update_available) {
            // Background timed out (NOT sentinel) — do a blocking check before proceeding.
            // skip_update guard is critical: if sentinel fired, the disconnected channel
            // must NOT trigger a fresh check or the device loops on every failed update.
            let (ftx, frx) = std::sync::mpsc::channel::<bool>();
            std::thread::spawn(move || { let _ = ftx.send(updater::check_for_update(VERSION)); });
            if let Ok(true) = frx.recv_timeout(std::time::Duration::from_millis(3000)) {
                update_available = true;
            }
        }
    }
    if update_available && (is_sp_mode || is_mp_mode) {
        use renderer::Bgra;
        use renderer::font::{draw_str_scaled, draw_str, str_width_scaled, str_width};
        use world::{SCREEN_W, SCREEN_H};
        let forced = is_mp_mode;
        let sw = SCREEN_W as i32; let sh = SCREEN_H as i32;
        let bar_x = 40i32; let bar_w = sw - 80;
        let bar_y = sh/2 + 10; let bar_h = 24i32;
        // Pi-served changelog (see pre-title block) — always current, no rebuild.
        let changelog = updater::fetch_changelog(3)
            .unwrap_or_else(|| vec!["update notes unavailable offline".to_string()]);
        let proceed = loop {
            input.poll();
            if input.just_pressed(input::Button::A) {
                let binary = updater::stream_binary(|done, total| {
                    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(8, 10, 22));
                    buf.fill_rect(0, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
                    let t = "DOWNLOADING UPDATE";
                    draw_str_scaled(&mut buf, t, sw/2 - str_width_scaled(t,2)/2, 10, Bgra::new(255,210,50), 2);
                    buf.fill_rect(bar_x-2, bar_y-2, (bar_w+4) as u32, (bar_h+4) as u32, Bgra::new(60,60,100));
                    buf.fill_rect(bar_x, bar_y, bar_w as u32, bar_h as u32, Bgra::new(20,20,40));
                    let frac = if total > 0 { done as f32 / total as f32 } else { 0.0 };
                    let filled = (bar_w as f32 * frac) as u32;
                    if filled > 0 { buf.fill_rect(bar_x, bar_y, filled, bar_h as u32, Bgra::new(80,200,120)); }
                    let pct = format!("{}%", (frac * 100.0) as u32);
                    draw_str(&mut buf, &pct, sw/2 - str_width(&pct)/2, bar_y + bar_h + 10, Bgra::new(180,180,200));
                    buf.blit_to_fb(&mut fb, 0);
                });
                match binary {
                    Some(b) if b.len() > 4 && b[0] == 0x7f && &b[1..4] == b"ELF" => {
                        draw_msg(&mut buf, &mut fb, "APPLYING UPDATE...");
                        updater::apply_binary(&b, &mut buf, &mut fb);
                        break false; // apply_binary called exec; if we're here exec failed
                    }
                    _ => { draw_msg(&mut buf, &mut fb, "DOWNLOAD FAILED"); std::thread::sleep(std::time::Duration::from_secs(2)); }
                }
            }
            // B/Start: SP → skip update and proceed; MP → back to title
            if input.just_pressed(input::Button::B) || input.just_pressed(input::Button::Start) {
                break !forced;
            }
            buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(8, 10, 22));
            buf.fill_rect(0, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
            let t = if forced { "UPDATE REQUIRED FOR MULTIPLAYER" } else { "UPDATE AVAILABLE" };
            let t_col = if forced { Bgra::new(255, 80, 80) } else { Bgra::new(255, 210, 50) };
            draw_str_scaled(&mut buf, t, sw/2 - str_width_scaled(t,2)/2, 10, t_col, 2);
            let v = format!("VERSION {}", VERSION);
            draw_str_scaled(&mut buf, &v, sw/2 - str_width_scaled(&v, 1)/2, 34, Bgra::new(100, 100, 140), 1);
            for (i, line) in changelog.iter().enumerate() {
                draw_str_scaled(&mut buf, line, 18, 54 + i as i32 * 12, Bgra::new(110, 130, 160), 1);
            }
            draw_str_scaled(&mut buf, "A = INSTALL NOW", sw/2 - str_width_scaled("A = INSTALL NOW",2)/2, sh - 70, Bgra::new(80, 220, 120), 2);
            let b_label = if forced { "B = BACK" } else { "B = SKIP" };
            draw_str_scaled(&mut buf, b_label, sw/2 - str_width_scaled(b_label,2)/2, sh - 38, Bgra::new(140, 140, 160), 2);
            buf.blit_to_fb(&mut fb, 0);
            std::thread::sleep(TICK_DURATION);
        };
        if !proceed { continue 'game; }
    }
    // HOTSEAT = local 2-player, VS_CPU = CPU AI
    let is_live         = choice == CHOICE_LIVE;
    let is_tat          = choice == CHOICE_TAKE_A_TURN;
    let is_test         = choice == game::title::CHOICE_TEST;
    let is_hotseat      = choice == CHOICE_HOTSEAT || is_test;
    let is_vs_cpu       = choice == CHOICE_VS_CPU;
    let is_live_ranked  = choice == game::title::CHOICE_LIVE_RANKED;
    let is_tat_ranked   = choice == game::title::CHOICE_TAT_RANKED;
    let is_live_stats        = choice == game::title::CHOICE_LIVE_STATS;
    let is_tat_stats         = choice == game::title::CHOICE_TAT_STATS;
    let is_leaderboard_casual = choice == game::title::CHOICE_LEADERBOARD_CASUAL;
    let is_leaderboard_ranked = choice == game::title::CHOICE_LEADERBOARD_RANKED;
    let cpu_team: Option<usize> = if is_vs_cpu { Some(1) } else { None };

    // Stats screens
    if is_live_stats || is_tat_stats {
        let mode = if is_live_stats { "live" } else { "tat" };
        use game::account::load_saved_creds;
        if let Some((_, token)) = load_saved_creds() {
            show_stats_screen(&mut fb, &mut input, &mut buf, &token, mode);
        } else {
            draw_msg(&mut buf, &mut fb, "LOG IN FIRST");
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        continue 'game;
    }

    // Leaderboard screens
    if is_leaderboard_casual || is_leaderboard_ranked {
        show_leaderboard_screen(&mut fb, &mut input, &mut buf, is_leaderboard_ranked);
        continue 'game;
    }

    // TAT RANKED → same account flow as casual TAT but with ranked flag
    if is_tat_ranked {
        run_take_a_turn_ranked(&mut fb, &mut input, &mut buf);
        continue 'game;
    }

    // ── Account + roster (live mode, casual or ranked) ─────────────────────
    let mut live_roster: Option<game::account::Roster> = None;
    let mut live_ranked_match = false;
    let mut live_elo_my:  i32 = 0;
    let mut live_elo_opp: i32 = 0;
    let mut live_game_port: u16 = 7777; // port of the spawned game server instance
    if is_live || is_live_ranked {
        use game::account::{AccountScreen, AccountAction, RosterPicker, RosterAction,
                             load_saved_creds};
        // Login — load teams from local cache instantly (no network call)
        let (token, mut rosters) = if let Some((u, t)) = load_saved_creds() {
            let r = game::account::load_cached_rosters();
            (t, r)
        } else {
            let mut acct = AccountScreen::new();
            let result = loop {
                let fs = std::time::Instant::now();
                input.poll();
                buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32,
                    renderer::Bgra::new(8, 8, 20));
                if let Some(a) = acct.update(&input, &mut buf, 0) { break a; }
                buf.blit_to_fb(&mut fb, 0);
                let e = fs.elapsed(); if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
            };
            match result {
                AccountAction::LoggedIn { token, rosters, .. } => (token, rosters),
                AccountAction::Back => { continue 'game; }
            }
        };
        if rosters.is_empty() { rosters.push(game::account::Roster::default_named(0)); }
        // Daily login bonus
        if let Some((earned, weekly)) = game::account::claim_daily_login(&token) {
            show_login_bonus(&mut buf, &mut fb, &mut input, earned, weekly);
        }
        // Roster picker
        let mut picker = RosterPicker::new(token, rosters);
        let picked = loop {
            let fs = std::time::Instant::now();
            input.poll();
            buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32,
                renderer::Bgra::new(8, 8, 20));
            if let Some(a) = picker.update(&input, &mut buf, 0) { break a; }
            buf.blit_to_fb(&mut fb, 0);
            let e = fs.elapsed(); if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
        };
        match picked {
            RosterAction::Selected(r) => live_roster = Some(r),
            RosterAction::Skip => {} // proceed with default generic names
            RosterAction::Back => { continue 'game; }
        }
    }

    // ── Ranked live: join queue and wait for match ─────────────────────────
    if is_live_ranked {
        use game::account::{http_post, http_get, json_field, load_saved_creds};
        let token = load_saved_creds().map(|(_, t)| t).unwrap_or_default();
        // Join queue
        let body = format!(r#"{{"token":"{}"}}"#, token);
        match http_post("/api/ranked/queue/join", &body) {
            Ok(resp) if resp.contains("matched") => {
                // Paired immediately
                if resp.contains("\"matched\"") {
                    live_ranked_match = true;
                    live_elo_opp     = json_field(&resp, "opponent_elo").and_then(|s| s.parse().ok()).unwrap_or(1000);
                    live_elo_my      = json_field(&resp, "my_elo").and_then(|s| s.parse().ok()).unwrap_or(1000);
                    live_game_port   = json_field(&resp, "port").and_then(|s| s.parse().ok()).unwrap_or(7777);
                }
            }
            Ok(ref wait_resp) => {
                // Waiting — capture our ELO from the waiting response before the poll loop
                live_elo_my = json_field(wait_resp, "my_elo").and_then(|s| s.parse().ok()).unwrap_or(0);
                let (tx, rx) = std::sync::mpsc::channel::<Option<(u16,i32,i32)>>();
                let token2 = token.clone();
                std::thread::spawn(move || {
                    for _ in 0..60 { // poll up to 60 times (max 30 seconds)
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        let path = format!("/api/ranked/queue/status?token={}", token2);
                        if let Ok(resp) = http_get(&path) {
                            if resp.contains("\"matched\"") {
                                let port: u16 = json_field(&resp, "port").and_then(|s| s.parse().ok()).unwrap_or(7777);
                                let opp: i32  = json_field(&resp, "opponent_elo").and_then(|s| s.parse().ok()).unwrap_or(1000);
                                let my: i32   = json_field(&resp, "my_elo").and_then(|s| s.parse().ok()).unwrap_or(1000);
                                let _ = tx.send(Some((port, my, opp)));
                                return;
                            }
                        }
                    }
                    let _ = tx.send(None); // timed out
                });
                let matched = loop {
                    input.poll();
                    if input.just_pressed(input::Button::B) || input.just_pressed(input::Button::Start) {
                        // Leave queue
                        let b2 = format!(r#"{{"token":"{}"}}"#, token);
                        std::thread::spawn(move || { http_post("/api/ranked/queue/leave", &b2).ok(); });
                        continue 'game;
                    }
                    let wait_msg = if live_elo_my > 0 {
                        format!("FINDING MATCH...  ELO {}  B=CANCEL", live_elo_my)
                    } else {
                        "FINDING MATCH...  B=CANCEL".to_string()
                    };
                    draw_msg(&mut buf, &mut fb, &wait_msg);
                    if let Ok(result) = rx.try_recv() { break result; }
                    std::thread::sleep(std::time::Duration::from_millis(33));
                };
                match matched {
                    Some((port, my, opp)) => { live_ranked_match = true; live_game_port = port; live_elo_my = my; live_elo_opp = opp; }
                    None => { draw_msg(&mut buf, &mut fb, "NO MATCH FOUND"); std::thread::sleep(std::time::Duration::from_secs(2)); continue 'game; }
                }
            }
            Err(_) => { draw_msg(&mut buf, &mut fb, "NETWORK ERROR"); std::thread::sleep(std::time::Duration::from_secs(2)); continue 'game; }
        }
    }

    // ── Multiplayer connect — background thread so B cancels instantly ──────
    let mut net_conn: Option<net::ServerConn> = None;
    if is_live || is_live_ranked {
        use net::ServerConn;
        let ver = VERSION;
        // Spawn connect attempt; main loop polls and checks B each frame
        let (conn_tx, conn_rx) = std::sync::mpsc::channel::<Result<ServerConn, ()>>();
        let port = live_game_port;
        std::thread::spawn(move || {
            let addr = format!("crumbonium.duckdns.org:{}", port);
            let _ = conn_tx.send(ServerConn::connect(&addr).map_err(|_| ()));
        });
        let connected = loop {
            input.poll();
            if input.just_pressed(input::Button::Start) || input.just_pressed(input::Button::B) {
                continue 'game;
            }
            match conn_rx.try_recv() {
                Ok(Ok(mut c)) => {
                    c.send_raw(b"MMAY");
                    c.send_raw(ver.as_bytes());
                    c.send_raw(b"\n");
                    break Some(c);
                }
                Ok(Err(_)) => {
                    draw_msg(&mut buf, &mut fb, "CONNECT FAILED  (B=BACK)");
                    // Wait a moment so user can see the message, then let them cancel
                    loop {
                        input.poll();
                        if input.just_pressed(input::Button::B) || input.just_pressed(input::Button::A) { break; }
                        std::thread::sleep(std::time::Duration::from_millis(33));
                    }
                    break None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    draw_msg(&mut buf, &mut fb, "CONNECTING...  B=CANCEL");
                    std::thread::sleep(std::time::Duration::from_millis(33));
                }
                Err(_) => break None,
            }
        };
        if let Some(c) = connected {
            net_conn = Some(c);
        } else {
            continue 'game;
        }
    }
    let mut my_team: usize = 0;
    let mut server_seed: Option<u64> = None;
    if let Some(ref mut conn) = net_conn {
        // Short read timeout so recv_blocking returns quickly and B can be polled
        conn.stream.set_read_timeout(Some(std::time::Duration::from_millis(200))).ok();
        let mut settle = 0u32;
        let welcome = loop {
            settle += 1;
            input.poll();
            if input.just_pressed(input::Button::Start) || input.just_pressed(input::Button::B) {
                break None;
            }
            draw_msg(&mut buf, &mut fb, "WAITING FOR OPPONENT... B=CANCEL");
            if let Some(w) = conn.recv_blocking::<net::msg::WelcomeMsg>() { break Some(w); }
        };
        if let Some(w) = welcome {
            conn.stream.set_read_timeout(None).ok(); // clear timeout for gameplay
            my_team = w.your_team;
            server_seed = Some(w.seed);
            conn.start_reader();
            let msg = if my_team == 0 { "YOU ARE RED  -  YOUR TURN FIRST" } else { "YOU ARE BLUE  -  OPPONENT GOES FIRST" };
            draw_msg(&mut buf, &mut fb, msg);
            std::thread::sleep(std::time::Duration::from_millis(800));
        } else {
            // B pressed — drop connection and return to title
            net_conn = None;
            continue 'game;
        }
    }
    if is_tat { run_take_a_turn(&mut fb, &mut input, &mut buf); continue 'game; }
    let game_seed = server_seed.unwrap_or_else(current_time_seed);
    let mut game    = build_default_game(game_seed);
    // Test mode: give every team the full weapon set with infinite ammo.
    if is_test {
        use physics::projectile::WeaponKind;
        let all_weapons: Vec<(WeaponKind, Option<u32>)> = vec![
            (WeaponKind::Bazooka,     None),
            (WeaponKind::Grenade,     None),
            (WeaponKind::Shotgun,     None),
            (WeaponKind::NinjaRope,   None),
            (WeaponKind::Tnt,         None),
            (WeaponKind::Landmine,    None),
            (WeaponKind::BaseballBat, None),
            (WeaponKind::BananaBomb,  None),
            (WeaponKind::Revolver,      None),
            (WeaponKind::Blasthive,     None),
            (WeaponKind::BlackHoleBomb, None),
            (WeaponKind::PlasmaTorch,   None),
            (WeaponKind::Garcia,        None),
        ];
        for team in &mut game.teams {
            team.weapons = all_weapons.clone();
            team.selected_weapon = 0;
        }
        game.turn.turn_number = 100; // unlock all time-gated weapons
        game.is_test = true;         // show the map seed on screen
    }
    if is_mp_mode {
        game.is_multiplayer = true;  // enable scrap crate drops
    }
    // Apply roster to live mode player's team
    if live_ranked_match {
        game.teams[my_team].elo     = live_elo_my  as u32;
        game.teams[1 - my_team].elo = live_elo_opp as u32;
    }
    if let Some(ref roster) = live_roster {
        let my_t = my_team;
        game.teams[my_t].name         = roster.name.clone();
        game.teams[my_t].avatar_id    = roster.avatar_id;
        game.teams[my_t].headstone_id = roster.headstone_id;
        for si in 0..game.teams[my_t].soldiers.len().min(4) {
            game.teams[my_t].soldiers[si].name             = roster.worm_names[si].clone();
            game.teams[my_t].soldiers[si].hat_id           = roster.hat_ids[si];
            game.teams[my_t].soldiers[si].uniform_color_id = roster.uniform_color_ids[si];
            game.teams[my_t].soldiers[si].boot_color_id    = roster.boot_color_ids[si];
            game.teams[my_t].soldiers[si].gun_style_id     = roster.gun_style_ids[si];
        }
    }
    // Match intro screen for all live matches
    if is_live || live_ranked_match {
        show_match_intro(&mut fb, &mut buf, &mut input, &game, my_team);
    }

    // VS CPU: player picks their team; selecting immediately starts the game
    if is_vs_cpu {
        use game::account::{load_cached_rosters, load_saved_creds};
        let rosters = load_cached_rosters();
        let token   = load_saved_creds().map(|(_, t)| t).unwrap_or_default();
        match show_roster_picker(&mut fb, &mut input, &mut buf, &rosters, &token) {
            None    => { continue 'game; }
            Some(r) => {
                game.teams[0].name      = r.name.clone();
                game.teams[0].avatar_id = r.avatar_id;
                for si in 0..game.teams[0].soldiers.len().min(4) {
                    game.teams[0].soldiers[si].name             = r.worm_names[si].clone();
                    game.teams[0].soldiers[si].hat_id           = r.hat_ids[si];
                    game.teams[0].soldiers[si].uniform_color_id = r.uniform_color_ids[si];
                    game.teams[0].soldiers[si].boot_color_id    = r.boot_color_ids[si];
                    game.teams[0].soldiers[si].gun_style_id     = r.gun_style_ids[si];
                }
            }
        }
    }
    // Hotseat/Test: silently apply cosmetics from first cached roster to team 0
    if is_hotseat {
        if let Some(r) = game::account::load_cached_rosters().into_iter().next() {
            game.teams[0].name      = r.name.clone();
            game.teams[0].avatar_id = r.avatar_id;
            for si in 0..game.teams[0].soldiers.len().min(4) {
                game.teams[0].soldiers[si].name             = r.worm_names[si].clone();
                game.teams[0].soldiers[si].hat_id           = r.hat_ids[si];
                game.teams[0].soldiers[si].uniform_color_id = r.uniform_color_ids[si];
                game.teams[0].soldiers[si].boot_color_id    = r.boot_color_ids[si];
                game.teams[0].soldiers[si].gun_style_id     = r.gun_style_ids[si];
            }
        }
    }
    lstate = game::loop_runner::LoopState::new(); // always reset — previous game may have left paused=true
    // CPU AI state — reset each turn
    let mut cpu_state = CpuState::undecided();
    let start_pos   = game.teams[0].soldiers[0].pos;
    let mut cam     = Camera::new(start_pos.x);
    cam.snap_to(start_pos);
    let mut game_over_ticks: u32 = 0;
    // Last server tick whose StateMsg.sounds we've already played — dedupes the
    // 90× repeated final state on game-over and lets us recover sounds from any
    // intermediate ticks dropped while draining.
    let mut last_sound_tick: u32 = u32::MAX;
    let mut final_result: Option<game::state::GameResult> = None;
    // ELO delta shown on end screen for ranked matches
    let mut elo_delta: i32 = 0;
    let mut elo_delta_rx: Option<std::sync::mpsc::Receiver<i32>> = None;
    let mut fps_window_start = Instant::now();
    let mut fps_frame_count: u32 = 0;
    loop {
        let frame_start = Instant::now();
        input.poll();
        if let Some(ref mut conn) = net_conn {
            // Disconnect detection — reader thread or write failure sets this flag.
            if conn.is_disconnected() {
                draw_msg(&mut buf, &mut fb, "LOST CONNECTION");
                std::thread::sleep(std::time::Duration::from_secs(2));
                return_to_mp = true;
                continue 'game;
            }
            use net::msg::{InputMsg, NetButton};
            // Suppress A-HELD when server_fire_grace active (same state used in tick/server_tick).
            let suppress_a_held = game.server_fire_grace > 0;
            let held: Vec<NetButton> = [
                (input::Button::Up,     NetButton::Up),
                (input::Button::Down,   NetButton::Down),
                (input::Button::Left,   NetButton::Left),
                (input::Button::Right,  NetButton::Right),
                (input::Button::A,      NetButton::A),
                (input::Button::B,      NetButton::B),
                (input::Button::Y,      NetButton::Y),
                (input::Button::Start,  NetButton::Start),
                (input::Button::Select, NetButton::Select),
                (input::Button::L1,     NetButton::L1),
                (input::Button::R1,     NetButton::R1),
            ].iter().filter(|(b,_)| input.held(*b) && !(suppress_a_held && *b == input::Button::A)).map(|(_,n)| *n).collect();
            let pressed: Vec<NetButton> = [
                (input::Button::Up,     NetButton::Up),
                (input::Button::Down,   NetButton::Down),
                (input::Button::Left,   NetButton::Left),
                (input::Button::Right,  NetButton::Right),
                (input::Button::B,      NetButton::B),
                (input::Button::Y,      NetButton::Y),
                (input::Button::A,      NetButton::A),
                (input::Button::Start,  NetButton::Start),
                (input::Button::Select, NetButton::Select),
                (input::Button::L1,     NetButton::L1),
                (input::Button::R1,     NetButton::R1),
            ].iter().filter(|(b,_)| input.just_pressed(*b)).map(|(_,n)| *n).collect();
            let released: Vec<NetButton> = [
                (input::Button::A, NetButton::A),
            ].iter().filter(|(b,_)| input.just_released(*b)).map(|(_,n)| *n).collect();
            let selected_weapon_kind = game.teams[my_team].current_weapon().to_net_u8();
            let (hat_ids, uniform_color_ids, boot_color_ids, gun_style_ids, worm_names) = {
                let t = &game.teams[my_team];
                let n = t.soldiers.len().min(4);
                let mut h = [0u8;4]; let mut u = [0u8;4]; let mut b = [0u8;4]; let mut g = [0u8;4];
                let mut w: [String;4] = Default::default();
                for i in 0..n {
                    h[i] = t.soldiers[i].hat_id; u[i] = t.soldiers[i].uniform_color_id;
                    b[i] = t.soldiers[i].boot_color_id; g[i] = t.soldiers[i].gun_style_id;
                    w[i] = t.soldiers[i].name.clone();
                }
                (h, u, b, g, w)
            };
            if !lstate.paused { conn.send(&InputMsg { tick: lstate.tick, held, pressed, released, aim_angle: game.aim.angle, selected_weapon_kind, hat_ids, uniform_color_ids, boot_color_ids, gun_style_ids, worm_names }); }
            // Drain ALL pending state messages:
            //   - sounds collected from every state so no SFX tick is skipped
            //   - first received state's projectiles used for position (avoids the
            //     2-tick jump when two states arrive in one frame, which looked choppy)
            //   - latest state used for everything else (turn, soldiers, result)
            let mut latest_state: Option<net::msg::StateMsg> = None;
            let mut first_projectiles: Option<Vec<net::msg::NetProjectile>> = None;
            let mut pending_sounds: Vec<u8> = Vec::new();
            while let Some(state) = conn.try_recv::<net::msg::StateMsg>() {
                if state.tick != last_sound_tick {
                    pending_sounds.extend_from_slice(&state.sounds);
                    last_sound_tick = state.tick;
                }
                if first_projectiles.is_none() {
                    first_projectiles = Some(state.projectiles.clone());
                }
                latest_state = Some(state);
            }
            // Play the server's recorded SFX (skip while the game-over overlay is up).
            if final_result.is_none() {
                for id in &pending_sounds {
                    if let Some(s) = crate::audio::Sfx::from_u8(*id) { crate::audio::play(s); }
                }
            }
            let got_state = latest_state.is_some();
            if let Some(state) = latest_state {
                // While showing the 10-second game-over overlay, don't let a new-game
                // StateMsg (result=Ongoing) from the server clear our final result.
                if final_result.is_none() {
                    apply_server_state(&mut game, &mut cam, &state, my_team);
                    // Restore projectile positions from the FIRST state received this
                    // frame so multi-state frames don't cause a 2-tick position jump.
                    if let Some(projs) = first_projectiles {
                        use crate::physics::projectile::{Projectile, WeaponKind, FuseState};
                        use crate::world::{Vec2, WorldPos};
                        game.projectiles.clear();
                        for np in &projs {
                            let kind = WeaponKind::from_net_u8(np.kind_u8);
                            let mut proj = Projectile::new(WorldPos::new(np.x, np.y), Vec2::new(np.vel_x, np.vel_y), kind);
                            if np.fuse_ticks > 0 { proj.fuse = FuseState::Burning(np.fuse_ticks); }
                            proj.is_fragment = np.is_fragment;
                            game.projectiles.push(proj);
                        }
                    }
                    // Latch the result the first time the game ends
                    if !matches!(game.result, game::state::GameResult::Ongoing) {
                        final_result = Some(game.result.clone());
                    }
                }
            }
            // Check for server reset — ignore WelcomeMsg while showing game-over screen
            if final_result.is_none() && matches!(game.result, game::state::GameResult::Ongoing) {
                if let Some(w) = conn.try_recv_welcome() {
                    my_team = w.your_team;
                    let new_seed = w.seed;
                    game = build_default_game_opts(new_seed, true, true); // live mode always has mines/barrels
                    game_over_ticks = 0;
                    lstate = game::loop_runner::LoopState::new();
                    let start_pos = game.teams[0].soldiers[0].pos;
                    cam = Camera::new(start_pos.x);
                    cam.snap_to(start_pos);
                    draw_msg(&mut buf, &mut fb, if my_team == 0 { "YOU ARE RED" } else { "YOU ARE BLUE" });
                }
            } else {
                // Drain WelcomeMsg silently — we'll exit after the game-over timer
                while conn.try_recv_welcome().is_some() {}
            }
            // Advance projectiles one tick every frame, same as non-live simulate().
            // When a server state arrived it set positions to tick N; ticking predicts N+1.
            // The "first state's projectiles" logic above ensures multi-packet frames
            // still only advance by one tick (first state = N, tick → N+1).
            let wind = game.wind.value();
            for proj in &mut game.projectiles {
                crate::physics::tick::tick(proj, wind);
            }
            {
                use crate::physics::projectile::WeaponKind;
                let spawns: Vec<_> = game.projectiles.iter()
                    .filter(|p| p.kind == WeaponKind::Bazooka)
                    .map(|p| p.pos)
                    .collect();
                for pos in spawns { game.smoke_particles.push((pos, 22)); }
            }
            game.smoke_particles.retain_mut(|(_, t)| { if *t > 0 { *t -= 1; true } else { false } });
        }
        let mut mp_quit = false;
        let running = if net_conn.is_some() {
            lstate.tick = lstate.tick.wrapping_add(1);
            game.tick   = lstate.tick;
            let settle = lstate.tick.wrapping_sub(lstate.pause_open_tick) > 10;
            if !lstate.paused {
                if input.just_pressed(input::Button::Start) && settle {
                    lstate.paused          = true;
                    lstate.pause_open_tick = lstate.tick;
                    lstate.pause_cursor    = 0;
                    game.aim.power         = 0.0;
                }
            } else {
                let do_resume = |lstate: &mut game::loop_runner::LoopState, aim: &mut game::state::AimState| {
                    lstate.paused          = false;
                    lstate.pause_open_tick = lstate.tick;
                    lstate.fire_grace      = 10;
                    aim.power              = 0.0;
                };
                if input.just_pressed(input::Button::Start) && settle { do_resume(&mut lstate, &mut game.aim); }
                if input.just_pressed(input::Button::B)               { do_resume(&mut lstate, &mut game.aim); }
                if input.just_pressed(input::Button::Up)   { lstate.pause_cursor = if lstate.pause_cursor == 0 { 1 } else { 0 }; }
                if input.just_pressed(input::Button::Down) { lstate.pause_cursor = if lstate.pause_cursor == 0 { 1 } else { 0 }; }
                if input.just_pressed(input::Button::A) && lstate.pause_cursor == 0 { do_resume(&mut lstate, &mut game.aim); }
                if input.just_pressed(input::Button::A) && lstate.pause_cursor == 1 { mp_quit = true; }
            }
            if mp_quit { continue 'game; }
            // ── Weapon menu + aim — only when it's our turn ───────────────────────
            let my_turn = game.turn.current_team == my_team;
            if !lstate.paused && my_turn {
                let menu_active = game::loop_runner::process_weapon_menu(&mut game, &input);
                if !menu_active {
                    game::loop_runner::process_aim(&mut game, &input);
                }
                game::loop_runner::tick_fire_grace(&mut game);
                if lstate.fire_grace > 0 { lstate.fire_grace -= 1; game.aim.power = 0.0; }
            } else if !my_turn {
                // Not our turn — block weapon menu; aim.power comes from server state.
                game.weapon_menu_open = false;
            }
            // Camera pan or follow active soldier
            let cam_speed = 20.0f32;
            if input.held(input::Button::L1) {
                // L1 + dpad: free pan — stays when L1 released, clears on turn change
                if input.held(input::Button::Left)  { cam.pan(-cam_speed); }
                if input.held(input::Button::Right) { cam.pan( cam_speed); }
            } else if input.held(input::Button::R1) {
                if input.held(input::Button::Left)  { cam.pan(-cam_speed); }
                if input.held(input::Button::Right) { cam.pan( cam_speed); }
            } else if let Some(g) = game.garcia.as_ref() {
                // Hand of Jerry: track the falling sprite, else the targeting cursor
                // (mirrors update_camera() so live matches local).
                if g.falling {
                    cam.follow_always(world::WorldPos::new(g.render_x, g.fall_y.max(0.0)));
                } else {
                    let ti = game.turn.current_team();
                    let sy = game.teams.get(ti)
                        .and_then(|t| t.soldiers.get(t.active))
                        .map(|s| s.pos.y)
                        .unwrap_or(g.fall_y);
                    cam.follow(world::WorldPos::new(g.render_x, sy));
                }
            } else if let Some(p) = game.projectiles.first() {
                cam.follow_always(p.pos);
            } else {
                // Follow the active soldier only. Following transient airborne
                // soldiers (knockback victims) made the camera flicker back and
                // forth between them and the active soldier as their state toggled
                // across network updates — keep it locked on the active soldier.
                let ti = game.turn.current_team();
                if let Some(team) = game.teams.get(ti) {
                    if let Some(s) = team.soldiers.get(team.active) {
                        cam.follow(s.pos);
                    }
                }
            }
            if input.just_released(input::Button::R1) { cam.release_pan(); }
            game::loop_runner::update_visuals(&mut game);
            cam.tick();
            game::loop_runner::render_live(&game, &mut buf, &mut cam, &mut lstate, my_team);
            // draw_weapon_menu_overlay uses game.weapon_menu_open — same source as tick()
            game::loop_runner::draw_weapon_menu_overlay(&game, &mut buf, cam.left_edge() as i32);
            // Game over overlay — use latched final_result so server's new-game reset can't clear it
            if let Some(ref fr) = final_result.clone() {
                // Report ranked result on first tick, capture ELO delta via channel
                if game_over_ticks == 0 && live_ranked_match {
                    if let game::state::GameResult::Winner(winner_team) = fr {
                        use game::account::{http_post, json_field, load_saved_creds};
                        let winner_slot = *winner_team;
                        if let Some((_, tok)) = load_saved_creds() {
                            let live_kills  = (4u32).saturating_sub(game.teams.get(1 - my_team).map(|t| t.alive_count()).unwrap_or(0));
                            let live_deaths = (4u32).saturating_sub(game.teams.get(my_team).map(|t| t.alive_count()).unwrap_or(0));
                            let wk_json: String = {
                                let mut wk: std::collections::HashMap<&'static str, u32> = std::collections::HashMap::new();
                                if let Some(opp) = game.teams.get(1 - my_team) {
                                    for s in &opp.soldiers {
                                        if s.is_dead() {
                                            let name = s.kill_weapon.map(|w| w.display_name()).unwrap_or("UNKNOWN");
                                            *wk.entry(name).or_insert(0) += 1;
                                        }
                                    }
                                }
                                let pairs: Vec<String> = wk.iter().map(|(k,v)| format!(r#""{}":{}"#, k, v)).collect();
                                format!("{{{}}}", pairs.join(","))
                            };
                            let body = format!(r#"{{"token":"{}","winner_slot":{},"kills":{},"deaths":{},"weapon_kills":{},"seed":{}}}"#, tok, winner_slot, live_kills, live_deaths, wk_json, game_seed);
                            let (dtx, drx) = std::sync::mpsc::channel::<i32>();
                            std::thread::spawn(move || {
                                if let Ok(resp) = http_post("/api/match/live/result", &body) {
                                    let d = json_field(&resp, "elo_delta").and_then(|s| s.parse().ok()).unwrap_or(0);
                                    let _ = dtx.send(d);
                                }
                            });
                            elo_delta_rx = Some(drx);
                        }
                    }
                }
                // Poll for ELO delta result
                if let Some(ref rx) = elo_delta_rx {
                    if let Ok(d) = rx.try_recv() { elo_delta = d; elo_delta_rx = None; }
                }
                game_over_ticks += 1;
                let winner = if let game::state::GameResult::Winner(t) = fr { Some(*t) } else { None };
                let wa = if let Some(w) = winner { game.teams.get(w).map(|t| t.avatar_id).unwrap_or(0) } else { 0 };
                let (kills, hp_left, memo) = game::loop_runner::match_end_stats(&game);
                crate::renderer::hud::draw_game_over(&mut buf, winner, Some(my_team), cam.left_edge() as i32, wa, elo_delta, kills, hp_left, &memo);
                // Countdown bar at bottom
                {
                    use world::{SCREEN_W, SCREEN_H};
                    let remaining = 300u32.saturating_sub(game_over_ticks);
                    let bar_w = (SCREEN_W as u32 * remaining / 300) as u32;
                    if bar_w > 0 {
                        let by = SCREEN_H as i32 - 4;
                        buf.fill_rect(cam.left_edge() as i32, by, SCREEN_W, 4, renderer::Bgra::new(30,30,50));
                        buf.fill_rect(cam.left_edge() as i32, by, bar_w, 4, renderer::Bgra::new(80,180,255));
                    }
                }
                if game_over_ticks >= 300 // 10 seconds at 30Hz
                    || input.just_pressed(input::Button::A)
                    || input.just_pressed(input::Button::Start)
                {
                    mp_quit = true;
                }
            }
            if lstate.paused {
                crate::renderer::hud::draw_pause_menu(&mut buf, lstate.pause_cursor as u8, cam.left_edge() as i32);
            }
            !mp_quit
        } else {
            // Singleplayer — handle CPU AI turn if needed
            if let Some(ct) = cpu_team {
                let active = game.active_team();
                let has_fired = game.teams[active].soldiers[game.teams[active].active].has_fired;
                if active == ct {
                    // CPU's entire turn — synthetic input blocks d-pad/fire,
                    // but allow Start so player can pause or exit
                    let mut cpu_input = input::InputState::new();
                    if input.just_pressed(input::Button::Start) {
                        cpu_input.inject_press(input::Button::Start);
                    }
                    // CPU doesn't need retreat time — skip it so the turn ends promptly
                    if game.turn.is_retreating() {
                        game.turn.skip_retreat();
                    }
                    if !has_fired && !lstate.paused {
                        if !cpu_state.decided {
                            let seed = game.tick.wrapping_mul(1664525).wrapping_add(1013904223);
                            cpu_state = CpuState::decide(&game, ct, seed);
                        }
                        if cpu_state.walk_ticks > 0 {
                            // Walking phase — inject directional input
                            cpu_state.walk_ticks -= 1;
                            match cpu_state.walk_dir {
                                1  => { cpu_input.inject_press(input::Button::Right); }
                                -1 => { cpu_input.inject_press(input::Button::Left); }
                                _  => {}
                            }
                        } else if cpu_state.thinking > 0 {
                            cpu_state.thinking -= 1;
                        } else {
                            // Fire
                            game.aim.angle = cpu_state.angle;
                            game.aim.power = cpu_state.power;
                            game::loop_runner::fire_bazooka_tat(&mut game);
                            cpu_state = CpuState::undecided();
                        }
                    }
                    // VS CPU: local player is the non-CPU team
                    let player_team = Some(1 - ct);
                    tick(&mut game, &cpu_input, &mut buf, &mut cam, &mut lstate, player_team)
                } else {
                    if !has_fired { cpu_state = CpuState::undecided(); }
                    let player_team = Some(1 - ct);
                    tick(&mut game, &input, &mut buf, &mut cam, &mut lstate, player_team)
                }
            } else if is_live {
                // Live multiplayer: my_team set from server
                tick(&mut game, &input, &mut buf, &mut cam, &mut lstate, Some(my_team))
            } else {
                // Hotseat: no single local player
                tick(&mut game, &input, &mut buf, &mut cam, &mut lstate, None)
            }
        };

        buf.blit_to_fb(&mut fb, cam.left_edge());

        fps_frame_count += 1;
        let fps_elapsed = fps_window_start.elapsed();
        if fps_elapsed >= Duration::from_secs(1) {
            lstate.display_fps = (fps_frame_count as f32 / fps_elapsed.as_secs_f32()).round() as u32;
            fps_frame_count = 0;
            fps_window_start = Instant::now();
        }

        let elapsed = frame_start.elapsed();
        if elapsed < TICK_DURATION {
            std::thread::sleep(TICK_DURATION - elapsed);
        }
        if !running {
            if net_conn.is_some() { return_to_mp = true; }
            continue 'game;
        }
    } // end inner game loop
    } // end 'game loop
}
/// Build the default game: human team 0 vs CPU team 1.
fn build_default_game(seed: u64) -> GameState {
    build_default_game_opts(seed, true, true)
}

fn build_default_game_opts(seed: u64, with_mines: bool, with_barrels: bool) -> GameState {
    let mut terrain = Terrain::generate_tactical(seed);

    // Team 0 spawns in the left interior, team 1 in the right interior. The finder
    // keeps both teams ≥ SPAWN_EDGE_MARGIN from the world edges.
    let team0_spawns = terrain.find_team_spawns(0, WORLD_W / 2 - 40, 4);
    let team1_spawns = terrain.find_team_spawns(WORLD_W / 2 + 40, WORLD_W, 4);

    let teams = vec![
        Team::new(0, false, Difficulty::Medium, &team0_spawns),
        Team::new(1, false, Difficulty::Medium, &team1_spawns),
    ];

    let mut game = GameState::new(seed, terrain, teams, 2);
    if with_mines   { place_map_mines(&mut game); }
    if with_barrels { place_map_barrels(&mut game); }
    game
}

/// Deterministically place 9–15 armed mines across the map using the game seed.
fn place_map_mines(game: &mut GameState) {
    use game::state::{PlacedMine, MineState};
    let seed = game.map_seed;
    let mine_count = 9 + (seed % 7) as usize;  // 9–15

    // Simple LCG derived from seed
    let mut rng = seed.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
    let spread = WORLD_W / (mine_count as u32 + 1);

    for i in 1..=mine_count {
        rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
        let offset = (rng % spread as u64) as u32;
        let x = (spread * i as u32 + offset).clamp(20, WORLD_W - 20);
        if let Some(surf_y) = game.terrain.surface_y_at(x) {
            let mine_pos = WorldPos::new(x as f32, surf_y as f32 - 1.0);
            if (surf_y as f32) < crate::world::WATER_Y as f32 - 10.0
                && !too_close_to_soldiers(&game, mine_pos)
            {
                game.mines.push(PlacedMine {
                    pos: mine_pos,
                    state: MineState::Armed,
                    arm_ticks: 0,
                    trigger_ticks: 0,
                });
            }
        }
    }
}

/// Deterministically place 7–11 explosive barrels across the map using the game seed.
fn place_map_barrels(game: &mut game::state::GameState) {
    use game::state::{Barrel, BarrelState};
    let seed = game.map_seed;
    let count = 7 + (seed.wrapping_mul(0xDEAD_C0DE) % 5) as usize; // 7–11

    let mut rng = seed.wrapping_mul(0xBEEF_1234_5678_9ABCu64).wrapping_add(1442695040888963407);
    let spread = WORLD_W / (count as u32 + 1);

    for i in 1..=count {
        rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
        let offset = (rng % spread as u64) as u32;
        let x = (spread * i as u32 + offset).clamp(20, WORLD_W - 20);
        if let Some(surf_y) = game.terrain.surface_y_at(x) {
            // Barrel footprint: 10px above and below pos (half-height = 10).
            // Place pos so the bottom edge (pos.y + 10) sits 1px above the surface.
            let pos = WorldPos::new(x as f32, surf_y as f32 - 11.0);
            if (surf_y as f32) < crate::world::WATER_Y as f32 - 10.0
                && !too_close_to_soldiers(game, pos)
            {
                game.barrels.push(Barrel {
                    pos,
                    vel: crate::world::Vec2::new(0.0, 0.0),
                    hp: 60,
                    state: BarrelState::Normal,
                });
            }
        }
    }
}

/// Returns true if `pos` is within the mine's safe-spawn exclusion radius of any soldier.
/// Uses 50px — covers trigger radius (20) + blast radius (25) + margin.
fn too_close_to_soldiers(game: &game::state::GameState, pos: WorldPos) -> bool {
    const EXCLUSION: f32 = 50.0;
    game.teams.iter().flat_map(|t| t.soldiers.iter()).any(|s| {
        let dx = s.pos.x - pos.x;
        let dy = s.pos.y - pos.y;
        (dx * dx + dy * dy).sqrt() < EXCLUSION
    })
}

/// Seed from system time.
fn current_time_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64) // nanosecond precision — no same-seed repeat on quick restart
        .unwrap_or(12345)
}

fn run_multiplayer(_fb: &mut renderer::Framebuffer, _input: &mut input::InputState, _buf: &mut renderer::WorldBuffer) {}

/// Ranked TAT — same as casual but passes ranked=true to the lobby.
fn run_take_a_turn_ranked(fb: &mut renderer::Framebuffer, input: &mut input::InputState, buf: &mut WorldBuffer) {
    run_take_a_turn_impl(fb, input, buf, true);
}

/// Show the roster picker and return the chosen roster, or None if cancelled.
fn show_roster_picker(
    fb:      &mut renderer::Framebuffer,
    input:   &mut input::InputState,
    buf:     &mut WorldBuffer,
    rosters: &[game::account::Roster],
    token:   &str,
) -> Option<game::account::Roster> {
    use game::account::{RosterPicker, RosterAction};
    let mut rs = if rosters.is_empty() {
        vec![game::account::Roster::default_named(0)]
    } else {
        rosters.to_vec()
    };
    // list_only: skip auto-opening editor so first A press selects immediately
    let mut picker = RosterPicker::new_list_only(token.to_string(), rs);
    loop {
        let fs = std::time::Instant::now();
        input.poll();
        buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, renderer::Bgra::new(8, 8, 20));
        match picker.update(input, buf, 0) {
            Some(RosterAction::Selected(r)) => return Some(r),
            Some(RosterAction::Skip)        => return Some(game::account::Roster::default_named(0)),
            Some(RosterAction::Back)        => return None,
            None => {}
        }
        buf.blit_to_fb(fb, 0);
        let e = fs.elapsed();
        if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
    }
}

fn show_my_teams_menu(
    fb:      &mut renderer::Framebuffer,
    input:   &mut input::InputState,
    buf:     &mut WorldBuffer,
    rosters: &[game::account::Roster],
    token:   &str,
) {
    use renderer::Bgra;
    use renderer::font::{draw_str_scaled, str_width_scaled, draw_str, str_width};
    use world::{SCREEN_W, SCREEN_H};

    const ITEMS: &[&str] = &["ROSTERS", "STORE", "EQUIP"];
    let mut cursor = 0usize;

    loop {
        let fs = std::time::Instant::now();
        input.poll();

        if input.just_pressed(input::Button::B) { return; }
        let n = ITEMS.len();
        if input.just_pressed(input::Button::Up)   { cursor = if cursor == 0 { n - 1 } else { cursor - 1 }; }
        if input.just_pressed(input::Button::Down) { cursor = (cursor + 1) % n; }

        if input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start) {
            match cursor {
                0 => { show_roster_picker(fb, input, buf, rosters, token); }
                1 => { show_store_screen(fb, input, buf, token); }
                2 => { show_equip_screen(fb, input, buf, rosters, token); }
                _ => {}
            }
        }

        // Draw — same layout as title submenus
        renderer::title_bg::draw_title_bg(buf, 0);
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        let panel_y = 281i32;
        let item_h  = 38i32;
        let label   = "MY TEAMS";
        let lw = str_width_scaled(label, 2);
        draw_str_scaled(buf, label, sw/2 - lw/2 + 1, panel_y + 9,  Bgra::new(0, 0, 0), 2);
        draw_str_scaled(buf, label, sw/2 - lw/2,     panel_y + 8,  Bgra::new(200, 200, 230), 2);

        let start_y = panel_y + 32;
        for (i, &item) in ITEMS.iter().enumerate() {
            let iy = start_y + i as i32 * item_h;
            let iw = str_width_scaled(item, 2);
            let selected = i == cursor;
            if selected {
                buf.fill_rect(sw/2 - 155, iy - 4, 310, 28, Bgra::new(20, 30, 70));
                buf.fill_rect(sw/2 - 155, iy - 4, 3,   28, Bgra::new(255, 180, 0));
            }
            let col    = if selected { Bgra::new(255, 225, 55) } else { Bgra::new(0, 0, 0) };
            let shadow = if selected { Bgra::new(0, 0, 0) } else { Bgra::new(200, 200, 200) };
            if selected {
                draw_str_scaled(buf, ">", sw/2 - iw/2 - 25, iy + 1, Bgra::new(0,0,0), 2);
                draw_str_scaled(buf, ">", sw/2 - iw/2 - 24, iy,     Bgra::new(255, 180, 0), 2);
            }
            draw_str_scaled(buf, item, sw/2 - iw/2 + 1, iy + 1, shadow, 2);
            draw_str_scaled(buf, item, sw/2 - iw/2,     iy,     col,    2);
        }
        let hint = "A=SELECT  B=BACK";
        draw_str(buf, hint, sw/2 - str_width(hint)/2, sh - 18, Bgra::new(100, 100, 140));

        buf.blit_to_fb(fb, 0);
        let e = fs.elapsed();
        if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
    }
}

fn show_store_screen(
    fb:    &mut renderer::Framebuffer,
    input: &mut input::InputState,
    buf:   &mut WorldBuffer,
    token: &str,
) {
    use game::account::fetch_profile;
    use game::store::{StoreScreen, StoreAction};

    draw_status(buf, fb, "LOADING...");

    let (balance, owned_hats, owned_guns, owned_uniforms, owned_boots) =
        fetch_profile(token).unwrap_or((0, vec![], vec![], vec![], vec![]));

    let mut screen = StoreScreen::new(
        token.to_string(),
        balance,
        &owned_hats,
        &owned_guns,
        &owned_uniforms,
        &owned_boots,
    );

    loop {
        let fs = std::time::Instant::now();
        input.poll();
        buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, renderer::Bgra::new(8, 12, 28));
        match screen.update(input, buf) {
            Some(StoreAction::Back) => return,
            None => {}
        }
        buf.blit_to_fb(fb, 0);
        let e = fs.elapsed();
        if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
    }
}

fn show_equip_screen(
    fb:      &mut renderer::Framebuffer,
    input:   &mut input::InputState,
    buf:     &mut WorldBuffer,
    rosters: &[game::account::Roster],
    token:   &str,
) {
    use game::account::{CosmeticsScreen, CosmeticsAction, fetch_profile, save_cached_rosters};

    if rosters.is_empty() { return; }

    draw_status(buf, fb, "LOADING...");
    let profile = fetch_profile(token);
    if profile.is_none() {
        draw_status(buf, fb, "COULD NOT LOAD - CHECK CONNECTION");
        std::thread::sleep(std::time::Duration::from_secs(2));
        return;
    }
    let (_, owned_hats, owned_guns, owned_uniforms, owned_boots) = profile.unwrap();

    // If only one roster, use it directly; otherwise let player pick
    let chosen = if rosters.len() == 1 {
        Some(rosters[0].clone())
    } else {
        show_roster_picker(fb, input, buf, rosters, token)
    };

    let roster = match chosen { Some(r) => r, None => return };

    let mut screen = CosmeticsScreen::new(
        roster, owned_hats, owned_guns, owned_uniforms, owned_boots,
    );

    loop {
        let fs = std::time::Instant::now();
        input.poll();
        buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, renderer::Bgra::new(8, 10, 22));
        match screen.update(input, buf) {
            Some(CosmeticsAction::Back) => return,
            Some(CosmeticsAction::Saved(r)) => {
                // Post updated cosmetics to server
                let h = r.hat_ids;
                let u = r.uniform_color_ids;
                let b = r.boot_color_ids;
                let g = r.gun_style_ids;
                let tok = token.to_string();
                let body = format!(
                    r#"{{"token":"{}","id":{},"name":{},"worm_names":[{},{},{},{}],"hat_ids":[{},{},{},{}],"uniform_color_ids":[{},{},{},{}],"boot_color_ids":[{},{},{},{}],"gun_style_ids":[{},{},{},{}]}}"#,
                    tok, r.id, game::account::json_str(&r.name),
                    game::account::json_str(&r.worm_names[0]),
                    game::account::json_str(&r.worm_names[1]),
                    game::account::json_str(&r.worm_names[2]),
                    game::account::json_str(&r.worm_names[3]),
                    h[0],h[1],h[2],h[3], u[0],u[1],u[2],u[3],
                    b[0],b[1],b[2],b[3], g[0],g[1],g[2],g[3]);
                std::thread::spawn(move || {
                    game::account::http_post("/api/rosters/update", &body).ok();
                });
                // Update local cache
                let mut updated = rosters.to_vec();
                if let Some(pos) = updated.iter().position(|x| x.id == r.id) {
                    updated[pos] = r;
                }
                save_cached_rosters(&updated);
                return;
            }
            None => {}
        }
        buf.blit_to_fb(fb, 0);
        let e = fs.elapsed();
        if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
    }
}

fn show_missions_screen(
    fb:    &mut renderer::Framebuffer,
    input: &mut input::InputState,
    buf:   &mut WorldBuffer,
    token: &str,
) {
    use game::missions::{MissionsScreen, MissionsAction};
    draw_status(buf, fb, "LOADING...");
    let mut screen = MissionsScreen::new(token.to_string());
    screen.load();
    loop {
        let fs = std::time::Instant::now();
        input.poll();
        buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, renderer::Bgra::new(8, 10, 22));
        if let Some(MissionsAction::Back) = screen.update(input, buf) { return; }
        buf.blit_to_fb(fb, 0);
        let e = fs.elapsed();
        if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
    }
}

fn draw_status(buf: &mut renderer::WorldBuffer, fb: &mut renderer::Framebuffer, msg: &str) {
    use renderer::Bgra;
    use renderer::font::{draw_str_scaled, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(8, 10, 22));
    let x = SCREEN_W as i32 / 2 - str_width_scaled(msg, 2) / 2;
    let y = SCREEN_H as i32 / 2 - 8;
    draw_str_scaled(buf, msg, x, y, Bgra::new(255, 210, 50), 2);
    buf.blit_to_fb(fb, 0);
}

fn show_login_bonus(
    buf:     &mut renderer::WorldBuffer,
    fb:      &mut renderer::Framebuffer,
    input:   &mut input::InputState,
    earned:  u32,
    weekly:  u32,
) {
    use renderer::Bgra;
    use renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    let sw = SCREEN_W as i32;
    let sh = SCREEN_H as i32;
    let cx = sw / 2;

    let bg       = Bgra::new(8, 10, 22);
    let gold     = Bgra::new(255, 210, 50);
    let gold_dim = Bgra::new(180, 140, 20);
    let teal     = Bgra::new(80, 220, 200);
    let white    = Bgra::new(220, 220, 220);
    let panel_bg = Bgra::new(18, 22, 48);
    let bar_col  = if weekly > 0 { Bgra::new(200, 100, 255) } else { teal };

    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, bg);

    // Top accent bar
    buf.fill_rect(0, 0, SCREEN_W, 6, bar_col);

    // Card panel
    let pw = 280i32; let ph = 180i32;
    let px = cx - pw / 2; let py = sh / 2 - ph / 2 - 10;
    buf.fill_rect(px - 2, py - 2, (pw + 4) as u32, (ph + 4) as u32, bar_col);
    buf.fill_rect(px, py, pw as u32, ph as u32, panel_bg);

    // Title
    let title = if weekly > 0 { "WEEKLY STREAK!" } else { "DAILY LOGIN" };
    let tx = cx - str_width_scaled(title, 2) / 2;
    draw_str_scaled(buf, title, tx, py + 14, bar_col, 2);

    // Divider
    buf.fill_rect(px + 16, py + 36, (pw - 32) as u32, 1, gold_dim);

    // Big gear icon (centred, large)
    let gx = cx; let gy = py + 80;
    let gear_col = gold;
    let gear_bg  = panel_bg;
    buf.fill_circle(gx, gy, 22, gear_col);
    // 4 teeth N/S/E/W
    buf.fill_rect(gx - 5, gy - 30, 11, 10, gear_col);
    buf.fill_rect(gx - 5, gy + 20, 11, 10, gear_col);
    buf.fill_rect(gx - 30, gy - 5, 10, 11, gear_col);
    buf.fill_rect(gx + 20, gy - 5, 10, 11, gear_col);
    // Hollow centre
    buf.fill_circle(gx, gy, 10, gear_bg);

    // Scrap amount — large, to the right of the gear
    let amount = format!("+{}", earned);
    let ax = gx + 38;
    let ay = gy - str_width_scaled(&amount, 3) / 2 - 4; // rough vertical centre
    draw_str_scaled(buf, &amount, ax, gy - 14, gold, 3);

    // "SCRAP" label below amount
    let scrap_lbl = "SCRAP";
    draw_str_scaled(buf, scrap_lbl, ax, gy + 10, gold_dim, 2);

    // Bottom hint
    let hint = "PRESS A TO CONTINUE";
    let hx = cx - str_width(hint) / 2;
    draw_str(buf, hint, hx, py + ph - 20, white);

    // Bottom accent bar
    buf.fill_rect(0, sh - 6, SCREEN_W, 6, bar_col);

    buf.blit_to_fb(fb, 0);

    // Wait up to 3s or A press
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        input.poll();
        if input.just_pressed(input::Button::A) || std::time::Instant::now() >= deadline { break; }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

/// Full-screen match intro: shows both teams side-by-side with avatars and ELO.
/// Dismisses on A/Start or after 3 seconds.
fn show_match_intro(
    fb:      &mut Framebuffer,
    buf:     &mut WorldBuffer,
    input:   &mut input::InputState,
    game:    &game::state::GameState,
    my_team: usize,
) {
    use renderer::Bgra;
    use renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
    use renderer::draw_sprites::TEAM_COLOURS;
    use renderer::avatar::draw_avatar;
    use world::{SCREEN_W, SCREEN_H};

    let sw  = SCREEN_W as i32;
    let sh  = SCREEN_H as i32;
    let mid = sw / 2;
    const AV: u32 = 80;

    for tick in 0u32..90 {
        input.poll();
        if input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start) { break; }

        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(8, 10, 22));

        // Header bar
        buf.fill_rect(0, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
        let title = "MATCH";
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, mid - tw/2, 12, Bgra::new(255, 210, 50), 2);

        for ti in 0..2usize {
            let t   = &game.teams[ti];
            let col = TEAM_COLOURS[ti.min(3)];
            let hx  = if ti == 0 { mid / 2 } else { mid + mid / 2 };

            // Avatar
            let av_x = hx - AV as i32 / 2;
            let av_y = 70i32;
            draw_avatar(buf, av_x, av_y, AV, t.avatar_id);

            // Team colour bar under avatar
            buf.fill_rect(av_x, av_y + AV as i32 + 2, AV, 3, col);

            // Team name
            let name = t.name.to_uppercase();
            let nw = str_width_scaled(&name, 2);
            draw_str_scaled(buf, &name, hx - nw/2, av_y + AV as i32 + 12, col, 2);

            // ELO (ranked only)
            if t.elo > 0 {
                let elo_str = format!("ELO  {}", t.elo);
                let ew = str_width(&elo_str);
                draw_str(buf, &elo_str, hx - ew/2, av_y + AV as i32 + 30, Bgra::new(180, 180, 100));
            }

            // YOU indicator
            if ti == my_team {
                let you = "YOU";
                let yw = str_width(you);
                draw_str(buf, you, hx - yw/2, av_y - 16, Bgra::new(100, 220, 100));
            }
        }

        // VS
        let vs = "VS";
        let vw = str_width_scaled(vs, 3);
        draw_str_scaled(buf, vs, mid - vw/2, sh / 2 - 14, Bgra::new(255, 255, 255), 3);

        // Countdown bar
        let filled = (sw * (89i32 - tick as i32) / 89).max(0) as u32;
        buf.fill_rect(0, sh - 5, SCREEN_W, 5, Bgra::new(25, 25, 40));
        buf.fill_rect(0, sh - 5, filled, 5, Bgra::new(70, 70, 140));

        buf.blit_to_fb(fb, 0);
        std::thread::sleep(TICK_DURATION);
    }
}

fn draw_msg(buf: &mut WorldBuffer, fb: &mut Framebuffer, msg: &str) {
    use renderer::Bgra;
    use renderer::font::{draw_str_scaled, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(8, 10, 22));
    let x = SCREEN_W as i32 / 2 - str_width_scaled(msg, 2) / 2;
    let y = SCREEN_H as i32 / 2 - 8;
    draw_str_scaled(buf, msg, x, y, Bgra::new(255, 210, 50), 2);
    buf.blit_to_fb(fb, 0);
}


fn apply_server_state(
    game:    &mut game::state::GameState,
    _cam:    &mut renderer::Camera,
    state:   &net::msg::StateMsg,
    my_team: usize,
) {
    // Soldiers that newly died this state update — death messages are generated
    // client-side (server only has default names; the client uses the names it
    // actually displays). Death SFX still come from StateMsg.sounds.
    let mut new_deaths: Vec<(String, usize, usize, u8)> = Vec::new();
    for ns in &state.soldiers {
        if let Some(team) = game.teams.get_mut(ns.team) {
            if let Some(soldier) = team.soldiers.get_mut(ns.index) {
                let was_alive = soldier.hp > 0;
                if ns.dead && was_alive {
                    new_deaths.push((soldier.name.clone(), ns.team, ns.index, ns.death_cause_u8));
                }
                if ns.hp < soldier.hp { soldier.hp_display_ticks = 150; }
                soldier.pos.x           = ns.x;
                soldier.pos.y           = ns.y;
                soldier.hp              = ns.hp;
                soldier.facing          = ns.facing;
                soldier.has_fired       = ns.has_fired;
                soldier.airtime         = ns.airtime;
                soldier.walk_ticks      = ns.walk_ticks;
                // Sync opponent cosmetics and names (local player's own are set from roster at game start)
                if ns.team != my_team {
                    soldier.hat_id           = ns.hat_id;
                    soldier.uniform_color_id = ns.uniform_color_id;
                    soldier.boot_color_id    = ns.boot_color_id;
                    soldier.gun_style_id     = ns.gun_style_id;
                    soldier.name             = ns.name.clone();
                }
                use game::soldier::SoldierState;
                soldier.state = if ns.dead {
                    SoldierState::Dead
                } else if ns.airborne {
                    SoldierState::Airborne { vel: world::Vec2::new(0.0, 0.0), spinning: ns.spinning }
                } else if ns.walking {
                    SoldierState::Walking { dir: ns.facing as f32 }
                } else {
                    SoldierState::Idle
                };
            }
        }
        // Sync opponent team's selected weapon for correct gun visual
        if ns.team != my_team {
            if let Some(team) = game.teams.get_mut(ns.team) {
                team.selected_weapon = ns.selected_weapon;
            }
        }
    }
    // Push client-side death messages for soldiers that just died (parity with
    // local modes; uses the names the client displays + the networked cause).
    for (name, team, idx, cause_u8) in new_deaths {
        use game::soldier::DeathCause;
        let cause = match cause_u8 {
            1 => DeathCause::Explosion, 2 => DeathCause::Fall,
            3 => DeathCause::Water, _ => DeathCause::Generic,
        };
        let seed = game.tick.wrapping_mul(1664525)
            .wrapping_add(team as u32 * 7).wrapping_add(idx as u32 * 13);
        let phrase = game::loop_runner::death_phrase(cause, seed);
        game.messages.push(game::state::GameMessage {
            text: format!("{} {}", name, phrase), team: Some(team), ticks: 120,
        });
    }
    game.projectiles.clear();
    for np in &state.projectiles {
        use crate::physics::projectile::{Projectile, WeaponKind, FuseState};
        use crate::world::{Vec2, WorldPos};
        let kind = WeaponKind::from_net_u8(np.kind_u8);
        let mut proj = Projectile::new(WorldPos::new(np.x, np.y), Vec2::new(np.vel_x, np.vel_y), kind);
        if np.fuse_ticks > 0 {
            proj.fuse = FuseState::Burning(np.fuse_ticks);
        }
        proj.is_fragment = np.is_fragment;
        game.projectiles.push(proj);
    }
    // On opponent's turn, apply their aim angle from server state for display.
    // On our turn, keep local angle (managed by process_aim() for smooth no-RTT aiming).
    if state.turn_team != my_team {
        game.aim.angle = state.aim_angle;
    }
    game.aim.power          = state.aim_power;
    // aim_fuse_ticks, weapon_menu_open/cursor, selected_weapon managed locally.
    // Detect turn change for message queue
    let prev_team   = game.turn.current_team;
    let prev_active = game.teams.get(prev_team).map(|t| t.active).unwrap_or(0);

    game.wind               = crate::physics::Wind::new(state.wind);
    game.turn.ticks_left = state.turn_secs * 30;
    game.turn.current_team = state.turn_team;

    if state.turn_team != prev_team || state.active_soldier != prev_active {
        game::loop_runner::push_turn_message(game);
    }
    if let Some(t) = game.teams.get_mut(state.turn_team) { t.active = state.active_soldier; }
    // Snap the camera to the new active soldier on a turn change so it doesn't
    // lerp/shake across the map from the previous turn's framing.
    if state.turn_team != prev_team || state.active_soldier != prev_active {
        if let Some(s) = game.teams.get(state.turn_team).and_then(|t| t.soldiers.get(state.active_soldier)) {
            _cam.snap_to(s.pos);
        }
    }
    game.turn.phase = match state.phase {
        net::msg::NetPhase::Acting       => crate::game::turn::TurnPhase::Acting,
        net::msg::NetPhase::Watching     => crate::game::turn::TurnPhase::Watching,
        net::msg::NetPhase::Retreating   => crate::game::turn::TurnPhase::Retreating { ticks_left: 30 },
        net::msg::NetPhase::Ending       => crate::game::turn::TurnPhase::Ending,
    };
    // Sync game result from server
    game.result = match state.result {
        net::msg::NetResult::Ongoing   => crate::game::state::GameResult::Ongoing,
        net::msg::NetResult::Winner(t) => crate::game::state::GameResult::Winner(t),
        net::msg::NetResult::Draw      => crate::game::state::GameResult::Draw,
    };
    // Sync crate positions from server (server is authoritative on drops/collection)
    game.crates = state.crates.iter().map(|nc| {
        use crate::game::state::CrateKind;
        use crate::physics::projectile::WeaponKind;
        crate::game::state::DroppedCrate {
            pos:        crate::world::WorldPos::new(nc.x, nc.y),
            // Variant only drives the rendered colour/symbol; payload is a
            // placeholder (server owns the real contents on collection).
            kind:       match nc.kind_u8 {
                1 => CrateKind::Weapon(WeaponKind::Bazooka),
                2 => CrateKind::Scrap(0),
                _ => CrateKind::Health,
            },
            landed:     nc.landed,
            descent_vy: 1.5,
            damage_this_turn: 0,
            fall_ticks: 0,
        }
    }).collect();

    // Sync mines (server-authoritative: positions, states, countdowns)
    game.mines = state.mines.iter().map(|nm| {
        use crate::game::state::{PlacedMine, MineState};
        use crate::world::WorldPos;
        PlacedMine {
            pos: WorldPos::new(nm.x, nm.y),
            state: match nm.state_u8 {
                2 => MineState::Triggered,
                1 => MineState::Armed,
                _ => MineState::Arming,
            },
            arm_ticks: nm.arm_ticks,
            trigger_ticks: nm.trigger_ticks,
        }
    }).collect();

    // Plasma torch: reconstruct the active torch state from the networked direction
    // so the live client draws the flame at the tip (5c in render). While the torch
    // is active, its terrain carving produces a stream of craters every tick — DON'T
    // spawn explosion flashes for those (they'd flash over the soldier/body carve).
    use crate::game::state::{PlasmaTorchState, TorchDir};
    let torch_active = state.torch_dir != 0;
    game.plasma_torch = match state.torch_dir {
        1 => Some(PlasmaTorchState { dir: TorchDir::UpForward,   fuel_ticks: 1 }),
        2 => Some(PlasmaTorchState { dir: TorchDir::Forward,     fuel_ticks: 1 }),
        3 => Some(PlasmaTorchState { dir: TorchDir::DownForward, fuel_ticks: 1 }),
        _ => None,
    };

    let known = game.crater_log.len();
    for nc in state.craters.iter().skip(known) {
        use crate::world::{Crater, WorldPos};
        Crater::new(nc.cx, nc.cy, nc.radius).carve(&mut game.terrain);
        game.crater_log.push((nc.cx, nc.cy, nc.radius));
        if !torch_active {
            game.explosions.push(crate::game::state::Explosion::new(
                WorldPos::new(nc.cx, nc.cy), nc.radius,
            ));
        }
    }

    // Sync headstones (server-authoritative; settled server-side). Rendering only
    // uses pos/team/headstone_id, so the other fields are placeholders.
    game.graves = state.graves.iter().map(|ng| crate::game::state::Grave {
        pos:          crate::world::WorldPos::new(ng.x, ng.y),
        team:         ng.team,
        soldier_idx:  0,
        died_tick:    0,
        vel_y:        0.0,
        settled:      true,
        headstone_id: ng.headstone_id,
    }).collect();

    // Sync blood splats (server-authoritative; decayed server-side).
    game.blood_splats = state.blood_splats.iter()
        .map(|nb| (crate::world::WorldPos::new(nb.x, nb.y), nb.ticks))
        .collect();

    // Sync barrels (positions + hp from server)
    {
        use crate::game::state::{Barrel, BarrelState};
        use crate::world::{Vec2, WorldPos};
        game.barrels = state.barrels.iter().map(|nb| Barrel {
            pos: WorldPos::new(nb.x, nb.y),
            vel: Vec2::new(0.0, 0.0),
            hp: nb.hp,
            state: BarrelState::Normal,
        }).collect();
    }

    // Sync black holes
    {
        use crate::game::state::BlackHole;
        use crate::world::WorldPos;
        game.black_holes = state.black_holes.iter().map(|nb| BlackHole {
            pos: WorldPos::new(nb.x, nb.y),
            lifetime: nb.ticks_left,
        }).collect();
    }

    // Sync fire patches
    {
        use crate::game::state::FirePatch;
        use crate::world::{Vec2, WorldPos};
        game.fire_patches = state.fire_patches.iter().map(|nf| FirePatch {
            pos: WorldPos::new(nf.x, nf.y),
            vel: Vec2::new(nf.vel_x, nf.vel_y),
            landed: nf.landed,
            lifetime: nf.lifetime,
        }).collect();
    }

    // Sync rope (rendering only — physics runs on server)
    {
        use crate::game::state::RopeState;
        use crate::world::WorldPos;
        game.rope = state.rope.as_ref().map(|nr| RopeState {
            anchor:   WorldPos::new(nr.anchor_x, nr.anchor_y),
            hook:     WorldPos::new(nr.hook_x,   nr.hook_y),
            flying:   nr.flying,
            length:   nr.length,
            hook_vel: crate::world::Vec2::new(0.0, 0.0),
        });
    }

    // Sync opponent team name
    if let Some(opp_team) = game.teams.get_mut(1 - my_team) {
        if !state.opp_team_name.is_empty() {
            opp_team.name = state.opp_team_name.clone();
        }
    }

    // Sync Garcia (Hand of Jerry)
    {
        use crate::game::state::GarciaState;
        game.garcia = state.garcia.as_ref().map(|ng| GarciaState {
            cursor_x: ng.cursor_x, render_x: ng.render_x, blink_timer: ng.blink_timer,
            falling: ng.falling, fall_y: ng.fall_y, vel_y: ng.vel_y, bounce_count: ng.bounce_count,
        });
    }
}



fn download_update(buf: &mut WorldBuffer, fb: &mut Framebuffer) {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    draw_msg(buf, fb, "DOWNLOADING UPDATE...");

    let host = "crumbonium.duckdns.org";
    let req = format!(
        "GET /arty/arty HTTP/1.0
Host: {}
Connection: close

",
        host
    );

    let mut stream = match TcpStream::connect((host, 80u16)) {
        Ok(s) => s,
        Err(_) => { draw_msg(buf, fb, "UPDATE FAILED"); std::thread::sleep(std::time::Duration::from_secs(2)); return; }
    };
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30))).ok();
    if stream.write_all(req.as_bytes()).is_err() { return; }

    let mut response = Vec::new();
    if stream.read_to_end(&mut response).is_err() { return; }

    // Find body after blank line
    let sep = b"\r\n\r\n";
    let body_start = match response.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(i) => i + 4,
        None => return,
    };
    let binary = &response[body_start..];

    // Write to temp file then replace
    let tmp = "/mnt/SDCARD/App/Arty/arty.new";
    let dest = "/mnt/SDCARD/App/Arty/arty";
    {
        let mut f = match std::fs::File::create(tmp) {
            Ok(f) => f,
            Err(_) => { draw_msg(buf, fb, "UPDATE FAILED"); std::thread::sleep(std::time::Duration::from_secs(2)); return; }
        };
        if f.write_all(binary).is_err() { return; }
    }

    // chmod +x and replace
    unsafe { libc::chmod(tmp.as_ptr() as *const libc::c_char, 0o755); }
    if std::fs::rename(tmp, dest).is_err() { return; }

    draw_msg(buf, fb, "UPDATE DONE - RESTARTING...");
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Re-exec self
    let path = std::ffi::CString::new(dest).unwrap();
    unsafe { libc::execv(path.as_ptr(), std::ptr::null()); }
}

fn run_take_a_turn(fb: &mut renderer::Framebuffer, input: &mut input::InputState, buf: &mut WorldBuffer) {
    run_take_a_turn_impl(fb, input, buf, false);
}

fn run_take_a_turn_impl(fb: &mut renderer::Framebuffer, input: &mut input::InputState, buf: &mut WorldBuffer, ranked: bool) {
    use game::account::{AccountScreen, AccountAction, RosterPicker, RosterAction, load_saved_creds};

    // ── Login ────────────────────────────────────────────────────────────────
    let (token, username, mut rosters) = if let Some((u, t)) = load_saved_creds() {
        let r = game::account::load_cached_rosters();
        (t, u, r)
    } else {
        let mut acct = AccountScreen::new();
        let result = loop {
            let fs = std::time::Instant::now();
            input.poll();
            buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, renderer::Bgra::new(8, 8, 20));
            if let Some(a) = acct.update(input, buf, 0) { break a; }
            buf.blit_to_fb(fb, 0);
            let e = fs.elapsed(); if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
        };
        match result {
            AccountAction::LoggedIn { token, username, rosters } => (token, username, rosters),
            AccountAction::Back => return,
        }
    };

    if rosters.is_empty() { rosters.push(game::account::Roster::default_named(0)); }

    // Daily login bonus — silent if already claimed today, presentable screen if new
    if let Some((earned, weekly)) = game::account::claim_daily_login(&token) {
        show_login_bonus(buf, fb, input, earned, weekly);
    }

    // Helper: pick/load roster for a specific match (locked after first selection)
    // my_slot + moves_len determine if this is the player's very first turn.
    // p0's first turn: moves_len == 0; p1's first turn: moves_len == 1.
    // Beyond that the match is already in progress — never show the roster picker again.
    let pick_roster_for_match = |fb: &mut renderer::Framebuffer, input: &mut input::InputState,
                                  buf: &mut WorldBuffer, match_id: i64, rosters: &[game::account::Roster],
                                  token: &str, my_slot: usize, moves_len: usize| -> Option<game::account::Roster> {
        // Already chose one for this match → use it, no picker shown
        if let Some(r) = game::account::load_match_roster(match_id) { return Some(r); }
        // Mid-match (past first turn) and local save is gone (e.g. reboot) →
        // auto-select first roster without showing picker so the team can't be changed.
        let is_first_turn = moves_len == my_slot; // 0 for p0, 1 for p1
        if !is_first_turn {
            let r = rosters.first().cloned()
                .unwrap_or_else(|| game::account::Roster::default_named(0));
            game::account::save_match_roster(match_id, &r);
            return Some(r);
        }
        // First turn: show picker so player can choose their team
        let mut rs = rosters.to_vec();
        if rs.is_empty() { rs.push(game::account::Roster::default_named(0)); }
        let mut picker = RosterPicker::new(token.to_string(), rs);
        let picked = loop {
            let fs = std::time::Instant::now();
            input.poll();
            buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, renderer::Bgra::new(8, 8, 20));
            if let Some(a) = picker.update(input, buf, 0) { break a; }
            buf.blit_to_fb(fb, 0);
            let e = fs.elapsed(); if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
        };
        let r = match picked {
            RosterAction::Selected(r) => r,
            RosterAction::Skip => game::account::Roster::default_named(0),
            RosterAction::Back => return None,
        };
        game::account::save_match_roster(match_id, &r);
        Some(r)
    };

    // ── Match lobby ──────────────────────────────────────────────────────────
    use game::lobby::{LobbyScreen, LobbyAction};
    let mut lobby = LobbyScreen::new_ranked(token.clone(), username.clone(), VERSION, ranked);
    loop {
        let frame_start = std::time::Instant::now();
        input.poll();
        buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, renderer::Bgra::new(8, 8, 20));
        match lobby.update(input, buf, 0) {
            Some(LobbyAction::Back) => break,
            Some(LobbyAction::LoggedOut) => {
                // Re-show account screen
                use game::account::{AccountScreen, AccountAction};
                let mut acct = AccountScreen::new();
                let result = loop {
                    let fs = std::time::Instant::now();
                    input.poll();
                    if let Some(a) = acct.update(input, buf, 0) { break a; }
                    buf.blit_to_fb(fb, 0);
                    let e = fs.elapsed();
                    if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
                };
                match result {
                    AccountAction::LoggedIn { token: t2, username: u2, .. } => {
                        lobby = LobbyScreen::new(t2, u2, VERSION);
                    }
                    AccountAction::Back => break,
                }
            }
            Some(LobbyAction::StartMatch { match_id, seed, my_slot, moves, my_elo, opp_elo, has_mines, has_barrels, opp_name, opp_worm_names, opp_hat_ids, opp_uniform_color_ids, opp_boot_color_ids, opp_gun_style_ids, days_remaining }) => {
                let roster_for_match = pick_roster_for_match(fb, input, buf, match_id, &rosters, &token, my_slot, moves.len());
                let selected_roster = match roster_for_match {
                    Some(r) => r,
                    None => { continue; } // user pressed Back from roster picker
                };
                if let Some((new_move, tat_kills, tat_deaths, tat_weapon_kills)) = run_tat_game(fb, input, buf, seed, my_slot, &moves, &selected_roster.name, selected_roster.avatar_id, selected_roster.headstone_id, &selected_roster.worm_names, &selected_roster.hat_ids, &selected_roster.uniform_color_ids, &selected_roster.boot_color_ids, &selected_roster.gun_style_ids, my_elo, opp_elo, has_mines, has_barrels, &opp_name, &opp_worm_names, &opp_hat_ids, &opp_uniform_color_ids, &opp_boot_color_ids, &opp_gun_style_ids, days_remaining) {
                    // Submit move (with kill/death/weapon-kill stats) in background
                    use game::account::load_saved_creds;
                    if let Some((_, token)) = load_saved_creds() {
                        let inputs_json: String = new_move.inputs.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",");
                        let wk_json: String = {
                            let pairs: Vec<String> = tat_weapon_kills.iter()
                                .map(|(k, v)| format!(r#""{}":{}"#, k, v))
                                .collect();
                            format!("{{{}}}", pairs.join(","))
                        };
                        let body = format!(r#"{{"token":"{}","angle":{},"power":{},"facing":{},"active_soldier":{},"inputs":[{}],"kills":{},"deaths":{},"weapon_kills":{}}}"#,
                            token, new_move.angle, new_move.power, new_move.facing, new_move.active_soldier, inputs_json, tat_kills, tat_deaths, wk_json);
                        let url = format!("/api/match/{}/move", match_id);
                        std::thread::spawn(move || { game::account::http_post(&url, &body).ok(); });
                        // If the game ended this turn, also POST the match result
                        if tat_kills == 4 || tat_deaths == 4 {
                            let winner_slot = if tat_kills == 4 { my_slot } else { 1 - my_slot };
                            let result_body = format!(r#"{{"token":"{}","winner_slot":{},"kills":{},"deaths":{}}}"#,
                                token, winner_slot, tat_kills, tat_deaths);
                            let result_url = format!("/api/match/{}/result", match_id);
                            std::thread::spawn(move || { game::account::http_post(&result_url, &result_body).ok(); });
                        }
                    }
                    // Show confirmation screen for 3 seconds before returning to lobby
                    draw_msg(buf, fb, "MOVE SUBMITTED");
                    std::thread::sleep(std::time::Duration::from_millis(1200));
                }
                lobby = LobbyScreen::new(
                    load_saved_creds().map(|(_, t)| t).unwrap_or_default(),
                    load_saved_creds().map(|(u, _)| u).unwrap_or_default(),
                    VERSION,
                );
            }
            None => {}
        }
        buf.blit_to_fb(fb, 0);
        let e = frame_start.elapsed();
        if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
    }
}

fn run_tat_game(
    fb:          &mut renderer::Framebuffer,
    input:       &mut input::InputState,
    buf:         &mut WorldBuffer,
    seed:        u64,
    my_slot:     usize,
    moves:       &[game::lobby::Move],
    team_name:         &str,
    avatar_id:         u8,
    headstone_id:      u8,
    worm_names:        &[String; 4],
    hat_ids:           &[u8; 4],
    uniform_color_ids: &[u8; 4],
    boot_color_ids:    &[u8; 4],
    gun_style_ids:     &[u8; 4],
    my_elo:            i32,
    opp_elo:      i32,
    has_mines:    bool,
    has_barrels:  bool,
    opp_name:      &str,
    opp_worm_names:        &[String; 4],
    opp_hat_ids:           &[u8; 4],
    opp_uniform_color_ids: &[u8; 4],
    opp_boot_color_ids:    &[u8; 4],
    opp_gun_style_ids:     &[u8; 4],
    days_remaining: i32,
) -> Option<(game::lobby::Move, u32, u32, std::collections::HashMap<&'static str, u32>)> { // (move, kills, deaths, weapon_kills)
    use game::turn::TurnPhase;

    let mut game = build_default_game_opts(seed, has_mines, has_barrels);
    let mut lstate = game::loop_runner::LoopState::new();
    // Apply roster team name, avatar, soldier names, and ELO to the player's team
    game.teams[my_slot].name         = team_name.to_string();
    game.teams[my_slot].avatar_id    = avatar_id;
    game.teams[my_slot].headstone_id = headstone_id;
    for si in 0..game.teams[my_slot].soldiers.len().min(4) {
        game.teams[my_slot].soldiers[si].name             = worm_names[si].clone();
        game.teams[my_slot].soldiers[si].hat_id           = hat_ids[si];
        game.teams[my_slot].soldiers[si].uniform_color_id = uniform_color_ids[si];
        game.teams[my_slot].soldiers[si].boot_color_id    = boot_color_ids[si];
        game.teams[my_slot].soldiers[si].gun_style_id     = gun_style_ids[si];
    }
    if my_elo > 0 || opp_elo > 0 {
        game.teams[my_slot].elo        = my_elo  as u32;
        game.teams[1 - my_slot].elo    = opp_elo as u32;
    }
    // Apply opponent's username as their team name if provided
    if !opp_name.is_empty() {
        game.teams[1 - my_slot].name = opp_name.to_string();
    }
    // Apply opponent worm names and cosmetics
    let opp_slot = 1 - my_slot;
    let opp_team_name = game.teams[opp_slot].name.clone();
    for si in 0..game.teams[opp_slot].soldiers.len().min(4) {
        let name = opp_worm_names[si].trim();
        game.teams[opp_slot].soldiers[si].name = if name.is_empty() || name.starts_with("Soldier ") {
            format!("{} {}", opp_team_name, si + 1)
        } else {
            name.to_string()
        };
        game.teams[opp_slot].soldiers[si].hat_id           = opp_hat_ids[si];
        game.teams[opp_slot].soldiers[si].uniform_color_id = opp_uniform_color_ids[si];
        game.teams[opp_slot].soldiers[si].boot_color_id    = opp_boot_color_ids[si];
        game.teams[opp_slot].soldiers[si].gun_style_id     = opp_gun_style_ids[si];
    }
    // Drain any button held from lobby / roster picker before showing intro screens
    loop {
        input.poll();
        if !input.held(input::Button::A) && !input.held(input::Button::Start) { break; }
        std::thread::sleep(TICK_DURATION);
    }
    // Match intro (VS) screen — only at the very start of the match (the player's
    // first turn), never again after each subsequent turn.
    let is_first_turn = moves.len() == my_slot; // 0 for p0, 1 for p1
    if (my_elo > 0 || opp_elo > 0) && is_first_turn {
        show_match_intro(fb, buf, input, &game, my_slot);
    }
    // Snap all soldiers to surface to prevent fall damage on first tick
    for ti in 0..game.teams.len() {
        for si in 0..game.teams[ti].soldiers.len() {
            game::loop_runner::snap_to_surface(&mut game, ti, si);
        }
    }

    // All turns start at +FRAC_PI_4 (aimed upward). This matches what the player
    // sees on their first turn, and subsequent turns get this via the Ending handler.
    game.aim.angle = std::f32::consts::FRAC_PI_4;
    game.aim.power = 0.0;

    // Replay all previous moves
    let empty = InputState::new();
    let last_idx = moves.len().saturating_sub(1);
    for (i, mv) in moves.iter().enumerate() {
        let team = game.turn.current_team();
        let is_last = i == last_idx && !moves.is_empty();
        if is_last {
            // opp_slot is always 1-my_slot when it is our turn.
            // Force active team so fire_bazooka targets the right soldier.
            let opp_slot = 1 - my_slot;
            game.turn.current_team = opp_slot;
            // Use the recorded active soldier index so the replay fires from the exact right soldier.
            game.teams[opp_slot].active = mv.active_soldier;
            let opp_si = mv.active_soldier;
            game.teams[opp_slot].soldiers[opp_si].has_fired = false;
            if mv.inputs.is_empty() {
                game.teams[opp_slot].soldiers[opp_si].facing = mv.facing;
                game.aim.angle = mv.angle;
                game.aim.power = mv.power;
            }
            let mut replay_cam = renderer::Camera::new(game.teams[opp_slot].soldiers[opp_si].pos.x);
            replay_cam.snap_to(game.teams[opp_slot].soldiers[opp_si].pos);
            // Clear messages accumulated during fast-forward — only the live replay's messages should show
            game.messages.clear();
            // Static black "OPPONENT'S MOVE" screen for 4 seconds before replay begins
            crate::audio::set_muted(true);
            {
                use renderer::Bgra;
                use renderer::font::{draw_str_scaled, str_width_scaled};
                let sw  = crate::world::SCREEN_W as i32;
                let sh  = crate::world::SCREEN_H as i32;
                let msg = "OPPONENT'S MOVE";
                let mw  = str_width_scaled(msg, 2);
                let mx  = sw / 2 - mw / 2;
                let my  = sh / 2 - 8;
                buf.fill_rect(0, 0, sw as u32, sh as u32, Bgra::new(0, 0, 0));
                draw_str_scaled(buf, msg, mx + 1, my + 1, Bgra::new(0, 0, 0), 2);
                draw_str_scaled(buf, msg, mx,     my,     Bgra::new(255, 210, 50), 2);
                buf.blit_to_fb(fb, 0);
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
            crate::audio::set_muted(false);
            let mut prev_bits: u16 = 0;
            let input_len = mv.inputs.len();
            let mut replay_tick = 0usize;
            while replay_tick < input_len {
                let frame_start = std::time::Instant::now();
                let bits = mv.inputs[replay_tick];
                let tick_input = input::InputState::from_bits(prev_bits, bits);
                prev_bits = bits;
                // server_tick now emits detonation SFX itself (game.emit_sound),
                // which plays locally on this device — no external diff needed.
                game::loop_runner::server_tick(&mut game, &tick_input);
                game.messages.retain(|m| !m.text.contains("got a ") && !m.text.contains("picked up"));
                replay_tick += 1;
                if let Some(p) = game.projectiles.first() {
                    replay_cam.follow_always(p.pos);
                } else if let Some(ex) = game.explosions.last() {
                    replay_cam.follow_always(ex.pos);
                } else {
                    let airborne_pos = game.teams.iter().flat_map(|t| t.soldiers.iter())
                        .filter(|s| matches!(s.state, game::soldier::SoldierState::Airborne { .. }))
                        .map(|s| s.pos).next();
                    if let Some(pos) = airborne_pos { replay_cam.follow(pos); }
                    else { replay_cam.follow(game.teams[opp_slot].soldiers[game.teams[opp_slot].active].pos); }
                }
                replay_cam.tick();
                game::loop_runner::render(&game, buf, &mut replay_cam, &mut lstate);
                buf.blit_to_fb(fb, replay_cam.left_edge());
                let e = frame_start.elapsed();
                if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
                if matches!(game.turn.phase, TurnPhase::Acting) && game.projectiles.is_empty() && game.turn.current_team() != opp_slot { break; }
            }
            if !game.teams[opp_slot].soldiers[game.teams[opp_slot].active].has_fired {
                game::loop_runner::fire_bazooka_tat(&mut game);
            }
            for _ in 0..600 {
                let frame_start = std::time::Instant::now();
                // server_tick emits detonation SFX itself (plays locally here).
                game::loop_runner::server_tick(&mut game, &empty);
                game.messages.retain(|m| !m.text.contains("got a ") && !m.text.contains("picked up"));
                if let Some(p) = game.projectiles.first() {
                    replay_cam.follow_always(p.pos);
                } else if let Some(ex) = game.explosions.last() {
                    replay_cam.follow_always(ex.pos);
                } else {
                    let airborne_pos = game.teams.iter().flat_map(|t| t.soldiers.iter())
                        .filter(|s| matches!(s.state, game::soldier::SoldierState::Airborne { .. }))
                        .map(|s| s.pos).next();
                    if let Some(pos) = airborne_pos { replay_cam.follow(pos); }
                    else { replay_cam.follow(game.teams[opp_slot].soldiers[game.teams[opp_slot].active].pos); }
                }
                replay_cam.tick();
                game::loop_runner::render(&game, buf, &mut replay_cam, &mut lstate);
                buf.blit_to_fb(fb, replay_cam.left_edge());
                let e = frame_start.elapsed();
                if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
                if matches!(game.turn.phase, TurnPhase::Acting) && game.projectiles.is_empty() && game.turn.current_team() != opp_slot { break; }
            }
            // Snap camera so there's no lerp lag entering the player's turn
            let opp_si = game.teams[opp_slot].active;
            replay_cam.snap_to(game.teams[opp_slot].soldiers[opp_si].pos);
        } else {
            crate::audio::set_muted(true);
            let si = game.teams[team].active;
            game.teams[team].soldiers[si].has_fired = false;
            if mv.inputs.is_empty() {
                game.teams[team].soldiers[si].facing = mv.facing;
                game.aim.angle = mv.angle;
                game.aim.power = mv.power;
            }
            let mut prev_bits: u16 = 0;
            for &bits in &mv.inputs {
                let tick_input = input::InputState::from_bits(prev_bits, bits);
                prev_bits = bits;
                game::loop_runner::server_tick(&mut game, &tick_input);
                game.messages.retain(|m| !m.text.contains("got a ") && !m.text.contains("picked up"));
                if game.teams[team].soldiers[game.teams[team].active].has_fired { break; }
            }
            if !game.teams[team].soldiers[game.teams[team].active].has_fired {
                game::loop_runner::fire_bazooka_tat(&mut game);
            }
            // 800 < TURN_TICKS(1350): prevents double-advance if break misses
            for _ in 0..800 {
                game::loop_runner::server_tick(&mut game, &empty);
                if matches!(game.turn.phase, TurnPhase::Acting) && game.projectiles.is_empty() && game.turn.current_team() != team { break; }
            }
            crate::audio::set_muted(false);
        }
    }

    // Snapshot opponent alive state before player's turn for per-weapon kill delta
    let opp_alive_before: Vec<bool> = game.teams.get(1 - my_slot)
        .map(|t| t.soldiers.iter().map(|s| s.is_alive()).collect())
        .unwrap_or_default();

    // Set up for player turn
    game.turn.current_team = my_slot;
    game.turn.phase = game::turn::TurnPhase::Acting;
    game.turn.ticks_left = 1350;
    game.aim.angle = std::f32::consts::FRAC_PI_4;
    game.aim.power = 0.0;
    // server_tick() now honors crate_watch_ticks; clear any leftover hold from the
    // replay so the player's interactive turn isn't input-locked at the start.
    game.crate_watch_ticks = 0;
    { let si = game.teams[my_slot].active; game.teams[my_slot].soldiers[si].has_fired = false; game::loop_runner::snap_to_surface(&mut game, my_slot, si); }
    // Snap camera to active soldier
    let start_pos = game.teams[my_slot].soldiers[game.teams[my_slot].active].pos;
    let mut cam = renderer::Camera::new(start_pos.x);
    cam.snap_to(start_pos);

    // Drain any A-held state carried over from the lobby (match confirmation uses A).
    loop { input.poll(); if !input.held(input::Button::A) { break; } std::thread::sleep(TICK_DURATION); }

    // Player takes their turn interactively, including retreat after firing.
    let mut recorded_inputs: Vec<u16> = Vec::new();
    let mut fired = false;
    let mut pre_angle = game.aim.angle;
    let mut pre_power = game.aim.power;
    let mut pre_active = game.teams[my_slot].active;
    let mut pre_facing = game.teams[my_slot].soldiers[pre_active].facing;
    loop {
        let frame_start = std::time::Instant::now();
        input.poll();
        if !fired {
            pre_angle = game.aim.angle;
            pre_power = game.aim.power;
            pre_active = game.teams[my_slot].active;
            pre_facing = game.teams[my_slot].soldiers[pre_active].facing;
        }
        recorded_inputs.push(input.to_bits());

        // tick() gives identical weapon menu + physics + camera + render to hotseat
        let running = tick(&mut game, input, buf, &mut cam, &mut lstate, Some(my_slot));

        if !fired && game.teams[my_slot].soldiers[game.teams[my_slot].active].has_fired {
            fired = true;
        }

        // Days-remaining deadline overlay (bottom-right, screen-anchored)
        if days_remaining > 0 {
            use renderer::font::{draw_str, str_width};
            use renderer::fb::Bgra;
            use crate::world::SCREEN_H;
            let label = format!("{}d left", days_remaining);
            let lw = str_width(&label) as i32;
            let col = if days_remaining <= 3 {
                Bgra::new(255, 80, 80)
            } else if days_remaining <= 7 {
                Bgra::new(255, 180, 50)
            } else {
                Bgra::new(120, 120, 150)
            };
            let dx = cam.left_edge() as i32 + crate::world::SCREEN_W as i32 - lw - 8;
            let dy = SCREEN_H as i32 - 18;
            draw_str(buf, &label, dx, dy, col);
        }

        buf.blit_to_fb(fb, cam.left_edge());
        let e = frame_start.elapsed();
        if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }

        if !running {
            // tick() returns false for pause-quit OR game-over acknowledgement
            if !matches!(game.result, game::state::GameResult::Ongoing) {
                break; // game ended — still submit the move
            } else if fired {
                break; // quit after firing (retreat phase) — submit the recorded move
            } else {
                // Haven't fired yet — block the quit and resume play
                lstate.paused = false;
                game.aim.power = 0.0;
                draw_msg(buf, fb, "FIRE YOUR WEAPON FIRST");
                std::thread::sleep(std::time::Duration::from_millis(1500));
                // loop continues — player must fire before leaving
            }
        }

        // Exit once retreat is over and turn has fully advanced
        if fired && matches!(game.turn.phase, TurnPhase::Acting) && game.projectiles.is_empty() {
            break;
        }
        // Also exit if game ended (won or lost this turn)
        if !matches!(game.result, game::state::GameResult::Ongoing) {
            std::thread::sleep(std::time::Duration::from_secs(3));
            break;
        }
    }
    let my_kills  = (4u32).saturating_sub(game.teams.get(1 - my_slot).map(|t| t.alive_count()).unwrap_or(0));
    let my_deaths = (4u32).saturating_sub(game.teams.get(my_slot).map(|t| t.alive_count()).unwrap_or(0));
    // Per-weapon kills: soldiers alive before this turn that are now dead
    let mut weapon_kills: std::collections::HashMap<&'static str, u32> = std::collections::HashMap::new();
    if let Some(opp_team) = game.teams.get(1 - my_slot) {
        for (i, soldier) in opp_team.soldiers.iter().enumerate() {
            if opp_alive_before.get(i).copied().unwrap_or(false) && soldier.is_dead() {
                let name = soldier.kill_weapon
                    .map(|w| w.display_name())
                    .unwrap_or("UNKNOWN");
                *weapon_kills.entry(name).or_insert(0) += 1;
            }
        }
    }
    Some((game::lobby::Move { angle: pre_angle, power: pre_power, facing: pre_facing, active_soldier: pre_active, inputs: recorded_inputs }, my_kills, my_deaths, weapon_kills))
}

fn show_leaderboard_screen(
    fb:     &mut renderer::Framebuffer,
    input:  &mut input::InputState,
    buf:    &mut WorldBuffer,
    ranked: bool,
) {
    use renderer::Bgra;
    use renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    use game::account::{http_get, json_field, load_saved_creds};

    let sw = SCREEN_W as i32;
    let sh = SCREEN_H as i32;
    let title = if ranked { "RANKED LEADERBOARD" } else { "CASUAL LEADERBOARD" };
    let (my_username, token) = load_saved_creds().unwrap_or_default();

    draw_msg(buf, fb, "LOADING...");

    let base = if ranked { "/api/leaderboard" } else { "/api/leaderboard/casual" };
    let url  = if token.is_empty() { base.to_string() } else { format!("{}?token={}", base, token) };
    let resp = match http_get(&url) {
        Ok(r)  => r,
        Err(_) => {
            draw_msg(buf, fb, "NETWORK ERROR");
            std::thread::sleep(std::time::Duration::from_secs(2));
            return;
        }
    };

    // Extract a named JSON array or object field from the response
    let extract_array = |key: &str| -> Vec<String> {
        let tag = format!("\"{}\":", key);
        let start = match resp.find(&tag) {
            Some(i) => i + tag.len(),
            None    => return vec![],
        };
        let rest = resp[start..].trim_start();
        if !rest.starts_with('[') { return vec![]; }
        let mut depth = 0i32;
        let mut end   = 0usize;
        for (i, c) in rest.char_indices() {
            match c { '[' | '{' => depth += 1, ']' | '}' => { depth -= 1; if depth == 0 { end = i; break; } } _ => {} }
        }
        let inner = rest[1..end].trim();
        if inner.is_empty() { return vec![]; }
        inner.split("},{")
            .map(|s| format!("{{{}}}", s.trim_matches(|c: char| c == '{' || c == '}')))
            .collect()
    };
    let extract_obj = |key: &str| -> Option<String> {
        let tag = format!("\"{}\":", key);
        let start = resp.find(&tag)? + tag.len();
        let rest  = resp[start..].trim_start();
        if !rest.starts_with('{') { return None; }
        let mut depth = 0i32;
        let mut end   = 0usize;
        for (i, c) in rest.char_indices() {
            match c { '{' => depth += 1, '}' => { depth -= 1; if depth == 0 { end = i; break; } } _ => {} }
        }
        Some(rest[..=end].to_string())
    };

    // ── Parse data ────────────────────────────────────────────────────────────

    struct WinEntry  { username: String, wins: u32, losses: u32, elo: u32, rank_name: String }
    struct KillEntry { username: String, kills: u32 }

    let win_list: Vec<WinEntry> = extract_array("wins").iter().map(|e| WinEntry {
        username:  json_field(e, "username").unwrap_or_default(),
        wins:      json_field(e, "wins").and_then(|s| s.parse().ok()).unwrap_or(0),
        losses:    json_field(e, "losses").and_then(|s| s.parse().ok()).unwrap_or(0),
        elo:       json_field(e, "elo").and_then(|s| s.parse().ok()).unwrap_or(0),
        rank_name: json_field(e, "rank").unwrap_or_default(),
    }).collect();

    let kill_list: Vec<KillEntry> = extract_array("kills").iter().map(|e| KillEntry {
        username: json_field(e, "username").unwrap_or_default(),
        kills:    json_field(e, "kills").and_then(|s| s.parse().ok()).unwrap_or(0),
    }).collect();

    // Player's own position (from me_wins / me_kills objects, if logged in)
    let me_wins_obj  = extract_obj("me_wins");
    let me_kills_obj = extract_obj("me_kills");
    let (me_w_pos, me_w, me_l, me_elo, me_rname) = me_wins_obj.as_deref().map(|o| (
        json_field(o, "pos").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
        json_field(o, "wins").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
        json_field(o, "losses").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
        json_field(o, "elo").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
        json_field(o, "rank").unwrap_or_default(),
    )).unwrap_or((0, 0, 0, 0, String::new()));
    let (me_k_pos, me_k) = me_kills_obj.as_deref().map(|o| (
        json_field(o, "pos").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
        json_field(o, "kills").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
    )).unwrap_or((0, 0));

    // Whether the player already appears in the visible win/kill lists
    let me_in_wins  = !my_username.is_empty() && win_list.iter().any(|r| r.username == my_username);
    let me_in_kills = !my_username.is_empty() && kill_list.iter().any(|r| r.username == my_username);

    // ── Build flat item list ──────────────────────────────────────────────────
    // Each item: (pixel_height, item_type)
    // item_type encodes what to draw; we store as u8 tag + index into win/kill_list
    // For simplicity, store as a Vec of closures is too complex in Rust;
    // instead we'll use a height-per-item vec and draw by re-iterating in the render loop.

    const H_HEAD: i32 = 26;  // section header height
    const H_ROW:  i32 = 18;  // data row height
    const H_DIV:  i32 = 14;  // divider between sections

    // Compute total scroll height
    let wins_section_h  = H_HEAD + if win_list.is_empty()  { H_ROW } else { win_list.len()  as i32 * H_ROW }
                        + if !me_in_wins  && me_w_pos > 0  { H_ROW } else { 0 };
    let kills_section_h = H_HEAD + if kill_list.is_empty() { H_ROW } else { kill_list.len() as i32 * H_ROW }
                        + if !me_in_kills && me_k_pos > 0  { H_ROW } else { 0 };
    let total_h  = wins_section_h + H_DIV + kills_section_h;
    let body_top = 38i32;
    let body_bot = sh - 26i32;
    let viewport = body_bot - body_top;
    let max_scroll = (total_h - viewport).max(0);
    let mut scroll: i32 = 0;
    let mut hold_ticks: i32 = 0; // for scroll repeat while held

    // ── Colours ───────────────────────────────────────────────────────────────
    let col_head    = Bgra::new(255, 220, 50);
    let col_label   = Bgra::new(100, 100, 150);
    let col_name    = Bgra::new(220, 220, 240);
    let col_val     = Bgra::new(180, 200, 255);
    let col_rank    = Bgra::new(140, 200, 100);
    let col_me_bg   = Bgra::new(20, 40, 80);
    let col_me_name = Bgra::new(255, 200, 60);
    let col_me_you  = Bgra::new(255, 160, 40);
    let dim_line    = Bgra::new(40, 40, 70);

    loop {
        input.poll();
        if input.just_pressed(input::Button::B) || input.just_pressed(input::Button::Start) { break; }
        if input.held(input::Button::Down) || input.held(input::Button::Up) {
            hold_ticks += 1;
        } else {
            hold_ticks = 0;
        }
        // First press scrolls immediately; after 10-tick delay, scroll every 2 ticks (15/s at 30Hz)
        let do_scroll = input.just_pressed(input::Button::Down)
            || input.just_pressed(input::Button::Up)
            || (hold_ticks > 10 && hold_ticks % 2 == 0);
        if do_scroll {
            if input.held(input::Button::Down) { scroll = (scroll + H_ROW).min(max_scroll); }
            if input.held(input::Button::Up)   { scroll = (scroll - H_ROW).max(0); }
        }

        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(6, 8, 20));
        buf.fill_rect(0, 0, SCREEN_W, 38, Bgra::new(18, 22, 50));
        buf.fill_rect(0, 38, SCREEN_W, 1, dim_line);
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, sw/2 - tw/2, 9, col_head, 2);

        // Clip drawing to body region
        // iy = absolute content y; ry = screen y = iy - scroll + body_top
        let screen_y = |iy: i32| iy - scroll + body_top;
        let visible  = |iy: i32, h: i32| {
            let ry = screen_y(iy);
            ry + h > body_top && ry < body_bot
        };

        let mut iy = 0i32;

        // helper: draw one data row
        let draw_win_row = |buf: &mut WorldBuffer, iy: i32, pos: usize, entry: &WinEntry, is_me: bool| {
            let ry = screen_y(iy);
            if is_me { buf.fill_rect(0, ry, SCREEN_W, H_ROW as u32, col_me_bg); }
            let place = format!("{}.", pos);
            draw_str(buf, &place, 14, ry + 3, col_label);
            let name = if entry.username.len() > 13 { &entry.username[..13] } else { &entry.username };
            let nc = if is_me { col_me_name } else { col_name };
            draw_str(buf, name, 38, ry + 3, nc);
            if is_me { draw_str(buf, "YOU", sw - 8 - str_width("YOU"), ry + 3, col_me_you); }
            else {
                let wl = format!("{}-{}", entry.wins, entry.losses);
                draw_str(buf, &wl, sw - 8 - str_width(&wl), ry + 3, col_val);
                if ranked && entry.elo > 0 {
                    let elo_s = entry.elo.to_string();
                    let x2 = sw - 8 - str_width(&wl) - str_width(&elo_s) - 10;
                    draw_str(buf, &elo_s, x2, ry + 3, col_val);
                    if !entry.rank_name.is_empty() {
                        let x3 = x2 - str_width(&entry.rank_name) - 8;
                        draw_str(buf, &entry.rank_name, x3, ry + 3, col_rank);
                    }
                }
            }
        };
        let draw_kill_row = |buf: &mut WorldBuffer, iy: i32, pos: usize, entry: &KillEntry, is_me: bool| {
            let ry = screen_y(iy);
            if is_me { buf.fill_rect(0, ry, SCREEN_W, H_ROW as u32, col_me_bg); }
            let place = format!("{}.", pos);
            draw_str(buf, &place, 14, ry + 3, col_label);
            let name = if entry.username.len() > 16 { &entry.username[..16] } else { &entry.username };
            let nc = if is_me { col_me_name } else { col_name };
            draw_str(buf, name, 38, ry + 3, nc);
            if is_me { draw_str(buf, "YOU", sw - 8 - str_width("YOU"), ry + 3, col_me_you); }
            else {
                let k = entry.kills.to_string();
                draw_str(buf, &k, sw - 8 - str_width(&k), ry + 3, col_val);
            }
        };

        // ── TOP WINS section ─────────────────────────────────────────────────
        if visible(iy, H_HEAD) {
            let ry = screen_y(iy);
            draw_str(buf, "TOP WINS", 14, ry + 4, col_head);
            buf.fill_rect(10, ry + H_HEAD - 3, (SCREEN_W - 20) as u32, 1, dim_line);
        }
        iy += H_HEAD;

        if win_list.is_empty() {
            if visible(iy, H_ROW) { draw_str(buf, "No games played yet", 28, screen_y(iy) + 3, col_label); }
            iy += H_ROW;
        } else {
            for (i, entry) in win_list.iter().enumerate() {
                if visible(iy, H_ROW) {
                    let is_me = !my_username.is_empty() && entry.username == my_username;
                    draw_win_row(buf, iy, i + 1, entry, is_me);
                }
                iy += H_ROW;
            }
        }
        // Player's own wins row if not in list
        if !me_in_wins && me_w_pos > 0 {
            if visible(iy, H_ROW) {
                let me_entry = WinEntry { username: my_username.clone(), wins: me_w, losses: me_l, elo: me_elo, rank_name: me_rname.clone() };
                draw_win_row(buf, iy, me_w_pos as usize, &me_entry, true);
            }
            iy += H_ROW;
        }

        // ── Divider ──────────────────────────────────────────────────────────
        if visible(iy, H_DIV) {
            buf.fill_rect(10, screen_y(iy) + H_DIV/2, (SCREEN_W - 20) as u32, 1, dim_line);
        }
        iy += H_DIV;

        // ── TOP KILLS section ─────────────────────────────────────────────────
        if visible(iy, H_HEAD) {
            let ry = screen_y(iy);
            draw_str(buf, "TOP KILLS", 14, ry + 4, col_head);
            buf.fill_rect(10, ry + H_HEAD - 3, (SCREEN_W - 20) as u32, 1, dim_line);
        }
        iy += H_HEAD;

        if kill_list.is_empty() {
            if visible(iy, H_ROW) { draw_str(buf, "No games played yet", 28, screen_y(iy) + 3, col_label); }
            iy += H_ROW;
        } else {
            for (i, entry) in kill_list.iter().enumerate() {
                if visible(iy, H_ROW) {
                    let is_me = !my_username.is_empty() && entry.username == my_username;
                    draw_kill_row(buf, iy, i + 1, entry, is_me);
                }
                iy += H_ROW;
            }
        }
        // Player's own kills row if not in list
        if !me_in_kills && me_k_pos > 0 {
            if visible(iy, H_ROW) {
                let me_entry = KillEntry { username: my_username.clone(), kills: me_k };
                draw_kill_row(buf, iy, me_k_pos as usize, &me_entry, true);
            }
        }

        // ── Footer ────────────────────────────────────────────────────────────
        buf.fill_rect(0, body_bot, SCREEN_W, 1, dim_line);
        draw_str(buf, "B = BACK", 20, body_bot + 4, Bgra::new(70, 70, 110));
        if max_scroll > 0 {
            let hint = "UP/DOWN TO SCROLL";
            draw_str(buf, hint, sw - str_width(hint) - 20, body_bot + 4, Bgra::new(70, 70, 110));
        }

        buf.blit_to_fb(fb, 0);
        std::thread::sleep(TICK_DURATION);
    }
}

fn show_stats_screen(
    fb:    &mut renderer::Framebuffer,
    input: &mut input::InputState,
    buf:   &mut WorldBuffer,
    token: &str,
    mode:  &str, // "live" or "tat"
) {
    use renderer::Bgra;
    use renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    use game::account::http_get;

    let title = if mode == "live" { "LIVE STATS" } else { "TAT STATS" };
    let sw = SCREEN_W as i32;
    let sh = SCREEN_H as i32;

    // Fetch stats from API
    draw_msg(buf, fb, "LOADING STATS...");
    let url = format!("/api/stats?mode={}&token={}", mode, token);
    let resp = match http_get(&url) {
        Ok(r) => r,
        Err(_) => {
            draw_msg(buf, fb, "NETWORK ERROR");
            std::thread::sleep(std::time::Duration::from_secs(2));
            return;
        }
    };

    // Parse JSON fields (reuse existing json_field helper via account module)
    use game::account::json_field;
    let cas_w   = json_field(&resp, "casual_wins").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let cas_l   = json_field(&resp, "casual_losses").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let cas_k   = json_field(&resp, "casual_kills").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let cas_d   = json_field(&resp, "casual_deaths").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let rnk_w   = json_field(&resp, "ranked_wins").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let rnk_l   = json_field(&resp, "ranked_losses").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let rnk_k   = json_field(&resp, "ranked_kills").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let rnk_d   = json_field(&resp, "ranked_deaths").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let elo_str = json_field(&resp, "elo").unwrap_or_default();

    let head_col   = Bgra::new(140, 200, 255);
    let label_col  = Bgra::new(130, 130, 160);
    let val_col    = Bgra::new(240, 240, 255);
    let dim_line   = Bgra::new(50, 50, 80);

    loop {
        input.poll();
        if input.just_pressed(input::Button::B) || input.just_pressed(input::Button::Start) { break; }

        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(6, 8, 20));
        // Header
        buf.fill_rect(0, 0, SCREEN_W, 36, Bgra::new(18, 22, 50));
        buf.fill_rect(0, 36, SCREEN_W, 1, dim_line);
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, sw/2 - tw/2, 9, Bgra::new(255, 220, 50), 2);

        let mut y = 48i32;
        let line = |buf: &mut WorldBuffer, label: &str, val: &str, y: i32| {
            draw_str(buf, label, 24, y, label_col);
            let vw = str_width(val);
            draw_str(buf, val, sw - 24 - vw, y, val_col);
        };

        // CASUAL section
        draw_str(buf, "CASUAL", 20, y, head_col); y += 18;
        buf.fill_rect(20, y, (SCREEN_W - 40) as u32, 1, dim_line); y += 6;
        line(buf, "Wins",             &cas_w.to_string(), y); y += 16;
        line(buf, "Losses",           &cas_l.to_string(), y); y += 16;
        line(buf, "Soldiers Killed",  &cas_k.to_string(), y); y += 16;
        line(buf, "Soldiers Lost",    &cas_d.to_string(), y); y += 24;

        // RANKED section
        draw_str(buf, "RANKED", 20, y, head_col); y += 18;
        buf.fill_rect(20, y, (SCREEN_W - 40) as u32, 1, dim_line); y += 6;
        line(buf, "Wins",             &rnk_w.to_string(), y); y += 16;
        line(buf, "Losses",           &rnk_l.to_string(), y); y += 16;
        line(buf, "Soldiers Killed",  &rnk_k.to_string(), y); y += 16;
        line(buf, "Soldiers Lost",    &rnk_d.to_string(), y); y += 16;
        if !elo_str.is_empty() {
            line(buf, "ELO", &elo_str, y);
        }

        // Footer
        buf.fill_rect(0, sh - 26, SCREEN_W, 1, dim_line);
        draw_str(buf, "B = BACK", 20, sh - 18, Bgra::new(70, 70, 110));

        buf.blit_to_fb(fb, 0);
        std::thread::sleep(TICK_DURATION);
    }
}
