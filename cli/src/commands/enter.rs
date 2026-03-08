use anyhow::{bail, Context};
use std::process::Command;

use crate::checks;
use crate::config::Config;
use crate::paths::AppPaths;

pub struct EnterFlags {
    pub nv: bool,
    pub rocm: bool,
    pub bind: Vec<String>,
    pub passthrough: Vec<String>,
}

pub fn run(flags: EnterFlags) -> anyhow::Result<()> {
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

    let apptainer = checks::apptainer_binary()
        .context("apptainer/singularity not found")?;

    let mut args: Vec<String> = vec!["shell".to_string()];
    args.push("--overlay".to_string());
    args.push(paths.overlay_path.to_string_lossy().to_string());

    // GPU from config, overridden by flags
    let use_nv = flags.nv || config.enter.gpu == "nvidia";
    let use_rocm = flags.rocm || config.enter.gpu == "rocm";
    if use_nv {
        args.push("--nv".to_string());
    }
    if use_rocm {
        args.push("--rocm".to_string());
    }

    // Bind mounts from config + flags
    for b in &config.enter.bind {
        args.push("--bind".to_string());
        args.push(b.clone());
    }
    for b in &flags.bind {
        args.push("--bind".to_string());
        args.push(b.clone());
    }

    // Passthrough args
    args.extend(flags.passthrough.iter().cloned());

    args.push(paths.sif_path.to_string_lossy().to_string());

    let err = exec_replace(&apptainer, &args);
    Err(err.into())
}

/// Replace the current process with apptainer (Unix exec).
fn exec_replace(program: &str, args: &[String]) -> std::io::Error {
    use std::os::unix::process::CommandExt;
    Command::new(program).args(args).exec()
}
