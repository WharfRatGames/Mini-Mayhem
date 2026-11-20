//! Headless scenario sim: stage a situation, step the real game (`server_tick`), print a per-tick trace and write a
//! contact-sheet PNG of rendered frames. Lets behaviour (HP countdown, explosions, banana, plates) be checked on the
//! host without a device or the Wine rig.
//!
//!   cargo run --release --example scenario -- <hp|bazooka|banana|plates> [--ticks N] [--every K] [--out file.png] [--seed S]
use arty::game::loop_runner::{render, server_tick, LoopState};
use arty::input::InputState;
use arty::physics::projectile::{FuseState, Projectile, WeaponKind};
use arty::renderer::buffer::WorldBuffer;
use arty::renderer::camera::Camera;
use arty::world::{WorldPos, Vec2, WORLD_W};

const CW: usize = 320;
const CH: usize = 240;

/// Button script from `--keys "TICK:A,FROM-TO:R"` (A = one-tick press; L/R = held over the range).
fn scripted_input(a: &[String], t: usize) -> InputState {
    use arty::input::Button;
    let keys = a.iter().position(|x| x == "--keys").and_then(|i| a.get(i + 1)).cloned().unwrap_or_default();
    let bit = |b: Button| 1u16 << Button::ALL.iter().position(|&x| x == b).unwrap();
    let (mut now, mut prev) = (0u16, 0u16);
    for item in keys.split(',').filter(|s| !s.is_empty()) {
        let (range, key) = item.split_once(':').unwrap();
        let b = match key { "A" => Button::A, "L" => Button::Left, _ => Button::Right };
        let (lo, hi) = match range.split_once('-') { Some((l, h)) => (l.parse::<usize>().unwrap(), h.parse::<usize>().unwrap()), None => { let v = range.parse().unwrap(); (v, v) } };
        if t >= lo && t <= hi { now |= bit(b); }
        if t >= 1 && t - 1 >= lo && t - 1 <= hi { prev |= bit(b); }
    }
    InputState::from_bits(prev, now)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let name = a.get(1).map(String::as_str).unwrap_or("hp").to_string();
    let arg = |k: &str, d: usize| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(d);
    let ticks = arg("--ticks", 150);
    let every = arg("--every", 10);
    let seed = arg("--seed", 0x18BD410020ECA8CD) as u64;
    let out = a.iter().position(|x| x == "--out").and_then(|i| a.get(i + 1)).cloned().unwrap_or(format!("/tmp/scenario_{name}.png"));

    let json = a.iter().any(|x| x == "--json");
    let damage = arg("--damage", 43) as u32;
    let mut g = arty::replay::new_game(seed, &[0, 1]);
    // Victim = team 1 soldier 0, observer-friendly: everyone else is left alone.
    let (mut vt, mut vs) = (1usize, 0usize);
    let vp = g.teams[vt].soldiers[vs].pos;
    match name.as_str() {
        "hp" => g.teams[vt].soldiers[vs].take_damage(damage),
        "bazooka" if a.iter().any(|x| x == "--at") => {
            // --at X,Y puts the victim's feet there; --ex X,Y is the blast centre.
            let pair = |k: &str| -> (f32, f32) { let v: Vec<f32> = a[a.iter().position(|x| x == k).unwrap() + 1].split(',').map(|x| x.parse().unwrap()).collect(); (v[0], v[1]) };
            let (ax, ay) = pair("--at"); let (ex, ey) = pair("--ex");
            g.teams[vt].soldiers[vs].pos = WorldPos::new(ax, ay);
            g.apply_explosion(WorldPos::new(ex, ey), WeaponKind::Bazooka); g.turn.phase = arty::game::turn::TurnPhase::Watching;
            if let arty::game::soldier::SoldierState::Airborne { vel, .. } = &g.teams[vt].soldiers[vs].state { println!("LAUNCH vel ({:.3},{:.3}) px/tick = ({:.3},{:.3}) px/WA-frame", vel.x, vel.y, vel.x * 0.6, vel.y * 0.6); }
        }
        "bazooka" => { g.apply_explosion(WorldPos::new(vp.x + 18.0, vp.y), WeaponKind::Bazooka); g.turn.phase = arty::game::turn::TurnPhase::Watching; }
        "banana" => {
            let mut p = Projectile::new(WorldPos::new(vp.x - 60.0, vp.y - 40.0), Vec2::new(14.0, -6.0), WeaponKind::BananaBomb);
            p.fuse = FuseState::Burning(90);
            g.projectiles.push(p);
            g.turn.phase = arty::game::turn::TurnPhase::Watching; // what firing does
        }
        "direct" => {
            // bazooka shell flying straight at the victim from the left, fired by team 0's active soldier
            let mut p = Projectile::new(WorldPos::new(vp.x - 40.0, vp.y - 6.0), Vec2::new(14.0, 0.0), WeaponKind::Bazooka);
            p.owner = Some((0, g.teams[0].active));
            g.projectiles.push(p);
            g.turn.phase = arty::game::turn::TurnPhase::Watching;
        }
        "flight" => {
            // Free flight of one projectile from a WA launch: --vel vx,vy (WA px/frame, converted to px/tick) --wind UNITS --kind bazooka|grenade
            let sarg = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
            let v: Vec<f32> = sarg("--vel").unwrap_or("20,0".into()).split(',').map(|x| x.parse().unwrap()).collect();
            let kind = match sarg("--kind").as_deref() { Some("grenade") => WeaponKind::Grenade, _ => WeaponKind::Bazooka };
            g.wind = arty::physics::Wind::from_units(sarg("--wind").and_then(|x| x.parse().ok()).unwrap_or(0));
            let start = WorldPos::new(900.0, 300.0); // open air: only the relative path is compared
            let k = 50.0 / 30.0;
            let p = Projectile::new(start, Vec2::new(v[0] * k, v[1] * k), kind);
            g.projectiles.push(p);
            g.turn.phase = arty::game::turn::TurnPhase::Watching;
            let mut t = 0;
            while t <= ticks {
                if let Some(p) = g.projectiles.first() {
                    println!("{{\"t\":{t},\"dx\":{:.3},\"dy\":{:.3},\"vx\":{:.4},\"vy\":{:.4}}}", p.pos.x - start.x, p.pos.y - start.y, p.vel.x / k, p.vel.y / k);
                } else { break; }
                server_tick(&mut g, &InputState::default(), None, None);
                t += 1;
            }
            return;
        }
        "airstrike" => {
            // Strike on the victim from the left (--left for a right-to-left plane): plane starts 1024 px out, 5 bombs.
            let dir_right = !a.iter().any(|x| x == "--left");
            let (tx, ty) = (vp.x, vp.y - 20.0);
            g.airstrike = Some(arty::game::state::AirstrikeState {
                cursor_x: tx, render_x: tx, cursor_y: ty, render_y: ty, blink_timer: 0, active: true,
                plane_x: if dir_right { tx - 1024.0 } else { tx + 1024.0 },
                plane_vx: if dir_right { 32.0 * 5.0 / 3.0 } else { -32.0 * 5.0 / 3.0 },
                bombs_dropped: 0, direction_right: dir_right, spawn_cam_left: 0.0,
            });
            g.turn.phase = arty::game::turn::TurnPhase::Watching;
            println!("target ({tx:.0},{ty:.0}) dir_right={dir_right}; camera (on the cursor, 640 px wide) spans x {:.0}..{:.0}", tx - 320.0, tx + 320.0);
        }
        "garcia" => {
            // Concrete Donkey on the victim: falls from y=-208 (WA frame), one explosion on contact. --wind UNITS
            let sarg = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
            g.wind = arty::physics::Wind::from_units(sarg("--wind").and_then(|x| x.parse().ok()).unwrap_or(0));
            g.garcia = Some(arty::game::state::GarciaState {
                cursor_x: vp.x, render_x: vp.x, cursor_y: vp.y, render_y: vp.y, blink_timer: 0, falling: true,
                fall_y: arty::world::WATER_Y as f32 - 696.0 - 208.0, vel_y: 0.0, bounce_count: 0, vel_x: 0.0,
            });
            g.turn.phase = arty::game::turn::TurnPhase::Watching;
        }
        "superbot" | "jumpbot" => {
            // Bot placed by team 0's active soldier. --keys "1:A,10-30:R,60:A,80:A" scripts the buttons (A press, L/R held over a tick range).
            let ti = g.active_team(); let si = g.teams[ti].active;
            arty::game::loop_runner::fire_bot(&mut g, ti, si, name == "superbot");
            g.turn.phase = arty::game::turn::TurnPhase::Watching;
        }
        "edgewalk" => {
            // Active worm on the floor 30 px from the left map edge (seed 0x18e9b0d64c24edd2 has an open cavern edge); hold Left with --keys "1-400:L".
            vt = g.active_team(); vs = g.teams[vt].active;
            g.teams[vt].soldiers[vs].pos = WorldPos::new(30.0, 796.0);
        }
        "edgebot" => {
            // Sheep dropped at the open left edge (seed 0x18e9b0d64c24edd2), facing left: it should walk off the map and drown, not turn around.
            let ti = g.active_team(); let si = g.teams[ti].active;
            g.teams[ti].soldiers[si].pos = WorldPos::new(30.0, 796.0);
            g.teams[ti].soldiers[si].facing = -1;
            arty::game::loop_runner::fire_bot(&mut g, ti, si, a.iter().any(|x| x == "--super"));
            g.turn.phase = arty::game::turn::TurnPhase::Watching;
        }
        "blackhole" => {
            // Black hole on the victim (--damage leaves him low so the collapse kills him); then watch what holds the turn open.
            g.teams[vt].soldiers[vs].take_damage(damage);
            let hdy = a.iter().position(|x| x == "--holedy").and_then(|i| a.get(i + 1)).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
            g.black_holes.push(arty::game::state::BlackHole { pos: WorldPos::new(vp.x, vp.y + hdy), lifetime: 150 });
            g.turn.phase = arty::game::turn::TurnPhase::Watching;
        }
        "plates" => {}
        _ => { eprintln!("unknown scenario {name}"); std::process::exit(2); }
    }

    let mut ls = LoopState::new();
    let mut buf = WorldBuffer::new();
    let mut frames: Vec<(usize, Vec<u8>)> = Vec::new();
    if !json { println!("tick  hp disp(f)  pending delay  pos           state       projectiles"); }
    for t in 0..=ticks {
        let s = &g.teams[vt].soldiers[vs];
        if json {
            println!("{{\"t\":{t},\"hp\":{},\"shown\":{:.3},\"x\":{:.2},\"y\":{:.2},\"proj\":{}}}", s.hp, s.displayed_hp_f, s.pos.x, s.pos.y, g.projectiles.len());
        } else {
        println!("{t:4}  {:3} {:3} ({:6.2})  {:3} {:3}  ({:7.1},{:7.1})  {:?}  {}",
            s.hp, s.displayed_hp, s.displayed_hp_f, s.pending_damage, s.hp_countdown_delay, s.pos.x, s.pos.y,
            std::mem::discriminant(&s.state), g.projectiles.len());
        }
        if let Some(gc) = g.garcia.as_ref() { println!("      donkey x {:.1} y {:.1} vy {:.2} vx {:.2}", gc.cursor_x, gc.fall_y, gc.vel_y, gc.vel_x); }
        if let Some(air) = g.airstrike.as_ref() {
            println!("      plane x {:.0} (view {}) dropped {}", air.plane_x, if (air.plane_x - air.cursor_x).abs() <= 320.0 { "IN" } else { "out" }, air.bombs_dropped);
        }
        if let Some(b) = g.jumpbot.as_ref() {
            println!("      bot ({:.1},{:.1}) head {:.1} flying {} phase {} grounded {} fuse {}", b.x, b.y, b.heading, b.flying, b.fly_phase, b.grounded, b.fuse_ticks);
        }
        if let Some(p) = g.projectiles.first() {
            if t % every == 0 { println!("      proj {:?} pos ({:.1},{:.1}) vel ({:.2},{:.2}) fuse {:?} age {}", p.kind, p.pos.x, p.pos.y, p.vel.x, p.vel.y, p.fuse, p.age_ticks); }
        }
        if !json && t % every == 0 {
            let fp = if name == "banana" && !g.projectiles.is_empty() { g.projectiles[0].pos } else if name == "airstrike" { g.projectiles.iter().find(|p| !p.is_fragment).map(|p| p.pos).unwrap_or(WorldPos::new(vp.x, vp.y - 20.0)) } else { s.pos };
            let cam = Camera::new(fp.x, fp.y);
            ls.tick = t as u32;
            render(&g, &mut buf, &cam, &mut ls);
            let (cx, cy) = (cam.left_edge() as usize, cam.top_edge() as usize);
            let mut crop = Vec::with_capacity(CW * CH * 3);
            // crop the middle CW x CH of the 640x480 view
            for y in 0..CH { for x in 0..CW {
                let o = ((cy + 120 + y) * WORLD_W as usize + cx + 160 + x) * 4;
                let d = buf.raw_data(); crop.extend_from_slice(&[d[o + 2], d[o + 1], d[o]]);
            }}
            frames.push((t, crop));
        }
        if name == "blackhole" && t % 25 == 0 {
            let air: Vec<_> = g.teams.iter().enumerate().flat_map(|(ti, tm)| tm.soldiers.iter().enumerate().filter(|(_, s)| matches!(s.state, arty::game::soldier::SoldierState::Airborne { .. })).map(move |(si, s)| (ti, si, s.pos.x as i32, s.pos.y as i32))).collect();
            println!("      phase {:?} holes {} pend_deaths {} expl {} proj {} airborne {:?}", g.turn.phase, g.black_holes.len(), g.pending_deaths.len(), g.explosions.len(), g.projectiles.len(), air);
        }
        let inp = scripted_input(&a, t);
        server_tick(&mut g, &inp, None, None);
    }

    if json { return; }
    let cols = 4.min(frames.len()).max(1);
    let rows = (frames.len() + cols - 1) / cols;
    let (w, h) = (cols * CW, rows * CH);
    let mut img = vec![0u8; w * h * 3];
    for (i, (_, f)) in frames.iter().enumerate() {
        let (ox, oy) = ((i % cols) * CW, (i / cols) * CH);
        for y in 0..CH { let d = ((oy + y) * w + ox) * 3; img[d..d + CW * 3].copy_from_slice(&f[y * CW * 3..(y + 1) * CW * 3]); }
    }
    let file = std::fs::File::create(&out).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgb); enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&img).unwrap();
    println!("wrote {out}: {} frames (ticks {:?})", frames.len(), frames.iter().map(|f| f.0).collect::<Vec<_>>());
}
