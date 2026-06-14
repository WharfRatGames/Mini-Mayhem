/// Hat and gun cosmetic sprites embedded from deploy/assets/cosmetics/.
/// Hats:  66×60 px RGBA (22×20 game px @ 3x). IDs 1–9 are scrap-purchasable.
/// Guns:  138×78 px RGBA (46×26 game px @ 3x). IDs 1–5 are scrap-purchasable.
use std::sync::OnceLock;
use super::buffer::WorldBuffer;

// ── Hat sprites (IDs 1–11) ────────────────────────────────────────────────────

static HAT_PNGS: [&[u8]; 11] = [
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
];

static GUN_PNGS: [&[u8]; 8] = [
    include_bytes!("../../deploy/assets/cosmetics/gun_0.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_1.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_2.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_3.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_4.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_5.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_6.png"),
    include_bytes!("../../deploy/assets/cosmetics/gun_7.png"),
];

struct Sprite { pub w: usize, pub h: usize, pub px: Vec<[u8; 4]> }

static HAT_SPRITES: OnceLock<[Option<Sprite>; 11]> = OnceLock::new();
static GUN_SPRITES: OnceLock<[Option<Sprite>; 8]>  = OnceLock::new();

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

fn hat_sprites() -> &'static [Option<Sprite>; 11] {
    HAT_SPRITES.get_or_init(|| std::array::from_fn(|i| decode(HAT_PNGS[i])))
}

fn gun_sprites() -> &'static [Option<Sprite>; 8] {
    GUN_SPRITES.get_or_init(|| std::array::from_fn(|i| decode(GUN_PNGS[i])))
}

/// Draw hat sprite (id 1–11) centred at (cx, cy), scaled to render_w × render_h.
pub fn draw_hat(buf: &mut WorldBuffer, id: u8, cx: i32, cy: i32, render_w: i32, render_h: i32) {
    let idx = (id as usize).wrapping_sub(1);
    let sprites = hat_sprites();
    if idx >= sprites.len() { return; }
    let sp = match &sprites[idx] { Some(s) => s, None => return };
    blit_scaled(buf, sp, cx - render_w / 2, cy - render_h / 2, render_w, render_h);
}

/// Draw gun sprite (id 1–7) centred at (cx, cy), scaled to render_w × render_h.
/// id 0 = default gun (gun_0.png).
pub fn draw_gun(buf: &mut WorldBuffer, id: u8, cx: i32, cy: i32, render_w: i32, render_h: i32) {
    let idx = id as usize;
    let sprites = gun_sprites();
    if idx >= sprites.len() { return; }
    let sp = match &sprites[idx] { Some(s) => s, None => return };
    blit_scaled(buf, sp, cx - render_w / 2, cy - render_h / 2, render_w, render_h);
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
