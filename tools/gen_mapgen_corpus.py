#!/usr/bin/env python3
"""Generate a reference corpus of WA MapGEN maps under Wine/Xvfb.

MapGEN CLI: MapGen.exe -t GAME_TYPE [-o OUT_FILE] [-w]
  -w stops once the terrain shape is done (skips full art pass where possible;
  decorative sprites are still baked in and are stripped later by
  extract_wa_mask.py --clean-sprites).

Shape variety comes from MapGEN's internal per-run RNG; each invocation
produces a new layout. Output: PNGs 1920x696 (native, same as our mask size)
under OUT_DIR/<type>/NNN.png.

Usage: python3 tools/gen_mapgen_corpus.py [--count 50] [--out ~/arty-mapgen-corpus]
Requires: an X display (runs Xvfb :99 itself if not already up).
"""
import argparse, os, subprocess, sys, time
from pathlib import Path

MAPGEN_DIR = Path(__file__).resolve().parent.parent / "assets/Worms Armageddon/User/MapGen"
TYPES = ["island", "cavern", "bng"]
DISPLAY = ":99"


def ensure_xvfb():
    r = subprocess.run(["xdpyinfo", "-display", DISPLAY], capture_output=True)
    if r.returncode != 0:
        subprocess.Popen(["Xvfb", DISPLAY, "-screen", "0", "1024x768x24"],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        time.sleep(2)


def gen_one(out_path: Path, gtype: str, complexity: int) -> bool:
    env = dict(os.environ, DISPLAY=DISPLAY, WINEDEBUG="-all")
    # Settings file: kill decorative objects/floaters so the PNG is (nearly)
    # a pure terrain silhouette; sweep complexity for shape variety.
    sf = out_path.with_suffix(".settings.txt")
    sf.write_text(f"type {gtype}\nwidth 1920\nheight 696\n"
                  f"objects 0\nfloaters 0\ncomplexity {complexity}\nwater 0\n")
    r = subprocess.run(
        ["wine", "MapGen.exe", "-s", str(sf), "-o", str(out_path), "-w"],
        cwd=MAPGEN_DIR, env=env, capture_output=True, timeout=180)
    sf.unlink(missing_ok=True)
    ok = out_path.exists() and out_path.stat().st_size > 10000
    if not ok:
        sys.stderr.write(f"FAIL {gtype} -> {out_path}: rc={r.returncode}\n")
    return ok


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--count", type=int, default=50)
    ap.add_argument("--out", default=str(Path.home() / "arty-mapgen-corpus"))
    ap.add_argument("--types", nargs="*", default=TYPES)
    args = ap.parse_args()

    ensure_xvfb()
    out_root = Path(args.out)
    fails = 0
    for gtype in args.types:
        d = out_root / gtype
        d.mkdir(parents=True, exist_ok=True)
        for i in range(args.count):
            p = d / f"{i:03d}.png"
            if p.exists():
                continue
            complexity = 10 + (i * 80) // max(args.count - 1, 1)  # sweep 10..90
            if not gen_one(p, gtype, complexity):
                fails += 1
                if not gen_one(p, gtype, complexity):  # one retry
                    fails += 1
            print(f"{gtype} {i + 1}/{args.count}", flush=True)
    print(f"done, failures={fails}")
    return 1 if fails else 0


if __name__ == "__main__":
    sys.exit(main())
