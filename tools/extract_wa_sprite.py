#!/usr/bin/env python3
"""Extract a sprite animation from Worms Armageddon's Gfx.dir into a baked asset.

Parses a `.spr` entry packed inside `DATA/Gfx/Gfx.dir` and writes the WSPR
format consumed by `src/renderer/wa_sprites.rs`:

  "WSPR" magic (4 bytes)
  u16 frame_w, u16 frame_h, u16 frame_count, u16 pal_count   (little-endian)
  pal_count x 3 bytes RGB
  frame_count x frame_w x frame_h bytes of palette indices
      (0 = transparent, v > 0 -> palette[v-1])

Known entries in the retail Gfx.dir (locate others with `strings -t x Gfx.dir`
and the DIR table: each table entry is {u32 offset, u32 size, name}):
  wbackflp.spr  --offset 0x14c674 --size 0x18bf   (worm backflip, 22 frames)

Usage:
  python3 tools/extract_wa_sprite.py "assets/Worms Armageddon/DATA/Gfx/Gfx.dir" \
      --offset 0x14c674 --size 0x18bf \
      -o src/renderer/wa_sprites/wbackflp.bin --preview /tmp/backflip.ppm

.spr layout (reverse-engineered; verified against wbackflp.spr):
  "SPR\\x1a", u32 file size, u8 bpp (8), u8 flags (0xC0 = compressed+palettized),
  u16 palette count, RGB triples,
  u32 chunk_count, chunk_count x {u32 comp_offset, u32 zero, u32 dec_size}
      (comp_offset relative to the start of compressed data),
  3 x u16 unknown, u16 frame_w, u16 frame_h, u16 frame_count,
  frame_count x {u32 (chunk_idx << 24 | offset_in_decompressed_chunk),
                 i16 x, i16 y, i16 right, i16 bottom},
  Team17-LZ77 compressed chunks (same codec as land.dat, see
  tools/extract_wa_mask.py). Frame pixels are a tight (right-x)*(bottom-y)
  run placed at (x, y) inside the frame_w x frame_h box.
"""

import argparse
import struct
import sys

from extract_wa_mask import decompress_team17


def parse_spr(spr: bytes):
    """Returns (frame_w, frame_h, palette: list[(r,g,b)], frames: list[bytearray])."""
    if spr[:4] != b"SPR\x1a":
        raise ValueError("missing SPR signature (wrong --offset?)")
    p = 8
    bpp, flags = spr[p], spr[p + 1]
    p += 2
    if bpp != 8 or flags != 0xC0:
        raise ValueError(f"unsupported spr variant: bpp={bpp} flags={flags:#x}")
    (pal_count,) = struct.unpack_from("<H", spr, p)
    p += 2
    palette = [tuple(spr[p + i * 3:p + i * 3 + 3]) for i in range(pal_count)]
    p += pal_count * 3
    (chunk_count,) = struct.unpack_from("<I", spr, p)
    p += 4
    chunk_hdrs = []
    for _ in range(chunk_count):
        comp_off, _zero, dec_size = struct.unpack_from("<III", spr, p)
        p += 12
        chunk_hdrs.append((comp_off, dec_size))
    p += 6  # three unknown u16 (0, 0x23, 0 in wbackflp.spr)
    frame_w, frame_h, frame_count = struct.unpack_from("<HHH", spr, p)
    p += 6
    entries = []
    for _ in range(frame_count):
        off, x, y, right, bottom = struct.unpack_from("<Ihhhh", spr, p)
        p += 12
        entries.append((off >> 24, off & 0xFFFFFF, x, y, right, bottom))
    data_start = p
    chunks = [decompress_team17(spr[data_start + comp_off:], dec_size)
              for comp_off, dec_size in chunk_hdrs]

    frames = []
    for chunk_idx, off, x, y, right, bottom in entries:
        w, h = right - x, bottom - y
        src = chunks[chunk_idx]
        box = bytearray(frame_w * frame_h)
        for row in range(h):
            base = off + row * w
            box[(y + row) * frame_w + x:(y + row) * frame_w + x + w] = \
                src[base:base + w]
        frames.append(box)
    return frame_w, frame_h, palette, frames


def write_preview(path, frame_w, frame_h, palette, frames, per_row=11):
    rows = (len(frames) + per_row - 1) // per_row
    sw, sh = frame_w * per_row, frame_h * rows
    img = bytearray(sw * sh * 3)
    for i, frame in enumerate(frames):
        bx, by = (i % per_row) * frame_w, (i // per_row) * frame_h
        for yy in range(frame_h):
            for xx in range(frame_w):
                v = frame[yy * frame_w + xx]
                if v == 0:
                    continue
                o = ((by + yy) * sw + bx + xx) * 3
                img[o:o + 3] = bytes(palette[v - 1])
    with open(path, "wb") as f:
        f.write(f"P6\n{sw} {sh}\n255\n".encode())
        f.write(img)


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("gfx_dir", help="path to DATA/Gfx/Gfx.dir")
    ap.add_argument("--offset", required=True, help="spr offset in Gfx.dir (e.g. 0x14c674)")
    ap.add_argument("--size", required=True, help="spr size in bytes (e.g. 0x18bf)")
    ap.add_argument("-o", "--output", required=True, help="output .bin (WSPR) path")
    ap.add_argument("--preview", help="write a PPM contact sheet here")
    args = ap.parse_args()

    off, size = int(args.offset, 0), int(args.size, 0)
    with open(args.gfx_dir, "rb") as f:
        f.seek(off)
        spr = f.read(size)
    frame_w, frame_h, palette, frames = parse_spr(spr)
    print(f"{len(frames)} frames {frame_w}x{frame_h}, {len(palette)} colours")

    out = bytearray(b"WSPR")
    out += struct.pack("<HHHH", frame_w, frame_h, len(frames), len(palette))
    for rgb in palette:
        out += bytes(rgb)
    for frame in frames:
        out += frame
    with open(args.output, "wb") as f:
        f.write(out)
    print(f"wrote {args.output} ({len(out)} bytes)")
    if args.preview:
        write_preview(args.preview, frame_w, frame_h, palette, frames)
        print(f"preview: {args.preview}")


if __name__ == "__main__":
    main()
