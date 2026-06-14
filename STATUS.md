# Mini Mayhem — Project Status

## Version: 0.5.4.132
## Modes: SINGLEPLAYER (VS CPU / Hotseat) | LIVE GAME | TAKE A TURN (async TAT)

## Recent changes (0.5.4.121–0.5.4.132)
- Bazooka rocket 50% bigger (11×3px) + scaled smoke trail
- Live opponent charge meter fixed
- VS screen now shown for all live matches
- Garcia (Hand of Jerry) sprite: bolt filled solid white, hand colors restored
  (red/blue), bolt stays white in `deploy/assets/GARCIA.png`
- Water level raised 10%

## Recent changes (0.5.4.120 — live/local parity)
- **Shared simulation core**: `tick()` (local) and `server_tick()` (live+TAT) are
  now thin wrappers over one `simulate()` in loop_runner.rs. Gameplay logic lives
  there only — ends the hand-mirrored twin-function drift. Camera follow/snap
  moved to client-only `update_camera()`.
- **Death explosions now fire in live + TAT** (were silent — `step_death_explosions`
  / `record_deaths` only ran locally, so the queued death blast never resolved).
- **Headstones in live**: graves are server-authoritative — `StateMsg.graves`.
- **Crate-watch unified**: the 3 s post-drop input hold + crate message now apply
  in live too (previously live skipped it).
- **Crate type in live**: `NetCrate.kind_u8` — weapon/scrap crates render with the
  right colour/symbol (were all shown as white health crates).
- **Blood splats in live**: networked via `StateMsg.blood_splats`.
- **Death messages in live**: generated client-side from synced name +
  `NetSoldier.death_cause_u8` (server only has default names) via shared
  `death_phrase()`. Death SFX routed through `emit_sound(Sfx::Death/DeathWater)`.
- **Garcia (Hand of Jerry) camera** now tracks the cursor/falling sprite in live.
- Intentionally NOT networked: opponent's weapon inventory (keeps crate pickups
  hidden) — opponent's open weapon menu shows their default loadout.

## Recent changes (0.5.4.7x–0.5.4.81)
- Spawn fallback = separate rounded mounds spread to the emptiest gaps across a
  half (no more single flat slab that bunched a whole team in a boxy void); only
  adds dirt with a tapered headroom dome, reads as hills (0.5.4.81)
- Charge meter / aim reticle now originate from the skeletal gun muzzle (0.5.4.80)
- Even spawn spread across each landform top + instant match-end on team wipe
  (check_win() every tick) (0.5.4.78)
- Wider spawn spacing (MIN_SEP=140, TNT-safe) + seed shown in TEST mode (0.5.4.77)
- Bigger maps (WORLD_W 1920 = 3 screens) + black hole 40% smaller (0.5.4.76)
- Tactical terrain: 3-octave relief, central chasm pass, landform-aware spawning
  on real post-gen terrain (no pillars/shelves); 5 archetypes (0.5.4.73–0.5.4.75)
- Texture atlas terrain (59 pooled tiles), SFX no-clip limiter, per-weapon kill
  stats; assorted bug fixes (0.5.4.5x–0.5.4.70)

---

## Infrastructure
- Miyoo 1: root@10.0.0.110  Miyoo 2: root@10.0.0.126  Pi: arty-pi (10.0.0.123)
- Game server: port 7777 (Rust aarch64 binary, systemd arty-game service)
- API server: port 7778 → nginx /api/ (Python, systemd arty-api.service)
- Update OTA: http://crumbonium.duckdns.org/arty/ (/var/www/html/arty/)
- DB: ~/mayhem-server/arty.db (SQLite)

---

## What Works

### Core Gameplay
- Tactical terrain generation (5 archetypes: hills, cliffs/overhangs, floating
  islands, caverns, canyon/mesa; central chasms, water zone)
- Turn system: Acting → Watching → Retreat → Ending
- Wind, gravity, per-soldier HP, fall damage, water drowning
- Camera: follow active soldier, R1 snap-pan, L1 free-pan
- Pause menu, game-over screen, hotseat hot-seat turn advance

### Weapons
- Bazooka (infinite, charged, wind-affected)
- Grenade (infinite, fuse L1/R1 1–5 s)
- Shotgun (crate, 2/turn, instant, blood splat)
- TNT (crate, 1 use, placed, 5 s fuse, locked until turn 5)
- Landmine (crate, placed, 3 s arm → proximity trigger → 1 s fuse after the arm beep)
- Meteor Bomb / BananaBomb (crate, lands + scatters 5 burning fragments; initial blast 60% of TNT)
- Revolver (crate, 6 hitscan shots/turn, re-aim between shots; hits any body part)
- Grappling Hook (3/turn + crate, free movement tool — does NOT end turn)
- Baseball Bat (crate, melee 30 dmg + knockback; locked 3 cycles)
- Blasthive / Beehive (crate, throws hive → 6 homing bees, 12 dmg/sting, no knockback)
- Black Hole Bomb (crate, ~4%; gravity well pins soldiers 5 s then 35 dmg on collapse)
- Shared team loadout; depleted weapons removed from menu automatically
- Crate pool: Mine 17 / TNT 13 / Meteor 12 / Revolver 8 / Beehive 7 / BlackHole 4 / Shotgun 6 / Rope 5 / Bat 5 / Health 23 (%)

### Crates & Map
- Weapon + health crates; parachute descent; gravity-fall when terrain below destroyed
- Destructible on 20+ damage in a turn
- 9–15 map-generated landmines per seed
- Animated water with foam/shimmer

### HUD & UI
- Team avatars, HP bars, ELO (ranked), turn timer (pauses while charging)
- Soldier names above HP box; death/event messages over avatars
- Turn-start message, crate-drop message, weapon indicator bottom-left
- Weapon menu: 2-column grid, ammo counter, fuse selector, grapple icon
- Days-remaining shown during TAT turns (bottom-right, colour-coded)

### Take A Turn (TAT)
- Login / register (case-insensitive); 15-match limit; 14-day forfeit timer
- Casual and ranked queues; ELO shown ranked only
- Match list scrolls (8 visible), days-remaining per match
- Opponent move screen 3 s before replay; crate pickup messages suppressed during replay
- Opponent soldier names use team name (e.g. "Smith 1")
- MOVE SUBMITTED screen 3 s after submitting
- Roster selection per match (avatar, headstone, soldier names)

### Live Multiplayer
- TCP bincode authoritative server; CASUAL / RANKED lobby
- Version-gated (server rejects wrong client versions)
- Auto-OTA update on title screen
- Full gameplay/visual parity with local modes (death explosions, headstones,
  crate-watch hold, crate types, blood splats, death messages, Garcia camera).
  Server runs `simulate()`; client renders authoritative state + crater-derived
  explosion flashes. Opponent weapon inventory deliberately hidden.

---

## Known Issues / Next Up
- Kill/death tracking not yet wired through match-end POST body
- Ninja rope TAT replay accuracy (may drift if physics diverge)
- Reconnect after live-game disconnect not implemented
- Sound effects in (ALSA 48k mono, per-sound no-clip limiter); grapple/fire still silent
- .110/.126 Miyoos frequently offline — OTA staged on Pi auto-updates them; direct
  push when reachable (kill arty first — file locked while running)

---

## Key Files
- `src/main.rs` — entry, title, connect, run_tat_game()
- `src/game/loop_runner.rs` — simulate() (shared core), tick()+update_camera() (client),
  server_tick() (server/TAT wrapper), update_visuals() (live-client visual stepper),
  death_phrase(), all weapon/physics/render logic
- `src/game/state.rs` — GameState, RopeState, crate pool
- `src/game/team.rs` — loadout, prune_empty_weapons()
- `src/game/lobby.rs` — LobbyScreen, LobbyAction, TAT match list
- `src/game/title.rs` — TitleScreen, How To Play pages
- `src/game/account.rs` — AccountScreen, http_post/get, credentials
- `src/physics/projectile.rs` — WeaponKind enum, net serialisation
- `src/physics/outcome.rs` — grenade bounce, wall/floor collision
- `src/net/msg.rs` — network structs (must match server/msg.rs)
- `src/server/main.rs` — live game server, REQUIRED_VERSION
- `src/renderer/draw_sprites.rs` — soldier, water, weapon icons
- `deploy/update_server.sh` — OTA push + Miyoo direct deploy

---

## Build & Deploy (verified)
```bash
ZIG="/home/dusty/miyoo-games/move_square/zig-linux-x86_64-0.13.0"
# 1. Bump VERSION (src/main.rs) AND REQUIRED_VERSION (src/server/main.rs), same value
# 2. Add a changelog line to deploy/changelog.txt (newest first; served live)
# Client (Miyoo armv7) — MUST use --profile miyoo:
PATH="$ZIG:$PATH" cargo zigbuild --target armv7-unknown-linux-gnueabihf --profile miyoo
# Server (Pi aarch64):
PATH="$ZIG:$PATH" cargo zigbuild --target aarch64-unknown-linux-gnu --release --bin server

# Server deploy + restart
scp target/aarch64-unknown-linux-gnu/release/server arty-pi:/home/Grunkus/arty-server.new
ssh arty-pi "mv /home/Grunkus/arty-server.new /home/Grunkus/arty-server && kill \$(pgrep arty-server)"
# Client OTA staging
scp target/armv7-unknown-linux-gnueabihf/miyoo/arty arty-pi:/home/Grunkus/arty-client
ssh arty-pi "cp /home/Grunkus/arty-client /var/www/html/arty/arty && echo '<VERSION>' > /var/www/html/arty/version.txt"
# Direct push to BOTH Miyoos (kill first — file locked while running)
ssh root@10.0.0.126 "killall arty 2>/dev/null; sleep 1" && scp target/armv7-unknown-linux-gnueabihf/miyoo/arty root@10.0.0.126:/mnt/SDCARD/App/Arty/arty
ssh root@10.0.0.110 "killall arty 2>/dev/null; sleep 1" && scp target/armv7-unknown-linux-gnueabihf/miyoo/arty root@10.0.0.110:/mnt/SDCARD/App/Arty/arty
```
Client output is `target/armv7-unknown-linux-gnueabihf/miyoo/arty` (miyoo profile, NOT release/).

### Dev-host terrain/texture preview
The crate builds natively as a lib (`cargo build --lib`), so terrain/tiles can be
rendered to PNG via a throwaway `examples/*.rs` using `build_world_cache` +
`terrain_textures::tile` — useful for verifying texture/terrain changes off-device.

## Changelog Location
The on-screen update notes are fetched live from `/arty/changelog.txt` (via
`updater::fetch_changelog`). Edit `deploy/changelog.txt` (one line per release,
newest first); `deploy/update_server.sh` SCPs it to the host on deploy.
