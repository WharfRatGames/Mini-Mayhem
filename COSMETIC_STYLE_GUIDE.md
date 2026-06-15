# Arty Cosmetic Dimensions

> For previewing hat/gun cosmetics (and prototyping sprite-based body parts)
> against the in-game soldier rig in every pose, see `tools/paperdoll.py`
> and `tools/paperdoll_parts/README.md`.

## Hats - in-game

Anchor: (cx, cy) = head center.
Helmet cap occupies dy = -5 to 0, so hat pixels start at cy - 6.

| Bound | Value |
|---|---|
| Tallest point | cy - 12 |
| First safe row | cy - 6 |
| Max width | cx - 5 to cx + 5 |

## Hats - shop sprite

| Property | Value |
|---|---|
| Image size | 66 x 60 px |
| Game pixels (3x scale) | 22 x 20 gp |
| Head anchor | pixel (33, 45) |
| Hat pixels above | image y = 45 |
| Max height | image y = 6 |
| Max width | image x 18 - 48 |
| Format | PNG RGBA8, transparent background |

## Guns - in-game

Coordinate t = forward distance from arm end. p = perpendicular offset from barrel axis.

| Dimension | Range |
|---|---|
| Barrel length (t) | 6 - 14 |
| Barrel half-width (p) | 0 - 3 |

## Guns - shop sprite

| Property | Value |
|---|---|
| Image size | 138 x 78 px |
| Game pixels (3x scale) | 46 x 26 gp |
| Barrel axis | image y = 30 |
| Barrel origin | image x ~33 |
| Barrel tip padding | ~4 gp from right edge |
| Format | PNG RGBA8, transparent background |

## Uniforms

Single color applied to torso, arms, and legs. Helmet cap always stays team color.

## Boots

Single color applied to a 4 x 3 px rect at each foot.
