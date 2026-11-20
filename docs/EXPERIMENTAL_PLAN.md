# Arty experimental branch: design and plan

> **RULE (user, 2026-11-19): ALWAYS use `tools/wa_rig` FIRST to measure/verify any WA value or behaviour (HP, damage, speeds, timings, spawns, offsets) before decompile-guessing or hand-writing gdb. Add a scenario + probes/fields there if one is missing. Arty-side checks: `cargo run --release --example scenario -- <hp|bazooka|banana|direct>` in /home/dusty/arty-experimental.**
> Resume file: /home/dusty/worms_decomp/resume.md (section START HERE).

Status: living document. This is the design for the `experimental` branch only; production `main` is unaffected.
Origin: a "what if we started over" discussion, applied retroactively to the existing code instead of a rewrite.

## Goal

Arty (Rust, Miyoo Mini Plus / OnionOS) should feel identical to Worms Armageddon, by porting WA.exe's algorithms
(`findings/physics.md` in the decompile workspace is the spec).

### Define "identical" (keep these two lists current)

**Non-negotiables (match WA):**
- Worm walk / jump / backflip / fall physics (16.16 fixed point, 50 Hz frames), via the `wa_*` modules.
- Projectile, explosion and damage rules.
- Turn flow and terrain-collision semantics.

**Deliberate deviations (documented, not accidental):**
- Sim ticks at 30 Hz and converts 5/3 to WA's 50 Hz at the worm-physics boundary.
- Pixel-art soldiers (24x32, drawn at `LOOK_SCALE` = 1.5) instead of WA's sprites; five looks, cosmetics layers.
- Walk pace is 1.5x WA's (`WALK_FRAMES_PER_TICK` 5 per 2 ticks instead of 5 per 3; `walk_frames_for_age`), because the ~2x taller soldiers made WA's
  px/s read as half speed. Walk animation is 2 ticks per frame to match. Jumps and falls stay at WA's real speed.
- Jump arc scale `JUMP_SCALE` = 1.0 (WA's real velocities). It was sqrt(0.6) while soldiers were ~23 px tall.
- Black hole collapse damage is graded by distance (full inside 40 px, linear to 0 at 108 px). Arty-only weapon.
- Map objects: art scaled by `object_art_scale()` (= drawn soldier height / WA worm 27 px x `OBJECT_ART_TRIM` 0.7, ~1.2x; full 1.7x read as giant on device) so mine (WA 16x8) and oil drum (WA 24x36)
  keep WA's proportions beside the soldiers (sizes read from Gfx.dir). Collision footprints stay at WA size (art only).
  Mines and drums now spawn like WA: the same placement pass as worms (`Terrain::find_spawn_pool`, 107 px exclusion, skipped when
  there is no room), 3 of 5 spots mines (max 12), 2 of 5 drums (max 8). Object counts therefore drop versus the old 16-24 / 14-20.
- Walking headroom: a soldier can't walk into a gap lower than `HEAD_CLEAR` (36 px) unless it's no worse than where he stands (never trapped), because the
  WA 9x16 collision body is much smaller than the ~46 px drawn soldier. Jumps and falls are unchanged.
- Jump/backflip crouch: WA's wind-up is 20 frames (0.4 s) with a crouch pose. Arty animates the crouch (`SoldierAnim::Crouch`, the idle sprite squashed to
  `CROUCH_SQUASH` 0.7 toward his feet, eased in over 3 ticks) and shortens the wind-up to `WA_WINDUP_FRAMES` = 8 (0.16 s) so it reads as a wind-up
  without feeling like input lag. Synced to live clients as `NetSoldier.crouch` (wind-up age; needs the exp server rebuilt with the client).
  Landing recovery (12 frames) is unchanged. Also fixed: the pixel-art draw window clipped the top of every soldier's helmet.
- Per-soldier cosmetics, store, accounts, online play: Arty-only.

## Principles

1. **Replayable.** Any bug is a recorded input stream plus a seed, so it becomes a failing test instead of a log hunt.
2. **One rule per question.** One ground/support test, not four that disagree on edge cases.
3. **The decompile is the spec**; verify ports against real WA captures where possible (oracle tests).
4. **Server authoritative, client renders.** Offline and online share `simulate()`. `tests/parity.rs` guards this.
5. **Measure before changing the tick rate.** Device frame budget is ~33 ms at 30 fps with ~21 ms used.
6. **Named constants for scales.** No magic scale factors; tests tie body size to jump apex.

## Lessons from this project (what actually broke)

- Position rounding to whole pixels: 0.83 px/tick walking hitched ~5x/s (fixed by drawing at sub-pixel position).
- 30 Hz <-> 50 Hz conversion: rounding and jitter sources in walk and fall damage.
- Duplicate ground checks (`is_on_ground`, `is_on_ground_lenient`, `is_on_soldier`, `is_embedded_in_terrain`):
  soldiers stuck Idle/not-grounded where one check says grounded and another says not.
- One audio device open per sound: overlapping sounds dropped, pops on every open/close.
- Hard-coded scale factors that stopped matching when the art changed (jump arc vs soldier height).
- "All or nothing" thresholds (black hole 13 px horizon) read as random damage.

## Plan

Order is payoff per risk. Each step ships on its own.

| # | Step | Status |
|---|------|--------|
| 1 | Replay harness: record inputs + seed, replay, per-tick state hash, golden tests, sim-time perf timer | done in source; server recording hook is env-gated (`ARTY_RECORD_DIR`), not yet enabled on the Pi |
| 2 | One ground-and-support rule (`Support` / `support()`), plus the stuck-soldier bugs the replay invariant found | done in source, not yet on a device |
| 3 | 50 Hz fixed-point sim state | **decided: not needed.** Device `[sim]` timing (Miyoo .126): ~0.75 ms per tick, max ~1.2 ms, ~21 ms per second of sim = ~2% of the frame budget, so the sim is not the bottleneck and the 30 Hz tick with 5/3 conversion at the worm-physics boundary stays |
| 4 | Replace remaining hard-coded scales with named constants + a test tying body size to jump apex | done in source (`tests/scale_contract.rs`: backflip >= 0.75 drawn heights, walk >= 0.8 heights/s) |
| 5 | Audio: software mixer | **done, verified on device.** Mixer logs show 7-18 ms queue; the late-later-shots bug was the device FIFO lead behind a continuously running stream, fixed by stopping the stream when idle and restarting fresh per burst (25 ms queue, widens to 60 ms on underrun) |
| 6 | Oracle tests: diff the sheep, walk and backflip ports against real WA frame by frame | **done.** Sheep walker: 600 Unicorn scenarios (`tests/wa_sheep_oracle.rs`); sheep bounce: 2000 Unicorn cases; sheep fuse/landing read from decompile. Walk cycle and jump/backflip/fall were already asserted against live WA captures (`tests/wa_worm_air.rs`: phase table, 10+ real walk runs, jumps, wall bounces, worm-on-worm). Unverified: sheep launch from the weapon-fire path, blast numbers, exact sheep shape (static read = shape 2) |

Not planned: a crate split (large refactor, little benefit while features are in flight).

### Step 1 detail: replay harness

- `src/replay.rs` (library): `Replay` (seed, team count, held-button bits per tick), `Recorder`, `new_game`, `state_hash`, `run`.
- Text format, versioned, run-length encoded; golden files live in `tests/replays/`.
- `tests/replay.rs`: determinism (two runs identical), golden checkpoints (bless with `REPLAY_BLESS=1`), timing printout.
- Sim timer: `dlog::note_sim` accumulates per-second sim time into the `[perf]` line (`sim avg/max`), offline play only
  (the live client does not run `simulate()`; the server does).
- Done: `Replay`/`Recorder`/`state_hash`/`run`, `tests/replay.rs` (determinism, golden, reload, sensitivity), `[sim]` perf line.
  Host cost measured: ~24 us/tick for a walk/jump/fire scenario, worst single tick under 1 ms. Device numbers pending (needs a push).
- Caveat: `new_game()` builds a plain 2-team game; a replay only reproduces a real match if the match was built the same way
  (spawn rules, CPU teams, loadouts). The recorder hook therefore needs to capture the match setup, not just seed + inputs.
- Format v2 (`arty-replay 2`): per tick the full server input (`TickInput`: held, pressed, released, aim angle, muzzle, weapon kind),
  run-length encoded; replays run through `server_tick` (the live-server path, which also advances `game.tick`) on a game built by
  `game::setup::build_game_n` (the server's own builder, mines and barrels included).
- `replay::MatchRecorder`: `ARTY_RECORD_DIR=<dir>` on the server records every ranked and casual match to
  `<dir>/<match_id>-<n>.replay` (ranked rematches get a new file). Unset = nothing happens. To enable on the exp Pi server add
  `Environment=ARTY_RECORD_DIR=%h/mayhem-server-exp/replays` to `arty-game-exp.service`. Replay a file with
  `Replay::from_text(&fs::read_to_string(p)?)?.run()`; a divergence names the first tick.
- Fidelity limits: roster/cosmetic fields are not sim state and are not recorded; disconnect/forfeit handling that happens
  outside `server_tick` is not reproduced.
- **One match setup.** `src/game/setup.rs` now holds `build_game_n`, `place_map_mines`, `place_map_barrels`. Before, the client and
  server each had a copy and they differed: the server's barrel placement lacked the client's cavern-floor lookup and scenery
  clearance, so live matches on cave maps could put barrels on the unreachable rock roof. The server now uses the client's (more
  complete) version, a deliberate live-behaviour change.
- Probe tests: `spawn_walk_probe::probe_on_ground` became the real invariant `tests/replay.rs::every_spawned_soldier_starts_grounded`; `bh_probe` is already a deterministic regression test. `spawn_walk_probe`'s print-only walk/sheep probes remain as probes.

### Step 2 detail: ground rule (done in source)

- `loop_runner::support()` classifies a soldier once: `Firm`, `OnSoldier`, `Ridge`, `Embedded`, `NearSoldier`, `None`.
  Walking, jumping, backflipping and the Idle/Airborne handoff all ask it (`can_act()`, `is_firm()`, `holds()`), so they cannot
  disagree. The old primitives (`is_on_ground`, `is_on_ground_lenient`, `is_on_soldier`, ...) remain as its inputs.
- The refactor on its own changed no behaviour: the replay goldens were bit-identical before and after.
- The server's own third copy of `is_on_ground` was dead code (`apply_input` was never called) and is deleted.
- `tests/replay.rs::idle_soldiers_can_act_within_a_few_ticks` is the invariant: over 8 seeds x 4000 random-input ticks, a living Idle
  soldier must be able to act within 12 ticks. It found three real stuck-soldier bugs (each now fixed):
  1. **Ceiling hang.** A rising worm that bumps an overhang "lands" there (WA stop rule). The Idle logic exempted any body
     overlap as "embedded, leave alone", so he hung in mid-air for 80+ ticks. Now an embedded soldier with open air under his feet
     sinks 3 px/tick until clear (then falls normally) or reaches the waterline (then drowns). `unstick_embedded_soldier`
     + Airborne was tried first and rejected: it picks the nearest clear spot, which can be above, and loops forever.
  2. **Beside a neighbour.** A WA "landed" flag can fire in a sweep and leave the body clear of what it hit, and the
     `NearSoldier` exemption then held him in open air beside the other soldier. `wa_air_tick` now confirms real support
     (`WormBody::supported_with`) before counting a landing, and an Idle soldier overlapping a neighbour with nothing under him is
     separated 1 px/tick. Resting on a worm is now also judged by WA's mask shapes (`WormBody::resting_on_worm`), not only the
     rectangle test.
  3. **Below the waterline.** A soldier that stopped at/below `WATER_Y` stayed alive and Idle (hover). He now drowns.
- Remaining and deliberate: `jumpbot_is_on_ground` is a separate footprint (the Jumpbot is not a soldier) and was left alone.
- Goldens were re-blessed after the behaviour changes above; they first diverged late (ticks 780 and 3810), i.e. only where a
  soldier hit one of these cases. The WA capture tests (`wa_worm_air`) are unaffected.

## Test suite status

`cargo test --release --lib --bins --tests` is fully green (the 15 stale lib tests were updated to current behaviour: weapon slots are
looked up by kind not index, `Button::Menu` exists, TNT is 78 px/75 dmg vs grenade 50/50 (WA), world is 1920x960, pixel-art soldiers have no
flat team-colour fill, projectiles need several steps to reach the water). `examples/bg_preview.rs` still does not compile.

## Rules for working on this branch

- Version label is `0.5.4.439-exp20` on client and Pi exp server; keep them matching unless both are rebuilt.
- Don't build without being told; push to the Miyoo(s) only when told (always attempt 126 AND 110 on an exp push unless
  scoped); commit only when asked.
- Internal tooling (dlog, screen viewer, replay recording) never ships or is advertised to players.

## Step 6 progress: sheep walker oracle (done)
`emu/gen_sheep_oracle.py` runs WA's real FUN_0050a900 under Unicorn on 600 random terrain scenarios; `tests/wa_sheep_oracle.rs` replays them through `wa_sheep.rs`
(all 600 match). Fixes found: hop angle is measured from vertical (sin*m, cos*m via WA trig tables), hop trigger is `|dy| >= 0x40000`,
far probe is 8 px above the sheep and a hop is cancelled when the spot 48 px straight up is blocked. Sheep shape: statically read as shape id 2 (7x5 box; launch struct = record+0x38, ctor 0x507d10 -> table[*param_2]), matching the assumption; not rig-confirmed (the bazooka's record reads 3 yet the rig saw T2, so the field mapping is not fully understood). Still unverified: flight/bounce/fuse, walk cycle, backflip.

Sheep flight (static, not oracle-run): `air()` now uses the oracle-verified `wa_body::sweep` (up to 2 sweep+bounce rounds, like FUN_004fcdf0) and
WA's bounce FUN_004ff490 for weapon category 3 (obj+0x6c=0x9999, +0x70=+0x74=0): floor/ceiling hit zeroes velocity, wall hit reflects vx*0.6 and zeroes vy.
Not yet checked: FUN_005018f0 landing test, fuse/explosion timing, walk cycle, backflip. Goldens not re-blessed (replay tests still pass).
Sheep bounce now emulator-verified: FUN_004ff490 run under Unicorn on 2000 random velocities/sides matches the Rust rule exactly (floor/ceiling -> 0,0; wall -> vx = -((vx*0x9999)>>16), vy = 0).
Landing test FUN_005018f0 read from the decompile: for category 3, if the body overlaps one pixel below after a hit, all velocity is zeroed regardless of side (ported). Remaining: fuse/explosion timing, walk cycle, backflip.
Sheep fuse: WA rec+0x8c = 20000 ms, obj[0xdf] -= 20 per 50 Hz frame (FUN_0050b400 case 2) = 20 s real time. Arty had 10 s; now `JUMPBOT_FUSE_TICKS = 600` (the 5 s on-screen countdown is unchanged, a deliberate Arty UI choice). Suite green.

## Other weapons' physics (plan /home/dusty/.claude/plans/moonlit-forging-map.md)
Step 1 done: `tools/gen_weapon_table.py` -> `src/physics/wa_weapons.rs` (23 records with category, fuse, wind/gravity %, blast, bounce fields; spot-check test).
Launch path decoded statically (findings/physics.md): all thrown/fired weapons share ctor 0x507d10; launch speed = direction * charge * 0x18 (same as bazooka), grenade bounce 30%/60% and fuse from the player's setting come from the launch struct.
Next: wire wa_thrown + charge curve into the game, then name records and extend to cluster/banana/HHG/mortar. Resume file: /home/dusty/worms_decomp/resume.md.
Thrown-weapon constants (findings/physics.md "Thrown-weapon constants"): floor slide 0xf5c2 (0.96, from the rig-measured worm friction), explode-on-contact/stick rules and the sheep's 1-in-16 wall turn decoded (turn ported in wa_sheep.rs; tests green). Charge-to-power curve NOT yet found statically; needs one Wine/gdb capture of held-fire at 3 durations.
Grenade flight + bounce VERIFIED against a real WA throw (Wine rig): `src/physics/wa_thrown.rs` + `tests/wa_grenade_flight.rs` (151 frames bit-exact: free flight, floor bounce 0.96/0.30, rolling decay, stop rule, fuse). Charge: worm+0x248 += 873/frame while held, speed = power*24 (see findings/physics.md). NOT yet wired into the game (grenade still uses the old float path); cap of the charge is still unknown.

Grenade WIRED (-exp21, untested on device): `Projectile.thrown` + `tick::tick_grenade` run `wa_thrown::Thrown` at 5 WA frames per 3 ticks (`step_projectile` skips the float land collision; edges/water still use the swept test and a wall bounce is copied back into the fixed-point body). Charge/launch speed: `tick::uses_wa_charge` (bazooka + grenade share the 873/frame meter, full = 24 px/frame = 40 px/tick; still auto-fires at full like the bazooka, WA does not). Fuse = player's fuse_ticks (30 Hz) -> ms. Old float grenade unit tests moved to TNT (float path). Cluster/banana/HHG/mortar still on the float path. Not built or pushed.

-exp22: walking fix - `headroom()` now checks the centre column only (side columns read the hillside as a ceiling and refused every steep downhill step; found by scanning seed 18E959B6D033C1D0, 2681 -> 98 refused spots, the rest real >8 px walls; tests/walk_down_slope.rs; replay goldens re-blessed). -exp23: ClusterBomb and HolyHandGrenade also run on `wa_thrown` (`tick::uses_wa_thrown`; records 6/7/43 share one engine, 30% bounce; HHG keeps the Armed->Detonating rule: it does NOT explode until it stops moving, per the user; the 3 s fuse only arms it). Banana (rec 8, 60% bounce, fuse) NOT moved: Arty explodes it on contact, WA bounces it - needs a gameplay decision.

-exp24: WA explosion model (rig-verified, findings/physics.md "Explosion effect on an object"): for bazooka, grenade, HHG and TNT (`state::wa_blast_kind`) HP lost = floor(damage*(reach-d)/reach) with reach = 2*(damage+2) px (bazooka 104 px, wider than its 48 px crater), distance from the worm's body centre (foot-6); knockback = A/3 px/tick along the unit vector away from the blast, no upward bias, none when A = 0. Other weapons keep the old linear-in-crater-radius model. Replay goldens re-blessed. Not yet done: WA's direct-hit impulse (projectile velocity/4 to the object it hits), per-weapon force% (only 100 measured), cluster/banana/mortar damage values.

-exp25: (1) Meteor Bomb is now WA's Banana Bomb (`WeaponKind::BananaBomb`, label "BANANA BOMB"): `wa_thrown` engine with the 60% bounce, player-set fuse (L1/R1), explodes at the fuse for 75 dmg r51 (rec 8) and throws 5 pieces (rec 42: same blast, bounce 60%, 3 s fuse); piece launch speeds approximated (cluster-like), WA spawn table not decoded. (2) Bazooka direct hit: the soldier it hits is shoved by the shell velocity/4 (negated for the shooter), FUN_004fe750 msg 0x4b, from the decompile only (not rig-tested). (3) Plates: name plate over HP plate, black rounded boxes with a grey border, WA's sampled team text colours (white on the active worm), hidden on the active worm after he fires; the HP number is the counting-down `displayed_hp`. NOT done: WA's HP countdown timing was not measured (Arty: 2 s hold after the damage popup, then 1 HP per 30 Hz tick).

-exp25 HP countdown (rig-measured): WA worm object +0x178 = displayed HP, +0x17c = real HP (both 16.16). No hold; from the frame after the hit the display closes 10% of the gap per 50 Hz frame (exponential ease), snapping when the gap <= 1 HP (a 100->57 hit took ~35 frames, ~0.7 s). Arty now does this: `Soldier::step_hp_display` eases with factor 0.161/tick (0.9^(5/3)), `HP_COUNTDOWN_DELAY_TICKS` = 0 (popup tally still gates the start). Not yet checked on device.

Sim tool: `cargo run --release --example scenario -- <hp|bazooka|banana|direct|plates> [--ticks N] [--every K] [--out f.png]` stages a case, steps `server_tick` headless, prints a per-tick trace (hp, displayed hp, state, projectile) and writes a contact-sheet PNG of rendered frames. Checked: hp ease (tally pause 20 ticks then ~20 ticks ease), banana bounce -> fuse 90 -> pieces, bazooka direct hit (42 dmg, ~153 px knock + shove). Internal only.

Rig (Blast Zone scheme: only Pro/Artillery/Blast Zone give banana ammo; Blast Zone also gives HHG x2; Armageddon has none): WA cluster bomb piece launch (ctor 0x507d10 launch struct): 5 pieces, piece index at word 8, bounce 30, fuse 3000 ms, spawn within ~5 px of the burst, velocity vx within +-2.1 px/frame, vy -5.2..-6.6 px/frame (ALL pieces go up; Arty's old "flower" with slower lower pieces was wrong). `state::wa_piece_velocity` now drives both cluster and banana pieces (banana assumed to share the spawn code; not seen on the rig because F2 cycling never reached the banana). Rig tip: weapon banners are brief; F2 press count vs weapon is not a simple cycle.

CORRECTION (wa_rig, tools/wa_rig, scenarios hp_countdown + hp_delay_late): WA DOES hold before the HP countdown. Real HP (worm +0x17c) only changes ~148-151 frames after the shot is fired (~142-147 after the explosion; it matches the 3 s post-shot retreat), and the displayed HP (+0x178) starts its 10%/frame ease the same frame. The earlier "no hold" reading came from sampling that began after the hold. Arty: `HP_COUNTDOWN_DELAY_TICKS` = 66 (+20 tally ticks) so the countdown starts ~86 ticks (~144 frames) after the explosion.

wa_rig (tools/wa_rig in the decomp repo): scripted WA scenarios on the Wine rig. First banana capture: 4 pieces (not 5), vx +-1.6, vy -7.0..-8.2 px/frame -> Arty banana now throws 4 pieces with that spread (`state::wa_piece_velocity(.., banana=true)`); help text says 4. Single capture, p2=67 seen in Blast Zone (Arty keeps 75 from the record table).

## wa_rig verification status (2026-11-19)
Method: `tools/wa_rig` (Wine+gdb) scenario -> events -> compare against Arty replays (`examples/wa_flight.rs`, `wa_walk.rs`, `scenario.rs`) or unit tests. Findings: `/home/dusty/worms_decomp/findings/physics.md`.
Verified identical to WA: bazooka flight (wind 7), grenade + banana flight/bounce (incl. worm bodies, 60% banana bounce), exact `charge N` launch speeds, HHG blast reach (204) and knockback (x1.25), fall damage (`wa_worm::fall_damage`, 7 drops), walking incl. slopes and blocked-step restart, cluster/banana piece launch distribution, HP countdown ease, explosion model (reach 2*(dmg+2), knockback A/5 * force%).
Known deviations / open: cluster main blast model (WA: damage 20, force 0, reach 44), force % of other weapons, walk turn delay (~14 frames), piece RNG (own hash), HHG explodes exactly 241 frames after launch (unexplained), mapgen forced island maps, Blast Zone scheme weapon tweaks not modelled (Arty = Intermediate).
Pending build: `-exp27` (HHG freeze fix, piece velocities, no direct-hit shove, walk restart, HHG force 125%). Uncommitted.

## Status 2026-11-20 (resume from /home/dusty/worms_decomp/resume.md)
Deployed `-exp28` (Pi exp server + Miyoo .126; .110 pending). Done since -exp27: Super Bot (WA Super Sheep as a Jump Bot variant with jetpack; menu, rare crates, test mode), Jump Bot redraw (coil legs), WA sheep blast (75/r78/force 100), WA worm death explosion (30/force 100), 14-frame walk turn delay + 0.5 px nudge, Air Strike + Garcia from the decompile, HHG sinks in water, new `scenario superbot|jumpbot --keys` and `menu_shot` examples, rig poke `hp_src`. Tests: lib 443, parity 25, replay 11 (goldens re-blessed for the turn delay). Source is staged but NOT committed (pre-commit all-modes hook; bypass needs the user).
Remaining plan: commit; push -exp28 to .110; playtest; walking down-slope/ledge + turn capture; HHG 241-frame explosion; death-explosion crater radius; mapgen vs forced islands; rig cleanup; Freeze/Mine Layer/Earthquake undecided.
