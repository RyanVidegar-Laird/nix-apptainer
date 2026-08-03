# Bioinformatics Example

A multi-environment flake demonstrating R, Python, and samtools dev shells
for use inside nix-apptainer.

## New to Nix?

The 60-second version:

- **Nix** is a package manager plus a small config language: you
  describe an environment in a file, Nix builds exactly that — today,
  and identically in two years.
- **nixpkgs** is its package collection (100k+ packages, including
  CRAN/Bioconductor R packages and Python libraries).
- A **derivation** is Nix's build recipe: fixed inputs in, identical
  result out. Every package is one.
- A **flake** (like `flake.nix` here) is just a standard structure
  around this: declare *inputs* (nixpkgs) and *outputs* (the dev
  shells below), and any flake can be used the same way.

Learn more: [nix.dev](https://nix.dev) (official tutorials) ·
[Zero to Nix](https://zero-to-nix.com) (gentle intro) ·
[package search](https://search.nixos.org) ·
[Nix reference manual](https://nixos.org/manual/nix/stable/)

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

## About the nixpkgs pin

`flake.nix` pins `nixos-26.05` to match the container image's nixpkgs
release. Check the container's release with
`nix eval --raw nixpkgs#lib.trivial.release` and keep the pin in sync
when you update the base image — mismatched versions cause confusing
build failures.

On ARM clusters, change `x86_64-linux` to `aarch64-linux` in `flake.nix`.
