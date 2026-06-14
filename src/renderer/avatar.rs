/// Avatar system — 64×64 RGBA PNGs embedded in the binary, decoded once on first use.
use super::buffer::WorldBuffer;
use super::fb::Bgra;
use std::sync::OnceLock;

pub const AVATAR_COUNT: usize = 13;
pub const AVATAR_SRC_SIZE: u32 = 64;

static AVATAR_PNGS: [&[u8]; AVATAR_COUNT] = [
    include_bytes!("../../deploy/avatars/avatar_0.png"),
    include_bytes!("../../deploy/avatars/avatar_1.png"),
    include_bytes!("../../deploy/avatars/avatar_2.png"),
    include_bytes!("../../deploy/avatars/avatar_3.png"),
    include_bytes!("../../deploy/avatars/avatar_4.png"),
    include_bytes!("../../deploy/avatars/avatar_5.png"),
    include_bytes!("../../deploy/avatars/avatar_6.png"),
    include_bytes!("../../deploy/avatars/avatar_7.png"),
    include_bytes!("../../deploy/avatars/avatar_8.png"),
    include_bytes!("../../deploy/avatars/avatar_9.png"),
    include_bytes!("../../deploy/avatars/avatar_10.png"),
    include_bytes!("../../deploy/avatars/avatar_11.png"),
    include_bytes!("../../deploy/avatars/avatar_12.png"),
];

static DECODED: OnceLock<[Option<Vec<u8>>; AVATAR_COUNT]> = OnceLock::new();

fn decoded() -> &'static [Option<Vec<u8>>; AVATAR_COUNT] {
    DECODED.get_or_init(|| {
        std::array::from_fn(|i| decode_rgba(AVATAR_PNGS[i]))
    })
}

fn decode_rgba(bytes: &[u8]) -> Option<Vec<u8>> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let color_type = info.color_type;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    reader.next_frame(&mut buf).ok()?;
    match color_type {
        png::ColorType::Rgba => Some(buf), // RGBA8 — use directly
        png::ColorType::Rgb  => {
            // RGB8 — add full alpha
            Some(buf.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255u8]).collect())
        }
        _ => None,
    }
}

/// Draw avatar `id` (0–3) at world position (x, y), scaled to display_size × display_size.
/// Uses nearest-neighbour scaling — good for pixel art.
pub fn draw_avatar(buf: &mut WorldBuffer, x: i32, y: i32, display_size: u32, avatar_id: u8) {
    let id = avatar_id as usize;
    if id >= AVATAR_COUNT { return; }
    let pixels = match decoded()[id].as_deref() {
        Some(p) => p,
        None    => return,
    };
    let src = AVATAR_SRC_SIZE as usize;
    let dst = display_size as usize;
    for dy in 0..dst {
        for dx in 0..dst {
            let sx = dx * src / dst;
            let sy = dy * src / dst;
            let idx = (sy * src + sx) * 4;
            if idx + 3 >= pixels.len() { continue; }
            let a = pixels[idx + 3];
            if a < 128 { continue; }
            let col = Bgra::new(pixels[idx], pixels[idx + 1], pixels[idx + 2]);
            buf.set_pixel(x + dx as i32, y + dy as i32, col);
        }
    }
}

/// Short name shown in the editor when no avatar image is visible.
pub fn avatar_label(id: u8) -> &'static str {
    match id {
        0  => "HELMET",    1  => "RED BERET",   2  => "VISOR",
        3  => "BANDANA",   4  => "INFANTRY",     5  => "COMMS",
        6  => "BEANIE",    7  => "BLONDE",       8  => "USHANKA",
        9  => "GOGGLES",   10 => "GAS MASK",     11 => "GENERAL",
        12 => "SCOUT",     _  => "SOLDIER",
    }
}
