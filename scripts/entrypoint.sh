#!/usr/bin/env bash
# entrypoint.sh — Apptainer container entrypoint
# Handles first-run Nix store DB initialization and shell setup.

set -euo pipefail

# Clear NixOS profile guards leaked from the host so /etc/profile
# re-sources set-environment (which adds $HOME/.nix-profile/bin to PATH).
# Apptainer propagates host env into the container; these exported guards
# cause the container's /etc/profile to skip its own PATH setup.
unset __NIXOS_SET_ENVIRONMENT_DONE __ETC_PROFILE_DONE __ETC_BASHRC_SOURCED

# Ensure home directory exists (overlay may not have it on first use)
mkdir -p "$HOME" 2>/dev/null || true

# Ensure PATH includes nix and system tools (needed for the TERM check;
# the login shell will get full PATH from /etc/profile)
export PATH="/usr/local/bin:/run/sw/bin:/bin:/usr/bin:${PATH:-}"
export NIX_REMOTE=""

# Fall back to xterm-256color if TERM is unset or unrecognized
if [ -z "${TERM:-}" ] || ! infocmp "$TERM" >/dev/null 2>&1; then
    export TERM=xterm-256color
fi

# Restore 777 on /nix/var/nix — fuse-overlayfs access() needs world-writable
# mode bits because it doesn't check ownership (see README "Known issues").
chmod -R 777 /nix/var/nix 2>/dev/null || true

# Warn if Nix build sandbox is unavailable (user namespaces not supported)
if [ -z "${NIX_APPTAINER_NO_SANDBOX_WARN:-}" ] && [ ! -f /run/.nix-apptainer-sandbox-checked ]; then
    if ! unshare -U true 2>/dev/null; then
        echo "Warning: Nix build sandbox unavailable (user namespaces not supported on this host). Builds will run unsandboxed." >&2
    fi
    touch /run/.nix-apptainer-sandbox-checked 2>/dev/null || true
fi

# --- Execute command or interactive shell ---
if [ $# -gt 0 ]; then
    exec "$@"
else
    exec /bin/bash --login
fi
