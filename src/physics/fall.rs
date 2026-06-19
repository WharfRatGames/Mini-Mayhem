

/// Safe fall distance in pixels (net downward displacement). Falls at or below this cause no damage.
pub const SAFE_FALL_PX: f32 = 80.0;

/// Damage per pixel of net downward fall beyond the safe threshold.
pub const FALL_DAMAGE_PER_PX: f32 = 0.15;

/// Tracks a single worm's fall state across physics ticks.
/// Created fresh when a worm leaves solid ground, discarded when it lands.
#[derive(Debug, Clone)]
pub struct FallTracker {
    /// Y position where the worm left solid ground (or was launched).
    pub start_y: f32,
    /// Highest point (lowest Y value) reached while airborne — fall is measured from here.
    pub apex_y: f32,
    /// Whether the worm is currently airborne.
    pub falling: bool,
}

impl FallTracker {
    pub fn new() -> Self {
        Self { start_y: 0.0, apex_y: 0.0, falling: false }
    }

    /// Call when a worm steps off solid ground or is knocked into the air.
    pub fn begin_fall(&mut self, y: f32) {
        self.start_y = y;
        self.apex_y  = y;
        self.falling = true;
    }

    /// Call each tick while airborne — tracks the highest point reached.
    pub fn update(&mut self, y: f32) {
        if self.falling {
            self.apex_y = self.apex_y.min(y);
        }
    }

    /// Call when the worm lands on solid ground.
    /// Returns damage dealt based on fall from apex to landing point.
    /// Resets the tracker back to grounded state.
    pub fn land(&mut self, land_y: f32) -> u32 {
        if !self.falling {
            return 0;
        }
        self.falling = false;
        let fall_dist = (land_y - self.apex_y).max(0.0);
        fall_damage(fall_dist)
    }

    /// Call when the worm enters water while falling.
    /// Water is instant drown — no fall damage calculated.
    pub fn drown(&mut self) {
        self.falling = false;
    }
}

impl Default for FallTracker {
    fn default() -> Self { Self::new() }
}

/// Calculate fall damage for a given fall distance in pixels.
/// 0 damage at or below SAFE_FALL_PX, then 1 damage per excess pixel.
pub fn fall_damage(fall_px: f32) -> u32 {
    if fall_px <= SAFE_FALL_PX {
        return 0;
    }
    let excess = fall_px - SAFE_FALL_PX;
    (excess * FALL_DAMAGE_PER_PX).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── fall_damage function ──────────────────────────────────────────────────

    #[test]
    fn zero_fall_deals_no_damage() {
        assert_eq!(fall_damage(0.0), 0);
    }

    #[test]
    fn fall_at_safe_threshold_deals_no_damage() {
        assert_eq!(fall_damage(SAFE_FALL_PX), 0);
    }

    #[test]
    fn fall_just_below_threshold_deals_no_damage() {
        assert_eq!(fall_damage(SAFE_FALL_PX - 0.1), 0);
    }

    #[test]
    fn fall_over_threshold_deals_damage() {
        assert!(fall_damage(SAFE_FALL_PX + 20.0) > 0);
    }

    #[test]
    fn fall_damage_scales_with_distance() {
        let small = fall_damage(SAFE_FALL_PX + 20.0);
        let large = fall_damage(SAFE_FALL_PX + 100.0);
        assert!(large > small, "larger fall should deal more damage");
    }

    #[test]
    fn large_fall_deals_proportional_damage() {
        // 200px fall: (200-80)*0.15 = 18 damage
        assert_eq!(fall_damage(200.0), 18);
        // 300px fall: (300-80)*0.15 = 33 damage
        assert_eq!(fall_damage(300.0), 33);
    }

    // ── FallTracker ───────────────────────────────────────────────────────────

    #[test]
    fn new_tracker_is_not_falling() {
        let t = FallTracker::new();
        assert!(!t.falling);
    }

    #[test]
    fn begin_fall_sets_falling_true() {
        let mut t = FallTracker::new();
        t.begin_fall(100.0);
        assert!(t.falling);
        assert_eq!(t.start_y, 100.0);
    }

    #[test]
    fn land_within_safe_distance_deals_no_damage() {
        let mut t = FallTracker::new();
        t.begin_fall(100.0);
        let dmg = t.land(100.0 + SAFE_FALL_PX);
        assert_eq!(dmg, 0);
    }

    #[test]
    fn land_exactly_at_safe_threshold_deals_no_damage() {
        let mut t = FallTracker::new();
        t.begin_fall(200.0);
        let dmg = t.land(200.0 + SAFE_FALL_PX);
        assert_eq!(dmg, 0);
    }

    #[test]
    fn land_beyond_safe_distance_deals_damage() {
        let mut t = FallTracker::new();
        t.begin_fall(100.0);
        let dmg = t.land(100.0 + SAFE_FALL_PX + 20.0); // 20px excess → 20*0.15 = 3
        assert_eq!(dmg, 3);
    }

    #[test]
    fn land_resets_falling_state() {
        let mut t = FallTracker::new();
        t.begin_fall(100.0);
        assert!(t.falling);
        t.land(200.0);
        assert!(!t.falling);
    }

    #[test]
    fn landing_when_not_falling_deals_no_damage() {
        let mut t = FallTracker::new();
        // Never called begin_fall
        let dmg = t.land(500.0);
        assert_eq!(dmg, 0);
    }

    #[test]
    fn drown_resets_falling_state() {
        let mut t = FallTracker::new();
        t.begin_fall(100.0);
        t.drown();
        assert!(!t.falling);
    }

    #[test]
    fn update_tracks_apex_not_start_y() {
        let mut t = FallTracker::new();
        t.begin_fall(300.0);
        t.update(200.0); // rising — apex_y should update
        t.update(150.0); // higher still
        assert_eq!(t.start_y, 300.0, "start_y should always be last ground Y");
        assert_eq!(t.apex_y,  150.0, "apex_y tracks the highest point reached");
    }

    #[test]
    fn high_arc_landing_at_same_height_deals_damage() {
        // Launched from y=300, apex at y=100, lands back at y=300 — 200px fall from apex
        let mut t = FallTracker::new();
        t.begin_fall(300.0);
        t.update(100.0); // apex: 200px above start
        let dmg = t.land(300.0);
        let expected = fall_damage(300.0 - 100.0); // fall from apex
        assert_eq!(dmg, expected);
        assert!(dmg > 0, "high arc should deal fall damage even landing at launch height");
    }

    #[test]
    fn small_arc_below_threshold_no_damage() {
        // Apex only 40px above landing — below SAFE_FALL_PX
        let mut t = FallTracker::new();
        t.begin_fall(300.0);
        t.update(260.0); // only 40px up
        let dmg = t.land(300.0);
        assert_eq!(dmg, 0);
    }

    #[test]
    fn explosion_knockup_then_fall_uses_apex() {
        // Blown up from y=300, apex at y=150, lands 100px lower at y=400
        // Fall = 400 - 150 = 250px
        let mut t = FallTracker::new();
        t.begin_fall(300.0);
        t.update(150.0); // apex
        let dmg = t.land(400.0);
        let expected = fall_damage(400.0 - 150.0);
        assert_eq!(dmg, expected);
    }

    #[test]
    fn multiple_falls_work_independently() {
        let mut t = FallTracker::new();

        // First fall: 50px excess → 50*0.15 = 7.5 → rounds to 8
        t.begin_fall(100.0);
        let dmg1 = t.land(100.0 + SAFE_FALL_PX + 50.0);
        assert_eq!(dmg1, 8);

        // Second fall: 10px excess → 10*0.15 = 1.5 → rounds to 2
        t.begin_fall(200.0);
        let dmg2 = t.land(200.0 + SAFE_FALL_PX + 10.0);
        assert_eq!(dmg2, 2);
    }

    #[test]
    fn safe_fall_constant_is_80_pixels() {
        assert_eq!(SAFE_FALL_PX, 80.0);
    }
}
