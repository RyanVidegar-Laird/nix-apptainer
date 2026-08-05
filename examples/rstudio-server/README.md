# RStudio Server Example

Run a private, unprivileged RStudio Server inside nix-apptainer and
use it from your laptop's browser through an SSH tunnel.

## Usage

Inside the container:

```bash
cd examples/rstudio-server
nix develop        # first run downloads the RStudio closure (~1-2 GB)
rstudio-start
```

`rstudio-start` picks a free port, generates a session password (or
uses `$RSTUDIO_PASSWORD` if set), and prints the exact tunnel command,
e.g.:

```
On your laptop:
  ssh -L 8787:localhost:8790 you@login02.cluster.edu
then open http://localhost:8787 and log in with the values above.
```

Log in with your cluster username and the printed password.

## Ports

- Default: scan from 8787 upward and use the first free port.
- `RSTUDIO_PORT=48231 rstudio-start`: use exactly that port, failing
  if it's taken. Picking one random high port and exporting
  `RSTUDIO_PORT` in your shell config gives you a stable tunnel
  command you can keep in one long-lived SSH session.

## Why a password?

`rserver` listens on `127.0.0.1` only, but on a shared node every
local user can reach localhost ports — the password keeps your
session yours.

## State

Everything lives under `~/.local/share/rstudio-server`, so sessions
persist across container restarts and nothing touches system paths.

## Adding R packages

Edit the `packages` list in `flake.nix`:

```nix
rstudio = pkgs.rstudioServerWrapper.override {
  packages = with pkgs.rPackages; [
    dplyr
    tidyr
    ggplot2
    DESeq2
  ];
};
```

Then restart: `exit` the dev shell, `nix develop`, `rstudio-start`.

## About the nixpkgs pin

`flake.nix` pins `nixos-26.05` to match the container image's nixpkgs
release (check inside the container:
`nix eval --raw nixpkgs#lib.trivial.release`); keep it in sync when
you update the base image. On ARM clusters, change `x86_64-linux` to
`aarch64-linux`.
