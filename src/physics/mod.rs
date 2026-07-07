#![allow(unused_imports)]
pub mod projectile;
pub mod tick;
pub mod collision;
pub mod outcome;
pub mod fall;
pub mod step;
pub mod wind;

pub use projectile::{Projectile, WeaponKind, FuseState};
pub use tick::{tick, tick_n, GRAVITY, TERMINAL_VELOCITY, WIND_SCALE};
pub use collision::{swept_collision, CollisionResult};
pub use outcome::{resolve, Outcome};
pub use fall::{FallTracker, fall_damage, SAFE_FALL_PX, FALL_DAMAGE_PER_PX};
pub use step::{step_projectile, StepResult};
pub use wind::{Wind, WIND_MAX, WIND_ARROWS};

/// Returns true if the straight line from (x0,y0) to (x1,y1) crosses no
/// blocked pixels (solid terrain or object mask) in the terrain.
pub fn line_clear(x0: i32, y0: i32, x1: i32, y1: i32, terrain: &crate::world::Terrain) -> bool {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let steps = dx.max(dy).max(1);
    for i in 0..=steps {
        let x = x0 + (x1 - x0) * i / steps;
        let y = y0 + (y1 - y0) * i / steps;
        if terrain.is_blocked(x, y) { return false; }
    }
    true
}
