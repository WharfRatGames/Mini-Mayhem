#!/bin/bash
MIYOO1="root@10.0.0.110"
MIYOO2="root@10.0.0.126"
MIYOO_PATH="/mnt/SDCARD/App/Arty/mini-mayhem"
VERSION=$1
if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version>"
    exit 1
fi

# Abort if changelog hasn't been updated for this version
if ! grep -q "^$VERSION" deploy/changelog.txt 2>/dev/null; then
    echo "ERROR: deploy/changelog.txt has no entry for $VERSION — add one before deploying"
    exit 1
fi
BINARY="target/armv7-unknown-linux-gnueabihf/miyoo/mini-mayhem"
SERVER_BINARY="target/aarch64-unknown-linux-gnu/release/server"
scp "$BINARY" arty-pi:/var/www/html/arty/mini-mayhem
ssh arty-pi "echo $VERSION > /var/www/html/arty/version.txt"
echo "Update server now serving $VERSION"
# Push game server binary before restarting
if [ -f "$SERVER_BINARY" ]; then
    scp "$SERVER_BINARY" arty-pi:/home/Grunkus/mayhem-server/server.new
    ssh arty-pi "mv /home/Grunkus/mayhem-server/server.new /home/Grunkus/mayhem-server/server"
    echo "Game server binary updated"
fi

if [ -f "deploy/changelog.txt" ]; then
    scp deploy/changelog.txt arty-pi:/var/www/html/arty/changelog.txt
    echo "Changelog updated"
else
    echo "WARNING: deploy/changelog.txt missing — update screen will show stale/empty notes"
fi

# Deploy dashboard
if [ -f "deploy/dashboard/index.html" ]; then
    ssh arty-pi "mkdir -p /var/www/html/arty/dashboard"
    scp deploy/dashboard/index.html arty-pi:/var/www/html/arty/dashboard/index.html
    echo "Dashboard deployed → https://crumbonium.duckdns.org/arty/dashboard/"
fi

# Deploy IRC dashboard
if [ -f "deploy/ircdash/index.html" ]; then
    ssh arty-pi "mkdir -p /var/www/html/ircdash"
    scp deploy/ircdash/index.html arty-pi:/var/www/html/ircdash/index.html
    scp deploy/irc_dash.py arty-pi:/home/Grunkus/mayhem-server/irc_dash.py
    scp deploy/irc-dash.service arty-pi:/home/Grunkus/.config/systemd/user/irc-dash.service
    ssh arty-pi "systemctl --user daemon-reload && systemctl --user enable irc-dash && systemctl --user restart irc-dash"
    # Add nginx proxy for /irc/ → localhost:7781 if not already present
    ssh arty-pi "grep -q 'irc/state' /etc/nginx/sites-available/default || \
        echo fragtownusa | sudo -S sed -i 's|location / {|location /irc/ { proxy_pass http://127.0.0.1:7781/irc/; proxy_read_timeout 5s; }\n\tlocation / {|' \
        /etc/nginx/sites-available/default"
    echo "IRC dashboard deployed → https://crumbonium.duckdns.org/ircdash/"
fi

# Deploy API
scp deploy/arty_api.py arty-pi:/home/Grunkus/mayhem-server/arty_api.py
if [ -f "deploy/arty-api.service" ]; then
    scp deploy/arty-api.service arty-pi:/home/Grunkus/.config/systemd/user/arty-api.service
    ssh arty-pi "systemctl --user daemon-reload"
fi
ssh arty-pi "systemctl --user restart arty-api" 2>/dev/null || true
echo "API updated"

# Validate nginx config and reload if needed
if ssh arty-pi "echo fragtownusa | sudo -S nginx -t 2>&1"; then
    ssh arty-pi "echo fragtownusa | sudo -S systemctl reload nginx" && echo "nginx reloaded OK"
else
    echo "WARNING: nginx config test failed — NOT reloading nginx (fix conflict before reloading)"
fi

# Generate and serve manifest of app files (including sfx assets)
DEPLOY_DIR="deploy"
MANIFEST=""
for f in launch.sh config.json icon.png; do
    if [ -f "$DEPLOY_DIR/$f" ]; then
        SIZE=$(wc -c < "$DEPLOY_DIR/$f")
        HASH=$(sha256sum "$DEPLOY_DIR/$f" | awk '{print $1}')
        MANIFEST="$MANIFEST$f $SIZE $HASH\n"
        scp "$DEPLOY_DIR/$f" arty-pi:/var/www/html/arty/$f
    fi
done
if [ -d "$DEPLOY_DIR/assets/sfx" ]; then
    ssh arty-pi "mkdir -p /var/www/html/arty/sfx/death"
    for wav in "$DEPLOY_DIR/assets/sfx/"*.wav; do
        [ -f "$wav" ] || continue
        fname="sfx/$(basename "$wav")"
        SIZE=$(wc -c < "$wav")
        HASH=$(sha256sum "$wav" | awk '{print $1}')
        MANIFEST="$MANIFEST$fname $SIZE $HASH\n"
        scp "$wav" "arty-pi:/var/www/html/arty/$fname"
    done
    for wav in "$DEPLOY_DIR/assets/sfx/death/"*.wav; do
        [ -f "$wav" ] || continue
        fname="sfx/death/$(basename "$wav")"
        SIZE=$(wc -c < "$wav")
        HASH=$(sha256sum "$wav" | awk '{print $1}')
        MANIFEST="$MANIFEST$fname $SIZE $HASH\n"
        scp "$wav" "arty-pi:/var/www/html/arty/$fname"
    done
fi
ssh arty-pi "printf '$MANIFEST' > /var/www/html/arty/manifest.txt"

# Restart game server via systemd service (auto-restarts on crash)
echo "Restarting game server..."
ssh arty-pi "systemctl --user restart arty-game"
echo "Game server restarted"

# Push directly to Miyoos with retry (3 attempts, 1.5s between)
LOCAL_HASH=$(md5sum "$BINARY" | awk '{print $1}')
echo "Pushing to Miyoos... (md5: $LOCAL_HASH)"

push_to_miyoo() {
    local HOST=$1 LABEL=$2
    for attempt in 1 2 3; do
        # Stage to /tmp, copy to SDCARD BEFORE kill so the launcher restarts with the new binary.
        # Kill-before-copy causes a race: launcher restarts old game, FAT32 locks the file,
        # cp fails silently, device stays on old version with game killed.
        if scp -o ConnectTimeout=5 -o StrictHostKeyChecking=no \
               "$BINARY" "$HOST:/tmp/mini-mayhem.new" 2>/dev/null && \
           ssh -o ConnectTimeout=5 -o StrictHostKeyChecking=no "$HOST" \
               "cp /tmp/mini-mayhem.new $MIYOO_PATH && pkill mini-mayhem 2>/dev/null; pkill arty 2>/dev/null; rm /tmp/mini-mayhem.new" 2>/dev/null; then
            REMOTE_HASH=$(ssh -o ConnectTimeout=5 "$HOST" \
                "md5sum $MIYOO_PATH 2>/dev/null | awk '{print \$1}'" 2>/dev/null)
            if [ "$REMOTE_HASH" = "$LOCAL_HASH" ]; then
                echo "  $LABEL OK (hash verified, attempt $attempt)"
                ssh -o ConnectTimeout=5 -o StrictHostKeyChecking=no "$HOST" \
                    "mkdir -p /mnt/SDCARD/App/Arty/sfx/death" 2>/dev/null
                scp -o ConnectTimeout=5 -o StrictHostKeyChecking=no \
                    -r deploy/assets/sfx/. "$HOST:/mnt/SDCARD/App/Arty/sfx/" 2>/dev/null \
                    && echo "  $LABEL sfx OK" || echo "  $LABEL sfx FAILED (non-fatal)"
                return 0
            else
                echo "  $LABEL hash mismatch on attempt $attempt, retrying..."
            fi
        else
            if [ $attempt -lt 3 ]; then
                echo "  $LABEL unreachable (attempt $attempt), retrying in 1.5s..."
                sleep 1.5
            fi
        fi
    done
    echo "  $LABEL FAILED after 3 attempts"
}

push_to_miyoo "$MIYOO1" ".110"
push_to_miyoo "$MIYOO2" ".126"

# ── Local shareable build ─────────────────────────────────────────────────────
BUILD_DIR="$HOME/mini-mayhem-builds"
mkdir -p "$BUILD_DIR"
STAGE="$BUILD_DIR/MiniMayhem"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$BINARY" "$STAGE/mini-mayhem"
for f in launch.sh config.json icon.png; do
    [ -f "$DEPLOY_DIR/$f" ] && cp "$DEPLOY_DIR/$f" "$STAGE/$f"
done
ZIP="$BUILD_DIR/mini-mayhem-$VERSION.zip"
if (cd "$BUILD_DIR" && zip -r "$ZIP" MiniMayhem/ -x "*.DS_Store" > /dev/null); then
    echo "Shareable build: $ZIP ($(du -sh "$ZIP" | cut -f1))"
else
    echo "WARNING: zip creation failed — builds upload skipped"
fi
rm -rf "$STAGE"
ls -t "$BUILD_DIR"/mini-mayhem-*.zip 2>/dev/null | tail -n +4 | xargs rm -f 2>/dev/null

# Publish zip to nginx for download
if [ -f "$ZIP" ]; then
    ssh arty-pi "mkdir -p /var/www/html/arty/builds"
    if scp "$ZIP" "arty-pi:/var/www/html/arty/builds/mini-mayhem-$VERSION.zip"; then
        ssh arty-pi "ls -t /var/www/html/arty/builds/mini-mayhem-*.zip 2>/dev/null | tail -n +4 | xargs rm -f 2>/dev/null"
        echo "Download: http://crumbonium.duckdns.org/arty/builds/mini-mayhem-$VERSION.zip"
    else
        echo "WARNING: builds scp to Pi failed — nginx not updated"
    fi
else
    echo "WARNING: $ZIP not found — builds upload skipped"
fi

# Publish to GitHub Releases (attach deploy/assets.zip; build zip stays on Pi only)
NOTES=$(head -1 deploy/changelog.txt 2>/dev/null || echo "v$VERSION")
WIN_EXE="target/x86_64-pc-windows-gnu/release/mini-mayhem.exe"
WIN_ARGS=""
if [ -f "$WIN_EXE" ]; then
    # Serve raw exe for OTA self-update
    scp "$WIN_EXE" arty-pi:/var/www/html/arty/mini-mayhem.exe
    echo "Windows OTA binary deployed"
    WIN_STAGE=$(mktemp -d)
    mkdir -p "$WIN_STAGE/MiniMayhemWindows"
    cp "$WIN_EXE" "$WIN_STAGE/MiniMayhemWindows/"
    cp -r deploy/assets/sfx "$WIN_STAGE/MiniMayhemWindows/"
    WIN_ZIP="$WIN_STAGE/mini-mayhem-windows-$VERSION.zip"
    (cd "$WIN_STAGE" && zip -r "$WIN_ZIP" MiniMayhemWindows/ -x "*.DS_Store" > /dev/null)
    WIN_ARGS="$WIN_ZIP"
    echo "Windows bundle: $WIN_ZIP ($(du -sh "$WIN_ZIP" | cut -f1))"
    ssh arty-pi "mkdir -p /var/www/html/arty/builds"
    if scp "$WIN_ZIP" "arty-pi:/var/www/html/arty/builds/mini-mayhem-windows-$VERSION.zip"; then
        ssh arty-pi "ls -t /var/www/html/arty/builds/mini-mayhem-windows-*.zip 2>/dev/null | tail -n +4 | xargs rm -f 2>/dev/null"
        echo "Windows download: http://crumbonium.duckdns.org/arty/builds/mini-mayhem-windows-$VERSION.zip"
    else
        echo "WARNING: Windows builds scp to Pi failed"
    fi
else
    echo "WARNING: Windows binary not found at $WIN_EXE — skipping"
fi
if gh release create "v$VERSION" "$ZIP" deploy/assets.zip $WIN_ARGS \
    --repo WharfRatGames/Mini-Mayhem \
    --title "v$VERSION" \
    --notes "$NOTES" \
    --latest 2>&1; then
    echo "GitHub release: https://github.com/WharfRatGames/Mini-Mayhem/releases/tag/v$VERSION"
else
    echo "WARNING: GitHub release failed (non-fatal)"
fi
[ -n "$WIN_STAGE" ] && rm -rf "$WIN_STAGE"

# Regenerate and push cosmetics gallery images to Pi
if python3 deploy/make_galleries.py 2>/dev/null; then
    scp /tmp/hat_galleries/*.png arty-pi:/home/Grunkus/mayhem-server/galleries/ 2>/dev/null \
        && echo "Cosmetics galleries updated on Pi" \
        || echo "WARNING: gallery scp failed"
else
    echo "WARNING: gallery generation failed (non-fatal)"
fi

# Notify Discord bot: patch notes + refresh cosmetics gallery
ssh arty-pi "curl -s -X POST http://127.0.0.1:7779/notify/patch \
    -H 'Content-Type: application/json' \
    -d '{\"version\":\"$VERSION\"}'" 2>/dev/null \
    && echo "Discord notified (patch notes)" \
    || echo "Discord notify skipped (bot not running)"
