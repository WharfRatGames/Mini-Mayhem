/// Terrain texture atlases. Each atlas is a grid of labelled texture swatches on
/// a near-black background. The swatches are NOT on a clean fixed grid (margins
/// and sizes vary), and the labels are text we must NOT bake into the tiles — so
/// we detect the swatch column/row bands by background projection (which excludes
/// the sparse label text and the dark gaps) and slice each tile from a band
/// intersection. Tiles from both atlases are pooled into one indexed list.
use std::sync::OnceLock;

const ATLAS1: &[u8] = include_bytes!("../../assets/textures/TEXTURES.png");   // 6×4 = 24
const ATLAS3: &[u8] = include_bytes!("../../assets/textures/textures3.png");  // 4×2 = 8
const ATLAS4: &[u8] = include_bytes!("../../assets/textures/textures4.png");  // 7×5 = 35
const ATLAS5: &[u8] = include_bytes!("../../assets/textures5.png");           // 8×5 = 40

/// One texture swatch. Pixels are RGBA8, row-major, `w`×`h`.
pub struct Tile {
    pub w:  usize,
    pub h:  usize,
    pub px: Vec<[u8; 4]>,
}

impl Tile {
    #[inline]
    pub fn sample(&self, x: usize, y: usize) -> [u8; 4] {
        self.px[y * self.w + x]
    }
}

/// A pixel counts as atlas background (the near-black gaps / label backdrop).
#[inline]
fn is_bg(p: [u8; 4]) -> bool {
    p[3] < 10 || (p[0] < 40 && p[1] < 40 && p[2] < 40)
}

static TILES: OnceLock<Vec<Tile>> = OnceLock::new();

/// Get a tile by selector `id`. The selector is mapped modulo the pooled tile
/// count, so any seed-derived `u8` resolves to a valid tile. Returns None only
/// if both atlases failed to decode.
pub fn tile(id: u8) -> Option<&'static Tile> {
    let t = tiles();
    if t.is_empty() { None } else { Some(&t[id as usize % t.len()]) }
}

fn tiles() -> &'static Vec<Tile> {
    TILES.get_or_init(|| {
        let mut v = Vec::new();
        // ATLAS1 captions sit in the dark gaps above each swatch (excluded by band
        // detection), so its tiles are already text-free → band-detect it.
        if let Some(mut a) = decode_one(ATLAS1, false) { v.append(&mut a); }
        // ATLAS3 separates its swatches with dark *coloured* frames (not near-black
        // gaps), so background-projection band detection collapses it into a few
        // giant multi-swatch slabs. It's a regular 4×2 grid — slice it on that grid
        // instead, trimming the frame from each cell.
        if let Some(mut c) = decode_grid(ATLAS3, 4, 2, 30) { v.append(&mut c); }
        // ATLAS4 is a 7×5 grid of colourful tiles (lava, crystals, slime, etc.)
        // separated by ~13px black borders baked into the image.
        if let Some(mut d) = decode_grid(ATLAS4, 7, 5, 13) { v.append(&mut d); }
        // ATLAS5 is an 8×5 grid with ~4px near-black separators between tiles.
        if let Some(mut e) = decode_grid(ATLAS5, 8, 5, 4) { v.append(&mut e); }
        v
    })
}

/// Decode a PNG atlas to row-major RGBA8, returning `(pixels, width, height)`.
fn read_rgba(bytes: &[u8]) -> Option<(Vec<[u8; 4]>, usize, usize)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let info = reader.info();
    let (aw, ah) = (info.width as usize, info.height as usize);
    let color = info.color_type;
    let mut raw = vec![0u8; reader.output_buffer_size()];
    reader.next_frame(&mut raw).ok()?;

    // Normalise to RGBA8 (these atlases are RGB → opaque alpha).
    let rgba: Vec<[u8; 4]> = match color {
        png::ColorType::Rgba => raw.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect(),
        png::ColorType::Rgb  => raw.chunks_exact(3).map(|c| [c[0], c[1], c[2], 255]).collect(),
        _ => return None,
    };
    if rgba.len() < aw * ah { return None; }
    Some((rgba, aw, ah))
}

/// Slice an atlas that is a regular cols×rows grid of swatches separated by
/// non-black frames (so background-projection band detection fails). Cells are
/// evenly sized; `inset` px are trimmed from each cell to clear the frame.
fn decode_grid(bytes: &[u8], cols: usize, rows: usize, inset: usize) -> Option<Vec<Tile>> {
    let (rgba, aw, ah) = read_rgba(bytes)?;
    let (cw, ch) = (aw / cols, ah / rows);
    if cw == 0 || ch == 0 { return None; }
    let mut out = Vec::with_capacity(cols * rows);
    for r in 0..rows {
        for c in 0..cols {
            out.push(extract(&rgba, aw, c * cw, r * ch, cw, ch, inset, false));
        }
    }
    Some(out)
}

/// Decode one atlas and slice it into tiles via background-projection band
/// detection (auto-detects the grid dimensions; works for any rows×cols layout).
fn decode_one(bytes: &[u8], strip_labels: bool) -> Option<Vec<Tile>> {
    let (rgba, aw, ah) = read_rgba(bytes)?;

    const MIN_BAND: usize = 40; // ignore thin runs (label text / noise)

    // Column bands: x-runs whose background fraction (over all rows) is < 0.5.
    let col_bg: Vec<f32> = (0..aw)
        .map(|x| (0..ah).filter(|&y| is_bg(rgba[y * aw + x])).count() as f32 / ah as f32)
        .collect();
    let cols = low_runs(&col_bg, 0.5, MIN_BAND);

    // Row bands: y-runs whose background fraction (over all cols) is < 0.5. The
    // label text sits in the dark gaps above each swatch → excluded here.
    let row_bg: Vec<f32> = (0..ah)
        .map(|y| (0..aw).filter(|&x| is_bg(rgba[y * aw + x])).count() as f32 / aw as f32)
        .collect();
    let rows = low_runs(&row_bg, 0.5, MIN_BAND);

    if cols.is_empty() || rows.is_empty() { return None; }

    let mut out = Vec::with_capacity(cols.len() * rows.len());
    for &(ry0, ry1) in &rows {
        for &(cx0, cx1) in &cols {
            out.push(extract(&rgba, aw, cx0, ry0, cx1 - cx0 + 1, ry1 - ry0 + 1, 3, strip_labels));
        }
    }
    Some(out)
}

/// Find contiguous runs (inclusive start,end) where `frac[i] < thr`, keeping only
/// runs at least `min_len` long.
fn low_runs(frac: &[f32], thr: f32, min_len: usize) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (i, &v) in frac.iter().enumerate() {
        if v < thr {
            if start.is_none() { start = Some(i); }
        } else if let Some(s) = start.take() {
            if i - s >= min_len { runs.push((s, i - 1)); }
        }
    }
    if let Some(s) = start {
        if frac.len() - s >= min_len { runs.push((s, frac.len() - 1)); }
    }
    runs
}

fn extract(rgba: &[[u8; 4]], aw: usize, x0: usize, y0: usize, w: usize, h: usize, inset: usize, strip_labels: bool) -> Tile {
    // Inset a few px so the tile's edge columns/rows are clean swatch content. The
    // cell rectangle includes a thin border between swatches; those edge pixels are
    // sampled twice at the mirror-tiling folds in atlas_sample, showing as strips in
    // terrain. Dropping the border removes them; the texture body tiles seamlessly.
    let (sx, sy) = (x0 + inset.min(w / 4), y0 + inset.min(h / 4));
    let w = w.saturating_sub(2 * inset).max(1);
    let h = h.saturating_sub(2 * inset).max(1);
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            px.push(rgba[(sy + y) * aw + (sx + x)]);
        }
    }
    // Bands can include a thin margin of the black gap (or rounded swatch corners),
    // which would render as black specks in terrain. Replace every background pixel
    // with its nearest non-background neighbour so the tile is pure swatch colour.
    scrub_bg(&mut px, w, h);
    // ATLAS2 prints a caption across the top of each swatch — erase it.
    if strip_labels { strip_top_label(&mut px, w, h); }
    Tile { w, h, px }
}

/// Erase the caption printed across the top of a swatch by mirror-reflecting
/// clean texture over the label rows. Labels are light text on a dark background
/// so they can't be detected by brightness alone — strip a fixed 32px which
/// comfortably covers the tallest observed label (~24px) with margin.
/// Only applied when the tile is tall enough that 32px < 40% of the height.
fn strip_top_label(px: &mut [[u8; 4]], w: usize, h: usize) {
    const LABEL_PX: usize = 32;
    if h < LABEL_PX * 2 + 4 { return; } // tile too small — skip
    let lh = LABEL_PX;
    for y in 0..lh {
        let src = 2 * lh - 1 - y; // mirror row from just below the label band
        for x in 0..w {
            px[y * w + x] = px[src * w + x];
        }
    }
}

/// Remove all background (near-black) pixels from a tile by flood-filling each one
/// from the nearest non-background pixel: a left→right then right→left pass per row
/// fills from horizontal neighbours, then a top→bottom / bottom→top pass per column
/// catches any fully-background rows. No-op if the tile is entirely background.
fn scrub_bg(px: &mut [[u8; 4]], w: usize, h: usize) {
    for y in 0..h {
        let mut last: Option<[u8; 4]> = None;
        for x in 0..w {
            let i = y * w + x;
            if is_bg(px[i]) { if let Some(c) = last { px[i] = c; } } else { last = Some(px[i]); }
        }
        let mut last: Option<[u8; 4]> = None;
        for x in (0..w).rev() {
            let i = y * w + x;
            if is_bg(px[i]) { if let Some(c) = last { px[i] = c; } } else { last = Some(px[i]); }
        }
    }
    for x in 0..w {
        let mut last: Option<[u8; 4]> = None;
        for y in 0..h {
            let i = y * w + x;
            if is_bg(px[i]) { if let Some(c) = last { px[i] = c; } } else { last = Some(px[i]); }
        }
        let mut last: Option<[u8; 4]> = None;
        for y in (0..h).rev() {
            let i = y * w + x;
            if is_bg(px[i]) { if let Some(c) = last { px[i] = c; } } else { last = Some(px[i]); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pool must be 24 (ATLAS1) + 8 (ATLAS3) + 35 (ATLAS4) clean swatches.
    #[test]
    fn pool_has_all_swatches() {
        assert_eq!(tiles().len(), 107, "expected 24 + 8 + 35 + 40 pooled tiles");
    }

    /// Regression: ATLAS3's coloured frames once defeated band detection and
    /// produced ~1150×1018 multi-swatch mega-tiles that rendered as garbled
    /// terrain. Every real swatch is comfortably smaller than this.
    #[test]
    fn no_garbled_mega_tiles() {
        for (i, t) in tiles().iter().enumerate() {
            assert!(
                t.w > 0 && t.h >= 4 && t.w < 400 && t.h < 600,
                "tile {i} has implausible swatch size {}x{}", t.w, t.h
            );
        }
    }
}
