# tests/vm-test.nix
#
# Tier 3: Full lifecycle VM integration test.
# Boots a NixOS VM with Apptainer, runs setup/enter/overlay lifecycle.
# Requires KVM — run with `nix build .#vm-test`.
{ pkgs, sifImage, scripts }:

pkgs.testers.runNixOSTest {
  name = "nix-apptainer-lifecycle";

  nodes.machine =
    { config, pkgs, ... }:
    {
      # Enable Apptainer in the VM (NixOS uses programs.singularity with apptainer package)
      programs.singularity.enable = true;
      programs.singularity.package = pkgs.apptainer;

      # Provide fuse-overlayfs for ext3 overlay mounting
      environment.systemPackages = [ pkgs.fuse-overlayfs ];

      # Enough disk and memory for the test
      virtualisation.memorySize = 2048;
      virtualisation.diskSize = 4096;

      # Apptainer tries to bind-mount /etc/localtime; ensure it exists
      time.timeZone = "UTC";

      # Copy SIF image and scripts into the VM
      environment.etc."test/base-nixos.sif".source = sifImage;
      environment.etc."test/setup.sh" = {
        source = "${scripts}/setup.sh";
        mode = "0755";
      };
      environment.etc."test/enter.sh" = {
        source = "${scripts}/enter.sh";
        mode = "0755";
      };
    };

  testScript = ''
    machine.wait_for_unit("default.target")

    sif = "/etc/test/base-nixos.sif"
    work = "/tmp/test-workdir"

    # Create a working directory with the SIF and scripts
    machine.succeed("mkdir -p " + work)
    machine.succeed(f"cp {sif} {work}/base-nixos.sif")
    machine.succeed(f"cp /etc/test/setup.sh {work}/setup.sh")
    machine.succeed(f"cp /etc/test/enter.sh {work}/enter.sh")

    with subtest("Basic container execution"):
        result = machine.succeed(
            f"apptainer exec {work}/base-nixos.sif /bin/sh -c 'echo hello'"
        )
        assert "hello" in result, f"Expected 'hello' in output, got: {result}"

    with subtest("setup.sh --help"):
        machine.succeed(
            f"NIX_APPTAINER_SIF={work}/base-nixos.sif bash {work}/setup.sh --help"
        )

    with subtest("setup.sh fails on missing SIF"):
        machine.fail(
            f"NIX_APPTAINER_SIF=/nonexistent.sif bash {work}/setup.sh"
        )

    with subtest("setup.sh creates overlay and initializes DB"):
        # Use the minimum 64MB overlay — enough for DB init, will exhaust on installs
        machine.succeed(
            f"cd {work} && NIX_APPTAINER_SIF={work}/base-nixos.sif "
            f"bash {work}/setup.sh --size 64 --sif {work}/base-nixos.sif --overlay {work}/overlay.img"
        )
        # Verify overlay was created
        machine.succeed(f"test -f {work}/overlay.img")

    with subtest("Nix store has paths after setup"):
        result = machine.succeed(
            f"apptainer exec --overlay {work}/overlay.img {work}/base-nixos.sif "
            f"/usr/local/bin/nix path-info --all 2>/dev/null | wc -l"
        )
        path_count = int(result.strip())
        assert path_count > 0, f"Expected store paths > 0, got {path_count}"

    with subtest("enter.sh --help"):
        machine.succeed(
            f"NIX_APPTAINER_SIF={work}/base-nixos.sif "
            f"NIX_APPTAINER_OVERLAY={work}/overlay.img "
            f"bash {work}/enter.sh --help"
        )

    with subtest("enter.sh fails on missing SIF"):
        machine.fail(
            f"NIX_APPTAINER_SIF=/nonexistent.sif "
            f"NIX_APPTAINER_OVERLAY={work}/overlay.img "
            f"bash {work}/enter.sh"
        )

    with subtest("enter.sh exec runs command in container"):
        result = machine.succeed(
            f"NIX_APPTAINER_SIF={work}/base-nixos.sif "
            f"NIX_APPTAINER_OVERLAY={work}/overlay.img "
            f"bash {work}/enter.sh exec /bin/sh -c 'echo container-works'"
        )
        assert "container-works" in result, f"Expected 'container-works', got: {result}"

    with subtest("Persistence across container restarts"):
        result2 = machine.succeed(
            f"apptainer exec --overlay {work}/overlay.img {work}/base-nixos.sif "
            f"/usr/local/bin/nix path-info --all 2>/dev/null | wc -l"
        )
        path_count2 = int(result2.strip())
        assert path_count2 == path_count, (
            f"Path count changed: {path_count} -> {path_count2}"
        )

    with subtest("Overlay exhaustion does not corrupt store"):
        # With only 64MB overlay (most already used by DB), try to build something.
        # This should fail (ENOSPC) but not corrupt the existing DB.
        machine.fail(
            f"apptainer exec --overlay {work}/overlay.img {work}/base-nixos.sif "
            f"/usr/local/bin/nix build --no-link --expr "
            "'\"(import <nixpkgs> {{}}).hello\"' "
            "2>/dev/null"
        )

        # Verify existing paths are still queryable (no corruption)
        result3 = machine.succeed(
            f"apptainer exec --overlay {work}/overlay.img {work}/base-nixos.sif "
            f"/usr/local/bin/nix path-info --all 2>/dev/null | wc -l"
        )
        path_count3 = int(result3.strip())
        assert path_count3 > 0, "Store corrupted after overlay exhaustion"
  '';
}
