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
    draw_sprites::{draw_soldier, draw_soldier_v3, draw_projectile, draw_grenade_projectile, draw_aim_arrow, draw_headstone, draw_explosion, draw_garcia_sprite},
    skeleton::{draw_soldier_skeletal, SoldierAnim},
    draw_terrain,
    hud::{draw_game_over, draw_pause_menu},
    camera::Camera,
};
use super::state::{GameState, GameMessage, RopeState};
use super::soldier::SoldierState;

// ── Grappling hook constants ──────────────────────────────────────────────────
const ROPE_HOOK_SPEED:    f32 = 40.0;  // px/tick
const ROPE_SWING_FORCE:   f32 = 2.0;   // tangential impulse px/tick² — snappy WA swing authority
const ROPE_GRAVITY:       f32 = 2.5;   // pendulum gravity — fast, Worms-like build-up
const ROPE_RETRACT:       f32 = 4.0;   // px/tick rope length change — snappy reel in/out
const ROPE_MIN_LEN:       f32 = 20.0;
const ROPE_MAX_LEN:       f32 = 320.0;
const ROPE_MAX_SPEED:     f32 = 40.0;  // px/tick per component — prevents tunnelling

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
    /// Per-section pixel-write counts from the most recent frame's render
    /// (TEST mode profiling overlay — see `render_my_team`'s `mark!` calls).
    pub pixel_stats:              Vec<(&'static str, u64)>,
    /// Cached water surface strip (SCREEN_W × WATER_STRIP_H × 4 BGRA bytes).
    /// Regenerated every 3 ticks or when cam_x changes; blitted each frame.
    pub water_strip:      Vec<u8>,
    pub water_strip_tick: u32,  // last tick/3 bucket when strip was built
    pub water_strip_cam:  u32,  // cam_x used when strip was built
    /// Skeleton barrel tip from the previous render frame — used as the fire
    /// origin for hitscan weapons so shots start exactly where the reticle does.
    pub last_muzzle: Option<(f32, f32)>,
}

impl LoopState {
    pub fn new() -> Self {
        use crate::renderer::draw_sprites::{WATER_STRIP_H};
        use crate::world::SCREEN_W;
        Self {
            paused: false, pause_open_tick: 0, tick: 0, pause_cursor: 0,
            weapon_menu_open: false, weapon_menu_cursor: 0, fire_grace: 0,
            prev_turn_number: 0,
            world_cache: crate::renderer::WorldBuffer::new(),
            cache_initialized: false,
            cache_craters_processed: 0,
            bg_cache: crate::renderer::WorldBuffer::new(),
            bg_debris: Vec::new(),
            display_fps: 0,
            pixel_stats: Vec::new(),
            water_strip: vec![0u8; (SCREEN_W * WATER_STRIP_H * 4) as usize],
            water_strip_tick: u32::MAX,
            water_strip_cam: u32::MAX,
            last_muzzle: None,
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
    simulate_with_muzzle(game, input, None)
}

pub fn simulate_with_muzzle(game: &mut GameState, input: &InputState, muzzle_override: Option<(f32, f32)>) -> SimStep {
    use super::turn::TurnPhase;

    game.sounds.clear(); // per-tick sound event buffer (shipped to live client)
    game.fx_events.clear(); // per-tick fx-spawn event buffer (shipped to live client)
    // Advance client-only effect particles (explosion fallout / dust / sparks /
    // splashes) once per tick, before any phase early-returns. Visual only.
    crate::renderer::fx::step_fx(&mut game.fx, &game.terrain, game.wind.value());
    // Object mask: re-stamp barrels + armed mines so collision sees them as solid.
    stamp_objects(game);

    tick_fire_grace(game); // weapon-confirm suppression — one source
    // Timer pauses while the player is charging a power shot (A held). It still
    // ticks while the weapon menu is open so pressure stays on.
    if game.aim.power <= 0.0 {
        game.turn.tick();
    }

    // ── Crate pre-turn phase: block player input (timer already ran above) ────
    if game.crate_watch_ticks > 0 {
        game.crate_watch_ticks -= 1;
        apply_all_gravity(game, &crate::input::InputState::new());
        game.step_crates();
        game.collect_crates();
        game.step_explosions();
        for team in &mut game.teams {
            for s in &mut team.soldiers {
                if s.hp_display_ticks > 0 { s.hp_display_ticks -= 1; }
                if s.displayed_hp > s.hp { s.displayed_hp = s.displayed_hp.saturating_sub(1).max(s.hp); }
                else if s.displayed_hp < s.hp { s.displayed_hp = s.hp; }
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
                process_acting_sim(game, input, muzzle_override);
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
            if game.projectiles.is_empty() && game.explosions.is_empty() && game.pending_deaths.is_empty() && game.black_holes.is_empty() && game.garcia.is_none() && game.airstrike.is_none() && all_grounded {
                let hit = game.active_worm_hit;
                game.active_worm_hit = false;
                game.retreat_locked  = hit;
                game.turn.on_projectiles_resolved();
            }
        }
        TurnPhase::Retreating { .. } => {
            if !game.retreat_locked { process_movement(game, input); }
            apply_all_gravity(game, input);
            // Tick down (and clear once expired) the damage-focus camera hold;
            // the player taking the stick cancels it immediately (see update_camera).
            if let Some((pos, ticks)) = game.damage_focus {
                let moving = input.held(Button::Left) || input.held(Button::Right);
                if moving || ticks <= 1 {
                    game.damage_focus = None;
                } else {
                    game.damage_focus = Some((pos, ticks - 1));
                }
            }
        }
        TurnPhase::Ending => {
            use crate::game::soldier::SoldierState as SS;
            let ti0 = game.active_team();
            let si0 = game.teams[ti0].active;
            if game.teams[ti0].soldiers[si0].is_alive()
                && matches!(game.teams[ti0].soldiers[si0].state, SS::Airborne { .. })
            {
                apply_all_gravity(game, input);
                // soldier still in the air — hold off turn advance until they land
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
            game.rope                = None;
            game.rope_session        = false;
            game.tnt_placed          = false;
            game.plasma_torch        = None;
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
            game.turn.advance(&alive);
            game.active_team_mut().advance_active();
            {
                let rv = game.turn.turn_number.wrapping_mul(2654435761) as f32 / u32::MAX as f32;
                game.wind = crate::physics::Wind::next_turn(rv);
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
            if s.hp_display_ticks > 0 { s.hp_display_ticks -= 1; }
            if s.displayed_hp > s.hp { s.displayed_hp = s.displayed_hp.saturating_sub(1).max(s.hp); }
            else if s.displayed_hp < s.hp { s.displayed_hp = s.hp; }
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
                process_camera_pan(cam, input, game);
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
                        let gx = game.garcia.as_ref().unwrap().render_x;
                        let soldier_y = game.teams[ti].soldiers[si].pos.y;
                        cam.follow(crate::world::WorldPos::new(gx, soldier_y));
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
                if let Some(ref g) = game.garcia {
                    let gpos = crate::world::WorldPos::new(g.cursor_x, g.fall_y.max(0.0));
                    cam.follow_always(gpos);
                } else if game.tnt_placed {
                    let ti = game.active_team();
                    let si = game.teams[ti].active;
                    cam.follow(game.teams[ti].soldiers[si].pos);
                } else if let Some(p) = game.projectiles.first() {
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
                    } else if let Some((pos, ticks)) = game.damage_focus {
                        // Hold on the damaged soldier until the player pans/moves
                        // or the hold timer runs out — whichever comes first.
                        if !cam.panning && !moving && ticks > 0 {
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
        if input.just_pressed(Button::Start) && settle { resume(lstate, game); }
        if input.just_pressed(Button::B)               { resume(lstate, game); }
        if input.just_pressed(Button::Up)   { lstate.pause_cursor = if lstate.pause_cursor == 0 { 1 } else { 0 }; }
        if input.just_pressed(Button::Down) { lstate.pause_cursor = if lstate.pause_cursor == 0 { 1 } else { 0 }; }
        if input.just_pressed(Button::A) {
            match lstate.pause_cursor {
                0 => resume(lstate, game),
                _ => return false,
            }
        }
        // Timer always ticks — pause menu doesn't freeze the clock
        game.turn.tick();
        render(game, buf, cam, lstate);
        draw_pause_menu(buf, lstate.pause_cursor as u8, cam.left_edge() as i32);
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
        let (kills, hp_left, memo) = match_end_stats(game);
        let wc = winner.and_then(|w| game.teams.get(w)).map(|t| t.color_id).unwrap_or(0);
        draw_game_over(buf, winner, my_team, cam.left_edge() as i32, wa, 0, 0, kills, hp_left, &memo, wc);
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
        draw_weapon_menu(buf, &game.teams[ti].weapons, game.weapon_menu_cursor, cam.left_edge() as i32, game.aim.fuse_ticks, game.turn.turn_number, game.teams.len());
    } else {
        // Keep spawn_cam_left current while the player is targeting the airstrike.
        if let Some(ref mut air) = game.airstrike {
            if !air.active { air.spawn_cam_left = cam.left_edge_f32(); }
        }
        let step = simulate_with_muzzle(game, input, lstate.last_muzzle);
        lstate.prev_turn_number = game.turn.turn_number;
        update_camera(game, cam, input, step, prev_turn);
        render(game, buf, cam, lstate);
    }
    true
}

// ── Acting phase ─────────────────────────────────────────────────────────────

/// Acting-phase simulation only — no camera. Camera follow lives in
/// update_camera() (client). Shared by tick() and server_tick() via simulate().
fn process_acting_sim(game: &mut GameState, input: &InputState, muzzle_override: Option<(f32, f32)>) {
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
    let in_rope      = game.rope_session;
    let in_torch     = game.plasma_torch.is_some();
    let in_garcia         = game.garcia.is_some();
    let in_airstrike      = game.airstrike.is_some();
    let in_homing_missile = game.homing_missile.as_ref().map_or(false, |hm| !hm.confirmed);
    if !has_fired || in_revolver || in_minigun || in_uzi || in_rope || in_torch || in_garcia || in_airstrike || in_homing_missile {
        if in_torch {
            process_fire(game, input, muzzle_override); // direction changes only while torching
            step_plasma_torch(game);
        } else if in_garcia {
            step_garcia(game, input);
        } else if in_airstrike {
            step_airstrike(game, input);
        } else if in_homing_missile {
            step_homing_missile(game, input);
        } else {
            process_movement(game, input);
            process_aim(game, input);
            process_fire(game, input, muzzle_override); // no tick-guard — matches server_tick() exactly
        }
    }

    apply_all_gravity(game, input);
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
    }
    // L1 + dpad: free pan that stays when L1 is released (no snap-back).
    // snap_to() on turn change clears the pan flag, resuming soldier follow.
    if input.held(Button::L1) {
        if input.held(Button::Left)  { cam.pan(-speed); }
        if input.held(Button::Right) { cam.pan( speed); }
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
    let on_ground = is_on_ground(game, ti, si);

    let mut walked = false;
    if input.held(Button::Left) && on_ground {
        let nx = game.teams[ti].soldiers[si].pos.x - 2.0;
        try_move_horizontal(game, ti, si, nx);
        game.teams[ti].soldiers[si].facing = -1;
        walked = true;
    }
    if input.held(Button::Right) && on_ground {
        let nx = game.teams[ti].soldiers[si].pos.x + 2.0;
        try_move_horizontal(game, ti, si, nx);
        game.teams[ti].soldiers[si].facing = 1;
        walked = true;
    }
    if walked {
        game.teams[ti].soldiers[si].walk_ticks =
            game.teams[ti].soldiers[si].walk_ticks.wrapping_add(1);
        game.teams[ti].soldiers[si].state =
            SoldierState::Walking { dir: game.teams[ti].soldiers[si].facing as f32 };
        // Footstep dust kicked up behind the trailing foot every few steps.
        let s = &game.teams[ti].soldiers[si];
        if s.walk_ticks % 6 == 0 {
            let (fx, fy, facing) = (s.pos.x, s.pos.y + 3.0, s.facing as f32);
            game.emit_fx(crate::renderer::fx::FxEvent::Dust { x: fx, y: fy, count: 2, kick: 0.4, dir: facing });
        }
    } else {
        game.teams[ti].soldiers[si].walk_ticks = 0;
        if matches!(game.teams[ti].soldiers[si].state, SoldierState::Walking { .. }) {
            game.teams[ti].soldiers[si].state = SoldierState::Idle;
        }
    }
    let is_idle = matches!(game.teams[ti].soldiers[si].state,
        SoldierState::Idle | SoldierState::Walking { .. });
    if input.just_pressed(Button::B) && on_ground && is_idle {
        // Forward jump: maximum horizontal, moderate height
        let vx = game.teams[ti].soldiers[si].facing as f32 * 5.0;
        let y0 = game.teams[ti].soldiers[si].pos.y;
        game.teams[ti].soldiers[si].pos.y -= jump_unstick_lift(game, ti, si);
        game.teams[ti].soldiers[si].state =
            SoldierState::Airborne { vel: crate::world::Vec2::new(vx, -4.0), spinning: false };
        // Start the air clock fresh — see backflip note below.
        game.teams[ti].soldiers[si].airtime = 0;
        game.teams[ti].soldiers[si].fall.begin_fall(y0);
    }
    let on_ground_lenient = on_ground || is_on_ground_lenient(game, ti, si);
    if input.just_pressed(Button::Y) && on_ground_lenient && is_idle {
        // Backflip: maximum vertical, short horizontal — for ledge climbing
        let vx = game.teams[ti].soldiers[si].facing as f32 * -1.5;
        let y0 = game.teams[ti].soldiers[si].pos.y;
        game.teams[ti].soldiers[si].pos.y -= jump_unstick_lift(game, ti, si);
        game.teams[ti].soldiers[si].state =
            SoldierState::Airborne { vel: crate::world::Vec2::new(vx, -6.5), spinning: true };
        // Reset airtime so the spin always plays its full revolution. Without this,
        // a stale airtime (e.g. the soldier was just grabbed into Walking near the
        // ground while a direction is held, which doesn't reset it) is already >= 20,
        // so the airborne step cancels `spinning` on the first tick — the soldier
        // hops backward with no flip animation.
        game.teams[ti].soldiers[si].airtime = 0;
        game.teams[ti].soldiers[si].fall.begin_fall(y0);
    }
}

pub fn try_move_horizontal(game: &mut GameState, ti: usize, si: usize, new_x: f32) {
    use crate::renderer::draw_sprites::{SOLDIER_W, SOLDIER_H, SOLDIER_HALF_W};
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
    let lead_off = if dir > 0 { SOLDIER_HALF_W as i32 } else { -(SOLDIER_HALF_W as i32) };
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
            (0..=SOLDIER_H).all(|h| !game.terrain.is_blocked(next_lc, cfy0 - su - h)));
        if !passable {
            new_x = (lc - lead_off) as f32;
            break;
        }
        lc = next_lc;
    }

    let ix = new_x as i32;
    // Edge columns of the soldier body (center ± half-width - 1).
    let ix_l = ix - SOLDIER_HALF_W as i32;
    let ix_r = ix + SOLDIER_HALF_W as i32;
    // Leading edge column (in the direction of travel) — the side that would run
    // INTO an obstacle. Barrels/mines stay solid (is_blocked), but a soldier already
    // wedged against one must still be able to step AWAY from it.
    let lead_col = if dir > 0 { ix_r } else { ix_l };

    // Is the soldier's CURRENT footprint already overlapping a barrel/mine/wall?
    // (If so we permit an escape step whose leading edge is clear.)
    let cxi   = cur_x as i32;
    let cfy   = cur_y as i32;
    let cx_l  = cxi - SOLDIER_HALF_W as i32;
    let cx_r  = cxi + SOLDIER_HALF_W as i32;
    let stuck_now = (0..=SOLDIER_H).any(|h|
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
            .all(|xc| (0..=SOLDIER_H).all(|h| !game.terrain.is_blocked(xc, fy - h)));
        // Escape allowance: if already wedged, permit the step as long as the leading
        // edge (the side moving forward) is clear — so you can back off a barrel you're
        // stuck on, but never push further into one.
        let lead_clear = (0..=SOLDIER_H).all(|h| !game.terrain.is_blocked(lead_col, fy - h));
        if !terrain_clear && !(stuck_now && lead_clear) { continue; }
        // No other alive soldier may overlap this position.
        let soldier_clear = others.iter().all(|&(ox, oy)| {
            (new_x - ox).abs() >= SOLDIER_W as f32 || (try_y - oy).abs() >= SOLDIER_H as f32
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
    use crate::renderer::draw_sprites::SOLDIER_HALF_W;
    let x = game.teams[ti].soldiers[si].pos.x as i32;
    let y = game.teams[ti].soldiers[si].pos.y as i32;
    // Check all 3 body columns (left edge, center, right edge) — matching
    // try_move_horizontal's footprint — so a move that lands clear there
    // can't be snapped sideways into terrain at an edge column.
    let x_l = x - SOLDIER_HALF_W as i32;
    let x_r = x + SOLDIER_HALF_W as i32;
    let any_solid = |yy: i32| {
        game.terrain.is_blocked(x_l, yy) || game.terrain.is_blocked(x, yy) || game.terrain.is_blocked(x_r, yy)
    };
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

pub fn is_on_ground(game: &GameState, ti: usize, si: usize) -> bool {
    use crate::renderer::draw_sprites::SOLDIER_HALF_W;
    let s = &game.teams[ti].soldiers[si];
    let x = s.pos.x as i32;
    let y = s.pos.y as i32;
    // Probe the full 3-column body footprint (left edge, center, right edge —
    // matching try_move_horizontal/snap_to_surface) so a soldier standing with
    // a clear center column but a blocked edge column isn't reported as "on
    // ground and free to move" when the move check would actually reject it.
    // Exclude columns where the foot-level pixel is already solid — those are
    // vertical walls beside the soldier, not ground beneath it. Without this
    // guard, pressing against a wall makes is_on_ground return true and
    // spamming jump ratchets the soldier upward.
    [x - SOLDIER_HALF_W as i32, x, x + SOLDIER_HALF_W as i32].iter().any(|&xc| {
        if game.terrain.is_solid(xc, y) { return false; }
        game.terrain.is_blocked(xc, y + 1)
            || game.terrain.is_blocked(xc, y + 2)
            || game.terrain.is_blocked(xc, y + 3)
    })
}

/// Lenient ground check for backflip: probes deeper (up to 5px below foot) and
/// doesn't exclude columns where the foot pixel is solid (handles slopes where the
/// soldier is slightly embedded). Also checks the center column only so edge-standing
/// soldiers (where edge columns are excluded by the wall guard) still qualify.
pub fn is_on_ground_lenient(game: &GameState, ti: usize, si: usize) -> bool {
    use crate::renderer::draw_sprites::SOLDIER_HALF_W;
    let s = &game.teams[ti].soldiers[si];
    let x = s.pos.x as i32;
    let y = s.pos.y as i32;
    [x - SOLDIER_HALF_W as i32, x, x + SOLDIER_HALF_W as i32].iter().any(|&xc| {
        (1..=5).any(|dy| game.terrain.is_blocked(xc, y + dy))
    })
}

/// Upward "unstick" lift (0–2 px) for a jump/backflip, clamped to the clear space
/// above the soldier's head. Without this clamp the fixed `pos.y -= 2.0` teleports
/// the soldier up through solid terrain, so spamming jump in a tight pocket ratchets
/// them up through the ground to the surface. Airborne physics handles real motion.
pub fn jump_unstick_lift(game: &GameState, ti: usize, si: usize) -> f32 {
    use crate::renderer::draw_sprites::{SOLDIER_H, SOLDIER_HALF_W};
    let x = game.teams[ti].soldiers[si].pos.x as i32;
    let head = game.teams[ti].soldiers[si].pos.y as i32 - SOLDIER_H as i32;
    let x_l = x - SOLDIER_HALF_W as i32;
    let x_r = x + SOLDIER_HALF_W as i32;
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
    use crate::renderer::draw_sprites::SOLDIER_HALF_W;
    let ix = cx as i32;
    let ix_l = ix - SOLDIER_HALF_W as i32;
    let ix_r = ix + SOLDIER_HALF_W as i32;
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
        && game.plasma_torch.is_none(); // menu disabled while torch is burning

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
            const COLS: usize = 3;
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
            if input.just_pressed(Button::Up) {
                let new_row = if row == 0 { col_len(col).saturating_sub(1) } else { row - 1 };
                game.weapon_menu_cursor = col * rows + new_row;
            }
            if input.just_pressed(Button::Down) {
                let new_row = if row + 1 < col_len(col) { row + 1 } else { 0 };
                game.weapon_menu_cursor = col * rows + new_row;
            }
            // L1/R1 adjusts grenade fuse even while menu is open
            {
                use crate::physics::projectile::WeaponKind;
                let ck = game.teams[ti].weapons[game.weapon_menu_cursor].0;
                if ck == WeaponKind::Grenade {
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
pub fn tick_fire_grace(game: &mut GameState) {
    if game.server_fire_grace > 0 {
        game.server_fire_grace -= 1;
        game.aim.power = 0.0;
    }
}

// ── Aim & Fire ────────────────────────────────────────────────────────────────

/// Adjust aim angle and grenade fuse. Public so live client can call it.
pub fn process_aim(game: &mut GameState, input: &InputState) {
    // While rope is attached, Up/Down controls rope length (handled in process_fire).
    if game.rope.as_ref().map_or(false, |r| !r.flying) { return; }
    let delta = if input.held(Button::L1) { 0.01f32 } else { 0.04f32 };
    if input.held(Button::Up)   { game.aim.angle += delta; }
    if input.held(Button::Down) { game.aim.angle -= delta; }

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

    // Grenade fuse: L1 = shorter, R1 = longer (1-5 s in 1-s steps at 30 Hz)
    if ck == WeaponKind::Grenade {
        if input.just_pressed(Button::L1) { game.aim.fuse_ticks = game.aim.fuse_ticks.saturating_sub(30).max(30); }
        if input.just_pressed(Button::R1) { game.aim.fuse_ticks = (game.aim.fuse_ticks + 30).min(150); }
    }
}

fn fire_rope_hook(game: &mut GameState, ti: usize, si: usize) {
    use crate::world::{WorldPos, Vec2};
    let fm    = game.teams[ti].soldiers[si].facing as f32;
    let angle = game.aim.angle;
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
    });
    // Soldier stays on the ground while hook is flying — becomes Airborne when hook attaches.
}

fn release_rope(game: &mut GameState) {
    game.rope = None; // rope_session stays true — player can re-rope mid-air
}

fn process_fire(game: &mut GameState, input: &InputState, muzzle_override: Option<(f32, f32)>) {
    use crate::physics::projectile::WeaponKind;
    let weapon = game.active_team_ref().current_weapon();

    // ── Plasma torch active: hold A to keep burning; release A to stop ──────────
    if game.plasma_torch.is_some() {
        if input.just_pressed(Button::Up) {
            let d = game.plasma_torch.as_ref().unwrap().dir;
            game.plasma_torch.as_mut().unwrap().dir = d.step_up();
        }
        if input.just_pressed(Button::Down) {
            let d = game.plasma_torch.as_ref().unwrap().dir;
            game.plasma_torch.as_mut().unwrap().dir = d.step_down();
        }
        // Release A early → extinguish torch and end turn
        if input.just_released(Button::A) {
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
        // Up/Down: adjust rope length while attached
        if let Some(ref mut rope) = game.rope {
            if !rope.flying {
                if input.held(Button::Up) {
                    rope.length = (rope.length - ROPE_RETRACT).max(ROPE_MIN_LEN);
                }
                if input.held(Button::Down) {
                    rope.length = (rope.length + ROPE_RETRACT).min(ROPE_MAX_LEN);
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

    // Landmine: instant placement on A press — starts arming, turn ends.
    if weapon == WeaponKind::Landmine {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0 {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_mine(game, ti, si);
        }
        return;
    }

    // TNT: instant placement on A press — no charge needed, locked until turn 5.
    if weapon == WeaponKind::Tnt {
        if input.just_pressed(Button::A) && game.server_fire_grace == 0
           && game.turn.turn_number >= 5 * game.teams.len() as u32
        {
            let ti = game.active_team();
            let si = game.teams[ti].active;
            fire_tnt(game, ti, si);
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
    const CHARGE_RATE: f32 = 0.02;  // 0.6/s at 30 Hz; full charge ~50 ticks

    let is_bazooka = weapon == WeaponKind::Bazooka;

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
        game.aim.power = (game.aim.power + CHARGE_RATE).min(MAX_CHARGE);
        if is_bazooka && game.aim.power >= MAX_CHARGE {
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
    const TORCH_RADIUS:   f32 = 14.0; // carving circle radius at tip — must be ≥ sqrt(SOLDIER_HALF_W²+10²)≈12.2 to clear full soldier height at tip
    const BODY_RADIUS:    f32 = 14.0; // carving circle at soldier midpoint — SOLDIER_H=20, tunnel ~28px tall

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
    let body_cy = sy - 10.0; // vertical midpoint of soldier (SOLDIER_H/2); r=11 covers full height

    let tip_x = sx + dx * TORCH_TIP_DIST;
    let tip_y = body_cy + dy * TORCH_TIP_DIST;

    // Only advance (and carve) if there's actually solid terrain to dig through.
    // Prevents the torch from propelling the soldier through open air.
    // Check BEYOND the carve zone (tip_dist + tip_radius = 28px) so the first-tick
    // carve (which clears 0-28px) doesn't cause has_solid=false on tick 2.
    let check_start = TORCH_TIP_DIST + TORCH_RADIUS + 2.0; // 34px from soldier
    let has_solid = (0..=4).any(|i| {
        let d = check_start + i as f32 * 3.0; // 30, 33, 36, 39, 42 px ahead
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
        let d = crate::game::state::biome_dirt(game.terrain.archetype);
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

fn step_garcia(game: &mut GameState, input: &InputState) {
    use crate::world::WORLD_W;
    use crate::physics::WeaponKind;

    let g = match game.garcia.as_mut() { Some(g) => g, None => return };

    if g.falling {
        // Gravity accelerates fall; positive vel_y = downward
        g.vel_y = (g.vel_y + 1.2).min(20.0);
        g.fall_y += g.vel_y;

        let fx = g.cursor_x as i32;
        let fy = g.fall_y as i32;

        // Collision: find the actual terrain surface below the sprite center.
        // Use surface_y_at so we always detect terrain even after explosion craters.
        let surface_y = if fx >= 0 && fx < crate::world::WORLD_W as i32 {
            game.terrain.surface_y_at(fx as u32)
                .map(|y| y as i32)
                .unwrap_or(crate::world::WORLD_H as i32)
        } else {
            crate::world::WORLD_H as i32
        };

        // Hit when the sprite center reaches the terrain surface (accounting for half-height)
        let hit_ground = g.vel_y > 0.0 && fy + 54 >= surface_y;

        // Extract what we need before potentially dropping the borrow
        let cursor_x   = g.cursor_x;
        let bounce_count = g.bounce_count;
        let do_carve   = fy >= 0 && g.vel_y > 0.0 && !hit_ground;
        let carve_pos  = (g.cursor_x, g.fall_y);
        drop(g);

        if do_carve {
            let crater = crate::world::Crater::new(carve_pos.0, carve_pos.1, 24.0);
            crater.carve(&mut game.terrain);
            game.crater_log.push((carve_pos.0, carve_pos.1, 24.0));
        }

        if hit_ground {
            let land_y = surface_y as f32;
            let at_water = land_y >= crate::world::WATER_Y as f32;

            if let Some(g) = game.garcia.as_mut() { g.fall_y = land_y - 54.0; }
            let hit_pos = crate::world::WorldPos::new(cursor_x, land_y);

            // Shrink explosion force slightly each bounce so later hits feel lighter
            let force_scale = (1.0f32 - bounce_count as f32 * 0.1).max(0.4);
            game.apply_explosion_force(hit_pos, WeaponKind::Garcia, force_scale);
            let left  = crate::world::WorldPos::new(hit_pos.x - 18.0, hit_pos.y);
            let right = crate::world::WorldPos::new(hit_pos.x + 18.0, hit_pos.y);
            game.apply_explosion_scaled(left,  WeaponKind::Garcia, force_scale * 0.7, 0.8, 0.4);
            game.apply_explosion_scaled(right, WeaponKind::Garcia, force_scale * 0.7, 0.8, 0.4);

            game.emit_sound(crate::audio::Sfx::Smash);

            if at_water {
                game.garcia = None;
            } else if let Some(g) = game.garcia.as_mut() {
                g.bounce_count += 1;
                g.vel_y = -12.0;
            }
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
        g.cursor_y = (g.cursor_y + CURSOR_SPEED).min(400.0);
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
            g.fall_y       = g.cursor_y - 200.0;
            g.vel_y        = 8.0;
            g.bounce_count = 0;
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
    const BOMB_SPACING: f32 = 20.0;
    const PLANE_SPEED:  f32 = 9.0;
    const CURSOR_SPEED: f32 = 14.0;

    // Returns the world X where bomb index `i` (0..5, left-to-right) should drop.
    fn bomb_x(cursor_x: f32, i: u32) -> f32 {
        let half = (BOMB_COUNT as f32 - 1.0) / 2.0;
        (cursor_x + (i as f32 - half) * BOMB_SPACING).clamp(0.0, WORLD_W as f32 - 1.0)
    }
    // Next undroped bomb index in drop order (left-to-right for right-flying plane).
    fn next_drop_idx(dropped: u32, dir_right: bool) -> u32 {
        if dir_right { dropped } else { BOMB_COUNT - 1 - dropped }
    }

    let s = match game.airstrike.as_mut() { Some(s) => s, None => return };

    if !s.active {
        if input.held(Button::Left)  { s.cursor_x = (s.cursor_x - CURSOR_SPEED).max(0.0); }
        if input.held(Button::Right) { s.cursor_x = (s.cursor_x + CURSOR_SPEED).min(WORLD_W as f32 - 1.0); }
        if input.held(Button::Up)    { s.cursor_y = (s.cursor_y - CURSOR_SPEED).max(12.0); }
        if input.held(Button::Down)  { s.cursor_y = (s.cursor_y + CURSOR_SPEED).min(400.0); }
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
            s.plane_x       = if dir_right { s.spawn_cam_left } else { s.spawn_cam_left + SCREEN_W as f32 };
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

    if s.bombs_dropped < BOMB_COUNT {
        let cursor_x  = s.cursor_x;
        let dropped   = s.bombs_dropped;
        let dir_right = s.direction_right;
        let idx       = next_drop_idx(dropped, dir_right);
        let bx        = bomb_x(cursor_x, idx);
        let passed    = if dir_right { s.plane_x >= bx } else { s.plane_x <= bx };
        if passed {
            game.projectiles.push(Projectile::new(
                WorldPos::new(bx, 10.0),
                Vec2::new(0.0, 3.0),
                WeaponKind::AirStrike,
            ));
            game.airstrike.as_mut().unwrap().bombs_dropped += 1;
        }
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
    // power=1.0 → 20 (unchanged); the extra 1.0..MAX_CHARGE band adds launch speed
    // (hence range) only when the player fills the extended meter.
    let power = game.aim.power.min(MAX_CHARGE) * 20.0;

    let sy = game.teams[ti].soldiers[si].pos.y - 4.0 - angle.sin() * 12.0;
    let sx = game.teams[ti].soldiers[si].pos.x + angle.cos() * fm * 12.0;

    let spawn = WorldPos::new(sx, sy);
    let vel   = Vec2::new(angle.cos() * power * fm, -angle.sin() * power);

    let mut proj = Projectile::new(spawn, vel, kind);
    if kind == WeaponKind::Grenade {
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

    const PELLETS: usize = 5;
    const SPREAD: f32   = 0.10;   // ±0.10 rad (~5.7°) per pellet
    const RANGE:  i32   = 220;    // pixels before pellet expires
    const PELLET_DMG: u32 = 5;    // 5 pellets × 5 = 25 max per shot
    const PELLET_FORCE: f32 = 1.8; // directional knockback — 5 pellets = 9 px/tick max
    const RECOIL: f32  = 2.0;     // shooter kickback

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

    // LCG seeded from tick + shot number for deterministic but varied spread
    let seed = game.tick.wrapping_mul(1664525).wrapping_add(1013904223);

    // Collect damage per soldier so we only apply once per shot
    let n_teams  = game.teams.len();
    let n_sol: Vec<usize> = (0..n_teams).map(|t| game.teams[t].soldiers.len()).collect();
    let mut hits: Vec<Vec<(u32, f32, f32)>> = (0..n_teams)
        .map(|t| vec![(0u32, 0.0f32, 0.0f32); n_sol[t]])
        .collect();
    // Splat positions collected here, pushed to game.blood_splats after borrow ends
    let mut splat_hits: Vec<(f32, f32, f32, f32)> = Vec::new(); // (px, py, dx, dy)

    for pellet in 0..PELLETS {
        let r  = seed.wrapping_mul(pellet as u32 + 7).wrapping_add(pellet as u32 * 31337) as f32
                 / u32::MAX as f32;
        let spread = (r - 0.5) * 2.0 * SPREAD;
        let angle  = base_angle + spread;
        let dx = angle.cos() * fm;
        let dy = -angle.sin();

        let mut px = muzzle_x;
        let mut py = muzzle_y;
        let mut hit = false;
        for _ in 0..RANGE {
            px += dx;
            py += dy;
            let ix = px as i32;
            let iy = py as i32;
            if ix < 0 || iy < 0 { break; }
            if game.terrain.is_solid(ix, iy) {
                if !hitscan_hit_crate(game, px, py) {
                    let crater = crate::world::Crater::new(px, py, 4.0);
                    crater.carve(&mut game.terrain);
                    game.crater_log.push((px, py, 4.0));
                }
                break;
            }
            // Barrel direct hit
            for barrel in &mut game.barrels {
                if let super::state::BarrelState::Normal = barrel.state {
                    if (barrel.pos.x - px).abs() < 8.0 && (barrel.pos.y - py).abs() < 12.0 {
                        barrel.state = super::state::BarrelState::Triggered { ticks: 6 };
                        hit = true;
                        break;
                    }
                }
            }
            if hit { break; }
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
                        hits[t][s].0 += PELLET_DMG;
                        hits[t][s].1 += dx * PELLET_FORCE;
                        hits[t][s].2 += dy * PELLET_FORCE;
                        splat_hits.push((px, py, dx, dy));
                        hit = true;
                        break 'soldiers;
                    }
                }
            }
            if hit { break; }
        }
    }

    // Blood splats for each pellet that hit a soldier
    let mut rng = seed as u64;
    for (spx, spy, sdx, sdy) in splat_hits {
        rng = rng.wrapping_mul(2654435761).wrapping_add(1);
        let fwd = 5.0 + (rng & 0x1F) as f32 * 0.5;
        let lat = ((rng >> 5) & 0xF) as f32 - 7.5;
        game.blood_splats.push((
            crate::world::WorldPos::new(
                spx + sdx * fwd + (-sdy) * lat,
                spy + sdy * fwd + ( sdx) * lat,
            ), 90,
        ));
    }

    // Apply accumulated damage + knockback, track if active worm was hit
    let active_ti = game.active_team();
    let active_si = game.teams[active_ti].active;
    let active_hp_before = game.teams[active_ti].soldiers[active_si].hp;
    for t in 0..n_teams {
        for s in 0..n_sol[t] {
            let (dmg, vx, vy) = hits[t][s];
            if dmg == 0 { continue; }
            game.teams[t].soldiers[s].death_cause = crate::game::soldier::DeathCause::Explosion;
            game.teams[t].soldiers[s].take_damage(dmg);
            let new_state = match &game.teams[t].soldiers[s].state {
                SoldierState::Airborne { vel, spinning } => SoldierState::Airborne {
                    vel: Vec2::new(vel.x + vx, vel.y + vy),
                    spinning: *spinning,
                },
                SoldierState::Idle | SoldierState::Walking { .. } => SoldierState::Airborne {
                    vel: Vec2::new(vx, vy),
                    spinning: false,
                },
                SoldierState::Dead => continue,
            };
            game.teams[t].soldiers[s].state = new_state;
        }
    }
    if game.teams[active_ti].soldiers[active_si].hp < active_hp_before {
        game.active_worm_hit = true;
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
        server_tick(game, &input, None);
        game.messages.retain(|m| !m.text.contains("got a ") && !m.text.contains("picked up"));
    }
    menu_open
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
    const BAT_DAMAGE: u32 = 30;

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
        // Launch first so the soldier flies before taking damage (visual read).
        let grounded = matches!(target.state, SoldierState::Idle | SoldierState::Walking { .. });
        if grounded { target.fall.begin_fall(target.pos.y); }
        target.state = SoldierState::Airborne {
            vel: Vec2::new(BAT_POWER * angle.cos() * fm, -BAT_POWER * angle.sin()),
            spinning: true,
        };
        target.death_cause = DeathCause::Explosion;
        target.kill_weapon = Some(crate::physics::WeaponKind::BaseballBat);
        target.take_damage(BAT_DAMAGE);
        // If the hit killed them, clear the airborne state so gravity doesn't
        // skip the corpse and freeze the turn.
        if !target.is_alive() {
            target.state = SoldierState::Dead;
        }
        if target.is_alive() {
            // Fresh air clock so the knockback spin animates in full (a stale
            // airtime >= 20 would cancel `spinning` on the first airborne tick).
            target.airtime = 0;
        }
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
                // height from foot (sy2) up to head (sy2 - SOLDIER_H).
                let hit_top_offset = if crate::renderer::skeleton::SOLDIER_STYLE_V2 { 10.0 } else { 2.0 };
                if (rx - sx2).abs() < 10.0
                    && ry >= sy2 - crate::renderer::draw_sprites::SOLDIER_H as f32 - hit_top_offset
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
                    mine.trigger_ticks = 8;
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
                    && ry >= sy2 - crate::renderer::draw_sprites::SOLDIER_H as f32 - hit_top_offset
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
            let d = crate::game::state::biome_dirt(game.terrain.archetype);
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
    const DAMAGE:            u32 = 3;
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
                    mine.trigger_ticks = 8;
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
                    && ry >= sy2 - crate::renderer::draw_sprites::SOLDIER_H as f32 - hit_top_offset
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
            let d = crate::game::state::biome_dirt(game.terrain.archetype);
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
    fuse_ticks: u32,
    turn_number: u32,
    num_teams: usize,
) {
    use crate::renderer::fb::Bgra;
    use crate::renderer::font::{draw_str, str_width, draw_str_scaled, str_width_scaled};
    use crate::physics::projectile::WeaponKind;
    use crate::world::{SCREEN_W, SCREEN_H};

    let cols: i32 = 3;
    let cell_w: i32 = 120;
    let cell_h: i32 = 64;
    const MAX_ROWS: i32 = 6;
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
    let wy = (SCREEN_H as i32 - win_h as i32) / 2;

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
            _ => {
                buf.fill_rect(icon_cx - 6, icon_cy - 4, 12, 8, dark);
                buf.fill_rect(icon_cx - 5, icon_cy - 3, 10, 6, icol);
            }
        }

        // TNT lock overlay: padlock icon + rotation countdown (5 complete rotations)
        let tnt_unlock = 5 * num_teams as u32;
        if *kind == WeaponKind::Tnt && turn_number < tnt_unlock {
            let team_count = (num_teams as u32).max(1);
            let rotations_done = turn_number / team_count;
            let rotations_left = 5u32.saturating_sub(rotations_done);
            let lk = Bgra::new(180, 180, 60); // golden lock
            let lkd = Bgra::new(100, 100, 30);
            buf.fill_rect(icon_cx - 5, icon_cy - 12, 3, 8, lk);
            buf.fill_rect(icon_cx + 2, icon_cy - 12, 3, 8, lk);
            buf.fill_rect(icon_cx - 5, icon_cy - 14, 10, 3, lk);
            buf.fill_rect(icon_cx - 4, icon_cy - 13, 8, 2, lkd);
            buf.fill_rect(icon_cx - 7, icon_cy - 5, 14, 10, lk);
            buf.fill_rect(icon_cx - 6, icon_cy - 4, 12, 8, lkd);
            buf.fill_rect(icon_cx - 1, icon_cy - 3,  2, 2, Bgra::new(30, 30, 20));
            buf.fill_rect(icon_cx - 1, icon_cy - 1,  2, 4, Bgra::new(30, 30, 20));
            let cdown = format!("T-{}", rotations_left);
            draw_str(buf, &cdown, cx + cell_w - str_width(&cdown) - 6, cy + cell_h - 18, Bgra::new(220, 200, 60));
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
            WeaponKind::ClusterBomb => "CLUSTER",
            WeaponKind::Landmine    => "MINE",
            WeaponKind::Tnt         => "TNT",
            WeaponKind::BananaBomb  => "METEOR BOMB",
            WeaponKind::Revolver    => "REVOLVER",
            WeaponKind::NinjaRope    => "GRAPPLE",
            WeaponKind::BaseballBat  => "BAT",
            WeaponKind::Blasthive      => "BLASTHIVE",
            WeaponKind::BlackHoleBomb  => "BLACK HOLE",
            WeaponKind::PlasmaTorch    => "TORCH",
            WeaponKind::Garcia         => "HAND OF JERRY",
            WeaponKind::AirStrike      => "AIR STRIKE",
            WeaponKind::HolyHandGrenade => "SACRED ORD.",
            WeaponKind::Minigun         => "MINIGUN",
            WeaponKind::Uzi             => "MAC-10",
            WeaponKind::HomingMissile   => "HOMING MISSILE",
            _                          => "WEAPON",
        };
        let nc = if selected { Bgra::new(255, 220, 50) } else { Bgra::new(150, 150, 180) };
        if *kind == WeaponKind::HolyHandGrenade {
            // Fixed 3-second fuse, no adjustment
            draw_str(buf, name, cx + cell_w/2 - str_width(name)/2, cy + cell_h - 26, nc);
            let fuse_col = if selected { Bgra::new(255, 210, 40) } else { Bgra::new(140, 120, 40) };
            draw_str(buf, "3s FIXED", cx + cell_w/2 - str_width("3s FIXED")/2, cy + cell_h - 15, fuse_col);
        } else if *kind == WeaponKind::Grenade {
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
    let sw    = crate::world::SCREEN_W as i32;
    let sh    = crate::world::SCREEN_H as i32;

    // Plasma-torch burn sound: plays only while the torch is active, stops on
    // deactivation/early release. Driven here so it covers every mode (local/TAT
    // use the simulated plasma_torch; live reconstructs it from networked torch_dir).
    crate::audio::update_torch(game.plasma_torch.is_some());

    // Per-section pixel-write profiling (TEST mode overlay, see section 9b/9d
    // below). `mark!` records how many pixels were written since the previous
    // mark and resets the running total.
    let mut pixel_stats: Vec<(&'static str, u64)> = Vec::new();
    let mut last_pw = buf.pixel_writes;
    macro_rules! mark {
        ($label:expr) => {
            pixel_stats.push(($label, buf.pixel_writes - last_pw));
            last_pw = buf.pixel_writes;
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
    crate::renderer::bg_image::copy_bg_viewport(buf, &lstate.bg_cache, &game.terrain, game.map_seed, cam_x);
    mark!("bg_sky");
    let gw = background::gust_wind(game.wind.value(), lstate.tick);
    background::draw_backdrop(buf, &game.terrain, cam_x);
    mark!("backdrop");
    background::update_debris(&mut lstate.bg_debris, &game.terrain, gw, lstate.tick);

    buf.copy_viewport_from_sky_aware(&lstate.world_cache, cam_x, &game.terrain, &lstate.bg_cache, game.map_seed);
    mark!("terrain_copy");
    background::draw_debris(buf, &game.terrain, &lstate.bg_debris, cam_x, lstate.tick);
    mark!("debris");

    // 2. Water ripple — cached strip, regenerated every tick (30 Hz).
    {
        use crate::renderer::draw_sprites::{render_water_strip, WATER_STRIP_H};
        render_water_strip(&mut lstate.water_strip, game.tick, cam_x);
        buf.blit_water_strip(&lstate.water_strip, cam_x);
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
        if wx >= vx0 - 8.0 && wx < vx1 + 8.0 && wy >= 0.0 && wy < sh as f32 {
            let gcolor = game.teams.get(grave.team).map(|t| t.color_id as usize).unwrap_or(grave.team.min(3));
            draw_headstone(buf, grave.pos, gcolor, grave.headstone_id);
        }
    }

    // 4b. Crates — 24×24 box with symbol; parachute while descending
    for cr in &game.crates {
        let wx = cr.pos.x as i32;
        let wy = cr.pos.y as i32;
        if cr.pos.x < vx0 - 32.0 || cr.pos.x >= vx1 + 32.0 { continue; }
        if wy < -80 || wy >= sh { continue; }

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
            if wy < -20 || wy >= sh { continue; }
            let flicker = matches!(barrel.state, BarrelState::Triggered { .. })
                && (game.tick / 3) % 2 == 0;
            let body_col = if flicker { Bgra::new(255, 140, 40) } else { Bgra::new(160, 30, 20) };
            let hi_col   = Bgra::new(200, 60, 50);
            let dark_col = Bgra::new(80, 10, 8);
            let band_col = Bgra::new(200, 180, 50); // yellow warning band
            // Shadow (sits at terrain surface — pos.y is first air pixel above terrain)
            buf.fill_rect(wx - 7, wy,      14, 3, Bgra::new(0, 0, 0));
            // Body — shifted up 10px so bottom sits at terrain surface
            buf.fill_rect(wx - 7, wy - 23, 14, 23, dark_col);
            buf.fill_rect(wx - 6, wy - 22, 12, 21, body_col);
            buf.fill_rect(wx - 6, wy - 22,  2, 21, hi_col);  // left highlight
            // Warning band
            buf.fill_rect(wx - 7, wy - 14, 14, 3, band_col);
            // Barrel lid top
            buf.fill_rect(wx - 7, wy - 24, 14, 2, dark_col);
        }
    }

    // 4d. Mines — flat disc with blinking red LED
    for mine in &game.mines {
        let wx = mine.pos.x as i32;
        let wy = mine.pos.y as i32;
        if mine.pos.x < vx0 - 16.0 || mine.pos.x >= vx1 + 16.0 { continue; }
        if wy < -20 || wy >= sh { continue; }
        let dk  = Bgra::new(15,  50, 15);   // dark outline
        let mid = Bgra::new(40, 100, 30);   // body
        let hi  = Bgra::new(60, 135, 45);   // top highlight
        let rim = Bgra::new(30,  75, 22);   // rim edge
        // Shadow
        buf.fill_rect(wx - 6, wy + 3, 13, 2, Bgra::new(0, 0, 0));
        // Disc body — wide flat cylinder (15px wide, 6px tall)
        buf.fill_rect(wx - 5, wy + 2,  11, 1, dk);   // bottom edge
        buf.fill_rect(wx - 7, wy,      15, 2, mid);   // lower body
        buf.fill_rect(wx - 7, wy - 2,  15, 2, mid);   // upper body
        buf.fill_rect(wx - 6, wy - 3,  13, 1, rim);   // top rim
        buf.fill_rect(wx - 5, wy - 4,  11, 1, dk);    // top dark edge
        // Left/right curved ends
        buf.set_pixel(wx - 7, wy + 1, rim);
        buf.set_pixel(wx + 7, wy + 1, rim);
        // Top face highlight strip
        buf.fill_rect(wx - 4, wy - 3, 9, 1, hi);
        // Red LED nub on top centre
        let led_on = match mine.state {
            super::state::MineState::Arming    => (game.tick / 15) % 2 == 0,
            super::state::MineState::Armed     => (game.tick / 5)  % 2 == 0,
            super::state::MineState::Triggered => (game.tick / 3)  % 2 == 0,
        };
        if led_on {
            buf.fill_rect(wx - 1, wy - 5, 3, 2, Bgra::new(220, 25, 25));
            buf.set_pixel(wx, wy - 5, Bgra::new(255, 110, 110));
        } else {
            buf.fill_rect(wx - 1, wy - 5, 3, 2, Bgra::new(80, 10, 10));
        }
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
            if wx >= vx0 - 16.0 && wx < vx1 + 16.0 && wy >= 0.0 && wy < sh as f32 {
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
                let anim = match &soldier.state {
                    SoldierState::Airborne { vel, spinning: true } =>
                        SoldierAnim::Airborne { vel_x: vel.x, vel_y: vel.y, airtime: soldier.airtime, spinning: true },
                    SoldierState::Airborne { vel, spinning: false } =>
                        SoldierAnim::Airborne { vel_x: vel.x, vel_y: vel.y, airtime: soldier.airtime, spinning: false },
                    SoldierState::Walking { .. } =>
                        SoldierAnim::Walking { tick: soldier.walk_ticks },
                    SoldierState::Dead =>
                        SoldierAnim::Dead,
                    SoldierState::Idle =>
                        SoldierAnim::Idle,
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
                let muzzle = draw_soldier_skeletal(buf, soldier.pos, team.color_id as usize, soldier.facing, soldier.displayed_hp, &anim, aim_angle, soldier.hp > 0,
                    soldier.hat_id, soldier.uniform_color_id, soldier.boot_color_id, gun_style, held_weapon,
                    game.wind.value(), game.tick, soldier.on_fire_ticks);
                if ti == active_ti && si == active_si {
                    active_muzzle = muzzle;
                    if let Some(m) = muzzle { lstate.last_muzzle = Some(m); }
                }

                // Soldier name — bold 8px (drawn twice offset by 1px) with shadow
                {
                    use crate::renderer::font::{draw_str, str_width};
                    use crate::renderer::draw_sprites::{SOLDIER_H, TEAM_COLOURS};
                    let col  = TEAM_COLOURS[team.color_id as usize];
                    let dark = Bgra::new(0, 0, 0);
                    let nw   = str_width(&soldier.name) + 1; // +1 for bold shift
                    let nx   = soldier.pos.x as i32 - nw / 2;
                    let hat_lift = if soldier.hat_id > 0 { 21 } else { 0 };
                    let ny   = soldier.pos.y as i32 - SOLDIER_H as i32 - 40 - hat_lift;
                    // Shadow pass + single bold pass (was 2 shadow + 2 bold;
                    // halved to cut per-frame glyph draw calls).
                    draw_str(buf, &soldier.name, nx + 1, ny + 1, dark);
                    draw_str(buf, &soldier.name, nx,     ny,     col);
                }

                // Active-soldier marker: downward triangle, raised above name
                if ti == active_ti && si == active_si && game.turn.is_acting() {
                    let cx  = soldier.pos.x as i32;
                    let hat_lift = if soldier.hat_id > 0 { 21 } else { 0 };
                    let ty  = soldier.pos.y as i32 - crate::renderer::draw_sprites::SOLDIER_H - 52 - hat_lift;
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
    // Procedural teardrop flame: tapers to a point, concentric colour bands,
    // per-flame flicker + sideways sway that grows toward the tip.
    for patch in &game.fire_patches {
        let wx = patch.pos.x as i32;
        let wy = patch.pos.y as i32;
        if patch.pos.x < vx0 - 10.0 || patch.pos.x >= vx1 + 10.0 { continue; }
        if wy < -20 || wy >= sh { continue; }

        let mid   = Bgra::new(255, 130, 15);
        let inner = Bgra::new(255, 205, 60);
        let core  = Bgra::new(255, 250, 205);

        // Tall dancing flame when burning on the ground; small spark in flight.
        // (~30% larger than before: 13→17, 6→8; max_half scales with h so the
        // whole flame grows proportionally in both height and width.)
        let h = if patch.landed { 17 } else { 8 };
        let max_half = h as f32 * 0.42;
        // Independent phase per flame — drives both sway/width flicker and outer
        // colour toggle so flames don't all change colour in lockstep.
        // Use lifetime for time and spawn-velocity as a stable per-patch offset
        // (vel is constant after spawn, so phase doesn't jump as the patch moves).
        let phase = patch.lifetime as f32 * 0.30 + patch.vel.x * 1.1 + patch.vel.y * 0.9;
        let outer = if phase.sin() > 0.0 { Bgra::new(210, 40, 0) } else { Bgra::new(235, 60, 0) };
        let base_y = wy + 1;

        for ry in 0..h {
            let f = ry as f32 / (h as f32 - 1.0); // 0 at base → 1 at tip
            // Teardrop: slightly pinched base, bulge low, taper to a point.
            let w_profile = (1.0 - f).powf(0.65);
            let base_pinch = (f * 5.0).min(1.0);
            let mut half = (max_half * w_profile * (0.55 + 0.45 * base_pinch)).round() as i32;
            // Subtle width flicker
            if (phase + ry as f32).sin() > 0.6 { half += 1; }
            let sway = ((phase + ry as f32 * 0.6).sin() * f * f * 2.6).round() as i32;
            let y = base_y - ry;
            if y < 0 || y >= sh { continue; }
            let fcx = wx + sway;
            if half <= 0 {
                buf.set_pixel(fcx, y, inner);
                continue;
            }
            // Row is in-bounds vertically (checked above) and, in the common case,
            // horizontally too — skip the per-pixel bounds check then.
            let row_in_bounds = fcx - half >= 0 && fcx + half < crate::world::WORLD_W as i32;
            for dx in -half..=half {
                let edge = dx.abs() as f32 / (half as f32 + 0.5); // 0 centre → ~1 edge
                let col = if edge > 0.70 { outer } else if edge > 0.34 { mid } else { inner };
                if row_in_bounds {
                    buf.set_pixel_unchecked((fcx + dx) as u32, y as u32, col);
                } else {
                    buf.set_pixel(fcx + dx, y, col);
                }
            }
        }
        // White-hot core near the base
        if patch.landed {
            buf.fill_rect(wx - 1, base_y - h / 3, 3, (h / 3).max(1) as u32, core);
        }
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
            let sy = game.teams[ti].soldiers[si].pos.y - 8.0;
            let tip_x = (sx + dx * 12.0) as i32;
            let tip_y = (sy + dy * 12.0) as i32;
            if tip_x >= cam_x as i32 && tip_x < cam_x as i32 + sw {
                let phase = game.tick as f32 * 0.6;
                let r1 = if phase.sin() > 0.0 { 7 } else { 6 };
                let r2 = if phase.cos() > 0.0 { 5 } else { 4 };
                buf.fill_circle(tip_x, tip_y, r1, Bgra::new(220, 60, 10));
                buf.fill_circle(tip_x, tip_y, r2, Bgra::new(255, 150, 30));
                buf.fill_circle(tip_x, tip_y, 2,  Bgra::new(255, 240, 120));
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
        if wx >= vx0 && wx < vx1 && wy >= 0.0 && wy < sh as f32 {
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
                    (a, game.aim.power)
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
        let end = if rope.flying { rope.hook } else { rope.anchor };
        let rope_col = Bgra::new(180, 200, 140);
        let hook_col = Bgra::new(220, 180, 80);
        buf.draw_line(spos.x as i32, spos.y as i32 - 6, end.x as i32, end.y as i32, rope_col);
        buf.draw_line(spos.x as i32 + 1, spos.y as i32 - 5, end.x as i32, end.y as i32, rope_col);
        buf.fill_rect(end.x as i32 - 2, end.y as i32 - 2, 4, 4, hook_col);
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
        if wx >= vx0 - exp.radius && wx < vx1 + exp.radius && wy >= 0.0 && wy < sh as f32 + exp.radius {
            draw_explosion(buf, exp.pos, exp.radius, exp.age);
        }
    }

    // 7b'. Effect particles — explosion fallout, dust, sparks, splashes.
    crate::renderer::fx::draw_fx(buf, &game.fx, cam_x);

    // 7c. TNT fuse countdown banner — screen-anchored so it's visible regardless of camera
    if game.tnt_placed {
        use crate::physics::projectile::{WeaponKind, FuseState};
        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
        if let Some(tnt) = game.projectiles.iter().find(|p| p.kind == WeaponKind::Tnt) {
            if let FuseState::Burning(ticks) = tnt.fuse {
                let secs   = ticks / 30;
                let msg    = format!("TNT  {}", secs);
                let mw     = str_width_scaled(&msg, 2);
                let mx     = cam_x as i32 + sw / 2 - mw / 2;
                let my     = 24i32;
                buf.fill_rect(mx - 6, my - 4, (mw + 12) as u32, 22, Bgra::new(60, 10, 10));
                buf.fill_rect(mx - 6, my - 4, (mw + 12) as u32,  1, Bgra::new(200, 50, 30));
                buf.fill_rect(mx - 6, my + 17, (mw + 12) as u32, 1, Bgra::new(200, 50, 30));
                draw_str_scaled(buf, &msg, mx + 1, my + 1, Bgra::new(0, 0, 0), 2);
                draw_str_scaled(buf, &msg, mx,     my,     Bgra::new(255, 120, 60), 2);
            }
        }
    }

    mark!("fx_overlay");

    // 8. Status indicators — fuse timer (grenade), 2nd-shot prompt (shotgun), shots remaining (revolver)
    if game.turn.is_acting() {
        use crate::physics::projectile::WeaponKind;
        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
        let active = game.active_team_ref().active_soldier();
        if game.shotgun_shots_left > 0 && !active.has_fired {
            let msg = "SHOT 2 - PRESS A";
            let mw = str_width_scaled(msg, 2);
            let mx = cam_x as i32 + sw / 2 - mw / 2;
            let my = 4;
            buf.fill_rect(mx - 4, my - 3, (mw + 8) as u32, 19, Bgra::new(0, 0, 0));
            buf.fill_rect(mx - 4, my - 3, (mw + 8) as u32, 1, Bgra::new(80, 80, 100));
            buf.fill_rect(mx - 4, my + 15, (mw + 8) as u32, 1, Bgra::new(80, 80, 100));
            draw_str_scaled(buf, msg, mx + 1, my + 1, Bgra::new(0, 0, 0), 2);
            draw_str_scaled(buf, msg, mx,     my,     Bgra::new(255, 180, 60), 2);
        }
        if game.revolver_shots_left > 0 {
            let msg = format!("SHOTS: {}", game.revolver_shots_left);
            let mw = str_width_scaled(&msg, 2);
            let mx = cam_x as i32 + sw / 2 - mw / 2;
            let my = 4;
            buf.fill_rect(mx - 4, my - 3, (mw + 8) as u32, 19, Bgra::new(0, 0, 0));
            buf.fill_rect(mx - 4, my - 3, (mw + 8) as u32, 1, Bgra::new(80, 80, 100));
            buf.fill_rect(mx - 4, my + 15, (mw + 8) as u32, 1, Bgra::new(80, 80, 100));
            draw_str_scaled(buf, &msg, mx + 1, my + 1, Bgra::new(0, 0, 0), 2);
            draw_str_scaled(buf, &msg, mx,     my,     Bgra::new(255, 220, 80), 2);
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

        for ti in 0..game.teams.len().min(2) {
            let t = &game.teams[ti];
            let av_x = if ti == 0 {
                cam_x as i32 + 4
            } else {
                cam_x as i32 + sw - AV as i32 - 4
            };
            let av_y = 4i32;
            draw_avatar(buf, av_x, av_y, AV, t.avatar_id);

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

            // ELO — shown below HP bar for ranked matches
            if t.elo > 0 {
                use crate::renderer::font::{draw_str, str_width};
                let elo_str = format!("{}", t.elo);
                let ew = str_width(&elo_str);
                let ex = av_x + bar_w / 2 - ew / 2;
                let ey = bar_y + BAR_H + 2;
                draw_str(buf, &elo_str, ex, ey, TEAM_COLOURS[t.color_id as usize]);
            }
        }
    }

    mark!("avatars");

    // 8b/8c. Top-of-screen message stack — drawn after avatars so messages appear on top
    {
        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
        use crate::renderer::draw_sprites::TEAM_COLOURS;
        let cx = cam_x as i32 + sw / 2;
        let mut top_y: i32 = 4;

        // Word-wrap `text` into lines that each fit within `max_w` px at `scale`.
        let wrap = |text: &str, scale: i32, max_w: i32| -> Vec<String> {
            let mut lines: Vec<String> = Vec::new();
            let mut cur = String::new();
            for word in text.split_whitespace() {
                let trial = if cur.is_empty() { word.to_string() } else { format!("{} {}", cur, word) };
                if str_width_scaled(&trial, scale) <= max_w {
                    cur = trial;
                } else {
                    if !cur.is_empty() { lines.push(std::mem::take(&mut cur)); }
                    cur = word.to_string();
                }
            }
            if !cur.is_empty() { lines.push(cur); }
            if lines.is_empty() { lines.push(String::new()); }
            lines
        };

        let draw_top_msg =
            |buf: &mut crate::renderer::WorldBuffer, text: &str, col: Bgra, y: i32| -> i32 {
                const PAD: i32 = 4;
                let max_w = sw - PAD * 2;
                // Keep the big scale=2 font: fit on one line, else word-wrap into 2 lines.
                // Only drop to scale=1 if it still won't fit (very long text / single words).
                let (scale, lines) = if str_width_scaled(text, 2) <= max_w {
                    (2, vec![text.to_string()])
                } else {
                    let w2 = wrap(text, 2, max_w);
                    if w2.len() <= 2 && w2.iter().all(|l| str_width_scaled(l, 2) <= max_w) {
                        (2, w2)
                    } else {
                        (1, wrap(text, 1, max_w))
                    }
                };

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
            };

        // In live mode filter messages: hide crate-collection notices belonging to
        // the opponent so only the collecting player sees what they picked up.
        let visible_msgs: Vec<&GameMessage> = game.messages.iter().filter(|m| {
            match (my_team, m.team) {
                (Some(mine), Some(t)) => t == mine, // live: only own-team messages
                _ => true,
            }
        }).collect();

        let msg_colour = |t: Option<usize>| -> Bgra {
            match t {
                Some(t) => TEAM_COLOURS[game.teams.get(t).map(|tm| tm.color_id as usize).unwrap_or(t.min(3))],
                None    => Bgra::new(255, 210, 50),
            }
        };
        if let Some(msg) = visible_msgs.first() {
            let col = msg_colour(msg.team);
            let h = draw_top_msg(buf, &msg.text, col, top_y);
            top_y += h;
        }

        let mut extra_rows = 0;
        for msg in visible_msgs.iter().skip(1) {
            let col = msg_colour(msg.team);
            let h = draw_top_msg(buf, &msg.text, col, top_y);
            top_y += h;
            extra_rows += 1;
            if extra_rows >= 2 { break; }
        }
    }

    mark!("messages");

    // 8b. HUD — drawn at cam_x offset so it stays screen-anchored
    // HUD strips are keyed by colour identity (0-3 = Red/Blue/Green/Yellow), so
    // each player's strip shows in the colour they picked, regardless of their
    // compact team index.
    let find_team = |color: usize| game.teams.iter().find(|t| t.color_id as usize == color);
    let team_alive: [u32; 4] = std::array::from_fn(|i| {
        find_team(i).map(|t| t.alive_count()).unwrap_or(0)
    });
    let team_hp: [u32; 4] = std::array::from_fn(|i| {
        find_team(i).map(|t| t.total_hp()).unwrap_or(0)
    });
    let active_color = game.teams.get(game.active_team()).map(|t| t.color_id as usize).unwrap_or(0);
    // Clear stale HUD pixels (wind meter / weapon name / FPS) left over from a
    // previous frame's camera position before redrawing the HUD this frame.
    buf.fill_deep_water_band();

    draw_hud_world(buf, cam_x, &game.wind, game.turn.secs_remaining(),
        game.turn.turn_number, active_color, &team_alive, &team_hp);

    mark!("hud");

    // 9. Weapon indicator (bottom-left, shows current weapon name)
    {
        use crate::renderer::font::{draw_str, draw_str_shadow, str_width};
        use crate::renderer::fb::Bgra;
        use crate::physics::projectile::WeaponKind;
        use crate::world::{SCREEN_H, SCREEN_W};
        let ti = game.active_team();
        let si = game.teams[ti].active;
        let weapon = game.teams[ti].current_weapon();
        let name = match weapon {
            WeaponKind::Bazooka     => "BAZOOKA",
            WeaponKind::Grenade     => "GRENADE",
            WeaponKind::Shotgun     => "SHOTGUN",
            WeaponKind::ClusterBomb => "CLUSTER",
            WeaponKind::Tnt         => "TNT",
            WeaponKind::Landmine    => "MINE",
            WeaponKind::BananaBomb  => "METEOR BOMB",
            WeaponKind::Revolver    => "REVOLVER",
            WeaponKind::NinjaRope   => "GRAPPLE",
            WeaponKind::BaseballBat => "BAT",
            WeaponKind::Blasthive     => "BLASTHIVE",
            WeaponKind::BlackHoleBomb => "BLACK HOLE",
            WeaponKind::Minigun       => "MINIGUN",
            WeaponKind::Uzi           => "UZI",
            _ => "WEAPON",
        };
        // Small box bottom-left, sized to fit the weapon name + hint
        let bx = cam_x as i32 + 6;
        let by = SCREEN_H as i32 - 24;
        let hint = "[SEL]";
        let name_w = str_width(name);
        let hint_w = str_width(hint);
        let box_w = (name_w + hint_w + 12).max(74) as u32;
        buf.fill_rect(bx - 2, by - 2, box_w, 18, Bgra::new(10, 10, 25));
        draw_str_shadow(buf, name, bx, by, Bgra::new(255, 220, 80));
        draw_str(buf, hint, bx + name_w + 8, by + 2, Bgra::new(110, 110, 150));
    }

    mark!("weapon_indicator");

    // 9b. Seed display (TEST mode only) — upper-right corner, screen-anchored
    if game.is_test {
        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
        use crate::renderer::fb::Bgra;
        use crate::world::SCREEN_W;
        let arch_name = match game.terrain.archetype {
            0 => "HILLS",
            1 => "CLIFFS",
            2 => "ISLANDS",
            3 => "CAVERNS",
            4 => "MESA",
            _ => "?",
        };
        let label = format!("SEED {:016X}  {}", game.map_seed, arch_name);
        let w = str_width_scaled(&label, 2);
        let x = cam_x as i32 + SCREEN_W as i32 - w - 6;
        let y = 6;
        buf.fill_rect(x - 3, y - 2, (w + 6) as u32, 18, Bgra::new(10, 10, 25));
        draw_str_scaled(buf, &label, x, y, Bgra::new(180, 220, 120), 2);
    }

    mark!("seed_display");

    // 9c. FPS counter — bottom-right corner, screen-anchored
    {
        use crate::renderer::font::{draw_str, str_width};
        use crate::renderer::fb::Bgra;
        let fps_str = format!("{} FPS", lstate.display_fps);
        let x = cam_x as i32 + sw - str_width(&fps_str) - 6;
        let y = crate::renderer::HUD_Y - 12;
        draw_str(buf, &fps_str, x, y, Bgra::new(200, 200, 200));
    }
    mark!("fps_counter");

    // 9d. Per-section pixel-write breakdown (TEST mode only) — top sections by
    // pixel count, sorted descending, drawn below the seed display.
    if game.is_test {
        use crate::renderer::font::{draw_str_scaled, str_width_scaled};
        use crate::renderer::fb::Bgra;
        use crate::world::SCREEN_W;
        let mut sorted = pixel_stats.clone();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        let total_pw: u64 = sorted.iter().map(|(_, n)| n).sum();
        let mut y = 28;
        {
            let text = format!("{:<14}{:>7}", "TOTAL", total_pw);
            let w = str_width_scaled(&text, 1);
            let x = cam_x as i32 + SCREEN_W as i32 - w - 6;
            buf.fill_rect(x - 2, y - 1, (w + 4) as u32, 10, Bgra::new(30, 10, 10));
            draw_str_scaled(buf, &text, x, y, Bgra::new(255, 180, 80), 1);
            y += 11;
        }
        for (label, px) in sorted.iter().take(8) {
            let text = format!("{:<14}{:>7}", label, px);
            let w = str_width_scaled(&text, 1);
            let x = cam_x as i32 + SCREEN_W as i32 - w - 6;
            buf.fill_rect(x - 2, y - 1, (w + 4) as u32, 10, Bgra::new(10, 10, 25));
            draw_str_scaled(buf, &text, x, y, Bgra::new(220, 220, 220), 1);
            y += 10;
        }
    }

    lstate.pixel_stats = pixel_stats;
}

/// HUD drawn at world-space x=cam_x so it stays fixed on screen during panning.
fn draw_hud_world(
    buf:         &mut WorldBuffer,
    cam_x:       u32,
    wind:        &crate::physics::Wind,
    turn_secs:   u32,
    turn_number: u32,
    active_team: usize,
    team_alive:  &[u32; 4],
    total_hp:    &[u32; 4],
) {
    use crate::renderer::font::{draw_str, str_width};
    use crate::renderer::draw_sprites::TEAM_COLOURS;

    let ox    = cam_x as i32;
    let hud_y = crate::renderer::HUD_Y;
    let sw    = crate::world::SCREEN_W as i32;

    // Background bar
    buf.fill_rect(ox, hud_y, crate::world::SCREEN_W,
        crate::renderer::HUD_H, Bgra::new(15, 15, 25));

    // Timer
    let timer_str    = format!("{:02}", turn_secs);
    let timer_colour = if turn_secs <= 5 { Bgra::new(220, 60, 60) } else { Bgra::new(255, 220, 0) };
    let timer_x      = ox + sw - str_width(&timer_str) - 4;
    draw_str(buf, &timer_str, timer_x, hud_y + 6, timer_colour);

    // Turn number
    let turn_str = format!("T{}", turn_number);
    draw_str(buf, &turn_str, timer_x - str_width(&turn_str) - 6, hud_y + 6,
        Bgra::new(220, 220, 220));
    // Wind meter — centre-anchored deflection gauge
    let bar_w = 160i32;
    let bar_h = 8u32;
    let bar_x = ox + sw / 2 - bar_w / 2;
    let bar_y = hud_y + 6;
    let centre_x = bar_x + bar_w / 2;
    let wcolour = if wind.value() >= 0.0 { Bgra::new(80, 180, 255) } else { Bgra::new(255, 140, 60) };
    // Background + quarter-marks
    buf.fill_rect(bar_x, bar_y, bar_w as u32, bar_h, Bgra::new(40, 40, 60));
    buf.fill_rect(bar_x + bar_w/4 - 1, bar_y, 1, bar_h, Bgra::new(70, 70, 100));
    buf.fill_rect(bar_x + 3*bar_w/4,   bar_y, 1, bar_h, Bgra::new(70, 70, 100));
    // Fill from centre outward toward wind direction
    let fill = (wind.value().abs() * (bar_w / 2) as f32) as u32;
    if fill > 0 {
        if wind.value() >= 0.0 {
            buf.fill_rect(centre_x, bar_y, fill, bar_h, wcolour);
        } else {
            buf.fill_rect(centre_x - fill as i32, bar_y, fill, bar_h, wcolour);
        }
    }
    // Centre tick — zero-wind reference point
    buf.fill_rect(centre_x - 1, bar_y, 2, bar_h, Bgra::new(140, 140, 180));

    // Team strips (left)
    for team in 0..4usize {
        if team_alive[team] == 0 { continue; }
        let strip_x = ox + 4 + team as i32 * 36;
        let strip_y = hud_y + 3;
        let colour  = TEAM_COLOURS[team];

        if team == active_team {
            buf.fill_rect(strip_x - 1, strip_y - 1, 34, 16, Bgra::new(255, 220, 0));
        }
        buf.fill_rect(strip_x, strip_y, 32, 14, Bgra::new(15, 15, 25));

        let alive_str = format!("x{}", team_alive[team]);
        draw_str(buf, &alive_str, strip_x + 1, strip_y + 1, colour);

        let max_hp  = team_alive[team] * 100;
        let hp_frac = (total_hp[team] as f32 / max_hp as f32).clamp(0.0, 1.0);
        let bar_w   = (28.0 * hp_frac) as u32;
        buf.fill_rect(strip_x + 2, strip_y + 9, 28, 3, Bgra::new(40, 40, 40));
        if bar_w > 0 {
            buf.fill_rect(strip_x + 2, strip_y + 9, bar_w, 3, colour);
        }
    }
}

/// Re-stamp the object mask each tick from current barrel and armed-mine positions.
/// Must be called before any collision checks so soldiers and projectiles treat
/// barrels and armed mines as solid obstacles (Worms-style object mask).
fn stamp_objects(game: &mut GameState) {
    use crate::game::state::MineState;
    game.terrain.clear_objects();

    // Barrels: 14×24 px footprint matching the visual (pos.y = terrain surface,
    // barrel extends upward to pos.y-24; dx ±7 matches the drawn body width).
    for barrel in &game.barrels {
        let cx = barrel.pos.x as i32;
        let cy = barrel.pos.y as i32;
        for dy in -24..=0i32 {
            for dx in -7..=7i32 {
                game.terrain.stamp_object(cx + dx, cy + dy);
            }
        }
    }

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
                if hx < 0.0 || hx >= crate::world::WORLD_W as f32
                    || hy < 0.0
                    || hy >= crate::world::WATER_Y as f32
                {
                    // Missed — cancel rope
                    game.rope = None;
                    attached = true; // use flag to break without reborrow
                    break;
                }
                if game.terrain.is_solid(hx as i32, hy as i32) {
                    let soldier_pos = game.teams[ati].soldiers[asi].pos;
                    let dx = hx - soldier_pos.x;
                    let dy = hy - soldier_pos.y;
                    let dist = (dx * dx + dy * dy).sqrt().max(1.0).min(ROPE_MAX_LEN);
                    let anchor = crate::world::WorldPos::new(hx, hy);
                    if let Some(ref mut r) = game.rope {
                        r.flying = false;
                        r.anchor = anchor;
                        r.hook   = anchor;
                        r.length = dist;
                    }
                    // Lift soldier into Airborne with an angle-aware nudge.
                    // The upward component is proportional to how much above the soldier the anchor is,
                    // so a steep rope gives a strong upward kick and a low rope gives mostly horizontal.
                    let fm = game.teams[ati].soldiers[asi].facing as f32;
                    if matches!(game.teams[ati].soldiers[asi].state, SoldierState::Idle | SoldierState::Walking { .. }) {
                        let spos = game.teams[ati].soldiers[asi].pos;
                        let rdx  = anchor.x - spos.x;
                        let rdy  = anchor.y - spos.y; // negative when anchor is above
                        let rdist = (rdx*rdx + rdy*rdy).sqrt().max(1.0);
                        let up_frac = (-rdy / rdist).max(0.0); // 0 when horizontal, 1 when vertical
                        let vel_up    = up_frac * 6.0;         // strong upward kick scaled by angle
                        let vel_horiz = fm * (1.0 - up_frac * 0.5).max(0.5) * 3.0; // less horiz when more vertical
                        game.teams[ati].soldiers[asi].fall.begin_fall(spos.y);
                        game.teams[ati].soldiers[asi].state = SoldierState::Airborne {
                            vel: crate::world::Vec2::new(vel_horiz, -vel_up),
                            spinning: false,
                        };
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
            let on_ground = is_on_ground(game, ti, si);
            let state = game.teams[ti].soldiers[si].state.clone();
            match state {
                SoldierState::Airborne { mut vel, mut spinning } => {
                    // ── Rope constraint physics (active soldier only) ─────────
                    let is_active = ti == ati && si == asi;
                    if is_active {
                        if let Some(rope) = game.rope.as_ref().filter(|r| !r.flying) {
                            let mut anchor = rope.anchor;
                            // Target length BEFORE any corner-wrap this tick. The
                            // momentum-conservation step below compares the post-wrap
                            // length against this so a wrap-shortening feeds a
                            // "slingshot around the corner" speed boost (WA feel).
                            let length0 = rope.length;

                            // Position is the swing pivot for all the tangent math below.
                            let cx = game.teams[ti].soldiers[si].pos.x;
                            let cy = game.teams[ti].soldiers[si].pos.y;

                            // ── Corner wrap (single-segment) ─────────────────────────
                            // Sample the line anchor→soldier every 6px. If it crosses
                            // terrain, re-anchor at the last clear sample (the corner the
                            // rope bends over) and shorten the free segment. This keeps a
                            // one-anchor system (no chain) yet lets the soldier swing
                            // around pillars / overhangs — the signature WA rope move.
                            {
                                let wdx = cx - anchor.x;
                                let wdy = cy - anchor.y;
                                let wdist = (wdx * wdx + wdy * wdy).sqrt().max(0.1);
                                let ux = wdx / wdist;
                                let uy = wdy / wdist;
                                let check_steps = (wdist / 6.0) as u32;
                                let mut last_clear = 0.0f32;
                                let mut hit = false;
                                for step in 1..=check_steps {
                                    let d = step as f32 * 6.0;
                                    let sx = anchor.x + ux * d;
                                    let sy = anchor.y + uy * d;
                                    if game.terrain.is_solid(sx as i32, sy as i32) { hit = true; break; }
                                    last_clear = d;
                                }
                                // Only re-anchor when the corner is a meaningful distance
                                // from the current anchor (>=2px) to avoid per-pixel jitter.
                                if hit && last_clear >= 2.0 {
                                    let new_anchor = crate::world::WorldPos::new(
                                        anchor.x + ux * last_clear,
                                        anchor.y + uy * last_clear,
                                    );
                                    let remaining = (rope.length - last_clear).max(ROPE_MIN_LEN);
                                    if let Some(ref mut rm) = game.rope {
                                        rm.anchor = new_anchor;
                                        rm.length = remaining;
                                    }
                                    anchor = new_anchor;
                                }
                            }
                            // Post-wrap target length and rope direction from the
                            // (possibly moved) anchor.
                            let length = game.rope.as_ref().map(|r| r.length).unwrap_or(length0);
                            let rdx = cx - anchor.x;
                            let rdy = cy - anchor.y;
                            let rdist = (rdx * rdx + rdy * rdy).sqrt().max(0.1);
                            let dir_x = rdx / rdist;
                            let dir_y = rdy / rdist;
                            let effective_len = length;

                            // 1. Pendulum gravity
                            vel.y = (vel.y + ROPE_GRAVITY).min(ROPE_MAX_SPEED);
                            // 2. Swing force from Left/Right input
                            if input.held(Button::Left)  { vel.x -= ROPE_SWING_FORCE; }
                            if input.held(Button::Right) { vel.x += ROPE_SWING_FORCE; }
                            // 3. Angular momentum conservation on rope-length change.
                            //    When the rope shortens, tangential speed must increase to
                            //    conserve angular momentum (L = r × v_tangential = constant).
                            //    This is the "figure skater pulling arms in" acceleration.
                            if let Some(ref rope_m) = game.rope {
                                let new_len = rope_m.length;
                                if new_len < length0 && new_len > 0.1 {
                                    let scale = length0 / new_len; // conservation factor
                                    // Only scale the tangential component
                                    let radial_pre = vel.x * dir_x + vel.y * dir_y;
                                    let tx = vel.x - dir_x * radial_pre;
                                    let ty = vel.y - dir_y * radial_pre;
                                    vel.x = dir_x * radial_pre + tx * scale.min(2.0);
                                    vel.y = dir_y * radial_pre + ty * scale.min(2.0);
                                }
                            }
                            // 4. Project velocity onto tangent plane (remove outward radial).
                            let radial = vel.x * dir_x + vel.y * dir_y;
                            if radial > 0.0 {
                                vel.x -= dir_x * radial;
                                vel.y -= dir_y * radial;
                            }
                            // 5. Cap speed to prevent tunnelling
                            let spd = (vel.x * vel.x + vel.y * vel.y).sqrt();
                            if spd > ROPE_MAX_SPEED {
                                let s = ROPE_MAX_SPEED / spd;
                                vel.x *= s;
                                vel.y *= s;
                            }
                            // 6. Step with tangential velocity
                            let mut nx = cx + vel.x;
                            let mut ny = cy + vel.y;
                            // 7. Position clamp to rope length
                            let dx = nx - anchor.x;
                            let dy = ny - anchor.y;
                            let dist = (dx * dx + dy * dy).sqrt();
                            if dist > effective_len && dist > 0.1 {
                                nx = anchor.x + dx / dist * effective_len;
                                ny = anchor.y + dy / dist * effective_len;
                            }
                            // 5. Terrain collision — swept check along the full movement
                            // path so angled/horizontal swings can't phase through walls.
                            // Still skips the first tick (total speed < 3) so newly-attached
                            // rope doesn't immediately cancel by detecting the ground underfoot.
                            let move_len = ((nx - cx).abs() + (ny - cy).abs()).ceil() as i32 + 1;
                            let mut last_clear_x = cx;
                            let mut last_clear_y = cy;
                            let mut hit = false;
                            if (vel.x * vel.x + vel.y * vel.y).sqrt() > 3.0 {
                                for s in 1..=move_len {
                                    let t = s as f32 / move_len as f32;
                                    let sx = cx + (nx - cx) * t;
                                    let sy = cy + (ny - cy) * t;
                                    if (0..=crate::renderer::draw_sprites::SOLDIER_H)
                                        .any(|h| game.terrain.is_solid(sx as i32, sy as i32 - h))
                                    {
                                        hit = true;
                                        break;
                                    }
                                    last_clear_x = sx;
                                    last_clear_y = sy;
                                }
                            }
                            if hit {
                                // Swinging into ground — detach rope and land at last clear pos
                                let land_y = land_on_surface(&game.terrain, last_clear_x, last_clear_y) as f32;
                                let dmg = game.teams[ti].soldiers[si].fall.land(land_y);
                                if dmg > 0 {
                                    game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Fall;
                                    game.teams[ti].soldiers[si].take_damage(dmg);
                                }
                                game.teams[ti].soldiers[si].pos.x = last_clear_x;
                                game.teams[ti].soldiers[si].pos.y = land_y;
                                game.teams[ti].soldiers[si].airtime = 0;
                                if game.teams[ti].soldiers[si].is_dead() {
                                    game.teams[ti].soldiers[si].state = SoldierState::Dead;
                                } else {
                                    game.teams[ti].soldiers[si].state = SoldierState::Idle;
                                }
                                game.rope = None;
                                game.rope_session = false; // landed — session over, turn continues
                            } else if ny >= crate::world::WATER_Y as f32 {
                                // Drowned
                                game.teams[ti].soldiers[si].death_cause = crate::game::soldier::DeathCause::Water;
                                game.teams[ti].soldiers[si].take_damage(999);
                                game.teams[ti].soldiers[si].state = SoldierState::Dead;
                                let ati = game.active_team();
                                if ti == ati && si == game.teams[ati].active { game.active_worm_hit = true; }
                                game.rope = None;
                                game.rope_session = false;
                            } else if nx < 0.0 || nx >= crate::world::WORLD_W as f32 {
                                // Soldier swung off the map edge — drown/die
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
                            }
                            continue; // skip normal gravity below
                        }
                    }
                    // ── Normal airborne physics ───────────────────────────────
                    vel.y = (vel.y + 0.5).min(23.0); // raised cap to preserve rope-release / slingshot momentum
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
                    let mut landed = false;
                    for _ in 0..steps {
                        cx += sx_;
                        cy += sy_;
                        let ix = cx as i32;
                        let iy = cy as i32;
                        let ix_l = ix - crate::renderer::draw_sprites::SOLDIER_HALF_W as i32;
                        let ix_r = ix + crate::renderer::draw_sprites::SOLDIER_HALF_W as i32;
                        let terrain_hit = (0..=crate::renderer::draw_sprites::SOLDIER_H)
                            .any(|h| game.terrain.is_blocked(ix_l, iy - h)
                                || game.terrain.is_blocked(ix,   iy - h)
                                || game.terrain.is_blocked(ix_r, iy - h));
                        let soldier_hit = !terrain_hit && game.teams.iter().enumerate().any(|(oti, oteam)| {
                            oteam.soldiers.iter().enumerate().any(|(osi, os)| {
                                if (oti == ti && osi == si) || !os.is_alive() { return false; }
                                let ox = os.pos.x as i32;
                                let oy = os.pos.y as i32;
                                (ix - ox).abs() < crate::renderer::draw_sprites::SOLDIER_W as i32
                                    && iy >= oy - crate::renderer::draw_sprites::SOLDIER_H as i32
                                    && iy <= oy + crate::renderer::draw_sprites::SOLDIER_H as i32
                            })
                        });
                        if soldier_hit && dy >= 0.0 {
                            // Landed on another soldier. Rather than bouncing in place
                            // (which kept the soldier airborne, froze the turn and shook
                            // the camera), slide off to the nearest clear side and settle
                            // on the terrain there.
                            let sw = crate::renderer::draw_sprites::SOLDIER_W as i32;
                            let sh = crate::renderer::draw_sprites::SOLDIER_H as i32;
                            // Center x of the soldier we landed on (push away from it).
                            let mut other_cx = cx;
                            'find_other: for (oti, oteam) in game.teams.iter().enumerate() {
                                for (osi, os) in oteam.soldiers.iter().enumerate() {
                                    if (oti == ti && osi == si) || !os.is_alive() { continue; }
                                    let ox = os.pos.x as i32;
                                    let oy = os.pos.y as i32;
                                    if (ix - ox).abs() < sw && iy >= oy - sh && iy <= oy + 1 {
                                        other_cx = os.pos.x;
                                        break 'find_other;
                                    }
                                }
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
                                game.rope_session = false;
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
                                let wl = wx - (crate::renderer::draw_sprites::SOLDIER_HALF_W as i32 - 1);
                                let wr = wx + (crate::renderer::draw_sprites::SOLDIER_HALF_W as i32 - 1);
                                let x_hit = (0..=crate::renderer::draw_sprites::SOLDIER_H)
                                    .any(|h| game.terrain.is_blocked(wl, wy_c - h)
                                        || game.terrain.is_blocked(wx, wy_c - h)
                                        || game.terrain.is_blocked(wr, wy_c - h));
                                let y_hit = (0..=crate::renderer::draw_sprites::SOLDIER_H)
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
                                    game.rope_session = false;
                                    // turn continues — no on_fired()
                                }
                                landed = true;
                            }
                            break;
                        }
                        // Flew off the map edge — instant kill
                        if cx < 0.0 || cx >= crate::world::WORLD_W as f32 {
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
                    if !on_ground {
                        let y0 = game.teams[ti].soldiers[si].pos.y;
                        game.teams[ti].soldiers[si].fall.begin_fall(y0);
                        game.teams[ti].soldiers[si].state = SoldierState::Airborne { vel: crate::world::Vec2::new(0.0, 0.0), spinning: false };
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
    for team in &mut game.teams {
        for s in &mut team.soldiers {
            if s.hp_display_ticks > 0 { s.hp_display_ticks -= 1; }
            if s.displayed_hp > s.hp { s.displayed_hp = s.displayed_hp.saturating_sub(1).max(s.hp); }
            else if s.displayed_hp < s.hp { s.displayed_hp = s.hp; }
        }
    }
    game.messages.retain_mut(|m| { m.ticks = m.ticks.saturating_sub(1); m.ticks > 0 });
    game.bullet_trails.retain_mut(|t| { if t.2 > 0 { t.2 -= 1; true } else { false } });
}

/// Draw the weapon-selection overlay when game.weapon_menu_open is true.
/// Call after render() in TAT and live-mode client paths.
/// Compute kill/HP stats and pick a memorable one-liner for the end screen.
/// Returns ([team0_kills, team1_kills], [team0_hp, team1_hp], memorable_line).
pub fn match_end_stats(game: &GameState) -> ([u32; 2], [u32; 2], String) {
    let kills0 = game.teams.get(1).map(|t| t.soldiers.iter().filter(|s| s.is_dead()).count() as u32).unwrap_or(0);
    let kills1 = game.teams.get(0).map(|t| t.soldiers.iter().filter(|s| s.is_dead()).count() as u32).unwrap_or(0);
    let hp0    = game.teams.get(0).map(|t| t.total_hp()).unwrap_or(0);
    let hp1    = game.teams.get(1).map(|t| t.total_hp()).unwrap_or(0);

    let winner_alive0 = game.teams.get(0).map(|t| t.alive_count()).unwrap_or(0);
    let winner_alive1 = game.teams.get(1).map(|t| t.alive_count()).unwrap_or(0);
    let craters       = game.crater_log.len() as u32;
    let turns         = game.turn.turn_number / 2;
    let big_blast     = game.crater_log.iter().any(|c| c.2 > 65.0);
    let winner_hp     = hp0.max(hp1);
    let clean_sweep   = kills0 == 4 || kills1 == 4;

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
    if winner_alive0 == 1 || winner_alive1 == 1 { candidates.push("Last soldier standing.".to_string()); }
    // Always-eligible fallbacks
    candidates.push("Well fought.".to_string());
    candidates.push("The battlefield is silent.".to_string());
    candidates.push("History will remember this.".to_string());

    // Use map_seed — stable for the entire match, never changes after game ends.
    let pick = (game.map_seed.wrapping_mul(2654435761) as usize) % candidates.len();
    ([kills0, kills1], [hp0, hp1], candidates.swap_remove(pick))
}

pub fn draw_weapon_menu_overlay(game: &GameState, buf: &mut WorldBuffer, cam_x: i32) {
    if !game.weapon_menu_open { return; }
    let ti = game.active_team();
    let si = game.teams[ti].active;
    draw_weapon_menu(buf, &game.teams[ti].weapons, game.weapon_menu_cursor, cam_x, game.aim.fuse_ticks, game.turn.turn_number, game.teams.len());
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
pub fn server_tick(game: &mut GameState, input: &crate::input::InputState, muzzle: Option<(f32, f32)>) {
    game.tick = game.tick.wrapping_add(1);
    let _ = simulate_with_muzzle(game, input, muzzle);
}

pub fn push_active_soldier_out(game: &mut GameState) {
    use crate::renderer::draw_sprites::{SOLDIER_H, SOLDIER_HALF_W};
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
                let terrain_clear = (0..=SOLDIER_H).all(|h|
                    !game.terrain.is_blocked(ix - SOLDIER_HALF_W as i32, iy - h)
                    && !game.terrain.is_blocked(ix, iy - h)
                    && !game.terrain.is_blocked(ix + SOLDIER_HALF_W as i32, iy - h));
                if terrain_clear {
                    game.teams[active_ti].soldiers[active_si].pos.x = new_x;
                }
            }
        }
    }
}
