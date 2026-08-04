//! Progress display for the sandbox unpack: `apptainer build --sandbox`
//! is silent for minutes, so a background thread watches the unpack grow.
//! TTY: indicatif bar (percentage when the SIF carries unpacked-size
//! metadata), repositioned every couple of seconds and repainted every
//! 120ms. Non-TTY (batch logs): a plain line after 5s, then every 30s.
//! Display problems must never affect the unpack itself.
//!
//! Note what gets measured — see `unpack_bytes_written`. Apptainer stages
//! into a sibling of the destination and renames at the end, so watching
//! the destination alone shows nothing until the unpack is over.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Best-effort on-disk bytes of a tree. Hardlinked inodes count once
/// (matching `du` and the build-time metadata); symlinks not followed.
pub fn dir_disk_usage(root: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    let mut seen_inodes = std::collections::HashSet::new();
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            // DirEntry::metadata does not traverse symlinks.
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                stack.push(entry.path());
            }
            if meta.nlink() > 1 && !seen_inodes.insert((meta.dev(), meta.ino())) {
                continue;
            }
            total += meta.blocks() * 512;
        }
    }
    total
}

/// Prefix apptainer gives its staging directory: `bundle.NewBundle` calls
/// `os.MkdirTemp(<parent of destination>, "build-temp-")`.
const STAGING_PREFIX: &str = "build-temp-";

/// Staging directories already present next to the destination. Taken
/// before the unpack starts: an interrupted earlier run can leave one
/// behind holding gigabytes, which would otherwise read as instant progress.
pub fn staging_snapshot(parent: &Path) -> std::collections::HashSet<PathBuf> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return std::collections::HashSet::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(STAGING_PREFIX))
        })
        .collect()
}

/// Bytes written so far by an in-flight unpack.
///
/// `apptainer build --sandbox` does NOT write into the destination: it
/// stages into `<parent>/build-temp-<random>/` and renames into place at
/// the very end (a sibling, so the rename stays on one filesystem). So the
/// destination reads 0 for the entire unpack and only jumps at completion.
/// Count the destination *plus* any staging directory that appeared after
/// `pre_existing` was snapshotted. Falls back to the destination alone if
/// no staging directory is found, so an apptainer that names it differently
/// degrades to a static bar rather than breaking.
pub fn unpack_bytes_written(dest: &Path, pre_existing: &std::collections::HashSet<PathBuf>) -> u64 {
    let mut total = dir_disk_usage(dest);
    let Some(parent) = dest.parent() else {
        return total;
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return total;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_staging = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(STAGING_PREFIX));
        if !is_staging || pre_existing.contains(&path) {
            continue;
        }
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            total += dir_disk_usage(&path);
        }
    }
    total
}

/// Watches the unpack grow on a background thread until `finish`/drop.
pub struct UnpackProgress {
    stop: Arc<AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
    started: Instant,
    dir: PathBuf,
}

impl UnpackProgress {
    pub fn start(dir: PathBuf, expected_bytes: Option<u64>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread_dir = dir.clone();
        // Snapshot before the unpack starts, so only this run's staging counts.
        let pre = dir.parent().map(staging_snapshot).unwrap_or_default();
        let handle =
            std::thread::spawn(move || watch(&thread_dir, expected_bytes, &thread_stop, &pre));
        Self {
            stop,
            handle: Some(handle),
            started: Instant::now(),
            dir,
        }
    }

    /// Stop the display and print a one-line summary (also the signal in
    /// non-TTY logs that the unpack completed and how big it was).
    pub fn finish(mut self) {
        self.stop_thread();
        let written = dir_disk_usage(&self.dir);
        let secs = self.started.elapsed().as_secs();
        println!(
            "  Unpacked {} in {}m {}s",
            crate::util::human_size(written),
            secs / 60,
            secs % 60
        );
    }

    fn stop_thread(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for UnpackProgress {
    fn drop(&mut self) {
        self.stop_thread();
    }
}

fn watch(
    dir: &Path,
    expected_bytes: Option<u64>,
    stop: &AtomicBool,
    pre_existing: &std::collections::HashSet<PathBuf>,
) {
    let tty = std::io::stderr().is_terminal();
    let started = Instant::now();
    let bar = tty.then(|| {
        let pb = match expected_bytes {
            Some(total) => {
                let pb = indicatif::ProgressBar::new(total);
                pb.set_style(
                    indicatif::ProgressStyle::with_template(
                        "  [{bar:40.cyan/blue}] {percent}% \u{b7} {bytes}/{total_bytes} \u{b7} {elapsed}",
                    )
                    .expect("hardcoded progress bar template")
                    .progress_chars("##-"),
                );
                pb
            }
            None => {
                let pb = indicatif::ProgressBar::new_spinner();
                pb.set_style(
                    indicatif::ProgressStyle::with_template(
                        "  {spinner} {bytes} written \u{b7} {elapsed}",
                    )
                    .expect("hardcoded spinner template"),
                );
                pb
            }
        };
        pb.enable_steady_tick(Duration::from_millis(120));
        pb
    });
    let mut last_line = Instant::now();
    // Report early once so a batch log shows the unpack is alive, then settle
    // into a quiet cadence.
    let mut line_interval = Duration::from_secs(5);
    // Re-walk cadence, adapted to what the walk actually costs. A sandbox
    // tree is hundreds of thousands of files; on shared HPC storage a single
    // recursive stat pass can take seconds. Backing off to 4x the measured
    // walk time keeps the display under ~20% of the thread's time so it
    // cannot compete for I/O with the unpack it is watching.
    const MIN_WALK_INTERVAL: Duration = Duration::from_secs(2);
    let mut walk_interval = MIN_WALK_INTERVAL;
    // None means "walk on the first tick". Not `Instant::now() - 60s`:
    // Instant is monotonic-since-boot, so that underflows and panics when
    // the process starts within 60s of boot (exactly the VM-test case).
    let mut last_walk: Option<Instant> = None;
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
        if last_walk.is_some_and(|t| t.elapsed() < walk_interval) {
            continue;
        }
        let walk_started = Instant::now();
        let written = unpack_bytes_written(dir, pre_existing);
        // Stamped AFTER the walk: measuring from the start would let a walk
        // slower than the interval run back-to-back with no pause at all.
        last_walk = Some(Instant::now());
        walk_interval = MIN_WALK_INTERVAL.max(walk_started.elapsed() * 4);
        if let Some(pb) = &bar {
            pb.set_position(written.min(expected_bytes.unwrap_or(u64::MAX)));
        } else if last_line.elapsed() >= line_interval {
            last_line = Instant::now();
            line_interval = Duration::from_secs(30);
            println!(
                "  still unpacking: {} written ({}s)",
                crate::util::human_size(written),
                started.elapsed().as_secs()
            );
        }
    }
    if let Some(pb) = bar {
        pb.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dir_disk_usage_counts_hardlinks_once() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a");
        std::fs::write(&a, vec![0u8; 8192]).unwrap();
        std::fs::hard_link(&a, dir.path().join("b")).unwrap();
        let usage = dir_disk_usage(dir.path());
        // Two dir entries, one inode: must count ~8 KB, not ~16 KB.
        assert!(usage >= 8192, "usage {usage}");
        assert!(usage < 16384, "hardlink counted twice: {usage}");
    }

    #[test]
    fn test_dir_disk_usage_missing_dir_is_zero() {
        assert_eq!(dir_disk_usage(std::path::Path::new("/nonexistent-xyz")), 0);
    }

    /// Build a staging dir like apptainer's, holding one file of `bytes`.
    fn staging(parent: &Path, name: &str, bytes: usize) -> PathBuf {
        let d = parent.join(name);
        std::fs::create_dir_all(d.join("rootfs")).unwrap();
        std::fs::write(d.join("rootfs/blob"), vec![0u8; bytes]).unwrap();
        d
    }

    #[test]
    fn test_unpack_bytes_counts_staging_dir_while_dest_absent() {
        // The bug: apptainer stages into <parent>/build-temp-*/ and renames
        // into place only at the end, so watching dest alone reads 0 for the
        // entire unpack and the progress bar never moves.
        let tmp = tempfile::TempDir::new().unwrap();
        let dest = tmp.path().join("sandbox");
        let pre = staging_snapshot(tmp.path());
        staging(tmp.path(), "build-temp-2150350840", 16384);
        assert!(!dest.exists(), "dest must still be absent");
        let seen = unpack_bytes_written(&dest, &pre);
        assert!(seen >= 16384, "staging bytes not counted: {seen}");
    }

    #[test]
    fn test_unpack_bytes_ignores_stale_staging_from_interrupted_run() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dest = tmp.path().join("sandbox");
        staging(tmp.path(), "build-temp-stale", 16384);
        // Snapshot taken after the stale dir exists — as create_sandbox does.
        let pre = staging_snapshot(tmp.path());
        assert_eq!(unpack_bytes_written(&dest, &pre), 0);
        // A new staging dir alongside the stale one still counts.
        staging(tmp.path(), "build-temp-fresh", 16384);
        assert!(unpack_bytes_written(&dest, &pre) >= 16384);
    }

    #[test]
    fn test_unpack_bytes_counts_dest_after_rename() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dest = tmp.path().join("sandbox");
        let pre = staging_snapshot(tmp.path());
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("blob"), vec![0u8; 16384]).unwrap();
        assert!(unpack_bytes_written(&dest, &pre) >= 16384);
    }

    #[test]
    fn test_unpack_bytes_ignores_unrelated_siblings() {
        // The real parent is the data dir: base.sif and overlay/ live there
        // and must not be mistaken for unpack progress.
        let tmp = tempfile::TempDir::new().unwrap();
        let dest = tmp.path().join("sandbox");
        let pre = staging_snapshot(tmp.path());
        std::fs::write(tmp.path().join("base.sif"), vec![0u8; 65536]).unwrap();
        std::fs::create_dir_all(tmp.path().join("overlay")).unwrap();
        std::fs::write(tmp.path().join("overlay/upper"), vec![0u8; 65536]).unwrap();
        assert_eq!(unpack_bytes_written(&dest, &pre), 0);
    }
}
