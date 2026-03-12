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
