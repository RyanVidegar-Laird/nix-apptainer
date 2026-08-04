use anyhow::{Context, bail};
use std::path::Path;

use crate::system::System;

/// Probe expression: a trivial derivation with no inputs, buildable
/// offline. If this builds with sandbox=true, the Nix build sandbox
/// works on this host (nested userns + bind mounts on a plain tree).
const PROBE_EXPR: &str = r#"derivation { name = "sandbox-probe"; system = builtins.currentSystem; builder = "/bin/sh"; args = [ "-c" "echo ok > $out" ]; }"#;

/// Args for `apptainer -q build --sandbox <dir> <sif>` — rootless SIF unpack.
/// `-q` keeps residual INFO lines from garbling the progress bar; output is
/// captured and only surfaced on failure.
pub fn build_sandbox_args(dir: &str, sif: &str) -> Vec<String> {
    vec![
        "-q".to_string(),
        "build".to_string(),
        "--sandbox".to_string(),
        dir.to_string(),
        sif.to_string(),
    ]
}

/// Args for the sandboxed-build probe. HOME is a user-owned host dir
/// (root-owned sticky /tmp makes Nix reject HOME=/tmp on HPC); TMPDIR is
/// pinned because legacy nix-build dies creating a scratch dir from an
/// unbound host TMPDIR. /usr/bin/env instead of --env for old runtimes.
pub fn probe_args(dir: &str, home: &str) -> Vec<String> {
    vec![
        "exec".to_string(),
        "--writable".to_string(),
        dir.to_string(),
        "/usr/bin/env".to_string(),
        format!("HOME={home}"),
        "TMPDIR=/tmp".to_string(),
        "nix-build".to_string(),
        "--option".to_string(),
        "sandbox".to_string(),
        "true".to_string(),
        "--no-out-link".to_string(),
        "-E".to_string(),
        PROBE_EXPR.to_string(),
    ]
}

/// Unpack the SIF into a writable sandbox directory. Removes any
/// existing directory first (callers confirm destructive paths).
pub fn create_sandbox(
    sys: &dyn System,
    apptainer: &str,
    sif: &Path,
    dir: &Path,
    expected_bytes: Option<u64>,
) -> anyhow::Result<()> {
    if dir.exists() {
        crate::util::make_writable_recursive(dir);
        std::fs::remove_dir_all(dir)
            .with_context(|| format!("Failed to remove old sandbox: {}", dir.display()))?;
    }
    if let Some(parent) = dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let dir_str = dir.to_string_lossy();
    let sif_str = sif.to_string_lossy();
    let args = build_sandbox_args(&dir_str, &sif_str);
    let argrefs: Vec<&str> = args.iter().map(String::as_str).collect();
    let progress = crate::progress::UnpackProgress::start(dir.to_path_buf(), expected_bytes);
    let result = sys.run_command_capture(apptainer, &argrefs);
    progress.finish();
    match result {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            eprint!("{}", String::from_utf8_lossy(&out.stderr));
            bail!(
                "{apptainer} build --sandbox failed (exit code: {:?})",
                out.status.code()
            )
        }
        Err(e) => Err(e).with_context(|| format!("Failed to run {apptainer} build --sandbox")),
    }
}

/// Result of the build-sandbox probe.
pub struct ProbeOutcome {
    pub enabled: bool,
    /// Tail of the probe's stderr when it failed — a failed probe can be
    /// environmental (unbound TMPDIR, bad HOME), not a real capability gap.
    pub detail: Option<String>,
}

/// Run the probe; write the verdict (both ways — a stale `sandbox = true`
/// must not survive a failing probe) to /etc/nix/nix.conf.local inside the
/// sandbox, picked up via the image's `!include`.
pub fn probe_and_enable_nix_sandbox(
    sys: &dyn System,
    apptainer: &str,
    dir: &Path,
) -> anyhow::Result<ProbeOutcome> {
    // Host path under /tmp, which apptainer bind-mounts into the container
    // at the same path by default, so it is writable from both sides.
    let uid = nix::unistd::getuid().as_raw();
    let probe_home = std::env::temp_dir().join(format!("nix-apptainer-probe-{uid}"));
    std::fs::create_dir_all(&probe_home)
        .with_context(|| format!("Failed to create probe home: {}", probe_home.display()))?;

    let dir_str = dir.to_string_lossy();
    let args = probe_args(&dir_str, &probe_home.to_string_lossy());
    let argrefs: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = sys.run_command_capture(apptainer, &argrefs);
    let _ = std::fs::remove_dir_all(&probe_home);

    let (enabled, detail) = match result {
        Ok(out) if out.status.success() => (true, None),
        Ok(out) => (false, Some(stderr_tail(&out.stderr, 5))),
        Err(e) => (false, Some(e.to_string())),
    };
    let verdict = if enabled {
        "sandbox = true\n"
    } else {
        "sandbox = false\n"
    };
    std::fs::write(dir.join("etc/nix/nix.conf.local"), verdict)
        .context("Failed to write etc/nix/nix.conf.local")?;
    Ok(ProbeOutcome { enabled, detail })
}

/// Last `n` lines of captured stderr.
fn stderr_tail(stderr: &[u8], n: usize) -> String {
    let s = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = s.lines().collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_args_pins_home_and_tmpdir() {
        let args = probe_args("/data/sandbox", "/tmp/nix-apptainer-probe-1000");
        assert_eq!(args[0], "exec");
        assert_eq!(args[1], "--writable");
        assert_eq!(args[2], "/data/sandbox");
        // Env pins go through /usr/bin/env for old-runtime compatibility.
        // Both are required: a host TMPDIR at an unbound path or a
        // root-owned HOME each break the probe's nix-build (cluster Bug A/B).
        assert!(args.contains(&"HOME=/tmp/nix-apptainer-probe-1000".to_string()));
        assert!(args.contains(&"TMPDIR=/tmp".to_string()));
        // sandbox=true must be forced regardless of baked config
        let i = args.iter().position(|a| a == "sandbox").unwrap();
        assert_eq!(args[i - 1], "--option");
        assert_eq!(args[i + 1], "true");
        assert!(args.last().unwrap().contains("sandbox-probe"));
    }

    use std::cell::Cell;
    use std::path::Path as StdPath;

    pub struct FakeSystem {
        pub exit_code: i32,
        pub stderr: &'static str,
        pub ran: Cell<bool>,
    }

    impl crate::system::System for FakeSystem {
        fn run_command(&self, _: &str, _: &[&str]) -> anyhow::Result<std::process::ExitStatus> {
            unimplemented!("probe uses run_command_capture")
        }
        fn run_command_capture(
            &self,
            _program: &str,
            _args: &[&str],
        ) -> anyhow::Result<std::process::Output> {
            use std::os::unix::process::ExitStatusExt;
            self.ran.set(true);
            Ok(std::process::Output {
                status: std::process::ExitStatus::from_raw(self.exit_code << 8),
                stdout: Vec::new(),
                stderr: self.stderr.as_bytes().to_vec(),
            })
        }
        fn find_command(&self, _: &str) -> Option<String> {
            None
        }
        fn command_version(&self, _: &str, _: &str) -> Option<String> {
            None
        }
        fn available_disk_bytes(&self, _: &StdPath) -> Option<u64> {
            None
        }
        fn path_exists(&self, _: &StdPath) -> bool {
            false
        }
        fn resolve_command_path(&self, _: &str) -> Option<std::path::PathBuf> {
            None
        }
        fn filesystem_magic(&self, _: &StdPath) -> Option<i64> {
            None
        }
    }

    #[test]
    fn test_create_sandbox_replaces_readonly_tree_and_errors_on_failure() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let sandbox = dir.path().join("sandbox");
        let sub = sandbox.join("nix/store/pkg");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o555)).unwrap();

        let ok_sys = FakeSystem {
            exit_code: 0,
            stderr: "",
            ran: Cell::new(false),
        };
        create_sandbox(&ok_sys, "apptainer", Path::new("/x.sif"), &sandbox, None).unwrap();
        assert!(ok_sys.ran.get());
        assert!(!sub.exists(), "old tree must be removed before unpack");

        let fail_sys = FakeSystem {
            exit_code: 1,
            stderr: "FATAL: boom",
            ran: Cell::new(false),
        };
        let err = create_sandbox(&fail_sys, "apptainer", Path::new("/x.sif"), &sandbox, None)
            .unwrap_err();
        assert!(err.to_string().contains("build --sandbox"), "err: {err}");
    }

    fn sandbox_with_etc_nix() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("etc/nix")).unwrap();
        dir
    }

    #[test]
    fn test_probe_success_writes_sandbox_true() {
        let dir = sandbox_with_etc_nix();
        let sys = FakeSystem {
            exit_code: 0,
            stderr: "",
            ran: Cell::new(false),
        };
        let outcome = probe_and_enable_nix_sandbox(&sys, "apptainer", dir.path()).unwrap();
        assert!(outcome.enabled);
        assert!(outcome.detail.is_none());
        let conf = std::fs::read_to_string(dir.path().join("etc/nix/nix.conf.local")).unwrap();
        assert_eq!(conf, "sandbox = true\n");
    }

    #[test]
    fn test_probe_failure_writes_sandbox_false_with_detail() {
        let dir = sandbox_with_etc_nix();
        // Simulate a stale success from an earlier probe: must be overwritten.
        std::fs::write(
            dir.path().join("etc/nix/nix.conf.local"),
            "sandbox = true\n",
        )
        .unwrap();
        let sys = FakeSystem {
            exit_code: 1,
            stderr: "error: creating directory: No such file or directory",
            ran: Cell::new(false),
        };
        let outcome = probe_and_enable_nix_sandbox(&sys, "apptainer", dir.path()).unwrap();
        assert!(!outcome.enabled);
        assert!(outcome.detail.unwrap().contains("No such file"));
        let conf = std::fs::read_to_string(dir.path().join("etc/nix/nix.conf.local")).unwrap();
        assert_eq!(conf, "sandbox = false\n");
    }
}
