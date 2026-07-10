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
8. Gameplay logic lives in ONE place: simulate_with_muzzle(game, input, muzzle,
   aim_angle) in loop_runner.rs. tick() (local) and server_tick() (live+TAT) are
   thin wrappers that call it. Put gameplay changes in simulate_with_muzzle()
   ONLY — do NOT duplicate into the wrappers.
   (Was: "edit both tick() and server_tick()" — that twin-function model is gone
   as of v0.5.4.120; the hand-mirroring caused the live death-explosion bug.)
9. Any weapon that deploys/activates (not just fires-and-forgets a projectile)
   must call turn.on_fired() at the moment of activation, not once its effect
   finishes. Plasma Torch got this wrong for several versions — on_fired() was
   only called when its ~4s burn completed, so the Acting-phase timer kept
   counting down the whole time it was active (fixed v0.5.4.424). Garcia/
   airstrike/TNT/mine/guns all call on_fired() at activation already — use
   those as the reference pattern for any new deploy-style weapon.

## Simulation architecture (loop_runner.rs — search the fn names)
- simulate_with_muzzle(game, input, muzzle, aim_angle) — shared core: phase dispatch
  (Acting/Watching/Retreating/Ending) + end-of-tick cleanup + crate-watch + death
  explosions + SFX + grave settling + visual decay. Returns SimStep. No camera/render.
  aim_angle: Option<f32> — when Some, applied directly in process_aim (server path);
  when None, buttons drive aim (hotseat/TAT). Up/Down are NEVER stripped — they flow
  to cursor-phase weapons (homing missile, airstrike) via process_acting_sim.
- tick() = client preamble (pause/menu/game-over/fire-grace) → process_weapon_menu()
  → simulate_with_muzzle(aim=None) → update_camera() → render().
- server_tick(game, input, muzzle, aim_angle) = game.tick+=1 → simulate_with_muzzle().
  Server passes aim_angle=Some(msg.aim_angle); all other paths pass None.
- replay_tick(game, prev_bits, curr_bits) = process_weapon_menu() → server_tick(aim=None).
  Both TAT paths in main.rs call this.
- update_camera() (client-only) re-derives the follow target post-sim; snaps on
  turn change via lstate.prev_turn_number.
- update_visuals() = LIVE-CLIENT-ONLY per-frame stepper (the live client never
  runs simulate): step_explosions + hp/message/trail decay. TAT no longer calls it.
- Watching ends on: projectiles + explosions + pending_deaths + black_holes + garcia
  empty AND all LIVING soldiers grounded.
- fire_bazooka() private, fire_bazooka_tat() public wrapper
- snap_to_surface(), is_on_ground(), jump_unstick_lift(), death_phrase() public

## Render pipeline (loop_runner.rs render_my_team)
- World cache (sky+terrain) baked once → copy_viewport_from each frame → atmospheric
  background → water ripple → entities → particles → explosions → fx → HUD.
  All background/fx layers are client-only visuals.
- Background order (renderer/background.rs, all share one gusting wind via gust_wind()):
  clouds (par 0.15) → sun + parallax hills (draw_backdrop) → seed landform (par 0.65,
  generate_landform/draw_landform, cached in LoopState.bg_landform, regen on map_seed
  change) → wind debris (sky pixels only; motes now sway + flutter via phase/spin/rot).
- renderer/fx.rs = event-driven effect particles (FxParticle/FxKind: DirtChunk/Spark/
  Dust/Splash). Lives in GameState.fx (NOT networked — like smoke_particles). step_fx()
  runs once per simulate() tick (top, before phase early-returns); draw_fx() after the
  explosion rings. Spawned in apply_explosion_scaled (fallout/splash), at landing,
  footstep, and torch-dig sites. biome_dirt(archetype) colours debris. Capped at FX_MAX.
- sky_colour(x, y, archetype) in draw_terrain.rs is biome-tinted + baked cloud bands;
  horizon color is biome-independent (draw_water_surface trough restore relies on it).

## Gotchas
- cargo miyoo → target/armv7-unknown-linux-gnueabihf/miyoo/mini-mayhem (NOT release/;
  the binary is named `mini-mayhem` per Cargo.toml, not `arty`)
- Server binary stripped by LTO — version not visible via strings, check source
- touch src/server/main.rs to force server rebuild
- nginx: arty-api block port 80 must have both /arty/ and /api/ locations
- HUD is draw_hud_world() in loop_runner.rs NOT renderer/hud.rs
- auto-update uses shell script to avoid FAT overwrite issue
- Creds saved to SDCARD and /tmp as fallback
- API JSON has spaces after colons — json_field handles this
- WeaponKind::max_damage() only feeds explosive splash-damage scaling
  (apply_explosion_scaled, e.g. grenade/TNT/meteor bomb) — for hitscan/bullet
  weapons (Uzi, pistol, revolver, minigun) the real per-shot damage is a local
  `const DAMAGE` inside that weapon's fire_*_shot function. Changing
  max_damage() alone does nothing for those weapons (bit us in v0.5.4.400: the
  "MAC-10 nerf" only touched the unused display stat, not real damage).
- (2026-07-08) apply_explosion_scaled's old +20-within-10px "direct-hit bonus"
  was REMOVED for all weapons — max_damage() is now the literal ceiling, no
  exceptions. If you see damage readings above a weapon's max_damage(), that's
  a regression, not the old intentional bonus coming back.
- Terrain generation/spawn-placement code must use is_solid (terrain only),
  never is_blocked (terrain OR objects) — the per-tick `objects` bitmask is
  populated by stamp_objects() during the running game loop and is always
  empty at generation time. To account for scenery at gen time, query
  self.scenery directly (see surface_y_at_with_scenery, clear_of_scenery).
- Crater::carve is the single choke point for terrain destruction — every
  path calls it (local sim, server, live client crater_log replay, TAT).
  Anything that must stay consistent with cratering across modes (e.g.
  scenery destruction) belongs INSIDE carve, not in the explosion code;
  it then needs no StateMsg fields.
- Scenery destruction is per-pixel, matching terrain exactly: SceneryObject
  has `mask: Option<Vec<bool>>` (world-pixel resolution over its footprint
  box, None = fully intact/no allocation). Crater::carve clears bits within
  the blast circle same as terrain.solid; object drops only once every bit
  is gone. Both stamp_objects (collision) and renderer/scenery.rs's Scaled
  wrapper (rendering) must consult the mask — Scaled keeps the old one-shot
  fill_rect path when mask is None (cheap, the common case) and only falls
  to per-pixel plotting for objects an explosion has actually clipped.
- The muzzle spawn point (pos.y-4-sin*12) is inside the shooter's own hit
  box at steep aim angles. Projectile::owner + cleared_owner exclude the
  shooter as a target until the projectile leaves their box once — any new
  fired-from-the-soldier projectile should set owner (see fire_weapon).
- Per-pixel loops over WorldBuffer/terrain must iterate y-outer/x-inner
  (row-major; x-outer strides 7.7KB per write). atlas_sample's landform-top
  question is answered by Terrain::run_top() from the solid_runs column
  cache (kept fresh by recompute_column_cache) — never rescan upward
  per pixel.

- Any "last resort" / pathological fallback path (e.g. terrain.rs's
  find_team_spawns final branch, unstick_embedded_soldier's search) must be
  checked for uniqueness/bounds explicitly — these branches are reached rarely
  enough that a missing dedup/search-radius check can ship unnoticed for a
  long time (two soldiers spawning on the same pixel, a knocked-back soldier
  permanently embedded in a thick ceiling — both fixed v0.5.4.424, both were
  in fallback branches the normal-case tests don't exercise).
- Rate limiting (per-IP, /register and /login) lives in deploy/arty_api.py
  as an in-memory sliding window (_rate_limited/_rate_hits) — resets on API
  restart, no DB persistence. Deployed v0.5.4.423.

- Scenery sprite colours are TRUE RGB via Bgra::new(r,g,b) — the desktop and
  Miyoo pipelines render the struct's fields faithfully. The scenery-gallery
  writer silently swapped R/B until v0.5.4.428, which had masked two sprites
  authored with reversed channels (mushroom cap, sunflower — they really
  rendered blue/cyan in game). When adding sprite art, trust the fixed
  gallery: what the sheet shows is what the game shows.
- Scenery footprint/scale/variant-count changes are TERRAIN-DETERMINISM
  changes: placement clearance at generation time uses footprint(), so any
  tweak shifts scenery (and spawn interplay) for existing seeds -> bump
  VERSION + REQUIRED_VERSION like any map-gen change.
- MapGEN reference corpus workflow (v0.5.4.428): MapGen.exe (in assets/Worms
  Armageddon/User/MapGen/) runs headless under Wine/Xvfb with a settings file
  — wine MapGen.exe -s FILE -o OUT.png -w; keys type/width/height/objects/
  complexity/floaters/water; `objects 0` + `floaters 0` = near-sprite-free
  1920x696 silhouettes. tools/gen_mapgen_corpus.py regenerates the corpus
  (~/arty-mapgen-corpus, never committed); tools/terrain_stats.py + the
  dump-terrain bin compare our seeds' silhouette metrics against it. bng-type
  output keeps its surface props regardless of settings — stats only, never a
  mask source.

## Services (Pi)
- arty-api.service: systemd auto-start
- Game server: manual — fuser -k 7777/tcp 2>/dev/null; sleep 1; RUST_LOG=info ~/mayhem-server/server
- nginx: /api/ proxies to :7778, /arty/ serves from /var/www/html
