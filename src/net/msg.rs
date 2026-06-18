//! Network messages between client and server.
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMsg {
    pub tick:      u32,
    pub held:      Vec<NetButton>,
    pub pressed:   Vec<NetButton>,
    pub released:  Vec<NetButton>,
    /// Client's authoritative aim angle — server applies directly to avoid drift.
    pub aim_angle: f32,
    /// Rendered barrel-tip position from the skeleton renderer. Server uses this
    /// as the hitscan ray origin for Revolver/Shotgun so all modes fire identically.
    /// (0.0, 0.0) means "not available this tick" — server falls back to approximation.
    pub muzzle_x: f32,
    pub muzzle_y: f32,
    /// Client's authoritative selected-weapon KIND (net id). The server selects by
    /// kind, not index, so a pruned/diverged loadout can't make the wrong weapon fire.
    pub selected_weapon_kind: u8,
    /// Set to true on the final InputMsg before a voluntary quit (forfeit).
    /// Server immediately awards the win to the remaining player and skips the
    /// reconnect window — this is not a disconnection, it is an explicit forfeit.
    #[serde(default)]
    pub quit: bool,
    /// Per-soldier cosmetics and names sent by the client so the server can
    /// broadcast them to the opponent. Applied whenever received (idempotent).
    pub hat_ids:           [u8; 4],
    pub uniform_color_ids: [u8; 4],
    pub boot_color_ids:    [u8; 4],
    pub gun_style_ids:     [u8; 4],
    pub worm_names:        [String; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetButton {
    Up, Down, Left, Right,
    A, B, X, Y,
    L1, R1, L2, R2,
    Start, Select,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetCrate {
    pub x:      f32,
    pub y:      f32,
    pub landed: bool,
    /// 0=Health, 1=Weapon, 2=Scrap — so the live client renders the right
    /// crate colour/symbol (server is authoritative on the actual contents).
    pub kind_u8: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetMine {
    pub x:             f32,
    pub y:             f32,
    /// 0=Arming, 1=Armed, 2=Triggered
    pub state_u8:      u8,
    pub arm_ticks:     u32,
    pub trigger_ticks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetCrater {
    pub cx:     f32,
    pub cy:     f32,
    pub radius: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetMessage {
    pub text: String,
    /// -1 = neutral (None), else team slot index.
    pub team: i8,
    pub ticks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetGrave {
    pub x:            f32,
    pub y:            f32,
    pub team:         usize,
    pub headstone_id: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetBloodSplat {
    pub x:     f32,
    pub y:     f32,
    pub ticks: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateMsg {
    pub tick:           u32,
    pub soldiers:       Vec<NetSoldier>,
    pub projectiles:    Vec<NetProjectile>,
    pub wind:           f32,
    pub turn_team:      usize,
    pub active_soldier: usize,
    pub turn_secs:      u32,
    pub phase:          NetPhase,
    pub aim_angle:          f32,
    pub aim_power:          f32,
    pub result:             NetResult,
    pub craters:            Vec<NetCrater>,
    /// Event messages (crate pickups, etc.) for the live client to show.
    pub messages:           Vec<NetMessage>,
    /// Headstones — server-authoritative so live clients show them without
    /// re-deriving deaths locally (which would double the death SFX).
    pub graves:             Vec<NetGrave>,
    /// Blood splatter decals (server-authoritative; decayed server-side).
    pub blood_splats:       Vec<NetBloodSplat>,
    /// Weapon menu state — so client can render the overlay during live play.
    pub weapon_menu_open:   bool,
    pub weapon_menu_cursor: usize,
    pub aim_fuse_ticks:     u32,
    pub crates:             Vec<NetCrate>,
    pub mines:              Vec<NetMine>,
    /// Sound events the server emitted this tick (`Sfx as u8`), so the live
    /// client — which runs no simulation — plays the same SFX as every other mode.
    pub sounds:             Vec<u8>,
    /// Cosmetic FX-spawn events the server emitted this tick, so the live client
    /// (which runs no sim) spawns the same particle bursts as every other mode.
    pub fx_events:          Vec<crate::renderer::fx::FxEvent>,
    pub barrels:            Vec<NetBarrel>,
    pub black_holes:        Vec<NetBlackHole>,
    pub fire_patches:       Vec<NetFirePatch>,
    pub rope:               Option<NetRope>,
    /// Display name per compact team index.
    pub team_names:         Vec<String>,
    /// Colour identity (0-3) per compact team index.
    pub team_colors:        Vec<u8>,
    pub garcia:             Option<NetGarcia>,
    pub airstrike:          Option<NetAirstrike>,
    /// Active plasma-torch direction: 0=none, 1=UpForward, 2=Forward, 3=DownForward.
    /// Lets the live client draw the torch flame at the tip and suppress the
    /// per-tick crater-derived explosion flashes the torch's carving would spawn.
    pub torch_dir:          u8,
    /// Remaining fuel ticks for the plasma torch (0 when inactive).
    #[serde(default)]
    pub torch_fuel:         u32,
    /// Some(seconds_remaining) while the match is paused waiting for the
    /// opponent to reconnect; None during normal play.
    pub paused_opponent:    Option<u32>,
    /// True on the final StateMsg if the pause window expired without the
    /// opponent reconnecting — client shows "opponent left" instead of a
    /// normal result screen.
    pub opponent_abandoned: bool,
    /// Weapon inventory per team: [(kind_u8, ammo_or_0xFFFF_for_infinite)].
    /// Lets the live client show accurate ammo counts and weapon list.
    pub team_weapons:       Vec<NetTeamWeapons>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetTeamWeapons {
    pub selected: usize,
    /// (kind_u8, ammo): ammo=0xFFFF means infinite.
    pub weapons:  Vec<(u8, u32)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetBarrel {
    pub x:  f32,
    pub y:  f32,
    pub hp: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetBlackHole {
    pub x:          f32,
    pub y:          f32,
    pub ticks_left: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetFirePatch {
    pub x:          f32,
    pub y:          f32,
    pub lifetime:   u32,
    pub landed:     bool,
    pub vel_x:      f32,
    pub vel_y:      f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetRope {
    pub anchor_x:  f32,
    pub anchor_y:  f32,
    pub hook_x:    f32,
    pub hook_y:    f32,
    pub flying:    bool,
    pub length:    f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetAirstrike {
    pub cursor_x:        f32,
    pub render_x:        f32,
    pub cursor_y:        f32,
    pub render_y:        f32,
    pub blink_timer:     u32,
    pub active:          bool,
    pub plane_x:         f32,
    pub plane_vx:        f32,
    pub bombs_dropped:   u32,
    pub direction_right: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetGarcia {
    pub cursor_x:    f32,
    pub render_x:    f32,
    pub cursor_y:    f32,
    pub render_y:    f32,
    pub blink_timer: u32,
    pub falling:     bool,
    pub fall_y:      f32,
    pub vel_y:       f32,
    pub bounce_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetSoldier {
    /// Compact team index (0..team_count) — used to look up the local team.
    pub team:            usize,
    /// Colour identity 0-3 (Red/Blue/Green/Yellow) for rendering.
    pub color_id:        u8,
    pub index:           usize,
    pub x:               f32,
    pub y:               f32,
    pub hp:              u8,
    pub facing:          i8,
    pub dead:            bool,
    pub has_fired:       bool,
    pub selected_weapon: usize,
    pub airborne:        bool,
    pub spinning:        bool,
    /// Airborne velocity — lets the live client tilt the soldier's torso lean
    /// during knockback flight, matching local modes.
    pub vel_x:           f32,
    pub vel_y:           f32,
    pub airtime:         u32,
    pub walk_ticks:      u32,
    pub walking:         bool,
    pub hat_id:           u8,
    pub uniform_color_id: u8,
    pub boot_color_id:    u8,
    pub gun_style_id:     u8,
    pub name:             String,
    /// 0=Generic,1=Explosion,2=Fall,3=Water — lets the live client pick the
    /// right death-message flavour pool (it generates the text locally).
    pub death_cause_u8:   u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetProjectile {
    pub x:           f32,
    pub y:           f32,
    pub vel_x:       f32,
    pub vel_y:       f32,
    pub kind_u8:     u8,   // WeaponKind index
    pub fuse_ticks:  u32,  // 0 = no fuse / expired
    pub is_fragment: bool, // true for BananaBomb sub-munitions
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum NetResult {
    Ongoing,
    Winner(usize),
    Draw,
}


#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum NetPhase {
    Acting,
    Watching,
    Retreating,
    Ending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WelcomeMsg {
    /// This client's compact team index (0..team_count).
    pub your_team:  usize,
    pub seed:       u64,
    /// Number of teams in this match (2-4).
    pub team_count: usize,
    /// This client's chosen colour identity (0-3).
    pub your_color: u8,
    /// Per-player reconnect token for casual matches (empty = non-reconnectable).
    #[serde(default)]
    pub reconnect_token: String,
}

// ── Casual lobby protocol ───────────────────────────────────────────────────
// Exchanged only during the pre-match casual lobby phase (before the normal
// StateMsg/InputMsg gameplay stream begins). Ranked play does not use these.

/// Roster info a player announces when joining the lobby.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyJoin {
    pub name:              String,
    pub username:          String,
    pub avatar_id:         u8,
    pub headstone_id:      u8,
    pub worm_names:        [String; 4],
    pub hat_ids:           [u8; 4],
    pub uniform_color_ids: [u8; 4],
    pub boot_color_ids:    [u8; 4],
    pub gun_style_ids:     [u8; 4],
}

/// One player's lobby state, broadcast to everyone in the lobby.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LobbyPlayer {
    pub name:      String,
    pub username:  String,
    pub avatar_id: u8,
    /// None until the player picks a colour.
    pub color_id:  Option<u8>,
    pub ready:     bool,
}

/// Client → server lobby messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LobbyClientMsg {
    Join(LobbyJoin),
    PickColor { color_id: u8 },
    SetReady  { ready: bool },
    Leave,
}

/// Server → client lobby messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LobbyServerMsg {
    /// Full lobby roster plus this client's own slot in the list.
    State { players: Vec<LobbyPlayer>, your_index: usize },
    /// Match is starting — carries the usual welcome payload.
    Start(WelcomeMsg),
}

pub fn decode_len(header: &[u8; 4]) -> usize {
    u32::from_le_bytes(*header) as usize
}
