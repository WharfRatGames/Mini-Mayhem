//! region <seed hex> <x0> <x1> <y0> <y1>: ASCII of solid ('#'), deco/scenery ('o'), air ('.') around a spot.
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let seed = u64::from_str_radix(&a[1], 16).unwrap();
    let (x0, x1, y0, y1): (i32, i32, i32, i32) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap(), a[5].parse().unwrap());
    let g = arty::replay::new_game(seed, &[0, 1]);
    for y in y0..y1 { let row: String = (x0..x1).map(|x| if g.terrain.is_solid(x, y) { if g.terrain.deco[(y as usize) * 1920 + x as usize] != 0 { 'o' } else { '#' } } else { '.' }).collect(); println!("{y:4} {row}"); }
}
