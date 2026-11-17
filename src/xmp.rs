//! XMP sidecar writer — render an [`EditRecipe`] as an Adobe Camera Raw /
//! Lightroom `.xmp` sidecar (the `crs:` namespace), so the AI's edit opens as
//! adjustable develop sliders in the user's catalog.
//!
//! Key names, value conventions, and structure were verified against a real ACR
//! sidecar from the user's own library (`P28.xmp`): `ProcessVersion=15.4`,
//! signed-integer sliders, `Sharpness` on 0..150, tone curve as an `rdf:Seq` of
//! `"x, y"` strings (see `docs/M1_PLAN.md` §5 and §9). We emit only the keys we
//! set; Lightroom fills the rest from defaults.

use crate::recipe::{
    BrushStroke, ColorGrade, Crop, CurvePoint, EditRecipe, Hsl, LocalAdjustment, MaskCombine,
    MaskComponent, MaskGeometry, RangeMask,
};

const MAX_XMP_BYTES: usize = 16 * 1024 * 1024;
mod brush_table;
mod crs_source;
mod frame;
mod geometry;
mod losses;
mod mask_xml;
mod merge;
mod read;
mod read_corrections;
mod read_masks;
mod scan;
mod write;
mod xml;

use brush_table::{MaskBrushError, MaskBrushReader};
pub use crs_source::{unparsable_crs_numbers};
pub(crate) use crs_source::{CrsSource, Scope, Tag};
use crs_source::{
    block_between, parse_curve, parse_curve_checked, parse_point_colors, parse_point_colors_checked,
    xml_unescape,
};
pub use frame::{FrameAspect};
use frame::{
    FrameScope, LR_MASK_FRAME_SCALE, declared_orientation, frame_fallback,
    honour_declared_orientation, photo_frame_fallback,
};
pub use geometry::{crop_import_note, crop_import_note_for_photo};
use geometry::{
    CropDecode, EngineRadial, LrRadial, RadialDecode, engine_to_lr, engine_to_lr_crop, lr_to_engine,
    read_crop,
};
pub use losses::{
    MaskImportLoss, MaskImportReason, MaskLoss, MaskLossReason, PASSTHROUGH_CRS,
    describe_global_losses, describe_import_losses, describe_mask_losses, global_export_losses,
    global_render_gaps,
    import_losses, import_losses_for_photo, lens_profile_enabled, mask_export_losses,
    LensProfileSetup, approximate_focus_distance, lens_profile_filename, lens_profile_name, lens_profile_setup,
    unmodelled_global_crs,
};
pub(crate) use losses::{CALIBRATION_CRS, OWNED_ELEMENT_ONLY, SDR_CRS};
use losses::{read_creative_look, upright_matrices};
use mask_xml::{
    AI_MASK_PROVENANCE_KEYS, MASK_INTENT_URI, ai_mask_xml, ai_mask_xml_spelled, brush_mask_xml,
    brush_mask_xml_spelled, combine_name, combine_spelling, geometry_inversion, guid,
    intent_namespace_declared, lr_net_inverted, mask_geom_xml, mask_intent_attr, projected_combine,
    range_mask_xml,
};
pub use merge::{
    MergeOutcome, merge_recipe_into_xmp, merge_recipe_into_xmp_in_frame,
    merge_recipe_into_xmp_in_frame_for_photo,
};
use merge::{unspoken_attr_keys};
pub use read::{
    xmp_to_recipe, xmp_to_recipe_clamped, xmp_to_recipe_clamped_with_diag, xmp_to_recipe_for_photo,
    xmp_to_recipe_with_diag,
};
use read::{is_autoshade_sidecar, read_document, upgrade_era_marker};
use read_corrections::{classify_correction, parse_paint_stroke};
pub use read_masks::{
    MaskBrushTableRefusal, unsupported_corrections, unsupported_corrections_for_photo,
};
use read_masks::{
    MaskCorrectionParse, component_import_reasons, correction_value_reasons, mask_summary,
    mask_summary_with_source, parse_masks_with_source, parse_retouch_areas,
};
pub(crate) use scan::{crs_own_scope, owned_element_body};
use scan::{
    CONSTRUCTS, XmlComponent, component_body, components_in, correction_own_scope,
    element_close_start, find_crs_description, find_matching_close, next_xml_attribute,
    next_xml_tag, owned_element_body_span, refresh_rationale_comment, scan_tag_end, tag_name,
    top_level_owned_spans, xml_attribute_raw, xmlns_conflict,
};
pub use write::{
    recipe_to_xmp, recipe_to_xmp_in_frame, recipe_to_xmp_in_frame_for_photo,
    recipe_to_xmp_with_losses,
};
pub(crate) use write::{bare_document, owned_attr_keys};
use write::{
    find_outside_constructs, frame_declaration, in_source_frame, insert_crs_description, masks_xml,
    owned_attrs, owned_children, safe_rationale, written_name,
};
use xml::{attr, local_fmt, signed, xml_attr_escape, xml_char_allowed, xml_text_escape};






/// The AutoShade payload: the whole develop under this app's own namespace,
/// which a Lightroom rewrite preserves (v1.3.1) — see the module's own docs.
mod payload;
pub use payload::written_lens_profile;
