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

/// Create a directory overlay with upper/ and work/ subdirs.
pub fn create_directory_overlay(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path.join("upper"))
        .with_context(|| format!("Failed to create overlay upper dir: {}", path.display()))?;
    std::fs::create_dir_all(path.join("work"))
        .with_context(|| format!("Failed to create overlay work dir: {}", path.display()))?;
    Ok(())
}

/// Pre-seed the Nix store DB into the overlay by running nix-store --load-db
/// inside the container. This makes the DB available on the native filesystem
/// (in the overlay's upper layer) so SQLite reads bypass squashfuse.
///
/// This is idempotent — nix-store --load-db is additive and safe to re-run.
/// If it fails, the overlay is still usable (just slower); a warning is printed.
pub fn preseed_nix_db(sys: &dyn System, apptainer: &str, overlay: &str, sif: &str) -> anyhow::Result<()> {
    let status = sys.run_command(
        apptainer,
        &["exec", "--overlay", overlay, sif, "sh", "-c", "nix-store --load-db < /nix-path-registration"],
    ).with_context(|| "Failed to run apptainer exec for DB pre-seeding")?;

    if !status.success() {
        eprintln!("Warning: Nix DB pre-seeding failed (exit code: {:?}). The container will still work but may be slower on first use.", status.code());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_directory_overlay() {
        let tmp = TempDir::new().unwrap();
        let overlay_dir = tmp.path().join("overlay");
        create_directory_overlay(&overlay_dir).unwrap();
        assert!(overlay_dir.join("upper").is_dir());
        assert!(overlay_dir.join("work").is_dir());
    }

    #[test]
    fn test_create_directory_overlay_idempotent() {
        let tmp = TempDir::new().unwrap();
        let overlay_dir = tmp.path().join("overlay");
        create_directory_overlay(&overlay_dir).unwrap();
        // Second call should not error
        create_directory_overlay(&overlay_dir).unwrap();
        assert!(overlay_dir.join("upper").is_dir());
    }
}

