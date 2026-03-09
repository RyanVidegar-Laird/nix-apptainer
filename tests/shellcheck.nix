# tests/shellcheck.nix
#
# Tier 1b: Runs shellcheck on all shell scripts.
{ runCommand, shellcheck, scripts }:

runCommand "nix-apptainer-test-shellcheck"
  {
    nativeBuildInputs = [ shellcheck ];
  }
  ''
    echo "Running shellcheck on scripts..."
    shellcheck --severity=warning \
      ${scripts}/setup.sh \
      ${scripts}/enter.sh \
      ${scripts}/entrypoint.sh \
      ${scripts}/sign-release.sh
    echo "All scripts passed shellcheck."
    touch $out
  ''
