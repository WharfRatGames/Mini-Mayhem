use super::buffer::WorldBuffer;
use super::fb::Bgra;

/// Each glyph is 8 rows of 8 bits.
/// Bit 7 (MSB) of each byte = leftmost pixel.
type Glyph = [u8; 8];

/// Draw a single character at (x, y) in the world buffer.
/// Returns the x position after the character (x + 8).
pub fn draw_char(buf: &mut WorldBuffer, ch: char, x: i32, y: i32, colour: Bgra) -> i32 {
    let glyph = glyph_for(ch);
    for (row, &byte) in glyph.iter().enumerate() {
        for col in 0..8i32 {
            if byte & (0x80 >> col) != 0 {
                buf.set_pixel(x + col, y + row as i32, colour);
            }
        }
    }
    x + 8
}

/// Draw a string starting at (x, y). Characters are 8px wide with 1px spacing.
/// Returns the x position after the last character.
pub fn draw_str(buf: &mut WorldBuffer, s: &str, x: i32, y: i32, colour: Bgra) -> i32 {
    let mut cx = x;
    let chars: Vec<char> = s.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        cx = draw_char(buf, ch, cx, y, colour);
        if i < chars.len() - 1 {
            cx += 1; // 1px letter spacing between chars only
        }
    }
    cx
}

/// Draw a string with a dark shadow 1px down-right for readability over terrain.
pub fn draw_str_shadow(buf: &mut WorldBuffer, s: &str, x: i32, y: i32, colour: Bgra) {
    draw_str(buf, s, x + 1, y + 1, Bgra::new(0, 0, 0));
    draw_str(buf, s, x, y, colour);
}

/// Pixel width of a string in the bitmap font.
pub fn str_width(s: &str) -> i32 {
    if s.is_empty() { return 0; }
    let chars = s.chars().count() as i32;
    chars * 8 + (chars - 1) // 8px per char + 1px spacing between
}

/// Draw a character at pixel scale `scale` (e.g. 2 = 16×16, 3 = 24×24).
pub fn draw_char_scaled(buf: &mut WorldBuffer, ch: char, x: i32, y: i32, colour: Bgra, scale: i32) -> i32 {
    let glyph = glyph_for(ch);
    for (row, &byte) in glyph.iter().enumerate() {
        for col in 0..8i32 {
            if byte & (0x80 >> col) != 0 {
                for dy in 0..scale {
                    for dx in 0..scale {
                        buf.set_pixel(x + col * scale + dx, y + row as i32 * scale + dy, colour);
                    }
                }
            }
        }
    }
    x + 8 * scale
}

/// Draw a scaled string. Returns x after the last character.
pub fn draw_str_scaled(buf: &mut WorldBuffer, s: &str, x: i32, y: i32, colour: Bgra, scale: i32) -> i32 {
    let mut cx = x;
    let chars: Vec<char> = s.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        cx = draw_char_scaled(buf, ch, cx, y, colour, scale);
        if i < chars.len() - 1 { cx += scale; }
    }
    cx
}

/// Pixel width of a scaled string.
pub fn str_width_scaled(s: &str, scale: i32) -> i32 {
    if s.is_empty() { return 0; }
    let chars = s.chars().count() as i32;
    chars * 8 * scale + (chars - 1) * scale
}

/// Return the glyph for a character.
/// Unknown characters render as a small dot.
fn glyph_for(ch: char) -> Glyph {
    match ch {
        ' ' => [0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00],
        '!' => [0x18,0x18,0x18,0x18,0x18,0x00,0x18,0x00],
        '-' => [0x00,0x00,0x00,0x7E,0x00,0x00,0x00,0x00],
        '+' => [0x00,0x18,0x18,0x7E,0x18,0x18,0x00,0x00],
        ':' => [0x00,0x18,0x18,0x00,0x18,0x18,0x00,0x00],
        '/' => [0x02,0x04,0x08,0x10,0x20,0x40,0x00,0x00],
        '%' => [0x62,0x64,0x08,0x10,0x26,0x46,0x00,0x00],
        '?' => [0x3C,0x42,0x04,0x08,0x08,0x00,0x08,0x00],
        '.' => [0x00,0x00,0x00,0x00,0x00,0x00,0x18,0x00],
        '\'' => [0x18,0x18,0x10,0x00,0x00,0x00,0x00,0x00], // apostrophe: small upper tick
        '>' => [0x00,0x10,0x08,0x04,0x08,0x10,0x00,0x00],
        '<' => [0x00,0x04,0x08,0x10,0x08,0x04,0x00,0x00],
        '0' => [0x3C,0x42,0x46,0x4A,0x62,0x42,0x3C,0x00],
        '1' => [0x08,0x18,0x08,0x08,0x08,0x08,0x1C,0x00],
        '2' => [0x3C,0x42,0x02,0x1C,0x20,0x40,0x7E,0x00],
        '3' => [0x3C,0x42,0x02,0x1C,0x02,0x42,0x3C,0x00],
        '4' => [0x04,0x0C,0x14,0x24,0x7E,0x04,0x04,0x00],
        '5' => [0x7E,0x40,0x78,0x04,0x02,0x44,0x38,0x00],
        '6' => [0x1C,0x20,0x40,0x7C,0x42,0x42,0x3C,0x00],
        '7' => [0x7E,0x02,0x04,0x08,0x10,0x20,0x20,0x00],
        '8' => [0x3C,0x42,0x42,0x3C,0x42,0x42,0x3C,0x00],
        '9' => [0x3C,0x42,0x42,0x3E,0x02,0x04,0x38,0x00],
        'A' => [0x18,0x24,0x42,0x7E,0x42,0x42,0x42,0x00],
        'B' => [0x7C,0x42,0x42,0x7C,0x42,0x42,0x7C,0x00],
        'C' => [0x3C,0x42,0x40,0x40,0x40,0x42,0x3C,0x00],
        'D' => [0x78,0x44,0x42,0x42,0x42,0x44,0x78,0x00],
        'E' => [0x7E,0x40,0x40,0x78,0x40,0x40,0x7E,0x00],
        'F' => [0x7E,0x40,0x40,0x78,0x40,0x40,0x40,0x00],
        'G' => [0x3C,0x42,0x40,0x4E,0x42,0x42,0x3C,0x00],
        'H' => [0x42,0x42,0x42,0x7E,0x42,0x42,0x42,0x00],
        'I' => [0x3E,0x08,0x08,0x08,0x08,0x08,0x3E,0x00],
        'J' => [0x1E,0x04,0x04,0x04,0x04,0x44,0x38,0x00],
        'K' => [0x42,0x44,0x48,0x70,0x48,0x44,0x42,0x00],
        'L' => [0x40,0x40,0x40,0x40,0x40,0x40,0x7E,0x00],
        'M' => [0x42,0x66,0x5A,0x42,0x42,0x42,0x42,0x00],
        'N' => [0x42,0x62,0x52,0x4A,0x46,0x42,0x42,0x00],
        'O' => [0x3C,0x42,0x42,0x42,0x42,0x42,0x3C,0x00],
        'P' => [0x7C,0x42,0x42,0x7C,0x40,0x40,0x40,0x00],
        'Q' => [0x3C,0x42,0x42,0x42,0x4A,0x44,0x3A,0x00],
        'R' => [0x7C,0x42,0x42,0x7C,0x48,0x44,0x42,0x00],
        'S' => [0x3C,0x42,0x40,0x3C,0x02,0x42,0x3C,0x00],
        'T' => [0x7E,0x10,0x10,0x10,0x10,0x10,0x10,0x00],
        'U' => [0x42,0x42,0x42,0x42,0x42,0x42,0x3C,0x00],
        'V' => [0x42,0x42,0x42,0x42,0x24,0x18,0x00,0x00],
        'W' => [0x42,0x42,0x42,0x42,0x5A,0x66,0x42,0x00],
        'X' => [0x42,0x24,0x18,0x18,0x24,0x42,0x00,0x00],
        'Y' => [0x42,0x42,0x24,0x18,0x10,0x10,0x10,0x00],
        'Z' => [0x7E,0x02,0x04,0x18,0x20,0x40,0x7E,0x00],
        // Lowercase — map to uppercase for now
        'a'..='z' => glyph_for((ch as u8 - 32) as char),
        // Fallback: small dot
        _ => [0x00,0x00,0x00,0x18,0x18,0x00,0x00,0x00],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf() -> WorldBuffer { WorldBuffer::new() }

    // ── glyph_for ────────────────────────────────────────────────────────────

    #[test]
    fn space_glyph_is_all_zero() {
        assert_eq!(glyph_for(' '), [0u8; 8]);
    }

    #[test]
    fn all_digits_have_nonzero_glyphs() {
        for ch in '0'..='9' {
            let g = glyph_for(ch);
            assert!(g.iter().any(|&b| b != 0), "digit '{ch}' glyph is empty");
        }
    }

    #[test]
    fn all_uppercase_letters_have_nonzero_glyphs() {
        for ch in 'A'..='Z' {
            let g = glyph_for(ch);
            assert!(g.iter().any(|&b| b != 0), "letter '{ch}' glyph is empty");
        }
    }

    #[test]
    fn lowercase_maps_to_uppercase() {
        for (lower, upper) in ('a'..='z').zip('A'..='Z') {
            assert_eq!(glyph_for(lower), glyph_for(upper),
                "'{lower}' should map to '{upper}' glyph");
        }
    }

    #[test]
    fn unknown_char_returns_dot_fallback() {
        let dot = glyph_for('~');
        assert!(dot.iter().any(|&b| b != 0), "fallback glyph should not be blank");
    }

    // ── draw_char ─────────────────────────────────────────────────────────────

    #[test]
    fn draw_char_returns_x_plus_8() {
        let mut b = buf();
        let next_x = draw_char(&mut b, 'A', 10, 10, Bgra::white());
        assert_eq!(next_x, 18);
    }

    #[test]
    fn draw_char_sets_pixels_for_nonspace() {
        let mut b = buf();
        draw_char(&mut b, 'I', 100, 100, Bgra::white());
        // 'I' has pixels — at least one should be set near (100,100)
        let mut found = false;
        for dy in 0..8i32 {
            for dx in 0..8i32 {
                if b.get_pixel(100 + dx, 100 + dy) == Bgra::white() {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "draw_char 'I' should set at least one pixel");
    }

    #[test]
    fn draw_space_sets_no_pixels() {
        let mut b = buf();
        draw_char(&mut b, ' ', 50, 50, Bgra::white());
        for dy in 0..8i32 {
            for dx in 0..8i32 {
                assert_eq!(b.get_pixel(50 + dx, 50 + dy), Bgra::black(),
                    "space should not set any pixels");
            }
        }
    }

    #[test]
    fn draw_char_uses_correct_colour() {
        let mut b = buf();
        let colour = Bgra::new(200, 100, 50);
        draw_char(&mut b, 'O', 20, 20, colour);
        let mut found = false;
        for dy in 0..8i32 {
            for dx in 0..8i32 {
                if b.get_pixel(20 + dx, 20 + dy) == colour {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "drawn pixels should use the supplied colour");
    }

    // ── draw_str ─────────────────────────────────────────────────────────────

    #[test]
    fn draw_str_advances_x_per_char() {
        let mut b = buf();
        let end_x = draw_str(&mut b, "AB", 0, 0, Bgra::white());
        // A(8) + spacing(1) + B(8) = 17
        assert_eq!(end_x, 17);
    }

    #[test]
    fn draw_str_empty_returns_start_x() {
        let mut b = buf();
        let end_x = draw_str(&mut b, "", 50, 50, Bgra::white());
        assert_eq!(end_x, 50);
    }

    #[test]
    fn draw_str_single_char_returns_x_plus_8() {
        let mut b = buf();
        let end_x = draw_str(&mut b, "X", 0, 0, Bgra::white());
        assert_eq!(end_x, 8);
    }

    // ── str_width ─────────────────────────────────────────────────────────────

    #[test]
    fn str_width_empty_is_zero() {
        assert_eq!(str_width(""), 0);
    }

    #[test]
    fn str_width_single_char_is_8() {
        assert_eq!(str_width("A"), 8);
    }

    #[test]
    fn str_width_two_chars_is_17() {
        assert_eq!(str_width("AB"), 17); // 8 + 1 + 8
    }

    #[test]
    fn str_width_matches_draw_str_advance() {
        let mut b = buf();
        let s = "HELLO";
        let end_x = draw_str(&mut b, s, 0, 0, Bgra::white());
        assert_eq!(end_x, str_width(s));
    }

    // ── draw_str_shadow ───────────────────────────────────────────────────────

    #[test]
    fn draw_str_shadow_sets_pixels() {
        let mut b = buf();
        draw_str_shadow(&mut b, "HI", 50, 50, Bgra::white());
        let mut found = false;
        for dy in 0..10i32 {
            for dx in 0..20i32 {
                if b.get_pixel(50 + dx, 50 + dy) != Bgra::black() {
                    found = true;
                    break;
                }
            }
        }
        assert!(found, "draw_str_shadow should draw pixels");
    }

    // ── out of bounds ─────────────────────────────────────────────────────────

    #[test]
    fn draw_str_at_right_edge_does_not_panic() {
        let mut b = buf();
        draw_str(&mut b, "HELLO", crate::world::WORLD_W as i32 - 10, 100, Bgra::white());
    }

    #[test]
    fn draw_str_at_bottom_edge_does_not_panic() {
        let mut b = buf();
        draw_str(&mut b, "HELLO", 100, crate::world::WORLD_H as i32 - 4, Bgra::white());
    }
}
