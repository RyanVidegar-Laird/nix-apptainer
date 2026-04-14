use anyhow::{bail, Context};
use std::os::unix::fs::PermissionsExt;
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
/// Pre-creates the Nix state directory tree inside upper/ so that
/// fuse-overlayfs presents user-owned dirs (not root-owned from squashfs).
/// This is critical for directory overlays where copy-up preserves
/// squashfs root ownership and non-root users can't chmod those dirs.
pub fn create_directory_overlay(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path.join("upper"))
        .with_context(|| format!("Failed to create overlay upper dir: {}", path.display()))?;
    std::fs::create_dir_all(path.join("work"))
        .with_context(|| format!("Failed to create overlay work dir: {}", path.display()))?;

    // Pre-create Nix state dirs in upper/ so they are owned by the
    // calling user on the host filesystem. Without this, these dirs
    // only exist in the squashfs (owned by root due to -all-root),
    // and fuse-overlayfs copy-up preserves that root ownership —
    // causing "Permission denied" when Nix tries to chmod them.
    let upper = path.join("upper");
    let nix_state_dirs = [
        "nix/var/nix/db",
        "nix/var/nix/profiles/per-user",
        "nix/var/nix/gcroots/auto",
        "nix/var/nix/temproots",
        "nix/var/nix/builds",
        "nix/store/.links",
    ];
    let world_writable = std::fs::Permissions::from_mode(0o777);
    for dir in nix_state_dirs {
        let full = upper.join(dir);
        std::fs::create_dir_all(&full)
            .with_context(|| format!("Failed to create overlay dir: {}", full.display()))?;
        // Set 777 on every component from upper/ down to the leaf.
        // create_dir_all applies umask (typically 0o755) to intermediates,
        // which would shadow the squashfs's 777 dirs and re-trigger the
        // fuse-overlayfs access() EPERM bug on older versions.
        let mut cumulative = upper.clone();
        for component in Path::new(dir).components() {
            cumulative = cumulative.join(component);
            std::fs::set_permissions(&cumulative, world_writable.clone())
                .with_context(|| format!("Failed to set permissions on: {}", cumulative.display()))?;
        }
    }

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
        &["exec", "--overlay", overlay, sif, "sh", "-c",
          "chmod -R 777 /nix/var/nix 2>/dev/null; nix-store --load-db < /nix-path-registration"],
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

    #[test]
    fn test_directory_overlay_creates_nix_state_dirs() {
        let tmp = TempDir::new().unwrap();
        let overlay_dir = tmp.path().join("overlay");
        create_directory_overlay(&overlay_dir).unwrap();

        // Nix state dirs must exist in upper/ so fuse-overlayfs
        // presents them as user-owned (not root-owned from squashfs)
        let upper = overlay_dir.join("upper");
        for dir in [
            "nix/var/nix/db",
            "nix/var/nix/profiles/per-user",
            "nix/var/nix/gcroots/auto",
            "nix/var/nix/temproots",
            "nix/var/nix/builds",
            "nix/store/.links",
        ] {
            assert!(upper.join(dir).is_dir(), "missing upper/{dir}");
        }
    }

    #[test]
    fn test_directory_overlay_nix_dirs_are_world_writable() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new().unwrap();
        let overlay_dir = tmp.path().join("overlay");
        create_directory_overlay(&overlay_dir).unwrap();

        let upper = overlay_dir.join("upper");
        // Check both a leaf dir and an intermediate dir
        for dir in ["nix/var/nix/db", "nix/var/nix"] {
            let mode = std::fs::metadata(upper.join(dir))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o777, "{dir} should be mode 777, got {:o}", mode & 0o777);
        }
    }
}

