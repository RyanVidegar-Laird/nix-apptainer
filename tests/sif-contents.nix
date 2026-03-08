# tests/sif-contents.nix
#
# Tier 2: Builds the SIF image, extracts squashfs, verifies layout.
# Catches mksquashfs staging bugs (paths nested under build dirs).
{ runCommand, squashfsTools, apptainer, sifImage }:

runCommand "nix-apptainer-test-sif"
  {
    nativeBuildInputs = [
      squashfsTools
      apptainer
    ];
    __structuredAttrs = true;
  }
  ''
    export APPTAINER_TMPDIR=$(mktemp -d)
    export APPTAINER_CACHEDIR=$(mktemp -d)
    export HOME=$(mktemp -d)

    fail() { echo "FAIL: $1"; exit 1; }
    pass() { echo "  - $1"; }

    sif="${sifImage}"

    echo "Verifying SIF image structure..."

    # Extract squashfs partition from SIF
    apptainer sif list "$sif" | head -5
    apptainer sif dump 1 "$sif" > test.sqfs

    # Extract squashfs contents (read-only, to a temp dir)
    unsquashfs -d extracted test.sqfs

    echo ""
    echo "Checking squashfs root layout..."

    # --- Paths must be at squashfs root, NOT nested under build paths ---
    # This is the main regression test: mksquashfs flags can cause
    # paths like extracted/build/rootfs/bin instead of extracted/bin
    for path in \
      bin/sh \
      etc/hostname \
      .singularity.d/runscript \
      usr/local/bin/nix \
      nix-path-registration
    do
      # Use -e or -L: some paths are symlinks into /nix/store whose targets
      # are dangling in the extracted squashfs (the store paths are there but
      # may not be resolvable as absolute paths from the build sandbox)
      [ -e "extracted/$path" ] || [ -L "extracted/$path" ] \
        || fail "$path not at squashfs root (layout bug)"
      pass "$path at squashfs root"
    done

    # nix/store must contain entries
    store_count=$(ls -1 extracted/nix/store/ | wc -l)
    [ "$store_count" -gt 0 ] || fail "nix/store is empty in squashfs"
    pass "nix/store has entries ($store_count)"

    # --- Verify writable permissions on overlay-targeted dirs ---
    echo ""
    echo "Checking overlay-writable permissions..."
    for dir in nix/store nix/var home tmp var root etc; do
      if [ -d "extracted/$dir" ]; then
        # Check owner has write permission
        perms=$(stat -c '%a' "extracted/$dir")
        # The first digit (owner) must have write (2) set
        owner_perm=$((perms / 100))
        if [ $((owner_perm & 2)) -eq 0 ]; then
          fail "$dir is not owner-writable (mode $perms)"
        fi
        pass "$dir is owner-writable (mode $perms)"
      fi
    done

    echo ""
    echo "All SIF content checks passed."
    touch $out
  ''
