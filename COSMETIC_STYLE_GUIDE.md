# Arty Cosmetic Dimensions

> For previewing hat/gun cosmetics (and prototyping sprite-based body parts)
> against the in-game soldier rig in every pose, see `tools/paperdoll.py`
> and `tools/paperdoll_parts/README.md`.

## Hats - in-game

In-game rendering blits the actual shop sprite (`cosmetic_sprites::draw_hat`),
scaled to 32 x 29 game px (1.45x the shop sprite's native 22 x 20 gp), centred
on (cx, cy - 7) where (cx, cy) = head center. The 7px upward shift accounts for
the sprite's head-anchor pixel sitting 5px below sprite centre at native scale
(5 x 1.45 ≈ 7).

Propeller Hat (id 2) additionally draws an animated 2x2px blade overlay above
the sprite (`top_y = cy - 7 - 29/2 - 2`), spinning direction/speed tied to wind.

| Bound | Value |
|---|---|
| Render size | 32 x 29 gp |
| Anchor offset from head centre | (0, -7) |

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

In-game rendering blits the actual shop sprite (`cosmetic_sprites::draw_gun_oriented`),
rotated to the aim angle with the barrel drawn at a fixed length of 17 game px
(scale = 17 / 31, since the shop sprite's barrel is ~31 gp from origin to tip).
The sprite's origin (game px 11, 10) maps to the arm-end (t=0, p=0).

| Dimension | Range |
|---|---|
| Barrel length (t) | 0 - 17 |

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
