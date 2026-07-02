#!/usr/bin/env python3
"""Extract a 1-bpp terrain silhouette mask from Worms Armageddon map data.

Converts WA's `DATA/land.dat` (written every time WA generates a map) — or a
monochrome PNG — into the baked mask format used by `src/world/wa_templates.rs`:
1920x696, 1 bit per pixel, row-major, MSB-first (167,040 bytes).

Workflow to grow the source-art library:
  1. Launch WA, generate a map of the desired type (island or cavern) in the
     map editor, start (or preview) a game so WA writes DATA/land.dat, quit.
  2. python3 tools/extract_wa_mask.py "assets/Worms Armageddon/DATA/land.dat" \
         -o src/world/wa_masks/islandN.bin --preview /tmp/islandN.ppm
  3. Eyeball the preview, then add the file to the mask array in
     src/world/wa_templates.rs (WA_ISLAND_MASKS or WA_CAVERN_MASKS).
  4. Bump VERSION (src/main.rs) and REQUIRED_VERSION (src/server/main.rs) —
     map generation changes on both ends.
  Repeat per map: land.dat only ever holds the most recent map.

Format notes (see https://worms2d.info/Land_Data_file, worms2d.info/Image_file,
and Syroot.Worms' reference implementation):
  land.dat: "LND\\x1a", u32 file size, i32 w, i32 h, u32 top_border (1 = cavern
  map with indestructible border), u32 water height, object-location table,
  then three IMG chunks: visual foreground, COLLISION MASK, background. The
  collision mask (2nd chunk) is the authoritative solid/air bitmap.
  IMG chunk: "IMG\\x1a", u32 size, u8 bpp (0 / >32 => optional description
  string precedes the real bpp), u8 flags (0x40 = Team17-LZ77 compressed,
  0x80 = palettized: u16 colour count + RGB triples; palette index 0 is
  implicitly black/transparent), i16 w, i16 h, pixel data. Any nonzero
  palette index = solid.
"""

import argparse
import struct
import sys

MASK_W, MASK_H = 1920, 696


def decompress_team17(data: bytes, out_size: int) -> bytearray:
    """Team17's LZ77 variant (see Syroot.Worms Team17Compression.cs)."""
    out = bytearray()
    pos = 0
    n = len(data)
    while pos < n and len(out) < out_size + 3:  # a few files overrun slightly
        cmd = data[pos]
        pos += 1
        if cmd & 0x80 == 0:
            out.append(cmd)  # literal (palette index 0..0x7F)
            continue
        if pos >= n:
            break
        arg1 = (cmd >> 3) & 0x0F
        arg2 = ((cmd << 8) | data[pos]) & 0x7FF
        pos += 1
        if arg1 == 0:
            if arg2 == 0:
                break  # end of stream
            if pos >= n:
                break
            count = data[pos] + 18
            pos += 1
            offset = arg2
        else:
            count = arg1 + 2
            offset = arg2 + 1
        for _ in range(count):
            out.append(out[-offset])
    return out[:out_size]


def parse_img(buf: bytes, off: int):
    """Parse one IMG chunk at `off`. Returns (width, height, indexed_pixels)."""
    if buf[off:off + 4] != b"IMG\x1a":
        raise ValueError(f"no IMG signature at offset {off}")
    p = off + 8  # skip signature + u32 file size
    bpp = buf[p]
    p += 1
    if bpp == 0:
        bpp = buf[p]
        p += 1
    elif bpp > 32:
        while buf[p] != 0:  # description string, zero-terminated
            p += 1
        p += 1
        bpp = buf[p]
        p += 1
    flags = buf[p]
    p += 1
    if flags & 0x80:  # palettized
        (count,) = struct.unpack_from("<H", buf, p)
        p += 2 + count * 3
    w, h = struct.unpack_from("<hh", buf, p)
    p += 4
    size = (bpp * w * h) // 8
    if flags & 0x40:  # Team17-LZ77 compressed
        pixels = decompress_team17(buf[p:], size)
    else:
        pixels = bytearray(buf[p:p + size])
    if len(pixels) < size:
        raise ValueError(f"IMG at {off}: decompressed {len(pixels)} of {size} bytes")
    if bpp == 1:
        # Collision masks are 1-bpp: unpack MSB-first to one byte per pixel.
        row_bytes = (w + 7) // 8
        pixels = bytearray(
            (pixels[y * row_bytes + (x >> 3)] >> (7 - (x & 7))) & 1
            for y in range(h) for x in range(w)
        )
    elif bpp != 8:
        raise ValueError(f"IMG at {off}: unsupported bpp {bpp}")
    return w, h, pixels


def load_land_dat(path: str):
    """Returns (width, height, solid_rows: list[list[bool]], top_border: bool)."""
    buf = open(path, "rb").read()
    if buf[:4] not in (b"LND\x1a", b"LND\x1b"):
        raise ValueError("not a land.dat file (missing LND signature)")
    land_w, land_h, top_border = struct.unpack_from("<iiI", buf, 8)
    # Locate IMG chunks by signature scan (robust against header variants).
    offsets = []
    start = 0
    while True:
        i = buf.find(b"IMG\x1a", start)
        if i < 0:
            break
        offsets.append(i)
        start = i + 4
    if not offsets:
        raise ValueError("no IMG chunks found")
    # Chunks: [foreground, collision mask, background]. Prefer the collision
    # mask — it is the authoritative solid/air bitmap.
    chunk = offsets[1] if len(offsets) >= 2 else offsets[0]
    w, h, pixels = parse_img(buf, chunk)
    if (w, h) != (land_w, land_h):
        print(f"note: IMG {w}x{h} differs from land header {land_w}x{land_h}")
    rows = [[pixels[y * w + x] != 0 for x in range(w)] for y in range(h)]
    return w, h, rows, bool(top_border)


def load_png(path: str, threshold: float):
    try:
        from PIL import Image
    except ImportError:
        sys.exit("PNG input requires Pillow: pip install Pillow")
    img = Image.open(path)
    if img.mode == "RGBA":
        alpha = img.getchannel("A")
        rows = [[alpha.getpixel((x, y)) > 0 for x in range(img.width)]
                for y in range(img.height)]
    else:
        g = img.convert("L")
        cut = threshold * 255
        rows = [[g.getpixel((x, y)) >= cut for x in range(img.width)]
                for y in range(img.height)]
    return img.width, img.height, rows, False


def rescale_nearest(rows, w, h):
    if (w, h) == (MASK_W, MASK_H):
        return rows
    print(f"rescaling {w}x{h} -> {MASK_W}x{MASK_H} (nearest)")
    return [[rows[y * h // MASK_H][x * w // MASK_W] for x in range(MASK_W)]
            for y in range(MASK_H)]


def seal_border(rows):
    """Cavern masks must be a closed hull: force a 1px solid border."""
    for x in range(MASK_W):
        rows[0][x] = True
        rows[MASK_H - 1][x] = True
    for y in range(MASK_H):
        rows[y][0] = True
        rows[y][MASK_W - 1] = True


def pack_1bpp(rows) -> bytes:
    out = bytearray(MASK_W * MASK_H // 8)
    for y in range(MASK_H):
        base = y * (MASK_W // 8)
        row = rows[y]
        for x in range(MASK_W):
            if row[x]:
                out[base + (x >> 3)] |= 1 << (7 - (x & 7))
    return bytes(out)


def write_preview(rows, path):
    """PBM/PPM preview (openable by any viewer; no dependencies)."""
    with open(path, "wb") as f:
        f.write(f"P5\n{MASK_W} {MASK_H}\n255\n".encode())
        f.write(bytes(0 if rows[y][x] else 255
                      for y in range(MASK_H) for x in range(MASK_W)))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("input", help="land.dat or monochrome/RGBA PNG")
    ap.add_argument("-o", "--output", required=True, help="output .bin mask path")
    ap.add_argument("--cavern", action="store_true",
                    help="treat as enclosed cavern art: seal a 1px solid border")
    ap.add_argument("--preview", help="write a PGM preview image here")
    ap.add_argument("--threshold", type=float, default=0.5,
                    help="PNG luminance threshold (default 0.5)")
    args = ap.parse_args()

    if open(args.input, "rb").read(4) in (b"LND\x1a", b"LND\x1b"):
        w, h, rows, top_border = load_land_dat(args.input)
        if top_border and not args.cavern:
            print("note: land.dat has top_border set (cavern map) — "
                  "consider --cavern and saving as a cavernN.bin")
    else:
        w, h, rows, _ = load_png(args.input, args.threshold)

    rows = rescale_nearest(rows, w, h)
    if args.cavern:
        seal_border(rows)

    solid = sum(sum(r) for r in rows) / (MASK_W * MASK_H)
    kind = "cavern" if args.cavern else "island"
    print(f"solid fraction: {solid:.3f} "
          f"(sane {kind} range: {'0.55-0.85' if args.cavern else '0.20-0.50'})")

    data = pack_1bpp(rows)
    assert len(data) == 167040
    with open(args.output, "wb") as f:
        f.write(data)
    print(f"wrote {args.output} ({len(data)} bytes)")
    if args.preview:
        write_preview(rows, args.preview)
        print(f"preview: {args.preview}")


if __name__ == "__main__":
    main()
