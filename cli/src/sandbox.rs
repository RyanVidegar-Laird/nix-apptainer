use anyhow::{Context, bail};
use std::path::Path;

use crate::system::System;

/// Probe expression: a trivial derivation with no inputs, buildable
/// offline. If this builds with sandbox=true, the Nix build sandbox
/// works on this host (nested userns + bind mounts on a plain tree).
const PROBE_EXPR: &str = r#"derivation { name = "sandbox-probe"; system = builtins.currentSystem; builder = "/bin/sh"; args = [ "-c" "echo ok > $out" ]; }"#;

/// Args for `apptainer build --sandbox <dir> <sif>` — rootless SIF unpack.
pub fn build_sandbox_args(dir: &str, sif: &str) -> Vec<String> {
    vec![
        "build".to_string(),
        "--sandbox".to_string(),
        dir.to_string(),
        sif.to_string(),
    ]
}

/// Args for the sandboxed-build probe. HOME=/tmp keeps Nix's eval cache
/// off the (possibly unmounted) host home; /usr/bin/env is used instead
/// of apptainer's --env flag for old-runtime compatibility.
pub fn probe_args(dir: &str) -> Vec<String> {
    vec![
        "exec".to_string(),
        "--writable".to_string(),
        dir.to_string(),
        "/usr/bin/env".to_string(),
        "HOME=/tmp".to_string(),
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
    let status = sys
        .run_command(apptainer, &argrefs)
        .with_context(|| format!("Failed to run {apptainer} build --sandbox"))?;
    if !status.success() {
        bail!(
            "{apptainer} build --sandbox failed (exit code: {:?})",
            status.code()
        );
    }
    Ok(())
}

/// Run the probe; on success write /etc/nix/nix.conf.local inside the
/// sandbox (picked up via the image's `!include`), enabling sandbox = true.
/// Returns whether the Nix build sandbox is now enabled.
pub fn probe_and_enable_nix_sandbox(
    sys: &dyn System,
    apptainer: &str,
    dir: &Path,
) -> anyhow::Result<bool> {
    let dir_str = dir.to_string_lossy();
    let args = probe_args(&dir_str);
    let argrefs: Vec<&str> = args.iter().map(String::as_str).collect();
    let ok = sys
        .run_command(apptainer, &argrefs)
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        std::fs::write(dir.join("etc/nix/nix.conf.local"), "sandbox = true\n")
            .context("Failed to write etc/nix/nix.conf.local")?;
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_sandbox_args() {
        assert_eq!(
            build_sandbox_args("/data/sandbox", "/data/base.sif"),
            vec!["build", "--sandbox", "/data/sandbox", "/data/base.sif"]
        );
    }

    #[test]
    fn test_probe_args_shape() {
        let args = probe_args("/data/sandbox");
        assert_eq!(args[0], "exec");
        assert_eq!(args[1], "--writable");
        assert_eq!(args[2], "/data/sandbox");
        assert!(args.contains(&"nix-build".to_string()));
        // sandbox=true must be forced regardless of baked config
        let i = args.iter().position(|a| a == "sandbox").unwrap();
        assert_eq!(args[i - 1], "--option");
        assert_eq!(args[i + 1], "true");
        assert!(args.last().unwrap().contains("sandbox-probe"));
    }
}
