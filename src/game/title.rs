use crate::input::{InputState, Button};
use crate::renderer::{WorldBuffer, Bgra};
use crate::renderer::font::{draw_str, draw_str_scaled, draw_str_shadow_scaled, str_width, str_width_scaled};
use crate::world::{SCREEN_W, SCREEN_H};

/// What the player picked on the title screen. Never serialized — purely a
/// local navigation result, so plain enum variants (no repr concerns).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TitleChoice {
    Quit,
    Live,        // casual live
    TakeATurn,
    Multi,
    Hotseat,
    VsCpu,
    Sp,
    MyTeam,
    LiveRanked,
    Test,        // all-weapons infinite-ammo hotseat
    Leaderboards,
    Missions,
    Settings,
}

const ITEMS:      &[&str] = &["SINGLEPLAYER", "MULTIPLAYER", "MY TEAMS", "SETTINGS", "HOW TO PLAY", "QUIT"];
const SP_ITEMS:   &[&str] = &["VS CPU", "HOTSEAT", "TEST"];
const SP_INFO:    &[&str] = &[
    "Fight a computer opponent on a random map.",
    "Two players share this device and take turns.",
    "Every weapon with infinite ammo. Practise anything.",
];
const MP_INFO:    &[&str] = &[
    "Real-time match against another player. Casual or ranked.",
    "Async games: play your turn whenever you like. Up to 15 at once, 14 days per turn.",
    "Daily and weekly challenges. Claim scrap for the ones you finish.",
    "Top players by wins and kills. L1/R1 switches casual and ranked.",
];
const LIVE_INFO:  &[&str] = &[
    "Real-time match. No ELO at stake.",
    "Real-time match against a player near your ELO. Wins and losses change your rank.",
];
const MP_ITEMS:   &[&str] = &["LIVE MATCH", "TAKE A TURN", "MISSIONS", "LEADERBOARDS"];
const LIVE_ITEMS: &[&str] = &["CASUAL", "RANKED"];
/// MP_ITEMS index of LIVE MATCH — cursor returns here when backing out of it.
const MP_LIVE_IDX: usize = 0;

// Max items visible in the panel before scrolling kicks in.
const MAX_VISIBLE: usize = 4;

/// Single source for controls reference text — shown both in the title's
/// HOW TO PLAY pager and in SETTINGS → CONTROLS.
pub const PAGE_CONTROLS: HelpPage = HelpPage {
    title: "CONTROLS",
    lines: &[
        "D-PAD LEFT/RIGHT   Move",
        "D-PAD UP/DOWN      Aim angle",
        "HOLD A + RELEASE   Charge and fire",
        "B                  Jump forward",
        "Y                  Backflip",
        "SELECT             Weapon menu",
        "START              Pause / menu",
        "",
        "R1 + D-PAD         Pan camera (snaps back)",
        "L1 + D-PAD         Pan camera (stays put)",
        "",
        "WEAPON MENU",
        "  D-PAD  Browse    A  Confirm    B  Cancel",
        "  L1 / R1  Adjust grenade fuse",
    ],
};

/// Per-weapon controls — the one copy, shared by HOW TO PLAY and
/// SETTINGS → CONTROLS. Keep ≤15 lines (HOW TO PLAY pager height).
pub const PAGE_WEAPON_CONTROLS: HelpPage = HelpPage {
    title: "WEAPON CONTROLS",
    lines: &[
        "GRAPPLE HOOK",
        "  A              Fire / Release / Re-rope",
        "  UP / DOWN      Shorten / Lengthen rope",
        "  LEFT / RIGHT   Swing while attached",
        "",
        "REVOLVER   A fires once per shot (6 per turn)",
        "SHOTGUN    A fires one of 2 shots per turn",
        "GRENADE / CLUMP BOMB   L1 / R1  fuse 1-5s",
        "TNT / MINE / BAT / JUMPBOT   A to place/swing",
        "",
        "PLASMA TORCH   Hold A to burn, UP/DOWN aims",
        "JACKHAMMER     A starts drilling down, A stops",
        "AIR STRIKE     UP/DOWN moves cursor, A strikes",
        "HAND OF JERRY  LEFT/RIGHT aims, A drops",
    ],
};

/// Pages shown by SETTINGS → CONTROLS.
pub const CONTROLS_PAGES: &[HelpPage] = &[PAGE_CONTROLS, PAGE_WEAPON_CONTROLS];

const HOW_TO_PAGES: &[HelpPage] = &[
    PAGE_CONTROLS,
    PAGE_WEAPON_CONTROLS,
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
            "CRATES",
            "  Walk over to collect. Health = +25 HP.",
            "  Weapon crates give a random weapon.",
            "  Teal crates = SCRAP (multiplayer only).",
            "",
            "Reduce all enemies to 0 HP to win!",
        ],
    },
    HelpPage {
        title: "WEAPONS",
        lines: &[
            "Infinite-ammo loadout weapons:",
            "",
            "BAZOOKA  Wind-affected rocket. 50 dmg,",
            "         blast 48px. Overcharge for speed.",
            "PISTOL   Hitscan 5-shot burst, 5 dmg/bullet.",
            "MAC-10   Hitscan 20-shot burst, 2 dmg/bullet.",
            "GRENADE  Bounces off terrain, no wind. 50 dmg,",
            "         blast 50px. Fuse 1-5s (L1/R1).",
            "SHOTGUN  Single precise hitscan shot, up to",
            "         25 dmg — grazes do less. Range 220px.",
            "         Recoil moves the shooter. 2 per turn.",
        ],
    },
    HelpPage {
        title: "MORE WEAPONS",
        lines: &[
            "Limited-use loadout weapons:",
            "",
            "MOLOTOV     Wide pool of long-burning fire. x2",
            "TNT         Placed, 4-5s fuse. 75 dmg. x1",
            "LANDMINE    Arms in 3s, proximity. 50 dmg. x2",
            "CLUMP BOMB  Splits into 5 bomblets. x2",
            "BASEBALL BAT  26 dmg + spin knockback. x1",
            "HOMING MISSILE  Locks on after ~1s of",
            "            flight. 50 dmg. x1",
            "GRAPPLE HOOK  Free movement, doesn't end",
            "            your turn. x5",
            "PLASMA TORCH  Hold A to tunnel terrain. x3",
            "JACKHAMMER  Drills straight down for ~6s,",
            "            hurting anyone in the shaft. x3",
        ],
    },
    HelpPage {
        title: "MORE WEAPONS 2",
        lines: &[
            "JUMPBOT  Placed like TNT; hops off on its",
            "         own, vaulting obstacles, and blows",
            "         up big. 75 dmg. x1",
            "",
            "Crate-only weapons:",
            "",
            "REVOLVER   6 hitscan shots per turn, 15 dmg",
            "           + knockback each. Range 800px.",
            "BANANA BOMB  Bounces, then bursts into 4",
            "           bouncing pieces. 75 dmg each.",
            "SACRED ORDNANCE  Rolls to a stop, then",
            "           detonates huge. 100 dmg.",
            "GARCIA     Aim a cursor; he drops from the",
            "           sky, bounces to a stop and booms.",
        ],
    },
    HelpPage {
        title: "SPECIAL WEAPONS",
        lines: &[
            "More crate-only weapons:",
            "",
            "BLASTHIVE  Bursts into 6 homing bees,",
            "           5 dmg per sting (30 if all hit).",
            "BLACK HOLE  Pulls soldiers, barrels and",
            "           projectiles within 108px, then",
            "           collapses after 5s. 35 dmg.",
            "AIR STRIKE  Unlocks after 7 turn cycles.",
            "           UP/DOWN cursor, A to call.",
            "           5 bombs, 50 dmg each.",
            "HAND OF JERRY  Ultra-rare (~3% drop).",
            "           LEFT/RIGHT aim, A to drop.",
            "           Smashes and bounces to water,",
            "           45 dmg per bounce.",
        ],
    },
    HelpPage {
        title: "GAME MODES",
        lines: &[
            "VS CPU   Fight an AI opponent solo.",
            "HOTSEAT  Two players share one device.",
            "TEST     All weapons, infinite ammo.",
            "",
            "LIVE MATCH (casual or ranked)",
            "  Real-time multiplayer. Ranked uses ELO.",
            "",
            "TAKE A TURN (casual or ranked)",
            "  Async — play at your pace.",
            "  Matchmake or join by code (casual).",
            "  14-day turn timer, up to 15 games.",
            "",
            "Your stats are in MY TEAMS -> PROFILE.",
        ],
    },
    HelpPage {
        title: "COSMETICS",
        lines: &[
            "Dress up your roster: MY TEAMS -> EQUIP.",
            "",
            "  HAT      Headwear above the soldier.",
            "  UNIFORM  Body and arm colours.",
            "  BOOTS    Leg colours.",
            "  GUN      Cosmetic weapon shape.",
            "",
            "Cosmetics lock at match start — EQUIP",
            "changes apply to new matches only.",
            "",
            "EARN SCRAP  Win +75, lose +25, scrap",
            "  crates +5-30, daily login +25, streak",
            "  +150, missions ~25-320 each.",
            "  Spend it in MY TEAMS -> STORE.",
        ],
    },
    HelpPage {
        title: "MISSIONS",
        lines: &[
            "MULTIPLAYER -> MISSIONS shows your active",
            "challenges. Press A on a completed one",
            "to claim its scrap.",
            "",
            "DAILY   3 from a pool, reset daily.",
            "  Around 25-90 scrap each.",
            "WEEKLY  3 bigger targets, reset weekly.",
            "  Around 100-320 scrap each.",
            "",
            "Progress counts across TAT and LIVE.",
            "",
            "LOGIN BONUS  +25 scrap once per day;",
            "  a 7-day streak pays +150 on top.",
        ],
    },
    HelpPage {
        title: "RANKS & ELO",
        lines: &[
            "Every account starts at 1000 ELO.",
            "Only RANKED matches change it.",
            "",
            "  Recruit      under 800",
            "  Private      800  - 999",
            "  Corporal     1000 - 1199  (start)",
            "  Sergeant     1200 - 1399",
            "  Lieutenant   1400 - 1599",
            "  Captain      1600 - 1799",
            "  Major        1800 - 1999",
            "  Commander    2000+",
            "",
            "Beating higher-ranked players earns more.",
        ],
    },
    HelpPage {
        title: "TIPS",
        lines: &[
            "Wind changes every turn — only the",
            "BAZOOKA is affected by it.",
            "",
            "High ground limits enemy aim angles.",
            "Blast the floor out from under enemies.",
            "",
            "SHOTGUN recoil also moves YOU —",
            "use it to reposition.",
            "",
            "GRAPPLE: release at the bottom of the",
            "swing for maximum distance.",
            "",
            "HOMING MISSILE: fire early — it needs",
            "~1s of flight before it locks on.",
        ],
    },
    HelpPage {
        title: "MORE TIPS",
        lines: &[
            "BLASTHIVE: throw at their feet —",
            "the bees swarm upward.",
            "",
            "BAT: face the target before swinging.",
            "",
            "PLASMA TORCH: tunnel under enemies or",
            "burrow to escape. Release A to stop.",
            "",
            "JACKHAMMER: drill a pit under yourself",
            "to hide from direct fire — or drill",
            "down onto an enemy in a shaft below.",
            "",
            "HAND OF JERRY: aim over a cluster —",
            "each bounce does 45 dmg down to water.",
        ],
    },
];

fn scroll_for(cursor: usize, _n: usize) -> usize {
    crate::renderer::hud::list_scroll_for(cursor, MAX_VISIBLE)
}

pub struct HelpPage {
    pub title: &'static str,
    pub lines: &'static [&'static str],
}

#[derive(PartialEq)]
enum Sub { None, SP, MP, Live, HowToPlay }

pub struct TitleScreen {
    cursor:        usize,
    sub_cursor:    usize,
    scroll_offset: usize,
    sub:           Sub,
    help_page:     usize,
    tick:          u32,
    version:       &'static str,
    update_available: bool,
    /// Header text for submenus: logged-in username, or NOT LOGGED IN.
    who:           String,
    /// Cached scrap balance (None = no cached profile).
    scrap:         Option<u32>,
}

impl TitleScreen {
    pub fn new(version: &'static str) -> Self {
        let who = crate::game::account::load_saved_creds()
            .map(|(u, _)| u.to_uppercase())
            .filter(|u| !u.is_empty())
            .unwrap_or_else(|| "NOT LOGGED IN".to_string());
        let scrap = crate::game::account::load_cached_profile().map(|p| p.0);
        Self { cursor: 0, sub_cursor: 0, scroll_offset: 0, sub: Sub::None, help_page: 0, tick: 0, version, update_available: false, who, scrap }
    }

    /// Re-read login + cached scrap (after MY TEAMS, where you log in/out).
    pub fn refresh_account(&mut self) {
        let fresh = Self::new(self.version);
        self.who = fresh.who;
        self.scrap = fresh.scrap;
    }

    pub fn set_update_available(&mut self, v: bool) {
        self.update_available = v;
    }

    /// Return to the MULTIPLAYER submenu, keeping the cursor where it was
    /// (e.g. on MISSIONS after closing the missions screen).
    pub fn continue_to_submenu(&mut self) {
        if self.sub != Sub::MP { self.back_to_mp(0); }
    }

    fn back_to_mp(&mut self, cursor: usize) {
        self.sub = Sub::MP;
        self.sub_cursor = cursor;
        self.scroll_offset = scroll_for(cursor, MP_ITEMS.len());
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
                        1 => return Some(TitleChoice::TakeATurn),
                        2 => return Some(TitleChoice::Missions),
                        3 => return Some(TitleChoice::Leaderboards),
                        _ => { self.sub = Sub::None; self.scroll_offset = 0; }
                    }
                }
            }
            Sub::Live => {
                let n = LIVE_ITEMS.len();
                if input.just_pressed(Button::Up)   { self.nav_up(n); }
                if input.just_pressed(Button::Down) { self.nav_down(n); }
                if input.just_pressed(Button::B)    { self.back_to_mp(MP_LIVE_IDX); }
                if input.just_pressed(Button::A) || input.just_pressed(Button::Start) {
                    match self.sub_cursor {
                        0 => return Some(TitleChoice::Live),
                        _ => return Some(TitleChoice::LiveRanked),
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
                        0 => TitleChoice::VsCpu,
                        1 => TitleChoice::Hotseat,
                        2 => TitleChoice::Test,
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
                        0 => return Some(TitleChoice::Sp),
                        1 => return Some(TitleChoice::Multi),
                        2 => return Some(TitleChoice::MyTeam),
                        3 => return Some(TitleChoice::Settings),
                        4 => { self.sub = Sub::HowToPlay; self.help_page = 0; return None; }
                        _ => return Some(TitleChoice::Quit),
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

        // Submenus: full-screen side layout (list left, info card right).
        let sub_menu: Option<(&str, &[&str], &[&str])> = match self.sub {
            Sub::SP   => Some(("SINGLEPLAYER", SP_ITEMS, SP_INFO)),
            Sub::MP   => Some(("MULTIPLAYER", MP_ITEMS, MP_INFO)),
            Sub::Live => Some(("LIVE MATCH", LIVE_ITEMS, LIVE_INFO)),
            _ => None,
        };
        if let Some((crumb, items, info)) = sub_menu {
            use crate::renderer::hud::{draw_side_menu, SideItem, SideCard, SIDE_SCRAP};
            let rows: Vec<SideItem> = items.iter()
                .map(|&label| SideItem { label, badge: "", danger: false }).collect();
            let c = self.sub_cursor.min(items.len() - 1);
            let mut stats = Vec::new();
            if self.sub == Sub::MP && c == 2 {
                if let Some(sc) = self.scrap { stats.push(("SCRAP".to_string(), sc.to_string(), SIDE_SCRAP)); }
            }
            let card = SideCard { title: items[c], body: info[c], stats: &stats };
            draw_side_menu(buf, crumb, &self.who, &rows, c, &card, &[("A", "SELECT"), ("B", "BACK")]);
            return;
        }

        // Title background image
        draw_title_bg(buf, 0);

        // Main title menu (unchanged layout)
        let list: Vec<(&str, bool)> = ITEMS.iter().map(|&s| (s, false)).collect();
        crate::renderer::hud::draw_list_panel(
            buf, None, &list, self.cursor, self.scroll_offset,
            &crate::renderer::hud::ListStyle::default());

        // Hint + version
        crate::renderer::hud::draw_button_hints(buf, &[("A", "SELECT")], 0, 0);
        draw_str(buf, self.version, sw - str_width(self.version) - 6, sh - 18, Bgra::new(70, 70, 100));
        if self.update_available {
            // Blinking banner above the version string so an available update is
            // always visible from the title screen (selecting any MP mode offers it).
            let t = "UPDATE AVAILABLE";
            let col = if (self.tick / 20) % 2 == 0 { Bgra::new(255, 210, 50) } else { Bgra::new(200, 150, 30) };
            draw_str(buf, t, sw - str_width(t) - 6, sh - 32, col);
        }
    }

    fn draw_help(&self, buf: &mut WorldBuffer) {
        let sw = SCREEN_W as i32;
        let sh = SCREEN_H as i32;
        let page = &HOW_TO_PAGES[self.help_page];
        let n_pages = HOW_TO_PAGES.len();

        // Background
        buf.fill_rect(0, 0, SCREEN_W, SCREEN_H, Bgra::new(6, 8, 20));

        // Header bar + page title
        crate::renderer::hud::draw_screen_header(buf, page.title);

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
