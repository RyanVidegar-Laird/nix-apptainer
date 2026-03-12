use anyhow::{bail, Context};
use std::process::Command;

use crate::checks;
use crate::config::Config;
use crate::container::{build_apptainer_args, ContainerMode, ContainerOpts};
use crate::paths::AppPaths;
use crate::system::RealSystem;

pub struct ExecFlags {
    pub nv: bool,
    pub rocm: bool,
    pub bind: Vec<String>,
    pub passthrough: Vec<String>,
    pub command: Vec<String>,
    pub quiet: bool,
}

pub fn run(flags: ExecFlags) -> anyhow::Result<()> {
    let sys = RealSystem;
    let paths = AppPaths::resolve()?;
    let config = Config::load(&paths.config_file)?;

    if !paths.sif_path.exists() {
        bail!(
            "Base SIF not found at {}. Run `nix-apptainer init` first.",
            paths.sif_path.display()
        );
    }
    if !paths.overlay_path.exists() {
        bail!(
            "Overlay not found at {}. Run `nix-apptainer init` first.",
            paths.overlay_path.display()
        );
    }

    // Warn if overlay is getting full (compare actual disk usage vs allocated size)
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(meta) = std::fs::metadata(&paths.overlay_path) {
            let on_disk = meta.blocks() * 512;
            let allocated = meta.len();
            if let Some(warning) = crate::util::overlay_usage_warning(on_disk, allocated, 80) {
                eprintln!("{warning}");
            }
        }
    }

    if flags.command.is_empty() {
        bail!("No command specified. Usage: nix-apptainer exec -- <command>");
    }

    let apptainer = checks::apptainer_binary(&sys)
        .context("apptainer/singularity not found")?;

    let opts = ContainerOpts {
        paths: &paths,
        config: &config,
        nv: flags.nv,
        rocm: flags.rocm,
        bind: &flags.bind,
        passthrough: &flags.passthrough,
        quiet: flags.quiet || config.enter.quiet,
    };
    let mut args = build_apptainer_args(&opts, ContainerMode::Exec);
    args.extend(flags.command.iter().cloned());

    let err = exec_replace(&apptainer, &args);
    Err(err.into())
}

fn exec_replace(program: &str, args: &[String]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    Command::new(program).args(args).exec()
}
