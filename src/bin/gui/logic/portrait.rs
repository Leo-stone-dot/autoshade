//! The Portraits panel's worker data, the skin-switch slider boundary, and the
//! verbs the view asks for. The panel draws; `portrait_actions.rs` decides.

use std::path::PathBuf;

/// A slider's reach: the guardrail's limit while the switch is on, the field's
/// own travel when it is off.
pub(crate) fn reach(guarded: bool, limit: f32, free: f32) -> f32 {
    if guarded { limit } else { free }
}

/// What the find step found, as facts for the landing (L12#4: worded in the
/// language live when it lands).
pub(crate) struct PortraitFind {
    /// The photo it ran on: a landing for another photo is dropped.
    pub(crate) photo: PathBuf,
    /// The subject matte's raster, or why there is none.
    pub(crate) subject: Result<PathBuf, String>,
    /// The face finder's answer: `Ok(None)` = it ran and found no face.
    pub(crate) faces: Result<Option<FoundFaces>, String>,
}

/// The faces of one find step: how many, the two rasters, and the skin colour
/// seed (`face::skin_seed`; `None` when the face holds no pixel).
pub(crate) struct FoundFaces {
    pub(crate) n: usize,
    pub(crate) face: PathBuf,
    pub(crate) eyes: PathBuf,
    pub(crate) seed: Option<([f32; 3], [f32; 2])>,
}

/// What the Portraits panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortraitAction {
    /// Find the people/subject and their faces (a worker; its masks land as a
    /// recipe change).
    FindPeople,
    /// Remove blemishes on the detected face (the Retouch heal, held to it).
    RemoveBlemishes,
    /// "Keep the person sharp": the blur protects the subject matte, or nothing.
    KeepPersonSharp(bool),
    /// "Keep skin natural" flips for the open card.
    SetKeepSkin(bool),
}
