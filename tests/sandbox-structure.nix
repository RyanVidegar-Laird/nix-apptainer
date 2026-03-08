# tests/sandbox-structure.nix
#
# Tier 1c: Builds the sandbox and asserts expected paths, symlinks, and permissions.
{ runCommand, jq, sandbox }:

runCommand "nix-apptainer-test-sandbox"
  {
    nativeBuildInputs = [ jq ];
  }
  ''
    sb="${sandbox}"

    fail() { echo "FAIL: $1"; exit 1; }
    pass() { echo "  - $1"; }

    echo "Checking sandbox structure..."

    # --- Directories that must exist ---
    for dir in \
      nix/store \
      nix/store/.links \
      nix/var/nix/db \
      bin \
      usr/bin \
      home/nixuser \
      tmp \
      var/tmp \
      root \
      .singularity.d \
      .singularity.d/env \
      usr/local/bin
    do
      [ -d "$sb/$dir" ] || fail "directory $dir missing"
      pass "directory $dir exists"
    done

    # --- Symlinks that must exist and point into /nix/store ---
    for link in \
      bin/sh \
      usr/bin/env \
      run/current-system \
      run/sw \
      usr/local/bin/nix
    do
      [ -L "$sb/$link" ] || fail "$link is not a symlink"
      target=$(readlink "$sb/$link")
      # Verify target points into /nix/store (structural check).
      # We can't always resolve the target in the build sandbox because
      # some targets (e.g. the nix binary) aren't in the NixOS closure.
      echo "$target" | grep -q "^/nix/store/" || fail "$link -> $target does not point into /nix/store"
      pass "symlink $link -> $target"
    done

    # --- Files that must exist ---
    [ -f "$sb/.singularity.d/runscript" ] || fail ".singularity.d/runscript missing"
    [ -x "$sb/.singularity.d/runscript" ] || fail ".singularity.d/runscript not executable"
    pass ".singularity.d/runscript exists and is executable"

    [ -f "$sb/.singularity.d/env/90-environment.sh" ] || fail "90-environment.sh missing"
    pass ".singularity.d/env/90-environment.sh exists"

    # labels.json must be valid JSON
    [ -f "$sb/.singularity.d/labels.json" ] || fail "labels.json missing"
    jq empty "$sb/.singularity.d/labels.json" || fail "labels.json is not valid JSON"
    pass ".singularity.d/labels.json is valid JSON"

    # nix-path-registration must exist and be non-empty
    [ -f "$sb/nix-path-registration" ] || fail "nix-path-registration missing"
    [ -s "$sb/nix-path-registration" ] || fail "nix-path-registration is empty"
    pass "nix-path-registration exists and is non-empty"

    # /etc/nix/nix.conf must exist (may be a symlink into /nix/store whose
    # target is not accessible in the build sandbox — check with -L not -e)
    [ -e "$sb/etc/nix/nix.conf" ] || [ -L "$sb/etc/nix/nix.conf" ] || fail "etc/nix/nix.conf missing"
    pass "etc/nix/nix.conf exists"

    # /etc/hostname — NixOS generates this as a symlink into /nix/store
    [ -e "$sb/etc/hostname" ] || [ -L "$sb/etc/hostname" ] || fail "etc/hostname missing"
    # Read the symlink target to check content (the target is a store path with just the hostname)
    if [ -e "$sb/etc/hostname" ]; then
      grep -q "nix-apptainer" "$sb/etc/hostname" || fail "etc/hostname does not contain nix-apptainer"
      pass "etc/hostname contains nix-apptainer"
    else
      # It's a dangling symlink — verify its target path looks right
      target=$(readlink "$sb/etc/hostname")
      echo "$target" | grep -q "/nix/store/" || fail "etc/hostname symlink does not point to /nix/store"
      pass "etc/hostname is a symlink to $target (content not verifiable in build sandbox)"
    fi

    # --- Nix store has contents ---
    # Avoid broken pipe from ls|head under set -e by using a simple test
    store_count=$(ls -1 "$sb/nix/store" | wc -l)
    [ "$store_count" -gt 0 ] || fail "nix/store is empty"
    pass "nix/store contains store paths ($store_count entries)"

    echo ""
    echo "All sandbox structure checks passed."
    touch $out
  ''
