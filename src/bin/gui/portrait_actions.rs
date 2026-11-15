//! Portraits' business: finding the people/subject (the worker), landing its
//! masks, the skin-limit switch and its per-card bookkeeping, the AI blemish
//! removal start, and the depth landing.
//!
//! The panel draws and emits verbs; this app layer decides and spawns.

use crate::*;
use crate::logic::portrait::{FoundFaces, PortraitFind};
use crate::panels::portrait::PortraitLine;
use autoshade::face::{face_raster, role_mask, subject_raster};
use autoshade::recipe::{LocalAdjustment, MaskRole};

pub(crate) fn find_people(
    original: &image::DynamicImage,
    base: &image::DynamicImage,
    lens: Option<&autoshade::recipe::LensProfile>,
    photo: &std::path::Path,
    recipe: &EditRecipe,
    film: Option<u32>,
    raw_source: bool,
) -> PortraitFind {
    let cfg = autoshade::config::Config::load();
    let staged = match crate::masks::stage_preview_png(original, "portrait") {
        Ok(p) => p,
        Err(e) => {
            let why = format!("{e:#}");
            return PortraitFind { photo: photo.to_path_buf(), subject: Err(why.clone()), faces: Err(why) };
        }
    };
    let subject = (|| -> anyhow::Result<PathBuf> {
        let opts = autoshade::segment::SegmentOpts::from_config(&cfg, "subject");
        let mask = autoshade::store::claim_raster(photo, "mask-subject")?;
        autoshade::segment::segment_file(&opts, &staged, &mask).inspect_err(|_| {
            // A failed run releases its claimed (0-byte) name.
            let _ = std::fs::remove_file(&mask);
        })?;
        Ok(mask)
    })()
    .map_err(|e| format!("{e:#}"));
    let json = staged.with_extension("json");
    let faces = (|| -> anyhow::Result<Option<FoundFaces>> {
        let report = autoshade::face::detect_faces(&cfg, &staged, &json)?;
        if report.faces.is_empty() {
            return Ok(None);
        }
        let (w, h) = (original.width(), original.height());
        let face_mask = autoshade::face::face_mask(&report, w, h);
        let eyes_mask = autoshade::face::eyes_mask(&report, w, h);
        let seed = panel_skin_seed(base, &face_mask, recipe, film, raw_source, lens);
        let face = autoshade::face::save_mask(&face_mask, photo, "mask-face")?;
        let eyes = autoshade::face::save_mask(&eyes_mask, photo, "mask-eyes")?;
        Ok(Some(FoundFaces { n: report.faces.len(), face, eyes, seed }))
    })()
    .map_err(|e| format!("{e:#}"));
    let _ = std::fs::remove_file(&staged);
    let _ = std::fs::remove_file(&json);
    PortraitFind { photo: photo.to_path_buf(), subject, faces }
}

/// The skin seed the find step writes: `face::skin_seed` on the pixels the
/// skin mask's colour range is matched on — `base` developed with `recipe`'s
/// masks before the skin mask (all of them when there is none yet), geometry
/// stripped (`face::skin_seed_reference`, the eyedropper's convention).
/// `face_mask` is in the original frame; on a lens-corrected base (`lens`,
/// v1.17.0) it is read into the base's frame first.
pub(crate) fn panel_skin_seed(
    base: &image::DynamicImage,
    face_mask: &image::GrayImage,
    recipe: &EditRecipe,
    film: Option<u32>,
    raw_source: bool,
    lens: Option<&autoshade::recipe::LensProfile>,
) -> Option<([f32; 3], [f32; 2])> {
    let before = recipe.masks.iter().position(|m| m.role == MaskRole::Skin).unwrap_or(recipe.masks.len());
    let reference = autoshade::face::skin_seed_reference(base, recipe, before, film, raw_source, lens);
    match lens {
        Some(p) => {
            autoshade::face::skin_seed(&reference, &autoshade::render::sensor_raster_to_corrected(face_mask, p))
        }
        None => autoshade::face::skin_seed(&reference, face_mask),
    }
}

impl AutoShadeApp {

pub(crate) fn start_find_people(&mut self) {
        if self.workspace.busy {
            return;
        }
        let (Some(base), Some(photo)) = (self.editor.photo().base_preview.clone(), self.editor.photo().src_path.clone()) else { return };
        // The detectors see the ORIGINAL frame the masks are stored in; the
        // skin seed is sampled on the canvas's own base (v1.17.0: a corrected
        // one, with the face mask read into its frame).
        let Some(original) = self.original_frame_base() else { return };
        let lens = self.base_corrected().then(|| autoshade::render::geometry_profile(&self.editor.doc().recipe).into_owned());
        let recipe = self.editor.doc().recipe.clone();
        let film = self.film_short_edge();
        let raw_source = self.source_is_raw();
        self.workspace.busy = true;
        self.workspace.portrait.line = PortraitLine::Finding;
        self.workspace.status = tr(self.lang, K_ACTIONS_FINDING_THE_PEOPLE_AND_FACES_THE_FIRST_RUN_DOWNL).into();
        self.spawn_worker(
            move || {
                Msg::PortraitFound(Box::new(find_people(&original, &base, lens.as_ref(), &photo, &recipe, film, raw_source)))
            },
            |e| Msg::PortraitFound(Box::new(PortraitFind { photo: PathBuf::new(), subject: Err(e.to_string()), faces: Err(e.to_string()) })),
        );
    }

pub(crate) fn on_portrait_found(&mut self, lang: Lang, found: PortraitFind) {
        self.workspace.busy = false;
        if self.editor.photo().src_path.as_deref() != Some(found.photo.as_path()) && !found.photo.as_os_str().is_empty() {
            // Another photo is open now: its masks are not this one's.
            self.workspace.portrait.line = PortraitLine::Idle;
            return;
        }
        let mut fresh: Vec<LocalAdjustment> = Vec::new();
        let mut faces_n = 0;
        if let Ok(None) = &found.faces {
            // No face this time: the face, eyes and skin masks of an earlier
            // search stood on a face that is not found — left in place they
            // keep acting with their sliders while the groups say "no face".
            self.editor.doc_mut().recipe.masks.retain(|m| !matches!(m.role, MaskRole::Face | MaskRole::Eyes | MaskRole::Skin));
        }
        if let Ok(Some(f)) = &found.faces {
            faces_n = f.n;
            let face = f.face.to_string_lossy().into_owned();
            fresh.push(autoshade::face::portrait_adjustment(MaskRole::Face, face.clone()));
            fresh.push(autoshade::face::portrait_adjustment(MaskRole::Eyes, f.eyes.to_string_lossy().into_owned()));
            if let Some((seed, at)) = f.seed {
                fresh.push(autoshade::face::skin_adjustment(face, seed, at));
            }
        }
        if let Ok(subject) = &found.subject {
            let subject = subject.to_string_lossy().into_owned();
            fresh.push(autoshade::face::background_adjustment(subject.clone()));
            // A blur that protects the person follows the new matte.
            if let Some(p) = self.editor.doc_mut().recipe.lens_blur.as_mut().and_then(|lb| lb.protect.as_mut()) {
                *p = subject;
            }
        }
        for m in fresh {
            match role_mask(&mut self.editor.doc_mut().recipe, m.role) {
                Some(old) => {
                    old.mask = m.mask;
                    if m.range.is_some() {
                        old.range = m.range;
                    }
                }
                None => self.editor.doc_mut().recipe.masks.push(m),
            }
        }
        let failed = match (&found.subject, &found.faces) {
            (Err(a), Err(b)) => {
                let t = trf(lang, K_ACTIONS_COULD_NOT_FIND_THE_PEOPLE, &[("err", &format!("{a}; {b}"))]);
                self.workspace.portrait.line = PortraitLine::Failed(t.clone());
                self.workspace.status = t.clone();
                self.toast(ToastKind::Error, t);
                return;
            }
            (Err(e), _) | (_, Err(e)) => Some(e.clone()),
            _ => None,
        };
        if found.faces.is_ok() {
            self.workspace.portrait.found = Some((found.photo.clone(), faces_n));
        }
        self.workspace.portrait.line = PortraitLine::Found { faces: faces_n, failed: failed.clone() };
        self.workspace.overlay.overlay_stale = true;
        self.editor.session_mut().dirty = true;
        self.workspace.status = self.portrait_line_text(lang);
        if let Some(why) = failed {
            self.toast(ToastKind::Error, trf(lang, K_ACTIONS_PART_OF_THE_SEARCH_DID_NOT_WORK, &[("err", &why)]));
        }
    }

pub(crate) fn start_remove_blemishes(&mut self) {
        if let Some(face) = face_raster(&self.editor.doc().recipe) {
            self.start_heal(false, Some(face));
        }
    }

pub(crate) fn guard_portrait_skin(&mut self) {
        if self.skin_held() && autoshade::face::keep_skin_natural_in(&mut self.editor.doc_mut().recipe) {
            self.editor.session_mut().dirty = true;
        }
    }

pub(crate) fn skin_held(&self) -> bool {
        self.keep_skin_on()
    }

pub(crate) fn skin_held_for(&self, photo: &std::path::Path) -> bool {
        if self.editor.photo().src_path.as_deref() == Some(photo) {
            return self.skin_held();
        }
        self.workspace.open.nav_stash.get(photo).is_none_or(|st| {
            self.workspace.portrait.keep_skin.get(&(photo.to_path_buf(), st.id.clone())).copied().unwrap_or(true)
        })
    }

pub(crate) fn release_skin_for_history(&mut self) {
        if self.skin_held()
            && autoshade::face::skin_values_beyond(&self.editor.doc().recipe) > 0
            && let Some(key) = self.skin_key()
        {
            self.workspace.portrait.keep_skin.insert(key, false);
        }
    }

pub(crate) fn skin_switch(&self) -> Option<bool> {
        self.skin_key().map(|_| self.keep_skin_on())
    }

pub(crate) fn restore_skin_switch(&mut self, on: Option<bool>) {
        if let (Some(on), Some(key)) = (on, self.skin_key()) {
            self.workspace.portrait.keep_skin.insert(key, on);
        }
    }

fn skin_key(&self) -> Option<(PathBuf, String)> {
        let photo = self.editor.photo().src_path.clone()?;
        Some((photo, self.editor.doc().variants.get(self.editor.doc().active).map(|v| v.id.clone()).unwrap_or_default()))
    }

pub(crate) fn keep_skin_on(&self) -> bool {
        self.skin_key().and_then(|k| self.workspace.portrait.keep_skin.get(&k).copied()).unwrap_or(true)
    }

pub(crate) fn set_keep_skin(&mut self, on: bool) {
        let Some(key) = self.skin_key() else { return };
        self.workspace.portrait.keep_skin.insert(key, on);
        if !on {
            return;
        }
        let n = autoshade::face::skin_values_beyond(&self.editor.doc().recipe);
        if n > 0 && autoshade::face::keep_skin_natural_in(&mut self.editor.doc_mut().recipe) {
            self.editor.session_mut().dirty = true;
            self.workspace.status = if n == 1 {
                tr(self.lang, K_ACTIONS_SKIN_KEPT_NATURAL_1_VALUE_BROUGHT_BACK_WITHIN_TH).into()
            } else {
                trf(self.lang, K_ACTIONS_SKIN_KEPT_NATURAL_VALUES_BROUGHT_BACK_WITHIN_THE, &[("n", &n.to_string())])
            };
        }
    }

pub(crate) fn decide_keep_skin_for_photo(&mut self) {
        let Some(photo) = self.editor.photo().src_path.clone() else { return };
        self.workspace.portrait.keep_skin.retain(|(p, _), _| *p != photo);
        self.switch_off_cards_past_limits();
    }

fn switch_off_cards_past_limits(&mut self) {
        let Some(photo) = self.editor.photo().src_path.clone() else { return };
        for (i, v) in self.editor.doc().variants.iter().enumerate() {
            let recipe = if i == self.editor.doc().active { &self.editor.doc().recipe } else { &v.recipe };
            if autoshade::face::skin_values_beyond(recipe) > 0 {
                self.workspace.portrait.keep_skin.insert((photo.clone(), v.id.clone()), false);
            }
        }
    }

pub(crate) fn decide_keep_skin_for_card(&mut self) {
        let Some(key) = self.skin_key() else { return };
        self.workspace.portrait.keep_skin.remove(&key);
        if autoshade::face::skin_values_beyond(&self.editor.doc().recipe) > 0 {
            self.workspace.portrait.keep_skin.insert(key, false);
        }
    }

pub(crate) fn inherit_keep_skin(&mut self, from: &str, to: &str) {
        let Some(photo) = self.editor.photo().src_path.clone() else { return };
        if let Some(on) = self.workspace.portrait.keep_skin.get(&(photo.clone(), from.to_string())).copied() {
            self.workspace.portrait.keep_skin.insert((photo, to.to_string()), on);
        }
    }

pub(crate) fn after_portrait_depth(&mut self) {
        if !std::mem::take(&mut self.workspace.portrait.blur_from_panel) {
            return;
        }
        let centre = self.editor.doc().recipe.masks.iter().find(|m| m.role == MaskRole::Skin).and_then(|m| match m.range {
            Some(autoshade::recipe::RangeMask::Color { px, py, .. }) => Some([px, py]),
            _ => None,
        });
        let subject = subject_raster(&self.editor.doc().recipe);
        let Some(lb) = self.editor.doc_mut().recipe.lens_blur.as_mut() else { return };
        if let Some(c) = centre {
            lb.focus = c;
            self.workspace.tools.lens_focus_picking = false;
        }
        if lb.protect.is_none() {
            lb.protect = subject;
        }
        self.editor.session_mut().dirty = true;
    }

pub(crate) fn set_keep_person_sharp(&mut self, on: bool) -> bool {
        let subject = subject_raster(&self.editor.doc().recipe);
        let Some(lb) = self.editor.doc_mut().recipe.lens_blur.as_mut() else { return false };
        let protect = if on { subject } else { None };
        if on && protect.is_none() {
            return false;
        }
        lb.protect = protect;
        lb.enabled = true;
        true
    }
}
