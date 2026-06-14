/// Title screen background — 640×480 PNG decoded once on first use.
use super::buffer::WorldBuffer;
use super::fb::Bgra;
use std::sync::OnceLock;

static DECODED: OnceLock<Option<Vec<u8>>> = OnceLock::new();

const PNG_BYTES: &[u8] = include_bytes!("../../deploy/assets/title_bg.png");

fn decoded() -> Option<&'static [u8]> {
    DECODED.get_or_init(|| {
        let decoder = png::Decoder::new(std::io::Cursor::new(PNG_BYTES));
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0u8; reader.output_buffer_size()];
        reader.next_frame(&mut buf).ok()?;
        // Handle RGB (3 bytes) or RGBA (4 bytes)
        let info = reader.info();
        let out = match info.color_type {
            png::ColorType::Rgb  => buf.chunks(3)
                .flat_map(|c| [c[0], c[1], c[2], 255u8]).collect(),
            png::ColorType::Rgba => buf,
            _ => return None,
        };
        Some(out)
    }).as_deref()
}

/// Draw the title background image at world position (cam_x, 0).
pub fn draw_title_bg(buf: &mut WorldBuffer, cam_x: i32) {
    let pixels = match decoded() { Some(p) => p, None => return };
    const W: usize = 640;
    const H: usize = 480;
    for y in 0..H {
        for x in 0..W {
            let idx = (y * W + x) * 4;
            if idx + 3 >= pixels.len() { break; }
            let col = Bgra::new(pixels[idx], pixels[idx+1], pixels[idx+2]);
            buf.set_pixel(cam_x + x as i32, y as i32, col);
        }
    }
}
