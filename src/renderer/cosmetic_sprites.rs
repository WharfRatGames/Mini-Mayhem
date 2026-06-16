/// Hat, gun, and boot cosmetic sprites embedded from deploy/assets/cosmetics/.
/// Hats:  66×60 px RGBA (22×20 game px @ 3x). IDs 1–15 are scrap-purchasable.
/// Guns:  138×78 px RGBA (46×26 game px @ 3x). IDs 1–10 are scrap-purchasable.
/// Boots: 36×27 px RGBA (12×9 game px @ 3x). IDs 1–4 are scrap-purchasable.
use std::sync::OnceLock;
use super::buffer::WorldBuffer;

// ── Hat sprites (IDs 1–11) ────────────────────────────────────────────────────

static HAT_PNGS: [&[u8]; 24] = [
    include_bytes!("../../deploy/assets/cosmetics/hat_1.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_2.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_3.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_4.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_5.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_6.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_7.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_8.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_9.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_10.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_11.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_12.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_13.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_14.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_15.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_16.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_17.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_18.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_19.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_20.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_21.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_22.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_23.png"),
    include_bytes!("../../deploy/assets/cosmetics/hat_24.png"),
];

static GUN_PNGS: [&[u8]; 12] = [
    include_bytes!("../../deploy/assets/cosmetics/gun_0.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_1.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_2.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_3.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_4.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_5.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_6.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_7.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_8.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_9.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_10.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_11.png"),
];

// ── Boot sprites (IDs 0–5) ───────────────────────────────────────────────────

static BOOT_PNGS: [&[u8]; 6] = [
    include_bytes!("../../deploy/assets/cosmetics/boot_0.png"),
    include_bytes!("../../deploy/assets/cosmetics/boot_1.png"),
    include_bytes!("../../deploy/assets/cosmetics/boot_2.png"),
    include_bytes!("../../deploy/assets/cosmetics/boot_3.png"),
    include_bytes!("../../deploy/assets/cosmetics/boot_4.png"),
    include_bytes!("../../deploy/assets/cosmetics/boot_5.png"),
];

struct Sprite { pub w: usize, pub h: usize, pub px: Vec<[u8; 4]> }

static HAT_SPRITES:  OnceLock<[Option<Sprite>; 18]> = OnceLock::new();
static GUN_SPRITES:  OnceLock<[Option<Sprite>; 12]>  = OnceLock::new();
static BOOT_SPRITES: OnceLock<[Option<Sprite>; 6]>  = OnceLock::new();

fn decode(bytes: &[u8]) -> Option<Sprite> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let (w, h) = (info.width as usize, info.height as usize);
    let color  = info.color_type;
    let mut raw = vec![0u8; reader.output_buffer_size()];
    reader.next_frame(&mut raw).ok()?;
    let px: Vec<[u8;4]> = match color {
        png::ColorType::Rgba => raw.chunks_exact(4).map(|c| [c[0],c[1],c[2],c[3]]).collect(),
        png::ColorType::Rgb  => raw.chunks_exact(3).map(|c| [c[0],c[1],c[2],255]).collect(),
        _ => return None,
    };
    Some(Sprite { w, h, px })
}

fn hat_sprites() -> &'static [Option<Sprite>; 18] {
    HAT_SPRITES.get_or_init(|| std::array::from_fn(|i| decode(HAT_PNGS[i])))
}

fn gun_sprites() -> &'static [Option<Sprite>; 12] {
    GUN_SPRITES.get_or_init(|| std::array::from_fn(|i| decode(GUN_PNGS[i])))
}

fn boot_sprites() -> &'static [Option<Sprite>; 6] {
    BOOT_SPRITES.get_or_init(|| std::array::from_fn(|i| decode(BOOT_PNGS[i])))
}

/// Draw hat sprite (id 1–15) centred at (cx, cy), scaled to render_w × render_h.
pub fn draw_hat(buf: &mut WorldBuffer, id: u8, cx: i32, cy: i32, render_w: i32, render_h: i32) {
    let idx = (id as usize).wrapping_sub(1);
    let sprites = hat_sprites();
    if idx >= sprites.len() { return; }
    let sp = match &sprites[idx] { Some(s) => s, None => return };
    if id == 2 {
        // Propeller Hat: the sprite's own propeller (source rows 18-26 of 60)
        // is a static bar; skip it here so skeleton.rs can draw an animated
        // spinning propeller in its place instead.
        blit_scaled_skip_rows(buf, sp, cx - render_w / 2, cy - render_h / 2, render_w, render_h, 18, 27);
        return;
    }
    blit_scaled(buf, sp, cx - render_w / 2, cy - render_h / 2, render_w, render_h);
}

/// Draw gun sprite (id 1–10) centred at (cx, cy), scaled to render_w × render_h.
/// id 0 = default gun (gun_0.png).
pub fn draw_gun(buf: &mut WorldBuffer, id: u8, cx: i32, cy: i32, render_w: i32, render_h: i32) {
    let idx = id as usize;
    let sprites = gun_sprites();
    if idx >= sprites.len() { return; }
    let sp = match &sprites[idx] { Some(s) => s, None => return };
    blit_scaled(buf, sp, cx - render_w / 2, cy - render_h / 2, render_w, render_h);
}

/// Draw boot sprite (id 0–5) centred at (cx, cy), scaled to render_w × render_h.
pub fn draw_boot(buf: &mut WorldBuffer, id: u8, cx: i32, cy: i32, render_w: i32, render_h: i32) {
    let idx = id as usize;
    let sprites = boot_sprites();
    if idx >= sprites.len() { return; }
    let sp = match &sprites[idx] { Some(s) => s, None => return };
    blit_scaled(buf, sp, cx - render_w / 2, cy - render_h / 2, render_w, render_h);
}

/// Draw gun sprite `id` rotated/scaled so its grip sits at `origin` and its
/// barrel points along `fwd` (unit vector), with `prp` the perpendicular unit
/// vector (rotated 90° from `fwd`). `length_px` is the desired barrel length
/// in game pixels. Returns the world position of the barrel tip.
pub fn draw_gun_oriented(
    buf: &mut WorldBuffer, id: u8,
    origin: (f32, f32), fwd: (f32, f32), prp: (f32, f32),
    length_px: f32,
) -> (f32, f32) {
    let sprites = gun_sprites();
    let idx = id as usize;
    if idx >= sprites.len() { return origin; }
    let sp = match &sprites[idx] { Some(s) => s, None => return origin };
    // Per COSMETIC_STYLE_GUIDE.md: barrel origin ~ image x33 (game px 11),
    // barrel axis at image y30 (game px 10); the gun's tip is ~46-4=42 game
    // px from the left edge, so ~31 game px of barrel ahead of the origin.
    const ORIGIN_GX: f32 = 11.0;
    const AXIS_GY:   f32 = 10.0;
    const BARREL_GW: f32 = 31.0;
    let scale = length_px / BARREL_GW;
    for sy in 0..sp.h {
        for sx in 0..sp.w {
            let [r, g, b, a] = sp.px[sy * sp.w + sx];
            if a < 16 { continue; }
            let t = (sx as f32 / 3.0 - ORIGIN_GX) * scale;
            let p = (sy as f32 / 3.0 - AXIS_GY) * scale;
            let x = (origin.0 + fwd.0 * t + prp.0 * p).round() as i32;
            let y = (origin.1 + fwd.1 * t + prp.1 * p).round() as i32;
            buf.set_pixel(x, y, super::fb::Bgra::new(r, g, b));
        }
    }
    (origin.0 + fwd.0 * length_px, origin.1 + fwd.1 * length_px)
}

/// Like blit_scaled, but skips source pixels whose row falls within [skip_y0, skip_y1).
fn blit_scaled_skip_rows(buf: &mut WorldBuffer, sp: &Sprite, x0: i32, y0: i32, rw: i32, rh: i32, skip_y0: usize, skip_y1: usize) {
    if rw <= 0 || rh <= 0 { return; }
    for dy in 0..rh {
        for dx in 0..rw {
            let sx = (dx * sp.w as i32 / rw) as usize;
            let sy = (dy * sp.h as i32 / rh) as usize;
            if sx >= sp.w || sy >= sp.h { continue; }
            if sy >= skip_y0 && sy < skip_y1 { continue; }
            let [r, g, b, a] = sp.px[sy * sp.w + sx];
            if a < 16 { continue; }
            buf.set_pixel(x0 + dx, y0 + dy, super::fb::Bgra::new(r, g, b));
        }
    }
}

fn blit_scaled(buf: &mut WorldBuffer, sp: &Sprite, x0: i32, y0: i32, rw: i32, rh: i32) {
    if rw <= 0 || rh <= 0 { return; }
    for dy in 0..rh {
        for dx in 0..rw {
            let sx = (dx * sp.w as i32 / rw) as usize;
            let sy = (dy * sp.h as i32 / rh) as usize;
            if sx >= sp.w || sy >= sp.h { continue; }
            let [r, g, b, a] = sp.px[sy * sp.w + sx];
            if a < 16 { continue; }
            buf.set_pixel(x0 + dx, y0 + dy, super::fb::Bgra::new(r, g, b));
        }
    }
}
