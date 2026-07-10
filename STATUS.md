# Mini Mayhem — Project Status

## IN PROGRESS 2026-07-10 — Fire terrain-eating: implemented, rate/duration NOT WA-calibrated

Molotov/barrel/crate fire now gradually eats terrain while burning (WA-style), reversing part
of the 0.5.4.425 "no crater" behavior below — implemented in `step_fire_patches`
(`src/game/state.rs`) via a small periodic `Crater::carve` while a `FirePatch` is landed.
Added `FirePatch.landed_ticks` (new field, propagated through `NetFirePatch` in
`src/net/msg.rs`, `build_state`/`apply_server_state` in `net_sync.rs`, and the parity
checklist + test snapshots in `tests/parity.rs` per CLAUDE.md's sync rules). Pushed to Miyoo
`.126` as an **unversioned working-copy build** (no `VERSION` bump) for testing — `.110` not
touched, Pi/server/GitHub/Discord release unchanged at 0.5.4.425/f2f606b.

**Current constants** (`step_fire_patches`): `BURN_CARVE_RADIUS=1.5`, `BURN_CARVE_INTERVAL=8`
ticks, `BURN_CARVE_DURATION_TICKS=150` (5s @ 30fps, counted per-patch from when that patch
lands — carving stops after, damage/burn continues). **These are a hand-tuned guess, not a
measured match to real WA** — user asked to match WA's actual rate/duration, but every
attempt to measure it (both static RE and live Wine-rig dynamic capture) failed to produce a
trustworthy number this session. Full history, what was tried, and why it failed is in
`PROGRESS.txt` under "TASK IN PROGRESS (2026-07-10)" — read that before resuming, don't
re-derive from scratch. Short version: static RE dead-ended (only found sprite filenames, no
constants); the Wine rig repeatedly defeated automated timing/camera tracking (per-turn
camera hard-cuts, short post-fire retreat window, pre-turn input lockout, unreliable
real-time↔game-time ratio). User is going to try a different model/session for the
calibration step next.

## Infra fix 2026-07-10 (non-gameplay, no code changes) — Windows backups silently broken since 2026-06-30

Both Windows-side backup Scheduled Tasks (`ArtyBackup`, `Pi4doomBackup`) had been silently
no-oping every run since 2026-06-30. Root cause: PowerShell's execution policy on the Windows
machine blocked all `.ps1` scripts outright ("running scripts is disabled on this system") —
invisible because the tasks launch via a hidden `wscript.exe`/`-WindowStyle 0` wrapper that
swallows console errors. Fixed with `Set-ExecutionPolicy RemoteSigned -Scope CurrentUser` on
the Windows box. While diagnosing (needed inbound SSH to the Windows machine to run
diagnostics), also found and fixed an unrelated Windows Firewall misconfiguration: a
`Block SSH WAN` rule was scoped to `RemoteAddress=Any` instead of the built-in `Internet`
keyword, so it was blocking LAN SSH too, not just WAN as intended — narrowed its scope so SSH
from the internet is still blocked but LAN access works.

Verified both tasks end-to-end post-fix: `ArtyBackup` produced a fresh `arty-*.tar.gz` and then
auto-fired successfully on its own next hourly Task Scheduler trigger (`Last Result: 0`).
`Pi4doomBackup` pulled a fresh `pi4doom-*.img.gz` whose size (12,003,436,314 bytes) matches the
source file on the build machine exactly. Both tasks confirmed `Scheduled Task State: Enabled`
with correct triggers (hourly / every-3-days). Caveat: both are `Logon Mode: Interactive only`
— they silently skip their run if nobody is logged into the Windows desktop session at trigger
time; switching to "run whether logged on or not" would need the account password stored in
the task and wasn't done.

## Version: 0.5.4.425 DEPLOYED to Pi/server/GitHub/Discord (2026-07-09, commit f2f606b) — Molotov fire overhaul

Reworked fire so soldiers caught in a Molotov can survive and get out instead of being
trapped and instakilled. All in `step_fire_patches` (`src/game/state.rs`) plus the fire
render section (`src/renderer/loop_runner.rs`) and new `src/renderer/wa_sprites.rs`.
Tests: `tests/fire_reaction.rs` (3) + `tests/flame_render.rs`; parity 23/23.

- **Animated flame sprites**: procedural teardrop flame replaced by a 32-frame flame sprite
  (flicker loop → shrink-to-spark burn-out), two variants so neighbours aren't in lockstep.
  Baked `src/renderer/wa_sprites/flame1.bin` / `flame2.bin`, loader `wa_sprites.rs`.
  Render-only (reads synced `FirePatch` fields) — no `StateMsg`/net changes.
- **Molotov leaves no crater**: `apply_explosion_scaled` skips terrain carving, the
  `crater_log` push, and the dirt-fallout FX for `WeaponKind::MolotovCocktail` — a dug pit
  plus a fire pool was an inescapable death trap. Blast keeps its light damage/knockback.
- **Bounded burn damage**: capped per soldier at 1 HP / 8 ticks (~3.75 HP/s) instead of per
  fire patch — a Molotov spawns ~48 overlapping patches, which used to stack into an instakill.
- **Escape movement**: burning soldiers hop (every 14 ticks; `HOP_VX 4.0`, `HOP_VY 4.0`) and
  slide in one direction committed at ignition (away from the blast, latched into the existing
  `Soldier.facing` — no new synced field). A barrier ahead flips the hop the other way; burning
  soldiers pass through each other mid-hop so a clump on a peak can separate. The earlier
  downhill-slide was removed (it dragged worms back into the pooled fire and they oscillated).

## Version: 0.5.4.424 DEPLOYED to Pi/server/GitHub/Discord (2026-07-08, commit fe17553)

- **Plasma torch now stops the turn timer on deploy**: `turn.on_fired()` was only called once
  the torch finished burning (~4s later), so the Acting-phase timer kept counting down the whole
  time it was active — every other weapon (TNT, mine, Garcia, airstrike, guns) already stopped
  it immediately on fire/deploy. Now calls `on_fired()` at activation (A press), matching that
  pattern; `step_plasma_torch` keeps running via its `in_torch` bypass regardless of turn phase,
  so nothing else needed to change.
- **Spawn-overlap fix**: `find_team_spawns`'s pathological last-resort fallback (used only when
  no standable spot exists anywhere in the band) had no uniqueness check — integer division in
  the `base` column formula could collapse two different spawn indices onto the same point on a
  badly fragmented map, landing two soldiers on the exact same pixel. Now nudges vertically until
  clear of every already-placed spawn.
- **Ceiling-stuck soldiers fixed further**: the `.422` `unstick_embedded_soldier` watchdog (see
  below) only searched vertically within ±48px and silently gave up (soldier left visibly
  embedded in `Idle` state) if that failed — e.g. knockback burying a soldier deep in a thick
  overhang/cavern ceiling. Now also tries a horizontal search (a pocket thick top-to-bottom can
  still be open to the side), then as a last resort teleports to the nearest column with a known
  standable spot rather than leaving the soldier stuck forever.
- Gates: `cargo check --tests` clean, `cargo test --test parity` 23/23, `cargo test --test
  wa_collage_check` 6/6.

## Version: 0.5.4.423 DEPLOYED to Pi/server/GitHub/Discord (2026-07-08, commit 6424cc8)

- **Rate limiting on `/login` and `/register`** (`deploy/arty_api.py`): per-IP sliding window,
  5 attempts / 60s per endpoint, in-memory with a background cleanup thread purging stale
  entries every 5 minutes. Over the limit returns `429 {"error": "too many attempts..."}`.
  Client (`src/game/account.rs`) recognizes the 429 and shows "TOO MANY ATTEMPTS, TRY AGAIN
  LATER" instead of falling through to the generic wrong-password/registration-failed message.

## Version: 0.5.4.422 DEPLOYED to Pi/server/GitHub/Discord (2026-07-08, commit bf70571)

- **New weapon: Robot** (`WeaponKind::Robot`, WA Sheep-style autonomous walker). Placed like
  TNT (instant, no aim/charge). Walks/climbs 0-8px steps; when blocked by something taller it
  launches a single automatic obstacle-clearing jump (ballistic arc, terrain-detection driven,
  not player input) and reverses only if it lands still blocked — matches WA's Sheep exactly.
  10s fixed real-time fuse (ticks unconditionally every tick regardless of turn/phase), numeric
  countdown over its head for the final 5s. Detonates on fuse expiry, water contact, being
  caught in another explosion, or a manual A-press by the placing team during their own turn
  (server only forwards the active team's input, so true any-time remote detonation isn't
  wired — same-turn only). Placing team's own turn holds in Watching until it detonates, then
  the normal retreat window opens. Rare-tier crate drop. 75 max damage / 45 blast radius / 18
  knockback. In-world sprite has a 2-frame walk-cycle leg animation, glowing eyes, mouth, and
  antenna; matching robot-head weapon-menu icon. Fully synced (`RobotState`/`NetRobot`/parity
  checklist in net_sync.rs + tests/parity.rs). Two real bugs found and fixed post-implementation:
  (1) the jump never actually happened — the immediate ballistic step called
  `robot_snap_to_surface`, which re-landed it on the ground it just launched from (still within
  the 10px landing-scan window) before it ever visibly moved; (2) horizontal clearance was
  tested at the pre-jump ground height instead of the risen height, so it never saw an obstacle
  as cleared. Fixed by only landing-checking once past the arc's apex (`vel_y > 0`) and testing
  clearance at the post-move y.
- **Explosion direct-hit bonus REMOVED for all weapons.** `apply_explosion_scaled` used to add
  a flat +20 (capped at 99) whenever a soldier was within 10px of the blast center, for every
  explosive except Blasthive/Bazooka/HomingMissile. Removed entirely — `max_damage()` is now
  the true ceiling for every weapon regardless of distance, no exceptions.
- **Movement smoothing over curved terrain** (crater lips, hills). `snap_to_surface` and
  `is_on_ground` (loop_runner.rs) widened from 3 fixed probe columns (left/center/right edge)
  to the full soldier body width, so they agree with `try_move_horizontal`'s already-full-width
  check — a curve's true peak between the 3 old sample points no longer gets missed, which used
  to read as a jerky extra step. `try_move_horizontal` itself needed no change (already correct).
- **WA mask library grown 12→18** (14 island, 4 cavern) — new masks extracted from fresh
  `MapGen.exe` output under Wine/Xvfb (headless CLI: `-t TYPE -o FILE -w`, much simpler than
  driving the full game), post-processed with a connected-component cleanup pass (morphological
  opening + border-touching component filter) to strip decorative sprites (trees/cacti/skulls)
  that MapGen bakes into its preview PNG, keeping only the real terrain silhouette. Chosen for
  genuinely curvy/rounded contours (rolling hills, wavy canyons, blobby cave chambers).
- **Spawn placement**: (a) cavern spawn-clustering bug fixed — a strict single-pass separation
  scan used to silently skip a soldier outright once a chamber was too small to fit everyone
  with full separation, and the fallback logic (built for open-air/surface terrain) couldn't
  rescue cavern maps, so skipped soldiers defaulted to the same point. Replaced with the same
  greedy max-min-separation dispersion pattern already used elsewhere, so a cramped chamber now
  spreads as far as it actually allows. (b) `MIN_SEP`/`MIN_SEP_V` loosened 140/120→70/60px after
  dynamic analysis of real WA (Wine) showed it does NOT enforce meaningful separation between a
  team's worms (observed two same-team worms spawn ~20-30px apart) — kept a modest floor rather
  than matching WA's fully loose placement. (c) scenery clearance margin widened footprint+10→
  footprint+24 so two nearby decorations' exclusion zones merge and swallow gaps too narrow to
  actually stand/move in, instead of leaving a technically-clear sliver. (d) barrels now also
  check horizontal scenery clearance (`too_close_to_scenery`, main.rs) — they always checked
  landing height against scenery but never horizontal distance, so one could land stacked
  directly against/on top of a decoration.
- **Stuck-soldier turn-lock fix**: blast knockback into a cramped cavern ceiling could embed a
  soldier in terrain; the airborne swept-movement loop would revert to that exact (embedded)
  spot every tick since its first sub-step was already blocked, so the soldier never landed,
  `state` stayed `Airborne` forever, and the turn could never advance. Added
  `unstick_embedded_soldier` watchdog: nudges up or down (whichever clears first) before normal
  airborne physics; forces `Idle` if truly nowhere clear is found nearby.
- **Boulder (Rugged scenery) height reduced** 48px→33px — was just over the ~46px backflip
  apex, making it un-climbable; confirmed via a new labeled scenery reference gallery (below).
- **Weapon menu scroll fix**: Up/Down used to wrap as soon as they hit the *current column's
  own* item count (columns can be shorter than the grid when item count isn't a multiple of 4),
  so a short column could never reach a row that only existed in a longer column — and since
  scrolling follows the cursor's row, the view could get stuck short of the bottom. Now falls
  back to column 0 (always full-length by construction) whenever the current column lacks an
  item at the target row.
- **New dev tool**: `cargo run --bin scenery-gallery` renders every scenery sprite per theme
  (Pastoral/Rugged/Underground) into a labeled `assets/scenery_gallery.png` with footprint
  dimensions — added after having to guess which sprite a "gray dome" bug report meant.
- **Robot jump distance tuned**: `ROBOT_JUMP_VX` 2.2→3.8 so the obstacle-clearing hop covers
  ~40-60px (WA Sheep-like) instead of ~26px, while keeping the existing ~21px peak height and
  ~0.4s air time (both already matched the target from the vel_y/gravity pair, untouched).
- **Retreat-phase camera damage-hold fixed**: `damage_focus` used to expire on a fixed 50-tick
  timer that could cut the camera away before the damage popup + HP countdown actually finished
  (worst case ~80 ticks: 20-tick settle + 60-tick countdown). Simplified from
  `Option<(WorldPos, u32)>` to `Option<WorldPos>` and it now clears only when
  `GameState::any_soldier_tallying()` goes false or the player takes the stick — whichever comes
  first — wired through `StateMsg`/`net_sync.rs`/`main.rs` for the live client.
- Gates: `cargo check --tests` clean, `cargo test --test parity` 23/23, `cargo test --test
  wa_collage_check` 6/6.

## Version: 0.5.4.421 DEPLOYED to Pi/server/GitHub/Discord (2026-07-07, commit 04240d8) —
## bazooka wind-hook + crate-camera fix + damage-tally turn-end fix. This is a *new* build that
## reclaims the .421 number from the reverted physics port below (originally tagged .422, then
## the GitHub release/Discord post were deleted and redeployed as .421 to reuse the number).
## Modes: SINGLEPLAYER (VS CPU / Hotseat) | LIVE GAME | TAKE A TURN (async TAT)
## Miyoo push: BOTH .110 and .126 unreachable this session (offline on LAN) — push pending once
## either device is back online. .126 still runs an ad-hoc pre-version-bump test build of the
## wind-hook change (reports itself as 0.5.4.420) from earlier in this session.

## Deployed 2026-07-07 (admin dashboard — delete account)
Added a "Delete Account" action to the player detail panel in the admin dashboard
(`deploy/dashboard/index.html`), backed by a new `POST /admin/delete_account` endpoint
in `deploy/arty_api.py` (admin-key gated, deletes the user row plus rosters/cosmetics/
warbond-transactions/challenges/queue rows keyed to that user_id; match history rows are
left as-is). Confirmation requires typing the exact username, not just OK/Cancel. Deploy-only
change (dashboard HTML + Python API on the Pi) — no client/server binary or protocol change,
so no VERSION bump.

## Deployed 2026-07-07 (v0.5.4.421 — bazooka wind-hook, crate-camera fix, damage-tally fix)
Bazooka gets its own wind constant (`BAZOOKA_WIND_SCALE = 0.32` vs. the shared `WIND_SCALE = 0.08`,
`src/physics/tick.rs`) so strong wind can visibly hook a rocket around terrain or even reverse its
horizontal direction mid-flight — the classic WA trick. Deliberately scoped narrower than the
reverted .421 physics port below: gravity/launch/charge untouched, only the wind term, and the
existing linear/velocity-independent wind model is kept as-is (matches real WA behavior) rather
than replaced. Crate-watch phase now holds on any unlanded crate instead of a fixed 90-tick timer,
so the camera no longer cuts to the active soldier while a supply crate is still falling.
`TurnPhase::Ending` now also blocks turn advance while any soldier still has
`pending_damage/damage_settle/hp_countdown_delay > 0`, not just while the active soldier is
airborne — turns can no longer end mid damage-tally/HP-drain. All three changes live inside
`simulate_with_muzzle`/`tick()` (auto-parity across all 5 paths per CLAUDE.md); confirmed via
`cargo test --test parity` (22/22 passing). GitHub release + Discord patch-notes post were first
published as v0.5.4.422, then deleted and republished as v0.5.4.421 (see Version note above).

## Reverted 2026-07-07 (v0.5.4.421 first attempt — bazooka physics port)
Ported bazooka-only gravity/wind/launch/charge constants from live dynamic analysis of the
reference WA.exe (Wine + gdb). Fixed a real bug post-ship (auto-fire compared against the shared
`MAX_CHARGE=1.3` overcharge band instead of true full charge `1.0`, so every shot over-charged and
over-launched vs. the ported numbers). Playtesting still said it didn't feel like WA. Seven RE
rounds (3 static, 4 dynamic) hunting for a readable charge-accumulator in worm/game-struct memory
found nothing — the only real data is a velocity-fit extrapolation (~26 frames/~0.52s @50fps from
4 measured shots), not a direct read. Reverted the whole commit (`git revert 958313b` → `e87c72e`)
rather than ship unconfirmed numbers; this also reverted three unrelated bundled changes (rope
wall-clip fix, grapple-onto-scenery, HP-tick-down turn-hold, cavern skull shrink, WA random-drop
spawns) that will need redoing separately if still wanted. GitHub release `v0.5.4.421` and its
Discord patch-notes post were deleted; `changelog.txt`/`posted_versions.txt` on the Pi cleaned up.
Bazooka is back to the shared `GRAVITY=0.3`/`WIND_SCALE=0.08`/`launch=20.0`/`CHARGE_RATE=0.02`
physics every other weapon uses. RE trail preserved in the `reference-wa-ghidra-re` and
`reference-wa-wine-dynamic-analysis` memory notes.

## Deployed 2026-07-06/07 (v0.5.4.420 — commit 181d291)
Rope reel-out fix (paying out rope with Down now actually lowers the soldier — physics was
unconditionally stripping outward-radial velocity even when slack had just been added) plus a
WA-accurate hard positional rope constraint (re-snaps onto the current-length circle every tick,
reverse-engineered from WA's `FUN_005009c0`, rather than a soft gravity-fills-slack pendulum). WA
mask library grown 2→12 (10 island, 2 cavern) by extracting real WA `MapGen.exe` output under
Wine across several game types — caverns now use real extracted cavern art instead of inverted
island art. VERSION + REQUIRED_VERSION bumped (terrain-determinism contract: mask growth changes
RNG-selected output for existing seeds).

## Building 2026-07-06 (v0.5.4.419 — VERSION + REQUIRED_VERSION bumped to .419)

### Grapple ground-stick fixed (`loop_runner.rs`)
Swinging on the rope could sink the soldier into the ground and freeze it. Root cause: the swing
swept-collision loop started *ahead* of the soldier's position and defaulted `last_clear` to the
(already-embedded) point, so the hit branch pinned the soldier there at zero velocity every tick;
attach never lifted the soldier clear either. Added `rope_unstick()` — lifts an embedded rope
soldier straight up to the nearest clear body-column position — called at the top of the swing
block each tick and right after attach. Runs in `simulate` (all 5 paths); no `StateMsg` change.

### Map verticality restored (`wa_templates.rs`, `terrain.rs`)
Partial revert of .417's relief compression, now that the grapple can reach tall/isolated tops:
`RELIEF_COMPRESSION` 2.0→1.25, `DEPTH_RAMP` 9.0→4.0, `GROUND_T` 0.62→0.66 (surface band
~84px→~190px), ledge gap 30–44px→30–60px. Maps are much taller/more vertical again. The
`island_relief_is_traversable` test guard was relaxed (>60px-cliff fraction 8%→20%) to permit
grapple-reachable verticality. Terrain is seed-derived (not networked) but client+server must run
identical code — hence the version bump.

## Deployed 2026-07-06 (v0.5.4.418 — commit 3719e5f)
Grapple physics overhaul from reverse-engineering WA.exe: WA-accurate gravity/hook/swing/reel
constants (tangential swing), firing-angle limit, vertical auto-detach, rope-knocking, retreat
window, and a multi-corner `RopeState.wrap` chain (synced via `NetRope.wrap`, parity-covered).
Rope-swing body pose; wooden crates halved.

## Deployed 2026-07-05 (v0.5.4.417 — VERSION + REQUIRED_VERSION bumped to .417)

### Scenery hitboxes removed entirely (`loop_runner.rs`)
Following .416's footprint-tightening pass, scenery (rocks/bushes/crates/crystals/etc.) is now
purely cosmetic — deleted the `stamp_objects()` loop that wrote scenery footprints into the
object collision mask. Soldiers and projectiles now pass straight through scenery.

### Grapple wall-stick fix (`loop_runner.rs`, `apply_all_gravity`)
Swinging into a wall used to hard-detach the rope and force-land the soldier at the last clear
point (with fall damage) — read as getting "stuck to the wall." Now wall contact stops the
soldier at the last clear point with velocity zeroed but keeps the rope attached; gravity/tension
pulls them free next tick instead of forcing a landing. Lives inside `apply_all_gravity`, called
from `simulate_with_muzzle` — automatically covered on all 5 paths per the parity rules, no
`StateMsg` changes needed.

### Dead gray-barrel prop removed (`renderer/scenery.rs`)
Deleted `draw_barrel` and its dispatch arm — part of an unreachable 4th "Tropical" scenery
archetype (`draw_tropical`) never wired into the live `Theme` enum (Underground/Pastoral/Rugged
only), so it was inert dead code, not something actually seen in-game.

### Terrain relief compression — maps too vertical to play (`wa_templates.rs`, `terrain.rs`)
Generated maps had 100–290px cliffs/pillars; soldiers walk up only 8px, jump ~16px, backflip
~46px, so valley soldiers were stranded below and couldn't reach or attack tops. Three
deterministic levers (applied to island **and** cavern maps):
- **Relief compression** (`wa_templates.rs`) — mask sampled "zoomed out" 2× around a mid-band
  anchor (`RELIEF_COMPRESSION=2.0`, `ANCHOR=0.55`); `sample_segment` now returns air above the
  mask / solid below (both map types) instead of clamping edge rows into vertical streaks.
- **Depth ramp** (`terrain.rs`) — solidness grows with depth (`GROUND_T=0.62`, `DEPTH_RAMP=9.0`):
  every column gets connected ground near the waterline, high floating chunks melt into air,
  the whole height band collapses. Kills marooned pillars and floating islands.
- **Per-column cave crust** (`terrain.rs`) — cave-punch/dilation now keep `CRUST_PX=44` of rock
  below each column's *actual* surface. The old fixed-band crust (`ty<=0.18`) assumed terrain
  reached the top of the band, which the compressed profile no longer does, so it protected
  nothing and hollowed out ground under soldiers.
- Overhang-shelf air gap capped 30–44px (was 30–75px) so ledges stay backflip-reachable.
- **Result:** island p95 cliff height 126–287px → **28–57px** across 12 seeds; silhouettes still
  read as WA collages. New guard test `island_relief_is_traversable` in `wa_collage_check.rs`
  (≤8% columns >60px, ≥80% columns have ground). Intentional chasms/pits still exceed 60px.
- Changes maps for existing seeds → in-progress TAT matches will regenerate/desync (accepted);
  that is why `VERSION`/`REQUIRED_VERSION` are bumped so old clients hard-reject.

### Terrain generation parallelized — slow match start in every mode (`terrain.rs`)
`generate_tactical` ran 5 OpenSimplex + collage evals per pixel single-threaded (~260ms desktop,
several seconds on the Miyoo's Cortex-A7) — the freeze between selecting a mode and the match
appearing. Density-field fill and both box-blur passes (island + cavern branches) now run across
all cores via scoped threads over disjoint row chunks. Pure per-pixel math → **bit-identical
output** (verified by full-bitmap hash over 10 seeds vs. serial), so no desync risk and no map
change on its own. 260ms → 60–90ms desktop; ~2× on Miyoo (2 cores), ~3× on Pi (4 cores).

### TAT / DB latency — API server (`deploy/arty_api.py`, DEPLOYED to Pi 2026-07-05)
TAT game list, test-match start, and all DB reads were slow. Root causes fixed:
- One shared SQLite connection serialized every request → per-request connection via new
  `open_db()` (WAL + `synchronous=NORMAL` + 5s busy timeout); reads now run in parallel.
- Zero indexes → added 11 (`users(token)`, `matches(p0/p1,done)`, `rosters(user_id)`, pool
  tables, cosmetics, challenges) — created on startup via `CREATE INDEX IF NOT EXISTS`.
- `/matches/pending` N+1 (two user queries per match) → single LEFT JOIN.
- `/match/create` committed 3× per ranked pairing → one transaction per path.
- Deployed + verified live (indexes present, `/matches/pending` ~5–10ms). DB backed up first
  (`arty.db.bak-20260705`). Python-only, no protocol/version change.

### Bug reporter follow-up (client + API)
- **Screenshot dimming bug** — `BugReporter::draw()` re-dimmed the *live* `WorldBuffer` every
  tick (`px/4` on already-halved pixels) instead of the pristine `self.screenshot` capture —
  crushed to black in 1-2 ticks. Now reads from `self.screenshot` every frame.
- **Bug reports never reached Discord** — `arty_api.py`'s `/notify/bug_report` call was missing
  `?key=<ADMIN_KEY>` → silent `403`. Fixed (shipped with the API deploy above).
- **"Report sent" screen lingered 12s** → 2.5s success / 5s failure.

- Gates: `cargo check --tests` clean, parity 22/22, `wa_collage_check` 6/6, `py_compile` clean.

## Deployed 2026-07-05 (v0.5.4.416)
- **Bug reporter camera fix** — `capture()`/`draw()` and the shared `Keyboard::draw()` only
  accounted for `cam.left_edge()`, never `cam.top_edge()`. If the camera was scrolled
  vertically when Menu was pressed, the reporter UI drew at world-rows `[0,SCREEN_H)` while
  `blit_to_fb` displayed `[cam_top,+SCREEN_H)` — "opens on top half of screen only". Fixed
  across `bug_report.rs`, `keyboard.rs`, and the 6 call sites in `account.rs`/`lobby.rs`.
- **Invisible-object render bug** (real, user-reported "detonated invisible mine") —
  `render_my_team`'s viewport culling for mines/barrels/crates/fire-patches compared raw
  world-Y against `SCREEN_H` with no `cam_y` offset (graves/soldiers/explosions already did
  this correctly). Anything in the lower half of a tall map never rendered once the camera
  scrolled down to actually look at that area, while remaining fully solid/live in the sim.
  Fixed all 4 call sites in `loop_runner.rs`.
- **Fall damage retuned** — safe threshold 80px→130px, rate 0.15/px→0.10/px.
- **Scenery hitboxes tightened** for 11 round/irregular sprite types (rocks, bushes, piles,
  boulders, crystals, skulls, cairns) toward their visual core — a rectangular box around an
  irregular sprite always overhangs the corners more than it does for blocky sprites
  (posts/crates/walls/logs), which already fit well. Full per-pixel masks would be the
  complete fix (`SceneryObject.mask` is currently only populated by crater carving, never at
  spawn) — out of scope this pass.

## Deployed 2026-07-05 (v0.5.4.415)
- **TNT and Baseball Bat turn-locks removed.** Bat's "locked 3 full cycles" was already
  dead/unenforced (comment only); TNT's real gate (`turn_number >= 5*num_teams`, plus a
  padlock icon overlay) removed from `loop_runner.rs` and `cpu.rs` (the AI obeyed the same
  gate). Guide text updated.
- **Spawn clumping fix** — `find_team_spawns`'s last-resort fallback was a first-fit scan
  over candidates in x-ascending order, which clumps the whole team into the first usable
  cluster of columns on a badly fragmented map (repro seed `18bf66258fd61523`: only 4 raw
  landform segments total, zero ≥60px wide) even when an isolated usable column exists far
  away. Rewritten to greedily maximize worst-case separation (matching `sep_ok`'s OR rule as
  a continuous score, integer math only) — verified it now grabs the distant outlier first.
- **TAT Menu-freeze fix** — the Menu→bug-reporter `MenuGuard` was only wired into `main()`'s
  loop; TAT's interactive turn loop (`run_tat_game`) never disabled keymon's MENU
  interception, so pressing Menu during a TAT turn let the OS's own menu silently eat the
  button (looked like a freeze, no report screen ever shown). Hoisted `MenuGuard` to module
  scope, wired into `run_tat_game` too.
- **Live-match weapon-kill sync** — `kill_weapon` was never synced to the live client
  (`NetSoldier` had no field for it; the parity checklist claimed it "arrives as a message",
  which never actually happened). Every live-match kill reported weapon "UNKNOWN" for
  missions/leaderboards even though the field was set correctly server-side; TAT/hotseat were
  unaffected (real local simulation). Added `NetSoldier.kill_weapon_u8` (255=None) via the
  existing `WeaponKind::to/from_net_u8` helpers, wired through `build_state`/
  `apply_server_state`, both parity checklists updated.
- Found while wiring weapon-kill missions: **Shotgun's hitscan path never set
  `kill_weapon`**; **`BaseballBat` had no `display_name()` entry** (fell through to generic
  "WEAPON"). Both fixed.
- **Missions pool expanded** — `DAILY_POOL`/`WEEKLY_POOL` grew from 3 fixed challenges each
  to 16/17 candidates (`deploy/arty_api.py`), including per-weapon kill challenges. 3 are
  deterministically selected per day/week (seeded by the period string via `hashlib.md5` →
  `random.Random`) so every player sees the identical rotating set on a given day, but it
  changes day-to-day/week-to-week. Hand of Jerry weekly mission swapped for Clump Bomb (HoJ
  is a ~3% crate drop — a 2-kill weekly target was unattainable).

## Deployed 2026-07-05 (v0.5.4.414)
- **Sacred Ordnance max damage** 100 → 80.
- **HOW TO PLAY guide overhaul** — update-screen text bumped to scale 2 (was unscaled),
  capped at last 5 patch notes; corrected stale numbers (TNT 112→75, Air Strike 75→50,
  Blasthive 12→5/sting, Hand of Jerry 85→45/bounce, Grapple Hook "3 uses"→5, Shotgun's
  now-single-ray falloff mechanic, a Revolver stat contradiction between two guide pages);
  added entries for 5 previously-undocumented loadout weapons (Pistol, MAC-10, Molotov
  Cocktail, Clump Bomb, Homing Missile).

## Deployed 2026-07-05 (v0.5.4.413)
- **Cavern-map barrel fix** — `place_map_barrels` used raw `surface_y_at_with_scenery`,
  landing barrels on the sealed rock cap on cavern maps instead of inside an accessible
  chamber — same bug `maybe_drop_crate` already worked around via
  `standable_cave_foot_simple`. Fixed to match.
- **Server hardening**, found while load-testing (20 synchronous matches, then a 10-min
  trickle-in/hold/trickle-out test): new `[profile.server]` (`panic=unwind` instead of
  `release`'s `panic=abort`) so one match panicking can't abort the whole process and take
  every other in-progress match down with it; each match-running thread wrapped in
  `catch_unwind`. TLS+app handshake moved off the single accept-loop thread into a
  per-connection thread so a burst of simultaneous connects parallelizes handshake cost
  across cores instead of serializing behind each other. Server match-start log now prints
  each player's account/character name per team, not just team indices.
- **Deploy script bug** (self-inflicted, found + fixed same day) — `update_server.sh`
  hardcoded the server binary path to the old release-profile output dir; after the
  `piserver` alias switched to `--profile server`, every deploy was silently repushing a
  stale `.412` binary (still `panic=abort`, still `REQUIRED_VERSION 0.5.4.412`) regardless of
  the requested version. Confirmed live via a raw protocol test (a `.412` handshake got `OK`
  from a server claiming to be `.413`). Fixed path, re-verified with the same test.
- **Shotgun damage falloff (WA gun-blast model)** — `fire_shotgun` in
  `loop_runner.rs` no longer deals a flat 25 to whichever worm the ray's bounding
  box crossed (and nothing when it stopped on terrain a pixel short). The single
  hitscan ray still stops at the first surface (terrain / worm / barrel / crate),
  but the impact point now spawns a small gun blast: each worm takes damage by how
  close its body centre is to that point — full 25 inside a `CORE_R=18` plateau
  (a clean body hit is unchanged), linear falloff to 0 at `MAX_R=40`. So a graze
  or a shot that clips ground beside a worm lands for under 25, and a near-miss
  onto terrain next to a worm now splashes it instead of doing nothing. Knockback
  scales with the same falloff. Purely inside the sim (all 5 paths share
  `fire_shotgun`); no new `StateMsg` fields, parity 22/22.

## Built 2026-07-04 (v0.5.4.412, pending deploy)
- **Live damage popups always visible** — `damage_focus` (x, y, ticks_left of the most
  recently damaged soldier) is now synced in `StateMsg` (was server-only bookkeeping);
  the live client's camera holds on the damaged soldier during the Retreating phase
  (mirroring what hotseat's `update_camera` already did), so the popup + HP countdown
  actually land on screen instead of playing out off-camera.
- **Damage-popup colours fixed** — `FxEvent::DamagePopup`/`FxDamageText` now carry
  `color_id` (the lobby-picked `TEAM_COLOURS` index) instead of raw team index, so the
  floating number matches the victim's HP box in 4-colour live casual.
- **Client start barrier** — after `show_match_intro`, the live client sends ready
  `InputMsg`s and holds on a "WAITING FOR PLAYERS..." screen until the server's state
  reaches `tick >= 1` (35s deadline, longer than the server's 30s gate). Both players
  now enter the arena and see the turn timer start on the same server tick;
  reconnects fall through immediately since the match is already ticking.

## Deployed 2026-07-04 (v0.5.4.411)
- **Chain-reaction damage tallies** — `flush_damage_tallies` now holds all popups
  while the world is "hot" (projectiles/explosions/pending deaths/black holes/Garcia/
  airstrike/triggered mines+barrels/airborne soldiers in flight); the settle window
  freezes during the hold so a mine/barrel cascade or a full burn shows one total
  instead of a flurry of separate numbers. Shotgun is exempt (still one popup per
  shot). Soldiers on fire keep tallying until the burn ends, then the total pops.
- **Plasma torch bore widened** r15→r17 (34px) — the walk-through check needs 29px
  over the full 14px body width; the old 30px bore left only a 1px margin and could
  wedge soldiers inside the tunnel.
- **Live match-start ready gate** (ranked + casual) — the server now holds tick 0 and
  broadcasts a frozen state until every client has sent its first `InputMsg` (terrain
  built + intro finished); `START_READY_TIMEOUT=30s` caps the wait. Nobody misses the
  start of turn 1 anymore.

## Deployed 2026-07-04 (v0.5.4.409–410)
- **Damage tally** — hits landing within ~0.7s of each other aggregate into ONE popup
  (`Soldier::pending_damage`/`damage_settle`, `flush_damage_tallies` in
  `loop_runner.rs`); the HP counter holds for 2s after the popup
  (`hp_countdown_delay`, synced via `NetSoldier`) before draining 1/tick. Shotgun
  collapses the settle window per shot, so it still shows one number per shell.
- **Parity enforcement overhaul** — nested compile-time checklists in `net_sync.rs`
  for every gameplay struct; exhaustive `*Snap` destructures in `tests/parity.rs`
  (caught and fixed `death_cause` not being applied client-side); fx spawn functions
  made private to `fx.rs`; `no_channel_bypass_in_source` guard test; pre-commit
  `cargo check --tests` gate (`SKIP_COMPILE_CHECK=1` escape hatch);
  `.github/workflows/parity.yml` repeats the gate + full parity suite in CI.
- **Spawn vertical dispersion** — `standable_foot_levels` (up to 3 levels per column,
  relaxed headroom below the topmost), greedy min-vertical-distance landform
  selection, `MIN_SEP_V=120` vertical separation between spawns. Emergency spawn
  mounds removed entirely — `find_team_spawns` never mutates terrain anymore;
  constraints relax through a fallback chain instead. Shotgun reworked to one
  precise hitscan ray per trigger pull (up to 25 damage, 50 for both shots) — the
  pellet scatter at the impact point is purely cosmetic. New tests:
  `spawns_disperse_vertically`, `print_spawn_dist` (ignored helper).

## Deployed 2026-07-04 (v0.5.4.408)
- **Damage-number popups** — getting hit now pops a floating "-N" over the soldier's
  HP counter (Worms Armageddon style): spawns right at the counter
  (`FxDamageText`/`FxEvent::DamagePopup` in `src/renderer/fx.rs`), rises and
  decelerates (`step_fx_text`), fades from bright to dim red, and despawns after
  ~45 ticks. Spawned via `game.emit_fx(...)` at every real damage site in
  `src/game/loop_runner.rs` (explosions, bat, hitscan weapons, fall damage) —
  skips the instant-death `take_damage(999)` calls (drowning/map-edge), which
  aren't meaningful HP deltas. Rides the existing `FxEvent`/`fx_events` channel
  so it auto-replicates to live clients with no `StateMsg` changes; `fx_text`
  classified not-networked in the `net_sync.rs`/`tests/parity.rs` checklists,
  same as the existing particle `fx` field. The HP counter's own tick-down
  animation (`Soldier::displayed_hp`) already existed and needed no changes.
- **Scenery craters EXACTLY like terrain** — refines .406's all-or-nothing scenery removal
  into true per-pixel destruction. `SceneryObject` (`src/world/terrain.rs`) now carries
  `mask: Option<Vec<bool>>` — a lazy per-pixel intact/destroyed grid over its collision
  footprint box, in world-pixel (post-scale) resolution; `None` means fully intact, so an
  object untouched by any explosion (the common case) pays no allocation. `Crater::carve`
  (`src/world/crater.rs`) clears mask bits within the blast circle the same per-pixel rule
  it already uses on `terrain.solid`, and only drops the object once every tracked pixel is
  gone — a graze at the edge of a big tree now nicks the corner instead of vaporizing the
  whole tree. Both collision (`stamp_objects` in `src/game/loop_runner.rs`) and rendering
  (`renderer/scenery.rs`'s `Scaled` wrapper) honor the mask; rendering keeps the previous
  one-shot `fill_rect` fast path whenever `mask` is still `None` and only falls to per-pixel
  plotting for objects an explosion has actually clipped, so undamaged scenery (nearly all
  of it, at any moment) costs nothing extra. Still lives inside `Crater::carve` so every
  path (local sim, server, live client `crater_log` replay, both TAT loops) stays
  deterministic with no `StateMsg` changes; small carves (pistol/revolver/MAC-10 chips
  r≤4, plasma-torch nibbles) still leave scenery standing.
- **Backflip 10% higher** — Y-button backflip's initial vertical launch velocity
  (`src/game/loop_runner.rs`) raised from `-6.5` to `-6.82` (jump height scales with
  velocity², so a 10% height increase needs `v·√1.1`). Lives inside `simulate_with_muzzle`,
  so it's automatically correct across all 5 gameplay paths.

## Deployed 2026-07-03 (v0.5.4.406)
- **Scenery destruction (initial, all-or-nothing)** — explosions (crater radius ≥ 8)
  destroyed any scenery object whose collision footprint intersected the blast circle.
  Superseded by the per-pixel mask above (deployed 2026-07-04, v0.5.4.407).
- **Steep-angle bazooka self-detonation fixed** — the muzzle spawn point
  (`pos.y - 4 - sin(angle)*12`) sits inside the shooter's own hit box at high aim angles,
  and the rocket hit box (`dx<12, dy∈(-34,4)`) had no owner exclusion — a mid-charge
  near-vertical shot was still inside the shooter's box on its first collision check and
  detonated on the shooter. `Projectile` now carries `owner: Option<(team, soldier)>`
  (set by `fire_weapon`) and `cleared_owner`; `step_projectiles` ignores the owner as a
  target until the projectile has left their box once, after which the owner is a normal
  target (a shot falling back on your own head still hurts). Covered by
  `tests/fix_verification.rs`.
- **Faster match load** — three fixes to the ~630ms (desktop-release; several seconds on
  the Miyoo) match-start pipeline:
  1. `atlas_sample` (`src/renderer/draw_terrain.rs`) scanned pixel-by-pixel upward to find
     each solid pixel's landform top — O(depth) per pixel, quadratic per column on deep
     terrain, dominating `build_world_cache`. Now answered from the per-column
     `solid_runs` cache via new `Terrain::run_top()` (`src/world/terrain.rs`), with the
     old scan kept only as a fallback. `build_world_cache`: 235ms → 102ms desktop.
  2. Loop order fixed in `build_world_cache`/`update_cache_region`/`build_bg_cache` —
     they iterated x-outer over row-major buffers (7.7KB stride per write).
  3. `bg_image::prewarm_for_seed()` + a terrain-texture-tile warmup thread start decoding
     the background PNG and atlas tile as soon as the seed/terrain are known
     (`build_default_game_opts` in `src/main.rs`), overlapping the decode with map
     generation instead of stalling the first rendered frame.

## Recent changes (0.5.4.404–0.5.4.405)
- **WA-timed backflip (0.5.4.405)** — the soldier's backflip now follows the rotation curve
  of the real WA 22-frame backflip animation: `SPIN_CURVE[22]` in `src/renderer/skeleton.rs`
  holds per-frame principal-axis angles measured from the extracted frames (eased takeoff,
  fast mid-flip tumble, eased landing, 22-tick cycle), replacing the old linear 18-tick spin.
  The soldier keeps its own look (hat/gun/uniform visible through the flip).
- **SPR extraction tool (0.5.4.404)** — new `tools/extract_wa_sprite.py` parses sprite
  animations out of WA's packed `Gfx.dir` (format reverse-engineered and documented in the
  docstring: IMG-style palettized header, Team17-LZ77 chunks, per-frame placement rects).
  0.5.4.404 briefly shipped the actual worm sprite frames drawn during backflips; reverted
  in 0.5.4.405 to keep the soldier's appearance — the tool and the motion data remain.

## Recent changes (0.5.4.401–0.5.4.403)
- **Guaranteed-walkable spawns (0.5.4.403)** — `standable_foot_y`, `standable_cave_foot_y`,
  and `standable_cave_foot_simple` (`src/world/terrain.rs`) previously validated only the
  exact 3-column footprint (`x_l`, `x`, `x_r`), never checking that a soldier could take a
  single step out of it. A wall-to-wall `SOLDIER_W`-wide slot passed every footing check yet
  was provably unwalkable by `try_move_horizontal`. Added an escape-room check requiring at
  least one column two pixels beyond either footprint edge to be clear over full body height.
- **Mound-series terrain removed (0.5.4.403)** — a low-weight sine-relief (`hill_col`) was
  layered onto every non-cavern map's density field regardless of the underlying WA collage
  silhouette, producing a repeating "series of mounds" look on flatter maps. Disabled
  (`rolling = false`); the collage remains the sole macro shape.
- **Hand of Jerry water-splash fix (0.5.4.403)** — `step_garcia`'s fall collision fell back
  to `WORLD_H` (map bottom) when the column had no solid terrain (open water), so the hand
  free-fell far past the visible water line before `hit_ground` triggered the smash sound.
  Falls back to `WATER_Y` instead — the splash now registers right at the surface.
- **Embedded-scenery pruning (0.5.4.403)** — the emergency spawn-mound fallback in
  `find_team_spawns` raises a 141px-wide column of solid dirt (`MOUND_HW=70`) but only
  checked scenery clearance at its center point, so objects elsewhere in that span could get
  buried. Extended the existing post-mound pruning pass (previously only caught objects left
  floating) to also catch and remove objects left embedded.
- **Barrels/mines on top of scenery (0.5.4.402)** — new `Terrain::surface_y_at_with_scenery()`
  scans scenery footprints for the spawn column and returns the higher of terrain surface or
  scenery top; used in all 4 map-gen spawn functions so barrels/mines never spawn embedded.
- **Pistol audio pop fix (0.5.4.402)** — `pistol.wav` (708ms) was longer than the interval
  between burst shots (700ms), so the Miyoo's ALSA device was still draining the previous
  shot when the next tried to open it, producing an audible pop every other shot. Added
  `try_load_capped()` (`src/audio.rs`) to trim the loaded clip to 380ms with a clean fade-out.
- **Hand of Jerry camera fix (0.5.4.402)** — camera now follows the Hand of Jerry's targeting
  cursor vertically in every path (hotseat, live client, both TAT replay loops), matching Air
  Strike and Homing Missile, which already did.
- **Scenery footprint fixes (0.5.4.402)** — mushroom and skull both had a declared collision
  footprint taller than the actual drawn sprite (~6px invisible solid strip at the 3× scale
  tier); corrected to match the drawn art.
- **Pistol burst fix + balance (0.5.4.401)** — pistol was firing only 1 of its 5 shots: the
  shot counter was set to 4 *after* calling `fire_pistol_shot()` instead of before, so the
  function's own end-of-burst check saw a stale 0 and ended the turn immediately. MAC-10
  damage cut a further 20% (3→2/bullet — the 0.5.4.400 nerf only changed an unused display
  stat, not the real per-bullet constant). Meteor Bomb main explosion damage cut 20% (45→36).
- **Deployed 2026-07-03** to Pi server/API/dashboard/Windows OTA/GitHub build for all three
  versions; Miyoos were unreachable at each deploy and will pick up the OTA update on next boot.

## Recent changes (0.5.4.392–0.5.4.400)
- **Soldiers stand on other soldiers + MAC-10 nerf + update-check overhaul (0.5.4.400)** —
  new `is_on_soldier()` grounding check (a square landing on another soldier's head stands
  on it like a platform instead of always sliding off); MAC-10 `max_damage()` cut 40%
  (8→5/bullet, later found to be an unused display stat — see 0.5.4.401); update-check
  overhaul fixed the freeze on "CHECKING FOR UPDATES..." after choosing Casual Live (title
  screen UPDATE AVAILABLE banner, cancellable check gate, handshake off main thread).
- **Pistol 5-shot burst + big grounded scenery (0.5.4.399)** — pistol burst cut to
  5 shots total (1 immediate A-press + 4 auto-burst, was 6). Scenery objects now
  render 2–3× bigger (`SceneryObject::scale()`: small sprites 3×, tall ones 2×) via
  a `Scaled` wrapper in `scenery.rs` that magnifies all 40+ hand-authored 1× sprites
  about the object's bottom-center anchor; `footprint()` returns scaled dims so
  collision/stamping/placement match the visuals. Placement now requires solid ground
  under the central half of the footprint (bottom 5 rows may sink into a slope,
  central-¾ width air clearance above); cavern maps place scenery on standable cave
  floors instead of the sealed cap (Underground props finally appear); spawn-mound
  headroom carving prunes any object whose support it removes. `find_team_spawns`
  now also rejects candidates within footprint + 10px of any scenery object so
  soldiers never spawn inside the now-solid props.
- **Solid scenery + frame pacing (0.5.4.398)** — scenery objects became real obstacles:
  per-sprite collision footprints stamped into the per-tick object mask in
  `stamp_objects()` (soldiers stand on them, projectiles collide); placement rejects
  embedded spots. `TICK_DURATION` exactly 33,333µs + absolute-deadline pacing (sleep
  overshoot no longer compounds, ~28fps ceiling gone); `blit_row` NEON-vectorizable;
  TEST overlay shows per-section µs + prev-frame blit cost. Also resolved the
  "only CAVERNS in test mode" report: the code was verified correct on x86/qemu-armv7/
  on-device — the stale shipped .396 Miyoo binary was at fault; confirmed fixed on .398.
- **Pistol SFX + generation speed (0.5.4.397)** — `pistol.wav` was a 48-second
  recording with one bang at t=9.3s: each shot's playback thread held the Miyoo's
  exclusive `hw:0,0` for the whole clip, so rapid-fire shots queued into the next
  turn, and the first-sound lazy decode (9.2MB → 48s×48kHz resample on the game
  thread) stalled match start. Trimmed to the 0.5s bang (`assets/sfx/` +
  `deploy/assets/sfx/`), `play_pistol` now uses the skip-if-busy path. Collage
  generation: domain warp precomputed on a 512×128 bilinear grid + single-lookup
  fast path away from seams — caverns now generate faster than the pre-collage
  generator. New guard test `tests/soldier_visibility.rs` renders every spawned
  soldier and asserts it produces pixels.
- **Seed-based WA collage map generation (0.5.4.396)** — every seed now composes a
  novel silhouette by splicing 2–4 horizontal segments of real WA terrain art
  (per-segment mask/shift/mirror/y-transform, 60–120px crossfades, low-freq domain
  warp; coverage check rejects all-ocean windows). Cavern maps carve chambers from
  the same collage *inverted* — the old procedural fill+carve cavern generator is
  gone. Theme dispatch centralized in `Theme::of(is_cavern, template_id)`
  (template_id = dominant segment's mask). New `tools/extract_wa_mask.py` extracts
  masks from WA `land.dat` (Team17-LZ77, collision-mask chunk) or PNG; masks renamed
  `island0/island1.bin`. Guard tests in `tests/wa_collage_check.rs`.
- **Invisible soldiers fixed (0.5.4.395)** — `render_my_team()`'s vertical draw-culling
  for soldiers, headstones, projectiles, and explosions checked world-y against a
  hardcoded `0..480` window instead of the camera-scrolled viewport (`cam_y..cam_y+480`).
  Anything spawned below world-y 480 (bottom half of the 960px world) was silently
  skipped from rendering entirely — never drawn, not occluded. Became far more visible
  after the WA-terrain rewrite since spawns (especially cave floors) land deeper more
  often. Fixed all four cull sites to use `cam_y`.
- **Aim rotating during camera pan fixed (0.5.4.394)** — `process_aim()` only excluded
  `L1` from Up/Down aim adjustment, but both `L1` and `R1` pan the camera vertically.
  Holding `R1`+Up/Down to scroll was also rotating the aim angle. Both modifiers now
  excluded.
- **Archetype system removed (0.5.4.393)** — the old 5-way landform archetype
  (hills/cliffs/islands/caverns/canyon) is gone. Every map is now either sourced from
  one of 2 real WA terrain masks (`template_id: 0|1`) or an occasional carved-cavern
  map (`is_cavern: bool`, ~20% odds). Chasms, overhang shelves, and cave carving are
  now seed-random on any non-cavern map instead of gated by archetype, so any
  combination can occur. Cosmetic dispatch (sky/debris/dirt tint/scenery theme)
  collapsed to a 3-way split: cavern look, WA template 0, WA template 1.
- **WA-sourced terrain silhouettes (0.5.4.392)** — terrain macro-shape now comes from
  two real Worms Armageddon terrain bitmaps, extracted directly from the original
  game's `land.dat` (1-bit-per-pixel, 1920×696, found via reverse-engineering the
  file format). Baked as Rust constants (`src/world/wa_templates.rs`,
  `include_bytes!`) — no runtime file I/O, fully deterministic per seed (seed picks
  mask, x-shift, and mirror). Maps read as authentically WA-styled instead of
  purely-synthetic noise silhouettes.

## Recent changes (0.5.4.391)
- **15 new scenery sprites** — five new draw functions per archetype added to
  `src/renderer/scenery.rs`:
  pastoral (0): sunflower, log, pebble cluster;
  rugged (1): broken wall, lichen rock, cairn;
  tropical (2): coconut, driftwood, crab trap;
  underground (3): stalactite shard, rusted chain, ribcage;
  arid (4): dry shrub, sun-bleached log, horns.
  Sprite counts raised from [5,4,4,4,4] to [8,7,7,7,7]; 28 objects per map (was 12);
  MIN_SPACING reduced 240→110px to allow density at the larger count.
- **Flat plains sub-variant** — archetype 0 (hills) has a new WA Rocky/Desert-style
  sub-variant: high threshold, near-zero fade, high contrast → mostly flat terrain
  with scattered small bumps. Roughly 25% of hill maps now generate as flat plains.
- **Per-archetype hill amplitude** — cliffs and canyon now use HILL_AMP 0.08/0.10
  (was shared 0.24), so ridged noise and terracing define those archetypes rather
  than rolling humps appearing on every map.
- **Stronger canyon/cliff variety** — canyon terrace_mix floors raised (slot canyon
  0.50, badlands 0.72, fortress 0.82); cliff warp_amp minimum raised to 0.40.
- **Barrels 14–20, mines 16–24** — scaled up for the 1920×960 world (was 7–11 /
  9–15). Both client and server placement functions updated identically.
- **Pistol sound fix** — first shot in each burst now emits sound on the A-press
  frame. Previously the burst-fire block ran before the A-press handler in the same
  tick, delaying the first sound by one frame.

## Recent changes (0.5.4.390)
- **Scenery objects** — pixel-art decorations placed on every map (12 per map,
  MIN_SPACING=240px), drawn behind terrain from `src/renderer/scenery.rs`.
  Five themed sprite variants per archetype. Seed-derived and cosmetic only.
- **Weapon menu wider cells** — cell width 92→120px for better name readability.
  5-column layout retained at this version.
- **HOMING MSL rename** — weapon displays as "HOMING MSL" (was truncated
  "HOMING MISSILE").
- **Pistol immediate first shot** — first shot fires on the A-press frame
  (partial fix; sound still one frame late — fully corrected in 0.5.4.391).

## Recent changes (0.5.4.389)
- **HUD screen-anchored** — HUD bar, FPS counter, avatar box, weapon indicator, weapon menu,
  test-mode seed/pixel-stats display, and pause overlay all track `cam_y` and stay at their
  correct screen positions when the camera is scrolled vertically.
- **R1+Up/Down vertical pan** — pans the camera vertically with snap-back to the active
  soldier on release (same as R1+Left/Right for horizontal). L1+Up/Down continues to pan
  without snap-back.
- **Cursor vertical range** — Garcia, Air Strike, and Hand of Jerry cursors can now move
  down to the waterline (was hard-capped at y=400); matches Homing Missile behaviour.
- **Vertical spawn spread** — soldiers spawn at varied heights across all archetypes. Maps
  with punched caves (hills, cliffs, islands, canyon) pre-scan for cave floors and reserve
  half the spawn slots for underground positions, so teams start on ledges, in tunnels, and
  at mid-terrain rather than all on the topmost surface layer.
- **Cave generation speed** — Moore dilation passes reduced 4→2; test-mode entry and server
  game startup are ~2× faster on cave-heavy maps (allocation ~3.7 MB instead of ~7.4 MB).

## Recent changes (0.5.4.388)
- **WA-style terrain generation** — tuned `generate_tactical` to produce maps resembling
  Worms Armageddon's hand-crafted levels: sharper silhouettes (box-blur radius 10→5),
  much more cave coverage (hills 30%→65%, cliffs 35%→70%, canyon 0%→50%), wider cave
  passages (`cave_thresh` 0.12→0.16, noise frequency 7.0/6.0→5.5/5.0 for larger structures),
  and more/wider-spread chasms (2–4 central-only → 3–5 across 22–78% of map width).
- **Mine max damage** — landmine max damage increased from 35→50.

## Recent changes (0.5.4.387)
- **Vertical maps and scrolling** — world height doubled to 960px (2× the 480px viewport).
  Maps now use the full height: terrain spans 700px vertically (`TERRAIN_MIN_Y=80`,
  `TERRAIN_MAX_Y=780`), peaks can reach near the top of the world, and caves/canyons run
  much deeper. Camera gains a Y axis — it follows the active soldier vertically and
  `L1+Up/Down` pans vertically during a turn (same as horizontal `L1+Left/Right`).
- **Generator tuned for taller world** — `SKY_BAND` reduced 0.30→0.12 so terrain reaches
  higher instead of being forced below y≈290. Cave archetype `SKY_FLOOR`/`CAVE_FLOOR` now
  derived proportionally from `TERRAIN_MIN_Y`/`WATER_Y` (were hardcoded 130/345 from the old
  480px world — cave maps were only using ~215px of the available 760px).
- **Background rendering fixed** — background images are now viewport-relative (screen Y),
  so they always fill the screen correctly when scrolled down. Previously they read empty
  cache rows (black) for any world Y above 480.
- **Deploy/Discord fix** — `update_server.sh` now reads the admin key from the Pi and
  passes `?key=` on the `/notify/patch` call; patch notes were silently 403ing on every deploy.
- **Windows backup silent** — Task Scheduler now launches backup via `wscript.exe` +
  `run_backup.vbs` (window style 0) instead of the self-relaunch trick; no console flash.

## Recent changes (0.5.4.385–0.5.4.386)
- **Smooth, organic terrain (0.5.4.385 → .386)** — `generate_tactical` now box-blurs the
  continuous density field before thresholding, so hills/cliffs/islands/canyons render as
  rounded organic blobs instead of jagged ridges. `.385` was conservative (blur radius 3);
  `.386` is much rounder (radius 10, islands 6; fine relief octave removed, ×4 halved,
  threshold lowered to offset blur shrinkage). Chasms and overhang shelves stay sharp;
  caverns (archetype 3) unchanged. Deterministic — client/server rebuild identical terrain
  from the seed, so no `StateMsg`/parity change. Tradeoff: gentler slopes make more terrain
  walk-up-able (fine jump-forcing relief mostly gone), accepted for the look.
- **Legacy generator removed (0.5.4.386)** — deleted the unused `generate_worms` (old
  full-Perlin jagged generator). `generate_tactical` is now the only generator reachable
  in-game; `from_heightmap` remains for `#[cfg(test)]` fixtures only.

## Recent changes (0.5.4.382–0.5.4.384)
- **Bot dashboard auth (0.5.4.384)** — admin key login screen added to bot.html (localStorage,
  same key as other dashboards). All `/notify/*` endpoints in `mayhem_bot.py` gated by
  `?key=` check (403 on bad key).
- **IRC channel creation locked (0.5.4.383)** — `PredefChannelsOnly = yes` in ngircd.conf;
  only opers can create new channels. Predefined channels: #lobby, #general, #dicerpg, #zpg.
- **Miyoo framebuffer blit perf (0.5.4.383)** — all 480 row-reversals batched into a single
  heap `flip_buf`; flushed to mmap in one copy instead of 480 separate writes.
- **OTA update check (0.5.4.383)** — HTTPS first (2s timeout), HTTP fallback, LAN fallback
  for hairpin NAT. Test mode no longer blocks on update check at entry (instant).
- **Infrastructure hardening (2026-06-28)** — firewall (iptables) restricts Pi port 9000
  (TheLounge) to LAN + home + VPN IPs; port 22 LAN-only on Pi, laptop, and Windows PC.
  SSH key auth configured on laptop (arty_pi). ESTABLISHED,RELATED INPUT rule added to
  dusty/10.0.0.45 so outbound SSH return traffic is accepted.
- **Windows backup (2026-06-28)** — `deploy/backup_arty.ps1` runs on Windows via Task
  Scheduler (hourly), pulls from dusty via scp; SHA256 dedup, keeps 5 most recent.
- **Windows backup key auth fixed (2026-07-09)** — `ArtyBackup`'s scheduled (non-interactive)
  runs had no way to authenticate: the `arty-backup@build` key (`~/.ssh/id_ed25519_backup.pub`
  on dusty, used by the Windows Task Scheduler job) was missing from dusty's
  `~/.ssh/authorized_keys`, so key auth failed and sshd silently fell back to a password
  prompt with nobody there to answer it. Added the key to `authorized_keys`; confirmed via
  `/var/log/auth.log` that connections now succeed via publickey.
- **Pi4doom backup pull to Windows added (2026-07-09)** — new `deploy/backup_pi4doom.ps1` +
  `deploy/run_backup_pi4doom.vbs` mirror the `backup_arty.ps1`/`run_backup.vbs` pattern to
  pull the Pi SD-card image backup from the build machine to Windows. Pulls the newest
  `pi4doom-*.img.gz` from `dusty:/mnt/sdd1/backup/pi4doom/` (the pi4doom-backup systemd
  timer's own output — NOT from the Pi directly, avoiding a redundant second dd/gzip pass)
  to `D:\misc\backups\pi4doom`, skips if already pulled (tracked via `last.txt`), keeps 3
  most recent (~12GB each). Task Scheduler task `Pi4doomBackup`: `wscript.exe
  run_backup_pi4doom.vbs` → `powershell.exe -NonInteractive -WindowStyle Hidden`, every 3
  days at 22:15, matching the existing silent-launch pattern used by `ArtyBackup`. Verified
  via manual `schtasks /Run` — silent, `Last Result: 0`, file confirmed on disk.

## IRC / Bot changes (2026-07-02)
- **ZPG plugin overhaul** — the IRC zero-player game (`assets/zpg_plugin_code.txt`)
  gained several systems making tacos, classes, and factions mechanically
  meaningful instead of cosmetic:
  - **Class passives** — each of the 7 classes now has a real per-tick effect
    (XP%, item-find%, boss dmg%, starting luck, taco%, flat taco bonus, or
    world stability trickle). `!classinfo` lists them.
  - **Faction passives** — every faction grants its members a bonus even when
    not dominant (Packet Order +15% XP, Raccoon Syndicate 2x tacos, Chaos Null
    +10pp item find); dominant-faction world effects (Machine Spirit stability/
    boss-pressure, Chaos Null entropy) were strengthened and decoupled from the
    now-per-member Packet Order/Raccoon Syndicate effects. `!factioninfo` lists them.
  - **Taco shop** — `!shop` / `!buy <item>` spends tacos on luck, power, XP,
    world stability, a one-shot boss-damage bomb, or a rename token.
  - **Equipment rarities** — item finds roll Common/Rare/Epic/Legendary with
    weighted odds and bonus multipliers; shown in `!zpgstats` and event text.
  - **Stat-driven PvP** — replaced the flat random roll with attack/crit/dodge/
    armor mechanics driven by level, luck, and item_bonus; battle reports
    narrate crits, dodges, and armor absorption.
  - **Structured inventory** — items now belong to a slot (Weapon/Armor/
    Accessory/Relic); only the best item per slot counts toward power
    (auto-equipped, no manual command). `!zpgstats` shows the real per-slot
    loadout and a gear/shop power split instead of a flat item list.
  - **Prestige** — `!prestige` at level 50+ resets level/xp/luck for a
    permanent stacking +5% XP bonus per prestige; gear and tacos are kept.

## IRC / Bot changes (2026-07-02, world systems pass 2)
- **HP is back** — re-introduced (removed 2026-06-29) specifically so boss
  fights have stakes: `hp`/`max_hp` columns, passive regen each tick, boss
  retaliation damage against attackers, KO ("falls back") on 0 HP with a
  small taco penalty and a death-counter increment.
- **Boss actions** — `!attack` (default) / `!defend` (no dmg either way,
  +2 stability) / `!repair` (no dmg either way, +4 stability, -3 entropy),
  chosen per boss round via a `boss_actions` table.
- **World-choice votes** — `!vote a|b`; opens at ~3%/tick when none is
  active, resolves by majority on the *next* tick (no sub-tick timer
  exists), applying a stability+XP or entropy+tacos effect server-wide.
- **Seasons** — 30-day length; rollover crowns a title + Legendary "Golden
  Packet Analyzer" for the top player, records a `season_history` row,
  resets faction scores. `!seasons` shows history and time remaining.
- **Location flavor** — 6 named locations; ~40% of solo events now
  mention one instead of the generic pool.
- **Achievements** — 8 tracked (First Blood, Taco Hoarder, XP Grinder,
  Boss Slayer, Legendary Finder, Survivor, Chaos Loyalist, Machine Saint);
  `!achievements` lists earned ones.
- **Rare global events** — ~1%/tick, ×0.5 or ×2.0 server-wide XP for
  ~30 minutes, with expiry broadcasts.
- **NPC events** — ~3%/tick flavor events with a small shared effect
  (tacos/XP/stability/entropy) for every current player.
- **Daily quests** — 3 of 6 templates rotate in each day; `!quests` shows
  the board and completion status; 15 tacos + 30 XP per completion.
- **Hidden ultra-rare loot** — "The Root Password," a one-time
  server-wide 0.01%-per-roll drop granting +10 permanent luck and a
  maxed Legendary Relic.
- **Server milestones** — total boss kills tracked server-wide; crossing
  10/25/50/100/250/500/1000 broadcasts an event and grants +10 tacos to
  every current player.
- **Event pacing** — solo/location events, Common/Rare finds, lucky-break,
  and misfortune messages are now batched per tick and only up to 4 are
  broadcast (random sample), instead of flooding one line per player per
  tick. Level-ups, Epic+/Legendary finds, boss/PvP/world/vote/NPC/rare/
  faction events, achievements, quests, and milestones still broadcast
  immediately.
- **Bug fix** — `_faction_event` called `.get()` on a `sqlite3.Row` (no
  such method), silently aborting any tick that rolled that branch
  (caught by the outer try/except, so it never crashed the bot but also
  never broadcast faction flavor). Predates this session; fixed to use
  `'faction' in p.keys()`.
- **Verification** — no live Supybot/IRC harness available, so the
  plugin.py section was stubbed-imported directly (fake `supybot.*`
  modules) and driven through 400 simulated ticks with 4 players (0
  exceptions), vote open/resolve counts confirmed balanced, and a
  forced 31-day-old `season_start_ts` was used to exercise the full
  season-rollover path end-to-end.
- **Deployed live** — pushed to `benbot.service` (Limnoria/Supybot,
  `~/irc-bots/TriviaBot/Trivia_Bot.conf`) on the Pi (Grunkus@10.0.0.123).
  Live plugin path: `~/irc-bots/TriviaBot/plugins/ZPG/`. Pre-deploy
  `__init__.py`/`config.py`/`plugin.py` backed up to a sibling
  `ZPG_backup_<timestamp>/` dir first; `assets/zpg_plugin_code.txt` split
  back into the 3 real files via its `===== ZPG/*.py =====` markers,
  `py_compile`-checked on the Pi, stale `__pycache__` cleared, service
  restarted. Confirming the first live 300s world tick runs clean.

## IRC / Bot changes (2026-06-29)
- **ZPG overhaul** — HP system removed (was cosmetic-only, never decreased). Random event
  volume massively increased: 47 SOLO_EVENTS (was 13), 25 WORLD_EVENTS (was 10), 2 new
  bosses, 8 new items, lucky break event (+XP, 5%), misfortune event (-XP, 3%), faction
  rivalry event (3%), 4 PvP message variants.
- **Windows backup silent** — `backup_arty.ps1` self-relaunches with `-WindowStyle Hidden`
  so Task Scheduler no longer pops a PowerShell console window (superseded in .387 by VBScript wrapper).

## Recent changes (0.5.4.366–0.5.4.371)
- **Live play unresponsiveness fix (0.5.4.366)** — background reader thread held the
  stream mutex during blocking `read()` calls (up to 33ms), so `conn.send()` in the main
  render loop was blocked for up to a full server tick every frame. Fixed by setting a 5ms
  read timeout on the reader thread so the mutex is released frequently. InputMsg sends now
  block at most ~5ms.
- **Cave crate drops (0.5.4.366)** — crates now land inside cave boundaries using
  `standable_cave_foot_simple` (platform + roof check, no BFS) to find the cave floor, then
  spawn just below the ceiling above it. Eliminates BFS stalls on server tick and unreachable
  ceiling drops.
- **API fixes (0.5.4.366)** — stats mode parameter was re-parsed from the already-stripped
  path (always empty), defaulting to "tat"; fixed to read from `qs_params`. Duplicate match
  rows guarded with `if is_win:` check.
- **Pistol (0.5.4.371)** — new weapon; infinite ammo; 6-shot burst with ~0.75s between
  shots; 5 dmg/shot hitscan. Fast-firing alternative to Bazooka/Revolver. `WeaponKind`
  net u8=27.
- **Molotov 2 per loadout (0.5.4.371)** — starting loadout grants 2 Molotov uses (was 1).
- **MAC-10 infinite ammo (0.5.4.371)** — MAC-10 is now always available with infinite ammo;
  removed from crate pool (was a 2-use crate weapon).

## Recent changes (0.5.4.367–0.5.4.370)
- **Molotov Cocktail (0.5.4.367)** — new throwable weapon; wind-affected; shatters on
  impact and spawns 7–10 fire patches in a wide arc (-144°..+144°); infinite ammo;
  bottle-with-rag pixel sprite velocity-aligned in flight.
- **Loadout changes (0.5.4.367)** — Shotgun and Ninja Rope are now infinite ammo; MAC-10
  (`WeaponKind::Uzi`) added to starting loadout with 2 uses.
- **Terrain variation (0.5.4.367)** — more dramatic hills/valleys: amplitude 0.528→0.62,
  ridges octave 0.30→0.35, roughness 0.10→0.15; valleys 2–4 (was 1–2), depth 0.40–0.60;
  hill bumps 6–12 (was 4–8); `TERRAIN_MIN_Y` 160→140 for taller peaks.
- **Update check before splash (0.5.4.367)** — update result consumed before `draw_splash`;
  splash shortened 5s→3s.
- **IRC Dashboard (0.5.4.368)** — new page at https://crumbonium.duckdns.org/ircdash/
  (own URL, separate from /arty/). Reads BenBot's ChannelLogger logs every 3s — no bot
  connection needed. Shows live user lists + chat feed for all 4 channels; ZPG and
  DiceRPG leaderboards from SQLite. `irc_dash.py` serves JSON on port 7781; nginx proxies
  `/irc/`. Systemd user service `irc-dash.service`. Retains last-known state on disconnect.
- **Discord dedup (0.5.4.368)** — `mayhem_bot.py` records posted versions to
  `posted_versions.txt`; refuses to re-post the same version. Version-specific search
  added so changelog rotation is no longer needed to trigger notifications.
- **WA-style fire physics (0.5.4.369)** — grounded flames now slide downhill one pixel
  per tick (down → down-left → down-right) and collect in pits, matching Worms Armageddon
  petrol bomb behaviour. Airborne fire has strong wind drift + heavy bounce damping (0.25x).
  Soldiers inside fire get a lateral push nudge. `on_fire_ticks` synced to live clients
  via `NetSoldier` so squirm animation now shows in multiplayer.
- **All-modes parity (0.5.4.370)** — `aim_angle` now passed directly through `server_tick`
  so Up/Down reach cursor-phase weapons (homing missile, airstrike, any future weapon)
  without special-casing in server preprocessing. Parity test suite covers all 5 execution
  paths via `assert_all_paths_in_sync`. New cursor-phase weapons are automatic.

## Recent changes (0.5.4.361–0.5.4.364)
- **Account login screen cursor (0.5.4.361)** — replaced fixed A=Login / Y=Register
  button hints with an Up/Down cursor menu; A confirms selection. Uses
  `draw_menu_selection` highlight.
- **Windows OTA auto-update (0.5.4.362)** — `updater.rs` split into Linux and Windows
  paths. Windows: fetches `/arty/version.txt`, downloads `mini-mayhem.exe` to `%TEMP%`,
  writes a `.bat` swap-and-relaunch (can't replace running .exe on Windows), spawns
  `cmd /C start /B`, then exits. `deploy/update_server.sh` now SCPs Windows exe to Pi.
- **MP update gate freeze fix (0.5.4.362 + 0.5.4.364)** — after OTA, sentinel →
  `skip_update=true` → `update_rx` channel Disconnected → was always hitting the 1.5s
  fallback check on every MP entry. Fixed by `&& !skip_update` guard on the gate.
- **Airborne knockback removed (0.5.4.363)** — soldiers landing on others now slide
  off sideways without launching the target (velocity-proportional launch removed).
- **Landmine size up (0.5.4.363)** — blast radius 43→55, proximity trigger 25→35.
- **Horn hat position fixed (0.5.4.363)** — dy 8→5 (raised 3px); dx now
  facing-aware (-2 right / +2 left) instead of fixed -1.
- **Admin dashboard API caching (0.5.4.364)** — three background cache threads in
  `arty_api.py` (status 30s, temp 5s, lobbies 5s). Handlers return cached dict instantly.
  SQLite WAL mode + timeout=2 on background connections eliminates live-mode latency.
- **IRC #lobby AutoJoin (0.5.4.364)** — ngircd `[Channel]` block `AutoJoin=yes` puts
  all IRC clients in #lobby on connect.
- **Dashboard polish (0.5.4.364)** — log scrolls to top on first open; temperature card
  always shown; sys-stats poll 5s→2s.
- **Terrain height variation** — more dramatic hills and valleys: amplitude scale
  0.528→0.62, o2 (ridges) weight 0.30→0.35, o3 (roughness) 0.10→0.15; valleys
  increased to 2-4 (was 1-2), depth 0.40-0.60 (was fixed 0.45); hill bumps 6-12
  (was 4-8), wider range; `TERRAIN_MIN_Y` 160→140 for taller peaks.
- **Update check before splash** — update thread result is consumed before the
  splash screen renders (was after 5s splash). Splash shortened to 3s. Preload
  threads still run in parallel during the update-check wait.
- **Loadout changes** — Shotgun and Ninja Rope are now infinite ammo; MAC-10 added
  to starting loadout with 2 uses (`WeaponKind::Uzi`).
- **IRC ChanServ channel registration** — all four channels (#lobby, #General-Chat,
  #dicerpg, #zpg) registered with Anope ChanServ; Crumbo auto-opped on all channels
  via `FLAGS` +AOPf. Workaround required temporary Services Root oper block so
  CrumboBot could use `OperServ MODE` to self-op before registering, then reverted.
- **Trivia bot (BenBot) improvements** — 3-second wrong-answer timeout added with
  immediate channel message on first wrong attempt; A/B/C/D multiple-choice display
  restored (was overwritten during edits, recovered from Pi backup image via `debugfs`).
  Question database expanded from 448 → 3,649 questions across 5 categories:
  General Knowledge, Video Games, Science & Nature, Television, Music
  (fetched from OpenTrivia DB via session-token paged requests; backup at
  `local/questions.txt.bak`).

## Recent changes (0.5.4.237) — live-mode parity infrastructure
- **`emit_fx` FX channel** — cosmetic particle bursts (explosion fallout, footstep/
  landing dust, torch chips) now spawn via `game.emit_fx(FxEvent)`, recorded into
  `StateMsg.fx_events` and replayed on live clients — mirroring the `emit_sound`
  channel. Spawn-once-works-in-all-modes. Also fixed live airborne lean (networked
  soldier velocity) and game-over headline parity.
- **Unified network structs** — `src/server/msg.rs` deleted; `src/net/msg.rs` is the
  single source of truth, exposed via `pub mod net` in the lib. Both bins use
  `arty::net::{msg::*, encode}`.
- **`net_sync` module** — `build_state` + `apply_server_state` moved to
  `src/game/net_sync.rs` (lib), making the server→client round-trip testable.
- **Parity test** — `tests/parity.rs` round-trips a perturbed game and fails if a
  synced field is dropped (`cargo test --test parity`).
- **Compile-time field guard** — adding a field to `GameState`/`InputMsg` breaks the
  exhaustiveness checklists in `net_sync.rs` until it's classified; default is SYNCED.
- **Test suite restored** — the stale lib test suite (frozen since weapons moved off
  `Soldier`) was fixed/retired: 441 pass, 0 fail.

## Recent changes (0.5.4.195–0.5.4.197)
- **Live opponent-name sync fix** — `StateMsg.opp_team_name` (computed relative
  to whose turn it was, so the non-active client saw their *own* name reflected
  back as the opponent's) replaced with `team_names: [String; 2]`; each client
  now reads `team_names[1 - my_team]`.
- **Propeller Hat** — sprite's own static propeller bar (source rows 18-26) is
  now skipped at render time; a single rotating bar is drawn at the hub instead,
  sized to match the hat (half-length 6px, thickness 3px) and matching the
  sprite's propeller color.
- **Plasma Torch** can now be activated and used as a weapon with no terrain
  immediately ahead (removed the "stop if tip is in open air" early-exit).
- **Black Hole projectile** now renders as a layered void circle (dark purple →
  purple → black) with two orbiting glow particles, instead of a plain yellow dot.
- **Update screen changelog** now word-wraps to the screen width and caps total
  lines to fit above the A/B prompts (`renderer::font::wrap_text`).
- **Garcia (Hand of Jerry) cursor** now starts near the active soldier
  (`pos.y - 40`) instead of at the top of the screen.
- **Fire squirm** — soldiers standing in a `FirePatch` now jitter slightly each
  tick (`Soldier::on_fire_ticks`, consumed in `draw_soldier_skeletal`).
- **CPU AI** now picks a random weapon (Bazooka/Grenade/Shotgun/Tnt/Meteor Bomb/
  Blasthive/Black Hole, ammo permitting) each turn via `cpu::AI_USABLE_WEAPONS`,
  instead of always firing Bazooka.

## Recent changes (0.5.4.168–0.5.4.192)
- **HUD smear fix** — `fill_deep_water_band()` now runs before `draw_hud_world()`
  each frame, so wind/timer/weapon HUD no longer leaves trails during camera
  scroll/FPS changes.
- **Live "lost connection" fix** — server read timeout raised 5s → 15s; version
  handshake kept in sync (VERSION/REQUIRED_VERSION).
- **Hand of Jerry (Garcia) cursor** now moves freely on both X and Y axes (single
  crosshair, dpad up/down added); `NetGarcia` carries `cursor_y`/`render_y`.
- **Live crater/terrain sync reverted** to full crater-log transmission + client-side
  dedup-by-length (delta transmission from 831a23a was dropping packets under the
  50ms write timeout and permanently desyncing terrain).
- **Live crate-pickup messages** now sync to clients via `StateMsg.messages`
  (`NetMessage`) — previously only generated server-side and never shown.
- **Damage-focus camera** — after an explosion damages any soldier, the camera
  briefly holds on that soldier during the retreat phase (so HP loss can be read),
  cancelled immediately if the player pans/moves or after ~1s (`DAMAGE_FOCUS_TICKS`).
- **Wind meter redesigned** — center-anchored deflection gauge (160px wide), no
  numeric readout. Weapon-name HUD box width now sized to the weapon name.
- **Cosmetics now render the real shop-icon sprites in-game** — hats and guns
  (rotated to follow aim angle) use the same PNGs as the shop UI
  (`cosmetic_sprites::draw_hat`/`draw_gun_oriented`), replacing the old hand-drawn
  procedural shapes (which had drifted out of sync with several icons — e.g. the
  "Wizard Hat" cosmetic was rendered as a gold crown, "Revolver" as a grenade).
  Boots already used real sprites.
- **Cosmetic sprite anchors fixed (0.5.4.193)** to match
  `COSMETIC_STYLE_GUIDE.md`'s documented head/barrel anchor points, then
  **sized up (0.5.4.194)** — hats 22x20 -> 32x29 game px, gun barrel length
  12 -> 17 — for in-game readability. Propeller Hat's spinning blades
  (lost when hats switched to static sprites) restored as an animated
  overlay that still follows wind direction/speed.
- TAT "opponent's move" screen confirmed at 5s (stale comment said 4s).

## Recent changes (0.5.4.135–0.5.4.167)
- **Fix background sky-band double-paint (0.5.4.167)** — `copy_bg_viewport`'s
  row-memcpy background pass previously covered `0..max_y` (the *tallest*
  on-screen `sky_limit`), so columns with a shorter sky had rows
  `sky_limit[wx]..max_y` painted by the background and then immediately
  overwritten by the terrain copy in `copy_viewport_from_sky_aware`. The
  background pass now covers only `0..min_y` (the *shortest* on-screen
  `sky_limit`, guaranteed sky for every column); columns whose own
  `sky_limit` is taller get that extra `min_y..sky_limit[wx]` band filled
  once from the background cache during the terrain copy instead. Net
  "terrain+bg" pixel writes drop by exactly the previously double-painted
  amount — most on terrain with high height variance across the screen
  (cliffs/canyons/floating islands), negligible on flat terrain.
- **Row-memcpy background sky-band copy (0.5.4.166)** — the background
  sky-band copy (parallax layer) now uses per-row slice copies instead of
  per-pixel reads/writes, cutting up to ~150k individual pixel ops/frame down
  to a handful of contiguous memcpys.
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
- Tactical terrain generation from real WA terrain art (2 masks, mirrored/shifted
  per seed) plus an occasional carved-cavern map; seed-random chasms, overhangs,
  caves; water zone
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

### Cosmetic paper-doll preview tool
`tools/paperdoll.py` (Python + Pillow) is an **offline design/preview tool
only** — it does not affect the game, which still renders soldiers
procedurally (`src/renderer/skeleton.rs`). It ports that file's bone math
(lengths, joint angles, pose functions) so sprite-based cosmetic parts can be
authored and previewed against every in-game pose (idle, walk cycle,
airborne, spin/backflip, dead) with team/uniform/boot colours and existing
hat/gun cosmetics. See `tools/paperdoll_parts/README.md` for the authoring
spec (orientation, pivots, recolour placeholders).
