//! Live-multiplayer parity guard.
//!
//! The live client never runs `simulate()` — it rebuilds state from `StateMsg`
//! via `net_sync::{build_state, apply_server_state}`. If a field changes in the
//! sim but isn't carried through that round-trip, live mode silently diverges.
//!
//! ## How the automation works
//!
//! `synced_snapshot()` exhaustively destructures `GameState` with no `..`. Adding
//! a field to `GameState` breaks that destructure until you either include the
//! field in the snapshot (if it should be synced) or exclude it with `_` and a
//! comment explaining why. The test then just calls `assert_eq!(snapshot(server),
//! snapshot(client))` — no manual assertions required.
//!
//! The two categories of excluded fields:
//!   • "synced to wire, managed locally" — sent in StateMsg but the live client
//!     applies them from its own input rather than from the server state
//!     (weapon_menu_open/cursor, aim.fuse_ticks, aim.angle on own turn, tick).
//!   • "not networked" — intentionally server-only or client-only state.

use arty::game::net_sync::{build_state, apply_server_state};
use arty::game::state::GameState;
use arty::game::team::{Team, Difficulty};
use arty::physics::projectile::{WeaponKind, Projectile, FuseState};
use arty::renderer::Camera;
use arty::world::{Terrain, WorldPos, Vec2, WORLD_W};

// ── Snapshot types ────────────────────────────────────────────────────────────
// All primitives so we get PartialEq + Debug for free.

#[derive(Debug, PartialEq)]
struct SoldierSnap {
    pos: (f32, f32),
    hp: u8,
    facing: i8,
    has_fired: bool,
    /// 0=Idle 1=Walking 2=Airborne(vx,vy) 3=Dead 4=Other
    state_disc: u8,
    airborne_vel: Option<(f32, f32)>,
    airtime: u32,
    walk_ticks: u32,
    /// Effective wire value: full hold while damage is still tallying.
    hp_countdown_delay: u32,
    on_fire_ticks_u8: u8,
    death_cause_u8: u8,
    kill_weapon_u8: u8,
}

#[derive(Debug, PartialEq)]
struct TeamSnap {
    active: usize,
    selected_weapon: usize,
    soldiers: Vec<SoldierSnap>,
    weapons: Vec<(u8, u32)>, // (kind_u8, ammo: 0xFFFF=infinite)
}

#[derive(Debug, PartialEq)]
struct ProjSnap {
    kind: u8,
    pos: (f32, f32),
    vel: (f32, f32),
    age_ticks: u32,
    fuse: u64, // encoded same way as NetProjectile: 0=None, 0xFFFFFFFE=Armed, 0x80000000|n=Detonating, n=Burning
    is_fragment: bool,
    homing_target: Option<(f32, f32)>,
}

#[derive(Debug, PartialEq)]
struct CrateSnap { pos: (f32, f32), landed: bool, kind_u8: u8 }

#[derive(Debug, PartialEq)]
struct MineSnap { pos: (f32, f32), state_u8: u8, arm_ticks: u32, trigger_ticks: u32 }

#[derive(Debug, PartialEq)]
struct BarrelSnap { pos: (f32, f32), hp: i32 }
// barrel.vel and barrel.state are NOT synced: apply_server_state always resets them to 0/Normal.

#[derive(Debug, PartialEq)]
struct BlackHoleSnap { pos: (f32, f32), lifetime: u32 }

#[derive(Debug, PartialEq)]
struct FirePatchSnap { pos: (f32, f32), vel: (f32, f32), landed: bool, lifetime: u32 }

#[derive(Debug, PartialEq)]
struct RopeSnap { anchor: (f32, f32), hook: (f32, f32), flying: bool, length: f32 }
// rope.hook_vel is NOT synced: always reset to Vec2::ZERO in apply_server_state.

#[derive(Debug, PartialEq)]
struct GarciaSn {
    cursor: (f32, f32), render: (f32, f32), blink_timer: u32,
    falling: bool, fall_y: f32, vel_y: f32, bounce_count: u32,
}

#[derive(Debug, PartialEq)]
struct AirstrikeSn {
    cursor: (f32, f32), render: (f32, f32), blink_timer: u32,
    active: bool, plane_x: f32, plane_vx: f32,
    bombs_dropped: u32, direction_right: bool,
}
// airstrike.spawn_cam_left is NOT synced: client-only, updated by tick() before plane launches.

#[derive(Debug, PartialEq)]
struct HomingMissileSn {
    cursor: (f32, f32), render: (f32, f32), blink_timer: u32, confirmed: bool,
}

#[derive(Debug, PartialEq)]
struct GraveSnap { pos: (f32, f32), team: usize, headstone_id: u8 }

#[derive(Debug, PartialEq)]
struct BloodSplatSnap { pos: (f32, f32), ticks: u32 }

#[derive(Debug, PartialEq)]
struct MessageSnap { text: String, team_i8: i8, ticks: u32 }

/// All synced fields of GameState in a flat, PartialEq-comparable form.
#[derive(Debug, PartialEq)]
struct SyncedSnapshot {
    // teams
    teams: Vec<TeamSnap>,
    // turn
    turn_team: usize,
    turn_number: u32,
    // wind
    wind: f32,
    // projectiles
    projectiles: Vec<ProjSnap>,
    // collections
    crates: Vec<CrateSnap>,
    mines: Vec<MineSnap>,
    barrels: Vec<BarrelSnap>,
    black_holes: Vec<BlackHoleSnap>,
    fire_patches: Vec<FirePatchSnap>,
    // terrain
    crater_log: Vec<(f32, f32, f32)>,
    // aim (only power; angle managed locally on own turn; fuse_ticks managed locally)
    aim_power: f32,
    // result (0=Ongoing, 1=Winner(t), 2=Draw — encode winner as high bits)
    result: u64,
    // special weapon sessions
    rope: Option<RopeSnap>,
    garcia: Option<GarciaSn>,
    airstrike: Option<AirstrikeSn>,
    homing_missile: Option<HomingMissileSn>,
    // plasma torch (0=inactive, 1-3=dir, fuel_ticks)
    torch_dir: u8,
    torch_fuel: u32,
    // cosmetic state (server-authoritative)
    graves: Vec<GraveSnap>,
    blood_splats: Vec<BloodSplatSnap>,
    messages: Vec<MessageSnap>,
}

/// Build a snapshot from `GameState`. This function exhaustively destructures
/// `GameState` with NO `..` — adding a field to `GameState` breaks compilation
/// here, forcing the developer to either include it in the snapshot or explicitly
/// exclude it with a `// not synced:` comment.
fn synced_snapshot(g: &GameState) -> SyncedSnapshot {
    use arty::game::state::{
        GameState, GameResult, MineState, CrateKind, TorchDir,
        AirstrikeState, GarciaState, HomingMissileState, PlasmaTorchState,
    };
    use arty::game::soldier::SoldierState;
    use arty::physics::projectile::FuseState;

    let GameState {
        // ── Included in snapshot (all must be round-tripped by apply_server_state) ──
        teams, turn, projectiles, crates, mines, barrels,
        fire_patches, black_holes, wind, aim, result, crater_log,
        graves, rope, messages, blood_splats, plasma_torch,
        garcia, airstrike, homing_missile,
        // ── Synced to wire but managed locally by the live client, not from server ──
        tick:                _, // game.tick increments locally on client; StateMsg.tick used for dedup
        sounds:              _, // per-tick event channel; tested in round_trip_preserves_synced_state
        fx_events:           _, // per-tick event channel; tested in round_trip_preserves_synced_state
        weapon_menu_open:    _, // client applies from own button input, not from server
        weapon_menu_cursor:  _, // same
        // ── Not networked (server-only sim state or client-only visuals) ──
        terrain:             _, // client rebuilds from crater_log
        crate_timer:         _, // server-internal drop timer
        map_seed:            _, // fixed at match start
        is_test:             _, // local mode flag
        is_multiplayer:      _, // local mode flag
        scrap_earned:        _, // server-authoritative; displayed at end of match
        explosions:          _, // client-local ring flash visuals
        active_worm_hit:     _, // local camera/retreat logic flag
        retreat_locked:      _, // local camera logic flag
        damage_focus:        _, // local camera focus helper
        server_fire_grace:   _, // server-only fire suppression counter
        pistol_shots_left:   _, // server-only burst state
        pistol_fire_timer:   _, // server-only burst timer
        shotgun_shots_left:  _, // server-only multi-shot state
        revolver_shots_left: _, // server-only multi-shot state
        minigun_shots_left:  _, // server-only burst state
        minigun_fire_timer:  _, // server-only burst timer
        uzi_shots_left:      _, // server-only burst state
        uzi_fire_timer:      _, // server-only burst timer
        bullet_trails:       _, // client-only visual trail
        rope_session:        _, // local acting-phase flag
        rope_used_this_turn: _, // local charge-consumption flag
        tnt_placed:          _, // local watching-phase flag
        crate_watch_ticks:   _, // local pre-turn crate-camera phase
        smoke_particles:     _, // client-only bazooka smoke
        fx:                  _, // client-only particle system
        fx_text:             _, // client-only damage-number popups
        pending_deaths:      _, // transient pre-explosion wait
        meteor_chain:        _, // transient per-tick flag
    } = g;

    // ── teams ─────────────────────────────────────────────────────────────────
    // Exhaustive destructures (no `..`): adding a field to Team/Soldier breaks
    // this test's build until the field is snapshotted or excluded with a
    // `// not synced:` comment — same forcing function as net_sync's checklists.
    let teams = teams.iter().map(|t| {
        let arty::game::team::Team {
            soldiers, active, selected_weapon, weapons,
            // ── Synced but fixed at game setup (both sides equal by construction) ──
            color_id: _, name: _,
            // ── Not networked ──
            slot: _,         // not synced: lobby seat
            avatar_id: _,    // not synced: lobby/roster cosmetic
            elo: _,          // not synced: server/API concern
            is_cpu: _,       // not synced: local-mode concept
            difficulty: _,   // not synced: local-mode CPU setting
            headstone_id: _, // not synced: graves carry it on the wire
        } = t;
        let soldiers = soldiers.iter().map(|s| {
            let arty::game::soldier::Soldier {
                pos, facing, hp, state, airtime, walk_ticks, has_fired,
                hp_countdown_delay, on_fire_ticks, death_cause, pending_damage,
                // ── Synced to wire but applied opponent-only on the client ──
                name: _, hat_id: _, uniform_color_id: _, boot_color_id: _, gun_style_id: _,
                // ── Not networked ──
                team: _, index: _,  // structural identity, fixed at game setup
                fall: _,            // not synced: server-side fall-damage tracker
                has_moved: _,       // not synced: server-internal turn bookkeeping
                has_grave: _,       // not synced: graves ship separately
                death_explosion_pending: _, // not synced: server-side death timer
                hp_display_ticks: _,        // not synced: client-local visibility timer
                displayed_hp: _,    // not synced: client-local animation toward hp
                damage_settle: _,   // not synced: server-side tally window
                kill_weapon,        // → kill_weapon_u8 (255 = None)
            } = s;
            let (state_disc, airborne_vel) = match state {
                SoldierState::Idle           => (0, None),
                SoldierState::Walking {..}   => (1, None),
                SoldierState::Airborne { vel, .. } => (2, Some((vel.x, vel.y))),
                SoldierState::Dead           => (3, None),
                _                            => (4, None),
            };
            SoldierSnap {
                pos: (pos.x, pos.y), hp: *hp, facing: *facing,
                has_fired: *has_fired, state_disc, airborne_vel,
                airtime: *airtime, walk_ticks: *walk_ticks,
                // Mirror build_state's wire transform (full hold while tallying).
                hp_countdown_delay: if *pending_damage > 0 {
                    arty::game::soldier::HP_COUNTDOWN_DELAY_TICKS
                } else { *hp_countdown_delay },
                on_fire_ticks_u8: (*on_fire_ticks).min(255) as u8,
                death_cause_u8: {
                    use arty::game::soldier::DeathCause;
                    match death_cause {
                        DeathCause::Generic => 0, DeathCause::Explosion => 1,
                        DeathCause::Fall => 2, DeathCause::Water => 3,
                    }
                },
                kill_weapon_u8: kill_weapon.map(|w| w.to_net_u8()).unwrap_or(255),
            }
        }).collect();
        let weapons = weapons.iter()
            .map(|&(k, a)| (k.to_net_u8(), a.unwrap_or(0xFFFF)))
            .collect();
        TeamSnap { active: *active, selected_weapon: *selected_weapon, soldiers, weapons }
    }).collect();

    // ── projectiles ───────────────────────────────────────────────────────────
    let projectiles = projectiles.iter().map(|p| {
        let arty::physics::projectile::Projectile {
            pos, vel, kind, age_ticks, fuse, is_fragment, homing_target,
            owner: _,         // not synced: server-side self-hit grace
            cleared_owner: _, // not synced: server-side self-hit grace
        } = p;
        let fuse = match fuse {
            FuseState::None           => 0u64,
            FuseState::Burning(n)     => *n as u64,
            FuseState::Expired        => 0xFFFF_FFFFu64,
            FuseState::Armed          => 0xFFFF_FFFEu64,
            FuseState::Detonating(n)  => 0x8000_0000u64 | *n as u64,
        };
        ProjSnap {
            kind: kind.to_net_u8(),
            pos: (pos.x, pos.y), vel: (vel.x, vel.y),
            age_ticks: *age_ticks, fuse, is_fragment: *is_fragment,
            homing_target: *homing_target,
        }
    }).collect();

    // ── crates ────────────────────────────────────────────────────────────────
    let crates = crates.iter().map(|c| {
        let arty::game::state::DroppedCrate {
            pos, kind, landed,
            descent_vy: _,       // not synced: server-side physics
            damage_this_turn: _, // not synced: server-side destruction tally
            fall_ticks: _,       // not synced: server-side descent bookkeeping
        } = c;
        let kind_u8 = match kind {
            CrateKind::Health    => 0,
            CrateKind::Weapon(_) => 1,
            CrateKind::Scrap(_)  => 2,
        };
        CrateSnap { pos: (pos.x, pos.y), landed: *landed, kind_u8 }
    }).collect();

    // ── mines ─────────────────────────────────────────────────────────────────
    let mines = mines.iter().map(|m| {
        let arty::game::state::PlacedMine { pos, state, arm_ticks, trigger_ticks } = m;
        let state_u8 = match state { MineState::Arming => 0, MineState::Armed => 1, MineState::Triggered => 2 };
        MineSnap { pos: (pos.x, pos.y), state_u8, arm_ticks: *arm_ticks, trigger_ticks: *trigger_ticks }
    }).collect();

    // ── barrels ───────────────────────────────────────────────────────────────
    let barrels = barrels.iter().map(|b| {
        let arty::game::state::Barrel {
            pos, hp,
            vel: _,   // not synced: apply_server_state resets to 0
            state: _, // not synced: apply_server_state resets to Normal
        } = b;
        BarrelSnap { pos: (pos.x, pos.y), hp: *hp }
    }).collect();

    // ── black holes ───────────────────────────────────────────────────────────
    let black_holes = black_holes.iter().map(|h| {
        let arty::game::state::BlackHole { pos, lifetime } = h;
        BlackHoleSnap { pos: (pos.x, pos.y), lifetime: *lifetime }
    }).collect();

    // ── fire patches ──────────────────────────────────────────────────────────
    let fire_patches = fire_patches.iter().map(|f| {
        let arty::game::state::FirePatch { pos, vel, landed, lifetime } = f;
        FirePatchSnap {
            pos: (pos.x, pos.y), vel: (vel.x, vel.y), landed: *landed, lifetime: *lifetime,
        }
    }).collect();

    // ── aim (power only; see field comments) ──────────────────────────────────
    let aim_power = aim.power;

    // ── result ────────────────────────────────────────────────────────────────
    let result = match result {
        GameResult::Ongoing    => 0u64,
        GameResult::Winner(t)  => 0x1_0000_0000u64 | *t as u64,
        GameResult::Draw       => 0x2_0000_0000u64,
    };

    // ── rope ──────────────────────────────────────────────────────────────────
    let rope = rope.as_ref().map(|r| {
        let arty::game::state::RopeState {
            anchor, hook, flying, length,
            hook_vel: _, // not synced: apply_server_state resets to zero
        } = r;
        RopeSnap {
            anchor: (anchor.x, anchor.y), hook: (hook.x, hook.y),
            flying: *flying, length: *length,
        }
    });

    // ── garcia ────────────────────────────────────────────────────────────────
    let garcia = garcia.as_ref().map(|g| {
        let GarciaState { cursor_x, cursor_y, render_x, render_y, blink_timer,
                          falling, fall_y, vel_y, bounce_count } = g;
        GarciaSn {
            cursor: (*cursor_x, *cursor_y), render: (*render_x, *render_y),
            blink_timer: *blink_timer, falling: *falling,
            fall_y: *fall_y, vel_y: *vel_y, bounce_count: *bounce_count,
        }
    });

    // ── airstrike ─────────────────────────────────────────────────────────────
    let airstrike = airstrike.as_ref().map(|a| {
        let AirstrikeState {
            cursor_x, cursor_y, render_x, render_y, blink_timer, active,
            plane_x, plane_vx, bombs_dropped, direction_right,
            spawn_cam_left: _, // not synced: client-only camera bookkeeping
        } = a;
        AirstrikeSn {
            cursor: (*cursor_x, *cursor_y), render: (*render_x, *render_y),
            blink_timer: *blink_timer, active: *active,
            plane_x: *plane_x, plane_vx: *plane_vx,
            bombs_dropped: *bombs_dropped, direction_right: *direction_right,
        }
    });

    // ── homing missile ────────────────────────────────────────────────────────
    let homing_missile = homing_missile.as_ref().map(|h| {
        let HomingMissileState { cursor_x, cursor_y, render_x, render_y,
                                 blink_timer, confirmed } = h;
        HomingMissileSn {
            cursor: (*cursor_x, *cursor_y), render: (*render_x, *render_y),
            blink_timer: *blink_timer, confirmed: *confirmed,
        }
    });

    // ── plasma torch ──────────────────────────────────────────────────────────
    let (torch_dir, torch_fuel) = plasma_torch.as_ref().map(|t| {
        let PlasmaTorchState { dir, fuel_ticks } = t;
        let d = match dir { TorchDir::UpForward => 1, TorchDir::Forward => 2, TorchDir::DownForward => 3 };
        (d, *fuel_ticks)
    }).unwrap_or((0, 0));

    // ── graves ────────────────────────────────────────────────────────────────
    let graves = graves.iter().map(|g| {
        let arty::game::state::Grave {
            pos, team, headstone_id,
            soldier_idx: _, // not synced: server-side dedup key
            died_tick: _,   // not synced: server-side bookkeeping
            vel_y: _,       // not synced: graves ship pre-settled
            settled: _,     // not synced: graves ship pre-settled
        } = g;
        GraveSnap { pos: (pos.x, pos.y), team: *team, headstone_id: *headstone_id }
    }).collect();

    // ── blood splats ──────────────────────────────────────────────────────────
    let blood_splats = blood_splats.iter().map(|(p, t)| BloodSplatSnap { pos: (p.x, p.y), ticks: *t }).collect();

    // ── messages ──────────────────────────────────────────────────────────────
    let messages = messages.iter().map(|m| {
        let arty::game::state::GameMessage { text, team, ticks } = m;
        MessageSnap {
            text: text.clone(),
            team_i8: team.map(|t| t as i8).unwrap_or(-1),
            ticks: *ticks,
        }
    }).collect();

    SyncedSnapshot {
        teams, turn_team: turn.current_team, turn_number: turn.turn_number, wind: wind.value(),
        projectiles, crates, mines, barrels, black_holes, fire_patches,
        crater_log: crater_log.clone(), aim_power, result, rope, garcia, airstrike,
        homing_missile, torch_dir, torch_fuel, graves, blood_splats, messages,
    }
}

// ── Test helpers ──────────────────────────────────────────────────────────────

fn build_game(seed: u64) -> GameState {
    let mut terrain = Terrain::generate_tactical(seed);
    let all = terrain.find_team_spawns(0, WORLD_W, 8);
    let t0: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 0).map(|(_, s)| s).collect();
    let t1: Vec<_> = all.iter().cloned().enumerate().filter(|(i, _)| i % 2 == 1).map(|(_, s)| s).collect();
    let teams = vec![
        Team::new(0, false, Difficulty::Medium, &t0),
        Team::new(1, false, Difficulty::Medium, &t1),
    ];
    GameState::new(seed, terrain, teams, 2)
}

fn round_trip(server: &GameState, tick: u32, my_team: usize) -> GameState {
    let mut client = build_game(server.map_seed);
    let mut cam = Camera::new(0.0, 0.0);
    let state = build_state(server, tick, my_team);
    apply_server_state(&mut client, &mut cam, &state, my_team);
    client
}

// ── Core parity tests ─────────────────────────────────────────────────────────

/// The main gap-catcher: perturb server state, round-trip through the netcode,
/// and assert every synced field was preserved. `synced_snapshot()` is the source
/// of truth — it destructures `GameState` exhaustively, so future field additions
/// that aren't handled here will break compilation, not silently diverge.
#[test]
fn round_trip_preserves_synced_state() {
    let mut server = build_game(1234);

    // Perturb every synced field to a non-default value so a missing or broken
    // apply_server_state line can't hide behind a coincidental zero match.
    use arty::game::state::{
        DroppedCrate, CrateKind, Barrel, BarrelState, BlackHole,
        FirePatch, RopeState, PlasmaTorchState, TorchDir,
        GarciaState, AirstrikeState, HomingMissileState,
        GameMessage, Grave,
    };

    // Soldiers
    server.teams[0].soldiers[0].pos.x += 17.0;
    server.teams[0].soldiers[0].facing = -1;
    server.teams[1].soldiers[0].hp = 55;
    // Make a soldier airborne so the vel fields are exercised
    server.teams[0].soldiers[1].state = arty::game::soldier::SoldierState::Airborne {
        vel: Vec2::new(3.5, -4.2), spinning: false,
    };

    // Turn
    server.turn.current_team = 1;
    server.turn.turn_number = 8;

    // Wind
    server.wind = arty::physics::Wind::new(0.42);

    // Aim power
    server.aim.power = 0.75;

    // Projectile with non-default age_ticks and homing_target
    {
        let mut p = Projectile::new(WorldPos::new(800.0, 200.0), Vec2::new(5.0, -3.0), WeaponKind::HomingMissile);
        p.age_ticks = 45;
        p.homing_target = Some((900.0, 180.0));
        server.projectiles.push(p);
    }

    // Crate (landed weapon crate)
    server.crates.push(DroppedCrate {
        pos: WorldPos::new(300.0, 250.0),
        kind: CrateKind::Weapon(WeaponKind::Grenade),
        landed: true,
        descent_vy: 1.5, damage_this_turn: 0, fall_ticks: 0,
    });

    // Barrel with non-default hp
    server.barrels.push(Barrel {
        pos: WorldPos::new(450.0, 280.0),
        vel: Vec2::new(0.0, 0.0),
        hp: 50,
        state: BarrelState::Normal,
    });

    // Black hole
    server.black_holes.push(BlackHole {
        pos: WorldPos::new(600.0, 200.0),
        lifetime: 120,
    });

    // Fire patch (airborne)
    server.fire_patches.push(FirePatch {
        pos: WorldPos::new(350.0, 180.0),
        vel: Vec2::new(2.0, -1.5),
        landed: false,
        lifetime: 150,
    });

    // Rope (flying hook)
    server.rope = Some(RopeState {
        anchor:   WorldPos::new(500.0, 100.0),
        hook:     WorldPos::new(520.0, 80.0),
        flying:   true,
        length:   60.0,
        hook_vel: Vec2::new(1.0, -2.0),
    });

    // Plasma torch
    server.plasma_torch = Some(PlasmaTorchState {
        dir: TorchDir::Forward,
        fuel_ticks: 90,
    });

    // Garcia (targeting mode)
    server.garcia = Some(GarciaState {
        cursor_x: 400.0, cursor_y: 300.0, render_x: 398.0, render_y: 301.0,
        blink_timer: 5, falling: false, fall_y: 0.0, vel_y: 0.0, bounce_count: 0,
    });

    // AirStrike (active, plane in flight)
    server.airstrike = Some(AirstrikeState {
        cursor_x: 700.0, cursor_y: 100.0, render_x: 699.0, render_y: 101.0,
        blink_timer: 3, active: true, plane_x: 200.0, plane_vx: 4.0,
        bombs_dropped: 1, direction_right: true, spawn_cam_left: 0.0,
    });

    // Homing missile cursor
    server.homing_missile = Some(HomingMissileState {
        cursor_x: 850.0, cursor_y: 200.0, render_x: 848.5, render_y: 201.3, blink_timer: 7,
        confirmed: true,
    });

    // Grave
    server.graves.push(Grave {
        pos: WorldPos::new(250.0, 310.0),
        team: 1, soldier_idx: 0, died_tick: 0, vel_y: 0.0, settled: true,
        headstone_id: 2,
    });

    // Blood splat
    server.blood_splats.push((WorldPos::new(480.0, 290.0), 60));

    // Message
    server.messages.push(GameMessage {
        text: "Test message".to_string(),
        team: Some(0),
        ticks: 80,
    });

    // Explosion for crater_log + fx_events + sounds
    let blast = server.teams[1].soldiers[0].pos;
    server.apply_explosion(blast, WeaponKind::Bazooka);
    server.emit_sound(arty::audio::Sfx::Explosion);

    assert!(!server.crater_log.is_empty(), "explosion should carve a crater");
    assert!(!server.fx_events.is_empty(), "explosion should emit fx_events");

    // Use my_team=1 so aim.angle IS applied (state.turn_team=1 == my_team? No,
    // turn_team=1 and my_team=1 → same → angle not applied on own turn).
    // Use my_team=0 so turn_team(1) != my_team(0) → aim.angle is applied.
    let state = build_state(&server, 7, 0);
    let client = round_trip(&server, 7, 0);

    assert_eq!(synced_snapshot(&server), synced_snapshot(&client));

    // Event channels are pass-through (not in snapshot since they're per-tick, not state)
    assert_eq!(state.sounds,    server.sounds,    "sounds channel dropped by build_state");
    assert_eq!(state.fx_events, server.fx_events, "fx_events channel dropped by build_state");
}

/// All major projectile kinds, each with distinct field values, must survive.
/// Covers: basic, fuse states (Burning/Armed/Detonating), fragment flag,
/// age_ticks, and homing_target.
#[test]
fn all_projectile_kinds_survive_round_trip() {
    let mut server = build_game(42);
    let pos = WorldPos::new(500.0, 100.0);

    server.projectiles.push(Projectile::new(pos, Vec2::new(4.0, -2.0), WeaponKind::Bazooka));
    {
        let mut p = Projectile::new(pos, Vec2::new(3.0, -1.5), WeaponKind::Grenade);
        p.fuse = FuseState::Burning(90);
        server.projectiles.push(p);
    }
    {
        let mut p = Projectile::new(pos, Vec2::new(5.0, -4.0), WeaponKind::BananaBomb);
        p.fuse = FuseState::Burning(60);
        p.is_fragment = true;
        server.projectiles.push(p);
    }
    {
        let mut p = Projectile::new(pos, Vec2::new(6.0, -1.0), WeaponKind::HomingMissile);
        p.age_ticks = 55;
        p.homing_target = Some((700.0, 150.0));
        server.projectiles.push(p);
    }
    { let mut p = Projectile::new(pos, Vec2::new(2.0, -3.0), WeaponKind::HolyHandGrenade); p.fuse = FuseState::Armed; server.projectiles.push(p); }
    { let mut p = Projectile::new(pos, Vec2::new(0.0,  0.0), WeaponKind::HolyHandGrenade); p.fuse = FuseState::Detonating(8); server.projectiles.push(p); }
    { let mut p = Projectile::new(pos, Vec2::new(0.0,  0.0), WeaponKind::Tnt); p.fuse = FuseState::Burning(45); server.projectiles.push(p); }
    server.projectiles.push(Projectile::new(pos, Vec2::new(3.5, -2.0), WeaponKind::BlackHoleBomb));
    server.projectiles.push(Projectile::new(pos, Vec2::new(4.0, -1.0), WeaponKind::Blasthive));

    let client = round_trip(&server, 1, 0);
    assert_eq!(synced_snapshot(&server), synced_snapshot(&client));
}

/// Garcia targeting (not yet confirmed) and falling (in flight) states both survive.
#[test]
fn garcia_state_parity() {
    let mut server = build_game(7);
    server.garcia = Some(arty::game::state::GarciaState {
        cursor_x: 400.0, cursor_y: 300.0, render_x: 399.0, render_y: 299.5,
        blink_timer: 12, falling: false, fall_y: 0.0, vel_y: 0.0, bounce_count: 0,
    });
    assert_eq!(synced_snapshot(&server), synced_snapshot(&round_trip(&server, 1, 0)));

    let g = server.garcia.as_mut().unwrap();
    g.falling = true; g.fall_y = -120.5; g.vel_y = 9.2; g.bounce_count = 1;
    assert_eq!(synced_snapshot(&server), synced_snapshot(&round_trip(&server, 2, 0)));
}

/// AirStrike cursor and active-plane (bombs dropping) both survive.
#[test]
fn airstrike_state_parity() {
    let mut server = build_game(8);
    server.airstrike = Some(arty::game::state::AirstrikeState {
        cursor_x: 600.0, cursor_y: 100.0, render_x: 598.0, render_y: 102.0,
        blink_timer: 4, active: false, plane_x: 0.0, plane_vx: 0.0,
        bombs_dropped: 0, direction_right: true, spawn_cam_left: 0.0,
    });
    assert_eq!(synced_snapshot(&server), synced_snapshot(&round_trip(&server, 1, 0)));

    let a = server.airstrike.as_mut().unwrap();
    a.active = true; a.plane_x = 250.0; a.plane_vx = 4.5; a.bombs_dropped = 2; a.direction_right = false;
    assert_eq!(synced_snapshot(&server), synced_snapshot(&round_trip(&server, 2, 0)));
}

/// Mines in all three states (Arming → Armed → Triggered) survive with correct
/// state discriminant and countdown values.
#[test]
fn mines_in_all_states_survive_round_trip() {
    use arty::game::state::{PlacedMine, MineState};
    let mut server = build_game(99);
    server.mines.push(PlacedMine { pos: WorldPos::new(200.0, 300.0), state: MineState::Arming,    arm_ticks: 72, trigger_ticks: 0  });
    server.mines.push(PlacedMine { pos: WorldPos::new(400.0, 310.0), state: MineState::Armed,     arm_ticks: 0,  trigger_ticks: 0  });
    server.mines.push(PlacedMine { pos: WorldPos::new(600.0, 290.0), state: MineState::Triggered, arm_ticks: 0,  trigger_ticks: 11 });
    assert_eq!(synced_snapshot(&server), synced_snapshot(&round_trip(&server, 1, 0)));
}

/// Fire patches carry their velocity and landed flag through the round-trip.
/// Missing vel means an airborne patch lands instantly; missing landed makes a
/// settled patch fly away.
#[test]
fn fire_patches_survive_round_trip() {
    use arty::game::state::FirePatch;
    let mut server = build_game(55);
    server.fire_patches.push(FirePatch { pos: WorldPos::new(300.0, 150.0), vel: Vec2::new(3.5, -2.0), landed: false, lifetime: 180 });
    server.fire_patches.push(FirePatch { pos: WorldPos::new(500.0, 320.0), vel: Vec2::new(0.0,  0.0), landed: true,  lifetime: 90  });
    assert_eq!(synced_snapshot(&server), synced_snapshot(&round_trip(&server, 1, 0)));
}

/// turn_number must survive: it drives weapon unlock checks (TNT at turn 5,
/// AirStrike at turn 7, HomingMissile at turn 2×teams). Before this fix the live
/// client always saw turn_number=0 and all timed weapons were locked forever.
#[test]
fn turn_number_survives_round_trip() {
    let mut server = build_game(11);
    server.turn.turn_number = 14;
    assert_eq!(synced_snapshot(&server), synced_snapshot(&round_trip(&server, 1, 0)));
}

/// Run the server sim for 50 ticks with a homing missile and a live grenade in
/// flight; round-trip on every tick and assert the snapshots stay identical.
/// Catches drift bugs where a field accumulates differently after tick 0
/// (e.g. age_ticks incremented twice per tick on the live client).
#[test]
fn multi_tick_parity_stays_in_sync() {
    use arty::game::loop_runner::server_tick;
    use arty::input::InputState;

    let mut server = build_game(77);
    {
        let mut m = Projectile::new(WorldPos::new(300.0, 200.0), Vec2::new(5.0, -2.0), WeaponKind::HomingMissile);
        m.homing_target = Some((600.0, 150.0));
        server.projectiles.push(m);
    }
    {
        let mut g = Projectile::new(WorldPos::new(300.0, 200.0), Vec2::new(-2.0, -4.0), WeaponKind::Grenade);
        g.fuse = FuseState::Burning(60);
        server.projectiles.push(g);
    }

    // Reuse a single client to avoid the 50× terrain-generation cost of round_trip().
    let mut client = build_game(server.map_seed);
    let mut cam = Camera::new(0.0, 0.0);
    let input = InputState::new();
    for tick in 0..50u32 {
        server_tick(&mut server, &input, None, None);
        let state = build_state(&server, tick, 0);
        apply_server_state(&mut client, &mut cam, &state, 0);
        assert_eq!(
            synced_snapshot(&server), synced_snapshot(&client),
            "snapshot diverged at tick {tick}"
        );
    }
}

// ── Wire-encoding tests ───────────────────────────────────────────────────────

/// Every Sfx variant must survive the u8 wire encoding round-trip intact.
#[test]
fn sfx_net_roundtrip() {
    use arty::audio::Sfx;
    let all: &[Sfx] = &[
        Sfx::Explosion, Sfx::Tnt, Sfx::Grenade, Sfx::Meteor,
        Sfx::BlackHole, Sfx::Mine, Sfx::MineArm, Sfx::Barrel,
        Sfx::Revolver, Sfx::Shotgun, Sfx::Bat, Sfx::CrateDrop,
        Sfx::PlasmaTorch, Sfx::Garcia, Sfx::Smash, Sfx::Death,
        Sfx::DeathWater, Sfx::HolyHandGrenade,
    ];
    for &sfx in all {
        let decoded = Sfx::from_u8(sfx as u8);
        assert_eq!(decoded, Some(sfx), "{sfx:?} did not survive u8 round-trip");
    }
}

/// HHG Armed and Detonating fuse states survive the net encoding.
#[test]
fn hhg_fuse_state_net_roundtrip() {
    let mut server = build_game(42);
    let pos = WorldPos::new(500.0, 100.0);
    let mut p1 = Projectile::new(pos, Vec2::new(0.0, 0.0), WeaponKind::HolyHandGrenade); p1.fuse = FuseState::Armed;
    let mut p2 = Projectile::new(pos, Vec2::new(0.0, 0.0), WeaponKind::HolyHandGrenade); p2.fuse = FuseState::Detonating(5);
    server.projectiles.extend([p1, p2]);

    let client = round_trip(&server, 1, 0);
    assert_eq!(client.projectiles.len(), 2);
    assert_eq!(client.projectiles[0].fuse, FuseState::Armed);
    assert_eq!(client.projectiles[1].fuse, FuseState::Detonating(5));
}

/// Muzzle fields on InputMsg must survive the bincode wire round-trip.
#[test]
fn inputmsg_muzzle_roundtrip() {
    use arty::net::msg::InputMsg;
    use arty::net::encode;
    let msg = InputMsg {
        tick: 1, held: vec![], pressed: vec![], released: vec![],
        aim_angle: 0.5, selected_weapon_kind: 0,
        hat_ids: [0; 4], uniform_color_ids: [0; 4],
        boot_color_ids: [0; 4], gun_style_ids: [0; 4],
        worm_names: Default::default(),
        muzzle_x: 123.45, muzzle_y: 67.89,
        quit: false,
    };
    let bytes = encode(&msg).expect("encode failed");
    let decoded: InputMsg = bincode::deserialize(&bytes[4..]).expect("decode failed");
    assert_eq!(decoded.muzzle_x, 123.45);
    assert_eq!(decoded.muzzle_y, 67.89);
}

// ── Sim correctness tests ─────────────────────────────────────────────────────

/// TAT replay must call process_weapon_menu before server_tick so weapon switches
/// are applied — regression guard for the TAT code paths.
#[test]
fn tat_replay_applies_weapon_switch() {
    use arty::game::loop_runner::replay_tick;

    let mut game = build_game(42);
    game.teams[0].weapons = vec![(WeaponKind::Bazooka, None), (WeaponKind::Grenade, None)];
    game.teams[0].selected_weapon = 0;

    const SELECT: u16 = 1 << 13;
    const RIGHT:  u16 = 1 << 3;
    const A_BTN:  u16 = 1 << 4;

    let bitmasks: Vec<u16> = [SELECT, RIGHT, A_BTN]
        .into_iter()
        .chain(std::iter::repeat(0u16).take(30))
        .chain([A_BTN, 0])
        .collect();

    let mut prev = 0u16;
    for &bits in &bitmasks {
        replay_tick(&mut game, prev, bits);
        prev = bits;
    }

    assert!(!game.projectiles.is_empty(), "TAT replay: expected a projectile");
    assert_eq!(game.projectiles[0].kind, WeaponKind::Grenade,
        "TAT replay fired {:?} instead of Grenade", game.projectiles[0].kind);
}

// ── All-modes cursor parity ───────────────────────────────────────────────────
// These tests verify that cursor-phase weapons respond to Up/Down across all
// three execution paths: tick() (hotseat), server_tick() (live server), and
// replay_tick() (TAT replay). Regression guard for the server/main.rs
// clear_button(Up)/clear_button(Down) stripping that broke cursor Y in live mode.
//
// When adding a new cursor-phase weapon that uses Up/Down, add a test here.

/// Homing missile cursor must move vertically when Up/Down are held, in all paths.
#[test]
fn homing_missile_cursor_y_all_paths() {
    use arty::game::loop_runner::{simulate_with_muzzle, server_tick, replay_tick};
    use arty::game::state::HomingMissileState;
    use arty::input::{InputState, Button};

    fn game_with_hm(seed: u64) -> GameState {
        let mut g = build_game(seed);
        g.homing_missile = Some(HomingMissileState {
            cursor_x: 500.0, cursor_y: 250.0,
            render_x: 500.0, render_y: 250.0,
            blink_timer: 0, confirmed: false,
        });
        g
    }

    let initial_y = 250.0f32;
    const TICKS: u32 = 10;
    const UP_BIT: u16 = 1 << 0; // Button::Up is index 0 in Button::ALL

    // Path 1: simulate_with_muzzle() (hotseat/reference path)
    let mut g1 = game_with_hm(42);
    let mut inp = InputState::new(); inp.inject_press(Button::Up);
    for _ in 0..TICKS { simulate_with_muzzle(&mut g1, &inp, None, None); }
    let y1 = g1.homing_missile.as_ref().unwrap().cursor_y;
    assert!(y1 < initial_y, "simulate: Up should decrease cursor_y (was {initial_y}, got {y1})");

    // Path 2: server_tick() (live server) — was broken: Up was stripped unconditionally
    let mut g2 = game_with_hm(42);
    let mut srv = InputState::new(); srv.inject_press(Button::Up);
    for _ in 0..TICKS { server_tick(&mut g2, &srv, None, None); }
    let y2 = g2.homing_missile.as_ref().unwrap().cursor_y;
    assert!(y2 < initial_y, "server_tick: Up should decrease cursor_y (was {initial_y}, got {y2})");
    assert_eq!(y1, y2, "server_tick cursor_y must match simulate");

    // Path 3: replay_tick() (TAT replay)
    let mut g3 = game_with_hm(42);
    let mut prev = 0u16;
    for _ in 0..TICKS { replay_tick(&mut g3, prev, UP_BIT); prev = UP_BIT; }
    let y3 = g3.homing_missile.as_ref().unwrap().cursor_y;
    assert_eq!(y1, y3, "replay_tick cursor_y must match simulate");
}

/// Airstrike cursor must move vertically when Up/Down are held, in all paths.
#[test]
fn airstrike_cursor_y_all_paths() {
    use arty::game::loop_runner::{simulate_with_muzzle, server_tick, replay_tick};
    use arty::game::state::AirstrikeState;
    use arty::input::{InputState, Button};

    fn game_with_air(seed: u64) -> GameState {
        let mut g = build_game(seed);
        g.airstrike = Some(AirstrikeState {
            cursor_x: 500.0, cursor_y: 250.0,
            render_x: 500.0, render_y: 250.0,
            blink_timer: 0, active: false, // active=false → cursor phase
            plane_x: 0.0, plane_vx: 0.0,
            bombs_dropped: 0, direction_right: true, spawn_cam_left: 0.0,
        });
        g
    }

    let initial_y = 250.0f32;
    const TICKS: u32 = 10;
    const UP_BIT: u16 = 1 << 0;

    let mut g1 = game_with_air(42);
    let mut inp = InputState::new(); inp.inject_press(Button::Up);
    for _ in 0..TICKS { simulate_with_muzzle(&mut g1, &inp, None, None); }
    let y1 = g1.airstrike.as_ref().unwrap().cursor_y;
    assert!(y1 < initial_y, "simulate: Up should decrease airstrike cursor_y (got {y1})");

    let mut g2 = game_with_air(42);
    let mut srv = InputState::new(); srv.inject_press(Button::Up);
    for _ in 0..TICKS { server_tick(&mut g2, &srv, None, None); }
    let y2 = g2.airstrike.as_ref().unwrap().cursor_y;
    assert!(y2 < initial_y, "server_tick: Up should decrease airstrike cursor_y (got {y2})");
    assert_eq!(y1, y2, "server_tick airstrike cursor_y must match simulate");

    let mut g3 = game_with_air(42);
    let mut prev = 0u16;
    for _ in 0..TICKS { replay_tick(&mut g3, prev, UP_BIT); prev = UP_BIT; }
    let y3 = g3.airstrike.as_ref().unwrap().cursor_y;
    assert_eq!(y1, y3, "replay_tick airstrike cursor_y must match simulate");
}

// ── All-paths simulation parity ───────────────────────────────────────────────
//
// `assert_all_paths_in_sync` runs the same input sequence through every
// testable simulation path and asserts `synced_snapshot` matches across all.
//
// Use this in every new gameplay feature test. It catches bugs like the
// server/main.rs Up/Down stripping issue: a missing input-preprocessing step
// in one path produces a divergent synced_snapshot and fails immediately.
//
// Paths covered:
//   1. simulate_with_muzzle — hotseat/CPU reference
//   2. server_tick          — live server (library level, excludes server/main.rs preprocessing)
//   3. replay_tick          — TAT visual replay + fast-forward
//   4. StateMsg round-trip  — live client (build_state → apply_server_state)
//
// `steps` is &[(input_bits: u16, tick_count: usize)]. Each step holds `input_bits`
// for `tick_count` ticks; bits map to Button::ALL indices (0=Up, 1=Down, 4=A, …).

fn assert_all_paths_in_sync(
    seed: u64,
    setup: impl Fn(&mut GameState),
    steps: &[(u16, usize)],
) {
    use arty::game::loop_runner::{simulate_with_muzzle, server_tick, replay_tick};
    use arty::input::InputState;

    let mut g_sim = { let mut g = build_game(seed); setup(&mut g); g };
    let mut g_srv = { let mut g = build_game(seed); setup(&mut g); g };
    let mut g_tat = { let mut g = build_game(seed); setup(&mut g); g };

    for &(bits, n) in steps {
        let input = InputState::from_bits(0, bits);
        for _ in 0..n {
            simulate_with_muzzle(&mut g_sim, &input, None, None);
            server_tick(&mut g_srv, &input, None, None);
        }
        let mut prev = 0u16;
        for _ in 0..n {
            replay_tick(&mut g_tat, prev, bits);
            prev = bits;
        }
    }

    assert_eq!(
        synced_snapshot(&g_sim), synced_snapshot(&g_srv),
        "simulate vs server_tick diverged",
    );
    assert_eq!(
        synced_snapshot(&g_sim), synced_snapshot(&g_tat),
        "simulate vs replay_tick (TAT) diverged",
    );
    let total_ticks = steps.iter().map(|(_, n)| n).sum::<usize>() as u32;
    let client = round_trip(&g_sim, total_ticks, 0);
    assert_eq!(
        synced_snapshot(&g_sim), synced_snapshot(&client),
        "simulate vs live client (StateMsg round-trip) diverged",
    );
}

// Bit indices matching Button::ALL order: Up=0, Down=1, Left=2, Right=3, A=4
const UP_BITS:   u16 = 1 << 0;
const DOWN_BITS: u16 = 1 << 1;
const A_BITS:    u16 = 1 << 4;

/// Fire a bazooka and let it fly — covers basic projectile physics in all paths.
#[test]
fn weapon_sim_parity_bazooka() {
    assert_all_paths_in_sync(42, |_| {}, &[(A_BITS, 1), (0, 30)]);
}

/// Grenade: hold A to charge, release, let it bounce and detonate.
#[test]
fn weapon_sim_parity_grenade() {
    use arty::physics::projectile::WeaponKind;
    assert_all_paths_in_sync(43, |g| {
        g.teams[0].weapons = vec![(WeaponKind::Grenade, None)];
        g.teams[0].selected_weapon = 0;
    }, &[(A_BITS, 20), (0, 90)]);
}

/// Homing missile cursor phase: Up moves cursor, A confirms, missile flies.
#[test]
fn weapon_sim_parity_homing_missile() {
    use arty::physics::projectile::WeaponKind;
    use arty::game::state::HomingMissileState;
    assert_all_paths_in_sync(44, |g| {
        g.teams[0].weapons = vec![(WeaponKind::HomingMissile, None)];
        g.teams[0].selected_weapon = 0;
        g.homing_missile = Some(HomingMissileState {
            cursor_x: 500.0, cursor_y: 250.0,
            render_x: 500.0, render_y: 250.0,
            blink_timer: 0, confirmed: false,
        });
    }, &[
        (UP_BITS, 10),   // move cursor up
        (A_BITS, 1),     // confirm target
        (0, 40),         // let missile fly
    ]);
}

/// Multiple turn advances with fire each turn — catches wind/turn state drift.
#[test]
fn weapon_sim_parity_multi_turn() {
    assert_all_paths_in_sync(45, |_| {}, &[
        (A_BITS, 1), (0, 200),  // turn 1: fire + settle
        (A_BITS, 1), (0, 200),  // turn 2: fire + settle
    ]);
}

/// Two sims from the same seed with the same inputs stay identical.
#[test]
fn sim_is_deterministic() {
    use arty::game::loop_runner::server_tick;
    use arty::input::InputState;

    let mut a = build_game(99);
    let mut b = build_game(99);
    let blast = a.teams[1].soldiers[0].pos;
    a.apply_explosion(blast, WeaponKind::Bazooka);
    b.apply_explosion(blast, WeaponKind::Bazooka);

    let input = InputState::new();
    for _ in 0..30 {
        server_tick(&mut a, &input, None, None);
        server_tick(&mut b, &input, None, None);
    }

    assert_eq!(a.crater_log, b.crater_log);
    assert_eq!(a.wind.value(), b.wind.value());
    assert_eq!(a.turn.current_team, b.turn.current_team);
    for (ti, (ta, tb)) in a.teams.iter().zip(b.teams.iter()).enumerate() {
        for (si, (sa, sb)) in ta.soldiers.iter().zip(tb.soldiers.iter()).enumerate() {
            assert_eq!(sa.pos.x, sb.pos.x, "t{ti} s{si} pos.x");
            assert_eq!(sa.pos.y, sb.pos.y, "t{ti} s{si} pos.y");
            assert_eq!(sa.hp, sb.hp, "t{ti} s{si} hp");
        }
    }
}

/// Explosion damage must spawn a floating damage number for every hurt soldier
/// (regression: apply_explosion_scaled applied damage without any popup), the
/// popup is DEFERRED until the damage tally settles (~20 ticks with no new
/// hits), it arms the ~2 s HP-countdown hold, and the event reaches a live
/// client through the fx_events round-trip.
#[test]
fn explosion_damage_emits_popup() {
    use arty::game::loop_runner::server_tick;
    use arty::input::InputState;
    use arty::renderer::fx::FxEvent;

    let mut server = build_game(650);
    let target = server.teams[1].soldiers[0].pos;
    let hp_before = server.teams[1].soldiers[0].hp;
    server.fx_events.clear();
    server.apply_explosion(target, WeaponKind::Bazooka);
    let dealt = (hp_before - server.teams[1].soldiers[0].hp) as u32;
    assert!(dealt > 0, "explosion must damage the target");

    // Deferred: no popup at the moment of impact — it tallies first.
    assert!(
        !server.fx_events.iter().any(|ev| matches!(ev, FxEvent::DamagePopup { .. })),
        "popup must not appear until the damage tally settles"
    );

    // Tick until the tally flushes; capture the popup and that tick's state.
    // The popup is deferred until the world settles (DAMAGE_SETTLE_TICKS=20 with
    // no new hits) — and the explosion first knocks the target airborne, so the
    // window must cover knockback-fall settle time plus the tally, which varies
    // with where the victim lands on the terrain.
    let input = InputState::new();
    let mut found: Option<(u8, u8, arty::net::msg::StateMsg)> = None;
    for tick in 0..120u32 {
        server_tick(&mut server, &input, None, None);
        let popup = server.fx_events.iter().find_map(|ev| match *ev {
            FxEvent::DamagePopup { amount, color_id, .. } => Some((amount, color_id)),
            _ => None,
        });
        if let Some((amount, color_id)) = popup {
            found = Some((amount, color_id, build_state(&server, tick, 0)));
            break;
        }
    }
    let (amount, color_id, state) = found.expect("settled tally must emit FxEvent::DamagePopup");
    assert!(amount as u32 >= dealt, "popup shows the tallied total");
    assert_eq!(color_id, server.teams[1].color_id,
        "popup carries the damaged team's picked colour (matches the HP box)");
    assert!(
        server.teams[1].soldiers[0].hp_countdown_delay > 0,
        "popup must arm the HP-countdown hold"
    );

    // Live-client side: the event rides StateMsg.fx_events and respawns the
    // text, and the countdown hold arrives via NetSoldier.
    let mut client = build_game(server.map_seed);
    let mut cam = Camera::new(0.0, 0.0);
    client.fx_text.clear();
    apply_server_state(&mut client, &mut cam, &state, 0);
    for ev in &state.fx_events {
        arty::renderer::fx::apply_event(&mut client.fx, &mut client.fx_text, ev);
    }
    assert!(!client.fx_text.is_empty(), "live client must spawn the popup text");
    assert!(
        client.teams[1].soldiers[0].hp_countdown_delay > 0,
        "live client must receive the HP-countdown hold"
    );
}

/// Hits landing in quick succession (e.g. 5 pistol shots) tally into ONE popup
/// with the total, not one popup per hit; displayed_hp holds until the ~2 s
/// post-popup delay expires, then ticks down.
#[test]
fn damage_tally_aggregates_and_delays_countdown() {
    use arty::game::loop_runner::server_tick;
    use arty::input::InputState;
    use arty::renderer::fx::FxEvent;

    let mut server = build_game(651);
    let input = InputState::new();
    let hp_before = server.teams[1].soldiers[0].hp;

    // 5 hits of 5, spaced 4 ticks apart — inside the settle window each time.
    let mut popups = 0u32;
    let mut total_shown = 0u32;
    for hit in 0..5 {
        server.teams[1].soldiers[0].take_damage(5);
        for _ in 0..4 {
            server_tick(&mut server, &input, None, None);
            for ev in &server.fx_events {
                if let FxEvent::DamagePopup { amount, .. } = *ev {
                    popups += 1;
                    total_shown += amount as u32;
                }
            }
        }
        let _ = hit;
    }
    // Let the tally settle and flush.
    for _ in 0..30 {
        server_tick(&mut server, &input, None, None);
        for ev in &server.fx_events {
            if let FxEvent::DamagePopup { amount, .. } = *ev {
                popups += 1;
                total_shown += amount as u32;
            }
        }
    }
    assert_eq!(popups, 1, "burst damage must tally into a single popup");
    assert_eq!(total_shown, 25, "popup shows the summed total");

    // displayed_hp holds at the pre-damage value through the ~2 s delay…
    let s = &server.teams[1].soldiers[0];
    assert_eq!(s.hp, hp_before - 25);
    assert!(s.hp_countdown_delay > 0);
    assert_eq!(s.displayed_hp, hp_before, "HP box holds until the delay expires");

    // …then drains 1/tick once the delay runs out.
    for _ in 0..(s.hp_countdown_delay + 30) {
        server_tick(&mut server, &input, None, None);
    }
    assert_eq!(
        server.teams[1].soldiers[0].displayed_hp,
        server.teams[1].soldiers[0].hp,
        "displayed_hp must finish counting down to the real hp"
    );
}

/// Channel-bypass guard: the ONLY legal ways to produce gameplay sounds/FX are
/// `GameState::emit_sound` / `GameState::emit_fx` — they spawn locally AND ship
/// the event to live clients. A raw `.sounds.push(...)`, `.fx_events.push(...)`
/// or `audio::play(...)` anywhere else desyncs live mode (or plays a sound the
/// opponent never hears). The fx spawn fns are already private to fx.rs; this
/// test seals the sound channel, which visibility can't. If you add a legal
/// call site, update the allowlist with a comment explaining why it's safe.
#[test]
fn no_channel_bypass_in_source() {
    use std::path::{Path, PathBuf};

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() { walk(&p, out); }
            else if p.extension().is_some_and(|x| x == "rs") { out.push(p); }
        }
    }

    // (file suffix, pattern, allowed count, why)
    let allow: &[(&str, &str, usize)] = &[
        ("game/state.rs", ".sounds.push(", 1),    // emit_sound body
        ("game/state.rs", "audio::play(", 1),     // emit_sound body
        ("game/state.rs", ".fx_events.push(", 1), // emit_fx body
        ("main.rs", "audio::play(", 1),           // live client replaying StateMsg.sounds
    ];
    let patterns = [".sounds.push(", ".fx_events.push(", "audio::play("];

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    walk(&root, &mut files);

    let mut violations = Vec::new();
    for f in files {
        let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        if rel.starts_with("audio") { continue; } // the audio module implements play()
        let src = std::fs::read_to_string(&f).unwrap();
        for pat in patterns {
            let count = src.matches(pat).count();
            let allowed = allow.iter()
                .find(|(suf, p, _)| rel.ends_with(suf) && *p == pat)
                .map_or(0, |(_, _, n)| *n);
            if count > allowed {
                violations.push(format!(
                    "src/{rel}: {count}x `{pat}` (allowed {allowed}) — route it through \
                     GameState::emit_sound / emit_fx so live clients stay in sync"
                ));
            }
        }
    }
    assert!(violations.is_empty(), "channel bypasses found:\n{}", violations.join("\n"));
}
