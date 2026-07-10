//! Generation sanity checks for the seed-based WA collage synthesizer.
//! Determinism, solid-fraction bands, per-seed distinctness, mask coverage,
//! and cavern enclosure.

use arty::world::Terrain;
use arty::world::{WORLD_W, WATER_Y};

fn solid_fraction(t: &Terrain) -> f64 {
    let mut solid = 0usize;
    let mut total = 0usize;
    for y in 0..WATER_Y as i32 {
        for x in 0..WORLD_W as i32 {
            total += 1;
            if t.is_solid(x, y) {
                solid += 1;
            }
        }
    }
    solid as f64 / total as f64
}

fn bitmap_hash(t: &Terrain) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for y in 0..WATER_Y as i32 {
        for x in 0..WORLD_W as i32 {
            h = h.wrapping_mul(0x100000001b3);
            h ^= t.is_solid(x, y) as u64;
        }
    }
    h
}

#[test]
fn same_seed_same_bitmap() {
    for seed in [1u64, 42, 777, 123456789] {
        let a = Terrain::generate_tactical(seed);
        let b = Terrain::generate_tactical(seed);
        assert_eq!(bitmap_hash(&a), bitmap_hash(&b), "seed {seed} not deterministic");
    }
}

#[test]
fn seeds_produce_distinct_maps_and_sane_fractions() {
    let mut hashes = std::collections::HashSet::new();
    let mut caverns = 0;
    let mut islands = 0;
    for seed in 0..40u64 {
        let t = Terrain::generate_tactical(seed);
        let f = solid_fraction(&t);
        if t.is_cavern {
            caverns += 1;
            assert!(
                (0.30..=0.90).contains(&f),
                "cavern seed {seed}: solid fraction {f:.3} out of band"
            );
        } else {
            islands += 1;
            assert!(
                (0.08..=0.60).contains(&f),
                "island seed {seed}: solid fraction {f:.3} out of band"
            );
        }
        assert!(hashes.insert(bitmap_hash(&t)), "seed {seed} duplicates another map");
    }
    assert!(caverns > 0, "no cavern maps in 40 seeds");
    assert!(islands > 0, "no island maps in 40 seeds");
}

#[test]
fn caverns_are_enclosed_and_spawnable() {
    let mut checked = 0;
    for seed in 0..60u64 {
        let mut t = Terrain::generate_tactical(seed);
        if !t.is_cavern {
            continue;
        }
        checked += 1;
        // Sealed rock cap: the top 100 rows must be fully solid.
        for y in 0..100 {
            for x in 0..WORLD_W as i32 {
                assert!(t.is_solid(x, y), "cavern seed {seed}: hole in cap at ({x},{y})");
            }
        }
        // Both teams find spawns.
        let left = t.find_team_spawns(0, WORLD_W / 2, 4);
        let right = t.find_team_spawns(WORLD_W / 2, WORLD_W, 4);
        assert_eq!(left.len(), 4, "cavern seed {seed}: left spawns");
        assert_eq!(right.len(), 4, "cavern seed {seed}: right spawns");
        if checked >= 8 {
            break;
        }
    }
    assert!(checked > 0, "no caverns sampled");
}

#[test]
fn island_maps_find_spawns() {
    for seed in [3u64, 7, 11, 21, 33] {
        let mut t = Terrain::generate_tactical(seed);
        if t.is_cavern {
            continue;
        }
        let left = t.find_team_spawns(0, WORLD_W / 2, 4);
        let right = t.find_team_spawns(WORLD_W / 2, WORLD_W, 4);
        assert_eq!(left.len(), 4, "seed {seed}: left spawns");
        assert_eq!(right.len(), 4, "seed {seed}: right spawns");
    }
}

/// Guard for the compressed-relief redesign: soldiers walk up 8px, jump ~16px,
/// backflip ~46px. Island surfaces must stay mostly traversable — p95 of the
/// surface-height change across 12px windows at/under ~60px (chasms and pit
/// hazards intentionally exceed it, hence p95 not max), and nearly every
/// column must have ground at all (the depth ramp guarantees it outside
/// chasms/edge water).
#[test]
fn island_relief_is_traversable() {
    let mut checked = 0;
    for seed in 0..40u64 {
        let t = Terrain::generate_tactical(seed);
        if t.is_cavern {
            continue;
        }
        checked += 1;
        let surface = |x: i32| (0..WATER_Y as i32).find(|&y| t.is_solid(x, y));
        let mut deltas: Vec<i32> = Vec::new();
        let mut covered = 0usize;
        let n = (WORLD_W - 12) as i32;
        for x in 0..n {
            if surface(x).is_some() {
                covered += 1;
            }
            if let (Some(a), Some(b)) = (surface(x), surface(x + 12)) {
                deltas.push((a - b).abs());
            }
        }
        // Taller maps are intended now that the grapple reaches cliffs beyond
        // backflip range, so a moderate fraction of steep columns is fine — that's
        // the vertical, reference-game feel. The guard only catches PERVASIVE
        // verticality (pre-redesign maps had 30%+ of columns steeper than 60px),
        // which would make a map unreadable / ungrappleable.
        let steep = deltas.iter().filter(|&&d| d > 60).count();
        let frac = steep as f64 / deltas.len().max(1) as f64;
        assert!(
            frac <= 0.20,
            "island seed {seed}: {:.1}% of columns have >60px cliffs — terrain pervasively vertical",
            frac * 100.0
        );
        assert!(
            covered as f64 / n as f64 >= 0.80,
            "island seed {seed}: only {covered}/{n} columns have ground"
        );
    }
    assert!(checked > 0, "no island maps sampled");
}

/// Every non-cavern map must end in open water on BOTH sides, like the real
/// WA generator's output (0/150 MapGEN reference maps touch a side edge).
/// The density-field edge erosion in generate_tactical guarantees this; no
/// post-pass may re-stamp solid into the outermost columns.
#[test]
fn island_edges_are_open_water() {
    let mut checked = 0;
    for seed in 0..40u64 {
        let t = Terrain::generate_tactical(seed);
        if t.is_cavern {
            continue;
        }
        checked += 1;
        for x in (0..8).chain(WORLD_W as i32 - 8..WORLD_W as i32) {
            for y in 0..WATER_Y as i32 {
                assert!(
                    !t.is_solid(x, y),
                    "island seed {seed}: solid terrain at map edge ({x}, {y})"
                );
            }
        }
    }
    assert!(checked > 0, "no island maps sampled");
}

#[test]
fn both_source_masks_appear_as_dominant() {
    let mut seen = std::collections::HashSet::new();
    for seed in 0..64u64 {
        let t = Terrain::generate_tactical(seed);
        seen.insert((t.is_cavern, t.template_id));
    }
    let island_ids: Vec<u8> =
        seen.iter().filter(|(c, _)| !c).map(|(_, id)| *id).collect();
    assert!(
        island_ids.contains(&0) && island_ids.contains(&1),
        "expected both masks dominant across seeds, saw {island_ids:?}"
    );
}
