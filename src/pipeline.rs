//! Shared pipeline core used by both the CLI (`main.rs`) and the web UI
//! (`serve.rs`): run the advise chain for one RAW and write its outputs to the
//! right place. Keeping this in one module means the CLI and the server can
//! never drift apart.

#![deny(clippy::print_stderr)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::advisor::{
    Advisor, Decision, HeuristicProposer, Opinions, PiProposer, Preview, Verdict,
};
use crate::config::Config;
use crate::decode;
use crate::recipe::{DirectionAdherence, EditRecipe, GradeStrength, StrengthTier};
use crate::xmp;

/// The TASTE dials one develop is asked for — the two independent axes plus the
/// reference library's opt-in images.
///
/// A struct rather than more parameters: `produce_recipe` was already at
/// clippy's argument ceiling. It carries both axes because both are per-develop
/// user intent, but they are deliberately SEPARATE fields with separate names:
///   * [`style`](Self::style) = "how much like my reference looks" — how far the
///     proposal follows the finished photos the library returns;
///   * [`strength`](Self::strength) = "how hard to push" — how committed the
///     grade is (R23-3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradeRequest {
    /// What guides this develop: the reference library or the typed direction
    /// ([`Guide`]). With [`Guide::Direction`] the library is not read at all.
    pub guide: Guide,
    /// 0..1 — how far the proposal follows the reference looks (the GUI's
    /// 「Style」 slider, `--style` on the CLI). 0 disables the search entirely;
    /// read only under [`Guide::Library`].
    pub style: f32,
    /// Also SHOW the model the reference photos, as extra input images after
    /// the develop target. Opt-in and off by default: up to
    /// [`crate::library::REFERENCE_COUNT`] extra images on every call of a paid
    /// analysis (the GUI's checkbox discloses the cost).
    pub send_reference_image: bool,
    /// How COMMITTED the grade should be (the GUI's 「Strength」 slider,
    /// `--strength` on the CLI). Defaults to [`GradeStrength::DEFAULT`].
    pub strength: GradeStrength,
    /// DEEP THINKING (R23-4, feedback #13): ask the proposer for its structured
    /// working in the same strict response, raise this run's reasoning tier one
    /// step, and let the visual judge converge over more than one round.
    ///
    /// `false` by DEFAULT and by construction on the unattended surfaces: batch
    /// and eval build their request through [`Self::default`] /
    /// [`Self::with_style`], and `produce_recipe` calls the proposer
    /// unconditionally — before any judge gate — so a thinking chain riding on
    /// `propose` would double a 500-photo batch's spend and stop eval from
    /// measuring the bare proposal the restraint constants were calibrated
    /// against. It is an INTERACTIVE opt-in: the GUI checkbox, `--deep`, the web
    /// body's `deep`.
    pub think: bool,
    /// How closely to follow the optional direction.
    pub adherence: DirectionAdherence,
    /// May the SigLIP sidecar embed this develop's query? A VALUE, not a
    /// process-environment read: the CLI resolves it from `--embed` /
    /// `--no-embed`, the GUI from its own preference, both through
    /// [`crate::library::EmbeddingSwitch::resolve`]. Without it the library
    /// cannot be searched, and the rationale says so.
    pub embed: crate::library::EmbeddingSwitch,
}

/// The direction text a box-selected REGION folds into: the normalized corners
/// plus what the user asked, phrased so the proposer emits a mask exactly
/// there. The desktop app's region tool and the web request both build this
/// sentence — one implementation, so the two surfaces cannot drift; an empty
/// direction becomes the neutral one.
pub fn region_guidance(left: f32, top: f32, right: f32, bottom: f32, direction: &str) -> String {
    let direction = direction.trim();
    let direction =
        if direction.is_empty() { "make a tasteful local improvement" } else { direction };
    format!(
        "The user SELECTED a target region (normalized 0..1 frame coords): \
         left={left:.3} top={top:.3} right={right:.3} bottom={bottom:.3}. Apply the direction ONLY \
         to that region — emit a mask covering it (a radial mask with those exact \
         left/top/right/bottom bounds and feather ~0.4 is ideal, or a linear gradient for a thin \
         edge band). Direction: {direction}"
    )
}

impl Default for GradeRequest {
    /// The struct defaults, PLUS the embedding switch, whose default is the
    /// environment's answer — resolved here, once per request, so nothing
    /// downstream reads the process environment again.
    fn default() -> Self {
        GradeRequest {
            guide: Default::default(),
            style: Default::default(),
            send_reference_image: Default::default(),
            strength: Default::default(),
            think: Default::default(),
            adherence: Default::default(),
            embed: crate::library::EmbeddingSwitch::resolve(None, false),
        }
    }
}

impl GradeRequest {
    /// References as words only, at the shipped grade strength — the shape
    /// every non-GUI surface uses unless it says otherwise.
    pub fn with_style(style: f32) -> Self {
        Self { style, ..Self::default() }
    }

    /// The Style value the library is searched at: [`Self::style`] when the
    /// library guides this develop, 0 (no search, nothing to disclose) when
    /// the direction does.
    pub fn library_style(&self) -> f32 {
        match self.guide {
            Guide::Library => self.style,
            Guide::Direction => 0.0,
        }
    }
}

/// What guides one develop: the photographer's reference library OR their
/// typed direction — never both (user ruling 2026-10-14: 「"参考图库"和"提示词"
/// 两者只能二选一」). Two aims in one prompt made the advisor arbitrate between
/// them, and every reviewer downstream had to be told which one won; with one
/// aim per develop there is nothing to arbitrate.
///
/// `Default` is [`Self::Library`]: a develop with no direction typed is the
/// library's (or, at Style 0, nobody's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Guide {
    /// The finished photos the library returns; any typed direction is not sent.
    #[default]
    Library,
    /// The typed direction; the library is not read.
    Direction,
}

impl Guide {
    pub fn as_str(self) -> &'static str {
        match self {
            Guide::Library => "library",
            Guide::Direction => "direction",
        }
    }

    /// The command-line / request spelling.
    pub fn parse(text: &str) -> Result<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "library" => Ok(Guide::Library),
            "direction" => Ok(Guide::Direction),
            other => anyhow::bail!("unknown guide {other:?} — use \"library\" or \"direction\""),
        }
    }

    /// The ONE resolution the command line and the web body share. Unsaid,
    /// a typed direction picks [`Self::Direction`] and no direction picks
    /// [`Self::Library`]. Said as `library` WITH a direction is refused rather
    /// than dropping the photographer's words unseen.
    pub fn resolve(explicit: Option<Guide>, direction: Option<&str>) -> Result<Self> {
        let typed = direction.is_some_and(|d| !d.trim().is_empty());
        match explicit {
            Some(Guide::Library) if typed => anyhow::bail!(
                "a direction was given but the develop is guided by the reference library — \
                 the two are either/or: drop the direction, or guide by the direction"
            ),
            Some(g) => Ok(g),
            None if typed => Ok(Guide::Direction),
            None => Ok(Guide::Library),
        }
    }
}

/// The long edge of the JPEG preview handed to the vision proposer. Named
/// since R23-2 so the opt-in REFERENCE photos go through the same sizing as
/// the photo being developed instead of a second hand-typed number.
const ADVISOR_PREVIEW_EDGE: u32 = 1568;

/// The intent every downstream reviewer of one analysis shares (R23-3, B2).
///
/// A FUNCTION and not four inline literals: `produce_recipe` hands this to the
/// verifier, to the first judgement, and to the re-judge inside every
/// hint-guided revision round, and a second construction site is exactly how
/// the revision round would come to judge against a different brief than the
/// round it is revising — which is the mechanism that flattened the look in the
/// first place (six showcase runs, revision hints that were pure subtraction).
///
fn grade_intent<'a>(
    req: &GradeRequest,
    direction: Option<&'a str>,
    style_look: Option<&'a str>,
) -> crate::advisor::GradeIntent<'a> {
    crate::advisor::GradeIntent { strength: req.strength, adherence: req.adherence, direction, style_look }
}

pub fn direction_adherence_tier(
    direction: Option<&str>,
    adherence: DirectionAdherence,
) -> Option<&'static str> {
    direction
        .filter(|text| !text.trim().is_empty())
        .map(|_| adherence.tier().as_str())
}

/// Encode one reference photo as the JPEG preview the vision model receives —
/// the same decode → resize → JPEG path the photo being developed takes
/// ([`ADVISOR_PREVIEW_EDGE`]), so the two frames arrive comparable.
///
/// Takes a decode permit: this is a SECOND full decode inside one analyze, and
/// the process-wide cap exists precisely so concurrent decodes cannot stack
/// their ~180 MB buffers.
fn reference_preview(path: &Path) -> Result<Vec<u8>> {
    let _permit = decode::DecodePermit::acquire();
    let decoded = decode::decode_any(path)
        .with_context(|| format!("decode the reference photo {}", path.display()))?;
    let img = decoded.preview_resized(ADVISOR_PREVIEW_EDGE);
    let mut jpeg = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        .context("encode the style reference preview as JPEG")?;
    Ok(jpeg)
}

/// Run the full advise chain for one RAW: decode → propose (GPT or heuristic
/// fallback) → Claude verify → up to two verifier-driven revision rounds →
/// (when `judge`) the visual closed loop, which since R23-4 converges over one
/// to three guided rounds depending on the strength axis and `req.think`.
/// `verbose` prints the proposer/verifier/judge lines and, in thinking mode,
/// the model's full tool plan (CLI uses true, the server uses false).
/// Run the advise chain for one RAW. `guidance` is an optional user direction
/// (a prompt steering the edit, e.g. "warmer and moodier") woven into the GPT
/// prompt.
///
/// `raw` is the PHOTO — what the develop store, the WB anchor and every mask
/// cache are keyed on. `pixels` is the frame the model sees and the render
/// uses when that is not the photo itself: a variant on AI-generated pixels
/// (its ✨ raster), whose look and white balance are baked in, so the anchor
/// is left unread. `None` is the photo, the case every batch surface is.
///
/// With `base` the chain is an ADJUST (the GUI's 「AI adjust the selected
/// variant」, the CLI's and the web's refine): the model sees the base's own
/// render, is handed the request and the base's numbers, and may ask for
/// `ai` masks (sky / subject / object under a point) that
/// [`crate::segment::resolve_ai_masks`] cuts locally before the judge renders.
///
/// `sink` is where this call's DISCLOSURES go (R29-1). The chain raises two
/// ungated lines — the proposer fallback and the reference-search degrade — and
/// until this parameter existed they were hard-coded `eprintln!`s a caller
/// could neither route nor order (see [`crate::diag`]). Pass
/// [`crate::diag::stderr`] for the shipped behaviour; a pooled caller passes a
/// [`crate::diag::Collector`] and renders the lines into the photo's own block.
/// The SUBJECT is bound here from `raw`, so no caller can attribute this
/// photo's warnings to another.
// One advise chain's own inputs — every one of them is a decision a caller
// makes per call (which photo, whose config, how loud, what direction, what
// base, which grade, judge or not, where the disclosures go). Bundling them
// into a struct would move the arity, not remove it, and `GradeRequest`
// already carries the half that groups.
#[allow(clippy::too_many_arguments)]
pub fn produce_recipe(
    raw: &Path,
    pixels: Option<&Path>,
    cfg: &Config,
    verbose: bool,
    guidance: Option<&str>,
    base: Option<&EditRecipe>,
    req: GradeRequest,
    judge: bool,
    sink: &dyn crate::diag::Sink,
) -> Result<(EditRecipe, Verdict, Vec<crate::rationale::Note>)> {
    // S3: a FRESH analysis (no refine base, no direction, no saved pixels, no
    // --deep) runs the pi harness by default. This is the seam the desktop GUI
    // and `eval` come through. The harness commits the develop (its commit
    // tool) and returns the committed recipe; the GUI's own analyzed handler
    // then re-writes the same develop — the duplicate write is the known cost
    // of keeping the GUI's save/undo contract intact until the deferred-commit
    // mode lands. Everything the harness does not carry (refine, direction,
    // pixels, deep) stays on the in-process chain.
    if crate::harness::enabled()
        && crate::harness::key_present(cfg)
        && fresh_analysis(pixels, guidance, base, req.think)
    {
        // Deferred: the caller owns persistence (the GUI's on_analyzed backup
        // and undo contract), so the harness must not commit a second time.
        let run = crate::harness::run_deferred(raw)?;
        return Ok((run.recipe, run.verdict, Vec::new()));
    }
    // The third element (L12#2B): every DETERMINISTIC rationale fragment this
    // call appended, as typed notes rendering the rationale string's SUFFIX
    // (the prefix is the model's own prose). In-process only — the GUI
    // renders them localized; CLI/web/persistence keep the English string.
    //
    // The verifier is the pi harness now (S5.10): its key is the requirement
    // this flow is CERTAIN to hit, and checking it here keeps the paid proposer
    // from being billed before a certain verify failure. Checked before even
    // the decode.
    if !crate::harness::key_present(cfg) {
        anyhow::bail!(
            "no key for the pi harness — the data-level verifier runs on pi; set the \
             analysis key in Settings (or DEEPSEEK_API_KEY)"
        );
    }
    // This call's diagnostics channel, bound to the photograph it is about.
    let diag = crate::diag::Diag::about(sink, raw);
    // decode_any: a camera RAW, or an already-baked PNG/TIFF/JPEG (PNG-source mode).
    let decoded = decode::decode_any(pixels.unwrap_or(raw))?;

    // Library OR direction (user ruling 2026-10-14): a library-guided develop
    // sends no direction. The surfaces resolve the choice first
    // (`Guide::resolve`, the desktop switch), so nothing typed is lost here.
    let guidance = match req.guide {
        Guide::Library => None,
        Guide::Direction => guidance,
    };
    // The photographer's OWN words, captured BEFORE the Refine envelope below
    // shadows `guidance` with a whole EditRecipe JSON. Gates 3 and 4 (verifier,
    // visual judge) want the intent, not the recipe they are already reading.
    let user_direction = guidance;

    // ADJUST mode: when `base` (the user's CURRENT edit) is given, the model
    // is shown that edit's render and its numbers and asked for the complete
    // recipe that replaces it. Absent a base, behaviour is unchanged — a
    // fresh proposal from the original.
    //
    // The AI proposes/verifies over the camera's embedded preview, where the
    // base look AND the lens corrections are already IN the pixels — strip
    // both from the copy woven into the prompt and from the render it is
    // shown (either would double-apply). The strip lives HERE, not in the
    // callers: `base` must keep the user's REAL profile so
    // carry_over_unrepresentable below can preserve unsaved lens toggles
    // (pre-stripped callers made every Refine revert them to the saved
    // profile).
    let stripped_base: Option<EditRecipe> = base.map(|b| {
        let mut stripped = b.clone();
        stripped.strip_baked_start();
        stripped
    });
    let refine_owned: Option<String> = stripped_base.as_ref().map(|stripped| {
        adjust_envelope(&serde_json::to_string(stripped).unwrap_or_default(), guidance)
    });
    let guidance = refine_owned.as_deref().or(guidance);

    let preview_img = decoded.preview_resized(ADVISOR_PREVIEW_EDGE);
    let mut jpeg = Vec::new();
    preview_img
        .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        .context("encode preview JPEG for advisor")?;
    let preview = Preview { jpeg };
    // The REFERENCE LIBRARY (2026-10-13): the finished looks this develop may
    // follow, when the library guides it. Searched with this photo's image
    // vector, embedded from exactly this preview through the frame a build
    // stages (`library::query_vector`), so a query and a stored vector are
    // comparable. Nothing here can fail the develop: every gap degrades to no
    // references, and the rationale says why (`style_gap_note`).
    let mut library_gap: Option<String> = None;
    let library = match (req.library_style() > 0.0).then(crate::library::load) {
        Some(crate::library::Loaded::Ready(lib)) => Some(lib),
        Some(crate::library::Loaded::Unusable(err)) => {
            // Once per RUN on stderr — the file is unusable for every photo in
            // it, so naming whichever worker reached it first would misread as
            // "this photo's library". The per-photo half rides the rationale.
            static ONCE: std::sync::Once = std::sync::Once::new();
            let run = diag.rebound(crate::diag::Subject::Run);
            let msg = err.clone();
            ONCE.call_once(move || {
                run.warn(format!(
                    "reference library unavailable ({msg}) — the Style slider has no effect \
                     until it is rebuilt"
                ));
            });
            library_gap = Some(err);
            None
        }
        Some(crate::library::Loaded::Absent) | None => None,
    };
    let query = match &library {
        Some(_) if !req.embed.on() => {
            library_gap = Some("image embedding is switched off, so the library cannot be searched".into());
            None
        }
        Some(_) => {
            let opts = crate::embed::EmbedOpts::from_config(cfg);
            match crate::library::query_vector(&opts, &decoded.preview, &crate::store::store_root()) {
                Ok(q) => Some(q),
                Err(e) => {
                    diag.warn(format!("reference search unavailable ({e:#}) — no reference looks for this develop"));
                    library_gap = Some(crate::rationale::error_line(&e));
                    None
                }
            }
        }
        None => None,
    };
    // The full-resolution preview buffer is DEAD from here on — only meta and
    // histogram feed the advise chain, which can stall on the network for
    // minutes. Keeping `decoded` whole pinned hundreds of MB of 61MP pixels
    // for that entire window.
    let decode::Decoded { meta, histogram, .. } = decoded;
    let hits: Vec<crate::library::Hit> = match (&library, &query) {
        (Some(lib), Some(q)) => lib.search(
            &crate::library::Query { image: q, exclude: Some(raw) },
            crate::library::REFERENCE_COUNT,
        ),
        _ => Vec::new(),
    };
    // Opt-in: SHOW the model the references, not only their phrases. Each that
    // loads becomes the next image after the develop target; one that does not
    // is named and goes as words, and the rest still go.
    let mut ref_previews: Vec<Preview> = Vec::new();
    let mut image_numbers: Vec<Option<usize>> = Vec::new();
    let mut ref_image_err: Option<String> = None;
    if req.send_reference_image {
        for h in &hits {
            match reference_preview(Path::new(&h.entry.path)) {
                Ok(jpeg) => {
                    ref_previews.push(Preview { jpeg });
                    image_numbers.push(Some(1 + ref_previews.len()));
                }
                Err(e) => {
                    image_numbers.push(None);
                    ref_image_err.get_or_insert_with(|| crate::rationale::error_line(&e));
                }
            }
        }
    }
    let reference =
        crate::library::reference_block(&hits, &image_numbers, crate::library::StyleVoice::for_style(req.style));
    let ref_str = reference.as_deref();
    // B2: the intent every downstream reviewer in this function shares (R23-3).
    // ONE value, spread into the verifier, the first judgement and every
    // hint-guided re-judge below; it carries the look the references name.
    let style_look = crate::library::look_summary(&hits);
    let intent = grade_intent(&req, user_direction, style_look.as_deref());
    let reference_files: Vec<String> = hits.iter().map(|h| stem(Path::new(&h.entry.path)).to_string()).collect();
    if verbose && !hits.is_empty() {
        println!("style    : {} reference look(s) — {}", hits.len(), reference_files.join(", "));
        if !ref_previews.is_empty() {
            println!(
                "style    : {} of them go to the vision model as images (the reference-image option \
                 is on) — extra images on each call of this analysis",
                ref_previews.len()
            );
        }
    }
    if verbose
        && let Some(e) = &ref_image_err
    {
        println!("style    : a reference photo could not be prepared ({e}) — it went as words only");
    }
    if verbose
        && let Some(g) = guidance {
            println!("direction: {g}");
        }

    let (meta, hist) = (&meta, &histogram);

    // The WB anchor, ONE read for all THREE of its consumers (the single-read
    // rule): the PROPOSER's prompt, the visual judge's render clone and the
    // deliverable stamp consume the same values, so they can never disagree
    // across a concurrent writer's retire/publish window.
    //
    // Read HERE, above the propose call, since R23-1: `temperature_k` is an
    // ABSOLUTE Kelvin target and `tint` a shift RELATIVE to the same anchor, so
    // the prompt must state the anchor the deliverable will actually carry
    // (feedback #12) — a second, fresher read at the prompt would re-decode the
    // RAW and could quote the model a different anchor than the render uses.
    // The base-look snapshot deliberately stays where it was (below, after the
    // AI chain): it has ONE consumer, the deliverable stamp, so reading it late
    // keeps it as fresh as it has always been instead of widening its staleness
    // window across minutes of network calls for no gain.
    let (anchor_k, anchor_tint) = if pixels.is_some() {
        // Baked pixels carry their white balance: no anchor to quote or to
        // stamp (the canvas copy of such a card has none either).
        (None, None)
    } else {
        match saved_recipe_snapshot(raw, &crate::diag::Diag::about(sink, raw)) {
            // Saved-first, like the base-look stamp below (a legacy save keeps
            // None -> the 5500 K anchor -> byte-identical rendering of its
            // tuned Kelvin).
            Some(saved) => (saved.as_shot_k, saved.as_shot_tint),
            None => fresh_as_shot_wb(raw),
        }
    };
    // What the PROPOSER sees in adjust mode: the current edit RENDERED — the
    // same render the judge scores (`judge_view` strips the same two fields
    // and carries the same anchor) — so the request is answered against the
    // picture the photographer is looking at, not against the straight
    // preview plus a JSON to imagine. A fresh analysis sees the straight
    // preview, as before; the judge's reference stays the straight preview.
    let proposer_preview = match &stripped_base {
        Some(stripped) => {
            let mut shown = stripped.clone();
            shown.as_shot_k = anchor_k;
            shown.as_shot_tint = anchor_tint;
            let mut jpeg = Vec::new();
            crate::render::develop_preview(&preview_img, &shown)
                .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
                .context("encode the current edit's render for the advisor")?;
            Preview { jpeg }
        }
        None => Preview { jpeg: preview.jpeg.clone() },
    };
    // The `ai` masks a proposal asks for are cut HERE, by the local
    // segmenter, before the verifier, the judge or the deliverable read the
    // recipe: the judge renders the recipe, and an AI mask without its alpha
    // renders as no mask at all. Keyed and homed on the PHOTO, segmented on
    // the pixels the render uses (`segment::resolve_ai_masks`, F1-C). Cheap
    // when nothing asks: a recipe with no unresolved AI mask decodes nothing.
    let cut_ai_masks = |r: &mut EditRecipe, notes: &mut Vec<crate::rationale::Note>| {
        let DevelopMasks { ai: res, faces } = resolve_develop_masks(cfg, raw, pixels.unwrap_or(raw), r);
        if res.resolved > 0 {
            crate::rationale::push_note(
                &mut r.rationale,
                notes,
                crate::rationale::Note::new(
                    crate::rationale::keys::AI_MASKS_SEGMENTED,
                    vec![("n", res.resolved.to_string())],
                ),
            );
        }
        for (name, why) in &res.unresolved {
            crate::rationale::push_note(
                &mut r.rationale,
                notes,
                crate::rationale::Note::new(
                    crate::rationale::keys::AI_MASK_NOT_CUT,
                    vec![("name", name.clone()), ("why", why.clone())],
                ),
            );
        }
        // The Portraits targets `face` / `eyes` (v1.14.0): outlined by the
        // face finder on the same photo/pixels split; a mask it cannot
        // outline is taken out, not carried — it has no raster to render.
        if faces.resolved > 0 {
            crate::rationale::push_note(
                &mut r.rationale,
                notes,
                crate::rationale::Note::new(
                    crate::rationale::keys::FACE_MASKS_OUTLINED,
                    vec![("n", faces.resolved.to_string())],
                ),
            );
        }
        for (name, why) in &faces.dropped {
            crate::rationale::push_note(
                &mut r.rationale,
                notes,
                crate::rationale::Note::new(
                    crate::rationale::keys::FACE_MASK_LEFT_OUT,
                    vec![("name", name.clone()), ("why", why.clone())],
                ),
            );
        }
    };

    // GPT vision when a key is set; on failure (quota/network) warn and fall back
    // to the heuristic so we still produce a recipe (disclosure, not masking).
    let openai = PiProposer::new(cfg);
    // The per-call inputs every propose in this function shares (the revision
    // rounds differ only in `hint`).
    let propose_ctx = crate::advisor::ProposeContext {
        reference: ref_str,
        guidance,
        hint: None,
        as_shot_k: anchor_k,
        // Rides EVERY round of this analysis (the revision rounds spread this
        // struct): a reference the first call saw and the revision did not
        // would make the two rounds answer different questions.
        reference_images: &ref_previews,
        // GATE 1 (prompt) + GATE 2 (`temper`, inside the provider).
        strength: req.strength,
        // R23-4: the structured working + the deepened tier, on EVERY propose
        // this analysis makes (the revision rounds spread this struct) — a
        // first call that planned and a revision that did not would answer two
        // different questions, exactly like the reference image above.
        think: req.think,
        adherence: req.adherence,
    };
    let mut det_notes: Vec<crate::rationale::Note> = Vec::new();
    // The working of the proposal that SURVIVES (R23-4). Every arm that
    // replaces `recipe` wholesale replaces this with it — a plan describing a
    // discarded candidate is worse than none.
    let mut thinking: Option<crate::advisor::Thinking> = None;
    // …and which nullable controls THAT proposal spoke about (R23-1b, the
    // lens trio; 2026-10-21, the effects, B&W and calibration too), tracked
    // the same way and for the same reason: `carry_over_unrepresentable`
    // must not re-impose the base's values over an opinion the surviving
    // recipe actually stated. Nothing stated = the historical behaviour.
    let mut opinions = Opinions::default();
    let (mut recipe, can_revise) = if cfg.openai_api_key.is_some() {
        if verbose {
            println!("proposer : OpenAI ({})", cfg.openai_model);
        }
        // PATCH(local): the tool-use contract, applied to the proposer. Its reply is a
        // schema-checked object; when the validator refuses it, the agent gets the
        // validator's OWN message back and one more turn — the same loop a tool call
        // gets in an agent harness. Loosening the schema instead would have hidden
        // the hallucination (and did, briefly: `full_bottom` slipped through).
        let mut repair_hint: Option<String> = None;
        let mut repair_count = 0usize;
        let proposal = loop {
            let ctx = match &repair_hint {
                Some(h) => crate::advisor::ProposeContext { hint: Some(h.as_str()), ..propose_ctx },
                None => propose_ctx,
            };
            match openai.propose_planned_with_base(&proposer_preview, meta, hist, &ctx, base) {
                Ok(p) => break Ok(p),
                Err(e) => {
                    let msg = e.to_string();
                    let schema_refusal = msg.contains("unknown field")
                        || msg.contains("missing field")
                        || msg.contains("invalid type")
                        || msg.contains("no variant of enum")
                        || msg.contains("expected one of");
                    if schema_refusal && repair_count < 2 {
                        repair_count += 1;
                        diag.warn(format!(
                            "schema validator rejected the reply ({msg}) → returning the error to the model (repair {repair_count}/2)"
                        ));
                        repair_hint = Some(format!(
                            "YOUR PREVIOUS REPLY WAS REJECTED BY THE SCHEMA VALIDATOR: {msg}\n\
                             Send the whole answer again as ONE JSON object and make it pass validation.\n\
                             Use only the field names the schema lists; never invent one. A linear gradient\
                             mask has exactly zero_x, zero_y, full_x, full_y (there is no full_bottom / full_top /\
                             x / y / width / height). Do not repeat the rejected reply."
                        ));
                        continue;
                    }
                    break Err(e);
                }
            }
        };
        match proposal {
            Ok(p) => {
                thinking = p.thinking;
                opinions = p.opinions;
                // The provider's own disclosures are the HEAD of this call's
                // note list (A10): they were appended to the model's prose, so
                // by the suffix contract they are where the notes start.
                det_notes = p.notes;
                let mut asked = p.recipe;
                cut_ai_masks(&mut asked, &mut det_notes);
                (asked, true)
            }
            Err(e) if base.is_some() => {
                // REFINE means "adjust MY edit": the heuristic fallback
                // proposes from scratch and cannot see the base, so falling
                // back here REPLACED the user's current edit with a generic
                // baseline — and an Accept verdict then auto-saved it. A
                // failed refine fails loudly; the edit on screen stays as it
                // was.
                return Err(anyhow::Error::from(e)
                    .context("the AI refine failed — your current edit is unchanged"));
            }
            Err(e) => {
                // THE line `jobs`' module doc names by file:line as the one a
                // parallel batch reorders. R28 Batch-5 5c stamped it with the
                // photograph; R29-1 puts it on the caller's channel, so a
                // pooled caller can also decide WHEN it appears. The same fact
                // rides the typed note channel below — which `batch` used to
                // discard and now renders into the photo's block.
                diag.warn(format!(
                    "GPT proposer failed ({e})\n  → falling back to the heuristic baseline."
                ));
                // Hand the REAL cause to the heuristic: this stderr line is
                // invisible in the windowed GUI, so the recipe's rationale is
                // the only place the user can learn why the AI didn't run.
                let heuristic = HeuristicProposer {
                    fallback_reason: Some(crate::rationale::error_line(&anyhow::Error::new(e))),
                };
                let (r, note) = heuristic.propose_noted(hist, req.strength)?;
                det_notes.push(note);
                (r, false)
            }
        }
    } else {
        // Refine is not a proposal — it is "adjust THIS edit". The heuristic
        // cannot do that: it never receives `base` and builds a fresh recipe
        // from the histogram alone, so letting it answer a refine replaces
        // the user's work with a generic baseline AND saves it (the verifier
        // accepts a plain baseline happily). The has-key path already refuses
        // for the same reason; this branch is reachable with no key at all,
        // and both UIs offer the Refine control without checking for one.
        if base.is_some() {
            anyhow::bail!(
                "Refine needs the image model, and no OPENAI_API_KEY is set — \
                 your current edit is unchanged"
            );
        }
        if verbose {
            println!("proposer : heuristic baseline (set OPENAI_API_KEY to use GPT vision)");
        }
        let heuristic = HeuristicProposer::default();
        let (r, note) = heuristic.propose_noted(hist, req.strength)?;
        det_notes.push(note);
        (r, false)
    };

    // Verifier (analyst role): a data-only acceptance check executed by the
    // pi harness (S5.10). The paid Claude-CLI / OpenAI-chat verifier providers
    // no longer exist.
    let verifier: Box<dyn Advisor> = Box::new(crate::advisor::PiVerifier::new());
    if verbose {
        println!("verifier : pi harness");
    }
    let mut verdict = verifier.verify(&recipe, meta, hist, &intent)?;

    // Bounded verify→revise loop (only if GPT actually produced the recipe). With
    // the now-symmetric verifier — which pushes a too-flat edit to commit AND a
    // too-cooked one to ease — a few rounds converge toward a finished look instead
    // of just ratcheting down. Capped at MAX_REVISIONS to bound cost/latency; we
    // stop early on Accept or when the verifier stops giving a revision hint.
    const MAX_REVISIONS: usize = 2;
    let mut round = 0;
    while can_revise && round < MAX_REVISIONS && verdict.decision != Decision::Accept {
        let Some(hint) = verdict.revised_hint.clone() else { break };
        round += 1;
        if verbose {
            println!("verdict {:?} → revision {round}/{MAX_REVISIONS} (hint: {hint})", verdict.decision);
        }
        // Disclosure, not masking (same policy as the propose fallback above): a
        // transient provider failure in a LATER round must not throw away the
        // already-paid-for, already-VERIFIED recipe from the previous round.
        // Keep that (recipe, verdict) pair — they stay consistent, so the
        // returned verdict still describes the returned recipe — and record why
        // the loop stopped in the rationale, the one channel all three surfaces
        // show (the windowed GUI has no console for the CLI's stderr). A
        // FIRST-round failure still errors: there is no good pair to keep.
        let mut revised = match openai.propose_planned_with_base(
            &proposer_preview,
            meta,
            hist,
            &crate::advisor::ProposeContext { hint: Some(&hint), ..propose_ctx },
            base,
        ) {
            Ok(r) => r,
            Err(e) => {
                crate::rationale::push_note(
                    &mut recipe.rationale,
                    &mut det_notes,
                    crate::rationale::Note::new(
                        crate::rationale::keys::REVISION_FAILED,
                        vec![("round", round.to_string()), ("e", crate::rationale::error_line(&anyhow::Error::new(e)))],
                    ),
                );
                break;
            }
        };
        cut_ai_masks(&mut revised.recipe, &mut revised.notes);
        match verifier.verify(&revised.recipe, meta, hist, &intent) {
            Ok(v) => {
                recipe = revised.recipe;
                // Fresh model prose — the notes described the DISCARDED
                // recipe's tail, so they reset with it (the suffix contract),
                // to the new proposal's own provider disclosures.
                det_notes = revised.notes;
                // …and the working travels with the proposal it explains.
                thinking = revised.thinking;
                opinions = revised.opinions;
                verdict = v;
            }
            Err(e) => {
                crate::rationale::push_note(
                    &mut recipe.rationale,
                    &mut det_notes,
                    crate::rationale::Note::new(
                        crate::rationale::keys::REVISION_VERIFY_FAILED,
                        vec![("round", round.to_string()), ("e", crate::rationale::error_line(&anyhow::Error::new(e)))],
                    ),
                );
                break;
            }
        }
    }

    // R20: the visual CLOSED LOOP — the first eye ever laid on the result.
    // The proposer emits numbers blind (it never sees what they render to)
    // and the verifier judges data-only by contract, so a plausible-but-off
    // develop sailed through both. Render the verified proposal and put it
    // in front of the vision model as a JUDGE; on a revise verdict run ONE
    // guided round, adopted only when the re-judge scores at least as high
    // (do-no-harm — the fit's own rule applied here). Every branch discloses
    // through the rationale; a judge failure degrades, never errors.
    //
    // Placed at the END of the look-mutation chain on purpose (review R20:
    // M1+S1+S2 shared this one upstream cause — the judge used to sit in
    // the middle): the WB anchor is the hoisted one the deliverable will carry (temperature_k is
    // ABSOLUTE Kelvin against it — an unanchored clone rendered a tungsten
    // shot's correction as identity), and a refine's bitmap masks are
    // carried into the render clone. The judged render IS the look the user
    // receives; only the calibration stamp below remains, and the clone
    // deliberately excludes it — the embedded preview already CONTAINS the
    // base look and lens corrections, so the clone keeps base_curve empty
    // and lens_profile default (the refine-strip rule) while carrying the
    // real WB anchor.
    //
    // COST: paid vision calls (1 judge; +1 propose, +1 verify, +1 judge per
    // guided round bought). `judge` is therefore an explicit caller
    // decision: interactive analyze passes true (this closed loop IS the
    // R20 strengthening); batch and eval pass false — a 500-photo batch
    // must not silently multiply spend, and eval measures the RAW proposal
    // (review R20-M2/M3).
    //
    // R23-4 turns the single round into a bounded CONVERGENCE loop
    // (`judge_convergence` / `judge_next_round`): up to 3 rounds, and only
    // where the photographer asked for depth or for a bold grade — every
    // other path keeps exactly the one round R20 shipped. The three R20
    // decisions hold PER ROUND, not once: the fee is the caller's explicit
    // choice (this same `judge` gate), a revision is adopted only when the
    // re-judge holds the score (`visual_review_round`'s `>=`), and an
    // identical revision short-circuits before paying for a verify or a
    // re-judge (that check lives inside `visual_review_round`, so it fires
    // on every round by construction). Worst case for one analyze: 11 API
    // calls today (6 carrying images, 8 high-detail frames), 17 with deep
    // thinking at a committed strength (10 with images, 14 frames) — the
    // numbers the GUI tooltip and `--deep`'s help quote.
    if judge && can_revise {
        let judge_view = |r: &EditRecipe, opinions: Opinions| -> EditRecipe {
            let mut v = r.clone();
            if let Some(b) = base {
                // A refine's bitmap masks are part of the look under
                // judgement (the response schema cannot return them; the
                // deliverable gets them re-attached below) — without this
                // the judge saw the user's sky mask missing and bought a
                // revision to fix what was already fixed (review R20-S1).
                // The lens opinion rides along for the same reason: the judge
                // must score the frame the deliverable will BE, and since
                // R23-1b those two differ whenever the model stated a lens
                // value (it would otherwise be judged with the base's).
                carry_over_unrepresentable(&mut v, b, opinions, None);
            }
            v.strip_baked_start();
            v.as_shot_k = anchor_k;
            v.as_shot_tint = anchor_tint;
            v
        };
        let judge_of = |r: &EditRecipe,
                        opinions: Opinions|
         -> Result<crate::advisor::Judgement, crate::advisor::AdvisorError> {
            let rendered = crate::render::develop_preview(&preview_img, &judge_view(r, opinions));
            let mut jpeg = Vec::new();
            rendered
                .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
                .map_err(|e| {
                    crate::advisor::AdvisorError::Io(std::io::Error::other(format!(
                        "encode judge render JPEG: {e}"
                    )))
                })?;
            crate::advisor::judge_pair(
                crate::advisor::JudgeImages { reference: &preview.jpeg, candidate: &jpeg },
                crate::advisor::JudgeTask::Develop,
                // Oversize context is OMITTED, not truncated: a cut JSON is
                // a malformed blob to the judge (review R20-N5); judge_pair's
                // own 16 KiB bound stays as the backstop.
                serde_json::to_string(r)
                    .ok()
                    .filter(|s| s.len() <= 16 * 1024)
                    .as_deref(),
                // GATE 4: the judge can BUY a revision, so a rubric that does
                // not know the target strength does not merely mis-report — it
                // re-timid-ifies the develop the user asked to push.
                Some(intent),
            )
        };
        // R23-4: how far this analysis may iterate, from ONE pure decision
        // (`judge_convergence`) — the target the rubric already stated in
        // words, and the round cap that keeps it bounded.
        let (target, cap) = judge_convergence(req.strength, req.think);
        // Every note this block writes, HELD until the loop ends. An adopted
        // round replaces `recipe` wholesale and clears `det_notes` with it (the
        // rationale-suffix contract), so a note pushed mid-loop would be
        // dropped by the very round it describes — the same defect R23-2 fixed
        // for the style block by moving it below the judge. Flushed onto the
        // FINAL recipe in order, which on the single-round path is
        // byte-identical to the old inline pushes (nothing is ever dropped
        // there).
        let mut log: Vec<crate::rationale::Note> = Vec::new();
        // The CANDIDATE's lens opinion, written by the propose closure and read
        // by the judge closure inside the same `visual_review_round` call. A
        // `Cell` because those two closures are alive together (one `FnMut`, one
        // `Fn`) and a plain `&mut` capture in the first would forbid the read in
        // the second; `Opinions` is `Copy`, so this is a plain slot, not
        // shared mutability of anything the round can observe out of order — the
        // round always proposes before it judges.
        let candidate_opinions = std::cell::Cell::new(Opinions::default());
        match judge_of(&recipe, opinions) {
            Err(e) => {
                log.push(crate::rationale::Note::new(
                    crate::rationale::keys::JUDGE_UNAVAILABLE,
                    vec![("e", crate::rationale::error_line(&anyhow::Error::new(e)))],
                ));
            }
            Ok(first) => {
                if verbose {
                    println!(
                        "judge    : {:.0}/100 {:?} — {}",
                        first.score, first.decision, first.critique
                    );
                }
                // The judgement describing the recipe currently held, and how
                // many guided rounds have been paid for.
                let mut current = first;
                let mut rounds_done = 0usize;
                loop {
                    let score1 = format!("{:.0}", current.score);
                    // R20's rule owns round 1 (a non-Accept verdict WITH an
                    // instruction); the target and the cap own every round
                    // after it. See `judge_next_round`.
                    let Some(hint) = judge_next_round(rounds_done, &current, target, cap) else {
                        log.push(crate::rationale::Note::new(
                            crate::rationale::keys::JUDGE_SCORE,
                            vec![("score", score1), ("critique", current.critique.clone())],
                        ));
                        break;
                    };
                    // Short prefix: the whole hint is bounded at 1024 in
                    // the proposer, so every prefix byte comes out of the
                    // judge's own instruction (review R20-N2).
                    let h = format!("visual judge (it SAW your previous render): {hint}");
                    if verbose {
                        println!(
                            "judge    : guided revision {}/{cap} (hint: {h})",
                            rounds_done + 1
                        );
                    }
                    let mut candidate_thinking: Option<crate::advisor::Thinking> = None;
                    let mut candidate_notes: Vec<crate::rationale::Note> = Vec::new();
                    let outcome = visual_review_round(
                        &h,
                        &recipe,
                        |h| {
                            let p = openai.propose_planned_with_base(
                                &proposer_preview,
                                meta,
                                hist,
                                &crate::advisor::ProposeContext {
                                    hint: Some(h),
                                    ..propose_ctx
                                },
                                base,
                            )?;
                            candidate_thinking = p.thinking;
                            candidate_opinions.set(p.opinions);
                            candidate_notes = p.notes;
                            let mut asked = p.recipe;
                            cut_ai_masks(&mut asked, &mut candidate_notes);
                            Ok(asked)
                        },
                        |r| verifier.verify(r, meta, hist, &intent),
                        // The candidate's own opinion: `visual_review_round`
                        // judges only the recipe its propose closure just
                        // returned (`judge(&revised)`), never the held one.
                        |r| judge_of(r, candidate_opinions.get()),
                        current.score,
                    );
                    rounds_done += 1;
                    match outcome {
                        VisualRound::Adopted { recipe: r2, verdict: v2, second } => {
                            recipe = *r2;
                            // Fresh model prose — the notes described the
                            // DISCARDED recipe's tail (the suffix contract),
                            // to this candidate's own provider disclosures.
                            det_notes = std::mem::take(&mut candidate_notes);
                            // …and the working travels with its proposal.
                            thinking = candidate_thinking;
                            opinions = candidate_opinions.get();
                            verdict = v2;
                            if verbose {
                                println!(
                                    "judge    : re-scored {:.0}/100 — adopted",
                                    second.score
                                );
                            }
                            let score2 = format!("{:.0}", second.score);
                            // Look ahead with the SAME predicate the loop
                            // head uses: an adoption that is the last word
                            // keeps R20's terminal note (byte-identical on
                            // the single-round path), while one the loop will
                            // iterate past logs the round and moves on.
                            let more =
                                judge_next_round(rounds_done, &second, target, cap).is_some();
                            log.push(crate::rationale::Note::new(
                                if more {
                                    crate::rationale::keys::JUDGE_ROUND
                                } else {
                                    crate::rationale::keys::JUDGE_ADOPTED
                                },
                                if more {
                                    vec![
                                        ("round", rounds_done.to_string()),
                                        ("score1", score1),
                                        ("score2", score2),
                                        ("target", format!("{target:.0}")),
                                    ]
                                } else {
                                    vec![
                                        ("score1", score1),
                                        ("score2", score2),
                                        ("critique", second.critique.clone()),
                                    ]
                                },
                            ));
                            if !more {
                                break;
                            }
                            current = second;
                        }
                        VisualRound::Unchanged => {
                            // The model returned the SAME settings: another
                            // round would ask the same question of the same
                            // recipe and pay for the same answer. (R20 定案 3
                            // — the short-circuit lives inside
                            // `visual_review_round`, so it fires on EVERY
                            // round, not just the first.)
                            log.push(crate::rationale::Note::new(
                                crate::rationale::keys::JUDGE_UNCHANGED,
                                vec![("score", score1), ("critique", current.critique.clone())],
                            ));
                            break;
                        }
                        VisualRound::KeptLower { second_score } => {
                            if verbose {
                                println!(
                                    "judge    : re-scored {second_score:.0}/100 — lower, revision discarded"
                                );
                            }
                            log.push(crate::rationale::Note::new(
                                crate::rationale::keys::JUDGE_KEPT,
                                vec![
                                    ("score1", score1),
                                    ("critique", current.critique.clone()),
                                    ("score2", format!("{second_score:.0}")),
                                ],
                            ));
                            break;
                        }
                        VisualRound::RoundFailed { e } => {
                            log.push(crate::rationale::Note::new(
                                crate::rationale::keys::JUDGE_ROUND_FAILED,
                                vec![
                                    ("score", score1),
                                    ("critique", current.critique.clone()),
                                    ("e", crate::rationale::error_line(&anyhow::anyhow!("{}", e))),
                                ],
                            ));
                            break;
                        }
                        VisualRound::RejudgeFailed { e } => {
                            log.push(crate::rationale::Note::new(
                                crate::rationale::keys::JUDGE_REJUDGE_FAILED,
                                vec![
                                    ("score", score1),
                                    ("critique", current.critique.clone()),
                                    ("e", crate::rationale::error_line(&anyhow::anyhow!("{}", e))),
                                ],
                            ));
                            break;
                        }
                    }
                }
            }
        }
        // Onto the recipe that SURVIVED the loop.
        for note in log {
            crate::rationale::push_note(&mut recipe.rationale, &mut det_notes, note);
        }
    }

    // ── R23-4 THINKING disclosure, same placement and same reason ──────────
    // The three single-sentence fields of the working that belongs to the
    // recipe actually being returned. Bounded at the trust boundary already
    // (advisor::THINK_FIELD_MAX_BYTES), so what lands here is three sentences,
    // not a transcript — the plan's per-family reasoning is deliberately NOT in
    // the rationale (it is 9 clauses long, and this string is capped and
    // reprinted on five surfaces); the CLI prints it in full below.
    if let Some(t) = &thinking {
        for (key, arg, text) in [
            (crate::rationale::keys::THINK_SCENE, "scene", &t.scene),
            (crate::rationale::keys::THINK_LOOK, "look", &t.intended_look),
            (crate::rationale::keys::THINK_CRITIQUE, "critique", &t.self_critique),
        ] {
            if !text.is_empty() {
                crate::rationale::push_note(
                    &mut recipe.rationale,
                    &mut det_notes,
                    crate::rationale::Note::new(key, vec![(arg, text.clone())]),
                );
            }
        }
        // R23-1b: the PIXEL-tool suggestions, on the SAME channel and bounded
        // the same way (≤3 entries, each `why` already capped at the trust
        // boundary). One line, because that is all this is: the model naming
        // work the develop cannot do. Nothing is executed and no parameters are
        // implied — a button here would read as "the AI has this configured",
        // and R20 settled that a paid, destructive operation is the explicit
        // caller's decision.
        if !t.pixel_tools.is_empty() {
            let tools = t
                .pixel_tools
                .iter()
                .map(|s| {
                    if s.why.is_empty() {
                        s.tool.wire().to_string()
                    } else {
                        format!("{} ({})", s.tool.wire(), s.why)
                    }
                })
                .collect::<Vec<_>>()
                .join("; ");
            crate::rationale::push_note(
                &mut recipe.rationale,
                &mut det_notes,
                crate::rationale::Note::new(
                    crate::rationale::keys::PIXEL_TOOLS,
                    vec![("tools", tools)],
                ),
            );
        }
        if verbose && !t.tool_plan.is_empty() {
            println!("plan     : the model's tool plan for this photo");
            for step in &t.tool_plan {
                println!(
                    "  {} {:<14} {}",
                    if step.used { "USE " } else { "skip" },
                    step.control,
                    step.why
                );
            }
        }
    }

    // ── R23-2 reference DISCLOSURE, after every wholesale recipe replacement ──
    // The one channel all three surfaces show, written HERE because an adopted
    // visual revision replaces `recipe` wholesale and clears `det_notes` with
    // it. In reading order: which finished photos this develop leaned on, in
    // whether they went as images, and whether the whole mechanism came up
    // empty.
    if let Some(note) = references_note(&reference_files) {
        crate::rationale::push_note(&mut recipe.rationale, &mut det_notes, note);
    }
    if !ref_previews.is_empty() {
        crate::rationale::push_note(
            &mut recipe.rationale,
            &mut det_notes,
            crate::rationale::Note::new(
                crate::rationale::keys::STYLE_REF_IMAGE,
                vec![("n", ref_previews.len().to_string())],
            ),
        );
    }
    if let Some(e) = &ref_image_err {
        crate::rationale::push_note(
            &mut recipe.rationale,
            &mut det_notes,
            crate::rationale::Note::new(
                crate::rationale::keys::STYLE_REF_IMAGE_FAILED,
                vec![("e", e.clone())],
            ),
        );
    }
    if let Some(tier) = direction_adherence_tier(user_direction, req.adherence) {
        crate::rationale::push_note(
            &mut recipe.rationale,
            &mut det_notes,
            crate::rationale::Note::new(
                crate::rationale::keys::ADVISOR_NOTE_DIRECTION_ADHERENCE,
                vec![("tier", tier.into())],
            ),
        );
    }
    if let Some(note) = style_gap_note(req.library_style(), ref_str, library_gap.as_deref()) {
        crate::rationale::push_note(&mut recipe.rationale, &mut det_notes, note);
    }

    // Base look, stamped in ONE place for every surface: the proposal and the
    // verification above both ran over the camera's embedded preview — the
    // very base the curve approximates — so the AI's JSON round-trip never
    // decides it. A saved recipe.json owns its curve verbatim (a legacy save
    // must keep rendering as it was tuned); otherwise a fresh estimate.
    // Without this, a CLI-written analyze recipe carried an empty curve and
    // the open-time "recipe.json keeps its saved curve" rule then pinned the
    // dark pre-base-look rendering onto that photo forever.
    //
    // Its OWN read of the saved recipe (the WB anchor above took an earlier one
    // because the prompt needed it): this stamp is the snapshot's only consumer,
    // so it stays as late — and as fresh — as it has always been.
    match saved_recipe_snapshot(raw, &crate::diag::Diag::about(sink, raw)) {
        Some(saved) => {
            // The era stamp travels WITH the curve (the paste rule): the
            // proposer's recipe is era-2 by Default — or whatever integer the
            // model chose to emit, since the strict schema forces the field —
            // and stamping a saved era-1 curve under either laundered it past
            // every repair permanently, from the very writer (Analyze) that
            // produces the canonical recipe.json.
            recipe.version = saved.version;
            recipe.base_curve = saved.base_curve;
            recipe.lens_profile = saved.lens_profile;
            // v1.17.0: the profile and Look the saved develop starts from
            // are calibration too (a refine's carry-over restates them from
            // its own base below).
            recipe.camera_profile = saved.camera_profile;
            recipe.look = saved.look;
        }
        None => {
            // A fresh start by THIS build — and the stamp is authored here,
            // never by the model: version is provenance, and the response
            // schema makes the model emit SOMETHING for it.
            recipe.version = crate::recipe::CALIB_ERA;
            fresh_canvas_start(raw, &crate::diag::Diag::about(sink, raw)).stamp(&mut recipe);
            recipe.lens_profile = fresh_lens_profile(raw);
        }
    }
    // The as-shot WB anchor is the third calibration half — same saved-first
    // rule, via the ONE early snapshot (the prompt named these very values and
    // the judge above rendered against them, so the anchor the model was told,
    // the anchor it was scored on and the anchor delivered are all one).
    recipe.as_shot_k = anchor_k;
    recipe.as_shot_tint = anchor_tint;
    // The stamp is complete, and every number this call produced -- the fit's
    // residual and the judge's score alike -- was read before it existed. Same
    // disclosure as CLI `match`, from the other production stamper.
    // REFINE means "adjust MY edit", so it must not delete work the model was
    // never able to return. The strict response schema
    // (advisor::catalogue::edit_recipe_schema) can express only LINEAR and RADIAL
    // primary geometry; it carries no components, no enabled toggle, no colour
    // gains and no mask role — so a round-trip silently dropped every bitmap
    // mask (AI-selected sky/subject, painted, reverse-fit zones) with its
    // recolour gains. The result then auto-saves. Carry those back from the base
    // the user actually had — and, for the three manual lens fields the schema
    // DOES carry since R23-1b, keep the photographer's value only where this
    // proposal said nothing about them.
    carry_over_then_note_domain(&mut recipe, base, opinions, &mut det_notes);
    Ok((recipe, verdict, det_notes))
}

/// The LAST two steps of [`produce_recipe`]'s stamp, in the one order that is
/// right: the carry-over first, the domain note after it. The carry-over may
/// replace the lens profile the stamp just wrote with the one the
/// photographer's base actually has (its toggles included), and that profile
/// is part of the frame the note describes — so a note read before it could
/// name a domain shift the delivered recipe does not have, or miss one it
/// does. One function, so a test pins the order instead of the position of
/// two lines in a thousand-line body.
fn carry_over_then_note_domain(
    recipe: &mut EditRecipe,
    base: Option<&EditRecipe>,
    opinions: Opinions,
    det_notes: &mut Vec<crate::rationale::Note>,
) {
    if let Some(b) = base {
        carry_over_unrepresentable(recipe, b, opinions, Some(&mut *det_notes));
    }
    let shift = post_stamp_domain_shift(recipe);
    note_post_stamp_domain(&mut recipe.rationale, det_notes, shift);
}

/// "These are the looks it followed" — the transparency half of feedback #6
/// (R23-2). `None` when nothing was found: the gap note below covers that
/// case, and a "referenced 0 photos" line would be noise.
pub fn references_note(files: &[String]) -> Option<crate::rationale::Note> {
    if files.is_empty() {
        return None;
    }
    Some(crate::rationale::Note::new(
        crate::rationale::keys::STYLE_NEIGHBOURS,
        vec![("files", files.join(", ")), ("n", files.len().to_string())],
    ))
}

/// "This develop asked for the reference looks and got NOTHING" — the
/// disclosure that closes feedback #6's silent arms with ONE condition
/// (R23-2): keyed off the FINAL reference, so no library at all and a search
/// that found nothing share one note, while a known cause — an unusable
/// file, embedding switched off, a failed query — names itself.
///
/// A pure function so all three arms are testable without a paid call — the
/// production path reaches this only after minutes of network work.
pub(crate) fn style_gap_note(
    strength: f32,
    reference: Option<&str>,
    load_err: Option<&str>,
) -> Option<crate::rationale::Note> {
    // Nothing was asked for, or the reference arrived: nothing to disclose.
    // (A claim about a mechanism the user switched off is its own honesty bug.)
    if strength <= 0.0 || reference.is_some() {
        return None;
    }
    Some(match load_err {
        Some(e) => crate::rationale::Note::new(
            crate::rationale::keys::STYLE_UNAVAILABLE,
            vec![("e", crate::rationale::error_line(&anyhow::anyhow!("{}", e)))],
        ),
        None => crate::rationale::Note::new(
            crate::rationale::keys::STYLE_NO_REFERENCE,
            vec![("pct", format!("{:.0}", strength.clamp(0.0, 1.0) * 100.0))],
        ),
    })
}

// ── judge CONVERGENCE (R23-4, feedback #13) ────────────────────────────────
//
// R20 shipped a loop that could only ask "did that make it worse?"
// (`visual_review_round`'s do-no-harm `>=`). The judge's rubric has stated an
// ABSOLUTE scale since day one — `judge.rs`: 90+ ship as-is, 75-89 good with
// minor polish left, 50-74 clearly improvable — and nothing read it, so a
// develop scored 62 and stopped there as contentedly as one scored 91.
//
// These are the three target scores that scale, one per strength band. They are
// TASTE constants, not measurements: named and adjacent so a retune is one edit
// with the whole ladder visible, and pinned by
// `the_convergence_ladder_is_gated_and_ordered`.
const JUDGE_TARGET_RESTRAINED: f32 = 78.0;
const JUDGE_TARGET_BALANCED: f32 = 82.0;
const JUDGE_TARGET_COMMITTED: f32 = 88.0;

/// The score this analysis iterates toward, and the CAP on guided rounds.
///
/// Both scale with the strength axis: a restrained develop is finished sooner,
/// by definition, than one the photographer asked to commit. The cap is GATED,
/// though — R20's first定案 is that a paid visual round is an explicit caller
/// decision, and multiplying rounds multiplies that spend, so anything past the
/// historical single round needs the user to have asked for depth (`think`) or
/// for a bold grade (the committed band). Everything else keeps exactly the
/// round budget every release since R20 has had.
///
/// The gate is a DISJUNCTION and every cost disclosure has to read that way
/// (R23 review MED-2): `think` alone raises the ceiling, and a Strength above
/// 70% alone raises it too, with no box ticked — the worst case of 17 calls
/// (10 carrying images) belongs to EITHER, not to both together. The GUI's
/// three tooltips and the CLI's `--strength` help all say it disjunctively for
/// that reason.
///
/// `judge = false` (batch, eval) never reaches this: the whole block is behind
/// that gate, so the unattended surfaces cannot iterate at all.
pub(crate) fn judge_convergence(strength: GradeStrength, think: bool) -> (f32, usize) {
    let (target, cap) = match strength.tier() {
        StrengthTier::Restrained => (JUDGE_TARGET_RESTRAINED, 1),
        StrengthTier::Balanced => (JUDGE_TARGET_BALANCED, 2),
        StrengthTier::Committed => (JUDGE_TARGET_COMMITTED, 3),
    };
    let deep = think || strength.tier() == StrengthTier::Committed;
    (target, if deep { cap } else { 1 })
}

/// May the judge buy ANOTHER guided round, and with what instruction?
///
/// `Some(hint)` runs a round; `None` means the reviewed develop stands. The
/// FIRST round's test is R20's, unchanged and unconditional: a non-Accept
/// verdict carrying an instruction. `target` deliberately does NOT apply at
/// `rounds_done == 0` — a default-path analysis must behave exactly as it did
/// before R23-4 whatever the target says, and suppressing the historical round
/// because a 79 already cleared 78 would be a silent behaviour change dressed
/// up as convergence. From the second round on the target IS the stop
/// condition, so the loop finally knows the difference between "not worse" and
/// "good enough".
pub(crate) fn judge_next_round(
    rounds_done: usize,
    j: &crate::advisor::Judgement,
    target: f32,
    cap: usize,
) -> Option<&str> {
    if rounds_done >= cap || (rounds_done > 0 && j.score >= target) {
        return None;
    }
    // An Accept's hint (if any) is advice, not a request — only a
    // revise/reject verdict WITH an instruction buys the round.
    if j.decision == Decision::Accept {
        return None;
    }
    j.hint.as_deref().map(str::trim).filter(|h| !h.is_empty())
}

/// Outcome of one judge-guided visual revision round (R20 closed loop).
pub(crate) enum VisualRound {
    /// The revision re-scored at least as high — it replaces the recipe,
    /// and `verdict` is the data verifier's fresh word on it.
    Adopted {
        recipe: Box<EditRecipe>,
        verdict: Verdict,
        second: crate::advisor::Judgement,
    },
    /// The revision came back with IDENTICAL settings — nothing to verify or
    /// re-judge (two paid calls saved), and no "was adopted" claim for a
    /// recipe that never changed (review R20-S3).
    Unchanged,
    /// The revision re-scored LOWER — discarded (do-no-harm).
    KeptLower { second_score: f32 },
    /// Propose or verify failed — nothing to compare, keep the reviewed recipe.
    RoundFailed { e: String },
    /// The revision exists but could not be re-judged — an UNCOMPARED swap
    /// would gamble the reviewed recipe on it, so it is discarded.
    RejudgeFailed { e: String },
}

/// ONE guided revision, adopt-or-keep. Factored over closures so the
/// contract — a revision must re-judge AT LEAST AS HIGH to replace the
/// reviewed recipe, an identical revision short-circuits, and every failure
/// keeps the reviewed recipe — is testable without a network (the three
/// steps are remote in production).
pub(crate) fn visual_review_round(
    hint: &str,
    current: &EditRecipe,
    propose: impl FnOnce(&str) -> Result<EditRecipe, crate::advisor::AdvisorError>,
    verify: impl FnOnce(&EditRecipe) -> Result<Verdict, crate::advisor::AdvisorError>,
    judge: impl FnOnce(&EditRecipe) -> Result<crate::advisor::Judgement, crate::advisor::AdvisorError>,
    first_score: f32,
) -> VisualRound {
    let revised = match propose(hint) {
        Ok(r) => r,
        Err(e) => return VisualRound::RoundFailed { e: e.to_string() },
    };
    // SETTINGS equality, not struct equality: the model's prose (rationale,
    // confidence) differs every call, so raw == would never fire — the no-op
    // being caught is "same sliders/curves/masks", the render-identical case.
    let strip = |r: &EditRecipe| {
        let mut c = r.clone();
        c.rationale = String::new();
        c.confidence = 0.0;
        c
    };
    if strip(&revised) == strip(current) {
        return VisualRound::Unchanged;
    }
    let v2 = match verify(&revised) {
        Ok(v) => v,
        Err(e) => return VisualRound::RoundFailed { e: e.to_string() },
    };
    match judge(&revised) {
        // >= not >: at EQUAL scores the revision wins because the judge asked
        // for it — the hint was followed and nothing was lost.
        Ok(second) if second.score >= first_score => VisualRound::Adopted {
            recipe: Box::new(revised),
            verdict: v2,
            second,
        },
        Ok(second) => VisualRound::KeptLower { second_score: second.score },
        Err(e) => VisualRound::RejudgeFailed { e: e.to_string() },
    }
}

/// The ADJUST envelope: what the model is told when `base` is the edit the
/// photographer is looking at (2026-10-21, replacing the one-line refine
/// envelope). It names the frame it sees, asks for the COMPLETE recipe that
/// replaces the edit, and says how to select a part of the picture — an `ai`
/// mask by content, a `range` to confine it, a gradient for the rest — one
/// mask per named part, an existing mask reused by its byte-exact `name`.
pub(crate) fn adjust_envelope(base_json: &str, request: Option<&str>) -> String {
    format!(
        "ADJUST the photographer's CURRENT edit of this photograph as they ask — do not start \
         over. IMAGE 1 shows the photograph AS IT LOOKS NOW, developed from the negative by the \
         CURRENT EDIT below. Return the COMPLETE EditRecipe that REPLACES that edit — it is \
         applied to the negative, not on top of the picture you see — keeping every value the \
         request does not touch and changing exactly what it implies. Use the whole control \
         set the catalogue lists (tone, white balance, presence, HSL, colour grading, curves, \
         detail, framing) AND the local masks: when the request names a PART of the picture — \
         the sky, the subject, a person, a building, the foreground — put the change on a mask \
         instead of the globals. A kind=ai mask is cut by a local segmenter after you answer: \
         what=sky, what=subject (the main subject), or what=object under the point ref_x/ref_y \
         (0..1 frame coordinates, origin top-left), and for people what=face, what=eyes or \
         what=background (everything but the person), found on the photo itself without a \
         point; add a `range` (luminance or colour) to \
         confine any mask further; use linear and radial masks for gradients and vignettes. \
         One mask per named part: to change a mask the CURRENT EDIT already has, return it \
         with its `name` byte-exact (its selection is kept) rather than adding a second one on \
         the same part, and never stack two masks on one part. CURRENT EDIT (EditRecipe JSON): \
         {base_json}. REQUEST: {}",
        request.unwrap_or("make a small, tasteful improvement")
    )
}

/// Re-attach what the AI's response schema CANNOT express, from the refine
/// base the photographer actually had.
///
/// `advisor::catalogue::edit_recipe_schema` can encode only LINEAR and RADIAL
/// primary geometry; it carries no components, no enabled toggle, no colour
/// gains and no mask role. That loss list is the `engine_only` column of
/// `advisor::catalogue::LOCAL_CONTROLS` / `RECIPE_CONTROLS` plus the Bitmap
/// geometry the schema omits — re-checked at R23-1b, which SHRANK it: the
/// radial `angle` and the three manual lens fields are in the schema now, so
/// neither is carried blindly any more (the lens trio is settled by `lens`
/// below, and a rotated ellipse is simply returned). A missing bitmap mask in
/// the response carries NO intent — the model had no way to return one — yet
/// the refined recipe auto-saves, so every AI-selected sky/subject mask,
/// painted mask and reverse-fit zone (with its recolour gains) silently
/// disappeared the moment the user clicked Refine.
///
/// `opinions` is the set of NULLABLE controls the model actually stated
/// (R23-1b for the three manual lens controls; 2026-10-21 for the post-crop
/// vignette, the grain, the B&W treatment and the Calibration panel). The
/// overwrite below used to be unconditional, with a sound reason at the time
/// — the model never saw those fields, so its zeros meant nothing. Now that it
/// can state them, an unconditional overwrite would make the schema addition
/// a no-op on exactly the path (a refine or an adjust) where a hand-dialled
/// value is most likely to exist; a null is "no opinion" and the base's value
/// is carried, a number is the answer and stands. `Opinions::default()`
/// (nothing stated) reproduces the historical behaviour exactly, which is
/// what every non-schema proposal passes.
pub(crate) fn carry_over_unrepresentable(
    recipe: &mut EditRecipe,
    base: &EditRecipe,
    opinions: Opinions,
    notes: Option<&mut Vec<crate::rationale::Note>>,
) {
    use crate::recipe::{MaskGeometry, MaskRole};

    let schema_loses = |m: &crate::recipe::LocalAdjustment| {
        matches!(&m.mask, MaskGeometry::Bitmap { .. })
            || !m.components.is_empty()
            || !m.enabled
            || m.color_gains.is_some()
            || m.role != MaskRole::Custom
    };

    let mut carried_indices = Vec::new();
    let mut unmatched = false;
    for (base_index, original) in base.masks.iter().enumerate().filter(|(_, m)| schema_loses(m)) {
        let returned_matches: Vec<usize> = recipe
            .masks
            .iter()
            .enumerate()
            .filter_map(|(i, m)| (m.name == original.name).then_some(i))
            .collect();
        let base_name_is_unique = !original.name.is_empty()
            && base.masks.iter().filter(|m| m.name == original.name).count() == 1;

        if base_name_is_unique && returned_matches.len() == 1 {
            let refined = &mut recipe.masks[returned_matches[0]];
            // Only the raster geometry is un-returnable now: the model can
            // state a radial `angle` (R23-1b), so a rotated ellipse it sends
            // back IS its answer and re-imposing the base's rotation would
            // ignore it.
            //
            // RASTER-BACKED, not `Bitmap` (`render::is_raster_backed`): the
            // reverse-fit's sky/land zones are Select Sky components now
            // (`recipe::MaskGeometry::select_sky`), and asking the variant
            // would have let a schema response replace a zone's geometry with
            // whatever shape it named — orphaning the fit's own alpha and
            // putting the zone's dials on a gradient. That is precisely the
            // "the reverse-fit zone silently disappeared the moment the user
            // clicked Refine" defect this function exists for.
            if crate::render::is_raster_backed(&original.mask) {
                refined.mask = original.mask.clone();
            }
            refined.components = original.components.clone();
            refined.enabled = original.enabled;
            refined.color_gains = original.color_gains;
            refined.role = original.role;
        } else if crate::render::is_raster_backed(&original.mask)
            && original.name.is_empty()
        {
            // No schema response can be a round-trip copy of an unnamed raster
            // selection, so preserve the existing prepend behaviour. The zone
            // masks reach this arm exactly as they did as bitmaps: their `name`
            // is empty by construction (the display label comes from
            // `MaskRole::en_name`).
            carried_indices.push(base_index);
        } else {
            // A state-bearing mask the response did not identifiably return.
            // The returned list may hold its RENAMED refined copy — keeping
            // both applies the coverage twice, and a same-name discard only
            // covered the obedient half: a renamed copy sailed through it.
            // No guess is safe, so the wholesale fallback below takes over.
            unmatched = true;
        }
    }

    if unmatched {
        // Conservative wholesale fallback: the base masks stand unchanged and
        // the response's MASK edits are discarded (its global refinements
        // stay). The prompt pins mask names, so this is the disobedient-
        // response path — and the rationale says what happened, because a
        // silent revert here reads as "the model ignored my masks".
        recipe.masks = base.masks.clone();
        let note = crate::rationale::Note::plain(crate::rationale::keys::MASKS_NOT_PRESERVED);
        recipe.rationale.push_str(&crate::rationale::render_one(&note));
        if let Some(v) = notes
            && v.len() < crate::rationale::MAX_NOTES
        {
            v.push(note);
        }
    } else {
        carried_indices.sort_unstable();
        carried_indices.dedup();
        if !carried_indices.is_empty() {
            let proposed = std::mem::take(&mut recipe.masks);
            recipe.masks =
                carried_indices.into_iter().map(|i| base.masks[i].clone()).collect();
            recipe.masks.extend(proposed);
        }
    }
    // Manual lens corrections are geometry the photographer dialled in, and a
    // response that says NOTHING about them (null, or a proposer with no such
    // field at all) must not re-warp the frame by defaulting them. A response
    // that DOES state one is answering the question it was asked — the
    // catalogue tells it null means "no opinion" and 0 means "zero it" — so
    // that value stands.
    // A NULLABLE control is carried only where this proposal said nothing
    // about it: null was "no opinion", a number (or the switch's boolean) is
    // the answer. Pinned by `an_unlocked_control_is_carried_only_where_the_
    // model_said_nothing`, which derives the field list from the registry.
    macro_rules! carry_unless_stated {
        ($($field:ident),* $(,)?) => {
            $( if !opinions.stated(stringify!($field)) { recipe.$field = base.$field; } )*
        };
    }
    carry_unless_stated!(lens_distortion, lens_vignette, lens_vignette_mid);
    // The lens PROFILE (with the user's toggles) rides too — but only when
    // the base actually carries one: a refine caller that still pre-strips
    // it to Default is sending a sentinel, never "the user turned everything
    // off" (a real profile keeps its in-camera data vectors even with every
    // toggle off, and a data-less photo's profile equals Default on both
    // sides). Without this, Refine reverted unsaved lens toggles to the
    // saved profile stamped above.
    if base.lens_profile != Default::default() {
        recipe.lens_profile = base.lens_profile.clone();
    }
    // The nine CARRIED effects (R25 B2) ride too, and they have no other way
    // home: they are `engine_only`, so the response schema cannot restate
    // them, and unlike the calibration nothing re-stamps them per photo. A
    // refine that dropped them would strip the photographer's imported
    // Lightroom grain / post-crop vignette from the recipe AND — because we
    // own those keys now, so the merge strips before rewriting — from the
    // sidecar beside the RAW on the very next save. That is data loss, not a
    // missing feature. `carried_effects_survive_a_refine` re-derives the field
    // list from `Tier::CarriedOnly`, so the B3/B4 rows fail loudly here until
    // this block is widened with them.
    // The colour field rides for the same reason and with the same force
    // (R33 §G): it is `engine_only`, so no response can restate it, and
    // nothing re-stamps it per photo. A refine that dropped it would silently
    // delete a solved field the user can see in the canvas.
    recipe.colour_field = base.colour_field.clone();
    // …and so does every other engine-only carrier with no `crs:` key
    // (2026-10-13): the imported spot removals, the generative-fill layers and
    // the AI lens blur. Each is pixels or geometry measured from THIS photo,
    // the schema never asks for them, and the GUI lands a refine by replacing
    // the card's recipe whole — so a refine without these lines silently
    // deleted the removals and the layers. Pinned from the registry by
    // `every_crs_less_engine_carrier_survives_a_refine`.
    recipe.retouch = base.retouch.clone();
    recipe.pixel_layers = base.pixel_layers.clone();
    recipe.lens_blur = base.lens_blur.clone();
    carry_unless_stated!(post_crop_vignette);
    carry_unless_stated!(post_crop_vignette_mid);
    carry_unless_stated!(post_crop_vignette_feather);
    carry_unless_stated!(post_crop_vignette_round);
    carry_unless_stated!(post_crop_vignette_style);
    carry_unless_stated!(post_crop_vignette_hl);
    recipe.lens_profile_distortion_scale = base.lens_profile_distortion_scale;
    recipe.lens_profile_vignetting_scale = base.lens_profile_vignetting_scale;
    // v1.5.0 F6: the Transform panel. Engine-only, so no response restates any
    // of it, and one of them — `upright_transform` — is Adobe's own measurement
    // of this photograph, which nothing on this side could recompute. A refine
    // that dropped these would straighten a building and then un-straighten it
    // on the next save.
    recipe.perspective_vertical = base.perspective_vertical;
    recipe.perspective_horizontal = base.perspective_horizontal;
    recipe.perspective_rotate = base.perspective_rotate;
    recipe.perspective_scale = base.perspective_scale;
    recipe.perspective_aspect = base.perspective_aspect;
    recipe.perspective_x = base.perspective_x;
    recipe.perspective_y = base.perspective_y;
    recipe.perspective_upright = base.perspective_upright;
    recipe.upright_transform = base.upright_transform.clone();
    recipe.crop_constrain_to_warp = base.crop_constrain_to_warp;
    // v1.5.0 F7: which rendering this photograph starts from. Neither is a
    // number the model was asked for, and both are the photographer's own
    // choice made in Lightroom — a refine that dropped them would develop the
    // next save through a different profile than the one on screen.
    recipe.camera_profile = base.camera_profile.clone();
    recipe.look = base.look.clone();
    // The effects, the B&W treatment and the calibration (below) are
    // NULLABLE rows since 2026-10-21: the base's value rides only where the
    // response answered null.
    carry_unless_stated!(grain);
    carry_unless_stated!(grain_size);
    carry_unless_stated!(grain_rough);
    // R25 B3 widened the block by fifteen: the eight detail axes (carried
    // then, rendered since v1.5.0 and engine-only either way), the auto-CA
    // switch and the six de-fringe keys. Same argument, one sharper edge —
    // de-fringe's neutral is ADOBE'S DEFAULT, not zero, so a dropped hue
    // window would not merely lose a value, it would write 0/0 into the
    // sidecar and change what Lightroom renders.
    recipe.sharpen_radius = base.sharpen_radius;
    recipe.sharpen_detail = base.sharpen_detail;
    recipe.sharpen_mask = base.sharpen_mask;
    recipe.nr_detail = base.nr_detail;
    recipe.nr_contrast = base.nr_contrast;
    recipe.color_nr = base.color_nr;
    recipe.color_nr_detail = base.color_nr_detail;
    recipe.color_nr_smooth = base.color_nr_smooth;
    // v1.5.0: which of the carried COMPANIONS hold a real zero rides with
    // them. Engine-only bookkeeping with no `crs:` spelling, so no response
    // restates it — and without it a Detail the photographer set to 0 would
    // come home as "absent" and render at Lightroom's 25.
    recipe.explicit_zero = base.explicit_zero.clone();
    recipe.auto_lateral_ca = base.auto_lateral_ca;
    recipe.defringe_purple = base.defringe_purple;
    recipe.defringe_purple_lo = base.defringe_purple_lo;
    recipe.defringe_purple_hi = base.defringe_purple_hi;
    recipe.defringe_green = base.defringe_green;
    recipe.defringe_green_lo = base.defringe_green_lo;
    recipe.defringe_green_hi = base.defringe_green_hi;
    // The manual CA pair rides too, and it is the ONE pair here that RENDERS.
    // It is `engine_only` for a reason of its own (catalogue.rs: a 1–3 px
    // edge artefact the advisor's ~1024 px preview cannot show), which means
    // the schema never asks for it and it has no other way home — exactly the
    // carried case, minus the tier. `carried_effects_survive_a_refine` covers
    // it explicitly, since the derivation there is by `Tier::CarriedOnly`.
    recipe.ca_r = base.ca_r;
    recipe.ca_b = base.ca_b;
    // v1.5.0: the parametric tone curve rides on the Detail axes' ground — it
    // renders, it is engine-only (the model states the point curve instead),
    // and its keys are owned, so a dropped region would take the
    // photographer's Lightroom curve out of the sidecar on the next save.
    recipe.param_shadows = base.param_shadows;
    recipe.param_darks = base.param_darks;
    recipe.param_lights = base.param_lights;
    recipe.param_highlights = base.param_highlights;
    recipe.param_shadow_split = base.param_shadow_split;
    recipe.param_midtone_split = base.param_midtone_split;
    recipe.param_highlight_split = base.param_highlight_split;
    // v1.5.0: the Calibration panel, the B&W treatment and the point colours
    // ride on the same ground — they render, they are engine-only (the model
    // states colour through the mixer and the grade), and their sidecar
    // properties are owned, so a dropped value would take the photographer's
    // own Lightroom calibration, grey mix or swatches out of the file on the
    // next save.
    carry_unless_stated!(cal_shadow_tint);
    carry_unless_stated!(cal_red_hue);
    carry_unless_stated!(cal_red_sat);
    carry_unless_stated!(cal_green_hue);
    carry_unless_stated!(cal_green_sat);
    carry_unless_stated!(cal_blue_hue);
    carry_unless_stated!(cal_blue_sat);
    carry_unless_stated!(convert_to_grayscale);
    carry_unless_stated!(gray_red);
    carry_unless_stated!(gray_orange);
    carry_unless_stated!(gray_yellow);
    carry_unless_stated!(gray_green);
    carry_unless_stated!(gray_aqua);
    carry_unless_stated!(gray_blue);
    carry_unless_stated!(gray_purple);
    carry_unless_stated!(gray_magenta);
    recipe.point_colors = base.point_colors.clone();
    // v1.5.0 F8: HDR edit mode, its headroom and the seven SDR-rendition
    // controls, on the same ground again — rendered, engine-only, owned keys.
    // The edge here is the MODE: it is the gate the other eight render behind,
    // so a Refine that dropped it would not merely lose a slider, it would
    // switch off a photograph's whole rendition and write that decision back
    // into the sidecar, with the seven still sitting in the file saying
    // otherwise.
    recipe.hdr_edit = base.hdr_edit;
    recipe.hdr_max_ev = base.hdr_max_ev;
    recipe.sdr_blend = base.sdr_blend;
    recipe.sdr_brightness = base.sdr_brightness;
    recipe.sdr_contrast = base.sdr_contrast;
    recipe.sdr_highlights = base.sdr_highlights;
    recipe.sdr_shadows = base.sdr_shadows;
    recipe.sdr_whites = base.sdr_whites;
    recipe.sdr_clarity = base.sdr_clarity;
    // R25 B4: the Transform / profile-name pass-through block rides for the
    // SAME reason and with the sharpest edge of all. It is `EngineCarrier`, so
    // no response can restate it; we now OWN its nine keys, so the next
    // save's merge strips them out of the user's sidecar before rewriting —
    // and the writer emits only what the map holds. A Refine that dropped this
    // map would therefore delete the photographer's Upright correction and
    // camera profile from the file beside their RAW, permanently, without a
    // single control having moved. `carried_effects_survive_a_refine` derives
    // its field list from the tiers that render nothing, so this line is
    // required to exist by the same test the B2/B3 blocks answer to.
    recipe.passthrough = base.passthrough.clone();
    carry_radial_carried_attributes(recipe, base);
    // THE ERA RULE (v1.5.0): the refined recipe is no more current than the
    // base it copied the lines above from. The advisor stamps its response
    // `SCHEMA_ERA` — every number the model returns answers this build's
    // control list — but every engine-only value above is the BASE's, and a
    // base older than a control holds that control's serde default, not a
    // value. Stamped current, that default would OWN its key, and the next
    // save would strip the photographer's own Lightroom value from the sidecar
    // (a v1.4 recipe's Refine deleting its parametric curve). A key the model
    // MOVED still publishes, because the era gate releases any key that left
    // its default (`xmp::unspoken_attr_keys`).
    recipe.schema_era = recipe.schema_era.min(base.schema_era);
    recipe.clamp(); // the size caps still apply after re-attaching
}

/// `crs:Midpoint` and `crs:Version` ride home from the refine base (R27 L-09).
///
/// The two are CARRIED-ONLY radial attributes: Lightroom writes them on every
/// radial, this engine never consults them, and `advisor::catalogue`'s mask
/// geometry schema deliberately omits them — a model cannot know a value it
/// never saw, and a required property is a property it would invent. The cost
/// was that a Refine returned the geometry with both at their serde defaults
/// (50 / 2), so refining an imported Lightroom radial silently rewrote the
/// photographer's Midpoint on the very next save.
///
/// R25 registered this as 「not worth fixing」 on the ground that the fix meant
/// widening `schema_loses` above — the state-bearing predicate that decides
/// which masks take the WHOLESALE-REVERT path. That reasoning stands, so this
/// does not touch it: it is a separate pass over the matched masks, it can
/// neither set `unmatched` nor change which masks are carried, and a base
/// whose masks the response did not identifiably return has already had them
/// restored verbatim by then (making this a self-copy).
///
/// Matching is the SAME rule the block above uses — the base name is unique
/// and exactly one returned mask answers to it — and only Radial → Radial
/// copies: a response that sent back a different geometry KIND is a new shape,
/// with no midpoint of the old one to inherit.
fn carry_radial_carried_attributes(recipe: &mut EditRecipe, base: &EditRecipe) {
    use crate::recipe::MaskGeometry;
    let carried = |g: &MaskGeometry| match g {
        MaskGeometry::Radial { midpoint, mask_version, .. } => Some((*midpoint, *mask_version)),
        _ => None,
    };
    for original in base.masks.iter() {
        let Some((midpoint, mask_version)) = carried(&original.mask) else { continue };
        if original.name.is_empty()
            || base.masks.iter().filter(|m| m.name == original.name).count() != 1
        {
            continue;
        }
        let answers: Vec<usize> = recipe
            .masks
            .iter()
            .enumerate()
            .filter_map(|(i, m)| (m.name == original.name).then_some(i))
            .collect();
        let [i] = answers[..] else { continue };
        if let MaskGeometry::Radial { midpoint: mp, mask_version: mv, .. } =
            &mut recipe.masks[i].mask
        {
            *mp = midpoint;
            *mv = mask_version;
        }
    }
}

/// Was this saved curve estimated by an EARLIER calibration era?
///
/// Every non-empty `base_curve` the store holds was estimated by the build
/// that wrote it (no surface edits one by hand), and the estimator has
/// changed twice: era 1 read a washed working develop, era 2 matched pixel
/// histograms and took the rendition's texture for tone
/// (`render::camera_base_knots`). A curve stamped below [`CALIB_ERA`] is
/// therefore re-estimated on the next open, whatever its shape. Until
/// 2026-09-23 only an era-1 curve that LOOKED washed was (every interior
/// input above the half and the curve darkening somewhere), and an era-2
/// curve never, so a photo saved under either kept its old look for good —
/// the operator ruled that it must not.
///
/// An EMPTY curve is left alone: it is the identity verdict, or a recipe
/// written before base looks existed, and every legacy develop was tuned on
/// exactly that rendering. Endpoints alone (fewer than three knots) are the
/// same nothing.
///
/// [`CALIB_ERA`]: crate::recipe::CALIB_ERA
pub fn base_curve_is_pre_era(version: u32, curve: &[[f32; 2]]) -> bool {
    version < crate::recipe::CALIB_ERA && curve.len() >= 3
}

/// Successful estimates already computed this run, keyed by photo identity —
/// including the EMPTY answer, which is the estimator's identity verdict
/// ("this photo needs no base look"), but never an inability: those must
/// retry once the file is readable again.
///
/// The repair is asked on every open AND, since the render funnel carries it,
/// on every render of an affected photo. Each estimate costs a RAW decode plus
/// a working-resolution develop, and the answer cannot change while the
/// process runs, so paying it once per photo is the difference between a
/// correction and a stall on the UI thread.
///
/// Identity = size + mtime (the thumbnail cache's convention): an in-place
/// replacement is caught whenever it changes either; an equal-size,
/// timestamp-preserving swap is NOT, and content-hashing a 50-120 MB RAW per
/// check would cost the very read this memo exists to avoid.
type CurveMemoKey = (PathBuf, CurveIdent);

fn fresh_curve_memo()
-> &'static std::sync::Mutex<std::collections::HashMap<CurveMemoKey, Vec<[f32; 2]>>> {
    static MEMO: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<CurveMemoKey, Vec<[f32; 2]>>>,
    > = std::sync::OnceLock::new();
    MEMO.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// The (size, mtime) half of the memo key. CAPTURE IT BEFORE READING the
/// file: an answer must be filed under the identity of the content it was
/// computed FROM, so a file replaced mid-read lands under the OLD key and
/// the next reader (new identity) recomputes. The repair does this; primers
/// must too — stat'ing after a multi-second develop filed a mid-open
/// replacement's NEW identity with the OLD content's answer, permanently.
pub type CurveIdent = (u64, Option<std::time::SystemTime>);

pub fn curve_ident(raw: &Path) -> CurveIdent {
    std::fs::metadata(raw).ok().map(|m| (m.len(), m.modified().ok())).unwrap_or((0, None))
}

/// Seed the memo with an ANSWER a caller already computed. The GUI open
/// worker develops the neutral and CDF-matches the camera rendition anyway;
/// discarding that answer made every later repair site (Ctrl+Z's
/// apply_step, a variant switch, Ctrl+S) pay a second full decode + develop
/// on the UI thread. Answers only — an inability must never be primed (it
/// stays uncached so the next reader retries). FIRST ANSWER WINS
/// (`or_insert`): the memo's contract is that the answer cannot change
/// while the process runs, and the GUI's working edge is a user preference
/// — overwriting on a resolution switch would have made the repaired curve
/// a function of that preference. The caller's edge may differ from the
/// repair's 2048 cap; each side is box-binned to <=1024 INDEPENDENTLY (the
/// neutral by `render::camera_base_look`'s cap, the camera side inside
/// `camera_base_knots`), so answers agree within the estimator's documented
/// tolerance — not byte-identically, which is exactly why one answer is
/// recorded and kept.
pub fn prime_curve_memo(raw: &Path, ident: CurveIdent, knots: Vec<[f32; 2]>) {
    if let Ok(mut m) = fresh_curve_memo().lock() {
        m.entry((raw.to_path_buf(), ident)).or_insert(knots);
    }
}

/// Re-estimate a base curve that an earlier calibration era saved, and say
/// so. Returns the disclosure when the curve was replaced.
///
/// The estimate runs on a capped develop ([`photo_base_knots`]), the result
/// is persisted like any user edit, and `build_tone_lut` composes it under
/// the full-resolution render — so an estimator's defect reaches every
/// deliverable of every photo saved under it: era 1's washed working develop
/// rendered several stops dark once batch 43 fixed the sampler (the stored
/// curve was then laid over a correct develop), and era 2's pixel-histogram
/// match put grain back into a night sky. This is the one place a saved
/// curve is replaced, and it replaces only with an ANSWER.
pub fn repair_pre_era_base_curve(raw: &Path, r: &mut EditRecipe) -> Option<String> {
    repair_pre_era_base_curve_in(raw, r, &crate::diag::photo(raw))
}

/// [`repair_pre_era_base_curve`] on a caller-supplied channel.
///
/// The estimator below discloses three separate inabilities, and under a pooled
/// `batch` they used to reach the process stderr in completion order. A caller
/// that owns a transcript block hands its own sink here instead; the wrapper
/// above keeps every GUI/web/store call site on the process default, byte for
/// byte as it was.
pub fn repair_pre_era_base_curve_in(
    raw: &Path,
    r: &mut EditRecipe,
    d: &crate::diag::Diag,
) -> Option<String> {
    if !base_curve_is_pre_era(r.version, &r.base_curve) {
        return None;
    }
    let ident = curve_ident(raw);
    let key = (raw.to_path_buf(), ident);
    let memo = fresh_curve_memo();
    let cached = memo.lock().ok().and_then(|m| m.get(&key).cloned());
    let fresh = match cached {
        Some(c) => Some(c),
        None => {
            let c = photo_base_knots_checked_in(raw, d);
            // ANSWERS are cached — the empty identity verdict included. An
            // inability (`None`) is not: it means the decode, the preview or
            // the neutral develop did not work THIS time (a locked file, a
            // disconnected share), and caching it would leave the photo
            // unrepaired for the rest of the process even after it became
            // readable. FIRST ANSWER WINS here too: the lock was released
            // across the estimate, and the open worker can prime this very
            // key in that gap (the same-path keep-flight is where the two
            // threads meet) — a bare insert would put a different curve in
            // the memo than the one this call installs, so the repair
            // ADOPTS whichever answer won.
            match (c, memo.lock()) {
                (Some(knots), Ok(mut m)) => Some(m.entry(key).or_insert(knots).clone()),
                (c, _) => c,
            }
        }
    };
    // Tri-state, and the difference is the whole repair: an ANSWER replaces
    // the washed curve — the EMPTY answer too, which says "this photo needs
    // no base look" (treating it as a failure left the washed curve rendering
    // stops-dark forever, and re-paid the full decode + develop on every
    // reader, because inabilities are uncached). An INABILITY leaves the
    // curve AND the era-1 stamp alone, so the next reader retries.
    let knots = fresh?;
    r.base_curve = knots;
    r.version = crate::recipe::CALIB_ERA;
    Some(
        "this photo's camera base look was re-estimated: it was saved by an earlier version, \
         whose estimate of the camera's tone this version replaces"
            .to_string(),
    )
}

/// What [`migrate_recipe_coord_frame`] did, for the caller's disclosure —
/// three separate facts, each said in its own sentence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoordMigration {
    /// The orientation an era-0 recipe's geometry was turned by (EXIF plus
    /// the photographer's turns); `Normal` when the recipe was already in the
    /// display frame.
    pub orientation: rawler::Orientation,
    /// Sensor pixels the geometry was translated by — the develop window's
    /// move of 2026-09-21 ([`crate::decode::SourceWindow::legacy_shift`]);
    /// `(0, 0)` for a photo whose window never moved.
    pub shift: (i32, i32),
    /// Raster mask files re-written into the corrected frame (turned and/or
    /// translated) under names derived from the operation, originals kept.
    /// Until v1.6.0 a raster was the one thing this migration could only
    /// disclose; it is re-written now, so nothing is left to apologise for.
    pub rasters: usize,
}

impl CoordMigration {
    /// Did the geometry TURN (as opposed to only translate)?
    pub fn turned(&self) -> bool {
        !matches!(self.orientation, rawler::Orientation::Normal | rawler::Orientation::Unknown)
    }
}

/// The photo's SOURCE frame and its EXIF turn — [`crate::decode::source_frame`]
/// with a memo, keyed like the curve memo.
///
/// Both halves, not just the orientation: reading either costs a full
/// `RawSource::new` (rawler slurps the whole file, 60–120 MB for a 61 MP ARW),
/// `source_frame` answers both from ONE header walk, and since R29 C1 the
/// migration needs the SIZE as well as the turn — a brush's radii are in width
/// units and a quarter turn rescales them by the frame aspect
/// ([`crate::render::CoordFrame`]). The strip-card and batch-export paths ask
/// per recipe, not per photo, which is what the memo is for.
type SourceFrame = ((usize, usize), rawler::Orientation);

/// The memo holds the WHOLE [`crate::decode::SourceWindow`] — the frame, the
/// turn and, since v1.6.0, the legacy window shift the era-2 migration needs —
/// so its readers still cost one header walk per photo per process.
fn orient_memo(
) -> &'static std::sync::Mutex<std::collections::HashMap<CurveMemoKey, crate::decode::SourceWindow>> {
    static MEMO: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<CurveMemoKey, crate::decode::SourceWindow>>,
    > = std::sync::OnceLock::new();
    MEMO.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// [`orient_memo`]'s reader, and the ONE door to a photograph's un-rotated
/// rectangle plus its own EXIF turn. Inabilities are NOT cached — a locked
/// file must be retried, the same rule the curve memo follows.
///
/// `pub(crate)` since W14: the XMP import needs the EXIF state to solve the
/// sidecar orientation law (`render::quarter_turns_between`), and it needs the
/// rectangle for the same brush rescale [`photo_frame_aspect`] needs it for.
/// Three consumers, one header walk per photo per process — before this the
/// frame supply had a second private memo of its own, so a photo could be
/// walked twice for one fact.
pub(crate) fn source_frame_memo(path: &Path) -> Option<SourceFrame> {
    source_window_memo(path).map(|w| (w.size, w.exif))
}

/// [`source_frame_memo`] with the third answer the era-2 migration reads
/// ([`crate::decode::SourceWindow::legacy_shift`]): the same memo, the same
/// retry-on-inability rule.
pub(crate) fn source_window_memo(path: &Path) -> Option<crate::decode::SourceWindow> {
    let key = (path.to_path_buf(), curve_ident(path));
    if let Some(hit) = orient_memo().lock().ok().and_then(|m| m.get(&key).copied()) {
        return Some(hit);
    }
    let answer = crate::decode::source_window(path).ok()?;
    Some(match orient_memo().lock() {
        Ok(mut m) => *m.entry(key).or_insert(answer),
        Err(_) => answer,
    })
}

/// Bring a saved recipe's geometry into THIS build's display frame — the
/// load-time half of [`crate::recipe::COORD_ERA`], two eras deep.
///
/// **Era 0 → 1 (v0.30.0): the turn.** Every recipe written up to v0.29.x
/// stored its crop rectangle and mask geometries against the frame the app
/// actually drew, and for a rotated/flipped RAW that was the SENSOR frame:
/// rawler 0.7.2 reports `Orientation::Normal` for everything but DNG/QTK, so
/// a portrait ARW was displayed sideways and the user drew on a sideways
/// canvas. Now that `render::orient_f32` turns the frame for real, those
/// coordinates land on the wrong axis unless they are turned with it.
///
/// **Era 1 → 2 (v1.6.0): the translation.** On 2026-09-21 the develop window
/// of a body that declares a crop but no active area moved from the sensor's
/// corner onto the declared origin (`decode::align_default_crop`; the
/// ILCE-7RM4A files: `(32, 20)`). The window kept its size and every stored
/// coordinate is normalised to the frame, so a recipe saved by any earlier
/// release keeps its numbers while the picture under them moved by 0.34 % of
/// the width and 0.32 % of the height. [`crate::decode::SourceWindow::
/// legacy_shift`] says how far, per file; the vector is turned into the
/// display frame by `render::orient_vector` and every coordinate goes through
/// `render::shift_recipe_coords`. Raster masks are FILES and are re-written
/// as translated (and, for an era-0 recipe, turned) files — the gap the first
/// era could only disclose is closed with the same stroke, see
/// [`migrate_raster_file`].
///
/// Tri-state, the [`repair_pre_era_base_curve`] discipline:
///   * already era-current, or a photo whose frame neither turns nor moves →
///     stamp, no note;
///   * turned and/or translated → stamp AND say so;
///   * the frame could NOT be read (unreadable/locked RAW) → `Ok(None)`,
///     nothing stamped, nothing moved, so the next reader retries; a raster
///     that cannot be read or re-written → `Err`, the same nothing-moved
///     state, and the caller says so. Stamping on an inability would declare
///     the coordinates migrated when they are not, and no later reader could
///     tell.
pub fn migrate_recipe_coord_frame(
    raw: &Path,
    r: &mut EditRecipe,
) -> std::io::Result<Option<CoordMigration>> {
    if r.coord_era >= crate::recipe::COORD_ERA {
        return Ok(None);
    }
    // A baked image is not a RAW: `decode::load_image` has applied its EXIF
    // orientation since long before this era, and its window never moved
    // (`legacy_shift` is zero by construction), so its saved coordinates are
    // already display-frame. Stamp and stop — asking rawler would only fail.
    if !crate::decode::is_raw(raw) {
        r.coord_era = crate::recipe::COORD_ERA;
        return Ok(None);
    }
    let Some(window) = source_window_memo(raw) else {
        return Ok(None);
    };
    migrate_recipe_geometry(raw, r, window)
}

/// The frame-independent half of [`migrate_recipe_coord_frame`]: what to do
/// to `r` once the photo's [`crate::decode::SourceWindow`] is known. Split
/// off so the whole path — geometry, rasters, the stamp, the second pass that
/// finds nothing to do, the refusal — is exercised without a decodable RAW.
///
/// **All-or-nothing, in memory AND on disk**, the [`rotate_recipe`] rule:
/// every raster is re-written BEFORE the recipe is touched, and a failure
/// returns with `r` exactly as it was, era stamp included. The files written
/// before the failure are NOT deleted, unlike the rotate's fresh claims: their
/// names are derived from the operation, so the next attempt finds them and
/// goes straight to the one that failed — nothing accumulates.
pub(crate) fn migrate_recipe_geometry(
    src: &Path,
    r: &mut EditRecipe,
    window: crate::decode::SourceWindow,
) -> std::io::Result<Option<CoordMigration>> {
    // The era gate once more, cheaply: the funnel above checks it before it
    // pays for the window, and a caller that has the window in hand (the
    // tests, a future second pass) must find the same wall.
    if r.coord_era >= crate::recipe::COORD_ERA {
        return Ok(None);
    }
    let (sw, sh) = window.size;
    // The composed orientation, not the EXIF one (R27): era 0 means "these
    // numbers are in the SENSOR frame", and the frame they must reach is the
    // one this build DISPLAYS — EXIF plus the photographer's own turns. Today
    // the two are always equal here (`quarter_turns` did not exist before
    // v0.33, and no era-0 recipe can carry one), so this is correctness by
    // construction rather than a behaviour change; writing `raw_orientation`
    // alone would be a latent bug the day a v0.33 recipe is hand-edited to
    // era 0. The era-2 translation, on the other hand, is applied in the
    // DISPLAY frame of an era-1 recipe, which IS this composed state — so the
    // same value serves both halves.
    let orientation = crate::render::compose_orientation(window.exif, r.quarter_turns);
    let turn = (r.coord_era == 0
        && !matches!(orientation, rawler::Orientation::Normal | rawler::Orientation::Unknown))
    .then_some(orientation);
    // The translation: `legacy_shift` is where this build's window sits
    // RELATIVE to the old one, in sensor pixels, so the picture — and every
    // coordinate drawn on it — moves the other way, by that many pixels over
    // the frame's own size, and then into the display frame by the same
    // linear map the pixels take.
    let (dx, dy) = window.legacy_shift;
    let shift = if (dx, dy) == (0, 0) || sw == 0 || sh == 0 {
        (0.0, 0.0)
    } else {
        crate::render::orient_vector(orientation, -(dx as f32) / sw as f32, -(dy as f32) / sh as f32)
    };
    if turn.is_none() && shift == (0.0, 0.0) {
        // The stamp lands once the frame is KNOWN: an unrotated photo whose
        // window never moved is already in this era's frame, and leaving it
        // behind would pay the metadata read again on every future load.
        r.coord_era = crate::recipe::COORD_ERA;
        return Ok(None);
    }

    // --- Phase 1: every raster into its re-written file. Nothing in `r`
    // moves until all of them are on disk.
    //
    // WHICH rasters differs by half. A turn (era 0) walks
    // `turnable_raster_paths_mut`: `orient_recipe_coords` DROPS any other AI
    // mask's cached alpha so the next develop re-segments at the turned
    // point, and re-writing a file phase 2 then forgets is the orphan R28
    // Batch-3 3b closed. A translation alone walks `bitmap_paths_mut`, the
    // caches included: a translated alpha IS the alpha of the translated
    // picture (the same pixels, moved), and keeping it is what leaves the mask
    // working on a machine with no segmentation sidecar to re-derive it.
    let mut rewritten: Vec<(String, String)> = Vec::new();
    {
        let mut probe = r.clone();
        for m in probe.masks.iter_mut() {
            let paths = if turn.is_some() { m.turnable_raster_paths_mut() } else { m.bitmap_paths_mut() };
            for path in paths {
                if rewritten.iter().any(|(from, _)| from == path.as_str()) {
                    continue; // one file, several masks — re-write it once
                }
                let to = migrate_raster_file(path.as_str(), src, turn, orientation, shift, (dx, dy))?;
                rewritten.push((path.clone(), to));
            }
        }
    }

    // --- Phase 2: commit. Geometry, then the raster references, then the
    // stamp.
    let mut moved = false;
    if let Some(o) = turn {
        // Era-0 numbers are SENSOR-frame by definition, so the frame the
        // brush rewrite must rescale out of is the source rectangle
        // `source_window` reported — not the display one (R29 C1,
        // `render::CoordFrame`).
        let frame = crate::render::CoordFrame::new(sw as f64, sh as f64);
        moved |= crate::render::orient_recipe_coords(r, o, frame);
    }
    if shift != (0.0, 0.0) {
        moved |= crate::render::shift_recipe_coords(r, shift.0, shift.1);
    }
    for m in r.masks.iter_mut() {
        // A superset of both walks phase 1 staged from, so every re-written
        // file is re-pointed and nothing is pointed at a file never made.
        for path in m.bitmap_paths_mut() {
            if let Some((_, to)) = rewritten.iter().find(|(from, _)| from == path.as_str()) {
                *path = to.clone();
            }
        }
    }
    r.coord_era = crate::recipe::COORD_ERA;
    // Nothing to say when nothing moved — a recipe whose only content is
    // global sliders.
    let said = moved && (crate::render::recipe_has_frame_coords(r) || !rewritten.is_empty());
    Ok(said.then_some(CoordMigration {
        orientation: turn.unwrap_or(rawler::Orientation::Normal),
        shift: if shift == (0.0, 0.0) { (0, 0) } else { (dx, dy) },
        rasters: rewritten.len(),
    }))
}

/// The EXIF tag value of an orientation (1..=8; `Unknown` reads as `Normal`),
/// for the raster file names [`migrate_raster_file`] derives.
fn orientation_code(o: rawler::Orientation) -> u8 {
    use rawler::Orientation as O;
    match o {
        O::Normal | O::Unknown => 1,
        O::HorizontalFlip => 2,
        O::Rotate180 => 3,
        O::VerticalFlip => 4,
        O::Transpose => 5,
        O::Rotate90 => 6,
        O::Transverse => 7,
        O::Rotate270 => 8,
    }
}

/// One raster mask brought into the current frame: turned by `turn` when an
/// era-0 recipe sits on a rotated RAW, then translated by `shift` (display
/// frame, normalised — the raster's own pixel count turns it into pixels, so
/// a full-resolution zone alpha moves by exactly the declared origin and a
/// downscaled brush base by the same fraction of ITS width). Returns the
/// path to store in the recipe, spelled like the one it replaces: a bare name
/// stays bare (the store's own convention, and what `serve` hands back
/// byte-for-byte), an absolute path stays absolute.
///
/// **The name is derived from the operation, never claimed fresh**:
/// `<stem>-e2-o<orientation code>[t]-<dx>x<dy>.png` in the photo's develop
/// dir. Every recipe that points at the same raster — the live one, each
/// variant card, each version snapshot — lands on ONE file, and a pass that
/// nothing re-saves (the CLI's every render, the web UI's every `api_recipe`
/// read, an export) finds the file and stops instead of growing a fresh orphan
/// per pass, which a `claim_raster` name would have done. The rewrite goes
/// through a `.part` file and a rename, so a file that exists is complete. The
/// original stays put: version snapshots froze their own copies and a saved
/// recipe elsewhere may still point at it.
///
/// Off-frame samples clamp to the raster's edge: the strip the new window
/// exposes past the old one held no mask, and continuing the edge value is
/// what a gradient that reached the frame's border means there. An integer
/// pixel move is an exact copy; a fractional one (a downscaled raster) is
/// bilinear.
fn migrate_raster_file(
    from: &str,
    src: &Path,
    turn: Option<rawler::Orientation>,
    orientation: rawler::Orientation,
    shift: (f32, f32),
    shift_px: (i32, i32),
) -> std::io::Result<String> {
    let given = Path::new(from);
    let bare = given.is_relative() && given.parent().is_none_or(|p| p.as_os_str().is_empty());
    let from_path = if bare { crate::store::develop_dir(src).join(given) } else { given.to_path_buf() };
    let stem = from_path.file_stem().and_then(|s| s.to_str()).unwrap_or("mask");
    let kind = format!(
        "{stem}-e2-o{}{}-{}x{}",
        orientation_code(orientation),
        if turn.is_some() { "t" } else { "" },
        shift_px.0,
        shift_px.1
    );
    let target = crate::store::raster_target(src, &kind);
    let stored = if bare {
        target.file_name().and_then(|n| n.to_str()).unwrap_or(&kind).to_string()
    } else {
        target.to_string_lossy().into_owned()
    };
    if std::fs::metadata(&target).is_ok_and(|m| m.len() > 0) {
        return Ok(stored);
    }
    // Through THE raster entry: the frame its marker names rides to the
    // rewritten file (`render::stamp_mask_frame` below).
    let raster = crate::render::open_mask_raster(&from_path).map_err(|e| {
        std::io::Error::other(format!("re-write the mask raster {}: {e:#}", from_path.display()))
    })?;
    let (img, frame) = (raster.image, raster.frame);
    let img = match turn {
        Some(o) => crate::render::oriented(img, o),
        None => img,
    };
    let luma = img.to_luma8();
    // A move that is a whole number of pixels to within a thousandth is one —
    // the fraction is f32 rounding of `shift`, and snapping it is what makes
    // the full-resolution case an exact copy rather than a 0.999 blend.
    let snap = |v: f32| if (v - v.round()).abs() < 1e-3 { v.round() } else { v };
    let moved = crate::render::shift_luma_raster(
        &luma,
        snap(shift.0 * luma.width() as f32),
        snap(shift.1 * luma.height() as f32),
    );
    if let Some(par) = target.parent() {
        std::fs::create_dir_all(par)?;
    }
    let part = target.with_extension(format!(
        "part-{}-{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    if let Err(e) = moved.save_with_format(&part, image::ImageFormat::Png) {
        let _ = std::fs::remove_file(&part);
        return Err(std::io::Error::other(format!("write {}: {e}", part.display())));
    }
    if let Err(e) = crate::render::stamp_mask_frame(&part, frame.as_ref()) {
        let _ = std::fs::remove_file(&part);
        return Err(std::io::Error::other(format!("mark {}: {e:#}", part.display())));
    }
    std::fs::rename(&part, &target)?;
    Ok(stored)
}

/// Everything a saved recipe must be brought through before it becomes a live
/// one — the ONE call every load and every re-save shares, so a new migration
/// cannot be wired into some surfaces and forgotten on others (the defect
/// class `render_source_checked`'s comment describes: two surfaces of the same
/// build disagreeing about the same file).
#[derive(Debug, Clone, Default)]
pub struct LoadMigration {
    /// The washed pre-era base curve was re-estimated; the engine's sentence.
    pub relook: Option<String>,
    /// The geometry was turned and/or translated into this build's frame.
    pub reframe: Option<CoordMigration>,
    /// The reframe could NOT be done (a raster mask unreadable or
    /// unwritable): nothing moved, nothing was stamped, and this is the
    /// reason — said, because the photo then renders with its saved geometry
    /// off the picture until the cause is fixed.
    pub reframe_error: Option<String>,
}

impl LoadMigration {
    /// Did anything change? (Drives ● / thumb invalidation at the call sites
    /// that only need "is this recipe different now".) A failed reframe
    /// changed nothing.
    pub fn any(&self) -> bool {
        self.relook.is_some() || self.reframe.is_some()
    }

    /// The chained ENGINE sentence for the CLI / HTTP surfaces. The GUI has
    /// its own localized set — these facts are disclosed separately
    /// everywhere, because they are unrelated corrections.
    pub fn note(&self) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        if let Some(n) = &self.relook {
            parts.push(n.clone());
        }
        if let Some(c) = &self.reframe {
            parts.push(coord_migration_note(*c));
        }
        if let Some(e) = &self.reframe_error {
            parts.push(coord_migration_failure_note(e));
        }
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

/// The engine (English, CLI/HTTP) sentences for a coordinate-frame migration
/// — one per fact, joined: a translation and a raster rewrite are each their
/// own sentence rather than a clause buried in the turn's.
pub fn coord_migration_note(c: CoordMigration) -> String {
    let mut parts: Vec<String> = Vec::new();
    if c.turned() {
        parts.push(concat!(
            "this photo's saved crop and masks were moved to match the RAW's EXIF orientation: ",
            "earlier versions displayed rotated RAWs sideways, so their coordinates were stored ",
            "against the sideways frame"
        )
        .to_string());
    }
    if c.shift != (0, 0) {
        parts.push(format!(
            concat!(
                "this photo's saved crop, masks and retouches were moved by ({}, {}) px: earlier ",
                "versions developed this camera's RAWs from the sensor's corner instead of the crop ",
                "the camera declares, so everything drawn on them sat that far off the picture"
            ),
            c.shift.0, c.shift.1
        ));
    }
    if c.rasters > 0 {
        parts.push(format!(
            "{} raster mask file(s) were re-written into the corrected frame; the originals are kept",
            c.rasters
        ));
    }
    parts.join("; ")
}

/// The engine sentence for a coordinate-frame migration that could NOT run.
pub fn coord_migration_failure_note(err: &str) -> String {
    format!(
        concat!(
            "this photo's saved geometry could NOT be brought into the corrected frame ({}): ",
            "it is left as saved and will be tried again on the next open"
        ),
        err
    )
}

/// What a [`rotate_recipe`] call actually did, for the caller's disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RotateOutcome {
    /// The recipe's `quarter_turns` AFTER the turn (0..=3).
    pub quarter_turns: u8,
    /// How many raster masks were re-written, turned, under fresh names — and
    /// then actually RE-ATTACHED to the recipe.
    ///
    /// `Bitmap` masks only. An `AiMask`'s cached alpha used to be counted here
    /// (it rides the storage walk, `recipe::LocalAdjustment::bitmap_paths_mut`)
    /// although phase 2 discards it, so the number promised turned masks the
    /// recipe no longer carried — R28 Batch-3 3b removed it from the walk
    /// rather than from the count, which is the same fix seen from the other
    /// side (adjudication F8-B).
    pub rasters_turned: usize,
}

/// Turn a photo's saved develop by `delta` CLOCKWISE quarter turns: the
/// photographer's rotate action, in one place for every surface.
///
/// Three things move together, and they have to move together or the photo
/// comes back with its masks somewhere else:
///
///  1. **The stored geometry**, through [`crate::render::orient_recipe_coords`]
///     — crop rectangle, every mask geometry, the Range-Mask sample point, the
///     straighten sign, and since R29 C1 every brush DAB (coordinates turned,
///     radii rescaled by the frame aspect; see phase 0 below for where that
///     aspect comes from). That function is the era-0 → era-1 migration's own
///     engine, proven exact for tilted radials by
///     `rotated_radial_mask_covers_the_rotated_pixels`.
///  2. **The raster masks** — really turned: these are our own PNGs and
///     `image`'s `rotate90` is lossless. (The `coord_era` migration could only
///     disclose them until v1.6.0; it re-writes them the same way now, under
///     operation-derived names — [`migrate_raster_file`] says why the two
///     naming rules differ.) Turned copies land under FRESHLY CLAIMED names
///     (`store::claim_raster`) and the old files stay put — version snapshots
///     freeze their own copies and a saved recipe elsewhere may still point at
///     them, so rewriting in place would silently change what an old version
///     renders (the same rule the zoned reverse-fit follows).
///     **`Bitmap` rasters and the zoned fit's own zone alphas**
///     (`LocalAdjustment::turnable_raster_paths_mut`): any OTHER `AiMask`'s
///     alpha is a CACHE this turn invalidates, and item 1 clears it so the next
///     develop re-segments at the turned reference point. A zone's alpha is not
///     a cache — the fit rendered it and solved its dials against it — so it is
///     turned here and item 1 keeps it (`recipe::MaskRole::is_zone`).
///  3. **`quarter_turns` itself**, which is what makes the render, the export
///     and the next load agree with the geometry above.
///
/// **The turn passed to `orient_recipe_coords` is the DELTA, never the composed
/// value.** The stored coordinates are already in the CURRENT display frame;
/// re-applying the accumulated turn would move them a second time. (This is
/// the one hazard the ROADMAP skeleton called out by name.)
///
/// **All-or-nothing, in memory AND on disk.** Every raster is turned into its
/// new file BEFORE the recipe is touched; if one cannot be read or written,
/// nothing changes and the caller gets the error. A half-turned develop —
/// parametric masks moved, a painted mask left behind — is the exact
/// silent-corruption shape `backup_saved_develop` refuses for the same reason.
/// The DISK half is R28 Batch-3 3b (adjudication F8-A): a failure now also
/// deletes the turned copies already written and the claim of the raster that
/// failed, because "nothing was changed" was a sentence the GUI printed
/// (`gui/actions.rs`) over a develop dir that had grown a fresh orphan PNG per
/// attempt.
///
/// `delta` folds `% 4`; `0` is a no-op that still reports the current state.
pub fn rotate_recipe(
    r: &mut EditRecipe,
    src: &Path,
    delta: u8,
) -> std::io::Result<RotateOutcome> {
    let delta = delta % 4;
    if delta == 0 {
        return Ok(RotateOutcome { quarter_turns: r.quarter_turns % 4, rasters_turned: 0 });
    }
    let o = crate::render::quarter_turn_orientation(delta);

    // --- Phase 0: the frame shape, and ONLY when a brush needs it (R29 C1).
    //
    // A brush's radii are in WIDTH units while its dabs are circles in PIXELS,
    // so turning one needs the aspect of the frame the strokes are currently in
    // — the CURRENT display frame, i.e. the source rectangle carried through
    // the EXIF turn AND the quarter turns already applied, which is exactly
    // `frame_size_turned(src, r.quarter_turns)` before the count below moves.
    //
    // Lazily, because the read is a metadata walk of the photo (a `RawSource`
    // slurp for a RAW) and every other geometry this function turns is
    // aspect-free: a develop with no brush must not start paying for one. It
    // runs BEFORE phase 1 so a failure still leaves nothing behind on disk, and
    // it is an ERROR rather than a silent skip because a brush turned without
    // its rescale is a mask drawn in the wrong shape, which is the outcome this
    // batch exists to end.
    let frame = if crate::render::recipe_has_brush_strokes(r) {
        let (w, h) = crate::decode::frame_size_turned(src, r.quarter_turns).map_err(|e| {
            std::io::Error::other(format!(
                "read the frame of {} to turn its brush strokes: {e:#}",
                src.display()
            ))
        })?;
        crate::render::CoordFrame::new(w as f64, h as f64)
    } else {
        None
    };

    // --- Phase 1: turn every raster into a fresh file. Nothing in `r` moves
    // until all of them are on disk.
    //
    // `turnable_raster_paths_mut`, NOT `bitmap_paths_mut`: the storage walk
    // includes an AI mask's cached alpha and this one does not (R28 Batch-3 3b,
    // adjudication F8-B). See that method for why the two sets differ — in
    // short, phase 2 below DROPS the AI cache by design, so turning it produced
    // an orphan file and a `rasters_turned` count that over-promised. The zone
    // masks are the one AI alpha phase 2 KEEPS, so they are in this walk.
    let mut rewritten: Vec<(String, String)> = Vec::new();
    let staged = {
        let mut probe = r.clone();
        (|| -> std::io::Result<()> {
            for m in probe.masks.iter_mut() {
                for path in m.turnable_raster_paths_mut() {
                    if rewritten.iter().any(|(from, _)| from == path.as_str()) {
                        continue; // one file, several masks — turn it once
                    }
                    let turned = turn_raster_file(Path::new(path.as_str()), src, o)?;
                    rewritten.push((path.clone(), turned));
                }
            }
            Ok(())
        })()
    };
    if let Err(e) = staged {
        // ALL-OR-NOTHING ON DISK TOO (R28 Batch-3 3b, adjudication F8-A). The
        // in-memory half was already true — `r` has not been touched yet — but
        // the rasters turned BEFORE the failure were finished files in the
        // photo's develop dir that nothing would ever reference again, and the
        // GUI told the photographer "nothing was changed" (`gui/actions.rs`)
        // while they accumulated one per attempt. `turn_raster_file` releases
        // the claim of the raster that failed; these are the ones that
        // succeeded. Best-effort per file, like every other cleanup in the
        // store (`store::detach_rasters`, `gui/masks.rs`): a delete that cannot
        // happen must not replace the real error with a filesystem one.
        for (_, made) in rewritten.iter() {
            let _ = std::fs::remove_file(made);
        }
        return Err(e);
    }

    // --- Phase 2: commit. Geometry, then the raster references, then the
    // turn count.
    crate::render::orient_recipe_coords(r, o, frame);
    for m in r.masks.iter_mut() {
        // The SAME walk phase 1 staged from, so a path can never be re-pointed
        // at a file that phase never made.
        for path in m.turnable_raster_paths_mut() {
            if let Some((_, to)) = rewritten.iter().find(|(from, _)| from == path.as_str()) {
                *path = to.clone();
            }
        }
    }
    r.quarter_turns = (r.quarter_turns + delta) % 4;
    Ok(RotateOutcome { quarter_turns: r.quarter_turns, rasters_turned: rewritten.len() })
}

/// One raster mask, turned by `o` into a freshly claimed file inside `src`'s
/// develop dir. Returns the new ABSOLUTE path (the frame every live recipe
/// holds; `store::relativize_mask_paths` bares it again at write time).
///
/// The claim prefix is the old file's stem with any `-<n>` claim suffix
/// stripped, so turning `mask-sky-2.png` produces `mask-sky-3.png` rather than
/// `mask-sky-2-2.png` — the same numbering `store::claim_raster` hands out
/// everywhere else.
fn turn_raster_file(
    from: &Path,
    src: &Path,
    o: rawler::Orientation,
) -> std::io::Result<String> {
    // Through THE raster entry: a quarter turn keeps a lens-corrected raster
    // in its frame (the lens map is radial about the centre, its radius the
    // half-diagonal), so the marker rides to the turned file.
    let raster = crate::render::open_mask_raster(from).map_err(|e| {
        std::io::Error::other(format!("turn the mask raster {}: {e:#}", from.display()))
    })?;
    let turned = crate::render::oriented(raster.image, o);
    let stem = from.file_stem().and_then(|s| s.to_str()).unwrap_or("mask");
    let prefix = stem.rsplit_once('-').filter(|(_, n)| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty()).map_or(stem, |(head, _)| head);
    let target = crate::store::claim_raster(src, prefix)?;
    if let Err(e) = turned.to_luma8().save(&target) {
        // RELEASE THE CLAIM WE COULD NOT FILL — `claim_raster` reserves the
        // name by creating a 0-byte file, so a failed write leaves a file that
        // looks like a mask, decodes as nothing, and burns the name for every
        // later claim. Same rule at the repo's two other claim sites
        // (`store::detach_rasters`, `gui/masks.rs`'s failed segment run), and
        // it is what lets `rotate_recipe`'s caller be told nothing changed.
        let _ = std::fs::remove_file(&target);
        return Err(std::io::Error::other(format!("write {}: {e}", target.display())));
    }
    if let Err(e) = crate::render::stamp_mask_frame(&target, raster.frame.as_ref()) {
        let _ = std::fs::remove_file(&target);
        return Err(std::io::Error::other(format!("mark {}: {e:#}", target.display())));
    }
    Ok(target.to_string_lossy().into_owned())
}

/// [`repair_pre_era_base_curve`] + [`migrate_recipe_coord_frame`], in that
/// order — the load-time migration funnel. Order is not arbitrary: the repair
/// reads the photo's PIXELS (decode + neutral develop) while the reframe reads
/// only its metadata, and the repair's early-out is the cheaper of the two.
pub fn migrate_loaded_recipe(raw: &Path, r: &mut EditRecipe) -> LoadMigration {
    let relook = repair_pre_era_base_curve(raw, r);
    let (reframe, reframe_error) = match migrate_recipe_coord_frame(raw, r) {
        Ok(c) => (c, None),
        Err(e) => (None, Some(e.to_string())),
    };
    LoadMigration { relook, reframe, reframe_error }
}

/// The photo's saved recipe (central store first, then legacy), parsed once —
/// the calibration-stamping snapshot both fields must come from.
///
/// The pre-era repair happens HERE, in the one funnel every library consumer
/// shares, so no surface can render a washed-frame curve by forgetting to ask.
fn saved_recipe_snapshot(raw: &Path, d: &crate::diag::Diag) -> Option<EditRecipe> {
    // A crashed publish's survivor is a save like any other — republish it
    // BEFORE reading, or this snapshot returns None inside the retire window
    // and the caller stamps fresh calibration over the survivor's (the same
    // touch-time rule migrate_legacy follows; a failed recovery is re-decided
    // by the backup gate downstream).
    let _ = crate::store::recover_orphan_baks(raw);
    // Newest intent decides WHICH develop's calibration carries forward
    // (L13#4). When Lightroom's own sidecar out-ranks the store, recipe.json
    // is a superseded generation: both restore surfaces already re-stamp
    // FRESH calibration for that photo (serve.rs api_recipe, the GUI's
    // stamp_calibration on an XMP restore), and stamping the superseded
    // recipe's curve/profile/anchor onto a brand-new AI recipe made Analyze
    // hand back a canvas that renders unlike the one it replaced. `None` is
    // the right answer: the fresh arm in every consumer stamps exactly what
    // the restore surfaces stamp.
    match crate::store::lightroom_sidecar(raw) {
        crate::store::LrSidecar::Only(t) | crate::store::LrSidecar::NewerThanStore(t)
            if !crate::xmp::xmp_to_recipe_for_photo(&t, raw).is_noop() =>
        {
            return None;
        }
        crate::store::LrSidecar::Unreadable(why) => {
            // Unreadable is not absent — but it cannot outrank either.
            // Disclosed, then the stored develop stands.
            d.warn(format!(
                "a Lightroom sidecar sits beside this photo but could not be read ({why}) — calibration falls back to the stored develop"
            ));
        }
        _ => {}
    }
    for p in [crate::store::recipe_target(raw), crate::store::legacy_recipe(raw)] {
        if let Ok(text) = crate::store::read_text_capped(&p, crate::store::MAX_STORE_JSON)
            && let Ok(mut r) = serde_json::from_str::<EditRecipe>(&text)
        {
            if let Some(note) = repair_pre_era_base_curve_in(raw, &mut r, d) {
                d.warn(note);
            }
            return Some(r);
        }
    }
    None
}

/// The per-photo calibration a programmatic writer stamps — every field from
/// ONE saved-recipe snapshot. The era stamp is a FIELD of it: it describes
/// the curve's provenance, so a writer that stamps the curve without its
/// version launders a saved era-1 curve under its own era-2 recipe (the
/// defect the paste path shipped, then fixed). The two split accessors this
/// replaces (`photo_base_curve`, `photo_lens_profile`) had no remaining
/// callers and re-created both hazards for the next one.
pub struct PhotoCalibration {
    pub version: u32,
    pub base_curve: Vec<[f32; 2]>,
    pub lens_profile: crate::recipe::LensProfile,
    pub as_shot_k: Option<f32>,
    pub as_shot_tint: Option<f32>,
    /// v1.17.0 — the camera profile and creative Look a saved develop names
    /// (a sidecar's own on an import; none on a fresh open). Colour data: it
    /// decides the base develop ([`crate::render::BasePart`]) every surface
    /// shows, fits and bakes from, and never the tone.
    pub camera_profile: String,
    pub look: Option<crate::recipe::CreativeLook>,
}

impl PhotoCalibration {
    /// Every calibration half of a saved recipe, read whole.
    fn of_saved(r: EditRecipe) -> Self {
        Self {
            version: r.version,
            base_curve: r.base_curve,
            lens_profile: r.lens_profile,
            as_shot_k: r.as_shot_k,
            as_shot_tint: r.as_shot_tint,
            camera_profile: r.camera_profile,
            look: r.look,
        }
    }
}

/// What a fresh develop of a photograph starts from: the camera-matched base
/// curve estimated from the embedded preview (empty for a baked image).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CanvasStart {
    pub base_curve: Vec<[f32; 2]>,
}

impl CanvasStart {
    /// Write this start onto `r`.
    pub fn stamp(self, r: &mut EditRecipe) {
        r.base_curve = self.base_curve;
    }
}

/// The base part (v1.17.0, [`crate::render::BasePart`]) of the develop this
/// photograph starts from: its saved develop's, or a fresh start's — the part
/// a surface without a recipe in hand (a CLI retouch, a web pane that has
/// not been sent one) bakes or shows under. A fresh start names no camera
/// profile, so it is the neutral part; that answer needs no estimate.
pub fn photo_base_part(raw: &Path) -> crate::render::BasePart {
    match saved_recipe_snapshot(raw, &crate::diag::photo(raw)) {
        Some(r) => crate::render::BasePart::of(&r),
        None => crate::render::BasePart::default(),
    }
}

/// [`CanvasStart`] for `raw`, its one disclosure on `d`.
pub fn fresh_canvas_start(raw: &Path, d: &crate::diag::Diag) -> CanvasStart {
    fresh_canvas_start_with(raw, d, || photo_base_knots_in(raw, d))
}

/// THE one decision of a fresh start, shared by every fresh-open surface —
/// [`fresh_canvas_start`], the GUI's open worker and the web's fresh payload.
/// `estimate` is the camera-matched base-curve estimate: the two interactive
/// surfaces hand in their own, made on the neutral develop they already
/// hold, so a fresh open still pays one demosaic. `_d` keeps the surfaces'
/// one call shape.
pub fn fresh_canvas_start_with(
    raw: &Path,
    _d: &crate::diag::Diag,
    estimate: impl FnOnce() -> Vec<[f32; 2]>,
) -> CanvasStart {
    if !decode::is_raw(raw) {
        return CanvasStart::default();
    }
    CanvasStart { base_curve: estimate() }
}

/// The start of a develop IMPORTED from a Lightroom document (a sidecar, the
/// saved projection, a RAW's embedded packet): the photo's camera base curve,
/// as a fresh develop's. A camera profile or Look the document names stays
/// on the recipe as colour data (`render::BasePart`); it never changes how a
/// control behaves.
///
/// A fresh start with nothing in it (a baked image, an estimate that found no
/// base look) leaves a curve the document itself brought where it is.
pub fn stamp_import_start(raw: &Path, r: &mut EditRecipe, d: &crate::diag::Diag) {
    fresh_canvas_start(raw, d).stamp_import(r);
}

impl CanvasStart {
    /// Nothing to start from: no curve.
    pub fn is_empty(&self) -> bool {
        self.base_curve.is_empty()
    }

    /// [`stamp_import_start`] with a start the caller already holds (a pack
    /// of imports of one capture estimates it once).
    pub fn stamp_import(&self, r: &mut EditRecipe) {
        if !self.is_empty() {
            self.clone().stamp(r);
        }
    }
}

/// ALL calibration halves from ONE saved-recipe snapshot: independent reads
/// could pair an OLD curve with a NEW profile when a concurrent publish lands
/// between them. (`produce_recipe` is the one stamper that reads the snapshot
/// TWICE — the as-shot anchor early, because its prompt must quote the anchor
/// the deliverable will carry, and the curve, era and profile late, so that
/// stamp stays fresh across minutes of network calls; each half is still
/// read whole from one snapshot.) The fresh arm is era-stamped by
/// construction — those estimates come from THIS build's sampler.
pub fn photo_calibration(raw: &Path) -> PhotoCalibration {
    match saved_recipe_snapshot(raw, &crate::diag::photo(raw)) {
        Some(r) => PhotoCalibration::of_saved(r),
        None => fresh_photo_calibration(raw),
    }
}

/// [`photo_calibration`]'s fresh arm on its own — era-stamped by
/// construction (these estimates come from THIS build's sampler).
pub(crate) fn fresh_photo_calibration(raw: &Path) -> PhotoCalibration {
    let (as_shot_k, as_shot_tint) = fresh_as_shot_wb(raw);
    let start = fresh_canvas_start(raw, &crate::diag::photo(raw));
    PhotoCalibration {
        version: crate::recipe::CALIB_ERA,
        base_curve: start.base_curve,
        lens_profile: fresh_lens_profile(raw),
        as_shot_k,
        as_shot_tint,
        camera_profile: String::new(),
        look: None,
    }
}

/// True when a calibration carries NOTHING the render would act on — no
/// base curve, no camera profile, no active lens component (the recipe's own
/// activity predicates — re-implementing them here is the drift the codebase
/// already fixed once), no as-shot WB anchor.
pub fn calibration_is_neutral(cal: &PhotoCalibration) -> bool {
    cal.base_curve.is_empty()
        && cal.camera_profile.is_empty()
        && cal.look.as_ref().is_none_or(|l| l.base_profile.is_empty())
        && !cal.lens_profile.vignette_active()
        && !cal.lens_profile.geometry_active()
        && cal.as_shot_k.is_none()
}

/// The calibration authority for REVERSE-FIT surfaces (CLI `match` + GUI
/// 反推): saved-first like [`photo_calibration`], but an all-NEUTRAL saved
/// calibration falls through to the fresh estimate. A neutral saved
/// calibration is either a pre-base-curve legacy save or a previous
/// UNSTAMPED fit (the R15 poison: an old fit recipe with an empty curve
/// became the saved-first authority, so every later fit inherited the
/// empty base and the seed never engaged). Falling through is safe for a
/// fit specifically — the solve compensates ON TOP of whatever base it is
/// given, so the final look is unchanged; only the conditioning improves —
/// while ordinary develops keep the legacy render-as-saved contract via
/// [`photo_calibration`].
/// Narrow disclosed edge: a photo whose saved develop turned every lens
/// component OFF and has neither curve nor as-shot anchor reads as
/// "neutral" and a fit re-stamps the fresh all-on profile — a fit is a NEW
/// develop, and all-on matches the in-camera default that stamping always
/// produces. Branching on the snapshot directly keeps the fresh estimate
/// (a demosaic for a RAW) to at most ONE run.
pub fn fit_calibration(raw: &Path) -> PhotoCalibration {
    match saved_recipe_snapshot(raw, &crate::diag::photo(raw)) {
        Some(r) => {
            let cal = PhotoCalibration::of_saved(r);
            if calibration_is_neutral(&cal) { fresh_photo_calibration(raw) } else { cal }
        }
        None => fresh_photo_calibration(raw),
    }
}

/// Stamp a recipe with the photo's calibration (the GUI's snapshot and open
/// stampers cite its era rule, below). No reverse fit stamps after its solve:
/// every fit surface composes the calibration INTO the solve (`fit_source`,
/// the CLI's `match_source`, the GUI's fit arms), because a stamp laid on
/// after the solve is a base the fitted deltas never saw — the CLI's
/// baked-source arm did exactly that until 2026-09-26. The era
/// stamp rides WITH the curve (the paste rule): the fitted recipe is era-2
/// by Default, and stamping a saved era-1 curve under it would launder the
/// provenance the pre-era repair keys on.
pub fn stamp_fit_calibration(recipe: &mut crate::recipe::EditRecipe, cal: PhotoCalibration) {
    recipe.version = cal.version;
    recipe.base_curve = cal.base_curve;
    recipe.lens_profile = cal.lens_profile;
    recipe.as_shot_k = cal.as_shot_k;
    recipe.as_shot_tint = cal.as_shot_tint;
    recipe.camera_profile = cal.camera_profile;
    recipe.look = cal.look;
}

/// What a post-stamp calibration adds to the delivered render, as a phrase --
/// or `None` when it adds nothing and the two frames are already one.
///
/// The production stamper is the inline saved-first stamp inside
/// [`produce_recipe`] (analyze / batch / web): it leaves a recipe whose
/// residual and review score were read BEFORE the calibration existed, so it
/// asks this question. CLI `match` composed its calibration into every solve
/// from 2026-09-26 and has nothing left to disclose.
pub fn post_stamp_domain_shift(recipe: &crate::recipe::EditRecipe) -> Option<String> {
    // The start the render applies: a named camera profile's colour tables
    // and the camera curve.
    let start = match (recipe.names_profile(), !recipe.base_curve.is_empty()) {
        (true, true) => Some("its camera profile and camera curve"),
        (true, false) => Some("its camera profile"),
        (false, true) => Some("its camera curve"),
        (false, false) => None,
    };
    // "Enabled" is the recipe's own activity predicates — the ones the
    // renderer gates each stage on — not the switches alone: a profile whose
    // arrays are populated but all switched off changes no pixel, and neither
    // does a switch that is on over an empty array.
    let lp = &recipe.lens_profile;
    let lens = (lp.vignette_active() || lp.geometry_active()).then_some("its lens profile");
    match (start, lens) {
        (Some(s), Some(l)) => Some(format!("{s} and {l}")),
        (Some(one), None) | (None, Some(one)) => Some(one.to_string()),
        (None, None) => None,
    }
}

/// Takes the PHRASE rather than the recipe: both callers already hold a mutable
/// borrow of the recipe's own rationale when they push.
pub fn note_post_stamp_domain(
    rationale: &mut String,
    notes: &mut Vec<crate::rationale::Note>,
    shift: Option<String>,
) {
    if let Some(what) = shift {
        crate::rationale::push_note(
            rationale,
            notes,
            crate::rationale::Note::new(
                crate::rationale::keys::FIT_RESIDUAL_PRE_CALIBRATION,
                vec![("what", what)],
            ),
        );
    }
}

/// Working edge of the reverse-fit's source frame. Comfortably above both
/// consumers (the fit analyses at 384, the AI judge at 1024) and the same
/// working edge the base-look estimator uses.
pub const FIT_SOURCE_EDGE: u32 = 2048;

/// THE source frame of a reverse fit, and the calibration that solve composes
/// — one function, because the two entry points used to choose differently
/// and the choice decides the answer.
///
/// The desktop app fitted its 1280-px `source_preview`; the CLI fitted the
/// camera's EMBEDDED rendition unless its aspect disagreed with the sensor
/// frame. Those are two different images of one capture — different tone
/// curve, different sharpening, different noise — and
/// [`crate::fit::structure_divergence`] is measured on whichever one the entry
/// chose. On the user's `reimagine-2` pair (2026-09-10) that difference alone
/// decided the SOLVER: the embedded rendition reads D = 0.361 and lands in
/// Atmosphere mode, the 2048-px neutral develop reads D = 0.276 and lands in
/// Full — one threshold, two answers, same photograph. So the composed branch
/// is now unconditional and the embedded-preview branch is gone.
///
/// A bare non-RAW source has no sensor frame to develop and no calibration
/// to compose. A master a denoise or a stack remade the negative into is
/// different: both entries fit its pixels under the PHOTO's calibration —
/// the desktop app through its ◈/▦ card, the CLI through
/// `match --negative <RAW>` (v1.6.2; a bare master used to be fitted with no
/// calibration, which on the reference pair moved the sky's structural
/// reading across the zone line and dropped the lens profile from the render).
///
/// The source is the calibration's own BASE develop (v1.17.0,
/// [`crate::render::base_develop`]): the solve renders every candidate with
/// the preview develop over it, which runs no profile stage, so a source
/// without the profile would be fitted as a different picture from the one
/// the canvas and the export show. It comes as a [`crate::render::Plate`]
/// ([`crate::render::source_plate`]).
///
/// In the FRAME that base develop is in: the sensor frame for a RAW. The fit
/// draws its zone rasters in its source's frame and they say so
/// ([`crate::render::fit_raster_frame`]), so a mask is never resampled from
/// one frame into the other on the way.
pub fn fit_source(raw: &Path) -> Result<(crate::render::Plate, crate::recipe::EditRecipe)> {
    let base = calibration_recipe(fit_calibration(raw));
    Ok((fit_source_of(raw, &base)?, base))
}

/// [`fit_source`]'s plate of `raw` under `base`.
fn fit_source_of(raw: &Path, base: &EditRecipe) -> Result<crate::render::Plate> {
    crate::render::source_plate(raw, &crate::render::BasePart::of(base), FIT_SOURCE_EDGE)
}

/// The fit source of a BAKED file (a master, a bare raster) whose pixels are
/// `image`: a plate in the frame the file's marker says its pixels are in
/// ([`crate::render::SourceFrame::corrected`]), so its candidates read their
/// masks where the pixels are.
pub fn fit_source_baked(path: &Path, image: image::DynamicImage) -> Result<crate::render::Plate> {
    let corrected = crate::render::SourceFrame::of(path)?.corrected();
    Ok(crate::render::Plate::baked_in(image, corrected))
}

/// A calibration-only [`EditRecipe`] — the `base` the R16 fit composes
/// into its solve (`fit::fit_recipe_from` / `fit_zoned::fit_recipe_zoned_from`).
/// The as-shot WB anchors only RIDE (no temperature is set, so
/// `apply_recipe_wb` never fires — they exist for the develop panel's WB
/// baseline). This retired the v0.24.0 two-pass seed
/// (`fit_calibration_seed`): pre-rendering the calibration clipped
/// saturated channels at the pass boundary (`scale_chroma` clamp order,
/// measured up to ~18.7/255 mean on saturated fixtures), while composing
/// the base INTO the solve makes every candidate render the canvas's own
/// one-pass `user(base(x))` — the gap is gone by construction and the fit's
/// residual numbers describe exactly what the user sees.
pub fn calibration_recipe(cal: PhotoCalibration) -> crate::recipe::EditRecipe {
    crate::recipe::EditRecipe {
        version: cal.version,
        base_curve: cal.base_curve,
        lens_profile: cal.lens_profile,
        as_shot_k: cal.as_shot_k,
        as_shot_tint: cal.as_shot_tint,
        camera_profile: cal.camera_profile,
        look: cal.look,
        ..Default::default()
    }
}

/// What the reverse fit measured about its TARGET's lens geometry, before any
/// pixel was paired.
///
/// The fit pairs its source with the target pixel for pixel, so the two must
/// be in one frame. The source is in the frame of the calibration's base
/// develop — lens-corrected when the camera stage resamples the lens
/// ([`fit_source`]) — and a Lightroom export is in whichever frame it was
/// exported with: the profile on moves its pixels (on one calibration pair,
/// ±10 px at the corners of a 1000-px frame). The fit cannot read the answer
/// off a sidecar (it is never handed one, and one of the calibration targets
/// was exported with the profile off), so it MEASURES it: the target against
/// the source frame as shot and as the profile corrects it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TargetGeometry {
    /// The calibration moves no pixel (no distortion knots, no CA overshoot,
    /// no source with a lens): there is no question to ask, and the target is
    /// paired as it always was.
    Unmoved,
    /// The target was lens-corrected. It is paired as it stands, with the
    /// source in the lens-corrected frame (a source developed in the sensor
    /// frame is moved there by the engine's own geometry stage).
    Corrected { corrected: f32, as_shot: f32 },
    /// The target keeps the uncorrected geometry: it is paired as it stands
    /// with the source in the sensor frame (a corrected RAW source is
    /// developed again without the geometry), and the recipe draws no profile
    /// distortion or CA, so the render keeps the geometry the target was
    /// exported with.
    AsShot { corrected: f32, as_shot: f32 },
    /// Neither frame won by [`GEOMETRY_WIN_RATIO`] and [`GEOMETRY_WIN_MARGIN`]
    /// — or the target reads as shot over a lens-corrected MASTER, which has
    /// no sensor-frame develop to go back to: paired as it stands, the
    /// profile left as it was, and said so.
    Ambiguous { corrected: f32, as_shot: f32 },
}

/// The fit's target after [`fit_target`]: the target as it was exported, in
/// the frame the fit's source now is.
pub struct FitTarget {
    pub image: image::DynamicImage,
    pub geometry: TargetGeometry,
}

/// Long edge the geometry probe reads at — the size the alignment instrument
/// was calibrated on: on a lens-corrected calibration target the fit's own
/// final render read peaks 0.48–0.81 and its uncorrected reference frame
/// 0.06–0.17.
const GEOMETRY_PROBE_EDGE: u32 = 1000;
/// Side of one probe patch (a power of two, for the transform).
const GEOMETRY_PATCH: usize = 256;
/// Patch centres as fractions of the frame: the four corner regions and the
/// top centre, where a lens profile moves most, and the centre, where it moves
/// least.
const GEOMETRY_PATCH_CENTRES: [(f32, f32); 6] =
    [(0.14, 0.18), (0.50, 0.20), (0.86, 0.18), (0.14, 0.82), (0.86, 0.82), (0.50, 0.52)];
/// A frame is the target's when its mean peak is at least this multiple of
/// the other's…
pub const GEOMETRY_WIN_RATIO: f32 = 2.0;
/// …and at least this far above it, so two weak readings cannot decide.
pub const GEOMETRY_WIN_MARGIN: f32 = 0.10;

/// Measure the target's geometry against the fit's source frame and put the
/// SOURCE in the target's frame — see [`TargetGeometry`]. The target is never
/// resampled. `src` is the fit's source plate ([`fit_source`],
/// [`fit_source_baked`]); `raw` the RAW it was developed from, when it was, so
/// a corrected source can be developed again in the sensor frame. `base` is the
/// calibration the fit composes; on an as-shot target its profile distortion
/// and CA come off, because the recipe inherits them from it.
pub fn fit_target(
    src: &mut crate::render::Plate,
    raw: Option<&Path>,
    target: &image::DynamicImage,
    base: &mut EditRecipe,
) -> Result<FitTarget> {
    let geom = crate::render::geometry_profile(base).into_owned();
    let amount = base.lens_distortion;
    let as_exported = |geometry| Ok(FitTarget { image: target.clone(), geometry });
    if !crate::render::geometry_moves_frame(&geom, amount) {
        return as_exported(TargetGeometry::Unmoved);
    }
    let src_corrected = src.corrected();
    let (corrected, as_shot) = target_alignment(src, src_corrected, target, base, &geom, amount);
    let wins = |a: f32, b: f32| a >= GEOMETRY_WIN_RATIO * b && a - b >= GEOMETRY_WIN_MARGIN;
    if wins(corrected, as_shot) {
        if !src_corrected {
            // A source developed in the sensor frame (the tail resamples this
            // calibration's lens, or an unmarked master): the engine's own
            // geometry stage moves it into the frame the target was exported
            // in, as the export moves the same pixels.
            let moved = crate::render::apply_lens_geometry(src, &geom, amount);
            *src = crate::render::Plate::baked_in(moved, true);
        }
        return as_exported(TargetGeometry::Corrected { corrected, as_shot });
    }
    if wins(as_shot, corrected) {
        if src_corrected {
            // The source must be in the sensor frame too: a RAW develops again
            // with the geometry off; a corrected master has no such develop.
            let Some(raw) = raw else { return as_exported(TargetGeometry::Ambiguous { corrected, as_shot }) };
            let mut flat = base.clone();
            flat.lens_profile.distortion_on = false;
            flat.lens_profile.ca_on = false;
            *src = fit_source_of(raw, &flat)?;
        }
        base.lens_profile.distortion_on = false;
        base.lens_profile.ca_on = false;
        return as_exported(TargetGeometry::AsShot { corrected, as_shot });
    }
    as_exported(TargetGeometry::Ambiguous { corrected, as_shot })
}

/// The verdict, said once per fit in the rationale.
pub fn note_target_geometry(report: &mut crate::fit::FitReport, geometry: TargetGeometry) {
    use crate::rationale::{Note, keys};
    let two = |v: f32| format!("{v:.2}");
    let note = match geometry {
        TargetGeometry::Unmoved => return,
        TargetGeometry::Corrected { corrected, as_shot } => Note::new(
            keys::TARGET_GEOMETRY_CORRECTED,
            vec![("corrected", two(corrected)), ("as_shot", two(as_shot))],
        ),
        TargetGeometry::AsShot { corrected, as_shot } => Note::new(
            keys::TARGET_GEOMETRY_AS_SHOT,
            vec![("corrected", two(corrected)), ("as_shot", two(as_shot))],
        ),
        TargetGeometry::Ambiguous { corrected, as_shot } => Note::new(
            keys::TARGET_GEOMETRY_AMBIGUOUS,
            vec![("corrected", two(corrected)), ("as_shot", two(as_shot))],
        ),
    };
    crate::rationale::push_note(&mut report.recipe.rationale, &mut report.notes, note);
}

/// Mean phase-correlation peaks `(corrected, as_shot)` of the target against
/// the source frame as corrected and as shot, over [`GEOMETRY_PATCH_CENTRES`]
/// at [`GEOMETRY_PROBE_EDGE`]. The source is rendered under the calibration so
/// the two sides share a tone; each patch is high-passed (a 9-px box removed)
/// so the reading is about edges, not brightness.
///
/// The profile is applied to whichever side is in the sensor frame: to the
/// source's render when it is (`src_corrected` false), and to the TARGET when
/// the source is already corrected — the as-shot hypothesis is then that the
/// correction takes the target onto the source. Either way the reading is of
/// an image the geometry stage moved against one it did not.
fn target_alignment(
    src: &image::DynamicImage,
    src_corrected: bool,
    target: &image::DynamicImage,
    base: &EditRecipe,
    geom: &crate::recipe::LensProfile,
    amount: f32,
) -> (f32, f32) {
    let (sw, sh) = (src.width().max(1) as f32, src.height().max(1) as f32);
    let scale = GEOMETRY_PROBE_EDGE as f32 / sw.max(sh);
    let (w, h) = (((sw * scale).round() as u32).max(1), ((sh * scale).round() as u32).max(1));
    if (w as usize) < GEOMETRY_PATCH || (h as usize) < GEOMETRY_PATCH {
        return (0.0, 0.0);
    }
    let filter = image::imageops::FilterType::Lanczos3;
    let look = crate::render::develop_preview(&src.resize_exact(w, h, filter), base);
    let t = target.resize_exact(w, h, filter);
    let mean = |readings: Vec<(f32, i32, i32)>| {
        readings.iter().map(|r| r.0).sum::<f32>() / readings.len().max(1) as f32
    };
    if src_corrected {
        let t_corrected = crate::render::apply_lens_geometry(&t, geom, amount);
        return (mean(patch_alignment(&t, &look)), mean(patch_alignment(&t_corrected, &look)));
    }
    let corrected = crate::render::apply_lens_geometry(&look, geom, amount);
    (mean(patch_alignment(&t, &corrected)), mean(patch_alignment(&t, &look)))
}

/// `(peak, dy, dx)` of `b` against `a` on each of [`GEOMETRY_PATCH_CENTRES`];
/// the two rasters share one size of at least [`GEOMETRY_PATCH`] a side.
fn patch_alignment(a: &image::DynamicImage, b: &image::DynamicImage) -> Vec<(f32, i32, i32)> {
    let (w, h) = (a.width() as usize, a.height() as usize);
    let n = GEOMETRY_PATCH;
    let luma = |img: &image::DynamicImage| -> Vec<f32> {
        img.to_rgb8().pixels().map(|p| 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32).collect()
    };
    let (la, lb) = (luma(a), luma(b));
    GEOMETRY_PATCH_CENTRES
        .iter()
        .map(|&(fx, fy)| {
            let x0 = ((fx * w as f32) as usize).saturating_sub(n / 2).min(w - n);
            let y0 = ((fy * h as f32) as usize).saturating_sub(n / 2).min(h - n);
            let patch = |plane: &[f32]| -> Vec<f32> {
                let mut p = Vec::with_capacity(n * n);
                for y in y0..y0 + n {
                    p.extend_from_slice(&plane[y * w + x0..y * w + x0 + n]);
                }
                high_pass_9(&p, n)
            };
            phase_correlation(&patch(&la), &patch(&lb), n)
        })
        .collect()
}

/// `p` less its 9×9 box mean, the box mirrored at the patch edge.
fn high_pass_9(p: &[f32], n: usize) -> Vec<f32> {
    let mirror = |i: isize| -> usize {
        let m = n as isize;
        let i = if i < 0 { -i - 1 } else if i >= m { 2 * m - i - 1 } else { i };
        i.clamp(0, m - 1) as usize
    };
    let mut rows = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            rows[y * n + x] = (-4..=4).map(|d| p[y * n + mirror(x as isize + d)]).sum::<f32>() / 9.0;
        }
    }
    let mut out = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let mean = (-4..=4).map(|d| rows[mirror(y as isize + d) * n + x]).sum::<f32>() / 9.0;
            out[y * n + x] = p[y * n + x] - mean;
        }
    }
    out
}

/// The phase-correlation peak of two `n`×`n` patches and where it sits:
/// height 1.0 for a pure translation of one picture, falling toward 0 as the
/// second stops being a shifted copy of the first; `(dy, dx)` the shift.
fn phase_correlation(a: &[f32], b: &[f32], n: usize) -> (f32, i32, i32) {
    let mut fa: Vec<(f64, f64)> = a.iter().map(|v| (*v as f64, 0.0)).collect();
    let mut fb: Vec<(f64, f64)> = b.iter().map(|v| (*v as f64, 0.0)).collect();
    fft2(&mut fa, n, false);
    fft2(&mut fb, n, false);
    let mut r: Vec<(f64, f64)> = fa
        .iter()
        .zip(&fb)
        .map(|(&(ar, ai), &(br, bi))| {
            let (re, im) = (ar * br + ai * bi, ai * br - ar * bi);
            let m = (re * re + im * im).sqrt() + 1e-9;
            (re / m, im / m)
        })
        .collect();
    fft2(&mut r, n, true);
    let (at, peak) = r
        .iter()
        .enumerate()
        .fold((0, f64::MIN), |best, (i, v)| if v.0 > best.1 { (i, v.0) } else { best });
    let wrap = |v: usize| if v < n / 2 { v as i32 } else { v as i32 - n as i32 };
    (peak as f32, wrap(at / n), wrap(at % n))
}

/// In-place 2-D transform of an `n`×`n` row-major grid (`n` a power of two);
/// `inverse` includes the 1/n² scale.
fn fft2(data: &mut [(f64, f64)], n: usize, inverse: bool) {
    let mut line = vec![(0.0, 0.0); n];
    for y in 0..n {
        line.copy_from_slice(&data[y * n..(y + 1) * n]);
        fft1(&mut line, inverse);
        data[y * n..(y + 1) * n].copy_from_slice(&line);
    }
    for x in 0..n {
        for y in 0..n {
            line[y] = data[y * n + x];
        }
        fft1(&mut line, inverse);
        for y in 0..n {
            data[y * n + x] = line[y];
        }
    }
    if inverse {
        let s = 1.0 / (n * n) as f64;
        data.iter_mut().for_each(|v| *v = (v.0 * s, v.1 * s));
    }
}

/// Iterative radix-2 Cooley–Tukey, unscaled.
fn fft1(a: &mut [(f64, f64)], inverse: bool) {
    let n = a.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = 2.0 * std::f64::consts::PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (ur, ui) = a[start + k];
                let (xr, xi) = a[start + k + len / 2];
                let (vr, vi) = (xr * cr - xi * ci, xr * ci + xi * cr);
                a[start + k] = (ur + vr, ui + vi);
                a[start + k + len / 2] = (ur - vr, ui - vi);
                (cr, ci) = (cr * wr - ci * wi, cr * wi + ci * wr);
            }
        }
        len <<= 1;
    }
}

/// Fresh in-camera lens profile for `raw`, stamped "all available components
/// on" (the user's chosen default). What the camera's own rendition shows of
/// them is a separate question, and a measured one: none of ten ILCE-7RM4A
/// embedded previews shows the vignette lift (`render::camera_base_look`).
/// Cheap — one TIFF metadata parse, no decode.
pub fn fresh_lens_profile(raw: &Path) -> crate::recipe::LensProfile {
    fresh_lens_profile_from(raw, &crate::lensmeta::LensQuery::default())
}

/// [`fresh_lens_profile`] with what a sidecar adds to the lookup.
fn fresh_lens_profile_from(raw: &Path, query: &crate::lensmeta::LensQuery) -> crate::recipe::LensProfile {
    let mut p = crate::lensmeta::read_for(raw, query);
    p.vignette_on = !p.vignette.is_empty();
    p.distortion_on = !p.distortion.is_empty();
    p.ca_on = !p.ca_r.is_empty() && !p.ca_b.is_empty();
    p
}

/// [`fresh_lens_profile`] for a photo whose masks came from a SIDECAR — the
/// only form that can answer the mask-warp question correctly (R29 Batch-3).
///
/// `crs:LensProfileEnable="0"` says Lightroom applied no lens correction, so
/// RADIAL geometry stays in its stored frame, and so does LINEAR (v1.17.0,
/// Camera Raw; the 2026-08 H2 rule moved its handles through the camera map).
/// The ordinary `mask_warp` therefore remains identity, and the solved map is
/// set aside in `linear_handle_warp`, which no render path reads.
///
/// **The switch is obeyed** (user ruling 2026-09-17, reversing R29's): a
/// document that says `crs:LensProfileEnable="0"` is a document Lightroom drew
/// with NO lens correction, so this engine draws none either — the three
/// profile toggles come off with the warp.
///
/// R29 left the toggles alone on the ground that they are the photographer's
/// own and Lightroom's switch is about ADOBE's profile, not the camera's
/// knots. Two things answer that now. v1.5.0 renders Adobe's `.lcp` itself
/// ([`crate::lcp::solve`]), so for a body with no in-camera metadata the
/// switch IS about the very knots we are holding. And the old split was
/// INCOHERENT even for a Sony: [`retain_disabled_linear_handle_warp`] already
/// moved the map out of `mask_warp` for this state, so masks were positioned
/// in the uncorrected frame while the pixels beside them were corrected. One
/// switch, one frame.
///
/// The manual pair (`ca_r`/`ca_b`) and the manual Distortion/Vignette sliders
/// are untouched: they are not the profile, and Lightroom's own switch does
/// not turn them off either.
///
/// `None` sidecar, or a document that says nothing, is [`fresh_lens_profile`]
/// unchanged.
pub fn fresh_lens_profile_for_sidecar(
    raw: &Path,
    sidecar: Option<&str>,
) -> crate::recipe::LensProfile {
    // A sidecar of OURS whose lens keys Lightroom left as written carries the
    // develop's own lens profile: that is the authority (`xmp::
    // written_lens_profile`). The switches below are obeyed when they are an
    // edit, or when the document is not ours.
    if let Some(p) = sidecar.and_then(crate::xmp::written_lens_profile) {
        return p;
    }
    let query = sidecar.map_or_else(Default::default, |x| crate::lensmeta::LensQuery {
        named: sidecar_named_profile(x),
        focus_distance_m: crate::xmp::approximate_focus_distance(x),
    });
    let mut p = fresh_lens_profile_from(raw, &query);
    if let Some(x) = sidecar {
        obey_sidecar_lens_switches(&mut p, x, crate::lcp::pool_present());
    }
    p
}

/// The profile a sidecar names, when its setup makes the name count: any
/// stated setup with crs:LensProfileName, and Custom even without one (Camera
/// Raw then resolves nothing). With no setup the name is not read — measured:
/// a name and a strength of 50 with no setup render at 100.
fn sidecar_named_profile(x: &str) -> Option<crate::lensmeta::NamedProfile> {
    use crate::xmp::LensProfileSetup;
    let setup = crate::xmp::lens_profile_setup(x)?;
    let name = crate::xmp::lens_profile_name(x);
    let custom = setup == LensProfileSetup::Custom;
    (name.is_some() || custom).then(|| crate::lensmeta::NamedProfile {
        name: name.unwrap_or_default(),
        filename: crate::xmp::lens_profile_filename(x),
        custom,
    })
}

/// What a sidecar's two lens keys do to the profile's toggles — one function,
/// so the test hands it a profile with every component on and reads the real
/// rule rather than a sketch of it.
///
/// * `crs:LensProfileEnable="0"` → [`obey_disabled_profile_switch`];
/// * `crs:LensProfileSetup="Custom"` → no camera CA: Camera Raw does not apply
///   the camera's CA knots under a hand-picked profile (measured, see
///   [`crate::xmp::LensProfileSetup`]); `LensDefaults`, `Auto` and no setup
///   leave the camera's CA as it is;
/// * Custom naming no profile this machine's pool has (`pool` is
///   [`crate::lcp::pool_present`]) → no profile correction at all, as with
///   the switch off: Camera Raw then renders neither the vignetting nor the
///   distortion (measured: a flat field stays flat and a real frame's geometry
///   is the switched-off one to 0.01 px). With no pool here the name is
///   unknown rather than absent, and the camera's correction stays.
fn obey_sidecar_lens_switches(p: &mut crate::recipe::LensProfile, sidecar: &str, pool: bool) {
    let custom = crate::xmp::lens_profile_setup(sidecar) == Some(crate::xmp::LensProfileSetup::Custom);
    if crate::xmp::lens_profile_enabled(sidecar) == Some(false) || (custom && pool && p.adobe.is_none()) {
        obey_disabled_profile_switch(p);
    }
    if custom {
        p.ca_on = false;
    }
}

/// What `crs:LensProfileEnable="0"` means, as ONE function: Lightroom drew no
/// profile correction, so neither do we — vignetting and distortion off, and
/// the solved camera map moved aside for LINEAR's H2 rule with a stamp saying
/// why.
///
/// The camera's CHROMATIC ABERRATION stays on (2026-10-28): Camera Raw applies
/// the camera's CA knots with the profile switched off, measured on a raw
/// whose CA knots were zeroed and quadrupled — red over green moved by the
/// knots' own polynomial with the switch at "0" exactly as at "1" under
/// `LensDefaults`. CA moves no green pixel, so the mask frame is unaffected.
///
/// A function rather than four lines inline because the state it acts on cannot
/// be reached from a test through [`fresh_lens_profile`]: that reads a RAW, and
/// with no RAW on disk the three toggles are already false, so an assertion on
/// them passed whatever this code did. The test now hands a profile with every
/// component ON to this function — the real one, not a sketch of it.
fn obey_disabled_profile_switch(p: &mut crate::recipe::LensProfile) {
    retain_disabled_linear_handle_warp(p);
    p.vignette_on = false;
    p.distortion_on = false;
}

/// Set the solved camera/LCP map aside across the disabled-sidecar boundary
/// (it was LINEAR H2's; no render path reads it since v1.17.0) so RADIAL's
/// `mask_warp` stays empty. Kept at this upstream boundary so every importer
/// and render surface receives one coherent frame fact.
fn retain_disabled_linear_handle_warp(p: &mut crate::recipe::LensProfile) {
    p.linear_handle_warp = std::mem::take(&mut p.mask_warp);
    p.mask_warp_src = crate::recipe::MaskWarpSource::DisabledInSidecar;
}

/// Fresh as-shot WB anchor for `raw` — the camera's absolute Kelvin + tint
/// from its own metadata (`render::as_shot_wb`; metadata-only decode, no
/// demosaic). `(None, None)` when unavailable (non-RAW, no colour matrix,
/// damaged coefficients): the engine then keeps its historical 5500 K anchor.
pub fn fresh_as_shot_wb(raw: &Path) -> (Option<f32>, Option<f32>) {
    match crate::render::as_shot_wb(raw) {
        Some((k, t)) => (Some(k), Some(t)),
        None => (None, None),
    }
}

/// Fresh camera-matched base-look estimate for `raw`: a neutral develop
/// CDF-matched against the embedded preview (`render::camera_base_knots`).
///
/// Since v1.17.0 it has two readers and no other: the start of a fresh develop
/// on a machine that cannot render Lightroom's canvas ([`fresh_canvas_start`]),
/// and the re-estimate of a curve an earlier era stamped
/// ([`repair_pre_era_base_curve_in`]). A fresh open where Camera Raw is
/// installed starts from the profile instead and never pays this demosaic.
/// Best-effort by design: a develop/decode failure yields "no base look"
/// rather than failing the caller's real operation (whose own render will
/// surface the same error loudly). Callers that must tell that failure apart
/// from the estimator's own empty answer use [`photo_base_knots_checked_in`].
fn photo_base_knots_in(raw: &Path, d: &crate::diag::Diag) -> Vec<[f32; 2]> {
    photo_base_knots_checked_in(raw, d).unwrap_or_default()
}

/// The checked form of [`photo_base_knots_in`]: `None` is an INABILITY (a
/// non-RAW, no embedded preview, a failed read or develop, too few pixels to
/// judge) where the unchecked form hands back an empty curve — the estimator's
/// own empty answer is `Some`. The three disclosures below each name one of
/// those inabilities, and each is about the photograph the channel is bound
/// to — which is why they no longer spell its path themselves: the sink
/// renders the attribution.
pub fn photo_base_knots_checked_in(
    raw: &Path,
    d: &crate::diag::Diag,
) -> Option<Vec<[f32; 2]>> {
    if !decode::is_raw(raw) {
        return None;
    }
    let camera = match decode::embedded_preview(raw) {
        Ok(Some(c)) => c,
        Ok(None) => return None,
        Err(e) => {
            d.warn(format!("base look skipped: the embedded preview could not be read ({e})"));
            return None;
        }
    };
    // The estimate is CDF statistics — a ≤2048 working develop carries the
    // same histogram signal as 61 MP at a fraction of the transients.
    match crate::render::render_to_image(raw, &EditRecipe::default(), None, Some(2048)) {
        Ok(neutral) => {
            // The one entry of the three open paths: the frame the rendition
            // SHOWS (v1.2.2) and the corners it shows (2026-09-21) are both
            // settled there, on the pair itself.
            match crate::render::camera_base_look(&neutral, &fresh_lens_profile(raw), &camera) {
                Some(k) => Some(k),
                None => {
                    // Could not JUDGE (too few pixels on a side) — an
                    // inability like the arms above, never a verdict.
                    d.warn("base look skipped: too few pixels to compare");
                    None
                }
            }
        }
        Err(e) => {
            // Disclosed, not silent: the caller's own render will surface the
            // same failure loudly, but the resulting darker-than-canvas output
            // needs a traceable cause in the log.
            d.warn(format!("base look skipped: the neutral develop failed ({e})"));
            None
        }
    }
}

/// Publish the recipe for `raw`. `sink` receives the clamp disclosure, if the
/// write costs anything (R29-1: it used to be a bare `eprintln!`).
pub fn write_recipe(
    raw: &Path,
    recipe: &EditRecipe,
    out: Option<PathBuf>,
    sink: &dyn crate::diag::Sink,
) -> Result<PathBuf> {
    let out = out.unwrap_or_else(|| crate::store::recipe_target(raw));
    // `-o some_dir` names an existing DIRECTORY: refuse instead of renaming
    // the whole directory to .bak below and publishing a recipe FILE in its
    // place (the user meant "write into it", which this API does not do).
    if out.is_dir() {
        anyhow::bail!(
            "recipe target {} is a directory — pass a file path (e.g. {}/recipe.json)",
            out.display(),
            out.display()
        );
    }
    ensure_parent(&out)?;
    let bytes = recipe_bytes_for(recipe, out.parent(), &crate::diag::Diag::about(sink, raw))?;
    // Publish via tmp+rename rather than truncating the AUTHORITATIVE file in
    // place: a crash mid-write used to leave a half-written recipe.json (loud
    // Unreadable, but the develop was gone). The old file is retired to .bak
    // first for CRASH RECOVERY (fs::rename does replace an existing file on
    // Windows — verified empirically) — worst case is a briefly missing file
    // with the intact .tmp beside it, never corrupt JSON.
    // Per-process AND per-call tmp name from the ONE shared store counter
    // (see store::next_tmp_seq): a GUI and a web server saving the same
    // photo used to share one fixed .tmp, the web server threads REQUESTS,
    // and a same-process migration publish mints into the SAME
    // `.json.tmp.<pid>.<seq>` namespace — every site must share one counter.
    // (.bak stays shared — the retire/restore pair below is
    // last-writer-wins by design; one photo has one interactive writer in
    // practice.)
    if out == crate::store::recipe_target(raw) {
        crate::store::durable_retire_and_write(
            &out,
            &out.with_extension("json.bak"),
            &bytes,
        )
        .with_context(|| format!("publish recipe {}", out.display()))?;
        crate::store::note_source(raw); // breadcrumb for the hashed store dir
    } else {
        // A redirected `-o` (and every v<N> snapshot) is OUTSIDE
        // recover_orphan_baks' pair list: retiring here stranded the old
        // bytes as an unrecoverable `.bak` beside a MISSING live file when a
        // crash hit the retire window — `apply` then saw no recipe at all.
        // Stage+rename replaces the previous bytes atomically instead: no
        // missing-file window, no orphan a reader will never republish.
        crate::store::durable_write(&out, &bytes)
            .with_context(|| format!("publish recipe {}", out.display()))?;
    }
    Ok(out)
}

/// The exact on-disk recipe bytes for `anchor`: a relativized, clamped COPY —
/// the caller's in-memory recipe keeps its absolute paths for rendering.
/// Rasters living beside the recipe are stored by bare file name so the
/// develop dir stays relocatable (store::resolve_mask_paths re-anchors them
/// at load). The size caps belong to the WRITE, not to each caller's memory:
/// every render route clamped its untrusted input and `api_xmp` — the one
/// that persists — did not, so a hostile body landed on disk as the photo's
/// authoritative recipe.json and was re-parsed by every later reader. The
/// routes that already clamp see a no-op — a floor no future route can
/// forget to stand on.
///
/// `diag` is the caller's channel for the clamp disclosure below, carrying the
/// photograph this recipe belongs to (R28 Batch-5 5c threaded the path;
/// R29-1 threads the whole channel). `anchor` cannot stand in for the
/// photograph: it is a DIRECTORY (the develop dir, or wherever `-o` pointed),
/// so it names the store's hash, not the picture.
fn recipe_bytes_for(
    recipe: &EditRecipe,
    anchor: Option<&std::path::Path>,
    diag: &crate::diag::Diag<'_>,
) -> Result<Vec<u8>> {
    let mut on_disk = recipe.clone();
    let dropped = on_disk.clamp();
    if !dropped.is_empty() {
        // describe(): only the non-zero losses — curve/string truncation was
        // invisible behind a "0 mask(s)" line (16-lane scan L16).
        diag.emit(
            crate::diag::Mark::WarningWord,
            format!("recipe limits discarded {}", dropped.describe()),
        );
    }
    if let Some(parent) = anchor {
        crate::store::relativize_mask_paths(&mut on_disk, parent);
    }
    Ok(serde_json::to_vec_pretty(&on_disk)?)
}

/// The CENTRAL-STORE recipe bytes for `raw` — identical to what a plain
/// [`write_recipe`] publishes — for staging into a
/// [`crate::store::commit_develop`] single-generation save.
pub fn recipe_store_bytes(
    raw: &Path,
    recipe: &EditRecipe,
    sink: &dyn crate::diag::Sink,
) -> Result<Vec<u8>> {
    let target = crate::store::recipe_target(raw);
    recipe_bytes_for(recipe, target.parent(), &crate::diag::Diag::about(sink, raw))
}

/// First FREE pixel-master path for `tag` (`tag`, `tag-2`, … `tag-999`) in
/// the store's masters folder ([`crate::store::masters_dir`], not the
/// delivery folder the user cleans),
/// CLAIMED atomically (`create_new`): pixels.json links these paths, GUI undo
/// history holds them, and a CANCELLED worker may still be running toward the
/// name it probed — an existence probe alone would hand the same name to the
/// replacement task and let the two write over each other. The claimed
/// placeholder is simply overwritten by the worker's save. `None` past the
/// 999 cap (refuse, never alias). Shared by the GUI's five starters and the
/// web fill/heal handlers — a fixed web output name used to overwrite a
/// master an earlier develop still referenced.
pub fn unique_out(path: &Path, tag: &str) -> Option<PathBuf> {
    // n = 0 → "tag"; n = 1..=998 → "tag-2".."tag-999" (never tag-1000).
    for n in 0..=998u32 {
        let t = if n == 0 { tag.to_string() } else { format!("{tag}-{}", n + 1) };
        let cand = crate::store::masters_dir().join(format!("{}.{t}.png", stem(path)));
        if ensure_parent(&cand).is_err() {
            return None;
        }
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&cand) {
            Ok(_) => return Some(cand),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// First FREE deliverable path for `out`: `out` itself, then
/// `<stem>-2.<ext>` … `<stem>-999.<ext>` beside it, CLAIMED atomically
/// (`create_new`) the way [`unique_out`] claims a master — so two saves
/// racing for one name cannot both take it, and an existing file is never
/// replaced. The parent folder is made first. The claimed placeholder is
/// empty; the render's staged publish replaces it, and a caller whose render
/// fails removes it. Errors past the 999 cap (refuse, never alias). The GUI's
/// Normal mode saves through this: a second save of one photo lands as
/// `-2`, not as a refusal and not over the first.
pub fn claim_numbered(out: &Path) -> Result<PathBuf> {
    ensure_parent(out)?;
    let parent = out.parent().unwrap_or(Path::new(""));
    let file_stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = out.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 1..=999u32 {
        let cand = if n == 1 { out.to_path_buf() } else { parent.join(format!("{file_stem}-{n}{ext}")) };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&cand) {
            Ok(_) => return Ok(cand),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("create {}", cand.display())),
        }
    }
    anyhow::bail!("{} and 998 numbered copies of it already exist — pick another folder", out.display())
}

/// Write the develop's XMP projection: the published path, the note when
/// the merge could not be performed and the sidecar was REGENERATED instead
/// (see [`write_xmp_doc`] for why that is a loss worth telling the user
/// about), and the per-mask list of what the projection itself could not
/// carry (M6a — bitmap/muted masks skipped, bitmap components omitted, radial
/// rotation dropped, recolour gains past Temp/Tint's reach;
/// [`xmp::mask_export_losses`]).
/// Every caller receives both (round-12 disclosure threading): the old
/// note-dropping `write_xmp` wrapper was how five of seven surfaces stayed
/// silent, so the values ride the RETURN TYPE where a surface has to look at
/// them. stderr already hears both via `write_xmp_doc`; UI surfaces route
/// what they are handed here.
///
/// `sink` receives the two ungated lines this path raises (the merge note and
/// the mask-loss list) — R29-1: the caller decides where they go, and the
/// SUBJECT is bound from `raw` here rather than taken on trust.
pub fn write_xmp(
    raw: &Path,
    recipe: &EditRecipe,
    sink: &dyn crate::diag::Sink,
) -> Result<(PathBuf, Option<String>, Vec<xmp::MaskLoss>)> {
    let target = xmp_target(raw);
    let (bytes, note, losses) = xmp_projection_bytes(raw, recipe, sink)?;
    // Stage + rename, never truncate in place (see `write_xmp_doc`).
    crate::store::durable_write(&target, &bytes)
        .with_context(|| format!("publish xmp {}", target.display()))?;
    Ok((target, note, losses))
}

/// The `xmp` member of a [`crate::store::DevelopCommit`] for the develop
/// being committed, with the projection's two disclosures (the merge note
/// and the per-mask loss list) for the caller's own channel.
///
/// The projection is DERIVED from `recipe.json` and lives or dies with it,
/// in the same generation:
///
/// * `on_source` — the develop describes the RAW itself (an Original or
///   Reverse-fit card; the CLI's fits and analyses; a web save over no
///   generated master) — and the photo is a camera RAW: `Write`, the bytes
///   [`write_xmp`] would publish;
/// * otherwise `Clear`: a develop that sits on AI-generated pixels has no
///   sidecar that reproduces those pixels, and a baked PNG/JPEG has no
///   Lightroom-sidecar convention at all.
///
/// Until 2026-09-13 every writer published the projection by a SEPARATE
/// call after its commit — and the quit-time Save-all skipped that call for
/// a generated card without retiring what stood there. A projection written
/// for a reverse-fit card therefore outlived the card (deleted) and the
/// recipe it projected (replaced by the generated card's neutral one), and
/// the open path restored its +90 saturation over the pristine AI-generated
/// pixels. `Clear` in the same generation is the writer-side half of that
/// fix; the readers' half is that a present `recipe.json` — neutral or not
/// — is never out-answered by the projection beside it.
pub fn xmp_projection_member(
    raw: &Path,
    recipe: &EditRecipe,
    on_source: bool,
    sink: &dyn crate::diag::Sink,
) -> Result<(crate::store::CommitMember, Option<String>, Vec<xmp::MaskLoss>)> {
    if !on_source || !crate::decode::is_raw(raw) {
        return Ok((crate::store::CommitMember::Clear, None, Vec::new()));
    }
    let (bytes, note, losses) = xmp_projection_bytes(raw, recipe, sink)?;
    Ok((crate::store::CommitMember::Write(bytes), note, losses))
}

/// What [`resolve_develop_masks`] did: the AI masks the local segmenter
/// re-derived (or could not), and the face / eyes / skin masks the face finder
/// outlined (or took out).
pub struct DevelopMasks {
    pub ai: crate::segment::AiMaskResolution,
    pub faces: crate::face::FaceMaskResolution,
}

impl DevelopMasks {
    /// The line a surface shows as a WARNING, or `None` when every mask that
    /// asked for a raster got one: the masks left inert or taken out, with the
    /// segmenter's and face finder's own sentences.
    pub fn warning(&self) -> Option<String> {
        let mut lines: Vec<String> = Vec::new();
        if !self.ai.unresolved.is_empty() {
            lines.extend(self.ai.describe());
        }
        if !self.faces.dropped.is_empty() {
            lines.extend(self.faces.describe());
        }
        (!lines.is_empty()).then(|| lines.join("; "))
    }
}

/// THE "resolve before develop" step: give every mask that names no raster
/// yet its raster, immediately before the render that needs it — an AI mask
/// imported from a Lightroom sidecar (`segment::resolve_ai_masks`, which the
/// importer deliberately leaves `raster: None`) and a face / eyes / skin
/// intent (`face::resolve_face_masks`). Cached beside the photo's develop, so
/// a second develop costs a PNG probe.
///
/// `raw` keys and homes the caches; `src` is the render source's pixels (a
/// saved retouch master, when there is one). Until 2026-10-26 only the CLI's
/// `apply` / `auto` and the analysis flow called the two resolvers, so the
/// browser rendered an imported AI mask inert (`render::mask_raster` skips an
/// adjustment whose raster is `None`) while the manual promises it is
/// re-derived locally at develop time. Every develop surface calls this.
pub fn resolve_develop_masks(
    cfg: &crate::config::Config,
    raw: &Path,
    src: &Path,
    recipe: &mut EditRecipe,
) -> DevelopMasks {
    let ai = crate::segment::resolve_ai_masks(cfg, raw, src, recipe);
    let faces = crate::face::resolve_face_masks(cfg, raw, src, recipe);
    DevelopMasks { ai, faces }
}

/// What became of the Lightroom projection in a [`commit_analyzed_develop`].
pub enum ProjectionOutcome {
    /// Published at `path`, with the merge note and the mask verdicts.
    Written { path: PathBuf, note: Option<String>, losses: Vec<xmp::MaskLoss> },
    /// Retired: the develop sits on AI-generated pixels, or the photo is not
    /// a camera RAW — no sidecar reproduces it.
    Cleared,
    /// The projection could not be produced; the recipe still landed and any
    /// earlier projection is kept. Said, never fatal (the cross-surface rule).
    Failed(anyhow::Error),
}

/// Publish a PROGRAMMATIC develop (Analyze, web and CLI) the way a save
/// publishes one: recipe, strip record and Lightroom projection in ONE
/// single-generation commit ([`crate::store::commit_develop`]).
///
/// Before 2026-10-26 both Analyze surfaces called `write_recipe` and then
/// `write_xmp`, so over an AI-generated master they still wrote a projection
/// (which no sidecar can honour — [`xmp_projection_member`]) and left the
/// strip record calling the edited develop the pristine generated card. The
/// rule is the web save's: the develop sits on the RECORDED generated master
/// when there is one (`ActiveWrite::DevelopOnAiPixels`), else on the source
/// (`ActiveWrite::Unknown`, the record stands); the pixel link is kept.
/// The caller runs the backup gate first, under its develop lock.
pub fn commit_analyzed_develop(
    raw: &Path,
    recipe: &EditRecipe,
    sink: &dyn crate::diag::Sink,
) -> Result<ProjectionOutcome> {
    let ai_master = crate::store::recorded_pixel_source(raw).and_then(|(o, g)| g.then_some(o));
    let variants = crate::store::variants_member(
        raw,
        match &ai_master {
            Some(m) => crate::store::ActiveWrite::DevelopOnAiPixels { recipe, master: m },
            None => crate::store::ActiveWrite::Unknown,
        },
    )?;
    let (xmp, outcome) = match xmp_projection_member(raw, recipe, ai_master.is_none(), sink) {
        Ok((crate::store::CommitMember::Write(b), note, losses)) => (
            crate::store::CommitMember::Write(b),
            ProjectionOutcome::Written { path: xmp_target(raw), note, losses },
        ),
        Ok((member, _, _)) => (member, ProjectionOutcome::Cleared),
        Err(e) => (crate::store::CommitMember::Keep, ProjectionOutcome::Failed(e)),
    };
    crate::store::commit_develop(
        raw,
        crate::store::DevelopCommit {
            recipe: Some(recipe_store_bytes(raw, recipe, sink)?),
            pixels: crate::store::CommitMember::Keep,
            variants,
            xmp,
        },
    )?;
    Ok(outcome)
}

/// The projection's BYTES without publishing them: exactly what
/// [`write_xmp`] puts at [`xmp_target`] for `raw` — merge base (the sidecar
/// Lightroom writes beside the RAW, else the previous projection) and all —
/// so a develop commit can stage the sidecar into the same generation as the
/// recipe it projects. Same return shape as the writer minus the path.
pub fn xmp_projection_bytes(
    raw: &Path,
    recipe: &EditRecipe,
    sink: &dyn crate::diag::Sink,
) -> Result<(Vec<u8>, Option<String>, Vec<xmp::MaskLoss>)> {
    use crate::store::SidecarRead;
    let target = xmp_target(raw);
    // MERGE, never regenerate over Lightroom's work (A11): the base is the
    // sidecar Lightroom itself writes (beside the RAW) when one exists — its
    // LR-only properties (the camera profile / Look, PointColor, LR
    // lens-profile data, foreign namespaces) survive our save, so the file
    // the user copies back beside the RAW keeps them. Else the previous
    // projection at the destination, which carries forward whatever an
    // earlier merge preserved.
    //
    // The base carries ITS OWN PATH so a failed merge can name the file whose
    // properties are lost — the note used to name the OUTPUT for every base,
    // pointing users at a file that was never the problem. And an UNREADABLE
    // base is a disclosure, never a silent fall-through: `read_sidecar`'s
    // one-Option shape let an over-the-cap Lightroom sidecar be silently
    // replaced by our own previous projection as the merge base, so a REAL
    // loss produced no note — the exact silence this function exists to end.
    let mut notes: Vec<String> = Vec::new();
    let mut base: Option<(PathBuf, String)> = None;
    // Only a camera RAW has a Lightroom-sidecar convention; a baked
    // PNG/TIFF's neighbouring .xmp is someone else's file (store::
    // lightroom_sidecar draws the same line for restore). Reading it anyway
    // meant a foreign sidecar was consulted on every save of a baked photo.
    if crate::decode::is_raw(raw)
        && let Some(lr) = crate::xmp_pair::XmpPairing::beside().find(raw)
    {
        match crate::store::read_sidecar_checked(&lr) {
            // A blank file carries nothing to merge and nothing to lose.
            SidecarRead::Ok(t) if !t.trim().is_empty() => base = Some((lr, t)),
            SidecarRead::Ok(_) | SidecarRead::Missing => {}
            // Deliberately says "may not be", not "are not": the save falls
            // through to the previous projection below, which carries forward
            // whatever an earlier merge preserved — so some of that sidecar's
            // properties can survive by that route. What is certain is that
            // anything added to it SINCE is not represented, and that is the
            // loss worth naming.
            SidecarRead::Unreadable(why) => notes.push(format!(
                "the Lightroom sidecar at {} could not be read ({why}), so this save was \
                 merged against AutoShade's own previous version instead — anything changed \
                 in that sidecar since is not in the new file; the sidecar itself is untouched",
                lr.display()
            )),
        }
    }
    if base.is_none() {
        match crate::store::read_sidecar_checked(&target) {
            SidecarRead::Ok(t) if !t.trim().is_empty() => base = Some((target.clone(), t)),
            SidecarRead::Ok(_) | SidecarRead::Missing => {}
            SidecarRead::Unreadable(why) => notes.push(format!(
                "the previous XMP at {} could not be used as the merge base ({why}) — \
                 it is replaced by a regenerated file that carries none of its properties",
                target.display()
            )),
        }
    }
    let (doc, note, losses) =
        build_xmp_doc(&target, recipe, base, &crate::diag::Diag::about(sink, raw), notes)?;
    Ok((doc.into_bytes(), note, losses))
}

/// Write the XMP to an EXPLICIT path. Used when the recipe was redirected with
/// `-o`: the two halves of one develop must stay in the same folder, or the
/// GUI/web would keep restoring an older `out/<stem>.xmp` instead.
///
/// `diag` carries BOTH halves the old `photo: Option<&Path>` parameter was
/// standing in for (R29-1): the photograph — read back out for the frame
/// aspect — and the channel its disclosures travel on.
pub fn write_xmp_at(
    out: PathBuf,
    recipe: &EditRecipe,
    diag: &crate::diag::Diag<'_>,
) -> Result<(PathBuf, Option<String>, Vec<xmp::MaskLoss>)> {
    use crate::store::SidecarRead;
    let mut notes: Vec<String> = Vec::new();
    let base = match crate::store::read_sidecar_checked(&out) {
        SidecarRead::Ok(t) if !t.trim().is_empty() => Some((out.clone(), t)),
        SidecarRead::Ok(_) | SidecarRead::Missing => None,
        SidecarRead::Unreadable(why) => {
            notes.push(format!(
                "the previous XMP at {} could not be used as the merge base ({why}) — \
                 it is replaced by a regenerated file that carries none of its properties",
                out.display()
            ));
            None
        }
    };
    write_xmp_doc(out, recipe, base, diag, notes)
}

/// Returns the published path and, when the merge could not run, a note naming
/// what that cost.
///
/// A base the splicer cannot account for falls back to a FRESH document rather
/// than failing the save — but that fallback DROPS exactly what the merge
/// exists to protect: the camera profile / creative `Look`, `crs:PointColor`,
/// Lightroom's own lens-profile block, foreign namespaces, the xpacket
/// wrapper. Doing it silently re-entered the A11 data loss through the back
/// door: the user saw a plain "saved" and found their Lightroom work gone the
/// next time they opened the catalog. Everywhere else in this codebase a
/// degraded result is disclosed (`render_source_checked` refuses,
/// `unparsable_crs_numbers` warns about far smaller losses); this now matches.
fn write_xmp_doc(
    out: PathBuf,
    recipe: &EditRecipe,
    merge_base: Option<(PathBuf, String)>,
    diag: &crate::diag::Diag<'_>,
    notes: Vec<String>,
) -> Result<(PathBuf, Option<String>, Vec<xmp::MaskLoss>)> {
    let (doc, note, losses) = build_xmp_doc(&out, recipe, merge_base, diag, notes)?;
    // Stage + rename, never truncate in place: `fs::write` opens the LIVE
    // sidecar with O_TRUNC, so a full disk, an interruption or a competing
    // writer left a truncated file where a valid Lightroom sidecar used to
    // be — the previous projection destroyed by the failed attempt to
    // replace it. (fs::rename replaces the destination on every platform.)
    crate::store::durable_write(&out, doc.as_bytes())
        .with_context(|| format!("publish xmp {}", out.display()))?;
    Ok((out, note, losses))
}

/// The document half of [`write_xmp_doc`]: the projection's text plus its
/// two disclosures, with nothing written. `out` is only NAMED here — the
/// regenerated-base note says which file lost what — so the same builder
/// serves the direct writer and the commit member ([`xmp_projection_bytes`]).
fn build_xmp_doc(
    out: &Path,
    recipe: &EditRecipe,
    merge_base: Option<(PathBuf, String)>,
    diag: &crate::diag::Diag<'_>,
    mut notes: Vec<String>,
) -> Result<(String, Option<String>, Vec<xmp::MaskLoss>)> {
    // ONE source for the photograph: the channel this write discloses on. A
    // separate `photo` parameter beside a separate attribution could disagree,
    // and the frame aspect below is decided by the same identity the warnings
    // are attributed to.
    let photo = diag.photo();
    // The same floor `write_recipe` stands on, for the same reason: this is a
    // persistence boundary, the string caps exist because `rationale` is
    // filled from an upstream failure body, and the XMP lands BESIDE THE RAW
    // in the user's library. recipe.json was protected structurally and this
    // half only by the convention that every caller clamps first.
    let mut bounded = recipe.clone();
    bounded.clamp();
    let recipe = &bounded;
    let base_path = merge_base.as_ref().map(|(p, _)| p.clone());
    // The frame the geometry projection writes INTO (`xmp::FrameAspect`).
    // A base sidecar that declares its own `tiff:ImageWidth/ImageLength` still
    // wins over this — see `merge_recipe_into_xmp_in_frame`. Unreadable = no
    // frame = the loss is disclosed rather than guessed, the same tri-state
    // `migrate_recipe_coord_frame` takes on the orientation it cannot read.
    //
    // R27 WIDENED THE GATE. v0.32.0 paid for this only on a rotated radial,
    // because nothing else in the projection needed an aspect. Two
    // measurements changed that: a tilted CROP is a rotated-corner encoding in
    // the source frame and cannot be written without `W/H`
    // (`P3-cropangle-model.md` §2), and EVERY geometry on a portrait capture
    // has to be turned back into the sensor frame before it is written
    // (`P1-portrait-mask-frame.md` §5). So the gate is now "does this recipe
    // carry frame coordinates at all", which is one memoised metadata read per
    // photo per process — and it is no longer "a cost for nothing".
    //
    // R29 C1 widened what that PREDICATE answers rather than the gate itself: a
    // brush group is a frame coordinate now, so a brush-only recipe fetches the
    // aspect where it used to skip it. That is not incidental — `in_source_
    // frame` needs the frame to un-turn the dab stream, and without this the
    // writer would have emitted a portrait capture's brush in the display
    // frame while its `tiff:` block declared the sensor one.
    //
    // W14 widened it once more, for the same reason and one field further out:
    // a QUARTER TURN is a frame fact even when the recipe holds no coordinate
    // at all. Without this a photo rotated in AutoShade and saved with nothing
    // else on it wrote a sidecar that declared no orientation, so the rotation
    // never reached Lightroom — the export half of the same law the import
    // half now reads (`render::quarter_turns_between`).
    let frame = photo
        .filter(|_| {
            crate::render::recipe_has_frame_coords(recipe)
                || recipe.straighten_deg != 0.0
                || !recipe.quarter_turns.is_multiple_of(4)
        })
        .and_then(|p| photo_frame_aspect(p, recipe.quarter_turns));
    let merged = merge_base.and_then(|(_, b)| {
        xmp::merge_recipe_into_xmp_in_frame_for_photo(&b, recipe, frame, photo)
    });
    // A merge that SUCCEEDED can still have losses it could not avoid (the
    // base's unrepresentable mask block giving way to the recipe's own
    // masks) — its notes ride the same channel as the regeneration note.
    let merged = merged.map(|mut o| {
        notes.append(&mut o.notes);
        (o.doc, o.losses)
    });
    if merged.is_none()
        && let Some(bp) = base_path
    {
        // The note names the file whose properties are LOST — the base — and
        // is honest about what happened to it: a base at the output path is
        // genuinely replaced by the regeneration; a base beside the RAW is
        // not touched at all, only unrepresented in the new file. "The camera
        // creative Look, LR lens-profile data" are EXAMPLES of what a
        // Lightroom base carries, not a claim about this one — a ratings-only
        // sidecar loses its ratings the same way. (Lightroom's Texture used to
        // head that list; R25 B2 models it, so a regenerated file DOES carry
        // it and naming it here would have made the disclosure false. The
        // "camera profile" went the same way in R25 B4: `crs:CameraProfile`
        // and the Transform block ride in `EditRecipe::passthrough` and are
        // rewritten by the regeneration too — the creative `Look` is a NESTED
        // element with no string spelling and is still genuinely lost, which
        // is why it is the example that stayed.)
        notes.push(if bp.as_path() == out {
            format!(
                "the existing sidecar at {} could not be merged — it was regenerated, \
                 so properties it carried (e.g. Lightroom's creative Look, \
                 LR lens-profile data) are not in the new file",
                out.display()
            )
        } else {
            format!(
                "the sidecar at {} could not be merged — the new file at {} was \
                 regenerated without the properties it carried (e.g. Lightroom's \
                 creative Look, LR lens-profile data, ratings); \
                 the sidecar itself is untouched",
                bp.display(),
                out.display()
            )
        });
    }
    // Both disclosure lines in this function travel the caller's channel and
    // carry the photograph when the caller knows one (R28 Batch-5 5c stamped
    // them; R29-1 routes them). The merge/export path had the identity all
    // along and simply did not print it, which is what made a `--jobs 3`
    // transcript unreadable.
    let note = (!notes.is_empty()).then(|| {
        let msg = notes.join("; ");
        diag.warn(msg.clone());
        // The RETURNED note is unprefixed: it travels the structured channel to
        // a caller that already knows which photo it asked about (and the GUI
        // localises it), so stamping it there would print the stem twice.
        msg
    });
    // M6a: the projection's OWN lossy edges (bitmap/muted masks skipped, extra
    // bitmap components omitted, rotation + recolour dropped) — judged by the WRITER on
    // the CLAMPED recipe, i.e. exactly the masks the document carries. The
    // import direction had four disclosure sites and the export direction had
    // none; this is that half. The caller's channel covers every CLI path from
    // here — one line, one place — the same deal the merge note above has;
    // UI/web surfaces localise the structured list they get back.
    //
    // The document and the verdicts arrive TOGETHER (R22 NIT-1): both arms
    // produce the list while emitting the mask block, so this no longer calls
    // `mask_export_losses` and builds that block a second time per save.
    let (doc, mask_losses) = match merged {
        Some(pair) => pair,
        None => xmp::recipe_to_xmp_in_frame_for_photo(recipe, frame, photo),
    };
    for line in xmp_loss_lines(recipe, &mask_losses) {
        diag.warn(line);
    }
    Ok((doc, note, mask_losses))
}

/// What a projection of `recipe` leaves behind, as the English lines a CLI or
/// web surface shows: the mask half (`losses`, the writer's own verdicts) and
/// the GLOBAL half ([`xmp::global_export_losses`] — a lens blur, fill layers,
/// spot removal, a colour field, the camera calibration). Empty = faithful.
///
/// One door for both halves: until 2026-10-26 only the GUI asked the global
/// question, so a recipe saved from the CLI or the browser dropped those
/// controls from the sidecar without a word.
pub fn xmp_loss_lines(recipe: &EditRecipe, losses: &[xmp::MaskLoss]) -> Vec<String> {
    xmp::describe_mask_losses(losses)
        .into_iter()
        .chain(xmp::describe_global_losses(&xmp::global_export_losses(recipe)))
        .collect()
}

/// The photo's own SOURCE frame — un-rotated size plus the turn that carries
/// it to the display frame — through [`source_frame_memo`]: both answers cost
/// the same metadata read, and the strip-card and batch surfaces ask per
/// recipe, not per photo. `None` for a photo whose frame cannot be read; the
/// caller discloses that rather than guessing an aspect.
///
/// **The SOURCE frame, not the display one** (R27 A7). `decode::frame_size`
/// applies the orientation and hands back what the render produces; the frame
/// a `crs:` coordinate is measured against is the one the FILE stores
/// (`P1-portrait-mask-frame.md` §1: a portrait export's mask numbers are
/// fractions of the 9504 × 6336 sensor array even though its pixels are
/// 6336 × 9504). Feeding the display dimensions here inverted the aspect for
/// every portrait capture — the projection's `s = W/H` came out as `H/W`.
///
/// `quarter_turns` is the photographer's own rotation, composed onto the EXIF
/// state, so the declared `tiff:Orientation` describes the frame this build
/// actually displays — closing the Batch-2 registration that a sidecar written
/// for a turned photo described the un-turned frame.
///
/// **This composition IS the export half of the sidecar orientation law**
/// (`render::quarter_turns_between`, which states it and cites the
/// measurement): the reader solves `compose_orientation(EXIF, quarter_turns)
/// == tiff:Orientation` for the turn, and this is the same equation run
/// forwards so the writer declares the frame the render delivers.
fn photo_frame_aspect(photo: &Path, quarter_turns: u8) -> Option<xmp::FrameAspect> {
    let ((w, h), exif) = source_frame_memo(photo)?;
    xmp::FrameAspect::from_size_turned(
        w as f64,
        h as f64,
        crate::render::compose_orientation(exif, quarter_turns),
    )
}

/// Where the .xmp for `raw` goes — the photo's central develop dir (see
/// `store`; the photo library itself stays read-only). Kept here because every
/// surface already imports it from pipeline.
pub fn xmp_target(raw: &Path) -> PathBuf {
    crate::store::xmp_target(raw)
}

/// Fold `.`/`..` LEXICALLY (no filesystem access — the target may not exist
/// yet). Shared by [`guard_readonly`] and the CLI's canonical-path equality
/// check (`-o` spelled with `dir/../` segments must still classify as the
/// canonical file). See the RootDir note inside for the drive-relative trap.
pub fn normalize_lexical(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut n = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => match n.components().next_back() {
                Some(Component::Normal(_)) => {
                    n.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {
                    // The root's parent IS the root: "<drive>/../lib" folds to
                    // "<drive>/lib" exactly as the filesystem resolves it.
                    // Popping the root would yield drive-relative "D:lib",
                    // which dodges starts_with checks.
                }
                _ => n.push(".."),
            },
            Component::CurDir => {}
            other => n.push(other.as_os_str()),
        }
    }
    n
}

/// Canonicalize the deepest EXISTING ancestor, then re-attach the
/// not-yet-created tail: junction/subst/symlink aliases and case-flipped
/// spellings resolve to the true on-disk form even when the LEAF is absent.
/// Shared by [`guard_readonly`] and the CLI's canonical-path equality check.
pub fn resolve_existing_pub(p: &Path) -> PathBuf {
    let mut cur = p;
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    loop {
        if let Ok(mut c) = std::fs::canonicalize(cur) {
            for t in tail.iter().rev() {
                c.push(t);
            }
            return c;
        }
        match (cur.parent(), cur.file_name()) {
            (Some(par), Some(name)) if !par.as_os_str().is_empty() => {
                tail.push(name);
                cur = par;
            }
            _ => return p.to_path_buf(),
        }
    }
}

/// By default, AutoShade never overwrites a photo. A destination that would
/// create a NEW file is writable wherever the user put it — including a folder
/// inside the photo library — because the guard protects the photo FILES, not
/// the folders around them: a subfolder the user picked (`exports`,
/// contact sheets) or a new name beside the originals is a destination, and
/// nothing of theirs is lost by writing it.
///
/// The PROJECT's ./out and the store's own areas (the develop dirs and the
/// pixel masters, `store::own_areas_in`) are always writable, even when the
/// source itself lives there (e.g. `match` fitting a look onto a previously
/// exported preview) — overwriting OUR OWN deliverables is the point of those
/// areas.
pub fn guard_readonly(out: &Path, raw: &Path) -> Result<()> {
    guard_readonly_in(out, raw, &crate::store::store_root(), &crate::config::delivery_root())
}

/// [`guard_readonly`] over an explicit store root and delivery root, so the
/// tests can site both without touching the process-global settings.
pub(crate) fn guard_readonly_in(out: &Path, raw: &Path, store: &Path, delivery: &Path) -> Result<()> {
    use std::path::absolute;
    // Fold `.`/`..` LEXICALLY (no filesystem access — the target may not
    // exist yet): `std::path::absolute` keeps `..` segments, so a path like
    // "out/../<library>/x.tif" used to START WITH ./out for the allow-check
    // while the filesystem resolved it INTO the protected library.
    let normalize = normalize_lexical; // extracted for the CLI's same_path
    // Canonicalize the deepest EXISTING ancestor, then re-attach the
    // not-yet-created tail: a junction / subst / symlink alias or a
    // case-flipped spelling ("photos" for "Photos" — NTFS is
    // case-insensitive, starts_with is not) otherwise denotes the library
    // while failing every lexical comparison below. Canonical paths carry the
    // TRUE on-disk casing and resolved links, and all sides go through the
    // same lens, so the \\?\ verbatim prefix cancels out.
    let resolve_existing = resolve_existing_pub; // extracted for same_path
    let (Ok(out_abs), Ok(raw_abs)) = (absolute(out), absolute(raw)) else {
        return Ok(());
    };
    let (out_abs, raw_abs) =
        (resolve_existing(&normalize(&out_abs)), resolve_existing(&normalize(&raw_abs)));
    // The store's OWN areas, not its whole root: `AUTOSHADE_DATA_DIR` may
    // name a drive root or a library's parent, and allowing everything
    // beneath it made every photo folder there writable. A photo that itself
    // lives in one of these areas (a `match` on a pixel master) writes beside
    // it because that write lands in the same area.
    for area in crate::store::own_areas_in(store) {
        if let Ok(area) = absolute(&area)
            && out_abs.starts_with(resolve_existing(&normalize(&area)))
        {
            return Ok(());
        }
    }
    // OUR own output areas. Two entries, not one: the configured delivery
    // root (M8) and the historical `./out`, which keeps holding the pixel
    // masters an older develop already links to — a user who repoints the
    // root must not find that `match`-ing on one of those old exports is
    // suddenly refused. Both are `Destination`-trusted or literal, so no
    // planted file can widen this allowance (see `config::SETTINGS`).
    //
    // A RELATIVE area inside the photo's own folder is not "ours" by that
    // name (2026-10-26): `./out` resolves against the current directory, so a
    // CLI run from inside the library made it a subfolder of the library.
    // Such a folder is still writable as a destination, but it does not skip
    // the overwrite check below — a delivery folder that happens to BE the
    // photo's folder must not turn "replace a photo" into "write to ./out".
    let raw_dir = raw_abs.parent();
    for own in [delivery.to_path_buf(), PathBuf::from(crate::config::DEFAULT_DELIVERY_ROOT)]
    {
        if let Ok(own_out) = absolute(&own) {
            let own_out = resolve_existing(&normalize(&own_out));
            let relative_in_photo = own.is_relative()
                && raw_dir.is_some_and(|d| own_out.starts_with(d))
                && !raw_abs.starts_with(&own_out);
            if out_abs.starts_with(&own_out) && !relative_in_photo {
                return Ok(());
            }
        }
    }
    // NEVER clobber an existing PHOTO outside our own output areas. A SIBLING
    // folder of the same library (<library>/TripB while the RAW sits in
    // <library>/TripA) is fully writable, so `-o` could destroy a library
    // photo. We cannot know where the library ROOT is — there is no such
    // setting — but we do not need to: outside ./out and the store's areas
    // (both allowed above, where overwriting OUR OWN deliverables is the
    // point), refusing to replace an existing image file is stricter than any
    // root guess and needs no configuration. Writing a NEW file elsewhere
    // stays allowed: the user asked for that path, and nothing of theirs is
    // lost.
    if is_source(&out_abs) && out_abs.exists() {
        return Err(OverwriteRefused { path: out_abs, delivery: delivery.to_path_buf() }.into());
    }
    Ok(())
}

/// [`guard_readonly`]'s refusal to replace an existing photo outside our own
/// output areas — typed, so a caller can tell it from the other refusals by
/// `downcast_ref` rather than by its English (the GUI's Normal mode words it
/// plainly). With the default delivery folder the message is the one this
/// refusal has always printed; a configured one is named instead of `./out`.
#[derive(Debug)]
pub struct OverwriteRefused {
    pub path: PathBuf,
    /// The delivery folder in force (`config::delivery_root`), as configured.
    pub delivery: PathBuf,
}

impl std::fmt::Display for OverwriteRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if is_default_delivery(&self.delivery) {
            write!(
                f,
                "refusing to overwrite the existing photo {} — outputs must not replace photos outside ./out. Pick a new name, or write to ./out (the default).",
                self.path.display()
            )
        } else {
            let root = shown_folder(&self.delivery);
            write!(
                f,
                "refusing to overwrite the existing photo {} — outputs must not replace photos outside {root} (the delivery folder). Pick a new name, or write to {root}.",
                self.path.display()
            )
        }
    }
}

impl std::error::Error for OverwriteRefused {}

fn is_default_delivery(p: &Path) -> bool {
    p == Path::new(crate::config::DEFAULT_DELIVERY_ROOT)
}

/// A folder as the hints spell it: a relative one with the `./` that says it
/// resolves against the current directory, an absolute one as it stands.
fn shown_folder(p: &Path) -> String {
    let starts_dotted = matches!(
        p.components().next(),
        Some(std::path::Component::CurDir | std::path::Component::ParentDir)
    );
    if p.is_relative() && !starts_dotted {
        format!("./{}", p.display())
    } else {
        p.display().to_string()
    }
}



/// `<delivery root>/<stem>.<kind>.<ext>` — outputs never go beside the source
/// RAW. The root is [`crate::config::delivery_root`] (R24-5 M8), which
/// defaults to the historical `./out`, so an unset setting produces exactly
/// the path this function always produced. THE funnel: every deliverable and
/// every pixel master is named through here or through [`BatchNames`], which
/// itself calls it — so the setting has one consumption point, not five.
pub fn default_out(raw: &Path, kind: &str, ext: &str) -> PathBuf {
    crate::config::delivery_root().join(format!("{}.{kind}.{ext}", stem(raw)))
}

/// Batch-scope deliverable names. `default_out` keys deliverables by STEM
/// alone — deliberate (a re-export replaces its previous deliverable, and
/// user scripts key on the bare name) — but one batch can hold two
/// DSC00001.ARW from different folders (camera counter rollover), and both
/// would then write the SAME file: the second silently destroyed the first.
/// Four review units reported it independently. Within one batch the first
/// claimant keeps the bare name; later same-stem claimants get the import
/// convention `<stem> (2).<kind>.<ext>`, and every deviation is recorded in
/// `renamed` so the batch summary can say WHICH photo took WHICH name.
/// Claims fold case: the filesystem would collide `DSC` with `dsc` even
/// though the strings differ.
#[derive(Default)]
pub struct BatchNames {
    taken: std::collections::HashSet<String>,
    /// Disclosure lines, `"<final deliverable> ← <source photo>"`.
    pub renamed: Vec<String>,
    /// Delivery ROOT for every claim. `None` = whatever [`default_out`] gives
    /// — the CLI's shape, i.e. the configured delivery root (M8), `./out`
    /// unless the user moved it — and what `Default::default()` still
    /// produces byte for byte. The GUI passes the user's export destination
    /// setting here so a batch render lands where its single exports do
    /// (R22-7); the naming and dedup rules are untouched by it.
    root: Option<PathBuf>,
}

impl BatchNames {
    /// A batch that delivers into `dir` instead of the delivery root.
    pub fn rooted(dir: PathBuf) -> Self {
        Self { root: Some(dir), ..Default::default() }
    }

    /// The deliverable path for `raw`, unique within this batch.
    pub fn claim(&mut self, raw: &Path, kind: &str, ext: &str) -> PathBuf {
        let mut out = self.rooted_name(raw, kind, ext, 1);
        let mut n = 1u32;
        while !self.taken.insert(out.to_string_lossy().to_lowercase()) {
            n += 1;
            out = self.rooted_name(raw, kind, ext, n);
        }
        if n > 1 {
            self.renamed.push(format!("{} ← {}", out.display(), raw.display()));
        }
        out
    }

    /// `<root>/<stem>[ (n)].<kind>.<ext>`. `n == 1` is the bare name, and it
    /// comes from [`default_out`] so this type never re-spells the deliverable
    /// convention; only the parent directory is swapped when a root was set.
    fn rooted_name(&self, raw: &Path, kind: &str, ext: &str, n: u32) -> PathBuf {
        let base = if n == 1 {
            default_out(raw, kind, ext)
        } else {
            crate::config::delivery_root()
                .join(format!("{} ({n}).{kind}.{ext}", stem(raw)))
        };
        match (&self.root, base.file_name()) {
            (Some(root), Some(name)) => root.join(name),
            _ => base,
        }
    }
}

/// Write a pixel master: stage under a temporary name, then publish.
///
/// A failed encode must not leave a partial file at the name `pixels.json` and
/// the GUI will link, and a repeat write must not destroy the previous master
/// before the new one is known good. The publish is `store::durable_replace`
/// rather than a bare rename, because that link is durable and the bytes have
/// to be on disk before anything points at them.
///
/// JPEG cannot carry 16 bits or an alpha channel, so a JPEG target is
/// flattened — the caller chose that format, and the alternative is failing a
/// write the caller asked for.
///
/// Tagged with the engine's sRGB profile ([`crate::render::write_working_space`]):
/// a master holds this pipeline's working space by construction, and written
/// untagged a 16-bit one re-read as "16-bit but carries no ICC profile", the
/// warning meant for an editor's ProPhoto export (a real three-frame stack,
/// 2026-09-26).
///
/// It lives HERE, beside [`default_out`] and [`guard_readonly`], because every
/// pixel master goes out the same door: `retouch`'s heal and clone, and the
/// stacked master from [`crate::stack`]. A second copy of this would be a
/// second place for the staging rule to be got wrong.
///
/// `baked` is the lens geometry in the pixels (v1.17.0,
/// [`crate::render::BakedGeometry`]): stamped on the STAGED file, so the
/// master is never published without the frame it is in.
pub fn save_master(
    out: &Path,
    img: image::DynamicImage,
    baked: Option<&crate::render::BakedGeometry>,
) -> Result<()> {
    ensure_parent(out)?;
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    let fmt = image::ImageFormat::from_extension(&ext).unwrap_or(image::ImageFormat::Png);
    let img = if fmt == image::ImageFormat::Jpeg {
        image::DynamicImage::ImageRgb8(img.into_rgb8())
    } else {
        img
    };
    let staged = out.with_extension(format!(
        "{ext}.tmp.{}.{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    if let Err(e) = crate::render::write_working_space(&staged, &img, fmt) {
        let _ = std::fs::remove_file(&staged);
        return Err(e).with_context(|| format!("write {}", staged.display()));
    }
    if let Some(g) = baked
        && let Err(e) = crate::render::stamp_baked_geometry(&staged, g)
    {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }
    if let Err(e) = crate::store::durable_replace(&staged, out) {
        let _ = std::fs::remove_file(&staged);
        return Err(e).with_context(|| format!("publish {}", out.display()));
    }
    Ok(())
}

pub fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create output dir {}", parent.display()))?;
        }
    Ok(())
}

/// The output-path preflight every PAID command runs BEFORE its first API
/// call (L09#1): the read-only-library guard, a directory-target refusal,
/// and the parent directory. An `-o` mistake used to surface only AFTER
/// the analysis / image call had been billed — `autoshade analyze x.arw -o
/// <existing dir>` paid for propose+verify, then write_recipe bailed with
/// nothing saved. Trade-off, stated on purpose: creating the parent
/// up-front can leave an empty directory behind if the paid call then
/// fails — that beats burning the call (a REFUSAL leaving an empty dir is
/// the case main.rs's apply path guards against; here money was spent).
/// For the GUI/web surfaces this is a provable no-op: their `out` comes
/// from `unique_out`, which already ensure_parents and create_new-claims
/// the name.
pub fn preflight_out(out: &Path, src: &Path) -> Result<()> {
    guard_readonly(out, src)?;
    if out.is_dir() {
        anyhow::bail!(
            "output target {} is a directory — pass a file path (e.g. {}/result.png)",
            out.display(),
            out.display()
        );
    }
    ensure_parent(out)?;
    // An EXISTING destination must itself be replaceable (review R12-06):
    // the sibling probe below proves the DIRECTORY writes, not that a
    // read-only file already sitting at the name can be replaced. The GUI's
    // unique_out claims are its own 0-byte files, so this stays a no-op
    // there.
    if out.is_file()
        && let Err(e) = std::fs::OpenOptions::new().write(true).open(out)
    {
        anyhow::bail!(
            "output file {} exists and is not writable ({e}) — checked before the paid call",
            out.display()
        );
    }
    // An EXISTING parent never proved it was writable (L10-7): an ACL-denied
    // export dir used to surface only after the paid call. Probe with a
    // uniquely-named sibling (pid + process-wide seq — the sibling_tmp rule),
    // removed immediately; the refusal names the directory.
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        let probe = parent.join(format!(
            ".autoshade-write-probe.{}.{}",
            std::process::id(),
            crate::store::next_tmp_seq()
        ));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
            Ok(f) => {
                drop(f);
                let _ = std::fs::remove_file(&probe);
            }
            Err(e) => {
                anyhow::bail!(
                    "output directory {} is not writable ({e}) — checked before the paid call",
                    parent.display()
                );
            }
        }
    }
    Ok(())
}

pub fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or("out")
}

/// The `"<stem>: "` the DEFAULT diagnostics sink puts in front of a
/// photograph's message ([`crate::diag::Subject::stamp`]) — a FORMATTING
/// helper, and the only thing left of what used to be the whole mechanism.
///
/// **Why the attribution exists at all.** `batch` defaults to `--jobs 3` and
/// the pool's ordering guarantee covers STDOUT only: workers write into their
/// own block and the sequencer releases blocks in index order, so the
/// transcript is byte-identical to a serial one. STDERR had no such thing —
/// `verbose` gates the progress chatter, not the warnings, so every ungated
/// `eprintln!` in the develop chain landed on a shared stream in COMPLETION
/// order and named no photograph at all. Three photos in flight, three
/// warnings, and nothing said which was which. `jobs`' module doc has always
/// disclosed the block channel; this is the identity the reordering costs,
/// bought back.
///
/// **What changed at R29-1.** R28 Batch-5 5c bought that identity back as a
/// PREFIX and registered the gap a prefix leaves: the deepest root the R27
/// adjudication names (F6) is that the pipeline had no caller-supplied
/// diagnostics channel at all, so the disclosure lines could not be routed,
/// suppressed or ORDERED by anyone who was not the process. [`crate::diag`] is
/// that channel, and it is no longer open: the develop chain's disclosures
/// travel as `diag::Line`s carrying their subject as DATA, and `batch` now
/// renders its workers' lines into the photo's own transcript block, so their
/// order is the sequencer's (index) rather than the scheduler's (completion).
/// Nothing but the default sink calls this function.
pub fn attribution(photo: &Path) -> String {
    format!("{}: ", stem(photo))
}

/// Whether a directory entry is a directory, WITHOUT an extra stat per file:
/// `DirEntry::file_type()` comes free with the directory listing (measurable on
/// large libraries / network shares), and only symlinks fall back to the
/// following `Path::is_dir` so junction/symlink traversal behaves as before.
fn entry_is_dir(entry: &std::fs::DirEntry) -> std::io::Result<bool> {
    let ft = entry.file_type()?;
    Ok(if ft.is_symlink() { entry.path().is_dir() } else { ft.is_dir() })
}

/// Recursively collect every camera RAW under `dir`, sorted.
///
/// "A RAW" has exactly ONE definition app-wide — [`decode::is_raw`] (arw, dng,
/// raw, raf, nef, cr2, cr3, orf, rw2), the same predicate [`find_sources`] and
/// the render/decode path use. This scanner used to accept `.arw` alone, which
/// left `batch` and `eval` blind to libraries the GUI and web
/// open fine.
pub fn find_raws(dir: &Path) -> Result<Vec<PathBuf>> {
    walk_photos(dir, decode::is_raw)
}

/// The shared scanner behind [`find_raws`] / [`find_sources`].
///
/// Cycle-proof by IDENTITY, not just depth: `entry_is_dir` deliberately
/// follows directory symlinks/junctions, and a link back into an ancestor
/// re-enters the same directory under a NEW spelling — the old depth cap
/// turned that into every RAW appearing up to 64× (batch then analyzed AND
/// BILLED each occurrence). Every directory is entered once per canonical
/// identity, and found files are deduped by canonical identity too (a file
/// symlink is the same photo). Resilient per entry: one unreadable
/// SUBDIRECTORY (AV lock, permissions) warns and is skipped instead of
/// aborting the whole scan — only an unreadable ROOT is a real failure. The
/// depth cap stays as a backstop for canonicalize-failing paths, and it is
/// checked before a directory is recorded as visited, so a directory first
/// met past the cap is still scanned when a shallower spelling reaches it.
fn walk_photos(root: &Path, pred: fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    walk_photos_counted(root, pred).map(|(v, _)| v)
}

/// [`walk_photos`], plus how many entries the scan could not read and skipped.
/// Each skip already warns on stderr AS IT HAPPENS; the count exists so a
/// windowed caller can say so too (L08 — the GUI has no console).
fn walk_photos_counted(
    root: &Path,
    pred: fn(&Path) -> bool,
) -> Result<(Vec<PathBuf>, usize)> {
    fn walk(
        dir: &Path,
        pred: fn(&Path) -> bool,
        out: &mut Vec<PathBuf>,
        visited: &mut std::collections::HashSet<PathBuf>,
        depth: u32,
        is_root: bool,
        skipped: &mut usize,
    ) -> std::io::Result<()> {
        // The backstop is checked BEFORE the identity is recorded: a
        // directory first met past the cap is not read here, and recording
        // it would make a shallower spelling of the same directory skip it as
        // "already scanned" — every RAW under it then went unfound.
        if depth > 64 {
            return Ok(());
        }
        if let Ok(c) = std::fs::canonicalize(dir)
            && !visited.insert(c)
        {
            return Ok(()); // already scanned through another spelling
        }
        let rd = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(e) if !is_root => {
                crate::diag::run()
                    .warn(format!("skipping unreadable folder {} ({e})", dir.display()));
                *skipped += 1;
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        for entry in rd {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    crate::diag::run().warn(format!(
                        "skipping unreadable entry under {} ({e})",
                        dir.display()
                    ));
                    *skipped += 1;
                    continue;
                }
            };
            let p = entry.path();
            match entry_is_dir(&entry) {
                Ok(true) => walk(&p, pred, out, visited, depth + 1, false, skipped)?,
                Ok(false) => {
                    if pred(&p) {
                        out.push(p);
                    }
                }
                Err(e) => {
                    crate::diag::run()
                        .warn(format!("skipping unreadable entry {} ({e})", p.display()));
                    *skipped += 1;
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    let mut visited = std::collections::HashSet::new();
    let mut skipped = 0usize;
    walk(root, pred, &mut out, &mut visited, 0, true, &mut skipped)
        .with_context(|| format!("scan {}", root.display()))?;
    out.sort();
    // Canonical-identity dedupe (first occurrence in sorted order wins): two
    // spellings of one file — a file symlink beside its target, or paths via
    // different directory links — are ONE photo, and each duplicate used to
    // bill its own analysis in batch.
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())));
    Ok((out, skipped))
}

/// Every ALREADY-BAKED raster extension the app opens — the counterpart to
/// [`decode::RAW_EXTS`], and the second half of "a photo". One list, because
/// there used to be four hand-typed copies of it (here, `serve::is_baked_ext`,
/// the GUI's `PHOTO_EXTS`, the web `accept` attribute) with nothing pinning
/// them together; the web copy had already drifted, silently refusing `.orf`,
/// `.rw2` and `.raw` uploads that every other surface accepted.
///
/// Bounded by what the `image` crate is compiled to DECODE (`Cargo.toml`'s
/// feature list) — adding an entry here without the matching codec feature
/// would let a file through the dialog and then fail at decode.
pub const BAKED_EXTS: [&str; 8] = ["png", "tif", "tiff", "jpg", "jpeg", "webp", "bmp", "gif"];

/// Does this path name a photo — a camera RAW or an already-baked raster?
/// Shared by the gallery scan and by [`guard_readonly`]'s never-clobber rule.
pub fn is_source(p: &Path) -> bool {
    crate::decode::is_raw(p) || is_baked(p)
}

/// The baked half of [`is_source`], on its own — what the web server's
/// add-path and upload gates ask, and what the GUI's file dialog lists
/// alongside the RAW set.
pub fn is_baked(p: &Path) -> bool {
    p.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| BAKED_EXTS.iter().any(|b| x.eq_ignore_ascii_case(b)))
}

/// Every extension the app opens, RAW and baked, for the surfaces that need a
/// LIST rather than a predicate (a file dialog's filter, an `<input accept>`).
/// Derived, never typed.
pub fn photo_exts() -> Vec<&'static str> {
    crate::decode::RAW_EXTS.iter().chain(BAKED_EXTS.iter()).copied().collect()
}

/// Like [`find_raws`] but also includes already-baked images (PNG/TIFF/JPEG), so
/// the web UI can browse and edit LR/PS-denoised exports alongside RAWs. Sorted.
pub fn find_sources(dir: &Path) -> Result<Vec<PathBuf>> {
    walk_photos(dir, is_source)
}

/// [`find_sources`], plus the number of unreadable entries the scan skipped —
/// the GUI folder scan consumes the count (stderr already names each one).
pub fn find_sources_counted(dir: &Path) -> Result<(Vec<PathBuf>, usize)> {
    walk_photos_counted(dir, is_source)
}



/// What the harness can carry: a FRESH analysis. A refine base, a direction,
/// a saved pixel source and `--deep` still go to the in-process chain.
pub(crate) fn fresh_analysis(
    pixels: Option<&Path>,
    guidance: Option<&str>,
    base: Option<&EditRecipe>,
    think: bool,
) -> bool {
    pixels.is_none() && guidance.is_none() && base.is_none() && !think
}
