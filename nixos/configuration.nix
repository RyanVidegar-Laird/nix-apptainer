# nixos/configuration.nix
{
  config,
  lib,
  pkgs,
  modulesPath,
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

  nix.settings = {
    sandbox = false;
    sandbox-fallback = false;
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
  ];

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
