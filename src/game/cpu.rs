/// Easy CPU AI for singleplayer VS CPU mode.
use crate::world::{WorldPos, WORLD_W, WATER_Y};
use super::state::GameState;

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
        // TNT is locked until turn 5*team_count, same as the player input path
        // (see server/main.rs) — the CPU must obey the same weapon-cycle lock.
        let tnt_unlocked = game.turn.turn_number >= 5 * game.teams.len() as u32;
        let candidates: Vec<usize> = game.teams[cpu_team].weapons.iter().enumerate()
            .filter(|(_, (kind, ammo))| AI_USABLE_WEAPONS.contains(kind) && ammo.map_or(true, |a| a > 0)
                && (*kind != WeaponKind::Tnt || tnt_unlocked))
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
        let wind = game.wind.value() * 0.05;

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
                let power = power_frac * 20.0;
                let (lx, ly) = simulate(shoot_sx, sy, angle, power, facing, wind, &game.terrain);
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
}

fn simulate(sx: f32, sy: f32, angle: f32, power: f32, facing: f32,
            wind: f32, terrain: &crate::world::Terrain) -> (f32, f32) {
    let mut x  = sx + angle.cos() * facing * 12.0;
    let mut y  = sy - angle.sin() * 12.0;
    let mut vx = angle.cos() * power * facing / 5.0;
    let mut vy = -angle.sin() * power / 5.0;
    for _ in 0..300 {
        vx += wind;
        vy = (vy + 0.5).min(12.0);
        x += vx; y += vy;
        if x < 0.0 || x >= WORLD_W as f32 { break; }
        if y >= WATER_Y as f32 { break; }
        if terrain.is_solid(x as i32, y as i32) { return (x, y); }
    }
    (x, y)
}

fn lcg(s: u32) -> u32 {
    s.wrapping_mul(1664525).wrapping_add(1013904223)
}
