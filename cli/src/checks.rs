use std::path::Path;

use crate::system::System;

pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub required: bool,
}

/// Find apptainer or singularity binary. Returns the binary name.
pub fn find_apptainer(sys: &dyn System) -> CheckResult {
    for name in ["apptainer", "singularity"] {
        if let Some(version) = sys.command_version(name, "--version") {
            return CheckResult {
                name: "Container runtime".to_string(),
                passed: true,
                message: version,
                required: true,
            };
        }
    }
    CheckResult {
        name: "Container runtime".to_string(),
        passed: false,
        message: "Neither apptainer nor singularity found on PATH. Install apptainer: https://apptainer.org/docs/admin/main/installation.html".to_string(),
        required: true,
    }
}

/// Returns the name of the apptainer/singularity binary, if found.
pub fn apptainer_binary(sys: &dyn System) -> Option<String> {
    for name in ["apptainer", "singularity"] {
        if sys.find_command(name).is_some() {
            return Some(name.to_string());
        }
    }
    None
}

/// Check for FUSE support.
pub fn check_fuse(sys: &dyn System) -> CheckResult {
    let dev_fuse = sys.path_exists(Path::new("/dev/fuse"));
    let fusermount = sys.command_version("fusermount3", "-V").is_some()
        || sys.command_version("fusermount", "-V").is_some();
    if dev_fuse || fusermount {
        CheckResult {
            name: "FUSE support".to_string(),
            passed: true,
            message: "available".to_string(),
            required: true,
        }
    } else {
        CheckResult {
            name: "FUSE support".to_string(),
            passed: false,
            message: "Neither /dev/fuse nor fusermount found. Install fuse3: e.g. `sudo apt install fuse3`".to_string(),
            required: true,
        }
    }
}

/// Check for fuse-overlayfs binary.
pub fn check_fuse_overlayfs(sys: &dyn System) -> CheckResult {
    if sys.command_version("fuse-overlayfs", "--version").is_some() {
        CheckResult {
            name: "fuse-overlayfs".to_string(),
            passed: true,
            message: "available".to_string(),
            required: true,
        }
    } else {
        CheckResult {
            name: "fuse-overlayfs".to_string(),
            passed: false,
            message: "Not found on PATH. Required for overlay mounts.".to_string(),
            required: true,
        }
    }
}

/// Check for fakeroot support.
pub fn check_fakeroot(sys: &dyn System) -> CheckResult {
    if sys.command_version("fakeroot", "--version").is_some() {
        CheckResult {
            name: "fakeroot".to_string(),
            passed: true,
            message: "available".to_string(),
            required: false,
        }
    } else {
        CheckResult {
            name: "fakeroot".to_string(),
            passed: false,
            message: "Not found. Some overlay operations may require it. Install: e.g. `sudo apt install fakeroot`".to_string(),
            required: false,
        }
    }
}

/// Check available disk space at the given path.
pub fn check_disk_space(sys: &dyn System, path: &Path) -> CheckResult {
    let check_path = std::iter::successors(Some(path), |p| p.parent())
        .find(|p| p.exists())
        .unwrap_or(Path::new("/"));
    match sys.available_disk_bytes(check_path) {
        Some(bytes) => {
            let gb = bytes as f64 / 1_073_741_824.0;
            let passed = gb >= 2.0;
            CheckResult {
                name: "Disk space".to_string(),
                passed,
                message: format!("{:.1} GB available at {}", gb, check_path.display()),
                required: false,
            }
        }
        None => CheckResult {
            name: "Disk space".to_string(),
            passed: true,
            message: "Could not determine available space".to_string(),
            required: false,
        },
    }
}

/// Results of running all system checks.
pub struct SystemCheckReport {
    pub results: Vec<CheckResult>,
    pub apptainer_binary: Option<String>,
    pub any_required_failed: bool,
}

/// Run all system checks.
pub fn run_all_checks(sys: &dyn System, data_path: &Path) -> SystemCheckReport {
    let results = vec![
        find_apptainer(sys),
        check_fuse(sys),
        check_fuse_overlayfs(sys),
        check_fakeroot(sys),
        check_disk_space(sys, data_path),
    ];
    let any_required_failed = results.iter().any(|c| c.required && !c.passed);
    let apptainer_binary = apptainer_binary(sys);
    SystemCheckReport {
        results,
        apptainer_binary,
        any_required_failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::ExitStatus;

    struct MockSystem {
        commands: std::collections::HashMap<String, String>,
        disk_bytes: Option<u64>,
        existing_paths: Vec<std::path::PathBuf>,
    }

    impl MockSystem {
        fn with_apptainer() -> Self {
            let mut commands = std::collections::HashMap::new();
            commands.insert("apptainer".to_string(), "apptainer version 1.3.0".to_string());
            commands.insert("fusermount3".to_string(), "fusermount3 version 3.16.1".to_string());
            commands.insert("fuse-overlayfs".to_string(), "fuse-overlayfs 1.13".to_string());
            Self {
                commands,
                disk_bytes: Some(10 * 1_073_741_824),
                existing_paths: vec![std::path::PathBuf::from("/dev/fuse")],
            }
        }

        fn empty() -> Self {
            Self {
                commands: std::collections::HashMap::new(),
                disk_bytes: None,
                existing_paths: vec![],
            }
        }
    }

    impl crate::system::System for MockSystem {
        fn run_command(&self, _program: &str, _args: &[&str]) -> anyhow::Result<ExitStatus> {
            unimplemented!("not used in check tests")
        }
        fn find_command(&self, name: &str) -> Option<String> {
            self.commands.get(name).map(|_| name.to_string())
        }
        fn command_version(&self, name: &str, _flag: &str) -> Option<String> {
            self.commands.get(name).cloned()
        }
        fn available_disk_bytes(&self, _path: &Path) -> Option<u64> {
            self.disk_bytes
        }
        fn path_exists(&self, path: &Path) -> bool {
            self.existing_paths.iter().any(|p| p == path)
        }
    }

    #[test]
    fn test_find_apptainer_found() {
        let sys = MockSystem::with_apptainer();
        let result = find_apptainer(&sys);
        assert!(result.passed);
        assert!(result.message.contains("apptainer"));
    }

    #[test]
    fn test_find_apptainer_not_found() {
        let sys = MockSystem::empty();
        let result = find_apptainer(&sys);
        assert!(!result.passed);
        assert!(result.required);
    }

    #[test]
    fn test_find_apptainer_singularity_fallback() {
        let mut sys = MockSystem::empty();
        sys.commands.insert("singularity".to_string(), "singularity version 3.11".to_string());
        let result = find_apptainer(&sys);
        assert!(result.passed);
        assert!(result.message.contains("singularity"));
    }

    #[test]
    fn test_check_fuse_dev_fuse() {
        let mut sys = MockSystem::empty();
        sys.existing_paths.push(std::path::PathBuf::from("/dev/fuse"));
        let result = check_fuse(&sys);
        assert!(result.passed);
    }

    #[test]
    fn test_check_fuse_fusermount() {
        let mut sys = MockSystem::empty();
        sys.commands.insert("fusermount3".to_string(), "fusermount3 version 3.16.1".to_string());
        let result = check_fuse(&sys);
        assert!(result.passed);
    }

    #[test]
    fn test_check_fuse_neither() {
        let sys = MockSystem::empty();
        let result = check_fuse(&sys);
        assert!(!result.passed);
        assert!(result.required);
    }

    #[test]
    fn test_disk_space_plenty() {
        let sys = MockSystem::with_apptainer();
        let result = check_disk_space(&sys, Path::new("/tmp"));
        assert!(result.passed);
    }

    #[test]
    fn test_disk_space_low() {
        let mut sys = MockSystem::with_apptainer();
        sys.disk_bytes = Some(1_073_741_824);
        let result = check_disk_space(&sys, Path::new("/tmp"));
        assert!(!result.passed);
    }

    #[test]
    fn test_disk_space_unavailable() {
        let mut sys = MockSystem::with_apptainer();
        sys.disk_bytes = None;
        let result = check_disk_space(&sys, Path::new("/tmp"));
        assert!(result.passed);
    }

    #[test]
    fn test_run_all_checks_all_pass() {
        let sys = MockSystem::with_apptainer();
        let report = run_all_checks(&sys, Path::new("/tmp"));
        assert!(!report.any_required_failed);
        assert!(report.apptainer_binary.is_some());
    }

    #[test]
    fn test_run_all_checks_required_fails() {
        let sys = MockSystem::empty();
        let report = run_all_checks(&sys, Path::new("/tmp"));
        assert!(report.any_required_failed);
    }
}
