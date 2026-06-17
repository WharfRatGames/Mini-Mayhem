# Multiplayer

Mini-Mayhem has two online multiplayer modes: **Live Game** (real-time) and **Take a Turn** (async). Both require a free account.

---

## Creating an Account

From the title screen: **MY ACCOUNT → REGISTER**. Choose a username and password. Your account stores your ELO rating, match history, scrap balance, and cosmetic unlocks.

---

## Live Game

Both players connect to the server simultaneously and play in real time. The server runs the full physics simulation — clients only send inputs and render the authoritative state.

### Queues

| Queue | ELO affected | Reconnect window |
|---|---|---|
| Casual | No | Yes (3 min) |
| Ranked | Yes (K=32) | Yes (3 min) |

### Disconnection & Reconnect

If you lose connection mid-match:
- A **3-minute timer** appears for the remaining player
- Return to the title screen — a popup will offer **RECONNECT** or **ABANDON**
- If you reconnect in time, the match resumes from where it left off
- If the timer expires, the connected player wins by forfeit

### Ranked ELO

- Starting ELO: **1000**
- Floor: **100** (can't go below)
- K-factor: **32** (same for all players)
- ELO change is displayed on the match result screen

---

## Take a Turn (TAT)

Async multiplayer — each player takes their turn when they have time. The match state lives on the server between turns.

- **14-day turn timer** — miss your turn and your opponent wins
- **Casual** and **Ranked** queues available
- Your opponent's last move is replayed when you load the match so you can see what happened

---

## Leaderboard

Accessible from the multiplayer menu. Shows:

- **Top Wins** — ranked by total wins, with ELO and win/loss record
- **Top Kills** — ranked by total kills across all matches
- Your own position is highlighted in gold even if you're outside the top 50

