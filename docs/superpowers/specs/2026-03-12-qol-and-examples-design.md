# QoL Improvements and Examples

**Date**: 2026-03-12
**Status**: Draft

## Overview

A set of quality-of-life improvements to the nix-apptainer container and an examples folder demonstrating real-world usage patterns. The changes improve the default experience for interactive Nix usage, handle host-kernel diversity gracefully, and give new users a working starting point.

## Changes

### 1. Nix-output-monitor wrapper

**Problem**: Nix's raw progress output uses aggressive cursor-control escape sequences that cause visible flickering/glitching during the evaluation and metadata-fetching phases. This happens across terminals and machines.

**Solution**: A POSIX shell wrapper at `/usr/local/bin/nix` that transparently routes nom-compatible subcommands through `nix-output-monitor` on interactive terminals.

**Prerequisite**: Add `pkgs.nix` to `environment.systemPackages` so the real nix binary is available at `/run/sw/bin/nix`. The existing `/usr/local/bin/nix` symlink in `build-sandbox.nix` will be replaced by the wrapper script.

**Behavior**:
- If `NIX_APPTAINER_NO_NOM` is set (any value), exec real nix
- If stdout is not a terminal (`! [ -t 1 ]`), exec real nix
- If `nom` is not found (`! command -v nom`), exec real nix (safety fallback)
- If subcommand is one of: `build`, `develop`, `shell`, `flake`, `run` — exec `nom` with all args
- Otherwise, exec real nix at `/run/sw/bin/nix`

**Changes**:
- Add `nix-output-monitor` and `pkgs.nix` to `environment.systemPackages` in `nixos/configuration.nix`
- Replace the existing `/usr/local/bin/nix` symlink in `lib/build-sandbox.nix` with the wrapper script
- `/usr/local/bin` is already first in PATH (set in `90-environment.sh` and `entrypoint.sh`)

**Escape hatch**: `/run/sw/bin/nix` is always the real binary. `NIX_APPTAINER_NO_NOM=1` disables the wrapper globally.

### 2. Nix build sandbox with fallback and warning

**Problem**: `sandbox = false` means builds inside the container are not isolated. On hosts that support user namespaces, we should use sandboxing for build purity.

**Solution**: Enable Nix sandbox by default with graceful fallback and a suppressible warning.

**Changes to `nixos/configuration.nix`**:
- `nix.settings.sandbox = true`
- `nix.settings.sandbox-fallback = true`

**Runtime warning in `scripts/entrypoint.sh`**:
- Probe namespace support: `unshare -U true 2>/dev/null`
- If it fails, print: `Warning: Nix build sandbox unavailable (user namespaces not supported on this host). Builds will run unsandboxed.`
- Cache result in `/run/.nix-apptainer-sandbox-checked` — `/run` is a tmpfs inside the container, so the cache is per-session (cleared when the container exits), unlike `/tmp` which may be bind-mounted from the host
- If `NIX_APPTAINER_NO_SANDBOX_WARN` is set, suppress the warning

### 3. Custom image building (documentation only)

**Problem**: Users may want to customize the base image (add packages, modify NixOS config, use home-manager) without having Nix on the host.

**Solution**: Document the workflow — no code changes needed.

**Workflow**:
1. Enter container, clone/fork the nix-apptainer repo
2. Modify `nixos/configuration.nix` (or add home-manager config)
3. Run `nix build` inside the container (apptainer and squashfsTools are pulled in as build deps by the flake)
4. Copy the resulting `.sif` out via a bind-mounted path
5. Exit, use the new `.sif` with the host's apptainer

**Note**: Testing the new `.sif` inside the container (nested apptainer) requires nested user namespace support on the host kernel. This works automatically with Apptainer 1.1+ and sufficient `max_user_namespaces` — no special configuration needed.

**Deliverable**: `docs/custom-image-building.md` — standalone doc with step-by-step instructions.

### 4. TERMINFO_DIRS enrichment

**Problem**: Terminals like kitty, alacritty, wezterm ship their own terminfo. The container only searches `/run/sw/share/terminfo`, so these terminals fall back to `xterm-256color` even when the host has the correct terminfo installed and visible via bind mounts.

**Solution**: Extend `TERMINFO_DIRS` in `90-environment.sh` to include common host paths:
```
export TERMINFO_DIRS="/run/sw/share/terminfo:/usr/share/terminfo:/usr/lib/terminfo"
```

Apptainer bind-mounts system directories by default, so host paths are often visible. If they don't exist, ncurses silently skips them. The TERM fallback in entrypoint.sh remains as a safety net.

**Change**: `lib/build-sandbox.nix` — update the `90-environment.sh` content.

### 5. Overlay disk usage warning

**Problem**: A full overlay causes cryptic write errors. Users on HPC clusters may not check `nix-apptainer status` regularly.

**Solution**: Check overlay usage before container launch and warn at 80% capacity.

**Implementation in CLI (`enter.rs` / `exec.rs`)**:
- Before exec'ing apptainer, check overlay usage by comparing actual disk usage (`MetadataExt::blocks() * 512`) against the allocated file size (`MetadataExt::len()`), matching the approach used by the `status` command. Note: `statvfs` on the overlay file returns the *host* filesystem stats, not the ext3 internals — we use file metadata instead.
- If >= 80%, print: `Warning: Overlay is 84% full (42.0/50.0 GB). Consider running 'nix-collect-garbage' or expanding the overlay (truncate + e2fsck + resize2fs; see docs/custom-image-building.md).` (used/total derived dynamically from statvfs)
- No suppression env var — if you're about to run out of disk, you should know

**Overlay expansion**: Overlays can be expanded without recreation via `truncate` + `e2fsck` + `resize2fs`. The warning message and `docs/custom-image-building.md` will document this procedure. A CLI subcommand for this (`overlay resize`) is deferred to a future round.

### 6. `--quiet` flag for enter/exec

**Problem**: Apptainer prints stderr warnings (localtime mount, overlay warnings) that clutter output.

**Solution**: Plumb Apptainer's native `--quiet` flag through the CLI.

**Changes**:
- Add `--quiet` / `-q` flag to `enter` and `exec` CLI subcommands (clap args in `main.rs`, plumbed via flags structs)
- When set, pass `--quiet` to apptainer in `container.rs`'s `build_apptainer_args()` — note: `--quiet` is a global apptainer flag and must be inserted **before** the subcommand (`run`/`exec`) in the args vec
- Add `quiet` field (default `false`) to the `[enter]` section of `config.toml`, with `#[serde(default)]`
- Config value is overridden by the CLI flag

### 7. Examples folder with nix-direnv

**What**: A bioinformatics example flake demonstrating multiple devShells with direnv auto-loading.

**Base image change** (`nixos/configuration.nix`):
- Add `programs.direnv.enable = true` — installs direnv, nix-direnv, and configures shell hooks automatically

**Example structure**:
```
examples/
└── bioinformatics/
    ├── flake.nix       # Multiple devShells
    ├── .envrc           # use flake
    └── README.md
```

**devShells in `flake.nix`** (using flake-utils):
- `r` — R with rPackages: dplyr, tidyr, ggplot2
- `python` — Python with pythonPackages: numpy, pandas
- `samtools` — samtools
- `full` — all of the above combined
- `default` — aliases `full`

**README covers**:
- What each shell provides and how to use it (`nix develop .#r`, etc.)
- Direnv auto-loading: `cd examples/bioinformatics` → direnv prompts to allow → environment auto-loads
- How to extend (add packages)
- Note on future RStudio/Jupyter examples

## Files modified

| File | Change |
|------|--------|
| `nixos/configuration.nix` | Add `pkgs.nix`, `nix-output-monitor` to systemPackages; `sandbox = true`, `sandbox-fallback = true`; `programs.direnv.enable = true` |
| `lib/build-sandbox.nix` | Replace `/usr/local/bin/nix` symlink with nom wrapper script; update `TERMINFO_DIRS` in `90-environment.sh` |
| `scripts/entrypoint.sh` | Sandbox probe + warning |
| `cli/src/main.rs` | Add `--quiet` / `-q` clap arg to `Enter` and `Exec` variants |
| `cli/src/commands/enter.rs` | Plumb `quiet` flag from args; overlay usage warning via file metadata |
| `cli/src/commands/exec.rs` | Plumb `quiet` flag from args; overlay usage warning via file metadata |
| `cli/src/container.rs` | Insert `--quiet` before subcommand in `build_apptainer_args()` |
| `cli/src/config.rs` | Add `quiet: bool` field (default false) to enter config |
| `tests/sandbox-structure.nix` | Verify nom wrapper at `/usr/local/bin/nix` is a script (not symlink); verify `TERMINFO_DIRS` |
| `tests/shellcheck.nix` | Ensure nom wrapper script is included in shellcheck scope |

## Files added

| File | Purpose |
|------|---------|
| `examples/bioinformatics/flake.nix` | Multi-shell bioinformatics devenv |
| `examples/bioinformatics/.envrc` | `use flake` for direnv auto-loading |
| `examples/bioinformatics/README.md` | Usage guide with direnv workflow |
| `docs/custom-image-building.md` | Custom image building workflow + overlay expansion docs |

## Out of scope

- CLI `overlay resize` subcommand (future round)
- RStudio / Jupyter notebook examples (future round)
- Home-manager profile integration (future round)
- `--fakeroot` / privileged nested Apptainer (not needed for the Nix-based build workflow)
