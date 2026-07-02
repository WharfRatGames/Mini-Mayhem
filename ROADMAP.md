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
- [x] Terrain generation ~40% faster — precomputed hill_col[], octaves 4→3 (~4.4M fewer noise calls per map, v0.5.4.394)
- [x] Crater carving (destructible terrain)
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
- [x] 30fps pacing groundwork — exact 33.333ms tick, absolute-deadline frame pacing (sleep overshoot no longer compounds), NEON-vectorizable fb row flip, per-section µs profiler in TEST overlay (in working tree, pending deploy)
- [x] All HUD elements screen-anchored to cam_y — stay at correct screen position when camera scrolls vertically (v0.5.4.389)
- [x] Cursor weapons full vertical range — Garcia/Air Strike/Hand of Jerry can reach the waterline (v0.5.4.389)
- [x] Hotseat local multiplayer
- [x] Soldier skeletal animation (walk cycle, backflip, airborne lean)
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
- [x] Solid scenery — per-sprite collision footprints stamped into the object mask; soldiers stand on them, projectiles collide; placement rejects spots embedded in slopes/overhangs (in working tree, pending deploy)
- [x] Fall damage
- [x] Drown death

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
