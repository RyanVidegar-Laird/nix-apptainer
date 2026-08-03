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
  util-linux,
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
    if stdenv.hostPlatform.isx86_64 then
      2
    else if stdenv.hostPlatform.isAarch64 then
      4
    else
      throw "Unsupported architecture for SIF: ${stdenv.hostPlatform.system}";

  # Build the squashfs from the sandbox rootfs.
  # Start with a staging directory and copy sandbox contents into it
  # so that the rootfs sits at the squashfs filesystem root.
  squashfs =
    runCommand "${name}-squashfs"
      {
        nativeBuildInputs = [
          squashfsTools
          coreutils
          util-linux
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

        # Make the whole tree owner-writable. Nix store outputs are read-only
        # (mode 555), which breaks both runtime modes:
        #   - overlay modes: fuse-overlayfs can't create upper-layer entries
        #   - sandbox mode: `apptainer build --sandbox` writes into the
        #     unpacked tree (.singularity.d/actions, env/*.sh, runscript) and
        #     `--writable` sessions write anywhere
        # Enumerating paths here encodes an overlay-mode assumption — that only
        # the paths we anticipate need writes. `--writable` inverts that: the
        # runtime owns the whole tree.
        #
        # Security note: this makes /nix/store writable, matching the trust
        # model of single-user Nix (no daemon, user owns the store). The base
        # squashfs remains immutable in overlay modes. Nix's content-addressing
        # and signature verification still protect against substituter-level
        # tampering. A user could modify their own store paths, but that only
        # affects their own environment.
        chmod -R u+w rootfs

        # Some fuse-overlayfs versions report EPERM from access(path, W_OK)
        # on 755 dirs even when owned by the caller. Nix checks this on
        # /nix/var/nix/db, so make these dirs world-writable in the squashfs.
        chmod -R 777 rootfs/nix/var/nix

        # Hardlink identical store files so a sandbox-mode unpack
        # (unsquashfs) recreates them as links instead of full copies.
        # util-linux hardlink only merges files with equal content, mode,
        # owner, and mtime — safe for a store tree (mtimes are all epoch).
        hardlink rootfs/nix/store

        mksquashfs rootfs $out \
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
