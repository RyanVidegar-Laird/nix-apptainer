use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::digest::Sha256Digest;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct State {
    /// Version tag of the currently installed SIF (e.g. "v0.1.0")
    #[serde(default)]
    pub sif_version: String,
    /// SHA256 digest of the installed SIF file
    #[serde(default)]
    pub sif_sha256: Sha256Digest,
    /// ISO 8601 timestamp of the last update check
    #[serde(default)]
    pub last_update_check: String,
}

impl State {
    /// Load state from JSON file. Returns default if file doesn't exist.
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read state: {}", path.display()))?;
        serde_json::from_str(&contents)
            .with_context(|| format!("Failed to parse state: {}", path.display()))
    }

    /// Save state to JSON file. Creates parent directories if needed.
    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create state directory: {}", parent.display()))?;
        }
        let contents = serde_json::to_string_pretty(self)
            .context("Failed to serialize state")?;
        std::fs::write(path, contents)
            .with_context(|| format!("Failed to write state: {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let state = State::default();
        assert_eq!(state.sif_version, "");
        assert_eq!(state.sif_sha256, Sha256Digest::default());
    }

    #[test]
    fn test_load_missing_file() {
        let state = State::load(Path::new("/nonexistent/state.json")).unwrap();
        assert_eq!(state.sif_version, "");
    }

    #[test]
    fn test_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let digest = Sha256Digest::from_hex(
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        )
        .unwrap();
        let state = State {
            sif_version: "v0.2.0".to_string(),
            sif_sha256: digest.clone(),
            last_update_check: "2026-03-07T12:00:00Z".to_string(),
        };
        state.save(&path).unwrap();
        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.sif_version, "v0.2.0");
        assert_eq!(loaded.sif_sha256, digest);
    }
}
