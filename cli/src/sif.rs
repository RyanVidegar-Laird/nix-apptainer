use anyhow::{bail, Context};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::Digest;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::digest::Sha256Digest;

/// Describes where to get the SIF from.
pub enum SifSource {
    /// Download from GitHub releases for owner/repo
    GitHub { repo: String },
    /// Download from a URL
    Url { url: String },
    /// Copy from a local file
    Local { path: String },
}

impl SifSource {
    /// Parse the sif.source config value into a SifSource.
    pub fn from_config(source: &str, repo: &str) -> Self {
        if source == "github" {
            SifSource::GitHub {
                repo: repo.to_string(),
            }
        } else if source.starts_with("http://") || source.starts_with("https://") {
            SifSource::Url {
                url: source.to_string(),
            }
        } else {
            SifSource::Local {
                path: source.to_string(),
            }
        }
    }
}

/// Info about a GitHub release.
pub struct ReleaseInfo {
    pub tag: String,
    pub sif_url: String,
    pub sha256_url: Option<String>,
}

/// Query the GitHub API for the latest release containing a .sif asset.
pub fn fetch_latest_release(repo: &str) -> anyhow::Result<ReleaseInfo> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let client = reqwest::blocking::Client::builder()
        .user_agent("nix-apptainer")
        .build()?;
    let resp: serde_json::Value = client
        .get(&url)
        .send()
        .context("Failed to query GitHub releases")?
        .error_for_status()
        .context("GitHub API returned an error")?
        .json()?;

    let tag = resp["tag_name"]
        .as_str()
        .context("No tag_name in release")?
        .to_string();

    let assets = resp["assets"]
        .as_array()
        .context("No assets in release")?;

    let sif_asset = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .is_some_and(|n| n.ends_with(".sif"))
        })
        .context("No .sif asset found in latest release")?;

    let sif_url = sif_asset["browser_download_url"]
        .as_str()
        .context("No download URL for SIF asset")?
        .to_string();

    let sha256_url = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .is_some_and(|n| n.ends_with(".sha256"))
        })
        .and_then(|a| a["browser_download_url"].as_str().map(|s| s.to_string()));

    Ok(ReleaseInfo {
        tag,
        sif_url,
        sha256_url,
    })
}

/// Download a file with a progress bar. Returns the SHA256 digest.
pub fn download_file(url: &str, dest: &Path) -> anyhow::Result<Sha256Digest> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("nix-apptainer")
        .build()?;
    let mut resp = client
        .get(url)
        .send()
        .with_context(|| format!("Failed to download: {url}"))?
        .error_for_status()
        .with_context(|| format!("Download failed: {url}"))?;

    let total_size = resp.content_length().unwrap_or(0);

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::with_template("  [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

    let mut file = fs::File::create(dest)
        .with_context(|| format!("Failed to create: {}", dest.display()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        pb.inc(n as u64);
    }
    pb.finish_and_clear();

    Ok(Sha256Digest::from_hasher(hasher))
}

/// Copy a local SIF file and compute its SHA256.
pub fn copy_local_sif(src: &str, dest: &Path) -> anyhow::Result<Sha256Digest> {
    let src_path = Path::new(src);
    if !src_path.exists() {
        bail!("SIF file not found: {src}");
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let total_size = fs::metadata(src_path)?.len();
    let pb = ProgressBar::new(total_size);
    pb.set_style(
        ProgressStyle::with_template("  [{bar:40.cyan/blue}] {bytes}/{total_bytes}")
            .unwrap()
            .progress_chars("##-"),
    );

    let mut src_file = fs::File::open(src_path)?;
    let mut dst_file = fs::File::create(dest)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = src_file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        dst_file.write_all(&buf[..n])?;
        hasher.update(&buf[..n]);
        pb.inc(n as u64);
    }
    pb.finish_and_clear();

    Ok(Sha256Digest::from_hasher(hasher))
}

/// Verify a SHA256 digest against an expected value (from .sha256 file content).
/// The .sha256 file typically contains "hash  filename\n".
pub fn verify_sha256(actual: &Sha256Digest, expected_content: &str) -> bool {
    let expected_hex = expected_content
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim();
    match Sha256Digest::from_hex(expected_hex) {
        Ok(expected) => *actual == expected,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sif_source_from_config() {
        match SifSource::from_config("github", "org/repo") {
            SifSource::GitHub { repo } => assert_eq!(repo, "org/repo"),
            _ => panic!("expected GitHub"),
        }
        match SifSource::from_config("https://example.com/image.sif", "") {
            SifSource::Url { url } => assert_eq!(url, "https://example.com/image.sif"),
            _ => panic!("expected Url"),
        }
        match SifSource::from_config("/data/shared/base.sif", "") {
            SifSource::Local { path } => assert_eq!(path, "/data/shared/base.sif"),
            _ => panic!("expected Local"),
        }
    }

    #[test]
    fn test_verify_sha256() {
        let digest = Sha256Digest::from_hex(
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        )
        .unwrap();
        assert!(verify_sha256(
            &digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  base.sif\n"
        ));
        assert!(verify_sha256(
            &digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        ));
        assert!(!verify_sha256(
            &digest,
            "0000000000000000000000000000000000000000000000000000000000000000  base.sif\n"
        ));
    }
}
