use crate::checks;
use crate::config::{Config, OverlayType};
use crate::paths::AppPaths;
use crate::state::State;
use crate::system::RealSystem;

pub fn run() -> anyhow::Result<()> {
    let sys = RealSystem;
    let paths = AppPaths::resolve()?;
    let config = Config::load(&paths.config_file)?;
    let state = State::load(&paths.state_file)?;

    // SIF info
    let sif_info = if paths.sif_path.exists() {
        let size = std::fs::metadata(&paths.sif_path)?.len();
        let size_str = crate::util::human_size(size);
        let version = if state.sif_version.is_empty() {
            "unknown".to_string()
        } else {
            state.sif_version.clone()
        };
        format!("{version} ({size_str})")
    } else {
        "not installed".to_string()
    };

    // Overlay info
    let overlay_info = match config.overlay.overlay_type {
        OverlayType::Directory => {
            if paths.overlay_dir.exists() {
                let size = dir_size(&paths.overlay_dir.join("upper"));
                format!("directory ({} used)", crate::util::human_size(size))
            } else {
                "directory (not created)".to_string()
            }
        }
        OverlayType::Ext3 => {
            if paths.overlay_path.exists() {
                let meta = std::fs::metadata(&paths.overlay_path)?;
                #[cfg(unix)]
                let on_disk = {
                    use std::os::unix::fs::MetadataExt;
                    meta.blocks() * 512
                };
                let allocated = meta.len();
                let capacity = config.overlay.ext3_size_mb * 1024 * 1024;
                format!(
                    "ext3 ({} on disk / {} allocated / {} capacity)",
                    crate::util::human_size(on_disk),
                    crate::util::human_size(allocated),
                    crate::util::human_size(capacity)
                )
            } else {
                "ext3 (not created)".to_string()
            }
        }
    };

    // Apptainer
    let apptainer_info = {
        let r = checks::find_apptainer(&sys);
        if r.passed { r.message } else { "not found".to_string() }
    };

    // GPU
    let gpu_info = match &config.enter.gpu {
        crate::config::GpuMode::None => "none".to_string(),
        crate::config::GpuMode::Nvidia => "nvidia".to_string(),
        crate::config::GpuMode::Rocm => "rocm".to_string(),
    };

    // Bind mounts
    let bind_info = if config.enter.bind.is_empty() {
        "none".to_string()
    } else {
        config.enter.bind.join(", ")
    };

    println!("Base image:  {sif_info}");
    println!("Overlay:     {overlay_info}");
    println!("Data dir:    {}", paths.data_dir.display());
    println!("Config:      {}", paths.config_file.display());
    println!("Apptainer:   {apptainer_info}");
    println!("GPU config:  {gpu_info}");
    println!("Bind mounts: {bind_info}");

    Ok(())
}

fn dir_size(path: &std::path::Path) -> u64 {
    if path.is_file() {
        return std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    }
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            total += dir_size(&entry.path());
        }
    }
    total
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_ext3_overlay_format() {
        let on_disk = 128 * 1024 * 1024u64;
        let allocated = 2 * 1024 * 1024 * 1024u64;
        let capacity = 50 * 1024 * 1024 * 1024u64;
        let result = format!(
            "ext3 ({} on disk / {} allocated / {} capacity)",
            crate::util::human_size(on_disk),
            crate::util::human_size(allocated),
            crate::util::human_size(capacity)
        );
        assert_eq!(result, "ext3 (128.0 MB on disk / 2.0 GB allocated / 50.0 GB capacity)");
    }
}
