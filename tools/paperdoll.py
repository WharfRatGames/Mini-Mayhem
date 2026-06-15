#!/usr/bin/env python3
"""paperdoll.py — sprite-based paper-doll cosmetic compositor/preview for Arty.

Ports the skeletal forward-kinematics from src/renderer/skeleton.rs so that
PNG body-part sprites can be authored once and previewed in every pose the
game renders (idle, walk cycle, airborne, backflip/spin, dead), at any team
colour / uniform / boot / hat / gun combination.

Quick start
-----------
    # 1. Generate a starter set of part templates + parts.json manifest:
    python3 tools/paperdoll.py --make-templates tools/paperdoll_parts

    # 2. Edit the PNGs in tools/paperdoll_parts (see README.md there for the
    #    authoring spec: orientation, pivots, recolour placeholder colours).

    # 3. Preview a single pose:
    python3 tools/paperdoll.py tools/paperdoll_parts --pose walk --frame 0 \
        --team 1 --uniform 2 --hat 3 --gun 1 -o /tmp/preview.png

    # 4. Render a walk-cycle sprite sheet:
    python3 tools/paperdoll.py tools/paperdoll_parts --sheet -o /tmp/walk.png

All "game pixel" coordinates below match src/renderer/skeleton.rs exactly
(bone lengths, joint formulas, pose functions) so a part authored to the
spec in paperdoll_parts/README.md will line up with the procedural rig.
"""
from __future__ import annotations

import argparse
import json
import math
import os
from dataclasses import dataclass

from PIL import Image

# ── Constants ported from src/renderer/skeleton.rs ──────────────────────────

SCALE = 3  # 1 game pixel = 3x3 image pixels (matches hat_*.png / gun_*.png)

TORSO, HEAD, ARM_R, ARM_L, LEG_R, LEG_L = range(6)
PI = math.pi
TAU = 2 * math.pi

# (parent, length, default_angle)
DEFAULT_BONES = {
    TORSO: (None,  13.0, 0.0),
    HEAD:  (TORSO, 6.0,  0.0),
    ARM_R: (TORSO, 9.0,  0.0),
    ARM_L: (TORSO, 9.0,  0.0),
    LEG_R: (None,  11.0, PI),
    LEG_L: (None,  11.0, PI),
}

TEAM_COLOURS = [
    (220, 80, 80),    # Red
    (80, 120, 220),   # Blue
    (80, 200, 80),    # Green
    (220, 180, 40),   # Yellow
]
TEAM_COLOURS_DEAD = [
    (100, 40, 40),
    (40, 60, 100),
    (40, 90, 40),
    (100, 80, 20),
]
UNIFORM_COLOURS = {
    0: None,  # use team colour
    1: (60, 100, 50),    # Camo Green
    2: (190, 155, 90),   # Desert Tan
    3: (30, 30, 35),     # Midnight Black
    4: (230, 230, 235),  # Snow White
    5: (30, 40, 120),    # Navy
    6: (200, 120, 160),  # Pink Camo
    7: (200, 165, 40),   # Gold Plate
}
BOOT_COLOURS = {
    0: (35, 30, 22),   # Default dark brown
    1: (180, 40, 40),  # Red
    2: (220, 215, 205),# White
    3: (190, 155, 30), # Gold
    4: (50, 80, 40),   # Combat Green
    5: (30, 80, 220),  # Electric Blue
}

# Recolour placeholder pixel colours used in part sprites (see README.md).
BODY_PLACEHOLDER = (255, 0, 255, 255)   # magenta -> uniform/team colour
BOOT_PLACEHOLDER = (0, 255, 255, 255)   # cyan    -> boot colour

# Canvas: hip is placed at (HIP_X, HIP_Y) in game pixels. Chosen with enough
# headroom for a hat (up to ~12px above the head) and footroom for boots.
CANVAS_GP = (80, 80)          # canvas size in game pixels
HIP_GP = (40.0, 53.0)         # hip position in game pixels (-> feet at y=64)


def rot(x: float, y: float, a: float) -> tuple[float, float]:
    s, c = math.sin(a), math.cos(a)
    return (x * c - y * s, x * s + y * c)


def smoothstep(t: float) -> float:
    return t * t * (3.0 - 2.0 * t)


def walk_swing_r(tick: int) -> float:
    STRIDE = 20.0
    phase = (tick % STRIDE) / STRIDE
    t4 = phase * 4.0
    frac = smoothstep(t4 - math.floor(t4))
    k = int(t4) % 4
    if k == 0:
        return 1.0 - frac
    if k == 1:
        return -frac
    if k == 2:
        return -1.0 + frac
    return frac


# ── Pose functions (ported from skeleton.rs) ────────────────────────────────

def pose_idle(angles: dict, t: float) -> None:
    breath = math.sin(t * 1.8) * 0.04
    angles[TORSO] = breath
    angles[HEAD] = -breath * 0.5
    angles[ARM_R] = 0.6
    angles[ARM_L] = -0.6
    angles[LEG_R] = PI - 0.05
    angles[LEG_L] = PI + 0.05


def pose_walk(angles: dict, tick: int, facing: float) -> float:
    LEG_AMP = 0.6
    ARM_AMP = 0.26
    swing_r = walk_swing_r(tick)
    bob = swing_r * swing_r
    angles[TORSO] = swing_r * 0.06
    angles[HEAD] = (0.5 - bob) * 0.08
    angles[LEG_R] = PI + swing_r * LEG_AMP
    angles[LEG_L] = PI - swing_r * LEG_AMP
    angles[ARM_R] = -swing_r * ARM_AMP * facing + 0.4
    angles[ARM_L] = swing_r * ARM_AMP * facing - 0.4
    return swing_r


def pose_airborne(angles: dict, vel_x: float, vel_y: float) -> None:
    lean = max(-0.5, min(0.5, vel_x * 0.025))
    tuck = max(-0.4, min(0.25, -vel_y * 0.03))
    angles[TORSO] = lean
    angles[HEAD] = -lean * 0.3
    angles[LEG_R] = PI + tuck + 0.15
    angles[LEG_L] = PI + tuck - 0.15
    angles[ARM_R] = lean * 0.5 + 0.4
    angles[ARM_L] = lean * 0.5 - 0.4


def pose_spin(angles: dict, airtime: int, facing: float) -> None:
    angle = -(facing) * airtime / 18.0 * TAU
    angles[TORSO] = angle
    angles[HEAD] = 0.0
    angles[ARM_R] = 0.35
    angles[ARM_L] = -0.35
    angles[LEG_R] = PI + angle + 0.12
    angles[LEG_L] = PI + angle - 0.12


def pose_dead(angles: dict, facing: float) -> None:
    flop = (PI / 2) * facing
    angles[TORSO] = flop
    angles[HEAD] = (PI / 4) * facing
    angles[LEG_R] = PI + 0.9
    angles[LEG_L] = PI + 0.3
    angles[ARM_R] = -1.1
    angles[ARM_L] = 0.4


# ── Rig: computes every joint position needed by the renderer ───────────────

@dataclass
class Rig:
    hip: tuple[float, float]
    shoulder: tuple[float, float]
    head_cx: float
    head_cy: float
    head_world_angle: float
    arm_orig: tuple[float, float]
    back_arm: tuple[float, float]
    fwd_arm: tuple[float, float]
    back_knee: tuple[float, float]
    back_foot: tuple[float, float]
    front_knee: tuple[float, float]
    front_foot: tuple[float, float]


def compute_positions(root, angles) -> dict:
    origins = {}
    world_angles = {}
    ends = {}
    for i in range(6):
        parent, length, _default = DEFAULT_BONES[i]
        parent_origin = root if parent is None else ends[parent]
        parent_angle = 0.0 if parent is None else world_angles[parent]
        origins[i] = parent_origin
        wa = parent_angle + angles[i]
        world_angles[i] = wa
        dx, dy = rot(0.0, -length, wa)
        ends[i] = (parent_origin[0] + dx, parent_origin[1] + dy)
    return ends, world_angles


def build_rig(pose: str, *, tick: int = 0, facing: int = 1,
               vel_x: float = 0.0, vel_y: float = 0.0,
               airtime: int = 0, aim_deg: float | None = None) -> Rig:
    f = float(facing)
    angles = {i: DEFAULT_BONES[i][2] for i in range(6)}
    walk_sr = 0.0

    if pose == "idle":
        pose_idle(angles, 0.0)
        root = (HIP_GP[0], HIP_GP[1])
    elif pose == "walk":
        walk_sr = pose_walk(angles, tick, f)
        bob = walk_sr * walk_sr
        rise = (1.0 - bob) * 2.0
        root = (HIP_GP[0] + walk_sr * 3.0 * f, HIP_GP[1] - rise)
    elif pose == "airborne":
        pose_airborne(angles, vel_x, vel_y)
        root = (HIP_GP[0], HIP_GP[1])
    elif pose == "spin":
        pose_spin(angles, airtime, f)
        root = (HIP_GP[0], HIP_GP[1])
    elif pose == "dead":
        pose_dead(angles, f)
        root = (HIP_GP[0], HIP_GP[1])
    else:
        raise ValueError(f"unknown pose {pose!r}")

    # Aim override (replaces the facing-side arm angle).
    if aim_deg is not None:
        aim = math.radians(aim_deg)
        torso_world = angles[TORSO]
        aim_disp = aim if f >= 0.0 else PI - aim
        arm_world = (PI / 2) - aim_disp
        arm_local = arm_world - torso_world
        if f >= 0.0:
            angles[ARM_R] = arm_local
        else:
            angles[ARM_L] = arm_local

    ends, world_angles = compute_positions(root, angles)

    hip = root
    shoulder = ends[TORSO]
    head_cx, head_cy = shoulder[0], shoulder[1] - 4.0
    head_world_angle = world_angles[HEAD]

    arm_orig = (hip[0] + (shoulder[0] - hip[0]) * 0.70,
                hip[1] + (shoulder[1] - hip[1]) * 0.70)
    shift = (arm_orig[0] - shoulder[0], arm_orig[1] - shoulder[1])
    arm_r_vis = (ends[ARM_R][0] + shift[0], ends[ARM_R][1] + shift[1])
    arm_l_vis = (ends[ARM_L][0] + shift[0], ends[ARM_L][1] + shift[1])

    if pose == "dead":
        bend_r = bend_l = 0.0
    else:
        bend_r = (1.0 - walk_sr) * 0.5 * 3.5
        bend_l = (1.0 + walk_sr) * 0.5 * 3.5

    leg_r_end, leg_l_end = ends[LEG_R], ends[LEG_L]
    knee_r = ((hip[0] + leg_r_end[0]) * 0.5 + f * bend_r, (hip[1] + leg_r_end[1]) * 0.5)
    knee_l = ((hip[0] + leg_l_end[0]) * 0.5 + f * bend_l, (hip[1] + leg_l_end[1]) * 0.5)

    if walk_sr >= 0.0:
        back_knee, back_foot, front_knee, front_foot = knee_l, leg_l_end, knee_r, leg_r_end
    else:
        back_knee, back_foot, front_knee, front_foot = knee_r, leg_r_end, knee_l, leg_l_end

    if f >= 0.0:
        back_arm, fwd_arm = arm_l_vis, arm_r_vis
    else:
        back_arm, fwd_arm = arm_r_vis, arm_l_vis

    return Rig(hip, shoulder, head_cx, head_cy, head_world_angle,
                arm_orig, back_arm, fwd_arm,
                back_knee, back_foot, front_knee, front_foot)


# ── Sprite compositing ───────────────────────────────────────────────────────

def recolor(img: Image.Image, mapping: dict[tuple, tuple]) -> Image.Image:
    """Replace exact RGBA placeholder colours with target RGB (keeps alpha)."""
    if not mapping:
        return img
    img = img.convert("RGBA")
    px = img.load()
    w, h = img.size
    for y in range(h):
        for x in range(w):
            p = px[x, y]
            if p in mapping:
                tgt = mapping[p]
                px[x, y] = (tgt[0], tgt[1], tgt[2], p[3])
    return img


def paste_rotated(canvas: Image.Image, part: Image.Image, pivot: tuple[float, float],
                   origin_gp: tuple[float, float], angle: float, flip_x: bool = False) -> None:
    """Paste `part` (authored pointing "up" with `pivot` at its base) so that
    `pivot` lands on `origin_gp` (in game pixels), rotated by `angle` radians
    using the same screen-space convention as skeleton.rs's `rot()`.
    """
    if flip_x:
        part = part.transpose(Image.FLIP_LEFT_RIGHT)
        pivot = (part.width - pivot[0], pivot[1])

    ox, oy = origin_gp[0] * SCALE, origin_gp[1] * SCALE
    px, py = pivot
    c, s = math.cos(angle), math.sin(angle)
    # Inverse mapping (output canvas px -> input part px); see tools/paperdoll.py docstring derivation.
    a, b, cc = c, s, px - ox * c - oy * s
    d, e, ff = -s, c, py + ox * s - oy * c
    out = part.transform(canvas.size, Image.AFFINE, (a, b, cc, d, e, ff),
                          resample=Image.BICUBIC, fillcolor=(0, 0, 0, 0))
    canvas.alpha_composite(out)


def segment_angle(origin: tuple[float, float], end: tuple[float, float]) -> float:
    """Angle `a` such that rot(0, -1, a) points from origin toward end."""
    dx = end[0] - origin[0]
    dy = end[1] - origin[1]
    return math.atan2(dx, -dy)


# ── Parts manifest ────────────────────────────────────────────────────────────

DEFAULT_MANIFEST = {
    "torso":     {"file": "torso.png",     "pivot": [10, 39]},
    "head":      {"file": "head.png",      "pivot": [18, 18]},
    "arm":       {"file": "arm.png",       "pivot": [7, 27]},
    "leg_upper": {"file": "leg_upper.png", "pivot": [7, 18]},
    "leg_lower": {"file": "leg_lower.png", "pivot": [7, 15]},
    "boot":      {"file": "boot.png",      "pivot": [6, 2]},
}

# Existing in-game cosmetic sprites (assets/cosmetics/), reused as-is.
COSMETICS_DIR = os.path.join(os.path.dirname(__file__), "..", "assets", "cosmetics")
HAT_PIVOT = (33, 45)   # head anchor (game CX=11, CY=15 @ 3x) — see SPECS.txt
GUN_PIVOT = (33, 30)   # barrel origin/axis (game x~11, y=10 @ 3x) — see SPECS.txt


def load_manifest(parts_dir: str) -> dict:
    path = os.path.join(parts_dir, "parts.json")
    if os.path.exists(path):
        with open(path) as f:
            return json.load(f)
    return DEFAULT_MANIFEST


def load_part(parts_dir: str, manifest: dict, name: str) -> tuple[Image.Image, tuple[float, float]]:
    spec = manifest[name]
    img = Image.open(os.path.join(parts_dir, spec["file"])).convert("RGBA")
    return img, tuple(spec["pivot"])


# ── Rendering ─────────────────────────────────────────────────────────────────

def render(parts_dir: str, *, pose: str, tick: int, facing: int,
           vel_x: float, vel_y: float, airtime: int, aim_deg: float | None,
           team: int, uniform: int, boots: int, hat: int, gun: int,
           dead: bool = False) -> Image.Image:
    manifest = load_manifest(parts_dir)
    rig = build_rig(pose if not dead else "dead", tick=tick, facing=facing,
                     vel_x=vel_x, vel_y=vel_y, airtime=airtime, aim_deg=aim_deg)

    team_col = TEAM_COLOURS_DEAD[team % 4] if dead else TEAM_COLOURS[team % 4]
    body_col = UNIFORM_COLOURS.get(uniform) or team_col
    boot_col = BOOT_COLOURS.get(boots, BOOT_COLOURS[0])
    body_map = {BODY_PLACEHOLDER: body_col}
    boot_map = {BOOT_PLACEHOLDER: boot_col}

    canvas = Image.new("RGBA", (CANVAS_GP[0] * SCALE, CANVAS_GP[1] * SCALE), (0, 0, 0, 0))
    f_flip = facing < 0

    def put_leg(hip, knee, foot):
        upper, piv_u = load_part(parts_dir, manifest, "leg_upper")
        lower, piv_l = load_part(parts_dir, manifest, "leg_lower")
        boot_img, piv_b = load_part(parts_dir, manifest, "boot")
        upper = recolor(upper, body_map)
        lower = recolor(lower, {**body_map, **boot_map})
        boot_img = recolor(boot_img, boot_map)
        paste_rotated(canvas, upper, piv_u, hip, segment_angle(hip, knee))
        paste_rotated(canvas, lower, piv_l, knee, segment_angle(knee, foot))
        paste_rotated(canvas, boot_img, piv_b, foot, 0.0, flip_x=f_flip)

    def put_arm(origin, end):
        arm, piv = load_part(parts_dir, manifest, "arm")
        arm = recolor(arm, body_map)
        paste_rotated(canvas, arm, piv, origin, segment_angle(origin, end))

    # Z-order matches draw_soldier_skeletal: back leg, back arm, torso, head
    # (+helmet/hat), front leg, front arm (+gun).
    put_leg(rig.hip, rig.back_knee, rig.back_foot)
    put_arm(rig.arm_orig, rig.back_arm)

    torso, piv_t = load_part(parts_dir, manifest, "torso")
    torso = recolor(torso, body_map)
    paste_rotated(canvas, torso, piv_t, rig.hip, segment_angle(rig.hip, rig.shoulder))

    head, piv_h = load_part(parts_dir, manifest, "head")
    paste_rotated(canvas, head, piv_h, (rig.head_cx, rig.head_cy), rig.head_world_angle, flip_x=f_flip)

    if hat > 0:
        hat_img = Image.open(os.path.join(COSMETICS_DIR, f"hat_{hat}.png")).convert("RGBA")
        paste_rotated(canvas, hat_img, HAT_PIVOT, (rig.head_cx, rig.head_cy), rig.head_world_angle, flip_x=f_flip)

    put_leg(rig.hip, rig.front_knee, rig.front_foot)
    put_arm(rig.arm_orig, rig.fwd_arm)

    if gun > 0:
        gun_img = Image.open(os.path.join(COSMETICS_DIR, f"gun_{gun}.png")).convert("RGBA")
        # Guns are authored pointing right (local +x = barrel direction), unlike
        # limb parts which are authored pointing up. rot(1,0,angle) must equal
        # (cos(aim_disp), -sin(aim_disp)); solving gives angle=-aim (facing
        # right) or angle=aim (facing left, after the horizontal flip below).
        aim = math.radians(aim_deg if aim_deg is not None else 0.0)
        gun_angle = aim if facing < 0 else -aim
        paste_rotated(canvas, gun_img, GUN_PIVOT, rig.fwd_arm, gun_angle, flip_x=f_flip)

    return canvas


# ── Templates ────────────────────────────────────────────────────────────────

def make_templates(parts_dir: str) -> None:
    os.makedirs(parts_dir, exist_ok=True)
    sizes = {
        "torso": (21, 39), "head": (36, 36), "arm": (15, 27),
        "leg_upper": (15, 18), "leg_lower": (15, 15), "boot": (12, 9),
    }
    for name, (w, h) in sizes.items():
        spec = DEFAULT_MANIFEST[name]
        img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        px = img.load()
        body = name in ("torso", "arm", "leg_upper")
        for y in range(h):
            for x in range(w):
                if body:
                    px[x, y] = BODY_PLACEHOLDER
                elif name == "leg_lower":
                    px[x, y] = BOOT_PLACEHOLDER if y > h * 0.6 else BODY_PLACEHOLDER
                elif name == "boot":
                    px[x, y] = BOOT_PLACEHOLDER
                elif name == "head":
                    cx, cy, r = w / 2, h / 2, w / 2 - 1
                    if (x - cx) ** 2 + (y - cy) ** 2 <= r * r:
                        px[x, y] = (218, 178, 140, 255)
        # Pivot crosshair in red, drawn last.
        pivx, pivy = spec["pivot"]
        for d in range(-2, 3):
            if 0 <= pivx + d < w:
                px[pivx + d, min(h - 1, max(0, pivy))] = (255, 0, 0, 255)
            if 0 <= pivy + d < h:
                px[min(w - 1, max(0, pivx)), pivy + d] = (255, 0, 0, 255)
        img.save(os.path.join(parts_dir, spec["file"]))
    with open(os.path.join(parts_dir, "parts.json"), "w") as f:
        json.dump(DEFAULT_MANIFEST, f, indent=2)
    print(f"Wrote templates + parts.json to {parts_dir}")


# ── CLI ──────────────────────────────────────────────────────────────────────

def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("parts_dir", nargs="?", help="directory of part PNGs + parts.json")
    ap.add_argument("--make-templates", metavar="DIR", help="write starter part PNGs + parts.json to DIR and exit")
    ap.add_argument("--pose", default="idle", choices=["idle", "walk", "airborne", "spin", "dead"])
    ap.add_argument("--frame", type=int, default=0, help="walk-cycle tick (0-19) for --pose walk")
    ap.add_argument("--facing", type=int, default=1, choices=[-1, 1])
    ap.add_argument("--vel-x", type=float, default=0.0)
    ap.add_argument("--vel-y", type=float, default=0.0)
    ap.add_argument("--airtime", type=int, default=0)
    ap.add_argument("--aim", type=float, default=None, help="aim angle in degrees (world space)")
    ap.add_argument("--team", type=int, default=0, choices=[0, 1, 2, 3])
    ap.add_argument("--uniform", type=int, default=0, help="0=team colour, 1-7=uniform palette")
    ap.add_argument("--boots", type=int, default=0, help="0-5 boot colour palette")
    ap.add_argument("--hat", type=int, default=0, help="0=none, 1-11=hat_<id>.png")
    ap.add_argument("--gun", type=int, default=0, help="0=none, 1-7=gun_<id>.png")
    ap.add_argument("--dead", action="store_true")
    ap.add_argument("--sheet", action="store_true", help="render a walk-cycle sprite sheet instead of a single frame")
    ap.add_argument("-o", "--out", default="/tmp/paperdoll.png")
    args = ap.parse_args()

    if args.make_templates:
        make_templates(args.make_templates)
        return

    if not args.parts_dir:
        ap.error("parts_dir is required unless --make-templates is given")

    common = dict(facing=args.facing, vel_x=args.vel_x, vel_y=args.vel_y,
                   airtime=args.airtime, aim_deg=args.aim, team=args.team,
                   uniform=args.uniform, boots=args.boots, hat=args.hat,
                   gun=args.gun, dead=args.dead)

    if args.sheet:
        frames = [render(args.parts_dir, pose="walk", tick=t, **common) for t in range(0, 20, 2)]
        w, h = frames[0].size
        sheet = Image.new("RGBA", (w * len(frames), h), (0, 0, 0, 0))
        for i, fr in enumerate(frames):
            sheet.alpha_composite(fr, (i * w, 0))
        sheet.save(args.out)
        print(f"Wrote {len(frames)}-frame walk sheet to {args.out}")
    else:
        img = render(args.parts_dir, pose=args.pose, tick=args.frame, **common)
        img.save(args.out)
        print(f"Wrote {args.out}")


if __name__ == "__main__":
    main()
