use crate::config::{Config, GpuMode};
use crate::paths::AppPaths;

/// Whether to launch an interactive shell or execute a command.
pub enum ContainerMode {
    /// `apptainer run` -- runs the container's runscript (interactive shell)
    Run,
    /// `apptainer exec` -- runs a specific command
    Exec,
}

/// Options for building the apptainer command line.
pub struct ContainerOpts<'a> {
    pub paths: &'a AppPaths,
    pub config: &'a Config,
    pub nv: bool,
    pub rocm: bool,
    pub bind: &'a [String],
    pub passthrough: &'a [String],
}

/// Build the argument list for an apptainer run/exec invocation.
///
/// Returns a `Vec<String>` of arguments to pass after the apptainer binary name.
/// For `Exec` mode, the caller must append the command to run after calling this.
pub fn build_apptainer_args(opts: &ContainerOpts, mode: ContainerMode) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    // Mode
    match mode {
        ContainerMode::Run => args.push("run".to_string()),
        ContainerMode::Exec => args.push("exec".to_string()),
    }

    // Overlay
    args.push("--overlay".to_string());
    args.push(opts.paths.overlay_path.to_string_lossy().to_string());

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

    // Passthrough args
    args.extend(opts.passthrough.iter().cloned());

    // SIF path
    args.push(opts.paths.sif_path.to_string_lossy().to_string());

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EnterConfig, OverlayConfig, SifConfig};
    use std::path::PathBuf;

    fn test_paths() -> AppPaths {
        AppPaths::resolve_with_data_dir(PathBuf::from("/tmp/test"))
    }

    fn test_config() -> Config {
        Config {
            sif: SifConfig::default(),
            overlay: OverlayConfig::default(),
            enter: EnterConfig::default(),
        }
    }

    #[test]
    fn test_run_mode_basic() {
        let paths = test_paths();
        let config = test_config();
        let opts = ContainerOpts {
            paths: &paths,
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert_eq!(args[0], "run");
        assert_eq!(args[1], "--overlay");
        assert!(args.last().unwrap().ends_with("base.sif"));
    }

    #[test]
    fn test_exec_mode() {
        let paths = test_paths();
        let config = test_config();
        let opts = ContainerOpts {
            paths: &paths,
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
        };
        let args = build_apptainer_args(&opts, ContainerMode::Exec);
        assert_eq!(args[0], "exec");
    }

    #[test]
    fn test_gpu_from_flag() {
        let paths = test_paths();
        let config = test_config();
        let opts = ContainerOpts {
            paths: &paths,
            config: &config,
            nv: true,
            rocm: false,
            bind: &[],
            passthrough: &[],
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(args.contains(&"--nv".to_string()));
        assert!(!args.contains(&"--rocm".to_string()));
    }

    #[test]
    fn test_gpu_from_config() {
        let paths = test_paths();
        let mut config = test_config();
        config.enter.gpu = GpuMode::Rocm;
        let opts = ContainerOpts {
            paths: &paths,
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &[],
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(args.contains(&"--rocm".to_string()));
    }

    #[test]
    fn test_bind_mounts_combined() {
        let paths = test_paths();
        let mut config = test_config();
        config.enter.bind = vec!["/data:/data".to_string()];
        let flag_binds = vec!["/scratch:/scratch".to_string()];
        let opts = ContainerOpts {
            paths: &paths,
            config: &config,
            nv: false,
            rocm: false,
            bind: &flag_binds,
            passthrough: &[],
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
        let config = test_config();
        let passthrough = vec!["--writable-tmpfs".to_string()];
        let opts = ContainerOpts {
            paths: &paths,
            config: &config,
            nv: false,
            rocm: false,
            bind: &[],
            passthrough: &passthrough,
        };
        let args = build_apptainer_args(&opts, ContainerMode::Run);
        assert!(args.contains(&"--writable-tmpfs".to_string()));
    }
}
