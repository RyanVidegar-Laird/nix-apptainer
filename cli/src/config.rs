use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub sif: SifConfig,
    #[serde(default)]
    pub overlay: OverlayConfig,
    #[serde(default)]
    pub enter: EnterConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SifConfig {
    /// "github", a URL, or a local file path
    #[serde(default = "default_source")]
    pub source: String,
    /// GitHub repo in "owner/name" format
    #[serde(default = "default_repo")]
    pub repo: String,
}

fn default_source() -> String {
    "github".to_string()
}

fn default_repo() -> String {
    "RyanVidegar-Laird/nix-apptainer".to_string()
}

impl Default for SifConfig {
    fn default() -> Self {
        Self {
            source: default_source(),
            repo: default_repo(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum OverlayType {
    #[default]
    Directory,
    Ext3,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OverlayConfig {
    #[serde(default, rename = "type")]
    pub overlay_type: OverlayType,
    /// ext3 overlay size in megabytes (sparse). Only used when type = "ext3".
    #[serde(default = "default_ext3_size", alias = "size_mb")]
    pub ext3_size_mb: u64,
}

fn default_ext3_size() -> u64 {
    51200
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            overlay_type: OverlayType::default(),
            ext3_size_mb: default_ext3_size(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default, PartialEq, Eq, Clone)]
#[serde(rename_all = "lowercase")]
pub enum GpuMode {
    #[default]
    #[serde(rename = "")]
    None,
    Nvidia,
    Rocm,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct EnterConfig {
    /// GPU passthrough mode
    #[serde(default)]
    pub gpu: GpuMode,
    /// Bind mounts in "src:dst" format
    #[serde(default)]
    pub bind: Vec<String>,
    /// Suppress apptainer stderr warnings
    #[serde(default)]
    pub quiet: bool,
}

impl Config {
    /// Load config from a TOML file. Returns default config if file doesn't exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config: {}", path.display()))?;
        toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config: {}", path.display()))
    }

    /// Save config to a TOML file. Creates parent directories if needed.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
        }
        let contents = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;
        std::fs::write(path, contents)
            .with_context(|| format!("Failed to write config: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{GpuMode, OverlayType};
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.sif.source, "github");
        assert_eq!(config.sif.repo, "RyanVidegar-Laird/nix-apptainer");
        assert_eq!(config.overlay.ext3_size_mb, 51200);
        assert_eq!(config.enter.gpu, GpuMode::None);
        assert!(config.enter.bind.is_empty());
    }

    #[test]
    fn test_load_missing_file() {
        let config = Config::load(Path::new("/nonexistent/config.toml")).unwrap();
        assert_eq!(config.sif.source, "github");
    }

    #[test]
    fn test_quiet_default_false() {
        let config = Config::default();
        assert!(!config.enter.quiet);
    }

    #[test]
    fn test_quiet_from_toml() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, r#"
[enter]
quiet = true
"#).unwrap();
        let config = Config::load(f.path()).unwrap();
        assert!(config.enter.quiet);
    }

    #[test]
    fn test_overlay_type_default_directory() {
        let config = Config::default();
        assert_eq!(config.overlay.overlay_type, OverlayType::Directory);
    }

    #[test]
    fn test_overlay_type_ext3_from_toml() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, r#"
[overlay]
type = "ext3"
ext3_size_mb = 20480
"#).unwrap();
        let config = Config::load(f.path()).unwrap();
        assert_eq!(config.overlay.overlay_type, OverlayType::Ext3);
        assert_eq!(config.overlay.ext3_size_mb, 20480);
    }

    #[test]
    fn test_overlay_size_mb_alias() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, r#"
[overlay]
size_mb = 30000
"#).unwrap();
        let config = Config::load(f.path()).unwrap();
        assert_eq!(config.overlay.ext3_size_mb, 30000);
    }

    #[test]
    fn test_overlay_type_directory_from_toml() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, r#"
[overlay]
type = "directory"
"#).unwrap();
        let config = Config::load(f.path()).unwrap();
        assert_eq!(config.overlay.overlay_type, OverlayType::Directory);
    }

    #[test]
    fn test_roundtrip() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, r#"
[sif]
source = "/data/shared/base.sif"
repo = "myorg/nix-apptainer"

[overlay]
size_mb = 20480

[enter]
gpu = "nvidia"
bind = ["/scratch:/scratch", "/data:/data"]
"#).unwrap();
        let config = Config::load(f.path()).unwrap();
        assert_eq!(config.sif.source, "/data/shared/base.sif");
        assert_eq!(config.overlay.ext3_size_mb, 20480);
        assert_eq!(config.enter.gpu, GpuMode::Nvidia);
        assert_eq!(config.enter.bind.len(), 2);
    }
}
