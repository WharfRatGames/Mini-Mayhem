//! Live-multiplayer parity guard.
//!
//! The live client never runs `simulate()` — it rebuilds state from `StateMsg`
//! via `net_sync::{build_state, apply_server_state}`. If a field changes in the
//! sim but isn't carried through that round-trip, live mode silently diverges
//! from every other mode. These tests fail the build when that happens.
//!
//! Lives in `tests/` (an integration target) so it compiles against the lib's
//! normal build and is unaffected by stale inline `#[cfg(test)]` modules.

use arty::game::net_sync::{build_state, apply_server_state};
use arty::game::state::GameState;
use arty::game::team::{Team, Difficulty};
use arty::game::soldier::SoldierState;
use arty::physics::projectile::WeaponKind;
use arty::renderer::Camera;
use arty::world::{Terrain, WorldPos, WORLD_W};

/// Deterministic two-team game from a seed (mirrors `build_default_game_opts`,
/// minus the bin-private mine/barrel seeding).
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

fn state_discriminant(s: &SoldierState) -> u8 {
    match s {
        SoldierState::Idle        => 0,
        SoldierState::Walking{..} => 1,
        SoldierState::Airborne{..}=> 2,
        SoldierState::Dead        => 3,
        _                         => 4,
    }
}

/// Assert the client (rebuilt purely from a StateMsg) matches the authoritative
/// server game on every field the netcode promises to sync.
fn assert_parity(server: &GameState, client: &GameState) {
    assert_eq!(server.teams.len(), client.teams.len(), "team count");
    for (ti, (st, ct)) in server.teams.iter().zip(client.teams.iter()).enumerate() {
        assert_eq!(st.soldiers.len(), ct.soldiers.len(), "team {ti} soldier count");
        assert_eq!(st.active, ct.active, "team {ti} active soldier");
        for (si, (ss, cs)) in st.soldiers.iter().zip(ct.soldiers.iter()).enumerate() {
            assert_eq!(ss.pos.x, cs.pos.x, "team {ti} soldier {si} pos.x");
            assert_eq!(ss.pos.y, cs.pos.y, "team {ti} soldier {si} pos.y");
            assert_eq!(ss.hp, cs.hp, "team {ti} soldier {si} hp");
            assert_eq!(ss.facing, cs.facing, "team {ti} soldier {si} facing");
            assert_eq!(ss.has_fired, cs.has_fired, "team {ti} soldier {si} has_fired");
            assert_eq!(state_discriminant(&ss.state), state_discriminant(&cs.state),
                       "team {ti} soldier {si} state");
        }
    }
    assert_eq!(server.wind.value(), client.wind.value(), "wind");
    assert_eq!(server.turn.current_team, client.turn.current_team, "turn team");
    assert_eq!(server.crater_log, client.crater_log, "crater log");
    assert_eq!(server.crates.len(), client.crates.len(), "crate count");
    assert_eq!(server.mines.len(), client.mines.len(), "mine count");
    assert_eq!(server.barrels.len(), client.barrels.len(), "barrel count");
    assert_eq!(server.black_holes.len(), client.black_holes.len(), "black hole count");
    assert_eq!(server.fire_patches.len(), client.fire_patches.len(), "fire patch count");
}

/// The core gap-catcher: perturb the server, round-trip through the netcode, and
/// assert nothing was dropped. A field added to the sim but not to build_state
/// will diverge here.
#[test]
fn round_trip_preserves_synced_state() {
    let mut server = build_game(1234);
    let mut client = build_game(1234);
    let mut cam = Camera::new(0.0);

    // Perturb: move/damage a soldier, advance the turn, set wind, and detonate
    // an explosion (populates crater_log, fx_events, sounds, soldier damage).
    server.teams[0].soldiers[0].pos.x += 17.0;
    server.teams[0].soldiers[0].facing = -1;
    server.teams[1].soldiers[0].hp = 55;
    server.turn.current_team = 1;
    server.wind = arty::physics::Wind::new(0.42);
    let blast = server.teams[1].soldiers[0].pos;
    server.apply_explosion(blast, WeaponKind::Bazooka);
    server.emit_sound(arty::audio::Sfx::Explosion);

    // The explosion must have produced craters + fx for the assertions to bite.
    assert!(!server.crater_log.is_empty(), "explosion should carve a crater");
    assert!(!server.fx_events.is_empty(), "explosion should emit fx_events");

    let state = build_state(&server, 7, 0);
    apply_server_state(&mut client, &mut cam, &state, 0);

    assert_parity(&server, &client);

    // Cosmetic event channels: build_state must carry these verbatim, else live
    // clients lose the matching SFX / particle bursts.
    assert_eq!(state.sounds, server.sounds, "sounds channel dropped by build_state");
    assert_eq!(state.fx_events, server.fx_events, "fx_events channel dropped by build_state");
}

/// Every Sfx variant must survive the u8 wire encoding round-trip intact.
/// Forgetting to add a new sound to `from_u8` would silently drop it on live
/// clients; this test catches that before it ships.
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
        let encoded = sfx as u8;
        let decoded = Sfx::from_u8(encoded);
        assert_eq!(decoded, Some(sfx), "{sfx:?} did not survive u8 round-trip");
    }
}

/// HHG FuseState variants (Armed, Detonating) must survive the net encoding round-trip.
#[test]
fn hhg_fuse_state_net_roundtrip() {
    use arty::physics::projectile::{FuseState, WeaponKind};
    use arty::world::{WorldPos, Vec2};
    use arty::physics::projectile::Projectile;

    let mut server = build_game(42);
    let mut client = build_game(42);
    let mut cam = arty::renderer::Camera::new(0.0);

    // Spawn an HHG projectile in Armed state
    let pos = WorldPos::new(500.0, 100.0);
    let mut proj = Projectile::new(pos, Vec2::new(0.0, 0.0), WeaponKind::HolyHandGrenade);
    proj.fuse = FuseState::Armed;
    server.projectiles.push(proj.clone());

    // Spawn one in Detonating state
    let mut proj2 = Projectile::new(pos, Vec2::new(0.0, 0.0), WeaponKind::HolyHandGrenade);
    proj2.fuse = FuseState::Detonating(5);
    server.projectiles.push(proj2.clone());

    let state = build_state(&server, 1, 0);
    apply_server_state(&mut client, &mut cam, &state, 0);

    assert_eq!(client.projectiles.len(), 2, "both HHG projectiles should survive round-trip");
    assert_eq!(client.projectiles[0].fuse, FuseState::Armed, "Armed state round-trip failed");
    assert_eq!(client.projectiles[1].fuse, FuseState::Detonating(5), "Detonating(5) round-trip failed");
}

/// Muzzle fields on InputMsg must survive the bincode wire round-trip intact.
/// If they were accidentally dropped, live-mode hitscan would silently fall back
/// to the approximation formula instead of the exact rendered barrel tip.
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
    assert_eq!(decoded.muzzle_x, 123.45, "muzzle_x lost in wire round-trip");
    assert_eq!(decoded.muzzle_y, 67.89, "muzzle_y lost in wire round-trip");
}

/// The shared sim must be deterministic: two games from the same seed fed the
/// same inputs stay identical. Guards against time-based RNG / map iteration
/// order creeping in (which would desync server vs client).
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
        server_tick(&mut a, &input, None);
        server_tick(&mut b, &input, None);
    }

    assert_eq!(a.crater_log, b.crater_log, "crater log diverged");
    assert_eq!(a.wind.value(), b.wind.value(), "wind diverged");
    assert_eq!(a.turn.current_team, b.turn.current_team, "turn diverged");
    for (ti, (ta, tb)) in a.teams.iter().zip(b.teams.iter()).enumerate() {
        for (si, (sa, sb)) in ta.soldiers.iter().zip(tb.soldiers.iter()).enumerate() {
            assert_eq!(sa.pos.x, sb.pos.x, "team {ti} soldier {si} pos.x diverged");
            assert_eq!(sa.pos.y, sb.pos.y, "team {ti} soldier {si} pos.y diverged");
            assert_eq!(sa.hp, sb.hp, "team {ti} soldier {si} hp diverged");
        }
    }
}
