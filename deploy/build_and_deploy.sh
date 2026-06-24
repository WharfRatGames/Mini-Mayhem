#!/bin/bash
set -e
VERSION=$1
if [ -z "$VERSION" ]; then
    echo "Usage: $0 <version>"
    exit 1
fi

echo "==> Building Miyoo client..."
cargo miyoo --bin mini-mayhem

echo "==> Building Pi server..."
cargo piserver --bin server

echo "==> Building Windows client..."
cargo build --release --target x86_64-pc-windows-gnu --bin mini-mayhem

echo "==> Packaging Windows release zip..."
WIN_EXE="target/x86_64-pc-windows-gnu/release/mini-mayhem.exe"
WIN_ZIP="target/mini-mayhem-windows-$VERSION.zip"
if [ -f "$WIN_EXE" ]; then
    rm -f "$WIN_ZIP"
    # Put exe + sfx/ in a flat zip (player unzips and runs mini-mayhem.exe)
    cp "$WIN_EXE" /tmp/mini-mayhem.exe
    (cd /tmp && zip -q "$OLDPWD/$WIN_ZIP" mini-mayhem.exe)
    if [ -d "deploy/assets/sfx" ]; then
        (cd deploy/assets && zip -qr "$OLDPWD/$WIN_ZIP" sfx/)
    fi
    echo "Windows zip: $WIN_ZIP"
else
    echo "WARNING: Windows binary not found — skipping zip"
fi

echo "==> Deploying $VERSION..."
bash deploy/update_server.sh "$VERSION"
