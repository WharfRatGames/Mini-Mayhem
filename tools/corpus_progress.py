#!/usr/bin/env python3
"""Show progress of the MapGEN corpus (see gen_mapgen_corpus.py).

Counts the PNGs present per type against a target and draws a bar per type,
plus generation rate and ETA derived from recent file mtimes (so it needs no
cooperation from the generator process).

Usage: python3 tools/corpus_progress.py [--target 400] [--watch]
       python3 tools/corpus_progress.py --out ~/arty-mapgen-corpus
"""
import argparse, sys, time
from pathlib import Path

TYPES = ["island", "cavern", "bng"]


def stats(d: Path):
    pngs = list(d.glob("*.png"))
    mtimes = sorted(p.stat().st_mtime for p in pngs)
    return len(pngs), mtimes


def rate_eta(mtimes, remaining, window=20):
    """maps/min over the last `window` files, and ETA seconds for `remaining`."""
    recent = mtimes[-window:]
    if len(recent) < 2:
        return None, None
    span = recent[-1] - recent[0]
    if span <= 0:
        return None, None
    per_sec = (len(recent) - 1) / span
    if per_sec <= 0:
        return None, None
    active = (time.time() - recent[-1]) < 30  # still producing?
    eta = remaining / per_sec if (remaining and active) else None
    return per_sec * 60, eta


def fmt_eta(sec):
    if sec is None:
        return "—"
    m, s = divmod(int(sec), 60)
    return f"{m}m{s:02d}s"


def draw(out_root: Path, target: int):
    lines = [f"MapGEN corpus  →  {out_root}", ""]
    for t in TYPES:
        n, mtimes = stats(out_root / t)
        remaining = max(target - n, 0)
        frac = min(n / target, 1.0) if target else 0.0
        bar = "#" * int(frac * 30)
        rpm, eta = rate_eta(mtimes, remaining)
        rate_s = f"{rpm:4.1f}/min" if rpm else "  idle  "
        lines.append(f"{t:8} [{bar:<30}] {n:>3}/{target}  {rate_s}  ETA {fmt_eta(eta)}")
    return "\n".join(lines)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(Path.home() / "arty-mapgen-corpus"))
    ap.add_argument("--target", type=int, default=400,
                    help="expected final count per type")
    ap.add_argument("--watch", action="store_true")
    args = ap.parse_args()
    out_root = Path(args.out)

    if not args.watch:
        print(draw(out_root, args.target))
        return 0
    try:
        while True:
            sys.stdout.write("\033[H\033[J" + draw(out_root, args.target) + "\n")
            sys.stdout.flush()
            time.sleep(2)
    except KeyboardInterrupt:
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
