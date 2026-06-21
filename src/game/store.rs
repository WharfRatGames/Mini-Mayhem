use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
use crate::renderer::cosmetic_sprites;
use crate::world::{SCREEN_W, SCREEN_H};

pub enum StoreAction { Back }

#[derive(Clone)]
struct StoreItem {
    cosm_type: &'static str,
    cosm_id:   u8,
    name:      &'static str,
    cost:      u32,
    owned:     bool,
}

pub static CATALOG: &[(&str, u8, &str, u32)] = &[
    // Hats (scrap only)
    ("hat",       1, "Top Hat",       200),
    ("hat",       2, "Propeller Hat", 350),
    ("hat",       3, "Flower",        150),
    ("hat",       4, "Crown",         400),
    ("hat",       5, "Fez",           250),
    ("hat",       6, "Beret",         200),
    ("hat",       7, "Party Hat",     200),
    ("hat",       8, "Halo",          500),
    ("hat",       9, "Devil Horns",   500),
    ("hat",       10, "Wizard Hat",   650),
    ("hat",       11, "Ninja Band",   450),
    ("hat",       12, "Blue Party Hat", 200),
    ("hat",       13, "Cowboy Hat",   350),
    ("hat",       14, "Pirate Hat",   500),
    ("hat",       15, "Viking Helm",  550),
    ("hat",       16, "Beanie",       150),
    ("hat",       17, "Bandana",      150),
    ("hat",       18, "Angel Ring",   500),
    ("hat",       19, "Horn Nubs",    450),
    ("hat",       20, "Laurel Wreath",350),
    ("hat",       21, "Party Hat 2",  200),
    ("hat",       22, "Pirate Tricorn",500),
    ("hat",       23, "Mohawk",       300),
    ("hat",       24, "Bow",          200),
    ("hat",       25, "Frontier Hat", 350),
    ("hat",       26, "War Helm",     500),
    ("hat",       27, "Sombrero",     300),
    ("hat",       28, "Luchador Mask", 600),
    // Gun styles (scrap only)
    ("gun_style", 1, "Pistol",        200),
    ("gun_style", 2, "Shotgun",       300),
    ("gun_style", 3, "Sniper",        400),
    ("gun_style", 4, "Minigun",       500),
    ("gun_style", 5, "Cannon",        500),
    ("gun_style", 6, "Plasma Gun",    600),
    ("gun_style", 7, "Golden Gun",    750),
    ("gun_style", 8, "Revolver",      350),
    ("gun_style", 9, "Flamethrower",  650),
    ("gun_style", 10, "Rocket Launcher", 800),
    ("gun_style", 11, "SMG",           350),
    ("gun_style", 12, "Flintlock",    500),
    ("gun_style", 13, "Crossbow",     600),
    // Uniforms (scrap only)
    ("uniform",   1, "Camo Green",    200),
    ("uniform",   2, "Desert Tan",    200),
    ("uniform",   3, "Midnight Black",300),
    ("uniform",   4, "Snow White",    300),
    ("uniform",   5, "Navy",          250),
    // Boots (scrap only)
    ("boots",     1, "Red",           100),
    ("boots",     2, "White",         100),
    ("boots",     3, "Gold",          150),
    ("boots",     4, "Combat Green",  100),
    ("boots",     5, "Electric Blue", 150),
];

/// Look up the display name for a cosmetic by type+id from the store catalog.
pub fn catalog_name(cosm_type: &str, id: u8) -> Option<&'static str> {
    CATALOG.iter()
        .find(|&&(ct, cid, _, _)| ct == cosm_type && cid == id)
        .map(|&(_, _, name, _)| name)
}

pub struct StoreScreen {
    token:      String,
    balance:    u32,
    items:      Vec<StoreItem>,
    cursor:     usize,
    scroll:     usize,
    status:     String,
    status_ttl: u32,
}

impl StoreScreen {
    pub fn new(
        token:   String,
        balance: u32,
        owned_hats:     &[u8],
        owned_guns:     &[u8],
        owned_uniforms: &[u8],
        owned_boots:    &[u8],
    ) -> Self {
        let items = CATALOG.iter().map(|&(ct, cid, name, cost)| {
            let owned = match ct {
                "hat"       => owned_hats.contains(&cid),
                "gun_style" => owned_guns.contains(&cid),
                "uniform"   => owned_uniforms.contains(&cid),
                "boots"     => owned_boots.contains(&cid),
                _           => false,
            };
            StoreItem { cosm_type: ct, cosm_id: cid, name, cost, owned }
        }).collect();
        Self { token, balance, items, cursor: 0, scroll: 0, status: String::new(), status_ttl: 0 }
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer) -> Option<StoreAction> {
        // Input
        if input.just_pressed(Button::B) { return Some(StoreAction::Back); }

        let n = self.items.len();
        const COLS: usize = 2;
        const ROWS: usize = 5; // visible rows

        if input.just_pressed(Button::Up) {
            if self.cursor >= COLS { self.cursor -= COLS; }
        }
        if input.just_pressed(Button::Down) {
            if self.cursor + COLS < n { self.cursor += COLS; }
        }
        if input.just_pressed(Button::Left)  && self.cursor > 0     { self.cursor -= 1; }
        if input.just_pressed(Button::Right) && self.cursor + 1 < n { self.cursor += 1; }

        // Scroll to keep cursor visible
        let cur_row = self.cursor / COLS;
        if cur_row < self.scroll { self.scroll = cur_row; }
        if cur_row >= self.scroll + ROWS { self.scroll = cur_row + 1 - ROWS; }

        // Buy on A
        if input.just_pressed(Button::A) {
            let item = &self.items[self.cursor];
            if item.owned {
                self.status = "ALREADY OWNED".to_string();
                self.status_ttl = 60;
            } else if self.balance < item.cost {
                self.status = "NOT ENOUGH SCRAP".to_string();
                self.status_ttl = 60;
            } else {
                let ct = item.cosm_type;
                let cid = item.cosm_id;
                let cost = item.cost;
                match crate::game::account::shop_buy(&self.token, ct, cid) {
                    Ok(()) => {
                        self.balance = self.balance.saturating_sub(cost);
                        self.items[self.cursor].owned = true;
                        self.status = "PURCHASED!".to_string();
                        self.status_ttl = 90;
                    }
                    Err(e) => {
                        self.status = e.to_uppercase();
                        self.status_ttl = 90;
                    }
                }
            }
        }

        if self.status_ttl > 0 { self.status_ttl -= 1; }

        self.draw(buf);
        None
    }

    fn draw(&self, buf: &mut WorldBuffer) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;

        // Background
        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(8, 12, 28));

        // Header
        buf.fill_rect(0, 0, SCREEN_W, 26, Bgra::new(20, 25, 60));
        buf.fill_rect(0, 24, SCREEN_W, 2, Bgra::new(60, 80, 160));

        let title = "STORE";
        draw_str_scaled(buf, title, 8, 6, Bgra::new(255, 220, 50), 2);

        let bal_str = format!("SCRAP: {}", self.balance);
        let bw = str_width_scaled(&bal_str, 2);
        draw_str_scaled(buf, &bal_str, sw - bw - 8, 6, Bgra::new(60, 220, 180), 2);

        // Item grid
        const COLS: usize = 2;
        const ROWS: usize = 5;
        let cell_w = sw / COLS as i32;
        let cell_h = (sh - 50) / ROWS as i32; // 50 = header(26) + status(24)
        let grid_y = 28;

        for row in 0..ROWS {
            let real_row = self.scroll + row;
            for col in 0..COLS {
                let idx = real_row * COLS + col;
                if idx >= self.items.len() { continue; }
                let item = &self.items[idx];
                let cx = col as i32 * cell_w;
                let cy = grid_y + row as i32 * cell_h;
                let selected = idx == self.cursor;

                // Cell background
                let bg = if selected {
                    Bgra::new(30, 50, 100)
                } else if item.owned {
                    Bgra::new(15, 35, 25)
                } else {
                    Bgra::new(12, 16, 38)
                };
                buf.fill_rect(cx + 2, cy + 2, (cell_w - 4) as u32, (cell_h - 4) as u32, bg);
                if selected {
                    buf.fill_rect(cx + 2, cy + 2, (cell_w - 4) as u32, 1, Bgra::new(80, 120, 220));
                    buf.fill_rect(cx + 2, cy + 2, 1, (cell_h - 4) as u32, Bgra::new(80, 120, 220));
                    buf.fill_rect(cx + 2, cy + cell_h - 5, (cell_w - 4) as u32, 1, Bgra::new(80, 120, 220));
                    buf.fill_rect(cx + cell_w - 3, cy + 2, 1, (cell_h - 4) as u32, Bgra::new(80, 120, 220));
                }

                // Icon area: left ~half of cell
                let icon_cx = cx + cell_w / 4;
                let icon_cy = cy + cell_h / 2;
                let icon_w  = cell_w / 2 - 8;
                let icon_h  = cell_h - 10;
                match item.cosm_type {
                    "hat" => {
                        cosmetic_sprites::draw_hat(buf, item.cosm_id, icon_cx, icon_cy, icon_w, icon_h);
                    }
                    "gun_style" => {
                        cosmetic_sprites::draw_gun(buf, item.cosm_id, icon_cx, icon_cy, icon_w, icon_h);
                    }
                    "uniform" => {
                        let col = uniform_swatch(item.cosm_id);
                        let sw = icon_w * 3 / 4;
                        let sh = icon_h / 2;
                        buf.fill_rect(icon_cx - sw/2, icon_cy - sh/2, sw as u32, sh as u32, col);
                        buf.fill_rect(icon_cx - sw/2 - 1, icon_cy - sh/2 - 1, sw as u32 + 2, 1, Bgra::new(60,70,100));
                        buf.fill_rect(icon_cx - sw/2 - 1, icon_cy + sh/2,     sw as u32 + 2, 1, Bgra::new(60,70,100));
                        buf.fill_rect(icon_cx - sw/2 - 1, icon_cy - sh/2 - 1, 1, sh as u32 + 2, Bgra::new(60,70,100));
                        buf.fill_rect(icon_cx + sw/2,     icon_cy - sh/2 - 1, 1, sh as u32 + 2, Bgra::new(60,70,100));
                    }
                    "boots" => {
                        cosmetic_sprites::draw_boot(buf, item.cosm_id, icon_cx, icon_cy, icon_w, icon_h, false);
                    }
                    _ => {}
                }

                // Text area: right half
                let tx = cx + cell_w / 2 + 4;
                let nc = if selected { Bgra::new(255, 240, 120) } else { Bgra::new(200, 200, 220) };
                // Category tag
                let tag = match item.cosm_type {
                    "hat"       => "HAT",
                    "gun_style" => "GUN",
                    "uniform"   => "UNIFORM",
                    "boots"     => "BOOTS",
                    _           => "",
                };
                draw_str(buf, tag, tx, cy + 6, Bgra::new(120, 140, 180));
                draw_str(buf, item.name, tx, cy + 18, nc);

                // Cost / owned state
                if item.owned {
                    draw_str(buf, "OWNED", tx, cy + cell_h - 16, Bgra::new(60, 200, 120));
                } else {
                    let cost_str = format!("{} SC", item.cost);
                    let cc = if self.balance >= item.cost { Bgra::new(60, 220, 180) } else { Bgra::new(180, 60, 60) };
                    draw_str(buf, &cost_str, tx, cy + cell_h - 16, cc);
                }
            }
        }

        // Scroll indicators
        if self.scroll > 0 {
            draw_str(buf, "^", sw - 12, grid_y, Bgra::new(180, 180, 220));
        }
        let total_rows = (self.items.len() + COLS - 1) / COLS;
        if self.scroll + ROWS < total_rows {
            draw_str(buf, "v", sw - 12, grid_y + ROWS as i32 * cell_h - 10, Bgra::new(180, 180, 220));
        }

        // Status bar at bottom
        buf.fill_rect(0, sh - 22, SCREEN_W, 22, Bgra::new(12, 14, 35));
        if self.status_ttl > 0 {
            let sc = if self.status.contains("PURCHASED") { Bgra::new(60, 220, 120) }
                     else if self.status.contains("OWNED") || self.status.contains("SCRAP") { Bgra::new(220, 100, 60) }
                     else { Bgra::new(220, 60, 60) };
            let sw2 = str_width(&self.status);
            draw_str(buf, &self.status, sw / 2 - sw2 / 2, sh - 16, sc);
        } else {
            crate::renderer::hud::draw_button_hints(buf, &[("A", "BUY"), ("B", "BACK")], 0);
        }
    }
}

fn uniform_swatch(id: u8) -> Bgra {
    match id {
        1 => Bgra::new(60,  100, 50),   // Camo Green
        2 => Bgra::new(160, 130, 80),   // Desert Tan
        3 => Bgra::new(20,  20,  20),   // Midnight Black
        4 => Bgra::new(230, 230, 230),  // Snow White
        5 => Bgra::new(30,  50,  120),  // Navy
        _ => Bgra::new(80,  80,  80),
    }
}

