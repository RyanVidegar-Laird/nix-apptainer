# nixos/configuration.nix
{
  config,
  lib,
  pkgs,
  modulesPath,
  nixpkgs-input,
  ...
}:

{
  imports = [
    "${modulesPath}/profiles/minimal.nix"
  ];

  boot.isContainer = true;

  # Single-user nix — no daemon
  # container-config.nix sets NIX_REMOTE = "daemon", override it
  environment.variables.NIX_REMOTE = lib.mkForce "";

  # Expose the build-time nixpkgs source tree in the flake registry so
  # users can reference it as "flake:nixpkgs" (e.g. in home-manager configs)
  # without downloading nixpkgs again (~180 MB). The source tree is baked
  # into the squashfs image via the store path reference.
  nix.registry.nixpkgs.to = {
    type = "path";
    path = nixpkgs-input.outPath;
  };

  nix.settings = {
    sandbox = true;
    sandbox-fallback = lib.mkForce true;
    filter-syscalls = true;
    experimental-features = [
      "nix-command"
      "flakes"
    ];
    max-jobs = "auto";
  };

  environment.systemPackages = with pkgs; [
    coreutils
    bashInteractive
    git
    cachix
    ripgrep
    fd
    gnused
    gawk
    helix
    curl
    wget
    nano
    ncurses
    nix-output-monitor
    home-manager
  ];

  # Interactive shell configuration
  programs.bash = {
    promptInit = ''
      # Apptainer sets PROMPT_COMMAND to override PS1 with "Apptainer> "
      unset PROMPT_COMMAND
      if [ "$TERM" != "dumb" ]; then
        PS1='\[\e[1;34m\][nix-apptainer]\[\e[0m\] \[\e[1;32m\]\u@\h\[\e[0m\]:\[\e[1;33m\]\w\[\e[0m\]\$ '
      fi
    '';
    interactiveShellInit = ''
      # Fall back to xterm-256color if terminal type is unrecognized
      if [ -z "''${TERM:-}" ]; then
        export TERM=xterm-256color
      elif ! infocmp "$TERM" >/dev/null 2>&1; then
        export TERM=xterm-256color
      fi

      HISTSIZE=10000
      HISTFILESIZE=20000
      HISTCONTROL=ignoreboth:erasedups
      shopt -s histappend
      shopt -s globstar 2>/dev/null
    '';
  };

  programs.direnv.enable = true;

  # Minimal user setup
  users.users.nixuser = {
    isNormalUser = true;
    home = "/home/nixuser";
    shell = pkgs.bashInteractive;
  };

  # Disable features that need systemd/init
  systemd.services = { };
  networking.hostName = "nix-apptainer";

  system.stateVersion = "25.11";
}
