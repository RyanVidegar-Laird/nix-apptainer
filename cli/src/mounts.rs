//! Mount-point discovery and seeding for sandbox mode.
//!
//! In `--writable` mode apptainer cannot fabricate missing mount points,
//! so site-configured binds are silently skipped ("Skipping mount ...").
//! This module parses those warnings, and pre-creates mount points inside
//! the unpacked sandbox tree. We only create paths — the site's
//! apptainer.conf (or our own --bind/--mount args) does the mounting.

use std::path::Path;

/// Container path named by one apptainer mount-failure line, if any.
///
/// Apptainer reports a missing mount point three different ways depending on
/// how the bind was configured and whether a layer is available (formats
/// taken verbatim from its `starter` binary):
///
///   Skipping mount %s [%s]: %s doesn't exist in container
///   By using --writable, Apptainer can't create %s destination automatically…
///   …while mounting %s: destination %s doesn't exist in container
fn missing_mount_path(line: &str) -> Option<&str> {
    // Ordered: the "Skipping mount" line also ends in "doesn't exist in
    // container", so it must be matched before the generic destination form.
    if let Some(rest) = line.split("Skipping mount ").nth(1) {
        return rest.split_whitespace().next();
    }
    if let Some(rest) = line.split("can't create ").nth(1) {
        return rest.split_whitespace().next();
    }
    if line.contains("doesn't exist in container")
        && let Some(rest) = line.split("destination ").nth(1)
    {
        return rest.split_whitespace().next();
    }
    None
}

/// Extract container paths apptainer reported as missing mount points.
/// Deduplicates, preserves order, absolute paths only.
pub fn parse_skipped_mounts(stderr: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in stderr.lines() {
        let Some(path) = missing_mount_path(line) else {
            continue;
        };
        if path.starts_with('/') && seen.insert(path.to_string()) {
            out.push(path.to_string());
        }
    }
    out
}

/// Container-side destination of a --bind entry ("src", "src:dst", "src:dst:opts").
pub fn bind_dest(entry: &str) -> Option<String> {
    let mut parts = entry.split(':');
    let src = parts.next()?;
    let dst = parts.next().unwrap_or(src);
    dst.starts_with('/').then(|| dst.to_string())
}

/// Container-side destination of a long-form --mount entry.
pub fn mount_dest(entry: &str) -> Option<String> {
    entry.split(',').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        matches!(k.trim(), "dest" | "destination" | "dst" | "target").then(|| v.trim().to_string())
    })
}

/// What `seed_path` did.
pub enum Seeded {
    /// Created a directory mirroring a host directory.
    Dir,
    /// Created an empty file mirroring a host file.
    File,
    /// Host path missing — created a directory and the caller should warn.
    MissingHostDir,
    /// Target already present in the sandbox — nothing to do.
    Exists,
}

/// Create `container_path` inside the sandbox, mirroring the host path's
/// type (file vs directory). The host path is stat'd because for site
/// binds src == dst, so the host tells us which kind the mount expects.
pub fn seed_path(sandbox_dir: &Path, container_path: &str) -> anyhow::Result<Seeded> {
    let target = sandbox_dir.join(container_path.trim_start_matches('/'));
    if target.symlink_metadata().is_ok() {
        return Ok(Seeded::Exists);
    }
    match std::fs::metadata(container_path) {
        Ok(m) if m.is_file() => {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::File::create(&target)?;
            Ok(Seeded::File)
        }
        Ok(_) => {
            std::fs::create_dir_all(&target)?;
            Ok(Seeded::Dir)
        }
        Err(_) => {
            std::fs::create_dir_all(&target)?;
            Ok(Seeded::MissingHostDir)
        }
    }
}

/// Seed a list of container paths, printing warnings instead of failing —
/// a missing mount point degrades to a skipped mount, same as today.
pub fn seed_paths(sandbox_dir: &Path, paths: &[String]) {
    for p in paths {
        match seed_path(sandbox_dir, p) {
            Ok(Seeded::MissingHostDir) => {
                eprintln!(
                    "Warning: {p} does not exist on the host; created a directory mount point anyway."
                );
            }
            Ok(_) => {}
            Err(e) => eprintln!("Warning: could not create mount point {p}: {e}"),
        }
    }
}

/// Launch a throwaway container against the unpacked sandbox and collect
/// the mount points apptainer itself says it skipped. Runtime warnings are
/// the only source that covers every mechanism (bind path, hostfs, ...).
/// Never fails — discovery is a convenience; empty on any error.
pub fn discover_missing_mount_points(
    sys: &dyn crate::system::System,
    apptainer: &str,
    sandbox_dir: &Path,
) -> Vec<String> {
    // --writable is essential, not incidental: without it apptainer stacks an
    // overlay/underlay over the sandbox and silently *fabricates* every
    // missing mount point, so the probe reports nothing to create. Real
    // sessions run --writable, where it cannot, and warns instead.
    //
    // /bin/sh, not /bin/true: the image ships a minimal /bin. Apptainer emits
    // its mount warnings while setting up the container, before exec'ing the
    // command, but a missing binary would make the failure look like ours.
    let dir = sandbox_dir.to_string_lossy();
    match sys.run_command_capture(
        apptainer,
        &["exec", "--writable", dir.as_ref(), "/bin/sh", "-c", "true"],
    ) {
        Ok(out) => parse_skipped_mounts(&String::from_utf8_lossy(&out.stderr)),
        Err(_) => Vec::new(),
    }
}

/// Ensure every configured mount target exists in the sandbox:
/// mount_points, bind destinations (config + flags), long-form mount
/// destinations. Idempotent and cheap — called before every sandbox launch.
pub fn ensure_mount_points(
    sandbox_dir: &Path,
    cfg: &crate::config::EnterConfig,
    extra_binds: &[String],
) {
    let mut targets: Vec<String> = cfg.mount_points.clone();
    targets.extend(cfg.bind.iter().filter_map(|b| bind_dest(b)));
    targets.extend(extra_binds.iter().filter_map(|b| bind_dest(b)));
    targets.extend(cfg.mount.iter().filter_map(|m| mount_dest(m)));
    seed_paths(sandbox_dir, &targets);
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_parse_skipped_mounts() {
        let stderr = "\
WARNING: Skipping mount /datastore [hostfs]: /datastore doesn't exist in container
WARNING: Skipping mount /share [hostfs]: /share doesn't exist in container
WARNING: Skipping mount /etc/site.conf [binds]: /etc/site.conf doesn't exist in container
INFO:    unrelated line
WARNING: Skipping mount /datastore [hostfs]: duplicate
";
        assert_eq!(
            parse_skipped_mounts(stderr),
            vec!["/datastore", "/share", "/etc/site.conf"]
        );
    }

    #[test]
    fn test_parse_writable_cannot_create_destination() {
        // Verbatim from apptainer's starter binary. This is what a --writable
        // session emits when no layer is available to fabricate the target.
        let stderr = "\
WARNING: By using --writable, Apptainer can't create /datastore destination automatically without overlay or underlay
WARNING: No layer in use (overlay or underlay), check your configuration, Apptainer can't create /share destination automatically without overlay or underlay
";
        assert_eq!(parse_skipped_mounts(stderr), vec!["/datastore", "/share"]);
    }

    #[test]
    fn test_parse_fatal_destination_form() {
        let stderr = "FATAL:   container creation failed: mount hook function failure: \
mount /host/src->/sitedata error: while mounting /host/src: \
destination /sitedata doesn't exist in container\n";
        assert_eq!(parse_skipped_mounts(stderr), vec!["/sitedata"]);
    }

    #[test]
    fn test_parse_skipped_mounts_ignores_non_absolute() {
        let stderr = "WARNING: Skipping mount proc [kernel]: not supported\n";
        assert!(parse_skipped_mounts(stderr).is_empty());
    }

    #[test]
    fn test_bind_dest() {
        assert_eq!(bind_dest("/data"), Some("/data".to_string()));
        assert_eq!(bind_dest("/src:/dst"), Some("/dst".to_string()));
        assert_eq!(bind_dest("/src:/dst:ro"), Some("/dst".to_string()));
        assert_eq!(bind_dest("relative:also-relative"), None);
    }

    #[test]
    fn test_mount_dest() {
        assert_eq!(
            mount_dest("type=bind,source=/data,dest=/mnt,ro"),
            Some("/mnt".to_string())
        );
        assert_eq!(
            mount_dest("type=bind,src=/a,destination=/b"),
            Some("/b".to_string())
        );
        assert_eq!(mount_dest("type=bind,source=/data,ro"), None);
    }

    #[test]
    fn test_seed_path_mirrors_host_dir() {
        let sandbox = TempDir::new().unwrap();
        // /tmp exists on the host and is a directory
        let kind = seed_path(sandbox.path(), "/tmp").unwrap();
        assert!(matches!(kind, Seeded::Dir));
        assert!(sandbox.path().join("tmp").is_dir());
    }

    #[test]
    fn test_seed_path_mirrors_host_file() {
        let sandbox = TempDir::new().unwrap();
        let host = TempDir::new().unwrap();
        let host_file = host.path().join("site.conf");
        std::fs::write(&host_file, b"x").unwrap();
        let kind = seed_path(sandbox.path(), host_file.to_str().unwrap()).unwrap();
        assert!(matches!(kind, Seeded::File));
        let target = sandbox
            .path()
            .join(host_file.to_str().unwrap().trim_start_matches('/'));
        assert!(target.is_file());
    }

    #[test]
    fn test_seed_path_missing_host_creates_dir() {
        let sandbox = TempDir::new().unwrap();
        let kind = seed_path(sandbox.path(), "/nonexistent-host-path-xyz").unwrap();
        assert!(matches!(kind, Seeded::MissingHostDir));
        assert!(sandbox.path().join("nonexistent-host-path-xyz").is_dir());
    }

    #[test]
    fn test_seed_path_existing_target_untouched() {
        let sandbox = TempDir::new().unwrap();
        std::fs::create_dir_all(sandbox.path().join("tmp")).unwrap();
        let kind = seed_path(sandbox.path(), "/tmp").unwrap();
        assert!(matches!(kind, Seeded::Exists));
    }

    #[test]
    fn test_ensure_mount_points_covers_all_sources() {
        let sandbox = TempDir::new().unwrap();
        let cfg = crate::config::EnterConfig {
            mount_points: vec!["/tmp".to_string()],
            bind: vec!["/srcdir:/bounddir".to_string()],
            mount: vec!["type=bind,source=/x,dest=/mountdir".to_string()],
            ..crate::config::EnterConfig::default()
        };
        ensure_mount_points(sandbox.path(), &cfg, &["/flagdir".to_string()]);
        assert!(sandbox.path().join("tmp").is_dir());
        assert!(sandbox.path().join("bounddir").is_dir());
        assert!(sandbox.path().join("mountdir").is_dir());
        assert!(sandbox.path().join("flagdir").is_dir());
    }

    struct StderrSystem(&'static str);

    impl crate::system::System for StderrSystem {
        fn run_command(&self, _: &str, _: &[&str]) -> anyhow::Result<std::process::ExitStatus> {
            unimplemented!("discovery uses run_command_capture")
        }
        fn run_command_capture(&self, _: &str, _: &[&str]) -> anyhow::Result<std::process::Output> {
            use std::os::unix::process::ExitStatusExt;
            Ok(std::process::Output {
                status: std::process::ExitStatus::from_raw(0),
                stdout: Vec::new(),
                stderr: self.0.as_bytes().to_vec(),
            })
        }
        fn find_command(&self, _: &str) -> Option<String> {
            None
        }
        fn command_version(&self, _: &str, _: &str) -> Option<String> {
            None
        }
        fn available_disk_bytes(&self, _: &Path) -> Option<u64> {
            None
        }
        fn path_exists(&self, _: &Path) -> bool {
            false
        }
        fn resolve_command_path(&self, _: &str) -> Option<std::path::PathBuf> {
            None
        }
        fn filesystem_magic(&self, _: &Path) -> Option<i64> {
            None
        }
    }

    #[test]
    fn test_discover_missing_mount_points() {
        let sys = StderrSystem(
            "WARNING: Skipping mount /datastore [hostfs]: /datastore doesn't exist in container\n",
        );
        let found = discover_missing_mount_points(&sys, "apptainer", Path::new("/sb"));
        assert_eq!(found, vec!["/datastore"]);
    }

    #[test]
    fn test_seed_paths_never_panics_on_unwritable_sandbox() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
        seed_paths(dir.path(), &["/tmp".to_string()]);
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}
