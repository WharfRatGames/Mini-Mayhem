# Mini Mayhem — Project Status

## Version: 0.5.4.165
## Modes: SINGLEPLAYER (VS CPU / Hotseat) | LIVE GAME | TAKE A TURN (async TAT)

## Recent changes (0.5.4.135–0.5.4.165)
- **Fix sealed/unescapable cave spawns on caverns maps (0.5.4.165)** — cave
  spawn placement now flood-fills (walk/fall/jump within reasonable limits)
  from the candidate floor to confirm it actually connects to an open-to-sky
  exit, instead of just checking that an unroofed floor exists somewhere
  nearby (which could be on the other side of a wall). Fixes seed
  `18B918CE5F30EA29`'s bottom-left soldier trapped in an unreachable tunnel.
- **Fix black borders on BG1-derived backgrounds (0.5.4.164)** — bg_0..3,
  bg_extra_city, and bg_extra_pyramids (6 of the 15 pooled backgrounds) had a
  ~3-9px near-black border baked in from BG1.png's contact-sheet grid lines,
  visible as a black edge along the top/sides of the screen once scaled up.
  Now cropped (10px margin) before scaling so the art stretches to fill.
- **Merge bg+terrain viewport copy for cave columns (0.5.4.163)** — perf:
  the background cache now only paints the sky band above `sky_limit`; the
  terrain viewport copy's cave/chasm/overhang branch now fills the gaps
  between solid spans with background pixels in the same pass, so each pixel
  in `sky_limit..WATER_Y` is written once instead of twice. Parallax preserved
  (same parallax-shifted source column used for both the sky band and the gap
  fills).
- **Per-section pixel-write profiling overlay (0.5.4.162)** — TEST mode now
  shows a top-right breakdown of how many pixels each render section
  (terrain+bg, water, objects, soldiers, fire patches, plasma torch, garcia,
  black holes, smoke trail, projectiles, fx overlay, status, avatars,
  messages, hud, weapon indicator, seed display, fps counter) wrote in the
  last frame, sorted descending, to help target the next 30fps optimization.
- **Restore background parallax without pixelation (0.5.4.161)** — the
  background cache now stores the chosen image at native 1:1 resolution (one
  cache column per source-image column, no stretching), and
  `copy_bg_viewport` re-samples it each frame with a parallax-shifted (0.10)
  column offset that wraps within the cached width. The cache is no longer
  terrain-dependent, so crater carves no longer need to repaint it.
- **Fix background pixelation (0.5.4.160)** — the 0.5.4.158 parallax fix
  applied a 0.10 factor directly to the world-x -> image-x mapping, which
  stretched each source-image column across ~10 world pixels (severe
  blockiness). The background cache now maps world columns 1:1 to image
  columns (tiled); this layer scrolls 1:1 with the world like terrain, with
  no parallax (parallax and pixel-perfect art turned out to be incompatible
  for a precomputed world-space cache).
- **Balance + perf (0.5.4.159)** — bazooka direct hits capped at 50 damage
  (was up to 70 with the direct-hit bonus); bee stings reduced from 12 to 5
  damage each; bazooka smoke trail now spawns from the rocket's tail instead
  of its nose; background images are now bilinear-scaled instead of
  nearest-neighbor (fixes pixelation on the ~1.5x upscale to SCREEN_H); perf:
  cave/chasm terrain columns now use precomputed solid spans
  (`Terrain::solid_runs`) in the sky-aware viewport copy instead of a
  per-pixel `is_solid` check (~230k checks/frame removed on cave/chasm maps).
- **Background cache parallax + full BG1 pool (0.5.4.158)** — the
  0.5.4.157 background cache dropped this layer's camera parallax as a
  tradeoff and only included 4 of BG1.png's 6 slices; parallax (0.10) is
  now baked into the world-x -> image-x mapping used when building the
  cache, and all 6 BG1 slices are included (pool is now 15, was 13).
- **BG2 background cache (0.5.4.157)** — the BG2 sky image was re-rendered
  per-pixel from source every frame, painting down to the waterline
  (~640x400px) for every cave/chasm/floating-island column — by far the
  largest remaining per-frame cost. Now pre-rendered once per map into a
  world-space cache and stamped into the viewport via row memcpys.
- **Framebuffer blit perf (0.5.4.156)** — the unconditional every-frame
  180-degree screen rotation blit was reversing each row one byte at a time
  (~1.2M indexed ops/frame); now copies whole pixels via chunked slice
  copies. Unlike fire patches (which the user confirmed don't affect fps),
  this runs every frame regardless of scene contents, making it the most
  likely remaining cause of the 25fps vs 30fps target.
- **Fire-patch flame perf (0.5.4.155)** — flame rendering now uses
  unchecked pixel access for the common in-bounds case (was bounds-checking
  every pixel of every flame row, up to ~250 checks per burning fire/soldier
  each frame).
- **Soldiers stuck on any uphill slope (0.5.4.154)** — the horizontal-move
  leading-edge sweep in `try_move_horizontal` checked each intermediate
  column against the soldier's *current* foot height with no step-up
  allowance, so a 1px rise in terrain immediately ahead halted the entire
  move (truncating it back to 0px) even though the destination check a few
  lines later would happily step up to 8px. Projectiles don't go through
  this function and were unaffected — explaining "soldiers can't move,
  projectiles can." The sweep now allows the same 0-8px step-up tolerance.
- **Light-blue patches on fresh maps + skeleton draw perf (0.5.4.153)** —
  found the real root cause of the light-blue patches reported even before
  any terrain is destroyed: `Terrain::find_team_spawns` (spawn-mound raising)
  clears a tapered "headroom" dome above each mound, which can punch a new
  air gap into ground that was previously solid between the old `sky_limit`
  and the mound top — but only updated `sky_limit`/`solid_to_water` when the
  mound raised the column's visible top, leaving them stale (`solid_to_water
  == true` with an actual gap) on ~90-870 columns per map depending on seed.
  Now calls `Terrain::recompute_column_cache` unconditionally after raising
  each mound column, like `Crater::carve` already does. Also:
  `WorldBuffer::draw_line` (used by every soldier skeleton bone segment) now
  bounds-checks once per line instead of once per pixel, using
  `set_pixel_unchecked` in the common in-bounds case — closing more of the
  fps gap.
- **Stuck soldiers (round 3) + camera shake + water perf (0.5.4.152)** —
  `is_on_ground`/`jump_unstick_lift` now check the full 3-column body
  footprint (left edge, center, right edge), matching `try_move_horizontal`;
  previously they only checked the center column, so a soldier could be
  "on ground" per the gate but have movement silently rejected by the
  stricter footprint check on an edge column — reading as stuck on invisible
  terrain. Camera no longer shakes left-right during multi-explosion Watching
  phases: instead of always following `explosions.last()` or the first
  airborne soldier (which flip-flop between widely separated x positions as
  entries are added/removed), it now follows whichever is closest to the
  camera's current center. Also: `draw_water_surface` now uses unchecked
  pixel access (closing more of the fps gap).
- **Crater cache + remaining perf fix (0.5.4.151)** — `Crater::carve` clears `solid[]`
  bits but previously left `sky_limit`/`solid_to_water` stale; for columns that were
  `solid_to_water == true`, the viewport copy's block-copy fast path then painted the
  cached pre-carve pixels (a flat placeholder sky colour) over the new hole, showing
  a "light blue" patch instead of the real background. `Crater::carve` now calls
  `Terrain::recompute_column_cache` for every affected column. Also: the viewport
  copy's per-column air-gap branch and the sun-glow disc now use
  `is_solid_unchecked`/`get_pixel_unchecked`/`set_pixel_unchecked` to drop redundant
  bounds checks from their hot per-pixel loops (closing the rest of the 25->30fps gap).
- **Stuck soldiers + perf fix (0.5.4.150)** — `snap_to_surface`/`land_on_surface` now
  check the full 3-column body width (left edge, center, right edge), matching
  `try_move_horizontal`'s footprint; previously they only checked the center column,
  so a soldier could be snapped sideways into terrain at an edge and then appear stuck
  on nothing. Also fixes the 25fps regression from 0.5.4.149: `draw_static_bg`'s
  air-gap paint (up to ~360 rows/column) now writes pixels directly via
  `set_pixel_unchecked` instead of the bounds-checked `set_pixel`. `dp.sh` now also
  runs `update_server.sh` so the nginx zip/`version.txt`/changelog/manifest/server
  binary are refreshed on every normal deploy.
- **New backgrounds (0.5.4.148)** — `assets/BG/BG2.png` (a 3×3 contact sheet) is sliced
  into 9 painted skies (`deploy/assets/backgrounds/bg2_0..8.png`). `renderer/bg_image.rs`
  holds the 9-image pool; one is chosen per map from the seed (`bg_index_for_seed`), so
  backgrounds vary match-to-match across every archetype (replaces the old per-archetype
  bg_0..3). Deterministic so client/server/live agree.
- **Ghosting fix (0.5.4.149)** — the 0.5.4.148 background sky-band clip left air pixels
  below the surface (chasms, caves, fresh craters) un-repainted, so stale frame-buffer
  content (title screen, persistent wind particles/explosions, blacked-out terrain) showed
  through. Now the sky-band clip is only applied to fully-solid columns; columns with an
  air gap are painted down to the waterline.
- **Render perf (0.5.4.148)** — background drawn only in the sky band on solid columns;
  `WorldBuffer::fill_rect`/`fill_circle` rewritten to clamp-once + contiguous-row
  `copy_from_slice` instead of per-pixel bounds-checked `set_pixel`.
- **Collision + spawns (0.5.4.147)** — walk/airborne collision now uses full body
  width+height (fixes clipping through walls/ceilings); spawns require the same full-body
  clearance; viewport copy block-copies fully-solid columns (`solid_to_water`).
- **Spawn-mound + water-surface render fixes (0.5.4.144–146)**; **floating-island land
  density tuned (0.5.4.143)**.
- **Static background images, first version (0.5.4.135)** — `renderer/bg_image.rs` added,
  per-archetype PNGs from `deploy/assets/backgrounds/` (later superseded by the 9-image
  seed-rotated pool in 0.5.4.148).
- `examples/bg_preview.rs` — composites background + terrain to a viewport PNG (with a
  sentinel-fill ghosting check) for host-side eyeballing.

## Recent changes (0.5.4.134)
- Terrain heightmap amplitude +10% (noise scale 0.48 → 0.528 of terrain range)
- Background debris reacts more strongly to wind (vx scaled 3x at spawn, wind
  influence per tick 0.05 → 0.25)
- Garcia targeting cursor speed 6 → 14; removed full-screen vertical targeting line
- Falling Garcia hand now sinks behind the water surface (re-drawn after the sprite)
- Soldier airborne terrain collision now checks only the top half of soldier height
  (was full body) to reduce snagging/clipping through terrain while jumping

## Recent changes (0.5.4.121–0.5.4.133)
- **Atmospheric backgrounds, second pass** (client-only visual; `renderer/background.rs`
  + new `renderer/fx.rs`) — adds, on top of the first pass:
  - **Seed-generated mid-ground landforms**: a procedural silhouette ridge built from the
    *same map seed* as the terrain (`generate_landform`, `+8000` noise offset), flavored
    per archetype (rolling hills / ridged cliffs / terraced canyon mesas / island humps /
    cavern massif). Cached in `LoopState`, regenerated on a new match, drawn at parallax
    0.65 behind the real terrain which always occludes it.
  - **Drifting clouds** (parallax 0.15, soft additive biome-tinted blobs) + **wind gusts**
    (`gust_wind` — synthesized visual modulation of the turn-fixed wind that all ambient
    layers share; strong gusts throw extra debris).
  - **Livelier debris**: motes sway in arcs and 2px ones flutter as tumbling flakes/leaves.
  - **Effect particles** (`fx.rs`, client-only, not networked — same pattern as smoke):
    explosion fallout (dirt chunks + sparks), water splashes, landing dust, footstep dust,
    and plasma-torch dig chips. Cheap fake-physics (gravity + wind drift + fade), capped
    at `FX_MAX`; stepped once per `simulate()` tick, drawn over the explosion rings.
- **Worms-style atmospheric backgrounds** (first pass, `renderer/background.rs`):
  biome-tinted sky + faint baked cloud bands (`draw_terrain::sky_colour`), a sun glow +
  two parallax distant-hill ridges, and wind-driven ambient debris per map archetype
  (snow / pollen / sea-mist / dust / embers). All drawn behind terrain (sky pixels only).
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
