#!/usr/bin/env python3
"""Compare terrain-silhouette statistics: MapGEN reference corpus vs. our
`Terrain::generate_tactical` output.

Inputs:
  --corpus DIR   MapGEN PNGs (1920x696) laid out DIR/{island,cavern,bng}/*.png
  --ours DIR     dump-terrain output DIR/{island,cavern}/*.bin
                 (1920x960 1-bpp MSB-first; terrain band rows 80..840)

For each map we compute silhouette metrics, then print per-class aggregate
tables (median [p10..p90]) side by side. MapGEN's island+bng classes are both
open-air maps and are compared against our "island" class.

Sprite cleanup for corpus PNGs reuses extract_wa_mask.clean_sprites.
"""
import argparse, sys
from pathlib import Path
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from extract_wa_mask import clean_sprites  # noqa: E402

W = 1920
OUR_H, OUR_BAND = 960, (80, 840)   # TERRAIN_MIN_Y .. WATER_Y
PNG_H = 696


def load_our_bin(path):
    bits = np.frombuffer(Path(path).read_bytes(), dtype=np.uint8)
    a = np.unpackbits(bits).reshape(OUR_H, W).astype(bool)
    return a[OUR_BAND[0]:OUR_BAND[1]]


def load_corpus_png(path, cavern):
    from PIL import Image
    img = Image.open(path).convert("RGB")
    a = np.asarray(img).sum(axis=2) > 8   # non-black = solid
    rows = clean_sprites(a.tolist(), img.width, img.height, cavern)
    return np.array(rows, dtype=bool)


def metrics(solid: np.ndarray, cavern: bool):
    h, w = solid.shape
    m = {}
    m["solid_frac"] = solid.mean()

    # Surface profile: first solid row per column (h = "no ground").
    first = np.where(solid.any(axis=0), solid.argmax(axis=0), h)
    ground = first < h
    m["ground_cols"] = ground.mean()

    prof = first[ground].astype(float)
    if prof.size > 24:
        gx = np.flatnonzero(ground)
        # height delta over 12px windows (only between nearby ground columns)
        deltas = []
        for i in range(len(gx) - 12):
            if gx[i + 12] - gx[i] <= 24:
                deltas.append(abs(prof[i + 12] - prof[i]))
        deltas = np.array(deltas) if deltas else np.array([0.0])
        m["rough_p50"] = np.percentile(deltas, 50)
        m["rough_p95"] = np.percentile(deltas, 95)
        m["cliff_frac"] = (deltas > 60).mean()
        # waviness: fraction of surface-profile spectral power in low bins
        p = prof - prof.mean()
        spec = np.abs(np.fft.rfft(p)) ** 2
        tot = spec[1:].sum()
        m["lowfreq_share"] = spec[1:9].sum() / tot if tot > 0 else 0.0
        m["surf_span"] = np.percentile(prof, 90) - np.percentile(prof, 10)
    # Overhang: air strictly below some solid in the same column.
    below_top = (np.arange(h)[:, None] > first[None, :]) & ground[None, :]
    m["overhang_frac"] = ((~solid) & below_top).mean()

    # Connected components (8-neighbour approximated by 4-neighbour).
    lab = label4(solid)
    sizes = np.bincount(lab.ravel())[1:]
    sizes = sizes[sizes > 50]
    m["n_chunks"] = len(sizes)
    if cavern:
        alab = label4(~solid)
        asz = np.sort(np.bincount(alab.ravel())[1:])[::-1]
        asz = asz[asz > 500]
        m["n_chambers"] = len(asz)
        m["chamber_share"] = asz[0] / (~solid).sum() if len(asz) else 0.0
    else:
        # edge water: solid pixels in outermost 4 columns
        m["edge_solid"] = solid[:, list(range(4)) + list(range(w - 4, w))].mean()
    return m


def label4(a: np.ndarray) -> np.ndarray:
    """Tiny 4-neighbour connected-component labeling (no scipy)."""
    h, w = a.shape
    lab = np.zeros((h, w), dtype=np.int32)
    cur = 0
    for sy, sx in zip(*np.nonzero(a)):
        if lab[sy, sx]:
            continue
        cur += 1
        stack = [(sy, sx)]
        lab[sy, sx] = cur
        while stack:
            y, x = stack.pop()
            if y > 0 and a[y - 1, x] and not lab[y - 1, x]:
                lab[y - 1, x] = cur; stack.append((y - 1, x))
            if y < h - 1 and a[y + 1, x] and not lab[y + 1, x]:
                lab[y + 1, x] = cur; stack.append((y + 1, x))
            if x > 0 and a[y, x - 1] and not lab[y, x - 1]:
                lab[y, x - 1] = cur; stack.append((y, x - 1))
            if x < w - 1 and a[y, x + 1] and not lab[y, x + 1]:
                lab[y, x + 1] = cur; stack.append((y, x + 1))
    return lab


def agg(rows):
    out = {}
    keys = sorted({k for r in rows for k in r})
    for k in keys:
        v = np.array([r[k] for r in rows if k in r], dtype=float)
        out[k] = (np.median(v), np.percentile(v, 10), np.percentile(v, 90))
    return out


def show(title, groups):
    print(f"\n== {title} ==")
    keys = sorted({k for g in groups.values() for k in g})
    hdr = f"{'metric':<16}" + "".join(f"{n:>28}" for n in groups)
    print(hdr)
    for k in keys:
        line = f"{k:<16}"
        for g in groups.values():
            if k in g:
                med, lo, hi = g[k]
                line += f"{med:>10.3f} [{lo:>6.3f}..{hi:>6.3f}]"
            else:
                line += f"{'-':>28}"
        print(line)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--corpus", required=True)
    ap.add_argument("--ours", required=True)
    ap.add_argument("--limit", type=int, default=0, help="cap maps per class")
    ap.add_argument("--cache", help="JSON-lines per-map metric cache; reruns "
                                    "skip maps already present (crash-resumable)")
    args = ap.parse_args()

    cache = {}
    if args.cache and Path(args.cache).exists():
        import json
        for line in Path(args.cache).read_text().splitlines():
            rec = json.loads(line)
            cache[rec["path"]] = rec["metrics"]

    def run(paths, loader, cavern):
        rows = []
        for i, p in enumerate(sorted(paths)):
            if args.limit and i >= args.limit:
                break
            key = str(p)
            if key in cache:
                rows.append(cache[key])
                continue
            m = {k: float(v) for k, v in metrics(loader(p), cavern).items()}
            rows.append(m)
            if args.cache:
                import json
                with open(args.cache, "a") as f:
                    f.write(json.dumps({"path": key, "metrics": m}) + "\n")
            print(f"  {p}", file=sys.stderr)
        return rows

    c = Path(args.corpus)
    o = Path(args.ours)
    groups_isl = {
        "mapgen island": agg(run((c / "island").glob("*.png"),
                                 lambda p: load_corpus_png(p, False), False)),
        "mapgen bng": agg(run((c / "bng").glob("*.png"),
                              lambda p: load_corpus_png(p, False), False)),
        "ours island": agg(run((o / "island").glob("*.bin"),
                               load_our_bin, False)),
    }
    show("OPEN-AIR (island/bng)", groups_isl)
    groups_cav = {
        "mapgen cavern": agg(run((c / "cavern").glob("*.png"),
                                 lambda p: load_corpus_png(p, True), True)),
        "ours cavern": agg(run((o / "cavern").glob("*.bin"),
                               load_our_bin, True)),
    }
    show("CAVERN", groups_cav)


if __name__ == "__main__":
    main()
