//! Strong isolated hot sites, mapped on every supported Bayer develop before
//! anything else reads the mosaic. A hot site is a defect of the sensor, not
//! noise, so it does not wait for AI denoise to be asked for: until 2026-09-21
//! it did (the rule arrived with the denoise lane and stayed inside it), which
//! left the same photograph with its hot pixels in an ordinary develop and
//! without them in a denoised one. The user ruled that ordinary develops map too.
//! All decisions read the unchanged mosaic. A three-pixel border is left alone:
//! a neighbour's own same-colour median needs two more sensor pixels.
//!
//! The rule is a test against a null model: flat ground, unclipped, lit like
//! the rest of its tile. It had only ever been run on night sky, where that
//! model holds. Run on six ordinary city and dusk frames (2026-09-21) it mapped
//! 1 / 3866 / 1213 / 1246 / 4003 / 308 sites inside the picture, none at a
//! photosite any other frame shared: out-of-focus lights, sunlit walls, blown
//! signs, a red LED board. On three night frames 66 of the sites it mapped sat
//! inside stars. Each piece of evidence is therefore asked whether it CAN
//! testify where it stands, every doubt is read for the scene, and the same six
//! frames now map 0 / 0 / 0 / 6 / 0 / 4 (what is left is named in ARCHITECTURE):
//!
//!   * "20 sigma" is the noise WHERE THE SITE STANDS. The tile's residual MAD
//!     is the noise of the tile's majority. A bright structure in a mostly dark
//!     tile carries many times that in photon noise, and texture in one colour
//!     plane makes every bright sample of it an outlier of the same-colour
//!     median. One measurement answers both: the spread of the same-colour
//!     samples within four pixels of the site joins its scale at a half. Left
//!     out, four of the ordinary frames map 10 / 43 / 46 / 32 sites more; joined
//!     whole, the three night frames lose 23 / 7 / 24 of theirs. Flat Gaussian
//!     ground exceeds its tile's sigma at a half once in 133,000 sites
//!     (simulated), so flat ground is judged by its tile as it always was.
//!   * Clipped samples measure no noise. A tile that is part blown had its MAD
//!     collapse, and every ripple of its unblown part became a 20-sigma event.
//!     So the tile's statistics and the site's neighbourhood are taken over
//!     samples that can move, and a site whose background is clipped, or most
//!     of whose neighbourhood is, is not judged. (On the ten frames each of the
//!     three covers for the others: two sites on blown ground return only when
//!     the last two are both gone.)
//!   * A witness at white cannot show an excess, so it cannot vouch that the
//!     site stands alone; and the four DIAGONAL neighbours are asked as well as
//!     the four edge ones — for a green site they are the same colour, so no
//!     colour of light can hide from them. Asked of the edge four only, the
//!     night frames map three sites more.
//!
//! What stood here first, and why it went. A line through the frame's tiles
//! (variance against level, per CFA phase) carried the tile's sigma to the
//! site's brightness, beside the spread of the ring of eight at a third. Taken
//! out of that rule, the line changed three sites on the ten frames — and two
//! of them were lone photosites on a night sky that this rule maps. It had also
//! needed the median's own noise taken out of every background first, or it
//! declined 24 of the sites on the flat sky of two high-ISO night frames. The
//! neighbourhood measures at the site what the line inferred from the frame.

use anyhow::{bail, Result};
use rayon::prelude::*;
use rawler::rawimage::RawImageData;

const PLANE_TILE: usize = 64;
const HOT_SIGMA: f32 = 20.0;
const NEIGHBOUR_SIGMA: f32 = 3.0;
/// How much of the spread of a site's own neighbourhood joins its scale. At a
/// half, flat Gaussian ground exceeds its tile's sigma once in 133,000 sites
/// (simulated, 4e7 neighbourhoods) and then by a hair, so flat ground is judged
/// by its tile as it always was; a bright or textured neighbourhood is judged
/// by ten of its own sigmas.
const LOCAL_SHARE: f32 = 0.5;
/// A sample this far up from black toward white, or whose background is, has
/// part of its noise cut off by the clip and joins no tile's statistics.
const CLIP_SHARE: f32 = 0.9;

/// Map the frame's hot sites in place and say how many there were. The gate is
/// the one the AI path uses — a 2×2 Bayer mosaic of integer samples with its
/// levels declared — so the two paths can never disagree about which frames
/// are mapped; any other sensor is returned untouched.
pub(crate) fn map_for(raw: &mut rawler::RawImage) -> Result<usize> {
    let Ok(levels) = super::mosaic_args_for(raw) else { return Ok(0) };
    let RawImageData::Integer(data) = &mut raw.data else { return Ok(0) };
    if raw.width.checked_mul(raw.height) != Some(data.len()) {
        bail!("hot-pixel mapping: mosaic dimensions do not match its samples");
    }
    let corrections = find(data, raw.width, raw.height, &levels);
    for &(i, value) in &corrections {
        data[i] = value;
    }
    Ok(corrections.len())
}

fn median(values: &mut [f32]) -> f32 {
    let n = values.len();
    let (lo, mid, _) = values.select_nth_unstable_by(n / 2, f32::total_cmp);
    if n.is_multiple_of(2) {
        (lo.iter().copied().fold(f32::NEG_INFINITY, f32::max) + *mid) * 0.5
    } else {
        *mid
    }
}

/// The eight same-colour neighbours of a sample, two pixels away.
#[inline(always)]
fn ring8(data: &[u16], w: usize, x: usize, y: usize) -> [u16; 8] {
    let (above, row, below) = ((y - 2) * w + x, y * w + x, (y + 2) * w + x);
    [
        data[above - 2], data[above], data[above + 2],
        data[row - 2], data[row + 2],
        data[below - 2], data[below], data[below + 2],
    ]
}

/// The median of the eight same-colour neighbours: half the sum of the 4th and
/// 5th of the sorted eight, exact in f32 (the sum stays below 2^17).
///
/// A fixed network on the integer samples rather than a general selection on
/// floats, because this runs once per sensor sample on EVERY develop since
/// 2026-09-21. The scan as first written — that selection, plus a record kept
/// for every sample until its tile's sigma was known — cost 0.75–0.78 s of a
/// 61 MP develop (two frames), close to a fifth of it; what it costs now is in
/// ARCHITECTURE beside the frames it was timed on.
fn median8(data: &[u16], w: usize, x: usize, y: usize) -> f32 {
    let mut v = ring8(data, w, x, y);
    // An optimal 19-comparator sorting network for eight inputs (checked over
    // all 256 zero-one inputs when it was written, and against a plain sort in
    // the tests), less its last comparator: that one only orders the middle
    // pair, and the pair is summed. Literal indices, so the eight values live
    // in registers.
    macro_rules! order {
        ($($a:literal $b:literal),+) => {$(
            let (lo, hi) = (v[$a].min(v[$b]), v[$a].max(v[$b]));
            v[$a] = lo;
            v[$b] = hi;
        )+};
    }
    order!(0 1, 2 3, 4 5, 6 7, 0 2, 1 3, 4 6, 5 7, 1 2, 5 6, 0 4, 3 7, 1 5, 2 6, 1 4, 3 6, 2 4, 3 5);
    (u32::from(v[3]) + u32::from(v[4])) as f32 * 0.5
}

/// The spread, as a sigma, of the same-colour samples within four pixels of a
/// site: the MAD about their median of those that can carry noise (below the
/// clip), the site itself left out. It is the noise AT THE SITE'S OWN
/// BRIGHTNESS and the texture of its plane at once, measured where the site
/// stands. `None` when fewer than half of them can carry noise: mostly clipped
/// ground measures none, so nothing on it is judged. Candidates only, so plain
/// sorts; in doubled integers, because a median of integers may be a half.
fn local_sigma(data: &[u16], w: usize, h: usize, x: usize, y: usize, clip: f32) -> Option<f32> {
    let mut twice = [0u32; 24];
    let (mut n, mut seen) = (0, 0);
    // Two steps back where the frame allows them, the site's own parity kept.
    let (x0, y0) = (x - 2 * (x / 2).min(2), y - 2 * (y / 2).min(2));
    for yy in (y0..h.min(y + 5)).step_by(2) {
        for xx in (x0..w.min(x + 5)).step_by(2) {
            if (xx, yy) == (x, y) {
                continue;
            }
            seen += 1;
            let value = data[yy * w + xx];
            if f32::from(value) < clip {
                twice[n] = 2 * u32::from(value);
                n += 1;
            }
        }
    }
    if n == 0 || 2 * n < seen {
        return None;
    }
    let near = &mut twice[..n];
    near.sort_unstable();
    let median = (near[(n - 1) / 2] + near[n / 2]) / 2;
    for value in near.iter_mut() {
        *value = value.abs_diff(median);
    }
    near.sort_unstable();
    Some(1.4826 * (near[(n - 1) / 2] + near[n / 2]) as f32 * 0.25)
}

struct Tile {
    /// The residual MAD of the samples that can carry noise (neither they nor
    /// their background near the clip). Zero when there were none.
    sigma: f32,
    /// (index, background, excess) of every sample above 20 TILE sigmas — a
    /// superset of the hot sites, since the scale that finally judges one is
    /// never smaller than its tile's.
    candidates: Vec<(usize, f32, f32)>,
}

/// One CFA phase of one tile, `id` counting phases, then rows, then columns of
/// tiles. Samples at the two-pixel boundary contribute to the statistics, but
/// cannot be candidates.
fn scan(data: &[u16], w: usize, h: usize, levels: &super::MosaicArgs, id: usize) -> Tile {
    let side = 2 * PLANE_TILE;
    let (nx, ny) = (w.div_ceil(side), h.div_ceil(side));
    let phase = id / (nx * ny);
    let tx = id % nx;
    let ty = id / nx % ny;
    let clip = levels.black[phase] + CLIP_SHARE * (levels.white - levels.black[phase]);
    // This phase's samples in the tile that have all eight same-colour
    // neighbours: the two-pixel frame border is cut from the ranges, not
    // tested per sample. `side` is even, so a start moved in by 2 keeps its phase.
    let inside = |start: usize| if start < 2 { start + 2 } else { start };
    let (y0, y1) = (inside(ty * side + phase / 2), ((ty + 1) * side).min(h - 2));
    let (x0, x1) = (inside(tx * side + phase % 2), ((tx + 1) * side).min(w - 2));
    let mut residuals = Vec::with_capacity(PLANE_TILE * PLANE_TILE);
    let mut backgrounds = Vec::with_capacity(PLANE_TILE * PLANE_TILE);
    // The samples that can carry noise, apart: the two selections below
    // reorder their input, and the scan's own columns keep their order for
    // the candidate sweep.
    let mut spread = Vec::with_capacity(PLANE_TILE * PLANE_TILE);
    for y in (y0..y1).step_by(2) {
        for x in (x0..x1).step_by(2) {
            let (value, background) = (f32::from(data[y * w + x]), median8(data, w, x, y));
            residuals.push(value - background);
            backgrounds.push(background);
            if value < clip && background < clip {
                spread.push(value - background);
            }
        }
    }
    if spread.is_empty() {
        return Tile { sigma: 0.0, candidates: Vec::new() };
    }
    let centre = median(&mut spread);
    for r in &mut spread { *r = (*r - centre).abs(); }
    let sigma = 1.4826 * median(&mut spread);
    // Above 20 TILE sigmas: every hot site is among these. A tile without a
    // sigma (every measuring residual alike) has no scale to judge by.
    let mut candidates = Vec::new();
    let mut sample = residuals.iter().zip(&backgrounds);
    for y in (y0..y1).step_by(2) {
        for x in (x0..x1).step_by(2) {
            let (&excess, &background) = sample.next().expect("one residual per scanned sample");
            // …on ground the statistics above would have read: over a clipped background an excess is
            // what is left under white, not a measurement.
            if sigma > 0.0 && background < clip && excess > HOT_SIGMA * sigma && x >= 3 && y >= 3 && x + 3 < w && y + 3 < h {
                candidates.push((y * w + x, background, excess));
            }
        }
    }
    Tile { sigma, candidates }
}

fn find(data: &[u16], w: usize, h: usize, levels: &super::MosaicArgs) -> Vec<(usize, u16)> {
    if w < 7 || h < 7 {
        return Vec::new();
    }
    let side = 2 * PLANE_TILE;
    let nx = w.div_ceil(side);
    let ny = h.div_ceil(side);
    // One tile's residuals per worker, not a full-frame f32 sigma map.
    let tiles: Vec<Tile> = (0..4 * nx * ny).into_par_iter().map(|id| scan(data, w, h, levels, id)).collect();
    let per_phase = nx * ny;
    let tile_at = |x: usize, y: usize| &tiles[((y % 2) * 2 + x % 2) * per_phase + (y / side) * nx + x / side];
    tiles.par_iter().flat_map_iter(|tile| tile.candidates.iter().filter_map(|&(i, background, excess)| {
        let (x, y) = (i % w, i / w);
        let phase = (y % 2) * 2 + x % 2;
        let clip = levels.black[phase] + CLIP_SHARE * (levels.white - levels.black[phase]);
        // The tile's sigma is the noise of the tile's majority. Where the site
        // stands the ground may be brighter (many times the photon noise) or
        // textured in its own plane (every bright sample an outlier of the
        // same-colour median): its own neighbourhood says so, and the larger of
        // the two scales judges it.
        let scale = tile.sigma.max(LOCAL_SHARE * local_sigma(data, w, h, x, y, clip)?);
        if excess <= HOT_SIGMA * scale {
            return None;
        }
        // Light reaches every photosite around the one it lands on; a defect
        // reaches none. A witness vouches only if it could have shown light.
        // One thing else puts a loud sample beside a hot one: the camera's own
        // long-exposure filter levels a sample down to its brightest same-colour
        // neighbour, so what survives it comes in same-colour PAIRS that repeat
        // each other to the count (3649 beside 3649, 1597 beside 1597, 5469
        // beside 5468 on two night frames; none of them at a photosite the
        // other frame shares, none with a lit neighbour of another colour —
        // particle hits, not stars). For red and blue the partner is two pixels
        // off, outside these eight; for green it is a DIAGONAL neighbour. Light
        // does not repeat itself to the count.
        let alone = [
            (x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1),
            (x - 1, y - 1), (x + 1, y - 1), (x - 1, y + 1), (x + 1, y + 1),
        ].iter().enumerate().all(|(k, &(xx, yy))| {
            if k >= 4 && data[yy * w + xx].abs_diff(data[i]) <= 1 {
                return true;
            }
            // A witness is heard at its TILE's sigma, never a larger one: every
            // doubt is read for the scene. (Heard at the sigma its own brightness
            // would earn, the tip of a trailed star lost a photosite on the star
            // frame — its neighbours, lit by the trail, passed as quiet.)
            let around = median8(data, w, xx, yy);
            let quiet = NEIGHBOUR_SIGMA * tile_at(xx, yy).sigma;
            quiet > 0.0 && f32::from(data[yy * w + xx]) - around <= quiet && levels.white - around >= quiet
        });
        alone.then_some((i, background.round() as u16))
    })).collect()
}
