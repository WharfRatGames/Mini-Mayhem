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
