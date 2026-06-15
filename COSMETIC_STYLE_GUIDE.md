# Arty Cosmetic Dimensions

> For previewing hat/gun cosmetics (and prototyping sprite-based body parts)
> against the in-game soldier rig in every pose, see `tools/paperdoll.py`
> and `tools/paperdoll_parts/README.md`.

## Hats - in-game

In-game rendering blits the actual shop sprite (`cosmetic_sprites::draw_hat`),
scaled to 40 x 36 game px, centred on (cx, cy - 9) where (cx, cy) = head center.

Propeller Hat (id 2): the sprite's own static propeller bar (source rows 18-26
of the 66x60 sprite) is skipped during the blit (`blit_scaled_skip_rows`), and a
single rotating bar (half-length 6px, thickness 3px, colour sampled from the
sprite's propeller) is drawn at the hub in its place, spinning direction/speed
tied to wind.

| Bound | Value |
|---|---|
| Render size | 40 x 36 gp |
| Anchor offset from head centre | (0, -9) |

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
