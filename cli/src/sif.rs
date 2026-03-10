use anyhow::{bail, Context};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::Digest;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use crate::digest::Sha256Digest;

/// Describes where to get the SIF from.
#[derive(Debug)]
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
    pub fn from_config(source: &str, repo: &str) -> anyhow::Result<Self> {
        if source == "github" {
            Ok(SifSource::GitHub {
                repo: repo.to_string(),
            })
        } else if source.starts_with("https://") {
            Ok(SifSource::Url {
                url: source.to_string(),
            })
        } else if source.starts_with("http://") {
            anyhow::bail!(
                "HTTP downloads are not supported for security reasons. Use HTTPS: {}",
                source.replace("http://", "https://")
            )
        } else {
            Ok(SifSource::Local {
                path: source.to_string(),
            })
        }
    }
}

/// Info about a GitHub release.
pub struct ReleaseInfo {
    pub tag: String,
    pub sif_url: String,
    pub sif_asset_name: String,
    pub sha256_url: Option<String>,
}

/// Return the Nix-style platform string for the current system (e.g. "x86_64-linux").
pub fn current_arch() -> String {
    let arch = std::env::consts::ARCH;
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    format!("{arch}-{os}")
}

/// Parse a GitHub release JSON response into a ReleaseInfo.
pub fn parse_release_response(resp: &serde_json::Value) -> anyhow::Result<ReleaseInfo> {
    let tag = resp["tag_name"]
        .as_str()
        .context("No tag_name in release")?
        .to_string();

    let assets = resp["assets"]
        .as_array()
        .context("No assets in release")?;

    let arch = current_arch();
    let expected_sif_name = format!("base-nixos-{arch}.sif");

    let sif_asset = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .is_some_and(|n| n == expected_sif_name)
        })
        .with_context(|| format!("No SIF asset for {arch} in latest release"))?;

    let sif_asset_name = sif_asset["name"]
        .as_str()
        .context("SIF asset missing 'name' field")?
        .to_string();

    let sif_url = sif_asset["browser_download_url"]
        .as_str()
        .context("No download URL for SIF asset")?
        .to_string();

    let expected_sha_name = format!("SHA256SUMS-{arch}");
    let sha256_url = assets
        .iter()
        .find(|a| {
            a["name"]
                .as_str()
                .is_some_and(|n| n == expected_sha_name)
        })
        .and_then(|a| a["browser_download_url"].as_str().map(|s| s.to_string()));

    Ok(ReleaseInfo {
        tag,
        sif_url,
        sif_asset_name,
        sha256_url,
    })
}

/// Query the GitHub API for the latest release containing a .sif asset.
pub fn fetch_latest_release(repo: &str) -> anyhow::Result<ReleaseInfo> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let client = reqwest::blocking::Client::builder()
        .user_agent("nix-apptainer")
        .https_only(true)
        .build()?;
    let resp: serde_json::Value = client
        .get(&url)
        .send()
        .context("Failed to query GitHub releases")?
        .error_for_status()
        .context("GitHub API returned an error")?
        .json()?;

    parse_release_response(&resp)
}

/// Download a file with a progress bar. Returns the SHA256 digest.
pub fn download_file(url: &str, dest: &Path) -> anyhow::Result<Sha256Digest> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("nix-apptainer")
        .https_only(true)
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
            .expect("hardcoded progress bar template")
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
            .expect("hardcoded progress bar template")
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

/// Verify a SHA256 digest against a SHA256SUMS file.
/// The file contains lines of "hash  filename". If `sif_filename` is provided,
/// only the line matching that filename is used. Otherwise the first line is used.
pub fn verify_sha256(actual: &Sha256Digest, expected_content: &str, sif_filename: Option<&str>) -> bool {
    let line = if let Some(filename) = sif_filename {
        expected_content
            .lines()
            .find(|l| l.contains(filename))
    } else {
        expected_content.lines().next()
    };

    let expected_hex = line
        .unwrap_or("")
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
        match SifSource::from_config("github", "org/repo").unwrap() {
            SifSource::GitHub { repo } => assert_eq!(repo, "org/repo"),
            _ => panic!("expected GitHub"),
        }
        match SifSource::from_config("https://example.com/image.sif", "").unwrap() {
            SifSource::Url { url } => assert_eq!(url, "https://example.com/image.sif"),
            _ => panic!("expected Url"),
        }
        match SifSource::from_config("/data/shared/base.sif", "").unwrap() {
            SifSource::Local { path } => assert_eq!(path, "/data/shared/base.sif"),
            _ => panic!("expected Local"),
        }
    }

    #[test]
    fn test_sif_source_rejects_http() {
        let result = SifSource::from_config("http://example.com/image.sif", "");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("HTTP downloads are not supported"));
    }

    #[test]
    fn test_verify_sha256() {
        let digest = Sha256Digest::from_hex(
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        )
        .unwrap();
        // Single-line format with filename
        assert!(verify_sha256(
            &digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  base.sif\n",
            None,
        ));
        // Hash only
        assert!(verify_sha256(
            &digest,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
            None,
        ));
        // Mismatch
        assert!(!verify_sha256(
            &digest,
            "0000000000000000000000000000000000000000000000000000000000000000  base.sif\n",
            None,
        ));
    }

    #[test]
    fn test_verify_sha256_multiline() {
        let digest = Sha256Digest::from_hex(
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824",
        )
        .unwrap();
        let sums = "\
0000000000000000000000000000000000000000000000000000000000000000  base-nixos-aarch64-linux.sif
2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  base-nixos-x86_64-linux.sif
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  nix-apptainer-x86_64-linux
";
        // Should find the correct line by filename
        assert!(verify_sha256(
            &digest,
            sums,
            Some("base-nixos-x86_64-linux.sif"),
        ));
        // Wrong arch should not match
        assert!(!verify_sha256(
            &digest,
            sums,
            Some("base-nixos-aarch64-linux.sif"),
        ));
        // Missing filename returns false
        assert!(!verify_sha256(
            &digest,
            sums,
            Some("nonexistent.sif"),
        ));
    }

    #[test]
    fn test_current_arch() {
        let arch = current_arch();
        assert!(
            arch.contains('-'),
            "current_arch() should return 'arch-os' format, got: {arch}"
        );
        // Should be a known platform
        let valid = [
            "x86_64-linux",
            "aarch64-linux",
            "x86_64-darwin",
            "aarch64-darwin",
        ];
        assert!(
            valid.contains(&arch.as_str()),
            "unexpected platform: {arch}"
        );
    }

    #[test]
    fn test_parse_release_response_valid() {
        let json = serde_json::json!({
            "tag_name": "v0.2.0",
            "assets": [
                {
                    "name": format!("base-nixos-{}.sif", current_arch()),
                    "browser_download_url": "https://example.com/base.sif"
                },
                {
                    "name": format!("SHA256SUMS-{}", current_arch()),
                    "browser_download_url": "https://example.com/SHA256SUMS"
                }
            ]
        });
        let info = parse_release_response(&json).unwrap();
        assert_eq!(info.tag, "v0.2.0");
        assert_eq!(info.sif_url, "https://example.com/base.sif");
        assert!(info.sha256_url.is_some());
    }

    #[test]
    fn test_parse_release_response_missing_arch() {
        let json = serde_json::json!({
            "tag_name": "v0.2.0",
            "assets": [
                {
                    "name": "base-nixos-aarch64-unknown.sif",
                    "browser_download_url": "https://example.com/wrong.sif"
                }
            ]
        });
        let result = parse_release_response(&json);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_release_response_no_sha256() {
        let json = serde_json::json!({
            "tag_name": "v0.1.0",
            "assets": [
                {
                    "name": format!("base-nixos-{}.sif", current_arch()),
                    "browser_download_url": "https://example.com/base.sif"
                }
            ]
        });
        let info = parse_release_response(&json).unwrap();
        assert!(info.sha256_url.is_none());
    }

    #[test]
    fn test_parse_release_response_no_tag() {
        let json = serde_json::json!({ "assets": [] });
        assert!(parse_release_response(&json).is_err());
    }

    #[test]
    fn test_parse_release_response_no_assets() {
        let json = serde_json::json!({ "tag_name": "v1.0" });
        assert!(parse_release_response(&json).is_err());
    }
}
