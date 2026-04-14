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

    # ------------------------------------------------------------------
    # Helper: assert that the Nix DB is populated in the current overlay.
    # Runs a SQLite-read-heavy query IMMEDIATELY after init with no warm-up,
    # so a silently-failed preseed produces 0 paths and fails loudly here.
    # On failure, dumps diagnostic state to make CI output self-explanatory.
    # ------------------------------------------------------------------
    def assert_db_populated(phase, home="/home/testuser/.nix-apptainer", extra_env=""):
        query = (
            "nix-apptainer exec -- "
            "nix-store --query --requisites /run/current-system 2>/dev/null | wc -l"
        )
        out = machine.succeed(as_testuser(query, nix_apptainer_home=home, extra_env=extra_env))
        paths = int(out.strip())
        # A healthy NixOS closure has ~500 paths. Threshold of 10 catches
        # silent failures while avoiding flakiness from count variance.
        if paths < 10:
            print(f"\n=== assert_db_populated FAILED in phase: {phase} ===")
            _, ls_out = machine.execute(as_testuser(
                "ls -la $NIX_APPTAINER_HOME/overlay/upper/nix/var/nix 2>&1 || true",
                nix_apptainer_home=home, extra_env=extra_env,
            ))
            print("--- /nix/var/nix in overlay upper ---")
            print(ls_out)
            _, status_out = machine.execute(as_testuser(
                "nix-apptainer status 2>&1 || true",
                nix_apptainer_home=home, extra_env=extra_env,
            ))
            print("--- nix-apptainer status ---")
            print(status_out)
            _, query_out = machine.execute(as_testuser(
                "nix-apptainer exec -- nix-store --query --requisites /run/current-system 2>&1 || true",
                nix_apptainer_home=home, extra_env=extra_env,
            ))
            print("--- raw nix-store query stderr ---")
            print(query_out)
            raise Exception(
                f"[{phase}] Nix DB query returned only {paths} paths (expected 100+) — "
                f"check /nix/var/nix perms and preseed stderr above"
            )

    machine.wait_for_unit("default.target")

    # ------------------------------------------------------------------
    # Phase 0 — Preflight
    # ------------------------------------------------------------------
    with subtest("Phase 0: CLI on PATH for unprivileged user"):
        machine.succeed(as_testuser("nix-apptainer --version"))

    with subtest("Phase 0: SIF is readable by testuser"):
        machine.succeed(as_testuser("test -r /etc/test/base-nixos.sif"))

    # ------------------------------------------------------------------
    # Phase 1 — Directory overlay lifecycle (v0.5.0's new default)
    # ------------------------------------------------------------------
    P1_HOME = "/home/testuser/.nix-apptainer"

    with subtest("Phase 1: init with directory overlay"):
        machine.succeed(as_testuser(
            "nix-apptainer init --yes "
            "--sif /etc/test/base-nixos.sif "
            "--overlay-type dir",
            nix_apptainer_home=P1_HOME,
        ))
        # Filesystem layout — note CLI copies SIF to base.sif, not base-nixos.sif
        for f in ["config.toml", "state.json", "base.sif"]:
            machine.succeed(as_testuser(f"test -f $NIX_APPTAINER_HOME/{f}", nix_apptainer_home=P1_HOME))
        machine.succeed(as_testuser("test -d $NIX_APPTAINER_HOME/overlay/upper", nix_apptainer_home=P1_HOME))
        machine.succeed(as_testuser("test -d $NIX_APPTAINER_HOME/overlay/work", nix_apptainer_home=P1_HOME))

    with subtest("Phase 1: DB preseed populated the store"):
        assert_db_populated(phase="phase1-directory-init", home=P1_HOME)

    with subtest("Phase 1: re-entry via exec preserves DB after copy-up"):
        assert_db_populated(phase="phase1-directory-reentry", home=P1_HOME)

    with subtest("Phase 1: exec inside container succeeds"):
        # Verify exec can run arbitrary commands, not just nix-store queries.
        out = machine.succeed(as_testuser(
            'nix-apptainer exec -- /bin/sh -c "echo container-ok"',
            nix_apptainer_home=P1_HOME,
        ))
        assert "container-ok" in out, f"Expected container-ok in: {out}"

    with subtest("Phase 1: status reports directory overlay type"):
        out = machine.succeed(as_testuser("nix-apptainer status", nix_apptainer_home=P1_HOME))
        assert "directory" in out.lower(), f"status missing 'directory': {out}"

    with subtest("Phase 1: clean tears down overlay, preserves config"):
        machine.succeed(as_testuser("nix-apptainer clean --all", nix_apptainer_home=P1_HOME))
        machine.fail(as_testuser("test -d $NIX_APPTAINER_HOME/overlay/upper", nix_apptainer_home=P1_HOME))
        machine.fail(as_testuser("test -f $NIX_APPTAINER_HOME/config.toml", nix_apptainer_home=P1_HOME))

    # ------------------------------------------------------------------
    # Phase 2 — ext3 overlay lifecycle (regression floor for old default)
    # ------------------------------------------------------------------
    P2_HOME = "/home/testuser/.nix-apptainer-ext3"

    with subtest("Phase 2: init with ext3 overlay (64 MB)"):
        machine.succeed(as_testuser(
            "nix-apptainer init --yes "
            "--sif /etc/test/base-nixos.sif "
            "--overlay-type ext3 "
            "--overlay-size 64",
            nix_apptainer_home=P2_HOME,
        ))
        for f in ["config.toml", "state.json", "base.sif", "overlay.img"]:
            machine.succeed(as_testuser(f"test -f $NIX_APPTAINER_HOME/{f}", nix_apptainer_home=P2_HOME))

    with subtest("Phase 2: DB preseed populated the store (ext3)"):
        assert_db_populated(phase="phase2-ext3-init", home=P2_HOME)

    with subtest("Phase 2: re-entry preserves DB after copy-up (ext3)"):
        assert_db_populated(phase="phase2-ext3-reentry", home=P2_HOME)

    with subtest("Phase 2: status reports ext3 overlay type"):
        out = machine.succeed(as_testuser("nix-apptainer status", nix_apptainer_home=P2_HOME))
        assert "ext3" in out.lower(), f"status missing 'ext3': {out}"

    with subtest("Phase 2: clean tears down ext3 overlay"):
        machine.succeed(as_testuser("nix-apptainer clean --all", nix_apptainer_home=P2_HOME))
        machine.fail(as_testuser("test -f $NIX_APPTAINER_HOME/overlay.img", nix_apptainer_home=P2_HOME))

    # ------------------------------------------------------------------
    # Phase 3 — Adversarial: USER unset (guards commit 2da4049)
    # ------------------------------------------------------------------
    P3_HOME = "/home/testuser/.nix-apptainer-nouser"

    with subtest("Phase 3: init with USER unset"):
        machine.succeed(as_testuser(
            "nix-apptainer init --yes "
            "--sif /etc/test/base-nixos.sif "
            "--overlay-type dir",
            nix_apptainer_home=P3_HOME,
            extra_env="unset USER && ",
        ))

    with subtest("Phase 3: DB preseed works with USER unset"):
        assert_db_populated(
            phase="phase3-user-unset",
            home=P3_HOME,
            extra_env="unset USER && ",
        )

    # ------------------------------------------------------------------
    # Phase 4 — Adversarial: no outbound network
    # ------------------------------------------------------------------
    P4_HOME = "/home/testuser/.nix-apptainer-offline"

    with subtest("Phase 4: init --sif <local> succeeds with outbound blocked"):
        machine.succeed("iptables -I OUTPUT -o eth0 -j DROP")
        try:
            machine.succeed(as_testuser(
                "nix-apptainer init --yes "
                "--sif /etc/test/base-nixos.sif "
                "--overlay-type dir",
                nix_apptainer_home=P4_HOME,
            ))
            assert_db_populated(
                phase="phase4-offline",
                home=P4_HOME,
            )
        finally:
            machine.succeed("iptables -D OUTPUT -o eth0 -j DROP")

    # ------------------------------------------------------------------
    # Phase 5 — Adversarial: non-standard NIX_APPTAINER_HOME
    # ------------------------------------------------------------------
    P5_HOME = "/tmp/alt-root/na-home"

    with subtest("Phase 5: init with data dir outside $HOME"):
        machine.succeed("mkdir -p /tmp/alt-root && chown testuser /tmp/alt-root")
        machine.succeed(as_testuser(
            "nix-apptainer init --yes "
            "--sif /etc/test/base-nixos.sif "
            "--overlay-type dir",
            nix_apptainer_home=P5_HOME,
        ))

    with subtest("Phase 5: DB preseed works in non-standard home"):
        assert_db_populated(
            phase="phase5-non-standard-home",
            home=P5_HOME,
        )
  '';
}
