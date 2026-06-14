# Getting Started Coding for Miyoo Mini Plus (Onion OS)

---

## What you're working with

The Miyoo Mini Plus runs stripped Linux. No GPU — rendering goes directly through a framebuffer device (`/dev/fb0`). Input comes from evdev. The screen is 640×480 physical pixels. SSH works over WiFi with a blank root password.

---

## Toolchain (Rust, cross-compile from Linux)

```bash
# Get zig — just extract, no install needed
wget https://ziglang.org/download/0.13.0/zig-linux-x86_64-0.13.0.tar.xz
tar xf zig-linux-x86_64-0.13.0.tar.xz -C /tmp/
export PATH="/tmp/zig-linux-x86_64-0.13.0:$PATH"

cargo install cargo-zigbuild
rustup target add armv7-unknown-linux-gnueabihf

# Build — the .2.17 pins the glibc floor; Onion has 2.28 so this always works
cargo zigbuild --release --target armv7-unknown-linux-gnueabihf.2.17
```

Always `--release`. Debug builds run at ~3 FPS on the hardware.

---

## App folder structure

```
/mnt/SDCARD/App/YourApp/
├── yourapp        # ELF ARM binary
├── launch.sh      # sets up env, runs the binary
├── config.json    # launcher metadata
├── icon.png       # launcher icon (shown in Onion menu)
└── assets/        # anything your app loads at runtime
```

**`config.json`:**
```json
{
  "label": "My App",
  "icon": "icon.png",
  "description": "Does a thing"
}
```

Icon path must be relative — absolute paths break.

**`launch.sh`:**
```bash
#!/bin/sh
cd "$(dirname "$0")"
export HOME=/mnt/SDCARD
./yourapp
```

Make sure it's executable: `chmod +x launch.sh yourapp`.

---

## Deploying

SSH in over WiFi — blank root password on Onion, just press Enter:

```bash
ssh root@<device-ip>
```

Sync your app folder:

```bash
rsync -avz App/YourApp/ root@<device-ip>:/mnt/SDCARD/App/YourApp/
```

Find the IP in Onion's network settings menu.

---

## Rendering (direct framebuffer)

Write pixels directly to `/dev/fb0`. The buffer is **BGRA byte order, not RGB**. This is the single most common mistake.

```rust
// Each pixel at (x, y) in a 640×480 buffer:
let offset = (y * 640 + x) * 4;
buf[offset + 0] = blue;
buf[offset + 1] = green;
buf[offset + 2] = red;
buf[offset + 3] = 255; // alpha
```

Query the actual screen size at runtime with the `FBIOGET_VSCREENINFO` ioctl rather than hardcoding 640×480.

---

## Input (evdev)

```rust
// Open /dev/input/event0
// Read input_event structs: { time, type, code, value }
// type == 1 (EV_KEY): key press/release
// value == 1: pressed, value == 0: released

// Button → Linux key code:
// D-pad: Up=103  Down=108  Left=105  Right=106
// A=57   B=29    X=42      Y=56
// L1=18  R1=20   L2=15     R2=14
// Start=28   Select=97
```

The MENU button (center) is intercepted by Onion's `keymon` daemon for system shortcuts. It is not available to your app.

---

## Audio

Use ALSA directly via `dlopen("libasound.so.2")` — load it at runtime rather than linking at compile time. The hardware DAC runs at **8000 Hz mono** regardless of what sample rate you request, so resample everything to 8000 Hz before sending it to ALSA. WAV files only — OGG support is broken on this platform.

---

## Things that will bite you

| Gotcha | Fix |
|---|---|
| Colors look wrong | Framebuffer is BGRA, not RGB |
| App runs at ~3 FPS | Always build `--release` |
| App doesn't appear in launcher | Check `config.json` is valid JSON with a relative icon path |
| Binary won't run | `chmod +x yourapp launch.sh` after copying |
| MENU button does nothing | It's taken by the OS — don't use it |
