mod msg;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use msg::*;
use log::info;

// Pull in the real game modules
use arty::world::{Heightmap, Terrain, WORLD_W};
use arty::game::{
    state::GameState,
    team::{Team, Difficulty},
    turn::TurnPhase,
};
use arty::world::WorldPos;

const PORT_DEFAULT: u16 = 7777;
const TICK_DURATION: Duration = Duration::from_millis(1000 / 30);

fn main() {
    // Log to a text file (in addition to terminal/journal when run attached).
    // ARTY_LOG_PATH overrides the default location.
    let log_path = std::env::var("ARTY_LOG_PATH")
        .unwrap_or_else(|_| "arty-server.log".to_string());
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .expect("failed to open log file");
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .target(env_logger::Target::Pipe(Box::new(log_file)))
        .format_timestamp_secs()
        .init();
    // ARTY_PORT env var lets the API spawn instances on different ports
    let port: u16 = std::env::var("ARTY_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(PORT_DEFAULT);
    info!("Miyoo Mayhem server on :{}", port);
    let listener = TcpListener::bind(("0.0.0.0", port)).expect("bind failed");
    let mut match_id: u64 = 0;
    let registry: Registry = Arc::new(Mutex::new(HashMap::new()));
    let mut pending: Option<(TcpStream, String)> = None;
    loop {
        let (stream, _addr, token) = accept_one(&listener);

        // Reconnect: token matches a paused slot of an in-progress match.
        if !token.is_empty() {
            let slot = registry.lock().unwrap_or_else(|e| e.into_inner()).get(&token).cloned();
            if let Some(slot) = slot {
                if reconnect_into(&slot, &stream) {
                    continue;
                }
            }
        }

        // Fresh pairing — first of a pair waits for the second. If the player
        // holding the `pending` slot gave up (closed their connection) before
        // a second player arrived, the dead stream would otherwise get paired
        // with this new, unrelated connection — starting a match against a
        // socket nobody is on the other end of. Discard dead pendings and let
        // this new connection take the slot instead.
        loop {
            match pending.take() {
                None => { pending = Some((stream, token)); break; }
                Some((s0, tok0)) => {
                    if !stream_alive(&s0) {
                        info!("Dropping dead pending connection");
                        continue;
                    }
                    let shared_token = if !tok0.is_empty() { tok0 } else { token };
                    match_id += 1;
                    let mid = match_id;
                    let registry2 = registry.clone();
                    thread::spawn(move || run_match(mid, s0, stream, registry2, shared_token));
                    break;
                }
            }
        }
    }
}

struct SharedConn {
    stream: TcpStream,
    inbox:  Arc<Mutex<Option<InputMsg>>>,
    disc:   Arc<AtomicBool>,
    gen:    Arc<AtomicU64>,
}

struct MatchSlot {
    conns: [Mutex<SharedConn>; 2],
    seed:  u64,
}

type Registry = Arc<Mutex<HashMap<String, Arc<MatchSlot>>>>;

/// Attempt to swap `stream` into whichever team slot of `slot` is currently
/// disconnected. Returns true on success (a fresh read thread was spawned).
fn reconnect_into(slot: &Arc<MatchSlot>, stream: &TcpStream) -> bool {
    for team in 0..2 {
        let mut sc = slot.conns[team].lock().unwrap_or_else(|e| e.into_inner());
        if sc.disc.load(Ordering::Relaxed) {
            let write_clone = match stream.try_clone() { Ok(s) => s, Err(_) => return false };
            let read_clone  = match stream.try_clone() { Ok(s) => s, Err(_) => return false };
            let welcome_clone = match stream.try_clone() { Ok(s) => s, Err(_) => return false };
            write_clone.set_nodelay(true).ok();
            write_clone.set_write_timeout(Some(Duration::from_millis(50))).ok();
            read_clone.set_read_timeout(Some(Duration::from_secs(5))).ok();
            sc.stream = write_clone;
            let new_gen = sc.gen.fetch_add(1, Ordering::Relaxed) + 1;
            sc.disc.store(false, Ordering::Relaxed);
            *sc.inbox.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let inbox = sc.inbox.clone();
            let disc = sc.disc.clone();
            let gen = sc.gen.clone();
            drop(sc);
            // Send a fresh WelcomeMsg so a client returning from the title screen
            // (no in-memory game state) can rebuild the match from the same seed;
            // the next StateMsg then syncs positions/terrain/HP to the live state.
            if let Some(bytes) = encode(&WelcomeMsg { your_team: team, seed: slot.seed }) {
                let mut s = &welcome_clone;
                let _ = s.write_all(&bytes);
            }
            thread::spawn(move || {
                read_loop(read_clone, inbox);
                if gen.load(Ordering::Relaxed) == new_gen {
                    disc.store(true, Ordering::Relaxed);
                }
            });
            info!("Reconnected into team {team}");
            return true;
        }
    }
    false
}

/// How long a paused match waits for the disconnected player to reconnect.
const RECONNECT_TIMEOUT: Duration = Duration::from_secs(180);

/// Open (creating if needed) a per-match log file alongside the main server
/// log, so each game's history can be tailed/grepped independently when many
/// matches run concurrently. Falls back to /dev/null if it can't be opened.
fn match_log_file(match_id: u64) -> std::fs::File {
    let dir = std::env::var("ARTY_LOG_PATH").ok()
        .and_then(|p| std::path::Path::new(&p).parent().map(|d| d.to_path_buf()))
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let path = dir.join(format!("match-{match_id}.log"));
    std::fs::OpenOptions::new().create(true).append(true).open(&path)
        .unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap())
}

/// Write a timestamped line to a per-match log file.
fn mlog(f: &mut std::fs::File, msg: &str) {
    use std::io::Write as _;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let _ = writeln!(f, "[{now}] {msg}");
}

/// Log to both the shared server log (with the `[match N]` prefix, as before)
/// and this match's own log file.
macro_rules! mboth {
    ($mfile:expr, $match_id:expr, $($arg:tt)*) => {{
        let msg = format!($($arg)*);
        info!("[match {}] {}", $match_id, msg);
        mlog($mfile, &msg);
    }};
}

fn run_match(match_id: u64, s0: TcpStream, s1: TcpStream, registry: Registry, session_token: String) {
    let mut mfile = match_log_file(match_id);
    mboth!(&mut mfile, match_id, "starting");
    s0.set_nodelay(true).ok();
    s1.set_nodelay(true).ok();
    s0.set_write_timeout(Some(Duration::from_millis(50))).ok();
    s1.set_write_timeout(Some(Duration::from_millis(50))).ok();
    s0.set_read_timeout(Some(Duration::from_secs(15))).ok();
    s1.set_read_timeout(Some(Duration::from_secs(15))).ok();
    mboth!(&mut mfile, match_id, "Both connected - starting!");
    thread::sleep(Duration::from_secs(2));

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

    send_msg(&s0, &WelcomeMsg { your_team: 0, seed });
    send_msg(&s1, &WelcomeMsg { your_team: 1, seed });

    let inp0: Arc<Mutex<Option<InputMsg>>> = Arc::new(Mutex::new(None));
    let inp1: Arc<Mutex<Option<InputMsg>>> = Arc::new(Mutex::new(None));

    // Atomic flags set by read threads the instant a client TCP connection closes.
    // Much more reliable than write errors — writes succeed even when client is gone
    // because data sits in the kernel TCP send buffer.
    let disc0 = Arc::new(AtomicBool::new(false));
    let disc1 = Arc::new(AtomicBool::new(false));
    let gen0 = Arc::new(AtomicU64::new(0));
    let gen1 = Arc::new(AtomicU64::new(0));

    let (read_s0, ws0, ws1, read_s1) = match (
        s0.try_clone(), s0.try_clone(), s1.try_clone(), s1.try_clone()
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        _ => { mboth!(&mut mfile, match_id, "socket clone failed — aborting"); return; }
    };
    thread::spawn({
        let i = inp0.clone(); let d = disc0.clone(); let g = gen0.clone();
        move || { read_loop(read_s0, i); if g.load(Ordering::Relaxed) == 0 { d.store(true, Ordering::Relaxed); } }
    });
    thread::spawn({
        let i = inp1.clone(); let d = disc1.clone(); let g = gen1.clone();
        move || { read_loop(read_s1, i); if g.load(Ordering::Relaxed) == 0 { d.store(true, Ordering::Relaxed); } }
    });

    let match_slot = Arc::new(MatchSlot { conns: [
        Mutex::new(SharedConn { stream: ws0, inbox: inp0.clone(), disc: disc0.clone(), gen: gen0.clone() }),
        Mutex::new(SharedConn { stream: ws1, inbox: inp1.clone(), disc: disc1.clone(), gen: gen1.clone() }),
    ], seed });
    let reconnectable = !session_token.is_empty();
    if reconnectable {
        registry.lock().unwrap_or_else(|e| e.into_inner()).insert(session_token.clone(), match_slot.clone());
    }
    macro_rules! write_team {
        ($team:expr, $bytes:expr) => {{
            let sc = match_slot.conns[$team].lock().unwrap_or_else(|e| e.into_inner());
            let mut s = &sc.stream; let _ = s.write_all($bytes);
        }};
    }

    let mut game = build_game(seed);
    let mut tick: u32 = 0;
    let mut paused: Option<(usize, Instant)> = None; // (disconnected_team, since)
    // Index into game.crater_log of the first crater not yet sent to clients
    // via the main per-tick broadcast — see `build_state`.
    let mut last_craters_sent: usize = 0;

    loop {
        let t = Instant::now();

        // Pause/resume handling — only meaningful when registered (session_token set).
        if let Some((dteam, since)) = paused {
            let still_disc = match dteam { 0 => disc0.load(Ordering::Relaxed), _ => disc1.load(Ordering::Relaxed) };
            if !still_disc {
                let name = game.teams[dteam].soldiers.get(0).map(|s| s.name.as_str()).unwrap_or("?");
                mboth!(&mut mfile, match_id, "team {dteam} ({name}) reconnected — resuming");
                // Reconnecting client has no crater history — send the full
                // backlog once (one-off catch-up cost is fine here).
                if let Some(state_bytes) = encode(&build_state(&game, tick, 0)) {
                    write_team!(dteam, &state_bytes);
                }
                last_craters_sent = game.crater_log.len();
                paused = None;
            } else if since.elapsed() >= RECONNECT_TIMEOUT {
                let name = game.teams[dteam].soldiers.get(0).map(|s| s.name.as_str()).unwrap_or("?");
                mboth!(&mut mfile, match_id, "team {dteam} ({name}) did not reconnect — ending match");
                let connected = 1 - dteam;
                let mut state = build_state(&game, tick, last_craters_sent);
                state.opponent_abandoned = true;
                state.result = NetResult::Winner(connected);
                if let Some(bytes) = encode(&state) { write_team!(connected, &bytes); }
                break;
            } else {
                let mut state = build_state(&game, tick, last_craters_sent);
                state.paused_opponent = Some((RECONNECT_TIMEOUT - since.elapsed()).as_secs() as u32);
                if let Some(bytes) = encode(&state) { write_team!(1 - dteam, &bytes); }
                let e = t.elapsed();
                if e < TICK_DURATION { thread::sleep(TICK_DURATION - e); }
                continue;
            }
        }

        tick = tick.wrapping_add(1);
        game.tick = tick;

        // Disconnect detection via read-thread flags
        if disc0.load(Ordering::Relaxed) || disc1.load(Ordering::Relaxed) {
            let dteam = if disc0.load(Ordering::Relaxed) { 0 } else { 1 };
            let name = game.teams[dteam].soldiers.get(0).map(|s| s.name.as_str()).unwrap_or("?");
            if reconnectable {
                mboth!(&mut mfile, match_id, "team {dteam} ({name}) disconnected — pausing");
                paused = Some((dteam, Instant::now()));
                continue;
            }
            mboth!(&mut mfile, match_id, "Client disconnected — resetting (team {dteam} = {name}, disc0={} disc1={})",
                disc0.load(Ordering::Relaxed), disc1.load(Ordering::Relaxed));
            break;
        }

        let i1 = inp1.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let i0 = inp0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        // Apply cosmetics/names from both clients every tick so the server's game
        // state reflects each player's roster and StateMsg broadcasts them to the opponent.
        for (team, msg_opt) in [(0usize, &i0), (1usize, &i1)] {
            if let Some(msg) = msg_opt {
                if let Some(t) = game.teams.get_mut(team) {
                    for si in 0..t.soldiers.len().min(4) {
                        t.soldiers[si].hat_id           = msg.hat_ids[si];
                        t.soldiers[si].uniform_color_id = msg.uniform_color_ids[si];
                        t.soldiers[si].boot_color_id    = msg.boot_color_ids[si];
                        t.soldiers[si].gun_style_id     = msg.gun_style_ids[si];
                        if !msg.worm_names[si].is_empty() {
                            t.soldiers[si].name = msg.worm_names[si].clone();
                        }
                    }
                }
            }
        }
        let active = game.active_team();
        let inp = if active == 0 { i0 } else { i1 };
        let mut input_state = inp.as_ref().map(msg_to_input).unwrap_or_else(arty::input::InputState::new);

        // Apply client's authoritative aim angle directly; strip Up/Down so
        // process_aim doesn't double-apply them on top of the received angle.
        if let Some(ref msg) = inp {
            game.aim.angle = msg.aim_angle;
            use arty::input::Button;
            input_state.clear_button(Button::Up);
            input_state.clear_button(Button::Down);
            // Apply the client's authoritative weapon selection BY KIND. The client's
            // loadout indices can diverge from the server's (the server prunes spent
            // weapons; the client doesn't simulate), so selecting by index fired the
            // wrong weapon. Selecting by kind is index-independent.
            use arty::physics::projectile::WeaponKind;
            let kind = WeaponKind::from_net_u8(msg.selected_weapon_kind);
            let ti = game.active_team();
            if let Some(idx) = game.teams[ti].weapons.iter().position(|(w, _)| *w == kind) {
                game.teams[ti].selected_weapon = idx;
            }
        }
        // Clear one-shot events so pressed/released dont repeat
        if active == 0 { if let Some(ref mut i) = *inp0.lock().unwrap_or_else(|e| e.into_inner()) { i.pressed.clear(); i.released.clear(); } }
        else           { if let Some(ref mut i) = *inp1.lock().unwrap_or_else(|e| e.into_inner()) { i.pressed.clear(); i.released.clear(); } }
        // Snapshot HP before the tick so we can detect damage/kills caused by it,
        // and attribute them to whichever team's turn it was (the acting player —
        // also correct for self-damage, where attacker == victim's team).
        let attacker_team = game.turn.current_team;
        let hp_before: Vec<Vec<(u8, bool)>> = game.teams.iter()
            .map(|t| t.soldiers.iter().map(|s| (s.hp, s.is_alive())).collect())
            .collect();

        arty::game::loop_runner::server_tick(&mut game, &input_state);

        // Log per-soldier damage/kills caused by this tick, with the weapon
        // responsible (kill_weapon is set on every damaging hit, not just kills).
        for (ti, team) in game.teams.iter().enumerate() {
            for (si, soldier) in team.soldiers.iter().enumerate() {
                let (hp_pre, alive_pre) = hp_before[ti][si];
                use arty::game::soldier::DeathCause;
                let cause_label = match soldier.death_cause {
                    DeathCause::Fall  => "FALL".to_string(),
                    DeathCause::Water => "DROWNING".to_string(),
                    _ => soldier.kill_weapon.map(|w| w.display_name().to_string()).unwrap_or_else(|| "UNKNOWN".to_string()),
                };
                if soldier.hp < hp_pre {
                    let dmg = hp_pre - soldier.hp;
                    mboth!(&mut mfile, match_id,
                        "DAMAGE: team {attacker_team} -> team {ti} soldier {si} for {dmg} with {cause_label} (hp {hp_pre}->{})",
                        soldier.hp);
                }
                if alive_pre && !soldier.is_alive() {
                    mboth!(&mut mfile, match_id,
                        "KILL: team {attacker_team} killed team {ti} soldier {si} with {cause_label}");
                }
            }
        }

        // Game over — send final state for 3 seconds then start a new game
        if !matches!(game.result, arty::game::state::GameResult::Ongoing) {
            if let Some(final_bytes) = encode(&build_state(&game, tick, last_craters_sent)) {
            for _ in 0..90 {
                if disc0.load(Ordering::Relaxed) || disc1.load(Ordering::Relaxed) { break; }
                write_team!(0, &final_bytes);
                write_team!(1, &final_bytes);
                thread::sleep(TICK_DURATION);
            }
            } // end if let Some(final_bytes)
            if disc0.load(Ordering::Relaxed) || disc1.load(Ordering::Relaxed) {
                mboth!(&mut mfile, match_id, "Client left during game-over — resetting");
                break;
            }
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            game = build_game(seed);
            tick = 0;
            last_craters_sent = 0;
            *inp0.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *inp1.lock().unwrap_or_else(|e| e.into_inner()) = None;
            if let Some(bytes) = encode(&WelcomeMsg { your_team: 0, seed }) { write_team!(0, &bytes); }
            if let Some(bytes) = encode(&WelcomeMsg { your_team: 1, seed }) { write_team!(1, &bytes); }
            mboth!(&mut mfile, match_id, "Game over — new game with seed {}", seed);
            continue;
        }

        if let Some(state_bytes) = encode(&build_state(&game, tick, last_craters_sent)) {
            last_craters_sent = game.crater_log.len();
            write_team!(0, &state_bytes);
            write_team!(1, &state_bytes);
        }

        let e = t.elapsed();
        if e < TICK_DURATION { thread::sleep(TICK_DURATION - e); }
    }
    if reconnectable {
        registry.lock().unwrap_or_else(|e| e.into_inner()).remove(&session_token);
    }
    mboth!(&mut mfile, match_id, "ended");
}

fn apply_input(game: &mut GameState, input: &InputMsg) {
    use arty::input::Button;
    let ti = game.active_team();
    let si = game.teams[ti].active;

    let held = |b: NetButton| input.held.contains(&b);
    let pressed  = |b: NetButton| input.pressed.contains(&b);
    let released = |b: NetButton| input.released.contains(&b);

    // Movement
    if held(NetButton::Left) {
        let nx = game.teams[ti].soldiers[si].pos.x - 2.0;
        let nx = nx.clamp(0.0, (WORLD_W - 1) as f32);
        game.teams[ti].soldiers[si].pos.x = nx;
        game.teams[ti].soldiers[si].facing = -1;
        snap_to_surface(game, ti, si);
    }
    if held(NetButton::Right) {
        let nx = game.teams[ti].soldiers[si].pos.x + 2.0;
        let nx = nx.clamp(0.0, (WORLD_W - 1) as f32);
        game.teams[ti].soldiers[si].pos.x = nx;
        game.teams[ti].soldiers[si].facing = 1;
        snap_to_surface(game, ti, si);
    }

    // Aim
    let delta = 0.08f32;
    if held(NetButton::Up)   { game.aim.angle += delta; }
    if held(NetButton::Down) { game.aim.angle -= delta; }

    // Plasma torch: step aim through 3 valid directions on Up/Down press.
    use arty::physics::projectile::WeaponKind;
    if game.teams[ti].current_weapon() == WeaponKind::PlasmaTorch && game.plasma_torch.is_none() {
        const TORCH_ANGLE: f32 = 0.611;
        game.aim.angle = if game.aim.angle > TORCH_ANGLE * 0.5 { TORCH_ANGLE }
                         else if game.aim.angle < -TORCH_ANGLE * 0.5 { -TORCH_ANGLE }
                         else { 0.0 };
        if pressed(NetButton::Up) {
            game.aim.angle = (game.aim.angle + TORCH_ANGLE).min(TORCH_ANGLE);
        }
        if pressed(NetButton::Down) {
            game.aim.angle = (game.aim.angle - TORCH_ANGLE).max(-TORCH_ANGLE);
        }
    }

    // Fire
    if held(NetButton::A) {
        game.aim.power = (game.aim.power + 1.0).min(100.0);
    }
    // Jump
    let on_ground = is_on_ground(game, ti, si);
    if pressed(NetButton::B) && on_ground {
        use arty::game::soldier::SoldierState;
        use arty::world::Vec2;
        let vx = game.teams[ti].soldiers[si].facing as f32 * 5.0;
        game.teams[ti].soldiers[si].pos.y -= arty::game::loop_runner::jump_unstick_lift(game, ti, si);
        game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel: Vec2::new(vx, -4.0), spinning: false };
        game.teams[ti].soldiers[si].airtime = 0;
    }
    if pressed(NetButton::Y) && on_ground {
        use arty::game::soldier::SoldierState;
        use arty::world::Vec2;
        let vx = game.teams[ti].soldiers[si].facing as f32 * -1.5;
        game.teams[ti].soldiers[si].pos.y -= arty::game::loop_runner::jump_unstick_lift(game, ti, si);
        game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel: Vec2::new(vx, -6.5), spinning: true };
        // Reset airtime so the backflip spin always plays in full (see loop_runner).
        game.teams[ti].soldiers[si].airtime = 0;
    }
    if released(NetButton::A) && game.aim.power > 0.0 {
        fire_bazooka(game);
        game.aim.power = 0.0;
    }
    // TNT: instant placement on A press (no charge), locked until turn 5.
    if pressed(NetButton::A) && game.aim.power == 0.0 {
        use arty::physics::projectile::WeaponKind;
        let weapon = game.teams[ti].current_weapon();
        if weapon == WeaponKind::Tnt && game.turn.turn_number >= 5 * game.teams.len() as u32 {
            arty::game::loop_runner::fire_tnt(game, ti, si);
        }
        if weapon == WeaponKind::Landmine {
            arty::game::loop_runner::fire_mine(game, ti, si);
        }
    }
}

fn fire_bazooka(game: &mut GameState) {
    use arty::physics::projectile::{Projectile, WeaponKind};
    use arty::world::Vec2;
    let ti = game.active_team();
    let si = game.teams[ti].active;
    let fm = game.teams[ti].soldiers[si].facing as f32;
    let angle = game.aim.angle;
    let power = game.aim.power / 100.0 * 20.0;
    let sx = game.teams[ti].soldiers[si].pos.x + fm * 8.0;
    let sy = game.teams[ti].soldiers[si].pos.y - 4.0;
    game.projectiles.push(Projectile::new(WorldPos::new(sx, sy), Vec2::new(angle.cos() * power * fm, -angle.sin() * power), WeaponKind::Bazooka));
    game.teams[ti].soldiers[si].has_fired = true;
    game.turn.on_fired();
}

fn build_game(seed: u64) -> GameState {
    let mut terrain = arty::world::Terrain::generate_tactical(seed);
    // Team 0 spawns in the left interior, team 1 in the right interior. Shared finder
    // (identical to the client) keeps both teams ≥ SPAWN_EDGE_MARGIN from the edges.
    let team0 = terrain.find_team_spawns(0, WORLD_W / 2 - 40, 4);
    let team1 = terrain.find_team_spawns(WORLD_W / 2 + 40, WORLD_W, 4);
    let teams = vec![
        Team::new(0, false, Difficulty::Medium, &team0),
        Team::new(1, false, Difficulty::Medium, &team1),
    ];
    let mut game = GameState::new(seed, terrain, teams, 2);
    place_map_mines(&mut game);
    place_map_barrels(&mut game);
    game
}

fn place_map_mines(game: &mut GameState) {
    use arty::game::state::{PlacedMine, MineState};
    let seed = game.map_seed;
    let mine_count = 9 + (seed % 7) as usize;
    let mut rng = seed.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
    let spread = WORLD_W / (mine_count as u32 + 1);
    for i in 1..=mine_count {
        rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
        let offset = (rng % spread as u64) as u32;
        let x = (spread * i as u32 + offset).clamp(20, WORLD_W - 20);
        if let Some(surf_y) = game.terrain.surface_y_at(x) {
            let mine_pos = WorldPos::new(x as f32, surf_y as f32 - 1.0);
            if (surf_y as f32) < arty::world::WATER_Y as f32 - 10.0
                && !too_close_to_soldiers_srv(game, mine_pos)
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

fn place_map_barrels(game: &mut GameState) {
    use arty::game::state::{Barrel, BarrelState};
    let seed = game.map_seed;
    let count = 7 + (seed.wrapping_mul(0xDEAD_C0DE) % 5) as usize;
    let mut rng = seed.wrapping_mul(0xBEEF_1234_5678_9ABCu64).wrapping_add(1442695040888963407);
    let spread = WORLD_W / (count as u32 + 1);
    for i in 1..=count {
        rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
        let offset = (rng % spread as u64) as u32;
        let x = (spread * i as u32 + offset).clamp(20, WORLD_W - 20);
        if let Some(surf_y) = game.terrain.surface_y_at(x) {
            let pos = WorldPos::new(x as f32, surf_y as f32 - 11.0);
            if (surf_y as f32) < arty::world::WATER_Y as f32 - 10.0
                && !too_close_to_soldiers_srv(game, pos)
            {
                game.barrels.push(Barrel {
                    pos,
                    vel: arty::world::Vec2::new(0.0, 0.0),
                    hp: 60,
                    state: BarrelState::Normal,
                });
            }
        }
    }
}

fn too_close_to_soldiers_srv(game: &GameState, pos: WorldPos) -> bool {
    const EXCLUSION: f32 = 50.0;
    game.teams.iter().flat_map(|t| t.soldiers.iter()).any(|s| {
        let dx = s.pos.x - pos.x;
        let dy = s.pos.y - pos.y;
        (dx * dx + dy * dy).sqrt() < EXCLUSION
    })
}

fn build_state(game: &GameState, tick: u32, _crater_start: usize) -> StateMsg {
    let phase = match game.turn.phase {
        TurnPhase::Acting       => NetPhase::Acting,
        TurnPhase::Watching     => NetPhase::Watching,
        TurnPhase::Retreating{..}=> NetPhase::Retreating,
        TurnPhase::Ending       => NetPhase::Ending,
    };
    StateMsg {
        tick,
        soldiers: game.teams.iter().flat_map(|t| t.soldiers.iter().map(|s| {
            use arty::game::soldier::SoldierState;
            let (airborne, spinning) = match s.state {
                SoldierState::Airborne { spinning, .. } => (true, spinning),
                _ => (false, false),
            };
            NetSoldier {
                team: t.slot, index: s.index,
                x: s.pos.x, y: s.pos.y,
                hp: s.hp, facing: s.facing, dead: s.is_dead(), has_fired: s.has_fired,
                selected_weapon: t.selected_weapon,
                airborne, spinning, airtime: s.airtime, walk_ticks: s.walk_ticks,
                walking: matches!(s.state, arty::game::soldier::SoldierState::Walking { .. }),
                hat_id: s.hat_id, uniform_color_id: s.uniform_color_id,
                boot_color_id: s.boot_color_id, gun_style_id: s.gun_style_id,
                name: s.name.clone(),
                death_cause_u8: {
                    use arty::game::soldier::DeathCause;
                    match s.death_cause {
                        DeathCause::Generic => 0, DeathCause::Explosion => 1,
                        DeathCause::Fall => 2, DeathCause::Water => 3,
                    }
                },
            }
        })).collect(),
        projectiles: game.projectiles.iter().map(|p| {
            use arty::physics::projectile::FuseState;
            NetProjectile {
                x: p.pos.x, y: p.pos.y,
                vel_x: p.vel.x, vel_y: p.vel.y,
                kind_u8: p.kind.to_net_u8(),
                fuse_ticks: match p.fuse { FuseState::Burning(n) => n, _ => 0 },
                is_fragment: p.is_fragment,
            }
        }).collect(),
        wind: game.wind.value(),
        turn_team: game.active_team(),

        turn_secs: game.turn.secs_remaining(),
        active_soldier: game.teams[game.active_team()].active,
        phase,
        aim_angle: game.aim.angle,
        aim_power: game.aim.power,
        result: match game.result {
            arty::game::state::GameResult::Ongoing   => NetResult::Ongoing,
            arty::game::state::GameResult::Winner(t) => NetResult::Winner(t),
            arty::game::state::GameResult::Draw      => NetResult::Draw,
        },
        // Send the full crater log every tick — a StateMsg can be silently
        // dropped (write_team! ignores write errors under the 50ms write
        // timeout), and a dropped delta would permanently desync the
        // client's terrain. The client dedupes by length (see apply_server_state).
        craters: game.crater_log.iter()
            .map(|e| NetCrater { cx: e.0, cy: e.1, radius: e.2 }).collect(),
        messages: game.messages.iter()
            .map(|m| NetMessage { text: m.text.clone(), team: m.team.map(|t| t as i8).unwrap_or(-1), ticks: m.ticks })
            .collect(),
        graves: game.graves.iter().map(|g| NetGrave {
            x: g.pos.x, y: g.pos.y, team: g.team, headstone_id: g.headstone_id,
        }).collect(),
        blood_splats: game.blood_splats.iter().map(|(p, t)| NetBloodSplat {
            x: p.x, y: p.y, ticks: *t,
        }).collect(),
        weapon_menu_open:   game.weapon_menu_open,
        weapon_menu_cursor: game.weapon_menu_cursor,
        aim_fuse_ticks:     game.aim.fuse_ticks,
        crates: game.crates.iter().map(|c| {
            use arty::game::state::CrateKind;
            NetCrate {
                x: c.pos.x, y: c.pos.y, landed: c.landed,
                kind_u8: match c.kind { CrateKind::Health => 0, CrateKind::Weapon(_) => 1, CrateKind::Scrap(_) => 2 },
            }
        }).collect(),
        mines: game.mines.iter().map(|m| {
            use arty::game::state::MineState;
            NetMine {
                x: m.pos.x, y: m.pos.y,
                state_u8: match m.state { MineState::Arming => 0, MineState::Armed => 1, MineState::Triggered => 2 },
                arm_ticks: m.arm_ticks,
                trigger_ticks: m.trigger_ticks,
            }
        }).collect(),
        sounds: game.sounds.clone(),
        barrels: game.barrels.iter().map(|b| NetBarrel {
            x: b.pos.x, y: b.pos.y, hp: b.hp,
        }).collect(),
        black_holes: game.black_holes.iter().map(|h| NetBlackHole {
            x: h.pos.x, y: h.pos.y, ticks_left: h.lifetime,
        }).collect(),
        fire_patches: game.fire_patches.iter().map(|f| NetFirePatch {
            x: f.pos.x, y: f.pos.y, lifetime: f.lifetime,
            landed: f.landed, vel_x: f.vel.x, vel_y: f.vel.y,
        }).collect(),
        rope: game.rope.as_ref().map(|r| NetRope {
            anchor_x: r.anchor.x, anchor_y: r.anchor.y,
            hook_x: r.hook.x, hook_y: r.hook.y,
            flying: r.flying, length: r.length,
        }),
        opp_team_name: {
            let opp = 1 - game.active_team();
            game.teams.get(opp).map(|t| t.name.clone()).unwrap_or_default()
        },
        garcia: game.garcia.as_ref().map(|g| NetGarcia {
            cursor_x: g.cursor_x, render_x: g.render_x, cursor_y: g.cursor_y, render_y: g.render_y,
            blink_timer: g.blink_timer,
            falling: g.falling, fall_y: g.fall_y, vel_y: g.vel_y, bounce_count: g.bounce_count,
        }),
        torch_dir: {
            use arty::game::state::TorchDir;
            match game.plasma_torch.as_ref().map(|t| t.dir) {
                Some(TorchDir::UpForward)   => 1,
                Some(TorchDir::Forward)     => 2,
                Some(TorchDir::DownForward) => 3,
                None                        => 0,
            }
        },
        paused_opponent: None,
        opponent_abandoned: false,
    }
}

fn read_loop(mut s: TcpStream, inbox: Arc<Mutex<Option<InputMsg>>>) {
    loop {
        let mut hdr = [0u8; 4];
        if s.read_exact(&mut hdr).is_err() { break; }
        let len = decode_len(&hdr);
        if len > 65536 { break; }
        let mut buf = vec![0u8; len];
        if s.read_exact(&mut buf).is_err() { break; }
        if let Ok(msg) = bincode::deserialize::<InputMsg>(&buf) {
            *inbox.lock().unwrap_or_else(|e| e.into_inner()) = Some(msg);
        }
    }
    info!("read_loop: client connection closed");
}

fn send_msg<T: serde::Serialize>(mut s: &TcpStream, msg: &T) {
    if let Some(bytes) = encode(msg) {
        let _ = s.write_all(&bytes);
    }
}

fn snap_to_surface(game: &mut GameState, ti: usize, si: usize) {
    let x = game.teams[ti].soldiers[si].pos.x as i32;
    let y = game.teams[ti].soldiers[si].pos.y as i32;
    for dy in 0i32..=20 {
        let fy = y + dy;
        if fy >= arty::world::WORLD_H as i32 { break; }
        if game.terrain.is_solid(x, fy) {
            game.teams[ti].soldiers[si].pos.y = (fy - 1).max(0) as f32;
            return;
        }
    }
}

fn is_on_ground(game: &GameState, ti: usize, si: usize) -> bool {
    let s = &game.teams[ti].soldiers[si];
    let x = s.pos.x as i32;
    let y = s.pos.y as i32;
    // Probe a small horizontal window, not just the single foot column: on a steep
    // slope or ledge edge the exact column under the foot can be air within 3px even
    // while the soldier is clearly standing, which silently blocked jump/backflip.
    (-1..=1).any(|dx| {
        game.terrain.is_solid(x + dx, y + 1)
            || game.terrain.is_solid(x + dx, y + 2)
            || game.terrain.is_solid(x + dx, y + 3)
    })
}

const MAGIC: &[u8; 4] = b"MMAY";

const REQUIRED_VERSION: &str = "0.5.4.193";

/// Read up to `max` bytes until (and excluding) a `\n`, returning the trimmed string.
/// Returns None on read error.
fn read_line(stream: &mut TcpStream, max: usize) -> Option<String> {
    let mut s = String::new();
    for _ in 0..max {
        let mut b = [0u8; 1];
        if stream.read_exact(&mut b).is_err() { return None; }
        if b[0] == b'\n' { break; }
        s.push(b[0] as char);
    }
    Some(s.trim().to_string())
}

/// Accept a single client connection, perform the MAGIC + version + session-token
/// handshake, and return the live stream, peer address, and session token
/// (empty string for non-reconnectable connections, e.g. casual play).
/// True unless the peer has closed the connection (a 1-byte `peek` returns
/// `Ok(0)` on EOF). A live connection with nothing to read returns
/// `WouldBlock`/`TimedOut`, which we also treat as alive.
fn stream_alive(s: &TcpStream) -> bool {
    let mut b = [0u8; 1];
    s.set_read_timeout(Some(Duration::from_millis(1))).ok();
    let r = s.peek(&mut b);
    s.set_read_timeout(None).ok();
    match r {
        Ok(0) => false,
        Ok(_) => true,
        Err(e) => e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut,
    }
}

fn accept_one(listener: &TcpListener) -> (TcpStream, std::net::SocketAddr, String) {
    loop {
        let (mut stream, addr) = match listener.accept() {
            Ok(pair) => pair,
            Err(e) => { info!("accept error: {e}"); continue; }
        };
        let mut buf = [0u8; 4];
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        match stream.read_exact(&mut buf) {
            Ok(_) if &buf == MAGIC => {
                let ver = match read_line(&mut stream, 16) {
                    Some(v) => v,
                    None => { info!("Handshake read failed: {}", addr); continue; }
                };
                if ver != REQUIRED_VERSION {
                    info!("Rejected wrong version {}: {}", ver, addr);
                    continue;
                }
                let token = match read_line(&mut stream, 32) {
                    Some(t) => t,
                    None => { info!("Handshake read failed: {}", addr); continue; }
                };
                stream.set_read_timeout(None).ok();
                info!("Player (v{}): {}", ver, addr);
                return (stream, addr, token);
            }
            _ => {
                info!("Rejected: {}", addr);
            }
        }
    }
}

fn msg_to_input(msg: &InputMsg) -> arty::input::InputState {
    use arty::input::{InputState, Button};
    let mut state = InputState::new();
    let map = |n: &NetButton| -> Option<Button> {
        match n {
            NetButton::Up    => Some(Button::Up),
            NetButton::Down  => Some(Button::Down),
            NetButton::Left  => Some(Button::Left),
            NetButton::Right => Some(Button::Right),
            NetButton::A     => Some(Button::A),
            NetButton::B     => Some(Button::B),
            NetButton::X     => Some(Button::X),
            NetButton::Y     => Some(Button::Y),
            NetButton::L1    => Some(Button::L1),
            NetButton::R1    => Some(Button::R1),
            NetButton::L2    => Some(Button::L2),
            NetButton::R2    => Some(Button::R2),
            NetButton::Start  => Some(Button::Start),
            NetButton::Select => Some(Button::Select),
        }
    };
    for b in &msg.held    { if let Some(btn) = map(b) { state.inject_held(btn); } }
    for b in &msg.pressed { if let Some(btn) = map(b) { state.inject_press(btn); } }
    for b in &msg.released{ if let Some(btn) = map(b) { state.inject_release(btn); } }
    state
}
