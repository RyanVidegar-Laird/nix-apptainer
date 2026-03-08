use anyhow::Context;
use dialoguer::{Confirm, MultiSelect};
use std::fs;
use std::path::Path;

use crate::paths::AppPaths;

pub struct CleanFlags {
    pub all: bool,
    pub cache: bool,
    pub overlay: bool,
}

pub fn run(flags: CleanFlags) -> anyhow::Result<()> {
    let paths = AppPaths::resolve()?;

    if flags.all {
        remove_with_label("Download cache", &paths.cache_dir)?;
        remove_with_label("Overlay", &paths.overlay_path)?;
        remove_with_label("Base SIF", &paths.sif_path)?;
        remove_with_label("State", &paths.state_file)?;
        remove_with_label("Config", &paths.config_file)?;
        // Try to remove the now-empty directories
        let _ = fs::remove_dir(&paths.data_dir);
        if let Some(parent) = paths.config_file.parent() {
            let _ = fs::remove_dir(parent);
        }
        println!("\nAll nix-apptainer data removed.");
        return Ok(());
    }

    if flags.cache {
        remove_with_label("Download cache", &paths.cache_dir)?;
        return Ok(());
    }

    if flags.overlay {
        if paths.overlay_path.exists() {
            let proceed = Confirm::new()
                .with_prompt(
                    "Remove overlay? This destroys all packages installed inside the container",
                )
                .default(false)
                .interact()?;
            if proceed {
                remove_with_label("Overlay", &paths.overlay_path)?;
            } else {
                println!("Aborted.");
            }
        } else {
            println!("No overlay found.");
        }
        return Ok(());
    }

    // Interactive mode: show checklist
    let mut items: Vec<(String, &Path)> = Vec::new();

    if paths.cache_dir.exists() {
        let size = dir_size(&paths.cache_dir);
        items.push((
            format!("Download cache (~{})", human_size(size)),
            &paths.cache_dir,
        ));
    }
    if paths.overlay_path.exists() {
        let size = fs::metadata(&paths.overlay_path)
            .map(|m| m.len())
            .unwrap_or(0);
        items.push((
            format!("Overlay image (~{})", human_size(size)),
            &paths.overlay_path,
        ));
    }
    if paths.sif_path.exists() {
        let size = fs::metadata(&paths.sif_path)
            .map(|m| m.len())
            .unwrap_or(0);
        items.push((
            format!("Base SIF (~{})", human_size(size)),
            &paths.sif_path,
        ));
    }
    if paths.config_file.exists() || paths.state_file.exists() {
        items.push(("Config + state".to_string(), &paths.config_file));
    }

    if items.is_empty() {
        println!("Nothing to clean.");
        return Ok(());
    }

    let labels: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
    let selections = MultiSelect::new()
        .with_prompt("What should be removed?")
        .items(&labels)
        .interact()?;

    if selections.is_empty() {
        println!("Nothing selected.");
        return Ok(());
    }

    // Warn if overlay is selected
    let has_overlay = selections.iter().any(|&i| items[i].0.contains("Overlay"));
    if has_overlay {
        println!(
            "\n\u{26a0} Removing the overlay will destroy all packages installed inside the container."
        );
        let proceed = Confirm::new()
            .with_prompt("Proceed?")
            .default(false)
            .interact()?;
        if !proceed {
            println!("Aborted.");
            return Ok(());
        }
    }

    for &i in &selections {
        let (ref label, path) = items[i];
        if label.contains("Config") {
            remove_with_label("Config", &paths.config_file)?;
            remove_with_label("State", &paths.state_file)?;
        } else {
            remove_with_label(label, path)?;
        }
    }

    // Clean up empty directories
    let _ = fs::remove_dir(&paths.data_dir);
    if let Some(parent) = paths.config_file.parent() {
        let _ = fs::remove_dir(parent);
    }

    println!("\nDone.");
    Ok(())
}

fn remove_with_label(label: &str, path: &Path) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        fs::remove_dir_all(path)
            .with_context(|| format!("Failed to remove {label}: {}", path.display()))?;
    } else {
        fs::remove_file(path)
            .with_context(|| format!("Failed to remove {label}: {}", path.display()))?;
    }
    println!("  Removed: {label} ({})", path.display());
    Ok(())
}

fn dir_size(path: &Path) -> u64 {
    if path.is_file() {
        return fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            total += dir_size(&entry.path());
        }
    }
    total
}

fn human_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;
    const KB: u64 = 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
