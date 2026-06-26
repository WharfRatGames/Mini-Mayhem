# Mini Mayhem (Arty)

A Worms-style 2D artillery game written in Rust, built for the **Miyoo Mini Plus** handheld. Two teams of soldiers take turns firing an arsenal of destructive weapons across procedurally generated, fully destructible terrain.

## Features

### Gameplay
- Procedural terrain generation with 5 archetypes: rolling hills, cliffs/overhangs, floating islands, caverns, and canyon/mesa
- Fully destructible terrain — craters, tunnels, and collapses persist
- Turn-based flow: Acting → Watching → Retreat → Ending
- Wind, gravity, per-soldier HP, fall damage, and water/drowning mechanics
- Atmospheric visuals: parallax backgrounds, drifting clouds, wind-driven debris, biome-tinted skies

### Weapons
Bazooka, Grenade (variable fuse), Shotgun, Pistol (6-shot burst), MAC-10 (∞), TNT, Landmine, Molotov Cocktail (2 uses, 12 flames), Meteor Bomb, Revolver, Grappling Hook, Baseball Bat, Blasthive (homing bees), Garcia (targeted artillery), Homing Missile, Air Strike, Black Hole Bomb, and more — plus weapon/health/scrap crate drops.

### Game Modes
- **Singleplayer** — VS CPU or local hotseat
- **Live Multiplayer** — real-time TCP matches (casual/ranked, ELO)
- **Take A Turn (TAT)** — asynchronous play-by-mail style matches with login/registration, match queues, and forfeit timers

## Architecture

- `src/main.rs` — entry point, title/lobby/account screens, live client loop, TAT replay
- `src/game/loop_runner.rs` — shared `simulate_with_muzzle()` core; `tick()` (hotseat), `server_tick()` (live+TAT), and `replay_tick()` (TAT replay) are thin wrappers. All 5 execution paths produce identical results — enforced by `assert_all_paths_in_sync` in tests.
- `src/game/net_sync.rs` — `build_state`/`apply_server_state` for live client state rebuild; compile-time parity checklists
- `src/game/` — state, team/loadout, turn system, soldier animation
- `src/physics/` — projectile/weapon definitions, collision and bounce outcomes
- `src/renderer/` — sprites, terrain, backgrounds, particle FX
- `src/net/` — shared network message structs (`msg.rs`, `InputMsg`, `StateMsg`)
- `src/server/` — authoritative game server binary
- `src/api/` — REST API for accounts, matches, leaderboards (SQLite-backed)

## Building

### Desktop (development)
```bash
cargo build
cargo run --bin arty
```

### Miyoo Mini Plus (ARMv7, target device)
```bash
PATH="$ZIG:$PATH" cargo zigbuild --target armv7-unknown-linux-gnueabihf --profile miyoo
```
Output: `target/armv7-unknown-linux-gnueabihf/miyoo/arty`

## Status

See [STATUS.md](STATUS.md) for the current version, detailed changelog, and known issues.
