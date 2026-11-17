use std::collections::BTreeSet;

use image::{DynamicImage, GrayImage, Luma};

use super::*;
use crate::rationale::values;

pub(super) const SPATIAL_MAX_DEPTH: u8 = 2;
pub(super) const SPATIAL_MAX_ATTACHMENTS: usize = 4;
pub(super) const SPATIAL_RESIDUAL_MIN: f32 = 2.0 / 255.0;
pub(super) const SPATIAL_FRAME_REGRESSION_TOL: f32 = 0.0;
pub(super) const TILE_RASTER_EDGE: u32 = 2048;
/// Half a pixel of the tile analysis raster, in normalised frame units.
/// The Zero/Full ramp fits between adjacent pixel centres, preserving the
/// integer cell exactly. Unrelated to the renderer's guided-filter tile size.
const TILE_GRADIENT_RAMP: f32 = 0.5 / TILE_RASTER_EDGE as f32;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct TileId {
    pub(super) depth: u8,
    pub(super) row: u8,
    pub(super) col: u8,
}

impl TileId {
    fn grid(self) -> u32 {
        1u32 << self.depth
    }

    fn label(self) -> String {
        format!("Spatial tile r{}c{}", self.row, self.col)
    }

    fn tag(self) -> String {
        format!("d{}r{}c{}", self.depth, self.row, self.col)
    }

}

#[derive(Clone, Debug)]
pub(super) struct TileReading {
    pub(super) id: TileId,
    pub(super) source_weights: Vec<f32>,
    pub(super) target_weights: Vec<f32>,
    pub(super) source_share: f32,
    pub(super) target_share: f32,
    /// The tile's own footprint on the analysis grid, in pixels — the quantity
    /// [`MIN_MASK_PIXELS`] gates, and not the same thing as the evidence-scoped
    /// `source_share` beside it (a share is a weighted fraction of the frame;
    /// this is a count of the pixels the raster actually covers).
    pub(super) pixels: usize,
    pub(super) residual: f32,
    pub(super) ci95: f32,
    /// `None` is [`fit::structure_divergence`]'s abstention. It is NOT a matched
    /// reading, so [`eligible`] refuses the tile rather than letting a footprint
    /// nothing measured clear a gate that exists to require survival.
    pub(super) divergence: Option<fit::Divergence>,
}

impl TileReading {
    fn score(&self) -> f32 {
        self.residual.abs() * self.source_share.min(self.target_share)
    }
}

pub(super) struct ScopedMaskEvidence {
    pub(super) source_weights: Vec<f32>,
    pub(super) target_weights: Vec<f32>,
    pub(super) source_share: f32,
    pub(super) target_share: f32,
}

/// Re-aggregate the frozen frame evidence over one analysis-grid mask. Tiles
/// and free-form components share this exact population ruler.
pub(super) fn scoped_mask_evidence(
    target: &[[f32; 3]],
    evidence: &fit::EvidenceModel,
    geometry: &[f32],
) -> ScopedMaskEvidence {
    let n = (evidence.width as usize * evidence.height as usize)
        .min(target.len())
        .min(evidence.source_weights.len())
        .min(evidence.target_weights.len())
        .min(geometry.len());
    let scoped = evidence.scoped(target, &geometry[..n], &geometry[..n]);
    let mut source_weights = scoped.source_weights;
    let mut target_weights = scoped.target_weights;
    source_weights.resize(n, 0.0);
    target_weights.resize(n, 0.0);
    let source_share = source_weights.iter().map(|v| *v as f64).sum::<f64>() as f32
        / n.max(1) as f32;
    let target_share = target_weights.iter().map(|v| *v as f64).sum::<f64>() as f32
        / n.max(1) as f32;
    ScopedMaskEvidence { source_weights, target_weights, source_share, target_share }
}

fn in_tile(id: TileId, x: u32, y: u32, width: u32, height: u32) -> bool {
    x * id.grid() / width.max(1) == id.col as u32
        && y * id.grid() / height.max(1) == id.row as u32
}

fn weighted_residual(
    current: &[[f32; 3]],
    target: &[[f32; 3]],
    weights: &[f32],
) -> (f32, f32) {
    let mut sum_w = 0.0f64;
    let mut sum_w2 = 0.0f64;
    let mut sum = 0.0f64;
    for ((source, target), weight) in current.iter().zip(target).zip(weights) {
        let weight = weight.max(0.0) as f64;
        if weight == 0.0 {
            continue;
        }
        let residual = (fit::luma601(target) - fit::luma601(source)) as f64;
        sum_w += weight;
        sum_w2 += weight * weight;
        sum += weight * residual;
    }
    if sum_w <= 0.0 || sum_w2 <= 0.0 {
        return (0.0, f32::INFINITY);
    }
    let mean = sum / sum_w;
    let mut variance = 0.0f64;
    for ((source, target), weight) in current.iter().zip(target).zip(weights) {
        let weight = weight.max(0.0) as f64;
        if weight == 0.0 {
            continue;
        }
        let residual = (fit::luma601(target) - fit::luma601(source)) as f64;
        variance += weight * (residual - mean).powi(2);
    }
    variance /= sum_w;
    let effective_n = sum_w * sum_w / sum_w2;
    let ci95 = 1.96 * (variance / effective_n.max(1.0)).sqrt();
    (mean as f32, ci95 as f32)
}

/// Everything a tile reading holds that does NOT depend on the current render:
/// its geometry, the frame evidence re-aggregated over that geometry, its
/// footprint and its structural reading.  The traversal re-reads the same
/// node once per generation — a four-tile attachment sweeps the same 20 nodes
/// four times — and only the residual moves between those passes, because the
/// evidence model is frozen before the first producer runs and the structural
/// reading is taken against `evidence.source_pixels`, not against the render.
/// Keyed by [`TileId`], so the recompute happens once per node per fit.
struct TileEvidence {
    scoped: ScopedMaskEvidence,
    pixels: usize,
    divergence: Option<fit::Divergence>,
}

type TileEvidenceCache = std::collections::BTreeMap<TileId, std::rc::Rc<TileEvidence>>;


fn tile_evidence(
    id: TileId,
    target: &[[f32; 3]],
    evidence: &fit::EvidenceModel,
    cache: &mut TileEvidenceCache,
) -> std::rc::Rc<TileEvidence> {
    if let Some(hit) = cache.get(&id) {
        return std::rc::Rc::clone(hit);
    }
    let n = (evidence.width as usize * evidence.height as usize)
        .min(target.len())
        .min(evidence.source_weights.len())
        .min(evidence.target_weights.len());
    let mut geometry = vec![0.0f32; n];
    for y in 0..evidence.height {
        for x in 0..evidence.width {
            let i = (y * evidence.width + x) as usize;
            if i < n && in_tile(id, x, y, evidence.width, evidence.height) {
                geometry[i] = 1.0;
            }
        }
    }
    // The tile is judged by ITS population: the frame's per-bin verdicts are
    // re-aggregated over the tile's own members, so a mid-tone ground tile
    // keeps the evidence that a replaced sky's identical luma bins withheld
    // frame-wide.
    let scoped = scoped_mask_evidence(target, evidence, &geometry);
    // The tile's structure is read over the pixels it can pair: a corner
    // tile of a target moved into the sensor frame holds part of the ring.
    let paired = evidence.present_weights(&geometry);
    let divergence = fit::structure_divergence(
        &evidence.source_pixels[..n.min(evidence.source_pixels.len())],
        &target[..n],
        evidence.width,
        evidence.height,
        &paired,
    );
    let entry = std::rc::Rc::new(TileEvidence {
        pixels: paired.iter().filter(|value| **value > 0.0).count(),
        scoped,
        divergence,
    });
    cache.insert(id, std::rc::Rc::clone(&entry));
    entry
}

fn read_tile(
    id: TileId,
    current: &[[f32; 3]],
    target: &[[f32; 3]],
    evidence: &fit::EvidenceModel,
    cache: &mut TileEvidenceCache,
) -> TileReading {
    let held = tile_evidence(id, target, evidence, cache);
    let (residual, ci95) = weighted_residual(current, target, &held.scoped.source_weights);
    TileReading {
        id,
        source_weights: held.scoped.source_weights.clone(),
        target_weights: held.scoped.target_weights.clone(),
        // ONE share ruler: this reading is the same number the attachment gate
        // now applies (`fit_zoned::attach_one_zone`), so a tile admitted here
        // can no longer be refused there for a size it was never measured at.
        source_share: held.scoped.source_share,
        target_share: held.scoped.target_share,
        pixels: held.pixels,
        residual,
        ci95,
        divergence: held.divergence,
    }
}

fn eligible(reading: &TileReading, parent_residual: f32) -> Result<(), &'static str> {
    // The footprint floor comes FIRST because it is the cheapest true statement
    // about the candidate, and because the gates under it read statistics taken
    // over that footprint: a share and a structural correlation measured over a
    // handful of pixels are not more informative for having been computed.
    if reading.pixels < MIN_MASK_PIXELS {
        Err("footprint")
    } else if reading.source_share < MIN_ZONE_SHARE {
        Err("source-share")
    } else if reading.target_share < MIN_ZONE_SHARE {
        Err("target-share")
    } else if reading.divergence.is_none() {
        // No reading, so no claim that the structure survived — and survival is
        // exactly what the next arm requires. Until v1.2.4 the instrument
        // answered an unmeasurable footprint with D = 0 and this gate passed it.
        Err("structure-unmeasured")
    } else if reading.divergence.is_some_and(|d| d.d >= fit::DIVERGENCE_ZONE) {
        Err("structural-divergence")
    } else if !reading.ci95.is_finite() || reading.residual.abs() <= reading.ci95 {
        Err("confidence-interval")
    } else if (reading.residual - parent_residual).abs() < SPATIAL_RESIDUAL_MIN {
        Err("parent-residual")
    } else {
        Ok(())
    }
}

fn reading_args(
    reading: &TileReading,
    parent_residual: f32,
) -> Vec<(&'static str, String)> {
    vec![
        ("id", reading.id.tag()),
        ("s", format!("{:.3}", reading.source_share)),
        ("t", format!("{:.3}", reading.target_share)),
        // An unread footprint says so; a 0.000 here would read as a measured
        // perfect structural match.
        ("d", reading.divergence.map_or_else(
            || values::UNMEASURED.to_string(), |d| format!("{:.3}", d.d))),
        ("residual", format!("{:+.5}", reading.residual)),
        ("parent", format!("{:+.5}", parent_residual)),
        ("ci", format!("{:.5}", reading.ci95)),
    ]
}

fn tile_mask(src: &DynamicImage, id: TileId) -> (crate::render::Plate, GrayImage) {
    let guide = crate::render::Plate::thumbnail_of(src, TILE_RASTER_EDGE);
    let mut mask = GrayImage::new(guide.width(), guide.height());
    for y in 0..mask.height() {
        for x in 0..mask.width() {
            let value = if in_tile(id, x, y, mask.width(), mask.height()) { 255 } else { 0 };
            mask.put_pixel(x, y, Luma([value]));
        }
    }
    (guide, mask)
}

pub(super) fn push_refinement_note(
    report: &mut FitReport,
    label: &str,
    kept: bool,
    reading: crate::mask_refine::RefineReading,
) {
    crate::rationale::push_note(
        &mut report.recipe.rationale,
        &mut report.notes,
        crate::rationale::Note::new(
            if kept {
                crate::rationale::keys::MASK_REFINEMENT_KEPT
            } else {
                crate::rationale::keys::MASK_REFINEMENT_ABSTAINED
            },
            vec![
                ("label", label.to_string()),
                ("coverage", format!("{:.6}", reading.coverage_delta)),
                ("before", format!("{:.6}", reading.edge_before)),
                ("after", format!("{:.6}", reading.edge_after)),
                ("core", reading.core_changed.to_string()),
            ],
        ),
    );
}

fn push_abstention(
    report: &mut FitReport,
    reading: &TileReading,
    parent_residual: f32,
    reason: &str,
    generation: usize,
) {
    let mut args = reading_args(reading, parent_residual);
    args.push(("reason", reason.to_string()));
    args.push(("generation", generation.to_string()));
    crate::rationale::push_note(
        &mut report.recipe.rationale,
        &mut report.notes,
        crate::rationale::Note::new(crate::rationale::keys::TILE_ABSTAINED, args),
    );
}

#[derive(Clone, Debug)]
struct PendingTile {
    reading: TileReading,
    parent_residual: f32,
}

type TileVisit = (TileReading, f32, Result<(), &'static str>);
type TileSearch = (Vec<TileVisit>, Option<TileReading>);

fn rank_pending(pending: &mut [PendingTile]) {
    pending.sort_by(|a, b| {
        b.reading
            .score()
            .total_cmp(&a.reading.score())
            .then_with(|| a.reading.id.cmp(&b.reading.id))
    });
}

fn next_tile(
    current: &[[f32; 3]],
    target: &[[f32; 3]],
    evidence: &fit::EvidenceModel,
    attached: &BTreeSet<TileId>,
    refused: &BTreeSet<TileId>,
    cache: &mut TileEvidenceCache,
) -> TileSearch {
    let root = read_tile(TileId { depth: 0, row: 0, col: 0 }, current, target, evidence, cache);
    let mut pending = Vec::new();
    for row in 0..2 {
        for col in 0..2 {
            pending.push(PendingTile {
                reading: read_tile(
                    TileId { depth: 1, row, col },
                    current,
                    target,
                    evidence,
                    cache,
                ),
                parent_residual: root.residual,
            });
        }
    }
    let mut visited = Vec::new();
    while !pending.is_empty() {
        rank_pending(&mut pending);
        let node = pending.remove(0);
        let verdict = eligible(&node.reading, node.parent_residual);
        let eligible_node = verdict.is_ok();
        visited.push((node.reading.clone(), node.parent_residual, verdict));
        if node.reading.id.depth == SPATIAL_MAX_DEPTH {
            if eligible_node
                && !attached.contains(&node.reading.id)
                && !refused.contains(&node.reading.id)
            {
                return (visited, Some(node.reading));
            }
            continue;
        }
        let depth = node.reading.id.depth + 1;
        for row_offset in 0..2 {
            for col_offset in 0..2 {
                pending.push(PendingTile {
                    reading: read_tile(
                        TileId {
                            depth,
                            row: node.reading.id.row * 2 + row_offset,
                            col: node.reading.id.col * 2 + col_offset,
                        },
                        current,
                        target,
                        evidence,
                        cache,
                    ),
                    parent_residual: node.reading.residual,
                });
            }
        }
    }
    (visited, None)
}

fn tile_geometry(id: TileId, size: (u32, u32)) -> (MaskGeometry, Vec<MaskComponent>) {
    // Integer cell arithmetic places a break at ceil(k * size / grid), not
    // k / grid when the raster has an odd dimension. Keep that exact edge.
    let edge = |k: u32, n: u32| (k * n).div_ceil(id.grid()) as f32 / n.max(1) as f32;
    let (left, right) = (edge(id.col as u32, size.0), edge(id.col as u32 + 1, size.0));
    let (top, bottom) = (edge(id.row as u32, size.1), edge(id.row as u32 + 1, size.1));
    let r = TILE_GRADIENT_RAMP * 0.5;
    let shapes = [
        MaskGeometry::Linear { zero_x: left - r, zero_y: 0.5, full_x: left + r, full_y: 0.5 },
        MaskGeometry::Linear { zero_x: right + r, zero_y: 0.5, full_x: right - r, full_y: 0.5 },
        MaskGeometry::Linear { zero_x: 0.5, zero_y: top - r, full_x: 0.5, full_y: top + r },
        MaskGeometry::Linear { zero_x: 0.5, zero_y: bottom + r, full_x: 0.5, full_y: bottom - r },
    ];
    let mut shapes = shapes.into_iter();
    let base = shapes.next().expect("four half planes");
    let components = shapes.map(|geometry| MaskComponent { inverted: false, geometry, mode: MaskCombine::Intersect }).collect();
    (base, components)
}

fn tile_attachment(
    reading: &TileReading,
    size: (u32, u32),
    source_weights: Vec<f32>,
    target_weights: Vec<f32>,
    coverage: ZoneCoverage,
) -> ZoneAttachment {
    let (mask, components) = tile_geometry(reading.id, size);
    ZoneAttachment {
        source_weights,
        target_weights,
        coverage: Some(coverage),
        mask,
        components,
        range: None,
        name: reading.id.label(),
        role: MaskRole::Custom,
        inverted: false,
        label: reading.id.label(),
        min_share: MIN_ZONE_SHARE,
        frame_regression_tol: SPATIAL_FRAME_REGRESSION_TOL,
    }
}

/// R36. Which carrier ships once a refined raster and its native
/// four-gradient twin have BOTH passed the estimator and the boundary gate:
/// the native one, unless it fits the photo worse — on the hard cell's own
/// population or on the frame — by more than the tie band. R35 asked instead
/// whether the two RENDERS resembled each other within the boundary budget,
/// which kept three of the reference pair's four tiles as rasters whose edge
/// alpha the guide had moved without ever asking which edge fits the photo
/// better. The raster is an intermediate, not the truth; equal fidelity is a
/// tie, and a tie goes to the carrier Lightroom can read.
fn native_carrier_fits(raster_zone: f32, native_zone: f32, raster_frame: f32, native_frame: f32) -> bool {
    native_zone <= raster_zone + CARRIER_TIE && native_frame <= raster_frame + CARRIER_TIE
}
const CARRIER_TIE: f32 = 1e-6;

/// Alpha error bounds the compositing error for channel values in [0,1].
/// Compare every pixel, not only coverage and the frozen cores: equal mass
/// could otherwise hide an edge moved from one place to another.
fn refinement_alpha_delta(hard: &GrayImage, refined: &GrayImage) -> f32 {
    if hard.dimensions() != refined.dimensions() { return f32::INFINITY; }
    hard.as_raw().iter().zip(refined.as_raw())
        .map(|(a, b)| (*a as f32 - *b as f32).abs() / 255.0)
        .fold(0.0, f32::max)
}

fn boundary_args(
    id: TileId,
    k: f32,
    before: BoundaryReading,
    after: BoundaryReading,
) -> Vec<(&'static str, String)> {
    vec![
        ("id", id.tag()),
        ("k", format!("{k:.3}")),
        ("before", format!("{:.4}", before.rim)),
        ("after", format!("{:.4}", after.rim)),
        ("asked", format!("{:.4}", after.asked)),
        ("charged", format!("{:.4}", after.charged)),
        ("colour", format!("{:.4}", after.colour)),
        ("colour_charged", format!("{:.4}", after.colour_charged)),
        ("max", format!("{ZONE_BOUNDARY_STEP_MAX:.3}")),
        ("transitions", after.transitions.to_string()),
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BitmapBoundaryWhy {
    /// The composed frame at the surviving `k` regressed by more than the
    /// caller's own admission tolerance (`BitmapBoundaryInput::frame_tol`).
    Frame,
    Rim,
    /// The mask reaches the gate with a contour that could not be sampled at
    /// all. An unmeasurable boundary is a REFUSAL, never a pass: reading
    /// `0.000` off `0` transitions is exactly what let every hard-edged tile
    /// through the gate that was supposed to be holding the seam budget.
    Unmeasured,
    /// The correction that survived the gate does not move a single pixel of
    /// the analysis render: the accepted render is byte-identical to the
    /// frame WITHOUT it. Step 9 made this reachable — under the transported
    /// differential a `k=0` render reads exactly 0.0 by construction, so the
    /// budget can never refuse it and the bisection returns the largest `k`
    /// that passes, which for a correction whose every visible strength
    /// introduces a seam is a `k` that renders to nothing.
    ///
    /// The test is BYTE IDENTITY of the render, deliberately not a threshold
    /// on `k`. `k == 0.0` alone would catch almost nothing: the reading falls
    /// continuously to zero with `k`, and the 8-bit analysis render quantises
    /// a small enough `k` into a literal no-op, so the bisection almost
    /// always lands on a tiny POSITIVE `k` rather than on zero. And a
    /// threshold ON `k` is not a threshold on visibility at all — `k` scales
    /// whatever dials the zone happens to carry, so the same `k` moves
    /// different numbers of pixels for different corrections. Comparing the
    /// two renders asks the question directly and needs no constant.
    ///
    /// An inert attachment is strictly worse than a refusal: it occupies the
    /// exclusion budget the next candidate needs, consumes the attachment
    /// cap, keeps a raster on disk, and discloses a before/after pair it did
    /// not produce.
    Inert,
}

pub(super) struct BitmapBoundaryAccepted {
    pub(super) pixels: Vec<[f32; 3]>,
    pub(super) reading: BoundaryReading,
    pub(super) initial: BoundaryReading,
    pub(super) k: f32,
}

#[derive(Debug)]
pub(super) struct BitmapBoundaryRefusal {
    pub(super) why: BitmapBoundaryWhy,
    pub(super) initial: BoundaryReading,
}

/// Which ruler measures this mask family's boundary. These are two readings
/// of two different shapes, not two estimates of one number, so the caller
/// states which shape it is handing over rather than letting one ruler
/// silently return 0.0 on the other's geometry.
pub(super) enum BoundaryRuler<'a> {
    /// Soft, feathered masks — semantic segmentation rasters. The signed
    /// overshoot INSIDE the transition band, measured against the settled
    /// interiors on the same scan line ([`boundary_rim`]). `weights` is the
    /// exact vector this ruler has always been handed: on the semantic-region
    /// path that is the segmentation raster's OWN alpha at analysis size
    /// (`mask_weights` of `region.source`), so it is already a contour and
    /// not an evidence-scoped product.
    TransitionBand {
        weights: &'a [f32],
        /// The same frame rendered WITHOUT this correction, for exactly the
        /// reason [`BoundaryRuler::CrossBoundaryStep`] carries one: the rim
        /// this ruler now reports is the rim the correction INTRODUCED, so a
        /// bow the scene already had under the feather must cancel instead of
        /// being charged. Carried in the ENUM rather than as a loose
        /// argument, so a caller cannot hand over a stale reference by
        /// omitting it.
        reference: &'a [[f32; 3]],
    },
    /// Hard 0/255 rasters — spatial tiles and free masks, which have no
    /// transition band for the rim ruler to read and therefore always scored
    /// `rim 0.000 / 0 transitions` and passed. The correction's own induced
    /// step ACROSS the 50% contour ([`boundary_step`]).
    CrossBoundaryStep {
        /// The mask's own alpha at analysis size — the contour the renderer
        /// applies. NOT the estimator weights: those carry the zone's per-bin
        /// evidence verdicts, so their 50% contour is punched full of interior
        /// holes that no correction can ever make visible.
        geometry: &'a [f32],
        /// The same frame rendered WITHOUT this correction. The gated quantity
        /// is a difference in differences against it, which is what makes a
        /// hard raster measurable at all and what keeps a real subject edge
        /// under the mask border from reading as a seam nobody introduced.
        reference: &'a [[f32; 3]],
    },
}

pub(super) struct BitmapBoundaryInput<'a> {
    pub(super) ruler: BoundaryRuler<'a>,
    /// R37. The paired target the boundary is allowed to reproduce, at the
    /// analysis geometry: the step IT carries at a crossing is not a seam
    /// (`fit_zoned::unasked`). `None` charges every introduced step — the
    /// context rule alone, which is what the fixtures pinned on it measure.
    pub(super) target_boundary: Option<&'a [[f32; 3]]>,
    pub(super) initial_px: Vec<[f32; 3]>,
    pub(super) frame_before: f32,
    /// The composed-frame regression this caller's ADMISSION allowed: the
    /// gate re-checks the frame at the surviving `k` under the same
    /// tolerance the candidate was admitted on, never a stricter one of its
    /// own. Spatial tiles and free masks pass
    /// [`SPATIAL_FRAME_REGRESSION_TOL`]; a semantic region passes
    /// [`ZONE_GLOBAL_REGRESSION_TOL`] (2026-10-22: the gate used to apply 0.0
    /// to every family, so a region `attach_one_zone` had admitted within its
    /// 0.02 drift budget was silently refused here).
    pub(super) frame_tol: f32,
}

/// One bitmap boundary/composed-frame gate shared by tiles, free masks and
/// the multi-region semantic path, each naming its own ruler and its own
/// frame tolerance (`frame_tol`, the caller's admission tolerance — one
/// tolerance per gate family).
///
/// For [`BoundaryRuler::CrossBoundaryStep`] a reading of `0` transitions is a
/// REFUSAL. That combination is precisely what shipped a seam: every hard
/// raster scored `rim 0.000` off an empty transition band, and the gate read
/// that as "well inside budget".
pub(super) fn enforce_bitmap_boundary(
    s_img: &DynamicImage,
    tgt_px: &[[f32; 3]],
    report: &mut FitReport,
    first_mask: usize,
    input: BitmapBoundaryInput<'_>,
) -> Result<BitmapBoundaryAccepted, BitmapBoundaryRefusal> {
    let BitmapBoundaryInput { ruler, target_boundary, initial_px, frame_before, frame_tol } = input;
    let presence = report.evidence.presence.clone();
    let measure = |rendered: &[[f32; 3]]| match ruler {
        BoundaryRuler::TransitionBand { weights, reference } => {
            // `initial_px` is this gate's k=1 candidate, so BOTH rulers read
            // the correction's own slope off the same frozen frame.
            boundary_rim_toward(
                target_boundary,
                reference,
                rendered,
                &initial_px,
                weights,
                s_img.width(),
                s_img.height(),
                presence.as_deref(),
            )
        }
        BoundaryRuler::CrossBoundaryStep { geometry, reference } => {
            boundary_step_toward(
                target_boundary,
                reference,
                rendered,
                &initial_px,
                geometry,
                s_img.width(),
                s_img.height(),
                presence.as_deref(),
            )
        }
    };
    // One ruler, one budget. The gate body is ruler-agnostic, so the budget
    // has to be chosen HERE rather than read from a constant both rulers
    // share — otherwise re-deriving the rim silently re-tunes every spatial
    // tile and free mask that passes through this same comparison.
    let budget = match ruler {
        BoundaryRuler::TransitionBand { .. } => ZONE_BOUNDARY_RIM_MAX,
        BoundaryRuler::CrossBoundaryStep { .. } => ZONE_BOUNDARY_STEP_MAX,
    };
    let initial = measure(&initial_px);
    let hard_edged = matches!(ruler, BoundaryRuler::CrossBoundaryStep { .. });
    if hard_edged && initial.transitions == 0 {
        report.recipe.masks.truncate(first_mask);
        return Err(BitmapBoundaryRefusal { why: BitmapBoundaryWhy::Unmeasured, initial });
    }
    let original = report.recipe.masks[first_mask].clone();
    let render_at = |report: &mut FitReport, k: f32| {
        shrink_zone_corrections(
            &mut report.recipe.masks[first_mask..=first_mask],
            std::slice::from_ref(&original),
            &[1.0],
            k,
        );
        let pixels = fit::pixels_of(&render::develop_preview(s_img, &report.recipe));
        let reading = measure(&pixels);
        let frame = fit::look_err_with_evidence(&pixels, tgt_px, &report.evidence);
        (reading, pixels, frame)
    };
    // `gated()`, not `rim`: every crossing ranked after its own context
    // charge, in luma AND in colour, whichever is worse. On a fully textured
    // or genuinely ramped border the charge branch returns the raw reading
    // bit for bit, so everything that passed on context before this batch
    // still does; what is new is that the soft family charges too, and that
    // a correction which moves one channel without moving luma is no longer
    // read as 0.
    let kept = if initial.gated() <= budget {
        let frame = fit::look_err_with_evidence(&initial_px, tgt_px, &report.evidence);
        Some((1.0, initial, initial_px, frame))
    } else {
        let zero = render_at(report, 0.0);
        if zero.0.gated() > budget {
            None
        } else {
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            let mut best = (0.0, zero.0, zero.1, zero.2);
            for _ in 0..12 {
                let mid = (lo + hi) * 0.5;
                let measured = render_at(report, mid);
                if measured.0.gated() <= budget {
                    lo = mid;
                    best = (mid, measured.0, measured.1, measured.2);
                } else {
                    hi = mid;
                }
            }
            Some(best)
        }
    };
    let Some((k, reading, pixels, frame)) = kept else {
        report.recipe.masks.truncate(first_mask);
        return Err(BitmapBoundaryRefusal { why: BitmapBoundaryWhy::Rim, initial });
    };
    // Both rulers now carry the frame rendered WITHOUT this correction, so
    // the question "did the surviving k actually do anything" is answerable
    // for free and exactly. See `BitmapBoundaryWhy::Inert`.
    let reference = match ruler {
        BoundaryRuler::TransitionBand { reference, .. } => reference,
        BoundaryRuler::CrossBoundaryStep { reference, .. } => reference,
    };
    if pixels == reference {
        report.recipe.masks.truncate(first_mask);
        return Err(BitmapBoundaryRefusal { why: BitmapBoundaryWhy::Inert, initial });
    }
    if frame > frame_before + frame_tol {
        report.recipe.masks.truncate(first_mask);
        return Err(BitmapBoundaryRefusal { why: BitmapBoundaryWhy::Frame, initial });
    }
    shrink_zone_corrections(
        &mut report.recipe.masks[first_mask..=first_mask],
        std::slice::from_ref(&original),
        &[1.0],
        k,
    );
    Ok(BitmapBoundaryAccepted { pixels, reading, initial, k })
}

pub(super) fn attach_tiles(
    src: &DynamicImage,
    target: &DynamicImage,
    report: &mut FitReport,
    raster_home: &crate::store::OwnedRaster,
    refine: bool,
    cap: usize,
) -> Vec<f32> {
    // One analysis geometry for both rasters (`fit::analysis_pair`), so the
    // coverage and estimator vectors below are congruent by construction —
    // the two asserts pin that contract.
    let (s_img, t_img) = fit::analysis_pair(src, target);
    let tgt_px = fit::pixels_of(&t_img);
    let mut attached = BTreeSet::new();
    let mut refused = BTreeSet::new();
    let mut generation = 0usize;
    let mut excluded = vec![0.0f32; report.evidence.source_weights.len()];
    // One cache for the whole traversal: the render-independent half of every
    // node's reading is computed once and re-read on the later generations.
    let mut cache = TileEvidenceCache::new();
    let corr = report.correspondence.take();
    while attached.len() < cap {
        let current = fit::pixels_of(&render::develop_preview(&s_img, &report.recipe));
        let root = read_tile(
            TileId { depth: 0, row: 0, col: 0 },
            &current,
            &tgt_px,
            &report.evidence,
            &mut cache,
        );
        let (visited, candidate) = next_tile(
            &current,
            &tgt_px,
            &report.evidence,
            &attached,
            &refused,
            &mut cache,
        );
        // ONE aggregated sweep note per generation: the full per-node map
        // re-rendered every generation was a transcript, and it truncated the
        // attachment disclosure off the persisted rationale. Leaf candidates
        // keep their full reading; every other verdict survives as id-in-
        // bucket; nodes already attached or refused told their story in their
        // own generation.
        let mut sweep: [(&str, Vec<String>); 7] = [
            ("eligible", Vec::new()),
            ("source-share", Vec::new()),
            ("target-share", Vec::new()),
            ("structural-divergence", Vec::new()),
            ("confidence-interval", Vec::new()),
            ("parent-residual", Vec::new()),
            ("other", Vec::new()),
        ];
        for (reading, parent_residual, verdict) in visited {
            if attached.contains(&reading.id) || refused.contains(&reading.id) {
                continue;
            }
            match verdict {
                Ok(()) if reading.id.depth == SPATIAL_MAX_DEPTH => {
                    let mut args = reading_args(&reading, parent_residual);
                    args.push(("generation", generation.to_string()));
                    crate::rationale::push_note(
                        &mut report.recipe.rationale,
                        &mut report.notes,
                        crate::rationale::Note::new(crate::rationale::keys::TILE_ELIGIBLE, args),
                    );
                }
                Ok(()) => sweep[0].1.push(reading.id.tag()),
                Err(reason) => {
                    let (bucket, tag) = match sweep.iter().position(|(key, _)| *key == reason) {
                        Some(found) => (found, reading.id.tag()),
                        None => (6, format!("{}({reason})", reading.id.tag())),
                    };
                    sweep[bucket].1.push(tag);
                }
            }
        }
        let list = |ids: &[String]| {
            if ids.is_empty() { "none".to_string() } else { ids.join(" ") }
        };
        crate::rationale::push_note(
            &mut report.recipe.rationale,
            &mut report.notes,
            crate::rationale::Note::new(
                crate::rationale::keys::TILE_SWEEP,
                vec![
                    ("generation", generation.to_string()),
                    ("eligible", list(&sweep[0].1)),
                    ("s", list(&sweep[1].1)),
                    ("t", list(&sweep[2].1)),
                    ("d", list(&sweep[3].1)),
                    ("ci", list(&sweep[4].1)),
                    ("parent", list(&sweep[5].1)),
                    ("other", list(&sweep[6].1)),
                ],
            ),
        );
        let Some(reading) = candidate else { break };
        let owned = match raster_home.claim_sibling("mask-zone-tile") {
            Ok(path) => path,
            Err(e) => {
                push_abstention(
                    report,
                    &reading,
                    root.residual,
                    &format!("raster-claim: {e}"),
                    generation,
                );
                refused.insert(reading.id);
                continue;
            }
        };
        let (guide, raw_mask) = tile_mask(src, reading.id);
        let mut refinement_delta = 0.0;
        let (mask, mut refined) = if refine {
            match crate::mask_refine::guided_refine(
                &guide,
                &raw_mask,
                8,
                (4.0f32 / 255.0).powi(2),
            ) {
                crate::mask_refine::RefineOutcome::Kept { mask, reading: refined } => {
                    push_refinement_note(report, &reading.id.label(), true, refined);
                    refinement_delta = refinement_alpha_delta(&raw_mask, &mask);
                    if refinement_delta == 0.0 {
                        (raw_mask, false)
                    } else {
                        (mask, true)
                    }
                }
                crate::mask_refine::RefineOutcome::Abstained { reading: refined } => {
                    push_refinement_note(report, &reading.id.label(), false, refined);
                    (raw_mask, false)
                }
            }
        } else {
            (raw_mask, false)
        };
        if let Err(e) = owned.save_mask(&mask) {
            owned.remove();
            push_abstention(
                report,
                &reading,
                root.residual,
                &format!("raster-write: {e}"),
                generation,
            );
            refused.insert(reading.id);
            continue;
        }
        // The raster is what the correction moves; the estimator weights are
        // that raster times the tile's own evidence reading.
        let coverage = ZoneCoverage {
            source: mask_weights(&mask, s_img.width(), s_img.height()),
            target: mask_weights(&mask, t_img.width(), t_img.height()),
        };
        assert_eq!(coverage.source.len(), reading.source_weights.len());
        assert_eq!(coverage.target.len(), reading.target_weights.len());
        let mut accepted_coverage = coverage.source.clone();
        let (source_weights, target_weights) = if refined {
            let source = coverage
                .source
                .iter()
                .zip(&reading.source_weights)
                .map(|(mask, evidence)| mask * evidence)
                .collect::<Vec<_>>();
            let target = coverage
                .target
                .iter()
                .zip(&reading.target_weights)
                .map(|(mask, evidence)| mask * evidence)
                .collect::<Vec<_>>();
            (source, target)
        } else {
            (reading.source_weights.clone(), reading.target_weights.clone())
        };
        let mut attachment =
            tile_attachment(&reading, mask.dimensions(), source_weights, target_weights, coverage);
        if refined {
            attachment.mask = MaskGeometry::Bitmap { path: owned.path().to_string_lossy().into_owned() };
            attachment.components.clear();
        }
        let frame_before = fit::look_err_with_evidence(&current, &tgt_px, &report.evidence);
        let mut frame_err = frame_before;
        let first_tile = report.recipe.masks.len();
        let notes_before_attach = report.notes.len();
        let rationale_before_attach = report.recipe.rationale.len();
        let accepted = attach_one_zone(
            &s_img,
            &tgt_px,
            report,
            &mut frame_err,
            &attachment,
            reading.divergence,
            corr.as_ref(),
        );
        let Some(mut accepted) = accepted else {
            owned.remove();
            push_abstention(
                report,
                &reading,
                root.residual,
                "shared-estimator",
                generation,
            );
            refused.insert(reading.id);
            continue;
        };
        let boundary = enforce_bitmap_boundary(
            &s_img,
            &tgt_px,
            report,
            first_tile,
            BitmapBoundaryInput {
                ruler: BoundaryRuler::CrossBoundaryStep {
                    geometry: &accepted_coverage,
                    reference: &current,
                },
                target_boundary: Some(&tgt_px[..]),
                initial_px: std::mem::take(&mut accepted.rendered),
                frame_before,
                frame_tol: SPATIAL_FRAME_REGRESSION_TOL,
            },
        );
        let mut boundary = match boundary {
            Ok(boundary) => {
                crate::rationale::push_note(
                    &mut report.recipe.rationale,
                    &mut report.notes,
                    crate::rationale::Note::new(
                        crate::rationale::keys::TILE_BOUNDARY_PASSED,
                        boundary_args(reading.id, boundary.k, boundary.initial, boundary.reading),
                    ),
                );
                boundary
            }
            Err(refusal) => {
                crate::rationale::push_note(
                    &mut report.recipe.rationale,
                    &mut report.notes,
                    crate::rationale::Note::new(
                        crate::rationale::keys::TILE_BOUNDARY_REFUSED,
                        boundary_args(reading.id, 0.0, refusal.initial, refusal.initial),
                    ),
                );
                owned.remove();
                refused.insert(reading.id);
                continue;
            }
        };
        let mut rendered_delta = values::UNMEASURED_HARD_MASK.to_string();
        let mut fidelity = values::UNMEASURED.to_string();
        if refined {
            // A guide can move edge alpha substantially while the fitted
            // correction barely moves the image. Put a native trial through
            // the SAME estimator and boundary gate, then compare both actual
            // renders at the 2048-edge tile raster. No coverage/core shortcut
            // and no slider-magnitude proxy decides this projection.
            let saved_recipe = report.recipe.clone();
            let saved_notes = report.notes.clone();
            report.recipe.masks.truncate(first_tile);
            report.recipe.rationale.truncate(rationale_before_attach);
            report.notes.truncate(notes_before_attach);
            let (native_mask, native_components) = tile_geometry(reading.id, mask.dimensions());
            // R38: the contour in the PREVIEW's frame. The pixels this trial
            // is judged on come from `develop_preview`, which evaluates a
            // parametric geometry through `MaskFrame::downstream`; under a lens
            // profile that edge is not at the stored coordinates a stored-frame
            // contour would put the ruler's feet around
            // (`preview_mask_coverage`, and the pin
            // `the_native_trial_gate_reads_the_edge_the_preview_paints`).
            let native_coverage = render::preview_mask_coverage(&LocalAdjustment {
                mask: native_mask.clone(), components: native_components.clone(), ..Default::default()
            }, &s_img, &report.recipe);
            let weights: Vec<f32> = native_coverage.as_raw().iter().map(|a| *a as f32 / 255.0).collect();
            let native_attachment = tile_attachment(&reading, mask.dimensions(),
                reading.source_weights.clone(), reading.target_weights.clone(),
                ZoneCoverage { source: weights.clone(), target: weights.clone() });
            let mut native_frame = frame_before;
            let native = attach_one_zone(&s_img, &tgt_px, report, &mut native_frame,
                &native_attachment, reading.divergence, corr.as_ref());
            rendered_delta = values::NATIVE_TRIAL_GATED.to_string();
            let mut projection = None;
            if let Some(mut native) = native {
                let trial = enforce_bitmap_boundary(&s_img, &tgt_px, report, first_tile,
                    BitmapBoundaryInput {
                        ruler: BoundaryRuler::CrossBoundaryStep { geometry: &weights, reference: &current },
                        target_boundary: Some(&tgt_px[..]),
                        initial_px: std::mem::take(&mut native.rendered), frame_before,
                        frame_tol: SPATIAL_FRAME_REGRESSION_TOL,
                    });
                if let Ok(trial) = trial {
                    let raster_pixels = render::develop_preview(&guide, &saved_recipe).to_rgb8();
                    let native_pixels = render::develop_preview(&guide, &report.recipe).to_rgb8();
                    let delta = raster_pixels.as_raw().iter().zip(native_pixels.as_raw())
                        .map(|(a,b)| a.abs_diff(*b) as f32 / 255.0).fold(0.0, f32::max);
                    rendered_delta = format!("{delta:.6}");
                    // R36: fidelity to the target decides (see
                    // `native_carrier_fits`); the rendered change is a reading.
                    // Both residuals are read over the HARD cell — the one
                    // population both carriers claim — and both frames over
                    // the same frozen evidence.
                    let hard_target = zone_moments(&tgt_px, &reading.target_weights);
                    let raster_zone = zone_err(
                        &zone_moments(&boundary.pixels, &reading.source_weights),
                        &hard_target,
                    );
                    let native_zone = zone_err(
                        &zone_moments(&trial.pixels, &reading.source_weights),
                        &hard_target,
                    );
                    let raster_frame_after =
                        fit::look_err_with_evidence(&boundary.pixels, &tgt_px, &report.evidence);
                    let native_frame_after =
                        fit::look_err_with_evidence(&trial.pixels, &tgt_px, &report.evidence);
                    fidelity = format!("{raster_zone:.6}/{native_zone:.6}");
                    if native_carrier_fits(raster_zone, native_zone, raster_frame_after, native_frame_after) {
                        projection = Some((native, trial, native_attachment));
                    }
                }
            }
            if let Some((native, trial, native_attachment)) = projection {
                accepted = native;
                boundary = trial;
                accepted_coverage = native_attachment.coverage.as_ref().expect("tile coverage").source.clone();
                attachment = native_attachment;
                refined = false;
                crate::rationale::push_note(&mut report.recipe.rationale, &mut report.notes,
                    crate::rationale::Note::new(crate::rationale::keys::TILE_BOUNDARY_PASSED,
                        boundary_args(reading.id, boundary.k, boundary.initial, boundary.reading)));
            } else {
                report.recipe = saved_recipe;
                report.notes = saved_notes;
            }
        }
        if !refined { owned.remove(); }
        crate::rationale::push_note(
            &mut report.recipe.rationale,
            &mut report.notes,
            crate::rationale::Note::new(
                crate::rationale::keys::TILE_MASK_CARRIER,
                vec![
                    ("id", reading.id.tag()),
                    ("carrier", if refined { values::BITMAP_CARRIER } else { values::FOUR_GRADIENTS }.to_string()),
                    ("delta", format!("{refinement_delta:.6}")),
                    ("rendered", rendered_delta),
                    ("fidelity", fidelity),
                ],
            ),
        );
        let target_moments = zone_moments(&tgt_px, &attachment.target_weights);
        accepted.after = zone_err(
            &zone_moments(&boundary.pixels, &attachment.source_weights),
            &target_moments,
        );
        let frame_after =
            fit::look_err_with_evidence(&boundary.pixels, &tgt_px, &report.evidence);
        // R39. The tile that ships is the SHRUNK one, and it must still leave
        // its cell no worse than the frame it was attached to
        // (`AcceptedZone::still_accepted`; `judged_before` IS that frame's
        // reading here, the tiles being sequential): a correction the
        // boundary gate has negotiated down to a fraction of itself can fail
        // to help its own cell while the frame reading drifts a hair the
        // right way.
        if !accepted.still_accepted(
            &boundary.pixels, &tgt_px, accepted.judged_before, frame_before, frame_after,
        ) {
            crate::rationale::push_note(
                &mut report.recipe.rationale,
                &mut report.notes,
                accepted.shrunk_refusal_note(
                    &boundary.pixels, &tgt_px, accepted.judged_before, boundary.k, frame_before, frame_after,
                ),
            );
            report.recipe.masks.truncate(first_tile);
            owned.remove();
            refused.insert(reading.id);
            continue;
        }
        report.err_after = frame_after;
        crate::rationale::push_note(
            &mut report.recipe.rationale,
            &mut report.notes,
            crate::rationale::Note::new(
                crate::rationale::keys::TILE_ATTACHED,
                vec![
                    ("id", reading.id.tag()),
                    ("before", format!("{:.5}", accepted.before)),
                    ("after", format!("{:.5}", accepted.after)),
                    ("frame_before", format!("{frame_before:.5}")),
                    ("frame_after", format!("{frame_after:.5}")),
                    ("boundary", format!("{:.5}", boundary.reading.rim)),
                ],
            ),
        );
        let _path = owned.into_path();
        // The SHRUNK alpha, not the raw raster. What this vector withholds from
        // the free-mask producer is the correction a tile actually delivers, and
        // the boundary gate may have negotiated that correction down to a
        // fraction `k` of itself (the calibration island's accepted tiles kept
        // k = 0.114 / 0.168 / 0.187 under the pre-R40 ruler; R40 charges each
        // crossing by its discontinuity, so the shrink it finds is its own). A
        // tile shrunk to a ninth of its fitted
        // strength leaves most of its residual on the frame, so blocking the
        // next producer with the full alpha hid work nobody had done.
        for (dst, alpha) in excluded.iter_mut().zip(accepted_coverage) {
            *dst = dst.max(alpha * boundary.k);
        }
        attached.insert(reading.id);
        generation += 1;
    }
    crate::rationale::push_note(
        &mut report.recipe.rationale,
        &mut report.notes,
        crate::rationale::Note::new(
            crate::rationale::keys::TILE_DEPTH_CAP,
            vec![
                ("depth", SPATIAL_MAX_DEPTH.to_string()),
                ("cap", cap.to_string()),
                ("attached", attached.len().to_string()),
            ],
        ),
    );
    report.correspondence = corr;
    let final_px = fit::pixels_of(&render::develop_preview(&s_img, &report.recipe));
    report.err_after = fit::look_err_with_evidence(&final_px, &tgt_px, &report.evidence);
    fit::append_finished_disclosure(report, &final_px, &tgt_px);
    excluded
}
