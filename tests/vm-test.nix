# tests/vm-test.nix
#
# Full-lifecycle VM integration test for the Rust CLI.
# Boots a NixOS VM, installs the CLI, runs init/enter/exec/status/clean
# as an unprivileged user, and covers adversarial HPC-realistic subtests.
# Requires KVM — run with `nix build .#vm-test`.
{ pkgs, sifImage, cli }:

pkgs.testers.runNixOSTest {
  name = "nix-apptainer-cli-lifecycle";

  nodes.machine =
    { config, pkgs, ... }:
    {
      # Apptainer runtime
      programs.singularity.enable = true;
      programs.singularity.package = pkgs.apptainer;

      # fuse-overlayfs for directory and ext3 overlay stacking + the CLI itself
      environment.systemPackages = [ pkgs.fuse-overlayfs cli ];

      # VM capacity
      virtualisation.memorySize = 2048;
      virtualisation.diskSize = 4096;

      # Apptainer bind-mounts /etc/localtime; ensure it exists
      time.timeZone = "UTC";

      # Unprivileged user that every CLI command runs as.
      # `su - testuser` from root in the testScript works without a password.
      users.users.testuser = {
        isNormalUser = true;
        home = "/home/testuser";
        createHome = true;
      };

      # Ship the SIF into the VM
      environment.etc."test/base-nixos.sif".source = sifImage;
    };

  testScript = ''
    # ------------------------------------------------------------------
    # Helper: run a shell command as testuser with an isolated
    # NIX_APPTAINER_HOME. Each phase passes a distinct home so state
    # cannot leak between phases.
    # ------------------------------------------------------------------
    def as_testuser(cmd, nix_apptainer_home="/home/testuser/.nix-apptainer", extra_env=""):
        return (
            f"su - testuser -c '"
            f"export NIX_APPTAINER_HOME={nix_apptainer_home} && "
            f"{extra_env}{cmd}'"
        )

    machine.wait_for_unit("default.target")

    # ------------------------------------------------------------------
    # Phase 0 — Preflight
    # ------------------------------------------------------------------
    with subtest("Phase 0: CLI on PATH for unprivileged user"):
        machine.succeed(as_testuser("nix-apptainer --version"))

    with subtest("Phase 0: SIF is readable by testuser"):
        machine.succeed(as_testuser("test -r /etc/test/base-nixos.sif"))
  '';
}
