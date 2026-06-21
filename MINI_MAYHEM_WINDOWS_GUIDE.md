# Mini Mayhem — Windows Desktop Port Guide

A complete reference for building, running, and updating the Windows desktop
port of [Mini Mayhem](https://github.com/WharfRatGames/Mini-Mayhem),
originally a Miyoo Mini Plus game. This port adds a windowed display, audio,
and gamepad/keyboard input for Windows (and desktop Linux/macOS), while
staying fully compatible with the live multiplayer server — Windows and
Miyoo players can play in the same matches.

---

## 1. One-time setup

### 1.1 Install Rust

Download and run the installer from **<https://rustup.rs/>**. When prompted,
the default options are fine (MSVC toolchain). Let the installer finish
completely — it opens a console window and asks you to press `1` and Enter
to confirm the default install; closing that window early means nothing
actually gets installed even though it looked like it ran.

**Important:** if you had a Command Prompt or File Explorer window open
*before* installing Rust, close it and open a new one (or just restart your
PC). Windows only loads your PATH into a process when that process starts,
so anything already running won't see `cargo` until it's relaunched.

Verify it worked by opening a **new** Command Prompt and running:

```
cargo --version
```

You should see a version number.

### 1.2 Install Visual C++ Build Tools

Rust on Windows needs a linker, which comes from Visual Studio's Build
Tools. Install the "Desktop development with C++" workload via the
[Visual Studio Installer](https://visualstudio.microsoft.com/downloads/), or
grab the standalone Build Tools directly:
<https://aka.ms/vs/17/release/vs_BuildTools.exe>

### 1.3 Get the repo and the helper files

```
git clone https://github.com/WharfRatGames/Mini-Mayhem.git
```

Then place these two files in the repo root, next to `Cargo.toml`:

- **`build_and_deploy.bat`** — automates pulling, building, and packaging
- **`assets-backup.zip`** — a backup of assets upstream's git history is
  currently missing (see [§4](#4-known-upstream-issues) — without this,
  the build will fail with missing-file errors)

---

## 2. Building the game

### 2.1 The easy way

Double-click `build_and_deploy.bat`. It will:

1. Pull the latest changes from git
2. Restore any missing assets (both the upstream `deploy/assets.zip` and,
   if needed, the `assets-backup.zip` fallback)
3. Run `cargo build --bin mini-mayhem --release`
4. Assemble a ready-to-run folder at `dist\MiniMayhem-Windows\`
5. Ask if you want to launch it right away

That folder contains everything needed to run or share the game —
`mini-mayhem.exe`, all assets, and any runtime DLLs it needs.

### 2.2 Script flags

Run from a Command Prompt to combine flags:

| Flag | Effect |
|---|---|
| `nopull` | Skip `git pull`; build whatever's currently checked out |
| `zip` | Also produce a timestamped zip in `dist\` for sharing |
| `silent` | Skip the "launch now?" prompt (useful for automation) |

Example: `build_and_deploy.bat nopull zip silent`

### 2.3 Building manually

If you want to understand or customize the process:

```powershell
cd Mini-Mayhem
git pull

# Assets needed before the build can compile (see §4)
Expand-Archive -Path assets-backup.zip -DestinationPath assets -Force
Expand-Archive -Path deploy\assets.zip -DestinationPath . -Force

cargo build --bin mini-mayhem --release
```

The binary lands at `target\release\mini-mayhem.exe`. To run it, it needs
these alongside it:

```
mini-mayhem.exe
sfx\                    ← copy of deploy\assets\sfx\
backgrounds\
cosmetics\
GARCIA.png
title_bg.png
avatars\                (optional)
config.json             (optional)
```

---

## 3. Updating to new versions

Mini Mayhem is actively developed, so new commits land upstream regularly.
The workflow is simple in the common case:

```
build_and_deploy.bat
```

That's it — pull, rebuild, done. Because all the desktop-specific code is
isolated behind `#[cfg(...)]` blocks in a handful of files, most upstream
changes (new weapons, balance tweaks, UI changes) won't touch anything the
desktop port modified, so `git pull` merges cleanly almost every time.

### When it might not be that simple

If `git pull` fails with a merge conflict, it means an upstream commit
touched one of the files the desktop port modifies:

- `src/renderer/fb.rs`, `src/renderer/desktop_window.rs`
- `src/audio.rs`
- `src/input/state.rs`
- `src/updater.rs`
- `src/game/account.rs` (specifically the save-path logic)
- `src/main.rs` (a few specific lines)
- `Cargo.toml`

That needs a manual merge — resolving conflicts so both upstream's change
*and* the desktop-specific code survive. This is exactly the kind of thing
worth asking Claude to help with: hand over the conflict, or just ask for a
fresh merge of the latest upstream commits into the desktop port.

---

## 4. Known upstream issues

These aren't bugs in the desktop port — they're gaps in the main
Mini-Mayhem repo itself, worth flagging to the maintainer. The scripts and
files here work around them, but it's worth knowing what's going on.

### 4.1 Missing top-level `assets/` folder

A cleanup commit removed the top-level `assets/` folder (textures,
backgrounds, etc. — embedded into the binary at compile time) from git
tracking with **no replacement bundle**. A separate, similar folder
(`deploy/assets/`) *did* get a replacement zip committed
(`deploy/assets.zip`), but `assets/` did not.

**Practical effect:** a fresh clone, or any `git pull` that includes this
commit, leaves `assets/` missing entirely — and the build fails with
missing-file errors. This affects everyone building from the upstream repo
right now, not just the desktop port.

**Workaround:** keep `assets-backup.zip` around. `build_and_deploy.bat`
checks for the required files automatically and restores them from this
backup if missing.

### 4.2 Minigun and Uzi may be silent

The newer Minigun and Uzi weapons reference `minigun.wav` and `mac10.wav`,
but neither file is in `deploy/assets.zip` yet. They likely only exist on
the live multiplayer server's own asset-sync manifest — a mechanism the
desktop build deliberately doesn't use (see §5.3). Everything else should
have sound; just these two weapons may not, until the files are added to
the bundle upstream.

---

## 5. Technical reference

For anyone (including a future Claude session) picking this back up later.

### 5.1 What's platform-specific

Two backends exist behind compile-time feature flags, sharing one identical
public API so the rest of the game never needs to know which is active:

| | Miyoo Mini Plus (`target_arch = "arm"`) | Desktop (`desktop` feature, on by default) |
|---|---|---|
| Display | Raw `/dev/fb0` mmap + ioctl | `minifb` window |
| Input | Raw `/dev/input/event0` evdev | `minifb` keyboard + `gilrs` gamepad |
| Audio | dlopen'd ALSA | `rodio` (WASAPI / CoreAudio / ALSA) |
| Save data | `/mnt/SDCARD/App/...` | `<exe dir>/arty_data/` |
| Self-update | Real OTA (download + exec) | No-op stub — update via `git pull` instead |

Everything else — physics, networking, game logic, rendering math — is
completely untouched and identical on every platform. The multiplayer
protocol is plain `bincode` over TLS, with nothing platform-specific in it,
which is why Windows and Miyoo players can play in the same match.

### 5.2 Two real bugs fixed during the initial port

1. **Frames were never presented.** Every screen the game draws funnels
   through one function, `blit_to_fb`. On the Miyoo, writing pixels there
   is instantly visible (direct framebuffer mmap). On desktop, `minifb`
   needs an explicit `update_with_buffer()` call to actually push pixels to
   the window *and* process its event queue — that call was missing.
   Fixed by having `blit_to_fb` call `Framebuffer::present()` every frame.

2. **Long input-poll loops still looked frozen even after fix #1.** Some
   screens (the splash screen, confirmation prompts) loop on
   `poll_input → sleep` for several seconds without redrawing. Since
   minifb only processes window messages *during* a present call, those
   loops starved the message queue — Windows would report "Not
   Responding" and keypresses wouldn't register. Fixed by having input
   polling also re-present the last frame on every call, so the message
   queue gets pumped every game tick regardless of whether anything
   changed on screen.

### 5.3 Why desktop doesn't do asset auto-sync

The Miyoo build has a background task that compares its local files
against a manifest on the live server and downloads anything missing or
out of date — useful for an embedded device that's hard to manually
update. The desktop build deliberately doesn't do this; updates happen via
`git pull` + rebuild instead, same as any other PC game built from source.
This is also why newly-added sound effects (like `minigun.wav`) that only
exist on the server's sync manifest don't show up automatically on
desktop — they need to actually be committed to the repo.

### 5.4 The cross-compilation caveat

If you ever receive a `.exe` that wasn't built using the steps in this
guide (for example, cross-compiled from a Linux sandbox without a proper
Windows Rust toolchain), treat it as best-effort only. That kind of build
requires hand-patching Rust's standard library source to work around
missing official Windows target support, which is inherently less
trustworthy than a binary built normally with `rustup` on an actual Windows
machine, as described in this guide. If something a Linux-built `.exe`
does seems flaky, freezing, or oddly behaved, **building from source on
Windows directly is the more reliable fix.**

---

## 6. Controls

| Action | Keyboard | Gamepad |
|---|---|---|
| Move / aim | Arrow keys | D-pad |
| Fire / charge | A or Space | South (A / Cross) |
| Jump | B or Z | East (B / Circle) |
| Backflip | X | West (X / Square) |
| Y button | Y | North (Y / Triangle) |
| Previous weapon | Q | Left bumper |
| Next weapon | E | Right bumper |
| L2 | `[` | Left trigger |
| R2 | `]` | Right trigger |
| Start / pause | Enter | Start |
| Select | Tab | Select / Back |

Any XInput-compatible controller (Xbox, PlayStation via DS4Windows, etc.)
works automatically via `gilrs`. The keyboard always works regardless of
whether a controller is connected.

---

## 7. Troubleshooting

**"Rust/Cargo not found on PATH" even after installing Rust**
Almost always a stale environment in whatever window you're running from.
Try, in order: open a brand-new Command Prompt and run the script from
there directly; restart Windows Explorer (Task Manager → Windows Explorer
→ Restart); or just reboot. The updated `build_and_deploy.bat` also
auto-detects cargo at its default install location even if PATH hasn't
caught up yet, so this should now be rare.

**Build fails with a linker error**
Visual C++ Build Tools aren't installed — see [§1.2](#12-install-visual-c-build-tools).

**Build fails with missing-file / `include_bytes!` errors**
The asset folders aren't extracted yet — see [§4.1](#41-missing-top-level-assets-folder).
Run the batch script, or extract the zips manually per [§2.3](#23-building-manually).

**Windows Defender flags the `.exe`**
Common for any small, freshly-compiled, unsigned indie binary with low
install-base reputation — Defender's cloud heuristics weight that
combination heavily regardless of actual behavior. Building it yourself
from source (rather than running a binary someone else compiled and sent
you) is both more trustworthy and avoids re-triggering reputation-based
flags tied to a specific file hash.

**A specific weapon has no sound**
Check [§4.2](#42-minigun-and-uzi-may-be-silent) — a couple of newer weapons'
sound files aren't bundled yet upstream.

**The window appears but looks frozen / doesn't respond to input**
This was a real bug in earlier builds of the desktop port (see
[§5.2](#52-two-real-bugs-fixed-during-the-initial-port)), now fixed. If
you're hitting this on a build made from this guide's steps, make sure
you're on the latest source (`git pull`) and not running an old leftover
`.exe`.
