# Home-Manager in nix-apptainer

Use [home-manager](https://github.com/nix-community/home-manager) to
declaratively manage your shell, editor, and tools inside the container.
Changes persist in the ext3 overlay across sessions.

## How it works

nix-apptainer provides three layers of configuration:

| Layer | Source | Managed by |
|-------|--------|-----------|
| System | NixOS config baked into the SIF image | `nixos/configuration.nix` in nix-apptainer |
| User | home-manager config activated in the overlay | Your own flake (e.g. your nixos-configs) |
| Project | Per-project flakes and devShells | `flake.nix` in your project directory |

Home-manager writes to `$HOME` (dotfiles, `~/.nix-profile`, etc.), which
lives entirely in the overlay. Once activated, everything persists — no
re-activation needed on subsequent container entries.

## Prerequisites

A flake that exposes a `homeConfigurations.<name>` output targeted at
container use. This means:

- **Dynamic identity**: use `builtins.getEnv "USER"` and
  `builtins.getEnv "HOME"` so it works with whatever username the HPC
  cluster assigns you (requires `--impure` flag)
- **Headless modules only**: exclude GUI-specific modules (terminal
  emulators, desktop apps, etc.)

### Example: flake-parts with dendritic pattern

If your nixos-configs uses flake-parts, add a container "host" that
selects only the modules you want:

```nix
# modules/hosts/container/default.nix
{ inputs, config, ... }:
{
  flake.homeConfigurations.container =
    let
      pkgs = import inputs.nixpkgs {
        system = "x86_64-linux";
        overlays = [ config.flake.overlays.unstable ];
        config.allowUnfree = true;
      };
    in
    inputs.home-manager.lib.homeManagerConfiguration {
      inherit pkgs;
      modules =
        (with config.flake.modules.homeManager; [
          fish
          git
          direnv
          helix
          # ... your headless modules
        ])
        ++ [
          {
            home.stateVersion = "24.05";
            home.username =
              let u = builtins.getEnv "USER";
              in if u == "" then "nobody" else u;
            home.homeDirectory =
              let h = builtins.getEnv "HOME";
              in if h == "" then "/homeless-shelter" else h;
          }
        ];
    };
}
```

### Example: standalone home-manager flake

If you don't have an existing nixos-configs repo, create a standalone flake.

The container registers its build-time nixpkgs in the flake registry, so
you can use `flake:nixpkgs` to reuse it — avoiding a ~300 MB download of a
second nixpkgs:

```nix
# flake.nix
{
  inputs = {
    # Reuse the nixpkgs already baked into the container image.
    # Resolves via the flake registry entry set by nix-apptainer.
    nixpkgs.url = "flake:nixpkgs";
    home-manager = {
      url = "github:nix-community/home-manager/release-25.11";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, home-manager, ... }:
    let
      pkgs = import nixpkgs { system = "x86_64-linux"; };
    in {
      homeConfigurations.container =
        home-manager.lib.homeManagerConfiguration {
          inherit pkgs;
          modules = [{
            home.stateVersion = "24.05";
            home.username =
              let u = builtins.getEnv "USER";
              in if u == "" then "nobody" else u;
            home.homeDirectory =
              let h = builtins.getEnv "HOME";
              in if h == "" then "/homeless-shelter" else h;

            programs.git.enable = true;
            programs.direnv.enable = true;
            programs.fzf.enable = true;
            # Add your preferred tools here
          }];
        };
    };
}
```

Push this to GitHub and reference it in the activation command below.

> **Note:** `flake:nixpkgs` resolves to whatever nixpkgs the container was
> built with (currently nixos-25.11). Make sure your home-manager release
> matches (e.g. `release-25.11`). If you pin your own nixpkgs instead,
> home-manager will download that full closure separately.

## Activation

```bash
# Enter the container
nix-apptainer enter

# First time only — activate home-manager
nix run home-manager -- switch --flake github:youruser/yourrepo#container --impure
```

`home-manager` is not in the base image, so `nix run home-manager --`
fetches and runs it without a permanent install. After the first
activation, home-manager installs itself into your profile and you can
use `home-manager switch` directly.

Restart your shell (or run your configured shell, e.g. `fish`) to pick
up the new configuration.

## Updating

After pushing changes to your config repo:

```bash
nix-apptainer enter
home-manager switch --flake github:youruser/yourrepo#container --impure --refresh
```

The `--refresh` flag bypasses the flake cache to pick up the latest commit.

## Notes

- **Network access required**: the first activation downloads packages.
  On HPC clusters with restricted networking, consider using a
  [binary cache](https://nixos.wiki/wiki/Binary_Cache) or activating
  from a node with internet access.
- **Overlay space**: a typical home-manager activation uses 1-3 GB of
  overlay space depending on your module set. The CLI warns at 80% usage.
- **No secrets**: avoid pulling SOPS/agenix secrets into the container.
  Use `builtins.getEnv` for identity only.
- **Overlay is per-machine**: each machine gets its own overlay. Only
  the SIF image is portable.
