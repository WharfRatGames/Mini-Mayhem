#!/usr/bin/env python3
"""IRC Dashboard monitor — connects to ngircd, tracks channels/users/messages,
serves JSON on port 7781 for the /arty/ircdash/ dashboard."""

import ssl, socket, threading, time, json, collections, os, sqlite3
from http.server import HTTPServer, BaseHTTPRequestHandler

IRC_HOST   = "localhost"
IRC_PORT   = 6697
IRC_NICK   = "DashBot"
IRC_USER   = "dashbot"
IRC_REAL   = "IRC Dashboard Monitor"
IRC_PASS   = "alpaca1114"
HTTP_PORT  = 7781
CHANNELS   = ["#lobby", "#General-Chat", "#dicerpg", "#zpg"]
MAX_MSGS   = 60  # messages kept per channel
ZPG_DB     = os.path.expanduser("~/irc-bots/TriviaBot/data/zpg.db")
DICERPG_DB = os.path.expanduser("~/irc-bots/TriviaBot/data/dicerpg.db")

# ── Shared state ──────────────────────────────────────────────────────────────
lock    = threading.Lock()
users   = {ch: set()  for ch in CHANNELS}   # ch → set of nicks
msgs    = {ch: collections.deque(maxlen=MAX_MSGS) for ch in CHANNELS}
irc_connected = False
connect_time  = 0.0
irc_sock      = None

def ts():
    return time.strftime("%H:%M:%S")

# ── IRC client ────────────────────────────────────────────────────────────────
def send(sock, line):
    try:
        sock.sendall((line + "\r\n").encode())
    except Exception:
        pass

def irc_thread():
    global irc_connected, connect_time, irc_sock
    while True:
        try:
            raw = socket.create_connection((IRC_HOST, IRC_PORT), timeout=15)
            ctx = ssl.create_default_context()
            ctx.check_hostname = False
            ctx.verify_mode    = ssl.CERT_NONE
            sock = ctx.wrap_socket(raw, server_hostname=IRC_HOST)
            irc_sock = sock
            send(sock, f"PASS {IRC_PASS}")
            send(sock, f"NICK {IRC_NICK}")
            send(sock, f"USER {IRC_USER} 0 * :{IRC_REAL}")

            buf = ""
            logged_in = False
            while True:
                chunk = sock.recv(4096).decode(errors="replace")
                if not chunk:
                    break
                buf += chunk
                while "\r\n" in buf:
                    line, buf = buf.split("\r\n", 1)
                    handle_line(sock, line, logged_in)
                    if not logged_in and (" 001 " in line or " 376 " in line or " 422 " in line):
                        logged_in = True
                        with lock:
                            irc_connected = True
                            connect_time  = time.time()
                        for ch in CHANNELS:
                            send(sock, f"JOIN {ch}")
        except Exception as e:
            print(f"[irc] disconnected: {e}")
        finally:
            with lock:
                irc_connected = False
                irc_sock = None
                # Keep user lists and messages — stale data is better than blank
        time.sleep(10)

def handle_line(sock, line, logged_in):
    parts = line.split()
    if not parts:
        return
    if parts[0] == "PING":
        send(sock, "PONG " + parts[1] if len(parts) > 1 else "PONG")
        return

    # :nick!user@host prefix
    prefix = parts[0][1:] if parts[0].startswith(":") else ""
    nick   = prefix.split("!")[0] if "!" in prefix else prefix
    cmd    = parts[1] if len(parts) > 1 else ""
    target = parts[2] if len(parts) > 2 else ""

    if cmd == "353":  # NAMES reply
        ch = parts[4] if len(parts) > 4 else target
        ch = ch.lstrip("=@*").strip()
        nicks_raw = line.split(":", 2)[-1].split()
        ch_lower = ch.lower()
        for c in CHANNELS:
            if c.lower() == ch_lower:
                with lock:
                    for n in nicks_raw:
                        users[c].add(n.lstrip("@+~&!"))
                break

    elif cmd == "JOIN":
        ch = target.lstrip(":") if not target.startswith("#") else target
        ch = parts[2].lstrip(":") if len(parts) > 2 else ch
        with lock:
            for c in CHANNELS:
                if c.lower() == ch.lower():
                    users[c].add(nick)
                    msgs[c].append({"t": ts(), "nick": "*", "msg": f"{nick} joined"})
                    break

    elif cmd == "PART":
        ch = target
        with lock:
            for c in CHANNELS:
                if c.lower() == ch.lower():
                    users[c].discard(nick)
                    msgs[c].append({"t": ts(), "nick": "*", "msg": f"{nick} left"})
                    break

    elif cmd == "QUIT":
        reason = " ".join(parts[2:]).lstrip(":") if len(parts) > 2 else ""
        with lock:
            for c in CHANNELS:
                users[c].discard(nick)

    elif cmd == "KICK":
        ch       = target
        kicked   = parts[3] if len(parts) > 3 else "?"
        with lock:
            for c in CHANNELS:
                if c.lower() == ch.lower():
                    users[c].discard(kicked)
                    msgs[c].append({"t": ts(), "nick": "*", "msg": f"{kicked} was kicked by {nick}"})
                    break

    elif cmd == "NICK":
        new_nick = parts[2].lstrip(":") if len(parts) > 2 else "?"
        with lock:
            for c in CHANNELS:
                if nick in users[c]:
                    users[c].discard(nick)
                    users[c].add(new_nick)

    elif cmd == "PRIVMSG":
        ch  = target
        msg = " ".join(parts[3:]).lstrip(":")
        with lock:
            for c in CHANNELS:
                if c.lower() == ch.lower():
                    msgs[c].append({"t": ts(), "nick": nick, "msg": msg})
                    break

# ── ZPG leaderboard ───────────────────────────────────────────────────────────
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

# ── HTTP handler ──────────────────────────────────────────────────────────────
class Handler(BaseHTTPRequestHandler):
    def log_message(self, *a): pass

    def do_GET(self):
        if self.path in ("/irc/state", "/irc/state/"):
            with lock:
                data = {
                    "connected":    irc_connected,
                    "uptime_s":     int(time.time() - connect_time) if irc_connected else 0,
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
    threading.Thread(target=irc_thread, daemon=True).start()
    print(f"[irc-dash] HTTP on :{HTTP_PORT}")
    HTTPServer(("127.0.0.1", HTTP_PORT), Handler).serve_forever()
