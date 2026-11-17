use crate::world::Vec2;

/// Wind is stored as a value in [-1.0, 1.0].
/// Positive = rightward, negative = leftward.
/// Applied to projectiles each tick via WIND_SCALE in tick.rs.
///
/// Wind is fully re-randomised each turn — no memory of previous value.
/// Any value in [-1.0, 1.0] is equally likely.

/// WA's wind range in whole units; wind acceleration = units / 35 px/frame^2 (50 fps).
pub const WA_WIND_UNITS: i32 = 10;

/// Maximum wind strength. 1.0 = full scale.
pub const WIND_MAX: f32 = 1.0;

/// Wind strength below which we display "calm".
pub const WIND_CALM_THRESHOLD: f32 = 0.05;

/// Number of arrow segments shown on the HUD wind indicator.
/// Each segment represents 1/WIND_ARROWS of full wind strength.
pub const WIND_ARROWS: u32 = 5;

/// The current wind state.
#[derive(Debug, Clone, Copy)]
pub struct Wind {
    /// Current strength in [-1.0, 1.0].
    value: f32,
}

impl Wind {
    /// Create wind with a specific value. Clamps to [-1.0, 1.0].
    pub fn new(value: f32) -> Self {
        Self { value: value.clamp(-WIND_MAX, WIND_MAX) }
    }

    /// Start with no wind.
    pub fn calm() -> Self {
        Self { value: 0.0 }
    }

    /// Current wind value in [-1.0, 1.0].
    pub fn value(self) -> f32 {
        self.value
    }

    /// Generate a completely new random wind value for the next turn.
    /// No memory of the previous value — any value in [-1.0, 1.0] equally likely.
    /// `rng_val` should be a random f32 in [0.0, 1.0) from the caller.
    pub fn next_turn(rng_val: f32) -> Self {
        // Map [0, 1) → [-1.0, 1.0]
        let value = rng_val * 2.0 - 1.0;
        Self { value: value.clamp(-WIND_MAX, WIND_MAX) }
    }

    /// Wind in whole WA "wind units" (-10..=10); the stored value is units / 10.
    pub fn units(self) -> i32 {
        (self.value * WA_WIND_UNITS as f32).round() as i32
    }

    /// Build wind from whole WA units, clamped to -10..=10.
    pub fn from_units(w: i32) -> Self {
        let w = w.clamp(-WA_WIND_UNITS, WA_WIND_UNITS);
        Self { value: w as f32 / WA_WIND_UNITS as f32 }
    }

    /// WA's per-turn wind roll (decompiled 0x55bc80): an integer in -10..=10
    /// (`rng % 21 - 10`), re-rolled (up to 10 tries, keeping the last) while the new
    /// wind is within 0xccc (0.05 px/frame^2, i.e. one unit) of the previous wind.
    /// The scheme's centre-bias term is 0 here. `turn` and `seed` make the roll
    /// deterministic so server and clients agree.
    pub fn next_turn_wa(prev: Wind, turn: u32, seed: u32) -> Self {
        let per_unit = |w: i32| (w << 16) / 35; // 16.16 px/frame^2, C-style truncation
        let prev_v = per_unit(prev.units());
        let mut state = turn.wrapping_mul(2654435761).wrapping_add(seed);
        let mut w = 0i32;
        let mut tries = 10;
        loop {
            state = state.wrapping_mul(0x0019_660d).wrapping_add(0x3c6e_f35f);
            let byte = (state >> 16) & 0xff;
            w = ((byte % 21) as i32 - 10).clamp(-WA_WIND_UNITS, WA_WIND_UNITS);
            tries -= 1;
            if tries == 0 || (per_unit(w) - prev_v).abs() >= 0xccc { break; }
        }
        Self::from_units(w)
    }

    /// Returns true if wind is effectively calm.
    pub fn is_calm(self) -> bool {
        self.value.abs() < WIND_CALM_THRESHOLD
    }

    /// Direction as a unit vector (for applying to physics).
    /// Y is always 0 — wind is purely horizontal.
    pub fn as_vec2(self) -> Vec2 {
        Vec2::new(self.value, 0.0)
    }

    /// How many HUD arrows to display and in which direction.
    /// Returns (count, rightward). Count is 0..=WIND_ARROWS.
    pub fn hud_arrows(self) -> (u32, bool) {
        let rightward = self.value >= 0.0;
        let count = (self.value.abs() * WIND_ARROWS as f32).round() as u32;
        let count = count.min(WIND_ARROWS);
        (count, rightward)
    }

    /// A simple text label for the wind strength.
    pub fn label(self) -> &'static str {
        let abs = self.value.abs();
        if abs < WIND_CALM_THRESHOLD { return "CALM"; }
        if abs < 0.33               { return "LIGHT"; }
        if abs < 0.66               { return "MODERATE"; }
        "STRONG"
    }
}

impl Default for Wind {
    fn default() -> Self { Self::calm() }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Construction ──────────────────────────────────────────────────────────

    #[test]
    fn calm_wind_has_zero_value() {
        assert_eq!(Wind::calm().value(), 0.0);
    }

    #[test]
    fn new_wind_clamps_above_max() {
        let w = Wind::new(2.0);
        assert_eq!(w.value(), WIND_MAX);
    }

    #[test]
    fn new_wind_clamps_below_min() {
        let w = Wind::new(-2.0);
        assert_eq!(w.value(), -WIND_MAX);
    }

    #[test]
    fn new_wind_stores_value_in_range() {
        let w = Wind::new(0.5);
        assert!((w.value() - 0.5).abs() < 1e-5);
    }

    // ── next_turn ─────────────────────────────────────────────────────────────

    #[test]
    fn next_turn_stays_within_max_bounds() {
        for i in 0..=20 {
            let rng = i as f32 / 20.0;
            let next = Wind::next_turn(rng);
            assert!(
                next.value() >= -WIND_MAX && next.value() <= WIND_MAX,
                "wind {} out of bounds", next.value()
            );
        }
    }

    #[test]
    fn next_turn_at_zero_rng_is_full_left() {
        let next = Wind::next_turn(0.0);
        assert!((next.value() - (-1.0)).abs() < 1e-5,
            "rng=0 should give full leftward wind -1.0, got {}", next.value());
    }

    #[test]
    fn next_turn_at_one_rng_is_full_right() {
        let next = Wind::next_turn(1.0);
        assert!((next.value() - 1.0).abs() < 1e-5,
            "rng=1 should give full rightward wind 1.0, got {}", next.value());
    }

    #[test]
    fn next_turn_at_half_rng_is_calm() {
        let next = Wind::next_turn(0.5);
        assert!(next.value().abs() < 1e-5,
            "rng=0.5 should give calm wind 0.0, got {}", next.value());
    }

    #[test]
    fn next_turn_can_instantly_reverse() {
        // Pure random — full reversal in one turn is expected
        let full_left  = Wind::next_turn(0.0);
        let full_right = Wind::next_turn(1.0);
        assert!(full_left.value() < 0.0);
        assert!(full_right.value() > 0.0);
    }

    #[test]
    fn next_turn_is_independent_of_previous_wind() {
        // Same rng_val always gives same result regardless of current wind
        let a = Wind::next_turn(0.25);
        let b = Wind::next_turn(0.25);
        assert!((a.value() - b.value()).abs() < 1e-5,
            "same rng should always produce same wind");
    }

    // ── is_calm ───────────────────────────────────────────────────────────────

    #[test]
    fn zero_wind_is_calm() {
        assert!(Wind::calm().is_calm());
    }

    #[test]
    fn small_wind_is_calm() {
        assert!(Wind::new(WIND_CALM_THRESHOLD - 0.01).is_calm());
    }

    #[test]
    fn moderate_wind_is_not_calm() {
        assert!(!Wind::new(0.5).is_calm());
    }

    // ── as_vec2 ───────────────────────────────────────────────────────────────

    #[test]
    fn positive_wind_gives_rightward_vec2() {
        let v = Wind::new(0.7).as_vec2();
        assert!(v.x > 0.0);
        assert_eq!(v.y, 0.0);
    }

    #[test]
    fn negative_wind_gives_leftward_vec2() {
        let v = Wind::new(-0.7).as_vec2();
        assert!(v.x < 0.0);
        assert_eq!(v.y, 0.0);
    }

    #[test]
    fn calm_wind_gives_zero_vec2() {
        let v = Wind::calm().as_vec2();
        assert_eq!(v.x, 0.0);
        assert_eq!(v.y, 0.0);
    }

    // ── hud_arrows ────────────────────────────────────────────────────────────

    #[test]
    fn calm_wind_shows_zero_arrows() {
        let (count, _) = Wind::calm().hud_arrows();
        assert_eq!(count, 0);
    }

    #[test]
    fn full_rightward_wind_shows_max_arrows_rightward() {
        let (count, rightward) = Wind::new(1.0).hud_arrows();
        assert_eq!(count, WIND_ARROWS);
        assert!(rightward);
    }

    #[test]
    fn full_leftward_wind_shows_max_arrows_leftward() {
        let (count, rightward) = Wind::new(-1.0).hud_arrows();
        assert_eq!(count, WIND_ARROWS);
        assert!(!rightward);
    }

    #[test]
    fn half_wind_shows_roughly_half_arrows() {
        let (count, _) = Wind::new(0.5).hud_arrows();
        // 0.5 * 5 = 2.5 → rounds to 3 (or 2 — either is fine within 1)
        assert!(count >= 2 && count <= 3, "half wind should show 2-3 arrows, got {count}");
    }

    #[test]
    fn arrow_count_never_exceeds_max() {
        for i in -10..=10 {
            let v = i as f32 * 0.15;
            let (count, _) = Wind::new(v).hud_arrows();
            assert!(count <= WIND_ARROWS, "arrow count {count} exceeded max {WIND_ARROWS}");
        }
    }

    // ── label ────────────────────────────────────────────────────────────────

    #[test]
    fn calm_label() {
        assert_eq!(Wind::calm().label(), "CALM");
    }

    #[test]
    fn strong_label_at_full_wind() {
        assert_eq!(Wind::new(1.0).label(), "STRONG");
        assert_eq!(Wind::new(-1.0).label(), "STRONG");
    }

    #[test]
    fn light_label_at_low_wind() {
        assert_eq!(Wind::new(0.2).label(), "LIGHT");
        assert_eq!(Wind::new(-0.2).label(), "LIGHT");
    }

    #[test]
    fn moderate_label_at_mid_wind() {
        assert_eq!(Wind::new(0.5).label(), "MODERATE");
        assert_eq!(Wind::new(-0.5).label(), "MODERATE");
    }

    // ── Integration with physics ──────────────────────────────────────────────

    #[test]
    fn wind_value_passed_to_step_affects_trajectory() {
        use crate::world::{WorldPos, Terrain};
        use crate::physics::{Projectile, step_projectile};

        let t = Terrain::empty();
        let mut p_right = Projectile::new(
            WorldPos::new(100.0, 100.0),
            Vec2::new(5.0, -8.0),
            crate::physics::WeaponKind::Bazooka,
        );
        let mut p_left = Projectile::new(
            WorldPos::new(100.0, 100.0),
            Vec2::new(5.0, -8.0),
            crate::physics::WeaponKind::Bazooka,
        );

        let wind_right = Wind::new(1.0);
        let wind_left  = Wind::new(-1.0);

        for _ in 0..30 {
            step_projectile(&mut p_right, &t, wind_right.value());
            step_projectile(&mut p_left,  &t, wind_left.value());
        }

        assert!(
            p_right.pos.x > p_left.pos.x,
            "rightward wind should push projectile further right than leftward wind"
        );
    }

    #[test]
    fn wa_wind_is_whole_units_in_range_and_rerolls_near_previous() {
        let mut prev = Wind::calm();
        for turn in 0..500u32 {
            let next = Wind::next_turn_wa(prev, turn, 12345);
            let u = next.units();
            assert!((-10..=10).contains(&u), "units {u}");
            assert!((next.value() * 10.0 - u as f32).abs() < 1e-5, "not whole units");
            prev = next;
        }
    }

    #[test]
    fn wa_wind_rarely_repeats_within_one_unit() {
        let mut prev = Wind::calm();
        let mut near = 0;
        for turn in 0..1000u32 {
            let next = Wind::next_turn_wa(prev, turn, 777);
            if (next.units() - prev.units()).abs() <= 1 { near += 1; }
            prev = next;
        }
        // 10 re-rolls make a "within one unit" result very unlikely (~(3/21)^10 per turn).
        assert!(near < 5, "too many near-repeat winds: {near}");
    }
}
