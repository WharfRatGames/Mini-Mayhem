#!/bin/sh
cd "$(dirname "$0")"
export HOME=/mnt/SDCARD
# Release audio device from audioserver so tinyplay can open it directly
killall audioserver 2>/dev/null
sleep 0.2
/customer/app/tinymix set "Playback Volume Line Out" 0 2>/dev/null || true
exec ./mini-mayhem
