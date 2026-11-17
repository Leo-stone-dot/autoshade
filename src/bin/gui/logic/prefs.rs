//! The persisted-dial laws: the clamps a prefs file goes through on restore and
//! the era migration for the shared/still-single denoise dial. Pure data in,
//! pure data out — the app applies the result.

/// The one known preview edges (px): anything else falls back to the default.
pub(crate) const KNOWN_PREVIEW_EDGES: [u32; 3] = [1280, 2560, 4096];

pub(crate) fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

pub(crate) fn clamp_quality(v: f32) -> f32 {
    v.clamp(1.0, 100.0)
}

pub(crate) fn clamp_sharpen(v: f32) -> f32 {
    v.clamp(0.0, 100.0)
}

/// A three-choice dial (quality): anything past the last choice lands on it.
pub(crate) fn choice(v: usize) -> usize {
    v.min(2)
}

/// The delivery color space: only the known codes survive, the rest fall to sRGB (0).
pub(crate) fn known_space(v: u8) -> u8 {
    if v <= 2 { v } else { 0 }
}

/// A working-preview edge: unknown steps fall back to the fluid default.
pub(crate) fn known_preview_edge(v: u32) -> u32 {
    if KNOWN_PREVIEW_EDGES.contains(&v) { v } else { 1280 }
}

/// What an old prefs file becomes under the current era. `baked` is set by the
/// era-1 migration only; `reset_note` carries the old RAW values when they were
/// not already the new default (the caller words the note).
#[derive(Debug, PartialEq)]
pub(crate) struct MigratedDenoise {
    pub raw_detail: f32,
    pub raw_export: f32,
    pub baked_detail: Option<f32>,
    pub baked_export: Option<f32>,
    pub reset_note: Option<(f32, f32)>,
}

/// Advance one saved dial file to the current era.
///
/// Era 0 wrote one RAW strength and no baked one: era 1's law is the identity
/// (the raw dial is restored as the historical 1.0 by the caller's hop).
/// Era 1 shared ONE dial between RAW and SCUNet: it cannot say which source a
/// file used, so the choice is preserved for SCUNet (baked) and only RAW is
/// reset to the new default — with a note when the old values differed.
pub(crate) fn migrate_era(era: u32, raw_detail: f32, raw_export: f32) -> MigratedDenoise {
    let default = autoshade::denoise::DEFAULT_STRENGTH_RAW;
    match era {
        0 => MigratedDenoise {
            raw_detail: 1.0,
            raw_export: 1.0,
            baked_detail: None,
            baked_export: None,
            reset_note: None,
        },
        1 => MigratedDenoise {
            raw_detail: default,
            raw_export: default,
            baked_detail: Some(clamp01(raw_detail)),
            baked_export: Some(clamp01(raw_export)),
            reset_note: (raw_detail != default || raw_export != default).then_some((raw_detail, raw_export)),
        },
        _ => MigratedDenoise {
            raw_detail,
            raw_export,
            baked_detail: None,
            baked_export: None,
            reset_note: None,
        },
    }
}
