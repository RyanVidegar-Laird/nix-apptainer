# tests/eval.nix
#
# Tier 1a: Asserts properties of the evaluated NixOS configuration.
# Runs as a pure Nix evaluation — no container runtime needed.
{ runCommand, nixosConfig }:

let
  cfg = nixosConfig.config;

  assertions = [
    {
      check = cfg.boot.isContainer == true;
      msg = "boot.isContainer must be true";
    }
    {
      check = cfg.nix.settings.sandbox == true;
      msg = "nix.settings.sandbox must be true";
    }
    {
      check = cfg.nix.settings.sandbox-fallback == true;
      msg = "nix.settings.sandbox-fallback must be true";
    }
    {
      check = builtins.elem "nix-command" cfg.nix.settings.experimental-features;
      msg = "experimental-features must include nix-command";
    }
    {
      check = builtins.elem "flakes" cfg.nix.settings.experimental-features;
      msg = "experimental-features must include flakes";
    }
    {
      check = cfg.networking.hostName == "nix-apptainer";
      msg = "hostname must be nix-apptainer";
    }
    {
      check = cfg.system.build.toplevel != null;
      msg = "system.build.toplevel must exist";
    }
  ];

  # Check that expected packages are in systemPackages by name
  packageNames = map (p: p.pname or p.name or "") cfg.environment.systemPackages;
  expectedPackages = [
    "git"
    "curl"
    "coreutils"
  ];
  packageChecks = map (name: {
    check = builtins.any (p: p == name) packageNames;
    msg = "environment.systemPackages must include ${name}";
  }) expectedPackages;

  allAssertions = assertions ++ packageChecks;

  # Evaluate all assertions — fails at eval time if any are false
  failedAssertions = builtins.filter (a: !a.check) allAssertions;
  assertionErrors = builtins.map (a: a.msg) failedAssertions;
in

if failedAssertions != [ ] then
  builtins.throw "NixOS config assertions failed:\n  ${builtins.concatStringsSep "\n  " assertionErrors}"
else
  runCommand "nix-apptainer-test-eval" { } ''
    echo "All NixOS config assertions passed:"
    ${builtins.concatStringsSep "\n" (map (a: "echo '  - ${a.msg}'") allAssertions)}
    touch $out
  ''
