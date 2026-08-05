# Troubleshooting on HPC

Findings from validating the v0.6.0 release on a RHEL 9 cluster
(apptainer 1.3.2-1.el9, setuid install, network scratch filesystem).
Status: investigation in progress; ext3 overlay verdict pending.
Nothing below has been fixed in code yet — this documents symptoms,
causes, and workarounds.

## TL;DR

On clusters running apptainer ≤1.3.x, the bundled fuse-overlayfs is
version ≤1.13, which has a permission-check bug that breaks **local Nix
builds** inside the container. Downloads/substitutions work; anything
that must actually build fails with `Operation not permitted`. There is
no user-side fix for the helper binary itself.

## Symptom pattern

Three distinct EPERM failures, all one root cause (buggy fuse-overlayfs
mediating the overlay):

1. **Sandboxed build** — fails on every host tested, at varying stages:
   - `cannot change directory to "/nix/store/<drv>.chroot/root"` (fo 1.13)
   - `cannot create real-root directory: Permission denied` (fo 1.15)
   - `cannot rename: Invalid cross-device link` at the final
     output move (workstation; overlay returns EXDEV for the
     chroot-store → store rename)

   `sandbox-fallback` catches none of these — the user namespace is
   created successfully; the failures come later. Conclusion:
   **`sandbox = false` is mandatory on overlay-backed stores.**
   Workaround: `sandbox = false` in `~/.config/nix/nix.conf` *inside
   the container* (persists in the overlay).
2. **Unsandboxed build dir**: `changing into "/nix/var/nix/builds/nix-...": Operation not permitted`.
   Modern Nix builds under `/nix/var/nix/builds` (post-CVE change),
   which is on the overlay. Workaround: `build-dir = /tmp` in the same
   nix.conf (`/tmp` is a host bind mount, not on the overlay).
3. **Store writes during install phase**: `install: cannot create directory '/nix/store/<out>/bin': Operation not permitted`.
   **No known workaround** on a directory overlay — the output path
   must be on the overlay. This is the hard stop.

Why simple things still work: `nix run nixpkgs#hello` etc. are
substitutions — Nix unpacks NARs with plain `mkdir`/`open`, which the
buggy fuse-overlayfs handles. Local builds run tools (GNU `install`)
that pre-check writability with `access()`/`faccessat()` — the call
fuse-overlayfs ≤1.13 answers with EPERM on 0755 dirs you own
(containers/fuse-overlayfs#232, #374). Same overlay, different syscall.

## Why you can't swap in a fixed fuse-overlayfs

- Apptainer resolves FUSE helpers from its own
  `libexec/apptainer/bin/` **before** `$PATH`. A newer static binary
  in `~/bin` is ignored, with or without `--userns`. Confirm what's
  actually running: `ps -ef | grep fuse-overlayfs` (host) while a
  session is open.
- Check the bundled version:
  `/usr/libexec/apptainer/bin/fuse-overlayfs --version` → buggy if ≤1.13.
- FUSE mounts don't say "overlay" in `/proc/mounts` — apptainer passes
  a pre-opened `/dev/fuse` fd, so the rootfs shows as type `fuse`.

The only real fix is an admin-side apptainer upgrade (newer releases
bundle a fixed fuse-overlayfs).

## Why ext3 doesn't dodge it either (on modern apptainer)

Expected: setuid apptainer kernel-mounts ext3 + kernel overlayfs, no
FUSE. Reality: since CVE-2023-30549, the default is
`allow setuid-mount extfs = no` — user-provided ext3 images go through
fuse2fs even in setuid installs, and kernel overlayfs can't stack on
FUSE, so fuse-overlayfs returns. Verified in this cluster's
`/etc/apptainer/apptainer.conf`.

**Tested and confirmed: ext3 does not avoid failure 3.** With
`sandbox = false` + `build-dir = /tmp`, the build still dies at
`install: cannot create directory '<out>/bin': Operation not permitted`
— identical to the directory overlay. The bug is in fuse-overlayfs
1.13's permission checks, independent of what backs the upper layer.
On apptainer ≤1.3.x, local builds inside the container are impossible
under any overlay type; the only options are the sandbox-directory
mode below or an admin apptainer upgrade.

## The setup that has worked for years (no overlay at all)

A writable **sandbox directory** container sidesteps every FUSE/overlay
issue: `apptainer build --sandbox <dir> <image>` then
`apptainer shell -w <dir>`. No squashfs mount, no overlay merge, no
setuid mounts — just a directory tree with direct writes. Trade-offs:
many small files on the parallel FS (inode quota, metadata pressure),
no read-only base, updates mean re-unpacking. A SIF unpacks into a
sandbox dir without root:

```bash
apptainer build --sandbox ~/nix-apptainer-root base-nixos-*.sif
apptainer shell -w ~/nix-apptainer-root
```

Candidate future CLI mode (`--overlay-type sandbox`) — not implemented.

## `flake:nixpkgs` resolves to unstable — registry pin not effective

**Confirmed on the workstation** (and retroactively explains the
channel downloads on both HPCs): locking a flake with
`nixpkgs.url = "flake:nixpkgs"` inside the container resolved to
`https://releases.nixos.org/nixpkgs/nixpkgs-26.11pre...` (unstable via
the global registry), NOT the baked 26.05 nixpkgs. The image's
`nix.registry.nixpkgs.to` pin is not taking effect at runtime.

Consequences: the "no re-download, always version-matched" premise of
`flake:nixpkgs` in the examples is currently false; worse, it silently
locks a *mismatched* nixpkgs (e.g. home-manager 26.05 against nixpkgs
26.11pre), producing downstream build failures (fish 4.8 dropped
`create_manpage_completions.py`, which HM 26.05's fish module calls).

Diagnose in-container: `nix registry list` and
`cat /etc/nix/registry.json` — is the system entry present?

Interim workaround: delete any lock created this way, then pin
explicitly: `--override-input nixpkgs github:NixOS/nixpkgs/nixos-26.05`
(or set the explicit URL in the flake). Until the registry bug is
fixed, examples should pin explicitly.

## Stray nixpkgs-unstable download

During home-manager activation, a ~37 MiB fetch of
`channels.nixos.org/nixpkgs-unstable/nixexprs.tar.xz` appears. The
image pins the *flake registry* (`flake:nixpkgs` → baked nixpkgs) but
not the *legacy* `<nixpkgs>` lookup path, which some tools (the
home-manager CLI wrapper) still use — it falls back to the unstable
channel. Candidate image fix: `nix.nixPath = [ "nixpkgs=flake:nixpkgs" ]`.

Tested negative: `export NIX_PATH=nixpkgs=flake:nixpkgs` inside the
container did NOT stop the channel fetch. Either a `nix-path` entry in
the image's generated nix.conf takes precedence over the env var, or
the tooling hardcodes `channel:nixpkgs-unstable`. Pinpoint the actual
origin during the fix (check `nix config show | grep nix-path`
in-container and what evaluates `<nixpkgs>`) before trusting the
nixPath fix alone. The fetch is cached in the store after the first
run, so the practical cost is one-time per tarball TTL.

Related: `nix run home-manager -- ...` resolves via the global registry
to home-manager **master**. Bootstrap with the matching release
instead:

```bash
nix run github:nix-community/home-manager/release-26.05 -- switch --flake .#container --impure
```

This still fetches home-manager's own locked nixpkgs (~196 MiB). To
reuse the baked nixpkgs instead (at the cost of building the
home-manager package locally — which requires builds to work):
`--override-input nixpkgs flake:nixpkgs`.

## Hung Ctrl-C / unkillable processes

A wedged FUSE daemon leaves processes in uninterruptible sleep (`D`
state); SIGINT/SIGKILL don't bite until the FUSE request completes.
From a second shell on the same node:

```bash
ps -eo pid,stat,cmd | grep -E 'nix|apptainer|fuse' | grep -v grep
pkill -9 -u $USER -f fuse-overlayfs     # errors out pending I/O, unblocks D-state
```

Then let the apptainer session collapse and clean up. After a hard
kill mid-build, consider the overlay suspect; for test environments,
wipe and re-init.

Note: ext3 overlays are single-session — you can't enter the same
overlay from two processes (directory overlays tolerate it).

## Performance notes

Everything is slower on HPC than it looks locally:

- Compute/interactive nodes often have throttled egress — the initial
  SIF download and first `home-manager switch` (1–3 GB of
  substitutions) are dominated by this.
- The Nix store is many small files behind FUSE on a network FS —
  metadata-heavy operations (unpacking nixpkgs, activations) crawl.
  An ext3 image overlay keeps store writes inside one big file, which
  can be gentler on Lustre/GPFS-type filesystems than a directory
  overlay's million files.
- Where possible: do first-time downloads on login/DTN nodes, and put
  `NIX_APPTAINER_HOME` on node-local storage if the cluster has it.

## Kernel overlayfs (rootless) is also broken — differently

On a NixOS workstation (nixpkgs apptainer, no fuse-overlayfs on PATH →
kernel overlayfs in a user namespace), unsandboxed builds get further:
the home-manager package **built successfully**, but storing the flake
source then crashed Nix (`moveFile` abort). Chain: kernel overlayfs
returns EXDEV on rename → Nix falls back to copying → cleanup unlink
fails EPERM (userns overlayfs can't create whiteouts) → exception
during exception handling → core dump. This is the long-known reason
the project depends on fuse-overlayfs: kernel overlayfs in userns
cannot support Nix's rename/unlink patterns.

Workstation remedy (nixpkgs apptainer honors PATH — no bundled
helpers):

```bash
nix shell nixpkgs#fuse-overlayfs
# then run nix-apptainer enter from within that shell
```

(A plain `export PATH=$(nix build ...)/bin:$PATH` was tried and did
NOT work — use `nix shell`.)

## Results matrix

| | apptainer 1.3.2 / fo 1.13 | apptainer 1.4.5 / fo 1.15 | workstation / kernel overlayfs | workstation / fo ≥1.14 (nix shell) |
|---|---|---|---|---|
| sandboxed build (any overlay) | ✗ chroot chdir EPERM | ✗ real-root EACCES | ✗ EXDEV at output move | not tested |
| unsandboxed build, directory overlay | ✗ install EPERM | pending | ✗ moveFile crash (source ingest) | **✓ full home-manager activation** |
| unsandboxed build, ext3 overlay | ✗ install EPERM | not tested | — | — |

The sandbox failure is universal: Nix's build sandbox cannot work on an
overlay-backed store, kernel or FUSE, any version. The unsandboxed
failures split by merge driver: fuse-overlayfs ≤1.13 (permission bug)
and kernel overlayfs in userns (rename/unlink).

**Confirmed working configuration**: fuse-overlayfs ≥1.14 +
`sandbox = false` + explicitly pinned nixpkgs
(`--override-input nixpkgs github:NixOS/nixpkgs/nixos-26.05`, needed
because the registry pin is broken — see above). Full home-manager
activation succeeded on the workstation with this combination, on the
same nixpkgs rev the image was built from.

## Validated outcome (2026-07-31)

Workstation, directory overlay from the v0.6.0 release, nixpkgs
fuse-overlayfs on PATH (via `nix shell`), `sandbox = false`,
`--override-input nixpkgs github:NixOS/nixpkgs/nixos-26.05`:

- `home-manager switch` completed; generation activated and persisted.
- Re-entry lands in fish via the bash handoff; `gs`/`ga`/`gd`
  abbreviations work; git 2.x, direnv 2.37.1, fzf 0.72.0 on PATH.
- The example flakes were switched to explicit `nixos-26.05` pins as a
  result of the registry finding; all `flake:nixpkgs` references
  removed from examples.

Gotcha: a run that locked against unstable leaves a poisoned
`flake.lock` next to the config — delete it before re-running with the
override, or the bad nixpkgs stays locked.

## Open items

- **Pending**: rerun on apptainer 1.4.5 HPC (fo 1.15, directory
  overlay) with `sandbox = false` + the nixpkgs override — kill the
  in-flight unstable run and delete its `flake.lock` first. Expected
  to succeed; would confirm the recipe on a real cluster.
- **Pending diagnostic**: in-container `nix registry list` and
  `cat /etc/nix/registry.json` — why doesn't the image's
  `nix.registry.nixpkgs` pin take effect? Also
  `nix config show | grep nix-path` for the channel-fetch origin.
- Old HPC (apptainer 1.3.2): dead end for local builds under any
  overlay type; sandbox-directory mode or admin upgrade only.
- Candidate fixes (not implemented, need a plan):
  - Image: fix the flake registry pin (root cause of unstable
    downloads and version-mismatch failures); bake `sandbox = false`
    (mandatory on all overlay stores — three distinct failure modes
    observed); consider `build-dir = /tmp` and
    `nix.nixPath = [ "nixpkgs=flake:nixpkgs" ]`.
  - `checks.rs`: detect bundled fuse-overlayfs ≤1.13 (look in
    apptainer's `libexec/apptainer/bin`, not PATH) and warn with
    remedies (admin upgrade / sandbox-dir mode); on nixpkgs apptainer,
    detect missing fuse-overlayfs on PATH (kernel-overlayfs trap).
  - CLI: sandbox-directory mode (`apptainer build --sandbox` + `shell -w`)
    for clusters where the FUSE stack is broken.
  - Docs: README known-issues pointer to this file.
  - Upstream: Nix `moveFile` abort (exception during exception
    handling) is reportable; nixpkgs apptainer missing fuse-overlayfs
    in `defaultPathInputs`.
- If the examples' explicit pins ever revert to `flake:nixpkgs`, that
  requires the registry fix to land in a release first.

## `rm -rf` on the sandbox directory fails with "Permission denied"

Expected. Nix marks store paths it builds read-only (mode 555), and
read-only directories cannot have entries removed. Use `nix-apptainer
clean` (which makes the tree writable first), or:

    chmod -R u+w ~/.local/share/nix-apptainer/sandbox
    rm -rf ~/.local/share/nix-apptainer/sandbox

## "failed to get key material: 404" during sandbox unpack (pre-0.7.1)

Versions before 0.7.1 did not import the release signing key, so
`apptainer build --sandbox` fell back to a keyserver lookup and warned:

    WARNING: failed to get key material: 404 Not Found
    WARNING: Bootstrap image could not be verified, but build will continue.

Harmless: `init` verifies the download's SHA256 before unpacking. 0.7.1
imports the key so verification succeeds and the warnings disappear.

## Builds fail with "No such file or directory" creating a temp dir

`nix-build` creates a scratch directory under `$TMPDIR` before any build
starts, and `build-dir` cannot rescue it. If the host `$TMPDIR` names a
path that is not visible inside the container — common on clusters where
it points at per-job scratch — every build dies immediately.

Fix by either pinning a container-side path or binding the host one:

    [enter]
    tmpdir = "/tmp"

    # or, to keep using host scratch:
    tmpdir = "/scratch/$USER/tmp"
    bind = ["/scratch:/scratch"]
    mount_points = ["/scratch"]     # sandbox mode only

In sandbox mode apptainer cannot create a missing mount point, so the
`mount_points` entry is what makes the bind succeed rather than be
silently skipped.

## Site data (`/datastore`, `/share`, ...) is missing in the container

Two different causes, in every storage mode:

**1. Isolation (the usual one).** The container runs with
`--no-mount hostfs,home,cwd`, so filesystems the site would mount
automatically are deliberately absent. Opt back in explicitly:

    [enter]
    bind = ["/datastore:/datastore"]

`nix-apptainer init` discovers the candidates and offers them — re-run it to
see the list again.

**2. Missing mount point (sandbox mode only).** Apptainer cannot fabricate
mount points in a `--writable` sandbox, so a bind whose destination does not
exist is skipped with:

    WARNING: Skipping mount /datastore [binds]: /datastore doesn't exist in container

Destinations of `bind`/`mount` entries are seeded automatically. For `bind
path` entries in the site's own `apptainer.conf`, list them yourself — they
are recreated before every launch:

    [enter]
    mount_points = ["/datastore", "/share"]

## My home directory is visible inside the container

Sites that set `mount hostfs = yes` mount every host filesystem, including
your home. That is a *separate* mechanism from `mount home = yes`, so
`--no-home` cannot suppress it — and neither can `--no-mount home` alone.
Both must be named together, which is what nix-apptainer does:

    apptainer exec --no-mount hostfs,home ...

Check what your site sets with:

    grep -E '^\s*mount (hostfs|home)' \
      "$(apptainer buildcfg | sed -n 's/^APPTAINER_CONFDIR=//p')/apptainer.conf"

## `nix store optimise` grows the overlay in dir/ext3 modes

Run `nix store optimise` in **sandbox mode only**. In `dir`/`ext3`
modes the baked store lives in the read-only lower layer, and
hardlinking a lower-layer file forces fuse-overlayfs to copy it up —
so optimise *adds* an upper-layer copy of every deduplicated file
instead of freeing space, and can fill a fixed-size ext3 image. In
sandbox mode the store is a plain directory and optimise works
normally (the baked closure ships with canonical 444/555 file modes
for exactly this reason).
