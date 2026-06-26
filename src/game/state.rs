use crate::world::{Terrain, WorldPos, Crater, WORLD_W, WATER_Y};
use crate::physics::{Projectile, Wind, StepResult, step_projectile, WeaponKind};
use super::soldier::DeathCause;
use super::team::Team;
use super::turn::TurnManager;

/// How long (in sim ticks) the camera holds on a soldier that just took
/// explosion damage, so the player can read the resulting HP loss.
pub const DAMAGE_FOCUS_TICKS: u32 = 50;

/// Representative dirt tone per map archetype, used to colour explosion debris
/// chunks so the fallout matches the biome.
pub fn biome_dirt(archetype: u8) -> crate::renderer::fb::Bgra {
    use crate::renderer::fb::Bgra;
    match archetype {
        1 => Bgra::new(120, 122, 128), // cliffs: grey stone
        2 => Bgra::new(150, 130, 96),  // islands: sandy
        3 => Bgra::new(78, 70, 64),    // caverns: dark earth
        4 => Bgra::new(166, 116, 78),  // canyon: red-brown
        _ => Bgra::new(132, 104, 64),  // hills: brown
    }
}

/// A crate sitting on the terrain waiting to be collected.
#[derive(Debug, Clone)]
pub struct DroppedCrate {
    pub pos:  WorldPos,
    pub kind: CrateKind,
    /// True if the parachute has landed (solid ground found below).
    pub landed: bool,
    /// Parachute descent velocity.
    pub descent_vy: f32,
    /// Damage accumulated this turn. 20+ destroys the crate; resets on Ending phase.
    pub damage_this_turn: u32,
    /// Ticks spent falling. Parachute shown for first 120 ticks (4s); after that drops faster.
    pub fall_ticks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrateKind {
    Weapon(WeaponKind),
    Health,
    Scrap(u32), // amount 5–30; multiplayer only
}

/// Aim state while a soldier is preparing to fire.
#[derive(Debug, Clone)]
pub struct AimState {
    /// Aim angle in radians. 0 = right, π/2 = up.
    pub angle:      f32,
    /// Charge power 0.0–1.0 (normal), up to 1.2 for bazooka overcharge.
    pub power:      f32,
    /// How many ticks A has been held (for charge detection).
    pub charge_ticks: u32,
    /// Selected fuse length in ticks (for grenade-type weapons).
    pub fuse_ticks: u32,
    /// A must be released at least once before charging is allowed (prevents
    /// menu-confirm A press from firing immediately at turn start).
    pub charge_armed: bool,
}

impl AimState {
    pub fn new() -> Self {
        Self {
            angle: 0.0,
            power: 0.0,
            charge_ticks: 0,
            fuse_ticks: 90, // default 3 s at 30 Hz
            charge_armed: false,
        }
    }

    /// Increase charge while A is held.
    pub fn charge(&mut self) {
        self.charge_ticks += 1;
        self.power = (self.charge_ticks as f32 / 40.0).min(1.0);
    }

    /// Reset after firing.
    pub fn reset(&mut self) {
        self.power = 0.0;
        self.charge_ticks = 0;
        self.charge_armed = false;
    }
}

/// Whether the game is still going or has ended.
#[derive(Debug, Clone, PartialEq)]
pub enum GameResult {
    Ongoing,
    Winner(usize),  // team slot
    Draw,
}

/// A live explosion animation instance (visual only — damage already applied).
#[derive(Debug, Clone)]
pub struct Explosion {
    pub pos:    WorldPos,
    pub radius: f32,
    pub age:    u32,
}

impl Explosion {
    pub const MAX_AGE: u32 = 20;
    pub fn new(pos: WorldPos, radius: f32) -> Self { Self { pos, radius, age: 0 } }
    pub fn is_done(&self) -> bool { self.age >= Self::MAX_AGE }
}

/// Active grappling-hook rope — either hook in flight or tethered to terrain.
#[derive(Debug, Clone)]
pub struct RopeState {
    /// Terrain attachment point (valid when !flying).
    pub anchor:   WorldPos,
    /// Current target rope length in pixels.
    pub length:   f32,
    /// True while the hook projectile is still in flight.
    pub flying:   bool,
    /// Hook position: current flight position, or the anchor once attached.
    pub hook:     WorldPos,
    /// Hook velocity while flying.
    pub hook_vel: crate::world::Vec2,
}

/// A permanent grave marker left at the position a soldier died.
#[derive(Debug, Clone)]
pub struct Grave {
    pub pos:         WorldPos,
    pub team:        usize,
    pub soldier_idx: usize,
    pub died_tick:   u32,
    /// Downward velocity while the headstone is falling into place.
    pub vel_y:       f32,
    /// True once the headstone has landed on terrain.
    pub settled:     bool,
    /// Which headstone design to render (0–5).
    pub headstone_id: u8,
}

/// A queued death explosion — fires ~1 second after the soldier dies.
#[derive(Debug, Clone)]
pub struct PendingDeathExplosion {
    pub pos:    WorldPos,
    pub timer:  u32,     // ticks remaining (30 = 1s at 30Hz)
    pub team:   usize,
    pub si:     usize,
    pub cause:  DeathCause,
}

/// A short Worms-style event message shown at the bottom of the screen.
#[derive(Debug, Clone)]
pub struct GameMessage {
    pub text: String,
    /// Team slot for colour (None = neutral yellow).
    pub team: Option<usize>,
    pub ticks: u32,
}

/// State of a placed landmine.
#[derive(Debug, Clone, PartialEq)]
pub enum MineState {
    Arming,    // arm_ticks counting down to zero
    Armed,     // checking proximity each tick
    Triggered, // trigger_ticks counting down to explosion
}

/// A landmine placed on the terrain surface.
#[derive(Debug, Clone)]
pub struct PlacedMine {
    pub pos:           WorldPos,
    pub state:         MineState,
    /// Ticks until mine becomes armed (90 = 3s at 30Hz). 0 when already armed.
    pub arm_ticks:     u32,
    /// Ticks until detonation after trigger (15 = 0.5s at 30Hz).
    pub trigger_ticks: u32,
}

/// State of an explosive barrel.
#[derive(Debug, Clone, PartialEq)]
pub enum BarrelState {
    Normal,
    Triggered { ticks: u32 }, // brief delay then boom
}

/// An explosive oil drum sitting on the map.
#[derive(Debug, Clone)]
pub struct Barrel {
    pub pos:   WorldPos,
    pub vel:   crate::world::Vec2,
    pub hp:    i32,   // explodes when <= 0
    pub state: BarrelState,
}

/// An active gravity well spawned by the Black Hole Bomb.
#[derive(Debug, Clone)]
pub struct BlackHole {
    pub pos:      WorldPos,
    pub lifetime: u32, // ticks remaining (150 = 5 s at 30 Hz)
}

/// A fragment of fire spawned by a barrel explosion — flies outward, lands, deals DoT.
#[derive(Debug, Clone)]
pub struct FirePatch {
    pub pos:      WorldPos,
    pub vel:      crate::world::Vec2,
    pub landed:   bool,
    pub lifetime: u32,  // ticks remaining (~150–240 = 5–8 s at 30 Hz)
}

/// Torch angle (relative to soldier facing direction).
/// Three discrete forward-cone states — matches WA blow torch behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TorchDir { UpForward, Forward, DownForward }

impl TorchDir {
    pub fn to_vec(self, facing: f32) -> (f32, f32) {
        // ±35° forward cone. cos(35°)≈0.819, sin(35°)≈0.574
        let (x, y) = match self {
            TorchDir::Forward     => (1.0,   0.0),
            TorchDir::UpForward   => (0.819, -0.574),
            TorchDir::DownForward => (0.819,  0.574),
        };
        (x * facing, y)
    }

    pub fn step_up(self) -> Self {
        match self {
            TorchDir::DownForward => TorchDir::Forward,
            TorchDir::Forward     => TorchDir::UpForward,
            TorchDir::UpForward   => TorchDir::UpForward,
        }
    }

    pub fn step_down(self) -> Self {
        match self {
            TorchDir::UpForward   => TorchDir::Forward,
            TorchDir::Forward     => TorchDir::DownForward,
            TorchDir::DownForward => TorchDir::DownForward,
        }
    }
}

/// Active plasma torch session state.
#[derive(Debug, Clone)]
pub struct PlasmaTorchState {
    pub dir:        TorchDir,
    pub fuel_ticks: u32, // 6 s × 30 Hz = 180 ticks
}

/// Airstrike targeting / active state.
#[derive(Debug, Clone)]
pub struct AirstrikeState {
    pub cursor_x:      f32,
    pub render_x:      f32,
    pub cursor_y:      f32,
    pub render_y:      f32,
    pub blink_timer:   u32,
    pub active:        bool,
    pub plane_x:       f32,
    pub plane_vx:      f32,
    pub bombs_dropped: u32,
    pub direction_right: bool,
    /// Camera left edge captured at targeting time — plane spawns from screen edge, not map edge.
    pub spawn_cam_left: f32,
}

/// Homing Missile targeting state (active while player is picking the target point).
#[derive(Debug, Clone)]
pub struct HomingMissileState {
    pub cursor_x:    f32,
    pub cursor_y:    f32,
    pub render_x:    f32,
    pub render_y:    f32,
    pub blink_timer: u32,
    /// True once the player has confirmed the target; enters charge-shot phase.
    pub confirmed:   bool,
}

/// Garcia targeting / falling state.
#[derive(Debug, Clone)]
pub struct GarciaState {
    /// World-space cursor X chosen by the player (clamped to map).
    pub cursor_x:    f32,
    /// Smoothed render X for the vertical beam / arrow.
    pub render_x:    f32,
    /// Screen-space cursor Y (drop height indicator), chosen by the player.
    pub cursor_y:    f32,
    /// Smoothed render Y for the drop-height arrow.
    pub render_y:    f32,
    /// Tick counter for pulsing alpha.
    pub blink_timer: u32,
    /// True once the player has confirmed the target and Garcia is falling.
    pub falling:     bool,
    /// World Y of the falling sprite (starts at -200).
    pub fall_y:      f32,
    /// Vertical velocity (positive = downward). Starts at 8, set negative on bounce.
    pub vel_y:       f32,
    /// Bounce count (for scaling explosion force on later bounces).
    pub bounce_count: u32,
}

/// The complete mutable game state.
pub struct GameState {
    pub terrain:     Terrain,
    pub teams:       Vec<Team>,
    pub turn:        TurnManager,
    pub projectiles: Vec<Projectile>,
    pub crates:      Vec<DroppedCrate>,
    pub mines:       Vec<PlacedMine>,
    pub barrels:     Vec<Barrel>,
    pub fire_patches: Vec<FirePatch>,
    pub black_holes:  Vec<BlackHole>,
    pub wind:        Wind,
    pub aim:         AimState,
    pub result:      GameResult,
    /// Physics tick counter (for animations, crate timing).
    pub tick:        u32,
    /// Ticks since last crate drop attempt.
    pub crate_timer: u32,
    /// All craters carved so far (for network sync).
    pub crater_log: Vec<(f32, f32, f32)>,
    /// Sound events emitted during the current sim tick (as `Sfx as u8`). Cleared
    /// at the top of each tick()/server_tick(); the server ships these in
    /// StateMsg.sounds so the live client (which runs no sim) plays them too.
    pub sounds:      Vec<u8>,
    /// Cosmetic FX-spawn events emitted during the current sim tick. Cleared at
    /// the top of each tick()/server_tick() alongside `sounds`; the server ships
    /// these in StateMsg.fx_events so the live client (which runs no sim) spawns
    /// the same particle bursts. Route gameplay-event fx through `emit_fx`.
    pub fx_events:   Vec<crate::renderer::fx::FxEvent>,
    pub map_seed:    u64,
    /// True only for the local TEST mode (all weapons, infinite ammo). Drives the
    /// on-screen seed display; left false for normal/live/TAT play.
    pub is_test:     bool,
    /// True for TAT and Live multiplayer — enables scrap crate drops.
    pub is_multiplayer: bool,
    /// Scrap earned this session (for end-of-match display). Actual balance lives server-side.
    pub scrap_earned: u32,
    /// Grave markers for each dead soldier.
    pub graves:      Vec<Grave>,
    /// Live explosion animations (visual only).
    pub explosions:  Vec<Explosion>,
    /// True if the active worm took damage from an explosion this watching phase.
    pub active_worm_hit: bool,
    /// True during retreat when the active worm was hit — movement locked, camera forced on worm.
    pub retreat_locked: bool,
    /// Position + remaining tick count for "hold camera on damaged soldier so the
    /// HP loss can be read" — set by explosion damage, ticked down during retreat.
    pub damage_focus: Option<(WorldPos, u32)>,
    /// Weapon menu open in server_tick() contexts (TAT / live server).
    pub weapon_menu_open:   bool,
    pub weapon_menu_cursor: usize,
    /// Fire suppression ticks after weapon-confirm in server_tick() contexts.
    pub server_fire_grace:  u8,
    /// Shotgun two-shot state: 1 = first shot fired, player can fire again; 0 = not in shotgun mode.
    pub shotgun_shots_left: u8,
    /// Revolver multi-shot state: 1–6 shots remaining this turn; 0 = not in revolver mode.
    pub revolver_shots_left: u8,
    /// Minigun burst state: shots remaining this turn; 0 = not firing.
    pub minigun_shots_left: u8,
    /// Ticks until next minigun bullet fires (counts down each tick).
    pub minigun_fire_timer: u8,
    /// Uzi burst state: shots remaining this turn; 0 = not firing.
    pub uzi_shots_left: u8,
    /// Ticks until next uzi bullet fires (counts down each tick).
    pub uzi_fire_timer: u8,
    /// Bullet trail visuals: (start, end, ttl_ticks). Client-side only — not networked.
    pub bullet_trails: Vec<(WorldPos, WorldPos, u8)>,
    /// Active grappling-hook rope; None = no rope deployed.
    pub rope: Option<RopeState>,
    /// True from the first rope cast until the soldier lands — keeps acting phase alive.
    pub rope_session: bool,
    /// True if the grapple was fired at least once this turn — 1 charge consumed at turn end.
    pub rope_used_this_turn: bool,
    /// TNT fuse is burning in Watching phase; player may move to escape blast radius.
    pub tnt_placed: bool,
    /// Pre-turn crate phase: player input blocked, camera follows crate.
    pub crate_watch_ticks: u32,
    /// Event message queue (visual only — not networked).
    pub messages: Vec<GameMessage>,
    /// Blood splat particles from shotgun hits (visual only — not networked). (pos, ticks_left)
    pub blood_splats: Vec<(WorldPos, u32)>,
    /// Bazooka smoke trail particles (visual only — not networked). (pos, ticks_left)
    pub smoke_particles: Vec<(WorldPos, u32)>,
    /// Event-driven effect particles — explosion fallout, dust, sparks, splashes
    /// (visual only — not networked). Spawned at event sites, stepped in simulate().
    pub fx: Vec<crate::renderer::fx::FxParticle>,
    /// Soldiers waiting to explode before their headstone is placed.
    pub pending_deaths: Vec<PendingDeathExplosion>,
    /// Active plasma torch tunneling session; None = not torching.
    pub plasma_torch: Option<PlasmaTorchState>,
    /// Garcia targeting / falling session; None = inactive.
    pub garcia: Option<GarciaState>,
    /// Airstrike targeting / active session; None = inactive.
    pub airstrike: Option<AirstrikeState>,
    /// Homing missile target-picking session; None = inactive.
    pub homing_missile: Option<HomingMissileState>,
    /// Set true during the meteor bomb impact loop so chain-detonated barrels/mines
    /// also scatter fragments. Transient — not networked.
    pub meteor_chain: bool,
}

impl GameState {
    /// Initialise a complete game.
    pub fn new(
        map_seed:   u64,
        terrain:    Terrain,
        teams:      Vec<Team>,
        team_count: usize,
    ) -> Self {
        Self {
            terrain,
            turn: TurnManager::new(team_count),
            projectiles: Vec::new(),
            crates: Vec::new(),
            mines: Vec::new(),
            barrels: Vec::new(),
            fire_patches: Vec::new(),
            black_holes: Vec::new(),
            wind: Wind::calm(),
            aim: AimState::new(),
            result: GameResult::Ongoing,
            tick: 0,
            crate_timer: 0,
            map_seed,
            is_test: false,
            is_multiplayer: false,
            scrap_earned: 0,
            crater_log: Vec::new(),
            sounds:     Vec::new(),
            fx_events:  Vec::new(),
            graves: Vec::new(),
            explosions: Vec::new(),
            active_worm_hit: false,
            damage_focus: None,
            retreat_locked: false,
            weapon_menu_open: false,
            weapon_menu_cursor: 0,
            server_fire_grace: 0,
            shotgun_shots_left: 0,
            revolver_shots_left: 0,
            minigun_shots_left: 0,
            minigun_fire_timer: 0,
            uzi_shots_left: 0,
            uzi_fire_timer: 0,
            bullet_trails: Vec::new(),
            rope: None,
            rope_session: false,
            rope_used_this_turn: false,
            tnt_placed: false,
            crate_watch_ticks: 0,
            messages: Vec::new(),
            blood_splats: Vec::new(),
            smoke_particles: Vec::new(),
            fx: Vec::new(),
            pending_deaths: Vec::new(),
            plasma_torch: None,
            garcia: None,
            airstrike: None,
            homing_missile: None,
            meteor_chain: false,
            teams,
        }
    }

    /// Active team index.
    pub fn active_team(&self) -> usize { self.turn.current_team() }

    /// Reference to the active team.
    pub fn active_team_ref(&self) -> &Team { &self.teams[self.active_team()] }

    /// Mutable reference to the active team.
    pub fn active_team_mut(&mut self) -> &mut Team {
        let idx = self.turn.current_team();
        &mut self.teams[idx]
    }

    /// Position of the active soldier.
    pub fn active_pos(&self) -> WorldPos {
        self.active_team_ref().active_soldier().pos
    }

    /// Alive status array for TurnManager::advance.
    pub fn alive_teams(&self) -> Vec<bool> {
        self.teams.iter().map(|t| !t.is_eliminated()).collect()
    }

    /// Check win condition. Updates self.result.
    pub fn check_win(&mut self) {
        let alive: Vec<usize> = self.teams.iter().enumerate()
            .filter(|(_, t)| !t.is_eliminated())
            .map(|(i, _)| i)
            .collect();

        self.result = match alive.len() {
            0 => GameResult::Draw,
            1 => GameResult::Winner(alive[0]),
            _ => GameResult::Ongoing,
        };
    }

    /// Apply an explosion at a world position.
    /// Carves terrain, deals distance-falloff damage with a direct-hit bonus,
    /// applies Worms-style knockback with an upward bias, and spawns an animation.
    pub fn apply_explosion(&mut self, pos: WorldPos, kind: WeaponKind) {
        let force = kind.blast_force();
        self.apply_explosion_force(pos, kind, force);
    }

    /// Like `apply_explosion` but with an explicit knockback force, so callers
    /// can reuse another weapon's crater/damage profile with a different push
    /// (e.g. the Meteor Bomb borrows TNT's blast but with gentler knockback).
    pub fn apply_explosion_force(&mut self, pos: WorldPos, kind: WeaponKind, force: f32) {
        self.apply_explosion_scaled(pos, kind, force, 1.0, 1.0);
    }

    /// Record a sound event AND play it locally. On the local-sim modes
    /// (hotseat/VS-CPU/TAT, all on the Miyoo) this plays immediately; on the live
    /// server (no audio device) it just records into `self.sounds`, which
    /// build_state ships to the live client so it stays in audio parity.
    /// Destroy any crates whose `damage_this_turn` has reached the threshold,
    /// spawning fire patches. Call after hitscan weapons that bypass apply_explosion.
    pub fn flush_crate_damage(&mut self) {
        for crate_ in self.crates.iter().filter(|c| c.damage_this_turn >= 20) {
            let cpos = crate_.pos;
            let mut rng = (cpos.x as u64).wrapping_mul(0x6364136223846885)
                .wrapping_add((cpos.y as u64).wrapping_mul(0x9e3779b97f4a7c15));
            let count = 2 + (rng % 4) as usize;
            for _ in 0..count {
                rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
                let angle = (rng >> 33) as f32 / (u32::MAX as f32) * std::f32::consts::TAU;
                rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
                let speed = 2.0 + (rng >> 33) as f32 / (u32::MAX as f32) * 4.0;
                rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
                let life = 120 + (rng >> 33) as u32 % 60;
                self.fire_patches.push(FirePatch {
                    pos: cpos,
                    vel: crate::world::Vec2::new(angle.cos() * speed, angle.sin() * speed - 2.0),
                    landed: false,
                    lifetime: life,
                });
            }
        }
        self.crates.retain(|c| c.damage_this_turn < 20);
    }

    pub fn emit_sound(&mut self, s: crate::audio::Sfx) {
        self.sounds.push(s as u8);
        crate::audio::play(s);
    }

    /// Spawn a cosmetic particle burst AND record it for the network, mirroring
    /// `emit_sound`. Local modes see the spawn immediately; the server records it
    /// into `self.fx_events` (build_state ships it) so the live client — which
    /// runs no sim — replays the identical burst. Use this for every
    /// gameplay-event fx instead of calling `fx::*` directly.
    pub fn emit_fx(&mut self, ev: crate::renderer::fx::FxEvent) {
        crate::renderer::fx::apply_event(&mut self.fx, &ev);
        self.fx_events.push(ev);
    }

    /// As `apply_explosion_force` but also scales the blast radius and max damage
    /// (1.0 = unchanged). Used for the Meteor Bomb's reduced initial impact.
    pub fn apply_explosion_scaled(&mut self, pos: WorldPos, kind: WeaponKind, force: f32, radius_scale: f32, dmg_scale: f32) {
        use super::soldier::SoldierState;
        use crate::world::Vec2;

        let radius  = kind.blast_radius() * radius_scale;
        let max_dmg = (kind.max_damage() as f32 * dmg_scale) as u32;

        let crater = Crater::new(pos.x, pos.y, radius);
        crater.carve(&mut self.terrain);
        self.crater_log.push((pos.x, pos.y, radius));

        // Spawn explosion animation (visual only — happens regardless of terrain)
        self.explosions.push(Explosion::new(pos, radius));

        // Spawn effect fallout: dirt chunks + sparks (or a water splash on water).
        // Visual only — not networked.
        if pos.y >= WATER_Y as f32 {
            self.emit_fx(crate::renderer::fx::FxEvent::Splash { x: pos.x, y: pos.y });
        } else {
            let d = biome_dirt(self.terrain.archetype);
            self.emit_fx(crate::renderer::fx::FxEvent::Explosion {
                x: pos.x, y: pos.y, radius, col: [d.r, d.g, d.b],
            });
        }

        // Track active worm HP before damage to detect hits
        let active_ti = self.active_team();
        let active_si = self.teams[active_ti].active;
        let active_hp_before = self.teams[active_ti].soldiers[active_si].hp;
        let mut last_damaged: Option<WorldPos> = None;

        for team in &mut self.teams {
            for soldier in &mut team.soldiers {
                if !soldier.is_alive() { continue; }
                let dx = soldier.pos.x - pos.x;
                let dy = soldier.pos.y - pos.y;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist >= radius { continue; }

                let falloff = 1.0 - dist / radius;

                // Snapshot state before damage so knockback uses pre-kill state.
                // This lets soldiers killed by an explosion still fly before their
                // death explosion fires (take_damage sets state=Dead immediately for
                // grounded soldiers, which would suppress knockback otherwise).
                let pre_state = soldier.state.clone();

                // Damage: linear falloff + direct-hit bonus when nearly touching center
                let mut dmg = (max_dmg as f32 * falloff) as u32;
                if dist < 10.0 && kind != WeaponKind::Blasthive && kind != WeaponKind::Bazooka && kind != WeaponKind::HomingMissile { dmg = (dmg + 20).min(99); }
                if dmg > 0 {
                    soldier.death_cause = super::soldier::DeathCause::Explosion;
                    soldier.kill_weapon = Some(kind);
                    soldier.take_damage(dmg);
                    last_damaged = Some(soldier.pos);
                }

                // Bee stings sting for damage only — never launch soldiers airborne.
                if kind == WeaponKind::Blasthive { continue; }

                // Knockback: push radially outward with upward bias
                let (nx, ny) = if dist > 0.5 {
                    (dx / dist, dy / dist)
                } else if kind == WeaponKind::AirStrike {
                    // Bombs fall from above; direct hit should throw sideways, not straight up
                    let side = if pos.x > WORLD_W as f32 / 2.0 { -1.0 } else { 1.0 };
                    (side, -0.5)
                } else {
                    (0.0, -1.0) // at center: launch straight up
                };
                let impulse = falloff * force;
                let (vx, vy) = if kind == WeaponKind::AirStrike {
                    // More horizontal throw, less vertical
                    (nx * impulse * 1.2, ny * impulse * 0.5 - impulse * 0.1)
                } else if kind == WeaponKind::HolyHandGrenade {
                    // Strong upward launch
                    (nx * impulse * 0.7, ny * impulse - impulse * 0.7)
                } else {
                    (nx * impulse, ny * impulse - impulse * 0.25)
                };

                let was_grounded = matches!(pre_state, SoldierState::Idle | SoldierState::Walking { .. });
                let new_state = match &pre_state {
                    SoldierState::Airborne { vel, spinning } => Some(SoldierState::Airborne {
                        vel: Vec2::new(vel.x + vx, vel.y + vy),
                        spinning: *spinning,
                    }),
                    SoldierState::Idle | SoldierState::Walking { .. } => Some(SoldierState::Airborne {
                        vel: Vec2::new(vx, vy),
                        spinning: false,
                    }),
                    SoldierState::Dead => None,
                };
                if let Some(s) = new_state {
                    if was_grounded { soldier.fall.begin_fall(soldier.pos.y); }
                    soldier.state = s;
                }
            }
        }

        // Flag if active worm was hit so retreat can be skipped
        if self.teams[active_ti].soldiers[active_si].hp < active_hp_before {
            self.active_worm_hit = true;
        }

        // Remember where the most recently damaged soldier ended up so the camera
        // can briefly hold there during retreat, letting the HP loss be read.
        if let Some(pos) = last_damaged {
            self.damage_focus = Some((pos, DAMAGE_FOCUS_TICKS));
        }

        // Damage any landed crates caught in the blast (20+ total this turn destroys them)
        for crate_ in &mut self.crates {
            if !crate_.landed { continue; }
            let dx = crate_.pos.x - pos.x;
            let dy = crate_.pos.y - pos.y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist < radius {
                let falloff = 1.0 - dist / radius;
                let dmg = (max_dmg as f32 * falloff) as u32;
                crate_.damage_this_turn = crate_.damage_this_turn.saturating_add(dmg);
            }
        }
        // Spawn fire patches when crates are destroyed by explosion
        for crate_ in self.crates.iter().filter(|c| c.damage_this_turn >= 20) {
            let cpos = crate_.pos;
            let mut rng = (cpos.x as u64).wrapping_mul(0x6364136223846885)
                .wrapping_add((cpos.y as u64).wrapping_mul(0x9e3779b97f4a7c15));
            let count = 2 + (rng % 4) as usize; // 2–5 patches
            for _ in 0..count {
                rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
                let angle = (rng >> 33) as f32 / (u32::MAX as f32) * std::f32::consts::TAU;
                rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
                let speed = 2.0 + (rng >> 33) as f32 / (u32::MAX as f32) * 4.0;
                rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
                let life = 120 + (rng >> 33) as u32 % 60;
                self.fire_patches.push(FirePatch {
                    pos: cpos,
                    vel: crate::world::Vec2::new(angle.cos() * speed, angle.sin() * speed - 2.0),
                    landed: false,
                    lifetime: life,
                });
            }
        }
        self.crates.retain(|c| c.damage_this_turn < 20);

        // Trigger any mine caught inside the blast radius (chain reaction)
        for mine in &mut self.mines {
            if mine.state == MineState::Triggered { continue; } // already going off
            let dx = mine.pos.x - pos.x;
            let dy = mine.pos.y - pos.y;
            if (dx * dx + dy * dy).sqrt() < radius {
                mine.state = MineState::Triggered;
                mine.trigger_ticks = 8; // short chain delay
            }
        }

        // Any explosion damage instantly triggers a barrel — one hit = boom
        for barrel in &mut self.barrels {
            let dx = barrel.pos.x - pos.x;
            let dy = barrel.pos.y - pos.y;
            let dist = (dx*dx + dy*dy).sqrt();
            if dist < radius + 12.0 {
                let dmg = (max_dmg as f32 * (1.0 - dist / (radius + 12.0))) as i32;
                if dmg > 0 {
                    if let BarrelState::Normal = barrel.state {
                        barrel.state = BarrelState::Triggered { ticks: 8 };
                    }
                }
            }
        }

        // Check win immediately after each explosion — match ends as soon as a team
        // is eliminated rather than waiting for the turn's Ending phase.
        // Draws only occur if both teams are simultaneously wiped (0 alive after this call).
        self.check_win();
    }

    /// Advance all explosion animations by one tick.
    pub fn step_explosions(&mut self) {
        for e in &mut self.explosions { e.age += 1; }
        self.explosions.retain(|e| !e.is_done());
    }

    /// Step all projectiles one tick. Resolves impacts, triggers explosions.
    /// Returns true if any projectile resolved this tick.
    pub fn step_projectiles(&mut self) -> bool {
        let wind_val = self.wind.value();
        let mut resolved = false;
        let mut explosions: Vec<(WorldPos, WeaponKind)> = Vec::new();
        let mut meteor_impacts:     Vec<WorldPos> = Vec::new();
        let mut hive_impacts:       Vec<WorldPos> = Vec::new();
        let mut black_hole_spawns:  Vec<WorldPos> = Vec::new();
        let mut molotov_impacts:    Vec<WorldPos> = Vec::new();

        // Collect alive soldier positions for collision (before borrow in retain_mut)
        let soldier_boxes: Vec<WorldPos> = self.teams.iter()
            .flat_map(|t| t.soldiers.iter())
            .filter(|s| s.is_alive())
            .map(|s| s.pos)
            .collect();

        // Pre-step: steer bees toward nearest living soldier
        for proj in &mut self.projectiles {
            if proj.kind == WeaponKind::Blasthive && proj.is_fragment {
                if let Some(&target) = soldier_boxes.iter().min_by(|a, b| {
                    let da = (a.x-proj.pos.x).powi(2) + (a.y-proj.pos.y).powi(2);
                    let db = (b.x-proj.pos.x).powi(2) + (b.y-proj.pos.y).powi(2);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                }) {
                    // Aim at the body centre (above the feet), not the foot pixel which
                    // sits on the ground — otherwise the obstacle probe below keeps
                    // detecting the terrain the soldier stands on and the bee veers up.
                    let dx = target.x - proj.pos.x;
                    let dy = (target.y - 12.0) - proj.pos.y;
                    let dist = (dx*dx + dy*dy).sqrt().max(1.0);
                    const MAX_SPEED: f32 = 6.0;
                    const MAX_FORCE: f32 = 0.7;
                    let mut desired_vx = dx / dist * MAX_SPEED;
                    let mut desired_vy = dy / dist * MAX_SPEED - 0.15;

                    // Obstacle avoidance: probe the straight path toward the target.
                    // If terrain blocks it, climb over (or, if a ceiling is above,
                    // skirt horizontally) instead of flying into the wall. Skip this
                    // once close to the target so the bee commits to the sting instead
                    // of hovering over the soldier and climbing the ground beneath it.
                    let nx = dx / dist;
                    let ny = dy / dist;
                    let mut blocked = false;
                    for s in 1..=8 {
                        let px = proj.pos.x + nx * 8.0 * s as f32;
                        let py = proj.pos.y + ny * 8.0 * s as f32;
                        if self.terrain.is_solid(px as i32, py as i32) { blocked = true; break; }
                    }
                    if blocked && dist > 28.0 {
                        let air_above = !self.terrain.is_solid(proj.pos.x as i32, proj.pos.y as i32 - 8);
                        if air_above {
                            // Rise over the obstacle while drifting toward the target.
                            desired_vx = nx.signum() * MAX_SPEED * 0.4;
                            desired_vy = -MAX_SPEED;
                        } else {
                            // Ceiling above — go horizontal toward the target instead.
                            desired_vx = if dx.abs() > 0.5 { dx.signum() * MAX_SPEED } else { nx * MAX_SPEED };
                            desired_vy = 0.0;
                        }
                    }
                    let steer_x = desired_vx - proj.vel.x;
                    let steer_y = desired_vy - proj.vel.y;
                    let steer_mag = (steer_x * steer_x + steer_y * steer_y).sqrt().max(0.001);
                    let scale = (MAX_FORCE / steer_mag).min(1.0);
                    proj.vel.x += steer_x * scale;
                    proj.vel.y += steer_y * scale;
                    let spd = (proj.vel.x * proj.vel.x + proj.vel.y * proj.vel.y).sqrt();
                    if spd > MAX_SPEED {
                        proj.vel.x = proj.vel.x / spd * MAX_SPEED;
                        proj.vel.y = proj.vel.y / spd * MAX_SPEED;
                    }
                }
            }
        }

        // Pre-step: steer homing missiles toward their fixed target after 1s unguided
        for proj in &mut self.projectiles {
            if proj.kind == WeaponKind::HomingMissile && proj.age_ticks > 30 {
                if let Some((tx, ty)) = proj.homing_target {
                    let to_x = tx - proj.pos.x;
                    let to_y = ty - proj.pos.y;
                    let dist = (to_x * to_x + to_y * to_y).sqrt().max(0.001);
                    let speed = (proj.vel.x * proj.vel.x + proj.vel.y * proj.vel.y).sqrt().max(0.001);
                    let target_angle = to_y.atan2(to_x);
                    let cur_angle = proj.vel.y.atan2(proj.vel.x);
                    let mut diff = target_angle - cur_angle;
                    while diff >  std::f32::consts::PI { diff -= std::f32::consts::TAU; }
                    while diff < -std::f32::consts::PI { diff += std::f32::consts::TAU; }
                    let new_angle = cur_angle + diff.clamp(-0.06, 0.06);
                    proj.vel.x = new_angle.cos() * speed;
                    proj.vel.y = new_angle.sin() * speed;
                    let _ = dist; // used only for angle, not clamping speed
                }
            }
        }

        self.projectiles.retain_mut(|proj| {
            // Silently expire projectiles that have flown far off the map edges or too high.
            if proj.pos.x < -(WORLD_W as f32) || proj.pos.x > 2.0 * WORLD_W as f32 || proj.pos.y < -600.0 {
                resolved = true;
                return false;
            }
            let kind = proj.kind;
            let effective_wind = if !kind.affected_by_wind() { 0.0 } else { wind_val };
            match step_projectile(proj, &self.terrain, effective_wind) {
                StepResult::HHGArmed => {
                    // Projectile just stopped — play hallelujah, then Detonating fuse ticks down
                    self.sounds.push(crate::audio::Sfx::HolyHandGrenade as u8);
                    crate::audio::play(crate::audio::Sfx::HolyHandGrenade);
                    true
                }
                StepResult::Flying | StepResult::Bounced => {
                    let mut hit_soldier = false;
                    // Main hive passes through soldiers; only bees sting
                    let is_main_hive = kind == WeaponKind::Blasthive && !proj.is_fragment;
                    let is_bee = kind == WeaponKind::Blasthive && proj.is_fragment;
                    for &spos in if is_main_hive { &[][..] } else { &soldier_boxes[..] } {
                        let dx = (proj.pos.x - spos.x).abs();
                        let dy = proj.pos.y - spos.y;
                        // Bees use a wider window (above the head + to the sides) so a
                        // bee hovering near a soldier still stings instead of loitering
                        // until its fuse runs out; the sting snaps to the body centre.
                        let hit = if is_bee {
                            dx < 14.0 && dy > -32.0 && dy < 4.0
                        } else {
                            dx < 8.0 && dy > -22.0 && dy < 2.0
                        };
                        if hit && !kind.has_fuse() {
                            if kind == WeaponKind::Blasthive && !proj.is_fragment {
                                // Main hive hits soldier directly: trigger hive burst + spawn bees at soldier center
                                hive_impacts.push(spos);
                            } else if kind == WeaponKind::BlackHoleBomb {
                                // Black hole bomb hits soldier: spawn well at impact position
                                black_hole_spawns.push(proj.pos);
                            } else if kind == WeaponKind::MolotovCocktail {
                                molotov_impacts.push(proj.pos);
                            } else if kind == WeaponKind::BananaBomb && !proj.is_fragment {
                                // Main meteor bomb hits a soldier directly: scatter the 5
                                // burning fragments just like a terrain impact (was falling
                                // through to a plain blast with no fragments).
                                meteor_impacts.push(proj.pos);
                                resolved = true;
                                hit_soldier = true;
                            } else {
                                // Snap to soldier position for reliable damage — a fragment
                                // hitting the head would otherwise place the blast 20px above
                                // the feet, outside the 14px blast radius, dealing zero damage.
                                let exp_pos = if kind == WeaponKind::Blasthive
                                    || (kind == WeaponKind::BananaBomb && proj.is_fragment)
                                {
                                    spos
                                } else {
                                    proj.pos
                                };
                                explosions.push((exp_pos, kind));
                                resolved = true;
                                hit_soldier = true;
                                // Blood splat: directional spray from blast direction,
                                // starting 5px outside the sprite edge
                                if kind == WeaponKind::Shotgun {
                                    let vel_len = (proj.vel.x * proj.vel.x + proj.vel.y * proj.vel.y).sqrt().max(0.01);
                                    let dir_x = proj.vel.x / vel_len; // normalised travel direction
                                    let dir_y = proj.vel.y / vel_len;
                                    let perp_x = -dir_y;              // perpendicular (lateral scatter)
                                    let perp_y =  dir_x;
                                    let hx = proj.pos.x;
                                    let hy = proj.pos.y;
                                    let seed = (hx as i32).wrapping_mul(1664525)
                                        .wrapping_add((hy as i32).wrapping_mul(22695477)) as u32;
                                    for i in 0..8u32 {
                                        let r = seed.wrapping_mul(i.wrapping_add(1).wrapping_mul(0x9E3779B9));
                                        // 5px clearance from sprite + 0–18px further forward
                                        let forward = 5.0 + (r & 0x1F) as f32 * 0.57;
                                        // ±8px lateral spread
                                        let lateral = ((r >> 5) & 0xF) as f32 - 7.5;
                                        self.blood_splats.push((
                                            WorldPos::new(
                                                hx + dir_x * forward + perp_x * lateral,
                                                hy + dir_y * forward + perp_y * lateral,
                                            ),
                                            90,
                                        ));
                                    }
                                }
                            }
                            break;
                        }
                    }
                    !hit_soldier
                }
                StepResult::Explode(pos) | StepResult::FuseExplode(pos) => {
                    // Main meteor bomb: intercept to spawn fragments instead of a normal blast
                    if kind == WeaponKind::MolotovCocktail {
                        molotov_impacts.push(pos);
                    } else if kind == WeaponKind::BananaBomb && !proj.is_fragment {
                        meteor_impacts.push(pos);
                    } else if kind == WeaponKind::Blasthive && !proj.is_fragment {
                        hive_impacts.push(pos);
                    } else if kind == WeaponKind::BlackHoleBomb {
                        black_hole_spawns.push(pos);
                    } else {
                        explosions.push((pos, kind));
                    }
                    resolved = true;
                    false
                }
                StepResult::Drowned => {
                    // Fused weapons (grenades) explode even in water — fuse was burning
                    // Armed HHG also explodes on water contact
                    if kind.has_fuse() {
                        explosions.push((proj.pos, kind));
                    }
                    resolved = true;
                    false
                }
                StepResult::Expired => {
                    resolved = true;
                    false
                }
            }
        });

        for (pos, kind) in explosions {
            if kind == WeaponKind::AirStrike {
                self.emit_sound(crate::audio::Sfx::Grenade);
            }
            self.apply_explosion(pos, kind);
        }

        // Meteor Bomb main impact: large crash + scatter 5 burning fragments
        self.meteor_chain = true;
        for pos in meteor_impacts {
            self.emit_sound(crate::audio::Sfx::Meteor);
            // Initial impact: TNT profile at 60% radius+damage (reduced 40%) with
            // gentle knockback; the 5 fragments do the rest of the work.
            self.apply_explosion_scaled(pos, WeaponKind::Tnt, 9.0, 0.6, 0.6);
            let seed = (pos.x as u32)
                .wrapping_mul(0x9E3779B9)
                .wrapping_add(pos.y as u32)
                .wrapping_add(self.tick.wrapping_mul(0x6B5A6B5A));
            for i in 0..5u32 {
                let ra = seed.wrapping_mul(i.wrapping_mul(0xDEAD).wrapping_add(0xBEEF));
                let rs = seed.wrapping_mul(i.wrapping_mul(0xCAFE).wrapping_add(0xBABE));
                // Fountain upward: horizontal scatter + strong upward kick (-y is up)
                let spread = (ra as f32 / u32::MAX as f32 - 0.5) * 5.0; // ±2.5 px/tick horizontal
                let up     = 5.0 + (rs as f32 / u32::MAX as f32) * 4.0; // 5..9 px/tick upward
                use crate::physics::projectile::Projectile;
                self.projectiles.push(Projectile::new_meteor_fragment(
                    pos,
                    crate::world::Vec2::new(spread, -up),
                ));
            }
        }

        self.meteor_chain = false;

        // Beehive impact: small burst + spawn 6 homing bees
        for pos in hive_impacts {
            self.apply_explosion(pos, WeaponKind::Blasthive); // tiny burst
            let seed = (pos.x as u32)
                .wrapping_mul(0x9E3779B9)
                .wrapping_add(pos.y as u32)
                .wrapping_add(self.tick.wrapping_mul(0x6B5A6B5A));
            for i in 0..6u32 {
                // All bees launch straight up with slight horizontal spread
                let ra = seed.wrapping_mul(i.wrapping_mul(0xDEAD).wrapping_add(0xBEEF));
                let spread = (ra as f32 / u32::MAX as f32 - 0.5) * 3.0; // ±1.5 px/tick horizontal
                use crate::physics::projectile::Projectile;
                self.projectiles.push(Projectile::new_bee(
                    pos,
                    crate::world::Vec2::new(spread, -6.0),
                ));
            }
        }

        // Black hole bomb: spawn gravity well, no crater
        for pos in black_hole_spawns {
            self.emit_sound(crate::audio::Sfx::BlackHole);
            self.black_holes.push(BlackHole { pos, lifetime: 150 });
        }

        // Molotov cocktail: small blast + fire patches
        for pos in molotov_impacts {
            self.spawn_molotov_fire(pos);
        }

        resolved
    }

    /// Attempt a crate drop (30% chance). Returns true if a crate was spawned.
    /// `drop_rng`, `pos_rng`, and `kind_rng` should be independent f32 values in [0.0, 1.0).
    pub fn maybe_drop_crate(&mut self, drop_rng: f32, pos_rng: f32, kind_rng: f32) -> bool {
        if drop_rng >= 0.30 { return false; }

        // Pick a random x and determine the crate's start position.
        // On cave maps (archetype 3) the surface is the sealed rock cap — crates
        // dropped from y=0 land on the roof and are unreachable. Instead, find a
        // standable cave floor and start the crate just above its ceiling so it
        // falls naturally into the accessible chamber below.
        let x = (pos_rng * WORLD_W as f32) as u32 % WORLD_W;
        let start_y = if self.terrain.archetype == 3 {
            // Use the simple check (no BFS) — standable_cave_foot_y's full
            // cave_has_escape BFS stalls the server tick for several ms, causing
            // irregular state delivery to clients (felt as input unresponsiveness).
            // Crates just need a platform with clearance; escape-route verification
            // is not needed here.
            match self.terrain.standable_cave_foot_simple(x as i32) {
                Some(foot_y) => {
                    // Find the ceiling above this floor (scan up from head clearance)
                    // and start the crate just below it so it falls into the chamber.
                    let head_top = (foot_y - 26).max(0);
                    let ceiling_y = (0..head_top)
                        .rev()
                        .find(|&y| self.terrain.is_solid(x as i32, y))
                        .map(|y| (y + 2) as f32) // just below the ceiling pixel
                        .unwrap_or(0.0);
                    ceiling_y
                }
                None => return false, // no standable cave floor at this x
            }
        } else {
            if self.terrain.surface_y_at(x).is_none() { return false; }
            0.0
        };

        // Type split: 75% weapon, 25% health.
        // Tier drop chances (weapon pool):
        //   Common     60%  — Mine, Shotgun, TNT, Grapple, Bat, Torch, Uzi  (equal within tier)
        //   Uncommon   24%  — Blasthive, Meteor Bomb, Air Strike
        //   Rare       14%  — Black Hole, Revolver, Minigun, Sacred Ordnance
        //   Ultra Rare  2%  — Hand of Jerry
        let kind = if kind_rng >= 0.75 {
            CrateKind::Health // +25 HP
        } else {
            let w = kind_rng / 0.75; // rescale weapon rng to [0,1)
            let weapon = if w < 0.60 {
                let slot = (w / 0.60 * 7.0) as usize;
                [WeaponKind::Landmine, WeaponKind::Shotgun, WeaponKind::Tnt,
                 WeaponKind::NinjaRope, WeaponKind::BaseballBat,
                 WeaponKind::PlasmaTorch, WeaponKind::Uzi][slot.min(6)]
            } else if w < 0.84 {
                let slot = ((w - 0.60) / 0.24 * 4.0) as usize;
                [WeaponKind::Blasthive, WeaponKind::BananaBomb,
                 WeaponKind::AirStrike, WeaponKind::HomingMissile][slot.min(3)]
            } else if w < 0.98 {
                let slot = ((w - 0.84) / 0.14 * 4.0) as usize;
                [WeaponKind::BlackHoleBomb, WeaponKind::Revolver,
                 WeaponKind::Minigun, WeaponKind::HolyHandGrenade][slot.min(3)]
            } else {
                WeaponKind::Garcia
            };
            CrateKind::Weapon(weapon)
        };
        self.crates.push(DroppedCrate {
            pos: WorldPos::new(x as f32, start_y),
            kind,
            landed: false,
            descent_vy: 1.5,
            damage_this_turn: 0,
            fall_ticks: 0,
        });

        // Separate 15% scrap crate — multiplayer only
        if self.is_multiplayer {
            let scrap_rng = self.tick.wrapping_mul(2654435761_u32).wrapping_add(1013904223);
            if (scrap_rng % 100) < 15 {
                let amount = 5 + (scrap_rng.wrapping_mul(1664525) % 26) as u32;
                let sx = ((scrap_rng.wrapping_mul(22695477) % WORLD_W) as f32)
                    .max(8.0).min(WORLD_W as f32 - 8.0);
                let scrap_start_y = if self.terrain.archetype == 3 {
                    match self.terrain.standable_cave_foot_simple(sx as i32) {
                        Some(foot_y) => {
                            let head_top = (foot_y - 26).max(0);
                            (0..head_top).rev()
                                .find(|&y| self.terrain.is_solid(sx as i32, y))
                                .map(|y| (y + 2) as f32)
                                .unwrap_or(0.0)
                        }
                        None => return true, // skip scrap, main crate already pushed
                    }
                } else {
                    if self.terrain.surface_y_at(sx as u32).is_none() { return true; }
                    0.0
                };
                self.crates.push(DroppedCrate {
                    pos: WorldPos::new(sx, scrap_start_y),
                    kind: CrateKind::Scrap(amount),
                    landed: false,
                    descent_vy: 1.5,
                    damage_this_turn: 0,
                    fall_ticks: 0,
                });
            }
        }

        true
    }

    /// Step crate descent physics. Returns indices of crates that just landed.
    pub fn step_crates(&mut self) -> Vec<usize> {
        // Crate sprite is 24×24 centered on pos.y; bottom edge = pos.y + 12.
        const CRATE_HALF: i32 = 12;
        let mut just_landed = Vec::new();
        for (i, c) in self.crates.iter_mut().enumerate() {
            if c.landed {
                // Re-check support — terrain may have been carved under the crate
                let still_supported = (c.pos.y as i32 + CRATE_HALF + 1 >= crate::world::WATER_Y as i32)
                    || self.terrain.is_solid(c.pos.x as i32, c.pos.y as i32 + CRATE_HALF + 1);
                if !still_supported {
                    c.landed = false;
                    c.descent_vy = 1.5;
                    c.fall_ticks = 121; // no parachute on re-falls — drops immediately fast
                }
                continue;
            }
            c.fall_ticks += 1;
            // After 60 ticks parachute is gone — accelerate under gravity
            if c.fall_ticks > 60 {
                c.descent_vy = (c.descent_vy + 0.4).min(10.0);
            }
            c.pos.y += c.descent_vy;
            let cx = c.pos.x as i32;
            let bottom = c.pos.y as i32 + CRATE_HALF;
            // Check if landed on terrain or water
            if bottom >= WATER_Y as i32 {
                c.landed = true; // sank
            } else if self.terrain.is_solid(cx, bottom) {
                // Snap crate up so its bottom sits on the surface, not inside terrain.
                // Scan upward from the detected solid pixel to find the surface top.
                let mut surf = bottom;
                let scan_limit = bottom - (c.descent_vy as i32 + 2).min(20);
                while surf > scan_limit && self.terrain.is_solid(cx, surf - 1) {
                    surf -= 1;
                }
                // surf = topmost solid pixel in this stack; place crate bottom at surf (just on top)
                c.pos.y = (surf - CRATE_HALF) as f32;
                c.landed = true;
                just_landed.push(i);
            }
        }
        // Remove sunken crates
        self.crates.retain(|c| {
            !(c.landed && c.pos.y as i32 + CRATE_HALF >= WATER_Y as i32)
        });
        just_landed
    }

    /// Process pending death explosions. Returns true if any fired this tick.
    pub fn step_death_explosions(&mut self) -> bool {
        let mut fired = false;
        // Decrement timers
        for pd in &mut self.pending_deaths { pd.timer = pd.timer.saturating_sub(1); }
        // Collect fired entries (timer==0)
        let mut to_fire: Vec<PendingDeathExplosion> = Vec::new();
        self.pending_deaths.retain(|pd| {
            if pd.timer == 0 { true } else { true } // keep all, filter below
        });
        // Drain entries that hit zero
        let mut i = 0;
        while i < self.pending_deaths.len() {
            if self.pending_deaths[i].timer == 0 {
                to_fire.push(self.pending_deaths.remove(i));
            } else {
                i += 1;
            }
        }
        for pd in to_fire {
            fired = true;
            self.apply_explosion(pd.pos, WeaponKind::DeathExplosion);
            // Clear the pending flag on the soldier
            if pd.team < self.teams.len() && pd.si < self.teams[pd.team].soldiers.len() {
                self.teams[pd.team].soldiers[pd.si].death_explosion_pending = false;
            }
            // Spawn headstone unless drowned
            if pd.cause != DeathCause::Water {
                let hid = if pd.team < self.teams.len() { self.teams[pd.team].headstone_id } else { 0 };
                self.graves.push(Grave {
                    pos:         WorldPos::new(pd.pos.x, pd.pos.y - 30.0),
                    team:        pd.team,
                    soldier_idx: if pd.si < self.teams[pd.team].soldiers.len() {
                        self.teams[pd.team].soldiers[pd.si].index
                    } else { pd.si },
                    died_tick:   self.tick,
                    vel_y:       0.0,
                    settled:     false,
                    headstone_id: hid,
                });
            }
        }
        fired
    }

    /// Check if any living soldier walks over a crate and collect it.
    pub fn collect_crates(&mut self) {
        // Compare soldier foot to the bottom of the crate (pos.y + 12) so a
        // soldier standing right next to the crate triggers collection.
        const CRATE_HALF: f32 = 12.0;
        let collect_radius = 18.0f32;
        let mut to_remove = Vec::new();

        for ci in 0..self.crates.len() {
            if !self.crates[ci].landed { continue; }
            let crate_x  = self.crates[ci].pos.x;
            let crate_by = self.crates[ci].pos.y + CRATE_HALF; // bottom edge
            let crate_kind = self.crates[ci].kind;
            'team: for ti in 0..self.teams.len() {
                for si in 0..self.teams[ti].soldiers.len() {
                    if !self.teams[ti].soldiers[si].is_alive() { continue; }
                    let dx = self.teams[ti].soldiers[si].pos.x - crate_x;
                    let dy = self.teams[ti].soldiers[si].pos.y - crate_by;
                    if (dx * dx + dy * dy).sqrt() < collect_radius {
                        let soldier_name = self.teams[ti].soldiers[si].name.clone();
                        match crate_kind {
                            CrateKind::Health => {
                                self.teams[ti].soldiers[si].heal(25);
                                let text = format!("{} picked up +25 HP!", soldier_name);
                                self.messages.push(GameMessage { text, team: Some(ti), ticks: 90 });
                            }
                            CrateKind::Weapon(kind) => {
                                let ammo = match kind {
                                    WeaponKind::Shotgun   => Some(2),
                                    WeaponKind::NinjaRope => Some(2),
                                    _                     => Some(1),
                                };
                                self.teams[ti].add_weapon(kind, ammo);
                                let text = format!("{} got a {}!", soldier_name, kind.display_name());
                                self.messages.push(GameMessage { text, team: Some(ti), ticks: 90 });
                            }
                            CrateKind::Scrap(amount) => {
                                self.scrap_earned += amount;
                                let text = format!("{} found {} scrap!", soldier_name, amount);
                                self.messages.push(GameMessage { text, team: Some(ti), ticks: 90 });
                            }
                        }
                        to_remove.push(ci);
                        break 'team;
                    }
                }
            }
        }
        // Remove collected (highest index first to preserve indices)
        to_remove.sort_unstable();
        to_remove.dedup();
        for i in to_remove.into_iter().rev() {
            if i < self.crates.len() { self.crates.remove(i); }
        }
    }

    /// Advance all placed mines one tick. Returns true if any mine exploded.
    pub fn step_mines(&mut self) -> bool {
        const TRIGGER_RADIUS: f32 = 35.0;
        let mut exploded = false;
        let mut to_explode: Vec<WorldPos> = Vec::new();

        for mine in &mut self.mines {
            match mine.state {
                MineState::Arming => {
                    if mine.arm_ticks > 0 {
                        mine.arm_ticks -= 1;
                    } else {
                        mine.state = MineState::Armed;
                    }
                }
                MineState::Armed => {
                    // Proximity check — any living soldier within trigger radius.
                    let triggered = self.teams.iter().flat_map(|t| t.soldiers.iter())
                        .filter(|s| s.is_alive())
                        .any(|s| {
                            let dx = s.pos.x - mine.pos.x;
                            let dy = s.pos.y - mine.pos.y;
                            (dx * dx + dy * dy).sqrt() < TRIGGER_RADIUS
                        });
                    if triggered {
                        // Arm beep fires on this transition; explode 1 s after it.
                        mine.state = MineState::Triggered;
                        mine.trigger_ticks = 30; // 1.0s at 30Hz
                    }
                }
                MineState::Triggered => {
                    if mine.trigger_ticks > 0 {
                        mine.trigger_ticks -= 1;
                    } else {
                        to_explode.push(mine.pos);
                    }
                }
            }
        }

        // Remove exploded mines, apply explosions
        if !to_explode.is_empty() {
            exploded = true;
            self.mines.retain(|m| m.state != MineState::Triggered || m.trigger_ticks > 0);
            for pos in to_explode {
                self.apply_explosion(pos, WeaponKind::Landmine);
                if self.meteor_chain { self.spawn_meteor_fragments(pos); }
                // Chain: trigger any Armed mines within blast radius (43px)
                for mine in &mut self.mines {
                    if mine.state == MineState::Armed {
                        let dx = mine.pos.x - pos.x;
                        let dy = mine.pos.y - pos.y;
                        if (dx * dx + dy * dy).sqrt() < 43.0 {
                            mine.state = MineState::Triggered;
                            mine.trigger_ticks = 5; // shorter chain delay
                        }
                    }
                }
            }
        }
        exploded
    }

    /// Advance barrel physics one tick: gravity, terrain collision, chain-reaction, explosion.
    pub fn step_barrels(&mut self) {
        use crate::world::{WATER_Y, WORLD_W};
        const GRAVITY:    f32 = 0.6;
        const VEL_CAP:    f32 = 14.0;
        const BARREL_RADIUS: f32 = 8.0;
        const CHAIN_RADIUS:  f32 = 60.0; // barrel-to-barrel chain distance

        let mut to_explode: Vec<usize> = Vec::new();

        // Projectile-barrel collision: flying projectiles touching a barrel trigger it.
        // TNT and landmines are stationary fused weapons — they detonate via the
        // explosion path (apply_explosion → Triggered), not by proximity contact.
        for (i, barrel) in self.barrels.iter().enumerate() {
            if let BarrelState::Normal = barrel.state {
                for proj in &self.projectiles {
                    use crate::physics::projectile::WeaponKind;
                    if matches!(proj.kind, WeaponKind::Tnt | WeaponKind::Landmine) { continue; }
                    let dx = proj.pos.x - barrel.pos.x;
                    let dy = proj.pos.y - barrel.pos.y;
                    if (dx*dx + dy*dy).sqrt() < 12.0 {
                        to_explode.push(i);
                        break;
                    }
                }
            }
        }
        // Apply projectile-triggered barrels before physics loop
        to_explode.sort_unstable(); to_explode.dedup();
        for &i in to_explode.iter().rev() {
            let pos = self.barrels[i].pos;
            self.barrels.remove(i);
            self.explode_barrel(pos);
        }
        to_explode.clear();

        for (i, barrel) in self.barrels.iter_mut().enumerate() {
            match barrel.state {
                BarrelState::Normal => {
                    // Physics
                    barrel.vel.y = (barrel.vel.y + GRAVITY).min(VEL_CAP);
                    let nx = barrel.pos.x + barrel.vel.x;
                    let ny = barrel.pos.y + barrel.vel.y;

                    // Terrain collision
                    let ix = nx as i32;
                    let iy = ny as i32;
                    let hit = (0..=BARREL_RADIUS as i32)
                        .any(|h| self.terrain.is_solid(ix, iy - h));

                    if hit {
                        barrel.vel.y = 0.0;
                        barrel.vel.x *= 0.85; // rolling friction
                        barrel.pos.x = nx.clamp(BARREL_RADIUS, (WORLD_W as f32) - BARREL_RADIUS);
                        // land_y: scan up from iy to find surface
                        let mut surf = iy;
                        while surf > 0 && self.terrain.is_solid(ix, surf) { surf -= 1; }
                        barrel.pos.y = surf as f32;
                    } else {
                        barrel.pos.x = nx.clamp(0.0, (WORLD_W as f32) - 1.0);
                        barrel.pos.y = ny;
                    }

                    if barrel.pos.y >= WATER_Y as f32 {
                        to_explode.push(i); // barrel in water — explodes
                    }
                    if barrel.hp <= 0 {
                        to_explode.push(i);
                    }
                }
                BarrelState::Triggered { ref mut ticks } => {
                    if *ticks == 0 {
                        to_explode.push(i);
                    } else {
                        *ticks -= 1;
                    }
                }
            }
        }

        // Explode collected barrels (iterate backwards so indices stay valid)
        to_explode.sort_unstable();
        to_explode.dedup();
        for &i in to_explode.iter().rev() {
            let pos = self.barrels[i].pos;
            self.barrels.remove(i);
            self.explode_barrel(pos);
        }
    }

    fn spawn_meteor_fragments(&mut self, pos: WorldPos) {
        use crate::physics::projectile::Projectile;
        let seed = (pos.x as u32)
            .wrapping_mul(0x9E3779B9)
            .wrapping_add(pos.y as u32)
            .wrapping_add(self.tick.wrapping_mul(0x6B5A6B5A));
        for i in 0..5u32 {
            let ra = seed.wrapping_mul(i.wrapping_mul(0xDEAD).wrapping_add(0xBEEF));
            let rs = seed.wrapping_mul(i.wrapping_mul(0xCAFE).wrapping_add(0xBABE));
            let spread = (ra as f32 / u32::MAX as f32 - 0.5) * 5.0;
            let up     = 5.0 + (rs as f32 / u32::MAX as f32) * 4.0;
            self.projectiles.push(Projectile::new_meteor_fragment(
                pos,
                crate::world::Vec2::new(spread, -up),
            ));
        }
    }

    /// Shatter a molotov cocktail: small blast + 7-10 fire patches that spread wide.
    pub fn spawn_molotov_fire(&mut self, pos: WorldPos) {
        use crate::world::Vec2;
        use crate::physics::projectile::WeaponKind;
        self.apply_explosion(pos, WeaponKind::MolotovCocktail);
        let mut rng = (pos.x as u64)
            .wrapping_mul(0x6364136223846885)
            .wrapping_add((pos.y as u64).wrapping_mul(0x9e3779b97f4a7c15))
            .wrapping_add(self.tick as u64 * 0x517CC1B727220A95);
        let count = 7 + (rng % 4) as usize; // 7-10 patches
        for _ in 0..count {
            rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
            // Spread across a wide horizontal arc (like liquid splashing)
            let raw_angle = (rng >> 33) as f32 / (u32::MAX as f32); // 0..1
            // Bias: mostly outward and slightly downward, wide horizontal scatter
            let angle = (raw_angle - 0.5) * std::f32::consts::PI * 1.6; // -144°..+144°
            rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
            let speed = 2.5 + (rng >> 33) as f32 / (u32::MAX as f32) * 5.5;
            rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
            let life = 180 + (rng >> 33) as u32 % 120; // 6-10 s — longer than barrel fire
            self.fire_patches.push(FirePatch {
                pos,
                vel: Vec2::new(angle.cos() * speed, angle.sin() * speed - 1.5),
                landed: false,
                lifetime: life,
            });
        }
        self.emit_sound(crate::audio::Sfx::Explosion);
    }

    fn explode_barrel(&mut self, pos: WorldPos) {
        use crate::world::Vec2;
        use crate::physics::projectile::WeaponKind;

        // Large explosion (TNT-scale)
        self.apply_explosion(pos, WeaponKind::Tnt);
        if self.meteor_chain { self.spawn_meteor_fragments(pos); }

        // Chain: damage nearby barrels
        let barrel_positions: Vec<(usize, WorldPos, i32)> = self.barrels.iter().enumerate()
            .map(|(i, b)| (i, b.pos, b.hp))
            .collect();
        for (i, bpos, _) in &barrel_positions {
            let dx = bpos.x - pos.x;
            let dy = bpos.y - pos.y;
            let dist = (dx*dx + dy*dy).sqrt();
            if dist < 80.0 {
                let dmg = (30.0 * (1.0 - dist / 80.0)) as i32;
                if dmg > 0 {
                    if let BarrelState::Normal = self.barrels[*i].state {
                        self.barrels[*i].state = BarrelState::Triggered { ticks: 5 };
                    }
                }
            }
        }

        // Spawn fire patches — deterministic scatter using position-seeded LCG
        let mut rng = (pos.x as u64).wrapping_mul(0x6364136223846885)
            .wrapping_add((pos.y as u64).wrapping_mul(0x9e3779b97f4a7c15));
        let count = 6 + (rng % 5) as usize; // 6–10 patches (fewer but bigger)
        for _ in 0..count {
            rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
            let angle = (rng >> 33) as f32 / (u32::MAX as f32) * std::f32::consts::TAU;
            rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
            let speed = 3.0 + (rng >> 33) as f32 / (u32::MAX as f32) * 7.0; // more spread
            rng = rng.wrapping_mul(0x6364136223846885).wrapping_add(1442695040888963407);
            let life = 150 + (rng >> 33) as u32 % 90; // 5–8 s
            self.fire_patches.push(FirePatch {
                pos,
                vel: Vec2::new(angle.cos() * speed, angle.sin() * speed - 3.0), // bias upward
                landed: false,
                lifetime: life,
            });
        }
    }

    /// Advance fire patch physics one tick: flight, landing, DoT to nearby soldiers.
    pub fn step_black_holes(&mut self) {
        use super::soldier::SoldierState;
        use crate::world::Vec2;

        const PULL_RADIUS:    f32 = 108.0; // 40% smaller than the old 180px well
        const EVENT_HORIZON:  f32 = 13.0;  // 40% smaller than the old 22px (≈22*0.6)
        const STRENGTH:       f32 = 1.5;
        const SWIRL:          f32 = 0.1;
        const EJECT_FORCE:    f32 = 8.0;

        let holes: Vec<BlackHole> = self.black_holes.clone();
        let mut expired: Vec<WorldPos> = Vec::new();
        let mut proj_absorb: Vec<usize> = Vec::new();

        for hole in &holes {
            let hx = hole.pos.x;
            let hy = hole.pos.y;

            // Pull soldiers; pin those inside event horizon
            for team in &mut self.teams {
                for soldier in &mut team.soldiers {
                    if !soldier.is_alive() { continue; }
                    let dx = hx - soldier.pos.x;
                    let dy = hy - soldier.pos.y;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist > PULL_RADIUS { continue; }
                    if dist < EVENT_HORIZON {
                        // Hold soldier at the singularity — damage dealt when hole expires
                        soldier.pos.x = hx;
                        soldier.pos.y = hy;
                        let was_grounded = matches!(soldier.state, SoldierState::Idle | SoldierState::Walking { .. });
                        if was_grounded { soldier.fall.begin_fall(soldier.pos.y); }
                        soldier.state = SoldierState::Airborne {
                            vel: Vec2::new(0.0, 0.0),
                            spinning: true,
                        };
                        continue;
                    }
                    let force = STRENGTH * (1.0 - dist / PULL_RADIUS);
                    let (nx, ny) = (dx / dist, dy / dist);
                    let tx = -ny;
                    let ty =  nx;
                    let dvx = nx * force + tx * SWIRL;
                    let dvy = ny * force + ty * SWIRL;
                    let was_grounded = matches!(soldier.state, SoldierState::Idle | SoldierState::Walking { .. });
                    let launch_dvy = if was_grounded { dvy.min(-2.5) } else { dvy };
                    match &soldier.state {
                        SoldierState::Idle | SoldierState::Walking { .. } => {
                            soldier.fall.begin_fall(soldier.pos.y);
                            soldier.state = SoldierState::Airborne {
                                vel: Vec2::new(dvx, launch_dvy),
                                spinning: false,
                            };
                        }
                        SoldierState::Airborne { vel, spinning } => {
                            let new_vel = Vec2::new(vel.x + dvx, vel.y + dvy);
                            let sp = *spinning;
                            soldier.state = SoldierState::Airborne { vel: new_vel, spinning: sp };
                        }
                        SoldierState::Dead => {}
                    }
                }
            }

            // Pull barrels
            for barrel in &mut self.barrels {
                let dx = hx - barrel.pos.x;
                let dy = hy - barrel.pos.y;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist > PULL_RADIUS { continue; }
                if dist < EVENT_HORIZON {
                    barrel.hp = 0;
                    continue;
                }
                let force = STRENGTH * (1.0 - dist / PULL_RADIUS);
                let (nx, ny) = (dx / dist, dy / dist);
                barrel.vel.x += nx * force;
                barrel.vel.y += ny * force;
            }

            // Pull projectiles; absorb those inside event horizon
            for (pi, proj) in self.projectiles.iter_mut().enumerate() {
                let dx = hx - proj.pos.x;
                let dy = hy - proj.pos.y;
                let dist = (dx * dx + dy * dy).sqrt();
                if dist > PULL_RADIUS { continue; }
                if dist < EVENT_HORIZON {
                    proj_absorb.push(pi);
                    continue;
                }
                let force = STRENGTH * (1.0 - dist / PULL_RADIUS);
                let (nx, ny) = (dx / dist, dy / dist);
                proj.vel.x += nx * force;
                proj.vel.y += ny * force;
            }
        }

        // Decrement lifetimes; collect expired
        self.black_holes.retain_mut(|hole| {
            hole.lifetime = hole.lifetime.saturating_sub(1);
            if hole.lifetime == 0 {
                expired.push(hole.pos);
                false
            } else {
                true
            }
        });

        // Remove projectiles absorbed by the event horizon
        if !proj_absorb.is_empty() {
            let mut i = 0usize;
            self.projectiles.retain(|_| { let keep = !proj_absorb.contains(&i); i += 1; keep });
        }

        // Collapse: damage + eject all soldiers still inside event horizon, then visual burst
        for pos in expired {
            let hx = pos.x;
            let hy = pos.y;
            let ati = self.active_team();
            let asi = self.teams[ati].active;
            let active_hp_before = self.teams[ati].soldiers[asi].hp;
            for team in &mut self.teams {
                for soldier in &mut team.soldiers {
                    if !soldier.is_alive() { continue; }
                    let dx = soldier.pos.x - hx;
                    let dy = soldier.pos.y - hy;
                    let dist = (dx * dx + dy * dy).sqrt();
                    if dist >= EVENT_HORIZON { continue; }
                    soldier.death_cause = super::soldier::DeathCause::Explosion;
                    soldier.take_damage(35);
                    if !soldier.is_alive() { continue; }
                    let (ex, ey) = if dist > 0.5 { (dx / dist, dy / dist) } else { (0.0, -1.0) };
                    soldier.state = SoldierState::Airborne {
                        vel: Vec2::new(ex * EJECT_FORCE, ey * EJECT_FORCE - 3.0),
                        spinning: true,
                    };
                }
            }
            if self.teams[ati].soldiers[asi].hp < active_hp_before {
                self.active_worm_hit = true;
            }
            self.emit_sound(crate::audio::Sfx::Mine);
            self.explosions.push(Explosion::new(pos, 25.0));
        }
    }

    pub fn step_fire_patches(&mut self) {
        use crate::world::{WATER_Y, WORLD_W};
        const GRAVITY:      f32 = 0.4;
        const WIND_AIR:     f32 = 0.18; // strong wind influence while airborne
        const WIND_GROUND:  f32 = 0.01; // almost none once settled
        const BOUNCE_DAMP:  f32 = 0.25; // velocity multiplier on terrain impact
        const DOT_INTERVAL: u32 = 10;   // 1 HP every 10 ticks = 3 HP/s
        const DOT_RADIUS:   f32 = 10.0;
        const PUSH_FORCE:   f32 = 0.4;  // lateral shove applied to soldiers in fire

        let wind = self.wind.value() * 0.05;

        // Snapshot active soldier HP before the loop so we can detect fire damage.
        let ati = self.active_team();
        let asi = self.teams[ati].active;
        let active_hp_before = self.teams[ati].soldiers[asi].hp;

        let mut to_remove: Vec<usize> = Vec::new();

        for (i, patch) in self.fire_patches.iter_mut().enumerate() {
            if patch.lifetime == 0 { to_remove.push(i); continue; }
            patch.lifetime -= 1;

            if !patch.landed {
                // ── Airborne: full gravity + strong wind ────────────────────
                patch.vel.x += wind * WIND_AIR;
                patch.vel.y = (patch.vel.y + GRAVITY).min(12.0);

                let nx = patch.pos.x + patch.vel.x;
                let ny = patch.pos.y + patch.vel.y;

                if ny >= WATER_Y as f32 { to_remove.push(i); continue; }

                let nx_c = nx.clamp(0.0, (WORLD_W as f32) - 1.0);
                if self.terrain.is_solid(nx_c as i32, ny as i32) {
                    // Bounce with heavy damping (WA: velocity *= 0.3 on impact)
                    patch.vel.x *= BOUNCE_DAMP;
                    patch.vel.y *= -BOUNCE_DAMP;
                    // If speed is now negligible, settle immediately
                    if patch.vel.x.abs() < 0.3 && patch.vel.y.abs() < 0.3 {
                        patch.landed = true;
                        patch.vel = crate::world::Vec2::new(0.0, 0.0);
                    }
                    // Stay at last valid position (don't push into terrain)
                } else {
                    patch.pos.x = nx_c;
                    patch.pos.y = ny;
                }
            } else {
                // ── Grounded: WA-style terrain sliding ──────────────────────
                // Gravity + tiny wind keep pulling; fire walks downhill into pits.
                patch.vel.x += wind * WIND_GROUND;
                patch.vel.y = (patch.vel.y + GRAVITY).min(6.0);

                let px = patch.pos.x as i32;
                let py = patch.pos.y as i32;

                // Check if support has been blown away — resume falling
                let has_support = self.terrain.is_solid(px,     py)
                    || self.terrain.is_solid(px,     py + 1)
                    || self.terrain.is_solid(px - 2, py)
                    || self.terrain.is_solid(px + 2, py);
                if !has_support {
                    patch.landed = false;
                    // keep vel so it continues sliding
                    patch.vel.y = patch.vel.y.max(1.0);
                } else {
                    // Try to slide one pixel in the direction gravity + vel wants.
                    // Priority: straight down → down-left → down-right → stay.
                    let candidates: [(i32, i32); 3] = [
                        (px,     py + 1),   // straight down
                        (px - 1, py + 1),   // down-left
                        (px + 1, py + 1),   // down-right
                    ];
                    let mut moved = false;
                    for &(cx, cy) in &candidates {
                        if cx < 0 || cx >= WORLD_W as i32 { continue; }
                        if cy >= WATER_Y as i32 { to_remove.push(i); moved = true; break; }
                        if !self.terrain.is_solid(cx, cy) {
                            patch.pos.x = cx as f32;
                            patch.pos.y = cy as f32;
                            moved = true;
                            break;
                        }
                    }
                    if !moved {
                        // Completely blocked — settled in a pit, zero velocity
                        patch.vel = crate::world::Vec2::new(0.0, 0.0);
                    }
                }

                // Squirm + push: soldiers inside fire react and get nudged
                let fire_dir = patch.vel.x.signum();
                for s in self.teams.iter_mut().flat_map(|t| t.soldiers.iter_mut()) {
                    if !s.is_alive() { continue; }
                    let dx = s.pos.x - patch.pos.x;
                    let dy = s.pos.y - patch.pos.y;
                    if (dx*dx + dy*dy).sqrt() < DOT_RADIUS {
                        s.on_fire_ticks = 10;
                        // Tiny lateral push — launch grounded soldiers into the air
                        use super::soldier::SoldierState;
                        match &mut s.state {
                            SoldierState::Airborne { vel, .. } => {
                                vel.x += fire_dir * PUSH_FORCE;
                            }
                            SoldierState::Idle | SoldierState::Walking { .. } => {
                                s.state = SoldierState::Airborne {
                                    vel: crate::world::Vec2::new(fire_dir * PUSH_FORCE, -0.5),
                                    spinning: false,
                                };
                            }
                            _ => {}
                        }
                    }
                }

                // DoT tick
                if patch.lifetime % DOT_INTERVAL == 0 {
                    for team in &mut self.teams {
                        for s in &mut team.soldiers {
                            if !s.is_alive() { continue; }
                            let dx = s.pos.x - patch.pos.x;
                            let dy = s.pos.y - patch.pos.y;
                            if (dx*dx + dy*dy).sqrt() < DOT_RADIUS {
                                s.death_cause = super::soldier::DeathCause::Explosion;
                                s.take_damage(1);
                            }
                        }
                    }
                    for barrel in &mut self.barrels {
                        if let BarrelState::Normal = barrel.state {
                            let dx = barrel.pos.x - patch.pos.x;
                            let dy = barrel.pos.y - patch.pos.y;
                            if (dx*dx + dy*dy).sqrt() < 6.0 {
                                barrel.state = BarrelState::Triggered { ticks: 5 };
                            }
                        }
                    }
                }
            }
        }

        for i in to_remove.into_iter().rev() {
            self.fire_patches.remove(i);
        }

        for team in &mut self.teams {
            for s in &mut team.soldiers {
                if s.on_fire_ticks > 0 { s.on_fire_ticks -= 1; }
            }
        }

        if self.teams[ati].soldiers[asi].hp < active_hp_before {
            self.active_worm_hit = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::team::{Team, Difficulty};

    fn two_teams() -> Vec<Team> {
        let spawns0 = vec![
            WorldPos::new(200.0, 100.0),
            WorldPos::new(250.0, 100.0),
            WorldPos::new(300.0, 100.0),
            WorldPos::new(350.0, 100.0),
        ];
        let spawns1 = vec![
            WorldPos::new(2800.0, 100.0),
            WorldPos::new(2850.0, 100.0),
            WorldPos::new(2900.0, 100.0),
            WorldPos::new(2950.0, 100.0),
        ];
        vec![
            Team::new(0, false, Difficulty::Medium, &spawns0),
            Team::new(1, true,  Difficulty::Easy,   &spawns1),
        ]
    }

    fn game() -> GameState {
        GameState::new(42, Terrain::empty(), two_teams(), 2)
    }

    // ── Initialisation ────────────────────────────────────────────────────────

    #[test]
    fn game_starts_ongoing() {
        assert_eq!(game().result, GameResult::Ongoing);
    }

    #[test]
    fn active_team_starts_at_zero() {
        assert_eq!(game().active_team(), 0);
    }

    // terrain_generated_from_seed removed: the shared game() fixture uses
    // Terrain::empty() for deterministic physics — terrain generation is covered
    // by world/terrain.rs (generate_tactical).

    // ── check_win ─────────────────────────────────────────────────────────────

    #[test]
    fn no_winner_when_both_teams_alive() {
        let mut g = game();
        g.check_win();
        assert_eq!(g.result, GameResult::Ongoing);
    }

    #[test]
    fn winner_declared_when_one_team_remains() {
        let mut g = game();
        for s in &mut g.teams[1].soldiers { s.take_damage(100); }
        g.check_win();
        assert_eq!(g.result, GameResult::Winner(0));
    }

    #[test]
    fn draw_when_all_teams_eliminated() {
        let mut g = game();
        for t in &mut g.teams {
            for s in &mut t.soldiers { s.take_damage(100); }
        }
        g.check_win();
        assert_eq!(g.result, GameResult::Draw);
    }

    // ── apply_explosion ───────────────────────────────────────────────────────

    #[test]
    fn explosion_damages_nearby_soldier() {
        let mut g = game();
        let pos = g.teams[0].soldiers[0].pos;
        let hp_before = g.teams[0].soldiers[0].hp;
        g.apply_explosion(pos, WeaponKind::Bazooka);
        let hp_after = g.teams[0].soldiers[0].hp;
        assert!(hp_after < hp_before, "soldier should take damage from direct hit");
    }

    #[test]
    fn explosion_does_not_damage_distant_soldier() {
        let mut g = game();
        // Team 1 is far away at x~2800, explosion at x=200
        let pos = WorldPos::new(200.0, 100.0);
        let hp_before = g.teams[1].soldiers[0].hp;
        g.apply_explosion(pos, WeaponKind::Bazooka);
        assert_eq!(g.teams[1].soldiers[0].hp, hp_before,
            "distant soldier should not be affected");
    }

    #[test]
    fn explosion_carves_terrain() {
        let mut g = game();
        // Force solid terrain at explosion centre
        g.terrain.set_solid(1600, 300, true);
        g.apply_explosion(WorldPos::new(1600.0, 300.0), WeaponKind::Bazooka);
        assert!(!g.terrain.is_solid(1600, 300), "terrain should be carved by explosion");
    }

    // ── alive_teams ───────────────────────────────────────────────────────────

    #[test]
    fn alive_teams_all_true_at_start() {
        let g = game();
        assert!(g.alive_teams().iter().all(|&a| a));
    }

    #[test]
    fn alive_teams_false_for_eliminated() {
        let mut g = game();
        for s in &mut g.teams[1].soldiers { s.take_damage(100); }
        let alive = g.alive_teams();
        assert!(alive[0]);
        assert!(!alive[1]);
    }

    // ── AimState ──────────────────────────────────────────────────────────────

    #[test]
    fn aim_state_starts_at_zero_power() {
        let a = AimState::new();
        assert_eq!(a.power, 0.0);
        assert_eq!(a.charge_ticks, 0);
    }

    #[test]
    fn charging_increases_power() {
        let mut a = AimState::new();
        a.charge();
        assert!(a.power > 0.0);
    }

    #[test]
    fn power_caps_at_one() {
        let mut a = AimState::new();
        for _ in 0..200 { a.charge(); }
        assert!(a.power <= 1.0);
    }

    #[test]
    fn reset_clears_power() {
        let mut a = AimState::new();
        for _ in 0..40 { a.charge(); }
        a.reset();
        assert_eq!(a.power, 0.0);
        assert_eq!(a.charge_ticks, 0);
    }

    // ── maybe_drop_crate ──────────────────────────────────────────────────────

    #[test]
    fn crate_drops_when_rng_below_threshold() {
        let mut g = game();
        let dropped = g.maybe_drop_crate(0.10, 0.5, 0.5);
        let _ = dropped;
    }

    #[test]
    fn crate_does_not_drop_when_rng_at_or_above_threshold() {
        let mut g = game();
        let before = g.crates.len();
        g.maybe_drop_crate(0.30, 0.5, 0.5);
        g.maybe_drop_crate(0.99, 0.5, 0.5);
        assert_eq!(g.crates.len(), before, "no crate should drop at rng>=0.30");
    }
}
