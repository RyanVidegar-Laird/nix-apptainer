# Bioinformatics Example

A single dev shell with R (dplyr, tidyr, ggplot2, DESeq2), Python
(numpy, pandas), and samtools.

New to Nix? Start with [nix.dev](https://nix.dev) or
[Zero to Nix](https://zero-to-nix.com); find packages at
[search.nixos.org](https://search.nixos.org).

## Usage

```bash
cd examples/bioinformatics
nix develop
```

Or with direnv (pre-installed in the container):

```bash
echo "use flake" > .envrc && direnv allow
```

## Adding packages

Edit the package lists in `flake.nix` (e.g. add `edgeR` next to
`DESeq2` in `rEnv`), then `nix develop` again or `direnv reload`.

## About the nixpkgs pin

`flake.nix` pins `nixos-26.05` to match the container image's nixpkgs
release (check inside the container:
`nix eval --raw nixpkgs#lib.trivial.release`); keep it in sync when
you update the base image. On ARM clusters, change `x86_64-linux` to
`aarch64-linux`.
