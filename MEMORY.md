# Arty Developer Memory

## CRITICAL RULES (never violate)
1. NEVER heredocs for Rust code — use the Edit tool for all .rs edits (heredocs corrupt)
2. Always cp src/net/msg.rs src/server/msg.rs after msg changes (STRUCTS ONLY — server's
   encode() returns Option<Vec<u8>>; restore it after any blind cp)
3. For multi-line .rs edits use the Edit tool, NOT sed; sed is fine only for the version bump
4. bincode field changes break deserialization silently
5. Miyoo SDCARD file writes work; SSH direct launch crashes
6. EVERY BUILD: bump VERSION in src/main.rs AND REQUIRED_VERSION in src/server/main.rs
7. Always rebuild AND deploy server binary every build (touch src/server/main.rs)
8. Gameplay logic lives in ONE place: simulate(game, input) in loop_runner.rs.
   tick() (local) and server_tick() (live+TAT) are thin wrappers that call it.
   Put gameplay changes in simulate() ONLY — do NOT duplicate into the wrappers.
   (Was: "edit both tick() and server_tick()" — that twin-function model is gone
   as of v0.5.4.120; the hand-mirroring caused the live death-explosion bug.)

## Simulation architecture (loop_runner.rs — search the fn names)
- simulate() — shared core: phase dispatch (Acting/Watching/Retreating/Ending) +
  end-of-tick cleanup + crate-watch + death explosions + SFX + grave settling +
  visual decay. Returns SimStep (Normal/MenuOpen/CrateWatch). No camera/render.
- tick() = client preamble (pause/menu-render/game-over/fire-grace) → simulate()
  → update_camera() → render(). server_tick() = game.tick+=1 → simulate().
- update_camera() (client-only) re-derives the follow target post-sim; snaps on
  turn change via lstate.prev_turn_number.
- update_visuals() = LIVE-CLIENT-ONLY per-frame stepper (the live client never
  runs simulate): step_explosions + hp/message/trail decay. TAT no longer calls it.
- Watching ends on: projectiles + explosions + pending_deaths + black_holes + garcia
  empty AND all LIVING soldiers grounded.
- fire_bazooka() private, fire_bazooka_tat() public wrapper
- snap_to_surface(), is_on_ground(), jump_unstick_lift(), death_phrase() public

## Gotchas
- cargo miyoo → target/armv7-unknown-linux-gnueabihf/miyoo/arty (NOT release/)
- Server binary stripped by LTO — version not visible via strings, check source
- touch src/server/main.rs to force server rebuild
- nginx: arty-api block port 80 must have both /arty/ and /api/ locations
- HUD is draw_hud_world() in loop_runner.rs NOT renderer/hud.rs
- auto-update uses shell script to avoid FAT overwrite issue
- Creds saved to SDCARD and /tmp as fallback
- API JSON has spaces after colons — json_field handles this

## Services (Pi)
- arty-api.service: systemd auto-start
- Game server: manual — fuser -k 7777/tcp 2>/dev/null; sleep 1; RUST_LOG=info ~/mayhem-server/server
- nginx: /api/ proxies to :7778, /arty/ serves from /var/www/html
