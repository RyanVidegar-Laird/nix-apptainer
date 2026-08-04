//! Progress display for the sandbox unpack: `apptainer build --sandbox`
//! is silent for minutes, so a background thread watches the target
//! directory grow. TTY: indicatif bar (percentage when the SIF carries
//! unpacked-size metadata). Non-TTY (batch logs): a periodic plain line.
//! Display problems must never affect the unpack itself.

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

/// Watches `dir` grow on a background thread until `finish`/drop.
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
        let handle = std::thread::spawn(move || watch(&thread_dir, expected_bytes, &thread_stop));
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

fn watch(dir: &Path, expected_bytes: Option<u64>, stop: &AtomicBool) {
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
    let mut last_walk = Instant::now() - Duration::from_secs(60);
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(200));
        if last_walk.elapsed() < Duration::from_secs(2) {
            continue;
        }
        last_walk = Instant::now();
        let written = dir_disk_usage(dir);
        if let Some(pb) = &bar {
            pb.set_position(written.min(expected_bytes.unwrap_or(u64::MAX)));
        } else if last_line.elapsed() >= Duration::from_secs(30) {
            last_line = Instant::now();
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
}
