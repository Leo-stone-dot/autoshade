//! Eval harness — how close is the AI's edit to the user's own?
//!
//! For RAWs that have a sibling `.xmp` (the user's ACR/Lightroom develop
//! settings = ground truth), run the AI advisor and compare global slider
//! values. Reports per-field **mean absolute error** (how far off) and **mean
//! signed bias** AI−user (which direction the AI leans). That bias is the
//! tuning signal: e.g. "AI contrast runs +8 hotter than you" → nudge the prompt.
//!
//! XMP is parsed by plain text scan of `crs:Key="value"` (the values are
//! attributes on rdf:Description; verified against the user's real P28.xmp).
//! No exiftool needed.
//!
//! **The ruler widened in R23-1** (feedback #12): the comparison table is now
//! DERIVED from `advisor::catalogue::RECIPE_CONTROLS` instead of being a
//! hand-kept list of 14 sliders, and it covers the 24 HSL cells, the 14
//! colour-grade fields, all four tone curves and the local-mask counts —
//! controls the old table was completely blind to (a `grep -c 'hsl'` over this
//! file used to return 0), which is why nothing could measure whether the AI
//! ever used them. A gap score from before that change is NOT comparable with
//! one from after it: more controls are measured, and a control the user moved
//! while the AI ignored it now counts as the miss it always was.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// `Context` is gone since R27 Batch-7: the one `?` in `run` (an unreadable
// sidecar) cannot propagate out of a worker closure any more, so that failure
// travels back as `PhotoOutcome::Aborted` carrying the SAME message the
// `with_context` produced — `read_sidecar` returns an `Option`, so the context
// string WAS the whole error and nothing is lost by formatting it directly.
use anyhow::Result;

use crate::advisor::catalogue::{self, Shape, COLOR_GRADE_CRS, RECIPE_CONTROLS};
use crate::config::Config;
use crate::pipeline;
use crate::recipe::EditRecipe;

// The `crs:` readers live beside the XMP writer they invert (xmp.rs, where the
// sidecar READER also uses them). Since R28 Batch-5 5d the scope is a TYPE:
// every read in this harness is against a `Scope` — a whole sidecar, narrowed
// to the crs Description's own span — which is what it always meant and now
// states. `style.rs` imports the same two names from `xmp` directly.
pub(crate) use crate::xmp::{CrsSource, Scope};

/// The provenance rule a row's USER value follows.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Rule {
    /// An ordinary slider: present = the photographer set it.
    Plain,
    /// White balance: under `WhiteBalance="As Shot"` the Temperature/Tint
    /// attributes record the CAMERA's values, not a user edit — counting them
    /// charged the AI a false "omission" for correctly leaving WB as-shot.
    Wb,
    /// `crs:CropAngle` is applied by Adobe only under `HasCrop="True"`, so a
    /// stale angle from a disabled crop is not a straighten the user asked for
    /// (the same gate `xmp::xmp_to_recipe` applies on import).
    CropGated,
}

/// Which recipe value a row reads.
#[derive(Debug, Clone, Copy)]
enum Source {
    /// A top-level registry field, by the row's own `metric` name.
    Field,
    /// One cell of the 8-band mixer.
    HslCell { axis: usize, band: usize },
    /// One colour-grade wheel field.
    Grade(&'static str),
}

/// One comparable control: what to call it, which ACR attribute carries the
/// user's own value, and how to read the AI's side. Built by [`rows`] from the
/// control registry, so a control that becomes AI-visible later enters the
/// ruler without a second edit here.
///
/// There is NO unit conversion left on this struct. It carried a per-row
/// `scale` for exactly one control — `sharpening`, on the belief that
/// `crs:Sharpness` was ACR's 0..100 against the recipe's 0..150 — and that
/// belief was disproved in v0.31.1 (real sidecars carry `Sharpness="150"`; see
/// `xmp::xmp_to_recipe`). Every row is now 1:1 by construction, so the field is
/// gone rather than left as a column of 1.0s that invites the next scale guess.
#[derive(Debug, Clone)]
struct Row {
    /// Report label: a registry field name, or `hsl.<axis>.<band>` /
    /// `color_grade.<field>` for a family cell.
    metric: String,
    /// The `crs:` attribute the user's value is read from.
    crs: String,
    rule: Rule,
    source: Source,
    /// True for the four `*_hue` wheels: hue is CIRCULAR, so 350° vs 10° is a
    /// 20° disagreement, not 340°.
    circular: bool,
}

/// Rows below this sample size are visibly marked in the report. The paired
/// same-version analysis found n<20 rows 8.48x less stable per row than
/// n>=20 rows, so this is an annotation threshold, not a score threshold.
const LOW_N_ANNOTATION_THRESHOLD: usize = 20;

impl Row {
    /// Full scale this control's MAE is normalised by for the gap score. Kept
    /// as the pre-R23 normaliser (a "typical" full range, not the clamp band)
    /// so the aggregate stays comparable control-for-control; hue uses its
    /// circular half-turn.
    fn full_scale(&self) -> f64 {
        match self.metric.as_str() {
            _ if self.circular => 180.0,
            "exposure_ev" => 5.0,
            "sharpening" => 150.0,
            "temperature_k" => 2000.0,
            _ => 100.0,
        }
    }

    /// Deadband for "did anyone actually move this?" — exposure is in stops.
    fn eps(&self) -> f32 {
        eps_for(&self.metric)
    }

    /// AI − user, wrapped into ±180 for the circular hue wheels.
    fn diff(&self, ai: f32, user: f32) -> f32 {
        let d = ai - user;
        if self.circular {
            let w = d.rem_euclid(360.0);
            if w > 180.0 { w - 360.0 } else { w }
        } else {
            d
        }
    }
}

/// The ruler's "did anyone actually move this?" deadband, by metric name —
/// lifted out of [`Row::eps`] so a row that has to ask about ANOTHER control's
/// movement ([`hue_carries_colour`]) asks the same question with the same
/// number instead of restating it.
fn eps_for(metric: &str) -> f32 {
    if metric == "exposure_ev" { 0.05 } else { 0.5 }
}

/// Does a colour-grade HUE row describe colour that BOTH sides actually
/// applied? (R28 Batch-5 5b.)
///
/// A toning wheel's hue is an ANGLE, and the wheel paints nothing until its
/// SATURATION leaves zero — `render::apply_color_grade` multiplies the tint by
/// `sat/100`, so at zero saturation every hue from 0° to 359° renders the
/// identical (untinted) pixel. The old row counted a photo whenever EITHER side
/// moved the hue, saturation unasked: on the 147-photo baseline that made
/// `color_grade.shadow_hue` read `mean|Δ| = 141`, a number with no colour
/// anywhere behind it — two colourless wheels parked at opposite ends of a
/// circle. The real finding the artifact was burying is one row down and stands
/// unchanged: `shadow_sat` bias **+9.02**, i.e. the AI tints shadows the
/// photographer leaves neutral.
///
/// So a hue delta is measured only where both sides put colour on the wheel.
/// The threshold is not a new constant: it is the ruler's OWN movement deadband
/// ([`eps_for`], 0.5 on the wheels' 0..100 saturation axis) applied to the
/// companion `*_sat` control — "counted when both sides moved the saturation",
/// in exactly the sense this harness already means by "moved".
///
/// **What this costs, stated:** a photo where the photographer toned and the AI
/// did not no longer reaches the hue row at all, so that row's `AI-omit` count
/// falls. The omission is not lost — it is recorded where it is measurable, on
/// the `*_sat` row, which is the control that carries the decision. And the
/// four `*_hue` rows are NOT comparable across v0.34.0: the pre-R28 M-C
/// baseline's `shadow_hue` numbers count a different population.
///
/// Non-hue rows (every HSL cell included — those are ±100 shift sliders, not
/// wheel angles, and `Row::circular` is false for them) return `true` and are
/// untouched.
fn hue_carries_colour(row: &Row, scope: Scope<'_>, ai: &EditRecipe) -> bool {
    let Some(stem) = row
        .metric
        .strip_prefix("color_grade.")
        .and_then(|f| f.strip_suffix("_hue"))
    else {
        return true;
    };
    let sat_field = format!("{stem}_sat");
    // Derived from the registry, like every other reader here: a new wheel
    // inherits this gate instead of needing a second edit.
    let Some((_, sat_attr)) = COLOR_GRADE_CRS.iter().find(|(f, _)| *f == sat_field) else {
        return true;
    };
    let neutral = catalogue::color_grade_value(&Default::default(), &sat_field).unwrap_or(0.0);
    let eps = eps_for(&format!("color_grade.{sat_field}"));
    let moved = |v: Option<f32>| v.is_some_and(|v| (v - neutral).abs() > eps);
    // The user's side is read the same way `user_value` reads a `Rule::Plain`
    // row (an absent attribute = the neutral = not moved), and the AI's the
    // same way `ai_value` reads a `Source::Grade` row.
    moved(scope.crs_f32(sat_attr))
        && moved(catalogue::color_grade_value(&ai.color_grade, &sat_field))
}

/// The comparison table, derived from the control registry: every AI-visible
/// control that lands on ONE `crs:` attribute becomes a row, and the two family
/// shapes expand into their per-band / per-wheel cells.
///
/// Engine-only rows are excluded on purpose — the AI cannot set them, so
/// scoring it against them would measure a schema gap, not a taste gap. The
/// lens trio entered the schema in R23-1b and therefore entered the ruler in
/// the same edit, exactly as this note promised: three more rows
/// (`VignetteAmount` / `VignetteMidpoint` / `LensManualDistortionAmount`),
/// derived, with no hand-kept list to update.
fn rows() -> Vec<Row> {
    let mut out = Vec::new();
    for c in RECIPE_CONTROLS.iter().filter(|c| !c.engine_only) {
        // Scalar rows only: the Black & White switch (2026-10-21) is the
        // first AI-visible boolean with an attribute, and a ruler row needs a
        // number on both sides.
        if let Some(attr) = c.crs.attr()
            && c.shape.is_scalar()
        {
            out.push(Row {
                metric: c.name.to_string(),
                crs: attr.to_string(),
                rule: match c.name {
                    "temperature_k" | "tint" => Rule::Wb,
                    "straighten_deg" => Rule::CropGated,
                    _ => Rule::Plain,
                },
                source: Source::Field,
                circular: false,
            });
        }
        match c.shape {
            Shape::Hsl => {
                for f in catalogue::hsl_expansion() {
                    out.push(Row {
                        metric: f.metric,
                        crs: f.crs,
                        rule: Rule::Plain,
                        source: Source::HslCell { axis: f.axis, band: f.band },
                        circular: false,
                    });
                }
            }
            Shape::ColorGrade => {
                for (field, attr) in COLOR_GRADE_CRS {
                    out.push(Row {
                        metric: format!("color_grade.{field}"),
                        crs: attr.to_string(),
                        rule: Rule::Plain,
                        source: Source::Grade(field),
                        circular: field.ends_with("_hue"),
                    });
                }
            }
            _ => {}
        }
    }
    out
}

/// The AI recipe's value for a row (`None` only for a row whose reader is
/// missing — the registry test makes that unreachable).
fn ai_value(row: &Row, r: &EditRecipe) -> Option<f32> {
    match row.source {
        Source::Field => match row.metric.as_str() {
            // ACR records an ABSOLUTE Temperature and Tint, so compare
            // absolutes: As Shot resolves through the recipe's own stamp (the
            // engine's historical 5500 K fallback), and the engine's Tint is
            // a DELTA from the as-shot green.
            "temperature_k" => Some(crate::render::WbLaw::absolute_point_of(r).0),
            "tint" => Some(crate::render::WbLaw::absolute_point_of(r).1),
            f => catalogue::global_value(r, f).and_then(|v| v.scalar()),
        },
        Source::HslCell { axis, band } => catalogue::hsl_value(&r.hsl, axis, band),
        Source::Grade(f) => catalogue::color_grade_value(&r.color_grade, f),
    }
}

/// The row's NEUTRAL value — READ from the defaults, never assumed to be 0:
/// `color_grade.blending` is neutral at 50, and the WB pair's neutral is the
/// photo's own as-shot anchor. (A hard-coded 0 counted every untouched
/// blending as a ±50 disagreement.)
fn neutral(row: &Row, r: &EditRecipe) -> f32 {
    match row.metric.as_str() {
        "temperature_k" => r.as_shot_k.unwrap_or(5500.0),
        "tint" => r.as_shot_tint.unwrap_or(0.0),
        _ => ai_value(row, &EditRecipe::default()).unwrap_or(0.0),
    }
}

/// The user's value for one row, or `None` when this sidecar does not state it.
/// Read STRAIGHT: every `crs:` attribute on the ruler carries the same number
/// the recipe field does (see [`Row`] for the one exception that used to exist).
fn user_value(row: &Row, scope: Scope<'_>, user_wb: bool, has_crop: bool) -> Option<f32> {
    match row.rule {
        Rule::Wb if !user_wb => None,
        Rule::CropGated if !has_crop => None,
        _ => scope.crs_f32(&row.crs),
    }
}

/// Parse an ACR tone-curve `<rdf:Seq>` of `<rdf:li>x, y</rdf:li>` points (each
/// 0..255 input,output) for the given crs tag (e.g. "ToneCurvePV2012"). Empty
/// vec if the tag is absent. The master tone curve is the single biggest "look"
/// control that the flat-slider comparison above was completely blind to.
fn parse_tone_curve(xmp: &str, tag: &str) -> Vec<(f32, f32)> {
    // The same name-matched finder the importer uses (xmp::owned_element_body):
    // the literal `<crs:Tag>` scan this replaced could not see the
    // attribute-carrying spelling, so the judge silently understated the gap
    // score on documents the importer now reads.
    let Ok(Some(body)) = crate::xmp::owned_element_body(xmp, &format!("crs:{tag}")) else {
        return Vec::new();
    };
    let mut pts = Vec::new();
    for chunk in body.split("<rdf:li>").skip(1) {
        let Some(end) = chunk.find("</rdf:li>") else { continue };
        let mut it = chunk[..end].split(',');
        if let (Some(xs), Some(ys)) = (it.next(), it.next())
            && let (Ok(x), Ok(y)) = (xs.trim().parse::<f32>(), ys.trim().parse::<f32>())
            // Same domain xmp::parse_curve enforces by clamping to u8: a
            // NaN/inf/out-of-range point from a foreign sidecar would poison
            // the LUT and print a NaN gap score.
            && x.is_finite()
            && y.is_finite()
            && (0.0..=255.0).contains(&x)
            && (0.0..=255.0).contains(&y)
        {
            pts.push((x, y));
        }
    }
    pts
}

/// The AI recipe's master tone curve as the same (input,output) point list.
fn ai_tone_curve_points(r: &EditRecipe) -> Vec<(f32, f32)> {
    r.tone_curve.iter().map(|p| (p.input as f32, p.output as f32)).collect()
}

/// Build a 256-entry [0..255]→[0..255] LUT from tone-curve control points
/// (piecewise-linear). Identity when empty. Endpoints are pinned to
/// (0,0)/(255,255) unless the curve places its own point at an end — the same
/// rule as `render::curve_lut`, so the judge scores the curve the engine
/// actually renders (an unpinned copy would report e.g. a black lift the
/// pinned render no longer produces).
fn curve_lut(points: &[(f32, f32)]) -> [f32; 256] {
    let mut lut = [0f32; 256];
    if points.is_empty() {
        for (i, v) in lut.iter_mut().enumerate() {
            *v = i as f32;
        }
        return lut;
    }
    let mut pts = points.to_vec();
    pts.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    // Duplicate inputs: FIRST wins — the same rule as render::curve_lut, so
    // the judge scores the curve the engine actually renders (interpolating
    // across a duplicated input stepped at a different height).
    pts.dedup_by(|b, a| (b.0 - a.0).abs() < 1e-6);
    if pts[0].0 > 0.0 {
        pts.insert(0, (0.0, 0.0));
    }
    if pts[pts.len() - 1].0 < 255.0 {
        pts.push((255.0, 255.0));
    }
    for (i, v) in lut.iter_mut().enumerate() {
        *v = interp255(&pts, i as f32);
    }
    lut
}

fn interp255(pts: &[(f32, f32)], x: f32) -> f32 {
    if x <= pts[0].0 {
        return pts[0].1;
    }
    let last = pts[pts.len() - 1];
    if x >= last.0 {
        return last.1;
    }
    for w in pts.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x >= x0 && x <= x1 {
            let t = if (x1 - x0).abs() < 1e-6 { 0.0 } else { (x - x0) / (x1 - x0) };
            return y0 + (y1 - y0) * t;
        }
    }
    x
}

/// How much a curve lifts the black point (output at input 0; 0 = pinned black).
fn curve_black_lift(lut: &[f32; 256]) -> f32 {
    lut[0]
}

/// S-curve strength: how much the curve brightens the quarter-highlight (input
/// 191) AND darkens the quarter-shadow (input 64) relative to identity. >0 adds
/// contrast (an S); <0 flattens; ~0 is identity/linear.
fn curve_s_strength(lut: &[f32; 256]) -> f32 {
    (lut[191] - 191.0) - (lut[64] - 64.0)
}

/// RMS difference between two 0..255 LUTs (same scale as the curve values).
fn curve_rmse(a: &[f32; 256], b: &[f32; 256]) -> f64 {
    let mut s = 0f64;
    for i in 0..256 {
        let d = (a[i] - b[i]) as f64;
        s += d * d;
    }
    (s / 256.0).sqrt()
}


#[derive(Debug, Default, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct Acc {
    sum_abs: f64,
    sum_signed: f64,
    n: u32,
    /// Times the user used this control but the AI left it neutral/omitted it —
    /// a real miss the old both-set gate dropped silently.
    omit: u32,
}

impl Acc {
    /// Fold one photograph's contribution in. Split out of the loop by R27
    /// Batch-7 so the per-photo work can run on a worker while the SUMS are
    /// still added in photo order — `f64` addition is not associative, so
    /// folding in completion order would move the reported gap score in the
    /// last ulp depending on which photo happened to finish first.
    fn merge(&mut self, other: &Acc) {
        self.sum_abs += other.sum_abs;
        self.sum_signed += other.sum_signed;
        self.n += other.n;
        self.omit += other.omit;
    }
}

fn low_n_marker(n: u32) -> &'static str {
    if (n as usize) < LOW_N_ANNOTATION_THRESHOLD {
        " [low n]"
    } else {
        ""
    }
}

/// Preserve the established scalar table formatting, adding only the new
/// low-sample marker to rows that need it.
fn format_scalar_report_line(metric: &str, a: &Acc) -> String {
    let label = format!("{metric}{}", low_n_marker(a.n));
    if a.n > 0 {
        format!(
            "{label:<22} {:>4} {:>10.2} {:>+13.2} {:>8}\n",
            a.n,
            a.sum_abs / a.n as f64,
            a.sum_signed / a.n as f64,
            a.omit
        )
    } else {
        format!("{label:<22} {:>4} {:>10} {:>13} {:>8}\n", a.n, "—", "—", a.omit)
    }
}

fn format_scalar_report_rows(ruler: &[Row], acc: &BTreeMap<String, Acc>) -> String {
    let mut out = format!(
        "Legend: [low n] marks rows with n < {LOW_N_ANNOTATION_THRESHOLD}; treat their per-control means as less stable.\n"
    );
    for row in ruler {
        let Some(a) = acc.get(&row.metric) else { continue };
        if a.n == 0 && a.omit == 0 {
            continue;
        }
        out.push_str(&format_scalar_report_line(&row.metric, a));
    }
    out
}

fn format_supplementary_gap_line(weighted: f64) -> String {
    format!(
        "Supplementary n-weighted gap score: {weighted:.1}%  (scalar block redistributed by n; term structure unchanged)\n"
    )
}

fn n_weighted_scalar_mean(rows: &[(f64, u32)]) -> Option<f64> {
    let total_n: u64 = rows.iter().map(|(_, n)| u64::from(*n)).sum();
    (total_n > 0).then(|| {
        rows.iter()
            .map(|(value, n)| *value * f64::from(*n) / total_n as f64)
            .sum()
    })
}

/// What became of one photograph. The three non-`Stats` arms exist because the
/// serial loop had three distinct exits (`continue` on a failed analysis, `?` on
/// an unreadable sidecar, and — new — "an earlier photo already aborted the
/// run"), and a pool cannot `?` out of a worker: the decision has to travel back
/// as a value and be acted on in index order.
enum PhotoOutcome {
    Stats(Box<PhotoStats>),
    /// A saved row already answers for this photograph, so nothing was
    /// decoded, asked or billed for it — its stats are folded from the state
    /// file at the same index, through the same [`Acc::merge`].
    Restored,
    /// `produce_recipe` failed; the FAILED line is already in this photo's
    /// block and the run continues, exactly as the serial `continue` did.
    Failed,
    /// The hard error the serial loop raised with `?`, carried out to be
    /// re-raised after the pool drains.
    Aborted(String),
    /// Dequeued after another photo aborted — never analyzed, never billed.
    Skipped,
}

/// Emit one output fragment.
///
/// At one job the fragment goes STRAIGHT to stdout and is flushed, so the
/// `[i/n] stem ... ` prefix still appears before the minute of network work the
/// photo is about to spend — the serial harness's progress indicator, kept
/// byte-for-byte. Above one job nothing may print from a worker: the fragment
/// joins the photo's block and the sequencer releases it in index order.
fn emit(live: bool, block: &mut String, text: &str) {
    if live {
        use std::io::Write;
        print!("{text}");
        let _ = std::io::stdout().flush();
    } else {
        block.push_str(text);
    }
}

/// Everything ONE photograph contributes to the report — produced on a worker,
/// folded on the main thread in index order (see [`Acc::merge`]).
///
/// Serialisable since the resume work below: this struct IS the state file's
/// payload, so a photograph measured in one run folds into another run's
/// report through the same [`Acc::merge`] it would have taken fresh. NO
/// `#[serde(default)]` on purpose — a truncated last line (the crash this
/// whole mechanism exists for) must fail to parse and be re-measured, not
/// deserialise into an all-zero contribution that claims the photograph was
/// measured.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct PhotoStats {
    /// This photo's per-control contributions, keyed exactly like the report.
    acc: BTreeMap<String, Acc>,
    /// `(rmse, user_lift, ai_lift, user_s, ai_s)` when either side drew a
    /// master curve.
    curve: Option<(f64, f64, f64, f64, f64)>,
    /// Per-channel curve RMSE, entered only where either side drew one.
    rgb: [Option<f64>; 3],
    /// `(imported, our importer refuses, AI proposed)` when any side had one.
    masks: Option<(usize, usize, usize)>,
}

// ── resumable progress ──────────────────────────────────────────────────────
//
// Two 147-photo PAID runs were thrown away whole (2026-08-20) because ONE
// upstream stream error poisoned each of them, and the transcript held no
// per-photo numbers to salvage: the report table below prints once, at the end,
// so a run that does not reach the end measured nothing anybody can use. This
// is the second arm of that one root cause — "a single mid-run failure forfeits
// the whole run"; the first is the transport retry in `advisor`.
//
// Every photograph whose measurement COMPLETED and came from the vision
// proposer is appended here as it lands, and a rerun folds those rows back in
// instead of re-buying them. The rows are the same `PhotoStats` the pool
// produces and take the same fold at the same index, so the arithmetic does not
// depend on where a run was interrupted.

/// The state file's FIRST line: what the rows below it were measured under.
///
/// Deliberately narrow, and deliberately not a machine description. A run is
/// resumable only against the same build and the same two models, so those are
/// what is recorded and compared. NO endpoint URL, NO key, and no path but the
/// folder the user themselves typed — this file lives in the per-user store,
/// but it is still a file a user may hand to somebody else when reporting a bad
/// baseline.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StateHeader {
    autoshade_version: String,
    /// The vision proposer's model id (`Config::openai_model`).
    proposer_model: String,
    /// The verifier's model id (`Config::analysis_model`).
    analysis_model: String,
    limit: usize,
    /// The folder AS THE USER GAVE IT.
    dir: String,
    /// Unix seconds. Provenance only — never compared.
    started: u64,
}

/// One measured photograph, as one line of the state file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StateRow {
    /// Readability of the file itself; `rel` is what a resume matches on.
    stem: String,
    /// The photograph's path RELATIVE to the eval folder, which is what
    /// actually identifies it: `pipeline::find_raws` RECURSES, so two shoot
    /// folders under one library can each hold a `DSC00001`, and a stem-keyed
    /// row would answer for the wrong photograph. Falls back to the stem when
    /// the path cannot be expressed under the folder — never an absolute path,
    /// which would put a machine path into a file the user may share.
    rel: String,
    /// SHA-256 of the sidecar this measurement was taken against: the
    /// photographer re-editing that photo invalidates the row.
    xmp_sha256: String,
    stats: PhotoStats,
}
/// SHA-256, lowercase hex — the shared implementation.
///
/// This module and `describe` each wrote their own until the two were folded
/// into [`crate::sha256`]: same digest, one known-answer test, one place a
/// mistyped constant can hide. The digest names a state file and decides
/// whether a saved measurement still describes the sidecar it was taken from,
/// so it must stay a fixed function of the bytes for ever.
use crate::sha256::sha256_hex;

/// A photograph's identity INSIDE the eval folder — see [`StateRow::rel`].
fn photo_rel(dir: &Path, raw: &Path) -> String {
    raw.strip_prefix(dir)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| pipeline::stem(raw).to_string())
}

/// Where a run's progress lives when the caller names no path.
///
/// Under the per-user store ([`crate::store::store_root`]), NEVER beside the
/// photographs: the library is read-only by settled project law. The name is
/// derived from the two things that decide the work list — the folder and
/// `--limit` — so re-running the same command finds its own file and no other
/// run's. Windows spellings fold case, the rule `store::photo_key` already
/// follows, so `<drive>\pics` and `<drive>\Pics` are one run.
fn default_state_path(dir: &Path, limit: usize) -> PathBuf {
    let abs = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    let spelling = abs.to_string_lossy().into_owned();
    let spelling = if cfg!(windows) { spelling.to_lowercase() } else { spelling };
    let key = sha256_hex(format!("{spelling}|{limit}").as_bytes());
    crate::store::store_root().join("eval-state").join(format!("{}.jsonl", &key[..16]))
}

/// The saved rows at `path`, or an empty vec when there is nothing there.
///
/// `Err` is a REFUSAL, not a failure to read: a file whose header describes a
/// different build or different models must not be folded into this run's
/// table, because one table measured by two different things is not a
/// measurement. Both sides are printed so the reader can see WHICH moved.
fn load_state(path: &Path, want: &StateHeader) -> Result<Vec<StateRow>> {
    let Ok(text) = std::fs::read_to_string(path) else { return Ok(Vec::new()) };
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut lines = text.lines();
    let Some(Ok(head)) = lines.next().map(serde_json::from_str::<StateHeader>) else {
        anyhow::bail!(
            "the saved eval progress at {} does not begin with a readable header — re-run with \
             --fresh to discard it, or --state <path> to write this run somewhere else",
            path.display()
        );
    };
    if head.autoshade_version != want.autoshade_version
        || head.proposer_model != want.proposer_model
        || head.analysis_model != want.analysis_model
    {
        anyhow::bail!(
            "the saved eval progress at {p} was measured under autoshade {hv} (proposer {hp}, \
             analysis {ha}); this run is autoshade {wv} (proposer {wp}, analysis {wa}). Folding \
             them into one table would report a gap score measured by two different things — \
             re-run with --fresh to start over, or --state <path> to keep both.",
            p = path.display(),
            hv = head.autoshade_version,
            hp = head.proposer_model,
            ha = head.analysis_model,
            wv = want.autoshade_version,
            wp = want.proposer_model,
            wa = want.analysis_model,
        );
    }
    // A line that does not parse is a line that is not trusted — a crash
    // mid-write leaves exactly one of those at the end of the file, and the
    // photograph it half-described is simply measured again.
    Ok(lines.filter_map(|l| serde_json::from_str::<StateRow>(l).ok()).collect())
}

/// Which of `work`'s photographs the saved rows still answer for, by index,
/// plus the stems whose row was thrown away as stale.
///
/// A row is honoured only when it names this photograph AND the sidecar on disk
/// still hashes to what the measurement was taken against: the photographer
/// re-editing a photo invalidates the measurement OF that photo, and silently
/// reusing the row would score the AI against an edit that no longer exists.
/// The sidecar is read through the same capped reader [`run`] measures with
/// (`store::read_sidecar`), so the hash covers exactly the text that was read.
fn resume_plan(
    dir: &Path,
    work: &[(&Path, &Path)],
    rows: Vec<StateRow>,
) -> (Vec<Option<Box<PhotoStats>>>, Vec<String>) {
    // Last row wins: a photograph re-measured after a stale row was discarded
    // has both lines in the file, and the newer one is the live measurement.
    let mut by_rel: BTreeMap<String, StateRow> = BTreeMap::new();
    for row in rows {
        by_rel.insert(row.rel.clone(), row);
    }
    let mut restored = Vec::with_capacity(work.len());
    let mut stale = Vec::new();
    for (raw, sidecar) in work {
        let Some(row) = by_rel.remove(&photo_rel(dir, raw)) else {
            restored.push(None);
            continue;
        };
        let now = crate::store::read_sidecar(sidecar).map(|t| sha256_hex(t.as_bytes()));
        if now.as_deref() == Some(row.xmp_sha256.as_str()) {
            restored.push(Some(Box::new(row.stats)));
        } else {
            stale.push(row.stem.clone());
            restored.push(None);
        }
    }
    (restored, stale)
}

/// Appends one row per completed photograph, as it completes.
///
/// Flushed per row on purpose: the whole point is that a run killed by the next
/// upstream stream error keeps everything measured before it.
struct StateLog {
    path: PathBuf,
    /// `None` = writing is off, because it failed. A state file that cannot be
    /// written DEGRADES the run to un-resumable and says so; it does not fail a
    /// run whose expensive half is the API spend.
    sink: std::sync::Mutex<Option<std::fs::File>>,
}

impl StateLog {
    /// The file itself: appended to when this is a resume, created with its
    /// header line when it is not.
    fn open_sink(
        path: &Path,
        header: &StateHeader,
        resuming: bool,
    ) -> std::io::Result<std::fs::File> {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if resuming {
            return std::fs::OpenOptions::new().append(true).open(path);
        }
        let mut f = std::fs::File::create(path)?;
        let head = serde_json::to_string(header).map_err(std::io::Error::other)?;
        writeln!(f, "{head}")?;
        Ok(f)
    }

    /// Open the file, writing the header when this is not a resume.
    fn open(path: &Path, header: &StateHeader, resuming: bool) -> Self {
        let sink = match Self::open_sink(path, header, resuming) {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!(
                    "⚠ eval progress cannot be saved to {} ({e}) — this run still measures, but \
                     a rerun will have to re-buy every photograph.",
                    path.display()
                );
                None
            }
        };
        Self { path: path.to_path_buf(), sink: std::sync::Mutex::new(sink) }
    }

    fn record(&self, row: &StateRow) {
        use std::io::Write as _;
        let mut guard = self.sink.lock().unwrap_or_else(|p| p.into_inner());
        let Some(file) = guard.as_mut() else { return };
        let wrote = serde_json::to_string(row)
            .map_err(std::io::Error::other)
            .and_then(|line| writeln!(file, "{line}").and_then(|()| file.flush()));
        if let Err(e) = wrote {
            eprintln!(
                "⚠ eval progress stopped being saved to {} ({e}) — the rows already written are \
                 still good.",
                self.path.display()
            );
            *guard = None;
        }
    }
}

/// Did the VISION proposer answer for this photograph, or did the run fall back
/// to the deterministic baseline?
///
/// Read off the typed note channel `produce_recipe` already returns — no new
/// field, no new global, because the fact was already on the wire. The
/// heuristic stands in under exactly two keys: `HEURISTIC_UNAVAILABLE` (the
/// proposer WAS tried and failed — the mid-run stream error that cost two paid
/// 147-photo runs) and `HEURISTIC_NO_KEY` (no proposer was configured at all).
/// Both mean the numbers came from arithmetic over the histogram rather than
/// from the model this measurement is about, so neither may be saved as
/// progress: a rerun that folded them back in would print a baseline built
/// partly out of the fallback, which is the poisoning this file exists to
/// prevent.
///
/// The note cannot be lost on THIS path: it is pushed first, and the two places
/// that clear the note vec (an adopted verifier revision, an adopted visual
/// round) are both unreachable here — a fallback proposal sets
/// `can_revise = false`, and `eval` passes `judge = false`.
fn proposer_answered(notes: &[crate::rationale::Note]) -> bool {
    use crate::rationale::keys;
    !notes
        .iter()
        .any(|n| n.key == keys::HEURISTIC_UNAVAILABLE || n.key == keys::HEURISTIC_NO_KEY)
}

/// Append this photograph's row unless the proposer fell back. Returns whether
/// the row was persisted — the caller discloses the `false` case in the
/// transcript and in the report's provenance line.
fn record_measured(log: &StateLog, notes: &[crate::rationale::Note], row: &StateRow) -> bool {
    if !proposer_answered(notes) {
        return false;
    }
    log.record(row);
    true
}

/// Supplementary gap view: keep the headline's active-term structure and
/// redistribute only the scalar block by observed sample counts (n / total
/// scalar n). With equal n, this is exactly the headline over the same rows;
/// the curve terms and their one-term-per-curve normalization are unchanged.
/// This intentionally does not replace the headline or alter the state file.
fn supplementary_weighted_gap(
    ruler: &[Row],
    acc: &BTreeMap<String, Acc>,
    curve_n: u32,
    sum_curve_rmse: f64,
    rgb_acc: &[(u32, f64); 3],
) -> Option<f64> {
    let scalar: Vec<(f64, u32)> = ruler
        .iter()
        .filter_map(|row| {
            let a = acc.get(&row.metric)?;
            (a.n > 0).then(|| (((a.sum_abs / a.n as f64) / row.full_scale()), a.n))
        })
        .collect();
    let scalar_terms = scalar.len() as u32;
    let mut sum = if scalar_terms > 0 {
        f64::from(scalar_terms) * n_weighted_scalar_mean(&scalar).unwrap()
    } else {
        0.0
    };
    let mut terms = scalar_terms;
    if curve_n > 0 {
        sum += (sum_curve_rmse / curve_n as f64) / 255.0;
        terms += 1;
    }
    for (n, total) in rgb_acc {
        if *n > 0 {
            sum += (*total / *n as f64) / 255.0;
            terms += 1;
        }
    }
    (terms > 0).then(|| 100.0 * sum / terms as f64)
}

/// The pool body's FIRST decision: a photograph a valid saved row already
/// answers for is not measured — no decode, no API call, no spend.
///
/// Separated from the body so that promise is testable without a network: the
/// test counts how often `measure` runs.
fn restore_or_measure<M>(
    live: bool,
    block: &mut String,
    restored: bool,
    measure: M,
) -> PhotoOutcome
where
    M: FnOnce(&mut String) -> PhotoOutcome,
{
    if restored {
        emit(live, block, "from saved progress\n");
        return PhotoOutcome::Restored;
    }
    measure(block)
}

pub fn run(
    dir: &Path,
    xmp_dir: Option<&Path>,
    limit: usize,
    jobs: usize,
    fresh: bool,
    state: Option<&Path>,
) -> Result<()> {
    let cfg = Config::load();
    // R27 P1, deliberate NON-action: this stays RAW-only, and not for want of
    // a flag. `eval` scores the AI against the user's OWN edit, which it reads
    // from the sibling `.xmp`. For a baked photo that sidecar is off limits by
    // a settled, thrice-cited policy — `store::lightroom_sidecar` returns None
    // for non-RAW ("a baked PNG/TIFF's neighbouring `.xmp` is not ours to
    // interpret"), `pipeline`'s merge base skips it, and the GUI disables
    // "Export XMP beside photo". Including baked sources here would reopen
    // that decision through the back door, and would score the AI against a
    // sidecar written by some other program about some other image.
    let raws = pipeline::find_raws(dir)?;
    // The SAME pairing rule the style index builds with ([`crate::xmp_pair`]),
    // resolved once here: `run` and `resume_plan` must agree about which file a
    // measurement was taken against, and two derivations of one name is exactly
    // how they would stop agreeing.
    let pairing = crate::xmp_pair::XmpPairing::new(dir, xmp_dir);
    let pairs: Vec<(&Path, PathBuf)> = raws
        .iter()
        .filter_map(|r| pairing.find(r).map(|x| (r.as_path(), x)))
        .collect();
    println!(
        "found {} RAW(s); {} have an .xmp sidecar (your edits). Evaluating {}.",
        raws.len(),
        pairs.len(),
        pairs.len().min(limit)
    );
    if pairs.is_empty() {
        println!(
            "Nothing to evaluate — no .xmp sidecars were found beside these RAWs or under \
             --xmp-dir."
        );
        return Ok(());
    }

    // The ruler, in registry order (which is also the report order — the two
    // used to be separate hand-kept lists with a comment claiming they matched).
    let ruler = rows();
    // The work list is fixed up front — exactly the first `limit` pairs the old
    // sequential `.take(limit)` selected — then a bounded, memory-budgeted pool
    // works through it (R27 Batch-7). At `--jobs 1` this is the serial loop it
    // replaces, line for line and sum for sum.
    let work: Vec<(&Path, &Path)> =
        pairs.iter().take(limit).map(|(r, x)| (*r, x.as_path())).collect();
    let n = work.len();

    // ── resumable progress ───────────────────────────────────────────────
    let state_path = match state {
        Some(p) => p.to_path_buf(),
        None => default_state_path(dir, limit),
    };
    let header = StateHeader {
        autoshade_version: env!("CARGO_PKG_VERSION").to_string(),
        proposer_model: cfg.openai_model.clone(),
        analysis_model: cfg.analysis_model.clone(),
        limit,
        dir: dir.display().to_string(),
        started: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    println!("progress: {}", state_path.display());
    if fresh && state_path.exists() {
        std::fs::remove_file(&state_path).map_err(|e| {
            anyhow::anyhow!("--fresh could not discard {}: {e}", state_path.display())
        })?;
        println!("  --fresh: saved progress discarded; every photograph is measured again.");
    }
    let saved = load_state(&state_path, &header)?;
    // Append to a file that already carries a valid header; write a new one
    // otherwise. An existing file whose rows all turned out stale is still
    // appended to — its stale lines stay, and the fresh row for the same
    // photograph, written later, is the one a later resume reads.
    let resuming = !saved.is_empty();
    let (mut restored, stale) = resume_plan(dir, &work, saved);
    for stem in &stale {
        println!("  {stem}: its .xmp changed since it was measured — measuring it again.");
    }
    let loaded = restored.iter().filter(|r| r.is_some()).count();
    if loaded > 0 {
        println!("resuming: {loaded} of {n} already measured, {} to go.", n - loaded);
    }
    let log = StateLog::open(&state_path, &header, resuming);
    // How many photographs this run measured with the deterministic fallback
    // standing in for the proposer. A COUNT, order-independent, so an atomic
    // is honest here where an f64 sum would not be.
    let fallbacks = std::sync::atomic::AtomicU32::new(0);
    // `plan_for` for the same reason `batch` uses it (R28 Batch-4 4a), even
    // though `eval`'s work list is RAW+.xmp pairs by construction and the
    // survey therefore finds nothing to raise: the door a caller takes should
    // not depend on a filter two functions away staying RAW-only.
    let photos: Vec<&Path> = work.iter().map(|(r, _)| *r).collect();
    let plan = crate::jobs::plan_for(jobs, &photos);
    if let Some(note) = &plan.note {
        println!("{note}");
    }
    if plan.jobs > 1 {
        println!(
            "  {} photo(s) in flight; each photo's lines are held and printed in order.",
            plan.jobs
        );
    }
    let mut acc: BTreeMap<String, Acc> = BTreeMap::new();
    let mut evaluated = 0u32;
    // Master-tone-curve comparison (the look control the flat sliders miss),
    // accumulated only over photos where YOU drew a curve.
    let (mut curve_n, mut sum_curve_rmse) = (0u32, 0f64);
    let (mut sum_user_lift, mut sum_ai_lift) = (0f64, 0f64);
    let (mut sum_user_s, mut sum_ai_s) = (0f64, 0f64);
    // The three per-channel RGB curves (R23-1): same RMSE metric as the master,
    // one accumulator per channel, entered only when either side drew one.
    let rgb_curves = [("red", "ToneCurvePV2012Red"), ("green", "ToneCurvePV2012Green"),
        ("blue", "ToneCurvePV2012Blue")];
    let mut rgb_acc = [(0u32, 0f64); 3];
    // Local masks: how many the photographer used (importable + the ones our
    // importer refuses) against how many the AI proposed. A count, not a
    // divergence — deliberately NOT folded into the gap score below.
    let (mut mask_photos, mut user_masks, mut user_masks_unsupported, mut ai_masks) =
        (0u32, 0usize, 0usize, 0usize);

    // Set the moment a photo raises the HARD error the serial loop raised with
    // `?` (an unreadable sidecar). Checked at dequeue so the remaining photos
    // are skipped instead of billed — at `--jobs 1` that is exactly the old
    // stop-at-once behaviour; above it, the photos already in flight finish
    // first and the run then fails with the same message.
    let aborted = std::sync::atomic::AtomicBool::new(false);
    let live = plan.jobs == 1;
    let outcomes = crate::jobs::for_each_indexed(plan.jobs, n, |i, block| {
        if aborted.load(std::sync::atomic::Ordering::Relaxed) {
            return PhotoOutcome::Skipped;
        }
        let (raw, sidecar) = work[i];
        emit(live, block, &format!("[{}/{}] {} ... ", i + 1, n, pipeline::stem(raw)));
        restore_or_measure(live, block, restored[i].is_some(), |block| {
            let Some(xmp_text) = crate::store::read_sidecar(sidecar) else {
                aborted.store(true, std::sync::atomic::Ordering::Relaxed);
                emit(live, block, "FAILED\n");
                return PhotoOutcome::Aborted(format!("read user xmp for {}", raw.display()));
            };
            let mut stats = PhotoStats::default();
            // The Description's OWN scope, like every other whole-document reader
            // (`xmp::crs_own_scope`): a nested creative Look's baked parameters are
            // the PROFILE's, not the photographer's, and scoring the AI against
            // them charged it for matching a look no slider in the sidecar states.
            let scope = crate::xmp::crs_own_scope(&xmp_text);
            let scope = Scope::new(scope.as_ref());
            // Any value OTHER than "As Shot" is a user WB decision; an absent
            // attribute (non-LR / hand-trimmed sidecar) keeps the Kelvin as
            // intentional.
            let user_wb = scope.crs_str("WhiteBalance").as_deref() != Some("As Shot");
            let has_crop = scope.crs_str("HasCrop").as_deref() == Some("True");
            // style_strength = 0 AND judge = false: eval measures the RAW AI
            // proposal vs your edits, so it must NOT pull toward your historical
            // style and must NOT let the visual closed loop revise the proposal
            // (either would bias the gap — and the judge is a paid vision call
            // per photo, unasked-for in a measurement run).
            let (ai, _verdict, notes) =
                match pipeline::produce_recipe(
                    raw,
                    None,
                    &cfg,
                    false,
                    None,
                    None,
                    // The CALIBRATION point, named: eval is the measurement
                    // baseline R23-5's wider ruler is calibrated against, so it must
                    // not drift when the product default moves (R23-3).
                    //
                    // What this pins is the NUMBER axis — the prompt's ±50/±35 pair
                    // and temper's knees are bit-for-bit pre-R23 here. It does NOT
                    // freeze the request: R23-1 rewrote the proposer prose at every
                    // strength (and the verbatim old wording now lives in the ≤ 0.4
                    // band, where soft_cap_factor is 0.93 and the numbers would drift
                    // instead). So scores from this harness are not comparable across
                    // R23 whatever value stands here — the same break the module doc
                    // already records for the widened ruler — and the 147-photo set
                    // has to be re-measured to give the new baseline a number.
                    pipeline::GradeRequest {
                        strength: crate::recipe::GradeStrength::calibrated(),
                        ..Default::default()
                    },
                    false,
                    // The DEFAULT channel, deliberately (R29-1). `batch`
                    // collects its workers' diagnostics and renders them into
                    // the photo's block; eval does not, because the same
                    // disclosures already ride the typed `rationale::Note`
                    // channel it renders below by construction, and routing
                    // both would print each fallback twice in the measurement
                    // transcript. What eval loses is the ORDER of the stderr
                    // copy, which is where it was before this batch.
                    crate::diag::stderr(),
                ) {
                Ok(v) => v,
                Err(e) => {
                    emit(live, block, &format!("FAILED: {e}\n"));
                    return PhotoOutcome::Failed;
                }
            };
            for row in ruler.iter() {
                let eps = row.eps();
                let n0 = neutral(row, &ai);
                let user_val = user_value(row, scope, user_wb, has_crop);
                // "Used" is measured against the control's NEUTRAL, not against
                // zero: `color_grade.blending` sits at 50 untouched, so a
                // zero-based test called every neutral wheel a ±50 disagreement.
                let user_used = user_val.is_some_and(|u| (u - n0).abs() > eps);
                let ai_used = match row.metric.as_str() {
                    // Stating an absolute Kelvin AT ALL is a WB decision, even one
                    // that lands on the as-shot value.
                    "temperature_k" => ai.temperature_k.is_some(),
                    _ => ai_value(row, &ai).is_some_and(|a| (a - n0).abs() > eps),
                };
                // An absent slider means the photographer left it at its neutral,
                // an absent white balance means they kept as-shot — so a one-sided
                // disagreement still compares two real numbers instead of being
                // dropped from the aggregate (which understated the gap score).
                let u = user_val.unwrap_or(n0);
                let ai_val = ai_value(row, &ai);
                // A both-neutral row is not a comparison: Lightroom writes EVERY
                // slider explicitly (Contrast2012="0"), so counting 0↔0 rows
                // inflated n and diluted each control's MAE toward zero — the gap
                // score understated real divergence. A row where EITHER side
                // moved still counts (that disagreement is the signal).
                if !user_used && !ai_used {
                    continue;
                }
                // …with ONE exception, and it is a different question (R28 Batch-5
                // 5b): a toning wheel's hue is only a value while the wheel has
                // saturation. "Either side moved it" is the right rule for a slider
                // whose every position renders something; it is the wrong rule for
                // an angle that renders nothing at zero saturation, and it is what
                // produced the 141° `shadow_hue` artifact. See `hue_carries_colour`
                // for the threshold, what it costs, and why that row's history is
                // not comparable with what this run prints.
                if row.circular && !hue_carries_colour(row, scope, &ai) {
                    continue;
                }
                let e = stats.acc.entry(row.metric.clone()).or_default();
                match ai_val {
                    Some(a) => {
                        let d = row.diff(a, u) as f64;
                        e.sum_abs += d.abs();
                        e.sum_signed += d;
                        e.n += 1;
                        // You moved it; the AI parked it at neutral → a miss.
                        if user_used && !ai_used {
                            e.omit += 1;
                        }
                    }
                    // Unreachable while every row has a reader (pinned by the
                    // catalogue tests); kept so a future row without one still
                    // shows up as a miss instead of vanishing.
                    None => {
                        if (u - n0).abs() > eps {
                            e.omit += 1;
                        }
                    }
                }
            }

            // --- master tone curve: did the AI commit to a curve like you did? ---
            // Entered when EITHER side has a real curve: gating on the user's
            // alone made an AI-only curve invisible to the metric (an identity
            // user curve is a valid comparison baseline — curve_lut of no points
            // is the identity LUT). ANY point counts: even a one-point curve
            // renders (pinned endpoints), so `>= 2` still hid real curves.
            // Scoped like `parse_user_xmp` — the SAME rule
            // at every read of a whole sidecar, or the report scores the AI
            // against a curve the profile baked, not the one the user drew.
            let user_curve =
                parse_tone_curve(&crate::xmp::crs_own_scope(&xmp_text), "ToneCurvePV2012");
            let ai_curve = ai_tone_curve_points(&ai);
            if !user_curve.is_empty() || !ai_curve.is_empty() {
                let ulut = curve_lut(&user_curve);
                let alut = curve_lut(&ai_curve);
                stats.curve = Some((
                    curve_rmse(&ulut, &alut),
                    curve_black_lift(&ulut) as f64,
                    curve_black_lift(&alut) as f64,
                    curve_s_strength(&ulut) as f64,
                    curve_s_strength(&alut) as f64,
                ));
            }

            // --- the three per-channel RGB curves (R23-1) ---------------------
            // The colour-shaping companion to the master curve, and the other half
            // of the "did the AI use the look controls at all?" question. Same
            // either-side rule and same RMSE metric; the SHAPE metrics (black lift
            // / S-strength) are master-curve vocabulary and are not repeated here.
            for (ch, (_, tag)) in rgb_curves.iter().enumerate() {
                let user_curve = parse_tone_curve(scope.text(), tag);
                let ai_curve: Vec<(f32, f32)> = match catalogue::global_value(&ai, &format!("{}_curve", rgb_curves[ch].0)) {
                    Some(catalogue::GlobalValue::Curve(pts)) => {
                        pts.iter().map(|p| (p.input as f32, p.output as f32)).collect()
                    }
                    _ => Vec::new(),
                };
                if user_curve.is_empty() && ai_curve.is_empty() {
                    continue;
                }
                stats.rgb[ch] =
                    Some(curve_rmse(&curve_lut(&user_curve), &curve_lut(&ai_curve)));
            }

            // --- local masks: counts, not divergence --------------------------
            // The user's side comes from the same importer the app uses, plus the
            // corrections it REFUSES (foreign brush/AI masks): reporting only the
            // importable ones would understate how much local work the photograph
            // actually had.
            let imported = crate::xmp::xmp_to_recipe_for_photo(&xmp_text, raw).masks.len();
            let refused = crate::xmp::unsupported_corrections_for_photo(&xmp_text, raw);
            if imported + refused + ai.masks.len() > 0 {
                stats.masks = Some((imported, refused, ai.masks.len()));
            }

            emit(live, block, "done\n");
            // Above one job the per-photo ⚠ lines the pipeline raises on stderr
            // (the GPT-proposer fallback warn in `pipeline::produce_recipe`) arrive in COMPLETION
            // order and can no longer be read as belonging to the line above them.
            // The same disclosures ride the typed note channel by construction, so
            // attach them to THIS photo's block — the attribution the reordering
            // costs, bought back on the channel that already carries it. At one job
            // the transcript stays exactly what it was.
            if !live && !notes.is_empty() {
                let text = crate::rationale::render_en(&notes);
                let text = text.trim();
                if !text.is_empty() {
                    block.push_str("       ");
                    block.push_str(text);
                    block.push('\n');
                }
            }
            // The measurement is complete: save it, so the NEXT upstream stream
            // error costs this photograph nothing. A photograph the proposer
            // fell back on completes the run exactly as it does today — it is
            // in the table below — but it is not saved, and the disclosure says
            // so on the photo's own block rather than only in the summary.
            let row = StateRow {
                stem: pipeline::stem(raw).to_string(),
                rel: photo_rel(dir, raw),
                xmp_sha256: sha256_hex(xmp_text.as_bytes()),
                stats,
            };
            if !record_measured(&log, &notes, &row) {
                fallbacks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                emit(
                    live,
                    block,
                    "       (the proposer fell back to the deterministic baseline — NOT saved as \
                     progress; a rerun measures this photograph again)\n",
                );
            }
            PhotoOutcome::Stats(Box::new(row.stats))
        })
    });

    // ── FOLD, strictly in photo order ────────────────────────────────────────
    // `f64` addition is not associative, so summing in completion order would
    // move the printed gap score in the last ulp depending on which photo
    // finished first. Folding the positional results reproduces the serial
    // arithmetic exactly, at any `--jobs`.
    let mut abort: Option<String> = None;
    // Provenance of the table below, counted while it is folded.
    let (mut loaded_rows, mut measured_now) = (0u32, 0u32);
    for (i, outcome) in outcomes.into_iter().enumerate() {
        let Some(outcome) = outcome else { continue };
        // A restored photograph and a freshly measured one reach the fold the
        // SAME way, at the SAME index — that is what makes a resumed run's
        // arithmetic identical to an uninterrupted one's, whatever the split.
        let stats = match outcome {
            PhotoOutcome::Stats(s) => Some((s, false)),
            PhotoOutcome::Restored => restored[i].take().map(|s| (s, true)),
            // Reported in its own block; the run continues (unchanged).
            PhotoOutcome::Failed | PhotoOutcome::Skipped => None,
            // The FIRST hard error wins, so the message a `--jobs 1` run would
            // have raised is the message this run raises.
            PhotoOutcome::Aborted(why) => {
                if abort.is_none() {
                    abort = Some(why);
                }
                None
            }
        };
        let Some((s, from_state)) = stats else { continue };
        if from_state {
            loaded_rows += 1;
        } else {
            measured_now += 1;
        }
        for (metric, one) in &s.acc {
            acc.entry(metric.clone()).or_default().merge(one);
        }
        if let Some((rmse, ul, al, us, as_)) = s.curve {
            curve_n += 1;
            sum_curve_rmse += rmse;
            sum_user_lift += ul;
            sum_ai_lift += al;
            sum_user_s += us;
            sum_ai_s += as_;
        }
        for (ch, r) in s.rgb.iter().enumerate() {
            if let Some(rmse) = r {
                rgb_acc[ch].0 += 1;
                rgb_acc[ch].1 += rmse;
            }
        }
        if let Some((imported, refused, ai_n)) = s.masks {
            mask_photos += 1;
            user_masks += imported;
            user_masks_unsupported += refused;
            ai_masks += ai_n;
        }
        evaluated += 1;
    }
    if let Some(why) = abort {
        anyhow::bail!("{why}");
    }

    if evaluated == 0 {
        // Attempted pairs that ALL failed must not exit 0 — broken
        // credentials looked like a clean run to CI (16-lane scan L09).
        if n > 0 {
            anyhow::bail!("no photo evaluated — every attempted pair failed (see the FAILED lines above)");
        }
        println!("No photos evaluated.");
        return Ok(());
    }

    println!("\n=== AI vs your edits ({evaluated} photo(s)) ===");
    // WHERE the rows came from, before the numbers they produced: a table
    // built partly out of saved progress must say so, and a fallback row —
    // counted here, deliberately NOT saved — is the reason a rerun's photo
    // count can move.
    println!(
        "{evaluated} row(s) total: {loaded_rows} loaded from saved progress, {measured_now} \
         measured this run, {} fallback row(s) NOT persisted.",
        fallbacks.load(std::sync::atomic::Ordering::Relaxed)
    );
    // …and WHY that count is as low as it is: a run that spent repeats is not
    // the same run as one that never needed them, and the fallback number
    // above is the number those repeats were bought to hold down.
    println!("progress file: {}", state_path.display());
    println!("{:<22} {:>4} {:>10} {:>13} {:>8}", "field", "n", "mean|Δ|", "bias(AI−you)", "AI-omit");
    print!("{}", format_scalar_report_rows(&ruler, &acc));

    // --- master tone curve summary -------------------------------------------
    if curve_n > 0 {
        let avg = |s: f64| s / curve_n as f64;
        println!("\n=== Master tone curve ({curve_n} photo(s) where you or the AI drew one) ===");
        println!("  black lift (output@0):  you {:>6.1}   AI {:>6.1}", avg(sum_user_lift), avg(sum_ai_lift));
        println!("  S-strength (contrast):  you {:>6.1}   AI {:>6.1}", avg(sum_user_s), avg(sum_ai_s));
        println!("  curve RMSE (AI vs you): {:>6.1}   (0 = identical, on the 0..255 scale)", avg(sum_curve_rmse));
        if avg(sum_user_s).abs() > 4.0 && avg(sum_ai_s).abs() < avg(sum_user_s).abs() * 0.5 {
            println!("  → the AI's curve is much flatter than yours: it is omitting the S-curve that gives your photos their contrast.");
        }
    } else {
        println!("\n(no master tone curve in your XMPs to compare)");
    }

    // --- per-channel RGB curves + local masks (R23-1 ruler expansion) --------
    if rgb_acc.iter().any(|(n, _)| *n > 0) {
        println!("\n=== Per-channel RGB curves (photo(s) where you or the AI drew one) ===");
        for (ch, (name, _)) in rgb_curves.iter().enumerate() {
            let (n, sum) = rgb_acc[ch];
            if n > 0 {
                println!("  {name:<6} n {n:>3}   curve RMSE (AI vs you): {:>6.1}", sum / n as f64);
            }
        }
    } else {
        println!("\n(no per-channel RGB curves on either side)");
    }
    if mask_photos > 0 {
        let per = |v: usize| v as f64 / mask_photos as f64;
        println!("\n=== Local masks ({mask_photos} photo(s) with masks on either side) ===");
        println!(
            "  per photo: you {:.2} (+{:.2} our importer refuses)   AI {:.2}",
            per(user_masks),
            per(user_masks_unsupported),
            per(ai_masks)
        );
        println!("  (a COUNT, not a divergence — deliberately not folded into the gap score)");
    } else {
        println!("\n(no local masks on either side)");
    }

    // --- aggregate gap score -------------------------------------------------
    // Mean fractional divergence across the controls you used, each MAE
    // normalised by a sensible full-scale, plus the tone-curve terms. One number
    // to watch move as the advisor improves.
    //
    // NOT comparable with a pre-R23-1 score: the ruler now covers the HSL
    // mixer, the colour-grade wheels and the three channel curves, so controls
    // the AI never touched (and that were invisible before) now weigh in.
    let (mut frac_sum, mut frac_n) = (0f64, 0u32);
    for row in &ruler {
        if let Some(a) = acc.get(&row.metric)
            && a.n > 0
        {
            frac_sum += (a.sum_abs / a.n as f64) / row.full_scale();
            frac_n += 1;
        }
    }
    if curve_n > 0 {
        frac_sum += (sum_curve_rmse / curve_n as f64) / 255.0;
        frac_n += 1;
    }
    for (n, sum) in rgb_acc {
        if n > 0 {
            frac_sum += (sum / n as f64) / 255.0;
            frac_n += 1;
        }
    }
    // n/a, not 0.0%: with nothing measured, a perfect score claimed the AI
    // matched a look that was never compared.
    if frac_n > 0 {
        let gap = 100.0 * frac_sum / frac_n as f64;
        println!(
            "\nOverall gap score: {gap:.1}%  (mean per-control divergence incl. tone curve; lower = closer to your look)"
        );
        if let Some(weighted) = supplementary_weighted_gap(&ruler, &acc, curve_n, sum_curve_rmse, &rgb_acc) {
            print!("{}", format_supplementary_gap_line(weighted));
        }
    } else {
        println!(
            "\nOverall gap score: n/a — no comparable controls were measured (the XMPs may hold \
             no readable crs settings)"
        );
    }
    println!(
        "Interpretation: positive bias = AI sets this higher than you do; large mean|Δ| = you \
         disagree a lot on that control; AI-omit = times you used a control the AI ignored. Use \
         these to calibrate the advisor prompt."
    );
    // The one row whose MEANING changed, disclosed in the report itself rather
    // than only in the ledger — a reader comparing this transcript against the
    // R27 147-photo baseline has to know the population moved (R28 Batch-5 5b,
    // `hue_carries_colour`).
    println!(
        "Note: the four color_grade.*_hue rows count a photo only when BOTH sides put saturation \
         on that wheel (a hue at zero saturation renders nothing). Their n / mean|Δ| / AI-omit are \
         NOT comparable with a pre-v0.34.0 run; every other row is unchanged."
    );

    Ok(())
}
