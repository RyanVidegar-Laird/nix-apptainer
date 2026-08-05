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
        outputs = [
          "out"
          "size"
        ];
      }
      ''
        # Stage sandbox contents so mksquashfs places the rootfs at the
        # squashfs root. When mksquashfs receives a single directory, it
        # unwraps it and makes the directory's contents the fs root.
        mkdir rootfs
        # --no-preserve=links: if the build host's store is optimised,
        # identical files (e.g. every empty file) share one inode across
        # the whole sandbox — store closure and /nix/var/nix seed alike.
        # Preserving that structure lets the per-tree chmods below bleed
        # into each other through shared inodes (and makes the result
        # depend on the builder's store state). Stage unlinked; the
        # `hardlink` step below re-links within nix/store only.
        cp -a --no-preserve=links ${sandbox}/. rootfs/

        # Permission invariant: DIRECTORIES owner-writable, store FILES
        # canonical (444/555).
        #
        # Dirs need u+w so fuse-overlayfs can create upper-layer entries
        # (overlay modes) and so `apptainer build --sandbox` unpacks and
        # `--writable` sessions can create/delete entries anywhere
        # (sandbox mode). Non-store files (.singularity.d, /etc) keep
        # u+w too — apptainer's unpack overwrites some of them in place.
        #
        # Store files must NOT be owner-writable: Nix's optimiser skips
        # any S_IWUSR file as "suspicious", so a writable baked closure
        # makes `nix store optimise` warn per-file and never deduplicate
        # it. cp -a preserved the store's 444/555 modes; u+w then u-w on
        # files restores exactly those. New store paths built inside the
        # container are canonicalised by Nix itself. (In overlay modes
        # optimise copies lower files up and grows the overlay — see
        # docs/troubleshooting.md; sandbox mode is where it works.)
        #
        # Security note: writable dirs match the trust model of
        # single-user Nix (no daemon, user owns the store). The base
        # squashfs remains immutable in overlay modes.
        chmod -R u+w rootfs
        find rootfs/nix/store -type f -exec chmod u-w {} +

        # Some fuse-overlayfs versions report EPERM from access(path, W_OK)
        # on 755 dirs even when owned by the caller. Nix checks this on
        # /nix/var/nix/db, so make these dirs world-writable in the squashfs.
        chmod -R 777 rootfs/nix/var/nix

        # Hardlink identical store files so a sandbox-mode unpack
        # (unsquashfs) recreates them as links instead of full copies.
        # util-linux hardlink only merges files with equal content, mode,
        # owner, and mtime — safe for a store tree (mtimes are all epoch).
        hardlink rootfs/nix/store

        # On-disk bytes of the staged tree (post-hardlink, so duplicates
        # count once — matching what a sandbox-mode unsquashfs writes).
        # Consumed by the CLI's unpack progress bar via SIF metadata.
        du -sB1 rootfs | cut -f1 > "$size"

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

    # Embed the unpacked size so the CLI can render a real progress bar
    # during sandbox unpack. --datatype 6 = GenericJSON; --groupid 1 keeps
    # it inside the signed object group (signing happens in release CI).
    printf '{"unpacked_bytes": %s}' "$(cat ${squashfs.size})" > meta.json
    apptainer sif add \
      --datatype 6 \
      --groupid 1 \
      "$out" \
      meta.json
  ''
