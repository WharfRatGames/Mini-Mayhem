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
    /// Client's authoritative selected-weapon KIND (net id). The server selects by
    /// kind, not index, so a pruned/diverged loadout can't make the wrong weapon fire.
    pub selected_weapon_kind: u8,
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
    pub barrels:            Vec<NetBarrel>,
    pub black_holes:        Vec<NetBlackHole>,
    pub fire_patches:       Vec<NetFirePatch>,
    pub rope:               Option<NetRope>,
    pub opp_team_name:      String,
    pub garcia:             Option<NetGarcia>,
    /// Active plasma-torch direction: 0=none, 1=UpForward, 2=Forward, 3=DownForward.
    /// Lets the live client draw the torch flame at the tip and suppress the
    /// per-tick crater-derived explosion flashes the torch's carving would spawn.
    pub torch_dir:          u8,
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
pub struct NetGarcia {
    pub cursor_x:    f32,
    pub render_x:    f32,
    pub blink_timer: u32,
    pub falling:     bool,
    pub fall_y:      f32,
    pub vel_y:       f32,
    pub bounce_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetSoldier {
    pub team:            usize,
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
    pub your_team: usize,
    pub seed:      u64,
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let payload = bincode::serialize(msg).unwrap();
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&payload);
    buf
}

pub fn decode_len(header: &[u8; 4]) -> usize {
    u32::from_le_bytes(*header) as usize
}
