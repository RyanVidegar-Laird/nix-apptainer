use anyhow::{bail, Context};
use std::process::Command;

use crate::checks;
use crate::config::Config;
use crate::container::{build_apptainer_args, ContainerMode, ContainerOpts};
use crate::paths::AppPaths;
use crate::system::RealSystem;

pub struct EnterFlags {
    pub nv: bool,
    pub rocm: bool,
    pub bind: Vec<String>,
    pub passthrough: Vec<String>,
}

pub fn run(flags: EnterFlags) -> anyhow::Result<()> {
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

    let apptainer = checks::apptainer_binary(&sys)
        .context("apptainer/singularity not found")?;

    let opts = ContainerOpts {
        paths: &paths,
        config: &config,
        nv: flags.nv,
        rocm: flags.rocm,
        bind: &flags.bind,
        passthrough: &flags.passthrough,
    };
    let args = build_apptainer_args(&opts, ContainerMode::Run);

    let err = exec_replace(&apptainer, &args);
    Err(err.into())
}

fn exec_replace(program: &str, args: &[String]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    Command::new(program).args(args).exec()
}
