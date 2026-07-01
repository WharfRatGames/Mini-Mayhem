//! Real Worms Armageddon terrain silhouettes, extracted offline from the
//! game's own `assets/Worms Armageddon/DATA/land.dat` (a 1-bit-per-pixel,
//! row-major, MSB-first bitmap embedded in WA's resource-chunk format).
//! Baked here as static byte arrays so `Terrain::generate_tactical` stays
//! pure/seed-derived with no runtime file I/O (see CLAUDE.md).

pub const WA_MASK_W: u32 = 1920;
pub const WA_MASK_H: u32 = 696;

static WA_MASKS: [&[u8]; 2] = [
    include_bytes!("wa_masks/mask0.bin"),
    include_bytes!("wa_masks/mask1.bin"),
];

fn mask_bit(mask: &[u8], x: u32, y: u32) -> bool {
    let row_bytes = WA_MASK_W / 8;
    let byte = mask[(y * row_bytes + x / 8) as usize];
    (byte >> (7 - (x % 8))) & 1 != 0
}

/// Sample a real WA terrain silhouette at normalized `(nx, ny)` in
/// `[0,1)×[0,1)`. `seed` picks one of the two baked masks and applies a
/// deterministic horizontal shift/mirror so distinct seeds produce visually
/// distinct maps from the same underlying real WA art. Returns `1.0` inside
/// solid ground, `0.0` outside.
pub fn wa_density(seed: u64, nx: f64, ny: f64) -> f64 {
    let mask = WA_MASKS[(seed & 1) as usize];
    let shift = ((seed >> 1) % WA_MASK_W as u64) as u32;
    let mirror = (seed >> 20) & 1 == 1;

    let mut mx = ((nx * WA_MASK_W as f64) as u32 + shift) % WA_MASK_W;
    if mirror {
        mx = WA_MASK_W - 1 - mx;
    }
    let my = ((ny * WA_MASK_H as f64) as u32).min(WA_MASK_H - 1);

    if mask_bit(mask, mx, my) { 1.0 } else { 0.0 }
}
