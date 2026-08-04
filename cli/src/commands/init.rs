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
fn fetch_sif(
    source: &SifSource,
    paths: &AppPaths,
) -> anyhow::Result<(String, Sha256Digest, Option<String>)> {
    match source {
        SifSource::GitHub { repo } => {
            println!("Fetching latest release from {repo}...");
            let release = sif::fetch_latest_release(repo)?;
            println!("  Found {} \u{2014} downloading...", release.tag);
            let hash = sif::download_and_verify(&release, &paths.sif_path)?;
            Ok((release.tag, hash, release.signing_key_url))
        }
        SifSource::Url { url } => {
            println!("Downloading SIF from {url}...");
            let hash = sif::download_file(url, &paths.sif_path)?;
            println!("  SHA256: {hash}");
            Ok(("custom".to_string(), hash, None))
        }
        SifSource::Local { path } => {
            println!("Copying SIF from {path}...");
            let hash = sif::copy_local_sif(path, &paths.sif_path)?;
            println!("  SHA256: {hash}");
            Ok(("local".to_string(), hash, None))
        }
    }
}

/// Announce the unpack, naming the size when the SIF carries the metadata.
fn announce_unpack(expected: Option<u64>) {
    match expected {
        Some(bytes) => println!(
            "Unpacking SIF into sandbox directory ({})...",
            crate::util::human_size(bytes)
        ),
        None => {
            println!("Unpacking SIF into sandbox directory (this can take several minutes)...")
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
    enter: crate::config::EnterConfig,
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
        enter,
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

    // --- Overlay type (needed before checks: they're mode-aware) ---
    let overlay_type = if let Some(ref t) = flags.overlay_type {
        match t.as_str() {
            "dir" | "directory" => OverlayType::Directory,
            "ext3" => OverlayType::Ext3,
            "sandbox" => OverlayType::Sandbox,
            _ => anyhow::bail!(
                "Invalid storage type '{}'. Use 'sandbox' (default), 'dir', or 'ext3'.",
                t
            ),
        }
    } else if flags.yes {
        OverlayType::Sandbox
    } else {
        let choices = vec![
            "Sandbox directory (recommended \u{2014} no overlay/FUSE overhead, enables the Nix build sandbox; needs ~10 GB)",
            "Directory overlay (smaller footprint \u{2014} ~2 GB; needs working fuse-overlayfs)",
            "ext3 image (single file \u{2014} gentler on inode-constrained parallel filesystems)",
        ];
        let selection = Select::new()
            .with_prompt("Storage type")
            .items(&choices)
            .default(0)
            .interact()?;
        match selection {
            0 => OverlayType::Sandbox,
            1 => OverlayType::Directory,
            2 => OverlayType::Ext3,
            _ => unreachable!(),
        }
    };

    // --- System checks ---
    let report = checks::run_all_checks(&sys, &paths.data_dir, &overlay_type);
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
            let custom: String = Input::new().with_prompt("Enter path").interact_text()?;
            AppPaths::resolve_with_data_dir(PathBuf::from(custom))
        } else {
            paths
        }
    };

    // Show disk space at chosen location
    let min_gb = if overlay_type == OverlayType::Sandbox {
        10.0
    } else {
        2.0
    };
    let disk_check = checks::check_disk_space(&sys, &paths.data_dir, min_gb);
    println!("  Disk space: {}", disk_check.message);
    println!();

    // --- Container TMPDIR ---
    // Prepopulated from the host's $TMPDIR so cluster users see the value
    // they actually have. Keeping it only works if that path is visible
    // inside the container, so the alternative is spelled out here rather
    // than discovered later as a mid-build nix-build failure.
    let tmpdir = if flags.yes {
        String::new()
    } else {
        let host_tmpdir = std::env::var("TMPDIR").unwrap_or_default();
        let inherit_label = if host_tmpdir.is_empty() {
            "Inherit from the host (currently unset)".to_string()
        } else {
            format!("Inherit from the host (currently {host_tmpdir})")
        };
        println!("TMPDIR must name a path the container can see: nix-build creates a");
        println!("scratch directory there before any build starts. A host scratch path");
        println!("needs a matching `bind` (and, in sandbox mode, a `mount_points` entry).");
        let choices = vec![
            inherit_label,
            "/tmp inside the container (always present)".to_string(),
            "Custom path".to_string(),
        ];
        let selection = Select::new()
            .with_prompt("TMPDIR inside the container")
            .items(&choices)
            .default(0)
            .interact()?;
        match selection {
            0 => String::new(),
            1 => "/tmp".to_string(),
            2 => {
                let custom: String = Input::new()
                    .with_prompt("Enter TMPDIR path")
                    .with_initial_text(if host_tmpdir.is_empty() {
                        "/tmp".to_string()
                    } else {
                        host_tmpdir
                    })
                    .interact_text()?;
                custom.trim().to_string()
            }
            _ => unreachable!(),
        }
    };
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
    let (version, hash, signing_key_url) = fetch_sif(&sif_source, &paths)?;

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
        OverlayType::Directory | OverlayType::Sandbox => flags.overlay_size.unwrap_or(51200),
    };

    let apptainer = checks::apptainer_binary(&sys).context("apptainer/singularity not found")?;

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
                    crate::util::make_writable_recursive(&paths.overlay_dir);
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
                        .with_prompt(
                            "Overlay already exists. Overwrite? (destroys all installed packages)",
                        )
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
        OverlayType::Sandbox => {
            // Before any unpack: without the key in the local keyring,
            // `apptainer build --sandbox` 404s against a keyserver and warns
            // that the image could not be verified.
            if let Some(ref key_url) = signing_key_url {
                println!("Importing image signing key...");
                if let Err(e) =
                    crate::sif::import_signing_key(&sys, &apptainer, key_url, &paths.cache_dir)
                {
                    eprintln!("  Warning: {e}");
                    eprintln!("  The unpack will show a benign verification warning.");
                }
            }
            if paths.sandbox_dir.exists() {
                let should_recreate = if flags.yes {
                    true
                } else {
                    Confirm::new()
                        .with_prompt(
                            "Sandbox directory already exists. Recreate? (destroys all local changes)",
                        )
                        .default(false)
                        .interact()?
                };
                if !should_recreate {
                    println!("Keeping existing sandbox.");
                } else {
                    let expected = crate::sifmeta::read_unpacked_bytes(&paths.sif_path);
                    announce_unpack(expected);
                    crate::sandbox::create_sandbox(
                        &sys,
                        &apptainer,
                        &paths.sif_path,
                        &paths.sandbox_dir,
                        expected,
                    )?;
                }
            } else {
                let expected = crate::sifmeta::read_unpacked_bytes(&paths.sif_path);
                announce_unpack(expected);
                crate::sandbox::create_sandbox(
                    &sys,
                    &apptainer,
                    &paths.sif_path,
                    &paths.sandbox_dir,
                    expected,
                )?;
            }
        }
    }

    // --- Pre-seed Nix DB (overlay modes only) ---
    let mut mount_points: Vec<String> = Vec::new();
    match overlay_type {
        OverlayType::Directory | OverlayType::Ext3 => {
            let overlay_str = match overlay_type {
                OverlayType::Directory => paths.overlay_dir.to_string_lossy().to_string(),
                OverlayType::Ext3 => paths.overlay_path.to_string_lossy().to_string(),
                OverlayType::Sandbox => unreachable!(),
            };
            println!("Pre-seeding Nix store database...");
            overlay::preseed_nix_db(
                &sys,
                &apptainer,
                &overlay_str,
                &paths.sif_path.to_string_lossy(),
            )?;
        }
        OverlayType::Sandbox => {
            // DB is baked into the image and now on a plain filesystem — no
            // pre-seed needed. Discovery and seeding come before the probe:
            // the probe must see the environment user builds will see.
            let discovered =
                crate::mounts::discover_missing_mount_points(&sys, &apptainer, &paths.sandbox_dir);
            mount_points = if discovered.is_empty() {
                Vec::new()
            } else if flags.yes {
                println!("Creating mount points for site-configured binds:");
                for p in &discovered {
                    println!("  {p}");
                }
                discovered
            } else {
                println!("The site apptainer config mounts paths that don't exist in the image;");
                println!("selected paths are created in the sandbox so those mounts succeed:");
                let defaults = vec![true; discovered.len()];
                let picks = dialoguer::MultiSelect::new()
                    .with_prompt("Mount points to create (space to toggle)")
                    .items(&discovered)
                    .defaults(&defaults)
                    .interact()?;
                picks.into_iter().map(|i| discovered[i].clone()).collect()
            };
            crate::mounts::seed_paths(&paths.sandbox_dir, &mount_points);

            println!("Probing Nix build sandbox support...");
            let outcome =
                crate::sandbox::probe_and_enable_nix_sandbox(&sys, &apptainer, &paths.sandbox_dir)?;
            if outcome.enabled {
                println!("  Works \u{2014} enabled (sandbox = true) for this installation.");
            } else {
                println!("  Unavailable \u{2014} builds will run unsandboxed (sandbox = false).");
                if let Some(detail) = outcome.detail {
                    println!("  Probe output (failure can be environmental; a re-run of");
                    println!("  `nix-apptainer init` keeping the existing sandbox re-probes):");
                    for line in detail.lines() {
                        println!("    {line}");
                    }
                }
            }
        }
    }

    // --- Save config and state ---
    save_init_state(
        &paths,
        &sif_source,
        &overlay_type,
        ext3_size_mb,
        &version,
        hash,
        crate::config::EnterConfig {
            tmpdir,
            mount_points,
            ..crate::config::EnterConfig::default()
        },
    )?;

    println!();
    println!("Setup complete! Run `nix-apptainer enter` to start.");

    Ok(())
}
