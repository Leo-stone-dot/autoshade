//! The AI lens blur's depth map (2026-10-13): the bridge to `python/depth.py`,
//! which runs MoGe-2 over one picture and writes a 16-bit nearness PNG (see
//! that script's docstring for the format and the model choice, and
//! [`crate::render::LensBlur`] for how the render reads it).

use std::path::Path;

use anyhow::{bail, Context, Result};

/// Run the sidecar `cfg.depth_script` names: `input` (any image file) →
/// `output` (16-bit nearness PNG in `input`'s frame). Through the shared
/// model-sidecar run, so the GPU slot, the bounded wait and "exit 0 is not
/// success" are the family's; and the artifact must DECODE as a raster before
/// anything references it, as a segmentation mask must.
pub fn estimate_depth(cfg: &crate::config::Config, input: &Path, output: &Path) -> Result<()> {
    let script = Path::new(&cfg.depth_script);
    if !script.exists() {
        bail!(
            "depth sidecar not found at {} — run from the project dir or set AUTOSHADE_DEPTH_SCRIPT.",
            script.display()
        );
    }
    let args: Vec<std::ffi::OsString> =
        vec!["-E".into(), script.into(), "--input".into(), input.into(), "--output".into(), output.into()];
    let before = crate::run_model_sidecar_artifact("depth sidecar", &cfg.sidecar_python(), args, output)?;
    if let Err(e) = crate::render::open_mask_bounded(output) {
        crate::denoise::discard_failed_output(output, before);
        return Err(e.context(format!("depth sidecar produced an undecodable map {}", output.display())));
    }
    crate::store::durable_adopt(output).with_context(|| format!("sync depth map {}", output.display()))
}
