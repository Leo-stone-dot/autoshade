//! Multi-class semantic region discovery for the zoned reverse-fit.
//!
//! The sidecar returns soft ADE20K class planes for each frame.  This module
//! pairs those planes, applies the shared support floor, and turns them into a
//! deterministic disjoint partition.  It deliberately contains no fitting
//! policy: the caller can feed each resulting region to the existing generic
//! `ZoneAttachment` estimator.

use image::GrayImage;

use super::MIN_ZONE_SHARE;
use crate::render::MASK_RASTER_BUDGET_BYTES;

/// The default maximum number of accepted semantic regions.
pub const MAX_SEMANTIC_REGIONS: usize = 4;
pub const DEFAULT_SEMANTIC_REGIONS: usize = 2;

/// One soft semantic class plane from one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct ClassPlane {
    pub class_id: u16,
    pub label: String,
    pub mean_confidence: f32,
    pub mask: GrayImage,
}

/// A paired source/target semantic region after overlap resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticRegion {
    pub class_id: u16,
    pub label: String,
    pub mean_confidence: f32,
    pub source: GrayImage,
    pub target: GrayImage,
    pub source_share: f32,
    pub target_share: f32,
}

fn weights(mask: &GrayImage) -> Vec<f32> {
    mask.pixels().map(|p| p.0[0] as f32 / 255.0).collect()
}

fn share(mask: &GrayImage) -> f32 {
    let n = (mask.width() as usize).saturating_mul(mask.height() as usize).max(1);
    weights(mask).into_iter().sum::<f32>() / n as f32
}

fn priority(a: &SemanticRegion, b: &SemanticRegion) -> std::cmp::Ordering {
    b.mean_confidence
        .partial_cmp(&a.mean_confidence)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| {
            let aa = a.source_share + a.target_share;
            let bb = b.source_share + b.target_share;
            aa.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
        })
        .then_with(|| a.class_id.cmp(&b.class_id))
}

fn subtract(mask: &GrayImage, claimed: &[f32]) -> GrayImage {
    let mut out = mask.clone();
    for (i, p) in out.pixels_mut().enumerate() {
        let v = p.0[0] as f32 / 255.0;
        let remainder = (v - claimed.get(i).copied().unwrap_or(0.0)).clamp(0.0, 1.0);
        p.0[0] = (remainder * 255.0).round() as u8;
    }
    out
}

/// Pair class planes, reject unsupported classes, resolve overlaps, and return
/// at most `max_regions` disjoint regions in ascending class-id order.
///
/// Priority is higher confidence, then smaller two-sided area, then lower
/// class id.  Processing by that priority means a specific/high-confidence
/// child owns overlapping pixels and a broad parent receives only its
/// complement.  A class missing on either side is never retained.
pub fn resolve_regions(
    source: &[ClassPlane],
    target: &[ClassPlane],
    max_regions: usize,
) -> Vec<SemanticRegion> {
    if max_regions == 0 {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    for s in source {
        let Some(t) = target.iter().find(|t| t.class_id == s.class_id) else { continue };
        if s.mask.dimensions() != t.mask.dimensions() {
            continue;
        }
        let ss = share(&s.mask);
        let ts = share(&t.mask);
        if ss < MIN_ZONE_SHARE || ts < MIN_ZONE_SHARE {
            continue;
        }
        let confidence = s.mean_confidence.min(t.mean_confidence);
        if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
            continue;
        }
        candidates.push(SemanticRegion {
            class_id: s.class_id,
            label: if s.label.is_empty() { t.label.clone() } else { s.label.clone() },
            mean_confidence: confidence,
            source: s.mask.clone(),
            target: t.mask.clone(),
            source_share: ss,
            target_share: ts,
        });
    }
    candidates.sort_by(priority);
    let (w, h) = candidates
        .first()
        .map(|r| r.source.dimensions())
        .unwrap_or((0, 0));
    let len = (w as usize).saturating_mul(h as usize);
    let mut claimed_source = vec![0.0f32; len];
    let mut claimed_target = vec![0.0f32; len];
    let mut accepted = Vec::new();
    for mut region in candidates {
        // One partition, one geometry: the claimed planes are sized from the
        // first candidate, and a plane of another size shares no pixel `i`
        // with them — it cannot take part in the overlap resolution, and
        // accumulating it into the claimed planes would index past them.
        if region.source.dimensions() != (w, h) {
            continue;
        }
        let source = subtract(&region.source, &claimed_source);
        let target = subtract(&region.target, &claimed_target);
        let ss = share(&source);
        let ts = share(&target);
        if ss < MIN_ZONE_SHARE || ts < MIN_ZONE_SHARE {
            continue;
        }
        for (i, p) in source.pixels().enumerate() {
            claimed_source[i] = (claimed_source[i] + p.0[0] as f32 / 255.0).min(1.0);
        }
        for (i, p) in target.pixels().enumerate() {
            claimed_target[i] = (claimed_target[i] + p.0[0] as f32 / 255.0).min(1.0);
        }
        region.source = source;
        region.target = target;
        region.source_share = ss;
        region.target_share = ts;
        accepted.push(region);
        if accepted.len() == max_regions.min(MAX_SEMANTIC_REGIONS) {
            break;
        }
    }
    accepted.sort_by_key(|r| r.class_id);
    accepted
}

/// Return whether adding `regions` rasters of `width` x `height` stays inside
/// the renderer's existing bitmap budget.  The accounting matches the render
/// engine's four-byte-per-texel reservation.
pub fn bitmap_budget_allows(
    existing_bytes: usize,
    width: u32,
    height: u32,
    regions: usize,
) -> bool {
    existing_bytes.saturating_add(
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
            .saturating_mul(regions),
    ) <= MASK_RASTER_BUDGET_BYTES
}

/// Confidence aggregation is deliberately worst-case across accepted regions.
pub fn worst_region_residual(residuals: &[f32]) -> f32 {
    residuals.iter().copied().fold(0.0, f32::max)
}
