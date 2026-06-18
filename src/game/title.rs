use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str, draw_str_scaled, draw_str_shadow_scaled, str_width, str_width_scaled};
use crate::world::{SCREEN_W, SCREEN_H};

pub type TitleChoice = u8;
pub const CHOICE_QUIT:         u8 = 1;
pub const CHOICE_LIVE:         u8 = 2;  // casual live (existing)
pub const CHOICE_TAKE_A_TURN:  u8 = 3;
pub const CHOICE_MULTI:        u8 = 4;
pub const CHOICE_HOTSEAT:      u8 = 5;
pub const CHOICE_VS_CPU:       u8 = 6;
pub const CHOICE_SP:           u8 = 7;
pub const CHOICE_MY_TEAM:      u8 = 8;
pub const CHOICE_LIVE_RANKED:  u8 = 9;
pub const CHOICE_TAT_RANKED:   u8 = 10;
pub const CHOICE_LIVE_STATS:         u8 = 11;
pub const CHOICE_TAT_STATS:          u8 = 12;
pub const CHOICE_TEST:               u8 = 13;  // all-weapons infinite-ammo hotseat
pub const CHOICE_LEADERBOARD_CASUAL: u8 = 14;
pub const CHOICE_LEADERBOARD_RANKED: u8 = 15;
pub const CHOICE_MISSIONS:           u8 = 16;
pub const CHOICE_SETTINGS:           u8 = 17;

const ITEMS:      &[&str] = &["SINGLEPLAYER", "MULTIPLAYER", "MY TEAMS", "SETTINGS", "HOW TO PLAY", "QUIT"];
const SP_ITEMS:   &[&str] = &["VS CPU", "HOTSEAT", "TEST"];
const MP_ITEMS:   &[&str] = &["LIVE GAME", "TAKE A TURN", "MISSIONS"];
const LIVE_ITEMS: &[&str] = &["CASUAL", "RANKED", "LEADERBOARD", "STATS"];
const TAT_ITEMS:  &[&str] = &["CASUAL", "RANKED", "LEADERBOARD", "STATS"];

// Max items visible in the panel before scrolling kicks in.
const MAX_VISIBLE: usize = 4;

const HOW_TO_PAGES: &[HelpPage] = &[
    HelpPage {
        title: "CONTROLS",
        lines: &[
            "D-PAD LEFT/RIGHT   Move",
            "D-PAD UP/DOWN      Aim angle",
            "HOLD A + RELEASE   Charge and fire (most weapons)",
            "B                  Jump forward",
            "Y                  Backflip",
            "SELECT             Weapon menu",
            "START              Pause",
            "",
            "R1 + D-PAD         Pan camera (snaps back)",
            "L1 + D-PAD         Pan camera (stays put)",
            "",
            "WEAPON MENU",
            "  D-PAD            Browse weapons",
            "  A  Confirm   B / SELECT  Cancel",
            "  L1/R1  Adjust grenade fuse",
        ],
    },
    HelpPage {
        title: "WEAPON CONTROLS",
        lines: &[
            "GRAPPLE HOOK",
            "  A                Fire / Release / Re-rope",
            "  UP / DOWN        Shorten / Lengthen rope",
            "  LEFT / RIGHT     Swing force (while attached)",
            "",
            "REVOLVER  1 powerful hitscan shot per turn.",
            "  A                Fire",
            "",
            "SHOTGUN  5-pellet spray, 2 shots per turn.",
            "  A                Fire",
            "",
            "GRENADE  L1 / R1  Fuse: 1 to 5 seconds",
            "",
            "TNT / MINE / BAT: press A to place/swing",
            "",
            "PLASMA TORCH",
            "  HOLD A            Burn (release to stop)",
            "  UP                Aim up-forward (~35 deg)",
            "  DOWN              Aim down-forward (~35 deg)",
        ],
    },
    HelpPage {
        title: "HOW TO PLAY",
        lines: &[
            "MINI MAYHEM — turn-based artillery.",
            "Move, aim, fire. Last team standing wins.",
            "",
            "TURN RULES",
            "  Retreat phase after firing — move to safety.",
            "  Taking damage ends your turn immediately.",
            "  Timer pauses while charging a shot.",
            "",
            "Soldier names and HP shown above each unit.",
            "",
            "CRATES",
            "  Walk over to collect. Health = +25 HP.",
            "  Weapon crates give a random weapon.",
            "  Teal crates = SCRAP (multiplayer only).",
            "    Spend scrap in MY TEAMS -> STORE.",
            "",
            "Reduce all enemies to 0 HP to win!",
        ],
    },
    HelpPage {
        title: "WEAPONS",
        lines: &[
            "BAZOOKA  Wind-affected rocket. 50 dmg direct.",
            "         Blast radius 45px. Overcharge for speed.",
            "",
            "GRENADE  Bounces off terrain. Not wind-affected.",
            "         45 dmg. Blast radius 30px.",
            "         Fuse 1-5 seconds (L1/R1 to adjust).",
            "",
            "SHOTGUN  5 pellets, 5 dmg each = 25 max/shot.",
            "         Range 220px. Recoil moves the shooter.",
            "         2 shots per turn.",
            "",
            "TNT  Placed. Random 4-5 second fuse.",
            "     112 dmg. Blast radius 75px.",
            "     Locked until 5 full turn rotations.",
            "",
            "LANDMINE  Placed. Arms after 3 seconds.",
            "          Proximity triggered. 50 dmg. 2 uses.",
        ],
    },
    HelpPage {
        title: "MORE WEAPONS",
        lines: &[
            "GRAPPLE HOOK  Free movement. 3 uses + crates.",
            "              Does NOT end your turn.",
            "              1 charge consumed per turn used.",
            "",
            "BASEBALL BAT  1 use per loadout + crates.",
            "              30 dmg + spinning knockback.",
            "              28px reach. Locked 3 full cycles.",
            "",
            "REVOLVER  Hitscan. 6 shots per turn. Crate-only.",
            "          15 dmg + knockback. Range 800px.",
            "",
            "METEOR BOMB  Crate-only. Explodes on terrain hit.",
            "             Scatters 5 fragments. 18 dmg each.",
            "             Not wind-affected.",
        ],
    },
    HelpPage {
        title: "SPECIAL WEAPONS",
        lines: &[
            "BLASTHIVE  Thrown hive. Crate-only.",
            "           Spawns 6 homing bees on impact.",
            "           12 dmg per sting. Seeks nearest soldier.",
            "",
            "BLACK HOLE  Crate-only. Detonates on impact.",
            "            Pulls soldiers, barrels, projectiles",
            "            within 108px. Collapses after 5s: 35 dmg.",
            "",
            "PLASMA TORCH  3 per loadout + crate finds.",
            "              Hold A to burn. UP/DOWN changes angle.",
            "              Tunnels terrain. 1 charge, 4 sec max.",
            "",
            "HAND OF JERRY  Ultra-rare crate-only (~3% drop).",
            "               Cursor appears on selection.",
            "               LEFT/RIGHT aim column. A to drop.",
            "               Smashes + bounces until water. 85 dmg.",
        ],
    },
    HelpPage {
        title: "GAME MODES",
        lines: &[
            "VS CPU  Fight an AI opponent solo.",
            "",
            "HOTSEAT  Two players share one device.",
            "",
            "TEST  All weapons, infinite ammo, hotseat.",
            "      Good for trying new weapons.",
            "",
            "LIVE GAME  Real-time multiplayer.",
            "  CASUAL   Direct connect, no ELO.",
            "  RANKED   ELO matchmaking.",
            "",
            "TAKE A TURN (TAT)  Async — play at your pace.",
            "  CASUAL   Matchmake or join by code.",
            "  RANKED   Closest ELO opponent.",
            "  14-day turn timer. Up to 15 active games.",
        ],
    },
    HelpPage {
        title: "COSMETICS",
        lines: &[
            "Customize your soldiers' look with cosmetics.",
            "MY TEAMS -> EQUIP to dress up your roster.",
            "",
            "TYPES",
            "  HAT      Headwear drawn above the soldier.",
            "  UNIFORM  Body and arm color override.",
            "  BOOTS    Leg color override.",
            "  GUN      Weapon shape for cosmetic guns.",
            "           (Active weapon overrides for combat.)",
            "",
            "Cosmetics are locked per match at the start.",
            "EQUIP changes only apply to new matches.",
            "",
            "EARNING SCRAP — all ways to get scrap:",
            "  Win a match          +75",
            "  Lose a match         +25",
            "  Scrap crate (MP)     +5 to +30",
            "  Daily login bonus    +25",
            "  7-day streak bonus   +150 extra",
            "  Daily missions       +30 / +60 / +45",
            "  Weekly missions      +150 / +250 / +200",
            "  Spend in MY TEAMS -> STORE.",
        ],
    },
    HelpPage {
        title: "MISSIONS",
        lines: &[
            "MULTIPLAYER -> MISSIONS to view active challenges.",
            "Complete them to earn bonus SCRAP.",
            "",
            "DAILY CHALLENGES  Reset every day.",
            "  Play any match         +30 scrap",
            "  Win a match            +60 scrap",
            "  Get 3 kills            +45 scrap",
            "",
            "WEEKLY CHALLENGES  Reset every week.",
            "  Play 5 matches        +150 scrap",
            "  Win 3 matches         +250 scrap",
            "  Get 10 kills          +200 scrap",
            "",
            "Progress is tracked across TAT and LIVE modes.",
            "Press A on a complete challenge to claim scrap.",
            "",
            "LOGIN BONUS  Earned once per day on TAT/Live.",
            "  Daily login    +25 scrap",
            "  7-day streak   +150 bonus on top of daily",
        ],
    },
    HelpPage {
        title: "RANKS & ELO",
        lines: &[
            "Every account starts at 1000 ELO.",
            "Only RANKED matches affect your ELO.",
            "",
            "RANKS",
            "  Recruit      under 800",
            "  Private      800  - 999",
            "  Corporal     1000 - 1199  (starting rank)",
            "  Sergeant     1200 - 1399",
            "  Lieutenant   1400 - 1599",
            "  Captain      1600 - 1799",
            "  Major        1800 - 1999",
            "  Commander    2000+",
            "",
            "Beating higher-ranked players earns more ELO.",
            "Win = gain.  Loss = lose.",
        ],
    },
    HelpPage {
        title: "TIPS",
        lines: &[
            "Wind changes every turn — watch the HUD arrow.",
            "GRENADE, METEOR BOMB, BLASTHIVE ignore wind.",
            "",
            "High ground limits enemy aim angles.",
            "Blow terrain out from under enemies for ring-outs.",
            "",
            "SHOTGUN: close range, 5 pellets, pushes enemies.",
            "Recoil also moves YOU — use it to reposition.",
            "",
            "GRAPPLE to reach high ground before firing.",
            "Release at the bottom of the swing for distance.",
            "",
            "REVOLVER: single shot, 19 dmg, 800px range.",
            "BLASTHIVE: throw at feet — bees swarm upward.",
            "BAT: always face target before swinging.",
            "",
            "PLASMA TORCH: 3 fixed angles — forward, up-fwd, down-fwd.",
            "Tunnel under enemies or burrow to escape.",
            "One charge per use. Release A to stop early.",
            "",
            "HAND OF JERRY: aim over a cluster of soldiers.",
            "Each bounce does 85 dmg. Bounces to water level.",
        ],
    },
];

fn scroll_for(cursor: usize, _n: usize) -> usize {
    cursor.saturating_sub(MAX_VISIBLE - 1)
}

struct HelpPage {
    title: &'static str,
    lines: &'static [&'static str],
}

#[derive(PartialEq)]
enum Sub { None, SP, MP, Live, Tat, HowToPlay }

pub struct TitleScreen {
    cursor:        usize,
    sub_cursor:    usize,
    scroll_offset: usize,
    sub:           Sub,
    help_page:     usize,
    tick:          u32,
    version:       &'static str,
}

impl TitleScreen {
    pub fn new(version: &'static str) -> Self {
        Self { cursor: 0, sub_cursor: 0, scroll_offset: 0, sub: Sub::None, help_page: 0, tick: 0, version }
    }

    pub fn continue_to_submenu(&mut self) {
        self.sub = Sub::MP;
        self.sub_cursor = 0;
    }

    pub fn continue_to_sp_submenu(&mut self) {
        self.sub = Sub::SP;
        self.sub_cursor = 0;
    }

    pub fn update(&mut self, input: &InputState, buf: &mut WorldBuffer) -> Option<TitleChoice> {
        self.tick = self.tick.wrapping_add(1);
        let n_pages = HOW_TO_PAGES.len();

        match self.sub {
            Sub::HowToPlay => {
                if input.just_pressed(Button::B) || input.just_pressed(Button::Start) {
                    self.sub = Sub::None;
                } else if input.just_pressed(Button::Right) || input.just_pressed(Button::R1) {
                    self.help_page = (self.help_page + 1) % n_pages;
                } else if input.just_pressed(Button::Left) || input.just_pressed(Button::L1) {
                    self.help_page = if self.help_page == 0 { n_pages - 1 } else { self.help_page - 1 };
                }
                self.draw_help(buf);
                return None;
            }
            Sub::MP => {
                let n = MP_ITEMS.len();
                if input.just_pressed(Button::Up)   { self.nav_up(n); }
                if input.just_pressed(Button::Down) { self.nav_down(n); }
                if input.just_pressed(Button::B)    { self.sub = Sub::None; self.scroll_offset = 0; }
                if input.just_pressed(Button::A) || input.just_pressed(Button::Start) {
                    match self.sub_cursor {
                        0 => { self.sub = Sub::Live; self.sub_cursor = 0; self.scroll_offset = 0; }
                        1 => { self.sub = Sub::Tat;  self.sub_cursor = 0; self.scroll_offset = 0; }
                        2 => return Some(CHOICE_MISSIONS),
                        _ => { self.sub = Sub::None; self.scroll_offset = 0; }
                    }
                }
            }
            Sub::Live => {
                let n = LIVE_ITEMS.len();
                if input.just_pressed(Button::Up)   { self.nav_up(n); }
                if input.just_pressed(Button::Down) { self.nav_down(n); }
                if input.just_pressed(Button::B)    { self.sub = Sub::MP; self.sub_cursor = 0; self.scroll_offset = 0; }
                if input.just_pressed(Button::A) || input.just_pressed(Button::Start) {
                    match self.sub_cursor {
                        0 => return Some(CHOICE_LIVE),
                        1 => return Some(CHOICE_LIVE_RANKED),
                        2 => return Some(CHOICE_LEADERBOARD_CASUAL),
                        3 => return Some(CHOICE_LIVE_STATS),
                        _ => { self.sub = Sub::MP; self.sub_cursor = 0; self.scroll_offset = 0; }
                    }
                }
            }
            Sub::Tat => {
                let n = TAT_ITEMS.len();
                if input.just_pressed(Button::Up)   { self.nav_up(n); }
                if input.just_pressed(Button::Down) { self.nav_down(n); }
                if input.just_pressed(Button::B)    { self.sub = Sub::MP; self.sub_cursor = 0; self.scroll_offset = 0; }
                if input.just_pressed(Button::A) || input.just_pressed(Button::Start) {
                    match self.sub_cursor {
                        0 => return Some(CHOICE_TAKE_A_TURN),
                        1 => return Some(CHOICE_TAT_RANKED),
                        2 => return Some(CHOICE_LEADERBOARD_CASUAL),
                        3 => return Some(CHOICE_TAT_STATS),
                        _ => { self.sub = Sub::MP; self.sub_cursor = 0; self.scroll_offset = 0; }
                    }
                }
            }
            Sub::SP => {
                let n = SP_ITEMS.len();
                if input.just_pressed(Button::Up)   { self.nav_up(n); }
                if input.just_pressed(Button::Down) { self.nav_down(n); }
                if input.just_pressed(Button::B)    { self.sub = Sub::None; self.scroll_offset = 0; }
                if input.just_pressed(Button::A) || input.just_pressed(Button::Start) {
                    return Some(match self.sub_cursor {
                        0 => CHOICE_VS_CPU,
                        1 => CHOICE_HOTSEAT,
                        2 => CHOICE_TEST,
                        _ => { self.sub = Sub::None; self.scroll_offset = 0; return None; }
                    });
                }
            }
            Sub::None => {
                let n = ITEMS.len();
                if input.just_pressed(Button::Up)   { self.cursor = if self.cursor == 0 { n-1 } else { self.cursor-1 }; self.scroll_offset = scroll_for(self.cursor, n); }
                if input.just_pressed(Button::Down) { self.cursor = (self.cursor+1) % n; self.scroll_offset = scroll_for(self.cursor, n); }
                if input.just_pressed(Button::A) || input.just_pressed(Button::Start) {
                    match self.cursor {
                        0 => return Some(CHOICE_SP),
                        1 => return Some(CHOICE_MULTI),
                        2 => return Some(CHOICE_MY_TEAM),
                        3 => return Some(CHOICE_SETTINGS),
                        4 => { self.sub = Sub::HowToPlay; self.help_page = 0; return None; }
                        _ => return Some(CHOICE_QUIT),
                    }
                }
            }
        }

        self.draw_menu(buf);
        None
    }

    fn nav_up(&mut self, n: usize) {
        self.sub_cursor = if self.sub_cursor == 0 { n - 1 } else { self.sub_cursor - 1 };
        self.scroll_offset = scroll_for(self.sub_cursor, n);
    }

    fn nav_down(&mut self, n: usize) {
        self.sub_cursor = (self.sub_cursor + 1) % n;
        self.scroll_offset = scroll_for(self.sub_cursor, n);
    }

    fn draw_menu(&self, buf: &mut WorldBuffer) {
        use crate::renderer::title_bg::draw_title_bg;
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;

        // Title background image
        draw_title_bg(buf, 0);

        // Which items and cursor to show
        let (items, cursor) = match self.sub {
            Sub::MP         => (MP_ITEMS   as &[&str], self.sub_cursor),
            Sub::SP         => (SP_ITEMS   as &[&str], self.sub_cursor),
            Sub::Live       => (LIVE_ITEMS as &[&str], self.sub_cursor),
            Sub::Tat        => (TAT_ITEMS  as &[&str], self.sub_cursor),
            _               => (ITEMS      as &[&str], self.cursor),
        };

        let item_h = 38i32;
        let n_items  = items.len() as i32;
        let label_offset = if self.sub != Sub::None && self.sub != Sub::HowToPlay { 32 } else { 8 };
        let panel_h  = n_items * item_h + label_offset + 30;
        let panel_y  = 281i32;

        // Sub-menu label
        if self.sub != Sub::None && self.sub != Sub::HowToPlay {
            let label = match self.sub {
                Sub::SP                          => "SINGLEPLAYER",
                Sub::MP                          => "MULTIPLAYER",
                Sub::Live                        => "LIVE GAME",
                Sub::Tat                         => "TAKE A TURN",
                _                                => "",
            };
            let lw = str_width_scaled(label, 2);
            draw_str_shadow_scaled(buf, label, sw/2 - lw/2, panel_y + 8, Bgra::new(200, 200, 230), 2);
        }

        // Menu items overlaid directly on image — scroll window keeps cursor visible
        let scroll = self.scroll_offset;
        let visible: Vec<(usize, &&str)> = items.iter().enumerate()
            .skip(scroll).take(MAX_VISIBLE).collect();
        let start_y = panel_y + label_offset;
        // Scroll arrows
        if scroll > 0 {
            draw_str_scaled(buf, "^", sw/2 - 8, start_y - 16, Bgra::new(180, 180, 220), 1);
        }
        if scroll + MAX_VISIBLE < items.len() {
            let arrow_y = start_y + visible.len() as i32 * item_h;
            draw_str_scaled(buf, "v", sw/2 - 8, arrow_y, Bgra::new(180, 180, 220), 1);
        }
        for (slot, (i, &item)) in visible.iter().enumerate() {
            let iy = start_y + slot as i32 * item_h;
            let iw = str_width_scaled(item, 2);
            let selected = *i == cursor;
            if selected {
                crate::renderer::hud::draw_menu_selection(buf, sw/2 - 155, iy - 4, 310, 28);
            }
            let col = if selected { Bgra::new(255, 225, 55) } else { Bgra::new(0, 0, 0) };
            draw_str_shadow_scaled(buf, item, sw/2 - iw/2, iy, col, 2);
        }

        // Hint + version
        if self.sub != Sub::None && self.sub != Sub::HowToPlay {
            crate::renderer::hud::draw_button_hints(buf, &[("A", "SELECT"), ("B", "BACK")], 0);
        } else {
            crate::renderer::hud::draw_button_hints(buf, &[("A", "SELECT")], 0);
        }
        draw_str(buf, self.version, sw - str_width(self.version) - 6, sh - 18, Bgra::new(70, 70, 100));
    }

    fn draw_help(&self, buf: &mut WorldBuffer) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        let page = &HOW_TO_PAGES[self.help_page];
        let n_pages = HOW_TO_PAGES.len();

        // Background
        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(6, 8, 20));

        // Header bar
        buf.fill_rect(0, 0, SCREEN_W, 36, Bgra::new(18, 22, 50));
        buf.fill_rect(0, 36, SCREEN_W, 1, Bgra::new(60, 60, 120));

        // Page title
        let tw = str_width_scaled(page.title, 2);
        draw_str_scaled(buf, page.title, sw/2 - tw/2, 9, Bgra::new(255, 220, 50), 2);

        // Page indicator dots
        let dot_total = n_pages as i32 * 10;
        let dot_start = sw/2 - dot_total/2;
        for i in 0..n_pages {
            let dx = dot_start + i as i32 * 10 + 4;
            let col = if i == self.help_page { Bgra::new(255,200,50) } else { Bgra::new(60,60,100) };
            buf.fill_rect(dx, 28, 6, 6, col);
        }

        // Body text
        let line_h = 27i32;
        let body_top = 46i32;
        let text_col    = Bgra::new(210, 210, 230);
        let heading_col = Bgra::new(140, 200, 255);
        let dim_col     = Bgra::new(130, 130, 160);

        for (i, &line) in page.lines.iter().enumerate() {
            let ly = body_top + i as i32 * line_h;
            if ly + line_h > sh - 28 { break; }
            if line.is_empty() { continue; }
            // Lines with leading spaces are sub-items (dimmer)
            // Lines with no leading space that are short and ALL-CAPS or contain no space = heading
            let (col, x_off) = if line.starts_with("  ") {
                (dim_col, 12i32)
            } else if !line.contains(' ') || (line.len() < 20 && line.chars().all(|c| c.is_uppercase() || c == ' ')) {
                (heading_col, 0i32)
            } else {
                (text_col, 0i32)
            };
            let lx = 20 + x_off;
            draw_str(buf, line.trim_start(), lx, ly, col);
        }

        // Footer
        buf.fill_rect(0, sh - 26, SCREEN_W, 1, Bgra::new(40, 40, 80));
        let nav = "< >  PREV/NEXT PAGE";
        let back = "B = BACK";
        draw_str(buf, nav,  20,              sh - 18, Bgra::new(70, 70, 110));
        draw_str(buf, back, sw - str_width(back) - 20, sh - 18, Bgra::new(70, 70, 110));

        // Page number
        let pn = &format!("{}/{}", self.help_page + 1, n_pages);
        draw_str(buf, pn, sw/2 - str_width(pn)/2, sh - 18, Bgra::new(100, 100, 140));
    }
}
