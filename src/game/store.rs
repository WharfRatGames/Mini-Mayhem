use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str, draw_str_scaled, str_width, str_width_scaled};
use crate::renderer::cosmetic_sprites;
use crate::world::{SCREEN_W, SCREEN_H};

pub enum StoreAction { Back }

#[derive(Clone, PartialEq)]
enum Currency { Scrap, Warbond }

#[derive(Clone)]
struct StoreItem {
    cosm_type: &'static str,
    cosm_id:   u8,
    name:      &'static str,
    cost:      u32,
    currency:  Currency,
    owned:     bool,
    coming_soon: bool,
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
    ("hat",       29, "Mortarboard",  300),
    ("hat",       30, "Baseball Cap", 200),
    ("hat",       31, "Samurai Helm",        550),
    ("hat",       32, "Obsidian Crown",     1500),
    ("hat",       33, "Pharaoh Headdress",  1800),
    ("hat",       34, "Demon King Horns",   1600),
    ("hat",       35, "Astronaut Helmet",   1500),
    ("hat",       36, "Dragon Skull",       2000),
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
    ("gun_style", 14, "Revolver",    400),
    ("gun_style", 15, "Laser Pistol",500),
    ("gun_style", 16, "Gold Musket", 900),
    ("gun_style", 17, "Fusion Rifle",      650),
    ("gun_style", 18, "Obsidian Cannon", 1800),
    ("gun_style", 19, "Crystal Sniper",  1500),
    ("gun_style", 20, "Dragon's Breath", 2000),
    ("gun_style", 21, "Blood Revolver",  1600),
    ("gun_style", 22, "Thunder Rail",    1800),
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

// Premium warbond hats (id, name, cost_wb)
static WARBOND_HATS: &[(u8, &str, u32)] = &[
    (38, "Cosmic Crown",     150),
    (39, "Phoenix Crest",    120),
    (40, "Void Wraith Hood", 200),
    (41, "Gilded Jester",    100),
    (42, "Crimson War Mask", 200),
    (43, "Worm Hat",         100),
];

// Placeholder warbond packages — not purchasable yet
static WB_PACKAGES: &[(&str, u32)] = &[
    ("100 Warbonds",  100),
    ("500 Warbonds",  500),
    ("1200 Warbonds", 1200),
    ("2500 Warbonds", 2500),
];

#[derive(Clone, PartialEq)]
enum Tab { Scrap, Warbonds }

pub struct StoreScreen {
    token:      String,
    balance:    u32,
    warbonds:   u32,
    items:      Vec<StoreItem>,
    wb_items:   Vec<StoreItem>,
    tab:        Tab,
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
        warbonds: u32,
    ) -> Self {
        let items = Self::build_scrap_items(owned_hats, owned_guns, owned_uniforms, owned_boots);
        let wb_items = Self::build_wb_items(owned_hats, warbonds);
        Self { token, balance, warbonds, items, wb_items, tab: Tab::Scrap,
               cursor: 0, scroll: 0, status: String::new(), status_ttl: 0 }
    }

    fn build_scrap_items(oh: &[u8], og: &[u8], ou: &[u8], ob: &[u8]) -> Vec<StoreItem> {
        CATALOG.iter().map(|&(ct, cid, name, cost)| {
            let owned = match ct {
                "hat"       => oh.contains(&cid),
                "gun_style" => og.contains(&cid),
                "uniform"   => ou.contains(&cid),
                "boots"     => ob.contains(&cid),
                _           => false,
            };
            StoreItem { cosm_type: ct, cosm_id: cid, name, cost, currency: Currency::Scrap,
                        owned, coming_soon: false }
        }).collect()
    }

    fn build_wb_items(owned_hats: &[u8], _warbonds: u32) -> Vec<StoreItem> {
        let mut items: Vec<StoreItem> = WARBOND_HATS.iter().map(|&(id, name, cost)| {
            StoreItem { cosm_type: "hat", cosm_id: id, name, cost,
                        currency: Currency::Warbond,
                        owned: owned_hats.contains(&id),
                        coming_soon: false }
        }).collect();
        // Package placeholders
        for &(name, cost) in WB_PACKAGES {
            items.push(StoreItem { cosm_type: "package", cosm_id: 0, name, cost,
                                   currency: Currency::Warbond, owned: false, coming_soon: true });
        }
        items
    }

    pub fn set_profile(&mut self, scrap: u32, owned_hats: &[u8], owned_guns: &[u8],
                       owned_uniforms: &[u8], owned_boots: &[u8], warbonds: u32) {
        self.balance = scrap;
        self.warbonds = warbonds;
        for item in &mut self.items {
            item.owned = match item.cosm_type {
                "hat"       => owned_hats.contains(&item.cosm_id),
                "gun_style" => owned_guns.contains(&item.cosm_id),
                "uniform"   => owned_uniforms.contains(&item.cosm_id),
                "boots"     => owned_boots.contains(&item.cosm_id),
                _           => false,
            };
        }
        for item in &mut self.wb_items {
            if item.cosm_type == "hat" {
                item.owned = owned_hats.contains(&item.cosm_id);
            }
        }
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer) -> Option<StoreAction> {
        if input.just_pressed(Button::B) { return Some(StoreAction::Back); }

        // Tab switch with L/R
        if input.just_pressed(Button::L1) && self.tab != Tab::Scrap {
            self.tab = Tab::Scrap; self.cursor = 0; self.scroll = 0;
        }
        if input.just_pressed(Button::R1) && self.tab != Tab::Warbonds {
            self.tab = Tab::Warbonds; self.cursor = 0; self.scroll = 0;
        }

        let active = if self.tab == Tab::Scrap { &mut self.items } else { &mut self.wb_items };
        let n = active.len();
        const COLS: usize = 2;
        const ROWS: usize = 5;

        if input.just_pressed(Button::Up)    { if self.cursor >= COLS { self.cursor -= COLS; } }
        if input.just_pressed(Button::Down)  { if self.cursor + COLS < n { self.cursor += COLS; } }
        if input.just_pressed(Button::Left)  { if self.cursor > 0     { self.cursor -= 1; } }
        if input.just_pressed(Button::Right) { if self.cursor + 1 < n { self.cursor += 1; } }

        let cur_row = self.cursor / COLS;
        if cur_row < self.scroll { self.scroll = cur_row; }
        if cur_row >= self.scroll + ROWS { self.scroll = cur_row + 1 - ROWS; }

        if input.just_pressed(Button::A) {
            let item = &active[self.cursor];
            if item.coming_soon {
                self.status = "COMING SOON".to_string();
                self.status_ttl = 90;
            } else if item.owned {
                self.status = "ALREADY OWNED".to_string();
                self.status_ttl = 60;
            } else {
                let bal = if item.currency == Currency::Warbond { self.warbonds } else { self.balance };
                if bal < item.cost {
                    self.status = if item.currency == Currency::Warbond {
                        "NOT ENOUGH WARBONDS".to_string()
                    } else {
                        "NOT ENOUGH SCRAP".to_string()
                    };
                    self.status_ttl = 60;
                } else {
                    let ct = item.cosm_type;
                    let cid = item.cosm_id;
                    let cost = item.cost;
                    let is_wb = item.currency == Currency::Warbond;
                    match crate::game::account::shop_buy(&self.token, ct, cid) {
                        Ok(()) => {
                            if is_wb { self.warbonds = self.warbonds.saturating_sub(cost); }
                            else     { self.balance  = self.balance.saturating_sub(cost); }
                            let active2 = if self.tab == Tab::Scrap { &mut self.items } else { &mut self.wb_items };
                            active2[self.cursor].owned = true;
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
        }

        if self.status_ttl > 0 { self.status_ttl -= 1; }
        self.draw(buf);
        None
    }

    fn draw(&self, buf: &mut WorldBuffer) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;

        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H as u32, Bgra::new(8, 12, 28));

        // Header bar
        buf.fill_rect(0, 0, SCREEN_W, 26, Bgra::new(20, 25, 60));
        buf.fill_rect(0, 24, SCREEN_W, 2, Bgra::new(60, 80, 160));

        draw_str_scaled(buf, "STORE", 8, 6, Bgra::new(255, 220, 50), 2);

        // Balances top-right
        let wb_str  = format!("{} WB", self.warbonds);
        let sc_str  = format!("{} SC", self.balance);
        let wb_w = str_width_scaled(&wb_str, 2);
        let sc_w = str_width_scaled(&sc_str, 2);
        draw_str_scaled(buf, &wb_str, sw - wb_w - 8,        6, Bgra::new(255, 200, 60), 2);
        draw_str_scaled(buf, &sc_str, sw - wb_w - sc_w - 20, 6, Bgra::new(60, 220, 180), 2);

        // Tabs
        let tab_y = 28i32;
        let tab_h = 16i32;
        let tabs = [("SCRAP", Tab::Scrap), ("WARBONDS", Tab::Warbonds)];
        let mut tx = 0i32;
        for (label, t) in &tabs {
            let tw = str_width_scaled(label, 1) + 16;
            let active = self.tab == *t;
            let bg = if active { Bgra::new(40, 55, 120) } else { Bgra::new(15, 18, 45) };
            buf.fill_rect(tx, tab_y, tw as u32, tab_h as u32, bg);
            let tc = if active { Bgra::new(255, 220, 50) } else { Bgra::new(120, 130, 160) };
            draw_str(buf, label, tx + 8, tab_y + 4, tc);
            if active {
                buf.fill_rect(tx, tab_y + tab_h - 2, tw as u32, 2, Bgra::new(255, 180, 40));
            }
            tx += tw + 2;
        }

        // Item grid
        const COLS: usize = 2;
        const ROWS: usize = 5;
        let grid_y = tab_y + tab_h + 2;
        let cell_w = sw / COLS as i32;
        let cell_h = (sh - grid_y - 22) / ROWS as i32;

        let active = if self.tab == Tab::Scrap { &self.items } else { &self.wb_items };

        for row in 0..ROWS {
            let real_row = self.scroll + row;
            for col in 0..COLS {
                let idx = real_row * COLS + col;
                if idx >= active.len() { continue; }
                let item = &active[idx];
                let cx = col as i32 * cell_w;
                let cy = grid_y + row as i32 * cell_h;
                let selected = idx == self.cursor;

                let bg = if selected {
                    Bgra::new(30, 50, 100)
                } else if item.coming_soon {
                    Bgra::new(20, 18, 35)
                } else if item.owned {
                    Bgra::new(15, 35, 25)
                } else {
                    Bgra::new(12, 16, 38)
                };
                buf.fill_rect(cx + 2, cy + 2, (cell_w - 4) as u32, (cell_h - 4) as u32, bg);
                if selected {
                    let bc = if self.tab == Tab::Warbonds { Bgra::new(220, 160, 40) } else { Bgra::new(80, 120, 220) };
                    buf.fill_rect(cx + 2, cy + 2,              (cell_w - 4) as u32, 1, bc);
                    buf.fill_rect(cx + 2, cy + 2,              1, (cell_h - 4) as u32, bc);
                    buf.fill_rect(cx + 2, cy + cell_h - 5,     (cell_w - 4) as u32, 1, bc);
                    buf.fill_rect(cx + cell_w - 3, cy + 2,     1, (cell_h - 4) as u32, bc);
                }

                // Icon
                let icon_cx = cx + cell_w / 4;
                let icon_cy = cy + cell_h / 2;
                let icon_w  = cell_w / 2 - 8;
                let icon_h  = cell_h - 10;
                match item.cosm_type {
                    "hat" => {
                        let (iw, ih) = if item.cosm_id == 26 {
                            (icon_w / 3, icon_h / 3)
                        } else {
                            (icon_w, icon_h)
                        };
                        cosmetic_sprites::draw_hat(buf, item.cosm_id, icon_cx, icon_cy, iw, ih, false);
                    }
                    "gun_style" => {
                        cosmetic_sprites::draw_gun(buf, item.cosm_id, icon_cx, icon_cy, icon_w, icon_h);
                    }
                    "uniform" => {
                        let col = uniform_swatch(item.cosm_id);
                        let sw2 = icon_w * 3 / 4;
                        let sh2 = icon_h / 2;
                        buf.fill_rect(icon_cx - sw2/2, icon_cy - sh2/2, sw2 as u32, sh2 as u32, col);
                    }
                    "boots" => {
                        cosmetic_sprites::draw_boot(buf, item.cosm_id, icon_cx, icon_cy, icon_w, icon_h, false);
                    }
                    "package" => {
                        // WB coin icon placeholder
                        let c = Bgra::new(255, 200, 40);
                        buf.fill_circle(icon_cx, icon_cy, 10, c);
                        buf.fill_circle(icon_cx, icon_cy,  8, Bgra::new(200, 150, 20));
                        draw_str(buf, "WB", icon_cx - 5, icon_cy - 4, Bgra::new(255, 240, 180));
                    }
                    _ => {}
                }

                // Text
                let ttx = cx + cell_w / 2 + 4;
                let nc = if selected { Bgra::new(255, 240, 120) } else { Bgra::new(200, 200, 220) };
                let tag = match item.cosm_type {
                    "hat"       => "HAT",
                    "gun_style" => "GUN",
                    "uniform"   => "UNIFORM",
                    "boots"     => "BOOTS",
                    "package"   => "PACK",
                    _           => "",
                };
                draw_str(buf, tag, ttx, cy + 6, Bgra::new(120, 140, 180));
                draw_str(buf, item.name, ttx, cy + 18, nc);

                if item.coming_soon {
                    draw_str(buf, "COMING SOON", ttx, cy + cell_h - 16, Bgra::new(160, 120, 60));
                } else if item.owned {
                    draw_str(buf, "OWNED", ttx, cy + cell_h - 16, Bgra::new(60, 200, 120));
                } else {
                    let (cost_str, bal, cc_ok, cc_no) = if item.currency == Currency::Warbond {
                        (format!("{} WB", item.cost), self.warbonds,
                         Bgra::new(255, 200, 60), Bgra::new(180, 60, 60))
                    } else {
                        (format!("{} SC", item.cost), self.balance,
                         Bgra::new(60, 220, 180), Bgra::new(180, 60, 60))
                    };
                    let cc = if bal >= item.cost { cc_ok } else { cc_no };
                    draw_str(buf, &cost_str, ttx, cy + cell_h - 16, cc);
                }
            }
        }

        // Scroll indicators
        if self.scroll > 0 {
            draw_str(buf, "^", sw - 12, grid_y, Bgra::new(180, 180, 220));
        }
        let total_rows = (active.len() + COLS - 1) / COLS;
        if self.scroll + ROWS < total_rows {
            draw_str(buf, "v", sw - 12, grid_y + ROWS as i32 * cell_h - 10, Bgra::new(180, 180, 220));
        }

        // Status bar
        buf.fill_rect(0, sh - 22, SCREEN_W, 22, Bgra::new(12, 14, 35));
        if self.status_ttl > 0 {
            let sc = if self.status.contains("PURCHASED") { Bgra::new(60, 220, 120) }
                     else if self.status.contains("SOON")  { Bgra::new(255, 200, 60) }
                     else if self.status.contains("OWNED") || self.status.contains("SCRAP") || self.status.contains("WARBOND") { Bgra::new(220, 100, 60) }
                     else { Bgra::new(220, 60, 60) };
            let sw2 = str_width(&self.status);
            draw_str(buf, &self.status, sw / 2 - sw2 / 2, sh - 16, sc);
        } else {
            crate::renderer::hud::draw_button_hints(buf, &[("A", "BUY"), ("B", "BACK"), ("L1/R1", "TAB")], 0, 0);
        }
    }
}

fn uniform_swatch(id: u8) -> Bgra {
    match id {
        1 => Bgra::new(60,  100, 50),
        2 => Bgra::new(160, 130, 80),
        3 => Bgra::new(20,  20,  20),
        4 => Bgra::new(230, 230, 230),
        5 => Bgra::new(30,  50,  120),
        _ => Bgra::new(80,  80,  80),
    }
}
