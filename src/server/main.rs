use std::collections::HashMap;
use std::io::{self, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rustls::ServerConnection;
use rustls_pemfile::{certs, private_key};

type ServerStream = rustls::StreamOwned<ServerConnection, TcpStream>;
type ArcStream    = Arc<Mutex<ServerStream>>;

/// Connection info for one player in a paused casual match — used for reconnect.
struct CasualSlot {
    write:      ArcStream,
    input:      Arc<Mutex<Option<InputMsg>>>,
    disc:       Arc<AtomicBool>,
    quit:       Arc<AtomicBool>,
    gen:        Arc<AtomicU64>,
    team:       usize,
    seed:       u64,
    team_count: usize,
    color:      u8,
}
type CasualRegistry = Arc<Mutex<HashMap<String, Arc<CasualSlot>>>>;

use arty::net::{msg::*, encode};
use arty::game::net_sync::build_state;
use log::info;
use chrono::Local;

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
/// Token sent by clients who want to enter the ranked queue (not a reconnect).
const RANKED_QUEUE_TOKEN: &str = "RANKED";

type RankedQueue = Arc<Mutex<Vec<ArcStream>>>;

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
        .format(|buf, record| {
            use std::io::Write;
            writeln!(buf, "[{}  {}  {}] {}",
                Local::now().format("%Y-%m-%d %H:%M:%S %Z"),
                record.level(),
                record.target(),
                record.args())
        })
        .init();
    // ARTY_PORT env var lets the API spawn instances on different ports
    let port: u16 = std::env::var("ARTY_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(PORT_DEFAULT);
    info!("Miyoo Mayhem server on :{}", port);
    let tls_config = load_tls_config();
    let listener = TcpListener::bind(("0.0.0.0", port)).expect("bind failed");
    let match_id: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
    let registry: Registry = Arc::new(Mutex::new(HashMap::new()));
    let casual_registry: CasualRegistry = Arc::new(Mutex::new(HashMap::new()));
    let ranked_queue: RankedQueue = Arc::new(Mutex::new(Vec::new()));
    let lobby: SharedLobby = Arc::new(Mutex::new(Lobby::default()));
    loop {
        let (stream, _addr, token) = accept_one(&listener, &tls_config);

        // Casual play (empty session token) goes into the shared lobby, where
        // up to 4 players ready up before the match starts.
        if token.is_empty() {
            let lobby2 = lobby.clone();
            let mid = match_id.clone();
            let cr2 = casual_registry.clone();
            thread::spawn(move || casual_conn(stream, lobby2, mid, cr2));
            continue;
        }

        // Reconnect: check ranked registry, then casual registry.
        {
            let slot = registry.lock().unwrap_or_else(|e| e.into_inner()).get(&token).cloned();
            if let Some(slot) = slot {
                if reconnect_into(&slot, &stream) { continue; }
            }
        }
        {
            let cs = casual_registry.lock().unwrap_or_else(|e| e.into_inner()).get(&token).cloned();
            if let Some(cs) = cs {
                if casual_reconnect_into(&cs, &stream) { continue; }
            }
        }

        // Ranked queue: a player connecting with RANKED_QUEUE_TOKEN wants a
        // ranked match. Drain dead waiters first, then pair if someone else is
        // already waiting, otherwise add this player to the queue.
        if token == RANKED_QUEUE_TOKEN {
            let username = read_line(&mut *stream.lock().unwrap_or_else(|e| e.into_inner()), 64)
                .unwrap_or_else(|| "?".to_string());
            // Drain dead waiters, then either pair immediately or enqueue.
            let mut q = ranked_queue.lock().unwrap_or_else(|e| e.into_inner());
            q.retain(|s| stream_alive(s));
            if q.is_empty() {
                q.push(stream);
                info!("Ranked: {} queued ({} waiting)", username, q.len());
            } else {
                let s0 = q.remove(0);
                drop(q); // release lock before spawning
                let mid = match_id.fetch_add(1, Ordering::Relaxed) + 1;
                let registry2 = registry.clone();
                info!("Ranked: pairing {} into match {mid}", username);
                thread::spawn(move || run_ranked_match(mid, s0, stream, registry2));
            }
            continue;
        }

        info!("Unknown token — ignoring connection");
    }
}

struct SharedConn {
    stream: ArcStream,
    inbox:  Arc<Mutex<Option<InputMsg>>>,
    disc:   Arc<AtomicBool>,
    quit:   Arc<AtomicBool>,
    gen:    Arc<AtomicU64>,
}

struct MatchSlot {
    conns: [Mutex<SharedConn>; 2],
    seed:  u64,
}

type Registry = Arc<Mutex<HashMap<String, Arc<MatchSlot>>>>;

/// Attempt to swap `stream` into whichever team slot of `slot` is currently
/// disconnected. Returns true on success (a fresh read thread was spawned).
fn reconnect_into(slot: &Arc<MatchSlot>, stream: &ArcStream) -> bool {
    for team in 0..2 {
        let mut sc = slot.conns[team].lock().unwrap_or_else(|e| e.into_inner());
        if sc.disc.load(Ordering::Relaxed) {
            sc.stream = Arc::clone(stream);
            let new_gen = sc.gen.fetch_add(1, Ordering::Relaxed) + 1;
            sc.disc.store(false, Ordering::Relaxed);
            *sc.inbox.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let inbox = sc.inbox.clone();
            let disc = sc.disc.clone();
            let quit = sc.quit.clone();
            let gen = sc.gen.clone();
            let read_arc = Arc::clone(stream);
            let welcome_arc = Arc::clone(stream);
            drop(sc);
            if let Some(bytes) = encode(&WelcomeMsg { your_team: team, seed: slot.seed, team_count: 2, your_color: team as u8, reconnect_token: String::new() }) {
                write_arc(&welcome_arc, &bytes);
            }
            thread::spawn(move || {
                read_loop(read_arc, inbox, quit);
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

/// Reconnect a player into an in-progress casual match. Returns true on success.
fn casual_reconnect_into(slot: &Arc<CasualSlot>, stream: &ArcStream) -> bool {
    if !slot.disc.load(Ordering::Relaxed) { return false; }
    // CasualSlot.write is an ArcStream; swap it to the new connection by replacing
    // the inner ServerStream. We lock both arcs to do the swap atomically.
    {
        let mut old = slot.write.lock().unwrap_or_else(|e| e.into_inner());
        let new_inner = {
            // We can't move out of the new arc without consuming it, so we need
            // a different approach: just replace slot.write with the new arc entirely.
            // Since CasualSlot is behind Arc<CasualSlot> (immutable), we can't
            // reassign slot.write. Instead, use the existing arc and swap contents.
            // We move the inner stream from stream into old.
            // This requires unsafe or a different approach.
            // Simplest: we can't swap immutably. Use a second mutex level.
            // Actually: CasualSlot.write is ArcStream = Arc<Mutex<ServerStream>>.
            // We can't change WHICH arc slot.write points to (immutable field on Arc).
            // Instead, we swap the inner ServerStream between the two arcs.
            let mut new_guard = stream.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::swap(&mut *old, &mut *new_guard);
            // Now slot.write's mutex contains the new connection's stream.
            // The incoming stream arc now holds the old (dead) stream.
        };
        drop(old);
        let _ = new_inner;
    }
    let new_gen = slot.gen.fetch_add(1, Ordering::Relaxed) + 1;
    slot.disc.store(false, Ordering::Relaxed);
    *slot.input.lock().unwrap_or_else(|e| e.into_inner()) = None;
    if let Some(bytes) = encode(&WelcomeMsg {
        your_team: slot.team, seed: slot.seed, team_count: slot.team_count,
        your_color: slot.color, reconnect_token: String::new(),
    }) {
        write_arc(&slot.write, &bytes);
    }
    let inbox = slot.input.clone();
    let disc = slot.disc.clone();
    let quit = slot.quit.clone();
    let gen = slot.gen.clone();
    let read_arc = Arc::clone(&slot.write);
    thread::spawn(move || {
        read_loop(read_arc, inbox, quit);
        if gen.load(Ordering::Relaxed) == new_gen {
            disc.store(true, Ordering::Relaxed);
        }
    });
    info!("Casual reconnected team {}", slot.team);
    true
}

/// Generate a short unique token for a casual match slot.
fn gen_casual_token(match_id: u64, team: usize) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
        .subsec_nanos();
    format!("c{:016x}{:07x}{}", match_id, nanos, team)
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

/// Ranked match entry point — generates reconnect tokens and delegates to run_match.
fn run_ranked_match(match_id: u64, s0: ArcStream, s1: ArcStream, registry: Registry) {
    let token = format!("r{:016x}{:016x}", match_id,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64);
    run_match(match_id, s0, s1, registry, token);
}

fn run_match(match_id: u64, s0: ArcStream, s1: ArcStream, registry: Registry, session_token: String) {
    let mut mfile = match_log_file(match_id);
    mboth!(&mut mfile, match_id, "starting");
    mboth!(&mut mfile, match_id, "Both connected - starting!");
    thread::sleep(Duration::from_secs(2));

    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

    send_msg(&s0, &WelcomeMsg { your_team: 0, seed, team_count: 2, your_color: 0, reconnect_token: session_token.clone() });
    send_msg(&s1, &WelcomeMsg { your_team: 1, seed, team_count: 2, your_color: 1, reconnect_token: session_token.clone() });

    let inp0: Arc<Mutex<Option<InputMsg>>> = Arc::new(Mutex::new(None));
    let inp1: Arc<Mutex<Option<InputMsg>>> = Arc::new(Mutex::new(None));

    let disc0 = Arc::new(AtomicBool::new(false));
    let disc1 = Arc::new(AtomicBool::new(false));
    let gen0 = Arc::new(AtomicU64::new(0));
    let gen1 = Arc::new(AtomicU64::new(0));
    let quit0 = Arc::new(AtomicBool::new(false));
    let quit1 = Arc::new(AtomicBool::new(false));

    let read_arc0 = Arc::clone(&s0);
    let read_arc1 = Arc::clone(&s1);
    thread::spawn({
        let i = inp0.clone(); let d = disc0.clone(); let g = gen0.clone(); let q = quit0.clone();
        move || { read_loop(read_arc0, i, q); if g.load(Ordering::Relaxed) == 0 { d.store(true, Ordering::Relaxed); } }
    });
    thread::spawn({
        let i = inp1.clone(); let d = disc1.clone(); let g = gen1.clone(); let q = quit1.clone();
        move || { read_loop(read_arc1, i, q); if g.load(Ordering::Relaxed) == 0 { d.store(true, Ordering::Relaxed); } }
    });

    let match_slot = Arc::new(MatchSlot { conns: [
        Mutex::new(SharedConn { stream: s0, inbox: inp0.clone(), disc: disc0.clone(), quit: quit0.clone(), gen: gen0.clone() }),
        Mutex::new(SharedConn { stream: s1, inbox: inp1.clone(), disc: disc1.clone(), quit: quit1.clone(), gen: gen1.clone() }),
    ], seed });
    let reconnectable = !session_token.is_empty();
    if reconnectable {
        registry.lock().unwrap_or_else(|e| e.into_inner()).insert(session_token.clone(), match_slot.clone());
    }
    macro_rules! write_team {
        ($team:expr, $bytes:expr) => {{
            let sc = match_slot.conns[$team].lock().unwrap_or_else(|e| e.into_inner());
            write_arc(&sc.stream, $bytes);
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

        // Voluntary forfeit — client sent InputMsg { quit: true }.
        // Award the win immediately; skip the reconnect window entirely.
        if quit0.load(Ordering::Relaxed) || quit1.load(Ordering::Relaxed) {
            let qteam = if quit0.load(Ordering::Relaxed) { 0 } else { 1 };
            let winner = 1 - qteam;
            let name = game.teams[qteam].soldiers.get(0).map(|s| s.name.as_str()).unwrap_or("?");
            mboth!(&mut mfile, match_id, "team {qteam} ({name}) forfeited — team {winner} wins");
            let mut state = build_state(&game, tick, last_craters_sent);
            state.result = NetResult::Winner(winner);
            if let Some(bytes) = encode(&state) {
                write_team!(winner, &bytes);
                let sc = match_slot.conns[winner].lock().unwrap_or_else(|e| e.into_inner());
                flush_arc(&sc.stream);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            break;
        }
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
                            t.soldiers[si].name = sanitize_name(&msg.worm_names[si]);
                        }
                    }
                }
            }
        }
        let active = game.active_team();
        let inp = if active == 0 { i0 } else { i1 };
        let mut input_state = inp.as_ref().map(msg_to_input).unwrap_or_else(arty::input::InputState::new);

        // Pass the client's aim_angle into server_tick so process_aim applies it
        // directly and skips button-driven adjustment. Up/Down are NOT stripped
        // here — they flow through to cursor-phase weapons (homing missile,
        // airstrike, any future weapon) without special-casing.
        let muzzle_override: Option<(f32, f32)>;
        let aim_angle_override: Option<f32>;
        if let Some(ref msg) = inp {
            aim_angle_override = if msg.aim_angle.is_finite() {
                Some(msg.aim_angle)
            } else {
                None
            };
            use arty::physics::projectile::WeaponKind;
            let kind = WeaponKind::from_net_u8(msg.selected_weapon_kind);
            let ti = game.active_team();
            if let Some(idx) = game.teams[ti].weapons.iter().position(|(w, _)| *w == kind) {
                game.teams[ti].selected_weapon = idx;
            }
            muzzle_override = if msg.muzzle_x != 0.0 || msg.muzzle_y != 0.0 {
                Some((msg.muzzle_x, msg.muzzle_y))
            } else {
                None
            };
        } else {
            aim_angle_override = None;
            muzzle_override = None;
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

        let msgs_before: std::collections::HashSet<String> =
            game.messages.iter().map(|m| m.text.clone()).collect();
        arty::game::loop_runner::server_tick(&mut game, &input_state, muzzle_override, aim_angle_override);

        // Log in-game messages (death phrases, crate pickups, etc.)
        for m in &game.messages {
            if !msgs_before.contains(&m.text) {
                mboth!(&mut mfile, match_id, "MSG: {}", m.text);
            }
        }

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

        // Log in-game messages (death phrases, crate pickups, etc.)
        for m in &game.messages {
            if m.ticks == 119 { // first tick the message is live (set to 120, decremented once)
                mboth!(&mut mfile, match_id, "MSG: {}", m.text);
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
            if let Some(bytes) = encode(&WelcomeMsg { your_team: 0, seed, team_count: 2, your_color: 0, reconnect_token: String::new() }) { write_team!(0, &bytes); }
            if let Some(bytes) = encode(&WelcomeMsg { your_team: 1, seed, team_count: 2, your_color: 1, reconnect_token: String::new() }) { write_team!(1, &bytes); }
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

// ── Casual lobby ────────────────────────────────────────────────────────────

/// A player waiting in (or playing from) the casual lobby. The Arc handles are
/// shared with the per-connection reader thread, which keeps reading from the
/// socket across the lobby→game transition (decoding LobbyClientMsg before the
/// match starts, InputMsg after).
struct LobbyMember {
    id:       u64,
    write:    ArcStream,
    input:    Arc<Mutex<Option<InputMsg>>>,
    disc:     Arc<AtomicBool>,
    quit:     Arc<AtomicBool>,
    gen:      Arc<AtomicU64>,
    started:  Arc<AtomicBool>,
    join:     Option<LobbyJoin>,
    color_id: Option<u8>,
    ready:    bool,
}

#[derive(Default)]
struct Lobby {
    members: Vec<LobbyMember>,
    next_id: u64,
}
type SharedLobby = Arc<Mutex<Lobby>>;

/// Read one length-prefixed frame (blocking). None on IO error/EOF/oversize.
fn read_frame(s: &mut TcpStream) -> Option<Vec<u8>> {
    let mut hdr = [0u8; 4];
    s.read_exact(&mut hdr).ok()?;
    let len = decode_len(&hdr);
    if len == 0 || len > 65536 { return None; }
    let mut buf = vec![0u8; len];
    s.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Broadcast the current lobby roster to everyone in it.
fn broadcast_lobby(lobby: &SharedLobby) {
    let lb = lobby.lock().unwrap_or_else(|e| e.into_inner());
    let players: Vec<LobbyPlayer> = lb.members.iter().map(|m| LobbyPlayer {
        name:      m.join.as_ref().map(|j| j.name.clone()).unwrap_or_default(),
        username:  m.join.as_ref().map(|j| j.username.clone()).unwrap_or_default(),
        avatar_id: m.join.as_ref().map(|j| j.avatar_id).unwrap_or(0),
        color_id:  m.color_id,
        ready:     m.ready,
    }).collect();
    for (i, m) in lb.members.iter().enumerate() {
        if let Some(bytes) = encode(&LobbyServerMsg::State { players: players.clone(), your_index: i }) {
            write_arc(&m.write, &bytes);
        }
    }
}

/// Per-connection handler for casual play: registers the player in the lobby,
/// relays lobby messages, and once the match starts keeps feeding InputMsg.
fn casual_conn(stream: ArcStream, lobby: SharedLobby, match_id: Arc<AtomicU64>, casual_registry: CasualRegistry) {
    let write   = stream;
    let input: Arc<Mutex<Option<InputMsg>>> = Arc::new(Mutex::new(None));
    let disc    = Arc::new(AtomicBool::new(false));
    let quit    = Arc::new(AtomicBool::new(false));
    let gen     = Arc::new(AtomicU64::new(0));
    let started = Arc::new(AtomicBool::new(false));
    let read_stream = Arc::clone(&write);

    let my_id = {
        let mut lb = lobby.lock().unwrap_or_else(|e| e.into_inner());
        let id = lb.next_id; lb.next_id += 1; id
    };
    let mut casual_read_buf: Vec<u8> = Vec::new();

    loop {
        let buf = match read_one_arc(&read_stream, &mut casual_read_buf) { Some(b) => b, None => break };
        if started.load(Ordering::Relaxed) {
            if let Ok(inp) = bincode::deserialize::<InputMsg>(&buf) {
                if inp.quit { quit.store(true, Ordering::Relaxed); }
                let mut guard = input.lock().unwrap_or_else(|e| e.into_inner());
                match guard.as_mut() {
                    Some(prev) => {
                        for b in &inp.pressed  { if !prev.pressed.contains(b)  { prev.pressed.push(*b);  } }
                        for b in &inp.released { if !prev.released.contains(b) { prev.released.push(*b); } }
                        prev.held              = inp.held;
                        prev.aim_angle         = inp.aim_angle;
                        prev.selected_weapon_kind = inp.selected_weapon_kind;
                        prev.hat_ids           = inp.hat_ids;
                        prev.uniform_color_ids = inp.uniform_color_ids;
                        prev.boot_color_ids    = inp.boot_color_ids;
                        prev.gun_style_ids     = inp.gun_style_ids;
                        prev.worm_names        = inp.worm_names;
                        prev.muzzle_x          = inp.muzzle_x;
                        prev.muzzle_y          = inp.muzzle_y;
                        prev.tick              = inp.tick;
                    }
                    None => *guard = Some(inp),
                }
            }
        } else if let Ok(m) = bincode::deserialize::<LobbyClientMsg>(&buf) {
            handle_lobby_msg(&lobby, &match_id, my_id, &write, &input, &disc, &quit, &gen, &started, casual_registry.clone(), m);
        }
    }
    // Only mark disconnected if gen=0 (no reconnect has happened on this slot).
    if gen.load(Ordering::Relaxed) == 0 {
        disc.store(true, Ordering::Relaxed);
    }
    let removed = {
        let mut lb = lobby.lock().unwrap_or_else(|e| e.into_inner());
        let before = lb.members.len();
        lb.members.retain(|m| m.id != my_id);
        before != lb.members.len()
    };
    if removed { broadcast_lobby(&lobby); }
}

#[allow(clippy::too_many_arguments)]
fn handle_lobby_msg(
    lobby:           &SharedLobby,
    match_id:        &Arc<AtomicU64>,
    my_id:           u64,
    write:           &ArcStream,
    input:           &Arc<Mutex<Option<InputMsg>>>,
    disc:            &Arc<AtomicBool>,
    quit:            &Arc<AtomicBool>,
    gen:             &Arc<AtomicU64>,
    started:         &Arc<AtomicBool>,
    casual_registry: CasualRegistry,
    msg:             LobbyClientMsg,
) {
    let mut start_members: Option<Vec<LobbyMember>> = None;
    {
        let mut lb = lobby.lock().unwrap_or_else(|e| e.into_inner());
        match msg {
            LobbyClientMsg::Join(j) => {
                if !lb.members.iter().any(|m| m.id == my_id) && lb.members.len() < 4 {
                    info!("Lobby join: {} ({})", j.username, j.name);
                    lb.members.push(LobbyMember {
                        id: my_id, write: write.clone(), input: input.clone(),
                        disc: disc.clone(), quit: quit.clone(), gen: gen.clone(),
                        started: started.clone(), join: Some(j), color_id: None, ready: false,
                    });
                }
            }
            LobbyClientMsg::PickColor { color_id } => {
                let c = color_id.min(3);
                let taken = lb.members.iter().any(|m| m.id != my_id && m.color_id == Some(c));
                if !taken {
                    if let Some(m) = lb.members.iter_mut().find(|m| m.id == my_id) { m.color_id = Some(c); }
                }
            }
            LobbyClientMsg::SetReady { ready } => {
                if let Some(m) = lb.members.iter_mut().find(|m| m.id == my_id) {
                    // Readying requires a chosen colour.
                    if !(ready && m.color_id.is_none()) { m.ready = ready; }
                }
            }
            LobbyClientMsg::Leave => { lb.members.retain(|m| m.id != my_id); }
        }
        // Start when >=2 players present and all are ready with a colour.
        if lb.members.len() >= 2 && lb.members.iter().all(|m| m.ready && m.color_id.is_some()) {
            start_members = Some(std::mem::take(&mut lb.members));
        }
    }
    if let Some(members) = start_members {
        for m in &members { m.started.store(true, Ordering::Relaxed); }
        let mid = match_id.fetch_add(1, Ordering::Relaxed) + 1;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        thread::spawn(move || run_lobby_match(mid, members, seed, casual_registry));
    } else {
        broadcast_lobby(lobby);
    }
}

/// Run a casual N-player match (2-4 teams). 2-player matches support reconnect;
/// on game over the clients return to the title screen.
fn run_lobby_match(match_id: u64, members: Vec<LobbyMember>, seed: u64, casual_registry: CasualRegistry) {
    let mut mfile = match_log_file(match_id);
    let n = members.len();
    let colors: Vec<u8> = members.iter().map(|m| m.color_id.unwrap_or(0)).collect();
    mboth!(&mut mfile, match_id, "casual lobby match starting with {n} players");

    // For 2-player matches: generate reconnect tokens and register slots.
    let tokens: Vec<String> = if n == 2 {
        (0..n).map(|i| gen_casual_token(match_id, i)).collect()
    } else {
        vec![String::new(); n]
    };
    if n == 2 {
        let mut cr = casual_registry.lock().unwrap_or_else(|e| e.into_inner());
        for (i, m) in members.iter().enumerate() {
            cr.insert(tokens[i].clone(), Arc::new(CasualSlot {
                write: m.write.clone(), input: m.input.clone(),
                disc:  m.disc.clone(),  quit:  m.quit.clone(), gen: m.gen.clone(),
                team: i, seed, team_count: n, color: colors[i],
            }));
        }
    }

    for (i, m) in members.iter().enumerate() {
        let w = WelcomeMsg { your_team: i, seed, team_count: n, your_color: colors[i], reconnect_token: tokens[i].clone() };
        if let Some(bytes) = encode(&LobbyServerMsg::Start(w)) {
            write_arc(&m.write, &bytes);
        }
    }
    thread::sleep(Duration::from_millis(500));

    let mut game = build_game_n(seed, &colors);
    // Apply each player's roster to their team.
    for (i, m) in members.iter().enumerate() {
        if let (Some(j), Some(team)) = (m.join.as_ref(), game.teams.get_mut(i)) {
            team.name         = sanitize_name(&j.name);
            team.avatar_id    = j.avatar_id;
            team.headstone_id = j.headstone_id;
            for si in 0..team.soldiers.len().min(4) {
                if !j.worm_names[si].is_empty() { team.soldiers[si].name = sanitize_name(&j.worm_names[si]); }
                team.soldiers[si].hat_id           = j.hat_ids[si];
                team.soldiers[si].uniform_color_id = j.uniform_color_ids[si];
                team.soldiers[si].boot_color_id    = j.boot_color_ids[si];
                team.soldiers[si].gun_style_id     = j.gun_style_ids[si];
            }
        }
    }

    let mut tick: u32 = 0;
    let mut eliminated = vec![false; n];
    let mut paused: Option<(usize, Instant)> = None; // (disconnected_team, since) — 2-player only
    macro_rules! write_all_conns {
        ($bytes:expr) => {{
            for m in &members {
                if m.disc.load(Ordering::Relaxed) { continue; }
                write_arc(&m.write, $bytes);
            }
        }};
    }

    loop {
        let t = Instant::now();

        // Pause/resume handling — 2-player casual only.
        if n == 2 {
            if let Some((dteam, since)) = paused {
                let still_disc = members[dteam].disc.load(Ordering::Relaxed);
                if !still_disc {
                    mboth!(&mut mfile, match_id, "casual team {dteam} reconnected — resuming");
                    // Send full state so the returning player catches up.
                    if let Some(state_bytes) = encode(&build_state(&game, tick, 0)) {
                        write_arc(&members[dteam].write, &state_bytes);
                    }
                    paused = None;
                } else if since.elapsed() >= RECONNECT_TIMEOUT {
                    mboth!(&mut mfile, match_id, "casual team {dteam} did not reconnect — ending");
                    let connected = 1 - dteam;
                    let mut state = build_state(&game, tick, 0);
                    state.opponent_abandoned = true;
                    state.result = NetResult::Winner(connected);
                    if let Some(bytes) = encode(&state) {
                        write_arc(&members[connected].write, &bytes);
                    }
                    break;
                } else {
                    // Still waiting — send countdown to the connected player.
                    let connected = 1 - dteam;
                    let mut state = build_state(&game, tick, 0);
                    state.paused_opponent = Some((RECONNECT_TIMEOUT - since.elapsed()).as_secs() as u32);
                    if let Some(bytes) = encode(&state) {
                        write_arc(&members[connected].write, &bytes);
                    }
                    let e = t.elapsed();
                    if e < TICK_DURATION { thread::sleep(TICK_DURATION - e); }
                    continue;
                }
            }
        }

        tick = tick.wrapping_add(1);
        game.tick = tick;

        if members.iter().all(|m| m.disc.load(Ordering::Relaxed)) {
            mboth!(&mut mfile, match_id, "all players left — ending");
            break;
        }

        // Voluntary forfeit — player sent InputMsg { quit: true }.
        // For 2-player: award the win immediately, no reconnect window.
        // For N>2: eliminate immediately (same as disconnect but no reconnect).
        for (i, m) in members.iter().enumerate() {
            if m.quit.load(Ordering::Relaxed) && !eliminated[i] {
                if n == 2 {
                    let winner = 1 - i;
                    let name = game.teams[i].soldiers.get(0).map(|s| s.name.as_str()).unwrap_or("?");
                    mboth!(&mut mfile, match_id, "casual team {i} ({name}) forfeited — team {winner} wins");
                    let mut state = build_state(&game, tick, 0);
                    state.result = NetResult::Winner(winner);
                    if let Some(bytes) = encode(&state) {
                        write_arc(&members[winner].write, &bytes);
                        flush_arc(&members[winner].write);
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    return;
                } else {
                    eliminated[i] = true;
                    if let Some(team) = game.teams.get_mut(i) {
                        for s in &mut team.soldiers { s.take_damage(100); }
                    }
                    mboth!(&mut mfile, match_id, "casual team {i} forfeited — eliminated");
                }
            }
        }

        // Handle disconnects: pause for 2-player, eliminate immediately for N>2.
        for (i, m) in members.iter().enumerate() {
            if m.disc.load(Ordering::Relaxed) && !eliminated[i] {
                if n == 2 && paused.is_none() {
                    mboth!(&mut mfile, match_id, "casual team {i} disconnected — pausing for reconnect");
                    paused = Some((i, Instant::now()));
                } else if n != 2 {
                    eliminated[i] = true;
                    if let Some(team) = game.teams.get_mut(i) {
                        for s in &mut team.soldiers { s.take_damage(100); }
                    }
                    mboth!(&mut mfile, match_id, "team {i} left — eliminated");
                }
            }
        }

        // Skip simulation while paused (2-player).
        if paused.is_some() { continue; }

        // Apply cosmetics/names from every player each tick.
        let inputs: Vec<Option<InputMsg>> = members.iter()
            .map(|m| m.input.lock().unwrap_or_else(|e| e.into_inner()).clone()).collect();
        for (i, inp) in inputs.iter().enumerate() {
            if let (Some(msg), Some(team)) = (inp, game.teams.get_mut(i)) {
                for si in 0..team.soldiers.len().min(4) {
                    team.soldiers[si].hat_id           = msg.hat_ids[si];
                    team.soldiers[si].uniform_color_id = msg.uniform_color_ids[si];
                    team.soldiers[si].boot_color_id    = msg.boot_color_ids[si];
                    team.soldiers[si].gun_style_id     = msg.gun_style_ids[si];
                    if !msg.worm_names[si].is_empty() { team.soldiers[si].name = sanitize_name(&msg.worm_names[si]); }
                }
            }
        }

        let active = game.active_team();
        let inp = inputs.get(active).cloned().flatten();
        let mut input_state = inp.as_ref().map(msg_to_input).unwrap_or_else(arty::input::InputState::new);
        let muzzle_override2: Option<(f32, f32)>;
        let aim_angle_override2: Option<f32>;
        if let Some(ref msg) = inp {
            aim_angle_override2 = if msg.aim_angle.is_finite() { Some(msg.aim_angle) } else { None };
            use arty::physics::projectile::WeaponKind;
            let kind = WeaponKind::from_net_u8(msg.selected_weapon_kind);
            if let Some(idx) = game.teams[active].weapons.iter().position(|(w, _)| *w == kind) {
                game.teams[active].selected_weapon = idx;
            }
            muzzle_override2 = if msg.muzzle_x != 0.0 || msg.muzzle_y != 0.0 {
                Some((msg.muzzle_x, msg.muzzle_y))
            } else {
                None
            };
        } else {
            aim_angle_override2 = None;
            muzzle_override2 = None;
        }
        if let Some(m) = members.get(active) {
            if let Some(ref mut i) = *m.input.lock().unwrap_or_else(|e| e.into_inner()) { i.pressed.clear(); i.released.clear(); }
        }

        arty::game::loop_runner::server_tick(&mut game, &input_state, muzzle_override2, aim_angle_override2);

        if !matches!(game.result, arty::game::state::GameResult::Ongoing) {
            if let Some(final_bytes) = encode(&build_state(&game, tick, 0)) {
                for _ in 0..90 {
                    if members.iter().all(|m| m.disc.load(Ordering::Relaxed)) { break; }
                    write_all_conns!(&final_bytes);
                    thread::sleep(TICK_DURATION);
                }
            }
            break; // casual: no rematch
        }

        if let Some(state_bytes) = encode(&build_state(&game, tick, 0)) {
            write_all_conns!(&state_bytes);
        }
        let e = t.elapsed();
        if e < TICK_DURATION { thread::sleep(TICK_DURATION - e); }
    }
    // Clean up casual reconnect tokens.
    if n == 2 {
        let mut cr = casual_registry.lock().unwrap_or_else(|e| e.into_inner());
        for tok in &tokens { cr.remove(tok); }
    }
    mboth!(&mut mfile, match_id, "casual lobby match ended");
}

fn build_game(seed: u64) -> GameState {
    // 2-team game with default colours (Red=0, Blue=1) — used by ranked.
    build_game_n(seed, &[0, 1])
}

/// Build an N-team game (N = colors.len(), 2-4). The map is divided into N
/// vertical bands so each team spawns in its own region; `colors[i]` sets team
/// i's colour identity. The 2-team case reproduces the original left/right split
/// exactly so ranked/local maps are unchanged.
fn build_game_n(seed: u64, colors: &[u8]) -> GameState {
    let mut terrain = arty::world::Terrain::generate_tactical(seed);
    let n = colors.len().clamp(2, 4);
    let all_spawns = terrain.find_team_spawns(0, WORLD_W, n * 4);
    let mut teams = Vec::with_capacity(n);
    for i in 0..n {
        let spawns: Vec<_> = all_spawns.iter().cloned()
            .enumerate().filter(|(k, _)| k % n == i).map(|(_, s)| s).collect();
        let mut team = Team::new(i, false, Difficulty::Medium, &spawns);
        team.set_color(colors[i]);
        teams.push(team);
    }
    let mut game = GameState::new(seed, terrain, teams, n);
    place_map_mines(&mut game);
    place_map_barrels(&mut game);
    game
}

fn place_map_mines(game: &mut GameState) {
    use arty::game::state::{PlacedMine, MineState};
    let seed = game.map_seed;
    let mine_count = 16 + (seed % 9) as usize;  // 16–24
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
    let count = 14 + (seed.wrapping_mul(0xDEAD_C0DE) % 7) as usize;  // 14–20
    let mut rng = seed.wrapping_mul(0xBEEF_1234_5678_9ABCu64).wrapping_add(1442695040888963407);
    let spread = WORLD_W / (count as u32 + 1);
    for i in 1..=count {
        rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
        let offset = (rng % spread as u64) as u32;
        let x = (spread * i as u32 + offset).clamp(20, WORLD_W - 20);
        if let Some(surf_y) = game.terrain.surface_y_at(x) {
            let pos = WorldPos::new(x as f32, surf_y as f32 - 1.0);
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


fn read_loop(s: ArcStream, inbox: Arc<Mutex<Option<InputMsg>>>, quit: Arc<AtomicBool>) {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        match read_one_arc(&s, &mut buf) {
            Some(frame) => {
                if let Ok(msg) = bincode::deserialize::<InputMsg>(&frame) {
                    if msg.quit { quit.store(true, Ordering::Relaxed); }
                    // Merge pressed/released events from the new message into any
                    // existing inbox entry rather than overwriting it wholesale.
                    // Without this, if two client frames arrive between server ticks,
                    // just_pressed events from the first are silently dropped.
                    let mut guard = inbox.lock().unwrap_or_else(|e| e.into_inner());
                    match guard.as_mut() {
                        Some(prev) => {
                            for b in &msg.pressed  { if !prev.pressed.contains(b)  { prev.pressed.push(*b);  } }
                            for b in &msg.released { if !prev.released.contains(b) { prev.released.push(*b); } }
                            prev.held              = msg.held;
                            prev.aim_angle         = msg.aim_angle;
                            prev.selected_weapon_kind = msg.selected_weapon_kind;
                            prev.hat_ids           = msg.hat_ids;
                            prev.uniform_color_ids = msg.uniform_color_ids;
                            prev.boot_color_ids    = msg.boot_color_ids;
                            prev.gun_style_ids     = msg.gun_style_ids;
                            prev.worm_names        = msg.worm_names;
                            prev.muzzle_x          = msg.muzzle_x;
                            prev.muzzle_y          = msg.muzzle_y;
                            prev.tick              = msg.tick;
                        }
                        None => *guard = Some(msg),
                    }
                }
            }
            None => break,
        }
    }
    info!("read_loop: client connection closed");
}

fn send_msg<T: serde::Serialize>(s: &ArcStream, msg: &T) {
    if let Some(bytes) = encode(msg) {
        write_arc(s, &bytes);
    }
}

fn write_arc(s: &ArcStream, bytes: &[u8]) {
    let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
    let _ = guard.write_all(bytes);
}

fn flush_arc(s: &ArcStream) {
    let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
    let _ = guard.flush();
}

/// Read one complete length-prefixed frame. Holds the lock briefly per chunk so
/// concurrent writes are never blocked more than ~5 ms.
fn read_one_arc(s: &ArcStream, read_buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    loop {
        if read_buf.len() >= 4 {
            let len = u32::from_le_bytes(read_buf[..4].try_into().unwrap()) as usize;
            if len == 0 || len > 65536 { return None; }
            if read_buf.len() >= 4 + len {
                let frame = read_buf[4..4+len].to_vec();
                read_buf.drain(..4+len);
                return Some(frame);
            }
        }
        let mut tmp = [0u8; 4096];
        let n = {
            let mut guard = s.lock().unwrap_or_else(|e| e.into_inner());
            // Short timeout keeps the lock window bounded so broadcast_lobby can
            // acquire the write lock without waiting more than ~5 ms.
            guard.get_ref().set_read_timeout(Some(Duration::from_millis(5))).ok();
            match guard.read(&mut tmp) {
                Ok(0) => return None,
                Ok(n) => n,
                Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                    drop(guard);
                    thread::sleep(Duration::from_millis(1));
                    continue;
                }
                Err(_) => return None,
            }
        };
        read_buf.extend_from_slice(&tmp[..n]);
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

/// Sanitize a player-supplied name: printable ASCII only, max 20 chars.
fn sanitize_name(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_graphic() || *c == ' ').take(20).collect()
}

const MAGIC: &[u8; 4] = b"MMAY";

/// Exact client version required. Bump with every release.
const REQUIRED_VERSION: &str = "0.5.4.396";

fn version_ok(ver: &str) -> bool {
    ver == REQUIRED_VERSION
}

/// Read up to `max` bytes until (and excluding) a `\n`, returning the trimmed string.
/// Returns None on read error.
fn read_line(stream: &mut impl Read, max: usize) -> Option<String> {
    let mut s = String::new();
    for _ in 0..max {
        let mut b = [0u8; 1];
        if stream.read_exact(&mut b).is_err() { return None; }
        if b[0] == b'\n' { break; }
        s.push(b[0] as char);
    }
    Some(s.trim().to_string())
}

fn stream_alive(s: &ArcStream) -> bool {
    let guard = s.lock().unwrap_or_else(|e| e.into_inner());
    let tcp = guard.get_ref();
    let mut b = [0u8; 1];
    tcp.set_read_timeout(Some(Duration::from_millis(1))).ok();
    let r = tcp.peek(&mut b);
    tcp.set_read_timeout(None).ok();
    match r {
        Ok(0) => false,
        Ok(_) => true,
        Err(e) => matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut),
    }
}

fn load_tls_config() -> Arc<rustls::ServerConfig> {
    let cert_path = std::env::var("ARTY_TLS_CERT")
        .unwrap_or_else(|_| "/etc/letsencrypt/live/crumbonium.duckdns.org/fullchain.pem".to_string());
    let key_path = std::env::var("ARTY_TLS_KEY")
        .unwrap_or_else(|_| "/etc/letsencrypt/live/crumbonium.duckdns.org/privkey.pem".to_string());
    let cert_file = std::fs::File::open(&cert_path).expect("TLS cert not found — set ARTY_TLS_CERT");
    let key_file  = std::fs::File::open(&key_path).expect("TLS key not found — set ARTY_TLS_KEY");
    let server_certs = certs(&mut BufReader::new(cert_file))
        .collect::<Result<Vec<_>, _>>().expect("invalid cert PEM");
    let private_key = private_key(&mut BufReader::new(key_file))
        .expect("key read error").expect("no private key in PEM");
    Arc::new(rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(server_certs, private_key)
        .expect("TLS config error"))
}

/// Accept one client: TCP accept → TLS handshake → app handshake → return ArcStream.
fn accept_one(listener: &TcpListener, tls_config: &Arc<rustls::ServerConfig>) -> (ArcStream, std::net::SocketAddr, String) {
    loop {
        let (mut tcp, addr) = match listener.accept() {
            Ok(pair) => pair,
            Err(e) => { info!("accept error: {e}"); continue; }
        };
        tcp.set_read_timeout(Some(Duration::from_secs(5))).ok();
        tcp.set_nodelay(true).ok();

        // TLS handshake
        let conn = match ServerConnection::new(Arc::clone(tls_config)) {
            Ok(c) => c,
            Err(e) => { info!("TLS init error from {addr}: {e}"); continue; }
        };
        let mut tls = rustls::StreamOwned::new(conn, tcp);

        // Application handshake over TLS
        let mut magic = [0u8; 4];
        match tls.read_exact(&mut magic) {
            Ok(_) if &magic == MAGIC => {}
            _ => { info!("Rejected (bad magic): {addr}"); continue; }
        }
        let ver = match read_line(&mut tls, 16) {
            Some(v) => v,
            None => { info!("Handshake read failed: {addr}"); continue; }
        };
        if !version_ok(&ver) {
            info!("Rejected old version {ver}: {addr}");
            let _ = tls.write_all(b"REJECTED:VERSION\n");
            continue;
        }
        let _ = tls.write_all(b"OK\n");
        let token = match read_line(&mut tls, 70) {
            Some(t) => t,
            None => { info!("Handshake read failed: {addr}"); continue; }
        };
        // Clear handshake timeout; normal I/O timeouts are set per-connection downstream.
        tls.get_ref().set_read_timeout(None).ok();
        info!("Player (v{ver}): {addr}");
        return (Arc::new(Mutex::new(tls)), addr, token);
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
