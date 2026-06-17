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
Bazooka, Grenade (variable fuse), Shotgun, TNT, Landmine, Meteor Bomb, Revolver, Grappling Hook, Baseball Bat, Blasthive (homing bees), and Black Hole Bomb — plus weapon/health crate drops.

### Game Modes
- **Singleplayer** — VS CPU or local hotseat
- **Live Multiplayer** — real-time TCP matches (casual/ranked, ELO)
- **Take A Turn (TAT)** — asynchronous play-by-mail style matches with login/registration, match queues, and forfeit timers

## Architecture

- `src/main.rs` — entry point, title screen, connection handling
- `src/game/loop_runner.rs` — shared `simulate()` core driving both local (`tick()`) and server/live (`server_tick()`) updates, weapon/physics logic, visuals
- `src/game/` — state, team/loadout, lobby, title, account screens
- `src/physics/` — projectile/weapon definitions, collision and bounce outcomes
- `src/renderer/` — sprites, terrain, backgrounds, FX
- `src/net/` — shared network message structs (`msg.rs`)
- `src/server/` — live game server (authoritative `simulate()`)
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
