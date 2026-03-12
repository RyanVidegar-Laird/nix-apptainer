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
