#!/usr/bin/env python3
"""IRC Dashboard — reads BenBot's ChannelLogger logs; no IRC connection needed.
Serves JSON on port 7781 for the /ircdash/ dashboard."""

import os, re, time, json, threading, sqlite3, collections
from http.server import HTTPServer, BaseHTTPRequestHandler

LOG_BASE   = os.path.expanduser("~/irc-bots/TriviaBot/logs/ChannelLogger/crumbonium")
HTTP_PORT  = 7781
MAX_MSGS   = 60
ZPG_DB     = os.path.expanduser("~/irc-bots/TriviaBot/data/zpg.db")
DICERPG_DB = os.path.expanduser("~/irc-bots/TriviaBot/data/dicerpg.db")

CHANNELS = ["#lobby", "#general", "#dicerpg", "#zpg"]

# Log dir names use lowercase channel names
def log_path(ch):
    name = ch.lower()
    return os.path.join(LOG_BASE, name, f"{name}.log")

# ── Regex patterns ─────────────────────────────────────────────────────────────
RE_MSG  = re.compile(r'^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})\s+<([^>]+)>\s+(.+)$')
RE_JOIN = re.compile(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\s+\*\*\*\s+(\S+)\s+<[^>]+>\s+has joined')
RE_QUIT = re.compile(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\s+\*\*\*\s+(\S+)\s+<[^>]+>\s+has (quit|left)')
RE_KICK = re.compile(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\s+\*\*\*\s+(\S+)\s+was kicked')
RE_NICK = re.compile(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\s+\*\*\*\s+(\S+)\s+is now known as\s+(\S+)')

# ── Shared state ───────────────────────────────────────────────────────────────
lock   = threading.Lock()
users  = {ch: set()  for ch in CHANNELS}
msgs   = {ch: collections.deque(maxlen=MAX_MSGS) for ch in CHANNELS}
last_mtime = {ch: 0.0 for ch in CHANNELS}
last_size  = {ch: 0   for ch in CHANNELS}

def ts_to_hms(iso):
    return iso[11:19]  # "HH:MM:SS" from "YYYY-MM-DDTHH:MM:SS"

def rebuild_channel(ch):
    """Re-read the entire log for a channel to rebuild user list + recent msgs."""
    path = log_path(ch)
    if not os.path.exists(path):
        return

    new_users = set()
    new_msgs  = collections.deque(maxlen=MAX_MSGS)

    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.rstrip("\n")
            m = RE_MSG.match(line)
            if m:
                new_msgs.append({"t": ts_to_hms(m.group(1)), "nick": m.group(2), "msg": m.group(3)})
                continue
            mj = RE_JOIN.match(line)
            if mj:
                nick = mj.group(1)
                new_users.add(nick)
                new_msgs.append({"t": ts_to_hms(line[:19]), "nick": "*", "msg": f"{nick} joined"})
                continue
            mq = RE_QUIT.match(line)
            if mq:
                nick = mq.group(1)
                new_users.discard(nick)
                new_msgs.append({"t": ts_to_hms(line[:19]), "nick": "*", "msg": f"{nick} left"})
                continue
            mk = RE_KICK.match(line)
            if mk:
                new_users.discard(mk.group(1))
                continue
            mn = RE_NICK.match(line)
            if mn:
                old, new = mn.group(1), mn.group(2)
                new_users.discard(old)
                new_users.add(new)

    with lock:
        users[ch] = new_users
        msgs[ch]  = new_msgs
        last_mtime[ch] = os.path.getmtime(path)
        last_size[ch]  = os.path.getsize(path)

def append_new_lines(ch, path):
    """Efficiently read only new lines since last check."""
    new_msgs_local = []
    new_users_delta = {}  # nick → "join" or "quit"

    with open(path, encoding="utf-8", errors="replace") as f:
        f.seek(last_size[ch])
        for line in f:
            line = line.rstrip("\n")
            m = RE_MSG.match(line)
            if m:
                new_msgs_local.append({"t": ts_to_hms(m.group(1)), "nick": m.group(2), "msg": m.group(3)})
                continue
            mj = RE_JOIN.match(line)
            if mj:
                nick = mj.group(1)
                new_users_delta[nick] = "join"
                new_msgs_local.append({"t": ts_to_hms(line[:19]), "nick": "*", "msg": f"{nick} joined"})
                continue
            mq = RE_QUIT.match(line)
            if mq:
                nick = mq.group(1)
                new_users_delta[nick] = "quit"
                new_msgs_local.append({"t": ts_to_hms(line[:19]), "nick": "*", "msg": f"{nick} left"})
                continue
            mk = RE_KICK.match(line)
            if mk:
                new_users_delta[mk.group(1)] = "quit"
                continue
            mn = RE_NICK.match(line)
            if mn:
                new_users_delta[mn.group(1)] = "quit"
                new_users_delta[mn.group(2)] = "join"

    with lock:
        for nick, action in new_users_delta.items():
            if action == "join":
                users[ch].add(nick)
            else:
                users[ch].discard(nick)
        for entry in new_msgs_local:
            msgs[ch].append(entry)
        last_mtime[ch] = os.path.getmtime(path)
        last_size[ch]  = os.path.getsize(path)

def poll_loop():
    # Initial full read of all channels
    for ch in CHANNELS:
        try:
            rebuild_channel(ch)
        except Exception as e:
            print(f"[init] {ch}: {e}")

    while True:
        time.sleep(3)
        for ch in CHANNELS:
            try:
                path = log_path(ch)
                if not os.path.exists(path):
                    continue
                mtime = os.path.getmtime(path)
                size  = os.path.getsize(path)
                if mtime != last_mtime[ch] or size != last_size[ch]:
                    if size < last_size[ch]:
                        # Log rotated — full rebuild
                        rebuild_channel(ch)
                    else:
                        append_new_lines(ch, path)
            except Exception as e:
                print(f"[poll] {ch}: {e}")

# ── RPG leaderboards ───────────────────────────────────────────────────────────
def zpg_top(n=8):
    try:
        if not os.path.exists(ZPG_DB):
            return []
        con = sqlite3.connect(ZPG_DB)
        con.row_factory = sqlite3.Row
        rows = con.execute(
            "SELECT nick, level, xp, hp, max_hp, item_bonus, tacos, faction "
            "FROM players ORDER BY level DESC, xp DESC LIMIT ?", (n,)
        ).fetchall()
        con.close()
        return [dict(r) for r in rows]
    except Exception:
        return []

def dicerpg_top(n=5):
    try:
        if not os.path.exists(DICERPG_DB):
            return []
        con = sqlite3.connect(DICERPG_DB)
        con.row_factory = sqlite3.Row
        rows = con.execute(
            "SELECT nick, hp, max_hp, xp, tacos, kills "
            "FROM players ORDER BY kills DESC, xp DESC LIMIT ?", (n,)
        ).fetchall()
        con.close()
        return [dict(r) for r in rows]
    except Exception:
        return []

# ── HTTP handler ───────────────────────────────────────────────────────────────
class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a): pass

    def do_GET(self):
        if self.path in ("/irc/state", "/irc/state/"):
            with lock:
                data = {
                    "channels": {
                        ch: {
                            "users": sorted(list(users[ch]), key=str.lower),
                            "msgs":  list(msgs[ch]),
                        }
                        for ch in CHANNELS
                    },
                }
            data["zpg_top"]     = zpg_top()
            data["dicerpg_top"] = dicerpg_top()
            body = json.dumps(data).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", len(body))
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_response(404)
            self.end_headers()

if __name__ == "__main__":
    threading.Thread(target=poll_loop, daemon=True).start()
    print(f"[irc-dash] HTTP on :{HTTP_PORT}")
    HTTPServer(("127.0.0.1", HTTP_PORT), Handler).serve_forever()
