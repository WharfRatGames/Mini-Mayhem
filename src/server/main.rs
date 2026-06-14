mod msg;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Info)
        .init();
    // ARTY_PORT env var lets the API spawn instances on different ports
    let port: u16 = std::env::var("ARTY_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(PORT_DEFAULT);
    info!("Miyoo Mayhem server on :{}", port);
    let listener = TcpListener::bind(("0.0.0.0", port)).expect("bind failed");
    loop {
    info!("Waiting for 2 players...");

    let (s0, a0) = accept_player(&listener, 0);
    let (s1, a1) = accept_player(&listener, 1);
    s0.set_nodelay(true).ok();
    s1.set_nodelay(true).ok();
    s0.set_write_timeout(Some(Duration::from_millis(50))).ok();
    s1.set_write_timeout(Some(Duration::from_millis(50))).ok();
    s0.set_read_timeout(Some(Duration::from_secs(5))).ok();
    s1.set_read_timeout(Some(Duration::from_secs(5))).ok();
    info!("Both connected - starting!");
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
    use std::sync::atomic::{AtomicBool, Ordering};
    let disc0 = Arc::new(AtomicBool::new(false));
    let disc1 = Arc::new(AtomicBool::new(false));

    let (read_s0, mut ws0, mut ws1, read_s1) = match (
        s0.try_clone(), s0.try_clone(), s1.try_clone(), s1.try_clone()
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        _ => { info!("Socket clone failed — resetting"); continue; }
    };
    thread::spawn({
        let i = inp0.clone(); let d = disc0.clone();
        move || { read_loop(read_s0, i); d.store(true, Ordering::Relaxed); }
    });
    thread::spawn({
        let i = inp1.clone(); let d = disc1.clone();
        move || { read_loop(read_s1, i); d.store(true, Ordering::Relaxed); }
    });

    let mut game = build_game(seed);
    let mut tick: u32 = 0;

    loop {
        let t = Instant::now();
        tick = tick.wrapping_add(1);
        game.tick = tick;

        // Immediate disconnect detection via read-thread flags
        if disc0.load(Ordering::Relaxed) || disc1.load(Ordering::Relaxed) {
            info!("Client disconnected — resetting (disc0={} disc1={})",
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
        arty::game::loop_runner::server_tick(&mut game, &input_state);

        // Game over — send final state for 3 seconds then start a new game
        if !matches!(game.result, arty::game::state::GameResult::Ongoing) {
            if let Some(final_bytes) = encode(&build_state(&game, tick)) {
            for _ in 0..90 {
                if disc0.load(Ordering::Relaxed) || disc1.load(Ordering::Relaxed) { break; }
                let _ = ws0.write_all(&final_bytes);
                let _ = ws1.write_all(&final_bytes);
                thread::sleep(TICK_DURATION);
            }
            } // end if let Some(final_bytes)
            if disc0.load(Ordering::Relaxed) || disc1.load(Ordering::Relaxed) {
                info!("Client left during game-over — resetting");
                break;
            }
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            game = build_game(seed);
            tick = 0;
            *inp0.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *inp1.lock().unwrap_or_else(|e| e.into_inner()) = None;
            send_msg(&ws0, &WelcomeMsg { your_team: 0, seed });
            send_msg(&ws1, &WelcomeMsg { your_team: 1, seed });
            info!("Game over — new game with seed {}", seed);
            continue;
        }

        if let Some(state_bytes) = encode(&build_state(&game, tick)) {
            let _ = ws0.write_all(&state_bytes);
            let _ = ws1.write_all(&state_bytes);
        }

        let e = t.elapsed();
        if e < TICK_DURATION { thread::sleep(TICK_DURATION - e); }
    }
    } // end outer loop
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

fn build_state(game: &GameState, tick: u32) -> StateMsg {
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
        craters: game.crater_log.iter().map(|e| NetCrater { cx: e.0, cy: e.1, radius: e.2 }).collect(),
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
            cursor_x: g.cursor_x, render_x: g.render_x, blink_timer: g.blink_timer,
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

const REQUIRED_VERSION: &str = "0.5.4.145";

fn accept_player(listener: &TcpListener, slot: usize) -> (TcpStream, std::net::SocketAddr) {
    loop {
        let (mut stream, addr) = match listener.accept() {
            Ok(pair) => pair,
            Err(e) => { info!("accept error: {e}"); continue; }
        };
        let mut buf = [0u8; 4];
        stream.set_read_timeout(Some(Duration::from_secs(3))).ok();
        match stream.read_exact(&mut buf) {
            Ok(_) if &buf == MAGIC => {
                // Read version line
                let mut ver_buf = [0u8; 16];
                let mut ver = String::new();
                for i in 0..16 {
                    let mut b = [0u8; 1];
                    if stream.read_exact(&mut b).is_err() { break; }
                    if b[0] == b'\n' { break; }
                    ver_buf[i] = b[0];
                    ver.push(b[0] as char);
                }
                if ver.trim() != REQUIRED_VERSION {
                    info!("Rejected wrong version {}: {}", ver.trim(), addr);
                    continue;
                }
                stream.set_read_timeout(None).ok();
                info!("Player {} (v{}): {}", slot, ver.trim(), addr);
                return (stream, addr);
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
