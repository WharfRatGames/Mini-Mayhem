mod world;
mod physics;
mod renderer;
mod input;
mod game;
mod net;
mod updater;
mod audio;
mod https;
mod bug_report;
const VERSION: &str = "0.5.4.393";

use std::time::{Duration, Instant};
use world::{WorldPos, Heightmap, Terrain, WORLD_W};
use renderer::{Framebuffer, WorldBuffer, Camera};
use renderer::hud::{COLOR_DARK_BG};
use input::InputState;
use game::{
    title::{TitleScreen, CHOICE_QUIT, CHOICE_LIVE, CHOICE_TAKE_A_TURN,
             CHOICE_SP, CHOICE_HOTSEAT, CHOICE_VS_CPU, CHOICE_SETTINGS},
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
    #[cfg(not(feature = "desktop"))]
    unsafe { for fd in 3i32..=255 { libc::close(fd); } }

    // Tell keymon to stop intercepting the MENU button so we can read KEY_ESC.
    // Written just before the inner game loop and removed on Drop (including on
    // `continue 'game` back to title). Title screen gets normal OS Menu behaviour.
    #[cfg(not(feature = "desktop"))]
    struct MenuGuard;
    #[cfg(not(feature = "desktop"))]
    impl Drop for MenuGuard {
        fn drop(&mut self) { let _ = std::fs::remove_file("/tmp/disable_menu_button"); }
    }

    // ── Open hardware ─────────────────────────────────────────────────────────
    let mut fb = Framebuffer::open()
        .expect("failed to open display");
    let mut input = InputState::new();
    #[cfg(not(feature = "desktop"))]
    input.open().expect("Failed to open /dev/input/event0");
    let mut buf    = WorldBuffer::new();
    let mut lstate = LoopState::new();

    // Initialise audio engine (rodio/ALSA on armv7; no-op elsewhere).
    audio::init();

    // Pre-resolve server DNS at launch so TCP connect is instant when the player
    // joins a live match. DuckDNS can take 15-20s on first lookup; overlapping
    // it with the splash/title hides the wait entirely.
    net::start_dns_prefetch();

    // Kick off the update check before the splash so the HTTP request runs
    // during the splash window (0-5s) rather than after it. recv_timeout at the
    // pre-title gate then finds the result already in the channel instead of
    // racing against the 2s HTTP timeout.
    let skip_update = updater::prior_update_attempted();
    let (update_tx, update_rx) = std::sync::mpsc::channel::<(bool, bool)>();
    if !skip_update {
        std::thread::spawn(move || { let _ = update_tx.send(updater::check_for_update(VERSION)); });
    } else {
        drop(update_tx); // channel disconnected immediately; recv_timeout returns Err right away
    }

    updater::sync_assets_bg(VERSION);

    // Preload SFX and warm texture atlas in background (overlaps update check wait).
    std::thread::spawn(audio::preload);
    std::thread::spawn(|| { crate::renderer::terrain_textures::tile(0); });

    // After a live game ends, return to the MP submenu rather than full title.
    let mut return_to_mp = false;
    // Set when a ranked live match drops involuntarily (not a player-chosen
    // exit) — offers a reconnect popup on the title screen for up to 180s.
    let mut pending_reconnect: Option<(String, u16, Instant)> = None;
    // Restore reconnect state across a full app restart (e.g. the player
    // exited after "LOST CONNECTION" instead of waiting at the title screen).
    if let Some((tok, port, since_unix)) = game::account::take_pending_reconnect() {
        let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let age = std::time::Duration::from_secs(now_unix.saturating_sub(since_unix));
        let since = Instant::now() - age;
        pending_reconnect = Some((tok, port, since));
    }
    // Set when the player accepts the reconnect popup; consumed to skip
    // login/roster/matchmaking and resume the existing match directly.
    let mut force_reconnect: Option<(String, u16)> = None;
    // Cached result from the background update-check thread.
    let mut update_available = false;

    // ── Update check (before splash) ────────────────────────────────────────
    // Wait for the background check to complete, then show the update screen
    // if needed — all before the splash so the player sees it immediately.
    if !skip_update {
        if let Ok((true, tls_broken)) = update_rx.recv_timeout(std::time::Duration::from_secs(8)) {
            update_available = true;
            use renderer::Bgra;
            use renderer::font::{draw_str_scaled, draw_str, str_width_scaled, str_width, wrap_text};
            use world::{SCREEN_W, SCREEN_H};
            let sw = SCREEN_W as i32; let sh = SCREEN_H as i32;
            let bar_x = 40i32; let bar_w = sw - 80;
            let bar_y = sh/2 + 10; let bar_h = 24i32;
            let changelog = updater::fetch_changelog(3)
                .unwrap_or_else(|| vec!["update notes unavailable offline".to_string()]);
            let max_lines = ((sh - 70 - 54) / 12).max(1) as usize;
            let changelog: Vec<String> = changelog.iter()
                .flat_map(|line| wrap_text(line, 1, sw - 36))
                .take(max_lines)
                .collect();
            'pretitle_update: loop {
                input.poll();
                if input.just_pressed(input::Button::A) {
                    let binary = updater::stream_binary(|done, total| {
                        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
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
                        buf.blit_to_fb(&mut fb, 0, 0);
                    });
                    match binary {
                        Some(b) if b.starts_with(b"\x7fELF") => {
                            draw_msg(&mut buf, &mut fb, "APPLYING UPDATE...");
                            updater::apply_binary(&b, &mut buf, &mut fb);
                        }
                        _ => { draw_msg(&mut buf, &mut fb, "DOWNLOAD FAILED"); std::thread::sleep(std::time::Duration::from_secs(2)); }
                    }
                    break 'pretitle_update;
                }
                if !tls_broken && (input.just_pressed(input::Button::B) || input.just_pressed(input::Button::Start)) {
                    break 'pretitle_update; // normal update — skip allowed
                }
                buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
                buf.fill_rect(0, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
                let t = if tls_broken { "UPDATE REQUIRED" } else { "UPDATE AVAILABLE" };
                draw_str_scaled(&mut buf, t, sw/2 - str_width_scaled(t,2)/2, 10, Bgra::new(255,210,50), 2);
                let v = format!("VERSION {}", VERSION);
                draw_str_scaled(&mut buf, &v, sw/2 - str_width_scaled(&v,1)/2, 34, Bgra::new(100,100,140), 1);
                for (i, line) in changelog.iter().enumerate() {
                    draw_str(&mut buf, line, 18, 54 + i as i32 * 12, Bgra::new(110,130,160));
                }
                draw_str_scaled(&mut buf, "A = INSTALL NOW", sw/2 - str_width_scaled("A = INSTALL NOW",2)/2, sh - 38, Bgra::new(80,220,120), 2);
                if !tls_broken {
                    draw_str_scaled(&mut buf, "B = SKIP", sw/2 - str_width_scaled("B = SKIP",2)/2, sh - 20, Bgra::new(140,140,160), 1);
                }
                buf.blit_to_fb(&mut fb, 0, 0);
                std::thread::sleep(TICK_DURATION);
            }
        }
    }

    // Splash screen: show wharf.jpg briefly before going to the title.
    renderer::splash::draw_splash(&mut buf);
    buf.blit_to_fb(&mut fb, 0, 0);
    {
        let splash_start = Instant::now();
        while splash_start.elapsed() < std::time::Duration::from_secs(3) {
            input.poll();
            if input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start) { break; }
            std::thread::sleep(TICK_DURATION);
        }
    }

    // ── Title screen ────────────────────────────────────────────────────────
    'game: loop {
    let mut title = TitleScreen::new(VERSION);
    if return_to_mp { title.continue_to_submenu(); return_to_mp = false; }

    // Reconnect popup — only shown for involuntary disconnects from a
    // reconnectable match, while the server's pause window is open.
    if let Some((tok, port, since)) = pending_reconnect.take() {
        if since.elapsed() < std::time::Duration::from_secs(180) {
            let mut cursor: usize = 0; // 0=RECONNECT, 1=ABANDON
            let accept = loop {
                input.poll();
                let secs_left = 180u64.saturating_sub(since.elapsed().as_secs());
                draw_reconnect_popup(&mut buf, &mut fb, cursor, secs_left);
                if input.just_pressed(input::Button::Up)   { cursor = 0; }
                if input.just_pressed(input::Button::Down) { cursor = 1; }
                if input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start) {
                    break cursor == 0;
                }
                if input.just_pressed(input::Button::B) { break false; }
                std::thread::sleep(TICK_DURATION);
            };
            if accept { force_reconnect = Some((tok, port)); }
            game::account::clear_pending_reconnect();
        }
    }
    let is_reconnect_resume = force_reconnect.is_some();
    let choice = if is_reconnect_resume { CHOICE_LIVE } else { loop {
        let c = loop {
            let frame_start = Instant::now();
            input.poll();
            // Non-blocking poll — cache result once background thread finishes.
            if !update_available { if let Ok((true, _)) = update_rx.try_recv() { update_available = true; } }
            if let Some(c) = title.update(&input, &mut buf) { break c; }
            buf.blit_to_fb(&mut fb, 0, 0);
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
        if c == CHOICE_SETTINGS {
            let mut settings_screen = game::settings::SettingsScreen::new();
            loop {
                let fs = std::time::Instant::now();
                input.poll();
                if let Some(game::settings::SettingsAction::Back) = settings_screen.update(&input, &mut buf) { break; }
                buf.blit_to_fb(&mut fb, 0, 0);
                let e = fs.elapsed();
                if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
            }
            continue;
        }
        if c == game::title::CHOICE_MISSIONS {
            let token = game::account::load_saved_creds().map(|(_, t)| t).unwrap_or_default();
            show_missions_screen(&mut fb, &mut input, &mut buf, &token);
            title.continue_to_submenu(); // return to MULTIPLAYER submenu
            continue;
        }
        if c == game::title::CHOICE_ACCOUNT {
            run_account_menu(&mut fb, &mut input, &mut buf);
            title.continue_to_submenu();
            continue;
        }
        if c != game::title::CHOICE_MULTI { break c; }
        input.poll();
        title.continue_to_submenu();
    }};
    if choice == CHOICE_QUIT { return; }
    // ── Unified update gate: optional for SP, required for MP ────────────────
    // Non-blocking drain — catches results that arrived while in the title loop.
    if !update_available {
        if let Ok((true, _)) = update_rx.try_recv() { update_available = true; }
    }
    let is_sp_mode = matches!(choice, CHOICE_HOTSEAT | CHOICE_VS_CPU)
        || choice == game::title::CHOICE_TEST;
    let is_mp_mode = choice == CHOICE_LIVE
        || choice == CHOICE_TAKE_A_TURN
        || choice == game::title::CHOICE_LIVE_RANKED
        || choice == game::title::CHOICE_TAT_RANKED;
    // Wait for background thread result; if channel is disconnected (sentinel dropped tx),
    // spawn a fresh dedicated check so SP/MP always gets a live result.
    // MP/TAT always checks regardless of skip_update — the sentinel only prevents retry
    // loops at boot; for multiplayer entry we must know the current version even if a
    // prior update attempt failed and we're stuck on the old binary.
    if (is_sp_mode && !skip_update || is_mp_mode) && !update_available {
        if is_mp_mode {
            use renderer::Bgra;
            use renderer::font::{draw_str_scaled, str_width_scaled};
            use world::{SCREEN_W, SCREEN_H};
            let sw = SCREEN_W as i32; let sh = SCREEN_H as i32;
            let t = "CHECKING FOR UPDATES...";
            buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
            draw_str_scaled(&mut buf, t, sw/2 - str_width_scaled(t, 2)/2, sh/2 - 8, Bgra::new(140, 140, 180), 2);
            buf.blit_to_fb(&mut fb, 0, 0);
        }
        let got = update_rx.recv_timeout(std::time::Duration::from_millis(500));
        if let Ok((true, _)) = got {
            update_available = true;
        } else if is_mp_mode {
            // MP only: re-check in case version changed since boot. SP skips to avoid blocking.
            let (ftx, frx) = std::sync::mpsc::channel::<bool>();
            std::thread::spawn(move || { let _ = ftx.send(updater::check_for_update(VERSION).0); });
            if let Ok(true) = frx.recv_timeout(std::time::Duration::from_secs(6)) {
                update_available = true;
            }
        }
    }
    if update_available && (is_sp_mode || is_mp_mode) {
        use renderer::Bgra;
        use renderer::font::{draw_str_scaled, draw_str, str_width_scaled, str_width, wrap_text};
        use world::{SCREEN_W, SCREEN_H};
        let forced = is_mp_mode;
        let sw = SCREEN_W as i32; let sh = SCREEN_H as i32;
        let bar_x = 40i32; let bar_w = sw - 80;
        let bar_y = sh/2 + 10; let bar_h = 24i32;
        // Pi-served changelog (see pre-title block) — always current, no rebuild.
        let changelog = updater::fetch_changelog(3)
            .unwrap_or_else(|| vec!["update notes unavailable offline".to_string()]);
        let max_lines = ((sh - 70 - 54) / 12).max(1) as usize;
        let changelog: Vec<String> = changelog.iter()
            .flat_map(|line| wrap_text(line, 1, sw - 36))
            .take(max_lines)
            .collect();
        let proceed = loop {
            input.poll();
            if input.just_pressed(input::Button::A) {
                let binary = updater::stream_binary(|done, total| {
                    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
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
                    buf.blit_to_fb(&mut fb, 0, 0);
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
            buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
            buf.fill_rect(0, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
            let t = if forced { "UPDATE REQUIRED FOR MULTIPLAYER" } else { "UPDATE AVAILABLE" };
            let t_col = if forced { Bgra::new(255, 80, 80) } else { Bgra::new(255, 210, 50) };
            draw_str_scaled(&mut buf, t, sw/2 - str_width_scaled(t,2)/2, 10, t_col, 2);
            let v = format!("VERSION {}", VERSION);
            draw_str_scaled(&mut buf, &v, sw/2 - str_width_scaled(&v, 1)/2, 34, Bgra::new(100, 100, 140), 1);
            for (i, line) in changelog.iter().enumerate() {
                draw_str(&mut buf, line, 18, 54 + i as i32 * 12, Bgra::new(110, 130, 160));
            }
            draw_str_scaled(&mut buf, "A = INSTALL NOW", sw/2 - str_width_scaled("A = INSTALL NOW",2)/2, sh - 70, Bgra::new(80, 220, 120), 2);
            let b_label = if forced { "B = BACK" } else { "B = SKIP" };
            draw_str_scaled(&mut buf, b_label, sw/2 - str_width_scaled(b_label,2)/2, sh - 38, Bgra::new(140, 140, 160), 2);
            buf.blit_to_fb(&mut fb, 0, 0);
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
    let mut live_username = String::new();
    let mut live_ranked_match = false;
    let mut live_elo_my:  i32 = 0;
    let mut live_elo_opp: i32 = 0;
    let mut live_opp_username = String::new();
    let mut live_game_port: u16 = 7777; // port of the spawned game server instance
    let mut session_token = String::new(); // empty for casual/non-ranked — server treats as non-reconnectable
    if is_reconnect_resume {
        let (tok, port) = force_reconnect.take().unwrap();
        session_token = tok;
        live_game_port = port;
        live_ranked_match = true;
    } else if is_live || is_live_ranked {
        use game::account::{AccountScreen, AccountAction, RosterPicker, RosterAction,
                             load_saved_creds};
        // Login — load teams from local cache instantly (no network call)
        let (token, mut rosters) = if let Some((u, t)) = load_saved_creds() {
            live_username = u;
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
                buf.blit_to_fb(&mut fb, 0, 0);
                let e = fs.elapsed(); if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
            };
            match result {
                AccountAction::LoggedIn { token, username, rosters, .. } => { live_username = username; (token, rosters) }
                AccountAction::Back => { continue 'game; }
            }
        };
        if rosters.is_empty() { rosters.push(game::account::Roster::default_named(0)); }
        // Daily login bonus — fired off in the background so a slow/laggy
        // connection to the bonus endpoint doesn't freeze the screen before
        // every match (this blocking call was the main cause of "match start
        // is slow"). Picked up after the roster picker if it's ready by then.
        let (bonus_tx, bonus_rx) = std::sync::mpsc::channel();
        {
            let token = token.clone();
            std::thread::spawn(move || { let _ = bonus_tx.send(game::account::claim_daily_login(&token)); });
        }
        // Roster picker
        let mut picker = RosterPicker::new(token, rosters);
        let picked = loop {
            let fs = std::time::Instant::now();
            input.poll();
            buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32,
                renderer::Bgra::new(8, 8, 20));
            if let Some(a) = picker.update(&input, &mut buf, 0) { break a; }
            buf.blit_to_fb(&mut fb, 0, 0);
            let e = fs.elapsed(); if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
        };
        if let Ok(Some((earned, weekly))) = bonus_rx.try_recv() {
            show_login_bonus(&mut buf, &mut fb, &mut input, earned, weekly);
        }
        match picked {
            RosterAction::Selected(r) => live_roster = Some(r),
            RosterAction::Skip => {} // proceed with default generic names
            RosterAction::Back => { continue 'game; }
        }
    }

    // ── Ranked live: queue on the game server directly ─────────────────────
    // Connect with the RANKED token; the server queues us and sends WelcomeMsg
    // once a second player arrives. The "WAITING FOR OPPONENT" loop below
    // (after TCP connect) handles the wait + B=cancel.
    if is_live_ranked {
        session_token = "RANKED".to_string();
        live_ranked_match = true;
    }

    // ── Multiplayer connect — background thread so B cancels instantly ──────
    let mut net_conn: Option<net::ServerConn> = None;
    if is_live || is_live_ranked {
        use net::ServerConn;
        let ver = VERSION;
        // Spawn connect attempt; main loop polls and checks B each frame
        let (conn_tx, conn_rx) = std::sync::mpsc::channel::<Result<ServerConn, ()>>();
        let port = live_game_port;
        let pre_resolved = net::cached_server_addr();
        std::thread::spawn(move || {
            let result = if let Some(sock) = pre_resolved {
                ServerConn::connect_addr(sock, port)
            } else {
                let addr = format!("crumbonium.duckdns.org:{}", port);
                ServerConn::connect(&addr)
            };
            let _ = conn_tx.send(result.map_err(|_| ()));
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
                    c.send_raw(session_token.as_bytes());
                    c.send_raw(b"\n");
                    if live_ranked_match {
                        c.send_raw(live_username.as_bytes());
                        c.send_raw(b"\n");
                    }
                    // Read server response: "OK\n" or "REJECTED:VERSION\n"
                    let resp = c.read_line_blocking();
                    if resp.trim() == "REJECTED:VERSION" {
                        draw_msg(&mut buf, &mut fb, "UPDATE REQUIRED");
                        std::thread::sleep(std::time::Duration::from_secs(1));
                        update_available = true;
                        continue 'game;
                    }
                    break Some(c);
                }
                Ok(Err(_)) => {
                    draw_msg(&mut buf, &mut fb, "CONNECT FAILED  (B=BACK)");
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
    let mut team_count: usize = 2;
    let mut my_color: u8 = 0;
    // Final lobby roster (casual) — index = compact team index. Used to seed
    // each team's colour/name before the first StateMsg arrives.
    let mut lobby_players: Vec<net::msg::LobbyPlayer> = Vec::new();
    if let Some(ref mut conn) = net_conn {
        let welcome = if is_live && !is_live_ranked {
            // Casual: run the lobby (ready-up + pick colour).
            match run_casual_lobby(&mut fb, &mut buf, &mut input, conn, live_roster.as_ref(), &live_username) {
                Some((w, players)) => { lobby_players = players; Some(w) }
                None => None,
            }
        } else {
            // Ranked: wait for the opponent the server pairs us with.
            conn.set_read_timeout(Some(std::time::Duration::from_millis(200)));
            loop {
                input.poll();
                if input.just_pressed(input::Button::Start) || input.just_pressed(input::Button::B) {
                    break None;
                }
                draw_msg(&mut buf, &mut fb, "WAITING FOR OPPONENT... B=CANCEL");
                if let Some(w) = conn.recv_blocking::<net::msg::WelcomeMsg>() { break Some(w); }
            }
        };
        if let Some(w) = welcome {
            conn.set_read_timeout(None); // clear timeout for gameplay
            my_team     = w.your_team;
            server_seed = Some(w.seed);
            team_count  = w.team_count.clamp(2, 4);
            my_color    = w.your_color.min(3);
            if !w.reconnect_token.is_empty() {
                session_token = w.reconnect_token.clone();
            }
            conn.start_reader();
            let colour_name = ["RED", "BLUE", "GREEN", "YELLOW"][my_color as usize];
            let msg = if my_team == 0 {
                format!("YOU ARE {}  -  YOUR TURN FIRST", colour_name)
            } else {
                format!("YOU ARE {}", colour_name)
            };
            draw_msg(&mut buf, &mut fb, &msg);
            std::thread::sleep(std::time::Duration::from_millis(800));
        } else {
            // B pressed — drop connection and return to title
            net_conn = None;
            continue 'game;
        }
    }
    if is_tat { run_take_a_turn(&mut fb, &mut input, &mut buf); continue 'game; }
    let game_seed = server_seed.unwrap_or_else(current_time_seed);
    let mut game    = build_default_game_n(game_seed, team_count);
    // Seed colours from the welcome / lobby so the intro screen and first frames
    // render in the right team colours (the first StateMsg then keeps them synced).
    if let Some(t) = game.teams.get_mut(my_team) { t.set_color(my_color); }
    for (i, p) in lobby_players.iter().enumerate() {
        if let Some(t) = game.teams.get_mut(i) {
            if let Some(c) = p.color_id { t.set_color(c); }
            if !p.name.is_empty() { t.name = p.name.clone(); }
            t.avatar_id = p.avatar_id;
        }
        if i != my_team && !p.username.is_empty() {
            live_opp_username = p.username.clone();
        }
    }
    // Test mode: give every team the full weapon set with infinite ammo.
    if is_test {
        use physics::projectile::WeaponKind;
        let all_weapons: Vec<(WeaponKind, Option<u32>)> = vec![
            (WeaponKind::Bazooka,     None),
            (WeaponKind::Grenade,     None),
            (WeaponKind::Shotgun,     None),
            (WeaponKind::Pistol,      None),
            (WeaponKind::NinjaRope,   None),
            (WeaponKind::Tnt,         None),
            (WeaponKind::Landmine,    None),
            (WeaponKind::BaseballBat, None),
            (WeaponKind::BananaBomb,  None),
            (WeaponKind::ClusterBomb,     None),
            (WeaponKind::MolotovCocktail, None),
            (WeaponKind::Revolver,      None),
            (WeaponKind::Blasthive,     None),
            (WeaponKind::BlackHoleBomb, None),
            (WeaponKind::PlasmaTorch,   None),
            (WeaponKind::Garcia,          None),
            (WeaponKind::AirStrike,       None),
            (WeaponKind::HolyHandGrenade, None),
            (WeaponKind::Minigun,         None),
            (WeaponKind::Uzi,             None),
            (WeaponKind::HomingMissile,   None),
        ];
        let mut all_weapons_sorted = all_weapons.clone();
        all_weapons_sorted.sort_by_key(|(k, _)| k.menu_sort_key());
        for team in &mut game.teams {
            team.weapons = all_weapons_sorted.clone();
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
        let mut intro_usernames = [String::new(), String::new()];
        if live_ranked_match {
            intro_usernames[my_team]     = live_username.clone();
            intro_usernames[1 - my_team] = live_opp_username.clone();
        } else {
            for (i, p) in lobby_players.iter().enumerate().take(2) {
                intro_usernames[i] = p.username.clone();
            }
        }
        show_match_intro(&mut fb, &mut buf, &mut input, &game, my_team, &intro_usernames, false);
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
    let mut cam     = Camera::new(start_pos.x, crate::world::TERRAIN_MAX_Y as f32);
    cam.snap_to(start_pos);
    let mut game_over_ticks: u32 = 0;
    // Last server tick whose StateMsg.sounds we've already played — dedupes the
    // 90× repeated final state on game-over and lets us recover sounds from any
    // intermediate ticks dropped while draining.
    let mut last_sound_tick: u32 = u32::MAX;
    let mut final_result: Option<game::state::GameResult> = None;
    // ELO delta shown on end screen for ranked matches
    let mut elo_delta: i32 = 0;
    let mut scrap_earned: u32 = 0;
    let mut elo_delta_rx: Option<std::sync::mpsc::Receiver<(i32, u32)>> = None;
    let mut last_blit = Instant::now();
    let mut fps_accum_us: u64 = 0;  // sum of inter-blit intervals in the window
    let mut fps_frame_count: u32 = 0;
    let mut paused_secs: Option<u32> = None;
    let mut opponent_left_ticks: u32 = 0; // banner: "OPPONENT DISCONNECTED"
    let mut opponent_abandoned = false;
    let mut opponent_quit_acked = false; // true after player dismisses the quit dialog
    let mut last_state_time = Instant::now(); // detect stalled server (no state + no final result)
    let mut bug_reporter: Option<bug_report::BugReporter> = None;
    // Enable Menu→KEY_ESC for the bug reporter during gameplay only.
    // Dropped on any `continue 'game` so the title screen gets normal OS Menu.
    #[cfg(not(feature = "desktop"))]
    let _menu_guard = { let _ = std::fs::write("/tmp/disable_menu_button", b""); MenuGuard };
    loop {
        let frame_start = Instant::now();
        input.poll();

        // MENU button opens the bug reporter (keymon disabled at startup)
        if bug_reporter.is_none()
            && input.just_pressed(input::Button::Menu)
        {
            bug_reporter = Some(bug_report::BugReporter::capture(&buf, cam.left_edge()));
        }
        if let Some(ref mut reporter) = bug_reporter {
            let cancelled = reporter.tick(&input);
            reporter.draw(&mut buf, cam.left_edge());
            buf.blit_to_fb(&mut fb, cam.left_edge(), cam.top_edge());
            if cancelled || reporter.is_done() {
                bug_reporter = None;
            }
            // Consume this frame — don't run the rest of the game loop
            let elapsed = frame_start.elapsed().as_micros() as u64;
            if elapsed < 33_333 {
                std::thread::sleep(std::time::Duration::from_micros(33_333 - elapsed));
            }
            continue;
        }

        if let Some(ref mut conn) = net_conn {
            // Disconnect detection — reader thread or write failure sets this flag.
            if conn.is_disconnected() {
                draw_msg(&mut buf, &mut fb, "LOST CONNECTION");
                std::thread::sleep(std::time::Duration::from_secs(2));
                // If this was a reconnectable ranked match, offer a reconnect popup
                // on the title screen (only for involuntary drops, not B/Start exits).
                if !session_token.is_empty() {
                    pending_reconnect = Some((session_token.clone(), live_game_port, Instant::now()));
                    let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
                    game::account::save_pending_reconnect(&session_token, live_game_port, now_unix);
                }
                return_to_mp = true;
                continue 'game;
            }
            use net::msg::{InputMsg, NetButton};
            // Process the weapon menu before building the InputMsg so that an A press
            // used to confirm a weapon selection is not forwarded to the server as a
            // fire input (server has no menu and would fire a phantom shot).
            let my_turn_now = game.turn.current_team == my_team;
            let menu_consumed_a = if !lstate.paused && my_turn_now {
                let was_open = game.weapon_menu_open;
                let menu_active = game::loop_runner::process_weapon_menu(&mut game, &input);
                game::loop_runner::tick_fire_grace(&mut game);
                if lstate.fire_grace > 0 { lstate.fire_grace -= 1; game.aim.power = 0.0; }
                if !menu_active { game::loop_runner::process_aim(&mut game, &input, None); }
                // A was consumed by the menu if the menu was open (or just opened) and A was pressed
                was_open && input.just_pressed(input::Button::A)
            } else {
                if !my_turn_now { game.weapon_menu_open = false; }
                false
            };
            // Suppress A-HELD when server_fire_grace active (same state used in tick/server_tick).
            let suppress_a_held = game.server_fire_grace > 0;
            let menu_open = game.weapon_menu_open;
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
            ].iter().filter(|(b,_)| {
                if !input.held(*b) { return false; }
                if suppress_a_held && *b == input::Button::A { return false; }
                // Don't send movement while weapon menu is open
                if menu_open && (*b == input::Button::Left || *b == input::Button::Right) { return false; }
                true
            }).map(|(_,n)| *n).collect();
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
            ].iter().filter(|(b,_)| input.just_pressed(*b) && !(menu_consumed_a && *b == input::Button::A)).map(|(_,n)| *n).collect();
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
            // Drain ALL pending state messages:
            //   - sounds collected from every state so no SFX tick is skipped
            //   - first received state's projectiles used for position (avoids the
            //     2-tick jump when two states arrive in one frame, which looked choppy)
            //   - latest state used for everything else (turn, soldiers, result)
            let mut latest_state: Option<net::msg::StateMsg> = None;
            let mut first_projectiles: Option<Vec<net::msg::NetProjectile>> = None;
            let mut pending_sounds: Vec<u8> = Vec::new();
            let mut pending_fx: Vec<crate::renderer::fx::FxEvent> = Vec::new();
            while let Some(state) = conn.try_recv::<net::msg::StateMsg>() {
                if state.tick != last_sound_tick {
                    pending_sounds.extend_from_slice(&state.sounds);
                    pending_fx.extend_from_slice(&state.fx_events);
                    last_sound_tick = state.tick;
                }
                if first_projectiles.is_none() {
                    first_projectiles = Some(state.projectiles.clone());
                }
                latest_state = Some(state);
            }
            // Play the server's recorded SFX + spawn its recorded FX (skip while
            // the game-over overlay is up). Mirrors the sounds channel so effects
            // spawned in the sim appear on the live client without bespoke replay.
            if final_result.is_none() {
                for id in &pending_sounds {
                    if let Some(s) = crate::audio::Sfx::from_u8(*id) { crate::audio::play(s); }
                }
                for ev in &pending_fx {
                    crate::renderer::fx::apply_event(&mut game.fx, ev);
                }
            }
            let got_state = latest_state.is_some();
            if got_state {
                last_state_time = Instant::now();
                if let Some(sent) = lstate.last_input_sent.take() {
                    let rtt = sent.elapsed().as_millis() as u32;
                    // Smooth: 80% old + 20% new sample
                    lstate.ping_ms = if lstate.ping_ms == 0 { rtt } else { (lstate.ping_ms * 4 / 5).saturating_add(rtt / 5) };
                }
            }
            // Send input AFTER draining states so RTT is measured from previous tick's
            // send to this tick's state arrival (not same-tick buffered states → 0ms).
            if !lstate.paused && final_result.is_none() {
                conn.send(&InputMsg { tick: lstate.tick, held, pressed, released, aim_angle: game.aim.angle, selected_weapon_kind, hat_ids, uniform_color_ids, boot_color_ids, gun_style_ids, worm_names, muzzle_x: lstate.last_muzzle.map(|(x,_)| x).unwrap_or(0.0), muzzle_y: lstate.last_muzzle.map(|(_,y)| y).unwrap_or(0.0), quit: false });
                lstate.last_input_sent = Some(std::time::Instant::now());
            }
            if let Some(state) = &latest_state {
                if state.opponent_abandoned { opponent_abandoned = true; opponent_left_ticks = opponent_left_ticks.max(150); }
                let prev_paused = paused_secs;
                paused_secs = state.paused_opponent;
                if prev_paused.is_none() && paused_secs.is_some() { opponent_left_ticks = 150; }
            }
            if let Some(state) = latest_state {
                // While showing the 10-second game-over overlay, don't let a new-game
                // StateMsg (result=Ongoing) from the server clear our final result.
                if final_result.is_none() {
                    game::net_sync::apply_server_state(&mut game, &mut cam, &state, my_team);
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
                            proj.age_ticks   = np.age_ticks;
                            if kind == WeaponKind::HomingMissile && (np.homing_target_x != 0.0 || np.homing_target_y != 0.0) {
                                proj.homing_target = Some((np.homing_target_x, np.homing_target_y));
                            }
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
                    cam = Camera::new(start_pos.x, crate::world::TERRAIN_MAX_Y as f32);
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
                    .filter(|p| p.kind == WeaponKind::Bazooka || p.kind == WeaponKind::HomingMissile)
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
                if input.just_pressed(input::Button::A) && lstate.pause_cursor == 1 {
                    let msg = if live_ranked_match {
                        "QUIT = FORFEIT + ELO LOSS  A=CONFIRM  B=CANCEL"
                    } else {
                        "QUIT = FORFEIT (OPPONENT WINS)  A=CONFIRM  B=CANCEL"
                    };
                    let confirm = loop {
                        input.poll();
                        draw_msg(&mut buf, &mut fb, msg);
                        if input.just_pressed(input::Button::A) { break true; }
                        if input.just_pressed(input::Button::B) { break false; }
                        std::thread::sleep(TICK_DURATION);
                    };
                    if confirm { mp_quit = true; }
                }
            }
            if mp_quit {
                // Notify the server this is a voluntary forfeit before disconnecting.
                // The server awards the win to the opponent immediately and skips the
                // reconnect window, so the remaining player sees "you win" right away.
                if let Some(ref mut conn) = net_conn {
                    conn.send(&net::msg::InputMsg {
                        tick: lstate.tick, held: vec![], pressed: vec![], released: vec![],
                        aim_angle: game.aim.angle, selected_weapon_kind: 0,
                        hat_ids: [0;4], uniform_color_ids: [0;4], boot_color_ids: [0;4],
                        gun_style_ids: [0;4], worm_names: Default::default(),
                        muzzle_x: 0.0, muzzle_y: 0.0, quit: true,
                    });
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                continue 'game;
            }
            // Weapon menu + aim already processed above (before InputMsg send).
            // Camera pan or follow active soldier
            let cam_speed = 20.0f32;
            if input.held(input::Button::L1) {
                // L1 + dpad: free pan — stays when L1 released, clears on turn change
                if input.held(input::Button::Left)  { cam.pan(-cam_speed); }
                if input.held(input::Button::Right) { cam.pan( cam_speed); }
                if input.held(input::Button::Up)    { cam.pan_y(-cam_speed); }
                if input.held(input::Button::Down)  { cam.pan_y( cam_speed); }
            } else if input.held(input::Button::R1) {
                if input.held(input::Button::Left)  { cam.pan(-cam_speed); }
                if input.held(input::Button::Right) { cam.pan( cam_speed); }
                if input.held(input::Button::Up)    { cam.pan_y(-cam_speed); }
                if input.held(input::Button::Down)  { cam.pan_y( cam_speed); }
            } else if let Some(ref hm) = game.homing_missile {
                if !hm.confirmed {
                    cam.follow(world::WorldPos::new(hm.render_x, hm.render_y));
                } else {
                    let ti = game.turn.current_team();
                    if let Some(team) = game.teams.get(ti) {
                        if let Some(s) = team.soldiers.get(team.active) { cam.follow(s.pos); }
                    }
                }
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
            } else if let Some(ref air) = game.airstrike {
                if !air.active {
                    // Targeting phase: follow the cursor (mirrors update_camera Acting branch)
                    cam.follow(world::WorldPos::new(air.render_x, air.render_y));
                } else {
                    // Plane live: bombs show as projectiles; fall back to active soldier
                    let ti = game.turn.current_team();
                    if let Some(team) = game.teams.get(ti) {
                        if let Some(s) = team.soldiers.get(team.active) { cam.follow(s.pos); }
                    }
                }
            } else if let Some(p) = game.projectiles.first() {
                cam.follow_always(p.pos);
            } else if !game.explosions.is_empty() {
                // Track nearest explosion to current camera center (mirrors Watching branch)
                let cam_center = cam.left_edge_f32() + world::SCREEN_W as f32 / 2.0;
                if let Some(e) = game.explosions.iter()
                    .min_by(|a, b| (a.pos.x - cam_center).abs()
                        .partial_cmp(&(b.pos.x - cam_center).abs()).unwrap())
                {
                    cam.follow_always(e.pos);
                }
            } else {
                // Airborne soldiers from knockback — use nearest-to-center heuristic
                // (same as update_camera Watching branch) to avoid flip-flopping.
                let cam_center = cam.left_edge_f32() + world::SCREEN_W as f32 / 2.0;
                use game::soldier::SoldierState;
                let airborne = game.teams.iter().flat_map(|t| t.soldiers.iter())
                    .filter(|s| matches!(s.state, SoldierState::Airborne { .. }))
                    .map(|s| s.pos)
                    .min_by(|a, b| (a.x - cam_center).abs()
                        .partial_cmp(&(b.x - cam_center).abs()).unwrap());
                let ti = game.turn.current_team();
                let active_pos = game.teams.get(ti)
                    .and_then(|t| t.soldiers.get(t.active))
                    .map(|s| s.pos);
                if let Some(pos) = airborne.or(active_pos) { cam.follow(pos); }
            }
            if input.just_released(input::Button::R1) { cam.release_pan(); }
            game::loop_runner::update_visuals(&mut game);
            {
                use renderer::background;
                let gw = background::gust_wind(game.wind.value(), lstate.tick);
                background::update_debris(&mut lstate.bg_debris, &game.terrain, gw, lstate.tick);
            }
            cam.tick();
            game::loop_runner::render_live(&game, &mut buf, &mut cam, &mut lstate, my_team);
            // draw_weapon_menu_overlay uses game.weapon_menu_open — same source as tick()
            game::loop_runner::draw_weapon_menu_overlay(&game, &mut buf, cam.left_edge() as i32, cam.top_edge() as i32);
            if let Some(secs) = paused_secs {
                draw_disconnect_overlay(&mut buf, cam.left_edge() as i32, secs);
            }
            if opponent_left_ticks > 0 && paused_secs.is_none() {
                opponent_left_ticks -= 1;
                use renderer::font::{draw_str_scaled, str_width_scaled};
                let msg = "OPPONENT DISCONNECTED";
                let mw = str_width_scaled(msg, 1);
                let mx = cam.left_edge() as i32 + world::SCREEN_W as i32 / 2 - mw / 2;
                let alpha = (opponent_left_ticks.min(30) as f32 / 30.0 * 255.0) as u8;
                buf.fill_rect(mx - 6, 10, (mw + 12) as u32, 18, renderer::Bgra::new(40, 10, 10));
                draw_str_scaled(&mut buf, msg, mx, 13, renderer::Bgra::new(255, alpha / 2 + 80, alpha / 2 + 80), 1);
            } else if opponent_left_ticks > 0 {
                opponent_left_ticks -= 1;
            }
            // Safety net: if no state has arrived for 15 seconds and we have no
            // final result, the server has reset without us seeing the win state.
            // Force exit so the player isn't stuck frozen forever.
            if final_result.is_none() && last_state_time.elapsed().as_secs() > 15 {
                mp_quit = true;
            }
            // Opponent-quit blocking dialog — shown before the game-over screen.
            // The player must acknowledge before we exit.
            if opponent_abandoned && !opponent_quit_acked {
                draw_opponent_quit_overlay(&mut buf, cam.left_edge() as i32);
                if input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start) {
                    opponent_quit_acked = true;
                    mp_quit = true;
                }
            }
            // Game over overlay — use latched final_result so server's new-game reset can't clear it.
            // Skipped when the opponent-quit dialog is pending.
            else if let Some(ref fr) = final_result.clone() {
                // Report ranked result on first tick, capture ELO delta via channel
                if game_over_ticks == 0 && (live_ranked_match || is_live) {
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
                            let body = format!(r#"{{"token":"{}","winner_slot":{},"my_slot":{},"ranked":{},"session_token":"{}","kills":{},"deaths":{},"weapon_kills":{},"seed":{},"opponent":"{}"}}"#, tok, winner_slot, my_team, live_ranked_match, session_token, live_kills, live_deaths, wk_json, game_seed, live_opp_username);
                            let (dtx, drx) = std::sync::mpsc::channel::<(i32, u32)>();
                            std::thread::spawn(move || {
                                if let Ok(resp) = http_post("/api/match/live/result", &body) {
                                    let d = json_field(&resp, "elo_delta").and_then(|s| s.parse().ok()).unwrap_or(0);
                                    let s = json_field(&resp, "scrap_earned").and_then(|s| s.parse().ok()).unwrap_or(0);
                                    let _ = dtx.send((d, s));
                                }
                            });
                            elo_delta_rx = Some(drx);
                        }
                    }
                }
                // Poll for ELO delta result
                if let Some(ref rx) = elo_delta_rx {
                    if let Ok((d, s)) = rx.try_recv() { elo_delta = d; scrap_earned = s; elo_delta_rx = None; }
                }
                game_over_ticks += 1;
                let winner = if let game::state::GameResult::Winner(t) = fr { Some(*t) } else { None };
                let wa = if let Some(w) = winner { game.teams.get(w).map(|t| t.avatar_id).unwrap_or(0) } else { 0 };
                let (kills, hp_left, memo) = game::loop_runner::match_end_stats(&game);
                let wc = winner.and_then(|w| game.teams.get(w)).map(|t| t.color_id).unwrap_or(0);
                // Pass None (not Some(my_team)) so the headline reads
                // "RED/BLUE TEAM WINS!" identically to local modes. The ELO line
                // is gated on elo_delta != 0, so ranked still shows it.
                crate::renderer::hud::draw_game_over(&mut buf, winner, None, cam.left_edge() as i32, cam.top_edge(), wa, elo_delta, scrap_earned, kills, hp_left, &memo, wc);
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
                crate::renderer::hud::draw_pause_menu(&mut buf, lstate.pause_cursor as u8, cam.left_edge() as i32, cam.top_edge());
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
                            game.teams[ct].selected_weapon = cpu_state.weapon_idx;
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

        buf.blit_to_fb(&mut fb, cam.left_edge(), cam.top_edge());

        // Measure actual inter-blit interval so the counter reflects real display
        // cadence rather than loop iterations (sleep overshoot can make the two diverge).
        let now = Instant::now();
        fps_accum_us += now.duration_since(last_blit).as_micros() as u64;
        last_blit = now;
        fps_frame_count += 1;
        if fps_accum_us >= 1_000_000 {
            lstate.display_fps = (fps_frame_count as f64 / fps_accum_us as f64 * 1_000_000.0).round() as u32;
            fps_frame_count = 0;
            fps_accum_us = 0;
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

/// Build a client-side game with `team_count` teams (2-4). Soldier positions are
/// server-authoritative in live play (synced every StateMsg), so spawn placement
/// here only needs to be valid, not match the server exactly. The 2-team case
/// defers to the original full-map interleave for parity with local/ranked play.
fn build_default_game_n(seed: u64, team_count: usize) -> GameState {
    let n = team_count.clamp(2, 4);
    if n == 2 { return build_default_game(seed); }
    let mut terrain = Terrain::generate_tactical(seed);
    let all_spawns = terrain.find_team_spawns(0, WORLD_W, n * 4);
    let mut teams = Vec::with_capacity(n);
    for i in 0..n {
        let spawns: Vec<_> = all_spawns.iter().cloned()
            .enumerate().filter(|(k, _)| k % n == i).map(|(_, s)| s).collect();
        teams.push(Team::new(i, false, Difficulty::Medium, &spawns));
    }
    let mut game = GameState::new(seed, terrain, teams, n);
    place_map_mines(&mut game);
    place_map_barrels(&mut game);
    game
}

/// Casual lobby screen: announce our roster, let the player pick a colour and
/// ready up, and render everyone in the lobby until the server starts the match.
/// Returns the WelcomeMsg + final roster on start, or None if the player backs out.
fn run_casual_lobby(
    fb:       &mut Framebuffer,
    buf:      &mut WorldBuffer,
    input:    &mut input::InputState,
    conn:     &mut net::ServerConn,
    roster:   Option<&game::account::Roster>,
    username: &str,
) -> Option<(net::msg::WelcomeMsg, Vec<net::msg::LobbyPlayer>)> {
    use net::msg::{LobbyClientMsg, LobbyServerMsg, LobbyJoin};

    // Announce our roster.
    let join = match roster {
        Some(r) => LobbyJoin {
            name: r.name.clone(), username: username.to_string(),
            avatar_id: r.avatar_id, headstone_id: r.headstone_id,
            worm_names: r.worm_names.clone(), hat_ids: r.hat_ids,
            uniform_color_ids: r.uniform_color_ids, boot_color_ids: r.boot_color_ids,
            gun_style_ids: r.gun_style_ids,
        },
        None => LobbyJoin {
            name: "PLAYER".to_string(), username: username.to_string(),
            avatar_id: 0, headstone_id: 0,
            worm_names: [String::new(), String::new(), String::new(), String::new()],
            hat_ids: [0;4], uniform_color_ids: [0;4], boot_color_ids: [0;4], gun_style_ids: [0;4],
        },
    };
    conn.send(&LobbyClientMsg::Join(join));
    conn.set_read_timeout(Some(std::time::Duration::from_millis(60)));

    let mut players: Vec<net::msg::LobbyPlayer> = Vec::new();
    let mut your_index: usize = 0;
    let mut my_color: u8 = 0;
    let mut picked = false;   // whether we've sent an initial colour pick
    let mut ready = false;
    let mut frame: u32 = 0;
    let mut player_left_ticks: u32 = 0; // countdown for "player left" notice

    loop {
        frame = frame.wrapping_add(1);
        input.poll();
        if conn.is_disconnected() {
            draw_msg(buf, fb, "DISCONNECTED FROM SERVER  (B=BACK)");
            loop {
                input.poll();
                if input.just_pressed(input::Button::B) || input.just_pressed(input::Button::A) { break; }
                std::thread::sleep(std::time::Duration::from_millis(33));
            }
            return None;
        }
        if input.just_pressed(input::Button::B) {
            conn.send(&LobbyClientMsg::Leave);
            return None;
        }
        // Cycle colour (skip colours already taken by other players).
        let cycle = |dir: i32, cur: u8, players: &[net::msg::LobbyPlayer], me: usize| -> u8 {
            let taken: Vec<u8> = players.iter().enumerate()
                .filter(|(i, _)| *i != me)
                .filter_map(|(_, p)| p.color_id).collect();
            let mut c = cur as i32;
            for _ in 0..4 {
                c = (c + dir).rem_euclid(4);
                if !taken.contains(&(c as u8)) { break; }
            }
            c as u8
        };
        if !ready {
            let mut changed = false;
            if input.just_pressed(input::Button::Left)  { my_color = cycle(-1, my_color, &players, your_index); changed = true; }
            if input.just_pressed(input::Button::Right) { my_color = cycle( 1, my_color, &players, your_index); changed = true; }
            // Resend PickColor every 2s while players is empty — keeps connection alive and
            // triggers a new State from the server in case the initial one was missed.
            if changed || !picked || (players.is_empty() && frame % 60 == 0) {
                conn.send(&LobbyClientMsg::PickColor { color_id: my_color });
                picked = true;
            }
        }
        if input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start) {
            ready = !ready;
            conn.send(&LobbyClientMsg::SetReady { ready });
        }

        // Drain server messages.
        while let Some(m) = conn.recv_blocking::<LobbyServerMsg>() {
            match m {
                LobbyServerMsg::State { players: ps, your_index: yi } => {
                    if ps.len() < players.len() { player_left_ticks = 150; } // ~5s at 30fps
                    players = ps; your_index = yi;
                    if let Some(c) = players.get(your_index).and_then(|p| p.color_id) {
                        my_color = c; // server confirmed our colour
                    } else {
                        // Server hasn't confirmed our colour (conflict or initial join) —
                        // find the first free slot and request it.
                        let taken: Vec<u8> = players.iter().enumerate()
                            .filter(|(i, _)| *i != your_index)
                            .filter_map(|(_, p)| p.color_id)
                            .collect();
                        for d in 0..4u8 {
                            let c = (my_color + d) % 4;
                            if !taken.contains(&c) { my_color = c; break; }
                        }
                        conn.send(&LobbyClientMsg::PickColor { color_id: my_color });
                        picked = true;
                    }
                    if let Some(p) = players.get(your_index) { ready = p.ready; }
                }
                LobbyServerMsg::Start(w) => {
                    conn.set_read_timeout(None);
                    return Some((w, players));
                }
            }
            // After each message, switch to a short poll for any additional buffered messages.
            conn.set_read_timeout(Some(std::time::Duration::from_millis(10)));
        }
        // Back to normal drain timeout for next frame.
        conn.set_read_timeout(Some(std::time::Duration::from_millis(60)));

        draw_casual_lobby(buf, &players, your_index, ready);
        if player_left_ticks > 0 {
            player_left_ticks -= 1;
            use renderer::font::{draw_str_scaled, str_width_scaled};
            use renderer::Bgra;
            let msg = "A PLAYER LEFT THE LOBBY";
            let mw = str_width_scaled(msg, 1);
            let mx = world::SCREEN_W as i32 / 2 - mw / 2;
            let alpha = (player_left_ticks.min(30) as f32 / 30.0 * 255.0) as u8;
            buf.fill_rect(mx - 6, world::SCREEN_H as i32 - 56, (mw + 12) as u32, 18, Bgra::new(40, 10, 10));
            draw_str_scaled(buf, msg, mx, world::SCREEN_H as i32 - 53, Bgra::new(255, alpha / 2 + 80, alpha / 2 + 80), 1);
        }
        buf.blit_to_fb(fb, 0, 0);
        std::thread::sleep(std::time::Duration::from_millis(33));
    }
}

/// Render the casual lobby: a row per player with avatar, name (in team colour),
/// chosen colour and ready state, plus the controls hint.
fn draw_casual_lobby(
    buf:        &mut WorldBuffer,
    players:    &[net::msg::LobbyPlayer],
    your_index: usize,
    ready:      bool,
) {
    use renderer::Bgra;
    use renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
    use renderer::avatar::draw_avatar;
    use renderer::draw_sprites::TEAM_COLOURS;
    use world::{SCREEN_W, SCREEN_H};

    let sw = SCREEN_W as i32;
    let sh = SCREEN_H as i32;

    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
    // Header
    buf.fill_rect(0, 0, SCREEN_W, 40, Bgra::new(18, 22, 48));
    let title = "CASUAL LOBBY";
    let tw = str_width_scaled(title, 2);
    draw_str_scaled(buf, title, sw / 2 - tw / 2, 12, Bgra::new(255, 210, 50), 2);

    let colour_name = |c: Option<u8>| -> &'static str {
        match c { Some(0) => "RED", Some(1) => "BLUE", Some(2) => "GREEN", Some(3) => "YELLOW", _ => "PICK COLOUR" }
    };

    // Player rows
    const AV: u32 = 52;
    let row_h = 64i32;
    let top = 56i32;
    for slot in 0..4usize {
        let ry = top + slot as i32 * row_h;
        let occupied = slot < players.len();
        // Row panel
        let panel = if occupied { Bgra::new(20, 24, 44) } else { Bgra::new(14, 16, 30) };
        buf.fill_rect(20, ry, (sw - 40) as u32, (row_h - 8) as u32, panel);
        if slot == your_index && occupied {
            buf.fill_rect(20, ry, 4, (row_h - 8) as u32, Bgra::new(255, 210, 50));
        }

        if !occupied {
            let waiting = "WAITING FOR PLAYER...";
            draw_str(buf, waiting, 40, ry + row_h / 2 - 12, Bgra::new(70, 75, 95));
            continue;
        }

        let p = &players[slot];
        let col = p.color_id.map(|c| TEAM_COLOURS[c.min(3) as usize]).unwrap_or(Bgra::new(150, 150, 170));

        // Avatar
        draw_avatar(buf, 28, ry + 2, AV, p.avatar_id);
        // Colour swatch under avatar
        buf.fill_rect(28, ry + 2 + AV as i32, AV, 3, col);

        // Name (in team colour, 2x)
        let name = if p.name.is_empty() { "PLAYER".to_string() } else { p.name.to_uppercase() };
        draw_str_scaled(buf, &name, 92, ry + 8, col, 2);
        // Colour label
        draw_str(buf, colour_name(p.color_id), 92, ry + 30, col);
        // Account username
        if !p.username.is_empty() {
            draw_str(buf, &p.username, 92, ry + 42, Bgra::new(110, 115, 145));
        }

        // Ready indicator (right side)
        let label = if p.ready { "READY" } else { "NOT READY" };
        let lcol  = if p.ready { Bgra::new(80, 220, 120) } else { Bgra::new(180, 90, 90) };
        let lw = str_width_scaled(label, 2);
        draw_str_scaled(buf, label, sw - 40 - lw, ry + 18, lcol, 2);
    }

    // Footer / controls
    let ready_players = players.iter().filter(|p| p.ready).count();
    let status = if players.len() < 2 {
        "NEED 2+ PLAYERS TO START".to_string()
    } else if ready_players == players.len() {
        "ALL READY - STARTING...".to_string()
    } else {
        format!("{}/{} READY", ready_players, players.len())
    };
    let stw = str_width(&status);
    draw_str(buf, &status, sw / 2 - stw / 2, sh - 40, Bgra::new(200, 210, 240));

    let hint = if ready { "A: UNREADY   B: LEAVE" } else { "L/R: COLOUR   A: READY   B: LEAVE" };
    let hw = str_width(hint);
    draw_str(buf, hint, sw / 2 - hw / 2, sh - 24, Bgra::new(110, 115, 145));
}

fn build_default_game_opts(seed: u64, with_mines: bool, with_barrels: bool) -> GameState {
    let mut terrain = Terrain::generate_tactical(seed);

    // Worms-style: find 8 spawn points across the full map, then interleave between
    // teams so both can appear anywhere (no fixed left/right sides).
    let all_spawns = terrain.find_team_spawns(0, WORLD_W, 8);
    let team0_spawns: Vec<_> = all_spawns.iter().cloned().enumerate().filter(|(i,_)| i%2==0).map(|(_,s)|s).collect();
    let team1_spawns: Vec<_> = all_spawns.iter().cloned().enumerate().filter(|(i,_)| i%2==1).map(|(_,s)|s).collect();

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
    let mine_count = 16 + (seed % 9) as usize;  // 16–24

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
    let count = 14 + (seed.wrapping_mul(0xDEAD_C0DE) % 7) as usize; // 14–20

    let mut rng = seed.wrapping_mul(0xBEEF_1234_5678_9ABCu64).wrapping_add(1442695040888963407);
    let spread = WORLD_W / (count as u32 + 1);

    for i in 1..=count {
        rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
        let offset = (rng % spread as u64) as u32;
        let x = (spread * i as u32 + offset).clamp(20, WORLD_W - 20);
        if let Some(surf_y) = game.terrain.surface_y_at(x) {
            // pos.y = first air pixel above terrain (surf_y - 1); physics rests here.
            let pos = WorldPos::new(x as f32, surf_y as f32 - 1.0);
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
        buf.blit_to_fb(fb, 0, 0);
        let e = fs.elapsed();
        if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
    }
}

/// MULTIPLAYER → ACCOUNT entry point.
/// Not logged in: shows login/register screen.
/// Logged in: shows a small screen with username + logout option.
fn run_account_menu(
    fb:    &mut renderer::Framebuffer,
    input: &mut input::InputState,
    buf:   &mut WorldBuffer,
) {
    use renderer::Bgra;
    use renderer::font::{draw_str_scaled, draw_str_shadow_scaled, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};

    if let Some((username, _token)) = game::account::load_saved_creds() {
        // Already logged in — show "logged in as" + logout option
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        loop {
            let fs = std::time::Instant::now();
            input.poll();
            if input.just_pressed(input::Button::B) || input.just_pressed(input::Button::Start) { break; }
            if input.just_pressed(input::Button::Y) {
                game::account::clear_saved_creds();
                draw_msg(buf, fb, "LOGGED OUT");
                std::thread::sleep(std::time::Duration::from_secs(1));
                break;
            }
            buf.fill_rect(0, 0, SCREEN_W, SCREEN_H as u32, COLOR_DARK_BG);
            buf.fill_rect(0, 0, SCREEN_W, 36, Bgra::new(18, 22, 50));
            let tw = str_width_scaled("ACCOUNT", 2);
            draw_str_shadow_scaled(buf, "ACCOUNT", sw/2 - tw/2, 9, Bgra::new(255, 210, 50), 2);
            let msg = format!("LOGGED IN AS  {}", username.to_uppercase());
            let mw = str_width_scaled(&msg, 2);
            draw_str_scaled(buf, &msg, sw/2 - mw/2, sh/2 - 30, Bgra::new(120, 200, 120), 2);
            draw_str_scaled(buf, "Y  LOG OUT", sw/2 - str_width_scaled("Y  LOG OUT", 2)/2, sh/2 + 10, Bgra::new(220, 100, 80), 2);
            draw_str_scaled(buf, "B  BACK",    sw/2 - str_width_scaled("B  BACK", 2)/2,    sh/2 + 40, Bgra::new(140, 140, 140), 2);
            buf.blit_to_fb(fb, 0, 0);
            let e = fs.elapsed(); if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
        }
    } else {
        // Not logged in — show full login/register screen
        use game::account::{AccountScreen, AccountAction};
        let mut acct = AccountScreen::new();
        loop {
            let fs = std::time::Instant::now();
            input.poll();
            buf.fill_rect(0, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(8, 8, 20));
            if let Some(action) = acct.update(&input, buf, 0) {
                match action {
                    AccountAction::LoggedIn { username, .. } => {
                        let msg = format!("WELCOME  {}", username.to_uppercase());
                        draw_msg(buf, fb, &msg);
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                    AccountAction::Back => {}
                }
                break;
            }
            buf.blit_to_fb(fb, 0, 0);
            let e = fs.elapsed(); if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
        }
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
    use renderer::font::{draw_str_scaled, draw_str_shadow_scaled, str_width_scaled, draw_str, str_width};
    use world::{SCREEN_W, SCREEN_H};

    const ITEMS: &[&str] = &["ROSTERS", "STORE", "EQUIP", "PROFILE", "LOG OUT"];
    let mut cursor = 0usize;
    let mut scroll = 0usize;
    const VISIBLE: usize = 4;
    let username = game::account::load_saved_creds().map(|(u, _)| u).unwrap_or_default();

    loop {
        let fs = std::time::Instant::now();
        input.poll();

        if input.just_pressed(input::Button::B) { return; }
        let n = ITEMS.len();
        if input.just_pressed(input::Button::Up) {
            cursor = (cursor + n - 1) % n;
            if cursor < scroll { scroll = cursor; }
        }
        if input.just_pressed(input::Button::Down) {
            cursor = (cursor + 1) % n;
            if cursor == 0 { scroll = 0; }
            else if cursor >= scroll + VISIBLE { scroll = cursor + 1 - VISIBLE; }
        }

        if input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start) {
            match cursor {
                0 => { show_roster_picker(fb, input, buf, rosters, token); }
                1 => { show_store_screen(fb, input, buf, token); }
                2 => { show_equip_screen(fb, input, buf, rosters, token); }
                3 => { show_profile_screen(fb, input, buf, token, &username); }
                4 => {
                    game::account::clear_saved_creds();
                    draw_msg(buf, fb, "LOGGED OUT");
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    return;
                }
                _ => {}
            }
        }

        // Draw — same layout as title submenus
        renderer::title_bg::draw_title_bg(buf, 0);
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        let panel_y = 261i32;
        let item_h  = 34i32;
        if !username.is_empty() {
            use renderer::font::str_width;
            let uw = str_width(&username);
            draw_str(buf, &username, sw/2 - uw/2, panel_y - 14, Bgra::new(110, 115, 165));
        }

        let start_y = panel_y + 8;
        let visible_items = ITEMS.iter().enumerate().skip(scroll).take(VISIBLE);
        for (i, &item) in visible_items {
            let slot = (i - scroll) as i32;
            let iy = start_y + slot * item_h;
            let iw = str_width_scaled(item, 2);
            let selected = i == cursor;
            if selected {
                crate::renderer::hud::draw_menu_selection(buf, sw/2 - 155, iy - 4, 310, 28);
            }
            let col = if selected {
                if i == n - 1 { Bgra::new(255, 100, 80) } else { Bgra::new(255, 225, 55) }
            } else {
                if i == n - 1 { Bgra::new(180, 70, 60) } else { Bgra::new(0, 0, 0) }
            };
            draw_str_shadow_scaled(buf, item, sw/2 - iw/2, iy, col, 2);
        }
        // scroll indicators
        if scroll > 0 {
            let aw = str_width_scaled("▲", 2);
            draw_str_scaled(buf, "▲", sw/2 - aw/2, start_y - item_h, Bgra::new(150, 150, 180), 2);
        }
        if scroll + VISIBLE < n {
            let slot_y = start_y + VISIBLE as i32 * item_h;
            let aw = str_width_scaled("▼", 2);
            draw_str_scaled(buf, "▼", sw/2 - aw/2, slot_y, Bgra::new(150, 150, 180), 2);
        }
        crate::renderer::hud::draw_button_hints(buf, &[("A", "SELECT"), ("B", "BACK")], 0, 0);

        buf.blit_to_fb(fb, 0, 0);
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
    use game::account::{fetch_profile, load_cached_profile};
    use game::store::{StoreScreen, StoreAction};

    let cached = load_cached_profile().unwrap_or_default();
    let mut screen = StoreScreen::new(
        token.to_string(),
        cached.0,
        &cached.1,
        &cached.2,
        &cached.3,
        &cached.4,
        cached.5,
    );

    // Refresh profile from network in background
    let tok = token.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { tx.send(fetch_profile(&tok)).ok(); });

    loop {
        let fs = std::time::Instant::now();
        input.poll();
        if let Ok(Some(p)) = rx.try_recv() {
            screen.set_profile(p.0, &p.1, &p.2, &p.3, &p.4, p.5);
        }
        buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, renderer::Bgra::new(8, 12, 28));
        match screen.update(input, buf) {
            Some(StoreAction::Back) => return,
            None => {}
        }
        buf.blit_to_fb(fb, 0, 0);
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
    use game::account::{CosmeticsScreen, CosmeticsAction, fetch_profile, load_cached_profile, save_cached_rosters};

    if rosters.is_empty() { return; }

    // If only one roster, use it directly; otherwise let player pick
    let chosen = if rosters.len() == 1 {
        Some(rosters[0].clone())
    } else {
        show_roster_picker(fb, input, buf, rosters, token)
    };

    let roster = match chosen { Some(r) => r, None => return };

    let cached = load_cached_profile().unwrap_or_default();
    let mut screen = CosmeticsScreen::new(
        roster, cached.1, cached.2, cached.3, cached.4,
        token.to_string(), cached.0,
    );

    // Refresh profile from network in background
    let tok = token.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || { tx.send(fetch_profile(&tok)).ok(); });

    loop {
        let fs = std::time::Instant::now();
        input.poll();
        if let Ok(Some(p)) = rx.try_recv() {
            screen.set_profile(p.0, p.1, p.2, p.3, p.4);
        }
        buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, COLOR_DARK_BG);
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
        buf.blit_to_fb(fb, 0, 0);
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
        buf.fill_rect(0, 0, crate::world::SCREEN_W, crate::world::SCREEN_H as u32, COLOR_DARK_BG);
        if let Some(MissionsAction::Back) = screen.update(input, buf) { return; }
        buf.blit_to_fb(fb, 0, 0);
        let e = fs.elapsed();
        if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
    }
}

fn draw_status(buf: &mut renderer::WorldBuffer, fb: &mut renderer::Framebuffer, msg: &str) {
    use renderer::Bgra;
    use renderer::font::{draw_str_scaled, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
    let x = SCREEN_W as i32 / 2 - str_width_scaled(msg, 2) / 2;
    let y = SCREEN_H as i32 / 2 - 8;
    draw_str_scaled(buf, msg, x, y, Bgra::new(255, 210, 50), 2);
    buf.blit_to_fb(fb, 0, 0);
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

    let bg       = COLOR_DARK_BG;
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

    buf.blit_to_fb(fb, 0, 0);

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
    fb:        &mut Framebuffer,
    buf:       &mut WorldBuffer,
    input:     &mut input::InputState,
    game:      &game::state::GameState,
    my_team:   usize,
    usernames: &[String; 2],
    skippable: bool,
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

    for tick in 0u32..150 {
        input.poll();
        if skippable && (input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start)) { break; }

        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);

        // Header bar
        buf.fill_rect(0, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
        let title = "MATCH";
        let tw = str_width_scaled(title, 2);
        draw_str_scaled(buf, title, mid - tw/2, 12, Bgra::new(255, 210, 50), 2);

        for ti in 0..game.teams.len().min(2) {
            let t   = &game.teams[ti];
            let col = TEAM_COLOURS[t.color_id as usize];
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

            // Account username
            let uname = &usernames[ti];
            if !uname.is_empty() {
                let uw = str_width(uname);
                draw_str(buf, uname, hx - uw/2, av_y + AV as i32 + 30, Bgra::new(140, 140, 180));
            }

            // ELO (ranked only)
            if t.elo > 0 {
                let elo_str = format!("ELO  {}", t.elo);
                let ew = str_width(&elo_str);
                let elo_y = av_y + AV as i32 + if uname.is_empty() { 30 } else { 42 };
                draw_str(buf, &elo_str, hx - ew/2, elo_y, Bgra::new(180, 180, 100));
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
        draw_str_scaled(buf, vs, mid - vw/2, sh / 2 - 14, Bgra::new(255, 215, 50), 3);

        // Countdown bar
        let filled = (sw * (89i32 - tick as i32) / 89).max(0) as u32;
        buf.fill_rect(0, sh - 5, SCREEN_W, 5, Bgra::new(25, 25, 40));
        buf.fill_rect(0, sh - 5, filled, 5, Bgra::new(70, 70, 140));

        buf.blit_to_fb(fb, 0, 0);
        std::thread::sleep(TICK_DURATION);
    }
}

/// Full-screen status message, styled like the other menu screens (account,
/// missions, lobby): dark background with a yellow title in a header bar.
fn draw_msg(buf: &mut WorldBuffer, fb: &mut Framebuffer, msg: &str) {
    use renderer::Bgra;
    use renderer::font::{draw_str_scaled, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    let sw = SCREEN_W as i32;
    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
    buf.fill_rect(0, 0, SCREEN_W, 44, Bgra::new(18, 22, 48));
    let x = sw / 2 - str_width_scaled(msg, 2) / 2;
    draw_str_scaled(buf, msg, x, 10, Bgra::new(255, 210, 50), 2);
    buf.blit_to_fb(fb, 0, 0);
}

/// Styled reconnect-or-abandon dialog shown on the title screen after an
/// involuntary disconnect. `cursor`=0→RECONNECT selected, 1→ABANDON selected.
fn draw_reconnect_popup(buf: &mut WorldBuffer, fb: &mut Framebuffer, cursor: usize, secs_left: u64) {
    use renderer::Bgra;
    use renderer::font::{draw_str_scaled, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    let sw = SCREEN_W as i32;
    let sh = SCREEN_H as i32;
    // Dim full screen.
    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
    // Dialog box.
    let dw: i32 = 440;
    let dh: i32 = 220;
    let dx = (sw - dw) / 2;
    let dy = (sh - dh) / 2;
    // Border + background.
    buf.fill_rect(dx - 2, dy - 2, (dw + 4) as u32, (dh + 4) as u32, Bgra::new(60, 70, 120));
    buf.fill_rect(dx, dy, dw as u32, dh as u32, Bgra::new(12, 14, 30));
    // Header bar.
    buf.fill_rect(dx, dy, dw as u32, 40, Bgra::new(24, 28, 70));
    let title = "CONNECTION LOST";
    let tx = dx + (dw - str_width_scaled(title, 2)) / 2;
    draw_str_scaled(buf, title, tx, dy + 8, Bgra::new(255, 90, 90), 2);
    // Body text.
    let mins = secs_left / 60;
    let secs = secs_left % 60;
    let countdown = format!("Reconnect window: {}:{:02}", mins, secs);
    let cx = dx + (dw - str_width_scaled(&countdown, 1)) / 2;
    draw_str_scaled(buf, &countdown, cx, dy + 52, Bgra::new(160, 170, 200), 1);
    // Menu options.
    let options = ["RECONNECT", "ABANDON"];
    let option_colors = [Bgra::new(80, 220, 120), Bgra::new(200, 100, 100)];
    for (i, (opt, col)) in options.iter().zip(option_colors.iter()).enumerate() {
        let oy = dy + 100 + (i as i32) * 46;
        let highlight = cursor == i;
        if highlight {
            buf.fill_rect(dx + 20, oy - 4, (dw - 40) as u32, 36, Bgra::new(20, 24, 55));
            buf.fill_rect(dx + 20, oy - 4, (dw - 40) as u32, 36, Bgra::new(30, 35, 80));
            let arrow_x = dx + 30;
            draw_str_scaled(buf, ">", arrow_x, oy, Bgra::new(255, 210, 50), 2);
        }
        let ox = dx + (dw - str_width_scaled(opt, 2)) / 2;
        draw_str_scaled(buf, opt, ox, oy, if highlight { *col } else { Bgra::new(100, 110, 140) }, 2);
    }
    // Controls hint.
    let hint = "UP/DOWN  A=SELECT  B=ABANDON";
    let hx = dx + (dw - str_width_scaled(hint, 1)) / 2;
    draw_str_scaled(buf, hint, hx, dy + dh - 22, Bgra::new(80, 90, 120), 1);
    buf.blit_to_fb(fb, 0, 0);
}

/// In-game overlay drawn while waiting for the opponent to reconnect.
fn draw_disconnect_overlay(buf: &mut WorldBuffer, cam_left: i32, secs: u32) {
    use renderer::Bgra;
    use renderer::font::{draw_str_scaled, str_width_scaled};
    use world::SCREEN_W;
    let sw = SCREEN_W as i32;
    let mins = secs / 60;
    let s = secs % 60;
    let msg1 = "OPPONENT DISCONNECTED";
    let msg2 = format!("Reconnecting...  {}:{:02} remaining", mins, s);
    let bw: i32 = 400;
    let bh: i32 = 54;
    let bx = cam_left + (sw - bw) / 2;
    let by: i32 = 12;
    buf.fill_rect(bx - 2, by - 2, (bw + 4) as u32, (bh + 4) as u32, Bgra::new(50, 60, 110));
    buf.fill_rect(bx, by, bw as u32, bh as u32, Bgra::new(10, 12, 28));
    let x1 = bx + (bw - str_width_scaled(msg1, 1)) / 2;
    draw_str_scaled(buf, msg1, x1, by + 6, Bgra::new(255, 180, 50), 1);
    let x2 = bx + (bw - str_width_scaled(&msg2, 1)) / 2;
    draw_str_scaled(buf, &msg2, x2, by + 28, Bgra::new(160, 170, 200), 1);
}

/// Full-screen blocking overlay shown when the opponent intentionally quits.
fn draw_opponent_quit_overlay(buf: &mut WorldBuffer, cam_left: i32) {
    use renderer::Bgra;
    use renderer::font::{draw_str_scaled, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    let sw = SCREEN_W as i32;
    let sh = SCREEN_H as i32;
    // Semi-transparent dark layer.
    buf.fill_rect(cam_left, 0, SCREEN_W, SCREEN_H, Bgra::new(0, 0, 0));
    let dw: i32 = 420;
    let dh: i32 = 180;
    let dx = cam_left + (sw - dw) / 2;
    let dy = (sh - dh) / 2;
    buf.fill_rect(dx - 2, dy - 2, (dw + 4) as u32, (dh + 4) as u32, Bgra::new(80, 30, 30));
    buf.fill_rect(dx, dy, dw as u32, dh as u32, Bgra::new(14, 8, 8));
    buf.fill_rect(dx, dy, dw as u32, 42, Bgra::new(40, 12, 12));
    let t1 = "OPPONENT QUIT";
    let x1 = dx + (dw - str_width_scaled(t1, 2)) / 2;
    draw_str_scaled(buf, t1, x1, dy + 8, Bgra::new(255, 80, 80), 2);
    let t2 = "Your opponent has left the match.";
    let x2 = dx + (dw - str_width_scaled(t2, 1)) / 2;
    draw_str_scaled(buf, t2, x2, dy + 60, Bgra::new(200, 160, 160), 1);
    let t3 = "You win!";
    let x3 = dx + (dw - str_width_scaled(t3, 2)) / 2;
    draw_str_scaled(buf, t3, x3, dy + 82, Bgra::new(80, 220, 120), 2);
    let t4 = "PRESS A TO CONTINUE";
    let x4 = dx + (dw - str_width_scaled(t4, 1)) / 2;
    draw_str_scaled(buf, t4, x4, dy + dh - 28, Bgra::new(140, 150, 180), 1);
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
            buf.blit_to_fb(fb, 0, 0);
            let e = fs.elapsed(); if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
        };
        match result {
            AccountAction::LoggedIn { token, username, rosters } => (token, username, rosters),
            AccountAction::Back => return,
        }
    };

    if rosters.is_empty() { rosters.push(game::account::Roster::default_named(0)); }

    // Daily login bonus — fired off in the background (not awaited) so a
    // slow connection to the bonus endpoint can't freeze match start; the
    // reward is recorded server-side regardless of whether this popup shows.
    {
        let token = token.clone();
        std::thread::spawn(move || { game::account::claim_daily_login(&token); });
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
            buf.blit_to_fb(fb, 0, 0);
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
                    buf.blit_to_fb(fb, 0, 0);
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
                if let Some((new_move, tat_kills, tat_deaths, tat_weapon_kills, tat_end_screen)) = run_tat_game(fb, input, buf, seed, my_slot, &moves, &selected_roster.name, selected_roster.avatar_id, selected_roster.headstone_id, &selected_roster.worm_names, &selected_roster.hat_ids, &selected_roster.uniform_color_ids, &selected_roster.boot_color_ids, &selected_roster.gun_style_ids, my_elo, opp_elo, has_mines, has_barrels, &opp_name, &opp_worm_names, &opp_hat_ids, &opp_uniform_color_ids, &opp_boot_color_ids, &opp_gun_style_ids, days_remaining) {
                    // Submit move (with kill/death/weapon-kill stats) in background
                    use game::account::load_saved_creds;
                    let mut tat_scrap: u32 = 0;
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
                        // If the game ended this turn, POST the match result synchronously to get scrap
                        if tat_kills == 4 || tat_deaths == 4 {
                            let winner_slot = if tat_kills == 4 { my_slot } else { 1 - my_slot };
                            let result_body = format!(r#"{{"token":"{}","winner_slot":{},"kills":{},"deaths":{}}}"#,
                                token, winner_slot, tat_kills, tat_deaths);
                            let result_url = format!("/api/match/{}/result", match_id);
                            tat_scrap = game::account::http_post(&result_url, &result_body)
                                .ok()
                                .and_then(|resp| game::account::json_field(&resp, "scrap_earned").and_then(|s| s.parse().ok()))
                                .unwrap_or(0);
                        }
                    }
                    // Show game-over screen if match ended, otherwise move-submitted confirmation
                    if let Some((winner, av, col, kills, hp_left, memo)) = tat_end_screen {
                        let mut go_ticks = 0u32;
                        loop {
                            let fs = std::time::Instant::now();
                            input.poll();
                            crate::renderer::hud::draw_game_over(buf, winner, Some(my_slot), 0, 0, av, 0, tat_scrap, kills, hp_left, &memo, col);
                            buf.blit_to_fb(fb, 0, 0);
                            go_ticks += 1;
                            if go_ticks >= 300 || input.just_pressed(input::Button::A) || input.just_pressed(input::Button::Start) { break; }
                            let e = fs.elapsed();
                            if e < TICK_DURATION { std::thread::sleep(TICK_DURATION - e); }
                        }
                    } else {
                        draw_msg(buf, fb, "MOVE SUBMITTED");
                        std::thread::sleep(std::time::Duration::from_millis(1200));
                    }
                }
                lobby = LobbyScreen::new(
                    load_saved_creds().map(|(_, t)| t).unwrap_or_default(),
                    load_saved_creds().map(|(u, _)| u).unwrap_or_default(),
                    VERSION,
                );
            }
            None => {}
        }
        buf.blit_to_fb(fb, 0, 0);
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
) -> Option<(game::lobby::Move, u32, u32, std::collections::HashMap<&'static str, u32>, Option<(Option<usize>, u8, u8, [u32;2], [u32;2], String)>)> { // (move, kills, deaths, weapon_kills, end_screen)
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
        show_match_intro(fb, buf, input, &game, my_slot, &[String::new(), String::new()], true);
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
            let mut replay_cam = renderer::Camera::new(game.teams[opp_slot].soldiers[opp_si].pos.x, game.teams[opp_slot].soldiers[opp_si].pos.y);
            replay_cam.snap_to(game.teams[opp_slot].soldiers[opp_si].pos);
            // Clear messages accumulated during fast-forward — only the live replay's messages should show
            game.messages.clear();
            // Styled "OPPONENT'S MOVE" screen before replay begins
            crate::audio::set_muted(true);
            draw_msg(buf, fb, "OPPONENT'S MOVE");
            std::thread::sleep(std::time::Duration::from_millis(2000));
            crate::audio::set_muted(false);
            let mut prev_bits: u16 = 0;
            let input_len = mv.inputs.len();
            let mut replay_tick = 0usize;
            while replay_tick < input_len {
                let frame_start = std::time::Instant::now();
                let bits = mv.inputs[replay_tick];
                game::loop_runner::replay_tick(&mut game, prev_bits, bits);
                prev_bits = bits;
                replay_tick += 1;
                if let Some(ref hm) = game.homing_missile {
                    replay_cam.follow(world::WorldPos::new(if hm.confirmed { game.teams[opp_slot].soldiers[game.teams[opp_slot].active].pos.x } else { hm.render_x }, hm.render_y));
                } else if let Some(ref g) = game.garcia {
                    if g.falling { replay_cam.follow_always(world::WorldPos::new(g.render_x, g.fall_y.max(0.0))); }
                    else { let sy = game.teams[opp_slot].soldiers[game.teams[opp_slot].active].pos.y; replay_cam.follow(world::WorldPos::new(g.render_x, sy)); }
                } else if let Some(ref air) = game.airstrike {
                    if !air.active { replay_cam.follow(world::WorldPos::new(air.render_x, air.render_y)); }
                    else { replay_cam.follow(game.teams[opp_slot].soldiers[game.teams[opp_slot].active].pos); }
                } else if let Some(p) = game.projectiles.first() {
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
                buf.blit_to_fb(fb, replay_cam.left_edge(), replay_cam.top_edge());
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
                game::loop_runner::server_tick(&mut game, &empty, None, None);
                game.messages.retain(|m| !m.text.contains("got a ") && !m.text.contains("picked up"));
                if let Some(ref hm) = game.homing_missile {
                    replay_cam.follow(world::WorldPos::new(if hm.confirmed { game.teams[opp_slot].soldiers[game.teams[opp_slot].active].pos.x } else { hm.render_x }, hm.render_y));
                } else if let Some(ref g) = game.garcia {
                    if g.falling { replay_cam.follow_always(world::WorldPos::new(g.render_x, g.fall_y.max(0.0))); }
                    else { let sy = game.teams[opp_slot].soldiers[game.teams[opp_slot].active].pos.y; replay_cam.follow(world::WorldPos::new(g.render_x, sy)); }
                } else if let Some(ref air) = game.airstrike {
                    if !air.active { replay_cam.follow(world::WorldPos::new(air.render_x, air.render_y)); }
                    else { replay_cam.follow(game.teams[opp_slot].soldiers[game.teams[opp_slot].active].pos); }
                } else if let Some(p) = game.projectiles.first() {
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
                buf.blit_to_fb(fb, replay_cam.left_edge(), replay_cam.top_edge());
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
                let menu_open = game::loop_runner::replay_tick(&mut game, prev_bits, bits);
                prev_bits = bits;
                if !menu_open && game.teams[team].soldiers[game.teams[team].active].has_fired { break; }
            }
            if !game.teams[team].soldiers[game.teams[team].active].has_fired {
                game::loop_runner::fire_bazooka_tat(&mut game);
            }
            // 800 < TURN_TICKS(1350): prevents double-advance if break misses
            for _ in 0..800 {
                game::loop_runner::server_tick(&mut game, &empty, None, None);
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
    let mut cam = renderer::Camera::new(start_pos.x, crate::world::TERRAIN_MAX_Y as f32);
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

        buf.blit_to_fb(fb, cam.left_edge(), cam.top_edge());
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
        // Also exit if game ended (won or lost this turn) — caller shows game-over screen
        if !matches!(game.result, game::state::GameResult::Ongoing) {
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
    let end_screen = if !matches!(game.result, game::state::GameResult::Ongoing) {
        let winner = if let game::state::GameResult::Winner(t) = game.result { Some(t) } else { None };
        let av  = winner.and_then(|w| game.teams.get(w)).map(|t| t.avatar_id).unwrap_or(0);
        let col = winner.and_then(|w| game.teams.get(w)).map(|t| t.color_id).unwrap_or(0);
        let (kills, hp_left, memo) = game::loop_runner::match_end_stats(&game);
        Some((winner, av, col, kills, hp_left, memo))
    } else { None };
    Some((game::lobby::Move { angle: pre_angle, power: pre_power, facing: pre_facing, active_soldier: pre_active, inputs: recorded_inputs }, my_kills, my_deaths, weapon_kills, end_screen))
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
        if max_scroll > 0 {
            crate::renderer::hud::draw_button_hints(buf, &[("UP/DOWN", "SCROLL"), ("B", "BACK")], 0, 0);
        } else {
            crate::renderer::hud::draw_button_hints(buf, &[("B", "BACK")], 0, 0);
        }

        buf.blit_to_fb(fb, 0, 0);
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
        crate::renderer::hud::draw_button_hints(buf, &[("B", "BACK")], 0, 0);

        buf.blit_to_fb(fb, 0, 0);
        std::thread::sleep(TICK_DURATION);
    }
}

fn show_profile_screen(
    fb:    &mut renderer::Framebuffer,
    input: &mut input::InputState,
    buf:   &mut WorldBuffer,
    token: &str,
    username: &str,
) {
    use renderer::Bgra;
    use renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
    use world::{SCREEN_W, SCREEN_H};
    use game::account::{http_get, json_field};

    draw_msg(buf, fb, "LOADING...");

    let profile_resp = http_get(&format!("/api/profile?token={}", token)).unwrap_or_default();
    let live_resp    = http_get(&format!("/api/stats?mode=live&token={}", token)).unwrap_or_default();
    let tat_resp     = http_get(&format!("/api/stats?mode=tat&token={}", token)).unwrap_or_default();
    let history_resp = http_get(&format!("/api/match/history?token={}&limit=10", token)).unwrap_or_default();

    let live_w = json_field(&live_resp, "casual_wins").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0)
               + json_field(&live_resp, "ranked_wins").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let live_l = json_field(&live_resp, "casual_losses").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0)
               + json_field(&live_resp, "ranked_losses").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let live_k = json_field(&live_resp, "casual_kills").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0)
               + json_field(&live_resp, "ranked_kills").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let tat_w  = json_field(&tat_resp, "casual_wins").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0)
               + json_field(&tat_resp, "ranked_wins").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let tat_l  = json_field(&tat_resp, "casual_losses").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0)
               + json_field(&tat_resp, "ranked_losses").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let tat_k  = json_field(&tat_resp, "casual_kills").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0)
               + json_field(&tat_resp, "ranked_kills").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let elo      = json_field(&profile_resp, "elo").unwrap_or_default();
    let scrap    = json_field(&profile_resp, "scrap").unwrap_or_default();
    let warbonds = json_field(&profile_resp, "warbonds").unwrap_or_default();

    let sw = SCREEN_W as i32;
    let sh = SCREEN_H as i32;
    let head_col  = Bgra::new(255, 220, 50);
    let label_col = Bgra::new(130, 130, 160);
    let val_col   = Bgra::new(240, 240, 255);
    let dim_line  = Bgra::new(50, 50, 80);

    loop {
        input.poll();
        if input.just_pressed(input::Button::B) || input.just_pressed(input::Button::Start) { break; }

        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, COLOR_DARK_BG);
        // Header
        buf.fill_rect(0, 0, SCREEN_W, 36, Bgra::new(18, 22, 50));
        buf.fill_rect(0, 36, SCREEN_W, 1, dim_line);
        let tw = str_width_scaled("PROFILE", 2);
        draw_str_scaled(buf, "PROFILE", sw/2 - tw/2, 9, head_col, 2);

        let mut y = 50i32;
        let line = |buf: &mut WorldBuffer, label: &str, val: &str, y: i32| {
            draw_str(buf, label, 24, y, label_col);
            let vw = str_width(val);
            draw_str(buf, val, sw - 24 - vw, y, val_col);
        };

        // Username + ELO
        if !username.is_empty() {
            let uw = str_width_scaled(username, 2);
            draw_str_scaled(buf, username, sw/2 - uw/2, y, Bgra::new(200, 200, 255), 2);
            y += 20;
        }
        if !elo.is_empty() {
            let elo_str = format!("ELO  {}", elo);
            let ew = str_width(&elo_str);
            draw_str(buf, &elo_str, sw/2 - ew/2, y, Bgra::new(180, 220, 120));
            y += 20;
        }
        buf.fill_rect(20, y, (SCREEN_W - 40) as u32, 1, dim_line); y += 10;

        // Economy
        if !scrap.is_empty() || !warbonds.is_empty() {
            draw_str(buf, "BALANCE", 20, y, Bgra::new(140, 200, 255)); y += 18;
            if !scrap.is_empty()    { line(buf, "Scrap",    &scrap,    y); y += 16; }
            if !warbonds.is_empty() { line(buf, "Warbonds", &warbonds, y); y += 16; }
            buf.fill_rect(20, y, (SCREEN_W - 40) as u32, 1, dim_line); y += 10;
        }

        // Live stats
        draw_str(buf, "LIVE GAME", 20, y, Bgra::new(140, 200, 255)); y += 18;
        buf.fill_rect(20, y, (SCREEN_W - 40) as u32, 1, dim_line); y += 6;
        line(buf, "Wins",   &live_w.to_string(), y); y += 16;
        line(buf, "Losses", &live_l.to_string(), y); y += 16;
        line(buf, "Kills",  &live_k.to_string(), y); y += 20;

        // TAT stats
        draw_str(buf, "TAKE A TURN", 20, y, Bgra::new(140, 200, 255)); y += 18;
        buf.fill_rect(20, y, (SCREEN_W - 40) as u32, 1, dim_line); y += 6;
        line(buf, "Wins",   &tat_w.to_string(), y); y += 16;
        line(buf, "Losses", &tat_l.to_string(), y); y += 16;
        line(buf, "Kills",  &tat_k.to_string(), y); y += 20;

        // Match history
        if let Some(arr_start) = history_resp.find("\"history\":[") {
            let arr = &history_resp[arr_start + 11..];
            if arr.trim_start().starts_with('{') {
                buf.fill_rect(20, y, (SCREEN_W - 40) as u32, 1, dim_line); y += 8;
                draw_str(buf, "RECENT MATCHES", 20, y, Bgra::new(140, 200, 255)); y += 18;
                buf.fill_rect(20, y, (SCREEN_W - 40) as u32, 1, dim_line); y += 6;

                // Header row
                draw_str(buf, "RESULT", 20,  y, label_col);
                draw_str(buf, "OPPONENT",   130, y, label_col);
                draw_str(buf, "MODE",   340, y, label_col);
                draw_str(buf, "KILLS", 440, y, label_col);
                draw_str(buf, "SCRAP", 530, y, label_col);
                y += 14;
                buf.fill_rect(20, y, (SCREEN_W - 40) as u32, 1, Bgra::new(40, 40, 65)); y += 4;

                let mut depth = 0i32;
                let mut entry_start = 0usize;
                for (i, c) in arr.char_indices() {
                    match c {
                        '{' => { if depth == 0 { entry_start = i; } depth += 1; }
                        '}' => {
                            depth -= 1;
                            if depth == 0 && y < sh - 30 {
                                let entry = &arr[entry_start..=i];
                                let result  = json_field(entry, "result").unwrap_or_default();
                                let opp     = json_field(entry, "opponent").unwrap_or_default();
                                let mode    = json_field(entry, "mode").unwrap_or_default();
                                let ranked  = json_field(entry, "ranked").map(|s| s == "true").unwrap_or(false);
                                let kills   = json_field(entry, "kills").unwrap_or_default();
                                let scrap   = json_field(entry, "scrap").unwrap_or_else(|| "-".to_string());

                                let (res_str, res_col) = match result.as_str() {
                                    "win"  => ("WIN",  Bgra::new(80, 220, 100)),
                                    "loss" => ("LOSS", Bgra::new(220, 80, 80)),
                                    _      => ("DRAW", Bgra::new(160, 160, 160)),
                                };
                                let mode_str = if ranked {
                                    if mode == "live" { "RANKED LIVE" } else { "RANKED TAT" }
                                } else {
                                    if mode == "live" { "LIVE" } else { "TAT" }
                                };
                                let opp_display = if opp.is_empty() { "?" } else { &opp };
                                draw_str(buf, res_str,     20,  y, res_col);
                                draw_str(buf, opp_display, 130, y, val_col);
                                draw_str(buf, mode_str,    340, y, label_col);
                                draw_str(buf, &kills,      440, y, val_col);
                                draw_str(buf, &scrap,      530, y, Bgra::new(220, 200, 80));
                                y += 16;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        crate::renderer::hud::draw_button_hints(buf, &[("B", "BACK")], 0, 0);

        buf.blit_to_fb(fb, 0, 0);
        std::thread::sleep(TICK_DURATION);
    }
}
