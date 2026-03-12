# QoL Improvements and Examples Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add nix-output-monitor integration, Nix sandbox fallback with warnings, overlay usage warnings, `--quiet` flag, TERMINFO enrichment, direnv support, and a bioinformatics example flake.

**Architecture:** Changes span three layers: NixOS config (packages + settings), container build (sandbox assembly + wrapper scripts), and the Rust CLI (flags + overlay checks). Each task is self-contained and testable independently.

**Tech Stack:** Nix (NixOS modules, derivations), Bash (entrypoint/wrapper scripts), Rust (clap CLI), flake-utils (example)

**Spec:** `docs/superpowers/specs/2026-03-12-qol-and-examples-design.md`

---

## Chunk 1: NixOS config + container build changes

### Task 1: Add packages and settings to NixOS configuration

**Files:**
- Modify: `nixos/configuration.nix`

- [ ] **Step 1: Add nix-output-monitor, nix, and direnv to configuration**

In `nixos/configuration.nix`, make these changes:

1. Add `nix-output-monitor` to `environment.systemPackages`:

```nix
environment.systemPackages = with pkgs; [
  coreutils
  bashInteractive
  git
  cachix
  ripgrep
  fd
  gnused
  gawk
  helix
  curl
  wget
  nano
  ncurses
  nix-output-monitor
];
```

2. Change sandbox settings:

```nix
nix.settings = {
  sandbox = true;
  sandbox-fallback = true;
  filter-syscalls = true;
  experimental-features = [
    "nix-command"
    "flakes"
  ];
  max-jobs = "auto";
};
```

3. Add direnv after the `programs.bash` block:

```nix
programs.direnv.enable = true;
```

- [ ] **Step 2: Verify NixOS evaluation still works**

Run: `nix flake check --no-build 2>&1 | head -5`

This runs the eval check without building. Expected: no evaluation errors.

If this fails because `--no-build` is not supported, use: `nix build .#checks.x86_64-linux.eval`

- [ ] **Step 3: Commit**

```bash
git add nixos/configuration.nix
git commit -m "feat: add nom, direnv, sandbox settings to NixOS config

Enable nix build sandbox with fallback, add nix-output-monitor
and programs.direnv.enable for improved UX."
```

---

### Task 2: Nom wrapper script and TERMINFO_DIRS in build-sandbox.nix

**Files:**
- Modify: `lib/build-sandbox.nix`

The existing `build-sandbox.nix` creates a symlink at `/usr/local/bin/nix` → `${nix}/bin/nix` (line 107). We replace that with a wrapper script. The real nix binary is available at `/run/sw/bin/nix` because the NixOS `nix` module adds `pkgs.nix` to `environment.systemPackages` by default (it's how `nix.settings` works). After making changes, verify `/run/sw/bin/nix` exists in the sandbox output (`ls -la result/run/sw/bin/nix`).

- [ ] **Step 1: Replace nix symlink with wrapper script**

In `lib/build-sandbox.nix`, replace line 107:
```nix
ln -s ${nix}/bin/nix $sandbox/usr/local/bin/nix
```

With a wrapper script. The tricky part is writing a shell script inside a Nix `'' ... ''` string. Use `cat > ... <<'WRAPPER'` to avoid shell expansion in the heredoc. **Important**: `${...}` inside Nix multi-line strings is Nix interpolation — escape with `''${...}` to get literal `${...}` in the output. However, since we use a **quoted heredoc** (`<<'WRAPPER'`), the heredoc body is literal shell text passed through `cat`. The Nix `''` string still sees the `${...}` patterns though, so we MUST escape them:

```nix
    # Nom wrapper — routes nom-compatible subcommands through nix-output-monitor
    # on interactive terminals. Bypass: NIX_APPTAINER_NO_NOM=1 or /run/sw/bin/nix
    cat > $sandbox/usr/local/bin/nix <<'WRAPPER'
#!/bin/sh
real_nix="/run/sw/bin/nix"

# Bypass conditions: env var, non-interactive, nom not found
if [ -n "''${NIX_APPTAINER_NO_NOM:-}" ] || ! [ -t 1 ] || ! command -v nom >/dev/null 2>&1; then
    exec "$real_nix" "$@"
fi

# Only route nom-compatible subcommands through nom
case "''${1:-}" in
    build|develop|shell|flake|run)
        exec nom "$@"
        ;;
    *)
        exec "$real_nix" "$@"
        ;;
esac
WRAPPER
    chmod +x $sandbox/usr/local/bin/nix
```

Note: `''${...}` is the Nix escape syntax to produce literal `${...}` in the output. The `<<'WRAPPER'` prevents shell expansion, but Nix interpolation still applies to the string content. The other nix binaries (`nix-store`, `nix-env`, etc.) keep their existing symlinks to `${nix}/bin/`. Only the `nix` wrapper changes.

- [ ] **Step 1.5: Verify /run/sw/bin/nix exists in sandbox**

After building, verify the NixOS module provides nix at `/run/sw/bin/`:
```bash
nix build .#sandbox && ls -la result/run/sw/bin/nix
```
If this fails (nix not at `/run/sw/bin/nix`), add `pkgs.nix` explicitly to `environment.systemPackages` in Task 1 and rebuild.

- [ ] **Step 2: Update TERMINFO_DIRS in 90-environment.sh**

In the same file, update the `90-environment.sh` content (around line 124-131). Change:

```
export TERMINFO_DIRS="/run/sw/share/terminfo"
```

To:

```
export TERMINFO_DIRS="/run/sw/share/terminfo:/usr/share/terminfo:/usr/lib/terminfo"
```

- [ ] **Step 3: Build the sandbox to verify**

Run: `nix build .#sandbox`

Expected: builds successfully. The wrapper script should be at `result/usr/local/bin/nix`.

Verify the wrapper is a script, not a symlink:
```bash
file result/usr/local/bin/nix
# Expected: "result/usr/local/bin/nix: POSIX shell script, ASCII text executable"
```

Verify TERMINFO_DIRS:
```bash
grep TERMINFO result/.singularity.d/env/90-environment.sh
# Expected: contains /usr/share/terminfo:/usr/lib/terminfo
```

- [ ] **Step 4: Commit**

```bash
git add lib/build-sandbox.nix
git commit -m "feat: add nom wrapper and enrich TERMINFO_DIRS

Replace /usr/local/bin/nix symlink with a wrapper script that
routes nom-compatible subcommands through nix-output-monitor on
interactive terminals. Add host terminfo paths to TERMINFO_DIRS."
```

---

### Task 3: Sandbox warning in entrypoint.sh

**Files:**
- Modify: `scripts/entrypoint.sh`

- [ ] **Step 1: Add sandbox probe after the chmod block**

In `scripts/entrypoint.sh`, after the `chmod -R 777 /nix/var/nix` line (line 19) and before the `# --- Execute command or interactive shell ---` comment (line 21), add:

```bash
# Warn if Nix build sandbox is unavailable (user namespaces not supported)
if [ -z "${NIX_APPTAINER_NO_SANDBOX_WARN:-}" ] && [ ! -f /run/.nix-apptainer-sandbox-checked ]; then
    if ! unshare -U true 2>/dev/null; then
        echo "Warning: Nix build sandbox unavailable (user namespaces not supported on this host). Builds will run unsandboxed." >&2
    fi
    touch /run/.nix-apptainer-sandbox-checked 2>/dev/null || true
fi
```

Key details:
- Uses `${NIX_APPTAINER_NO_SANDBOX_WARN:-}` to avoid `set -u` crash
- Caches in `/run/` (container tmpfs, not host `/tmp`)
- `touch` has `|| true` in case `/run` is read-only (unlikely but safe)
- Warning goes to stderr (`>&2`)

- [ ] **Step 2: Run shellcheck on the modified script**

Run: `shellcheck --severity=warning scripts/entrypoint.sh`

Expected: no warnings.

- [ ] **Step 3: Commit**

```bash
git add scripts/entrypoint.sh
git commit -m "feat: warn when Nix build sandbox is unavailable

Probe user namespace support on container entry. If unavailable,
print a one-time warning. Suppress with NIX_APPTAINER_NO_SANDBOX_WARN."
```

---

### Task 4: Update sandbox structure tests

**Files:**
- Modify: `tests/sandbox-structure.nix`

- [ ] **Step 1: Update the symlink test for /usr/local/bin/nix**

The test currently checks that `usr/local/bin/nix` is a symlink pointing into `/nix/store/`. It needs to change to check that it's a regular executable file (the wrapper script), not a symlink.

In `tests/sandbox-structure.nix`, remove `usr/local/bin/nix` from the symlink check loop (lines 37-52). Then add a new check after the symlink section:

```bash
# --- Nom wrapper must be a script, not a symlink ---
[ -f "$sb/usr/local/bin/nix" ] || fail "usr/local/bin/nix missing"
[ -x "$sb/usr/local/bin/nix" ] || fail "usr/local/bin/nix not executable"
[ ! -L "$sb/usr/local/bin/nix" ] || fail "usr/local/bin/nix should be a script, not a symlink"
head -1 "$sb/usr/local/bin/nix" | grep -q "^#!/bin/sh" || fail "usr/local/bin/nix missing shebang"
pass "usr/local/bin/nix is a nom wrapper script"
```

Also add a TERMINFO_DIRS check:

```bash
# --- 90-environment.sh must have enriched TERMINFO_DIRS ---
grep -q "/usr/share/terminfo" "$sb/.singularity.d/env/90-environment.sh" \
  || fail "90-environment.sh missing /usr/share/terminfo in TERMINFO_DIRS"
pass "90-environment.sh has enriched TERMINFO_DIRS"
```

- [ ] **Step 2: Run the updated test**

Run: `nix build .#checks.x86_64-linux.sandbox-structure`

Expected: all checks pass.

- [ ] **Step 3: Commit**

```bash
git add tests/sandbox-structure.nix
git commit -m "test: update sandbox tests for nom wrapper and TERMINFO_DIRS"
```

---

### Task 5: Run full check suite

- [ ] **Step 1: Run nix flake check**

Run: `nix flake check`

This runs eval, shellcheck, sandbox-structure, sif-contents, and CLI tests. Expected: all pass.

If shellcheck fails on the nom wrapper, it won't be caught here since the wrapper is embedded in build-sandbox.nix (not in scripts/). The shellcheck test only covers files in `scripts/`. The wrapper will be checked structurally by the sandbox-structure test (shebang check).

- [ ] **Step 2: Fix any failures and re-run**

- [ ] **Step 3: Commit any fixes**

---

## Chunk 2: CLI changes (--quiet flag + overlay warning)

### Task 6: Add `quiet` field to config

**Files:**
- Modify: `cli/src/config.rs`

- [ ] **Step 1: Write test for quiet config field**

In `cli/src/config.rs`, add a test to the existing `mod tests` block:

```rust
#[test]
fn test_quiet_default_false() {
    let config = Config::default();
    assert!(!config.enter.quiet);
}

#[test]
fn test_quiet_from_toml() {
    let mut f = NamedTempFile::new().unwrap();
    writeln!(f, r#"
[enter]
quiet = true
"#).unwrap();
    let config = Config::load(f.path()).unwrap();
    assert!(config.enter.quiet);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd cli && cargo test test_quiet`

Expected: compilation error — `quiet` field doesn't exist yet.

- [ ] **Step 3: Add quiet field to EnterConfig**

In `cli/src/config.rs`, add the `quiet` field to `EnterConfig`:

```rust
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct EnterConfig {
    /// GPU passthrough mode
    #[serde(default)]
    pub gpu: GpuMode,
    /// Bind mounts in "src:dst" format
    #[serde(default)]
    pub bind: Vec<String>,
    /// Suppress apptainer stderr warnings
    #[serde(default)]
    pub quiet: bool,
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd cli && cargo test test_quiet`

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add cli/src/config.rs
git commit -m "feat: add quiet field to EnterConfig"
```

---

### Task 7: Add `--quiet` flag to CLI and plumb through container.rs

**Files:**
- Modify: `cli/src/main.rs`
- Modify: `cli/src/commands/enter.rs`
- Modify: `cli/src/commands/exec.rs`
- Modify: `cli/src/container.rs`

- [ ] **Step 1: Write test for --quiet in container args**

In `cli/src/container.rs`, add a test:

```rust
#[test]
fn test_quiet_flag() {
    let paths = test_paths();
    let config = test_config();
    let opts = ContainerOpts {
        paths: &paths,
        config: &config,
        nv: false,
        rocm: false,
        bind: &[],
        passthrough: &[],
        quiet: true,
    };
    let args = build_apptainer_args(&opts, ContainerMode::Run);
    // --quiet must come before the subcommand
    assert_eq!(args[0], "--quiet");
    assert_eq!(args[1], "run");
}

#[test]
fn test_no_quiet_by_default() {
    let paths = test_paths();
    let config = test_config();
    let opts = ContainerOpts {
        paths: &paths,
        config: &config,
        nv: false,
        rocm: false,
        bind: &[],
        passthrough: &[],
        quiet: false,
    };
    let args = build_apptainer_args(&opts, ContainerMode::Run);
    assert_eq!(args[0], "run");
    assert!(!args.contains(&"--quiet".to_string()));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd cli && cargo test test_quiet`

Expected: compilation error — `quiet` not a field of `ContainerOpts`.

- [ ] **Step 3: Add quiet to ContainerOpts and build_apptainer_args**

In `cli/src/container.rs`:

1. Add `quiet` field to `ContainerOpts`:

```rust
pub struct ContainerOpts<'a> {
    pub paths: &'a AppPaths,
    pub config: &'a Config,
    pub nv: bool,
    pub rocm: bool,
    pub bind: &'a [String],
    pub passthrough: &'a [String],
    pub quiet: bool,
}
```

2. Insert `--quiet` before the mode in `build_apptainer_args`:

```rust
pub fn build_apptainer_args(opts: &ContainerOpts, mode: ContainerMode) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    // Global flags (must come before subcommand)
    if opts.quiet {
        args.push("--quiet".to_string());
    }

    // Mode
    match mode {
        ContainerMode::Run => args.push("run".to_string()),
        ContainerMode::Exec => args.push("exec".to_string()),
    }

    // ... rest unchanged
```

3. Update ALL existing tests in `container.rs` to include `quiet: false` in `ContainerOpts`. There are 5 existing tests that construct `ContainerOpts` — each needs the field added.

- [ ] **Step 4: Add --quiet flag to Enter and Exec in main.rs**

In `cli/src/main.rs`, add to both `Enter` and `Exec` variants:

```rust
/// Suppress apptainer warnings
#[arg(short, long)]
quiet: bool,
```

And plumb through in the `match` arms:

For `Enter`:
```rust
Commands::Enter {
    nv,
    rocm,
    bind,
    passthrough,
    quiet,
} => commands::enter::run(commands::enter::EnterFlags {
    nv,
    rocm,
    bind,
    passthrough,
    quiet,
}),
```

For `Exec`:
```rust
Commands::Exec {
    nv,
    rocm,
    bind,
    passthrough,
    command,
    quiet,
} => commands::exec::run(commands::exec::ExecFlags {
    nv,
    rocm,
    bind,
    passthrough,
    command,
    quiet,
}),
```

- [ ] **Step 5: Update EnterFlags, ExecFlags, and run functions**

In `cli/src/commands/enter.rs`:

Add `quiet` to `EnterFlags`:
```rust
pub struct EnterFlags {
    pub nv: bool,
    pub rocm: bool,
    pub bind: Vec<String>,
    pub passthrough: Vec<String>,
    pub quiet: bool,
}
```

In the `run` function, use `flags.quiet || config.enter.quiet` when building `ContainerOpts`:
```rust
let opts = ContainerOpts {
    paths: &paths,
    config: &config,
    nv: flags.nv,
    rocm: flags.rocm,
    bind: &flags.bind,
    passthrough: &flags.passthrough,
    quiet: flags.quiet || config.enter.quiet,
};
```

Do the same for `cli/src/commands/exec.rs` — add `quiet` to `ExecFlags` and plumb it into `ContainerOpts`.

- [ ] **Step 6: Run all CLI tests**

Run: `cd cli && cargo test`

Expected: all tests pass.

Run: `cd cli && cargo clippy -- -D warnings`

Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add cli/src/main.rs cli/src/container.rs cli/src/commands/enter.rs cli/src/commands/exec.rs
git commit -m "feat: add --quiet flag to enter and exec commands

Plumbs apptainer's --quiet flag (inserted before subcommand) through
the CLI. Configurable via --quiet/-q flag or enter.quiet in config."
```

---

### Task 8: Overlay usage warning in enter/exec

**Files:**
- Modify: `cli/src/commands/enter.rs`
- Modify: `cli/src/commands/exec.rs`
- Modify: `cli/src/util.rs` (add shared overlay check function)

**Design note**: The overlay is a sparse ext3 image file. We cannot use `statvfs` on the file (that returns the *host* filesystem stats, not the ext3 internals). Instead, we use the same approach as the `status` command: compare actual disk usage (`MetadataExt::blocks() * 512`) against the allocated file size (`MetadataExt::len()`). When on-disk usage approaches the ext3 max capacity, the overlay is filling up.

- [ ] **Step 1: Write test for overlay warning utility**

In `cli/src/util.rs`, add a test:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd cli && cargo test test_overlay_warning`

Expected: compilation error — `overlay_usage_warning` doesn't exist.

- [ ] **Step 3: Implement overlay_usage_warning in util.rs**

Add to `cli/src/util.rs`:

```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd cli && cargo test test_overlay_warning`

Expected: all three pass.

- [ ] **Step 5: Add overlay check to enter.rs and exec.rs**

In `cli/src/commands/enter.rs`, after the overlay existence check and before building `ContainerOpts`, add:

```rust
// Warn if overlay is getting full (compare actual disk usage vs allocated size)
#[cfg(unix)]
{
    use std::os::unix::fs::MetadataExt;
    if let Ok(meta) = std::fs::metadata(&paths.overlay_path) {
        let on_disk = meta.blocks() * 512;
        let allocated = meta.len();
        if let Some(warning) = crate::util::overlay_usage_warning(on_disk, allocated, 80) {
            eprintln!("{warning}");
        }
    }
}
```

Add the same block to `cli/src/commands/exec.rs` in the same location.

- [ ] **Step 6: Run all CLI tests and clippy**

Run: `cd cli && cargo test && cargo clippy -- -D warnings`

Expected: all pass, no warnings.

- [ ] **Step 7: Commit**

```bash
git add cli/src/util.rs cli/src/commands/enter.rs cli/src/commands/exec.rs
git commit -m "feat: warn when overlay usage exceeds 80%

Check overlay disk usage via statvfs before launching container.
Print a warning to stderr if >= 80% full."
```

---

## Chunk 3: Examples and documentation

### Task 9: Bioinformatics example flake

**Files:**
- Create: `examples/bioinformatics/flake.nix`
- Create: `examples/bioinformatics/.envrc`
- Create: `examples/bioinformatics/README.md`

- [ ] **Step 1: Create examples directory**

```bash
mkdir -p examples/bioinformatics
```

- [ ] **Step 2: Create flake.nix**

Create `examples/bioinformatics/flake.nix`:

```nix
{
  description = "Bioinformatics dev environment — R, Python, samtools";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        rEnv = pkgs.rWrapper.override {
          packages = with pkgs.rPackages; [
            dplyr
            tidyr
            ggplot2
          ];
        };

        pythonEnv = pkgs.python3.withPackages (ps: with ps; [
          numpy
          pandas
        ]);
      in
      {
        devShells = {
          r = pkgs.mkShell {
            packages = [ rEnv ];
            shellHook = ''
              echo "R environment loaded: dplyr, tidyr, ggplot2"
            '';
          };

          python = pkgs.mkShell {
            packages = [ pythonEnv ];
            shellHook = ''
              echo "Python environment loaded: numpy, pandas"
            '';
          };

          samtools = pkgs.mkShell {
            packages = [ pkgs.samtools ];
            shellHook = ''
              echo "samtools $(samtools --version | head -1) loaded"
            '';
          };

          full = pkgs.mkShell {
            packages = [
              rEnv
              pythonEnv
              pkgs.samtools
            ];
            shellHook = ''
              echo "Full bioinformatics environment loaded: R, Python, samtools"
            '';
          };

          default = pkgs.mkShell {
            packages = [
              rEnv
              pythonEnv
              pkgs.samtools
            ];
            shellHook = ''
              echo "Full bioinformatics environment loaded: R, Python, samtools"
            '';
          };
        };
      }
    );
}
```

- [ ] **Step 3: Create .envrc**

Create `examples/bioinformatics/.envrc`:

```
use flake
```

- [ ] **Step 4: Create README.md**

Create `examples/bioinformatics/README.md`:

```markdown
# Bioinformatics Example

A multi-environment flake demonstrating R, Python, and samtools dev shells
for use inside nix-apptainer.

## Available environments

| Shell | Command | Packages |
|-------|---------|----------|
| R | `nix develop .#r` | dplyr, tidyr, ggplot2 |
| Python | `nix develop .#python` | numpy, pandas |
| samtools | `nix develop .#samtools` | samtools |
| Full | `nix develop` | All of the above |

## Usage

### Manual

```bash
cd examples/bioinformatics
nix develop .#r       # R only
nix develop .#python  # Python only
nix develop           # everything
```

### With direnv (recommended)

The container ships with direnv and nix-direnv pre-installed. When you
`cd` into this directory, direnv will prompt you to allow the `.envrc`:

```bash
cd examples/bioinformatics
# direnv: error .envrc is blocked. Run `direnv allow` to approve its content
direnv allow
# direnv: loading .envrc
# direnv: using flake
# Full bioinformatics environment loaded: R, Python, samtools
```

After the first load, the environment is cached and activates instantly
on subsequent visits.

## Extending

To add packages, edit `flake.nix`. For example, to add `bioconductor-deseq2`
to the R environment:

```nix
rEnv = pkgs.rWrapper.override {
  packages = with pkgs.rPackages; [
    dplyr
    tidyr
    ggplot2
    BiocGenerics
    DESeq2
  ];
};
```

Then `nix develop .#r` or `direnv reload` to pick up the changes.

## Future examples

- RStudio Server with the R environment
- Jupyter notebooks with R and Python kernels
```

- [ ] **Step 5: Verify the flake evaluates**

```bash
cd examples/bioinformatics && nix flake show
```

Expected: shows devShells for r, python, samtools, full, default. Don't build (would be slow) — just verify evaluation.

- [ ] **Step 6: Commit**

```bash
git add examples/
git commit -m "feat: add bioinformatics example with R, Python, samtools

Multi-shell flake demonstrating nix develop environments with
direnv auto-loading via .envrc."
```

---

### Task 10: Custom image building documentation

**Files:**
- Create: `docs/custom-image-building.md`

- [ ] **Step 1: Write documentation**

Create `docs/custom-image-building.md`:

```markdown
# Building a Custom nix-apptainer Image

You can build a customized nix-apptainer image from inside the container
itself — no Nix installation on the host required.

## Prerequisites

- A working nix-apptainer setup (`nix-apptainer init` completed)
- A bind-mounted output directory (e.g., `--bind /scratch:/scratch`)

## Steps

### 1. Enter the container

```bash
nix-apptainer enter --bind /scratch:/scratch
```

### 2. Clone the repository

```bash
git clone https://github.com/RyanVidegar-Laird/nix-apptainer.git
cd nix-apptainer
```

### 3. Customize the configuration

Edit `nixos/configuration.nix` to add packages or change settings:

```nix
environment.systemPackages = with pkgs; [
  # ... existing packages ...
  htop
  tmux
  # Add your packages here
];
```

### 4. Build the image

```bash
nix build
```

This produces `result/nix-apptainer.sif`. The build fetches `apptainer`
and `squashfsTools` as build dependencies automatically.

### 5. Copy the image out

```bash
cp result/nix-apptainer.sif /scratch/my-custom-image.sif
```

### 6. Use the custom image

Exit the container, then use your custom image with the host apptainer:

```bash
# Create a new overlay for the custom image
apptainer overlay create --sparse --size 51200 /scratch/my-overlay.img

# Enter the custom image
apptainer run --overlay /scratch/my-overlay.img /scratch/my-custom-image.sif
```

## Testing inside the container (nested Apptainer)

If the host kernel supports nested user namespaces (check
`cat /proc/sys/user/max_user_namespaces` — must be > 1), you can test
your image inside the container:

```bash
apptainer exec result/nix-apptainer.sif nix --version
```

This requires no special configuration — Apptainer 1.1+ handles
`--userns` nesting automatically.

## Expanding an existing overlay

If your overlay is running out of space, you can expand it without
recreating it:

```bash
# Exit the container first, then on the host:

# 1. Expand the sparse file (e.g., to 100 GB)
truncate -s 100G ~/.local/share/nix-apptainer/overlay.img

# 2. Check filesystem integrity
e2fsck -f ~/.local/share/nix-apptainer/overlay.img

# 3. Resize the filesystem to fill the new space
resize2fs ~/.local/share/nix-apptainer/overlay.img
```

The overlay remains sparse — only actually-used blocks consume disk space.
```

- [ ] **Step 2: Commit**

```bash
git add docs/custom-image-building.md
git commit -m "docs: add custom image building and overlay expansion guide"
```

---

### Task 11: Final integration check

- [ ] **Step 1: Run full test suite**

```bash
nix flake check
```

Expected: all checks pass (eval, shellcheck, sandbox-structure, sif-contents, cli).

- [ ] **Step 2: Run CLI tests directly**

```bash
cd cli && cargo test && cargo clippy -- -D warnings
```

Expected: all pass, no warnings.

- [ ] **Step 3: Verify git status is clean**

```bash
git status
```

Expected: working tree clean (all changes committed).

- [ ] **Step 4: Build the full SIF image (optional, slow)**

```bash
nix build
```

This builds the complete `.sif` container image with all changes. Only needed for a final smoke test — not required for every run.
