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

Exit the container, then point the CLI at your custom image. Use a
separate data directory so it doesn't clash with your existing setup:

```bash
export NIX_APPTAINER_HOME=/scratch/$USER/custom
nix-apptainer init --sif /scratch/my-custom-image.sif
nix-apptainer enter
```

This copies the SIF into place and creates a fresh directory overlay
(the default).

## Testing inside the container (nested Apptainer)

If the host kernel supports nested user namespaces (check
`cat /proc/sys/user/max_user_namespaces` — must be > 1), you can test
your image inside the container:

```bash
apptainer exec result/nix-apptainer.sif nix --version
```

This requires no special configuration — Apptainer 1.1+ handles
`--userns` nesting automatically.

## Expanding an existing overlay (ext3 only)

Directory overlays (the default) have no fixed size — this section only
applies if you chose `--overlay-type ext3`. If your ext3 overlay is
running out of space, expand it without recreating it:

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
