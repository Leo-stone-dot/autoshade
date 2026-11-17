//! Render engine v1 — apply an [`EditRecipe`] to the full-resolution RAW and
//! produce a developed image (no Lightroom needed).
//!
//! Pipeline: `rawler` demosaics + colour-calibrates the sensor data to a
//! full-res sRGB-gamma float image (`RawDevelop::develop_intermediate`), then we
//! apply the recipe. A non-2×2 RGB colour filter array (X-Trans) is the one
//! exception: rawler's demosaic is Bayer-only, so this module demosaics and
//! calibrates that class itself — see [`demosaic_over_cfa_geometry`]. The tonal
//! ops (exposure, contrast, whites/blacks, highlights/shadows, tone curve) are
//! all 1-D functions of a channel value, so they collapse into a single
//! per-channel lookup table; saturation/vibrance run per pixel; then
//! orientation + crop.
//!
//! HONEST SCOPE: these ops are tasteful **approximations**, not bit-exact
//! Lightroom — clarity is a luma unsharp mask, the Detail panel's capture
//! sharpening (v1.5.0) is the law measured against Lightroom 9.4 in v1.6.5 and
//! its luminance and colour noise reduction are built from Adobe's documented
//! slider behaviour, all in `detail.rs`, dehaze is a pointwise
//! scattering inversion (see [`apply_dehaze`]). LOCAL-mask clarity/dehaze/texture ARE engine-rendered
//! since R22 (local temperature/tint since batch #2-B) — see [`apply_masks`]
//! for the pass order and the two documented residues vs the global chain.
//! `texture` gained its GLOBAL stage in R25 B2 and the two share one radius
//! model (0.5% of the short edge, floored at 2 px) — still our own
//! calibration, Adobe's being unpublished, but now one calibration instead of
//! a local-only one with nothing to align against.

#![deny(clippy::print_stderr)]

use std::borrow::Cow;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use image::{DynamicImage, GenericImageView, ImageBuffer, ImageEncoder, Rgb, RgbImage};
use rawler::decoders::RawDecodeParams;
use rawler::get_decoder;
use rawler::imgop::develop::{Intermediate, ProcessingStep, RawDevelop};
use rawler::rawsource::RawSource;
use rawler::Orientation;
use rayon::prelude::*;

use crate::recipe::{Crop, EditRecipe, MaskGeometry, RangeMask};

mod denoise_grain;
mod detail;
mod finish;
mod hdr;
mod lens;
mod perspective;
mod profile;
mod baked;
mod base_look;
mod brush;
mod as_shot;
mod camera;
mod canvas;
mod colour;
mod develop;
mod export;
mod geometry;
mod layers;
mod lens_blur;
mod luma;
mod mask_falloff;
mod mask_raster;
mod mask_warp;
mod mask_weight;
mod mask_gains;
mod masks;
mod orient;
mod resample;
mod source;
mod tone;
mod wb;
pub use detail::FilmScale;
pub use finish::{frame_and_finish, CropPolicy};

const LUT_N: usize = 4096;
pub(crate) const MASK_RASTER_BUDGET_BYTES: usize = 256 * 1024 * 1024;

pub use base_look::{camera_base_knots, camera_base_look, camera_frame_of};
use brush::{brush_raster, brush_token_num, rasterise_brush_group};
pub use as_shot::as_shot_wb;
use camera::{
    calibrate_camera_buffer, camera_matrix, normalise_wb, resolve_camera_profile, space_primaries,
    validate_calibration, CameraMeta,
};
pub use colour::{point_color_at, point_color_rgb, point_color_sampling_recipe};
pub(crate) use colour::{
    HSL_CENTERS, HSL_HUE_REACH_DEG, WHEEL_SAT_REACH, bracket_bands, chroma, color_grade_regions, hsl_band_reach, rgb_to_hsl,
    wheel_tint,
};
use colour::{
    CHROMA_GATE, apply_calibration, apply_color_grade, apply_gray_mix, apply_hsl,
    apply_point_colors, apply_sat_vibrance, hsl_to_rgb, to_u16, to_u8,
};
pub use develop::{
    develop_preview, develop_preview_film, develop_preview_framed, develop_preview_with,
    retouch_spots,
};
pub(crate) use develop::{apply_colour_field, field_axis, field_guide_luma};
use develop::{apply_develop_with_rasters, dehaze_airlight, dehaze_px, luma601};
pub use export::{
    ExportColorSpace, ExportOpts, convert_export_color_space, render_master_to_file, render_to_file,
    stage_and_publish,
};
pub(crate) use export::{SRGB_ICC, mat3_from_slice, write_working_space};
use export::{ADOBE_PRIM, D65_XY, P3_PRIM, SRGB_PRIM, inv3, mat_mul3, mat_vec3, rgb_to_xyz};
pub use geometry::{
    MANUAL_CA_PER_UNIT, apply_lens_distortion, apply_lens_geometry, apply_lens_geometry_rgba,
    corrected_to_sensor_frame, distort_norm, geometry_moves_frame, geometry_profile, lens_geom_norm,
    lens_ungeom_norm, rotate_straighten_rgba, tail_lens, undistort_norm,
};
pub(crate) use geometry::{profile_knot_interp};
use geometry::channel_luts;
use geometry::{profile_fill_scale};
pub(crate) use luma::{bilinear_plane, gauss_blur_plane, neighbours4};
// The zoned fit's detail stage reads the two bands the local Texture and
// Clarity passes move, with this engine's own blur and radii.
pub(crate) use luma::{blur_plane, clarity_radius, texture_radius};
use luma::{
    box_blur_h, box_blur_v, scale_chroma, texture_pass, unsharp_luma,
    unsharp_luma_weighted, write_luma_additive, write_luma_weighted,
};
use mask_falloff::{LINEAR_FALLOFF, linear_coverage, radial_falloff, shaped_coverage};

/// The coverage a linear gradient mask gives at `t` along its handles (0 at
/// the Zero handle, 1 at Full), the measured Lightroom profile the renderer
/// paints — so a caller modelling a ramp before rendering it models the ramp
/// that will be painted.
pub(crate) fn linear_ramp_coverage(t: f32) -> f32 {
    linear_coverage(t, LINEAR_FALLOFF)
}

/// A [`MaskGeometry::Shaped`] ramp's coverage at handle parameter `t`
/// ([`mask_falloff::shaped_coverage`]) — the same table the renderer reads,
/// for a caller modelling the ramp before rendering it.
pub(crate) fn shaped_ramp_coverage(t: f32, falloff: &[f32]) -> f32 {
    mask_falloff::shaped_coverage(t, falloff)
}
pub use mask_raster::{
    dead_bitmap_rasters, feather_mask, geometry_raster_path, geometry_raster_path_mut,
    is_raster_backed, mask_from_memory_bounded, mask_raster_write_fits_budget, morph_mask,
    open_mask_bounded, refine_mask_guided,
};
pub(crate) use mask_raster::{sample_gray_norm};
pub use layers::PixelLayer;
pub use lens_blur::LensBlur;
use mask_raster::{
    MaskBitmap, MaskRasterSnapshot, best_effort_mask_raster_snapshot, load_mask_bitmap,
    load_mask_raster_snapshot,
};
pub use mask_warp::{
    lr_mask_unwarp_norm, lr_mask_warp_norm, mask_warp_factor, mask_warp_from_camera_knots,
    original_to_view_norm, view_to_original_norm,
};
pub use mask_weight::{
    chromaticity_distance, colour_range_amount, colour_range_tolerance, mask_coverage,
    preview_mask_coverage, range_weight,
};
pub(crate) use mask_weight::{MASK_SAMPLE_CENTRE};
use mask_weight::{Bmp, combined_mask_weight};
pub use masks::{MaskFrame, corrected_raster_to_sensor, engine_active, sensor_raster_to_corrected, sensor_rgba_to_corrected};
pub(crate) use masks::{linear_to_oklab, tint_from_push, tint_luma, tint_window};
use masks::{MaskUnwarp, apply_masks};
pub use orient::{
    CoordFrame, compose_orientation, orient_point, orient_recipe_coords, orient_vector,
    quarter_turn_orientation, quarter_turns_between, recipe_has_brush_strokes,
    recipe_has_frame_coords, shift_luma_raster, shift_recipe_coords, turn_image,
};
pub(crate) use orient::{oriented};
pub use resample::{inscribed_dims, rotate_straighten, thumbnail, thumbnail_exact};
use resample::{
    downscale_f32, flip_h_in_place, orient_f32, sample_bilinear_ch, sample_bilinear_rgb16,
};
pub use source::{render_baked_to_image, render_to_image, render_to_image_in, source_pixels};
pub use canvas::{
    base_develop, master_develop, master_pixels_for, source_pixels_for, source_plate, plate_corrected, BasePart,
    Master, Plate,
};
pub use baked::{
    carry_baked_geometry, fit_raster_frame, open_mask_raster, read_baked_geometry, save_mask_raster,
    stamp_baked_geometry, stamp_mask_frame, BakedGeometry, MaskRaster, RasterFrame, SourceFrame,
    BAKED_GEOMETRY_KEY, BAKED_GEOMETRY_TIFF_TAG,
};
use source::{apply_crop, render_frame, rgb16_source};
pub use tone::curve_lut;
pub(crate) use tone::{
    TONE_KNOTS_X, build_tone_lut, sample_lut, sample_tone_model, tone_exposure_curve,
    tone_knot_weights, tone_model_knots, tone_slider_basis,
};
use tone::{apply_rgb_curves, engine_channel_curves, hermite_eval};
pub(crate) use mask_gains::gains_as_white;
pub use wb::{local_temp_to_kelvin, solve_wb_for, solve_wb_from_neutral};
pub(crate) use wb::{WbLaw, linear_to_srgb, srgb_to_linear, wb_gains};
use wb::{
    apply_recipe_wb, colour_gain_luts, decode_wide, encode_wide, ramp, smoothstep, transfer_luts,
};
