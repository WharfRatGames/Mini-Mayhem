# Paper-doll cosmetic parts

Authoring spec for sprite parts used by `tools/paperdoll.py`. Run

```bash
python3 tools/paperdoll.py --make-templates tools/paperdoll_parts
```

to (re)generate starter PNGs + `parts.json` in this directory, then edit the
PNGs in place (keep the same canvas size and pivot, or update `parts.json`
if you resize).

## Scale & orientation

- **Scale**: 1 game pixel = 3×3 image pixels (same convention as the existing
  `hat_*.png` / `gun_*.png` cosmetics in `assets/cosmetics/`).
- **Every limb/torso part is authored pointing "up"** (toward the top of the
  image), with its **pivot at the bottom** — the joint it attaches to. The
  tool rotates each part around its pivot to match the rig's computed bone
  angle for the current pose/frame, so a part that looks correct pointing
  straight up will look correct at any rotation.
- `head` and `boot` are placed (not stretched) — `head` rotates with the
  torso/head bone angle; `boot` is drawn unrotated (just flipped
  horizontally when facing left).

## Parts & sizes (game px / image px @ 3x)

| Part        | Size (img px) | Pivot (img px) | Represents                              |
|-------------|---------------|-----------------|------------------------------------------|
| `torso.png`     | 21 × 39 | (10, 39) | hip → shoulder (bone length 13)        |
| `head.png`      | 36 × 36 | (18, 18) | head circle, centered on head anchor   |
| `arm.png`       | 15 × 27 | (7, 27)  | shoulder → hand (bone length 9)        |
| `leg_upper.png` | 15 × 18 | (7, 18)  | hip → knee                              |
| `leg_lower.png` | 15 × 15 | (7, 15)  | knee → foot                             |
| `boot.png`      | 12 × 9  | (6, 2)   | 4×3 game-px boot block at the foot      |

These are starting sizes only — `parts.json` controls the actual pivot used,
so you can resize a part as long as you update its `pivot` entry to match
(in image pixels, measured from the top-left corner).

## Recolour placeholders

Paint these **exact** RGBA colours into a part to have `paperdoll.py`
substitute the in-game team/uniform/boot colour at render time (alpha is
preserved, so anti-aliased edges still work if the edge pixel itself isn't
a placeholder colour):

- `(255, 0, 255, 255)` magenta → uniform colour (or team colour if
  uniform = 0). Used by `torso`, `arm`, `leg_upper`, and the upper part of
  `leg_lower`.
- `(0, 255, 255, 255)` cyan → boot colour. Used by `boot` and the lower part
  of `leg_lower`.

Everything else (outlines, skin, details) is drawn as authored — use the
game's dark outline colour `(22, 14, 6)` for outlines to match the
procedural renderer.

## Hats & guns

Hats and guns reuse the existing `assets/cosmetics/hat_<id>.png` /
`gun_<id>.png` sprites and anchors documented in
`assets/cosmetics/SPECS.txt` — no new files needed; pass `--hat N`/`--gun N`
to `paperdoll.py`.

## Canvas

The tool renders onto an 80×80 game-pixel (240×240px) canvas with the hip
fixed near (40, 53) — feet land around y=64, leaving room below for boots
and above for a tall hat. This matches `CANVAS_GP`/`HIP_GP` in
`tools/paperdoll.py`; change those constants if you need more room.
