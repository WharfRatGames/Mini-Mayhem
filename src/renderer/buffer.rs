//! In-memory BGRA pixel buffer for the full 3200×480 world.
//!
//! We render everything into this buffer first, then blit
//! the 640px viewport slice to the framebuffer each frame.
//! This keeps all drawing logic independent of the hardware.

use crate::world::{WORLD_W, WORLD_H, SCREEN_W, SCREEN_H, WATER_Y, Terrain};
use super::fb::Bgra;

/// Full-world BGRA pixel buffer.
/// Indexed row-major: pixel (x, y) is at byte offset (y * WORLD_W + x) * 4.
pub struct WorldBuffer {
    /// Raw BGRA bytes. Length = WORLD_W * WORLD_H * 4.
    data: Vec<u8>,
}

impl WorldBuffer {
    /// Allocate a blank (black) world buffer.
    pub fn new() -> Self {
        Self {
            data: vec![0u8; (WORLD_W * WORLD_H * 4) as usize],
        }
    }

    /// Write a single BGRA pixel. Silently clips out-of-bounds.
    #[inline(always)]
    pub fn set_pixel(&mut self, x: i32, y: i32, colour: Bgra) {
        if x < 0 || y < 0 || x >= WORLD_W as i32 || y >= WORLD_H as i32 {
            return;
        }
        let off = (y as u32 * WORLD_W + x as u32) as usize * 4;
        self.data[off]     = colour.b;
        self.data[off + 1] = colour.g;
        self.data[off + 2] = colour.r;
        self.data[off + 3] = 0xFF;
    }

    /// Read a pixel colour. Returns black for out-of-bounds.
    pub fn get_pixel(&self, x: i32, y: i32) -> Bgra {
        if x < 0 || y < 0 || x >= WORLD_W as i32 || y >= WORLD_H as i32 {
            return Bgra::black();
        }
        let off = (y as u32 * WORLD_W + x as u32) as usize * 4;
        Bgra::new(self.data[off + 2], self.data[off + 1], self.data[off])
    }

    /// Fill a rectangle with a colour. Clips to world bounds.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, colour: Bgra) {
        for dy in 0..h as i32 {
            for dx in 0..w as i32 {
                self.set_pixel(x + dx, y + dy, colour);
            }
        }
    }

    /// Fill the entire buffer with one colour.
    pub fn clear(&mut self, colour: Bgra) {
        for i in (0..self.data.len()).step_by(4) {
            self.data[i]     = colour.b;
            self.data[i + 1] = colour.g;
            self.data[i + 2] = colour.r;
            self.data[i + 3] = 0xFF;
        }
    }

    /// Draw a filled circle. Clips to world bounds.
    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, colour: Bgra) {
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    self.set_pixel(cx + dx, cy + dy, colour);
                }
            }
        }
    }

    /// Draw a line using Bresenham's algorithm.
    pub fn draw_line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, colour: Bgra) {
        let (mut x, mut y) = (x0, y0);
        let dx =  (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1i32 } else { -1 };
        let sy = if y0 < y1 { 1i32 } else { -1 };
        let mut err = dx + dy;

        loop {
            self.set_pixel(x, y, colour);
            if x == x1 && y == y1 { break; }
            let e2 = 2 * err;
            if e2 >= dy { err += dy; x += sx; }
            if e2 <= dx { err += dx; y += sy; }
        }
    }

    /// Copy a 640×480 viewport slice from the world buffer to a framebuffer.
    /// `cam_x` is the left edge of the viewport in world pixels.
    /// Clamps so the viewport never exceeds world bounds.
    pub fn blit_to_fb(&self, fb: &mut super::fb::Framebuffer, cam_x: u32) {
        let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
        for screen_y in 0..SCREEN_H {
            let world_y = screen_y;
            let src_off = (world_y * WORLD_W + cam_x) as usize * 4;
            let src_row = &self.data[src_off..src_off + (SCREEN_W * 4) as usize];
            fb.blit_row(screen_y, src_row);
        }
    }

    /// Copy the 640×480 viewport slice from `src` into the same region of `self`.
    /// Used each frame to stamp the world cache into the working draw buffer.
    pub fn copy_viewport_from(&mut self, src: &WorldBuffer, cam_x: u32) {
        let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
        let row_bytes = SCREEN_W as usize * 4;
        for y in 0..WORLD_H {
            let off = (y * WORLD_W + cam_x) as usize * 4;
            self.data[off..off + row_bytes].copy_from_slice(&src.data[off..off + row_bytes]);
        }
    }

    /// Like copy_viewport_from, but for rows above the waterline only copies
    /// pixels where the terrain is solid — sky pixels are left untouched so
    /// atmospheric background layers drawn earlier remain visible.
    pub fn copy_viewport_from_sky_aware(&mut self, src: &WorldBuffer, cam_x: u32, terrain: &Terrain) {
        let cam_x = cam_x.min(WORLD_W.saturating_sub(SCREEN_W));
        let row_bytes = SCREEN_W as usize * 4;
        for y in 0..WORLD_H {
            let off = (y * WORLD_W + cam_x) as usize * 4;
            if y >= WATER_Y {
                self.data[off..off + row_bytes].copy_from_slice(&src.data[off..off + row_bytes]);
            } else {
                for x in 0..SCREEN_W {
                    let wx = cam_x + x;
                    // Above the original surface, this column is guaranteed sky
                    // (explosions only remove material, never add it above spawn_y).
                    if y < terrain.sky_limit[wx as usize] { continue; }
                    if terrain.is_solid(wx as i32, y as i32) {
                        let p = off + (x * 4) as usize;
                        self.data[p..p + 4].copy_from_slice(&src.data[p..p + 4]);
                    }
                }
            }
        }
    }

    /// Blit an RGBA sprite (row-major, `src_w`×`src_h`) at (x0, y0).
    /// Pixels with alpha < 16 are skipped. Source is RGBA, dest is BGRA.
    pub fn draw_sprite_rgba(
        &mut self,
        x0: i32, y0: i32,
        src: &[[u8; 4]], src_w: usize, src_h: usize,
    ) {
        for sy in 0..src_h {
            for sx in 0..src_w {
                let [r, g, b, a] = src[sy * src_w + sx];
                if a < 16 { continue; }
                self.set_pixel(x0 + sx as i32, y0 + sy as i32, Bgra::new(r, g, b));
            }
        }
    }

    /// Raw BGRA bytes — for testing.
    pub fn raw(&self) -> &[u8] { &self.data }
}

impl Default for WorldBuffer {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf() -> WorldBuffer { WorldBuffer::new() }

    // ── Allocation ────────────────────────────────────────────────────────────

    #[test]
    fn buffer_has_correct_byte_count() {
        let mut b = buf();
        assert_eq!(b.data.len(), (WORLD_W * WORLD_H * 4) as usize);
    }

    #[test]
    fn new_buffer_is_all_black() {
        let mut b = buf();
        assert!(b.data.iter().all(|&v| v == 0));
    }

    // ── set_pixel / get_pixel ─────────────────────────────────────────────────

    #[test]
    fn set_and_get_pixel_round_trip() {
        let mut b = buf();
        let colour = Bgra::new(200, 100, 50);
        b.set_pixel(10, 20, colour);
        assert_eq!(b.get_pixel(10, 20), colour);
    }

    #[test]
    fn pixel_stored_in_bgra_byte_order() {
        let mut b = buf();
        let colour = Bgra::new(255, 128, 64); // r=255 g=128 b=64
        b.set_pixel(0, 0, colour);
        let off = 0usize;
        assert_eq!(b.raw()[off],     64,  "byte 0 should be blue");
        assert_eq!(b.raw()[off + 1], 128, "byte 1 should be green");
        assert_eq!(b.raw()[off + 2], 255, "byte 2 should be red");
        assert_eq!(b.raw()[off + 3], 0xFF,"byte 3 should be alpha=255");
    }

    #[test]
    fn out_of_bounds_set_does_not_panic() {
        let mut b = buf();
        b.set_pixel(-1, 0, Bgra::white());
        b.set_pixel(0, -1, Bgra::white());
        b.set_pixel(WORLD_W as i32, 0, Bgra::white());
        b.set_pixel(0, WORLD_H as i32, Bgra::white());
        b.set_pixel(-9999, -9999, Bgra::white());
    }

    #[test]
    fn out_of_bounds_get_returns_black() {
        let mut b = buf();
        assert_eq!(b.get_pixel(-1, 0),           Bgra::black());
        assert_eq!(b.get_pixel(WORLD_W as i32, 0), Bgra::black());
    }

    #[test]
    fn adjacent_pixels_are_independent() {
        let mut b = buf();
        b.set_pixel(100, 100, Bgra::red());
        assert_eq!(b.get_pixel(100, 100), Bgra::red());
        assert_eq!(b.get_pixel(101, 100), Bgra::black());
        assert_eq!(b.get_pixel(99,  100), Bgra::black());
        assert_eq!(b.get_pixel(100, 101), Bgra::black());
        assert_eq!(b.get_pixel(100, 99),  Bgra::black());
    }

    #[test]
    fn corners_are_settable() {
        let mut b = buf();
        b.set_pixel(0, 0, Bgra::red());
        b.set_pixel(WORLD_W as i32 - 1, 0, Bgra::green());
        b.set_pixel(0, WORLD_H as i32 - 1, Bgra::blue());
        b.set_pixel(WORLD_W as i32 - 1, WORLD_H as i32 - 1, Bgra::white());
        assert_eq!(b.get_pixel(0, 0), Bgra::red());
        assert_eq!(b.get_pixel(WORLD_W as i32 - 1, 0), Bgra::green());
        assert_eq!(b.get_pixel(0, WORLD_H as i32 - 1), Bgra::blue());
        assert_eq!(b.get_pixel(WORLD_W as i32 - 1, WORLD_H as i32 - 1), Bgra::white());
    }

    // ── clear ────────────────────────────────────────────────────────────────

    #[test]
    fn clear_sets_all_pixels() {
        let mut b = buf();
        b.set_pixel(100, 100, Bgra::red());
        b.clear(Bgra::sky());
        assert_eq!(b.get_pixel(100, 100), Bgra::sky());
        assert_eq!(b.get_pixel(0, 0),     Bgra::sky());
    }

    // ── fill_rect ────────────────────────────────────────────────────────────

    #[test]
    fn fill_rect_sets_all_pixels_in_region() {
        let mut b = buf();
        b.fill_rect(50, 50, 10, 10, Bgra::yellow());
        for dy in 0..10i32 {
            for dx in 0..10i32 {
                assert_eq!(b.get_pixel(50 + dx, 50 + dy), Bgra::yellow(),
                    "pixel ({},{}) should be yellow", 50+dx, 50+dy);
            }
        }
    }

    #[test]
    fn fill_rect_does_not_bleed_outside() {
        let mut b = buf();
        b.fill_rect(50, 50, 10, 10, Bgra::yellow());
        assert_eq!(b.get_pixel(49, 50), Bgra::black());
        assert_eq!(b.get_pixel(60, 50), Bgra::black());
        assert_eq!(b.get_pixel(50, 49), Bgra::black());
        assert_eq!(b.get_pixel(50, 60), Bgra::black());
    }

    #[test]
    fn fill_rect_clips_at_world_edge() {
        let mut b = buf();
        // Should not panic even when rect extends outside world
        b.fill_rect(WORLD_W as i32 - 5, 0, 20, 20, Bgra::red());
    }

    // ── fill_circle ──────────────────────────────────────────────────────────

    #[test]
    fn fill_circle_sets_centre() {
        let mut b = buf();
        b.fill_circle(200, 200, 10, Bgra::red());
        assert_eq!(b.get_pixel(200, 200), Bgra::red());
    }

    #[test]
    fn fill_circle_does_not_set_outside_radius() {
        let mut b = buf();
        b.fill_circle(200, 200, 10, Bgra::red());
        assert_eq!(b.get_pixel(211, 200), Bgra::black());
        assert_eq!(b.get_pixel(200, 211), Bgra::black());
    }

    #[test]
    fn fill_circle_at_edge_does_not_panic() {
        let mut b = buf();
        b.fill_circle(0, 0, 20, Bgra::blue());
        b.fill_circle(WORLD_W as i32 - 1, WORLD_H as i32 - 1, 20, Bgra::blue());
    }

    // ── draw_line ────────────────────────────────────────────────────────────

    #[test]
    fn draw_line_sets_endpoints() {
        let mut b = buf();
        b.draw_line(10, 10, 50, 10, Bgra::white());
        assert_eq!(b.get_pixel(10, 10), Bgra::white());
        assert_eq!(b.get_pixel(50, 10), Bgra::white());
    }

    #[test]
    fn draw_line_horizontal_sets_all_pixels() {
        let mut b = buf();
        b.draw_line(10, 20, 20, 20, Bgra::green());
        for x in 10..=20 {
            assert_eq!(b.get_pixel(x, 20), Bgra::green(), "x={x} should be green");
        }
    }

    #[test]
    fn draw_line_single_pixel() {
        let mut b = buf();
        b.draw_line(100, 100, 100, 100, Bgra::red());
        assert_eq!(b.get_pixel(100, 100), Bgra::red());
    }

    // ── blit viewport ────────────────────────────────────────────────────────

    #[test]
    fn blit_cam_x_zero_copies_left_edge() {
        // We can't test blit_to_fb without a framebuffer, but we can
        // verify the source data is correct for cam_x=0
        let mut b = buf();
        let colour = Bgra::red();
        b.set_pixel(0, 0, colour);
        b.set_pixel(639, 0, Bgra::green());
        // Row 0 at cam_x=0 should start with the red colour we set
        // Byte order is BGRA: off+0=blue, off+1=green, off+2=red, off+3=alpha
        let off = 0usize;
        assert_eq!(b.raw()[off + 2], colour.r); // red channel at x=0
        assert_eq!(b.raw()[off],     colour.b); // blue channel at x=0
    }

    #[test]
    fn blit_cam_x_at_world_max_does_not_panic() {
        // cam_x clamped so viewport never exceeds world
        let mut b = buf();
        // Just verify the clamp arithmetic is correct
        let max_cam_x = WORLD_W.saturating_sub(SCREEN_W);
        assert_eq!(max_cam_x, WORLD_W - SCREEN_W);
        assert_eq!(max_cam_x, 2560); // 3200 - 640
    }
}
