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
- [x] Grappling hook physics overhaul — swing tuned to reverse-engineered reference constants (gentler gravity/swing, tangential along-arc control, faster reel), firing-angle limit, vertical auto-detach on ground, rope-knocking, post-rope retreat window, and a multi-corner rope that bends around and unwinds off terrain corners (v0.5.4.418); ground-stick fix so the soldier rests on the surface instead of sinking (v0.5.4.419)
- [x] Map verticality restored — .417's relief compression relaxed (RELIEF_COMPRESSION 2.0→1.25, DEPTH_RAMP 9.0→4.0) so cliffs/peaks/valleys are tall again, now that the grapple reaches isolated tops (v0.5.4.419)
- [x] Rope reel-out fix + WA-accurate hard rope constraint — paying out rope with Down now actually lowers the soldier; rope hard-snaps to the current-length circle every tick like WA's real constraint instead of waiting on gravity to fill slack (v0.5.4.420)
- [x] WA mask library grown 2→12 (10 island, 2 cavern) — extracted from real WA `MapGen.exe` output under Wine across several game types; caverns now use real extracted cavern art instead of inverted island art (v0.5.4.420)
- [ ] Bazooka physics matching WA's real feel — v0.5.4.421 (first attempt) ported reference-derived gravity/wind/launch/charge constants but playtesting said it still didn't match WA; reverted 2026-07-07 after 7 RE rounds failed to find a confirmable charge-duration ground truth. Bazooka is back to the shared gravity/launch/charge physics; revisit if a reliable measurement method turns up
- [x] Bazooka wind-hook — strong wind can visibly curve a bazooka around terrain corners, or even reverse its horizontal direction mid-flight on slow/near-vertical shots, the classic WA trick. Scoped narrower than the reverted attempt above: only the wind constant changed (`BAZOOKA_WIND_SCALE`, 4x the shared value), gravity/launch/charge left alone, linear velocity-independent wind model kept as-is (v0.5.4.421)
- [x] Crate-watch camera no longer cuts to the active soldier while a supply crate is still falling — was gated on a fixed 90-tick timer, now holds on any unlanded crate (v0.5.4.421)
- [x] Turn no longer ends mid damage-tally — `TurnPhase::Ending` now also blocks turn advance while any soldier's damage popup/HP-drain sequence is still in progress, not just while the active soldier is airborne (v0.5.4.421)
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
- [x] Weapon unlock timers (Air Strike / Homing Missile) — Bat's timer was already dead/unenforced and TNT's was removed outright in v0.5.4.415, both now available from turn 1
- [x] Adjustable grenade/clump bomb fuse (L1/R1)
- [x] Crate drops (weapon, health, scrap)
- [x] Rarity-tier weapon pool (Common / Uncommon / Rare / Ultra Rare)
- [x] Weapon menu — 4-column scrollable grid, 120px cells (pending)
- [x] Graves and headstones
- [x] Blood splats
- [x] Barrel explosions and chain reactions (14–20 barrels per map)
- [x] Map landmines 16–24 per map (v0.5.4.391)
- [x] Themed scenery objects — 28 per map, styled per WA template / cavern mode (v0.5.4.390/.391)
- [x] Solid scenery — per-sprite collision footprints stamped into the object mask; soldiers stand on them, projectiles collide; placement rejects spots embedded in slopes/overhangs (v0.5.4.398) — **hitbox removed entirely in v0.5.4.417**; scenery is now purely cosmetic
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
- [x] Damage numbers tally instead of stacking — hits landing within ~0.7s aggregate into one popup, HP counter holds 2s before draining; shotgun still pops one number per shell (v0.5.4.409/.410)
- [x] Spawn points spread across the whole vertical range of the map (valley floors, mid-level ledges, underside platforms), never by mutating terrain — constraints relax instead of stamping artificial mounds (v0.5.4.410)
- [x] Shotgun reworked to a single precise hitscan ray per trigger pull (up to 25 damage, 50 for both shots) — the pellet scatter at the impact point is purely cosmetic (v0.5.4.410)
- [x] Chain-reaction damage (mine/barrel cascades, death explosions, full burns) tallies into one popup instead of a flurry of separate numbers (v0.5.4.411)
- [x] Plasma torch tunnels widened (bore r15→r17) so soldiers always fit through (v0.5.4.411)
- [x] Live match-start ready gate — server holds turn 1 and broadcasts frozen state until every client has loaded in; both players see turn 1 start on the same tick (v0.5.4.411/.412)
- [x] Live damage popups always land on screen — camera holds on the damaged soldier during retreat (`damage_focus` now synced) instead of playing out off-camera (v0.5.4.412)
- [x] Damage-popup colour now matches the victim's picked lobby colour instead of raw team index — correct in 4-colour live casual (v0.5.4.412)
- [x] Shotgun damage now falls off with distance from the impact point — a clean, centred hit still deals the full 25, but a graze or a shot that clips ground beside a worm deals proportionally less, and a near-miss onto terrain next to a worm now splashes it instead of doing nothing (WA gun-blast model; v0.5.4.413)
- [x] Sacred Ordnance max damage reduced 100 → 80 (v0.5.4.414)
- [x] Fall damage retuned — safe threshold 80px → 130px, damage rate 0.15/px → 0.10/px (v0.5.4.416)
- [x] Scenery hitboxes tightened for 11 round/irregular sprite types (rocks, bushes, piles, boulders, crystals, skulls, cairns) toward their visual core instead of a rectangle around the full sprite spread; full per-pixel masks would be the complete fix (v0.5.4.416)
- [x] Fixed mines/barrels/crates/fire-patches not rendering when the camera is scrolled vertically to view the lower half of a tall map, while remaining fully solid/live in the sim — could look like an object detonated "out of nowhere" (v0.5.4.416)
- [x] Fixed spawn placement clumping the whole team into one tiny cluster on badly fragmented maps with no wide landforms — fallback now greedily maximizes separation instead of first-fit scanning (v0.5.4.415)
- [x] Terrain relief compressed ~2× so maps are actually playable — WA-collage cliffs were 100–290px tall while soldiers walk up 8px / jump ~16px / backflip ~46px, stranding valley soldiers below unreachable tops; mask now sampled zoomed-out around a mid-band anchor + a depth ramp that guarantees connected ground and melts floating chunks + per-column cave crust; island p95 cliff 126–287px → 28–57px, guard test `island_relief_is_traversable` (v0.5.4.417)
- [x] Scenery hitboxes removed entirely — following .416's footprint-tightening pass, decorative props (rocks/bushes/crates/crystals/etc.) no longer collide at all; soldiers and projectiles pass straight through (v0.5.4.417)
- [x] Grapple wall-stick fix — swinging into a wall used to hard-detach the rope and force-land the soldier at the last clear point (read as getting stuck); now the soldier stops at the last clear point with velocity zeroed but stays attached, so gravity/tension pulls them free next tick instead of forcing a landing (v0.5.4.417)
- [x] Dead gray-barrel scenery prop removed — `draw_barrel` was part of an unreachable 4th "Tropical" scenery archetype never wired into the live Theme enum (Underground/Pastoral/Rugged only); inert dead code, not something ever seen in-game (v0.5.4.417)

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
- [x] Live-match weapon-kill stats fixed — `kill_weapon` was never synced to the live client (the parity checklist claimed it "arrives as a message", which never actually happened), so every live-match kill reported weapon "UNKNOWN" for missions/leaderboards even though it was set correctly server-side; TAT/hotseat (real local simulation) were unaffected. Added `NetSoldier.kill_weapon_u8` (v0.5.4.415)
- [x] Server hardening — `panic=unwind` server build profile (was inheriting `release`'s `panic=abort`, so one match panicking could abort the whole process and take every other in-progress match down with it) + `catch_unwind` per match thread; TLS+app handshake moved off the single accept-loop thread into a per-connection thread so a burst of simultaneous connects parallelizes instead of serializing (v0.5.4.413)
- [x] API DB latency fixed — TAT list / test-match start / all DB reads were slow: one shared SQLite connection serialized every request (→ per-request `open_db()` with WAL + `synchronous=NORMAL`), zero indexes (→ 11 added), `/matches/pending` N+1 (→ single JOIN), `/match/create` triple-commit (→ one txn); Python-only, deployed to Pi 2026-07-05 (v0.5.4.417 cycle)
- [x] Terrain generation parallelized across all cores (density field + box blur, island + cavern) — was the multi-second freeze between selecting a mode and the match appearing on the Miyoo; bit-identical output so no desync (260ms → 60–90ms desktop, ~2× Miyoo, ~3× Pi) (v0.5.4.417)

---

## ✅ Phase 5 — Accounts, ELO & Economy
*The meta-game layer*

- [x] Account registration and login
- [x] ELO rating system (K=32, floor 100)
- [x] Ranked and casual queues (both TAT and Live)
- [x] Leaderboard (top wins + top kills, per-mode)
- [x] Scrap currency (soft, earned from matches + login + challenges)
- [x] Warbonds currency (premium)
- [x] Daily and weekly challenges — expanded from 3 fixed challenges each to a pool of 16 daily / 17 weekly candidates (including per-weapon kill challenges); 3 are deterministically selected per period (seeded by the period string), so every player sees the same rotating set on a given day/week but it changes day-to-day/week-to-week (v0.5.4.415)
- [x] Shop (hats, gun styles, uniform colors, boot colors, headstones)
- [x] Daily login rewards + streak bonuses
- [x] Cosmetic sync in live multiplayer (opponent's hats/uniforms/guns visible)
- [x] Roster editor with per-soldier cosmetics

---

## 🚧 In Progress / Near-Term

- [ ] **Bug reporter fixes, not yet built/committed** — screenshot dimming bug (was re-dimming
      the live WorldBuffer every tick instead of the pristine capture, crushing to black
      within 1-2 ticks), Discord forwarding fixed (missing `?key=` on the bot notify call was
      causing a silent 403), "report sent" screen shortened from 12s to 2.5s/5s
      (success/failure)
- [ ] **Scrap earned on game-over screen** — show how much scrap you earned from the match before returning to title
- [ ] **Profile screen** — view owned cosmetics, current balance, win/loss record from within the game
- [ ] **Roster editor live preview** — see your soldier update in real time while picking cosmetics
- [ ] **Port 443 / HTTPS** — router port-forwarding for TLS on the API (nginx config and cert are ready; awaiting port forward)

---

## 🔭 Planned — Phase 6 (Polish & Live Ops)

- [ ] **Spectator mode** — watch a live match in progress without participating
- [ ] **Replay system** — save and replay matches locally
- [ ] **Additional weapons** — new crate-only weapons to expand the pool (Robot/Sheep-style walker added 2026-07-08; still open for more)
- [ ] **Map variety** — more real WA terrain masks beyond the current 2, additional sub-variants (library grown 2→18 total across island/cavern as of 2026-07-08; still open for more)
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
