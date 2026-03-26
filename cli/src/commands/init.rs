use anyhow::Context;
use dialoguer::{Confirm, Input, Select};
use std::path::PathBuf;

use crate::checks;
use crate::config::{Config, OverlayType};
use crate::digest::Sha256Digest;
use crate::overlay;
use crate::paths::AppPaths;
use crate::sif::{self, SifSource};
use crate::state::State;
use crate::system::RealSystem;

/// CLI flags for non-interactive init.
pub struct InitFlags {
    pub sif: Option<String>,
    pub overlay_size: Option<u64>,
    pub overlay_type: Option<String>,
    pub data_dir: Option<PathBuf>,
    pub yes: bool,
}

/// Fetch or copy a SIF image based on the source configuration.
fn fetch_sif(source: &SifSource, paths: &AppPaths) -> anyhow::Result<(String, Sha256Digest)> {
    match source {
        SifSource::GitHub { repo } => {
            println!("Fetching latest release from {repo}...");
            let release = sif::fetch_latest_release(repo)?;
            println!("  Found {} \u{2014} downloading...", release.tag);
            let hash = sif::download_file(&release.sif_url, &paths.sif_path)?;
            println!("  SHA256: {hash}");

            if let Some(ref sha_url) = release.sha256_url {
                let expected = reqwest::blocking::Client::builder()
                    .user_agent("nix-apptainer")
                    .https_only(true)
                    .build()?
                    .get(sha_url)
                    .send()?
                    .text()?;
                if sif::verify_sha256(&hash, &expected, Some(&release.sif_asset_name)) {
                    println!("  SHA256 verified \u{2713}");
                } else {
                    anyhow::bail!("SHA256 mismatch! Expected: {expected}, Got: {hash}");
                }
            }

            Ok((release.tag, hash))
        }
        SifSource::Url { url } => {
            println!("Downloading SIF from {url}...");
            let hash = sif::download_file(url, &paths.sif_path)?;
            println!("  SHA256: {hash}");
            Ok(("custom".to_string(), hash))
        }
        SifSource::Local { path } => {
            println!("Copying SIF from {path}...");
            let hash = sif::copy_local_sif(path, &paths.sif_path)?;
            println!("  SHA256: {hash}");
            Ok(("local".to_string(), hash))
        }
    }
}

/// Save configuration and state after successful init.
fn save_init_state(
    paths: &AppPaths,
    sif_source: &SifSource,
    overlay_type: &OverlayType,
    ext3_size_mb: u64,
    version: &str,
    hash: Sha256Digest,
) -> anyhow::Result<()> {
    let config_source = match sif_source {
        SifSource::GitHub { repo } => ("github".to_string(), repo.clone()),
        SifSource::Url { url } => (url.clone(), String::new()),
        SifSource::Local { path } => (path.clone(), String::new()),
    };
    let config = Config {
        sif: crate::config::SifConfig {
            source: config_source.0,
            repo: config_source.1,
        },
        overlay: crate::config::OverlayConfig {
            overlay_type: overlay_type.clone(),
            ext3_size_mb,
        },
        enter: crate::config::EnterConfig::default(),
    };
    config.save(&paths.config_file)?;

    let mut state = State {
        sif_version: version.to_string(),
        sif_sha256: hash,
        ..State::default()
    };
    state.touch_update_check();
    state.save(&paths.state_file)?;
    Ok(())
}

pub fn run(flags: InitFlags) -> anyhow::Result<()> {
    let sys = RealSystem;
    println!("Checking system requirements...\n");

    // Determine data dir early so we can check disk space there
    let paths = if let Some(ref dir) = flags.data_dir {
        AppPaths::resolve_with_data_dir(dir.clone())
    } else {
        AppPaths::resolve()?
    };

    // --- System checks ---
    let report = checks::run_all_checks(&sys, &paths.data_dir);
    for r in &report.results {
        let icon = if r.passed {
            "\u{2713}"
        } else if r.required {
            "\u{2717}"
        } else {
            "!"
        };
        println!("  {icon} {}: {}", r.name, r.message);
    }
    println!();

    if report.any_required_failed {
        anyhow::bail!("Required system checks failed. Fix the issues above and try again.");
    }

    // --- Check for existing setup ---
    if (paths.config_file.exists() || paths.sif_path.exists()) && !flags.yes {
        let proceed = Confirm::new()
            .with_prompt("Existing configuration detected. Reconfigure?")
            .default(false)
            .interact()?;
        if !proceed {
            println!("Aborted.");
            return Ok(());
        }
    }

    // --- Data directory ---
    let paths = if flags.data_dir.is_some() || flags.yes {
        paths
    } else {
        let choices = vec![
            format!("Default ({})", paths.data_dir.display()),
            "Custom path".to_string(),
        ];
        let selection = Select::new()
            .with_prompt("Where should nix-apptainer store its data?")
            .items(&choices)
            .default(0)
            .interact()?;
        if selection == 1 {
            let custom: String = Input::new()
                .with_prompt("Enter path")
                .interact_text()?;
            AppPaths::resolve_with_data_dir(PathBuf::from(custom))
        } else {
            paths
        }
    };

    // Show disk space at chosen location
    let disk_check = checks::check_disk_space(&sys, &paths.data_dir);
    println!("  Disk space: {}", disk_check.message);
    println!();

    // --- SIF source ---
    let sif_source = if let Some(ref sif) = flags.sif {
        SifSource::from_config(sif, "")?
    } else if flags.yes {
        SifSource::GitHub {
            repo: "RyanVidegar-Laird/nix-apptainer".to_string(),
        }
    } else {
        let choices = vec![
            "Download latest from GitHub (recommended)",
            "Use a local SIF file",
            "Use a custom URL",
        ];
        let selection = Select::new()
            .with_prompt("How would you like to get the base image?")
            .items(&choices)
            .default(0)
            .interact()?;
        match selection {
            0 => SifSource::GitHub {
                repo: "RyanVidegar-Laird/nix-apptainer".to_string(),
            },
            1 => {
                let path: String = Input::new()
                    .with_prompt("Path to local SIF file")
                    .interact_text()?;
                SifSource::Local { path }
            }
            2 => {
                let url: String = Input::new()
                    .with_prompt("URL to SIF file")
                    .interact_text()?;
                SifSource::Url { url }
            }
            _ => unreachable!(),
        }
    };

    // --- Fetch SIF ---
    let (version, hash) = fetch_sif(&sif_source, &paths)?;

    // --- Overlay type ---
    let overlay_type = if let Some(ref t) = flags.overlay_type {
        match t.as_str() {
            "dir" | "directory" => OverlayType::Directory,
            "ext3" => OverlayType::Ext3,
            _ => anyhow::bail!("Invalid overlay type '{}'. Use 'dir' or 'ext3'.", t),
        }
    } else if flags.yes {
        OverlayType::Directory
    } else {
        let choices = vec![
            "Directory overlay (recommended \u{2014} best performance)",
            "ext3 image (sparse, fixed capacity)",
        ];
        let selection = Select::new()
            .with_prompt("Overlay type")
            .items(&choices)
            .default(0)
            .interact()?;
        match selection {
            0 => OverlayType::Directory,
            1 => OverlayType::Ext3,
            _ => unreachable!(),
        }
    };

    // --- Overlay ---
    let ext3_size_mb = match overlay_type {
        OverlayType::Ext3 => {
            if let Some(size) = flags.overlay_size {
                size
            } else if flags.yes {
                51200
            } else {
                let size_str: String = Input::new()
                    .with_prompt("ext3 overlay size in MB (sparse)")
                    .default("51200".to_string())
                    .interact_text()?;
                size_str.parse::<u64>().context("Invalid overlay size")?
            }
        }
        OverlayType::Directory => flags.overlay_size.unwrap_or(51200),
    };

    match overlay_type {
        OverlayType::Directory => {
            if paths.overlay_dir.exists() {
                let should_recreate = if flags.yes {
                    true
                } else {
                    Confirm::new()
                        .with_prompt("Directory overlay already exists. Overwrite? (destroys all installed packages)")
                        .default(false)
                        .interact()?
                };
                if should_recreate {
                    std::fs::remove_dir_all(&paths.overlay_dir)?;
                    println!("Creating directory overlay...");
                    overlay::create_directory_overlay(&paths.overlay_dir)?;
                } else {
                    println!("Keeping existing overlay.");
                }
            } else {
                println!("Creating directory overlay...");
                overlay::create_directory_overlay(&paths.overlay_dir)?;
            }
        }
        OverlayType::Ext3 => {
            if paths.overlay_path.exists() {
                let should_recreate = if flags.yes {
                    true
                } else {
                    Confirm::new()
                        .with_prompt("Overlay already exists. Overwrite? (destroys all installed packages)")
                        .default(false)
                        .interact()?
                };
                if should_recreate {
                    std::fs::remove_file(&paths.overlay_path)?;
                    println!("Creating ext3 overlay ({ext3_size_mb} MB, sparse)...");
                    overlay::create_overlay(&sys, &paths.overlay_path, ext3_size_mb)?;
                } else {
                    println!("Keeping existing overlay.");
                }
            } else {
                println!("Creating ext3 overlay ({ext3_size_mb} MB, sparse)...");
                overlay::create_overlay(&sys, &paths.overlay_path, ext3_size_mb)?;
            }
        }
    }

    // --- Pre-seed Nix DB ---
    let apptainer = checks::apptainer_binary(&sys)
        .context("apptainer/singularity not found")?;
    let overlay_str = match overlay_type {
        OverlayType::Directory => paths.overlay_dir.to_string_lossy().to_string(),
        OverlayType::Ext3 => paths.overlay_path.to_string_lossy().to_string(),
    };
    println!("Pre-seeding Nix store database...");
    overlay::preseed_nix_db(&sys, &apptainer, &overlay_str, &paths.sif_path.to_string_lossy())?;

    // --- Save config and state ---
    save_init_state(&paths, &sif_source, &overlay_type, ext3_size_mb, &version, hash)?;

    println!();
    println!("Setup complete! Run `nix-apptainer enter` to start.");

    Ok(())
}

