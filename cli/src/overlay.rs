use anyhow::{bail, Context};
use std::path::Path;
use std::process::Command;

use crate::checks;
use crate::system::System;

/// Create a sparse ext3 overlay image.
pub fn create_overlay(sys: &dyn System, path: &Path, size_mb: u64) -> anyhow::Result<()> {
    if size_mb < 64 {
        bail!("Overlay size must be at least 64 MB");
    }

    let apptainer = checks::apptainer_binary(sys)
        .context("apptainer/singularity not found")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let status = Command::new(&apptainer)
        .args(["overlay", "create", "--sparse", "--size", &size_mb.to_string()])
        .arg(path)
        .status()
        .with_context(|| format!("Failed to run {apptainer} overlay create"))?;

    if !status.success() {
        bail!("{apptainer} overlay create failed (exit code: {:?})", status.code());
    }

    Ok(())
}

/// Initialize the Nix store database inside the container.
pub fn init_nix_db(sys: &dyn System, sif_path: &Path, overlay_path: &Path) -> anyhow::Result<()> {
    let apptainer = checks::apptainer_binary(sys)
        .context("apptainer/singularity not found")?;

    let status = Command::new(&apptainer)
        .args(["exec", "--overlay"])
        .arg(overlay_path)
        .arg(sif_path)
        .args([
            "/bin/sh", "-c",
            "if [ -f /nix-path-registration ] && [ ! -f /nix/var/nix/db/db.sqlite ]; then /usr/local/bin/nix-store --load-db < /nix-path-registration && echo 'Store database initialized.' ; else echo 'Store database already exists or no registration file found.' ; fi"
        ])
        .status()
        .with_context(|| format!("Failed to run {apptainer} exec"))?;

    if !status.success() {
        bail!("Nix store database initialization failed");
    }

    Ok(())
}
