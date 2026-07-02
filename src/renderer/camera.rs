use crate::world::{WORLD_W, WORLD_H, SCREEN_W, SCREEN_H, WorldPos};

/// How quickly the camera lerps toward its target. 1.0 = instant.
/// 0.12 gives a smooth but responsive follow feel.
pub const CAM_LERP: f32 = 0.12;

/// How many pixels per tick the camera pans when R+dpad or L1+dpad is held.
pub const CAM_PAN_SPEED: f32 = 8.0;

/// Camera state.
/// `x` is the left edge of the 640px viewport in world pixels.
/// `y` is the top edge of the 480px viewport in world pixels.
#[derive(Debug, Clone)]
pub struct Camera {
    /// Current viewport left edge (fractional for smooth lerp).
    x: f32,
    /// Target the camera is lerping toward (X).
    target_x: f32,
    /// Current viewport top edge (fractional for smooth lerp).
    y: f32,
    /// Target the camera is lerping toward (Y).
    target_y: f32,
    /// Whether the player is currently free-panning.
    pub panning: bool,
}

impl Camera {
    /// Create a camera centred on a world position.
    pub fn new(world_x: f32, world_y: f32) -> Self {
        let x = centred_on_x(world_x);
        let y = centred_on_y(world_y);
        Self { x, target_x: x, y, target_y: y, panning: false }
    }

    /// Current left edge of the viewport as an integer pixel offset.
    pub fn left_edge(&self) -> u32 {
        (self.x as u32).min(max_cam_x())
    }

    /// Current top edge of the viewport as an integer pixel offset.
    pub fn top_edge(&self) -> u32 {
        (self.y as u32).min(max_cam_y())
    }

    /// Current fractional left edge — for sub-pixel smooth rendering.
    pub fn left_edge_f32(&self) -> f32 {
        self.x
    }

    /// Current fractional top edge.
    pub fn top_edge_f32(&self) -> f32 {
        self.y
    }

    /// Set the follow target to centre on a world position (X and Y).
    /// Only applies when not panning.
    pub fn follow(&mut self, pos: WorldPos) {
        if !self.panning {
            self.target_x = centred_on_x(pos.x);
            self.target_y = centred_on_y(pos.y);
        }
    }

    /// Set the follow target X only (does not change Y target).
    pub fn follow_x(&mut self, pos: WorldPos) {
        if !self.panning {
            self.target_x = centred_on_x(pos.x);
        }
    }

    /// Pan the camera horizontally by dx pixels per tick.
    pub fn pan(&mut self, dx: f32) {
        self.panning = true;
        self.target_x = (self.target_x + dx).clamp(0.0, max_cam_x() as f32);
        self.x = self.target_x;
    }

    /// Pan the camera vertically by dy pixels per tick.
    pub fn pan_y(&mut self, dy: f32) {
        self.panning = true;
        self.target_y = (self.target_y + dy).clamp(0.0, max_cam_y() as f32);
        self.y = self.target_y;
    }

    /// Release pan mode. Camera will lerp back to following the active entity.
    pub fn release_pan(&mut self) {
        self.panning = false;
    }

    /// Advance the camera by one tick — lerps toward target.
    pub fn tick(&mut self) {
        let dx = self.target_x - self.x;
        self.x += dx * CAM_LERP;
        self.x = self.x.clamp(0.0, max_cam_x() as f32);

        let dy = self.target_y - self.y;
        self.y += dy * CAM_LERP;
        self.y = self.y.clamp(0.0, max_cam_y() as f32);
    }

    /// Instantly snap to a world position with no lerp. Also clears pan mode.
    pub fn snap_to(&mut self, pos: WorldPos) {
        self.x = centred_on_x(pos.x);
        self.target_x = self.x;
        self.y = centred_on_y(pos.y);
        self.target_y = self.y;
        self.panning = false;
    }

    /// Follow a position ignoring pan mode — used for projectiles so they
    /// are always tracked regardless of free-cam state.
    pub fn follow_always(&mut self, pos: WorldPos) {
        self.target_x = centred_on_x(pos.x);
        self.target_y = centred_on_y(pos.y);
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
    /// Returns None if the position is outside the current viewport.
    pub fn world_to_screen_y(&self, world_y: f32) -> Option<i32> {
        let sy = (world_y - self.y).round() as i32;
        if sy >= 0 && sy < SCREEN_H as i32 {
            Some(sy)
        } else {
            None
        }
    }

    /// Convert a world position to screen coordinates.
    /// Returns None if outside the viewport.
    pub fn world_to_screen(&self, pos: WorldPos) -> Option<(i32, i32)> {
        let sx = self.world_to_screen_x(pos.x)?;
        let sy = self.world_to_screen_y(pos.y)?;
        Some((sx, sy))
    }

    /// How far (in pixels) the camera is from its target. Useful for
    /// deciding whether to skip a lerp frame.
    pub fn lag(&self) -> f32 {
        let dx = self.target_x - self.x;
        let dy = self.target_y - self.y;
        (dx * dx + dy * dy).sqrt()
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

/// The maximum valid camera top edge — keeps the bottom of the
/// viewport from exceeding the world height.
pub fn max_cam_y() -> u32 {
    WORLD_H.saturating_sub(SCREEN_H)
}

/// Calculate the camera left edge that centres the viewport on world_x.
fn centred_on_x(world_x: f32) -> f32 {
    let half = SCREEN_W as f32 / 2.0;
    (world_x - half).clamp(0.0, max_cam_x() as f32)
}

/// Calculate the camera top edge that centres the viewport on world_y.
fn centred_on_y(world_y: f32) -> f32 {
    let half = SCREEN_H as f32 / 2.0;
    (world_y - half).clamp(0.0, max_cam_y() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam_at(x: f32, y: f32) -> Camera { Camera::new(x, y) }
    fn pos(x: f32, y: f32) -> WorldPos { WorldPos::new(x, y) }

    #[test]
    fn camera_starts_centred_on_position() {
        let c = cam_at(1600.0, 480.0);
        assert_eq!(c.left_edge(), 1280);
        assert_eq!(c.top_edge(), 240);
    }

    #[test]
    fn camera_at_world_start_clamps_to_zero() {
        let c = cam_at(0.0, 0.0);
        assert_eq!(c.left_edge(), 0);
        assert_eq!(c.top_edge(), 0);
    }

    #[test]
    fn camera_at_world_end_clamps_to_max() {
        let c = cam_at(WORLD_W as f32, WORLD_H as f32);
        assert_eq!(c.left_edge(), max_cam_x());
        assert_eq!(c.top_edge(), max_cam_y());
    }

    #[test]
    fn max_cam_x_is_correct() {
        assert_eq!(max_cam_x(), WORLD_W - SCREEN_W);
    }

    #[test]
    fn max_cam_y_is_correct() {
        assert_eq!(max_cam_y(), WORLD_H - SCREEN_H);
    }

    #[test]
    fn follow_sets_target_to_centre_on_pos() {
        let mut c = cam_at(0.0, 0.0);
        c.follow(pos(1600.0, 480.0));
        assert!((c.target_x - 1280.0).abs() < 1.0);
        assert!((c.target_y - 240.0).abs() < 1.0);
    }

    #[test]
    fn follow_does_nothing_while_panning() {
        let mut c = cam_at(0.0, 0.0);
        c.pan(500.0);
        let x_before = c.x;
        c.follow(pos(3000.0, 900.0));
        assert_eq!(c.x, x_before, "follow should not move camera while panning");
    }

    #[test]
    fn pan_y_moves_camera_immediately() {
        let mut c = cam_at(960.0, 240.0);
        let y_before = c.y;
        c.pan_y(100.0);
        assert!(c.y > y_before);
    }

    #[test]
    fn pan_y_clamps_to_world_bounds() {
        let mut c = cam_at(0.0, 0.0);
        c.pan_y(-9999.0);
        assert_eq!(c.top_edge(), 0);

        let mut c2 = cam_at(0.0, WORLD_H as f32);
        c2.pan_y(9999.0);
        assert_eq!(c2.top_edge(), max_cam_y());
    }

    #[test]
    fn world_to_screen_y_uses_cam_y() {
        let c = cam_at(960.0, 480.0); // top_edge = 240
        assert_eq!(c.world_to_screen_y(240.0), Some(0));
        assert_eq!(c.world_to_screen_y(480.0), Some(240));
        assert_eq!(c.world_to_screen_y(239.0), None);
        assert_eq!(c.world_to_screen_y(720.0), None);
    }

    #[test]
    fn tick_moves_camera_toward_target() {
        let mut c = cam_at(0.0, 0.0);
        c.follow(pos(1600.0, 480.0));
        let y_before = c.y;
        c.tick();
        assert!(c.y > y_before);
    }

    #[test]
    fn camera_settles_after_many_ticks() {
        let mut c = cam_at(0.0, 0.0);
        c.follow(pos(1600.0, 480.0));
        for _ in 0..200 { c.tick(); }
        assert!(c.is_settled());
        assert!((c.left_edge() as i32 - 1280).abs() <= 1);
        assert!((c.top_edge() as i32 - 240).abs() <= 1);
    }

    #[test]
    fn snap_instantly_centres_on_position() {
        let mut c = cam_at(0.0, 0.0);
        c.snap_to(pos(1600.0, 480.0));
        assert_eq!(c.left_edge(), 1280);
        assert_eq!(c.top_edge(), 240);
        assert!(c.is_settled());
    }
}
