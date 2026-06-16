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
- **Cover it:** `tests/parity.rs` round-trips a perturbed game through the netcode
  and fails if a synced field is dropped. Add an assertion there for the new
  field. Run `cargo test --test parity`.
- **Compile-time forcing function:** adding a field to `GameState` or `InputMsg`
  breaks the exhaustiveness checklists in `src/game/net_sync.rs`
  (`_gamestate_parity_checklist` / `_inputmsg_parity_checklist`). You can't compile
  until you classify the new field — which is the prompt to do the
  `StateMsg`/`build_state`/`apply_server_state`/parity-test work.
- **Default is SYNCED.** New gameplay/visible state should be synced unless you can
  justify otherwise. To skip syncing, put the field in the checklist's
  "not networked" group with a `// not synced: <reason>` comment. When unsure, sync
  it — an over-synced field is harmless; a missed one is a silent live-mode desync.

Render-time, client-local differences (e.g. hiding the opponent's crate-pickup
messages in `render_live`) are intentional and are *not* state — the parity test
compares state only and won't flag them.

## Message structs are shared

`src/net/msg.rs` is the single source of truth, exposed via `pub mod net` in
`src/lib.rs`. Both the `arty` client and the `server` bin use it
(`use arty::net::{msg::*, encode}`). Do **not** re-create a server-side copy.

## Build / test

- `cargo build --bin arty` (client), `cargo build --bin server` (server).
- `cargo test --test parity` runs the parity guard. NB: some inline
  `#[cfg(test)]` modules are stale (reference an old `Soldier.weapons` API) and
  don't compile under `cargo test --lib`; the integration test is unaffected.
- Do not build for the Miyoo device or deploy without explicit instruction; bump
  the `VERSION` string in `src/main.rs` first.
