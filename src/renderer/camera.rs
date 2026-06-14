use crate::world::{WORLD_W, SCREEN_W, SCREEN_H, WorldPos};

/// How quickly the camera lerps toward its target. 1.0 = instant.
/// 0.12 gives a smooth but responsive follow feel.
pub const CAM_LERP: f32 = 0.12;

/// How many pixels per tick the camera pans when R+dpad is held.
pub const CAM_PAN_SPEED: f32 = 8.0;

/// Camera state.
/// `x` is the left edge of the 640px viewport in world pixels.
/// Ranges from 0 to WORLD_W - SCREEN_W (0 to 2560).
#[derive(Debug, Clone)]
pub struct Camera {
    /// Current viewport left edge (fractional for smooth lerp).
    x: f32,
    /// Target the camera is lerping toward.
    target_x: f32,
    /// Whether the player is currently free-panning (R held).
    pub panning: bool,
}

impl Camera {
    /// Create a camera centred on a world x position.
    pub fn new(world_x: f32) -> Self {
        let x = centred_on(world_x);
        Self { x, target_x: x, panning: false }
    }

    /// Current left edge of the viewport as an integer pixel offset.
    /// Use this to blit the correct slice of the world buffer.
    pub fn left_edge(&self) -> u32 {
        (self.x as u32).min(max_cam_x())
    }

    /// Current fractional left edge — for sub-pixel smooth rendering.
    pub fn left_edge_f32(&self) -> f32 {
        self.x
    }

    /// Set the follow target to centre on a world position.
    /// Only applies when not panning.
    pub fn follow(&mut self, pos: WorldPos) {
        if !self.panning {
            self.target_x = centred_on(pos.x);
        }
    }

    /// Pan the camera by dx pixels per tick (called when R+dpad held).
    pub fn pan(&mut self, dx: f32) {
        self.panning = true;
        self.target_x = (self.target_x + dx).clamp(0.0, max_cam_x() as f32);
        // Snap immediately while panning — no lerp lag
        self.x = self.target_x;
    }

    /// Release pan mode. Camera will lerp back to following the active entity.
    pub fn release_pan(&mut self) {
        self.panning = false;
    }

    /// Advance the camera by one tick — lerps toward target.
    pub fn tick(&mut self) {
        let diff = self.target_x - self.x;
        self.x += diff * CAM_LERP;
        self.x = self.x.clamp(0.0, max_cam_x() as f32);
    }

    /// Instantly snap to a world position with no lerp.
    /// Also clears pan mode so follow() resumes normally.
    pub fn snap_to(&mut self, pos: WorldPos) {
        self.x = centred_on(pos.x);
        self.target_x = self.x;
        self.panning = false;
    }

    /// Follow a position ignoring pan mode — used for projectiles so they
    /// are always tracked regardless of L1 free-cam state.
    pub fn follow_always(&mut self, pos: WorldPos) {
        self.target_x = centred_on(pos.x);
    }

    /// Convert a world x coordinate to a screen x coordinate.
    /// Returns None if the position is outside the current viewport.
    pub fn world_to_screen_x(&self, world_x: f32) -> Option<i32> {
        let sx = (world_x - self.x).round() as i32;
        if sx >= 0 && sx < SCREEN_W as i32 {
            Some(sx)
        } else {
            None
        }
    }

    /// Convert a world y coordinate to a screen y coordinate.
    /// Y is not scrolled — world and screen Y are the same.
    pub fn world_to_screen_y(world_y: f32) -> i32 {
        world_y.round() as i32
    }

    /// Convert a world position to screen coordinates.
    /// Returns None if outside the viewport.
    pub fn world_to_screen(&self, pos: WorldPos) -> Option<(i32, i32)> {
        let sx = self.world_to_screen_x(pos.x)?;
        let sy = Self::world_to_screen_y(pos.y);
        if sy >= 0 && sy < SCREEN_H as i32 {
            Some((sx, sy))
        } else {
            None
        }
    }

    /// How far (in pixels) the camera is from its target. Useful for
    /// deciding whether to skip a lerp frame.
    pub fn lag(&self) -> f32 {
        (self.target_x - self.x).abs()
    }

    /// True if the camera has essentially reached its target.
    pub fn is_settled(&self) -> bool {
        self.lag() < 0.5
    }
}

/// The maximum valid camera left edge — keeps the right side of the
/// viewport from exceeding the world width.
pub fn max_cam_x() -> u32 {
    WORLD_W.saturating_sub(SCREEN_W)
}

/// Calculate the camera left edge that centres the viewport on world_x.
fn centred_on(world_x: f32) -> f32 {
    let half = SCREEN_W as f32 / 2.0;
    (world_x - half).clamp(0.0, max_cam_x() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam_at(x: f32) -> Camera { Camera::new(x) }
    fn pos(x: f32, y: f32) -> WorldPos { WorldPos::new(x, y) }

    // ── Construction and clamping ─────────────────────────────────────────────

    #[test]
    fn camera_starts_centred_on_position() {
        let c = cam_at(1600.0);
        // Centred on 1600: left edge = 1600 - 320 = 1280
        assert_eq!(c.left_edge(), 1280);
    }

    #[test]
    fn camera_at_world_start_clamps_to_zero() {
        let c = cam_at(0.0);
        assert_eq!(c.left_edge(), 0);
    }

    #[test]
    fn camera_at_world_end_clamps_to_max() {
        let c = cam_at(WORLD_W as f32);
        assert_eq!(c.left_edge(), max_cam_x());
    }

    #[test]
    fn max_cam_x_is_correct() {
        assert_eq!(max_cam_x(), WORLD_W - SCREEN_W);
        assert_eq!(max_cam_x(), 2560);
    }

    // ── Follow ────────────────────────────────────────────────────────────────

    #[test]
    fn follow_sets_target_to_centre_on_pos() {
        let mut c = cam_at(0.0);
        c.follow(pos(1600.0, 100.0));
        assert!((c.target_x - 1280.0).abs() < 1.0);
    }

    #[test]
    fn follow_does_nothing_while_panning() {
        let mut c = cam_at(0.0);
        c.pan(500.0);
        let x_before = c.x;
        c.follow(pos(3000.0, 100.0));
        assert_eq!(c.x, x_before, "follow should not move camera while panning");
    }

    #[test]
    fn follow_resumes_after_pan_released() {
        let mut c = cam_at(100.0);
        c.pan(200.0);
        c.release_pan();
        assert!(!c.panning);
        c.follow(pos(1600.0, 100.0));
        // After release, target should update
        assert!((c.target_x - 1280.0).abs() < 1.0);
    }

    // ── Tick / lerp ───────────────────────────────────────────────────────────

    #[test]
    fn tick_moves_camera_toward_target() {
        let mut c = cam_at(0.0);
        c.follow(pos(1600.0, 100.0)); // target = 1280
        let x_before = c.x;
        c.tick();
        assert!(c.x > x_before, "camera should move toward target");
    }

    #[test]
    fn camera_settles_after_many_ticks() {
        let mut c = cam_at(0.0);
        c.follow(pos(1600.0, 100.0));
        for _ in 0..200 {
            c.tick();
        }
        assert!(c.is_settled(), "camera should settle after 200 ticks");
        assert!((c.left_edge() as i32 - 1280).abs() <= 1);
    }

    #[test]
    fn camera_does_not_overshoot_target() {
        let mut c = cam_at(0.0);
        c.follow(pos(1600.0, 100.0));
        let target = c.target_x;
        for _ in 0..200 {
            c.tick();
            assert!(
                c.x <= target + 0.5,
                "camera x={} should not overshoot target={}", c.x, target
            );
        }
    }

    #[test]
    fn lag_decreases_each_tick() {
        let mut c = cam_at(0.0);
        c.follow(pos(2000.0, 100.0));
        let lag0 = c.lag();
        c.tick();
        let lag1 = c.lag();
        assert!(lag1 < lag0, "lag should decrease each tick");
    }

    // ── Pan ───────────────────────────────────────────────────────────────────

    #[test]
    fn pan_moves_camera_immediately() {
        let mut c = cam_at(1600.0);
        let x_before = c.x;
        c.pan(100.0);
        assert!(c.x > x_before, "pan should move camera immediately");
    }

    #[test]
    fn pan_sets_panning_flag() {
        let mut c = cam_at(1600.0);
        assert!(!c.panning);
        c.pan(10.0);
        assert!(c.panning);
    }

    #[test]
    fn pan_clamps_to_world_bounds() {
        let mut c = cam_at(0.0);
        c.pan(-9999.0);
        assert_eq!(c.left_edge(), 0);

        let mut c2 = cam_at(WORLD_W as f32);
        c2.pan(9999.0);
        assert_eq!(c2.left_edge(), max_cam_x());
    }

    #[test]
    fn release_pan_clears_panning_flag() {
        let mut c = cam_at(1600.0);
        c.pan(100.0);
        assert!(c.panning);
        c.release_pan();
        assert!(!c.panning);
    }

    // ── snap_to ───────────────────────────────────────────────────────────────

    #[test]
    fn snap_instantly_centres_on_position() {
        let mut c = cam_at(0.0);
        c.snap_to(pos(1600.0, 100.0));
        assert_eq!(c.left_edge(), 1280);
        assert!(c.is_settled());
    }

    #[test]
    fn snap_has_zero_lag() {
        let mut c = cam_at(0.0);
        c.snap_to(pos(2000.0, 100.0));
        assert!(c.lag() < 0.5);
    }

    // ── world_to_screen ───────────────────────────────────────────────────────

    #[test]
    fn world_to_screen_x_at_left_edge_is_zero() {
        // Use a position where left_edge is known
        let c = Camera::new(640.0);
        // snap so left_edge = centred_on(640) = 640-320 = 320
        let screen_x = c.world_to_screen_x(320.0);
        assert_eq!(screen_x, Some(0));
    }

    #[test]
    fn world_pos_left_of_viewport_returns_none() {
        let c = cam_at(1600.0); // left_edge = 1280
        assert_eq!(c.world_to_screen_x(1279.0), None);
    }

    #[test]
    fn world_pos_right_of_viewport_returns_none() {
        let c = cam_at(1600.0); // left_edge = 1280, right = 1920
        assert_eq!(c.world_to_screen_x(1920.0), None);
    }

    #[test]
    fn world_to_screen_y_is_identity() {
        assert_eq!(Camera::world_to_screen_y(100.0), 100);
        assert_eq!(Camera::world_to_screen_y(0.0),   0);
        assert_eq!(Camera::world_to_screen_y(479.0), 479);
    }

    #[test]
    fn world_to_screen_visible_position() {
        let c = cam_at(1600.0); // left_edge = 1280
        // World x=1600 should map to screen x=320
        let sx = c.world_to_screen_x(1600.0);
        assert_eq!(sx, Some(320));
    }

    #[test]
    fn world_to_screen_off_bottom_returns_none() {
        let c = cam_at(1600.0);
        let result = c.world_to_screen(pos(1600.0, 500.0));
        assert!(result.is_none(), "y=500 is below screen height 480");
    }
}
