# Mini-Mayhem Roadmap

A living document of what's shipped, what's in progress, and what's coming.

---

## ✅ Phase 1 — Terrain & Physics
*Core engine foundation*

- [x] Terrain generated from real Worms Armageddon map art — 2 masks extracted from the original game's land.dat, baked as Rust constants, seed picks mask/shift/mirror (v0.5.4.392)
- [x] Seed-based WA collage generation — every seed splices/warps/crossfades segments of real WA art into a novel silhouette; caverns carve chambers from the same art inverted; extraction tool `tools/extract_wa_mask.py` (land.dat/PNG → mask.bin) (v0.5.4.396)
- [x] Collage generation fast path — domain warp on a precomputed bilinear grid; caverns generate faster than the old procedural generator (v0.5.4.397)
- [x] Archetype system removed — replaced by template_id (WA mask) + is_cavern (~20% odds); chasms/overhangs/caves now seed-random on any map (v0.5.4.393)
- [x] Maps twice the screen height — 700px vertical terrain range, generator tuned for full use (v0.5.4.387)
- [x] Vertical spawn spread — soldiers spawn at varied heights (cave ledges, tunnels, mid-terrain) not just the topmost surface (v0.5.4.389)
- [x] Guaranteed-walkable spawns — footing checks now require escape room beyond at least one edge of the footprint, rejecting wall-to-wall soldier-width slots that were standable but unwalkable (v0.5.4.403)
- [x] Mound-series terrain removed — a low-weight sine-relief layered onto every non-cavern map's density field, producing a repeating "series of mounds" look regardless of the WA collage silhouette, has been disabled (v0.5.4.403)
- [x] Terrain generation ~40% faster — precomputed hill_col[], octaves 4→3 (~4.4M fewer noise calls per map, v0.5.4.394)
- [x] Crater carving (destructible terrain)
- [x] Scenery destructible exactly like terrain — per-pixel mask over each object's
      footprint, cleared by the same blast-circle rule as terrain.solid; explosions
      only eat the part of a tree/rock/crate they actually overlap (v0.5.4.407)
- [x] Euler projectile ballistics
- [x] Wind simulation
- [x] Gravity, bounce, and friction physics
- [x] Water death zone

---

## ✅ Phase 2 — Renderer & Input
*Getting it running on the Miyoo*

- [x] `/dev/fb0` direct framebuffer rendering (BGRA)
- [x] `evdev` hardware button input
- [x] 8×8 pixel font
- [x] Camera follow and free pan (horizontal and vertical; L1+Up/Down vertical pan v0.5.4.387; R1+Up/Down with snap-back v0.5.4.389; aim no longer rotates while panning with R1, v0.5.4.394)
- [x] Fixed invisible soldiers — camera-relative draw culling now used for soldiers/headstones/projectiles/explosions instead of a fixed screen window (v0.5.4.395)
- [x] 30fps pacing groundwork — exact 33.333ms tick, absolute-deadline frame pacing (sleep overshoot no longer compounds), NEON-vectorizable fb row flip, per-section µs profiler in TEST overlay (v0.5.4.398)
- [x] All HUD elements screen-anchored to cam_y — stay at correct screen position when camera scrolls vertically (v0.5.4.389)
- [x] Cursor weapons full vertical range — Garcia/Air Strike/Hand of Jerry can reach the waterline (v0.5.4.389)
- [x] Hotseat local multiplayer
- [x] Soldier skeletal animation (walk cycle, backflip, airborne lean)
- [x] WA-timed backflip — whole-skeleton rotation follows the real WA backflip animation's 22-frame curve (eased takeoff, fast tumble, eased landing), measured from frames extracted by new `tools/extract_wa_sprite.py` (v0.5.4.405)
- [x] Faster match load — landform-top texture sampling answered from the solid_runs column cache instead of a per-pixel upward scan (build_world_cache 235→102ms desktop); row-major loop order in the world/bg cache builders; background PNG + terrain tile decode prewarmed on worker threads during map generation (v0.5.4.406)
- [x] Backflip jumps 10% higher — initial launch velocity raised -6.5→-6.82 (v0.5.4.407)
- [x] Team color rendering

---

## ✅ Phase 3 — Weapons & Turn System
*The actual game*

- [x] Turn-based system with timer and retreat phase
- [x] Bazooka, Grenade, Shotgun, MAC-10, Pistol, TNT, Landmine, Ninja Rope, Baseball Bat, Plasma Torch, Clump Bomb, Homing Missile, Molotov (loadout)
- [x] Blasthive, Meteor Bomb, Revolver, Black Hole Bomb, Air Strike, Garcia, Hand of Jerry, Sacred Ordnance (crate-only)
- [x] Molotov Cocktail — 48 fire patches, ~2.5 min burn, WA-style pooling fire physics (pending)
- [x] Weapon unlock timers (Bat / TNT / Air Strike / Homing Missile)
- [x] Adjustable grenade/clump bomb fuse (L1/R1)
- [x] Crate drops (weapon, health, scrap)
- [x] Rarity-tier weapon pool (Common / Uncommon / Rare / Ultra Rare)
- [x] Weapon menu — 4-column scrollable grid, 120px cells (pending)
- [x] Graves and headstones
- [x] Blood splats
- [x] Barrel explosions and chain reactions (14–20 barrels per map)
- [x] Map landmines 16–24 per map (v0.5.4.391)
- [x] Themed scenery objects — 28 per map, styled per WA template / cavern mode (v0.5.4.390/.391)
- [x] Solid scenery — per-sprite collision footprints stamped into the object mask; soldiers stand on them, projectiles collide; placement rejects spots embedded in slopes/overhangs (v0.5.4.398)
- [x] Big grounded scenery — 2–3× scale with matching collision footprints; always seated on ground (cave floors on cavern maps); spawns keep clear of solid props (v0.5.4.399)
- [x] Pistol trimmed to 5-shot burst (v0.5.4.399) — actually firing all 5 shots fixed in v0.5.4.401 (shot counter was set after the first shot instead of before)
- [x] Soldiers can stand on top of other soldiers — square landings stand on the other's head instead of always sliding off (v0.5.4.400)
- [x] Fall damage
- [x] Drown death
- [x] MAC-10 damage cut 40% (8→5/bullet) (v0.5.4.400) — that changed an unused display stat only; the real per-bullet damage cut a further 20% (3→2) in v0.5.4.401
- [x] Meteor Bomb main explosion damage cut 20% (45→36) (v0.5.4.401)
- [x] Barrels and mines spawn on top of scenery objects instead of embedded inside them (v0.5.4.402)
- [x] Scenery objects can no longer end up buried inside terrain raised by the emergency spawn-mound fallback (v0.5.4.403)
- [x] Destructible scenery — explosions (crater radius ≥ 8) remove scenery objects overlapping the blast, deterministically in every mode via Crater::carve; small-arms chips and torch nibbles leave scenery standing (v0.5.4.406)
- [x] Steep-angle bazooka self-detonation fixed — projectiles ignore their shooter until they've left the shooter's hit box once (Projectile::owner) (v0.5.4.406)
- [x] Damage-number popups — getting hit pops a floating "-N" over the soldier's HP counter (Worms Armageddon style), rising and fading as the counter itself ticks down; routed through FxEvent/emit_fx so it auto-replicates to live clients with no StateMsg changes (v0.5.4.407)
- [x] Pistol rapid-fire audio pop fixed — sound clip capped below the burst-shot interval so the audio device is never still busy when the next shot fires (v0.5.4.402)
- [x] Hand of Jerry camera now follows its targeting cursor vertically, matching Air Strike and Homing Missile (v0.5.4.402)
- [x] Hand of Jerry's smash sound now plays at the water line instead of far below it when dropped over open water (v0.5.4.403)

---

## ✅ Phase 4 — Dedicated Server & Live Multiplayer
*Real-time online play*

- [x] Authoritative server simulation (clients send inputs only, server runs physics)
- [x] TCP game server on Raspberry Pi 4
- [x] Live 1v1 real-time matches
- [x] Take a Turn (async) matches
- [x] Version handshake (client/server must match)
- [x] Reconnect window (3-minute grace period for disconnects — both casual and ranked)
- [x] Opponent quit notification with blocking confirmation
- [x] Live-mode parity system (compile-time checklists + integration tests + all-paths test helper)
- [x] OTA (over-the-air) auto-update on launch
- [x] Update UX overhaul — title-screen UPDATE AVAILABLE banner, re-check on MULTIPLAYER select, cancellable non-blocking check gate (fixes Casual Live freeze), handshake off main thread, version-reject opens install screen (v0.5.4.400)
- [x] Python/SQLite REST API (accounts, match history, leaderboard)

---

## ✅ Phase 5 — Accounts, ELO & Economy
*The meta-game layer*

- [x] Account registration and login
- [x] ELO rating system (K=32, floor 100)
- [x] Ranked and casual queues (both TAT and Live)
- [x] Leaderboard (top wins + top kills, per-mode)
- [x] Scrap currency (soft, earned from matches + login + challenges)
- [x] Warbonds currency (premium)
- [x] Daily and weekly challenges
- [x] Shop (hats, gun styles, uniform colors, boot colors, headstones)
- [x] Daily login rewards + streak bonuses
- [x] Cosmetic sync in live multiplayer (opponent's hats/uniforms/guns visible)
- [x] Roster editor with per-soldier cosmetics

---

## 🚧 In Progress / Near-Term

- [ ] **Scrap earned on game-over screen** — show how much scrap you earned from the match before returning to title
- [ ] **Profile screen** — view owned cosmetics, current balance, win/loss record from within the game
- [ ] **Roster editor live preview** — see your soldier update in real time while picking cosmetics
- [ ] **Port 443 / HTTPS** — router port-forwarding for TLS on the API (nginx config and cert are ready; awaiting port forward)

---

## 🔭 Planned — Phase 6 (Polish & Live Ops)

- [ ] **Spectator mode** — watch a live match in progress without participating
- [ ] **Replay system** — save and replay matches locally
- [ ] **Additional weapons** — new crate-only weapons to expand the pool
- [ ] **Map variety** — more real WA terrain masks beyond the current 2, additional sub-variants
- [ ] **Server monitoring dashboard** — uptime, active matches, player counts
- [ ] **Rate limiting on API** — per-IP rate limits on `/register` and `/login` to prevent brute force
- [ ] **Input stream logging** — full per-match input logs for future replay analysis and anti-cheat
- [ ] **4-player live matches** — extend live mode beyond 1v1
- [ ] **Tournament bracket** — organized competitive play with bracket progression
- [ ] **Seasonal resets** — periodic ELO soft-resets with season reward cosmetics

---

## Technical Notes

The game server is authoritative — clients cannot influence positions, HP, or match outcomes. All simulation runs server-side in live multiplayer.

Parity is enforced at three layers:
1. **Compile-time** — `_gamestate_parity_checklist` in `src/game/net_sync.rs` forces every new `GameState` field to be classified synced or unsynced before the code compiles.
2. **Architecture** — `aim_angle: Option<f32>` flows through `server_tick` so Up/Down buttons reach cursor-phase weapons without special-casing in server preprocessing. New weapons added to `simulate_with_muzzle` are automatic in all 5 execution paths.
3. **Runtime tests** — `assert_all_paths_in_sync` in `tests/parity.rs` runs any input sequence through hotseat, server, TAT replay, and live client paths and asserts `synced_snapshot` matches across all of them.
