use std::path::PathBuf;

/// Resolved paths for all nix-apptainer data locations.
pub struct AppPaths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub sif_path: PathBuf,
    pub overlay_path: PathBuf,
    pub overlay_dir: PathBuf,
    pub state_file: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppPaths {
    /// Resolve paths. If `NIX_APPTAINER_HOME` is set, everything goes there.
    /// Otherwise, use XDG directories.
    pub fn resolve() -> anyhow::Result<Self> {
        if let Ok(home) = std::env::var("NIX_APPTAINER_HOME") {
            Ok(Self::from_base_dir(PathBuf::from(home)))
        } else {
            Self::from_xdg()
        }
    }

    /// Resolve with a custom data directory override (from `init --data-dir`).
    pub fn resolve_with_data_dir(data_dir: PathBuf) -> Self {
        Self::from_base_dir(data_dir)
    }

    fn from_base_dir(base: PathBuf) -> Self {
        Self {
            config_file: base.join("config.toml"),
            data_dir: base.clone(),
            sif_path: base.join("base.sif"),
            overlay_path: base.join("overlay.img"),
            overlay_dir: base.join("overlay"),
            state_file: base.join("state.json"),
            cache_dir: base.join("cache"),
        }
    }

    fn from_xdg() -> anyhow::Result<Self> {
        let xdg_dirs = xdg::BaseDirectories::with_prefix("nix-apptainer")
            .map_err(|e| anyhow::anyhow!("Could not determine XDG directories: {e}"))?;
        let config_dir = xdg_dirs.get_config_home();
        let data_dir = xdg_dirs.get_data_home();
        let cache_dir = xdg_dirs.get_cache_home();
        Ok(Self {
            config_file: config_dir.join("config.toml"),
            data_dir: data_dir.clone(),
            sif_path: data_dir.join("base.sif"),
            overlay_path: data_dir.join("overlay.img"),
            overlay_dir: data_dir.join("overlay"),
            state_file: data_dir.join("state.json"),
            cache_dir,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_base_dir() {
        let paths = AppPaths::from_base_dir(PathBuf::from("/tmp/nix-apptainer-test"));
        assert_eq!(paths.config_file, PathBuf::from("/tmp/nix-apptainer-test/config.toml"));
        assert_eq!(paths.sif_path, PathBuf::from("/tmp/nix-apptainer-test/base.sif"));
        assert_eq!(paths.overlay_path, PathBuf::from("/tmp/nix-apptainer-test/overlay.img"));
        assert_eq!(paths.state_file, PathBuf::from("/tmp/nix-apptainer-test/state.json"));
        assert_eq!(paths.cache_dir, PathBuf::from("/tmp/nix-apptainer-test/cache"));
    }

    #[test]
    fn test_overlay_dir_from_base() {
        let paths = AppPaths::from_base_dir(PathBuf::from("/tmp/nix-apptainer-test"));
        assert_eq!(paths.overlay_dir, PathBuf::from("/tmp/nix-apptainer-test/overlay"));
    }

    #[test]
    fn test_overlay_dir_resolve_with_data_dir() {
        let paths = AppPaths::resolve_with_data_dir(PathBuf::from("/scratch/user/nix"));
        assert_eq!(paths.overlay_dir, PathBuf::from("/scratch/user/nix/overlay"));
    }

    #[test]
    fn test_resolve_with_data_dir() {
        let paths = AppPaths::resolve_with_data_dir(PathBuf::from("/scratch/user/nix"));
        assert_eq!(paths.sif_path, PathBuf::from("/scratch/user/nix/base.sif"));
        assert_eq!(paths.overlay_path, PathBuf::from("/scratch/user/nix/overlay.img"));
        assert_eq!(paths.config_file, PathBuf::from("/scratch/user/nix/config.toml"));
    }
}
