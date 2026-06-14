use super::buffer::WorldBuffer;
use super::fb::Bgra;
use crate::world::{SCREEN_W, SCREEN_H};

const WHARF_JPG: &[u8] = include_bytes!("../../assets/wharf.jpg");

/// Decode wharf.jpg and draw it scaled to fill the 640×480 screen.
/// Fits width exactly (784→640), centers the 480-row crop of the 953-row scaled result.
pub fn draw_splash(buf: &mut WorldBuffer) {
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(WHARF_JPG));
    let pixels = match decoder.decode() {
        Ok(p) => p,
        Err(_) => return,
    };
    let meta = decoder.info().unwrap();
    let src_w = meta.width as usize;
    let src_h = meta.height as usize;
    let dst_w = SCREEN_W as usize;
    let dst_h = SCREEN_H as usize;

    // Scale: fit height exactly (show whole image, black bars left/right).
    // scaled_w = src_w * dst_h / src_h
    let scaled_w = src_w * dst_h / src_h;
    let x_offset = ((dst_w as isize - scaled_w as isize) / 2).max(0) as usize;

    // Fill black first so the side bars are dark.
    buf.fill_rect(0, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(0, 0, 0));

    for dy in 0..dst_h {
        let src_y = (dy * src_h / dst_h).min(src_h - 1);
        for dx in 0..scaled_w.min(dst_w) {
            let src_x = (dx * src_w / scaled_w).min(src_w - 1);
            let idx = (src_y * src_w + src_x) * 3;
            if idx + 2 >= pixels.len() { continue; }
            let r = pixels[idx];
            let g = pixels[idx + 1];
            let b = pixels[idx + 2];
            buf.set_pixel((x_offset + dx) as i32, dy as i32, Bgra::new(r, g, b));
        }
    }
}
