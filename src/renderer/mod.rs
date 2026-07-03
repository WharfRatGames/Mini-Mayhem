#![allow(unused_imports)]

// ── Shared sine LUT ───────────────────────────────────────────────────────────

const SIN_LUT_N: usize = 1024;

/// Fast sin approximation via 1024-entry lookup table.
/// Max error < 0.003 rad — imperceptible for visual animation.
pub fn sin_lut(x: f32) -> f32 {
    static LUT: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
    let lut = LUT.get_or_init(|| {
        (0..SIN_LUT_N)
            .map(|i| (i as f32 * std::f32::consts::TAU / SIN_LUT_N as f32).sin())
            .collect()
    });
    let idx = (x * (SIN_LUT_N as f32 / std::f32::consts::TAU))
        .rem_euclid(SIN_LUT_N as f32) as usize;
    lut[idx]
}

pub mod fb;
pub mod buffer;
pub mod camera;
pub mod draw_terrain;
pub mod background;
pub mod bg_image;
pub mod fx;
pub mod draw_sprites;
pub mod font;
pub mod hud;
pub mod keyboard;
pub mod avatar;
pub mod title_bg;
pub mod terrain_textures;
pub mod skeleton;
pub mod cosmetic_sprites;
pub mod splash;
pub mod scenery;

pub use fb::{Framebuffer, Bgra};
pub use buffer::WorldBuffer;
pub use camera::{Camera, CAM_LERP, CAM_PAN_SPEED, max_cam_x};
pub use draw_terrain::{draw_terrain, draw_terrain_viewport};
pub use draw_sprites::{
    draw_soldier, draw_hp_number, draw_aim_arrow,
    draw_projectile, draw_think_indicator, draw_water_surface,
    TEAM_COLOURS, SOLDIER_W, SOLDIER_H,
};
pub use font::{draw_char, draw_str, draw_str_shadow, str_width};
pub use hud::{draw_hud, draw_game_over, draw_countdown, draw_pause_menu, HUD_H, HUD_Y};
