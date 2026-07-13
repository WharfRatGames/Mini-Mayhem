/// Labeled reference sheet of every scenery decoration sprite, per theme.
/// Run: cargo run --bin scenery-gallery
/// Output: assets/scenery_gallery.png
use arty::renderer::buffer::WorldBuffer;
use arty::renderer::scenery::draw_scenery;
use arty::world::terrain::{SceneryObject, Terrain, Theme};

// ── Canvas (self-contained, mirrors sprite_sheet.rs — no live-code dependency) ─

struct Canvas { data: Vec<u8>, w: u32, h: u32 }

impl Canvas {
    fn new(w: u32, h: u32, bg: (u8, u8, u8)) -> Self {
        let mut data = vec![0u8; (w * h * 4) as usize];
        for i in 0..w * h {
            let o = (i * 4) as usize;
            data[o] = bg.0; data[o + 1] = bg.1; data[o + 2] = bg.2; data[o + 3] = 255;
        }
        Self { data, w, h }
    }
    fn set(&mut self, x: i32, y: i32, r: u8, g: u8, b: u8) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 { return; }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        self.data[i] = r; self.data[i + 1] = g; self.data[i + 2] = b; self.data[i + 3] = 255;
    }
    fn blit(&mut self, wbuf: &WorldBuffer, sx: i32, sy: i32, sw: u32, sh: u32, dx: i32, dy: i32) {
        for row in 0..sh as i32 {
            for col in 0..sw as i32 {
                let px = wbuf.get_pixel(sx + col, sy + row);
                self.set(dx + col, dy + row, px.r, px.g, px.b);
            }
        }
    }
}

fn draw_label(canvas: &mut Canvas, x: i32, y: i32, text: &str) {
    let mut cx = x;
    for ch in text.chars() {
        for (row, &bits) in char_bits(ch).iter().enumerate() {
            for col in 0u8..5 {
                if bits & (1 << (4 - col)) != 0 {
                    canvas.set(cx + col as i32, y + row as i32, 210, 215, 230);
                }
            }
        }
        cx += 6;
    }
}

fn char_bits(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0b01110,0b10001,0b10001,0b11111,0b10001,0b10001,0b10001],
        'B' => [0b11110,0b10001,0b10001,0b11110,0b10001,0b10001,0b11110],
        'C' => [0b01110,0b10001,0b10000,0b10000,0b10000,0b10001,0b01110],
        'D' => [0b11110,0b10001,0b10001,0b10001,0b10001,0b10001,0b11110],
        'E' => [0b11111,0b10000,0b10000,0b11110,0b10000,0b10000,0b11111],
        'F' => [0b11111,0b10000,0b10000,0b11110,0b10000,0b10000,0b10000],
        'G' => [0b01110,0b10001,0b10000,0b10111,0b10001,0b10001,0b01111],
        'H' => [0b10001,0b10001,0b10001,0b11111,0b10001,0b10001,0b10001],
        'I' => [0b01110,0b00100,0b00100,0b00100,0b00100,0b00100,0b01110],
        'J' => [0b00111,0b00010,0b00010,0b00010,0b00010,0b10010,0b01100],
        'K' => [0b10001,0b10010,0b10100,0b11000,0b10100,0b10010,0b10001],
        'L' => [0b10000,0b10000,0b10000,0b10000,0b10000,0b10000,0b11111],
        'M' => [0b10001,0b11011,0b10101,0b10101,0b10001,0b10001,0b10001],
        'N' => [0b10001,0b11001,0b10101,0b10101,0b10011,0b10001,0b10001],
        'O' => [0b01110,0b10001,0b10001,0b10001,0b10001,0b10001,0b01110],
        'P' => [0b11110,0b10001,0b10001,0b11110,0b10000,0b10000,0b10000],
        'Q' => [0b01110,0b10001,0b10001,0b10001,0b10101,0b10010,0b01101],
        'R' => [0b11110,0b10001,0b10001,0b11110,0b10100,0b10010,0b10001],
        'S' => [0b01111,0b10000,0b10000,0b01110,0b00001,0b00001,0b11110],
        'T' => [0b11111,0b00100,0b00100,0b00100,0b00100,0b00100,0b00100],
        'U' => [0b10001,0b10001,0b10001,0b10001,0b10001,0b10001,0b01110],
        'V' => [0b10001,0b10001,0b10001,0b10001,0b10001,0b01010,0b00100],
        'W' => [0b10001,0b10001,0b10001,0b10101,0b10101,0b11011,0b10001],
        'X' => [0b10001,0b10001,0b01010,0b00100,0b01010,0b10001,0b10001],
        'Y' => [0b10001,0b10001,0b01010,0b00100,0b00100,0b00100,0b00100],
        'Z' => [0b11111,0b00001,0b00010,0b00100,0b01000,0b10000,0b11111],
        '0' => [0b01110,0b10011,0b10101,0b10101,0b10101,0b11001,0b01110],
        '1' => [0b00100,0b01100,0b00100,0b00100,0b00100,0b00100,0b01110],
        '2' => [0b01110,0b10001,0b00001,0b00010,0b00100,0b01000,0b11111],
        '3' => [0b11110,0b00001,0b00001,0b01110,0b00001,0b00001,0b11110],
        '4' => [0b00010,0b00110,0b01010,0b10010,0b11111,0b00010,0b00010],
        '5' => [0b11111,0b10000,0b11110,0b00001,0b00001,0b10001,0b01110],
        '6' => [0b00110,0b01000,0b10000,0b11110,0b10001,0b10001,0b01110],
        '7' => [0b11111,0b00001,0b00010,0b00100,0b01000,0b01000,0b01000],
        '8' => [0b01110,0b10001,0b10001,0b01110,0b10001,0b10001,0b01110],
        '9' => [0b01110,0b10001,0b10001,0b01111,0b00001,0b00010,0b01100],
        ' ' => [0,0,0,0,0,0,0],
        _   => [0b11111,0b10001,0b10001,0b10001,0b10001,0b10001,0b11111],
    }
}

// ── Gallery ─────────────────────────────────────────────────────────────────

const CELL_W: u32 = 110;
const CELL_H: u32 = 150;
const HEADER: u32 = 26;
const ROW_LABEL: u32 = 16;
const COLS: usize = 16;

struct ThemeRow {
    name: &'static str,
    is_cavern: bool,
    template_id: u8,
    sprite_names: [&'static str; COLS],
}

fn main() {
    let rows: [ThemeRow; 3] = [
        ThemeRow {
            name: "PASTORAL", is_cavern: false, template_id: 0,
            sprite_names: ["FLOWER","MUSHROOM","MOSSY ROCK","FENCE POST","BUSH","SUNFLOWER","LOG","PEBBLES","HAY BALE","SCARECROW","WELL","WHEELBARROW","BEEHIVE","BIRDHOUSE","WATER CAN","PUMPKIN"],
        },
        ThemeRow {
            name: "RUGGED", is_cavern: false, template_id: 1,
            sprite_names: ["PINE TREE","BOULDER","CRATE","DEAD STUMP","BROKEN WALL","LICHEN ROCK","CAIRN","MENHIR","SIGNPOST","CAMPFIRE","CARTWHEEL","RAM SKULL","ANVIL","TOTEM POLE","FIREWOOD","FIREWOOD"],
        },
        ThemeRow {
            name: "UNDERGROUND", is_cavern: true, template_id: 0,
            sprite_names: ["CRYSTAL","BONE PILE","TORCH","SKULL","STALACTITE","CHAIN PILE","RIBCAGE","GLOW SHROOM","STALAGMITE","MINECART","GEODE","LANTERN","CHEST","ORE PICK","CANDLES","CANDLES"],
        },
    ];

    let img_w = CELL_W * COLS as u32;
    let img_h = (HEADER + ROW_LABEL + CELL_H) * rows.len() as u32 + HEADER;
    let mut canvas = Canvas::new(img_w, img_h, (18, 20, 30));
    draw_label(&mut canvas, 8, 8, "SCENERY GALLERY");

    let mut wbuf = WorldBuffer::new();

    for (ri, row) in rows.iter().enumerate() {
        let row_y0 = HEADER + ri as u32 * (HEADER + ROW_LABEL + CELL_H);
        draw_label(&mut canvas, 8, row_y0 as i32, row.name);

        let theme = Theme::of(row.is_cavern, row.template_id);
        let mut terrain = Terrain::empty();
        terrain.is_cavern = row.is_cavern;
        terrain.template_id = row.template_id;

        // Place one object per column, bottom-anchored within its cell, in a
        // fresh single-object terrain per sprite (draw_scenery draws every
        // object in terrain.scenery — one at a time keeps footprints from
        // overlapping between adjacent, differently-sized sprites). Always
        // drawn at the SAME fixed world anchor (draw_scenery positions sprites
        // at their absolute world x/y, not relative to a camera offset) so the
        // source rect blitted out of `wbuf` is identical every iteration.
        const ANCHOR_X: i32 = 200;
        const ANCHOR_Y: i32 = 120;
        for col in 0..COLS {
            terrain.scenery = vec![SceneryObject { x: ANCHOR_X as u32, y: ANCHOR_Y as u32, sprite: col as u8 }];

            wbuf.fill_rect(ANCHOR_X - CELL_W as i32 / 2, ANCHOR_Y - CELL_H as i32, CELL_W, CELL_H, arty::renderer::fb::Bgra::new(30, 33, 46));
            draw_scenery(&mut wbuf, &terrain, ANCHOR_X, ANCHOR_Y);

            let cell_x0 = col as u32 * CELL_W;
            let dst_x = cell_x0 as i32;
            let dst_y = (row_y0 + ROW_LABEL) as i32;
            canvas.blit(&wbuf, ANCHOR_X - CELL_W as i32 / 2, ANCHOR_Y - CELL_H as i32, CELL_W, CELL_H, dst_x, dst_y);

            let (hw, h) = SceneryObject { x: ANCHOR_X as u32, y: ANCHOR_Y as u32, sprite: col as u8 }.footprint(theme);
            draw_label(&mut canvas, dst_x + 2, dst_y + CELL_H as i32 - 22, row.sprite_names[col]);
            let dims = format!("{}x{}", hw * 2, h);
            draw_label(&mut canvas, dst_x + 2, dst_y + CELL_H as i32 - 12, &dims);
        }
    }

    let path = "assets/scenery_gallery.png";
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(file, img_w, img_h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&canvas.data).unwrap();
    println!("Wrote {path}  ({img_w}x{img_h})");
}
