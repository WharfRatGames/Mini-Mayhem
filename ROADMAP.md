# Mini-Mayhem Roadmap

A living document of what's shipped, what's in progress, and what's coming.

---

## ✅ Phase 1 — Terrain & Physics
*Core engine foundation*

- [x] Perlin heightmap terrain generation
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
- [x] Camera follow and free pan
- [x] Hotseat local multiplayer
- [x] Soldier skeletal animation (walk cycle, backflip, airborne lean)
- [x] Team color rendering

---

## ✅ Phase 3 — Weapons & Turn System
*The actual game*

- [x] Turn-based system with timer and retreat phase
- [x] Bazooka, Grenade, Shotgun, TNT, Landmine, Ninja Rope, Baseball Bat, Plasma Torch (loadout)
- [x] Blasthive, Meteor Bomb, Revolver, Black Hole Bomb, Air Strike, Hand of Jerry, Sacred Ordnance (crate-only)
- [x] Weapon unlock timers (Bat / TNT / Air Strike)
- [x] Adjustable grenade fuse (L1/R1)
- [x] Crate drops (weapon, health, scrap)
- [x] Rarity-tier weapon pool (Common / Uncommon / Rare / Ultra Rare)
- [x] Graves and headstones
- [x] Blood splats
- [x] Barrel explosions and chain reactions
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
- [ ] **Map variety** — multiple terrain archetypes (cave systems, island chains, fortress maps)
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
