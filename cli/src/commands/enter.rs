use anyhow::Context;
use std::process::Command;

use crate::checks;
use crate::config::{Config, OverlayType};
use crate::container::{ContainerMode, ContainerOpts, ContainerTarget, build_apptainer_args};
use crate::paths::AppPaths;
use crate::system::RealSystem;

pub struct EnterFlags {
    pub nv: bool,
    pub rocm: bool,
    pub bind: Vec<String>,
    pub passthrough: Vec<String>,
    pub quiet: bool,
    pub force: bool,
}

pub fn run(flags: EnterFlags) -> anyhow::Result<()> {
    let sys = RealSystem;
    let paths = AppPaths::resolve()?;
    let config = Config::load(&paths.config_file)?;

    let storage = super::resolve_storage(&config, &paths)?;

    // Warn if ext3 overlay is getting full
    #[cfg(unix)]
    if config.overlay.overlay_type == OverlayType::Ext3 {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(&paths.overlay_path) {
            let on_disk = meta.blocks() * 512;
            let allocated = meta.len();
            if let Some(warning) = crate::util::overlay_usage_warning(on_disk, allocated, 80) {
                eprintln!("{warning}");
            }
        }
    }

    let apptainer = checks::apptainer_binary(&sys).context("apptainer/singularity not found")?;
    let target = match &storage {
        super::Storage::Overlay(overlay) => ContainerTarget::Overlay {
            sif: &paths.sif_path,
            overlay,
        },
        super::Storage::Sandbox(dir) => ContainerTarget::Sandbox { dir },
    };
    let opts = ContainerOpts {
        target,
        config: &config,
        nv: flags.nv,
        rocm: flags.rocm,
        bind: &flags.bind,
        passthrough: &flags.passthrough,
        quiet: flags.quiet || config.enter.quiet,
    };
    let args = build_apptainer_args(&opts, ContainerMode::Run);

    if let super::Storage::Sandbox(_) = &storage
        && let Some(lock) = crate::lock::acquire(&paths.sandbox_lock, flags.force)?
    {
        crate::lock::hold_across_exec(lock)?;
    }

    let err = exec_replace(&apptainer, &args);
    Err(err.into())
}

fn exec_replace(program: &str, args: &[String]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    Command::new(program).args(args).exec()
}
