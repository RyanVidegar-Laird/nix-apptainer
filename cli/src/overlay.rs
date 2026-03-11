use anyhow::{bail, Context};
use std::path::Path;

use crate::checks;
use crate::system::System;

/// Create a sparse ext3 overlay image at the given path.
/// Requires apptainer/singularity on PATH. Minimum size is 64 MB.
pub fn create_overlay(sys: &dyn System, path: &Path, size_mb: u64) -> anyhow::Result<()> {
    if size_mb < 64 {
        bail!("Overlay size must be at least 64 MB");
    }

    let apptainer = checks::apptainer_binary(sys)
        .context("apptainer/singularity not found")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let path_str = path.to_string_lossy();
    let size_str = size_mb.to_string();
    let status = sys.run_command(
        &apptainer,
        &["overlay", "create", "--sparse", "--size", &size_str, &path_str],
    ).with_context(|| format!("Failed to run {apptainer} overlay create"))?;

    if !status.success() {
        bail!("{apptainer} overlay create failed (exit code: {:?})", status.code());
    }

    Ok(())
}

