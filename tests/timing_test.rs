use std::time::Instant;
#[test]
#[ignore]
fn time_map_load() {
    // Simulate full match load: generate + spawns + world cache + bg cache
    let seed = 42u64;
    let t0 = Instant::now();
    let mut terrain = arty::world::Terrain::generate_tactical(seed);
    println!("generate_tactical: {}ms", t0.elapsed().as_millis());
    let t1 = Instant::now();
    let _spawns = terrain.find_team_spawns(0, arty::world::WORLD_W, 8);
    println!("find_team_spawns: {}ms", t1.elapsed().as_millis());
    let t2 = Instant::now();
    let mut cache = arty::renderer::WorldBuffer::new();
    arty::renderer::draw_terrain::build_world_cache(&mut cache, &terrain);
    println!("build_world_cache: {}ms", t2.elapsed().as_millis());
    let t3 = Instant::now();
    let _bg = arty::renderer::bg_image::build_bg_cache(seed);
    println!("build_bg_cache: {}ms", t3.elapsed().as_millis());
}
