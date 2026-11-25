use crate::input::{InputState, Button};
use crate::renderer::WorldBuffer;

pub enum StoreAction { Back }

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
    ("gun_style", 14, "Six Shooter", 400),
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

/// Display name of any cosmetic, including ones outside the scrap catalog (warbond hats, exclusives).
pub fn cosmetic_name(cosm_type: &str, id: u8) -> String {
    match (cosm_type, id) {
        ("look", _) => return crate::renderer::soldier_looks::NAMES[(id as usize) % crate::renderer::soldier_looks::N_LOOKS].to_string(),
        ("hat", 0) => return "NONE".into(),
        ("uniform", 0) => return "TEAM COLOUR".into(),
        ("boots", 0) | ("gun_style", 0) => return "STANDARD".into(),
        ("hat", 37) => return "BEN'S BIRTHDAY HAT".into(),
        ("uniform", 6) => return "PINK CAMO".into(),
        ("uniform", 7) => return "GOLD PLATE".into(),
        _ => {}
    }
    if let Some(n) = catalog_name(cosm_type, id) { return n.to_uppercase(); }
    if cosm_type == "hat" {
        if let Some(&(_, n, _)) = WARBOND_HATS.iter().find(|e| e.0 == id) { return n.to_uppercase(); }
    }
    format!("#{}", id)
}

/// Store tabs: label and the catalog category ("wb" = warbond hats).
const TABS: [(&str, &str); 5] = [("HAT", "hat"), ("UNIFORM", "uniform"), ("BOOTS", "boots"), ("GUN", "gun_style"), ("WARBOND", "wb")];

pub struct StoreScreen {
    token:      String,
    balance:    u32,
    warbonds:   u32,
    /// Owned ids per category: hat, uniform, boots, gun_style.
    owned:      [Vec<u8>; 4],
    soldiers:   [(String, crate::game::shop_ui::Fit); 4],
    soldier:    usize,
    tab:        usize,
    /// Cursor and scroll per tab, so switching tabs keeps your place.
    cursor:     [usize; 5],
    first:      [usize; 5],
    status:     String,
    status_ttl: u32,
}

impl StoreScreen {
    /// `team`: the soldiers that try items on (L1/R1); the first roster, or a default team.
    pub fn new(
        token:   String,
        balance: u32,
        owned_hats:     &[u8],
        owned_guns:     &[u8],
        owned_uniforms: &[u8],
        owned_boots:    &[u8],
        warbonds: u32,
        team: &crate::game::account::Roster,
    ) -> Self {
        let mut s = Self { token, balance, warbonds, owned: Default::default(),
                           soldiers: crate::game::shop_ui::soldiers_of(team),
                           soldier: 0, tab: 0, cursor: [0; 5], first: [0; 5], status: String::new(), status_ttl: 0 };
        s.set_profile(balance, owned_hats, owned_guns, owned_uniforms, owned_boots, warbonds);
        s
    }

    pub fn set_profile(&mut self, scrap: u32, owned_hats: &[u8], owned_guns: &[u8],
                       owned_uniforms: &[u8], owned_boots: &[u8], warbonds: u32) {
        self.balance = scrap;
        self.warbonds = warbonds;
        self.owned = [owned_hats.to_vec(), owned_uniforms.to_vec(), owned_boots.to_vec(), owned_guns.to_vec()];
    }

    /// Items of a tab: (category worn as, id, cost, paid in warbonds).
    fn items(tab: usize) -> Vec<(&'static str, u8, u32, bool)> {
        match TABS[tab].1 {
            "wb" => WARBOND_HATS.iter().map(|&(id, _, c)| ("hat", id, c, true)).collect(),
            cat => CATALOG.iter().filter(|e| e.0 == cat).map(|&(ct, id, _, c)| (ct, id, c, false)).collect(),
        }
    }

    fn owns(&self, cat: &str, id: u8) -> bool {
        let i = match cat { "hat" => 0, "uniform" => 1, "boots" => 2, "gun_style" => 3, _ => return false };
        self.owned[i].contains(&id)
    }

    fn say(&mut self, msg: &str, ttl: u32) { self.status = msg.to_string(); self.status_ttl = ttl; }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer) -> Option<StoreAction> {
        if input.just_pressed(Button::B) { return Some(StoreAction::Back); }
        if input.just_pressed(Button::L1) { self.soldier = (self.soldier + 3) % 4; }
        if input.just_pressed(Button::R1) { self.soldier = (self.soldier + 1) % 4; }
        if input.just_pressed(Button::Left)  { self.tab = (self.tab + TABS.len() - 1) % TABS.len(); }
        if input.just_pressed(Button::Right) { self.tab = (self.tab + 1) % TABS.len(); }
        let items = Self::items(self.tab);
        let t = self.tab;
        if input.just_pressed(Button::Up)   && self.cursor[t] > 0 { self.cursor[t] -= 1; }
        if input.just_pressed(Button::Down) && self.cursor[t] + 1 < items.len() { self.cursor[t] += 1; }
        self.cursor[t] = self.cursor[t].min(items.len().saturating_sub(1));
        self.first[t] = crate::game::shop_ui::scroll_to(self.first[t], self.cursor[t], items.len());

        if input.just_pressed(Button::A) {
            if let Some(&(ct, cid, cost, wb)) = items.get(self.cursor[t]) {
                let bal = if wb { self.warbonds } else { self.balance };
                if self.owns(ct, cid) {
                    self.say("ALREADY OWNED", 60);
                } else if bal < cost {
                    self.say(if wb { "NOT ENOUGH WARBONDS" } else { "NOT ENOUGH SCRAP" }, 60);
                } else {
                    match crate::game::account::shop_buy(&self.token, ct, cid) {
                        Ok(()) => {
                            if wb { self.warbonds = self.warbonds.saturating_sub(cost); }
                            else  { self.balance  = self.balance.saturating_sub(cost); }
                            let i = match ct { "hat" => 0, "uniform" => 1, "boots" => 2, _ => 3 };
                            self.owned[i].push(cid);
                            let msg = format!("BOUGHT {}!", cosmetic_name(ct, cid));
                            self.say(&msg, 90);
                        }
                        Err(e) => { let m = e.to_uppercase(); self.say(&m, 90); }
                    }
                }
            }
        }

        if self.status_ttl > 0 { self.status_ttl -= 1; }
        self.draw(buf);
        None
    }

    pub fn draw(&self, buf: &mut WorldBuffer) {
        use crate::game::shop_ui::{self as ui, Tile, ShopView};
        let t = self.tab;
        let tiles: Vec<Tile> = Self::items(t).into_iter().map(|(ct, id, cost, wb)| {
            let (tag, tag_col) = if self.owns(ct, id) { ("OWNED".to_string(), ui::GREEN) } else {
                let bal = if wb { self.warbonds } else { self.balance };
                (format!("{} {}", cost, if wb { "WARBONDS" } else { "SCRAP" }),
                 if bal >= cost { crate::renderer::hud::SIDE_SCRAP } else { ui::RED })
            };
            Tile { cat: ct, id, name: cosmetic_name(ct, id), tag, tag_col }
        }).collect();
        let info = format!("SCRAP {}   WARBONDS {}", self.balance, self.warbonds);
        let tabs: Vec<&str> = TABS.iter().map(|e| e.0).collect();
        let status = (self.status_ttl > 0).then(|| {
            let col = if self.status.starts_with("BOUGHT") { ui::GREEN } else { ui::RED };
            (self.status.as_str(), col)
        });
        ui::draw(buf, &ShopView {
            title: "STORE", info: &info,
            hints: &[("L1/R1", "SOLDIER"), ("LEFT/RIGHT", "CATEGORY"), ("UP/DOWN", "BROWSE"), ("A", "BUY"), ("B", "BACK")],
            soldiers: &self.soldiers, cur: self.soldier, tabs: &tabs, tab: t, tiles: &tiles,
            first: self.first[t], sel: Some(self.cursor[t]), what: "FOR SALE", status,
        });
    }

    /// Preview / test hook: jump to a soldier, tab and item.
    pub fn set_view(&mut self, soldier: usize, tab: usize, cursor: usize) {
        self.soldier = soldier % 4;
        self.tab = tab % TABS.len();
        let n = Self::items(self.tab).len();
        self.cursor[self.tab] = cursor.min(n.saturating_sub(1));
        self.first[self.tab] = crate::game::shop_ui::scroll_to(0, self.cursor[self.tab], n);
    }

    pub fn tab_len(tab: usize) -> usize { Self::items(tab).len() }
    pub const TABS: usize = TABS.len();
}

#[cfg(test)]
mod nav_tests {
    use super::*;
    use crate::input::{InputState, Button};
    use crate::game::account::{CosmeticsScreen, Roster};

    fn press(b: Button) -> InputState { let mut i = InputState::new(); i.inject_press(b); i }

    #[test]
    fn down_moves_the_store_cursor_and_equip_item() {
        let mut buf = WorldBuffer::new();
        let r = Roster::default_named(0);
        let mut st = StoreScreen::new(String::new(), 1015, &[], &[], &[], &[], 0, &r);
        st.update(&press(Button::Down), &mut buf);
        assert_eq!(st.cursor[0], 1, "store DOWN");
        st.update(&press(Button::Right), &mut buf);
        assert_eq!(st.tab, 1, "store RIGHT");
        let mut eq = CosmeticsScreen::new(r.clone(), vec![1, 2, 3], vec![], vec![], vec![], String::new(), 0);
        let before = r.hat_ids[0];
        let out = eq.update(&press(Button::Down), &mut buf);
        assert!(out.is_none());
        match eq.update(&press(Button::Start), &mut buf) {
            Some(crate::game::account::CosmeticsAction::Saved(r2)) => assert_ne!(r2.hat_ids[0], before, "equip DOWN"),
            _ => panic!("no save"),
        }
    }
}
