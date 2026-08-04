use anyhow::{Context, bail};

use crate::checks;
use crate::config::Config;
use crate::container::{ContainerMode, ContainerOpts, ContainerTarget, build_apptainer_args};
use crate::paths::AppPaths;
use crate::system::RealSystem;

pub struct ExecFlags {
    pub nv: bool,
    pub rocm: bool,
    pub bind: Vec<String>,
    pub passthrough: Vec<String>,
    pub command: Vec<String>,
    pub quiet: bool,
    pub force: bool,
}

pub fn run(flags: ExecFlags) -> anyhow::Result<()> {
    let sys = RealSystem;
    let paths = AppPaths::resolve()?;
    let config = Config::load(&paths.config_file)?;

    let storage = super::resolve_storage(&config, &paths)?;

    super::warn_if_overlay_full(&config, &paths);

    if flags.command.is_empty() {
        bail!("No command specified. Usage: nix-apptainer exec -- <command>");
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
    let mut args = build_apptainer_args(&opts, ContainerMode::Exec);
    args.extend(flags.command.iter().cloned());

    if let super::Storage::Sandbox(dir) = &storage {
        crate::mounts::ensure_mount_points(dir, &config.enter, &flags.bind);
        if let Some(lock) = crate::lock::acquire(&paths.sandbox_lock, flags.force)? {
            crate::lock::hold_across_exec(lock)?;
        }
    }

    let err = super::exec_replace(&apptainer, &args);
    Err(err.into())
}
