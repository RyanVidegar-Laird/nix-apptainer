# nix-apptainer

[![built with garnix](https://img.shields.io/endpoint.svg?url=https%3A%2F%2Fgarnix.io%2Fapi%2Fbadges%2FRyanVidegar-Laird%2Fnix-apptainer)](https://garnix.io/repo/RyanVidegar-Laird/nix-apptainer)

Apptainer container image with a minimal NixOS system and single-user Nix for HPC environments. This acts as a shim / portable shell where a persistent, writable `/nix/store` is available and `nix` commands (including flakes) work out of the box.

## Quick start

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
nix-apptainer enter -B /scratch:/scratch # bind mounts
nix-apptainer enter --quiet              # suppress apptainer warnings
nix-apptainer exec -- nix develop        # run a single command
```

## How it works

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

### config.toml reference

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

## Distributing to teammates

```bash
# Copy the static CLI binary and SIF to a shared location
cp nix-apptainer /shared/containers/
cp base-nixos.sif /shared/containers/

# Teammates then:
/shared/containers/nix-apptainer init --sif /shared/containers/base-nixos.sif
/shared/containers/nix-apptainer enter
```

## Examples

- [examples/bioinformatics/](examples/bioinformatics/) — Multi-environment flake with R, Python, and samtools dev shells with direnv auto-loading
- [examples/home-manager/](examples/home-manager/) — Integrating home-manager with the container overlay

## Development

```bash
nix develop              # shell with apptainer, rust toolchain, etc.
nix build .#sandbox      # build just the rootfs directory (for debugging)
nix build                # build the full .sif image
nix build .#cli          # build the static CLI binary
nix flake check          # run all checks (eval, shellcheck, sandbox, sif, cli tests)
```

### Build from source

```bash
git clone https://github.com/RyanVidegar-Laird/nix-apptainer.git
cd nix-apptainer
nix build .#cli -o cli-result    # static CLI binary
nix build -o sif-result          # base SIF image
```

### Manual setup (shell scripts)

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
curl -LO "$REPO/SHA256SUMS" && curl -LO "$REPO/SHA256SUMS.sig"

gpg --verify SHA256SUMS.sig SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
apptainer verify "base-nixos-${ARCH}-linux.sif"
```

## Requirements

- Nix with flakes enabled (for building)
- Apptainer >= 1.1 (for running)
- FUSE support on the host (`/dev/fuse` or `fusermount`)

## Known issues

### Nix DB "not writable" on re-entry

On some systems, the second and subsequent container entries may fail with:

```
error: Nix database directory '/nix/var/nix/db' is not writable: Operation not permitted
```

The expected cause is that fuse-overlayfs's `access()` implementation checks raw mode bits without considering file ownership ([containers/fuse-overlayfs#232](https://github.com/containers/fuse-overlayfs/issues/232), [containers/fuse-overlayfs#374](https://github.com/containers/fuse-overlayfs/issues/374)). The base image sets `/nix/var/nix` to mode `0777` to accommodate this, but overlayfs copy-up during the first session reduces it to `0755` via umask. The container entrypoint restores the permissions at each startup as a workaround. Systems that use kernel overlayfs (rather than fuse-overlayfs) for the overlay merge are not affected.

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
