//! Temporary verification for: scenery cratering, bazooka self-hit fix.

use arty::game::state::GameState;
use arty::game::team::{Team, Difficulty};
use arty::physics::projectile::{Projectile, WeaponKind};
use arty::world::{Crater, Terrain, Vec2, WorldPos, WATER_Y};

fn flat_terrain() -> Terrain {
    let mut t = Terrain::empty();
    for x in 0..arty::world::WORLD_W as i32 {
        for y in 400..WATER_Y as i32 {
            t.set_solid(x, y, true);
        }
    }
    for x in 0..arty::world::WORLD_W as i32 {
        t.recompute_column_cache(x);
    }
    t
}

fn one_v_one(terrain: Terrain) -> GameState {
    let spawns0: Vec<_> = (0..4).map(|i| WorldPos::new(200.0 + i as f32 * 100.0, 400.0)).collect();
    let spawns1: Vec<_> = (0..4).map(|i| WorldPos::new(2600.0 + i as f32 * 100.0, 400.0)).collect();
    let teams = vec![
        Team::new(0, false, Difficulty::Medium, &spawns0),
        Team::new(1, false, Difficulty::Medium, &spawns1),
    ];
    GameState::new(42, terrain, teams, 2)
}

/// Scenery is now baked directly into `terrain.solid` (WA-style — see
/// `Terrain::generate_tactical`), so a test that wants realistic scenery
/// collision must bake it the same way generation does, not just push a bare
/// `SceneryObject` onto an already-solid block of ground.
fn bake_scenery(t: &mut Terrain, obj: arty::world::terrain::SceneryObject) {
    let theme = arty::world::terrain::Theme::of(t.is_cavern, t.template_id);
    let scale = obj.scale(theme);
    for (px, py) in arty::renderer::scenery::scenery_pixels(theme, obj.sprite, scale, obj.x as i32, obj.y as i32) {
        t.set_solid(px, py, true);
    }
    t.scenery.push(obj);
}

#[test]
fn big_crater_only_destroys_overlapping_portion() {
    // Air terrain (not flat ground) so the object's own baked silhouette is
    // the only solid material near it — isolates "does the blast eat the
    // sprite" from "is there unrelated solid ground under the box".
    let mut t = Terrain::empty();
    bake_scenery(&mut t, arty::world::terrain::SceneryObject { x: 500, y: 400, sprite: 1 });
    bake_scenery(&mut t, arty::world::terrain::SceneryObject { x: 900, y: 400, sprite: 1 });
    for x in 0..arty::world::WORLD_W as i32 { t.recompute_column_cache(x); }

    // Bazooka-sized crater overlapping only part of the first object's footprint
    // (its farthest corner is outside r=45) — the far side should survive.
    Crater::new(500.0, 395.0, 45.0).carve(&mut t);
    assert_eq!(t.scenery.len(), 2, "part of the object is untouched by the blast, so it stays");

    // A crater big enough to fully engulf the footprint removes the object.
    Crater::new(500.0, 395.0, 80.0).carve(&mut t);
    assert_eq!(t.scenery.len(), 1, "fully-covered object should be removed");
    assert_eq!(t.scenery[0].x, 900);
}

#[test]
fn bullet_crater_leaves_scenery_standing() {
    let mut t = Terrain::empty();
    bake_scenery(&mut t, arty::world::terrain::SceneryObject { x: 500, y: 400, sprite: 1 });
    for x in 0..arty::world::WORLD_W as i32 { t.recompute_column_cache(x); }
    Crater::new(500.0, 395.0, 3.0).carve(&mut t); // pistol-sized chip
    assert_eq!(t.scenery.len(), 1, "small-arms craters must not fell scenery");
}

#[test]
fn steep_shot_does_not_detonate_on_shooter() {
    let mut game = one_v_one(flat_terrain());
    let shooter = game.teams[0].soldiers[0].pos;
    // Mid-charge, near-vertical shot: spawns inside the shooter's hit box
    // (the exact case that self-detonated before the owner-exclusion fix).
    let spawn = WorldPos::new(shooter.x + 0.5, shooter.y - 16.0);
    let mut proj = Projectile::new(spawn, Vec2::new(0.2, -13.0), WeaponKind::Bazooka);
    proj.owner = Some((0, 0));
    game.projectiles.push(proj);
    let hp_before = game.teams[0].soldiers[0].hp;
    for _ in 0..10 {
        game.step_projectiles();
    }
    assert!(!game.projectiles.is_empty(), "rocket must fly clear, not detonate at the muzzle");
    assert_eq!(game.teams[0].soldiers[0].hp, hp_before, "shooter must take no damage");
}

#[test]
fn returning_shot_still_hits_the_shooter() {
    let mut game = one_v_one(flat_terrain());
    let shooter = game.teams[0].soldiers[0].pos;
    // Projectile that has already left the shooter, now falling back onto them.
    let mut proj = Projectile::new(
        WorldPos::new(shooter.x, shooter.y - 200.0),
        Vec2::new(0.0, 6.0),
        WeaponKind::Bazooka,
    );
    proj.owner = Some((0, 0));
    proj.age_ticks = 30; // long past the spawn ticks
    game.projectiles.push(proj);
    let hp_before = game.teams[0].soldiers[0].hp;
    for _ in 0..80 {
        game.step_projectiles();
        if game.projectiles.is_empty() { break; }
    }
    assert!(game.projectiles.is_empty(), "rocket should have detonated");
    assert!(game.teams[0].soldiers[0].hp < hp_before, "falling back on the shooter must still hurt");
}
