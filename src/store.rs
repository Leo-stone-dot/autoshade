//! Central per-user develop store — WHERE a photo's develop state lives.
//!
//! Until v0.12.0 every sidecar (recipe.json / .xmp / v<N> snapshots / mask
//! rasters) was keyed by the photo's bare file STEM inside a cwd-relative
//! `./out`. Two same-named photos in different folders therefore shared one
//! sidecar (silent cross-clobber), and launching the app from a different
//! directory hid every existing edit. This module fixes both by giving each
//! photo its own directory under a per-user root, keyed by the photo's
//! ABSOLUTE path:
//!
//! ```text
//! <root>/develops/<stem>-<fnv1a64(abs path)>/
//!     recipe.json          the working develop (single source of truth)
//!     <stem>.xmp           Lightroom projection (name kept: LR needs <stem>.xmp)
//!     v<N>.recipe.json     numbered snapshots
//!     <kind>.png           mask rasters (mask-sky, mask-zone-sky, …)
//!     pixels.json          baked pixel-master link (retouch/reimagine origin)
//!     variants.json        GUI variant strip (background variants + active kind)
//!     source.txt           breadcrumb: which photo this dir belongs to
//! ```
//!
//! The root resolves at runtime (never hardcoded): `AUTOSHADE_DATA_DIR` env
//! override → `%LOCALAPPDATA%/autoshade` (the thumb cache already lives there)
//! → the system temp dir. EXPORTS (developed/retouch/heal/… images) are user
//! deliverables and deliberately STAY in ./out.
//!
//! Rasters (masks, the lens blur's depth map and protect matte, pixel
//! layers) are referenced from recipe.json by a path string. Inside the
//! store that string is the BARE file name, resolved against the recipe's own
//! directory at load time ([`resolve_mask_paths`]) and relativized back at
//! write time ([`relativize_mask_paths`]) — so a develop dir is relocatable
//! and legacy cwd-relative "out/…" references keep working unchanged.

use std::{
    cell::RefCell,
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

// Raster paths are walked through `EditRecipe::raster_paths_mut` (every mask's
// `LocalAdjustment::bitmap_paths_mut`, plus the lens blur's and the pixel
// layers'), so this module no longer pattern-matches `MaskGeometry` directly.
use crate::recipe::EditRecipe;
mod backup;
mod clear;
mod commit;
mod durable;
mod key;
mod lock;
mod migrate;
mod paths;
mod root;
mod sidecar;
mod snapshot;
mod variants;
mod versions;

pub use backup::{
    backup_saved_develop, claim_version, delete_version, rollback_frozen_rasters, snapshot_rasters,
};
use backup::{deleting_marker, read_deleted_versions, register_deleted_version};
pub use clear::{
    ClearOutcome, clear_develop, disable_unanchored_mask_paths, list_versions, mark_develop_cleared,
    relativize_mask_paths, resolve_mask_paths,
};
use clear::{clear_sweep, contained_join, remote_or_device_path};
pub use commit::{DevelopCommit, commit_develop};
use commit::{commit_dir_in, resolve_pending_commit_unlocked};
pub use durable::{durable_adopt};
pub(crate) use durable::{
    durable_rename, durable_replace, durable_retire_and_write, durable_write, next_tmp_seq,
};
use durable::{
    copy_atomic, durable_os, durable_write_tracked, move_file_no_clobber, publish_no_clobber,
    sibling_tmp, sync_staged, write_staged,
};
pub use key::{AliasNote, photo_key, take_alias_note, volume_folds_case};
pub(crate) use key::{photo_key_lexical};
use key::{forget_resolved_key, resolve_key_in, resume_marked_adoption};
pub use lock::{DevelopLockMode, with_develop_lock, with_settings_lock};
use lock::{with_develop_lock_in, with_path_lock};
pub use migrate::{migrate_legacy, migrate_legacy_from_many};
pub use paths::{
    mask_family,
    OwnedRaster, claim_raster, clear_pixel_source, detach_rasters, develop_dir, export_xmp_beside,
    masters_dir, own_areas_in, pixel_source_path, pixel_source_record_bytes, raster_target, recipe_target, saved_quarter_turns,
    variants_path, version_target, write_pixel_source, xmp_beside_target, xmp_target,
};
use paths::{
    clear_pixel_source_unlocked_in, develop_dir_in, pixel_source_path_in, publish_json_sidecar,
    recipe_target_in, variants_path_in, xmp_target_in,
};
pub use root::{RootAdoption, RootTrust, store_root, store_root_with_trust};
use root::{FNV_OFFSET, fnv1a64, fnv1a64_update};
pub use sidecar::{
    LrSidecar, MAX_STORE_JSON, SidecarRead, SidecarStamp, embedded_packet_for_restore, has_develop,
    has_develop_or_sidecar, legacy_recipe, legacy_xmp, lightroom_sidecar, read_bytes_capped,
    read_sidecar, read_sidecar_checked, read_sidecar_stamped, read_text_capped, settings_path,
};
use sidecar::{
    clear_pending, clear_pending_in, legacy_out_roots, legacy_suppressed, legacy_suppressed_in,
    suppress_legacy_in,
};
pub use snapshot::{
    DevelopSnapshot, develop_revision, develop_revision_of, has_pixel_source, read_develop_snapshot,
    read_pixel_source, recipe_revision, recorded_pixel_source, recover_orphan_baks,
    render_source_checked,
};
use snapshot::{
    has_pixel_source_in, recover_orphan_baks_unlocked, resolve_pending_clear_unlocked,
    settle_consumed_marker,
};
pub use variants::{
    ActiveWrite, CommitMember, MAX_STORE_NAME, VariantEntry, VariantsRead, VariantsRecord,
    active_card_is_denoised, clear_variants, read_variants, read_variants_checked, variants_member, variants_record_bytes,
    write_variants,
};
use variants::{capped_name, refuse_unresolved_strip};
pub use versions::{
    EditState, EditStateKind, VERSION_ORIGIN_AUTO, VERSION_ORIGIN_USER, VersionMetaEntry,
    list_edits, note_source, read_version_meta, record_version_meta, set_version_name,
};
use versions::{
    delete_version_unlocked, record_version_meta_unlocked, recover_pending_version_deletes_unlocked,
};
