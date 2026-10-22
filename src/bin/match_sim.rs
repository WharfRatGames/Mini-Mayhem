//! Headless CPU-vs-CPU batch match simulator, for crash/bug testing and
//! balance stats. Sim-only: does not touch or affect the shipped desktop
//! VS-CPU experience (see `src/game/cpu.rs::decide_with_difficulty` and
//! `src/game/cpu_driver.rs`).
//!
//! Panic isolation requires an unwind build — the default `release` profile
//! uses `panic = "abort"`, which makes catch_unwind a no-op. Build/run with:
//!
//!   cargo build --profile server --bin match-sim
//!   cargo run --profile server --bin match-sim -- --matches 200 --teams 2 --difficulty easy,hard
//!
//! Flags:
//!   --matches N        number of matches to run (default 50)
//!   --seed N            starting seed (default 1); matches use seed, seed+1, ...
//!   --teams N           team count 2-4 (default 2)
//!   --difficulty a,b,.. per-team difficulty: easy|medium|hard (default all easy)
//!   --max-ticks N       safety cap per match (default 54000 = ~30min at 30Hz)
//!   --csv PATH          also write one row per match to PATH
//!   --events-csv PATH   also write one row per fire/death event across all matches to PATH

use arty::game::cpu_driver::CpuDriver;
use arty::game::state::GameState;
use arty::game::team::{Difficulty, Team};
use arty::input::InputState;
use arty::physics::projectile::WeaponKind;
use arty::world::terrain::Terrain;
use arty::world::WORLD_W;

fn add_fire(counts: &mut Vec<(WeaponKind, u32)>, kind: WeaponKind) {
    match counts.iter_mut().find(|(k, _)| *k == kind) {
        Some((_, n)) => *n += 1,
        None => counts.push((kind, 1)),
    }
}

fn merge_fires(into: &mut Vec<(WeaponKind, u32)>, from: &[(WeaponKind, u32)]) {
    for (k, n) in from {
        match into.iter_mut().find(|(k2, _)| k2 == k) {
            Some((_, m)) => *m += n,
            None => into.push((*k, *n)),
        }
    }
}

struct MatchConfig {
    seed: u64,
    team_count: usize,
    difficulties: Vec<Difficulty>,
    max_ticks: u32,
}

struct MatchOutcome {
    seed: u64,
    result: arty::game::state::GameResult,
    timed_out: bool,
    ticks_elapsed: u32,
    turns_elapsed: u32,
    weapon_fires: Vec<(WeaponKind, u32)>,
    panicked: bool,
    panic_msg: Option<String>,
    events: Vec<EventRow>,
}

/// One row per fire/death event, for post-hoc analysis across a whole batch.
struct EventRow {
    seed: u64,
    tick: u32,
    turn: u32,
    kind: &'static str,   // "fire" | "death"
    team: usize,
    soldier_idx: usize,
    weapon: Option<WeaponKind>,
    angle: Option<f32>,
    power: Option<f32>,
    hp: Option<u8>,
}

fn parse_difficulty(s: &str) -> Difficulty {
    match s.to_ascii_lowercase().as_str() {
        "medium" | "med" | "m" => Difficulty::Medium,
        "hard" | "h" => Difficulty::Hard,
        _ => Difficulty::Easy,
    }
}

fn build_match(cfg: &MatchConfig) -> GameState {
    let mut terrain = Terrain::generate_tactical(cfg.seed);
    let all = terrain.find_team_spawns(0, WORLD_W, cfg.team_count * 4);
    let teams: Vec<Team> = (0..cfg.team_count)
        .map(|slot| {
            let spawns: Vec<_> = all.iter().cloned().enumerate()
                .filter(|(i, _)| i % cfg.team_count == slot)
                .map(|(_, s)| s)
                .collect();
            Team::new(slot, true, cfg.difficulties[slot], &spawns)
        })
        .collect();
    GameState::new(cfg.seed, terrain, teams, cfg.team_count)
}

fn run_match(cfg: &MatchConfig) -> MatchOutcome {
    let mut game = build_match(cfg);
    let mut driver = CpuDriver::new(cfg.team_count);
    let mut weapon_fires: Vec<(WeaponKind, u32)> = Vec::new();
    let mut events: Vec<EventRow> = Vec::new();
    let mut timed_out = false;

    // hp snapshot per (team, soldier) to detect death transitions each tick
    let mut prev_hp: Vec<Vec<u8>> = game.teams.iter().map(|t| t.soldiers.iter().map(|s| s.hp).collect()).collect();

    loop {
        if !matches!(game.result, arty::game::state::GameResult::Ongoing) {
            break;
        }
        if game.tick >= cfg.max_ticks {
            timed_out = true;
            break;
        }
        let seed = game.tick.wrapping_mul(1664525).wrapping_add(1013904223).wrapping_add(cfg.seed as u32);
        let active = game.active_team();
        let active_soldier = game.teams[active].active;
        let seed_for_input = seed;
        let (input, fired) = driver.drive_turn(&mut game, seed_for_input);
        if let Some(kind) = fired {
            add_fire(&mut weapon_fires, kind);
            events.push(EventRow {
                seed: cfg.seed, tick: game.tick, turn: game.turn.turn_number,
                kind: "fire", team: active, soldier_idx: active_soldier,
                weapon: Some(kind), angle: Some(game.aim.angle), power: Some(game.aim.power), hp: None,
            });
        }
        let input: InputState = input;
        arty::game::loop_runner::server_tick(&mut game, &input, None, None);

        if std::env::var("MATCH_SIM_DEBUG").is_ok() && game.tick % 2000 == 0 {
            let ti0 = game.active_team();
            let si0 = game.teams[ti0].active;
            let s = &game.teams[ti0].soldiers[si0];
            let airborne: Vec<(usize, usize)> = game.teams.iter().enumerate()
                .flat_map(|(ti, t)| t.soldiers.iter().enumerate().filter(|(_, s)| matches!(s.state, arty::game::soldier::SoldierState::Airborne { .. })).map(move |(si, _)| (ti, si)))
                .collect();
            let airborne_pos: Vec<(usize, usize, f32, f32, u32, bool)> = airborne.iter()
                .map(|&(ti, si)| { let s = &game.teams[ti].soldiers[si]; (ti, si, s.pos.x, s.pos.y, s.airtime, s.is_alive()) })
                .collect();
            let crates_unlanded = game.crates.iter().filter(|c| !c.landed).count();
            eprintln!("DEBUG tick={} turn={} phase={:?} active=({ti0},{si0}) hp={} state={:?} pending_dmg={} settle={} hp_delay={} projectiles={} explosions={} pending_deaths={} black_holes={} garcia={} airstrike={} jumpbot={} crate_watch_ticks={} crates_unlanded={} airborne={:?} airborne_pos={:?}",
                game.tick, game.turn.turn_number, game.turn.phase, s.hp, s.state,
                s.pending_damage, s.damage_settle, s.hp_countdown_delay,
                game.projectiles.len(), game.explosions.len(), game.pending_deaths.len(), game.black_holes.len(),
                game.garcia.is_some(), game.airstrike.is_some(), game.jumpbot.is_some(),
                game.crate_watch_ticks, crates_unlanded, airborne, airborne_pos);
        }

        for (ti, team) in game.teams.iter().enumerate() {
            for (si, sol) in team.soldiers.iter().enumerate() {
                if prev_hp[ti][si] > 0 && sol.hp == 0 {
                    events.push(EventRow {
                        seed: cfg.seed, tick: game.tick, turn: game.turn.turn_number,
                        kind: "death", team: ti, soldier_idx: si,
                        weapon: None, angle: None, power: None, hp: Some(0),
                    });
                }
                prev_hp[ti][si] = sol.hp;
            }
        }
    }

    MatchOutcome {
        seed: cfg.seed,
        result: game.result,
        timed_out,
        ticks_elapsed: game.tick,
        turns_elapsed: game.turn.turn_number,
        weapon_fires,
        panicked: false,
        panic_msg: None,
        events,
    }
}

fn run_batch(n: u32, start_seed: u64, team_count: usize, difficulties: &[Difficulty], max_ticks: u32) -> Vec<MatchOutcome> {
    (0..n)
        .map(|i| {
            let cfg = MatchConfig {
                seed: start_seed + i as u64,
                team_count,
                difficulties: difficulties.to_vec(),
                max_ticks,
            };
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_match(&cfg))) {
                Ok(outcome) => outcome,
                Err(e) => MatchOutcome {
                    seed: cfg.seed,
                    result: arty::game::state::GameResult::Ongoing,
                    timed_out: false,
                    ticks_elapsed: 0,
                    turns_elapsed: 0,
                    weapon_fires: Vec::new(),
                    panicked: true,
                    panic_msg: Some(panic_payload_to_string(e)),
                    events: Vec::new(),
                },
            }
        })
        .collect()
}

fn panic_payload_to_string(e: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = e.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = e.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

fn main() {
    let mut n_matches: u32 = 50;
    let mut start_seed: u64 = 1;
    let mut team_count: usize = 2;
    let mut difficulties: Vec<Difficulty> = vec![Difficulty::Easy; 2];
    let mut max_ticks: u32 = 54_000;
    let mut csv_path: Option<String> = None;
    let mut events_csv_path: Option<String> = None;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--matches" => n_matches = args.next().unwrap().parse().unwrap(),
            "--seed" => start_seed = args.next().unwrap().parse().unwrap(),
            "--teams" => {
                team_count = args.next().unwrap().parse().unwrap();
                difficulties = vec![Difficulty::Easy; team_count];
            }
            "--difficulty" => {
                let list = args.next().unwrap();
                let parsed: Vec<Difficulty> = list.split(',').map(parse_difficulty).collect();
                if parsed.len() == team_count {
                    difficulties = parsed;
                } else {
                    eprintln!("--difficulty count ({}) doesn't match --teams ({}); ignoring", parsed.len(), team_count);
                }
            }
            "--max-ticks" => max_ticks = args.next().unwrap().parse().unwrap(),
            "--csv" => csv_path = args.next(),
            "--events-csv" => events_csv_path = args.next(),
            other => eprintln!("unknown flag: {other}"),
        }
    }

    let outcomes = run_batch(n_matches, start_seed, team_count, &difficulties, max_ticks);

    let mut panics = 0u32;
    let mut timeouts = 0u32;
    let mut wins = vec![0u32; team_count];
    let mut draws = 0u32;
    let mut total_ticks = 0u64;
    let mut total_turns = 0u64;
    let mut max_ticks_seen = 0u32;
    let mut weapon_totals: Vec<(WeaponKind, u32)> = Vec::new();

    for o in &outcomes {
        if o.panicked {
            panics += 1;
            eprintln!("PANIC seed={} msg={}", o.seed, o.panic_msg.as_deref().unwrap_or(""));
            continue;
        }
        if o.timed_out { timeouts += 1; }
        match o.result {
            arty::game::state::GameResult::Winner(t) if t < team_count => wins[t] += 1,
            arty::game::state::GameResult::Draw => draws += 1,
            _ => {}
        }
        total_ticks += o.ticks_elapsed as u64;
        total_turns += o.turns_elapsed as u64;
        max_ticks_seen = max_ticks_seen.max(o.ticks_elapsed);
        merge_fires(&mut weapon_totals, &o.weapon_fires);
    }

    let completed = outcomes.len() as u32 - panics;
    println!("=== match-sim results ===");
    println!("matches: {n_matches}  panics: {panics}  timeouts: {timeouts}");
    if completed > 0 {
        println!("avg ticks: {:.0}  max ticks: {}  avg turns: {:.0}",
            total_ticks as f64 / completed as f64, max_ticks_seen, total_turns as f64 / completed as f64);
    }
    for (t, w) in wins.iter().enumerate() {
        println!("team {t} ({:?}) wins: {w}  ({:.1}%)", difficulties[t], 100.0 * *w as f64 / completed.max(1) as f64);
    }
    println!("draws: {draws}");
    println!("weapon usage:");
    let mut wt = weapon_totals;
    wt.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, v) in wt {
        println!("  {k:?}: {v}");
    }

    if let Some(path) = csv_path {
        use std::io::Write;
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "seed,result,timed_out,ticks,turns,panicked").unwrap();
        for o in &outcomes {
            writeln!(f, "{},{:?},{},{},{},{}", o.seed, o.result, o.timed_out, o.ticks_elapsed, o.turns_elapsed, o.panicked).unwrap();
        }
        println!("wrote {path}");
    }

    if let Some(path) = events_csv_path {
        use std::io::Write;
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "seed,tick,turn,kind,team,soldier_idx,weapon,angle,power,hp").unwrap();
        let mut n_events = 0u64;
        for o in &outcomes {
            for e in &o.events {
                writeln!(f, "{},{},{},{},{},{},{},{},{},{}",
                    e.seed, e.tick, e.turn, e.kind, e.team, e.soldier_idx,
                    e.weapon.map(|w| format!("{w:?}")).unwrap_or_default(),
                    e.angle.map(|a| format!("{a:.4}")).unwrap_or_default(),
                    e.power.map(|p| format!("{p:.4}")).unwrap_or_default(),
                    e.hp.map(|h| h.to_string()).unwrap_or_default(),
                ).unwrap();
                n_events += 1;
            }
        }
        println!("wrote {path} ({n_events} events)");
    }
}
