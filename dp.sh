export PATH="/tmp/zig-linux-x86_64-0.13.0:$PATH"
VERSION=$(grep -oP '(?<=const VERSION: &str = ")[^"]+' src/main.rs)
cargo miyoo && ./deploy/sync.sh 10.0.0.110 onion && ./deploy/update_server.sh "$VERSION"
