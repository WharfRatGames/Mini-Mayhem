#!/bin/bash
set -euo pipefail

DEVICE_IP="${1:-192.168.1.50}"
DEVICE_USER="${2:-onion}"
DEVICE_PATH="/mnt/SDCARD/App/Arty"
BINARY="target/armv7-unknown-linux-gnueabihf/miyoo/arty"

if [ ! -f "$BINARY" ]; then
  echo "Binary not found. Build first: cargo miyoo"
  exit 1
fi

echo "Syncing to ${DEVICE_USER}@${DEVICE_IP}:${DEVICE_PATH}..."
rsync -avz --no-perms --no-owner --no-group --progress \
  "$BINARY" \
  deploy/launch.sh \
  deploy/config.json \
  "${DEVICE_USER}@${DEVICE_IP}:${DEVICE_PATH}/"

# Sync sfx assets if present
if [ -d "deploy/assets/sfx" ]; then
  ssh "${DEVICE_USER}@${DEVICE_IP}" "mkdir -p ${DEVICE_PATH}/sfx"
  rsync -avz --no-perms --no-owner --no-group deploy/assets/sfx/ "${DEVICE_USER}@${DEVICE_IP}:${DEVICE_PATH}/sfx/"
fi

ssh "${DEVICE_USER}@${DEVICE_IP}" "chmod +x ${DEVICE_PATH}/arty ${DEVICE_PATH}/launch.sh"
rsync -avz --no-perms --no-owner --no-group --progress "$BINARY" deploy/launch.sh deploy/config.json "root@10.0.0.126:${DEVICE_PATH}/"
if [ -d "deploy/assets/sfx" ]; then
  ssh "root@10.0.0.126" "mkdir -p ${DEVICE_PATH}/sfx"
  rsync -avz --no-perms --no-owner --no-group deploy/assets/sfx/ "root@10.0.0.126:${DEVICE_PATH}/sfx/"
fi
ssh "root@10.0.0.126" "chmod +x ${DEVICE_PATH}/arty ${DEVICE_PATH}/launch.sh"
echo "Done. Launch Arty from the Onion app menu."
