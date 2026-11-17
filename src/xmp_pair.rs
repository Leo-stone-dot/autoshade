//! ONE rule for pairing a photograph with the PHOTOGRAPHER's XMP sidecar.
//!
//! Until this module the rule was open-coded as `raw.with_extension("xmp")` at
//! six sites — the style-index pair scan and its sidecar read, `eval`'s pair
//! scan, its per-photo read, its resume hash, its forensic probe, and the CLI's
//! Lightroom import note. Two defects followed from having no owner:
//!
//! * **A separate sidecar tree could not be used at all.** A photographer who
//!   keeps `.xmp` files beside the RAWs is the only one the index could learn
//!   from; anyone whose sidecars live in a parallel folder (a read-only photo
//!   volume, a catalogue exported elsewhere) got "0 RAW+.xmp pairs" and an
//!   empty-index refusal, with nothing to point at the real folder.
//! * **`with_extension` writes a LOWERCASE `.xmp`.** On Windows `Path::exists`
//!   folds case, so `SHOT.XMP` paired anyway; on macOS and Linux it does not,
//!   so the same library indexed on Windows and indexed on the Mac build (which
//!   has shipped since v1.1.0) produced different pair counts from the same
//!   files, and said nothing about it.
//!
//! **The rule.** Given a RAW, the sidecar is the file whose STEM is the RAW's
//! stem and whose EXTENSION is `xmp` in any ASCII case, looked for in three
//! places in order:
//!
//! 1. `<xmp_dir>/<the RAW's folder relative to the library root>/<stem>.xmp` —
//!    a sidecar tree that MIRRORS the library, which is what an exported
//!    catalogue and every "keep metadata off the photo volume" workflow
//!    produces;
//! 2. `<xmp_dir>/<stem>.xmp` — the FLAT mirror, one folder of sidecars for a
//!    nested library;
//! 3. `<the RAW's own folder>/<stem>.xmp` — the sibling, which is what
//!    Lightroom writes and the only place the old code looked.
//!
//! Mirror before flat before sibling, because each is more specific about
//! WHICH library this sidecar belongs to than the next: a flat `<stem>.xmp`
//! can be claimed by two RAWs of the same name in two subfolders, and the
//! sibling is a file the user may not even have meant to hand us.
//!
//! **The extension is matched case-insensitively; the STEM is not.** The
//! extension is the half the world spells inconsistently (Lightroom writes
//! `.xmp`, several camera utilities and older ACR builds write `.XMP`), while
//! every writer of a sidecar — Lightroom, ACR and this app's own
//! [`crate::store::xmp_beside_target`] — reproduces the photograph's stem byte
//! for byte. Folding the stem too would let `shot.arw` claim `Shot.xmp` on a
//! case-sensitive volume, i.e. one photograph's edit scored against another's
//! pixels, which is a worse failure than the one being fixed.
//!
//! **Directory listing, never `Path::exists`.** Case-insensitive matching by
//! probing `<stem>.XMP`, `<stem>.Xmp`, … is eight stats per photograph and
//! still only covers the casings someone thought of; and on a case-insensitive
//! volume `exists()` answers about a name the FS chose, not the one on disk.
//! One `read_dir` per FOLDER, memoised, answers exactly and once.
//!
//! **What this module deliberately does NOT own.** The develop chain's own
//! sidecar sites — [`crate::store::lightroom_sidecar`],
//! `store::has_develop_or_sidecar` and `pipeline::write_xmp`'s merge base —
//! read a path that is also the path AutoShade WRITES back to
//! ([`crate::store::xmp_beside_target`]). Resolving those reads
//! case-insensitively without moving the write would split one photograph's
//! sidecar into two files (`SHOT.XMP` read, `shot.xmp` written), so that pair
//! is a separate change with its own risk and is registered, not smuggled in
//! here. Everything this module owns is READ-ONLY: nothing is ever written to
//! a path it returns.

use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The sidecar extension in the spelling every WRITER uses. Compared with
/// [`str::eq_ignore_ascii_case`], written as-is.
const XMP_EXT: &str = "xmp";

/// One library scan's pairing rule, with the folder listings it has already
/// paid for.
///
/// Held by ONE thread for the length of a scan: the memo behind it is a
/// [`RefCell`], deliberately, because every caller resolves its pairs up front
/// on the calling thread and carries the resolved paths into whatever pool
/// comes next. That ordering is what keeps a 2,000-photograph library at one
/// `read_dir` per folder instead of one per photograph.
pub struct XmpPairing<'a> {
    root: Option<&'a Path>,
    xmp_dir: Option<&'a Path>,
    listed: RefCell<HashMap<PathBuf, HashMap<OsString, OsString>>>,
}

impl<'a> XmpPairing<'a> {
    /// The pairing for a library scan: `root` is the folder the RAWs were
    /// found under, `xmp_dir` the `--xmp-dir` the user gave (or `None`).
    pub fn new(root: &'a Path, xmp_dir: Option<&'a Path>) -> Self {
        XmpPairing { root: Some(root), xmp_dir, listed: RefCell::new(HashMap::new()) }
    }

    /// The convention with no separate sidecar tree: the sibling file alone.
    ///
    /// For the single-photograph callers (the CLI's Lightroom import note),
    /// which have no library root to be relative to. They still go through
    /// this module rather than `with_extension`, because the case rule is the
    /// whole point and a photograph does not stop having a `.XMP` sidecar
    /// because it was named on a command line.
    pub fn beside() -> XmpPairing<'static> {
        XmpPairing { root: None, xmp_dir: None, listed: RefCell::new(HashMap::new()) }
    }

    /// This RAW's sidecar, or `None` when no folder in the rule holds one.
    ///
    /// The returned path names a file that EXISTED when its folder was listed.
    /// It is never a name to write to — see the module docs.
    pub fn find(&self, raw: &Path) -> Option<PathBuf> {
        let stem = raw.file_stem()?;
        for dir in self.search_dirs(raw) {
            if let Some(name) = self.lookup(&dir, stem) {
                return Some(dir.join(name));
            }
        }
        None
    }

    /// The three folders of the rule, in order, deduplicated — a flat
    /// `--xmp-dir` pointed at the library root is ONE folder, not three
    /// listings of it.
    fn search_dirs(&self, raw: &Path) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::with_capacity(3);
        if let Some(x) = self.xmp_dir {
            if let Some(rel) = self.root.and_then(|r| raw.parent()?.strip_prefix(r).ok()) {
                dirs.push(x.join(rel));
            }
            let flat = x.to_path_buf();
            if !dirs.contains(&flat) {
                dirs.push(flat);
            }
        }
        if let Some(p) = raw.parent() {
            let sibling = p.to_path_buf();
            if !dirs.contains(&sibling) {
                dirs.push(sibling);
            }
        }
        dirs
    }

    /// The sidecar named `<stem>.<any case of xmp>` in `dir`, from the
    /// memoised listing.
    fn lookup(&self, dir: &Path, stem: &OsStr) -> Option<OsString> {
        let mut listed = self.listed.borrow_mut();
        let names = listed.entry(dir.to_path_buf()).or_insert_with(|| list_xmps(dir));
        names.get(stem).cloned()
    }
}

/// Every `<stem> -> <file name>` this folder offers, by ONE listing.
///
/// An unreadable folder (absent, denied, a `--xmp-dir` the user mistyped)
/// answers an EMPTY map rather than failing: a missing sidecar folder means
/// "no pair from here", which the next candidate folder — and, failing that,
/// the build's own skipped-for-sidecar disclosure — already reports honestly.
fn list_xmps(dir: &Path) -> HashMap<OsString, OsString> {
    // `Path::new("")` is what `Path::parent` answers for a bare file name, and
    // `read_dir("")` fails on every platform. The CWD is what
    // `with_extension("xmp").exists()` resolved such a name against, so it is
    // what this resolves against too.
    let target = if dir.as_os_str().is_empty() { Path::new(".") } else { dir };
    let mut out: HashMap<OsString, OsString> = HashMap::new();
    let Ok(rd) = std::fs::read_dir(target) else { return out };
    for entry in rd.flatten() {
        // A FOLDER called `shot.xmp` is not a sidecar. Cheap here (the entry's
        // type is already in hand on both platforms) and the alternative is an
        // unreadable-file error several frames later.
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let as_path = Path::new(&name);
        if !as_path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(XMP_EXT))
        {
            continue;
        }
        let Some(stem) = as_path.file_stem() else { continue };
        match out.entry(stem.to_os_string()) {
            Entry::Vacant(v) => {
                v.insert(name);
            }
            Entry::Occupied(mut o) => {
                if outranks(&name, o.get()) {
                    o.insert(name);
                }
            }
        }
    }
    out
}

/// Which of two same-stem candidates a folder offers is THE sidecar.
///
/// Only reachable on a case-SENSITIVE volume, where `shot.xmp` and `shot.XMP`
/// can both exist. Exactly-`xmp` wins, because that is the spelling every
/// writer produces and so the one the photographer's live editor is keeping
/// current; between two equally-odd casings the lexicographically smaller name
/// wins, so the answer does not depend on the order the OS happened to list
/// the folder in.
fn outranks(candidate: &OsStr, current: &OsStr) -> bool {
    let exact = |n: &OsStr| Path::new(n).extension().is_some_and(|e| e == OsStr::new(XMP_EXT));
    match (exact(candidate), exact(current)) {
        (true, false) => true,
        (false, true) => false,
        _ => candidate < current,
    }
}
