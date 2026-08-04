use anyhow::Context;
use dialoguer::Confirm;

use crate::checks;
use crate::config::{Config, OverlayType};
use crate::paths::AppPaths;
use crate::sandbox;
use crate::sif;
use crate::state::State;
use crate::system::RealSystem;

pub struct UpdateFlags {
    pub check: bool,
    pub yes: bool,
}

pub fn run(flags: UpdateFlags) -> anyhow::Result<()> {
    let paths = AppPaths::resolve()?;
    let config = Config::load(&paths.config_file)?;
    let state = State::load(&paths.state_file)?;

    if config.sif.source != "github" {
        anyhow::bail!(
            "Update only works with GitHub source. Current source: {}",
            config.sif.source
        );
    }

    println!("Checking for updates from {}...", config.sif.repo);
    let release = sif::fetch_latest_release(&config.sif.repo)?;

    let current = if state.sif_version.is_empty() {
        "none".to_string()
    } else {
        state.sif_version.clone()
    };

    if release.tag == current {
        println!("Already up to date ({current}).");
        return Ok(());
    }

    println!("  Current: {current}");
    println!("  Available: {}", release.tag);

    if flags.check {
        println!("\nUpdate available. Run `nix-apptainer update` to download.");
        return Ok(());
    }

    if !flags.yes {
        let proceed = Confirm::new()
            .with_prompt("Download update?")
            .default(true)
            .interact()?;
        if !proceed {
            println!("Aborted.");
            return Ok(());
        }
    }

    let is_sandbox = config.overlay.overlay_type == OverlayType::Sandbox;
    if is_sandbox {
        println!();
        println!("Sandbox mode: updating re-unpacks the base image into a fresh directory.");
        println!("This DISCARDS all local changes — packages you built or installed inside");
        println!("the container are lost unless pushed to a cache first.");
        if !flags.yes {
            let proceed = Confirm::new()
                .with_prompt("Discard local changes and update?")
                .default(false)
                .interact()?;
            if !proceed {
                println!("Aborted.");
                return Ok(());
            }
        }
    }

    println!("Downloading {}...", release.tag);
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

    if is_sandbox {
        let sys = RealSystem;
        let apptainer =
            checks::apptainer_binary(&sys).context("apptainer/singularity not found")?;
        if let Some(ref key_url) = release.signing_key_url {
            println!("Importing image signing key...");
            if let Err(e) = sif::import_signing_key(&sys, &apptainer, key_url, &paths.cache_dir) {
                eprintln!("  Warning: {e}");
                eprintln!("  The unpack will show a benign verification warning.");
            }
        }
        let expected = crate::sifmeta::read_unpacked_bytes(&paths.sif_path);
        match expected {
            Some(bytes) => println!(
                "Re-unpacking sandbox directory ({})...",
                crate::util::human_size(bytes)
            ),
            None => println!("Re-unpacking sandbox directory (this can take several minutes)..."),
        }
        sandbox::create_sandbox(
            &sys,
            &apptainer,
            &paths.sif_path,
            &paths.sandbox_dir,
            expected,
        )?;
        // Re-unpack wipes the tree, so the configured mount points have to be
        // recreated. No re-discovery here — config.toml is the declarative record.
        crate::mounts::ensure_mount_points(&paths.sandbox_dir, &config.enter, &[]);
        println!("Probing Nix build sandbox support...");
        let outcome = sandbox::probe_and_enable_nix_sandbox(&sys, &apptainer, &paths.sandbox_dir)?;
        if outcome.enabled {
            println!("  Works \u{2014} enabled (sandbox = true).");
        } else {
            println!("  Unavailable \u{2014} builds will run unsandboxed (sandbox = false).");
            if let Some(detail) = outcome.detail {
                println!("  Probe output (failure can be environmental):");
                for line in detail.lines() {
                    println!("    {line}");
                }
            }
        }
    }

    let mut new_state = State {
        sif_version: release.tag.clone(),
        sif_sha256: hash,
        ..State::default()
    };
    new_state.touch_update_check();
    new_state.save(&paths.state_file)?;

    println!("\nUpdated to {}.", release.tag);
    Ok(())
}
