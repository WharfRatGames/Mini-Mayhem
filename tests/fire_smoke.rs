//! WA (decompile 0x5501a0): a flame that reaches the water dies and spawns 4 + 4*splits steam puffs that rise and fade.
use arty::game::state::FirePatch;
use arty::renderer::fx::{step_fx, FxKind};
use arty::world::{Vec2, WorldPos, WATER_Y};

#[test]
fn flame_in_water_makes_rising_smoke_and_dies() {
    let mut g = arty::replay::new_game(5, &[0, 1]);
    g.fire_patches.clear();
    g.fx.clear();
    g.fire_patches.push(FirePatch {
        pos: WorldPos::new(60.0, WATER_Y as f32 - 3.0), vel: Vec2::new(0.0, 6.0), landed: false, lifetime: 100, landed_ticks: 0,
        carves: false, splits: 0, dir: 0, timer: 0, cool: 0, wisp: 0, turns: false,
    });
    for _ in 0..10 { g.step_fire_patches(); }
    assert!(g.fire_patches.is_empty(), "the flame is gone");
    let puffs: Vec<_> = g.fx.iter().filter(|p| p.kind == FxKind::Smoke).collect();
    assert_eq!(puffs.len(), 4, "4 + 4*0 puffs");
    let y0: f32 = puffs.iter().map(|p| p.pos.y).sum::<f32>() / 4.0;
    let mut fx = std::mem::take(&mut g.fx);
    for _ in 0..20 { step_fx(&mut fx, &g.terrain, 0.0); }
    let y1: f32 = fx.iter().filter(|p| p.kind == FxKind::Smoke).map(|p| p.pos.y).sum::<f32>() / 4.0;
    assert!(y1 < y0 - 3.0, "smoke rises: {y0} -> {y1}");
    for _ in 0..90 { step_fx(&mut fx, &g.terrain, 0.0); }
    assert!(fx.iter().all(|p| p.kind != FxKind::Smoke), "puffs live 33..75 ticks, then vanish");
}

#[test]
fn burning_flame_sheds_occasional_wisps_more_when_small() {
    // WA: ~0.02 puffs/frame on a fresh flame, up to ~0.06 as it shrinks.
    let count = |lifetime: u32| {
        let mut g = arty::replay::new_game(5, &[0, 1]);
        g.fire_patches.clear();
        g.fx.clear();
        // flame resting on the floor of the world far from any worm: just stand it above the water on a solid pixel is not needed,
        // wisps are drawn whether or not it moves
        g.fire_patches.push(FirePatch {
            pos: WorldPos::new(60.0, 100.0), vel: Vec2::new(0.0, 0.0), landed: false, lifetime, landed_ticks: 0,
            carves: false, splits: 0, dir: 0, timer: 0, cool: 0, wisp: 0, turns: false,
        });
        let mut n = 0;
        for _ in 0..1500 {
            g.fire_patches[0].lifetime = lifetime; // hold the size constant
            g.fire_patches[0].pos = WorldPos::new(60.0, 100.0); g.fire_patches[0].vel = Vec2::new(0.0, 0.0);
            g.tick += 1;
            g.step_fire_patches();
            n += g.fx.iter().filter(|p| p.kind == FxKind::Smoke).count();
            g.fx.clear();
        }
        n
    };
    let fresh = count(150);
    let small = count(20);
    assert!(fresh > 0 && small > fresh, "fresh {fresh}, small {small}");
}
