/// Easy CPU AI for singleplayer VS CPU mode.
use crate::world::{WorldPos, WORLD_W, WATER_Y};
use super::state::GameState;
use super::team::Difficulty;

pub struct CpuState {
    pub walk_dir:   i8,   // -1=walk left, 0=stand, +1=walk right
    pub walk_ticks: u32,  // ticks remaining to walk before firing
    pub thinking:   u32,  // pause ticks after walking, before firing
    pub angle:      f32,
    pub power:      f32,
    pub weapon_idx: usize, // index into team.weapons to fire with
    pub decided:    bool,
}

/// Weapon kinds the CPU's ballistic aim/simulate logic can usefully fire.
use crate::physics::projectile::WeaponKind;
const AI_USABLE_WEAPONS: &[WeaponKind] = &[
    WeaponKind::Bazooka,
    WeaponKind::Grenade,
    WeaponKind::Shotgun,
    WeaponKind::Tnt,
    WeaponKind::BananaBomb,
    WeaponKind::Blasthive,
    WeaponKind::BlackHoleBomb,
    WeaponKind::MolotovCocktail,
];

/// Wider weapon pool used by Medium/Hard sim-only difficulty tiers — adds
/// projectile-arc weapons the same ballistic grid-search can still evaluate.
const AI_USABLE_WEAPONS_HARD: &[WeaponKind] = &[
    WeaponKind::Bazooka,
    WeaponKind::Grenade,
    WeaponKind::Shotgun,
    WeaponKind::Tnt,
    WeaponKind::BananaBomb,
    WeaponKind::Blasthive,
    WeaponKind::BlackHoleBomb,
    WeaponKind::MolotovCocktail,
    WeaponKind::ClusterBomb,
    WeaponKind::Revolver,
    WeaponKind::Uzi,
    WeaponKind::Pistol,
];

impl CpuState {
    pub fn undecided() -> Self {
        Self { walk_dir: 0, walk_ticks: 0, thinking: 0, angle: 0.0, power: 0.5, weapon_idx: 0, decided: false }
    }

    /// Decide movement + shot for this turn.
    pub fn decide(game: &GameState, cpu_team: usize, noise_seed: u32) -> Self {
        let s = &game.teams[cpu_team].soldiers[game.teams[cpu_team].active];
        let sx = s.pos.x;
        let sy = s.pos.y - 4.0;
        let facing = s.facing as f32;

        // Find closest living enemy
        let mut best_pos: Option<WorldPos> = None;
        let mut best_dist = f32::MAX;
        for (ti, team) in game.teams.iter().enumerate() {
            if ti == cpu_team { continue; }
            for sol in &team.soldiers {
                if !sol.is_alive() { continue; }
                let d = (sol.pos.x - sx).abs() + (sol.pos.y - sy).abs();
                if d < best_dist { best_dist = d; best_pos = Some(sol.pos); }
            }
        }
        let target = match best_pos {
            Some(p) => p,
            None => return Self { walk_dir: 0, walk_ticks: 0, thinking: 30,
                                   angle: std::f32::consts::FRAC_PI_4, power: 0.6, weapon_idx: 0, decided: true },
        };

        // Pick a random usable weapon with ammo remaining (infinite = None).
        let candidates: Vec<usize> = game.teams[cpu_team].weapons.iter().enumerate()
            .filter(|(_, (kind, ammo))| AI_USABLE_WEAPONS.contains(kind) && ammo.map_or(true, |a| a > 0))
            .map(|(i, _)| i)
            .collect();
        let weapon_idx = if candidates.is_empty() { 0 } else {
            candidates[(lcg(noise_seed + 6) as usize) % candidates.len()]
        };

        let r1 = lcg(noise_seed)     as f32 / u32::MAX as f32;
        let r2 = lcg(noise_seed + 1) as f32 / u32::MAX as f32;
        let r3 = lcg(noise_seed + 2) as f32 / u32::MAX as f32;
        let r4 = lcg(noise_seed + 3) as f32 / u32::MAX as f32;

        // 70% chance to walk toward target
        let walk_dir;
        let walk_ticks;
        if r1 < 0.70 {
            let dir = if target.x > sx { 1i8 } else { -1i8 };
            walk_dir   = dir;
            walk_ticks = (lcg(noise_seed + 4) % 30 + 10) as u32; // 10-39 ticks = 20-78px
        } else {
            walk_dir   = 0;
            walk_ticks = 0;
        }

        // Find best shot from current (or approx moved) position
        let shoot_sx = sx + walk_dir as f32 * walk_ticks as f32 * 2.0; // rough position estimate
        let mut best_angle = std::f32::consts::FRAC_PI_4;
        let mut best_power = 0.6f32;
        let mut best_miss  = f32::MAX;

        // LOS penalty: if terrain completely blocks the direct path, penalise miss distance.
        // CPU still fires but prefers angles with a clearer trajectory.
        let los_blocked = !crate::physics::line_clear(
            shoot_sx as i32, sy as i32, target.x as i32, target.y as i32, &game.terrain,
        );
        let los_penalty = if los_blocked { 80.0f32 } else { 0.0 };

        for ai in -75i32..=75 {
            let angle = ai as f32 * std::f32::consts::PI / 180.0;
            for pi in [35u32, 50, 65, 80, 95] {
                let power_frac = pi as f32 / 100.0;
                let kind = game.teams[cpu_team].weapons.get(weapon_idx).map(|w| w.0).unwrap_or(WeaponKind::Bazooka);
                let (lx, ly) = simulate_shot(kind, shoot_sx, sy, angle, power_frac, facing, game.wind.value(), &game.terrain);
                let mut miss = ((lx - target.x).powi(2) + (ly - target.y).powi(2)).sqrt();
                // Apply LOS penalty once — all angles share the same penalty here
                // (keeps penalty consistent across the search)
                miss += los_penalty;
                if miss < best_miss {
                    best_miss  = miss;
                    best_angle = angle;
                    best_power = power_frac;
                }
            }
        }

        // Easy difficulty inaccuracy
        let angle_err = (r2 - 0.5) * 0.22;
        let power_err = (r3 - 0.5) * 0.16;
        let chaos = r4 < 0.15;
        let final_angle = best_angle + angle_err + if chaos { (r2 - 0.5) * 0.40 } else { 0.0 };
        let final_power = (best_power + power_err).clamp(0.15, 1.0);

        let thinking = lcg(noise_seed + 5) % 50 + 30; // 30-79 ticks thinking

        Self {
            walk_dir, walk_ticks,
            thinking,
            angle: final_angle,
            power: final_power,
            weapon_idx,
            decided: true,
        }
    }

    /// Sim-only variant of `decide` with tiered strength. Used exclusively by
    /// `match-sim` — never called from the shipped desktop VS-CPU path, which
    /// keeps calling `decide` (fixed Easy) unchanged.
    pub fn decide_with_difficulty(game: &GameState, cpu_team: usize, noise_seed: u32, difficulty: Difficulty) -> Self {
        let s = &game.teams[cpu_team].soldiers[game.teams[cpu_team].active];
        let sx = s.pos.x;
        let sy = s.pos.y - 4.0;
        let facing = s.facing as f32;

        // Find target: nearest enemy on Easy, lowest-HP enemy on Medium/Hard.
        let mut best_pos: Option<WorldPos> = None;
        let mut best_score = f32::MAX;
        for (ti, team) in game.teams.iter().enumerate() {
            if ti == cpu_team { continue; }
            for sol in &team.soldiers {
                if !sol.is_alive() { continue; }
                let d = (sol.pos.x - sx).abs() + (sol.pos.y - sy).abs();
                let score = match difficulty {
                    Difficulty::Easy => d,
                    // Prefer low-HP kills, but don't ignore distance entirely.
                    Difficulty::Medium | Difficulty::Hard => sol.hp as f32 * 4.0 + d,
                };
                if score < best_score { best_score = score; best_pos = Some(sol.pos); }
            }
        }
        let target = match best_pos {
            Some(p) => p,
            None => return Self { walk_dir: 0, walk_ticks: 0, thinking: 30,
                                   angle: std::f32::consts::FRAC_PI_4, power: 0.6, weapon_idx: 0, decided: true },
        };

        let pool: &[WeaponKind] = match difficulty {
            Difficulty::Easy => AI_USABLE_WEAPONS,
            Difficulty::Medium | Difficulty::Hard => AI_USABLE_WEAPONS_HARD,
        };
        let candidates: Vec<usize> = game.teams[cpu_team].weapons.iter().enumerate()
            .filter(|(_, (kind, ammo))| pool.contains(kind) && ammo.map_or(true, |a| a > 0))
            .map(|(i, _)| i)
            .collect();
        let weapon_idx = if candidates.is_empty() { 0 } else {
            candidates[(lcg(noise_seed + 6) as usize) % candidates.len()]
        };

        let r1 = lcg(noise_seed)     as f32 / u32::MAX as f32;
        let r2 = lcg(noise_seed + 1) as f32 / u32::MAX as f32;
        let r3 = lcg(noise_seed + 2) as f32 / u32::MAX as f32;
        let r4 = lcg(noise_seed + 3) as f32 / u32::MAX as f32;

        let walk_dir;
        let walk_ticks;
        if r1 < 0.70 {
            let dir = if target.x > sx { 1i8 } else { -1i8 };
            walk_dir   = dir;
            walk_ticks = (lcg(noise_seed + 4) % 30 + 10) as u32;
        } else {
            walk_dir   = 0;
            walk_ticks = 0;
        }

        let shoot_sx = sx + walk_dir as f32 * walk_ticks as f32 * 2.0;
        let mut best_angle = std::f32::consts::FRAC_PI_4;
        let mut best_power = 0.6f32;
        let mut best_miss  = f32::MAX;

        let los_blocked = !crate::physics::line_clear(
            shoot_sx as i32, sy as i32, target.x as i32, target.y as i32, &game.terrain,
        );
        let los_penalty = if los_blocked { 80.0f32 } else { 0.0 };

        for ai in -75i32..=75 {
            let angle = ai as f32 * std::f32::consts::PI / 180.0;
            for pi in [35u32, 50, 65, 80, 95] {
                let power_frac = pi as f32 / 100.0;
                let kind = game.teams[cpu_team].weapons.get(weapon_idx).map(|w| w.0).unwrap_or(WeaponKind::Bazooka);
                let (lx, ly) = simulate_shot(kind, shoot_sx, sy, angle, power_frac, facing, game.wind.value(), &game.terrain);
                let mut miss = ((lx - target.x).powi(2) + (ly - target.y).powi(2)).sqrt();
                miss += los_penalty;
                if miss < best_miss {
                    best_miss  = miss;
                    best_angle = angle;
                    best_power = power_frac;
                }
            }
        }

        // Inaccuracy shrinks as difficulty increases; Hard is near-perfect.
        let (angle_scale, power_scale, chaos_chance) = match difficulty {
            Difficulty::Easy   => (1.0, 1.0, 0.15),
            Difficulty::Medium => (0.45, 0.45, 0.05),
            Difficulty::Hard   => (0.10, 0.10, 0.0),
        };
        let angle_err = (r2 - 0.5) * 0.22 * angle_scale;
        let power_err = (r3 - 0.5) * 0.16 * power_scale;
        let chaos = r4 < chaos_chance;
        let final_angle = best_angle + angle_err + if chaos { (r2 - 0.5) * 0.40 } else { 0.0 };
        let final_power = (best_power + power_err).clamp(0.15, 1.0);

        let thinking = lcg(noise_seed + 5) % 50 + 30;

        Self {
            walk_dir, walk_ticks,
            thinking,
            angle: final_angle,
            power: final_power,
            weapon_idx,
            decided: true,
        }
    }
}

/// Where a shot will land, using the game's REAL projectile physics: same muzzle position, launch speed,
/// gravity, wind and collision as `fire_weapon` + `step_projectile` (so every game mode's CPU aims with the
/// exact rules the shell will fly by, including future changes to them).
fn simulate_shot(kind: WeaponKind, sx: f32, sy: f32, angle: f32, power_frac: f32, facing: f32,
                 wind: f32, terrain: &crate::world::Terrain) -> (f32, f32) {
    use crate::physics::{Projectile, step_projectile, StepResult};
    use crate::physics::tick::{launch_speed, muzzle_spawn};
    use crate::world::Vec2;
    let speed = launch_speed(kind, power_frac);
    let mut p = Projectile::new(
        muzzle_spawn(sx, sy, angle, facing),
        Vec2::new(angle.cos() * speed * facing, -angle.sin() * speed),
        kind,
    );
    let w = if kind.affected_by_wind() { wind } else { 0.0 };
    for _ in 0..900 {
        match step_projectile(&mut p, terrain, w) {
            StepResult::Flying | StepResult::Bounced => {
                if p.pos.x < -(WORLD_W as f32) || p.pos.x > 2.0 * WORLD_W as f32 || p.pos.y < -600.0 { break; }
            }
            StepResult::Explode(e) | StepResult::FuseExplode(e) => return (e.x, e.y),
            _ => break,
        }
    }
    (p.pos.x, p.pos.y)
}

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(1664525).wrapping_add(1013904223)
}

#[cfg(test)]
mod shot_sim_tests {
    use super::*;
    use crate::physics::{step_projectile, Projectile, StepResult};

    /// The CPU's predicted landing must equal where a really-fired shell explodes (same code path).
    #[test]
    fn cpu_prediction_matches_real_flight() {
        let mut t = crate::world::Terrain::empty();
        for y in 600..crate::world::WORLD_H as i32 { for x in 0..crate::world::WORLD_W as i32 { t.set_solid(x, y, true); } }
        for &(kind, wind) in &[(WeaponKind::Bazooka, 0.3f32), (WeaponKind::Bazooka, -0.7), (WeaponKind::Grenade, 0.0)] {
            let (sx, sy, angle, charge, facing) = (300.0, 590.0, 0.9f32, 0.4f32, 1.0f32);
            let (px, py) = simulate_shot(kind, sx, sy, angle, charge, facing, wind, &t);
            let sp = crate::physics::tick::launch_speed(kind, charge);
            let mut p = Projectile::new(crate::physics::tick::muzzle_spawn(sx, sy, angle, facing),
                crate::world::Vec2::new(angle.cos() * sp * facing, -angle.sin() * sp), kind);
            let w = if kind.affected_by_wind() { wind } else { 0.0 };
            let mut got = None;
            for _ in 0..900 { let r = step_projectile(&mut p, &t, w); if let StepResult::Explode(e) | StepResult::FuseExplode(e) = r { got = Some(e); break; } if !matches!(r, StepResult::Flying | StepResult::Bounced) { eprintln!("{:?} ended {:?} at {:?}", kind, r, p.pos); break; } }
            let e = got.expect("real shell exploded");
            assert_eq!((px, py), (e.x, e.y));
        }
    }
}

#[cfg(test)]
mod shot_sim_timing {
    use super::*;
    #[test]
    fn full_aim_search_time() {
        let mut t = crate::world::Terrain::empty();
        for y in 600..crate::world::WORLD_H as i32 { for x in 0..crate::world::WORLD_W as i32 { t.set_solid(x, y, true); } }
        let t0 = std::time::Instant::now();
        for kind in [WeaponKind::Bazooka, WeaponKind::Grenade] {
            for ai in -75i32..=75 { for pi in [35u32, 50, 65, 80, 95] {
                simulate_shot(kind, 300.0, 590.0, ai as f32 * 0.01745, pi as f32 / 100.0, 1.0, 0.2, &t);
            } }
        }
        eprintln!("aim search (bazooka+grenade, 755 sims each): {:?}", t0.elapsed());
    }
}
