//! `RECIPE_CONTROLS` — the ONE registry of the develop controls the AI
//! contract carries (user feedback #12, R23-1).
//!
//! Before this table the [`EditRecipe`] was the de-facto source and every
//! downstream copy was hand-transcribed from it, with a single comment
//! ("Mirrors `src/recipe.rs` — keep in sync") as the only constraint: the
//! advisor's strict JSON schema (twice — the recipe and the nested local
//! adjustment), the proposer prompt's prose, the eval harness's comparison
//! table and its report order, and the style index's `REF_KEYS`/`MAP` pair.
//! A control added to the recipe therefore reached the renderer, the XMP
//! writer and the GUI while staying INVISIBLE to the AI and to every ruler
//! that measures the AI — silently, forever.
//!
//! So the registry is the source now, and it is enforced from two sides:
//!
//!   * **the compiler** — [`global_value`] and [`local_value`] destructure
//!     `EditRecipe` / `LocalAdjustment` with NO `..` rest pattern and consume
//!     every binding in a `match`, so a new field is a build error (an
//!     un-destructured field breaks the pattern; a destructured-but-unmapped
//!     one is an `unused_variables` deny) until its row exists here.
//!     [`color_grade_value`] and [`hsl_value`] do the same for the two nested
//!     objects the schema spells out field by field.
//!   * **the tests** — the registry, the serde key set and the strict
//!     schema's `required` array are compared THREE ways (both mirrors), and
//!     every numeric row's stated range is re-derived from
//!     `EditRecipe::clamp` itself, so the range the model is told can never
//!     be a range the engine does not enforce.
//!
//! Consumers: [`edit_recipe_schema`] (the advisor's strict response schema),
//! [`prompt_catalogue`] (the proposer prompt's control list), `eval.rs` (the
//! comparison ruler) and `style.rs` (the reference key set, asserted against
//! this table).

use serde_json::{json, Value};

use crate::recipe::{
    ColorGrade, Crop, CurvePoint, EditRecipe, Hsl, HSL_BANDS, LensProfile, LocalAdjustment,
};

/// The JSON shape one control takes in the advisor's strict response schema —
/// or, for the engine-only carriers, the shape it has in `recipe.json` alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A plain `number`.
    Number,
    /// `["number","null"]` — null is the control's "no opinion" state.
    ///
    /// Where that null LANDS differs by control, on purpose: `temperature_k`
    /// carries it all the way into the recipe (`Option<f32>`, "keep as-shot"),
    /// while the three manual LENS fields are plain `f32` the engine always
    /// renders — their null lives only in the advisor's parse layer
    /// (`openai::take_lens_opinion`), where it means "the photographer's own
    /// value stands" and 0 means "zero it". Adding an Option to the recipe
    /// instead would put a fourth spelling of "unset" into `recipe.json`, the
    /// XMP writer and R21's fingerprint for a value the renderer must always
    /// have (R23-1b).
    NullableNumber,
    /// `["boolean","null"]` — null is "no opinion", as for [`Shape::NullableNumber`];
    /// the Black & White switch (2026-10-21).
    NullableBool,
    /// An `integer` (the schema-era stamp).
    Integer,
    /// A `boolean`.
    Bool,
    /// A `string`.
    Text,
    /// An array of `{input,output}` curve points, both 0..255.
    Curve,
    /// The 8-band HSL mixer object (three arrays of 8 numbers).
    Hsl,
    /// The 3-wheel + global colour-grade object (14 flat numbers).
    ColorGrade,
    /// The normalised crop rectangle, or null for "keep the whole frame".
    NullableCrop,
    /// An array of local adjustments (the nested schema mirror).
    Masks,
    /// A mask geometry: the linear/radial tagged union.
    MaskGeometry,
    /// A Range-Mask refinement (luminance/colour tagged union), or null.
    NullableRangeMask,
    /// A structure outside the advisor schema: base-curve knots, the in-camera lens
    /// profile, extra mask components, linear-light colour gains, the mask
    /// role. These have no schema spelling at all — [`Shape::schema`] answers
    /// `None`, which is why every carrier row is also `engine_only`.
    EngineCarrier,
}

impl Shape {
    /// The strict-mode JSON-schema property for this shape, or `None` for the
    /// engine-only carriers (which the schema must never ask for).
    pub fn schema(self) -> Option<Value> {
        let num = || json!({"type": "number"});
        Some(match self {
            Shape::Number => num(),
            Shape::NullableNumber => json!({"type": ["number", "null"]}),
            Shape::NullableBool => json!({"type": ["boolean", "null"]}),
            Shape::Integer => json!({"type": "integer"}),
            Shape::Bool => json!({"type": "boolean"}),
            Shape::Text => json!({"type": "string"}),
            Shape::Curve => curve_arr(),
            Shape::Hsl => hsl_schema(),
            Shape::ColorGrade => color_grade_schema(),
            Shape::NullableCrop => crop_schema(),
            Shape::Masks => json!({"type": "array", "items": local_adjustment_schema()}),
            Shape::MaskGeometry => mask_geometry_schema(),
            Shape::NullableRangeMask => range_mask_schema(),
            Shape::EngineCarrier => return None,
        })
    }

    /// Is this shape a single number the eval harness / the range-honesty test
    /// can read and write through one JSON field?
    pub fn is_scalar(self) -> bool {
        matches!(self, Shape::Number | Shape::NullableNumber)
    }

    /// Does the schema offer this shape as null — "no opinion", the
    /// photographer's own value stands ([`crate::advisor::Opinions`])?
    pub fn is_nullable(self) -> bool {
        matches!(self, Shape::NullableNumber | Shape::NullableBool)
    }
}

/// The AI-visible controls whose null the PARSE LAYER resolves — "no
/// opinion", the photographer's value stands — in registry order: the bit
/// order of [`crate::advisor::Opinions`]. At most 64 (one `u64`).
///
/// `temperature_k` is nullable too but is NOT here: its null travels into the
/// recipe as `Option::None` ("keep as-shot"), which is the recipe's own
/// spelling and needs no carry (see [`Shape::NullableNumber`]). Read off the
/// typed value rather than the name, so a second such row is excluded by the
/// same rule.
pub fn nullable_controls() -> Vec<&'static str> {
    let neutral = EditRecipe::default();
    let out: Vec<&str> = RECIPE_CONTROLS
        .iter()
        .filter(|c| !c.engine_only && c.shape.is_nullable())
        .filter(|c| !matches!(global_value(&neutral, c.name), Some(GlobalValue::OptNum(_))))
        .map(|c| c.name)
        .collect();
    assert!(out.len() <= 64, "Opinions is one u64: {} nullable controls", out.len());
    out
}

/// Where a control lands in a Lightroom / Camera Raw sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrsKey {
    /// Exactly ONE `crs:` attribute on the sidecar's own `rdf:Description` —
    /// readable with `xmp::crs_f32`, which is what makes a control measurable
    /// against the user's real edits (the eval ruler is derived from these).
    Attr(&'static str),
    /// A FAMILY of keys (per band / wheel / channel), or a key that lives on a
    /// nested element rather than the Description's attributes. The descriptor
    /// is documentation — the exact spellings live in `xmp.rs`, and the
    /// per-band/per-wheel expansions this module needs are
    /// [`hsl_expansion`] and [`COLOR_GRADE_CRS`].
    Family(&'static str),
    /// No classic-ACR counterpart: engine calibration, or app-internal
    /// provenance (rationale/confidence).
    None,
}

impl CrsKey {
    /// The single attribute spelling, for the consumers that read one key.
    pub fn attr(self) -> Option<&'static str> {
        match self {
            CrsKey::Attr(k) => Some(k),
            _ => None,
        }
    }

    /// Does this control have a sidecar property of its OWN — one key (or key
    /// family) that carries it out and reads it back?
    pub fn is_owned(self) -> bool {
        !matches!(self, CrsKey::None)
    }
}

/// **The five-tier control registry** (R24-5 M0, feedback #15b).
///
/// Two independent facts decide what a control IS, and until now nothing wrote
/// them down together: does the ENGINE render it, and does it reach the
/// LIGHTROOM SIDECAR? [`Control::engine_only`] answers a third, different
/// question (may the AI set it), and `CrsKey` answered half of the second —
/// so "the GUI offers a slider the engine ignores" and "the save silently
/// drops what you are looking at" were both unrepresentable claims. Each tier
/// below names one (renders × exports) combination, and the disclosure
/// surfaces are DERIVED from it (`xmp::global_export_losses`,
/// `xmp::unmodelled_global_crs`) rather than restating a list.
///
/// Nothing here changes a control's behaviour: this round declares today's
/// truth and asserts it. Moving a control BETWEEN tiers (the LR-gap batches
/// B2–B5 — global Texture, Detail/Defringe, Transform/Calibration, mask
/// geometry) is the work this registry exists to make reviewable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// The engine renders it AND it round-trips through its own `crs:`
    /// property. What a user assumes every control is.
    Rendered,
    /// Round-trips through its own `crs:` property, and the engine renders
    /// NOTHING from it. Legitimate only for controls that carry meaning rather
    /// than pixels — every member must be listed in [`CARRIED_ONLY_GLOBAL`] /
    /// [`CARRIED_ONLY_LOCAL`] with the reason, or the inclusion test fails.
    CarriedOnly,
    /// Carried through the sidecar verbatim and never interpreted — neither
    /// engine nor reader gives it meaning. Reserved for the LR-only property
    /// blocks the XMP merge already preserves; R25 B4 gave the tier its first
    /// and so far only member, `passthrough` (Lightroom's Transform / Upright
    /// block and the camera profile's name — `xmp::PASSTHROUGH_CRS`, nine keys
    /// read and written as the strings Lightroom wrote).
    ///
    /// The line against [`CarriedOnly`](Tier::CarriedOnly): a carried control
    /// is a NUMBER we read, bound and could render one day, and it owes the
    /// whitelist a reason why we do not. A pass-through property is a STRING
    /// we have no opinion about at all — no band, no clamp, no neutral, and
    /// nothing to promise. That is why it needs no allow-list and why
    /// `unparsable_crs_numbers` exempts it: there is no reading to get wrong.
    PassThrough,
    /// The engine renders it and the sidecar cannot carry it — what you see
    /// here is NOT what Lightroom will show. The half of the disclosure story
    /// that had no name: `xmp::global_export_losses` is generated from exactly
    /// this tier.
    RenderedNotExported,
    /// Has no `crs:` property of its own; only a value DERIVED from it is
    /// written (the stamped as-shot white balance, which reaches the sidecar
    /// as `crs:Temperature`/`crs:Tint` when the user made no WB choice). It
    /// therefore never reads back as itself.
    DerivedWriteOnly,
}

impl Tier {
    /// Does the develop engine's output change when this control changes?
    pub fn renders(self) -> bool {
        match self {
            // DerivedWriteOnly is a rendering control too — its members are
            // the as-shot WB the engine develops with when the recipe states
            // no temperature of its own. The tier names its EXPORT shape,
            // which is the fact that had no other home.
            Tier::Rendered | Tier::RenderedNotExported | Tier::DerivedWriteOnly => true,
            Tier::CarriedOnly | Tier::PassThrough => false,
        }
    }

    /// Does this control own a `crs:` property that carries it out and back?
    /// Cross-checked against [`Control::crs`] by the registry's own test, so
    /// the tier can never claim an export shape the writer does not have.
    pub fn owns_crs_key(self) -> bool {
        match self {
            Tier::Rendered | Tier::CarriedOnly | Tier::PassThrough => true,
            Tier::RenderedNotExported | Tier::DerivedWriteOnly => false,
        }
    }
}

/// The `CarriedOnly` allow-list for [`RECIPE_CONTROLS`] — global controls a
/// surface may set although the engine renders nothing from them, each with
/// the reason.
///
/// **Policy SF4-C** (decided 2026-08-18, R25) limited membership to ADOBE-ONLY
/// OPERATORS — a control whose model Adobe has not published — on the ground
/// that approximating one would put our invention into the photographer's
/// picture. **The user revoked it on 2026-09-17** for v1.5.0: every control
/// the app can set is to render, a slight deviation from Lightroom allowed and
/// compatibility the aim. So this list only shrinks now, an entry leaving with
/// the batch that renders it (the eight Detail axes left first, for
/// `render/detail.rs`, then the nine Effects for `render/finish.rs` — which
/// also answered the reason they were carried, by giving the engine a stage
/// AFTER the crop, then the lens batch for `render/lens.rs`).
///
/// **It is now EMPTY**, and that is the whole point of Track F: there is no
/// longer a global control this app can set and this engine renders nothing
/// from. The list, the allow-list gates around it and
/// `xmp::global_render_gaps` all stay — emptiness is a measurement, not a
/// reason to remove the instrument, and the next row that arrives carried
/// discloses itself the day it is added instead of the release after. The
/// reasons the twenty-four members USED to give are not kept here: each is now
/// answered by the module that renders the control (`render/detail.rs`,
/// `render/finish.rs`, `render/lens.rs`), and the last of them — Adobe's
/// unpublished 0..100 de-fringe hue scale — is answered by
/// `lens::PURPLE_SCALE` / `GREEN_SCALE`, which state the mapping as OURS.
pub const CARRIED_ONLY_GLOBAL: &[(&str, &str)] = &[];

/// The `CarriedOnly` allow-list for [`LOCAL_CONTROLS`].
pub const CARRIED_ONLY_LOCAL: &[(&str, &str)] = &[(
    "name",
    "a label, not an operator: it round-trips as crs:CorrectionName/MaskName so \
     a mask keeps its identity across Lightroom, and no pixel depends on it",
)];

/// One develop control: what it is called on the wire, what shape and band it
/// takes, whether the AI may set it, where it lands in an XMP, and the one
/// line the model is told about it.
#[derive(Debug, Clone, Copy)]
pub struct Control {
    /// The serde field name — the SAME string in `recipe.json`, in the strict
    /// schema and in the eval report.
    pub name: &'static str,
    pub shape: Shape,
    /// The documented legal band, exactly what `EditRecipe::clamp` enforces
    /// (`None` for the non-scalar shapes). Pinned by
    /// `stated_ranges_are_the_ranges_clamp_enforces`.
    pub range: Option<(f32, f32)>,
    /// The neutral / default value, spelled for the prompt.
    pub neutral: &'static str,
    /// ENGINE-ONLY: the advisor schema never asks the model for it, and
    /// `pipeline::carry_over_unrepresentable` re-attaches it after a refine.
    ///
    /// Since R23-1b every remaining one is a PERMANENT design marker, not
    /// staged work, and there are now three kinds. The stamped calibration
    /// (`as_shot_k`, `as_shot_tint`, `base_curve`, `lens_profile`) is the
    /// engine's own measurement of this photo; `components` / `role` are
    /// existing editor state preserved across an advisor refine (native
    /// components now have a CRS spelling and roles have editing metadata),
    /// while `color_gains` still has no classic-ACR counterpart. Schema
    /// ownership is independent of exportability. The nine R25 B2 EFFECTS
    /// were carried under policy SF4-C — asking the model for a number that
    /// moves no pixel here would spend a paid request on a slider only
    /// Lightroom can honour. The eight Detail axes render since v1.5.0 and stay
    /// engine-only on `ca_r`'s ground (one-to-three-pixel structure the
    /// advisor's preview cannot show). The parametric tone curve (v1.5.0) is
    /// engine-only because the point curve the model already has is the more
    /// expressive spelling of the same move. The point colours (v1.5.0) are
    /// engine-only because a swatch is picked off the photo, not stated.
    /// The nine effects, the B&W treatment with its grey mix and the
    /// Calibration panel WERE engine-only until 2026-10-21 (user ruling: the
    /// AI gets every develop tool it can judge from the preview) and are
    /// NULLABLE rows now — null is "no opinion", so a required field no
    /// longer returns neutral values over the photographer's own on every
    /// refine; `crate::advisor::Opinions` carries which were stated. `enabled`
    /// stays here for a reason of its own — see [`LOCAL_CONTROLS`]' row for it.
    ///
    /// Every engine-only row needs a HOME for its value across a refine: the
    /// response cannot restate it, so either the pipeline re-stamps it
    /// (`version`/`coord_era`/the calibration) or
    /// `pipeline::carry_over_unrepresentable` copies it from the base
    /// (the mask state, the lens trio, the carried effects). A row with
    /// neither is a value the first Refine silently deletes.
    pub engine_only: bool,
    pub crs: CrsKey,
    /// WHAT this control is, on the (renders × exports) axes — see [`Tier`].
    ///
    /// `None` is not "unclassified": it means the row is NOT a develop control
    /// at all but part of the response ENVELOPE (the era stamp) or the AI's own
    /// provenance (`rationale`, `confidence`), mask bookkeeping the solver
    /// owns (`role`), the recipe's own bookkeeping about OTHER rows
    /// (`explicit_zero`, which says how a companion's stored 0 reads). A `None`
    /// row may never own a `crs:` property — asserted, so the escape hatch
    /// cannot be used to dodge a classification.
    ///
    /// Adding a field to `EditRecipe` / `LocalAdjustment` already fails the
    /// build until it has a row here (the [`global_value`] / [`local_value`]
    /// destructures); this field makes the row itself incomplete until its
    /// tier is decided, because a struct literal cannot omit it.
    pub tier: Option<Tier>,
    /// One line for the model: what this control does.
    pub purpose: &'static str,
}

impl Control {
    /// `(-100..100, neutral 0)` — the range/neutral parenthetical the prompt
    /// catalogue prints. Integers and structures print their shape instead of
    /// a numeric band.
    pub fn spec(&self) -> String {
        match self.range {
            Some((lo, hi)) => format!("{}..{}, neutral {}", trim_num(lo), trim_num(hi), self.neutral),
            None => format!("neutral {}", self.neutral),
        }
    }
}

/// `-100` not `-100.0`, `0.65` still `0.65` — the prompt reads like Lightroom's
/// own labels.
fn trim_num(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v}")
    }
}

/// Every field of [`EditRecipe`], in DECLARATION order — which is also the
/// order the strict schema's `required` array takes, so the generated schema
/// is byte-identical to the hand-written mirror it replaced.
pub const RECIPE_CONTROLS: [Control; 116] = [
    Control {
        name: "version",
        shape: Shape::Integer,
        range: None,
        neutral: "2",
        engine_only: false,
        crs: CrsKey::None,
        tier: None,
        purpose: "schema + calibration-era stamp; return 2",
    },
    Control {
        name: "coord_era",
        shape: Shape::Integer,
        range: None,
        neutral: "1",
        // ENGINE-ONLY on purpose: this says which FRAME `crop` and the mask
        // geometries are drawn in, and the model only ever sees the display
        // frame — asking it would invite a wrong answer about a fact it
        // cannot observe. Stamped where the model's JSON becomes a recipe
        // (`advisor::proposer`), exactly like the other measured stamps.
        engine_only: true,
        crs: CrsKey::None,
        tier: None,
        purpose: "engine bookkeeping: which coordinate frame crop/masks are stored in \
                  (0 = the pre-v0.30 sensor frame, 1 = the EXIF display frame)",
    },
    Control {
        name: "schema_era",
        shape: Shape::Integer,
        range: None,
        neutral: "1",
        // ENGINE-ONLY for the same reason as `coord_era`, and a sharper one:
        // this says which controls the recipe has ever SEEN, which is a fact
        // about the file it came from. A model asked for it could only guess,
        // and a wrong guess makes the XMP merge either strip a key it should
        // have left alone or leave one it should have replaced. Stamped where
        // model JSON becomes a recipe (`advisor::proposer`) and where a request
        // body does (`serve::live_frame_recipe`) — neither is a FILE, so
        // neither predates anything.
        engine_only: true,
        crs: CrsKey::None,
        tier: None,
        purpose: "engine bookkeeping: which control set this recipe was written against \
                  (0 = before the R25 carried keys existed, 1 = current)",
    },
    Control {
        name: "exposure_ev",
        shape: Shape::Number,
        range: Some((-5.0, 5.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Exposure2012"),
        tier: Some(Tier::Rendered),
        purpose: "global exposure in stops (EV)",
    },
    Control {
        name: "contrast",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Contrast2012"),
        tier: Some(Tier::Rendered),
        purpose: "global contrast about the midtone",
    },
    Control {
        name: "highlights",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Highlights2012"),
        tier: Some(Tier::Rendered),
        purpose: "recover (negative) or open up (positive) the BRIGHT tones",
    },
    Control {
        name: "shadows",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Shadows2012"),
        tier: Some(Tier::Rendered),
        purpose: "lift (positive) or deepen (negative) the DARK tones",
    },
    Control {
        name: "whites",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Whites2012"),
        tier: Some(Tier::Rendered),
        purpose: "the WHITE point — how bright the brightest tones sit",
    },
    Control {
        name: "blacks",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Blacks2012"),
        tier: Some(Tier::Rendered),
        purpose: "the BLACK point — how deep the darkest tones sit",
    },
    Control {
        name: "temperature_k",
        shape: Shape::NullableNumber,
        range: Some((2000.0, 50000.0)),
        neutral: "null = keep as-shot",
        engine_only: false,
        crs: CrsKey::Attr("Temperature"),
        tier: Some(Tier::Rendered),
        purpose: "white balance as an ABSOLUTE colour temperature in Kelvin (a TARGET, not a \
                  shift): lower = cooler/bluer result, higher = warmer. null keeps the camera's \
                  as-shot WB",
    },
    Control {
        name: "tint",
        shape: Shape::Number,
        range: Some((-150.0, 150.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Tint"),
        tier: Some(Tier::Rendered),
        purpose: "green (negative) / magenta (positive) shift RELATIVE to the as-shot white \
                  balance — unlike temperature_k this one is a delta, not an absolute value",
    },
    Control {
        name: "as_shot_k",
        shape: Shape::NullableNumber,
        range: Some((2000.0, 50000.0)),
        neutral: "null = unknown",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::DerivedWriteOnly),
        purpose: "the camera's OWN white balance in absolute Kelvin, stamped per photo by the \
                  engine — the anchor temperature_k is measured against",
    },
    Control {
        name: "as_shot_tint",
        shape: Shape::NullableNumber,
        range: Some((-150.0, 150.0)),
        neutral: "null = unknown",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::DerivedWriteOnly),
        purpose: "the as-shot green/magenta companion of as_shot_k (display + XMP honesty only)",
    },
    Control {
        name: "vibrance",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Vibrance"),
        tier: Some(Tier::Rendered),
        purpose: "saturation that protects the already-saturated colours",
    },
    Control {
        name: "saturation",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Saturation"),
        tier: Some(Tier::Rendered),
        purpose: "uniform saturation of every colour",
    },
    Control {
        name: "clarity",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Clarity2012"),
        tier: Some(Tier::Rendered),
        purpose: "midtone local contrast / punch",
    },
    Control {
        name: "dehaze",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Dehaze"),
        tier: Some(Tier::Rendered),
        purpose: "atmospheric haze removal (negative ADDS haze)",
    },
    Control {
        name: "texture",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Texture"),
        tier: Some(Tier::Rendered),
        purpose: "FINE detail — skin, foliage, fabric — without clarity's midtone volume \
                  (negative smooths it away)",
    },
    Control {
        name: "hsl",
        shape: Shape::Hsl,
        range: None,
        neutral: "every band 0",
        engine_only: false,
        crs: CrsKey::Family("{Hue,Saturation,Luminance}Adjustment<Band> (8 bands)"),
        tier: Some(Tier::Rendered),
        purpose: "the 8-band colour mixer: `hue`, `saturation` and `luminance` are each an array \
                  of EXACTLY 8 numbers (-100..100) in the FIXED band order red, orange, yellow, \
                  green, aqua, blue, purple, magenta",
    },
    // v1.5.0 — Lightroom's Black & White treatment: the switch and its
    // eight-band grey mix, rendered by `render::apply_gray_mix` in the colour
    // mixer's place. Engine-only until 2026-10-21 on the parametric curve's
    // ground (required fields would return a colour photo and a flat mix
    // over the photographer's own on every refine); nullable now, so the
    // model can ask for a black-and-white rendering and leave one alone.
    Control {
        name: "convert_to_grayscale",
        shape: Shape::NullableBool,
        // A switch, not a band: `Shape::is_scalar` is false, so there is no
        // range to state (the `auto_lateral_ca` row's rule).
        range: None,
        neutral: "null = no opinion (false = colour)",
        engine_only: false,
        crs: CrsKey::Attr("ConvertToGrayscale"),
        tier: Some(Tier::Rendered),
        purpose: "Lightroom's Black & White treatment: the photo renders as a grey mix of its colours",
    },
    Control {
        name: "gray_red",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GrayMixerRed"),
        tier: Some(Tier::Rendered),
        purpose: "B&W mix, reds: how light (+) or dark (−) what was red turns in the grey rendering",
    },
    Control {
        name: "gray_orange",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GrayMixerOrange"),
        tier: Some(Tier::Rendered),
        purpose: "B&W mix, oranges: how light (+) or dark (−) what was orange turns in the grey rendering",
    },
    Control {
        name: "gray_yellow",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GrayMixerYellow"),
        tier: Some(Tier::Rendered),
        purpose: "B&W mix, yellows: how light (+) or dark (−) what was yellow turns in the grey rendering",
    },
    Control {
        name: "gray_green",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GrayMixerGreen"),
        tier: Some(Tier::Rendered),
        purpose: "B&W mix, greens: how light (+) or dark (−) what was green turns in the grey rendering",
    },
    Control {
        name: "gray_aqua",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GrayMixerAqua"),
        tier: Some(Tier::Rendered),
        purpose: "B&W mix, aquas: how light (+) or dark (−) what was aqua turns in the grey rendering",
    },
    Control {
        name: "gray_blue",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GrayMixerBlue"),
        tier: Some(Tier::Rendered),
        purpose: "B&W mix, blues: how light (+) or dark (−) what was blue turns in the grey rendering",
    },
    Control {
        name: "gray_purple",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GrayMixerPurple"),
        tier: Some(Tier::Rendered),
        purpose: "B&W mix, purples: how light (+) or dark (−) what was purple turns in the grey rendering",
    },
    Control {
        name: "gray_magenta",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GrayMixerMagenta"),
        tier: Some(Tier::Rendered),
        purpose: "B&W mix, magentas: how light (+) or dark (−) what was magenta turns in the grey rendering",
    },
    // v1.5.0 — Lightroom's Point Color, rendered by `render::apply_point_colors`
    // after the colour mixer. A carrier: a list of nineteen-number swatches is
    // nothing a model could state, and a swatch is picked off the photo with
    // the eyedropper.
    Control {
        name: "point_colors",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "empty = no swatch",
        engine_only: true,
        crs: CrsKey::Family("PointColors (an rdf:Seq of nineteen-number swatches)"),
        tier: Some(Tier::Rendered),
        purpose: "Lightroom's Point Color swatches: each shifts the hue, saturation and luminance of \
                  the one colour it was sampled from",
    },
    Control {
        name: "color_grade",
        shape: Shape::ColorGrade,
        range: None,
        neutral: "every wheel 0, blending 50",
        engine_only: false,
        crs: CrsKey::Family("SplitToning* + ColorGrade*"),
        tier: Some(Tier::Rendered),
        purpose: "3-wheel + global colour grading: per region (shadow/midtone/highlight/global) a \
                  `*_hue` 0..360, `*_sat` 0..100 and `*_lum` -100..100, plus `blending` 0..100 \
                  (region overlap) and `balance` -100..100 (shadow/highlight split)",
    },
    Control {
        name: "sharpening",
        shape: Shape::Number,
        range: Some((0.0, 150.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("Sharpness"),
        tier: Some(Tier::Rendered),
        // This row's band is ALSO the sidecar's, since v0.31.1: `crs:Sharpness`
        // stores Lightroom's Amount slider 1:1 and that slider's UI maximum is
        // 150, not 100 (15 real sidecars carry `Sharpness="150"`; web survey
        // 2026-08-18, see `xmp::xmp_to_recipe`). So `crs_number_is_in_recipe_range`
        // derives this key like every other one — the hand-written scale
        // residue it used to need is gone.
        purpose: "capture sharpening amount, 1:1 with Lightroom's Detail > Sharpening Amount \
                  slider and with `crs:Sharpness` (both 0..150); 0 = absent, which renders at \
                  Lightroom's own default for the source (40 on a RAW, 0 on a baked raster)",
    },
    Control {
        name: "noise_reduction",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("LuminanceSmoothing"),
        tier: Some(Tier::Rendered),
        purpose: "global luminance noise reduction",
    },
    // The eight detail axes around the two sliders above (R25 B3), RENDERED
    // since v1.5.0 (`render/detail.rs`). Still `engine_only`, on the ground
    // `ca_r` states below: they shape one-to-three-pixel structure the
    // advisor's ~1024 px preview cannot show, so a required schema field could
    // only return a guess — and, being required, would return 0 and replace
    // the photographer's own value on every Refine.
    //
    // A COMPANION's stored 0 (`recipe::LR_COMPANION_DEFAULTS`) renders at
    // Lightroom's default unless `explicit_zero` names it; that is what "0 =
    // absent" below means.
    Control {
        name: "sharpen_radius",
        shape: Shape::Number,
        // 0 = absent; Lightroom's own band starts at 0.5, so the stated band
        // has to reach the neutral or `clamp` and the registry disagree.
        range: Some((0.0, 3.0)),
        neutral: "0 = absent (renders at Lightroom's default, 1.0)",
        engine_only: true,
        crs: CrsKey::Attr("SharpenRadius"),
        tier: Some(Tier::Rendered),
        purpose: "the radius capture sharpening works at, in full-resolution pixels",
    },
    Control {
        name: "sharpen_detail",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "0 = absent (renders at Lightroom's default, 25)",
        engine_only: true,
        crs: CrsKey::Attr("SharpenDetail"),
        tier: Some(Tier::Rendered),
        purpose: "how far sharpening suppresses its own halos (0) or lifts fine texture (100)",
    },
    Control {
        name: "sharpen_mask",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("SharpenEdgeMasking"),
        tier: Some(Tier::Rendered),
        purpose: "how far sharpening is confined to edges (0 = everywhere)",
    },
    Control {
        name: "nr_detail",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "0 = absent (renders at Lightroom's default, 50)",
        engine_only: true,
        crs: CrsKey::Attr("LuminanceNoiseReductionDetail"),
        tier: Some(Tier::Rendered),
        purpose: "how much texture luminance noise reduction keeps",
    },
    Control {
        name: "nr_contrast",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("LuminanceNoiseReductionContrast"),
        tier: Some(Tier::Rendered),
        purpose: "how much low-frequency local contrast luminance noise reduction puts back",
    },
    Control {
        name: "color_nr",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        // NOT a companion: 0 is no colour noise reduction, and the writer
        // states it (`xmp::amount_carries`) so Lightroom does not apply its
        // RAW default of 25 to a photo this engine shows without any.
        neutral: "0 = none",
        engine_only: true,
        crs: CrsKey::Attr("ColorNoiseReduction"),
        tier: Some(Tier::Rendered),
        purpose: "colour noise reduction — a luma-guided smoothing of the colour-difference planes",
    },
    Control {
        name: "color_nr_detail",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "0 = absent (renders at Lightroom's default, 50)",
        engine_only: true,
        crs: CrsKey::Attr("ColorNoiseReductionDetail"),
        tier: Some(Tier::Rendered),
        purpose: "how closely colour noise reduction keeps to the edges luma draws",
    },
    Control {
        name: "color_nr_smooth",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "0 = absent (renders at Lightroom's default, 50)",
        engine_only: true,
        crs: CrsKey::Attr("ColorNoiseReductionSmoothness"),
        tier: Some(Tier::Rendered),
        purpose: "how much low-frequency colour mottling colour noise reduction removes",
    },
    Control {
        name: "lens_vignette",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = zero the photographer's own value)",
        engine_only: false,
        crs: CrsKey::Attr("VignetteAmount"),
        tier: Some(Tier::Rendered),
        purpose: "manual LENS vignette compensation — a radial gain in LINEAR light before any \
                  tonal work (a falloff correction, NOT a creative corner darkening); null means \
                  you have NO opinion and the photographer's own value stands",
    },
    Control {
        name: "lens_vignette_mid",
        shape: Shape::NullableNumber,
        range: Some((0.0, 100.0)),
        neutral: "null = no opinion (the engine's own neutral is 50)",
        engine_only: false,
        crs: CrsKey::Attr("VignetteMidpoint"),
        tier: Some(Tier::Rendered),
        purpose: "where the vignette compensation lands (lower reaches toward the centre); null \
                  means no opinion",
    },
    Control {
        name: "lens_distortion",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = zero the photographer's own value)",
        engine_only: false,
        crs: CrsKey::Attr("LensManualDistortionAmount"),
        tier: Some(Tier::Rendered),
        purpose: "manual geometric distortion correction (positive straightens BARREL, negative \
                  PINCUSHION); null means you have NO opinion and the photographer's own value \
                  stands",
    },
    // v1.5.0, F5: the lens PROFILE's own two strengths. Engine-only on the
    // manual CA pair's ground — the correction they scale is measured optics
    // (the camera's own knots, or an Adobe `.lcp` on this machine), and the
    // model sees a JPEG. They are the only two rows in this registry whose
    // neutral is 100.
    Control {
        name: "lens_profile_distortion_scale",
        shape: Shape::Number,
        range: Some((0.0, 200.0)),
        neutral: "100 (the profile's own map; 0 = no profile distortion correction)",
        engine_only: true,
        crs: CrsKey::Attr("LensProfileDistortionScale"),
        tier: Some(Tier::Rendered),
        purpose: "how much of the lens profile's geometric correction to apply",
    },
    Control {
        name: "lens_profile_vignetting_scale",
        shape: Shape::Number,
        range: Some((0.0, 200.0)),
        neutral: "100 (the profile's own gains; 0 = no profile vignetting correction)",
        engine_only: true,
        crs: CrsKey::Attr("LensProfileVignettingScale"),
        tier: Some(Tier::Rendered),
        purpose: "how much of the lens profile's vignetting correction to apply",
    },
    // v1.5.0 F6 — Lightroom's TRANSFORM panel, rendered as one projective map
    // between the lens-geometry resample and the straighten
    // (`render::perspective`). Engine-only on the Calibration panel's ground:
    // a required schema field would return neutral values over the
    // photographer's own keystone on every Refine, and a ~1024 px preview is
    // not where a building's verticals get judged.
    //
    // Their `crs:` keys were `Tier::PassThrough` from R25 B4 until here — the
    // whole point of that tier's doc saying it hoped to SHRINK.
    Control {
        name: "perspective_vertical",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("PerspectiveVertical"),
        tier: Some(Tier::Rendered),
        purpose: "Transform, Vertical: the keystone a camera tilted up or down leaves in the verticals",
    },
    Control {
        name: "perspective_horizontal",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("PerspectiveHorizontal"),
        tier: Some(Tier::Rendered),
        purpose: "Transform, Horizontal: the same keystone about the vertical axis",
    },
    Control {
        name: "perspective_rotate",
        shape: Shape::Number,
        range: Some((-10.0, 10.0)),
        neutral: "0 (DEGREES, not the -100..100 of its neighbours)",
        engine_only: true,
        crs: CrsKey::Attr("PerspectiveRotate"),
        tier: Some(Tier::Rendered),
        purpose: "Transform, Rotate: a turn inside the transform, before the crop's own straighten",
    },
    Control {
        name: "perspective_scale",
        shape: Shape::Number,
        // Lightroom's own band (since 2026-09-24; the recipe clamps to it —
        // at 0 the transform map collapsed and the whole step dropped out).
        range: Some((50.0, 150.0)),
        neutral: "100 (the frame as it is)",
        engine_only: true,
        crs: CrsKey::Attr("PerspectiveScale"),
        tier: Some(Tier::Rendered),
        purpose: "Transform, Scale: zoom inside the transform, the usual answer to a keystone's empty corners",
    },
    Control {
        name: "perspective_aspect",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("PerspectiveAspect"),
        tier: Some(Tier::Rendered),
        purpose: "Transform, Aspect: stretches the frame vertically (+) or horizontally (−), area-preserving",
    },
    Control {
        name: "perspective_x",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("PerspectiveX"),
        tier: Some(Tier::Rendered),
        purpose: "Transform, X Offset: slides the transformed frame sideways",
    },
    Control {
        name: "perspective_y",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("PerspectiveY"),
        tier: Some(Tier::Rendered),
        purpose: "Transform, Y Offset: slides the transformed frame up or down",
    },
    Control {
        // An INSTRUCTION, like `auto_lateral_ca` — and the second one this
        // engine renders by running a solver. Adobe's solver is unpublished but
        // its ANSWER is not: every sidecar that used Upright carries the six
        // matrices in `upright_transform`, so a Lightroom photograph renders
        // Adobe's own numbers and only this app's own dropdown reaches for
        // `render::perspective::solve_upright`.
        name: "perspective_upright",
        shape: Shape::Number,
        range: Some((0.0, 5.0)),
        neutral: "0 = off (1 auto, 2 level, 3 vertical, 4 full, 5 guided)",
        engine_only: true,
        crs: CrsKey::Attr("PerspectiveUpright"),
        tier: Some(Tier::Rendered),
        purpose: "Transform, Upright MODE: which automatic perspective correction to apply",
    },
    Control {
        // Read and rendered, never written — `lens_profile`'s tier, for
        // `lens_profile`'s reason: these are Adobe's measurement of this
        // photograph, and the merge preserves the document's own bytes.
        name: "upright_transform",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "empty = Lightroom never solved an Upright for this photo",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::RenderedNotExported),
        purpose: "Adobe's solved Upright matrices, read from crs:UprightTransform_0…N and indexed by \
                  the mode that selects them",
    },
    Control {
        name: "crop_constrain_to_warp",
        shape: Shape::Bool,
        range: None,
        neutral: "false = Lightroom's own value on all 52 sidecars in this library that carry it",
        engine_only: true,
        crs: CrsKey::Attr("CropConstrainToWarp"),
        tier: Some(Tier::Rendered),
        purpose: "Transform, Constrain Crop: shrink the crop until it lies inside the warped frame \
                  instead of showing the empty corners",
    },
    // Manual lateral CA (R25 B3) — RENDERED, and the FOURTH kind of
    // `engine_only` (see the field's doc): not staged work, not stamped
    // calibration, but a control the AI must not be asked for. Lateral CA is
    // a one-to-three-pixel edge artefact; the advisor sees a ~1024 px preview,
    // where it is physically not there, so a required schema field could only
    // ever be a guess — and, being required, that guess would come back as 0
    // and delete the photographer's own value on every Refine (the exact
    // defect `LensOpinion` exists for). It rides
    // `pipeline::carry_over_unrepresentable` instead.
    Control {
        name: "ca_r",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("ChromaticAberrationR"),
        tier: Some(Tier::Rendered),
        purpose: "manual lateral CA on the red / cyan axis — folded onto the same per-channel \
                  radius LUT the in-camera profile's CA uses",
    },
    Control {
        name: "ca_b",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("ChromaticAberrationB"),
        tier: Some(Tier::Rendered),
        purpose: "manual lateral CA on the blue / yellow axis",
    },
    Control {
        name: "auto_lateral_ca",
        shape: Shape::Bool,
        // A FLAG, not a band — `Shape::is_scalar` is false, so
        // `stated_ranges_are_the_ranges_clamp_enforces` does not probe it and
        // there is no range to state.
        range: None,
        neutral: "false",
        engine_only: true,
        crs: CrsKey::Attr("AutoLateralCA"),
        tier: Some(Tier::Rendered),
        purpose: "Lightroom's 「Remove chromatic aberration」 switch — an INSTRUCTION, so \
                  rendering it means running the solver it names (`render::lens::solve_lateral_ca`) \
                  and handing the answer to the manual pair above",
    },
    // De-fringe. Six keys carried and unrendered from R25 B3 until v1.5.0,
    // when the user revoked policy SF4-C and `render/lens.rs` stated the
    // 0..100 hue scale as OURS. Still the one block whose neutral is Adobe's
    // default rather than zero — see the fields' own doc in `recipe.rs` for
    // why absence is not a state this block has.
    Control {
        name: "defringe_purple",
        shape: Shape::Number,
        range: Some((0.0, 20.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("DefringePurpleAmount"),
        tier: Some(Tier::Rendered),
        purpose: "how hard Lightroom removes purple fringing",
    },
    Control {
        name: "defringe_purple_lo",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "30 (Adobe's own default — this block has no absent state)",
        engine_only: true,
        crs: CrsKey::Attr("DefringePurpleHueLo"),
        tier: Some(Tier::Rendered),
        purpose: "the low end of the purple hue window de-fringe acts in",
    },
    Control {
        name: "defringe_purple_hi",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "70 (Adobe's own default)",
        engine_only: true,
        crs: CrsKey::Attr("DefringePurpleHueHi"),
        tier: Some(Tier::Rendered),
        purpose: "the high end of the purple hue window",
    },
    Control {
        name: "defringe_green",
        shape: Shape::Number,
        range: Some((0.0, 20.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("DefringeGreenAmount"),
        tier: Some(Tier::Rendered),
        purpose: "how hard Lightroom removes green fringing",
    },
    Control {
        name: "defringe_green_lo",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "40 (Adobe's own default)",
        engine_only: true,
        crs: CrsKey::Attr("DefringeGreenHueLo"),
        tier: Some(Tier::Rendered),
        purpose: "the low end of the green hue window",
    },
    Control {
        name: "defringe_green_hi",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "60 (Adobe's own default)",
        engine_only: true,
        crs: CrsKey::Attr("DefringeGreenHueHi"),
        tier: Some(Tier::Rendered),
        purpose: "the high end of the green hue window",
    },
    // The nine Effects. Carried and unrendered from R25 B2 until v1.5.0, when
    // the engine grew a stage after the crop (`render/finish.rs`) and they
    // became `Tier::Rendered` like everything else a slider can move.
    // Engine-only until 2026-10-21; nullable now — a vignette and a grain are
    // looks a vision model can ask for, and null leaves the photographer's.
    Control {
        name: "post_crop_vignette",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("PostCropVignetteAmount"),
        tier: Some(Tier::Rendered),
        purpose: "creative corner darkening applied AFTER the crop",
    },
    Control {
        name: "post_crop_vignette_mid",
        shape: Shape::NullableNumber,
        range: Some((0.0, 100.0)),
        neutral: "null = no opinion (0 = absent (Lightroom's own default is 50))",
        engine_only: false,
        crs: CrsKey::Attr("PostCropVignetteMidpoint"),
        tier: Some(Tier::Rendered),
        purpose: "how far toward the centre the post-crop vignette reaches",
    },
    Control {
        name: "post_crop_vignette_feather",
        shape: Shape::NullableNumber,
        range: Some((0.0, 100.0)),
        neutral: "null = no opinion (0 = absent (Lightroom's own default is 50))",
        engine_only: false,
        crs: CrsKey::Attr("PostCropVignetteFeather"),
        tier: Some(Tier::Rendered),
        purpose: "how soft the post-crop vignette's edge is",
    },
    Control {
        name: "post_crop_vignette_round",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("PostCropVignetteRoundness"),
        tier: Some(Tier::Rendered),
        purpose: "how circular (positive) or rectangular (negative) the post-crop vignette is",
    },
    Control {
        name: "post_crop_vignette_style",
        shape: Shape::NullableNumber,
        range: Some((0.0, 3.0)),
        neutral: "null = no opinion (0 = absent (Lightroom's own default is 1, Highlight Priority))",
        engine_only: false,
        crs: CrsKey::Attr("PostCropVignetteStyle"),
        tier: Some(Tier::Rendered),
        purpose: "WHICH post-crop vignette operator Adobe applies: 1 = Highlight Priority, \
                  2 = Colour Priority, 3 = Paint Overlay",
    },
    Control {
        name: "post_crop_vignette_hl",
        shape: Shape::NullableNumber,
        range: Some((0.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("PostCropVignetteHighlightContrast"),
        tier: Some(Tier::Rendered),
        purpose: "how far a DARKENING post-crop vignette spares the highlights inside it",
    },
    Control {
        name: "grain",
        shape: Shape::NullableNumber,
        range: Some((0.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GrainAmount"),
        tier: Some(Tier::Rendered),
        purpose: "film-grain amount",
    },
    Control {
        name: "grain_size",
        shape: Shape::NullableNumber,
        range: Some((0.0, 100.0)),
        neutral: "null = no opinion (0 = absent (Lightroom's own default is 25))",
        engine_only: false,
        crs: CrsKey::Attr("GrainSize"),
        tier: Some(Tier::Rendered),
        purpose: "how coarse each grain particle is",
    },
    Control {
        name: "grain_rough",
        shape: Shape::NullableNumber,
        range: Some((0.0, 100.0)),
        neutral: "null = no opinion (0 = absent (Lightroom's own default is 50))",
        engine_only: false,
        crs: CrsKey::Attr("GrainFrequency"),
        tier: Some(Tier::Rendered),
        purpose: "how irregular the grain pattern is",
    },
    Control {
        // v1.5.0. Not a control: one bit per COMPANION row saying its stored
        // 0 is a value (Detail 0) rather than "the sidecar said nothing"
        // (`EditRecipe::explicit_zero`, read through `EditRecipe::resolved`).
        // `EngineCarrier` — a model has nothing to say about which zeros are
        // meant — and it rides a Refine with the companions it qualifies
        // (`pipeline::carry_over_unrepresentable`).
        name: "explicit_zero",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "empty = every companion's stored 0 renders at Lightroom's default",
        engine_only: true,
        crs: CrsKey::None,
        tier: None,
        purpose: "which companion controls hold a real 0 rather than an absent value",
    },
    Control {
        name: "straighten_deg",
        shape: Shape::Number,
        range: Some((-45.0, 45.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Attr("CropAngle"),
        tier: Some(Tier::Rendered),
        purpose: "clockwise straighten angle in degrees, for a tilted horizon",
    },
    Control {
        name: "quarter_turns",
        shape: Shape::Integer,
        range: Some((0.0, 3.0)),
        neutral: "0 = no turn",
        // ENGINE-ONLY, and for a stronger reason than `coord_era`'s: the model
        // is shown the photo ALREADY in its display frame, so "how far should
        // this be turned" is a question whose answer it can only get wrong —
        // and getting it wrong turns the user's crop and every mask with it.
        // Which way is up is the photographer's call, taken with a toolbar
        // button, not a proposal.
        engine_only: true,
        // NOT `CropAngle` (that is `straighten_deg`, ±45): Lightroom expresses
        // a quarter turn as `tiff:Orientation`, which is not a `crs:` key and
        // so has no place in this registry. R27 Batch-3 (A8) DOES write it now
        // — `xmp::frame_declaration` emits the COMPOSED orientation (the
        // camera's EXIF state ∘ these quarter turns), which is what closes the
        // 「a sidecar we write describes the UNTURNED frame」 gap registered in
        // ROADMAP. What stays true, and is why this row is still
        // `CrsKey::None`: the turn cannot be recovered from the sidecar on the
        // way back (classic ACR has nowhere to put it), so a re-import reports
        // no turn and the authoritative restore path is the store's
        // `recipe.json`.
        crs: CrsKey::None,
        tier: None,
        purpose: "engine bookkeeping: the photographer's own clockwise quarter turns (0..3), \
                  composed with the camera's EXIF orientation into the one frame the render, \
                  the crop and every mask geometry live in",
    },
    Control {
        name: "crop",
        shape: Shape::NullableCrop,
        range: None,
        neutral: "null = the whole frame",
        engine_only: false,
        crs: CrsKey::Family("HasCrop + Crop{Left,Top,Right,Bottom}"),
        tier: Some(Tier::Rendered),
        purpose: "the kept rectangle in normalised 0..1 frame coordinates; null keeps the full frame",
    },
    Control {
        name: "tone_curve",
        shape: Shape::Curve,
        range: None,
        neutral: "empty = identity",
        engine_only: false,
        crs: CrsKey::Family("ToneCurvePV2012"),
        tier: Some(Tier::Rendered),
        purpose: "master tone curve as {input,output} points, both 0..255 — the free-form \
                  alternative to the Contrast slider",
    },
    Control {
        name: "red_curve",
        shape: Shape::Curve,
        range: None,
        neutral: "empty = identity",
        engine_only: false,
        crs: CrsKey::Family("ToneCurvePV2012Red"),
        tier: Some(Tier::Rendered),
        purpose: "per-channel RED curve (same point form as tone_curve) for a deliberate colour \
                  cast in specific tones",
    },
    Control {
        name: "green_curve",
        shape: Shape::Curve,
        range: None,
        neutral: "empty = identity",
        engine_only: false,
        crs: CrsKey::Family("ToneCurvePV2012Green"),
        tier: Some(Tier::Rendered),
        purpose: "per-channel GREEN curve",
    },
    Control {
        name: "blue_curve",
        shape: Shape::Curve,
        range: None,
        neutral: "empty = identity",
        engine_only: false,
        crs: CrsKey::Family("ToneCurvePV2012Blue"),
        tier: Some(Tier::Rendered),
        purpose: "per-channel BLUE curve",
    },
    // v1.5.0 — Lightroom's PARAMETRIC tone curve, rendered by
    // `render::parametric_lut` between the Basic-panel tone model and the point
    // curve above. Engine-only: the model already has `tone_curve`, which can
    // draw every shape these seven can and more, so asking for both would ask
    // one question twice — and, the fields being required, a response would
    // return zeros that replace the photographer's own curve on every Refine.
    Control {
        name: "param_shadows",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("ParametricShadows"),
        tier: Some(Tier::Rendered),
        purpose: "parametric curve, Shadows region: lifts (+) or sinks (−) the darkest tones",
    },
    Control {
        name: "param_darks",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("ParametricDarks"),
        tier: Some(Tier::Rendered),
        purpose: "parametric curve, Darks region: the tones between the shadow and midtone splits",
    },
    Control {
        name: "param_lights",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("ParametricLights"),
        tier: Some(Tier::Rendered),
        purpose: "parametric curve, Lights region: the tones between the midtone and highlight splits",
    },
    Control {
        name: "param_highlights",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("ParametricHighlights"),
        tier: Some(Tier::Rendered),
        purpose: "parametric curve, Highlights region: the brightest tones",
    },
    // The three SPLITS. Each stated range is the split's own band — where its
    // slider reaches with the other two at rest (`recipe::parametric_split_band`)
    // — because that is what `EditRecipe::clamp` enforces field by field; the
    // ordering between them is resolved where the curve uses them
    // (`EditRecipe::parametric_splits`).
    Control {
        name: "param_shadow_split",
        shape: Shape::Number,
        range: Some((10.0, 70.0)),
        neutral: "25 (Adobe's default)",
        engine_only: true,
        crs: CrsKey::Attr("ParametricShadowSplit"),
        tier: Some(Tier::Rendered),
        purpose: "where the parametric Shadows region ends and Darks begins, in percent of the tonal range",
    },
    Control {
        name: "param_midtone_split",
        shape: Shape::Number,
        range: Some((20.0, 80.0)),
        neutral: "50 (Adobe's default)",
        engine_only: true,
        crs: CrsKey::Attr("ParametricMidtoneSplit"),
        tier: Some(Tier::Rendered),
        purpose: "where the parametric Darks region ends and Lights begins, in percent of the tonal range",
    },
    Control {
        name: "param_highlight_split",
        shape: Shape::Number,
        range: Some((30.0, 90.0)),
        neutral: "75 (Adobe's default)",
        engine_only: true,
        crs: CrsKey::Attr("ParametricHighlightSplit"),
        tier: Some(Tier::Rendered),
        purpose: "where the parametric Lights region ends and Highlights begins, in percent of the tonal range",
    },
    Control {
        // v1.5.0 F7 — the name Lightroom developed the photograph through.
        // It rode to the sidecar through `passthrough` until this batch and
        // moved no pixel; `crate::dcp` now reads Adobe's own profile file out
        // of the user's install and the render applies its tables.
        name: "camera_profile",
        shape: Shape::Text,
        range: None,
        neutral: "empty = no profile named, and the RAW's own matrix stands",
        engine_only: true,
        crs: CrsKey::Attr("CameraProfile"),
        tier: Some(Tier::Rendered),
        purpose: "the Adobe camera profile this photograph is developed through (\"Adobe \
                  Standard\", \"Camera Vivid\"); rendered from the .dcp installed on this \
                  machine, never from a bundled copy",
    },
    Control {
        // Read from `<crs:Look>`, rendered, and — since v1.17.0, when the
        // fresh canvas starts from 「Adobe Color」 — WRITTEN too: a fresh
        // document and a merge onto a base without a Look get the element
        // (`xmp::write::look_elem`); a base's own Look keeps its verbatim
        // merge path, which is stricter than re-serialising it.
        name: "look",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "absent = no creative profile",
        engine_only: true,
        crs: CrsKey::Family("<crs:Look> element"),
        tier: Some(Tier::Rendered),
        purpose: "the creative profile — its colour table (read from Camera Raw's install), \
                  its tone curves, its baked slider moves and its black-and-white switch",
    },
    Control {
        name: "base_curve",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "empty = no base look",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::RenderedNotExported),
        purpose: "camera-matched base-look knots estimated per photo by the engine (calibration, \
                  never exported to XMP)",
    },
    Control {
        name: "lens_profile",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "empty = every component off",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::RenderedNotExported),
        purpose: "the in-camera lens profile read from the RAW's own metadata (calibration, never \
                  exported to XMP)",
    },
    Control {
        // R25 B4 — the FIRST member `Tier::PassThrough` has ever had, and the
        // batch the tier's own doc named as the one allowed to give it one.
        //
        // One row for nine `crs:` keys, which is why the shape is a
        // carrier: the value is a key → verbatim-string map, not a number the
        // schema could ask for. `EngineCarrier` is what keeps it out of the
        // advisor's response schema for good (`engine_carriers_never_reach_
        // the_schema`) — and that is not a scheduling decision. There is
        // nothing here for a model to decide: we do not know what these values
        // MEAN, which is the definition of the tier.
        name: "passthrough",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "empty = the document named no camera profile and carried no Upright bookkeeping",
        engine_only: true,
        crs: CrsKey::Family("CameraProfile + Upright* bookkeeping (xmp::PASSTHROUGH_CRS)"),
        tier: Some(Tier::PassThrough),
        purpose: "the camera profile's NAME and the bookkeeping Lightroom's own Upright solver \
                  leaves behind (its version stamp, assumed centre and focal length, cache digest), \
                  carried between the sidecar and recipe.json verbatim and never interpreted",
    },
    // v1.5.0 — Lightroom's Calibration panel, rendered as the develop's first
    // stage (`render::apply_calibration`). Engine-only on the parametric
    // curve's ground, and on one of its own: a primary belongs to the camera
    // profile, which the advisor's preview has already been rendered through.
    // v1.5.0 F8 — Lightroom's HDR edit mode and the seven-control SDR panel it
    // shows there, rendered as the develop's LAST stage (`render::hdr`).
    // Engine-only, all nine, and on their own ground: these describe the RANGE
    // the photograph was edited in and how it is to be mapped into the one this
    // engine can publish. A model asked for an SDR Highlights would be setting
    // a control that only exists when a flag it cannot check is on, and setting
    // `hdr_edit` itself would be making a claim about the capture.
    Control {
        name: "hdr_edit",
        shape: Shape::Bool,
        // A switch, not a band — `convert_to_grayscale`'s rule.
        range: None,
        neutral: "false",
        engine_only: true,
        crs: CrsKey::Attr("HDREditMode"),
        tier: Some(Tier::Rendered),
        purpose: "was this photograph edited in Lightroom's HDR mode: the gate the seven SDR \
                  controls below render behind",
    },
    Control {
        name: "hdr_max_ev",
        shape: Shape::Number,
        range: Some((0.0, 8.0)),
        neutral: "0 = no headroom above diffuse white",
        engine_only: true,
        crs: CrsKey::Attr("HDRMaxValue"),
        tier: Some(Tier::Rendered),
        purpose: "how many stops of highlight headroom sit above diffuse white, which sets how \
                  far the SDR rendition's shoulder reaches",
    },
    Control {
        name: "sdr_blend",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("SDRBlend"),
        tier: Some(Tier::Rendered),
        purpose: "how much of the headroom's shoulder reaches the SDR rendition: −100 none, \
                  0 the model's own amount, +100 twice it",
    },
    Control {
        name: "sdr_brightness",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("SDRBrightness"),
        tier: Some(Tier::Rendered),
        purpose: "the SDR rendition's exposure — named Brightness because Lightroom's SDR panel \
                  has no Exposure slider",
    },
    Control {
        name: "sdr_contrast",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("SDRContrast"),
        tier: Some(Tier::Rendered),
        purpose: "the SDR rendition's contrast",
    },
    Control {
        name: "sdr_highlights",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("SDRHighlights"),
        tier: Some(Tier::Rendered),
        purpose: "the SDR rendition's highlights, on top of the shoulder the headroom already put \
                  there",
    },
    Control {
        name: "sdr_shadows",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("SDRShadows"),
        tier: Some(Tier::Rendered),
        purpose: "the SDR rendition's shadows",
    },
    Control {
        name: "sdr_whites",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("SDRWhites"),
        tier: Some(Tier::Rendered),
        purpose: "the SDR rendition's white point",
    },
    Control {
        name: "sdr_clarity",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: true,
        crs: CrsKey::Attr("SDRClarity"),
        tier: Some(Tier::Rendered),
        purpose: "the SDR rendition's local contrast, through the Basic panel's own Clarity \
                  operator at the same radius",
    },
    Control {
        name: "cal_shadow_tint",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("ShadowTint"),
        tier: Some(Tier::Rendered),
        purpose: "Calibration, shadows tint: pushes the darkest tones toward green (−) or magenta (+)",
    },
    Control {
        name: "cal_red_hue",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("RedHue"),
        tier: Some(Tier::Rendered),
        purpose: "Calibration, red primary hue: turns every colour the camera's red primary builds",
    },
    Control {
        name: "cal_red_sat",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("RedSaturation"),
        tier: Some(Tier::Rendered),
        purpose: "Calibration, red primary saturation: strengthens (+) or mutes (−) every colour it builds",
    },
    Control {
        name: "cal_green_hue",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GreenHue"),
        tier: Some(Tier::Rendered),
        purpose: "Calibration, green primary hue: turns every colour the camera's green primary builds",
    },
    Control {
        name: "cal_green_sat",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("GreenSaturation"),
        tier: Some(Tier::Rendered),
        purpose: "Calibration, green primary saturation: strengthens (+) or mutes (−) every colour it builds",
    },
    Control {
        name: "cal_blue_hue",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("BlueHue"),
        tier: Some(Tier::Rendered),
        purpose: "Calibration, blue primary hue: turns every colour the camera's blue primary builds",
    },
    Control {
        name: "cal_blue_sat",
        shape: Shape::NullableNumber,
        range: Some((-100.0, 100.0)),
        neutral: "null = no opinion (0 = none)",
        engine_only: false,
        crs: CrsKey::Attr("BlueSaturation"),
        tier: Some(Tier::Rendered),
        purpose: "Calibration, blue primary saturation: strengthens (+) or mutes (−) every colour it builds",
    },
    Control {
        name: "masks",
        shape: Shape::Masks,
        range: None,
        neutral: "empty = global only",
        engine_only: false,
        crs: CrsKey::Family("MaskGroupBasedCorrections"),
        tier: Some(Tier::Rendered),
        purpose: "local (masked) adjustments — one entry per mask, controls listed below",
    },
    Control {
        // v1.5.0 F9. `RenderedNotExported`, and the tier is the interesting
        // part of this row, because the naive reading of the round trip says
        // `Rendered`: open a photograph whose sidecar carries
        // `crs:RetouchAreas`, save it back through the merge, and the block is
        // still there, byte for byte. It survives because this writer does not
        // OWN it (`xmp::OWNED_ELEMENT_ONLY` names six elements and this is not
        // one), so the merge never strips it and never rewrites it.
        //
        // Not owning it is exactly why the tier cannot be `Rendered`.
        // `Tier::owns_crs_key` is checked against the writer, and a fresh
        // `recipe_to_xmp` — the Analyze writer's path, and every surface that
        // writes a sidecar where none existed — emits nothing for this field.
        // On that path the removals are gone. `RenderedNotExported` is what
        // makes `xmp::global_export_losses` say so instead of letting a
        // photographer discover it in the exported file.
        //
        // `EngineCarrier`/`engine_only` for the reason `colour_field` is: a
        // retouch area is a geometry read off the photographer's own file, and
        // there is nothing an advisor could say about one that would be an
        // answer — a required schema field would return an empty list and
        // delete the photographer's removals on every Refine.
        name: "retouch",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "empty = nothing removed",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::RenderedNotExported),
        purpose: "spot removal imported from Lightroom — each area re-solved from this frame's own pixels",
    },
    Control {
        // 2026-10-01. A generative fill's layer: an RGBA patch over the
        // finished develop (`render::PixelLayer`). `RenderedNotExported` for
        // `retouch`'s reason — classic XMP has no element for it, so the save
        // line names it — and `EngineCarrier`/`engine_only` because it is
        // pixels a model made, not a value an advisor could state: a required
        // schema field would come back empty and delete the layers on every
        // Refine.
        name: "pixel_layers",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "empty = no layers",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::RenderedNotExported),
        purpose: "generative-fill layers composited over the finished develop",
    },
    Control {
        // 2026-10-13. The AI lens blur: a depth map MoGe-2 estimated for this
        // photo, a focus point and two dials (`render::LensBlur`).
        // `RenderedNotExported` for `pixel_layers`' reason — classic XMP has
        // no element for a depth-driven blur, so the save line names it — and
        // `EngineCarrier`/`engine_only` because a depth map is a measurement
        // of THIS photo's scene, not a value an advisor could state: a
        // required schema field would come back empty and delete it on every
        // Refine.
        name: "lens_blur",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "absent = no blur",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::RenderedNotExported),
        purpose: "AI lens blur from an estimated depth map, focused where the photographer clicked",
    },
    Control {
        // R33 §G. The THIRD global `RenderedNotExported` row, beside the two
        // calibration carriers — and the first one that is an EDIT rather than
        // a measurement of the photo. A 12×32×8 bilateral grid of local colour
        // and tone deltas is a coordinate system classic XMP does not have, so
        // there is no `crs:` key to lose it to and no partial projection worth
        // making: the sidecar carries the rest of the recipe unchanged and the
        // save line NAMES the field as the part it could not take.
        //
        // `EngineCarrier`, so the advisor's response schema never asks for it:
        // nothing the model could say about ninety-six vertices would be an
        // answer, and a required field would return an empty grid and delete a
        // solved one on every Refine. It rides a refine through
        // `pipeline::carry_over_unrepresentable` like the lens profile does.
        name: "colour_field",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "absent = no field",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::RenderedNotExported),
        purpose: "smooth 12x32x8 local colour/tone field solved by the reverse fit (rendered in-app, never exported to XMP)",
    },
    Control {
        name: "rationale",
        shape: Shape::Text,
        range: None,
        neutral: "\"\"",
        engine_only: false,
        crs: CrsKey::None,
        tier: None,
        purpose: "one or two sentences explaining these choices, for the photographer to \
                  sanity-check",
    },
    Control {
        name: "confidence",
        shape: Shape::Number,
        range: Some((0.0, 1.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::None,
        tier: None,
        purpose: "your own confidence in this develop, 0..1",
    },
];

/// Every field of [`LocalAdjustment`], in DECLARATION order — the nested
/// schema mirror. Its field set is NOT a subset of the global one
/// (`temperature` is a relative shift, not Kelvin; `hue` rotates every colour
/// rather than one band), which is exactly why the drift had two independent
/// surfaces. `texture` used to be the clearest example and is no longer one:
/// R25 B2 gave it a global row too, sharing this one's operator and radius.
pub const LOCAL_CONTROLS: [Control; 30] = [
    // KNOWN GRANULARITY LIMIT (stated, not abstracted away). `Tier` hangs on
    // a Control ROW, and the whole geometry is one row — so the row says
    // `Rendered` while three of `MaskGeometry::Radial`'s fields are not:
    // `roundness` and (since R25 P5) `midpoint` are CARRIED — round-tripped
    // through recipe.json and the sidecar, never consulted by `mask_weight`,
    // because neither one's Lightroom scale has been measured; `angle` is the
    // mirror case, RENDERED here and deliberately absent from the sidecar
    // (`xmp::MaskLossReason::Rotation` discloses that, with the angle). A
    // sub-field tier would be a new abstraction built for one row — the
    // limitation is cheaper written down, and the two disclosure surfaces
    // (`mask_export_losses`, `import_losses`) already name the real losses
    // per mask, which is what a tier would only summarise.
    Control {
        name: "mask",
        shape: Shape::MaskGeometry,
        range: None,
        neutral: "a linear gradient",
        engine_only: false,
        crs: CrsKey::Family("CorrectionMasks/Mask"),
        tier: Some(Tier::Rendered),
        // `roundness` is told as "leave it at 0" on purpose. Its domain is
        // Lightroom's ±100 slider (v0.31.1), but this engine carries the value
        // without rendering it — so a model that authored a real one would
        // write a number into the sidecar that Lightroom honours and the
        // preview does not, i.e. it would invent a WYSIWYG gap. Saying 0 keeps
        // the AI where every radial in the reference library already sits.
        purpose: "WHERE this adjustment applies: kind=ai (what=sky, what=subject — the main \
                  subject — or what=object under the point ref_x/ref_y, in 0..1 frame coords; \
                  for people what=face, what=eyes or what=background — everything but the \
                  person — found on the photo itself, so their ref_x/ref_y are not read; all \
                  cut locally after you answer) to select a part of the picture by what it IS; \
                  kind=linear (zero_* = the no-effect edge, full_* = the \
                  full-effect edge, in 0..1 frame coords) for a gradient; or kind=radial \
                  (top/left/bottom/right + feather 0..1, roundness 0 — a carried Lightroom \
                  value this engine does not render, so leave it at 0 — flipped, and angle in \
                  DEGREES counter-clockwise about the ellipse centre, 0 = axis-aligned — \
                  rotate it to follow an oblique subject) for a vignette or a soft spot. \
                  Add `range` to confine any of them by luminance or colour",
    },
    Control {
        name: "components",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "empty = the base geometry alone",
        engine_only: true,
        crs: CrsKey::Family("CorrectionMasks / MaskBlendMode"),
        tier: Some(Tier::Rendered),
        purpose: "extra Add/Subtract/Intersect shapes composed onto `mask` — Lightroom-native \
                  except Bitmap components; keep an existing mask's `name` byte-exact to preserve them",
    },
    Control {
        name: "enabled",
        shape: Shape::Bool,
        range: None,
        neutral: "true",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::RenderedNotExported),
        purpose: "the per-mask eye toggle (mutes the mask without destroying its tuned Amount)",
    },
    // WHY `enabled` STAYS ENGINE-ONLY (R23-1b, weighed and declined).
    //
    // Every value the model could put in this field is either discarded or
    // meaningless, while one of them is destructive:
    //   * a NEW mask — `LocalAdjustment::default()` is already `enabled: true`,
    //     and a model that authors a mask it immediately mutes has authored
    //     nothing. Zero gain.
    //   * an EXISTING mask — the photographer's mute is theirs. A muted mask is
    //     already "state-bearing" for `carry_over_unrepresentable`
    //     (`schema_loses` tests `!m.enabled`), so a refine restores it by name;
    //     letting the model set the field instead would hand it the power to
    //     silence a mask the user deliberately kept, and any guard that stops it
    //     (restore the user's value for every name-matched mask) makes the
    //     field's value discarded anyway.
    // Deleting a mask, the request this field might look like it serves, is
    // already expressible: the model omits it from `masks`.
    Control {
        name: "range",
        shape: Shape::NullableRangeMask,
        range: None,
        neutral: "null = pure geometry",
        engine_only: false,
        crs: CrsKey::Family("CorrectionMasks/Mask/RangeMask"),
        tier: Some(Tier::Rendered),
        purpose: "optional Range-Mask refinement INTERSECTED with the geometry: \
                  {kind:\"luminance\", lo_outer<=lo<=hi<=hi_outer in 0..1} keeps only that \
                  brightness band, {kind:\"color\", r,g,b 0..1 reference, amount 0..1 tolerance, \
                  px,py sample point} keeps only similar colours",
    },
    Control {
        name: "name",
        shape: Shape::Text,
        range: None,
        neutral: "\"\"",
        engine_only: false,
        crs: CrsKey::Family("CorrectionName + MaskName"),
        tier: Some(Tier::CarriedOnly),
        purpose: "the mask's IDENTITY, not decoration — when refining an edit that already has \
                  masks, return each existing name byte-exact or your mask edits are discarded",
    },
    Control {
        name: "amount",
        shape: Shape::Number,
        range: Some((0.0, 1.0)),
        neutral: "1",
        engine_only: false,
        crs: CrsKey::Family("CorrectionAmount"),
        tier: Some(Tier::Rendered),
        purpose: "master opacity of this mask's whole adjustment",
    },
    Control {
        name: "inverted",
        shape: Shape::Bool,
        range: None,
        neutral: "false",
        engine_only: false,
        crs: CrsKey::Family("MaskInverted"),
        tier: Some(Tier::Rendered),
        purpose: "invert the mask region",
    },
    Control {
        name: "exposure_ev",
        shape: Shape::Number,
        range: Some((-5.0, 5.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalExposure2012"),
        tier: Some(Tier::Rendered),
        purpose: "local exposure in stops",
    },
    Control {
        name: "contrast",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalContrast2012"),
        tier: Some(Tier::Rendered),
        purpose: "local contrast",
    },
    Control {
        name: "highlights",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalHighlights2012"),
        tier: Some(Tier::Rendered),
        purpose: "local highlights",
    },
    Control {
        name: "shadows",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalShadows2012"),
        tier: Some(Tier::Rendered),
        purpose: "local shadows",
    },
    Control {
        name: "whites",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalWhites2012"),
        tier: Some(Tier::Rendered),
        purpose: "local white point",
    },
    Control {
        name: "blacks",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalBlacks2012"),
        tier: Some(Tier::Rendered),
        purpose: "local black point",
    },
    Control {
        name: "clarity",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalClarity2012"),
        tier: Some(Tier::Rendered),
        purpose: "local midtone contrast",
    },
    Control {
        name: "dehaze",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalDehaze"),
        tier: Some(Tier::Rendered),
        purpose: "local haze removal",
    },
    Control {
        name: "texture",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalTexture"),
        tier: Some(Tier::Rendered),
        // R25 B2 gave this control its global twin, and the two share ONE
        // radius model — so the sentence that used to say "there is no global
        // Texture" now says the opposite, in the same place the model reads.
        purpose: "local FINE-detail contrast (smaller radius than clarity) — the masked form of \
                  the global `texture`, same scale and same radius",
    },
    Control {
        name: "sharpness",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalSharpness"),
        tier: Some(Tier::Rendered),
        purpose: "local capture sharpening at the same radius as the global `sharpening` — but on \
                  ACR's own LOCAL scale, where the band is SIGNED: positive sharpens, NEGATIVE \
                  SOFTENS (blurs) inside the mask, which is how a background is thrown back",
    },
    Control {
        name: "saturation",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalSaturation"),
        tier: Some(Tier::Rendered),
        purpose: "local saturation",
    },
    Control {
        name: "hue",
        shape: Shape::Number,
        range: Some((-180.0, 180.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalHue"),
        tier: Some(Tier::Rendered),
        purpose: "local HUE ROTATION of every colour inside the mask (±100 = ±30°, the same scale \
                  as the global mixer's hue axis) — for turning one region's colour without \
                  touching the same colour elsewhere in the frame",
    },
    Control {
        name: "temperature",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalTemperature"),
        tier: Some(Tier::Rendered),
        purpose: "local warm/cool shift — a RELATIVE ±100 shift, NOT Kelvin (the global \
                  temperature_k is the only absolute one)",
    },
    Control {
        name: "tint",
        shape: Shape::Number,
        range: Some((-100.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalTint"),
        tier: Some(Tier::Rendered),
        purpose: "local green/magenta shift",
    },
    Control {
        name: "toning_hue",
        shape: Shape::Number,
        range: Some((0.0, 360.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalToningHue"),
        tier: Some(Tier::Rendered),
        purpose: "the hue (0..360°, Lightroom's wheel) of the mask's colour TINT — Lightroom's mask \"Color\" swatch; inert until `toning_amount` is set",
    },
    Control {
        name: "toning_amount",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalToningSaturation"),
        tier: Some(Tier::Rendered),
        purpose: "strength of the mask's colour tint toward `toning_hue`: shifts every pixel in the mask, neutral or coloured, the same way toward that one hue, most in the midtones, without brightening or darkening — unlike `hue`, which rotates colours by an angle",
    },
    Control {
        name: "noise_reduction",
        shape: Shape::Number,
        range: Some((0.0, 100.0)),
        neutral: "0",
        engine_only: false,
        crs: CrsKey::Family("LocalLuminanceNoise"),
        tier: Some(Tier::Rendered),
        purpose: "local luminance noise reduction — for a \"this region is noisy\" request",
    },
    // The four LOCAL point curves (R25 P6). Lightroom spells them
    // `crs:MainCurve` / `RedCurve` / `GreenCurve` / `BlueCurve` — BARE names,
    // no `PV2012` suffix, unlike the global four this table already carries —
    // as child elements of the Correction, and writes them SPARSELY (a real
    // sidecar carries Red and Green with neither Main nor Blue). Hence four
    // independent rows with the same `Shape::Curve` the globals use, and not
    // one composite row: the row set is the field set.
    //
    // `Tier::Rendered` on all four: `render::apply_masks` feeds `main_curve`
    // through the very `build_tone_lut` the global master curve goes through
    // and the other three through `curve_lut` in the same fused pixel loop,
    // and `xmp::masks_xml` emits each non-empty one inside the correction.
    Control {
        name: "main_curve",
        shape: Shape::Curve,
        range: None,
        neutral: "empty = identity",
        engine_only: false,
        crs: CrsKey::Family("MainCurve"),
        tier: Some(Tier::Rendered),
        purpose: "this mask's own master tone curve as {input,output} points, both 0..255 — the \
                  free-form shape the local Contrast/Highlights sliders cannot draw; leave it \
                  empty unless the region needs a tonal move no slider expresses",
    },
    Control {
        name: "red_curve",
        shape: Shape::Curve,
        range: None,
        neutral: "empty = identity",
        engine_only: false,
        crs: CrsKey::Family("RedCurve"),
        tier: Some(Tier::Rendered),
        purpose: "this mask's RED channel curve (same point form as main_curve), for a colour \
                  cast confined to one region and one tonal band",
    },
    Control {
        name: "green_curve",
        shape: Shape::Curve,
        range: None,
        neutral: "empty = identity",
        engine_only: false,
        crs: CrsKey::Family("GreenCurve"),
        tier: Some(Tier::Rendered),
        purpose: "this mask's GREEN channel curve",
    },
    Control {
        name: "blue_curve",
        shape: Shape::Curve,
        range: None,
        neutral: "empty = identity",
        engine_only: false,
        crs: CrsKey::Family("BlueCurve"),
        tier: Some(Tier::Rendered),
        purpose: "this mask's BLUE channel curve",
    },
    Control {
        name: "color_gains",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "null = neutral",
        engine_only: true,
        crs: CrsKey::None,
        tier: Some(Tier::RenderedNotExported),
        purpose: "per-channel LINEAR-light gains for zone recolouring, written by the reverse-fit \
                  (engine-only: no classic-ACR counterpart exists)",
    },
    Control {
        name: "role",
        shape: Shape::EngineCarrier,
        range: None,
        neutral: "custom",
        engine_only: true,
        crs: CrsKey::None,
        tier: None,
        purpose: "which semantic zone produced this mask (engine-only stable identity)",
    },
];

// ── the nested object shapes ────────────────────────────────────────────────
// These spell out types the recipe composes (Hsl, ColorGrade, Crop,
// CurvePoint, MaskGeometry, RangeMask) rather than fields of the recipe
// itself, so they stay shape definitions instead of registry rows. Their
// field sets are pinned to serde in this module's tests, and
// `color_grade_value` / `hsl_value` give the two field-bearing ones the same
// exhaustive-destructure treatment the top-level structs get.

/// MaskGeometry tagged enum (`#[serde(tag="kind")]`) → anyOf of the two
/// variants the AI may author; each is strict (all props required,
/// `additionalProperties:false`). `Bitmap` is deliberately ABSENT: a raster
/// mask is a file the engine writes (AI segmentation, painting), never
/// something a text response can name.
///
/// `Radial::angle` IS here since R23-1b. Its absence used to be justified by
/// the XMP omission ("our own field, unverified against `crs:Angle`"), but the
/// two questions are separate — and v0.32.0 closed the XMP one anyway: the
/// sidecar now carries the rotation, folded from this engine's normalised-frame
/// convention into Lightroom's pixel-frame one (`xmp::engine_to_lr`), and
/// `xmp::MaskLossReason::Rotation` is down to the case where the document
/// declares no frame to fold against. THIS
/// schema is the engine's own contract — `render::mask_weight` rotates the
/// ellipse by exactly this field, the GUI edits it, and `recipe.json`
/// round-trips it. Withholding it meant an oblique subject could only ever get
/// an axis-aligned ellipse from the model, and every refine of a rotated mask
/// had to be repaired by `carry_over_unrepresentable`.
fn mask_geometry_schema() -> Value {
    let num = || json!({"type": "number"});
    json!({
        "anyOf": [
            {"type": "object", "additionalProperties": false,
             "required": ["kind","zero_x","zero_y","full_x","full_y"],
             "properties": {"kind": {"type": "string", "enum": ["linear"]},
                "zero_x": num(), "zero_y": num(), "full_x": num(), "full_y": num()}},
            {"type": "object", "additionalProperties": false,
             "required": ["kind","top","left","bottom","right","feather","roundness","flipped",
                "angle"],
             "properties": {"kind": {"type": "string", "enum": ["radial"]},
                "top": num(), "left": num(), "bottom": num(), "right": num(),
                "feather": num(), "roundness": num(), "flipped": {"type": "boolean"},
                "angle": num()}},
            // A part of the picture named by its CONTENT (2026-10-21): the sky,
            // the main subject, or the object under a point. The model asks;
            // the local segmenter cuts the alpha before anything renders it
            // (`openai::expand_ai_mask_intents`, `segment::resolve_ai_masks`).
            // The same three targets `python/segment.py` serves the GUI's own
            // Select Sky / Select Subject / point-prompted object, and since
            // v1.14.0 the Portraits mode's face / eyes (`python/face.py`,
            // `face::resolve_face_masks`) and background (the subject cut,
            // inverted).
            {"type": "object", "additionalProperties": false,
             "required": ["kind","what","ref_x","ref_y"],
             "properties": {"kind": {"type": "string", "enum": ["ai"]},
                "what": {"type": "string", "enum": AI_MASK_TARGETS},
                "ref_x": num(), "ref_y": num()}}
        ]
    })
}

/// The content targets an `ai` mask may name, in the schema's enum order:
/// `sky` (OneFormer), `subject` (BiRefNet), `object` (SAM 2.1 at the point),
/// and the Portraits mode's `face` and `eyes` (MediaPipe's Face Landmarker)
/// and `background` (BiRefNet's subject, inverted) — none of these three
/// reads the model's point.
pub const AI_MASK_TARGETS: [&str; 6] = ["sky", "subject", "object", "face", "eyes", "background"];

/// RangeMask tagged enum (`#[serde(tag="kind")]`) → anyOf of the two variants
/// + null (strict mode requires the field to be PRESENT, so "no range" = null).
fn range_mask_schema() -> Value {
    let num = || json!({"type": "number"});
    json!({
        "anyOf": [
            {"type": "object", "additionalProperties": false,
             "required": ["kind","lo_outer","lo","hi","hi_outer"],
             "properties": {"kind": {"type": "string", "enum": ["luminance"]},
                "lo_outer": num(), "lo": num(), "hi": num(), "hi_outer": num()}},
            {"type": "object", "additionalProperties": false,
             "required": ["kind","r","g","b","amount","px","py"],
             "properties": {"kind": {"type": "string", "enum": ["color"]},
                "r": num(), "g": num(), "b": num(), "amount": num(),
                "px": num(), "py": num()}},
            {"type": "null"}
        ]
    })
}

/// HSL: three numeric arrays (red..magenta). Length is pinned at 8 by
/// `recipe::Hsl`'s `[f32;8]` at DESERIALIZE time; OpenAI strict mode cannot
/// pin array length (minItems/maxItems are unsupported and 400 the request),
/// so the proposer prompt enforces "exactly 8, in band order" — and
/// `openai::repair_hsl_axis_lengths` repairs a miscounted axis instead of
/// throwing the whole paid proposal away.
fn hsl_schema() -> Value {
    let hsl_axis = || json!({"type": "array", "items": {"type": "number"}});
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["hue", "saturation", "luminance"],
        "properties": {"hue": hsl_axis(), "saturation": hsl_axis(), "luminance": hsl_axis()}
    })
}

/// Colour grading wheels (flat scalar object), per `recipe::ColorGrade`.
fn color_grade_schema() -> Value {
    let num = || json!({"type": "number"});
    json!({
        "type": "object", "additionalProperties": false,
        "required": ["shadow_hue","shadow_sat","shadow_lum","midtone_hue","midtone_sat","midtone_lum",
            "highlight_hue","highlight_sat","highlight_lum","global_hue","global_sat","global_lum",
            "blending","balance"],
        "properties": {
            "shadow_hue": num(), "shadow_sat": num(), "shadow_lum": num(),
            "midtone_hue": num(), "midtone_sat": num(), "midtone_lum": num(),
            "highlight_hue": num(), "highlight_sat": num(), "highlight_lum": num(),
            "global_hue": num(), "global_sat": num(), "global_lum": num(),
            "blending": num(), "balance": num()
        }
    })
}

fn crop_schema() -> Value {
    let num = || json!({"type": "number"});
    json!({"type": ["object","null"], "additionalProperties": false,
        "required": ["left","top","right","bottom"],
        "properties": {"left": num(), "top": num(), "right": num(), "bottom": num()}})
}

/// An array of `{input,output}` curve points (master + the three RGB
/// channels). The integers are bounded to 0..255 (`recipe::CurvePoint` is u8)
/// so the model can't emit an out-of-range value that fails the whole-recipe
/// deserialize.
fn curve_arr() -> Value {
    let int255 = || json!({"type": "integer", "minimum": 0, "maximum": 255});
    json!({"type": "array", "items": {"type": "object",
        "additionalProperties": false, "required": ["input", "output"],
        "properties": {"input": int255(), "output": int255()}}})
}

/// Build one strict-mode object schema from a registry: every AI-visible row
/// becomes a property AND a `required` entry (OpenAI strict mode requires
/// both), engine-only rows appear nowhere.
fn strict_object(controls: &[Control]) -> Value {
    let mut required = Vec::new();
    let mut properties = serde_json::Map::new();
    for c in controls.iter().filter(|c| !c.engine_only) {
        let prop = c.shape.schema().unwrap_or_else(|| {
            // Unreachable by construction (every EngineCarrier row is
            // engine_only, pinned by `engine_carriers_never_reach_the_schema`)
            // — but a future row that got the pair wrong would otherwise emit
            // a property-less `required` entry, which 400s the request with a
            // schema error instead of naming the mistake.
            panic!("control `{}` is in the schema but has no schema shape", c.name)
        });
        required.push(Value::String(c.name.to_string()));
        properties.insert(c.name.to_string(), prop);
    }
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": Value::Array(required),
        "properties": Value::Object(properties),
    })
}

/// The nested local-adjustment mirror, derived from [`LOCAL_CONTROLS`].
fn local_adjustment_schema() -> Value {
    strict_object(&LOCAL_CONTROLS)
}

// ── control FAMILIES + the thinking envelope (R23-4, feedback #13) ──────────

/// One FAMILY of develop controls — the vocabulary the thinking mode writes
/// its `tool_plan` in, and (since R25 P0) the source of the develop panel's
/// per-section ●.
///
/// NOT one row per field: every global control × `{use, why}` would multiply
/// the response (and its cost) for no gain, while the reported defect is that
/// whole CLASSES of tool go unconsidered ("it never touches hsl or
/// color_grade"), which is a question about families. `members` are
/// [`RECIPE_CONTROLS`] names and the partition is pinned BOTH ways by
/// `every_ai_visible_control_belongs_to_exactly_one_family`, so a control added
/// to the registry cannot quietly fall outside the plan the model is asked to
/// make.
#[derive(Debug, Clone, Copy)]
pub struct Family {
    /// The wire spelling — one value of the `control` enum in the envelope
    /// schema, and the word the prompt uses.
    pub name: &'static str,
    /// What the model is told this family covers.
    pub covers: &'static str,
    /// The registry rows it owns.
    pub members: &'static [&'static str],
}

impl Family {
    /// Does the AI ever see this family? DERIVED — a family is AI-visible iff
    /// at least one member is a control the schema asks for.
    ///
    /// The `effects` family (R25 B2) was the first with none, and B3 added
    /// `detail_effects` and `lens_effects` beside it: their members are
    /// `engine_only`, so putting them in the thinking envelope's `control`
    /// enum would ask the model to plan with tools the recipe schema then
    /// refuses to accept. They are still families, because the
    /// develop panel's ● is derived from this table — and deriving the answer
    /// (rather than adding a hand-set flag) is what stops the two facts from
    /// drifting apart. `every_ai_visible_control_belongs_to_exactly_one_family`
    /// additionally pins that a family is ALL visible or ALL engine-only.
    pub fn ai_visible(&self) -> bool {
        self.members.iter().any(|m| global_control(m).is_some_and(|c| !c.engine_only))
    }
}

/// The globals that are a MEASUREMENT OF THIS PHOTO rather than an edit
/// anybody chose — four of them the engine's own, re-stamped on every open by
/// `pipeline::stamp_calibration`, and since v1.5.0 two of ADOBE'S.
///
/// F6 added `upright_transform`, the matrices Lightroom's Upright solver wrote
/// into the sidecar. It joins on the list's real question (can the photographer
/// act on losing it?) and not on its historical name: nobody chose those
/// numbers, nobody can restate them, and the merge leaves the document's own
/// copy in place anyway — so an interrupting toast about them would fire on
/// every save of every Upright photo and say nothing actionable.
///
/// F7 added `look` on all three of those grounds at once, measured on the
/// photographer's own RAW library rather than on the fixture packs: **161 of
/// its 175 sidecars carry a `crs:Look`, and 152 of those are Lightroom's
/// default `Adobe Color`** — the profile it stamps on a file it has merely
/// touched, whose baked half is one gentle S-curve. The photographer did
/// choose it, in the sense that they left the default alone; what they cannot
/// do is act on losing it, because this app ships no profile picker (F7's
/// stated scope) and `xmp::top_level_owned_spans` preserves the element
/// verbatim anyway, so the merge loses nothing at all. A toast on 92% of saves
/// about a block the output still contains is the alarm-on-every-save this
/// list exists to prevent. The name is still not a measurement, and that is
/// still not the question being asked.
///
/// The list is not new: [`Control::engine_only`]'s doc has named the first four
/// as "the stamped calibration" since R23-1b. What is new (R33 §G) is that a
/// SECOND question used to be answered with `engine_only` because the two
/// coincided — `gui::util::xmp_loss_interrupts` asks whether an export loss is
/// something the photographer can act on, and until now every `engine_only`
/// global that could be lost was one of these four. `colour_field` is the
/// counterexample: the advisor cannot state ninety-six vertices, so it is
/// `engine_only`, but the user asked for it by raising Strength and can switch
/// it off in one click. Answering "can they act on it?" with "can the model
/// say it?" would have filed the first actionable global loss this app has
/// ever had into the quiet channel.
pub const STAMPED_CALIBRATION: [&str; 6] = [
    "as_shot_k",
    "as_shot_tint",
    "base_curve",
    "lens_profile",
    "look",
    "upright_transform",
];

/// The 21 families the global controls partition into — 13 the AI plans with
/// plus eight engine-only groupings (see [`Family::ai_visible`]); `effects`,
/// `black_white` and `calibration` crossed to the planned side on 2026-10-21.
pub const CONTROL_FAMILIES: [Family; 21] = [
    Family {
        name: "tone",
        covers: "exposure, contrast and the four tonal bands (highlights / shadows / whites / blacks)",
        members: &["exposure_ev", "contrast", "highlights", "shadows", "whites", "blacks"],
    },
    Family {
        name: "white_balance",
        covers: "the absolute Kelvin target and the relative green/magenta tint",
        members: &["temperature_k", "tint"],
    },
    Family {
        // R25 B2: `texture` joined this family with the engine stage that
        // renders it. The develop panel's Presence ● widened with it — a
        // decision, which is why `the_family_dots_match_the_hand_written_
        // predicates_they_replaced` had to be edited to say so.
        name: "presence",
        covers: "vibrance, saturation, clarity, dehaze and texture",
        members: &["vibrance", "saturation", "clarity", "dehaze", "texture"],
    },
    Family {
        name: "hsl",
        covers: "the 8-band hue / saturation / luminance colour mixer",
        members: &["hsl"],
    },
    Family {
        name: "color_grade",
        covers: "the shadow / midtone / highlight / global colour-grading wheels",
        members: &["color_grade"],
    },
    Family {
        name: "curves",
        covers: "the master tone curve and the three per-channel RGB curves",
        members: &["tone_curve", "red_curve", "green_curve", "blue_curve"],
    },
    Family {
        // v1.5.0. A family of its own beside `curves` rather than seven new
        // members on it, for `detail_effects`' reason: a family is all
        // AI-visible or all engine-only, and the point curves are tools the
        // model plans with. The develop panel's Curves ● is the OR of the two.
        name: "parametric",
        covers: "Lightroom's parametric tone curve — four region sliders over three movable \
                 splits, rendered here and set by the photographer rather than the AI",
        members: &[
            "param_shadows",
            "param_darks",
            "param_lights",
            "param_highlights",
            "param_shadow_split",
            "param_midtone_split",
            "param_highlight_split",
        ],
    },
    Family {
        name: "detail",
        covers: "capture sharpening and global luminance noise reduction",
        members: &["sharpening", "noise_reduction"],
    },
    Family {
        name: "framing",
        covers: "the crop rectangle and the straighten angle",
        members: &["straighten_deg", "crop"],
    },
    Family {
        name: "lens",
        covers: "the manual optical corrections — vignette falloff and geometric distortion \
                 (physical fixes, not mood; leave them null unless you SEE a defect)",
        members: &["lens_vignette", "lens_vignette_mid", "lens_distortion"],
    },
    Family {
        name: "transform",
        covers: "Lightroom's Transform panel — the two keystones, rotate, scale, aspect and the two \
                 offsets, plus the Upright mode and Constrain Crop; rendered here, and set by the \
                 photographer rather than the AI",
        members: &[
            "perspective_vertical",
            "perspective_horizontal",
            "perspective_rotate",
            "perspective_scale",
            "perspective_aspect",
            "perspective_x",
            "perspective_y",
            "perspective_upright",
            "upright_transform",
            "crop_constrain_to_warp",
        ],
    },
    Family {
        name: "masks",
        covers: "local (masked) adjustments — dodging, burning, holding a sky back",
        members: &["masks"],
    },
    Family {
        // R33 §G. A family of its own rather than a fourth member of `masks`,
        // for the rule `every_ai_visible_control_belongs_to_exactly_one_family`
        // states: a family is ALL AI-visible or ALL engine-only, and `masks`
        // is a tool the model really does plan with. The develop panel shows
        // this row inside its Masks section all the same — the section is a
        // layout, the family is a partition of the registry, and they were
        // never required to agree.
        name: "colour_field",
        covers: "the smooth local colour/tone field the reverse fit solves — engine-only, no Lightroom equivalent",
        members: &["colour_field"],
    },
    Family {
        // The first family with no AI-visible member (R25 B2), which is what
        // [`Family::ai_visible`] was derived for; AI-visible since 2026-10-21,
        // when its nine rows became nullable.
        name: "effects",
        covers: "the post-crop vignette (amount, midpoint, feather, roundness, style, highlights) \
                 and the film grain (amount, size, roughness), rendered after the crop — null \
                 leaves the photographer's effects alone",
        members: &[
            "post_crop_vignette",
            "post_crop_vignette_mid",
            "post_crop_vignette_feather",
            "post_crop_vignette_round",
            "post_crop_vignette_style",
            "post_crop_vignette_hl",
            "grain",
            "grain_size",
            "grain_rough",
        ],
    },
    Family {
        // R25 B3. A SEPARATE family from `detail` rather than eight new
        // members on it, because a family is all-AI-visible or all-engine-only
        // (`every_ai_visible_control_belongs_to_exactly_one_family`) and
        // `detail`'s two sliders are tools the model really does plan with.
        // The develop panel's Detail ● is the OR of the two — the section
        // holds both halves, so its dot has to.
        name: "detail_effects",
        covers: "the sharpening radius / detail / edge-mask triple, colour noise reduction and \
                 the noise-reduction shaping axes — rendered here, set by the photographer \
                 rather than the AI",
        members: &[
            "sharpen_radius",
            "sharpen_detail",
            "sharpen_mask",
            "nr_detail",
            "nr_contrast",
            "color_nr",
            "color_nr_detail",
            "color_nr_smooth",
        ],
    },
    Family {
        // The Lens section's engine-only half, same split and same reason.
        // The family split is about who the AI may PLAN with, not about what
        // renders — grouping is what lights one ●. (Until v1.5.0 the tiers
        // here were MIXED, the pair rendering and the rest carried; the whole
        // family renders now, so the split is about planning alone.)
        name: "lens_effects",
        covers: "the manual chromatic-aberration pair, Lightroom's auto-CA switch, its de-fringe \
                 block and the lens profile's two strengths — all rendered, all set by the \
                 photographer rather than the AI",
        members: &[
            "ca_r",
            "ca_b",
            "lens_profile_distortion_scale",
            "lens_profile_vignetting_scale",
            "auto_lateral_ca",
            "defringe_purple",
            "defringe_purple_lo",
            "defringe_purple_hi",
            "defringe_green",
            "defringe_green_lo",
            "defringe_green_hi",
        ],
    },
    Family {
        // v1.5.0. The colour mixer section's engine-only halves, on
        // `detail_effects`' reason: `hsl` is a tool the model plans with, so
        // the B&W treatment and the point colours are families of their own,
        // and the develop panel's mixer ● is the OR of the three.
        name: "black_white",
        covers: "Lightroom's Black & White treatment and its eight-band grey mix (how light each \
                 colour turns in grey) — null leaves the photographer's treatment alone",
        members: &[
            "convert_to_grayscale",
            "gray_red",
            "gray_orange",
            "gray_yellow",
            "gray_green",
            "gray_aqua",
            "gray_blue",
            "gray_purple",
            "gray_magenta",
        ],
    },
    Family {
        name: "point_color",
        covers: "Lightroom's Point Color swatches — rendered here, picked off the photo by the \
                 photographer",
        members: &["point_colors"],
    },
    Family {
        // v1.5.0 F7. Separate from `calibration`: those seven are the
        // photographer's own primaries tweak, this pair is WHICH rendering
        // the photograph starts from.
        name: "profile",
        covers: "the Adobe camera profile and the creative Look this photograph is developed \
                 through",
        members: &["camera_profile", "look"],
    },
    Family {
        // v1.5.0. A develop-panel section of its own, as in Lightroom.
        name: "calibration",
        covers: "Lightroom's Calibration panel — the shadows tint and the hue / saturation of the \
                 red, green and blue primaries, the deepest colour move there is — null leaves \
                 the photographer's calibration alone",
        members: &[
            "cal_shadow_tint",
            "cal_red_hue",
            "cal_red_sat",
            "cal_green_hue",
            "cal_green_sat",
            "cal_blue_hue",
            "cal_blue_sat",
        ],
    },
    Family {
        // v1.5.0 F8. Its own section for the same reason Lightroom gives it
        // one: these do not adjust the photograph, they say which RANGE it was
        // adjusted in and how that range reaches an SDR file.
        name: "hdr",
        covers: "Lightroom's HDR edit mode and the SDR rendition it is published through — \
                 rendered here, set by the photographer rather than the AI",
        members: &[
            "hdr_edit",
            "hdr_max_ev",
            "sdr_blend",
            "sdr_brightness",
            "sdr_contrast",
            "sdr_highlights",
            "sdr_shadows",
            "sdr_whites",
            "sdr_clarity",
        ],
    },
];

/// The AI-visible rows that are BOOKKEEPING rather than develop tools, so they
/// belong to no family. Spelled out (not inferred) so a future non-tool field
/// is a deliberate edit here rather than a silent hole in the partition.
pub const NOT_A_TOOL: [&str; 3] = ["version", "rationale", "confidence"];

/// The controls [`family_is_active`] does NOT count — a non-neutral value that
/// changes no pixel anywhere, so lighting the develop panel's ● for it would
/// promise an edit the preview, the export and Lightroom all render
/// identically. Each entry states its reason, on the [`CARRIED_ONLY_GLOBAL`]
/// pattern: an exemption without a stated reason is indistinguishable from an
/// oversight.
pub const DOT_EXEMPT: &[(&str, &str)] = &[
    (
        "lens_vignette_mid",
        "the engine builds the manual falloff LUT only when the AMOUNT is non-zero \
         — `(r.lens_vignette != 0.0).then(|| manual_vignette_lut(…))`, render.rs — \
         and the XMP carries VignetteMidpoint under the same zero amount, so a \
         moved Midpoint alone changes no pixel in the preview, the export or \
         Lightroom. Whenever it DOES matter, `lens_vignette != 0.0` has already lit \
         the dot (R22 #16, re-checked R25)",
    ),
    // v1.5.0. A split only says where two regions of the parametric curve
    // meet.
    (
        "param_shadow_split",
        "while every parametric REGION is 0 the curve is the identity wherever its splits \
         sit — `render::parametric_lut` answers None — so a moved split alone changes no \
         pixel in the preview or the export, the straight line Lightroom's own panel draws \
         until a region moves. Whenever a split DOES matter, a region slider has already lit \
         the dot",
    ),
    ("param_midtone_split", "same reason as param_shadow_split: a split alone moves no pixel"),
    ("param_highlight_split", "same reason as param_shadow_split: a split alone moves no pixel"),
    // v1.5.0. A grey-mix band only says how light one colour turns in grey.
    (
        "gray_red",
        "the grey mix renders only under the Black & White treatment — \
         `apply_develop_with_rasters` runs `apply_gray_mix` while `convert_to_grayscale` is on \
         and the colour path otherwise, as Lightroom ignores the mix on a colour photo — so a \
         band moved on a colour photo changes no pixel. Whenever a band DOES matter, the switch \
         has already lit the dot",
    ),
    ("gray_orange", "same reason as gray_red: the grey mix renders only in black and white"),
    ("gray_yellow", "same reason as gray_red: the grey mix renders only in black and white"),
    ("gray_green", "same reason as gray_red: the grey mix renders only in black and white"),
    ("gray_aqua", "same reason as gray_red: the grey mix renders only in black and white"),
    ("gray_blue", "same reason as gray_red: the grey mix renders only in black and white"),
    ("gray_purple", "same reason as gray_red: the grey mix renders only in black and white"),
    ("gray_magenta", "same reason as gray_red: the grey mix renders only in black and white"),
    // v1.5.0. Every AMOUNT-GATED companion, on `lens_vignette_mid`'s exact
    // reason and found by the same question: does this control alone move a
    // pixel? Each operator below is built only when its own amount is non-zero
    // (`Vignette::of` / `Grain::of` / `Window::of` all answer `None` at zero),
    // so a companion moved on its own renders identically in the preview, the
    // export and Lightroom. Whenever it DOES matter, the amount beside it has
    // already lit the dot.
    (
        "post_crop_vignette_mid",
        "`finish::Vignette::of` returns `None` while PostCropVignetteAmount is 0, so the \
         midpoint alone builds no falloff — and Lightroom writes the companion keys only \
         alongside a non-zero amount, which is why their absent state is Adobe's default \
         rather than 0",
    ),
    ("post_crop_vignette_feather", "same reason as post_crop_vignette_mid: the amount gates it"),
    ("post_crop_vignette_round", "same reason as post_crop_vignette_mid: the amount gates it"),
    ("post_crop_vignette_style", "same reason as post_crop_vignette_mid: the amount gates it"),
    (
        "post_crop_vignette_hl",
        "gated TWICE over: Highlights acts only when the amount is non-zero AND negative \
         (`finish::Vignette::apply`, Adobe's own rule), so on its own it is doubly inert",
    ),
    (
        "grain_size",
        "`finish::Grain::of` returns `None` while GrainAmount is 0, so a lattice size alone \
         has no grain to size",
    ),
    ("grain_rough", "same reason as grain_size: the amount gates it"),
    (
        "defringe_purple_lo",
        "`lens::Window::of` returns `None` while DefringePurpleAmount is 0, so moving the hue \
         window alone corrects nothing — and this block's neutral is Adobe's 30/70, not zero, \
         so a photographer who never touched it still carries non-zero numbers here",
    ),
    ("defringe_purple_hi", "same reason as defringe_purple_lo: the amount gates the window"),
    ("defringe_green_lo", "same reason as defringe_purple_lo: the amount gates the window"),
    ("defringe_green_hi", "same reason as defringe_purple_lo: the amount gates the window"),
];

/// Is any control in this family standing away from its neutral state?
///
/// The develop panel's ● per section — "a collapsed active adjustment is never
/// invisible" — DERIVED from the family table instead of a hand-written field
/// tuple per section. Four of those tuples had already drifted one field short
/// of their section by R22 #16, and every LR-gap batch that adds a control adds
/// another chance to forget one; a family that gains a member now gains the dot
/// with it.
///
/// Neutral means "as [`EditRecipe::default`] has it", read through
/// [`global_value`] so a renamed or retyped field cannot slip past — with the
/// two structured look objects answering for THEMSELVES (see
/// [`value_is_active`]) and [`DOT_EXEMPT`] left out.
///
/// A companion named in [`EditRecipe::explicit_zero`] is active whatever its
/// stored number says: that 0 renders differently from the default recipe's
/// absent one, which inequality on the number alone cannot see.
pub fn family_is_active(f: &Family, r: &EditRecipe) -> bool {
    let neutral = EditRecipe::default();
    f.members.iter().filter(|m| !DOT_EXEMPT.iter().any(|(n, _)| n == *m)).any(|m| {
        r.explicit_zero.iter().any(|e| e == m)
            || match (global_value(r, m), global_value(&neutral, m)) {
                (Some(live), Some(base)) => value_is_active(live, base),
                // Reached by exactly one member, `upright_transform`, which
                // `global_value` leaves unmapped on purpose (see its doc):
                // Adobe's matrices render nothing until `perspective_upright`
                // selects one, and that mode lights this ● itself. Any other
                // `None` would be a typo'd member, which `every_ai_visible_
                // control_belongs_to_exactly_one_family` rules out — either
                // way a value this function cannot read must not claim activity.
                _ => false,
            }
    })
}

/// "Away from neutral", per shape: plain inequality against the default recipe,
/// except for the two structured look objects, which are asked their OWN
/// question.
///
/// `Hsl::is_neutral` / `ColorGrade::is_neutral` are the gates the renderer and
/// the XMP writer skip on, and the grade's is deliberately NOT inequality: a
/// wheel turned to a hue at zero saturation, or Blending moved on its own,
/// changes no pixel (recipe.rs states it). Comparing the whole object would
/// therefore light a ● over a section that renders identically — the same
/// mistake [`DOT_EXEMPT`] exists to prevent for the lens midpoint.
fn value_is_active(live: GlobalValue<'_>, neutral: GlobalValue<'_>) -> bool {
    match live {
        GlobalValue::Hsl(h) => !h.is_neutral(),
        GlobalValue::Grade(cg) => !cg.is_neutral(),
        // A swatch whose three shifts are all 0 is a real state that moves no
        // pixel — the point colours' own `is_neutral`, on the grade's reason.
        GlobalValue::PointColors(swatches) => swatches.iter().any(|p| !p.is_neutral()),
        other => other != neutral,
    }
}

/// The THINKING ENVELOPE (R23-4, six fields since R23-1b): the model states
/// what it sees, which tool families it will use and why, and the look it is
/// aiming for BEFORE it writes the numbers — then critiques its own answer
/// after, and may point at the PIXEL tools no develop control can reach.
///
/// `recipe` is the SAME [`edit_recipe_schema`] the default path sends, nested
/// verbatim: the thinking fields are not develop controls and must never enter
/// [`RECIPE_CONTROLS`] (they would reach `recipe.json`, the XMP comment,
/// `deny_unknown_fields` downgrades and R21's `recipe_norm` structure
/// fingerprint). One envelope, one paid call, zero new roles.
///
/// ORDER, measured on three paid calls: strict structured output does NOT
/// generate fields in the declared order. The probe
/// (`openai::tests::think_envelope_field_order_probe`) came back with the
/// answer's keys in strict ALPHABETICAL order every time — R27 (2026-08-19,
/// through the relay) at `recipe` 170, `scene` 1269, `self_critique` 1393,
/// `tool_plan` 1595, and twice on 2026-09-02 (`gpt-5` on api.openai.com) at
/// `recipe` 194/230, `scene` 1604/2527, `self_critique` 1702/2667,
/// `tool_plan` 1961/2859 — so the model writes the recipe FIRST and the
/// "thinking" after it, whatever `required` declares. The offsets move with
/// the length of the model's prose; the ORDER does not. Two endpoints, two
/// models, two dates, one order:
/// the mechanism the first run could only hypothesise (keys serialised
/// sorted, the same way this tree's own `serde_json` does) is what the
/// second run confirms. Consequence: "think before you write" rests on the
/// PROMPT alone; the envelope's structural benefits (an explicit plan, an
/// auditable self-critique, a per-family use/skip decision) survive
/// unchanged, because they never depended on generation order.
///
/// The rename the mechanism makes possible — spelling the thinking fields so
/// they sort ahead of `recipe` — is NOT taken, and this is the decision, not
/// a pause before one. It would buy generation order at the price of naming
/// every envelope field after a provider's serialiser: the names reach the
/// prompt, the parser, the `deny_unknown_fields` envelope struct and every
/// fixture, and they would have to be renamed back the day a provider honours
/// the declaration. The probe's assertion now encodes the alphabetical
/// measurement, so that day announces itself.
pub fn think_envelope_schema() -> Value {
    let families: Vec<Value> = CONTROL_FAMILIES
        .iter()
        .filter(|f| f.ai_visible())
        .map(|f| Value::String(f.name.to_string()))
        .collect();
    let tools: Vec<Value> = crate::advisor::PixelTool::ALL
        .iter()
        .map(|(n, _)| Value::String((*n).to_string()))
        .collect();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": [
            "scene", "tool_plan", "intended_look", "recipe", "self_critique",
            "pixel_tool_suggestions"
        ],
        "properties": {
            "scene": {"type": "string"},
            "tool_plan": {"type": "array", "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["control", "use", "why"],
                "properties": {
                    "control": {"type": "string", "enum": Value::Array(families)},
                    "use": {"type": "boolean"},
                    "why": {"type": "string"}
                }
            }},
            "intended_look": {"type": "string"},
            "recipe": edit_recipe_schema(),
            "self_critique": {"type": "string"},
            // The PIXEL tools, which no recipe field can reach (R23-1b). Last
            // on purpose: it is an aside about work the develop cannot do, and
            // the four thinking fields around the recipe are the ones whose
            // order carries the "think first" claim. Strict mode has no
            // optional property — every property must be `required` — so the
            // "nothing to suggest" answer is an EMPTY ARRAY, which the prompt
            // states and which is the common case.
            "pixel_tool_suggestions": {"type": "array", "items": {
                "type": "object",
                "additionalProperties": false,
                "required": ["tool", "why"],
                "properties": {
                    "tool": {"type": "string", "enum": Value::Array(tools)},
                    "why": {"type": "string"}
                }
            }}
        }
    })
}

/// The prompt half of the envelope: what each field means, in the order the
/// response must take. Generated from [`CONTROL_FAMILIES`] so the plan's
/// candidate list can never advertise a family the schema's enum rejects.
///
/// The ORDER SENTENCE below is the one line that must be re-read whenever the
/// envelope grows a field — R23-1b added `pixel_tool_suggestions` to the schema
/// and this sentence still listed five, which asks the model for a shape
/// nothing described. `the_think_envelope_nests_the_recipe_schema_unchanged`
/// now parses this very sentence against the schema's `required` array.
pub fn think_prompt() -> String {
    let list: String = CONTROL_FAMILIES
        .iter()
        .filter(|f| f.ai_visible())
        .map(|f| format!("  • {} — {}\n", f.name, f.covers))
        .collect();
    format!(
        "THINK FIRST, THEN ANSWER. Your reply is an ENVELOPE whose keys must be emitted in \
THIS ORDER: `scene`, `tool_plan`, `intended_look`, `recipe`, `self_critique`, \
`pixel_tool_suggestions`. Write the first \
three BEFORE the numbers — they are your working, not a summary of it.\n\
  1. `scene` — ONE sentence on what this photograph is and what its light is doing.\n\
  2. `tool_plan` — ONE entry for EACH tool family below, in this order, with `use` true or \
false and ONE short clause of `why`. \"this photo does not need it\" is a valid reason; \
\"I did not consider it\" is not. Deciding a family is unnecessary is a real decision — make it \
explicitly rather than by omission.\n\
{list}\
  3. `intended_look` — ONE sentence naming the finished look you are going for.\n\
  4. `recipe` — the EditRecipe itself, which must EXECUTE the plan above: a family you marked \
`use: true` has to show up in the numbers, and one you marked `use: false` stays at its neutral \
value.\n\
  5. `self_critique` — ONE sentence judging your own answer against the TARGET STRENGTH \
above: is this a finished photograph at that strength, or did you play it safe? Name the \
weakest part.\n\
  6. `pixel_tool_suggestions` — AT MOST 3 entries, and `[]` (empty) is the normal answer. \
These are the app's PIXEL tools, which NO develop control can reach: {tools}. Name one only \
when this photograph visibly needs it (a sensor spot or a distracting object → Heal / \
CloneStamp / GenerativeFill; heavy noise a slider cannot fix → Denoise; a selection that \
needs the photographer's own painting beyond what a kind=ai mask in `masks` selects → \
SelectSubject / SelectSky; a re-imagined frame → \
Reimagine), with ONE clause of `why`. You are ADVISING the photographer, not running these \
tools: nothing here is executed, several cost money, and the develop you return must stand on \
its own without them.\n",
        tools = crate::advisor::PixelTool::ALL
            .iter()
            .map(|(n, _)| *n)
            .collect::<Vec<_>>()
            .join(" / "),
    )
}

/// JSON Schema for [`EditRecipe`] in OpenAI strict mode: every property listed
/// in `required`, `additionalProperties:false`, optionals expressed as
/// nullable — DERIVED from [`RECIPE_CONTROLS`] (R23-1), so a new recipe field
/// can no longer be invisible to the AI.
pub fn edit_recipe_schema() -> Value {
    strict_object(&RECIPE_CONTROLS)
}

/// The proposer prompt's control list: name, band, neutral value and the one
/// line of purpose, for every control the model may set. Generated, so the
/// prompt can never advertise a control the schema rejects (or omit one it
/// accepts) — the prompt's own blind spot before R23-1 (it never mentioned
/// white balance, dehaze, sharpening, crop or straighten at all).
pub fn prompt_catalogue() -> String {
    let render = |controls: &[Control]| -> String {
        controls
            .iter()
            .filter(|c| !c.engine_only)
            .map(|c| format!("  • {} ({}) — {}\n", c.name, c.spec(), c.purpose))
            .collect::<String>()
    };
    format!(
        "CONTROL CATALOGUE — the COMPLETE set of controls you may return, each with the range it \
accepts (ranges are SAFETY bounds, not targets), its neutral value, and what it is for. Leave a \
control at its neutral value when the photograph does not call for it; anything not listed here \
you cannot set.\nGLOBAL:\n{}PER MASK (one object per entry of `masks`; the slider scales match \
the globals):\n{}",
        render(&RECIPE_CONTROLS),
        render(&LOCAL_CONTROLS),
    )
}

// ── typed value access (the compiler-enforced coverage) ─────────────────────

/// One control's value read off a recipe — the shape-preserving view the eval
/// ruler and diagnostics read through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GlobalValue<'a> {
    Num(f32),
    OptNum(Option<f32>),
    Bool(bool),
    Int(u32),
    Text(&'a str),
    Curve(&'a [CurvePoint]),
    Hsl(&'a Hsl),
    Grade(&'a ColorGrade),
    Crop(Option<&'a Crop>),
    Masks(&'a [LocalAdjustment]),
    /// The imported spot-removal areas (v1.5.0 F9). Equality is the whole
    /// list, which is the right "is it active" answer for a set of geometries
    /// with no neutral value of their own — an area exists or it does not.
    Retouch(&'a [crate::retouch::RetouchArea]),
    /// The generative-fill layers (2026-10-01); equality is the whole list.
    Layers(&'a [crate::render::PixelLayer]),
    /// The AI lens blur (2026-10-13), or `None` for a recipe without one.
    LensBlur(Option<&'a crate::render::LensBlur>),
    Knots(&'a [[f32; 2]]),
    Lens(&'a LensProfile),
    /// The solved colour field, or `None` for a recipe that carries none.
    ColourField(Option<&'a crate::recipe::ColourField>),
    /// The R25 B4 pass-through block: `crs:` key → the verbatim string
    /// Lightroom wrote. Equality is the whole map, which is the right
    /// "is it active" answer for a value nothing interprets.
    PassThrough(&'a std::collections::BTreeMap<String, String>),
    /// A list of control names — `explicit_zero`.
    Names(&'a [String]),
    /// The point colours (v1.5.0), in the recipe's order.
    PointColors(&'a [crate::recipe::PointColor]),
    /// The creative profile's baked half (v1.5.0 F7), or `None` for a
    /// photograph developed through no creative profile at all.
    Look(Option<&'a crate::recipe::CreativeLook>),
}

impl GlobalValue<'_> {
    /// The value as a single number, for the scalar consumers (`None` for the
    /// structured shapes and for an unset nullable).
    pub fn scalar(&self) -> Option<f32> {
        match self {
            GlobalValue::Num(v) => Some(*v),
            GlobalValue::OptNum(v) => *v,
            _ => None,
        }
    }
}

/// The value of the named [`RECIPE_CONTROLS`] row in `r`, or `None` for a name
/// that is not a control.
///
/// **This is one of the two compiler tripwires this module exists for.** The
/// `let EditRecipe { .. }` below has NO rest pattern, so a new recipe field
/// fails to compile here; and because every binding but one is consumed by a
/// match arm, a field that IS destructured but never mapped fails as an
/// `unused_variables` deny. Either way the build stops until the registry
/// gains the row.
///
/// The one exception is `upright_transform`, bound to `_` on purpose: Adobe's
/// solved Upright matrices have no [`GlobalValue`] shape, so this answers
/// `None` for that registry row. [`family_is_active`] reads that as "not
/// active", which is the right answer — the matrices render nothing until
/// `perspective_upright` (a mapped `Num`) selects one, and that mode lights
/// the `transform` ● itself.
pub fn global_value<'a>(r: &'a EditRecipe, name: &str) -> Option<GlobalValue<'a>> {
    let EditRecipe {
        camera_profile,
        look,
        version,
        coord_era,
        schema_era,
        exposure_ev,
        contrast,
        highlights,
        shadows,
        whites,
        blacks,
        temperature_k,
        tint,
        as_shot_k,
        as_shot_tint,
        vibrance,
        saturation,
        clarity,
        dehaze,
        texture,
        hsl,
        convert_to_grayscale,
        gray_red,
        gray_orange,
        gray_yellow,
        gray_green,
        gray_aqua,
        gray_blue,
        gray_purple,
        gray_magenta,
        point_colors,
        color_grade,
        sharpening,
        noise_reduction,
        sharpen_radius,
        sharpen_detail,
        sharpen_mask,
        nr_detail,
        nr_contrast,
        color_nr,
        color_nr_detail,
        color_nr_smooth,
        lens_vignette,
        lens_vignette_mid,
        lens_distortion,
        lens_profile_distortion_scale,
        lens_profile_vignetting_scale,
        perspective_vertical,
        perspective_horizontal,
        perspective_rotate,
        perspective_scale,
        perspective_aspect,
        perspective_x,
        perspective_y,
        perspective_upright,
        upright_transform: _,
        crop_constrain_to_warp,
        ca_r,
        ca_b,
        auto_lateral_ca,
        defringe_purple,
        defringe_purple_lo,
        defringe_purple_hi,
        defringe_green,
        defringe_green_lo,
        defringe_green_hi,
        post_crop_vignette,
        post_crop_vignette_mid,
        post_crop_vignette_feather,
        post_crop_vignette_round,
        post_crop_vignette_style,
        post_crop_vignette_hl,
        grain,
        grain_size,
        grain_rough,
        explicit_zero,
        straighten_deg,
        quarter_turns,
        crop,
        tone_curve,
        red_curve,
        green_curve,
        blue_curve,
        param_shadows,
        param_darks,
        param_lights,
        param_highlights,
        param_shadow_split,
        param_midtone_split,
        param_highlight_split,
        base_curve,
        lens_profile,
        masks,
        retouch,
        pixel_layers,
        lens_blur,
        colour_field,
        passthrough,
        cal_shadow_tint,
        cal_red_hue,
        cal_red_sat,
        cal_green_hue,
        cal_green_sat,
        cal_blue_hue,
        cal_blue_sat,
        hdr_edit,
        hdr_max_ev,
        sdr_blend,
        sdr_brightness,
        sdr_contrast,
        sdr_highlights,
        sdr_shadows,
        sdr_whites,
        sdr_clarity,
        rationale,
        confidence,
    } = r;
    Some(match name {
        "version" => GlobalValue::Int(*version),
        "coord_era" => GlobalValue::Int(*coord_era),
        "schema_era" => GlobalValue::Int(*schema_era),
        "exposure_ev" => GlobalValue::Num(*exposure_ev),
        "contrast" => GlobalValue::Num(*contrast),
        "highlights" => GlobalValue::Num(*highlights),
        "shadows" => GlobalValue::Num(*shadows),
        "whites" => GlobalValue::Num(*whites),
        "blacks" => GlobalValue::Num(*blacks),
        "temperature_k" => GlobalValue::OptNum(*temperature_k),
        "tint" => GlobalValue::Num(*tint),
        "as_shot_k" => GlobalValue::OptNum(*as_shot_k),
        "as_shot_tint" => GlobalValue::OptNum(*as_shot_tint),
        "vibrance" => GlobalValue::Num(*vibrance),
        "saturation" => GlobalValue::Num(*saturation),
        "clarity" => GlobalValue::Num(*clarity),
        "dehaze" => GlobalValue::Num(*dehaze),
        "texture" => GlobalValue::Num(*texture),
        "hsl" => GlobalValue::Hsl(hsl),
        "convert_to_grayscale" => GlobalValue::Bool(*convert_to_grayscale),
        "gray_red" => GlobalValue::Num(*gray_red),
        "gray_orange" => GlobalValue::Num(*gray_orange),
        "gray_yellow" => GlobalValue::Num(*gray_yellow),
        "gray_green" => GlobalValue::Num(*gray_green),
        "gray_aqua" => GlobalValue::Num(*gray_aqua),
        "gray_blue" => GlobalValue::Num(*gray_blue),
        "gray_purple" => GlobalValue::Num(*gray_purple),
        "gray_magenta" => GlobalValue::Num(*gray_magenta),
        "point_colors" => GlobalValue::PointColors(point_colors),
        "color_grade" => GlobalValue::Grade(color_grade),
        "sharpening" => GlobalValue::Num(*sharpening),
        "noise_reduction" => GlobalValue::Num(*noise_reduction),
        "sharpen_radius" => GlobalValue::Num(*sharpen_radius),
        "sharpen_detail" => GlobalValue::Num(*sharpen_detail),
        "sharpen_mask" => GlobalValue::Num(*sharpen_mask),
        "nr_detail" => GlobalValue::Num(*nr_detail),
        "nr_contrast" => GlobalValue::Num(*nr_contrast),
        "color_nr" => GlobalValue::Num(*color_nr),
        "color_nr_detail" => GlobalValue::Num(*color_nr_detail),
        "color_nr_smooth" => GlobalValue::Num(*color_nr_smooth),
        "lens_vignette" => GlobalValue::Num(*lens_vignette),
        "lens_vignette_mid" => GlobalValue::Num(*lens_vignette_mid),
        "lens_distortion" => GlobalValue::Num(*lens_distortion),
        "ca_r" => GlobalValue::Num(*ca_r),
        "ca_b" => GlobalValue::Num(*ca_b),
        "lens_profile_distortion_scale" => GlobalValue::Num(*lens_profile_distortion_scale),
        "lens_profile_vignetting_scale" => GlobalValue::Num(*lens_profile_vignetting_scale),
        "auto_lateral_ca" => GlobalValue::Bool(*auto_lateral_ca),
        "perspective_vertical" => GlobalValue::Num(*perspective_vertical),
        "perspective_horizontal" => GlobalValue::Num(*perspective_horizontal),
        "perspective_rotate" => GlobalValue::Num(*perspective_rotate),
        "perspective_scale" => GlobalValue::Num(*perspective_scale),
        "perspective_aspect" => GlobalValue::Num(*perspective_aspect),
        "perspective_x" => GlobalValue::Num(*perspective_x),
        "perspective_y" => GlobalValue::Num(*perspective_y),
        "perspective_upright" => GlobalValue::Num(*perspective_upright),
        "crop_constrain_to_warp" => GlobalValue::Bool(*crop_constrain_to_warp),
        "defringe_purple" => GlobalValue::Num(*defringe_purple),
        "defringe_purple_lo" => GlobalValue::Num(*defringe_purple_lo),
        "defringe_purple_hi" => GlobalValue::Num(*defringe_purple_hi),
        "defringe_green" => GlobalValue::Num(*defringe_green),
        "defringe_green_lo" => GlobalValue::Num(*defringe_green_lo),
        "defringe_green_hi" => GlobalValue::Num(*defringe_green_hi),
        "post_crop_vignette" => GlobalValue::Num(*post_crop_vignette),
        "post_crop_vignette_mid" => GlobalValue::Num(*post_crop_vignette_mid),
        "post_crop_vignette_feather" => GlobalValue::Num(*post_crop_vignette_feather),
        "post_crop_vignette_round" => GlobalValue::Num(*post_crop_vignette_round),
        "post_crop_vignette_style" => GlobalValue::Num(*post_crop_vignette_style),
        "post_crop_vignette_hl" => GlobalValue::Num(*post_crop_vignette_hl),
        "grain" => GlobalValue::Num(*grain),
        "grain_size" => GlobalValue::Num(*grain_size),
        "grain_rough" => GlobalValue::Num(*grain_rough),
        "explicit_zero" => GlobalValue::Names(explicit_zero),
        "straighten_deg" => GlobalValue::Num(*straighten_deg),
        "quarter_turns" => GlobalValue::Int(u32::from(*quarter_turns)),
        "crop" => GlobalValue::Crop(crop.as_ref()),
        "tone_curve" => GlobalValue::Curve(tone_curve),
        "red_curve" => GlobalValue::Curve(red_curve),
        "green_curve" => GlobalValue::Curve(green_curve),
        "blue_curve" => GlobalValue::Curve(blue_curve),
        "param_shadows" => GlobalValue::Num(*param_shadows),
        "param_darks" => GlobalValue::Num(*param_darks),
        "param_lights" => GlobalValue::Num(*param_lights),
        "param_highlights" => GlobalValue::Num(*param_highlights),
        "param_shadow_split" => GlobalValue::Num(*param_shadow_split),
        "param_midtone_split" => GlobalValue::Num(*param_midtone_split),
        "param_highlight_split" => GlobalValue::Num(*param_highlight_split),
        "base_curve" => GlobalValue::Knots(base_curve),
        "camera_profile" => GlobalValue::Text(camera_profile),
        "look" => GlobalValue::Look(look.as_ref()),
        "lens_profile" => GlobalValue::Lens(lens_profile),
        "masks" => GlobalValue::Masks(masks),
        "retouch" => GlobalValue::Retouch(retouch),
        "pixel_layers" => GlobalValue::Layers(pixel_layers),
        "lens_blur" => GlobalValue::LensBlur(lens_blur.as_ref()),
        "colour_field" => GlobalValue::ColourField(colour_field.as_ref()),
        "passthrough" => GlobalValue::PassThrough(passthrough),
        "cal_shadow_tint" => GlobalValue::Num(*cal_shadow_tint),
        "cal_red_hue" => GlobalValue::Num(*cal_red_hue),
        "cal_red_sat" => GlobalValue::Num(*cal_red_sat),
        "cal_green_hue" => GlobalValue::Num(*cal_green_hue),
        "cal_green_sat" => GlobalValue::Num(*cal_green_sat),
        "cal_blue_hue" => GlobalValue::Num(*cal_blue_hue),
        "cal_blue_sat" => GlobalValue::Num(*cal_blue_sat),
        "hdr_edit" => GlobalValue::Bool(*hdr_edit),
        "hdr_max_ev" => GlobalValue::Num(*hdr_max_ev),
        "sdr_blend" => GlobalValue::Num(*sdr_blend),
        "sdr_brightness" => GlobalValue::Num(*sdr_brightness),
        "sdr_contrast" => GlobalValue::Num(*sdr_contrast),
        "sdr_highlights" => GlobalValue::Num(*sdr_highlights),
        "sdr_shadows" => GlobalValue::Num(*sdr_shadows),
        "sdr_whites" => GlobalValue::Num(*sdr_whites),
        "sdr_clarity" => GlobalValue::Num(*sdr_clarity),
        "rationale" => GlobalValue::Text(rationale),
        "confidence" => GlobalValue::Num(*confidence),
        _ => return None,
    })
}

/// The local-adjustment counterpart of [`GlobalValue`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LocalValue<'a> {
    Num(f32),
    OptGains(Option<[f32; 3]>),
    Bool(bool),
    Text(&'a str),
    Curve(&'a [CurvePoint]),
    Geometry(&'a crate::recipe::MaskGeometry),
    Components(&'a [crate::recipe::MaskComponent]),
    Range(Option<&'a crate::recipe::RangeMask>),
    Role(crate::recipe::MaskRole),
}

impl LocalValue<'_> {
    pub fn scalar(&self) -> Option<f32> {
        match self {
            LocalValue::Num(v) => Some(*v),
            _ => None,
        }
    }
}

/// The value of the named [`LOCAL_CONTROLS`] row in `m` — the second compiler
/// tripwire (see [`global_value`]); `LocalAdjustment` is destructured with no
/// rest pattern and every binding consumed.
pub fn local_value<'a>(m: &'a LocalAdjustment, name: &str) -> Option<LocalValue<'a>> {
    let LocalAdjustment {
        mask,
        components,
        enabled,
        range,
        name: label,
        amount,
        inverted,
        exposure_ev,
        contrast,
        highlights,
        shadows,
        whites,
        blacks,
        clarity,
        dehaze,
        texture,
        sharpness,
        saturation,
        hue,
        temperature,
        tint,
        toning_hue,
        toning_amount,
        noise_reduction,
        main_curve,
        red_curve,
        green_curve,
        blue_curve,
        color_gains,
        role,
    } = m;
    Some(match name {
        "mask" => LocalValue::Geometry(mask),
        "components" => LocalValue::Components(components),
        "enabled" => LocalValue::Bool(*enabled),
        "range" => LocalValue::Range(range.as_ref()),
        "name" => LocalValue::Text(label),
        "amount" => LocalValue::Num(*amount),
        "inverted" => LocalValue::Bool(*inverted),
        "exposure_ev" => LocalValue::Num(*exposure_ev),
        "contrast" => LocalValue::Num(*contrast),
        "highlights" => LocalValue::Num(*highlights),
        "shadows" => LocalValue::Num(*shadows),
        "whites" => LocalValue::Num(*whites),
        "blacks" => LocalValue::Num(*blacks),
        "clarity" => LocalValue::Num(*clarity),
        "dehaze" => LocalValue::Num(*dehaze),
        "texture" => LocalValue::Num(*texture),
        "sharpness" => LocalValue::Num(*sharpness),
        "saturation" => LocalValue::Num(*saturation),
        "hue" => LocalValue::Num(*hue),
        "temperature" => LocalValue::Num(*temperature),
        "tint" => LocalValue::Num(*tint),
        "toning_hue" => LocalValue::Num(*toning_hue),
        "toning_amount" => LocalValue::Num(*toning_amount),
        "noise_reduction" => LocalValue::Num(*noise_reduction),
        "main_curve" => LocalValue::Curve(main_curve),
        "red_curve" => LocalValue::Curve(red_curve),
        "green_curve" => LocalValue::Curve(green_curve),
        "blue_curve" => LocalValue::Curve(blue_curve),
        "color_gains" => LocalValue::OptGains(*color_gains),
        "role" => LocalValue::Role(*role),
        _ => return None,
    })
}

// ── family expansions (the per-band / per-wheel rulers) ─────────────────────

/// The three HSL axes: the recipe's array name, and the `crs` attribute stem
/// each band spelling is built on (`SaturationAdjustmentBlue`, …).
pub const HSL_AXES: [(&str, &str); 3] = [
    ("hue", "HueAdjustment"),
    ("saturation", "SaturationAdjustment"),
    ("luminance", "LuminanceAdjustment"),
];

/// One band of one HSL axis, as a measurable field.
#[derive(Debug, Clone)]
pub struct HslField {
    /// Report/metric name, e.g. `hsl.saturation.blue`.
    pub metric: String,
    /// The ACR attribute, e.g. `SaturationAdjustmentBlue`.
    pub crs: String,
    /// Index into [`HSL_AXES`].
    pub axis: usize,
    /// Index into `recipe::HSL_BANDS`.
    pub band: usize,
}

/// The 24 attributes the 8-band mixer occupies — the expansion the registry's
/// `CrsKey::Family` stands for. Pinned against `xmp::owned_attr_keys` (the
/// writer's own universe) by this module's tests, so the ruler can only ever
/// read keys we also write.
pub fn hsl_expansion() -> Vec<HslField> {
    let mut out = Vec::with_capacity(24);
    for (axis, (axis_name, stem)) in HSL_AXES.iter().enumerate() {
        for (band, band_name) in HSL_BANDS.iter().enumerate() {
            out.push(HslField {
                metric: format!("hsl.{axis_name}.{}", band_name.to_lowercase()),
                crs: format!("{stem}{band_name}"),
                axis,
                band,
            });
        }
    }
    out
}

/// Index of the `hue` axis in [`HSL_AXES`], named so the one policy that
/// treats it differently from saturation and luminance — `style::style_targets`
/// ingests the eight hue bands and distils none of them — can say WHICH axis it
/// means without a bare `0`. Pinned to the table by
/// `hsl_axis_hue_is_the_first_axis`.
pub const HSL_AXIS_HUE: usize = 0;

/// One HSL cell by (axis, band) index — the exhaustive-destructure tripwire
/// for `recipe::Hsl` (a fourth axis stops the build here).
pub fn hsl_value(h: &Hsl, axis: usize, band: usize) -> Option<f32> {
    let Hsl { hue, saturation, luminance } = h;
    let arr = match axis {
        0 => hue,
        1 => saturation,
        2 => luminance,
        _ => return None,
    };
    arr.get(band).copied()
}

/// The same cell to WRITE — the mutable mirror of [`hsl_value`], destructured
/// the same way so a fourth axis stops the build in both.
///
/// It exists because the style distillation pulls the mixer toward the
/// photographer's own per-band habit (`style::blend_toward`), and a loop that
/// reaches a band by `(axis, band)` through this registry is a loop that cannot
/// carry its own private copy of the axis order.
pub fn hsl_value_mut(h: &mut Hsl, axis: usize, band: usize) -> Option<&mut f32> {
    let Hsl { hue, saturation, luminance } = h;
    let arr = match axis {
        0 => hue,
        1 => saturation,
        2 => luminance,
        _ => return None,
    };
    arr.get_mut(band)
}

/// The colour-grade SATURATION field that belongs to the same wheel as a
/// `*_hue` field — derived from [`COLOR_GRADE_CRS`] rather than written down a
/// second time.
///
/// A wheel's hue is an ANGLE and its saturation is how much of that angle is
/// applied, so the two are one decision: `style::style_targets` will not learn
/// a tint angle whose intensity it did not also learn, and it weights the
/// angles by exactly this field (an unsaturated wheel's hue is not a choice,
/// it is an untouched control).
pub fn wheel_saturation_of(hue_field: &str) -> Option<&'static str> {
    let stem = hue_field.strip_suffix("_hue")?;
    COLOR_GRADE_CRS
        .iter()
        .map(|(f, _)| *f)
        .find(|f| f.strip_suffix("_sat") == Some(stem))
}

/// The 14 colour-grade fields paired with their ACR attributes. ACR's own
/// convention (verified against the user's sidecars): shadow/highlight hue+sat
/// round-trip through the legacy `SplitToning*` keys, everything else through
/// `ColorGrade*`.
pub const COLOR_GRADE_CRS: [(&str, &str); 14] = [
    ("shadow_hue", "SplitToningShadowHue"),
    ("shadow_sat", "SplitToningShadowSaturation"),
    ("shadow_lum", "ColorGradeShadowLum"),
    ("midtone_hue", "ColorGradeMidtoneHue"),
    ("midtone_sat", "ColorGradeMidtoneSat"),
    ("midtone_lum", "ColorGradeMidtoneLum"),
    ("highlight_hue", "SplitToningHighlightHue"),
    ("highlight_sat", "SplitToningHighlightSaturation"),
    ("highlight_lum", "ColorGradeHighlightLum"),
    ("global_hue", "ColorGradeGlobalHue"),
    ("global_sat", "ColorGradeGlobalSat"),
    ("global_lum", "ColorGradeGlobalLum"),
    ("blending", "ColorGradeBlending"),
    ("balance", "SplitToningBalance"),
];

/// One SCALAR FIELD inside the nested `color_grade` object: the spelling the
/// clients name it by, the band `ColorGrade::clamp` enforces and the neutral
/// `ColorGrade::default` stores (blending's is 50, not 0).
///
/// These are deliberately NOT [`RECIPE_CONTROLS`] rows. The advisor sees the
/// grading object WHOLE (`Shape::ColorGrade`, one property in the strict
/// schema), so a nested row there would rewrite the AI contract; what the
/// clients need is the FORM view of the same object, and before this table
/// every client spelled that band again (the GUI's grading panel drew its two
/// whole-grade sliders with literal 0..100 / -100..100 and reset 50 / 0). The
/// registry stays the band source for top-level scalars; this table is its
/// nested counterpart, and `form_control` answers for both.
pub struct GradeField {
    /// The name the clients use: `color_grade.<field>`.
    pub name: &'static str,
    /// The band [`ColorGrade::clamp`] enforces on the field.
    pub range: (f32, f32),
    /// The neutral — `ColorGrade::default`'s value for the field.
    pub neutral: f32,
}

impl GradeField {
    /// The JSON field name, i.e. the part after the `color_grade.` qualifier.
    pub fn field(&self) -> &'static str {
        self.name.strip_prefix("color_grade.").unwrap_or(self.name)
    }
}

/// Every field of the nested `color_grade` object, in schema order. The band
/// categories are `ColorGrade::clamp`'s own (hue wraps 0..360, sat 0..100,
/// lum/balance -100..100, blending 0..100); the field list is pinned to the
/// schema and to [`COLOR_GRADE_CRS`] by `registry_serde_and_schema_agree_on_both_mirrors`.
pub const COLOR_GRADE_FIELDS: [GradeField; 14] = [
    GradeField { name: "color_grade.shadow_hue", range: (0.0, 360.0), neutral: 0.0 },
    GradeField { name: "color_grade.shadow_sat", range: (0.0, 100.0), neutral: 0.0 },
    GradeField { name: "color_grade.shadow_lum", range: (-100.0, 100.0), neutral: 0.0 },
    GradeField { name: "color_grade.midtone_hue", range: (0.0, 360.0), neutral: 0.0 },
    GradeField { name: "color_grade.midtone_sat", range: (0.0, 100.0), neutral: 0.0 },
    GradeField { name: "color_grade.midtone_lum", range: (-100.0, 100.0), neutral: 0.0 },
    GradeField { name: "color_grade.highlight_hue", range: (0.0, 360.0), neutral: 0.0 },
    GradeField { name: "color_grade.highlight_sat", range: (0.0, 100.0), neutral: 0.0 },
    GradeField { name: "color_grade.highlight_lum", range: (-100.0, 100.0), neutral: 0.0 },
    GradeField { name: "color_grade.global_hue", range: (0.0, 360.0), neutral: 0.0 },
    GradeField { name: "color_grade.global_sat", range: (0.0, 100.0), neutral: 0.0 },
    GradeField { name: "color_grade.global_lum", range: (-100.0, 100.0), neutral: 0.0 },
    GradeField { name: "color_grade.blending", range: (0.0, 100.0), neutral: 50.0 },
    GradeField { name: "color_grade.balance", range: (-100.0, 100.0), neutral: 0.0 },
];

/// One colour-grade wheel field by name — the exhaustive-destructure tripwire
/// for `recipe::ColorGrade` (a fifth wheel stops the build here).
pub fn color_grade_value(cg: &ColorGrade, field: &str) -> Option<f32> {
    let ColorGrade {
        shadow_hue,
        shadow_sat,
        shadow_lum,
        midtone_hue,
        midtone_sat,
        midtone_lum,
        highlight_hue,
        highlight_sat,
        highlight_lum,
        global_hue,
        global_sat,
        global_lum,
        blending,
        balance,
    } = cg;
    Some(match field {
        "shadow_hue" => *shadow_hue,
        "shadow_sat" => *shadow_sat,
        "shadow_lum" => *shadow_lum,
        "midtone_hue" => *midtone_hue,
        "midtone_sat" => *midtone_sat,
        "midtone_lum" => *midtone_lum,
        "highlight_hue" => *highlight_hue,
        "highlight_sat" => *highlight_sat,
        "highlight_lum" => *highlight_lum,
        "global_hue" => *global_hue,
        "global_sat" => *global_sat,
        "global_lum" => *global_lum,
        "blending" => *blending,
        "balance" => *balance,
        _ => return None,
    })
}

/// The same wheel field to WRITE — the mutable mirror of
/// [`color_grade_value`], destructured the same way so a fifth wheel stops the
/// build in both. Added for the same reason [`hsl_value_mut`] was: the style
/// distillation reaches a wheel by NAME through this registry rather than
/// keeping its own copy of the field list.
pub fn color_grade_value_mut<'a>(cg: &'a mut ColorGrade, field: &str) -> Option<&'a mut f32> {
    let ColorGrade {
        shadow_hue,
        shadow_sat,
        shadow_lum,
        midtone_hue,
        midtone_sat,
        midtone_lum,
        highlight_hue,
        highlight_sat,
        highlight_lum,
        global_hue,
        global_sat,
        global_lum,
        blending,
        balance,
    } = cg;
    Some(match field {
        "shadow_hue" => shadow_hue,
        "shadow_sat" => shadow_sat,
        "shadow_lum" => shadow_lum,
        "midtone_hue" => midtone_hue,
        "midtone_sat" => midtone_sat,
        "midtone_lum" => midtone_lum,
        "highlight_hue" => highlight_hue,
        "highlight_sat" => highlight_sat,
        "highlight_lum" => highlight_lum,
        "global_hue" => global_hue,
        "global_sat" => global_sat,
        "global_lum" => global_lum,
        "blending" => blending,
        "balance" => balance,
        _ => return None,
    })
}

/// The registry row for a global control name.
pub fn global_control(name: &str) -> Option<&'static Control> {
    RECIPE_CONTROLS.iter().find(|c| c.name == name)
}

/// The reset value a top-level scalar row's form shows: the COMPANION's
/// Lightroom default (`EditRecipe::resolved`, e.g. grain size 25 — the stored
/// 0 means "absent"), else the default recipe's own scalar.
fn form_reset(name: &str) -> Option<f32> {
    let default_recipe = EditRecipe::default();
    if crate::recipe::LR_COMPANION_DEFAULTS.iter().any(|(n, _)| *n == name) {
        Some(default_recipe.resolved(name))
    } else {
        match global_value(&default_recipe, name) {
            Some(GlobalValue::Num(v)) => Some(v),
            Some(GlobalValue::OptNum(Some(v))) => Some(v),
            Some(GlobalValue::Int(v)) => Some(v as f32),
            _ => None,
        }
    }
}

/// One FORM control's `(clamp band, reset)` by the name the clients use: a
/// top-level [`RECIPE_CONTROLS`] spelling, or the nested `color_grade.<field>`
/// form of [`COLOR_GRADE_FIELDS`]. This is the lookup the GUI's capability rows
/// use, so a row never restates a band or a reset; [`form_controls`] builds its
/// list from the two registries directly.
pub fn form_control(name: &str) -> Option<((f32, f32), Option<f32>)> {
    if let Some(field) = name.strip_prefix("color_grade.") {
        let row = COLOR_GRADE_FIELDS.iter().find(|f| f.field() == field)?;
        return Some((row.range, Some(row.neutral)));
    }
    let c = global_control(name)?;
    Some((c.range?, form_reset(name)))
}

/// The scalar controls the develop forms render, as `(name, clamp band, reset
/// value)`. The top-level rows are the catalogue's own (the band `EditRecipe::clamp`
/// is pinned to; the reset a companion's Lightroom default or the default
/// recipe's scalar); the nested colour-grade fields follow in schema order with
/// [`COLOR_GRADE_FIELDS`]' band and neutral. The GUI's capability table, the web
/// form's sliders and the CLI's `controls` command all read this one answer.
pub fn form_controls() -> Vec<(&'static str, (f32, f32), Option<f32>)> {
    let mut rows: Vec<(&'static str, (f32, f32), Option<f32>)> = RECIPE_CONTROLS
        .iter()
        .filter_map(|c| Some((c.name, c.range?, form_reset(c.name))))
        .collect();
    rows.extend(COLOR_GRADE_FIELDS.iter().map(|f| (f.name, f.range, Some(f.neutral))));
    rows
}
