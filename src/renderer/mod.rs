#![allow(unused_imports)]
pub mod fb;
pub mod buffer;
pub mod camera;
pub mod draw_terrain;
pub mod background;
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
