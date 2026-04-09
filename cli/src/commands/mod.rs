pub mod clean;
pub mod enter;
pub mod exec;
pub mod init;
pub mod status;
pub mod update;
pub mod verify;

use anyhow::bail;
use crate::config::{Config, OverlayType};
use crate::paths::AppPaths;

/// Resolve the overlay path based on the configured overlay type.
pub fn resolve_overlay(config: &Config, paths: &AppPaths) -> anyhow::Result<String> {
    match config.overlay.overlay_type {
        OverlayType::Directory => {
            if !paths.overlay_dir.exists() {
                bail!(
                    "Directory overlay not found at {}. Run `nix-apptainer init` first.",
                    paths.overlay_dir.display()
                );
            }
            Ok(paths.overlay_dir.to_string_lossy().to_string())
        }
        OverlayType::Ext3 => {
            if !paths.overlay_path.exists() {
                bail!(
                    "Overlay image not found at {}. Run `nix-apptainer init` first.",
                    paths.overlay_path.display()
                );
            }
            Ok(paths.overlay_path.to_string_lossy().to_string())
        }
    }
}
