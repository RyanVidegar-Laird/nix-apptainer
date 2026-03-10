use crate::checks;
use crate::config::Config;
use crate::paths::AppPaths;
use crate::state::State;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

pub fn run() -> anyhow::Result<()> {
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
    let overlay_info = if paths.overlay_path.exists() {
        let meta = std::fs::metadata(&paths.overlay_path)?;
        let on_disk = meta.blocks() * 512;
        let allocated = meta.len();
        let capacity = config.overlay.size_mb * 1024 * 1024;
        format!(
            "{} on disk / {} allocated / {} capacity",
            crate::util::human_size(on_disk),
            crate::util::human_size(allocated),
            crate::util::human_size(capacity)
        )
    } else {
        "not created".to_string()
    };

    // Apptainer
    let apptainer_info = {
        let r = checks::find_apptainer();
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

#[cfg(test)]
mod tests {
    #[test]
    fn test_overlay_format() {
        let on_disk = 128 * 1024 * 1024u64;
        let allocated = 2 * 1024 * 1024 * 1024u64;
        let capacity = 50 * 1024 * 1024 * 1024u64;
        let result = format!(
            "{} on disk / {} allocated / {} capacity",
            crate::util::human_size(on_disk),
            crate::util::human_size(allocated),
            crate::util::human_size(capacity)
        );
        assert_eq!(result, "128.0 MB on disk / 2.0 GB allocated / 50.0 GB capacity");
    }
}
