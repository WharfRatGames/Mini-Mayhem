# Arty — contributor notes

## Live-multiplayer parity (read before adding gameplay/visuals)

The live client **never runs `simulate()`**. It rebuilds state every tick from a
`StateMsg` via `src/game/net_sync.rs` (`build_state` on the server,
`apply_server_state` on the client). Anything produced inside the sim that isn't
carried through that round-trip is invisible in live multiplayer, even though it
works in hotseat / vs-CPU / TAT.

**Definition of done for new features:**

- **New synced state** (a field on `GameState` that affects what players see or
  the outcome): add it to `StateMsg` (`src/net/msg.rs`), populate it in
  `build_state`, and reconstruct it in `apply_server_state` (both in
  `src/game/net_sync.rs`).
- **New cosmetic FX** (particle bursts): never call `fx::explosion/splash/dust/dig`
  directly from sim code. Add a variant to `FxEvent` (`src/renderer/fx.rs`) and
  spawn via `game.emit_fx(...)`. It auto-replicates to live clients through the
  `fx_events` channel — same pattern as `emit_sound`/`sounds`.
- **New sound**: route through `game.emit_sound(Sfx)` (see `src/game/state.rs`).
- **Cover it:** write a test using `assert_all_paths_in_sync` in `tests/parity.rs`.
  It runs your input sequence through all 5 paths and asserts `synced_snapshot`
  matches across all of them. Run `cargo test --test parity`.
- **Compile-time forcing function:** adding a field to `GameState`, `InputMsg`,
  **or any nested gameplay struct** (`Soldier`, `Team`, `Projectile`,
  `DroppedCrate`, `PlacedMine`, `Barrel`, `FirePatch`, `BlackHole`, `Grave`,
  `RopeState`, `GarciaState`, `AirstrikeState`, `HomingMissileState`,
  `PlasmaTorchState`, `GameMessage`, `AimState`, `TurnManager`) breaks the
  exhaustiveness checklists in `src/game/net_sync.rs` AND the exhaustive
  snapshot destructures in `tests/parity.rs`. You can't compile until you
  classify the new field — which is the prompt to do the
  `StateMsg`/`build_state`/`apply_server_state`/parity-test work.
  **Recursion rule:** a new struct that rides a synced `GameState` field gets
  its own `_*_parity_checklist` in the same commit. **Never add `..` to a
  checklist or snapshot destructure** — that reopens the silent-desync hole.
- **Sealed channels:** the raw fx spawn fns (`fx::explosion/splash/dust/dig/
  damage_popup`) are private to `fx.rs` — sim code physically cannot bypass
  `emit_fx`. The sound channel is guarded by the `no_channel_bypass_in_source`
  test in `tests/parity.rs` (fails on raw `.sounds.push` / `audio::play`
  outside `emit_sound` and the live client's replay loop).
- **Commit + CI gates:** the pre-commit hook runs `cargo check --tests` on any
  gameplay diff (so unclassified fields can't be committed; emergency bypass
  `SKIP_COMPILE_CHECK=1` — `SKIP_MODES_CHECK=1` only skips the region
  heuristic). `.github/workflows/parity.yml` repeats the compile gate + parity
  suite on every push, so a clone without the hook is still covered.
- **Default is SYNCED.** New gameplay/visible state should be synced unless you can
  justify otherwise. To skip syncing, put the field in the checklist's
  "not networked" group with a `// not synced: <reason>` comment. When unsure, sync
  it — an over-synced field is harmless; a missed one is a silent live-mode desync.

Render-time, client-local differences (e.g. hiding the opponent's crate-pickup
messages in `render_live`) are intentional and are *not* state — the parity test
compares state only and won't flag them.

## All-modes checklist (required before marking ANY gameplay change done)

Every gameplay change — camera tracking, input handling, visual behavior, per-tick
logic, weapon behavior, UI overlays — must be verified against **all five paths**:

| Path | Where |
|---|---|
| Hotseat / VS CPU | `tick()` + `update_camera()` in `src/game/loop_runner.rs` |
| Live server | `server_tick()` in `src/game/loop_runner.rs` |
| Live client | camera + input block in `src/main.rs` ~line 758 |
| TAT visual replay (opponent's move) | `replay_tick()` call in `src/main.rs` ~line 2611 |
| TAT fast-forward (own move) | `server_tick()` call in `src/main.rs` ~line 2650 |

Before calling a change done, explicitly ask: *"does this also need to happen in
live client? TAT replay? TAT fast-forward?"* The default answer is **yes**.
Missing a path is a silent bug — nothing warns you.

## Automatic parity for simulation features

Gameplay logic added inside `simulate_with_muzzle` is **automatically correct in all
paths** — all five paths call it. The previous risk of silent divergence from
server-side input preprocessing (button stripping) is eliminated:

- `server_tick` takes `aim_angle: Option<f32>`; the server passes `Some(msg.aim_angle)`.
- `process_aim` applies it directly when `Some` and skips Up/Down button processing.
- **Up/Down are never stripped** — they reach cursor-phase weapons (homing missile,
  airstrike, any future weapon) without any special-casing in server code.
- Adding a new cursor-phase weapon: implement it in `simulate_with_muzzle`. Done.

The only remaining manual step is `StateMsg` classification (compile checklist catches it).

## TAT replay parity (read before changing tick() or the weapon menu)

TAT has **five code paths**:

| Path | Where |
|---|---|
| Hotseat / VS CPU | `tick()` in `loop_runner.rs` |
| Live server | `server_tick()` in `loop_runner.rs` |
| Live client (state rebuild) | `build_state` + `apply_server_state` in `net_sync.rs` |
| TAT visual replay (opponent's move) | `src/main.rs` ~line 2611 |
| TAT fast-forward (own move) | `src/main.rs` ~line 2650 |

**The invariant:** anything `tick()` does *before* calling `simulate_with_muzzle`
must also be done in both TAT replay loops. Currently that means
`process_weapon_menu` — called first in both `tick()` and `replay_tick()`, with
`server_tick` skipped when it returns `true` (menu open). If you ever add another
pre-simulate step to `tick()`, add it to `replay_tick()` too.

The `tat_replay_applies_weapon_switch` test in `tests/parity.rs` catches regressions
here. Run `cargo test --test parity` when changing the weapon menu or pre-simulate flow.

## Message structs are shared

`src/net/msg.rs` is the single source of truth, exposed via `pub mod net` in
`src/lib.rs`. Both the `arty` client and the `server` bin use it
(`use arty::net::{msg::*, encode}`). Do **not** re-create a server-side copy.

## Terrain generation

`Terrain::generate_tactical(seed)` in `src/world/terrain.rs` is the **only** generator
reachable in-game (called from `src/main.rs` and `src/server/main.rs`). It is purely
seed-derived and **never transmitted** — the map `seed` rides in `StateMsg` and both sides
rebuild the bitmap locally, so generation MUST stay deterministic (fixed-order f64/integer
math, no time/RNG/HashMap-order nondeterminism) and client+server MUST run identical code
(bump `VERSION`/`REQUIRED_VERSION` together for any change). Every seed composes a novel
silhouette as a collage of real WA terrain art (`src/world/wa_templates.rs`:
`collage_params`/`collage_density` — per-seed segments spliced, domain-warped and
crossfaded from baked 1-bpp masks in `src/world/wa_masks/`); cavern maps (~20% of seeds)
carve chambers from the same collage, inverted. Both branches box-blur the continuous
density field before thresholding. The mask library (20 island + 8 cavern as of
v0.5.4.428) is sourced from WA MapGen.exe run headless under Wine with a settings file
(`objects 0` — near-sprite-free silhouettes; see `tools/gen_mapgen_corpus.py`); bake masks
with `tools/extract_wa_mask.py` (land.dat, or a PNG with `--nonblack --clean-sprites`).
Generator constants are calibrated against a measured MapGEN reference corpus —
rerun `tools/terrain_stats.py` (with the `dump-terrain` bin) before retuning them.
Non-cavern maps must keep open water at BOTH side edges. Scenery footprints/variant
counts also feed seed-derived placement — changing them is a determinism change too.
Sanity tests live in `tests/wa_collage_check.rs`. `from_heightmap` is a
`#[cfg(test)]`-only fixture, not a live generator; the old `generate_worms` has been
removed.

## Build / test

- `cargo build` (all binaries), `cargo build --bin server` (server only).
- `cargo test --test parity` runs the parity guard. NB: some inline
  `#[cfg(test)]` modules are stale (reference an old `Soldier.weapons` API) and
  don't compile under `cargo test --lib`; the integration test is unaffected.
- Do not build for the Miyoo device or deploy without explicit instruction; bump
  the `VERSION` string in `src/main.rs` **and** `REQUIRED_VERSION` in
  `src/server/main.rs` (line ~1386) first — the server requires an exact version
  match and rejects every other client. The deploy script does **not** update
  `REQUIRED_VERSION` automatically.
  Quick check: `grep -n 'VERSION' src/main.rs src/server/main.rs`
- The server build profile is `[profile.server]` (`panic = "unwind"`, not `release`'s
  `panic = "abort"` — one match panicking must not abort the whole process and take every
  other in-progress match down with it), built via `cargo piserver`
  (`.cargo/config.toml`). Its output lives under
  `target/aarch64-unknown-linux-gnu/server/server` — **not** the `release` profile's
  directory. `deploy/update_server.sh`'s `SERVER_BINARY` path must match wherever the active
  profile/alias actually writes its output; a past mismatch here silently repushed a stale
  binary (wrong `REQUIRED_VERSION`, wrong panic behavior) on every deploy until caught by a
  raw protocol test. If you change the server's cargo profile or alias, grep
  `update_server.sh` for `SERVER_BINARY` and update it in the same change.
