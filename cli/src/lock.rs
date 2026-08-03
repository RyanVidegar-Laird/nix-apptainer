use anyhow::{Context, bail};
use nix::fcntl::{Flock, FlockArg};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

/// An exclusive advisory lock on a sandbox session.
#[derive(Debug)]
pub struct SessionLock(Flock<File>);

/// Try to take the session lock. `Ok(Some(_))` on success, `Ok(None)` when
/// `force` bypasses a held lock, `Err` when the lock is held and not forced.
pub fn acquire(path: &Path, force: bool) -> anyhow::Result<Option<SessionLock>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("Failed to open lock file: {}", path.display()))?;
    match Flock::lock(file, FlockArg::LockExclusiveNonblock) {
        Ok(mut lock) => {
            // Record our PID for diagnostics (best effort).
            let _ = lock.set_len(0);
            let _ = writeln!(&mut *lock, "{}", std::process::id());
            Ok(Some(SessionLock(lock)))
        }
        Err((_file, _errno)) => {
            let holder = std::fs::read_to_string(path).unwrap_or_default();
            let holder = holder.trim().to_string();
            if force {
                eprintln!(
                    "Warning: sandbox already in use by another session (PID {holder}); entering anyway (--force)."
                );
                Ok(None)
            } else {
                bail!(
                    "Sandbox is already in use by another session (PID {holder}).\n\
                     Concurrent writable sessions are unsynchronized outside the Nix store.\n\
                     Close the other session, or pass --force to enter anyway."
                );
            }
        }
    }
}

/// Keep the lock alive across exec(2): clear FD_CLOEXEC so the exec'd
/// process (apptainer) inherits the fd, then forget the guard so Drop
/// never unlocks. The kernel releases the flock when the fd finally closes.
pub fn hold_across_exec(lock: SessionLock) -> anyhow::Result<()> {
    use nix::fcntl::{FcntlArg, FdFlag, fcntl};
    fcntl(&*lock.0, FcntlArg::F_SETFD(FdFlag::empty()))
        .context("Failed to clear FD_CLOEXEC on session lock")?;
    std::mem::forget(lock);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // flock is per open-file-description, so two opens in one process
    // conflict — no subprocess needed to test contention.

    #[test]
    fn test_acquire_then_conflict() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("s.lock");
        let held = acquire(&path, false).unwrap();
        assert!(held.is_some());
        let err = acquire(&path, false).unwrap_err();
        assert!(err.to_string().contains("--force"), "err: {err}");
    }

    #[test]
    fn test_force_bypasses() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("s.lock");
        let _held = acquire(&path, false).unwrap();
        let forced = acquire(&path, true).unwrap();
        assert!(forced.is_none());
    }

    #[test]
    fn test_released_on_drop() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("s.lock");
        let held = acquire(&path, false).unwrap();
        drop(held);
        assert!(acquire(&path, false).unwrap().is_some());
    }

    #[test]
    fn test_lock_file_records_pid() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("s.lock");
        let _held = acquire(&path, false).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents.trim(), std::process::id().to_string());
    }
}
