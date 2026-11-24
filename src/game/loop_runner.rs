//! 20 Hz game tick — input → physics → render.
//!
//! The WorldBuffer is 1600×480 (world-wide).
//! blit_to_fb copies columns [cam_x .. cam_x+640] to the framebuffer.
//! So EVERYTHING must be drawn at world coordinates (no cam_x subtraction
//! in the render path — that happens in blit_to_fb automatically).
//! The one exception: draw_hud_world adds cam_x to every x so the HUD
//! stays anchored to the screen rather than the world.

use crate::input::{InputState, Button};
use crate::renderer::{
    WorldBuffer, Bgra,
    draw_sprites::{draw_soldier, draw_soldier_v3, draw_projectile, draw_grenade_projectile, draw_aim_arrow, draw_headstone, draw_explosion, draw_garcia_sprite, draw_jumpbot_sprite, draw_jetpack},
    skeleton::{draw_soldier_skeletal, SoldierAnim},
    draw_terrain,
    hud::{draw_game_over, draw_pause_menu, TeamEndStat},
    camera::Camera,
};
use super::state::{GameState, GameMessage, RopeState};
use super::soldier::SoldierState;

// ── Grappling hook constants ──────────────────────────────────────────────────
const ROPE_HOOK_SPEED:    f32 = 33.0;  // px/tick — reference extends 20 px/frame @50fps = 1000 px/s → /30fps ≈ 33
const ROPE_SWING_FORCE:   f32 = 0.278; // HORIZONTAL impulse — reference FUN_00513f00: (right−left)×0x1999
                                       // = 0.1 px/frame² @50fps applied straight to vx (no rotation) → ×(50/30)²
const ROPE_GRAVITY:       f32 = 0.8;   // pendulum gravity, matched to reference: 0.3 px/frame² @ 50fps
                                       // → 0.3·50²=750 px/s² → /30² ≈ 0.83 px/tick² (worm-scale ~0.73); use 0.8
const ROPE_RETRACT:       f32 = 6.7;   // px/tick reel — reference: 4 px/frame @50fps (0x40000) = 200 px/s → /30
                                       // reference rope: 8px segments, 64 max = 512px, min ~10px (ours kept screen-tuned below)
const ROPE_MIN_LEN:       f32 = 10.0;  // reference FUN_00500d00: rejects lengths < 0xa0000 = 10px
const ROPE_MAX_LEN:       f32 = 512.0; // reference: 64 segments × 8px
const ROPE_MAX_SPEED:     f32 = 60.0;  // anti-tunnel guard only — reference is uncapped; swept collision is the real safety
const ROPE_AIR_GRAVITY:   f32 = 0.8;   // worm gravity (0.3 @50fps) applied to the active soldier while
                                       // airborne during a rope session, so detach arcs match the swing
// Firing-angle limit: restricted mode can't fire below horizontal; unrestricted
// allows 54° below horizon (-0.9425 rad). Compile-time toggle to avoid a synced field.
const ROPE_MAX_DOWN_ANGLE:            f32 = 0.0;
const ROPE_MAX_DOWN_ANGLE_UNRESTRICT: f32 = -0.9425;
const ROPE_UNRESTRICT:                bool = false;
const ROPE_KNOCK_STRENGTH: f32 = 0.7;  // fraction of roper velocity imparted to a knocked soldier
const ROPE_KNOCK_MIN_SPEED: f32 = 3.0; // roper must move at least this fast to knock
const ROPE_RETREAT_TICKS:  u32 = 45;   // ~1.5s move window after the rope is put away

// ── Message pools ─────────────────────────────────────────────────────────────

const TURN_MSGS: &[&str] = &[
    "Bold move.",
    "Think fast.",
    "No pressure.",
    "Choose wisely.",
    "This could hurt.",
    "Wind's judging you.",
    "Physics are a suggestion.",
    "Something will explode.",
    "Make it count.",
    "Eyes on target.",
    "Fortune favours the bold.",
    "You've got this. Maybe.",
    "The terrain disagrees.",
    "Steady hands.",
    "No going back now.",
    "Trust the trajectory.",
    "Someone's gonna feel that.",
    "Chaos has a plan.",
    "One shot. Probably.",
    "The clock is ticking.",
];

const CRATE_MSGS: &[&str] = &[
    "Eyes up.",
    "Grab it first.",
    "Something useful dropped.",
    "Package incoming.",
    "Free real estate.",
    "First come, first served.",
    "Airdrop detected.",
    "A treat from above.",
    "The sky is generous.",
    "It's raining loot.",
    "Someone ordered supplies.",
    "That's yours if you want it.",
];

fn lcg_pick<'a>(pool: &[&'a str], seed: u32) -> &'a str {
    pool[(seed.wrapping_mul(2654435761) as usize) % pool.len()]
}

pub fn push_turn_message(game: &mut GameState) {
    let ti   = game.active_team();
    let name = game.teams[ti].name.clone();
    let text = format!("{}'s turn", name);
    game.messages.push(GameMessage { text, team: Some(ti), ticks: 60 }); // 2s
}

pub fn push_crate_message(game: &mut GameState) {
    let seed = game.turn.turn_number.wrapping_mul(0xBEEF_CAFE);
    let text = lcg_pick(CRATE_MSGS, seed).to_string();
    game.messages.push(GameMessage { text, team: None, ticks: 90 });
}

// ── Loop state ───────────────────────────────────────────────────────────────

pub struct LoopState {
    pub paused:             bool,
    /// TEST mode "PAUSE GAME": everything frozen including the turn timer, menu hidden (see `tick`).
    pub frozen:             bool,
    pub pause_open_tick:    u32,
    pub tick:               u32,
    pub pause_cursor:       usize,
    pub weapon_menu_open:   bool,
    pub weapon_menu_cursor: usize,
    /// Ticks remaining during which fire input is suppressed (after weapon select confirm).
    pub fire_grace:         u8,
    /// Turn number observed last frame — used by update_camera() to snap the
    /// camera to the new active soldier the frame the turn advances.
    pub prev_turn_number:   u32,
    /// Last three per-tick soldier positions (render-side only): averaging them turns WA's uneven 1-2 frame
    /// ticks into steady motion for soldiers whose `wa_vis` isn't known (live client / replay).
    pub vis_hist:           std::collections::HashMap<(usize, usize), (u32, [(f32, f32); 3])>,
    /// Cached terrain + sky buffer. Built once per game; patched on explosions.
    pub world_cache:             crate::renderer::WorldBuffer,
    pub cache_initialized:       bool,
    pub cache_craters_processed: usize,
    /// Pre-rendered BG2 background, world-space (see `bg_image::build_bg_cache`).
    pub bg_cache:                crate::renderer::WorldBuffer,
    /// Client-only ambient background debris (wind-driven motes). Not networked.
    pub bg_debris:               Vec<crate::renderer::background::BgParticle>,
    /// Smoothed FPS, updated by main.rs once per second and drawn bottom-right.
    pub display_fps:             u32,
    /// Per-section (pixel-writes, elapsed µs) from the most recent frame's
    /// render (TEST mode profiling overlay — see `render_my_team`'s `mark!` calls).
    pub pixel_stats:              Vec<(&'static str, u64, u64)>,
    /// µs spent in the previous frame's blit_to_fb (row flip + present),
    /// recorded by main.rs and shown in the TEST overlay one frame late.
    pub blit_us:                  u64,
    /// Cached water surface strip (SCREEN_W × WATER_STRIP_H × 4 BGRA bytes).
    /// Regenerated every 3 ticks or when cam_x changes; blitted each frame.
    pub water_strip:      Vec<u8>,
    pub water_strip_tick: u32,  // last tick/3 bucket when strip was built
    pub water_strip_cam:  u32,  // cam_x used when strip was built
    /// Skeleton barrel tip from the previous render frame — used as the fire
    /// origin for hitscan weapons so shots start exactly where the reticle does.
    pub last_muzzle: Option<(f32, f32)>,
    /// Smoothed round-trip latency in ms (live mode only). 0 = no sample yet.
    pub ping_ms: u32,
    /// Timestamp of the most recent InputMsg send — used to compute ping.
    pub last_input_sent: Option<std::time::Instant>,
}

impl LoopState {
    pub fn new() -> Self {
        use crate::renderer::draw_sprites::{WATER_STRIP_H};
        use crate::world::SCREEN_W;
        Self {
            paused: false, frozen: false, pause_open_tick: 0, tick: 0, pause_cursor: 0,
            weapon_menu_open: false, weapon_menu_cursor: 0, fire_grace: 0,
            prev_turn_number: 0,
            vis_hist: std::collections::HashMap::new(),
            world_cache: crate::renderer::WorldBuffer::new(),
            cache_initialized: false,
            cache_craters_processed: 0,
            bg_cache: crate::renderer::WorldBuffer::new(),
            bg_debris: Vec::new(),
            display_fps: 0,
            pixel_stats: Vec::new(),
            blit_us: 0,
            water_strip: vec![0u8; (SCREEN_W * WATER_STRIP_H * 4) as usize],
            water_strip_tick: u32::MAX,
            water_strip_cam: u32::MAX,
            last_muzzle: None,
            ping_ms: 0,
            last_input_sent: None,
        }
    }
}

// ── Shared simulation core ────────────────────────────────────────────────────

/// Outcome of one `simulate()` call — tells the client wrapper what to draw.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SimStep {
    /// A normal gameplay tick ran to completion.
    Normal,
    /// The weapon menu is open; the gameplay phase was skipped this tick.
    MenuOpen,
    /// The crate pre-turn watch is active; player input is held off.
    CrateWatch,
}

/// Single source of truth for one logic tick — no camera, no rendering, no
/// pause / game-over UI, no weapon menu. Both `tick()` (local hotseat / vs-cpu
/// / test) and `server_tick()` (live network + TAT replay) call this, so every
/// mode plays identically. The weapon menu is client-only and runs in `tick()`
/// before this call. Camera follow and visual-only grave settling live in the
/// client wrapper (`tick()` / `update_camera()`).
pub fn simulate(game: &mut GameState, input: &InputState) -> SimStep {
    simulate_with_muzzle(game, input, None, None)
}

pub fn simulate_with_muzzle(game: &mut GameState, input: &InputState, muzzle_override: Option<(f32, f32)>, aim_angle_override: Option<f32>) -> SimStep {
    use super::turn::TurnPhase;

    for t in game.teams.iter_mut() { for so in t.soldiers.iter_mut() { so.wa_vis = None; } }
    game.sounds.clear(); // per-tick sound event buffer (shipped to live client)
    game.fx_events.clear(); // per-tick fx-spawn event buffer (shipped to live client)
    // Advance client-only effect particles (explosion fallout / dust / sparks /
    // splashes) once per tick, before any phase early-returns. Visual only.
    crate::renderer::fx::step_fx(&mut game.fx, &game.terrain, game.wind.value());
    crate::renderer::fx::step_fx_text(&mut game.fx_text);
    flush_damage_tallies(game);
    // Object mask: re-stamp barrels + armed mines so collision sees them as solid.
    stamp_objects(game);

    // Jumpbot: fixed real-time fuse keeps running regardless of whose turn/phase
    // it is (unlike Garcia/Airstrike, which only tick during their owner's own
    // Watching phase) — so it's ticked here, unconditionally, every tick.
    if game.jumpbot.is_some() {
        step_jumpbot(game, input);
    }

    tick_fire_grace(game); // weapon-confirm suppression — one source
    // Timer pauses while the player is charging a power shot (A held) or torching.
    // It still ticks while the weapon menu is open so pressure stays on.
    if game.aim.power <= 0.0 && game.plasma_torch.is_none() && game.jackhammer.is_none() {
        game.turn.tick();
    }

    // ── Crate pre-turn phase: block player input (timer already ran above) ────
    // Held while the countdown is running OR any crate is still falling — a
    // crate can outlive the fixed countdown (e.g. terrain destroyed under a
    // landed crate re-triggers its fall), and the camera must not cut away
    // to the active soldier while a crate is still in the air.
    if game.crate_watch_ticks > 0 || game.crates.iter().any(|c| !c.landed) {
        game.crate_watch_ticks = game.crate_watch_ticks.saturating_sub(1);
        apply_all_gravity(game, &crate::input::InputState::new());
        game.step_crates();
        game.collect_crates();
        game.step_explosions();
        for team in &mut game.teams {
            for s in &mut team.soldiers {
                s.step_hp_display();
            }
        }
        record_deaths(game);
        update_graves(game);
        return SimStep::CrateWatch;
    }

    match game.turn.phase {
        TurnPhase::Acting => {
            // Check BEFORE input: if the active soldier was hit at the end of last
            // tick (by barrel/mine/fire after phase dispatch), end the turn now.
            if game.active_worm_hit {
                let ti = game.active_team();
                let si = game.teams[ti].active;
                game.teams[ti].soldiers[si].has_fired = true;
                game.turn.on_fired();
            } else {
                process_acting_sim(game, input, muzzle_override, aim_angle_override);
                if game.active_worm_hit {
                    let ti = game.active_team();
                    let si = game.teams[ti].active;
                    game.teams[ti].soldiers[si].has_fired = true;
                    game.turn.on_fired();
                }
            }
        }
        TurnPhase::Watching => {
            let exp_before = game.explosions.len();
            let tnt_before = game.projectiles.iter().filter(|p| p.kind == crate::physics::projectile::WeaponKind::Tnt).count();
            let hhg_before = game.projectiles.iter().filter(|p| p.kind == crate::physics::projectile::WeaponKind::HolyHandGrenade).count();
            game.step_projectiles();
            {
                use crate::physics::projectile::WeaponKind;
                let spawns: Vec<_> = game.projectiles.iter()
                    .filter(|p| p.kind == WeaponKind::Bazooka || p.kind == WeaponKind::HomingMissile)
                    .map(|p| {
                        let speed = (p.vel.x * p.vel.x + p.vel.y * p.vel.y).sqrt();
                        if speed < 0.1 {
                            p.pos
                        } else {
                            let nx = p.vel.x / speed;
                            let ny = p.vel.y / speed;
                            // Tail is ~7px behind the rocket's center along its axis.
                            crate::world::WorldPos::new(p.pos.x - nx * 7.0, p.pos.y - ny * 7.0)
                        }
                    })
                    .collect();
                for pos in spawns { game.smoke_particles.push((pos, 22)); }
            }
            if game.explosions.len() > exp_before {
                let tnt_after = game.projectiles.iter().filter(|p| p.kind == crate::physics::projectile::WeaponKind::Tnt).count();
                let hhg_after = game.projectiles.iter().filter(|p| p.kind == crate::physics::projectile::WeaponKind::HolyHandGrenade).count();
                if tnt_after < tnt_before || hhg_after < hhg_before { game.emit_sound(crate::audio::Sfx::Tnt); } else { game.emit_sound(crate::audio::Sfx::Explosion); }
            }
            // Garcia falling: tick it during Watching (camera follow is client-side).
            if game.garcia.is_some() {
                step_garcia(game, &crate::input::InputState::new());
            }
            // Airstrike active: tick plane + bomb drops during Watching.
            if game.airstrike.is_some() {
                step_airstrike(game, &crate::input::InputState::new());
            }
            // TNT burn: player moves to escape; gravity + movement with live input.
            if game.tnt_placed {
                process_movement(game, input);
                apply_all_gravity(game, input);
            } else {
                // Apply gravity so knocked soldiers fall and land (not just float).
                apply_all_gravity(game, &crate::input::InputState::new());
            }
            // TNT fuse spent — stop granting movement.
            if game.projectiles.iter().all(|p| p.kind != crate::physics::projectile::WeaponKind::Tnt) {
                game.tnt_placed = false;
            }
            // Transition only when: projectiles gone + explosion done + all landed.
            let all_grounded = game.teams.iter().flat_map(|t| t.soldiers.iter())
                .all(|s| !matches!(s.state, SoldierState::Airborne { .. }));
            // Don't let the placing team's own turn advance into Retreating while
            // their own Jumpbot is still walking/counting down — hold Watching until
            // it detonates, then the normal retreat window opens right after.
            // Scoped to `owner_team == active_team` so it never blocks a later,
            // unrelated team's turn just because someone else's Jumpbot is still
            // alive elsewhere on the map.
            let jumpbot_blocking = game.jumpbot.as_ref().map_or(false, |r| r.owner_team == game.active_team());
            // Diagnostics: a Watching phase that drags on says WHY every 4 s (internal log only).
            if game.tick % 120 == 0 && !(game.projectiles.is_empty() && game.explosions.is_empty() && game.pending_deaths.is_empty() && game.black_holes.is_empty() && game.garcia.is_none() && game.airstrike.is_none() && !jumpbot_blocking && all_grounded) {
                let air: Vec<(usize, usize, i32, i32)> = game.teams.iter().enumerate().flat_map(|(ti, t)| t.soldiers.iter().enumerate()
                    .filter(|(_, s)| matches!(s.state, SoldierState::Airborne { .. })).map(move |(si, s)| (ti, si, s.pos.x as i32, s.pos.y as i32))).collect();
                crate::dlog!("watch", "tick={} holding: proj={} expl={} pend_deaths={} holes={} garcia={} airstrike={} jumpbot={} airborne={:?}",
                    game.tick, game.projectiles.len(), game.explosions.len(), game.pending_deaths.len(), game.black_holes.len(),
                    game.garcia.is_some(), game.airstrike.is_some(), jumpbot_blocking, air);
            }
            if game.projectiles.is_empty() && game.explosions.is_empty() && game.pending_deaths.is_empty() && game.black_holes.is_empty() && game.garcia.is_none() && game.airstrike.is_none() && !jumpbot_blocking && all_grounded {
                let hit = game.active_worm_hit;
                game.active_worm_hit = false;
                game.retreat_locked  = hit;
                game.turn.on_projectiles_resolved();
            }
        }
        TurnPhase::Retreating { .. } => {
            if !game.retreat_locked { process_movement(game, input); }
            apply_all_gravity(game, input);
            // Clear the damage-focus camera hold once the damage tally (popup +
            // HP countdown) finishes, or immediately if the player takes the
            // stick — whichever comes first (see update_camera).
            if game.damage_focus.is_some() {
                let moving = input.held(Button::Left) || input.held(Button::Right);
                if moving || !game.any_soldier_tallying() {
                    game.damage_focus = None;
                }
            }
        }
        TurnPhase::Ending => {
            use crate::game::soldier::SoldierState as SS;
            let ti0 = game.active_team();
            let si0 = game.teams[ti0].active;
            // A soldier still hanging on the rope when the turn times out is
            // Airborne, and the rope constraint keeps them swinging forever, so
            // the Airborne-wait below would never resolve — the turn softlocks
            // and no input is processed in Ending. Force-detach so plain gravity
            // drops them to a landing and the turn can advance.
            if game.rope.is_some() {
                game.rope = None;
            }
            let damage_tallying = game.any_soldier_tallying();
            if (game.teams[ti0].soldiers[si0].is_alive()
                && matches!(game.teams[ti0].soldiers[si0].state, SS::Airborne { .. }))
                || damage_tallying
            {
                apply_all_gravity(game, input);
                // soldier still in the air, or damage still tallying/counting down — hold off turn advance
            } else {
            game.active_worm_hit   = false;
            game.retreat_locked    = false;
            game.damage_focus      = None;
            game.weapon_menu_open  = false;
            game.shotgun_shots_left  = 0;
            game.revolver_shots_left = 0;
            game.minigun_shots_left  = 0;
            game.minigun_fire_timer  = 0;
            game.uzi_shots_left      = 0;
            game.uzi_fire_timer      = 0;
            game.pistol_shots_left   = 0;
            game.pistol_fire_timer   = 0;
            game.rope                = None;
            game.rope_session        = false;
            game.rope_retreat_ticks  = 0;
            game.tnt_placed          = false;
            game.plasma_torch        = None;
            game.jackhammer          = None;
            game.garcia              = None;
            game.airstrike           = None;
            game.homing_missile      = None;
            // Force-explode any mines still counting down so they don't bleed into next turn
            {
                use crate::game::state::MineState;
                use crate::physics::projectile::WeaponKind;
                let pending: Vec<crate::world::WorldPos> = game.mines.iter()
                    .filter(|m| matches!(m.state, MineState::Triggered { .. }))
                    .map(|m| m.pos).collect();
                game.mines.retain(|m| !matches!(m.state, MineState::Triggered { .. }));
                for pos in pending { game.apply_explosion(pos, WeaponKind::Landmine); }
            }
            // Reset per-turn crate damage so crates get full HP next turn
            for c in &mut game.crates { c.damage_this_turn = 0; }
            // Consume one grapple charge if rope was used this turn
            if game.rope_used_this_turn {
                game.rope_used_this_turn = false;
                let rti = game.active_team();
                if let Some((_, ammo)) = game.teams[rti].weapons.iter_mut()
                    .find(|(k, _)| *k == crate::physics::WeaponKind::NinjaRope)
                {
                    if let Some(n) = ammo { *n = n.saturating_sub(1); }
                }
                game.teams[rti].prune_empty_weapons();
            }
            // Turn-summary log (server diagnostics; a no-op without a logger).
            for (ti, team) in game.teams.iter().enumerate() {
                for s in &team.soldiers {
                    if s.is_dead() { log::info!("KILLED: team={} soldier={}", ti, s.index); }
                }
                log::info!("TEAM {} HP: {}", ti, team.soldiers.iter().map(|s| s.hp as u32).sum::<u32>());
            }
            let alive: Vec<bool> = game.teams.iter().map(|t| t.alive_count() > 0).collect();
            game.fire_turn_end(); // WA msg 0x18: Molotov flames lose a fifth of their size each turn end
            game.turn.advance(&alive);
            game.active_team_mut().advance_active();
            {
                // WA wind roll: whole units -10..=10, re-rolled when too close to last turn's.
                game.wind = crate::physics::Wind::next_turn_wa(
                    game.wind, game.turn.turn_number, game.map_seed as u32);
                crate::dlog!("turn", "tick={} turn={} team={} wind_u={} hp={:?}", game.tick, game.turn.turn_number, game.active_team(), game.wind.units(),
                    game.teams.iter().map(|t| t.soldiers.iter().map(|s| s.hp as u32).sum::<u32>()).collect::<Vec<_>>());
            }
            // Crate drop — chance starting after turn 3
            if game.turn.turn_number >= 3 {
                let drop_rng = (game.turn.turn_number.wrapping_mul(0xDEAD_BEEF)) as f32 / u32::MAX as f32;
                let pos_rng  = (game.turn.turn_number.wrapping_mul(0xCAFE_BABE).wrapping_add(game.map_seed as u32)) as f32 / u32::MAX as f32;
                let kind_rng = (game.turn.turn_number.wrapping_mul(0xBEEF_CAFE)) as f32 / u32::MAX as f32;
                if game.maybe_drop_crate(drop_rng, pos_rng, kind_rng) {
                    game.crate_watch_ticks = 90;
                    push_crate_message(game);
                    game.emit_sound(crate::audio::Sfx::CrateDrop);
                }
            }
            // Check win condition
            game.check_win();
            let ti = game.active_team();
            let si = game.teams[ti].active;
            game.teams[ti].soldiers[si].begin_turn();
            game.teams[ti].selected_weapon = 0;
            game.aim.angle = std::f32::consts::FRAC_PI_4;
            game.aim.power = 0.0;
            game.aim.charge_armed = false;
            push_turn_message(game);
            } // end airborne-guard else
        }
    }

    // ── End-of-tick cleanup ───────────────────────────────────────────────────
    let triggered_before = game.mines.iter().filter(|m| m.state == crate::game::state::MineState::Triggered).count();
    let exp_mines = game.explosions.len();
    game.step_mines();
    if game.explosions.len() > exp_mines { game.emit_sound(crate::audio::Sfx::Mine); }
    let triggered_after = game.mines.iter().filter(|m| m.state == crate::game::state::MineState::Triggered).count();
    if triggered_after > triggered_before { game.emit_sound(crate::audio::Sfx::MineArm); }
    let exp_barrels = game.explosions.len();
    game.step_barrels();
    if game.explosions.len() > exp_barrels { game.emit_sound(crate::audio::Sfx::Barrel); }
    game.step_fire_patches();
    game.flush_crate_damage();
    game.step_black_holes();
    game.step_death_explosions();
    game.step_explosions();
    game.blood_splats.retain_mut(|(_, t)| { if *t > 0 { *t -= 1; true } else { false } });
    game.smoke_particles.retain_mut(|(_, t)| { if *t > 0 { *t -= 1; true } else { false } });
    game.bullet_trails.retain_mut(|t| { if t.2 > 0 { t.2 -= 1; true } else { false } });
    game.step_crates();
    game.collect_crates();
    push_active_soldier_out(game);
    for team in &mut game.teams {
        for s in &mut team.soldiers {
            s.step_hp_display();
        }
    }
    game.messages.retain_mut(|m| { m.ticks = m.ticks.saturating_sub(1); m.ticks > 0 });
    record_deaths(game);
    update_graves(game); // settle headstones server-side so they ship settled to live clients
    // End the match the instant a team is wiped out — even mid-turn (e.g. a shotgun
    // or revolver with shots still left).
    game.check_win();
    SimStep::Normal
}

/// Client-only: re-derive the camera target from post-`simulate()` game state,
/// reproducing the follow/snap priority that used to live inside the phase match.
/// `prev_turn` is the turn number observed last frame, used to snap on turn change.
fn update_camera(game: &GameState, cam: &mut Camera, input: &InputState, step: SimStep, prev_turn: u32) {
    use super::turn::TurnPhase;

    // Turn-change snap (replaces cam.snap_to() at the end of the Ending phase).
    if game.turn.turn_number != prev_turn {
        let ti = game.active_team();
        let si = game.teams[ti].active;
        cam.snap_to(game.teams[ti].soldiers[si].pos);
        cam.tick();
        return;
    }

    match step {
        SimStep::MenuOpen => { /* menu open: camera holds, no follow (matches old menu branch) */ return; }
        SimStep::CrateWatch => {
            if let Some(cr) = game.crates.iter().find(|c| !c.landed) {
                cam.follow(cr.pos);
            }
        }
        SimStep::Normal => match game.turn.phase {
            TurnPhase::Acting => {
                if game.pistol_shots_left == 0 {
                    process_camera_pan(cam, input, game);
                }
                if !input.held(Button::R1) {
                    let ti = game.active_team();
                    let si = game.teams[ti].active;
                    if let Some(ref hm) = game.homing_missile {
                        if !hm.confirmed {
                            cam.follow(crate::world::WorldPos::new(hm.render_x, hm.render_y));
                        } else {
                            cam.follow(game.teams[ti].soldiers[si].pos);
                        }
                    } else if game.garcia.as_ref().map_or(false, |g| !g.falling) {
                        let g = game.garcia.as_ref().unwrap();
                        cam.follow(crate::world::WorldPos::new(g.render_x, g.render_y));
                    } else if let Some(ref air) = game.airstrike {
                        if !air.active {
                            cam.follow(crate::world::WorldPos::new(air.render_x, air.render_y));
                        } else {
                            cam.follow(game.teams[ti].soldiers[si].pos);
                        }
                    } else {
                        cam.follow(game.teams[ti].soldiers[si].pos);
                    }
                }
            }
            TurnPhase::Watching => {
                if let Some(ref r) = game.jumpbot {
                    cam.follow_always(crate::world::WorldPos::new(r.x, r.y));
                } else if let Some(ref g) = game.garcia {
                    let gpos = crate::world::WorldPos::new(g.cursor_x, g.fall_y.max(0.0));
                    cam.follow_always(gpos);
                } else if game.tnt_placed {
                    let ti = game.active_team();
                    let si = game.teams[ti].active;
                    cam.follow(game.teams[ti].soldiers[si].pos);
                } else if let Some(p) = game.projectiles.iter().find(|p| !p.is_fragment) {
                    // Skip sub-munition swarms (Meteor Bomb frags, Cluster Bomb
                    // bomblets, Blasthive bees) — following whichever one happens to
                    // be first in the vec makes the camera jerk around erratically
                    // as they scatter; fall through to the explosion/airborne
                    // continuity heuristics below instead.
                    cam.follow_always(p.pos);
                } else if !game.explosions.is_empty() {
                    // When multiple explosions are live at once, picking `.last()`
                    // flip-flops the follow target between widely separated x
                    // positions as the newest explosion finishes and is removed —
                    // visible as left-right camera shake. Pick the one closest to
                    // where the camera already is for continuity.
                    let cam_center = cam.left_edge_f32() + crate::world::SCREEN_W as f32 / 2.0;
                    let e = game.explosions.iter()
                        .min_by(|a, b| (a.pos.x - cam_center).abs()
                            .partial_cmp(&(b.pos.x - cam_center).abs()).unwrap())
                        .unwrap();
                    cam.follow_always(e.pos);
                } else {
                    // Same continuity heuristic for airborne soldiers — the first
                    // one in team/soldier order can flip-flop between soldiers on
                    // opposite sides of the map as they land/launch.
                    let cam_center = cam.left_edge_f32() + crate::world::SCREEN_W as f32 / 2.0;
                    let airborne = game.teams.iter().flat_map(|t| t.soldiers.iter())
                        .filter(|s| matches!(s.state, SoldierState::Airborne { .. }))
                        .map(|s| s.pos)
                        .min_by(|a, b| (a.x - cam_center).abs()
                            .partial_cmp(&(b.x - cam_center).abs()).unwrap());
                    if let Some(pos) = airborne { cam.follow(pos); }
                }
            }
            TurnPhase::Retreating { .. } => {
                let ti = game.active_team();
                let si = game.teams[ti].active;
                if game.retreat_locked {
                    cam.follow(game.teams[ti].soldiers[si].pos);
                } else {
                    process_camera_pan(cam, input, game);
                    let moving = input.held(Button::Left) || input.held(Button::Right);
                    if !cam.panning && moving {
                        cam.follow(game.teams[ti].soldiers[si].pos);
                    } else if let Some(pos) = game.damage_focus {
                        // Hold on the damaged soldier until the player pans/moves
                        // or the damage tally finishes — whichever comes first
                        // (damage_focus is cleared by either in the tick above).
                        if !cam.panning && !moving {
                            cam.follow(pos);
                        }
                    }
                }
            }
            TurnPhase::Ending => { /* snap handled on the turn-change check above */ }
        },
    }
    cam.tick();
}

// ── Public tick entry point ──────────────────────────────────────────────────

/// Called once per frame by main. Returns false to quit.
pub fn tick(
    game:    &mut GameState,
    input:   &InputState,
    buf:     &mut WorldBuffer,
    cam:     &mut Camera,
    lstate:  &mut LoopState,
    my_team: Option<usize>,
) -> bool {
    lstate.tick = lstate.tick.wrapping_add(1);
    game.tick   = lstate.tick;

    use super::state::GameResult;

    let settle = lstate.tick.wrapping_sub(lstate.pause_open_tick) > 4;
    // In TAT mode, block pause once the active soldier has fired
    let can_pause = match my_team {
        Some(mt) => !game.teams[mt].soldiers[game.teams[mt].active].has_fired,
        None     => true,
    };

    // ── Pause (client-only; the server never pauses) ──────────────────────────
    if lstate.paused {
        let resume = |lstate: &mut LoopState, game: &mut GameState| {
            lstate.paused          = false;
            lstate.pause_open_tick = lstate.tick;
            lstate.fire_grace      = 10; // block fire for 10 ticks after resume
            game.aim.power         = 0.0;
            game.aim.charge_armed  = false;
        };
        // TEST mode: RESUME / PAUSE GAME / EXIT MATCH (PAUSE GAME = hard freeze incl. the turn timer, for inspecting a stuck screen).
        let n_items = if game.is_test { 3 } else { 2 };
        if lstate.frozen {
            if (input.just_pressed(Button::Start) && settle) || input.just_pressed(Button::B) || input.just_pressed(Button::A) {
                lstate.frozen = false; // back to the pause menu
                lstate.pause_open_tick = lstate.tick;
            }
            render(game, buf, cam, lstate); // no sim, no game.turn.tick()
            let label = "GAME PAUSED - PRESS A";
            let lw = crate::renderer::font::str_width_scaled(label, 1);
            crate::renderer::font::draw_str_scaled(buf, label, cam.left_edge() as i32 + (crate::world::SCREEN_W as i32 - lw) / 2, cam.top_edge() as i32 + 60, Bgra::new(255, 210, 50), 1);
            return true;
        }
        if input.just_pressed(Button::Start) && settle { resume(lstate, game); }
        if input.just_pressed(Button::B)               { resume(lstate, game); }
        if input.just_pressed(Button::Up)   { lstate.pause_cursor = (lstate.pause_cursor + n_items - 1) % n_items; }
        if input.just_pressed(Button::Down) { lstate.pause_cursor = (lstate.pause_cursor + 1) % n_items; }
        if input.just_pressed(Button::A) {
            match (lstate.pause_cursor, game.is_test) {
                (0, _) => resume(lstate, game),
                (1, true) => { lstate.frozen = true; lstate.pause_open_tick = lstate.tick; }
                _ => return false,
            }
        }
        // Timer always ticks — pause menu doesn't freeze the clock
        game.turn.tick();
        render(game, buf, cam, lstate);
        if !lstate.frozen {
            let items: &[&str] = if game.is_test { &["RESUME", "PAUSE GAME", "EXIT MATCH"] } else { &["RESUME", "EXIT MATCH"] };
            crate::renderer::hud::draw_pause_menu_items(buf, items, lstate.pause_cursor as u8, cam.left_edge() as i32, cam.top_edge());
        }
        return true;
    }
    // Open the pause menu this frame. The weapon menu has input priority (mirrors
    // the old ordering, where the menu was processed before pause). We set the
    // flag and fall through so this frame still runs one sim tick, exactly as the
    // old code did (it didn't early-return when opening pause).
    if input.just_pressed(Button::Start) && settle && can_pause && !game.weapon_menu_open {
        lstate.paused          = true;
        lstate.pause_open_tick = lstate.tick;
        lstate.pause_cursor    = 0;
        game.aim.power         = 0.0; // stale charge can't fire on resume
    }

    // ── Game over (client-only screen) ────────────────────────────────────────
    if !matches!(game.result, GameResult::Ongoing) {
        render(game, buf, cam, lstate);
        let winner = if let GameResult::Winner(t) = game.result { Some(t) } else { None };
        let wa = winner.and_then(|w| game.teams.get(w)).map(|t| t.avatar_id).unwrap_or(0);
        let (stats, memo) = match_end_stats(game);
        let wc = winner.and_then(|w| game.teams.get(w)).map(|t| t.color_id).unwrap_or(0);
        draw_game_over(buf, winner, my_team, cam.left_edge() as i32, cam.top_edge(), wa, 0, 0, &stats, &memo, wc);
        if input.just_pressed(Button::A) || input.just_pressed(Button::Start) {
            return false;
        }
        return true;
    }

    // Post-pause fire suppression (client-only, not part of the shared core).
    if lstate.fire_grace > 0 { lstate.fire_grace -= 1; game.aim.power = 0.0; }

    // ── Weapon menu (client-only; never runs on server) ──────────────────────
    let menu_open = process_weapon_menu(game, input);

    // ── Shared gameplay simulation ────────────────────────────────────────────
    let prev_turn = lstate.prev_turn_number;
    if menu_open {
        // Menu is open: skip the sim tick, just render + overlay.
        render(game, buf, cam, lstate);
        let ti = game.active_team();
        draw_weapon_menu(buf, &game.teams[ti].weapons, game.weapon_menu_cursor, cam.left_edge() as i32, cam.top_edge() as i32, game.aim.fuse_ticks, game.turn.turn_number, game.teams.len());
    } else {
        // Keep spawn_cam_left current while the player is targeting the airstrike.
        if let Some(ref mut air) = game.airstrike {
            if !air.active { air.spawn_cam_left = cam.left_edge_f32(); }
        }
        let sim_t = std::time::Instant::now();
        let step = simulate_with_muzzle(game, input, lstate.last_muzzle, None);
        crate::dlog::note_sim(sim_t.elapsed().as_micros() as u64);
        lstate.prev_turn_number = game.turn.turn_number;
        update_camera(game, cam, input, step, prev_turn);
        render(game, buf, cam, lstate);
    }
    true
}

// ── Acting phase ─────────────────────────────────────────────────────────────

/// Acting-phase simulation only — no camera. Camera follow lives in
/// update_camera() (client). Shared by tick() and server_tick() via simulate().
fn process_acting_sim(game: &mut GameState, input: &InputState, muzzle_override: Option<(f32, f32)>, aim_angle_override: Option<f32>) {
    let has_fired = game.active_team_ref().active_soldier().has_fired;

    // Snap idle soldier to surface on every frame
    let ti = game.active_team();
    let si = game.teams[ti].active;
    if game.teams[ti].soldiers[si].state == SoldierState::Idle {
        snap_to_surface(game, ti, si);
    }

    let in_revolver  = game.revolver_shots_left > 0;
    let in_minigun   = game.minigun_shots_left > 0;
    let in_uzi       = game.uzi_shots_left > 0;
    let in_pistol    = game.pistol_shots_left > 0;
    let in_rope      = game.rope_session;
    let in_torch     = game.plasma_torch.is_some();
    let in_jackhammer     = game.jackhammer.is_some();
    let in_garcia         = game.garcia.is_some();
    let in_airstrike      = game.airstrike.is_some();
    let in_homing_missile = game.homing_missile.as_ref().map_or(false, |hm| !hm.confirmed);
    if !has_fired || in_revolver || in_minigun || in_uzi || in_pistol || in_rope || in_torch || in_jackhammer || in_garcia || in_airstrike || in_homing_missile {
        if in_torch {
            process_fire(game, input, muzzle_override); // direction changes only while torching
            step_plasma_torch(game);
        } else if in_jackhammer {
            step_jackhammer(game, input);
        } else if in_garcia {
            step_garcia(game, input);
        } else if in_airstrike {
            step_airstrike(game, input);
        } else if in_homing_missile {
            step_homing_missile(game, input);
        } else if !game.weapon_menu_open {
            process_movement(game, input);
            process_aim(game, input, aim_angle_override);
            process_fire(game, input, muzzle_override); // no tick-guard — matches server_tick() exactly
        }
    }

    apply_all_gravity(game, input);

    // Rope-retreat window: once the rope is put away (soldier off the rope) the
    // session stays alive for a short move window, then ends the acting phase.
    if game.rope_retreat_ticks > 0 && game.rope.is_none() {
        game.rope_retreat_ticks -= 1;
        if game.rope_retreat_ticks == 0 {
            game.rope_session = false;
        }
    }
}

fn process_camera_pan(cam: &mut Camera, input: &InputState, game: &GameState) {
    if input.just_released(Button::R1) {
        cam.release_pan();
        let ti = game.active_team();
        let si = game.teams[ti].active;
        cam.snap_to(game.teams[ti].soldiers[si].pos);
    }
    let speed = 20.0f32;
    if input.held(Button::R1) {
        if input.held(Button::Left)  { cam.pan(-speed); }
        if input.held(Button::Right) { cam.pan( speed); }
        if input.held(Button::Up)    { cam.pan_y(-speed); }
        if input.held(Button::Down)  { cam.pan_y( speed); }
    }
    // L1 + dpad: free pan that stays when L1 is released (no snap-back).
    // snap_to() on turn change clears the pan flag, resuming soldier follow.
    if input.held(Button::L1) {
        if input.held(Button::Left)  { cam.pan(-speed); }
        if input.held(Button::Right) { cam.pan( speed); }
        if input.held(Button::Up)    { cam.pan_y(-speed); }
        if input.held(Button::Down)  { cam.pan_y( speed); }
    }
}

// ── Movement ─────────────────────────────────────────────────────────────────

fn process_movement(game: &mut GameState, input: &InputState) {
    if input.held(Button::R1) || input.held(Button::L1) { return; } // pan mode
    // Rope attached: Left/Right apply swing force in apply_all_gravity; block walking.
    if game.rope.as_ref().map_or(false, |r| !r.flying) { return; }

    let ti = game.active_team();
    let si = game.teams[ti].active;

    // SELECT opens/closes weapon menu
    if input.just_pressed(Button::Select) {
        // lstate not available in process_movement — weapon menu handled in tick()
    }
    // One classification for the whole tick (see `support`): walking, jumping and backflipping all ask the same question the
    // fall logic asks, so a soldier the fall logic leaves standing can always act.
    let on_ground = support(game, ti, si).can_act();
    use crate::game::soldier::{WaAct, WA_RECOVER_FRAMES, WA_TURN_FRAMES, WA_WINDUP_FRAMES};

    // ── Jump windup / landing recovery: fixed-length WA states during which the worm ignores movement input ──
    if let Some(act) = game.teams[ti].soldiers[si].wa_act {
        let frames = |ticks: u32| 5 * ticks / 3; // 50 fps frames elapsed after `ticks` 30 Hz ticks
        let sol = &mut game.teams[ti].soldiers[si];
        sol.wa_walk = None;
        sol.walk_ticks = 0;
        match act {
            WaAct::Windup { backflip, tilt, age } => {
                if frames(age + 1) >= WA_WINDUP_FRAMES {
                    sol.wa_act = None;
                    start_wa_jump(game, ti, si, backflip, tilt);
                } else {
                    sol.wa_act = Some(WaAct::Windup { backflip, tilt, age: age + 1 });
                    if matches!(sol.state, SoldierState::Walking { .. }) { sol.state = SoldierState::Idle; }
                }
            }
            WaAct::Turn { age } => {
                sol.wa_act = if frames(age + 1) >= WA_TURN_FRAMES { None } else { Some(WaAct::Turn { age: age + 1 }) };
            }
            WaAct::Recover { age } => {
                sol.wa_act = if frames(age + 1) >= WA_RECOVER_FRAMES { None } else { Some(WaAct::Recover { age: age + 1 }) };
            }
        }
        return;
    }

    // ── Walking (WA state 0x66): fixed-point walk cycle at 50 fps, 5 frames per 3 ticks ──
    let want = input.held(Button::Right) as i32 - input.held(Button::Left) as i32;
    let mut walked = false;
    if want != 0 && !on_ground && game.tick % 30 == 0 {
        let sp = game.teams[ti].soldiers[si].pos;
        crate::dlog!("walk", "REFUSED not-grounded team={} s={} pos=({:.2},{:.2}) state={:?} act={:?}", ti, si, sp.x, sp.y, game.teams[ti].soldiers[si].state, game.teams[ti].soldiers[si].wa_act);
    }
    if want != 0 && on_ground {
        if game.teams[ti].soldiers[si].facing as i32 != want {
            // WA: the first press only turns the worm: a 0.5 px nudge in the new direction, then 14 frames of turn animation
            // before the walk starts (wa_rig walk_right vs walk_slope).
            let s = &mut game.teams[ti].soldiers[si];
            s.facing = want as i8;
            s.wa_walk = None;
            s.wa_act = Some(WaAct::Turn { age: 0 });
            let nx = s.pos.x + 0.5 * want as f32;
            if !game.terrain.is_blocked(nx as i32, game.teams[ti].soldiers[si].pos.y as i32 - 4) {
                game.teams[ti].soldiers[si].pos.x = nx;
            }
        } else {
            walked = true;
            wa_walk_tick(game, ti, si, want);
        }
    }
    if walked {
        game.teams[ti].soldiers[si].walk_ticks =
            game.teams[ti].soldiers[si].walk_ticks.wrapping_add(1);
        if matches!(game.teams[ti].soldiers[si].state, SoldierState::Idle | SoldierState::Walking { .. }) {
            game.teams[ti].soldiers[si].state =
                SoldierState::Walking { dir: game.teams[ti].soldiers[si].facing as f32 };
        }
        // Footstep dust kicked up behind the trailing foot every few steps.
        let s = &game.teams[ti].soldiers[si];
        if s.walk_ticks % 6 == 0 {
            let (fx, fy, facing) = (s.pos.x, s.pos.y + 3.0, s.facing as f32);
            game.emit_fx(crate::renderer::fx::FxEvent::Dust { x: fx, y: fy, count: 2, kick: 0.4, dir: facing });
        }
    } else {
        game.teams[ti].soldiers[si].walk_ticks = 0;
        game.teams[ti].soldiers[si].wa_walk = None;
        if matches!(game.teams[ti].soldiers[si].state, SoldierState::Walking { .. }) {
            game.teams[ti].soldiers[si].state = SoldierState::Idle;
        }
    }
    let is_idle = matches!(game.teams[ti].soldiers[si].state,
        SoldierState::Idle | SoldierState::Walking { .. });
    // WA: pressing jump starts a crouch (state 0x77) and the takeoff impulse follows (`WA_WINDUP_FRAMES`, animated as `SoldierAnim::Crouch`).
    if input.just_pressed(Button::B) && on_ground && is_idle {
        game.teams[ti].soldiers[si].wa_act = Some(WaAct::Windup { backflip: false, tilt: 0, age: 0 });
        game.teams[ti].soldiers[si].state = SoldierState::Idle;
    } else if input.just_pressed(Button::Y) && on_ground && is_idle {
        game.teams[ti].soldiers[si].wa_act = Some(WaAct::Windup { backflip: true, tilt: 2, age: 0 });
        game.teams[ti].soldiers[si].state = SoldierState::Idle;
    }
}

/// Takeoff after the windup: WA's exact impulses (forward jump / backflip), no extra lift.
fn start_wa_jump(game: &mut GameState, ti: usize, si: usize, backflip: bool, tilt: u8) {
    use crate::physics::wa_worm as ww;
    let f = game.teams[ti].soldiers[si].facing as i32;
    let (vx16, vy16) = if backflip {
        match tilt { 1 => (-f * ww::JUMP_BACKFLIP2_BACKSPACE.0, ww::JUMP_BACKFLIP2_BACKSPACE.1), 2 => (-f * ww::JUMP_BACKFLIP2_ENTER.0, ww::JUMP_BACKFLIP2_ENTER.1), _ => (ww::JUMP_BACKFLIP_VX, ww::JUMP_BACKFLIP_VY) }
    } else { (f * ww::JUMP_FWD_VX, ww::JUMP_FWD_VY) };
    // Jump arc scale vs WA. It was sqrt(0.6) (arc shrunk to 0.6x) when the soldiers were drawn ~23 px tall; the pixel-art soldiers are
    // now ~46 px tall, so that made backflips far too low to clear anything. 1.0 = WA's real takeoff velocities (backflip apex ~41 px).
    // Scaling BOTH velocities by k at WA's gravity scales the whole arc (range and apex) by k^2.
    let k = JUMP_SCALE;
    let (vx16, vy16) = ((vx16 as f64 * k).round() as i32, (vy16 as f64 * k).round() as i32);
    let v = crate::world::Vec2::new(ww::vel_from_16(vx16), ww::vel_from_16(vy16));
    let p = game.teams[ti].soldiers[si].pos;
    let s = &mut game.teams[ti].soldiers[si];
    s.wa = Some((ww::body_from_foot(p.x, p.y, vx16, vy16), v));
    s.state = SoldierState::Airborne { vel: v, spinning: backflip };
    // Airtime restarts so the spin plays its full revolution (see wa_air_tick).
    s.airtime = 0;
    s.fall.begin_fall(p.y);
}

/// Scale on WA's jump takeoff velocities. Scaling both components by k scales the whole arc (range and apex) by k^2. Was
/// sqrt(0.6) while soldiers were drawn ~23 px tall; 1.0 = WA's real arc now that they are ~46 px. See `tests/scale_contract.rs`.
pub const JUMP_SCALE: f64 = 1.0;

/// Walk speed. WA walks at 50 Hz frames; Arty ticks at 30 Hz, so a straight conversion is 5 frames per 3 ticks. That felt slow with
/// the ~2x taller pixel-art soldiers (the same px/s reads as half the speed), so the walk cycle runs 1.5x: 5 frames per 2 ticks.
/// Deliberate deviation from WA's walking pace; jumps and falls stay at WA's real speed. Integer ratio, so the 2-3-2-3 frame
/// pattern is exact and deterministic (a pure function of `walk_ticks`).
pub const WALK_FRAMES_PER_TICK_NUM: u32 = 5;
pub const WALK_FRAMES_PER_TICK_DEN: u32 = 2;

/// WA walk frames to run on the tick with this `walk_ticks` age (2 or 3; averages 2.5).
pub fn walk_frames_for_age(age_ticks: u32) -> u32 {
    (WALK_FRAMES_PER_TICK_NUM * (age_ticks + 1)) / WALK_FRAMES_PER_TICK_DEN - (WALK_FRAMES_PER_TICK_NUM * age_ticks) / WALK_FRAMES_PER_TICK_DEN
}

/// Free height a walking soldier needs above his feet, in px. The collision body is WA's 9x16, but the soldier is drawn ~46 px tall, so
/// with WA's rules alone he walks through gaps that are far smaller than he looks. Walking refuses to enter a spot with less headroom than
/// this (unless it is no worse than where he already stands, so he can never be trapped). Deliberate deviation; jumps/falls stay WA's.
pub const HEAD_CLEAR: i32 = 36;

/// Free pixel rows above a standing foot point at column `x`, up to `limit`. Centre column only: on a steep slope the uphill
/// side column is the hillside itself, which read as a zero-height ceiling and refused every step down a slope.
pub fn headroom(terrain: &crate::world::Terrain, x: i32, foot_y: i32, limit: i32) -> i32 {
    for d in 1..=limit {
        if terrain.is_blocked(x, foot_y - d) { return d - 1; }
    }
    limit
}

/// May a walking soldier step from `(ox, oy)` to `(nx, ny)` under the headroom rule?
pub fn walk_headroom_ok(terrain: &crate::world::Terrain, ox: i32, oy: i32, nx: i32, ny: i32) -> bool {
    let new = headroom(terrain, nx, ny, HEAD_CLEAR);
    // HEAD_CLEAR_SLACK: a floor 1-2 px higher than the one he stands on under a low ceiling (a bridge/scenery with exactly HEAD_CLEAR px
    // above the pit floor) left him 1 px short of 36 and refused forever -> stuck between a cactus and that ledge (WA-3 seed 18eab93d,
    // 'Smeg', head_new=35 head_old=36). A 3 px shortfall is not a visible squeeze, so it passes.
    new >= HEAD_CLEAR - HEAD_CLEAR_SLACK || new >= headroom(terrain, ox, oy, HEAD_CLEAR)
}
pub const HEAD_CLEAR_SLACK: i32 = 3;

/// One 30 Hz tick of WA walking at `WALK_FRAMES_PER_TICK` of the fixed-point walk cycle (`wa_worm::walk_frame`).
fn wa_walk_tick(game: &mut GameState, ti: usize, si: usize, dir: i32) {
    use crate::physics::wa_missile::WaMissile;
    use crate::physics::wa_worm as ww;
    let (px, py) = { let p = game.teams[ti].soldiers[si].pos; (p.x, p.y) };
    let mut body = ww::body_from_foot(px, py, 0, 0);
    // Keep the sub-pixel x the walk cycle left behind (pos.x is exact in f32 for multiples of 1/8 px).
    let mut ws = match game.teams[ti].soldiers[si].wa_walk {
        Some(w) if w.dir == dir => w,
        _ => ww::WalkState::start(body.x, dir),
    };
    let age = game.teams[ti].soldiers[si].walk_ticks;
    let terrain = &game.terrain;
    let solid = |x: i32, y: i32| terrain.is_blocked(x, y);
    let mut fell = false;
    let mut prev_foot = ww::foot_of(&body);
    for _ in 0..walk_frames_for_age(age) {
        prev_foot = ww::foot_of(&body);
        match body.walk_frame(&mut ws, &solid) {
            -1 => { fell = true; break; }
            // WA (FUN_005140e0, wa_rig walk_slope: bit-exact over 201 frames): a blocked step drops the worm to state 0x65 (standing);
            // with the key still held the walk restarts at the current x (fresh start-up animation), which makes it creep 0.5 px at a time.
            0 => ws = ww::WalkState::start(body.x, dir),
            _ => {}
        }
    }
    let vis_frac = (WALK_FRAMES_PER_TICK_NUM * (age + 1) % WALK_FRAMES_PER_TICK_DEN) as f32 / WALK_FRAMES_PER_TICK_DEN as f32;
    // Other soldiers block the step (WA: worms are solid to a walking worm).
    let (nx, ny) = ww::foot_of(&body);
    if !walk_headroom_ok(&game.terrain, px as i32, py as i32, nx as i32, ny as i32) {
        // Gap too low for his drawn size: stay put (restart the walk cycle from where he really stands next tick).
        if age % 15 == 0 {
            crate::dlog!("walk", "REFUSED headroom dir={} team={} s={} pos=({:.2},{:.2}) -> ({:.2},{:.2}) head_new={} head_old={} fell={}", dir, ti, si, px, py, nx, ny,
                headroom(&game.terrain, nx as i32, ny as i32, HEAD_CLEAR), headroom(&game.terrain, px as i32, py as i32, HEAD_CLEAR), fell);
        }
        game.teams[ti].soldiers[si].wa_walk = None;
        return;
    }
    let blocked_by_soldier = game.teams.iter().enumerate().any(|(oti, t)| t.soldiers.iter().enumerate().any(|(osi, o)| {
        !(oti == ti && osi == si) && o.is_alive() && !matches!(o.state, SoldierState::Airborne { .. })
            // Standing on top of that worm (their body is mostly below our feet)? It can't block a sideways step: walk off its head.
            && o.pos.y - py < 8.0
            && crate::physics::wa_collide::overlaps_body(3, nx as i32, ny as i32 - ww::FOOT_OFFSET, 3, o.pos.x as i32, o.pos.y as i32 - ww::FOOT_OFFSET)
            // Already touching (landed on / spawned next to it)? Then only refuse steps that move *closer*; otherwise the worm is stuck for good.
            && (!crate::physics::wa_collide::overlaps_body(3, px as i32, py as i32 - ww::FOOT_OFFSET, 3, o.pos.x as i32, o.pos.y as i32 - ww::FOOT_OFFSET)
                || (nx - o.pos.x).abs() < (px - o.pos.x).abs())
    }));
    let s = &mut game.teams[ti].soldiers[si];
    if blocked_by_soldier {
        if age % 15 == 0 { crate::dlog!("walk", "REFUSED blocked-by-soldier team={} s={} pos=({:.2},{:.2}) -> ({:.2},{:.2})", ti, si, px, py, nx, ny); }
        // Do NOT keep the advanced walk state: it ran ahead of the soldier's real position every refused tick, so after ~20 ticks it was
        // already past the blocker and the next step landed on the far side (soldiers "teleported" through each other).
        s.wa_walk = None;
        return;
    }
    if age % 15 == 0 && (nx - px).abs() < 0.01 && (ny - py).abs() < 0.01 && !fell {
        crate::dlog!("walk", "NO-PROGRESS dir={} team={} s={} pos=({:.2},{:.2}) (terrain/barrel blocks both step and step-up)", dir, ti, si, px, py);
    }
    s.pos.x = nx;
    s.pos.y = ny;
    s.wa_vis = Some((prev_foot.0, prev_foot.1, vis_frac));
    s.wa_walk = Some(ws);
    if fell {
        s.fall.begin_fall(ny);
        s.wa_walk = None;
        s.state = SoldierState::Airborne { vel: crate::world::Vec2::new(0.0, 0.0), spinning: false };
    }
}

pub fn try_move_horizontal(game: &mut GameState, ti: usize, si: usize, new_x: f32) {
    use crate::renderer::draw_sprites::{BODY_W, BODY_H, BODY_HALF_W};
    let cur_y = game.teams[ti].soldiers[si].pos.y;

    // Collect other alive soldiers' positions before any mutable borrow.
    let others: Vec<(f32, f32)> = game.teams.iter().enumerate()
        .flat_map(|(oti, team)| team.soldiers.iter().enumerate()
            .filter(move |&(osi, s)| !(oti == ti && osi == si) && s.hp > 0)
            .map(|(_, s)| (s.pos.x, s.pos.y)))
        .collect();

    let cur_x = game.teams[ti].soldiers[si].pos.x;
    let dir   = if new_x >= cur_x { 1 } else { -1 }; // travel direction

    let cfy0 = cur_y as i32;
    // Sweep the leading edge across every intermediate column between the current
    // and target position so a multi-px step can't jump over a thin (1-2px) wall
    // that the destination-only check would miss.
    let lead_off = if dir > 0 { BODY_HALF_W as i32 } else { -(BODY_HALF_W as i32) };
    let cur_lead = cur_x as i32 + lead_off;
    let target_lead = new_x as i32 + lead_off;
    let mut new_x = new_x;
    let mut lc = cur_lead;
    while lc != target_lead {
        let step = if target_lead > lc { 1 } else { -1 };
        let next_lc = lc + step;
        // A column is passable if there's SOME step-up (0-8px, matching the
        // destination check below) that clears the full body height — not just
        // at the current foot level. Without this, any uphill slope (the first
        // column rises even 1px) makes the very first sweep step "blocked" and
        // truncates new_x back to cur_x, freezing the soldier in place.
        let passable = (0..=8i32).any(|su|
            (0..=BODY_H).all(|h| !game.terrain.is_blocked(next_lc, cfy0 - su - h)));
        if !passable {
            new_x = (lc - lead_off) as f32;
            break;
        }
        lc = next_lc;
    }

    let ix = new_x as i32;
    // Edge columns of the soldier body (center ± half-width - 1).
    let ix_l = ix - BODY_HALF_W as i32;
    let ix_r = ix + BODY_HALF_W as i32;
    // Leading edge column (in the direction of travel) — the side that would run
    // INTO an obstacle. Barrels/mines stay solid (is_blocked), but a soldier already
    // wedged against one must still be able to step AWAY from it.
    let lead_col = if dir > 0 { ix_r } else { ix_l };

    // Is the soldier's CURRENT footprint already overlapping a barrel/mine/wall?
    // (If so we permit an escape step whose leading edge is clear.)
    let cxi   = cur_x as i32;
    let cfy   = cur_y as i32;
    let cx_l  = cxi - BODY_HALF_W as i32;
    let cx_r  = cxi + BODY_HALF_W as i32;
    let stuck_now = (0..=BODY_H).any(|h|
        game.terrain.is_blocked(cx_l, cfy - h)
        || game.terrain.is_blocked(cxi,  cfy - h)
        || game.terrain.is_blocked(cx_r, cfy - h));

    for step_up in 0i32..=8 {
        let try_y = cur_y - step_up as f32;
        if try_y < 0.0 { break; }
        let fy = try_y as i32;
        // Every column across the full soldier width must be clear foot-to-head.
        // Checking only 3 columns (edges + center) misses terrain at intermediate
        // pixels — especially ceiling notches that cause head-clipping in tunnels.
        let terrain_clear = (ix_l..=ix_r)
            .all(|xc| (0..=BODY_H).all(|h| !game.terrain.is_blocked(xc, fy - h)));
        // Escape allowance: if already wedged, permit the step as long as the leading
        // edge (the side moving forward) is clear — so you can back off a barrel you're
        // stuck on, but never push further into one.
        let lead_clear = (0..=BODY_H).all(|h| !game.terrain.is_blocked(lead_col, fy - h));
        if !terrain_clear && !(stuck_now && lead_clear) { continue; }
        // No other alive soldier may overlap this position.
        let soldier_clear = others.iter().all(|&(ox, oy)| {
            (new_x - ox).abs() >= BODY_W as f32 || (try_y - oy).abs() >= BODY_H as f32
        });
        if soldier_clear {
            game.teams[ti].soldiers[si].pos.x = new_x;
            game.teams[ti].soldiers[si].pos.y = try_y;
            snap_to_surface(game, ti, si);
            return;
        }
    }
}

pub fn snap_to_surface(game: &mut GameState, ti: usize, si: usize) {
    use crate::renderer::draw_sprites::BODY_HALF_W;
    let x = game.teams[ti].soldiers[si].pos.x as i32;
    let y = game.teams[ti].soldiers[si].pos.y as i32;
    // Scan every column across the full body width — not just 3 fixed edge/
    // center probes — so a curved surface (e.g. a crater lip) whose true peak
    // falls between those 3 points is still detected, instead of snapping to
    // a height that misses the real local high point and reads as a "jerky"
    // extra step on the next tick's is_on_ground/try_move_horizontal check.
    let x_l = x - BODY_HALF_W as i32;
    let x_r = x + BODY_HALF_W as i32;
    let any_solid = |yy: i32| (x_l..=x_r).any(|xc| game.terrain.is_blocked(xc, yy));
    // If foot is inside terrain, escape upward by at most 2px (float rounding artifact).
    // More than 2px of escape means we'd push through a wall — don't do it.
    let start = if any_solid(y) {
        if !any_solid(y - 1)      { y - 1 }
        else if !any_solid(y - 2) { y - 2 }
        else { return; } // deeply embedded — leave for gravity to resolve
    } else { y };
    // Scan downward to land on surface (0 = already correct, up to 10px gap for slopes)
    for gap in 0i32..=10 {
        let fy = start + gap;
        if fy >= crate::world::WORLD_H as i32 { break; }
        if any_solid(fy) {
            game.teams[ti].soldiers[si].pos.y = (fy - 1).max(0) as f32;
            return;
        }
    }
}

/// How (if at all) a soldier is currently held up. This is the single answer to "is he standing?": movement gating and the
/// Idle/Airborne handoff both use it, so they cannot disagree (they used to - a strict check for walking, a lenient one for
/// falling - which left soldiers standing but unable to walk or jump). Variants are ordered strongest first; classification
/// stops at the first that applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// A column under the feet with a clear foot row and terrain within 3 px below (`is_on_ground`).
    Firm,
    /// Feet on another living soldier's head (`is_on_soldier`).
    OnSoldier,
    /// Terrain within 5 px below one of three sample columns, but the strict check fails: foot pixel itself solid on a thin
    /// ridge, barrel edge or slope (`is_on_ground_lenient`).
    Ridge,
    /// Body overlaps terrain and nothing escapes it: left alone rather than relaunched (`is_embedded_in_terrain`).
    Embedded,
    /// Within the airborne landing tolerance of another soldier (`is_resting_near_soldier`).
    NearSoldier,
    /// Nothing holds him up: he falls.
    None,
}

impl Support {
    /// May walk, jump or backflip.
    pub fn can_act(self) -> bool { matches!(self, Support::Firm | Support::OnSoldier | Support::Ridge) }
    /// Counts as landed for an airborne soldier.
    pub fn is_firm(self) -> bool { matches!(self, Support::Firm | Support::OnSoldier) }
    /// An Idle soldier stays Idle (does not start falling).
    pub fn holds(self) -> bool { self != Support::None }
}

pub fn support(game: &GameState, ti: usize, si: usize) -> Support {
    if is_on_ground(game, ti, si) { Support::Firm }
    else if is_on_soldier(game, ti, si) || is_resting_on_worm(game, ti, si) { Support::OnSoldier }
    else if is_on_ground_lenient(game, ti, si) { Support::Ridge }
    else if is_embedded_in_terrain(game, ti, si) { Support::Embedded }
    else if is_resting_near_soldier(game, ti, si) { Support::NearSoldier }
    else { Support::None }
}

/// Nudge soldier `(ti, si)` 1 px horizontally away from the nearest living soldier that satisfies `is_resting_near_soldier`'s loose
/// overlap test, if the destination column is free of terrain. Returns whether it moved.
fn separate_from_neighbour(game: &mut GameState, ti: usize, si: usize) -> bool {
    use crate::renderer::draw_sprites::{BODY_W, BODY_H, BODY_HALF_W};
    let (x, y) = { let p = game.teams[ti].soldiers[si].pos; (p.x, p.y) };
    let mut best: Option<f32> = None; // signed dx to the nearest overlapping neighbour
    for (oti, t) in game.teams.iter().enumerate() {
        for (osi, o) in t.soldiers.iter().enumerate() {
            if (oti == ti && osi == si) || !o.is_alive() { continue; }
            let dx = x - o.pos.x;
            if (x as i32 - o.pos.x as i32).abs() < BODY_W && (y as i32) >= o.pos.y as i32 - BODY_H && (y as i32) <= o.pos.y as i32 + BODY_H
                && best.map_or(true, |b| dx.abs() < b.abs()) { best = Some(dx); }
        }
    }
    let Some(dx) = best else { return false };
    let nx = x + if dx >= 0.0 { 1.0 } else { -1.0 };
    let (ix, iy) = (nx as i32, y as i32);
    let clear = (0..=BODY_H).all(|h| !game.terrain.is_blocked(ix - BODY_HALF_W, iy - h) && !game.terrain.is_blocked(ix, iy - h) && !game.terrain.is_blocked(ix + BODY_HALF_W, iy - h));
    if clear { game.teams[ti].soldiers[si].pos.x = nx; }
    clear
}

/// Resting on another worm by the real WA mask shapes (the airborne solver's own notion of landing on a worm), as opposed to
/// `is_on_soldier`'s rectangle. A worm that lands on a shoulder settles lower than "feet 3 px from the head", so the rectangle
/// alone left him standing, not falling and unable to act.
fn is_resting_on_worm(game: &GameState, ti: usize, si: usize) -> bool {
    use crate::physics::wa_worm as ww;
    let s = &game.teams[ti].soldiers[si];
    if s.on_fire_ticks > 0 { return false; }
    let others: Vec<(i32, i32)> = game.teams.iter().enumerate().flat_map(|(oti, t)| t.soldiers.iter().enumerate().filter_map(move |(osi, o)| {
        if (oti == ti && osi == si) || !o.is_alive() || o.on_fire_ticks > 0 { None } else { Some((o.pos.x as i32, o.pos.y as i32 - ww::FOOT_OFFSET)) }
    })).collect();
    ww::body_from_foot(s.pos.x, s.pos.y, 0, 0).resting_on_worm(&others)
}

pub fn is_on_ground(game: &GameState, ti: usize, si: usize) -> bool {
    use crate::renderer::draw_sprites::BODY_HALF_W;
    let s = &game.teams[ti].soldiers[si];
    let x = s.pos.x as i32;
    let y = s.pos.y as i32;
    // Probe every column across the full body footprint — not just 3 fixed
    // edge/center points — so a curved surface (crater lip) whose true peak
    // falls between those points is still detected as "on ground" instead of
    // disagreeing with what try_move_horizontal/snap_to_surface would find,
    // which otherwise reads as a jerky extra step. Body is only ~14-20px wide
    // so a full-width scan is cheap.
    // Exclude columns where the foot-level pixel is already solid — those are
    // vertical walls beside the soldier, not ground beneath it. Without this
    // guard, pressing against a wall makes is_on_ground return true and
    // spamming jump ratchets the soldier upward.
    (x - BODY_HALF_W as i32..=x + BODY_HALF_W as i32).any(|xc| {
        if game.terrain.is_solid(xc, y) { return false; }
        game.terrain.is_blocked(xc, y + 1)
            || game.terrain.is_blocked(xc, y + 2)
            || game.terrain.is_blocked(xc, y + 3)
    })
}

/// Is this soldier standing on top of another living soldier's head (within the
/// same foot-level tolerance as `is_on_ground`)? Terrain-only `is_on_ground`
/// can't see other soldiers, so anything that gates walking/jumping/falling on
/// "am I grounded" must OR this in too, or a soldier standing on a teammate's
/// head falls straight through them the next tick.
pub fn is_on_soldier(game: &GameState, ti: usize, si: usize) -> bool {
    use crate::renderer::draw_sprites::{BODY_W, BODY_H};
    let s = &game.teams[ti].soldiers[si];
    let x = s.pos.x as i32;
    let y = s.pos.y as i32;
    game.teams.iter().enumerate().any(|(oti, oteam)| {
        oteam.soldiers.iter().enumerate().any(|(osi, os)| {
            if (oti == ti && osi == si) || !os.is_alive() { return false; }
            let ox = os.pos.x as i32;
            let head = os.pos.y as i32 - BODY_H as i32;
            (x - ox).abs() < BODY_W as i32 && (y - head).abs() <= 3
        })
    })
}

/// Lenient ground check for backflip: probes deeper (up to 5px below foot) and
/// doesn't exclude columns where the foot pixel is solid (handles slopes where the
/// soldier is slightly embedded). Also checks the center column only so edge-standing
/// soldiers (where edge columns are excluded by the wall guard) still qualify.
pub fn is_on_ground_lenient(game: &GameState, ti: usize, si: usize) -> bool {
    use crate::renderer::draw_sprites::BODY_HALF_W;
    let s = &game.teams[ti].soldiers[si];
    let x = s.pos.x as i32;
    let y = s.pos.y as i32;
    [x - BODY_HALF_W as i32, x, x + BODY_HALF_W as i32].iter().any(|&xc| {
        (1..=5).any(|dy| game.terrain.is_blocked(xc, y + dy))
    })
}

/// Upward "unstick" lift (0–2 px) for a jump/backflip, clamped to the clear space
/// above the soldier's head. Without this clamp the fixed `pos.y -= 2.0` teleports
/// the soldier up through solid terrain, so spamming jump in a tight pocket ratchets
/// them up through the ground to the surface. Airborne physics handles real motion.
pub fn jump_unstick_lift(game: &GameState, ti: usize, si: usize) -> f32 {
    use crate::renderer::draw_sprites::{BODY_H, BODY_HALF_W};
    let x = game.teams[ti].soldiers[si].pos.x as i32;
    let head = game.teams[ti].soldiers[si].pos.y as i32 - BODY_H as i32;
    let x_l = x - BODY_HALF_W as i32;
    let x_r = x + BODY_HALF_W as i32;
    let mut lift = 0.0;
    for k in 1..=2 {
        if game.terrain.is_solid(x_l, head - k)
            || game.terrain.is_solid(x, head - k)
            || game.terrain.is_solid(x_r, head - k)
        {
            break;
        }
        lift = k as f32;
    }
    lift
}

// ── Gravity ───────────────────────────────────────────────────────────────────

/// Place foot exactly on the surface after a landing collision.
/// Each physics sub-step is ≤1px, so after stepping back, the foot is at most 1-2px
/// inside terrain due to float→int rounding. Correct by at most 2px upward only —
/// never push through walls — then scan down to find the exact surface.
fn land_on_surface(terrain: &crate::world::Terrain, cx: f32, cy: f32) -> i32 {
    use crate::renderer::draw_sprites::BODY_HALF_W;
    let ix = cx as i32;
    let ix_l = ix - BODY_HALF_W as i32;
    let ix_r = ix + BODY_HALF_W as i32;
    let fy = cy as i32;
    // Check all 3 body columns (left edge, center, right edge) — matching
    // try_move_horizontal's footprint — so landing can't embed an edge column.
    // Use is_blocked so barrels/mines count as landing surfaces.
    let any_blocked = |yy: i32| {
        terrain.is_blocked(ix_l, yy) || terrain.is_blocked(ix, yy) || terrain.is_blocked(ix_r, yy)
    };
    // Tiny upward correction for float-rounding (max 2px — cannot teleport through walls).
    let start = if any_blocked(fy) {
        if !any_blocked(fy - 1)      { fy - 1 }
        else if !any_blocked(fy - 2) { fy - 2 }
        else { fy }
    } else { fy };
    for gap in 0i32..=9 {
        if any_blocked(start + 1 + gap) { return (start + gap).max(0); }
    }
    start.max(0)
}

// ── Weapon menu — shared across tick(), server_tick(), and live client ────────

/// Process the weapon selection menu for the current active soldier.
/// Call each tick before other action processing.
///
/// Returns `true` if the menu was open and processed (callers should skip
/// movement/aim/fire for this tick).  Uses `game.weapon_menu_open/cursor` and
/// `game.server_fire_grace` — the same GameState fields used everywhere, so
/// all modes are guaranteed identical behaviour from one place.
pub fn process_weapon_menu(game: &mut GameState, input: &InputState) -> bool {
    use crate::game::turn::TurnPhase;
    let acting_unfired = matches!(game.turn.phase, TurnPhase::Acting)
        && !game.active_team_ref().active_soldier().has_fired
        && game.plasma_torch.is_none()  // menu disabled while torch is burning
        && game.jackhammer.is_none();   // ...or while the jackhammer is drilling

    if acting_unfired {
        let ti = game.active_team();
        let si = game.teams[ti].active;
        // `was_open` replaces the per-call `menu_just_opened` flag: SELECT cannot
        // close the menu on the same tick it opens it.
        let was_open = game.weapon_menu_open;

        if !game.weapon_menu_open && input.just_pressed(Button::Select)
            && game.shotgun_shots_left == 0
            && game.revolver_shots_left == 0
            && game.minigun_shots_left == 0
            && game.uzi_shots_left == 0
        {
            game.weapon_menu_cursor = game.teams[ti].selected_weapon;
            game.weapon_menu_open   = true;
            game.aim.power          = 0.0;
            game.server_fire_grace  = game.server_fire_grace.max(10);
        }

        if game.weapon_menu_open {
            let n    = game.teams[ti].weapons.len();
            const COLS: usize = 4;
            // Column-major layout: weapons fill each column top-to-bottom before moving right.
            // idx → (row = idx % rows, col = idx / rows)
            let rows = (n + COLS - 1) / COLS;
            let cur  = game.weapon_menu_cursor;
            let row  = cur % rows;
            let col  = cur / rows;
            // items in the last column may be fewer than rows
            let col_len = |c: usize| if c < COLS { n.saturating_sub(c * rows).min(rows) } else { 0 };
            if input.just_pressed(Button::Left) {
                game.weapon_menu_cursor = if col > 0 {
                    let prev_col = col - 1;
                    let r = row.min(col_len(prev_col).saturating_sub(1));
                    prev_col * rows + r
                } else {
                    // Wrap to last column
                    let last_col = (n - 1) / rows;
                    let r = row.min(col_len(last_col).saturating_sub(1));
                    last_col * rows + r
                };
            }
            if input.just_pressed(Button::Right) {
                let next_col = col + 1;
                game.weapon_menu_cursor = if next_col * rows < n {
                    let r = row.min(col_len(next_col).saturating_sub(1));
                    next_col * rows + r
                } else {
                    // Wrap to first column
                    row.min(col_len(0).saturating_sub(1))
                };
            }
            // Column 0 always fills every row (weapons lay out column-major top-to-
            // bottom then left-to-right, so earlier columns are never shorter than
            // later ones) — so it's always a safe fallback landing spot for any
            // `target_row` in 0..rows, even when the CURRENT column is shorter and
            // doesn't have an item there. Without this fallback, Up/Down used to
            // wrap as soon as they hit the current column's own (possibly short)
            // length, so the on-screen scroll — which follows the cursor's row —
            // could never advance past whatever that short column allowed, even
            // though longer columns still had unseen rows below/above.
            if input.just_pressed(Button::Up) {
                let target_row = if row == 0 { rows.saturating_sub(1) } else { row - 1 };
                let new_col = if target_row < col_len(col) { col } else { 0 };
                game.weapon_menu_cursor = new_col * rows + target_row;
            }
            if input.just_pressed(Button::Down) {
                let target_row = if row + 1 < rows { row + 1 } else { 0 };
                let new_col = if target_row < col_len(col) { col } else { 0 };
                game.weapon_menu_cursor = new_col * rows + target_row;
            }
            // L1/R1 adjusts grenade fuse even while menu is open
            {
                use crate::physics::projectile::WeaponKind;
                let ck = game.teams[ti].weapons[game.weapon_menu_cursor].0;
                if ck == WeaponKind::Grenade || ck == WeaponKind::BananaBomb {
                    if input.just_pressed(Button::L1) { game.aim.fuse_ticks = game.aim.fuse_ticks.saturating_sub(30).max(30); }
                    if input.just_pressed(Button::R1) { game.aim.fuse_ticks = (game.aim.fuse_ticks + 30).min(150); }
                }
            }
            if input.just_pressed(Button::A) {
                game.teams[ti].selected_weapon = game.weapon_menu_cursor;
                game.aim.power         = 0.0;
                game.weapon_menu_open  = false;
                game.server_fire_grace = 30;
                game.homing_missile    = None;
            }
            // Close on B or SELECT — but not the same tick SELECT opened it
            if was_open && (input.just_pressed(Button::B) || input.just_pressed(Button::Select)) {
                game.weapon_menu_open = false;
                game.homing_missile   = None;
            }
            return true;
        }
    } else {
        game.weapon_menu_open = false;
    }
    false
}

/// Shared fire-grace and pause-resume suppression step.
/// Call after process_weapon_menu every tick.
/// Flush settled damage tallies into one popup per soldier. `take_damage`
/// accumulates `pending_damage` and restarts `damage_settle` on every hit, so
/// a burst (5 pistol shots, shotgun pellets) shows a single total once the
/// hits stop landing. The popup also arms `hp_countdown_delay` so the HP box
/// holds its old value ~2 s before ticking down. Runs inside
/// `simulate_with_muzzle`, so all five paths share it; the popup replicates to
/// live clients via the fx_events channel.
fn flush_damage_tallies(game: &mut GameState) {
    // Chain reactions (shot knocks a soldier onto a mine, barrel cascades,
    // death explosions) must read as ONE total: hold every flush while the
    // world is still "hot" — anything in flight, fused, or falling can still
    // add damage. The settle window is deliberately NOT decremented while
    // holding, so after the world quiets there is still a full
    // DAMAGE_SETTLE_TICKS buffer for follow-on triggers (a soldier landing
    // next to an armed mine trips it a tick or two after touchdown).
    // Exception: between shotgun shots (`shotgun_shots_left > 0`) each trigger
    // pull pops its own number — per-shot feedback is the point of the weapon.
    let any_airborne = game.teams.iter().flat_map(|t| t.soldiers.iter())
        .any(|s| matches!(s.state, crate::game::soldier::SoldierState::Airborne { .. }));
    let any_fused = game.mines.iter()
            .any(|m| matches!(m.state, crate::game::state::MineState::Triggered))
        || game.barrels.iter()
            .any(|b| matches!(b.state, crate::game::state::BarrelState::Triggered { .. }));
    let world_hot = any_airborne || any_fused
        || !game.projectiles.is_empty()
        || !game.explosions.is_empty()
        || !game.pending_deaths.is_empty()
        || !game.black_holes.is_empty()
        || game.garcia.is_some()
        || game.airstrike.is_some();
    if world_hot && game.shotgun_shots_left == 0 { return; }

    let mut popups: Vec<(f32, f32, u32, u8)> = Vec::new();
    for team in game.teams.iter_mut() {
        // The popup is tinted with the team's picked colour (color_id), NOT the
        // team index — in 4-player casual, lobby colour picks make these differ,
        // and the number must match the victim's HP box.
        let col = team.color_id;
        for s in &mut team.soldiers {
            if s.pending_damage == 0 { continue; }
            // Actively burning: fire DoT is still adding damage, so keep
            // tallying until the burn stops — if it kills the soldier, the
            // popup shows the whole burn's total once (after death) and the
            // HP box then ticks down to 0.
            if s.on_fire_ticks > 0 { continue; }
            if s.damage_settle > 0 { s.damage_settle -= 1; continue; }
            popups.push((s.pos.x, s.pos.y, s.pending_damage, col));
            s.pending_damage = 0;
            s.hp_countdown_delay = crate::game::soldier::HP_COUNTDOWN_DELAY_TICKS;
        }
    }
    for (x, y, dmg, color_id) in popups {
        game.emit_fx(crate::renderer::fx::FxEvent::DamagePopup {
            x, y, amount: dmg.min(255) as u8, color_id,
        });
    }
}

pub fn tick_fire_grace(game: &mut GameState) {
    if game.server_fire_grace > 0 {
        game.server_fire_grace -= 1;
        game.aim.power = 0.0;
    }
}

// ── Aim & Fire ────────────────────────────────────────────────────────────────

/// Adjust aim angle and grenade fuse. Public so live client can call it.
///
/// `aim_angle_override`: when `Some(a)`, the server-supplied angle is applied
/// directly and Up/Down button processing for aim is skipped. Up/Down still
/// flow through to cursor-phase weapons (homing missile, airstrike, etc.)
/// because those are handled before this call, in `process_acting_sim`.
/// Pass `None` for hotseat and TAT, where buttons drive aim.
pub fn process_aim(game: &mut GameState, input: &InputState, aim_angle_override: Option<f32>) {
    if let Some(a) = aim_angle_override {
        // Server-supplied angle: apply directly, skip button-driven adjustment.
        // Up/Down are not consumed here so cursor-phase weapons see them.
        game.aim.angle = a.clamp(-std::f32::consts::PI, std::f32::consts::PI);
    } else {
        // While rope is attached, Up/Down controls rope length (handled in process_fire).
        if game.rope.as_ref().map_or(false, |r| !r.flying) { return; }
        // L1/R1 are reserved for camera pan — don't adjust aim angle while either is held.
        if !input.held(Button::L1) && !input.held(Button::R1) {
            if input.held(Button::Up)   { game.aim.angle += 0.04; }
            if input.held(Button::Down) { game.aim.angle -= 0.04; }
        }
    }

    use crate::physics::projectile::WeaponKind;
    let ti = game.active_team();
    let si = game.teams[ti].active;
    let ck = game.teams[ti].current_weapon();

    // Plasma torch: step aim through the 3 valid directions on Up/Down press.
    if ck == WeaponKind::PlasmaTorch && game.plasma_torch.is_none() {
        const TORCH_ANGLE: f32 = 0.611; // atan2(0.574, 0.819)
        // Clamp to nearest snapped position first (handles turn-start default angle).
        game.aim.angle = if game.aim.angle > TORCH_ANGLE * 0.5 { TORCH_ANGLE }
                         else if game.aim.angle < -TORCH_ANGLE * 0.5 { -TORCH_ANGLE }
                         else { 0.0 };
        // Step one level per press.
        if input.just_pressed(Button::Up) {
            game.aim.angle = (game.aim.angle + TORCH_ANGLE).min(TORCH_ANGLE);
        }
        if input.just_pressed(Button::Down) {
            game.aim.angle = (game.aim.angle - TORCH_ANGLE).max(-TORCH_ANGLE);
        }
    }

    // Grenade / Cluster Bomb fuse: L1 = shorter, R1 = longer (1-5 s in 1-s steps at 30 Hz)
    if ck == WeaponKind::Grenade || ck == WeaponKind::ClusterBomb || ck == WeaponKind::BananaBomb {
        if input.just_pressed(Button::L1) { game.aim.fuse_ticks = game.aim.fuse_ticks.saturating_sub(30).max(30); }
        if input.just_pressed(Button::R1) { game.aim.fuse_ticks = (game.aim.fuse_ticks + 30).min(150); }
    }
}

/// True if the segment a→b is free of solid terrain (endpoints excluded), sampled every 3px.
/// Terrain/body queries for the reference rope model.
struct RopeEnv<'a>(&'a crate::world::Terrain);
impl crate::physics::wa_rope::Env for RopeEnv<'_> {
    fn solid_px(&self, x: i32, y: i32) -> bool { self.0.is_solid(x, y) }
    fn blocked(&self, x16: i32, y16: i32) -> bool {
        let (x, y) = (x16 >> 16, y16 >> 16);
        (0..=crate::renderer::draw_sprites::BODY_H).any(|h| self.0.is_solid(x, y - h))
    }
}

/// Write a stepped chain back into the rope state (mirrors length / wrap pivots for sync + drawing).
fn rope_store_chain(game: &mut GameState, chain: crate::physics::wa_rope::Chain) {
    if let Some(ref mut rm) = game.rope {
        let (_, piv) = chain.pivots();
        rm.length = chain.len as f32 / 65536.0;
        rm.wrap = piv.iter().map(|&(x, y)| crate::world::WorldPos::new(x as f32 / 65536.0, y as f32 / 65536.0)).collect();
        rm.chain = Some(chain);
    }
}

fn rope_seg_clear(terrain: &crate::world::Terrain, ax: f32, ay: f32, bx: f32, by: f32) -> bool {
    let dx = bx - ax; let dy = by - ay;
    let dist = (dx * dx + dy * dy).sqrt().max(0.1);
    let steps = (dist / 3.0).ceil() as u32;
    for s in 1..steps {
        let t = s as f32 / steps as f32;
        if terrain.is_solid((ax + dx * t) as i32, (ay + dy * t) as i32) { return false; }
    }
    true
}

/// Marching from a toward b, returns the distance of the last clear sample before the
/// first solid pixel (the corner the rope bends over), or None if the segment is clear.
fn rope_find_corner(terrain: &crate::world::Terrain, ax: f32, ay: f32, bx: f32, by: f32) -> Option<f32> {
    let dx = bx - ax; let dy = by - ay;
    let dist = (dx * dx + dy * dy).sqrt().max(0.1);
    let ux = dx / dist; let uy = dy / dist;
    let steps = (dist / 3.0) as u32;
    let mut last_clear = 0.0f32;
    for step in 1..=steps {
        let d = step as f32 * 3.0;
        if terrain.is_solid((ax + ux * d) as i32, (ay + uy * d) as i32) { return Some(last_clear); }
        last_clear = d;
    }
    None
}

/// Lift a rope soldier straight up out of terrain to the nearest position where
/// its body column is clear, bounded. Uses the same column predicate as the swept
/// swing collision so "clear" is consistent. Without this an embedded soldier gets
/// pinned: the swept check starts ahead of the (embedded) position and rests it at
/// `last_clear` = the embedded point with zero velocity, re-triggered every tick.
fn rope_unstick(game: &mut GameState, ti: usize, si: usize) {
    use crate::renderer::draw_sprites::BODY_H;
    let x = game.teams[ti].soldiers[si].pos.x as i32;
    for _ in 0..(BODY_H + 8) {
        let y = game.teams[ti].soldiers[si].pos.y as i32;
        if !(0..=BODY_H).any(|h| game.terrain.is_solid(x, y - h)) { break; }
        game.teams[ti].soldiers[si].pos.y -= 1.0;
    }
}

/// Free a soldier embedded in solid terrain by nudging up or down (whichever
/// clears first) up to `BODY_H + 24` px. Returns true if a clear position
/// was found and applied (or the soldier wasn't embedded to begin with);
/// false if nowhere clear was found nearby. Called as a watchdog at the top
/// of the Airborne physics step — see call site for why this matters.
/// Is this soldier's body silhouette overlapping solid terrain at its current
/// position? Shared by `unstick_embedded_soldier` (which tries to escape this)
/// and the `Idle` gravity check (which must NOT try to re-launch a soldier
/// `unstick_embedded_soldier` already gave up on — see its doc comment).
fn is_embedded_in_terrain(game: &GameState, ti: usize, si: usize) -> bool {
    use crate::renderer::draw_sprites::{BODY_HALF_W, BODY_H};
    let x = game.teams[ti].soldiers[si].pos.x as i32;
    let y = game.teams[ti].soldiers[si].pos.y as i32;
    let x_l = x - BODY_HALF_W as i32;
    let x_r = x + BODY_HALF_W as i32;
    (0..=BODY_H).any(|h|
        game.terrain.is_blocked(x_l, y - h)
            || game.terrain.is_blocked(x, y - h)
            || game.terrain.is_blocked(x_r, y - h))
}

/// Is this soldier within the Airborne arm's own (loose, whole-body-height)
/// `soldier_hit` landing tolerance of another living soldier? That sweep-loop
/// check uses `|dy| < BODY_H` (any overlap of the two bodies' vertical
/// spans), far looser than `is_on_soldier`'s strict "feet within 3px of their
/// head" — so two soldiers pinned together (e.g. both knocked to the map's
/// y=0 ceiling) can satisfy the Airborne arm's landing check every tick while
/// never satisfying `is_on_soldier`, flipping Idle -> Airborne -> "landed" ->
/// Idle forever at zero net displacement. Shared with the `Idle` gravity check
/// so it agrees with what actually stops the Airborne arm from falling.
fn is_resting_near_soldier(game: &GameState, ti: usize, si: usize) -> bool {
    use crate::renderer::draw_sprites::{BODY_W, BODY_H};
    let x = game.teams[ti].soldiers[si].pos.x as i32;
    let y = game.teams[ti].soldiers[si].pos.y as i32;
    game.teams.iter().enumerate().any(|(oti, oteam)| {
        oteam.soldiers.iter().enumerate().any(|(osi, os)| {
            if (oti == ti && osi == si) || !os.is_alive() { return false; }
            let ox = os.pos.x as i32;
            let oy = os.pos.y as i32;
            (x - ox).abs() < BODY_W as i32
                && y >= oy - BODY_H as i32
                && y <= oy + BODY_H as i32
        })
    })
}

fn unstick_embedded_soldier(game: &mut GameState, ti: usize, si: usize) -> bool {
    use crate::renderer::draw_sprites::{BODY_HALF_W, BODY_H};
    let x  = game.teams[ti].soldiers[si].pos.x as i32;
    let y0 = game.teams[ti].soldiers[si].pos.y as i32;
    let clear = |game: &GameState, xx: i32, yy: i32| {
        let x_l = xx - BODY_HALF_W as i32;
        let x_r = xx + BODY_HALF_W as i32;
        !(0..=BODY_H).any(|h|
            game.terrain.is_blocked(x_l, yy - h)
                || game.terrain.is_blocked(xx, yy - h)
                || game.terrain.is_blocked(x_r, yy - h))
    };
    if clear(game, x, y0) { return true; } // not actually embedded
    let max_search = BODY_H as i32 + 24;
    // Vertical search first — cheapest, and matches the common cavern-ceiling case.
    for d in 1..=max_search {
        if clear(game, x, y0 - d) {
            game.teams[ti].soldiers[si].pos.y = (y0 - d) as f32;
            return true;
        }
        if clear(game, x, y0 + d) {
            game.teams[ti].soldiers[si].pos.y = (y0 + d) as f32;
            return true;
        }
    }
    // Horizontal search: a pocket that's thick top-to-bottom (deeper than
    // max_search) can still be open to the side, e.g. wedged against a wall
    // inside a thick overhang.
    for d in 1..=max_search {
        if clear(game, x - d, y0) {
            game.teams[ti].soldiers[si].pos.x = (x - d) as f32;
            return true;
        }
        if clear(game, x + d, y0) {
            game.teams[ti].soldiers[si].pos.x = (x + d) as f32;
            return true;
        }
    }
    // Last resort: teleport to the nearest column with a known-standable spot
    // rather than leaving the soldier visibly stuck in terrain forever — a
    // short teleport out of an unreachable pocket beats a permanently embedded
    // soldier that can never land and blocks the turn from ever advancing.
    for d in 0..=max_search * 4 {
        for xx in [x - d, x + d] {
            if let Some(fy) = game.terrain.standable_foot_levels(xx).into_iter().next() {
                game.teams[ti].soldiers[si].pos.x = xx as f32;
                game.teams[ti].soldiers[si].pos.y = fy as f32;
                return true;
            }
        }
    }
    false
}

fn fire_rope_hook(game: &mut GameState, ti: usize, si: usize) {
    use crate::world::{WorldPos, Vec2};
    let fm    = game.teams[ti].soldiers[si].facing as f32;
    // Firing-angle limit: can't fire below horizontal unless unrestricted (54° below).
    let limit = if ROPE_UNRESTRICT { ROPE_MAX_DOWN_ANGLE_UNRESTRICT } else { ROPE_MAX_DOWN_ANGLE };
    let angle = game.aim.angle.max(limit);
    let mx = game.teams[ti].soldiers[si].pos.x + fm * 10.0;
    let my = game.teams[ti].soldiers[si].pos.y - 8.0;
    let muzzle = WorldPos::new(mx, my);
    let hvx = angle.cos() * fm * ROPE_HOOK_SPEED;
    let hvy = -angle.sin() * ROPE_HOOK_SPEED;
    game.rope = Some(RopeState {
        anchor:   muzzle,
        length:   0.0,
        flying:   true,
        hook:     muzzle,
        hook_vel: Vec2::new(hvx, hvy),
        wrap:     Vec::new(),
        chain:    None,
    });
    // Soldier stays on the ground while hook is flying — becomes Airborne when hook attaches.
}

#[inline] fn nx_dbg(v: i32) -> f32 { v as f32 / 65536.0 }

fn release_rope(game: &mut GameState) {
    if game.rope.is_some() { crate::dlog!("rope", "tick={} released by player", game.tick); }
    game.rope = None; // rope_session stays true — player can re-rope mid-air
}

fn process_fire(game: &mut GameState, input: &InputState, muzzle_override: Option<(f32, f32)>) {
    use crate::physics::projectile::WeaponKind;
    let weapon = game.active_team_ref().current_weapon();

    // ── Plasma torch active: A starts it; press A again (not release) to stop ──
    if game.plasma_torch.is_some() {
        if input.just_pressed(Button::Up) {
            let d = game.plasma_torch.as_ref().unwrap().dir;
            game.plasma_torch.as_mut().unwrap().dir = d.step_up();
        }
        if input.just_pressed(Button::Down) {
            let d = game.plasma_torch.as_ref().unwrap().dir;
            game.plasma_torch.as_mut().unwrap().dir = d.step_down();
        }
        // Press A again → extinguish torch and end turn
        if input.just_pressed(Button::A) {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            game.plasma_torch = None;
            game.teams[ti].soldiers[si].has_fired = true;
            game.teams[ti].soldiers[si].state = crate::game::soldier::SoldierState::Idle;
            game.turn.on_fired();
        }
        return;
    }

    // ── Grappling hook session: checked before all other weapons ─────────────
    if game.rope_session {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            match game.rope.as_ref().map(|r| r.flying) {
                Some(false) => { release_rope(game); }           // attached → detach
                None        => { fire_rope_hook(game, ti, si); } // airborne → re-rope
                Some(true)  => {}                                 // hook flying → wait
            }
        }
        // Up/Down: adjust rope length while attached. Velocity scales by
        // old/new length at reel time, full vector, unclamped — the reference
        // does this inside the reel handler itself (FUN_00513f00 → FUN_00500d00),
        // so reel-in accelerates and paying out damps.
        let dir = input.held(Button::Down) as i32 - input.held(Button::Up) as i32;
        if dir != 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            let chain0 = game.rope.as_ref().filter(|r| !r.flying).and_then(|r| r.chain.clone());
            if let Some(mut c) = chain0 {
                let sp = game.teams[ti].soldiers[si].pos;
                let mut out: Option<((i32, i32), (i32, i32))> = None;
                if let SoldierState::Airborne { ref vel, .. } = game.teams[ti].soldiers[si].state {
                    let env = RopeEnv(&game.terrain);
                    let mut pos = ((sp.x * 65536.0) as i32, (sp.y * 65536.0) as i32);
                    let mut v = ((vel.x * 65536.0) as i32, (vel.y * 65536.0) as i32);
                    if c.set_length(c.len + dir * crate::physics::wa_rope::TICK_REEL, &mut pos, &mut v, &env) {
                        out = Some((pos, v));
                    }
                }
                if let Some((pos, v)) = out {
                    game.teams[ti].soldiers[si].pos.x = pos.0 as f32 / 65536.0;
                    game.teams[ti].soldiers[si].pos.y = pos.1 as f32 / 65536.0;
                    if let SoldierState::Airborne { ref mut vel, .. } = game.teams[ti].soldiers[si].state {
                        vel.x = v.0 as f32 / 65536.0;
                        vel.y = v.1 as f32 / 65536.0;
                    }
                    rope_store_chain(game, c);
                }
            }
        }
        return;
    }

    // Shotgun follow-up shots: checked before weapon so they route here after prune.
    if game.shotgun_shots_left > 0 {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            game.emit_sound(crate::audio::Sfx::Shotgun);
            fire_shotgun(game, muzzle_override);
        }
        return;
    }

    // Revolver multi-shot: must be checked FIRST so subsequent shots route here
    // even after prune_empty_weapons() has shifted selected_weapon away from Revolver.
    if game.revolver_shots_left > 0 {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_revolver_shot(game, ti, si, muzzle_override);
        }
        return;
    }

    // Minigun auto-burst: fires one bullet every 4 ticks (20 shots ≈ 3 seconds).
    if game.minigun_shots_left > 0 {
        if game.minigun_fire_timer == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_minigun_shot(game, ti, si, muzzle_override);
            game.minigun_fire_timer = 4;
        } else {
            game.minigun_fire_timer -= 1;
        }
        return;
    }

    // Uzi auto-burst: fires one bullet every 3 ticks (timer=2 → decrement 2→1→0 → fire = 3-tick interval;
    // 20 shots × 3 ticks = 57 ticks ≈ 1.9s at 30 Hz, matching the 1.892s wav).
    if game.uzi_shots_left > 0 {
        if game.uzi_fire_timer == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_uzi_shot(game, ti, si, muzzle_override);
            game.uzi_fire_timer = 2;
        } else {
            game.uzi_fire_timer -= 1;
        }
        return;
    }

    // Pistol auto-burst: fires one shot every 22 ticks (~0.73s at 30 Hz) until 5 shots done.
    if game.pistol_shots_left > 0 {
        if game.pistol_fire_timer == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_pistol_shot(game, ti, si, muzzle_override);
            game.pistol_fire_timer = 21;
        } else {
            game.pistol_fire_timer -= 1;
        }
        return;
    }

    // Landmine: instant placement on A press — starts arming, turn ends.
    if weapon == WeaponKind::Landmine {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_mine(game, ti, si);
        }
        return;
    }

    // TNT: instant placement on A press — no charge needed.
    if weapon == WeaponKind::Tnt {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_tnt(game, ti, si);
        }
        return;
    }

    // Jumpbot: instant placement on A press — no charge needed, walks off on its own.
    if weapon == WeaponKind::Jumpbot {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_jumpbot(game, ti, si);
        }
        return;
    }

    // Super Bot (WA Super Sheep): placed like the Jumpbot, A again flies it.
    if weapon == WeaponKind::SuperSheep {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_bot(game, ti, si, true);
        }
        return;
    }

    // Baseball bat: melee swing.
    if weapon == WeaponKind::BaseballBat {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            if !game.teams[ti].consume_weapon() { return; }
            game.teams[ti].prune_empty_weapons();
            fire_baseball_bat(game, ti, si);
        }
        return;
    }

    // Shotgun fires instantly on press — no charge, no release needed.
    // server_fire_grace guards against weapon-menu-confirm A triggering a shot.
    if weapon == WeaponKind::Shotgun {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            if !game.teams[ti].consume_weapon() { return; }
            game.teams[ti].prune_empty_weapons();
            game.shotgun_shots_left = 2;
            game.emit_sound(crate::audio::Sfx::Shotgun);
            fire_shotgun(game, muzzle_override);
        }
        return;
    }

    // Revolver first shot: initialise the 6-shot sequence.
    if weapon == WeaponKind::Revolver {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            if !game.teams[ti].consume_weapon() { return; }
            game.teams[ti].prune_empty_weapons();
            game.revolver_shots_left = 6;
            fire_revolver_shot(game, ti, si, muzzle_override);
        }
        return;
    }

    // Minigun: press A to start the 20-shot auto-burst.
    if weapon == WeaponKind::Minigun {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            if !game.teams[ti].consume_weapon() { return; }
            game.teams[ti].prune_empty_weapons();
            game.minigun_shots_left = 20;
            game.minigun_fire_timer = 0;
            game.emit_sound(crate::audio::Sfx::Minigun);
            fire_minigun_shot(game, ti, si, muzzle_override);
        }
        return;
    }

    // Uzi: press A to start the 20-shot auto-burst.
    if weapon == WeaponKind::Uzi {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            if !game.teams[ti].consume_weapon() { return; }
            game.teams[ti].prune_empty_weapons();
            game.uzi_shots_left = 20;
            game.uzi_fire_timer = 2;
            game.emit_sound(crate::audio::Sfx::Uzi);
            fire_uzi_shot(game, ti, si, muzzle_override);
        }
        return;
    }

    // Pistol: press A to start the 5-shot burst (one shot every ~0.75s, cannot be interrupted).
    // Fire the first shot immediately on A-press (same frame) so sound is not delayed.
    // Remaining 4 shots are handled by the pistol auto-burst block above.
    if weapon == WeaponKind::Pistol {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            game.pistol_shots_left = 5;
            fire_pistol_shot(game, ti, si, muzzle_override);
            game.pistol_fire_timer = 21;
        }
        return;
    }

    // Grappling hook: first press starts a session — charge consumed at turn end, not per-fire.
    // This lets the player re-rope as many times as they like in one turn for just 1 charge.
    if weapon == WeaponKind::NinjaRope {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            // Check there is at least one charge available (infinite = always ok)
            let has_rope = game.teams[ti].weapons.iter()
                .any(|(k, a)| *k == WeaponKind::NinjaRope && a.map_or(true, |n| n > 0));
            if !has_rope { return; }
            game.rope_used_this_turn = true;
            game.rope_session = true;
            // has_fired stays false — grapple is a free movement tool, not a weapon action
            fire_rope_hook(game, ti, si);
        }
        return;
    }

    // Plasma torch: activate on A press; movement driven by step_plasma_torch each tick.
    if weapon == WeaponKind::PlasmaTorch {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            if !game.teams[ti].consume_weapon() { return; }
            game.teams[ti].prune_empty_weapons();
            let init_dir = {
                const TA: f32 = 0.611;
                if game.aim.angle >= TA * 0.5 { crate::game::state::TorchDir::UpForward }
                else if game.aim.angle <= -TA * 0.5 { crate::game::state::TorchDir::DownForward }
                else { crate::game::state::TorchDir::Forward }
            };
            game.plasma_torch = Some(crate::game::state::PlasmaTorchState {
                dir:        init_dir,
                fuel_ticks: 120, // 4 s × 30 Hz
            });
            // Torch burn sound is driven by audio::update_torch() in render() from the
            // live torch state, so it plays only WHILE the torch is active (and stops
            // on early release) — no one-shot emit_sound here.
            // has_fired blocks re-opening the weapon menu / firing another weapon
            // while torching (the `in_torch` OR-clause in process_acting_sim keeps
            // this soldier active despite it), but do NOT call on_fired() here: that
            // flips TurnPhase::Acting -> Watching, and step_plasma_torch/process_fire's
            // torch-steering dispatch only runs from the Acting match arm in tick()
            // (loop_runner.rs ~239-257) — Watching is a different branch that never
            // calls it. Calling on_fired() at deploy time silently stopped the torch
            // from ever carving or steering: Watching's "all resolved" check is
            // trivially true (torch isn't a tracked projectile) so it fell through to
            // Retreating within a tick or two of deploy. on_fired() is called instead
            // in step_plasma_torch (fuel exhausted) and the A-press-to-stop handler
            // above, once the torch is actually done.
            game.teams[ti].soldiers[si].has_fired = true;
        }
        return;
    }

    // Jackhammer: activate on A press; drills straight down each tick via step_jackhammer.
    if weapon == WeaponKind::Jackhammer {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            if !game.teams[ti].consume_weapon() { return; }
            game.teams[ti].prune_empty_weapons();
            game.jackhammer = Some(crate::game::state::JackhammerState {
                fuel_ticks: 180, // 6 s × 30 Hz — doubled drill length
            });
            // Drilling sound is driven by audio::update_jackhammer() in render() from
            // the live jackhammer state, mirroring the torch — no one-shot emit here.
            // Do NOT call on_fired() here (same reasoning as the torch): step_jackhammer
            // runs only from the Acting match arm, so on_fired() at deploy time would
            // flip to Watching and stop the drill before it carves. on_fired() is called
            // in step_jackhammer once the fuel is exhausted.
            game.teams[ti].soldiers[si].has_fired = true;
        }
        return;
    }

    // Garcia: auto-start targeting the moment the weapon is selected; A confirms.
    if weapon == WeaponKind::Garcia {
        if game.garcia.is_none() {
            let ti = game.active_team();
            let sx = game.teams[ti].soldiers[game.teams[ti].active].pos.x;
            let sy = (game.teams[ti].soldiers[game.teams[ti].active].pos.y - 40.0).max(12.0);
            game.garcia = Some(crate::game::state::GarciaState {
                cursor_x:    sx,
                render_x:    sx,
                cursor_y:    sy,
                render_y:    sy,
                blink_timer: 0,
                falling:     false,
                fall_y:      -200.0,
                vel_y:       8.0,
                bounce_count: 0,
                vel_x: 0.0,
            });
        }
        return;
    }

    // Airstrike: cursor targeting, A to confirm, B to cancel. Locked until turn 7.
    if weapon == WeaponKind::AirStrike {
        if game.turn.turn_number < 7 * game.teams.len() as u32 { return; }
        if game.airstrike.is_none() {
            let ti = game.active_team();
            let sx = game.teams[ti].soldiers[game.teams[ti].active].pos.x;
            let sy = (game.teams[ti].soldiers[game.teams[ti].active].pos.y - 40.0).max(12.0);
            game.airstrike = Some(crate::game::state::AirstrikeState {
                cursor_x:     sx,
                render_x:     sx,
                cursor_y:     sy,
                render_y:     sy,
                blink_timer:  0,
                active:       false,
                plane_x:      0.0,
                plane_vx:     6.0,
                bombs_dropped: 0,
                direction_right: true,
                spawn_cam_left: 0.0, // updated each frame from tick() while targeting
            });
        }
        return;
    }

    // Homing Missile: cursor targeting, A to confirm and fire, B to cancel. Locked until turn 2.
    if weapon == WeaponKind::HomingMissile {
        if game.turn.turn_number < 2 * game.teams.len() as u32 { return; }
        if game.homing_missile.is_none() {
            let ti = game.active_team();
            let sx = game.teams[ti].soldiers[game.teams[ti].active].pos.x;
            let sy = (game.teams[ti].soldiers[game.teams[ti].active].pos.y - 60.0).max(12.0);
            game.homing_missile = Some(crate::game::state::HomingMissileState {
                cursor_x: sx,
                render_x: sx,
                cursor_y: sy,
                render_y: sy,
                blink_timer: 0,
                confirmed: false,
            });
        }
        if !game.homing_missile.as_ref().unwrap().confirmed {
            return; // cursor phase — step_homing_missile handles input
        }
        // confirmed: fall through to charge-shot logic below
    }

    // All other weapons: hold A to charge, release to fire (Worms-style one-way).
    // charge_armed prevents the menu-confirm A press from firing: A must be released
    // at least once before charging begins.
    const CHARGE_RATE: f32 = 0.0216;  // original 0.02 rate, 8% faster

    if !input.held(Button::A) {
        if !game.aim.charge_armed {
            game.aim.charge_armed = true;
        } else if game.aim.power > 0.0 {
            fire_weapon(game);
            game.aim.power = 0.0;
        }
    } else if game.aim.charge_armed {
        // Longer charge meter for every weapon: charge can build up to MAX_CHARGE.
        // power=1.0 still maps to the same velocity as before (feel unchanged for a
        // normal full charge); the extra band 1.0..MAX_CHARGE is bonus range.
        // The bazooka uses the reference game's exact meter: 1.5 s from empty to full
        // (rate and cap from physics::tick), auto-firing at exactly full charge.
        let (rate, cap) = if crate::physics::tick::uses_wa_charge(game.teams[game.active_team()].current_weapon()) {
            (crate::physics::tick::BAZOOKA_CHARGE_RATE, crate::physics::tick::BAZOOKA_MAX_CHARGE)
        } else {
            (CHARGE_RATE, MAX_CHARGE)
        };
        game.aim.power = (game.aim.power + rate).min(cap);
        // WA auto-fires once the bar is full rather than waiting for release —
        // every charge-meter weapon does this now (shotgun never reaches this
        // code path; it fires instantly on press, see WeaponKind::Shotgun above).
        if game.aim.power >= cap {
            fire_weapon(game);
            game.aim.power = 0.0;
        }
    }
}

/// Maximum charge (meter fully filled). power=1.0 keeps the original velocity; the
/// 1.0..MAX_CHARGE band is extra reach for the wider map. Shared by process_aim
/// (cap), fire_weapon (velocity), and draw_aim_arrow (bar fill scaling).
pub const MAX_CHARGE: f32 = 1.3;

fn carve_torch_circle(
    terrain:    &mut crate::world::Terrain,
    crater_log: &mut Vec<(f32, f32, f32)>,
    cx: f32, cy: f32, r: f32,
) {
    crate::world::crater::Crater::new(cx, cy, r).carve(terrain);
    crater_log.push((cx, cy, r));
}

/// Advance one tick of the active plasma torch session: carve terrain, move soldier, drain fuel.
fn step_plasma_torch(game: &mut GameState) {
    use crate::game::soldier::SoldierState;
    use crate::game::state::PlasmaTorchState;

    const TORCH_SPEED:    f32 = 2.0;  // px/tick forward
    const TORCH_TIP_DIST: f32 = 18.0; // px ahead where carving leads
    // r=17 → 34px bore. The walk check needs foot+BODY_H (29px) clear at
    // every column of the 14px-wide body, so a 30px bore (old r=15) left only
    // 1px of headroom — any carve scallop or rounding speck wedged soldiers
    // inside their own tunnel. 5px of margin makes torch tunnels always passable.
    const TORCH_RADIUS:   f32 = 17.0;
    const BODY_RADIUS:    f32 = 17.0; // same at body so tunnel entrance matches

    let (dir, fuel) = {
        let t = match game.plasma_torch.as_ref() { Some(t) => t, None => return };
        (t.dir, t.fuel_ticks)
    };

    let ti = game.active_team();
    let si = game.teams[ti].active;
    if !game.teams[ti].soldiers[si].is_alive() {
        game.plasma_torch = None;
        return;
    }

    let facing = game.teams[ti].soldiers[si].facing as f32;
    let (dx, dy) = dir.to_vec(facing);

    let sx = game.teams[ti].soldiers[si].pos.x;
    let sy = game.teams[ti].soldiers[si].pos.y;
    let body_cy = sy - (crate::renderer::draw_sprites::BODY_H as f32 * 0.5); // true vertical midpoint of soldier body

    let tip_x = sx + dx * TORCH_TIP_DIST;
    let tip_y = body_cy + dy * TORCH_TIP_DIST;

    // Only advance (and carve) if there's actually solid terrain to dig through.
    // Prevents the torch from propelling the soldier through open air.
    // Check BEYOND the carve zone (tip_dist + tip_radius) so the first-tick
    // carve doesn't cause has_solid=false on tick 2.
    let check_start = TORCH_TIP_DIST + TORCH_RADIUS + 2.0; // 37px from soldier
    // Scenery is baked into terrain.solid at generation time (WA-style — see
    // Terrain::generate_tactical), so a plain solid check already "sees" a
    // tree/rock ahead exactly like dirt; no separate scenery lookup needed.
    let has_solid = (0..=4).any(|i| {
        let d = check_start + i as f32 * 3.0;
        game.terrain.is_solid((sx + dx * d) as i32, (body_cy + dy * d) as i32)
    });
    if !has_solid {
        // Nothing to carve — don't move, but still burn fuel and keep state ticking.
        if fuel == 0 {
            game.plasma_torch = None;
            game.teams[ti].soldiers[si].has_fired = true;
            game.teams[ti].soldiers[si].state = SoldierState::Idle;
            game.turn.on_fired();
        } else {
            game.plasma_torch.as_mut().unwrap().fuel_ticks -= 1;
        }
        return;
    }

    // Three overlapping circles ensure the tunnel is fully passable at all heights.
    // Two circles (body + tip 18px apart, both r=14) leave a narrow waist midway
    // where head/foot clearance drops below the soldier size. Adding a mid circle
    // at 9px closes that gap.
    let mid_x = sx + dx * (TORCH_TIP_DIST * 0.5);
    let mid_y = body_cy + dy * (TORCH_TIP_DIST * 0.5);
    carve_torch_circle(&mut game.terrain, &mut game.crater_log, tip_x, tip_y, TORCH_RADIUS);
    carve_torch_circle(&mut game.terrain, &mut game.crater_log, mid_x, mid_y, TORCH_RADIUS);
    carve_torch_circle(&mut game.terrain, &mut game.crater_log, sx, body_cy, BODY_RADIUS);

    // Dirt chips spat back out of the bore.
    if game.tick % 3 == 0 {
        let d = crate::game::state::biome_dirt(game.terrain.is_cavern, game.terrain.template_id);
        game.emit_fx(crate::renderer::fx::FxEvent::Dig {
            x: tip_x, y: tip_y, dir: dx.signum(), col: [d.r, d.g, d.b],
        });
    }

    // Trigger any barrel the torch tip touches
    for barrel in &mut game.barrels {
        if let crate::game::state::BarrelState::Normal = barrel.state {
            let bdx = barrel.pos.x - tip_x;
            let bdy = barrel.pos.y - tip_y;
            if bdx * bdx + bdy * bdy < (TORCH_RADIUS + 8.0) * (TORCH_RADIUS + 8.0) {
                barrel.state = crate::game::state::BarrelState::Triggered { ticks: 6 };
            }
        }
    }

    // Continuous contact damage along the full beam (tip + body circles).
    // Check whether any enemy soldier's center is within DAMAGE_RADIUS of any point
    // along the beam from soldier body-center to tip.
    const DAMAGE_RADIUS: f32 = 14.0;
    const DMG_PER_TICK:  u32 = 1;
    for eti in 0..game.teams.len() {
        if eti == ti { continue; }
        for esi in 0..game.teams[eti].soldiers.len() {
            if !game.teams[eti].soldiers[esi].is_alive() { continue; }
            let ex = game.teams[eti].soldiers[esi].pos.x;
            let ey = game.teams[eti].soldiers[esi].pos.y - 10.0; // enemy mid-body
            // Point-to-segment distance: segment from (sx, body_cy) to (tip_x, tip_y)
            let seg_dx = tip_x - sx;
            let seg_dy = tip_y - body_cy;
            let seg_len2 = seg_dx * seg_dx + seg_dy * seg_dy;
            let dist2 = if seg_len2 < 0.001 {
                let d = (ex - sx) * (ex - sx) + (ey - body_cy) * (ey - body_cy);
                d
            } else {
                let t = ((ex - sx) * seg_dx + (ey - body_cy) * seg_dy) / seg_len2;
                let t = t.clamp(0.0, 1.0);
                let closest_x = sx + t * seg_dx;
                let closest_y = body_cy + t * seg_dy;
                (ex - closest_x) * (ex - closest_x) + (ey - closest_y) * (ey - closest_y)
            };
            if dist2 < DAMAGE_RADIUS * DAMAGE_RADIUS {
                game.teams[eti].soldiers[esi].kill_weapon =
                    Some(crate::physics::projectile::WeaponKind::PlasmaTorch);
                game.teams[eti].soldiers[esi].take_damage(DMG_PER_TICK);
                game.teams[eti].soldiers[esi].hp_display_ticks = 60;
            }
        }
    }

    // Move forward, clamped inside world bounds
    let new_x = (sx + dx * TORCH_SPEED)
        .max(5.0)
        .min(crate::world::WORLD_W as f32 - 6.0);
    let new_y = (sy + dy * TORCH_SPEED)
        .max(5.0)
        .min(crate::world::WATER_Y as f32 - 5.0);

    game.teams[ti].soldiers[si].pos.x = new_x;
    game.teams[ti].soldiers[si].pos.y = new_y;
    // Keep facing in the torch direction
    if dx > 0.0 { game.teams[ti].soldiers[si].facing =  1; }
    if dx < 0.0 { game.teams[ti].soldiers[si].facing = -1; }

    // Walking state prevents gravity from triggering Airborne during tunneling
    game.teams[ti].soldiers[si].walk_ticks =
        game.teams[ti].soldiers[si].walk_ticks.wrapping_add(1);
    game.teams[ti].soldiers[si].state =
        SoldierState::Walking { dir: game.teams[ti].soldiers[si].facing as f32 };

    if fuel == 0 {
        game.plasma_torch = None;
        game.teams[ti].soldiers[si].has_fired = true;
        game.teams[ti].soldiers[si].state = SoldierState::Idle;
        game.turn.on_fired();
    } else {
        game.plasma_torch.as_mut().unwrap().fuel_ticks -= 1;
    }
}

/// Jackhammer: drills straight down through terrain, sinking the soldier into a
/// vertical shaft. Straight-down only (no aiming) — matching the WA jackhammer.
/// Press A again to stop early; otherwise runs until fuel is exhausted.
fn step_jackhammer(game: &mut GameState, input: &InputState) {
    use crate::game::soldier::SoldierState;

    const DRILL_SPEED:  f32 = 1.6;  // px/tick downward
    const DRILL_RADIUS: f32 = 12.0; // bore radius; ~24px shaft
    const TIP_DIST:     f32 = 18.0; // px below foot where carving leads

    let ti = game.active_team();
    let si = game.teams[ti].active;
    if !game.teams[ti].soldiers[si].is_alive() {
        game.jackhammer = None;
        return;
    }

    let fuel = game.jackhammer.as_ref().map(|j| j.fuel_ticks).unwrap_or(0);

    // Press A again → stop drilling and end the turn.
    let stop = input.just_pressed(Button::A) && game.server_fire_grace == 0;

    let sx = game.teams[ti].soldiers[si].pos.x;
    let sy = game.teams[ti].soldiers[si].pos.y; // foot position
    let body_cy = sy - (crate::renderer::draw_sprites::BODY_H as f32 * 0.5);
    let tip_y = sy + TIP_DIST;

    // Only advance (and carve) if there's solid terrain below to dig through —
    // otherwise the drill would propel the soldier through open air / off a ledge.
    let check_start = TIP_DIST + DRILL_RADIUS + 2.0;
    let has_solid = (0..=4).any(|i| {
        let d = check_start + i as f32 * 3.0;
        game.terrain.is_solid(sx as i32, (sy + d) as i32)
    });

    if !stop && has_solid {
        // Overlapping circles keep the shaft fully passable from head to tip.
        carve_torch_circle(&mut game.terrain, &mut game.crater_log, sx, body_cy, DRILL_RADIUS);
        carve_torch_circle(&mut game.terrain, &mut game.crater_log, sx, sy, DRILL_RADIUS);
        carve_torch_circle(&mut game.terrain, &mut game.crater_log, sx, tip_y, DRILL_RADIUS);

        // Dirt chips spat back out of the bore.
        if game.tick % 3 == 0 {
            let d = crate::game::state::biome_dirt(game.terrain.is_cavern, game.terrain.template_id);
            game.emit_fx(crate::renderer::fx::FxEvent::Dig {
                x: sx, y: tip_y, dir: 0.0, col: [d.r, d.g, d.b],
            });
        }

        // Rattling drill loop: re-trigger the ~0.8s clip so it plays continuously
        // while drilling. Routed through emit_sound so it replicates to all modes.
        if fuel % 24 == 0 {
            game.emit_sound(crate::audio::Sfx::Jackhammer);
        }

        // Trigger any barrel the drill tip touches.
        for barrel in &mut game.barrels {
            if let crate::game::state::BarrelState::Normal = barrel.state {
                let bdx = barrel.pos.x - sx;
                let bdy = barrel.pos.y - tip_y;
                if bdx * bdx + bdy * bdy < (DRILL_RADIUS + 8.0) * (DRILL_RADIUS + 8.0) {
                    barrel.state = crate::game::state::BarrelState::Triggered { ticks: 6 };
                }
            }
        }

        // Continuous contact damage to enemies caught in the shaft.
        const DAMAGE_RADIUS: f32 = 14.0;
        const DMG_PER_TICK:  u32 = 1;
        for eti in 0..game.teams.len() {
            if eti == ti { continue; }
            for esi in 0..game.teams[eti].soldiers.len() {
                if !game.teams[eti].soldiers[esi].is_alive() { continue; }
                let ex = game.teams[eti].soldiers[esi].pos.x;
                let ey = game.teams[eti].soldiers[esi].pos.y - 10.0;
                // Point-to-segment distance: segment from (sx, body_cy) to (sx, tip_y).
                let seg_dy = tip_y - body_cy;
                let t = ((ey - body_cy) / seg_dy).clamp(0.0, 1.0);
                let closest_y = body_cy + t * seg_dy;
                let dist2 = (ex - sx) * (ex - sx) + (ey - closest_y) * (ey - closest_y);
                if dist2 < DAMAGE_RADIUS * DAMAGE_RADIUS {
                    game.teams[eti].soldiers[esi].kill_weapon =
                        Some(crate::physics::projectile::WeaponKind::Jackhammer);
                    game.teams[eti].soldiers[esi].take_damage(DMG_PER_TICK);
                    game.teams[eti].soldiers[esi].hp_display_ticks = 60;
                }
            }
        }

        // Sink the soldier down into the shaft, clamped above the water line.
        let new_y = (sy + DRILL_SPEED).min(crate::world::WATER_Y as f32 - 5.0);
        game.teams[ti].soldiers[si].pos.y = new_y;
        // Walking state prevents gravity from flipping to Airborne mid-drill.
        game.teams[ti].soldiers[si].walk_ticks =
            game.teams[ti].soldiers[si].walk_ticks.wrapping_add(1);
        game.teams[ti].soldiers[si].state =
            SoldierState::Walking { dir: game.teams[ti].soldiers[si].facing as f32 };
    }

    // End the session: player stopped, hit open air, or ran out of fuel.
    if stop || !has_solid || fuel == 0 {
        game.jackhammer = None;
        game.teams[ti].soldiers[si].has_fired = true;
        game.teams[ti].soldiers[si].state = SoldierState::Idle;
        game.turn.on_fired();
    } else {
        game.jackhammer.as_mut().unwrap().fuel_ticks -= 1;
    }
}

const JUMPBOT_W: i32 = 10;
const JUMPBOT_H: i32 = 14;
const JUMPBOT_HALF_W: i32 = 5;
const JUMPBOT_FUSE_TICKS: u32 = 600; // 20s @ 30Hz: WA sheep fuse rec+0x8c = 20000 ms, obj[0xdf] -= 20 per 50 Hz frame
/// Countdown number only appears above its head for the final 5 seconds.
const JUMPBOT_COUNTDOWN_TICKS: u32 = 150;
/// Ground gait: a hop cadence rather than a flat walk, matching the live-measured
/// WA Sheep (see memory reference-wa-movement-physics) — four phases per cycle:
/// creep, leap, settle, pause. Over a 12-tick cycle it nets ~0.86 px/tick average
/// (≈0.9 px/frame @30fps, the measured sheep pace) with a visible hopping rhythm.
const JUMPBOT_GAIT_CYCLE: u32 = 12;
/// Per-tick horizontal step for a given point in the gait cycle (px, unsigned).
/// Sums to ~10.3px over the 12-tick cycle → ~0.86 px/tick net.
fn jumpbot_gait_step(walk_ticks: u32) -> f32 {
    match walk_ticks % JUMPBOT_GAIT_CYCLE {
        0..=1 => 0.4, // creep (0.8px over 2 ticks)
        2..=4 => 2.5, // leap (7.5px over 3 ticks)
        5..=6 => 1.0, // settle (2.0px over 2 ticks)
        _     => 0.0, // pause between hops (5 ticks stationary)
    }
}
/// Upward speed for the obstacle-clearing jump (not a player action — purely
/// automatic terrain-detection, matching WA's Sheep).
const JUMPBOT_JUMP_SPEED: f32 = 6.5;
/// Horizontal speed while airborne from a jump — faster than the walk speed
/// so it visibly arcs over the obstacle instead of barely creeping forward.
/// Tuned so a ~0.4s hop (see JUMPBOT_JUMP_SPEED/gravity above) covers ~40-60px,
/// matching WA's Sheep jump distance.
const JUMPBOT_JUMP_VX: f32 = 3.8;

/// Place a Jumpbot at the active soldier's feet — instant placement like TNT,
/// no aim/charge. It then walks/climbs/falls autonomously (see `step_jumpbot`)
/// until its fuse expires, it touches water, or it's caught in another blast.
pub fn fire_jumpbot(game: &mut GameState, ti: usize, si: usize) { fire_bot(game, ti, si, false) }

pub fn fire_bot(game: &mut GameState, ti: usize, si: usize, super_bot: bool) {
    use crate::game::state::JumpbotState;
    if !game.teams[ti].consume_weapon() { return; }
    game.teams[ti].prune_empty_weapons();
    let facing = game.teams[ti].soldiers[si].facing as i32;
    let sx = game.teams[ti].soldiers[si].pos.x + facing as f32 * 6.0;
    let sy = game.teams[ti].soldiers[si].pos.y - 4.0;
    game.jumpbot = Some(JumpbotState {
        x: sx, y: sy, vel_y: 0.0, vel_x: 0.0, facing,
        fuse_ticks: JUMPBOT_FUSE_TICKS, grounded: false, walk_ticks: 0,
        owner_team: ti, just_jumped: false, wa: None,
        super_bot, flying: false, heading: 0.0, fly_phase: 0,
    });
    game.teams[ti].soldiers[si].has_fired = true;
    game.turn.on_fired();
}

/// Is the Jumpbot's footprint resting on solid terrain? Mirrors `is_on_ground`
/// but for the Jumpbot's smaller, standalone footprint (not a soldier slot).
fn jumpbot_is_on_ground(game: &GameState) -> bool {
    let r = match game.jumpbot.as_ref() { Some(r) => r, None => return false };
    let x = r.x as i32;
    let y = r.y as i32;
    [x - JUMPBOT_HALF_W, x, x + JUMPBOT_HALF_W].iter().any(|&xc| {
        if game.terrain.is_solid(xc, y) { return false; }
        game.terrain.is_blocked(xc, y + 1)
            || game.terrain.is_blocked(xc, y + 2)
            || game.terrain.is_blocked(xc, y + 3)
    })
}

/// Snap the Jumpbot onto the terrain surface after a gravity fall, matching
/// `snap_to_surface`'s soldier logic but on the Jumpbot's own footprint.
fn jumpbot_snap_to_surface(game: &mut GameState) {
    let (x, y) = match game.jumpbot.as_ref() { Some(r) => (r.x as i32, r.y as i32), None => return };
    let x_l = x - JUMPBOT_HALF_W;
    let x_r = x + JUMPBOT_HALF_W;
    let any_solid = |game: &GameState, yy: i32| {
        game.terrain.is_blocked(x_l, yy) || game.terrain.is_blocked(x, yy) || game.terrain.is_blocked(x_r, yy)
    };
    let start = if any_solid(game, y) {
        if !any_solid(game, y - 1)      { y - 1 }
        else if !any_solid(game, y - 2) { y - 2 }
        else { return; } // deeply embedded — leave for gravity to keep resolving
    } else { y };
    for gap in 0i32..=10 {
        let fy = start + gap;
        if fy >= crate::world::WORLD_H as i32 { break; }
        if any_solid(game, fy) {
            if let Some(r) = game.jumpbot.as_mut() {
                r.y = (fy - 1).max(0) as f32;
                r.grounded = true;
                r.vel_y = 0.0;
            }
            return;
        }
    }
}

/// Walk one step in the Jumpbot's current facing direction: climb small steps
/// (0-8px, matching `try_move_horizontal`). Returns true if it moved (small
/// step included), false if nothing clears — caller decides whether to jump
/// or reverse.
fn jumpbot_try_move_horizontal(game: &mut GameState) -> bool {
    let (cur_x, cur_y, facing, walk_ticks) = match game.jumpbot.as_ref() {
        Some(r) => (r.x, r.y, r.facing, r.walk_ticks),
        None => return true,
    };
    // Hop cadence (leap/settle/pause) instead of a flat step — matches the
    // measured WA Sheep gait. A pause tick (step 0.0) keeps the bot in place
    // this tick without triggering the obstacle-jump path.
    let new_x = cur_x + jumpbot_gait_step(walk_ticks) * facing as f32;
    let ix = new_x as i32;
    let ix_l = ix - JUMPBOT_HALF_W;
    let ix_r = ix + JUMPBOT_HALF_W;

    for step_up in 0i32..=8 {
        let try_y = cur_y - step_up as f32;
        if try_y < 0.0 { break; }
        let fy = try_y as i32;
        let terrain_clear = (ix_l..=ix_r)
            .all(|xc| (0..=JUMPBOT_H).all(|h| !game.terrain.is_blocked(xc, fy - h)));
        if terrain_clear {
            if let Some(r) = game.jumpbot.as_mut() {
                r.x = new_x;
                r.y = try_y;
            }
            jumpbot_snap_to_surface(game);
            return true;
        }
    }
    false
}

/// Advance the Jumpbot one tick while airborne from an obstacle-clearing jump:
/// ballistic motion, preserving `vel_x` (unlike a natural ledge-drop, which
/// falls straight down). Horizontal collision just stops `vel_x` on contact —
/// no mid-air climbing, matching WA's Sheep ("nothing special happens while
/// airborne").
fn jumpbot_jump_step(game: &mut GameState) {
    let (x, y, vel_x, vel_y) = match game.jumpbot.as_ref() {
        Some(r) => (r.x, r.y, r.vel_x, r.vel_y),
        None => return,
    };
    let new_vel_y = (vel_y + 1.2).min(20.0);
    let new_y = y + new_vel_y;
    let new_x = x + vel_x;
    let ix = new_x as i32;
    let ix_l = ix - JUMPBOT_HALF_W;
    let ix_r = ix + JUMPBOT_HALF_W;
    // Test clearance at the RISEN y (new_y), not the old ground-level y — the
    // whole point of jumping is to clear an obstacle that blocks at ground
    // level, so checking at the old y would always see it as still blocked.
    let fy = new_y as i32;
    let x_clear = (ix_l..=ix_r).all(|xc| (0..=JUMPBOT_H).all(|h| !game.terrain.is_blocked(xc, fy - h)));
    if let Some(r) = game.jumpbot.as_mut() {
        r.vel_y = new_vel_y;
        r.y = new_y;
        // Only apply the horizontal move if clear — but keep `vel_x` intact
        // either way (don't zero it), so a tick that's still blocked early in
        // the arc (before rising above the obstacle) just pauses horizontal
        // motion for that tick instead of permanently killing it; once risen
        // high enough to clear, it resumes moving sideways on a later tick.
        if x_clear {
            r.x = new_x;
        }
    }
    // Only look for a landing surface once descending (vel_y > 0) — checking
    // during ascent would immediately catch the ground it just launched from
    // (still within snap_to_surface's 10px landing-scan window) and cancel
    // the jump the same tick it started.
    if new_vel_y > 0.0 {
        jumpbot_snap_to_surface(game);
    }
}

/// Advance the autonomous Jumpbot walker one tick. Ticked unconditionally every
/// tick regardless of turn/phase (see call site in `simulate_with_muzzle`) since
/// its fuse is a fixed real-time countdown, matching WA's Sheep.
///
/// `input` is only consulted for the manual-detonate check below — the placing
/// team can press A to blow it early, but only while it's their own turn
/// (`game.active_team() == owner_team`). The server only ever forwards the
/// currently-active team's input into simulation (see `server_tick`'s
/// `active`/`inp` selection in src/server/main.rs), so an off-turn press from
/// the other team never reaches here — this check is a same-turn convenience,
/// not a true "any time" remote detonator.
pub fn step_jumpbot(game: &mut GameState, input: &InputState) {
    use crate::physics::WeaponKind;
    use crate::world::WorldPos;

    let kind = if game.jumpbot.as_ref().map_or(false, |r| r.super_bot) { WeaponKind::SuperSheep } else { WeaponKind::Jumpbot };
    let a_press = game.jumpbot.as_ref().map_or(false, |r| {
        r.owner_team == game.active_team() && input.just_pressed(Button::A)
    });
    let mut detonate = false;
    if a_press {
        let r = game.jumpbot.as_mut().unwrap();
        if !r.super_bot { detonate = true; }
        else {
            // WA Super Sheep: A starts flight (heading up), A again walks, a further A detonates.
            match r.fly_phase {
                0 => { r.fly_phase = 1; r.flying = true; r.heading = 0.0; r.wa = None; }
                1 => { r.fly_phase = 2; r.flying = false; r.wa = None; }
                _ => detonate = true,
            }
        }
    }
    let boom = |game: &mut GameState| {
        let (x, y) = { let r = game.jumpbot.as_ref().unwrap(); (r.x, r.y) };
        game.jumpbot = None;
        game.apply_explosion_force(WorldPos::new(x, y), kind, 1.0);
        game.emit_sound(crate::audio::Sfx::Jumpbot);
    };
    if detonate { boom(game); return; }

    let expired = {
        let r = match game.jumpbot.as_mut() { Some(r) => r, None => return };
        r.fuse_ticks = r.fuse_ticks.saturating_sub(1);
        r.fuse_ticks == 0
    };
    if expired { boom(game); return; }

    let flying = game.jumpbot.as_ref().map_or(false, |r| r.flying);
    let in_water = game.jumpbot.as_ref().map_or(false, |r| r.y >= crate::world::WATER_Y as f32);
    if in_water && !flying { boom(game); return; }

    if flying {
        // WA Super Sheep flight (decompile 0x50ac60): 0x5b0/65536 turn per 50 Hz frame while Left/Right is held,
        // 8 px/frame above the waterline and 4 below; explodes when its footprint touches land.
        use crate::physics::wa_missile::WaMissile;
        use crate::physics::wa_collide::overlaps_land;
        let steer = (input.held(Button::Right) as i32 - input.held(Button::Left) as i32) as f32
            * (0x5b0 as f32 / 65536.0 * 360.0);
        let (mut x, mut y, mut h, age) = { let r = game.jumpbot.as_ref().unwrap(); (r.x, r.y, r.heading, r.walk_ticks) };
        let world_w = crate::world::WORLD_W as i32;
        let terrain = &game.terrain;
        let solid = |px: i32, py: i32| py >= 0 && terrain.is_blocked(px, py); // WA: land outside the canvas is empty
        let mut hit = false;
        for _ in 0..WaMissile::frames_for_age(age) {
            h += steer;
            let speed = if y >= crate::world::WATER_Y as f32 { 4.0 } else { 8.0 };
            let rad = h.to_radians();
            x += speed * rad.sin();
            y -= speed * rad.cos();
            if overlaps_land(&solid, crate::physics::wa_sheep::SHAPE, x as i32, (y as i32) - crate::physics::wa_sheep::HALF_H) { hit = true; break; }
        }
        let r = game.jumpbot.as_mut().unwrap();
        r.x = x; r.y = y; r.heading = h.rem_euclid(360.0); r.grounded = false; r.walk_ticks = r.walk_ticks.wrapping_add(1);
        if h.to_radians().sin().abs() > 0.05 { r.facing = if h.to_radians().sin() > 0.0 { 1 } else { -1 }; }
        if hit || y < -400.0 || x < -(WA_EDGE_FALL_MARGIN as f32) || x >= (world_w + WA_EDGE_FALL_MARGIN) as f32 { boom(game); }
        return;
    }

    // WA Sheep driver (physics::wa_sheep): 50 Hz fixed-point walk / hop / flight, 1-2 frames per 30 Hz tick.
    {
        use crate::physics::wa_sheep::{SheepBody, HALF_H};
        use crate::physics::wa_missile::WaMissile;
        let (age, mut body) = {
            let r = game.jumpbot.as_ref().unwrap();
            let body = r.wa.unwrap_or_else(|| SheepBody::new(
                (r.x as f64 * 65536.0) as i32, ((r.y.floor() as i32 - HALF_H) << 16), r.facing, (game.tick as u32).wrapping_mul(2654435761)));
            (r.walk_ticks, body)
        };
        let world_w = crate::world::WORLD_W as i32;
        let terrain = &game.terrain;
        let solid = |x: i32, y: i32| y >= 0 && terrain.is_blocked(x, y); // WA: land outside the canvas is empty
        let mut sound = false;
        let mut landed = false;
        for _ in 0..WaMissile::frames_for_age(age) {
            let f = body.frame(&solid, crate::world::WATER_Y as i32);
            sound |= f.hopped;
            landed |= f.landed;
        }
        let airborne = body.airborne(&solid);
        let _ = (sound, landed);
        if let Some(r) = game.jumpbot.as_mut() {
            r.x = body.x as f32 / 65536.0;
            r.y = body.y as f32 / 65536.0 + HALF_H as f32;
            r.facing = body.dir;
            r.grounded = !airborne;
            r.vel_x = crate::physics::wa_worm::vel_from_16(body.vx);
            r.vel_y = crate::physics::wa_worm::vel_from_16(body.vy);
            r.just_jumped = airborne && body.vx != 0;
            r.wa = Some(body);
        }
    }

    if let Some(r) = game.jumpbot.as_mut() { r.walk_ticks = r.walk_ticks.wrapping_add(1); }
}

/// Flame icon painted on every oil drum (12 x 23 px, hand-drawn): O orange outline, Y yellow body, W pale core, D dark base.
const DRUM_FLAME: [&str; 23] = [
    ".........O..",
    ".........OO.",
    "........OOO.",
    "........OYOO",
    ".......OOYYO",
    ".......OYYYO",
    "..O....OYYYO",
    "..OO..OOYYYO",
    "..OOO.OYYYYO",
    "..OYOOOYYYYO",
    "..OYYOYYYYOO",
    ".OOYYYYYYYO.",
    ".OYYYYWYYYOO",
    ".OYYYYWWYYYO",
    "OOYYYWWWYYYO",
    "OYYYYWWWWYYO",
    "OYYYYWWWWYYO",
    "OYYYYYWWWYYO",
    "OOYYYYYWYYOO",
    ".OYYYYYYYYO.",
    ".OOYYYYYYOO.",
    "..OOOYYYOO..",
    "...DDDDDDD..",
];

fn step_garcia(game: &mut GameState, input: &InputState) {
    use crate::world::WORLD_W;
    use crate::physics::WeaponKind;

    let g = match game.garcia.as_mut() { Some(g) => g, None => return };

    if g.falling {
        // Concrete Donkey / Hand of Jerry as it was on the exp branch before the drag-fall and tunnelling experiments (reverted at the
        // user's request 2026-11-24): a bazooka-class bomb spawned at y = -208 (WA level frame) with v = 0, free fall at 0.24 px/frame^2 (no
        // drag, no speed cap), wind 70 %, one blast on first contact. (WA's real bomb has drag 0.9 and the video shows repeated blasts:
        // findings/physics.md "Concrete Donkey", kept there in case it is wanted again.)
        const GRAVITY_TICK: f32 = 0.24 * (5.0 / 3.0) * (5.0 / 3.0);
        g.vel_y += GRAVITY_TICK;
        g.vel_x += game.wind.value() * crate::physics::tick::BAZOOKA_WIND_SCALE * 0.7;
        g.fall_y += g.vel_y;
        g.cursor_x = (g.cursor_x + g.vel_x).clamp(0.0, WORLD_W as f32 - 1.0);
        // The sprite (and the live client's camera) are drawn at render_x: keep it on the wind-drifted x or the donkey falls straight
        // and the blast appears tens of px away from where it vanished ("teleports" on the first hit).
        g.render_x = g.cursor_x;

        let fx = g.cursor_x as i32;
        let fy = g.fall_y as i32;

        // Collision: the actual terrain surface below the sprite centre (surface_y_at keeps working after craters). With no solid
        // terrain in the column fall back to the water line so a donkey over water splashes where it can be seen.
        let surface_y = if fx >= 0 && fx < crate::world::WORLD_W as i32 {
            game.terrain.surface_y_at(fx as u32)
                .map(|y| y as i32)
                .unwrap_or(crate::world::WATER_Y as i32)
        } else {
            crate::world::WATER_Y as i32
        };

        // Hit when the sprite's lowest point reaches the terrain surface.
        let hit_ground = g.vel_y > 0.0 && fy + 54 >= surface_y;
        let cursor_x = g.cursor_x;
        drop(g);

        if hit_ground {
            let land_y = surface_y as f32;
            let hit_pos = crate::world::WorldPos::new(cursor_x, land_y);
            game.apply_explosion(hit_pos, WeaponKind::Garcia);
            game.emit_sound(crate::audio::Sfx::Smash);
            game.garcia = None;
        }
        return;
    }

    // Targeting phase: Left/Right move cursor, A confirms, B cancels
    const CURSOR_SPEED: f32 = 14.0;
    if input.held(Button::Left) {
        g.cursor_x = (g.cursor_x - CURSOR_SPEED).max(0.0);
    }
    if input.held(Button::Right) {
        g.cursor_x = (g.cursor_x + CURSOR_SPEED).min(WORLD_W as f32 - 1.0);
    }
    if input.held(Button::Up) {
        g.cursor_y = (g.cursor_y - CURSOR_SPEED).max(12.0);
    }
    if input.held(Button::Down) {
        g.cursor_y = (g.cursor_y + CURSOR_SPEED).min(crate::world::WATER_Y as f32 - 5.0);
    }
    // Smooth render_x/render_y toward cursor_x/cursor_y
    g.render_x += (g.cursor_x - g.render_x) * 0.25;
    g.render_y += (g.cursor_y - g.render_y) * 0.25;
    g.blink_timer = g.blink_timer.wrapping_add(1);

    if input.just_pressed(Button::A) && game.garcia.is_some() {
        // Confirm — consume weapon charge and start falling
        let ti = game.active_team();
        if !game.teams[ti].consume_weapon() {
            game.garcia = None;
            return;
        }
        game.teams[ti].prune_empty_weapons();
        let si = game.teams[ti].active;
        game.teams[ti].soldiers[si].has_fired = true;
        game.turn.on_fired();
        if let Some(g) = game.garcia.as_mut() {
            g.falling      = true;
            g.fall_y       = crate::world::WATER_Y as f32 - 696.0 - 208.0; // WA: y = -208 in the level frame, whatever the cursor height
            g.vel_y        = 0.0;
            g.vel_x        = 0.0;
            g.bounce_count = 0;
            g.blink_timer  = 0;
        }
        game.emit_sound(crate::audio::Sfx::Garcia);
    } else if input.just_pressed(Button::B) {
        // Cancel targeting
        game.garcia = None;
    }
}

fn step_airstrike(game: &mut GameState, input: &InputState) {
    use crate::physics::projectile::{Projectile, WeaponKind};
    use crate::world::{Vec2, WorldPos, WORLD_W, SCREEN_W};

    const BOMB_COUNT:   u32 = 5;
    const BOMB_SPACING: f32 = 32.0; // WA rec 27 +0x40
    // WA strike object (0x5553c0 / 0x555a20 / 0x555800, rec 27): the plane starts 1024 px before the target and flies 32 px per 50 Hz
    // frame (= BOMB_SPACING: one bomb per frame), a bomb leaves the plane at y = -208 (WA level frame) with vy 0 and vx = 4 px/frame
    // in the flight direction (rec+0x44 = 100 -> 100*65536/25), gravity 0.24 px/frame^2, no wind. The first bomb is released once
    // |plane - target| <= R = (4 px * frames the bomb needs to fall to the target y) + count*spacing/2.
    const PLANE_SPEED:  f32 = 32.0 * 5.0 / 3.0; // px per 30 Hz tick
    const APPROACH:     f32 = 1024.0;
    const BOMB_VX:      f32 = 4.0 * 5.0 / 3.0;
    /// Frame index (0-based, 32 px per frame from the start) of the first drop for a target at `ty`.
    fn first_drop_frame(ty: f32) -> u32 {
        let spawn_y = crate::world::WATER_Y as f32 - 696.0 - 208.0;
        let (mut y, mut vy, mut lead) = (spawn_y, 0.0f32, 0.0f32);
        if ty > spawn_y {
            loop { vy += 0.24; lead += 4.0; y += vy; if y >= ty { break; } }
        }
        let r = (lead + BOMB_COUNT as f32 * BOMB_SPACING / 2.0).max(BOMB_SPACING * 4.0 / 3.0).min(APPROACH);
        ((APPROACH - r) / BOMB_SPACING).ceil().max(0.0) as u32
    }
    const CURSOR_SPEED: f32 = 14.0;

    // World X where the plane releases its `k`-th bomb (k = 0..5) for a plane flying `dir` (+1/-1) at a target (tx, ty).
    fn drop_x(tx: f32, ty: f32, dir: f32, k: u32) -> f32 {
        tx - dir * APPROACH + dir * BOMB_SPACING * (first_drop_frame(ty) + k) as f32
    }

    let s = match game.airstrike.as_mut() { Some(s) => s, None => return };

    if !s.active {
        if input.held(Button::Left)  { s.cursor_x = (s.cursor_x - CURSOR_SPEED).max(0.0); }
        if input.held(Button::Right) { s.cursor_x = (s.cursor_x + CURSOR_SPEED).min(WORLD_W as f32 - 1.0); }
        if input.held(Button::Up)    { s.cursor_y = (s.cursor_y - CURSOR_SPEED).max(12.0); }
        if input.held(Button::Down)  { s.cursor_y = (s.cursor_y + CURSOR_SPEED).min(crate::world::WATER_Y as f32 - 5.0); }
        if input.just_pressed(Button::L1) { s.direction_right = false; }
        if input.just_pressed(Button::R1) { s.direction_right = true;  }
        s.render_x += (s.cursor_x - s.render_x) * 0.25;
        s.render_y += (s.cursor_y - s.render_y) * 0.25;
        s.blink_timer = s.blink_timer.wrapping_add(1);

        if input.just_pressed(Button::A) {
            let dir_right = s.direction_right;
            drop(s);
            let ti = game.active_team();
            if !game.teams[ti].consume_weapon() { game.airstrike = None; return; }
            game.teams[ti].prune_empty_weapons();
            let si = game.teams[ti].active;
            game.teams[ti].soldiers[si].has_fired = true;
            game.turn.on_fired();
            let s = game.airstrike.as_mut().unwrap();
            s.active        = true;
            s.plane_x       = if dir_right { s.cursor_x - APPROACH } else { s.cursor_x + APPROACH };
            s.plane_vx      = if dir_right { PLANE_SPEED } else { -PLANE_SPEED };
            s.bombs_dropped = 0;
        } else if input.just_pressed(Button::B) {
            game.airstrike = None;
        }
        return;
    }

    // Active phase: advance plane, drop bomb when plane crosses each bomb's X
    let s = game.airstrike.as_mut().unwrap();
    s.plane_x += s.plane_vx;

    // The plane covers up to two bomb spacings per tick, so release every bomb it has passed.
    loop {
        let s = game.airstrike.as_ref().unwrap();
        if s.bombs_dropped >= BOMB_COUNT { break; }
        let (cursor_x, cursor_y) = (s.cursor_x, s.cursor_y);
        let dir_right = s.direction_right;
        let dir       = if dir_right { 1.0 } else { -1.0 };
        let bx        = drop_x(cursor_x, cursor_y, dir, s.bombs_dropped);
        let passed    = if dir_right { s.plane_x >= bx } else { s.plane_x <= bx };
        if !passed { break; }
        game.projectiles.push(Projectile::new(
            WorldPos::new(bx, crate::world::WATER_Y as f32 - 696.0 - 208.0),
            Vec2::new(dir * BOMB_VX, 0.0),
            WeaponKind::AirStrike,
        ));
        game.airstrike.as_mut().unwrap().bombs_dropped += 1;
    }

    let s = game.airstrike.as_ref().unwrap();
    let plane_gone = if s.direction_right {
        s.plane_x > WORLD_W as f32 + 120.0
    } else {
        s.plane_x < -120.0
    };
    if s.bombs_dropped >= BOMB_COUNT && plane_gone {
        game.airstrike = None;
    }
}

fn step_homing_missile(game: &mut GameState, input: &InputState) {
    use crate::input::Button;
    use crate::world::{WORLD_W, WATER_Y};
    use crate::physics::projectile::{Projectile, WeaponKind};
    use crate::world::{Vec2, WorldPos};

    let hm = match game.homing_missile.as_mut() { Some(h) => h, None => return };
    hm.blink_timer = hm.blink_timer.wrapping_add(1);
    hm.render_x += (hm.cursor_x - hm.render_x) * 0.25;
    hm.render_y += (hm.cursor_y - hm.render_y) * 0.25;
    if input.held(Button::Left)  { hm.cursor_x -= 14.0; }
    if input.held(Button::Right) { hm.cursor_x += 14.0; }
    if input.held(Button::Up)    { hm.cursor_y -= 10.0; }
    if input.held(Button::Down)  { hm.cursor_y += 10.0; }
    hm.cursor_x = hm.cursor_x.clamp(0.0, WORLD_W as f32);
    hm.cursor_y = hm.cursor_y.clamp(12.0, WATER_Y as f32 - 20.0);

    if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
        game.homing_missile.as_mut().unwrap().confirmed = true;
        game.aim.charge_armed = false; // require A release before charge starts
        game.aim.power = 0.0;
        game.messages.push(crate::game::state::GameMessage {
            text: "TARGET LOCKED - AIM AND CHARGE".to_string(),
            team: None,
            ticks: 60,
        });
    }
}

fn fire_weapon(game: &mut GameState) {
    use crate::physics::projectile::{Projectile, WeaponKind, FuseState};
    use crate::world::{Vec2, WorldPos};

    let ti = game.active_team();
    let si = game.teams[ti].active;
    let kind = game.teams[ti].current_weapon();

    // Consume one use of the weapon (no-op for infinite ammo; blocks fire if out)
    if !game.teams[ti].consume_weapon() { return; }
    game.teams[ti].prune_empty_weapons();


    let fm    = game.teams[ti].soldiers[si].facing as f32;
    let angle = game.aim.angle;
    // Launch rules are shared with the CPU aim search (physics::tick::{muzzle_spawn, launch_speed}).
    let power = crate::physics::tick::launch_speed(kind, game.aim.power);
    let spawn = crate::physics::tick::muzzle_spawn(
        game.teams[ti].soldiers[si].pos.x, game.teams[ti].soldiers[si].pos.y, angle, fm);
    let vel   = Vec2::new(angle.cos() * power * fm, -angle.sin() * power);
    crate::dlog!("shot", "tick={} FIRE {:?} team={} soldier={} from ({:.1},{:.1}) muzzle ({:.2},{:.2}) angle={:.3} charge={:.3} vel=({:.3},{:.3}) wind_u={}",
        game.tick, kind, ti, si, game.teams[ti].soldiers[si].pos.x, game.teams[ti].soldiers[si].pos.y, spawn.x, spawn.y,
        angle, game.aim.power, vel.x, vel.y, game.wind.units());

    let mut proj = Projectile::new(spawn, vel, kind);
    proj.owner = Some((ti, si));
    if kind == WeaponKind::Grenade || kind == WeaponKind::ClusterBomb || kind == WeaponKind::BananaBomb {
        proj.fuse = FuseState::Burning(game.aim.fuse_ticks);
    }
    if kind == WeaponKind::HomingMissile {
        if let Some(ref hm) = game.homing_missile {
            proj.homing_target = Some((hm.cursor_x, hm.cursor_y));
        }
        game.homing_missile = None;
        game.retreat_locked = true;
    }
    // HHG uses a fixed 3-second fuse (already set by Projectile::new via default_fuse_ticks)
    match kind {
        WeaponKind::Grenade       => {}
        WeaponKind::BananaBomb    => {} // silent on throw; explosion fires play_explosion() naturally
        WeaponKind::Blasthive     => {} // silent on throw
        WeaponKind::BlackHoleBomb => {} // silent on throw
        _                         => {},
    }
    game.projectiles.push(proj);
    game.teams[ti].soldiers[si].has_fired = true;
    game.turn.on_fired();
}

fn fire_shotgun(game: &mut GameState, muzzle_override: Option<(f32, f32)>) {
    use crate::game::soldier::SoldierState;
    use crate::world::Vec2;

    // WA-style shotgun: each trigger pull is ONE precise hitscan ray along the
    // exact aim angle — no simulated pellet cloud. Two shots per turn, up to
    // 25 damage each (50 total). The pellet spread you see at the impact point
    // is purely cosmetic dust, emitted below.
    const RANGE:  i32   = 220;    // pixels before the shot expires
    const SHOT_DMG: u32 = 25;     // full damage on a direct hit
    const SHOT_FORCE: f32 = 7.0;  // directional knockback (px/tick)
    const RECOIL: f32  = 2.0;     // shooter kickback
    const FAKE_PELLETS: u32 = 4;  // cosmetic impact spots around the real hit
    const SPREAD: f32  = 0.10;    // ±0.10 rad visual scatter for the fake pellets

    let ti = game.active_team();
    let si = game.teams[ti].active;
    let fm = game.teams[ti].soldiers[si].facing as f32;
    let base_angle = game.aim.angle;

    // Fire from the rendered barrel tip so shots start exactly where the reticle does.
    let (muzzle_x, muzzle_y) = muzzle_override.unwrap_or_else(|| {
        let x = game.teams[ti].soldiers[si].pos.x + base_angle.cos() * fm * 26.0;
        let y = game.teams[ti].soldiers[si].pos.y - 20.0 - base_angle.sin() * 26.0;
        (x, y)
    });

    // Deterministic per-shot seed (visual scatter only).
    let seed = game.tick.wrapping_mul(1664525).wrapping_add(1013904223);

    let n_teams  = game.teams.len();
    let n_sol: Vec<usize> = (0..n_teams).map(|t| game.teams[t].soldiers.len()).collect();
    let dx = base_angle.cos() * fm;
    let dy = -base_angle.sin();

    // ── The real shot: single ray, single hit ─────────────────────────────────
    let mut px = muzzle_x;
    let mut py = muzzle_y;
    let mut stopped = false;
    for _ in 0..RANGE {
        px += dx;
        py += dy;
        let ix = px as i32;
        let iy = py as i32;
        if ix < 0 || iy < 0 { break; }
        if game.terrain.is_solid(ix, iy) {
            if !hitscan_hit_crate(game, px, py) {
                let crater = crate::world::Crater::new(px, py, 5.0);
                crater.carve(&mut game.terrain);
                game.crater_log.push((px, py, 5.0));
            }
            stopped = true;
            break;
        }
        // Barrel direct hit
        for barrel in &mut game.barrels {
            if let super::state::BarrelState::Normal = barrel.state {
                if (barrel.pos.x - px).abs() < 8.0 && (barrel.pos.y - py).abs() < 12.0 {
                    barrel.state = super::state::BarrelState::Triggered { ticks: 6 };
                    stopped = true;
                    break;
                }
            }
        }
        if stopped { break; }
        // Soldier hit: same bounding box used by projectile collision
        'soldiers: for t in 0..n_teams {
            for s in 0..n_sol[t] {
                if !game.teams[t].soldiers[s].is_alive() { continue; }
                let spx = game.teams[t].soldiers[s].pos.x;
                let spy = game.teams[t].soldiers[s].pos.y;
                let ddx = (px - spx).abs();
                let ddy = py - spy;
                let hit_top = if crate::renderer::skeleton::SOLDIER_STYLE_V2 { -30.0 } else { -22.0 };
                if ddx < 8.0 && ddy > hit_top && ddy < 2.0 {
                    stopped = true;
                    break 'soldiers;
                }
            }
        }
        if stopped { break; }
    }
    let (end_x, end_y) = (px, py);

    // ── WA-style gun blast: damage falls off with distance from the impact ─────
    // The ray stops at the first surface it meets (terrain, worm, barrel, crate)
    // and the "blast" is centred there — matching WA, where a Shotgun/Handgun/Uzi
    // bullet spawns a tiny explosion at its line-of-fire hit. Each worm takes
    // damage by how close its body centre is to that point: a clean, centred hit
    // deals the full 25 (CORE_R plateau), while a graze or a shot that clips
    // terrain beside the worm deals proportionally less, and beyond MAX_R
    // nothing. This is why a shot can land for under 25 — and why a near-miss
    // that stops on terrain next to a worm now still splashes it, instead of the
    // old all-or-nothing bounding-box hit.
    const CORE_R: f32 = 18.0;  // within this, full damage (covers a clean body hit)
    const MAX_R:  f32 = 40.0;  // beyond this, no damage
    let active_ti = game.active_team();
    let active_si = game.teams[active_ti].active;
    let active_hp_before = game.teams[active_ti].soldiers[active_si].hp;
    let mut any_hit = false;
    for t in 0..n_teams {
        for s in 0..n_sol[t] {
            if !game.teams[t].soldiers[s].is_alive() { continue; }
            let spx = game.teams[t].soldiers[s].pos.x;
            let spy = game.teams[t].soldiers[s].pos.y;
            let hit_top = if crate::renderer::skeleton::SOLDIER_STYLE_V2 { -30.0 } else { -22.0 };
            let body_cy = spy + (hit_top + 2.0) * 0.5;   // vertical centre of the body
            let cdx = end_x - spx;
            let cdy = end_y - body_cy;
            let dist = (cdx * cdx + cdy * cdy).sqrt();
            if dist >= MAX_R { continue; }
            let falloff = ((MAX_R - dist) / (MAX_R - CORE_R)).min(1.0);
            let dmg = (SHOT_DMG as f32 * falloff).round() as u32;
            if dmg == 0 { continue; }
            any_hit = true;
            let vx = dx * SHOT_FORCE * falloff;
            let vy = dy * SHOT_FORCE * falloff - 1.0;
            let sol = &mut game.teams[t].soldiers[s];
            sol.death_cause = crate::game::soldier::DeathCause::Explosion;
            sol.kill_weapon = Some(crate::physics::WeaponKind::Shotgun);
            sol.take_damage(dmg);
            // Damage shows per SHOT: collapse the settle window so this shell's
            // number pops immediately; the second shell tallies separately.
            sol.damage_settle = 0;
            let new_state = match &sol.state {
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
            if let Some(st) = new_state { game.teams[t].soldiers[s].state = st; }
        }
    }
    // Blood splats fanned out behind the impact (visual only).
    if any_hit {
        let mut rng = seed as u64;
        for _ in 0..3 {
            rng = rng.wrapping_mul(2654435761).wrapping_add(1);
            let fwd = 5.0 + (rng & 0x1F) as f32 * 0.5;
            let lat = ((rng >> 5) & 0xF) as f32 - 7.5;
            game.blood_splats.push((
                crate::world::WorldPos::new(
                    end_x + dx * fwd + (-dy) * lat,
                    end_y + dy * fwd + ( dx) * lat,
                ), 90,
            ));
        }
    }
    if game.teams[active_ti].soldiers[active_si].hp < active_hp_before {
        game.active_worm_hit = true;
    }

    // ── Cosmetic pellet spread: VISUAL ONLY ───────────────────────────────────
    // March a few scattered rays with read-only terrain checks and puff dust
    // where each would land — sells the shotgun blast without any of them
    // dealing damage or carving terrain. Routed through emit_fx so live
    // clients replay the identical burst.
    for k in 0..FAKE_PELLETS {
        let r = seed.wrapping_mul(k + 7).wrapping_add(k * 31337) as f32 / u32::MAX as f32;
        let ang = base_angle + (r - 0.5) * 2.0 * SPREAD;
        let (fdx, fdy) = (ang.cos() * fm, -ang.sin());
        let mut fx = muzzle_x;
        let mut fy = muzzle_y;
        for _ in 0..RANGE {
            fx += fdx;
            fy += fdy;
            let (ix, iy) = (fx as i32, fy as i32);
            if ix < 0 || iy < 0 { break; }
            if game.terrain.is_solid(ix, iy) {
                game.emit_fx(crate::renderer::fx::FxEvent::Dust {
                    x: fx, y: fy, count: 3, kick: 0.6, dir: -fdx,
                });
                break;
            }
        }
    }

    // Recoil — kick the shooter backwards and slightly upward
    let recoil_vx = -fm * RECOIL;
    let recoil_vy = -1.5;
    let shooter_state = match &game.teams[ti].soldiers[si].state {
        SoldierState::Airborne { vel, spinning } => SoldierState::Airborne {
            vel: Vec2::new(vel.x + recoil_vx, vel.y + recoil_vy),
            spinning: *spinning,
        },
        _ => SoldierState::Airborne {
            vel: Vec2::new(recoil_vx, recoil_vy),
            spinning: false,
        },
    };
    let y0 = game.teams[ti].soldiers[si].pos.y;
    game.teams[ti].soldiers[si].state = shooter_state;
    game.teams[ti].soldiers[si].fall.begin_fall(y0);

    game.flush_crate_damage();
    // Decrement; end turn when all shots exhausted
    game.shotgun_shots_left -= 1;
    if game.shotgun_shots_left == 0 {
        game.teams[ti].soldiers[si].has_fired = true;
        game.turn.on_fired();
    } else {
        game.aim.power = 0.0;
    }
}

/// Fire the active soldier's selected weapon (used by TAT mode).
pub fn fire_bazooka_tat(game: &mut GameState) { fire_weapon(game); }

/// One tick of TAT replay: process the weapon menu first, then run server_tick
/// only if the menu is not open. Mirrors what tick() does during recording so
/// weapon switches, fire-grace suppression, and sim-skip-while-menu-open all
/// behave identically. Returns true if the menu was open (server_tick skipped).
///
/// Both TAT replay paths in src/main.rs call this. The parity test
/// `tat_replay_applies_weapon_switch` calls it too — so the test actually
/// exercises the same code that runs in production.
pub fn replay_tick(game: &mut GameState, prev_bits: u16, curr_bits: u16) -> bool {
    use crate::input::InputState;
    let input = InputState::from_bits(prev_bits, curr_bits);
    let menu_open = process_weapon_menu(game, &input);
    if !menu_open {
        server_tick(game, &input, None, None);
        retain_non_pickup_messages(game);
    }
    menu_open
}

/// Drop crate-pickup notices from the message list. Used by the TAT replay
/// paths so the fast-forward/replay doesn't re-show stale pickup messages.
pub fn retain_non_pickup_messages(game: &mut GameState) {
    game.messages.retain(|m| !m.text.contains("got a ") && !m.text.contains("picked up"));
}

/// Place a TNT stick in front of the active soldier with a 5-second fuse (150 ticks at 30 Hz).
/// Enters Watching immediately so the fuse burns the same proven path as a grenade.
/// Retreat phase follows the explosion.
pub fn fire_tnt(game: &mut GameState, ti: usize, si: usize) {
    use crate::physics::projectile::Projectile;
    use crate::world::WorldPos;
    if !game.teams[ti].consume_weapon() { return; }
    game.teams[ti].prune_empty_weapons();
    let facing = game.teams[ti].soldiers[si].facing as f32;
    let sx = game.teams[ti].soldiers[si].pos.x + facing * 6.0;
    let sy = game.teams[ti].soldiers[si].pos.y - 4.0;
    game.projectiles.push(Projectile::new_tnt(WorldPos::new(sx, sy), 150));
    game.teams[ti].soldiers[si].has_fired = true;
    game.tnt_placed = true;
    game.turn.on_fired(); // enter Watching; fuse steps on the proven path
}

/// Place a landmine immediately in front of the active soldier. Mine starts arming (3s delay).
pub fn fire_mine(game: &mut GameState, ti: usize, si: usize) {
    use crate::world::WorldPos;
    use super::state::{PlacedMine, MineState};
    if !game.teams[ti].consume_weapon() { return; }
    game.teams[ti].prune_empty_weapons();
    let facing = game.teams[ti].soldiers[si].facing as f32;
    let sx = game.teams[ti].soldiers[si].pos.x + facing * 6.0;
    let sy = game.teams[ti].soldiers[si].pos.y;
    game.mines.push(PlacedMine {
        pos: WorldPos::new(sx, sy),
        state: MineState::Arming,
        arm_ticks: 90, // 3s at 30Hz
        trigger_ticks: 0,
    });
    game.teams[ti].soldiers[si].has_fired = true;
    game.turn.on_fired();
}

/// Baseball bat: melee swing that launches the nearest enemy soldier.
fn fire_baseball_bat(game: &mut GameState, ti: usize, si: usize) {
    use crate::game::soldier::{DeathCause, SoldierState};
    use crate::world::Vec2;

    const BAT_REACH:  f32 = 28.0; // horizontal range in front
    const BAT_HEIGHT: f32 = 22.0; // vertical window
    const BAT_POWER:  f32 = 15.0;
    const BAT_DAMAGE: u32 = 26; // was 30, -15%

    let sx = game.teams[ti].soldiers[si].pos.x;
    let sy = game.teams[ti].soldiers[si].pos.y;
    let fm = game.teams[ti].soldiers[si].facing as f32;
    let angle = game.aim.angle;

    // Collect soldiers inside the bat's swing arc (hits teammates too)
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for target_ti in 0..game.teams.len() {
        for target_si in 0..game.teams[target_ti].soldiers.len() {
            if target_ti == ti && target_si == si { continue; } // skip self
            if !game.teams[target_ti].soldiers[target_si].is_alive() { continue; }
            let tx = game.teams[target_ti].soldiers[target_si].pos.x;
            let ty = game.teams[target_ti].soldiers[target_si].pos.y;
            let dx = (tx - sx) * fm; // positive = in front of soldier
            let dy = (ty - sy).abs();
            if dx >= 0.0 && dx <= BAT_REACH && dy <= BAT_HEIGHT {
                hits.push((target_ti, target_si));
            }
        }
    }

    if !hits.is_empty() {
        game.emit_sound(crate::audio::Sfx::Bat);
    }
    for (target_ti, target_si) in hits {
        let target = &mut game.teams[target_ti].soldiers[target_si];
        // Launch first — damage is deferred until landing (WA-style: the knockback
        // plays out before the hit registers), applied alongside fall damage at
        // either landing site in this file.
        let grounded = matches!(target.state, SoldierState::Idle | SoldierState::Walking { .. });
        if grounded { target.fall.begin_fall(target.pos.y); }
        target.state = SoldierState::Airborne {
            vel: Vec2::new(BAT_POWER * angle.cos() * fm, -BAT_POWER * angle.sin()),
            spinning: true,
        };
        target.pending_bat_damage = target.pending_bat_damage.saturating_add(BAT_DAMAGE);
        // Fresh air clock so the knockback spin animates in full (a stale
        // airtime >= 20 would cancel `spinning` on the first airborne tick).
        target.airtime = 0;
    }

    game.teams[ti].soldiers[si].has_fired = true;
    game.turn.on_fired();
}

/// Hitscan revolver shot: ray-march up to 800 px, deal 15 damage + knockback to first soldier hit.
/// Check if a hitscan ray endpoint is inside a landed crate and damage/destroy it.
/// Returns true if a crate was hit (ray should stop).
fn hitscan_hit_crate(game: &mut GameState, rx: f32, ry: f32) -> bool {
    for crate_ in &mut game.crates {
        if !crate_.landed { continue; }
        if (crate_.pos.x - rx).abs() < 12.0 && (crate_.pos.y - ry).abs() < 12.0 {
            crate_.damage_this_turn = crate_.damage_this_turn.saturating_add(25);
            return true;
        }
    }
    false
}

fn fire_revolver_shot(game: &mut GameState, ti: usize, si: usize, muzzle_override: Option<(f32, f32)>) {
    game.emit_sound(crate::audio::Sfx::Revolver);
    use crate::world::Vec2;
    use crate::game::soldier::{DeathCause, SoldierState};

    const MAX_RANGE:  f32 = 800.0;
    const STEP:       f32 = 1.0;
    const DAMAGE:     u32 = 15;
    const KNOCKBACK:  f32 = 3.5;

    let fm    = game.teams[ti].soldiers[si].facing as f32;
    let angle = game.aim.angle;
    let step_x = angle.cos() * fm * STEP;
    let step_y = -angle.sin() * STEP;

    // Fire from the rendered barrel tip so the ray starts exactly where the reticle does.
    let (mut rx, mut ry) = muzzle_override.unwrap_or_else(|| {
        let x = game.teams[ti].soldiers[si].pos.x + angle.cos() * fm * 26.0;
        let y = game.teams[ti].soldiers[si].pos.y - 20.0 - angle.sin() * 26.0;
        (x, y)
    });
    let steps = (MAX_RANGE / STEP) as u32;
    let mut hit_ti: Option<usize> = None;
    let mut hit_si_idx: Option<usize> = None;

    'ray: for _ in 0..steps {
        rx += step_x;
        ry += step_y;
        if rx < 0.0 || rx >= crate::world::WORLD_W as f32 { break; }
        if ry >= crate::world::WATER_Y as f32 { break; }
        // Check soldiers/barrels at this step BEFORE terrain: a target standing on
        // the surface has its legs at ground level, so a leg shot must register the
        // soldier rather than stopping on the ground pixel first. A wall in front is
        // still hit at an earlier step, so it continues to block the shot.
        // Barrel hit
        for barrel in &mut game.barrels {
            if let super::state::BarrelState::Normal = barrel.state {
                if (barrel.pos.x - rx).abs() < 8.0 && (barrel.pos.y - ry).abs() < 12.0 {
                    barrel.state = super::state::BarrelState::Triggered { ticks: 6 };
                    break 'ray;
                }
            }
        }
        for check_ti in 0..game.teams.len() {
            for check_si in 0..game.teams[check_ti].soldiers.len() {
                if check_ti == ti && check_si == si { continue; }
                if !game.teams[check_ti].soldiers[check_si].is_alive() { continue; }
                let sx2 = game.teams[check_ti].soldiers[check_si].pos.x;
                let sy2 = game.teams[check_ti].soldiers[check_si].pos.y;
                // Hit if ray passes through the soldier's body: full width ±10px,
                // height from foot (sy2) up to head (sy2 - BODY_H).
                let hit_top_offset = if crate::renderer::skeleton::SOLDIER_STYLE_V2 { 10.0 } else { 2.0 };
                if (rx - sx2).abs() < 10.0
                    && ry >= sy2 - crate::renderer::draw_sprites::BODY_H as f32 - hit_top_offset
                    && ry <= sy2 + 4.0
                {
                    hit_ti = Some(check_ti);
                    hit_si_idx = Some(check_si);
                    break 'ray;
                }
            }
        }
        // Terrain blocks the shot only after the soldier/barrel checks at this step.
        if game.terrain.is_solid(rx as i32, ry as i32) { break; }
    }

    if let (Some(hti), Some(hsi)) = (hit_ti, hit_si_idx) {
        let dir_x = step_x / STEP;
        let dir_y = step_y / STEP;
        let vx = dir_x * KNOCKBACK;
        let vy = dir_y * KNOCKBACK - 1.5;
        let s = &mut game.teams[hti].soldiers[hsi];
        s.death_cause = DeathCause::Explosion;
        s.kill_weapon = Some(crate::physics::WeaponKind::Revolver);
        s.take_damage(DAMAGE);
        if s.is_alive() {
            let was_grounded = matches!(s.state, SoldierState::Idle | SoldierState::Walking { .. });
            s.state = match &s.state {
                SoldierState::Airborne { vel, spinning } => SoldierState::Airborne {
                    vel: Vec2::new(vel.x + vx, vel.y + vy),
                    spinning: *spinning,
                },
                _ => { if was_grounded { s.fall.begin_fall(s.pos.y); } SoldierState::Airborne { vel: Vec2::new(vx, vy), spinning: false } },
            };
        }
        // Do NOT set active_worm_hit here — the shooter can never be hit by their own
        // ray (excluded in the march), and teammate hits should not cut the sequence short.
        game.blood_splats.push((crate::world::WorldPos::new(rx, ry), 75));

    } else if rx >= 0.0 && rx < crate::world::WORLD_W as f32
           && ry >= 0.0 && ry < crate::world::WATER_Y as f32 {
        if !hitscan_hit_crate(game, rx, ry) {
            let crater = crate::world::Crater::new(rx, ry, 3.0);
            crater.carve(&mut game.terrain);
            game.crater_log.push((rx, ry, 3.0));
        }
    }

    game.flush_crate_damage();
    game.revolver_shots_left = game.revolver_shots_left.saturating_sub(1);
    if game.revolver_shots_left == 0 {
        game.teams[ti].soldiers[si].has_fired = true;
        game.turn.on_fired();
    }
}

fn fire_pistol_shot(game: &mut GameState, ti: usize, si: usize, muzzle_override: Option<(f32, f32)>) {
    game.emit_sound(crate::audio::Sfx::Pistol);
    use crate::world::Vec2;
    use crate::game::soldier::{DeathCause, SoldierState};

    const MAX_RANGE: f32 = 700.0;
    const STEP:      f32 = 1.0;
    const DAMAGE:    u32 = 5;
    const KNOCKBACK: f32 = 1.0;

    let fm    = game.teams[ti].soldiers[si].facing as f32;
    let angle = game.aim.angle;
    let step_x = angle.cos() * fm * STEP;
    let step_y = -angle.sin() * STEP;

    let (mut rx, mut ry) = muzzle_override.unwrap_or_else(|| {
        let x = game.teams[ti].soldiers[si].pos.x + angle.cos() * fm * 18.0;
        let y = game.teams[ti].soldiers[si].pos.y - 14.0 - angle.sin() * 18.0;
        (x, y)
    });
    let steps = (MAX_RANGE / STEP) as u32;
    let mut hit_ti: Option<usize> = None;
    let mut hit_si_idx: Option<usize> = None;

    'ray: for _ in 0..steps {
        rx += step_x;
        ry += step_y;
        if rx < 0.0 || rx >= crate::world::WORLD_W as f32 { break; }
        if ry >= crate::world::WATER_Y as f32 { break; }
        for barrel in &mut game.barrels {
            if let super::state::BarrelState::Normal = barrel.state {
                if (barrel.pos.x - rx).abs() < 8.0 && (barrel.pos.y - ry).abs() < 12.0 {
                    barrel.state = super::state::BarrelState::Triggered { ticks: 6 };
                    break 'ray;
                }
            }
        }
        for check_ti in 0..game.teams.len() {
            for check_si in 0..game.teams[check_ti].soldiers.len() {
                if check_ti == ti && check_si == si { continue; }
                if !game.teams[check_ti].soldiers[check_si].is_alive() { continue; }
                let sx2 = game.teams[check_ti].soldiers[check_si].pos.x;
                let sy2 = game.teams[check_ti].soldiers[check_si].pos.y;
                let hit_top_offset = if crate::renderer::skeleton::SOLDIER_STYLE_V2 { 10.0 } else { 2.0 };
                if (rx - sx2).abs() < 10.0
                    && ry >= sy2 - crate::renderer::draw_sprites::BODY_H as f32 - hit_top_offset
                    && ry <= sy2 + 4.0
                {
                    hit_ti = Some(check_ti);
                    hit_si_idx = Some(check_si);
                    break 'ray;
                }
            }
        }
        if game.terrain.is_solid(rx as i32, ry as i32) { break; }
    }

    if let (Some(hti), Some(hsi)) = (hit_ti, hit_si_idx) {
        let dir_x = step_x / STEP;
        let dir_y = step_y / STEP;
        let vx = dir_x * KNOCKBACK;
        let vy = dir_y * KNOCKBACK - 0.5;
        let s = &mut game.teams[hti].soldiers[hsi];
        s.death_cause = DeathCause::Explosion;
        s.kill_weapon = Some(crate::physics::WeaponKind::Pistol);
        s.take_damage(DAMAGE);
        if s.is_alive() {
            let was_grounded = matches!(s.state, SoldierState::Idle | SoldierState::Walking { .. });
            s.state = match &s.state {
                SoldierState::Airborne { vel, spinning } => SoldierState::Airborne {
                    vel: Vec2::new(vel.x + vx, vel.y + vy),
                    spinning: *spinning,
                },
                _ => { if was_grounded { s.fall.begin_fall(s.pos.y); } SoldierState::Airborne { vel: Vec2::new(vx, vy), spinning: false } },
            };
        }
        game.blood_splats.push((crate::world::WorldPos::new(rx, ry), 75));
    } else if rx >= 0.0 && rx < crate::world::WORLD_W as f32
           && ry >= 0.0 && ry < crate::world::WATER_Y as f32 {
        if !hitscan_hit_crate(game, rx, ry) {
            let crater = crate::world::Crater::new(rx, ry, 3.0);
            crater.carve(&mut game.terrain);
            game.crater_log.push((rx, ry, 3.0));
        }
    }

    game.flush_crate_damage();
    game.pistol_shots_left = game.pistol_shots_left.saturating_sub(1);
    if game.pistol_shots_left == 0 {
        game.teams[ti].soldiers[si].has_fired = true;
        game.turn.on_fired();
    }
}

fn fire_minigun_shot(game: &mut GameState, ti: usize, si: usize, muzzle_override: Option<(f32, f32)>) {
    use crate::world::Vec2;
    use crate::game::soldier::{DeathCause, SoldierState};

    const MAX_RANGE:     f32 = 600.0;
    const STEP:          f32 = 1.0;
    const DAMAGE:        u32 = 5;
    const KNOCKBACK:     f32 = 3.5;
    const SPREAD:        f32 = 0.14; // ±8° in radians
    const SHOOTER_RECOIL:    f32 = 0.8;
    const SHOOTER_RECOIL_VY: f32 = -0.3;

    let fm    = game.teams[ti].soldiers[si].facing as f32;
    let angle = game.aim.angle;

    // Per-shot spread using tick + shots_left as entropy
    let seed = (game.tick as u32)
        .wrapping_mul(0x9E3779B9)
        .wrapping_add(game.minigun_shots_left as u32 * 2654435761_u32);
    let r = seed as f32 / u32::MAX as f32;
    let spread = (r - 0.5) * 2.0 * SPREAD;
    let fire_angle = angle + spread;

    let step_x = fire_angle.cos() * fm * STEP;
    let step_y = -fire_angle.sin() * STEP;

    // Shooter recoil — pushes backward each shot; accumulates across the burst
    {
        let recoil_vx = -fm * SHOOTER_RECOIL;
        let y0 = game.teams[ti].soldiers[si].pos.y;
        let shooter_state = match &game.teams[ti].soldiers[si].state {
            SoldierState::Airborne { vel, spinning } => SoldierState::Airborne {
                vel: Vec2::new(vel.x + recoil_vx, vel.y + SHOOTER_RECOIL_VY),
                spinning: *spinning,
            },
            _ => SoldierState::Airborne {
                vel: Vec2::new(recoil_vx, SHOOTER_RECOIL_VY),
                spinning: false,
            },
        };
        game.teams[ti].soldiers[si].state = shooter_state;
        game.teams[ti].soldiers[si].fall.begin_fall(y0);
    }

    let (mut rx, mut ry) = muzzle_override.unwrap_or_else(|| {
        let x = game.teams[ti].soldiers[si].pos.x + angle.cos() * fm * 26.0;
        let y = game.teams[ti].soldiers[si].pos.y - 20.0 - angle.sin() * 26.0;
        (x, y)
    });
    let start_x = rx;
    let start_y = ry;
    let steps = (MAX_RANGE / STEP) as u32;
    let mut hit_ti: Option<usize> = None;
    let mut hit_si_idx: Option<usize> = None;

    'ray: for _ in 0..steps {
        rx += step_x;
        ry += step_y;
        if rx < 0.0 || rx >= crate::world::WORLD_W as f32 { break; }
        if ry >= crate::world::WATER_Y as f32 { break; }
        // Barrel hit
        for barrel in &mut game.barrels {
            if let super::state::BarrelState::Normal = barrel.state {
                if (barrel.pos.x - rx).abs() < 8.0 && (barrel.pos.y - ry).abs() < 12.0 {
                    barrel.state = super::state::BarrelState::Triggered { ticks: 6 };
                    break 'ray;
                }
            }
        }
        // Mine hit — trigger armed mines the bullet passes through
        for mine in &mut game.mines {
            if mine.state == super::state::MineState::Armed {
                if (mine.pos.x - rx).abs() < 8.0 && (mine.pos.y - ry).abs() < 8.0 {
                    mine.state = super::state::MineState::Triggered;
                    mine.trigger_ticks = 0;
                    break 'ray;
                }
            }
        }
        for check_ti in 0..game.teams.len() {
            for check_si in 0..game.teams[check_ti].soldiers.len() {
                if check_ti == ti && check_si == si { continue; }
                if !game.teams[check_ti].soldiers[check_si].is_alive() { continue; }
                let sx2 = game.teams[check_ti].soldiers[check_si].pos.x;
                let sy2 = game.teams[check_ti].soldiers[check_si].pos.y;
                let hit_top_offset = if crate::renderer::skeleton::SOLDIER_STYLE_V2 { 10.0 } else { 2.0 };
                if (rx - sx2).abs() < 10.0
                    && ry >= sy2 - crate::renderer::draw_sprites::BODY_H as f32 - hit_top_offset
                    && ry <= sy2 + 4.0
                {
                    hit_ti = Some(check_ti);
                    hit_si_idx = Some(check_si);
                    break 'ray;
                }
            }
        }
        if game.terrain.is_solid(rx as i32, ry as i32) { break; }
    }

    if let (Some(hti), Some(hsi)) = (hit_ti, hit_si_idx) {
        let dir_x = step_x / STEP;
        let dir_y = step_y / STEP;
        let vx = dir_x * KNOCKBACK;
        let vy = dir_y * KNOCKBACK - 0.5;
        let s = &mut game.teams[hti].soldiers[hsi];
        s.death_cause = DeathCause::Explosion;
        s.kill_weapon = Some(crate::physics::WeaponKind::Minigun);
        s.take_damage(DAMAGE);
        if s.is_alive() {
            let was_grounded = matches!(s.state, SoldierState::Idle | SoldierState::Walking { .. });
            s.state = match &s.state {
                SoldierState::Airborne { vel, spinning } => SoldierState::Airborne {
                    vel: Vec2::new(vel.x + vx, vel.y + vy),
                    spinning: *spinning,
                },
                _ => { if was_grounded { s.fall.begin_fall(s.pos.y); } SoldierState::Airborne { vel: Vec2::new(vx, vy), spinning: false } },
            };
        }
        game.blood_splats.push((crate::world::WorldPos::new(rx, ry), 40));
    } else if rx >= 0.0 && rx < crate::world::WORLD_W as f32
           && ry >= 0.0 && ry < crate::world::WATER_Y as f32 {
        if !hitscan_hit_crate(game, rx, ry) {
            let crater = crate::world::Crater::new(rx, ry, 2.0);
            crater.carve(&mut game.terrain);
            game.crater_log.push((rx, ry, 2.0));
            let d = crate::game::state::biome_dirt(game.terrain.is_cavern, game.terrain.template_id);
            game.emit_fx(crate::renderer::fx::FxEvent::Dig {
                x: rx, y: ry,
                dir: -step_x.signum(),
                col: [d.r, d.g, d.b],
            });
        }
    }

    // Bullet trail visual (client-side, fades in 2 ticks)
    game.bullet_trails.push((
        crate::world::WorldPos::new(start_x, start_y),
        crate::world::WorldPos::new(rx, ry),
        2,
    ));

    game.flush_crate_damage();
    game.minigun_shots_left = game.minigun_shots_left.saturating_sub(1);
    if game.minigun_shots_left == 0 {
        game.teams[ti].soldiers[si].has_fired = true;
        game.turn.on_fired();
    }
}

fn fire_uzi_shot(game: &mut GameState, ti: usize, si: usize, muzzle_override: Option<(f32, f32)>) {
    use crate::world::Vec2;
    use crate::game::soldier::{DeathCause, SoldierState};

    const MAX_RANGE:         f32 = 450.0;
    const STEP:              f32 = 1.0;
    const DAMAGE:            u32 = 2; // 20% cut from 3
    const KNOCKBACK:         f32 = 2.0;
    const SPREAD:            f32 = 0.22; // ±12.6° — wider than minigun's ±8°
    const SHOOTER_RECOIL:    f32 = 0.5;
    const SHOOTER_RECOIL_VY: f32 = -0.2;

    let fm    = game.teams[ti].soldiers[si].facing as f32;
    let angle = game.aim.angle;

    let seed = (game.tick as u32)
        .wrapping_mul(0x9E3779B9)
        .wrapping_add(game.uzi_shots_left as u32 * 2654435761);
    let r = seed as f32 / u32::MAX as f32;
    let spread = (r - 0.5) * 2.0 * SPREAD;
    let fire_angle = angle + spread;

    let step_x = fire_angle.cos() * fm * STEP;
    let step_y = -fire_angle.sin() * STEP;

    {
        let recoil_vx = -fm * SHOOTER_RECOIL;
        let y0 = game.teams[ti].soldiers[si].pos.y;
        let shooter_state = match &game.teams[ti].soldiers[si].state {
            SoldierState::Airborne { vel, spinning } => SoldierState::Airborne {
                vel: Vec2::new(vel.x + recoil_vx, vel.y + SHOOTER_RECOIL_VY),
                spinning: *spinning,
            },
            _ => SoldierState::Airborne {
                vel: Vec2::new(recoil_vx, SHOOTER_RECOIL_VY),
                spinning: false,
            },
        };
        game.teams[ti].soldiers[si].state = shooter_state;
        game.teams[ti].soldiers[si].fall.begin_fall(y0);
    }

    let (mut rx, mut ry) = muzzle_override.unwrap_or_else(|| {
        let x = game.teams[ti].soldiers[si].pos.x + angle.cos() * fm * 26.0;
        let y = game.teams[ti].soldiers[si].pos.y - 20.0 - angle.sin() * 26.0;
        (x, y)
    });
    let start_x = rx;
    let start_y = ry;
    let steps = (MAX_RANGE / STEP) as u32;
    let mut hit_ti: Option<usize> = None;
    let mut hit_si_idx: Option<usize> = None;

    'ray: for _ in 0..steps {
        rx += step_x;
        ry += step_y;
        if rx < 0.0 || rx >= crate::world::WORLD_W as f32 { break; }
        if ry >= crate::world::WATER_Y as f32 { break; }
        for barrel in &mut game.barrels {
            if let super::state::BarrelState::Normal = barrel.state {
                if (barrel.pos.x - rx).abs() < 8.0 && (barrel.pos.y - ry).abs() < 12.0 {
                    barrel.state = super::state::BarrelState::Triggered { ticks: 6 };
                    break 'ray;
                }
            }
        }
        for mine in &mut game.mines {
            if mine.state == super::state::MineState::Armed {
                if (mine.pos.x - rx).abs() < 8.0 && (mine.pos.y - ry).abs() < 8.0 {
                    mine.state = super::state::MineState::Triggered;
                    mine.trigger_ticks = 0;
                    break 'ray;
                }
            }
        }
        for check_ti in 0..game.teams.len() {
            for check_si in 0..game.teams[check_ti].soldiers.len() {
                if check_ti == ti && check_si == si { continue; }
                if !game.teams[check_ti].soldiers[check_si].is_alive() { continue; }
                let sx2 = game.teams[check_ti].soldiers[check_si].pos.x;
                let sy2 = game.teams[check_ti].soldiers[check_si].pos.y;
                let hit_top_offset = if crate::renderer::skeleton::SOLDIER_STYLE_V2 { 10.0 } else { 2.0 };
                if (rx - sx2).abs() < 10.0
                    && ry >= sy2 - crate::renderer::draw_sprites::BODY_H as f32 - hit_top_offset
                    && ry <= sy2 + 4.0
                {
                    hit_ti = Some(check_ti);
                    hit_si_idx = Some(check_si);
                    break 'ray;
                }
            }
        }
        if game.terrain.is_solid(rx as i32, ry as i32) { break; }
    }

    if let (Some(hti), Some(hsi)) = (hit_ti, hit_si_idx) {
        let dir_x = step_x / STEP;
        let dir_y = step_y / STEP;
        let vx = dir_x * KNOCKBACK;
        let vy = dir_y * KNOCKBACK - 0.5;
        let s = &mut game.teams[hti].soldiers[hsi];
        s.death_cause = DeathCause::Explosion;
        s.kill_weapon = Some(crate::physics::WeaponKind::Uzi);
        s.take_damage(DAMAGE);
        if s.is_alive() {
            let was_grounded = matches!(s.state, SoldierState::Idle | SoldierState::Walking { .. });
            s.state = match &s.state {
                SoldierState::Airborne { vel, spinning } => SoldierState::Airborne {
                    vel: Vec2::new(vel.x + vx, vel.y + vy),
                    spinning: *spinning,
                },
                _ => { if was_grounded { s.fall.begin_fall(s.pos.y); } SoldierState::Airborne { vel: Vec2::new(vx, vy), spinning: false } },
            };
        }
        game.blood_splats.push((crate::world::WorldPos::new(rx, ry), 40));
    } else if rx >= 0.0 && rx < crate::world::WORLD_W as f32
           && ry >= 0.0 && ry < crate::world::WATER_Y as f32 {
        if !hitscan_hit_crate(game, rx, ry) {
            let crater = crate::world::Crater::new(rx, ry, 1.5);
            crater.carve(&mut game.terrain);
            game.crater_log.push((rx, ry, 1.5));
            let d = crate::game::state::biome_dirt(game.terrain.is_cavern, game.terrain.template_id);
            game.emit_fx(crate::renderer::fx::FxEvent::Dig {
                x: rx, y: ry,
                dir: -step_x.signum(),
                col: [d.r, d.g, d.b],
            });
        }
    }

    game.bullet_trails.push((
        crate::world::WorldPos::new(start_x, start_y),
        crate::world::WorldPos::new(rx, ry),
        2,
    ));

    game.flush_crate_damage();
    game.uzi_shots_left = game.uzi_shots_left.saturating_sub(1);
    if game.uzi_shots_left == 0 {
        game.teams[ti].soldiers[si].has_fired = true;
        game.turn.on_fired();
    }
}

// ── Weapon menu ──────────────────────────────────────────────────────────────

pub fn draw_weapon_menu(
    buf:    &mut WorldBuffer,
    weapons: &[(crate::physics::projectile::WeaponKind, Option<u32>)],
    cursor: usize,
    cam_x:  i32,
    cam_y:  i32,
    fuse_ticks: u32,
    turn_number: u32,
    num_teams: usize,
) {
    use crate::renderer::fb::Bgra;
    use crate::renderer::font::{draw_str, str_width, draw_str_scaled, str_width_scaled};
    use crate::physics::projectile::WeaponKind;
    use crate::world::{SCREEN_W, SCREEN_H};

    let cols: i32 = 4;
    let cell_w: i32 = 120;
    let cell_h: i32 = 56;
    const MAX_ROWS: i32 = 4;
    // Column-major: weapons fill down each column before moving right.
    // idx → row = idx % total_rows, col = idx / total_rows
    let total_rows  = ((weapons.len() as i32) + cols - 1) / cols;
    let cursor_row  = (cursor as i32) % total_rows;
    let scroll      = if total_rows <= MAX_ROWS { 0 } else {
        let scroll_min = (cursor_row - MAX_ROWS + 1).max(0);
        let scroll_max = cursor_row;
        scroll_min.min(scroll_max).min(total_rows - MAX_ROWS)
    };
    let visible_rows = total_rows.min(MAX_ROWS);
    let win_w = (cols * cell_w + 20) as u32;
    let win_h = (visible_rows * cell_h + 44) as u32;
    let wx = cam_x + (SCREEN_W as i32 - win_w as i32) / 2;
    let wy = cam_y + (SCREEN_H as i32 - win_h as i32) / 2;

    // Window background + border
    buf.fill_rect(wx, wy, win_w, win_h, Bgra::new(8, 10, 24));
    buf.fill_rect(wx, wy, win_w, 2, Bgra::new(80, 80, 140));
    buf.fill_rect(wx, wy + win_h as i32 - 2, win_w, 2, Bgra::new(80, 80, 140));
    buf.fill_rect(wx, wy, 2, win_h, Bgra::new(80, 80, 140));
    buf.fill_rect(wx + win_w as i32 - 2, wy, 2, win_h, Bgra::new(80, 80, 140));
    // Title
    let title = "WEAPONS";
    let tw = str_width_scaled(title, 2);
    draw_str_scaled(buf, title, wx + (win_w as i32 - tw)/2, wy + 8, Bgra::new(255, 210, 50), 2);

    // Scroll arrows
    let arr = Bgra::new(160, 160, 200);
    if scroll > 0 {
        let ax = wx + win_w as i32 - 14;
        let ay = wy + 10;
        buf.fill_rect(ax + 3, ay,     1, 1, arr);
        buf.fill_rect(ax + 2, ay + 1, 3, 1, arr);
        buf.fill_rect(ax + 1, ay + 2, 5, 1, arr);
    }
    if scroll + MAX_ROWS < total_rows {
        let ax = wx + win_w as i32 - 14;
        let ay = wy + win_h as i32 - 20;
        buf.fill_rect(ax + 1, ay,     5, 1, arr);
        buf.fill_rect(ax + 2, ay + 1, 3, 1, arr);
        buf.fill_rect(ax + 3, ay + 2, 1, 1, arr);
    }

    for (i, (kind, ammo)) in weapons.iter().enumerate() {
        let col = (i as i32) / total_rows;
        let row = (i as i32) % total_rows;
        if row < scroll || row >= scroll + MAX_ROWS { continue; }
        let vis_row = row - scroll;
        let cx = wx + 10 + col * cell_w;
        let cy = wy + 38 + vis_row * cell_h;
        let selected = i == cursor;

        if selected {
            buf.fill_rect(cx, cy, cell_w as u32 - 4, cell_h as u32 - 4, Bgra::new(28, 35, 70));
        }

        // ── Weapon icon (pixel art, 32×24 centered in cell) ──────────────────
        let icon_cx = cx + cell_w/2;
        let icon_cy = cy + 12;
        let icol = if selected { Bgra::new(255, 220, 80) } else { Bgra::new(160, 160, 190) };
        let dark = Bgra::new(20, 15, 8);
        let gray = Bgra::new(90, 90, 100);

        let hi   = Bgra::new(icol.r.saturating_add(40), icol.g.saturating_add(35), icol.b.saturating_add(25));
        let mid  = Bgra::new(icol.r.saturating_sub(20), icol.g.saturating_sub(15), icol.b.saturating_sub(10));
        let ghi  = Bgra::new(150, 155, 165); // barrel highlight
        match kind {
            WeaponKind::Bazooka => {
                // ── Bazooka: tube body + long barrel + sight ─────────────────
                // Sight post
                buf.fill_rect(icon_cx - 1, icon_cy - 12, 3, 4, dark);
                buf.fill_rect(icon_cx,     icon_cy - 11, 1, 3, ghi);
                // Receiver outline + body
                buf.fill_rect(icon_cx - 12, icon_cy - 7, 16, 12, dark);
                buf.fill_rect(icon_cx - 11, icon_cy - 6, 14, 10, icol);
                buf.fill_rect(icon_cx - 11, icon_cy - 6, 14, 2, hi);
                // Barrel outline + body (long, extends right)
                buf.fill_rect(icon_cx + 4,  icon_cy - 3, 28, 5, dark);
                buf.fill_rect(icon_cx + 5,  icon_cy - 2, 26, 3, gray);
                buf.fill_rect(icon_cx + 5,  icon_cy - 2, 26, 1, ghi);
                // Stock rear (simple block)
                buf.fill_rect(icon_cx - 16, icon_cy - 3,  6, 5, dark);
                buf.fill_rect(icon_cx - 15, icon_cy - 2,  4, 3, mid);
            }
            WeaponKind::Grenade => {
                // ── Grenade: oval body with segments, pin, ring ───────────────
                let gbody = Bgra::new(55, 120, 45);  // military green
                let ghi2  = Bgra::new(90, 170, 70);  // highlight
                let gdark = Bgra::new(25, 60, 20);    // shadow
                // Oval outline (dark border around the whole grenade)
                buf.fill_rect(icon_cx - 5,  icon_cy - 9,  10, 2, dark);
                buf.fill_rect(icon_cx - 8,  icon_cy - 7,  16, 2, dark);
                buf.fill_rect(icon_cx - 9,  icon_cy - 5,  18, 10, dark);
                buf.fill_rect(icon_cx - 8,  icon_cy + 5,  16, 2, dark);
                buf.fill_rect(icon_cx - 5,  icon_cy + 7,  10, 2, dark);
                // Oval body fill
                buf.fill_rect(icon_cx - 4,  icon_cy - 8,   8, 2, gbody);
                buf.fill_rect(icon_cx - 7,  icon_cy - 6,  14, 2, gbody);
                buf.fill_rect(icon_cx - 8,  icon_cy - 4,  16, 8, gbody);
                buf.fill_rect(icon_cx - 7,  icon_cy + 4,  14, 2, gbody);
                buf.fill_rect(icon_cx - 4,  icon_cy + 6,   8, 2, gbody);
                // Highlight on upper-left
                buf.fill_rect(icon_cx - 6,  icon_cy - 6,   5, 2, ghi2);
                buf.fill_rect(icon_cx - 7,  icon_cy - 4,   3, 3, ghi2);
                // Shadow on lower-right
                buf.fill_rect(icon_cx + 3,  icon_cy + 2,   4, 3, gdark);
                // Horizontal seam line
                buf.fill_rect(icon_cx - 8,  icon_cy - 1,  16, 2, gdark);
                // Vertical seam line
                buf.fill_rect(icon_cx - 1,  icon_cy - 8,   2, 16, gdark);
                // Collar ring at top
                buf.fill_rect(icon_cx - 3,  icon_cy - 10,  6, 3, gray);
                buf.fill_rect(icon_cx - 2,  icon_cy - 9,   4, 1, ghi);
                // Safety pin lever
                buf.fill_rect(icon_cx - 5,  icon_cy - 14,  10, 2, gray);
                buf.fill_rect(icon_cx - 5,  icon_cy - 14,   2, 5, gray);
                buf.fill_rect(icon_cx + 3,  icon_cy - 14,   2, 5, gray);
                buf.fill_rect(icon_cx - 1,  icon_cy - 13,   2, 3, dark);
            }
            WeaponKind::Shotgun => {
                // ── Shotgun: double barrel, chunky receiver, short stock ──────
                // Upper barrel
                buf.fill_rect(icon_cx + 2,  icon_cy - 6, 22, 4, dark);
                buf.fill_rect(icon_cx + 3,  icon_cy - 5, 20, 2, gray);
                buf.fill_rect(icon_cx + 3,  icon_cy - 5, 20, 1, ghi);
                // Lower barrel
                buf.fill_rect(icon_cx + 2,  icon_cy - 1, 22, 4, dark);
                buf.fill_rect(icon_cx + 3,  icon_cy,     20, 2, gray);
                buf.fill_rect(icon_cx + 3,  icon_cy,     20, 1, ghi);
                // Muzzle cap (dark end)
                buf.fill_rect(icon_cx + 23, icon_cy - 6,  2, 9, dark);
                // Receiver body
                buf.fill_rect(icon_cx - 8,  icon_cy - 8, 12, 13, dark);
                buf.fill_rect(icon_cx - 7,  icon_cy - 7, 10, 11, icol);
                buf.fill_rect(icon_cx - 7,  icon_cy - 7, 10,  2, hi);
                // Stock
                buf.fill_rect(icon_cx - 19, icon_cy - 5, 13, 8, dark);
                buf.fill_rect(icon_cx - 18, icon_cy - 4, 11, 6, mid);
                buf.fill_rect(icon_cx - 18, icon_cy - 4, 11, 1, icol);
                // Pump forend (below barrels, between receiver and muzzle)
                buf.fill_rect(icon_cx + 3,  icon_cy + 3, 14, 4, dark);
                buf.fill_rect(icon_cx + 4,  icon_cy + 4, 12, 2, mid);
            }
            WeaponKind::Tnt => {
                // Red stick with gray fuse coming out the top
                let red  = Bgra::new(190, 25, 15);
                let rhi  = Bgra::new(230, 60, 45);
                let rdark = Bgra::new(110, 12, 8);
                let fuse = Bgra::new(160, 160, 160);
                // Body outline (dark border)
                buf.fill_rect(icon_cx - 5, icon_cy - 10, 10, 18, dark);
                // Body fill
                buf.fill_rect(icon_cx - 4, icon_cy - 9,   8, 16, red);
                // Left highlight
                buf.fill_rect(icon_cx - 4, icon_cy - 9,   2, 16, rhi);
                // Right shadow
                buf.fill_rect(icon_cx + 2, icon_cy - 9,   2, 16, rdark);
                // Gray fuse: straight up then slight curve to the right
                buf.fill_rect(icon_cx,     icon_cy - 12,  2,  3, fuse); // base
                buf.fill_rect(icon_cx + 1, icon_cy - 15,  2,  3, fuse); // bend
                buf.fill_rect(icon_cx + 2, icon_cy - 18,  2,  3, fuse); // tip
            }
            WeaponKind::BananaBomb => {
                // Meteor Bomb: rocky gray sphere with curved purple arc stripe
                let shadow = Bgra::new(55,  58,  60);
                let body   = Bgra::new(145, 150, 152);
                let hilit  = Bgra::new(200, 205, 207);
                let bright = Bgra::new(225, 228, 230);
                let purp   = Bgra::new(185, 55,  215);
                let purpd  = Bgra::new(105, 25,  145);
                buf.fill_circle(icon_cx,       icon_cy,      10, shadow); // dark outline
                buf.fill_circle(icon_cx,       icon_cy,       9, body);   // gray body
                buf.fill_circle(icon_cx - 3,   icon_cy - 1,   7, purpd);  // dark purple arc
                buf.fill_circle(icon_cx - 3,   icon_cy - 1,   6, purp);   // bright purple arc
                buf.fill_circle(icon_cx + 1,   icon_cy + 1,   6, body);   // gray overdraw → crescent stripe
                buf.fill_circle(icon_cx + 2,   icon_cy,       5, hilit);  // right-side lighter (3D depth)
                buf.fill_circle(icon_cx + 4,   icon_cy - 3,   2, bright); // bright highlight spot
            }
            WeaponKind::Landmine => {
                // Green metal ball with red LED
                buf.fill_circle(icon_cx, icon_cy + 2, 9, Bgra::new(20, 60, 20));    // dark outline
                buf.fill_circle(icon_cx, icon_cy + 2, 7, Bgra::new(45, 110, 35));   // body
                buf.fill_circle(icon_cx - 2, icon_cy,  3, Bgra::new(70, 150, 55));  // highlight
                // Red LED on top
                buf.fill_rect(icon_cx - 1, icon_cy - 8, 3, 3, Bgra::new(220, 30, 30));
                buf.fill_rect(icon_cx,     icon_cy - 7, 1, 1, Bgra::new(255, 120, 120));
            }
            WeaponKind::Revolver => {
                // Pixel-art revolver: barrel (right), cylinder (center), grip (down-left)
                // Barrel
                buf.fill_rect(icon_cx - 2, icon_cy - 3, 20, 4, dark);
                buf.fill_rect(icon_cx - 1, icon_cy - 2, 18, 2, gray);
                buf.fill_rect(icon_cx - 1, icon_cy - 2, 18, 1, ghi);
                // Cylinder (round chamber)
                buf.fill_circle(icon_cx - 5, icon_cy, 5, dark);
                buf.fill_circle(icon_cx - 5, icon_cy, 4, icol);
                buf.fill_circle(icon_cx - 6, icon_cy - 1, 2, hi);
                // Grip (angled down-left)
                buf.fill_rect(icon_cx - 10, icon_cy + 3, 5, 9, dark);
                buf.fill_rect(icon_cx - 9,  icon_cy + 4, 3, 7, mid);
            }
            WeaponKind::NinjaRope => {
                // Grappling hook: horizontal shaft + curved claw at right end
                // Shaft
                buf.fill_rect(icon_cx - 10, icon_cy - 1, 16, 3, dark);
                buf.fill_rect(icon_cx -  9, icon_cy,     14, 1, icol);
                // Hook ring (small circle)
                buf.fill_circle(icon_cx + 7, icon_cy,     5, dark);
                buf.fill_circle(icon_cx + 7, icon_cy,     3, icol);
                buf.fill_circle(icon_cx + 7, icon_cy,     2, Bgra::new(0,0,0));  // hole
                // Claw prong (bottom)
                buf.fill_rect(icon_cx + 4, icon_cy + 1,  2, 6, dark);
                buf.fill_rect(icon_cx + 5, icon_cy + 2,  1, 4, icol);
                // Rope tail (left)
                buf.fill_rect(icon_cx - 16, icon_cy - 1, 8, 2, Bgra::new(160, 170, 120));
            }
            WeaponKind::BaseballBat => {
                let tan    = Bgra::new(205, 165, 100);
                let tan_hi = Bgra::new(235, 200, 145);
                let tan_dk = Bgra::new(145, 105,  50);
                // Barrel (wide top)
                buf.fill_rect(icon_cx - 5, icon_cy - 11, 10, 8, tan_dk);
                buf.fill_rect(icon_cx - 4, icon_cy - 10,  8, 6, tan);
                buf.fill_rect(icon_cx - 4, icon_cy - 10,  8, 2, tan_hi);
                // Top cap
                buf.fill_rect(icon_cx - 3, icon_cy - 12,  6, 2, tan_dk);
                // Taper
                buf.fill_rect(icon_cx - 3, icon_cy -  3,  6, 4, tan_dk);
                buf.fill_rect(icon_cx - 2, icon_cy -  2,  4, 2, tan);
                // Handle (thin)
                buf.fill_rect(icon_cx - 1, icon_cy +  1,  3, 8, tan_dk);
                buf.fill_rect(icon_cx,     icon_cy +  2,  1, 6, tan);
                // Knob (bottom)
                buf.fill_rect(icon_cx - 2, icon_cy +  9,  5, 3, tan_dk);
                buf.fill_rect(icon_cx - 1, icon_cy + 10,  3, 2, tan);
            }
            WeaponKind::Blasthive => {
                // Beehive: stacked ring skep shape, amber/brown
                let hdk = Bgra::new(70, 45, 12);
                let hmd = Bgra::new(165, 110, 35);
                let hlt = Bgra::new(215, 170, 70);
                let hhi = Bgra::new(250, 220, 110);
                let bee = Bgra::new(255, 210, 0);
                // Dome
                buf.fill_circle(icon_cx, icon_cy - 8, 5, hdk);
                buf.fill_circle(icon_cx, icon_cy - 8, 4, hlt);
                buf.set_pixel(icon_cx - 1, icon_cy - 10, hhi);
                // Ring layers
                buf.fill_rect(icon_cx - 5, icon_cy - 4, 11, 3, hdk);
                buf.fill_rect(icon_cx - 4, icon_cy - 3,  9, 2, hmd);
                buf.fill_rect(icon_cx - 7, icon_cy - 1, 15, 3, hdk);
                buf.fill_rect(icon_cx - 6, icon_cy,     13, 2, hmd);
                buf.fill_rect(icon_cx - 6, icon_cy,      6, 1, hlt);
                buf.fill_rect(icon_cx - 7, icon_cy + 2, 15, 3, hdk);
                buf.fill_rect(icon_cx - 6, icon_cy + 3, 13, 2, hmd);
                buf.fill_rect(icon_cx - 5, icon_cy + 5, 11, 3, hdk);
                buf.fill_rect(icon_cx - 4, icon_cy + 6,  9, 2, hmd);
                buf.fill_rect(icon_cx - 4, icon_cy + 6,  5, 1, hlt);
                // Base board
                buf.fill_rect(icon_cx - 7, icon_cy + 8, 15, 2, hdk);
                buf.fill_rect(icon_cx - 6, icon_cy + 9, 13, 1, hmd);
                // Entry hole
                buf.fill_rect(icon_cx - 1, icon_cy + 7,  3, 1, hdk);
                // Tiny bee buzzing near hive
                buf.set_pixel(icon_cx + 9, icon_cy - 4, bee);
                buf.set_pixel(icon_cx + 9, icon_cy - 5, Bgra::new(0, 0, 0));
                buf.set_pixel(icon_cx + 9, icon_cy - 6, bee);
            }
            WeaponKind::BlackHoleBomb => {
                let purp  = Bgra::new(160, 0, 220);
                let purpd = Bgra::new(60,  0,  90);
                let void  = Bgra::new(0,   0,   0);
                let glow  = Bgra::new(200, 80, 255);
                buf.fill_circle(icon_cx, icon_cy, 9, purpd);
                buf.fill_circle(icon_cx, icon_cy, 7, purp);
                buf.fill_circle(icon_cx, icon_cy, 4, void);
                buf.set_pixel(icon_cx,     icon_cy - 8, glow);
                buf.set_pixel(icon_cx,     icon_cy + 8, glow);
                buf.set_pixel(icon_cx - 8, icon_cy,     glow);
                buf.set_pixel(icon_cx + 8, icon_cy,     glow);
            }
            WeaponKind::PlasmaTorch => {
                // Plasma torch: cylindrical gas tank (left) + hose + nozzle + flame (right)
                let steel    = Bgra::new(140, 150, 165);
                let steel_hi = Bgra::new(190, 205, 220);
                let steel_dk = Bgra::new(65,  72,  85);
                let fl_outer = Bgra::new(255, 80,  20);  // dark orange outer flame
                let fl_mid   = Bgra::new(255, 160, 30);  // mid orange
                let fl_hi    = Bgra::new(255, 235, 90);  // bright yellow centre
                // Gas tank body (left, cylinder silhouette)
                buf.fill_rect(icon_cx - 16, icon_cy - 8, 11, 17, steel_dk);
                buf.fill_rect(icon_cx - 15, icon_cy - 7,  9, 15, steel);
                buf.fill_rect(icon_cx - 15, icon_cy - 7,  9,  2, steel_hi);
                buf.fill_rect(icon_cx - 15, icon_cy + 6,  9,  1, steel_dk);
                // Tank top cap
                buf.fill_rect(icon_cx - 13, icon_cy - 10, 5, 3, steel_dk);
                buf.fill_rect(icon_cx - 12, icon_cy -  9, 3, 2, steel);
                // Hose (thin horizontal tube connecting tank to nozzle)
                buf.fill_rect(icon_cx - 5,  icon_cy - 2, 9, 4, steel_dk);
                buf.fill_rect(icon_cx - 4,  icon_cy - 1, 7, 2, steel);
                // Nozzle (slightly wider block at right end of hose)
                buf.fill_rect(icon_cx + 4,  icon_cy - 4, 5, 8, steel_dk);
                buf.fill_rect(icon_cx + 5,  icon_cy - 3, 3, 6, steel);
                // Flame — layered teardrop pointing right
                buf.fill_rect(icon_cx + 9,  icon_cy - 3, 8, 6, fl_outer);
                buf.fill_rect(icon_cx + 10, icon_cy - 4, 7, 8, fl_outer);
                buf.fill_rect(icon_cx + 11, icon_cy - 2, 6, 5, fl_mid);
                buf.fill_rect(icon_cx + 12, icon_cy - 1, 4, 3, fl_hi);
                buf.set_pixel(icon_cx + 13, icon_cy,     fl_hi);
            }
            WeaponKind::Jackhammer => {
                // Pneumatic jackhammer: T-grip handles up top, motor body, chisel bit down.
                let steel    = if selected { Bgra::new(175, 185, 200) } else { Bgra::new(105, 113, 126) };
                let steel_hi = Bgra::new(210, 222, 236);
                let steel_dk = Bgra::new(55,  62,  74);
                let bit      = if selected { Bgra::new(150, 158, 170) } else { Bgra::new(95, 102, 114) };
                let handle   = if selected { Bgra::new(230, 90, 40)   } else { Bgra::new(150, 60, 30) };
                // T-grip handle bar across the top
                buf.fill_rect(icon_cx - 9, icon_cy - 13, 18, 3, handle);
                // Neck from handle down to body
                buf.fill_rect(icon_cx - 2, icon_cy - 11, 4, 4, steel_dk);
                // Motor body (main cylinder)
                buf.fill_rect(icon_cx - 6, icon_cy - 7, 12, 12, steel_dk);
                buf.fill_rect(icon_cx - 5, icon_cy - 6, 10, 10, steel);
                buf.fill_rect(icon_cx - 5, icon_cy - 6, 10,  2, steel_hi);
                // Chisel shaft tapering to the drill bit
                buf.fill_rect(icon_cx - 3, icon_cy + 5, 6, 5, steel_dk);
                buf.fill_rect(icon_cx - 2, icon_cy + 5, 4, 4, bit);
                buf.fill_rect(icon_cx - 1, icon_cy + 9, 2, 4, bit);
                buf.set_pixel(icon_cx, icon_cy + 13, steel_hi);
            }
            WeaponKind::Garcia => {
                // Lightning bolt icon — red left half, blue right half, white bolt centre
                let red  = if selected { Bgra::new(255, 60, 60)  } else { Bgra::new(160, 40, 40)  };
                let blue = if selected { Bgra::new(80, 120, 255) } else { Bgra::new(50,  80, 160) };
                let wht  = Bgra::new(255, 255, 255);
                // Red left lobe
                buf.fill_rect(icon_cx - 14, icon_cy - 8, 11, 16, red);
                // Blue right lobe
                buf.fill_rect(icon_cx + 3, icon_cy - 8, 11, 16, blue);
                // White lightning bolt (3 segments, 3px wide)
                buf.fill_rect(icon_cx - 2, icon_cy - 8,  5, 5, wht);
                buf.fill_rect(icon_cx - 4, icon_cy - 3,  5, 5, wht);
                buf.fill_rect(icon_cx - 1, icon_cy + 2,  5, 6, wht);
            }
            WeaponKind::Jumpbot => {
                // Clockwork jumpbot head: metal block head, glowing eyes, mouth, antenna.
                let steel    = if selected { Bgra::new(170, 180, 195) } else { Bgra::new(100, 108, 120) };
                let steel_dk = Bgra::new(50, 56, 65);
                let eye      = if selected { Bgra::new(80, 220, 255) } else { Bgra::new(40, 130, 160) };
                let mouth_c  = Bgra::new(20, 22, 26);
                buf.fill_rect(icon_cx - 9, icon_cy - 8, 18, 16, steel_dk);
                buf.fill_rect(icon_cx - 8, icon_cy - 7, 16, 14, steel);
                buf.fill_rect(icon_cx - 5, icon_cy - 2, 4, 4, eye);
                buf.fill_rect(icon_cx + 1, icon_cy - 2, 4, 4, eye);
                buf.fill_rect(icon_cx - 4, icon_cy + 4, 8, 2, mouth_c);
                buf.fill_rect(icon_cx - 4, icon_cy + 4, 1, 2, steel_dk);
                buf.fill_rect(icon_cx + 3, icon_cy + 4, 1, 2, steel_dk);
                buf.fill_rect(icon_cx - 1, icon_cy - 14, 2, 6, steel_dk);
                buf.fill_circle(icon_cx, icon_cy - 15, 2, eye);
            }
            WeaponKind::SuperSheep => {
                // Jumpbot head with a red jetpack and flame behind it.
                let steel    = if selected { Bgra::new(170, 180, 195) } else { Bgra::new(100, 108, 120) };
                let steel_dk = Bgra::new(50, 56, 65);
                let eye      = if selected { Bgra::new(80, 220, 255) } else { Bgra::new(40, 130, 160) };
                let red      = if selected { Bgra::new(230, 70, 47) } else { Bgra::new(150, 50, 36) };
                let fl       = if selected { Bgra::new(255, 200, 60) } else { Bgra::new(170, 130, 40) };
                buf.fill_rect(icon_cx - 17, icon_cy - 8, 6, 12, steel_dk);
                buf.fill_rect(icon_cx - 16, icon_cy - 7, 4, 10, red);
                buf.fill_rect(icon_cx - 16, icon_cy + 4, 4, 5, fl);
                buf.fill_rect(icon_cx - 9, icon_cy - 8, 18, 16, steel_dk);
                buf.fill_rect(icon_cx - 8, icon_cy - 7, 16, 14, steel);
                buf.fill_rect(icon_cx - 5, icon_cy - 2, 4, 4, eye);
                buf.fill_rect(icon_cx + 1, icon_cy - 2, 4, 4, eye);
                buf.fill_rect(icon_cx - 1, icon_cy - 14, 2, 6, steel_dk);
                buf.fill_circle(icon_cx, icon_cy - 15, 2, red);
            }
            WeaponKind::AirStrike => {
                // Side-view plane: fuselage, cockpit, wings, tail
                let steel = if selected { Bgra::new(180, 195, 215) } else { Bgra::new(110, 120, 135) };
                let wing  = if selected { Bgra::new(140, 155, 175) } else { Bgra::new(85,  95, 110)  };
                let cock  = if selected { Bgra::new(80, 160, 240)  } else { Bgra::new(50, 100, 160)  };
                let exhaust = Bgra::new(255, 160, 50);
                // Fuselage (horizontal body)
                buf.fill_rect(icon_cx - 16, icon_cy - 2, 32, 5, dark);
                buf.fill_rect(icon_cx - 15, icon_cy - 1, 30, 3, steel);
                // Nose cone (tapered front right)
                buf.fill_rect(icon_cx + 15, icon_cy,     2, 1, steel);
                // Cockpit bump (top, slightly left of center)
                buf.fill_rect(icon_cx - 4, icon_cy - 6, 9, 5, dark);
                buf.fill_rect(icon_cx - 3, icon_cy - 5, 7, 4, cock);
                // Main wings (wide, below fuselage center)
                buf.fill_rect(icon_cx - 10, icon_cy + 3, 20, 4, dark);
                buf.fill_rect(icon_cx - 9,  icon_cy + 4, 18, 2, wing);
                // Tail fin (vertical, rear left)
                buf.fill_rect(icon_cx - 16, icon_cy - 7, 4, 6, dark);
                buf.fill_rect(icon_cx - 15, icon_cy - 6, 2, 4, wing);
                // Engine exhaust glow
                buf.fill_rect(icon_cx - 17, icon_cy,     2, 1, exhaust);
            }
            WeaponKind::HolyHandGrenade => {
                // ── Sacred Ordnance: large golden oval with gold cross on top ──
                let gbody  = Bgra::new(210, 155, 30);
                let ghi2   = Bgra::new(255, 230, 100);
                let gdark  = Bgra::new(140, 95, 10);
                let gold   = Bgra::new(255, 215, 45);
                let goldhi = Bgra::new(255, 248, 160);
                // Oval outline (~1.35× bigger)
                buf.fill_rect(icon_cx - 7,  icon_cy - 7,  14, 3, dark);
                buf.fill_rect(icon_cx - 11, icon_cy - 4,  22, 3, dark);
                buf.fill_rect(icon_cx - 12, icon_cy - 1,  24, 11, dark);
                buf.fill_rect(icon_cx - 11, icon_cy + 10, 22, 3, dark);
                buf.fill_rect(icon_cx - 7,  icon_cy + 13, 14, 3, dark);
                // Gold body fill
                buf.fill_rect(icon_cx - 5,  icon_cy - 6,  10, 3, gbody);
                buf.fill_rect(icon_cx - 9,  icon_cy - 3,  18, 3, gbody);
                buf.fill_rect(icon_cx - 10, icon_cy,       20, 9, gbody);
                buf.fill_rect(icon_cx - 9,  icon_cy + 9,  18, 3, gbody);
                buf.fill_rect(icon_cx - 5,  icon_cy + 12, 10, 3, gbody);
                // Highlight
                buf.fill_rect(icon_cx - 8,  icon_cy - 3,   7, 3, ghi2);
                buf.fill_rect(icon_cx - 9,  icon_cy,        4, 4, ghi2);
                // Horizontal seam
                buf.fill_rect(icon_cx - 10, icon_cy + 4,  20, 3, gdark);
                // Gold cross (vertical 4×14, horizontal 15×4)
                buf.fill_rect(icon_cx - 2,  icon_cy - 21,  4, 14, gold);
                buf.fill_rect(icon_cx - 7,  icon_cy - 15, 15,  4, gold);
                // Cross highlight
                buf.fill_rect(icon_cx - 1,  icon_cy - 20,  2, 11, goldhi);
                buf.fill_rect(icon_cx - 6,  icon_cy - 14, 12,  2, goldhi);
                // Collar ring connecting cross to body
                buf.fill_rect(icon_cx - 4,  icon_cy - 8,   8, 4, gray);
                buf.fill_rect(icon_cx - 3,  icon_cy - 7,   6, 2, icol);
            }
            WeaponKind::Minigun => {
                // Pixel-art minigun: clustered triple barrels + long receiver + grip
                let mdk = Bgra::new(60, 60, 60);
                let mmd = Bgra::new(130, 130, 140);
                let mhi = Bgra::new(200, 205, 215);
                // Long barrel housing
                buf.fill_rect(icon_cx - 2, icon_cy - 3, 18, 7, mdk);
                buf.fill_rect(icon_cx - 1, icon_cy - 2, 16, 5, mmd);
                buf.fill_rect(icon_cx - 1, icon_cy - 2, 16, 1, mhi);
                // Three barrel tips at front (clustered circles)
                buf.fill_circle(icon_cx + 16, icon_cy - 4, 2, mdk);
                buf.fill_circle(icon_cx + 16, icon_cy - 4, 1, mmd);
                buf.fill_circle(icon_cx + 16, icon_cy,     2, mdk);
                buf.fill_circle(icon_cx + 16, icon_cy,     1, mmd);
                buf.fill_circle(icon_cx + 16, icon_cy + 4, 2, mdk);
                buf.fill_circle(icon_cx + 16, icon_cy + 4, 1, mmd);
                // Receiver box (left)
                buf.fill_rect(icon_cx - 8, icon_cy - 5, 8, 11, mdk);
                buf.fill_rect(icon_cx - 7, icon_cy - 4, 6,  9, mmd);
                buf.fill_rect(icon_cx - 7, icon_cy - 4, 6,  1, mhi);
                // Grip
                buf.fill_rect(icon_cx - 6, icon_cy + 5, 4, 8, mdk);
                buf.fill_rect(icon_cx - 5, icon_cy + 6, 2, 6, mmd);
            }
            WeaponKind::MolotovCocktail => {
                // Pixel-art molotov: amber bottle with white rag and flame
                let gbody  = if selected { Bgra::new(220, 145, 40) } else { Bgra::new(150, 95,  25) };
                let gdark  = if selected { Bgra::new(140,  80, 15) } else { Bgra::new( 90, 50,  10) };
                let ghi    = if selected { Bgra::new(255, 215, 100)} else { Bgra::new(200, 150, 60) };
                let gneck  = if selected { Bgra::new(110,  60, 10) } else { Bgra::new( 70, 38,   8) };
                let rag    = Bgra::new(230, 225, 210);
                let flame1 = Bgra::new(255, 140,  20);
                let flame2 = Bgra::new(255, 230,  60);
                // Bottle body (rounded-rect, 14px tall, 10px wide)
                buf.fill_rect(icon_cx - 5, icon_cy - 7,  10, 14, gdark); // outline
                buf.fill_rect(icon_cx - 4, icon_cy - 6,   8, 12, gbody); // fill
                buf.fill_rect(icon_cx - 4, icon_cy - 6,   8,  3, ghi);   // top highlight
                buf.fill_rect(icon_cx - 4, icon_cy - 6,   1, 12, gdark); // left shading
                // Bottle neck (narrow, above body)
                buf.fill_rect(icon_cx - 2, icon_cy - 12,  4,  6, gdark);
                buf.fill_rect(icon_cx - 1, icon_cy - 11,  2,  4, gneck);
                // Rag (hanging out of neck, white strips)
                buf.fill_rect(icon_cx - 1, icon_cy - 15,  2,  4, rag);
                buf.set_pixel(icon_cx - 2, icon_cy - 14, rag);
                buf.set_pixel(icon_cx + 1, icon_cy - 13, rag);
                // Flame at rag tip
                buf.set_pixel(icon_cx,     icon_cy - 16, flame1);
                buf.set_pixel(icon_cx - 1, icon_cy - 17, flame1);
                buf.set_pixel(icon_cx + 1, icon_cy - 17, flame1);
                buf.set_pixel(icon_cx,     icon_cy - 18, flame2);
            }
            WeaponKind::Uzi => {
                // Pixel-art Mac-10: boxy receiver, stubby flush barrel, long box mag
                let mdk = Bgra::new(50, 50, 55);
                let mmd = Bgra::new(110, 112, 120);
                let mhi = Bgra::new(185, 190, 200);
                // Boxy receiver (wide and squat, Mac-10 hallmark)
                buf.fill_rect(icon_cx - 6, icon_cy - 4, 14, 9, mdk);
                buf.fill_rect(icon_cx - 5, icon_cy - 3, 12, 7, mmd);
                buf.fill_rect(icon_cx - 5, icon_cy - 3, 12, 1, mhi);
                // Short stubby barrel (barely extends past receiver)
                buf.fill_rect(icon_cx + 8,  icon_cy - 1, 5, 3, mdk);
                buf.fill_rect(icon_cx + 9,  icon_cy,     4, 1, mmd);
                // Wire stock stub on back (folded, just a nub)
                buf.fill_rect(icon_cx - 9, icon_cy - 2, 3, 5, mdk);
                buf.fill_rect(icon_cx - 8, icon_cy - 1, 1, 3, mmd);
                // Long box magazine feeding from bottom center
                buf.fill_rect(icon_cx - 3, icon_cy + 5,  6, 9, mdk);
                buf.fill_rect(icon_cx - 2, icon_cy + 6,  4, 7, mmd);
                buf.fill_rect(icon_cx - 2, icon_cy + 6,  4, 1, mhi);
            }
            WeaponKind::HomingMissile => {
                // Pixel-art missile: cylindrical body + pointed nose + 4 fins
                let mbody = if selected { Bgra::new(190, 195, 200) } else { Bgra::new(120, 125, 130) };
                let mnose = if selected { Bgra::new(220, 80, 60)   } else { Bgra::new(140, 50, 40)   };
                let mfin  = if selected { Bgra::new(160, 170, 180) } else { Bgra::new(90, 95, 100)   };
                let mjet  = Bgra::new(255, 160, 50);
                // Body (horizontal, pointing right)
                buf.fill_rect(icon_cx - 10, icon_cy - 2, 20, 5, dark);
                buf.fill_rect(icon_cx - 9,  icon_cy - 1, 18, 3, mbody);
                buf.fill_rect(icon_cx - 9,  icon_cy - 1, 18, 1, if selected { Bgra::new(220, 225, 230) } else { Bgra::new(150, 155, 160) });
                // Pointed nose cone (right side)
                buf.fill_rect(icon_cx + 9,  icon_cy,       3, 1, mnose);
                buf.fill_rect(icon_cx + 11, icon_cy,       1, 1, mnose);
                // Warhead band (red stripe)
                buf.fill_rect(icon_cx + 4,  icon_cy - 1,   4, 3, mnose);
                // Top and bottom fins (near nose)
                buf.fill_rect(icon_cx + 2,  icon_cy - 5,   5, 4, dark);
                buf.fill_rect(icon_cx + 3,  icon_cy - 4,   4, 3, mfin);
                buf.fill_rect(icon_cx + 2,  icon_cy + 2,   5, 4, dark);
                buf.fill_rect(icon_cx + 3,  icon_cy + 2,   4, 3, mfin);
                // Rear fins (tail)
                buf.fill_rect(icon_cx - 10, icon_cy - 5,   3, 4, dark);
                buf.fill_rect(icon_cx - 9,  icon_cy - 4,   2, 3, mfin);
                buf.fill_rect(icon_cx - 10, icon_cy + 2,   3, 4, dark);
                buf.fill_rect(icon_cx - 9,  icon_cy + 2,   2, 3, mfin);
                // Engine exhaust glow
                buf.fill_rect(icon_cx - 12, icon_cy,       3, 1, mjet);
            }
            WeaponKind::Pistol => {
                // Pixel-art semi-auto pistol: slide body, short barrel, angled grip
                // Slide (top block, runs most of the width)
                buf.fill_rect(icon_cx - 4, icon_cy - 6, 18, 6, dark);
                buf.fill_rect(icon_cx - 3, icon_cy - 5, 16, 4, gray);
                buf.fill_rect(icon_cx - 3, icon_cy - 5, 16, 1, ghi);
                // Barrel tip (protrudes slightly right)
                buf.fill_rect(icon_cx + 14, icon_cy - 4, 4, 3, dark);
                buf.fill_rect(icon_cx + 15, icon_cy - 3, 3, 1, ghi);
                // Frame / receiver (slightly wider than grip, below slide)
                buf.fill_rect(icon_cx - 4, icon_cy, 14, 5, dark);
                buf.fill_rect(icon_cx - 3, icon_cy + 1, 12, 3, icol);
                buf.fill_rect(icon_cx - 3, icon_cy + 1, 12, 1, hi);
                // Trigger guard (open loop below frame)
                buf.fill_rect(icon_cx + 1, icon_cy + 4, 6, 2, dark);
                buf.fill_rect(icon_cx,     icon_cy + 5, 1, 4, dark);
                buf.fill_rect(icon_cx + 7, icon_cy + 5, 1, 4, dark);
                // Grip (angled down-left from frame)
                buf.fill_rect(icon_cx - 4, icon_cy + 4, 5, 9, dark);
                buf.fill_rect(icon_cx - 3, icon_cy + 5, 3, 7, mid);
            }
            WeaponKind::ClusterBomb => {
                // Red grenade body with tapered/pointed top (W:A cluster bomb look)
                let cbody = Bgra::new(40, 40, 180);  // red body
                let chi   = Bgra::new(90, 80, 220);  // highlight
                let cdark = Bgra::new(15, 15, 80);   // shadow
                let cband = Bgra::new(30, 160, 220); // orange band (same as projectile)
                // Oval body (slightly smaller than grenade)
                buf.fill_rect(icon_cx - 4,  icon_cy - 6,   8, 2, dark);
                buf.fill_rect(icon_cx - 7,  icon_cy - 4,  14, 2, dark);
                buf.fill_rect(icon_cx - 8,  icon_cy - 2,  16, 7, dark);
                buf.fill_rect(icon_cx - 7,  icon_cy + 5,  14, 2, dark);
                buf.fill_rect(icon_cx - 4,  icon_cy + 7,   8, 2, dark);
                buf.fill_rect(icon_cx - 3,  icon_cy - 5,   6, 2, cbody);
                buf.fill_rect(icon_cx - 6,  icon_cy - 3,  12, 2, cbody);
                buf.fill_rect(icon_cx - 7,  icon_cy - 1,  14, 7, cbody);
                buf.fill_rect(icon_cx - 6,  icon_cy + 6,  12, 2, cbody);
                buf.fill_rect(icon_cx - 3,  icon_cy + 8,   6, 1, cbody);
                // Highlight top-left
                buf.fill_rect(icon_cx - 5,  icon_cy - 3,   4, 2, chi);
                buf.fill_rect(icon_cx - 6,  icon_cy - 1,   3, 3, chi);
                // Shadow lower-right
                buf.fill_rect(icon_cx + 3,  icon_cy + 3,   3, 3, cdark);
                // Orange equator band
                buf.fill_rect(icon_cx - 7,  icon_cy,      14, 2, cband);
                // Collar at top
                buf.fill_rect(icon_cx - 3,  icon_cy - 7,   6, 3, gray);
                buf.fill_rect(icon_cx - 2,  icon_cy - 6,   4, 1, ghi);
                // Tapered top (pyramid / pointed cap)
                buf.fill_rect(icon_cx - 2,  icon_cy - 10,  4, 3, dark);
                buf.fill_rect(icon_cx - 1,  icon_cy - 9,   2, 2, gray);
                buf.fill_rect(icon_cx,      icon_cy - 12,  1, 2, gray);
                buf.fill_rect(icon_cx - 1,  icon_cy - 12,  1, 1, dark);
                buf.fill_rect(icon_cx + 1,  icon_cy - 12,  1, 1, dark);
                // Pin lever (same as grenade)
                buf.fill_rect(icon_cx - 5,  icon_cy - 16,  10, 2, gray);
                buf.fill_rect(icon_cx - 5,  icon_cy - 16,   2, 4, gray);
                buf.fill_rect(icon_cx + 3,  icon_cy - 16,   2, 4, gray);
            }
            _ => {
                buf.fill_rect(icon_cx - 6, icon_cy - 4, 12, 8, dark);
                buf.fill_rect(icon_cx - 5, icon_cy - 3, 10, 6, icol);
            }
        }

        // Homing missile lock overlay: locked until 2 full turn cycles
        let hm_unlock = 2 * num_teams as u32;
        if *kind == WeaponKind::HomingMissile && turn_number < hm_unlock {
            let team_count = (num_teams as u32).max(1);
            let rotations_left = 2u32.saturating_sub(turn_number / team_count);
            let lk  = Bgra::new(180, 180, 60);
            let lkd = Bgra::new(100, 100, 30);
            buf.fill_rect(icon_cx - 5, icon_cy - 12,  3,  8, lk);
            buf.fill_rect(icon_cx + 2, icon_cy - 12,  3,  8, lk);
            buf.fill_rect(icon_cx - 5, icon_cy - 14, 10,  3, lk);
            buf.fill_rect(icon_cx - 4, icon_cy - 13,  8,  2, lkd);
            buf.fill_rect(icon_cx - 7, icon_cy - 5,  14, 10, lk);
            buf.fill_rect(icon_cx - 6, icon_cy - 4,  12,  8, lkd);
            buf.fill_rect(icon_cx - 1, icon_cy - 3,   2,  2, Bgra::new(30, 30, 20));
            buf.fill_rect(icon_cx - 1, icon_cy - 1,   2,  4, Bgra::new(30, 30, 20));
            let cdown = format!("T-{}", rotations_left);
            draw_str(buf, &cdown, cx + cell_w - str_width(&cdown) - 6, cy + cell_h - 18, Bgra::new(220, 200, 60));
        }

        // Airstrike lock overlay: locked until 7 full turn cycles
        let air_unlock = 7 * num_teams as u32;
        if *kind == WeaponKind::AirStrike && turn_number < air_unlock {
            let team_count = (num_teams as u32).max(1);
            let rotations_left = 7u32.saturating_sub(turn_number / team_count);
            let lk  = Bgra::new(180, 180, 60);
            let lkd = Bgra::new(100, 100, 30);
            buf.fill_rect(icon_cx - 5, icon_cy - 12,  3,  8, lk);
            buf.fill_rect(icon_cx + 2, icon_cy - 12,  3,  8, lk);
            buf.fill_rect(icon_cx - 5, icon_cy - 14, 10,  3, lk);
            buf.fill_rect(icon_cx - 4, icon_cy - 13,  8,  2, lkd);
            buf.fill_rect(icon_cx - 7, icon_cy - 5,  14, 10, lk);
            buf.fill_rect(icon_cx - 6, icon_cy - 4,  12,  8, lkd);
            buf.fill_rect(icon_cx - 1, icon_cy - 3,   2,  2, Bgra::new(30, 30, 20));
            buf.fill_rect(icon_cx - 1, icon_cy - 1,   2,  4, Bgra::new(30, 30, 20));
            let cdown = format!("T-{}", rotations_left);
            draw_str(buf, &cdown, cx + cell_w - str_width(&cdown) - 6, cy + cell_h - 18, Bgra::new(220, 200, 60));
        }

        // Ammo counter: pixel-art ∞ for infinite, "xN" for limited
        match ammo {
            None => {
                // Hand-drawn ∞: two 3×4 loops, total 8×4 px
                let col = if selected { Bgra::new(140, 200, 255) } else { Bgra::new(70, 110, 160) };
                let bx = cx + 4;
                let by = icon_cy - 6;
                buf.fill_rect(bx+1, by,   2, 1, col); // top-left arc
                buf.fill_rect(bx+5, by,   2, 1, col); // top-right arc
                buf.fill_rect(bx,   by+1, 1, 2, col); // left edge
                buf.fill_rect(bx+3, by+1, 2, 2, col); // centre bridge
                buf.fill_rect(bx+7, by+1, 1, 2, col); // right edge
                buf.fill_rect(bx+1, by+3, 2, 1, col); // bottom-left arc
                buf.fill_rect(bx+5, by+3, 2, 1, col); // bottom-right arc
            }
            Some(n) => {
                let ac = format!("x{}", n);
                let col = if selected { Bgra::new(220, 220, 120) } else { Bgra::new(140, 140, 80) };
                draw_str(buf, &ac, cx + 4, icon_cy - 4, col);
            }
        }

        // Weapon name
        let name = match kind {
            WeaponKind::Bazooka     => "BAZOOKA",
            WeaponKind::Grenade     => "GRENADE",
            WeaponKind::Shotgun     => "SHOTGUN",
            WeaponKind::ClusterBomb => "CLUMP BOMB",
            WeaponKind::Landmine    => "MINE",
            WeaponKind::Tnt         => "TNT",
            WeaponKind::BananaBomb  => "BANANA BOMB",
            WeaponKind::Revolver    => "REVOLVER",
            WeaponKind::NinjaRope    => "GRAPPLE",
            WeaponKind::BaseballBat  => "BAT",
            WeaponKind::Blasthive      => "BLASTHIVE",
            WeaponKind::BlackHoleBomb  => "BLACK HOLE",
            WeaponKind::PlasmaTorch    => "TORCH",
            WeaponKind::Jackhammer     => "JACKHAMMER",
            WeaponKind::Garcia         => "HAND OF JERRY",
            WeaponKind::AirStrike      => "AIR STRIKE",
            WeaponKind::HolyHandGrenade => "SACRED ORD.",
            WeaponKind::Minigun         => "MINIGUN",
            WeaponKind::Uzi             => "MAC-10",
            WeaponKind::MolotovCocktail => "MOLOTOV",
            WeaponKind::HomingMissile   => "HOMING MSL.",
            WeaponKind::Pistol          => "PISTOL",
            WeaponKind::Jumpbot           => "JUMPBOT",
            WeaponKind::SuperSheep        => "SUPER BOT",
            _                          => "WEAPON",
        };
        let nc = if selected { Bgra::new(255, 220, 50) } else { Bgra::new(150, 150, 180) };
        if *kind == WeaponKind::HolyHandGrenade {
            // Fixed 3-second fuse, no adjustment
            draw_str(buf, name, cx + cell_w/2 - str_width(name)/2, cy + cell_h - 26, nc);
            let fuse_col = if selected { Bgra::new(255, 210, 40) } else { Bgra::new(140, 120, 40) };
            draw_str(buf, "3s FIXED", cx + cell_w/2 - str_width("3s FIXED")/2, cy + cell_h - 15, fuse_col);
        } else if *kind == WeaponKind::Grenade || *kind == WeaponKind::BananaBomb {
            // Fuse weapons: name + fuse selector at bottom
            draw_str(buf, name, cx + cell_w/2 - str_width(name)/2, cy + cell_h - 26, nc);
            let fuse_secs = fuse_ticks / 30;
            let fuse_str = format!("FUSE {}s", fuse_secs);
            let fuse_col = if selected { Bgra::new(80, 220, 255) } else { Bgra::new(60, 120, 160) };
            draw_str(buf, &fuse_str, cx + cell_w/2 - str_width(&fuse_str)/2, cy + cell_h - 15, fuse_col);
            if selected {
                draw_str(buf, "L1/R1 FUSE", cx + cell_w/2 - str_width("L1/R1 FUSE")/2, cy + cell_h - 4, Bgra::new(80, 80, 115));
            }
        } else {
            draw_str(buf, name, cx + cell_w/2 - str_width(name)/2, cy + cell_h - 18, nc);
        }
    }
    draw_str(buf, "A=SELECT  B=CANCEL", wx + 10, wy + win_h as i32 - 16, Bgra::new(60, 60, 90));
}

// ── Render ────────────────────────────────────────────────────────────────────

pub fn render(game: &GameState, buf: &mut WorldBuffer, cam: &Camera, lstate: &mut LoopState) {
    render_my_team(game, buf, cam, lstate, None);
}

/// Like render() but filters crate-collection messages to only show those belonging
/// to `my_team` — used in live mode so opponents don't see what you collected.
pub fn render_live(game: &GameState, buf: &mut WorldBuffer, cam: &Camera, lstate: &mut LoopState, my_team: usize) {
    render_my_team(game, buf, cam, lstate, Some(my_team));
}

fn render_my_team(game: &GameState, buf: &mut WorldBuffer, cam: &Camera, lstate: &mut LoopState, my_team: Option<usize>) {
    let cam_x = cam.left_edge() as u32;
    let cam_y = cam.top_edge() as u32;
    let sw    = crate::world::SCREEN_W as i32;
    let sh    = crate::world::SCREEN_H as i32;

    // Plasma-torch burn sound: plays only while the torch is active, stops on
    // deactivation/early release. Driven here so it covers every mode (local/TAT
    // use the simulated plasma_torch; live reconstructs it from networked torch_dir).
    crate::audio::update_torch(game.plasma_torch.is_some());

    // Per-section pixel-write profiling (TEST mode overlay, see section 9b/9d
    // below). `mark!` records how many pixels were written since the previous
    // mark and resets the running total.
    let mut pixel_stats: Vec<(&'static str, u64, u64)> = Vec::new();
    let mut last_pw = buf.pixel_writes;
    let mut last_t = std::time::Instant::now();
    macro_rules! mark {
        ($label:expr) => {
            let now_t = std::time::Instant::now();
            pixel_stats.push((
                $label,
                buf.pixel_writes - last_pw,
                now_t.duration_since(last_t).as_micros() as u64,
            ));
            last_pw = buf.pixel_writes;
            last_t = now_t;
        };
    }

    // 1. World cache: build once, patch on explosions, copy viewport each frame.
    if !lstate.cache_initialized {
        crate::renderer::draw_terrain::build_world_cache(&mut lstate.world_cache, &game.terrain);
        lstate.bg_cache = crate::renderer::bg_image::build_bg_cache(game.map_seed);
        buf.fill_deep_water_band();
        lstate.cache_initialized = true;
        lstate.cache_craters_processed = game.crater_log.len();
    }
    // Process at most 2 crater cache patches per frame so a burst of craters
    // arriving in one StateMsg doesn't spike frame time.
    let crater_limit = lstate.cache_craters_processed + 2;
    while lstate.cache_craters_processed < game.crater_log.len()
        && lstate.cache_craters_processed < crater_limit
    {
        let (cx, cy, r) = game.crater_log[lstate.cache_craters_processed];
        crate::renderer::draw_terrain::update_cache_region(
            &mut lstate.world_cache, &game.terrain, cx, cy, r,
        );
        lstate.cache_craters_processed += 1;
    }
    // 1b. Atmospheric background (behind terrain): clouds, hills, seed landform,
    //     wind debris — all driven by a shared gusting wind so they breathe together.
    use crate::renderer::background;
    crate::renderer::bg_image::copy_bg_viewport(buf, &lstate.bg_cache, &game.terrain, game.map_seed, cam_x, cam_y);
    mark!("bg_sky");
    let gw = background::gust_wind(game.wind.value(), lstate.tick);
    background::draw_backdrop(buf, &game.terrain, cam_x, cam_y);
    mark!("backdrop");
    background::update_debris(&mut lstate.bg_debris, &game.terrain, gw, lstate.tick);

    buf.copy_viewport_from_sky_aware(&lstate.world_cache, cam_x, cam_y, &game.terrain, &lstate.bg_cache, game.map_seed);
    mark!("terrain_copy");
    crate::renderer::scenery::draw_scenery(buf, &game.terrain, cam_x as i32, cam_y as i32);
    background::draw_debris(buf, &game.terrain, &lstate.bg_debris, cam_x, cam_y, lstate.tick);
    mark!("debris");

    // 2. Water ripple — cached strip, regenerated every tick (30 Hz).
    {
        use crate::renderer::draw_sprites::{render_water_strip, WATER_STRIP_H};
        render_water_strip(&mut lstate.water_strip, game.tick, cam_x);
        buf.blit_water_strip(&lstate.water_strip, cam_x, cam_y);
        let _ = WATER_STRIP_H;
    }
    mark!("water");

    // Viewport bounds for culling
    let vx0 = cam_x as f32;
    let vx1 = vx0 + sw as f32;

    // 4. Headstones — drawn over terrain, under living soldiers
    for grave in &game.graves {
        let wx = grave.pos.x;
        let wy = grave.pos.y;
        if wx >= vx0 - 8.0 && wx < vx1 + 8.0 && wy >= cam_y as f32 && wy < cam_y as f32 + sh as f32 {
            let gcolor = game.teams.get(grave.team).map(|t| t.color_id as usize).unwrap_or(grave.team.min(3));
            draw_headstone(buf, grave.pos, gcolor, grave.headstone_id);
        }
    }

    // 4b. Crates — 24×24 box with symbol; parachute while descending
    for cr in &game.crates {
        let wx = cr.pos.x as i32;
        let wy = cr.pos.y as i32;
        if cr.pos.x < vx0 - 32.0 || cr.pos.x >= vx1 + 32.0 { continue; }
        if wy < cam_y as i32 - 80 || wy >= cam_y as i32 + sh { continue; }

        if !cr.landed && cr.fall_ticks <= 60 {
            // Parachute canopy (2× original size)
            let cx = wx;
            let py = wy - 36;
            buf.fill_rect(cx - 16, py,      32, 4, Bgra::new(220, 220, 240)); // top bar
            buf.fill_rect(cx - 18, py + 4,  10, 8, Bgra::new(220, 220, 240)); // left arc
            buf.fill_rect(cx + 8,  py + 4,  10, 8, Bgra::new(220, 220, 240)); // right arc
            // Parachute strings
            buf.fill_rect(cx - 10, py + 12,  2, 16, Bgra::new(200, 200, 200));
            buf.fill_rect(cx + 8,  py + 12,  2, 16, Bgra::new(200, 200, 200));
        }

        // Crate body colour depends on kind: white = health, brown = weapon
        use super::state::CrateKind;
        let (body_col, symbol_col) = match cr.kind {
            CrateKind::Health    => (Bgra::new(240, 240, 240), Bgra::new(200, 30,  30)),
            CrateKind::Weapon(_) => (Bgra::new(140, 90,  40),  Bgra::new(240, 200, 40)),
            CrateKind::Scrap(_)  => (Bgra::new(30,  130, 130), Bgra::new(200, 255, 220)),
        };
        buf.fill_rect(wx - 12, wy - 12, 24, 24, body_col);
        // Dark border (2px)
        buf.fill_rect(wx - 12, wy - 12, 24,  2, Bgra::new(40, 30, 20));
        buf.fill_rect(wx - 12, wy + 10, 24,  2, Bgra::new(40, 30, 20));
        buf.fill_rect(wx - 12, wy - 12,  2, 24, Bgra::new(40, 30, 20));
        buf.fill_rect(wx + 10, wy - 12,  2, 24, Bgra::new(40, 30, 20));
        match cr.kind {
            CrateKind::Health => {
                // Red cross (2×)
                buf.fill_rect(wx - 8, wy - 2, 16, 4, symbol_col);
                buf.fill_rect(wx - 2, wy - 8,  4, 16, symbol_col);
            }
            CrateKind::Weapon(_) => {
                // Yellow X (2×)
                buf.draw_line(wx - 8, wy - 8, wx + 8, wy + 8, symbol_col);
                buf.draw_line(wx + 8, wy - 8, wx - 8, wy + 8, symbol_col);
                buf.draw_line(wx - 7, wy - 8, wx + 9, wy + 8, symbol_col);
                buf.draw_line(wx + 9, wy - 8, wx - 7, wy + 8, symbol_col);
                buf.draw_line(wx - 8, wy - 7, wx + 8, wy + 9, symbol_col);
                buf.draw_line(wx + 8, wy - 7, wx - 8, wy + 9, symbol_col);
            }
            CrateKind::Scrap(_) => {
                // Gear icon: outer ring + 4 teeth + hollow centre
                let bg = body_col;
                buf.fill_circle(wx, wy, 5, symbol_col);
                // Teeth: N, S, E, W — 3px tall, 2px wide
                buf.fill_rect(wx - 1, wy - 8, 3, 4, symbol_col);
                buf.fill_rect(wx - 1, wy + 4, 3, 4, symbol_col);
                buf.fill_rect(wx - 8, wy - 1, 4, 3, symbol_col);
                buf.fill_rect(wx + 4, wy - 1, 4, 3, symbol_col);
                // Hollow centre
                buf.fill_circle(wx, wy, 2, bg);
            }
        }
    }

    // 4c. Barrels — red oil drum (drawn before mines so mines appear on top)
    {
        use super::state::BarrelState;
        for barrel in &game.barrels {
            let wx = barrel.pos.x as i32;
            let wy = barrel.pos.y as i32;
            if barrel.pos.x < vx0 - 16.0 || barrel.pos.x >= vx1 + 16.0 { continue; }
            if wy < cam_y as i32 - 20 || wy >= cam_y as i32 + sh { continue; }
            let flicker = matches!(barrel.state, BarrelState::Triggered { .. })
                && (game.tick / 3) % 2 == 0;
            let body_col = if flicker { Bgra::new(255, 140, 40) } else { Bgra::new(160, 30, 20) };
            let rim_col  = Bgra::new(214, 84, 70);
            let dark_col = Bgra::new(80, 10, 8);
            // Drawn EXACTLY at its collision footprint (BARREL_HALF_W/BARREL_H, see `stamp_objects`): WA's oil drum is a 20x31 shape and
            // the art used to be ~1.2x scaled 36 px (43 px) over a 25 px collision box, so soldiers stood half-way up it and walked through
            // its stripe, and it read too big. Plain body + lid/base rims, with a hand-drawn flame icon on the face (WA's drums carry one).
            let mut r = |x: i32, y: i32, w: i32, h: i32, c: Bgra| buf.fill_rect(wx + x, wy + y, w as u32, h as u32, c);
            let hw = BARREL_HALF_W;
            let h = BARREL_H;
            r(-hw, -h, 2 * hw + 1, h + 1, dark_col);                  // outline = the whole collision box
            r(-hw + 1, -h + 2, 2 * hw - 1, h - 2, body_col);
            r(-hw, -h, 2 * hw + 1, 3, rim_col);                       // lid rim
            r(-hw, -3, 2 * hw + 1, 3, rim_col);                       // base rim
            for (fy, row) in DRUM_FLAME.iter().enumerate() {
                for (fx, ch) in row.bytes().enumerate() {
                    let c = match ch {
                        b'O' => Bgra::new(240, 112, 6),
                        b'Y' => Bgra::new(255, 205, 10),
                        b'W' => Bgra::new(255, 240, 140),
                        b'D' => Bgra::new(190, 80, 6),
                        _ => continue,
                    };
                    buf.set_pixel(wx - 5 + fx as i32, wy - 28 + fy as i32, c);
                }
            }
        }
    }

    // 4d. Mines — flat disc with blinking red LED, drawn at WA's mine size (16x8 px) scaled like the soldiers
    for mine in &game.mines {
        let wx = mine.pos.x as i32;
        let wy = mine.pos.y as i32;
        if mine.pos.x < vx0 - 16.0 || mine.pos.x >= vx1 + 16.0 { continue; }
        if wy < cam_y as i32 - 20 || wy >= cam_y as i32 + sh { continue; }
        let dk  = Bgra::new(15,  50, 15);
        let mid = Bgra::new(40, 100, 30);
        let hi  = Bgra::new(60, 135, 45);
        let rim = Bgra::new(30,  75, 22);
        let mut r = |x: i32, y: i32, w: i32, h: i32, c: Bgra| obj_rect(buf, wx, wy, x, y, w, h, c);
        r(-6, 0, 12, 2, Bgra::new(0, 0, 0));     // shadow
        r(-5, -2, 10, 3, dk);                    // bottom edge
        r(-6, -5, 12, 4, mid);                   // body
        r(-5, -7, 10, 2, rim);                   // top rim
        r(-3, -7, 6, 1, hi);                     // highlight strip
        let led_on = match mine.state {
            super::state::MineState::Arming    => (game.tick / 15) % 2 == 0,
            super::state::MineState::Armed     => (game.tick / 5)  % 2 == 0,
            super::state::MineState::Triggered => (game.tick / 3)  % 2 == 0,
        };
        r(-1, -9, 3, 2, if led_on { Bgra::new(220, 25, 25) } else { Bgra::new(80, 10, 10) });
    }

    mark!("objects");

    // 5. Soldiers — world coordinates
    let active_ti = game.active_team();
    let active_si = game.teams[active_ti].active;
    let mut active_muzzle: Option<(f32, f32)> = None;
    for (ti, team) in game.teams.iter().enumerate() {
        for (si, soldier) in team.soldiers.iter().enumerate() {
            // Skip dead soldiers unless they're waiting to explode
            if soldier.is_dead() && !soldier.death_explosion_pending { continue; }
            let wx = soldier.pos.x;
            let wy = soldier.pos.y;
            if wx >= vx0 - 16.0 && wx < vx1 + 16.0 && wy >= cam_y as f32 && wy < cam_y as f32 + sh as f32 {
                use crate::physics::projectile::WeaponKind;
                let aim_angle = if game.turn.is_acting()
                    && ti == active_ti
                    && si == active_si
                    && !soldier.has_fired
                {
                    Some(game.aim.angle)
                } else {
                    None
                };
                // Attached to the (non-flying) rope: lay the body out along the
                // swing direction instead of the default airborne pose. The rope
                // only ever attaches to the active soldier.
                let on_rope = ti == active_ti && si == active_si
                    && game.rope.as_ref().map_or(false, |r| !r.flying);
                let anim = match &soldier.state {
                    SoldierState::Airborne { vel, .. } if on_rope =>
                        SoldierAnim::RopeSwing { vel_x: vel.x, vel_y: vel.y },
                    SoldierState::Airborne { vel, spinning: true } =>
                        SoldierAnim::Airborne { vel_x: vel.x, vel_y: vel.y, airtime: soldier.airtime, spinning: true },
                    SoldierState::Airborne { vel, spinning: false } =>
                        SoldierAnim::Airborne { vel_x: vel.x, vel_y: vel.y, airtime: soldier.airtime, spinning: false },
                    SoldierState::Walking { .. } =>
                        SoldierAnim::Walking { tick: soldier.walk_ticks },
                    SoldierState::Dead =>
                        SoldierAnim::Dead,
                    SoldierState::Idle => match soldier.wa_act {
                        Some(crate::game::soldier::WaAct::Windup { age, .. }) =>
                            SoldierAnim::Crouch { depth: crate::renderer::skeleton::crouch_depth(age) },
                        _ => SoldierAnim::Idle,
                    },
                };
                let (gun_style, held_weapon) = if ti == active_ti && si == active_si && game.turn.is_acting() {
                    match game.active_team_ref().current_weapon() {
                        w @ (WeaponKind::Grenade | WeaponKind::BananaBomb |
                             WeaponKind::Blasthive | WeaponKind::BlackHoleBomb |
                             WeaponKind::HolyHandGrenade | WeaponKind::Tnt |
                             WeaponKind::Landmine) => (soldier.gun_style_id, Some(w)),
                        _ => (soldier.gun_style_id, None),
                    }
                } else {
                    (soldier.gun_style_id, None)
                };
                // Draw at the nearest pixel (not truncated): at 30 Hz the sub-pixel walk would otherwise stall every third tick.
                let (vx_, vy_) = match soldier.wa_vis {
                    Some((px, py, fr)) => (px + (soldier.pos.x - px) * fr, py + (soldier.pos.y - py) * fr),
                    None => {
                        // Average of the last 3 ticks (= exactly 5 WA frames): constant speed instead of 1,2,2-frame steps.
                        let e = lstate.vis_hist.entry((ti, si)).or_insert((lstate.tick, [(soldier.pos.x, soldier.pos.y); 3]));
                        if e.0 != lstate.tick {
                            e.0 = lstate.tick;
                            e.1 = [e.1[1], e.1[2], (soldier.pos.x, soldier.pos.y)];
                        } else {
                            e.1[2] = (soldier.pos.x, soldier.pos.y);
                        }
                        let far = e.1.iter().any(|p| (p.0 - soldier.pos.x).abs() > 24.0 || (p.1 - soldier.pos.y).abs() > 24.0);
                        if far { e.1 = [(soldier.pos.x, soldier.pos.y); 3]; }
                        ((e.1[0].0 + e.1[1].0 + e.1[2].0) / 3.0, (e.1[0].1 + e.1[1].1 + e.1[2].1) / 3.0)
                    }
                };
                let dpos = crate::world::WorldPos { x: vx_.round(), y: vy_.round() };
                // The pixel-art soldiers are drawn at the sub-pixel position: at 0.83 px/tick a rounded position advances
                // 1,1,1,1,1,0 px and visibly hitches.
                let fpos = crate::world::WorldPos { x: vx_, y: vy_ };
                buf.begin_scale(dpos.x, dpos.y, crate::renderer::draw_sprites::WORM_SPRITE_SCALE);
                let muzzle = draw_soldier_skeletal(buf, fpos, team.color_id as usize, soldier.facing, soldier.displayed_hp, &anim, aim_angle, false, // plates are drawn below
                    soldier.hat_id, soldier.uniform_color_id, soldier.boot_color_id, gun_style, held_weapon,
                    game.wind.value(), game.tick, soldier.on_fire_ticks);
                buf.end_scale();
                let muzzle = muzzle.map(|(mx, my)| {
                    let k = crate::renderer::draw_sprites::WORM_SPRITE_SCALE;
                    (fpos.x + (mx - fpos.x) * k, fpos.y + (my - fpos.y) * k)
                });
                if ti == active_ti && si == active_si {
                    active_muzzle = muzzle;
                    if let Some(m) = muzzle { lstate.last_muzzle = Some(m); }
                }

                // WA plates: name over HP (the HP value is the counting-down `displayed_hp`).
                // WA hides the active worm's plates once he has fired (seen on the rig).
                let hide_plates = ti == active_ti && si == active_si && soldier.has_fired;
                if (soldier.hp > 0 || soldier.displayed_hp > 0) && !hide_plates {
                    crate::renderer::draw_sprites::draw_worm_plates(buf, dpos.x as i32, dpos.y as i32, &soldier.name,
                        soldier.displayed_hp, team.color_id as usize, ti == active_ti && si == active_si);
                }

                // Active-soldier marker: downward triangle, raised above name
                if ti == active_ti && si == active_si && game.turn.is_acting() {
                    let cx  = dpos.x as i32;
                    let hat_lift = crate::renderer::skeleton::hat_name_lift(soldier.hat_id);
                    let _ = hat_lift;
                    let hp_top = crate::renderer::draw_sprites::plate_top();
                    let ty  = dpos.y as i32 - hp_top - 32;
                    let col = crate::renderer::draw_sprites::TEAM_COLOURS[team.color_id as usize];
                    buf.fill_rect(cx - 6, ty,     13, 2, col);
                    buf.fill_rect(cx - 5, ty + 2, 11, 2, col);
                    buf.fill_rect(cx - 4, ty + 4,  9, 2, col);
                    buf.fill_rect(cx - 3, ty + 6,  7, 1, col);
                    buf.fill_rect(cx - 2, ty + 7,  5, 1, col);
                    buf.fill_rect(cx - 1, ty + 8,  3, 1, col);
                    buf.fill_rect(cx,     ty + 9,  1, 1, col);
                }
            }
        }
    }

    mark!("soldiers");

    // 5b. Fire patches — drawn after soldiers so fire renders in front.
    // WA draws a flame from a FIXED sprite frame chosen by its size (decompile 0x5501a0 case 3: frame 15/14/13/12 for size > 3/4, 1/2,
    // 1/4, less): no flicker or sway, it only shrinks. Same here: one static flame, four sizes.
    for patch in &game.fire_patches {
        let wx = patch.pos.x as i32;
        let wy = patch.pos.y as i32;
        if patch.pos.x < vx0 - 10.0 || patch.pos.x >= vx1 + 10.0 { continue; }
        if wy < cam_y as i32 - 20 || wy >= cam_y as i32 + sh { continue; }
        let size = (patch.lifetime as f32 / if patch.turns { 501.0 } else { 150.0 }).clamp(0.0, 1.0);
        let frame = if size > 0.75 { 0 } else if size > 0.5 { 24 } else if size > 0.25 { 27 } else { 30 };
        crate::renderer::fx::draw_flame_star(buf, frame, wx, wy + 1);
    }

    mark!("fire_patches");

    // 5c. Plasma torch — flickering flame ball at the nozzle tip
    if let Some(ref torch) = game.plasma_torch {
        let ti = game.active_team();
        let si = game.teams[ti].active;
        if game.teams[ti].soldiers[si].is_alive() {
            let facing = game.teams[ti].soldiers[si].facing as f32;
            let (dx, dy) = torch.dir.to_vec(facing);
            let sx = game.teams[ti].soldiers[si].pos.x;
            // Matches step_plasma_torch's body_cy (true vertical body midpoint) and
            // TORCH_TIP_DIST (18px), so the flame sits at the same point the carve
            // circle is centered on instead of a separately-tuned offset.
            let sy = game.teams[ti].soldiers[si].pos.y
                - (crate::renderer::draw_sprites::BODY_H as f32 * 0.5);
            let tip_x = (sx + dx * 18.0) as i32;
            let tip_y = (sy + dy * 18.0) as i32;
            if tip_x >= cam_x as i32 && tip_x < cam_x as i32 + sw {
                // Radii scaled to match TORCH_RADIUS (17, 34px bore) so the flame
                // visually fills the tunnel it's carving instead of reading as a
                // small spark inside a much wider bore.
                let phase = game.tick as f32 * 0.6;
                let r1 = if phase.sin() > 0.0 { 17 } else { 16 };
                let r2 = if phase.cos() > 0.0 { 13 } else { 12 };
                let r3 = if phase.sin() > 0.0 { 6 } else { 5 };
                buf.fill_circle(tip_x, tip_y, r1, Bgra::new(220, 60, 10));
                buf.fill_circle(tip_x, tip_y, r2, Bgra::new(255, 150, 30));
                buf.fill_circle(tip_x, tip_y, r3, Bgra::new(255, 240, 120));
            }
        }
    }

    mark!("plasma_torch");

    // 5d. Garcia targeting beam / falling sprite
    if let Some(ref garcia) = game.garcia {
        let rx = garcia.render_x as i32;

        if !garcia.falling {
            // Pulsing alpha: 60–255 over ~2 s cycle
            let alpha = 60u8.saturating_add(
                ((garcia.blink_timer as f32 * 0.15).sin() * 0.5 + 0.5) as u8 * 195
            );
            let _ = alpha;

            // Cross-hair cursor, freely movable on both axes
            let cross_x = rx;
            let cross_y = garcia.render_y as i32;
            let cross_col = Bgra::new(255, 240, 60);
            buf.fill_rect(cross_x - 6, cross_y - 1, 13, 3, cross_col);
            buf.fill_rect(cross_x - 1, cross_y - 6,  3, 13, cross_col);

        } else {
            // Falling: draw scaled GARCIA sprite (~5× worm height, close to classic Worms Donkey scale)
            let fy = garcia.fall_y as i32;
            draw_garcia_sprite(buf, rx, fy, 80, 107);
        }
    }

    mark!("garcia");

    // 5d-2. Jumpbot: walking clockwork sprite + final-5s countdown over its head
    if let Some(ref jumpbot) = game.jumpbot {
        draw_jumpbot_sprite(buf, jumpbot.x as i32, jumpbot.y as i32, jumpbot.facing, 16, 16, jumpbot.walk_ticks, jumpbot.grounded);
        if jumpbot.super_bot { draw_jetpack(buf, jumpbot.x as i32, jumpbot.y as i32, jumpbot.facing, jumpbot.flying, jumpbot.heading, jumpbot.walk_ticks); }
        if jumpbot.fuse_ticks <= JUMPBOT_COUNTDOWN_TICKS {
            use crate::renderer::font::{draw_str, str_width};
            let secs = (jumpbot.fuse_ticks / 30) + 1; // ceil, so it reads 5..1 not 4..0
            let msg  = secs.to_string();
            let mw   = str_width(&msg);
            let mx   = jumpbot.x as i32 - mw / 2;
            let my   = jumpbot.y as i32 - 16 - 12;
            draw_str(buf, &msg, mx + 1, my + 1, Bgra::new(0, 0, 0));
            draw_str(buf, &msg, mx, my, Bgra::new(255, 80, 60));
        }
    }

    mark!("jumpbot");

    // 5e-2. Airstrike: crosshair during targeting; plane silhouette during active
    if let Some(ref air) = game.airstrike {
        let cl = cam_x as i32;
        use crate::renderer::font::{draw_str, str_width};

        // Direction label at top-center of screen (shown during targeting and active)
        {
            let label = if air.direction_right { "RIGHT >" } else { "< LEFT" };
            let lw = str_width(label);
            let lx = cl + (crate::world::SCREEN_W as i32 - lw) / 2;
            let ly = 4i32;
            buf.fill_rect(lx - 4, ly - 2, (lw + 8) as u32, 11, Bgra::new(0, 0, 0));
            draw_str(buf, label, lx, ly, Bgra::new(255, 240, 60));
        }

        if !air.active {
            // Yellow crosshair — same style as Garcia
            let cross_col = Bgra::new(255, 240, 60);
            let cx = air.render_x as i32;
            let cy = air.render_y as i32;
            buf.fill_rect(cx - 6, cy - 1, 13, 3, cross_col);
            buf.fill_rect(cx - 1, cy - 6,  3, 13, cross_col);
        } else {
            // Flying plane silhouette
            let px = air.plane_x as i32 - cl;
            let py = 14i32;
            if px > -60 && px < crate::world::SCREEN_W as i32 + 60 {
                let wx = cl + px;
                let body    = Bgra::new(170, 185, 200);
                let wing    = Bgra::new(130, 145, 160);
                let glass   = Bgra::new(80, 160, 240);
                let exhaust = Bgra::new(255, 160, 50);
                let f: i32 = if air.direction_right { 1 } else { -1 };
                buf.fill_rect(wx - 22, py + 1, 44, 5, Bgra::new(80, 90, 100));
                buf.fill_rect(wx - 21, py + 2, 42, 3, body);
                buf.fill_rect(wx + f * 4 - 5, py - 4, 11, 5, Bgra::new(50, 60, 70));
                buf.fill_rect(wx + f * 4 - 4, py - 3,  9, 4, glass);
                buf.fill_rect(wx - 14, py + 5, 28, 5, Bgra::new(60, 70, 80));
                buf.fill_rect(wx - 13, py + 6, 26, 3, wing);
                buf.fill_rect(wx - f * 19 - 2, py - 5, 5, 7, Bgra::new(60, 70, 80));
                buf.fill_rect(wx - f * 19 - 1, py - 4, 3, 5, wing);
                buf.fill_rect(wx - f * 22, py + 2, 3, 2, exhaust);
            }
        }
    }

    // 5e. Black holes — drawn after fire patches, before projectiles
    for hole in &game.black_holes {
        let wx = hole.pos.x as i32;
        let wy = hole.pos.y as i32;
        if hole.pos.x < vx0 - 12.0 || hole.pos.x >= vx1 + 12.0 { continue; }
        let pulse = (game.tick / 5) % 2 == 0;
        let halo_r = if pulse { 11 } else { 10 }; // 40% smaller orb (was 18/16)
        let purpd = Bgra::new(50, 0, 80);
        let purp  = Bgra::new(140, 0, 200);
        let void  = Bgra::new(0, 0, 0);
        let glow  = Bgra::new(180, 60, 240);
        buf.fill_circle(wx, wy, halo_r, purpd);
        buf.fill_circle(wx, wy, halo_r - 3, purp);
        buf.fill_circle(wx, wy, 4, void);
        // Cardinal glow dots
        buf.set_pixel(wx,           wy - halo_r - 1, glow);
        buf.set_pixel(wx,           wy + halo_r + 1, glow);
        buf.set_pixel(wx - halo_r - 1, wy,           glow);
        buf.set_pixel(wx + halo_r + 1, wy,           glow);
    }

    mark!("black_holes");

    // 5e-3. Homing Missile targeting crosshair (same style as Garcia/AirStrike)
    if let Some(ref hm) = game.homing_missile {
        let rx = hm.render_x as i32;
        let ry = hm.render_y as i32;
        let cross_col = Bgra::new(255, 240, 60);
        buf.fill_rect(rx - 6, ry - 1, 13, 3, cross_col);
        buf.fill_rect(rx - 1, ry - 6,  3, 13, cross_col);
    }

    // 5c. Bazooka smoke trail (behind rockets)
    for (pos, ticks_left) in &game.smoke_particles {
        let t = *ticks_left;
        if pos.x >= vx0 && pos.x < vx1 {
            let shade = if t > 15 { Bgra::new(180, 180, 180) }
                        else if t > 10 { Bgra::new(140, 140, 140) }
                        else if t > 5  { Bgra::new(100, 100, 100) }
                        else            { Bgra::new(65,  65,  65)  };
            let r = if t > 15 { 5 } else if t > 8 { 3 } else { 2 };
            buf.fill_circle(pos.x as i32, pos.y as i32, r, shade);
        }
    }

    mark!("smoke_trail");

    // 6. Projectiles + fuse countdown
    for proj in &game.projectiles {
        use crate::physics::projectile::{WeaponKind, FuseState};
        let wx = proj.pos.x;
        let wy = proj.pos.y;
        if wx >= vx0 && wx < vx1 && wy >= cam_y as f32 && wy < cam_y as f32 + sh as f32 {
            if proj.kind == WeaponKind::Grenade {
                draw_grenade_projectile(buf, proj.pos);
                // Fuse countdown: show remaining seconds above the grenade
                if let FuseState::Burning(ticks) = proj.fuse {
                    use crate::renderer::font::{draw_str_scaled, str_width_scaled};
                    let secs = ((ticks + 29) / 30).min(9);
                    let label = format!("{}", secs);
                    let lx = proj.pos.x as i32 - str_width_scaled(&label, 2) / 2;
                    let ly = proj.pos.y as i32 - 20;
                    draw_str_scaled(buf, &label, lx + 1, ly + 1, Bgra::new(0, 0, 0), 2);
                    draw_str_scaled(buf, &label, lx,     ly,     Bgra::yellow(), 2);
                }
            } else if proj.kind == WeaponKind::ClusterBomb {
                // Main bomb: grenade body with orange stripe; fragment: smaller dot
                let cx = proj.pos.x as i32;
                let cy = proj.pos.y as i32;
                if proj.is_fragment {
                    // 7px oval fragment with orange band
                    let body = Bgra::new(55, 120, 45);
                    let dark = Bgra::new(25, 60, 20);
                    let band = Bgra::new(30, 130, 200); // orange stripe (BGR)
                    buf.fill_rect(cx - 2, cy - 3, 4, 1, dark);
                    buf.fill_rect(cx - 3, cy - 2, 6, 5, dark);
                    buf.fill_rect(cx - 2, cy + 3, 4, 1, dark);
                    buf.fill_rect(cx - 1, cy - 2, 2, 1, body);
                    buf.fill_rect(cx - 2, cy - 1, 4, 1, body);
                    buf.fill_rect(cx - 2, cy,     4, 1, band);
                    buf.fill_rect(cx - 2, cy + 1, 4, 1, body);
                    buf.fill_rect(cx - 1, cy + 2, 2, 1, body);
                } else {
                    draw_grenade_projectile(buf, proj.pos);
                    // Orange band over seam to distinguish from plain grenade
                    let band = Bgra::new(30, 130, 200); // orange (BGR)
                    buf.fill_rect(cx - 2, cy - 1, 4, 1, band);
                    // Fuse countdown
                    if let FuseState::Burning(ticks) = proj.fuse {
                        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
                        let secs = ((ticks + 29) / 30).min(9);
                        let label = format!("{}", secs);
                        let lx = proj.pos.x as i32 - str_width_scaled(&label, 2) / 2;
                        let ly = proj.pos.y as i32 - 20;
                        draw_str_scaled(buf, &label, lx + 1, ly + 1, Bgra::new(0, 0, 0), 2);
                        draw_str_scaled(buf, &label, lx,     ly,     Bgra::new(30, 165, 255), 2);
                    }
                }
            } else if proj.kind == WeaponKind::HolyHandGrenade {
                // Sacred Ordnance: large golden body with tumbling cross
                let gx = proj.pos.x as i32;
                let gy = proj.pos.y as i32;
                let gdark  = Bgra::new(140, 95, 10);
                let gbody  = Bgra::new(210, 155, 30);
                let ghi    = Bgra::new(255, 230, 100);
                let gray   = Bgra::new(160, 160, 165);
                let gold   = Bgra::new(255, 215, 45);
                let goldhi = Bgra::new(255, 248, 160);
                let lgt    = Bgra::new(200, 200, 205);
                // 8 orientations (every 45°); direction follows vel.x sign
                let speed  = (proj.vel.x.abs() + proj.vel.y.abs()) as u32;
                let rate   = if speed > 8 { 2 } else if speed > 3 { 4 } else { 0 };
                let raw    = if rate > 0 { (proj.age_ticks / rate) % 8 } else { 0 };
                // clockwise when moving right, counter-clockwise when moving left
                let spin   = if proj.vel.x >= 0.0 { raw } else { (8 - raw) % 8 };
                // Shared diagonal body (~circular, used for frames 1/3/5/7)
                let draw_diag_body = |buf: &mut crate::renderer::WorldBuffer| {
                    buf.fill_rect(gx - 3, gy - 5, 6, 1, gdark);
                    buf.fill_rect(gx - 4, gy - 4, 8, 8, gdark);
                    buf.fill_rect(gx - 3, gy + 4, 6, 1, gdark);
                    buf.fill_rect(gx - 2, gy - 4, 4, 1, gbody);
                    buf.fill_rect(gx - 3, gy - 3, 6, 7, gbody);
                    buf.fill_rect(gx - 2, gy + 3, 4, 1, gbody);
                    buf.fill_rect(gx - 1, gy - 3, 2, 1, ghi);
                    buf.fill_rect(gx - 2, gy - 2, 2, 2, ghi);
                    buf.fill_rect(gx - 3, gy,     6, 1, gdark);
                };
                match spin {
                    0 => {
                        // Cross up — tall oval
                        buf.fill_rect(gx - 3, gy - 7, 6, 1, gdark);
                        buf.fill_rect(gx - 4, gy - 6, 8, 11, gdark);
                        buf.fill_rect(gx - 3, gy + 5, 6, 1, gdark);
                        buf.fill_rect(gx - 2, gy - 6, 4, 1, gbody);
                        buf.fill_rect(gx - 3, gy - 5, 6, 9, gbody);
                        buf.fill_rect(gx - 2, gy + 4, 4, 1, gbody);
                        buf.fill_rect(gx - 2, gy - 4, 2, 1, ghi);
                        buf.fill_rect(gx - 3, gy - 3, 2, 3, ghi);
                        buf.fill_rect(gx - 3, gy,     6, 1, gdark);
                        buf.fill_rect(gx - 3, gy - 7, 6, 2, gray);
                        buf.fill_rect(gx - 2, gy - 6, 4, 1, lgt);
                        buf.fill_rect(gx - 1, gy - 15, 2, 9, gold);
                        buf.fill_rect(gx - 3, gy - 11, 7, 2, gold);
                        buf.fill_rect(gx,     gy - 14, 1, 7, goldhi);
                        buf.fill_rect(gx - 2, gy - 10, 5, 1, goldhi);
                    }
                    1 => {
                        // Cross upper-right (45°) — circular body
                        draw_diag_body(buf);
                        buf.fill_rect(gx + 1, gy - 5, 3, 2, gray);
                        buf.fill_rect(gx + 2, gy - 4, 1, 1, lgt);
                        // NE arm (6 diagonal steps of 2×2)
                        buf.fill_rect(gx + 2, gy - 6, 2, 2, gold);
                        buf.fill_rect(gx + 3, gy - 7, 2, 2, gold);
                        buf.fill_rect(gx + 4, gy - 8, 2, 2, gold);
                        buf.fill_rect(gx + 5, gy - 9, 2, 2, gold);
                        buf.fill_rect(gx + 6, gy -10, 2, 2, gold);
                        buf.fill_rect(gx + 7, gy -11, 2, 2, gold);
                        // Crossbar (SE direction, centred at step 3)
                        buf.fill_rect(gx + 6, gy - 8, 2, 2, gold);
                        buf.fill_rect(gx + 7, gy - 7, 2, 2, gold);
                        buf.fill_rect(gx + 4, gy -10, 2, 2, gold);
                        buf.fill_rect(gx + 3, gy -11, 2, 2, gold);
                        buf.fill_rect(gx + 5, gy -10, 1, 1, goldhi);
                        buf.fill_rect(gx + 6, gy -11, 1, 1, goldhi);
                        buf.fill_rect(gx + 6, gy - 8, 1, 1, goldhi);
                    }
                    2 => {
                        // Cross right — wide oval
                        buf.fill_rect(gx - 7, gy - 3, 1, 6, gdark);
                        buf.fill_rect(gx - 6, gy - 4, 11, 8, gdark);
                        buf.fill_rect(gx + 4, gy - 3, 1, 6, gdark);
                        buf.fill_rect(gx - 6, gy - 2, 1, 4, gbody);
                        buf.fill_rect(gx - 5, gy - 3, 9, 6, gbody);
                        buf.fill_rect(gx + 3, gy - 2, 1, 4, gbody);
                        buf.fill_rect(gx - 4, gy - 2, 2, 1, ghi);
                        buf.fill_rect(gx - 5, gy - 1, 2, 2, ghi);
                        buf.fill_rect(gx,     gy - 3, 1, 6, gdark);
                        buf.fill_rect(gx + 3, gy - 3, 2, 6, gray);
                        buf.fill_rect(gx + 4, gy - 2, 1, 4, lgt);
                        buf.fill_rect(gx + 5, gy - 1, 9, 2, gold);
                        buf.fill_rect(gx + 9, gy - 4, 2, 7, gold);
                        buf.fill_rect(gx + 6, gy - 1, 7, 1, goldhi);
                        buf.fill_rect(gx + 9, gy - 3, 1, 5, goldhi);
                    }
                    3 => {
                        // Cross lower-right (135°) — circular body
                        draw_diag_body(buf);
                        buf.fill_rect(gx + 1, gy + 3, 3, 2, gray);
                        buf.fill_rect(gx + 2, gy + 4, 1, 1, lgt);
                        // SE arm
                        buf.fill_rect(gx + 2, gy + 4, 2, 2, gold);
                        buf.fill_rect(gx + 3, gy + 5, 2, 2, gold);
                        buf.fill_rect(gx + 4, gy + 6, 2, 2, gold);
                        buf.fill_rect(gx + 5, gy + 7, 2, 2, gold);
                        buf.fill_rect(gx + 6, gy + 8, 2, 2, gold);
                        buf.fill_rect(gx + 7, gy + 9, 2, 2, gold);
                        // Crossbar (NE direction)
                        buf.fill_rect(gx + 6, gy + 6, 2, 2, gold);
                        buf.fill_rect(gx + 7, gy + 5, 2, 2, gold);
                        buf.fill_rect(gx + 4, gy + 8, 2, 2, gold);
                        buf.fill_rect(gx + 3, gy + 9, 2, 2, gold);
                        buf.fill_rect(gx + 5, gy + 7, 1, 1, goldhi);
                        buf.fill_rect(gx + 6, gy + 8, 1, 1, goldhi);
                        buf.fill_rect(gx + 6, gy + 6, 1, 1, goldhi);
                    }
                    4 => {
                        // Cross down — tall oval
                        buf.fill_rect(gx - 3, gy - 7, 6, 1, gdark);
                        buf.fill_rect(gx - 4, gy - 6, 8, 11, gdark);
                        buf.fill_rect(gx - 3, gy + 5, 6, 1, gdark);
                        buf.fill_rect(gx - 2, gy - 6, 4, 1, gbody);
                        buf.fill_rect(gx - 3, gy - 5, 6, 9, gbody);
                        buf.fill_rect(gx - 2, gy + 4, 4, 1, gbody);
                        buf.fill_rect(gx - 2, gy - 4, 2, 1, ghi);
                        buf.fill_rect(gx - 3, gy - 3, 2, 3, ghi);
                        buf.fill_rect(gx - 3, gy,     6, 1, gdark);
                        buf.fill_rect(gx - 3, gy + 5, 6, 2, gray);
                        buf.fill_rect(gx - 2, gy + 6, 4, 1, lgt);
                        buf.fill_rect(gx - 1, gy + 7, 2, 9, gold);
                        buf.fill_rect(gx - 3, gy + 11, 7, 2, gold);
                        buf.fill_rect(gx,     gy +  8, 1, 7, goldhi);
                        buf.fill_rect(gx - 2, gy + 12, 5, 1, goldhi);
                    }
                    5 => {
                        // Cross lower-left (225°) — circular body
                        draw_diag_body(buf);
                        buf.fill_rect(gx - 4, gy + 3, 3, 2, gray);
                        buf.fill_rect(gx - 3, gy + 4, 1, 1, lgt);
                        // SW arm
                        buf.fill_rect(gx - 4, gy + 4, 2, 2, gold);
                        buf.fill_rect(gx - 5, gy + 5, 2, 2, gold);
                        buf.fill_rect(gx - 6, gy + 6, 2, 2, gold);
                        buf.fill_rect(gx - 7, gy + 7, 2, 2, gold);
                        buf.fill_rect(gx - 8, gy + 8, 2, 2, gold);
                        buf.fill_rect(gx - 9, gy + 9, 2, 2, gold);
                        // Crossbar (NW direction)
                        buf.fill_rect(gx - 8, gy + 6, 2, 2, gold);
                        buf.fill_rect(gx - 9, gy + 5, 2, 2, gold);
                        buf.fill_rect(gx - 6, gy + 8, 2, 2, gold);
                        buf.fill_rect(gx - 5, gy + 9, 2, 2, gold);
                        buf.fill_rect(gx - 7, gy + 7, 1, 1, goldhi);
                        buf.fill_rect(gx - 8, gy + 8, 1, 1, goldhi);
                        buf.fill_rect(gx - 8, gy + 6, 1, 1, goldhi);
                    }
                    6 => {
                        // Cross left — wide oval
                        buf.fill_rect(gx - 7, gy - 3, 1, 6, gdark);
                        buf.fill_rect(gx - 6, gy - 4, 11, 8, gdark);
                        buf.fill_rect(gx + 4, gy - 3, 1, 6, gdark);
                        buf.fill_rect(gx - 6, gy - 2, 1, 4, gbody);
                        buf.fill_rect(gx - 5, gy - 3, 9, 6, gbody);
                        buf.fill_rect(gx + 3, gy - 2, 1, 4, gbody);
                        buf.fill_rect(gx - 4, gy - 2, 2, 1, ghi);
                        buf.fill_rect(gx - 5, gy - 1, 2, 2, ghi);
                        buf.fill_rect(gx,     gy - 3, 1, 6, gdark);
                        buf.fill_rect(gx - 5, gy - 3, 2, 6, gray);
                        buf.fill_rect(gx - 5, gy - 2, 1, 4, lgt);
                        buf.fill_rect(gx - 14, gy - 1, 9, 2, gold);
                        buf.fill_rect(gx - 11, gy - 4, 2, 7, gold);
                        buf.fill_rect(gx - 13, gy - 1, 7, 1, goldhi);
                        buf.fill_rect(gx - 10, gy - 3, 1, 5, goldhi);
                    }
                    _ => {
                        // Cross upper-left (315°) — circular body
                        draw_diag_body(buf);
                        buf.fill_rect(gx - 4, gy - 5, 3, 2, gray);
                        buf.fill_rect(gx - 3, gy - 4, 1, 1, lgt);
                        // NW arm
                        buf.fill_rect(gx - 4, gy - 6, 2, 2, gold);
                        buf.fill_rect(gx - 5, gy - 7, 2, 2, gold);
                        buf.fill_rect(gx - 6, gy - 8, 2, 2, gold);
                        buf.fill_rect(gx - 7, gy - 9, 2, 2, gold);
                        buf.fill_rect(gx - 8, gy -10, 2, 2, gold);
                        buf.fill_rect(gx - 9, gy -11, 2, 2, gold);
                        // Crossbar (SW direction)
                        buf.fill_rect(gx - 8, gy - 8, 2, 2, gold);
                        buf.fill_rect(gx - 9, gy - 7, 2, 2, gold);
                        buf.fill_rect(gx - 6, gy -10, 2, 2, gold);
                        buf.fill_rect(gx - 5, gy -11, 2, 2, gold);
                        buf.fill_rect(gx - 7, gy - 9, 1, 1, goldhi);
                        buf.fill_rect(gx - 8, gy -10, 1, 1, goldhi);
                        buf.fill_rect(gx - 8, gy - 8, 1, 1, goldhi);
                    }
                }
                // Fuse countdown while burning
                if let FuseState::Burning(ticks) = proj.fuse {
                    use crate::renderer::font::{draw_str_scaled, str_width_scaled};
                    let secs = ((ticks + 29) / 30).min(9);
                    let label = format!("{}", secs);
                    let lx = gx - str_width_scaled(&label, 2) / 2;
                    let ly = gy - 26;
                    draw_str_scaled(buf, &label, lx + 1, ly + 1, Bgra::new(0, 0, 0), 2);
                    draw_str_scaled(buf, &label, lx,     ly,     Bgra::new(255, 215, 45), 2);
                }
                // Gold glow when armed (stopped, about to detonate)
                if matches!(proj.fuse, FuseState::Armed | FuseState::Detonating(_)) {
                    buf.set_pixel(gx - 5, gy - 1, gold);
                    buf.set_pixel(gx + 4, gy - 1, gold);
                    buf.set_pixel(gx,     gy - 8, gold);
                }
            } else if proj.kind == WeaponKind::Tnt {
                let px = proj.pos.x as i32;
                let py = proj.pos.y as i32;
                // Red stick body
                buf.fill_rect(px - 3, py - 7,  6, 12, Bgra::new(190, 25, 15));
                buf.fill_rect(px - 3, py - 7,  2, 12, Bgra::new(230, 60, 45));
                buf.fill_rect(px + 1, py - 7,  2, 12, Bgra::new(110, 12,  8));
                // Gray fuse
                buf.fill_rect(px,     py - 9,  1,  3, Bgra::new(160, 160, 160));
                buf.fill_rect(px + 1, py - 11, 1,  2, Bgra::new(160, 160, 160));
                // Countdown label above stick
                if let FuseState::Burning(ticks) = proj.fuse {
                    use crate::renderer::font::{draw_str_scaled, str_width_scaled};
                    let secs = ticks / 30;
                    let label = format!("{}", secs);
                    let lx = px - str_width_scaled(&label, 2) / 2;
                    let ly = py - 20;
                    draw_str_scaled(buf, &label, lx + 1, ly + 1, Bgra::new(0, 0, 0), 2);
                    draw_str_scaled(buf, &label, lx,     ly,     Bgra::new(255, 100, 50), 2);
                }
            } else if proj.kind == WeaponKind::BananaBomb {
                let px = proj.pos.x as i32;
                let py = proj.pos.y as i32;
                if proj.is_fragment {
                    // Fragment: small gray sphere (~1.5× grenade)
                    buf.fill_circle(px, py, 4, Bgra::new(55, 55, 58));   // dark outline
                    buf.fill_circle(px, py, 3, Bgra::new(130, 130, 135)); // gray body
                    buf.set_pixel(px - 1, py - 2, Bgra::new(190, 190, 195)); // highlight dot
                    // Brief fuse countdown
                    if let FuseState::Burning(ticks) = proj.fuse {
                        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
                        let secs = ((ticks + 29) / 30).min(9);
                        let label = format!("{}", secs);
                        let lx = px - str_width_scaled(&label, 2) / 2;
                        draw_str_scaled(buf, &label, lx + 1, py - 14, Bgra::new(0, 0, 0), 2);
                        draw_str_scaled(buf, &label, lx,     py - 15, Bgra::new(200, 200, 210), 2);
                    }
                } else {
                    // Main Meteor Bomb: gray sphere ~2.5× grenade (grenade ~5px tall → radius 6)
                    buf.fill_circle(px, py, 7, Bgra::new(55, 55, 58));    // dark outline
                    buf.fill_circle(px, py, 6, Bgra::new(130, 130, 135)); // gray body
                    buf.fill_circle(px - 2, py - 2, 2, Bgra::new(190, 190, 195)); // highlight
                    // Fuse countdown
                    if let FuseState::Burning(ticks) = proj.fuse {
                        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
                        let secs = ((ticks + 29) / 30).min(9);
                        let label = format!("{}", secs);
                        let lx = px - str_width_scaled(&label, 2) / 2;
                        let ly = py - 20;
                        draw_str_scaled(buf, &label, lx + 1, ly + 1, Bgra::new(0, 0, 0), 2);
                        draw_str_scaled(buf, &label, lx,     ly,     Bgra::new(200, 200, 210), 2);
                    }
                }
            } else if proj.kind == WeaponKind::Blasthive {
                let px = proj.pos.x as i32;
                let py = proj.pos.y as i32;
                if proj.is_fragment {
                    // Bee: 5×4 animated sprite
                    let yel  = Bgra::new(255, 210, 0);
                    let blk  = Bgra::new(0, 0, 0);
                    let wing = Bgra::new(220, 245, 255);
                    let amb  = Bgra::new(200, 140, 0);
                    let wing_up = proj.age_ticks % 6 < 3;
                    if wing_up {
                        buf.set_pixel(px - 2, py - 2, wing);
                        buf.set_pixel(px + 2, py - 2, wing);
                        buf.set_pixel(px - 2, py - 1, wing);
                        buf.set_pixel(px + 2, py - 1, wing);
                    } else {
                        buf.set_pixel(px - 2, py,     wing);
                        buf.set_pixel(px + 2, py,     wing);
                        buf.set_pixel(px - 2, py + 1, wing);
                        buf.set_pixel(px + 2, py + 1, wing);
                    }
                    buf.set_pixel(px,     py - 1, blk);
                    buf.set_pixel(px - 1, py,     yel);
                    buf.set_pixel(px,     py,     yel);
                    buf.set_pixel(px + 1, py,     amb);
                    buf.set_pixel(px,     py + 1, blk);
                    // Trail
                    let tx = px - proj.vel.x.signum() as i32;
                    let ty = py - proj.vel.y.signum() as i32;
                    buf.set_pixel(tx, ty, Bgra::new(180, 140, 0));
                } else {
                    // Hive: amber rounded box with stripe rings
                    let hdk = Bgra::new(70, 45, 12);
                    let hmd = Bgra::new(165, 110, 35);
                    buf.fill_circle(px, py, 5, hdk);
                    buf.fill_circle(px, py, 4, hmd);
                    buf.draw_line(px - 3, py - 1, px + 3, py - 1, hdk);
                    buf.draw_line(px - 3, py + 1, px + 3, py + 1, hdk);
                    if let FuseState::Burning(ticks) = proj.fuse {
                        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
                        let secs = ((ticks + 29) / 30).min(9);
                        let label = format!("{}", secs);
                        let lx = px - str_width_scaled(&label, 2) / 2;
                        let ly = py - 20;
                        draw_str_scaled(buf, &label, lx + 1, ly + 1, Bgra::new(0, 0, 0), 2);
                        draw_str_scaled(buf, &label, lx,     ly,     Bgra::new(255, 220, 50), 2);
                    }
                }
            } else if proj.kind == WeaponKind::MolotovCocktail {
                crate::renderer::draw_sprites::draw_molotov_projectile(buf, proj.pos, proj.vel);
            } else if proj.kind == WeaponKind::Bazooka {
                crate::renderer::draw_sprites::draw_bazooka(buf, proj.pos, proj.vel);
            } else if proj.kind == WeaponKind::HomingMissile {
                crate::renderer::draw_sprites::draw_homing_missile(buf, proj.pos, proj.vel);
            } else if proj.kind == WeaponKind::BlackHoleBomb {
                let px = proj.pos.x as i32;
                let py = proj.pos.y as i32;
                let purpd = Bgra::new(60, 0, 90);
                let purp  = Bgra::new(160, 0, 220);
                let void  = Bgra::new(0, 0, 0);
                let glow  = Bgra::new(200, 80, 255);
                buf.fill_circle(px, py, 7, purpd);
                buf.fill_circle(px, py, 5, purp);
                buf.fill_circle(px, py, 3, void);
                // Orbiting glow particles
                let a = proj.age_ticks as f32 * 0.35;
                for &off in &[0.0f32, std::f32::consts::PI] {
                    let gx = px + (6.0 * (a + off).cos()) as i32;
                    let gy = py + (6.0 * (a + off).sin()) as i32;
                    buf.set_pixel(gx, gy, glow);
                }
            } else {
                draw_projectile(buf, proj.pos, 2, Bgra::yellow());
            }
        }
    }

    mark!("projectiles");

    // 7. Aim arrow
    if game.turn.is_acting() {
        let active = game.active_team_ref().active_soldier();
        if !active.has_fired {
            let wx = active.pos.x;
            if wx >= vx0 && wx < vx1 {
                let (display_angle, power) = if let Some(ref torch) = game.plasma_torch {
                    // Torch active: reticle points along the tunnel direction.
                    let facing = active.facing as f32;
                    let (tdx, tdy) = torch.dir.to_vec(facing);
                    let angle = (-tdy).atan2(tdx); // convert direction vec → angle
                    (angle, 0.02) // small power so reticle circle always draws
                } else {
                    let a = if active.facing < 0 { std::f32::consts::PI - game.aim.angle } else { game.aim.angle };
                    // The bazooka's meter tops out at 1.0, so stretch it to the
                    // full bar (the shared bar is scaled to MAX_CHARGE).
                    let p = if crate::physics::tick::uses_wa_charge(game.active_team_ref().current_weapon()) {
                        game.aim.power * (MAX_CHARGE / crate::physics::tick::BAZOOKA_MAX_CHARGE)
                    } else {
                        game.aim.power
                    };
                    (a, p)
                };
                if let Some(muzzle) = active_muzzle {
                    draw_aim_arrow(buf, muzzle, display_angle, power);
                }
            }
        }
    }

    // 7a-trail removed — revolver is instant hitscan, no visual trail

    // 7a-rope. Grappling hook rope — line from soldier to hook/anchor
    if let Some(ref rope) = game.rope {
        let rtx = game.active_team();
        let rsx = game.teams[rtx].active;
        let spos = game.teams[rtx].soldiers[rsx].pos;
        let rope_col = Bgra::new(180, 200, 140);
        let hook_col = Bgra::new(220, 180, 80);
        // 3px-thick segment: center line plus one line offset to each side along
        // the rounded perpendicular unit vector (.round(), not truncation — see
        // thick-line rounding rule).
        fn rope_seg(buf: &mut WorldBuffer, x0: i32, y0: i32, x1: i32, y1: i32, col: Bgra) {
            buf.draw_line(x0, y0, x1, y1, col);
            let dx = (x1 - x0) as f32;
            let dy = (y1 - y0) as f32;
            let len = (dx * dx + dy * dy).sqrt().max(0.1);
            let ox = (-dy / len).round() as i32;
            let oy = (dx / len).round() as i32;
            buf.draw_line(x0 + ox, y0 + oy, x1 + ox, y1 + oy, col);
            buf.draw_line(x0 - ox, y0 - oy, x1 - ox, y1 - oy, col);
        }
        // Pointed hook tip: a small triangle at the rope's far end, tip pointing
        // away from the last segment (into the wall / direction of flight).
        fn rope_point(buf: &mut WorldBuffer, from: (f32, f32), end: (f32, f32), col: Bgra) {
            let dx = end.0 - from.0;
            let dy = end.1 - from.1;
            let len = (dx * dx + dy * dy).sqrt().max(0.1);
            let (ux, uy) = (dx / len, dy / len);
            let (px_, py_) = (-uy, ux);
            // Draw shrinking perpendicular slices from the base toward the tip.
            for i in 0..=5 {
                let half = 3.0 * (1.0 - i as f32 / 5.0);
                let cx = end.0 + ux * i as f32;
                let cy = end.1 + uy * i as f32;
                buf.draw_line(
                    (cx + px_ * half).round() as i32, (cy + py_ * half).round() as i32,
                    (cx - px_ * half).round() as i32, (cy - py_ * half).round() as i32,
                    col,
                );
            }
        }
        if rope.flying {
            // Straight line to the in-flight hook.
            let end = rope.hook;
            rope_seg(buf, spos.x as i32, spos.y as i32 - 6, end.x as i32, end.y as i32, rope_col);
            rope_point(buf, (spos.x, spos.y - 6.0), (end.x, end.y), hook_col);
        } else {
            // Polyline: soldier → each wrap pivot (nearest first) → anchor. `wrap` runs
            // anchor→soldier, so walk it in reverse from the soldier's hand.
            let mut px = spos.x as i32;
            let mut py = spos.y as i32 - 6;
            for pivot in rope.wrap.iter().rev() {
                rope_seg(buf, px, py, pivot.x as i32, pivot.y as i32, rope_col);
                px = pivot.x as i32;
                py = pivot.y as i32;
            }
            rope_seg(buf, px, py, rope.anchor.x as i32, rope.anchor.y as i32, rope_col);
            rope_point(buf, (px as f32, py as f32), (rope.anchor.x, rope.anchor.y), hook_col);
        }
    }

    // 7b-pre. Blood splats — drawn on top of soldiers so they're visible over sprites
    for (pos, ticks) in &game.blood_splats {
        let bx = pos.x as i32;
        let by = pos.y as i32;
        if bx >= vx0 as i32 - 4 && bx < vx1 as i32 + 4 && by >= 0 && by < sh {
            let fade = (*ticks as f32 / 90.0).clamp(0.0, 1.0);
            let r = (200.0 * fade) as u8 + 35;
            buf.fill_rect(bx - 1, by - 1, 3, 3, Bgra::new(r, 8, 8));
        }
    }

    // 7b. Explosions — drawn on top of soldiers, under HUD
    for exp in &game.explosions {
        let wx = exp.pos.x;
        let wy = exp.pos.y;
        if wx >= vx0 - exp.radius && wx < vx1 + exp.radius && wy >= cam_y as f32 - exp.radius && wy < cam_y as f32 + sh as f32 + exp.radius {
            draw_explosion(buf, exp.pos, exp.radius, exp.age);
        }
    }

    // 7b'. Effect particles — explosion fallout, dust, sparks, splashes.
    crate::renderer::fx::draw_fx(buf, &game.fx, cam_x);
    // 7b''. Floating damage-number popups, drawn over the fx particles.
    crate::renderer::fx::draw_fx_text(buf, &game.fx_text, cam_x);

    mark!("fx_overlay");

    // 7c/8. Top-centre banner queue — TNT fuse, action prompts, then messages,
    // collected in priority order and drawn as one stack (max 3) so they never
    // overlap. Heights are consumed sequentially from y=4.
    let mut banners: Vec<(String, Bgra)> = Vec::new();
    if game.tnt_placed {
        use crate::physics::projectile::{WeaponKind, FuseState};
        if let Some(tnt) = game.projectiles.iter().find(|p| p.kind == WeaponKind::Tnt) {
            if let FuseState::Burning(ticks) = tnt.fuse {
                banners.push((format!("TNT  {}", ticks / 30), Bgra::new(255, 120, 60)));
            }
        }
    }
    if game.turn.is_acting() {
        let active = game.active_team_ref().active_soldier();
        if game.shotgun_shots_left > 0 && !active.has_fired {
            banners.push(("SHOT 2 - PRESS A".to_string(), Bgra::new(255, 180, 60)));
        }
        if game.revolver_shots_left > 0 {
            banners.push((format!("SHOTS: {}", game.revolver_shots_left), Bgra::new(255, 220, 80)));
        }
    }

    mark!("status");

    // 8d. Team avatars + health meters — small, top corners, screen-anchored
    {
        use crate::renderer::avatar::draw_avatar;
        use crate::renderer::draw_sprites::TEAM_COLOURS;
        const AV: u32 = 56;
        const BAR_H: i32 = 5;
        const BAR_GAP: i32 = 3;
        // Row height for teams 3/4, stacked below the top pair (avatar + bar + labels).
        const ROW_H: i32 = AV as i32 + BAR_GAP + BAR_H + 24;

        let active_ti = game.active_team();
        for ti in 0..game.teams.len().min(4) {
            let t = &game.teams[ti];
            let av_x = if ti % 2 == 0 {
                cam_x as i32 + 4
            } else {
                cam_x as i32 + sw - AV as i32 - 4
            };
            let av_y = cam_y as i32 + 4 + (ti as i32 / 2) * ROW_H;
            draw_avatar(buf, av_x, av_y, AV, t.avatar_id);
            // Active team gets a thin outline in its colour so the turn owner is
            // visible without the old bottom-bar strips.
            if ti == active_ti {
                let oc = Bgra::new(255, 220, 0);
                buf.fill_rect(av_x - 2, av_y - 2, AV + 4, 2, oc);
                buf.fill_rect(av_x - 2, av_y + AV as i32, AV + 4, 2, oc);
                buf.fill_rect(av_x - 2, av_y, 2, AV, oc);
                buf.fill_rect(av_x + AV as i32, av_y, 2, AV, oc);
            }

            // Health meter: total current HP / (soldiers.len() * 100)
            let max_hp = (t.soldiers.len() * 100) as u32;
            let cur_hp = t.total_hp().min(max_hp);
            let bar_y  = av_y + AV as i32 + BAR_GAP;
            let bar_w  = AV as i32;
            buf.fill_rect(av_x, bar_y, bar_w as u32, BAR_H as u32, Bgra::new(20, 20, 30));
            if cur_hp > 0 {
                let filled = ((cur_hp as i64 * bar_w as i64) / max_hp as i64).max(1) as u32;
                buf.fill_rect(av_x, bar_y, filled, BAR_H as u32, TEAM_COLOURS[t.color_id as usize]);
            }

            // Alive count — beside the HP bar on the screen-centre side.
            {
                use crate::renderer::font::{draw_str, str_width};
                let alive_str = format!("x{}", t.alive_count());
                let ax = if ti % 2 == 0 { av_x + bar_w + 4 } else { av_x - str_width(&alive_str) - 4 };
                draw_str(buf, &alive_str, ax, bar_y - 1, TEAM_COLOURS[t.color_id as usize]);
            }

            // ELO — shown below HP bar for ranked matches
            if t.elo > 0 {
                use crate::renderer::font::{draw_str, str_width};
                let elo_str = format!("{}", t.elo);
                let ew = str_width(&elo_str);
                let ex = av_x + bar_w / 2 - ew / 2;
                let ey = bar_y + BAR_H + 2;
                draw_str(buf, &elo_str, ex, ey, TEAM_COLOURS[t.color_id as usize]);
            }

            // Ping — shown below right avatar (ti == 1) in live mode
            if ti == 1 && lstate.ping_ms > 0 {
                use crate::renderer::font::{draw_str, str_width};
                let ping_colour = match lstate.ping_ms {
                    0..=80   => Bgra::new(80, 220, 80),   // green
                    81..=150 => Bgra::new(220, 220, 80),  // yellow
                    _        => Bgra::new(220, 80, 80),   // red
                };
                let ping_str = format!("{}ms", lstate.ping_ms);
                let pw = str_width(&ping_str);
                let elo_offset = if t.elo > 0 { 10 } else { 0 };
                let px = av_x + bar_w / 2 - pw / 2;
                let py = bar_y + BAR_H + 2 + elo_offset;
                draw_str(buf, &ping_str, px, py, ping_colour);
            }
        }
    }

    mark!("avatars");

    // 8b/8c. Top-of-screen banner stack — TNT/prompts (collected above) followed
    // by game messages, drawn after avatars so banners appear on top. Max 3.
    {
        use crate::renderer::draw_sprites::TEAM_COLOURS;
        let cx = cam_x as i32 + sw / 2;

        // In live mode filter messages: hide crate-collection notices belonging to
        // the opponent so only the collecting player sees what they picked up.
        let msg_colour = |t: Option<usize>| -> Bgra {
            match t {
                Some(t) => TEAM_COLOURS[game.teams.get(t).map(|tm| tm.color_id as usize).unwrap_or(t.min(3))],
                None    => Bgra::new(255, 210, 50),
            }
        };
        for m in game.messages.iter().filter(|m| {
            match (my_team, m.team) {
                (Some(mine), Some(t)) => t == mine, // live: only own-team messages
                _ => true,
            }
        }) {
            banners.push((m.text.clone(), msg_colour(m.team)));
        }

        let mut top_y: i32 = 4;
        for (text, col) in banners.iter().take(3) {
            top_y += draw_banner(buf, text, *col, cx, sw, top_y);
        }
    }

    mark!("messages");

    // 8b. HUD — drawn at cam_x offset so it stays screen-anchored
    // HUD strips are keyed by colour identity (0-3 = Red/Blue/Green/Yellow), so
    // each player's strip shows in the colour they picked, regardless of their
    // compact team index.
    // Clear stale HUD pixels (wind meter / weapon name / FPS) left over from a
    // previous frame's camera position before redrawing the HUD this frame.
    buf.fill_deep_water_band();

    let cur_weapon = weapon_name(game.teams[game.active_team()].current_weapon());
    draw_hud_world(buf, cam_x, cam_y, &game.wind, game.turn.secs_remaining(),
        game.turn.turn_number, cur_weapon, lstate.display_fps);

    mark!("hud");
    mark!("weapon_indicator");

    // 9b. Seed display (TEST mode only) — upper-right corner, screen-anchored
    if game.is_test {
        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
        use crate::renderer::fb::Bgra;
        use crate::world::SCREEN_W;
        let map_label = if game.terrain.is_cavern {
            "CAVERNS".to_string()
        } else {
            format!("WA-{}", game.terrain.template_id)
        };
        let label = format!("SEED {:016X}  {}", game.map_seed, map_label);
        let w = str_width_scaled(&label, 2);
        let x = cam_x as i32 + SCREEN_W as i32 - w - 6;
        let y = cam_y as i32 + 6;
        buf.fill_rect(x - 3, y - 2, (w + 6) as u32, 18, Bgra::new(10, 10, 25));
        draw_str_scaled(buf, &label, x, y, Bgra::new(180, 220, 120), 2);
    }

    mark!("seed_display");

    mark!("fps_counter");

    // 9d. Per-section pixel-write breakdown (TEST mode only) — top sections by
    // pixel count, sorted descending, drawn below the seed display.
    if game.is_test {
        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
        use crate::renderer::fb::Bgra;
        use crate::world::SCREEN_W;
        let mut sorted = pixel_stats.clone();
        sorted.sort_by(|a, b| b.2.cmp(&a.2)); // hottest by time, not pixel count
        let total_pw: u64 = sorted.iter().map(|(_, n, _)| n).sum();
        let total_us: u64 = sorted.iter().map(|(_, _, t)| t).sum();
        let mut y = cam_y as i32 + 28;
        {
            let text = format!("{:<14}{:>7}{:>6}us", "TOTAL", total_pw, total_us);
            let w = str_width_scaled(&text, 1);
            let x = cam_x as i32 + SCREEN_W as i32 - w - 6;
            buf.fill_rect(x - 2, y - 1, (w + 4) as u32, 10, Bgra::new(30, 10, 10));
            draw_str_scaled(buf, &text, x, y, Bgra::new(255, 180, 80), 1);
            y += 11;
        }
        {
            // Previous frame's fb blit (row flip + present) — timed in main.rs.
            let text = format!("{:<14}{:>7}{:>6}us", "blit(prev)", "-", lstate.blit_us);
            let w = str_width_scaled(&text, 1);
            let x = cam_x as i32 + SCREEN_W as i32 - w - 6;
            buf.fill_rect(x - 2, y - 1, (w + 4) as u32, 10, Bgra::new(10, 25, 10));
            draw_str_scaled(buf, &text, x, y, Bgra::new(180, 255, 180), 1);
            y += 11;
        }
        for (label, px, us) in sorted.iter().take(8) {
            let text = format!("{:<14}{:>7}{:>6}us", label, px, us);
            let w = str_width_scaled(&text, 1);
            let x = cam_x as i32 + SCREEN_W as i32 - w - 6;
            buf.fill_rect(x - 2, y - 1, (w + 4) as u32, 10, Bgra::new(10, 10, 25));
            draw_str_scaled(buf, &text, x, y, Bgra::new(220, 220, 220), 1);
            y += 10;
        }
    }

    lstate.pixel_stats = pixel_stats;
}

/// Display name for a weapon, shared by the HUD bar and anywhere else that
/// labels the current weapon.
pub fn weapon_name(weapon: crate::physics::projectile::WeaponKind) -> &'static str {
    use crate::physics::projectile::WeaponKind;
    match weapon {
        WeaponKind::Bazooka         => "BAZOOKA",
        WeaponKind::Grenade         => "GRENADE",
        WeaponKind::Shotgun         => "SHOTGUN",
        WeaponKind::ClusterBomb     => "CLUMP BOMB",
        WeaponKind::Tnt             => "TNT",
        WeaponKind::Landmine        => "MINE",
        WeaponKind::BananaBomb      => "BANANA BOMB",
        WeaponKind::Revolver        => "REVOLVER",
        WeaponKind::NinjaRope       => "GRAPPLE",
        WeaponKind::BaseballBat     => "BAT",
        WeaponKind::Blasthive       => "BLASTHIVE",
        WeaponKind::BlackHoleBomb   => "BLACK HOLE",
        WeaponKind::Minigun         => "MINIGUN",
        WeaponKind::Uzi             => "MAC-10",
        WeaponKind::Pistol          => "PISTOL",
        WeaponKind::PlasmaTorch     => "TORCH",
        WeaponKind::Jackhammer      => "JACKHAMMER",
        WeaponKind::Garcia          => "HAND OF JERRY",
        WeaponKind::AirStrike       => "AIR STRIKE",
        WeaponKind::HolyHandGrenade => "SACRED ORD.",
        WeaponKind::MolotovCocktail => "MOLOTOV",
        WeaponKind::HomingMissile   => "HOMING MSL.",
        WeaponKind::Jumpbot         => "JUMPBOT",
        WeaponKind::SuperSheep      => "SUPER BOT",
        _ => "WEAPON",
    }
}

/// Draw one top-centre banner box (black, bordered, centred, word-wrapped) and
/// return the vertical space it consumed. Shared by the TNT fuse countdown,
/// action prompts, game messages, and the live disconnect banner (main.rs).
pub fn draw_banner(buf: &mut WorldBuffer, text: &str, col: Bgra, cx: i32, sw: i32, y: i32) -> i32 {
    use crate::renderer::font::{draw_str_scaled, str_width_scaled, wrap_text};
    const PAD: i32 = 4;
    let max_w = sw - PAD * 2;
    // Keep the big scale=2 font: fit on one line, else word-wrap into 2 lines.
    // Only drop to scale=1 if it still won't fit (very long text / single words).
    let (scale, lines) = if str_width_scaled(text, 2) <= max_w {
        (2, vec![text.to_string()])
    } else {
        let w2 = wrap_text(text, 2, max_w);
        if w2.len() <= 2 && w2.iter().all(|l| str_width_scaled(l, 2) <= max_w) {
            (2, w2)
        } else {
            (1, wrap_text(text, 1, max_w))
        }
    };
    if lines.is_empty() { return 0; }

    let line_h  = 8 * scale;
    let gap     = scale;
    let n       = lines.len() as i32;
    let block_h = n * line_h + (n - 1) * gap;
    let box_w   = lines.iter().map(|l| str_width_scaled(l, scale)).max().unwrap_or(0);
    let bx      = cx - box_w / 2;
    buf.fill_rect(bx - PAD, y - PAD + 1, (box_w + PAD * 2) as u32, (block_h + PAD * 2) as u32, Bgra::new(0, 0, 0));
    buf.fill_rect(bx - PAD, y - PAD + 1, (box_w + PAD * 2) as u32, 1, Bgra::new(60, 60, 80));
    buf.fill_rect(bx - PAD, y + block_h + PAD, (box_w + PAD * 2) as u32, 1, Bgra::new(60, 60, 80));
    let mut ly = y;
    for line in &lines {
        let lw = str_width_scaled(line, scale);
        let lx = cx - lw / 2;
        draw_str_scaled(buf, line, lx + 1, ly + 1, Bgra::new(0, 0, 0), scale);
        draw_str_scaled(buf, line, lx,     ly,     col, scale);
        ly += line_h + gap;
    }
    block_h + PAD * 2 + 3
}

/// HUD drawn at world-space x=cam_x so it stays fixed on screen during panning.
fn draw_hud_world(
    buf:         &mut WorldBuffer,
    cam_x:       u32,
    cam_y:       u32,
    wind:        &crate::physics::Wind,
    turn_secs:   u32,
    turn_number: u32,
    weapon:      &str,
    fps:         u32,
) {
    use crate::renderer::font::{draw_str, draw_str_shadow, str_width};

    let ox    = cam_x as i32;
    let hud_y = cam_y as i32 + crate::renderer::HUD_Y;
    let sw    = crate::world::SCREEN_W as i32;

    // Background bar
    buf.fill_rect(ox, hud_y, crate::world::SCREEN_W,
        crate::renderer::HUD_H, Bgra::new(15, 15, 25));

    // Current weapon + menu hint (left)
    draw_str_shadow(buf, weapon, ox + 4, hud_y + 6, Bgra::new(255, 220, 80));
    draw_str(buf, "[SEL]", ox + 4 + str_width(weapon) + 8, hud_y + 6, Bgra::new(110, 110, 150));

    // Timer
    let timer_str    = format!("{:02}", turn_secs);
    let timer_colour = if turn_secs <= 5 { Bgra::new(220, 60, 60) } else { Bgra::new(255, 220, 0) };
    let timer_x      = ox + sw - str_width(&timer_str) - 4;
    draw_str(buf, &timer_str, timer_x, hud_y + 6, timer_colour);

    // Turn number
    let turn_str = format!("T{}", turn_number);
    let turn_x   = timer_x - str_width(&turn_str) - 6;
    draw_str(buf, &turn_str, turn_x, hud_y + 6, Bgra::new(220, 220, 220));

    // FPS — in-bar, left of the turn number
    let fps_str = format!("{} FPS", fps);
    draw_str(buf, &fps_str, turn_x - str_width(&fps_str) - 10, hud_y + 6, Bgra::new(140, 140, 160));

    // Wind meter — WA style: 180 px bar, centre line, one arrow per wind unit
    // (left = blue, right = red), each arrow ~7.4 px wide, up to 10 arrows.
    let bar_w = 180i32;
    let bar_h = 10u32;
    let bar_x = ox + sw / 2 - bar_w / 2;
    let bar_y = hud_y + 5;
    let centre_x = bar_x + bar_w / 2;
    let units = wind.units();
    let rightward = units >= 0;
    let (base, light) = if rightward {
        (Bgra::new(240, 96, 64), Bgra::new(248, 128, 104))
    } else {
        (Bgra::new(72, 88, 192), Bgra::new(136, 136, 248))
    };
    buf.fill_rect(bar_x - 1, bar_y - 1, (bar_w + 2) as u32, bar_h + 2, Bgra::new(230, 230, 230));
    buf.fill_rect(bar_x, bar_y, bar_w as u32, bar_h, Bgra::new(0, 0, 0));
    const UNIT_PX: f32 = 7.4;
    let n = units.unsigned_abs() as i32;
    for i in 0..n {
        let x0 = (i as f32 * UNIT_PX).round() as i32;
        let x1 = (((i + 1) as f32) * UNIT_PX).round() as i32;
        for dx in x0..x1 {
            // distance from the arrow's tip edge, used to draw a ">" / "<" chevron
            let from_tail = dx - x0;
            for row in 0..bar_h as i32 {
                let off = (row - bar_h as i32 / 2).abs();
                let on_chevron = (from_tail - (bar_h as i32 / 2 - off).max(0)).abs() <= 1;
                let c = if on_chevron { light } else { base };
                let px = if rightward { centre_x + dx } else { centre_x - dx - 1 };
                buf.fill_rect(px, bar_y + row, 1, 1, c);
            }
        }
    }
    // Centre line
    buf.fill_rect(centre_x, bar_y, 1, bar_h, Bgra::new(230, 230, 230));
}

/// Re-stamp the object mask each tick from current barrel and armed-mine positions.
/// Must be called before any collision checks so soldiers and projectiles treat
/// barrels and armed mines as solid obstacles (Worms-style object mask).
/// Map-object art scale: soldiers are drawn at `drawn_soldier_height()` px for a 27 px WA worm (extracted from Gfx.dir:
/// wwalk 18x27, mine 16x8, oil drum 24x36), so mines and drums use the same factor to stay in WA's proportions. Art only:
/// collision footprints (see `stamp_objects`) keep WA's real size.
pub fn object_art_scale() -> f32 { crate::renderer::draw_sprites::drawn_soldier_height() / 27.0 * OBJECT_ART_TRIM }

/// Tuned on device: the full soldier ratio (~1.7x) read as giant next to the soldiers (their art carries padding that WA's tight
/// 27 px worm does not), so objects use 70% of it (~1.2x): a drum is about as tall as a soldier, a mine about half his width.
pub const OBJECT_ART_TRIM: f32 = 0.7;

/// Fill a rect given in WA pixels relative to an object's foot point (`fx`,`fy`), scaled by `object_art_scale`.
fn obj_rect(buf: &mut crate::renderer::buffer::WorldBuffer, fx: i32, fy: i32, x: i32, y: i32, w: i32, h: i32, c: Bgra) {
    let k = object_art_scale();
    let x0 = fx + (x as f32 * k).round() as i32;
    let y0 = fy + (y as f32 * k).round() as i32;
    let x1 = fx + ((x + w) as f32 * k).round() as i32;
    let y1 = fy + ((y + h) as f32 * k).round() as i32;
    buf.fill_rect(x0, y0, (x1 - x0).max(1) as u32, (y1 - y0).max(1) as u32, c);
}

/// Oil drum footprint (WA shape 20x31; width trimmed to 17 because WA's frame carries padding). Drawn and collided at the same size.
pub const BARREL_HALF_W: i32 = 8;
pub const BARREL_H: i32 = 31;

fn stamp_objects(game: &mut GameState) {
    use crate::game::state::MineState;
    game.terrain.clear_objects();

    // Barrels: BARREL_H-tall footprint exactly matching the drawn drum (pos.y = terrain surface, extends up to pos.y-BARREL_H).
    for barrel in &game.barrels {
        let cx = barrel.pos.x as i32;
        let cy = barrel.pos.y as i32;
        for dy in -BARREL_H..=0i32 {
            for dx in -BARREL_HALF_W..=BARREL_HALF_W {
                game.terrain.stamp_object(cx + dx, cy + dy);
            }
        }
    }

    // Scenery is baked directly into terrain.solid at generation time (see
    // Terrain::generate_tactical / renderer::scenery::scenery_pixels), so it's
    // already solid ground like any other dirt pixel — no per-tick object
    // stamp needed here (unlike barrels/mines, which move and must be
    // re-stamped every tick).

    // Armed / triggered mines: 8×8 px footprint
    for mine in &game.mines {
        if matches!(mine.state, MineState::Armed | MineState::Triggered { .. }) {
            let cx = mine.pos.x as i32;
            let cy = mine.pos.y as i32;
            for dy in -4..=4i32 {
                for dx in -4..=4i32 {
                    game.terrain.stamp_object(cx + dx, cy + dy);
                }
            }
        }
    }
}

/// How far past a side edge a falling worm may drift before it is declared drowned (WA: no limit, it just falls).
const WA_EDGE_FALL_MARGIN: i32 = 1500;

/// One 30 Hz tick of WA's airborne worm physics for a non-rope airborne soldier (see `physics::wa_worm`).
/// 5 WA frames per 3 ticks. Landing/fall damage/drowning/edge death reuse Arty's existing rules.
fn wa_air_tick(game: &mut GameState, ti: usize, si: usize, vel: crate::world::Vec2, mut spinning: bool) {
    use crate::physics::wa_missile::WaMissile;
    use crate::physics::wa_worm as ww;
    let (mut body, was_synced) = match game.teams[ti].soldiers[si].wa {
        Some((b, v)) if v == vel => (b, true),
        _ => {
            let p = game.teams[ti].soldiers[si].pos;
            (ww::body_from_foot(p.x, p.y, ww::vel_to_16(vel.x), ww::vel_to_16(vel.y)), false)
        }
    };
    let _ = was_synced;
    let airtime = game.teams[ti].soldiers[si].airtime;
    game.teams[ti].soldiers[si].airtime += 1;
    if spinning && airtime + 1 >= 20 { spinning = false; }
    let (_, y0) = ww::foot_of(&body);
    game.teams[ti].soldiers[si].fall.update(y0);
    // Other living, non-airborne soldiers act as solid bodies (WA worm-vs-worm), as integer WA body-centre pixels.
    // (Rig capture: a worm launched from below flies through its launcher; approximated as: a rising worm ignores airborne ones.)
    let rising = body.vy < 0;
    let mut other_ids: Vec<(usize, usize)> = Vec::new();
    let mut others: Vec<(i32, i32)> = Vec::new();
    if game.teams[ti].soldiers[si].on_fire_ticks == 0 {
        for (oti, t) in game.teams.iter().enumerate() {
            for (osi, o) in t.soldiers.iter().enumerate() {
                if (oti == ti && osi == si) || !o.is_alive() || o.on_fire_ticks > 0 { continue; }
                if rising && matches!(o.state, SoldierState::Airborne { .. }) { continue; }
                other_ids.push((oti, osi));
                others.push((o.pos.x as i32, o.pos.y as i32 - ww::FOOT_OFFSET));
            }
        }
    }
    let others: &[(i32, i32)] = &others;
    let mut launches: Vec<((usize, usize), i32)> = Vec::new();
    let terrain = &game.terrain;
    let solid = |x: i32, y: i32| terrain.is_blocked(x, y);
    let mut landed = false;
    let mut impact_vy = 0;
    let mut prev_foot = ww::foot_of(&body);
    for _ in 0..WaMissile::frames_for_age(airtime) {
        prev_foot = ww::foot_of(&body);
        impact_vy = impact_vy.max(body.vy.wrapping_add(ww::GRAVITY16)); // speed at the hit (gravity is applied first)
        let f = body.step_air_with(&solid, others);
        if let Some((i, vy)) = f.launched { launches.push((other_ids[i], vy)); }
        if f.landed { landed = true; break; }
        if (body.x >> 16) < -WA_EDGE_FALL_MARGIN || (body.x >> 16) >= crate::world::WORLD_W as i32 + WA_EDGE_FALL_MARGIN { break; }
    }
    // The stop rule can fire in a sweep and leave the body a pixel or two clear of whatever it hit (a flank bounce off another
    // worm, say). WA then simply finds nothing under it next frame and keeps falling. Only count the landing if the body really is
    // supported where it ended up; otherwise it carries on airborne with the (zeroed) velocity and falls under gravity next tick,
    // instead of being declared Idle in mid-air beside the other soldier.
    if landed && !body.supported_with(&solid, others) { landed = false; }
    for ((oti, osi), vy16) in launches {
        let o = &mut game.teams[oti].soldiers[osi];
        let v = crate::world::Vec2::new(0.0, ww::vel_from_16(vy16));
        let (ox, oy) = (o.pos.x, o.pos.y);
        o.fall.begin_fall(oy);
        o.airtime = 0;
        o.wa = Some((ww::body_from_foot(ox, oy, 0, vy16), v));
        o.state = SoldierState::Airborne { vel: v, spinning: false };
    }
    let (fx, fy) = ww::foot_of(&body);
    let ati = game.active_team();
    let is_act = ti == ati && si == game.teams[ati].active;
    // WA has no side walls (rig 2026-11-20, findings/physics.md "Walk-off"): a worm past the map edge is over empty land, keeps
    // falling under gravity and drowns at the water line. Only a worm flung absurdly far out is cut off early (same fate).
    if fx < -(WA_EDGE_FALL_MARGIN as f32) || fx >= (crate::world::WORLD_W as i32 + WA_EDGE_FALL_MARGIN) as f32
        || ww::drowned(&body, crate::world::WATER_Y as i32) {
        let s = &mut game.teams[ti].soldiers[si];
        s.pos.x = fx.clamp(0.0, crate::world::WORLD_W as f32 - 1.0); // splash/grave drawn inside the map
        s.pos.y = fy.max(0.0);
        s.death_cause = crate::game::soldier::DeathCause::Water;
        s.take_damage(999);
        s.state = SoldierState::Dead;
        s.wa = None;
        if is_act { game.active_worm_hit = true; }
        return;
    }
    game.teams[ti].soldiers[si].pos.x = fx;
    game.teams[ti].soldiers[si].pos.y = fy.max(0.0);
    if !landed {
        game.teams[ti].soldiers[si].wa_vis = Some((prev_foot.0, prev_foot.1, (5 * (airtime + 1) % 3) as f32 / 3.0));
        let nv = crate::world::Vec2::new(ww::vel_from_16(body.vx), ww::vel_from_16(body.vy));
        let s = &mut game.teams[ti].soldiers[si];
        s.wa = Some((body, nv));
        s.state = SoldierState::Airborne { vel: nv, spinning };
        return;
    }
    // Landed.
    let _ = game.teams[ti].soldiers[si].fall.land(fy);
    let dmg = ww::fall_damage(impact_vy) as u32; // WA: from impact speed (FUN_0050dc00), not fall distance
    if dmg > 0 {
        crate::dlog!("fall", "tick={} team={} s={} dmg={} impact_vy={:.2}px/frame airtime={} rope_session={} rope={} foot=({:.1},{:.1})",
            game.tick, ti, si, dmg, impact_vy as f64 / 65536.0, airtime, game.rope_session, game.rope.is_some(), fx, fy);
    }
    if dmg > 0 {
        game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Fall;
        game.teams[ti].soldiers[si].take_damage(dmg);
        if is_act { game.active_worm_hit = true; }
    }
    let bat_dmg = game.teams[ti].soldiers[si].pending_bat_damage;
    if bat_dmg > 0 {
        game.teams[ti].soldiers[si].pending_bat_damage = 0;
        game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Explosion;
        game.teams[ti].soldiers[si].kill_weapon = Some(crate::physics::WeaponKind::BaseballBat);
        game.teams[ti].soldiers[si].take_damage(bat_dmg);
        if is_act { game.active_worm_hit = true; }
    }
    let ivy = ww::vel_from_16(impact_vy);
    if ivy > 2.0 {
        let n = (2.0 + ivy.min(8.0) * 0.6) as u32;
        game.emit_fx(crate::renderer::fx::FxEvent::Dust { x: fx, y: fy.max(0.0) + 3.0, count: n, kick: 0.8, dir: 0.0 });
    }
    let s = &mut game.teams[ti].soldiers[si];
    s.airtime = 0;
    s.wa = None;
    s.state = if s.is_dead() { SoldierState::Dead } else { SoldierState::Idle };
    if !s.is_dead() { s.wa_act = Some(crate::game::soldier::WaAct::Recover { age: 0 }); }
    // Rope session: landing ends the swing but NOT the turn (free movement tool), as in the old landing code.
    if game.rope_session && is_act {
        game.rope = None;
        game.rope_retreat_ticks = ROPE_RETREAT_TICKS;
    }
}

fn apply_all_gravity(game: &mut GameState, input: &InputState) {
    // ── Hook flight: advance hook each tick, check for terrain attachment ────
    let ati = game.active_team();
    let asi = game.teams[ati].active;
    if let Some(ref mut rope) = game.rope {
        if rope.flying {
            let mut hx = rope.hook.x;
            let mut hy = rope.hook.y;
            let hvx = rope.hook_vel.x;
            let hvy = rope.hook_vel.y;
            let steps = (hvx.abs().max(hvy.abs()) as u32).max(1);
            let sx = hvx / steps as f32;
            let sy = hvy / steps as f32;
            let mut attached = false;
            for _ in 0..steps {
                hx += sx;
                hy += sy;
                let soldier_pos = game.teams[ati].soldiers[asi].pos;
                let tdx = hx - soldier_pos.x;
                let tdy = hy - soldier_pos.y;
                if hx < 0.0 || hx >= crate::world::WORLD_W as f32
                    || hy < 0.0
                    || hy >= crate::world::WATER_Y as f32
                    // Reference caps the hook's ray-march at max rope length
                    // (FUN_00519a60 stops at maxlen [0x98]) — out of rope = miss.
                    || (tdx * tdx + tdy * tdy).sqrt() > ROPE_MAX_LEN
                {
                    // Missed — cancel rope
                    game.rope = None;
                    attached = true; // use flag to break without reborrow
                    break;
                }
                if game.terrain.is_solid(hx as i32, hy as i32) {
                    let dx = tdx;
                    let dy = tdy;
                    let dist = (dx * dx + dy * dy).sqrt().max(1.0).min(ROPE_MAX_LEN);
                    let anchor = crate::world::WorldPos::new(hx, hy);
                    // Reference (FUN_005008d0): a hit closer than 10 px does not attach.
                    let a16 = ((hx * 65536.0) as i32, (hy * 65536.0) as i32);
                    let theta = crate::physics::wa_rope::Chain::angle_of(-(tdx * 65536.0) as i32, -(tdy * 65536.0) as i32);
                    let chain = if dist >= ROPE_MIN_LEN {
                        crate::physics::wa_rope::Chain::attach(a16.0, a16.1, (dist * 65536.0) as i32, theta)
                    } else { None };
                    crate::dlog!("rope", "tick={} hook hit ({:.0},{:.0}) len={:.0} attached={}", game.tick, hx, hy, dist, chain.is_some());
                    if chain.is_none() {
                        game.rope = None;
                        attached = true;
                        break;
                    }
                    if let Some(ref mut r) = game.rope {
                        r.flying = false;
                        r.anchor = anchor;
                        r.hook   = anchor;
                        r.length = dist;
                        r.chain  = chain;
                    }
                    // Reference: attaching gives no velocity kick; the worm just starts
                    // swinging from wherever it is (state 0x7b leaves velocity alone).
                    if matches!(game.teams[ati].soldiers[asi].state, SoldierState::Idle | SoldierState::Walking { .. }) {
                        let spos = game.teams[ati].soldiers[asi].pos;
                        game.teams[ati].soldiers[asi].fall.begin_fall(spos.y);
                        game.teams[ati].soldiers[asi].state = SoldierState::Airborne {
                            vel: crate::world::Vec2::new(0.0, 0.0),
                            spinning: false,
                        };
                        rope_unstick(game, ati, asi);
                    }
                    attached = true;
                    break;
                }
            }
            if !attached {
                if let Some(ref mut r) = game.rope {
                    r.hook = crate::world::WorldPos::new(hx, hy);
                }
            }
        }
    }

    for ti in 0..game.teams.len() {
        for si in 0..game.teams[ti].soldiers.len() {
            // Dead airborne soldiers keep falling until they land (then state→Dead).
            // Dead+grounded soldiers skip physics entirely.
            if game.teams[ti].soldiers[si].is_dead()
                && !matches!(game.teams[ti].soldiers[si].state, SoldierState::Airborne { .. })
            { continue; }
            // Soldiers held inside a black hole's event horizon are pinned by
            // step_black_holes(); skip gravity so they can't fall, drown, or take
            // fall damage and vanish before the hole collapses (collapse deals the
            // 35 dmg + ejects them). EVENT_HORIZON = 13px (see step_black_holes).
            if !game.black_holes.is_empty() {
                let sp = game.teams[ti].soldiers[si].pos;
                let pinned = game.black_holes.iter().any(|h| {
                    let dx = h.pos.x - sp.x;
                    let dy = h.pos.y - sp.y;
                    dx * dx + dy * dy < 13.0 * 13.0
                });
                if pinned { continue; }
            }
            let support_now = support(game, ti, si);
            let on_ground = support_now.is_firm();
            let state = game.teams[ti].soldiers[si].state.clone();
            match state {
                SoldierState::Airborne { mut vel, mut spinning } => {
                    // Watchdog: if knockback (e.g. an explosion) left the soldier
                    // embedded in solid terrain — a cramped cavern ceiling is the
                    // common case — the swept-movement loop below reverts to this
                    // exact spot every tick since its very first sub-step is already
                    // blocked. The soldier never lands, `state` stays Airborne
                    // forever, and the turn can never advance (see the
                    // `all_grounded` gate in simulate_with_muzzle). Force an escape
                    // before running normal physics.
                    if !unstick_embedded_soldier(game, ti, si) {
                        // No clear spot found nearby — give up gracefully rather
                        // than hang the turn forever.
                        game.teams[ti].soldiers[si].state = SoldierState::Idle;
                        game.teams[ti].soldiers[si].airtime = 0;
                        continue;
                    }
                    // General watchdog: some other stuck-airborne case (not
                    // terrain-embedded — e.g. a soldier-vs-soldier collision that
                    // zeroes displacement every sub-step) can leave `state` frozen
                    // in Airborne indefinitely with no legitimate fall taking this
                    // long (world height / terminal velocity is a few hundred
                    // ticks at most). Same "give up gracefully" fallback as the
                    // embedded-terrain case above, just on a longer fuse so it
                    // never cuts off a real fall.
                    if game.teams[ti].soldiers[si].airtime > 900 {
                        game.teams[ti].soldiers[si].state = SoldierState::Idle;
                        game.teams[ti].soldiers[si].airtime = 0;
                        continue;
                    }
                    // ── Rope constraint physics (active soldier only) ─────────
                    let is_active = ti == ati && si == asi;
                    if is_active {
                        // WA: a perfectly-vertical rope while the soldier touches the
                        // ground auto-detaches (the soldier just stands up). vdy>0 means
                        // the soldier hangs below the anchor; 2px matches corner-wrap epsilon.
                        // Suppressed while Down is held: the player is actively paying out
                        // rope to lower themselves further, not standing up.
                        if let Some(rope) = game.rope.as_ref().filter(|r| !r.flying) {
                            let vdx = game.teams[ti].soldiers[si].pos.x - rope.anchor.x;
                            let vdy = game.teams[ti].soldiers[si].pos.y - rope.anchor.y;
                            if on_ground && vdy > 0.0 && vdx.abs() < 2.0 && !input.held(Button::Down) {
                                game.rope = None;
                            }
                        }
                        // Push the soldier out of any terrain it's embedded in BEFORE the
                        // swing math, so the swept collision starts from a clear point and
                        // can't pin an embedded soldier at zero velocity (the ground-stick).
                        if game.rope.as_ref().map_or(false, |r| !r.flying) {
                            rope_unstick(game, ti, si);
                        }
                        if let Some(mut chain) = game.rope.as_ref().filter(|r| !r.flying).and_then(|r| r.chain.clone()) {
                            use crate::physics::wa_rope as wr;
                            let cx = game.teams[ti].soldiers[si].pos.x;
                            let cy = game.teams[ti].soldiers[si].pos.y;
                            let mut pos16 = ((cx * 65536.0) as i32, (cy * 65536.0) as i32);
                            let mut v16 = ((vel.x * 65536.0) as i32, (vel.y * 65536.0) as i32);
                            let (pos_in, v_in) = (pos16, v16);
                            // Reference per-frame order: swing impulse (vx and +vy), world
                            // gravity, then damping + tangential arc motion inside step().
                            let sdir = input.held(Button::Right) as i32 - input.held(Button::Left) as i32;
                            if sdir != 0 {
                                v16.0 += sdir * wr::TICK_SWING;
                                v16.1 += wr::TICK_SWING;
                            }
                            v16.1 += wr::TICK_GRAVITY;
                            let outcome = {
                                let env = RopeEnv(&game.terrain);
                                chain.step(&mut pos16, &mut v16, ROPE_MAX_SPEED as i32 * 65536, &env)
                            };
                            // Diagnostics (dlog only): once per ~10 ticks while roped, and EVERY tick a swing step leaves the body where it was
                            // (blocked by the body test, a wrap/unwrap event or a stale anchor) so a "slammed into the ground after climbing a
                            // slope on the rope" report (fall dmg 21 @ 15.6 px/frame, 2026-11-24) can be traced from arty-exp.log.
                            if pos16 == pos_in || game.tick % 10 == 0 {
                                crate::dlog!("rope", "tick={} step pos=({:.1},{:.1}){} v=({:.2},{:.2})->({:.2},{:.2}) len={:.1} nseg={} out={:?} keys=[{}{}{}{}]",
                                    game.tick, nx_dbg(pos16.0), nx_dbg(pos16.1), if pos16 == pos_in { " UNMOVED" } else { "" },
                                    nx_dbg(v_in.0), nx_dbg(v_in.1), nx_dbg(v16.0), nx_dbg(v16.1), chain.len as f32 / 65536.0, chain.nseg, outcome,
                                    if input.held(Button::Left) { "L" } else { "" }, if input.held(Button::Right) { "R" } else { "" },
                                    if input.held(Button::Up) { "U" } else { "" }, if input.held(Button::Down) { "D" } else { "" });
                            }
                            // A swing step that could not move the body (pressed against the slope: the body test blocks it, WA leaves pos AND v
                            // untouched) must not bank speed: gravity + damping run every frame, so a worm held there reached 20+ px/frame and was
                            // slammed into the ground on release (user report 2026-11-24, touching a slope on a 14 px rope: fall dmg 21 @ 15.6 px/frame).
                            // tests/rope_slope.rs reproduces it (vy 25.5 px/tick without this).
                            if pos16 == pos_in && outcome != wr::Outcome::Released {
                                let m = |v: (i32, i32)| (v.0 as i64).pow(2) + (v.1 as i64).pow(2);
                                if m(v16) > m(v_in) { v16 = v_in; }
                            }
                            vel.x = v16.0 as f32 / 65536.0;
                            vel.y = v16.1 as f32 / 65536.0;
                            if outcome == wr::Outcome::Released {
                                // Anchor / pivot terrain destroyed: the rope drops.
                                crate::dlog!("rope", "tick={} released (anchor/pivot terrain gone) at ({:.0},{:.0})", game.tick, nx_dbg(pos16.0), nx_dbg(pos16.1));
                                game.rope = None;
                                game.teams[ti].soldiers[si].airtime += 1;
                                game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel, spinning };
                                continue;
                            }
                            let nx = pos16.0 as f32 / 65536.0;
                            let ny = pos16.1 as f32 / 65536.0;
                            rope_store_chain(game, chain);
                            let (last_clear_x, last_clear_y) = (cx, cy);
                            let hit = false;
                            if hit {
                                // Swinging into a wall — stop at the last clear point but stay
                                // attached to the rope (no sticking/landing); gravity + rope
                                // tension will pull the soldier away from the wall next tick.
                                vel.x = 0.0;
                                vel.y = 0.0;
                                game.teams[ti].soldiers[si].pos.x = last_clear_x;
                                game.teams[ti].soldiers[si].pos.y = last_clear_y;
                                game.teams[ti].soldiers[si].airtime += 1;
                                game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel, spinning };
                            } else if ny >= crate::world::WATER_Y as f32 {
                                // Drowned
                                game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Water;
                                game.teams[ti].soldiers[si].take_damage(999);
                                game.teams[ti].soldiers[si].state = SoldierState::Dead;
                                let ati = game.active_team();
                                if ti == ati && si == game.teams[ati].active { game.active_worm_hit = true; }
                                game.rope = None;
                                game.rope_session = false;
                            } else if nx < -(WA_EDGE_FALL_MARGIN as f32) || nx >= (crate::world::WORLD_W as i32 + WA_EDGE_FALL_MARGIN) as f32 {
                                // Swung absurdly far past a side edge (WA has no side walls: the swing just continues over empty land) — drown/die
                                game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Water;
                                game.teams[ti].soldiers[si].take_damage(999);
                                let ati = game.active_team();
                                if ti == ati && si == game.teams[ati].active { game.active_worm_hit = true; }
                                game.teams[ti].soldiers[si].state = SoldierState::Dead;
                                game.rope = None;
                                game.rope_session = false;
                            } else {
                                game.teams[ti].soldiers[si].pos.x = nx;
                                game.teams[ti].soldiers[si].pos.y = ny;
                                game.teams[ti].soldiers[si].airtime += 1;
                                game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel, spinning };
                                // Rope-knocking: a fast-moving roper shoves other soldiers
                                // it collides with. Fixed index order for replay determinism.
                                let roper_speed = (vel.x * vel.x + vel.y * vel.y).sqrt();
                                if roper_speed >= ROPE_KNOCK_MIN_SPEED {
                                    use crate::renderer::draw_sprites::{BODY_W, BODY_H};
                                    let kvel = crate::world::Vec2::new(
                                        vel.x * ROPE_KNOCK_STRENGTH,
                                        vel.y * ROPE_KNOCK_STRENGTH,
                                    );
                                    for oti in 0..game.teams.len() {
                                        for osi in 0..game.teams[oti].soldiers.len() {
                                            if oti == ti && osi == si { continue; }
                                            let os = &game.teams[oti].soldiers[osi];
                                            if !os.is_alive() { continue; }
                                            // Only knock a soldier that isn't already flying.
                                            if matches!(os.state, SoldierState::Airborne { .. }) { continue; }
                                            let ox = os.pos.x;
                                            let ohead = os.pos.y - BODY_H as f32;
                                            if (nx - ox).abs() < BODY_W as f32
                                                && (ny - BODY_H as f32 - ohead).abs() <= BODY_H as f32
                                            {
                                                game.teams[oti].soldiers[osi].state =
                                                    SoldierState::Airborne { vel: kvel, spinning: false };
                                            }
                                        }
                                    }
                                }
                            }
                            continue; // skip normal gravity below
                        }
                    }
                    // ── WA airborne physics (fixed point, 50 fps) ─────────────
                    // Rope sessions too: a released/flying-hook worm falls with WA's own flight physics (same gravity).
                    wa_air_tick(game, ti, si, vel, spinning);
                    continue;
                    // ── Normal airborne physics ───────────────────────────────
                    // During a rope session the active soldier falls at the WA worm
                    // rate (matching ROPE_GRAVITY) so detach arcs carry the swing's
                    // momentum instead of going floaty — covers detach flight, the
                    // hook-flying window, and re-attach gaps until landing.
                    let g = if is_active && game.rope_session { ROPE_AIR_GRAVITY } else { 0.5 };
                    vel.y = (vel.y + g).min(23.0); // raised cap to preserve rope-release / slingshot momentum
                    game.teams[ti].soldiers[si].airtime += 1;
                    // One full revolution = 4 frames × 5 ticks = 20 ticks, then stay upright
                    if spinning && game.teams[ti].soldiers[si].airtime >= 20 {
                        spinning = false;
                    }
                    // Spinning (backflip): fixed trajectory, no steering. Pure gravity.
                    let dx = vel.x;
                    let dy = vel.y;
                    let steps = (dx.abs().max(dy.abs()) as u32).max(1);
                    let sx_ = dx / steps as f32;
                    let sy_ = dy / steps as f32;
                    let mut cx = game.teams[ti].soldiers[si].pos.x;
                    let mut cy = game.teams[ti].soldiers[si].pos.y;
                    game.teams[ti].soldiers[si].fall.update(cy);
                    // Burning worms pass THROUGH other worms mid-hop: a clump of
                    // soldiers stuck together on a small peak could otherwise never
                    // hop past each other (each lands on a neighbour and stalls).
                    // They still land on terrain, so they separate onto the ground.
                    let self_burning = game.teams[ti].soldiers[si].on_fire_ticks > 0;
                    let mut landed = false;
                    for _ in 0..steps {
                        cx += sx_;
                        cy += sy_;
                        let ix = cx as i32;
                        let iy = cy as i32;
                        let ix_l = ix - crate::renderer::draw_sprites::BODY_HALF_W as i32;
                        let ix_r = ix + crate::renderer::draw_sprites::BODY_HALF_W as i32;
                        let terrain_hit = (0..=crate::renderer::draw_sprites::BODY_H)
                            .any(|h| game.terrain.is_blocked(ix_l, iy - h)
                                || game.terrain.is_blocked(ix,   iy - h)
                                || game.terrain.is_blocked(ix_r, iy - h));
                        let soldier_hit = !terrain_hit && !self_burning && game.teams.iter().enumerate().any(|(oti, oteam)| {
                            oteam.soldiers.iter().enumerate().any(|(osi, os)| {
                                if (oti == ti && osi == si) || !os.is_alive() { return false; }
                                let ox = os.pos.x as i32;
                                let oy = os.pos.y as i32;
                                (ix - ox).abs() < crate::renderer::draw_sprites::BODY_W as i32
                                    && iy >= oy - crate::renderer::draw_sprites::BODY_H as i32
                                    && iy <= oy + crate::renderer::draw_sprites::BODY_H as i32
                            })
                        });
                        if soldier_hit && dy >= 0.0 {
                            // Landed on another soldier. If the landing is mostly square on
                            // top (more than half the body width overlapping), stand on their
                            // head like a platform. Otherwise it's a glancing hit off the side
                            // — slide to the nearest clear side and settle on the terrain there.
                            let sw = crate::renderer::draw_sprites::BODY_W as i32;
                            let sh = crate::renderer::draw_sprites::BODY_H as i32;
                            // Find the soldier we landed on (need their x/y).
                            let mut other_cx = cx;
                            let mut other_oy = iy;
                            'find_other: for (oti, oteam) in game.teams.iter().enumerate() {
                                for (osi, os) in oteam.soldiers.iter().enumerate() {
                                    if (oti == ti && osi == si) || !os.is_alive() { continue; }
                                    let ox = os.pos.x as i32;
                                    let oy = os.pos.y as i32;
                                    if (ix - ox).abs() < sw && iy >= oy - sh && iy <= oy + 1 {
                                        other_cx = os.pos.x;
                                        other_oy = oy;
                                        break 'find_other;
                                    }
                                }
                            }
                            let overlap = sw - (ix - other_cx as i32).abs();
                            if overlap >= sw / 2 {
                                // Stand on top: feet land exactly at the other soldier's head.
                                cy = (other_oy - sh) as f32;
                                let dmg = game.teams[ti].soldiers[si].fall.land(cy);
                                if dmg > 0 {
                                    game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Fall;
                                    game.teams[ti].soldiers[si].take_damage(dmg);
                                    let ati = game.active_team();
                                    if ti == ati && si == game.teams[ati].active { game.active_worm_hit = true; }
                                }
                                let bat_dmg = game.teams[ti].soldiers[si].pending_bat_damage;
                                if bat_dmg > 0 {
                                    game.teams[ti].soldiers[si].pending_bat_damage = 0;
                                    game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Explosion;
                                    game.teams[ti].soldiers[si].kill_weapon = Some(crate::physics::WeaponKind::BaseballBat);
                                    game.teams[ti].soldiers[si].take_damage(bat_dmg);
                                    let ati = game.active_team();
                                    if ti == ati && si == game.teams[ati].active { game.active_worm_hit = true; }
                                }
                                game.teams[ti].soldiers[si].pos.x = cx;
                                game.teams[ti].soldiers[si].pos.y = cy.max(0.0);
                                game.teams[ti].soldiers[si].airtime = 0;
                                if game.teams[ti].soldiers[si].is_dead() {
                                    game.teams[ti].soldiers[si].state = SoldierState::Dead;
                                } else {
                                    game.teams[ti].soldiers[si].state = SoldierState::Idle;
                                }
                                if game.rope_session && ti == game.active_team() && si == game.teams[game.active_team()].active {
                                    game.rope = None;
                                    game.rope_retreat_ticks = ROPE_RETREAT_TICKS; // keep session alive for a retreat window
                                }
                                landed = true;
                                break;
                            }
                            let dir = if cx >= other_cx { 1.0 } else { -1.0 };
                            // Step sideways until clear of every other living soldier.
                            let mut tx = cx;
                            for _ in 0..(sw * 2) {
                                tx += dir;
                                let txi = tx as i32;
                                let still = game.teams.iter().enumerate().any(|(oti, oteam)| {
                                    oteam.soldiers.iter().enumerate().any(|(osi, os)| {
                                        if (oti == ti && osi == si) || !os.is_alive() { return false; }
                                        (txi - os.pos.x as i32).abs() < sw
                                    })
                                });
                                if !still { break; }
                            }
                            cx = tx.clamp(1.0, crate::world::WORLD_W as f32 - 1.0);
                            // Slid clear of the other soldier with no surface within 10 px below: he is beside the flank in open air,
                            // not landing. `land_on_surface` returns the current height when it finds nothing, which used to "land"
                            // him Idle in mid-air, pinned beside the other soldier and unable to act. Keep falling instead.
                            {
                                let hw = crate::renderer::draw_sprites::BODY_HALF_W as i32;
                                let (fx, fy) = (cx as i32, cy as i32);
                                let ground_below = (1..=10).any(|d| game.terrain.is_blocked(fx - hw, fy + d) || game.terrain.is_blocked(fx, fy + d) || game.terrain.is_blocked(fx + hw, fy + d));
                                if !ground_below { vel.x = 0.0; continue; }
                            }
                            cy = land_on_surface(&game.terrain, cx, cy) as f32;
                            let dmg = game.teams[ti].soldiers[si].fall.land(cy);
                            if dmg > 0 {
                                game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Fall;
                                game.teams[ti].soldiers[si].take_damage(dmg);
                                let ati = game.active_team();
                                if ti == ati && si == game.teams[ati].active { game.active_worm_hit = true; }
                            }
                            game.teams[ti].soldiers[si].pos.x = cx;
                            game.teams[ti].soldiers[si].pos.y = cy.max(0.0);
                            game.teams[ti].soldiers[si].airtime = 0;
                            if game.teams[ti].soldiers[si].is_dead() {
                                game.teams[ti].soldiers[si].state = SoldierState::Dead;
                            } else {
                                game.teams[ti].soldiers[si].state = SoldierState::Idle;
                            }
                            if game.rope_session && ti == game.active_team() && si == game.teams[game.active_team()].active {
                                game.rope = None;
                                game.rope_retreat_ticks = ROPE_RETREAT_TICKS; // keep session alive for a retreat window
                            }
                            landed = true;
                            break;
                        }
                        let hit = terrain_hit || soldier_hit;
                        if hit {
                            cx -= sx_;
                            cy -= sy_;
                            // Check if the x step alone caused the hit (wall, not floor/ceiling).
                            // If so, kill horizontal velocity and stay airborne so the soldier
                            // falls away from the wall instead of sticking to it.
                            let wall_only = !soldier_hit && dx != 0.0 && {
                                let wx = (cx + sx_) as i32;
                                let wy_c = cy as i32;
                                let wl = wx - (crate::renderer::draw_sprites::BODY_HALF_W as i32 - 1);
                                let wr = wx + (crate::renderer::draw_sprites::BODY_HALF_W as i32 - 1);
                                let x_hit = (0..=crate::renderer::draw_sprites::BODY_H)
                                    .any(|h| game.terrain.is_blocked(wl, wy_c - h)
                                        || game.terrain.is_blocked(wx, wy_c - h)
                                        || game.terrain.is_blocked(wr, wy_c - h));
                                let y_hit = (0..=crate::renderer::draw_sprites::BODY_H)
                                    .any(|h| game.terrain.is_blocked(ix_l, wy_c - h)
                                        || game.terrain.is_blocked(ix,   wy_c - h)
                                        || game.terrain.is_blocked(ix_r, wy_c - h));
                                x_hit && !y_hit
                            };
                            if wall_only {
                                vel.x = 0.0;
                                game.teams[ti].soldiers[si].pos.x = cx;
                                game.teams[ti].soldiers[si].pos.y = cy.max(0.0);
                                game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel, spinning };
                                landed = true;
                            } else if dy < 0.0 {
                                // Hit ceiling while going up — bounce, never push through
                                vel.y = vel.y.abs().max(0.5);
                                vel.x *= 0.5;
                                game.teams[ti].soldiers[si].pos.x = cx;
                                game.teams[ti].soldiers[si].pos.y = cy.max(0.0);
                                game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel, spinning };
                                landed = true;
                            } else {
                                // Moving downward — snap foot to surface, apply distance-based fall damage
                                cy = land_on_surface(&game.terrain, cx, cy) as f32;
                                {
                                    let dmg = game.teams[ti].soldiers[si].fall.land(cy);
                                    if dmg > 0 {
                                        game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Fall;
                                        game.teams[ti].soldiers[si].take_damage(dmg);
                                        let ati = game.active_team();
                                        if ti == ati && si == game.teams[ati].active {
                                            game.active_worm_hit = true;
                                        }
                                    }
                                    let bat_dmg = game.teams[ti].soldiers[si].pending_bat_damage;
                                    if bat_dmg > 0 {
                                        game.teams[ti].soldiers[si].pending_bat_damage = 0;
                                        game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Explosion;
                                        game.teams[ti].soldiers[si].kill_weapon = Some(crate::physics::WeaponKind::BaseballBat);
                                        game.teams[ti].soldiers[si].take_damage(bat_dmg);
                                        let ati = game.active_team();
                                        if ti == ati && si == game.teams[ati].active {
                                            game.active_worm_hit = true;
                                        }
                                    }
                                }
                                game.teams[ti].soldiers[si].pos.x = cx;
                                game.teams[ti].soldiers[si].pos.y = cy.max(0.0);
                                game.teams[ti].soldiers[si].airtime = 0;
                                // Landing dust puff — count scales with impact speed.
                                if vel.y > 2.0 {
                                    let n = (2.0 + vel.y.min(8.0) * 0.6) as u32;
                                    game.emit_fx(crate::renderer::fx::FxEvent::Dust {
                                        x: cx, y: cy.max(0.0) + 3.0, count: n, kick: 0.8, dir: 0.0,
                                    });
                                }
                                if game.teams[ti].soldiers[si].is_dead() {
                                    game.teams[ti].soldiers[si].state = SoldierState::Dead;
                                } else {
                                    game.teams[ti].soldiers[si].state = SoldierState::Idle;
                                }
                                // Rope session: landing ends the session but NOT the turn.
                                // Grapple is a free movement tool — player can still fire a weapon.
                                if game.rope_session && ti == game.active_team() && si == game.teams[game.active_team()].active {
                                    game.rope = None;
                                    game.rope_retreat_ticks = ROPE_RETREAT_TICKS; // keep session alive for a retreat window
                                    // turn continues — no on_fired()
                                }
                                landed = true;
                            }
                            break;
                        }
                        // Flew far past a side edge (WA: no wall, it keeps falling and drowns) — cut off only beyond the margin
                        if cx < -(WA_EDGE_FALL_MARGIN as f32) || cx >= (crate::world::WORLD_W as i32 + WA_EDGE_FALL_MARGIN) as f32 {
                            game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Water;
                            game.teams[ti].soldiers[si].take_damage(999);
                            let ati = game.active_team();
                            if ti == ati && si == game.teams[ati].active { game.active_worm_hit = true; }
                            game.teams[ti].soldiers[si].state = SoldierState::Dead;
                            landed = true;
                            break;
                        }
                    }
                    if !landed {
                        game.teams[ti].soldiers[si].pos.x = cx;
                        game.teams[ti].soldiers[si].pos.y = cy.max(0.0);
                        if cy >= crate::world::WATER_Y as f32 {
                            game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Water;
                            game.teams[ti].soldiers[si].take_damage(999);
                            let ati = game.active_team();
                            if ti == ati && si == game.teams[ati].active { game.active_worm_hit = true; }
                            game.teams[ti].soldiers[si].state = SoldierState::Dead;
                        } else {
                            game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel, spinning };
                        }
                    }
                }
                SoldierState::Idle => {
                    // Use the lenient ground check (no is_solid wall-guard) here, not
                    // the strict `on_ground`: on a thin terrain ridge the soldier's own
                    // foot pixel can itself register solid, which trips is_on_ground's
                    // wall-guard and reports not-grounded even though the Airborne arm's
                    // own landing sweep (is_blocked-based, no such guard) just placed the
                    // soldier there and considers it landed.
                    //
                    // Also exempt soldiers `unstick_embedded_soldier` already gave up on:
                    // that watchdog sets Idle in place when NO clear escape exists nearby
                    // (soldier buried too deep to reposition) specifically so `all_grounded`
                    // stops waiting on it. Without this exemption, this check immediately
                    // re-detects "not on real ground" at that same embedded spot and
                    // launches it back into Airborne — which fails to unstick for the exact
                    // same reason, gives up again, and the two fallbacks ping-pong forever
                    // at zero net displacement, softlocking the turn.
                    //
                    // Also exempt soldiers within the Airborne arm's own loose
                    // `soldier_hit` landing tolerance of another soldier (see
                    // `is_resting_near_soldier`) — otherwise two soldiers pinned
                    // together (e.g. both knocked to the map's y=0 ceiling) satisfy
                    // that landing check every Airborne tick but never satisfy the
                    // strict `is_on_soldier` here, causing the same ping-pong.
                    if !support_now.holds() {
                        let y0 = game.teams[ti].soldiers[si].pos.y;
                        game.teams[ti].soldiers[si].fall.begin_fall(y0);
                        game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel: crate::world::Vec2::new(0.0, 0.0), spinning: false };
                    } else if support_now == Support::NearSoldier {
                        // Overlapping another soldier's body with nothing under him (neither on terrain nor resting on him by
                        // WA's mask shapes): exempt from falling only so a pinned pair cannot ping-pong Idle<->Airborne. Left
                        // exempt it is a soldier hanging beside a neighbour in open air for the rest of the match, so separate
                        // the pair 1 px a tick instead; once clear he is no longer near anyone, `holds()` goes false and he falls.
                        separate_from_neighbour(game, ti, si);
                    } else if support_now == Support::Embedded {
                        // Embedded means: body overlaps terrain, yet nothing solid within 5 px under the feet (otherwise it would
                        // be Firm/Ridge). So it is a head or flank caught in an overhang or against a wall, with open air below
                        // - e.g. a rising worm that "landed" on a ceiling, which the old exemption left hanging in mid-air,
                        // unable to walk, jump or fall, until the turn timer ran out. Leaving him alone avoids relaunching a
                        // soldier the watchdog gave up on (the ping-pong described above), so instead let him sink a few px a tick:
                        // that moves the overlap off the ceiling, after which `holds()` is false and he falls normally, or he
                        // reaches support. Bounded: every tick either frees him or moves him towards the ground.
                        // (Tried `unstick_embedded_soldier` + Airborne here instead: it picks the nearest clear spot, which can be
                        // ABOVE (a 1 px bump on the body edge is 17 px of fall away but 6 px up), so he is teleported up, falls back
                        // onto the bump and repeats forever - worse than the hover, as the turn never settles. Sinking cannot loop.)
                        // Sink 3 px a tick (a fast drop, not a crawl). Nothing solid is within 5 px under his feet, so this is safe.
                        let sy = game.teams[ti].soldiers[si].pos.y;
                        if sy + 3.0 < crate::world::WATER_Y as f32 {
                            game.teams[ti].soldiers[si].pos.y = sy + 3.0;
                        } else {
                            // Down at the waterline: the world rules say anything at or below WATER_Y drowns.
                            let ati = game.active_team();
                            let s = &mut game.teams[ti].soldiers[si];
                            s.death_cause = crate::game::soldier::DeathCause::Water;
                            s.take_damage(999);
                            s.state = SoldierState::Dead;
                            s.wa = None;
                            if ti == ati && si == game.teams[ati].active { game.active_worm_hit = true; }
                        }
                    }
                }
                SoldierState::Dead | SoldierState::Walking { .. } => {}
            }
        }
    }

}

/// Per-frame visual stepping for the LIVE client, which renders authoritative
/// server state and never runs `simulate()`. Ages the crater-derived explosion
/// flashes and decays client-only display timers. Graves, soldier deaths, and
/// SFX are all server-authoritative — graves arrive in StateMsg, death sounds in
/// StateMsg.sounds — so this does NOT run record_deaths / step_death_explosions
/// (that would double the death sound and re-derive server state on the client).
/// TAT replay no longer calls this: its server_tick() runs the full shared core.
pub fn update_visuals(game: &mut GameState) {
    game.step_explosions();
    crate::renderer::fx::step_fx(&mut game.fx, &game.terrain, game.wind.value());
    crate::renderer::fx::step_fx_text(&mut game.fx_text);
    for team in &mut game.teams {
        for s in &mut team.soldiers {
            s.step_hp_display();
        }
    }
    game.messages.retain_mut(|m| { m.ticks = m.ticks.saturating_sub(1); m.ticks > 0 });
    game.bullet_trails.retain_mut(|t| { if t.2 > 0 { t.2 -= 1; true } else { false } });
}

/// Draw the weapon-selection overlay when game.weapon_menu_open is true.
/// Call after render() in TAT and live-mode client paths.
/// Compute kill/HP stats and pick a memorable one-liner for the end screen.
/// Returns ([team0_kills, team1_kills], [team0_hp, team1_hp], memorable_line).
pub fn match_end_stats(game: &GameState) -> (Vec<TeamEndStat>, String) {
    let stats: Vec<TeamEndStat> = game.teams.iter().map(|t| TeamEndStat {
        color_id: t.color_id,
        alive:    t.alive_count(),
        total:    t.soldiers.len() as u32,
        hp:       t.total_hp(),
    }).collect();

    let craters     = game.crater_log.len() as u32;
    let turns       = game.turn.turn_number / game.teams.len().max(1) as u32;
    let big_blast   = game.crater_log.iter().any(|c| c.2 > 65.0);
    let winner_hp   = stats.iter().map(|s| s.hp).max().unwrap_or(0);
    let clean_sweep = stats.iter().any(|s| s.total > 0 && s.alive == 0 && s.total == 4);

    // Each candidate embeds the triggering stat so the screen shows both fact and quip.
    let mut candidates: Vec<String> = Vec::new();
    if craters > 28 { candidates.push(format!("{} craters. The terrain will never recover.", craters)); }
    if craters < 8  { candidates.push(format!("Only {} craters. Barely scratched the surface.", craters)); }
    if clean_sweep  { candidates.push("All 4 eliminated. Total domination.".to_string()); }
    if winner_hp <= 5 { candidates.push(format!("Won on {} HP. One scratch away from defeat.", winner_hp)); }
    if winner_hp <= 5 { candidates.push(format!("{} HP remaining. Survived on fumes.", winner_hp)); }
    if turns > 18   { candidates.push(format!("{} turns of war. A long and gruelling battle.", turns)); }
    if turns < 5    { candidates.push(format!("{} turns. Over in a flash.", turns)); }
    if big_blast    { candidates.push("A truly massive explosion. They felt that one.".to_string()); }
    if stats.iter().any(|s| s.alive == 1) { candidates.push("Last soldier standing.".to_string()); }
    // Always-eligible fallbacks
    candidates.push("Well fought.".to_string());
    candidates.push("The battlefield is silent.".to_string());
    candidates.push("History will remember this.".to_string());

    // Use map_seed — stable for the entire match, never changes after game ends.
    let pick = (game.map_seed.wrapping_mul(2654435761) as usize) % candidates.len();
    (stats, candidates.swap_remove(pick))
}

pub fn draw_weapon_menu_overlay(game: &GameState, buf: &mut WorldBuffer, cam_x: i32, cam_y: i32) {
    if !game.weapon_menu_open { return; }
    let ti = game.active_team();
    let si = game.teams[ti].active;
    draw_weapon_menu(buf, &game.teams[ti].weapons, game.weapon_menu_cursor, cam_x, cam_y, game.aim.fuse_ticks, game.turn.turn_number, game.teams.len());
}

/// Scan for soldiers that just died this tick and record a grave for each.
/// Pick the flavour phrase for a death message. Shared by `record_deaths` (sim,
/// local + live server + TAT) and the live client, which generates death messages
/// locally from synced state so the text uses the names the client actually
/// displays (the server only has default names). `seed` just shuffles the pool —
/// it needn't match across machines, it's flavour text only.
pub fn death_phrase(cause: super::soldier::DeathCause, seed: u32) -> &'static str {
    use super::soldier::DeathCause;
    const DEATH_EXPLOSION: &[&str] = &[
        "didn't respect the blast radius.",
        "got too familiar with the payload.",
        "stood in the wrong postcode.",
        "experienced rapid disassembly.",
        "went out with a bang.",
        "tested proximity limits.",
        "became part of the landscape.",
        "found out what the fuse was for.",
        "took the direct route.",
        "underestimated the yield.",
        "volunteered as tribute.",
        "confused cover with decoration.",
    ];
    const DEATH_FALL: &[&str] = &[
        "had a disagreement with the floor.",
        "gravity remains undefeated.",
        "took the scenic route down.",
        "explored the vertical dimension.",
        "forgot to slow down.",
        "found a shortcut to the bottom.",
        "lost the altitude argument.",
        "landed emphatically.",
        "proved Newton right again.",
        "achieved maximum descent.",
    ];
    const DEATH_WATER: &[&str] = &[
        "found the deep end.",
        "went off the deep end.",
        "discovered they can't swim.",
        "made quite a splash.",
        "joined the fish.",
        "took an extended dip.",
        "is not coming back up.",
        "has gone under.",
        "chose the wet exit.",
        "sank the hard way.",
    ];
    const DEATH_GENERIC: &[&str] = &[
        "bit the dust.",
        "checked out early.",
        "didn't see that coming.",
        "had one job.",
        "is no longer with us.",
        "left without notice.",
        "retired involuntarily.",
        "subscribed to consequences.",
        "has exited the battlefield.",
        "took a wrong turn.",
        "respawns in another life.",
        "won't be doing that again.",
        "had a point to prove.",
        "was simply outgunned.",
        "went out on their own terms. Sort of.",
        "achieved peak misfortune.",
        "the physics were not cooperative.",
        "noted the lesson too late.",
        "had a good run.",
        "is legend now.",
    ];
    let pool = match cause {
        DeathCause::Explosion => DEATH_EXPLOSION,
        DeathCause::Fall      => DEATH_FALL,
        DeathCause::Water     => DEATH_WATER,
        DeathCause::Generic   => DEATH_GENERIC,
    };
    pool[(seed as usize) % pool.len()]
}

fn record_deaths(game: &mut GameState) {
    use super::state::{Grave, GameMessage};
    use super::soldier::DeathCause;
    use crate::renderer::draw_sprites::TEAM_COLOURS;

    let tick = game.tick;
    let mut new_graves: Vec<Grave> = Vec::new();
    let mut new_msgs: Vec<GameMessage> = Vec::new();

    use super::state::PendingDeathExplosion;
    let mut new_pending: Vec<PendingDeathExplosion> = Vec::new();
    let mut soldier_deaths: Vec<crate::audio::Sfx> = Vec::new();

    for (ti, team) in game.teams.iter_mut().enumerate() {
        for soldier in &mut team.soldiers {
            if soldier.is_dead() && soldier.state == SoldierState::Dead && !soldier.has_grave {
                // Route through emit_sound so the live client (which runs no sim
                // of its own) and TAT replay get death audio too — it's recorded
                // into game.sounds and shipped in StateMsg, and plays locally on
                // the simulating device. Direct play_* calls would be silent in
                // live mode. See GameState::emit_sound.
                let death_sfx = if soldier.death_cause == DeathCause::Water {
                    crate::audio::Sfx::DeathWater
                } else {
                    crate::audio::Sfx::Death
                };
                soldier_deaths.push(death_sfx);
                // Death message
                let seed = tick.wrapping_mul(1664525)
                    .wrapping_add(ti as u32 * 7)
                    .wrapping_add(soldier.index as u32 * 13);
                let phrase = death_phrase(soldier.death_cause, seed);
                let text = format!("{} {}", soldier.name, phrase);
                new_msgs.push(GameMessage { text, team: Some(ti), ticks: 120 });

                // Queue death explosion (1 second). Drowned soldiers skip explosion.
                if soldier.death_cause != DeathCause::Water {
                    new_pending.push(PendingDeathExplosion {
                        pos:   soldier.pos,
                        timer: 60, // 2s at 30Hz
                        team:  ti,
                        si:    soldier.index,
                        cause: soldier.death_cause,
                    });
                    soldier.death_explosion_pending = true;
                }
                // else: drowned — no explosion, no grave, soldier just disappears

                soldier.has_grave = true; // prevent re-queuing
            }
        }
    }
    game.pending_deaths.extend(new_pending);
    game.messages.extend(new_msgs);
    for sfx in soldier_deaths { game.emit_sound(sfx); }
    let _ = TEAM_COLOURS; // imported for future use
}

/// Drop unsettled headstones with gravity until they hit terrain.
/// Also re-drops settled headstones when terrain below them is destroyed,
/// and sinks them if they reach water.
fn update_graves(game: &mut GameState) {
    // Remove graves that have sunk in water
    game.graves.retain(|g| g.pos.y < crate::world::WATER_Y as f32);

    for grave in &mut game.graves {
        if grave.settled {
            // Re-check support — terrain may have been carved away
            let ix = grave.pos.x as i32;
            let iy = grave.pos.y as i32;
            let has_support = game.terrain.is_solid(ix, iy)
                || game.terrain.is_solid(ix, iy + 1);
            if !has_support {
                grave.settled = false;
                grave.vel_y = 0.5; // gentle restart
            }
            continue;
        }
        grave.vel_y = (grave.vel_y + 0.8).min(12.0);
        grave.pos.y += grave.vel_y;
        let ix = grave.pos.x as i32;
        let iy = grave.pos.y as i32;
        if grave.pos.y >= crate::world::WATER_Y as f32 {
            // Fell into water — will be removed next tick by retain above
        } else if game.terrain.is_solid(ix, iy) || game.terrain.is_solid(ix, iy + 1) {
            grave.vel_y = 0.0;
            grave.settled = true;
        }
    }
}

/// Run one game logic tick for the server / TAT replay — no camera, no
/// rendering. Thin wrapper over the shared `simulate()` core so live and TAT
/// play byte-for-byte identically to the local modes. Death explosions, the
/// crate-watch input hold, and all SFX now come from the same place.
pub fn server_tick(game: &mut GameState, input: &crate::input::InputState, muzzle: Option<(f32, f32)>, aim_angle: Option<f32>) {
    game.tick = game.tick.wrapping_add(1);
    let _ = simulate_with_muzzle(game, input, muzzle, aim_angle);
}

pub fn push_active_soldier_out(game: &mut GameState) {
    use crate::renderer::draw_sprites::{BODY_H, BODY_HALF_W};
    let active_ti = game.active_team();
    let active_si = game.teams[active_ti].active;
    let ax = game.teams[active_ti].soldiers[active_si].pos.x;
    let ay = game.teams[active_ti].soldiers[active_si].pos.y;
    for ti in 0..game.teams.len() {
        for si in 0..game.teams[ti].soldiers.len() {
            if ti == active_ti && si == active_si { continue; }
            if !game.teams[ti].soldiers[si].is_alive() { continue; }
            let dx = ax - game.teams[ti].soldiers[si].pos.x;
            let dy = ay - game.teams[ti].soldiers[si].pos.y;
            if dx.abs() < 14.0 && dy.abs() < 20.0 {
                let push = if dx >= 0.0 { 1.0 } else { -1.0 };
                let new_x = ax + push;
                let ix = new_x as i32;
                let iy = ay as i32;
                let terrain_clear = (0..=BODY_H).all(|h|
                    !game.terrain.is_blocked(ix - BODY_HALF_W as i32, iy - h)
                    && !game.terrain.is_blocked(ix, iy - h)
                    && !game.terrain.is_blocked(ix + BODY_HALF_W as i32, iy - h));
                if terrain_clear {
                    game.teams[active_ti].soldiers[active_si].pos.x = new_x;
                }
            }
        }
    }
}
