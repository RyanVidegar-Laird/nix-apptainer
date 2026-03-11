use anyhow::{bail, Context};
use std::process::Command;

use crate::checks;
use crate::paths::AppPaths;
use crate::system::RealSystem;

/// Verify the cryptographic signature of the installed SIF image.
///
/// Requires the signing key to be imported into the apptainer keyring
/// beforehand (e.g. `apptainer key pull`).
pub fn run() -> anyhow::Result<()> {
    let paths = AppPaths::resolve()?;

    if !paths.sif_path.exists() {
        bail!(
            "No SIF image found at {}. Run `nix-apptainer init` first.",
            paths.sif_path.display()
        );
    }

    let sys = RealSystem;
    let apptainer = checks::apptainer_binary(&sys)
        .context("apptainer/singularity not found")?;

    println!("Verifying SIF signature: {}", paths.sif_path.display());

    let output = Command::new(&apptainer)
        .args(["verify"])
        .arg(&paths.sif_path)
        .output()
        .with_context(|| format!("Failed to run {apptainer} verify"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !stdout.is_empty() {
        print!("{stdout}");
    }
    if !stderr.is_empty() {
        eprint!("{stderr}");
    }

    if output.status.success() {
        println!("\nSignature verification passed.");
        Ok(())
    } else {
        bail!("Signature verification failed. Have you imported the signing key? See: apptainer key pull")
    }
}
