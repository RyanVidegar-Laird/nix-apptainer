use std::path::Path;

use crate::config::OverlayType;
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

/// Parse fuse-overlayfs --version output, e.g. "fuse-overlayfs: version 1.13".
pub fn parse_fuse_overlayfs_version(output: &str) -> Option<(u32, u32)> {
    for tok in output.split_whitespace() {
        let t = tok.trim_matches(|c: char| !(c.is_ascii_digit() || c == '.'));
        let mut parts = t.split('.');
        if let (Some(maj), Some(min)) = (parts.next(), parts.next())
            && let (Ok(maj), Ok(min)) = (maj.parse(), min.parse())
        {
            return Some((maj, min));
        }
    }
    None
}

/// fuse-overlayfs ≤1.13 answers access(W_OK) with EPERM on 0755 dirs the
/// caller owns (containers/fuse-overlayfs#232, #374) — breaks local builds.
pub fn fuse_overlayfs_is_buggy(version: (u32, u32)) -> bool {
    version <= (1, 13)
}

/// The fuse-overlayfs bundled with an apptainer install. Apptainer resolves
/// helpers from libexec/<runtime>/bin BEFORE $PATH, so this is the binary
/// that actually runs.
pub fn bundled_fuse_overlayfs(
    sys: &dyn System,
    apptainer_path: &Path,
) -> Option<std::path::PathBuf> {
    let prefix = apptainer_path.parent()?.parent()?;
    for runtime in ["apptainer", "singularity"] {
        let candidate = prefix
            .join("libexec")
            .join(runtime)
            .join("bin")
            .join("fuse-overlayfs");
        if sys.path_exists(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Overlay modes only: fail on the bundled-fuse-overlayfs ≤1.13 bug, warn on
/// the no-fuse-overlayfs-at-all trap (kernel overlayfs in userns breaks Nix).
pub fn check_fuse_overlayfs(sys: &dyn System) -> CheckResult {
    let name = "fuse-overlayfs".to_string();
    let Some(apptainer) = apptainer_binary(sys) else {
        return CheckResult {
            name,
            passed: true,
            message: "skipped (no container runtime)".to_string(),
            required: false,
        };
    };
    let Some(apptainer_path) = sys.resolve_command_path(&apptainer) else {
        return CheckResult {
            name,
            passed: true,
            message: "could not locate the apptainer binary".to_string(),
            required: false,
        };
    };
    if let Some(bundled) = bundled_fuse_overlayfs(sys, &apptainer_path) {
        let version = sys
            .command_version(&bundled.to_string_lossy(), "--version")
            .as_deref()
            .and_then(parse_fuse_overlayfs_version);
        match version {
            Some(v) if fuse_overlayfs_is_buggy(v) => CheckResult {
                name,
                passed: false,
                required: true,
                message: format!(
                    "bundled fuse-overlayfs {}.{} has a permission bug that breaks local Nix \
                     builds on overlay stores. Use `--overlay-type sandbox`, or ask admins to \
                     upgrade apptainer.",
                    v.0, v.1
                ),
            },
            Some(v) => CheckResult {
                name,
                passed: true,
                required: true,
                message: format!("bundled, version {}.{}", v.0, v.1),
            },
            None => CheckResult {
                name,
                passed: true,
                required: false,
                message: "bundled (version unknown)".to_string(),
            },
        }
    } else if sys.find_command("fuse-overlayfs").is_some() {
        CheckResult {
            name,
            passed: true,
            required: false,
            message: "found on PATH".to_string(),
        }
    } else {
        CheckResult {
            name,
            passed: false,
            required: false,
            message: "not bundled and not on PATH — apptainer will fall back to kernel \
                      overlayfs, which breaks Nix (rename/unlink in userns). \
                      Run inside `nix shell nixpkgs#fuse-overlayfs`."
                .to_string(),
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

/// statfs f_type magics for network/parallel filesystems where an unpacked
/// sandbox (~10^5 small files) performs poorly and eats inode quota.
const NETWORK_FS_MAGICS: &[(i64, &str)] = &[
    (0x0BD0_0BD0, "Lustre"),
    (0x6969, "NFS"),
    (0x4750_4653, "GPFS"),
    (0x1983_0326, "BeeGFS"),
    (0x00C3_6400, "CephFS"),
];

pub fn network_fs_name(magic: i64) -> Option<&'static str> {
    NETWORK_FS_MAGICS
        .iter()
        .find(|(m, _)| *m == magic)
        .map(|(_, n)| *n)
}

/// Sandbox mode only: warn (never fail) when the data dir is on a
/// network/parallel filesystem.
pub fn check_sandbox_location(sys: &dyn System, path: &Path) -> CheckResult {
    let check_path = std::iter::successors(Some(path), |p| p.parent())
        .find(|p| sys.path_exists(p))
        .unwrap_or(Path::new("/"));
    match sys.filesystem_magic(check_path).and_then(network_fs_name) {
        Some(fs) => CheckResult {
            name: "Sandbox location".to_string(),
            passed: false,
            required: false,
            message: format!(
                "{fs} filesystem detected — an unpacked sandbox is hundreds of thousands of \
                 small files (inode quota, slow metadata). Prefer node-local or scratch \
                 storage via --data-dir or NIX_APPTAINER_HOME."
            ),
        },
        None => CheckResult {
            name: "Sandbox location".to_string(),
            passed: true,
            required: false,
            message: "local filesystem".to_string(),
        },
    }
}

/// Check available disk space at the given path against a minimum in GB.
pub fn check_disk_space(sys: &dyn System, path: &Path, min_gb: f64) -> CheckResult {
    let check_path = std::iter::successors(Some(path), |p| p.parent())
        .find(|p| p.exists())
        .unwrap_or(Path::new("/"));
    match sys.available_disk_bytes(check_path) {
        Some(bytes) => {
            let gb = bytes as f64 / 1_073_741_824.0;
            let passed = gb >= min_gb;
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
/// Contains individual check results, the detected apptainer binary name,
/// and whether any required check failed.
pub struct SystemCheckReport {
    pub results: Vec<CheckResult>,
    /// The detected apptainer/singularity binary name, if found.
    /// Used by status display and overlay operations.
    #[allow(dead_code)]
    pub apptainer_binary: Option<String>,
    pub any_required_failed: bool,
}

/// Run all system checks appropriate for the chosen storage mode.
/// Sandbox mode needs no FUSE at all; overlay modes need the full stack.
pub fn run_all_checks(
    sys: &dyn System,
    data_path: &Path,
    overlay_type: &OverlayType,
) -> SystemCheckReport {
    let mut results = vec![find_apptainer(sys)];
    match overlay_type {
        OverlayType::Sandbox => {
            results.push(check_sandbox_location(sys, data_path));
            // Unpacked sandbox needs several GB up front
            results.push(check_disk_space(sys, data_path, 10.0));
        }
        OverlayType::Directory | OverlayType::Ext3 => {
            results.push(check_fuse(sys));
            results.push(check_fuse_overlayfs(sys));
            results.push(check_fakeroot(sys));
            results.push(check_disk_space(sys, data_path, 2.0));
        }
    }
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
        resolved_paths: std::collections::HashMap<String, std::path::PathBuf>,
        fs_magic: Option<i64>,
    }

    impl MockSystem {
        fn with_apptainer() -> Self {
            let mut commands = std::collections::HashMap::new();
            commands.insert(
                "apptainer".to_string(),
                "apptainer version 1.3.0".to_string(),
            );
            commands.insert(
                "fusermount3".to_string(),
                "fusermount3 version 3.16.1".to_string(),
            );
            Self {
                commands,
                disk_bytes: Some(10 * 1_073_741_824),
                existing_paths: vec![std::path::PathBuf::from("/dev/fuse")],
                resolved_paths: std::collections::HashMap::new(),
                fs_magic: None,
            }
        }

        fn empty() -> Self {
            Self {
                commands: std::collections::HashMap::new(),
                disk_bytes: None,
                existing_paths: vec![],
                resolved_paths: std::collections::HashMap::new(),
                fs_magic: None,
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
        fn resolve_command_path(&self, name: &str) -> Option<std::path::PathBuf> {
            self.resolved_paths.get(name).cloned()
        }
        fn filesystem_magic(&self, _path: &Path) -> Option<i64> {
            self.fs_magic
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
        sys.commands.insert(
            "singularity".to_string(),
            "singularity version 3.11".to_string(),
        );
        let result = find_apptainer(&sys);
        assert!(result.passed);
        assert!(result.message.contains("singularity"));
    }

    #[test]
    fn test_check_fuse_dev_fuse() {
        let mut sys = MockSystem::empty();
        sys.existing_paths
            .push(std::path::PathBuf::from("/dev/fuse"));
        let result = check_fuse(&sys);
        assert!(result.passed);
    }

    #[test]
    fn test_check_fuse_fusermount() {
        let mut sys = MockSystem::empty();
        sys.commands.insert(
            "fusermount3".to_string(),
            "fusermount3 version 3.16.1".to_string(),
        );
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
        let result = check_disk_space(&sys, Path::new("/tmp"), 2.0);
        assert!(result.passed);
    }

    #[test]
    fn test_disk_space_low() {
        let mut sys = MockSystem::with_apptainer();
        sys.disk_bytes = Some(1_073_741_824);
        let result = check_disk_space(&sys, Path::new("/tmp"), 2.0);
        assert!(!result.passed);
    }

    #[test]
    fn test_disk_space_unavailable() {
        let mut sys = MockSystem::with_apptainer();
        sys.disk_bytes = None;
        let result = check_disk_space(&sys, Path::new("/tmp"), 2.0);
        assert!(result.passed);
    }

    #[test]
    fn test_parse_fuse_overlayfs_version() {
        assert_eq!(
            parse_fuse_overlayfs_version("fuse-overlayfs: version 1.13"),
            Some((1, 13))
        );
        assert_eq!(
            parse_fuse_overlayfs_version("fuse-overlayfs: version 1.10\nFUSE library version 3.9.1"),
            Some((1, 10))
        );
        assert_eq!(parse_fuse_overlayfs_version("garbage"), None);
    }

    #[test]
    fn test_fuse_overlayfs_buggy_boundary() {
        assert!(fuse_overlayfs_is_buggy((1, 13)));
        assert!(fuse_overlayfs_is_buggy((1, 10)));
        assert!(!fuse_overlayfs_is_buggy((1, 14)));
        assert!(!fuse_overlayfs_is_buggy((2, 0)));
    }

    #[test]
    fn test_bundled_fuse_overlayfs_found() {
        let mut sys = MockSystem::with_apptainer();
        let helper =
            std::path::PathBuf::from("/opt/apptainer/libexec/apptainer/bin/fuse-overlayfs");
        sys.existing_paths.push(helper.clone());
        let found = bundled_fuse_overlayfs(&sys, Path::new("/opt/apptainer/bin/apptainer"));
        assert_eq!(found, Some(helper));
    }

    #[test]
    fn test_check_fuse_overlayfs_buggy_bundled_fails_required() {
        let mut sys = MockSystem::with_apptainer();
        sys.resolved_paths.insert(
            "apptainer".to_string(),
            std::path::PathBuf::from("/usr/bin/apptainer"),
        );
        sys.existing_paths.push(std::path::PathBuf::from(
            "/usr/libexec/apptainer/bin/fuse-overlayfs",
        ));
        sys.commands.insert(
            "/usr/libexec/apptainer/bin/fuse-overlayfs".to_string(),
            "fuse-overlayfs: version 1.13".to_string(),
        );
        let result = check_fuse_overlayfs(&sys);
        assert!(!result.passed);
        assert!(result.required);
        assert!(result.message.contains("sandbox"), "msg: {}", result.message);
    }

    #[test]
    fn test_check_fuse_overlayfs_no_bundled_no_path_warns() {
        let mut sys = MockSystem::with_apptainer();
        sys.resolved_paths.insert(
            "apptainer".to_string(),
            std::path::PathBuf::from("/usr/bin/apptainer"),
        );
        // no bundled helper, no fuse-overlayfs on PATH
        let result = check_fuse_overlayfs(&sys);
        assert!(!result.passed);
        assert!(!result.required); // warning, not fatal
        assert!(
            result.message.contains("kernel overlayfs"),
            "msg: {}",
            result.message
        );
    }

    #[test]
    fn test_run_all_checks_all_pass() {
        let sys = MockSystem::with_apptainer();
        let report = run_all_checks(&sys, Path::new("/tmp"), &OverlayType::Directory);
        assert!(!report.any_required_failed);
        assert!(report.apptainer_binary.is_some());
    }

    #[test]
    fn test_run_all_checks_required_fails() {
        let sys = MockSystem::empty();
        let report = run_all_checks(&sys, Path::new("/tmp"), &OverlayType::Directory);
        assert!(report.any_required_failed);
    }

    #[test]
    fn test_network_fs_name() {
        assert_eq!(network_fs_name(0x0BD0_0BD0), Some("Lustre"));
        assert_eq!(network_fs_name(0x6969), Some("NFS"));
        assert_eq!(network_fs_name(0x0187_3101), None); // ext4-ish magic
    }

    #[test]
    fn test_check_sandbox_location_warns_on_lustre() {
        let mut sys = MockSystem::with_apptainer();
        sys.fs_magic = Some(0x0BD0_0BD0);
        sys.existing_paths
            .push(std::path::PathBuf::from("/lustre/home"));
        let result = check_sandbox_location(&sys, Path::new("/lustre/home/user/na"));
        assert!(!result.passed);
        assert!(!result.required);
        assert!(result.message.contains("Lustre"), "msg: {}", result.message);
    }

    #[test]
    fn test_check_sandbox_location_local_ok() {
        let mut sys = MockSystem::with_apptainer();
        sys.fs_magic = Some(0x0187_3101);
        sys.existing_paths.push(std::path::PathBuf::from("/home"));
        let result = check_sandbox_location(&sys, Path::new("/home/user/na"));
        assert!(result.passed);
    }

    #[test]
    fn test_run_all_checks_sandbox_mode_skips_fuse() {
        // no FUSE anywhere; apptainer must still be found for the report to pass
        let mut sys = MockSystem::empty();
        sys.commands.insert(
            "apptainer".to_string(),
            "apptainer version 1.3.0".to_string(),
        );
        sys.disk_bytes = Some(20 * 1_073_741_824);
        let report = run_all_checks(&sys, Path::new("/tmp"), &OverlayType::Sandbox);
        assert!(
            !report.any_required_failed,
            "sandbox mode must not require FUSE"
        );
        assert!(!report.results.iter().any(|r| r.name == "FUSE support"));
    }

    #[test]
    fn test_run_all_checks_overlay_mode_requires_fuse() {
        let mut sys = MockSystem::empty();
        sys.commands.insert(
            "apptainer".to_string(),
            "apptainer version 1.3.0".to_string(),
        );
        let report = run_all_checks(&sys, Path::new("/tmp"), &OverlayType::Directory);
        assert!(report.any_required_failed, "overlay mode must require FUSE");
    }
}
