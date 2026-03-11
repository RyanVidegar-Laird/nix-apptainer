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

/// Initialize the Nix store database inside the container.
/// Runs `nix-store --load-db` via apptainer exec if the DB doesn't already exist.
pub fn init_nix_db(sys: &dyn System, sif_path: &Path, overlay_path: &Path) -> anyhow::Result<()> {
    let apptainer = checks::apptainer_binary(sys)
        .context("apptainer/singularity not found")?;

    let overlay_str = overlay_path.to_string_lossy();
    let sif_str = sif_path.to_string_lossy();
    let status = sys.run_command(
        &apptainer,
        &[
            "exec", "--overlay", &overlay_str, &sif_str,
            "/bin/sh", "-c",
            "if [ -f /nix-path-registration ] && [ ! -f /nix/var/nix/db/db.sqlite ]; then /usr/local/bin/nix-store --load-db < /nix-path-registration && echo 'Store database initialized.' ; else echo 'Store database already exists or no registration file found.' ; fi"
        ],
    ).with_context(|| format!("Failed to run {apptainer} exec"))?;

    if !status.success() {
        bail!("Nix store database initialization failed");
    }

    Ok(())
}
