use std::path::Path;
use std::process::Command;

/// Abstraction over external system interactions for testability.
///
/// Production code uses `RealSystem`. Tests inject a mock.
pub trait System {
    /// Run an external command, return its exit status.
    fn run_command(&self, program: &str, args: &[&str])
    -> anyhow::Result<std::process::ExitStatus>;
    /// Check if a command exists on PATH. Returns the command name if found.
    fn find_command(&self, name: &str) -> Option<String>;
    /// Run a command with a version flag and return stdout if successful.
    /// Used for detecting tools and their versions.
    fn command_version(&self, name: &str, flag: &str) -> Option<String>;
    /// Query available disk space at the given path, in bytes.
    fn available_disk_bytes(&self, path: &Path) -> Option<u64>;
    /// Check if a path exists on the filesystem.
    fn path_exists(&self, path: &Path) -> bool;
    /// Resolve a command name to its on-disk path via $PATH (symlinks followed).
    fn resolve_command_path(&self, name: &str) -> Option<std::path::PathBuf>;
    /// statfs(2) f_type magic for the filesystem holding `path`.
    fn filesystem_magic(&self, path: &Path) -> Option<i64>;
}

/// Real system interactions — delegates to actual OS commands.
pub struct RealSystem;

impl System for RealSystem {
    fn run_command(
        &self,
        program: &str,
        args: &[&str],
    ) -> anyhow::Result<std::process::ExitStatus> {
        Command::new(program)
            .args(args)
            .status()
            .map_err(Into::into)
    }

    fn find_command(&self, name: &str) -> Option<String> {
        Command::new(name)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|_| name.to_string())
    }

    fn command_version(&self, name: &str, flag: &str) -> Option<String> {
        let output = Command::new(name).arg(flag).output().ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            None
        }
    }

    fn available_disk_bytes(&self, path: &Path) -> Option<u64> {
        let stat = nix::sys::statvfs::statvfs(path).ok()?;
        Some(stat.fragment_size() as u64 * stat.blocks_available() as u64)
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn resolve_command_path(&self, name: &str) -> Option<std::path::PathBuf> {
        let path_var = std::env::var_os("PATH")?;
        std::env::split_paths(&path_var)
            .map(|d| d.join(name))
            .find(|c| c.is_file())
            .map(|c| std::fs::canonicalize(&c).unwrap_or(c))
    }

    fn filesystem_magic(&self, path: &Path) -> Option<i64> {
        let stat = nix::sys::statfs::statfs(path).ok()?;
        Some(stat.filesystem_type().0 as i64)
    }
}
