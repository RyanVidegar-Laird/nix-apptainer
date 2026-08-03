# nix-apptainer

[![CI](https://github.com/RyanVidegar-Laird/nix-apptainer/actions/workflows/check.yml/badge.svg)](https://github.com/RyanVidegar-Laird/nix-apptainer/actions/workflows/check.yml)

Apptainer container image with a minimal NixOS system and single-user Nix for HPC environments. This acts as a shim / portable shell where a persistent, writable `/nix/store` is available and `nix` commands (including flakes) work out of the box.

## Why

I got sick of how messy dependency management can get for bioinformatics projects. Even a relatively simple one might involve various command line tools (`samtools`, `salmon`, `picard`, ...), an `R` environment (`tidyverse`, `ggplot2`, `limma`, `DESeq2`, ...), a python environment (`pandas`, `pytorch`, `scikit-learn`, ...), and some random old scripts a colleague recommended, all glued together with bash scripts or a workflow tool like `snakemake` or `nextflow`.

The chances that all required dependencies are available via, say, Conda/Mamba/Pixi are quite low. Chances that it'll be possible to resolve all conflicting versions within a single environment are even lower. One could instead [split out different environments](https://snakemake.readthedocs.io/en/latest/snakefiles/deployment.html#integrated-package-management) per analytical step (a Conda env here, Docker there, pip elsewhere, ...), yet that's a whole new abstracted dependency graph to manage.

Even after all of that, solutions like Conda still end up breaking after system upgrades or *ad hoc* installs due to dynamic linking. Good luck getting a Conda env to work again in two years.

Docker/Apptainer doesn't help much on it's own. Most Docker images in the field are *repeatable*, i.e. if you have the already-built image you can re-run it, which is a great start. However, the build process itself is rarely *reproducible* (all those arbitrary `apt-get update`s). An image built today will be different than the same image built in six months, unless one is very careful. Interactively working within one or more immutable Docker containers isn't a good development experience anyways.

Nix/Nixpkgs makes it easy to be very careful. Coupled with Apptainer's writable overlays, one can have a highly-reproducible environment on HPCs using a single configuration file, while still having an interactive development cycle.

I've been using this setup for 3+ years on an HPC with no issues. My project environments have survived many system upgrades, even a full upgrade from RHEL to Rocky Linux, and I didn't even notice. Up to producing this repo, I did so with my own messy bash scripts. This repo is simply a fancy, Rust-based 🚀, vibe-coded, convenience wrapper to make getting started easier.

## Quick Start

Download the CLI binary for your architecture from [GitHub Releases](https://github.com/RyanVidegar-Laird/nix-apptainer/releases):

```bash
ARCH=$(uname -m)  # x86_64 or aarch64
curl -Lo nix-apptainer "https://github.com/RyanVidegar-Laird/nix-apptainer/releases/latest/download/nix-apptainer-${ARCH}-linux"
chmod +x nix-apptainer
```

Set up and enter:

> **Note:** `apptainer` must be available on the system. On HPC clusters, this may require being on an interactive node.

```bash
nix-apptainer init       # downloads base image, creates writable overlay
nix-apptainer enter      # launch an interactive shell
```

Use Nix inside:

```bash
nix --version
nix build nixpkgs#hello
nix develop
```

Packages installed via Nix persist in the overlay across sessions.

### Manage

```bash
nix-apptainer status             # show current setup state
nix-apptainer update             # check for and fetch a new base image
nix-apptainer update --check     # just check, don't download
nix-apptainer clean              # interactive cleanup
nix-apptainer clean --all        # remove everything
```

### Options

```bash
nix-apptainer enter --nv                 # NVIDIA GPU passthrough
nix-apptainer enter --rocm               # AMD ROCm GPU passthrough
nix-apptainer enter -B /scratch:/scratch # bind mounts
nix-apptainer enter --quiet              # suppress apptainer warnings
nix-apptainer exec -- nix develop        # run a single command
nix-apptainer exec --passthrough <ARGS> -- <CMD>  # extra args for apptainer
```
> The `--nv`, `--rocm`, and `-B` for simply exist as shorthand for using `--passthrough <ARGS> -- <CMD>`, as they're commonly used.

## How It Works

The base image is a read-only squashfs containing a minimal NixOS system. A writable overlay stores all user modifications (installed packages, profiles, home directory). Apptainer merges them at runtime via overlayfs.

Two overlay types are supported:

- **Directory overlay** (default) — a plain directory tree. No size limit, best performance.
- **ext3 overlay** — a sparse ext3 image file. Fixed capacity, useful when sparse disk allocation is preferred.

```
base-nixos.sif (read-only)     overlay (writable)
├── /nix/store/ (base)         ├── /nix/store/ (new packages)
├── /etc/ (NixOS config)       ├── /nix/var/nix/db/
├── /bin/sh                    ├── /home/<user>/
└── /.singularity.d/           └── ...
         └──── overlayfs merge ────┘
```

By default, the host `$HOME` is **not** mounted into the container. The container gets its own home directory inside the overlay, preventing conflicts with host dotfiles and home-manager configurations. Use `--bind` to expose specific host directories (project dirs, scratch, data) as needed. Set `mount_home = true` in `config.toml` to mount the host home instead.

The Nix build sandbox is enabled with fallback — on hosts that support user namespaces, builds are isolated; otherwise they run unsandboxed with a one-time warning.

## Configuration

The CLI stores configuration in XDG directories by default:

| File | Default location | Description |
|------|-----------------|-------------|
| Config | `~/.config/nix-apptainer/config.toml` | SIF source, overlay size, GPU, bind mounts |
| Data | `~/.local/share/nix-apptainer/` | SIF image, overlay, state |
| Cache | `~/.cache/nix-apptainer/` | Download cache |

Set `NIX_APPTAINER_HOME` to consolidate everything in a single directory (useful on HPC clusters):

```bash
export NIX_APPTAINER_HOME=/scratch/$USER/nix-apptainer
```

### config.toml Reference

```toml
[sif]
source = "github"                    # "github", a URL, or a local file path
repo = "RyanVidegar-Laird/nix-apptainer"  # GitHub repo for updates

[overlay]
type = "directory"                   # "directory" (default) or "ext3"
ext3_size_mb = 51200                 # sparse overlay size in MB (ext3 only)

[enter]
gpu = "nvidia"                       # "", "nvidia", or "rocm"
bind = ["/scratch:/scratch", "/data:/data"]
quiet = false                        # suppress apptainer stderr warnings
mount_home = false                   # true to bind-mount host $HOME (default: false)
```


## Examples

- [examples/bioinformatics/](examples/bioinformatics/) — R, Python, and samtools dev shells with direnv auto-loading; includes a terse intro to Nix concepts with links to learn more
- [examples/home-manager/](examples/home-manager/) — declarative shell and tools (fish, git, direnv, fzf) that persist in the overlay

## Development

```bash
nix develop              # shell with apptainer, rust toolchain, etc.
nix build .#sandbox      # build just the rootfs directory (for debugging)
nix build                # build the full .sif image
nix build .#cli          # build the static CLI binary
nix flake check          # run all checks (eval, shellcheck, sandbox, sif, cli tests)
```

CI runs on GitHub Actions across x86_64 and aarch64: `nix flake check` on both, plus the VM lifecycle test (`nix build .#vm-test`) on the x86_64 KVM runner. Build artifacts are cached at [https://nix-apptainer.cachix.org](https://nix-apptainer.cachix.org).

### Build from Source

```bash
git clone https://github.com/RyanVidegar-Laird/nix-apptainer.git
cd nix-apptainer
nix build .#cli -o cli-result    # static CLI binary
nix build -o sif-result          # base SIF image
```

### Manual Setup (Shell Scripts)

For advanced users or environments where the CLI is not available, the shell scripts in `scripts/` provide the same functionality:

```bash
./scripts/setup.sh --sif ./base-nixos.sif    # one-time setup
./scripts/enter.sh --sif ./base-nixos.sif    # enter container
./scripts/enter.sh --nv                       # with NVIDIA GPU
./scripts/enter.sh exec nix develop           # run a command
```

### Verification

Verify signatures and checksums of release artifacts:

```bash
ARCH=$(uname -m)
REPO=https://github.com/RyanVidegar-Laird/nix-apptainer/releases/latest/download

curl -sL "$REPO/signing-key.asc" | gpg --import
curl -LO "$REPO/SHA256SUMS-${ARCH}-linux" && curl -LO "$REPO/SHA256SUMS-${ARCH}-linux.sig"

gpg --verify "SHA256SUMS-${ARCH}-linux.sig" "SHA256SUMS-${ARCH}-linux"
sha256sum --ignore-missing -c "SHA256SUMS-${ARCH}-linux"
apptainer verify "base-nixos-${ARCH}-linux.sif"
```

## Requirements

- Nix with flakes enabled (for building)
- Apptainer >= 1.1 (for running)
- FUSE support on the host (`/dev/fuse` or `fusermount`)

## Known Issues

### Nix DB "Not Writable" on Entry

On some systems, the second and subsequent container entries may fail with:

```
error: Nix database directory '/nix/var/nix/db' is not writable: Operation not permitted
```

The expected cause is that fuse-overlayfs's `access()` implementation checks raw mode bits without considering file ownership ([containers/fuse-overlayfs#232](https://github.com/containers/fuse-overlayfs/issues/232), [containers/fuse-overlayfs#374](https://github.com/containers/fuse-overlayfs/issues/374)). The base image sets `/nix/var/nix` to mode `0777` to accommodate this, but overlayfs copy-up during the first session reduces it to `0755` via umask. The container entrypoint restores the permissions at each startup as a workaround. Systems that use kernel overlayfs (rather than fuse-overlayfs) for the overlay merge are not affected.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
