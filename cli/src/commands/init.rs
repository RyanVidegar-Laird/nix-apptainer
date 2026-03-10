use anyhow::Context;
use dialoguer::{Confirm, Input, Select};
use std::path::PathBuf;

use crate::checks;
use crate::config::Config;
use crate::overlay;
use crate::paths::AppPaths;
use crate::sif::{self, SifSource};
use crate::state::State;

/// CLI flags for non-interactive init.
pub struct InitFlags {
    pub sif: Option<String>,
    pub overlay_size: Option<u64>,
    pub data_dir: Option<PathBuf>,
    pub yes: bool,
}

pub fn run(flags: InitFlags) -> anyhow::Result<()> {
    println!("Checking system requirements...\n");

    // Determine data dir early so we can check disk space there
    let paths = if let Some(ref dir) = flags.data_dir {
        AppPaths::resolve_with_data_dir(dir.clone())
    } else {
        AppPaths::resolve()?
    };

    // --- System checks ---
    let report = checks::run_all_checks(&paths.data_dir);
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
    let disk_check = checks::check_disk_space(&paths.data_dir);
    println!("  Disk space: {}", disk_check.message);
    println!();

    // --- SIF source ---
    let sif_source = if let Some(ref sif) = flags.sif {
        SifSource::from_config(sif, "")
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
    let (version, hash) = match &sif_source {
        SifSource::GitHub { repo } => {
            println!("Fetching latest release from {repo}...");
            let release = sif::fetch_latest_release(repo)?;
            println!("  Found {} \u{2014} downloading...", release.tag);
            let hash = sif::download_file(&release.sif_url, &paths.sif_path)?;
            println!("  SHA256: {hash}");

            // Verify against .sha256 file if available
            if let Some(ref sha_url) = release.sha256_url {
                let expected = reqwest::blocking::Client::builder()
                    .user_agent("nix-apptainer")
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

            (release.tag, hash)
        }
        SifSource::Url { url } => {
            println!("Downloading SIF from {url}...");
            let hash = sif::download_file(url, &paths.sif_path)?;
            println!("  SHA256: {hash}");
            ("custom".to_string(), hash)
        }
        SifSource::Local { path } => {
            println!("Copying SIF from {path}...");
            let hash = sif::copy_local_sif(path, &paths.sif_path)?;
            println!("  SHA256: {hash}");
            ("local".to_string(), hash)
        }
    };

    // --- Overlay ---
    let overlay_size = if let Some(size) = flags.overlay_size {
        size
    } else if flags.yes {
        51200
    } else {
        let default_str = "51200";
        let size_str: String = Input::new()
            .with_prompt("Overlay size in MB (sparse \u{2014} actual disk usage starts small)")
            .default(default_str.to_string())
            .interact_text()?;
        size_str
            .parse::<u64>()
            .context("Invalid overlay size")?
    };

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
            println!("Creating overlay ({overlay_size} MB, sparse)...");
            overlay::create_overlay(&paths.overlay_path, overlay_size)?;
        } else {
            println!("Keeping existing overlay.");
        }
    } else {
        println!("Creating overlay ({overlay_size} MB, sparse)...");
        overlay::create_overlay(&paths.overlay_path, overlay_size)?;
    }

    // --- Initialize Nix DB ---
    println!("Initializing Nix store database...");
    overlay::init_nix_db(&paths.sif_path, &paths.overlay_path)?;

    // --- Save config ---
    let config_source = match &sif_source {
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
            size_mb: overlay_size,
        },
        enter: crate::config::EnterConfig::default(),
    };
    config.save(&paths.config_file)?;

    // --- Save state ---
    let state = State {
        sif_version: version,
        sif_sha256: hash,
        last_update_check: crate::util::timestamp_now(),
    };
    state.save(&paths.state_file)?;

    println!();
    println!("Setup complete! Run `nix-apptainer enter` to start.");

    Ok(())
}

