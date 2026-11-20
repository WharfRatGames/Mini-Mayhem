//! Throw many Holy Hand Grenades at/near worms and report any that are still moving when the age limit hits (the "never explodes" freeze).
use arty::game::loop_runner::{server_tick};
use arty::input::InputState;
use arty::physics::projectile::{FuseState, Projectile, WeaponKind};
use arty::world::{WorldPos, Vec2};

fn main() {
    let mut bad = 0;
    let mut total = 0;
    for seed in 1u64..=12 {
        for si in 0..8usize {
            for k in 0..6usize {
                let mut g = arty::replay::new_game(seed * 7919, &[0, 1]);
                let (ti, ci) = (si % 2, (si / 2) % g.teams[si % 2].soldiers.len());
                let sp = g.teams[ti].soldiers[ci].pos;
                // throw from 30-90 px away, aimed roughly at the worm, with some variety
                let dir = if k % 2 == 0 { -1.0 } else { 1.0 };
                let from = WorldPos::new(sp.x + dir * (30.0 + 12.0 * k as f32), sp.y - 20.0 - 6.0 * k as f32);
                let v = Vec2::new(-dir * (2.0 + k as f32), -3.0 + k as f32 * 0.5);
                let mut p = Projectile::new(from, v, WeaponKind::HolyHandGrenade);
                p.fuse = FuseState::Burning(90);
                p.owner = Some((1 - ti, 0));
                g.projectiles.push(p);
                g.turn.phase = arty::game::turn::TurnPhase::Watching;
                let mut armed_at = None;
                let mut end = None;
                for t in 0..800 {
                    server_tick(&mut g, &InputState::default(), None, None);
                    if armed_at.is_none() && g.projectiles.first().map(|p| matches!(p.fuse, FuseState::Detonating(_))).unwrap_or(false) { armed_at = Some(t); }
                    if g.projectiles.is_empty() { end = Some(t); break; }
                }
                total += 1;
                if end.is_none() || end.unwrap() >= 585 {
                    bad += 1;
                    let pp = g.projectiles.first().map(|p| (p.pos.x, p.pos.y, p.vel.x, p.vel.y, p.age_ticks));
                    println!("STUCK seed {seed} worm ({ti},{ci}) k={k} armed={armed_at:?} end={end:?} proj={pp:?}");
                }
            }
        }
    }
    println!("{bad} stuck / {total}");
}
