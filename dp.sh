export PATH="/tmp/zig-linux-x86_64-0.13.0:$PATH"
cargo miyoo && ./deploy/sync.sh 10.0.0.110 onion
