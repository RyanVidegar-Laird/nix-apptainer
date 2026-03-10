use std::path::Path;
use std::process::Command;

pub struct CheckResult {
    pub name: String,
    pub passed: bool,
    pub message: String,
    pub required: bool,
}

/// Find apptainer or singularity binary. Returns the binary name.
pub fn find_apptainer() -> CheckResult {
    for name in ["apptainer", "singularity"] {
        if let Ok(output) = Command::new(name).arg("--version").output()
            && output.status.success()
        {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
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
pub fn apptainer_binary() -> Option<String> {
    for name in ["apptainer", "singularity"] {
        if Command::new(name).arg("--version").output().is_ok() {
            return Some(name.to_string());
        }
    }
    None
}

/// Check for FUSE support.
pub fn check_fuse() -> CheckResult {
    let dev_fuse = Path::new("/dev/fuse").exists();
    let fusermount = Command::new("fusermount3").arg("-V").output().is_ok()
        || Command::new("fusermount").arg("-V").output().is_ok();

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
pub fn check_fuse_overlayfs() -> CheckResult {
    if Command::new("fuse-overlayfs").arg("--version").output().is_ok() {
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
pub fn check_fakeroot() -> CheckResult {
    if Command::new("fakeroot").arg("--version").output().is_ok() {
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
pub fn check_disk_space(path: &Path) -> CheckResult {
    // Find the first existing ancestor directory to check
    let check_path = std::iter::successors(Some(path), |p| p.parent())
        .find(|p| p.exists())
        .unwrap_or(Path::new("/"));

    match available_bytes(check_path) {
        Some(bytes) => {
            let gb = bytes as f64 / 1_073_741_824.0;
            let passed = gb >= 2.0; // warn below 2GB
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
pub fn run_all_checks(data_path: &Path) -> SystemCheckReport {
    let results = vec![
        find_apptainer(),
        check_fuse(),
        check_fuse_overlayfs(),
        check_fakeroot(),
        check_disk_space(data_path),
    ];
    let any_required_failed = results.iter().any(|c| c.required && !c.passed);
    let apptainer_binary = apptainer_binary();
    SystemCheckReport {
        results,
        apptainer_binary,
        any_required_failed,
    }
}

fn available_bytes(path: &Path) -> Option<u64> {
    let stat = nix::sys::statvfs::statvfs(path).ok()?;
    Some(stat.fragment_size() as u64 * stat.blocks_available() as u64)
}
