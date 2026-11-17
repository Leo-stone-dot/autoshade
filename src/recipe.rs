//! The `EditRecipe` is the contract between the *AI advisor* (which decides
//! *what* to do) and the *render engine* (which decides *how* to do it,
//! deterministically). The AI never touches pixels — it only emits one of
//! these. Every field maps to a well-understood, Lightroom/ACR-style develop
//! control so the same recipe can be (a) rendered by our own pipeline or
//! (b) serialised to an XMP sidecar that Lightroom / Camera Raw reads directly.
//!
//! Ranges follow Adobe conventions so the numbers are intuitive and portable:
//!   * sliders such as contrast/highlights/shadows: -100..=100, 0 = no change
//!   * `exposure_ev`: stops of exposure, typically -5.0..=5.0, 0.0 = no change
//!   * `temperature_k`: absolute white-balance target in Kelvin (None = as-shot)

#![deny(clippy::print_stderr)]

use serde::{Deserialize, Serialize};

/// A complete, self-describing set of develop adjustments for one image.
///
/// `#[serde(default)]` means the AI may omit any field it doesn't want to
/// touch; the omitted control simply stays neutral. This keeps prompts small
/// and makes partial recipes valid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditRecipe {
    /// Schema version so we can evolve the contract without silently
    /// misreading old recipes — and, since [`CALIB_ERA`], the CALIBRATION
    /// era that produced `base_curve`: a curve stamped below the current era
    /// was estimated by an earlier estimator and is re-estimated on the next
    /// open; see `pipeline::repair_pre_era_base_curve`.
    pub version: u32,

    /// Which COORDINATE FRAME `crop` and every `masks` geometry are expressed
    /// in — see [`COORD_ERA`]. Absent in any recipe written before v0.30.0,
    /// which is exactly what `0` (the SENSOR frame) means.
    ///
    /// This is deliberately NOT folded into [`version`](Self::version).
    /// `version` is the CURVE's provenance and is transplanted between
    /// recipes on purpose — `paste_recipe_for` (gui/persist.rs), the Analyze
    /// writer (`pipeline::produce_recipe`), the quit-time generated-variant
    /// re-stamp (gui/actions.rs) and `photo_calibration` all copy the TARGET
    /// photo's `version` onto a recipe whose geometry came from somewhere
    /// else. A coordinate frame is intrinsic to the geometry and must never
    /// travel with a curve: stamping a saved era-2 `version` onto a recipe
    /// whose masks are already in the display frame would make the next load
    /// rotate them a second time. Two independent facts, two fields.
    #[serde(default = "coord_era_legacy")]
    pub coord_era: u32,

    /// Which SET OF CONTROLS this recipe was written against — see
    /// [`SCHEMA_ERA`]. Absent in any recipe written before v0.31.0, which is
    /// exactly what `0` means.
    ///
    /// The fact nothing else in the file could state: R25 gave the writer
    /// twenty-seven new `crs:` keys (global Texture, the carried effects, the
    /// detail axes, the manual CA pair, the de-fringe block), and every recipe
    /// written before them deserialises with those fields at their serde
    /// defaults. A default nobody read is NOT a photographer's decision, and
    /// the XMP merge cannot tell the two apart from the value alone: stripping
    /// `crs:Texture="-20"` out of someone's sidecar because a v0.30
    /// `recipe.json` "says" 0 is deleting an edit, not honouring one
    /// (`xmp::unspoken_attr_keys`).
    ///
    /// Deliberately NOT folded into [`version`](Self::version) or
    /// [`coord_era`](Self::coord_era), for the reason spelled out above: those
    /// two are the CURVE's provenance and the GEOMETRY's frame, and `version`
    /// in particular is transplanted between recipes on purpose. Which keys a
    /// recipe has ever seen is a third, independent fact — the four-fields-in-
    /// one-integer move is exactly what made the pre-`coord_era` migration
    /// unrecoverable.
    #[serde(default = "schema_era_legacy")]
    pub schema_era: u32,

    // --- Tone ---------------------------------------------------------------
    /// Global exposure in stops (EV). 0.0 = unchanged.
    pub exposure_ev: f32,
    /// -100..=100
    pub contrast: f32,
    /// -100..=100 (recover blown highlights when negative)
    pub highlights: f32,
    /// -100..=100 (lift shadows when positive)
    pub shadows: f32,
    /// -100..=100 (white point)
    pub whites: f32,
    /// -100..=100 (black point)
    pub blacks: f32,

    // --- White balance ------------------------------------------------------
    /// Absolute colour temperature target in Kelvin. `None` = keep as-shot.
    pub temperature_k: Option<f32>,
    /// Green/magenta tint, -100..=100. 0 = neutral.
    pub tint: f32,
    /// ENGINE-ONLY as-shot anchor (never in XMP): the camera's own white
    /// balance as an ABSOLUTE Kelvin CCT, derived from the RAW's WB
    /// coefficients through the camera colour matrix (`render::as_shot_wb`).
    /// Stamped like `base_curve`/`lens_profile` — fresh opens get it, a saved
    /// recipe.json owns its value verbatim. `None` = unknown → the engine
    /// keeps its historical 5500 K anchor, so legacy archives render
    /// byte-identically.
    pub as_shot_k: Option<f32>,
    /// The as-shot green/magenta companion, in the recipe's own tint scale
    /// (display + XMP honesty only — `tint` stays a RELATIVE shift from
    /// as-shot, so the engine never feeds this to `wb_gains`).
    pub as_shot_tint: Option<f32>,

    // --- Colour / presence --------------------------------------------------
    /// -100..=100, protects already-saturated colours.
    pub vibrance: f32,
    /// -100..=100, uniform saturation.
    pub saturation: f32,
    /// -100..=100, local contrast / midtone punch.
    pub clarity: f32,
    /// -100..=100, atmospheric haze removal.
    pub dehaze: f32,
    /// -100..=100, fine detail (skin, foliage, fabric) without clarity's
    /// midtone volume — Lightroom's Basic-panel Texture. The same unsharp
    /// operator as `clarity` at a SMALL radius and with no midtone mask; the
    /// per-mask `LocalAdjustment::texture` has rendered it since R22, and this
    /// is the global counterpart it never had (R25 B2). → `crs:Texture`.
    pub texture: f32,

    // --- Per-colour HSL (the 8 ACR colour bands) ----------------------------
    /// Lightroom's HSL / Color mixer. Default = all bands neutral (v1-compatible).
    pub hsl: Hsl,

    // --- Black & white and point colour (v1.5.0) ----------------------------
    // Lightroom's B&W treatment with its eight-band mixer, and the Color
    // Mixer's Point Color swatches — rendered by `render::apply_gray_mix` and
    // `render::apply_point_colors`, the first IN PLACE OF the colour mixer (as
    // in Lightroom, where the B&W panel replaces the HSL one). Each field is
    // skipped at its neutral, by the parametric curve's argument: a recipe that
    // never used them stays byte-identical to what v1.4 wrote.
    /// The B&W treatment: the frame develops to grey through the mixer below.
    /// → `crs:ConvertToGrayscale` (the top-level key; the flag a monochrome
    /// creative profile carries inside its own `crs:Look` is the Look's).
    #[serde(skip_serializing_if = "is_false")]
    pub convert_to_grayscale: bool,
    /// B&W mixer, -100..=100 per band, in [`HSL_BANDS`] order: how light that
    /// colour turns in grey. Renders only under `convert_to_grayscale`.
    /// → `crs:GrayMixerRed` … `crs:GrayMixerMagenta`.
    #[serde(skip_serializing_if = "is_zero")]
    pub gray_red: f32,
    #[serde(skip_serializing_if = "is_zero")]
    pub gray_orange: f32,
    #[serde(skip_serializing_if = "is_zero")]
    pub gray_yellow: f32,
    #[serde(skip_serializing_if = "is_zero")]
    pub gray_green: f32,
    #[serde(skip_serializing_if = "is_zero")]
    pub gray_aqua: f32,
    #[serde(skip_serializing_if = "is_zero")]
    pub gray_blue: f32,
    #[serde(skip_serializing_if = "is_zero")]
    pub gray_purple: f32,
    #[serde(skip_serializing_if = "is_zero")]
    pub gray_magenta: f32,
    /// Lightroom's Point Color swatches, at most [`MAX_POINT_COLORS`].
    /// → `crs:PointColors` (an `rdf:Seq` of 19-number records).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub point_colors: Vec<PointColor>,

    // --- Colour grading (3-wheel + global) ----------------------------------
    /// Lightroom's Color Grading wheels. Default = neutral (v1-compatible).
    pub color_grade: ColorGrade,

    // --- Detail -------------------------------------------------------------
    /// 0..=150, capture sharpening amount. A COMPANION since v1.6.0: a stored
    /// 0 is "absent" and renders at Lightroom's own default for the KIND of
    /// source — 40 on a RAW negative, none on a baked raster — read through
    /// [`EditRecipe::capture_sharpening`], never this field alone.
    pub sharpening: f32,
    /// 0..=100, luminance noise reduction.
    pub noise_reduction: f32,

    // --- Detail: the shaping axes (R25 B3; RENDERED since v1.5.0) -----------
    // Round-tripped through their own `crs:` keys since R25, when policy SF4-C
    // carried them without drawing them; `render/detail.rs` renders all eight
    // since the user revoked that policy. They shape the two sliders above —
    // Lightroom's sharpening has a radius / detail / edge-masking triple
    // around its amount, its luminance NR a detail / contrast pair — and add
    // the COLOUR NR operator with its own detail / smoothness pair.
    //
    // STORED 0 = "the sidecar said nothing", as since R25: the writer emits a
    // key only when it is non-zero, so an absent key stays absent and
    // Lightroom keeps its own default (Radius 1.0, Detail 25, Luminance Detail
    // 50, Colour NR Detail/Smoothness 50) instead of one we invented, and the
    // engine renders the same default through `EditRecipe::resolved`. A REAL
    // 0 on a companion is named in `explicit_zero`.
    /// Sharpening radius, 0..=3.0 (0 = absent; ACR default 1.0 and its band is
    /// 0.5..3.0). The one DECIMAL key in this block — Lightroom writes
    /// `crs:SharpenRadius="+1.0"`, verified in all seven of the user's
    /// sidecars. → `crs:SharpenRadius`.
    pub sharpen_radius: f32,
    /// Sharpening detail / halo suppression, 0..=100 (ACR default 25).
    /// → `crs:SharpenDetail`.
    pub sharpen_detail: f32,
    /// Sharpening edge mask, 0..=100. → `crs:SharpenEdgeMasking`.
    pub sharpen_mask: f32,
    /// Luminance NR detail, 0..=100 (ACR default 50).
    /// → `crs:LuminanceNoiseReductionDetail`.
    pub nr_detail: f32,
    /// Luminance NR contrast, 0..=100. → `crs:LuminanceNoiseReductionContrast`.
    pub nr_contrast: f32,
    /// CHROMA noise reduction amount, 0..=100 (ACR's RAW default 25; a
    /// recipe's 0 is "off", and the writer states it, `xmp::amount_carries`).
    /// → `crs:ColorNoiseReduction`.
    pub color_nr: f32,
    /// Chroma NR detail, 0..=100 (ACR default 50).
    /// → `crs:ColorNoiseReductionDetail`.
    pub color_nr_detail: f32,
    /// Chroma NR smoothness, 0..=100 (ACR default 50).
    /// → `crs:ColorNoiseReductionSmoothness`.
    pub color_nr_smooth: f32,

    // --- Lens corrections (manual) -------------------------------------------
    /// Lens vignette compensation, -100..=100: positive brightens corners
    /// (compensates falloff), negative darkens. Applied as a radial gain in
    /// LINEAR light before any tonal work. → `crs:VignetteAmount`.
    pub lens_vignette: f32,
    /// Where the correction lands, 0..=100 (ACR Midpoint, default 50): lower
    /// reaches toward the centre, higher confines it to the corners.
    /// → `crs:VignetteMidpoint`.
    pub lens_vignette_mid: f32,
    /// Manual geometric distortion correction, -100..=100 (ACR convention):
    /// positive straightens BARREL distortion (wide-angle bulge), negative
    /// straightens PINCUSHION (tele pinch). A radial resample of the frame
    /// applied after develop, before straighten — see `render::distort_norm`
    /// for the model. → `crs:LensManualDistortionAmount`.
    pub lens_distortion: f32,

    // --- The lens PROFILE's own two strengths (v1.5.0) ----------------------
    // Lightroom's Lens Corrections panel has one Amount slider per component
    // of the PROFILE correction, 0..200 with 100 = "as the profile says". They
    // are the only controls in this struct whose neutral is 100, so absence is
    // unambiguous: a value of 0 is a real "switch this component off", which is
    // why they need no `explicit_zero` companion (X2) the way Midpoint and
    // Feather do.
    //
    // They scale the correction's DEPARTURE from identity, so 100 leaves the
    // render bit-identical to the pre-v1.5.0 engine and 0 is exactly "no
    // profile correction" — the same shape for either knot source (the
    // camera's own metadata, or an Adobe `.lcp` on this machine).
    /// The profile distortion correction's strength, 0..=200 (Lightroom's
    /// Distortion Amount; 100 = the profile's own map). → `crs:LensProfileDistortionScale`.
    pub lens_profile_distortion_scale: f32,
    /// The profile vignetting correction's strength, 0..=200 (Lightroom's
    /// Vignetting Amount; 100 = the profile's own gains).
    /// → `crs:LensProfileVignettingScale`.
    pub lens_profile_vignetting_scale: f32,
    // --- Transform / Upright (v1.5.0 F6) ------------------------------------
    // Lightroom's Transform panel, rendered as ONE projective map between the
    // lens-geometry resample and the straighten (`render::perspective`).
    //
    // The coordinate system is Adobe's own, MEASURED rather than assumed: over
    // the 125 `crs:UprightTransform_*` matrices in this operator's library, the
    // frame centre in [0,1] coordinates is a fixed point to 8.6e-4 on the 64
    // near-identity ones (many to 5e-10), against 2.6e-2 for the sidecar's own
    // `UprightCenterNorm` and 3.6e-1 for a [-1,1] centre. So the matrices are
    // written in [0,1] FRAME coordinates — normalised per axis, which is why a
    // rotation's cover factor matches the UNIT SQUARE's `cos+sin` (1.016934 for
    // 0.9786°, against the matrix's own 1.017110) and not a 3:2 frame's
    // (1.011237). The seven manual sliders are built in the same space so the
    // two compose by plain multiplication.
    /// Transform, Vertical keystone, -100..=100. → `crs:PerspectiveVertical`.
    pub perspective_vertical: f32,
    /// Transform, Horizontal keystone, -100..=100. → `crs:PerspectiveHorizontal`.
    pub perspective_horizontal: f32,
    /// Transform, Rotate, -10..=10 DEGREES (Lightroom's own band, and the one
    /// real value in this library is `+0.9`). Distinct from `straighten_deg`,
    /// which is the crop's angle and runs after this stage.
    /// → `crs:PerspectiveRotate`.
    pub perspective_rotate: f32,
    /// Transform, Scale, 50..=150 with 100 neutral — Lightroom's own band
    /// (its slider stops at both ends; the 162 sidecars in this library that
    /// carry it all read 100) and the third control in this struct whose
    /// neutral is 100 (with the two lens-profile strengths). Bounded below
    /// 50 rather than at 0 since 2026-09-24: a scale of 0 collapses the
    /// Transform map to a point, and the stage then handed the frame back
    /// UNTOUCHED — every other slider and the Upright matrix silently
    /// dropped with it. → `crs:PerspectiveScale`.
    pub perspective_scale: f32,
    /// Transform, Aspect, -100..=100: stretches the frame vertically (+) or
    /// horizontally (−), area-preserving. → `crs:PerspectiveAspect`.
    pub perspective_aspect: f32,
    /// Transform, X Offset, -100..=100. → `crs:PerspectiveX`.
    pub perspective_x: f32,
    /// Transform, Y Offset, -100..=100. → `crs:PerspectiveY`.
    pub perspective_y: f32,
    /// Lightroom's Upright MODE: 0 off, 1 auto, 2 level, 3 vertical, 4 full,
    /// 5 guided. An INSTRUCTION, like the auto-CA switch — and Adobe publishes
    /// its RESULT even though its solver is unpublished, in
    /// `upright_transform`. When the sidecar carries the matrix this mode
    /// selects, the render is Adobe's own to the last digit; when it does not
    /// (this app's own dropdown on a photo Lightroom never solved), the mode is
    /// answered by `render::perspective::solve_upright`.
    /// → `crs:PerspectiveUpright`.
    pub perspective_upright: f32,
    /// Adobe's solved Upright matrices, index = the mode that selects them,
    /// row-major 3×3 in [0,1] frame coordinates. READ from
    /// `crs:UprightTransform_0…N` and rendered; never WRITTEN, on
    /// `lens_profile`'s ground — it is Adobe's measurement of this photograph,
    /// and the merge preserves the document's own bytes for us.
    pub upright_transform: Vec<[f32; 9]>,
    /// Lightroom's "Constrain Crop": shrink the crop until it lies inside the
    /// warped frame. Measured `"0"` on all 52 sidecars in this library that
    /// carry it, so the empty corners a Transform leaves are normally VISIBLE
    /// and the photographer crops by hand — which is exactly what those
    /// sidecars show. → `crs:CropConstrainToWarp`.
    pub crop_constrain_to_warp: bool,
    /// Manual lateral CA, red/cyan axis, -100..=100 (R25 B3). RENDERED: the
    /// engine already resamples red and blue at their own radius for the
    /// in-camera profile's CA knots (`render::apply_lens_geometry`), and this
    /// pair is a CONSTANT factor folded onto that same per-channel LUT — a
    /// lateral CA scales the channel linearly with radius, so a constant
    /// factor is its exact shape, not an approximation of one. Positive
    /// magnifies the red channel relative to green. The slider→scale
    /// calibration is OURS (`render::MANUAL_CA_PER_UNIT`); Adobe's is
    /// unpublished. → `crs:ChromaticAberrationR`.
    pub ca_r: f32,
    /// Manual lateral CA, blue/yellow axis, -100..=100.
    /// → `crs:ChromaticAberrationB`.
    pub ca_b: f32,
    /// Lightroom's "Remove chromatic aberration" switch — an INSTRUCTION to
    /// Adobe's own lateral-CA solver, not a parameter, so it is carried and
    /// never interpreted (SF4-C). Verified in the user's library:
    /// `crs:AutoLateralCA="0"` on six sidecars and `="1"` on one.
    /// → `crs:AutoLateralCA`.
    pub auto_lateral_ca: bool,

    // --- De-fringe: CARRIED, and the one block with NON-ZERO neutrals -------
    // Lightroom writes all six keys on every sidecar (verified: 7 of 7 in the
    // user's library carry `DefringePurpleAmount="0"` with the hue windows at
    // 30/70 and 40/60), so "absent" is not a state this block has in the wild
    // and its honest neutral is ADOBE'S DEFAULT, not zero. Two consequences,
    // both deliberate:
    //
    //   * the READER falls back to these defaults, not to 0 — `crs_f32`
    //     answers `None` for an absent key and taking that as 0 would import
    //     a hue window of 0..0 from a document that never mentioned one, so
    //     an untouched photo would stop being a no-op;
    //   * the WRITER emits all six unconditionally, the shape Lightroom
    //     itself writes.
    //
    // `is_noop` is `*self == EditRecipe::default()`, so it stays correct by
    // construction — and it is pinned both ways, on a sidecar carrying the six
    // defaults and on one carrying no de-fringe keys at all
    // (`xmp::tests::a_real_defringe_block_imports_as_a_noop`).
    /// Purple de-fringe amount, 0..=20. → `crs:DefringePurpleAmount`.
    pub defringe_purple: f32,
    /// Purple hue window, low end, 0..=100 (ACR default 30).
    /// → `crs:DefringePurpleHueLo`.
    pub defringe_purple_lo: f32,
    /// Purple hue window, high end, 0..=100 (ACR default 70).
    /// → `crs:DefringePurpleHueHi`.
    pub defringe_purple_hi: f32,
    /// Green de-fringe amount, 0..=20. → `crs:DefringeGreenAmount`.
    pub defringe_green: f32,
    /// Green hue window, low end, 0..=100 (ACR default 40).
    /// → `crs:DefringeGreenHueLo`.
    pub defringe_green_lo: f32,
    /// Green hue window, high end, 0..=100 (ACR default 60).
    /// → `crs:DefringeGreenHueHi`.
    pub defringe_green_hi: f32,

    // --- Effects: CARRIED to Lightroom, rendered by nothing here (R25 B2) ----
    // Nine Adobe-only operators under the SF4-C policy: we round-trip them
    // through their own `crs:` keys so a Lightroom edit survives an AutoShade
    // save, and we deliberately do NOT approximate them. Each carries its
    // reason in `advisor::catalogue::CARRIED_ONLY_GLOBAL`, and
    // `Tier::CarriedOnly` is what makes the omission a declared fact rather
    // than a slider that quietly does nothing.
    //
    // Every one is NEUTRAL AT ZERO on purpose, including the three whose ACR
    // default is not zero (Midpoint/Feather 50, Style 1): zero here means
    // "the sidecar said nothing", and the writer emits a key only when its
    // value is non-zero — so an absent key stays absent and Lightroom keeps
    // its own default instead of one we invented. Verified against the user's
    // own library: Lightroom writes `PostCropVignetteAmount="0"` /
    // `GrainAmount="0"` on nearly every sidecar and the COMPANION keys only
    // when the amount is non-zero, so an untouched photo still imports as a
    // no-op (`is_noop`) while a real vignette imports all six.
    /// Post-crop vignette amount, -100..=100. → `crs:PostCropVignetteAmount`.
    pub post_crop_vignette: f32,
    /// Post-crop vignette midpoint, 0..=100 (ACR default 50).
    /// → `crs:PostCropVignetteMidpoint`.
    pub post_crop_vignette_mid: f32,
    /// Post-crop vignette feather, 0..=100 (ACR default 50).
    /// → `crs:PostCropVignetteFeather`.
    pub post_crop_vignette_feather: f32,
    /// Post-crop vignette roundness, -100..=100.
    /// → `crs:PostCropVignetteRoundness`.
    pub post_crop_vignette_round: f32,
    /// Which post-crop vignette OPERATOR Adobe applies: 1 = Highlight
    /// Priority, 2 = Colour Priority, 3 = Paint Overlay (0 = the sidecar named
    /// none). → `crs:PostCropVignetteStyle`.
    pub post_crop_vignette_style: f32,
    /// Post-crop vignette highlight contrast, 0..=100.
    /// → `crs:PostCropVignetteHighlightContrast`.
    pub post_crop_vignette_hl: f32,
    /// Film grain amount, 0..=100. → `crs:GrainAmount`.
    pub grain: f32,
    /// Film grain size, 0..=100 (ACR default 25). → `crs:GrainSize`.
    pub grain_size: f32,
    /// Film grain roughness/frequency, 0..=100 (ACR default 50).
    /// → `crs:GrainFrequency`.
    pub grain_rough: f32,

    /// The COMPANION controls (see [`LR_COMPANION_DEFAULTS`]) whose stored 0 is
    /// a real value rather than "the sidecar said nothing", by serde name,
    /// sorted and de-duplicated by [`EditRecipe::clamp`].
    ///
    /// Since R25 a stored 0 on these fields (ten then; the Sharpening amount
    /// joined in v1.6.0) has meant ABSENT, and the
    /// writer has let Lightroom's own default (Detail 25, Feather 50, …) reach
    /// the sidecar by leaving the key out. That encoding could not say
    /// "Detail 0" — a value Lightroom's slider has — and while the controls
    /// moved no pixel here nobody could see the loss. v1.5.0 renders them, so
    /// the difference between 0 and 25 is now a picture; this list is the one
    /// bit per control that tells them apart, and [`EditRecipe::resolved`]
    /// reads every companion through it.
    ///
    /// **Serialisation: skipped when empty**, by [`colour_field`]'s argument:
    /// no recipe written before v1.5.0 has an explicit zero, so every one of
    /// them stays BYTE-IDENTICAL and `store::recipe_struct_hash` keeps matching
    /// every archived version. Reading an old file needs no migration either —
    /// its zeros already meant "absent", which is what an empty list says.
    ///
    /// [`colour_field`]: EditRecipe::colour_field
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub explicit_zero: Vec<String>,

    // --- Transform + profile: PASSED THROUGH, never interpreted (R25 B4) -----
    /// Lightroom's Transform (Upright / Perspective) block and its camera
    /// profile name, carried between the sidecar and `recipe.json` as the
    /// VERBATIM strings Lightroom wrote — key → value, no parsing, no clamping,
    /// no meaning. The first member of `Tier::PassThrough`, and the one
    /// contract that separates it from `CarriedOnly`: a carried control is a
    /// NUMBER we read, bound and could render one day; a passed-through
    /// property is a string we have no opinion about at all. Out-of-range,
    /// oddly spelled or eight-decimal values ride out exactly as they rode in.
    ///
    /// The DOMAIN is a named key set (`xmp::PASSTHROUGH_CRS`, nine keys),
    /// not "everything unknown": `owned_attr_keys` — the merge's strip
    /// universe — is a static list, so a free-for-all map would write keys the
    /// merge does not remove and Lightroom would read our value beside its own
    /// duplicate. Everything outside those nine stays where it always was:
    /// preserved by the merge, and NAMED by `xmp::unmodelled_global_crs`.
    /// (R25 listed seven `CameraCalibration*` keys here as well. Lightroom
    /// writes no such key — its Calibration panel is the unprefixed
    /// `crs:ShadowTint`, `crs:RedHue` … — so the map never held one, and
    /// v1.5.0 renders that panel from the `cal_*` fields below.)
    ///
    /// A `BTreeMap`, so `recipe.json` and the R21 structure fingerprint are
    /// deterministic. The container's `#[serde(default)]` covers absence (an
    /// empty map is `BTreeMap::default()`) — no field-level default is needed
    /// here, unlike `coord_era`, whose legacy default DIFFERS from the type's.
    pub passthrough: std::collections::BTreeMap<String, String>,

    // --- Camera calibration (v1.5.0) -----------------------------------------
    // Lightroom's Calibration panel, rendered as the first develop stage
    // (`render::Calibration`): each primary's hue and saturation move the
    // primary itself, and the shadows tint pulls the dark tones toward green
    // or magenta. Lightroom writes all seven keys on nearly every sidecar (162
    // of the operator's 175, census 2026-09-17) and really uses them —
    // `BlueSaturation="+83"`, `BlueHue="-40"`. Skipped at 0, the parametric
    // curve's argument again.
    /// Shadows tint, -100..=100: + magenta, − green. → `crs:ShadowTint`.
    #[serde(skip_serializing_if = "is_zero")]
    pub cal_shadow_tint: f32,
    /// Red primary hue, -100..=100 (+ toward yellow). → `crs:RedHue`.
    #[serde(skip_serializing_if = "is_zero")]
    pub cal_red_hue: f32,
    /// Red primary saturation, -100..=100. → `crs:RedSaturation`.
    #[serde(skip_serializing_if = "is_zero")]
    pub cal_red_sat: f32,
    /// Green primary hue, -100..=100 (+ toward cyan). → `crs:GreenHue`.
    #[serde(skip_serializing_if = "is_zero")]
    pub cal_green_hue: f32,
    /// Green primary saturation, -100..=100. → `crs:GreenSaturation`.
    #[serde(skip_serializing_if = "is_zero")]
    pub cal_green_sat: f32,
    /// Blue primary hue, -100..=100 (+ toward magenta). → `crs:BlueHue`.
    #[serde(skip_serializing_if = "is_zero")]
    pub cal_blue_hue: f32,
    /// Blue primary saturation, -100..=100. → `crs:BlueSaturation`.
    #[serde(skip_serializing_if = "is_zero")]
    pub cal_blue_sat: f32,

    // --- Geometry (optional) ------------------------------------------------
    /// Clockwise straighten angle in degrees, e.g. -2.5..=2.5 for horizons.
    pub straighten_deg: f32,
    /// How many CLOCKWISE quarter turns the PHOTOGRAPHER asked for, on top of
    /// whatever the camera's EXIF orientation already says. 0..=3, 0 = none.
    ///
    /// Composed with the EXIF value into ONE
    /// [`crate::render::compose_orientation`] result, which is what every
    /// orientation consumer reads — the engine's eight-state dihedral group is
    /// closed under composition, so a user turn on a `Transpose`-oriented file
    /// lands on a state `oriented`/`orient_point` already handle. The
    /// coordinates in [`crop`](Self::crop) and every `masks` geometry are
    /// stored in the frame this turn PRODUCES, exactly as they are stored in
    /// the frame the EXIF orientation produces (see [`COORD_ERA`]).
    ///
    /// Deliberately NOT folded into [`coord_era`](Self::coord_era) — the
    /// argument that field already carries, one step further: `coord_era` is
    /// which frame the stored numbers are IN (a storage epoch, migrated once
    /// and never again), `quarter_turns` is what the photographer ASKED FOR (a
    /// live edit, changed as often as any slider). Merging them would make an
    /// undo of a rotation look like a re-migration.
    ///
    /// **Serialisation: skipped when zero**, unlike `coord_era`/`schema_era`,
    /// which are always written. The difference is not style: an ABSENT era
    /// key means something DIFFERENT from the value a fresh recipe stamps
    /// (absent = legacy, so it must always be spelled out), whereas an absent
    /// `quarter_turns` means exactly 0, which is what it would have written.
    /// Skipping is therefore lossless, and it buys two things. (1) A recipe
    /// nobody rotated stays BYTE-IDENTICAL to what v0.32 wrote, so the R21
    /// deleted-version registry's structural arm (`store::recipe_struct_hash`
    /// re-serialises the whole recipe) keeps matching and needs no re-archive
    /// pass. (2) The class-① forward break that `deny_unknown_fields` above
    /// makes unavoidable is confined to recipes that actually carry a turn: a
    /// v0.32 exe still reads every un-rotated v0.33 `recipe.json`, and hard-
    /// rejects — by field name — exactly the ones it would have rendered
    /// sideways.
    #[serde(skip_serializing_if = "no_quarter_turn")]
    pub quarter_turns: u8,
    /// Optional crop as normalised [0,1] coordinates of the kept region.
    pub crop: Option<Crop>,

    // --- Free-form tone curve (optional) ------------------------------------
    /// Monotonic control points on the master tone curve, input/output in
    /// 0..=255. Empty = identity curve.
    pub tone_curve: Vec<CurvePoint>,

    /// Per-channel RGB tone curves (red/green/blue), input/output 0..=255. Empty =
    /// identity. The colour-shaping companion to `tone_curve`; emitted as
    /// `crs:ToneCurvePV2012Red/Green/Blue`.
    pub red_curve: Vec<CurvePoint>,
    pub green_curve: Vec<CurvePoint>,
    pub blue_curve: Vec<CurvePoint>,

    // --- Parametric tone curve (v1.5.0) --------------------------------------
    // Lightroom's Tone Curve panel in its parametric mode: four REGION sliders
    // over three movable SPLITS, composed between the Basic-panel tone model
    // and the point curve above (`render::parametric_lut`). Lightroom writes
    // all seven keys on every sidecar — the three 9.4 rewrites in the payload
    // fixtures carry `ParametricShadows="0"` … `ParametricHighlightSplit="75"`
    // — so, as for de-fringe, the neutral is ADOBE'S DEFAULT (splits
    // 25/50/75) and the reader falls back to it for an absent key.
    //
    // **Serialisation: each field is skipped at that default**, by
    // `colour_field`'s argument: a recipe that never moved the curve stays
    // BYTE-IDENTICAL to what v1.4 wrote (so `store::recipe_struct_hash` keeps
    // matching every archived version), and an older build meets an unknown
    // key only in a recipe that really uses one.
    /// Parametric Shadows region, -100..=100. → `crs:ParametricShadows`.
    #[serde(skip_serializing_if = "is_zero")]
    pub param_shadows: f32,
    /// Parametric Darks region, -100..=100. → `crs:ParametricDarks`.
    #[serde(skip_serializing_if = "is_zero")]
    pub param_darks: f32,
    /// Parametric Lights region, -100..=100. → `crs:ParametricLights`.
    #[serde(skip_serializing_if = "is_zero")]
    pub param_lights: f32,
    /// Parametric Highlights region, -100..=100. → `crs:ParametricHighlights`.
    #[serde(skip_serializing_if = "is_zero")]
    pub param_highlights: f32,
    /// Where Shadows ends and Darks begins, in percent of the input range
    /// (ACR default 25). → `crs:ParametricShadowSplit`.
    #[serde(skip_serializing_if = "at_shadow_split")]
    pub param_shadow_split: f32,
    /// Darks | Lights (ACR default 50). → `crs:ParametricMidtoneSplit`.
    #[serde(skip_serializing_if = "at_midtone_split")]
    pub param_midtone_split: f32,
    /// Lights | Highlights (ACR default 75). → `crs:ParametricHighlightSplit`.
    #[serde(skip_serializing_if = "at_highlight_split")]
    pub param_highlight_split: f32,

    // --- Camera profile and creative Look (v1.5.0, F7) ----------------------
    /// `crs:CameraProfile` — the camera profile Lightroom developed this
    /// photograph through. "Adobe Standard" on 160 of the 175 sidecars in the
    /// library this was measured against.
    ///
    /// Until v1.5.0 this name rode to the sidecar through `passthrough` and the
    /// engine rendered its colour from the RAW's own embedded matrix instead,
    /// so the name was a label on a rendering that did not obey it. It is now
    /// an OWNED key: [`crate::dcp`] reads Adobe's own `.dcp` from the user's
    /// install and the render applies its tables. Empty = no profile named, and
    /// the render is exactly what it was before.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub camera_profile: String,

    /// The creative profile `crs:Look` names — "Adobe Color" on 152 of those
    /// 175 sidecars — read for RENDERING only.
    ///
    /// **Read, not written.** The `<crs:Look>` ELEMENT keeps the verbatim merge
    /// path it has always had, because carrying Adobe's own markup through
    /// untouched is stricter than anything this struct could re-serialise. What
    /// this field adds is the half the engine can act on: the Look's baked tone
    /// curves, its baked slider moves and its black-and-white switch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub look: Option<CreativeLook>,

    // --- Camera-matched base look (engine-only, NEVER exported to XMP) -------
    /// Base tone curve `[x, y]` knots (luma, 0..=1) mapping the NEUTRAL raw
    /// develop toward the camera's own rendition, estimated per photo by
    /// CDF-matching block means of the neutral develop against the embedded
    /// preview (`render::camera_base_knots`). The engine composes it UNDER the user
    /// tone controls (`build_tone_lut`), so sliders start from a camera-like
    /// base instead of the darker scene-referred develop. Empty = no base look
    /// — exactly the pre-0.14 rendering, which every legacy recipe.json
    /// deserialises to, so existing saved develops keep their appearance.
    /// Lightroom renders XMP over its own camera profile, so xmp.rs ignores it.
    pub base_curve: Vec<[f32; 2]>,

    // --- In-camera lens profile corrections (engine-only, NEVER in XMP) ------
    /// Per-photo lens corrections read from the RAW's own metadata (Sony
    /// 0x7031-0x7037 — see `lensmeta`): the manufacturer's exact profile for
    /// THIS lens at THIS focal/aperture, no lensfun database needed. The
    /// default (empty, everything off) is what every legacy recipe.json
    /// deserialises to, so existing saved develops render byte-identically.
    /// Lightroom applies its own profile db to XMP, so xmp.rs ignores this.
    pub lens_profile: LensProfile,

    // --- Local (masked) adjustments -----------------------------------------
    /// Local adjustments applied through gradient masks. Empty = global-only
    /// (v1-compatible). Emitted as `crs:MaskGroupBasedCorrections` in the XMP
    /// and composited by the render engine.
    pub masks: Vec<LocalAdjustment>,

    /// Lightroom's spot removal (`crs:RetouchAreas`), v1.5.0 F9 — the dust,
    /// the power line, the stranger on the beach.
    ///
    /// This is the one place the parametric develop holds a PIXEL operation,
    /// and it is here because in Lightroom that is exactly what it is: a
    /// develop setting, stored in the sidecar, re-rendered on every open,
    /// undone by resetting the photo. Leaving it out did not leave it out of
    /// the photograph — it put every removed object back, on 25 of the
    /// reference library's 175 photographs, with nothing on screen to say so.
    ///
    /// The geometry is the authority and the pixels are not: for 116 of the
    /// 121 measured areas Lightroom records WHERE but not WHAT, because its
    /// own result lives in Adobe's store. So this engine re-solves each area
    /// from the frame's own pixels ([`retouch::heal_planar`]) and says which
    /// areas those are ([`retouch::SpotOrigin`]).
    ///
    /// NOT written back by this engine: `crs:RetouchAreas` is not one of the
    /// writer's owned elements, so a merge carries the photographer's original
    /// block through verbatim, Adobe's patch references and all.
    ///
    /// [`retouch::heal_planar`]: crate::retouch::heal_planar
    /// [`retouch::SpotOrigin`]: crate::retouch::SpotOrigin
    #[serde(default)]
    pub retouch: Vec<crate::retouch::RetouchArea>,

    /// Generated patches over the finished develop (2026-10-01): a generative
    /// fill made on this card lands HERE, as a layer, instead of as a new ✨
    /// card — see [`crate::render::PixelLayer`]. Engine-only and, like
    /// `retouch`, rendered but never written to the sidecar: Lightroom has no
    /// element for a pixel layer, and the save line says so.
    #[serde(default)]
    pub pixel_layers: Vec<crate::render::PixelLayer>,

    /// The AI lens blur (2026-10-13): a depth map the MoGe-2 sidecar estimated
    /// for this photo, a focus point and two dials — see
    /// [`crate::render::LensBlur`]. Engine-only and rendered but never written
    /// to the sidecar, like `pixel_layers`: Lightroom has no element for it.
    /// **Skipped when absent**, by `colour_field`'s argument: a recipe that
    /// never used it serialises byte for byte as before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens_blur: Option<crate::render::LensBlur>,

    // --- HDR edit mode and its SDR rendition (v1.5.0 F8) ---------------------
    // Lightroom's HDR mode re-labels the capture: diffuse white stops being the
    // top of the frame and the brightest stops become HEADROOM above it, with
    // `crs:HDRMaxValue` saying how many. Every file this engine can write is
    // still SDR — 8- or 16-bit sRGB — so the picture we owe such a photograph
    // is Lightroom's own answer to the same problem: the SDR RENDITION, the
    // seven-control panel Lightroom shows only in HDR mode.
    //
    // All nine are neutral at their defaults, so a photograph that never
    // entered HDR mode renders byte-identically to what v1.4 produced.
    //
    // CALIBRATION, stated plainly: the reference library has 114 sidecars
    // carrying `crs:HDREditMode="0"` and NOT ONE carrying `"1"`, so the shape
    // of the model below is reasoned from Lightroom's own vocabulary and from
    // this engine's already-calibrated operators, not fitted to a Lightroom
    // rendition. `render::hdr` states each constant and what would replace it.
    /// Was this photograph edited in HDR mode? → `crs:HDREditMode` (`"0"`/`"1"`).
    ///
    /// The GATE, exactly as in Lightroom, where the SDR panel exists only here:
    /// with this false the seven controls below render nothing, so a sidecar
    /// that carries a stale `crs:SDRBrightness` from an HDR session the
    /// photographer later left cannot silently re-tone an SDR photograph.
    #[serde(skip_serializing_if = "is_false")]
    pub hdr_edit: bool,
    /// The headroom, in STOPS above diffuse white → `crs:HDRMaxValue`
    /// (Lightroom writes `"+1.00"`). 0 = no headroom, and then HDR mode is a
    /// label with no tonal consequence.
    #[serde(skip_serializing_if = "is_zero")]
    pub hdr_max_ev: f32,
    /// How much of the headroom's shoulder reaches the SDR rendition,
    /// -100..=100 → `crs:SDRBlend`. 0 = the model's own amount, −100 = none
    /// (the SDR rendition ignores the headroom), +100 = twice it.
    #[serde(skip_serializing_if = "is_zero")]
    pub sdr_blend: f32,
    /// The SDR rendition's exposure, -100..=100 → `crs:SDRBrightness`. Named
    /// Brightness and not Exposure because the SDR panel has no Exposure
    /// slider: this IS that rendition's exposure control.
    #[serde(skip_serializing_if = "is_zero")]
    pub sdr_brightness: f32,
    /// -100..=100 → `crs:SDRContrast`.
    #[serde(skip_serializing_if = "is_zero")]
    pub sdr_contrast: f32,
    /// -100..=100 → `crs:SDRHighlights`.
    #[serde(skip_serializing_if = "is_zero")]
    pub sdr_highlights: f32,
    /// -100..=100 → `crs:SDRShadows`.
    #[serde(skip_serializing_if = "is_zero")]
    pub sdr_shadows: f32,
    /// -100..=100 → `crs:SDRWhites`.
    #[serde(skip_serializing_if = "is_zero")]
    pub sdr_whites: f32,
    /// -100..=100 → `crs:SDRClarity`. The same local-contrast operator the
    /// Basic panel's Clarity uses, at the same radius rule — one calibration,
    /// so "Clarity +30" means one structure wherever it is written.
    #[serde(skip_serializing_if = "is_zero")]
    pub sdr_clarity: f32,

    // --- Colour field (engine-only) -----------------------------------------
    /// A smooth 12×32×8 bilateral grid of local colour/tone deltas: the
    /// residual the mask-shaped controls above cannot reach, carried as one
    /// low-frequency field instead of as more rectangles.
    ///
    /// **Engine-only, and the first control in this struct that is.** Every
    /// other field here either projects to a `crs:` property or, like a bitmap
    /// mask, has a named XMP loss because Lightroom's own model has no place
    /// for it. This one is a whole coordinate system Lightroom does not have,
    /// so it is disclosed as a GLOBAL export loss
    /// ([`crate::advisor::catalogue::Tier::RenderedNotExported`]) and the
    /// sidecar carries the rest of the recipe unchanged.
    ///
    /// **Serialisation: skipped when absent**, following
    /// [`quarter_turns`](Self::quarter_turns)'s argument exactly. `None` and
    /// "the key is not there" mean the same thing, so skipping is lossless; it
    /// keeps a recipe that has no field BYTE-IDENTICAL to what the previous
    /// build wrote (so `store::recipe_struct_hash` still matches every
    /// archived version and needs no re-archive pass); and it confines the
    /// class-① forward break `deny_unknown_fields` makes unavoidable to the
    /// recipes that actually carry one.
    ///
    /// **[`SCHEMA_ERA`] is deliberately NOT bumped**, and that is not an
    /// oversight: that era's one consumer is
    /// `xmp::unspoken_attr_keys`, which asks a single question — has
    /// this recipe ever seen the twenty-seven R25 `crs:` keys? A colour field
    /// owns no `crs:` key and can own none. Bumping would re-classify every
    /// era-1 recipe in every store as legacy and start suppressing those
    /// twenty-seven keys again on the next merge, which is a real regression
    /// bought for nothing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colour_field: Option<ColourField>,

    // --- Provenance (the AI explains itself) --------------------------------
    /// One or two sentences: why these adjustments, for the user to sanity-check.
    pub rationale: String,
    /// AI self-reported confidence, 0.0..=1.0. Used to gate auto-apply.
    pub confidence: f32,
}

/// One smooth field of local colour and tone, on the same bilateral grid the
/// reverse fit analyses with ([`crate::fit_field`]): `x` × `y` spatial cells
/// × `b` luma bins, five parameters per vertex, read trilinearly per pixel.
///
/// The five are the analyzer's, in its order: EV, then a per-channel gain for
/// R, G and B, then a slope against the pixel's own smoothed luma. The render
/// is `clamp(c + delta, 0, 1)` with
/// `delta_c = ln2·c·EV + c·gain_c + (c - guide)·slope`, scaled by
/// [`amount`](Self::amount) — one shared function, so the engine and the
/// analyzer cannot drift apart
/// ([`crate::render::apply_colour_field`]).
///
/// `grid` is `x * y * b` vertices in splat order (x-major, then y, then the
/// luma bin), which is the order [`crate::fit_field`] solves and stores them
/// in. A field whose length does not match its own declared shape is not a
/// field: the render leaves the frame untouched rather than guessing which
/// axis was meant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColourField {
    /// Spatial cells across. 0 disables the field as surely as `enabled: false`.
    pub x: usize,
    /// Spatial cells down.
    pub y: usize,
    /// Luma bins.
    pub b: usize,
    /// `x * y * b` vertices, five parameters each, in splat order.
    pub grid: Vec<[f32; 5]>,
    /// How much of the field to apply, 0..=1. The slider the user gets.
    pub amount: f32,
    /// The user's on/off, kept separate from `amount` so a field parked at 0
    /// is distinguishable from one switched off with its strength remembered.
    pub enabled: bool,
}

impl Default for ColourField {
    fn default() -> Self {
        Self { x: 0, y: 0, b: 0, grid: Vec::new(), amount: 1.0, enabled: true }
    }
}

impl ColourField {
    /// Whether this field can be rendered at all: switched on, carrying some
    /// amount, and holding exactly the vertices its own shape declares.
    pub fn renderable(&self) -> bool {
        self.enabled
            && self.amount > 0.0
            && self.x > 0
            && self.y > 0
            && self.b > 0
            && self.grid_fits_shape()
    }

    /// Whether `grid` holds exactly the `x * y * b` vertices the shape
    /// declares — the one invariant every vertex read in the engine rests on:
    /// [`crate::render::apply_colour_field`] and the frame turns index `grid`
    /// by (x, y, b) with no bounds check of their own once [`renderable`]
    /// has said yes. The product is CHECKED, not wrapped: a posted recipe
    /// whose `x * y * b` overflowed to exactly 0 matched an empty grid, and
    /// the render's first vertex read panicked.
    ///
    /// [`renderable`]: Self::renderable
    pub fn grid_fits_shape(&self) -> bool {
        self.x.checked_mul(self.y).and_then(|xy| xy.checked_mul(self.b)) == Some(self.grid.len())
    }

    /// Round every stored parameter to 1e-4, so a solved field's JSON is
    /// stable across re-solves that agree to four decimals and a hand edit is
    /// readable.
    pub fn round(&mut self) {
        for vertex in &mut self.grid {
            for p in vertex {
                *p = (*p * 10_000.0).round() / 10_000.0;
            }
        }
        self.amount = (self.amount.clamp(0.0, 1.0) * 10_000.0).round() / 10_000.0;
    }
}

/// The current calibration era, stamped into every recipe we write.
///
/// Era 1 = any build up to R12 batch 43. Its working-resolution cap added
/// +0.5 to every channel of every capped frame, and `photo_base_knots`
/// estimates the camera base curve from exactly such a capped develop — so an
/// era-1 `base_curve` may encode a washed frame. Era 2 (up to v1.5.1) = the
/// estimate from a PIXEL histogram match, which took the camera rendition's
/// sharpening, noise and JPEG texture for tone and crowded its knots on a
/// night sky. Era 3 (v1.6.0) = the match made on block means
/// (`render::camera_base_knots`). A stamp below the current era is what lets
/// a later load tell an older estimate apart and replace it
/// (`pipeline::base_curve_is_pre_era`); nothing else in the file could.
pub const CALIB_ERA: u32 = 3;

/// The current COORDINATE-FRAME era, stamped into every recipe we write.
///
/// Era 0 = every recipe written up to v0.29.x. rawler 0.7.2 reports
/// `Orientation::Normal` for every non-DNG RAW (`decode::raw_orientation_of`
/// has the crate-source citation), so a portrait ARW was displayed, developed
/// and exported in its SENSOR frame — and the crop rectangle and mask
/// geometries the user drew on it were stored against that sideways canvas.
/// Era 1 = the display frame the EXIF orientation actually asks for, which is
/// what `render::orient_f32` now produces and what the C2 coordinate contract
/// ("masks live in the ORIGINAL frame") has always claimed.
///
/// For a `Normal`-oriented photo the two frames are identical, so era 1
/// only ever changes what a rotated/flipped RAW's saved geometry means.
///
/// Era 2 (v1.6.0) = the same display frame, measured from the develop window
/// THIS build cuts — the RAW's declared crop origin. Up to v1.5.1 a body that
/// declares a crop but no active area (the ILCE-7RM4A) was developed from the
/// sensor's corner, so an era-≤1 recipe on such a file is drawn `(32, 20)` px
/// off the picture; `pipeline::migrate_recipe_coord_frame` translates it once
/// (`decode::SourceWindow::legacy_shift`, `render::shift_recipe_coords`) and
/// re-writes its raster masks as moved files. For every other file the two
/// eras are the same frame and only the stamp moves.
pub const COORD_ERA: u32 = 2;

/// Serde default for [`EditRecipe::coord_era`] — deliberately NOT the
/// container's `Default::default()` value: a file with no `coord_era` key was
/// written before the field existed, and every one of those holds SENSOR-frame
/// coordinates. Pinned by `absent_coord_era_reads_as_the_legacy_frame`.
fn coord_era_legacy() -> u32 {
    0
}

/// The current CONTROL-SET era, stamped into every recipe this build writes.
///
/// Era 0 = every recipe written up to v0.30.x, whose JSON has no key for the
/// twenty-seven `crs:` properties R25 added to the writer — global `texture`,
/// the nine carried effects, the eight detail axes, the manual CA pair, the
/// auto-CA switch and the six de-fringe keys. Serde fills those fields from
/// [`EditRecipe::default`], so an era-0 recipe SAYS "texture 0, no grain,
/// Adobe's own de-fringe windows" without ever having read a document.
/// Era 1 = written by a build that has all twenty-seven, so its values for
/// them are statements.
///
/// Era 2 = v1.5.0, which added the keys named in [`V150_CONTROLS`] (the
/// parametric tone curve first). An era-1 recipe predates them exactly as an
/// era-0 one predates R25's, and [`SCHEMA_ERA_CONTROLS`] says which era
/// brought which control, so a recipe is gated only on the keys it has never
/// seen: an era-1 file still owns every R25 key.
///
/// The one consumer is [`crate::xmp::unspoken_attr_keys`]: on a recipe
/// older than a control, the merge neither strips nor re-emits that control's
/// key while it sits at its untouched default, so the base document's own
/// bytes stand. (`pipeline::carry_over_unrepresentable` keeps a Refine from
/// laundering the stamp: see its era rule.)
pub const SCHEMA_ERA: u32 = 2;

/// The controls each control-set era introduced, by registry name: index 0 is
/// era 1, index 1 era 2. A new era appends its list and bumps [`SCHEMA_ERA`];
/// a closed era's list never changes, whatever later happens to its controls'
/// tiers (a carried control that starts rendering has still been in every
/// era-1 file).
pub const SCHEMA_ERA_CONTROLS: [&[&str]; 2] = [&R25_CONTROLS, &V150_CONTROLS];

/// Era 1 (R25): global Texture, the eight Detail axes, the manual CA pair, the
/// auto-CA switch, the six de-fringe keys and the nine effects.
pub const R25_CONTROLS: [&str; 27] = [
    "texture",
    "sharpen_radius",
    "sharpen_detail",
    "sharpen_mask",
    "nr_detail",
    "nr_contrast",
    "color_nr",
    "color_nr_detail",
    "color_nr_smooth",
    "ca_r",
    "ca_b",
    "auto_lateral_ca",
    "defringe_purple",
    "defringe_purple_lo",
    "defringe_purple_hi",
    "defringe_green",
    "defringe_green_lo",
    "defringe_green_hi",
    "post_crop_vignette",
    "post_crop_vignette_mid",
    "post_crop_vignette_feather",
    "post_crop_vignette_round",
    "post_crop_vignette_style",
    "post_crop_vignette_hl",
    "grain",
    "grain_size",
    "grain_rough",
];

/// Era 2 (v1.5.0): the controls that release added a `crs:` key for — the
/// parametric curve, the Calibration panel, the B&W treatment and mixer, and
/// the point colours (an element, so the era gate's ATTRIBUTE list skips it
/// and the merge's element rule reads the stamp itself).
pub const V150_CONTROLS: [&str; 45] = [
    // F7: the camera profile's NAME. An era-1 recipe predates the field, so
    // the gate keeps the merge from stripping `crs:CameraProfile` out of a
    // Lightroom document that a recipe of ours simply has nothing to say about.
    "camera_profile",
    // F5: the lens profile's two strengths. Their neutral is 100, so the era
    // gate matters MORE for them than for a zero-neutral key: an era-1 recipe
    // that never saw them would otherwise re-emit `…Scale="100"` over a
    // document where Lightroom wrote something else.
    "lens_profile_distortion_scale",
    "lens_profile_vignetting_scale",
    // F6: the Transform panel. Seven sliders, the Upright mode and Lightroom's
    // Constrain Crop — every one of them a key this writer only CARRIED before
    // v1.5.0, so an era-1 recipe has never held one.
    "perspective_vertical",
    "perspective_horizontal",
    "perspective_rotate",
    "perspective_scale",
    "perspective_aspect",
    "perspective_x",
    "perspective_y",
    "perspective_upright",
    "crop_constrain_to_warp",
    "param_shadows",
    "param_darks",
    "param_lights",
    "param_highlights",
    "param_shadow_split",
    "param_midtone_split",
    "param_highlight_split",
    "cal_shadow_tint",
    "cal_red_hue",
    "cal_red_sat",
    "cal_green_hue",
    "cal_green_sat",
    "cal_blue_hue",
    "cal_blue_sat",
    "convert_to_grayscale",
    "gray_red",
    "gray_orange",
    "gray_yellow",
    "gray_green",
    "gray_aqua",
    "gray_blue",
    "gray_purple",
    "gray_magenta",
    "point_colors",
    // F8: HDR edit mode and the seven SDR-rendition controls. Nine keys this
    // writer had never emitted, so no era-1 recipe has ever held one.
    "hdr_edit",
    "hdr_max_ev",
    "sdr_blend",
    "sdr_brightness",
    "sdr_contrast",
    "sdr_highlights",
    "sdr_shadows",
    "sdr_whites",
    "sdr_clarity",
];

/// The COMPANION controls: shaping axes whose Lightroom default is NOT zero,
/// with that default. A stored 0 on one of them means "the sidecar said
/// nothing" — Lightroom's default applies — unless the control is named in
/// [`EditRecipe::explicit_zero`]. [`EditRecipe::resolved`] is the one reader.
///
/// Every default is Adobe's own, read off the user's sidecars where Lightroom
/// materialises them (`Sharpness="40"`, `SharpenRadius="+1.0"`,
/// `SharpenDetail="25"`, the Detail and Smoothness 50s) and off Lightroom's
/// panels for the effects block.
/// Style 1 is Highlight Priority. Radius and Style have no real 0 in
/// Lightroom (bands 0.5..3 and 1..3); the list still names them so every
/// member resolves the same way. The Sharpening amount (v1.6.0) is the one
/// member whose default is the SOURCE KIND's — [`LR_RAW_SHARPENING`] on a
/// RAW negative, none on a baked raster — so the render reads it through
/// [`EditRecipe::capture_sharpening`], never through `resolved` alone.
pub const LR_COMPANION_DEFAULTS: [(&str, f32); 11] = [
    ("sharpening", LR_RAW_SHARPENING),
    ("sharpen_radius", 1.0),
    ("sharpen_detail", 25.0),
    ("nr_detail", 50.0),
    ("color_nr_detail", 50.0),
    ("color_nr_smooth", 50.0),
    ("grain_size", 25.0),
    ("grain_rough", 50.0),
    ("post_crop_vignette_mid", 50.0),
    ("post_crop_vignette_feather", 50.0),
    ("post_crop_vignette_style", 1.0),
];

/// Lightroom's default Sharpening amount for a RAW negative — `Sharpness="40"`
/// on every RAW sidecar the user's Lightroom wrote untouched (six of the seven
/// reference files; the seventh was moved to 35) — and the amount a RAW whose
/// recipe holds none renders at ([`EditRecipe::capture_sharpening`], v1.6.0).
/// Lightroom gives a baked raster (JPEG / TIFF) no sharpening by default, so
/// this is the table's RAW entry, not a plain default: before v1.6.0 the app
/// rendered every fresh RAW unsharpened and wrote `Sharpness="0"` for it,
/// 40 short of the Lightroom develop it was measured against.
pub const LR_RAW_SHARPENING: f32 = 40.0;

/// Serde default for [`EditRecipe::schema_era`] — deliberately NOT the
/// container's `Default::default()` value, the same field-level-beats-
/// container-level trap [`coord_era_legacy`] documents: a file with no
/// `schema_era` key was written before the field existed, and every one of
/// those predates the R25 keys. Pinned by
/// `absent_schema_era_reads_as_the_legacy_control_set`.
fn schema_era_legacy() -> u32 {
    0
}

/// `skip_serializing_if` predicate for [`EditRecipe::quarter_turns`] — see
/// that field's doc for why THIS field is skipped when the era stamps beside
/// it never are. Pinned by
/// `an_unrotated_recipe_serialises_exactly_as_the_previous_build_wrote_it`.
#[allow(clippy::trivially_copy_pass_by_ref)] // serde's predicate signature
fn no_quarter_turn(k: &u8) -> bool {
    *k == 0
}

/// `skip_serializing_if` predicates for the v1.5.0 controls, each true at the
/// control's neutral — see the parametric block's doc in [`EditRecipe`].
#[allow(clippy::trivially_copy_pass_by_ref)] // serde's predicate signature
fn is_zero(v: &f32) -> bool {
    *v == 0.0
}
#[allow(clippy::trivially_copy_pass_by_ref)] // serde's predicate signature
fn is_false(v: &bool) -> bool {
    !*v
}
#[allow(clippy::trivially_copy_pass_by_ref)] // serde's predicate signature
fn at_shadow_split(v: &f32) -> bool {
    *v == PARAMETRIC_SPLITS[0]
}
#[allow(clippy::trivially_copy_pass_by_ref)] // serde's predicate signature
fn at_midtone_split(v: &f32) -> bool {
    *v == PARAMETRIC_SPLITS[1]
}
#[allow(clippy::trivially_copy_pass_by_ref)] // serde's predicate signature
fn at_highlight_split(v: &f32) -> bool {
    *v == PARAMETRIC_SPLITS[2]
}

/// Lightroom's parametric-curve splits at rest, in percent of the input range:
/// Shadows | Darks, Darks | Lights, Lights | Highlights.
pub const PARAMETRIC_SPLITS: [f32; 3] = [25.0, 50.0, 75.0];

/// How close two parametric splits, or a split and an end of the range, may
/// come — Lightroom's own panel stops a split 10 short of its neighbours.
pub const PARAMETRIC_SPLIT_GAP: f32 = 10.0;

/// The range split `i` (0 = Shadows | Darks) can reach when its neighbours
/// make room — 10..=70, 20..=80 and 30..=90, each split's own panel range.
pub fn parametric_split_band(i: usize) -> (f32, f32) {
    let gap = PARAMETRIC_SPLIT_GAP;
    (gap * (i + 1) as f32, 100.0 - gap * (3 - i) as f32)
}

impl Default for EditRecipe {
    fn default() -> Self {
        Self {
            version: CALIB_ERA,
            coord_era: COORD_ERA,
            schema_era: SCHEMA_ERA,
            exposure_ev: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            temperature_k: None,
            tint: 0.0,
            as_shot_k: None,
            as_shot_tint: None,
            vibrance: 0.0,
            saturation: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            texture: 0.0,
            hsl: Hsl::default(),
            convert_to_grayscale: false,
            gray_red: 0.0,
            gray_orange: 0.0,
            gray_yellow: 0.0,
            gray_green: 0.0,
            gray_aqua: 0.0,
            gray_blue: 0.0,
            gray_purple: 0.0,
            gray_magenta: 0.0,
            point_colors: Vec::new(),
            color_grade: ColorGrade::default(),
            sharpening: 0.0,
            noise_reduction: 0.0,
            sharpen_radius: 0.0,
            sharpen_detail: 0.0,
            sharpen_mask: 0.0,
            nr_detail: 0.0,
            nr_contrast: 0.0,
            color_nr: 0.0,
            color_nr_detail: 0.0,
            color_nr_smooth: 0.0,
            lens_vignette: 0.0,
            lens_vignette_mid: 50.0,
            lens_distortion: 0.0,
            // 100 = "the profile's own map / gains", the only non-zero-neutral
            // pair outside the de-fringe block.
            lens_profile_distortion_scale: 100.0,
            lens_profile_vignetting_scale: 100.0,
            perspective_vertical: 0.0,
            perspective_horizontal: 0.0,
            perspective_rotate: 0.0,
            perspective_scale: 100.0,
            perspective_aspect: 0.0,
            perspective_x: 0.0,
            perspective_y: 0.0,
            perspective_upright: 0.0,
            upright_transform: Vec::new(),
            crop_constrain_to_warp: false,
            ca_r: 0.0,
            ca_b: 0.0,
            auto_lateral_ca: false,
            // The one NON-ZERO neutral block in this struct — Adobe's own
            // defaults, because Lightroom writes all six keys on every
            // sidecar and a hue window of 0..0 is not a state any real
            // document expresses (see the field docs).
            defringe_purple: 0.0,
            defringe_purple_lo: 30.0,
            defringe_purple_hi: 70.0,
            defringe_green: 0.0,
            defringe_green_lo: 40.0,
            defringe_green_hi: 60.0,
            post_crop_vignette: 0.0,
            post_crop_vignette_mid: 0.0,
            post_crop_vignette_feather: 0.0,
            post_crop_vignette_round: 0.0,
            post_crop_vignette_style: 0.0,
            post_crop_vignette_hl: 0.0,
            grain: 0.0,
            grain_size: 0.0,
            grain_rough: 0.0,
            explicit_zero: Vec::new(),
            // Empty = the document carried no Transform block and no profile
            // name. There is no "neutral Upright" to invent here: a key we
            // never saw must not be written into somebody's sidecar.
            passthrough: std::collections::BTreeMap::new(),
            cal_shadow_tint: 0.0,
            cal_red_hue: 0.0,
            cal_red_sat: 0.0,
            cal_green_hue: 0.0,
            cal_green_sat: 0.0,
            cal_blue_hue: 0.0,
            cal_blue_sat: 0.0,
            // F8: no HDR mode, no headroom, no SDR rendition — the state every
            // photograph this engine has ever rendered was already in.
            hdr_edit: false,
            hdr_max_ev: 0.0,
            sdr_blend: 0.0,
            sdr_brightness: 0.0,
            sdr_contrast: 0.0,
            sdr_highlights: 0.0,
            sdr_shadows: 0.0,
            sdr_whites: 0.0,
            sdr_clarity: 0.0,
            straighten_deg: 0.0,
            quarter_turns: 0,
            crop: None,
            tone_curve: Vec::new(),
            red_curve: Vec::new(),
            green_curve: Vec::new(),
            blue_curve: Vec::new(),
            param_shadows: 0.0,
            param_darks: 0.0,
            param_lights: 0.0,
            param_highlights: 0.0,
            param_shadow_split: PARAMETRIC_SPLITS[0],
            param_midtone_split: PARAMETRIC_SPLITS[1],
            param_highlight_split: PARAMETRIC_SPLITS[2],
            camera_profile: String::new(),
            look: None,
            base_curve: Vec::new(),
            lens_profile: LensProfile::default(),
            masks: Vec::new(),
            retouch: Vec::new(),
            pixel_layers: Vec::new(),
            lens_blur: None,
            colour_field: None,
            rationale: String::new(),
            confidence: 0.0,
        }
    }
}

/// A creative profile's BAKED half, as the sidecar states it.
///
/// Adobe's creative profiles are two things at once: a set of ordinary develop
/// settings, which Lightroom writes out in full inside `<crs:Look>`, and a
/// three-dimensional colour table, which it writes as an opaque reference. This
/// struct is the first half — everything the engine can render — plus the NAME
/// of the second, so the part that is not rendered can be disclosed instead of
/// quietly dropped.
///
/// The field list is the measured vocabulary, not a guess: across the 161 Looks
/// in the library the Parameters block carries exactly `Version`,
/// `ProcessVersion`, `ConvertToGrayscale`, `CameraProfile`, `LookTable`, plus
/// `Clarity2012` (9 files), `Highlights2012` and `Shadows2012` (5 each), and
/// the four `ToneCurvePV2012*` children. Anything outside that set lands in
/// [`unrendered`](Self::unrendered) by name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct CreativeLook {
    /// `crs:Name` — "Adobe Color", "Adobe Landscape", "Adobe Monochrome".
    pub name: String,
    /// `crs:Amount`. 1 on every one of the 161 Looks measured; a Look whose
    /// profile sets `SupportsAmount` can carry less, and the render scales the
    /// baked half by it.
    pub amount: f32,
    /// The Look's OWN `crs:CameraProfile` — the camera profile it sits on top
    /// of, "Adobe Standard" on all 161. This is what the render resolves when
    /// the Description's own profile is absent.
    pub base_profile: String,
    /// `crs:LookTable`: a 32-hex reference to the creative colour table. Carried
    /// so the disclosure can NAME what is missing; see [`crate::dcp`] for what
    /// was measured about that payload and why it is not decodable.
    pub table: String,
    /// The Look's baked `crs:ConvertToGrayscale` — `true` on the four "Adobe
    /// Monochrome" files, which is how Lightroom 9.4 spells a black-and-white
    /// treatment. The Description's own switch stays the photographer's.
    pub grayscale: bool,
    /// Baked `crs:Clarity2012` / `Highlights2012` / `Shadows2012`.
    pub clarity: f32,
    pub highlights: f32,
    pub shadows: f32,
    /// Baked `crs:ToneCurvePV2012` and its three per-channel companions, in the
    /// same 0..=255 coordinates as the photographer's own curves.
    pub tone_curve: Vec<CurvePoint>,
    pub red_curve: Vec<CurvePoint>,
    pub green_curve: Vec<CurvePoint>,
    pub blue_curve: Vec<CurvePoint>,
    /// crs keys the Look bakes that this engine does not render, by name.
    ///
    /// Sorted and de-duplicated so the disclosure is stable. An empty list means
    /// the Look's whole baked half reached the render — which is the case for
    /// every Look in the measured library once the table is accounted for.
    pub unrendered: Vec<String>,
}

/// The warmth scale's octave: ±100 is a factor of 2^±1 on the white balance.
const WARMTH_OCTAVES: f32 = 1.0;

impl EditRecipe {
    /// The colour temperature on the warm/cool scale the Normal panel shows:
    /// −100 (cool) … +100 (warm), in hundredths of an octave around the
    /// anchor — the photo's stamped as-shot Kelvin, or the historical 5500 K
    /// for a recipe without one. No custom white balance reads 0.
    pub fn warmth(&self) -> f32 {
        match self.temperature_k {
            Some(k) if k > 0.0 => (100.0 / WARMTH_OCTAVES * (k / self.warmth_anchor()).log2())
                .clamp(-100.0, 100.0),
            _ => 0.0,
        }
    }

    /// The Kelvin a warmth writes: the anchor times 2^(warmth/100), inside the
    /// develop track's 2000–50000 K. A higher temperature renders warmer, as
    /// Lightroom's Temp slider does.
    pub fn kelvin_at_warmth(&self, warmth: f32) -> f32 {
        (self.warmth_anchor() * (warmth.clamp(-100.0, 100.0) / 100.0 * WARMTH_OCTAVES).exp2())
            .clamp(2000.0, 50000.0)
    }

    /// The Kelvin the warmth scale is anchored at: the camera's as-shot value
    /// when the recipe carries one, the develop panel's 5500 K otherwise.
    fn warmth_anchor(&self) -> f32 {
        self.as_shot_k.unwrap_or(5500.0)
    }

    /// The Adobe profile a Lightroom sidecar of this recipe has to NAME, or
    /// `None` when it need not or cannot.
    ///
    /// Camera Raw honours crs:LensProfileVignettingScale and
    /// crs:LensProfileDistortionScale only for a profile the sidecar names:
    /// measured on a flat field (vignetting) and on a real frame (distortion),
    /// a strength of 50 renders at 100 under LensDefaults, Auto or no setup
    /// with no crs:LensProfileName, and at 50 once the name is there. So the
    /// export names the profile whenever a strength is off 100 on a component
    /// that is rendering, under the setup [`Self::lightroom_lens_setup`] says.
    /// `None` with a strength off 100 and no profile on this machine is an
    /// export loss (`xmp::global_export_losses`).
    pub fn lightroom_named_lens_profile(&self) -> Option<&AdobeLensProfile> {
        let p = &self.lens_profile;
        let vignette = self.lens_profile_vignetting_scale != 100.0 && p.vignette_active();
        let distortion = self.lens_profile_distortion_scale != 100.0 && p.distortion_on && !p.distortion.is_empty();
        if vignette || distortion { p.adobe.as_ref() } else { None }
    }

    /// The crs:LensProfileSetup a sidecar naming the profile states: the one
    /// whose camera CA matches this recipe's. LensDefaults (coordinator ruling
    /// 2026-10-29: the name alone makes Camera Raw honour a strength, and this
    /// setup keeps the camera's CA, as the render does) — unless the camera's
    /// CA is there and switched off, which Camera Raw draws only under Custom
    /// (measured: no camera CA under Custom, the camera's under LensDefaults).
    pub fn lightroom_lens_setup(&self) -> &'static str {
        let p = &self.lens_profile;
        let ca_off = !p.ca_on && !p.ca_r.is_empty() && !p.ca_b.is_empty();
        if ca_off { "Custom" } else { "LensDefaults" }
    }

    /// Every machine-local raster path this recipe references: each mask's
    /// ([`LocalAdjustment::bitmap_paths_mut`]), the lens blur's depth map and
    /// protect matte, and each pixel layer's patch — the ONE walk the store's
    /// resolve (and its network/escape refusal), relativize, version freeze,
    /// detach and the sidecar payload's bare-naming share.
    ///
    /// Before it those walks iterated the masks alone, so the three non-mask
    /// rasters were read verbatim from a develop store or a sidecar someone
    /// else wrote (a `\\host\share` depth map made a render authenticate to
    /// a remote host), stored absolute, and never frozen with a version.
    /// Empty paths name no file and are skipped, as a mask's are.
    pub fn raster_paths_mut(&mut self) -> Vec<&mut String> {
        let mut out: Vec<&mut String> = self.masks.iter_mut().flat_map(|m| m.bitmap_paths_mut()).collect();
        if let Some(lb) = &mut self.lens_blur {
            out.extend(std::iter::once(&mut lb.depth_map).chain(lb.protect.as_mut()).filter(|p| !p.is_empty()));
        }
        out.extend(self.pixel_layers.iter_mut().map(|l| &mut l.path).filter(|p| !p.is_empty()));
        out
    }

    /// The creative profile's baked half, scaled by its own `crs:Amount`, or
    /// `None` when there is nothing baked to apply.
    ///
    /// One door for every reader, so "does this photograph carry a Look that
    /// moves pixels" is asked the same way everywhere — the mistake the B&W
    /// switch would otherwise invite, where the render checks one flag and the
    /// panel another.
    pub fn baked_look(&self) -> Option<&CreativeLook> {
        self.look.as_ref().filter(|l| !l.is_neutral())
    }

    /// Does this photograph render as black and white?
    ///
    /// Two switches say so and they belong to different people: the
    /// photographer's own `crs:ConvertToGrayscale`, and the one baked into a
    /// monochrome creative profile — which is how Lightroom 9.4 spells "Adobe
    /// Monochrome", on four of the 175 sidecars measured. Either is enough.
    pub fn renders_grayscale(&self) -> bool {
        self.convert_to_grayscale || self.baked_look().is_some_and(|l| l.grayscale)
    }

    /// Does the tone stage have anything to do?
    ///
    /// Every input [`crate::render::build_tone_lut`] reads, asked in one place
    /// so the short-circuit and the LUT cannot disagree.
    pub(crate) fn has_tone_work(&self) -> bool {
        self.exposure_ev != 0.0
            || self.contrast != 0.0
            || self.highlights != 0.0
            || self.shadows != 0.0
            || self.whites != 0.0
            || self.blacks != 0.0
            || !self.tone_curve.is_empty()
            || self.parametric_regions().is_some()
            || !self.base_curve.is_empty()
    }

    /// Does this recipe name a camera profile — its own `crs:CameraProfile`,
    /// or the base profile of its creative Look? The one name
    /// `render::resolve_camera_profile` resolves, asked in one place.
    pub fn names_profile(&self) -> bool {
        !self.camera_profile.is_empty()
            || self.look.as_ref().is_some_and(|l| !l.base_profile.is_empty())
    }

    /// Just the start this recipe develops from — its camera base curve, with
    /// the colour of a camera profile and creative Look it names — on
    /// otherwise neutral sliders: what a Before pane shows.
    pub fn start_only(&self) -> EditRecipe {
        EditRecipe {
            base_curve: self.base_curve.clone(),
            camera_profile: self.camera_profile.clone(),
            look: self.look.clone(),
            ..Default::default()
        }
    }

    /// Drop the start a BAKED rendition already carries in its pixels — the
    /// camera base curve, the camera profile and creative Look it was
    /// developed through (v1.17.0), and the lens corrections — so rendering
    /// over it applies none of them twice: a generated master, the camera's
    /// embedded preview, a baked image. The as-shot anchor is each caller's
    /// own rule and stays theirs.
    pub fn strip_baked_start(&mut self) {
        self.base_curve = Vec::new();
        self.camera_profile = String::new();
        self.look = None;
        self.lens_profile = Default::default();
    }
}

impl CreativeLook {
    /// A Look with no baked half changes nothing, whatever its name.
    ///
    /// The table reference alone does NOT count as neutral: it is the thing the
    /// engine cannot render, so a Look that carries only a table is exactly the
    /// case the disclosure exists for.
    pub fn is_neutral(&self) -> bool {
        self.amount == 0.0
            || (!self.grayscale
                && self.clarity == 0.0
                && self.highlights == 0.0
                && self.shadows == 0.0
                && self.tone_curve.is_empty()
                && self.red_curve.is_empty()
                && self.green_curve.is_empty()
                && self.blue_curve.is_empty())
    }
}

/// How many knots [`LensProfile::mask_warp`] carries when it is solved.
///
/// The map used to copy the source profile's sixteen samples. D2 measured up
/// to 0.3 px of interpolation error from that output table, so both producers
/// now publish the same dense canonical table.
pub const MASK_WARP_KNOTS: usize = 64;

/// The local Hue's end stop, either way: [`LocalAdjustment::hue`] runs
/// ±180, the half turn Camera Raw accepts and renders (`crs:LocalHue`
/// ±1.0). The user's own Lightroom sidecars never went past 0.264 (47.5°),
/// so the file range, not a census, sets the stop.
pub const LOCAL_HUE_MAX_DEG: f32 = 180.0;

/// Where [`LensProfile::mask_warp`] came from — or, when it is empty, WHY.
///
/// A typed answer rather than an empty vector, for the reason every disclosure
/// in this codebase is typed: "no warp" has six causes and they send the
/// photographer to six different places (install Camera Raw; this lens has no
/// profile; the sidecar switched the correction off; the profile is a fisheye
/// and we refuse to fake it; …). An empty `Vec` says none of that, and a
/// comment beside the call site is not a channel.
///
/// PERSISTED in `recipe.json` on purpose. The map is solved on the machine that
/// has the camera metadata and the Adobe profile pool; a `recipe.json` opened
/// somewhere else must still be able to say what was known when it was written
/// and what was not, without re-deriving anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MaskWarpSource {
    /// No map, and nothing was tried — the ordinary state for a photograph
    /// with no lens-correction data of any kind.
    #[default]
    Absent,
    /// Solved from the in-camera knots already in `distortion`
    /// (`render::mask_warp_from_camera_knots`). Source A: no external file, and
    /// exactly the polynomial the camera maker calibrated for this shot.
    CameraMetadata,
    /// Solved from an Adobe `.lcp` on this machine (`lcp::solve`).
    /// Source B, for bodies whose RAWs carry no knots.
    Lcp,
    /// The sidecar says `crs:LensProfileEnable="0"`: Lightroom drew NO lens
    /// correction. RADIAL therefore stays at stored coordinates; LINEAR uses
    /// its separately retained camera map to transport two handles into the raw
    /// frame. This is its own variant rather than [`Self::Absent`] because both
    /// the identity radial answer and the LINEAR transport are known facts.
    DisabledInSidecar,
    /// A profile was found and REFUSED because it is a fisheye model. Applying
    /// the rectilinear polynomial to one returns finite, plausible numbers and
    /// would move every mask by an invented amount (`lcp::Refusal::Fisheye`).
    FisheyeRefused,
    /// A profile was named or matched and could not be parsed or solved.
    Unparseable,
    /// This machine has no Adobe lens-profile directory — Camera Raw was never
    /// installed, or the build is not on Windows, where those roots live.
    NoProfileRoots,
}

impl MaskWarpSource {
    /// Every source — the ONE list the disclosure surfaces iterate, exactly as
    /// `xmp::MaskImportReason::ALL` serves the mask half. Pinned by
    /// `mask_warp_source_all_covers_every_variant`.
    pub const ALL: [MaskWarpSource; 7] = [
        MaskWarpSource::Absent,
        MaskWarpSource::CameraMetadata,
        MaskWarpSource::Lcp,
        MaskWarpSource::DisabledInSidecar,
        MaskWarpSource::FisheyeRefused,
        MaskWarpSource::Unparseable,
        MaskWarpSource::NoProfileRoots,
    ];

    /// Did a real map come out of this? The two `true` answers are the only
    /// ones that may be accompanied by a non-empty `mask_warp`, which
    /// `clamp` enforces rather than trusts.
    pub fn is_solved(self) -> bool {
        matches!(self, MaskWarpSource::CameraMetadata | MaskWarpSource::Lcp)
    }

    /// English label for the prose channel (CLI stderr / batch warnings).
    pub fn en(self) -> &'static str {
        match self {
            MaskWarpSource::Absent => "no lens-correction data for this photo",
            MaskWarpSource::CameraMetadata => "solved from the in-camera lens profile",
            MaskWarpSource::Lcp => "solved from an Adobe lens profile (.lcp)",
            MaskWarpSource::DisabledInSidecar => {
                "the sidecar turned the lens profile off - radial stays stored; linear transports handles"
            }
            MaskWarpSource::FisheyeRefused => {
                "the Adobe lens profile is a fisheye model - refused, not applied"
            }
            MaskWarpSource::Unparseable => "the Adobe lens profile could not be read",
            MaskWarpSource::NoProfileRoots => "no Adobe lens-profile directory on this machine",
        }
    }
}

/// In-camera lens profile corrections in ENGINE space: per-knot factors over
/// normalised radius (0 = centre, 1 = corner). The engine's established
/// interpolator places knot `i` at `(i + 0.5) / (n − 1)`. D2 established that
/// Sony's private distortion tag uses `(i+1)/16`; `lensmeta` converts that
/// native domain onto a dense canonical spline for the Lightroom mask solve.
/// Its ordinary render field deliberately remains on the established
/// calibration after the image-registration adjudication gate rejected a
/// render-path change. `vignette` holds linear-light GAINS;
/// `distortion` / `ca_r` / `ca_b` hold radius scale factors (CA multiplies the
/// distortion map per channel).
/// Conversion from the camera's raw integers lives in `lensmeta` — this
/// struct is camera-agnostic on purpose. Engine-only, never written to XMP.
///
/// **R29 Batch-3 adds `mask_warp` + `mask_warp_src`; D2 adds
/// `mask_warp_center`; D2 LINEAR adds `linear_handle_warp`. Each is a HARD
/// FORWARD BREAK.** This struct denies
/// unknown fields, so an older binary refuses a `recipe.json` carrying a frame
/// fact it cannot honour. Backwards is fine (all fields default), forwards is
/// deliberately not: silently dropping a coordinate-frame map would be the
/// one failure mode a mask frame cannot survive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct LensProfile {
    pub vignette: Vec<f32>,
    pub distortion: Vec<f32>,
    pub ca_r: Vec<f32>,
    pub ca_b: Vec<f32>,
    pub vignette_on: bool,
    pub distortion_on: bool,
    pub ca_on: bool,
    /// The RADIAL MASK WARP: `m(r) = r_exported / r_stored`, the radial
    /// magnification between the frame Lightroom STORED a radial point in and
    /// the frame it EXPORTED that point into. LINEAR uses the same solved camera
    /// map only as a two-handle forward transform when no geometry follows; it
    /// never applies this map pointwise. Same knot placement, same interpolator
    /// and same shape as `distortion` above — one convention, two producers
    /// (`render::mask_warp_from_camera_knots` and `lcp::solve`).
    ///
    /// Empty = identity, and `mask_warp_src` says why.
    ///
    /// **This is a LIGHTROOM-frame quantity, not a description of this
    /// engine's own render.** What each consumer must do with it depends on
    /// where that consumer evaluates masks; `render::lr_mask_warp_norm` and
    /// its inverse are the two primitive directions, and their block carries
    /// the frame table.
    ///
    /// NEVER written to XMP. The recipe keeps the sidecar's stored, plain-frame
    /// coordinates verbatim — `xmp::lr_to_engine` / `engine_to_lr` stay exact
    /// inverses of each other and this field is not part of that boundary.
    pub mask_warp: Vec<f32>,
    /// The camera map set aside when the sidecar disabled lens correction, so
    /// that [`Self::mask_warp`] stays empty in that state (RADIAL's measured
    /// stored-coordinate identity). Written for the 2026-08 D2 "H2" LINEAR
    /// rule, which sent a LINEAR's handles through it; **the render no longer
    /// reads it** (v1.17.0: Camera Raw keeps the handles where they are stored
    /// when correction is off, `render::MaskFrame`). Kept so `recipe.json`
    /// files that carry it still read and write unchanged.
    /// Never written to XMP.
    pub linear_handle_warp: Vec<f32>,
    /// Where `mask_warp` came from, or why there is none. See
    /// [`MaskWarpSource`].
    pub mask_warp_src: MaskWarpSource,
    /// Lightroom's radial-map centre in stored-frame pixels, paired with the
    /// stored-frame dimensions those pixels use. Camera-metadata profiles
    /// populate `raw_full_dims/2 - DefaultCropOrigin`; `None` is the legacy
    /// contract and means the current render's stored-frame centre.
    ///
    /// The dimensions are part of this ONE frame fact because previews are
    /// developed after a working-resolution downscale. They let render scale
    /// the pixel centre without guessing the source size.
    ///
    /// Persisted, never written to XMP. Adding it is a deliberate v1.0.0
    /// forward schema break: an older `deny_unknown_fields` reader refuses a
    /// recipe rather than silently discarding a coordinate-frame fact.
    pub mask_warp_center: Option<MaskWarpCenter>,
    /// The Adobe lens profile on this machine that Camera Raw takes for this
    /// photo (`lensmeta::read_for`), by the two names a sidecar gives it.
    /// `None` when there is none. It is what a Lightroom sidecar must name
    /// for Lightroom to honour the profile strengths
    /// ([`EditRecipe::lightroom_named_lens_profile`]).
    ///
    /// Absent from `recipe.json` when `None`, so a recipe without one stays
    /// readable by an older build; one that carries it is the same deliberate
    /// forward break as `mask_warp_center`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adobe: Option<AdobeLensProfile>,
}

/// An Adobe lens profile by the names a Lightroom sidecar gives it:
/// crs:LensProfileName (the file's own `stCamera:ProfileName`) and
/// crs:LensProfileFilename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdobeLensProfile {
    pub name: String,
    pub filename: String,
}

/// One stored-coordinate frame fact for Lightroom's radial mask transport.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaskWarpCenter {
    /// Full-raw-frame centre expressed in stored/default-crop pixels.
    pub stored_px: [f32; 2],
    /// Dimensions of that stored/default-crop pixel frame.
    pub stored_dims: [f32; 2],
}

impl LensProfile {
    /// The camera map the 2026-08 H2 rule moved a LINEAR's handles through with
    /// correction off: the dedicated field in the disabled-sidecar state, else
    /// the ordinary solved map. No render path reads it since v1.17.0.
    pub fn linear_handle_warp(&self) -> &[f32] {
        if self.mask_warp_src == MaskWarpSource::DisabledInSidecar {
            &self.linear_handle_warp
        } else {
            &self.mask_warp
        }
    }

    /// Vignette stage active? (toggle on AND data present)
    pub fn vignette_active(&self) -> bool {
        self.vignette_on && !self.vignette.is_empty()
    }
    /// Any geometric component (distortion / CA) active?
    pub fn geometry_active(&self) -> bool {
        (self.distortion_on && !self.distortion.is_empty())
            || (self.ca_on && !self.ca_r.is_empty() && !self.ca_b.is_empty())
    }
    /// Exactly the state stamping produces: every AVAILABLE component enabled
    /// (a toggle for absent data is meaningless either way). The default
    /// (all empty, all off) satisfies this too — so `is_noop` can treat the
    /// as-stamped profile as calibration-not-an-edit, while any user toggle
    /// AWAY from the stamp counts as an edit and survives saves.
    pub fn is_as_stamped(&self) -> bool {
        // Each toggle must equal "data present" (the != form is clippy's
        // spelling of a boolean-equality with a negation). CA needs the PAIR:
        // a half-damaged file (red knots without blue) renders no CA, so its
        // ca_on=true is NOT the stamped state and must read as an edit.
        let ca_present = !self.ca_r.is_empty() && !self.ca_b.is_empty();
        self.vignette_on != self.vignette.is_empty()
            && self.distortion_on != self.distortion.is_empty()
            && self.ca_on == ca_present
    }
    /// Exactly a state the OPEN path leaves behind — nothing the photographer chose here.
    /// `pipeline::fresh_lens_profile` enables every available camera component;
    /// `pipeline::obey_disabled_profile_switch` keeps the knots but switches
    /// all three off and records `DisabledInSidecar`. The all-empty default
    /// is included by `is_as_stamped` too.
    ///
    /// `EditRecipe::is_noop` deliberately stays on `is_as_stamped`: it feeds
    /// the `SavedDevelop` precedence contest and the unsaved badge, whose
    /// meaning must not move with the section header's as-opened rule.
    pub fn is_as_opened(&self) -> bool {
        self.is_as_stamped()
            || (self.mask_warp_src == MaskWarpSource::DisabledInSidecar
                && !self.vignette_on && !self.distortion_on && !self.ca_on)
    }
    /// Defensive ranges for hand-edited files: knot counts capped, vignette
    /// gains and radius factors held to physically plausible bands (the real
    /// Sony data sits well inside: gains ≲ 1.5×, distortion within ±6%).
    pub fn clamp(&mut self) {
        for v in [&mut self.vignette, &mut self.distortion, &mut self.ca_r, &mut self.ca_b] {
            v.truncate(32);
            // A non-finite knot survives f32::clamp and poisons the whole
            // correction spline — drop it (the base_curve knot rule).
            v.retain(|k| k.is_finite());
        }
        self.mask_warp.truncate(MASK_WARP_KNOTS);
        self.mask_warp.retain(|k| k.is_finite());
        self.linear_handle_warp.truncate(MASK_WARP_KNOTS);
        self.linear_handle_warp.retain(|k| k.is_finite());
        for g in self.vignette.iter_mut() {
            *g = g.clamp(0.25, 4.0);
        }
        for f in self.distortion.iter_mut() {
            *f = f.clamp(0.7, 1.3);
        }
        for f in self.ca_r.iter_mut().chain(self.ca_b.iter_mut()) {
            *f = f.clamp(0.98, 1.02);
        }
        // The SAME band as `distortion`, because it is the same kind of
        // quantity — a radius factor — solved from the same polynomials. The
        // widest real value this batch measured is 1.0425 (105 mm centre) and
        // the narrowest 0.9734 (24 mm centre), so ±30 % is defensive rather
        // than restrictive.
        for f in self.mask_warp.iter_mut() {
            *f = f.clamp(0.7, 1.3);
        }
        for f in self.linear_handle_warp.iter_mut() {
            *f = f.clamp(0.7, 1.3);
        }
        if self
            .mask_warp_center
            .is_some_and(|c| {
                let [x, y] = c.stored_px;
                let [w, h] = c.stored_dims;
                !x.is_finite()
                    || !y.is_finite()
                    || !w.is_finite()
                    || !h.is_finite()
                    || x < 0.0
                    || y < 0.0
                    || w <= 0.0
                    || h <= 0.0
            })
        {
            self.mask_warp_center = None;
        }
        // The tag and the data must agree, and the tag is the one that carries
        // a REASON — so it wins. A hand-edited file claiming `fisheye_refused`
        // beside warp knots is claiming two contradictory things, and
        // honouring the knots would render a warp whose provenance line says it
        // was refused. Enforced, not trusted: `clamp` is what every reader runs.
        if !self.mask_warp_src.is_solved() {
            self.mask_warp.clear();
        } else if self.mask_warp.len() < 2 {
            // A solved source with no usable spline is not solved. One knot is
            // a constant and zero is nothing; either way the reason is gone, so
            // say the honest thing rather than interpolate over a stub.
            self.mask_warp.clear();
            self.mask_warp_src = MaskWarpSource::Unparseable;
        }
        // This second map has exactly one honest state: the sidecar disabled
        // correction, so RADIAL needs identity while LINEAR still needs the
        // camera map for its two raw-frame handles. In every solved state the
        // ordinary `mask_warp` is the source; in every refusal there is no map
        // to retain. Two or more knots are required for a real spline.
        if self.mask_warp_src != MaskWarpSource::DisabledInSidecar
            || self.linear_handle_warp.len() < 2
        {
            self.linear_handle_warp.clear();
        }
    }
}

/// Normalised crop rectangle. All values in [0.0, 1.0], with (0,0) at the
/// top-left of the full frame.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

/// A single point on the master tone curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurvePoint {
    /// Input value, 0..=255.
    pub input: u8,
    /// Output value, 0..=255.
    pub output: u8,
}

/// The 8 ACR colour bands, in the order the [`Hsl`] arrays are indexed. These
/// map 1:1 to Lightroom's `crs:{Hue,Saturation,Luminance}AdjustmentRed` … keys.
pub const HSL_BANDS: [&str; 8] =
    ["Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta"];

/// Per-colour HSL adjustments — Lightroom's HSL / Color mixer. Each array is
/// indexed by [`HSL_BANDS`] (red, orange, yellow, green, aqua, blue, purple,
/// magenta). Values -100..=100, 0 = no change: `hue` rotates a band, `saturation`
/// changes its intensity, `luminance` its brightness. This is the single biggest
/// "look" control the global sliders cannot express (e.g. teal-foliage / orange-skin).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hsl {
    pub hue: [f32; 8],
    pub saturation: [f32; 8],
    pub luminance: [f32; 8],
}

impl Default for Hsl {
    fn default() -> Self {
        Self { hue: [0.0; 8], saturation: [0.0; 8], luminance: [0.0; 8] }
    }
}

impl Hsl {
    /// True when every band on every axis is neutral (lets the render + XMP skip it).
    pub fn is_neutral(&self) -> bool {
        self.hue.iter().chain(&self.saturation).chain(&self.luminance).all(|&v| v == 0.0)
    }

    /// Clamp every band to the documented -100..=100 range. Non-finite
    /// (foreign XMP text like HueAdjustmentRed="NaN") collapses to neutral 0:
    /// f32::clamp KEEPS NaN, which would poison the render arithmetic and
    /// later serialise as null into an unreadable recipe.
    pub fn clamp(&mut self) {
        for arr in [&mut self.hue, &mut self.saturation, &mut self.luminance] {
            for v in arr.iter_mut() {
                *v = if v.is_finite() { v.clamp(-100.0, 100.0) } else { 0.0 };
            }
        }
    }
}

/// Lightroom's Color Grading (the 3-wheel + global model that supersedes Split
/// Toning). Each tonal region (shadow/midtone/highlight) plus a global wheel gets
/// a `hue` (0..=360°), `sat` (0..=100), and `lum` (-100..=100). `blending` (0..=100,
/// default 50) sets how much the regions overlap; `balance` (-100..=100) shifts the
/// shadow/highlight split. Default = neutral. ACR XMP convention (verified against
/// the user's own sidecar): shadow/highlight hue+sat round-trip via the legacy
/// `crs:SplitToning*` keys, everything else via `crs:ColorGrade*`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorGrade {
    pub shadow_hue: f32,
    pub shadow_sat: f32,
    pub shadow_lum: f32,
    pub midtone_hue: f32,
    pub midtone_sat: f32,
    pub midtone_lum: f32,
    pub highlight_hue: f32,
    pub highlight_sat: f32,
    pub highlight_lum: f32,
    pub global_hue: f32,
    pub global_sat: f32,
    pub global_lum: f32,
    pub blending: f32,
    pub balance: f32,
}

impl Default for ColorGrade {
    fn default() -> Self {
        Self {
            shadow_hue: 0.0, shadow_sat: 0.0, shadow_lum: 0.0,
            midtone_hue: 0.0, midtone_sat: 0.0, midtone_lum: 0.0,
            highlight_hue: 0.0, highlight_sat: 0.0, highlight_lum: 0.0,
            global_hue: 0.0, global_sat: 0.0, global_lum: 0.0,
            blending: 50.0, // ACR default
            balance: 0.0,
        }
    }
}

impl ColorGrade {
    /// True when no wheel tints or lifts (sat + lum all zero) — render + XMP skip it.
    /// `blending`/`balance` alone do nothing without a saturated or lifted wheel.
    pub fn is_neutral(&self) -> bool {
        [
            self.shadow_sat, self.shadow_lum, self.midtone_sat, self.midtone_lum,
            self.highlight_sat, self.highlight_lum, self.global_sat, self.global_lum,
        ]
        .iter()
        .all(|&v| v == 0.0)
    }

    /// Clamp every wheel to its documented range (hue 0..360, sat 0..100, lum/balance
    /// -100..100, blending 0..100).
    pub fn clamp(&mut self) {
        // Non-finite → the field's NEUTRAL (blending's is 50): clamp and
        // rem_euclid both pass NaN straight through (see Hsl::clamp).
        let fin = |v: f32, neutral: f32| if v.is_finite() { v } else { neutral };
        // A corrupt HUE makes the whole wheel meaningless — zeroing the hue
        // alone turned "NaN hue + sat 50" into a saturated RED grade; the
        // corrupt-component-goes-INERT rule needs the paired sat zeroed too.
        for (h, sat) in [
            (&mut self.shadow_hue, &mut self.shadow_sat),
            (&mut self.midtone_hue, &mut self.midtone_sat),
            (&mut self.highlight_hue, &mut self.highlight_sat),
            (&mut self.global_hue, &mut self.global_sat),
        ] {
            if !h.is_finite() {
                *h = 0.0;
                *sat = 0.0;
            }
        }
        for h in [&mut self.shadow_hue, &mut self.midtone_hue, &mut self.highlight_hue, &mut self.global_hue] {
            *h = fin(*h, 0.0).rem_euclid(360.0);
        }
        for s in [&mut self.shadow_sat, &mut self.midtone_sat, &mut self.highlight_sat, &mut self.global_sat] {
            *s = fin(*s, 0.0).clamp(0.0, 100.0);
        }
        for l in [&mut self.shadow_lum, &mut self.midtone_lum, &mut self.highlight_lum, &mut self.global_lum] {
            *l = fin(*l, 0.0).clamp(-100.0, 100.0);
        }
        self.blending = fin(self.blending, 50.0).clamp(0.0, 100.0);
        self.balance = fin(self.balance, 0.0).clamp(-100.0, 100.0);
    }
}

/// How many point colours a recipe keeps — a bound on hand-edited and hostile
/// files (each swatch is a per-pixel membership test), not a panel limit.
pub const MAX_POINT_COLORS: usize = 16;

/// The saturation window a freshly sampled swatch materialises at Lightroom's
/// default Range (0.5), as half-widths around the sampled value: full effect
/// within ±0.18, none beyond ±0.73, clamped to 0..1. Read off both published
/// Lua dumps of a sampled swatch (Adobe community thread 14165872: SrcSat
/// 0.920754 → 0.190754 / 0.740754 / 1 / 1; MIDI2LR's `LocalPresets.lua`:
/// 0.473663 → 0 / 0.29 / 0.65 / 1). The luminance window uses the same widths
/// until the Lightroom kit's `PC-*` sidecars pin Lightroom's own rule.
pub const POINT_COLOR_FULL_HALF_WIDTH: f32 = 0.18;
pub const POINT_COLOR_NONE_HALF_WIDTH: f32 = 0.73;

/// How the Range slider scales a swatch's windows about its sampled colour:
/// a Range of `a` multiplies every half-width by `FLOOR + SPREAD·a`, which is 1
/// at Lightroom's default 0.5 — the windows both published dumps carry — and
/// never 0, so the narrowest Range still matches its own colour. PROVISIONAL
/// until the Lightroom kit's `PC-*` sidecars pin Lightroom's own rule.
pub const POINT_COLOR_RANGE_FLOOR: f32 = 0.25;
pub const POINT_COLOR_RANGE_SPREAD: f32 = 1.5;

/// One Lightroom Point Color swatch (v1.5.0): the colour it was sampled from,
/// the three shifts, and the windows that decide which pixels it moves.
///
/// Lightroom stores each swatch as ONE `rdf:li` of 19 comma-separated
/// numbers, in this struct's field order — SrcHue, SrcSat, SrcLum, HueShift,
/// SatScale, LumScale, RangeAmount, then HueRange, SatRange and LumRange as
/// {LowerNone, LowerFull, UpperFull, UpperNone} (the order JarvisArt's
/// XMP→Lua converter reads, and the field set of the Lua dumps cited on
/// [`POINT_COLOR_FULL_HALF_WIDTH`]) — and a list with no swatch as a single
/// item of nineteen `-1.000000`s (113 of the operator's 175 sidecars).
///
/// Every window is STORED, not derived: Lightroom materialises them from the
/// Range slider when the swatch is made, so the render reads the numbers and
/// never needs Lightroom's derivation. The hue window is RELATIVE — 0.5 is
/// the sampled hue — while saturation and luminance are absolute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PointColor {
    /// The sampled hue, radians 0..2π.
    pub src_hue: f32,
    /// The sampled saturation, 0..=1.
    pub src_sat: f32,
    /// The sampled luminance, 0..=1.
    pub src_lum: f32,
    /// Hue shift, -1..=1 (Lightroom's ±100 slider).
    pub hue_shift: f32,
    /// Saturation shift, -1..=1.
    pub sat_scale: f32,
    /// Luminance shift, -1..=1.
    pub lum_scale: f32,
    /// The Range slider, 0..=1 (Lightroom's default 0.5).
    pub range_amount: f32,
    /// {lower none, lower full, upper full, upper none}, relative (0.5 = src).
    pub hue_range: [f32; 4],
    /// {lower none, lower full, upper full, upper none}, absolute saturation.
    pub sat_range: [f32; 4],
    /// {lower none, lower full, upper full, upper none}, absolute luminance.
    pub lum_range: [f32; 4],
}

impl Default for PointColor {
    fn default() -> Self {
        Self {
            src_hue: 0.0,
            src_sat: 0.0,
            src_lum: 0.0,
            hue_shift: 0.0,
            sat_scale: 0.0,
            lum_scale: 0.0,
            range_amount: 0.5,
            hue_range: [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0],
            sat_range: [0.0, 0.0, 1.0, 1.0],
            lum_range: [0.0, 0.0, 1.0, 1.0],
        }
    }
}

impl PointColor {
    /// A swatch sampled from a colour (hue in radians, saturation and
    /// luminance 0..1) with Lightroom's default windows — see
    /// [`POINT_COLOR_FULL_HALF_WIDTH`] — and no shift yet.
    pub fn sampled(hue: f32, sat: f32, lum: f32) -> Self {
        let window = |v: f32| {
            let at = |d: f32| (v + d).clamp(0.0, 1.0);
            [
                at(-POINT_COLOR_NONE_HALF_WIDTH),
                at(-POINT_COLOR_FULL_HALF_WIDTH),
                at(POINT_COLOR_FULL_HALF_WIDTH),
                at(POINT_COLOR_NONE_HALF_WIDTH),
            ]
        };
        let mut p = Self {
            src_hue: hue,
            src_sat: sat,
            src_lum: lum,
            sat_range: window(sat),
            lum_range: window(lum),
            ..Self::default()
        };
        p.clamp();
        p
    }

    /// Does this swatch move anything? A sampled swatch with every shift at 0
    /// is a real Lightroom state that renders nothing.
    pub fn is_neutral(&self) -> bool {
        self.hue_shift == 0.0 && self.sat_scale == 0.0 && self.lum_scale == 0.0
    }

    /// Move the Range slider (0..=1) and rebuild the three windows from it, as
    /// Lightroom materialises them: the render reads the stored windows and
    /// never `range_amount`, so a Range that only stored its number would be a
    /// slider that moves no pixel. Each window keeps the sampled colour at its
    /// centre and scales its half-widths by [`POINT_COLOR_RANGE_SPREAD`]; the
    /// relative hue window's are 1/6 (full) and 1/2 (none) at the default.
    pub fn set_range(&mut self, amount: f32) {
        self.range_amount = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.5 };
        let k = POINT_COLOR_RANGE_FLOOR + POINT_COLOR_RANGE_SPREAD * self.range_amount;
        let window = |centre: f32, full: f32, none: f32| {
            [centre - none * k, centre - full * k, centre + full * k, centre + none * k]
        };
        self.hue_range = window(0.5, 1.0 / 6.0, 0.5);
        self.sat_range = window(self.src_sat, POINT_COLOR_FULL_HALF_WIDTH, POINT_COLOR_NONE_HALF_WIDTH);
        self.lum_range = window(self.src_lum, POINT_COLOR_FULL_HALF_WIDTH, POINT_COLOR_NONE_HALF_WIDTH);
        self.clamp();
    }

    /// The 19 numbers of one `crs:PointColors` item, in Lightroom's order.
    pub(crate) fn to_numbers(&self) -> [f32; 19] {
        let mut n = [0.0; 19];
        n[..7].copy_from_slice(&[
            self.src_hue,
            self.src_sat,
            self.src_lum,
            self.hue_shift,
            self.sat_scale,
            self.lum_scale,
            self.range_amount,
        ]);
        n[7..11].copy_from_slice(&self.hue_range);
        n[11..15].copy_from_slice(&self.sat_range);
        n[15..].copy_from_slice(&self.lum_range);
        n
    }

    /// One item read back: `Ok(None)` for Lightroom's no-swatch placeholder
    /// (nineteen `-1`s), `Err` for anything that is not a swatch this model
    /// can hold — the wrong count, a non-finite or out-of-domain number, a
    /// window whose edges run backwards. Refused, never repaired: a record
    /// that fails here is a layout this reader does not understand, and a
    /// repaired guess would move colours nobody picked.
    pub(crate) fn from_numbers(n: &[f32]) -> Result<Option<Self>, ()> {
        if n.len() != 19 {
            return Err(());
        }
        if n.iter().all(|v| *v == -1.0) {
            return Ok(None);
        }
        let unit = |v: f32| (0.0..=1.0).contains(&v);
        let signed = |v: f32| (-1.0..=1.0).contains(&v);
        let window = |w: &[f32]| w.iter().all(|v| unit(*v)) && w.windows(2).all(|p| p[0] <= p[1]);
        let hue_ok = (0.0..=std::f32::consts::TAU + 1e-3).contains(&n[0]);
        if n.iter().any(|v| !v.is_finite())
            || !hue_ok
            || !unit(n[1])
            || !unit(n[2])
            || !n[3..6].iter().all(|v| signed(*v))
            || !unit(n[6])
            || !window(&n[7..11])
            || !window(&n[11..15])
            || !window(&n[15..19])
        {
            return Err(());
        }
        let four = |at: usize| [n[at], n[at + 1], n[at + 2], n[at + 3]];
        Ok(Some(Self {
            src_hue: n[0],
            src_sat: n[1],
            src_lum: n[2],
            hue_shift: n[3],
            sat_scale: n[4],
            lum_scale: n[5],
            range_amount: n[6],
            hue_range: four(7),
            sat_range: four(11),
            lum_range: four(15),
        }))
    }

    /// Hold a swatch to its domains: a non-finite number returns to the
    /// default's, the hue wraps into 0..2π, shifts and windows saturate, and
    /// a window's edges are put back in order.
    pub fn clamp(&mut self) {
        let dflt = Self::default();
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.src_hue = fin(self.src_hue, 0.0).rem_euclid(std::f32::consts::TAU);
        self.src_sat = fin(self.src_sat, 0.0).clamp(0.0, 1.0);
        self.src_lum = fin(self.src_lum, 0.0).clamp(0.0, 1.0);
        for v in [&mut self.hue_shift, &mut self.sat_scale, &mut self.lum_scale] {
            *v = fin(*v, 0.0).clamp(-1.0, 1.0);
        }
        self.range_amount = fin(self.range_amount, dflt.range_amount).clamp(0.0, 1.0);
        for (w, d) in [
            (&mut self.hue_range, dflt.hue_range),
            (&mut self.sat_range, dflt.sat_range),
            (&mut self.lum_range, dflt.lum_range),
        ] {
            for (v, dv) in w.iter_mut().zip(d) {
                *v = fin(*v, dv).clamp(0.0, 1.0);
            }
            w.sort_by(f32::total_cmp);
        }
    }
}

/// A local (masked) adjustment: *where* it applies (`mask`) plus the slider
/// deltas to apply inside that mask. Sliders use the SAME UI scale as the global
/// [`EditRecipe`] fields; the XMP writer converts to ACR's local scale (exposure
/// stops/4, other sliders /100). `temperature` here is a *relative* shift, not
/// Kelvin (maps to `crs:LocalTemperature`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LocalAdjustment {
    pub mask: MaskGeometry,
    /// Extra shapes composed onto `mask` in order (Add / Subtract /
    /// Intersect — Lightroom's mask-component grammar). Empty = the base
    /// geometry alone (v1-compatible). Export grammar: see [`MaskComponent`].
    pub components: Vec<MaskComponent>,
    /// Lightroom's per-mask eye toggle: `false` mutes the adjustment without
    /// touching its tuned Amount (dragging Amount to 0 as a mute DESTROYED
    /// the tuned value). Disabled masks render nothing, show no coverage,
    /// don't gate exports, and are SKIPPED by the XMP projection (the recipe
    /// keeps them; a re-enable is one click — same lossy-projection stance
    /// as Bitmap masks).
    pub enabled: bool,
    /// Optional Range Mask refinement intersected with the COMBINED coverage
    /// (final weight = combined geometry × range). `None` = pure geometry
    /// (v1-compatible).
    pub range: Option<RangeMask>,
    /// Human label → `crs:CorrectionName` / `crs:MaskName`.
    pub name: String,
    /// Master opacity 0.0..=1.0 → `crs:CorrectionAmount` + `crs:MaskValue`.
    pub amount: f32,
    /// Invert the mask region → `crs:MaskInverted`.
    pub inverted: bool,
    pub exposure_ev: f32,
    pub contrast: f32,
    pub highlights: f32,
    pub shadows: f32,
    pub whites: f32,
    pub blacks: f32,
    /// Local midtone local-contrast → `crs:LocalClarity2012`. ENGINE-RENDERED
    /// since R22 (before that these three were carried and exported but never
    /// drawn, so a mask that moved only them did nothing in-app — user feedback
    /// #15a/#10B); see `render::apply_masks` for the pass order.
    pub clarity: f32,
    /// Local atmospheric-veil removal → `crs:LocalDehaze`. Shares the global
    /// haze model (`render::dehaze_airlight` + `dehaze_px`); the airlight is
    /// estimated once per frame, so mask order cannot change it.
    pub dehaze: f32,
    /// Local fine-detail contrast → `crs:LocalTexture`. Rendered as a
    /// small-radius unsharp mask; unlike clarity there is no global Texture
    /// stage to align its radius with, so the scaling is our own calibration
    /// (Adobe's model is not published) — Lightroom re-renders from the raw
    /// slider value in the XMP.
    pub texture: f32,
    /// Local capture sharpening → `crs:LocalSharpness`, -100..=100 and SIGNED
    /// (ACR's local Sharpness band): positive sharpens, negative SOFTENS. The
    /// radius is the global `sharpening` stage's own σ model, so "Sharpness 40"
    /// means the same structure globally and inside a mask. Added in R23-1b —
    /// the writer emitted a literal `"0"` for this key from the first sidecar
    /// on, so a photographer could soften a background in Lightroom but never
    /// in this app.
    pub sharpness: f32,
    pub saturation: f32,
    /// Local HUE ROTATION → `crs:LocalHue` (the file holds degrees ÷ 180),
    /// ±[`LOCAL_HUE_MAX_DEG`]; ±100 maps to ±30° (the global 8-band mixer's
    /// hue axis, `render::apply_hsl`).
    /// Rotates every hue inside the mask instead of one band across the whole
    /// frame — the one shape of colour move a global mixer cannot make. Added
    /// in R23-1b beside `sharpness`, for the same reason.
    pub hue: f32,
    /// Relative warm/cool shift (NOT Kelvin) → `crs:LocalTemperature`.
    pub temperature: f32,
    pub tint: f32,
    /// Lightroom's mask "Color" swatch, its hue → `crs:LocalToningHue`, 0..360°
    /// stored as written (the file carries degrees, unscaled). A colour TINT,
    /// not a rotation: where `hue` turns every colour by the same angle, this
    /// shifts every pixel's chroma the SAME way, toward this one hue — so a
    /// neutral cloud and a chromatic sky move together, which no rotation or
    /// channel gain can do with one setting. Rendered by `render::apply_masks`
    /// (see `render::tint_px` for the model, fitted to Lightroom's own
    /// exports). Inert while `toning_amount` is 0.
    pub toning_hue: f32,
    /// The swatch's strength → `crs:LocalToningSaturation` × 100, 0..=100 —
    /// the same ×100 convention as the mask's other 0..1 keys. 0 = no tint,
    /// the serde default, so every recipe written before the tint existed
    /// loads and renders unchanged.
    pub toning_amount: f32,
    /// Local luminance noise reduction, 0..=100 → `crs:LocalLuminanceNoise`.
    /// For "this region is noisy" requests; smooths only inside the mask.
    pub noise_reduction: f32,
    /// This mask's own master point curve → `crs:MainCurve`. Same `{input,
    /// output}` 0..=255 points as the global [`EditRecipe::tone_curve`], and
    /// composed through the very same LUT builder (`render::build_tone_lut`),
    /// so "a curve" means one thing everywhere in this engine.
    ///
    /// **The key is the BARE name.** Lightroom writes the global curves as
    /// `crs:ToneCurvePV2012{,Red,Green,Blue}` but the local ones as
    /// `crs:MainCurve` / `RedCurve` / `GreenCurve` / `BlueCurve` — no
    /// `PV2012` suffix — as child elements of the Correction, between
    /// `crs:LocalCurveRefineSaturation` and `crs:CorrectionMasks`. Their point
    /// payload is spelled `x,y` with NO SPACE after the comma, where the
    /// global curves' is `x, y` WITH one; `xmp::local_curve_elem` is a
    /// separate writer for exactly that reason.
    ///
    /// The four are SPARSE and INDEPENDENT — a real sidecar carries Red and
    /// Green with no Main and no Blue — so this is four plain vectors, not one
    /// four-curve struct. Empty = identity, and the two ways a vector can
    /// arrive empty (absent from the JSON, or written as `[]`) MEAN THE SAME
    /// THING: unlike `coord_era` / `Radial::midpoint`, whose type default is a
    /// legitimate value and therefore needs a field-level `#[serde(default =
    /// …)]`, an empty curve is "no curve" from either source. Do not copy that
    /// pattern here.
    pub main_curve: Vec<CurvePoint>,
    /// This mask's RED channel curve → `crs:RedCurve`. See [`main_curve`].
    ///
    /// [`main_curve`]: LocalAdjustment::main_curve
    pub red_curve: Vec<CurvePoint>,
    /// This mask's GREEN channel curve → `crs:GreenCurve`. See [`main_curve`].
    ///
    /// [`main_curve`]: LocalAdjustment::main_curve
    pub green_curve: Vec<CurvePoint>,
    /// This mask's BLUE channel curve → `crs:BlueCurve`. See [`main_curve`].
    ///
    /// [`main_curve`]: LocalAdjustment::main_curve
    pub blue_curve: Vec<CurvePoint>,
    /// Per-channel LINEAR-light gains for zone RECOLOURING, `1.0` = neutral.
    /// Produced by the zoned reverse-fit (fit_zoned.rs): a palette-transplant
    /// target (pale-blue sky → gold) demands channel ratios far beyond what
    /// any white-balance parametrisation can express (measured: blue→gold
    /// needs r/b ≈ 5.3×; the full 2000–40000 K blackbody range caps at
    /// ≈ 1.9×), so the fit writes the exact gains instead. ENGINE-ONLY: no
    /// classic-ACR counterpart exists, so the XMP writer cannot carry it —
    /// the fit may attach it to a native shape as well. The writer keeps the
    /// shape and names the separate recolour loss. Composes
    /// multiplicatively with the temperature/tint gains in the engine.
    pub color_gains: Option<[f32; 3]>,
    /// Which semantic zone produced this mask, a STABLE identity independent of
    /// the free-text `name` (which the GUI now translates, so it can no longer
    /// be an equality key). `Custom` for every user/AI mask; the zoned
    /// reverse-fit tags its masks `ZoneSky`/`ZoneLand` (fit_zoned.rs). XMP
    /// carries it as namespaced AutoShade editing intent; Lightroom's own
    /// mask grammar carries the coverage independently of this role.
    pub role: MaskRole,
}

impl Default for LocalAdjustment {
    fn default() -> Self {
        Self {
            mask: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.5, full_y: 0.5 },
            components: Vec::new(),
            enabled: true,
            range: None,
            name: String::new(),
            amount: 1.0,
            inverted: false,
            exposure_ev: 0.0,
            contrast: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            clarity: 0.0,
            dehaze: 0.0,
            texture: 0.0,
            sharpness: 0.0,
            saturation: 0.0,
            hue: 0.0,
            temperature: 0.0,
            tint: 0.0,
            toning_hue: 0.0,
            toning_amount: 0.0,
            noise_reduction: 0.0,
            main_curve: Vec::new(),
            red_curve: Vec::new(),
            green_curve: Vec::new(),
            blue_curve: Vec::new(),
            color_gains: None,
            role: MaskRole::Custom,
        }
    }
}

/// Which semantic zone a local adjustment came from — a STABLE mask identity
/// that survives UI translation and user renaming (the display `name` is free
/// text and, since i18n, no longer a reliable key). Set by the zoned
/// reverse-fit (fit_zoned.rs) and, since v1.14.0, by the Portraits mode
/// (`crate::face`); `Custom` for every other user-placed or AI-segmented
/// mask. Serialised in `recipe.json` as `"custom"` / `"zone_sky"` /
/// `"zone_land"` / `"face"` / `"eyes"` / `"skin"` / `"background"`; carried as
/// namespaced AutoShade intent in XMP.
///
/// **No `#[serde(other)]`, and the four portrait roles are therefore a forward
/// break.** A build from before v1.14.0 meets `"role":"face"` as an unknown
/// variant and refuses the whole recipe by name (serde's "unknown variant"),
/// the same class-① posture `deny_unknown_fields` gives every container here:
/// an older reader that silently demoted the role to `Custom` would also
/// write it back as `Custom`, quietly editing the photographer's file. A
/// catch-all variant could only help readers built AFTER it was added, so it
/// buys nothing for the builds that exist; it is not added. Recipes whose
/// masks all carry the three older roles are unaffected. XMP is unaffected
/// either way: an older reader takes any role tag it does not know as
/// `Custom` (`xmp::read_corrections`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskRole {
    /// A user-placed or AI-segmented mask — identity carried by `name` only.
    #[default]
    Custom,
    /// Sky zone from the zoned reverse-fit (rides the upright sky raster).
    ZoneSky,
    /// Land zone from the zoned reverse-fit (rides the INVERTED sky raster).
    ZoneLand,
    /// Every face the face finder outlined, unioned and feathered
    /// (`crate::face::face_mask`).
    Face,
    /// Both eyes of every face found, unioned and feathered
    /// (`crate::face::eyes_mask`).
    Eyes,
    /// The face mask narrowed to skin-coloured pixels by a colour range seeded
    /// from the face itself (`crate::face::skin_adjustment`).
    Skin,
    /// Everything but the subject: the subject matte, inverted
    /// (`crate::face::background_adjustment`).
    Background,
}

impl MaskRole {
    /// ASCII tag for the fit's rationale text and the XMP intent attribute
    /// (`"custom"` / `"sky"` / `"land"` / `"face"` / `"eyes"` / `"skin"` /
    /// `"background"`).
    pub fn tag(self) -> &'static str {
        match self {
            MaskRole::Custom => "custom",
            MaskRole::ZoneSky => "sky",
            MaskRole::ZoneLand => "land",
            MaskRole::Face => "face",
            MaskRole::Eyes => "eyes",
            MaskRole::Skin => "skin",
            MaskRole::Background => "background",
        }
    }

    /// The role a [`tag`](Self::tag) names, or `None` for a tag this build
    /// does not know — the reading half of the XMP intent attribute.
    pub fn from_tag(tag: &str) -> Option<Self> {
        [
            MaskRole::Custom,
            MaskRole::ZoneSky,
            MaskRole::ZoneLand,
            MaskRole::Face,
            MaskRole::Eyes,
            MaskRole::Skin,
            MaskRole::Background,
        ]
        .into_iter()
        .find(|r| r.tag() == tag)
    }

    /// Canonical English display name for a zone or portrait mask, or `None` for `Custom`
    /// (whose label is the user's own `name`). The GUI runs this through i18n
    /// so the mask row localises alongside the rest of the UI.
    pub fn en_name(self) -> Option<&'static str> {
        match self {
            MaskRole::Custom => None,
            MaskRole::ZoneSky => Some("Sky (reverse-fit)"),
            MaskRole::ZoneLand => Some("Land (reverse-fit)"),
            MaskRole::Face => Some("Face"),
            MaskRole::Eyes => Some("Eyes"),
            MaskRole::Skin => Some("Skin"),
            MaskRole::Background => Some("Background"),
        }
    }

    /// Did the zoned reverse-fit place this mask, and does it therefore OWN
    /// the alpha it rides?
    ///
    /// The distinction is not cosmetic. The sky and land zones ride out as
    /// Lightroom's own Select Sky ([`MaskGeometry::select_sky`]), so they are
    /// `AiMask`s — and for every OTHER `AiMask` in this app the `raster` field
    /// is a CACHE of a re-derivation, keyed by (photo, subtype, reference
    /// point, frame), which the code is free to drop, renew or recompute. A
    /// zone's raster is not: it is the alpha the fit MEASURED its exposure,
    /// gains and saturation against, claimed under a unique name beside the
    /// develop, and replacing it silently invalidates every number the fit
    /// disclosed. So the sites that treat an AI alpha as disposable
    /// (`segment::resolve_ai_masks`, `render::orient_recipe_coords`,
    /// `LocalAdjustment::turnable_raster_paths_mut`) ask this first.
    pub fn is_zone(self) -> bool {
        matches!(self, MaskRole::ZoneSky | MaskRole::ZoneLand)
    }
}

impl MaskGeometry {
    /// Lightroom's own **Select Sky** as this app produces it: a `Mask/Image`
    /// component with `crs:MaskSubType="2"`, prompted at `(ref_x, ref_y)` and
    /// rendered here from `raster`.
    ///
    /// ONE constructor, because the field list is a WIRE FORMAT: `subtype`
    /// decides what Lightroom rebuilds, `blend_mode`/`value` are the
    /// subtract-pair invariants that must stay `(0, 1)` for a union, and
    /// `mask_version` is Lightroom's own schema stamp (`"1"` on 105/105
    /// measured instances). A second hand-written copy of that list is exactly
    /// how one producer would start emitting a component Lightroom reads as
    /// something else.
    ///
    /// `provenance` and `gesture` are EMPTY on purpose. Both are the
    /// photographer's own material carried back verbatim from a sidecar this
    /// app read (Adobe's digests, model version, a refinement stroke); minting
    /// either here would assert a provenance nothing produced.
    ///
    /// **`name` is `"Sky 1"`** — Lightroom's own spelling for a Select Sky
    /// component, one of the measured `crs:MaskName` values. It names the
    /// COMPONENT inside the correction, not the row this app shows: a zone
    /// mask's display name comes from [`MaskRole::en_name`] and is localised.
    ///
    /// **`inverted` is the COMPONENT's own bit** (`crs:MaskInverted` as a
    /// Lightroom sidecar spells it). It composes with the correction's own
    /// flag through [`LocalAdjustment::net_inverted`] and is applied exactly
    /// once, in both the render and the sidecar. The zoned reverse-fit passes
    /// `false` here and keeps its inversion on the adjustment — see the call
    /// site — while an imported Lightroom mask arrives with the bit HERE,
    /// because that is where its file put it.
    pub fn select_sky(ref_x: f32, ref_y: f32, inverted: bool, raster: String) -> Self {
        MaskGeometry::AiMask {
            name: "Sky 1".to_string(),
            subtype: 2,
            ref_x,
            ref_y,
            blend_mode: 0,
            value: 1.0,
            inverted,
            mask_version: 1,
            provenance: Vec::new(),
            gesture: Vec::new(),
            raster: Some(raster),
        }
    }

    /// The inversion bit the GEOMETRY itself carries, or `false` for one that
    /// has none.
    ///
    /// THREE carriers, and they are the same fact in three spellings: a
    /// radial's `flipped`, a brush group's `inverted`, an AI mask's
    /// `inverted`. A `Linear` and a `Bitmap` have no bit of their own — their
    /// correction inverts through [`LocalAdjustment::inverted`] alone.
    ///
    /// Read it here and nowhere else. Every consumer that ever asked a
    /// geometry "are you inverted" by hand missed at least one carrier: the
    /// renderer's AI arm honoured no bit at all (so an imported Lightroom
    /// 「Select Sky, inverted」 rendered the sky it was told to exclude), and
    /// the sidecar writer's brush and AI arms wrote this bit ALONE, silently
    /// dropping the correction's own. Both were the same defect —
    /// hand-spelled inversion — and [`LocalAdjustment::net_inverted`] is where
    /// the two bits are now allowed to meet.
    pub fn own_inverted(&self) -> bool {
        match self {
            MaskGeometry::Radial { flipped, .. } => *flipped,
            MaskGeometry::Brush { inverted, .. } => *inverted,
            MaskGeometry::AiMask { inverted, .. } => *inverted,
            MaskGeometry::Linear { .. } | MaskGeometry::Shaped { .. } | MaskGeometry::Bitmap { .. } => false,
        }
    }
}

/// Where a local adjustment applies. Coordinates are normalised to the frame and
/// MAY fall outside [0,1] for gradients (matching ACR's geometry).
//
// FORWARD COMPATIBILITY, class 3 (R27 L-17, closing the registration at
// docs/ROADMAP.md's v0.31.0 entry). The old comment here read "serde does not
// support `deny_unknown_fields` on internally-tagged enums" — that is wrong,
// and the cost of believing it was the only SILENT forward break this project
// ever shipped: v0.31.0 added `midpoint` + `mask_version` to `Radial`, and a
// v0.30 binary reading such a recipe dropped both without a word and wrote the
// truncated geometry back, quietly editing the user's file. Classes 1 and 2
// (a new top-level `EditRecipe` field, a new `LocalAdjustment` curve) are LOUD
// because those containers deny; this one was not, purely for want of the
// attribute.
//
// The attribute placed on the CONTAINER covers every variant and exempts the
// tag itself (probed against serde 1.0.228 + serde_json 1.0.150 before landing;
// pinned by `an_unknown_radial_field_is_a_loud_refusal`). Consequence, stated
// so it is a decision rather than a surprise: a build that meets a recipe from
// a NEWER build now REFUSES it by name instead of silently truncating it —
// same posture as `EditRecipe`'s own `deny_unknown_fields`, and the reason
// downgrades are not supported. The AI's own schema is a strict SUBSET of
// these fields (`advisor::catalogue::mask_geometry_schema` /
// `range_mask_schema`, both `additionalProperties: false`), so no proposal
// shape changes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaskGeometry {
    /// Linear gradient — the zero→full vector sets direction + falloff width.
    /// Maps to ACR `What="Mask/Gradient"`.
    Linear { zero_x: f32, zero_y: f32, full_x: f32, full_y: f32 },
    /// A linear gradient whose falloff is a MEASURED curve rather than
    /// Lightroom's law: the zero→full handles set the axis exactly as
    /// [`MaskGeometry::Linear`] does, and `falloff` is the coverage at evenly
    /// spaced points of the handle parameter `t` in 0..=1 (first entry at the
    /// Zero handle, last at Full), interpolated linearly between them and
    /// clamped to 0..=1 ([`crate::render::shaped_ramp_coverage`]).
    ///
    /// Written by the zoned reverse-fit for a zone whose residual follows the
    /// target's own vertical profile (`fit_zoned::subzones`): on a night sky
    /// the glow at the horizon rises like an exponential over the lowest
    /// sixth of the sky, and no S-shaped ramp fits that without a visible
    /// band where the two shapes part (2026-10-20). Exported to XMP as a
    /// plain `Mask/Gradient` on the same handles — the curve itself cannot
    /// be written, so Lightroom renders its own law as the approximation.
    Shaped { zero_x: f32, zero_y: f32, full_x: f32, full_y: f32, falloff: Vec<f32> },
    /// Radial/elliptical gradient. Maps to ACR `What="Mask/CircularGradient"`.
    /// `feather` IS engine-rendered (clamped to 0..1). `roundness` is **carried
    /// only** — persisted, XMP round-tripped and accepted from the AI, but NOT
    /// rendered: the engine draws a pure ellipse. Since R29 B7 (2026-08-20)
    /// that is Lightroom's MEASURED behaviour at `Roundness = +100` with
    /// `Feather = 0` — a hand-authored probe rendered pixel-identically to
    /// its Roundness=0 reference — not just a refusal to guess; see
    /// `mask_weight` in render.rs. Negatives and the Roundness×Feather cross
    /// term are still unmeasured, so the value stays carried verbatim.
    ///
    /// Its DOMAIN is no longer a guess either: `crs:Roundness` is Lightroom's
    /// ±100 integer slider (all 24 radials in the harvested real-sidecar corpus
    /// write a bare signed integer, every one of them at the default `0`), not
    /// the 0..1 aspect ratio this field was once clamped to. v0.31.1 widened
    /// the clamp and the importer's gate to ±100 so a user who moved that
    /// slider keeps their mask (docs/V2_PLAN.md §7 item 11).
    ///
    /// `angle` (degrees, ENGINE convention: CLOCKWISE on screen about the bbox
    /// centre, 0 = axis-aligned — normalised frame coords are y-DOWN, so the
    /// very same rotation reads counter-clockwise in a y-UP maths frame, which
    /// is the reading this line used to print while claiming the screen one.
    /// Measured on the released binary's own output, not derived: E1-verdict
    /// §4a, R25 P9) IS rendered and GUI-editable, and is still OUR OWN field
    /// rather than `crs:Angle` — but v0.32.0 mapped the two onto each other in
    /// both directions. They are not the same number: `crs:Angle` is a tilt in
    /// PIXEL space and this one is a rotation applied in the NORMALISED frame,
    /// which differ by up to 11.2° of rendered tilt over the ±44° range real
    /// sidecars use. The fold between them is `xmp::lr_to_engine` /
    /// `xmp::engine_to_lr`, and it needs the frame's aspect ratio; a document
    /// that declares none still exports the UNROTATED ellipse and discloses the
    /// angle it could not write.
    ///
    /// The bbox is likewise no longer the sidecar's bbox: Lightroom's stored
    /// corners are the ROTATED corners of the ellipse's box, and the whole
    /// normalised frame is scaled by `xmp::LR_MASK_FRAME_SCALE` between the two
    /// conventions. What is stored HERE is the engine's own ellipse; the
    /// projection to and from Lightroom's is `xmp.rs`'s alone.
    ///
    /// `midpoint` and `mask_version` are the two attributes Lightroom writes on
    /// EVERY radial that this engine could not even read until R25 P5 — see
    /// their own docs. Both are CARRIED: read from the sidecar, kept in
    /// `recipe.json`, written back unchanged, and never consulted by
    /// `mask_weight`.
    Radial {
        top: f32,
        left: f32,
        bottom: f32,
        right: f32,
        feather: f32,
        roundness: f32,
        flipped: bool,
        #[serde(default)]
        angle: f32,
        /// Lightroom's `crs:Midpoint` (0..100, ACR default 50): where along the
        /// radial falloff the half-strength point sits — a second shaping knob
        /// beside `feather`. CARRIED ONLY: the engine's falloff is `feather`'s
        /// alone, so honouring this would reshape every imported mask on a
        /// guess (the roundness rule). Round-trips so a sidecar we rewrite
        /// still says what Lightroom said.
        ///
        /// FRAME-INDEPENDENT: a ratio along the radial axis, not a coordinate,
        /// so the `coord_era` migration (`render::orient_recipe_coords`) does
        /// not touch it — the same reason a curve point is not migrated.
        ///
        /// KNOWN BOUNDARY: this field and `mask_version` are deliberately NOT
        /// in the AI's mask-geometry schema (`advisor::catalogue`) — a model
        /// cannot know a value it never saw, and a required property is a
        /// property it would invent. So a REFINE returns the geometry with
        /// both at their serde defaults.
        ///
        /// R25 left that as a permanent cost; R27 (L-09, user ruling
        /// 2026-08-19 =「修」) closed it WITHOUT the widening R25 feared:
        /// `pipeline::carry_radial_carried_attributes` re-attaches the pair
        /// from the refine base in a pass of its own, so `schema_loses` — the
        /// state-bearing predicate that decides which masks take the
        /// wholesale-revert path — is untouched. The schema stays silent about
        /// both fields, which is still the right shape: the model is not asked
        /// for a number it cannot know, and the pipeline puts the
        /// photographer's own back. The GUI's ↻ Redraw preserved them all
        /// along (canvas.rs).
        #[serde(default = "radial_midpoint_centre")]
        midpoint: f32,
        /// Lightroom's `crs:Version` on the mask component (2 in every
        /// reference sidecar) — the geometry SCHEMA version Lightroom stamps,
        /// not our recipe's. CARRIED verbatim and never interpreted: dropping
        /// it made a rewritten sidecar claim a version Lightroom had not
        /// written, and inventing a meaning for it would be worse. Not
        /// clamped for the same reason — a value we do not read has no range
        /// to enforce.
        #[serde(default = "lightroom_mask_version")]
        mask_version: u32,
    },
    /// Free-form raster mask — the carrier for AI subject/sky segmentation and
    /// any painted selection. `path` names an 8-bit image whose LUMINANCE is
    /// the weight (white = full effect), sampled bilinearly in normalised
    /// coordinates so one file drives both the small preview and the full-res
    /// export. Lives in the ORIGINAL frame like the parametric geometries.
    /// Classic ACR XMP cannot express raster masks — the sidecar writer skips
    /// these (parametric masks still round-trip; positioning tradeoff like the
    /// §B retouch-master limitation). A missing/unreadable file renders the
    /// mask inert (weight 0) with a stderr warning rather than failing.
    Bitmap { path: String },
    /// Lightroom's BRUSH group — one `crs:What="Mask/Aggregate"` component and
    /// the `Mask/Paint` strokes it contains (R27 Batch-4, L-08).
    ///
    /// **RENDERED since R29 Batch-6b, from a measured model** — and until then
    /// it was carried and NOT rendered, which is a hard behaviour change for
    /// every recipe that holds one. `render::mask_weight` answered a literal
    /// `0.0` for this geometry from R27 Batch-4 until that batch, because the
    /// one input a rasteriser needs is the alpha kernel and the sidecar does
    /// not contain it (see [`BrushStroke::dabs`]). R29 Batch-6 MEASURED the
    /// kernel instead of guessing at it — 29 controlled Lightroom exports, a
    /// nine-rung hardness ladder and a 5 × 2 × 2 flow × radius × hardness grid
    /// of drags — so `render::brush_raster` now stamps the dab stream into an
    /// alpha and the geometry draws. **Nothing in this file changed for it:**
    /// the raster is a render-time artefact, so there is no new field, no
    /// schema bump and no `schema_era` gate. Both disclosures moved with the
    /// behaviour and now name the approximation rather than the absence
    /// (`MaskImportReason::BrushRendered` / `MaskLossReason::BrushRendered`).
    ///
    /// **The encoding, as re-derived** (the 2026-08 census snapshot: 177 sidecars of
    /// the user's own library, real XML parser, 0 parse failures; 42
    /// `Mask/Aggregate` and 398 `Mask/Paint` instances):
    ///
    ///  * an Aggregate is a ONE-LEVEL group. Its children are `Mask/Paint`,
    ///    398/398 — never a Gradient, a Radial, a RangeMask, an Image or
    ///    another Aggregate — and the maximum component nesting depth in the
    ///    whole library is exactly 2. `Mask/Paint` is NEVER a top-level
    ///    correction component (398/398 of the in-correction Paints are
    ///    children of an Aggregate).
    ///  * the composition mode relative to the rest of the correction sits on
    ///    the GROUP: `(blend_mode, value, inverted)` measured as `(1, 0, false)`
    ///    ×22, `(0, 1, false)` ×16, `(1, 0, true)` ×1 — Lightroom's subtract
    ///    pair and its plain add, the same encoding v0.31.1 taught the reader
    ///    for parametric components.
    ///  * the strokes INSIDE only ever union with each other: `MaskBlendMode`
    ///    is `"0"` on 398/398 Paints and `MaskInverted` `"false"` on 398/398.
    ///    Anything else is someone else's writer and is refused, not guessed
    ///    (the roundness rule).
    ///
    /// The TOTALS above (177 sidecars, 42 Aggregates, 398 Paints, 398
    /// in-correction Paints) are corpus measurements, not schema fields; the
    /// invariants they describe are enforced independently by the parser.
    ///
    /// **NEWER LIGHTROOM MAY STORE THESE STROKES IN A COMPANION `.acr`.**
    /// Lightroom 18.4 can replace text `Mask/Paint` children with a
    /// `MaskBrushTable` reference on the Aggregate. Before R30 B2, the two
    /// rewritten specimens failed loudly as `OutOfModel` (one owning
    /// correction in `P06`, two in `P04`); their table strokes did
    /// not render. The path-aware XMP reader now imports the adjudicated ACR
    /// grammar under strict bounds and reports one of nine named refusal
    /// classes when any unestablished form is encountered. Within an
    /// Aggregate, table records precede its surviving text Paint children in
    /// document order; gesture Paints remain with their gesture owner.
    ///
    /// The binary table is deliberately not recipe state. An unchanged
    /// path-aware merge reparses the base sidecar, recognizes that the recipe
    /// mask is unchanged, and preserves the original Aggregate block verbatim.
    /// Its table attributes and residual Paints therefore ride through without
    /// synthesizing a second text representation of the table-derived strokes.
    ///
    /// All four numbers ride out into the sidecar exactly as they rode in, so
    /// a brush mask this app imports and republishes still says what Lightroom
    /// said. `blend_mode` is therefore stored HERE as well as being projected
    /// onto the owning [`MaskComponent`]'s [`MaskCombine`] on import: the
    /// component's mode is what the renderer composes with, and this is what
    /// the WRITER re-emits. They are two spellings of one fact and the import
    /// is the only place that maps between them.
    Brush {
        /// `crs:MaskName` on the Aggregate — always "Brush *n*" in some UI
        /// language (`画笔 1` ×23, `Brush 1` ×7, `画笔 2` ×6, …). Carried so a
        /// republished sidecar keeps the photographer's own label.
        name: String,
        /// `crs:MaskBlendMode` on the Aggregate, VERBATIM. 0 = union onto the
        /// correction's coverage, 1 = subtract (paired with `value` 0).
        blend_mode: u32,
        /// `crs:MaskValue` on the Aggregate, VERBATIM — the other half of the
        /// subtract pair, never a strength (see `component_import_reasons` in
        /// xmp.rs for why reading it as one neutralises real masks).
        value: f32,
        /// `crs:MaskInverted` on the Aggregate.
        inverted: bool,
        /// The group's `Mask/Paint` children, in DOCUMENT ORDER. Order is
        /// load-bearing for the eventual raster (dabs accumulate), so it is
        /// preserved rather than sorted.
        strokes: Vec<BrushStroke>,
    },
    /// Lightroom's AI mask — one `crs:What="Mask/Image"` component (R27
    /// Batch-5, L-08 Arm C). The INTENT is imported and carried; the alpha is
    /// **RECOMPUTED on this machine by our own segmenter**, and that is a
    /// different thing from an import.
    ///
    /// **Why this cannot be an import.** F2's anatomy enumerated the full
    /// attribute vocabulary over 105 real instances (the current corpus count,
    /// the wider library for this batch, 21 attribute names, one child element)
    /// and found **no raster payload and no geometry payload** — the longest
    /// attribute value anywhere on one is 55 characters. The sidecar names
    /// WHICH segmentation to run (`MaskSubType` + `MaskName` +
    /// `ReferencePoint`), WHICH model produced the original (`ModelVersion`,
    /// three digests), and the proxy frame it ran in (`FullMaskSize`,
    /// `WholeImageArea`, `Origin`). Nothing else. Adobe recomputes the alpha on
    /// apply; so must we, with a different model.
    ///
    /// **So the disclosure is mandatory and it runs in both directions.** What
    /// this engine renders is OUR segmenter's alpha, not Adobe's raster: the
    /// coverage will differ at every edge and can differ grossly on a hard
    /// scene. `MaskImportReason::AiMaskRecomputed` says so on the way in and
    /// `MaskLossReason::AiMaskRecomputed` on the way out, in both languages.
    /// A sidecar/model failure leaves `raster` at `None`, which renders the
    /// mask INERT (weight 0, exactly like a `Bitmap` whose file is missing) and
    /// is disclosed as carried-not-rendered — never a silent zero, never a
    /// crash.
    ///
    /// **What rides back out** is the component Lightroom wrote, verbatim: the
    /// intent fields below plus `provenance`, which holds the attributes this
    /// engine never interprets. A republished sidecar therefore still says what
    /// Lightroom said, and Lightroom recomputes its own alpha from it — which
    /// is the one round trip that IS lossless here.
    AiMask {
        /// `crs:MaskName` — a localised user string (`天空 1`, `Sky 1`,
        /// `背景1`, `水源`). Carried so a republished sidecar keeps the
        /// photographer's own label; also the only hint that a `subtype == 0`
        /// mask meant *Background* rather than *Object*, which is why that
        /// guess is NOT made here (see `subtype`).
        name: String,
        /// `crs:MaskSubType` — **0 = Object/Background, 1 = Subject, 2 = Sky**
        /// (decoded by cross-tabulating against `MaskName` over 105 instances;
        /// still exactly three values).
        ///
        /// Object and Background share the value 0 and the sidecar does not
        /// separate them: `crs:MaskSubCategoryID` appears on 8 of 105 with two
        /// values, far too few to call an enum. So the segmenter is pointed at
        /// the OBJECT under the click point in both cases, and the possibility
        /// that the photographer meant its complement is disclosed rather than
        /// guessed — the roundness rule.
        subtype: u32,
        /// `crs:ReferencePoint` — the user's own click, normalised to the
        /// frame, present on 105/105. This is the ONLY spatial information the
        /// component carries, and it is what a point-promptable segmenter
        /// (SAM 2.1) consumes directly.
        ref_x: f32,
        ref_y: f32,
        /// `crs:MaskBlendMode` on the component, VERBATIM — 0 = union onto the
        /// correction's coverage, 1 = subtract (paired with `value` 0). Same
        /// encoding, same reading and same "carried for the writer, projected
        /// onto `MaskCombine` for the render" split as
        /// [`MaskGeometry::Brush::blend_mode`].
        blend_mode: u32,
        /// `crs:MaskValue`, VERBATIM — the other half of the subtract pair,
        /// never a strength.
        value: f32,
        /// `crs:MaskInverted`.
        inverted: bool,
        /// `crs:MaskVersion` — `"1"` on 105/105. Lightroom's own schema stamp
        /// for the component, carried and never interpreted, exactly like
        /// [`MaskGeometry::Radial::mask_version`].
        mask_version: u32,
        /// Every remaining `crs:` attribute the file carried, in DOCUMENT
        /// ORDER, as `(local name, value)` — the three digests and their
        /// version tags, `WholeImageArea`, `FullMaskSize`, `Origin`,
        /// `ModelVersion`, `ErrorReason`, `MaskSubCategoryID`.
        ///
        /// **PROVENANCE ONLY. Nothing here is read.** `FullMaskSize`
        /// (`"2880,1920"`) is the resolution Adobe's segmenter worked at,
        /// `Origin` is the mask raster's bbox origin in those proxy pixels, and
        /// `WholeImageArea` is the proxy frame's EFFECTIVE IMAGE AREA — four
        /// rationals in `(top, left, bottom, right)` order. R29 me3-c
        /// (2026-08-21) measured both halves of that sentence, because the
        /// version it replaces called `WholeImageArea` the crop rect and it is
        /// not one: the specimen carries `HasCrop="True"` with a visible crop
        /// and still writes `"0/1,0/1,1920/1,2880/1"` = the whole proxy frame,
        /// and across the library 85 of 94 values are the full frame while the
        /// 9 inset ones pair with a SHORTER `FullMaskSize` (`"2880,1913"`) —
        /// the signature of a lens-geometry inset, not of a user crop. The
        /// frame those numbers live in is the UNCROPPED one: projecting the
        /// decoded alpha through `FullMaskSize` as an uncropped frame and then
        /// applying `crs:Crop*` lands it on the subject including flyaway hair,
        /// while reading `FullMaskSize` as the cropped frame lands it visibly
        /// small and displaced. They still describe a raster this engine does
        /// not have and would still mislead a renderer that consulted them.
        /// They are kept so the sidecar round-trips and so a future measurement
        /// has the numbers, not so anything can act on them.
        ///
        /// A key outside the measured vocabulary is REFUSED by the parser
        /// rather than carried ([`xmp`]'s `AI_MASK_PROVENANCE_KEYS`): an
        /// open-ended attribute bag read off disk and written into XML is an
        /// injection surface, and a name we have never seen means a writer we
        /// have not measured.
        ///
        /// [`xmp`]: crate::xmp
        provenance: Vec<(String, String)>,
        /// The `crs:Gesture` child's `Mask/Paint` strokes — the user's brush
        /// REFINEMENT of the AI mask (present on 40 of 105 components, exactly
        /// one Paint each in the current corpus census; the count is
        /// gate-checked by `scripts/check_docs.py` (the shape it describes is
        /// not). Re-measured on the
        /// library as it stands
        /// 2026-08-21: 40 gesture blocks over 10 files, one Paint each (40/40),
        /// every one of them on `MaskSubType="0"`.
        ///
        /// Carried, not composited by the renderer — and since R29 me3-c / me5
        /// (2026-08-21) that is MEASURED in both subtypes rather than registered
        /// as unknown. There is no render-time overlay to miss: a gesture is a
        /// segmentation input, not paint added to a finished alpha.
        ///
        ///  * `MaskSubType="1"` (Subject): a hand-authored `crs:Gesture` is
        ///    accepted as a first-class object — Lightroom re-serialises it,
        ///    keeps `BrushGestureInterpretation`, and silently recomputes past
        ///    the stale digests — and contributes EXACTLY ZERO to the render.
        ///    The two full-size exports are pixel-identical (39 M px,
        ///    max|Δ| = 0) and their entropy-coded segments are byte-identical.
        ///  * `MaskSubType="0"` (Object): the gesture IS read, but as an INPUT
        ///    to the segmentation — the region prompt that says which object.
        ///    Deleting it changed 511,020 px (max|Δ| = 74 DN) inside a bbox
        ///    that contains all 12 dabs, while the same frame's two Sky masks
        ///    (subtype 2, no gesture) stayed bit-identical outside it. The
        ///    component was not dropped; Lightroom recomputed a DIFFERENT alpha
        ///    from the remaining inputs.
        ///
        /// R30 B3 now uses the measured subtype-0 meaning: the ReferencePoint is
        /// the first positive SAM point, followed by every ordered `d x y` dab
        /// from every Paint, with duplicates preserved. `r/f/h`, MaskValue,
        /// Radius, Flow and hardness remain state, not points or weights; there
        /// are no negative labels, boxes, centroids, sampling or dense prompts.
        /// Subtype 1/2 gestures remain transport-only and never steer their
        /// backends. This is a segmentation-INPUT fidelity improvement, not a
        /// render gap — an AI alpha is declared a local re-derivation either way.
        /// Carrying it is still what makes the corrections whose ONLY brush
        /// content is a gesture importable at all.
        ///
        /// Carried, and since R29 C1 also TURNED: these are `BrushStroke`s and
        /// they ride `render::orient_recipe_coords`' rewrite with the rest.
        /// They are written back into the sidecar, so leaving them in the old
        /// frame would hand Lightroom a refinement stroke beside a
        /// `ReferencePoint` that had moved.
        gesture: Vec<BrushStroke>,
        /// The RECOMPUTED alpha, once our segmenter has produced one — a path
        /// to an 8-bit grey PNG inside the photo's develop dir, keyed by
        /// (photo, subtype, reference point, frame, backend generation), plus a
        /// scoped hash of the exact sent point list for subtype-0 gestures, so a
        /// re-render reuses it instead of re-running the model
        /// (`segment::resolve_ai_masks`).
        ///
        /// Machine-local, like every other raster path in this file: it joins
        /// [`LocalAdjustment::bitmap_paths_mut`], so the store relativizes,
        /// resolves, detaches and snapshots it with the rest. `None` = not
        /// resolved yet, or the model declined — which renders inert and is
        /// disclosed, never silently zero.
        ///
        /// NOT written to the sidecar: what Lightroom gets back is the intent.
        #[serde(default)]
        raster: Option<String>,
    },
}

/// One `crs:What="Mask/Paint"` stroke inside a [`MaskGeometry::Brush`] group.
///
/// Exactly nine attributes on 398/398 in-correction instances, no optional
/// fields and no variation: `What`, `MaskActive`, `MaskBlendMode`,
/// `MaskInverted`, `MaskSyncID`, `MaskValue`, `Radius`, `Flow`, `CenterWeight`,
/// plus exactly one child element, `crs:Dabs`. The three whose value is an
/// invariant (`MaskActive="true"`, `MaskBlendMode="0"`, `MaskInverted="false"`)
/// are NOT fields here: a constant is not data, and the reader refuses a
/// component that breaks one rather than storing the surprise.
///
/// Denies unknown fields for the same reason [`MaskGeometry`] does.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrushStroke {
    /// `crs:MaskValue` — a genuine 0..1 stroke DENSITY here, not the subtract
    /// half-pair: `MaskBlendMode` is 0 on every Paint, so the pairing rule
    /// cannot apply. Measured `"1"` ×305 and 10 distinct fractional values on
    /// the rest. This is the missing member of the "`BlendMode=0` with
    /// `MaskValue≠1`" bucket the R25 census counted and could not explain.
    pub value: f32,
    /// `crs:Radius` — the brush size in WIDTH units (a dab is a circle in
    /// PIXELS, hence semi-axes `(r, r·W/H)` in the normalised frame). The
    /// component attribute is the stream's INITIAL state; an `r` token
    /// overrides it. 102 components carry no `r` token at all and every one of
    /// them has a non-zero radius here, so the value is never undefined.
    pub radius: f32,
    /// `crs:Flow` — initial flow state. Exactly redundant with the stream's
    /// first `f` token wherever one exists (72/72).
    pub flow: f32,
    /// `crs:CenterWeight` — HARDNESS on 0..1, the same quantity the stream's
    /// `h` token sets (the two observed `h` values, 1.0000 and 0.0006, are
    /// members of this attribute's own value set to 4 dp).
    pub center_weight: f32,
    /// `crs:MaskSyncID` as the file spelled it. Carried so `recipe.json`
    /// remembers Lightroom's own identity for the stroke; the WRITER mints a
    /// fresh one exactly like it does for every other component it emits, so
    /// this never travels back out (see `masks_xml`).
    pub sync_id: String,
    /// The `crs:Dabs` token stream, VERBATIM — one token per line, in the
    /// order the file lists its `<rdf:li>` items.
    ///
    /// **Historical grammar measurement** (22,966 tokens over 382 components, zero
    /// malformed): four forms and no others —
    ///
    /// | token | meaning |
    /// |---|---|
    /// | `r <f>` | set the current radius |
    /// | `f <f>` | set the current flow |
    /// | `h <f>` | set the current hardness |
    /// | `d <x> <y>` | stamp a dab at (x, y) with the current state |
    ///
    /// Initial state comes from the attributes above; tokens override in
    /// stream order. The polyline is ALREADY DENSIFIED by Lightroom at
    /// 0.2000 · r (15,582 consecutive-dab steps, IQR [0.1998, 0.2001], and
    /// **zero steps exceed 1.0 r — there are no pen-lifts inside a Paint**), so
    /// a renderer interpolates nothing: it stamps the dabs it is given. The
    /// coordinate frame is the PLAIN image frame, `k = 1.00001 ± 2×10⁻⁵` —
    /// measured HERE first, and since the 2026-08-19 ruling also what
    /// `xmp::LR_MASK_FRAME_SCALE` says of radial corners (1.0: the old 1.032
    /// was one frame's lens-profile warp, not a frame constant — the
    /// constant's own comment carries the evidence). Both facts are calibrated
    /// against Lightroom's own `crs:pm_*` pixel boxes on 79 spots, sub-pixel.
    ///
    /// **Why this is a STRING and not a parsed `Vec<Dab>`.** The one thing the
    /// sidecar does not carry is the alpha KERNEL — the falloff as a function
    /// of hardness, and the per-dab accumulation law. The file stores the
    /// stroke, never the alpha, so no amount of parsing recovers it, and the
    /// only published model for it is a third-party decompile reconstruction
    /// whose `Density` field does not exist in any of those historical components.
    /// Structuring the stream before that measurement existed would freeze a
    /// shape around a renderer nobody has written; carrying it verbatim kept
    /// the round trip exact meanwhile. R27 Batches 8-10 then MADE the
    /// measurement (screen accumulation; density scales each dab BEFORE the
    /// screen; a one-parameter flow odds law; an 11-rung hardness kernel TABLE
    /// with no closed form).
    ///
    /// **Verbatim is no longer unconditional** (R29 C1, the 2026-08-21 ruling).
    /// A TURN rewrites this stream numerically — every `d` coordinate through
    /// `render::orient_point`, every `r` token and [`radius`](Self::radius)
    /// rescaled by the frame aspect, because a dab is a circle in PIXELS while
    /// the radius is in width units. The alternative was a mask that renders in
    /// the wrong place beside every parametric shape that moved, and the render
    /// is what the ruling protected. The re-emitted numbers keep Lightroom's
    /// own six decimals (`render::LR_DAB_DECIMALS`), so the file stays legal
    /// and reads back identically — but a ROTATED photo's republished stream is
    /// no longer byte-identical to what Lightroom wrote, and neither is a
    /// PORTRAIT capture's, whose sensor→display→sensor projection passes
    /// through the same rewrite twice. An unrotated landscape photo is
    /// untouched: the identity orientations return before the rewrite.
    ///
    /// ~~What still gates RENDERING, never carrying, is the kernel's missing
    /// closed form~~ — **CLOSED 2026-08-21 (R29 Batch-6), and the stream is
    /// still a String for the reason above: this type is the sidecar's shape,
    /// not the renderer's.** The closed form is
    /// `k(ρ;h) = (1 − ρ^m(h))^n(h)` with `ln m` and `ln n` cubic in the
    /// hardness — 8 numbers, pooled rms 0.0102 and held-out 0.0109 against a
    /// nine-rung measured table that the table's own interpolation only
    /// reaches 0.0180 on — and the flow law is re-measured at κ = 0.1284 ±
    /// 0.0029 (universal to 2.24 % across a 3× radius change and both hardness
    /// ends; batch-10's single-cell 0.12189 sits 5.3 % low and the two must not
    /// be quoted as agreeing better than that). `render::brush_dabs` runs this
    /// state machine at develop time and `render::brush_raster` stamps it;
    /// neither writes anything back here. (docs/V2_PLAN.md §7 item 13;
    /// `b6-analysis.md` in the R29 materials ledger, outside the tree.)
    ///
    /// ~~plus Lightroom rasterising the mask in its PRE-lens-correction
    /// frame~~ — that half is CLOSED (R29 Batch-3). Lightroom does rasterise
    /// a brush pre-correction, and so does this engine: `render::apply_masks`
    /// runs before `render::apply_lens_geometry`, so the geometry stage
    /// carries a brush mask exactly as it carries the pixels. These
    /// coordinates are therefore already in the frame a renderer would want
    /// them in, and warping them would apply the lens field twice. The
    /// measurements, the per-mask-type frame table and the two regression
    /// pins are in `render.rs`'s mask-warp block header.
    pub dabs: String,
}

impl Default for BrushStroke {
    fn default() -> Self {
        // Lightroom's own neutrals for a stroke that says nothing: full
        // density, full flow, a soft edge, no dabs. Not the TYPE's defaults —
        // `flow: 0.0` would be a stroke that paints nothing, which is a
        // legitimate value and therefore the wrong reading of "absent" (the
        // same trap `radial_midpoint_centre` documents).
        Self {
            value: 1.0,
            radius: 0.0,
            flow: 1.0,
            center_weight: 0.0,
            sync_id: String::new(),
            dabs: String::new(),
        }
    }
}

/// Serde default for [`MaskGeometry::Radial::midpoint`] — deliberately NOT the
/// TYPE's default: `f32::default()` is 0.0, which on Lightroom's 0..100 scale
/// is a legitimate, extreme value ("the falloff's half point sits at the very
/// centre"), not "unset". A radial written before this field existed carries
/// ACR's neutral 50, so that is what its absence has to read as — the same
/// field-level-default-beats-container-default trap `coord_era_legacy`
/// documents, and the reason `#[serde(default)]` alone is wrong here.
/// Pinned by `radial_extras_are_dropped_by_an_older_reader_but_never_corrupt`.
fn radial_midpoint_centre() -> f32 {
    50.0
}

/// Serde default for [`MaskGeometry::Radial::mask_version`] — 2, the value
/// every reference Lightroom sidecar carries, and again not the type's 0
/// (which would claim a schema version nobody has ever written). Same
/// rationale as [`radial_midpoint_centre`].
fn lightroom_mask_version() -> u32 {
    2
}

/// How one extra [`MaskComponent`] composes onto the coverage built so far
/// (Lightroom's Add / Subtract / Intersect grammar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaskCombine {
    /// Union: `w = 1 − (1−w)(1−c)` — smooth screen-union, so two feathered
    /// shapes overlap without a hard seam (plain `max` creases the gradient
    /// where the shapes cross).
    #[default]
    Add,
    /// Carve out: `w = w · (1 − c)` — "everything so far, minus this shape".
    Subtract,
    /// Restrict: `w = w · c` — "everything so far, only where this shape is".
    Intersect,
}

/// One extra shape composed onto a [`LocalAdjustment`]'s base `mask`, in list
/// order: coverage starts at the base geometry's weight and each component
/// folds in via its [`MaskCombine`] mode. Linear, radial, brush and AI shapes
/// export in Lightroom's own grammar. The 174-sidecar census (399 correction
/// groups, 102 composed) verifies Add = blend 0/value 1, Subtract = blend
/// 1/value 0, Intersect = subtract the inverted shape. It does NOT verify
/// Adobe's arithmetic on feathered alphas. Bitmap components remain a named
/// export loss; recipe.json keeps their full composition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MaskComponent {
    pub geometry: MaskGeometry,
    pub mode: MaskCombine,
    /// Complement this component after its geometry's own inversion. In
    /// particular, a measured asymmetric linear feather must be complemented,
    /// not approximated by reversing its handles. Absent/false is byte-stable.
    /// Older readers fail loudly only for recipes that use the new bit.
    /// SCHEMA_ERA names the R25 CRS controls, so it does not move here.
    #[serde(skip_serializing_if = "component_not_inverted")]
    pub inverted: bool,
}

fn component_not_inverted(value: &bool) -> bool { !*value }

impl MaskComponent {
    pub fn net_inverted(&self) -> bool {
        self.inverted ^ self.geometry.own_inverted()
    }
}

impl Default for MaskComponent {
    fn default() -> Self {
        Self {
            geometry: MaskGeometry::Linear { zero_x: 0.5, zero_y: 0.0, full_x: 0.5, full_y: 0.5 },
            mode: MaskCombine::Add,
            inverted: false,
        }
    }
}

/// Every finite-coordinate check the sanitizer needs for one geometry —
/// shared by the base `mask` and each component.
fn geometry_is_finite(g: &MaskGeometry) -> bool {
    match g {
        MaskGeometry::Linear { zero_x, zero_y, full_x, full_y } => {
            [zero_x, zero_y, full_x, full_y].iter().all(|v| v.is_finite())
        }
        MaskGeometry::Shaped { zero_x, zero_y, full_x, full_y, falloff } => {
            [zero_x, zero_y, full_x, full_y].iter().all(|v| v.is_finite())
                && falloff.iter().all(|v| v.is_finite())
        }
        MaskGeometry::Radial { top, left, bottom, right, feather, roundness, angle, .. } => {
            [top, left, bottom, right, feather, roundness, angle].iter().all(|v| v.is_finite())
        }
        MaskGeometry::Bitmap { .. } => true,
        // Every number a stroke carries, including the ones nothing renders
        // yet: a NaN that survives here reaches the sidecar writer, and "NaN"
        // is not something Lightroom can parse back.
        MaskGeometry::Brush { value, strokes, .. } => {
            value.is_finite()
                && strokes.iter().all(|s| {
                    [s.value, s.radius, s.flow, s.center_weight].iter().all(|v| v.is_finite())
                })
        }
        // The reference point is a real COORDINATE — it decides where the
        // segmenter clicks — so a NaN here would put the prompt nowhere. The
        // gesture strokes get the same treatment as a brush group's for the
        // same reason: they reach the sidecar writer, and "NaN" is not
        // something Lightroom can parse back.
        MaskGeometry::AiMask { ref_x, ref_y, value, gesture, .. } => {
            [ref_x, ref_y, value].iter().all(|v| v.is_finite())
                && gesture.iter().all(|s| {
                    [s.value, s.radius, s.flow, s.center_weight].iter().all(|v| v.is_finite())
                })
        }
    }
}

fn range_is_finite(range: &Option<RangeMask>) -> bool {
    match range {
        Some(RangeMask::Luminance { lo_outer, lo, hi, hi_outer }) => {
            lo_outer.is_finite() && lo.is_finite() && hi.is_finite() && hi_outer.is_finite()
        }
        Some(RangeMask::Color { r, g, b, amount, px, py }) => {
            [r, g, b, amount, px, py].iter().all(|v| v.is_finite())
        }
        None => true,
    }
}

/// Coordinate/fraction clamp for one geometry — shared by the base `mask`
/// and each component (see the COORD_LIMIT rationale at the call site).
fn clamp_geometry(g: &mut MaskGeometry) {
    const COORD_LIMIT: f32 = 8.0;
    match g {
        MaskGeometry::Linear { zero_x, zero_y, full_x, full_y } => {
            for v in [zero_x, zero_y, full_x, full_y] {
                *v = v.clamp(-COORD_LIMIT, COORD_LIMIT);
            }
        }
        MaskGeometry::Shaped { zero_x, zero_y, full_x, full_y, falloff } => {
            for v in [zero_x, zero_y, full_x, full_y] {
                *v = v.clamp(-COORD_LIMIT, COORD_LIMIT);
            }
            // A coverage is a 0..=1 fraction (a non-finite entry dropped the
            // whole correction at `geometry_is_finite`, before this).
            for v in falloff.iter_mut() {
                *v = v.clamp(0.0, 1.0);
            }
        }
        MaskGeometry::Radial { top, left, bottom, right, feather, roundness, angle, midpoint, .. } => {
            for v in [top, left, bottom, right] {
                *v = v.clamp(-COORD_LIMIT, COORD_LIMIT);
            }
            // Lightroom's Midpoint is 0..100. A corrupt one goes to its
            // NEUTRAL rather than through `clamp` (which passes NaN straight
            // through — see `Hsl::clamp`): this value is written back into a
            // sidecar Lightroom reads, and "NaN" there is not a number it can
            // parse. `mask_version` is deliberately NOT clamped — a value we
            // never interpret has no range to enforce, and rewriting it would
            // make our sidecar claim a version Lightroom never wrote.
            *midpoint = if midpoint.is_finite() { midpoint.clamp(0.0, 100.0) } else { 50.0 };
            // `feather` is a 0..1 fraction everywhere that reads it (render,
            // XMP); a stored 2.0 was a value the engine silently treated as
            // 1.0 but persistence kept.
            *feather = feather.clamp(0.0, 1.0);
            // `roundness` is NOT on that scale. It is Lightroom's ±100 slider,
            // carried and never interpreted (see `MaskGeometry::Radial`), so
            // the clamp is the SIDECAR's band — the old 0..1 clamp would have
            // crushed a real `crs:Roundness="-30"` to 0 on the way back out,
            // which is how a carried value stops being carried. v0.31.1.
            *roundness = roundness.clamp(-100.0, 100.0);
            // Rotation wraps — ±180° covers every ellipse orientation twice
            // over (the shape has 180° symmetry), and an unbounded stored
            // angle would feed sin/cos a huge argument for no extra shapes.
            *angle = angle.rem_euclid(360.0);
            if *angle > 180.0 {
                *angle -= 360.0;
            }
        }
        MaskGeometry::Bitmap { .. } => {}
        // A brush group is CARRIED, so the bands here are the SIDECAR's, not a
        // render's: every one of the 382 measured Paints already sits inside
        // them, which makes this a guard against a hand-edited recipe.json and
        // nothing else. `blend_mode` is deliberately not touched — it is a
        // Lightroom enum we re-emit verbatim, and there is no "nearest legal
        // value" for an enum (same rule `mask_version` is left alone under).
        MaskGeometry::Brush { value, strokes, .. } => {
            let unit = |v: &mut f32, neutral: f32| {
                *v = if v.is_finite() { v.clamp(0.0, 1.0) } else { neutral };
            };
            unit(value, 1.0);
            for s in strokes.iter_mut() {
                unit(&mut s.value, 1.0);
                unit(&mut s.flow, 1.0);
                unit(&mut s.center_weight, 0.0);
                // Radius is a normalised length, so it takes the coordinate
                // limit rather than the unit band — the measured maximum is
                // 0.582, but a brush wider than the frame is a shape, not a
                // corruption.
                s.radius = if s.radius.is_finite() { s.radius.clamp(0.0, COORD_LIMIT) } else { 0.0 };
            }
        }
        // An AI mask's bands are the SIDECAR's plus one that is the RENDER's:
        // the reference point is a normalised click that becomes a pixel
        // coordinate the segmenter is prompted at, so it takes the coordinate
        // limit rather than a unit band (a click just outside the frame is a
        // thing Lightroom writes — the sidecar's own values run to 3 decimal
        // places inside [0,1], but the segment bridge is what decides what is
        // reachable, not this clamp). `blend_mode`, `mask_version` and every
        // `provenance` string are left alone for `MaskGeometry::Brush`'s
        // reason: there is no "nearest legal value" for an enum, a schema stamp
        // or an opaque digest.
        MaskGeometry::AiMask { ref_x, ref_y, value, gesture, provenance, .. } => {
            for v in [ref_x, ref_y] {
                *v = if v.is_finite() { v.clamp(-COORD_LIMIT, COORD_LIMIT) } else { 0.5 };
            }
            *value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 1.0 };
            for s in gesture.iter_mut() {
                for (v, neutral) in
                    [(&mut s.value, 1.0f32), (&mut s.flow, 1.0), (&mut s.center_weight, 0.0)]
                {
                    *v = if v.is_finite() { v.clamp(0.0, 1.0) } else { neutral };
                }
                s.radius = if s.radius.is_finite() { s.radius.clamp(0.0, COORD_LIMIT) } else { 0.0 };
            }
            // A hand-edited recipe.json must not be able to make one component
            // cost an unbounded allocation, or to smuggle a novel attribute
            // into a sidecar. The COUNT is bounded here; the KEYS are an
            // allowlist in the parser and again in the writer.
            provenance.truncate(MAX_AI_PROVENANCE_ENTRIES);
            for (k, v) in provenance.iter_mut() {
                truncate_chars(k, MAX_AI_PROVENANCE_KEY_CHARS);
                truncate_chars(v, MAX_AI_PROVENANCE_VALUE_CHARS);
            }
        }
    }
}

/// Bounds for [`MaskGeometry::AiMask::provenance`] — the measured vocabulary is
/// 11 optional attributes, the longest value in the whole reference library is
/// 55 characters (`crs:WholeImageArea`), and the longest name is
/// `LocalInputDigestVersion` at 23. Each cap leaves room without leaving the
/// door open.
const MAX_AI_PROVENANCE_ENTRIES: usize = 16;
const MAX_AI_PROVENANCE_KEY_CHARS: usize = 48;
const MAX_AI_PROVENANCE_VALUE_CHARS: usize = 128;

/// Truncate on a CHARACTER boundary — `String::truncate` panics mid-codepoint,
/// and these strings come off disk where a multi-byte value is legal.
fn truncate_chars(s: &mut String, max: usize) {
    if s.chars().count() > max {
        let cut = s.char_indices().nth(max).map_or(s.len(), |(i, _)| i);
        s.truncate(cut);
    }
}

impl LocalAdjustment {
    /// Every geometry this adjustment carries — the base mask and each
    /// component — as ONE walk.
    ///
    /// The two path walks below differ only in which geometries they accept, so
    /// the walk itself lives here once: a new geometry carrier is added in a
    /// single place and neither membership rule can silently miss it.
    fn geometries_mut(&mut self) -> impl Iterator<Item = &mut MaskGeometry> {
        std::iter::once(&mut self.mask).chain(self.components.iter_mut().map(|c| &mut c.geometry))
    }

    /// Is this correction's coverage inverted — **the one place** the two
    /// inversion bits are allowed to meet.
    ///
    /// This engine carries the inversion in two independent places and both
    /// are legitimate. [`Self::inverted`] is the correction's own Invert flag
    /// (the GUI checkbox, what the zoned fit's land zone sets, what the render
    /// applies to the composed coverage); [`MaskGeometry::own_inverted`] is
    /// the bit a geometry carries itself, which is where a Lightroom sidecar
    /// homes it and therefore where the importer leaves it. Lightroom has ONE
    /// inversion per component, so the projection has to hand it their XOR —
    /// and so does anything else asking which side of the mask the correction
    /// lands on.
    ///
    /// Hand-spelling that XOR is what this method exists to stop. Before it,
    /// `xmp::lr_net_inverted` composed the radial pair and no other, so a
    /// brush or AI mask exported the geometry's bit and DROPPED the
    /// correction's; `render::mask_weight`'s AI arm applied neither; and
    /// the old style index's mask-habit bucketing read the correction's and
    /// ignored every geometry's. A sky mask inverted in Lightroom therefore round-tripped
    /// out of this app as a mask over the sky.
    ///
    /// SCOPE: the BASE geometry. A COMPONENT carries its own bit and it is
    /// applied per component — by `render::mask_weight` inside
    /// `combined_mask_weight`, and by `xmp::masks_xml`, which hands each
    /// component its own [`MaskGeometry::own_inverted`]. Lightroom inverts per
    /// component too, so folding the correction's flag into every one of them
    /// would invert each instead of their composition.
    pub fn net_inverted(&self) -> bool {
        self.inverted != self.mask.own_inverted()
    }

    /// Mutable references to every Bitmap raster path this adjustment holds
    /// (base geometry + components) — the ONE walk the store's path
    /// relativize/resolve/detach/snapshot helpers share, so a new geometry
    /// carrier can never be forgotten by one of them.
    pub fn bitmap_paths_mut(&mut self) -> Vec<&mut String> {
        // TWO carriers now (R27 Batch-5): the explicit `Bitmap` raster and the
        // RECOMPUTED alpha an `AiMask` caches. They are the same kind of thing
        // to every consumer of this walk — a machine-local PNG path that must
        // be relativized on save, resolved on load, copied when a version
        // snapshot becomes live, and swept with a deleted version — so the
        // AiMask one goes through the same door rather than growing a second.
        //
        // An EMPTY `Bitmap` path names no file — it is a face/eyes intent a
        // proposal or a hand-written recipe left for the face finder
        // (`face::is_face_intent`) — and is skipped: rebased on load it would
        // become the recipe's own folder and stop being an intent.
        self.geometries_mut()
            .filter_map(|g| match g {
                MaskGeometry::Bitmap { path } if !path.is_empty() => Some(path),
                MaskGeometry::AiMask { raster: Some(path), .. } => Some(path),
                _ => None,
            })
            .collect()
    }

    /// The subset of [`bitmap_paths_mut`](Self::bitmap_paths_mut) a FRAME TURN
    /// owns: the explicit `Bitmap` rasters, and NOT an `AiMask`'s cached alpha.
    ///
    /// The distinction is the root of R28 Batch-3 3b (adjudication F8-B). The
    /// two walks answer different questions and the rotate was asking the wrong
    /// one: `bitmap_paths_mut` means "machine-local PNGs this recipe owns and
    /// the STORE must carry along", while a rotate needs "rasters that must be
    /// turned or the photo comes back with its masks somewhere else". A
    /// `Bitmap` qualifies — its pixels are the mask, and nothing can re-derive
    /// them. An `AiMask`'s raster does not: it is a CACHE of a recomputation,
    /// and the rotation's own semantics already discard it
    /// (`render::orient_recipe_coords` clears it so the next develop
    /// re-segments at the turned point; the era-2 TRANSLATION keeps and
    /// re-writes it instead — `pipeline::migrate_recipe_geometry` says why the
    /// two walks differ). Turning it too produced a correctly-turned file nothing
    /// ever pointed at, plus a `rasters_turned` count that promised work the
    /// recipe did not keep.
    ///
    /// ONE exception since the sky/land zones became Select Sky components: a
    /// ZONE mask's alpha is not a cache either ([`MaskRole::is_zone`]). It is
    /// the fit's own render product, nothing re-derives it from the recipe,
    /// and a rotate that dropped it would leave both zone corrections inert on
    /// any machine without the segmentation sidecar — the correction would
    /// simply vanish from the photograph. `rotate90` is lossless on it exactly
    /// as it is on a `Bitmap`, so it is turned with them.
    pub fn turnable_raster_paths_mut(&mut self) -> Vec<&mut String> {
        let zone = self.role.is_zone();
        self.geometries_mut()
            .filter_map(move |g| match g {
                MaskGeometry::Bitmap { path } if !path.is_empty() => Some(path),
                MaskGeometry::AiMask { raster: Some(path), .. } if zone => Some(path),
                _ => None,
            })
            .collect()
    }
}

/// Lightroom's Range Mask: a per-pixel refinement INTERSECTED with the mask's
/// geometry (final weight = geometry × range), so a sky gradient can affect only
/// the bright pixels, or only the blues. Serialised to XMP as a second
/// `Mask/RangeMask` component inside `crs:CorrectionMasks` — structure verified
/// against the user's own Lightroom sidecars (e.g. `P07.xmp` LumRange,
/// `P09.xmp` PointModels).
// Denies unknown fields for the same reason [`MaskGeometry`] does, and in the
// same breath: the two are this crate's only internally-tagged enums, so a
// fix that covered one of them would have left the identical silent-drop hole
// open in the other.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RangeMask {
    /// Select by luminance: full weight inside [lo, hi], smooth ramps over
    /// lo_outer→lo and hi→hi_outer. All four in 0..=1, non-decreasing —
    /// exactly ACR's `crs:LumRange="lo_outer lo hi hi_outer"`.
    Luminance { lo_outer: f32, lo: f32, hi: f32, hi_outer: f32 },
    /// Select pixels whose chromaticity (brightness-independent colour) is near
    /// the reference `r,g,b` (0..=1 sRGB). `amount` 0..=1 widens the tolerance
    /// (ACR `crs:ColorAmount`, LR default 0.5). `(px, py)` is the normalised
    /// sample point in the ORIGINAL frame — cosmetic, for LR's sample marker.
    Color { r: f32, g: f32, b: f32, amount: f32, px: f32, py: f32 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClampSummary {
    pub dropped_masks: usize,
    pub dropped_components: usize,
    /// Points cut from the tone/RGB/base curves past their caps — a curve
    /// truncation changes rendered TONE exactly as a dropped mask changes
    /// coverage, and it used to vanish without a count.
    pub truncated_curve_points: usize,
    /// Bytes cut from rationale / mask names / raster paths. A truncated
    /// PATH is load-bearing (the raster stops resolving); the count keeps
    /// the disclosure honest even for the cosmetic strings.
    pub truncated_string_bytes: usize,
}

impl ClampSummary {
    pub fn is_empty(self) -> bool {
        self == ClampSummary::default()
    }

    /// Fold another summary in — the accumulator restore paths use. Field-by-
    /// field copies at call sites silently dropped the curve/string counts the
    /// moment those fields were added (16-lane scan, L14/L15/L16).
    pub fn absorb(&mut self, other: ClampSummary) {
        self.dropped_masks += other.dropped_masks;
        self.dropped_components += other.dropped_components;
        self.truncated_curve_points += other.truncated_curve_points;
        self.truncated_string_bytes += other.truncated_string_bytes;
    }

    /// English stderr rendering listing only the non-zero losses — "0 mask(s)
    /// and 0 component(s)" for a curve-only truncation was a false all-clear.
    /// GUI surfaces use their own localized 4-placeholder message instead.
    pub fn describe(self) -> String {
        let mut parts = Vec::new();
        if self.dropped_masks > 0 {
            parts.push(format!("{} mask(s)", self.dropped_masks));
        }
        if self.dropped_components > 0 {
            parts.push(format!("{} mask component(s)", self.dropped_components));
        }
        if self.truncated_curve_points > 0 {
            parts.push(format!("{} curve point(s)", self.truncated_curve_points));
        }
        if self.truncated_string_bytes > 0 {
            parts.push(format!("{} string byte(s)", self.truncated_string_bytes));
        }
        parts.join(", ")
    }
}

/// Proof-of-sanitisation token (arch item c): constructing it is the ONE
/// place entry-point clamping happens, so the public render surfaces stop
/// hand-rolling clone+clamp+disclose triplets that had already drifted.
/// Derefs to the recipe — internal render functions keep their signatures.
pub struct ValidatedRecipe {
    recipe: EditRecipe,
    pub dropped: ClampSummary,
}

impl ValidatedRecipe {
    pub fn new(r: &EditRecipe) -> Self {
        let mut recipe = r.clone();
        let dropped = recipe.clamp();
        ValidatedRecipe { recipe, dropped }
    }

    /// Say what sanitisation cost, on the caller's channel — entry points call
    /// this once; silence is reserved for the recipe that lost nothing.
    ///
    /// **R29-1 found this one.** It is raised by all four render entry points,
    /// so a `batch --jobs 3` worker reaches it per photo — and the R28 Batch-5
    /// 5c attribution sweep missed it entirely, because that sweep re-walked
    /// `pipeline.rs`, `render.rs` and `main.rs` and this line lives here. It
    /// therefore printed with no photograph on it at all while its twin in
    /// `pipeline::recipe_bytes_for` carried a stem. Taking the channel fixes
    /// both halves at once: the subject rides with it, and the caller decides
    /// where it lands.
    pub fn disclose(&self, diag: &crate::diag::Diag<'_>) {
        let d = self.dropped;
        if !d.is_empty() {
            diag.emit(
                crate::diag::Mark::WarningWord,
                format!(
                    "recipe limits discarded {} mask(s) and {} mask component(s), \
                     truncated {} curve point(s) and {} string byte(s) before rendering",
                    d.dropped_masks,
                    d.dropped_components,
                    d.truncated_curve_points,
                    d.truncated_string_bytes
                ),
            );
        }
    }
}

impl std::ops::Deref for ValidatedRecipe {
    type Target = EditRecipe;
    fn deref(&self) -> &EditRecipe {
        &self.recipe
    }
}

impl EditRecipe {
    /// The ceiling on the persisted rationale string.
    ///
    /// An abuse bound, not a disclosure budget. The layered zoned fit's typed
    /// notes (17-bin range refusals, per-generation tile sweeps, refinement
    /// readings) legitimately reach ~6 KB on the calibration pair; 4096
    /// truncated the tile ATTACHMENT disclosure off the persisted recipe while
    /// the masks stayed.
    ///
    /// Raised from 16 KiB in v1.2.4, on a measurement rather than a feeling:
    /// `rationale::MAX_NOTES` is 64 and the longest template in
    /// `rationale::keys` is 647 bytes (`FIT_NOTE_CAST_HUE_FANNED`, measured
    /// 2026-09-02), so a run that fills the note vector can put 41,408 bytes of
    /// TEMPLATE alone on the string before a single interpolated value — two
    /// and a half times the old ceiling, and the sixty-four longest templates
    /// together already came to 16,183 of its 16,384 bytes. Cutting the prose
    /// first (see `cap_rationale`) is what keeps a disclosure from being lost;
    /// this is what keeps it from being lost when there is no prose left to
    /// drop. `the_rationale_ceiling_holds_every_typed_note_a_run_can_produce`
    /// re-derives the template bound from the source, so a longer note fails a
    /// test instead of costing a disclosure. An abuse bound it remains: a
    /// hand-edited foreign recipe.json still cannot smuggle a payload past it.
    ///
    /// R36: 512 KiB, with `rationale::MAX_NOTES` 64 → 512. A zoned fit on the
    /// reference pair renders about 115 sentences, so the 64-note vector was
    /// tripping its truncation sentinel on every such run; the longest template
    /// is now 780 bytes of source span (measured by the same test, 2026-09-11),
    /// and 512 × 780 = 399,360 bytes of template alone must fit. Still an
    /// abuse bound: an honest run writes a few tens of kilobytes.
    pub(crate) const MAX_RATIONALE: usize = 512 * 1024;

    /// Clamp every slider into its documented legal range. The AI is
    /// instructed to stay in range, but we never trust the input blindly —
    /// an out-of-range value would otherwise corrupt the render downstream.
    /// The same pass bounds every vector and string a crafted recipe could
    /// inflate, drops the components it cannot make sense of, and reports
    /// what it cut in the returned [`ClampSummary`].
    pub fn clamp(&mut self) -> ClampSummary {
        let mut summary = ClampSummary::default();
        // f32::clamp passes a NaN receiver STRAIGHT THROUGH — a non-finite
        // slider from malformed input (hand-edited JSON, foreign XMP) must
        // collapse to the neutral 0.0 instead, the same corrupt-component-
        // goes-inert rule the crop and range-mask guards below follow.
        let c = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { 0.0 };
        // SIZE limits, not just value limits. A recipe is also a set of
        // VECTORS, and a crafted one (hand-edited JSON, a hostile POST to the
        // local web server) could carry thousands of masks or curve points:
        // each active mask costs a full-frame pass and each curve is cloned +
        // sorted per render, so the render thread can be monopolised without a
        // single out-of-range number. The caps sit far above any real edit
        // (the GUI offers a handful of masks; a tone curve has at most a few
        // dozen points), so they can only ever truncate abuse.
        //
        // …and STRINGS, which this comment used to claim were covered ("every
        // field below is bounded") while bounding none of them. That was not
        // theoretical. `rationale` is filled from the advisor's failure text,
        // and an upstream HTTP error body flows into it verbatim
        // (`AdvisorError::Http{body}` -> `fallback_reason` -> the heuristic's
        // rationale), so a misbehaving endpoint needed no malice to write a
        // multi-megabyte string into recipe.json AND into the XMP comment
        // beside the RAW. Past 16 MiB `store::read_sidecar` then refuses to
        // read that sidecar back, which silently costs the Lightroom merge its
        // base. The web route's own 256 MiB body cap is the only other ceiling.
        const MAX_MASKS: usize = 64;
        const MAX_CURVE_POINTS: usize = 256;
        const MAX_BASE_KNOTS: usize = 256;
        /// A mask label is a UI affordance; Lightroom's own are far shorter.
        const MAX_NAME: usize = 256;
        /// A path, not a payload — comfortably past Windows' extended limit.
        const MAX_PATH: usize = 4096;
        /// Strokes in ONE brush group. The largest Aggregate in the reference
        /// library holds 14; 256 is abuse-only headroom, sized like MAX_MASKS.
        const MAX_BRUSH_STROKES: usize = 256;
        /// One stroke's `crs:Dabs` stream. The largest real stroke is 645 dabs
        /// / 1,267 tokens ≈ 13 KB, and the WHOLE library is 15,964 dabs — so
        /// 256 KiB is twenty times the worst case and still a bound.
        const MAX_DABS: usize = 256 * 1024;
        /// Truncate on a char boundary: `String::truncate` panics off one, and
        /// this runs on input nobody validated. Returns the bytes cut so the
        /// summary can report string loss instead of hiding it.
        fn cap(s: &mut String, max: usize) -> usize {
            let before = s.len();
            if s.len() > max {
                let mut end = max;
                while end > 0 && !s.is_char_boundary(end) {
                    end -= 1;
                }
                s.truncate(end);
            }
            before - s.len()
        }
        /// [`cap`] for a `'\n'`-separated TOKEN stream — today only
        /// [`BrushStroke::dabs`]. Cuts at the last separator at or before the
        /// byte cap, so every survivor is a WHOLE token.
        ///
        /// Why `cap` is the wrong tool here (R28 2b, adjudication F5). A char
        /// boundary is not a token boundary: 65,536 `"d 0 0"` tokens are
        /// 393,215 bytes, `MAX_DABS` is 262,144, and `cap` cut that stream
        /// mid-token (`"d 0 "`). The writer then emitted the fragment
        /// unchanged — its `split('\n')` is documented as the EXACT INVERSE of
        /// the reader's join (xmp.rs `brush_mask_xml`) — and our own next read
        /// refused the fragment, taking the whole Aggregate with it: every
        /// brush mask in the group disappeared. Structured payload, byte
        /// truncator; the units never matched.
        ///
        /// The cap is therefore a CEILING, not an exact size. The result is
        /// the longest whole-token prefix that fits, which is at most `max`
        /// bytes and can be shorter by up to one token. A stream whose FIRST
        /// token already exceeds `max` truncates to empty — the caller drops
        /// such a stroke rather than emit a dab-less Paint (a Paint with no
        /// dabs is not a stroke; the reader says so by refusing one).
        ///
        /// Returns the bytes cut, like `cap`.
        fn cap_tokens(s: &mut String, max: usize) -> usize {
            let before = s.len();
            if s.len() > max {
                // `max < s.len()`, so `max` is a valid byte index. '\n' is
                // ASCII, hence always a char boundary — truncating AT the
                // separator drops it too, which is the storage form (tokens
                // joined by '\n', no trailing one).
                match s.as_bytes()[..=max].iter().rposition(|&b| b == b'\n') {
                    Some(nl) => s.truncate(nl),
                    None => s.clear(),
                }
            }
            before - s.len()
        }
        /// [`cap`] for the PROSE rationale — the one capped string a person
        /// actually reads, and the only one whose TAIL is load-bearing. Cuts
        /// from the FRONT and spends part of the budget saying that it cut.
        /// Returns the content bytes lost, like `cap`.
        ///
        /// **Which end (v1.2.4).** This string is `<the model's prose><the
        /// engine's typed notes>` — the notes concatenate onto the end, and
        /// `rationale::render_en(&notes)` is required to equal the SUFFIX. A
        /// cut from the back therefore ate the disclosures first and left the
        /// prose whole: an island run of 121 paragraphs came back as 119 with
        /// its typed notes gone, and because the suffix no longer matched, the
        /// GUI could not localize what remained either. Cutting from the FRONT
        /// inverts both: the engine's own record survives, the strip still
        /// matches, and what is lost is the model's prose — which is the half
        /// no other reader depends on.
        ///
        /// The cut lands on the next PARAGRAPH break when one is within a
        /// kilobyte of the byte cut, so the survivor starts at a sentence
        /// rather than mid-word; a note template contains no newline, so this
        /// can never advance into the disclosures.
        ///
        /// Why plain `cap` is the wrong tool here. `cap` leaves NO trace in
        /// the string: a rationale that overran `MAX_RATIONALE` simply stopped
        /// mid-sentence, and every later reader — the persisted `recipe.json`,
        /// the rationale shown beside the develop — saw a record that looked
        /// COMPLETE. The loss was reported only out of band, through
        /// `ClampSummary::truncated_string_bytes`: one line of CLI stderr, and
        /// in the GUI a restore-time toast (actions.rs / workers.rs) that is
        /// gone the moment it is dismissed and never reaches whoever opens the
        /// file next week. The NOTES vector had already solved its half of
        /// this by pushing a `rationale::TRUNCATED_SENTINEL` past `MAX_NOTES`;
        /// the string was the asymmetric side.
        ///
        /// The marker is plain English, NOT the notes vec's NUL poison pill.
        /// That pill exists so a truncated vec can never strip-match the
        /// (complete) string; this string is itself the human record, and its
        /// reader is owed a sentence. It names the ceiling and the ORIGINAL
        /// length rather than the bytes dropped, because both are known before
        /// the cut — sizing a marker against a number that the marker's own
        /// length changes has no fixed point.
        ///
        /// The byte ceiling stays the load-bearing promise: the body is cut to
        /// `max - marker.len()` so the result is at most `max` INCLUDING the
        /// marker, and a `max` too small to hold one drops the marker rather than
        /// break the ceiling. (Unreachable in practice: the constant is 512 KiB
        /// and the marker is under a hundred bytes.)
        fn cap_rationale(s: &mut String, max: usize) -> usize {
            if s.len() <= max {
                return 0;
            }
            let before = s.len();
            let marker = format!(
                "[rationale truncated at the {max}-byte ceiling; the original was {before} bytes] "
            );
            if marker.len() >= max {
                return cap(s, max);
            }
            let keep = max - marker.len();
            // The naive cut, moved UP to a char boundary so the survivor is
            // valid UTF-8 (this runs on input nobody validated).
            let mut start = before - keep;
            while start < before && !s.is_char_boundary(start) {
                start += 1;
            }
            // …and up again to the next paragraph break when there is one
            // nearby, so the survivor opens at a sentence. Moving up only ever
            // drops MORE, so the ceiling still holds.
            if let Some(nl) = s[start..].find('\n')
                && nl < 1024
            {
                start += nl + 1;
            }
            let cut = start;
            let tail = s.split_off(start);
            s.clear();
            s.push_str(&marker);
            s.push_str(&tail);
            cut
        }
        /// One brush group's strokes, bounded — sync IDs and each stroke's
        /// token stream. Shared by [`cap_geometry`]'s `Brush` and `AiMask`
        /// arms, because a carrier bounded in one and trusted in the other is
        /// the hole that function exists to keep shut. Returns
        /// `(bytes cut, strokes dropped)`.
        ///
        /// A stroke whose stream truncates to EMPTY is dropped, not kept: the
        /// writer would otherwise emit `<rdf:li></rdf:li>` and our own reader
        /// refuses a Paint with zero tokens (`xmp::parse_dabs`), which loses
        /// the whole Aggregate — the identical failure `cap_tokens` exists to
        /// prevent, arriving one step later. Only a hand-edited `recipe.json`
        /// can reach it: on the import path no single token survives 256 bytes
        /// (`xmp::dab_token_is_known`), so a 256 KiB budget always fits
        /// hundreds of them.
        fn cap_strokes(strokes: &mut Vec<BrushStroke>, name_max: usize) -> (usize, usize) {
            let mut bytes = 0;
            for s in strokes.iter_mut() {
                bytes += cap(&mut s.sync_id, name_max);
                bytes += cap_tokens(&mut s.dabs, MAX_DABS);
            }
            let before = strokes.len();
            strokes.retain(|s| !s.dabs.is_empty());
            (bytes, before - strokes.len())
        }
        /// Every STRING and every VECTOR one geometry carries, bounded —
        /// shared by the base `mask` and by each component, because a carrier
        /// bounded in one of the two loops and trusted in the other is exactly
        /// the hole `bitmap_paths_mut` exists to stop reopening. Returns
        /// `(bytes cut, strokes dropped)`.
        fn cap_geometry(g: &mut MaskGeometry, name_max: usize) -> (usize, usize) {
            match g {
                MaskGeometry::Bitmap { path } => (cap(path, MAX_PATH), 0),
                MaskGeometry::Brush { name, strokes, .. } => {
                    let bytes = cap(name, name_max);
                    let over = strokes.len().saturating_sub(MAX_BRUSH_STROKES);
                    strokes.truncate(MAX_BRUSH_STROKES);
                    let (stroke_bytes, emptied) = cap_strokes(strokes, name_max);
                    (bytes + stroke_bytes, over + emptied)
                }
                // Every string an AI mask carries, including the ones only the
                // WRITER reads. `provenance` is capped by ENTRY COUNT in
                // `clamp_geometry` and by LENGTH here, so a hand-edited
                // recipe.json cannot make one component's XML unbounded from
                // either direction.
                MaskGeometry::AiMask { name, gesture, provenance, raster, .. } => {
                    let mut bytes = cap(name, name_max);
                    if let Some(p) = raster.as_mut() {
                        bytes += cap(p, MAX_PATH);
                    }
                    let over = gesture.len().saturating_sub(MAX_BRUSH_STROKES);
                    gesture.truncate(MAX_BRUSH_STROKES);
                    let (stroke_bytes, emptied) = cap_strokes(gesture, name_max);
                    bytes += stroke_bytes;
                    for (k, v) in provenance.iter_mut() {
                        bytes += cap(k, name_max);
                        bytes += cap(v, name_max);
                    }
                    (bytes, over + emptied)
                }
                MaskGeometry::Linear { .. } | MaskGeometry::Shaped { .. } | MaskGeometry::Radial { .. } => (0, 0),
            }
        }
        summary.truncated_string_bytes +=
            cap_rationale(&mut self.rationale, Self::MAX_RATIONALE);
        // The pass-through block is STRINGS from a foreign document, so it
        // falls under the same rule as `rationale` and the mask names above:
        // the reader fills it from a fixed nine keys, but a hand-edited
        // recipe.json is not obliged to. Bounded here rather than trusted —
        // and generously, because the whole promise of these values is that
        // they ride out byte-for-byte (`crs:CameraProfile` is a profile NAME,
        // and Adobe's are long).
        const MAX_PASSTHROUGH: usize = 64;
        const MAX_PASSTHROUGH_VALUE: usize = 512;
        while self.passthrough.len() > MAX_PASSTHROUGH {
            if let Some((_, v)) = self.passthrough.pop_last() {
                summary.truncated_string_bytes += v.len();
            }
        }
        for v in self.passthrough.values_mut() {
            summary.truncated_string_bytes += cap(v, MAX_PASSTHROUGH_VALUE);
        }
        // The camera profile and the creative Look (v1.5.0 F7) are strings and
        // curves from the same foreign document. `crs:CameraProfile` had the
        // pass-through cap above until it became an owned key — a carrier
        // bounded in one home and trusted in the other is the hole
        // `cap_geometry` exists to keep shut — so it keeps the same ceiling,
        // and so do the Look's names (one of them a 32-hex table reference)
        // and its list of unrendered keys. The Look's four curves are
        // `render::curve_lut`'s input exactly like the photographer's own,
        // cloned and sorted per render, so they take the curves' cap.
        summary.truncated_string_bytes += cap(&mut self.camera_profile, MAX_PASSTHROUGH_VALUE);
        if let Some(look) = &mut self.look {
            for s in [&mut look.name, &mut look.base_profile, &mut look.table] {
                summary.truncated_string_bytes += cap(s, MAX_PASSTHROUGH_VALUE);
            }
            while look.unrendered.len() > MAX_PASSTHROUGH {
                if let Some(k) = look.unrendered.pop() {
                    summary.truncated_string_bytes += k.len();
                }
            }
            for k in look.unrendered.iter_mut() {
                summary.truncated_string_bytes += cap(k, MAX_PASSTHROUGH_VALUE);
            }
            for curve in [
                &mut look.tone_curve,
                &mut look.red_curve,
                &mut look.green_curve,
                &mut look.blue_curve,
            ] {
                summary.truncated_curve_points += curve.len().saturating_sub(MAX_CURVE_POINTS);
                curve.truncate(MAX_CURVE_POINTS);
            }
            // …and its baked half is SLIDERS, added onto the photographer's
            // own by `with_baked`: `crs:Amount` is 0..=1 and the three moves
            // are Lightroom's ±100, so they take the globals' bands.
            look.amount = c(look.amount, 0.0, 1.0);
            look.clarity = c(look.clarity, -100.0, 100.0);
            look.highlights = c(look.highlights, -100.0, 100.0);
            look.shadows = c(look.shadows, -100.0, 100.0);
        }
        summary.dropped_masks = self.masks.len().saturating_sub(MAX_MASKS);
        self.masks.truncate(MAX_MASKS);
        // A layer's one number: an opacity a NaN cannot survive (it would
        // blank the patch's whole footprint), and a count bounded like masks.
        self.pixel_layers.truncate(MAX_MASKS);
        for l in &mut self.pixel_layers {
            l.opacity = if l.opacity.is_finite() { l.opacity.clamp(0.0, 1.0) } else { 1.0 };
        }
        // The lens blur's dials and focus point clamp to their ranges; one
        // with no depth map has nothing to draw from and is dropped whole.
        if let Some(lb) = &mut self.lens_blur {
            lb.clamp();
            summary.truncated_string_bytes += cap(&mut lb.depth_map, MAX_PATH);
            // An empty protect path names nothing: no protection, as absent.
            if let Some(p) = &mut lb.protect {
                summary.truncated_string_bytes += cap(p, MAX_PATH);
                if p.is_empty() {
                    lb.protect = None;
                }
            }
            if lb.depth_map.is_empty() {
                self.lens_blur = None;
            }
        }
        for m in &mut self.masks {
            summary.truncated_string_bytes += cap(&mut m.name, MAX_NAME);
            // Strokes cut from an over-long brush group count as COMPONENTS:
            // `Mask/Paint` is a component in Lightroom's own vocabulary (it
            // lives in the group's `<crs:Masks>` list), and the existing word
            // says what happened without inventing a fifth counter no
            // disclosure surface would print.
            let (bytes, strokes) = cap_geometry(&mut m.mask, MAX_NAME);
            summary.truncated_string_bytes += bytes;
            summary.dropped_components += strokes;
        }
        summary.truncated_curve_points +=
            self.base_curve.len().saturating_sub(MAX_BASE_KNOTS);
        self.base_curve.truncate(MAX_BASE_KNOTS);
        for curve in [
            &mut self.tone_curve,
            &mut self.red_curve,
            &mut self.green_curve,
            &mut self.blue_curve,
        ] {
            summary.truncated_curve_points += curve.len().saturating_sub(MAX_CURVE_POINTS);
            curve.truncate(MAX_CURVE_POINTS);
        }
        // Base-curve knots are (x, y) in 0..1 and are composed UNDER the user
        // curve — a non-finite or wild pair renders as a black/blown band.
        self.base_curve.retain(|k| k[0].is_finite() && k[1].is_finite());
        for k in self.base_curve.iter_mut() {
            k[0] = k[0].clamp(0.0, 1.0);
            k[1] = k[1].clamp(0.0, 1.0);
        }
        self.exposure_ev = c(self.exposure_ev, -5.0, 5.0);
        self.contrast = c(self.contrast, -100.0, 100.0);
        self.highlights = c(self.highlights, -100.0, 100.0);
        self.shadows = c(self.shadows, -100.0, 100.0);
        self.whites = c(self.whites, -100.0, 100.0);
        self.blacks = c(self.blacks, -100.0, 100.0);
        // A Lightroom document's Tint range (±150), so an imported value is
        // kept rather than clamped; within ±100 a render is what it was.
        self.tint = c(self.tint, -150.0, 150.0);
        self.vibrance = c(self.vibrance, -100.0, 100.0);
        self.saturation = c(self.saturation, -100.0, 100.0);
        self.clarity = c(self.clarity, -100.0, 100.0);
        self.dehaze = c(self.dehaze, -100.0, 100.0);
        self.texture = c(self.texture, -100.0, 100.0);
        // The Calibration panel and the B&W mixer (v1.5.0): Lightroom's ±100
        // sliders, all of them.
        for v in [
            &mut self.cal_shadow_tint,
            &mut self.cal_red_hue,
            &mut self.cal_red_sat,
            &mut self.cal_green_hue,
            &mut self.cal_green_sat,
            &mut self.cal_blue_hue,
            &mut self.cal_blue_sat,
            &mut self.gray_red,
            &mut self.gray_orange,
            &mut self.gray_yellow,
            &mut self.gray_green,
            &mut self.gray_aqua,
            &mut self.gray_blue,
            &mut self.gray_purple,
            &mut self.gray_magenta,
            // …and the SDR rendition (v1.5.0 F8), which is seven more of them.
            &mut self.sdr_blend,
            &mut self.sdr_brightness,
            &mut self.sdr_contrast,
            &mut self.sdr_highlights,
            &mut self.sdr_shadows,
            &mut self.sdr_whites,
            &mut self.sdr_clarity,
        ] {
            *v = c(*v, -100.0, 100.0);
        }
        // The HDR headroom is a COUNT OF STOPS, not a ±100 slider, and it has
        // no negative meaning: headroom below diffuse white is not headroom.
        // The ceiling is this engine's refusal bound, not Lightroom's — eight
        // stops above diffuse white is already far past any display, and a
        // sidecar claiming more is a document to distrust, not to obey.
        self.hdr_max_ev = c(self.hdr_max_ev, 0.0, 8.0);
        self.point_colors.truncate(MAX_POINT_COLORS);
        for p in &mut self.point_colors {
            p.clamp();
        }
        // The parametric curve (v1.5.0): Lightroom's ±100 regions, and each
        // split in its own band — the band its slider has with the other two
        // at rest. ORDER is not enforced here: a split's legal range depends on
        // its neighbours, so a per-field clamp that also ordered them would
        // move a value the registry states as legal (`parametric_splits`
        // orders them where they are used). A non-finite split is not a
        // position and returns to its default.
        for v in [
            &mut self.param_shadows,
            &mut self.param_darks,
            &mut self.param_lights,
            &mut self.param_highlights,
        ] {
            *v = c(*v, -100.0, 100.0);
        }
        for (i, v) in [
            &mut self.param_shadow_split,
            &mut self.param_midtone_split,
            &mut self.param_highlight_split,
        ]
        .into_iter()
        .enumerate()
        {
            let (lo, hi) = parametric_split_band(i);
            *v = if v.is_finite() { v.clamp(lo, hi) } else { PARAMETRIC_SPLITS[i] };
        }
        self.hsl.clamp();
        self.color_grade.clamp();
        self.sharpening = c(self.sharpening, 0.0, 150.0);
        self.noise_reduction = c(self.noise_reduction, 0.0, 100.0);
        // The carried detail axes (R25 B3). The radius keeps its DECIMAL —
        // Lightroom's own band is 0.5..3.0 in tenths and `crs:SharpenRadius`
        // is written with one, so rounding it here would quantise the value
        // the sidecar carries. The other seven are integer sliders.
        // ONE decimal, the grid `crs:SharpenRadius="+1.0"` is written on —
        // rounded here rather than only at the writer, so what the panel
        // shows and what the sidecar carries are the same number (the same
        // rule `post_crop_vignette_style` states below).
        self.sharpen_radius = (c(self.sharpen_radius, 0.0, 3.0) * 10.0).round() / 10.0;
        for v in [
            &mut self.sharpen_detail,
            &mut self.sharpen_mask,
            &mut self.nr_detail,
            &mut self.nr_contrast,
            &mut self.color_nr,
            &mut self.color_nr_detail,
            &mut self.color_nr_smooth,
        ] {
            *v = c(*v, 0.0, 100.0);
        }
        self.lens_vignette = c(self.lens_vignette, -100.0, 100.0);
        self.lens_vignette_mid = c(self.lens_vignette_mid, 0.0, 100.0);
        self.lens_distortion = c(self.lens_distortion, -100.0, 100.0);
        self.lens_profile_distortion_scale = c(self.lens_profile_distortion_scale, 0.0, 200.0);
        self.lens_profile_vignetting_scale = c(self.lens_profile_vignetting_scale, 0.0, 200.0);
        self.perspective_vertical = c(self.perspective_vertical, -100.0, 100.0);
        self.perspective_horizontal = c(self.perspective_horizontal, -100.0, 100.0);
        self.perspective_rotate = c(self.perspective_rotate, -10.0, 10.0);
        self.perspective_scale = c(self.perspective_scale, 50.0, 150.0);
        self.perspective_aspect = c(self.perspective_aspect, -100.0, 100.0);
        self.perspective_x = c(self.perspective_x, -100.0, 100.0);
        self.perspective_y = c(self.perspective_y, -100.0, 100.0);
        // The mode is an index into Lightroom's own six, so it is clamped to
        // them and rounded: a fractional mode would select no matrix at all.
        self.perspective_upright = c(self.perspective_upright.round(), 0.0, 5.0);
        // A matrix whose numbers are not finite is not a matrix. Dropping the
        // whole list keeps the index → mode correspondence, which a per-entry
        // filter would silently shift.
        if self.upright_transform.iter().flatten().any(|v| !v.is_finite()) {
            self.upright_transform.clear();
        }
        // Only Lightroom's six modes can select a matrix (`perspective_upright`
        // is clamped to 0..=5 above), so entries past the sixth are memory
        // nothing reads — a hand-edited file could carry a million of them
        // until 2026-09-24.
        self.upright_transform.truncate(6);
        self.ca_r = c(self.ca_r, -100.0, 100.0);
        self.ca_b = c(self.ca_b, -100.0, 100.0);
        // De-fringe: Adobe's own bands (amount 0..20, hue windows 0..100).
        // A hand-edited file cannot push a value outside them and reach the
        // sidecar, exactly as for the effects below.
        self.defringe_purple = c(self.defringe_purple, 0.0, 20.0);
        self.defringe_green = c(self.defringe_green, 0.0, 20.0);
        for v in [
            &mut self.defringe_purple_lo,
            &mut self.defringe_purple_hi,
            &mut self.defringe_green_lo,
            &mut self.defringe_green_hi,
        ] {
            *v = c(*v, 0.0, 100.0);
        }
        // The carried effects (R25 B2): bands are Adobe's own, so a
        // hand-edited value can never reach the sidecar outside them.
        self.post_crop_vignette = c(self.post_crop_vignette, -100.0, 100.0);
        self.post_crop_vignette_mid = c(self.post_crop_vignette_mid, 0.0, 100.0);
        self.post_crop_vignette_feather = c(self.post_crop_vignette_feather, 0.0, 100.0);
        self.post_crop_vignette_round = c(self.post_crop_vignette_round, -100.0, 100.0);
        // An operator INDEX, not a band: 1/2/3 are Adobe's three vignette
        // styles and 0 is "none named". Rounded here rather than only at the
        // writer so what the panel shows and what the sidecar carries are the
        // same number.
        self.post_crop_vignette_style = c(self.post_crop_vignette_style, 0.0, 3.0).round();
        self.post_crop_vignette_hl = c(self.post_crop_vignette_hl, 0.0, 100.0);
        self.grain = c(self.grain, 0.0, 100.0);
        self.grain_size = c(self.grain_size, 0.0, 100.0);
        self.grain_rough = c(self.grain_rough, 0.0, 100.0);
        // An explicit zero names a companion whose stored value IS 0; any
        // other entry (an unknown name from a hand-edited file, a name whose
        // field is non-zero) would claim something the fields do not say.
        let explicit: Vec<String> = LR_COMPANION_DEFAULTS
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| self.explicit_zero.iter().any(|e| e == n))
            .filter(|n| self.clone_companions().companion_slot(n).is_some_and(|(v, _)| *v == 0.0))
            .map(str::to_string)
            .collect();
        self.explicit_zero = explicit;
        self.explicit_zero.sort();
        self.lens_profile.clamp();
        self.straighten_deg = c(self.straighten_deg, -45.0, 45.0);
        // A quarter turn is CYCLIC, so out-of-domain folds rather than
        // saturating: 4 is not "the largest legal turn", it is no turn at all,
        // and clamping it to 3 would invent a three-quarter rotation nobody
        // asked for. The rest of this function clamps because its ranges are
        // intervals; this one is a residue class.
        self.quarter_turns %= 4;
        self.confidence = c(self.confidence, 0.0, 1.0);
        self.temperature_k = match self.temperature_k {
            // A Lightroom document's Temperature range, 2000–50000 K
            // (v1.17.0), so an imported value is kept rather than clamped.
            // The render saturates at its blackbody fit's 40000 K
            // (`wb::kelvin_to_rgb`), so the top 10000 K render as 40000 K.
            Some(k) if k.is_finite() => Some(k.clamp(2000.0, 50000.0)),
            // A non-finite Kelvin cannot fall back to the closure's 0.0 (out
            // of range) — corrupt WB goes back to as-shot.
            _ => None,
        };
        // The as-shot anchor obeys the same bands as the sliders; corrupt
        // values fall back to "unknown"
        // = the historical 5500 K anchor, never a nonsense anchor.
        self.as_shot_k = match self.as_shot_k {
            Some(k) if k.is_finite() => Some(k.clamp(2000.0, 50000.0)),
            _ => None,
        };
        self.as_shot_tint = match self.as_shot_tint {
            Some(t) if t.is_finite() => Some(t.clamp(-150.0, 150.0)),
            _ => None,
        };
        // Crop is a normalized [0,1] view-frame rectangle; untrusted input
        // (AI JSON, foreign XMP, a hand-edited recipe) can carry values
        // outside the frame or an inverted/empty rectangle. Clamp into the
        // frame, order the edges, and drop a degenerate crop entirely — the
        // geometry stage must never see an empty rectangle.
        if let Some(cr) = &mut self.crop {
            // NaN SURVIVES f32::clamp and defeats every comparison below — a
            // non-finite coordinate (malformed foreign XMP) drops the crop.
            if !(cr.left.is_finite()
                && cr.top.is_finite()
                && cr.right.is_finite()
                && cr.bottom.is_finite())
            {
                self.crop = None;
            } else {
                cr.left = cr.left.clamp(0.0, 1.0);
                cr.top = cr.top.clamp(0.0, 1.0);
                cr.right = cr.right.clamp(0.0, 1.0);
                cr.bottom = cr.bottom.clamp(0.0, 1.0);
                if cr.left > cr.right {
                    std::mem::swap(&mut cr.left, &mut cr.right);
                }
                if cr.top > cr.bottom {
                    std::mem::swap(&mut cr.top, &mut cr.bottom);
                }
                if cr.right - cr.left < 1e-3 || cr.bottom - cr.top < 1e-3 {
                    self.crop = None;
                }
            }
        }
        // The colour field is a shape and the vertices it declares, and the
        // render trusts the shape: once `renderable` agrees the grid holds
        // `x·y·b` vertices, `apply_colour_field` indexes it by (x, y, b)
        // without a bounds check. A grid that disagrees with its own shape is
        // therefore not a field — the render refuses it already, so carrying
        // it on is dead weight in recipe.json and the sidecar payload — and
        // a non-finite vertex would ride NaN into every pixel through the
        // same clamp-passes-NaN rule the sliders above guard. Both drop the
        // field whole, the crop's policy for a geometry with no sensible
        // neutral; the amount is the slider it is.
        if let Some(f) = &mut self.colour_field {
            f.amount = c(f.amount, 0.0, 1.0);
            if !f.grid_fits_shape() || f.grid.iter().flatten().any(|v| !v.is_finite()) {
                self.colour_field = None;
            }
        }
        // Mask GEOMETRY first: a non-finite coordinate (hand-edited or
        // foreign input) makes mask_weight return NaN, which defeats the
        // weight early-out and paints corrupt pixels — drop the whole mask,
        // the same policy as a NaN crop (an unusable geometry has no
        // sensible neutral to collapse to). A corrupt COMPONENT drops the
        // whole mask too: dropping only the component silently changes what
        // the mask covers (a lost Subtract WIDENS the effect area), which is
        // worse than losing the mask outright.
        let masks_before_retain = self.masks.len();
        self.masks.retain(|m| {
            geometry_is_finite(&m.mask) && m.components.iter().all(|c| geometry_is_finite(&c.geometry))
        });
        // A corrupt range changes the combined coverage just as surely as corrupt
        // geometry. Dropping only the range would widen the adjustment.
        self.masks.retain(|m| range_is_finite(&m.range));
        // Size cap for components, same reasoning as MAX_MASKS: each is a
        // per-pixel weight evaluation inside the render's hot loop.
        summary.dropped_masks = summary
            .dropped_masks
            .saturating_add(masks_before_retain.saturating_sub(self.masks.len()));
        const MAX_MASK_COMPONENTS: usize = 16;
        for m in self.masks.iter_mut() {
            summary.dropped_components = summary
                .dropped_components
                .saturating_add(m.components.len().saturating_sub(MAX_MASK_COMPONENTS));
            m.components.truncate(MAX_MASK_COMPONENTS);
            for c in m.components.iter_mut() {
                let (bytes, strokes) = cap_geometry(&mut c.geometry, MAX_NAME);
                summary.truncated_string_bytes += bytes;
                summary.dropped_components += strokes;
            }
        }
        // Finite is NOT sufficient. Mask coordinates are NORMALISED (0..1
        // over the frame, a little outside for handles dragged past the
        // edge), and the engine squares differences: a legal-looking 1e30
        // overflows to Inf, Inf/Inf yields NaN weights, and those pixels
        // quantise to black. Bound the magnitude to the range the UI can
        // actually produce.
        for m in self.masks.iter_mut() {
            clamp_geometry(&mut m.mask);
            for c in m.components.iter_mut() {
                clamp_geometry(&mut c.geometry);
            }
        }
        // Clamp each local adjustment to the same UI ranges as the globals.
        for m in self.masks.iter_mut() {
            // Same finite-or-neutral closure as the globals: a NaN amount
            // used to survive and poison every weighted blend downstream.
            m.amount = c(m.amount, 0.0, 1.0);
            m.exposure_ev = c(m.exposure_ev, -5.0, 5.0);
            for v in [
                &mut m.contrast, &mut m.highlights, &mut m.shadows, &mut m.whites,
                &mut m.blacks, &mut m.clarity, &mut m.dehaze, &mut m.texture,
                &mut m.sharpness, &mut m.saturation,
                &mut m.temperature, &mut m.tint,
            ] {
                *v = c(*v, -100.0, 100.0);
            }
            // The local Hue's own travel: Lightroom's half turn either way.
            m.hue = c(m.hue, -LOCAL_HUE_MAX_DEG, LOCAL_HUE_MAX_DEG);
            m.noise_reduction = c(m.noise_reduction, 0.0, 100.0);
            m.toning_amount = c(m.toning_amount, 0.0, 100.0);
            // The tint's hue CLAMPS to the 0..=360 band the advisor registry
            // states for it, like every other local slider — a wrap (the
            // colour-grade wheels' rule) would make that stated band a lie.
            m.toning_hue = c(m.toning_hue, 0.0, 360.0);
            // The four local point curves (R25 P6) take the GLOBAL curves'
            // own cap, spelled once above: each is cloned and sorted per
            // render exactly like `tone_curve`, and 64 masks × 4 curves is
            // already the widest surface a crafted recipe has here. Point
            // VALUES need no clamp — `CurvePoint` is a pair of `u8`, so
            // 0..=255 is the type.
            for curve in [
                &mut m.main_curve,
                &mut m.red_curve,
                &mut m.green_curve,
                &mut m.blue_curve,
            ] {
                summary.truncated_curve_points += curve.len().saturating_sub(MAX_CURVE_POINTS);
                curve.truncate(MAX_CURVE_POINTS);
            }
            // Recolour gains: keep each channel strictly positive and inside
            // a generous-but-sane range (0 would kill a channel outright; the
            // real repaint demands measure ≲ 3×). Neutral-ish gains collapse
            // back to None so a hand-rounded recipe stays clean.
            if let Some(g) = &mut m.color_gains {
                for ch in g.iter_mut() {
                    // Neutral gain is 1.0 — the right inert value for a
                    // non-finite channel (0.0 would kill the channel).
                    *ch = if ch.is_finite() { ch.clamp(0.05, 8.0) } else { 1.0 };
                }
                if g.iter().all(|ch| (ch - 1.0).abs() < 1e-3) {
                    m.color_gains = None;
                }
            }
            // Range mask invariants: everything in 0..=1, and the luminance
            // trapezoid non-decreasing (lo_outer ≤ lo ≤ hi ≤ hi_outer) so the
            // render's ramps and ACR's LumRange both stay well-formed.
            // Non-finite ranges were removed with their whole mask above, so
            // every remaining value is safe to clamp without widening coverage.

            match &mut m.range {
                Some(RangeMask::Luminance { lo_outer, lo, hi, hi_outer }) => {
                    let a = lo.clamp(0.0, 1.0);
                    let b = hi.clamp(0.0, 1.0);
                    let (a, b) = if a <= b { (a, b) } else { (b, a) };
                    *lo = a;
                    *hi = b;
                    *lo_outer = lo_outer.clamp(0.0, a);
                    *hi_outer = hi_outer.clamp(b, 1.0);
                }
                Some(RangeMask::Color { r, g, b, amount, px, py }) => {
                    for v in [r, g, b, amount, px, py] {
                        *v = v.clamp(0.0, 1.0);
                    }
                }
                None => {}
            }
        }
        summary
    }

    /// Taste guardrail for **AI-proposed** recipes (never manual edits): keep a
    /// finished develop from over-cooking the tone. Two rules:
    ///  1. **Couple highlight recovery to the white point.** Pulling Highlights
    ///     negative without raising Whites drags specular whites (sea foam, clouds,
    ///     sun glints) to grey — so recovery lifts Whites proportionally. This is the
    ///     principled "keep whites white", at the recipe layer (the renderer stays
    ///     faithful; it does not override the recipe). It is applied as a FLOOR
    ///     *after* rule 2, so rule 2 can never shave it — see the ordering note
    ///     in the body.
    ///  2. **Soft-cap over-aggressive tone moves** toward a tasteful ceiling with a
    ///     smooth knee (not a hard clip), so Highlights/Shadows asymptote near ±70
    ///     and Whites/Blacks near ±45 instead of slamming to the ±100 schema bound.
    ///
    /// `strength` scales rule 2 only — see [`GradeStrength::soft_cap_factor`] for
    /// the formula and [`GradeStrength::CALIBRATED`] for the point at which this
    /// function reproduces the shipped knees exactly. Rule 1 is **not** scaled:
    /// it is not a taste knob but the fix for a measured defect (bd3f9d4 —
    /// Highlights −78.81 with Whites +10.27 dragged sea foam to grey), so scaling
    /// it by strength would re-open that defect at low strength and over-apply it
    /// at high strength.
    pub fn temper(&mut self, strength: GradeStrength) {
        // Smoothly compress a magnitude past `knee` toward `ceil` (C1-continuous at
        // the knee; asymptotes to `ceil`, so |out| < ceil always). Identity below knee.
        fn soft_cap(v: f32, knee: f32, ceil: f32) -> f32 {
            let a = v.abs();
            if a <= knee {
                return v;
            }
            let span = ceil - knee;
            let excess = a - knee;
            v.signum() * (knee + span * (excess / (excess + span)))
        }
        // ONE factor for all four knees, so the calibrated RATIO between the
        // tone pair (50→70) and the point pair (30→45) survives the scaling.
        let f = strength.soft_cap_factor();
        let (tone_knee, tone_ceil) = (TEMPER_TONE_KNEE * f, TEMPER_TONE_CEIL * f);
        let (point_knee, point_ceil) = (TEMPER_POINT_KNEE * f, TEMPER_POINT_CEIL * f);
        // Couple recovery to whites. Read off the ORIGINAL highlights (the
        // recovery the model asked for), and applied as a FLOOR *after* the
        // point soft-cap. NOT scaled by `strength` — see the doc comment:
        // white-point protection is a measured defect fix, not a taste dial.
        //
        // The order is the rule, not an accident (R23 review NIT-2). Running the
        // coupling BEFORE the cap let the cap eat into it at low strength: at
        // s = 0 (knee 19.5, ceiling 29.25) a Highlights of −100 asks for whites
        // 30 and came back 24.56 — the one rule this axis promises never to
        // touch, scaled after all, and invisible to a test that only probed
        // −40. As a floor the promise holds at every strength, and nothing moves
        // at or above the calibration point: the guard's own ceiling is 30 (the
        // ±100 Highlights range caps −h·0.3 there, below the `.min(50)`), which
        // is exactly `TEMPER_POINT_KNEE` at f = 1, and `soft_cap` is the
        // identity below its knee — so max(cap(w), g) == cap(max(w, g)) for
        // every f ≥ 1. `clamp` runs before `temper` at both production call
        // sites (advisor::proposer, advisor::heuristic), which is what keeps
        // −h·0.3 inside that 30.
        fn white_point_floor(highlights: f32) -> Option<f32> {
            (highlights < 0.0).then(|| (-highlights * 0.3).min(50.0))
        }
        let guard = white_point_floor(self.highlights);
        self.highlights = soft_cap(self.highlights, tone_knee, tone_ceil);
        self.shadows = soft_cap(self.shadows, tone_knee, tone_ceil);
        self.whites = soft_cap(self.whites, point_knee, point_ceil);
        if let Some(g) = guard {
            self.whites = self.whites.max(g);
        }
        self.blacks = soft_cap(self.blacks, point_knee, point_ceil);
        // Same restraint on each local mask's tone sliders.
        for m in self.masks.iter_mut() {
            // Mask side of the SAME white-point protection — also unscaled, and
            // floored after the cap for the same reason.
            let guard = white_point_floor(m.highlights);
            m.highlights = soft_cap(m.highlights, tone_knee, tone_ceil);
            m.shadows = soft_cap(m.shadows, tone_knee, tone_ceil);
            m.whites = soft_cap(m.whites, point_knee, point_ceil);
            if let Some(g) = guard {
                m.whites = m.whites.max(g);
            }
            m.blacks = soft_cap(m.blacks, point_knee, point_ceil);
        }
    }

    /// A companion field by serde name, mutably, with its Lightroom default —
    /// `None` for a name that is not in [`LR_COMPANION_DEFAULTS`].
    fn companion_slot(&mut self, name: &str) -> Option<(&mut f32, f32)> {
        let default = LR_COMPANION_DEFAULTS.iter().find(|(n, _)| *n == name)?.1;
        let slot = match name {
            "sharpening" => &mut self.sharpening,
            "sharpen_radius" => &mut self.sharpen_radius,
            "sharpen_detail" => &mut self.sharpen_detail,
            "nr_detail" => &mut self.nr_detail,
            "color_nr_detail" => &mut self.color_nr_detail,
            "color_nr_smooth" => &mut self.color_nr_smooth,
            "grain_size" => &mut self.grain_size,
            "grain_rough" => &mut self.grain_rough,
            "post_crop_vignette_mid" => &mut self.post_crop_vignette_mid,
            "post_crop_vignette_feather" => &mut self.post_crop_vignette_feather,
            "post_crop_vignette_style" => &mut self.post_crop_vignette_style,
            _ => return None,
        };
        Some((slot, default))
    }

    /// The Calibration panel as the render reads it — shadows tint, then red,
    /// green and blue hue/saturation — or `None` while every slider is 0.
    pub fn calibration(&self) -> Option<[f32; 7]> {
        let cal = [
            self.cal_shadow_tint,
            self.cal_red_hue,
            self.cal_red_sat,
            self.cal_green_hue,
            self.cal_green_sat,
            self.cal_blue_hue,
            self.cal_blue_sat,
        ];
        cal.iter().any(|v| *v != 0.0).then_some(cal)
    }

    /// The seven SDR-rendition controls (v1.5.0 F8) in the order
    /// [`crate::xmp::SDR_CRS`] names them: Blend, Brightness, Contrast,
    /// Highlights, Shadows, Whites, Clarity.
    ///
    /// Unconditional, unlike [`calibration`](Self::calibration): the writer
    /// emits these key by key rather than as a block, so there is no
    /// "is the block active" question for this accessor to answer — and the
    /// RENDER's own question is a different one, asked by
    /// `render::hdr::SdrRendition::global`, which must also weigh
    /// [`hdr_edit`](Self::hdr_edit) and the headroom.
    pub fn sdr_controls(&self) -> [f32; 7] {
        [
            self.sdr_blend,
            self.sdr_brightness,
            self.sdr_contrast,
            self.sdr_highlights,
            self.sdr_shadows,
            self.sdr_whites,
            self.sdr_clarity,
        ]
    }

    /// The B&W mixer's eight bands in [`HSL_BANDS`] order.
    pub fn gray_mixer(&self) -> [f32; 8] {
        [
            self.gray_red,
            self.gray_orange,
            self.gray_yellow,
            self.gray_green,
            self.gray_aqua,
            self.gray_blue,
            self.gray_purple,
            self.gray_magenta,
        ]
    }

    /// The B&W mixer band `i` ([`HSL_BANDS`] order), mutably — the one door
    /// the panel's eight rows share.
    pub fn gray_mixer_mut(&mut self, i: usize) -> Option<&mut f32> {
        Some(match i {
            0 => &mut self.gray_red,
            1 => &mut self.gray_orange,
            2 => &mut self.gray_yellow,
            3 => &mut self.gray_green,
            4 => &mut self.gray_aqua,
            5 => &mut self.gray_blue,
            6 => &mut self.gray_purple,
            7 => &mut self.gray_magenta,
            _ => return None,
        })
    }

    /// The parametric curve's four REGION sliders — shadows, darks, lights,
    /// highlights — or `None` while every one is 0: the splits alone move no
    /// pixel (`render::parametric_lut`).
    pub fn parametric_regions(&self) -> Option<[f32; 4]> {
        let regions = [self.param_shadows, self.param_darks, self.param_lights, self.param_highlights];
        regions.iter().any(|v| *v != 0.0).then_some(regions)
    }

    /// The three parametric splits as the curve USES them, in percent: each
    /// at least [`PARAMETRIC_SPLIT_GAP`] above the one before it (and the
    /// start of the range) and short of the end by as much, resolved left to
    /// right. The panel's sliders cannot cross (their bounds follow their
    /// neighbours), so this changes nothing a slider set — it is what keeps a
    /// hand-edited file's crossed splits from handing the curve an empty or
    /// inverted region.
    pub fn parametric_splits(&self) -> [f32; 3] {
        let stored = [self.param_shadow_split, self.param_midtone_split, self.param_highlight_split];
        let mut floor = 0.0;
        std::array::from_fn(|i| {
            let (_, ceiling) = parametric_split_band(i);
            let v = if stored[i].is_finite() { stored[i] } else { PARAMETRIC_SPLITS[i] };
            floor = v.clamp(floor + PARAMETRIC_SPLIT_GAP, ceiling);
            floor
        })
    }

    /// The value a companion control RENDERS at: its stored value, or
    /// Lightroom's default when the stored 0 means "absent" (see
    /// [`EditRecipe::explicit_zero`]). Any other control name reads 0 — a
    /// caller asking for a non-companion here has the wrong function, and the
    /// render tests would show a neutral pass rather than a wrong one.
    pub fn resolved(&self, name: &str) -> f32 {
        let mut probe = self.clone_companions();
        match probe.companion_slot(name) {
            Some((v, _)) if *v != 0.0 || self.explicit_zero.iter().any(|n| n == name) => *v,
            Some((_, default)) => default,
            None => 0.0,
        }
    }

    /// Set a companion to the value a slider showed: 0 becomes an EXPLICIT
    /// zero, anything else is stored as itself and leaves the list. Unknown
    /// names are ignored.
    pub fn set_resolved(&mut self, name: &str, value: f32) {
        let Some((slot, _)) = self.companion_slot(name) else { return };
        *slot = value;
        self.explicit_zero.retain(|n| n != name);
        if value == 0.0 {
            self.explicit_zero.push(name.to_string());
            self.explicit_zero.sort();
        }
    }

    /// Return a companion to "the sidecar said nothing": stored 0, not explicit.
    pub fn clear_resolved(&mut self, name: &str) {
        if let Some((slot, _)) = self.companion_slot(name) {
            *slot = 0.0;
            self.explicit_zero.retain(|n| n != name);
        }
    }

    /// The Sharpening amount the CAPTURE renders at (v1.6.0). Lightroom's
    /// default for it is a property of the source's KIND — `Sharpness` 40 on
    /// a RAW negative, none on a baked raster (a JPEG, a TIFF, an AI
    /// rendition) — so a RAW whose recipe holds no value renders at
    /// [`LR_RAW_SHARPENING`] through [`resolved`](Self::resolved), a baked one
    /// at its stored value, and a real 0 on a RAW is an explicit zero. Every
    /// rendering surface passes the kind it develops (`decode::is_raw`); the
    /// analysis surfaces (the reverse fit, the judge) develop their
    /// thumbnails as baked, which is what they did before the default existed.
    pub fn capture_sharpening(&self, raw_source: bool) -> f32 {
        if raw_source { self.resolved("sharpening") } else { self.sharpening }
    }

    /// Just the eleven companion fields and the list, copied into an otherwise
    /// default recipe — what [`resolved`](Self::resolved) needs to borrow a
    /// slot without cloning masks, curves and fields.
    fn clone_companions(&self) -> EditRecipe {
        EditRecipe {
            sharpening: self.sharpening,
            sharpen_radius: self.sharpen_radius,
            sharpen_detail: self.sharpen_detail,
            nr_detail: self.nr_detail,
            color_nr_detail: self.color_nr_detail,
            color_nr_smooth: self.color_nr_smooth,
            grain_size: self.grain_size,
            grain_rough: self.grain_rough,
            post_crop_vignette_mid: self.post_crop_vignette_mid,
            post_crop_vignette_feather: self.post_crop_vignette_feather,
            post_crop_vignette_style: self.post_crop_vignette_style,
            ..EditRecipe::default()
        }
    }

    /// Returns true if the recipe leaves the image essentially untouched —
    /// useful to detect a "the AI declined to edit" no-op result.
    pub fn is_noop(&self) -> bool {
        *self == EditRecipe::default()
            // Ignore provenance fields when judging "did it actually edit?".
            // base_curve is the photo's camera-matched BASE (stamped on open),
            // not a user edit — a fresh-open recipe must still count as no-op.
            // The lens profile is the same kind of calibration WHILE it stays
            // exactly as stamped (every available component on); a user toggle
            // away from that is a real edit and must keep the recipe non-noop.
            || EditRecipe {
                rationale: String::new(),
                confidence: 0.0,
                // The era stamp is provenance, not an edit: a recipe saved by
                // an older build is still "no edits" and must still clear
                // rather than pin a permanent edited badge.
                version: EditRecipe::default().version,
                // Same rule for the coordinate-frame stamp: a legacy recipe
                // whose only difference from neutral is "written before the
                // orientation fix" holds no user edit, and counting it as one
                // would make a neutral legacy recipe.json outrank the XMP
                // beside it (`SavedDevelop::NoopOnly` precedence).
                coord_era: EditRecipe::default().coord_era,
                // …and for the control-set stamp. "This recipe predates the
                // R25 keys" is provenance about the SCHEMA, not an edit: a
                // neutral v0.30 recipe.json must still clear the ● and must
                // still lose the `SavedDevelop::NoopOnly` precedence contest
                // to a sidecar that holds real edits.
                schema_era: EditRecipe::default().schema_era,
                base_curve: Vec::new(),
                // The as-shot WB anchor is the same kind of stamped
                // calibration — a fresh-open stamp must still count as no-op.
                as_shot_k: None,
                as_shot_tint: None,
                    lens_profile: if self.lens_profile.is_as_stamped() {
                    LensProfile::default()
                } else {
                    self.lens_profile.clone()
                },
                // The pass-through blocks are PROVENANCE too (R25 B4), and
                // this exclusion is load-bearing rather than tidy. Lightroom
                // writes the whole Perspective block plus `crs:CameraProfile`
                // onto every file it has so much as looked at, whether or not
                // the photographer ever opened Transform — all seven reference
                // sidecars carry nine of these keys and six of the seven carry
                // the Perspective block entirely at rest — so counting them as
                // an edit would make an UNTOUCHED Lightroom sidecar out-rank the
                // photographer's own stored develop (`SavedDevelop`
                // precedence, gui/persist.rs) and blank their canvas on
                // open. Nothing in this app can set the map either: it is
                // filled by reading a document and by nothing else, so it can
                // never be the thing the user changed. The sidecar keeps its
                // block regardless — `xmp::merge_strip_keys` never strips a
                // key the recipe in hand does not carry.
                passthrough: std::collections::BTreeMap::new(),
                ..self.clone()
            } == EditRecipe::default()
    }
}

/// [`EditRecipe::temper`]'s shipped soft-cap knees and ceilings — the values
/// bd3f9d4 tuned, named so the strength axis scales ONE definition instead of
/// eight literals. `GradeStrength::CALIBRATED` reproduces exactly these.
pub const TEMPER_TONE_KNEE: f32 = 50.0; // Highlights / Shadows
pub const TEMPER_TONE_CEIL: f32 = 70.0;
pub const TEMPER_POINT_KNEE: f32 = 30.0; // Whites / Blacks
pub const TEMPER_POINT_CEIL: f32 = 45.0;

/// Which of the three strength BANDS a [`GradeStrength`] falls in — the coarse
/// dial the prompt/verifier/judge templates switch on.
///
/// Prose cannot be interpolated the way a number can, so the wording is banded
/// while every NUMBER on the axis stays continuous. Consequence worth knowing:
/// the calibration point (0.50) and the shipped default (0.65) share the
/// `Balanced` band, so they differ in the guardrail NUMBERS the prompt quotes
/// and in `temper`'s knees, not in the adjectives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrengthTier {
    /// ≤ 0.4 — the shipped-since-f944ef3 restraint prose, verbatim.
    Restrained,
    /// 0.4 … 0.7 — confident: the model must decide about each look control
    /// rather than default it to neutral.
    Balanced,
    /// Above 0.7 — committed: the reference becomes a floor rather than a
    /// ceiling, and only BROKEN data (not strength) counts as over-cooked.
    Committed,
}

/// How closely the proposer should follow the photographer's free-text
/// direction. This is prompt intent only; it never changes render bounds.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct DirectionAdherence(f32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdherenceTier {
    Hint,
    Direct,
    Brief,
}

impl AdherenceTier {
    /// The tier's WIRE spelling — one place, because it is written into two
    /// different outward things: the deliverable's `adherence` field and the
    /// verifier's prompt. Those used to spell it independently (a `match` in
    /// `pipeline` and a `{:?}` in `advisor`), which meant renaming a variant
    /// silently rewrote a paid model prompt while the sibling literal beside
    /// it stayed behind.
    ///
    /// The lowercase forms are the deliverable's; `Debug` is what the prompt
    /// carried, so [`Self::prompt_name`] keeps that capitalisation byte for
    /// byte rather than changing what a model reads.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hint => "hint",
            Self::Direct => "direct",
            Self::Brief => "brief",
        }
    }

    /// The tier as the verifier's prompt names it (capitalised, as `{:?}` was).
    pub fn prompt_name(self) -> &'static str {
        match self {
            Self::Hint => "Hint",
            Self::Direct => "Direct",
            Self::Brief => "Brief",
        }
    }
}

impl DirectionAdherence {
    pub const DEFAULT: f32 = 0.65;
    pub const TIER_LOW_MAX: f32 = GradeStrength::TIER_LOW_MAX;
    pub const TIER_MID_MAX: f32 = GradeStrength::TIER_MID_MAX;

    pub fn new(v: f32) -> Self {
        Self(if v.is_finite() { v.clamp(0.0, 1.0) } else { Self::DEFAULT })
    }

    /// Resolve a dial a surface may not have sent — the web body's absent
    /// `adherence`. The same one definition [`GradeStrength::from_optional`]
    /// exists for: an omitted field is the default, never 0.
    pub fn from_optional(v: Option<f32>) -> Self {
        v.map(Self::new).unwrap_or_default()
    }

    pub fn get(self) -> f32 { self.0 }
    pub fn pct(self) -> f32 { self.0 * 100.0 }
    pub fn tier(self) -> AdherenceTier {
        if self.0 <= Self::TIER_LOW_MAX {
            AdherenceTier::Hint
        } else if self.0 <= Self::TIER_MID_MAX {
            AdherenceTier::Direct
        } else {
            AdherenceTier::Brief
        }
    }
}

impl Default for DirectionAdherence {
    fn default() -> Self { Self(Self::DEFAULT) }
}

/// How COMMITTED an AI-proposed develop should be — the app's strength axis
/// (R23-3, feedback #5: "the AI is too timid, and I want a strength slider").
///
/// Deliberately **not** a field of [`EditRecipe`]: strength is the user's
/// INTENT for one analysis, not a develop parameter. In the recipe it would
/// (a) have to be projected into the Lightroom XMP contract, which has no such
/// notion, and (b) change `store::recipe_norm`'s structural fingerprint — the
/// R21 deleted-version registry documents that a schema drift there fails OPEN,
/// so every already-recorded deletion would silently lose its structure arm.
///
/// SIX gates in this app decide "how hard to push", and every one of them was a
/// hard-coded constant. This value drives all six — the proposer prompt
/// (`advisor::proposer`), [`EditRecipe::temper`], the verifier's two-sided band
/// (`advisor::build_verify_prompt`), the visual judge's rubric
/// (`advisor::judge`), the style-reference wording (`style::render_reference`)
/// and the no-AI fallback (`advisor::heuristic`). Missing ONE gate cancels the
/// axis: a bolder proposal that the verifier then calls over-cooked comes back
/// exactly as timid as before, which is why each module carries its own gate
/// test.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct GradeStrength(f32);

impl GradeStrength {
    /// The point every restraint NUMBER in the app was tuned at (the 147-photo
    /// eval of f944ef3, plus bd3f9d4's highlight-integrity cases). At this value
    /// the prompt quotes ±50/±35 and [`EditRecipe::temper`] uses the shipped
    /// 50→70 / 30→45 knees, bit for bit.
    ///
    /// NOT a "behave like the last release" switch, and calling it one was this
    /// axis's single dishonest claim. Two different axes run through here. The
    /// NUMBERS are reproduced exactly at 0.5. The restraint PROSE every pre-R23
    /// request carried verbatim is now the [`StrengthTier::Restrained`] wording,
    /// i.e. the ≤ 0.4 band — while 0.5 lands in [`StrengthTier::Balanced`]. So no
    /// single value on this dial reproduces a pre-R23 request in full: 0.5 buys
    /// the calibrated numbers, 0.4 the old words (at `soft_cap_factor` 0.93, i.e.
    /// with the numbers already tightened).
    pub const CALIBRATED: f32 = 0.5;
    /// What every surface starts at (user decision 2026-08-17 ⑦: "a bit braver
    /// than today, with one click back to the calibration point").
    pub const DEFAULT: f32 = 0.65;
    /// Upper edge of [`StrengthTier::Restrained`] (inclusive).
    pub const TIER_LOW_MAX: f32 = 0.4;
    /// Upper edge of [`StrengthTier::Balanced`] (inclusive).
    pub const TIER_MID_MAX: f32 = 0.7;
    /// Slope of the [`soft_cap_factor`](Self::soft_cap_factor) line.
    pub const SOFT_CAP_SPREAD: f32 = 0.7;

    /// Clamp into 0..=1. A non-finite value is the shipped default, never 0 —
    /// a NaN slider must not silently mean "maximum restraint".
    pub fn new(v: f32) -> Self {
        Self(if v.is_finite() { v.clamp(0.0, 1.0) } else { Self::DEFAULT })
    }

    /// The calibration point — the pre-R23 guardrail NUMBERS, not the whole
    /// pre-R23 request (see [`Self::CALIBRATED`] for what does and does not come
    /// back at this value).
    pub fn calibrated() -> Self {
        Self(Self::CALIBRATED)
    }

    /// Resolve a dial a surface may not have sent: the CLI's omitted
    /// `--strength`, the web body's absent `grade_strength`. ONE definition of
    /// that decision, because getting it wrong is silent — `Option::unwrap_or`
    /// with a bare `0.0` would make "the client said nothing" mean "be as timid
    /// as possible", on the very dial that exists because the AI was too timid.
    pub fn from_optional(v: Option<f32>) -> Self {
        v.map(Self::new).unwrap_or_default()
    }

    pub fn get(self) -> f32 {
        self.0
    }

    /// 0..=100, for the one-decimal-free `{:.0}%` the prompts quote.
    pub fn pct(self) -> f32 {
        self.0 * 100.0
    }

    pub fn tier(self) -> StrengthTier {
        if self.0 <= Self::TIER_LOW_MAX {
            StrengthTier::Restrained
        } else if self.0 <= Self::TIER_MID_MAX {
            StrengthTier::Balanced
        } else {
            StrengthTier::Committed
        }
    }

    /// How far ABOVE the calibration point, as 0..=1 (0 at or below it). The
    /// ramp every "open the numbers up" formula shares, so the axis can never
    /// tighten a number below its calibrated value: the guardrails were measured
    /// there, and only the bold half of the axis is new.
    pub fn above_calibration(self) -> f32 {
        ((self.0 - Self::CALIBRATED) / (1.0 - Self::CALIBRATED)).clamp(0.0, 1.0)
    }

    /// The multiplier on [`EditRecipe::temper`]'s four knees and ceilings.
    ///
    /// `1 + (s − 0.5) · 0.7`, so:
    ///   * s = 0.5 → 1.0 — the shipped 50→70 / 30→45 knees, bit for bit;
    ///   * s = 1.0 → 1.35 — the widest ceiling (70) becomes 94.5, and `soft_cap`
    ///     ASYMPTOTES to its ceiling, so the output stays ≥ 5.5 inside the ±100
    ///     hard `clamp`. The clamp is a safety bound, never a target, and this
    ///     axis does not touch it;
    ///   * s = 0.0 → 0.65 — knees at 32.5/19.5, i.e. more restrained than any
    ///     release so far, which is what a 0 % strength dial should mean.
    pub fn soft_cap_factor(self) -> f32 {
        1.0 + (self.0 - Self::CALIBRATED) * Self::SOFT_CAP_SPREAD
    }
}

impl Default for GradeStrength {
    /// [`Self::DEFAULT`] — the product default, so a call site that forgets to
    /// thread the axis gets the shipped behaviour rather than the timid one this
    /// round exists to fix. Measurement runs that need the fixed baseline say
    /// [`Self::calibrated`] explicitly (see `eval.rs`).
    fn default() -> Self {
        Self(Self::DEFAULT)
    }
}
