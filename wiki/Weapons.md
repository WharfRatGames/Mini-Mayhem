# Weapons

Weapons fall into two categories: **loadout weapons** (every team starts with them) and **crate-only weapons** (obtained from weapon crates dropped during the match).

---

## Loadout Weapons

These are available to every team from the start of every match.

### Bazooka ∞
The bread-and-butter direct-fire rocket. Medium blast radius, travels in an arc affected by wind. Explodes on terrain contact.

### Grenade ∞
Bounces off terrain before exploding on a timed fuse (1–5 seconds, adjustable with L1/R1 before throwing). Not affected by wind.

### Shotgun ∞
Fires 5 pellets in a tight spread. High damage at close range, minimal knockback per pellet. Hitscan — no projectile travel time.

### MAC-10 × 2
Full-auto burst — fires a rapid stream of bullets in a tight arc. Medium damage per hit, very fast fire rate. Effective at close-to-medium range.

### Ninja Rope × 5
Fires a grappling hook that sticks to terrain. Swing across gaps and up to high ground on a real pendulum — pump the swing from the sides of the arc to build height, and reel the rope in and out to control your radius. The rope bends around corners and unwinds as you swing back, so it wraps naturally through caves and around pillars. Momentum carries through when you let go, and you can rebuild it by re-hooking in mid-air; corner-wraps slingshot you around. Swinging into an enemy knocks them aside without ending your turn. Does no damage itself. Five ropes per loadout.

### TNT × 1
Place it and run — the fuse burns for a random 4–5 seconds. Massive blast radius and damage. Unlocked after 5 full turn rotations.

### Landmine × 2
Dropped at the soldier's feet. Arms after a short delay, then detonates when any soldier walks over it.

### Baseball Bat × 1
Melee swing launches the target soldier with massive knockback. No blast damage — pure physics chaos. Unlocked after 3 full turn rotations.

### Plasma Torch × 3
Burns a tunnel through terrain in the aimed direction. Does not damage soldiers directly. Useful for repositioning or creating kill pits. The turn timer pauses while the torch is running, so you won't lose your turn mid-tunnel.

### Jumpbot × 1
Placed at the soldier's feet like TNT — no aim or charge. It then walks off on its own, hopping forward in little leaps, vaulting obstacles it can't step over and reversing when it hits a wall. Its fuse runs on a fixed real-time countdown regardless of whose turn it is; it explodes when the fuse expires, when it touches water, or when the placing team presses the detonate button during their turn. Big blast (75 damage, 45px radius). It moves with a distinctive hopping gait.

### Molotov Cocktail ∞
Throws a bottle that shatters on impact, spraying a wide pool of flames — but leaves **no crater** (the fire does the work, not the blast). Flames flicker and burn out over a long time, sliding downhill and pooling in pits, wind-affected in flight. A soldier caught in the fire takes steady damage over time and reacts by hopping and moving to escape — heading one way out of the flames, jumping the other way if it hits a wall. It's deadly if the soldier is cornered against a barrier with nowhere to go, but out in the open it can usually get clear.

---

## Crate-Only Weapons

These are obtained exclusively from weapon crates. See [Crate Drops](Crate-Drops) for drop rates.

### Revolver *(Rare)*
Precise hitscan pistol. Low damage per shot but pinpoint accurate with no travel time. Multiple shots per turn.

### Blasthive *(Uncommon)*
Throw a beehive that bursts on impact and releases 6 homing bees. Each bee seeks the nearest living soldier and stings for small damage. Combined hits can be devastating.

### Meteor Bomb *(Uncommon)*
A large falling bomb that splits into 5 burning fragments on impact, scattering in a fountain pattern. Silent on throw — no warning until it hits.

### Sacred Ordnance *(Uncommon)*
A Holy Hand Grenade with a fixed 3-second fuse. After the fuse expires it **does not immediately explode** — it rolls to a complete stop first. Once still, a Hallelujah fanfare plays, then it detonates with a TNT-scale blast. Massive radius and knockback. Crate only.

### Garcia *(Uncommon)*
Calls in a targeted artillery strike. Aim the cursor at a location; Garcia drops from the sky and bounces to a stop before detonating. Bounce physics mean the final landing spot isn't exact — use terrain to your advantage.

### Homing Missile *(Rare)*
Lock a cursor onto a target position, then fire a missile that steers toward it. Up/Down moves the cursor before confirming with A. After launch the missile homes in and detonates on contact.

### Air Strike *(Uncommon)*
Calls a plane that drops a spread of bombs across a wide area. Move the cursor with Left/Right; confirm with A and the plane flies across, releasing bombs automatically. Unlocked after 7 full turn rotations.

### Black Hole Bomb *(Rare)*
Spawns a gravitational well that sucks nearby soldiers and projectiles toward its center, then collapses in a burst. Terrain is not affected during the pull phase.

### Hand of Jerry *(Ultra Rare)*
Drops a giant fist from the sky that crushes the targeted location. Enormous damage and crater. The rarest weapon in the crate pool.

---

## Weapon Unlock Timers

Some powerful loadout weapons are locked for the opening turns to prevent first-turn instakills.

| Weapon | Unlocks after |
|---|---|
| Baseball Bat | 3 full turn rotations |
| TNT | 5 full turn rotations |
| Air Strike | 7 full turn rotations |

---

## Damage Reference

Max damage is dealt at the centre of the blast and falls off linearly to zero at the edge of the
radius. Values below are tuned for Mini Mayhem's scale and balance.

| Weapon (in-game name) | Max damage | Blast radius | Notes |
|---|---|---|---|
| Bazooka | 50 | 45 px | arc, wind-affected |
| Grenade | 45 | 30 px | timed fuse, bounces |
| Clump Bomb | 30 / cluster | 20 px | splits into clusters |
| Meteor Bomb | 18 / fragment | 14 px | fragment spray (can't one-shot) |
| TNT | 75 | 75 px | ~4–5s fuse |
| Mine | 50 | 55 px | proximity trigger |
| Sacred Ordnance | 80 | 80 px | rolls to a stop, then detonates |
| Hand of Jerry | 45 | 55 px | bounces before detonating |
| Homing Missile | 45 | 30 px | steers to cursor |
| Air Strike | 50 / bomb | 45 px | spread of bombs |
| Black Hole Bomb | 35 | — | gravity well, no crater |
| Jumpbot | 75 | 45 px | walks off, hop gait |
| Molotov Cocktail | 20 impact + fire DoT | 20 px | no crater; fire does the work |
| Shotgun | 20 / pellet (×5) | 12 px | hitscan |
| Minigun | 12 / bullet | 8 px | full-auto |
| MAC-10 | 5 / bullet | 6 px | full-auto burst |
| Revolver | 15 / shot | — | hitscan, pinpoint |
| Pistol | 5 / bullet | — | hitscan |
| Blasthive | 5 / bee sting (×6) | 15 px | homing bees |
| Baseball Bat | 25 + big knockback | — | melee, no blast |
| Plasma Torch | 0 (digs terrain) | — | pauses turn timer |
| Ninja Rope | 0 | — | utility |
