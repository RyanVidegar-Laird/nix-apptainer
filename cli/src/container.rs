use std::path::Path;

use crate::config::{Config, GpuMode};

/// Whether to launch an interactive shell or execute a command.
pub enum ContainerMode {
    /// `apptainer run` -- runs the container's runscript (interactive shell)
    Run,
    /// `apptainer exec` -- runs a specific command
    Exec,
}

/// What the apptainer invocation targets.
pub enum ContainerTarget<'a> {
    /// Read-only SIF with a writable overlay (`--overlay <path> <sif>`).
    Overlay { sif: &'a Path, overlay: &'a str },
    /// Writable sandbox directory (`--writable <dir>`), no overlay, no FUSE.
    Sandbox { dir: &'a Path },
}

/// Options for building the apptainer command line.
pub struct ContainerOpts<'a> {
    pub target: ContainerTarget<'a>,
    pub config: &'a Config,
    pub nv: bool,
    pub rocm: bool,
    pub bind: &'a [String],
    pub passthrough: &'a [String],
    pub quiet: bool,
}

/// Build the argument list for an apptainer run/exec invocation.
///
/// Returns a `Vec<String>` of arguments to pass after the apptainer binary name.
pub fn build_apptainer_args(opts: &ContainerOpts, mode: ContainerMode) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    // Global flags (must come before subcommand)
    if opts.quiet {
        args.push("--quiet".to_string());
    }

    // Mode
    match mode {
        ContainerMode::Run => args.push("run".to_string()),
        ContainerMode::Exec => args.push("exec".to_string()),
    }

    // Storage: overlay on a SIF, or a writable sandbox directory
    match &opts.target {
        ContainerTarget::Overlay { overlay, .. } => {
            args.push("--overlay".to_string());
            args.push(overlay.to_string());
        }
        ContainerTarget::Sandbox { .. } => {
            args.push("--writable".to_string());
        }
    }

    // Isolate home directory: don't mount host $HOME or CWD into the
    // container. Prevents host dotfile conflicts.
    // HOME is set by Apptainer from the container's /etc/passwd.
    // For `enter` (run mode), entrypoint.sh creates $HOME and bash --login
    // cds there. For `exec` mode, / is a safe starting directory.
    if !opts.config.enter.mount_home {
        args.push("--no-home".to_string());
        args.push("--no-mount".to_string());
        args.push("cwd".to_string());
        args.push("--pwd".to_string());
        args.push("/".to_string());
    }

    // GPU from config, overridden by flags
    let use_nv = opts.nv || opts.config.enter.gpu == GpuMode::Nvidia;
    let use_rocm = opts.rocm || opts.config.enter.gpu == GpuMode::Rocm;
    if use_nv {
        args.push("--nv".to_string());
    }
    if use_rocm {
        args.push("--rocm".to_string());
    }

    // Bind mounts from config + flags
    for b in &opts.config.enter.bind {
        args.push("--bind".to_string());
        args.push(b.clone());
    }
    for b in opts.bind {
        args.push("--bind".to_string());
        args.push(b.clone());
    }

    // Clear NixOS profile guards leaked from the host so /etc/profile
    // re-sources set-environment (which adds $HOME/.nix-profile/bin to PATH)
    for var in [
        "__NIXOS_SET_ENVIRONMENT_DONE",
        "__ETC_PROFILE_DONE",
        "__ETC_BASHRC_SOURCED",
    ] {
        args.push("--env".to_string());
        args.push(format!("{var}="));
    }

    // Pin TMPDIR when configured. Cluster TMPDIRs often point at site
    // filesystems that may not be mounted in the container (sandbox mode
    // skips binds whose mount points are missing), and legacy nix-build
    // creates a client-side scratch dir from $TMPDIR before any build
    // starts — `build-dir` cannot rescue it. Empty means inherit the host's.
    if !opts.config.enter.tmpdir.is_empty() {
        args.push("--env".to_string());
        args.push(format!("TMPDIR={}", opts.config.enter.tmpdir));
    }

    // Passthrough args
    args.extend(opts.passthrough.iter().cloned());

    // Image argument: SIF for overlay modes, the sandbox dir otherwise
    match &opts.target {
        ContainerTarget::Overlay { sif, .. } => {
            args.push(sif.to_string_lossy().to_string());
        }
        ContainerTarget::Sandbox { dir } => {
            args.push(dir.to_string_lossy().to_string());
        }
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EnterConfig, OverlayConfig, SifConfig};
    use crate::paths::AppPaths;
    use std::path::PathBuf;

    fn test_paths() -> AppPaths {
        AppPaths::resolve_with_data_dir(PathBuf::from("/tmp/test"))
    }

    fn test_overlay() -> String {
        test_paths().overlay_path.to_string_lossy().to_string()
    }

    fn test_config() -> Config {
        Config {
            sif: SifConfig::default(),
            overlay: OverlayConfig::default(),
            enter: EnterConfig::default(),
        }
    }

    fn overlay_target<'a>(paths: &'a AppPaths, overlay: &'a str) -> ContainerTarget<'a> {
        ContainerTarget::Overlay {
            sif: &paths.sif_path,
            overlay,
        }
    }

    #[test]
    fn test_sandbox_target_args() {
        let config = test_config();
        let dir = PathBuf::from("/data/na/sandbox");
        let opts = ContainerOpts {
            target: ContainerTarget::Sandbox { dir: &dir },
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert_eq!(args[0], "run");
        assert!(args.contains(&"--writable".to_string()));
        assert!(!args.contains(&"--overlay".to_string()));
        assert_eq!(args.last().unwrap(), "/data/na/sandbox");
    }

    #[test]
    fn test_sandbox_target_exec_mode() {
        let config = test_config();
        let dir = PathBuf::from("/data/na/sandbox");
        let opts = ContainerOpts {
            target: ContainerTarget::Sandbox { dir: &dir },
            config: &config,
            nv: true,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: true,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Exec);
        // --quiet still first, --nv still honored, dir still last
        assert_eq!(args[0], "--quiet");
        assert_eq!(args[1], "exec");
        assert!(args.contains(&"--nv".to_string()));
        assert_eq!(args.last().unwrap(), "/data/na/sandbox");
    }

    #[test]
    fn test_run_mode_basic() {
        let paths = test_paths();
        let overlay = test_overlay();
        let config = test_config();
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert_eq!(args[0], "run");
        assert_eq!(args[1], "--overlay");
        assert!(args.last().unwrap().ends_with("base.sif"));
    }

    #[test]
    fn test_exec_mode() {
        let paths = test_paths();
        let overlay = test_overlay();
        let config = test_config();
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Exec);
        assert_eq!(args[0], "exec");
    }

    #[test]
    fn test_gpu_from_flag() {
        let paths = test_paths();
        let overlay = test_overlay();
        let config = test_config();
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: true,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(args.contains(&"--nv".to_string()));
        assert!(!args.contains(&"--rocm".to_string()));
    }

    #[test]
    fn test_gpu_from_config() {
        let paths = test_paths();
        let overlay = test_overlay();
        let mut config = test_config();
        config.enter.gpu = GpuMode::Rocm;
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(args.contains(&"--rocm".to_string()));
    }

    #[test]
    fn test_bind_mounts_combined() {
        let paths = test_paths();
        let overlay = test_overlay();
        let mut config = test_config();
        config.enter.bind = vec!["/data:/data".to_string()];
        let flag_binds = vec!["/scratch:/scratch".to_string()];
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &flag_binds,
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        let bind_count = args.iter().filter(|a| *a == "--bind").count();
        assert_eq!(bind_count, 2);
        assert!(args.contains(&"/data:/data".to_string()));
        assert!(args.contains(&"/scratch:/scratch".to_string()));
    }

    #[test]
    fn test_passthrough_args() {
        let paths = test_paths();
        let overlay = test_overlay();
        let config = test_config();
        let passthrough = vec!["--writable-tmpfs".to_string()];
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &passthrough,
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(args.contains(&"--writable-tmpfs".to_string()));
    }

    #[test]
    fn test_quiet_flag() {
        let paths = test_paths();
        let overlay = test_overlay();
        let config = test_config();
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: true,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        // --quiet must come before the subcommand
        assert_eq!(args[0], "--quiet");
        assert_eq!(args[1], "run");
    }

    #[test]
    fn test_no_quiet_by_default() {
        let paths = test_paths();
        let overlay = test_overlay();
        let config = test_config();
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert_eq!(args[0], "run");
        assert!(!args.contains(&"--quiet".to_string()));
    }

    #[test]
    fn test_no_home_by_default() {
        let paths = test_paths();
        let overlay = test_overlay();
        let config = test_config();
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(args.contains(&"--no-home".to_string()));
        assert!(args.contains(&"--no-mount".to_string()));
        assert!(args.contains(&"cwd".to_string()));
        let pwd_idx = args.iter().position(|a| a == "--pwd").unwrap();
        assert_eq!(
            args[pwd_idx + 1],
            "/",
            "--pwd must target / to avoid FATAL on fresh overlays"
        );
        assert!(!args.contains(&"--home".to_string()));
    }

    #[test]
    fn test_mount_home_skips_no_home() {
        let paths = test_paths();
        let overlay = test_overlay();
        let mut config = test_config();
        config.enter.mount_home = true;
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(!args.contains(&"--no-home".to_string()));
    }

    #[test]
    fn test_tmpdir_inherited_by_default() {
        let paths = test_paths();
        let overlay = test_overlay();
        let config = test_config();
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(
            !args.iter().any(|a| a.starts_with("TMPDIR=")),
            "an unset enter.tmpdir must leave the host's TMPDIR alone"
        );
    }

    #[test]
    fn test_tmpdir_pinned_all_modes_when_configured() {
        let paths = test_paths();
        let overlay = test_overlay();
        let mut config = test_config();
        config.enter.tmpdir = "/scratch/me/tmp".to_string();
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(args.contains(&"TMPDIR=/scratch/me/tmp".to_string()));
        let dir = PathBuf::from("/data/na/sandbox");
        let opts = ContainerOpts {
            target: ContainerTarget::Sandbox { dir: &dir },
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Exec);
        assert!(args.contains(&"TMPDIR=/scratch/me/tmp".to_string()));
    }

    #[test]
    fn test_nixos_env_guards_cleared() {
        let paths = test_paths();
        let overlay = test_overlay();
        let config = test_config();
        let opts = ContainerOpts {
            target: overlay_target(&paths, &overlay),
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
            quiet: false,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(args.contains(&"--env".to_string()));
        assert!(args.contains(&"__NIXOS_SET_ENVIRONMENT_DONE=".to_string()));
        assert!(args.contains(&"__ETC_PROFILE_DONE=".to_string()));
        assert!(args.contains(&"__ETC_BASHRC_SOURCED=".to_string()));
    }
}
