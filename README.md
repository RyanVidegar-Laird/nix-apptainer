# nix-apptainer

[![CI](https://github.com/RyanVidegar-Laird/nix-apptainer/actions/workflows/check.yml/badge.svg)](https://github.com/RyanVidegar-Laird/nix-apptainer/actions/workflows/check.yml)

Apptainer container image with a minimal NixOS system and single-user Nix for HPC environments. This acts as a shim / portable shell where a persistent, writable `/nix/store` is available and `nix` commands (including flakes) work out of the box.

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

```
base-nixos.sif (read-only)     overlay (writable)
├── /nix/store/ (base)         ├── /nix/store/ (new packages)
├── /etc/ (NixOS config)       ├── /nix/var/nix/db/
├── /bin/sh                    ├── /home/<user>/
└── /.singularity.d/           └── ...
         └──── overlayfs merge ────┘
```

The container is **isolated from the host filesystem by default**, in every
storage mode: not your `$HOME`, not the directory you launched from, not the
filesystems your site mounts automatically. You opt back in per path.

`init` discovers what your site would otherwise mount and offers the list.
It measures this rather than reading `apptainer.conf`, because
`mount hostfs = yes` means "mount every host filesystem" and has no list to
read: `init` runs a throwaway container twice, isolated and with site
defaults, and diffs the mount targets. Only targets are read, so NFS server
names never reach your config file. Picks are recorded as ordinary binds:

```toml
[enter]
bind = ["/datastore:/datastore", "/scratch:/scratch"]
```

Your host home is just another path — opt into it the same way.

## Storage modes

`nix-apptainer init --overlay-type <mode>` picks how writes are stored:

| mode | what it is | pick it when |
|---|---|---|
| `sandbox` (default) | SIF unpacked into a writable directory — no overlay, no FUSE | the default: works on every apptainer including old ones (≤ 1.3.x, where the bundled fuse-overlayfs ≤ 1.13 makes overlay builds fail with EPERM), and the only mode where the Nix build sandbox works |
| `dir` | read-only SIF + directory overlay via fuse-overlayfs | disk or inode quota is tight (~2 GB instead of ~10 GB); needs a working fuse-overlayfs (bundled ≥ 1.14) |
| `ext3` | read-only SIF + sparse ext3 image overlay | inode-constrained parallel filesystems (Lustre/GPFS) where one big file beats many small ones |

Sandbox mode costs disk: the unpacked tree is ~10 GB and hundreds of thousands
of files, so put it on node-local or scratch storage, not a network home. It
also re-unpacks on `update` (discarding local changes), and sessions take an
advisory lock (`--force` to bypass). In exchange it needs no FUSE at all, and
local builds work everywhere apptainer runs.

The image ships with the Nix build sandbox **off** (`sandbox = false`) because
overlay-backed stores cannot support it. In sandbox mode `init` probes whether
it works here and writes the verdict (`sandbox = true` or `false`) into the
container's `/etc/nix/nix.conf.local`. A failed probe can be environmental
rather than a real capability gap, so the probe's output is printed; re-running
`nix-apptainer init` and keeping the existing sandbox re-probes.

`init` checks your host and tells you which modes will work — including
detecting the buggy bundled fuse-overlayfs on old apptainer installs.

### Site bind mounts in sandbox mode

In sandbox mode apptainer cannot create missing mount points, so a bind whose
destination does not exist is silently skipped. Destinations of `bind` and
`mount` entries are seeded for you; `mount_points` covers the remaining case —
`bind path` entries in the site's `apptainer.conf`, which nix-apptainer does
not mount itself and cannot see in the config (paths below are illustrative):

```toml
[enter]
# Paths created inside the sandbox so the site's own binds succeed.
mount_points = ["/datastore", "/share"]

# Mounts nix-apptainer passes itself:
bind = ["/scratch:/scratch"]                      # --bind (all runtimes)
mount = ["type=bind,source=/data,dest=/mnt,ro"]   # --mount (Apptainer >= 1.1)
```

Mount points are (re)created at init, after updates, and before every
`enter`/`exec`, mirroring the host path's type (directory or file).

### TMPDIR

`nix-build` creates a scratch directory under `$TMPDIR` before any build
starts, so TMPDIR must name a path the container can actually see —
`build-dir` cannot rescue an unbound one. By default nix-apptainer leaves
the host's `$TMPDIR` alone; `init` shows you the current value and lets you
keep it, pin `/tmp`, or enter a custom path:

```toml
[enter]
tmpdir = "/scratch/$USER/tmp"   # empty (default) inherits the host's TMPDIR
```

If you point it at a host scratch path, add a matching `bind` — and in
sandbox mode a `mount_points` entry — or the mount is skipped and builds
fail once they try to use it.

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
type = "sandbox"                     # "sandbox" (default), "directory", or "ext3"
ext3_size_mb = 51200                 # sparse overlay size in MB (ext3 only)

[enter]
gpu = "nvidia"                       # "", "nvidia", or "rocm"
bind = ["/datastore:/datastore"]     # host paths to mount (init discovers candidates)
mount = ["type=bind,source=/data,dest=/mnt,ro"]  # long-form --mount (Apptainer >= 1.1)
mount_points = ["/datastore"]        # paths created so site-configured binds succeed
tmpdir = ""                          # container TMPDIR; empty inherits the host's
quiet = false                        # suppress apptainer stderr warnings
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
