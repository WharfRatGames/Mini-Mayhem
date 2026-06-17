# Arty — Security & Anti-Cheat

This documents what is actually implemented, what is intentionally out of scope,
and what should be added. Aspirational checklists that don't match the code have
been removed.

---

## What is already enforced

### Connection handshake (`server/main.rs: accept_one`)
- Client must send magic bytes `MMAY` then version string then session token, all
  newline-delimited, before any game traffic is accepted.
- Version mismatch → immediate disconnect and log entry. Old clients cannot join
  a new server and vice-versa.
- Frame reader (`read_frame`) hard-limits every incoming message to **65 536 bytes**.
  Oversized frames are dropped and the connection is closed — no allocation from
  client-supplied length.

### Server-authoritative simulation
- The server runs the full simulation every tick via `server_tick()`. The client
  sends only inputs (`InputMsg`: held/pressed/released buttons, aim angle, weapon
  kind). The server never trusts client-reported positions, damage, HP, or
  game-over conditions.
- Turn ownership is enforced server-side: only the `game.active_team()` slot's
  input is applied to the simulation each tick. The inactive player's input is
  ignored.
- Weapon selection is validated by kind, not index: if the client claims a weapon
  kind that doesn't exist in the server's loadout for that team, the selection is
  silently ignored.
- Weapon lock timers (TNT: 5 cycles, Bat: 3 cycles, AirStrike: 7 cycles) are
  checked server-side in `apply_input` — a client cannot fire a locked weapon by
  sending a fire input early.

### Packet size clamping
- Aim angle is validated as finite and clamped to `[-π, π]` before use (v0.5.4.242).
- Player position is never client-submitted. Movement is simulated entirely from
  button inputs; the client cannot teleport a soldier by sending a position.

### Authentication (`deploy/arty_api.py`)
- Passwords are stored as PBKDF2-HMAC-SHA256 with a random 16-byte salt
  (`secrets.token_hex(16)`). Legacy unsalted SHA-256 hashes are transparently
  upgraded to PBKDF2 on next login.
- Session tokens are 256-bit CSPRNG values (`secrets.token_hex(32)`) — single-use
  per login, rotated on each successful login (v0.5.4.242).
- All write endpoints (roster edits, purchases, match reporting) require a valid
  token. 401 is returned for invalid/missing tokens.
- Scrap, warbonds, ELO, and inventory are server-owned. The client can request
  a purchase; the server checks balance before committing.

### Match result integrity
- Match outcomes (ELO delta, win/loss, kill counts) are recorded by the game
  server process, not reported by clients. The API endpoint that records results
  is called from `server/main.rs` after the server observes the win condition,
  not from any client-reachable path.

### Logging
- Every match gets a dedicated log file (`match-<id>.log`) recording: connections,
  disconnections, damage events with HP before/after, kills with weapon, match
  outcome, and ELO changes.
- Version rejections, bad magic bytes, and oversize frames are logged to the
  shared server log.

---

## Known gaps / should be added

### Aim angle clamping ✓ FIXED (v0.5.4.242)
`msg.aim_angle` is now validated as finite and clamped to `[-π, π]` before
being applied in both `run_match` and `run_lobby_match`.

### Charge/power validation
The client sends a fire event but power is computed server-side from the charge
state — this is fine. However the charge accumulator itself runs on the server
from held-button time, so there is no client-exploitable power field. ✓

### Rate limiting on API endpoints
The API has no per-IP rate limiting. A script could hammer `/register` or
`/login` indefinitely.

**Fix:** track `{ip: (count, window_start)}` in memory; return 429 after e.g.
10 requests/minute per IP on auth endpoints.

### Worm name / roster name sanitization ✓ FIXED (v0.5.4.242)
All name strings from `InputMsg` and `JoinMsg` now pass through `sanitize_name`
(printable ASCII + space only, max 20 chars) before being written into the game
state.

### No per-connection input flood detection
A client that sends `InputMsg` frames faster than the tick rate (30 Hz) just
fills the inbox ring; the server only reads one per tick. Excess frames are
discarded. However a flood of large frames (up to 65 536 bytes each) could
saturate the read thread.

**Fix:** track frames received per second per connection; disconnect if it
exceeds e.g. 120/s (4× tick rate).

### Session token entropy ✓ FIXED (v0.5.4.242)
`gen_token` now returns `secrets.token_hex(32)` directly — 256 bits of CSPRNG
output, no username or timestamp input. The server's token read buffer was also
extended from 32 to 70 bytes to correctly handle the full 64-char hex token.

### No HTTPS on API
The API runs plain HTTP on the Pi. Tokens travel in cleartext.

**Acceptable for now** (private game, LAN-adjacent use), but worth noting. A
reverse proxy with a self-signed cert would be a low-effort improvement.

---

## Anti-cheat: what's in place

Because the server simulates everything and clients only send inputs, the
traditional cheat surface (modified game client reporting fake outcomes) largely
doesn't exist. Specifically:

| Cheat type | Status |
|---|---|
| Teleport / position hack | **Impossible** — position is never client-submitted |
| Infinite ammo | **Impossible** — ammo is tracked server-side, decremented after fire |
| Fire locked weapon early | **Blocked** — lock timers enforced in `apply_input` |
| Skip opponent's turn | **Impossible** — turn ownership enforced server-side |
| Fake win / ELO manipulation | **Impossible** — outcomes recorded by server process only |
| Aim bot (perfect angle) | **Possible** — aim angle is trusted from client; server can't distinguish a human from a bot |
| Speed hack (fast inputs) | **Partially mitigated** — server only processes one input per tick regardless |
| Client disconnect to avoid ELO loss | **Partially mitigated** — 3-minute reconnect window; after timeout opponent wins |

### Aim angle — the remaining exploitable surface
The server trusts `msg.aim_angle` completely. A modified client can send any
angle it wants, including perfectly computed trajectories. This is the one
meaningful cheat vector for live ranked play.

**Mitigations to consider (in order of complexity):**

1. **Clamp to ±π** — free, prevents NaN/Inf, doesn't stop aim bots.
2. **Rate-limit angle change per tick** — large instantaneous jumps are
   suspicious; log or flag them. Doesn't block slow aim bots.
3. **Replay-log validation** — store the full input stream server-side per
   match; a separate offline job can flag statistical outliers (angle accuracy
   distribution far above human baseline). Low false-positive rate, no latency
   impact.
4. **ELO anomaly detection** — flag accounts with win rates or kill rates more
   than 3σ above the population mean for manual review.

For the current player count, option 3 or 4 is the most practical: build the
logging infrastructure now so you have data if/when it becomes a problem.

---

## Summary

The server-authoritative architecture gives strong protection against the most
common cheats by construction. The main action items are:

1. ~~Clamp `aim_angle` to a finite range before applying~~ ✓ done v0.5.4.242
2. ~~Sanitize worm names from `InputMsg`~~ ✓ done v0.5.4.242
3. ~~Improve session token generation to use `secrets.token_hex(32)`~~ ✓ done v0.5.4.242
4. Add rate limiting to `/register` and `/login` (medium effort)
5. Log full input streams per match for future replay analysis (medium effort)
