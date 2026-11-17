//! The recipe library: named develops kept OUTSIDE any one photo.
//!
//! One file per recipe, `<store>/recipes/<name>.json`, in the same shape
//! `recipe.json` holds — so `autoshade apply <raw> <library file>` reads one
//! unchanged and the GUI and the CLI share a single format. The store root is
//! [`crate::store::store_root`]'s (`AUTOSHADE_DATA_DIR` moves the library with
//! the develops it was lifted from).
//!
//! A library entry is deliberately NOT the develop of the photo it was saved
//! from: it is the part of that develop that means the same thing on another
//! photograph. [`portable`] is that shaping, and it is the SAME rule the
//! clipboard paste follows (the GUI's paste payload calls it) — the masks,
//! layers and calibration whose pixels belong to one frame are dropped, and
//! the count of what was dropped rides back to the caller for disclosure.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::recipe::{EditRecipe, MaskGeometry};

/// Directory under the store root that holds the library.
pub const DIR_NAME: &str = "recipes";

pub fn library_dir() -> PathBuf {
    library_dir_in(&crate::store::store_root())
}

pub fn library_dir_in(root: &Path) -> PathBuf {
    root.join(DIR_NAME)
}

pub fn entry_path(name: &str) -> PathBuf {
    entry_path_in(&crate::store::store_root(), name)
}

pub fn entry_path_in(root: &Path, name: &str) -> PathBuf {
    library_dir_in(root).join(format!("{name}.json"))
}

/// Is a recipe with this name already in the library?
pub fn exists(name: &str) -> bool {
    entry_path(name).is_file()
}

/// What [`portable`] took OUT of a develop because it belongs to the photo it
/// was made on. Every field is a disclosure the caller is expected to say: a
/// silent drop is how a library entry quietly stops looking like the photo it
/// was saved from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Losses {
    /// Masks whose pixels are the source frame's own (bitmap rasters, and the
    /// reverse-fit zones that carry one).
    pub masks: usize,
    /// The per-photo spatial colour field.
    pub colour_field: bool,
    /// Fill layers — the source frame's own pixels.
    pub layers: usize,
    /// The AI lens blur, whose depth map measures the source scene.
    pub lens_blur: bool,
    /// Crop / straighten, when the caller asked for a look without geometry.
    pub geometry: bool,
}

impl Losses {
    pub fn is_empty(&self) -> bool {
        self.masks == 0
            && !self.colour_field
            && self.layers == 0
            && !self.lens_blur
            && !self.geometry
    }
}

/// `recipe` shaped to travel: the edit, and nothing that belongs to the source
/// photo. `geometry` false also drops crop + straighten (composition rarely
/// transfers between frames).
///
/// The one implementation of this rule: the GUI's clipboard paste computes its
/// foreign payload through here, and so does saving into the library.
pub fn portable(recipe: &EditRecipe, geometry: bool) -> (EditRecipe, Losses) {
    let mut out = recipe.clone();
    let mut losses = Losses::default();
    if !geometry {
        losses.geometry = out.crop.is_some() || out.straighten_deg != 0.0;
        out.crop = None;
        out.straighten_deg = 0.0;
    }
    // Bitmap masks reference per-photo rasters keyed to the SOURCE stem —
    // carried to another photo they point at the wrong file (and classic XMP
    // cannot carry them, so recipe.json and .xmp would disagree).
    //
    // The reverse-fit's sky/land ZONES travel no better, and they are AI masks
    // now (`MaskGeometry::select_sky`), so the variant test alone would carry
    // a stale raster onto a photo it was never segmented for. The distinction
    // is `MaskRole::is_zone`: an IMPORTED Lightroom AI mask carries only
    // intent, and `segment::resolve_ai_masks` re-derives it against whatever
    // photo it lands on, so that one still travels.
    // Four-gradient tiles carry their complete geometry and can travel; a
    // refined Bitmap anywhere in the composition still belongs to its photo.
    let photo_keyed = |m: &crate::recipe::LocalAdjustment| {
        std::iter::once(&m.mask).chain(m.components.iter().map(|c| &c.geometry)).any(|g| {
            matches!(g, MaskGeometry::Bitmap { .. })
                || (m.role.is_zone() && crate::render::geometry_raster_path(g).is_some())
        })
    };
    losses.masks = out.masks.iter().filter(|m| photo_keyed(m)).count();
    out.masks.retain(|m| !photo_keyed(m));
    // The R25 B4 PASS-THROUGH map is PER-DOCUMENT state, not a setting: it is
    // Lightroom's own Transform block and camera profile name read off ONE
    // document. An empty map STRIPS nothing, so every other photo's own block
    // survives untouched in its own sidecar and there is no loss to report.
    out.passthrough.clear();
    // A colour field is PER-PHOTO in the strongest sense a control in this
    // recipe can be: its twelve-by-eight spatial cells are normalised to the
    // frame it was solved on, so cell (3, 1) means "this scene's left-of-centre
    // sky" and nothing at all on another photograph.
    losses.colour_field = out.colour_field.take().is_some();
    // A fill layer is the source frame's own pixels at the source frame's
    // coordinates.
    losses.layers = std::mem::take(&mut out.pixel_layers).len();
    // A lens blur's depth map is the source scene's distances.
    losses.lens_blur = out.lens_blur.take().is_some();
    (out, losses)
}

/// One library row. A file that exists but does not parse is LISTED with its
/// error rather than hidden: a missing row is indistinguishable from a recipe
/// that was never saved, and the user would only find out at apply time.
#[derive(Debug)]
pub struct Stored {
    pub name: String,
    pub modified: Option<SystemTime>,
    /// Masks the stored recipe carries (0 when it does not parse).
    pub masks: usize,
    /// `Some` when the file exists but is not a readable recipe.
    pub error: Option<String>,
}

pub fn list() -> Vec<Stored> {
    list_in(&crate::store::store_root())
}

pub fn list_in(root: &Path) -> Vec<Stored> {
    let dir = library_dir_in(root);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<Stored> = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).is_none_or(|e| !e.eq_ignore_ascii_case("json")) {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else {
            continue;
        };
        let modified = entry.metadata().ok().and_then(|m| m.modified().ok());
        let parsed = crate::store::read_text_capped(&path, crate::store::MAX_STORE_JSON)
            .map_err(|e| e.to_string())
            .and_then(|text| serde_json::from_str::<EditRecipe>(&text).map_err(|e| e.to_string()));
        out.push(match parsed {
            Ok(recipe) => Stored { name, modified, masks: recipe.masks.len(), error: None },
            Err(e) => Stored { name, modified, masks: 0, error: Some(e) },
        });
    }
    out.sort_by_cached_key(|s| (s.name.to_lowercase(), s.name.clone()));
    out
}

pub fn read(name: &str) -> Result<EditRecipe> {
    read_in(&crate::store::store_root(), name)
}

pub fn read_in(root: &Path, name: &str) -> Result<EditRecipe> {
    let path = entry_path_in(root, name);
    let text = crate::store::read_text_capped(&path, crate::store::MAX_STORE_JSON)
        .with_context(|| format!("recipe 「{name}」 could not be read ({})", path.display()))?;
    let mut recipe: EditRecipe = serde_json::from_str(&text)
        .with_context(|| format!("recipe 「{name}」 is not a recipe file ({})", path.display()))?;
    // A hand-imported file may name its rasters, like a store recipe does.
    if let Some(base) = path.parent() {
        crate::store::resolve_mask_paths(&mut recipe, base);
    }
    Ok(recipe)
}

fn write_at(path: &Path, recipe: &EditRecipe) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("the recipe library folder could not be created ({})", dir.display()))?;
    }
    let body = serde_json::to_vec_pretty(recipe).context("the recipe could not be serialised")?;
    crate::store::durable_write(path, &body)
        .with_context(|| format!("the recipe could not be written ({})", path.display()))
}

/// Write a NEW entry. Refuses a taken name: an accidental overwrite of a look
/// the user cannot get back is the one failure this library must not have.
pub fn create(name: &str, recipe: &EditRecipe) -> Result<()> {
    create_in(&crate::store::store_root(), name, recipe)
}

pub fn create_in(root: &Path, name: &str, recipe: &EditRecipe) -> Result<()> {
    let path = entry_path_in(root, name);
    if path.exists() {
        anyhow::bail!("a recipe named 「{name}」 already exists");
    }
    write_at(&path, recipe)
}

/// Replace an EXISTING entry's content, keeping its name.
pub fn replace(name: &str, recipe: &EditRecipe) -> Result<()> {
    replace_in(&crate::store::store_root(), name, recipe)
}

pub fn replace_in(root: &Path, name: &str, recipe: &EditRecipe) -> Result<()> {
    let path = entry_path_in(root, name);
    if !path.is_file() {
        anyhow::bail!("no recipe named 「{name}」 in the library");
    }
    write_at(&path, recipe)
}

/// Rename an entry (the file moves with it). Refuses a taken name.
pub fn rename(from: &str, to: &str) -> Result<()> {
    rename_in(&crate::store::store_root(), from, to)
}

pub fn rename_in(root: &Path, from: &str, to: &str) -> Result<()> {
    let src = entry_path_in(root, from);
    let dst = entry_path_in(root, to);
    if !src.is_file() {
        anyhow::bail!("no recipe named 「{from}」 in the library");
    }
    if dst.exists() {
        anyhow::bail!("a recipe named 「{to}」 already exists");
    }
    std::fs::rename(&src, &dst)
        .with_context(|| format!("the recipe could not be renamed ({})", src.display()))
}

pub fn delete(name: &str) -> Result<()> {
    delete_in(&crate::store::store_root(), name)
}

pub fn delete_in(root: &Path, name: &str) -> Result<()> {
    let path = entry_path_in(root, name);
    std::fs::remove_file(&path)
        .with_context(|| format!("the recipe could not be deleted ({})", path.display()))
}

/// Read a recipe FILE the user picked, `.json` (this app's own recipe shape) or
/// `.xmp` (a Lightroom / Camera Raw sidecar, through the same importer the
/// desktop app runs when it opens a photo).
pub fn read_file(path: &Path) -> Result<EditRecipe> {
    let text = crate::store::read_text_capped(path, crate::store::MAX_STORE_JSON)
        .with_context(|| format!("{} could not be read", path.display()))?;
    let is_xmp = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("xmp"));
    if is_xmp {
        let (recipe, _) = crate::xmp::xmp_to_recipe_clamped(&text);
        Ok(recipe)
    } else {
        serde_json::from_str(&text)
            .with_context(|| format!("{} is not a recipe.json or an .xmp sidecar", path.display()))
    }
}

/// Why a name cannot be a library entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    /// A character the file system (or a path) cannot hold.
    Illegal(char),
}

/// The file-system name a typed recipe name becomes, or why it cannot.
pub fn check_name(raw: &str) -> std::result::Result<String, NameError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(NameError::Empty);
    }
    if name.chars().count() > crate::store::MAX_STORE_NAME {
        return Err(NameError::TooLong);
    }
    if let Some(c) = name
        .chars()
        .find(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
    {
        return Err(NameError::Illegal(c));
    }
    // Windows drops a trailing dot or space from a file name, so two names the
    // user sees as different would land on one file.
    if let Some(c) = name.chars().last().filter(|c| *c == '.' || *c == ' ') {
        return Err(NameError::Illegal(c));
    }
    Ok(name.to_string())
}
