use dialoguer::Confirm;

use crate::config::Config;
use crate::paths::AppPaths;
use crate::sif;
use crate::state::State;

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
