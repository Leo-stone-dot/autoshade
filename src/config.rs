//! Runtime configuration for the AI providers.
//!
//! Resolution order (later wins): built-in defaults → environment (a gitignored
//! `.env` via `dotenvy`) → the UI-written local file `autoshade.local.json`
//! (also gitignored). Secrets (API keys) come only from `.env` or that local
//! file, never from a committed source and never logged.
//!
//! Two AI roles (see `docs/ARCHITECTURE.md` §3), each independently configurable:
//!   * **analysis** (the verifier) — provider `oauth` (the `claude` CLI, no key)
//!     or `api` (an OpenAI-compatible chat endpoint).
//!   * **image** (the vision proposer) — `api` only: the `claude` CLI has no
//!     image input in print mode, so this role needs an OpenAI-compatible vision
//!     endpoint (point `image_base_url` anywhere that speaks the API).
//!
//! Style is handled by similarity retrieval (`src/style.rs`), not a global
//! calibration baked in here.

use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The UI-written / hand-edited local config. Every field is optional; a present
/// value overrides the environment. Lives in [`local_settings_path`] (gitignored).
///
/// `PartialEq` is load-bearing: [`LocalSettings::names_beyond`] asks whether
/// restricting a file to what its source may supply CHANGED it, so the
/// "warn about what was ignored" check can never drift from the stripping
/// itself (they used to be two hand-maintained field lists).
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocalSettings {
    pub analysis_provider: Option<String>,
    pub analysis_model: Option<String>,
    pub analysis_api_key: Option<String>,
    /// The endpoint `analysis_api_key` was saved FOR (see `image_api_key_base`).
    pub analysis_api_key_base: Option<String>,
    pub analysis_base_url: Option<String>,
    /// Reasoning effort for the analysis role. Absent ⇒ the environment may
    /// still supply a tier; explicitly `""` ⇒ "send no effort parameter",
    /// silencing the environment too (see [`explicit_or_env`]).
    pub analysis_effort: Option<String>,
    pub image_api_key: Option<String>,
    /// The endpoint `image_api_key` was saved FOR — stamped by both Settings
    /// writers when a key is typed, consulted by [`Config::load`]: a stored
    /// credential authenticates ONE endpoint, and a later provider flip or
    /// base-URL edit must not silently re-route it (the GUI's OAuth flip
    /// swaps the base to a local bridge while the cloud key stayed armed).
    /// Absent ⇒ a pre-v0.23.2 save; the key is allowed anywhere until the
    /// next save records a home. Not a secret and not an endpoint CHOICE —
    /// its only power is to WITHHOLD a key, so it needs no [`SETTINGS`] row.
    pub image_api_key_base: Option<String>,
    pub image_model: Option<String>,
    pub image_base_url: Option<String>,
    pub image_gen_model: Option<String>,
    /// Reasoning effort for the image (vision proposer) role. Same
    /// absent-vs-explicitly-blank semantics as `analysis_effort`.
    pub image_effort: Option<String>,
    /// Which request shape the image role sends: `"auto"` (the Responses API,
    /// then Chat Completions when the endpoint has no `/responses`),
    /// `"responses"` or `"chat"`. Absent ⇒ `"auto"`.
    pub image_api_family: Option<String>,
    /// The image-EDIT call as a JSON template (`the removed custom endpoint`), for a
    /// provider whose edit endpoint is not OpenAI's `/images/edits` — Venice's
    /// `/image/multi-edit`. Absent ⇒ the environment may supply one; an
    /// explicit `""` ⇒ OpenAI's call, silencing the environment too.
    pub image_gen_endpoint: Option<String>,
    /// `"oauth"` (ChatGPT-subscription via a local Codex bridge, e.g. CLIProxyAPI)
    /// or `"api"` (a real OpenAI-compatible key). Purely a UI/preset selector —
    /// both resolve to the same OpenAI-compatible HTTP path, differing only in
    /// `image_base_url`/`image_api_key`. Absent ⇒ `"api"` (prior behaviour).
    pub image_provider: Option<String>,
    /// The DELIVERY ROOT (R24-5 M8): the folder every rendered deliverable and
    /// every pixel master lands in. Absent / blank ⇒ [`DEFAULT_DELIVERY_ROOT`],
    /// i.e. the historical cwd-relative `./out` — so an upgrade changes
    /// nothing. See [`delivery_root`] for what it does and does NOT cover.
    pub out_dir: Option<String>,
    /// The Python interpreter the sidecars are launched with. Absent / blank ⇒
    /// the platform default (see [`default_python_bin`]).
    ///
    /// `Trust::Destination` like every other program name, so a `.env` and an
    /// ambient working-directory file still cannot supply it — the boundary
    /// does not move. What changes is that the CENTRAL settings file can, and
    /// that is the whole point: an app bundle launched from Finder inherits no
    /// shell environment at all, so `AUTOSHADE_PYTHON` was unreachable for
    /// exactly the users who need it (user ruling 2026-08-29, Q3).
    pub python_bin: Option<String>,
    /// ComfyUI settings live in this SAME per-user file under `comfy`.
    pub comfy: Option<serde_json::Value>,
}

/// Path to the local settings file — the per-user store root, so the SAME
/// settings load no matter which directory the app was launched from (this
/// used to be a cwd-relative `autoshade.local.json`).
pub fn local_settings_path() -> PathBuf {
    crate::store::settings_path()
}

/// The settings file's name, and the one it had up to v1.1.0.
///
/// The old spelling is no longer a name this app READS on every load: it is
/// renamed onto the current one, once, by [`adopt_pre_rename_settings_file`].
pub(crate) const SETTINGS_FILE: &str = "autoshade.local.json";
pub(crate) const LEGACY_SETTINGS_FILE: &str = "autoshop.local.json";

/// Where a [`LocalSettings`] came from. This is a TRUST label, not a
/// breadcrumb: the sources do not deserve the same authority.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SettingsOrigin {
    /// [`local_settings_path`] under a PER-USER store root. The user's own.
    Central,
    /// [`local_settings_path`] under the shared `<temp>/autoshade` fallback
    /// ([`crate::store::RootTrust::SharedFallback`]) — the right PATH, but a
    /// directory every account on the machine can write, so whoever created
    /// the file first is not necessarily this user. Treated as ambient.
    SharedRoot,
    /// A cwd-relative `autoshade.local.json`. AMBIENT: whatever directory the
    /// app happens to be launched from supplies it.
    WorkingDir,
    /// No file was read.
    None,
}

impl SettingsOrigin {
    /// What this origin is allowed to decide. Both ambient origins map to the
    /// same capability — a file nobody can prove the user wrote.
    pub(crate) fn source(self) -> Source {
        match self {
            SettingsOrigin::Central | SettingsOrigin::None => Source::Trusted,
            SettingsOrigin::SharedRoot | SettingsOrigin::WorkingDir => Source::WorkingDirFile,
        }
    }
}

impl LocalSettings {
    /// Drop every field `src` is not allowed to choose.
    ///
    /// An API key and the endpoint it is sent to are one decision. The web
    /// server already refuses to let a page make it — `serve.rs`'s
    /// cross-origin guard exists because "`/api/settings` would repoint the AI
    /// base URL at the attacker and the next Analyze would hand over the
    /// user's API key". The FILESYSTEM route to that same outcome was open:
    /// resolution is per FIELD, not per file, so a planted
    /// `autoshade.local.json` carrying only `image_base_url` redirected the
    /// endpoint while the real key still came from `.env` / the environment.
    /// Extracting a shared archive and running AutoShade inside it was enough.
    ///
    /// WHICH fields survive is no longer restated here: it is read off
    /// [`SETTINGS`], the one table that also decides what a `.env` may set and
    /// what reaches a child process. Three hand-kept lists used to encode this
    /// single policy in three vocabularies and drifted apart.
    fn restricted_to(mut self, src: Source) -> Self {
        for s in SETTINGS {
            if let Some(field) = s.field
                && !src.may_supply(s.trust)
            {
                *field(&mut self) = None;
            }
        }
        self
    }

    /// Does this file name anything `src` may not supply? Answered by asking
    /// whether the restriction above CHANGES it, so the warning and the
    /// stripping can never disagree.
    fn names_beyond(&self, src: Source) -> bool {
        self.clone().restricted_to(src) != *self
    }

    /// Keep each stored key's HOME (`*_api_key_base`) coherent after a
    /// settings mutation. Runs inside [`update_local_settings`], so BOTH
    /// writers (the GUI form and `serve.rs`'s POST) inherit one rule:
    ///
    /// - A key the writer just typed was stamped BY the writer with the base
    ///   on screen beside it — an existing stamp is never second-guessed.
    /// - A pre-v0.23.2 key with no home inherits the current base, but only
    ///   from a save that did NOT change the base: a base-changing save is
    ///   exactly the re-route this stamp exists to catch, and blessing the
    ///   new pairing there would launder the old key to the new endpoint.
    ///   (Residual, documented: such a key stays home-less — and therefore
    ///   allowed anywhere — until a base-stable save or a re-type.)
    /// - No key ⇒ no home.
    fn reconcile_key_homes(&mut self, before: &LocalSettings) {
        fn one(
            key: &Option<String>,
            home: &mut Option<String>,
            base: &Option<String>,
            before_home: &Option<String>,
            before_base: &Option<String>,
        ) {
            if key.is_none() {
                *home = None;
            } else if home.is_none() && before_home.is_none() && base == before_base {
                *home = base.clone().filter(|s| !s.trim().is_empty());
            }
        }
        one(
            &self.image_api_key,
            &mut self.image_api_key_base,
            &self.image_base_url,
            &before.image_api_key_base,
            &before.image_base_url,
        );
        one(
            &self.analysis_api_key,
            &mut self.analysis_api_key_base,
            &self.analysis_base_url,
            &before.analysis_api_key_base,
            &before.analysis_base_url,
        );
    }
}

/// Read the local settings file, if present, and say WHERE it came from.
/// A missing file yields defaults (we never block startup on it).
///
/// A malformed file does NOT quietly become defaults any more. It used to:
/// `unwrap_or_default()` turned one stray comma in a hand-edited file into a
/// `LocalSettings` with every field `None`, silently — and because both
/// writers (`serve.rs`'s settings route and the GUI's) read-merge-write onto
/// what they were handed and skip blank secrets, the very next Settings save
/// published that emptiness over the user's real keys. The file is documented
/// as hand-editable, so a typo is the expected vector, and the loss was
/// permanent and unannounced. Now the bytes are preserved beside the original
/// and the reader moves on to the next candidate.
fn preserve_corrupt_settings(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    // Under the cross-process settings lock, NoWait: `bytes` were read some
    // time ago, and the remove inside acts on whatever sits at `path` NOW.
    // Unguarded, a settings writer that atomically replaced the corrupt file
    // in between (both writers hold this same lock for their whole cycle) had
    // its GOOD file — keys and all — deleted by this stale rescuer. A busy
    // lock skips the rescue outright: the holder is about to overwrite the
    // corrupt file anyway, and its own load preserved the bytes first.
    crate::store::with_settings_lock(crate::store::DevelopLockMode::NoWait, || {
        rescue_if_unchanged(path, bytes)
    })
}

/// The rescue body, entered only with the settings lock held: re-verify the
/// live file still holds exactly the corrupt bytes that were read, then MOVE
/// it under a unique `.corrupt` name. A rename, never copy-then-delete
/// (Codex R12 #3): the lock serializes the app's own writers, but the file
/// is documented as hand-editable — an external editor's atomic replace
/// landing inside the microsecond verify→act window used to be DELETED
/// while the rescue copy held only the stale corrupt bytes. A rename
/// preserves whatever is live: worst case the new content lands under the
/// rescue name, which the warning prints, and nothing is ever destroyed.
fn rescue_if_unchanged(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<PathBuf> {
    let live = crate::store::read_bytes_capped(path, crate::store::MAX_STORE_JSON)?;
    if live != bytes {
        return Err(std::io::Error::other(
            "the settings file changed while being rescued — nothing was removed",
        ));
    }
    static CORRUPT_SEQ: std::sync::atomic::AtomicU64 =
        std::sync::atomic::AtomicU64::new(0);
    for _ in 0..16 {
        let kept = path.with_extension(format!(
            "json.corrupt.{}-{}",
            std::process::id(),
            CORRUPT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        // Claim the name first (a crashed predecessor's recycled pid+seq
        // must not be overwritten), then rename the live file over the
        // claim — fs::rename replaces the destination on both platforms.
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(0o600);
        }
        match opts.open(&kept) {
            Ok(file) => drop(file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
        // durable_rename, not bare rename (L03) and not durable_replace:
        // the parent-dir fsync keeps "preserved at <kept>" true after a
        // power cut, while replace's source-sync would open the LIVE file
        // for write — read-only or exclusively held, that fails a move
        // that used to succeed. The bytes are already on disk as-is.
        if let Err(e) = crate::store::durable_rename(path, &kept) {
            // Release the claim ONLY when the live file demonstrably did
            // not move (the settings lock is held, so this probe races no
            // cooperating writer). Once the rename itself succeeded, `kept`
            // IS the only copy of the user's keys — a finish_parent
            // failure must not have this cleanup delete the sole survivor.
            if path.exists() {
                let _ = std::fs::remove_file(&kept);
            }
            return Err(e);
        }
        // The rename carried the live file's own mode over the 0600 claim;
        // the file holds API keys, so restore the tight mode (best-effort —
        // Windows AppData is per-user already).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o600));
        }
        return Ok(kept);
    }
    Err(std::io::Error::other(
        "could not claim a corrupt-settings rescue file",
    ))
}

/// How a settings file should be NAMED in a diagnostic: its file name plus
/// the ROLE of the folder it sits in. Never the absolute path — see the call
/// site for why (stderr leaks the account name and profile layout).
fn file_label(p: &Path, origin: SettingsOrigin) -> String {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| SETTINGS_FILE.to_string());
    let place = match origin {
        SettingsOrigin::Central => "in your AutoShade data folder",
        SettingsOrigin::SharedRoot => "in the shared temp AutoShade folder",
        SettingsOrigin::WorkingDir => "in the current working directory",
        SettingsOrigin::None => "",
    };
    if place.is_empty() { name } else { format!("{name} ({place})") }
}

/// What became of a pre-rename settings file sitting in the store root.
///
/// The shape of [`crate::store::RootAdoption`], for the settings FILE, and the
/// same doctrine: rename or leave it alone, never copy, never merge.
#[derive(Clone, PartialEq, Eq, Debug)]
enum SettingsAdoption {
    /// No pre-rename file: nothing to move.
    Nothing,
    /// Both names exist. The current one is used and the old one is left
    /// exactly where it is — choosing between two settings files is a decision
    /// about the user's API keys, not one to make silently.
    KeptBoth,
    /// The pre-rename file was renamed onto the current name, keys and all.
    Migrated,
    /// The rename failed. This session reads the path inside, so nothing is
    /// lost, and the next launch tries again.
    FellBack(PathBuf),
}

/// Rename a pre-rename settings file onto the current name, or say why not.
///
/// v1.2.0 shipped `autoshop.local.json` as a standing SECOND SPELLING and said
/// the next release would drop it; four releases later it was still being read
/// on every load, at two locations. Dropping it outright would have cost the
/// API keys of the one user it was written for: someone upgrading from v1.1 or
/// earlier, whose store FOLDER [`crate::store::adopt_pre_rename_root`] renames
/// while the file inside keeps the old name until the first save. So the file
/// migrates the way the folder does — one rename, disclosed, self-retiring —
/// and the read path carries no second spelling any more.
///
/// On explicit paths, so the battery drives all four branches with temp
/// directories; [`settings_adoption`] holds the once-per-process latch.
fn adopt_pre_rename_settings_file(current: &Path, legacy: &Path) -> SettingsAdoption {
    if !legacy.is_file() {
        return SettingsAdoption::Nothing;
    }
    if current.exists() {
        eprintln!(
            "note: a pre-rename {LEGACY_SETTINGS_FILE} sits beside the {SETTINGS_FILE} \
             this version uses. Nothing was moved or merged — {SETTINGS_FILE} is in use."
        );
        return SettingsAdoption::KeptBoth;
    }
    match std::fs::rename(legacy, current) {
        Ok(()) => {
            eprintln!(
                "note: your settings file was renamed from {LEGACY_SETTINGS_FILE} to \
                 {SETTINGS_FILE} (the app was renamed). Every key and choice came with it."
            );
            SettingsAdoption::Migrated
        }
        Err(e) => {
            eprintln!(
                "warning: could not rename {LEGACY_SETTINGS_FILE} to {SETTINGS_FILE} ({e}). \
                 Your settings are still being read from the old name, so nothing is lost; \
                 the next launch tries again."
            );
            SettingsAdoption::FellBack(legacy.to_path_buf())
        }
    }
}

/// [`adopt_pre_rename_settings_file`] once per process.
///
/// `Config::load` runs on every render path, and a per-load rename attempt
/// would mean a per-load `is_file()` plus a repeated warning line — the same
/// reasoning that put a latch on the store root's own adoption.
fn settings_adoption(root: &Path) -> SettingsAdoption {
    static DONE: std::sync::OnceLock<SettingsAdoption> = std::sync::OnceLock::new();
    DONE.get_or_init(|| {
        adopt_pre_rename_settings_file(&local_settings_path(), &root.join(LEGACY_SETTINGS_FILE))
    })
    .clone()
}

pub fn load_local_settings_from() -> (LocalSettings, SettingsOrigin) {
    // The central path is only CENTRAL when the root it sits under is
    // per-account. `<temp>/autoshade` is the last-resort root and is writable
    // by every local account, so a settings file found there gets ambient
    // authority — the same treatment as one found in the working directory.
    let (root, trust) = crate::store::store_root_with_trust();
    let central = match trust {
        crate::store::RootTrust::PerUser => SettingsOrigin::Central,
        crate::store::RootTrust::SharedFallback => SettingsOrigin::SharedRoot,
    };
    debug_assert!(local_settings_path().starts_with(&root));
    // The pre-rename spelling is MIGRATED, not read: one rename per process,
    // and only its `FellBack` arm leaves a second central path for this
    // session. The cwd-relative pre-rename name is gone outright — an ambient
    // file nobody can prove the user wrote never deserved a second spelling.
    // Location precedence is unchanged: central still beats the working
    // directory.
    let fell_back = match settings_adoption(&root) {
        SettingsAdoption::FellBack(p) => Some(p),
        SettingsAdoption::Nothing | SettingsAdoption::KeptBoth | SettingsAdoption::Migrated => None,
    };
    let places = [
        Some((local_settings_path(), central)),
        fell_back.map(|p| (p, central)),
        Some((PathBuf::from(SETTINGS_FILE), SettingsOrigin::WorkingDir)),
    ];
    for (p, origin) in places.into_iter().flatten() {
        // These warnings go to stderr — into logs, screenshots and pasted bug
        // reports — so they name the FILE and the FOLDER ROLE, never the full
        // path: `%LOCALAPPDATA%\autoshade\…` spells out the account name and
        // the profile layout. Anyone who needs the real location has it on
        // screen: the Settings panel prints the store root.
        let file = file_label(&p, origin);
        let bytes = match crate::store::read_bytes_capped(&p, crate::store::MAX_STORE_JSON) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            // Unreadable ≠ absent (over-cap, permissions): the old silent
            // skip fell through to defaults with nothing to say why the
            // user's keys stopped applying.
            Err(e) => {
                eprintln!("warning: {file} cannot be read ({e}) — ignoring it");
                continue;
            }
        };
        let (bytes, why) = match settings_from_bytes(bytes) {
            Ok(v) => return (v, origin),
            Err(malformed) => malformed,
        };
        // Keep the bytes: they hold the user's API keys, and a save is about
        // to overwrite this path. Best-effort and once — a second launch must
        // not clobber the first rescue. BOTH malformed shapes land here: a
        // file that is not UTF-8 (Notepad's "Unicode" save is UTF-16) used to
        // fail inside the text read and take the skip above instead, and the
        // next save overwrote the keys it held.
        let kept = preserve_corrupt_settings(&p, &bytes).ok();
        eprintln!(
            "warning: {file} {why} — ignoring it{}",
            match kept.as_deref().and_then(std::path::Path::file_name) {
                Some(n) => format!(
                    "; your settings were preserved beside it as {}",
                    n.to_string_lossy()
                ),
                None => String::new(),
            }
        );
    }
    (LocalSettings::default(), SettingsOrigin::None)
}

/// The settings a candidate file's bytes hold — or, for EITHER malformed
/// shape, text that is not UTF-8 and text that is not this app's JSON, the
/// same bytes handed back with the reason, so the caller can preserve them
/// beside the file before a save overwrites it. One door for both shapes on
/// purpose: the UTF-8 gate used to sit inside the read, where its failure
/// looked like an unreadable file, which is skipped rather than rescued.
fn settings_from_bytes(bytes: Vec<u8>) -> Result<LocalSettings, (Vec<u8>, String)> {
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => return Err((e.into_bytes(), "is not readable UTF-8 text".to_string())),
    };
    match serde_json::from_str::<LocalSettings>(&text) {
        Ok(v) => Ok(v),
        Err(e) => Err((text.into_bytes(), format!("is not valid JSON ({e})"))),
    }
}

/// The settings a caller should MERGE INTO when saving (see
/// [`load_local_settings_from`] for why the origin matters elsewhere).
///
/// An ambient file's key and endpoint are stripped HERE too, not only on the
/// read path in [`Config::load`]. Both settings writers (`serve`'s route and
/// the GUI's panel) copy a field only when the incoming request names one, and
/// then save to the CENTRAL path — so a merge onto the raw working-directory
/// values promoted a planted `image_base_url` into the trusted file, where the
/// read-path guard no longer applies (`SettingsOrigin::Central`) and the very
/// next Analyze posts the real key to it. One settings save would have undone
/// the whole guard.
pub fn load_local_settings() -> LocalSettings {
    let (s, origin) = load_local_settings_from();
    s.restricted_to(origin.source())
}

/// Persist the local settings file (the POST /api/settings target).
/// tmp + rename: `fs::write` truncates in place, so a crash / disk-full mid-
/// write left partial JSON that `load_local_settings` silently turned into
/// complete defaults — losing every saved key and model choice.
pub fn save_local_settings(s: &LocalSettings) -> std::io::Result<PathBuf> {
    // Reentrant under `update_local_settings`' own lock; a DIRECT call is a
    // lone publish and takes the lock here — no caller can slip an
    // unserialized rename between another process's load and save.
    crate::store::with_settings_lock(crate::store::DevelopLockMode::Wait, || {
        save_local_settings_unlocked(s)
    })
}

fn save_local_settings_unlocked(s: &LocalSettings) -> std::io::Result<PathBuf> {
    let p = local_settings_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // ONE open claims AND holds the file (create_new + 0600 in the same
    // OpenOptions): the earlier split — a claim loop, then a separate
    // create_new write — met its OWN claim with AlreadyExists and failed
    // every settings save outright. pid+seq names avoid cross-process
    // collisions; the loop skips a crashed predecessor's stale tmp from a
    // recycled PID (bounded: 16 tries, then surface the error).
    // The file carries API keys: on Unix it is CREATED 0600 — a chmod after
    // the write left a world-readable window. Windows AppData is per-user.
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let mut tmp = std::path::PathBuf::new();
    let mut file: Option<std::fs::File> = None;
    for _ in 0..16 {
        tmp = p.with_extension(format!(
            "json.tmp{}-{}",
            std::process::id(),
            TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(0o600);
        }
        match opts.open(&tmp) {
            Ok(f) => {
                file = Some(f);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    let Some(mut f) = file else {
        return Err(std::io::Error::other(
            "could not claim a settings temp file (16 stale .tmp siblings?)",
        ));
    };
    let write = {
        use std::io::Write as _;
        f.write_all(serde_json::to_string_pretty(s).unwrap_or_default().as_bytes())
    };
    drop(f); // close before rename — Windows cannot rename an open file
    if let Err(e) = write {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // DURABLE replace (L03): fsync the staged bytes and the parent dir
    // around the rename. tmp+rename alone left a post-crash window where
    // the live name pointed at bytes the disk never received — and this
    // file holds the API keys. The 0600 claim's mode rides the rename.
    if let Err(e) = crate::store::durable_replace(&tmp, &p) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(p)
}

/// ONE settings read-modify-write, closed against every other writer in every
/// process: load, hand the caller the merge, save — all under
/// [`crate::store::with_settings_lock`]. Both writers (serve's POST route and
/// the GUI's Settings panel) go through here; an unlocked (or in-process-only
/// locked) cycle let the OTHER process's save land between this one's load
/// and rename and erased it — the file carries the API keys. Wait mode: the
/// critical section is one small file read and rewrite.
pub fn update_local_settings(
    mutate: impl FnOnce(&mut LocalSettings),
) -> std::io::Result<PathBuf> {
    crate::store::with_settings_lock(crate::store::DevelopLockMode::Wait, || {
        let mut cur = load_local_settings();
        let before = cur.clone();
        mutate(&mut cur);
        // One rule for both writers: see `reconcile_key_homes`.
        cur.reconcile_key_homes(&before);
        let saved = save_local_settings(&cur);
        // The delivery root is memoised (it is read on every output-name
        // claim); this is the ONE writer of the file it is read from, so the
        // memo is dropped here rather than being given a lifetime of its own.
        // Dropped unconditionally: a save that failed PART way is exactly the
        // case where the memo must not be trusted either.
        invalidate_delivery_root();
        saved
    })
}

/// The historical delivery root, and still the default: a cwd-relative
/// `./out`. Spelled once so the "an unset setting changes nothing" promise is
/// a constant, not a repeated literal.
pub const DEFAULT_DELIVERY_ROOT: &str = "out";

/// **The delivery root** — the one folder rendered deliverables are written
/// to (R24-5 M8).
///
/// Before this it was not a setting at all: `pipeline::default_out` spelled
/// `./out` literally, so the CLI, the batch renderer, the web download route
/// and the GUI's "./out folder" destination each hardcoded the same
/// cwd-relative path, and a user who wanted deliverables somewhere else had
/// no answer but "launch the app from a different directory".
///
/// WHAT IT COVERS — everything `pipeline::default_out` names: the developed
/// deliverable (`<stem>.developed.{tif,jpg}`), the extracted style prompt
/// (`<stem>.style.txt`), the `match` report and preview. All of those are
/// "the files AutoShade produces FROM a photo", one semantic, one root.
///
/// WHAT IT DOES NOT COVER, deliberately — each of these is a DIFFERENT
/// semantic that merely used to share the same folder name:
///   * the pixel masters (`pipeline::unique_out`: denoise / stack / heal /
///     clone / fill / adjust / reimagine) — until v1.12 they were here, and
///     a user cleaning exports deleted the files cards are built on (user
///     report 2026-10-18). They live in [`crate::store::masters_dir`] now.
///   * the develop store (recipes / XMP / versions / masks) — already
///     per-user under [`crate::store::store_root`] since v0.13, and moving it
///     is [`crate::store::store_root`]'s own setting (`AUTOSHADE_DATA_DIR`).
///   * `out/imported` — where the WEB surface parks an uploaded SOURCE photo.
///     That is library INPUT, not a deliverable; filing a stranger's upload
///     under the user's delivery folder would mix the two.
///   * the pre-v0.13 legacy `./out` migration roots
///     (`store::legacy_out_roots`, `AUTOSHADE_LEGACY_OUT`) — a READ-only
///     archaeology path pinned to where old builds actually wrote, which a
///     new setting cannot retroactively change.
///
/// Memoised: it is consulted on every output-name claim and resolving it
/// reads a file.
/// [`update_local_settings`] — the one writer — drops the memo.
///
/// A MISS resolves under the WRITE lock, held across the file read (R24
/// round-end LOW-1). Releasing it first left a window: an
/// [`invalidate_delivery_root`] landing between this load and this store put
/// the pre-save root back into an empty memo, and nothing invalidates it
/// again — one settings save could be ignored for the rest of the process.
/// The lock serialises miss against invalidate; the second check inside it
/// also collapses a first-call thundering herd into one read. No deadlock
/// order to respect: [`load_local_settings`] takes no lock of its own (only
/// [`save_local_settings`] does), so the settings-file lock is never wanted
/// from inside this one.
pub fn delivery_root() -> PathBuf {
    if let Ok(g) = DELIVERY_ROOT_MEMO.read()
        && let Some(p) = g.as_ref()
    {
        return p.clone();
    }
    let resolve = || {
        choose_delivery_root(
            load_local_settings().out_dir.as_deref(),
            resolve_env("AUTOSHADE_OUT_DIR"),
        )
    };
    // A poisoned memo resolves WITHOUT memoising rather than pinning a root
    // behind a lock nobody can take again — the same "degrade, never pin"
    // rule the read arm above follows.
    let Ok(mut g) = DELIVERY_ROOT_MEMO.write() else { return resolve() };
    if let Some(p) = g.as_ref() {
        return p.clone(); // another caller won the race and already resolved
    }
    let v = resolve();
    *g = Some(v.clone());
    v
}

static DELIVERY_ROOT_MEMO: std::sync::RwLock<Option<PathBuf>> =
    std::sync::RwLock::new(None);

/// Drop the [`delivery_root`] memo. Called by the one settings writer.
pub fn invalidate_delivery_root() {
    if let Ok(mut g) = DELIVERY_ROOT_MEMO.write() {
        *g = None;
    }
}

/// The resolution itself, as a pure function of its two sources — so the
/// precedence (file over environment over default), the blank-means-default
/// rule and the zero-change guarantee are testable without touching a global.
///
/// Note `file` must already have been through [`LocalSettings::restricted_to`]
/// (i.e. come from [`load_local_settings`]): the root is `Destination`, so an
/// ambient file's value is gone before it reaches here.
pub(crate) fn choose_delivery_root(file: Option<&str>, env: Option<String>) -> PathBuf {
    let owned = file.map(str::to_string);
    explicit_or_env(&owned, env)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map_or_else(|| PathBuf::from(DEFAULT_DELIVERY_ROOT), PathBuf::from)
}

// Clone: the web server snapshots the config OUT of its RwLock before long AI
// calls — holding the read guard across a multi-minute retouch blocked every
// settings save for the whole stall window.
#[derive(Clone)]
pub struct Config {
    // --- image role: the vision proposer (OpenAI-compatible API only) ---------
    /// API key for the image (vision) role + generative edits. `None` ⇒ the
    /// proposer falls back to the heuristic baseline.
    pub openai_api_key: Option<String>,
    pub openai_model: String,
    pub openai_base_url: String,
    /// Image model for generative retouch/reimagine (V2_PLAN §5).
    pub openai_image_model: String,
    /// Output quality tier for generative edits: low | medium | high | auto.
    pub openai_image_quality: String,
    /// Pixel budget for FLEXIBLE-size generative output. gpt-image-2 accepts any
    /// WIDTHxHEIGHT (edges ×16, ratio ≤3:1, ≤8 294 400 px total — the API max);
    /// cost scales with pixels, so lower this to spend less per image. Models
    /// without flexible sizing 400 the request and we fall back to the fixed
    /// 1024/1536 enum automatically.
    pub openai_image_max_px: u32,
    /// UI/preset selector for the image role: `"oauth"` (ChatGPT-subscription via
    /// a local Codex bridge) or `"api"` (a real OpenAI-compatible key). The engine
    /// path is identical for both — this only distinguishes how the endpoint above
    /// was populated, so the Settings UI can restore the right mode.
    pub image_provider: String,
    /// Reasoning effort for the image role, or `None` to send no such
    /// parameter and let the provider pick (the pre-v0.23.2 behaviour, and
    /// the only correct request for a model that does not reason at all).
    /// Validated by [`effort`]; the wire spelling differs per endpoint family
    /// and is applied in `the removed paid transport`.
    pub image_effort: Option<String>,
    /// Which request shape the image role sends — `"auto"`, `"responses"` or
    /// `"chat"`, validated by [`api_family`] and read by
    /// `the removed endpoint-family selection`. `auto` tries the Responses API and falls
    /// back to Chat Completions when the endpoint answers 404 or 405 there,
    /// so a base URL and a key of ANY OpenAI-compatible server work for the
    /// image role without the user knowing which of the two it serves.
    pub image_api_family: String,
    /// The custom image-edit template as the user wrote it (`the removed custom endpoint`),
    /// or `None` for OpenAI's `/images/edits`. Kept as text and parsed at the
    /// call, so a broken template fails the edit with its own message instead
    /// of quietly falling back to an endpoint the provider does not serve.
    pub image_gen_endpoint: Option<String>,

    // --- analysis role: the verifier (oauth = claude CLI, or api = OpenAI) -----
    /// `"oauth"` (default; the `claude` CLI) or `"api"` (OpenAI-compatible chat).
    pub analysis_provider: String,
    /// Model for the analysis role: a `claude` alias/id for oauth (default
    /// `opus`), or a chat model id for api.
    pub analysis_model: String,
    /// Reasoning effort for the analysis role, or `None` for the provider's
    /// own default. `oauth` passes it as the `claude` CLI's `--effort`
    /// (`low|medium|high|xhigh|max`, measured from `claude --help`); `api`
    /// passes the OpenAI-compatible spelling. See [`effort`].
    pub analysis_effort: Option<String>,
    /// Path/name of the `claude` executable (oauth analysis, reuses Claude OAuth).
    pub claude_bin: String,
    /// API key + base for the `api` analysis provider (independent of the image key).
    pub analysis_api_key: Option<String>,
    pub analysis_base_url: String,

    // --- AI denoise sidecar ---------------------------------------------------
    /// Python interpreter for the AI sidecars (`python/denoise.py`,
    /// `segment.py`, `embed.py`, `correspond.py`, `describe.py`).
    pub python_bin: String,
    /// SCUNet weight set (color_real_psnr default; see python/denoise.py).
    pub denoise_model: String,
    pub denoise_script: String,
    /// RAW-domain denoise sidecar (`python/denoise_raw.py`, 2026-09-15):
    /// DRUNet on the sensor MOSAIC, before demosaic, with the noise model
    /// measured on the frame. Same trust class as the other scripts — it
    /// names a program's argv — so env-only and Destination-trusted.
    pub denoise_raw_script: String,
    /// Where every sidecar's model weights live — see [`default_weights_dir`].
    /// `AUTOSHADE_WEIGHTS_DIR`, or `weights/` beside whichever script answered.
    pub weights_dir: String,
    /// AI segmentation sidecar (`python/segment.py`) — subject/sky bitmap masks.
    pub segment_script: String,
    /// Depth sidecar (`python/depth.py`, 2026-10-13) — the AI lens blur's
    /// MoGe-2 depth map. Env-only and Destination-trusted like its siblings:
    /// it names a program's argv.
    pub depth_script: String,
    /// Face sidecar (`python/face.py`, v1.14.0) — the Portraits mode's face
    /// and eye outlines from MediaPipe's Face Landmarker. Env-only and
    /// Destination-trusted like its siblings: it names a program's argv.
    pub face_script: String,
    /// Style-embedding sidecar (`python/embed.py`) — SigLIP 2 vectors for the
    /// style index (R27 Batch-5). Same trust class as the other two: it names
    /// a program's argv, so it is env-only and Destination-trusted.
    pub embed_script: String,
    /// Cross-image correspondence sidecar (`python/correspond.py`) — DIFT
    /// fields for content-divergent reverse-fits (step 7). Same trust class:
    /// a program's argv, so env-only and Destination-trusted.
    pub correspond_script: String,
    /// Look-description sidecar (`python/describe.py`) — Qwen3-VL prose about
    /// each exemplar's grade for the style index (step 14 / S2). Same trust
    /// class again: a program's argv, so env-only and Destination-trusted.
    pub describe_script: String,

    /// How strongly to lean on the user's historical edit style, 0.0..1.0.
    pub style_strength: f32,

    /// SHOW the model the retrieved reference photo, not just its numbers —
    /// the non-GUI half of the R23-2 opt-in (B1).
    ///
    /// OFF by default, and the default is the whole point: turning it on puts
    /// one of the photographer's own finished photographs on a paid vision
    /// call as IMAGE 2. `analyze --reference-image` / `auto --reference-image`
    /// turn it on for one run; this makes it the standing answer for a user who
    /// always wants it, the way `AUTOSHADE_STYLE_STRENGTH` does for the dial.
    pub send_reference_image: bool,
}

// --- the capability table ---------------------------------------------------
//
// ONE policy, declared once. Everything that used to be a hand-kept list —
// which `.env` names are refused, which settings-file fields an ambient file
// loses, which names are withheld from a child process, and which warnings
// fire — is derived from [`SETTINGS`] below.
//
// The three lists it replaces drifted, provably: the guard's own test carried
// a copied 14-name array while the constant had grown to 17, so PYTHONPATH,
// PYTHONHOME and the weight cache went unchecked. Worse, `Config::load` read
// that array BY INDEX (`pre(11)` was AUTOSHADE_OPENAI_MODEL), so inserting or
// removing a single name silently repointed unrelated config fields at the
// wrong variable — which is exactly what narrowing the list required.

/// What a setting DECIDES. This, and only this, determines who may set it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Trust {
    /// Authenticates and bills the user for AutoShade's OWN calls. A planted
    /// one spends a stranger's money on the user's work.
    Secret,
    /// Names WHERE bytes are sent, WHICH account pays, or WHAT program runs:
    /// endpoints, executables, script paths, search paths, the store root, a
    /// child's credentials. A planted one is exfiltration or code execution,
    /// not a preference. `AUTOSHADE_CLAUDE_BIN` and `AUTOSHADE_PYTHON` reach
    /// `Command::new` verbatim (`advisor/claude.rs`, `denoise.rs`,
    /// `segment.rs`) and the script variables become that command's argv.
    Destination,
    /// Picks WHICH model / provider / tuning number. Carries no credential and
    /// no address; the worst a planted one can do is choose a model the user
    /// did not want — visible in Settings and in every rationale.
    Preference,
}

/// Where a value came from, and therefore what it may decide.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Source {
    /// The live process environment, or the settings file under a per-user
    /// store root. The authority the process was started with.
    Trusted,
    /// A `.env`. AMBIENT — dotenvy searches the cwd and every parent
    /// (`find.rs`) — yet also where this project's own key lives.
    DotEnv,
    /// A settings file nobody can prove the user wrote: cwd-relative, or under
    /// the shared `<temp>` root.
    WorkingDirFile,
}

impl Source {
    /// The whole trust policy, in one match.
    pub(crate) fn may_supply(self, t: Trust) -> bool {
        match self {
            Source::Trusted => true,
            // A `.env` keeps SECRETS on purpose: it is where this project's
            // own key lives, and reversing that would break the documented
            // contract (README). The half it loses is Destination — the half
            // that turns a planted file into exfiltration or process
            // execution. A planted key bills the planter, and every Responses
            // call sends `store: false`, so the user's photos do not persist
            // in that account (advisor/proposer.rs).
            //
            // Model and provider names are Preference and therefore ALLOWED
            // (user decision, 2026-08-11), which restores what README and
            // ARCHITECTURE §3 have always promised. Note the boundary this
            // draws: a photo pack's `.env` can now flip
            // `AUTOSHADE_ANALYSIS_PROVIDER` from `oauth` to `api` — but the
            // base URL and the key it would need stay Destination/…, so the
            // call still goes to the user's own endpoint with the user's own
            // key. That is a model choice, not endpoint redirection.
            Source::DotEnv => t != Trust::Destination,
            // The working-directory / shared-root file gets no exception: the
            // app writes the CENTRAL file, so this one's only legitimate role
            // is the pre-v0.13 migration affordance.
            Source::WorkingDirFile => t == Trust::Preference,
        }
    }
}

/// One configurable setting: the environment variable that carries it, what it
/// decides, and — when the Settings UI can write it — the [`LocalSettings`]
/// field it binds to.
pub(crate) struct Setting {
    pub(crate) env: &'static str,
    pub(crate) trust: Trust,
    field: Option<fn(&mut LocalSettings) -> &mut Option<String>>,
}

const fn env_only(env: &'static str, trust: Trust) -> Setting {
    Setting { env, trust, field: None }
}

const fn bound(
    env: &'static str,
    trust: Trust,
    field: fn(&mut LocalSettings) -> &mut Option<String>,
) -> Setting {
    Setting { env, trust, field: Some(field) }
}

/// Every setting this program resolves, with what it decides.
///
/// A name absent from this table is `Preference` by default — that is the
/// pass-through third-party knobs depend on (HF_HOME, CUDA_VISIBLE_DEVICES,
/// proxy variables) and it preserves the `.env` contract for anything not
/// listed. New AutoShade settings must be added here; the tests below fail if
/// this file names an `AUTOSHADE_*` variable the table does not classify.
pub(crate) const SETTINGS: &[Setting] = &[
    // --- image role: the vision proposer + generative edits ------------------
    bound("OPENAI_API_KEY", Trust::Secret, |s| &mut s.image_api_key),
    bound("AUTOSHADE_OPENAI_BASE_URL", Trust::Destination, |s| &mut s.image_base_url),
    bound("AUTOSHADE_OPENAI_MODEL", Trust::Preference, |s| &mut s.image_model),
    bound("AUTOSHADE_OPENAI_IMAGE_MODEL", Trust::Preference, |s| &mut s.image_gen_model),
    bound("AUTOSHADE_IMAGE_PROVIDER", Trust::Preference, |s| &mut s.image_provider),
    bound("AUTOSHADE_IMAGE_EFFORT", Trust::Preference, |s| &mut s.image_effort),
    bound("AUTOSHADE_IMAGE_API_FAMILY", Trust::Preference, |s| &mut s.image_api_family),
    // Destination: it names the path the key and the photo are posted to.
    bound("AUTOSHADE_IMAGE_GEN_ENDPOINT", Trust::Destination, |s| &mut s.image_gen_endpoint),
    env_only("AUTOSHADE_IMAGE_QUALITY", Trust::Preference),
    env_only("AUTOSHADE_IMAGE_MAX_PX", Trust::Preference),
    // --- analysis role: the verifier -----------------------------------------
    bound("AUTOSHADE_ANALYSIS_API_KEY", Trust::Secret, |s| &mut s.analysis_api_key),
    bound("AUTOSHADE_ANALYSIS_BASE_URL", Trust::Destination, |s| &mut s.analysis_base_url),
    bound("AUTOSHADE_ANALYSIS_PROVIDER", Trust::Preference, |s| &mut s.analysis_provider),
    bound("AUTOSHADE_ANALYSIS_MODEL", Trust::Preference, |s| &mut s.analysis_model),
    bound("AUTOSHADE_ANALYSIS_EFFORT", Trust::Preference, |s| &mut s.analysis_effort),
    env_only("AUTOSHADE_CLAUDE_MODEL", Trust::Preference), // legacy alias for ANALYSIS_MODEL
    env_only("AUTOSHADE_CLAUDE_BIN", Trust::Destination),  // Command::new
    // --- python sidecars ------------------------------------------------------
    // Destination, and SETTABLE — from the central per-user settings file only
    // (`Source::Trusted`), which is what `bound` means here. See the field's
    // own doc on `LocalSettings`.
    bound("AUTOSHADE_PYTHON", Trust::Destination, |s| &mut s.python_bin), // Command::new
    env_only("AUTOSHADE_DENOISE_SCRIPT", Trust::Destination), // that command's argv
    env_only("AUTOSHADE_DENOISE_RAW_SCRIPT", Trust::Destination),
    env_only("AUTOSHADE_SEGMENT_SCRIPT", Trust::Destination),
    env_only("AUTOSHADE_DEPTH_SCRIPT", Trust::Destination),
    env_only("AUTOSHADE_FACE_SCRIPT", Trust::Destination),
    env_only("AUTOSHADE_EMBED_SCRIPT", Trust::Destination),
    env_only("AUTOSHADE_CORRESPOND_SCRIPT", Trust::Destination),
    env_only("AUTOSHADE_DESCRIBE_SCRIPT", Trust::Destination),
    // A redirected weight cache is a poisoned-model path.
    env_only("AUTOSHADE_WEIGHTS_DIR", Trust::Destination),
    env_only("AUTOSHADE_DENOISE_MODEL", Trust::Preference),
    // --- store, tuning knobs ---------------------------------------------------
    // Sites the TRUSTED settings file itself, so it decides where the key is
    // read from — the root of the whole trust story.
    env_only("AUTOSHADE_DATA_DIR", Trust::Destination),
    env_only("AUTOSHADE_STYLE_STRENGTH", Trust::Preference),
    // DESTINATION, not Preference, and this is the one place in this table
    // where that call is worth stating. It carries no key and no address, so
    // the letter of `Preference` fits — but what it decides is whether one of
    // the user's own finished PHOTOGRAPHS is put on the wire at all, and the
    // threat model this table exists for is an ambient `.env` arriving inside
    // an unzipped photo pack. A planted `AUTOSHADE_ANALYSIS_PROVIDER` picks a
    // model; a planted one of these uploads the photographer's work. The live
    // environment and the trusted central settings file may set it; a photo
    // pack may not.
    env_only("AUTOSHADE_SEND_REFERENCE_IMAGE", Trust::Destination),
    env_only("AUTOSHADE_HTTP_TIMEOUT_SECS", Trust::Preference),
    env_only("AUTOSHADE_SIDECAR_TIMEOUT_SECS", Trust::Preference),
    // Names a directory to READ pre-v0.13 sidecars from during an explicitly
    // user-started import. It writes nothing and runs nothing, so it stays a
    // preference — the strictest reading would make it Destination, but that
    // would silently break the documented `.env` migration knob.
    env_only("AUTOSHADE_LEGACY_OUT", Trust::Preference),
    // The DELIVERY ROOT (M8) is the exact opposite case, and lands on the
    // other side of the same line: it names a directory every export and
    // every pixel master is WRITTEN to. A planted one does not merely choose
    // a folder — it decides where a stranger's developed photos are filed.
    // "Names WHERE bytes are sent", so Destination: neither a `.env` nor an
    // ambient `autoshade.local.json` may supply it.
    bound("AUTOSHADE_OUT_DIR", Trust::Destination, |s| &mut s.out_dir),
    // --- foreign names this process or its children obey ------------------------
    env_only("PATH", Trust::Destination),
    // Both Python sidecars inherit the environment, and a `.env`'s
    // `PYTHONPATH=.` beside a hostile `numpy.py` is code execution at import
    // time (the sidecars also pass `-E` — defence in both layers).
    env_only("PYTHONPATH", Trust::Destination),
    env_only("PYTHONHOME", Trust::Destination),
    // The `claude` child's own routing. A credential is the routing decision
    // here: with ANTHROPIC_API_KEY present the CLI bills metered API credits
    // instead of the user's subscription (measured 2026-07-17, see
    // advisor/claude.rs), and ANTHROPIC_BASE_URL repoints the verifier
    // outright. Destination, so `.env` values never reach the child.
    env_only("ANTHROPIC_API_KEY", Trust::Destination),
    env_only("ANTHROPIC_AUTH_TOKEN", Trust::Destination),
    env_only("ANTHROPIC_BASE_URL", Trust::Destination),
];

/// What `name` decides. Unlisted ⇒ [`Trust::Preference`] (see [`SETTINGS`]).
///
/// ONE row per setting, matched literally: there is no second spelling to
/// normalise since v1.2.4 retired the pre-rename `AUTOSHOP_*` door, and a
/// second row per name would BE the bug that door invited — it would look
/// classified while a `.env` set it anyway.
pub(crate) fn trust_of(name: &str) -> Trust {
    SETTINGS.iter().find(|s| s.env == name).map_or(Trust::Preference, |s| s.trust)
}

/// The `.env`, parsed ONCE per process into an OWNED map (L16#3): the
/// process environment is NEVER written. The old path ran
/// `dotenv_override` + a 17-name restore loop inside a OnceLock whose
/// safety comment claimed "the first load happens on the main thread
/// before any worker exists" — false for the GUI binary, whose `main()`
/// never calls `Config::load` and which spawns a gallery worker inside
/// the creation closure, so the `unsafe { env::set_var }` raced every
/// concurrent `getenv` (UB on unix). With the owned map the `unsafe` is
/// GONE rather than re-justified, `Config::load` is callable from any
/// thread at any time, and no `.env` name can influence the live
/// environment at all — which also closes the unlisted-name gap
/// (e.g. LOCALAPPDATA siting the store root) for free. Residual
/// unchanged: a `.env` edited mid-session applies on the next launch.
static DOTENV: std::sync::OnceLock<std::collections::HashMap<String, String>> =
    std::sync::OnceLock::new();

fn dotenv_map() -> &'static std::collections::HashMap<String, String> {
    DOTENV.get_or_init(|| {
        // dotenv_iter: the SAME cwd-upward file search as dotenv_override,
        // minus every setenv (dotenvy 0.15 lib.rs/iter.rs). take_while(ok)
        // reproduces override's error order: it applied every line BEFORE
        // the first malformed one and none after (flatten alone would skip
        // the bad line and keep applying — Codex AL F5). KNOWN, ACCEPTED
        // divergence, documented on purpose: dotenvy's $VAR interpolation
        // inside the iterator resolves against the live environment only,
        // no longer against earlier .env lines (override used to setenv as
        // it went); no .env of this project uses interpolation, and full
        // fidelity would mean vendoring the parser.
        let map: std::collections::HashMap<String, String> = dotenvy::dotenv_iter()
            .map(|it| it.take_while(Result::is_ok).flatten().collect())
            .unwrap_or_default();
        // Warn only about what is actually being ignored. Same trigger as the
        // old post-override comparison — the .env names something it may not
        // supply, with a value that differs from the process's own — but the
        // set is now derived, so narrowing the policy narrows the warning in
        // the same edit. Model and provider names no longer trip it: warning
        // about a documented, legitimate `.env` model choice was noise that
        // contradicted README and ARCHITECTURE §3.
        for (k, v) in &map {
            if !Source::DotEnv.may_supply(trust_of(k))
                && env::var(k).ok().as_deref() != Some(v.as_str())
            {
                eprintln!(
                    "warning: ignoring {k} from a .env file — a .env found in the working \
                     directory (or any parent) is not trusted to choose where your API key \
                     is sent, which account pays, or which program is run. Set it in your \
                     own environment. (Model, provider and tuning values from a .env do \
                     still apply.)"
                );
            }
        }
        map
    })
}

/// Resolve `key` with dotenv precedence but WITHOUT environment mutation:
/// the `.env` value wins for names it may supply (the dotenv_override
/// contract this project chose on 2026-08-03 — this machine carries a
/// User-scope OPENAI_API_KEY that must not out-rank the project's own
/// .env key), the live environment is the fallback, and a `Destination`
/// name never resolves from a `.env` at all. Empty/whitespace counts as
/// unset (an empty .env value therefore masks the process value, exactly as
/// override-then-filter did). Out-of-crate consumers of `.env`-honoured
/// names (the HTTP/sidecar timeout knobs, the legacy-out override) go
/// through here, or they silently stop seeing `.env` values.
pub fn env_or_dotenv(key: &str) -> Option<String> {
    resolve_env(key)
}

/// THE environment resolver — one function for every name, protected or not.
/// `Config::load` used to carry two more (`nonempty` for `.env`-honoured
/// names, and a `pre(i)` closure that indexed the protected array
/// POSITIONALLY), which is how moving a name between the two policies could
/// silently repoint an unrelated config field at the wrong variable.
///
/// Environment and `.env` alike, each under that name's own trust policy.
///
/// ONE spelling. Autoshop became AutoShade after v1.1.0, and v1.2.0 opened a
/// door where every `AUTOSHOP_*` name still answered — for ONE more version,
/// said so in its own warning, and was still open four releases later. v1.2.4
/// closes it: a pre-rename name now resolves to nothing at all, here and at the
/// other two doors, in the process environment and in a `.env` alike.
pub(crate) fn resolve_env(name: &str) -> Option<String> {
    let live = env::var(name).ok();
    if Source::DotEnv.may_supply(trust_of(name)) {
        dotenv_map().get(name).cloned().or(live)
    } else {
        live
    }
    .filter(|s| !s.trim().is_empty())
}

/// The LIVE environment only — the door for names a `.env` has never been able
/// to supply and must not start supplying now.
///
/// The rename found 34 direct `std::env::var` reads scattered across the crate:
/// fixture roots, probe outputs, the guard on deleting from the calibration
/// corpus, two sidecar precision switches. Collecting them onto [`resolve_env`]
/// would have handed a planted `.env` control of every one of them — a trust
/// change smuggled in on a rename. This door keeps their semantics EXACTLY as
/// they were: the process environment, nothing else.
pub fn live_env(name: &str) -> Option<String> {
    env::var(name).ok()
}

/// [`live_env`] for values that are PATHS: a store root or a corpus directory
/// may be any sequence the platform accepts, and `env::var` silently drops the
/// ones that are not UTF-8.
pub fn live_env_os(name: &str) -> Option<std::ffi::OsString> {
    env::var_os(name)
}

/// Foreign environment names a `.env` may push INTO a child process.
///
/// An ALLOWLIST, and it has to be one. [`Trust`] classifies AUTOSHADE's own
/// settings — a CLOSED set, where "not in the table" means "not a setting of
/// ours", so defaulting to `Preference` is safe. A child's environment is an
/// OPEN set, where "not in the table" includes every loader and interpreter
/// hook the platform defines. Reusing the same predicate for both answered the
/// second question with the first one's default: a photo pack's `.env` saying
/// `LD_PRELOAD=./evil.so` rode into both Python sidecars, and `ld.so` loads
/// that library before `-E` — which only filters `PYTHON*` — has any say.
/// (Pre-existing: the 17-name denylist this table replaced did not list it
/// either. Codex R12-BA #2 named the shared cause.)
///
/// Enumerating `LD_PRELOAD`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES`,
/// `NODE_OPTIONS`, `GCONV_PATH` … is enumerating the attacker's options. This
/// list enumerates OURS, and the membership rule is the same three-way test
/// the table uses: a name qualifies only if it selects COMPUTE BEHAVIOUR —
/// no path, no endpoint, no credential, nothing that loads code. That
/// deliberately excludes the cache knobs the old comment advertised
/// (`HF_HOME`, `TORCH_HOME`): a redirected cache is a poisoned-model path,
/// which is exactly why `AUTOSHADE_WEIGHTS_DIR` is `Destination`. It equally
/// excludes the proxy variables: a proxy decides where bytes go.
///
/// The reach this costs is small and recoverable, because a child INHERITS the
/// parent's environment (nothing here calls `env_clear`): a user's own
/// `HF_HOME` or `HTTPS_PROXY` still reaches the sidecars untouched. Only a
/// `.env`'s attempt to ADD or OVERRIDE one is refused — and refused out loud.
const CHILD_ENV_PASSTHROUGH: &[&str] = &[
    // torch / CUDA device selection and allocator tuning. Numbers and device
    // indices; no filesystem path, no host.
    "CUDA_VISIBLE_DEVICES",
    "CUDA_DEVICE_ORDER",
    "PYTORCH_CUDA_ALLOC_CONF",
    // Thread-count knobs every BLAS/OpenMP stack under torch reads.
    "OMP_NUM_THREADS",
    "MKL_NUM_THREADS",
    "NUMEXPR_NUM_THREADS",
    "TOKENIZERS_PARALLELISM",
    // Booleans that only say "do not go online" — strictly more restrictive
    // than the default, and they name nothing.
    "HF_HUB_OFFLINE",
    "TRANSFORMERS_OFFLINE",
];

/// The `.env` entries a CHILD process may receive.
///
/// Under `dotenv_override` these names sat in the process environment and
/// every child inherited them; the owned map reproduces that reach on the
/// child's own block (`Command::envs` writes the CHILD's block, never the
/// parent's) — but only for [`CHILD_ENV_PASSTHROUGH`]. Everything else the
/// `.env` names is dropped, with one warning naming what was dropped: a
/// silently-ignored knob is the failure mode that made the old pass-through
/// feel harmless.
pub fn dotenv_child_env() -> Vec<(String, String)> {
    static WARNED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    let map = dotenv_map();
    let (kept, dropped): (Vec<_>, Vec<_>) = map
        .iter()
        .partition(|(k, _)| CHILD_ENV_PASSTHROUGH.contains(&k.as_str()));
    if !dropped.is_empty() {
        WARNED.get_or_init(|| {
            // The table's own refusals already warned when they were parsed
            // (see `dotenv_map`); this names the rest, so a `.env` knob that
            // stops reaching a sidecar says so instead of just not working.
            let mut names: Vec<&str> = dropped.iter().map(|(k, _)| k.as_str()).collect();
            names.sort_unstable();
            eprintln!(
                "note: these .env names are not passed to the AI/denoise/segment child \
                 processes — a .env is ambient input and a child's environment is where a \
                 planted one becomes code execution: {}. Set them in your own environment \
                 instead; children inherit it.",
                names.join(", ")
            );
        });
    }
    kept.into_iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Two API base URLs name the same endpoint, ignoring whitespace and a
/// trailing slash. ONE definition: the GUI's pick-list invalidation and the
/// key-home check below must never disagree about what "same" means.
pub fn same_endpoint(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/') == b.trim().trim_end_matches('/')
}

/// Resolve an optional setting where an EXPLICIT empty is itself a choice.
///
/// Both Settings surfaces store a cleared effort field as `""` — "let the
/// provider decide" — but the generic `pick_opt` filters empties out before
/// consulting the environment, so with `AUTOSHADE_*_EFFORT` set the one choice
/// the field exists to express was the one choice it could not make: the
/// blank fell through, the env tier came back, and Settings showed the tier
/// the user had just cleared. Absent (the field was never saved) still defers
/// to the environment.
fn explicit_or_env(file: &Option<String>, env: Option<String>) -> Option<String> {
    match file {
        Some(s) if s.trim().is_empty() => None,
        Some(s) => Some(s.clone()),
        None => env,
    }
}

/// The FILE-stored key for a role — but only at the endpoint it was saved
/// for. A stored credential authenticates one endpoint; when the resolved
/// base has moved (a provider flip's auto-swap, a typed edit, another
/// surface's save), sending the old key there is silent credential
/// misrouting — a cloud key handed to whatever listens on a local bridge
/// port, or a bridge gate token handed to the cloud. A key with no recorded
/// home (a pre-v0.23.2 save) stays allowed until a save records one.
/// Environment keys never pass through here: the env contract has no home
/// concept and is the user's own explicit pairing.
fn file_key_for(
    key: &Option<String>,
    home: &Option<String>,
    resolved_base: &str,
    name: &str,
) -> Option<String> {
    let k = key.clone()?;
    match home {
        Some(h) if !same_endpoint(h, resolved_base) => {
            eprintln!(
                "warning: not sending {name} — it was saved for {h}, but the configured \
                 endpoint is now {resolved_base}. Enter a key for this endpoint in Settings \
                 (or re-save the old one there); requests run without a key until then."
            );
            None
        }
        _ => Some(k),
    }
}

/// Normalise a reasoning-effort value from any source into something safe on
/// a JSON body AND on a child process's argv.
///
/// The vocabulary is deliberately NOT a closed set: `claude --help` documents
/// `low|medium|high|xhigh|max` (measured 2026-08-11) while OpenAI-compatible
/// endpoints carry their own tiers and third-party bridges add more — pinning
/// a list here would make this app the bottleneck on someone else's roadmap.
/// The value is BOUNDED instead: lowercase ASCII, digits, `-`/`_`, at most 32
/// bytes, never leading `-`. An endpoint that does not know the tier answers
/// 400 and the caller negotiates it away (`the removed paid transport`). The bound
/// is the part that matters — this string reaches `Command::args`, where an
/// unbounded `.env`-supplied value would be argv injection.
pub(crate) fn effort(v: Option<String>) -> Option<String> {
    let v = v?.trim().to_ascii_lowercase();
    if v.is_empty() {
        return None;
    }
    let shaped = v.len() <= 32
        && !v.starts_with('-')
        && v.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if !shaped {
        eprintln!(
            "warning: ignoring an unusable reasoning-effort value — expected a short tier \
             name such as low / medium / high"
        );
        return None;
    }
    Some(v)
}

/// The three spellings of the image role's request shape (`Config::
/// image_api_family`), lower-cased; anything else is `auto` with a warning,
/// so a typo in the settings file degrades to the probing default rather
/// than to a request no endpoint serves.
pub(crate) fn api_family(v: String) -> String {
    let v = v.trim().to_ascii_lowercase();
    match v.as_str() {
        "auto" | "responses" | "chat" => v,
        _ => {
            eprintln!(
                "warning: ignoring an unknown image API style {v:?} — expected auto, responses \
                 or chat; using auto"
            );
            "auto".into()
        }
    }
}

/// Can `k` appear in an `Authorization` header value at all?
///
/// Visible ASCII only. RFC 9110 field values also allow SP and HTAB, but no
/// real key contains either — and a space or a newline is exactly what a bad
/// copy/paste adds. Shared with `openai_models::list_models`, which faces the
/// same ureq diagnostic on the `GET /models` probe.
pub(crate) fn key_is_header_safe(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// An API key as it will appear in an `Authorization` header, or `None`.
///
/// Trims surrounding whitespace — a key copied out of a web page carries a
/// trailing newline more often than not — and then refuses any remaining byte
/// that cannot appear in a header value.
///
/// Both halves are load-bearing, and this is the ROOT fix for a key leak:
/// ureq builds the header eagerly and its rejection diagnostic quotes the
/// WHOLE header line back, so `Authorization: Bearer <the real key>` ends up
/// inside a `Transport` error (ureq 2.12.1 `header.rs:147`). That error string
/// travels into rationale text, the Settings status line, and any log the user
/// pastes into a bug report. Refusing the malformed key HERE means the
/// diagnostic never exists; redacting at each error site is the second layer.
/// Never prints the key itself.
fn header_safe_key(v: Option<String>, name: &str) -> Option<String> {
    let t = v?.trim().to_string();
    if t.is_empty() {
        return None;
    }
    if !key_is_header_safe(&t) {
        eprintln!(
            "warning: ignoring {name} — it contains characters that cannot appear in an HTTP \
             header (a stray newline or space from a copy/paste?). Re-copy the key and set it \
             again. Requests will run without a key until then."
        );
        return None;
    }
    Some(t)
}

impl Config {
    pub fn load() -> Self {
        // Every environment name — protected or not — goes through the ONE
        // resolver, which applies the capability table per name (.env value
        // first for what a .env may supply, live environment otherwise). The
        // two closures this replaces (`nonempty` and a positional `pre(i)`
        // into the protected array) encoded the same policy twice, by hand.
        let env_val = resolve_env;
        let (local, origin) = load_local_settings_from();
        // An ambient settings file may pick models, never a key or the
        // endpoint a key is sent to. See `LocalSettings::restricted_to`.
        let src = origin.source();
        if src != Source::Trusted && local.names_beyond(src) {
            let where_ = if origin == SettingsOrigin::SharedRoot {
                "in a settings file under the SHARED temp folder (this machine has no per-user \
                 data directory, so every account can write there)"
            } else {
                "in ./autoshade.local.json — a settings file found in the WORKING DIRECTORY"
            };
            eprintln!(
                "warning: ignoring the API key and base-URL fields {where_}: it is not trusted \
                 to choose where your API key is sent. Save your settings in the app, or set \
                 AUTOSHADE_DATA_DIR to a folder only you can write."
            );
        }
        let local = local.restricted_to(src);
        // local-file value wins over env; `pick` returns the first non-empty.
        let pick = |file: &Option<String>, e: Option<String>, default: &str| -> String {
            file.as_ref()
                .filter(|s| !s.trim().is_empty())
                .cloned()
                .or(e)
                .unwrap_or_else(|| default.to_string())
        };
        let pick_opt = |file: &Option<String>, e: Option<String>| -> Option<String> {
            file.as_ref()
                .filter(|s| !s.trim().is_empty())
                .cloned()
                .or(e)
        };

        let default_base = "https://api.openai.com/v1";
        // Bases resolve FIRST: each file-stored key is gated on the endpoint
        // it was saved for (`file_key_for`), so the key pick needs the
        // resolved base in hand.
        let image_base =
            pick(&local.image_base_url, env_val("AUTOSHADE_OPENAI_BASE_URL"), default_base);
        let analysis_base =
            pick(&local.analysis_base_url, env_val("AUTOSHADE_ANALYSIS_BASE_URL"), default_base);
        // Bundled sidecar helpers resolve against the PROGRAM's own tree,
        // never the cwd (see `bundled_helper`): a photo pack carrying
        // `python/denoise.py` used to have that file executed as the user
        // the next time denoise ran from inside it. The weight cache
        // defaults to `weights/` beside whichever script answered, so dev
        // builds keep the repo cache and a packaged install stays beside
        // the exe.
        let denoise_script =
            env_val("AUTOSHADE_DENOISE_SCRIPT").unwrap_or_else(|| bundled_helper("python/denoise.py"));
        let denoise_raw_script = env_val("AUTOSHADE_DENOISE_RAW_SCRIPT")
            .unwrap_or_else(|| bundled_helper("python/denoise_raw.py"));
        // ONE directory for every sidecar, not just denoise's (see
        // `default_weights_dir`). The pre-M2 `AUTOSHADE_DENOISE_CACHE` spelling
        // retires with the `AUTOSHOP_*` door in v1.2.4: it shipped in v1.2.0
        // under the same "the next release removes the old name" warning, and
        // four releases is not one.
        let weights_dir = env_val("AUTOSHADE_WEIGHTS_DIR")
            .unwrap_or_else(|| default_weights_dir(&denoise_script));
        let segment_script =
            env_val("AUTOSHADE_SEGMENT_SCRIPT").unwrap_or_else(|| bundled_helper("python/segment.py"));
        let depth_script =
            env_val("AUTOSHADE_DEPTH_SCRIPT").unwrap_or_else(|| bundled_helper("python/depth.py"));
        let face_script =
            env_val("AUTOSHADE_FACE_SCRIPT").unwrap_or_else(|| bundled_helper("python/face.py"));
        let embed_script =
            env_val("AUTOSHADE_EMBED_SCRIPT").unwrap_or_else(|| bundled_helper("python/embed.py"));
        let correspond_script = env_val("AUTOSHADE_CORRESPOND_SCRIPT")
            .unwrap_or_else(|| bundled_helper("python/correspond.py"));
        let describe_script = env_val("AUTOSHADE_DESCRIBE_SCRIPT")
            .unwrap_or_else(|| bundled_helper("python/describe.py"));
        Config {
            openai_api_key: header_safe_key(
                pick_opt(
                    &file_key_for(
                        &local.image_api_key,
                        &local.image_api_key_base,
                        &image_base,
                        "the image API key",
                    ),
                    env_val("OPENAI_API_KEY"),
                ),
                "the image API key",
            ),
            openai_model: pick(&local.image_model, env_val("AUTOSHADE_OPENAI_MODEL"), "gpt-5.5"),
            openai_base_url: image_base,
            openai_image_model: pick(
                &local.image_gen_model,
                env_val("AUTOSHADE_OPENAI_IMAGE_MODEL"),
                "gpt-image-1.5",
            ),
            openai_image_quality: env_val("AUTOSHADE_IMAGE_QUALITY")
                .unwrap_or_else(|| "high".to_string()),
            openai_image_max_px: env_val("AUTOSHADE_IMAGE_MAX_PX")
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(8_294_400),
            image_provider: pick(
                &local.image_provider,
                env_val("AUTOSHADE_IMAGE_PROVIDER"),
                "api",
            ),
            // `explicit_or_env`, not `pick_opt`: a saved `""` is the explicit
            // choice "send no effort parameter" and must silence an env tier.
            image_effort: effort(explicit_or_env(
                &local.image_effort,
                env_val("AUTOSHADE_IMAGE_EFFORT"),
            )),
            image_api_family: api_family(pick(
                &local.image_api_family,
                env_val("AUTOSHADE_IMAGE_API_FAMILY"),
                "auto",
            )),
            // `explicit_or_env`: a saved `""` is the choice "OpenAI's call".
            image_gen_endpoint: explicit_or_env(
                &local.image_gen_endpoint,
                env_val("AUTOSHADE_IMAGE_GEN_ENDPOINT"),
            )
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),

            analysis_provider: pick(
                &local.analysis_provider,
                env_val("AUTOSHADE_ANALYSIS_PROVIDER"),
                "oauth",
            ),
            analysis_model: pick(
                &local.analysis_model,
                env_val("AUTOSHADE_ANALYSIS_MODEL")
                    .or_else(|| env_val("AUTOSHADE_CLAUDE_MODEL")),
                "opus",
            ),
            analysis_effort: effort(explicit_or_env(
                &local.analysis_effort,
                env_val("AUTOSHADE_ANALYSIS_EFFORT"),
            )),
            claude_bin: env_val("AUTOSHADE_CLAUDE_BIN").unwrap_or_else(|| "claude".to_string()),
            analysis_api_key: header_safe_key(
                pick_opt(
                    &file_key_for(
                        &local.analysis_api_key,
                        &local.analysis_api_key_base,
                        &analysis_base,
                        "the analysis API key",
                    ),
                    env_val("AUTOSHADE_ANALYSIS_API_KEY"),
                ),
                "the analysis API key",
            ),
            analysis_base_url: analysis_base,

            python_bin: pick(
                &local.python_bin,
                env_val("AUTOSHADE_PYTHON"),
                default_python_bin(),
            ),
            denoise_model: env_val("AUTOSHADE_DENOISE_MODEL")
                .unwrap_or_else(|| "color_real_psnr".to_string()),
            denoise_script,
            denoise_raw_script,
            weights_dir,
            segment_script,
            depth_script,
            face_script,
            embed_script,
            correspond_script,
            describe_script,
            style_strength: env_val("AUTOSHADE_STYLE_STRENGTH")
                .and_then(|s| s.parse::<f32>().ok())
                // "NaN" parses as a valid f32 and SURVIVES clamp (clamp keeps
                // NaN) — a non-finite blend strength would poison the style
                // math downstream, so it falls back to the default instead.
                .filter(|v| v.is_finite())
                .unwrap_or(0.3)
                .clamp(0.0, 1.0),
            // The truthiness spelling `style::EmbeddingSwitch::resolve_with`
            // already uses for the sidecar switch, so "0"/"false"/"off"/empty
            // mean the same thing on every AutoShade boolean.
            send_reference_image: env_val("AUTOSHADE_SEND_REFERENCE_IMAGE")
                .is_some_and(|v| !matches!(v.trim(), "" | "0" | "false" | "off")),
        }
    }

    /// True if the analysis role is configured to use the OpenAI-compatible API.
    pub fn analysis_is_api(&self) -> bool {
        self.analysis_provider.eq_ignore_ascii_case("api")
    }

    /// `["--cache", <the shared weight cache>]` — the argv flag every sidecar
    /// takes, resolved once from [`Config::weights_dir`] — or NOTHING.
    ///
    /// Appended at the three SPAWN points rather than hand-copied into the six
    /// per-bridge argv builders, and that is a deliberate departure from this
    /// module's usual "argv is built by the caller" shape. Where the weights
    /// live is ONE policy that applies to every sidecar identically; carrying
    /// it the other way would mean the same two arguments in six builders and
    /// a new field in forty-nine option literals, which is the drift shape
    /// this codebase keeps removing rather than adding.
    ///
    /// EMPTY when it would only restate what the sidecar already assumes. All
    /// five scripts default `--cache` to `<their own dir>/weights`, so on an
    /// unconfigured Windows install (or repo checkout) the flag would carry
    /// the sidecar's own answer back to it: no behaviour change, but every
    /// spawned argv on the platform this app already ships on would differ
    /// from the one that shipped. The flag appears exactly when somebody has
    /// actually chosen elsewhere — `AUTOSHADE_WEIGHTS_DIR`, or the `.app`
    /// bundle rule in [`default_weights_dir`] — which is also the only case
    /// where it changes anything.
    pub fn weights_args(&self) -> Vec<std::ffi::OsString> {
        if self.weights_dir == script_relative_weights(&self.denoise_script) {
            return Vec::new();
        }
        vec!["--cache".into(), self.weights_dir.as_str().into()]
    }

    /// Environment the PYTHON sidecars' children need beyond what they inherit.
    ///
    /// macOS only, one variable. BiRefNet routes through
    /// `torchvision.ops.deform_conv2d`, torchvision ships no Metal kernel for
    /// it, and without this torch RAISES on that operator instead of running
    /// that one step on the CPU.
    ///
    /// Set on the child's own block and deliberately NOT added to the `.env`
    /// pass-through allowlist: a `.env` arriving inside a photo pack has no
    /// business deciding how a model executes, and the allowlist's membership
    /// rule (compute behaviour only, nothing that loads code) is not the
    /// question here — the question is who gets to answer it.
    ///
    /// UNVERIFIED, and disclosed as such in the README: whether this fallback
    /// actually covers torchvision's own operator is a question only Apple
    /// hardware answers. If it does not, `segment.py` already degrades to
    /// U²-Net / CPU and says so rather than failing silently.
    pub fn sidecar_child_env() -> Vec<(&'static str, &'static str)> {
        if cfg!(target_os = "macos") {
            vec![("PYTORCH_ENABLE_MPS_FALLBACK", "1")]
        } else {
            Vec::new()
        }
    }

    /// True when the image role points at a ChatGPT-subscription Codex bridge
    /// (OAuth preset) rather than a real OpenAI-compatible key (API preset).
    pub fn image_is_oauth(&self) -> bool {
        self.image_provider.eq_ignore_ascii_case("oauth")
    }
}

/// `<App>.app/Contents`, when `exe` sits at `…/Contents/MacOS/<exe>`.
///
/// Recognised by SHAPE rather than by `target_os`, so the two rules that
/// depend on it — where sidecars are searched for, and where model weights are
/// cached — can be pinned by test on a machine that has no `.app`. Both
/// callers apply it only where it means something.
fn app_bundle_contents(exe: &Path) -> Option<&Path> {
    let dir = exe.parent()?;
    if dir.file_name() != Some(std::ffi::OsStr::new("MacOS")) {
        return None;
    }
    let contents = dir.parent()?;
    (contents.file_name() == Some(std::ffi::OsStr::new("Contents"))).then_some(contents)
}

/// Where every sidecar keeps its downloaded model weights.
///
/// ONE answer for all five. The Python side has always defaulted `--cache` to
/// `<script dir>/weights` and only `denoise.rs` ever overrode it, which was
/// harmless right up until the script dir became read-only.
///
/// Inside a macOS `.app` the scripts live in `Contents/Resources/python/`, and
/// that is the wrong place for roughly 1.5 GB of downloaded model: the bundle
/// is code-signed, so writing into it invalidates the signature and Gatekeeper
/// refuses the NEXT launch — an app that breaks itself the first time the user
/// presses an AI button. It is also what a user drags to the Trash. The store
/// root is the writable, per-user, survives-an-upgrade place this app already
/// owns.
///
/// Everywhere else the answer is unchanged, deliberately: an existing Windows
/// install or repo checkout keeps the cache it already downloaded.
fn default_weights_dir(script: &str) -> String {
    let bundled = cfg!(target_os = "macos")
        && env::current_exe().is_ok_and(|e| app_bundle_contents(&e).is_some());
    if bundled {
        return crate::store::store_root().join("weights").to_string_lossy().into_owned();
    }
    script_relative_weights(script)
}

/// `<script dir>/weights` — the default the five Python sidecars apply to
/// `--cache` themselves when the argv carries no such flag.
///
/// Spelled here so [`Config::weights_args`] can recognise its own answer and
/// stay silent, which is what keeps an unconfigured install's argv exactly as
/// it was before the weight cache became one shared policy.
fn script_relative_weights(script: &str) -> String {
    Path::new(script)
        .parent()
        .map(|d| d.join("weights").to_string_lossy().into_owned())
        .unwrap_or_else(|| bundled_helper("python/weights"))
}

/// The interpreter name used when nothing names one.
///
/// Windows keeps `python` — the launcher every Windows install ships, and the
/// spelling every existing setup already works with. Everywhere else it is
/// `python3`: macOS removed `the system `python`` in 12.3 and most Linux
/// distributions dropped the unversioned name years earlier, so the old
/// constant could only ever fail there.
pub fn default_python_bin() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// Interpreter NAMES the probe tries, in priority order.
///
/// The OS resolves a bare name through `PATH`, which is the platform's own
/// answer to "where is python3" — no machine path is written in this tree.
/// A user whose interpreter is not on `PATH` types the path, which is what the
/// field is for.
pub const PYTHON_CANDIDATES: &[&str] = &["python3", "python"];

/// Does `bin` actually run? Asked by executing `--version`, because "the file
/// is there" is not the question a user pressing an AI button is asking — a
/// stub that only offers to install developer tools exists, and does not work.
///
/// Never inherits this process's stdio, and never opens a console of its own:
/// the windowed build has no console to inherit, and a probe started without
/// the console flag is how a GUI ends up with a stray terminal.
pub fn python_runs(bin: &str) -> bool {
    if bin.trim().is_empty() {
        return false;
    }
    crate::windowless_command(bin)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The first [`PYTHON_CANDIDATES`] name the OS resolves AND that runs, or
/// `None`.
///
/// `None` is a real answer the caller must show as one — "we asked the OS and
/// found nothing" is actionable, a silently unchanged field is not.
pub fn probe_python_bin() -> Option<String> {
    PYTHON_CANDIDATES.iter().find(|c| python_runs(c)).map(|c| (*c).to_string())
}

/// The directories [`bundled_helper`] searches, for an executable at `exe`.
///
/// Split out as a pure function over an explicit `app_bundles` flag — the same
/// shape `store::key_from_spelling` uses for its platform rule — so the macOS
/// behaviour can be pinned by test on a machine that has no `.app`, and so the
/// Windows walk can be pinned as unchanged in the same breath.
///
/// Inside a macOS application bundle the search STOPS AT THE BUNDLE. The
/// executable sits at `<App>.app/Contents/MacOS/<app>`, so the third ancestor
/// of its directory is whatever folder the `.app` was dragged into — ~/Downloads,
/// beside the zip it came out of — and a `python/segment.py` dropped there
/// satisfies `exists()`. That is precisely the planted-script hole
/// [`bundled_helper`]'s own doc says it closed, reopened by a directory layout
/// that postdates it. `Contents/Resources` is added instead: where the bundle
/// keeps its own sidecars.
fn bundled_helper_roots_for(exe: &Path, app_bundles: bool) -> Vec<PathBuf> {
    let Some(dir) = exe.parent() else { return Vec::new() };
    if app_bundles
        && let Some(contents) = app_bundle_contents(exe)
    {
        return vec![dir.to_path_buf(), contents.join("Resources")];
    }
    let mut roots = vec![dir.to_path_buf()];
    let mut up = dir;
    for _ in 0..3 {
        match up.parent() {
            Some(p) => {
                roots.push(p.to_path_buf());
                up = p;
            }
            None => break,
        }
    }
    roots
}

/// Resolve a bundled helper (sidecar script / weight cache) against the
/// PROGRAM'S OWN tree, never the working directory. The cwd is ambient input
/// — photos arrive in unzipped packs, and a pack carrying `python/denoise.py`
/// satisfied the old cwd-relative default's `exists()` gate, so the pack's
/// script ran as the user. Candidates: the executable's directory (a packaged
/// install) and its first three ancestors (`target/release` and
/// `target/debug/deps` both reach the repo root). When nothing answers, the
/// FIRST candidate still names the expected location, so the sidecar's
/// "not found at <path>" error stays actionable — and never names a path an
/// untrusted pack could satisfy.
pub fn bundled_helper(rel: &str) -> String {
    let roots = env::current_exe()
        .map(|exe| bundled_helper_roots_for(&exe, cfg!(target_os = "macos")))
        .unwrap_or_default();
    for root in &roots {
        let cand = root.join(rel);
        if cand.exists() {
            return cand.to_string_lossy().into_owned();
        }
    }
    roots.first().map(|r| r.join(rel).to_string_lossy().into_owned()).unwrap_or_else(|| {
        // current_exe() unavailable (e.g. a chroot without /proc): FAIL
        // CLOSED. Returning `rel` here would quietly reopen the
        // planted-script hole as a cwd-relative lookup (Codex R11 #2). A
        // NUL byte is unrepresentable in real paths on every platform, so
        // this sentinel can never be satisfied by any file an untrusted
        // pack could create — the sidecar's "not found at <path>" refusal
        // fires instead.
        format!("\u{0}install-dir-unavailable/{rel}")
    })
}

impl Config {
    /// The interpreter the AI sidecars actually run.
    ///
    /// An interpreter the user named — the settings field, or
    /// `AUTOSHADE_PYTHON` — is used as given. Otherwise the managed
    /// environment ([`crate::python_env`]) outranks the platform default, so
    /// the app never executes whichever `python` happens to be first on
    /// `PATH`: on Windows that name is Python 2 on machines that still carry
    /// it, and Python 2 cannot parse the sidecars.
    pub fn sidecar_python(&self) -> String {
        if self.python_bin != default_python_bin() {
            return self.python_bin.clone();
        }
        crate::python_env::installed_python()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.python_bin.clone())
    }
}


// --- DeepSeek (the user's provider) -----------------------------------------

/// DeepSeek's OpenAI-compatible endpoint.
pub const DEEPSEEK_BASE: &str = "https://api.deepseek.com";
/// The model the pi harness runs and the legacy chain is pointed at.
pub const DEEPSEEK_MODEL: &str = "deepseek-flash";
/// DeepSeek refuses OpenAI's `response_format.json_schema`; the chat family is
/// the plain chat-completions contract that both chains implement.
pub const DEEPSEEK_FAMILY: &str = "chat";

/// The key to store for ONE role when the OTHER role's key was just typed, the
/// two endpoints are both DeepSeek, and this role has no key of its own.
/// Returns `(key, key_base)`; `None` means "leave this role alone".
pub fn fill_deepseek_key(
    role_base: Option<&str>,
    other_base: Option<&str>,
    typed_other_key: &str,
    role_key_present: bool,
) -> Option<(String, Option<String>)> {
    if typed_other_key.is_empty() || role_key_present {
        return None;
    }
    let is_deepseek = |b: Option<&str>| b.is_some_and(|s| s.contains("api.deepseek.com"));
    if is_deepseek(role_base) && is_deepseek(other_base) {
        Some((typed_other_key.to_string(), role_base.map(str::to_string)))
    } else {
        None
    }
}
