//! Replay harness tests (docs/EXPERIMENTAL_PLAN.md step 1).
//!
//! * determinism: the same replay run twice must give identical state hashes at every checkpoint;
//! * golden: checkpoints are compared against `tests/replays/<name>.golden`. A mismatch names the first diverging tick. When a
//!   change to behaviour is intended, re-bless with `REPLAY_BLESS=1 cargo test --test replay`;
//! * timing: `cargo test --release --test replay -- --nocapture` prints host-side sim cost per scenario.
use arty::input::Button::*;
use arty::replay::{script, Replay, RunResult, TickInput};
use std::path::PathBuf;

fn dir() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/replays") }

/// Walk, jump, backflip, fire, then a few hundred idle ticks so the turn resolves and the next team acts.
fn scenario_walk_jump_fire(seed: u64) -> Replay {
    let mut r = Replay::new(seed, 2);
    r.frames = script(&[
        (&[], 30),
        (&[Right], 45), (&[], 10),
        (&[B], 1), (&[], 60),
        (&[Left], 30), (&[], 10),
        (&[Y], 1), (&[], 60),
        (&[Up], 20), (&[A], 25), (&[], 300),
        (&[Left], 60), (&[], 5), (&[B], 1), (&[], 50),
        (&[Right], 40), (&[A], 15), (&[], 300),
    ]);
    r
}

fn check_golden(name: &str, r: &Replay) {
    let res = r.run();
    let golden = dir().join(format!("{name}.golden"));
    let text: String = res.checkpoints.iter().map(|(t, h)| format!("{t} {h:016x}\n")).collect();
    if std::env::var("REPLAY_BLESS").is_ok() || !golden.exists() {
        std::fs::create_dir_all(dir()).unwrap();
        std::fs::write(dir().join(format!("{name}.replay")), r.to_text()).unwrap();
        std::fs::write(&golden, &text).unwrap();
        eprintln!("blessed {name}: {} checkpoints", res.checkpoints.len());
        return;
    }
    let want = std::fs::read_to_string(&golden).unwrap();
    for (got, exp) in text.lines().zip(want.lines()) {
        let tick = got.split_whitespace().next().unwrap_or("?");
        assert_eq!(got, exp, "{name}: state first diverges at tick {tick} (checkpoint 'tick hash'); re-bless with REPLAY_BLESS=1 if the change is intended");
    }
    assert_eq!(text.lines().count(), want.lines().count(), "{name}: checkpoint count changed");
}

fn report(name: &str, r: &Replay, res: &RunResult) {
    println!("{name}: {} ticks, sim total {:.1} ms, avg {:.0} us/tick, worst tick {} us (host)",
        r.frames.len(), res.sim_us_total as f64 / 1000.0, res.sim_us_total as f64 / r.frames.len().max(1) as f64, res.sim_us_max);
}

#[test]
fn replay_is_deterministic() {
    for seed in [0x18BD410020ECA8CDu64, 7, 1234] {
        let r = scenario_walk_jump_fire(seed);
        let a = r.run();
        let b = r.run();
        assert_eq!(a.checkpoints, b.checkpoints, "seed {seed:x}: two runs of the same replay diverged");
        report(&format!("walk_jump_fire seed {seed:x}"), &r, &a);
    }
}

#[test]
fn golden_walk_jump_fire() {
    check_golden("walk_jump_fire", &scenario_walk_jump_fire(0x18BD410020ECA8CD));
}

#[test]
fn saved_replay_reloads_identically() {
    let r = scenario_walk_jump_fire(7);
    let r2 = Replay::from_text(&r.to_text()).unwrap();
    assert_eq!(r.run().checkpoints, r2.run().checkpoints);
}

#[test]
fn scenario_actually_does_something_and_hash_is_sensitive() {
    let r = scenario_walk_jump_fire(0x18BD410020ECA8CD);
    let start = arty::replay::new_game(r.seed, &r.colors);
    let (res, end) = r.run_game();
    // someone moved, and a shot landed (crater) or damage happened
    let moved = start.teams.iter().zip(end.teams.iter()).any(|(a, b)| a.soldiers.iter().zip(b.soldiers.iter()).any(|(x, y)| (x.pos.x - y.pos.x).abs() > 5.0));
    let hurt = end.teams.iter().any(|t| t.soldiers.iter().any(|s| s.hp < 100)) || !end.crater_log.is_empty();
    assert!(moved, "scenario moved nobody");
    assert!(hurt, "scenario hurt nobody and cut no terrain");
    // hash changes as the game evolves, and a one-tick input change alters the final hash
    let hs: std::collections::HashSet<u64> = res.checkpoints.iter().map(|c| c.1).collect();
    assert!(hs.len() > res.checkpoints.len() / 2, "state hash barely changes: {} distinct of {}", hs.len(), res.checkpoints.len());
    let mut r2 = r.clone();
    // drop the whole first button run (a single tick can legitimately converge to the same end state)
    let i = r2.frames.iter().position(|f| f.held != 0).unwrap();
    let first = r2.frames[i].held;
    for f in r2.frames[i..].iter_mut() { if f.held != first { break; } *f = TickInput::default(); }
    assert_ne!(res.checkpoints.last(), r2.run().checkpoints.last(), "removing the first input run did not change the outcome");
}

/// Long pseudo-random input stream (fixed LCG, so it is the same every run): buttons change every few ticks, weighted towards
/// walking, jumping, backflips and charge-and-fire. This is the net for refactors of ground/support/fall logic: it drives
/// soldiers over craters, ledges and each other. Any behaviour change shows as a golden mismatch at the first diverging tick.
fn scenario_random(seed: u64, ticks: usize) -> Replay {
    let mut r = Replay::new(seed, 2);
    let mut x = seed ^ 0x9E3779B97F4A7C15;
    let mut next = move || { x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407); (x >> 33) as u32 };
    let pool: [&[arty::input::Button]; 12] = [&[], &[], &[Right], &[Right], &[Left], &[Left], &[Right, B], &[Left, B], &[Y], &[A], &[Up, A], &[Down]];
    while r.frames.len() < ticks {
        let hold = pool[next() as usize % pool.len()];
        let n = 2 + next() as usize % 25;
        r.frames.extend(script(&[(hold, n)]));
    }
    r.frames.truncate(ticks);
    r
}

#[test]
fn golden_random_inputs() {
    for (name, seed) in [("random_a", 0x18BD410020ECA8CDu64), ("random_b", 0x18e913e785709108), ("random_c", 5), ("random_d", 99)] {
        let r = scenario_random(seed, 4000);
        let a = r.run();
        assert_eq!(a.checkpoints, r.run().checkpoints, "{name}: not deterministic");
        check_golden(name, &r);
    }
}

#[test]
fn random_scenarios_exercise_the_game() {
    for seed in [0x18BD410020ECA8CDu64, 0x18e913e785709108, 5, 99] {
        let (_, g) = scenario_random(seed, 4000).run_game();
        let hp: Vec<u32> = g.teams.iter().map(|t| t.soldiers.iter().map(|s| s.hp as u32).sum()).collect();
        println!("seed {seed:x}: turns {} craters {} team hp {:?}", g.turn.turn_number, g.crater_log.len(), hp);
        assert!(g.turn.turn_number >= 2 && !g.crater_log.is_empty(), "random scenario for seed {seed:x} never fired or never advanced a turn");
    }
}

/// The invariant behind the "blue soldier 3 stuck" report: a living soldier the sim leaves standing (Idle) must be able to act
/// (walk / jump / backflip) within a few ticks. `Support::None` for a tick or two is the normal handoff (he starts falling on the
/// next tick); `Embedded` (body in terrain, nothing under the feet) must resolve quickly, because he is sinking free. Before the
/// single `support()` rule and the embedded drift, a worm that bumped a ceiling hung in mid-air for 80+ ticks unable to do anything.
#[test]
fn idle_soldiers_can_act_within_a_few_ticks() {
    use arty::game::loop_runner::support;
    use arty::game::soldier::SoldierState;
    use std::collections::HashMap;
    const LIMIT: usize = 12; // bounded: an embedded soldier sinks 3 px/tick, at most ~BODY_H/3 (6) ticks
    let mut checked = 0u64;
    let mut by_kind = std::collections::BTreeMap::<String, u64>::new();
    let mut longest = 0usize;
    let mut worst: Option<String> = None;
    for seed in [0x18BD410020ECA8CDu64, 0x18e913e785709108, 5, 99, 7, 11, 13, 21] {
        let r = scenario_random(seed, 4000);
        let mut stuck: HashMap<(usize, usize), usize> = HashMap::new();
        r.run_observed(|g, tick| {
            for (ti, t) in g.teams.iter().enumerate() { for (si, s) in t.soldiers.iter().enumerate() {
                let k = (ti, si);
                if !s.is_alive() || !matches!(s.state, SoldierState::Idle) { stuck.remove(&k); continue; }
                let sup = support(g, ti, si);
                checked += 1;
                *by_kind.entry(format!("{sup:?}")).or_default() += 1;
                if sup.can_act() { stuck.remove(&k); continue; }
                let n = stuck.entry(k).or_insert(0);
                *n += 1;
                longest = longest.max(*n);
                if *n > LIMIT && worst.is_none() {
                    worst = Some(format!("seed {seed:x} tick {tick}: team {ti} soldier {si} at ({:.1},{:.1}) Idle but not able to act for {} ticks ({sup:?})", s.pos.x, s.pos.y, n));
                }
            } }
        });
    }
    println!("idle soldier-ticks checked: {checked}, by support: {by_kind:?}, longest not-able-to-act run: {longest}");
    assert!(worst.is_none(), "{}", worst.unwrap());
}

/// The server-supplied parts of a tick (aim angle, muzzle, weapon kind) must reach the sim: a replay that records them has to
/// behave differently from the same buttons without them, or recorded live matches would not reproduce.
#[test]
fn recorded_aim_and_weapon_drive_the_sim() {
    let base = scenario_walk_jump_fire(7);
    let plain = base.run().checkpoints;
    let mut aimed = base.clone();
    for f in aimed.frames.iter_mut() { f.aim = Some(1.0); }
    assert_ne!(plain, aimed.run().checkpoints, "an aim angle override on every tick changed nothing");
    let mut again = aimed.clone();
    assert_eq!(aimed.run().checkpoints, again.run().checkpoints);
    again.frames[100].muzzle = Some((300.0, 300.0));
    assert_eq!(Replay::from_text(&again.to_text()).unwrap().to_text(), again.to_text());
}

/// What the server hook does, end to end: drive a game through `server_tick`, recording each tick the way `MatchRecorder::push` is
/// called in the server loop, save to text, reload, replay: every checkpoint must match the live run.
#[test]
fn recorder_reproduces_a_live_run_after_save_and_reload() {
    use arty::game::loop_runner::server_tick;
    use arty::input::InputState;
    use arty::replay::{new_game, state_hash, Recorder};
    let seed = 0x18BD410020ECA8CDu64;
    let colors = [0u8, 1];
    let mut game = new_game(seed, &colors);
    let mut rec = Recorder::new(seed, &colors);
    let mut live = Vec::new();
    let script_frames = scenario_random(seed, 1500).frames;
    for (i, f) in script_frames.iter().enumerate() {
        // as the server: build the input from the message, derive aim/muzzle overrides, then tick
        let input = InputState::from_sets(f.held, f.pressed, f.released);
        let aim = if i % 7 == 0 { Some(0.5 + (i as f32) * 0.001) } else { None };
        let muzzle = if i % 11 == 0 { Some((400.0 + i as f32, 300.0)) } else { None };
        rec.push(&input, aim, muzzle, None);
        server_tick(&mut game, &input, muzzle, aim);
        if (i + 1) % arty::replay::CHECKPOINT_STRIDE == 0 || i + 1 == script_frames.len() { live.push((i + 1, state_hash(&game))); }
    }
    let replay = Replay::from_text(&rec.finish().to_text()).unwrap();
    assert_eq!(replay.run().checkpoints, live, "a recorded + reloaded replay diverged from the live run");
}

/// Was `spawn_walk_probe::probe_on_ground` (print-only): every soldier the live builder spawns must be standing on firm ground,
/// on a spread of seeds, so a spawn can never start a match stuck or falling.
#[test]
fn every_spawned_soldier_starts_grounded() {
    use arty::game::loop_runner::is_on_ground;
    let mut bad = Vec::new();
    let mut seeds = vec![0x18BD410020ECA8CDu64, 0x18e913e785709108];
    seeds.extend((1..60u64).map(|i| i.wrapping_mul(0x9E3779B97F4A7C15)));
    for seed in seeds {
        let g = arty::replay::new_game(seed, &[0, 1]);
        for (ti, t) in g.teams.iter().enumerate() {
            for (si, s) in t.soldiers.iter().enumerate() {
                if !is_on_ground(&g, ti, si) { bad.push(format!("seed {seed:x} team{ti} s{si} at ({:.1},{:.1})", s.pos.x, s.pos.y)); }
            }
        }
    }
    assert!(bad.is_empty(), "{} soldiers spawned off the ground: {:?}", bad.len(), &bad[..bad.len().min(5)]);
}

/// Soldiers must not walk into a gap lower than HEAD_CLEAR, but a soldier already under a low ceiling can always walk out.
#[test]
fn walking_refuses_gaps_smaller_than_the_soldier() {
    use arty::game::loop_runner::{headroom, walk_headroom_ok, HEAD_CLEAR};
    let mut g = arty::replay::new_game(7, &[0, 1]);
    let (x, y) = { let s = &g.teams[0].soldiers[0]; (s.pos.x as i32, s.pos.y as i32) };
    // Clear a corridor of air 80 px high around the soldier, then lay a ceiling slab to the right at 20 px above the floor.
    for dx in -40..=60 { for dy in 1..=80 { g.terrain.set_solid(x + dx, y - dy, false); } }
    for dx in 20..=60 { for dy in 21..=30 { g.terrain.set_solid(x + dx, y - dy, true); } }
    assert!(headroom(&g.terrain, x, y, HEAD_CLEAR) >= HEAD_CLEAR);
    assert!(!walk_headroom_ok(&g.terrain, x, y, x + 30, y), "walked into a 20 px gap");
    assert!(walk_headroom_ok(&g.terrain, x, y, x + 10, y), "open ground refused");
    // Already inside the low gap: moving along it (same headroom) or out of it is allowed.
    assert!(walk_headroom_ok(&g.terrain, x + 30, y, x + 40, y));
    assert!(walk_headroom_ok(&g.terrain, x + 30, y, x + 10, y));
}
