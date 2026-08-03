# Home-Manager in nix-apptainer

[home-manager](https://github.com/nix-community/home-manager) manages
your shell, tools, and dotfiles from one config file. Inside
nix-apptainer everything it installs persists in the overlay, so you
activate once and it stays.

## Layers

| Layer | Source | Managed by |
|-------|--------|-----------|
| System | NixOS config baked into the SIF image | nix-apptainer |
| User | home-manager config activated in the overlay | you (this example) |
| Project | Per-project flakes and dev shells | `flake.nix` in each project |

## The config

[`flake.nix`](flake.nix) in this directory is a complete working
config: git, direnv, and fzf, with fish as the interactive shell and
`gs`/`ga`/`gd` abbreviations for common git commands.

```nix
{
  description = "Home-manager config for use inside nix-apptainer";

  inputs = {
    # Must match the container's nixpkgs release (check inside the
    # container: nix eval --raw nixpkgs#lib.trivial.release).
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    home-manager = {
      url = "github:nix-community/home-manager/release-26.05";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, home-manager, ... }:
    let
      # Change to "aarch64-linux" on ARM clusters.
      pkgs = import nixpkgs { system = "x86_64-linux"; };
    in {
      homeConfigurations.container =
        home-manager.lib.homeManagerConfiguration {
          inherit pkgs;
          modules = [{
            home.stateVersion = "26.05";
            home.username =
              let u = builtins.getEnv "USER";
              in if u == "" then "nobody" else u;
            home.homeDirectory =
              let h = builtins.getEnv "HOME";
              in if h == "" then "/homeless-shelter" else h;

            programs.git.enable = true;
            programs.direnv.enable = true;
            programs.fzf.enable = true;

            # fish as the interactive shell, with a few git shortcuts
            programs.fish = {
              enable = true;
              shellAbbrs = {
                gs = "git status";
                ga = "git add";
                gd = "git diff";
              };
            };
            # The container always starts bash; hand off interactive
            # sessions to fish (chsh isn't available here).
            programs.bash = {
              enable = true;
              initExtra = ''
                if [[ $- == *i* ]]; then exec fish; fi
              '';
            };
            # Add your preferred tools here
          }];
        };
    };
}
```

Copy this directory somewhere you control (or push it to GitHub) and
edit the module list to taste. On ARM clusters change `x86_64-linux`
to `aarch64-linux`.

## About these values

- **nixpkgs pin** must match the container's nixpkgs release —
  mismatched versions (e.g. home-manager 26.05 against nixpkgs
  unstable) cause hard-to-diagnose build failures.
- **home-manager branch** must match the same release.
  Check from inside the container:

  ```bash
  nix eval --raw nixpkgs#lib.trivial.release   # e.g. "26.05" → release-26.05
  ```

  Update this pin when you update the base image.
- **`home.stateVersion`** is set once when you first create your config
  (use the release current at that time) and then never changed — even
  across image or home-manager upgrades. It records which defaults your
  config started with; it is not a version to keep updated.
- **`home.username` / `home.homeDirectory`** read `$USER` and `$HOME`
  at activation, so one config works with whatever account each cluster
  gives you. This is why activation needs `--impure`.
- **fish handoff**: the container always starts bash; the small
  `programs.bash` block replaces interactive bash sessions with fish.
  Delete it if you prefer bash.

## Activate

The container's home directory is isolated from the host, so
bind-mount the directory containing your config:

```bash
nix-apptainer enter -B /path/to/config:/path/to/config
nix run github:nix-community/home-manager/release-26.05 -- switch --flake /path/to/config#container --impure
```

Or straight from GitHub:

```bash
nix-apptainer enter
nix run github:nix-community/home-manager/release-26.05 -- switch --flake github:you/repo#container --impure
```

(The pinned `nix run` URL matters: a bare `nix run home-manager` fetches
home-manager *master*, which won't match this config's release.)

> **Note:** the current image requires disabling the Nix build sandbox
> before anything can build — once, inside the container:
> ```bash
> mkdir -p ~/.config/nix
> echo "sandbox = false" >> ~/.config/nix/nix.conf
> ```
> Details in [docs/troubleshooting.md](../../docs/troubleshooting.md).

The first activation installs home-manager into your profile; after
that, use `home-manager switch ...` directly. Re-enter the container to
land in fish — try `gs`<kbd>space</kbd>, it expands to `git status`.

> **Tip:** add frequently used bind mounts to `config.toml`:
> ```toml
> [enter]
> bind = ["/home/you/configs:/home/you/configs"]
> ```

## Update

After changing your config:

```bash
home-manager switch --flake github:you/repo#container --impure --refresh
```

`--refresh` bypasses the flake cache so the latest commit is used.

## Notes

- **Network required** for the first activation. On restricted
  clusters, activate from a node with internet access.
- **Overlay space**: a typical activation uses 1–3 GB.
- **No secrets**: don't pull SOPS/agenix secrets into the container.
- **Per-machine overlay**: only the SIF image is portable; each machine
  gets its own overlay (and its own activation).
