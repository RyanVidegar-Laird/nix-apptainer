pub mod clean;
pub mod enter;
pub mod exec;
pub mod init;
pub mod status;
pub mod update;
pub mod verify;

use crate::config::{Config, OverlayType};
use crate::paths::AppPaths;
use anyhow::bail;
use std::path::PathBuf;

/// Replace this process with the apptainer invocation (never returns on
/// success — the Err is the exec(2) failure).
pub fn exec_replace(program: &str, args: &[String]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    std::process::Command::new(program).args(args).exec()
}

/// Warn on stderr when an ext3 overlay crosses 80% usage.
pub fn warn_if_overlay_full(config: &Config, paths: &AppPaths) {
    if config.overlay.overlay_type != OverlayType::Ext3 {
        return;
    }
    use std::os::unix::fs::MetadataExt;
    if let Ok(meta) = std::fs::metadata(&paths.overlay_path) {
        let on_disk = meta.blocks() * 512;
        let allocated = meta.len();
        if let Some(warning) = crate::util::overlay_usage_warning(on_disk, allocated, 80) {
            eprintln!("{warning}");
        }
    }
}

/// What the container session runs against.
#[derive(Debug)]
pub enum Storage {
    /// Read-only SIF plus a writable overlay (dir path or ext3 image path).
    Overlay(String),
    /// Writable sandbox directory — no SIF mounted at runtime.
    Sandbox(PathBuf),
}

/// Resolve the runtime storage from config. Also validates that the
/// pieces each mode needs actually exist (overlay modes need base.sif;
/// sandbox mode does not — the SIF is only a delivery format there).
pub fn resolve_storage(config: &Config, paths: &AppPaths) -> anyhow::Result<Storage> {
    match config.overlay.overlay_type {
        OverlayType::Directory | OverlayType::Ext3 => {
            if !paths.sif_path.exists() {
                bail!(
                    "Base SIF not found at {}. Run `nix-apptainer init` first.",
                    paths.sif_path.display()
                );
            }
            match config.overlay.overlay_type {
                OverlayType::Directory => {
                    if !paths.overlay_dir.exists() {
                        bail!(
                            "Directory overlay not found at {}. Run `nix-apptainer init` first.",
                            paths.overlay_dir.display()
                        );
                    }
                    Ok(Storage::Overlay(
                        paths.overlay_dir.to_string_lossy().to_string(),
                    ))
                }
                OverlayType::Ext3 => {
                    if !paths.overlay_path.exists() {
                        bail!(
                            "Overlay image not found at {}. Run `nix-apptainer init` first.",
                            paths.overlay_path.display()
                        );
                    }
                    Ok(Storage::Overlay(
                        paths.overlay_path.to_string_lossy().to_string(),
                    ))
                }
                OverlayType::Sandbox => unreachable!(),
            }
        }
        OverlayType::Sandbox => {
            if !paths.sandbox_dir.exists() {
                bail!(
                    "Sandbox directory not found at {}. Run `nix-apptainer init` first.",
                    paths.sandbox_dir.display()
                );
            }
            Ok(Storage::Sandbox(paths.sandbox_dir.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::OverlayConfig;
    use tempfile::TempDir;

    fn config_with(t: OverlayType) -> Config {
        Config {
            overlay: OverlayConfig {
                overlay_type: t,
                ext3_size_mb: 64,
            },
            ..Config::default()
        }
    }

    #[test]
    fn test_resolve_storage_sandbox_ok() {
        let tmp = TempDir::new().unwrap();
        let paths = AppPaths::resolve_with_data_dir(tmp.path().to_path_buf());
        std::fs::create_dir_all(&paths.sandbox_dir).unwrap();
        let storage = resolve_storage(&config_with(OverlayType::Sandbox), &paths).unwrap();
        match storage {
            Storage::Sandbox(dir) => assert_eq!(dir, paths.sandbox_dir),
            _ => panic!("expected Storage::Sandbox"),
        }
    }

    #[test]
    fn test_resolve_storage_sandbox_missing() {
        let tmp = TempDir::new().unwrap();
        let paths = AppPaths::resolve_with_data_dir(tmp.path().to_path_buf());
        let err = resolve_storage(&config_with(OverlayType::Sandbox), &paths).unwrap_err();
        assert!(err.to_string().contains("init"), "err: {err}");
    }

    #[test]
    fn test_resolve_storage_overlay_requires_sif() {
        let tmp = TempDir::new().unwrap();
        let paths = AppPaths::resolve_with_data_dir(tmp.path().to_path_buf());
        std::fs::create_dir_all(&paths.overlay_dir).unwrap();
        // overlay dir exists but base.sif does not
        let err = resolve_storage(&config_with(OverlayType::Directory), &paths).unwrap_err();
        assert!(err.to_string().contains("SIF"), "err: {err}");
    }

    #[test]
    fn test_resolve_storage_overlay_ok() {
        let tmp = TempDir::new().unwrap();
        let paths = AppPaths::resolve_with_data_dir(tmp.path().to_path_buf());
        std::fs::create_dir_all(&paths.overlay_dir).unwrap();
        std::fs::write(&paths.sif_path, b"fake").unwrap();
        let storage = resolve_storage(&config_with(OverlayType::Directory), &paths).unwrap();
        matches!(storage, Storage::Overlay(_))
            .then_some(())
            .expect("expected Storage::Overlay");
    }
}
