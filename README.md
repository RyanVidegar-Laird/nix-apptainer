# nix-apptainer

[![built with garnix](https://img.shields.io/endpoint.svg?url=https%3A%2F%2Fgarnix.io%2Fapi%2Fbadges%2FRyanVidegar-Laird%2Fnix-apptainer)](https://garnix.io/repo/RyanVidegar-Laird/nix-apptainer)

Apptainer container image with a minimal NixOS system and single-user Nix for HPC environments. Users get a portable shell where they can `nix develop`, `nix build`, and use flakes on clusters that don't have Nix installed.

## Quick start

### Install

Download the CLI binary and base SIF for your architecture from [GitHub Releases](https://github.com/RyanVidegar-Laird/nix-apptainer/releases):

```bash
ARCH=$(uname -m)  # x86_64 or aarch64
REPO=https://github.com/RyanVidegar-Laird/nix-apptainer/releases/latest/download

curl -LO "$REPO/nix-apptainer-${ARCH}-linux"
curl -LO "$REPO/base-nixos-${ARCH}-linux.sif"
chmod +x "nix-apptainer-${ARCH}-linux"
mv "nix-apptainer-${ARCH}-linux" nix-apptainer
```

Optionally verify signatures and checksums:

```bash
curl -sL "$REPO/signing-key.asc" | gpg --import
curl -LO "$REPO/SHA256SUMS" && curl -LO "$REPO/SHA256SUMS.sig"

gpg --verify SHA256SUMS.sig SHA256SUMS
sha256sum --ignore-missing -c SHA256SUMS
apptainer verify "base-nixos-${ARCH}-linux.sif"
```

Or build from source:

```bash
git clone https://github.com/RyanVidegar-Laird/nix-apptainer.git
cd nix-apptainer
nix build .#cli -o cli-result    # static CLI binary
nix build -o sif-result          # base SIF image
```

### Set up (one-time)

```bash
# Interactive guided setup — downloads the base image, creates overlay, initializes Nix DB
nix-apptainer init

# Or non-interactive with a local SIF
nix-apptainer init --sif ./base-nixos.sif --yes

# Or with a custom data directory (useful on HPC scratch filesystems)
nix-apptainer init --data-dir /scratch/$USER/nix-apptainer --yes
```

### Enter the container

```bash
nix-apptainer enter

# With NVIDIA GPU passthrough
nix-apptainer enter --nv

# With bind mounts
nix-apptainer enter -B /scratch:/scratch

# Run a single command
nix-apptainer exec -- nix develop
```

### Use Nix inside

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

## How it works

The base image is a read-only squashfs containing a minimal NixOS system. A sparse ext3 overlay file stores all user modifications (installed packages, profiles, home directory). Apptainer merges them at runtime via overlayfs.

```
base-nixos.sif (read-only)     overlay.img (writable, sparse)
├── /nix/store/ (base)         ├── /nix/store/ (new packages)
├── /etc/ (NixOS config)       ├── /nix/var/nix/db/
├── /bin/sh                    ├── /home/<user>/
└── /.singularity.d/           └── ...
         └──── overlayfs merge ────┘
```

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
size_mb = 51200                      # sparse overlay size in MB

[enter]
gpu = "nvidia"                       # "", "nvidia", or "rocm"
bind = ["/scratch:/scratch", "/data:/data"]
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

## Manual setup (shell scripts)

For advanced users or environments where the CLI is not available, the shell scripts in `scripts/` provide the same functionality:

```bash
./scripts/setup.sh --sif ./base-nixos.sif    # one-time setup
./scripts/enter.sh --sif ./base-nixos.sif    # enter container
./scripts/enter.sh --nv                       # with NVIDIA GPU
./scripts/enter.sh exec nix develop           # run a command
```

## Development

```bash
nix develop              # shell with apptainer, rust toolchain, etc.
nix build .#sandbox      # build just the rootfs directory (for debugging)
nix build                # build the full .sif image
nix build .#cli          # build the static CLI binary
nix flake check          # run all checks (eval, shellcheck, sandbox, sif, cli tests)
```

### CLI development

```bash
cd cli
cargo test               # run unit tests
cargo clippy             # run linter
cargo run -- status      # run CLI from source
```

## Requirements

- Nix with flakes enabled (for building)
- Apptainer >= 1.1 (for running)
- FUSE support on the host (`/dev/fuse` or `fusermount`)

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.
