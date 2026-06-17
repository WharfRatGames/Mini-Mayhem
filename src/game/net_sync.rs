//! Server→client state synchronisation, shared by the `server` binary
//! (`build_state`) and the `arty` client (`apply_server_state`).
//!
//! Living in the lib (rather than each bin) makes both ends importable from a
//! single place so the `tests/parity.rs` integration test can drive them
//! together and catch any field that the sim changes but the sync drops.

use crate::game::state::GameState;
use crate::game::turn::TurnPhase;
use crate::net::msg::*;
use crate::renderer::Camera;

/// Serialise the authoritative game state into a `StateMsg` for broadcast.
pub fn build_state(game: &GameState, tick: u32, _crater_start: usize) -> StateMsg {
    let phase = match game.turn.phase {
        TurnPhase::Acting       => NetPhase::Acting,
        TurnPhase::Watching     => NetPhase::Watching,
        TurnPhase::Retreating{..}=> NetPhase::Retreating,
        TurnPhase::Ending       => NetPhase::Ending,
    };
    StateMsg {
        tick,
        soldiers: game.teams.iter().enumerate().flat_map(|(ti, t)| t.soldiers.iter().map(move |s| {
            use crate::game::soldier::SoldierState;
            let (airborne, spinning, vel) = match s.state {
                SoldierState::Airborne { spinning, vel } => (true, spinning, vel),
                _ => (false, false, crate::world::Vec2::new(0.0, 0.0)),
            };
            NetSoldier {
                team: ti, color_id: t.color_id, index: s.index,
                x: s.pos.x, y: s.pos.y,
                hp: s.hp, facing: s.facing, dead: s.is_dead(), has_fired: s.has_fired,
                selected_weapon: t.selected_weapon,
                airborne, spinning, vel_x: vel.x, vel_y: vel.y, airtime: s.airtime, walk_ticks: s.walk_ticks,
                walking: matches!(s.state, SoldierState::Walking { .. }),
                hat_id: s.hat_id, uniform_color_id: s.uniform_color_id,
                boot_color_id: s.boot_color_id, gun_style_id: s.gun_style_id,
                name: s.name.clone(),
                death_cause_u8: {
                    use crate::game::soldier::DeathCause;
                    match s.death_cause {
                        DeathCause::Generic => 0, DeathCause::Explosion => 1,
                        DeathCause::Fall => 2, DeathCause::Water => 3,
                    }
                },
            }
        })).collect(),
        projectiles: game.projectiles.iter().map(|p| {
            use crate::physics::projectile::FuseState;
            NetProjectile {
                x: p.pos.x, y: p.pos.y,
                vel_x: p.vel.x, vel_y: p.vel.y,
                kind_u8: p.kind.to_net_u8(),
                fuse_ticks: match p.fuse {
                    FuseState::Burning(n)    => n,
                    FuseState::Armed         => 0xFFFF_FFFE,
                    FuseState::Detonating(n) => 0x8000_0000 | n,
                    _                        => 0,
                },
                is_fragment: p.is_fragment,
            }
        }).collect(),
        wind: game.wind.value(),
        turn_team: game.active_team(),

        turn_secs: game.turn.secs_remaining(),
        active_soldier: game.teams[game.active_team()].active,
        phase,
        aim_angle: game.aim.angle,
        aim_power: game.aim.power,
        result: match game.result {
            crate::game::state::GameResult::Ongoing   => NetResult::Ongoing,
            crate::game::state::GameResult::Winner(t) => NetResult::Winner(t),
            crate::game::state::GameResult::Draw      => NetResult::Draw,
        },
        // Send the full crater log every tick — a StateMsg can be silently
        // dropped (write_team! ignores write errors under the 50ms write
        // timeout), and a dropped delta would permanently desync the
        // client's terrain. The client dedupes by length (see apply_server_state).
        craters: game.crater_log.iter()
            .map(|e| NetCrater { cx: e.0, cy: e.1, radius: e.2 }).collect(),
        messages: game.messages.iter()
            .map(|m| NetMessage { text: m.text.clone(), team: m.team.map(|t| t as i8).unwrap_or(-1), ticks: m.ticks })
            .collect(),
        graves: game.graves.iter().map(|g| NetGrave {
            x: g.pos.x, y: g.pos.y, team: g.team, headstone_id: g.headstone_id,
        }).collect(),
        blood_splats: game.blood_splats.iter().map(|(p, t)| NetBloodSplat {
            x: p.x, y: p.y, ticks: *t,
        }).collect(),
        weapon_menu_open:   game.weapon_menu_open,
        weapon_menu_cursor: game.weapon_menu_cursor,
        aim_fuse_ticks:     game.aim.fuse_ticks,
        crates: game.crates.iter().map(|c| {
            use crate::game::state::CrateKind;
            NetCrate {
                x: c.pos.x, y: c.pos.y, landed: c.landed,
                kind_u8: match c.kind { CrateKind::Health => 0, CrateKind::Weapon(_) => 1, CrateKind::Scrap(_) => 2 },
            }
        }).collect(),
        mines: game.mines.iter().map(|m| {
            use crate::game::state::MineState;
            NetMine {
                x: m.pos.x, y: m.pos.y,
                state_u8: match m.state { MineState::Arming => 0, MineState::Armed => 1, MineState::Triggered => 2 },
                arm_ticks: m.arm_ticks,
                trigger_ticks: m.trigger_ticks,
            }
        }).collect(),
        sounds: game.sounds.clone(),
        fx_events: game.fx_events.clone(),
        barrels: game.barrels.iter().map(|b| NetBarrel {
            x: b.pos.x, y: b.pos.y, hp: b.hp,
        }).collect(),
        black_holes: game.black_holes.iter().map(|h| NetBlackHole {
            x: h.pos.x, y: h.pos.y, ticks_left: h.lifetime,
        }).collect(),
        fire_patches: game.fire_patches.iter().map(|f| NetFirePatch {
            x: f.pos.x, y: f.pos.y, lifetime: f.lifetime,
            landed: f.landed, vel_x: f.vel.x, vel_y: f.vel.y,
        }).collect(),
        rope: game.rope.as_ref().map(|r| NetRope {
            anchor_x: r.anchor.x, anchor_y: r.anchor.y,
            hook_x: r.hook.x, hook_y: r.hook.y,
            flying: r.flying, length: r.length,
        }),
        team_names: game.teams.iter().map(|t| t.name.clone()).collect(),
        team_colors: game.teams.iter().map(|t| t.color_id).collect(),
        garcia: game.garcia.as_ref().map(|g| NetGarcia {
            cursor_x: g.cursor_x, render_x: g.render_x, cursor_y: g.cursor_y, render_y: g.render_y,
            blink_timer: g.blink_timer,
            falling: g.falling, fall_y: g.fall_y, vel_y: g.vel_y, bounce_count: g.bounce_count,
        }),
        airstrike: game.airstrike.as_ref().map(|a| NetAirstrike {
            cursor_x: a.cursor_x, render_x: a.render_x, cursor_y: a.cursor_y, render_y: a.render_y,
            blink_timer: a.blink_timer, active: a.active,
            plane_x: a.plane_x, plane_vx: a.plane_vx,
            bombs_dropped: a.bombs_dropped, direction_right: a.direction_right,
        }),
        torch_dir: {
            use crate::game::state::TorchDir;
            match game.plasma_torch.as_ref().map(|t| t.dir) {
                Some(TorchDir::UpForward)   => 1,
                Some(TorchDir::Forward)     => 2,
                Some(TorchDir::DownForward) => 3,
                None                        => 0,
            }
        },
        paused_opponent: None,
        opponent_abandoned: false,
        team_weapons: game.teams.iter().map(|t| NetTeamWeapons {
            selected: t.selected_weapon,
            weapons: t.weapons.iter().map(|(k, a)| {
                (k.to_net_u8(), a.map_or(0xFFFF, |n| n as u32))
            }).collect(),
        }).collect(),
    }
}

/// Apply a received `StateMsg` onto the client's game (server is authoritative).
///
/// Cosmetic FX (particle bursts) are delivered separately via the `fx_events`
/// channel and replayed by the caller's frame loop — this function only
/// reconstructs gameplay/visible *state* (positions, terrain, crates, etc.).
pub fn apply_server_state(
    game:    &mut GameState,
    _cam:    &mut Camera,
    state:   &StateMsg,
    my_team: usize,
) {
    // Soldiers that newly died this state update — death messages are generated
    // client-side (server only has default names; the client uses the names it
    // actually displays). Death SFX still come from StateMsg.sounds.
    let mut new_deaths: Vec<(String, usize, usize, u8)> = Vec::new();
    for ns in &state.soldiers {
        if let Some(team) = game.teams.get_mut(ns.team) {
            if let Some(soldier) = team.soldiers.get_mut(ns.index) {
                let was_alive = soldier.hp > 0;
                if ns.dead && was_alive {
                    new_deaths.push((soldier.name.clone(), ns.team, ns.index, ns.death_cause_u8));
                }
                if ns.hp < soldier.hp { soldier.hp_display_ticks = 150; }
                soldier.pos.x           = ns.x;
                soldier.pos.y           = ns.y;
                soldier.hp              = ns.hp;
                soldier.facing          = ns.facing;
                soldier.has_fired       = ns.has_fired;
                soldier.airtime         = ns.airtime;
                soldier.walk_ticks      = ns.walk_ticks;
                // Sync opponent cosmetics and names (local player's own are set from roster at game start)
                if ns.team != my_team {
                    soldier.hat_id           = ns.hat_id;
                    soldier.uniform_color_id = ns.uniform_color_id;
                    soldier.boot_color_id    = ns.boot_color_id;
                    soldier.gun_style_id     = ns.gun_style_id;
                    soldier.name             = ns.name.clone();
                }
                use crate::game::soldier::SoldierState;
                soldier.state = if ns.dead {
                    SoldierState::Dead
                } else if ns.airborne {
                    SoldierState::Airborne { vel: crate::world::Vec2::new(ns.vel_x, ns.vel_y), spinning: ns.spinning }
                } else if ns.walking {
                    SoldierState::Walking { dir: ns.facing as f32 }
                } else {
                    SoldierState::Idle
                };
            }
        }
        // Sync opponent team's selected weapon for correct gun visual
        if ns.team != my_team {
            if let Some(team) = game.teams.get_mut(ns.team) {
                team.selected_weapon = ns.selected_weapon;
            }
        }
    }
    // Push client-side death messages for soldiers that just died (parity with
    // local modes; uses the names the client displays + the networked cause).
    for (name, team, idx, cause_u8) in new_deaths {
        use crate::game::soldier::DeathCause;
        let cause = match cause_u8 {
            1 => DeathCause::Explosion, 2 => DeathCause::Fall,
            3 => DeathCause::Water, _ => DeathCause::Generic,
        };
        let seed = game.tick.wrapping_mul(1664525)
            .wrapping_add(team as u32 * 7).wrapping_add(idx as u32 * 13);
        let phrase = crate::game::loop_runner::death_phrase(cause, seed);
        game.messages.push(crate::game::state::GameMessage {
            text: format!("{} {}", name, phrase), team: Some(team), ticks: 120,
        });
    }
    game.projectiles.clear();
    for np in &state.projectiles {
        use crate::physics::projectile::{Projectile, WeaponKind, FuseState};
        use crate::world::{Vec2, WorldPos};
        let kind = WeaponKind::from_net_u8(np.kind_u8);
        let mut proj = Projectile::new(WorldPos::new(np.x, np.y), Vec2::new(np.vel_x, np.vel_y), kind);
        proj.fuse = match np.fuse_ticks {
            0xFFFF_FFFE       => FuseState::Armed,
            n if n & 0x8000_0000 != 0 => FuseState::Detonating(n & !0x8000_0000),
            0                 => FuseState::None,
            n                 => FuseState::Burning(n),
        };
        proj.is_fragment = np.is_fragment;
        game.projectiles.push(proj);
    }
    // On opponent's turn, apply their aim angle from server state for display.
    // On our turn, keep local angle (managed by process_aim() for smooth no-RTT aiming).
    if state.turn_team != my_team {
        game.aim.angle = state.aim_angle;
    }
    game.aim.power          = state.aim_power;
    // aim_fuse_ticks, weapon_menu_open/cursor, selected_weapon managed locally.
    // Detect turn change for message queue
    let prev_team   = game.turn.current_team;
    let prev_active = game.teams.get(prev_team).map(|t| t.active).unwrap_or(0);

    game.wind               = crate::physics::Wind::new(state.wind);
    game.turn.ticks_left = state.turn_secs * 30;
    game.turn.current_team = state.turn_team;

    if state.turn_team != prev_team || state.active_soldier != prev_active {
        crate::game::loop_runner::push_turn_message(game);
    }
    if let Some(t) = game.teams.get_mut(state.turn_team) { t.active = state.active_soldier; }
    // Snap the camera to the new active soldier on a turn change so it doesn't
    // lerp/shake across the map from the previous turn's framing.
    if state.turn_team != prev_team || state.active_soldier != prev_active {
        if let Some(s) = game.teams.get(state.turn_team).and_then(|t| t.soldiers.get(state.active_soldier)) {
            _cam.snap_to(s.pos);
        }
    }
    game.turn.phase = match state.phase {
        NetPhase::Acting       => crate::game::turn::TurnPhase::Acting,
        NetPhase::Watching     => crate::game::turn::TurnPhase::Watching,
        NetPhase::Retreating   => crate::game::turn::TurnPhase::Retreating { ticks_left: 30 },
        NetPhase::Ending       => crate::game::turn::TurnPhase::Ending,
    };
    // Sync game result from server
    game.result = match state.result {
        NetResult::Ongoing   => crate::game::state::GameResult::Ongoing,
        NetResult::Winner(t) => crate::game::state::GameResult::Winner(t),
        NetResult::Draw      => crate::game::state::GameResult::Draw,
    };
    // Sync crate positions from server (server is authoritative on drops/collection)
    game.crates = state.crates.iter().map(|nc| {
        use crate::game::state::CrateKind;
        use crate::physics::projectile::WeaponKind;
        crate::game::state::DroppedCrate {
            pos:        crate::world::WorldPos::new(nc.x, nc.y),
            // Variant only drives the rendered colour/symbol; payload is a
            // placeholder (server owns the real contents on collection).
            kind:       match nc.kind_u8 {
                1 => CrateKind::Weapon(WeaponKind::Bazooka),
                2 => CrateKind::Scrap(0),
                _ => CrateKind::Health,
            },
            landed:     nc.landed,
            descent_vy: 1.5,
            damage_this_turn: 0,
            fall_ticks: 0,
        }
    }).collect();

    // Sync mines (server-authoritative: positions, states, countdowns)
    game.mines = state.mines.iter().map(|nm| {
        use crate::game::state::{PlacedMine, MineState};
        use crate::world::WorldPos;
        PlacedMine {
            pos: WorldPos::new(nm.x, nm.y),
            state: match nm.state_u8 {
                2 => MineState::Triggered,
                1 => MineState::Armed,
                _ => MineState::Arming,
            },
            arm_ticks: nm.arm_ticks,
            trigger_ticks: nm.trigger_ticks,
        }
    }).collect();

    // Plasma torch: reconstruct the active torch state from the networked direction
    // so the live client draws the flame at the tip (5c in render). While the torch
    // is active, its terrain carving produces a stream of craters every tick — DON'T
    // spawn explosion flashes for those (they'd flash over the soldier/body carve).
    use crate::game::state::{PlasmaTorchState, TorchDir};
    let torch_active = state.torch_dir != 0;
    game.plasma_torch = match state.torch_dir {
        1 => Some(PlasmaTorchState { dir: TorchDir::UpForward,   fuel_ticks: 1 }),
        2 => Some(PlasmaTorchState { dir: TorchDir::Forward,     fuel_ticks: 1 }),
        3 => Some(PlasmaTorchState { dir: TorchDir::DownForward, fuel_ticks: 1 }),
        _ => None,
    };

    // `state.craters` is the full match history every tick; only apply the
    // tail we haven't seen yet. The expanding-ring flash (game.explosions) is
    // reconstructed here; the dirt/spark particles arrive via the fx_events
    // channel. Torch carving streams craters every tick — skip the flash there.
    let known = game.crater_log.len();
    for nc in state.craters.iter().skip(known) {
        use crate::world::{Crater, WorldPos};
        Crater::new(nc.cx, nc.cy, nc.radius).carve(&mut game.terrain);
        game.crater_log.push((nc.cx, nc.cy, nc.radius));
        if !torch_active {
            game.explosions.push(crate::game::state::Explosion::new(
                WorldPos::new(nc.cx, nc.cy), nc.radius,
            ));
        }
    }

    // Sync event messages (crate pickups etc.) — server is authoritative for
    // both content and remaining-tick countdown.
    game.messages = state.messages.iter().map(|m| crate::game::state::GameMessage {
        text: m.text.clone(),
        team: if m.team < 0 { None } else { Some(m.team as usize) },
        ticks: m.ticks,
    }).collect();

    // Sync headstones (server-authoritative; settled server-side). Rendering only
    // uses pos/team/headstone_id, so the other fields are placeholders.
    game.graves = state.graves.iter().map(|ng| crate::game::state::Grave {
        pos:          crate::world::WorldPos::new(ng.x, ng.y),
        team:         ng.team,
        soldier_idx:  0,
        died_tick:    0,
        vel_y:        0.0,
        settled:      true,
        headstone_id: ng.headstone_id,
    }).collect();

    // Sync blood splats (server-authoritative; decayed server-side).
    game.blood_splats = state.blood_splats.iter()
        .map(|nb| (crate::world::WorldPos::new(nb.x, nb.y), nb.ticks))
        .collect();

    // Sync barrels (positions + hp from server)
    {
        use crate::game::state::{Barrel, BarrelState};
        use crate::world::{Vec2, WorldPos};
        game.barrels = state.barrels.iter().map(|nb| Barrel {
            pos: WorldPos::new(nb.x, nb.y),
            vel: Vec2::new(0.0, 0.0),
            hp: nb.hp,
            state: BarrelState::Normal,
        }).collect();
    }

    // Sync black holes
    {
        use crate::game::state::BlackHole;
        use crate::world::WorldPos;
        game.black_holes = state.black_holes.iter().map(|nb| BlackHole {
            pos: WorldPos::new(nb.x, nb.y),
            lifetime: nb.ticks_left,
        }).collect();
    }

    // Sync fire patches
    {
        use crate::game::state::FirePatch;
        use crate::world::{Vec2, WorldPos};
        game.fire_patches = state.fire_patches.iter().map(|nf| FirePatch {
            pos: WorldPos::new(nf.x, nf.y),
            vel: Vec2::new(nf.vel_x, nf.vel_y),
            landed: nf.landed,
            lifetime: nf.lifetime,
        }).collect();
    }

    // Sync rope (rendering only — physics runs on server)
    {
        use crate::game::state::RopeState;
        use crate::world::WorldPos;
        game.rope = state.rope.as_ref().map(|nr| RopeState {
            anchor:   WorldPos::new(nr.anchor_x, nr.anchor_y),
            hook:     WorldPos::new(nr.hook_x,   nr.hook_y),
            flying:   nr.flying,
            length:   nr.length,
            hook_vel: crate::world::Vec2::new(0.0, 0.0),
        });
    }

    // Sync every team's display name and colour identity from the server
    // (supports 2-4 teams with player-picked colours in casual lobbies).
    for (i, team) in game.teams.iter_mut().enumerate() {
        if let Some(name) = state.team_names.get(i) {
            if !name.is_empty() { team.name = name.clone(); }
        }
        if let Some(&c) = state.team_colors.get(i) {
            team.color_id = c.min(3);
        }
    }

    // Sync Garcia (Hand of Jerry)
    {
        use crate::game::state::GarciaState;
        game.garcia = state.garcia.as_ref().map(|ng| GarciaState {
            cursor_x: ng.cursor_x, render_x: ng.render_x, cursor_y: ng.cursor_y, render_y: ng.render_y,
            blink_timer: ng.blink_timer,
            falling: ng.falling, fall_y: ng.fall_y, vel_y: ng.vel_y, bounce_count: ng.bounce_count,
        });
    }
    // Sync Airstrike
    {
        use crate::game::state::AirstrikeState;
        game.airstrike = state.airstrike.as_ref().map(|na| AirstrikeState {
            cursor_x: na.cursor_x, render_x: na.render_x, cursor_y: na.cursor_y, render_y: na.render_y,
            blink_timer: na.blink_timer, active: na.active,
            plane_x: na.plane_x, plane_vx: na.plane_vx,
            bombs_dropped: na.bombs_dropped, direction_right: na.direction_right,
        });
    }
    // Sync weapon inventories so ammo counts and selection stay accurate
    for (i, tw) in state.team_weapons.iter().enumerate() {
        if let Some(team) = game.teams.get_mut(i) {
            use crate::physics::projectile::WeaponKind;
            team.weapons = tw.weapons.iter()
                .map(|&(k, a)| (WeaponKind::from_net_u8(k), if a == 0xFFFF { None } else { Some(a) }))
                .collect();
            team.selected_weapon = tw.selected.min(team.weapons.len().saturating_sub(1));
        }
    }
}

// ── Compile-time parity checklists (never called) ─────────────────────────────
//
// These exhaustively destructure the wire-relevant structs with NO `..`, so
// adding a field breaks compilation here. That turns a silent live-mode desync
// into a build error and forces a decision at the one place that matters.

/// Adding a field to `GameState` breaks this. SYNC IT BY DEFAULT:
///   • Default (synced) → add to `StateMsg` (src/net/msg.rs), set it in
///     `build_state`, reconstruct it in `apply_server_state`, assert it in
///     `tests/parity.rs`, then list it in the "synced" group below.
///   • Opt out (not networked) → only if it's a client-only visual or
///     server-internal sim value. Put it in the second group AND leave a
///     `// not synced: <reason>` so the exemption is a deliberate, reviewed choice.
/// If you're unsure, sync it — an over-synced field is harmless; a missed one is
/// a silent live-mode desync.
#[allow(dead_code)]
fn _gamestate_parity_checklist(g: &GameState) {
    let GameState {
        // ── Synced to clients via StateMsg / build_state / apply_server_state ──
        teams: _, turn: _, projectiles: _, crates: _, mines: _, barrels: _,
        fire_patches: _, black_holes: _, wind: _, aim: _, result: _, tick: _,
        crater_log: _, sounds: _, fx_events: _, graves: _, weapon_menu_open: _,
        weapon_menu_cursor: _, rope: _, messages: _, blood_splats: _,
        plasma_torch: _, garcia: _, airstrike: _,
        // ── Not networked: client-only visuals / server-internal sim state ──
        // (terrain is rebuilt on the client from `crater_log`; `explosions` from craters)
        terrain: _, crate_timer: _, map_seed: _, is_test: _, is_multiplayer: _,
        scrap_earned: _, explosions: _, active_worm_hit: _, retreat_locked: _,
        damage_focus: _, server_fire_grace: _, shotgun_shots_left: _,
        revolver_shots_left: _, bullet_trails: _, rope_session: _,
        rope_used_this_turn: _, tnt_placed: _, crate_watch_ticks: _,
        smoke_particles: _, fx: _, pending_deaths: _,
    } = g;
}

/// Adding a field to `InputMsg` (client→server) breaks this. Handle it where the
/// server applies input (src/server/main.rs) before listing it here.
#[allow(dead_code)]
fn _inputmsg_parity_checklist(m: &InputMsg) {
    let InputMsg {
        tick: _, held: _, pressed: _, released: _, aim_angle: _,
        selected_weapon_kind: _, hat_ids: _, uniform_color_ids: _,
        boot_color_ids: _, gun_style_ids: _, worm_names: _,
        muzzle_x: _, muzzle_y: _, // not synced: client-side rendered position, travels in InputMsg
        quit: _, // not synced: server-only forfeit signal, consumed immediately by server
    } = m;
}
