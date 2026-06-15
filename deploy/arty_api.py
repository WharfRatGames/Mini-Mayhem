#!/usr/bin/env python3
import socket, sqlite3, hashlib, json, time, os, random, string, threading, math, subprocess, secrets
PORT = 7778
DB = os.path.expanduser("~/mayhem-server/arty.db")
_key_file = os.path.expanduser("~/mayhem-server/admin_key.txt")
ADMIN_KEY = open(_key_file).read().strip() if os.path.exists(_key_file) else os.environ.get("ARTY_ADMIN_KEY", "changeme")

# ── DB init ────────────────────────────────────────────────────────────────────

def init_db(c):
    c.executescript("""
        CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, username TEXT UNIQUE NOT NULL, pw_hash TEXT NOT NULL, token TEXT);
        CREATE TABLE IF NOT EXISTS matches (id INTEGER PRIMARY KEY, code TEXT UNIQUE NOT NULL, p0 INTEGER, p1 INTEGER, seed INTEGER NOT NULL, moves TEXT NOT NULL DEFAULT '[]', turn INTEGER NOT NULL DEFAULT 0, done INTEGER NOT NULL DEFAULT 0);
        CREATE TABLE IF NOT EXISTS rosters (
            id INTEGER PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id),
            name TEXT NOT NULL,
            worm_names TEXT NOT NULL DEFAULT '["Soldier 1","Soldier 2","Soldier 3","Soldier 4"]'
        );
        CREATE TABLE IF NOT EXISTS ranked_pool (
            id INTEGER PRIMARY KEY,
            user_id INTEGER NOT NULL,
            elo INTEGER NOT NULL,
            joined_at INTEGER NOT NULL,
            match_id INTEGER DEFAULT NULL
        );
        CREATE TABLE IF NOT EXISTS live_queue (
            id INTEGER PRIMARY KEY,
            user_id INTEGER NOT NULL,
            elo INTEGER NOT NULL,
            joined_at INTEGER NOT NULL,
            paired_with INTEGER DEFAULT NULL,
            game_token TEXT DEFAULT NULL
        );
        CREATE TABLE IF NOT EXISTS casual_pool (
            id INTEGER PRIMARY KEY,
            user_id INTEGER NOT NULL,
            joined_at INTEGER NOT NULL,
            match_id INTEGER DEFAULT NULL
        );
    """)
    # Migrate existing columns safely
    for col, defn in [
        ("elo",    "INTEGER DEFAULT 1000"),
        ("wins",   "INTEGER DEFAULT 0"),
        ("losses", "INTEGER DEFAULT 0"),
    ]:
        try: c.execute(f"ALTER TABLE users ADD COLUMN {col} {defn}")
        except: pass
    for col, defn in [
        ("ranked",          "INTEGER DEFAULT 0"),
        ("winner",          "INTEGER DEFAULT NULL"),
        ("has_mines",       "INTEGER DEFAULT 0"),
        ("has_barrels",     "INTEGER DEFAULT 0"),
        ("turn_started_at", "INTEGER DEFAULT 0"),
        ("turn_timeout",    "INTEGER DEFAULT 0"),
        ("p0_kills",        "INTEGER DEFAULT 0"),
        ("p0_deaths",       "INTEGER DEFAULT 0"),
        ("p1_kills",        "INTEGER DEFAULT 0"),
        ("p1_deaths",       "INTEGER DEFAULT 0"),
        ("mode",            "TEXT DEFAULT 'tat'"),
        ("p0_weapon_kills",  "TEXT DEFAULT '{}'"),
        ("p1_weapon_kills",  "TEXT DEFAULT '{}'"),
    ]:
        try: c.execute(f"ALTER TABLE matches ADD COLUMN {col} {defn}")
        except: pass
    # Currency columns (scrap = soft, warbonds = premium)
    for col, defn in [
        ("scrap",         "INTEGER DEFAULT 0"),
        ("warbonds",        "INTEGER DEFAULT 0"),
        ("last_login_date", "TEXT DEFAULT NULL"),
        ("daily_streak",    "INTEGER DEFAULT 0"),
        # Legacy — kept so old data isn't lost, no longer written
        ("xp",              "INTEGER DEFAULT 0"),
        ("level",           "INTEGER DEFAULT 1"),
        ("coins",           "INTEGER DEFAULT 0"),
        ("gems",            "INTEGER DEFAULT 0"),
        ("last_win_date",   "TEXT DEFAULT NULL"),
    ]:
        try: c.execute(f"ALTER TABLE users ADD COLUMN {col} {defn}")
        except: pass
    # One-time migration: move any existing coins/gems into scrap/warbonds
    c.execute("UPDATE users SET scrap=scrap+coins, coins=0 WHERE coins>0")
    c.execute("UPDATE users SET warbonds=warbonds+gems, gems=0 WHERE gems>0")
    # Roster cosmetic columns
    for col, defn in [
        ("avatar_id",        "INTEGER DEFAULT 0"),
        ("headstone_id",     "INTEGER DEFAULT 0"),
        ("hat_ids",          "TEXT DEFAULT '[0,0,0,0]'"),
        ("gun_color_ids",    "TEXT DEFAULT '[0,0,0,0]'"),  # legacy name kept for migration
        ("gun_style_ids",    "TEXT DEFAULT '[0,0,0,0]'"),
        ("uniform_color_ids","TEXT DEFAULT '[0,0,0,0]'"),
        ("boot_color_ids",   "TEXT DEFAULT '[0,0,0,0]'"),
    ]:
        try: c.execute(f"ALTER TABLE rosters ADD COLUMN {col} {defn}")
        except: pass
    # One-time migration: copy gun_color_ids → gun_style_ids
    c.execute("UPDATE rosters SET gun_style_ids=gun_color_ids WHERE gun_color_ids!='[0,0,0,0]' AND gun_style_ids='[0,0,0,0]'")
    c.executescript("""
        CREATE TABLE IF NOT EXISTS player_cosmetics (
            user_id     INTEGER NOT NULL,
            cosm_type   TEXT NOT NULL,
            cosm_id     INTEGER NOT NULL,
            unlocked_at INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (user_id, cosm_type, cosm_id)
        );
        CREATE TABLE IF NOT EXISTS warbond_transactions (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id     INTEGER NOT NULL,
            warbonds    INTEGER NOT NULL,
            reason      TEXT NOT NULL,
            created_at  INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS player_challenges (
            user_id      INTEGER NOT NULL,
            challenge_id TEXT NOT NULL,
            period       TEXT NOT NULL,
            progress     INTEGER NOT NULL DEFAULT 0,
            claimed      INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (user_id, challenge_id, period)
        );
    """)
    # One-time cleanup: remove pool rows for already-finished matches (historical accumulation)
    c.execute("DELETE FROM ranked_pool WHERE match_id IS NOT NULL AND match_id IN (SELECT id FROM matches WHERE done=1)")
    c.execute("DELETE FROM casual_pool WHERE match_id IS NOT NULL AND match_id IN (SELECT id FROM matches WHERE done=1)")
    c.commit()

# ── ELO ───────────────────────────────────────────────────────────────────────

RANK_NAMES = [
    (2000, "Commander"), (1800, "Major"), (1600, "Captain"),
    (1400, "Lieutenant"), (1200, "Sergeant"), (1000, "Corporal"),
    (800,  "Private"),   (0,    "Recruit"),
]

def rank_name(elo):
    for threshold, name in RANK_NAMES:
        if elo >= threshold: return name
    return "Recruit"

def update_elo(db, uid_winner, uid_loser):
    row_w = db.execute("SELECT elo FROM users WHERE id=?", (uid_winner,)).fetchone()
    row_l = db.execute("SELECT elo FROM users WHERE id=?", (uid_loser,)).fetchone()
    if not row_w or not row_l: return 0, 0
    w_elo, l_elo = row_w[0] or 1000, row_l[0] or 1000
    K = 32
    E_w = 1 / (1 + 10 ** ((l_elo - w_elo) / 400))
    new_w = max(100, round(w_elo + K * (1 - E_w)))
    new_l = max(100, round(l_elo + K * (0 - (1 - E_w))))
    db.execute("UPDATE users SET elo=?, wins=wins+1 WHERE id=?", (new_w, uid_winner))
    db.execute("UPDATE users SET elo=?, losses=losses+1 WHERE id=?", (new_l, uid_loser))
    db.commit()
    return new_w - w_elo, new_l - l_elo   # deltas

# ── Shop catalog ─────────────────────────────────────────────────────────────

SHOP_CATALOG = [
    {"type":"hat","id":1, "name":"Top Hat",       "cost_scrap":200,"cost_warbonds":0},
    {"type":"hat","id":2, "name":"Propeller Hat", "cost_scrap":350,"cost_warbonds":0},
    {"type":"hat","id":3, "name":"Flower",        "cost_scrap":150,"cost_warbonds":0},
    {"type":"hat","id":4, "name":"Crown",         "cost_scrap":400,"cost_warbonds":0},
    {"type":"hat","id":5, "name":"Fez",           "cost_scrap":250,"cost_warbonds":0},
    {"type":"hat","id":6, "name":"Beret",         "cost_scrap":200,"cost_warbonds":0},
    {"type":"hat","id":7, "name":"Party Hat",     "cost_scrap":200,"cost_warbonds":0},
    {"type":"hat","id":8, "name":"Halo",          "cost_scrap":500,"cost_warbonds":0},
    {"type":"hat","id":9, "name":"Devil Horns",   "cost_scrap":500,"cost_warbonds":0},
    {"type":"hat","id":10,"name":"Gold Crown",    "cost_scrap":0,  "cost_warbonds":50},
    {"type":"hat","id":11,"name":"Laurel Wreath", "cost_scrap":0,  "cost_warbonds":30},
    {"type":"hat","id":12,"name":"Blue Party Hat","cost_scrap":200,"cost_warbonds":0},
    {"type":"hat","id":13,"name":"Cowboy Hat",    "cost_scrap":350,"cost_warbonds":0},
    {"type":"hat","id":14,"name":"Pirate Hat",    "cost_scrap":500,"cost_warbonds":0},
    {"type":"hat","id":15,"name":"Viking Helm",   "cost_scrap":550,"cost_warbonds":0},
    # Gun styles (full shape replacement, not just color)
    {"type":"gun_style","id":1,"name":"Pistol",      "cost_scrap":200,"cost_warbonds":0},
    {"type":"gun_style","id":2,"name":"Shotgun",     "cost_scrap":300,"cost_warbonds":0},
    {"type":"gun_style","id":3,"name":"Sniper",      "cost_scrap":400,"cost_warbonds":0},
    {"type":"gun_style","id":4,"name":"Minigun",     "cost_scrap":500,"cost_warbonds":0},
    {"type":"gun_style","id":5,"name":"Cannon",      "cost_scrap":500,"cost_warbonds":0},
    {"type":"gun_style","id":6,"name":"Laser",       "cost_scrap":0,  "cost_warbonds":30},
    {"type":"gun_style","id":7,"name":"Golden Gun",  "cost_scrap":0,  "cost_warbonds":40},
    {"type":"gun_style","id":8,"name":"Revolver",        "cost_scrap":350,"cost_warbonds":0},
    {"type":"gun_style","id":9,"name":"Flamethrower",    "cost_scrap":650,"cost_warbonds":0},
    {"type":"gun_style","id":10,"name":"Rocket Launcher","cost_scrap":800,"cost_warbonds":0},
    # Uniform colors
    {"type":"uniform","id":1,"name":"Camo Green",   "cost_scrap":200,"cost_warbonds":0},
    {"type":"uniform","id":2,"name":"Desert Tan",   "cost_scrap":200,"cost_warbonds":0},
    {"type":"uniform","id":3,"name":"Midnight Black","cost_scrap":300,"cost_warbonds":0},
    {"type":"uniform","id":4,"name":"Snow White",   "cost_scrap":300,"cost_warbonds":0},
    {"type":"uniform","id":5,"name":"Navy",         "cost_scrap":250,"cost_warbonds":0},
    {"type":"uniform","id":6,"name":"Pink Camo",    "cost_scrap":0,  "cost_warbonds":30},
    {"type":"uniform","id":7,"name":"Gold Plate",   "cost_scrap":0,  "cost_warbonds":40},
    # Boot colors
    {"type":"boots","id":1,"name":"Red Boots",       "cost_scrap":100,"cost_warbonds":0},
    {"type":"boots","id":2,"name":"White Boots",     "cost_scrap":100,"cost_warbonds":0},
    {"type":"boots","id":3,"name":"Gold Boots",      "cost_scrap":150,"cost_warbonds":0},
    {"type":"boots","id":4,"name":"Combat Green",    "cost_scrap":100,"cost_warbonds":0},
    {"type":"boots","id":5,"name":"Electric Blue",   "cost_scrap":0,  "cost_warbonds":20},
    # Headstones: IDs 0-5 are all default (free, no purchase). New reward styles added here later.
]

# ── Challenges ───────────────────────────────────────────────────────────────

DAILY_CHALLENGES = [
    {"id":"d_play",    "desc":"Play any match",       "stat":"matches", "target":1,  "scrap":30},
    {"id":"d_win",     "desc":"Win a match",           "stat":"wins",    "target":1,  "scrap":60},
    {"id":"d_kills",   "desc":"Get 3 kills",           "stat":"kills",   "target":3,  "scrap":45},
]
WEEKLY_CHALLENGES = [
    {"id":"w_play",    "desc":"Play 5 matches",        "stat":"matches", "target":5,  "scrap":150},
    {"id":"w_win",     "desc":"Win 3 matches",         "stat":"wins",    "target":3,  "scrap":250},
    {"id":"w_kills",   "desc":"Get 10 kills",          "stat":"kills",   "target":10, "scrap":200},
]
ALL_CHALLENGES = {c["id"]: c for c in DAILY_CHALLENGES + WEEKLY_CHALLENGES}

def daily_period():  return time.strftime("%Y-%m-%d")
def weekly_period(): return time.strftime("%Y-W%W")

def update_challenges(db, uid2, matches=0, wins=0, kills=0):
    dp = daily_period(); wp = weekly_period()
    for ch in DAILY_CHALLENGES:
        val = {"matches":matches,"wins":wins,"kills":kills}.get(ch["stat"],0)
        if val <= 0: continue
        db.execute("""INSERT INTO player_challenges(user_id,challenge_id,period,progress)
                      VALUES(?,?,?,?) ON CONFLICT(user_id,challenge_id,period)
                      DO UPDATE SET progress=MIN(progress+excluded.progress,?)""",
                   (uid2, ch["id"], dp, val, ch["target"]))
    for ch in WEEKLY_CHALLENGES:
        val = {"matches":matches,"wins":wins,"kills":kills}.get(ch["stat"],0)
        if val <= 0: continue
        db.execute("""INSERT INTO player_challenges(user_id,challenge_id,period,progress)
                      VALUES(?,?,?,?) ON CONFLICT(user_id,challenge_id,period)
                      DO UPDATE SET progress=MIN(progress+excluded.progress,?)""",
                   (uid2, ch["id"], wp, val, ch["target"]))
    db.commit()

# Daily/weekly login bonus
DAILY_LOGIN_SCRAP  = 25
WEEKLY_LOGIN_SCRAP = 150   # awarded when streak reaches a 7-day multiple

# ── Helpers ───────────────────────────────────────────────────────────────────

def ensure_default_roster(db, uid):
    n = db.execute("SELECT COUNT(*) FROM rosters WHERE user_id=?", (uid,)).fetchone()[0]
    if n == 0:
        uname = db.execute("SELECT username FROM users WHERE id=?", (uid,)).fetchone()[0]
        names = json.dumps([f"Soldier {i+1}" for i in range(4)])
        db.execute("INSERT INTO rosters(user_id,name,worm_names) VALUES(?,?,?)",
                   (uid, f"{uname}'s Team", names))
        db.commit()

def hash_pw(pw):
    """PBKDF2-HMAC-SHA256 with random salt. Format: pbkdf2$<salt>$<hex>"""
    salt = secrets.token_hex(16)
    h = hashlib.pbkdf2_hmac('sha256', pw.encode(), salt.encode(), 100000)
    return f"pbkdf2${salt}${h.hex()}"

def check_pw(pw, stored):
    """Verify password. Supports PBKDF2 (new) and legacy unsalted SHA-256."""
    if stored.startswith('pbkdf2$'):
        _, salt, hashed = stored.split('$', 2)
        h = hashlib.pbkdf2_hmac('sha256', pw.encode(), salt.encode(), 100000)
        return secrets.compare_digest(h.hex(), hashed)
    # Legacy: unsalted sha256 — still works, hash upgraded on next login
    return secrets.compare_digest(stored, hashlib.sha256(pw.encode()).hexdigest())

def gen_token(u): return hashlib.sha256(f"{u}{time.time_ns()}".encode()).hexdigest()
def gen_code(): return ''.join(random.choices(string.ascii_uppercase, k=6))
def gen_game_token(): return ''.join(random.choices(string.ascii_letters + string.digits, k=24))

def read_req(s):
    data = b''
    s.settimeout(3)
    try:
        while True:
            chunk = s.recv(4096)
            if not chunk: break
            data += chunk
            if b'\r\n\r\n' in data or b'\n\n' in data: break
    except: pass
    req = data.decode('utf-8', errors='replace')
    lines = req.split('\n')
    first = lines[0].strip().split()
    if len(first) < 2: return None, None, None, None
    method, path = first[0], first[1].split('?')[0]
    qs = first[1].split('?')[1] if '?' in first[1] else ''
    sep = req.find('\r\n\r\n')
    if sep == -1: sep = req.find('\n\n')
    body = req[sep+4:].strip() if sep != -1 else ''
    return method, path, body, qs

def send_json(s, status, obj):
    body = json.dumps(obj)
    resp = f"HTTP/1.0 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {len(body)}\r\n\r\n{body}"
    try: s.sendall(resp.encode())
    except: pass

# ── Request handler ───────────────────────────────────────────────────────────

def handle(db, sock):
    method, path, body, qs = read_req(sock)
    if not method: sock.close(); return
    try: data = json.loads(body) if body else {}
    except: data = {}
    qs_params = dict(p.split('=', 1) for p in qs.split('&') if '=' in p)

    def uid(token):
        r = db.execute("SELECT id FROM users WHERE token=?", (token,)).fetchone()
        return r[0] if r else None

    def uname(u):
        r = db.execute("SELECT username FROM users WHERE id=?", (u,)).fetchone()
        return r[0] if r else "unknown"

    def rosters_for(u):
        rows = db.execute(
            "SELECT id,name,worm_names,avatar_id,headstone_id,hat_ids,gun_style_ids,uniform_color_ids,boot_color_ids FROM rosters WHERE user_id=? ORDER BY id", (u,)
        ).fetchall()
        return [{"id": r[0], "name": r[1], "worm_names": json.loads(r[2]),
                 "avatar_id": r[3] or 0, "headstone_id": r[4] or 0,
                 "hat_ids":           json.loads(r[5] or "[0,0,0,0]"),
                 "gun_style_ids":     json.loads(r[6] or "[0,0,0,0]"),
                 "uniform_color_ids": json.loads(r[7] or "[0,0,0,0]"),
                 "boot_color_ids":    json.loads(r[8] or "[0,0,0,0]")} for r in rows]

    def get_elo(u):
        r = db.execute("SELECT elo FROM users WHERE id=?", (u,)).fetchone()
        return (r[0] or 1000) if r else 1000

    token = data.get("token") or qs_params.get("token")

    # ── Auth ──────────────────────────────────────────────────────────────────

    if method == "POST" and path == "/register":
        u = data.get("username","").strip()
        p = data.get("password","")
        if not u or not p: send_json(sock, 400, {"error":"missing fields"}); return
        t = gen_token(u)
        if db.execute("SELECT id FROM users WHERE lower(username)=lower(?)", (u,)).fetchone():
            send_json(sock, 409, {"error":"username taken"}); return
        try:
            db.execute("INSERT INTO users(username,pw_hash,token) VALUES(?,?,?)", (u, hash_pw(p), t))
            db.commit()
            uid2 = db.execute("SELECT id FROM users WHERE lower(username)=lower(?)", (u,)).fetchone()[0]
            ensure_default_roster(db, uid2)
            send_json(sock, 200, {"token": t, "username": u, "rosters": rosters_for(uid2)})
        except Exception: send_json(sock, 409, {"error":"username taken"})

    elif method == "POST" and path == "/login":
        u = data.get("username","").strip(); p = data.get("password","")
        t = gen_token(u)
        row2 = db.execute("SELECT id,username,pw_hash FROM users WHERE lower(username)=lower(?)", (u,)).fetchone()
        if row2 and check_pw(p, row2[2]):
            uid2, u = row2[0], row2[1]
            # Transparently upgrade legacy SHA-256 hash to PBKDF2 on login
            if not row2[2].startswith('pbkdf2$'):
                db.execute("UPDATE users SET pw_hash=? WHERE id=?", (hash_pw(p), uid2))
            db.execute("UPDATE users SET token=? WHERE id=?", (t, uid2))
            db.commit()
            n = 1
        else:
            n = 0
        if n == 1:
            ensure_default_roster(db, uid2)
            send_json(sock, 200, {"token": t, "username": u, "rosters": rosters_for(uid2)})
        else: send_json(sock, 401, {"error":"invalid credentials"})

    # ── Profile & leaderboard ─────────────────────────────────────────────────

    elif method == "GET" and path == "/profile":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        row = db.execute("SELECT elo,wins,losses,scrap,warbonds FROM users WHERE id=?", (uid2,)).fetchone()
        elo, wins, losses = (row[0] or 1000), (row[1] or 0), (row[2] or 0)
        scrap_val, warbonds_val = (row[3] or 0), (row[4] or 0)
        unlocked = [(r[0],r[1]) for r in db.execute(
            "SELECT cosm_type,cosm_id FROM player_cosmetics WHERE user_id=?", (uid2,)).fetchall()]
        send_json(sock, 200, {
            "elo": elo, "wins": wins, "losses": losses, "rank": rank_name(elo),
            "scrap": scrap_val, "warbonds": warbonds_val,
            "unlocked_hats":       [cid for ctype,cid in unlocked if ctype=="hat"],
            "unlocked_gun_styles": [cid for ctype,cid in unlocked if ctype=="gun_style"],
            "unlocked_uniforms":   [cid for ctype,cid in unlocked if ctype=="uniform"],
            "unlocked_boots":      [cid for ctype,cid in unlocked if ctype=="boots"],
            "unlocked_headstones": [cid for ctype,cid in unlocked if ctype=="headstone"],
        })

    elif method == "GET" and path == "/shop/catalog":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        unlocked = {(r[0],r[1]) for r in db.execute(
            "SELECT cosm_type,cosm_id FROM player_cosmetics WHERE user_id=?", (uid2,)).fetchall()}
        send_json(sock, 200, [{**item, "owned": (item["type"], item["id"]) in unlocked}
                              for item in SHOP_CATALOG])

    elif method == "POST" and path == "/shop/buy":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        cosm_type = data.get("cosmetic_type","")
        cosm_id   = int(data.get("cosmetic_id", 0))
        item = next((x for x in SHOP_CATALOG if x["type"]==cosm_type and x["id"]==cosm_id), None)
        if not item: send_json(sock, 404, {"error":"item not found"}); return
        if db.execute("SELECT 1 FROM player_cosmetics WHERE user_id=? AND cosm_type=? AND cosm_id=?",
                      (uid2, cosm_type, cosm_id)).fetchone():
            send_json(sock, 400, {"error":"already owned"}); return
        row = db.execute("SELECT scrap,warbonds FROM users WHERE id=?", (uid2,)).fetchone()
        scrap_val, warbonds_val = (row[0] or 0), (row[1] or 0)
        if item["cost_warbonds"] > 0:
            if warbonds_val < item["cost_warbonds"]:
                send_json(sock, 400, {"error":"not enough warbonds"}); return
            db.execute("UPDATE users SET warbonds=warbonds-? WHERE id=?", (item["cost_warbonds"], uid2))
        else:
            if scrap_val < item["cost_scrap"]:
                send_json(sock, 400, {"error":"not enough scrap"}); return
            db.execute("UPDATE users SET scrap=scrap-? WHERE id=?", (item["cost_scrap"], uid2))
        db.execute("INSERT INTO player_cosmetics(user_id,cosm_type,cosm_id,unlocked_at) VALUES(?,?,?,?)",
                   (uid2, cosm_type, cosm_id, int(time.time())))
        db.commit()
        row2 = db.execute("SELECT scrap,warbonds FROM users WHERE id=?", (uid2,)).fetchone()
        send_json(sock, 200, {"ok": True, "new_scrap": row2[0] or 0, "new_warbonds": row2[1] or 0})

    elif method == "POST" and path == "/admin/grant_warbonds":
        # Called by the payment backend after a successful purchase.
        # Body: {admin_key, username, warbonds, reason}
        if data.get("admin_key") != ADMIN_KEY:
            send_json(sock, 403, {"error":"forbidden"}); return
        username = data.get("username","").strip()
        wb_amount = int(data.get("warbonds", 0))
        reason = str(data.get("reason", "purchase"))[:128]
        if not username or wb_amount <= 0:
            send_json(sock, 400, {"error":"missing username or warbonds"}); return
        row = db.execute("SELECT id FROM users WHERE lower(username)=lower(?)", (username,)).fetchone()
        if not row: send_json(sock, 404, {"error":"user not found"}); return
        target_uid = row[0]
        db.execute("UPDATE users SET warbonds=warbonds+? WHERE id=?", (wb_amount, target_uid))
        db.execute("INSERT INTO warbond_transactions(user_id,warbonds,reason,created_at) VALUES(?,?,?,?)",
                   (target_uid, wb_amount, reason, int(time.time())))
        db.commit()
        new_wb = db.execute("SELECT warbonds FROM users WHERE id=?", (target_uid,)).fetchone()[0] or 0
        send_json(sock, 200, {"ok": True, "warbonds_granted": wb_amount, "new_warbond_total": new_wb})

    elif method == "POST" and path == "/player/daily_login":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        row = db.execute("SELECT last_login_date,daily_streak FROM users WHERE id=?", (uid2,)).fetchone()
        last_date, streak = (row[0] or ""), (row[1] or 0)
        today = time.strftime("%Y-%m-%d")
        yesterday = time.strftime("%Y-%m-%d", time.gmtime(time.time() - 86400))
        if last_date == today:
            send_json(sock, 200, {"scrap_awarded": 0, "streak": streak, "already_claimed": True}); return
        streak = (streak + 1) if last_date == yesterday else 1
        scrap = DAILY_LOGIN_SCRAP
        weekly_bonus = WEEKLY_LOGIN_SCRAP if streak % 7 == 0 else 0
        scrap += weekly_bonus
        db.execute("UPDATE users SET last_login_date=?,daily_streak=?,scrap=scrap+? WHERE id=?",
                   (today, streak, scrap, uid2))
        db.commit()
        send_json(sock, 200, {"scrap_awarded": scrap, "streak": streak,
                              "weekly_bonus": weekly_bonus, "already_claimed": False})

    elif method == "GET" and path == "/challenges":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        dp = daily_period(); wp = weekly_period()
        rows = db.execute(
            "SELECT challenge_id,period,progress,claimed FROM player_challenges WHERE user_id=?", (uid2,)
        ).fetchall()
        prog = {(r[0],r[1]): (r[2],r[3]) for r in rows}
        out = []
        for ch in DAILY_CHALLENGES:
            p, claimed = prog.get((ch["id"], dp), (0, 0))
            out.append({**ch, "period_type":"daily", "period":dp, "progress":p, "claimed":bool(claimed)})
        for ch in WEEKLY_CHALLENGES:
            p, claimed = prog.get((ch["id"], wp), (0, 0))
            out.append({**ch, "period_type":"weekly", "period":wp, "progress":p, "claimed":bool(claimed)})
        send_json(sock, 200, out)

    elif method == "POST" and path == "/challenges/claim":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        cid = data.get("challenge_id","")
        ch = ALL_CHALLENGES.get(cid)
        if not ch: send_json(sock, 404, {"error":"unknown challenge"}); return
        period = daily_period() if cid.startswith("d_") else weekly_period()
        row = db.execute(
            "SELECT progress,claimed FROM player_challenges WHERE user_id=? AND challenge_id=? AND period=?",
            (uid2, cid, period)
        ).fetchone()
        if not row or row[0] < ch["target"]:
            send_json(sock, 400, {"error":"challenge not complete"}); return
        if row[1]: send_json(sock, 400, {"error":"already claimed"}); return
        db.execute("UPDATE player_challenges SET claimed=1 WHERE user_id=? AND challenge_id=? AND period=?",
                   (uid2, cid, period))
        db.execute("UPDATE users SET scrap=scrap+? WHERE id=?", (ch["scrap"], uid2))
        db.commit()
        new_scrap = db.execute("SELECT scrap FROM users WHERE id=?", (uid2,)).fetchone()[0] or 0
        send_json(sock, 200, {"ok": True, "scrap_earned": ch["scrap"], "new_scrap": new_scrap})

    elif method == "GET" and path in ("/leaderboard", "/leaderboard/casual"):
        ranked_flag = 1 if path == "/leaderboard" else 0
        win_rows = db.execute("""
            SELECT u.username,
                   COALESCE(SUM(CASE WHEN m.winner=u.id THEN 1 ELSE 0 END),0) as wins,
                   COALESCE(SUM(CASE WHEN m.winner IS NOT NULL AND m.winner!=u.id THEN 1 ELSE 0 END),0) as losses,
                   u.elo
            FROM users u
            JOIN matches m ON (m.p0=u.id OR m.p1=u.id) AND m.done=1 AND m.ranked=?
            GROUP BY u.id
            ORDER BY wins DESC LIMIT 50
        """, (ranked_flag,)).fetchall()
        kill_rows = db.execute("""
            SELECT u.username,
                   COALESCE(SUM(CASE WHEN m.p0=u.id THEN m.p0_kills ELSE m.p1_kills END),0) as kills
            FROM users u
            JOIN matches m ON (m.p0=u.id OR m.p1=u.id) AND m.done=1 AND m.ranked=?
            GROUP BY u.id
            ORDER BY kills DESC LIMIT 50
        """, (ranked_flag,)).fetchall()
        wins_list  = [{"username":r[0],"wins":r[1],"losses":r[2],"elo":r[3] or 1000,"rank":rank_name(r[3] or 1000)} for r in win_rows]
        kills_list = [{"username":r[0],"kills":r[1]} for r in kill_rows]
        out = {"wins": wins_list, "kills": kills_list}
        my_uid = uid(token) if token else None
        if my_uid:
            my_uname = uname(my_uid)
            # my wins rank
            my_w_row = db.execute("""
                SELECT COALESCE(SUM(CASE WHEN m.winner=u.id THEN 1 ELSE 0 END),0),
                       COALESCE(SUM(CASE WHEN m.winner IS NOT NULL AND m.winner!=u.id THEN 1 ELSE 0 END),0),
                       u.elo
                FROM users u LEFT JOIN matches m ON (m.p0=u.id OR m.p1=u.id) AND m.done=1 AND m.ranked=?
                WHERE u.id=? GROUP BY u.id
            """, (ranked_flag, my_uid)).fetchone()
            if my_w_row:
                my_w, my_l, my_elo_val = my_w_row[0], my_w_row[1], (my_w_row[2] or 1000)
                pos_row = db.execute("""
                    SELECT COUNT(*)+1 FROM (
                        SELECT COALESCE(SUM(CASE WHEN m.winner=u.id THEN 1 ELSE 0 END),0) as w
                        FROM users u JOIN matches m ON (m.p0=u.id OR m.p1=u.id) AND m.done=1 AND m.ranked=?
                        GROUP BY u.id HAVING w > ?
                    )
                """, (ranked_flag, my_w)).fetchone()
                out["me_wins"] = {"username": my_uname, "wins": my_w, "losses": my_l,
                                  "elo": my_elo_val, "rank": rank_name(my_elo_val),
                                  "pos": pos_row[0] if pos_row else 1}
            # my kills rank
            my_k_row = db.execute("""
                SELECT COALESCE(SUM(CASE WHEN m.p0=u.id THEN m.p0_kills ELSE m.p1_kills END),0)
                FROM users u LEFT JOIN matches m ON (m.p0=u.id OR m.p1=u.id) AND m.done=1 AND m.ranked=?
                WHERE u.id=? GROUP BY u.id
            """, (ranked_flag, my_uid)).fetchone()
            if my_k_row:
                my_k = my_k_row[0]
                kpos_row = db.execute("""
                    SELECT COUNT(*)+1 FROM (
                        SELECT COALESCE(SUM(CASE WHEN m.p0=u.id THEN m.p0_kills ELSE m.p1_kills END),0) as k
                        FROM users u JOIN matches m ON (m.p0=u.id OR m.p1=u.id) AND m.done=1 AND m.ranked=?
                        GROUP BY u.id HAVING k > ?
                    )
                """, (ranked_flag, my_k)).fetchone()
                out["me_kills"] = {"username": my_uname, "kills": my_k,
                                   "pos": kpos_row[0] if kpos_row else 1}
        send_json(sock, 200, out)

    # ── Rosters ───────────────────────────────────────────────────────────────

    elif method == "GET" and path == "/rosters":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        send_json(sock, 200, rosters_for(uid2))

    elif method == "POST" and path == "/rosters/create":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        name = data.get("name","My Team").strip()[:32]
        wn = data.get("worm_names", ["Soldier 1","Soldier 2","Soldier 3","Soldier 4"])
        wn = [str(n)[:16] for n in wn[:4]]
        while len(wn) < 4: wn.append(f"Soldier {len(wn)+1}")
        av = int(data.get("avatar_id", 0))
        hs = int(data.get("headstone_id", 0))
        def parse_arr(key): return json.dumps([(int(x) if isinstance(x,(int,float)) else 0) for x in (data.get(key) or [0,0,0,0])][:4])
        hats     = parse_arr("hat_ids")
        guns     = parse_arr("gun_style_ids")
        uniforms = parse_arr("uniform_color_ids")
        boots    = parse_arr("boot_color_ids")
        db.execute("INSERT INTO rosters(user_id,name,worm_names,avatar_id,headstone_id,hat_ids,gun_style_ids,uniform_color_ids,boot_color_ids) VALUES(?,?,?,?,?,?,?,?,?)",
                   (uid2, name, json.dumps(wn), av, hs, hats, guns, uniforms, boots))
        db.commit()
        rid = db.execute("SELECT last_insert_rowid()").fetchone()[0]
        send_json(sock, 200, {"id": rid, "name": name, "worm_names": wn,
                              "avatar_id": av, "headstone_id": hs,
                              "hat_ids": json.loads(hats), "gun_style_ids": json.loads(guns),
                              "uniform_color_ids": json.loads(uniforms), "boot_color_ids": json.loads(boots)})

    elif method == "POST" and path == "/rosters/update":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        rid = data.get("id")
        if not db.execute("SELECT id FROM rosters WHERE id=? AND user_id=?", (rid, uid2)).fetchone():
            send_json(sock, 404, {"error":"not found"}); return
        name = data.get("name","My Team").strip()[:32]
        wn = data.get("worm_names", ["Soldier 1","Soldier 2","Soldier 3","Soldier 4"])
        wn = [str(n)[:16] for n in wn[:4]]
        while len(wn) < 4: wn.append(f"Soldier {len(wn)+1}")
        av = int(data.get("avatar_id", 0))
        hs = int(data.get("headstone_id", 0))
        def parse_arr(key): return json.dumps([(int(x) if isinstance(x,(int,float)) else 0) for x in (data.get(key) or [0,0,0,0])][:4])
        hats     = parse_arr("hat_ids")
        guns     = parse_arr("gun_style_ids")
        uniforms = parse_arr("uniform_color_ids")
        boots    = parse_arr("boot_color_ids")
        # Validate ownership for all cosmetic types (id=0 is always free)
        unlocked = {(r[0],r[1]) for r in db.execute(
            "SELECT cosm_type,cosm_id FROM player_cosmetics WHERE user_id=?", (uid2,)).fetchall()}
        checks = [("hat",json.loads(hats)),("gun_style",json.loads(guns)),
                  ("uniform",json.loads(uniforms)),("boots",json.loads(boots))]
        for ctype, ids in checks:
            for cid in ids:
                if cid != 0 and (ctype, cid) not in unlocked:
                    send_json(sock, 400, {"error": f"{ctype} {cid} not unlocked"}); return
        # Headstones 0-5 are all default (free). Future reward headstones validated here when added.
        db.execute("UPDATE rosters SET name=?,worm_names=?,avatar_id=?,headstone_id=?,hat_ids=?,gun_style_ids=?,uniform_color_ids=?,boot_color_ids=? WHERE id=?",
                   (name, json.dumps(wn), av, hs, hats, guns, uniforms, boots, rid))
        db.commit()
        send_json(sock, 200, {"id": rid, "name": name, "worm_names": wn,
                              "avatar_id": av, "headstone_id": hs,
                              "hat_ids": json.loads(hats), "gun_style_ids": json.loads(guns),
                              "uniform_color_ids": json.loads(uniforms), "boot_color_ids": json.loads(boots)})

    elif method == "POST" and path == "/rosters/delete":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        rid = data.get("id")
        if db.execute("SELECT COUNT(*) FROM rosters WHERE user_id=?", (uid2,)).fetchone()[0] <= 1:
            send_json(sock, 400, {"error":"cannot delete last roster"}); return
        db.execute("DELETE FROM rosters WHERE id=? AND user_id=?", (rid, uid2))
        db.commit()
        send_json(sock, 200, {"ok": True})

    # ── TAT matches ───────────────────────────────────────────────────────────

    elif method == "POST" and path == "/match/create":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        ranked = bool(data.get("ranked", False))
        force_code = bool(data.get("force_code", False))
        seed = int(time.time())
        active = db.execute("SELECT COUNT(*) FROM matches WHERE (p0=? OR p1=?) AND done=0", (uid2, uid2)).fetchone()[0]
        if active >= 15:
            send_json(sock, 400, {"error":"MATCH LIMIT REACHED (15 MAX)"}); sock.close(); return


        if not ranked and force_code:
            code = gen_code()
            db.execute("INSERT INTO matches(code,p0,seed,ranked,has_mines,has_barrels,mode,turn_started_at,turn_timeout) VALUES(?,?,?,0,1,1,'tat',?,?)", (code, uid2, seed, seed, 14*24*3600))
            db.commit()
            send_json(sock, 200, {"code": code})
            sock.close(); return

        if ranked:
            # Try to pair with someone in the ranked pool (within ±200 ELO, expanding over time)
            my_elo = get_elo(uid2)
            now = int(time.time())
            # Clean stale pool entries (> 5 min)
            db.execute("DELETE FROM ranked_pool WHERE joined_at < ? AND match_id IS NULL", (now - 300,))
            # Don't double-enter
            db.execute("DELETE FROM ranked_pool WHERE user_id=? AND match_id IS NULL", (uid2,))
            # Find a match
            opponent = db.execute(
                "SELECT id,user_id,elo,joined_at FROM ranked_pool WHERE match_id IS NULL AND user_id!=? ORDER BY ABS(elo-?) LIMIT 1",
                (uid2, my_elo)
            ).fetchone()
            if opponent:
                pool_id, opp_uid, opp_elo, opp_joined = opponent
                wait_secs = now - opp_joined
                window = 200 + (wait_secs // 30) * 50
                if abs(my_elo - opp_elo) <= window:
                    code = gen_code()
                    db.execute("INSERT INTO matches(code,p0,p1,seed,ranked,has_mines,has_barrels,mode,turn_started_at,turn_timeout) VALUES(?,?,?,?,1,1,1,'tat',?,?)", (code, opp_uid, uid2, seed, seed, 14*24*3600))
                    db.commit()
                    mid = db.execute("SELECT last_insert_rowid()").fetchone()[0]
                    db.execute("UPDATE ranked_pool SET match_id=? WHERE id=?", (mid, pool_id))
                    db.commit()
                    # Add pool entry for creator (already matched)
                    db.execute("INSERT INTO ranked_pool(user_id,elo,joined_at,match_id) VALUES(?,?,?,?)", (uid2, my_elo, now, mid))
                    db.commit()
                    send_json(sock, 200, {"match_id": mid, "searching": False, "opponent_elo": opp_elo})
                    sock.close(); return
            # No match found — add to pool
            db.execute("INSERT INTO ranked_pool(user_id,elo,joined_at) VALUES(?,?,?)", (uid2, my_elo, now))
            db.commit()
            send_json(sock, 200, {"searching": True})
        else:
            # Casual TAT: try to match from pool first, fall back to code-join
            now = int(time.time())
            db.execute("DELETE FROM casual_pool WHERE joined_at < ? AND match_id IS NULL", (now - 600,))
            db.execute("DELETE FROM casual_pool WHERE user_id=? AND match_id IS NULL", (uid2,))
            opponent = db.execute(
                "SELECT id,user_id FROM casual_pool WHERE match_id IS NULL AND user_id!=? LIMIT 1", (uid2,)
            ).fetchone()
            if opponent:
                pool_id, opp_uid = opponent
                code = gen_code()
                db.execute("INSERT INTO matches(code,p0,p1,seed,ranked,has_mines,has_barrels,mode,turn_started_at,turn_timeout) VALUES(?,?,?,?,0,1,1,'tat',?,?)", (code, opp_uid, uid2, seed, seed, 14*24*3600))
                db.commit()
                mid = db.execute("SELECT last_insert_rowid()").fetchone()[0]
                db.execute("UPDATE casual_pool SET match_id=? WHERE id=?", (mid, pool_id))
                db.execute("INSERT INTO casual_pool(user_id,joined_at,match_id) VALUES(?,?,?)", (uid2, now, mid))
                db.commit()
                send_json(sock, 200, {"match_id": mid, "searching": False})
            else:
                # No one waiting — create a match with a shareable code
                code = gen_code()
                db.execute("INSERT INTO matches(code,p0,seed,ranked,has_mines,has_barrels,mode,turn_started_at,turn_timeout) VALUES(?,?,?,0,1,1,'tat',?,?)", (code, uid2, seed, seed, 14*24*3600))
                db.commit()
                send_json(sock, 200, {"code": code})

    elif method == "GET" and path == "/ranked/tat/status":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        row = db.execute("SELECT match_id FROM ranked_pool WHERE user_id=? AND match_id IS NOT NULL ORDER BY id DESC LIMIT 1", (uid2,)).fetchone()
        if row:
            send_json(sock, 200, {"matched": True, "match_id": row[0]})
        else:
            # Still waiting — check they're still in pool
            in_pool = db.execute("SELECT id FROM ranked_pool WHERE user_id=? AND match_id IS NULL", (uid2,)).fetchone()
            send_json(sock, 200, {"matched": False, "in_pool": bool(in_pool)})

    elif method == "POST" and path == "/ranked/tat/cancel":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        db.execute("DELETE FROM ranked_pool WHERE user_id=? AND match_id IS NULL", (uid2,))
        db.commit()
        send_json(sock, 200, {"ok": True})

    elif method == "GET" and path == "/casual/tat/status":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        row = db.execute("SELECT match_id FROM casual_pool WHERE user_id=? AND match_id IS NOT NULL ORDER BY id DESC LIMIT 1", (uid2,)).fetchone()
        if row:
            send_json(sock, 200, {"matched": True, "match_id": row[0]})
        else:
            in_pool = db.execute("SELECT id FROM casual_pool WHERE user_id=? AND match_id IS NULL", (uid2,)).fetchone()
            send_json(sock, 200, {"matched": False, "in_pool": bool(in_pool)})

    elif method == "POST" and path == "/casual/tat/cancel":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        db.execute("DELETE FROM casual_pool WHERE user_id=? AND match_id IS NULL", (uid2,))
        db.commit()
        send_json(sock, 200, {"ok": True})

    elif method == "POST" and path == "/match/join":
        uid2 = uid(token); code = data.get("code","").upper()
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        active = db.execute("SELECT COUNT(*) FROM matches WHERE (p0=? OR p1=?) AND done=0", (uid2, uid2)).fetchone()[0]
        if active >= 15:
            send_json(sock, 400, {"error":"MATCH LIMIT REACHED (15 MAX)"}); sock.close(); return
        row = db.execute("SELECT id,ranked FROM matches WHERE code=? AND p1 IS NULL AND p0!=?", (code, uid2)).fetchone()
        if row:
            mid, is_ranked = row
            if is_ranked: send_json(sock, 400, {"error":"cannot join ranked match by code"}); return
            db.execute("UPDATE matches SET p1=? WHERE id=?", (uid2, mid))
            db.commit()
            send_json(sock, 200, {"match_id": mid})
        else: send_json(sock, 400, {"error":"match not found or full"})

    elif method == "POST" and "/move" in path:
        parts = path.strip('/').split('/'); mid = int(parts[1]) if len(parts) > 1 else 0
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        row = db.execute("SELECT p0,p1,turn,moves,done,ranked FROM matches WHERE id=?", (mid,)).fetchone()
        if not row: send_json(sock, 404, {"error":"not found"}); return
        p0,p1,turn,moves_json,done,is_ranked = row
        if done: send_json(sock, 400, {"error":"match over"}); return
        my_slot = 0 if uid2 == p0 else 1
        if turn != my_slot: send_json(sock, 400, {"error":"not your turn"}); return
        moves = json.loads(moves_json)
        moves.append({"angle": data.get("angle"), "power": data.get("power"), "facing": data.get("facing"),
                      "active_soldier": data.get("active_soldier", 0), "inputs": data.get("inputs", [])})
        kills_col  = f"p{my_slot}_kills"
        deaths_col = f"p{my_slot}_deaths"
        wk_col     = f"p{my_slot}_weapon_kills"
        kills_val  = int(data.get("kills", 0))
        deaths_val = int(data.get("deaths", 0))
        wk_in      = data.get("weapon_kills", {})
        if isinstance(wk_in, dict) and wk_in:
            wk_row = db.execute(f"SELECT {wk_col} FROM matches WHERE id=?", (mid,)).fetchone()
            wk_cur = json.loads(wk_row[0] or "{}") if wk_row else {}
            for w, c in wk_in.items():
                wk_cur[w] = wk_cur.get(w, 0) + int(c)
            db.execute(f"UPDATE matches SET moves=?,turn=?,turn_started_at=?,{kills_col}={kills_col}+?,{deaths_col}={deaths_col}+?,{wk_col}=? WHERE id=?",
                       (json.dumps(moves), 1-my_slot, int(time.time()), kills_val, deaths_val, json.dumps(wk_cur), mid))
        else:
            db.execute(f"UPDATE matches SET moves=?,turn=?,turn_started_at=?,{kills_col}={kills_col}+?,{deaths_col}={deaths_col}+? WHERE id=?",
                       (json.dumps(moves), 1-my_slot, int(time.time()), kills_val, deaths_val, mid))
        db.commit()
        send_json(sock, 200, {"ok": True})

    elif method == "POST" and path == "/match/live/result":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        winner_slot  = data.get("winner_slot")
        opp_username = data.get("opponent","")
        is_ranked    = bool(data.get("ranked", False))
        is_win       = (winner_slot == data.get("my_slot", 0))
        elo_delta    = 0
        if is_ranked and opp_username:
            opp_row = db.execute("SELECT id FROM users WHERE lower(username)=lower(?)", (opp_username,)).fetchone()
            if opp_row:
                opp_uid = opp_row[0]
                if is_win:
                    elo_delta_w, _ = update_elo(db, uid2, opp_uid)
                    elo_delta = elo_delta_w
                else:
                    _, elo_delta_l = update_elo(db, opp_uid, uid2)
                    elo_delta = elo_delta_l
        kills_val = int(data.get("kills", 0))
        scrap_earned = 75 if is_win else 25
        db.execute("UPDATE users SET scrap=scrap+? WHERE id=?", (scrap_earned, uid2))
        db.commit()
        update_challenges(db, uid2, matches=1, wins=1 if is_win else 0, kills=kills_val)
        new_elo = get_elo(uid2)
        send_json(sock, 200, {"ok": True, "elo_delta": elo_delta,
                              "new_elo": new_elo, "rank": rank_name(new_elo),
                              "scrap_earned": scrap_earned})

    elif method == "POST" and path.endswith("/result"):
        # POST /match/{id}/result  {token, winner_slot}
        parts = path.strip('/').split('/')
        mid = int(parts[1]) if len(parts) > 2 else 0
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        row = db.execute("SELECT p0,p1,done,ranked,winner FROM matches WHERE id=?", (mid,)).fetchone()
        if not row: send_json(sock, 404, {"error":"not found"}); return
        p0,p1,done,is_ranked,existing_winner = row
        if uid2 not in (p0, p1): send_json(sock, 403, {"error":"not in this match"}); return
        if existing_winner is not None: send_json(sock, 200, {"ok":True,"already_recorded":True}); return
        winner_slot = data.get("winner_slot")
        if winner_slot is None: send_json(sock, 400, {"error":"missing winner_slot"}); return
        uid_winner = p0 if winner_slot == 0 else p1
        uid_loser  = p1 if winner_slot == 0 else p0
        my_slot_idx = 0 if uid2 == p0 else 1
        kills_val  = int(data.get("kills", 0))
        deaths_val = int(data.get("deaths", 0))
        kills_col  = f"p{my_slot_idx}_kills"
        deaths_col = f"p{my_slot_idx}_deaths"
        wk_col     = f"p{my_slot_idx}_weapon_kills"
        wk_in      = data.get("weapon_kills", {})
        if isinstance(wk_in, dict) and wk_in:
            wk_row = db.execute(f"SELECT {wk_col} FROM matches WHERE id=?", (mid,)).fetchone()
            wk_cur = json.loads(wk_row[0] or "{}") if wk_row else {}
            for w, c in wk_in.items():
                wk_cur[w] = wk_cur.get(w, 0) + int(c)
            db.execute(f"UPDATE matches SET done=1, winner=?, {kills_col}={kills_col}+?, {deaths_col}={deaths_col}+?, {wk_col}=? WHERE id=?",
                       (uid_winner, kills_val, deaths_val, json.dumps(wk_cur), mid))
        else:
            db.execute(f"UPDATE matches SET done=1, winner=?, {kills_col}={kills_col}+?, {deaths_col}={deaths_col}+? WHERE id=?",
                       (uid_winner, kills_val, deaths_val, mid))
        db.execute("DELETE FROM ranked_pool WHERE match_id=?", (mid,))
        db.execute("DELETE FROM casual_pool WHERE match_id=?", (mid,))
        db.commit()
        elo_delta_w, elo_delta_l = 0, 0
        if is_ranked and uid_winner and uid_loser:
            elo_delta_w, elo_delta_l = update_elo(db, uid_winner, uid_loser)
        my_delta = elo_delta_w if uid2 == uid_winner else elo_delta_l
        is_win_tat = (uid2 == uid_winner)
        scrap_earned = 75 if is_win_tat else 25
        db.execute("UPDATE users SET scrap=scrap+? WHERE id=?", (scrap_earned, uid2))
        db.commit()
        update_challenges(db, uid2, matches=1, wins=1 if is_win_tat else 0, kills=kills_val)
        new_elo = get_elo(uid2)
        send_json(sock, 200, {"ok": True, "elo_delta": my_delta,
                              "new_elo": new_elo, "rank": rank_name(new_elo),
                              "scrap_earned": scrap_earned})

    elif method == "GET" and path.startswith("/match/") and path.endswith("/state"):
        parts = path.strip('/').split('/'); mid = int(parts[1]) if len(parts) > 1 else 0
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        row = db.execute("SELECT p0,p1,turn,moves,seed,done,ranked,has_mines,has_barrels,turn_started_at,turn_timeout FROM matches WHERE id=?", (mid,)).fetchone()
        if not row: send_json(sock, 404, {"error":"not found"}); return
        p0,p1,turn,moves,seed,done,is_ranked,has_mines_val,has_barrels_val,turn_started_at,turn_timeout = row
        has_mines_val   = has_mines_val or 0
        has_barrels_val = has_barrels_val or 0
        # 14-day turn timeout: forfeit if current player has not moved in time
        if not done and turn_timeout and turn_started_at:
            if int(time.time()) - turn_started_at > turn_timeout:
                uid_loser  = p0 if turn == 0 else p1
                uid_winner = p1 if turn == 0 else p0
                db.execute("UPDATE matches SET done=1, winner=? WHERE id=?", (uid_winner, mid))
                db.execute("DELETE FROM ranked_pool WHERE match_id=?", (mid,))
                db.execute("DELETE FROM casual_pool WHERE match_id=?", (mid,))
                if is_ranked and uid_winner and uid_loser:
                    update_elo(db, uid_winner, uid_loser)
                db.commit()
                done = True
        my_slot = 0 if uid2 == p0 else 1
        opp = uname(p1 if my_slot==0 else p0)
        my_elo_val = get_elo(uid2) or 1000
        opp_uid = p1 if my_slot == 0 else p0
        opp_elo_val = (get_elo(opp_uid) or 1000) if opp_uid else 1000
        # Fetch opponent's roster cosmetics and worm names
        opp_worm_names = ["", "", "", ""]
        opp_hat_ids = [0,0,0,0]; opp_uniform_ids = [0,0,0,0]
        opp_boot_ids = [0,0,0,0]; opp_gun_ids = [0,0,0,0]
        if opp_uid:
            opp_roster = db.execute(
                "SELECT worm_names,hat_ids,uniform_color_ids,boot_color_ids,gun_style_ids FROM rosters WHERE user_id=? ORDER BY id LIMIT 1", (opp_uid,)
            ).fetchone()
            if opp_roster:
                opp_worm_names = json.loads(opp_roster[0] or '["","","",""]')
                opp_hat_ids    = json.loads(opp_roster[1] or '[0,0,0,0]')
                opp_uniform_ids= json.loads(opp_roster[2] or '[0,0,0,0]')
                opp_boot_ids   = json.loads(opp_roster[3] or '[0,0,0,0]')
                opp_gun_ids    = json.loads(opp_roster[4] or '[0,0,0,0]')
        send_json(sock, 200, {"moves": json.loads(moves), "seed": seed, "turn": turn,
                              "my_slot": my_slot, "opponent": opp, "done": bool(done), "ranked": bool(is_ranked),
                              "my_elo": my_elo_val, "opponent_elo": opp_elo_val,
                              "has_mines": bool(has_mines_val), "has_barrels": bool(has_barrels_val),
                              "opponent_worm_names": opp_worm_names,
                              "opponent_hat_ids": opp_hat_ids, "opponent_uniform_color_ids": opp_uniform_ids,
                              "opponent_boot_color_ids": opp_boot_ids, "opponent_gun_style_ids": opp_gun_ids})

    elif method == "GET" and path == "/matches/pending":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        # Auto-forfeit expired matches before building the list
        now_ts = int(time.time())
        expired = db.execute(
            "SELECT id,p0,p1,turn,ranked FROM matches WHERE (p0=? OR p1=?) AND done=0 AND p1 IS NOT NULL AND turn_timeout>0 AND turn_started_at>0 AND (?-turn_started_at)>turn_timeout",
            (uid2, uid2, now_ts)
        ).fetchall()
        for eid,ep0,ep1,eturn,eranked in expired:
            e_loser  = ep0 if eturn == 0 else ep1
            e_winner = ep1 if eturn == 0 else ep0
            db.execute("UPDATE matches SET done=1, winner=? WHERE id=?", (e_winner, eid))
            db.execute("DELETE FROM ranked_pool WHERE match_id=?", (eid,))
            db.execute("DELETE FROM casual_pool WHERE match_id=?", (eid,))
            if eranked and e_winner and e_loser:
                update_elo(db, e_winner, e_loser)
        if expired: db.commit()
        rows = db.execute(
            "SELECT id,p0,p1,turn,code,ranked,turn_started_at,turn_timeout FROM matches WHERE (p0=? OR p1=?) AND done=0 AND p1 IS NOT NULL",
            (uid2,uid2)
        ).fetchall()
        out = []
        for mid,p0,p1,turn,code,is_ranked,ts,tt in rows:
            opp = uname(p1 if uid2==p0 else p0)
            my_slot = 0 if uid2==p0 else 1
            opp_uid_val = p1 if uid2==p0 else p0
            opp_elo_val = (get_elo(opp_uid_val) or 1000) if opp_uid_val else 1000
            days_rem = max(0, tt - (now_ts - ts)) // 86400 if (tt and ts) else -1
            out.append({"match_id":mid,"code":code,"opponent":opp,"your_turn":turn==my_slot,"ranked":bool(is_ranked),"opponent_elo":opp_elo_val,"days_remaining":days_rem})
        send_json(sock, 200, out)

    # ── Live ranked queue ─────────────────────────────────────────────────────

    elif method == "POST" and path == "/ranked/queue/join":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        my_elo = get_elo(uid2)
        now = int(time.time())
        # Remove stale entries and re-insert
        db.execute("DELETE FROM live_queue WHERE user_id=?", (uid2,))
        db.execute("DELETE FROM live_queue WHERE joined_at < ?", (now - 300,))
        db.execute("INSERT INTO live_queue(user_id,elo,joined_at) VALUES(?,?,?)", (uid2, my_elo, now))
        db.commit()
        # Try to pair immediately
        opponent = db.execute(
            "SELECT id,user_id,elo,joined_at FROM live_queue WHERE user_id!=? AND paired_with IS NULL ORDER BY ABS(elo-?) LIMIT 1",
            (uid2, my_elo)
        ).fetchone()
        if opponent:
            pool_id, opp_uid, opp_elo, opp_joined = opponent
            wait_secs = now - opp_joined
            window = 200 + (wait_secs // 30) * 50
            if abs(my_elo - opp_elo) <= window:
                port = 7777
                my_row = db.execute("SELECT id FROM live_queue WHERE user_id=?", (uid2,)).fetchone()
                db.execute("UPDATE live_queue SET paired_with=?,game_token=? WHERE id=?", (opp_uid, str(port), pool_id))
                db.execute("UPDATE live_queue SET paired_with=?,game_token=? WHERE id=?", (uid2, str(port), my_row[0]))
                db.commit()
                send_json(sock, 200, {"status":"matched","port":port,"opponent_elo":opp_elo,"my_elo":my_elo})
                sock.close(); return
        send_json(sock, 200, {"status":"waiting","my_elo":my_elo})

    elif method == "GET" and path == "/ranked/queue/status":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        my_elo = get_elo(uid2)
        now = int(time.time())
        # Try to match with anyone in queue now
        db.execute("DELETE FROM live_queue WHERE joined_at < ?", (now - 300,))
        row = db.execute("SELECT id,paired_with,game_token,elo FROM live_queue WHERE user_id=?", (uid2,)).fetchone()
        if not row: send_json(sock, 200, {"status":"not_in_queue"}); return
        my_q_id, paired_with, game_token, my_q_elo = row
        if paired_with:
            opp_elo = db.execute("SELECT elo FROM live_queue WHERE user_id=?", (paired_with,)).fetchone()
            send_json(sock, 200, {"status":"matched","port":int(game_token) if game_token and game_token.isdigit() else 7777,"opponent_elo":opp_elo[0] if opp_elo else my_q_elo,"my_elo":my_elo})
            sock.close(); return
        # Try to pair now
        opponent = db.execute(
            "SELECT id,user_id,elo,joined_at FROM live_queue WHERE user_id!=? AND paired_with IS NULL ORDER BY ABS(elo-?) LIMIT 1",
            (uid2, my_q_elo)
        ).fetchone()
        if opponent:
            pool_id, opp_uid, opp_elo, opp_joined = opponent
            wait_secs = now - opp_joined
            window = 200 + (wait_secs // 30) * 50
            if abs(my_q_elo - opp_elo) <= window:
                port = 7777
                db.execute("UPDATE live_queue SET paired_with=?,game_token=? WHERE id=?", (opp_uid, str(port), pool_id))
                db.execute("UPDATE live_queue SET paired_with=?,game_token=? WHERE id=?", (uid2, str(port), my_q_id))
                db.commit()
                send_json(sock, 200, {"status":"matched","port":port,"opponent_elo":opp_elo,"my_elo":my_elo})
                sock.close(); return
        send_json(sock, 200, {"status":"waiting","my_elo":my_elo})

    elif method == "POST" and path == "/ranked/queue/leave":
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        db.execute("DELETE FROM live_queue WHERE user_id=? AND paired_with IS NULL", (uid2,))
        db.commit()
        send_json(sock, 200, {"ok": True})

    elif method == "GET" and path.startswith("/stats"):
        # GET /stats?mode=live|tat&token=...
        import urllib.parse as _up
        qs = _up.parse_qs(path.split("?",1)[1] if "?" in path else "")
        mode_val = (qs.get("mode", ["tat"])[0]).lower()
        uid2 = uid(token)
        if not uid2: send_json(sock, 401, {"error":"invalid token"}); return
        # Aggregate stats by ranked flag
        def fetch_section(ranked_flag):
            row = db.execute("""
                SELECT
                    COALESCE(SUM(CASE WHEN winner=? THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN winner IS NOT NULL AND winner!=? THEN 1 ELSE 0 END),0),
                    COALESCE(SUM(CASE WHEN p0=? THEN p0_kills ELSE p1_kills END),0),
                    COALESCE(SUM(CASE WHEN p0=? THEN p0_deaths ELSE p1_deaths END),0)
                FROM matches
                WHERE (p0=? OR p1=?) AND done=1 AND ranked=? AND (mode=? OR mode IS NULL)
            """, (uid2,uid2, uid2,uid2, uid2,uid2, ranked_flag, mode_val)).fetchone()
            wk_rows = db.execute(f"""
                SELECT CASE WHEN p0=? THEN p0_weapon_kills ELSE p1_weapon_kills END
                FROM matches WHERE (p0=? OR p1=?) AND done=1 AND ranked=? AND (mode=? OR mode IS NULL)
            """, (uid2, uid2, uid2, ranked_flag, mode_val)).fetchall()
            wk_total = {}
            for (wk_json_str,) in wk_rows:
                try:
                    for w, c in json.loads(wk_json_str or "{}").items():
                        wk_total[w] = wk_total.get(w, 0) + c
                except: pass
            return {"wins": row[0], "losses": row[1], "kills": row[2], "deaths": row[3], "weapon_kills": wk_total}
        cas = fetch_section(0)
        rnk = fetch_section(1)
        elo_val = get_elo(uid2) or 1000
        send_json(sock, 200, {
            "casual_wins":   cas["wins"],   "casual_losses":  cas["losses"],
            "casual_kills":  cas["kills"],  "casual_deaths":  cas["deaths"],
            "casual_weapon_kills": cas["weapon_kills"],
            "ranked_wins":   rnk["wins"],   "ranked_losses":  rnk["losses"],
            "ranked_kills":  rnk["kills"],  "ranked_deaths":  rnk["deaths"],
            "ranked_weapon_kills": rnk["weapon_kills"],
            "elo": elo_val,
        })

    else: send_json(sock, 404, {"error":"not found"})
    sock.close()

# ── Main ───────────────────────────────────────────────────────────────────────

def main():
    db = sqlite3.connect(DB, check_same_thread=False)
    init_db(db)
    srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("0.0.0.0", PORT))
    srv.listen(10)
    print(f"Arty API on :{PORT}")
    while True:
        s, _ = srv.accept()
        threading.Thread(target=handle, args=(db, s), daemon=True).start()

if __name__ == "__main__":
    main()
