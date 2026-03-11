# lib/build-sif.nix
#
# Converts a sandbox directory into an Apptainer .sif image.
# Assembles a rootfs staging directory, packs it with mksquashfs,
# then wraps it in SIF format using apptainer sif commands.
{
  runCommand,
  squashfsTools,
  apptainer,
  lib,
  coreutils,
  stdenv,
}:

{
  sandbox,
  name ? "nix-apptainer",
  comp ? "gzip",
}:

let
  # Map Nix system arch to SIF partarch values
  # 2 = amd64, 4 = arm64
  sifPartArch =
    if stdenv.hostPlatform.isx86_64 then 2
    else if stdenv.hostPlatform.isAarch64 then 4
    else throw "Unsupported architecture for SIF: ${stdenv.hostPlatform.system}";

  # Build the squashfs from the sandbox rootfs.
  # Start with a staging directory and copy sandbox contents into it
  # so that the rootfs sits at the squashfs filesystem root.
  squashfs =
    runCommand "${name}-squashfs"
      {
        nativeBuildInputs = [
          squashfsTools
          coreutils
        ];
        __structuredAttrs = true;
        unsafeDiscardReferences.out = true;
      }
      ''
        # Stage sandbox contents so mksquashfs places the rootfs at the
        # squashfs root. When mksquashfs receives a single directory, it
        # unwraps it and makes the directory's contents the fs root.
        mkdir rootfs
        cp -a ${sandbox}/. rootfs/

        # Make directories writable so fuse-overlayfs can create upper-layer
        # entries. Without this, the overlay can't write to /nix/var, /nix/store,
        # etc. because nix store outputs are read-only (mode 555).
        #
        # Security note: this makes /nix/store writable in the overlay layer,
        # matching the trust model of single-user Nix (no daemon, user owns
        # the store). The base squashfs remains immutable. Nix's content-
        # addressing and signature verification still protect against
        # substituter-level tampering. A user could modify their own overlay's
        # store paths, but only affects their own environment.
        chmod -R u+w rootfs/nix/var rootfs/nix/store rootfs/home \
          rootfs/tmp rootfs/var rootfs/root rootfs/etc

        # Some fuse-overlayfs versions report EPERM from access(path, W_OK)
        # on 755 dirs even when owned by the caller. Nix checks this on
        # /nix/var/nix/db, so make these dirs world-writable in the squashfs.
        chmod -R 777 rootfs/nix/var/nix

        mksquashfs rootfs $out \
          -no-hardlinks \
          -all-root \
          -b 1048576 \
          -root-mode 0755 \
          -comp ${comp} \
          -processors $NIX_BUILD_CORES \
          -noappend
      '';
in
runCommand "${name}.sif"
  {
    nativeBuildInputs = [ apptainer ];
    __structuredAttrs = true;
    unsafeDiscardReferences.out = true;
  }
  ''
    export APPTAINER_TMPDIR=$(mktemp -d)
    export APPTAINER_CACHEDIR=$(mktemp -d)
    export HOME=$(mktemp -d)

    # Create empty SIF container
    apptainer sif new "$out"

    # Add the squashfs as a primary system partition
    # --datatype 4 = Partition data
    # --parttype 2 = System partition (PrimSys)
    # --partfs 1 = Squash filesystem
    # --partarch: 2 = amd64, 4 = arm64
    # --groupid 1: required for apptainer sign/verify
    apptainer sif add \
      --datatype 4 \
      --parttype 2 \
      --partfs 1 \
      --partarch ${toString sifPartArch} \
      --groupid 1 \
      "$out" \
      ${squashfs}
  ''
