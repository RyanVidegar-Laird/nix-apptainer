use std::time::SystemTime;

/// Format a byte count as a human-readable string (e.g. "1.5 GB").
pub fn human_size(bytes: u64) -> String {
    const GB: u64 = 1_073_741_824;
    const MB: u64 = 1_048_576;
    const KB: u64 = 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Return the current unix epoch timestamp in seconds.
pub fn timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Check overlay usage and return a warning message if above threshold.
///
/// `on_disk_bytes` is actual disk usage (from `MetadataExt::blocks() * 512`).
/// `allocated_bytes` is the file size (from `MetadataExt::len()`), representing
/// the ext3 filesystem capacity.
/// Returns None if usage is below the threshold percentage.
pub fn overlay_usage_warning(on_disk_bytes: u64, allocated_bytes: u64, threshold_pct: u8) -> Option<String> {
    if allocated_bytes == 0 {
        return None;
    }
    let pct = (on_disk_bytes as f64 / allocated_bytes as f64 * 100.0) as u8;
    if pct >= threshold_pct {
        Some(format!(
            "Warning: Overlay is {}% full ({}/{}). Consider running 'nix-collect-garbage' or expanding the overlay (truncate + e2fsck + resize2fs).",
            pct,
            human_size(on_disk_bytes),
            human_size(allocated_bytes),
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_size() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1_048_576), "1.0 MB");
        assert_eq!(human_size(1_073_741_824), "1.0 GB");
        assert_eq!(human_size(1_610_612_736), "1.5 GB");
    }

    #[test]
    fn test_overlay_warning_message() {
        // 80% used: 40 GB on disk out of 50 GB allocated
        let on_disk = 40 * 1_073_741_824u64;
        let allocated = 50 * 1_073_741_824u64;
        let msg = overlay_usage_warning(on_disk, allocated, 80);
        assert!(msg.is_some());
        let msg = msg.unwrap();
        assert!(msg.contains("80%"));
        assert!(msg.contains("Warning"));
    }

    #[test]
    fn test_overlay_warning_under_threshold() {
        // 20% used: 10 GB on disk out of 50 GB allocated
        let on_disk = 10 * 1_073_741_824u64;
        let allocated = 50 * 1_073_741_824u64;
        let msg = overlay_usage_warning(on_disk, allocated, 80);
        assert!(msg.is_none());
    }

    #[test]
    fn test_overlay_warning_zero_allocated() {
        let msg = overlay_usage_warning(0, 0, 80);
        assert!(msg.is_none());
    }

    #[test]
    fn test_timestamp_now_is_nonzero() {
        assert!(timestamp_now() > 0);
    }
}
